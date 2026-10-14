//! The terminal's state beside its rows: the cursor, the scrolling region, the tab stops, the
//! character sets, the style and the modes, and the operations of TERM-06 that use them.
//!
//! xterm is the model (TERM-01). There is one cursor, and it stays where it is when the screen
//! switches. Each screen keeps a saved cursor of its own. There is one style too, which
//! characters are printed in and whose colors every blank keeps.

use alloc::vec::Vec;
use core::mem;

use crate::report::Report;
use crate::screen::{Cell, Screen};
use crate::style::Style;
use crate::width::{discarded, width};
use crate::{Event, Size};

/// Where the cursor is, counted in cells from the top left of the screen.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Cursor {
    pub row: u16,
    pub col: u16,
}

/// The modes a program sets that the rest of a tab acts on. The Terminal Core only keeps them.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Modes {
    /// The cursor is shown (DECTCEM, TERM-06).
    pub cursor_visible: bool,
    /// The alternate screen is shown: a full-screen program is running (TERM-06).
    pub alternate_screen: bool,
    /// The cursor keys send their application sequences (DECCKM, #20).
    pub application_cursor: bool,
    /// The keypad sends its application sequences (DECKPAM, #20).
    pub application_keypad: bool,
    /// Enter sends CR LF, and a line feed also returns the cursor to the first column (LNM).
    pub newline: bool,
    /// Pasted text is bracketed (TERM-11, #20).
    pub bracketed_paste: bool,
    /// Focus in and out is reported (TERM-11, #20).
    pub focus: bool,
    /// Which mouse events are reported (TERM-10, #21).
    pub mouse: MouseTracking,
    /// Mouse events are reported with SGR's extended coordinates (TERM-10, #21).
    pub mouse_sgr: bool,
}

impl Default for Modes {
    fn default() -> Self {
        Self {
            cursor_visible: true,
            alternate_screen: false,
            application_cursor: false,
            application_keypad: false,
            newline: false,
            bracketed_paste: false,
            focus: false,
            mouse: MouseTracking::Off,
            mouse_sgr: false,
        }
    }
}

/// Which mouse events a program asked to be told of (TERM-10).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum MouseTracking {
    #[default]
    Off,
    /// Button presses (mode 9).
    Press,
    /// Presses and releases (mode 1000).
    Click,
    /// Those, and moves while a button is down (mode 1002).
    Drag,
    /// Those, and every move (mode 1003).
    Motion,
}

/// A mode a program sets and resets by its number: with SM and RM, or with DECSET and DECRST
/// for the DEC private modes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Mode {
    Insert,
    Newline,
    ApplicationCursor,
    Origin,
    Autowrap,
    CursorVisible,
    /// 47: switch screens.
    Alternate,
    /// 1047: switch screens, clearing the alternate screen on the way back.
    AlternateCleared,
    /// 1048: save or restore the cursor. Not a state.
    SaveCursor,
    /// 1049: save the cursor and switch to a cleared alternate screen; switch back and
    /// restore it.
    AlternateSaved,
    ApplicationKeypad,
    Focus,
    BracketedPaste,
    Mouse(MouseTracking),
    MouseSgr,
}

impl Mode {
    /// The mode SM and RM name with `number`.
    pub(crate) fn ansi(number: u16) -> Option<Self> {
        // Not 2, which locks the keyboard, nor 12, which would echo input locally.
        Some(match number {
            4 => Self::Insert,
            20 => Self::Newline,
            _ => return None,
        })
    }

    /// The DEC private mode DECSET and DECRST name with `number`.
    pub(crate) fn dec(number: u16) -> Option<Self> {
        // Not 3, which would resize the screen to 132 columns and clear it.
        Some(match number {
            1 => Self::ApplicationCursor,
            6 => Self::Origin,
            7 => Self::Autowrap,
            9 => Self::Mouse(MouseTracking::Press),
            25 => Self::CursorVisible,
            47 => Self::Alternate,
            66 => Self::ApplicationKeypad,
            1000 => Self::Mouse(MouseTracking::Click),
            1002 => Self::Mouse(MouseTracking::Drag),
            1003 => Self::Mouse(MouseTracking::Motion),
            1004 => Self::Focus,
            1006 => Self::MouseSgr,
            1047 => Self::AlternateCleared,
            1048 => Self::SaveCursor,
            1049 => Self::AlternateSaved,
            2004 => Self::BracketedPaste,
            _ => return None,
        })
    }
}

