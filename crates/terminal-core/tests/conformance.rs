//! The Terminal Core's conformance suite (MAINT-03): byte streams in, screen state out.
//!
//! The cases are in `tests/conformance/`, one file for each area. Each expectation is what
//! xterm does (TERM-01), or a deviation that DESIGN.md §3 documents and the case's comment
//! names. Only the public interface is used, as the Tab uses it.
//!
//! A case is a script, run from top to bottom:
//!
//! ```text
//! ## ED 1 erases above the cursor, its cell included     the case's name, unique
//! # ctlseqs: CSI Ps J                                     a comment
//! size 4x3                                                a new terminal, 4 columns by 3 rows
//! send abcd\r\nefgh\r\nijkl\e[2;2H                        one call to `interpret`
//! send \e[1J
//! screen                                                  every row, at its full width; a
//! |    |                                                  `>` after a row means it wraps
//! |  gh|
//! |ijkl|
//! cursor 2;2                                              counted from 1, as CUP counts
//! reply \e[?62;22c                                        the next reply not yet checked
//! mode bracketed_paste on                                 a field of `Modes`
//! ```
//!
//! A row is the text of its cells. A wide character takes two columns of it, as it does in a
//! monospace font, and its right half adds nothing. A zero-width character is written after the
//! character it joins, as it came: `e` and U+0301, not `é`.
//!
//! `send` and `reply` take `\e`, `\a`, `\b`, `\t`, `\n`, `\r`, `\\` and `\xHH`; the rest is
//! UTF-8 as written. No line may end in a space, so a trailing space is written `\x20`.
//! Every reply must be checked: one left over at the end of a case fails it (SEC-05).
//!
//! Each case runs twice: as written, and with every `send` given a byte at a time (ARC-03).

use std::collections::{BTreeSet, VecDeque};
use std::fs;
use std::path::Path;

use terminal_core::{Event, Modes, MouseTracking, Size, TerminalCore};

#[test]
fn every_case_passes() {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/conformance");
    let mut files: Vec<_> = fs::read_dir(&dir)
        .unwrap_or_else(|error| panic!("{}: {error}", dir.display()))
        .map(|entry| entry.unwrap().path())
        .filter(|path| path.extension().is_some_and(|extension| extension == "txt"))
        .collect();
    files.sort();

    let mut problems = Vec::new();
    let mut names = BTreeSet::new();
    let mut count = 0;
    for path in files {
        let file = format!(
            "tests/conformance/{}",
            path.file_name().unwrap().to_string_lossy()
        );
        let text = fs::read_to_string(&path).unwrap();
        for case in parse(&file, &text, &mut problems) {
            if !names.insert(case.name.clone()) {
                problems.push(format!(
                    "{file}:{}: a second case named {:?}",
                    case.line, case.name
                ));
            }
            count += 1;
            problems.extend(check(&file, &case));
        }
    }

    assert!(count > 0, "no cases in {}", dir.display());
    assert!(
        problems.is_empty(),
        "{} of the conformance suite's {count} cases fail:\n\n{}\n",
        problems.len(),
        problems.join("\n\n")
    );
}

// ---------------------------------------------------------------------------------------
// Cases
// ---------------------------------------------------------------------------------------

struct Case {
    /// The line of its `##`.
    line: usize,
    name: String,
    size: Size,
    steps: Vec<Step>,
}

struct Step {
    line: usize,
    kind: Kind,
}

enum Kind {
    Send(Vec<u8>),
    Screen(Vec<Row>),
    Cursor(u16, u16),
    Reply(Vec<u8>),
    Mode(String, String),
}

/// A row of the screen as a case draws it.
#[derive(PartialEq)]
struct Row {
    text: String,
    wrapped: bool,
}

/// The fields of `Modes` a case can name, with the words their values are written in.
const MODES: &[&str] = &[
    "cursor_visible",
    "alternate_screen",
    "application_cursor",
    "application_keypad",
    "newline",
    "bracketed_paste",
    "focus",
    "mouse",
    "mouse_sgr",
];

