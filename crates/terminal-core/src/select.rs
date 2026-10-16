//! The selection (TERM-17): what the user selected, by characters, words, lines or as a block,
//! and the text copying it gives.
//!
//! A selection is kept by the numbers of its rows, as the scrollback numbers them, so it stays
//! on its text while output scrolls it up the screen and into the scrollback. What a word and a
//! line take is worked out when the selection is made or moved, and kept: a change to the rows
//! it covers ends it (`terminal.rs`), and the rows beside it cannot change what it holds.
//!
//! Copying takes the characters from the cells, never from what is drawn, so it gives back what
//! the program wrote, ligatures and all (FONT-08): the marks joined to a character, ZWJ and the
//! variation selectors included. Rows that wrap are joined, and the spaces at the end of each
//! line are removed. DESIGN.md §3 *Selection* has the rules.

use alloc::string::String;
use core::ops::{Range, RangeInclusive};

use crate::rows::Rows;
use crate::screen::{Cell, Row};

/// What a word does not hold, besides blanks: Alacritty's `semantic_escape_chars`. So a path
/// and a URL's parts are words, and `file.rs:12` is two.
const SEPARATORS: &str = ",│`|:\"'()[]{}<>";

/// How a selection grows from where it starts (TERM-17).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Selecting {
    /// From a boundary between two cells to another: a drag.
    Characters,
    /// Whole words: a double-click, and the drag after it.
    Words,
    /// Whole lines, every row of a line that wraps: a triple-click, and the drag after it.
    Lines,
    /// The rows between two corners, and the columns between them: a drag with the OS's
    /// modifier for it (Appendix A).
    Block,
}

/// Where the pointer is, as a selection takes it: a row by its number, as
/// [`Scrollback::start`] counts them, a column and which half of the cell. While dragging past
/// an edge it may be outside the rows and columns there are, and is taken to the nearest.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Place {
    pub row: i64,
    pub col: i32,
    pub right_half: bool,
}

/// A boundary between two cells: before cell `col` of row `row`. `col` may be the grid's width,
/// after its last cell.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
struct Spot {
    row: u64,
    col: usize,
}

/// A selection as the Terminal Core keeps it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Selection {
    by: Selecting,
    /// What the press took: the boundary it fell on, as an empty span, or the word or line it
    /// fell in. A selection moved always holds it.
    anchor: (Spot, Spot),
    /// From where to where it goes: in reading order, but for a block, whose two corners these
    /// are.
    from: Spot,
    to: Spot,
}

