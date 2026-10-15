//! Where the grid is drawn: what the webview measured, made into whole device pixels, so that
//! every character, line and block starts on a pixel at any scale (HW-03).

use terminal_core::Size;

use super::shapes::{self, Primitive};

/// The most pixels a side of a canvas may have.
const LARGEST: f64 = 16384.0;

/// The scales a display may have: `devicePixelRatio` is taken between these.
const SCALES: (f64, f64) = (0.5, 8.0);

/// What the webview half measured, in device pixels: the font at its size there, and the
/// terminal area. The wire format between the two halves of the Renderer, with [`Frame`].
///
/// [`Frame`]: super::Frame
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Measure {
    /// `devicePixelRatio`: how many device pixels a CSS pixel is.
    pub scale: f64,
    /// The font as CSS names it: a family, or a list of them.
    pub font: String,
    /// The font's size: its size in CSS pixels times the scale.
    pub font_size: f64,
    /// How far a character advances.
    pub advance: f64,
    /// How far the font reaches above its baseline, and below it.
    pub ascent: f64,
    pub descent: f64,
    /// The terminal area's width and height.
    pub width: f64,
    pub height: f64,
}

/// The grid the Renderer lays the terminal out on: what the Tab resizes the Terminal Core, the
/// Session and the Input Handler to.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Layout {
    /// How many cells the terminal area holds.
    pub size: Size,
    /// A cell's size in device pixels.
    pub cell_width: u32,
    pub cell_height: u32,
    /// How many device pixels a CSS pixel is.
    pub scale: f64,
}

impl Layout {
    /// A cell's width and height in CSS pixels, as the Input Handler takes them.
    pub fn css_cell(&self) -> (f64, f64) {
        (
            f64::from(self.cell_width) / self.scale,
            f64::from(self.cell_height) / self.scale,
        )
    }
}

/// All that a frame with every row tells the webview besides the rows: the layout, the canvas
/// and the font, and what is drawn the same in every cell.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Geometry {
    pub(crate) layout: Layout,
    /// The canvas's size: the whole terminal area, the grid at its top left.
    pub(crate) canvas: (u32, u32),
    /// How far the baseline is from a cell's top.
    pub(crate) baseline: u32,
    pub(crate) font: String,
    pub(crate) font_size: f64,
    /// How thick a light line is, and a heavy one.
    pub(crate) light: u32,
    pub(crate) heavy: u32,
    /// A cell's underlines: single, double, curly, dotted and dashed.
    pub(crate) underlines: [Vec<Primitive>; 5],
    pub(crate) strikethrough: Vec<Primitive>,
}

impl Geometry {
    /// The layout for what the webview measured. None when a measurement is not a number above
    /// zero, as when the terminal area is hidden: then the grid stays as it was.
    pub(crate) fn new(measure: &Measure) -> Option<Self> {
        let positive = |value: f64| value.is_finite() && value > 0.0;
        let sizes = [
            measure.scale,
            measure.font_size,
            measure.advance,
            measure.ascent,
            measure.width,
            measure.height,
        ];
        let descent = measure.descent.is_finite() && measure.descent >= 0.0;
        if !(sizes.into_iter().all(positive) && descent) {
            return None;
        }
        // `as` saturates, and every value here is positive and at most `LARGEST`.
        let pixels = |value: f64| value.min(LARGEST) as u32;
        let cell_width = pixels(measure.advance.round()).max(1);
        let ascent = pixels(measure.ascent.ceil());
        let cell_height = (ascent + pixels(measure.descent.ceil())).clamp(1, LARGEST as u32);
        let baseline = ascent.min(cell_height);
        let canvas = (
            pixels(measure.width.round()).max(1),
            pixels(measure.height.round()).max(1),
        );
        let count = |area: u32, cell: u32| u16::try_from(area / cell).unwrap_or(u16::MAX).max(1);
        let layout = Layout {
            size: Size {
                cols: count(canvas.0, cell_width),
                rows: count(canvas.1, cell_height),
            },
            cell_width,
            cell_height,
            scale: measure.scale.clamp(SCALES.0, SCALES.1),
        };

        // A light line is 7% of the font, at least a pixel, and no more than a third of the
        // cell, so a double line's two rails and the room between them fit. A heavy one has as
        // much more on each side, so the two share a centre.
        let font_size = measure.font_size.min(LARGEST);
        let room = (cell_width.min(cell_height) / 3).max(1);
        let light = ((font_size * 0.07).round() as u32).clamp(1, room);
        let heavy = light + 2 * (light / 2).max(1);
        let (underlines, strikethrough) =
            shapes::decorations(cell_width, cell_height, baseline, font_size, light);

        Some(Self {
            layout,
            canvas,
            baseline,
            font: measure.font.clone(),
            font_size,
            light,
            heavy,
            underlines,
            strikethrough,
        })
    }
}
