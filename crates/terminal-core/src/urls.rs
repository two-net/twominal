//! The link at a cell, for the mouse to show and open (TERM-20): the one a program made of it
//! (OSC 8), or else a URL in the text around it. And which links may open, and as what (SEC-07).
//!
//! Nothing is kept: a link is looked for when asked, in the run of characters around the cell
//! that a URL may hold, so output costs nothing more. No terminal is the model for what a URL
//! is, though Alacritty and kitty find them much the same way. DESIGN.md §3 *URLs* has the rules.

use alloc::string::String;
use alloc::vec::Vec;

use crate::links::{LinkId, Links};
use crate::rows::Rows;
use crate::screen::Cell;

/// The most cells a run of URL characters may take for a URL to be looked for in it: VTE's
/// limit for a link, as [`crate::links`] keeps it.
#[cfg(not(any(test, fuzzing)))]
const RUN: usize = crate::links::URI_MOST;
/// A short run in the tests and under the fuzzer, so that they reach it.
#[cfg(any(test, fuzzing))]
pub(crate) const RUN: usize = 16;

/// The most bytes a URL's target may take, so a frame that shows it stays small.
#[cfg(not(any(test, fuzzing)))]
const TARGET: usize = crate::links::URI_MOST;
/// Fewer than a run of characters that are not ASCII takes, so the tests reach it.
#[cfg(any(test, fuzzing))]
pub(crate) const TARGET: usize = 24;

/// The schemes a URL found in the text starts with: those that may open (SEC-07).
const SCHEMES: [&[u8]; 4] = [b"http://", b"https://", b"file://", b"mailto:"];

/// What no URL character is, besides whitespace and the control characters: what no URL
/// holds as it is, and the CJK and fullwidth punctuation, so Chinese and Japanese text around
/// a URL is not part of it.
const NOT_URL: &str = "<>\"`{}|\\^、。，．：；！？「」『』（）【】";

/// What is taken off the end of a URL: what a sentence puts after one.
const TRAILING: &[u8] = b".,:;!?'*";

/// A link at a cell: where it leads, and the cells it holds.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LinkAt {
    target: String,
    cells: Cells,
}

/// The cells a link holds.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Cells {
    /// Every cell a program made part of this link, wherever it is.
    Made(LinkId),
    /// From a cell to the boundary after the last: a URL in the text. Rows by their numbers.
    Text {
        start: (u64, usize),
        end: (u64, usize),
    },
}

impl LinkAt {
    /// Where the link leads: the URI a program gave it, or the URL's characters, the marks
    /// joined to them included, as copying gives them. Whether and as what it opens is
    /// [`opening`]'s to say.
    pub fn target(&self) -> &str {
        &self.target
    }

    /// Whether the link holds `cell`, the cell at `col` of row `row`, by number. A hidden cell
    /// is part of none, and a URL holds no blank: not the one a wide character that went on to
    /// the next row left at the end of this one.
    pub fn holds(&self, row: u64, col: usize, cell: &Cell) -> bool {
        if cell.style().hidden {
            return false;
        }
        match self.cells {
            Cells::Made(link) => cell.link() == Some(link),
            Cells::Text { start, end } => {
                (start..end).contains(&(row, col))
                    && cell.chars().next().is_none_or(|char| !char.is_whitespace())
            }
        }
    }
}

/// The link at `col` of row `number`: the cell's own, if a program made one of it, or the URL
/// in the text around it. None on a hidden cell, and on one no URL holds.
pub(crate) fn link_at(rows: &Rows, links: &Links, number: u64, col: usize) -> Option<LinkAt> {
    let col = rows.left_half(number, col);
    let cell = rows.cell(number, col)?;
    if cell.style().hidden {
        return None;
    }
    if let Some(link) = cell.link() {
        return Some(LinkAt {
            target: links.get(link)?.uri().into(),
            cells: Cells::Made(link),
        });
    }
    url(rows, (number, col))
}

