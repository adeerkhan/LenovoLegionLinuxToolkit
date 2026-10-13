//! Services: privilege escalation, profiles and automation.
//!
//! Privilege lives in its own module because it is the only place the
//! application needs elevated rights, and keeping it in one module makes the
//! security boundary reviewable.

use crate::backends::Backend;

pub mod privilege;

use privilege::HELPER_PATH;

/// States that make the whole application inert, rather than making one control
/// unsupported.
///
/// Deliberately *not* gated on capability detection. These are conditions the
/// user has to fix, so they are surfaced once at the top of the window rather
/// than repeated under every affected control where they would be noise.
///
/// `escalation_available` is passed in rather than probed here so the warnings
/// can be tested without arranging a real polkit setup on the test machine.
pub fn system_warnings(backend: &dyn Backend, escalation_available: bool) -> Vec<String> {
    let mut warnings = Vec::new();

    // A polkit prompt without its helper, or a helper the policy does not name,
    // makes every write fail looking like a broken control.
    if !escalation_available {
        warnings.push(format!(
            "Privileged writes are unavailable: pkexec or {HELPER_PATH} is missing. \
             Settings will be shown but cannot be changed."
        ));
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
        let warnings = system_warnings(&MockBackend, false);
        assert!(
            warnings
                .iter()
                .any(|w| w.contains("Privileged writes are unavailable")),
            "expected an escalation warning, got {warnings:?}"
        );
    }

    #[test]
    fn silent_when_everything_is_in_place() {
        // The mock reports no driver-level warning, so with escalation
        // available there should be nothing to say at all.
        let warnings = system_warnings(&MockBackend, true);
        assert!(
            warnings.is_empty(),
            "expected no warnings when the system is ready, got {warnings:?}"
        );
    }
}
