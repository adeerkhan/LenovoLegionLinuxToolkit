//! Services: privilege escalation, profiles and automation.
//!
//! Privilege lives here because it is the only place the application needs
//! elevated rights, and keeping it in one module makes the security boundary
//! reviewable.

pub mod privilege;
