//! Legion Linux Toolkit — GUI binary.
//!
//! The GUI holds no hardware logic of its own beyond the capability model in the
//! library; everything here is presentation.

mod application;
mod design;
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
