//! The Terminal Core's own rules, which the conformance suite in `tests/conformance/` cannot
//! see through the public interface: internal state, the reports of the shell integration,
//! resizing, the table of widths, and the same state however the output is cut (ARC-03,
//! REL-02, FISH-05, PERF-06, TERM-02, TERM-03). What a program sees of the screen is checked there
//! (MAINT-03).

use alloc::string::{String, ToString};
use alloc::vec::Vec;
use alloc::{format, vec};

use crate::fuzz::{self, Random};
use crate::width::width;
use crate::{Color, Event, Report, Size, TerminalCore};

/// A terminal of `cols` × `rows` that has interpreted `bytes`.
fn core(cols: u16, rows: u16, bytes: &[u8]) -> TerminalCore {
    let mut core = TerminalCore::new(Size { cols, rows }, None);
    core.interpret(bytes);
    core
}

/// The rows of the screen being shown, without their trailing blanks.
fn rows(core: &TerminalCore) -> Vec<String> {
    core.screen()
        .rows()
        .iter()
        .map(|row| row.text().trim_end().to_string())
        .collect()
}

fn cursor(core: &TerminalCore) -> (u16, u16) {
    let cursor = core.cursor();
    (cursor.row, cursor.col)
}

/// The reports in `bytes`, for a session whose nonce is `n1`.
fn reports(bytes: &[u8]) -> Vec<Report> {
    let mut core = TerminalCore::new(Size { cols: 20, rows: 3 }, Some("n1"));
    core.interpret(bytes)
        .into_iter()
        .map(|event| match event {
            Event::Report(report) => report,
            other => panic!("not a report: {other:?}"),
        })
        .collect()
}

/// Rows `1` to `count`, each holding its number.
fn numbered(count: usize) -> Vec<u8> {
    let lines: Vec<String> = (1..=count).map(|line| line.to_string()).collect();
    lines.join("\r\n").into_bytes()
}

// The state behind the screen

#[test]
fn rep_is_the_character_printed_again() {
    let text = b"abcdefghij\r\nklmnopqrst\r\nuvwxyz\x1b[2;3H";
    // A wide character too: on an even width, on an odd one, whose last column it skips, and
    // on a width of one, where it never fits.
    for (cols, rows, char) in [
        (8, 5, "a"),
        (8, 5, "日"),
        (7, 5, "日"),
        (7, 3, "日"),
        (1, 5, "日"),
    ] {
        for modes in [
            &b""[..],
            b"\x1b[4h",
            b"\x1b[?7l",
            b"\x1b[4h\x1b[?7l",
            b"\x1b[2;3r",
            // Below the region on the last row, and above it.
            b"\x1b[2;3r\x1b[5;7H",
            b"\x1b[3;4r\x1b[1;5H\x1b[4h",
            // In a style, whose background the rows that scroll in take.
            b"\x1b[1;4;38;5;9;44m",
            // On a wide character's right half.
            b"\x1b[H\xe6\x97\xa5\x1b[D",
        ] {
            // Past two screenfuls and a row, 88 for `a` here, the core cuts the count by whole
            // rows, where xterm prints them all. The state must not show it.
            let counts = [
                1, 2, 7, 8, 9, 23, 26, 32, 33, 34, 39, 47, 48, 49, 50, 55, 56, 57, 87, 88, 89, 90,
                95, 96, 97, 100, 333,
            ];
            for count in counts {
                let prefix = [&text[..], modes].concat();
                let repeated = core(
                    cols,
                    rows,
                    &[&prefix[..], format!("{char}\x1b[{count}b").as_bytes()].concat(),
                );
                let printed = core(
                    cols,
                    rows,
                    &[&prefix[..], char.repeat(count + 1).as_bytes()].concat(),
                );
                assert!(
                    repeated.terminal == printed.terminal,
                    "{cols}x{rows} {char} {modes:?} {count}"
                );
            }
        }
    }
}

