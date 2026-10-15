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
//! |    |                                                  `>` after a row means it wraps,
//! |  gh|                                                  and `»` that it wraps and its last
//! |ijkl|                                                  cell is not part of its line
//! scrollback 2                                            the scrollback's rows, oldest
//! |ab  |                                                  first, as many as it says, padded
//! |cd  |                                                  to the width
//! resize 3x2                                              resizes the terminal; what follows
//!                                                         is drawn at the new size
//! limit 5                                                 keeps 5 rows of scrollback
//! clear                                                   Clear (TERM-22)
//! cursor 2;2                                              counted from 1, as CUP counts
//! reply \e[?62;22c                                        the next event not yet checked,
//! title vim - a.txt                                       which must be this: a reply, a
//! title                                                   title, or none, the directory
//! directory //host/tmp                                    OSC 7 gave, the bell, a write to
//! bell                                                    the clipboard (`c`), the primary
//! clipboard write cp text                                 selection (`p`) or both, or a
//! clipboard read c                                        read of one
//! mode bracketed_paste on                                 a field of `Modes`: `on` or
//! mode keyboard 5                                         `off`, the mouse's tracking or
//! mode cursor_style steady bar                            format, the keyboard flags'
//!                                                         number, or the cursor's style:
//!                                                         `default`, or `blinking` or
//!                                                         `steady` and its shape
//! style r bold foreground 1                               `r` stands for this style below
//! styles                                                  every row again, a letter for the
//! |rr..|                                                  style of each cell
//! |....|
//! |....|
//! link a https://example.com id1                          `a` stands for this link below:
//! links                                                   its URI, and its id if it has one
//! |aa.b|                                                  every row again, a letter for the
//! |....|                                                  link of each cell
//! |....|
//! select words 2;3                                        starts a selection, by characters,
//! select block 1;2+                                       words, lines or as a block, in a
//! extend 3;4                                              cell, `+` for its right half; moves
//! select all                                              its end; Select All; unselect
//! unselect
//! selected                                                every row again, `*` for a cell
//! |.**.|                                                  selected and `.` for one not
//! |....|
//! |....|
//! copy ab\ncd                                             the text copying gives, or with no
//! copy                                                    text that it gives none
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
//! A row of links has a letter for each cell. `.` is no link, and `a` to `z` the links the case
//! named before with `link`. Cells with the same letter are one link, and cells with letters of
//! their own are links of their own, even when the URIs are the same.
//!
//! A place to select is a row and a column counted from 1, as `cursor` counts them, from the
//! top left of the screen as it is at that step. Row 0 is the scrollback's newest row, -1 the
//! one before, and so on. A row or a column past the edge is a drag past it.
//!
//! `send`, `reply`, `title`, `directory`, `clipboard`'s text, `copy`'s, and `link`'s URI and id take
//! `\e`, `\a`, `\b`, `\t`, `\n`, `\r`, `\\` and `\xHH`; the rest is UTF-8 as written. No line
//! may end in a space, so a trailing space is written `\x20`. Every event must be checked: one
//! left over at the end of a case fails it (SEC-05).
//!
//! The Tab answers a question about a color with the theme's, unless the program set one. Here
//! the theme is made up: the foreground is `#c0c0c0`, the background `#101010`, the cursor
//! `#f0f000`, and color n of the palette n, n and n. The answer is checked as a `reply`.
//!
//! Each case runs twice: as written, and with every `send` given a byte at a time (ARC-03).

use std::collections::{BTreeSet, VecDeque};
use std::fs;
use std::path::Path;

use terminal_core::{
    Clipboard, Color, CursorShape, Event, LinkId, Modes, MouseFormat, MouseTracking, Place,
    Selecting, Selection, Size, Style, TerminalCore, ThemeColor, Underline,
};

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
    /// The links its letters stand for, as `link` named them.
    links: Vec<(char, Target)>,
    steps: Vec<Step>,
}

/// Where a link leads: its URI, and its id if it has one.
#[derive(Clone, PartialEq, Debug)]
struct Target {
    uri: String,
    id: Option<String>,
}

