//! Each row as it is drawn, and how a frame sends it: the text as runs of cells drawn alike, and
//! the backgrounds as spans.
//!
//! A row is built from the core's, with every color filled in, the text being composed laid
//! over it (HW-01), and the cells that may join a ligature marked (FONT-04 to FONT-07). The
//! Renderer compares rows built this way with the ones it sent last, so anything laid over the
//! screen is drawn and sent the same way.

use terminal_core::{width, Row};

use super::color::{Ink, Look, Palette, Rgb};

/// The most characters a cell holds: its own and two joined to it, as the core keeps them.
const CHARS: usize = 3;

/// Zero Width Joiner: joins the character before it to the one after, as in 👨‍👩‍👧.
const ZWJ: char = '\u{200D}';

/// One cell as it is drawn.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Drawn {
    /// The cell's text: its character, then those joined to it. Empty for the right half of a
    /// wide character, and for hidden text.
    chars: [Option<char>; CHARS],
    /// 1, 2 for a wide character, or 0 for its right half.
    width: u8,
    look: Look,
    /// The cell may be drawn as one with the cells beside it, so that the font's ligatures form
    /// (FONT-04). Set by [`ligatures`].
    joins: bool,
}

impl Drawn {
    /// A blank cell that looks like `look`.
    fn blank(look: Look) -> Self {
        Self {
            chars: [Some(' '), None, None],
            width: 1,
            look,
            joins: false,
        }
    }

    fn text(&self) -> impl Iterator<Item = char> + '_ {
        self.chars.iter().map_while(|char| *char)
    }

    /// The cell's character, if it holds one and nothing joined to it.
    fn alone(&self) -> Option<char> {
        self.chars[0].filter(|_| self.chars[1].is_none())
    }
}

/// A row as it is drawn: a cell for each column.
pub(crate) type Line = Vec<Drawn>;

/// `row` as it is drawn with `palette`.
pub(crate) fn draw(row: &Row, palette: &Palette) -> Line {
    let mut last = None;
    row.cells()
        .iter()
        .map(|cell| {
            // Neighbouring cells mostly share a style, so its look is kept.
            let look = match last {
                Some((style, look)) if style == *cell.style() => look,
                _ => {
                    let look = palette.look(cell.style());
                    last = Some((*cell.style(), look));
                    look
                }
            };
            let mut chars = [None; CHARS];
            if !look.hidden {
                for (slot, char) in chars.iter_mut().zip(cell.chars()) {
                    *slot = Some(char);
                }
            }
            Drawn {
                chars,
                width: cell.width() as u8,
                look,
                joins: false,
            }
        })
        .collect()
}

/// Lays `text`, which an input method is composing, over `line` from column `col`, in the
/// default colors and underlined, each character in the cells the core would give it. Text that
/// would pass the last column starts further left, and text longer than the row shows its end.
/// Returns the column it starts in, the caret, or None when it takes no cell, as control
/// characters and marks with nothing before them take none.
pub(crate) fn compose(line: &mut Line, col: usize, text: &str, palette: &Palette) -> Option<usize> {
    let look = palette.composing();
    let mut cells: Vec<Drawn> = Vec::new();
    for char in text.chars().filter(|char| !char.is_control()) {
        match width(char) {
            0 => {
                // Joined to the character before it, or dropped with none.
                let last = cells.iter_mut().rev().find(|cell| cell.width > 0);
                if let Some(slot) =
                    last.and_then(|cell| cell.chars.iter_mut().find(|slot| slot.is_none()))
                {
                    *slot = Some(char);
                }
            }
            width => {
                cells.push(Drawn {
                    chars: [Some(char), None, None],
                    width: width as u8,
                    look,
                    joins: false,
                });
                if width == 2 {
                    cells.push(Drawn {
                        chars: [None; CHARS],
                        width: 0,
                        look,
                        joins: false,
                    });
                }
            }
        }
    }
    let cols = line.len();
    if cells.is_empty() || cols == 0 {
        return None;
    }
    if cells.len() > cols {
        cells.drain(..cells.len() - cols);
        // The end of a wide character whose start was cut off.
        if cells[0].width == 0 {
            cells[0] = Drawn::blank(look);
        }
    }
    let start = col.min(cols - cells.len());
    let end = start + cells.len();
    // A wide character the text covers half of is blanked.
    if start > 0 && line[start].width == 0 {
        line[start - 1] = Drawn::blank(line[start - 1].look);
    }
    if end < cols && line[end].width == 0 {
        line[end] = Drawn::blank(line[end].look);
    }
    line[start..end].copy_from_slice(&cells);
    Some(start)
}

