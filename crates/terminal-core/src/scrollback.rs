//! The rows that scrolled off the top of the primary screen (TERM-14), and the reflow of its
//! lines when the grid changes size (TERM-16, REL-03).
//!
//! The rows are numbered, the first that ever scrolled off being 0, so the Renderer can keep
//! its view on the same text while output arrives (DESIGN.md §3, *The view*). A row whose line
//! ends in it is kept without the blanks in the default style at its end, so a short line
//! takes little memory (PERF-10). DESIGN.md §3 has the rules: *Scrollback* and *Reflow*.

use alloc::collections::VecDeque;
use alloc::vec::Vec;
use core::mem;
use core::ops::Range;

use crate::screen::{Cell, Row, Screen, Wrap};

/// The most rows the Scrollback setting may ask for (Appendix B).
pub(crate) const MOST: usize = 100_000;

/// How many rows are kept until the setting says otherwise: Appendix B's default.
pub(crate) const DEFAULT: usize = 10_000;

/// The rows that scrolled off the top of the primary screen, oldest first.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Scrollback {
    rows: VecDeque<Row>,
    /// The most rows it keeps.
    limit: usize,
    /// The number of its first row.
    start: u64,
}

impl Scrollback {
    pub(crate) fn new(limit: usize) -> Self {
        Self {
            rows: VecDeque::new(),
            limit: limit.min(MOST),
            start: 0,
        }
    }

    /// How many rows it holds.
    pub fn len(&self) -> usize {
        self.rows.len()
    }

    pub fn is_empty(&self) -> bool {
        self.rows.is_empty()
    }

    /// Its row `index`, counted from the oldest.
    pub fn get(&self, index: usize) -> Option<&Row> {
        self.rows.get(index)
    }

    /// The number of its oldest row: rows are numbered from the first that ever scrolled off,
    /// and the screen's first row is numbered `start() + len()`. A row dropped past the limit,
    /// or emptied, keeps the numbers of the rest. A resize numbers the rows anew.
    pub fn start(&self) -> u64 {
        self.start
    }

    /// The most rows it keeps.
    pub fn limit(&self) -> usize {
        self.limit
    }

    /// Keeps no more than `limit` rows, and no more than [`MOST`], dropping the oldest.
    pub(crate) fn set_limit(&mut self, limit: usize) {
        self.limit = limit.min(MOST);
        self.trim();
    }

    /// Takes `row`, which scrolled off the top of the screen. Past the limit, the oldest row
    /// goes.
    pub(crate) fn push(&mut self, row: &Row) {
        if self.limit == 0 {
            self.start += 1;
            return;
        }
        if self.rows.len() == self.limit {
            self.rows.pop_front();
            self.start += 1;
        }
        self.rows.push_back(row.stored());
    }

    /// Empties it: ED 3, RIS and Clear. The screen's rows keep their numbers.
    pub(crate) fn clear(&mut self) {
        self.start += self.rows.len() as u64;
        self.rows.clear();
    }

    fn trim(&mut self) {
        let over = self.rows.len().saturating_sub(self.limit);
        self.rows.drain(..over);
        self.start += over as u64;
    }

    /// Panics if it holds more rows than its limit, or a row it should not: one wider than
    /// `cols`, one that wraps and is narrower, one that ends in a blank in the default style
    /// where its line ends, or one that breaks a row's rules.
    #[cfg(any(test, fuzzing))]
    pub(crate) fn check(&self, cols: usize) {
        assert!(self.rows.len() <= self.limit, "{} rows", self.rows.len());
        for row in &self.rows {
            if row.wrapped() {
                assert_eq!(row.cells().len(), cols, "{row:?}");
            } else {
                assert!(row.cells().len() <= cols, "{row:?}");
                assert_ne!(row.cells().last(), Some(&Cell::BLANK), "{row:?}");
            }
            row.check();
        }
    }
}

/// A cell of the primary screen that a reflow keeps track of: a cursor, saved or not.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Point {
    pub(crate) row: usize,
    pub(crate) col: usize,
    /// The cursor waits to wrap: its character is in the last column, and the next starts the
    /// next row.
    pub(crate) pending: bool,
}

/// A point as a cell of the line it is in. A cursor waiting to wrap is at the cell after its
/// character's.
#[derive(Clone, Copy, Debug)]
struct Mark {
    cell: usize,
    pending: bool,
}

/// Where a mark is in the rows made from its line: the row, the column, and whether it waits to
/// wrap.
type Spot = (usize, usize, bool);

