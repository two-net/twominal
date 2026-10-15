//! What color each cell is drawn in (TERM-03, TERM-04): the palette's 16, xterm's 240 others
//! and 24-bit colors, and what inverse, dim and hidden do to them.
//!
//! The Terminal Core keeps a color as the program named it. Here it becomes what the window
//! draws, so a new palette recolors what is already on the screen (THM-02, THM-03).

use terminal_core::{Color, Style, Underline};

/// A color as the window draws it.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct Rgb {
    pub red: u8,
    pub green: u8,
    pub blue: u8,
}

impl Rgb {
    pub const fn new(red: u8, green: u8, blue: u8) -> Self {
        Self { red, green, blue }
    }

    /// The color `eighths` eighths of the way from this one to `other`.
    fn toward(self, other: Self, eighths: u16) -> Self {
        let mix = |from: u8, to: u8| {
            let (from, to) = (u16::from(from), u16::from(to));
            ((from * (8 - eighths) + to * eighths + 4) / 8) as u8
        };
        Self::new(
            mix(self.red, other.red),
            mix(self.green, other.green),
            mix(self.blue, other.blue),
        )
    }
}

/// The colors the terminal is drawn with, out of the active theme (THM-04). The Theme Manager
/// makes it (#38): the tab tier has none of its own, since theme resolution is not its to do
/// (ARC-05).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Palette {
    /// The text's color where a program set none.
    pub foreground: Rgb,
    /// The background's color where a program set none, and around the grid.
    pub background: Rgb,
    /// The 16 ANSI colors: what SGR's 30 to 37 and 90 to 97 name.
    pub ansi: [Rgb; 16],
}

/// The levels of each component in xterm's 6×6×6 cube, colors 16 to 231.
const LEVELS: [u8; 6] = [0, 95, 135, 175, 215, 255];

impl Palette {
    /// What `color` looks like, with `default` for the default color.
    pub(crate) fn rgb(&self, color: Color, default: Rgb) -> Rgb {
        match color {
            Color::Default => default,
            Color::Palette(index @ 0..=15) => self.ansi[usize::from(index)],
            Color::Palette(index @ 16..=231) => {
                let cube = usize::from(index - 16);
                Rgb::new(LEVELS[cube / 36], LEVELS[cube / 6 % 6], LEVELS[cube % 6])
            }
            // The 24 grays, from 8 to 238.
            Color::Palette(index) => {
                let gray = 8 + 10 * (index - 232);
                Rgb::new(gray, gray, gray)
            }
            Color::Rgb(red, green, blue) => Rgb::new(red, green, blue),
        }
    }

    /// How a cell written in `style` is drawn.
    pub(crate) fn look(&self, style: &Style) -> Look {
        let mut color = self.rgb(style.foreground, self.foreground);
        let mut background = self.rgb(style.background, self.background);
        if style.inverse {
            (color, background) = (background, color);
        }
        if style.dim {
            color = color.toward(background, 4);
        }
        let underline = if style.hidden {
            0
        } else {
            underline_number(style.underline)
        };
        Look {
            ink: Ink {
                color,
                bold: style.bold,
                italic: style.italic,
                strikethrough: style.strikethrough && !style.hidden,
                underline,
                // A color no underline shows would only split runs.
                underline_color: if underline == 0 {
                    color
                } else {
                    self.rgb(style.underline_color, color)
                },
            },
            background,
            hidden: style.hidden,
        }
    }

    /// How the text being composed is drawn: in the default colors, underlined.
    pub(crate) fn composing(&self) -> Look {
        Look {
            ink: Ink {
                color: self.foreground,
                bold: false,
                italic: false,
                strikethrough: false,
                underline: underline_number(Underline::Single),
                underline_color: self.foreground,
            },
            background: self.background,
            hidden: false,
        }
    }
}

/// The underline's style as kitty numbers it, as the frame sends it: 0 for none.
fn underline_number(underline: Underline) -> u8 {
    match underline {
        Underline::None => 0,
        Underline::Single => 1,
        Underline::Double => 2,
        Underline::Curly => 3,
        Underline::Dotted => 4,
        Underline::Dashed => 5,
    }
}

/// How a cell is drawn: its text, and its background.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Look {
    pub(crate) ink: Ink,
    pub(crate) background: Rgb,
    /// The cell's text is not drawn: SGR 8.
    pub(crate) hidden: bool,
}

/// What a cell's text is drawn with: all of how it looks but its background. Neighbouring cells
/// with the same ink are one run.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) struct Ink {
    pub(crate) color: Rgb,
    pub(crate) bold: bool,
    pub(crate) italic: bool,
    pub(crate) strikethrough: bool,
    /// 0 for none, then single, double, curly, dotted and dashed.
    pub(crate) underline: u8,
    /// The text's color when the underline has none of its own, or there is no underline.
    pub(crate) underline_color: Rgb,
}

impl Ink {
    /// Whether a blank in this ink draws anything: a blank cell can still be underlined or
    /// crossed out.
    pub(crate) fn decorates(&self) -> bool {
        self.underline != 0 || self.strikethrough
    }
}
