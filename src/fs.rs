//! Crash-safe file writes. The failure this prevents: a process that dies
//! halfway through rewriting a file, leaving a truncated or empty original.
//!
//! The fix is write-to-temp-then-rename. The temp file lives in the same
//! directory as the target so the final step is a rename within one
//! filesystem, which is atomic; a temp file in `/tmp` would make `persist` a
//! cross-device copy, which is not. A reader of the target therefore always
//! sees either the whole old file or the whole new one, never a mix.

use std::path::Path;

use anyhow::{Context, Result};
use std::io::Write;

/// Write `bytes` to `path` atomically, creating parent directories as needed.
// Ready helper, tested but not yet wired into a command (the `count` demo only
// reads). `#[expect]` would misfire under `cargo test`, where the test below
// does use it; a plain allow is correct until your first writing command calls
// it, at which point you can drop this line.
#[allow(dead_code)]
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

        // A second write replaces the contents, not appends, and the parent
        // directory it just created is reused without error.
        write_atomic(&target, b"second").unwrap();
        assert_eq!(std::fs::read(&target).unwrap(), b"second");
    }
}
