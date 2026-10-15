//! What the operating system commands programs send ask for (TERM-12, TERM-13): a title, the
//! working directory, colors, a hyperlink, the clipboard. This file reads them; `terminal.rs`
//! acts on them, and `control.rs` decides when, since an OSC acts only once it has ended with
//! BEL or ST. DESIGN.md §3 *What programs tell Twominal* has the rules.
//!
//! OSC 2222, the shell integration's reports, is `report.rs`'s.

use alloc::string::String;
use alloc::vec::Vec;

use crate::colors::{Rgb, ThemeColor};
use crate::links::{ID_MOST, URI_MOST};
use crate::REPORT_LIMIT;

/// The most parameters vte keeps of an OSC. It drops the text after a 16th `;`, so text that
/// may hold one, such as a URI, may have been cut short when there are this many.
const PARAMS: usize = 16;

/// The most characters a title keeps. A tab bar and a title bar show far fewer.
const TITLE_MOST: usize = 1024;

/// How an OSC ended, which its answer ends with too, as in xterm.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum End {
    Bel,
    /// ST, `ESC \`.
    St,
}

impl End {
    pub(crate) fn text(self) -> &'static str {
        match self {
            Self::Bel => "\x07",
            Self::St => "\x1b\\",
        }
    }
}

/// Where a program says it is (OSC 7): `file://host/path`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Directory {
    /// The machine, as the program named it: empty, `localhost` or a host name. Only the Tab
    /// can tell whether it is this one.
    pub host: String,
    /// The path, `%XX` read back as its byte. Bytes, because a path need not be UTF-8.
    pub path: Vec<u8>,
}

/// One of the places OSC 52 names.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Selection {
    /// The clipboard: `c`, and `s` or no name, which in xterm is the selection its settings
    /// choose.
    Clipboard,
    /// The primary selection, which only Linux has (SW-05): `p`.
    Primary,
}

/// What a program asked of the clipboard (OSC 52, TERM-13). Whether it may is the Clipboard
/// writes and reads settings' (SEC-04), which the Tab follows.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Clipboard {
    /// Put `text` in the clipboard, the primary selection or both. Empty text clears them.
    Write {
        clipboard: bool,
        primary: bool,
        text: String,
    },
    /// Say what is there.
    Read(ClipboardQuery),
}

/// A program asked what the clipboard or the primary selection holds (OSC 52 with `?`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ClipboardQuery {
    from: Selection,
    end: End,
}

impl ClipboardQuery {
    /// Where the program asked to read from.
    pub fn from(&self) -> Selection {
        self.from
    }

    /// The answer, as xterm gives it: `text` in base64, in the sequence that would write it,
    /// ended as the question was. Fixed text and the base64 alphabet only, so whatever `text`
    /// holds, even what a program put there, reaches the program as no control character
    /// (SEC-05).
    pub fn reply(&self, text: &str) -> Vec<u8> {
        let name = match self.from {
            Selection::Clipboard => "c",
            Selection::Primary => "p",
        };
        let (data, end) = (encode(text.as_bytes()), self.end.text());
        alloc::format!("\x1b]52;{name};{data}{end}").into_bytes()
    }
}

/// A link to start (OSC 8).
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct NewLink {
    pub(crate) uri: String,
    pub(crate) id: Option<String>,
}

/// A change to the colors, or a question about one, in the order the OSC gave them.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ColorChange {
    Set(ThemeColor, Rgb),
    Reset(ThemeColor),
    /// Every color of the palette.
    ResetPalette,
    Query(ThemeColor),
}

/// What an OSC asks for.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Osc {
    /// OSC 0 and 2. None for an empty title, which gives the tab its own back.
    Title(Option<String>),
    /// OSC 7.
    Directory(Directory),
    /// OSC 4, 10 to 12, 104 and 110 to 112.
    Colors(Vec<ColorChange>, End),
    /// OSC 8: the link that what is printed next is part of, or none.
    Link(Option<NewLink>),
    /// OSC 52.
    Clipboard(Clipboard),
}

