//! The rows of one screen, and the few ways they change.
//!
//! Knows nothing of escape sequences, the cursor or modes: `terminal.rs` decides what to do,
//! and this file does it. Every change to the rows goes through the methods here, so the
//! Renderer can learn what changed (#22) and the scrollback can take the rows that scroll off
//! (#26), each in one place.

use alloc::string::String;
use alloc::vec;
use alloc::vec::Vec;
use core::ops::Range;

/// One cell of the grid. Until #18 every character takes one cell, and until #19 a cell holds
/// its character and nothing else.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Cell {
    char: char,
}

impl Cell {
    /// What an erased cell holds.
    pub(crate) const BLANK: Self = Self { char: ' ' };

    pub(crate) fn new(char: char) -> Self {
        Self { char }
    }

    pub fn char(&self) -> char {
        self.char
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

    /// The row's characters, its trailing blanks included.
    pub fn text(&self) -> String {
        self.cells.iter().map(Cell::char).collect()
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

    pub(crate) fn write(&mut self, row: usize, col: usize, cell: Cell) {
        self.rows[row].cells[col] = cell;
    }

    pub(crate) fn set_wrapped(&mut self, row: usize, wrapped: bool) {
        self.rows[row].wrapped = wrapped;
    }

    /// Blanks the cells `cols` of `row`. Blanking to the end of the row ends any wrap there.
    pub(crate) fn erase(&mut self, row: usize, cols: Range<usize>) {
        let row = &mut self.rows[row];
        let end = cols.end.min(row.cells.len());
        row.cells[cols.start.min(end)..end].fill(Cell::BLANK);
        if end == row.cells.len() {
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
    /// Cells pushed past the end of the row are lost.
    pub(crate) fn insert_cells(&mut self, row: usize, col: usize, count: usize) {
        let cells = &mut self.rows[row].cells[col..];
        let count = count.min(cells.len());
        cells.rotate_right(count);
        cells[..count].fill(Cell::BLANK);
    }

    /// Removes `count` cells from `col` on, moving the rest left and blanking the end of the
    /// row. The line no longer reaches the end of the row, so it no longer wraps.
    pub(crate) fn delete_cells(&mut self, row: usize, col: usize, count: usize) {
        let row = &mut self.rows[row];
        let cells = &mut row.cells[col..];
        let count = count.min(cells.len());
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
    /// padded with blanks. With fewer rows, rows go from the top only as far as needed to keep
    /// row `keep` on the screen, and the rest from the bottom. Returns how many went from the
    /// top.
    pub(crate) fn resize(&mut self, cols: usize, rows: usize, keep: usize) -> usize {
        for row in &mut self.rows {
            row.cells.resize(cols, Cell::BLANK);
        }
        let from_top = (keep + 1).saturating_sub(rows).min(self.rows.len());
        self.rows.drain(..from_top);
        self.rows.resize(rows, Row::blank(cols));
        from_top
    }
}
