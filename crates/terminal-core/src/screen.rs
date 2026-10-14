//! The rows of one screen, and the few ways they change.
//!
//! Knows nothing of escape sequences, the cursor or modes: `terminal.rs` decides what to do,
//! and this file does it. Every change to the rows goes through the methods here, so the
//! Renderer can learn what changed (#22) and the scrollback can take the rows that scroll off
//! (#26), each in one place. None of them leaves half a wide character: a change to one of its
//! cells blanks both, as xterm does.

use alloc::string::String;
use alloc::vec;
use alloc::vec::Vec;
use core::ops::Range;

/// How many zero-width characters a cell keeps, as xterm keeps by default (its
/// `combiningChars`). More are dropped.
const MARKS: usize = 2;

/// A cell's marks when it has none. A constant, so that a new cell copies it rather than
/// building it: `None` is not zero, and building it costs a call for each character printed.
const NO_MARKS: [Option<char>; MARKS] = [None; MARKS];

/// One cell of the grid: a character, and the zero-width characters joined to it, such as
/// combining marks. A wide character takes two cells, and the second, its right half, holds
/// nothing of its own. Until #19 a cell holds no colors or attributes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Cell {
    char: char,
    /// The zero-width characters joined to `char`, in the order they came.
    marks: [Option<char>; MARKS],
    /// 1, 2 for a wide character, or 0 for its right half.
    width: u8,
}

impl Cell {
    /// What an erased cell holds.
    pub(crate) const BLANK: Self = Self::new(' ', 1);

    /// The right half of a wide character.
    const RIGHT_HALF: Self = Self::new(' ', 0);

    /// A cell holding `char`, which takes `width` cells: 1, or 2 for a wide character.
    pub(crate) const fn new(char: char, width: usize) -> Self {
        Self {
            char,
            marks: NO_MARKS,
            width: width as u8,
        }
    }

    /// The cell's text: its character and the zero-width characters joined to it. A wide
    /// character's right half has none.
    pub fn chars(&self) -> impl Iterator<Item = char> {
        let char = (self.width > 0).then_some(self.char);
        char.into_iter().chain(self.marks.into_iter().flatten())
    }

    /// How many columns the cell's text takes: 1, 2 for a wide character, whose right half is
    /// the next cell, or 0 for that right half.
    pub fn width(&self) -> usize {
        usize::from(self.width)
    }
}

/// One row of a screen.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Row {
    cells: Vec<Cell>,
    wrapped: bool,
}

impl Row {
    fn blank(cols: usize) -> Self {
        Self {
            cells: vec![Cell::BLANK; cols],
            wrapped: false,
        }
    }

    fn clear(&mut self) {
        self.cells.fill(Cell::BLANK);
        self.wrapped = false;
    }

    pub fn cells(&self) -> &[Cell] {
        &self.cells
    }

    /// Whether the line goes on in the next row: it was too long for this one, and wrapped.
    /// Copying joins such rows (TERM-17), and reflow keeps them together (TERM-16).
    pub fn wrapped(&self) -> bool {
        self.wrapped
    }

    /// The row's text, cell by cell, its trailing blanks included.
    pub fn text(&self) -> String {
        self.cells.iter().flat_map(Cell::chars).collect()
    }

    /// Blanks the wide character that has a cell on each side of the boundary before `col`, if
    /// there is one, before a change on one side would leave it in halves.
    fn blank_across(&mut self, col: usize) {
        if self.cells.get(col).is_some_and(|cell| cell.width == 0) {
            self.cells[col - 1..=col].fill(Cell::BLANK);
        }
    }
}

/// The rows of one screen. The primary screen holds the shell and what it printed; the
/// alternate screen holds a full-screen program, and keeps no scrollback.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Screen {
    /// Never empty, and every row is as long as the screen is wide.
    rows: Vec<Row>,
}

impl Screen {
    pub(crate) fn new(cols: usize, rows: usize) -> Self {
        Self {
            rows: vec![Row::blank(cols); rows],
        }
    }

    pub fn rows(&self) -> &[Row] {
        &self.rows
    }

    /// Writes `char` in `col` of `row`, taking `width` cells: 1, or 2 for a wide character,
    /// whose right half is the next cell. Both must be in the row. A wide character written over
    /// in part is blanked.
    pub(crate) fn write(&mut self, row: usize, col: usize, char: char, width: usize) {
        let row = &mut self.rows[row];
        row.blank_across(col);
        row.blank_across(col + width);
        row.cells[col] = Cell::new(char, width);
        if width == 2 {
            row.cells[col + 1] = Cell::RIGHT_HALF;
        }
    }

    /// Joins the zero-width character `mark` to the character in `col` of `row`, or to the
    /// wide character whose right half is there. A cell that already holds [`MARKS`] drops it.
    pub(crate) fn join(&mut self, row: usize, col: usize, mark: char) {
        let cells = &mut self.rows[row].cells;
        let col = if cells[col].width == 0 { col - 1 } else { col };
        if let Some(free) = cells[col].marks.iter_mut().find(|mark| mark.is_none()) {
            *free = Some(mark);
        }
    }

