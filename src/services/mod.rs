//! Services: privilege escalation, profiles and automation.
//!
//! Privilege lives in its own module because it is the only place the
//! application needs elevated rights, and keeping it in one module makes the
//! security boundary reviewable.

use crate::backends::Backend;

pub mod autostart;
pub mod notify;
pub mod privilege;

/// States that make the whole application inert, rather than making one control
/// unsupported.
///
/// Deliberately *not* gated on capability detection. These are conditions the
/// user has to fix, so they are surfaced once at the top of the window rather
/// than repeated under every affected control where they would be noise.
///
/// Both the availability flag and the diagnosis are passed in rather than
/// probed here. That keeps this function pure — a warning list that consulted
/// the real system would be untestable, and the first version of this
/// accidentally did consult it, which the "everything in place" test caught.
pub fn system_warnings(
    backend: &dyn Backend,
    escalation_available: bool,
    escalation_findings: &[String],
) -> Vec<String> {
    let mut warnings = Vec::new();

    // A polkit prompt without its helper, or a helper the policy does not name,
    // makes every write fail looking like a broken control.
    if !escalation_available {
        // Name the first cause rather than a generic message: every cause has a
        // different fix, and the user cannot see any of them. Only the first is
        // shown — a banner is one line, and `AdwBanner` ellipsises the rest into
        // unreadability. The full list is logged, and a failed write reports
        // pkexec's own stderr, which is where the detail belongs.
        let detail = escalation_findings
            .first()
            .map(|first| format!(" — {first}"))
            .unwrap_or_default();
        warnings.push(format!(
            "Privileged writes are unavailable{detail}. Settings will be shown but cannot \
             be changed."
        ));
    } else {
        // Escalation looks available, but that only means the file and pkexec
        // exist — not that polkit will authorise it. Anything wrong here shows
        // up as a password prompt that then fails, which is the worst kind of
        // failure to diagnose.
        for finding in escalation_findings {
            warnings.push(format!("Privileged writes may fail: {finding}"));
        }
    }

    // `ec_readonly=1` disables every EC write kernel-wide. Nothing on the page
    // can work, and the reason is invisible without this.
    if let Some(warning) = backend.global_warning() {
        warnings.push(warning);
    }

    warnings
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::backends::MockBackend;

    #[test]
    fn warns_when_privileged_writes_are_unavailable() {
        let warnings = system_warnings(&MockBackend, false, &[]);
        assert!(
            warnings
                .iter()
                .any(|w| w.contains("Privileged writes are unavailable")),
            "expected an escalation warning, got {warnings:?}"
        );
    }

    #[test]
    fn silent_when_everything_is_in_place() {
        // The mock reports no driver-level warning and no escalation findings,
        // so there should be nothing to say at all.
        let warnings = system_warnings(&MockBackend, true, &[]);
        assert!(
            warnings.is_empty(),
            "expected no warnings when the system is ready, got {warnings:?}"
        );
    }

    /// A failed polkit wiring must reach the user even when the helper and
    /// pkexec both exist — that combination prompts for a password and then
    /// fails, which is otherwise unexplainable.
    #[test]
    fn a_broken_polkit_wiring_is_surfaced() {
        let findings = vec!["no polkit action declares exec.path".to_string()];
        let warnings = system_warnings(&MockBackend, true, &findings);
        assert!(
            warnings.iter().any(|w| w.contains("may fail")),
            "expected the finding to be shown, got {warnings:?}"
        );
    }
}
