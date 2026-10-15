//! The colors programs set over the theme's, and their questions about them (TERM-12): OSC 4
//! for the 256 colors of the palette, 10 to 12 for the foreground, the background and the
//! cursor, and 104 and 110 to 112 to give them back.
//!
//! What the theme's colors look like is the Renderer's, so the core never answers a question
//! itself: it says what was asked, and what the program had set, and the Tab answers with the
//! Renderer's color when the program set none. The Renderer draws with these colors where a
//! program set them, and with the theme's elsewhere (#22).

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

use crate::osc::End;

/// One of the theme's colors that a program may ask for or set.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ThemeColor {
    /// One of the 256 colors of the palette (OSC 4).
    Palette(u8),
    /// The text's default color (OSC 10).
    Foreground,
    /// The default background (OSC 11).
    Background,
    /// The cursor's color (OSC 12).
    Cursor,
}

/// A color a program set: red, green and blue. XParseColor reads 16 bits each, of which the
/// high 8 are kept, as a display of 24-bit color keeps them for xterm, and as they are drawn.
pub type Rgb = [u8; 3];

/// The colors programs set, each over the theme's.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Colors {
    palette: [Option<Rgb>; 256],
    foreground: Option<Rgb>,
    background: Option<Rgb>,
    cursor: Option<Rgb>,
}

// Written out, since an array of 256 has no `Default` of its own.
impl Default for Colors {
    fn default() -> Self {
        Self {
            palette: [None; 256],
            foreground: None,
            background: None,
            cursor: None,
        }
    }
}

impl Colors {
    /// The color a program set for `color`, or none for the theme's.
    pub fn get(&self, color: ThemeColor) -> Option<Rgb> {
        match color {
            ThemeColor::Palette(index) => self.palette[usize::from(index)],
            ThemeColor::Foreground => self.foreground,
            ThemeColor::Background => self.background,
            ThemeColor::Cursor => self.cursor,
        }
    }

    /// Sets `color` to `to`, or gives it back to the theme.
    pub(crate) fn set(&mut self, color: ThemeColor, to: Option<Rgb>) {
        let slot = match color {
            ThemeColor::Palette(index) => &mut self.palette[usize::from(index)],
            ThemeColor::Foreground => &mut self.foreground,
            ThemeColor::Background => &mut self.background,
            ThemeColor::Cursor => &mut self.cursor,
        };
        *slot = to;
    }

    /// Gives the 256 colors of the palette back to the theme, as OSC 104 does without a number
    /// and as xterm's RIS and DECSTR do.
    pub(crate) fn reset_palette(&mut self) {
        self.palette = [None; 256];
    }
}

/// A program asked for one of the theme's colors (OSC 4, 10, 11 or 12 with `?`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ColorQuery {
    color: ThemeColor,
    /// What the program had set when it asked.
    set: Option<Rgb>,
    end: End,
}

impl ColorQuery {
    pub(crate) fn new(color: ThemeColor, set: Option<Rgb>, end: End) -> Self {
        Self { color, set, end }
    }

    /// The color asked for.
    pub fn color(&self) -> ThemeColor {
        self.color
    }

    /// The answer, as xterm gives it: the sequence that would set the color, ended as the
    /// question was. The color is the one the program had set when it asked, or else `theme`,
    /// the theme's, as the Renderer gives it. Each of its red, green and blue is written in
    /// four hex digits, its 8 bits twice, as xterm writes those of a display of 24-bit color.
    /// Fixed text and hex digits only (SEC-05).
    pub fn reply(&self, theme: Rgb) -> Vec<u8> {
        let [red, green, blue] = self
            .set
            .unwrap_or(theme)
            .map(|component| u16::from(component) * 0x101);
        let number = match self.color {
            ThemeColor::Palette(index) => format!("4;{index}"),
            ThemeColor::Foreground => String::from("10"),
            ThemeColor::Background => String::from("11"),
            ThemeColor::Cursor => String::from("12"),
        };
        let end = self.end.text();
        format!("\x1b]{number};rgb:{red:04x}/{green:04x}/{blue:04x}{end}").into_bytes()
    }
}
