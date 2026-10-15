//! The terminal's state beside its rows: the cursor, the scrolling region, the tab stops, the
//! character sets, the style and the modes, and the operations of TERM-06 that use them.
//!
//! xterm is the model (TERM-01). There is one cursor, and it stays where it is when the screen
//! switches. Each screen keeps a saved cursor of its own. There is one style too, which
//! characters are printed in and whose colors every blank keeps.
//!
//! It also keeps what programs set through operating system commands (TERM-12, TERM-13): the
//! title and the titles saved, the colors set over the theme's, and the link what is printed is
//! part of, with every link the cells hold.

use alloc::boxed::Box;
use alloc::string::String;
use alloc::vec::Vec;
use core::mem;
use core::ops::Range;

use crate::colors::{ColorQuery, Colors};
use crate::keyboard::{Keyboard, Stack};
use crate::links::{Link, LinkId, Links};
use crate::osc::{ColorChange, NewLink, Osc};
use crate::report::Report;
use crate::screen::{Cell, Screen};
use crate::scrollback::{self, Point, Scrollback};
use crate::style::Style;
use crate::width::{discarded, width};
use crate::{Event, Size};

/// How many titles `CSI 22 t` keeps, as xterm's `MAX_SAVED_TITLES`.
const TITLES: usize = 10;

/// Where the cursor is, counted in cells from the top left of the screen.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Cursor {
    pub row: u16,
    pub col: u16,
}

/// The cursor's shape (TERM-07).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CursorShape {
    Block,
    Underline,
    Bar,
}

/// What a program asked the cursor to look like with DECSCUSR (TERM-07).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CursorStyle {
    pub shape: CursorShape,
    pub blinking: bool,
}

/// The modes a program sets that the rest of a tab acts on. The Terminal Core only keeps them.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Modes {
    /// The cursor is shown (DECTCEM, TERM-06).
    pub cursor_visible: bool,
    /// The cursor's look a program asked for (DECSCUSR), or none for the one the settings give
    /// (TERM-07).
    pub cursor_style: Option<CursorStyle>,
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
    /// Which mouse events are reported (TERM-10).
    pub mouse: MouseTracking,
    /// How they are written (TERM-10).
    pub mouse_format: MouseFormat,
    /// The wheel sends the cursor keys on the alternate screen when the mouse is not reported
    /// (1007). On until a program turns it off, where xterm has it off until it is turned on.
    pub alternate_scroll: bool,
    /// How keys are encoded: the kitty keyboard protocol's flags in use on the screen being
    /// shown, or none for xterm's encoding (TERM-08, TERM-09).
    pub keyboard: Keyboard,
}

