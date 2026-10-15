//! What is drawn as shapes instead of from the font: box drawing, the block elements and
//! powerline's separators (FONT-10), and underlines and crossing out (TERM-04, TERM-05).
//!
//! A font draws box drawing to its own grid, which is rarely the cell's, so its lines leave
//! gaps or overlap. Here each character is rectangles and polygons in device pixels, inside its
//! cell, and any two characters meet: every line across a cell has the same centre, and the
//! same thickness for its weight. DESIGN.md §3 has the rules.

use std::f64::consts::PI;

/// One piece of a shape, in device pixels from its cell's top left corner. Every piece is
/// inside its cell.
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum Primitive {
    /// A rectangle of whole pixels, filled at `alpha`, from 0 to 1.
    Rect {
        x: u32,
        y: u32,
        width: u32,
        height: u32,
        alpha: f64,
    },
    /// A polygon, filled. Its corners may fall between pixels, where it is antialiased.
    Polygon(Vec<(f64, f64)>),
}

/// What the cursor is drawn as (TERM-07, ANIM-07).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum CursorLook {
    Block,
    Underline,
    Bar,
    /// A hollow block, for a window without the focus.
    Outline,
}

/// How heavy a line is.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Weight {
    None,
    Light,
    Heavy,
    Double,
}

/// The arms of box drawing's lines, corners and crossings, U+2500 to U+257F: up, right, down
/// and left, `-` for none, `l` light, `h` heavy and `d` double. An empty entry is a dashed
/// line, a rounded corner or a diagonal. Each comment is a row of Unicode's chart.
#[rustfmt::skip]
const ARMS: [&str; 128] = [
    // ─ ━ │ ┃ ┄ ┅ ┆ ┇ ┈ ┉ ┊ ┋ ┌ ┍ ┎ ┏
    "-l-l", "-h-h", "l-l-", "h-h-", "", "", "", "",
    "", "", "", "", "-ll-", "-hl-", "-lh-", "-hh-",
    // ┐ ┑ ┒ ┓ └ ┕ ┖ ┗ ┘ ┙ ┚ ┛ ├ ┝ ┞ ┟
    "--ll", "--lh", "--hl", "--hh", "ll--", "lh--", "hl--", "hh--",
    "l--l", "l--h", "h--l", "h--h", "lll-", "lhl-", "hll-", "llh-",
    // ┠ ┡ ┢ ┣ ┤ ┥ ┦ ┧ ┨ ┩ ┪ ┫ ┬ ┭ ┮ ┯
    "hlh-", "hhl-", "lhh-", "hhh-", "l-ll", "l-lh", "h-ll", "l-hl",
    "h-hl", "h-lh", "l-hh", "h-hh", "-lll", "-llh", "-hll", "-hlh",
    // ┰ ┱ ┲ ┳ ┴ ┵ ┶ ┷ ┸ ┹ ┺ ┻ ┼ ┽ ┾ ┿
    "-lhl", "-lhh", "-hhl", "-hhh", "ll-l", "ll-h", "lh-l", "lh-h",
    "hl-l", "hl-h", "hh-l", "hh-h", "llll", "lllh", "lhll", "lhlh",
    // ╀ ╁ ╂ ╃ ╄ ╅ ╆ ╇ ╈ ╉ ╊ ╋ ╌ ╍ ╎ ╏
    "hlll", "llhl", "hlhl", "hllh", "hhll", "llhh", "lhhl", "hhlh",
    "lhhh", "hlhh", "hhhl", "hhhh", "", "", "", "",
    // ═ ║ ╒ ╓ ╔ ╕ ╖ ╗ ╘ ╙ ╚ ╛ ╜ ╝ ╞ ╟
    "-d-d", "d-d-", "-dl-", "-ld-", "-dd-", "--ld", "--dl", "--dd",
    "ld--", "dl--", "dd--", "l--d", "d--l", "d--d", "ldl-", "dld-",
    // ╠ ╡ ╢ ╣ ╤ ╥ ╦ ╧ ╨ ╩ ╪ ╫ ╬ ╭ ╮ ╯
    "ddd-", "l-ld", "d-dl", "d-dd", "-dld", "-ldl", "-ddd", "ld-d",
    "dl-l", "dd-d", "ldld", "dldl", "dddd", "", "", "",
    // ╰ ╱ ╲ ╳ ╴ ╵ ╶ ╷ ╸ ╹ ╺ ╻ ╼ ╽ ╾ ╿
    "", "", "", "", "---l", "l---", "-l--", "--l-",
    "---h", "h---", "-h--", "--h-", "-h-l", "l-h-", "-l-h", "h-l-",
];

