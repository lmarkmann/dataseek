//! Where the tool keeps its files: config the user edits, cache it can rebuild,
//! and state that persists between runs.
//!
//! Uses etcetera's base strategy: XDG paths on Unix (macOS included), known
//! folders on Windows. Paths are resolved, not created;
//! [`crate::fs::write_atomic`] creates parents on write.

use std::path::PathBuf;

use anyhow::{Context, Result};
use etcetera::base_strategy::{BaseStrategy, choose_base_strategy};

const APP: &str = env!("CARGO_PKG_NAME");

/// The tool's three home directories, each scoped to the app name.
pub struct Paths {
    /// User-editable configuration.
    pub config: PathBuf,
    /// Disposable, rebuildable data.
    pub cache: PathBuf,
    /// State that should survive between runs but is not user-edited.
    pub state: PathBuf,
}

/// Resolve the tool's directories. Fails only if the home directory cannot be
/// found at all, which is the one case worth surfacing to the user.
pub fn resolve() -> Result<Paths> {
    let base =
        choose_base_strategy().context("cannot locate your home directory")?;
    Ok(Paths {
        config: base.config_dir().join(APP),
        cache: base.cache_dir().join(APP),
        // state_dir is None on Windows, where config_dir aliases data_dir.
        // Nest so wiping state cannot take the config with it.
        state: match base.state_dir() {
            Some(dir) => dir.join(APP),
            None => base.data_dir().join(APP).join("state"),
        },
    })
}
