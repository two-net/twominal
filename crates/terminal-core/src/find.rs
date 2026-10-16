//! Find (TERM-18, TERM-19): what the user looks for, as plain text or as a regular expression,
//! and every match of it on the screen shown and in the scrollback above it.
//!
//! Each line is searched as one text: its rows joined while they wrap and the spaces at its end
//! left out, as copying takes it (TERM-17), so a match may go on over the rows a line wraps
//! into. A line longer than 1,000 rows is searched in pieces cut at row numbers that are
//! multiples of it. Matches are found leftmost first and do not overlap, as `regex` finds them,
//! and are kept by the numbers of their rows, as the selection is, so they stay on their text
//! while output scrolls it.
//!
//! The matches are always those a search of the rows as they are would give. Output does not
//! search: it marks the rows it changed, and [`Find::refind`], which the Tab calls once a
//! frame, searches again the texts those rows are in. DESIGN.md §3 *Find* has the rules.

use alloc::collections::VecDeque;
use alloc::string::String;
use alloc::vec::Vec;
use core::fmt;
use core::ops::{Range, RangeInclusive};

use regex_automata::meta::{Cache, Regex};
use regex_automata::util::iter::Searcher;
use regex_automata::util::syntax;
use regex_automata::Input;

use crate::rows::Rows;
use crate::screen::{Cell, Row};

/// The most characters a query keeps.
pub const QUERY: usize = 1024;

/// The most matches kept: the newest. Each takes 32 bytes.
#[cfg(not(any(test, fuzzing)))]
const MOST: usize = 100_000;
/// Few, so that the fuzz targets and the tests reach the limit.
#[cfg(any(test, fuzzing))]
pub(crate) const MOST: usize = 8;

/// The most memory a pattern's NFA may take. A plain query of [`QUERY`] characters fits, with
/// its letters in both cases. A larger pattern, such as a long repetition of `.`, would make
/// the lazy DFA give way to the PikeVM, which is slower by the size of the pattern, so it is
/// not searched (REL-02).
const NFA_MOST: usize = 256 * 1024;

/// The most memory the lazy DFA keeps between searches.
const CACHE_MOST: usize = 1024 * 1024;

/// What the user looks for (TERM-18, TERM-19).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Search {
    /// The text, or the pattern, looked for. It keeps [`QUERY`] characters.
    pub query: String,
    /// Letter case counts: `a` does not find `A`.
    pub case: bool,
    /// The query is a regular expression, as the `regex` crate reads one.
    pub regex: bool,
}

/// A cell, by the number of its row and its column, or the boundary after the last cell of a
/// match, whose column may then be the grid's width.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
struct Spot {
    row: u64,
    col: usize,
}

/// A match: from its first cell to the boundary after its last.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Match {
    start: Spot,
    end: Spot,
}

/// What the current match follows (DESIGN.md §3 *Find*).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Anchor {
    /// The newest match, whichever it is.
    Newest,
    /// The last match that starts at or before this cell, or the first match when none does.
    At(Spot),
}

/// The matches on one screen and the scrollback above it, and what changed since they were
/// found.
#[derive(Clone, Debug)]
struct Kept {
    /// In reading order, the newest [`MOST`] of them.
    matches: VecDeque<Match>,
    /// There are older matches than those kept.
    more: bool,
    anchor: Anchor,
    /// The oldest row there was when they were found.
    first: u64,
    /// The rows whose text changed since, by number.
    dirty: Option<Range<u64>>,
    /// Every row is to be searched again: the rows were numbered anew, or the search is new.
    all: bool,
}

impl Kept {
    fn new(anchor: Anchor) -> Self {
        Self {
            matches: VecDeque::new(),
            more: false,
            anchor,
            first: 0,
            dirty: None,
            all: true,
        }
    }

    /// Which of the matches is the current one: what the anchor says. None without matches.
    fn current(&self) -> Option<usize> {
        let last = self.matches.len().checked_sub(1)?;
        Some(match self.anchor {
            Anchor::Newest => last,
            Anchor::At(spot) => self
                .matches
                .partition_point(|found| found.start <= spot)
                .saturating_sub(1),
        })
    }
}

/// The search and what it found (TERM-18): on the primary screen with its scrollback, and on
/// the alternate screen, each kept apart so that a full-screen program coming and going does not
/// make the scrollback be searched again.
pub(crate) struct Find {
    search: Search,
    /// None when the query is no pattern there is, or one too large to search.
    regex: Option<Regex>,
    cache: Option<Cache>,
    primary: Kept,
    alternate: Kept,
}