/// A set of graphic characters, which one of G0 to G3 is designated as.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) enum Charset {
    #[default]
    Ascii,
    /// ASCII with `£` for `#`.
    British,
    /// The VT100's line-drawing characters, which ncurses draws boxes with.
    Graphics,
}

impl Charset {
    /// The set an escape sequence names by its final byte.
    pub(crate) fn named(byte: u8) -> Option<Self> {
        Some(match byte {
            b'B' => Self::Ascii,
            b'A' => Self::British,
            b'0' => Self::Graphics,
            _ => return None,
        })
    }

    fn translate(self, char: char) -> char {
        match self {
            Self::Ascii => char,
            Self::British if char == '#' => '£',
            Self::British => char,
            Self::Graphics => graphics(char),
        }
    }
}

/// What `char` is in DEC Special Graphics.
fn graphics(char: char) -> char {
    match char {
        '_' => ' ',
        '`' => '◆',
        'a' => '▒',
        'b' => '␉',
        'c' => '␌',
        'd' => '␍',
        'e' => '␊',
        'f' => '°',
        'g' => '±',
        'h' => '␤',
        'i' => '␋',
        'j' => '┘',
        'k' => '┐',
        'l' => '┌',
        'm' => '└',
        'n' => '┼',
        'o' => '⎺',
        'p' => '⎻',
        'q' => '─',
        'r' => '⎼',
        's' => '⎽',
        't' => '├',
        'u' => '┤',
        'v' => '┴',
        'w' => '┬',
        'x' => '│',
        'y' => '≤',
        'z' => '≥',
        '{' => 'π',
        '|' => '≠',
        '}' => '£',
        '~' => '·',
        _ => char,
    }
}

/// G0 to G3, and which of them characters are printed in.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
struct Charsets {
    sets: [Charset; 4],
    /// Which of `sets` is in use: shifted to with SI, SO, LS2 and LS3.
    used: usize,
}

/// What DECSC saves and DECRC restores.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
struct Saved {
    row: usize,
    col: usize,
    pending: bool,
    origin: bool,
    charsets: Charsets,
    style: Style,
}

/// Everything the Terminal Core knows but the parser's state. The parser drives it through
/// `control.rs`.
#[derive(Debug, PartialEq)]
pub(crate) struct Terminal {
    /// The size, never zero.
    cols: usize,
    rows: usize,
    primary: Screen,
    alternate: Screen,
    row: usize,
    col: usize,
    /// A character was printed in the last column, so the next one starts the next row if
    /// autowrap is on: xterm's pending wrap. The cursor is then in the last column. As in
    /// xterm, it is kept whether autowrap is on or not, and turning autowrap on or off does
    /// not change it.
    pending: bool,
    /// What DECSC saved on the primary screen and on the alternate one.
    saved: [Option<Saved>; 2],
    /// The scrolling region's first and last rows. `top <= bottom < rows`.
    top: usize,
    bottom: usize,
    /// Whether each column has a tab stop.
    tabs: Vec<bool>,
    charsets: Charsets,
    /// What characters are printed in, as SGR last set it (TERM-03 to TERM-05).
    style: Style,
    modes: Modes,
    autowrap: bool,
    /// Rows are counted from the top of the scrolling region, and the cursor stays inside it
    /// (DECOM).
    origin: bool,
    insert: bool,
    /// The last character printed, for REP to repeat. Any other control function forgets it.
    last: Option<char>,
    /// Where the last character was written, until the cursor moves but by tabs: a zero-width
    /// character joins it there. xterm's `char_was_written`, which its `ResetWrap` clears, and
    /// `last_written_row` and `last_written_col`.
    written: Option<(usize, usize)>,
    /// The nonce that marks a report of the shell integration.
    nonce: Option<Vec<u8>>,
    /// Replies and reports since the bytes were handed in.
    events: Vec<Event>,
}

