//! The kitty keyboard protocol's flags (TERM-09): how a program asked to be told of keys.
//!
//! A program pushes flags with `CSI > flags u`, pops them with `CSI < n u`, changes the ones
//! in use with `CSI = flags ; mode u` and asks for them with `CSI ? u`. Each screen keeps a
//! stack of its own, so an editor on the alternate screen leaves the shell's flags alone.
//!
//! The protocol is kitty's, so kitty is the model, as xterm is for the rest: where the
//! specification says nothing, this does what kitty's `screen.c` does. The Input Handler reads
//! the flags and encodes keys by them (#20).

/// The flags of the kitty keyboard protocol's progressive enhancement. None of them means the
/// legacy encoding, as xterm's.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Keyboard(u8);

impl Keyboard {
    /// Esc, and keys with Ctrl, Alt or Super, are sent as escape codes no other key sends.
    pub const DISAMBIGUATE: Self = Self(1);
    /// Repeats and releases are reported, as well as presses.
    pub const EVENT_TYPES: Self = Self(2);
    /// The key's shifted form and its key on a US keyboard are reported with it.
    pub const ALTERNATE_KEYS: Self = Self(4);
    /// Every key is sent as an escape code, those that type text and the modifiers among them.
    pub const ALL_KEYS: Self = Self(8);
    /// The text a key types is reported with its escape code.
    pub const ASSOCIATED_TEXT: Self = Self(16);

    /// The flags of `bits`. Only seven bits are kept, as kitty keeps them, so a program that
    /// asks gets back what it set, flags not yet defined included.
    pub const fn new(bits: u8) -> Self {
        Self(bits & 0x7f)
    }

    pub const fn bits(self) -> u8 {
        self.0
    }

    /// Whether every flag of `flags` is set.
    pub const fn contains(self, flags: Self) -> bool {
        self.0 & flags.0 == flags.0
    }

    /// Whether keys are sent as xterm sends them: no flag is set.
    pub const fn is_legacy(self) -> bool {
        self.0 == 0
    }

    fn from_param(param: usize) -> Self {
        // Only the low seven bits are kept, so the rest may be cut off first.
        Self::new(param as u8)
    }
}

/// How many flags a screen's stack holds, as in kitty. A push onto a full stack drops the
/// oldest, so no program can make it grow.
pub(crate) const DEPTH: usize = 8;

/// The flags a program pushed on one screen, the newest last. With none, the flags are none.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct Stack {
    entries: [Keyboard; DEPTH],
    len: usize,
}

impl Stack {
    /// The flags in use: the newest pushed.
    pub(crate) fn current(&self) -> Keyboard {
        self.len
            .checked_sub(1)
            .map_or(Keyboard::default(), |top| self.entries[top])
    }

    #[cfg(any(test, fuzzing))]
    pub(crate) fn len(&self) -> usize {
        self.len
    }

    /// `CSI > flags u`. A full stack drops its oldest flags.
    pub(crate) fn push(&mut self, flags: usize) {
        if self.len == DEPTH {
            self.entries.copy_within(1.., 0);
            self.len -= 1;
        }
        self.entries[self.len] = Keyboard::from_param(flags);
        self.len += 1;
    }

    /// `CSI < count u`. Popping more than there are empties the stack, and then no flag is set.
    pub(crate) fn pop(&mut self, count: usize) {
        self.len -= count.min(self.len);
    }

    /// `CSI = flags ; how u`: 1 makes the flags in use `flags`, 2 sets those of `flags` too,
    /// 3 resets them. Anything else does nothing. On an empty stack it acts on a first entry,
    /// as kitty does.
    pub(crate) fn set(&mut self, flags: usize, how: usize) {
        let flags = Keyboard::from_param(flags).0;
        let current = self.current().0;
        let changed = match how {
            1 => flags,
            2 => current | flags,
            3 => current & !flags,
            _ => return,
        };
        if self.len == 0 {
            self.len = 1;
        }
        self.entries[self.len - 1] = Keyboard(changed);
    }
}
