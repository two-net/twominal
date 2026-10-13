//! Settings Store (SRS §6.1).
//!
//! Loads, validates, saves and announces settings.
//!
//! Settings live here and nowhere else (ARC-05): the file, its format, every default in
//! Appendix B and every validation rule. It names no other component. The App Shell passes
//! each change on to whoever it concerns, and tabs receive theirs as messages (ARC-04).
//!
//! Filled in by #9.

/// Every setting in Appendix B, as loaded from the configuration file.
pub struct SettingsStore;
