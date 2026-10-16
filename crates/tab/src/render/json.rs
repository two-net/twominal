//! A frame as JSON text: the wire format between the Renderer's two halves.
//!
//! Tauri 2.12 hands a channel message of under 8 KB to the webview as part of a script
//! (`ipc/channel.rs`), so the text a program writes ends up in that script (REL-02). Every
//! string is written as both JSON and JavaScript read it the same way: quotes, backslashes,
//! every control character, and U+2028 and U+2029, which JavaScript once took for line ends,
//! are escaped. Every number is finite.
//!
//! ```text
//! { "full":   { "canvas": [w, h], "grid": [cols, rows], "scale": s, "cell": [w, h],
//!               "baseline": b, "font": "…", "fontSize": s, "background": "#rrggbb",
//!               "underlines": [single, double, curly, dotted, dashed], "strikethrough": shape },
//!   "shift":  n,
//!   "offset": n,
//!   "colors": ["#rrggbb", …],
//!   "styles": [[color, flags], [color, flags, underline color], …],
//!   "shapes": { "─": shape, … },
//!   "rows":   [[row, [col, count, color, …], [[col, style, "text"], [col, style, "text", cells], …]], …],
//!   "cursor": { "cell": [row, col, cells], "color": color, "shape": shape,
//!               "text": [col, style, "text"], "blink": true, "fade": true, "glide": true },
//!   "find":   { "query": ["before the caret", "composed", "after it"], "match": [n, count],
//!               "selected": true, "case": true, "regex": true, "focused": true, "more": true,
//!               "invalid": true },
//!   "link":   { "target": "…", "opens": true, "hand": true, "top": true },
//!   "caret":  [row, col] }
//! ```
//!
//! `full` comes only in a frame with every row. `grid` is the grid's size in cells, which the
//! canvas's top left holds: rows and the copy are cut at its edges, so the margins keep the
//! background. `shift` is how many device pixels the rows moved up since the last frame, so the
//! webview copies the grid that far before it paints the rows. `offset` is how many device pixels
//! up from its cell each row is drawn, while the view is scrolled to between two rows. Each is
//! there only when it is not 0. A row's index counts from the view's top. Colors and styles are
//! numbered by their place
//! in their lists, which hold those the frame uses, each once. A style's flags are 1 for bold, 2
//! for italic, 4 for crossed out, and the underline's style times 8: none, single, double,
//! curly, dotted and dashed. A row's spans are its backgrounds, three numbers each, and its runs
//! its text. A run without a count has a cell for each character, each drawn on its own. One
//! with a count of 0 has a cell for each character too, and is drawn as one so that the font's
//! ligatures form, stretched across to exactly its cells. One with another count is drawn whole
//! over that many cells. A shape is a list of pieces: `[x, y, width, height]` filled,
//! `[x, y, width, height, alpha]` filled at `alpha`, and `["p", x, y, x, y, …]` a polygon, in
//! device pixels from the cell's top left.
//!
//! `cursor` is there only while the cursor is drawn. It covers `cells` cells from `col` of `row`,
//! counted from the view's top as the rows are, and drawn `offset` up as they are. Its shape is
//! filled in its color, from its first cell's top left. `text` is the run under a block,
//! drawn in the cursor's text color and cut to the cursor's cells: it may start in a cell before
//! them, as a flag does. Each of `blink`, `fade` and `glide` is there only when true.
//!
//! `find` is the find bar, in every frame while it is open. Its query is cut at the caret, with
//! the text an input method is composing there between. `match` is the current match, counted
//! from 1, or 0 for none, and how many there are; `more` says there are more than that. Each of
//! `selected`, the whole query, `case`, `regex`, `focused`, the bar has the keys, `more` and
//! `invalid`, the query is no pattern, is there only when true.
//!
//! `link` is where the link under the mouse leads, in every frame while there is one. `target`
//! is the URL the OS is handed if it is clicked, or the link's own target when it may not open.
//! `opens` says it may open, `hand` that a click there would open it now, and `top` that the
//! mouse is in the view's lower half, so the target is shown at the top. Each is there only when
//! true.

use std::collections::HashMap;
use std::fmt::Write;
use std::hash::Hash;

