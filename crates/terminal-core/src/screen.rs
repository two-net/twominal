//! The rows of one screen, and the few ways they change.
//!
//! Knows nothing of escape sequences, the cursor or modes: `terminal.rs` decides what to do,
//! and this file does it. Every change to the rows goes through the methods here, so the
//! scrollback takes the rows that scroll off in one place, `scroll_up`. None of them leaves half a
//! wide character: a change to one of its cells blanks both, as xterm does. What a change
//! blanks with is handed to it, because xterm blanks in the colors in use, and those are
//! `terminal.rs`'s. The Renderer is not told what changed: it compares the rows with the ones
//! it drew last.

use alloc::string::String;
use alloc::vec;
use alloc::vec::Vec;
use core::ops::Range;

use crate::links::LinkId;
use crate::scrollback::Scrollback;
use crate::style::{Packed, Style};

/// How many zero-width characters a cell keeps, as xterm keeps by default (its
/// `combiningChars`). More are dropped.
const MARKS: usize = 2;

/// A cell's marks when it has none. A constant, so that a new cell copies it rather than
/// building it: `None` is not zero, and building it costs a call for each character printed.
const NO_MARKS: [Option<char>; MARKS] = [None; MARKS];

/// One cell of the grid: a character, the zero-width characters joined to it, such as
/// combining marks, the style it is drawn in and the link it is part of. A wide character
/// takes two cells, and the second, its right half, holds nothing of its own but the style and
/// the link.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Cell {
    char: char,
    /// The zero-width characters joined to `char`, in the order they came.
    marks: [Option<char>; MARKS],
    /// 1, 2 for a wide character, or 0 for its right half.
    width: u8,
    style: Packed,
    /// The hyperlink a program made of the cell (OSC 8, TERM-13).
    link: Option<LinkId>,
}

impl Cell {
    /// What a new screen holds, and the cells a resize adds.
    pub(crate) const BLANK: Self = Self::blank(Style::PLAIN);

    /// A cell holding `char` in `style`, which takes `width` cells: 1, or 2 for a wide
    /// character. It is part of no link.
    pub(crate) const fn new(char: char, width: usize, style: Style) -> Self {
        Self {
            char,
            marks: NO_MARKS,
            width: width as u8,
            style: Packed::new(style),
            link: None,
        }
    }

    /// An empty cell in `style`, part of no link.
    pub(crate) const fn blank(style: Style) -> Self {
        Self::new(' ', 1, style)
    }

    /// The cell, part of `link`.
    pub(crate) fn linked(self, link: Option<LinkId>) -> Self {
        Self { link, ..self }
    }

    /// The right half of the wide character `left`: its style and its link.
    const fn right_half(left: Self) -> Self {
        Self {
            char: ' ',
            marks: NO_MARKS,
            width: 0,
            style: left.style,
            link: left.link,
        }
    }

    /// Whether the cell is a space with nothing joined to it, in any style: what a wide
    /// character that went on to the next row may leave in the last column.
    fn is_space(&self) -> bool {
        self.char == ' ' && self.width == 1 && self.marks == NO_MARKS
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

    /// The colors and attributes the cell is drawn in. A wide character's right half has the
    /// character's.
    pub fn style(&self) -> Style {
        self.style.style()
    }

    /// The hyperlink the cell is part of, which [`crate::TerminalCore::link`] gives. A wide
    /// character's right half is part of the character's.
    pub fn link(&self) -> Option<LinkId> {
        self.link
    }
}

/// Whether the line in a row goes on in the next row.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) enum Wrap {
    #[default]
    No,
    Yes,
    /// It goes on, and the row's last cell is not part of it: a wide character that did not
    /// fit there went on to the next row, and left the cell a space. Alacritty marks such a
    /// cell as a spacer.
    Early,
}

/// One row of a screen, or of the scrollback.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Row {
    cells: Vec<Cell>,
    wrap: Wrap,
}

impl Row {
    pub(crate) fn new(cells: Vec<Cell>, wrap: Wrap) -> Self {
        Self { cells, wrap }
    }

