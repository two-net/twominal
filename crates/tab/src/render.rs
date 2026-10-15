//! Renderer (SRS §6.1).
//!
//! Draws the Terminal Core's state and the command line with the active font, ligatures and
//! palette.
//!
//! Must not change terminal state. This half only reads: it lays out the grid in whole device
//! pixels from what the webview measured, and turns what the Terminal Core holds, with the text
//! being composed laid over it, into a [`Frame`] of the rows that changed. Its other half,
//! `src/tab/renderer.js`, measures the font and the terminal area and paints frames on the
//! window's canvas, a pixel for each of the display's, so text is sharp at any scale (HW-03).
//!
//! `layout.rs` makes the measurements whole pixels, `color.rs` fills in the colors, `line.rs`
//! builds each row as it is drawn and cuts it into runs, among them the runs that may hold the
//! font's ligatures, which the cursor splits (FONT-04 to FONT-07), `shapes.rs` makes box
//! drawing, blocks, powerline symbols and underlines into shapes that join from cell to cell
//! (FONT-10), and `json.rs` writes the frame. DESIGN.md §3 has the rules.
//!
//! A tab sends a new frame only after the last one was drawn, so frames never queue behind
//! heavy output (PERF-07).
//!
//! Filled in by #22 (grid), #23 (fonts), #24 (ligatures) and #25 (cursor).

mod color;
mod json;
mod layout;
mod line;
mod shapes;
#[cfg(test)]
mod tests;

use std::collections::{BTreeMap, HashMap};
use std::mem;
use std::sync::Arc;

use terminal_core::{Cursor, TerminalCore};

pub use color::{Palette, Rgb};
pub use layout::{Layout, Measure};

use layout::Geometry;
use line::{Cells, Line, Run, Span};
use shapes::Primitive;

/// One tab's render state: what the window was last sent and what has changed since.
pub struct Renderer {
    palette: Palette,
    /// Whether the font's ligatures are drawn: the Font ligatures setting (FONT-03).
    ligatures: bool,
    /// The layout of what the webview measured last. Nothing is drawn before it has measured.
    geometry: Option<Arc<Geometry>>,
    /// The rows as the window was last sent them.
    sent: Vec<Line>,
    /// Where the window was last told the caret is.
    caret: Option<Cursor>,
    /// Whether the next frame has every row.
    full: bool,
    /// The shapes of the characters drawn as shapes, for the layout in use. `None` for a
    /// character in their ranges that the font draws.
    shapes: HashMap<char, Option<Arc<[Primitive]>>>,
}

/// What to draw: the rows that changed, where the caret is, and, when every row is in it, the
/// layout. The wire format between the two halves of the Renderer, as [`Frame::json`] writes it.
/// No other code reads it.
#[derive(Clone, Debug, PartialEq)]
pub struct Frame {
    /// The layout and the background, in a frame with every row.
    full: Option<Full>,
    /// The rows that changed, from the top.
    rows: Vec<RowFrame>,
    /// The shapes of the characters the rows draw as shapes.
    shapes: BTreeMap<char, Arc<[Primitive]>>,
    /// Where the text being composed starts, or the cursor while nothing is: the cell the
    /// Input Handler's textarea follows (#31).
    caret: Cursor,
}

/// What a frame with every row has besides them.
#[derive(Clone, Debug, PartialEq)]
struct Full {
    geometry: Arc<Geometry>,
    /// The palette's background, which every row is painted on.
    background: Rgb,
}

/// One row of a frame.
#[derive(Clone, Debug, PartialEq, Eq)]
struct RowFrame {
    index: u16,
    spans: Vec<Span>,
    runs: Vec<Run>,
}

impl Renderer {
    /// A tab's Renderer, which draws with `palette`. It draws nothing until the webview has
    /// measured.
    pub fn new(palette: Palette) -> Self {
        Self {
            palette,
            // Appendix B's default.
            ligatures: true,
            geometry: None,
            sent: Vec::new(),
            caret: None,
            full: true,
            shapes: HashMap::new(),
        }
    }