const UP: usize = 0;
const RIGHT: usize = 1;
const DOWN: usize = 2;
const LEFT: usize = 3;

/// Whether `char` is drawn as a shape rather than from the font.
pub(crate) fn is_shape(char: char) -> bool {
    matches!(char, '\u{2500}'..='\u{259F}' | '\u{E0B0}'..='\u{E0BF}')
}

/// The arms of a box-drawing character that is made of them, up, right, down and left.
pub(crate) fn arms(char: char) -> Option<[Weight; 4]> {
    let index = (char as u32).checked_sub(0x2500)?;
    let code = ARMS.get(usize::try_from(index).ok()?)?;
    let mut arms = [Weight::None; 4];
    for (arm, letter) in arms.iter_mut().zip(code.bytes()) {
        *arm = match letter {
            b'l' => Weight::Light,
            b'h' => Weight::Heavy,
            b'd' => Weight::Double,
            _ => Weight::None,
        };
    }
    (code.len() == 4).then_some(arms)
}

/// The shape of `char` in a cell `width` by `height` pixels, whose light lines are `light`
/// pixels thick and heavy ones `heavy`. None for a character the font draws.
pub(crate) fn shape(
    char: char,
    width: u32,
    height: u32,
    light: u32,
    heavy: u32,
) -> Option<Vec<Primitive>> {
    let cell = Cell::new(width, height, light, heavy);
    let primitives = match char {
        '\u{2504}'..='\u{250B}' | '\u{254C}'..='\u{254F}' => cell.dashes(char),
        '\u{256D}'..='\u{2570}' => cell.arc(char),
        '\u{2571}'..='\u{2573}' => cell.diagonals(char),
        '\u{2500}'..='\u{257F}' => cell.arms(arms(char)?),
        '\u{2580}'..='\u{259F}' => cell.block(char),
        '\u{E0B0}'..='\u{E0BF}' => cell.powerline(char),
        _ => return None,
    };
    Some(primitives)
}

/// The underlines of a cell, single, double, curly, dotted and dashed, and its crossing out,
/// for a cell `width` by `height` pixels with its baseline `baseline` pixels from the top, a
/// font `font_size` pixels high and lines `light` pixels thick. Each underline runs on unbroken
/// into the next cell's.
pub(crate) fn decorations(
    width: u32,
    height: u32,
    baseline: u32,
    font_size: f64,
    light: u32,
) -> ([Vec<Primitive>; 5], Vec<Primitive>) {
    let cell = Cell::new(width, height, light, light);
    let (w, h, t) = (cell.width, cell.height, cell.light);
    // A tenth of the font below the baseline, as fonts commonly put it.
    let below = ((font_size * 0.1).round() as i32).max(1);
    let top = (baseline as i32 + below).min(h - t).max(0);
    let line = |top: i32| cell.rect(0, w, top, top + t);

    let single = line(top).into_iter().collect();
    let second = (top + 2 * t).min(h - t);
    let first = (second - 2 * t).max(0);
    let double = line(first).into_iter().chain(line(second)).collect();
    let curly = cell.wave((top).min(h - 3 * t).max(0)).into_iter().collect();
    // Square dots with at least a dot's room between them, a whole number of them in each cell.
    let dots = (w / (2 * t)).max(1);
    let dotted = (0..dots)
        .filter_map(|dot| {
            let x = (f64::from(dot * w) / f64::from(dots)).round() as i32;
            cell.rect(x, x + t, top, top + t)
        })
        .collect();
    // One dash a cell, as long as the gap after it.
    let quarter = |quarters: i32| (f64::from(w * quarters) / 4.0).round() as i32;
    let dashed = cell
        .rect(quarter(1), quarter(3), top, top + t)
        .into_iter()
        .collect();

    // Through the middle of a lowercase letter: three tenths of the font above the baseline.
    let above = (font_size * 0.3).round() as i32;
    let strike = (baseline as i32 - above - t / 2).min(h - t).max(0);
    let strikethrough = line(strike).into_iter().collect();

    ([single, double, curly, dotted, dashed], strikethrough)
}

