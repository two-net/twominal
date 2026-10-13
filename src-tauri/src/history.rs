//! History Store (SRS §6.1).
//!
//! Keeps the command history shared by all tabs; loads, saves and clears it.
//!
//! Must not store terminal output.
//!
//! The command history lives here and nowhere else (ARC-05). It is the one thing tabs share,
//! and they reach it only through the trait the Command Line defines, which this store
//! implements (ARC-04).
//!
//! Filled in with the command-line editor (CMD-12, CMD-13), which has no issue yet.

/// The command history shared by all tabs.
pub struct HistoryStore;