/// The cases in `text`. What cannot be read goes to `problems`, as `file:line: why`, and
/// the case it was in is dropped, so that nothing is silently left unchecked.
fn parse(file: &str, text: &str, problems: &mut Vec<String>) -> Vec<Case> {
    let mut cases = Vec::new();
    let mut case: Option<Case> = None;
    let mut broken = false;
    let mut lines = text.lines().enumerate().map(|(at, line)| (at + 1, line));

    while let Some((line, content)) = lines.next() {
        let mut problem = |why: String| problems.push(format!("{file}:{line}: {why}"));
        if content.ends_with(char::is_whitespace) {
            problem("ends in a space; write a trailing space as \\x20".to_string());
            broken = true;
            continue;
        }
        if let Some(name) = content.strip_prefix("## ") {
            cases.extend(case.take().filter(|_| !broken));
            broken = false;
            case = Some(Case {
                line,
                name: name.to_string(),
                size: Size { cols: 0, rows: 0 },
                steps: Vec::new(),
            });
            continue;
        }
        if content.is_empty() || content.starts_with("# ") || content == "#" {
            continue;
        }
        let Some(current) = case.as_mut() else {
            problem(format!("{content:?} before the first case"));
            continue;
        };
        let (keyword, rest) = content.split_once(' ').unwrap_or((content, ""));
        let sized = current.size.cols > 0;
        if (keyword == "size") == sized {
            problem(if sized {
                "a second `size`".to_string()
            } else {
                "a case starts with `size`".to_string()
            });
            broken = true;
            continue;
        }
        let kind = match keyword {
            "size" => match parse_size(rest) {
                Some(size) => {
                    current.size = size;
                    continue;
                }
                None => Err(format!("{rest:?} is not a size such as 80x24")),
            },
            "send" if rest.is_empty() => Err("`send` with nothing to send".to_string()),
            "send" => unescape(rest).map(Kind::Send),
            "reply" if rest.is_empty() => Err("`reply` with no reply".to_string()),
            "reply" => unescape(rest).map(Kind::Reply),
            "cursor" => parse_cursor(rest)
                .map(|(row, col)| Kind::Cursor(row, col))
                .ok_or(format!("{rest:?} is not a position such as 3;5")),
            "mode" => match rest.split_once(' ') {
                Some((name, value)) if MODES.contains(&name) => {
                    Ok(Kind::Mode(name.to_string(), value.to_string()))
                }
                _ => Err(format!("{rest:?} is not one of {MODES:?} and a value")),
            },
            "screen" if rest.is_empty() => {
                let mut rows = Vec::new();
                let mut why = None;
                for _ in 0..current.size.rows {
                    match lines.next() {
                        Some((at, row)) => match parse_row(row) {
                            Ok(row) => rows.push(row),
                            Err(error) => {
                                why.get_or_insert(format!("row on line {at}: {error}"));
                            }
                        },
                        None => {
                            why.get_or_insert("the file ends inside a screen".to_string());
                        }
                    }
                }
                why.map_or(Ok(Kind::Screen(rows)), Err)
            }
            _ => Err(format!("{keyword:?} is not something a case can say")),
        };
        match kind {
            Ok(kind) => current.steps.push(Step { line, kind }),
            Err(why) => {
                problem(why);
                broken = true;
            }
        }
    }
    cases.extend(case.filter(|_| !broken));

    for case in &cases {
        if case.steps.is_empty() {
            problems.push(format!(
                "{file}:{}: {:?} checks nothing",
                case.line, case.name
            ));
        }
    }
    cases
}

fn parse_size(text: &str) -> Option<Size> {
    let (cols, rows) = text.split_once('x')?;
    let size = Size {
        cols: cols.parse().ok()?,
        rows: rows.parse().ok()?,
    };
    (size.cols > 0 && size.rows > 0).then_some(size)
}

fn parse_cursor(text: &str) -> Option<(u16, u16)> {
    let (row, col) = text.split_once(';')?;
    Some((row.parse().ok()?, col.parse().ok()?))
}

/// A row drawn as `|`, its text and `|`, then `>` if it wraps. A `|` inside the row is a
/// character like any other: only the last one ends it. How wide the text is depends on its
/// characters, so it is not counted here; a row of the wrong width fails as the screen differs.
fn parse_row(text: &str) -> Result<Row, String> {
    let (inside, wrapped) = match text.strip_suffix("|>") {
        Some(inside) => (inside, true),
        None => (text.strip_suffix('|').unwrap_or(""), false),
    };
    match inside.strip_prefix('|') {
        Some(inside) => Ok(Row {
            text: inside.to_string(),
            wrapped,
        }),
        None => Err(format!("{text:?} is not a row between two |")),
    }
}

/// The bytes `text` stands for.
fn unescape(text: &str) -> Result<Vec<u8>, String> {
    let mut bytes = Vec::new();
    let mut chars = text.chars();
    while let Some(char) = chars.next() {
        if char != '\\' {
            bytes.extend_from_slice(char.encode_utf8(&mut [0; 4]).as_bytes());
            continue;
        }
        bytes.push(match chars.next() {
            Some('e') => 0x1b,
            Some('a') => 0x07,
            Some('b') => 0x08,
            Some('t') => b'\t',
            Some('n') => b'\n',
            Some('r') => b'\r',
            Some('\\') => b'\\',
            Some('x') => {
                let digits: String = chars.by_ref().take(2).collect();
                match u8::from_str_radix(&digits, 16) {
                    Ok(byte) if digits.len() == 2 => byte,
                    _ => return Err(format!("\\x{digits} is not \\x and two hex digits")),
                }
            }
            Some(other) => return Err(format!("\\{other} is not an escape")),
            None => return Err("ends in a lone \\".to_string()),
        });
    }
    Ok(bytes)
}

