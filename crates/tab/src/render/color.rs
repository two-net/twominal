//! What color each cell is drawn in (TERM-03, TERM-04): the palette's 16, xterm's 240 others
//! and 24-bit colors, and what inverse, dim and hidden do to them.
//!
//! The Terminal Core keeps a color as the program named it. Here it becomes what the window
//! draws, so a new palette recolors what is already on the screen (THM-02, THM-03). A program
//! may set any of the 256 colors, the foreground, the background and the cursor over the
//! palette's (OSC 4, 10 to 12; TERM-12), and those are drawn in their place.

use terminal_core::{Color, Style, ThemeColor, Underline};

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
    /// The cursor's color (TERM-07).
    pub cursor: Rgb,
    /// The color of the text under a block cursor.
    pub cursor_text: Rgb,
    /// The background of the cells selected (TERM-17). Their text keeps its color.
    pub selection: Rgb,
    /// The background of the cells find's matches hold (TERM-18).
    pub find: Rgb,
    /// The background of the cells the current match holds.
    pub find_current: Rgb,
    /// The color of the text the matches hold, on either background.
    pub find_text: Rgb,
}

/// The levels of each component in xterm's 6×6×6 cube, colors 16 to 231.
const LEVELS: [u8; 6] = [0, 95, 135, 175, 215, 255];

impl Palette {
    /// Color `index` of the 256.
    fn indexed(&self, index: u8) -> Rgb {
        match index {
            0..=15 => self.ansi[usize::from(index)],
            16..=231 => {
                let cube = usize::from(index - 16);
                Rgb::new(LEVELS[cube / 36], LEVELS[cube / 6 % 6], LEVELS[cube % 6])
            }
            // The 24 grays, from 8 to 238.
            _ => {
                let gray = 8 + 10 * (index - 232);
                Rgb::new(gray, gray, gray)
            }
        }
    }

    /// The theme's `color`, as a program that asks for it is told (TERM-12).
    pub(crate) fn theme(&self, color: ThemeColor) -> Rgb {
        match color {
            ThemeColor::Palette(index) => self.indexed(index),
            ThemeColor::Foreground => self.foreground,
            ThemeColor::Background => self.background,
            ThemeColor::Cursor => self.cursor,
        }
    }

    /// The colors the palette draws with where no program set any.
    #[cfg(test)]
    pub(crate) fn colors(&self) -> Colors {
        Colors::new(self, &terminal_core::Colors::default())
    }
}

/// The colors a frame is drawn with: the palette's, with those a program set in their place.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Colors {
    /// The text's color where a program set none.
    pub(crate) foreground: Rgb,
    /// The background's color where a program set none, and around the grid.
    pub(crate) background: Rgb,
    pub(crate) cursor: Rgb,
    /// The theme's: no program sets it.
    pub(crate) cursor_text: Rgb,
    /// The theme's: no program sets it.
    pub(crate) selection: Rgb,
    /// The theme's, as are the next two: no program sets them.
    pub(crate) find: Rgb,
    pub(crate) find_current: Rgb,
    pub(crate) find_text: Rgb,
    /// The 256 colors SGR names by number.
    indexed: [Rgb; 256],
}

impl Colors {
    /// `palette`, with what a program `set` over it.
    pub(crate) fn new(palette: &Palette, set: &terminal_core::Colors) -> Self {
        let color = |color| {
            set.get(color).map_or_else(
                || palette.theme(color),
                |[red, green, blue]| Rgb::new(red, green, blue),
            )
        };
        Self {
            foreground: color(ThemeColor::Foreground),
            background: color(ThemeColor::Background),
            cursor: color(ThemeColor::Cursor),
            cursor_text: palette.cursor_text,
            selection: palette.selection,
            find: palette.find,
            find_current: palette.find_current,
            find_text: palette.find_text,
            indexed: std::array::from_fn(|index| color(ThemeColor::Palette(index as u8))),
        }
    }

    /// What `color` looks like, with `default` for the default color.
    pub(crate) fn rgb(&self, color: Color, default: Rgb) -> Rgb {
        match color {
            Color::Default => default,
            Color::Palette(index) => self.indexed[usize::from(index)],
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

    /// How `ink`, a cell's text, is drawn under a block cursor: in the cursor's text color, its
    /// underline too unless the underline has a color of its own.
    pub(crate) fn under_cursor(&self, ink: Ink) -> Ink {
        Ink {
            color: self.cursor_text,
            underline_color: if ink.underline_color == ink.color {
                self.cursor_text
            } else {
                ink.underline_color
            },
            ..ink
        }
    }

    /// How a cell that looks like `look` is drawn when a match holds it, the current one when
    /// `current`: on the match's background, its text in the matches' color, its underline too
    /// unless the underline has a color of its own. Its attributes stay.
    pub(crate) fn found(&self, look: Look, current: bool) -> Look {
        let ink = look.ink;
        Look {
            ink: Ink {
                color: self.find_text,
                underline_color: if ink.underline_color == ink.color {
                    self.find_text
                } else {
                    ink.underline_color
                },
                ..ink
            },
            background: if current {
                self.find_current
            } else {
                self.find
            },
            hidden: look.hidden,
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