    pub(crate) fn set_wrapped(&mut self, row: usize, wrapped: bool) {
        self.rows[row].wrapped = wrapped;
    }

    /// Blanks the cells `cols` of `row`, and the whole of a wide character they hold half of.
    /// Blanking to the end of the row ends any wrap there.
    pub(crate) fn erase(&mut self, row: usize, cols: Range<usize>) {
        let row = &mut self.rows[row];
        let end = cols.end.min(row.cells.len());
        let start = cols.start.min(end);
        // The last cell may be the right half of a wide character the erase takes whole.
        let reach = end + usize::from(row.cells.get(end).is_some_and(|cell| cell.width == 0));
        row.blank_across(start);
        row.blank_across(end);
        row.cells[start..end].fill(Cell::BLANK);
        if reach == row.cells.len() {
            row.wrapped = false;
        }
    }

    /// Blanks every cell of `rows`.
    pub(crate) fn erase_rows(&mut self, rows: Range<usize>) {
        for row in &mut self.rows[rows] {
            row.clear();
        }
    }

    /// Fills the whole screen with `cell`, as DECALN does.
    pub(crate) fn fill(&mut self, cell: Cell) {
        for row in &mut self.rows {
            row.cells.fill(cell);
            row.wrapped = false;
        }
    }

    /// Moves the cells from `col` on `count` places right, blanking the cells they leave.
    /// Cells pushed past the end of the row are lost, and so is a wide character pushed half
    /// past it, or one `col` is the right half of.
    pub(crate) fn insert_cells(&mut self, row: usize, col: usize, count: usize) {
        let row = &mut self.rows[row];
        let count = count.min(row.cells.len() - col);
        row.blank_across(col);
        row.blank_across(row.cells.len() - count);
        let cells = &mut row.cells[col..];
        cells.rotate_right(count);
        cells[..count].fill(Cell::BLANK);
    }

    /// Removes `count` cells from `col` on, moving the rest left and blanking the end of the
    /// row. A wide character with one half removed leaves a blank for the other. The line no
    /// longer reaches the end of the row, so it no longer wraps.
    pub(crate) fn delete_cells(&mut self, row: usize, col: usize, count: usize) {
        let row = &mut self.rows[row];
        let count = count.min(row.cells.len() - col);
        row.blank_across(col);
        row.blank_across(col + count);
        let cells = &mut row.cells[col..];
        cells.rotate_left(count);
        let kept = cells.len() - count;
        cells[kept..].fill(Cell::BLANK);
        row.wrapped = false;
    }

    /// Moves the rows of `region` up by `count`, with blank rows coming in at its bottom. The
    /// rows that go past its top are lost. Each row keeps whether it wraps.
    pub(crate) fn scroll_up(&mut self, region: Range<usize>, count: usize) {
        let rows = &mut self.rows[region];
        let count = count.min(rows.len());
        rows.rotate_left(count);
        let kept = rows.len() - count;
        for row in &mut rows[kept..] {
            row.clear();
        }
    }

    /// Moves the rows of `region` down by `count`, with blank rows coming in at its top. The
    /// rows that go past its bottom are lost.
    pub(crate) fn scroll_down(&mut self, region: Range<usize>, count: usize) {
        let rows = &mut self.rows[region];
        let count = count.min(rows.len());
        rows.rotate_right(count);
        for row in &mut rows[..count] {
            row.clear();
        }
    }

    /// Makes the screen `cols` wide and `rows` high, without reflowing (#26). Rows are cut or
    /// padded with blanks, and a wide character cut in two is blanked. With fewer rows, rows go
    /// from the top only as far as needed to keep row `keep` on the screen, and the rest from
    /// the bottom. Returns how many went from the top.
    pub(crate) fn resize(&mut self, cols: usize, rows: usize, keep: usize) -> usize {
        for row in &mut self.rows {
            row.blank_across(cols);
            row.cells.resize(cols, Cell::BLANK);
        }
        let from_top = (keep + 1).saturating_sub(rows).min(self.rows.len());
        self.rows.drain(..from_top);
        self.rows.resize(rows, Row::blank(cols));
        from_top
    }

    /// Panics if a row is not `cols` wide or holds half a wide character.
    #[cfg(any(test, fuzzing))]
    pub(crate) fn check(&self, cols: usize) {
        for row in &self.rows {
            assert_eq!(row.cells.len(), cols);
            for (col, cell) in row.cells.iter().enumerate() {
                let follows_wide = col > 0 && row.cells[col - 1].width == 2;
                assert_eq!(cell.width == 0, follows_wide, "column {col} of {row:?}");
                assert!(cell.width != 0 || *cell == Cell::RIGHT_HALF, "{row:?}");
            }
            assert!(
                row.cells.last().is_none_or(|cell| cell.width < 2),
                "{row:?}"
            );
        }
    }
}
