//! Tests of the Renderer (HW-03, FONT-10, TERM-03 to TERM-05): what the webview measured in and
//! the layout out, the Terminal Core's screen in and frames out, and every shape drawn into
//! pixels here, to see that it stays in its cell and meets its neighbours.

use std::collections::BTreeSet;

use terminal_core::{Color, Size, Style, TerminalCore, Underline};

use super::color::Ink;
use super::shapes::{arms, decorations, is_shape, shape, Weight};
use super::*;

/// A palette whose colors are told apart at a glance.
fn palette() -> Palette {
    Palette {
        foreground: Rgb::new(200, 200, 200),
        background: Rgb::new(10, 10, 10),
        ansi: std::array::from_fn(|index| Rgb::new(index as u8, 0, 100 + index as u8)),
    }
}

/// What the webview measures for a font like JetBrains Mono at 13 pixels, at scale 1, over a
/// terminal area of `cols` by `rows` of its cells, which are 8 by 18 pixels.
fn measured(cols: u16, rows: u16) -> Measure {
    Measure {
        scale: 1.0,
        font: "Mono".to_string(),
        font_size: 13.0,
        advance: 7.8,
        ascent: 13.26,
        descent: 3.9,
        width: f64::from(cols) * 8.0,
        height: f64::from(rows) * 18.0,
    }
}

fn core(cols: u16, rows: u16, output: &str) -> TerminalCore {
    let mut core = TerminalCore::new(Size { cols, rows }, None);
    core.interpret(output.as_bytes());
    core
}

/// A Renderer for a terminal area of `cols` by `rows` cells.
fn renderer(cols: u16, rows: u16) -> Renderer {
    let mut renderer = Renderer::new(palette());
    renderer.measure(&measured(cols, rows)).unwrap();
    renderer
}

/// The first frame of `core`, which has every row.
fn first(core: &TerminalCore) -> Frame {
    let size = core.size();
    renderer(size.cols, size.rows).frame(core, "").unwrap()
}

/// A run as the tests write it: its column, its text and the cells it is drawn over, if not
/// one for each character.
type Text = (u16, String, Option<u16>);

fn texts(row: &RowFrame) -> Vec<Text> {
    row.runs
        .iter()
        .map(|run| (run.col, run.text.clone(), run.cells))
        .collect()
}

fn text(col: u16, text: &str) -> Text {
    (col, text.to_string(), None)
}

fn whole(col: u16, text: &str, cells: u16) -> Text {
    (col, text.to_string(), Some(cells))
}

/// The ink of the run at `col` of the first row.
fn ink_at(frame: &Frame, col: u16) -> Ink {
    frame.rows[0]
        .runs
        .iter()
        .find(|run| run.col == col)
        .unwrap_or_else(|| panic!("no run at {col} in {:?}", frame.rows[0]))
        .ink
}

// ---------------------------------------------------------------------------------------
// Colors
// ---------------------------------------------------------------------------------------

#[test]
fn the_first_sixteen_colors_are_the_palettes() {
    let palette = palette();
    for index in 0..16 {
        let color = palette.rgb(Color::Palette(index), Rgb::default());
        assert_eq!(color, palette.ansi[usize::from(index)]);
    }
}

#[test]
fn the_other_colors_are_xterms_cube_and_grays() {
    let palette = palette();
    let rgb = |index| palette.rgb(Color::Palette(index), Rgb::default());
    let levels = [0, 95, 135, 175, 215, 255];
    for index in 16..=231u8 {
        let cube = usize::from(index - 16);
        let expected = Rgb::new(levels[cube / 36], levels[cube / 6 % 6], levels[cube % 6]);
        assert_eq!(rgb(index), expected, "color {index}");
    }
    for index in 232..=255u8 {
        let gray = 8 + 10 * (index - 232);
        assert_eq!(rgb(index), Rgb::new(gray, gray, gray), "color {index}");
    }
    // A few xterm users know by heart.
    assert_eq!(rgb(16), Rgb::new(0, 0, 0));
    assert_eq!(rgb(196), Rgb::new(255, 0, 0));
    assert_eq!(rgb(208), Rgb::new(255, 135, 0));
    assert_eq!(rgb(231), Rgb::new(255, 255, 255));
    assert_eq!(rgb(232), Rgb::new(8, 8, 8));
    assert_eq!(rgb(255), Rgb::new(238, 238, 238));
}

#[test]
fn rgb_colors_and_defaults_are_drawn_as_they_are() {
    let palette = palette();
    assert_eq!(
        palette.rgb(Color::Rgb(1, 2, 3), Rgb::default()),
        Rgb::new(1, 2, 3)
    );
    let look = palette.look(&Style::default());
    assert_eq!(look.ink.color, palette.foreground);
    assert_eq!(look.background, palette.background);
}

#[test]
fn inverse_swaps_the_colors_once_the_defaults_are_filled_in() {
    let palette = palette();
    let look = palette.look(&Style {
        inverse: true,
        ..Style::default()
    });
    assert_eq!(look.ink.color, palette.background);
    assert_eq!(look.background, palette.foreground);

    let look = palette.look(&Style {
        foreground: Color::Palette(1),
        inverse: true,
        ..Style::default()
    });
    assert_eq!(look.ink.color, palette.background);
    assert_eq!(look.background, palette.ansi[1]);
}

#[test]
fn dim_text_is_halfway_to_its_background() {
    let palette = palette();
    let look = palette.look(&Style {
        dim: true,
        ..Style::default()
    });
    assert_eq!(look.ink.color, Rgb::new(105, 105, 105));
    // After inverse, toward the background it is drawn on.
    let look = palette.look(&Style {
        dim: true,
        inverse: true,
        background: Color::Rgb(100, 0, 0),
        ..Style::default()
    });
    assert_eq!(look.ink.color, Rgb::new(150, 100, 100));
    assert_eq!(look.background, palette.foreground);
}

#[test]
fn bold_never_brightens_a_color() {
    let palette = palette();
    for index in 0..8 {
        let look = palette.look(&Style {
            foreground: Color::Palette(index),
            bold: true,
            ..Style::default()
        });
        assert_eq!(look.ink.color, palette.ansi[usize::from(index)]);
        assert!(look.ink.bold);
    }
}