#[test]
fn ris_leaves_what_a_new_terminal_has() {
    // The region, the tab stops, the character sets, the style, the saved cursors, the
    // keyboard flags, the mouse modes and the other screen too, which `resets.txt`,
    // `styles.txt` and `keyboard.txt` see only in part.
    let mut core = core(
        10,
        3,
        b"\x1b[1;4:3;38;5;9;48;2;1;2;3;58:5:4mtext\x1b[2;3r\x1b[?25l\x1b[?2004h\x1b[?1000h\x1b[3g\
          \x1b[?1015h\x1b[?1007l\x1b(0\x1b[>5u\x1b7\x1b[?1049h\x1b[>31ufull",
    );
    core.interpret(b"\x1bc");
    assert!(core.terminal == self::core(10, 3, b"").terminal);
}

// Reports

#[test]
fn reports_that_carry_the_nonce_are_read() {
    let text = b"\x1b]2222;n1;cwd;/a%20b%3Bc%25d%C3%A9\x07\
                 \x1b]2222;n1;path;/usr/bin:/bin\x07\
                 \x1b]2222;n1;aliases;ll la\x07\
                 \x1b]2222;n1;functions;f%3Bx g\x07\
                 \x1b]2222;n1;cmdlets;Get-Item Set-Item\x07\
                 \x1b]2222;n1;ready\x07";
    assert_eq!(
        reports(text),
        [
            Report::Directory("/a b;c%dé".as_bytes().to_vec()),
            Report::SearchPath(b"/usr/bin:/bin".to_vec()),
            Report::Aliases(vec!["ll".to_string(), "la".to_string()]),
            Report::Functions(vec!["f;x".to_string(), "g".to_string()]),
            Report::Cmdlets(vec!["Get-Item".to_string(), "Set-Item".to_string()]),
            Report::Ready,
        ]
    );
    let mut core = TerminalCore::new(Size { cols: 10, rows: 1 }, Some("n1"));
    core.interpret(&[&b"a"[..], text, b"b"].concat());
    assert_eq!(rows(&core), ["ab"]);
}

#[test]
fn a_report_keeps_its_place_among_the_replies() {
    let mut core = TerminalCore::new(Size { cols: 10, rows: 1 }, Some("n1"));
    assert_eq!(
        core.interpret(b"\x1b[5n\x1b]2222;n1;ready\x07\x1b[5n"),
        [
            Event::Reply(b"\x1b[0n".to_vec()),
            Event::Report(Report::Ready),
            Event::Reply(b"\x1b[0n".to_vec()),
        ]
    );
}

#[test]
fn values_are_decoded_as_the_scripts_encode_them() {
    assert_eq!(
        reports(b"\x1b]2222;n1;cwd;%2f%2F\x07\x1b]2222;n1;path;\x07\x1b]2222;n1;aliases;\x07"),
        [
            Report::Directory(b"//".to_vec()),
            Report::SearchPath(Vec::new()),
            Report::Aliases(Vec::new()),
        ]
    );
    // A path is bytes, UTF-8 or not. A name that is not UTF-8 cannot be typed, so it is
    // left out.
    assert_eq!(
        reports(b"\x1b]2222;n1;cwd;/%FF\x07\x1b]2222;n1;functions;ok %FF  also\x07"),
        [
            Report::Directory(b"/\xff".to_vec()),
            Report::Functions(vec!["ok".to_string(), "also".to_string()]),
        ]
    );
}

