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
//! the modes, and `screen.rs` to the rows. `scrollback.rs` keeps the rows that scroll off the
//! primary screen (TERM-14) and wraps the lines again when the grid's width changes (TERM-16).
//! `width.rs` says how many cells a character takes (TERM-02), and `style.rs` what SGR does to the colors and attributes of the cells (TERM-03
//! to TERM-05). `keyboard.rs` keeps the kitty keyboard protocol's flags (TERM-09).
//! `report.rs` reads what shell integration reports. `osc.rs` reads what the other operating
//! system commands ask for: titles, the working directory, colors, links and the clipboard
//! (TERM-12, TERM-13). `colors.rs` keeps the colors programs set, and `links.rs` the links
//! they make. `select.rs` keeps what the user selected and gives the text copying takes
//! (TERM-17). DESIGN.md §3 lists what is supported and what is ignored on purpose.
//!
//! Filled in by #16 (interpreter and screen model), #17 (conformance suite and fuzzing),
//! #18 (Unicode), #19 (colors and attributes), #20 (keyboard flags), #21 (mouse modes), #26
//! (scrollback and reflow), #27 (operating system commands and the bell), #28 (selection) and
//! #29–#30.

#![no_std]
#![forbid(unsafe_code)]

extern crate alloc;
#[cfg(test)]
extern crate std;

mod colors;
mod control;
/// What the fuzz targets check. Only for them and for the tests.
#[cfg(any(test, fuzzing))]
#[doc(hidden)]
pub mod fuzz;
mod keyboard;
mod links;
mod osc;
mod report;
mod screen;
mod scrollback;
mod select;
mod style;
mod terminal;
#[cfg(test)]
mod tests;
mod width;

use alloc::boxed::Box;
use alloc::string::String;
use alloc::vec::Vec;
use core::{mem, str};

pub use colors::{ColorQuery, Colors, Rgb, ThemeColor};
pub use keyboard::Keyboard;
pub use links::{Link, LinkId};
pub use osc::{Clipboard, ClipboardQuery, Directory, Selection};
pub use report::Report;
pub use screen::{Cell, Row, Screen};
pub use scrollback::Scrollback;
pub use select::{Place, Selected, Selecting};
pub use style::{Color, Style, Underline};
use terminal::Terminal;
pub use terminal::{Cursor, CursorShape, CursorStyle, Modes, MouseFormat, MouseTracking};
pub use width::width;

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

