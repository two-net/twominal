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
use crate::{
    find, osc, Cell, Clipboard, Color, Event, LinkId, Place, Report, Search, Selecting, Selection,
    Size, TerminalCore, ThemeColor, QUERY,
};

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
            // rows, where xterm prints them all. The state must not show it, but for the rows of
            // the character the cut leaves out of the scrollback.
            let fit = cols as usize / width(char.chars().next().unwrap()).max(1);
            let counts = [
                1, 2, 7, 8, 9, 23, 26, 32, 33, 34, 39, 47, 48, 49, 50, 55, 56, 57, 87, 88, 89, 90,
                95, 96, 97, 100, 333,
            ];
            for count in counts {
                let prefix = [&text[..], modes].concat();
                let mut repeated = core(
                    cols,
                    rows,
                    &[&prefix[..], format!("{char}\x1b[{count}b").as_bytes()].concat(),
                );
                let mut printed = core(
                    cols,
                    rows,
                    &[&prefix[..], char.repeat(count + 1).as_bytes()].concat(),
                );
                if fit > 0 && count > fit * (2 * usize::from(rows) + 1) {
                    repeated.terminal.forget_scrollback();
                    printed.terminal.forget_scrollback();
                }
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

// Operating system commands (TERM-12, TERM-13)

/// What `bytes` gave a terminal of 10 × 2, but replies.
fn events(bytes: &[u8]) -> Vec<Event> {
    TerminalCore::new(Size { cols: 10, rows: 2 }, None).interpret(bytes)
}

/// The links of the screen's first row, cell by cell.
fn links(core: &TerminalCore) -> Vec<Option<LinkId>> {
    core.screen().rows()[0]
        .cells()
        .iter()
        .map(Cell::link)
        .collect()
}

#[test]
fn text_past_the_sixteenth_parameter_is_lost() {
    // vte keeps 16 parameters, so a title keeps 15 of its parts, and a link that may have lost
    // part of its URI is not made.
    let parts: Vec<String> = (0..20).map(|part| part.to_string()).collect();
    let title = parts.join(";");
    assert_eq!(
        events(format!("\x1b]2;{title}\x07").as_bytes()),
        [Event::Title(Some(parts[..15].join(";")))]
    );
    let core = core(
        10,
        1,
        format!("\x1b]8;;{title}\x07x\x1b]8;;a;b\x07y").as_bytes(),
    );
    assert_eq!(links(&core)[0], None);
    assert_eq!(core.link(links(&core)[1].unwrap()).unwrap().uri(), "a;b");
}

#[test]
fn a_title_keeps_its_first_1024_characters() {
    let title = "é".repeat(1100);
    let [Event::Title(Some(kept))] = &events(format!("\x1b]2;{title}\x07").as_bytes())[..] else {
        panic!("one title");
    };
    assert_eq!(kept.chars().count(), 1024);
}

#[test]
fn an_osc_that_fills_the_buffer_is_dropped_and_what_follows_is_read() {
    // A clipboard cut short would hold part of what the program sent.
    let data = "aGVs".repeat(crate::REPORT_LIMIT / 4);
    for osc in [
        format!("52;c;{data}"),
        format!("2;{data}"),
        format!("7;file:///{data}"),
    ] {
        let mut core = core(10, 1, b"");
        let text = format!("\x1b]{osc}\x07after\x07");
        assert_eq!(core.interpret(text.as_bytes()), [Event::Bell], "{osc:.10}");
        assert_eq!(rows(&core), ["after"]);
    }
    // A link cut short is not made, and ends the one open, as any link that cannot be.
    let core = core(
        10,
        1,
        format!("\x1b]8;;u\x07a\x1b]8;;{data}\x07b").as_bytes(),
    );
    assert!(links(&core)[0].is_some());
    assert_eq!(links(&core)[1], None);
    // One just short of it arrives whole.
    let data = "aGVs".repeat(crate::REPORT_LIMIT / 4 - 2);
    let [Event::Clipboard(clipboard)] = &events(format!("\x1b]52;c;{data}\x07").as_bytes())[..]
    else {
        panic!("one write");
    };
    let Clipboard::Write { text, .. } = &**clipboard else {
        panic!("a write");
    };
    assert_eq!(text.len(), data.len() / 4 * 3);
}

#[test]
fn base64_reads_back_what_it_writes() {
    let mut random = Random::new(52);
    for length in 0..40 {
        let bytes: Vec<u8> = (0..length).map(|_| random.number() as u8).collect();
        let text = osc::encode(&bytes);
        assert_eq!(text.len() % 4, 0);
        assert_eq!(osc::decode(text.as_bytes()), Some(bytes.clone()), "{text}");
        // Without its padding too.
        let bare = text.trim_end_matches('=');
        assert_eq!(osc::decode(bare.as_bytes()), Some(bytes), "{bare}");
    }
    assert_eq!(osc::encode(b"hello world"), "aGVsbG8gd29ybGQ=");
    for bad in [
        &b"a"[..],
        b"abcde",
        b"a=",
        b"ab=",
        b"abc==",
        b"a===",
        b"ab c",
        b"ab-_",
        b"=",
        b"ab===",
    ] {
        assert_eq!(osc::decode(bad), None, "{bad:?}");
    }
}

#[test]
fn a_clipboard_answer_is_base64_whatever_the_text() {
    let [Event::Clipboard(clipboard)] = &events(b"\x1b]52;p;?\x1b\\")[..] else {
        panic!("one read");
    };
    let Clipboard::Read(query) = **clipboard else {
        panic!("a read");
    };
    assert_eq!(query.from(), Selection::Primary);
    let text = "rm -rf ~\r\x1b[2J\x07\u{9b}é";
    let reply = query.reply(text);
    let data = reply
        .strip_prefix(b"\x1b]52;p;")
        .and_then(|rest| rest.strip_suffix(b"\x1b\\"))
        .expect("an OSC 52 ended by ST");
    assert_eq!(osc::decode(data).unwrap(), text.as_bytes());
    assert!(data
        .iter()
        .all(|byte| byte.is_ascii_alphanumeric() || b"+/=".contains(byte)));
}

#[test]
fn a_color_answer_is_the_color_set_or_else_the_theme() {
    let answers = |bytes: &[u8]| -> Vec<Vec<u8>> {
        events(bytes)
            .into_iter()
            .map(|event| match event {
                Event::Color(query) => query.reply([0x12, 0xab, 0xff]),
                other => panic!("not a question about a color: {other:?}"),
            })
            .collect()
    };
    assert_eq!(
        answers(b"\x1b]4;7;?\x07\x1b]10;?\x1b\\\x1b]4;7;#010203\x07\x1b]4;7;?\x07"),
        [
            b"\x1b]4;7;rgb:1212/abab/ffff\x07".to_vec(),
            b"\x1b]10;rgb:1212/abab/ffff\x1b\\".to_vec(),
            b"\x1b]4;7;rgb:0101/0202/0303\x07".to_vec(),
        ]
    );
    // What was set when the program asked, not what was set after.
    assert_eq!(
        answers(b"\x1b]11;?\x07\x1b]11;#ffffff\x07\x1b]11;?\x07\x1b]111\x07"),
        [
            b"\x1b]11;rgb:1212/abab/ffff\x07".to_vec(),
            b"\x1b]11;rgb:ffff/ffff/ffff\x07".to_vec(),
        ]
    );
    let core = core(10, 1, b"\x1b]4;200;rgb:ffff/8000/0\x07\x1b]12;#00ff00\x07");
    assert_eq!(
        core.colors().get(ThemeColor::Palette(200)),
        Some([0xff, 0x80, 0])
    );
    assert_eq!(core.colors().get(ThemeColor::Cursor), Some([0, 0xff, 0]));
    assert_eq!(core.colors().get(ThemeColor::Palette(1)), None);
    assert_eq!(core.colors().get(ThemeColor::Foreground), None);
}

#[test]
fn a_link_with_an_id_takes_one_place_however_often_it_is_drawn() {
    // A full-screen program draws its links again and again.
    let mut core = core(10, 1, b"");
    for _ in 0..100 {
        core.interpret(b"\x1b[H\x1b]8;id=1;u\x07a\x1b]8;;\x07\x1b]8;id=2;u\x07b\x1b]8;;\x07");
    }
    // The other places are free.
    for n in 0..crate::links::MOST - 2 {
        core.interpret(format!("\x1b]8;;v{n}\x07x").as_bytes());
    }
    let links = links(&core);
    assert!(
        links[..crate::links::MOST].iter().all(Option::is_some),
        "{links:?}"
    );
    assert_ne!(links[0], links[1]);
    core.terminal.check();
}

#[test]
fn a_full_table_frees_the_links_no_cell_holds() {
    let most = crate::links::MOST;
    let mut core = core(20, 2, b"");
    for n in 0..most {
        core.interpret(format!("\x1b]8;;u{n}\x07x").as_bytes());
    }
    // Every link is on the screen, so none can be freed.
    core.interpret(b"\x1b]8;;full\x07y");
    assert_eq!(links(&core)[most], None);
    // Now no cell holds one. The sweep that found none free is not run again until links that
    // take as much as it kept have been asked for: until then the links are not made.
    core.interpret(b"\x1b[2J\x1b[H");
    for n in 1..most {
        core.interpret(format!("\x1b]8;;w{n}\x07z").as_bytes());
        assert_eq!(links(&core)[n - 1], None, "link {n}");
    }
    core.interpret(b"\x1b]8;;last\x07z");
    let made = links(&core)[most - 1].expect("a link once the sweep ran");
    assert_eq!(core.link(made).unwrap().uri(), "last");
    core.terminal.check();
}

#[test]
fn links_are_counted_by_what_they_take() {
    // In tests the table holds three of the longest links.
    let uri = "u".repeat(crate::links::URI_MOST - 1);
    let id = "i".repeat(crate::links::ID_MOST);
    let mut core = core(10, 1, b"");
    for n in 0..4 {
        core.interpret(format!("\x1b]8;id={id};{uri}{n}\x07x").as_bytes());
    }
    assert_eq!(
        core.link(links(&core)[2].unwrap()).unwrap().id(),
        Some(&*id)
    );
    assert_eq!(links(&core)[3], None);
    // One byte longer, and none is made.
    let core = self::core(
        10,
        1,
        format!("\x1b]8;;{uri}uu\x07x\x1b]8;id={id}i;u\x07y").as_bytes(),
    );
    assert_eq!(links(&core)[..2], [None, None]);
    core.terminal.check();
}

// Resizing and the scrollback (TERM-14, TERM-16)

/// The scrollback's rows, without their trailing blanks.
fn scrollback(core: &TerminalCore) -> Vec<String> {
    let scrollback = core.scrollback();
    (0..scrollback.len())
        .map(|index| scrollback.get(index).unwrap().text().trim_end().to_string())
        .collect()
}

#[test]
fn narrowing_wraps_lines_again_and_widening_joins_them() {
    let mut core = core(10, 2, b"abcdefghij");
    core.resize(Size { cols: 5, rows: 2 });
    assert_eq!(rows(&core), ["abcde", "fghij"]);
    assert!(core.screen().rows()[0].wrapped());
    // The cursor waits to wrap after the j, where it was.
    assert_eq!(cursor(&core), (1, 4));
    core.interpret(b"k");
    assert_eq!(rows(&core), ["fghij", "k"]);
    assert_eq!(scrollback(&core), ["abcde"]);

    core.resize(Size { cols: 8, rows: 3 });
    assert_eq!(rows(&core), ["abcdefgh", "ijk", ""]);
    assert!(core.scrollback().is_empty());
    assert_eq!(cursor(&core), (1, 3));
    assert_eq!(core.screen().rows()[0].cells().len(), 8);
    core.terminal.check();
}

#[test]
fn fewer_rows_go_into_the_scrollback_and_more_come_back() {
    let mut core = core(5, 4, &numbered(4));
    core.resize(Size { cols: 5, rows: 2 });
    assert_eq!(rows(&core), ["3", "4"]);
    assert_eq!(scrollback(&core), ["1", "2"]);
    assert_eq!(cursor(&core), (1, 1));
    core.resize(Size { cols: 5, rows: 4 });
    assert_eq!(rows(&core), ["1", "2", "3", "4"]);
    assert!(core.scrollback().is_empty());
    assert_eq!(cursor(&core), (3, 1));
}

#[test]
fn rows_below_the_cursor_that_no_longer_fit_are_lost() {
    let mut core = core(5, 4, &numbered(4));
    core.interpret(b"\x1b[2H");
    core.resize(Size { cols: 5, rows: 2 });
    assert_eq!(rows(&core), ["2", "3"]);
    assert_eq!(scrollback(&core), ["1"]);
    assert_eq!(cursor(&core), (0, 0));
}

#[test]
fn a_screen_with_blank_rows_below_keeps_its_top() {
    // As after Ctrl+L: the old rows stay in the scrollback.
    let mut core = core(5, 3, &numbered(5));
    core.interpret(b"\x1b[H\x1b[2Jtop");
    core.resize(Size { cols: 5, rows: 5 });
    assert_eq!(rows(&core), ["top", "", "", "", ""]);
    assert_eq!(scrollback(&core), ["1", "2"]);
    assert_eq!(cursor(&core), (0, 3));
}

#[test]
fn the_primary_screen_keeps_its_rows_while_the_alternate_is_shown() {
    let mut core = core(5, 4, &numbered(4));
    core.interpret(b"\x1b[?1049h\x1b[Hvim");
    core.resize(Size { cols: 5, rows: 2 });
    assert_eq!(rows(&core), ["vim", ""]);
    core.interpret(b"\x1b[?1049l");
    assert_eq!(rows(&core), ["3", "4"]);
    assert_eq!(scrollback(&core), ["1", "2"]);
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
fn a_wide_character_that_would_start_in_the_last_column_starts_the_next_row() {
    let mut core = core(6, 2, "ab日本".as_bytes());
    core.resize(Size { cols: 5, rows: 2 });
    assert_eq!(rows(&core), ["ab日", "本"]);
    // The last column of the first row is not part of the line.
    let first = &core.screen().rows()[0];
    assert!(first.wrapped());
    assert_eq!(first.line().len(), 4);
    core.resize(Size { cols: 6, rows: 2 });
    assert_eq!(rows(&core), ["ab日本", ""]);
    core.terminal.check();
}

#[test]
fn a_grid_one_column_wide_drops_wide_characters() {
    let mut core = core(4, 3, "a日b".as_bytes());
    core.resize(Size { cols: 1, rows: 3 });
    assert_eq!(rows(&core), ["a", "b", ""]);
    core.terminal.check();
}

#[test]
fn resizing_the_alternate_screen_cuts_and_pads_it_in_the_default_style() {
    // The window changed the size, not the program, so what it adds has no colors.
    let mut core = core(3, 1, "\x1b[?1049h\x1b[44m\x1b[2Ja日".as_bytes());
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
fn resizes_keep_the_text_of_every_line() {
    // Random lines, some wide characters among them, through random resizes: joined, every
    // line has the text it had, while the scrollback has room for them all (REL-03).
    let mut random = Random::new(0x0026_0026);
    let alphabet = ["a", "b", " ", "日", "é", "x"];
    for _ in 0..200 {
        let mut core = TerminalCore::new(Size { cols: 7, rows: 4 }, None);
        let mut lines = Vec::new();
        for _ in 0..random.below(12) {
            let line: String = (0..random.below(20))
                .map(|_| alphabet[random.below(alphabet.len())])
                .collect();
            lines.push(line.trim_end().to_string());
        }
        core.interpret(lines.join("\r\n").as_bytes());
        while lines.last().is_some_and(String::is_empty) {
            lines.pop();
        }
        for _ in 0..6 {
            let size = Size {
                cols: 1 + random.below(12) as u16,
                rows: 1 + random.below(6) as u16,
            };
            core.resize(size);
            core.terminal.check();
            // A grid one column wide drops wide characters.
            if size.cols == 1 {
                break;
            }
            assert_eq!(logical_lines(&core), lines, "{size:?}");
        }
    }
}

/// The lines of the scrollback and the primary screen, each row's line joined to the next where
/// it wraps, without trailing blanks or the screen's blank rows at the end.
fn logical_lines(core: &TerminalCore) -> Vec<String> {
    let scrollback = core.scrollback();
    let rows = (0..scrollback.len())
        .map(|index| scrollback.get(index).unwrap())
        .chain(core.screen().rows());
    let mut lines = Vec::new();
    let mut line = String::new();
    for row in rows {
        line.extend(row.line().iter().flat_map(|cell| cell.chars()));
        if !row.wrapped() {
            lines.push(line.trim_end().to_string());
            line.clear();
        }
    }
    while lines.last().is_some_and(String::is_empty) {
        lines.pop();
    }
    lines
}

#[test]
fn the_scrollback_numbers_its_rows() {
    let mut core = core(5, 2, &numbered(5));
    let scrollback = core.scrollback();
    assert_eq!((scrollback.start(), scrollback.len()), (0, 3));
    core.set_scrollback(2);
    let scrollback = core.scrollback();
    assert_eq!((scrollback.start(), scrollback.len()), (1, 2));
    assert_eq!(self::scrollback(&core), ["2", "3"]);
    // Rows dropped past the limit, or emptied, keep the numbers of the rest.
    core.interpret(b"\r\n6");
    assert_eq!(core.scrollback().start(), 2);
    core.clear();
    assert_eq!((core.scrollback().start(), core.scrollback().len()), (4, 0));
    core.set_scrollback(1_000_000);
    assert_eq!(core.scrollback().limit(), 100_000);
}

#[test]
fn the_scrollback_keeps_short_rows_in_little_memory() {
    // PERF-10: a row is kept without the blanks at the end of its line, in no more memory than
    // its cells take. 32-byte cells: 10,000 full rows of 80 columns take 25.6 MB.
    assert_eq!(core::mem::size_of::<crate::Cell>(), 32);
    let mut core = TerminalCore::new(Size { cols: 80, rows: 24 }, None);
    let line = [&[b'x'; 40][..], b"\r\n"].concat();
    core.interpret(&line.repeat(10_100));
    let scrollback = core.scrollback();
    assert_eq!(scrollback.len(), 10_000);
    for index in 0..scrollback.len() {
        let row = scrollback.get(index).unwrap();
        assert_eq!(row.cells().len(), 40);
        assert_eq!(row.capacity(), 40);
    }
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
    b"\x1b]0;",
    b"\x1b]2;",
    b"\x1b]7;file://h/",
    b"\x1b]8;id=a;",
    b"\x1b]8;;",
    b"\x1b]8;;u\x07",
    b"\x1b]52;c;",
    b"\x1b]52;p;?\x07",
    b"\x1b]4;1;",
    b"\x1b]10;?;?",
    b"\x1b]11;",
    b"rgb:ff/00/00",
    b"#123",
    b"\x1b]104",
    b"\x1b]111",
    b"\x1b[22;2t",
    b"\x1b[23;2t",
    b"aGk=",
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
                       \x1b[3b\tx\x1b[c\x1b]2222;n1;ready\x07\xd0\xb9 \xd0\xbc\xf0\x9f\x98\x80!\
                       \x1b]2;a;title\x1b\\\x1b[22;2t\x07\x1b]8;id=1;https://e/x\x07link\x1b]8;;\x1b\\\
                       \x1b]11;?\x1b\\\x1b]4;1;#ff0000\x07\x1b]52;c;aGk=\x07\x1b]7;file://h/tmp\x07\
                       \x1b]2;cut\x1b[1m\x1b[23;2t\x1b]104\x07\x1b]52;p;?\x07";

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
fn a_selection_ends_or_stays_the_same_however_the_output_is_cut() {
    // The selection is in the scrollback and on the screen's first row, which the output scrolls
    // up a row at a time without writing on them.
    let output =
        b"\r\none\r\ntwo\r\nthree\r\n\xe4\xb8\xad\xe6\x96\x87 \x1b[31mred\x1b[m\r\nmore\r\n";
    for by in [
        Selecting::Characters,
        Selecting::Words,
        Selecting::Lines,
        Selecting::Block,
    ] {
        let mut whole = core(6, 3, b"a b c\r\nd e f\r\ng h i\r\nj k l");
        let at = |row, col| Place {
            row,
            col,
            right_half: false,
        };
        whole.select(at(0, 2), by);
        whole.extend(at(1, 4));
        let mut bytewise = core(6, 3, b"a b c\r\nd e f\r\ng h i\r\nj k l");
        bytewise.select(at(0, 2), by);
        bytewise.extend(at(1, 4));
        whole.interpret(output);
        for byte in output {
            bytewise.interpret(&[*byte]);
        }
        assert!(whole.terminal == bytewise.terminal, "{by:?}");
        assert!(whole.copy().is_some(), "{by:?}");
    }
}

#[test]
fn a_place_anywhere_is_taken_to_the_nearest() {
    let mut core = core(4, 2, b"ab\r\ncd\r\nef");
    let far = |row, col| Place {
        row,
        col,
        right_half: true,
    };
    for by in [
        Selecting::Characters,
        Selecting::Words,
        Selecting::Lines,
        Selecting::Block,
    ] {
        core.select(far(i64::MIN, i32::MIN), by);
        core.extend(far(i64::MAX, i32::MAX));
        core.terminal.check();
        assert_eq!(core.copy().as_deref(), Some("ab\ncd\nef"), "{by:?}");
    }
}

#[test]
fn reading_the_selection_changes_nothing() {
    let make = || {
        let mut core = core(4, 2, b"a\r\nb\r\nc\r\nd");
        core.select(
            Place {
                row: 0,
                col: 0,
                right_half: false,
            },
            Selecting::Lines,
        );
        core.extend(Place {
            row: 4,
            col: 0,
            right_half: false,
        });
        // Its first rows are dropped, and cut from it as it is read.
        core.set_scrollback(1);
        core
    };
    let (read, unread) = (make(), make());
    assert_eq!(read.copy().as_deref(), Some("b\nc\nd"));
    assert_eq!(read.selected().map(|selected| selected.rows()), Some(1..=3));
    assert!(read.terminal == unread.terminal);
}

#[test]
fn rows_are_found_by_their_numbers_on_the_screen_shown() {
    let mut core = core(4, 2, b"a\r\nb\r\nc");
    assert_eq!(core.row(0).map(|row| row.text()).as_deref(), Some("a"));
    assert_eq!(core.row(2).map(|row| row.text()).as_deref(), Some("c   "));
    assert!(core.row(3).is_none());
    // The alternate screen has no scrollback above it.
    core.interpret(b"\x1b[?1049h\x1b[Hx");
    assert!(core.row(0).is_none());
    assert_eq!(core.row(1).map(|row| row.text()).as_deref(), Some("x   "));
    core.set_scrollback(0);
    assert!(core.row(0).is_none());
}

// Finding

/// A search for `query`, as plain text or as a regular expression.
fn search(query: &str, regex: bool) -> Option<Search> {
    Some(Search {
        query: query.to_string(),
        case: false,
        regex,
    })
}

/// The screen's last row, by number: where find starts from a view at the bottom.
fn last_row(core: &TerminalCore) -> u64 {
    let scrollback = core.scrollback();
    scrollback.start() + scrollback.len() as u64 + u64::from(core.size().rows) - 1
}

/// The matches `core` has found, up to date, as their rows and the columns of each row.
fn matches(core: &mut TerminalCore) -> Vec<(u64, core::ops::Range<usize>)> {
    core.refind();
    let found = core.found().expect("a search");
    let scrollback = core.scrollback();
    let last = last_row(core);
    (scrollback.start()..=last)
        .flat_map(|row| found.cols(row).map(move |(cols, _)| (row, cols)))
        .collect()
}

#[test]
fn find_keeps_the_newest_matches_and_says_there_are_more() {
    let mut core = core(4, 2, &numbered(12));
    core.find(search("[0-9]+", true), last_row(&core));
    let found = core.found().expect("a search");
    assert_eq!((found.count(), found.more()), (find::MOST, true));
    // The newest: lines 5 to 12, on rows 4 to 11.
    assert_eq!(matches(&mut core).first(), Some(&(4, 0..1)));
    // Two of them go, so two older ones come back.
    core.interpret(b"\x1b[2J");
    assert_eq!(matches(&mut core).first(), Some(&(2, 0..1)));
    let found = core.found().expect("a search");
    assert_eq!((found.count(), found.more()), (find::MOST, true));
    // With no more to come back, there are no more.
    core.interpret(b"\x1b[3J");
    core.refind();
    let found = core.found().expect("a search");
    assert_eq!((found.count(), found.more()), (0, false));
}

#[test]
fn the_older_matches_left_out_can_go_while_the_newest_stay() {
    // Found by the fuzzer: the two left out are erased, and the eight kept do not change.
    let mut core = core(4, 10, &numbered(10));
    core.find(search("[0-9]+", true), last_row(&core));
    let found = core.found().expect("a search");
    assert_eq!((found.count(), found.more()), (find::MOST, true));
    core.interpret(b"\x1b[H\x1b[2K\x1b[2;1H\x1b[2K");
    core.refind();
    let found = core.found().expect("a search");
    assert_eq!((found.count(), found.more()), (find::MOST, false));
    assert!(core.terminal.found_fresh());
}

#[test]
fn a_long_line_is_searched_in_pieces() {
    // One line of 24 characters over 12 rows two wide: the pieces are rows 0 to 3, 4 to 7 and 8
    // to 11, as their numbers are multiples of 4 in the tests.
    let mut core = core(2, 12, b"abcdefghijklmnopqrstuvwx");
    core.find(search("fg", false), last_row(&core));
    assert_eq!(matches(&mut core), [(2, 1..2), (3, 0..1)]);
    // h is on row 3 and i on row 4.
    core.find(search("hi", false), last_row(&core));
    assert_eq!(matches(&mut core), []);
    // A piece is read with the rows beside it, so i does not start a line.
    core.find(search("^i", true), last_row(&core));
    assert_eq!(matches(&mut core), []);
    core.find(search("^a", true), last_row(&core));
    assert_eq!(matches(&mut core), [(0, 0..1)]);
    core.find(search("x$", true), last_row(&core));
    assert_eq!(matches(&mut core), [(11, 1..2)]);
}

#[test]
fn the_row_after_a_piece_reads_the_same_on_the_screen_and_after_it() {
    // Five rows of one line, the last blank: pieces of rows 0 to 3 and of row 4. The line ends in
    // row 4, whose blanks are not part of it on the screen or in the scrollback, so `$` finds h.
    let mut core = core(2, 6, b"abcdefgh  \r\n");
    core.find(search("h$", true), last_row(&core));
    assert_eq!(matches(&mut core), [(3, 1..2)]);
    core.interpret(b"\n\n\n\n\n");
    assert_eq!(matches(&mut core), [(3, 1..2)]);
    assert!(core.terminal.found_fresh());
}

#[test]
fn matches_keep_up_with_output_as_a_search_of_the_rows_finds() {
    let mut found = 0;
    for seed in 1..=40 {
        let mut random = Random::new(seed);
        let mut core = core(12, 4, b"");
        core.set_scrollback(16);
        let output = [SAMPLE, &hostile(&mut random, 3_000)].concat();
        let (query, regex) =
            [("e", false), ("[a-z]+", true), ("^.", true), (" ", false)][random.below(4)];
        core.find(search(query, regex), last_row(&core));
        for piece in output.chunks(1 + random.below(64)) {
            core.interpret(piece);
            match random.below(16) {
                0 => core.resize(Size {
                    cols: 1 + random.below(16) as u16,
                    rows: 1 + random.below(6) as u16,
                }),
                1 => core.clear(),
                2 => core.set_scrollback(random.below(20)),
                3 => core.find_next(random.below(2) == 0),
                _ => {}
            }
            if random.below(3) == 0 {
                core.refind();
                assert!(core.terminal.found_fresh(), "seed {seed}");
                core.terminal.check();
                found += core.found().map_or(0, |found| found.count());
            }
        }
    }
    // The rule was checked on matches, not only on nothing.
    assert!(found > 1_000, "{found} matches");
}

#[test]
fn reading_the_matches_changes_nothing() {
    let make = || {
        let mut core = core(4, 2, b"ab\r\nab\r\nab");
        core.find(search("ab", false), 2);
        core.interpret(b"\r\nab");
        core.refind();
        core
    };
    let (read, unread) = (make(), make());
    let found = read.found().expect("a search");
    assert_eq!((found.count(), found.current()), (4, Some(2)));
    assert_eq!(found.current_rows(), Some(2..=2));
    assert!(read.terminal == unread.terminal);
}

#[test]
fn a_query_keeps_its_first_1024_characters() {
    let text = "a".repeat(QUERY);
    // One line of four rows, which find searches as one text.
    let mut core = core(256, 4, text.as_bytes());
    core.find(search(&[text.as_str(), "b"].concat(), false), 3);
    assert_eq!(core.found().map(|found| found.count()), Some(1));
    // The longest query is a pattern, whatever its characters, in both letter cases.
    let longest: String = "aäßİσςж中".chars().cycle().take(QUERY).collect();
    core.find(search(&longest, false), 3);
    assert_eq!(core.found().map(|found| found.invalid()), Some(false));
    // A pattern too large to search is not one.
    core.find(search(r"\w{1000}", true), 3);
    assert_eq!(core.found().map(|found| found.invalid()), Some(true));
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

/// The slowest pattern find searches over the most rows the scrollback keeps, 200 wide: `\b`
/// over text that is not ASCII, which the lazy DFA leaves to the PikeVM. Run with
/// `cargo test --release -p terminal-core -- --ignored`.
#[test]
#[ignore = "a measurement, for release builds"]
fn the_slowest_pattern_searches_a_full_scrollback_in_seconds() {
    let line: String = "\u{421}\u{44a}\u{435}\u{448}\u{44c} \u{436}\u{435} \u{4e2d}\u{6587} "
        .chars()
        .cycle()
        .take(199)
        .collect();
    let line = [line.as_bytes(), b"\r\n"].concat();
    let mut core = TerminalCore::new(
        Size {
            cols: 200,
            rows: 50,
        },
        None,
    );
    core.set_scrollback(100_000);
    for _ in 0..100_050 {
        core.interpret(&line);
    }
    let start = std::time::Instant::now();
    core.find(search(r"(?:\b\w+\b.*){4}z", true), last_row(&core));
    let seconds = start.elapsed().as_secs_f64();
    std::println!("{seconds:.1} s");
    assert_eq!(core.found().map(|found| found.invalid()), Some(false));
    assert!(seconds < 30.0, "{seconds:.1} s");
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
