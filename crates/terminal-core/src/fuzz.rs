//! What the fuzz targets check (REL-02, ARC-03, SEC-05). It is here and not in the fuzz crate
//! so that `cargo test` checks it too: libFuzzer on output it searches for, the tests on
//! output they make up.
//!
//! An input's first two bytes are the size, the next eight the seed that says where the
//! output is cut, when the grid is resized and how many rows the scrollback keeps, and the
//! rest is the output. The size and the scrollback stand for the window and the settings,
//! which Twominal trusts, so they are kept small enough to fuzz quickly: a resize reflows
//! every row the scrollback keeps. Only the output is hostile. The seed also says where the
//! user selects (TERM-17), and copies, and what the user looks for (TERM-18).

use alloc::string::String;
use alloc::vec::Vec;

use crate::{Clipboard, Event, Place, Search, Selecting, Size, TerminalCore};

/// The nonce of the reports in fuzzed output, so that reports are read and not only skipped.
pub const NONCE: &str = "n1";

/// The largest grid fuzzed. A sequence does at most a screenful of work for each of its
/// parameters, so a larger grid only makes each input slower.
const MAX_COLS: u16 = 160;
const MAX_ROWS: u16 = 60;

/// The scrollback's limits fuzzed: none, one row, a few, and more than a screenful.
const LIMITS: [usize; 4] = [0, 1, 7, 64];

/// What the user may look for, and whether it is a regular expression: text, patterns, one that
/// is no pattern, and what fuzzed output is full of, so that the most matches kept is reached.
const SEARCHES: [(&str, bool); 12] = [
    ("a", false),
    ("ab", false),
    (" ", false),
    ("e\u{301}", false),
    ("\u{4e2d}", false),
    ("[", false),
    ("[a-z]+", true),
    ("^.", true),
    (".$", true),
    (r"\w+\s\w", true),
    ("a.*b", true),
    ("(", true),
];

/// xorshift64: the same numbers for the same seed, on every machine.
pub struct Random(u64);

impl Random {
    /// Any seed, 0 included.
    pub fn new(seed: u64) -> Self {
        Self(seed.wrapping_mul(0x2545_f491_4f6c_dd1d) | 1)
    }

    pub fn number(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }

    /// A number from 0 to `bound`, `bound` excluded.
    pub fn below(&mut self, bound: usize) -> usize {
        (self.number() % bound as u64) as usize
    }

    /// A size from zero, which is made one, to the largest fuzzed.
    fn size(&mut self) -> Size {
        Size {
            cols: self.below(usize::from(MAX_COLS) + 1) as u16,
            rows: self.below(usize::from(MAX_ROWS) + 1) as u16,
        }
    }

    /// One of the scrollback's limits fuzzed.
    fn limit(&mut self) -> usize {
        LIMITS[self.below(LIMITS.len())]
    }

    /// A place on the rows `core` has, or a little past them, as a drag past the edge gives.
    fn place(&mut self, core: &TerminalCore) -> Place {
        let scrollback = core.scrollback();
        let size = core.size();
        let rows = scrollback.len() + usize::from(size.rows) + 4;
        Place {
            row: scrollback.start() as i64 - 2 + self.below(rows) as i64,
            col: self.below(usize::from(size.cols) + 4) as i32 - 2,
            right_half: self.below(2) == 1,
        }
    }

    /// How to select: by characters, words or lines, or as a block.
    fn selecting(&mut self) -> Selecting {
        [
            Selecting::Characters,
            Selecting::Words,
            Selecting::Lines,
            Selecting::Block,
        ][self.below(4)]
    }

    /// Something to look for, letter case counting or not.
    fn search(&mut self) -> Search {
        let (query, regex) = SEARCHES[self.below(SEARCHES.len())];
        Search {
            query: query.into(),
            case: self.below(2) == 1,
            regex,
        }
    }

    /// A row of `core` to look from: any there is, or one past them.
    fn from(&mut self, core: &TerminalCore) -> u64 {
        let scrollback = core.scrollback();
        let rows = scrollback.len() + usize::from(core.size().rows) + 1;
        scrollback.start() + self.below(rows) as u64
    }

    /// How much of the `left` bytes to interpret next: one byte, a few, many, or all.
    fn cut(&mut self, left: usize) -> usize {
        let length = match self.below(4) {
            0 => 1,
            1 => 1 + self.below(8),
            2 => 1 + self.below(4096),
            _ => left,
        };
        length.min(left)
    }
}

/// The input of a fuzz target, read as the start of this file says. Every input is one, however
/// short: missing bytes are zero.
#[derive(Clone, Copy)]
struct Input<'a> {
    size: Size,
    seed: u64,
    output: &'a [u8],
}