    pub(crate) fn blank(cols: usize) -> Self {
        Self::new(vec![Cell::BLANK; cols], Wrap::No)
    }

    fn clear(&mut self, blank: Cell) {
        self.cells.fill(blank);
        self.wrap = Wrap::No;
    }

    /// The row's cells. A row of a screen is as wide as the screen. A row of the scrollback
    /// whose line ends in it stops at its last cell that is not a blank in the default style:
    /// the rest of it is such blanks.
    pub fn cells(&self) -> &[Cell] {
        &self.cells
    }

    /// Whether the line goes on in the next row: it was too long for this one, and wrapped.
    /// Copying joins such rows (TERM-17), and reflow keeps them together (TERM-16).
    pub fn wrapped(&self) -> bool {
        self.wrap != Wrap::No
    }

    /// The cells that are part of the row's line: all of them, but the last when a wide
    /// character that did not fit there went on to the next row and left it a space. Copying
    /// (TERM-17) and reflow (TERM-16) take these.
    pub fn line(&self) -> &[Cell] {
        match self.wrap {
            Wrap::Early => &self.cells[..self.cells.len() - 1],
            _ => &self.cells,
        }
    }

    pub(crate) fn into_cells(self) -> Vec<Cell> {
        self.cells
    }

    /// Whether the row holds nothing: every cell a blank in the default style, and no line
    /// that goes on.
    pub(crate) fn is_blank(&self) -> bool {
        self.wrap == Wrap::No && self.cells.iter().all(|cell| *cell == Cell::BLANK)
    }

    /// How many cells the row has before the blanks in the default style at its end.
    pub(crate) fn trimmed_len(&self) -> usize {
        self.cells
            .iter()
            .rposition(|cell| *cell != Cell::BLANK)
            .map_or(0, |last| last + 1)
    }

    /// The row as the scrollback keeps it: without the blanks in the default style at the end
    /// of a line that ends in it, and in no more memory than its cells take (PERF-10). A row
    /// that wraps is kept whole, since a blank where it wraps is part of its line.
    pub(crate) fn stored(&self) -> Self {
        let len = if self.wrapped() {
            self.cells.len()
        } else {
            self.trimmed_len()
        };
        let mut cells = Vec::with_capacity(len);
        cells.extend_from_slice(&self.cells[..len]);
        Self::new(cells, self.wrap)
    }

    /// The row as the scrollback keeps it, as [`Row::stored`] makes it, without a copy where
    /// none is needed.
    pub(crate) fn into_stored(mut self) -> Self {
        if !self.wrapped() {
            let len = self.trimmed_len();
            self.cells.truncate(len);
        }
        self.cells.shrink_to_fit();
        self
    }

    /// Makes the row `cols` wide, padding it with blanks in the default style. It must be no
    /// wider.
    pub(crate) fn pad(&mut self, cols: usize) {
        self.cells.resize(cols, Cell::BLANK);
    }

    /// The row's text, cell by cell, its trailing blanks included.
    pub fn text(&self) -> String {
        self.cells.iter().flat_map(Cell::chars).collect()
    }

    /// After something was written into, or joined to, cell `col`: when that is the last cell,
    /// it is part of the line again.
    fn joined_last(&mut self, col: usize) {
        if self.wrap == Wrap::Early && col + 1 == self.cells.len() {
            self.wrap = Wrap::Yes;
        }
    }

