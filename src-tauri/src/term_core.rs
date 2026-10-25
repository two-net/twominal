//! Terminal Core: turns `alacritty_terminal` screen state into plain-data frames.
//!
//! Parsing, the grid, scrollback and modes all live in `alacritty_terminal`. This module only
//! extracts what the renderer needs, so it knows nothing about the UI, themes or sessions
//! (ARC-02) and is deterministic for a given byte stream (ARC-03).

use alacritty_terminal::event::EventListener;
use alacritty_terminal::grid::Dimensions;
use alacritty_terminal::index::{Column, Line};
use alacritty_terminal::term::cell::{Cell, Flags};
use alacritty_terminal::term::color::Colors;
use alacritty_terminal::term::{point_to_viewport, TermDamage, TermMode};
use alacritty_terminal::vte::ansi::{Color, CursorShape, NamedColor, Rgb};
use alacritty_terminal::Term;
use serde::Serialize;

/// Grid size in cells.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Size {
    pub cols: usize,
    pub rows: usize,
}

impl Dimensions for Size {
    fn total_lines(&self) -> usize {
        self.rows
    }

    fn screen_lines(&self) -> usize {
        self.rows
    }

    fn columns(&self) -> usize {
        self.cols
    }
}

/// Default foreground colour.
pub const COLOR_FG: i32 = -1;
/// Default background colour.
pub const COLOR_BG: i32 = -2;
/// Marks a 24-bit colour; the low 24 bits hold `0xRRGGBB`.
pub const COLOR_RGB: i32 = 0x100_0000;

/// Cell flags as sent to the renderer. Our own bits, independent of alacritty's layout.
pub mod flag {
    pub const BOLD: u16 = 1;
    pub const DIM: u16 = 1 << 1;
    pub const ITALIC: u16 = 1 << 2;
    pub const UNDERLINE: u16 = 1 << 3;
    pub const DOUBLE_UNDERLINE: u16 = 1 << 4;
    pub const CURLY_UNDERLINE: u16 = 1 << 5;
    pub const DOTTED_UNDERLINE: u16 = 1 << 6;
    pub const DASHED_UNDERLINE: u16 = 1 << 7;
    pub const STRIKEOUT: u16 = 1 << 8;
    pub const INVERSE: u16 = 1 << 9;
    pub const HIDDEN: u16 = 1 << 10;
    pub const WIDE: u16 = 1 << 11;
}

/// Terminal modes the frontend needs for input encoding.
pub mod mode {
    pub const APP_CURSOR: u32 = 1;
    pub const BRACKETED_PASTE: u32 = 1 << 1;
    pub const ALT_SCREEN: u32 = 1 << 2;
    pub const SHOW_CURSOR: u32 = 1 << 3;
    pub const APP_KEYPAD: u32 = 1 << 4;
    pub const FOCUS_IN_OUT: u32 = 1 << 5;
}

/// A run of identically styled cells: `[col, text, fg, bg, flags, underline colour]`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Run(pub usize, pub String, pub i32, pub i32, pub u16, pub i32);

/// One rendered viewport row: `[row, runs]`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Row(pub usize, pub Vec<Run>);

/// Cursor position in the viewport and its shape: `[row, col, shape]`.
///
/// Shapes: 0 block, 1 underline, 2 beam, 3 hollow block.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
pub struct Cursor(pub usize, pub usize, pub u8);

/// Everything the renderer needs to bring its view up to date.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Frame {
    /// Every row is included and the renderer should drop rows it has not been sent.
    pub full: bool,
    pub cols: usize,
    pub rows: usize,
    pub lines: Vec<Row>,
    pub cursor: Option<Cursor>,
    pub mode: u32,
    /// Lines scrolled back into history; 0 means the view follows output.
    pub offset: usize,
}