#[test]
fn what_is_not_a_report_is_ignored() {
    for text in [
        // Another nonce, or another number.
        &b"\x1b]2222;n2;ready\x07"[..],
        b"\x1b]2222;n;ready\x07",
        b"\x1b]2222;;ready\x07",
        b"\x1b]2223;n1;ready\x07",
        // A key Twominal does not know, or one in the wrong shape.
        b"\x1b]2222;n1;title;x\x07",
        b"\x1b]2222;n1;ready;x\x07",
        b"\x1b]2222;n1;cwd\x07",
        b"\x1b]2222;n1;cwd;a;b\x07",
        b"\x1b]2222;n1\x07",
        // Not as the scripts write it.
        b"\x1b]2222;n1;cwd;/a b\x07",
        b"\x1b]2222;n1;cwd;%4\x07",
        b"\x1b]2222;n1;cwd;%zz\x07",
        b"\x1b]2222;n1;cwd;\xc3\xa9\x07",
        b"\x1b]2222;n1;aliases;a\xffb\x07",
        // Not ended by BEL: cut short.
        b"\x1b]2222;n1;ready\x1b\\",
        b"\x1b]2222;n1;ready\x18",
        b"\x1b]2222;n1;ready\x1b[H",
    ] {
        assert_eq!(reports(text), [], "{text:?}");
    }
    // Without integration nothing is a report.
    let mut core = core(10, 1, b"");
    assert_eq!(
        core.interpret(b"\x1b]2222;;ready\x07\x1b]2222;n1;ready\x07"),
        []
    );
}

/// A `functions` report of `count` names, each ten bytes long with the space after it.
fn functions(count: usize) -> Vec<u8> {
    let names: Vec<String> = (0..count).map(|name| format!("name{name:05}")).collect();
    [
        &b"\x1b]2222;n1;functions;"[..],
        names.join(" ").as_bytes(),
        b"\x07",
    ]
    .concat()
}

#[test]
fn a_long_report_arrives_whole() {
    // 200 KB, as a zsh with many plugins, or PowerShell with many modules, may send.
    let [Report::Functions(names)] = &reports(&functions(20_000))[..] else {
        panic!("one functions report");
    };
    assert_eq!(names.len(), 20_000);
    assert_eq!(names[19_999], "name19999");
}

#[test]
fn a_report_over_the_limit_is_dropped_and_what_follows_is_read() {
    let mut core = TerminalCore::new(Size { cols: 10, rows: 1 }, Some("n1"));
    let text = [&functions(30_000)[..], b"after\x1b]2222;n1;ready\x07"].concat();
    assert!(text.len() > crate::REPORT_LIMIT);
    assert_eq!(core.interpret(&text), [Event::Report(Report::Ready)]);
    assert_eq!(rows(&core), ["after"]);
}

// Resizing

#[test]
fn resizing_cuts_and_pads_rows() {
    let mut core = core(10, 2, b"abcdefghij");
    core.resize(Size { cols: 5, rows: 2 });
    assert_eq!(rows(&core), ["abcde", ""]);
    assert_eq!(cursor(&core), (0, 4));
    core.resize(Size { cols: 8, rows: 3 });
    assert_eq!(rows(&core), ["abcde", "", ""]);
    assert_eq!(core.screen().rows()[0].cells().len(), 8);
}

#[test]
fn fewer_rows_go_from_the_top_only_to_keep_the_cursor() {
    let mut core = core(5, 4, &numbered(4));
    core.resize(Size { cols: 5, rows: 2 });
    assert_eq!(rows(&core), ["3", "4"]);
    assert_eq!(cursor(&core), (1, 1));

    let mut core = self::core(5, 4, &numbered(4));
    core.interpret(b"\x1b[2H");
    core.resize(Size { cols: 5, rows: 2 });
    assert_eq!(rows(&core), ["1", "2"]);
    assert_eq!(cursor(&core), (1, 0));
}

#[test]
fn the_primary_screen_keeps_its_rows_while_the_alternate_is_shown() {
    let mut core = core(5, 4, &numbered(4));
    core.interpret(b"\x1b[?1049h\x1b[Hvim");
    core.resize(Size { cols: 5, rows: 2 });
    assert_eq!(rows(&core), ["vim", ""]);
    core.interpret(b"\x1b[?1049l");
    assert_eq!(rows(&core), ["3", "4"]);
    assert_eq!(cursor(&core), (1, 1));
}

#[test]
fn resizing_resets_the_region_and_extends_the_tab_stops() {
    let mut core = core(10, 4, &numbered(4));
    core.interpret(b"\x1b[1;2r");
    core.resize(Size { cols: 20, rows: 4 });
    core.interpret(b"\x1b[4H\n\t\tx");
    assert_eq!(rows(&core), ["2", "3", "4", "                x"]);
}

