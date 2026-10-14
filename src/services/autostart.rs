//! Start on login, via the XDG autostart directory.
//!
//! There is no service to install: a desktop entry in
//! `$XDG_CONFIG_HOME/autostart` is the standard, session-agnostic way for a
//! user application to be started by the desktop. Writing it is a plain file
//! operation in the user's own config directory — no privilege involved.

use std::path::{Path, PathBuf};

/// The desktop entry written when start-on-login is enabled.
///
/// `X-GNOME-Autostart-enabled=true` is understood by GNOME and ignored
/// elsewhere, so the file works on any freedesktop session.
const ENTRY: &str = "\
[Desktop Entry]
Type=Application
Name=Legion Toolkit
Comment=Control power modes, thermals, lighting and battery on Lenovo Legion laptops
Exec=legiontoolkit
Icon=legiontoolkit
Terminal=false
X-GNOME-Autostart-enabled=true
";

/// Where the autostart entry lives.
pub fn desktop_path() -> Option<PathBuf> {
    let base = std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .filter(|p| p.is_absolute())
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".config")))?;
    Some(base.join("autostart").join("legiontoolkit.desktop"))
}

/// Whether start-on-login is currently enabled (the entry exists).
pub fn is_enabled() -> bool {
    desktop_path().map(|p| p.exists()).unwrap_or(false)
}

/// Enable or disable start-on-login.
pub fn set(enabled: bool) -> anyhow::Result<()> {
    let path = desktop_path().ok_or_else(|| anyhow::anyhow!("no config directory"))?;
    write_at(&path, enabled)
}

/// The file operation, split out so it is testable against a temp path.
fn write_at(path: &Path, enabled: bool) -> anyhow::Result<()> {
    if enabled {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(path, ENTRY)?;
    } else if path.exists() {
        std::fs::remove_file(path)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_entry_launches_the_installed_binary() {
        assert!(ENTRY.contains("Exec=legiontoolkit"));
        assert!(ENTRY.contains("Type=Application"));
    }

    #[test]
    fn enabling_writes_the_entry_and_disabling_removes_it() {
        let dir = std::env::temp_dir().join(format!("ltk-autostart-{}", std::process::id()));
        let path = dir.join("legiontoolkit.desktop");
        let _ = std::fs::remove_dir_all(&dir);

        write_at(&path, true).expect("enable");
        assert!(path.exists());
        assert!(std::fs::read_to_string(&path)
            .unwrap()
            .contains("Exec=legiontoolkit"));

        write_at(&path, false).expect("disable");
        assert!(!path.exists());

        let _ = std::fs::remove_dir_all(&dir);
    }
}