use super::color::{Ink, Rgb};
use super::line::{Cells, Run};
use super::shapes::Primitive;
use super::{CursorFrame, FindFrame, Frame, Full, LinkFrame};

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
                write_run(&mut rows, run, styles.number(run.ink));
            }
            rows.push_str("]]");
        }
        // Numbered after the rows' styles, so that theirs are the same with or without it.
        let text = self.cursor.as_ref().and_then(|cursor| {
            let run = cursor.text.as_ref()?;
            Some((run, styles.number(run.ink)))
        });

        let mut out = String::with_capacity(rows.len() + 256);
        out.push('{');
        if let Some(full) = &self.full {
            out.push_str("\"full\":");
            write_full(&mut out, full);
            out.push(',');
        }
        if self.shift != 0 {
            let _ = write!(out, "\"shift\":{},", self.shift);
        }
        if self.offset != 0 {
            let _ = write!(out, "\"offset\":{},", self.offset);
        }
        // The styles name colors, so they are numbered before the colors are written.
        let mut written = String::new();
        for (index, ink) in styles.items.iter().enumerate() {
            if index > 0 {
                written.push(',');
            }
            write_style(&mut written, ink, &mut colors);
        }
        let mut cursor = String::new();
        if let Some(frame) = &self.cursor {
            write_cursor(&mut cursor, frame, colors.number(frame.color), text);
        }
        if let Some(find) = &self.find {
            write_find(&mut cursor, find);
        }
        if let Some(link) = &self.link {
            write_link(&mut cursor, link);
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
            "}},\"rows\":[{rows}],{cursor}\"caret\":[{},{}]}}",
            self.caret.row, self.caret.col
        );
        out
    }
}

/// `run` as a row holds it, in the style numbered `style`.
fn write_run(out: &mut String, run: &Run, style: usize) {
    let _ = write!(out, "[{},{style},", run.col);
    string(out, &run.text);
    match run.cells {
        Cells::Each => {}
        Cells::Joined => out.push_str(",0"),
        Cells::Whole(cells) => {
            let _ = write!(out, ",{cells}");
        }
    }
    out.push(']');
}

/// The cursor, in the color numbered `color`, with the text under it in its numbered style, and
/// the comma after it.
fn write_cursor(out: &mut String, cursor: &CursorFrame, color: usize, text: Option<(&Run, usize)>) {
    let _ = write!(
        out,
        "\"cursor\":{{\"cell\":[{},{},{}],\"color\":{color},\"shape\":",
        cursor.row, cursor.col, cursor.cells
    );
    write_shape(out, &cursor.shape);
    if let Some((run, style)) = text {
        out.push_str(",\"text\":");
        write_run(out, run, style);
    }
    for (on, name) in [
        (cursor.blinks, "blink"),
        (cursor.fades, "fade"),
        (cursor.glides, "glide"),
    ] {
        if on {
            let _ = write!(out, ",\"{name}\":true");
        }
    }
    out.push_str("},");
}

/// The find bar, and the comma after it.
fn write_find(out: &mut String, find: &FindFrame) {
    let bar = &find.bar;
    // A caret that is not on a character's start is at the end.
    let caret = if bar.query.is_char_boundary(bar.caret) {
        bar.caret
    } else {
        bar.query.len()
    };
    let (before, after) = bar.query.split_at(caret);
    out.push_str("\"find\":{\"query\":[");
    string(out, before);
    out.push(',');
    string(out, &bar.composing);
    out.push(',');
    string(out, after);
    let current = find.current.map_or(0, |current| current + 1);
    let _ = write!(out, "],\"match\":[{current},{}]", find.count);
    for (on, name) in [
        (bar.selected, "selected"),
        (bar.case, "case"),
        (bar.regex, "regex"),
        (bar.focused, "focused"),
        (find.more, "more"),
        (find.invalid, "invalid"),
    ] {
        if on {
            let _ = write!(out, ",\"{name}\":true");
        }
    }
    out.push_str("},");
}

/// Where the link under the mouse leads, and the comma after it.
fn write_link(out: &mut String, link: &LinkFrame) {
    out.push_str("\"link\":{\"target\":");
    string(out, &link.target);
    for (on, name) in [
        (link.opens, "opens"),
        (link.hand, "hand"),
        (link.top, "top"),
    ] {
        if on {
            let _ = write!(out, ",\"{name}\":true");
        }
    }
    out.push_str("},");
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
        "{{\"canvas\":[{},{}],\"grid\":[{},{}],\"scale\":",
        geometry.canvas.0, geometry.canvas.1, layout.size.cols, layout.size.rows
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