impl fmt::Debug for Find {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Find")
            .field("search", &self.search)
            .field("valid", &self.regex.is_some())
            .field("primary", &self.primary)
            .field("alternate", &self.alternate)
            .finish()
    }
}

/// Two finds are the same when they look for the same and have found the same. The regex and
/// what is yet to be searched again are left out: the regex is made from the search, and what
/// changed is searched again before the matches are read.
impl PartialEq for Find {
    fn eq(&self, other: &Self) -> bool {
        let same =
            |a: &Kept, b: &Kept| (&a.matches, a.more, a.anchor) == (&b.matches, b.more, b.anchor);
        self.search == other.search
            && same(&self.primary, &other.primary)
            && same(&self.alternate, &other.alternate)
    }
}

impl Find {
    /// A search for `search` in `rows`. The current match is the last that starts at or before
    /// `before`, where the search before it was, if there was one, or else at or before the end of
    /// row `from`; or else the first match.
    pub(crate) fn new(
        search: Search,
        rows: &Rows,
        from: u64,
        before: Option<(u64, usize)>,
    ) -> Self {
        let search = Search {
            query: search.query.chars().take(QUERY).collect(),
            ..search
        };
        let regex = compile(&search);
        let cache = regex.as_ref().map(Regex::create_cache);
        let mut find = Self {
            search,
            regex,
            cache,
            primary: Kept::new(Anchor::Newest),
            alternate: Kept::new(Anchor::Newest),
        };
        find.refind(rows);
        let kept = find.kept(rows.alternate);
        let (row, col) = before.unwrap_or((from, usize::MAX));
        kept.anchor = Anchor::At(Spot { row, col });
        // On the match it chose, so that it stays there whatever output finds before the place.
        if let Some(current) = kept.current() {
            kept.anchor = Anchor::At(kept.matches[current].start);
        }
        find
    }

    pub(crate) fn search(&self) -> &Search {
        &self.search
    }

    /// Where the search is, on the alternate screen or the primary: where the current match
    /// starts, or with none, where the next would be current. None while it follows the newest,
    /// and while every row is yet to be searched again.
    pub(crate) fn place(&self, alternate: bool) -> Option<(u64, usize)> {
        let kept = self.on(alternate);
        // The matches are of rows numbered before a resize. A query that is no pattern has
        // none, and keeps its place.
        if kept.all && self.regex.is_some() {
            return None;
        }
        let spot = match (kept.current(), kept.anchor) {
            (Some(current), _) => kept.matches[current].start,
            (None, Anchor::At(spot)) => spot,
            (None, Anchor::Newest) => return None,
        };
        Some((spot.row, spot.col))
    }

    /// The matches on the alternate screen, or on the primary.
    fn on(&self, alternate: bool) -> &Kept {
        if alternate {
            &self.alternate
        } else {
            &self.primary
        }
    }

    fn kept(&mut self, alternate: bool) -> &mut Kept {
        if alternate {
            &mut self.alternate
        } else {
            &mut self.primary
        }
    }

    /// Output changed the text of `rows`, by number, on the alternate screen or on the primary.
    pub(crate) fn changed(&mut self, alternate: bool, rows: Range<u64>) {
        let kept = self.kept(alternate);
        kept.dirty = Some(match kept.dirty.take() {
            Some(dirty) => dirty.start.min(rows.start)..dirty.end.max(rows.end),
            None => rows,
        });
    }

    /// Every row is to be searched again, on both screens: Clear and RIS.
    pub(crate) fn everything(&mut self) {
        self.primary.all = true;
        self.alternate.all = true;
    }

    /// The rows were numbered anew: a resize. The current match follows the newest output until
    /// the user moves it.
    pub(crate) fn resized(&mut self) {
        self.everything();
        self.primary.anchor = Anchor::Newest;
        self.alternate.anchor = Anchor::Newest;
    }

    /// The alternate screen is shown, which is a new screen each time, or the primary again,
    /// whose matches were kept, and which nothing changed meanwhile.
    pub(crate) fn switched(&mut self, alternate: bool) {
        if alternate {
            self.alternate = Kept::new(Anchor::Newest);
        }
    }

