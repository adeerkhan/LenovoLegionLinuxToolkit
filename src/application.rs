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