#[test]
fn an_underline_takes_the_text_color_unless_it_has_its_own() {
    let palette = palette();
    let look = palette.look(&Style {
        underline: Underline::Curly,
        inverse: true,
        ..Style::default()
    });
    assert_eq!(look.ink.underline, 3);
    assert_eq!(look.ink.underline_color, palette.background);

    let look = palette.look(&Style {
        underline: Underline::Single,
        underline_color: Color::Palette(196),
        ..Style::default()
    });
    assert_eq!(look.ink.underline_color, Rgb::new(255, 0, 0));
}

#[test]
fn hidden_text_draws_only_its_background() {
    let core = core(6, 1, "\x1b[8;4;9;41mab\x1b[0mc");
    let frame = first(&core);
    assert_eq!(texts(&frame.rows[0]), [text(2, "c")]);
    assert_eq!(
        frame.rows[0].spans,
        [Span {
            col: 0,
            count: 2,
            color: palette().ansi[1],
        }]
    );
}

// ---------------------------------------------------------------------------------------
// The layout
// ---------------------------------------------------------------------------------------

/// What the webview measures for a font like JetBrains Mono `size` CSS pixels high at `scale`,
/// over an area of 800 by 600 CSS pixels.
fn mono(size: f64, scale: f64) -> Measure {
    let font_size = size * scale;
    Measure {
        scale,
        font: "Mono".to_string(),
        font_size,
        advance: 0.6 * font_size,
        ascent: 1.02 * font_size,
        descent: 0.3 * font_size,
        width: 800.0 * scale,
        height: 600.0 * scale,
    }
}

#[test]
fn cells_are_whole_device_pixels_at_every_scale() {
    // 6 to 72 pt: as many CSS pixels on macOS, and 8 to 96 on Linux and Windows.
    for scale in [1.0, 1.25, 1.5, 1.75, 2.0, 2.5, 3.0] {
        for size in 6..=96 {
            let measure = mono(f64::from(size), scale);
            let geometry = Geometry::new(&measure).unwrap();
            let layout = geometry.layout;
            let context = format!("size {size} at {scale}: {layout:?}");

            assert_eq!(
                f64::from(layout.cell_width),
                measure.advance.round(),
                "{context}"
            );
            assert_eq!(
                f64::from(layout.cell_height),
                measure.ascent.ceil() + measure.descent.ceil(),
                "{context}"
            );
            assert_eq!(
                f64::from(geometry.baseline),
                measure.ascent.ceil(),
                "{context}"
            );
            assert_eq!(
                u32::from(layout.size.cols),
                (measure.width.round() as u32 / layout.cell_width).max(1),
                "{context}"
            );
            assert_eq!(
                u32::from(layout.size.rows),
                (measure.height.round() as u32 / layout.cell_height).max(1),
                "{context}"
            );
            let (width, height) = layout.css_cell();
            assert!(
                (width * scale - f64::from(layout.cell_width)).abs() < 1e-9,
                "{context}"
            );
            assert!(
                (height * scale - f64::from(layout.cell_height)).abs() < 1e-9,
                "{context}"
            );

            // Lines: at least a pixel, and a double one fits.
            let smaller = layout.cell_width.min(layout.cell_height);
            assert!(geometry.light >= 1, "{context}");
            assert!(
                geometry.light == 1 || 3 * geometry.light <= smaller,
                "{context}"
            );
            assert!(geometry.heavy > geometry.light, "{context}");
            assert_eq!((geometry.heavy - geometry.light) % 2, 0, "{context}");
            let decorations = geometry.underlines.iter().chain([&geometry.strikethrough]);
            for shape in decorations {
                assert!(!shape.is_empty(), "{context}");
                inside(shape, layout.cell_width, layout.cell_height, &context);
            }
        }
    }
}

#[test]
fn the_grid_is_at_least_a_cell() {
    let measure = Measure {
        width: 3.0,
        height: 3.0,
        ..measured(1, 1)
    };
    let layout = Geometry::new(&measure).unwrap().layout;
    assert_eq!(layout.size, Size { cols: 1, rows: 1 });
}

#[test]
fn measurements_that_are_no_size_are_no_layout() {
    let wrong = [f64::NAN, f64::INFINITY, f64::NEG_INFINITY, 0.0, -1.0];
    let fields: [fn(&mut Measure) -> &mut f64; 8] = [
        |measure| &mut measure.scale,
        |measure| &mut measure.font_size,
        |measure| &mut measure.advance,
        |measure| &mut measure.ascent,
        |measure| &mut measure.descent,
        |measure| &mut measure.width,
        |measure| &mut measure.height,
        |measure| &mut measure.scale,
    ];
    for (index, field) in fields.iter().enumerate() {
        for value in wrong {
            let mut measure = measured(80, 24);
            *field(&mut measure) = value;
            let descent_of_nothing = index == 4 && value == 0.0;
            assert_eq!(
                Geometry::new(&measure).is_some(),
                descent_of_nothing,
                "field {index} at {value}"
            );
        }
        // Huge numbers are taken at the most a canvas has, and never panic.
        let mut measure = measured(80, 24);
        *field(&mut measure) = 1e300;
        let geometry = Geometry::new(&measure).unwrap();
        assert!(geometry.canvas.0 <= 16384 && geometry.canvas.1 <= 16384);
        assert!(geometry.layout.size.cols >= 1 && geometry.layout.size.rows >= 1);
        assert!((0.5..=8.0).contains(&geometry.layout.scale));
    }
}

#[test]
fn a_terminal_area_that_is_hidden_keeps_the_grid() {
    let core = core(10, 2, "hi");
    let mut renderer = renderer(10, 2);
    renderer.frame(&core, "").unwrap();
    let hidden = Measure {
        width: 0.0,
        height: 0.0,
        ..measured(10, 2)
    };
    assert_eq!(renderer.measure(&hidden), None);
    assert_eq!(renderer.frame(&core, ""), None);
}

// ---------------------------------------------------------------------------------------
// What changed
// ---------------------------------------------------------------------------------------

#[test]
fn nothing_is_drawn_before_the_webview_has_measured() {
    let mut renderer = Renderer::new(palette());
    assert_eq!(renderer.frame(&core(4, 2, "hi"), ""), None);
}

#[test]
fn the_first_frame_has_the_layout_and_every_row() {
    let core = core(4, 3, "hi");
    let frame = first(&core);
    let full = frame.full.as_ref().unwrap();
    assert_eq!(full.background, palette().background);
    assert_eq!(full.geometry.layout.size, Size { cols: 4, rows: 3 });
    assert_eq!(full.geometry.canvas, (32, 54));
    let indexes: Vec<u16> = frame.rows.iter().map(|row| row.index).collect();
    assert_eq!(indexes, [0, 1, 2]);
    assert_eq!(texts(&frame.rows[0]), [text(0, "hi")]);
    assert!(frame.rows[1].runs.is_empty() && frame.rows[1].spans.is_empty());
    assert_eq!(frame.caret, terminal_core::Cursor { row: 0, col: 2 });
}