    /// Searches again what changed in `rows` since the last search, so that the matches are
    /// those a search of the rows as they are would give: the texts of the rows output changed,
    /// and of the row after them, which a change may have split from them; and when the oldest
    /// rows were dropped, the text of the oldest row left. Each with the rows of their lines
    /// around them up to a piece away, whose texts a line growing or shrinking past a piece
    /// changes. Past the most matches kept, the oldest go. When there were more, and fewer than
    /// that are left, or the rows of older matches changed or were dropped, the rows before the
    /// oldest kept are searched back from it.
    pub(crate) fn refind(&mut self, rows: &Rows) {
        let (Some(regex), Some(cache)) = (&self.regex, &mut self.cache) else {
            return;
        };
        let kept = if rows.alternate {
            &mut self.alternate
        } else {
            &mut self.primary
        };
        let first = rows.first();
        let last = rows.last();
        if kept.all {
            (kept.matches, kept.more) = search_all(regex, cache, rows);
            kept.all = false;
            kept.dirty = None;
            kept.first = first;
            return;
        }
        let mut again: Vec<RangeInclusive<u64>> = Vec::new();
        let dropped = first != kept.first;
        if dropped {
            while kept
                .matches
                .front()
                .is_some_and(|found| found.start.row < first)
            {
                kept.matches.pop_front();
            }
            again.push(first..=first);
        }
        if let Some(dirty) = kept.dirty.take() {
            let start = dirty.start.max(first);
            // The row after them too, which a wrap that ended may have split from them.
            let end = dirty.end.min(last);
            if start <= end {
                again.push(start..=end);
            }
        }
        kept.first = first;
        if again.is_empty() {
            return;
        }
        // While older matches are left out, the matches kept start at the oldest of them.
        let oldest = kept
            .matches
            .front()
            .map(|found| found.start)
            .filter(|_| kept.more);
        let (mut searched, mut haystack) = (Vec::new(), String::new());
        // Whether a text searched again could hold the older matches left out, which may then
        // be gone.
        let mut older = false;
        for rows_again in again {
            let around = rows.around(rows_again);
            let from = *rows.text(*around.start()).start();
            let to = *rows.text(*around.end()).end();
            older |= oldest.is_some_and(|oldest| from <= oldest.row);
            let start = kept.matches.partition_point(|found| found.start.row < from);
            let end = kept.matches.partition_point(|found| found.start.row <= to);
            let after: Vec<Match> = kept.matches.drain(start..).skip(end - start).collect();
            let mut row = from;
            while row <= to {
                let text = rows.text(row);
                row = text.end() + 1;
                searched.clear();
                search_text(regex, cache, rows, text, &mut searched, &mut haystack);
                kept.matches.extend(
                    searched
                        .iter()
                        .filter(|found| oldest.is_none_or(|oldest| found.start >= oldest)),
                );
            }
            kept.matches.extend(after);
        }
        if kept.matches.len() > MOST {
            kept.matches.drain(..kept.matches.len() - MOST);
            kept.more = true;
        } else if kept.more && (kept.matches.len() < MOST || dropped || older) {
            fill(regex, cache, rows, kept);
        }
    }

    /// Makes the match above the current one current, or the one below it when not `up`,
    /// round the ends. `rows` are searched again first where output changed them.
    pub(crate) fn step(&mut self, rows: &Rows, up: bool) {
        self.refind(rows);
        let kept = self.kept(rows.alternate);
        let Some(current) = kept.current() else {
            return;
        };
        let count = kept.matches.len();
        let next = if up {
            (current + count - 1) % count
        } else {
            (current + 1) % count
        };
        kept.anchor = Anchor::At(kept.matches[next].start);
    }

    /// What was found in `rows`, for the Renderer to draw and the find bar to count. None while
    /// every row is yet to be searched again, as after a resize, whose rows the matches do not
    /// number.
    pub(crate) fn found<'a>(&'a self, rows: &Rows) -> Option<Found<'a>> {
        let kept = self.on(rows.alternate);
        if kept.all && self.regex.is_some() {
            return None;
        }
        Some(Found {
            matches: &kept.matches,
            current: kept.current(),
            more: kept.more,
            invalid: self.regex.is_none(),
            cols: rows.cols,
        })
    }

    /// Panics if the matches break a rule: in reading order, apart, no more than the most kept,
    /// and with their columns in the grid.
    #[cfg(any(test, fuzzing))]
    pub(crate) fn check(&self, rows: &Rows) {
        let kept = self.on(rows.alternate);
        assert!(kept.matches.len() <= MOST, "{self:?}");
        assert!(!kept.more || kept.matches.len() == MOST, "{self:?}");
        if kept.all {
            return;
        }
        let mut before: Option<Spot> = None;
        for found in &kept.matches {
            assert!(found.start < found.end, "{self:?}");
            assert!(before.is_none_or(|end| end <= found.start), "{self:?}");
            assert!(found.end.col <= rows.cols, "{self:?}");
            before = Some(found.end);
        }
    }

    /// Whether the matches are those a search of `rows` gives now: what [`Find::refind`] must
    /// make them.
    #[cfg(any(test, fuzzing))]
    pub(crate) fn is_fresh(&self, rows: &Rows) -> bool {
        let Some(regex) = &self.regex else {
            return true;
        };
        let kept = self.on(rows.alternate);
        let fresh = search_all(regex, &mut regex.create_cache(), rows);
        !kept.all && (&kept.matches, kept.more) == (&fresh.0, fresh.1)
    }
}