impl Terminal {
    pub(crate) fn new(size: Size, nonce: Option<&str>) -> Self {
        let (cols, rows) = dimensions(size);
        Self {
            cols,
            rows,
            primary: Screen::new(cols, rows),
            alternate: Screen::new(cols, rows),
            row: 0,
            col: 0,
            pending: false,
            saved: [None, None],
            top: 0,
            bottom: rows - 1,
            tabs: (0..cols).map(default_stop).collect(),
            charsets: Charsets::default(),
            style: Style::PLAIN,
            modes: Modes::default(),
            autowrap: true,
            origin: false,
            insert: false,
            last: None,
            written: None,
            nonce: nonce.map(|nonce| nonce.as_bytes().to_vec()),
            events: Vec::new(),
        }
    }

    pub(crate) fn size(&self) -> Size {
        // Both came from a `Size`, so both fit.
        Size {
            cols: self.cols as u16,
            rows: self.rows as u16,
        }
    }

    /// The screen being shown.
    pub(crate) fn screen(&self) -> &Screen {
        if self.modes.alternate_screen {
            &self.alternate
        } else {
            &self.primary
        }
    }

    fn screen_mut(&mut self) -> &mut Screen {
        if self.modes.alternate_screen {
            &mut self.alternate
        } else {
            &mut self.primary
        }
    }

    pub(crate) fn cursor(&self) -> Cursor {
        Cursor {
            row: self.row as u16,
            col: self.col as u16,
        }
    }

    pub(crate) fn modes(&self) -> &Modes {
        &self.modes
    }

    pub(crate) fn nonce(&self) -> Option<&[u8]> {
        self.nonce.as_deref()
    }

    pub(crate) fn reply(&mut self, bytes: Vec<u8>) {
        self.events.push(Event::Reply(bytes));
    }

    pub(crate) fn report(&mut self, report: Report) {
        self.events.push(Event::Report(report));
    }

    pub(crate) fn take_events(&mut self) -> Vec<Event> {
        mem::take(&mut self.events)
    }

    // Printing

    /// Prints `char` at the cursor, in the character set in use. A zero-width character joins
    /// the character before it instead, and a format character xterm discards is discarded.
    pub(crate) fn print_char(&mut self, char: char) {
        let char = self.charsets.sets[self.charsets.used].translate(char);
        match width(char) {
            // Every character discarded has no width, so plain text is not looked up twice.
            // xterm forgets the character REP would repeat.
            0 if discarded(char) => self.forget_last(),
            0 => self.join(char),
            width => {
                self.put(char, width);
                self.last = Some(char);
            }
        }
    }

    /// Prints the last character printed `count` more times (REP), without the zero-width
    /// characters joined to it. xterm prints them all, however many. Once the characters have
    /// reached the bottom of the region and scrolled every row they found there out of it, each
    /// row's worth printed leaves the screen as it was, so a larger count is cut by whole rows
    /// to no more than that: two screenfuls and a row. One screenful and a row would do for
    /// characters that fill their rows, but a wide character on an odd width leaves the last
    /// column of each row as it found it, and in insert mode pushes the row's first cell there.
    pub(crate) fn repeat(&mut self, count: usize) {
        let Some(char) = self.last else { return };
        let width = width(char);
        // How many a row holds. None, for a wide character on a screen one column wide: it is
        // dropped each time, and only the first time does a pending wrap.
        let fit = self.cols / width;
        if fit == 0 {
            self.put(char, width);
            return;
        }
        let enough = fit * (2 * self.rows + 1);
        let mut left = if count > enough {
            enough + (count - enough) % fit
        } else {
            count
        };
        while left > 0 {
            self.wrap();
            // As much of the run as fits in the row at once, so that in insert mode the row
            // moves once, not for every character. A wide character in the last column is put
            // alone, to wrap or be dropped.
            let run = left.min((self.cols - self.col) / width);
            if run == 0 {
                self.put(char, width);
                left -= 1;
                continue;
            }
            self.make_room(run * width);
            for _ in 0..run {
                self.write(char, width);
            }
            left -= run;
        }
    }

    pub(crate) fn forget_last(&mut self) {
        self.last = None;
    }

    /// SUB: shows `␦` where a character was lost, as xterm does for a VT220 and later.
    pub(crate) fn substitute(&mut self) {
        self.put('\u{2426}', 1);
    }

