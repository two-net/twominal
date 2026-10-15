//! Renderer (SRS §6.1).
//!
//! Draws the Terminal Core's state and the command line with the active font, ligatures and
//! palette, the colors programs set over it, and the cursor.
//!
//! Must not change terminal state. This half only reads: it lays out the grid in whole device
//! pixels from what the webview measured, and turns what the Terminal Core holds, with the text
//! being composed laid over it, into a [`Frame`] of the rows that changed and of the cursor, in
//! the shape a program asked for or the settings give (TERM-07). Its other half,
//! `src/tab/renderer.js`, measures the font and the terminal area and paints frames on the
//! window's canvas, a pixel for each of the display's, so text is sharp at any scale (HW-03).
//!
//! `layout.rs` makes the measurements whole pixels, `color.rs` fills in the colors, `line.rs`
//! builds each row as it is drawn and cuts it into runs, among them the runs that may hold the
//! font's ligatures, which the cursor splits (FONT-04 to FONT-07), `shapes.rs` makes box
//! drawing, blocks, powerline symbols and underlines into shapes that join from cell to cell
//! (FONT-10), and the cursor's shapes, and `json.rs` writes the frame. DESIGN.md §3 has the
//! rules.
//!
//! It keeps the view: how far it is scrolled back, which a trackpad moves pixel by pixel
//! (ANIM-06). Rows are known by the numbers the Terminal Core gives them, so a frame sends only
//! the rows that changed, not those that moved, and says how far they moved.
//!
//! A tab sends a new frame only after the last one was drawn, so frames never queue behind
//! heavy output (PERF-07).
//!
//! Filled in by #22 (grid), #23 (fonts), #24 (ligatures), #25 (cursor) and #26 (scrolling).

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

use terminal_core::{Cursor, CursorShape, Row, Size, TerminalCore, ThemeColor};

pub use color::{Palette, Rgb};
pub use layout::{Layout, Measure};

use color::Colors;
use layout::Geometry;
use line::{Cells, Line, Run, Span};
use shapes::{CursorLook, Primitive};

/// One tab's render state: what the window was last sent and what has changed since.
pub struct Renderer {
    palette: Palette,
    /// The colors of the last frame: the palette's, with those a program set over them.
    colors: Option<Colors>,
    /// Whether the font's ligatures are drawn: the Font ligatures setting (FONT-03).
    ligatures: bool,
    /// The cursor settings (TERM-07, ANIM-08).
    cursor_settings: CursorSettings,
    /// How much may move (ANIM-02).
    motion: Motion,
    /// Whether the window has the focus (ANIM-07).
    focused: bool,
    /// The layout of what the webview measured last. Nothing is drawn before it has measured.
    geometry: Option<Arc<Geometry>>,
    /// How far the view is scrolled back. None while it follows the output.
    view: Option<View>,
    /// The rows as the window was last sent them, from the view's top.
    sent: Vec<Line>,
    /// Where the rows of the last frame were: the number of the first, how many device pixels
    /// up they were drawn, and the number of the screen's first row.
    sent_at: Option<(u64, u32, u64)>,
    /// The core's size in the last frame. Another size numbers the rows anew.
    size: Option<Size>,
    /// Where the window was last told the caret is.
    caret: Option<Cursor>,
    /// The cursor the window was last told to draw.
    cursor: Option<CursorFrame>,
    /// Whether the next frame has every row.
    full: bool,
    /// The shapes of the characters drawn as shapes, for the layout in use. `None` for a
    /// character in their ranges that the font draws.
    shapes: HashMap<char, Option<Arc<[Primitive]>>>,
}

/// How far the view is scrolled back: the number of its top row, as the Terminal Core numbers
/// rows, and the part of a row past it (DESIGN.md §3, *The view*).
#[derive(Clone, Copy, Debug, PartialEq)]
struct View {
    top: f64,
    /// The core's size the rows were numbered at. A resize numbers them anew.
    size: Size,
}

/// What to draw: the rows that changed, the cursor, where the caret is, and, when every row is
/// in it, the layout. The wire format between the two halves of the Renderer, as
/// [`Frame::json`] writes it. No other code reads it.
#[derive(Clone, Debug, PartialEq)]
pub struct Frame {
    /// The layout and the background, in a frame with every row.
    full: Option<Full>,
    /// How many device pixels the rows moved up since the last frame: the webview copies what it
    /// painted that far before it paints the rows. 0 in a frame with every row.
    shift: i64,
    /// How many device pixels up the rows are drawn from their cells: the part of a row the view
    /// is scrolled past its top row.
    offset: u32,
    /// The rows that changed, from the view's top.
    rows: Vec<RowFrame>,
    /// The shapes of the characters the rows draw as shapes.
    shapes: BTreeMap<char, Arc<[Primitive]>>,
    /// The cursor, drawn over the rows. None while it is not drawn.
    cursor: Option<CursorFrame>,
    /// Where the text being composed starts, or the cursor while nothing is: the cell the
    /// Input Handler's textarea follows (#31).
    caret: Cursor,
}