/// What the OSC of `params`, ended with `end`, asks for. None for one Twominal does not act
/// on, or one that is not as it should be.
pub(crate) fn read(params: &[&[u8]], end: End) -> Option<Osc> {
    let (number, rest) = params.split_first()?;
    let number = self::number(number)?;
    // vte keeps no more than the limit, so one that long was cut short. A link cut short is not
    // made, and ends the one open, as any link that cannot be made does.
    if cut_short(params) {
        return (number == 8).then_some(Osc::Link(None));
    }
    let cut = params.len() == PARAMS;
    match number {
        0 | 2 => title(rest),
        4 => colors(palette(rest), end),
        7 if !cut => directory(rest),
        8 => link(rest, cut),
        number @ 10..=19 => colors(dynamic(number, rest), end),
        52 => clipboard(rest, end),
        104 => colors(reset_palette(rest), end),
        110 => colors(vec_of(ColorChange::Reset(ThemeColor::Foreground)), end),
        111 => colors(vec_of(ColorChange::Reset(ThemeColor::Background)), end),
        112 => colors(vec_of(ColorChange::Reset(ThemeColor::Cursor)), end),
        _ => None,
    }
}

/// Whether vte's buffer was full, so that the OSC of `params` may have been cut short.
pub(crate) fn cut_short(params: &[&[u8]]) -> bool {
    let length =
        params.iter().map(|param| param.len()).sum::<usize>() + params.len().saturating_sub(1);
    length >= REPORT_LIMIT
}

fn vec_of(change: ColorChange) -> Vec<ColorChange> {
    alloc::vec![change]
}

/// The parameters from `params` on, joined again by the `;` vte split them at.
fn join(params: &[&[u8]]) -> Vec<u8> {
    params.join(&b';')
}

/// The number a parameter holds: digits only.
fn number(param: &[u8]) -> Option<usize> {
    if param.is_empty() || param.len() > 5 || !param.iter().all(u8::is_ascii_digit) {
        return None;
    }
    Some(
        param
            .iter()
            .fold(0, |value, &digit| value * 10 + usize::from(digit - b'0')),
    )
}

/// OSC 0 and 2: the title, UTF-8 read as well as it can be, without control characters, and
/// no longer than [`TITLE_MOST`]. With no text at all the OSC asks for nothing.
fn title(rest: &[&[u8]]) -> Option<Osc> {
    if rest.is_empty() {
        return None;
    }
    let text = join(rest);
    let title: String = String::from_utf8_lossy(&text)
        .chars()
        .filter(|char| !char.is_control())
        .take(TITLE_MOST)
        .collect();
    Some(Osc::Title((!title.is_empty()).then_some(title)))
}

/// OSC 7: `file://host/path`. The path must start with `/`. `%XX` is read back as its byte in
/// the host and the path, and any other byte is kept as it came: hooks written by hand send
/// `$PWD` as it is.
fn directory(rest: &[&[u8]]) -> Option<Osc> {
    let url = join(rest);
    let scheme = b"file://";
    if url.len() < scheme.len() || !url[..scheme.len()].eq_ignore_ascii_case(scheme) {
        return None;
    }
    let url = &url[scheme.len()..];
    let slash = url.iter().position(|&byte| byte == b'/')?;
    let (host, path) = url.split_at(slash);
    Some(Osc::Directory(Directory {
        host: String::from_utf8_lossy(&unescape(host)).into_owned(),
        path: unescape(path),
    }))
}

/// `bytes` with each `%XX` read back as its byte. A `%` without two hex digits stays as it is.
fn unescape(bytes: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(bytes.len());
    let mut at = 0;
    while at < bytes.len() {
        if bytes[at] == b'%' {
            if let (Some(high), Some(low)) = (
                bytes.get(at + 1).and_then(|&digit| hex(digit)),
                bytes.get(at + 2).and_then(|&digit| hex(digit)),
            ) {
                out.push(high << 4 | low);
                at += 3;
                continue;
            }
        }
        out.push(bytes[at]);
        at += 1;
    }
    out
}

fn hex(digit: u8) -> Option<u8> {
    char::from(digit).to_digit(16).map(|value| value as u8)
}