impl Rows<'_> {
    /// The boundary `at` falls on, taken to the nearest there is. Above the first row it is
    /// that row's start and below the last that row's end, as a drag past the edge selects to
    /// them; for a block the row and the column are each taken to their nearest.
    fn boundary(&self, at: Place, block: bool) -> Spot {
        let col = |at: Place| {
            let col = i64::from(at.col) + i64::from(at.right_half);
            col.clamp(0, self.cols as i64) as usize
        };
        let (first, last) = (self.first(), self.last());
        match u64::try_from(at.row) {
            Ok(row) if (first..=last).contains(&row) => Spot { row, col: col(at) },
            Ok(row) if row > last => Spot {
                row: last,
                col: if block { col(at) } else { self.cols },
            },
            _ => Spot {
                row: first,
                col: if block { col(at) } else { 0 },
            },
        }
    }

    /// The cell `at` falls on, taken to the nearest there is as its boundary is: above the first
    /// row that row's first cell, and below the last row that row's last cell.
    fn cell_at(&self, at: Place) -> (u64, usize) {
        let (first, last) = (self.first(), self.last());
        match u64::try_from(at.row) {
            Ok(row) if (first..=last).contains(&row) => (
                row,
                i64::from(at.col).clamp(0, self.cols as i64 - 1) as usize,
            ),
            Ok(row) if row > last => (last, self.cols - 1),
            _ => (first, 0),
        }
    }

    /// The word the cell at `col` of row `number` is in, from its first boundary to its last: a
    /// run of word characters, which may go on over the rows a line wraps into. A blank or a
    /// separator is a word of its own.
    fn word(&self, number: u64, col: usize) -> (Spot, Spot) {
        (self.word_start(number, col), self.word_end(number, col))
    }

    /// The boundary before the word the cell at `col` of row `number` is in.
    fn word_start(&self, number: u64, col: usize) -> Spot {
        let col = self.left_half(number, col);
        let mut first = (number, col);
        if self.in_word(number, col) {
            while let Some(before) = self.before(first.0, first.1) {
                if !self.in_word(before.0, before.1) {
                    break;
                }
                first = before;
            }
        }
        Spot {
            row: first.0,
            col: first.1,
        }
    }

    /// The boundary after the word the cell at `col` of row `number` is in.
    fn word_end(&self, number: u64, col: usize) -> Spot {
        let col = self.left_half(number, col);
        if !self.in_word(number, col) {
            let width = self.cell(number, col).map_or(1, |cell| cell.width().max(1));
            return Spot {
                row: number,
                col: (col + width).min(self.cols),
            };
        }
        let mut last = (number, col);
        while let Some(after) = self.after(last.0, last.1) {
            if !self.in_word(after.0, after.1) {
                break;
            }
            last = after;
        }
        Spot {
            row: last.0,
            col: last.1 + 1,
        }
    }

    /// Whether the cell at `col` of row `number` is part of a word: neither a blank nor a
    /// separator. A wide character's right half is when its character is.
    fn in_word(&self, number: u64, col: usize) -> bool {
        let Some(cell) = self.cell(number, col) else {
            return false;
        };
        match cell.chars().next() {
            None => col > 0 && self.in_word(number, col - 1),
            Some(char) => !char.is_whitespace() && !SEPARATORS.contains(char),
        }
    }

    /// The line row `number` is part of, from the start of its first row to the end of its
    /// last.
    fn line(&self, number: u64) -> (Spot, Spot) {
        let mut first = number;
        while first > self.first() && self.get(first - 1).is_some_and(Row::wrapped) {
            first -= 1;
        }
        let mut last = number;
        while last < self.last() && self.get(last).is_some_and(Row::wrapped) {
            last += 1;
        }
        (
            Spot { row: first, col: 0 },
            Spot {
                row: last,
                col: self.cols,
            },
        )
    }

    /// The span a press at `at` takes, for a selection `by`.
    fn anchor(&self, at: Place, by: Selecting) -> (Spot, Spot) {
        match by {
            Selecting::Characters | Selecting::Block => {
                let spot = self.boundary(at, by == Selecting::Block);
                (spot, spot)
            }
            Selecting::Words => {
                let (row, col) = self.cell_at(at);
                self.word(row, col)
            }
            Selecting::Lines => self.line(self.cell_at(at).0),
        }
    }

    /// The last row of the screen with something on it, or its first row when none has. Blanks
    /// in a color, as a program that paints its background leaves them, are nothing.
    fn last_used(&self) -> u64 {
        let used = self
            .screen
            .rows()
            .iter()
            .rposition(|row| {
                row.wrapped()
                    || row
                        .cells()
                        .iter()
                        .flat_map(Cell::chars)
                        .any(|char| char != ' ')
            })
            .unwrap_or(0);
        self.live() + used as u64
    }
}

impl Selection {
    /// A selection `by`, started at `at`.
    pub(crate) fn new(rows: &Rows, at: Place, by: Selecting) -> Self {
        let anchor = rows.anchor(at, by);
        Self {
            by,
            anchor,
            from: anchor.0,
            to: anchor.1,
        }
    }

    /// Everything from the oldest row to the last of the screen that has something on it:
    /// Select All.
    pub(crate) fn all(rows: &Rows) -> Self {
        let from = Spot {
            row: rows.first(),
            col: 0,
        };
        let to = Spot {
            row: rows.last_used(),
            col: rows.cols,
        };
        Self {
            by: Selecting::Characters,
            anchor: (from, from),
            from,
            to,
        }
    }

    /// Moves the end that moves to `to`. The anchor stays selected, and a word or a line is
    /// taken whole. A word is looked for only on the side the end moved to, since one can take
    /// as many rows as the scrollback keeps, and a drag moves it at every step.
    pub(crate) fn extend(&mut self, rows: &Rows, to: Place) {
        let (start, end) = self.anchor;
        let (from, to) = match self.by {
            Selecting::Block => (start, rows.boundary(to, true)),
            Selecting::Characters => {
                let spot = rows.boundary(to, false);
                (spot.min(start), spot.max(start))
            }
            Selecting::Words | Selecting::Lines => {
                let (row, col) = rows.cell_at(to);
                let cell = Spot { row, col };
                let words = self.by == Selecting::Words;
                if cell < start {
                    let first = if words {
                        rows.word_start(row, col)
                    } else {
                        rows.line(row).0
                    };
                    (first, end)
                } else if cell >= end {
                    let last = if words {
                        rows.word_end(row, col)
                    } else {
                        rows.line(row).1
                    };
                    (start, last)
                } else {
                    (start, end)
                }
            }
        };
        self.from = from;
        self.to = to;
    }