impl<'a> Input<'a> {
    fn read(data: &'a [u8]) -> Self {
        let byte = |at: usize| data.get(at).copied().unwrap_or(0);
        let mut seed = [0; 8];
        for (at, byte_of_seed) in seed.iter_mut().enumerate() {
            *byte_of_seed = byte(2 + at);
        }
        Self {
            size: Size {
                cols: u16::from(byte(0)) % (MAX_COLS + 1),
                rows: u16::from(byte(1)) % (MAX_ROWS + 1),
            },
            seed: u64::from_le_bytes(seed),
            output: data.get(10..).unwrap_or_default(),
        }
    }

    /// A terminal of the input's size, which keeps as many rows of scrollback as its seed's
    /// top bits say.
    fn core(&self) -> TerminalCore {
        let mut core = TerminalCore::new(self.size, Some(NONCE));
        core.set_scrollback(LIMITS[(self.seed >> 62) as usize]);
        core
    }
}

/// The input for a fuzz target that gives `output` to a terminal of `size`, made no larger than
/// the largest fuzzed, cut and resized as `seed` says.
pub fn input(size: Size, seed: u64, output: &[u8]) -> Vec<u8> {
    let cols = size.cols.min(MAX_COLS) as u8;
    let rows = size.rows.min(MAX_ROWS) as u8;
    [&[cols, rows][..], &seed.to_le_bytes(), output].concat()
}

/// Hostile output never breaks the rules the Terminal Core keeps (REL-02), and never gets a
/// reply that holds anything the program wrote (SEC-05): neither the core's own, nor the answers
/// to questions about colors and the clipboard, whatever the theme and the clipboard hold. The
/// output comes in pieces, with the grid resized, the scrollback's limit changed, Clear or a
/// selection made or moved between some of them. What is selected copies as text with no control
/// character but the line feed, and no space at the end of a line, and a selection that output
/// left copies what it did before, but for the rows dropped past the scrollback's limit, which
/// leave its start (TERM-17). A search is made, moved or ended between pieces too, and the
/// matches, brought up to date after some of them, are those a search of the rows as they are
/// finds (TERM-18).
pub fn interpret(data: &[u8]) {
    let input = Input::read(data);
    let mut random = Random::new(input.seed);
    let mut core = input.core();
    check(&core);
    let mut rest = input.output;
    while !rest.is_empty() {
        let (piece, after) = rest.split_at(random.cut(rest.len()));
        let copied = core.copy();
        let events = core.interpret(piece);
        if let (Some(before), Some(now)) = (&copied, core.copy()) {
            assert!(before.ends_with(&now), "{before:?} became {now:?}");
        }
        for event in events {
            match event {
                Event::Reply(reply) => assert!(is_reply(&reply), "a reply of {reply:?}"),
                Event::Color(query) => {
                    let theme = random.number().to_le_bytes();
                    let reply = query.reply([theme[0], theme[1], theme[2]]);
                    assert!(is_color_reply(&reply), "a reply of {reply:?}");
                }
                Event::Clipboard(clipboard) => {
                    if let Clipboard::Read(query) = *clipboard {
                        // The program may have put anything there.
                        let reply = query.reply(&String::from_utf8_lossy(piece));
                        assert!(is_clipboard_reply(&reply), "a reply of {reply:?}");
                    }
                }
                _ => {}
            }
        }
        check(&core);
        if random.below(2) == 0 {
            refind(&mut core);
        }
        match random.below(32) {
            0..4 => core.resize(random.size()),
            4 => core.set_scrollback(random.limit()),
            5 => core.clear(),
            6 => {
                let (at, by) = (random.place(&core), random.selecting());
                core.select(at, by);
            }
            7 => core.extend(random.place(&core)),
            8 => core.select_all(),
            9..11 => {
                let (search, from) = (random.search(), random.from(&core));
                core.find(Some(search), from);
            }
            11 => core.find_next(random.below(2) == 0),
            12 => core.find(None, 0),
            _ => {}
        }
        check(&core);
        if let Some(text) = core.copy() {
            assert!(is_copy(&text), "copied {text:?}");
        }
        rest = after;
    }
}

/// The same output gives the same state and the same events, whether it comes whole, a byte
/// at a time or in pieces (ARC-03).
pub fn split(data: &[u8]) {
    let input = Input::read(data);
    let mut random = Random::new(input.seed);
    let (whole, events) = run(input, |left| left);
    for (core, cut_events) in [run(input, |_| 1), run(input, |left| random.cut(left))] {
        assert!(
            core.terminal == whole.terminal,
            "the state depends on the cuts"
        );
        assert_eq!(core.held, whole.held, "the held bytes depend on the cuts");
        assert_eq!(cut_events, events, "the events depend on the cuts");
    }
}