/// The URL in the text that holds the character at `at`.
fn url(rows: &Rows, at: (u64, usize)) -> Option<LinkAt> {
    let char_at = |at: (u64, usize)| rows.cell(at.0, at.1).filter(|cell| in_url(cell));
    let mut taken = char_at(at)?.width();

    // The run of URL characters around it, from its first character.
    let mut first = at;
    while let Some(before) = previous(rows, first) {
        let Some(cell) = char_at(before) else {
            break;
        };
        taken += cell.width();
        if taken > RUN {
            return None;
        }
        first = before;
    }
    let mut run: Vec<((u64, usize), &Cell)> = Vec::new();
    let mut next_at = Some(first);
    taken = 0;
    while let Some(here) = next_at {
        let Some(cell) = char_at(here) else {
            break;
        };
        taken += cell.width();
        if taken > RUN {
            return None;
        }
        run.push((here, cell));
        next_at = next(rows, here);
    }
    let index = run.iter().position(|(here, _)| *here == at)?;

    // The first URL in the run, then the next after it, until one holds the character.
    let mut from = 0;
    while let Some((start, scheme)) = scheme(&run, from) {
        if index < start {
            return None;
        }
        let end = start + trimmed(&run[start..]);
        if end > start + scheme && index < end {
            let cells = &run[start..end];
            let target: String = cells.iter().flat_map(|(_, cell)| cell.chars()).collect();
            if target.len() > TARGET {
                return None;
            }
            let ((last_row, last_col), last) = cells[cells.len() - 1];
            return Some(LinkAt {
                target,
                cells: Cells::Text {
                    start: cells[0].0,
                    end: (last_row, last_col + last.width()),
                },
            });
        }
        from = end.max(start + 1);
    }
    None
}

/// Whether `cell` holds a character a URL may: none of what [`NOT_URL`] holds, no whitespace
/// or control character, and not hidden or made part of a link by a program. A wide
/// character's right half is its character's, which is looked at instead.
fn in_url(cell: &Cell) -> bool {
    let Some(char) = cell.chars().next() else {
        return false;
    };
    !char.is_whitespace()
        && !char.is_control()
        && !NOT_URL.contains(char)
        && cell.link().is_none()
        && !cell.style().hidden
}

/// The character before the one at `at` in its line, at the left half of a wide one.
fn previous(rows: &Rows, at: (u64, usize)) -> Option<(u64, usize)> {
    let (row, col) = rows.before(at.0, at.1)?;
    Some((row, rows.left_half(row, col)))
}

/// The character after the one at `at` in its line, past the right half of a wide one.
fn next(rows: &Rows, at: (u64, usize)) -> Option<(u64, usize)> {
    let mut after = rows.after(at.0, at.1)?;
    if rows.cell(after.0, after.1)?.width() == 0 {
        after = rows.after(after.0, after.1)?;
    }
    Some(after)
}

/// Where the first scheme is in `run`, from character `from` on, and how many characters it
/// takes. Each must be one ASCII character with nothing joined to it, in any letter case.
fn scheme(run: &[((u64, usize), &Cell)], from: usize) -> Option<(usize, usize)> {
    (from..run.len()).find_map(|start| {
        SCHEMES.iter().find_map(|scheme| {
            let chars = run.get(start..start + scheme.len())?;
            chars
                .iter()
                .zip(scheme.iter())
                .all(|((_, cell), byte)| ascii(cell).is_some_and(|a| a.eq_ignore_ascii_case(byte)))
                .then_some((start, scheme.len()))
        })
    })
}

/// How many of `url`'s characters are left once what a sentence or a pair of brackets puts
/// after it is taken off: [`TRAILING`], and the `)` and `]` it did not open.
fn trimmed(url: &[((u64, usize), &Cell)]) -> usize {
    let count = |byte: u8| {
        url.iter()
            .filter(|(_, cell)| ascii(cell) == Some(byte))
            .count()
    };
    // Openings are never taken off, so only the closings left change.
    let open = [count(b'('), count(b'[')];
    let mut close = [count(b')'), count(b']')];
    let mut end = url.len();
    while let Some(last) = end.checked_sub(1).and_then(|last| ascii(url[last].1)) {
        let bracket = match last {
            b')' => Some(0),
            b']' => Some(1),
            _ => None,
        };
        match bracket {
            Some(bracket) if close[bracket] > open[bracket] => close[bracket] -= 1,
            None if TRAILING.contains(&last) => {}
            _ => break,
        }
        end -= 1;
    }
    end
}

