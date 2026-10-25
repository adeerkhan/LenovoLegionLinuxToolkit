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
    use std::io::IsTerminal;

    // Logs go to stderr, which is where someone reading them expects them —
    // and where `2>&1 | tee` in CONTRIBUTING points. Colour only when that
    // stream is a terminal, so a teed log file is free of ANSI escapes;
    // NO_COLOR is respected by the subscriber itself on top of this.
    fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")),
        )
        .with_writer(std::io::stderr)
        .with_ansi(std::io::stderr().is_terminal())
        .init();

    tracing::info!(
        version = env!("CARGO_PKG_VERSION"),
        machine = %legiontoolkit::paths::machine_summary(),
        driver = %legiontoolkit::backends::driver_status(),
        "starting"
    );

    application::LtkApplication::new().run();
}

/// The screenshot request, if any: where to write, and which page to show.
///
/// Environment variables rather than command-line options because GTK 4 rejects
/// unknown arguments during option parsing, before the application runs, and
/// there is no main-option API left to register one with.
///
/// `LEGIONTOOLKIT_SHOT=<path>` renders the window to a PNG and exits.
/// `LEGIONTOOLKIT_PAGE=<id>` selects the section to capture.
/// `LEGIONTOOLKIT_LOG_TREE=1` logs the visible page's widget tree and exits
/// without capturing, for checking content a screenshot cannot reach.
pub fn shot_log_tree() -> bool {
    std::env::var("LEGIONTOOLKIT_LOG_TREE").as_deref() == Ok("1")
}

pub fn shot_request() -> Option<(String, Option<String>)> {
    let path = std::env::var("LEGIONTOOLKIT_SHOT")
        .ok()
        .filter(|path| !path.is_empty())?;
    Some((path, std::env::var("LEGIONTOOLKIT_PAGE").ok()))
}