#[test]
fn a_frame_holds_only_the_rows_that_changed() {
    let mut core = core(10, 3, "one\r\ntwo\r\nthree");
    let mut renderer = renderer(10, 3);
    renderer.frame(&core, "").unwrap();
    assert_eq!(renderer.frame(&core, ""), None);

    core.interpret(b"\x1b[2;1Hdue");
    let frame = renderer.frame(&core, "").unwrap();
    assert!(frame.full.is_none());
    assert_eq!(frame.rows.len(), 1);
    assert_eq!(frame.rows[0].index, 1);
    assert_eq!(texts(&frame.rows[0]), [text(0, "due")]);

    // A scroll moves every row.
    core.interpret(b"\x1b[3;1H\n");
    let frame = renderer.frame(&core, "").unwrap();
    let indexes: Vec<u16> = frame.rows.iter().map(|row| row.index).collect();
    assert_eq!(indexes, [0, 1, 2]);
}

#[test]
fn a_caret_that_moved_alone_is_a_frame() {
    let mut core = core(10, 2, "hi");
    let mut renderer = renderer(10, 2);
    renderer.frame(&core, "").unwrap();
    core.interpret(b"\x1b[2;5H");
    let frame = renderer.frame(&core, "").unwrap();
    assert!(frame.rows.is_empty());
    assert_eq!(frame.caret, terminal_core::Cursor { row: 1, col: 4 });
}

#[test]
fn a_new_palette_layout_or_size_draws_every_row() {
    let mut core = core(10, 2, "hi");
    let mut renderer = renderer(10, 2);
    renderer.frame(&core, "").unwrap();

    // The same palette or measurements change nothing.
    renderer.set_palette(palette());
    assert_eq!(
        renderer.measure(&measured(10, 2)).unwrap().size,
        core.size()
    );
    assert_eq!(renderer.frame(&core, ""), None);

    let mut other = palette();
    other.background = Rgb::new(1, 2, 3);
    renderer.set_palette(other);
    let frame = renderer.frame(&core, "").unwrap();
    assert_eq!(frame.full.unwrap().background, Rgb::new(1, 2, 3));
    assert_eq!(frame.rows.len(), 2);

    // A wider area, before and after the core is resized to it.
    let layout = renderer.measure(&measured(12, 2)).unwrap();
    assert_eq!(layout.size, Size { cols: 12, rows: 2 });
    assert_eq!(renderer.frame(&core, "").unwrap().rows.len(), 2);
    core.resize(layout.size);
    assert_eq!(renderer.frame(&core, "").unwrap().rows.len(), 2);

    // Another font.
    let font = Measure {
        font: "Other".to_string(),
        ..measured(12, 2)
    };
    renderer.measure(&font).unwrap();
    assert!(renderer.frame(&core, "").unwrap().full.is_some());

    renderer.redraw();
    assert!(renderer.frame(&core, "").unwrap().full.is_some());
}

/// A generator of numbers that are random enough to make up output: xorshift64*.
struct Random(u64);

impl Random {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 >> 12;
        self.0 ^= self.0 << 25;
        self.0 ^= self.0 >> 27;
        self.0.wrapping_mul(0x2545_F491_4F6C_DD1D)
    }

    fn below(&mut self, count: usize) -> usize {
        (self.next() % count as u64) as usize
    }

    fn pick<'a>(&mut self, items: &[&'a str]) -> &'a str {
        items[self.below(items.len())]
    }
}

/// Some output, made up of the pieces programs write.
fn output(random: &mut Random) -> String {
    const PIECES: &[&str] = &[
        "hello",
        "a b  c",
        "\r\n",
        "\n",
        "\t",
        "界",
        "e\u{301}",
        "👍🏽",
        "🇯🇵",
        "👨\u{200D}👩",
        "─┼═╬",
        "▀░▚",
        "\u{E0B0}",
        "\x1b[K",
        "\x1b[2J",
        "\x1b[H",
        "\x1b[A",
        "\x1b[3C",
        "\x1b[2;5H",
        "\x1b[S",
        "\x1b[L",
        "\x1b[P",
        "\x1b[@",
        "\x1b[0m",
        "\x1b[1;31m",
        "\x1b[42m",
        "\x1b[7m",
        "\x1b[2m",
        "\x1b[3;4:3m",
        "\x1b[8m",
        "\x1b[9m",
        "\x1b[38;5;208m",
        "\x1b[48;2;1;2;3m",
        "\x1b[58;5;196;4m",
        "\x1b[?1049h",
        "\x1b[?1049l",
        "\x1b[?7l",
        "\x1b[?7h",
    ];
    (0..1 + random.below(12))
        .map(|_| random.pick(PIECES))
        .collect()
}

#[test]
fn frames_add_up_to_what_a_full_frame_draws() {
    let mut random = Random(0x9E37_79B9_7F4A_7C15);
    let composings = [
        "",
        "",
        "",
        "x",
        "にほん",
        "e\u{301}",
        "too long to fit at all",
    ];
    for _ in 0..60 {
        let size = Size {
            cols: 1 + random.below(24) as u16,
            rows: 1 + random.below(8) as u16,
        };
        let mut core = TerminalCore::new(size, None);
        let mut colors = palette();
        let mut measure = measured(size.cols, size.rows);
        let mut renderer = Renderer::new(colors.clone());
        renderer.measure(&measure).unwrap();
        // What the window shows, row by row.
        let mut canvas: Vec<RowFrame> = Vec::new();
        let mut caret = None;

        for step in 0..40 {
            core.interpret(output(&mut random).as_bytes());
            match random.below(12) {
                0 => {
                    let size = Size {
                        cols: 1 + random.below(24) as u16,
                        rows: 1 + random.below(8) as u16,
                    };
                    measure = measured(size.cols, size.rows);
                    let layout = renderer.measure(&measure).unwrap();
                    core.resize(layout.size);
                }
                1 => {
                    colors.background = Rgb::new(random.below(256) as u8, 0, 0);
                    renderer.set_palette(colors.clone());
                }
                _ => {}
            }
            let composing = random.pick(&composings);
            if let Some(frame) = renderer.frame(&core, composing) {
                if frame.full.is_some() {
                    canvas.clear();
                }
                for row in frame.rows {
                    let index = usize::from(row.index);
                    if index >= canvas.len() {
                        canvas.resize(index + 1, row.clone());
                    }
                    canvas[index] = row;
                }
                caret = Some(frame.caret);
            }

            let mut fresh = Renderer::new(colors.clone());
            fresh.measure(&measure).unwrap();
            let full = fresh.frame(&core, composing).unwrap();
            assert_eq!(canvas, full.rows, "step {step}");
            assert_eq!(caret, Some(full.caret), "step {step}");
        }
    }
}