/// Marks the cells of `line` that may join a ligature: those holding one printable ASCII
/// character, a space included, with nothing joined to it (FONT-04). Programming ligatures are
/// made of ASCII, and any other character stays alone in its cell, where no shaper can move it
/// (FONT-05). The cell at `cursor`, and the cells beside it that a ligature could join to it,
/// are left unmarked, so that a ligature under the cursor is drawn as separate characters
/// (FONT-07): the cells around it that look alike and hold the same kind of character, letters
/// and digits or anything else, with no blank between.
pub(crate) fn ligatures(line: &mut Line, cursor: Option<usize>) {
    for col in 0..line.len() {
        // A skin tone after the character joins it, and they are drawn as one (`unit`).
        let toned = line
            .get(col + 1)
            .is_some_and(|next| next.chars[0].is_some_and(skin_tone));
        let cell = &mut line[col];
        cell.joins =
            cell.width == 1 && !toned && cell.alone().is_some_and(|char| matches!(char, ' '..='~'));
    }
    let Some(col) = cursor.filter(|col| line.get(*col).is_some_and(|cell| cell.joins)) else {
        return;
    };
    let at = line[col];
    if at.alone() == Some(' ') {
        // No ligature holds a blank.
        return;
    }
    let kind = |cell: &Drawn| cell.alone().map(|char| char.is_ascii_alphanumeric());
    let beside = |cell: &Drawn| {
        cell.joins && cell.look == at.look && cell.alone() != Some(' ') && kind(cell) == kind(&at)
    };
    let start = (0..col)
        .rev()
        .take_while(|before| beside(&line[*before]))
        .last()
        .unwrap_or(col);
    let end = (col + 1..line.len())
        .take_while(|after| beside(&line[*after]))
        .last()
        .map_or(col + 1, |last| last + 1);
    for cell in &mut line[start..end] {
        cell.joins = false;
    }
}

/// The cells a cursor at `col` of `line` covers: its own, or both of a wide character's from
/// either half. Returns the first, and how many.
pub(crate) fn covers(line: &Line, col: usize) -> (usize, usize) {
    match line.get(col).map(|cell| cell.width) {
        Some(0) if col > 0 => (col - 1, 2),
        Some(2) if col + 1 < line.len() => (col, 2),
        _ => (col, 1),
    }
}

/// The text the cell at `col` of `line` shows, as a run, for a block cursor to draw over in its
/// own colors: the cell's character, or the whole of what is drawn as one over it, such as a
/// wide character, a flag or a character and its skin tone, from its first cell. None where
/// nothing is drawn: a blank that is not underlined or crossed out, and hidden text.
pub(crate) fn under(line: &Line, col: usize) -> Option<Run> {
    runs(line).into_iter().find_map(|run| {
        let start = usize::from(run.col);
        match run.cells {
            Cells::Whole(cells) => (start..start + usize::from(cells))
                .contains(&col)
                .then_some(run),
            // A character a cell.
            Cells::Each | Cells::Joined => {
                let char = run.text.chars().nth(col.checked_sub(start)?)?;
                (char != ' ' || run.ink.decorates()).then(|| Run {
                    col: col as u16,
                    ink: run.ink,
                    text: char.to_string(),
                    cells: Cells::Each,
                })
            }
        }
    })
}

/// Cells next to each other whose text is drawn alike.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Run {
    /// The column of its first cell.
    pub(crate) col: u16,
    pub(crate) ink: Ink,
    pub(crate) text: String,
    pub(crate) cells: Cells,
}

/// How a run's text takes its cells.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Cells {
    /// Each character is a cell of its own, one column wide, and is drawn on its own.
    Each,
    /// Each character is a cell of its own, one column wide, and the text is drawn as one, so
    /// that the font's ligatures form, stretched across to exactly its cells (FONT-04, FONT-05).
    Joined,
    /// The text is drawn as one from the run's column, over this many cells: a wide character,
    /// a character with others joined to it, or a sequence drawn as one.
    Whole(u16),
}

/// Cells next to each other with one background.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Span {
    pub(crate) col: u16,
    pub(crate) count: u16,
    pub(crate) color: Rgb,
}

/// The backgrounds of `line`, but where they are `background`, on which every row is painted.
pub(crate) fn spans(line: &Line, background: Rgb) -> Vec<Span> {
    let mut spans: Vec<Span> = Vec::new();
    for (col, cell) in line.iter().enumerate() {
        let color = cell.look.background;
        match spans.last_mut() {
            Some(span) if span.color == color && usize::from(span.col + span.count) == col => {
                span.count += 1;
            }
            _ => spans.push(Span {
                col: col as u16,
                count: 1,
                color,
            }),
        }
    }
    spans.retain(|span| span.color != background);
    spans
}

