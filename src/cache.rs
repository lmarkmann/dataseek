//! The disk cache: per-query results, whole catalogs for the sources that are
//! searched locally, short-lived marks for sources that just failed and for
//! catalogs a background download holds, and why a catalog's last download
//! failed.
//!
//! One JSON file per entry under the cache directory, written atomically, so
//! parallel source threads never share a file or a lock. Fresh entries are
//! served without a request; expired ones are kept as a fallback for when the
//! source is down. The whole directory stays under [`budget_bytes`] and
//! [`BUDGET_FILES`]: [`Cache::trim`] evicts query results first, catalogs
//! next and the marks last, the least recently written first within each,
//! and runs once at the end of every search, never per write. A malformed
//! entry is removed when read.
//! Everything here is best effort: a cache that cannot be read or written
//! degrades to fetching, it never fails a search.

use std::io::Write as _;
use std::ops::RangeInclusive;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};

use crate::fs::write_atomic;

/// The size budget unless [`BUDGET_VAR`] sets another, in decimal megabytes
/// like every size dataseek prints.
pub const BUDGET_BYTES: u64 = 30 * 1000 * 1000;
pub const BUDGET_FILES: usize = 2000;
/// Replaces the size budget, in whole megabytes within [`BUDGET_MB`].
pub const BUDGET_VAR: &str = "DATASEEK_CACHE_MAX_MB";
pub const BUDGET_MB: RangeInclusive<u64> = 1..=10_000;

static BUDGET: OnceLock<u64> = OnceLock::new();

/// Read [`BUDGET_VAR`] once, at startup, so a bad value is a usage error
/// before any work starts rather than a cache that quietly ignores it.
pub fn read_budget() -> Result<(), String> {
    let Some(raw) = std::env::var_os(BUDGET_VAR) else { return Ok(()) };
    let mb = raw
        .to_str()
        .and_then(|text| text.trim().parse::<u64>().ok())
        .filter(|mb| BUDGET_MB.contains(mb))
        .ok_or_else(|| {
            format!(
                "invalid value '{}' for {BUDGET_VAR}: whole megabytes from \
                 {} to {}\n\ntip: set it to a number such as 100, or unset it \
                 for the 30 MB default\n",
                raw.to_string_lossy(),
                BUDGET_MB.start(),
                BUDGET_MB.end()
            )
        })?;
    let _ = BUDGET.set(mb.saturating_mul(1000 * 1000));
    Ok(())
}

/// The size budget in effect: [`BUDGET_VAR`] when set, else
/// [`BUDGET_BYTES`].
pub fn budget_bytes() -> u64 {
    BUDGET.get().copied().unwrap_or(BUDGET_BYTES)
}

/// Whether [`BUDGET_VAR`] set the budget.
pub fn budget_from_env() -> bool {
    BUDGET.get().is_some()
}

/// How long a query's results are served without asking the source again.
pub const QUERY_TTL: Duration = Duration::from_hours(6);
/// How long a downloaded catalog (UCI, SDMX dataflows, STAC collections) is
/// searched locally before it is fetched again.
pub const CATALOG_TTL: Duration = Duration::from_hours(7 * 24);
/// How long a source that just had an outage is skipped.
pub const OUTAGE_TTL: Duration = Duration::from_mins(10);
/// How long a catalog download started in the background keeps later
/// searches from starting the same one.
pub const WARMING_TTL: Duration = Duration::from_mins(10);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Query,
    Catalog,
    Outage,
    Warming,
    Failure,
}

impl Kind {
    const ALL: [Self; 5] = [
        Self::Query,
        Self::Catalog,
        Self::Outage,
        Self::Warming,
        Self::Failure,
    ];