// ---------------------------------------------------------------------------------------
// Runs and spans
// ---------------------------------------------------------------------------------------

#[test]
fn a_run_is_cells_drawn_alike() {
    let frame = first(&core(12, 1, "ab\x1b[31mcd\x1b[0m ef"));
    assert_eq!(
        texts(&frame.rows[0]),
        [text(0, "ab"), text(2, "cd"), text(5, "ef")]
    );
    assert_eq!(ink_at(&frame, 2).color, palette().ansi[1]);
    assert_eq!(ink_at(&frame, 5).color, palette().foreground);
}

#[test]
fn blanks_draw_nothing_unless_underlined_or_crossed_out() {
    let frame = first(&core(12, 1, "  a b  \x1b[4m  \x1b[0m"));
    assert_eq!(texts(&frame.rows[0]), [text(2, "a b"), text(7, "  ")]);
    let frame = first(&core(12, 1, "\x1b[9m  x\x1b[0m"));
    assert_eq!(texts(&frame.rows[0]), [text(0, "  x")]);
}

#[test]
fn a_wide_character_and_one_with_marks_are_runs_of_their_own() {
    let frame = first(&core(12, 1, "a界be\u{301}x"));
    assert_eq!(
        texts(&frame.rows[0]),
        [
            text(0, "a"),
            whole(1, "界", 2),
            text(3, "b"),
            whole(4, "e\u{301}", 1),
            text(5, "x"),
        ]
    );
}

#[test]
fn backgrounds_are_spans_but_the_palettes() {
    let frame = first(&core(8, 1, "\x1b[41m  \x1b[42m \x1b[0m  \x1b[41m "));
    let ansi = palette().ansi;
    assert_eq!(
        frame.rows[0].spans,
        [
            Span {
                col: 0,
                count: 2,
                color: ansi[1],
            },
            Span {
                col: 2,
                count: 1,
                color: ansi[2],
            },
            Span {
                col: 5,
                count: 1,
                color: ansi[1],
            },
        ]
    );
    // Inverse on the default colors paints the foreground.
    let frame = first(&core(8, 1, "\x1b[7m x"));
    assert_eq!(frame.rows[0].spans[0].color, palette().foreground);
    assert_eq!(frame.rows[0].spans[0].count, 2);
}

#[test]
fn sequences_the_core_keeps_in_several_cells_are_drawn_as_one() {
    // Two regional indicators are a flag.
    let frame = first(&core(10, 1, "🇯🇵🇫🇷x"));
    assert_eq!(
        texts(&frame.rows[0]),
        [whole(0, "🇯🇵", 2), whole(2, "🇫🇷", 2), text(4, "x")]
    );
    // A skin tone joins the emoji before it.
    let frame = first(&core(10, 1, "👍🏽!"));
    assert_eq!(texts(&frame.rows[0]), [whole(0, "👍🏽", 4), text(4, "!")]);
    // A ZWJ joins what follows.
    let family = "👨\u{200D}👩\u{200D}👧";
    let frame = first(&core(10, 1, family));
    assert_eq!(texts(&frame.rows[0]), [whole(0, family, 6)]);
    // A regional indicator alone is a letter in a cell of its own.
    let frame = first(&core(10, 1, "🇯 x"));
    assert_eq!(texts(&frame.rows[0]), [text(0, "🇯 x")]);
}

#[test]
fn cells_drawn_unlike_are_never_joined() {
    let frame = first(&core(10, 1, "🇯\x1b[31m🇵"));
    assert_eq!(texts(&frame.rows[0]), [text(0, "🇯"), text(1, "🇵")]);
    let frame = first(&core(10, 1, "👍\x1b[1m🏽"));
    assert_eq!(
        texts(&frame.rows[0]),
        [whole(0, "👍", 2), whole(2, "🏽", 2)]
    );
}

// ---------------------------------------------------------------------------------------
// The text being composed
// ---------------------------------------------------------------------------------------

#[test]
fn the_text_being_composed_is_drawn_at_the_cursor_underlined() {
    let core = core(10, 2, "ab");
    let mut renderer = renderer(10, 2);
    let frame = renderer.frame(&core, "にほ").unwrap();
    assert_eq!(
        texts(&frame.rows[0]),
        [text(0, "ab"), whole(2, "に", 2), whole(4, "ほ", 2)]
    );
    assert_eq!(frame.caret, terminal_core::Cursor { row: 0, col: 2 });
    let ink = ink_at(&frame, 2);
    assert_eq!(ink.color, palette().foreground);
    assert_eq!(ink.underline, 1);

    // Composing more redraws the row, and ending gives it back.
    let frame = renderer.frame(&core, "にほん").unwrap();
    assert_eq!(frame.rows.len(), 1);
    let frame = renderer.frame(&core, "").unwrap();
    assert_eq!(texts(&frame.rows[0]), [text(0, "ab")]);
    assert_eq!(frame.caret, terminal_core::Cursor { row: 0, col: 2 });
}

#[test]
fn composed_text_that_would_pass_the_last_column_starts_further_left() {
    let core = core(10, 1, "12345678");
    let frame = renderer(10, 1).frame(&core, "abcd").unwrap();
    assert_eq!(texts(&frame.rows[0]), [text(0, "123456"), text(6, "abcd")]);
    assert_eq!(frame.caret.col, 6);
}

#[test]
fn composed_text_longer_than_the_row_shows_its_end() {
    let frame = renderer(4, 1).frame(&core(4, 1, ""), "abcdef").unwrap();
    assert_eq!(texts(&frame.rows[0]), [text(0, "cdef")]);
    assert_eq!(frame.caret.col, 0);
    // Cut in the middle of a wide character, its end is a blank, underlined like the rest.
    let frame = renderer(3, 1).frame(&core(3, 1, ""), "a界bc").unwrap();
    assert_eq!(texts(&frame.rows[0]), [text(0, " bc")]);
}