/// A terminal that has interpreted `input`'s output, in pieces as long as `cut` says, and the
/// events it gave. At a byte the seed chooses, the same in every run, the user selects and looks
/// for something, so the output after it ends the selection, or keeps it, and changes the
/// matches the same way however it is cut. They are brought up to date at the end.
fn run(input: Input, mut cut: impl FnMut(usize) -> usize) -> (TerminalCore, Vec<Event>) {
    let mut core = input.core();
    let mut events = Vec::new();
    let mut chosen = Random::new(!input.seed);
    let (before, after) = input.output.split_at(chosen.below(input.output.len() + 1));
    for (part, selects) in [(before, true), (after, false)] {
        let mut rest = part;
        while !rest.is_empty() {
            let (piece, after) = rest.split_at(cut(rest.len()));
            events.extend(core.interpret(piece));
            rest = after;
        }
        if selects {
            let (at, by) = (chosen.place(&core), chosen.selecting());
            core.select(at, by);
            core.extend(chosen.place(&core));
            let (search, from) = (chosen.search(), chosen.from(&core));
            core.find(Some(search), from);
        }
    }
    core.refind();
    (core, events)
}

/// Whether `text` is what copying may give: no control character but the line feed, which
/// no cell holds, and no space at the end of a line.
fn is_copy(text: &str) -> bool {
    !text.chars().any(|char| char.is_control() && char != '\n')
        && !text.split('\n').any(|line| line.ends_with(' '))
}

/// Brings the matches up to date, and panics unless they are what a search of the rows finds.
fn refind(core: &mut TerminalCore) {
    core.refind();
    assert!(
        core.terminal.found_fresh(),
        "the matches are not what a search finds: {:?}",
        core.terminal
    );
}

/// Panics if `core` breaks a rule the Terminal Core relies on.
fn check(core: &TerminalCore) {
    core.terminal.check();
    // No more than a character's first three bytes.
    assert!(core.held.len() < 4, "{:?} held back", core.held);
}

/// Whether `reply` is built as every reply is: `CSI`, perhaps `?` or `>`, digits and `;`, and
/// the final byte of DA, DSR or CPR, `$y` for DECRQM, or `u` after `?` for the kitty keyboard
/// protocol's flags. Only those are answered, and only with fixed text and numbers (SEC-05).
fn is_reply(reply: &[u8]) -> bool {
    let Some(rest) = reply.strip_prefix(b"\x1b[") else {
        return false;
    };
    let private = rest.strip_prefix(b"?");
    let rest = private.or_else(|| rest.strip_prefix(b">")).unwrap_or(rest);
    let numbers = match rest.split_last() {
        Some((b'y', rest)) => rest.strip_suffix(b"$"),
        Some((b'c' | b'n' | b'R', rest)) => Some(rest),
        Some((b'u', rest)) if private.is_some() => Some(rest),
        _ => None,
    };
    numbers.is_some_and(|numbers| {
        numbers
            .iter()
            .all(|&byte| byte.is_ascii_digit() || byte == b';')
    })
}

/// `reply` without the BEL or ST it ends with, if it ends with one.
fn ended(reply: &[u8]) -> Option<&[u8]> {
    reply
        .strip_suffix(b"\x07")
        .or_else(|| reply.strip_suffix(b"\x1b\\"))
}

/// Whether `reply` is the answer to a question about a color: `OSC 4 ; n`, `OSC 10`, 11 or 12,
/// then `; rgb:` and three components of four hex digits, ended by BEL or ST.
fn is_color_reply(reply: &[u8]) -> bool {
    let Some(rest) = reply.strip_prefix(b"\x1b]").and_then(ended) else {
        return false;
    };
    let Some((number, color)) = rest.split_once_at(b";rgb:") else {
        return false;
    };
    let named = match number.strip_prefix(b"4;") {
        Some(index) => {
            !index.is_empty() && index.len() <= 3 && index.iter().all(u8::is_ascii_digit)
        }
        None => matches!(number, b"10" | b"11" | b"12"),
    };
    let components: Vec<&[u8]> = color.split(|&byte| byte == b'/').collect();
    named
        && components.len() == 3
        && components
            .iter()
            .all(|component| component.len() == 4 && component.iter().all(u8::is_ascii_hexdigit))
}

/// Whether `reply` is the answer to a question about the clipboard: `OSC 52 ; c` or `p`, `;`
/// and base64, ended by BEL or ST.
fn is_clipboard_reply(reply: &[u8]) -> bool {
    let Some(rest) = reply.strip_prefix(b"\x1b]52;").and_then(ended) else {
        return false;
    };
    let Some(data) = rest
        .strip_prefix(b"c;")
        .or_else(|| rest.strip_prefix(b"p;"))
    else {
        return false;
    };
    data.iter()
        .all(|&byte| byte.is_ascii_alphanumeric() || matches!(byte, b'+' | b'/' | b'='))
}

trait SplitOnce {
    /// The bytes before the first `separator` and those after it.
    fn split_once_at(&self, separator: &[u8]) -> Option<(&[u8], &[u8])>;
}

impl SplitOnce for [u8] {
    fn split_once_at(&self, separator: &[u8]) -> Option<(&[u8], &[u8])> {
        let at = self
            .windows(separator.len())
            .position(|window| window == separator)?;
        Some((&self[..at], &self[at + separator.len()..]))
    }
}