struct Step {
    line: usize,
    kind: Kind,
}

enum Kind {
    Send(Vec<u8>),
    Screen(Vec<Row>),
    Scrollback(Vec<Row>),
    Resize(Size),
    Limit(usize),
    Clear,
    Styles(Vec<Vec<Style>>),
    /// The rows of links, as the case drew them.
    Links(Vec<String>),
    Cursor(u16, u16),
    Said(Said),
    Mode(String, String),
    Select(Selecting, Spot),
    Extend(Spot),
    SelectAll,
    Unselect,
    /// The rows of selected cells, as the case drew them.
    Selected(Vec<String>),
    Copy(Option<String>),
}

/// A place to select as a case writes it: the row and the column counted from 1 from the
/// screen's top left, and whether it is the cell's right half.
#[derive(Clone, Copy)]
struct Spot {
    row: i64,
    col: i32,
    right_half: bool,
}

/// What the core said besides the screen, as a case writes it: an event, or for a question
/// about a color, the answer.
#[derive(Clone, PartialEq, Debug)]
enum Said {
    Reply(Vec<u8>),
    Title(Option<String>),
    Directory {
        host: Vec<u8>,
        path: Vec<u8>,
    },
    Bell,
    Write {
        clipboard: bool,
        primary: bool,
        text: Vec<u8>,
    },
    Read(Selection),
    /// A report of the shell integration, which no case can have: it runs without a nonce.
    Report(String),
}

impl std::fmt::Display for Said {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        match self {
            Self::Reply(bytes) => write!(f, "reply {}", escape(bytes)),
            Self::Title(None) => write!(f, "title"),
            Self::Title(Some(title)) => write!(f, "title {}", escape(title.as_bytes())),
            Self::Directory { host, path } => {
                write!(f, "directory //{}{}", escape(host), escape(path))
            }
            Self::Bell => write!(f, "bell"),
            Self::Write {
                clipboard,
                primary,
                text,
            } => {
                let targets = match (clipboard, primary) {
                    (true, true) => "cp",
                    (true, false) => "c",
                    (false, _) => "p",
                };
                write!(f, "clipboard write {targets}")?;
                if !text.is_empty() {
                    write!(f, " {}", escape(text))?;
                }
                Ok(())
            }
            Self::Read(from) => write!(f, "clipboard read {}", selection_name(*from)),
            Self::Report(report) => write!(f, "report {report}"),
        }
    }
}

fn selection_name(selection: Selection) -> &'static str {
    match selection {
        Selection::Clipboard => "c",
        Selection::Primary => "p",
    }
}

/// What a case expects of `event`, a question about a color answered with the made-up theme.
fn said(event: Event) -> Said {
    match event {
        Event::Reply(reply) => Said::Reply(reply),
        Event::Title(title) => Said::Title(title),
        Event::Directory(directory) => Said::Directory {
            host: directory.host.into_bytes(),
            path: directory.path,
        },
        Event::Bell => Said::Bell,
        Event::Color(query) => Said::Reply(query.reply(theme(query.color()))),
        Event::Clipboard(clipboard) => match *clipboard {
            Clipboard::Write {
                clipboard,
                primary,
                text,
            } => Said::Write {
                clipboard,
                primary,
                text: text.into_bytes(),
            },
            Clipboard::Read(query) => Said::Read(query.from()),
        },
        Event::Report(report) => Said::Report(format!("{report:?}")),
    }
}

/// The made-up theme's colors, which the start of this file lists.
fn theme(color: ThemeColor) -> [u8; 3] {
    match color {
        ThemeColor::Foreground => [0xc0, 0xc0, 0xc0],
        ThemeColor::Background => [0x10, 0x10, 0x10],
        ThemeColor::Cursor => [0xf0, 0xf0, 0x00],
        ThemeColor::Palette(index) => [index; 3],
    }
}

/// A row of the screen as a case draws it.
#[derive(PartialEq)]
struct Row {
    text: String,
    wrapped: bool,
    /// Its last cell is not part of its line: a wide character that did not fit there went on.
    early: bool,
}