/// The cursor drawn as `look` over `cells` cells `width` by `height` pixels, from the top left
/// of the first: a whole block, a bar at the left or an underline at the bottom two light lines
/// thick, or an outline one light line thick, in four pieces that do not overlap.
pub(crate) fn cursor(
    look: CursorLook,
    cells: u32,
    width: u32,
    height: u32,
    light: u32,
) -> Vec<Primitive> {
    let cell = Cell::new(width.saturating_mul(cells), height, light, light);
    let (w, h, t) = (cell.width, cell.height, cell.light);
    let pieces = match look {
        CursorLook::Block => vec![cell.rect(0, w, 0, h)],
        CursorLook::Underline => vec![cell.rect(0, w, h - 2 * t, h)],
        CursorLook::Bar => vec![cell.rect(0, 2 * t, 0, h)],
        CursorLook::Outline => vec![
            cell.rect(0, w, 0, t),
            cell.rect(0, t, t, h - t),
            cell.rect(w - t, w, t, h - t),
            cell.rect(0, w, h - t, h),
        ],
    };
    pieces.into_iter().flatten().collect()
}

/// A cell, and how thick its lines are, in pixels.
struct Cell {
    width: i32,
    height: i32,
    light: i32,
    heavy: i32,
}

/// Where lines lie across one axis of a cell, as ranges of pixels from the start of the axis:
/// a light line, a heavy one around it, and the two rails of a double line, with a light line's
/// room between them. Every character has the same bands, so their lines meet.
#[derive(Clone, Copy)]
struct Bands {
    light: (i32, i32),
    heavy: (i32, i32),
    low: (i32, i32),
    high: (i32, i32),
}

impl Bands {
    fn new(length: i32, light: i32, heavy: i32) -> Self {
        let start = (length - light).div_euclid(2);
        let more = (heavy - light) / 2;
        Self {
            light: (start, start + light),
            heavy: (start - more, start + light + more),
            low: (start - light, start),
            high: (start + light, start + 2 * light),
        }
    }

    /// Where a single line of `weight` lies.
    fn of(&self, weight: Weight) -> (i32, i32) {
        match weight {
            Weight::Heavy => self.heavy,
            _ => self.light,
        }
    }
}

impl Cell {
    fn new(width: u32, height: u32, light: u32, heavy: u32) -> Self {
        let int = |value: u32| i32::try_from(value).unwrap_or(i32::MAX);
        Self {
            width: int(width),
            height: int(height),
            light: int(light),
            heavy: int(heavy),
        }
    }

    /// Where vertical lines lie across the cell, and horizontal ones down it.
    fn bands(&self) -> (Bands, Bands) {
        (
            Bands::new(self.width, self.light, self.heavy),
            Bands::new(self.height, self.light, self.heavy),
        )
    }

    /// The part of the rectangle from `x0` to `x1` and `y0` to `y1` that is in the cell.
    fn rect(&self, x0: i32, x1: i32, y0: i32, y1: i32) -> Option<Primitive> {
        self.shaded(x0, x1, y0, y1, 1.0)
    }

    /// [`Cell::rect`], filled at `alpha`.
    fn shaded(&self, x0: i32, x1: i32, y0: i32, y1: i32, alpha: f64) -> Option<Primitive> {
        let (x0, x1) = (x0.clamp(0, self.width), x1.clamp(0, self.width));
        let (y0, y1) = (y0.clamp(0, self.height), y1.clamp(0, self.height));
        (x0 < x1 && y0 < y1).then(|| Primitive::Rect {
            x: x0 as u32,
            y: y0 as u32,
            width: (x1 - x0) as u32,
            height: (y1 - y0) as u32,
            alpha,
        })
    }

