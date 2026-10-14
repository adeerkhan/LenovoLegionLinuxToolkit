//! Integration with [keyRGB](https://github.com/Rainexn0b/keyRGB).
//!
//! Legion Gen10 keyboards are 24-zone (`048d:c195`) or per-key + chassis
//! (`048d:c197`) RGB, driven over USB. keyRGB already implements that ITE
//! controller, so this project does not reimplement it: two programs writing
//! the same keyboard fight each other — keyRGB's own documentation warns about
//! exactly that. So this application keeps the WMI on/off/brightness it already
//! has and hands *colour and effects* to keyRGB, launching it when asked.

use std::path::PathBuf;
use std::process::Command;

/// The installed `keyrgb` launcher, if there is one.
pub fn binary() -> Option<PathBuf> {
    if let Some(path) = std::env::var_os("PATH") {
        if let Some(found) = std::env::split_paths(&path)
            .map(|dir| dir.join("keyrgb"))
            .find(|candidate| candidate.is_file())
        {
            return Some(found);
        }
    }
    // The documented install location, in case PATH in this process is sparse.
    let home = std::env::var_os("HOME")?;
    let fallback = PathBuf::from(home).join(".local/bin/keyrgb");
    fallback.is_file().then_some(fallback)
}

pub fn is_installed() -> bool {
    binary().is_some()
}

/// Start keyRGB. It is a tray application, so this returns as soon as it is
/// launched rather than waiting for it.
pub fn launch() -> anyhow::Result<()> {
    let binary = binary().ok_or_else(|| anyhow::anyhow!("keyRGB is not installed"))?;
    Command::new(binary).spawn()?;
    Ok(())
}