/// OSC 8: `params ; URI`. An empty URI ends the link in use. A link that cannot be made, as
/// when its URI or `id` is too long or holds a byte outside space to `~`, or when vte may have
/// cut its URI short at its 16th parameter or at the end of its buffer, ends it too, so what
/// follows prints plain.
fn link(rest: &[&[u8]], cut: bool) -> Option<Osc> {
    let [options, uri @ ..] = rest else {
        return None;
    };
    if uri.is_empty() {
        return None;
    }
    let uri = join(uri);
    if uri.is_empty() {
        return Some(Osc::Link(None));
    }
    let printable = |bytes: &[u8]| bytes.iter().all(|byte| (b' '..=b'~').contains(byte));
    let id = options
        .split(|&byte| byte == b':')
        .find_map(|option| option.strip_prefix(b"id="))
        .filter(|id| !id.is_empty());
    let fits = !cut
        && uri.len() <= URI_MOST
        && printable(&uri)
        && id.is_none_or(|id| id.len() <= ID_MOST && printable(id));
    if !fits {
        return Some(Osc::Link(None));
    }
    // Both are ASCII, so these never replace anything.
    Some(Osc::Link(Some(NewLink {
        uri: String::from_utf8_lossy(&uri).into_owned(),
        id: id.map(|id| String::from_utf8_lossy(id).into_owned()),
    })))
}

fn colors(changes: Vec<ColorChange>, end: End) -> Option<Osc> {
    (!changes.is_empty()).then_some(Osc::Colors(changes, end))
}

/// OSC 4: pairs of a color's number and `?` or a color. A number that is not one of the
/// palette's ends them, as in xterm. A color that cannot be read changes nothing.
fn palette(rest: &[&[u8]]) -> Vec<ColorChange> {
    let mut changes = Vec::new();
    for pair in rest.chunks_exact(2) {
        let Some(index) = number(pair[0]).and_then(|index| u8::try_from(index).ok()) else {
            break;
        };
        changes.extend(change(ThemeColor::Palette(index), pair[1]));
    }
    changes
}

/// OSC 10 to 19: `?` or a color for the dynamic color `number`, then for each one after it, as
/// in xterm. Of them only the foreground, the background and the cursor are the theme's.
fn dynamic(number: usize, rest: &[&[u8]]) -> Vec<ColorChange> {
    let mut changes = Vec::new();
    for (offset, spec) in rest.iter().enumerate() {
        let color = match number + offset {
            10 => ThemeColor::Foreground,
            11 => ThemeColor::Background,
            12 => ThemeColor::Cursor,
            _ => continue,
        };
        changes.extend(change(color, spec));
    }
    changes
}

/// A question about `color` for `?`, or a change to the color `spec` names.
fn change(color: ThemeColor, spec: &[u8]) -> Option<ColorChange> {
    if spec == b"?" {
        Some(ColorChange::Query(color))
    } else {
        parse_color(spec).map(|rgb| ColorChange::Set(color, rgb))
    }
}

/// OSC 104: the palette's colors given by number, or with none every one of them.
fn reset_palette(rest: &[&[u8]]) -> Vec<ColorChange> {
    if rest.iter().all(|param| param.is_empty()) {
        return vec_of(ColorChange::ResetPalette);
    }
    rest.iter()
        .filter_map(|&param| number(param).and_then(|index| u8::try_from(index).ok()))
        .map(|index| ColorChange::Reset(ThemeColor::Palette(index)))
        .collect()
}

/// The color `spec` names, as XParseColor reads it: `rgb:r/g/b` with one to four hex digits for
/// each, scaled to 16 bits, or `#` with three, six, nine or twelve, whose digits are the high
/// bits. The high 8 bits of each are kept. Names of colors, and the other forms, are not read.
fn parse_color(spec: &[u8]) -> Option<Rgb> {
    parse_color16(spec).map(|rgb| rgb.map(|component| (component >> 8) as u8))
}