    fn dir(self) -> &'static str {
        match self {
            Self::Query => "queries",
            Self::Catalog => "catalogs",
            Self::Outage => "outages",
            Self::Warming => "warming",
            Self::Failure => "failures",
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
    /// The release that wrote it. Another release may parse a source
    /// differently, so its entries count as stale: refetched when online,
    /// still served when the fetch fails.
    #[serde(default)]
    version: String,
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

    /// The entry and whether it is still within `ttl`. A malformed entry
    /// reads as absent and is removed, so the next search sees no copy at
    /// all rather than one it cannot use. The file is checked as UTF-8 once
    /// and parsed as a `str`, which spares serde_json checking every string
    /// in a catalog of thousands on its own.
    pub fn load<T: DeserializeOwned>(
        &self,
        kind: Kind,
        key: &str,
        ttl: Duration,
    ) -> Option<(T, Freshness)> {
        let path = self.path(kind, key);
        let parsed = match std::fs::read_to_string(&path) {
            Ok(json) => serde_json::from_str::<Entry<T>>(&json).ok(),
            Err(e) if e.kind() == std::io::ErrorKind::InvalidData => None,
            Err(_) => return None,
        };
        let Some(entry) = parsed else {
            let _ = std::fs::remove_file(&path);
            return None;
        };
        let age = now().saturating_sub(entry.stored);
        let freshness = if age < ttl.as_secs()
            && entry.version == env!("CARGO_PKG_VERSION")
        {
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
        let entry = Entry {
            stored,
            version: env!("CARGO_PKG_VERSION").to_owned(),
            value,
        };
        if let Ok(bytes) = serde_json::to_vec(&entry) {
            let _ = write_atomic(&self.path(kind, key), &bytes);
        }
    }

    pub fn check_writable(&self) -> anyhow::Result<()> {
        let probe = self.path(Kind::Catalog, ".probe");
        write_atomic(&probe, b"")?;
        Ok(std::fs::remove_file(probe)?)
    }

    pub fn mark_outage(&self, source: &str) {
        self.store(Kind::Outage, source, &());
    }

    /// How long ago the source last had an outage, if within [`OUTAGE_TTL`].
    pub fn recent_outage(&self, source: &str) -> Option<Duration> {
        self.marked(Kind::Outage, source, OUTAGE_TTL)
    }

    /// Writes the warming mark unless one within [`WARMING_TTL`] exists,
    /// creating the file exclusively so two searches cannot both claim the
    /// same download. True when this caller holds the mark.
    pub fn claim_warming(&self, source: &str) -> bool {
        let path = self.path(Kind::Warming, source);
        if let Some(dir) = path.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        let entry = Entry {
            stored: now(),
            version: env!("CARGO_PKG_VERSION").to_owned(),
            value: (),
        };
        let Ok(bytes) = serde_json::to_vec(&entry) else { return false };
        for _ in 0..2 {
            let created = std::fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&path);
            match created {
                Ok(mut file) => return file.write_all(&bytes).is_ok(),
                Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {
                    if self.warming(source) {
                        return false;
                    }
                    let _ = std::fs::remove_file(&path);
                }
                Err(_) => return false,
            }
        }
        false
    }

    pub fn release_warming(&self, source: &str) {
        let _ = std::fs::remove_file(self.path(Kind::Warming, source));
    }

    /// Notes why the source's catalog failed to download, until a download
    /// succeeds.
    pub fn record_failure(&self, source: &str, reason: &str) {
        self.store(Kind::Failure, source, &reason);
    }

    pub fn clear_failure(&self, source: &str) {
        let _ = std::fs::remove_file(self.path(Kind::Failure, source));
    }

    /// Why the source's catalog last failed to download, if it has not
    /// downloaded since.
    pub fn last_failure(&self, source: &str) -> Option<String> {
        self.load::<String>(Kind::Failure, source, Duration::MAX)
            .map(|(reason, _)| reason)
    }

    /// Whether a background download of the source's catalog started
    /// within [`WARMING_TTL`].
    pub fn warming(&self, source: &str) -> bool {
        self.marked(Kind::Warming, source, WARMING_TTL).is_some()
    }

    fn marked(
        &self,
        kind: Kind,
        key: &str,
        ttl: Duration,
    ) -> Option<Duration> {
        let bytes = std::fs::read(self.path(kind, key)).ok()?;
        let entry: Entry<()> = serde_json::from_slice(&bytes).ok()?;
        let age = now().saturating_sub(entry.stored);
        (age < ttl.as_secs()).then(|| Duration::from_secs(age))
    }

    /// Whether any copy of the entry is on disk, fresh or not, without
    /// reading it.
    pub fn has(&self, kind: Kind, key: &str) -> bool {
        self.path(kind, key).is_file()
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

    /// Evict entries until the directory fits the budget: queries, then
    /// catalogs, then marks, the oldest first within each.
    pub fn trim(&self) -> Usage {
        trim_files(self.files(), budget_bytes(), BUDGET_FILES)
    }

    /// Remove the entries, never the root: `--cache-dir` can point at a
    /// directory that holds other files. The root goes only once empty.
    pub fn clear(&self) -> std::io::Result<()> {
        for kind in Kind::ALL {
            match std::fs::remove_dir_all(self.root.join(kind.dir())) {
                Err(e) if e.kind() != std::io::ErrorKind::NotFound => {
                    return Err(e);
                }
                _ => {}
            }
        }
        let _ = std::fs::remove_dir(&self.root);
        Ok(())
    }

    fn files(&self) -> Vec<CachedFile> {
        Kind::ALL
            .iter()
            .filter_map(|&kind| {
                let entries = std::fs::read_dir(self.root.join(kind.dir()));
                Some(
                    entries
                        .ok()?
                        .filter_map(Result::ok)
                        .map(move |e| (kind, e)),
                )
            })
            .flatten()
            .filter_map(|(kind, entry)| {
                let meta = entry.metadata().ok()?;
                meta.is_file().then(|| CachedFile {
                    path: entry.path(),
                    kind,
                    bytes: meta.len(),
                    modified: meta.modified().unwrap_or(UNIX_EPOCH),
                })
            })
            .collect()
    }
}

struct CachedFile {
    path: PathBuf,
    kind: Kind,
    bytes: u64,
    modified: SystemTime,
}

/// Queries go first, then catalogs; the small marks that keep searches from
/// repeating work go last.
const fn eviction_order(kind: Kind) -> u8 {
    match kind {
        Kind::Query => 0,
        Kind::Catalog => 1,
        Kind::Outage | Kind::Warming | Kind::Failure => 2,
    }
}

fn trim_files(
    mut files: Vec<CachedFile>,
    max_bytes: u64,
    max_files: usize,
) -> Usage {
    // A search writes about 42 query files and a catalog is written once a
    // week, so oldest-first alone evicted every catalog after some 45
    // searches, and the slowest take half a minute to download again.
    files.sort_by_key(|f| (eviction_order(f.kind), f.modified));
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

    // An upgrade that fixes an adapter must not keep serving what the old
    // release parsed, yet an old answer beats none when the fetch fails.
    #[test]
    fn another_release_entries_are_stale_but_readable() {
        let dir = tempfile::tempdir().unwrap();
        let cache = Cache::new(dir.path().to_path_buf());
        let path = cache.path(Kind::Query, "k");
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        let now = now();
        for written in [
            format!(r#"{{"stored":{now},"version":"0.0.1","value":[1]}}"#),
            format!(r#"{{"stored":{now},"value":[1]}}"#),
        ] {
            std::fs::write(&path, written).unwrap();
            let (value, freshness): (Vec<i32>, _) =
                cache.load(Kind::Query, "k", QUERY_TTL).unwrap();
            assert_eq!(value, vec![1]);
            assert_eq!(freshness, Freshness::Stale);
        }
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
    fn a_warming_mark_is_claimed_once_until_released() {
        let dir = tempfile::tempdir().unwrap();
        let cache = Cache::new(dir.path().to_path_buf());
        assert!(cache.claim_warming("openneuro"));
        assert!(cache.warming("openneuro"));
        assert!(!cache.claim_warming("openneuro"), "claimed twice");
        cache.release_warming("openneuro");
        assert!(cache.claim_warming("openneuro"));
        cache.store_expired(Kind::Warming, "physionet", &());
        assert!(cache.claim_warming("physionet"), "a stale mark held");
    }

    #[test]
    fn a_download_failure_is_kept_until_it_is_cleared() {
        let dir = tempfile::tempdir().unwrap();
        let cache = Cache::new(dir.path().to_path_buf());
        assert_eq!(cache.last_failure("ilo"), None);
        cache.record_failure("ilo", "timed out");
        assert_eq!(cache.last_failure("ilo").as_deref(), Some("timed out"));
        cache.clear_failure("ilo");
        assert_eq!(cache.last_failure("ilo"), None);
    }

    #[test]
    fn trimming_evicts_catalogs_before_the_marks() {
        let dir = tempfile::tempdir().unwrap();
        let file = |name: &str, kind: Kind, age: u64| {
            let path = dir.path().join(name);
            std::fs::write(&path, vec![b'x'; 100]).unwrap();
            CachedFile {
                path,
                kind,
                bytes: 100,
                modified: UNIX_EPOCH + Duration::from_secs(1000 - age),
            }
        };
        let files = vec![
            file("mark.json", Kind::Warming, 50),
            file("catalog.json", Kind::Catalog, 10),
        ];
        let usage = trim_files(files, 100, 10);
        assert_eq!(usage.files, 1);
        assert!(dir.path().join("mark.json").exists(), "mark evicted");
    }

    #[test]
    fn a_malformed_entry_reads_as_absent_and_is_removed() {
        let dir = tempfile::tempdir().unwrap();
        let cache = Cache::new(dir.path().to_path_buf());
        let path = cache.path(Kind::Catalog, "uci");
        write_atomic(&path, b"{\"stored\": 1, \"val").unwrap();
        assert!(cache.has(Kind::Catalog, "uci"));
        let loaded: Option<(Vec<String>, Freshness)> =
            cache.load(Kind::Catalog, "uci", CATALOG_TTL);
        assert!(loaded.is_none());
        assert!(!cache.has(Kind::Catalog, "uci"), "the bad copy was kept");
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
                kind: Kind::Query,
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
    fn trimming_evicts_newer_queries_before_an_older_catalog() {
        let dir = tempfile::tempdir().unwrap();
        let file = |name: &str, kind: Kind, age: u64| {
            let path = dir.path().join(name);
            std::fs::write(&path, vec![b'x'; 100]).unwrap();
            CachedFile {
                path,
                kind,
                bytes: 100,
                modified: UNIX_EPOCH + Duration::from_secs(1000 - age),
            }
        };
        let files = vec![
            file("catalog.json", Kind::Catalog, 50),
            file("query-old.json", Kind::Query, 20),
            file("query-new.json", Kind::Query, 10),
        ];
        let usage = trim_files(files, 100, 10);
        assert_eq!(usage.files, 1);
        assert!(dir.path().join("catalog.json").exists(), "catalog evicted");
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