#[test]
fn a_wide_character_the_composed_text_covers_half_of_is_blanked() {
    let right_half = core(10, 1, "a界b\x1b[3G");
    let frame = renderer(10, 1).frame(&right_half, "x").unwrap();
    assert_eq!(
        texts(&frame.rows[0]),
        [text(0, "a"), text(2, "x"), text(3, "b")]
    );
    let left_half = core(10, 1, "a界b\x1b[1G");
    let frame = renderer(10, 1).frame(&left_half, "xy").unwrap();
    assert_eq!(texts(&frame.rows[0]), [text(0, "xy"), text(3, "b")]);
}

#[test]
fn marks_join_the_composed_character_before_them() {
    let core = core(10, 1, "");
    let frame = renderer(10, 1).frame(&core, "\u{301}e\u{301}x").unwrap();
    assert_eq!(
        texts(&frame.rows[0]),
        [whole(0, "e\u{301}", 1), text(1, "x")]
    );
}

// ---------------------------------------------------------------------------------------
// Shapes
// ---------------------------------------------------------------------------------------

/// Every character drawn as a shape.
fn shaped() -> impl Iterator<Item = char> {
    ('\u{2500}'..='\u{259F}').chain('\u{E0B0}'..='\u{E0BF}')
}

/// Cells of many sizes, with each thickness of line that fits them: a cell's width and height,
/// and its light and heavy lines.
fn cells() -> Vec<(u32, u32, u32, u32)> {
    let mut sizes = BTreeSet::new();
    for width in 3..=40 {
        for height in (2 * width - 2)..=(2 * width + 4) {
            sizes.insert((width, height.clamp(6, 80)));
        }
    }
    sizes.extend([
        (3, 6),
        (3, 80),
        (40, 80),
        (10, 10),
        (6, 40),
        (40, 6),
        (7, 7),
    ]);
    let mut cells = Vec::new();
    for (width, height) in sizes {
        for light in 1..=3 {
            if light == 1 || 3 * light <= width.min(height) {
                cells.push((width, height, light, light + 2 * (light / 2).max(1)));
            }
        }
    }
    cells
}

/// Whether every piece of `shape` is inside a cell `width` by `height`.
fn inside(shape: &[Primitive], width: u32, height: u32, context: &str) {
    for primitive in shape {
        match primitive {
            Primitive::Rect {
                x,
                y,
                width: w,
                height: h,
                alpha,
            } => {
                assert!(*w > 0 && *h > 0, "{context}: {primitive:?}");
                assert!(
                    x + w <= width && y + h <= height,
                    "{context}: {primitive:?}"
                );
                assert!(*alpha > 0.0 && *alpha <= 1.0, "{context}: {primitive:?}");
            }
            Primitive::Polygon(points) => {
                assert!(points.len() >= 3, "{context}: {primitive:?}");
                for (x, y) in points {
                    let fits = (0.0..=f64::from(width)).contains(x)
                        && (0.0..=f64::from(height)).contains(y);
                    assert!(fits, "{context}: {primitive:?}");
                }
            }
        }
    }
}

/// The pixels `shape` fills in a cell `width` by `height`: a rectangle's, and a polygon's whose
/// centres it covers.
fn raster(shape: &[Primitive], width: u32, height: u32) -> Vec<Vec<bool>> {
    let mut pixels = vec![vec![false; width as usize]; height as usize];
    for primitive in shape {
        match primitive {
            Primitive::Rect {
                x,
                y,
                width: w,
                height: h,
                ..
            } => {
                for row in &mut pixels[*y as usize..(y + h) as usize] {
                    row[*x as usize..(x + w) as usize].fill(true);
                }
            }
            Primitive::Polygon(points) => {
                for (y, row) in pixels.iter_mut().enumerate() {
                    for (x, pixel) in row.iter_mut().enumerate() {
                        *pixel |= contains(points, (x as f64 + 0.5, y as f64 + 0.5));
                    }
                }
            }
        }
    }
    pixels
}

/// Whether `point` is inside the polygon through `points`, by the even-odd rule.
fn contains(points: &[(f64, f64)], (x, y): (f64, f64)) -> bool {
    let mut inside = false;
    for (index, &(x0, y0)) in points.iter().enumerate() {
        let (x1, y1) = points[(index + 1) % points.len()];
        if (y0 > y) != (y1 > y) && x < x0 + (y - y0) * (x1 - x0) / (y1 - y0) {
            inside = !inside;
        }
    }
    inside
}

/// How many pieces the pixels make, counting those that share a side as one.
fn pieces(pixels: &[Vec<bool>]) -> usize {
    let (height, width) = (pixels.len(), pixels[0].len());
    let mut seen = vec![vec![false; width]; height];
    let mut count = 0;
    for y in 0..height {
        for x in 0..width {
            if !pixels[y][x] || seen[y][x] {
                continue;
            }
            count += 1;
            let mut pending = vec![(x, y)];
            seen[y][x] = true;
            while let Some((x, y)) = pending.pop() {
                let next = [
                    (x.wrapping_sub(1), y),
                    (x + 1, y),
                    (x, y.wrapping_sub(1)),
                    (x, y + 1),
                ];
                for (x, y) in next {
                    if y < height && x < width && pixels[y][x] && !seen[y][x] {
                        seen[y][x] = true;
                        pending.push((x, y));
                    }
                }
            }
        }
    }
    count
}

/// The pixels the bands of a line of `weight` cover across an axis `length` long, with lines
/// `light` and `heavy` thick: the rule each character keeps, so that any two meet.
fn band(weight: Weight, length: u32, light: u32, heavy: u32) -> Vec<u32> {
    let start = (length - light) / 2;
    let more = (heavy - light) / 2;
    match weight {
        Weight::None => vec![],
        Weight::Light => (start..start + light).collect(),
        Weight::Heavy => (start - more..start + light + more).collect(),
        Weight::Double => (start - light..start)
            .chain(start + light..start + 2 * light)
            .collect(),
    }
}

/// The pixels filled along one edge of the cell: up, right, down or left.
fn edge(pixels: &[Vec<bool>], side: usize) -> Vec<u32> {
    let (height, width) = (pixels.len(), pixels[0].len());
    let filled = |cells: Vec<bool>| {
        cells
            .iter()
            .enumerate()
            .filter(|(_, filled)| **filled)
            .map(|(at, _)| at as u32)
            .collect()
    };
    match side {
        0 => filled(pixels[0].clone()),
        1 => filled(pixels.iter().map(|row| row[width - 1]).collect()),
        2 => filled(pixels[height - 1].clone()),
        _ => filled(pixels.iter().map(|row| row[0]).collect()),
    }
}