    /// The part of the polygon through `points` that is in the cell, with its corners rounded
    /// to a hundredth of a pixel.
    fn polygon(&self, points: impl IntoIterator<Item = (f64, f64)>) -> Option<Primitive> {
        let (w, h) = (f64::from(self.width), f64::from(self.height));
        let mut points: Vec<(f64, f64)> = points.into_iter().collect();
        points = clip(
            points,
            |p| p.0 >= 0.0,
            |a, b| (0.0, a.1 + (b.1 - a.1) * (0.0 - a.0) / (b.0 - a.0)),
        );
        points = clip(
            points,
            |p| p.0 <= w,
            |a, b| (w, a.1 + (b.1 - a.1) * (w - a.0) / (b.0 - a.0)),
        );
        points = clip(
            points,
            |p| p.1 >= 0.0,
            |a, b| (a.0 + (b.0 - a.0) * (0.0 - a.1) / (b.1 - a.1), 0.0),
        );
        points = clip(
            points,
            |p| p.1 <= h,
            |a, b| (a.0 + (b.0 - a.0) * (h - a.1) / (b.1 - a.1), h),
        );
        let round =
            |value: f64, most: f64| ((value * 100.0).round() / 100.0).clamp(0.0, most) + 0.0;
        points = points
            .into_iter()
            .map(|(x, y)| (round(x, w), round(y, h)))
            .collect();
        points.dedup();
        (points.len() >= 3).then_some(Primitive::Polygon(points))
    }

    /// A box-drawing character made of `arms`: up, right, down and left. Each arm runs in from
    /// its edge as far as the lines across it say.
    fn arms(&self, arms: [Weight; 4]) -> Vec<Primitive> {
        let (columns, rows) = self.bands();
        let mut primitives = Vec::new();
        for arm in [UP, RIGHT, DOWN, LEFT] {
            let weight = arms[arm];
            if weight == Weight::None {
                continue;
            }
            let horizontal = arm == RIGHT || arm == LEFT;
            // `along` is where the lines across the arm lie on its way in, and `across` where the
            // arm itself lies.
            let (along, across, length) = if horizontal {
                (columns, rows, self.width)
            } else {
                (rows, columns, self.height)
            };
            // The arms across this one: the one before it on `across` first, which is up for a
            // horizontal arm and left for a vertical one.
            let (before, after) = if horizontal {
                (arms[UP], arms[DOWN])
            } else {
                (arms[LEFT], arms[RIGHT])
            };
            let opposite = arms[(arm + 2) % 4];
            // An arm comes in from the far end of its axis (right, down) or from its start.
            let from_end = arm == RIGHT || arm == DOWN;
            // How far into a band the arm goes to reach its far side.
            let far = |(start, end): (i32, i32)| if from_end { start } else { end };
            // A double line across: the rail on this arm's side, and the other.
            let (near_rail, far_rail) = if from_end {
                (along.high, along.low)
            } else {
                (along.low, along.high)
            };
            let crossing = [before, after];
            let doubled = crossing.contains(&Weight::Double);
            let widest = if crossing.contains(&Weight::Heavy) {
                along.heavy
            } else {
                along.light
            };
            let mut push = |reach: i32, (from, to): (i32, i32)| {
                let (start, end) = if from_end {
                    (reach, length)
                } else {
                    (0, reach)
                };
                let rect = if horizontal {
                    self.rect(start, end, from, to)
                } else {
                    self.rect(from, to, start, end)
                };
                primitives.extend(rect);
            };

            if weight != Weight::Double {
                // Square corners: on to the far side of the widest line across. A double line
                // that runs straight on is met at its near rail, unless this line crosses it.
                let reach = if !doubled {
                    far(widest)
                } else if before == Weight::Double
                    && after == Weight::Double
                    && opposite == Weight::None
                {
                    far(near_rail)
                } else {
                    far(far_rail)
                };
                push(reach, across.of(weight));
                continue;
            }
            // Each rail of a double line turns at the quadrant it borders.
            let single = crossing.iter().any(|weight| *weight != Weight::None) && !doubled;
            for (rail, quadrant) in [(across.low, before), (across.high, after)] {
                let reach = if single {
                    // Across a single line, as a single arm would.
                    far(widest)
                } else if quadrant == Weight::Double {
                    // An inner corner, at the rail of the double line beside it.
                    far(near_rail)
                } else if opposite != Weight::None {
                    // On through the centre, to the arm opposite.
                    far(along.light)
                } else {
                    // An outer corner, round the far rail of the line across.
                    far(far_rail)
                };
                push(reach, rail);
            }
        }
        primitives
    }