/// `bytes` as a case writes them.
fn escape(bytes: &[u8]) -> String {
    let mut text = String::new();
    for chunk in bytes.utf8_chunks() {
        for char in chunk.valid().chars() {
            match char {
                '\x1b' => text.push_str("\\e"),
                '\\' => text.push_str("\\\\"),
                ' '..='~' => text.push(char),
                char if char.is_control() => {
                    let mut buffer = [0; 4];
                    for byte in char.encode_utf8(&mut buffer).bytes() {
                        text.push_str(&format!("\\x{byte:02x}"));
                    }
                }
                char => text.push(char),
            }
        }
        for byte in chunk.invalid() {
            text.push_str(&format!("\\x{byte:02x}"));
        }
    }
    text
}

// ---------------------------------------------------------------------------------------
// Running a case
// ---------------------------------------------------------------------------------------

/// What is wrong with `case`, as written and then a byte at a time. The second run is only
/// reported when the first passes, so one fault is told once.
fn check(file: &str, case: &Case) -> Vec<String> {
    let problems = run(case, false);
    if !problems.is_empty() {
        return report(file, case, "", problems);
    }
    report(file, case, " (sent a byte at a time)", run(case, true))
}

fn report(file: &str, case: &Case, how: &str, problems: Vec<(usize, String)>) -> Vec<String> {
    problems
        .into_iter()
        .map(|(line, why)| format!("{file}:{line}: {}{how}: {why}", case.name))
        .collect()
}

/// The lines whose expectations `case` does not meet, and why.
fn run(case: &Case, bytewise: bool) -> Vec<(usize, String)> {
    let mut core = TerminalCore::new(case.size, None);
    let mut replies = VecDeque::new();
    let mut problems = Vec::new();
    let mut last = case.line;

    for step in &case.steps {
        last = step.line;
        let mut problem = |why: String| problems.push((step.line, why));
        match &step.kind {
            Kind::Send(bytes) => {
                let pieces: Vec<&[u8]> = if bytewise {
                    bytes.chunks(1).collect()
                } else {
                    vec![bytes]
                };
                for piece in pieces {
                    for event in core.interpret(piece) {
                        match event {
                            Event::Reply(reply) => replies.push_back(reply),
                            other => problem(format!("an event that is not a reply: {other:?}")),
                        }
                    }
                }
            }
            Kind::Screen(expected) => {
                let actual = screen(&core);
                if &actual != expected {
                    problem(format!(
                        "the screen differs\nexpected\n{}\ngot\n{}",
                        draw(expected),
                        draw(&actual)
                    ));
                }
            }
            Kind::Cursor(row, col) => {
                let cursor = core.cursor();
                let actual = (cursor.row + 1, cursor.col + 1);
                if actual != (*row, *col) {
                    problem(format!(
                        "expected cursor {row};{col}, got cursor {};{}",
                        actual.0, actual.1
                    ));
                }
            }
            Kind::Reply(expected) => match replies.pop_front() {
                Some(actual) if &actual == expected => {}
                Some(actual) => problem(format!(
                    "expected reply {}, got reply {}",
                    escape(expected),
                    escape(&actual)
                )),
                None => problem(format!("expected reply {}, got none", escape(expected))),
            },
            Kind::Mode(name, expected) => {
                let actual = mode(core.modes(), name);
                if actual != *expected {
                    problem(format!(
                        "expected mode {name} {expected}, got mode {name} {actual}"
                    ));
                }
            }
        }
    }
    if !replies.is_empty() {
        let unchecked: Vec<String> = replies.iter().map(|reply| escape(reply)).collect();
        problems.push((
            last,
            format!("replies no line checks: {}", unchecked.join(" ")),
        ));
    }
    problems
}

fn screen(core: &TerminalCore) -> Vec<Row> {
    core.screen()
        .rows()
        .iter()
        .map(|row| Row {
            text: row.text(),
            wrapped: row.wrapped(),
        })
        .collect()
}

/// `rows` as a case draws them.
fn draw(rows: &[Row]) -> String {
    let lines: Vec<String> = rows
        .iter()
        .map(|row| format!("|{}|{}", row.text, if row.wrapped { ">" } else { "" }))
        .collect();
    lines.join("\n")
}

