//! A frame as JSON text: the wire format between the Renderer's two halves.
//!
//! Tauri 2.12 hands a channel message of under 8 KB to the webview as part of a script
//! (`ipc/channel.rs`), so the text a program writes ends up in that script (REL-02). Every
//! string is written as both JSON and JavaScript read it the same way: quotes, backslashes,
//! every control character, and U+2028 and U+2029, which JavaScript once took for line ends,
//! are escaped. Every number is finite.
//!
//! ```text
//! { "full":   { "canvas": [w, h], "scale": s, "cell": [w, h], "baseline": b,
//!               "font": "…", "fontSize": s, "background": "#rrggbb",
//!               "underlines": [single, double, curly, dotted, dashed], "strikethrough": shape },
//!   "colors": ["#rrggbb", …],
//!   "styles": [[color, flags], [color, flags, underline color], …],
//!   "shapes": { "─": shape, … },
//!   "rows":   [[row, [col, count, color, …], [[col, style, "text"], [col, style, "text", cells], …]], …],
//!   "caret":  [row, col] }
//! ```
//!
//! `full` comes only in a frame with every row. Colors and styles are numbered by their place
//! in their lists, which hold those the frame uses, each once. A style's flags are 1 for bold, 2
//! for italic, 4 for crossed out, and the underline's style times 8: none, single, double,
//! curly, dotted and dashed. A row's spans are its backgrounds, three numbers each, and its runs
//! its text. A run without a count has a cell for each character, each drawn on its own. One
//! with a count of 0 has a cell for each character too, and is drawn as one so that the font's
//! ligatures form, stretched across to exactly its cells. One with another count is drawn whole
//! over that many cells. A shape is a list of pieces: `[x, y, width, height]` filled,
//! `[x, y, width, height, alpha]` filled at `alpha`, and `["p", x, y, x, y, …]` a polygon, in
//! device pixels from the cell's top left.

use std::collections::HashMap;
use std::fmt::Write;
use std::hash::Hash;

use super::color::{Ink, Rgb};
use super::line::Cells;
use super::shapes::Primitive;
use super::{Frame, Full};

impl Frame {
    /// The frame as JSON text, for the webview half.
    pub fn json(&self) -> String {
        let mut colors = Numbered::default();
        let mut styles = Numbered::default();

        let mut rows = String::new();
        for (index, row) in self.rows.iter().enumerate() {
            if index > 0 {
                rows.push(',');
            }
            let _ = write!(rows, "[{},[", row.index);
            for (index, span) in row.spans.iter().enumerate() {
                if index > 0 {
                    rows.push(',');
                }
                let color = colors.number(span.color);
                let _ = write!(rows, "{},{},{color}", span.col, span.count);
            }
            rows.push_str("],[");
            for (index, run) in row.runs.iter().enumerate() {
                if index > 0 {
                    rows.push(',');
                }
                let style = styles.number(run.ink);
                let _ = write!(rows, "[{},{style},", run.col);
                string(&mut rows, &run.text);
                match run.cells {
                    Cells::Each => {}
                    Cells::Joined => rows.push_str(",0"),
                    Cells::Whole(cells) => {
                        let _ = write!(rows, ",{cells}");
                    }
                }
                rows.push(']');
            }
            rows.push_str("]]");
        }

        let mut out = String::with_capacity(rows.len() + 256);
        out.push('{');
        if let Some(full) = &self.full {
            out.push_str("\"full\":");
            write_full(&mut out, full);
            out.push(',');
        }
        // The styles name colors, so they are numbered before the colors are written.
        let mut written = String::new();
        for (index, ink) in styles.items.iter().enumerate() {
            if index > 0 {
                written.push(',');
            }
            write_style(&mut written, ink, &mut colors);
        }
        out.push_str("\"colors\":[");
        for (index, color) in colors.items.iter().enumerate() {
            if index > 0 {
                out.push(',');
            }
            write_color(&mut out, *color);
        }
        let _ = write!(out, "],\"styles\":[{written}],\"shapes\":{{");
        for (index, (char, shape)) in self.shapes.iter().enumerate() {
            if index > 0 {
                out.push(',');
            }
            string(&mut out, char.encode_utf8(&mut [0; 4]));
            out.push(':');
            write_shape(&mut out, shape);
        }
        let _ = write!(
            out,
            "}},\"rows\":[{rows}],\"caret\":[{},{}]}}",
            self.caret.row, self.caret.col
        );
        out
    }
}