    /// A dashed line: ┄ to ┋ and ╌ to ╏. Each dash is half of its share of the cell, with half
    /// the gap at each end, so dashes are evenly spaced from cell to cell.
    fn dashes(&self, char: char) -> Vec<Primitive> {
        let (count, first) = match char {
            '\u{2504}'..='\u{2507}' => (3, 0x2504),
            '\u{2508}'..='\u{250B}' => (4, 0x2508),
            _ => (2, 0x254C),
        };
        let index = char as u32 - first;
        let weight = if index % 2 == 1 {
            Weight::Heavy
        } else {
            Weight::Light
        };
        let vertical = index >= 2;
        let (columns, rows) = self.bands();
        let (length, (from, to)) = if vertical {
            (self.height, columns.of(weight))
        } else {
            (self.width, rows.of(weight))
        };
        let step = f64::from(length) / f64::from(count);
        (0..count)
            .filter_map(|dash| {
                let start = (f64::from(dash) * step + step / 4.0).round() as i32;
                let end = ((f64::from(dash + 1) * step - step / 4.0).round() as i32).max(start + 1);
                if vertical {
                    self.rect(from, to, start, end)
                } else {
                    self.rect(start, end, from, to)
                }
            })
            .collect()
    }

    /// A rounded corner: ╭ ╮ ╯ ╰. A light line along a quarter ellipse round the cell's corner,
    /// whose ends lie on the light lines of its neighbours.
    fn arc(&self, char: char) -> Vec<Primitive> {
        let (columns, rows) = self.bands();
        let t = f64::from(self.light);
        let (w, h) = (f64::from(self.width), f64::from(self.height));
        // The middles of the vertical and the horizontal light lines.
        let x = f64::from(columns.light.0) + t / 2.0;
        let y = f64::from(rows.light.0) + t / 2.0;
        // The corner it turns round, and where it starts, as the angle down from the right.
        let (centre, from) = match char {
            '╭' => ((w, h), PI),
            '╮' => ((0.0, h), 1.5 * PI),
            '╯' => ((0.0, 0.0), 0.0),
            _ => ((w, 0.0), 0.5 * PI),
        };
        let radii = ((centre.0 - x).abs(), (centre.1 - y).abs());
        self.ring(centre, radii, t, from, from + PI / 2.0)
            .into_iter()
            .collect()
    }

    /// A diagonal: ╱ ╲ ╳. Light lines from corner to corner.
    fn diagonals(&self, char: char) -> Vec<Primitive> {
        let (w, h) = (f64::from(self.width), f64::from(self.height));
        let t = f64::from(self.light);
        let rising = || self.line((w, 0.0), (0.0, h), t);
        let falling = || self.line((0.0, 0.0), (w, h), t);
        match char {
            '╱' => rising().into_iter().collect(),
            '╲' => falling().into_iter().collect(),
            _ => rising().into_iter().chain(falling()).collect(),
        }
    }