    /// Prints `char`, which takes `width` cells, at the cursor. A wide character in the last
    /// column goes to the next row, as xterm does, and leaves the column as it was. Without
    /// autowrap, or on a screen one column wide, it is dropped.
    fn put(&mut self, char: char, width: usize) {
        self.wrap();
        if self.col + width > self.cols {
            if !self.autowrap || width > self.cols {
                return;
            }
            self.pending = true;
            self.wrap();
        }
        self.make_room(width);
        self.write(char, width);
    }

    /// Joins the zero-width character `mark` to the character written last, until xterm would
    /// forget it (see `written`), or else to the character under the cursor, as xterm does.
    fn join(&mut self, mark: char) {
        let (row, col) = self.written.unwrap_or((self.row, self.col));
        self.screen_mut().join(row, col, mark);
    }

    /// Starts the next row if a wrap is pending and autowrap is on. Without autowrap the
    /// character in the last column is written over.
    fn wrap(&mut self) {
        if mem::take(&mut self.pending) && self.autowrap {
            let row = self.row;
            self.screen_mut().set_wrapped(row, true);
            self.col = 0;
            self.index();
        }
    }

    /// In insert mode, moves the rest of the row right to make room for `count` characters.
    fn make_room(&mut self, count: usize) {
        if self.insert {
            let (row, col, blank) = (self.row, self.col, self.blank());
            self.screen_mut().insert_cells(row, col, count, blank);
        }
    }

    /// Writes `char`, which takes `width` cells, under the cursor in the style in use, and
    /// moves the cursor on: past the character, or in the last column to a pending wrap. The
    /// cells must fit in the row.
    fn write(&mut self, char: char, width: usize) {
        let (row, col, blank) = (self.row, self.col, self.blank());
        let cell = Cell::new(char, width, self.style);
        self.screen_mut().write(row, col, cell, blank);
        self.written = Some((row, col));
        if col + width < self.cols {
            self.col += width;
        } else {
            self.col = self.cols - 1;
            self.pending = true;
        }
    }

    /// What a change blanks cells with: a space in the colors in use, without the attributes,
    /// as xterm's `ClearCells` blanks them. `xterm-256color` tells programs so (`bce`).
    fn blank(&self) -> Cell {
        Cell::blank(self.style.colors())
    }

    /// xterm's `ResetWrap`: cancels a pending wrap, and from now on a zero-width character joins
    /// the character under the cursor, not the one written last.
    fn reset_wrap(&mut self) {
        self.pending = false;
        self.written = None;
    }

    // Moving the cursor

    pub(crate) fn carriage_return(&mut self) {
        self.col = 0;
        self.reset_wrap();
    }

    /// Moves the cursor left, and no further than the first column. From a pending wrap that
    /// is the column before the last, as in xterm.
    pub(crate) fn backspace(&mut self) {
        self.move_back(1);
    }

    /// Moves the cursor down a row, scrolling the region when it is on its bottom row (IND).
    pub(crate) fn index(&mut self) {
        self.reset_wrap();
        if self.row == self.bottom {
            // On the primary screen, with the region at the top, the row that scrolls off
            // will go to the scrollback (#26).
            self.scroll_up(1);
        } else if self.row + 1 < self.rows {
            self.row += 1;
        }
    }

    /// Moves the cursor up a row, scrolling the region down when it is on its top row (RI).
    pub(crate) fn reverse_index(&mut self) {
        self.reset_wrap();
        if self.row == self.top {
            self.scroll_down(1);
        } else if self.row > 0 {
            self.row -= 1;
        }
    }

    /// LF, VT and FF: an index, and a carriage return as well in newline mode (LNM).
    pub(crate) fn line_feed(&mut self) {
        self.index();
        if self.modes.newline {
            self.col = 0;
        }
    }

    /// NEL.
    pub(crate) fn next_line(&mut self) {
        self.index();
        self.col = 0;
    }

    /// CUU. The cursor stops at the top of the scrolling region if it starts inside it.
    pub(crate) fn move_up(&mut self, count: usize) {
        let limit = if self.row >= self.top { self.top } else { 0 };
        self.row = self.row.saturating_sub(count).max(limit);
        self.reset_wrap();
    }

