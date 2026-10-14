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
//! mode bracketed_paste on                                 a field of `Modes`: `on` or
//! mode keyboard 5                                         `off`, the mouse's tracking, or
//!                                                         the keyboard flags' number
//! style r bold foreground 1                               `r` stands for this style below
//! styles                                                  every row again, a letter for the
//! |rr..|                                                  style of each cell
//! |....|
//! |....|
//! ```
//!
//! A row is the text of its cells. A wide character takes two columns of it, as it does in a
//! monospace font, and its right half adds nothing. A zero-width character is written after the
//! character it joins, as it came: `e` and U+0301, not `é`.
//!
//! A row of styles has a letter for each cell, a wide character's right half included. `.` is
//! the default style, and `a` to `z` the styles the case named before with `style`. A style is
//! what it names and the default in the rest, in these words: `bold`, `dim`, `italic`,
//! `underline` and one of `single`, `double`, `curly`, `dotted` and `dashed`, `strikethrough`,
//! `inverse`, `hidden`, and `foreground`, `background` or `underline-color` and a color, which is
//! a number of the palette from 0 to 255 or `#rrggbb`.
//!
//! `send` and `reply` take `\e`, `\a`, `\b`, `\t`, `\n`, `\r`, `\\` and `\xHH`; the rest is
//! UTF-8 as written. No line may end in a space, so a trailing space is written `\x20`.
//! Every reply must be checked: one left over at the end of a case fails it (SEC-05).
//!
//! Each case runs twice: as written, and with every `send` given a byte at a time (ARC-03).

use std::collections::{BTreeSet, VecDeque};
use std::fs;
use std::path::Path;

use terminal_core::{Color, Event, Modes, MouseTracking, Size, Style, TerminalCore, Underline};

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
    /// The styles its letters stand for, as `style` named them.
    letters: Vec<(char, Style)>,
    steps: Vec<Step>,
}

struct Step {
    line: usize,
    kind: Kind,
}

