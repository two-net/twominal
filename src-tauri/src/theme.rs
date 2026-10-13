//! Theme Manager (SRS §6.1).
//!
//! Turns Dark, Light or System into the active palette and announces changes.
//!
//! Theme resolution lives here and nowhere else (ARC-05). The App Shell tells it what the
//! user chose and what the OS reports, and passes the palette it announces on to the windows
//! and tabs.
//!
//! Filled in by #38 (themes) and #39 (palettes).

/// Resolves the theme setting and the OS appearance into the active palette.
pub struct ThemeManager;
