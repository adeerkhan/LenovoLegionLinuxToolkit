//! Best-effort desktop notifications.
//!
//! The freedesktop notification service is reached through `notify-send`, the
//! small standard client, rather than by linking a D-Bus stack. It is
//! best-effort by design: a machine with no session bus or no `notify-send`
//! simply does not get the notification, which is never a reason to fail the
//! action that triggered it.

/// Send a notification, unless the user turned notifications off.
///
/// Called off the main thread (from the automation worker), where a blocking
/// `notify-send` is harmless.
pub fn send(title: &str, body: &str) {
    if !crate::settings::current().notifications {
        return;
    }
    let _ = std::process::Command::new("notify-send")
        .arg("--app-name=Legion Toolkit")
        .arg("--icon=legiontoolkit")
        .arg(title)
        .arg(body)
        .status();
}