    /// CUD. The cursor stops at the bottom of the scrolling region if it starts inside it.
    pub(crate) fn move_down(&mut self, count: usize) {
        let limit = if self.row <= self.bottom {
            self.bottom
        } else {
            self.rows - 1
        };
        self.row = self.row.saturating_add(count).min(limit);
        self.reset_wrap();
    }

    pub(crate) fn move_forward(&mut self, count: usize) {
        self.col = self.col.saturating_add(count).min(self.cols - 1);
        self.reset_wrap();
    }

    pub(crate) fn move_back(&mut self, count: usize) {
        self.col = self.col.saturating_sub(count);
        self.reset_wrap();
    }

    /// Moves the cursor to column `col` of its row, counted from 0.
    pub(crate) fn set_col(&mut self, col: usize) {
        self.col = col.min(self.cols - 1);
        self.reset_wrap();
    }

    /// Moves the cursor to row `row`, counted from 0, and from the top of the scrolling region
    /// in origin mode.
    pub(crate) fn set_row(&mut self, row: usize) {
        self.row = if self.origin {
            self.top.saturating_add(row).min(self.bottom)
        } else {
            row.min(self.rows - 1)
        };
        self.reset_wrap();
    }

    pub(crate) fn goto(&mut self, row: usize, col: usize) {
        self.set_row(row);
        self.set_col(col);
    }

    /// Where the cursor is, as CPR reports it: counted from 1, and from the top of the
    /// scrolling region in origin mode.
    pub(crate) fn position(&self) -> (usize, usize) {
        let row = if self.origin {
            self.row.saturating_sub(self.top)
        } else {
            self.row
        };
        (row + 1, self.col + 1)
    }

    // Tab stops

    /// HT and CHT: on to the next tab stop, or to the last column.
    pub(crate) fn tab_forward(&mut self, count: usize) {
        for _ in 0..count.min(self.cols) {
            match (self.col + 1..self.cols).find(|&col| self.tabs[col]) {
                Some(col) => self.col = col,
                None => {
                    self.col = self.cols - 1;
                    break;
                }
            }
        }
    }

    /// CBT: back to the tab stop before, or to the first column. The cursor leaves the last
    /// column, so no wrap waits; but as after a tab, which xterm's CBT is like, a zero-width
    /// character still joins the character written last.
    pub(crate) fn tab_backward(&mut self, count: usize) {
        self.pending = false;
        for _ in 0..count.min(self.cols) {
            match (0..self.col).rev().find(|&col| self.tabs[col]) {
                Some(col) => self.col = col,
                None => {
                    self.col = 0;
                    break;
                }
            }
        }
    }

    pub(crate) fn set_tab_stop(&mut self) {
        self.tabs[self.col] = true;
    }

    pub(crate) fn clear_tab_stop(&mut self) {
        self.tabs[self.col] = false;
    }

    pub(crate) fn clear_tab_stops(&mut self) {
        self.tabs.fill(false);
    }

    /// A tab stop every 8 columns, as at the start (DECST8C).
    pub(crate) fn reset_tab_stops(&mut self) {
        for (col, stop) in self.tabs.iter_mut().enumerate() {
            *stop = default_stop(col);
        }
    }

    // Erasing, inserting and deleting

    /// ED: erases below the cursor (0), above it (1), or all of the screen (2), the cursor's
    /// row included in the first two. The scrollback (3) is #26's.
    pub(crate) fn erase_display(&mut self, which: usize) {
        let (row, col, rows, cols) = (self.row, self.col, self.rows, self.cols);
        let blank = self.blank();
        let screen = self.screen_mut();
        match which {
            0 => {
                screen.erase(row, col..cols, blank);
                screen.erase_rows(row + 1..rows, blank);
            }
            1 => {
                screen.erase_rows(0..row, blank);
                screen.erase(row, 0..col + 1, blank);
            }
            2 => screen.erase_rows(0..rows, blank),
            _ => return,
        }
        self.reset_wrap();
    }

