//! Terminal Core (SRS §6.1).
//!
//! Interprets a session's output; keeps the screen, scrollback, selection and Vim cursor;
//! answers terminal queries; announces what shell integration reports.
//!
//! Must not depend on the interface, rendering or OS processes (ARC-02). `no_std` makes
//! that a compile error: this crate cannot name a process, file, socket, thread or clock.
//!
//! Deterministic (ARC-03): the same input bytes and grid size always produce the same
//! screen state. Bytes enter through one entry point only, whether they come from the
//! Session or from a command line the user has just run (CMD-06).
//!
//! Filled in by #16 (interpreter and screen model), #17 (conformance suite and fuzzing),
//! #18 (Unicode), #19 (colors and attributes) and #26–#30.

#![no_std]
#![forbid(unsafe_code)]

/// The size of the terminal grid, in cells.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Size {
    pub cols: u16,
    pub rows: u16,
}

/// One tab's terminal: its screen, scrollback, selection and Vim cursor.
pub struct TerminalCore {
    size: Size,
}

impl TerminalCore {
    pub fn new(size: Size) -> Self {
        Self { size }
    }

    pub fn size(&self) -> Size {
        self.size
    }
}