/// The regex that finds `search`, or None when the query is no pattern, or one larger than
/// [`NFA_MOST`]. Plain text is escaped, so it finds itself.
fn compile(search: &Search) -> Option<Regex> {
    let pattern = if search.regex {
        search.query.clone()
    } else {
        escape(&search.query)
    };
    Regex::builder()
        .syntax(syntax::Config::new().case_insensitive(!search.case))
        .configure(
            Regex::config()
                .nfa_size_limit(Some(NFA_MOST))
                .hybrid_cache_capacity(CACHE_MOST),
        )
        .build(&pattern)
        .ok()
}

/// `text` as a pattern that finds it: every character the `regex` crate gives a meaning to,
/// `regex-syntax`'s `is_meta_character`, after a backslash. No other character may have one,
/// since a backslash before a letter is an error, and before `<` a word boundary.
fn escape(text: &str) -> String {
    let mut pattern = String::with_capacity(text.len());
    for char in text.chars() {
        if r"\.+*?()|[]{}^$#&-~".contains(char) {
            pattern.push('\\');
        }
        pattern.push(char);
    }
    pattern
}

/// Every match in `rows`, the newest [`MOST`] of them, searched from the newest text, and
/// whether there are more.
fn search_all(regex: &Regex, cache: &mut Cache, rows: &Rows) -> (VecDeque<Match>, bool) {
    let mut matches = VecDeque::new();
    let (mut found, mut haystack) = (Vec::new(), String::new());
    let mut row = rows.last();
    loop {
        let text = rows.text(row);
        found.clear();
        search_text(regex, cache, rows, text.clone(), &mut found, &mut haystack);
        for found in found.iter().rev() {
            matches.push_front(*found);
        }
        if matches.len() > MOST {
            matches.drain(..matches.len() - MOST);
            return (matches, true);
        }
        if *text.start() <= rows.first() {
            return (matches, false);
        }
        row = text.start() - 1;
    }
}

/// Brings back the older matches that were left out, as many as fit behind those kept, which
/// are every match from the oldest kept on: searching the rows before it, back from it, until one
/// more than fit is found, so that there are more, or the oldest row is reached, so that there
/// are not. With none kept, everything is searched again.
fn fill(regex: &Regex, cache: &mut Cache, rows: &Rows, kept: &mut Kept) {
    let Some(oldest) = kept.matches.front().map(|found| found.start) else {
        (kept.matches, kept.more) = search_all(regex, cache, rows);
        return;
    };
    let (mut found, mut haystack) = (Vec::new(), String::new());
    let mut row = oldest.row;
    loop {
        let text = rows.text(row);
        found.clear();
        search_text(regex, cache, rows, text.clone(), &mut found, &mut haystack);
        for found in found.iter().rev().filter(|found| found.start < oldest) {
            kept.matches.push_front(*found);
        }
        if kept.matches.len() > MOST {
            kept.matches.drain(..kept.matches.len() - MOST);
            kept.more = true;
            return;
        }
        if *text.start() <= rows.first() {
            kept.more = false;
            return;
        }
        row = text.start() - 1;
    }
}

/// The cells of row `number`, `row`, that find reads: those of its line, without the spaces at
/// its end when the line ends in it, as copying leaves them out. A row of the scrollback is kept
/// without the blanks at its end, so a row reads the same on the screen and after it.
fn read<'a>(rows: &Rows, number: u64, row: &'a Row) -> &'a [Cell] {
    let mut cells = row.line();
    if !row.wrapped() || number == rows.last() {
        while cells
            .last()
            .is_some_and(|cell| cell.chars().eq(core::iter::once(' ')))
        {
            cells = &cells[..cells.len() - 1];
        }
    }
    cells
}

/// Where a cell's characters are in the text searched.
struct Placed {
    byte: usize,
    row: u64,
    col: usize,
    /// How many columns its character takes: 1, or 2.
    width: usize,
}

