//! Each row as it is drawn, and how a frame sends it: the text as runs of cells drawn alike, and
//! the backgrounds as spans.
//!
//! A row is built from the core's, with every color filled in and the text being composed laid
//! over it (HW-01). The Renderer compares rows built this way with the ones it sent last, so
//! anything laid over the screen is drawn and sent the same way.

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
}

impl Drawn {
    /// A blank cell that looks like `look`.
    fn blank(look: Look) -> Self {
        Self {
            chars: [Some(' '), None, None],
            width: 1,
            look,
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
            }
        })
        .collect()
}

/// Lays `text`, which an input method is composing, over `line` from column `col`, in the
/// default colors and underlined, each character in the cells the core would give it. Text that
/// would pass the last column starts further left, and text longer than the row shows its end.
/// Returns the column it starts in: the caret.
pub(crate) fn compose(line: &mut Line, col: usize, text: &str, palette: &Palette) -> usize {
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
                });
                if width == 2 {
                    cells.push(Drawn {
                        chars: [None; CHARS],
                        width: 0,
                        look,
                    });
                }
            }
        }
    }
    let cols = line.len();
    if cells.is_empty() || cols == 0 {
        return col.min(cols.saturating_sub(1));
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
    start
}

/// Cells next to each other whose text is drawn alike.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Run {
    /// The column of its first cell.
    pub(crate) col: u16,
    pub(crate) ink: Ink,
    pub(crate) text: String,
    /// `None` when each character of `text` is a cell of its own, one column wide. Otherwise
    /// `text` is drawn as one from `col`, over this many cells: a wide character, a character
    /// with others joined to it, or a sequence drawn as one.
    pub(crate) cells: Option<u16>,
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

/// The text of `line`, as runs.
pub(crate) fn runs(line: &Line) -> Vec<Run> {
    let mut runs = Vec::new();
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
        let (end, text) = unit(line, col);
        match (cell.alone(), end == col + 1 && cell.width == 1) {
            (Some(char), true) => match &mut open {
                Some(run) if run.ink == cell.look.ink => run.text.push(char),
                _ => {
                    close(&mut runs, open.take());
                    open = Some(Run {
                        col: col as u16,
                        ink: cell.look.ink,
                        text: char.to_string(),
                        cells: None,
                    });
                }
            },
            _ => {
                close(&mut runs, open.take());
                runs.push(Run {
                    col: col as u16,
                    ink: cell.look.ink,
                    text,
                    cells: Some((end - col) as u16),
                });
            }
        }
        col = end;
    }
    close(&mut runs, open);
    runs
}

/// Adds `run`, if there is one, to `runs`, without the blanks at its ends that draw nothing.
fn close(runs: &mut Vec<Run>, run: Option<Run>) {
    let Some(mut run) = run else {
        return;
    };
    if !run.ink.decorates() {
        let kept = run.text.trim_start_matches(' ');
        run.col += (run.text.len() - kept.len()) as u16;
        run.text = kept.trim_end_matches(' ').to_string();
    }
    if !run.text.is_empty() {
        runs.push(run);
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
