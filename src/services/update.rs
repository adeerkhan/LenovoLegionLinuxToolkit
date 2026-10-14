//! Update notification against this project's GitHub releases.
//!
//! The reference application checks for updates in the background and tells the
//! user when a newer build exists. We do the same, minimally:
//!
//! - One HTTPS GET of the public releases API — no telemetry, no account, and
//!   no third-party service beyond GitHub itself.
//! - Runs on a background thread, and the result is polled from the GTK main
//!   loop, exactly as the GPU poller is: nothing blocks the UI.
//! - Only once per launch unless the user asks, and only when the setting is on.
//!
//! Downloading and installing the update is deliberately left to the user's
//! package manager or `git pull`, because an application cannot safely replace
//! its own installed binary and expect the running process to remain coherent.

use std::sync::Mutex;

/// The public releases API for this repository.
const RELEASES_API: &str =
    "https://api.github.com/repos/adeerkhan/LenovoLegionLinuxToolkit/releases/latest";

/// The newest published release.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Update {
    /// The published version, e.g. `0.2.0`.
    pub version: String,
    /// The release page, opened when the user asks for it.
    pub url: String,
}

/// The background check's outcome: `None` while running, `Some(None)` when the
/// check finished and found nothing newer.
static RESULT: Mutex<Option<Option<Update>>> = Mutex::new(None);

/// Query GitHub and record whether a newer release exists.
///
/// Safe to call more than once — the Settings page's "check now" does — because
/// the outcome simply overwrites the last one. Nothing blocks the caller.
pub fn check_in_background() {
    std::thread::Builder::new()
        .name("update-check".into())
        .spawn(|| {
            if let Ok(mut guard) = RESULT.lock() {
                *guard = Some(fetch_latest());
            }
        })
        .ok();
}

/// The update available now, if the check has finished and found one.
pub fn available() -> Option<Update> {
    RESULT.lock().ok()?.clone().flatten()
}

/// Whether the check has finished (so a poller can stop).
pub fn finished() -> bool {
    RESULT.lock().map(|guard| guard.is_some()).unwrap_or(false)
}

/// Do the check on the calling thread, for an explicit "check now".
pub fn check_now() -> Option<Update> {
    let found = fetch_latest();
    if let Ok(mut guard) = RESULT.lock() {
        *guard = Some(found.clone());
    }
    found
}

fn fetch_latest() -> Option<Update> {
    let response = ureq::get(RELEASES_API)
        .header("Accept", "application/vnd.github+json")
        .header(
            "User-Agent",
            concat!("legiontoolkit/", env!("CARGO_PKG_VERSION")),
        )
        .call()
        .ok()?;
    let mut response = response;
    let body = response.body_mut().read_to_string().ok()?;
    let json: serde_json::Value = serde_json::from_str(&body).ok()?;

    let version = json
        .get("tag_name")?
        .as_str()?
        .trim()
        .trim_start_matches('v')
        .to_string();
    let url = json.get("html_url")?.as_str()?.to_string();

    // A draft or prerelease is not something to offer a user.
    let draft = json.get("draft").and_then(|d| d.as_bool()).unwrap_or(false);
    let prerelease = json
        .get("prerelease")
        .and_then(|p| p.as_bool())
        .unwrap_or(false);
    if draft || prerelease {
        return None;
    }

    is_newer(&version, env!("CARGO_PKG_VERSION")).then_some(Update { version, url })
}

/// Whether `candidate` is a strictly newer version than `current`.
///
/// Dot-separated numeric components, compared left to right; anything missing
/// counts as zero, so `0.2` equals `0.2.0`.
fn is_newer(candidate: &str, current: &str) -> bool {
    for (a, b) in parts(candidate).iter().zip(parts(current).iter()) {
        match a.cmp(b) {
            std::cmp::Ordering::Greater => return true,
            std::cmp::Ordering::Less => return false,
            std::cmp::Ordering::Equal => continue,
        }
    }
    // `0.2.0` vs `0.2` is not newer, but `0.2.1` vs `0.2` is.
    parts(candidate).len() > parts(current).len()
}

fn parts(version: &str) -> Vec<u64> {
    version
        .split('.')
        .map(|p| p.parse::<u64>().unwrap_or(0))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn newer_versions_are_detected() {
        assert!(is_newer("0.2.0", "0.1.0"));
        assert!(is_newer("1.0.0", "0.9.9"));
        assert!(is_newer("0.2.1", "0.2"));
    }

    #[test]
    fn the_same_version_is_not_newer() {
        assert!(!is_newer("0.1.0", "0.1.0"));
        assert!(!is_newer("0.2", "0.2.0"));
    }

    #[test]
    fn an_older_or_malformed_version_is_not_newer() {
        assert!(!is_newer("0.1.0", "0.2.0"));
        assert!(!is_newer("not-a-version", "0.1.0"));
    }
}