/// The fields of `Modes` a case can name, with the words their values are written in.
const MODES: &[&str] = &[
    "cursor_visible",
    "cursor_style",
    "alternate_screen",
    "application_cursor",
    "application_keypad",
    "newline",
    "bracketed_paste",
    "focus",
    "mouse",
    "mouse_format",
    "alternate_scroll",
    "keyboard",
];

/// The cases in `text`. What cannot be read goes to `problems`, as `file:line: why`, and
/// the case it was in is dropped, so that nothing is silently left unchecked.
fn parse(file: &str, text: &str, problems: &mut Vec<String>) -> Vec<Case> {
    let mut cases = Vec::new();
    let mut case: Option<Case> = None;
    // The size the case has reached: rows are drawn at it.
    let mut now = Size { cols: 0, rows: 0 };
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
                links: Vec::new(),
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
                    now = size;
                    continue;
                }
                None => Err(format!("{rest:?} is not a size such as 80x24")),
            },
            "resize" => match parse_size(rest) {
                Some(size) => {
                    now = size;
                    Ok(Kind::Resize(size))
                }
                None => Err(format!("{rest:?} is not a size such as 80x24")),
            },
            "limit" => rest
                .parse()
                .map(Kind::Limit)
                .map_err(|_| format!("{rest:?} is not a number of rows")),
            "clear" if rest.is_empty() => Ok(Kind::Clear),
            "select" if rest == "all" => Ok(Kind::SelectAll),
            "select" => parse_select(rest),
            "extend" => parse_spot(rest)
                .map(Kind::Extend)
                .ok_or(format!("{rest:?} is not a place such as 3;5 or 3;5+")),
            "unselect" if rest.is_empty() => Ok(Kind::Unselect),
            "copy" if rest.is_empty() => Ok(Kind::Copy(None)),
            "copy" => unescape(rest).and_then(|text| match String::from_utf8(text) {
                Ok(text) => Ok(Kind::Copy(Some(text))),
                Err(_) => Err("a text to copy that is not UTF-8".to_string()),
            }),
            "selected" if rest.is_empty() => {
                let mut rows = Vec::new();
                let mut why = None;
                for _ in 0..now.rows {
                    match lines.next() {
                        Some((at, row)) => match parse_selected(row, now.cols) {
                            Ok(()) => rows.push(row.to_string()),
                            Err(error) => {
                                why.get_or_insert(format!("row on line {at}: {error}"));
                            }
                        },
                        None => {
                            why.get_or_insert("the file ends inside the selection".to_string());
                        }
                    }
                }
                why.map_or(Ok(Kind::Selected(rows)), Err)
            }
            "scrollback" => match rest.parse::<usize>() {
                Ok(count) => read_rows(&mut lines, count, "scrollback").map(Kind::Scrollback),
                Err(_) => Err(format!("{rest:?} is not a number of rows")),
            },
            "style" => match parse_letter(rest, &current.letters) {
                Ok(letter) => {
                    current.letters.push(letter);
                    continue;
                }
                Err(why) => Err(why),
            },
            "link" => match parse_link(rest, &current.links) {
                Ok(link) => {
                    current.links.push(link);
                    continue;
                }
                Err(why) => Err(why),
            },
            "send" if rest.is_empty() => Err("`send` with nothing to send".to_string()),
            "send" => unescape(rest).map(Kind::Send),
            "reply" if rest.is_empty() => Err("`reply` with no reply".to_string()),
            "reply" => unescape(rest).map(|reply| Kind::Said(Said::Reply(reply))),
            "title" if rest.is_empty() => Ok(Kind::Said(Said::Title(None))),
            "title" => unescape(rest).and_then(|title| match String::from_utf8(title) {
                Ok(title) => Ok(Kind::Said(Said::Title(Some(title)))),
                Err(_) => Err("a title that is not UTF-8".to_string()),
            }),
            "directory" => parse_directory(rest).map(Kind::Said),
            "bell" if rest.is_empty() => Ok(Kind::Said(Said::Bell)),
            "clipboard" => parse_clipboard(rest).map(Kind::Said),
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
                read_rows(&mut lines, usize::from(now.rows), "screen").map(Kind::Screen)
            }
            "styles" if rest.is_empty() => {
                let mut rows = Vec::new();
                let mut why = None;
                for _ in 0..now.rows {
                    match lines.next() {
                        Some((at, row)) => match parse_styles(row, now.cols, &current.letters) {
                            Ok(row) => rows.push(row),
                            Err(error) => {
                                why.get_or_insert(format!("row on line {at}: {error}"));
                            }
                        },
                        None => {
                            why.get_or_insert("the file ends inside the styles".to_string());
                        }
                    }
                }
                why.map_or(Ok(Kind::Styles(rows)), Err)
            }
            "links" if rest.is_empty() => {
                let mut rows = Vec::new();
                let mut why = None;
                for _ in 0..now.rows {
                    match lines.next() {
                        Some((at, row)) => match parse_links(row, now.cols, &current.links) {
                            Ok(()) => rows.push(row.to_string()),
                            Err(error) => {
                                why.get_or_insert(format!("row on line {at}: {error}"));
                            }
                        },
                        None => {
                            why.get_or_insert("the file ends inside the links".to_string());
                        }
                    }
                }
                why.map_or(Ok(Kind::Links(rows)), Err)
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

/// The next `count` rows of `lines`, of a screen or of the scrollback as `what` says.
fn read_rows<'a>(
    lines: &mut impl Iterator<Item = (usize, &'a str)>,
    count: usize,
    what: &str,
) -> Result<Vec<Row>, String> {
    let mut rows = Vec::new();
    let mut why = None;
    for _ in 0..count {
        match lines.next() {
            Some((at, row)) => match parse_row(row) {
                Ok(row) => rows.push(row),
                Err(error) => {
                    why.get_or_insert(format!("row on line {at}: {error}"));
                }
            },
            None => {
                why.get_or_insert(format!("the file ends inside a {what}"));
            }
        }
    }
    why.map_or(Ok(rows), Err)
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

/// `characters`, `words`, `lines` or `block`, and a place.
fn parse_select(text: &str) -> Result<Kind, String> {
    let (how, place) = text.split_once(' ').unwrap_or((text, ""));
    let by = match how {
        "characters" => Selecting::Characters,
        "words" => Selecting::Words,
        "lines" => Selecting::Lines,
        "block" => Selecting::Block,
        _ => {
            return Err(format!(
                "{how:?} is not `all`, or characters, words, lines or block"
            ))
        }
    };
    let spot = parse_spot(place).ok_or(format!("{place:?} is not a place such as 3;5 or 3;5+"))?;
    Ok(Kind::Select(by, spot))
}

/// A place such as `3;5`, or `3;5+` for the right half of the cell.
fn parse_spot(text: &str) -> Option<Spot> {
    let (text, right_half) = match text.strip_suffix('+') {
        Some(text) => (text, true),
        None => (text, false),
    };
    let (row, col) = text.split_once(';')?;
    Some(Spot {
        row: row.parse().ok()?,
        col: col.parse().ok()?,
        right_half,
    })
}

/// Checks a row of the selection drawn as `|`, `*` or `.` for each of the `cols` cells, and `|`.
fn parse_selected(text: &str, cols: u16) -> Result<(), String> {
    let inside = text
        .strip_prefix('|')
        .and_then(|inside| inside.strip_suffix('|'))
        .ok_or(format!(
            "{text:?} is not a row of the selection between two |"
        ))?;
    if let Some(other) = inside.chars().find(|char| !matches!(char, '*' | '.')) {
        return Err(format!("{other:?} is neither * nor ."));
    }
    let cells = inside.chars().count();
    if cells != usize::from(cols) {
        return Err(format!("{cells} cells on a screen {cols} wide"));
    }
    Ok(())
}

/// A row drawn as `|`, its text and `|`, then `>` if it wraps, or `»` if it wraps and its last
/// cell is not part of its line. A `|` inside the row is a character like any other: only the
/// last one ends it. How wide the text is depends on its characters, so it is not counted here;
/// a row of the wrong width fails as the screen differs.
fn parse_row(text: &str) -> Result<Row, String> {
    let (inside, wrapped, early) = if let Some(inside) = text.strip_suffix("|>") {
        (inside, true, false)
    } else if let Some(inside) = text.strip_suffix("|»") {
        (inside, true, true)
    } else {
        (text.strip_suffix('|').unwrap_or(""), false, false)
    };
    match inside.strip_prefix('|') {
        Some(inside) => Ok(Row {
            text: inside.to_string(),
            wrapped,
            early,
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

/// The directory `//host/path` says.
fn parse_directory(text: &str) -> Result<Said, String> {
    let url = unescape(text)?;
    let rest = url
        .strip_prefix(b"//")
        .ok_or(format!("{text:?} is not //host/path"))?;
    let slash = rest
        .iter()
        .position(|&byte| byte == b'/')
        .ok_or(format!("{text:?} has no path"))?;
    Ok(Said::Directory {
        host: rest[..slash].to_vec(),
        path: rest[slash..].to_vec(),
    })
}

/// `write`, the targets and the text, or `read` and the target.
fn parse_clipboard(text: &str) -> Result<Said, String> {
    let words: Vec<&str> = text.split(' ').collect();
    match words[..] {
        ["read", target @ ("c" | "p")] => Ok(Said::Read(if target == "c" {
            Selection::Clipboard
        } else {
            Selection::Primary
        })),
        ["write", targets @ ("c" | "p" | "cp"), ref text @ ..] if text.len() < 2 => {
            Ok(Said::Write {
                clipboard: targets.contains('c'),
                primary: targets.contains('p'),
                text: text
                    .first()
                    .map(|text| unescape(text))
                    .transpose()?
                    .unwrap_or_default(),
            })
        }
        _ => Err(format!(
            "{text:?} is not `write`, c, p or cp and a text, or `read` and c or p"
        )),
    }
}

/// The letter `link` names and the link it stands for, from `a https://example.com id`. A
/// letter is named once.
fn parse_link(text: &str, links: &[(char, Target)]) -> Result<(char, Target), String> {
    let words: Vec<&str> = text.split(' ').collect();
    let (letter, uri, id) = match words[..] {
        [letter, uri] => (letter, uri, None),
        [letter, uri, id] => (letter, uri, Some(id)),
        _ => return Err(format!("{text:?} is not a letter, a URI and maybe an id")),
    };
    let mut chars = letter.chars();
    let letter = match (chars.next(), chars.next()) {
        (Some(letter @ 'a'..='z'), None) => letter,
        _ => return Err(format!("{letter:?} is not a letter from a to z")),
    };
    if links.iter().any(|(named, _)| *named == letter) {
        return Err(format!("a second link named {letter}"));
    }
    let text = |text: &str| -> Result<String, String> {
        String::from_utf8(unescape(text)?).map_err(|_| format!("{text:?} is not UTF-8"))
    };
    Ok((
        letter,
        Target {
            uri: text(uri)?,
            id: id.map(text).transpose()?,
        },
    ))
}

/// Checks a row of links drawn as `|`, a letter for each of the `cols` cells, and `|`.
fn parse_links(text: &str, cols: u16, links: &[(char, Target)]) -> Result<(), String> {
    let inside = text
        .strip_prefix('|')
        .and_then(|inside| inside.strip_suffix('|'))
        .ok_or(format!("{text:?} is not a row of links between two |"))?;
    for letter in inside.chars() {
        if letter != '.' && !links.iter().any(|(named, _)| *named == letter) {
            return Err(format!("{letter:?} is no link the case names"));
        }
    }
    let cells = inside.chars().count();
    if cells != usize::from(cols) {
        return Err(format!("{cells} cells on a screen {cols} wide"));
    }
    Ok(())
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
    let mut events = VecDeque::new();
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
                    events.extend(core.interpret(piece).into_iter().map(said));
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
            Kind::Scrollback(expected) => {
                let actual = scrollback(&core);
                if &actual != expected {
                    problem(format!(
                        "the scrollback differs\nexpected\n{}\ngot\n{}",
                        draw(expected),
                        draw(&actual)
                    ));
                }
            }
            Kind::Resize(size) => core.resize(*size),
            Kind::Limit(rows) => core.set_scrollback(*rows),
            Kind::Clear => core.clear(),
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
            Kind::Links(expected) => {
                let actual = draw_links(&core, &case.links);
                if actual != expected.join("\n") {
                    problem(format!(
                        "the links differ\nexpected\n{}\ngot\n{actual}",
                        expected.join("\n")
                    ));
                }
            }
            Kind::Said(expected) => match events.pop_front() {
                Some(actual) if &actual == expected => {}
                Some(actual) => problem(format!("expected {expected}, got {actual}")),
                None => problem(format!("expected {expected}, got nothing")),
            },
            Kind::Mode(name, expected) => {
                let actual = mode(core.modes(), name);
                if actual != *expected {
                    problem(format!(
                        "expected mode {name} {expected}, got mode {name} {actual}"
                    ));
                }
            }
            Kind::Select(by, spot) => {
                let at = place(&core, *spot);
                core.select(at, *by);
            }
            Kind::Extend(spot) => {
                let to = place(&core, *spot);
                core.extend(to);
            }
            Kind::SelectAll => core.select_all(),
            Kind::Unselect => core.unselect(),
            Kind::Selected(expected) => {
                let actual = draw_selected(&core);
                if actual != expected.join("\n") {
                    problem(format!(
                        "the selection differs\nexpected\n{}\ngot\n{actual}",
                        expected.join("\n")
                    ));
                }
            }
            Kind::Copy(expected) => {
                let actual = core.copy();
                if actual != *expected {
                    let text = |text: &Option<String>| match text {
                        Some(text) => format!("copy {}", escape(text.as_bytes())),
                        None => "copy, with nothing to copy".to_string(),
                    };
                    problem(format!(
                        "expected {}, got {}",
                        text(expected),
                        text(&actual)
                    ));
                }
            }
        }
    }
    if !events.is_empty() {
        let unchecked: Vec<String> = events.iter().map(Said::to_string).collect();
        problems.push((
            last,
            format!("events no line checks: {}", unchecked.join(", ")),
        ));
    }
    problems
}

fn screen(core: &TerminalCore) -> Vec<Row> {
    core.screen().rows().iter().map(drawn).collect()
}

/// The number of the screen's first row.
fn live(core: &TerminalCore) -> i64 {
    let scrollback = core.scrollback();
    (scrollback.start() + scrollback.len() as u64) as i64
}

/// The place `spot` names, on the screen as it is now.
fn place(core: &TerminalCore, spot: Spot) -> Place {
    Place {
        row: live(core) + spot.row - 1,
        col: spot.col - 1,
        right_half: spot.right_half,
    }
}

/// The screen's rows as a case draws the selection: `*` for a cell selected.
fn draw_selected(core: &TerminalCore) -> String {
    let selected = core.selected();
    let cols = usize::from(core.size().cols);
    (0..core.size().rows)
        .map(|index| {
            let number = live(core) as u64 + u64::from(index);
            let taken = selected.map_or(0..0, |selected| selected.cols(number));
            let cells: String = (0..cols)
                .map(|col| if taken.contains(&col) { '*' } else { '.' })
                .collect();
            format!("|{cells}|")
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// The scrollback's rows, oldest first, padded to the screen's width.
fn scrollback(core: &TerminalCore) -> Vec<Row> {
    let scrollback = core.scrollback();
    let cols = usize::from(core.size().cols);
    (0..scrollback.len())
        .map(|index| {
            let row = scrollback.get(index).unwrap();
            let mut drawn = drawn(row);
            drawn.text.extend(std::iter::repeat_n(
                ' ',
                cols.saturating_sub(row.cells().len()),
            ));
            drawn
        })
        .collect()
}

fn drawn(row: &terminal_core::Row) -> Row {
    Row {
        text: row.text(),
        wrapped: row.wrapped(),
        early: row.line().len() < row.cells().len(),
    }
}

fn styles(core: &TerminalCore) -> Vec<Vec<Style>> {
    core.screen()
        .rows()
        .iter()
        .map(|row| row.cells().iter().map(|cell| cell.style()).collect())
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

/// The screen's links as a case draws them. Each link takes the first of the case's `letters`
/// that stands for its URI and id and no other link before it, in the order the cells are read.
/// A link no letter is left for gets a capital, and is said in words after the rows.
fn draw_links(core: &TerminalCore, letters: &[(char, Target)]) -> String {
    let mut given: Vec<(LinkId, char)> = Vec::new();
    let mut others: Vec<Target> = Vec::new();
    let mut lines = Vec::new();
    for row in core.screen().rows() {
        let mut line = String::from("|");
        for cell in row.cells() {
            let Some(link) = cell.link() else {
                line.push('.');
                continue;
            };
            if let Some((_, letter)) = given.iter().find(|(given, _)| *given == link) {
                line.push(*letter);
                continue;
            }
            let target = core.link(link).map_or(
                Target {
                    uri: "a link the core does not keep".to_string(),
                    id: None,
                },
                |kept| Target {
                    uri: kept.uri().to_string(),
                    id: kept.id().map(str::to_string),
                },
            );
            let free = letters.iter().find(|(letter, named)| {
                *named == target && !given.iter().any(|(_, taken)| taken == letter)
            });
            let letter = match free {
                Some((letter, _)) => *letter,
                None => {
                    others.push(target);
                    capital(others.len() - 1)
                }
            };
            given.push((link, letter));
            line.push(letter);
        }
        line.push('|');
        lines.push(line);
    }
    for (at, target) in others.iter().enumerate() {
        let id = target
            .id
            .as_ref()
            .map_or(String::new(), |id| format!(" {id}"));
        lines.push(format!("{} is {}{id}", capital(at), target.uri));
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
        .map(|row| {
            let mark = match (row.wrapped, row.early) {
                (true, true) => "»",
                (true, false) => ">",
                _ => "",
            };
            format!("|{}|{mark}", row.text)
        })
        .collect();
    lines.join("\n")
}

/// The value of the field `name` of `modes`, as a case writes it.
fn mode(modes: &Modes, name: &str) -> String {
    let on = |on: bool| if on { "on" } else { "off" }.to_string();
    match name {
        "cursor_visible" => on(modes.cursor_visible),
        "cursor_style" => match modes.cursor_style {
            None => "default".to_string(),
            Some(style) => {
                let blinking = if style.blinking { "blinking" } else { "steady" };
                let shape = match style.shape {
                    CursorShape::Block => "block",
                    CursorShape::Underline => "underline",
                    CursorShape::Bar => "bar",
                };
                format!("{blinking} {shape}")
            }
        },
        "alternate_screen" => on(modes.alternate_screen),
        "application_cursor" => on(modes.application_cursor),
        "application_keypad" => on(modes.application_keypad),
        "newline" => on(modes.newline),
        "bracketed_paste" => on(modes.bracketed_paste),
        "focus" => on(modes.focus),
        "mouse_format" => match modes.mouse_format {
            MouseFormat::Legacy => "legacy",
            MouseFormat::Utf8 => "utf8",
            MouseFormat::Sgr => "sgr",
            MouseFormat::Urxvt => "urxvt",
        }
        .to_string(),
        "alternate_scroll" => on(modes.alternate_scroll),
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
    assert!(!row.early);
    let row = parse_row("|日日 |»").unwrap();
    assert_eq!(row.text, "日日 ");
    assert!(row.wrapped && row.early);
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
        ("## a\nsize 2x1\nresize 2by1\n", "is not a size"),
        ("## a\nsize 2x1\nlimit many\n", "is not a number of rows"),
        (
            "## a\nsize 2x1\nscrollback two\n",
            "is not a number of rows",
        ),
        (
            "## a\nsize 2x1\nscrollback 1\n",
            "the file ends inside a scrollback",
        ),
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
        ("## a\nsize 2x1\ndirectory /tmp\n", "is not //host/path"),
        ("## a\nsize 2x1\ndirectory //host\n", "has no path"),
        (
            "## a\nsize 2x1\nbell now\n",
            "is not something a case can say",
        ),
        ("## a\nsize 2x1\ntitle \\xff\n", "not UTF-8"),
        ("## a\nsize 2x1\nclipboard write q hi\n", "is not `write`"),
        ("## a\nsize 2x1\nclipboard read cp\n", "is not `write`"),
        ("## a\nsize 2x1\nclipboard write c a b\n", "is not `write`"),
        ("## a\nsize 2x1\nlink a\n", "a URI and maybe an id"),
        ("## a\nsize 2x1\nlink A u\n", "is not a letter"),
        (
            "## a\nsize 2x1\nlink a u\nlink a v\n",
            "a second link named a",
        ),
        ("## a\nsize 2x1\nlinks\n|.a|\n", "is no link the case names"),
        (
            "## a\nsize 2x1\nlinks\n|...|\n",
            "3 cells on a screen 2 wide",
        ),
        ("## a\nsize 2x2\nlinks\n|..|\n", "ends inside the links"),
        ("## a\nsize 2x1\nselect some 1;1\n", "is not `all`"),
        ("## a\nsize 2x1\nselect words 1,1\n", "is not a place"),
        ("## a\nsize 2x1\nselect words\n", "is not a place"),
        ("## a\nsize 2x1\nextend 1;1-\n", "is not a place"),
        (
            "## a\nsize 2x1\nunselect now\n",
            "is not something a case can say",
        ),
        ("## a\nsize 2x1\ncopy \\xff\n", "not UTF-8"),
        ("## a\nsize 2x1\nselected\n|*x|\n", "is neither * nor ."),
        (
            "## a\nsize 2x1\nselected\n|***|\n",
            "3 cells on a screen 2 wide",
        ),
        (
            "## a\nsize 2x2\nselected\n|..|\n",
            "ends inside the selection",
        ),
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
        ["f.txt:3: b: events no line checks: reply \\e[0n"]
    );

    // Events of another kind, or none, where the case expects one.
    let text = "## d\nsize 3x1\nsend \\e]2;x\\a\\a\nbell\ntitle y\n";
    let cases = parse("f.txt", text, &mut Vec::new());
    assert_eq!(
        check("f.txt", &cases[0]),
        [
            "f.txt:4: d: expected bell, got title x",
            "f.txt:5: d: expected title y, got bell",
        ]
    );
    let text = "## e\nsize 3x1\nsend x\nclipboard read c\n";
    let cases = parse("f.txt", text, &mut Vec::new());
    assert_eq!(
        check("f.txt", &cases[0]),
        ["f.txt:4: e: expected clipboard read c, got nothing"]
    );

    // Links: one letter for two links, and a link no letter stands for.
    let text =
        "## f\nsize 3x1\nlink a u\nsend \\e]8;;u\\aa\\e]8;;u\\ab\\e]8;;v\\ac\nlinks\n|aaa|\n";
    let cases = parse("f.txt", text, &mut Vec::new());
    assert_eq!(
        check("f.txt", &cases[0]),
        ["f.txt:5: f: the links differ\nexpected\n|aaa|\ngot\n|aAB|\nA is u\nB is v"]
    );

    // The selection and what copying gives.
    let text = "## g\nsize 3x1\nsend abc\nselect characters 1;1\nextend 1;2+\nselected\n|***|\ncopy abc\nunselect\ncopy ab\n";
    let cases = parse("f.txt", text, &mut Vec::new());
    assert_eq!(
        check("f.txt", &cases[0]),
        [
            "f.txt:6: g: the selection differs\nexpected\n|***|\ngot\n|**.|",
            "f.txt:8: g: expected copy abc, got copy ab",
            "f.txt:10: g: expected copy ab, got copy, with nothing to copy",
        ]
    );
}