    /// EL: erases the cursor's row to the right of the cursor (0), to its left (1), or all of
    /// it (2), the cursor's cell included.
    pub(crate) fn erase_line(&mut self, which: usize) {
        let (row, col, cols) = (self.row, self.col, self.cols);
        let cols = match which {
            0 => col..cols,
            1 => 0..col + 1,
            2 => 0..cols,
            _ => return,
        };
        let blank = self.blank();
        self.screen_mut().erase(row, cols, blank);
        self.reset_wrap();
    }

    /// ECH: erases `count` cells from the cursor on, without moving it.
    pub(crate) fn erase_chars(&mut self, count: usize) {
        let (row, col, blank) = (self.row, self.col, self.blank());
        self.screen_mut()
            .erase(row, col..col.saturating_add(count), blank);
        self.reset_wrap();
    }

    /// ICH.
    pub(crate) fn insert_chars(&mut self, count: usize) {
        let (row, col, blank) = (self.row, self.col, self.blank());
        self.screen_mut().insert_cells(row, col, count, blank);
        self.reset_wrap();
    }

    /// DCH.
    pub(crate) fn delete_chars(&mut self, count: usize) {
        let (row, col, blank) = (self.row, self.col, self.blank());
        self.screen_mut().delete_cells(row, col, count, blank);
        self.reset_wrap();
    }

    /// IL: rows pushed past the bottom of the scrolling region are lost. Only inside it.
    pub(crate) fn insert_lines(&mut self, count: usize) {
        if self.inside_region() {
            let (region, blank) = (self.row..self.bottom + 1, self.blank());
            self.screen_mut().scroll_down(region, count, blank);
            self.carriage_return();
        }
    }

    /// DL: rows come in blank at the bottom of the scrolling region. Only inside it.
    pub(crate) fn delete_lines(&mut self, count: usize) {
        if self.inside_region() {
            let (region, blank) = (self.row..self.bottom + 1, self.blank());
            self.screen_mut().scroll_up(region, count, blank);
            self.carriage_return();
        }
    }

    /// SU, and IND on the bottom row of the scrolling region.
    pub(crate) fn scroll_up(&mut self, count: usize) {
        let (region, blank) = (self.top..self.bottom + 1, self.blank());
        self.screen_mut().scroll_up(region, count, blank);
    }

    /// SD, and RI on the top row of the scrolling region.
    pub(crate) fn scroll_down(&mut self, count: usize) {
        let (region, blank) = (self.top..self.bottom + 1, self.blank());
        self.screen_mut().scroll_down(region, count, blank);
    }

    fn inside_region(&self) -> bool {
        (self.top..=self.bottom).contains(&self.row)
    }

    /// DECSTBM, with the rows counted from 1. 0 means the first row for `top`, and the last
    /// one for `bottom`, as does a `bottom` below the screen. A region of less than two rows
    /// is ignored. The cursor goes home.
    pub(crate) fn set_region(&mut self, top: usize, bottom: usize) {
        let top = top.max(1) - 1;
        let bottom = match bottom {
            0 => self.rows,
            bottom => bottom.min(self.rows),
        } - 1;
        if top < bottom {
            self.top = top;
            self.bottom = bottom;
            self.goto(0, 0);
        }
    }

    /// DECALN: fills the screen with `E`, for aligning a CRT. Seldom used but by tests. As in
    /// xterm, the `E`s have the default style, the style in use keeps only its colors, the
    /// scrolling region becomes the whole screen and origin mode goes off.
    pub(crate) fn align(&mut self) {
        self.screen_mut().fill(Cell::new('E', 1, Style::PLAIN));
        self.style = self.style.colors();
        self.top = 0;
        self.bottom = self.rows - 1;
        self.origin = false;
        self.row = 0;
        self.col = 0;
        self.reset_wrap();
    }

    // The cursor's memory

    /// DECSC, into the slot of the screen being shown.
    pub(crate) fn save_cursor(&mut self) {
        let slot = self.modes.alternate_screen as usize;
        self.saved[slot] = Some(Saved {
            row: self.row,
            col: self.col,
            pending: self.pending,
            origin: self.origin,
            charsets: self.charsets,
            style: self.style,
        });
    }