/// Collects damage since the last call and returns it as a frame, then resets the damage.
///
/// `force_full` sends every row regardless of damage, e.g. when a hidden tab becomes visible.
pub fn frame<T: EventListener>(term: &mut Term<T>, force_full: bool) -> Frame {
    let rows = term.screen_lines();
    let cols = term.columns();

    let damaged: Option<Vec<usize>> = match term.damage() {
        TermDamage::Full => None,
        TermDamage::Partial(lines) => Some(lines.map(|bounds| bounds.line).collect()),
    };
    let full = force_full || damaged.is_none();
    let rows_to_send = match damaged {
        Some(lines) if !full => lines,
        _ => (0..rows).collect(),
    };

    let offset = term.grid().display_offset();
    let lines = rows_to_send
        .into_iter()
        .filter(|&row| row < rows)
        .map(|row| Row(row, row_runs(term, Line(row as i32 - offset as i32))))
        .collect();

    let content = term.renderable_content();
    let cursor = match content.cursor.shape {
        CursorShape::Hidden => None,
        shape => point_to_viewport(offset, content.cursor.point).map(|point| {
            let shape = match shape {
                CursorShape::Underline => 1,
                CursorShape::Beam => 2,
                CursorShape::HollowBlock => 3,
                _ => 0,
            };
            Cursor(point.line, point.column.0, shape)
        }),
    };
    let mode = encode_mode(*term.mode());

    term.reset_damage();
    Frame {
        full,
        cols,
        rows,
        lines,
        cursor,
        mode,
        offset,
    }
}

fn encode_mode(term_mode: TermMode) -> u32 {
    [
        (TermMode::APP_CURSOR, mode::APP_CURSOR),
        (TermMode::BRACKETED_PASTE, mode::BRACKETED_PASTE),
        (TermMode::ALT_SCREEN, mode::ALT_SCREEN),
        (TermMode::SHOW_CURSOR, mode::SHOW_CURSOR),
        (TermMode::APP_KEYPAD, mode::APP_KEYPAD),
        (TermMode::FOCUS_IN_OUT, mode::FOCUS_IN_OUT),
    ]
    .into_iter()
    .filter(|(alacritty, _)| term_mode.contains(*alacritty))
    .fold(0, |bits, (_, ours)| bits | ours)
}

/// Builds the styled runs of one grid line.
///
/// Runs cover the line contiguously from column 0 up to the last visible cell, so the row's text
/// reads the same as the line (copying works). Each wide character gets a run of its own so that
/// fallback-font widths cannot drift the grid.
fn row_runs<T>(term: &Term<T>, line: Line) -> Vec<Run> {
    let colors = term.colors();
    let row = &term.grid()[line];
    let cols = term.columns();

    let last = (0..cols).rev().find(|&col| !is_blank(&row[Column(col)]));
    let Some(last) = last else { return Vec::new() };

    let mut runs: Vec<Run> = Vec::new();
    for col in 0..=last {
        let cell = &row[Column(col)];
        if cell.flags.contains(Flags::WIDE_CHAR_SPACER) {
            continue;
        }

        let fg = encode_color(cell.fg, colors);
        let bg = encode_color(cell.bg, colors);
        let ul = cell
            .underline_color()
            .map_or(COLOR_FG, |color| encode_color(color, colors));
        let flags = encode_flags(cell.flags);

        let mut text = String::new();
        if cell.flags.contains(Flags::LEADING_WIDE_CHAR_SPACER) {
            text.push(' ');
        } else {
            text.push(cell.c);
            if let Some(zerowidth) = cell.zerowidth() {
                text.extend(zerowidth);
            }
        }

        let wide = flags & flag::WIDE != 0;
        match runs.last_mut() {
            Some(run)
                if !wide
                    && run.4 & flag::WIDE == 0
                    && (run.2, run.3, run.4, run.5) == (fg, bg, flags, ul) =>
            {
                run.1.push_str(&text)
            }
            _ => runs.push(Run(col, text, fg, bg, flags, ul)),
        }
    }
    runs
}

/// A cell that renders as nothing: a space on the default background with no decoration.
fn is_blank(cell: &Cell) -> bool {
    let decorations = Flags::INVERSE | Flags::ALL_UNDERLINES | Flags::STRIKEOUT;
    cell.c == ' '
        && cell.bg == Color::Named(NamedColor::Background)
        && !cell.flags.intersects(decorations)
        && cell.zerowidth().is_none()
}

