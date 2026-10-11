//! Crash-safe file writes: write to a temp file, then rename over the target.
//! The temp file lives in the target's directory so the rename stays on one
//! filesystem and is atomic; readers see the whole old file or the whole new
//! one.

use std::path::Path;

use anyhow::{Context, Result};
use std::io::Write;

/// Write `bytes` to `path` atomically, creating parent directories as needed.
pub fn write_atomic(path: &Path, bytes: &[u8]) -> Result<()> {
    let dir = match path.parent() {
        Some(p) if !p.as_os_str().is_empty() => p,
        _ => Path::new("."),
    };
    std::fs::create_dir_all(dir).with_context(|| {
        format!("cannot create directory {}", dir.display())
    })?;

    let mut tmp = tempfile::NamedTempFile::new_in(dir).with_context(|| {
        format!("cannot open a temp file in {}", dir.display())
    })?;
    tmp.write_all(bytes)
        .with_context(|| format!("cannot write {}", path.display()))?;
    tmp.as_file()
        .sync_all()
        .with_context(|| format!("cannot flush {}", path.display()))?;
    tmp.persist(path)
        .map_err(|e| e.error)
        .with_context(|| format!("cannot save {}", path.display()))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn writes_then_overwrites_in_place() {
        let dir = tempfile::tempdir().unwrap();
        let target = dir.path().join("nested").join("state.txt");

        write_atomic(&target, b"first").unwrap();
        assert_eq!(std::fs::read(&target).unwrap(), b"first");

        // Replaces rather than appends, and reuses the created parent.
        write_atomic(&target, b"second").unwrap();
        assert_eq!(std::fs::read(&target).unwrap(), b"second");
    }
}
