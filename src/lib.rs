//! Legion Linux Toolkit — shared library.
//!
//! Split out from the GUI binary so that the privileged helper can reuse the
//! path-resolution logic without linking any GUI code, and without the helper
//! carrying a GTK runtime into a process that runs as root.
//!
//! The security boundary is [`policy`]: it is the single allowlist of
//! capability keys the helper will write.

pub mod backends;
pub mod capability;
pub mod icons;
pub mod paths;
pub mod policy;
pub mod presets;
pub mod services;
pub mod settings;
pub mod system;