    /// A block element: ▀ to ▟. Eighths are rounded from both edges of the cell, so blocks that
    /// make a cell between them tile it.
    fn block(&self, char: char) -> Vec<Primitive> {
        let (w, h) = (self.width, self.height);
        let eighths = |length: i32, count: i32| (f64::from(length * count) / 8.0).round() as i32;
        let x = |count| eighths(w, count);
        let y = |count| eighths(h, count);
        let code = char as i32;
        let rect = match char {
            '▀' => self.rect(0, w, 0, y(4)),
            '\u{2581}'..='\u{2588}' => self.rect(0, w, y(8 - (code - 0x2580)), h),
            '\u{2589}'..='\u{258F}' => self.rect(0, x(0x2590 - code), 0, h),
            '▐' => self.rect(x(4), w, 0, h),
            '░' => self.shaded(0, w, 0, h, 0.25),
            '▒' => self.shaded(0, w, 0, h, 0.5),
            '▓' => self.shaded(0, w, 0, h, 0.75),
            '▔' => self.rect(0, w, 0, y(1)),
            '▕' => self.rect(x(7), w, 0, h),
            _ => {
                // The quadrants: upper left, upper right, lower left and lower right.
                let quadrants: [bool; 4] = match char {
                    '▖' => [false, false, true, false],
                    '▗' => [false, false, false, true],
                    '▘' => [true, false, false, false],
                    '▙' => [true, false, true, true],
                    '▚' => [true, false, false, true],
                    '▛' => [true, true, true, false],
                    '▜' => [true, true, false, true],
                    '▝' => [false, true, false, false],
                    '▞' => [false, true, true, false],
                    _ => [false, true, true, true],
                };
                let (middle, centre) = (x(4), y(4));
                let places = [
                    (0, middle, 0, centre),
                    (middle, w, 0, centre),
                    (0, middle, centre, h),
                    (middle, w, centre, h),
                ];
                return quadrants
                    .iter()
                    .zip(places)
                    .filter(|(on, _)| **on)
                    .filter_map(|(_, (x0, x1, y0, y1))| self.rect(x0, x1, y0, y1))
                    .collect();
            }
        };
        rect.into_iter().collect()
    }

    /// A powerline separator, U+E0B0 to U+E0BF: through the cell's corners and the middles of
    /// its edges, so a prompt's segments meet the cells beside them.
    fn powerline(&self, char: char) -> Vec<Primitive> {
        let (w, h) = (f64::from(self.width), f64::from(self.height));
        let t = f64::from(self.light);
        let middle = h / 2.0;
        let polygon = |points: &[(f64, f64)]| self.polygon(points.iter().copied());
        let pieces = match char {
            // Solid arrows, and their outlines.
            '\u{E0B0}' => vec![polygon(&[(0.0, 0.0), (w, middle), (0.0, h)])],
            '\u{E0B1}' => vec![
                self.line((0.0, 0.0), (w, middle), t),
                self.line((w, middle), (0.0, h), t),
            ],
            '\u{E0B2}' => vec![polygon(&[(w, 0.0), (0.0, middle), (w, h)])],
            '\u{E0B3}' => vec![
                self.line((w, 0.0), (0.0, middle), t),
                self.line((0.0, middle), (w, h), t),
            ],
            // Half ellipses, and their outlines.
            '\u{E0B4}' => vec![self.half((0.0, middle), (w, middle), -PI / 2.0)],
            '\u{E0B5}' => vec![self.ring(
                (0.0, middle),
                (w - t / 2.0, middle - t / 2.0),
                t,
                -PI / 2.0,
                PI / 2.0,
            )],
            '\u{E0B6}' => vec![self.half((w, middle), (w, middle), PI / 2.0)],
            '\u{E0B7}' => vec![self.ring(
                (w, middle),
                (w - t / 2.0, middle - t / 2.0),
                t,
                PI / 2.0,
                1.5 * PI,
            )],
            // Triangles in a corner of the cell, and the diagonals between them.
            '\u{E0B8}' => vec![polygon(&[(0.0, 0.0), (0.0, h), (w, h)])],
            '\u{E0BA}' => vec![polygon(&[(w, 0.0), (w, h), (0.0, h)])],
            '\u{E0BC}' => vec![polygon(&[(0.0, 0.0), (w, 0.0), (0.0, h)])],
            '\u{E0BE}' => vec![polygon(&[(0.0, 0.0), (w, 0.0), (w, h)])],
            '\u{E0B9}' | '\u{E0BF}' => vec![self.line((0.0, 0.0), (w, h), t)],
            _ => vec![self.line((w, 0.0), (0.0, h), t)],
        };
        pieces.into_iter().flatten().collect()
    }

