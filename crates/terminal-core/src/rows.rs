//! The rows there are, by number: the scrollback's and the screen's, as the selection and find
//! take them (TERM-17, TERM-18).
//!
//! Rows are numbered as [`Scrollback::start`] numbers them, so what is kept by their numbers
//! stays on its text while output scrolls it up the screen and into the scrollback.

use core::ops::RangeInclusive;

use crate::screen::{Cell, Row, Screen};
use crate::scrollback::Scrollback;

/// How many rows of a line find searches as one text. A longer line is searched in pieces cut
/// at the row numbers that are multiples of this, so a line that keeps growing is not searched
/// again whole at every frame (REL-02).
#[cfg(not(any(test, fuzzing)))]
const PIECE: u64 = 1_000;
/// Short pieces in the tests and under the fuzzer, so that they reach lines cut in pieces.
#[cfg(any(test, fuzzing))]
pub(crate) const PIECE: u64 = 4;

/// The rows of the screen shown, and of the scrollback above the primary screen, by number.
#[derive(Clone, Copy)]
pub(crate) struct Rows<'a> {
    pub(crate) screen: &'a Screen,
    pub(crate) scrollback: &'a Scrollback,
    /// The alternate screen is shown, which has no scrollback above it.
    pub(crate) alternate: bool,
    pub(crate) cols: usize,
}

impl<'a> Rows<'a> {
    /// The number of the screen's first row.
    pub(crate) fn live(&self) -> u64 {
        self.scrollback.start() + self.scrollback.len() as u64
    }

    /// The number of the oldest row there is.
    pub(crate) fn first(&self) -> u64 {
        if self.alternate {
            self.live()
        } else {
            self.scrollback.start()
        }
    }

    /// The number of the screen's last row.
    pub(crate) fn last(&self) -> u64 {
        self.live() + self.screen.rows().len() as u64 - 1
    }

    /// Row `number`, if there is one.
    pub(crate) fn get(&self, number: u64) -> Option<&'a Row> {
        if number < self.first() {
            return None;
        }
        match number.checked_sub(self.live()) {
            Some(on_screen) => self.screen.rows().get(usize::try_from(on_screen).ok()?),
            None => self
                .scrollback
                .get((number - self.scrollback.start()) as usize),
        }
    }

    /// The cell at `col` of row `number`'s line, if the line reaches it. A scrollback row is
    /// kept without the blanks at its end, and a row that wraps early without its last cell.
    pub(crate) fn cell(&self, number: u64, col: usize) -> Option<&'a Cell> {
        self.get(number)?.line().get(col)
    }

    /// Whether row `number` goes on in the next row there is.
    fn wraps(&self, number: u64) -> bool {
        number < self.last() && self.get(number).is_some_and(Row::wrapped)
    }

    /// `rows`, widened over the rows of their lines, but no further than [`PIECE`] rows each
    /// way: as far as a text find searched before they changed could reach. Whether a line is
    /// searched whole or in pieces depends on how long it is, so a change to one row of a line can
    /// change the texts of its other rows.
    pub(crate) fn around(&self, rows: RangeInclusive<u64>) -> RangeInclusive<u64> {
        let (mut first, mut last) = (*rows.start(), *rows.end());
        let from = first;
        while first > self.first() && from - first < PIECE && self.wraps(first - 1) {
            first -= 1;
        }
        let from = last;
        while last - from < PIECE && self.wraps(last) {
            last += 1;
        }
        first..=last
    }

    /// The rows find searches as one text with row `number`, which must be one there is: the
    /// line it is part of, or of a line longer than [`PIECE`] rows, the piece between two row
    /// numbers that are multiples of it. No more than twice [`PIECE`] rows are looked at.
    pub(crate) fn text(&self, number: u64) -> RangeInclusive<u64> {
        let mut first = number;
        while first > self.first() && number - first <= PIECE && self.wraps(first - 1) {
            first -= 1;
        }
        let mut last = number;
        while last - first < PIECE && self.wraps(last) {
            last += 1;
        }
        if last - first < PIECE {
            return first..=last;
        }
        // A long line: the piece of it around `number`.
        let start = number - number % PIECE;
        let end = start + PIECE - 1;
        let mut first = number;
        while first > start && first > self.first() && self.wraps(first - 1) {
            first -= 1;
        }
        let mut last = number;
        while last < end && self.wraps(last) {
            last += 1;
        }
        first..=last
    }
}
