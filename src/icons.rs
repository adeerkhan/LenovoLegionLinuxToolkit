//! Icon-name validation.
//!
//! GTK renders a missing icon as a generic placeholder glyph rather than
//! failing, so a typo in an icon name is invisible to every other kind of test
//! and shows up only as a slightly wrong-looking card. That is exactly what
//! happened: seven of the names this application used — `fan-symbolic`,
//! `cpu-symbolic`, `memory-symbolic`, `motherboard-symbolic`, `usb-symbolic`,
//! `air-conditioner-symbolic`, `emblem-ok-symbolic` — do not exist in the
//! Adwaita theme, and the cards were drawing placeholders.
//!
//! This module lets a test assert that every name the application declares is
//! one the system can actually draw.

use std::collections::HashSet;
use std::path::Path;
use std::sync::OnceLock;

/// Where icon themes live. XDG order: user data dirs first.
fn icon_roots() -> Vec<std::path::PathBuf> {
    let mut roots = Vec::new();
    if let Some(home) = std::env::var_os("HOME") {
        roots.push(std::path::PathBuf::from(home).join(".local/share/icons"));
    }
    roots.push(std::path::PathBuf::from("/usr/share/icons"));
    roots.push(std::path::PathBuf::from("/usr/local/share/icons"));
    roots
}

/// Every `*.svg` and `*.png` basename available in any installed theme.
fn available() -> &'static HashSet<String> {
    static NAMES: OnceLock<HashSet<String>> = OnceLock::new();
    NAMES.get_or_init(|| {
        let mut names = HashSet::new();
        for root in icon_roots() {
            collect(&root, &mut names, 0);
        }
        names
    })
}

/// Walk the icon tree, bounded in depth.
///
/// The layout is `<root>/<theme>/<size>/<category>/<name>.svg`, so four levels
/// is enough; the bound keeps a pathological tree from being walked forever.
fn collect(dir: &Path, names: &mut HashSet<String>, depth: usize) {
    if depth > 4 {
        return;
    }
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            collect(&path, names, depth + 1);
            continue;
        }
        let file = entry.file_name().to_string_lossy().to_string();
        if let Some(stem) = file
            .strip_suffix(".svg")
            .or_else(|| file.strip_suffix(".png"))
        {
            names.insert(stem.to_string());
        }
    }
}

/// Whether the system can draw `name`.
///
/// Returns `true` when no icon theme is installed at all, so the check is a
/// no-op on a machine without one rather than a false failure.
pub fn exists(name: &str) -> bool {
    if name.is_empty() {
        return true;
    }
    let names = available();
    if names.is_empty() {
        return true;
    }
    names.contains(name)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_theme_was_actually_found() {
        // If this fails the other checks are silently no-ops, which would be
        // worse than a failing test.
        assert!(
            !available().is_empty(),
            "no icon theme found; the existence check is not testing anything"
        );
        assert!(exists("input-keyboard-symbolic"));
    }

    #[test]
    fn a_name_that_cannot_exist_is_reported_missing() {
        assert!(!exists("definitely-not-a-real-icon-name-symbolic"));
    }
}