/// The color `spec` names, in 16 bits each, as XParseColor reads it.
fn parse_color16(spec: &[u8]) -> Option<[u16; 3]> {
    if let Some(rest) = spec
        .get(..4)
        .filter(|prefix| prefix.eq_ignore_ascii_case(b"rgb:"))
        .map(|_| &spec[4..])
    {
        let mut parts = rest.split(|&byte| byte == b'/');
        let mut component = || -> Option<u16> {
            let digits = parts.next()?;
            if digits.is_empty() || digits.len() > 4 {
                return None;
            }
            let value = hex_value(digits)?;
            let most = (1u32 << (4 * digits.len())) - 1;
            Some((value * 0xffff / most) as u16)
        };
        let rgb = [component()?, component()?, component()?];
        return parts.next().is_none().then_some(rgb);
    }
    let digits = spec.strip_prefix(b"#")?;
    if !matches!(digits.len(), 3 | 6 | 9 | 12) {
        return None;
    }
    let each = digits.len() / 3;
    let mut rgb = [0; 3];
    for (component, chunk) in rgb.iter_mut().zip(digits.chunks(each)) {
        *component = (hex_value(chunk)? << (16 - 4 * each)) as u16;
    }
    Some(rgb)
}

/// The value of up to eight hex digits.
fn hex_value(digits: &[u8]) -> Option<u32> {
    digits
        .iter()
        .try_fold(0, |value, &digit| Some(value << 4 | u32::from(hex(digit)?)))
}

/// OSC 52: `targets ; data`. `c`, `s` and no target name the clipboard, and `p` the primary
/// selection. `?` asks for the first of them. Other data is base64, and data that is not, as an
/// empty one, clears them, as in xterm.
fn clipboard(rest: &[&[u8]], end: End) -> Option<Osc> {
    let [targets, data @ ..] = rest else {
        return None;
    };
    if data.is_empty() {
        return None;
    }
    let named: Vec<Selection> = if targets.is_empty() {
        alloc::vec![Selection::Clipboard]
    } else {
        targets
            .iter()
            .filter_map(|target| match target {
                b'c' | b's' => Some(Selection::Clipboard),
                b'p' => Some(Selection::Primary),
                _ => None,
            })
            .collect()
    };
    let &first = named.first()?;
    if data == [b"?"] {
        return Some(Osc::Clipboard(Clipboard::Read(ClipboardQuery {
            from: first,
            end,
        })));
    }
    // A `;` is not base64, so data that held one clears.
    let text = match data {
        [data] => decode(data)
            .map(|bytes| String::from_utf8_lossy(&bytes).into_owned())
            .unwrap_or_default(),
        _ => String::new(),
    };
    Some(Osc::Clipboard(Clipboard::Write {
        clipboard: named.contains(&Selection::Clipboard),
        primary: named.contains(&Selection::Primary),
        text,
    }))
}

const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

/// `bytes` in base64, with the `=` that pads it to whole groups of four.
pub(crate) fn encode(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for group in bytes.chunks(3) {
        let value = group.iter().enumerate().fold(0u32, |value, (at, &byte)| {
            value | u32::from(byte) << (16 - 8 * at)
        });
        for at in 0..4 {
            if at <= group.len() {
                out.push(char::from(
                    ALPHABET[(value >> (18 - 6 * at) & 0x3f) as usize],
                ));
            } else {
                out.push('=');
            }
        }
    }
    out
}

/// The bytes `text` holds in base64, padded with `=` or not. None if it is not base64.
pub(crate) fn decode(text: &[u8]) -> Option<Vec<u8>> {
    let padded = text.len();
    let text = text
        .strip_suffix(b"==")
        .or_else(|| text.strip_suffix(b"="))
        .unwrap_or(text);
    // Padding only completes the last group of four, and no group has one character alone.
    if text.len() % 4 == 1 || (padded != text.len() && !padded.is_multiple_of(4)) {
        return None;
    }
    let mut out = Vec::with_capacity(text.len() / 4 * 3 + 2);
    for group in text.chunks(4) {
        let mut value = 0u32;
        for (at, &char) in group.iter().enumerate() {
            let digit = ALPHABET.iter().position(|&letter| letter == char)?;
            value |= (digit as u32) << (18 - 6 * at);
        }
        for at in 0..group.len() - 1 {
            out.push((value >> (16 - 8 * at)) as u8);
        }
    }
    Some(out)
}