#[test]
fn resizing_blanks_a_wide_character_it_cuts_in_two() {
    let mut core = core(6, 2, "ab日本".as_bytes());
    core.resize(Size { cols: 4, rows: 2 });
    assert_eq!(rows(&core), ["ab日", ""]);
    core.resize(Size { cols: 3, rows: 2 });
    assert_eq!(rows(&core), ["ab", ""]);
    let cells = core.screen().rows()[0].cells();
    assert_eq!(cells[2].width(), 1);
    core.terminal.check();
}

#[test]
fn resizing_pads_with_the_default_style() {
    // The window changed the size, not the program, so what it adds or cuts has no colors.
    let mut core = core(3, 1, "\x1b[44m\x1b[2Ja日".as_bytes());
    core.resize(Size { cols: 4, rows: 2 });
    let background = |core: &TerminalCore, row: usize, col: usize| {
        core.screen().rows()[row].cells()[col].style().background
    };
    assert_eq!(background(&core, 0, 0), Color::Palette(4));
    assert_eq!(background(&core, 0, 2), Color::Palette(4));
    assert_eq!(background(&core, 0, 3), Color::Default);
    assert_eq!(background(&core, 1, 0), Color::Default);
    core.resize(Size { cols: 2, rows: 2 });
    assert_eq!(rows(&core), ["a", ""]);
    assert_eq!(background(&core, 0, 1), Color::Default);
    core.terminal.check();
}

#[test]
fn a_size_of_zero_is_one() {
    let mut core = TerminalCore::new(Size { cols: 0, rows: 0 }, None);
    assert_eq!(core.size(), Size { cols: 1, rows: 1 });
    core.interpret(b"ab\x1b[5;5H\x1b[L\x1b[P\tc");
    assert_eq!(rows(&core), ["c"]);
    core.resize(Size { cols: 0, rows: 3 });
    assert_eq!(core.size(), Size { cols: 1, rows: 3 });
}

// Widths (TERM-02)

#[test]
fn widths_are_xterms() {
    for (char, cells) in [
        ('a', 1),
        ('日', 2),
        ('😀', 2),
        ('\u{1f3fd}', 2),
        // Ambiguous, and a regional indicator, half a flag.
        ('α', 1),
        ('\u{1f1ef}', 1),
        ('\u{301}', 0),
        ('\u{200d}', 0),
        ('\u{fe0f}', 0),
        ('\u{1161}', 0),
        // Where unicode-width alone would give another width.
        ('\u{ad}', 1),
        ('\u{bbe}', 1),
        ('\u{17d8}', 1),
        ('\u{3164}', 2),
        ('\u{600}', 0),
    ] {
        assert_eq!(width(char), cells, "{char:?}");
    }
}

#[test]
fn no_character_is_wider_than_two_cells() {
    for char in (0..=0x10_ffff).filter_map(char::from_u32) {
        assert!(width(char) <= 2, "{char:?}");
    }
}

// Deterministic, whatever the input (ARC-03, REL-02)