fn encode_flags(flags: Flags) -> u16 {
    [
        (Flags::BOLD, flag::BOLD),
        (Flags::DIM, flag::DIM),
        (Flags::ITALIC, flag::ITALIC),
        (Flags::UNDERLINE, flag::UNDERLINE),
        (Flags::DOUBLE_UNDERLINE, flag::DOUBLE_UNDERLINE),
        (Flags::UNDERCURL, flag::CURLY_UNDERLINE),
        (Flags::DOTTED_UNDERLINE, flag::DOTTED_UNDERLINE),
        (Flags::DASHED_UNDERLINE, flag::DASHED_UNDERLINE),
        (Flags::STRIKEOUT, flag::STRIKEOUT),
        (Flags::INVERSE, flag::INVERSE),
        (Flags::HIDDEN, flag::HIDDEN),
        (Flags::WIDE_CHAR, flag::WIDE),
    ]
    .into_iter()
    .filter(|(alacritty, _)| flags.contains(*alacritty))
    .fold(0, |bits, (_, ours)| bits | ours)
}

/// Encodes a colour as a palette index, a default marker, or 24-bit RGB.
///
/// Palette entries redefined by the program (OSC 4/10/11) are sent as RGB so they win over
/// the theme.
fn encode_color(color: Color, colors: &Colors) -> i32 {
    let rgb = |rgb: Rgb| {
        COLOR_RGB | (i32::from(rgb.r) << 16) | (i32::from(rgb.g) << 8) | i32::from(rgb.b)
    };
    let index = match color {
        Color::Spec(spec) => return rgb(spec),
        Color::Indexed(index) => usize::from(index),
        Color::Named(named) => named as usize,
    };
    if let Some(spec) = colors[index] {
        return rgb(spec);
    }
    match index {
        0..=255 => index as i32,
        i if i == NamedColor::Background as usize => COLOR_BG,
        // Dim variants of the 16 named colours.
        i if (NamedColor::DimBlack as usize..=NamedColor::DimWhite as usize).contains(&i) => {
            (i - NamedColor::DimBlack as usize) as i32
        }
        _ => COLOR_FG,
    }
}

#[cfg(test)]
pub mod test {
    use super::*;
    use alacritty_terminal::event::{Event, VoidListener};
    use alacritty_terminal::term::Config;
    use alacritty_terminal::vte::ansi::Processor;
    use std::sync::{Arc, Mutex};

    /// Collects terminal events so tests can assert on them.
    #[derive(Clone, Default)]
    pub struct Events(pub Arc<Mutex<Vec<Event>>>);

    impl EventListener for Events {
        fn send_event(&self, event: Event) {
            self.0.lock().unwrap().push(event);
        }
    }

    /// Drives a `Term` with the same parser the session event loop uses, without a window or PTY.
    pub struct Harness<T: EventListener> {
        pub term: Term<T>,
        parser: Processor,
    }

    impl Harness<VoidListener> {
        pub fn new(cols: usize, rows: usize) -> Self {
            Self::with_listener(cols, rows, VoidListener)
        }
    }

    impl<T: EventListener> Harness<T> {
        pub fn with_listener(cols: usize, rows: usize, listener: T) -> Self {
            let term = Term::new(Config::default(), &Size { cols, rows }, listener);
            Self {
                term,
                parser: Processor::new(),
            }
        }

        pub fn feed(&mut self, bytes: &[u8]) -> &mut Self {
            self.parser.advance(&mut self.term, bytes);
            self
        }

        pub fn frame(&mut self) -> Frame {
            frame(&mut self.term, false)
        }
    }

    fn row(frame: &Frame, row: usize) -> &[Run] {
        frame
            .lines
            .iter()
            .find(|line| line.0 == row)
            .map(|line| line.1.as_slice())
            .unwrap_or(&[])
    }

    fn text(frame: &Frame, row_index: usize) -> String {
        row(frame, row_index)
            .iter()
            .map(|run| run.1.as_str())
            .collect()
    }

    #[test]
    fn same_bytes_produce_the_same_frame() {
        let bytes = b"hello \x1b[1;31mworld\x1b[0m\r\n\x1b[38;2;1;2;3mrgb\x1b[0m \xe4\xb8\xad!";
        let a = Harness::new(20, 4).feed(bytes).frame();
        let b = Harness::new(20, 4).feed(bytes).frame();
        assert_eq!(a, b);
    }