    /// Blanks with `blank` the wide character that has a cell on each side of the boundary
    /// before `col`, if there is one, before a change on one side would leave it in halves.
    fn blank_across(&mut self, col: usize, blank: Cell) {
        if self.cells.get(col).is_some_and(|cell| cell.width == 0) {
            self.cells[col - 1..=col].fill(blank);
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

    /// The rows themselves, for the reflow to take and give back. It leaves them as many as
    /// the screen is high, and each as wide as it is.
    pub(crate) fn rows_mut(&mut self) -> &mut Vec<Row> {
        &mut self.rows
    }

    /// Moves the rows of `keep` to the top, and blanks the rest in the default style: Clear
    /// (TERM-22).
    pub(crate) fn keep(&mut self, keep: Range<usize>) {
        let kept = keep.len();
        self.rows.rotate_left(keep.start);
        for row in &mut self.rows[kept..] {
            row.clear(Cell::BLANK);
        }
    }

    /// Writes `cell` in `col` of `row`, and the right half of a wide character in the next
    /// cell. Both must be in the row. A wide character written over in part is blanked with
    /// `blank`.
    pub(crate) fn write(&mut self, row: usize, col: usize, cell: Cell, blank: Cell) {
        let row = &mut self.rows[row];
        let width = cell.width();
        row.blank_across(col, blank);
        row.blank_across(col + width, blank);
        row.cells[col] = cell;
        if width == 2 {
            row.cells[col + 1] = Cell::right_half(cell);
        }
        row.joined_last(col + width - 1);
    }

    /// Joins the zero-width character `mark` to the character in `col` of `row`, or to the
    /// wide character whose right half is there. A cell that already holds [`MARKS`] drops it.
    pub(crate) fn join(&mut self, row: usize, col: usize, mark: char) {
        let row = &mut self.rows[row];
        let col = if row.cells[col].width == 0 {
            col - 1
        } else {
            col
        };
        if let Some(free) = row.cells[col].marks.iter_mut().find(|mark| mark.is_none()) {
            *free = Some(mark);
        }
        row.joined_last(col);
    }

    /// Marks the line in `row` as going on in the next row. `early` when a wide character that
    /// did not fit in its last column goes on: when that column is a space, it is then not part
    /// of the line.
    pub(crate) fn wrap(&mut self, row: usize, early: bool) {
        let row = &mut self.rows[row];
        row.wrap = if early && row.cells.last().is_some_and(Cell::is_space) {
            Wrap::Early
        } else {
            Wrap::Yes
        };
    }

    /// Blanks with `blank` the cells `cols` of `row`, and the whole of a wide character they
    /// hold half of. Blanking to the end of the row ends any wrap there.
    pub(crate) fn erase(&mut self, row: usize, cols: Range<usize>, blank: Cell) {
        let row = &mut self.rows[row];
        let end = cols.end.min(row.cells.len());
        let start = cols.start.min(end);
        // The last cell may be the right half of a wide character the erase takes whole.
        let reach = end + usize::from(row.cells.get(end).is_some_and(|cell| cell.width == 0));
        row.blank_across(start, blank);
        row.blank_across(end, blank);
        row.cells[start..end].fill(blank);
        if reach == row.cells.len() {
            row.wrap = Wrap::No;
        }
    }

    /// Blanks every cell of `rows` with `blank`.
    pub(crate) fn erase_rows(&mut self, rows: Range<usize>, blank: Cell) {
        for row in &mut self.rows[rows] {
            row.clear(blank);
        }
    }

    /// Fills the whole screen with `cell`, as DECALN does.
    pub(crate) fn fill(&mut self, cell: Cell) {
        for row in &mut self.rows {
            row.cells.fill(cell);
            row.wrap = Wrap::No;
        }
    }

    /// Moves the cells from `col` on `count` places right, blanking with `blank` the cells
    /// they leave. Cells pushed past the end of the row are lost, and so is a wide character
    /// pushed half past it, or one `col` is the right half of. What is pushed into the last
    /// cell is part of the line.
    pub(crate) fn insert_cells(&mut self, row: usize, col: usize, count: usize, blank: Cell) {
        let row = &mut self.rows[row];
        let count = count.min(row.cells.len() - col);
        row.blank_across(col, blank);
        row.blank_across(row.cells.len() - count, blank);
        let cells = &mut row.cells[col..];
        cells.rotate_right(count);
        cells[..count].fill(blank);
        row.joined_last(row.cells.len() - 1);
    }

    /// Removes `count` cells from `col` on, moving the rest left and blanking the end of the
    /// row with `blank`. A wide character with one half removed leaves a blank for the other.
    /// The line no longer reaches the end of the row, so it no longer wraps.
    pub(crate) fn delete_cells(&mut self, row: usize, col: usize, count: usize, blank: Cell) {
        let row = &mut self.rows[row];
        let count = count.min(row.cells.len() - col);
        row.blank_across(col, blank);
        row.blank_across(col + count, blank);
        let cells = &mut row.cells[col..];
        cells.rotate_left(count);
        let kept = cells.len() - count;
        cells[kept..].fill(blank);
        row.wrap = Wrap::No;
    }

    /// Moves the rows of `region` up by `count`, with rows of `blank` coming in at its bottom.
    /// The rows that go past its top go to `scrollback`, or with none are lost. Each row keeps
    /// whether it wraps.
    pub(crate) fn scroll_up(
        &mut self,
        region: Range<usize>,
        count: usize,
        blank: Cell,
        scrollback: Option<&mut Scrollback>,
    ) {
        let rows = &mut self.rows[region];
        let count = count.min(rows.len());
        if let Some(scrollback) = scrollback {
            for row in &rows[..count] {
                scrollback.push(row);
            }
        }
        // The rows that left are cleared and come back in, so no row is allocated anew.
        rows.rotate_left(count);
        let kept = rows.len() - count;
        for row in &mut rows[kept..] {
            row.clear(blank);
        }
    }

    /// Moves the rows of `region` down by `count`, with rows of `blank` coming in at its top.
    /// The rows that go past its bottom are lost.
    pub(crate) fn scroll_down(&mut self, region: Range<usize>, count: usize, blank: Cell) {
        let rows = &mut self.rows[region];
        let count = count.min(rows.len());
        rows.rotate_right(count);
        for row in &mut rows[..count] {
            row.clear(blank);
        }
    }

    /// Makes the screen `cols` wide and `rows` high without reflowing, as the alternate screen
    /// is. Rows are cut or padded with blanks, in the default style since no program asked for
    /// them, and a wide character cut in two is blanked. With fewer rows, rows go from the top
    /// only as far as needed to keep row `keep` on the screen, and the rest from the bottom.
    /// Returns how many went from the top.
    pub(crate) fn resize(&mut self, cols: usize, rows: usize, keep: usize) -> usize {
        for row in &mut self.rows {
            if row.cells.len() != cols && row.wrap == Wrap::Early {
                // Its last cell is another.
                row.wrap = Wrap::Yes;
            }
            row.blank_across(cols, Cell::BLANK);
            row.cells.resize(cols, Cell::BLANK);
        }
        let from_top = (keep + 1).saturating_sub(rows).min(self.rows.len());
        self.rows.drain(..from_top);
        self.rows.resize(rows, Row::blank(cols));
        from_top
    }

    /// Panics if a row is not `cols` wide or breaks a rule of [`Row::check`].
    #[cfg(any(test, fuzzing))]
    pub(crate) fn check(&self, cols: usize) {
        for row in &self.rows {
            assert_eq!(row.cells.len(), cols);
            row.check();
        }
    }
}

impl Row {
    /// How many cells the row has room for, for the test of its memory (PERF-10).
    #[cfg(test)]
    pub(crate) fn capacity(&self) -> usize {
        self.cells.capacity()
    }

    /// Panics if the row holds half a wide character, or a right half whose style or link is not
    /// its character's, or marks a last cell that is not a space as left by a wide character.
    #[cfg(any(test, fuzzing))]
    pub(crate) fn check(&self) {
        for (col, cell) in self.cells.iter().enumerate() {
            let follows_wide = col > 0 && self.cells[col - 1].width == 2;
            assert_eq!(cell.width == 0, follows_wide, "column {col} of {self:?}");
            assert!(
                cell.width != 0 || *cell == Cell::right_half(self.cells[col - 1]),
                "{self:?}"
            );
        }
        assert!(
            self.cells.last().is_none_or(|cell| cell.width < 2),
            "{self:?}"
        );
        assert!(
            self.wrap != Wrap::Early || self.cells.last().is_some_and(Cell::is_space),
            "{self:?}"
        );
    }
}