/// Pieces of control functions, characters and garbage, for building hostile output.
const PIECES: &[&[u8]] = &[
    b"\x1b",
    b"\x1b[",
    b"\x1b]",
    b"\x1bP",
    b"\x1b_",
    b"[",
    b"]",
    b"?",
    b">",
    b"=",
    b"<",
    b"$",
    b"!",
    b" ",
    b"#",
    b"(",
    b")",
    b";",
    b":",
    b"0",
    b"1",
    b"2",
    b"5",
    b"6",
    b"9",
    b"25",
    b"38;5;",
    b"48;2;",
    b"38:2::",
    b"58:5:",
    b"4:3",
    b"255",
    b"256",
    b"47",
    b"1049",
    b"1047",
    b"1048",
    b"1005",
    b"1006",
    b"1007",
    b"1015",
    b"1016",
    b"2222",
    b"n1",
    b"65535",
    b"99999999",
    b"@",
    b"A",
    b"B",
    b"C",
    b"D",
    b"E",
    b"G",
    b"H",
    b"I",
    b"J",
    b"K",
    b"L",
    b"M",
    b"P",
    b"S",
    b"T",
    b"W",
    b"X",
    b"Z",
    b"b",
    b"c",
    b"d",
    b"g",
    b"h",
    b"l",
    b"m",
    b"n",
    b"p",
    b"q",
    b"r",
    b"s",
    b"t",
    b"u",
    b"7",
    b"8",
    b"\n",
    b"\r",
    b"\t",
    b"\x08",
    b"\x0e",
    b"\x0f",
    b"\x07",
    b"\x18",
    b"\x1a",
    b"\x05",
    b"\x7f",
    b"\x9b",
    b"\xc2",
    b"\xc2\x9b",
    b"\xe2\x94\x80",
    b"\xf0\x9f\x98\x80",
    b"\xe6\x97\xa5",
    b"\xcc\x81",
    b"\xe1\x85\xa1",
    b"\xe2\x80\x8d",
    b"\xef\xb8\x8f",
    b"\xe2\x80\x8b",
    b"\xc2\xad",
    b"\xff",
    b"\x1b\\",
    b"\x1b[>1u",
    b"\x1b[<u",
    b"\x1b[=5;2u",
    b"\x1b[?u",
    b"cwd;",
    b"ready",
    b"%",
    b"x",
    b"hello ",
];

/// `length` bytes or so of hostile output.
fn hostile(random: &mut Random, length: usize) -> Vec<u8> {
    let mut bytes = Vec::with_capacity(length + 8);
    while bytes.len() < length {
        if random.below(8) == 0 {
            bytes.push(random.number() as u8);
        } else {
            bytes.extend_from_slice(PIECES[random.below(PIECES.len())]);
        }
    }
    bytes
}

/// Output with a little of everything, to cut in pieces.
const SAMPLE: &[u8] =
    b"hello\r\n\x1b[31mred\x1b[m \x1b[1;38;5;196;48:2::1:2:3;4:3;58;2;9;8;7mstyled\x1b[44m\x1b[K\
                       \x1b[0m\r\n\xe2\x94\x80 caf\xc3\xa9 \xe6\x97\xa5\xe6\x9c\xac e\xcc\x81\xcc\x88\
                       \xf0\x9f\x91\xa8\xe2\x80\x8d\xf0\x9f\x91\xa9 \xe2\x80\x8b\xe6\x97\xa5\xc2\x85\x1b[2;5r\x1b[5H\n\n\n\
                       \x1b]2222;n1;cwd;/tmp\x07\x1b[?1049hfull\x1b[6n\x1b[?1049l\x1b(0lqk\x1b(Bz\
                       \x1b[3b\tx\x1b[c\x1b]2222;n1;ready\x07\xd0\xb9 \xd0\xbc\xf0\x9f\x98\x80!";

/// The state and events from `text`, given in pieces `cut` long.
fn cut(text: &[u8], mut cut: impl FnMut() -> usize) -> (TerminalCore, Vec<Event>) {
    let mut core = TerminalCore::new(Size { cols: 20, rows: 6 }, Some("n1"));
    let mut events = Vec::new();
    let mut rest = text;
    while !rest.is_empty() {
        let (piece, after) = rest.split_at(cut().min(rest.len()));
        events.extend(core.interpret(piece));
        rest = after;
    }
    (core, events)
}

#[test]
fn the_same_bytes_give_the_same_screen_wherever_they_are_cut() {
    let whole = cut(SAMPLE, || usize::MAX);
    for at in 1..SAMPLE.len() {
        let mut pieces = [at, usize::MAX].into_iter();
        let (core, events) = cut(SAMPLE, || pieces.next().unwrap());
        assert!(core.terminal == whole.0.terminal, "cut at {at}");
        assert_eq!(events, whole.1, "cut at {at}");
    }
}

