//! Omarchy theme integration.
//!
//! On startup, both OSD frontends read the active Omarchy theme and map it
//! to a [`Palette`] used by the renderer. A future commit (Commit 5) wires
//! up real CSS/conf parsing and a `notify`-based file watcher so the OSD
//! re-applies colors when the user switches themes.
//!
//! This module currently provides:
//!
//! - [`omarchy_theme_dir`] — canonical path lookup
//! - [`load_palette`] — entry point that returns the fallback palette today
//!   and will return a parsed palette in Commit 5
//! - [`ThemeWatcher`] — placeholder that hands out a single static palette;
//!   the real implementation will subscribe to filesystem events
//!
//! Keeping the surface stable now means neither frontend has to change
//! when the parsing lands.

use std::path::PathBuf;

use crate::osd::visual::Palette;

/// Canonical Omarchy "current theme" directory.
///
/// Resolves to `~/.config/omarchy/current/theme` regardless of whether the
/// directory exists. Commit 5 verifies the structure on the user's system
/// before wiring up real parsing.
pub fn omarchy_theme_dir() -> Option<PathBuf> {
    let home = std::env::var_os("HOME")?;
    let mut p = PathBuf::from(home);
    p.push(".config/omarchy/current/theme");
    Some(p)
}

/// Load a palette from the active Omarchy theme.
///
/// **Status:** stub. Returns [`Palette::fallback`] until Commit 5 lands the
/// real CSS/conf parser. The function signature is stable so frontends can
/// call it now.
pub fn load_palette() -> Palette {
    let _dir = omarchy_theme_dir();
    // Real parsing arrives in Commit 5. For now both frontends use the
    // hardcoded fallback so they're visually consistent during development.
    Palette::fallback()
}

/// Theme watcher placeholder.
///
/// The real implementation will own a `notify` watcher and emit a new
/// [`Palette`] each time the active theme changes. Today it just holds a
/// single palette; both frontends pass the snapshot through to their
/// renderer on each frame.
pub struct ThemeWatcher {
    palette: Palette,
}

impl ThemeWatcher {
    pub fn new() -> Self {
        Self {
            palette: load_palette(),
        }
    }

    /// Current palette. Cheap to call every frame.
    pub fn palette(&self) -> Palette {
        self.palette
    }
}

impl Default for ThemeWatcher {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn theme_dir_resolves_under_home() {
        std::env::set_var("HOME", "/tmp/fakehome");
        let p = omarchy_theme_dir().unwrap();
        assert!(p.ends_with(".config/omarchy/current/theme"));
    }

    #[test]
    fn watcher_returns_fallback_palette() {
        let w = ThemeWatcher::new();
        assert_eq!(w.palette(), Palette::fallback());
    }
}