    #[test]
    fn first_frame_is_full_and_plain_text_uses_default_colors() {
        let frame = Harness::new(10, 3).feed(b"hi").frame();
        assert!(frame.full);
        assert_eq!(frame.lines.len(), 3);
        assert_eq!(
            row(&frame, 0),
            &[Run(0, "hi".into(), COLOR_FG, COLOR_BG, 0, COLOR_FG)]
        );
        assert!(row(&frame, 1).is_empty());
        assert_eq!(frame.cursor, Some(Cursor(0, 2, 0)));
    }

    #[test]
    fn colors_and_flags_are_encoded() {
        let frame = Harness::new(20, 2)
            .feed(b"\x1b[1;31mr\x1b[0m\x1b[38;5;200mi\x1b[0m\x1b[38;2;1;2;3mt\x1b[0m\x1b[4:3mu")
            .frame();
        let runs = row(&frame, 0);
        assert_eq!(
            runs[0],
            Run(0, "r".into(), 1, COLOR_BG, flag::BOLD, COLOR_FG)
        );
        assert_eq!(runs[1].2, 200);
        assert_eq!(runs[2].2, COLOR_RGB | 0x010203);
        assert_eq!(runs[3].4, flag::CURLY_UNDERLINE);
    }

    #[test]
    fn wide_characters_get_their_own_run() {
        let frame = Harness::new(10, 1).feed("a中b".as_bytes()).frame();
        assert_eq!(
            row(&frame, 0),
            &[
                Run(0, "a".into(), COLOR_FG, COLOR_BG, 0, COLOR_FG),
                Run(1, "中".into(), COLOR_FG, COLOR_BG, flag::WIDE, COLOR_FG),
                Run(3, "b".into(), COLOR_FG, COLOR_BG, 0, COLOR_FG),
            ]
        );
    }

    #[test]
    fn inner_blanks_are_kept_so_text_reads_like_the_line() {
        let frame = Harness::new(20, 1).feed(b"a   b   ").frame();
        assert_eq!(text(&frame, 0), "a   b");
    }

    #[test]
    fn later_frames_only_carry_damaged_rows() {
        let mut harness = Harness::new(10, 5);
        harness.feed(b"one").frame();
        let frame = harness.feed(b"\x1b[3;1Hthree").frame();
        assert!(!frame.full);
        let rows: Vec<usize> = frame.lines.iter().map(|line| line.0).collect();
        assert!(rows.contains(&2), "row 2 was written: {rows:?}");
        assert!(!rows.contains(&4), "row 4 was untouched: {rows:?}");
        assert_eq!(text(&frame, 2), "three");
    }

    #[test]
    fn cursor_position_shape_and_modes_follow_the_program() {
        let frame = Harness::new(10, 5)
            .feed(b"\x1b[3;5H\x1b[6 q\x1b[?1h\x1b[?2004h")
            .frame();
        assert_eq!(frame.cursor, Some(Cursor(2, 4, 2)));
        assert_eq!(frame.mode & mode::APP_CURSOR, mode::APP_CURSOR);
        assert_eq!(frame.mode & mode::BRACKETED_PASTE, mode::BRACKETED_PASTE);

        let frame = Harness::new(10, 5).feed(b"\x1b[?25l\x1b[?1049h").frame();
        assert_eq!(frame.cursor, None);
        assert_eq!(frame.mode & mode::ALT_SCREEN, mode::ALT_SCREEN);
    }

    #[test]
    fn osc_titles_become_title_events() {
        let events = Events::default();
        let mut harness = Harness::with_listener(10, 2, events.clone());
        harness.feed(b"\x1b]2;vim main.rs\x07\x1b]0;\x07");
        let titles: Vec<String> = events
            .0
            .lock()
            .unwrap()
            .iter()
            .filter_map(|event| match event {
                Event::Title(title) => Some(title.clone()),
                _ => None,
            })
            .collect();
        assert_eq!(titles, ["vim main.rs", ""]);
    }
}