    /// The numbers of the first and the last row it reaches.
    pub(crate) fn rows(&self) -> RangeInclusive<u64> {
        self.from.row.min(self.to.row)..=self.from.row.max(self.to.row)
    }

    /// Whether it may still be one of the rows there are, and of the columns: what
    /// `Terminal::check` asks.
    #[cfg(any(test, fuzzing))]
    pub(crate) fn check(&self, rows: &Rows) {
        for spot in [self.from, self.to, self.anchor.0, self.anchor.1] {
            assert!(spot.col <= rows.cols, "{self:?}");
            assert!(spot.row <= rows.last(), "{self:?}");
            if rows.alternate {
                assert!(spot.row >= rows.live(), "{self:?}");
            }
        }
        assert!(self.anchor.0 <= self.anchor.1, "{self:?}");
        if self.by != Selecting::Block {
            assert!(self.from <= self.to, "{self:?}");
        }
    }

    /// What it selects of the rows there are, or None when that is nothing. The rows dropped
    /// past the scrollback's limit are cut from its start.
    pub(crate) fn selected<'a>(&self, rows: Rows<'a>) -> Option<Selected<'a>> {
        let first = rows.first();
        let block = self.by == Selecting::Block;
        let (from, to) = if block {
            (
                Spot {
                    row: self.from.row.min(self.to.row),
                    col: self.from.col.min(self.to.col),
                },
                Spot {
                    row: self.from.row.max(self.to.row),
                    col: self.from.col.max(self.to.col),
                },
            )
        } else {
            (self.from, self.to)
        };
        if to.row < first || from == to || (block && from.col == to.col) {
            return None;
        }
        let from = if from.row >= first {
            from
        } else if block {
            Spot { row: first, ..from }
        } else {
            Spot { row: first, col: 0 }
        };
        Some(Selected {
            rows,
            block,
            from,
            to,
        })
    }
}

/// What is selected: the rows, and the columns of each, for the Renderer to draw and for
/// copying.
#[derive(Clone, Copy)]
pub struct Selected<'a> {
    rows: Rows<'a>,
    block: bool,
    /// In reading order, and for a block its top left and bottom right corners.
    from: Spot,
    to: Spot,
}

impl Selected<'_> {
    /// The numbers of the rows it reaches.
    pub fn rows(&self) -> RangeInclusive<u64> {
        self.from.row..=self.to.row
    }

    /// The cells of row `number` it holds: none for a row it does not reach. A wide character
    /// is taken whole when either of its cells is.
    pub fn cols(&self, number: u64) -> Range<usize> {
        if !self.rows().contains(&number) {
            return 0..0;
        }
        let cols = if self.block {
            self.from.col..self.to.col
        } else {
            let start = if number == self.from.row {
                self.from.col
            } else {
                0
            };
            let end = if number == self.to.row {
                self.to.col
            } else {
                self.rows.cols
            };
            start..end
        };
        match self.rows.get(number) {
            Some(row) => whole(row, cols),
            None => cols,
        }
    }

    /// The text it holds, as copying gives it: rows that wrap joined, other rows' ends as line
    /// feeds, and the spaces at the end of each line removed. A block's rows each end a line.
    pub fn text(&self) -> String {
        let mut text = String::new();
        for number in self.rows() {
            let Some(row) = self.rows.get(number) else {
                continue;
            };
            let cols = self.cols(number);
            let line = row.line();
            let cells = &line[cols.start.min(line.len())..cols.end.min(line.len())];
            text.extend(cells.iter().flat_map(Cell::chars));
            if number == self.to.row {
                break;
            }
            if self.block || !row.wrapped() {
                trim_end(&mut text);
                text.push('\n');
            }
        }
        trim_end(&mut text);
        text
    }
}

/// `cols`, widened to take whole the wide characters it holds half of in `row`.
fn whole(row: &Row, cols: Range<usize>) -> Range<usize> {
    if cols.is_empty() {
        return cols;
    }
    let right_half = |col: usize| row.cells().get(col).is_some_and(|cell| cell.width() == 0);
    let start = cols.start - usize::from(right_half(cols.start));
    let end = cols.end + usize::from(right_half(cols.end));
    start..end
}

/// Removes the spaces at the end of `text`'s last line.
fn trim_end(text: &mut String) {
    let len = text.trim_end_matches(' ').len();
    text.truncate(len);
}