/// What interpreting output gives besides the screen. DESIGN.md §3 *A tab* says what the Tab
/// does with each.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Event {
    /// Bytes to send to the program, answering one of its queries. Made of fixed text and
    /// numbers, never of what the program wrote (SEC-05).
    Reply(Vec<u8>),
    /// What the shell integration reported (FISH-05).
    Report(Report),
    /// A program set the title (OSC 0, OSC 2), or gave back one `CSI 22 t` saved (TERM-12).
    /// None gives the tab its own title back (TAB-12).
    Title(Option<String>),
    /// A program said where it is (OSC 7, TERM-12).
    Directory(Box<Directory>),
    /// A program rang the bell (BEL, TERM-12, TERM-21).
    Bell,
    /// A program asked for one of the theme's colors (TERM-12). Its answer goes to the program
    /// in its place among the replies.
    Color(ColorQuery),
    /// A program asked to write or read the clipboard (TERM-13, SEC-04). The answer to a read
    /// goes to the program in its place among the replies.
    Clipboard(Box<Clipboard>),
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

    /// Interprets what the program wrote. Returns the replies to send it, what its shell
    /// integration reported and what else programs asked for, in the order they came. A
    /// sequence cut in two by the end of `bytes` is finished by the next call.
    ///
    /// The events come after all of `bytes` has been interpreted.
    pub fn interpret(&mut self, bytes: &[u8]) -> Vec<Event> {
        let joined;
        let bytes = if self.held.is_empty() {
            bytes
        } else {
            joined = [mem::take(&mut self.held).as_slice(), bytes].concat();
            &joined
        };
        let (whole, cut) = bytes.split_at(bytes.len() - unfinished(bytes));
        let mut rest = whole;
        while let Some(&next) = rest.first() {
            // vte stopped after the ESC that ended an OSC: it acts if that ESC starts ST. With
            // no byte left, the next call decides.
            if self.terminal.waiting() {
                self.terminal.settle(next == b'\\');
            }
            let done = self
                .parser
                .advance_until_terminated(&mut self.terminal, rest);
            rest = &rest[done..];
        }
        self.held.extend_from_slice(cut);
        self.terminal.take_events()
    }

    /// Makes the screens `size`. The primary screen's lines, with its scrollback's, are wrapped
    /// again to the new width (TERM-16), and the cursor moves with its cell. The scrolling
    /// region becomes the whole screen. A size of zero is made one, and the same size changes
    /// nothing. DESIGN.md §3 *Reflow* has the rules.
    pub fn resize(&mut self, size: Size) {
        self.terminal.resize(size);
    }

    /// The screen being shown: the primary screen, or the alternate one while a full-screen
    /// program runs.
    pub fn screen(&self) -> &Screen {
        self.terminal.screen()
    }

    /// The rows that scrolled off the top of the primary screen (TERM-14). The alternate
    /// screen keeps none.
    pub fn scrollback(&self) -> &Scrollback {
        self.terminal.scrollback()
    }

    /// Keeps no more than `rows` rows of scrollback, and no more than 100,000, dropping the
    /// oldest at once: the Scrollback setting (TERM-14, Appendix B). Until it is called it keeps
    /// 10,000, Appendix B's default.
    pub fn set_scrollback(&mut self, rows: usize) {
        self.terminal.set_scrollback(rows);
    }

    /// Clear (TERM-22): empties the scrollback, and on the primary screen keeps only the line
    /// the cursor is on, at the top. On the alternate screen it empties only the scrollback.
    pub fn clear(&mut self) {
        self.terminal.clear();
    }

    pub fn cursor(&self) -> Cursor {
        self.terminal.cursor()
    }

    pub fn modes(&self) -> &Modes {
        self.terminal.modes()
    }

    /// The colors programs set over the theme's, which the Renderer draws with (TERM-12).
    pub fn colors(&self) -> &Colors {
        self.terminal.colors()
    }

    /// The link a cell is part of, as [`Cell::link`] names it (TERM-13).
    pub fn link(&self, link: LinkId) -> Option<&Link> {
        self.terminal.link(link)
    }

    /// Row `number`, as [`Scrollback::start`] numbers the rows: of the scrollback or of the
    /// screen shown. The alternate screen has no scrollback above it. None for a row there is
    /// not.
    pub fn row(&self, number: u64) -> Option<&Row> {
        self.terminal.rows().get(number)
    }

    /// Starts a selection `by` at `at`, in place of the one there was (TERM-17): a press of the
    /// mouse. A word or a line is taken whole. A place outside the rows and columns there are
    /// is taken to the nearest.
    pub fn select(&mut self, at: Place, by: Selecting) {
        self.terminal.select(at, by);
    }

    /// Moves the selection's moving end to `to`, keeping what its start took: a drag. Without a
    /// selection it does nothing.
    pub fn extend(&mut self, to: Place) {
        self.terminal.extend(to);
    }

    /// Selects everything from the oldest row there is to the last of the screen that has
    /// something on it: Select All.
    pub fn select_all(&mut self) {
        self.terminal.select_all();
    }

    pub fn unselect(&mut self) {
        self.terminal.unselect();
    }

    /// What is selected, for the Renderer to draw. None when nothing is. A change to a row it
    /// covers, a resize, Clear, ED 3, RIS and a switch of screen end it. DESIGN.md §3
    /// *Selection* has the rules.
    pub fn selected(&self) -> Option<Selected<'_>> {
        self.terminal.selected()
    }

    /// The text selected, as copying gives it (TERM-17): the characters the program wrote,
    /// rows that wrap joined and the spaces at the end of each line removed. None when nothing
    /// is selected, or nothing but blanks on one row, so that copying them leaves the clipboard
    /// as it was.
    pub fn copy(&self) -> Option<String> {
        let text = self.selected()?.text();
        (!text.is_empty()).then_some(text)
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