#[test]
fn every_shape_stays_in_its_cell() {
    for (width, height, light, heavy) in cells() {
        for char in shaped() {
            let shape = shape(char, width, height, light, heavy).unwrap();
            let context = format!("{char} in {width} by {height}, lines {light} and {heavy}");
            // An eighth of a cell a few pixels wide rounds to nothing.
            let eighth = matches!(char, '▁' | '▏' | '▔' | '▕');
            assert!(!shape.is_empty() || eighth, "{context}");
            inside(&shape, width, height, &context);
        }
    }
    for char in ['a', '■', '\u{E0A0}', '\u{E0C0}', '\u{24FF}'] {
        assert!(!is_shape(char));
        assert_eq!(shape(char, 8, 18, 1, 3), None);
    }
}

#[test]
fn every_line_leaves_its_edge_where_its_weight_says() {
    // So a line meets the line of any neighbour that leaves the facing edge. In a cell no wider
    // than its lines they reach every edge, whatever their arms.
    for (width, height, light, heavy) in cells() {
        if width.min(height) < heavy.max(3 * light) + 2 {
            continue;
        }
        for char in '\u{2500}'..='\u{257F}' {
            let Some(arms) = arms(char) else { continue };
            let pixels = raster(
                &shape(char, width, height, light, heavy).unwrap(),
                width,
                height,
            );
            for (side, weight) in arms.into_iter().enumerate() {
                let across = if side % 2 == 0 { width } else { height };
                assert_eq!(
                    edge(&pixels, side),
                    band(weight, across, light, heavy),
                    "{char} in {width} by {height}, lines {light} and {heavy}, side {side}"
                );
            }
        }
    }
}

/// The corners of a side of a cell `width` by `height`: up, right, down or left.
fn corners(side: usize, width: u32, height: u32) -> [(f64, f64); 2] {
    let (w, h) = (f64::from(width), f64::from(height));
    match side {
        0 => [(0.0, 0.0), (w, 0.0)],
        1 => [(w, 0.0), (w, h)],
        2 => [(0.0, h), (w, h)],
        _ => [(0.0, 0.0), (0.0, h)],
    }
}

/// Where the corners of a polygon lie on one side of its cell, along that side.
fn on_side(points: &[(f64, f64)], side: usize, width: u32, height: u32) -> Vec<f64> {
    let (w, h) = (f64::from(width), f64::from(height));
    let mut along: Vec<f64> = points
        .iter()
        .filter_map(|&(x, y)| match side {
            0 => (y == 0.0).then_some(x),
            1 => (x == w).then_some(y),
            2 => (y == h).then_some(x),
            _ => (x == 0.0).then_some(y),
        })
        .collect();
    along.sort_by(f64::total_cmp);
    along.dedup();
    along
}

#[test]
fn rounded_corners_end_on_the_light_lines() {
    let ends = [('╭', [2, 1]), ('╮', [2, 3]), ('╯', [0, 3]), ('╰', [0, 1])];
    for (width, height, light, heavy) in cells() {
        let context = format!("{width} by {height}, line {light}");
        for (char, sides) in ends {
            let shape = shape(char, width, height, light, heavy).unwrap();
            let [Primitive::Polygon(points)] = shape.as_slice() else {
                panic!("{char} in {context}: {shape:?}");
            };
            for side in [0, 1, 2, 3] {
                let across = if side % 2 == 0 { width } else { height };
                let line: Vec<f64> = band(Weight::Light, across, light, heavy)
                    .first()
                    .map(|start| vec![f64::from(*start), f64::from(start + light)])
                    .filter(|_| sides.contains(&side))
                    .unwrap_or_default();
                assert_eq!(
                    on_side(points, side, width, height),
                    line,
                    "{char} in {context}, side {side}"
                );
            }
            // In a cell shaped as a font's are, the pixels at the ends are the light line's too.
            // In a cell much taller than wide, or wider than tall, the curve turns within the
            // last pixel.
            if width >= 6 && width <= height && height <= 3 * width {
                let pixels = raster(&shape, width, height);
                for side in sides {
                    let across = if side % 2 == 0 { width } else { height };
                    assert_eq!(
                        edge(&pixels, side),
                        band(Weight::Light, across, light, heavy),
                        "{char} in {context}, side {side}"
                    );
                }
            }
        }
    }
}

#[test]
fn double_lines_have_their_inner_and_outer_corners() {
    let expected = [
        ('═', 2),
        ('║', 2),
        ('╒', 1),
        ('╓', 1),
        ('╔', 2),
        ('╕', 1),
        ('╖', 1),
        ('╗', 2),
        ('╘', 1),
        ('╙', 1),
        ('╚', 2),
        ('╛', 1),
        ('╜', 1),
        ('╝', 2),
        ('╞', 1),
        ('╟', 2),
        ('╠', 3),
        ('╡', 1),
        ('╢', 2),
        ('╣', 3),
        ('╤', 2),
        ('╥', 1),
        ('╦', 3),
        ('╧', 2),
        ('╨', 1),
        ('╩', 3),
        ('╪', 1),
        ('╫', 1),
        ('╬', 4),
    ];
    for (width, height, light, heavy) in cells() {
        for (char, count) in expected {
            let pixels = raster(
                &shape(char, width, height, light, heavy).unwrap(),
                width,
                height,
            );
            assert_eq!(
                pieces(&pixels),
                count,
                "{char} in {width} by {height}, lines {light}:\n{}",
                picture(&pixels)
            );
        }
    }
}

#[test]
fn lines_and_crossings_are_one_piece() {
    for (width, height, light, heavy) in cells() {
        for char in '\u{2500}'..='\u{257F}' {
            let Some(arms) = arms(char) else { continue };
            if arms.contains(&Weight::Double) {
                continue;
            }
            let pixels = raster(
                &shape(char, width, height, light, heavy).unwrap(),
                width,
                height,
            );
            assert_eq!(pieces(&pixels), 1, "{char} in {width} by {height}");
        }
    }
}

