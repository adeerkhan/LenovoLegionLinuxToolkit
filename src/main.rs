//! Legion Linux Toolkit — GUI binary.
//!
//! The GUI holds no hardware logic of its own beyond the capability model in the
//! library; everything here is presentation.

mod application;
mod design;
mod pages;
mod shot;
mod window;

use tracing_subscriber::{fmt, EnvFilter};

fn main() {
    fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")),
        )
        .init();

    tracing::info!(
        version = env!("CARGO_PKG_VERSION"),
        machine = %legiontoolkit::paths::machine_summary(),
        driver = %legiontoolkit::backends::driver_status(),
        "starting"
    );

    application::LtkApplication::new().run();
}

/// Destination for `LEGIONTOOLKIT_SHOT`; the application writes a PNG of the
/// window and exits, so the layout can be reviewed without a display.
///
/// An environment variable rather than a command-line option because GTK 4
/// rejects unknown arguments during option parsing, before the application
/// runs, and there is no main-option API left to register one with.
pub fn shot_target() -> Option<String> {
    std::env::var("LEGIONTOOLKIT_SHOT")
        .ok()
        .filter(|path| !path.is_empty())
}

/// Which page to show before capturing, so `--shot` can review a section rather
/// than only the landing page.
pub fn shot_page() -> Option<String> {
    std::env::var("LEGIONTOOLKIT_PAGE").ok()
}