enum Kind {
    Send(Vec<u8>),
    Screen(Vec<Row>),
    Styles(Vec<Vec<Style>>),
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
    "keyboard",
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
                letters: Vec::new(),
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
            "style" => match parse_letter(rest, &current.letters) {
                Ok(letter) => {
                    current.letters.push(letter);
                    continue;
                }
                Err(why) => Err(why),
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
            "styles" if rest.is_empty() => {
                let mut rows = Vec::new();
                let mut why = None;
                for _ in 0..current.size.rows {
                    match lines.next() {
                        Some((at, row)) => {
                            match parse_styles(row, current.size.cols, &current.letters) {
                                Ok(row) => rows.push(row),
                                Err(error) => {
                                    why.get_or_insert(format!("row on line {at}: {error}"));
                                }
                            }
                        }
                        None => {
                            why.get_or_insert("the file ends inside the styles".to_string());
                        }
                    }
                }
                why.map_or(Ok(Kind::Styles(rows)), Err)
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

/// The letter `style` names and the style it stands for, from `r bold foreground 1`. A letter
/// is named once.
fn parse_letter(text: &str, letters: &[(char, Style)]) -> Result<(char, Style), String> {
    let (letter, words) = text.split_once(' ').unwrap_or((text, ""));
    let mut chars = letter.chars();
    let letter = match (chars.next(), chars.next()) {
        (Some(letter @ 'a'..='z'), None) => letter,
        _ => return Err(format!("{letter:?} is not a letter from a to z")),
    };
    if letters.iter().any(|(named, _)| *named == letter) {
        return Err(format!("a second style named {letter}"));
    }
    Ok((letter, parse_style(words)?))
}

/// The style `text` says, in the words the start of this file lists.
fn parse_style(text: &str) -> Result<Style, String> {
    if text.is_empty() {
        return Err("a style that names nothing; the default style is `.`".to_string());
    }
    let mut style = Style::default();
    let mut words = text.split(' ');
    while let Some(word) = words.next() {
        match word {
            "bold" => style.bold = true,
            "dim" => style.dim = true,
            "italic" => style.italic = true,
            "strikethrough" => style.strikethrough = true,
            "inverse" => style.inverse = true,
            "hidden" => style.hidden = true,
            "underline" => {
                style.underline = match words.next() {
                    Some("single") => Underline::Single,
                    Some("double") => Underline::Double,
                    Some("curly") => Underline::Curly,
                    Some("dotted") => Underline::Dotted,
                    Some("dashed") => Underline::Dashed,
                    other => {
                        return Err(format!(
                            "underline {other:?} is not single, double, curly, dotted or dashed"
                        ))
                    }
                }
            }
            "foreground" => style.foreground = parse_color(words.next())?,
            "background" => style.background = parse_color(words.next())?,
            "underline-color" => style.underline_color = parse_color(words.next())?,
            _ => return Err(format!("{word:?} is not part of a style")),
        }
    }
    Ok(style)
}

/// A color of the palette from 0 to 255, or `#rrggbb`.
fn parse_color(text: Option<&str>) -> Result<Color, String> {
    let color = text.and_then(|text| match text.strip_prefix('#') {
        Some(hex) if hex.len() == 6 && hex.bytes().all(|byte| byte.is_ascii_hexdigit()) => {
            let component = |at: usize| u8::from_str_radix(&hex[at..at + 2], 16).ok();
            Some(Color::Rgb(component(0)?, component(2)?, component(4)?))
        }
        Some(_) => None,
        None => text.parse().ok().map(Color::Palette),
    });
    color.ok_or(format!(
        "{text:?} is not a color: a number from 0 to 255, or #rrggbb"
    ))
}

/// A row of styles drawn as `|`, a letter for each of the `cols` cells, and `|`.
fn parse_styles(text: &str, cols: u16, letters: &[(char, Style)]) -> Result<Vec<Style>, String> {
    let inside = text
        .strip_prefix('|')
        .and_then(|inside| inside.strip_suffix('|'))
        .ok_or(format!("{text:?} is not a row of styles between two |"))?;
    let styles = inside
        .chars()
        .map(|letter| match letter {
            '.' => Ok(Style::default()),
            letter => letters
                .iter()
                .find(|(named, _)| *named == letter)
                .map(|(_, style)| *style)
                .ok_or(format!("{letter:?} is no style the case names")),
        })
        .collect::<Result<Vec<_>, _>>()?;
    if styles.len() != usize::from(cols) {
        return Err(format!("{} cells on a screen {cols} wide", styles.len()));
    }
    Ok(styles)
}

/// `style` in the words a case names it with: what it has but the default.
fn describe(style: &Style) -> String {
    let mut words: Vec<String> = Vec::new();
    for (on, word) in [
        (style.bold, "bold"),
        (style.dim, "dim"),
        (style.italic, "italic"),
    ] {
        if on {
            words.push(word.to_string());
        }
    }
    let underline = match style.underline {
        Underline::None => None,
        Underline::Single => Some("single"),
        Underline::Double => Some("double"),
        Underline::Curly => Some("curly"),
        Underline::Dotted => Some("dotted"),
        Underline::Dashed => Some("dashed"),
    };
    words.extend(underline.map(|underline| format!("underline {underline}")));
    for (on, word) in [
        (style.strikethrough, "strikethrough"),
        (style.inverse, "inverse"),
        (style.hidden, "hidden"),
    ] {
        if on {
            words.push(word.to_string());
        }
    }
    for (name, color) in [
        ("foreground", style.foreground),
        ("background", style.background),
        ("underline-color", style.underline_color),
    ] {
        match color {
            Color::Default => {}
            Color::Palette(index) => words.push(format!("{name} {index}")),
            Color::Rgb(red, green, blue) => {
                words.push(format!("{name} #{red:02x}{green:02x}{blue:02x}"))
            }
        }
    }
    words.join(" ")
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
            Kind::Styles(expected) => {
                let actual = styles(&core);
                if &actual != expected {
                    problem(format!(
                        "the styles differ\nexpected\n{}\ngot\n{}",
                        draw_styles(expected, &case.letters),
                        draw_styles(&actual, &case.letters)
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

fn styles(core: &TerminalCore) -> Vec<Vec<Style>> {
    core.screen()
        .rows()
        .iter()
        .map(|row| row.cells().iter().map(|cell| *cell.style()).collect())
        .collect()
}

/// `rows` of styles as a case draws them, with the case's `letters`. Each style no letter
/// stands for gets a capital, and is said in words after the rows.
fn draw_styles(rows: &[Vec<Style>], letters: &[(char, Style)]) -> String {
    let mut others: Vec<Style> = Vec::new();
    let mut lines = Vec::new();
    for row in rows {
        let mut line = String::from("|");
        for style in row {
            let named = letters.iter().find(|(_, named)| named == style);
            line.push(match named {
                _ if *style == Style::default() => '.',
                Some((letter, _)) => *letter,
                None => {
                    let at = others.iter().position(|other| other == style);
                    capital(at.unwrap_or_else(|| {
                        others.push(*style);
                        others.len() - 1
                    }))
                }
            });
        }
        line.push('|');
        lines.push(line);
    }
    for (at, style) in others.iter().enumerate() {
        lines.push(format!("{} is {}", capital(at), describe(style)));
    }
    lines.join("\n")
}

/// `A` for the first style no letter stands for, `B` for the second, and `?` past `Z`.
fn capital(at: usize) -> char {
    u8::try_from(at)
        .ok()
        .filter(|&at| at < 26)
        .map_or('?', |at| char::from(b'A' + at))
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
        "keyboard" => modes.keyboard.bits().to_string(),
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
fn styles_read_back_as_written() {
    let style =
        parse_style("bold underline curly foreground 1 background #0a0b0c underline-color 255")
            .unwrap();
    assert_eq!(
        style,
        Style {
            bold: true,
            underline: Underline::Curly,
            foreground: Color::Palette(1),
            background: Color::Rgb(10, 11, 12),
            underline_color: Color::Palette(255),
            ..Style::default()
        }
    );
    for words in [
        "bold dim italic underline single strikethrough inverse hidden",
        "underline double",
        "underline dotted",
        "underline dashed foreground 0",
        "foreground #ff8000 background 16 underline-color #000000",
    ] {
        let style = parse_style(words).unwrap();
        assert_eq!(describe(&style), words);
        assert_eq!(parse_style(&describe(&style)).unwrap(), style);
    }
    for bad in [
        "",
        "blink",
        "underline",
        "underline wavy",
        "foreground",
        "foreground 256",
        "foreground #12345",
        "foreground #12345g",
        "background red",
    ] {
        assert!(parse_style(bad).is_err(), "{bad}");
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
        ("## a\nsize 2x1\nstyle R bold\n", "is not a letter"),
        ("## a\nsize 2x1\nstyle rr bold\n", "is not a letter"),
        ("## a\nsize 2x1\nstyle r blink\n", "is not part of a style"),
        ("## a\nsize 2x1\nstyle r\n", "names nothing"),
        (
            "## a\nsize 2x1\nstyle r bold\nstyle r dim\n",
            "a second style named r",
        ),
        (
            "## a\nsize 2x1\nstyles\n|.x|\n",
            "is no style the case names",
        ),
        (
            "## a\nsize 2x1\nstyles\n|...|\n",
            "3 cells on a screen 2 wide",
        ),
        ("## a\nsize 2x2\nstyles\n|..|\n", "ends inside the styles"),
        ("## a\nsize 2x1\nstyles\n..\n", "is not a row of styles"),
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

    // Styles, with one no letter stands for.
    let text = "## c\nsize 3x1\nstyle r foreground 1\nsend \\e[31ma\\e[1mb\nstyles\n|rr.|\n";
    let cases = parse("f.txt", text, &mut Vec::new());
    assert_eq!(
        check("f.txt", &cases[0]),
        ["f.txt:5: c: the styles differ\nexpected\n|rr.|\ngot\n|rA.|\nA is bold foreground 1"]
    );

    // A reply no line checks.
    let cases = parse("f.txt", "## b\nsize 3x1\nsend \\e[5n\n", &mut Vec::new());
    assert_eq!(
        check("f.txt", &cases[0]),
        ["f.txt:3: b: replies no line checks: \\e[0n"]
    );
}