#[test]
fn the_same_bytes_give_the_same_screen_however_they_are_split() {
    // What the `split` fuzz target checks, on output made up here.
    for seed in 1..=50 {
        let mut random = Random::new(seed);
        let output = [SAMPLE, &hostile(&mut random, 5_000)].concat();
        fuzz::split(&fuzz::input(Size { cols: 20, rows: 6 }, seed, &output));
    }
}

#[test]
fn a_character_cut_in_two_loses_nothing_around_it() {
    // vte 0.15 alone drops the byte after a character it finishes, when another follows.
    for (first, second, row) in [
        (&b"\xd0"[..], &b"\xb9 \xd0\xbc"[..], "\u{439} \u{43c}"),
        (b"ab\xc3", b"\xa9\r\xc3\xa9", "\u{e9}b\u{e9}"),
        (b"caf\xc3", b"\xa9 \xc2\x85x", "caf\u{e9} x"),
        (b"\xe2\x94", b"\x80\x08\xe2\x94\x82", "\u{2502}"),
        (
            b"\xf0\x9f",
            b"\x98\x80 \xf0\x9f\x98\x80",
            "\u{1f600} \u{1f600}",
        ),
        (b"\xc3", b"\xa9\x1b[2J\x1b[H\xc3\xa9", "\u{e9}"),
        (b"\xc3", b"x", "\u{fffd}x"),
    ] {
        let mut core = core(10, 2, first);
        core.interpret(second);
        assert_eq!(rows(&core)[0], row, "{first:?} {second:?}");
        assert!(core.terminal == self::core(10, 2, &[first, second].concat()).terminal);
    }
}

#[test]
fn resizing_to_the_same_size_changes_nothing() {
    let mut core = core(10, 3, b"\x1b[2;3r0123456789");
    core.resize(Size { cols: 10, rows: 3 });
    // The wrap is still pending, and the region is as the program set it.
    core.interpret(b"x\x1b[3H\n");
    assert_eq!(rows(&core), ["0123456789", "", ""]);
}

#[test]
fn hostile_output_never_breaks_the_rules() {
    // What the `interpret` fuzz target checks, on output made up here.
    let mut random = Random::new(0x0016_0016);
    for (cols, rows) in [(0, 0), (1, 1), (1, 2), (2, 1), (3, 5), (80, 24)] {
        for seed in 1..=10 {
            let output = hostile(&mut random, 8_000);
            fuzz::interpret(&fuzz::input(Size { cols, rows }, seed, &output));
        }
    }
}

#[test]
fn every_input_is_one_for_the_fuzz_targets() {
    for data in [
        &b""[..],
        b"\x50",
        b"\x50\x18\x01",
        b"\xff\xff\xff\xff\xff\xff\xff\xff\xff\xff\x1b[6n",
    ] {
        fuzz::interpret(data);
        fuzz::split(data);
    }
}

#[test]
fn a_terminal_fits_on_a_small_stack() {
    // The parser is built on the stack before it is boxed.
    let size = std::thread::Builder::new()
        .stack_size(1 << 20)
        .spawn(|| TerminalCore::new(Size { cols: 80, rows: 24 }, None).size())
        .expect("a thread")
        .join()
        .expect("no overflow");
    assert_eq!(size, Size { cols: 80, rows: 24 });
}

/// PERF-06: plain text at 25 MB/s or more. Run with
/// `cargo test --release -p terminal-core -- --ignored`.
#[test]
#[ignore = "a measurement, for release builds"]
fn plain_text_is_read_at_25_mb_a_second() {
    let line = [&[b'x'; 78][..], b"\r\n"].concat();
    let chunk: Vec<u8> = line.iter().copied().cycle().take(64 * 1024).collect();
    let mut core = TerminalCore::new(Size { cols: 80, rows: 24 }, None);
    let total = 100 * 1024 * 1024;
    let start = std::time::Instant::now();
    for _ in 0..total / chunk.len() {
        core.interpret(&chunk);
    }
    let seconds = start.elapsed().as_secs_f64();
    let rate = total as f64 / seconds / 1_000_000.0;
    std::println!("{rate:.0} MB/s");
    assert!(rate >= 25.0, "{rate:.1} MB/s");
}