/// The cursor as the webview draws it, over the rows (TERM-07, ANIM-07, ANIM-08).
#[derive(Clone, Debug, PartialEq)]
struct CursorFrame {
    row: u16,
    /// Its first cell: a wide character's first, from either half.
    col: u16,
    /// 1, or 2 over a wide character.
    cells: u16,
    color: Rgb,
    /// Its pieces, from the top left of its first cell.
    shape: Vec<Primitive>,
    /// The text under a block, in the cursor's text color, as a run of the row.
    text: Option<Run>,
    blinks: bool,
    /// The blink fades in and out.
    fades: bool,
    /// It glides from where it was.
    glides: bool,
}

/// How the cursor is drawn where no program says otherwise: the cursor settings (TERM-07,
/// ANIM-07, ANIM-08).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CursorSettings {
    /// Cursor style. A program may ask for another (DECSCUSR).
    pub style: CursorShape,
    /// Cursor blink. Off, the cursor never blinks, whatever a program asks.
    pub blink: bool,
    /// Cursor fade: the blink fades in and out.
    pub fade: bool,
    /// Cursor glide: the cursor glides from cell to cell.
    pub glide: bool,
}

impl Default for CursorSettings {
    /// Appendix B's.
    fn default() -> Self {
        Self {
            style: CursorShape::Block,
            blink: true,
            fade: false,
            glide: false,
        }
    }
}

/// How much may move (ANIM-02).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Motion {
    /// Anything: the Animations setting is On, and the OS does not ask for less.
    #[default]
    Full,
    /// Only cross-fades of 150 ms or less: the OS asks for reduced motion.
    Reduced,
    /// Nothing: the Animations setting is Off.
    Off,
}

