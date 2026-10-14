//! What a cell looks like: its colors and attributes, and what SGR does to them (TERM-03 to
//! TERM-05).
//!
//! xterm is the model (TERM-01), as its `CASE_SGR` and `parse_extended_colors` read SGR, but
//! for the underline's styles and color, which xterm does not have. Those are kitty's, as VTE,
//! WezTerm and foot have them. DESIGN.md §3 says where else it differs.
//!
//! A color is kept as the program named it, not as it looks: colors 0 to 15 of the palette are
//! the theme's, so a theme change recolors what is already on the screen. What each color looks
//! like is the Renderer's (#22).

/// A color, as a program names it.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Color {
    /// The theme's foreground or background. For the underline, the text's color.
    #[default]
    Default,
    /// One of the 256 colors of the palette: 0 to 15 are the theme's ANSI colors (THM-04), and
    /// 16 to 255 xterm's 6×6×6 cube and its 24 grays.
    Palette(u8),
    /// A 24-bit color: red, green and blue.
    Rgb(u8, u8, u8),
}

/// How text is underlined.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Underline {
    #[default]
    None,
    Single,
    Double,
    Curly,
    Dotted,
    Dashed,
}

impl Underline {
    /// The style `4:number` names, as kitty numbers them.
    fn numbered(number: u16) -> Option<Self> {
        Some(match number {
            0 => Self::None,
            1 => Self::Single,
            2 => Self::Double,
            3 => Self::Curly,
            4 => Self::Dotted,
            5 => Self::Dashed,
            _ => return None,
        })
    }
}

/// The colors and attributes a cell is drawn in, as SGR set them when it was written.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Style {
    pub foreground: Color,
    pub background: Color,
    /// The underline's color. The default is the text's.
    pub underline_color: Color,
    pub underline: Underline,
    pub bold: bool,
    /// Faint: drawn with less contrast.
    pub dim: bool,
    pub italic: bool,
    /// The foreground and the background change places.
    pub inverse: bool,
    /// Its cells are taken, but nothing is drawn in them.
    pub hidden: bool,
    /// Crossed out.
    pub strikethrough: bool,
}

impl Default for Style {
    fn default() -> Self {
        Self::PLAIN
    }
}

impl Style {
    /// The style of text no SGR has touched. A constant, for the blank cell.
    pub(crate) const PLAIN: Self = Self {
        foreground: Color::Default,
        background: Color::Default,
        underline_color: Color::Default,
        underline: Underline::None,
        bold: false,
        dim: false,
        italic: false,
        inverse: false,
        hidden: false,
        strikethrough: false,
    };

    /// The style's colors, without its attributes: what xterm keeps of it in a cell it blanks,
    /// and after DECALN.
    pub(crate) fn colors(self) -> Self {
        Self {
            foreground: self.foreground,
            background: self.background,
            underline_color: self.underline_color,
            ..Self::PLAIN
        }
    }

    /// SGR: changes the style as each of `params` says, in order. A color takes the parameters
    /// after it that it needs. vte gives `CSI m` one parameter, an empty one, which is 0.
    pub(crate) fn select<'a>(&mut self, params: impl IntoIterator<Item = &'a [u16]>) {
        let mut params = params.into_iter();
        while let Some(param) = params.next() {
            let (code, subs) = param
                .split_first()
                .map_or((0, &[][..]), |(&code, subs)| (code, subs));
            // Only these take sub-parameters. xterm ignores any other parameter that has some.
            if !subs.is_empty() && !matches!(code, 4 | 38 | 48 | 58) {
                continue;
            }
            match code {
                0 => *self = Self::PLAIN,
                1 => self.bold = true,
                2 => self.dim = true,
                3 => self.italic = true,
                // A style kitty does not number changes nothing.
                4 => match subs.first() {
                    None => self.underline = Underline::Single,
                    Some(&number) => {
                        if let Some(underline) = Underline::numbered(number) {
                            self.underline = underline;
                        }
                    }
                },
                7 => self.inverse = true,
                8 => self.hidden = true,
                9 => self.strikethrough = true,
                21 => self.underline = Underline::Double,
                22 => {
                    self.bold = false;
                    self.dim = false;
                }
                23 => self.italic = false,
                24 => self.underline = Underline::None,
                27 => self.inverse = false,
                28 => self.hidden = false,
                29 => self.strikethrough = false,
                30..=37 => self.foreground = Color::Palette((code - 30) as u8),
                38 => set(&mut self.foreground, extended(subs, &mut params)),
                39 => self.foreground = Color::Default,
                40..=47 => self.background = Color::Palette((code - 40) as u8),
                48 => set(&mut self.background, extended(subs, &mut params)),
                49 => self.background = Color::Default,
                58 => set(&mut self.underline_color, extended(subs, &mut params)),
                59 => self.underline_color = Color::Default,
                90..=97 => self.foreground = Color::Palette((code - 90 + 8) as u8),
                100..=107 => self.background = Color::Palette((code - 100 + 8) as u8),
                // Blink among them: output never animates (ANIM-05, ANIM-07).
                _ => {}
            }
        }
    }
}

/// Sets `color` to `to`, if it is a color.
fn set(color: &mut Color, to: Option<Color>) {
    if let Some(to) = to {
        *color = to;
    }
}

/// The color 38, 48 or 58 names, in one of xterm's three ways: as its own sub-parameters `subs`
/// (`38:5:n`, `38:2:r:g:b`), as those of the parameter after it (`38;5:n`, `38;2:r:g:b`), or as
/// the parameters after it (`38;5;n`, `38;2;r;g;b`). The parameters it takes are used up,
/// whether they name a color or not.
fn extended<'a>(subs: &[u16], params: &mut impl Iterator<Item = &'a [u16]>) -> Option<Color> {
    let subs = match subs {
        [] => match params.next()? {
            &[kind] => return separate(kind, params),
            next => next,
        },
        subs => subs,
    };
    match subs {
        [5, index, ..] => palette(*index),
        // With four numbers or more, the first is the color space, which is skipped.
        [2, _, red, green, blue, ..] | [2, red, green, blue] => rgb(*red, *green, *blue),
        _ => None,
    }
}

/// The color of `38;5;n` or `38;2;r;g;b`, whose `kind` came alone: its numbers are the
/// parameters after it. Any other kind takes none.
fn separate<'a>(kind: u16, params: &mut impl Iterator<Item = &'a [u16]>) -> Option<Color> {
    let mut next = || {
        params
            .next()
            .map(|param| param.first().copied().unwrap_or(0))
    };
    match kind {
        5 => palette(next()?),
        2 => {
            // All three are taken, even when the first is out of range.
            let (red, green, blue) = (next(), next(), next());
            rgb(red?, green?, blue?)
        }
        _ => None,
    }
}

/// Color `index` of the palette, if there is one.
fn palette(index: u16) -> Option<Color> {
    u8::try_from(index).ok().map(Color::Palette)
}

/// The 24-bit color of these components, if each fits in 8 bits.
fn rgb(red: u16, green: u16, blue: u16) -> Option<Color> {
    let component = |value: u16| u8::try_from(value).ok();
    Some(Color::Rgb(
        component(red)?,
        component(green)?,
        component(blue)?,
    ))
}
