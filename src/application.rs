//! Application entry: the libadwaita application object.

use adw::prelude::*;

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
            window.present();

            // `--shot <path>` renders the window and exits, so the layout can
            // be reviewed without a display. Deferred one tick because the
            // window has no allocation until it has been mapped and measured.
            if let Some(path) = crate::shot_target() {
                let window = window.clone();
                // Select the page now, while activating, so the stack has laid
                // it out by the time the deferred capture reads its allocation.
                if let Some(page) = crate::shot_page() {
                    window.show_page(&page);
                }
                glib::timeout_add_local_once(std::time::Duration::from_millis(900), move || {
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