/// What a frame with every row has besides them.
#[derive(Clone, Debug, PartialEq)]
struct Full {
    geometry: Arc<Geometry>,
    /// The background where a program set none, which every row is painted on: the palette's,
    /// or the one a program set over it.
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
            colors: None,
            // Appendix B's default.
            ligatures: true,
            cursor_settings: CursorSettings::default(),
            motion: Motion::Full,
            focused: true,
            geometry: None,
            view: None,
            sent: Vec::new(),
            sent_at: None,
            size: None,
            caret: None,
            cursor: None,
            full: true,
            shapes: HashMap::new(),
        }
    }

    /// A new palette: the theme changed (THM-03). The colors a program set stay over it. When
    /// the colors drawn change, the next frame has every row.
    pub fn set_palette(&mut self, palette: Palette) {
        self.palette = palette;
    }

    /// The theme's `color`, which a program that asks for it is told unless it set one itself
    /// (TERM-12): the Tab gives it to the question's [`terminal_core::ColorQuery::reply`].
    pub fn color(&self, color: ThemeColor) -> Rgb {
        self.palette.theme(color)
    }

    /// Whether the font's ligatures are drawn: the Font ligatures setting changed (FONT-03). The
    /// next frame has every row.
    pub fn set_ligatures(&mut self, on: bool) {
        if on != self.ligatures {
            self.ligatures = on;
            self.full = true;
        }
    }

    /// The cursor settings: its style, whether it blinks, and its effects (TERM-07, ANIM-08).
    /// Until it is called they are Appendix B's. The next frame has the cursor they draw.
    pub fn set_cursor(&mut self, settings: CursorSettings) {
        self.cursor_settings = settings;
    }

    /// How much may move: the Animations setting, and whether the OS asks for reduced motion
    /// (ANIM-02, #40). Until it is called, anything may. The cursor fades only when something
    /// may move, and glides only when anything may.
    pub fn set_motion(&mut self, motion: Motion) {
        self.motion = motion;
    }

    /// Whether the window has the focus. Without it the cursor is an outline that does not
    /// blink (ANIM-07). Until it is called, it has.
    pub fn set_focused(&mut self, focused: bool) {
        self.focused = focused;
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

    /// Scrolls the view by `rows`, toward newer output when positive: what the Input Handler
    /// makes of the wheel, a trackpad, or Shift+Page Up and Down. Fractional, so a trackpad
    /// moves it pixel by pixel (ANIM-06). It stops at the oldest row of `core`'s scrollback and
    /// at the bottom, where it follows the output again. On the alternate screen, which keeps no
    /// scrollback, it does nothing.
    pub fn scroll(&mut self, core: &TerminalCore, rows: f64) {
        if core.modes().alternate_screen || !rows.is_finite() {
            return;
        }
        let size = core.size();
        let scrollback = core.scrollback();
        let live = scrollback.start() + scrollback.len() as u64;
        let from = self
            .view
            .filter(|view| view.size == size)
            .map_or(live as f64, |view| view.top);
        let top = (from + rows).max(scrollback.start() as f64);
        self.view = (top < live as f64).then_some(View { top, size });
    }

    /// Takes the view back to the newest output, which it follows from then on: for what the
    /// user types, the text being composed and Clear.
    pub fn scroll_to_bottom(&mut self) {
        self.view = None;
    }

    /// The number of the view's top row in `core`, and how many device pixels up its rows are
    /// drawn, for cells `height` pixels high. A part of a row that rounds to a whole one is the
    /// next row. The view follows the output on the alternate screen, after a resize and at the
    /// bottom, and stops at the oldest row kept.
    fn top(&mut self, core: &TerminalCore, height: u32) -> (u64, u32) {
        let scrollback = core.scrollback();
        let live = scrollback.start() + scrollback.len() as u64;
        let view = self
            .view
            .filter(|view| view.size == core.size() && !core.modes().alternate_screen);
        let Some(mut view) = view else {
            self.view = None;
            return (live, 0);
        };
        view.top = view.top.max(scrollback.start() as f64);
        let whole = view.top.floor();
        let mut top = whole as u64;
        let mut offset = ((view.top - whole) * f64::from(height)).round() as u32;
        if offset >= height {
            top += 1;
            offset = 0;
        }
        if top >= live {
            self.view = None;
            return (live, 0);
        }
        self.view = Some(view);
        (top, offset)
    }

    /// What changed since the last frame: what `core` holds, with `composing`, the text an input
    /// method is composing, over the cursor, in the view, in the palette's colors and those the
    /// program set over them. None when nothing did, and before the webview has measured. When
    /// the colors changed, every row is sent.
    pub fn frame(&mut self, core: &TerminalCore, composing: &str) -> Option<Frame> {
        let geometry = self.geometry.clone()?;
        let colors = Colors::new(&self.palette, core.colors());
        if self.colors.as_ref() != Some(&colors) {
            self.full = true;
        }
        let layout = &geometry.layout;
        let size = core.size();
        let height = layout.cell_height;
        let (top, offset) = self.top(core, height);
        let scrollback = core.scrollback();
        let live = scrollback.start() + scrollback.len() as u64;

        // The view's rows: one more than the grid has while it is between two rows.
        let count = usize::from(size.rows) + usize::from(offset > 0);
        let row = |number: u64| -> &Row {
            match number.checked_sub(live) {
                Some(on_screen) => &core.screen().rows()[on_screen as usize],
                None => scrollback
                    .get((number - scrollback.start()) as usize)
                    .expect("a row the view has"),
            }
        };
        let mut lines: Vec<Line> = (top..top + count as u64)
            .map(|number| line::draw(row(number), usize::from(size.cols), &colors))
            .collect();

        // The line the cursor is on, if it is in the view.
        let cursor = core.cursor();
        let cursor_line = (live + u64::from(cursor.row))
            .checked_sub(top)
            .map(|index| index as usize)
            .filter(|&index| index < count);
        let mut caret = cursor;
        let mut composed = false;
        if let Some(line) = cursor_line
            .filter(|_| !composing.is_empty())
            .and_then(|index| lines.get_mut(index))
        {
            if let Some(col) = line::compose(line, usize::from(cursor.col), composing, &colors) {
                caret.col = col as u16;
                composed = true;
            }
        }
        if self.ligatures {
            // A cursor that is shown splits the ligature it is on (FONT-07).
            let split = cursor_line.filter(|_| core.modes().cursor_visible);
            for (index, line) in lines.iter_mut().enumerate() {
                let col = (split == Some(index)).then_some(usize::from(cursor.col));
                line::ligatures(line, col);
            }
        }

        // The view moved against the screen, so the cursor moves with the text and does not
        // glide.
        let moved = self
            .sent_at
            .is_some_and(|(sent_top, sent_offset, sent_live)| {
                (live - top, offset) != (sent_live - sent_top, sent_offset)
            });
        // The text being composed marks where it goes, so the cursor is not drawn over it.
        let shown = core.modes().cursor_visible && !composed;
        let cursor = cursor_line
            .filter(|_| shown)
            .and_then(|index| self.cursor(core, &lines, &geometry, &colors, index, moved));

        let full = mem::take(&mut self.full) || self.size != Some(size);
        // A row is drawn `offset` pixels up from its cell, and the grid cuts it.
        let height = i64::from(height);
        let grid = i64::from(layout.size.rows) * height;
        let whole = |index: usize, offset: u32| {
            let y = index as i64 * height - i64::from(offset);
            y >= 0 && y + height <= grid
        };
        let (shift, sent_top, sent_offset) = match self.sent_at {
            Some((sent_top, sent_offset, _)) if !full => (
                (top as i64 - sent_top as i64) * height + i64::from(offset)
                    - i64::from(sent_offset),
                sent_top,
                sent_offset,
            ),
            _ => (0, 0, 0),
        };
        // A row is not sent again when the window has it whole, and keeps it whole as it moves.
        let kept = |index: usize, line: &Line| {
            let Some(before) = (top + index as u64).checked_sub(sent_top) else {
                return false;
            };
            let before = before as usize;
            !full
                && whole(index, offset)
                && whole(before, sent_offset)
                && self.sent.get(before) == Some(line)
        };
        let background = colors.background;
        let rows: Vec<RowFrame> = lines
            .iter()
            .enumerate()
            .filter(|(index, line)| !kept(*index, line))
            .map(|(index, line)| RowFrame {
                index: index as u16,
                spans: line::spans(line, background),
                runs: line::runs(line),
            })
            .collect();
        self.sent = lines;
        self.sent_at = Some((top, offset, live));
        self.size = Some(size);
        self.colors = Some(colors);
        if !full
            && rows.is_empty()
            && shift == 0
            && self.caret == Some(caret)
            && self.cursor == cursor
        {
            return None;
        }
        self.caret = Some(caret);
        self.cursor.clone_from(&cursor);

        let mut shapes = BTreeMap::new();
        let chars = rows
            .iter()
            .flat_map(|row| &row.runs)
            .chain(cursor.iter().filter_map(|cursor| cursor.text.as_ref()))
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
            shift,
            offset,
            rows,
            shapes,
            cursor,
            caret,
        })
    }

    /// The cursor as it is drawn over `lines`, where `core` has it, on the line `index` of the
    /// view: in the shape the program asked for or the settings give, or an outline in a window
    /// without the focus, in the color of `colors`. None when it is outside the grid laid out, as
    /// before the Tab has resized the core to it. It does not glide when the view `moved`
    /// against the screen.
    fn cursor(
        &self,
        core: &TerminalCore,
        lines: &[Line],
        geometry: &Geometry,
        colors: &Colors,
        index: usize,
        moved: bool,
    ) -> Option<CursorFrame> {
        let Cursor { row, col } = core.cursor();
        let line = lines.get(index)?;
        let (col, cells) = line::covers(line, usize::from(col));
        let layout = &geometry.layout;
        if row >= layout.size.rows || col + cells > usize::from(layout.size.cols) {
            return None;
        }

        let settings = self.cursor_settings;
        let asked = core.modes().cursor_style;
        let look = if self.focused {
            match asked.map_or(settings.style, |style| style.shape) {
                CursorShape::Block => CursorLook::Block,
                CursorShape::Underline => CursorLook::Underline,
                CursorShape::Bar => CursorLook::Bar,
            }
        } else {
            CursorLook::Outline
        };
        // The setting Off wins over a program that asks for a blink.
        let blinks = self.focused && settings.blink && asked.is_none_or(|style| style.blinking);
        let text = if look == CursorLook::Block {
            line::under(line, col).map(|run| Run {
                ink: colors.under_cursor(run.ink),
                ..run
            })
        } else {
            None
        };
        Some(CursorFrame {
            row: index as u16,
            col: col as u16,
            cells: cells as u16,
            color: colors.cursor,
            shape: shapes::cursor(
                look,
                cells as u32,
                layout.cell_width,
                layout.cell_height,
                geometry.light,
            ),
            text,
            blinks,
            fades: blinks && settings.fade && self.motion != Motion::Off,
            glides: settings.glide && self.motion == Motion::Full && !moved,
        })
    }
}