/// Makes `screen`, the primary screen, `cols` wide and `rows` high, with `scrollback` above it,
/// as DESIGN.md §3 *Reflow* says. When the width changes, the lines are wrapped again.
/// `cursor` is the screen's cursor, which stays on the screen, and `saved` another point to
/// follow. Each comes back in the cell it was in, or on the row of the screen nearest to it.
pub(crate) fn reflow(
    scrollback: &mut Scrollback,
    screen: &mut Screen,
    cols: usize,
    rows: usize,
    cursor: &mut Option<Point>,
    saved: &mut Option<Point>,
) {
    let old = mem::take(screen.rows_mut());
    let old_rows = old.len();
    let old_cols = old.first().map_or(cols, |row| row.cells().len());
    let rewrap = cols != old_cols;

    // The screen's text ends below its last row that is not blank, or below the cursor. A
    // screen filled to its bottom keeps its bottom; another keeps its top.
    let content = old
        .iter()
        .rposition(|row| !row.is_blank())
        .map_or(0, |last| last + 1);
    let end = cursor.map_or(content, |cursor| content.max(cursor.row + 1));
    let full = end == old_rows;

    // Every row, oldest first: the scrollback's, then the screen's to the end of its text.
    let above = scrollback.rows.len();
    let mut all: Vec<Row> = mem::take(&mut scrollback.rows).into();
    all.extend(old.into_iter().take(end));

    // The cursor, the saved point and the screen's first row, to follow.
    let points = [
        *cursor,
        *saved,
        (end > 0).then_some(Point {
            row: 0,
            col: 0,
            pending: false,
        }),
    ];
    // Where each went: its row counted from the newest row made, its column, and whether it
    // waits to wrap.
    let mut found: [Option<Spot>; 3] = [None; 3];
    // The rows made, newest first.
    let mut made: Vec<Row> = Vec::new();
    // How many of the newest rows made are on the screen or below it, so the screen's first is
    // the newest row above them. Known once every line of the screen is made.
    let mut taken: Option<usize> = None;

    // Lines are made from the newest, so that those the scrollback has no room for are never
    // made, and the screen's come first.
    let mut index = all.len();
    loop {
        if taken.is_none() && index <= above {
            taken = Some(screen_taken(full, rows, found[2], found[0]));
        }
        if index == 0 || taken.is_some_and(|taken| made.len() >= taken + scrollback.limit) {
            break;
        }
        // The line's rows are `first..index`: its last, and those before it that wrap.
        let mut first = index - 1;
        if rewrap {
            while first > 0 && all[first - 1].wrapped() {
                first -= 1;
            }
        }
        let line: Vec<Row> = all.drain(first..).collect();

        let mut marks: [Option<Mark>; 3] = [None; 3];
        let mut offset = 0;
        for (at, row) in (first..).zip(&line) {
            let len = if rewrap {
                row.line().len()
            } else {
                row.cells().len()
            };
            for (mark, point) in marks.iter_mut().zip(points) {
                if let Some(point) = point.filter(|point| at >= above && point.row == at - above) {
                    // On the column a wide character skipped, which is not part of the line, a
                    // point stays on its row, in the cell before.
                    let col = if point.pending {
                        point.col
                    } else {
                        point.col.min(len.saturating_sub(1))
                    };
                    *mark = Some(Mark {
                        cell: offset + col + usize::from(point.pending),
                        pending: point.pending,
                    });
                }
            }
            offset += len;
        }

        let mut spots: [Option<Spot>; 3] = [None; 3];
        let line = if rewrap {
            wrap(line, cols, marks, &mut spots)
        } else {
            // Nothing is wrapped again: the row moves whole, and its points with it.
            for (spot, mark) in spots.iter_mut().zip(marks) {
                *spot = mark.map(|mark| (0, mark.cell - usize::from(mark.pending), mark.pending));
            }
            line
        };
        let count = line.len();
        for (found, spot) in found.iter_mut().zip(spots) {
            if let Some((row, col, pending)) = spot {
                *found = Some((made.len() + count - 1 - row, col, pending));
            }
        }
        made.extend(line.into_iter().rev());
        index = first;
    }

    // The screen: the rows taken, but those that no longer fit below it, padded at the bottom
    // with blank ones. The rows above it go to the scrollback, as many as it keeps.
    let taken = taken.unwrap_or(0).min(made.len());
    let below = taken.saturating_sub(rows);
    let mut screen_rows: Vec<Row> = Vec::with_capacity(rows);
    let mut kept: VecDeque<Row> = VecDeque::new();
    for (from_newest, mut row) in made.into_iter().enumerate().rev() {
        if from_newest >= taken {
            if from_newest < taken + scrollback.limit {
                kept.push_back(row.into_stored());
            }
        } else if from_newest >= below {
            row.pad(cols);
            screen_rows.push(row);
        }
    }
    screen_rows.resize_with(rows, || Row::blank(cols));
    *screen.rows_mut() = screen_rows;
    scrollback.rows = kept;

    // Each point comes back on the screen.
    let back = |(from_newest, col, pending): Spot| {
        let row = taken as isize - 1 - from_newest as isize;
        let on_screen = (0..rows as isize).contains(&row);
        let col = col.min(cols - 1);
        Point {
            row: row.clamp(0, rows as isize - 1) as usize,
            col,
            pending: pending && on_screen && col == cols - 1,
        }
    };
    if cursor.is_some() {
        *cursor = found[0].map(back);
    }
    if let Some(point) = *saved {
        *saved = Some(found[1].map(back).unwrap_or(Point {
            // Below the text: as far below it as it was.
            row: (taken + point.row.saturating_sub(end)).min(rows - 1),
            col: point.col.min(cols - 1),
            pending: false,
        }));
    }
}