/// Things numbered in the order they are first met.
struct Numbered<T> {
    items: Vec<T>,
    numbers: HashMap<T, usize>,
}

impl<T> Default for Numbered<T> {
    fn default() -> Self {
        Self {
            items: Vec::new(),
            numbers: HashMap::new(),
        }
    }
}

impl<T: Copy + Eq + Hash> Numbered<T> {
    fn number(&mut self, item: T) -> usize {
        *self.numbers.entry(item).or_insert_with(|| {
            self.items.push(item);
            self.items.len() - 1
        })
    }
}

fn write_full(out: &mut String, full: &Full) {
    let geometry = &full.geometry;
    let layout = &geometry.layout;
    let _ = write!(
        out,
        "{{\"canvas\":[{},{}],\"scale\":",
        geometry.canvas.0, geometry.canvas.1
    );
    number(out, layout.scale);
    let _ = write!(
        out,
        ",\"cell\":[{},{}],\"baseline\":{},\"font\":",
        layout.cell_width, layout.cell_height, geometry.baseline
    );
    string(out, &geometry.font);
    out.push_str(",\"fontSize\":");
    number(out, geometry.font_size);
    out.push_str(",\"background\":");
    write_color(out, full.background);
    out.push_str(",\"underlines\":[");
    for (index, shape) in geometry.underlines.iter().enumerate() {
        if index > 0 {
            out.push(',');
        }
        write_shape(out, shape);
    }
    out.push_str("],\"strikethrough\":");
    write_shape(out, &geometry.strikethrough);
    out.push('}');
}

fn write_style(out: &mut String, ink: &Ink, colors: &mut Numbered<Rgb>) {
    let flags = u8::from(ink.bold)
        | u8::from(ink.italic) << 1
        | u8::from(ink.strikethrough) << 2
        | ink.underline << 3;
    let _ = write!(out, "[{},{flags}", colors.number(ink.color));
    if ink.underline != 0 {
        let _ = write!(out, ",{}", colors.number(ink.underline_color));
    }
    out.push(']');
}

fn write_color(out: &mut String, color: Rgb) {
    let _ = write!(
        out,
        "\"#{:02x}{:02x}{:02x}\"",
        color.red, color.green, color.blue
    );
}

fn write_shape(out: &mut String, shape: &[Primitive]) {
    out.push('[');
    for (index, primitive) in shape.iter().enumerate() {
        if index > 0 {
            out.push(',');
        }
        match primitive {
            Primitive::Rect {
                x,
                y,
                width,
                height,
                alpha,
            } => {
                let _ = write!(out, "[{x},{y},{width},{height}");
                if *alpha < 1.0 {
                    out.push(',');
                    number(out, *alpha);
                }
                out.push(']');
            }
            Primitive::Polygon(points) => {
                out.push_str("[\"p\"");
                for (x, y) in points {
                    out.push(',');
                    number(out, *x);
                    out.push(',');
                    number(out, *y);
                }
                out.push(']');
            }
        }
    }
    out.push(']');
}

/// `value` as a JSON number, to four places. Never `NaN` or an infinity, which JSON has no
/// words for.
fn number(out: &mut String, value: f64) {
    let value = if value.is_finite() {
        (value * 10_000.0).round() / 10_000.0 + 0.0
    } else {
        0.0
    };
    let _ = write!(out, "{value}");
}

/// `text` as a JSON string that JavaScript reads the same way.
pub(crate) fn string(out: &mut String, text: &str) {
    out.push('"');
    for char in text.chars() {
        match char {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\u{0}'..='\u{1F}' | '\u{7F}'..='\u{9F}' | '\u{2028}' | '\u{2029}' => {
                let _ = write!(out, "\\u{:04x}", u32::from(char));
            }
            char => out.push(char),
        }
    }
    out.push('"');
}
