//! Application entry: the libadwaita application object.

use adw::prelude::*;
use std::cell::Cell;

thread_local! {
    /// Whether the window has been shown at least once this process.
    static SHOWN: Cell<bool> = const { Cell::new(false) };
}

pub struct LtkApplication {
    app: adw::Application,
}

impl LtkApplication {
    pub fn new() -> Self {
        let app = adw::Application::builder()
            .application_id("org.legiontoolkit.Ltk")
            .build();

        app.connect_activate(|app| {
            crate::design::init();

            let window = crate::window::MainWindow::new(app);

            // `start_minimized` means "run in the background": the window is
            // not shown on the first activation, and a second launch (which
            // activates the already-running instance) reveals it. That is the
            // only coherent meaning without a tray icon, which this application
            // does not have yet.
            let shown_once = SHOWN.get();
            if !legiontoolkit::settings::current().start_minimized || shown_once {
                window.present();
            } else {
                tracing::info!("start_minimized is on; launch again to show the window");
            }
            SHOWN.set(true);

            // `--shot <path>` renders the window and exits, so the layout can
            // be reviewed without a display. Deferred one tick because the
            // window has no allocation until it has been mapped and measured.
            if let Some((path, page)) = crate::shot_request() {
                let window = window.clone();
                // Select the page now, while activating, so the stack has laid
                // it out by the time the deferred capture reads its allocation.
                if let Some(page) = page {
                    window.show_page(&page);
                }
                // Scrolling is deferred with the capture: the adjustment has no
                // range until the page has been laid out, and setting it here
                // did nothing.
                glib::timeout_add_local_once(std::time::Duration::from_millis(900), move || {
                    if crate::shot_log_tree() {
                        window.log_page();
                    }
                    if let Err(e) = window.capture(&path) {
                        tracing::error!("{e}");
                        std::process::exit(1);
                    }
                    std::process::exit(0);
                });
            }
        });

        Self { app }
    }

    pub fn run(&self) {
        self.app.run();
    }
}

impl Default for LtkApplication {
    fn default() -> Self {
        Self::new()
    }
}

impl crate::window::MainWindow {
    /// Render this window to a PNG. Used by `--shot`.
    pub fn capture(&self, path: &str) -> anyhow::Result<()> {
        crate::shot::capture(self.window.upcast_ref::<gtk4::Window>(), path)
    }
}