/// How many of the newest rows made are on the screen, `rows` high, or below it. A screen that
/// was `full` keeps its bottom. One that was not keeps the row its `top` row starts in at its
/// top, unless its text no longer fits below it. The `cursor`'s row is always on the screen.
fn screen_taken(full: bool, rows: usize, top: Option<Spot>, cursor: Option<Spot>) -> usize {
    let taken = if full {
        rows
    } else {
        top.map_or(0, |(top, ..)| (top + 1).min(rows))
    };
    cursor.map_or(taken, |(cursor, ..)| taken.max(cursor + 1))
}

/// Cuts the line in `rows`, oldest first, into rows `cols` wide, and returns them, oldest first.
/// The blanks in the default style at the line's end are not part of it, but for those up to the
/// cursor's cell, the first of `marks`, so a cursor past the text stays as far after it. Each
/// mark's row among those returned, its column, and whether it waits to wrap are put in
/// `spots`. A mark at or past the end of the line, a saved point past the text among them,
/// waits to wrap where the line fills its last row, and is after the text where it does not.
fn wrap(
    mut rows: Vec<Row>,
    cols: usize,
    marks: [Option<Mark>; 3],
    spots: &mut [Option<Spot>; 3],
) -> Vec<Row> {
    // A line in one row that fits, with nothing to follow in it, moves as it is.
    if rows.len() == 1 && marks.iter().all(Option::is_none) && !rows[0].wrapped() {
        let len = rows[0].trimmed_len();
        if len <= cols {
            let mut cells = rows.pop().expect("a row").into_cells();
            cells.truncate(len);
            return alloc::vec![Row::new(cells, Wrap::No)];
        }
    }

    let mut cells: Vec<Cell> = Vec::new();
    for row in &rows {
        cells.extend_from_slice(row.line());
    }
    let trimmed = cells
        .iter()
        .rposition(|cell| *cell != Cell::BLANK)
        .map_or(0, |last| last + 1);
    // The cursor past the text keeps the cells up to its own. Just after the text it needs none
    // of its own: it waits to wrap where the line fills its last row. A saved cursor keeps none,
    // so one a program left behind never moves the text.
    let keep = marks[0].map_or(0, |mark| {
        mark.cell + usize::from(!mark.pending && mark.cell > trimmed)
    });
    cells.resize(trimmed.max(keep), Cell::BLANK);

    let mut out = Vec::new();
    let mut row: Vec<Cell> = Vec::with_capacity(cols);
    let mut index = 0;
    while index < cells.len() {
        // A right half on its own, which no line starts with, is a blank.
        let (cell, width) = match cells[index].width() {
            0 => (Cell::blank(*cells[index].style()), 1),
            width => (cells[index], width),
        };
        if row.len() + width > cols {
            if width > cols {
                // A wide character on a grid one column wide is dropped, as when it is printed.
                place(&marks, spots, index..index + width, out.len(), 0);
                index += width;
                continue;
            }
            // A wide character that would start in the last column starts the next row.
            let early = width == 2 && row.len() + 1 == cols;
            if early {
                row.push(Cell::BLANK);
            }
            let wrap = if early { Wrap::Early } else { Wrap::Yes };
            out.push(Row::new(
                mem::replace(&mut row, Vec::with_capacity(cols)),
                wrap,
            ));
        }
        place(&marks, spots, index..index + width, out.len(), row.len());
        row.push(cell);
        if width == 2 {
            row.push(cells[index + 1]);
        }
        index += width;
    }
    // Past the end: a cursor waiting to wrap after the line's last character.
    for (spot, mark) in spots.iter_mut().zip(marks) {
        if mark.is_some_and(|mark| mark.cell >= cells.len()) {
            *spot = Some(if row.len() == cols {
                (out.len(), cols - 1, true)
            } else {
                (out.len(), row.len(), false)
            });
        }
    }
    out.push(Row::new(row, Wrap::No));
    out
}

/// Puts each of `marks` in `cells`, which go into `row` from `col`, in `spots`.
fn place(
    marks: &[Option<Mark>; 3],
    spots: &mut [Option<Spot>; 3],
    cells: Range<usize>,
    row: usize,
    col: usize,
) {
    for (spot, mark) in spots.iter_mut().zip(marks) {
        if let Some(mark) = mark.filter(|mark| cells.contains(&mark.cell)) {
            *spot = Some((row, col + (mark.cell - cells.start), false));
        }
    }
}
