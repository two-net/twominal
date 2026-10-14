//! Terminal Core (SRS §6.1).
//!
//! Interprets a session's output; keeps the screen, scrollback, selection and Vim cursor;
//! answers terminal queries; announces what shell integration reports.
//!
//! Must not depend on the interface, rendering or OS processes (ARC-02). `no_std` makes
//! that a compile error: this crate cannot name a process, file, socket, thread or clock.
//!
//! Deterministic (ARC-03): the same input bytes and grid size always produce the same
//! screen state. Bytes enter through one entry point only, [`TerminalCore::interpret`],
//! whether they come from the Session or from a command line the user has just run (CMD-06).
//!
//! It behaves as xterm does (TERM-01). vte splits the bytes into characters and control
//! functions; `control.rs` says what each one does; `terminal.rs` does it to the cursor and
//! the modes, and `screen.rs` to the rows. `report.rs` reads what shell integration reports.
//! DESIGN.md §3 lists what is supported and what is ignored on purpose.
//!
//! Filled in by #16 (interpreter and screen model), #17 (conformance suite and fuzzing),
//! #18 (Unicode), #19 (colors and attributes) and #26–#30.

#![no_std]
#![forbid(unsafe_code)]

extern crate alloc;
#[cfg(test)]
extern crate std;

mod control;
mod report;
mod screen;
mod terminal;
#[cfg(test)]
mod tests;

use alloc::boxed::Box;
use alloc::vec::Vec;
use core::{mem, str};

pub use report::Report;
pub use screen::{Cell, Row, Screen};
use terminal::Terminal;
pub use terminal::{Cursor, Modes, MouseTracking};

/// The most an operating system command may hold, a report of the shell integration included
/// (DESIGN.md §7). vte keeps no more than this, so a report this long or longer was cut short,
/// and is dropped.
const REPORT_LIMIT: usize = 256 * 1024;

/// The size of the terminal grid, in cells.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Size {
    pub cols: u16,
    pub rows: u16,
}

/// One tab's terminal: its screen, scrollback, selection and Vim cursor.
pub struct TerminalCore {
    /// Boxed, because its buffer for operating system commands is [`REPORT_LIMIT`] long.
    parser: Box<vte::Parser<REPORT_LIMIT>>,
    terminal: Terminal,
    /// The start of a character that the last bytes ended in the middle of, held back for the
    /// next. vte 0.15 loses a byte when it is left to finish such a character itself: given
    /// `é` cut after its first byte and then `\xa9 \xc3\xa9`, it drops the space.
    held: Vec<u8>,
}

/// What interpreting output gives besides the screen.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Event {
    /// Bytes to send to the program, answering one of its queries. Made of fixed text and
    /// numbers, never of what the program wrote (SEC-05).
    Reply(Vec<u8>),
    /// What the shell integration reported (FISH-05).
    Report(Report),
}

impl TerminalCore {
    /// A terminal of `size`, with nothing on its screens. `reports` is the nonce of the
    /// shell integration, as `Session::reports` gives it; with `None` nothing is a report.
    /// A size of zero is made one.
    pub fn new(size: Size, reports: Option<&str>) -> Self {
        Self {
            // Only without vte's `std` feature, so the buffer stays bounded (REL-02).
            parser: Box::new(vte::Parser::new_with_size()),
            terminal: Terminal::new(size, reports),
            held: Vec::new(),
        }
    }

    pub fn size(&self) -> Size {
        self.terminal.size()
    }

    /// Interprets what the program wrote. Returns the replies to send it and what its shell
    /// integration reported, in the order it asked and reported them. A sequence cut in two
    /// by the end of `bytes` is finished by the next call.
    ///
    /// The events come after all of `bytes` has been interpreted. If the command line ever
    /// needs the screen as it was at a report, `vte::Parser::advance_until_terminated` can
    /// stop there.
    pub fn interpret(&mut self, bytes: &[u8]) -> Vec<Event> {
        let joined;
        let bytes = if self.held.is_empty() {
            bytes
        } else {
            joined = [mem::take(&mut self.held).as_slice(), bytes].concat();
            &joined
        };
        let (whole, cut) = bytes.split_at(bytes.len() - unfinished(bytes));
        self.parser.advance(&mut self.terminal, whole);
        self.held.extend_from_slice(cut);
        self.terminal.take_events()
    }

    /// Makes the screens `size`. Lines are not reflowed yet (#26), and the scrolling region
    /// becomes the whole screen. A size of zero is made one, and the same size changes
    /// nothing.
    pub fn resize(&mut self, size: Size) {
        self.terminal.resize(size);
    }

    /// The screen being shown: the primary screen, or the alternate one while a full-screen
    /// program runs.
    pub fn screen(&self) -> &Screen {
        self.terminal.screen()
    }

    pub fn cursor(&self) -> Cursor {
        self.terminal.cursor()
    }

    pub fn modes(&self) -> &Modes {
        self.terminal.modes()
    }
}

/// How many bytes at the end of `bytes` start a UTF-8 character without finishing it. Holding
/// them back delays nothing else: every byte that ends a sequence is ASCII.
fn unfinished(bytes: &[u8]) -> usize {
    // A character is at most 4 bytes, so an unfinished one at most 3.
    let from = bytes.len().saturating_sub(3);
    let Some(start) = (from..bytes.len())
        .rev()
        .find(|&at| bytes[at] & 0xc0 != 0x80)
    else {
        return 0;
    };
    match str::from_utf8(&bytes[start..]) {
        Err(error) if error.valid_up_to() == 0 && error.error_len().is_none() => {
            bytes.len() - start
        }
        _ => 0,
    }
}
