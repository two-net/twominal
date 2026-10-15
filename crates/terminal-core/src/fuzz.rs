//! What the fuzz targets check (REL-02, ARC-03, SEC-05). It is here and not in the fuzz crate
//! so that `cargo test` checks it too: libFuzzer on output it searches for, the tests on
//! output they make up.
//!
//! An input's first two bytes are the size, the next eight the seed that says where the
//! output is cut, when the grid is resized and how many rows the scrollback keeps, and the
//! rest is the output. The size and the scrollback stand for the window and the settings,
//! which Twominal trusts, so they are kept small enough to fuzz quickly: a resize reflows
//! every row the scrollback keeps. Only the output is hostile.

use alloc::vec::Vec;

use crate::{Event, Size, TerminalCore};

/// The nonce of the reports in fuzzed output, so that reports are read and not only skipped.
pub const NONCE: &str = "n1";

/// The largest grid fuzzed. A sequence does at most a screenful of work for each of its
/// parameters, so a larger grid only makes each input slower.
const MAX_COLS: u16 = 160;
const MAX_ROWS: u16 = 60;

/// The scrollback's limits fuzzed: none, one row, a few, and more than a screenful.
const LIMITS: [usize; 4] = [0, 1, 7, 64];

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
/// reply that holds anything the program wrote (SEC-05). The output comes in pieces, with the
/// grid resized, the scrollback's limit changed or Clear between some of them.
pub fn interpret(data: &[u8]) {
    let input = Input::read(data);
    let mut random = Random::new(input.seed);
    let mut core = input.core();
    check(&core);
    let mut rest = input.output;
    while !rest.is_empty() {
        let (piece, after) = rest.split_at(random.cut(rest.len()));
        for event in core.interpret(piece) {
            if let Event::Reply(reply) = event {
                assert!(is_reply(&reply), "a reply of {reply:?}");
            }
        }
        check(&core);
        match random.below(32) {
            0..4 => core.resize(random.size()),
            4 => core.set_scrollback(random.limit()),
            5 => core.clear(),
            _ => {}
        }
        check(&core);
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
/// events it gave.
fn run(input: Input, mut cut: impl FnMut(usize) -> usize) -> (TerminalCore, Vec<Event>) {
    let mut core = input.core();
    let mut events = Vec::new();
    let mut rest = input.output;
    while !rest.is_empty() {
        let (piece, after) = rest.split_at(cut(rest.len()));
        events.extend(core.interpret(piece));
        rest = after;
    }
    (core, events)
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
