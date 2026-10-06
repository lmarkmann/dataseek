//! The disk cache: per-query results, whole catalogs for the sources that are
//! searched locally, and short-lived marks for sources that just failed.
//!
//! One JSON file per entry under the cache directory, written atomically, so
//! parallel source threads never share a file or a lock. Fresh entries are
//! served without a request; expired ones are kept as a fallback for when the
//! source is down. The whole directory stays under [`BUDGET_BYTES`] and
//! [`BUDGET_FILES`]: [`Cache::trim`] evicts the least recently written
//! entries first and runs once at the end of every search, never per write.
//! Everything here is best effort: a cache that cannot be read or written
//! degrades to fetching, it never fails a search.

use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};

use crate::fs::write_atomic;

pub const BUDGET_BYTES: u64 = 30 * 1024 * 1024;
pub const BUDGET_FILES: usize = 2000;

/// How long a query's results are served without asking the source again.
pub const QUERY_TTL: Duration = Duration::from_hours(6);
/// How long a downloaded catalog (UCI, SDMX dataflows, STAC collections) is
/// searched locally before it is fetched again.
pub const CATALOG_TTL: Duration = Duration::from_hours(7 * 24);
/// How long a source that just had an outage is skipped.
pub const OUTAGE_TTL: Duration = Duration::from_mins(10);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Query,
    Catalog,
    Outage,
}