#[test]
fn dashed_lines_have_their_dashes() {
    let dashes = [
        ('┄', 3, false),
        ('┅', 3, false),
        ('┆', 3, true),
        ('┇', 3, true),
        ('┈', 4, false),
        ('┉', 4, false),
        ('┊', 4, true),
        ('┋', 4, true),
        ('╌', 2, false),
        ('╍', 2, false),
        ('╎', 2, true),
        ('╏', 2, true),
    ];
    for (width, height, light, heavy) in cells() {
        for (char, count, vertical) in dashes {
            let length = if vertical { height } else { width };
            if length < 2 * count {
                continue;
            }
            let pixels = raster(
                &shape(char, width, height, light, heavy).unwrap(),
                width,
                height,
            );
            assert_eq!(
                pieces(&pixels),
                count as usize,
                "{char} in {width} by {height}"
            );
        }
    }
}

#[test]
fn blocks_that_make_a_cell_between_them_tile_it() {
    let pairs = [
        ('▀', '▄'),
        ('▔', '▇'),
        ('▌', '▐'),
        ('▉', '▕'),
        ('▘', '▟'),
        ('▝', '▙'),
        ('▖', '▜'),
        ('▗', '▛'),
        ('▚', '▞'),
    ];
    for (width, height, light, heavy) in cells() {
        for (one, other) in pairs {
            let one_pixels = raster(
                &shape(one, width, height, light, heavy).unwrap(),
                width,
                height,
            );
            let other_pixels = raster(
                &shape(other, width, height, light, heavy).unwrap(),
                width,
                height,
            );
            for (y, row) in one_pixels.iter().enumerate() {
                for (x, filled) in row.iter().enumerate() {
                    assert!(
                        *filled != other_pixels[y][x],
                        "{one} and {other} in {width} by {height} at {x}, {y}"
                    );
                }
            }
        }
        // The eighths grow by about an eighth each.
        let lower: Vec<usize> = ('\u{2581}'..='\u{2588}')
            .map(|char| {
                let pixels = raster(
                    &shape(char, width, height, light, heavy).unwrap(),
                    width,
                    height,
                );
                pixels.iter().filter(|row| row[0]).count()
            })
            .collect();
        assert!(lower.windows(2).all(|pair| pair[0] <= pair[1]), "{lower:?}");
        assert_eq!(lower[7], height as usize);
    }
}

#[test]
fn shades_are_the_text_color_over_the_background() {
    for (char, alpha) in [('░', 0.25), ('▒', 0.5), ('▓', 0.75)] {
        let shape = shape(char, 8, 18, 1, 3).unwrap();
        assert_eq!(
            shape,
            [Primitive::Rect {
                x: 0,
                y: 0,
                width: 8,
                height: 18,
                alpha,
            }]
        );
    }
}

#[test]
fn powerline_separators_meet_the_cells_beside_them() {
    // Each fills a side of its cell from corner to corner, and the solid arrows and half
    // ellipses come to a point or a curve at the other.
    let sides = [
        ('\u{E0B0}', vec![3]),
        ('\u{E0B2}', vec![1]),
        ('\u{E0B4}', vec![3]),
        ('\u{E0B6}', vec![1]),
        ('\u{E0B8}', vec![3, 2]),
        ('\u{E0BA}', vec![1, 2]),
        ('\u{E0BC}', vec![3, 0]),
        ('\u{E0BE}', vec![1, 0]),
    ];
    for (width, height, light, heavy) in cells() {
        for (char, sides) in &sides {
            let shape = shape(*char, width, height, light, heavy).unwrap();
            let context = format!("{char:?} in {width} by {height}");
            let [Primitive::Polygon(points)] = shape.as_slice() else {
                panic!("{context}: {shape:?}");
            };
            for side in sides {
                for corner in corners(*side, width, height) {
                    assert!(
                        points.contains(&corner),
                        "{context}, side {side}: {points:?}"
                    );
                }
            }
            if sides.len() == 1 {
                let pixels = raster(&shape, width, height);
                assert!(
                    edge(&pixels, sides[0] ^ 2).len() < height as usize,
                    "{context}"
                );
            }
        }
    }
}

/// The pixels as text: `#` for a filled one.
fn picture(pixels: &[Vec<bool>]) -> String {
    pixels
        .iter()
        .map(|row| {
            row.iter()
                .map(|filled| if *filled { '#' } else { '.' })
                .collect::<String>()
        })
        .collect::<Vec<_>>()
        .join("\n")
}

#[test]
fn shapes_look_as_they_should() {
    let drawn = |char| picture(&raster(&shape(char, 7, 9, 1, 3).unwrap(), 7, 9));
    assert_eq!(
        drawn('┼'),
        "...#...\n...#...\n...#...\n...#...\n#######\n...#...\n...#...\n...#...\n...#..."
    );
    assert_eq!(
        drawn('┍'),
        ".......\n.......\n.......\n...####\n...####\n...####\n...#...\n...#...\n...#..."
    );
    assert_eq!(
        drawn('╔'),
        ".......\n.......\n.......\n..#####\n..#....\n..#.###\n..#.#..\n..#.#..\n..#.#.."
    );
    assert_eq!(
        drawn('╬'),
        "..#.#..\n..#.#..\n..#.#..\n###.###\n.......\n###.###\n..#.#..\n..#.#..\n..#.#.."
    );
    assert_eq!(
        drawn('╤'),
        ".......\n.......\n.......\n#######\n.......\n#######\n...#...\n...#...\n...#..."
    );
    assert_eq!(
        drawn('▚'),
        "####...\n####...\n####...\n####...\n####...\n....###\n....###\n....###\n....###"
    );
    assert_eq!(
        drawn('\u{E0B0}'),
        "#......\n##.....\n####...\n#####..\n#######\n#####..\n####...\n##.....\n#......"
    );
}

#[test]
fn underlines_run_on_from_cell_to_cell() {
    for (width, height, light, _) in cells() {
        let baseline = height * 3 / 4;
        let (underlines, strikethrough) =
            decorations(width, height, baseline, f64::from(height) * 0.75, light);
        let context = format!("{width} by {height}, line {light}");
        for shape in underlines.iter().chain([&strikethrough]) {
            assert!(!shape.is_empty(), "{context}");
            inside(shape, width, height, &context);
        }
        // A wave starts where it ends.
        let Primitive::Polygon(wave) = &underlines[2][0] else {
            panic!("{context}: {:?}", underlines[2]);
        };
        let at = |x: f64| -> Vec<f64> {
            wave.iter()
                .filter(|point| point.0 == x)
                .map(|point| point.1)
                .collect()
        };
        let (mut start, mut end) = (at(0.0), at(f64::from(width)));
        start.sort_by(f64::total_cmp);
        end.sort_by(f64::total_cmp);
        assert_eq!(start, end, "{context}");
        // Dots and dashes are as far apart across cells as within them.
        for shape in [&underlines[3], &underlines[4]] {
            let starts: Vec<u32> = shape
                .iter()
                .map(|primitive| match primitive {
                    Primitive::Rect { x, .. } => *x,
                    Primitive::Polygon(_) => panic!("{context}"),
                })
                .chain(shape.first().map(|first| match first {
                    Primitive::Rect { x, .. } => x + width,
                    Primitive::Polygon(_) => unreachable!(),
                }))
                .collect();
            let steps: Vec<u32> = starts.windows(2).map(|pair| pair[1] - pair[0]).collect();
            let (least, most) = (steps.iter().min().unwrap(), steps.iter().max().unwrap());
            assert!(most - least <= 1, "{context}: {steps:?}");
            // No two touch, unless a cell has room for only one.
            assert!(*least > light || steps.len() == 1, "{context}: {steps:?}");
        }
    }
}