/// The value of the field `name` of `modes`, as a case writes it.
fn mode(modes: &Modes, name: &str) -> String {
    let on = |on: bool| if on { "on" } else { "off" }.to_string();
    match name {
        "cursor_visible" => on(modes.cursor_visible),
        "alternate_screen" => on(modes.alternate_screen),
        "application_cursor" => on(modes.application_cursor),
        "application_keypad" => on(modes.application_keypad),
        "newline" => on(modes.newline),
        "bracketed_paste" => on(modes.bracketed_paste),
        "focus" => on(modes.focus),
        "mouse_sgr" => on(modes.mouse_sgr),
        "mouse" => match modes.mouse {
            MouseTracking::Off => "off",
            MouseTracking::Press => "press",
            MouseTracking::Click => "click",
            MouseTracking::Drag => "drag",
            MouseTracking::Motion => "motion",
        }
        .to_string(),
        _ => unreachable!("parse lets only the names in MODES through"),
    }
}

// ---------------------------------------------------------------------------------------
// The suite's own rules
// ---------------------------------------------------------------------------------------

#[test]
fn escapes_read_back_as_written() {
    let bytes = unescape("a\\e[2J\\a\\b\\t\\n\\r\\\\\\x00\\xffé ").unwrap();
    assert_eq!(bytes, b"a\x1b[2J\x07\x08\t\n\r\\\x00\xff\xc3\xa9 ");
    assert_eq!(
        escape(&bytes),
        "a\\e[2J\\x07\\x08\\x09\\x0a\\x0d\\\\\\x00\\xffé "
    );
    assert_eq!(unescape(&escape(&bytes)).unwrap(), bytes);
    for bad in ["\\q", "\\x4", "\\xzz", "a\\"] {
        assert!(unescape(bad).is_err(), "{bad}");
    }
}

#[test]
fn rows_are_read_between_their_bars() {
    let row = parse_row("|a|b |>").unwrap();
    assert_eq!(row.text, "a|b ");
    assert!(row.wrapped);
    let row = parse_row("|日本||").unwrap();
    assert_eq!(row.text, "日本|");
    assert!(!row.wrapped);
    assert_eq!(parse_row("||").unwrap().text, "");
    for bad in ["abcd|", "|abcd|x", "|abcd", "|", "abcd|>"] {
        assert!(parse_row(bad).is_err(), "{bad}");
    }
}

#[test]
fn what_cannot_be_read_is_a_problem() {
    for (text, why) in [
        ("send x\n", "before the first case"),
        ("## a\nsend x\n", "a case starts with `size`"),
        ("## a\nsize 2x1\nsize 2x1\n", "a second `size`"),
        ("## a\nsize 2by1\n", "is not a size"),
        ("## a\nsize 2x1\nsend x \n", "ends in a space"),
        (
            "## a\nsize 2x1\nsned x\n",
            "is not something a case can say",
        ),
        ("## a\nsize 2x1\nsend\n", "nothing to send"),
        ("## a\nsize 2x1\nsend \\q\n", "is not an escape"),
        ("## a\nsize 2x1\ncursor 1,1\n", "is not a position"),
        ("## a\nsize 2x1\nmode mouse_mode on\n", "is not one of"),
        ("## a\nsize 2x2\nscreen\n|ab|\n", "ends inside a screen"),
        (
            "## a\nsize 2x1\nscreen\n|ab\n",
            "is not a row between two |",
        ),
        ("## a\nsize 2x1\n", "checks nothing"),
    ] {
        let mut problems = Vec::new();
        let cases = parse("f.txt", text, &mut problems);
        assert!(
            problems.iter().any(|problem| problem.contains(why)),
            "{text:?}: {problems:?}"
        );
        // A case that cannot be read is not run.
        assert!(cases.is_empty() || why == "checks nothing", "{text:?}");
    }
}

#[test]
fn a_case_fails_where_it_is_not_met() {
    let text = "## a\nsize 3x1\nsend ab\\e[6n\nscreen\n|abc|\ncursor 1;1\nreply \\e[1;1R\nmode newline on\n";
    let mut problems = Vec::new();
    let cases = parse("f.txt", text, &mut problems);
    assert!(problems.is_empty(), "{problems:?}");
    let problems = check("f.txt", &cases[0]);
    let expected = [
        "f.txt:4: a: the screen differs\nexpected\n|abc|\ngot\n|ab |",
        "f.txt:6: a: expected cursor 1;1, got cursor 1;3",
        "f.txt:7: a: expected reply \\e[1;1R, got reply \\e[1;3R",
        "f.txt:8: a: expected mode newline on, got mode newline off",
    ];
    assert_eq!(problems, expected);

    // A reply no line checks.
    let cases = parse("f.txt", "## b\nsize 3x1\nsend \\e[5n\n", &mut Vec::new());
    assert_eq!(
        check("f.txt", &cases[0]),
        ["f.txt:3: b: replies no line checks: \\e[0n"]
    );
}