/// The text of `line`, as runs. Cells next to each other that may join a ligature and look
/// alike, background and all, are a run drawn as one (FONT-06), unless it holds a single
/// character. The other cells, each drawn on its own, make runs of one text color and
/// attributes.
pub(crate) fn runs(line: &Line) -> Vec<Run> {
    let mut runs = Vec::new();
    // The run of cells each drawn on its own that the next such cell may join.
    let mut open: Option<Run> = None;
    let mut col = 0;
    while col < line.len() {
        let cell = &line[col];
        if cell.width == 0 || cell.chars[0].is_none() {
            // A right half, which its character's run covers, or hidden text.
            close(&mut runs, open.take());
            col += 1;
            continue;
        }
        if cell.joins {
            let end = (col..line.len())
                .find(|end| !(line[*end].joins && line[*end].look == cell.look))
                .unwrap_or(line.len());
            let mut joined = Run {
                col: col as u16,
                ink: cell.look.ink,
                text: line[col..end].iter().filter_map(Drawn::alone).collect(),
                cells: Cells::Joined,
            };
            trim(&mut joined);
            // Each character is ASCII, a byte.
            if joined.text.len() > 1 {
                close(&mut runs, open.take());
                runs.push(joined);
            } else {
                for (col, cell) in line.iter().enumerate().take(end).skip(col) {
                    if let Some(char) = cell.alone() {
                        add(&mut runs, &mut open, col, cell.look.ink, char);
                    }
                }
            }
            col = end;
            continue;
        }
        let (end, text) = unit(line, col);
        match (cell.alone(), end == col + 1 && cell.width == 1) {
            (Some(char), true) => add(&mut runs, &mut open, col, cell.look.ink, char),
            _ => {
                close(&mut runs, open.take());
                runs.push(Run {
                    col: col as u16,
                    ink: cell.look.ink,
                    text,
                    cells: Cells::Whole((end - col) as u16),
                });
            }
        }
        col = end;
    }
    close(&mut runs, open);
    runs
}

/// Adds `char`, in the cell at `col`, to `open`, the run of cells each drawn on its own, or
/// starts another when its ink is not `ink`.
fn add(runs: &mut Vec<Run>, open: &mut Option<Run>, col: usize, ink: Ink, char: char) {
    match open {
        Some(run) if run.ink == ink => run.text.push(char),
        _ => {
            close(runs, open.take());
            *open = Some(Run {
                col: col as u16,
                ink,
                text: char.to_string(),
                cells: Cells::Each,
            });
        }
    }
}

/// Adds `run`, if there is one, to `runs`, without the blanks at its ends that draw nothing.
fn close(runs: &mut Vec<Run>, run: Option<Run>) {
    let Some(mut run) = run else {
        return;
    };
    trim(&mut run);
    if !run.text.is_empty() {
        runs.push(run);
    }
}

/// Leaves off the blanks at the ends of `run` that draw nothing.
fn trim(run: &mut Run) {
    if !run.ink.decorates() {
        let kept = run.text.trim_start_matches(' ');
        run.col += (run.text.len() - kept.len()) as u16;
        run.text = kept.trim_end_matches(' ').to_string();
    }
}

/// Where the cells drawn as one from `col` end, and their text: the cell, or a sequence the core
/// keeps in several cells. Two regional indicators make a flag, a skin tone joins the character
/// before it, and a ZWJ joins whatever follows. Cells drawn unlike are never joined.
fn unit(line: &Line, col: usize) -> (usize, String) {
    let first = &line[col];
    let mut text: String = first.text().collect();
    let mut end = col + usize::from(first.width.max(1));
    let joins = |cell: &Drawn| cell.width > 0 && cell.chars[0].is_some() && cell.look == first.look;
    if first.alone().is_some_and(regional) {
        if let Some(next) = line.get(end).filter(|next| joins(next)) {
            if next.alone().is_some_and(regional) {
                text.extend(next.text());
                end += usize::from(next.width);
            }
        }
        return (end, text);
    }
    while let Some(next) = line.get(end).filter(|next| joins(next)) {
        if !(text.ends_with(ZWJ) || next.chars[0].is_some_and(skin_tone)) {
            break;
        }
        text.extend(next.text());
        end += usize::from(next.width);
    }
    (end, text)
}

/// A regional indicator: half of a flag.
fn regional(char: char) -> bool {
    matches!(char, '\u{1F1E6}'..='\u{1F1FF}')
}

/// One of the five skin tones that join an emoji before them.
fn skin_tone(char: char) -> bool {
    matches!(char, '\u{1F3FB}'..='\u{1F3FF}')
}