/// The cell's character, if it is one ASCII character with nothing joined to it.
fn ascii(cell: &Cell) -> Option<u8> {
    let mut chars = cell.chars();
    let char = chars.next()?;
    (chars.next().is_none() && char.is_ascii()).then_some(char as u8)
}

/// The URL to hand the OS to open the link to `target`, or None when SEC-07 does not let it
/// open. `host` is this machine's name, which a `file:` link may give, or empty for none.
///
/// Only `http`, `https`, `mailto` and `file` open, and not every link of them: a web link needs
/// a host and no user name, a mail may name no header for an attachment, however it is written, and a file must be a path from
/// the root on this machine, by no form of a Windows network or device path. The scheme is
/// written lowercase, a file link as `file:///path`, and every byte RFC 3986 does not give URLs
/// as `%XX`. So the OS is handed one word of printable ASCII that starts with a letter, and
/// what this gives opens as itself.
pub fn opening(target: &str, host: &str) -> Option<String> {
    let (scheme, rest) = target.split_once(':')?;
    let scheme = scheme.to_ascii_lowercase();
    let rest = match scheme.as_str() {
        "http" | "https" => {
            let (authority, _) = authority(rest)?;
            let authority = decoded(authority);
            if authority.is_empty() || authority.contains(&b'@') {
                return None;
            }
            rest
        }
        "mailto" => {
            // Read as the most lenient mail program may read it: decoded first, then split into
            // headers at `?`, `&` and `;`, the blanks around a name dropped.
            let text = decoded(rest);
            let attaches = text
                .split(|byte| matches!(byte, b'?' | b'&' | b';'))
                .any(|header| {
                    let Some(end) = header.iter().position(|&byte| byte == b'=') else {
                        return false;
                    };
                    let name = header[..end].trim_ascii();
                    name.get(..6)
                        .is_some_and(|start| start.eq_ignore_ascii_case(b"attach"))
                });
            if attaches {
                return None;
            }
            rest
        }
        "file" => {
            let path = match authority(rest) {
                Some((authority, path)) => {
                    let here = authority.is_empty()
                        || authority.eq_ignore_ascii_case("localhost")
                        || (!host.is_empty() && authority.eq_ignore_ascii_case(host));
                    if !here {
                        return None;
                    }
                    path
                }
                None => rest,
            };
            let bytes = decoded(path);
            let network = bytes.starts_with(b"//") || bytes.contains(&b'\\');
            if !path.starts_with('/') || network || bytes.iter().any(u8::is_ascii_control) {
                return None;
            }
            let mut url = String::from("file://");
            encode(&mut url, path);
            return Some(url);
        }
        _ => return None,
    };
    let mut url = scheme;
    url.push(':');
    encode(&mut url, rest);
    Some(url)
}

/// The authority after `//` at the start of what follows a scheme, and what follows it: the
/// path, the query and the fragment. None without `//`.
fn authority(rest: &str) -> Option<(&str, &str)> {
    let rest = rest.strip_prefix("//")?;
    let end = rest.find(['/', '?', '#']).unwrap_or(rest.len());
    Some(rest.split_at(end))
}

/// `text` with every `%XX` read back as its byte.
fn decoded(text: &str) -> Vec<u8> {
    let bytes = text.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut at = 0;
    while at < bytes.len() {
        let hex = bytes.get(at + 1..at + 3).and_then(|hex| {
            let digit = |byte: u8| char::from(byte).to_digit(16);
            Some(digit(hex[0])? * 16 + digit(hex[1])?)
        });
        match (bytes[at], hex) {
            (b'%', Some(byte)) => {
                out.push(byte as u8);
                at += 3;
            }
            (byte, _) => {
                out.push(byte);
                at += 1;
            }
        }
    }
    out
}

/// Writes `text` to `out` with every byte but RFC 3986's unreserved and reserved characters and
/// `%` as `%XX`.
fn encode(out: &mut String, text: &str) {
    const HEX: &[u8; 16] = b"0123456789ABCDEF";
    for byte in text.bytes() {
        if byte.is_ascii_alphanumeric() || b"-._~:/?#[]@!$&'()*+,;=%".contains(&byte) {
            out.push(char::from(byte));
        } else {
            out.push('%');
            out.push(char::from(HEX[usize::from(byte >> 4)]));
            out.push(char::from(HEX[usize::from(byte & 15)]));
        }
    }
}