impl Kind {
    fn dir(self) -> &'static str {
        match self {
            Self::Query => "queries",
            Self::Catalog => "catalogs",
            Self::Outage => "outages",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Freshness {
    Fresh,
    Stale,
}

#[derive(Serialize, Deserialize)]
struct Entry<T> {
    stored: u64,
    value: T,
}

pub struct Cache {
    root: PathBuf,
}

pub struct Usage {
    pub files: usize,
    pub bytes: u64,
}

impl Cache {
    pub fn new(root: PathBuf) -> Self {
        Self { root }
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    fn path(&self, kind: Kind, key: &str) -> PathBuf {
        self.root.join(kind.dir()).join(format!("{key}.json"))
    }

    /// The entry and whether it is still within `ttl`. Unreadable or
    /// malformed entries read as absent. The file is checked as UTF-8 once
    /// and parsed as a `str`, which spares serde_json checking every string
    /// in a catalog of thousands on its own.
    pub fn load<T: DeserializeOwned>(
        &self,
        kind: Kind,
        key: &str,
        ttl: Duration,
    ) -> Option<(T, Freshness)> {
        let json = std::fs::read_to_string(self.path(kind, key)).ok()?;
        let entry: Entry<T> = serde_json::from_str(&json).ok()?;
        let age = now().saturating_sub(entry.stored);
        let freshness = if age < ttl.as_secs() {
            Freshness::Fresh
        } else {
            Freshness::Stale
        };
        Some((entry.value, freshness))
    }

    pub fn store<T: Serialize>(&self, kind: Kind, key: &str, value: &T) {
        self.write(kind, key, now(), value);
    }

    /// An entry written at the epoch, long past every TTL.
    #[cfg(test)]
    pub fn store_expired<T: Serialize>(
        &self,
        kind: Kind,
        key: &str,
        value: &T,
    ) {
        self.write(kind, key, 0, value);
    }

    fn write<T: Serialize>(
        &self,
        kind: Kind,
        key: &str,
        stored: u64,
        value: &T,
    ) {
        let entry = Entry { stored, value };
        if let Ok(bytes) = serde_json::to_vec(&entry) {
            let _ = write_atomic(&self.path(kind, key), &bytes);
        }
    }

    pub fn mark_outage(&self, source: &str) {
        self.store(Kind::Outage, source, &());
    }

    /// How long ago the source last had an outage, if within [`OUTAGE_TTL`].
    pub fn recent_outage(&self, source: &str) -> Option<Duration> {
        let bytes = std::fs::read(self.path(Kind::Outage, source)).ok()?;
        let entry: Entry<()> = serde_json::from_slice(&bytes).ok()?;
        let age = now().saturating_sub(entry.stored);
        (age < OUTAGE_TTL.as_secs()).then(|| Duration::from_secs(age))
    }

    pub fn clear_outage(&self, source: &str) {
        let _ = std::fs::remove_file(self.path(Kind::Outage, source));
    }

    pub fn usage(&self) -> Usage {
        let files = self.files();
        Usage {
            files: files.len(),
            bytes: files.iter().map(|f| f.bytes).sum(),
        }
    }

    /// Evict the oldest entries until the directory fits the budget.
    pub fn trim(&self) -> Usage {
        trim_files(self.files(), BUDGET_BYTES, BUDGET_FILES)
    }

    pub fn clear(&self) -> std::io::Result<()> {
        match std::fs::remove_dir_all(&self.root) {
            Err(e) if e.kind() != std::io::ErrorKind::NotFound => Err(e),
            _ => Ok(()),
        }
    }

    fn files(&self) -> Vec<CachedFile> {
        [Kind::Query, Kind::Catalog, Kind::Outage]
            .iter()
            .filter_map(|kind| {
                std::fs::read_dir(self.root.join(kind.dir())).ok()
            })
            .flatten()
            .filter_map(Result::ok)
            .filter_map(|entry| {
                let meta = entry.metadata().ok()?;
                meta.is_file().then(|| CachedFile {
                    path: entry.path(),
                    bytes: meta.len(),
                    modified: meta.modified().unwrap_or(UNIX_EPOCH),
                })
            })
            .collect()
    }
}

struct CachedFile {
    path: PathBuf,
    bytes: u64,
    modified: SystemTime,
}

fn trim_files(
    mut files: Vec<CachedFile>,
    max_bytes: u64,
    max_files: usize,
) -> Usage {
    files.sort_by_key(|f| f.modified);
    let mut bytes: u64 = files.iter().map(|f| f.bytes).sum();
    let mut count = files.len();
    for file in &files {
        if bytes <= max_bytes && count <= max_files {
            break;
        }
        if std::fs::remove_file(&file.path).is_ok() {
            bytes = bytes.saturating_sub(file.bytes);
            count = count.saturating_sub(1);
        }
    }
    Usage { files: count, bytes }
}

/// A short, stable file name for a query. FNV-1a, not for security: it only
/// has to be the same on every run and spread keys over the alphabet.
pub fn query_key(source: &str, query: &str, limit: usize) -> String {
    let normalized = query.split_whitespace().collect::<Vec<_>>().join(" ");
    let material =
        format!("{source}\u{1f}{}\u{1f}{limit}", normalized.to_lowercase());
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for byte in material.bytes() {
        hash ^= u64::from(byte);
        hash = hash.wrapping_mul(0x0100_0000_01b3);
    }
    format!("{source}-{hash:016x}")
}

fn now() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| d.as_secs())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::record::Dataset;

    #[test]
    fn entries_round_trip_and_age_into_stale() {
        let dir = tempfile::tempdir().unwrap();
        let cache = Cache::new(dir.path().to_path_buf());
        cache.store(Kind::Query, "k", &vec![1, 2, 3]);
        let (value, fresh): (Vec<i32>, _) =
            cache.load(Kind::Query, "k", QUERY_TTL).unwrap();
        assert_eq!(value, vec![1, 2, 3]);
        assert_eq!(fresh, Freshness::Fresh);
        let (_, stale): (Vec<i32>, _) =
            cache.load(Kind::Query, "k", Duration::ZERO).unwrap();
        assert_eq!(stale, Freshness::Stale);
        assert!(
            cache.load::<Vec<i32>>(Kind::Catalog, "k", QUERY_TTL).is_none()
        );
    }

    #[test]
    fn a_cached_catalog_keeps_every_field_of_its_records() {
        let dir = tempfile::tempdir().unwrap();
        let cache = Cache::new(dir.path().to_path_buf());
        let full = Dataset {
            title: "Sea ice extent".into(),
            url: "https://x.org/ice".into(),
            description: Some("Daily".into()),
            publisher: Some("NSIDC".into()),
            doi: Some("10.5067/x".into()),
            license: Some("CC0".into()),
            updated: Some("2026-01-02".into()),
            size_bytes: Some(7),
            popularity: Some(3),
            aliases: vec!["10.5067/concept".into()],
        };
        let bare = Dataset::new("Bare", "https://x.org/bare");
        let stored = vec![full, bare];
        cache.store(Kind::Catalog, "nsidc", &stored);
        let (loaded, _): (Vec<Dataset>, _) =
            cache.load(Kind::Catalog, "nsidc", CATALOG_TTL).unwrap();
        assert_eq!(loaded, stored);
    }

    #[test]
    fn outage_marks_expire_and_clear() {
        let dir = tempfile::tempdir().unwrap();
        let cache = Cache::new(dir.path().to_path_buf());
        assert!(cache.recent_outage("zenodo").is_none());
        cache.mark_outage("zenodo");
        assert!(cache.recent_outage("zenodo").is_some());
        cache.clear_outage("zenodo");
        assert!(cache.recent_outage("zenodo").is_none());
    }

    #[test]
    fn trimming_evicts_oldest_first_until_within_budget() {
        let dir = tempfile::tempdir().unwrap();
        let mut files = Vec::new();
        for (i, age) in [30_u64, 20, 10].iter().enumerate() {
            let path = dir.path().join(format!("{i}.json"));
            std::fs::write(&path, vec![b'x'; 100]).unwrap();
            files.push(CachedFile {
                path,
                bytes: 100,
                modified: UNIX_EPOCH + Duration::from_secs(1000 - age),
            });
        }
        let usage = trim_files(files, 200, 10);
        assert_eq!(usage.files, 2);
        assert!(!dir.path().join("0.json").exists(), "oldest survived");
        assert!(dir.path().join("2.json").exists(), "newest evicted");
    }

    #[test]
    fn query_keys_ignore_case_and_spacing_but_not_limit() {
        assert_eq!(
            query_key("hf", "Climate  Data", 10),
            query_key("hf", "climate data", 10)
        );
        assert_ne!(
            query_key("hf", "climate", 10),
            query_key("hf", "climate", 20)
        );
        assert_ne!(
            query_key("hf", "climate", 10),
            query_key("kg", "climate", 10)
        );
    }
}
