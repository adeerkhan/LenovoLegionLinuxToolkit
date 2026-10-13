//! Hardware backends.
//!
//! The application is written against [`Backend`], never against sysfs. This
//! exists so the UI can be developed and screenshotted without the target
//! laptop attached, and so a capability whose logic we do not want to
//! reimplement (fan curves) can later be served by bridging to
//! `legion_cli` instead of by native Rust code.

pub mod mock;
pub mod sysfs;

use crate::capability::Capability;

/// Produces the capabilities available on this machine.
pub trait Backend: Send + Sync {
    /// Human-readable name of the backend, for the About page.
    fn name(&self) -> &str;

    /// The capability report. Called once at startup and re-invoked when the
    /// user asks to refresh.
    fn capabilities(&self) -> Vec<Capability>;

    /// A warning to show at the top of the window, if the driver is in a state
    /// that makes writes inert — e.g. `ec_readonly=1`.
    fn global_warning(&self) -> Option<String> {
        None
    }
}

pub use mock::MockBackend;
pub use sysfs::SysfsBackend;

use crate::paths;

/// A one-line description of the driver state, for logs and the About page.
pub fn driver_status() -> String {
    let paths = paths::discover();
    match (&paths.legion, &paths.hwmon) {
        (Some(platform), Some(hwmon)) => format!(
            "legion_laptop loaded ({} / {})",
            platform.display(),
            hwmon.display()
        ),
        (Some(platform), None) => {
            format!("legion_laptop loaded, no hwmon ({})", platform.display())
        }
        (None, _) => {
            if paths.ideapad.is_some() {
                "legion_laptop NOT loaded; ideapad_acpi only".to_string()
            } else {
                "no Legion driver detected".to_string()
            }
        }
    }
}