impl Default for Modes {
    fn default() -> Self {
        Self {
            cursor_visible: true,
            cursor_style: None,
            alternate_screen: false,
            application_cursor: false,
            application_keypad: false,
            newline: false,
            bracketed_paste: false,
            focus: false,
            mouse: MouseTracking::Off,
            mouse_format: MouseFormat::Legacy,
            alternate_scroll: true,
            keyboard: Keyboard::default(),
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

/// How mouse events are written for the program (TERM-10). The formats exclude each other, as
/// xterm's `extend_coords`: setting one makes it the one in use, and resetting one that is not
/// in use does nothing.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum MouseFormat {
    /// `CSI M` and a byte for the button and each coordinate, which stop at 223.
    #[default]
    Legacy,
    /// Those, with coordinates past 95 as two bytes of UTF-8, up to 2015 (mode 1005).
    Utf8,
    /// `CSI < b;x;y M`, and `m` for a release, in decimal: SGR's extended coordinates
    /// (mode 1006).
    Sgr,
    /// `CSI b;x;y M`, in decimal: urxvt's extended coordinates (mode 1015).
    Urxvt,
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
    /// 1005, 1006 and 1015. Never `Legacy`, which no mode names.
    MouseFormat(MouseFormat),
    AlternateScroll,
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
        // Not 3, which would resize the screen to 132 columns and clear it. Not 1001, xterm's
        // highlight tracking, nor 1016, whose reports are in pixels: Twominal reports no pixel
        // sizes, so a program could not tell cells by them.
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
            1005 => Self::MouseFormat(MouseFormat::Utf8),
            1006 => Self::MouseFormat(MouseFormat::Sgr),
            1007 => Self::AlternateScroll,
            1015 => Self::MouseFormat(MouseFormat::Urxvt),
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
    /// The rows that scrolled off the top of the primary screen (TERM-14).
    scrollback: Scrollback,
    row: usize,
    col: usize,
    /// A character was printed in the last column, so the next one starts the next row if
    /// autowrap is on: xterm's pending wrap. The cursor is then in the last column. As in
    /// xterm, it is kept whether autowrap is on or not, and turning autowrap on or off does
    /// not change it.
    pending: bool,
    /// What DECSC saved on the primary screen and on the alternate one.
    saved: [Option<Saved>; 2],
    /// The kitty keyboard protocol's flags pushed on the primary screen and on the alternate
    /// one. `modes.keyboard` is the shown screen's flags in use.
    keyboard: [Stack; 2],
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
    /// The title a program set (OSC 0 and 2), or none for the tab's own.
    title: Option<String>,
    /// The titles `CSI 22 t` saved, the newest last.
    titles: Vec<Option<String>>,
    /// The colors programs set over the theme's (OSC 4, 10 to 12).
    colors: Colors,
    /// The link what is printed is part of (OSC 8).
    link: Option<LinkId>,
    /// Every link a cell may hold.
    links: Links,
    /// An OSC that ended with ESC, which acts only if the next byte is `\`: then the ESC was
    /// the start of ST, and not of another sequence that cut the OSC short.
    waiting: Option<Osc>,
    /// Replies, reports and the rest since the bytes were handed in.
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
            scrollback: Scrollback::new(scrollback::DEFAULT),
            row: 0,
            col: 0,
            pending: false,
            saved: [None, None],
            keyboard: [Stack::default(); 2],
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
            title: None,
            titles: Vec::new(),
            colors: Colors::default(),
            link: None,
            links: Links::default(),
            waiting: None,
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

    pub(crate) fn scrollback(&self) -> &Scrollback {
        &self.scrollback
    }

    /// Keeps no more than `rows` rows of scrollback, and no more than 100,000, dropping the
    /// oldest: the Scrollback setting (TERM-14).
    pub(crate) fn set_scrollback(&mut self, rows: usize) {
        self.scrollback.set_limit(rows);
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

    /// BEL: the program rang the bell (TERM-12).
    pub(crate) fn bell(&mut self) {
        self.events.push(Event::Bell);
    }

    pub(crate) fn colors(&self) -> &Colors {
        &self.colors
    }

    pub(crate) fn link(&self, link: LinkId) -> Option<&Link> {
        self.links.get(link)
    }

    // Operating system commands

    /// Does what an OSC asks for.
    pub(crate) fn act(&mut self, osc: Osc) {
        match osc {
            Osc::Title(title) => self.set_title(title),
            Osc::Directory(directory) => self.events.push(Event::Directory(Box::new(directory))),
            Osc::Colors(changes, end) => {
                for change in changes {
                    match change {
                        ColorChange::Set(color, rgb) => self.colors.set(color, Some(rgb)),
                        ColorChange::Reset(color) => self.colors.set(color, None),
                        ColorChange::ResetPalette => self.colors.reset_palette(),
                        ColorChange::Query(color) => {
                            let set = self.colors.get(color);
                            self.events
                                .push(Event::Color(ColorQuery::new(color, set, end)));
                        }
                    }
                }
            }
            Osc::Link(link) => self.open_link(link),
            Osc::Clipboard(clipboard) => self.events.push(Event::Clipboard(Box::new(clipboard))),
        }
    }

    /// Holds an OSC that ended with ESC until the next byte says whether that was ST.
    pub(crate) fn wait(&mut self, osc: Osc) {
        self.waiting = Some(osc);
    }

    /// Whether an OSC waits for the byte after its ESC.
    pub(crate) fn waiting(&self) -> bool {
        self.waiting.is_some()
    }

    /// Acts on the OSC waiting if its ESC was the start of ST, and else drops it.
    pub(crate) fn settle(&mut self, terminated: bool) {
        if let Some(osc) = self.waiting.take() {
            if terminated {
                self.act(osc);
            }
        }
    }

    fn set_title(&mut self, title: Option<String>) {
        self.title.clone_from(&title);
        self.events.push(Event::Title(title));
    }

    /// `CSI 22 ; which ; at t`: saves the title, for `which` 0 or 2, which name the window's
    /// title, and `at` 0. A full stack drops its oldest, as xterm's ring does. The icon label
    /// alone, and xterm's places on the stack, are not kept.
    pub(crate) fn push_title(&mut self, which: usize, at: usize) {
        if matches!(which, 0 | 2) && at == 0 {
            if self.titles.len() == TITLES {
                self.titles.remove(0);
            }
            self.titles.push(self.title.clone());
        }
    }

    /// `CSI 23 ; which ; at t`: gives back the title saved last, as `push_title` takes them.
    pub(crate) fn pop_title(&mut self, which: usize, at: usize) {
        if matches!(which, 0 | 2) && at == 0 {
            if let Some(title) = self.titles.pop() {
                self.set_title(title);
            }
        }
    }

    /// Makes what is printed from now on part of `link`, or of none. A link that does not fit
    /// in the table, once the links no cell holds are freed, is not made.
    fn open_link(&mut self, link: Option<NewLink>) {
        let Self {
            primary,
            alternate,
            scrollback,
            links,
            ..
        } = self;
        self.link = link.and_then(|link| {
            links.make(&link.uri, link.id.as_deref(), || {
                cells(primary, alternate, scrollback)
            })
        });
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
            self.wrap_early();
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
        self.wrap_line(false);
    }

    /// Starts the next row for a wide character that does not fit in the last column, which
    /// is then not part of the line if it is a space.
    fn wrap_early(&mut self) {
        self.wrap_line(true);
    }

    fn wrap_line(&mut self, early: bool) {
        if mem::take(&mut self.pending) && self.autowrap {
            let row = self.row;
            self.screen_mut().wrap(row, early);
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

    /// Writes `char`, which takes `width` cells, under the cursor in the style and the link in
    /// use, and moves the cursor on: past the character, or in the last column to a pending wrap. The
    /// cells must fit in the row.
    fn write(&mut self, char: char, width: usize) {
        let (row, col, blank) = (self.row, self.col, self.blank());
        let cell = Cell::new(char, width, self.style).linked(self.link);
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
            // goes to the scrollback.
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
    /// row included in the first two, or the scrollback (3), as xterm does on either screen.
    /// Erasing the screen saves none of it in the scrollback, as in xterm without
    /// `cdXtraScroll`.
    pub(crate) fn erase_display(&mut self, which: usize) {
        if which == 3 {
            self.scrollback.clear();
            return;
        }
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

    /// DL: rows come in blank at the bottom of the scrolling region. Only inside it. On the
    /// top row of the primary screen the rows deleted go to the scrollback, as xterm's
    /// `DeleteLine` saves them.
    pub(crate) fn delete_lines(&mut self, count: usize) {
        if self.inside_region() {
            self.scroll_rows_up(self.row..self.bottom + 1, count);
            self.carriage_return();
        }
    }

    /// SU, and IND on the bottom row of the scrolling region.
    pub(crate) fn scroll_up(&mut self, count: usize) {
        self.scroll_rows_up(self.top..self.bottom + 1, count);
    }

    /// Moves the rows of `region` up by `count`, blank rows coming in at its bottom. On the
    /// primary screen the rows that leave the top of the screen go to the scrollback, as xterm's
    /// `xtermScroll` saves them, whatever the region's bottom.
    fn scroll_rows_up(&mut self, region: Range<usize>, count: usize) {
        let blank = self.blank();
        if self.modes.alternate_screen {
            self.alternate.scroll_up(region, count, blank, None);
        } else {
            let scrollback = (region.start == 0).then_some(&mut self.scrollback);
            self.primary.scroll_up(region, count, blank, scrollback);
        }
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

    // The kitty keyboard protocol

    /// `CSI > flags u`, on the stack of the screen being shown.
    pub(crate) fn push_keyboard(&mut self, flags: usize) {
        self.keyboard_stack().push(flags);
        self.keyboard_changed();
    }

    /// `CSI < count u`.
    pub(crate) fn pop_keyboard(&mut self, count: usize) {
        self.keyboard_stack().pop(count);
        self.keyboard_changed();
    }

    /// `CSI = flags ; how u`.
    pub(crate) fn set_keyboard(&mut self, flags: usize, how: usize) {
        self.keyboard_stack().set(flags, how);
        self.keyboard_changed();
    }

    fn keyboard_stack(&mut self) -> &mut Stack {
        &mut self.keyboard[self.modes.alternate_screen as usize]
    }

    /// Shows the flags of the screen being shown, after they or the screen changed.
    fn keyboard_changed(&mut self) {
        self.modes.keyboard = self.keyboard[self.modes.alternate_screen as usize].current();
    }

    // Modes

    pub(crate) fn set_keypad(&mut self, application: bool) {
        self.modes.application_keypad = application;
    }

    /// DECSCUSR (TERM-07): 1 to 6 are xterm's blinking and steady block, underline and bar. 0
    /// and 7 give the cursor back the settings' shape, as kitty, Ghostty, WezTerm, foot and
    /// Alacritty take 0, and as xterm takes 7, its own resources: xterm's 0 is a blinking block.
    /// Any other number does nothing, as in xterm. The shape is kept for both screens, and DECSC
    /// does not save it.
    pub(crate) fn set_cursor_style(&mut self, number: usize) {
        let (shape, blinking) = match number {
            0 | 7 => {
                self.modes.cursor_style = None;
                return;
            }
            1 => (CursorShape::Block, true),
            2 => (CursorShape::Block, false),
            3 => (CursorShape::Underline, true),
            4 => (CursorShape::Underline, false),
            5 => (CursorShape::Bar, true),
            6 => (CursorShape::Bar, false),
            _ => return,
        };
        self.modes.cursor_style = Some(CursorStyle { shape, blinking });
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
            Mode::MouseFormat(format) if on => self.modes.mouse_format = format,
            Mode::MouseFormat(format) => {
                if self.modes.mouse_format == format {
                    self.modes.mouse_format = MouseFormat::Legacy;
                }
            }
            Mode::AlternateScroll => self.modes.alternate_scroll = on,
        }
        // Each screen has flags of its own, so switching changes the ones in use.
        self.keyboard_changed();
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
            Mode::MouseFormat(format) => self.modes.mouse_format == format,
            Mode::AlternateScroll => self.modes.alternate_scroll,
        })
    }

    // Resets

    /// RIS: everything as it was at the start, but the size, the nonce, the scrollback's limit,
    /// which is a setting, and the replies and reports not yet handed out. The scrollback is
    /// emptied, as xterm's RIS empties it, and its rows keep their numbers. As in xterm's
    /// `ReallyReset`, the title and the titles saved stay, and so do the foreground, the
    /// background and the cursor a program set, while the palette goes back to the theme's.
    /// The link ends, and the links go with the cells that held them.
    pub(crate) fn reset(&mut self) {
        let events = mem::take(&mut self.events);
        let nonce = self.nonce.take();
        let mut scrollback = mem::replace(&mut self.scrollback, Scrollback::new(0));
        scrollback.clear();
        let (title, titles) = (self.title.take(), mem::take(&mut self.titles));
        let mut colors = mem::take(&mut self.colors);
        colors.reset_palette();
        *self = Self {
            nonce,
            events,
            scrollback,
            title,
            titles,
            colors,
            ..Self::new(self.size(), None)
        };
    }

    /// Clear (TERM-22): empties the scrollback, and on the primary screen moves the line the
    /// cursor is on, as many of its rows as are on the screen, to the top and blanks the rest in
    /// the default style. The cursor and the saved cursor move with their rows, and a wrap
    /// waiting stays. On the alternate screen the program owns the screen, and would not know
    /// to draw it again, so only the scrollback is emptied.
    pub(crate) fn clear(&mut self) {
        self.scrollback.clear();
        if self.modes.alternate_screen {
            return;
        }
        let rows = self.primary.rows();
        let mut first = self.row;
        while first > 0 && rows[first - 1].wrapped() {
            first -= 1;
        }
        let mut last = self.row;
        while last + 1 < self.rows && rows[last].wrapped() {
            last += 1;
        }
        self.primary.keep(first..last + 1);
        self.row -= first;
        if let Some(saved) = &mut self.saved[0] {
            saved.row = saved.row.saturating_sub(first);
        }
        self.written = self
            .written
            .filter(|(row, _)| (first..=last).contains(row))
            .map(|(row, col)| (row - first, col));
    }

    /// DECSTR, as xterm does it: the modes a program may have left behind go back to their
    /// defaults, autowrap included, but what is on the screen and where the cursor is stay.
    /// The screen being shown forgets its saved cursor; the other keeps its own. The style goes
    /// back to the default whole, as the VT510's does, where xterm's leaves dim, crossed-out and
    /// double-underlined text on. The cursor's shape goes back to the settings', as xterm's
    /// goes back to its resources. The kitty keyboard protocol's flags are forgotten on both
    /// screens, as kitty forgets them. The mouse modes stay, as in xterm, which resets them only
    /// for RIS. As in xterm, the palette goes back to the theme's, and the title and the other
    /// colors a program set stay. The link ends, as VTE ends it.
    pub(crate) fn soft_reset(&mut self) {
        self.modes.cursor_visible = true;
        self.modes.cursor_style = None;
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
        self.keyboard = [Stack::default(); 2];
        self.keyboard_changed();
        self.colors.reset_palette();
        self.link = None;
    }

    // Resizing

    /// Makes the screens `size`. The primary screen's lines are wrapped again to the new width
    /// with its scrollback, and its cursor and saved cursor move with their cells (TERM-16,
    /// DESIGN.md §3 *Reflow*). While the alternate screen is shown, the primary's cursor is the
    /// one saved there. The alternate screen is cut or padded, and loses rows from the top only
    /// as far as needed to keep a row on it: the cursor's while it is shown, the saved cursor's
    /// while not. The scrolling region becomes the whole screen. The same size changes nothing.
    pub(crate) fn resize(&mut self, size: Size) {
        let (cols, rows) = dimensions(size);
        // A window can change size without the grid changing. The program is not told, so
        // nothing it set may change either.
        if (cols, rows) == (self.cols, self.rows) {
            return;
        }
        let alternate = self.modes.alternate_screen;
        let point = |row, col, pending| Some(Point { row, col, pending });
        let mut cursor = point(self.row, self.col, self.pending);
        let mut saved = self.saved[0].and_then(|saved| point(saved.row, saved.col, saved.pending));
        let mut none = None;
        let (primary_cursor, other) = if alternate {
            (&mut saved, &mut none)
        } else {
            (&mut cursor, &mut saved)
        };
        scrollback::reflow(
            &mut self.scrollback,
            &mut self.primary,
            cols,
            rows,
            primary_cursor,
            other,
        );
        if let (Some(slot), Some(point)) = (&mut self.saved[0], saved) {
            slot.row = point.row;
            slot.col = point.col;
            slot.pending = point.pending;
        }

        let keep = if alternate {
            self.row
        } else {
            self.saved[1].map_or(0, |saved| saved.row)
        };
        let from_top = self.alternate.resize(cols, rows, keep);
        match cursor {
            Some(cursor) if !alternate => {
                self.row = cursor.row;
                self.col = cursor.col;
                self.pending = cursor.pending;
            }
            _ => {
                self.row = self.row.saturating_sub(from_top).min(rows - 1);
                self.col = self.col.min(cols - 1);
                self.pending = false;
            }
        }
        self.written = None;
        if let Some(saved) = &mut self.saved[1] {
            saved.row = saved.row.saturating_sub(from_top).min(rows - 1);
            saved.col = saved.col.min(cols - 1);
            saved.pending &= saved.col == cols - 1;
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

    /// Puts an empty scrollback in place of this one, numbered from 0, for a test to compare
    /// the rest of the state.
    #[cfg(test)]
    pub(crate) fn forget_scrollback(&mut self) {
        self.scrollback = Scrollback::new(self.scrollback.limit());
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
        self.scrollback.check(self.cols);
        for saved in self.saved.iter().flatten() {
            assert!(saved.row < self.rows && saved.col < self.cols, "{self:?}");
        }
        for stack in &self.keyboard {
            assert!(stack.len() <= crate::keyboard::DEPTH, "{self:?}");
        }
        assert_eq!(
            self.modes.keyboard,
            self.keyboard[self.modes.alternate_screen as usize].current(),
            "{self:?}"
        );
        if let Some((row, col)) = self.written {
            assert!(row < self.rows && col < self.cols, "{self:?}");
        }
        // A zero-width character is joined, never printed on its own.
        assert!(self.last.is_none_or(|char| width(char) > 0), "{self:?}");
        self.links.check();
        for link in cells(&self.primary, &self.alternate, &self.scrollback)
            .filter_map(Cell::link)
            .chain(self.link)
        {
            assert!(self.links.get(link).is_some(), "{link:?} in {self:?}");
        }
        assert!(self.titles.len() <= TITLES, "{self:?}");
    }
}

/// Every cell of both screens and of the scrollback: those that may hold a link.
fn cells<'a>(
    primary: &'a Screen,
    alternate: &'a Screen,
    scrollback: &'a Scrollback,
) -> impl Iterator<Item = &'a Cell> {
    let kept = (0..scrollback.len()).filter_map(|index| scrollback.get(index));
    primary
        .rows()
        .iter()
        .chain(alternate.rows())
        .chain(kept)
        .flat_map(|row| row.cells())
}

/// A size in cells, never zero.
fn dimensions(size: Size) -> (usize, usize) {
    (usize::from(size.cols.max(1)), usize::from(size.rows.max(1)))
}

/// Whether column `col` has a tab stop at the start: one every 8 columns.
fn default_stop(col: usize) -> bool {
    col.is_multiple_of(8)
}