    /// A new palette: the theme changed (THM-03). The next frame has every row.
    pub fn set_palette(&mut self, palette: Palette) {
        if palette != self.palette {
            self.palette = palette;
            self.full = true;
        }
    }

    /// Whether the font's ligatures are drawn: the Font ligatures setting changed (FONT-03). The
    /// next frame has every row.
    pub fn set_ligatures(&mut self, on: bool) {
        if on != self.ligatures {
            self.ligatures = on;
            self.full = true;
        }
    }

    /// What the webview measured: the font, the scale and the terminal area. Returns the layout,
    /// which the Tab resizes the core, the Session and the Input Handler to. None when the
    /// measurements are no size, as when the terminal area is hidden: the grid then stays as it
    /// was. When anything changed, the next frame has every row.
    pub fn measure(&mut self, measure: &Measure) -> Option<Layout> {
        let geometry = Geometry::new(measure)?;
        let layout = geometry.layout;
        if self.geometry.as_deref() != Some(&geometry) {
            self.geometry = Some(Arc::new(geometry));
            self.shapes.clear();
            self.full = true;
        }
        Some(layout)
    }

    /// Makes the next frame have every row, for a window that has nothing of this tab's on it:
    /// one that has just attached to it, or switched to it (TAB-09).
    pub fn redraw(&mut self) {
        self.full = true;
    }

    /// What changed since the last frame: what `core` holds, with `composing`, the text an input
    /// method is composing, over the cursor. None when nothing did, and before the webview has
    /// measured.
    pub fn frame(&mut self, core: &TerminalCore, composing: &str) -> Option<Frame> {
        let geometry = self.geometry.clone()?;
        let cursor = core.cursor();
        let mut lines: Vec<Line> = core
            .screen()
            .rows()
            .iter()
            .map(|row| line::draw(row, &self.palette))
            .collect();
        let mut caret = cursor;
        if !composing.is_empty() {
            if let Some(line) = lines.get_mut(usize::from(cursor.row)) {
                let col = line::compose(line, usize::from(cursor.col), composing, &self.palette);
                caret.col = col as u16;
            }
        }
        if self.ligatures {
            // A cursor that is shown splits the ligature it is on (FONT-07).
            let cursor = core.modes().cursor_visible.then_some(cursor);
            for (index, line) in lines.iter_mut().enumerate() {
                let col = cursor
                    .filter(|cursor| usize::from(cursor.row) == index)
                    .map(|cursor| usize::from(cursor.col));
                line::ligatures(line, col);
            }
        }

        let full = mem::take(&mut self.full)
            || lines.len() != self.sent.len()
            || lines.first().map(Vec::len) != self.sent.first().map(Vec::len);
        let background = self.palette.background;
        let rows: Vec<RowFrame> = lines
            .iter()
            .enumerate()
            .filter(|(index, line)| full || self.sent[*index] != **line)
            .map(|(index, line)| RowFrame {
                index: index as u16,
                spans: line::spans(line, background),
                runs: line::runs(line),
            })
            .collect();
        self.sent = lines;
        if !full && rows.is_empty() && self.caret == Some(caret) {
            return None;
        }
        self.caret = Some(caret);

        let mut shapes = BTreeMap::new();
        let chars = rows
            .iter()
            .flat_map(|row| &row.runs)
            .filter(|run| run.cells == Cells::Each)
            .flat_map(|run| run.text.chars())
            .filter(|char| shapes::is_shape(*char));
        for char in chars {
            let shape = self.shapes.entry(char).or_insert_with(|| {
                let layout = &geometry.layout;
                shapes::shape(
                    char,
                    layout.cell_width,
                    layout.cell_height,
                    geometry.light,
                    geometry.heavy,
                )
                .map(Arc::from)
            });
            if let Some(shape) = shape {
                shapes.insert(char, Arc::clone(shape));
            }
        }

        Some(Frame {
            full: full.then_some(Full {
                geometry,
                background,
            }),
            rows,
            shapes,
            caret,
        })
    }
}