    /// A line `thickness` thick from `from` to `to`, run on past both ends so that the cell's
    /// edges cut it square.
    fn line(&self, from: (f64, f64), to: (f64, f64), thickness: f64) -> Option<Primitive> {
        let (dx, dy) = (to.0 - from.0, to.1 - from.1);
        let length = dx.hypot(dy);
        let (along_x, along_y) = (dx / length * thickness, dy / length * thickness);
        let (side_x, side_y) = (-along_y / 2.0, along_x / 2.0);
        let (from, to) = (
            (from.0 - along_x, from.1 - along_y),
            (to.0 + along_x, to.1 + along_y),
        );
        self.polygon([
            (from.0 + side_x, from.1 + side_y),
            (to.0 + side_x, to.1 + side_y),
            (to.0 - side_x, to.1 - side_y),
            (from.0 - side_x, from.1 - side_y),
        ])
    }

    /// Half an ellipse round `centre`, of `radii`, from the angle `from` down from the right,
    /// filled.
    fn half(&self, centre: (f64, f64), radii: (f64, f64), from: f64) -> Option<Primitive> {
        self.polygon(arc_points(centre, radii, from, from + PI))
    }

    /// A line `thickness` thick along the ellipse round `centre` of `radii`, from the angle
    /// `from` to `to`.
    fn ring(
        &self,
        centre: (f64, f64),
        (rx, ry): (f64, f64),
        thickness: f64,
        from: f64,
        to: f64,
    ) -> Option<Primitive> {
        let half = thickness / 2.0;
        let outer = arc_points(centre, (rx + half, ry + half), from, to);
        let inner = arc_points(
            centre,
            ((rx - half).max(0.0), (ry - half).max(0.0)),
            from,
            to,
        );
        self.polygon(outer.chain(inner.collect::<Vec<_>>().into_iter().rev()))
    }

    /// A wave one cell long and a light line thick, three lines high from `top`: it starts and
    /// ends at its top, so it runs on into the next cell's.
    fn wave(&self, top: i32) -> Option<Primitive> {
        let (w, t) = (f64::from(self.width), f64::from(self.light));
        let steps = self.width.max(8);
        let middle = |x: f64| f64::from(top) + t / 2.0 + t * (1.0 - (2.0 * PI * x / w).cos());
        let xs = (0..=steps).map(move |step| w * f64::from(step) / f64::from(steps));
        let upper: Vec<_> = xs.clone().map(|x| (x, middle(x) - t / 2.0)).collect();
        let lower: Vec<_> = xs.rev().map(|x| (x, middle(x) + t / 2.0)).collect();
        self.polygon(upper.into_iter().chain(lower))
    }
}

/// Points along the ellipse round `centre` of `radii`, from the angle `from` to `to`, down from
/// the right, both ends included.
fn arc_points(
    centre: (f64, f64),
    (rx, ry): (f64, f64),
    from: f64,
    to: f64,
) -> impl Iterator<Item = (f64, f64)> {
    const STEPS: u32 = 24;
    (0..=STEPS).map(move |step| {
        let angle = from + (to - from) * f64::from(step) / f64::from(STEPS);
        (centre.0 + rx * angle.cos(), centre.1 + ry * angle.sin())
    })
}

/// The part of the polygon through `points` on the inside of one edge, which `inside` tells,
/// with `cut` giving where a side crosses the edge: one step of Sutherland and Hodgman's.
fn clip(
    points: Vec<(f64, f64)>,
    inside: impl Fn((f64, f64)) -> bool,
    cut: impl Fn((f64, f64), (f64, f64)) -> (f64, f64),
) -> Vec<(f64, f64)> {
    let mut kept = Vec::with_capacity(points.len() + 4);
    for (index, &point) in points.iter().enumerate() {
        let previous = points[(index + points.len() - 1) % points.len()];
        match (inside(previous), inside(point)) {
            (true, true) => kept.push(point),
            (true, false) => kept.push(cut(previous, point)),
            (false, true) => {
                kept.push(cut(previous, point));
                kept.push(point);
            }
            (false, false) => {}
        }
    }
    kept
}