/// Adds to `found` the matches in `text`, rows that find searches as one, in reading order. The
/// rows of the line just before and after it are read too, so that `^`, `$` and `\b` know what
/// is next to a piece of a longer line. Where each cell is in the text is worked out only when
/// something matched, which most texts do not. `haystack` is where the text is put, kept from one
/// text to the next so that its memory is.
fn search_text(
    regex: &Regex,
    cache: &mut Cache,
    rows: &Rows,
    text: RangeInclusive<u64>,
    found: &mut Vec<Match>,
    haystack: &mut String,
) {
    let (first, last) = (*text.start(), *text.end());
    haystack.clear();
    let chars = |number: u64| {
        rows.get(number)
            .into_iter()
            .flat_map(move |row| read(rows, number, row))
            .flat_map(Cell::chars)
    };
    if first > rows.first() && rows.get(first - 1).is_some_and(Row::wrapped) {
        haystack.extend(chars(first - 1));
    }
    let start = haystack.len();
    for number in text.clone() {
        haystack.extend(chars(number));
    }
    let end = haystack.len();
    if start == end {
        return;
    }
    if last < rows.last() && rows.get(last).is_some_and(Row::wrapped) {
        haystack.extend(chars(last + 1));
    }

    let mut spans = Vec::new();
    let mut searcher = Searcher::new(Input::new(haystack.as_str()).span(start..end));
    while let Some(found) = searcher.advance(|input| Ok(regex.search_with(cache, input))) {
        if !found.is_empty() {
            spans.push(found.range());
        }
    }
    if spans.is_empty() {
        return;
    }
    let mut placed = Vec::new();
    let mut byte = start;
    for number in text {
        let Some(row) = rows.get(number) else {
            continue;
        };
        for (col, cell) in read(rows, number, row).iter().enumerate() {
            let length: usize = cell.chars().map(char::len_utf8).sum();
            if length > 0 {
                placed.push(Placed {
                    byte,
                    row: number,
                    col,
                    width: cell.width().max(1),
                });
                byte += length;
            }
        }
    }
    for span in spans {
        let first = &placed[placed.partition_point(|cell| cell.byte <= span.start) - 1];
        let last = &placed[placed.partition_point(|cell| cell.byte < span.end) - 1];
        let new = Match {
            start: Spot {
                row: first.row,
                col: first.col,
            },
            end: Spot {
                row: last.row,
                col: last.col + last.width,
            },
        };
        // Two matches in the characters of one cell, as of a letter and the mark joined to it:
        // the cell is the first's.
        if found.last().is_some_and(|before| before.end > new.start) {
            continue;
        }
        found.push(new);
    }
}

/// What find found (TERM-18): the matches and which is current, for the Renderer to draw and
/// the find bar to count.
#[derive(Clone, Copy)]
pub struct Found<'a> {
    matches: &'a VecDeque<Match>,
    current: Option<usize>,
    more: bool,
    invalid: bool,
    cols: usize,
}

impl Found<'_> {
    /// How many matches are kept: no more than the newest 100,000.
    pub fn count(&self) -> usize {
        self.matches.len()
    }

    /// There are older matches than the newest 100,000 kept, which are neither drawn nor
    /// reached.
    pub fn more(&self) -> bool {
        self.more
    }

    /// The query is no pattern, or one too large to search, so nothing is found.
    pub fn invalid(&self) -> bool {
        self.invalid
    }

    /// Which match is current, counted from the oldest kept, from 0. None without matches.
    pub fn current(&self) -> Option<usize> {
        self.current
    }

    /// The numbers of the first and the last row of the current match, for the view to show.
    pub fn current_rows(&self) -> Option<RangeInclusive<u64>> {
        let found = self.matches.get(self.current?)?;
        Some(found.start.row..=found.end.row)
    }

    /// The cells of row `number` that matches hold, each run of them with whether it is the
    /// current match's, in reading order.
    pub fn cols(&self, number: u64) -> impl Iterator<Item = (Range<usize>, bool)> + '_ {
        let first = self.matches.partition_point(|found| found.end.row < number);
        (first..self.matches.len())
            .map_while(move |index| {
                let found = self.matches[index];
                (found.start.row <= number).then_some((index, found))
            })
            .map(move |(index, found)| {
                let start = if found.start.row == number {
                    found.start.col
                } else {
                    0
                };
                let end = if found.end.row == number {
                    found.end.col
                } else {
                    self.cols
                };
                (start..end, Some(index) == self.current)
            })
    }
}