    /// DECRC, from the slot of the screen being shown. With nothing saved there, the cursor
    /// goes home with ASCII, the default style and origin mode off.
    pub(crate) fn restore_cursor(&mut self) {
        let slot = self.modes.alternate_screen as usize;
        let saved = self.saved[slot].unwrap_or_default();
        self.row = saved.row.min(self.rows - 1);
        self.col = saved.col.min(self.cols - 1);
        // xterm moves the cursor with a ResetWrap, then gives the pending wrap back.
        self.reset_wrap();
        self.pending = saved.pending && self.col == self.cols - 1;
        self.origin = saved.origin;
        self.charsets = saved.charsets;
        self.style = saved.style;
    }

    // Character sets

    /// Designates `charset` as G0 to G3, by `which`.
    pub(crate) fn designate(&mut self, which: usize, charset: Charset) {
        self.charsets.sets[which] = charset;
    }

    /// Prints in G0 to G3, by `which`: SI, SO, LS2 and LS3.
    pub(crate) fn shift(&mut self, which: usize) {
        self.charsets.used = which;
    }

    // The style

    /// SGR, with the parameters and sub-parameters of each of `params`.
    pub(crate) fn select_style<'a>(&mut self, params: impl IntoIterator<Item = &'a [u16]>) {
        self.style.select(params);
    }

    // Modes

    pub(crate) fn set_keypad(&mut self, application: bool) {
        self.modes.application_keypad = application;
    }

    pub(crate) fn set_mode(&mut self, mode: Mode, on: bool) {
        match mode {
            Mode::Insert => self.insert = on,
            Mode::Newline => self.modes.newline = on,
            Mode::ApplicationCursor => self.modes.application_cursor = on,
            Mode::Origin => {
                self.origin = on;
                self.goto(0, 0);
            }
            Mode::Autowrap => self.autowrap = on,
            Mode::CursorVisible => self.modes.cursor_visible = on,
            Mode::Alternate => self.modes.alternate_screen = on,
            Mode::AlternateCleared => {
                if !on && self.modes.alternate_screen {
                    let blank = self.blank();
                    self.alternate.erase_rows(0..self.rows, blank);
                    // xterm clears it with ClearScreen, which starts with a ResetWrap.
                    self.reset_wrap();
                }
                self.modes.alternate_screen = on;
            }
            Mode::SaveCursor if on => self.save_cursor(),
            Mode::SaveCursor => self.restore_cursor(),
            Mode::AlternateSaved if on => {
                self.save_cursor();
                self.modes.alternate_screen = true;
                let blank = self.blank();
                self.alternate.erase_rows(0..self.rows, blank);
                self.reset_wrap();
            }
            Mode::AlternateSaved => {
                self.modes.alternate_screen = false;
                self.restore_cursor();
            }
            Mode::ApplicationKeypad => self.modes.application_keypad = on,
            Mode::Focus => self.modes.focus = on,
            Mode::BracketedPaste => self.modes.bracketed_paste = on,
            // Turning off any of them turns off mouse reporting, as in xterm.
            Mode::Mouse(tracking) => {
                self.modes.mouse = if on { tracking } else { MouseTracking::Off }
            }
            Mode::MouseSgr => self.modes.mouse_sgr = on,
        }
    }

    /// Whether `mode` is set, for DECRQM. `None` for 1048, which is an action, not a state.
    pub(crate) fn mode(&self, mode: Mode) -> Option<bool> {
        Some(match mode {
            Mode::Insert => self.insert,
            Mode::Newline => self.modes.newline,
            Mode::ApplicationCursor => self.modes.application_cursor,
            Mode::Origin => self.origin,
            Mode::Autowrap => self.autowrap,
            Mode::CursorVisible => self.modes.cursor_visible,
            Mode::Alternate | Mode::AlternateCleared | Mode::AlternateSaved => {
                self.modes.alternate_screen
            }
            Mode::SaveCursor => return None,
            Mode::ApplicationKeypad => self.modes.application_keypad,
            Mode::Focus => self.modes.focus,
            Mode::BracketedPaste => self.modes.bracketed_paste,
            Mode::Mouse(tracking) => self.modes.mouse == tracking,
            Mode::MouseSgr => self.modes.mouse_sgr,
        })
    }

    // Resets

    /// RIS: everything as it was at the start, but the size, the nonce, and the replies and
    /// reports not yet handed out.
    pub(crate) fn reset(&mut self) {
        let events = mem::take(&mut self.events);
        let nonce = self.nonce.take();
        *self = Self {
            nonce,
            events,
            ..Self::new(self.size(), None)
        };
    }

    /// DECSTR, as xterm does it: the modes a program may have left behind go back to their
    /// defaults, autowrap included, but what is on the screen and where the cursor is stay.
    /// The screen being shown forgets its saved cursor; the other keeps its own. The style goes
    /// back to the default whole, as the VT510's does, where xterm's leaves dim, crossed-out and
    /// double-underlined text on.
    pub(crate) fn soft_reset(&mut self) {
        self.modes.cursor_visible = true;
        self.modes.application_cursor = false;
        self.modes.application_keypad = false;
        self.insert = false;
        self.origin = false;
        self.autowrap = true;
        self.top = 0;
        self.bottom = self.rows - 1;
        self.charsets = Charsets::default();
        self.style = Style::PLAIN;
        self.saved[self.modes.alternate_screen as usize] = None;
    }

    // Resizing

    /// Makes the screens `size`, without reflowing (#26). On each screen rows go from the top
    /// only as far as needed to keep a row on it: the cursor's on the screen being shown, the
    /// saved cursor's on the other. The scrolling region becomes the whole screen. The same
    /// size changes nothing.
    pub(crate) fn resize(&mut self, size: Size) {
        let (cols, rows) = dimensions(size);
        // A window can change size without the grid changing. The program is not told, so
        // nothing it set may change either.
        if (cols, rows) == (self.cols, self.rows) {
            return;
        }
        let alternate = self.modes.alternate_screen;
        let saved_row = |slot: usize| self.saved[slot].map_or(0, |saved| saved.row);
        let keep = if alternate {
            [saved_row(0), self.row]
        } else {
            [self.row, saved_row(1)]
        };
        let from_top = [
            self.primary.resize(cols, rows, keep[0]),
            self.alternate.resize(cols, rows, keep[1]),
        ];

        self.row = self
            .row
            .saturating_sub(from_top[alternate as usize])
            .min(rows - 1);
        self.col = self.col.min(cols - 1);
        self.reset_wrap();
        for (saved, from_top) in self.saved.iter_mut().zip(from_top) {
            if let Some(saved) = saved {
                saved.row = saved.row.saturating_sub(from_top).min(rows - 1);
                saved.col = saved.col.min(cols - 1);
                saved.pending &= saved.col == cols - 1;
            }
        }
        self.top = 0;
        self.bottom = rows - 1;
        let had = self.tabs.len();
        self.tabs.resize(cols, false);
        for col in had..cols {
            self.tabs[col] = default_stop(col);
        }
        self.cols = cols;
        self.rows = rows;
    }

    /// Panics if the state breaks a rule the rest of this file relies on.
    #[cfg(any(test, fuzzing))]
    pub(crate) fn check(&self) {
        assert!(self.row < self.rows && self.col < self.cols, "{self:?}");
        assert!(!self.pending || self.col == self.cols - 1, "{self:?}");
        assert!(
            self.top <= self.bottom && self.bottom < self.rows,
            "{self:?}"
        );
        assert_eq!(self.tabs.len(), self.cols);
        assert!(self.charsets.used < 4);
        for screen in [&self.primary, &self.alternate] {
            assert_eq!(screen.rows().len(), self.rows);
            screen.check(self.cols);
        }
        for saved in self.saved.iter().flatten() {
            assert!(saved.row < self.rows && saved.col < self.cols, "{self:?}");
        }
        if let Some((row, col)) = self.written {
            assert!(row < self.rows && col < self.cols, "{self:?}");
        }
        // A zero-width character is joined, never printed on its own.
        assert!(self.last.is_none_or(|char| width(char) > 0), "{self:?}");
    }
}

/// A size in cells, never zero.
fn dimensions(size: Size) -> (usize, usize) {
    (usize::from(size.cols.max(1)), usize::from(size.rows.max(1)))
}

/// Whether column `col` has a tab stop at the start: one every 8 columns.
fn default_stop(col: usize) -> bool {
    col.is_multiple_of(8)
}
