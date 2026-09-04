//! Where the tool keeps its files, by platform convention. Three separate
//! directories, never dotfiles sprayed into `$HOME`: config the user edits,
//! cache the tool can delete and rebuild, state that persists between runs.
//!
//! Resolution uses etcetera's base strategy: XDG on Unix, so
//! `~/.config/<app>`, `~/.cache/<app>` and `~/.local/state/<app>` including on
//! macOS, and the Windows known folders (`%APPDATA%`, `%LOCALAPPDATA%`) on
//! Windows. Only macOS differs between the base and native strategies, so
//! `choose_native_strategy` is the switch for Apple-native locations and
//! changes nothing on Windows. Paths are resolved, not created; create a
//! directory the first time you write to it (see [`crate::fs::write_atomic`],
//! which does so for you).

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
        // state_dir is None on Windows, where the strategy also aliases
        // config_dir to data_dir. Falling back to data_dir alone would hand
        // back one directory under two names, so a clone that wipes state to
        // reset would take the user's config with it. Nest instead.
        state: match base.state_dir() {
            Some(dir) => dir.join(APP),
            None => base.data_dir().join(APP).join("state"),
        },
    })
}