// ---------------------------------------------------------------------------------------
// The frame as JSON
// ---------------------------------------------------------------------------------------

/// Reads back the JSON string at the start of `text`, as JSON and JavaScript both read it.
/// Returns it and what follows it.
fn read_string(text: &str) -> (String, &str) {
    let mut chars = text.char_indices();
    assert_eq!(chars.next().map(|(_, char)| char), Some('"'), "{text}");
    let mut read = String::new();
    let mut pending: Option<u32> = None;
    while let Some((at, char)) = chars.next() {
        match char {
            '"' => {
                assert_eq!(pending, None);
                return (read, &text[at + 1..]);
            }
            '\\' => {
                let escaped = chars.next().unwrap().1;
                let char = match escaped {
                    '"' | '\\' | '/' => escaped,
                    'n' => '\n',
                    't' => '\t',
                    'r' => '\r',
                    'b' => '\u{8}',
                    'f' => '\u{c}',
                    'u' => {
                        let hex: String = (0..4).map(|_| chars.next().unwrap().1).collect();
                        let unit = u32::from_str_radix(&hex, 16).unwrap();
                        match (pending.take(), unit) {
                            (None, 0xD800..=0xDBFF) => {
                                pending = Some(unit);
                                continue;
                            }
                            (Some(high), 0xDC00..=0xDFFF) => {
                                char::from_u32(0x10000 + ((high - 0xD800) << 10) + (unit - 0xDC00))
                                    .unwrap()
                            }
                            (None, unit) => char::from_u32(unit).unwrap(),
                            (Some(_), _) => panic!("a lone surrogate"),
                        }
                    }
                    other => panic!("\\{other} is no escape"),
                };
                read.push(char);
            }
            // What JSON forbids in a string, and what JavaScript once took for a line's end.
            char if char < ' ' || char == '\u{2028}' || char == '\u{2029}' => {
                panic!("{char:?} is not escaped")
            }
            char => read.push(char),
        }
    }
    panic!("the string does not end: {text}");
}

#[test]
fn every_character_is_written_as_json_and_javascript_read_it() {
    let all: Vec<char> = (0..=0x10FFFF).filter_map(char::from_u32).collect();
    for chunk in all.chunks(4096) {
        let text: String = chunk.iter().collect();
        let mut written = String::new();
        json::string(&mut written, &text);
        // Nothing a program writes can end the string, or the script it is in.
        assert!(!written.contains('\u{2028}') && !written.contains('\u{2029}'));
        assert!(!written.chars().any(|char| char < ' '));
        let (read, rest) = read_string(&written);
        assert_eq!(read, text);
        assert_eq!(rest, "");
    }
    let mut written = String::new();
    json::string(&mut written, "\"</script>\\\u{7}\u{85}\u{2028}");
    assert_eq!(written, "\"\\\"</script>\\\\\\u0007\\u0085\\u2028\"");
}

#[test]
fn a_frame_is_written_so() {
    let mut core = core(6, 2, "a");
    let mut renderer = renderer(6, 2);
    renderer.frame(&core, "").unwrap();
    core.interpret("\x1b[1;31mb\x1b[0;4:3;58;5;196m\"c\x1b[0;44m─\x1b[0m".as_bytes());
    let json = renderer.frame(&core, "").unwrap().json();
    assert_eq!(
        json,
        "{\"colors\":[\"#040068\",\"#c8c8c8\",\"#010065\",\"#ff0000\"],\
         \"styles\":[[1,0],[2,1],[1,24,3]],\
         \"shapes\":{\"─\":[[3,8,5,1],[0,8,4,1]]},\
         \"rows\":[[0,[4,1,0],[[0,0,\"a\"],[1,1,\"b\"],[2,2,\"\\\"c\"],[4,0,\"─\"]]]],\
         \"caret\":[0,5]}"
    );
}

#[test]
fn a_full_frame_has_the_layout() {
    let json = first(&core(2, 1, "")).json();
    assert!(
        json.starts_with(
            "{\"full\":{\"canvas\":[16,18],\"scale\":1,\"cell\":[8,18],\"baseline\":14,\
             \"font\":\"Mono\",\"fontSize\":13,\"background\":\"#0a0a0a\",\"underlines\":[[[0,15,8,1]],"
        ),
        "{json}"
    );
    assert!(json.contains("\"strikethrough\":[[0,10,8,1]]}"), "{json}");
    assert!(
        json.ends_with("\"rows\":[[0,[],[]]],\"caret\":[0,0]}"),
        "{json}"
    );
}

#[test]
fn frames_are_small() {
    // A prompt with some colors, on a terminal 200 columns wide: a frame while typing.
    let prompt = "\x1b[32muser@host\x1b[0m:\x1b[34m~/projects/twominal\x1b[0m$ ";
    let line = format!(
        "{prompt}{}",
        "cargo test --workspace -- --nocapture ".repeat(4)
    );
    let mut prompt = core(200, 60, "");
    let mut renderer = renderer(200, 60);
    renderer.frame(&prompt, "").unwrap();
    prompt.interpret(line.as_bytes());
    let typing = renderer.frame(&prompt, "").unwrap().json();
    assert!(typing.len() < 1024, "{} bytes: {typing}", typing.len());

    // A screen of plain text, all of it at once.
    let text = "The quick brown fox jumps over the lazy dog. ".repeat(5);
    let screen = format!("{}\r\n", &text[..199]).repeat(60);
    let full = first(&core(200, 60, &screen)).json();
    assert!(full.len() < 16 * 1024, "{} bytes", full.len());
}
