//! The bytes a program reads for a key, a paste or a change of focus (TERM-08, TERM-09,
//! TERM-11).
//!
//! With no kitty keyboard flag set, keys are encoded as xterm encodes them for
//! `TERM=xterm-256color`. With flags set, as kitty encodes them: this follows kitty's
//! `key_encoding.c`, the protocol's reference. DESIGN.md §3 lists where either is not followed.

use std::fmt::Write as _;

use terminal_core::{Keyboard, Modes};

use super::key::{Key, Mods, Named, Pad, Press};
use super::Kind;

const ESC: u8 = 0x1b;

/// The numbers of F5 to F12 in `CSI n ~`.
const TILDES: [u32; 8] = [15, 17, 18, 19, 20, 21, 23, 24];

/// The bytes for `press`, given the modes the program set. Empty when the program cannot be
/// told of it.
pub(crate) fn key(press: &Press, modes: &Modes) -> Vec<u8> {
    if modes.keyboard.is_legacy() {
        legacy(press, modes)
    } else {
        kitty(press, modes.keyboard, modes.application_cursor)
    }
}

/// `text`, pasted (TERM-11). A paste never types a control character, so it cannot end a
/// bracketed paste early or act on the program. Line breaks become CR, as Enter types, and
/// every other control character but tab a space, as xterm's `disallowedPasteControls` does
/// for the ones that matter.
pub(crate) fn paste(text: &str, bracketed: bool) -> Vec<u8> {
    let mut bytes = Vec::with_capacity(text.len() + 12);
    if bracketed {
        bytes.extend_from_slice(b"\x1b[200~");
    }
    let mut chars = text.chars().peekable();
    while let Some(char) = chars.next() {
        match char {
            '\r' => {
                chars.next_if_eq(&'\n');
                bytes.push(b'\r');
            }
            '\n' => bytes.push(b'\r'),
            '\t' => bytes.push(b'\t'),
            // C0, DEL and C1.
            char if char.is_control() => bytes.push(b' '),
            char => push_char(&mut bytes, char),
        }
    }
    if bracketed {
        bytes.extend_from_slice(b"\x1b[201~");
    }
    bytes
}

/// That the window gained or lost focus, for a program that asked (TERM-11).
pub(crate) fn focus(focused: bool) -> &'static [u8] {
    if focused {
        b"\x1b[I"
    } else {
        b"\x1b[O"
    }
}

// ---------------------------------------------------------------------------------------
// xterm
// ---------------------------------------------------------------------------------------

fn legacy(press: &Press, modes: &Modes) -> Vec<u8> {
    if press.kind == Kind::Release {
        return Vec::new();
    }
    let mods = press.mods;
    match press.key {
        Key::Char(_) => legacy_char(press),
        Key::Pad(pad) => {
            let plain = !mods.has(Mods::CTRL | Mods::ALT | Mods::SUPER);
            // xterm ignores DECKPAM for a key that types a character while NumLock is on.
            let numbers = pad.char().is_some() && mods.has(Mods::NUM_LOCK);
            match pad_final(pad) {
                Some(final_byte) if modes.application_keypad && plain && !numbers => {
                    vec![ESC, b'O', final_byte]
                }
                _ => match pad_named(pad) {
                    Some(named) => legacy_named(named, mods, modes),
                    None => legacy_char(press),
                },
            }
        }
        Key::Named(named) => legacy_named(named, mods, modes),
    }
}

/// A key that types: what it typed, Ctrl and Alt applied.
fn legacy_char(press: &Press) -> Vec<u8> {
    let mods = press.mods;
    // xterm's Meta has no encoding for characters. Terminal.app and iTerm2 send nothing too.
    if mods.has(Mods::SUPER) {
        return Vec::new();
    }
    let ctrl = mods.has(Mods::CTRL);
    let alt = mods.has(Mods::ALT);
    let mut chars = press.typed.chars();
    let (Some(typed), None) = (chars.next(), chars.next()) else {
        // Several characters, from a layout that types them at once.
        let mut bytes = if alt { vec![ESC] } else { Vec::new() };
        bytes.extend_from_slice(press.typed.as_bytes());
        return bytes;
    };
    // With Ctrl or Alt, a key that types no ASCII on this layout is the key at its place on a
    // US keyboard, so Ctrl+C and Alt+B work on Cyrillic, as in kitty.
    let char = match press.us {
        Some((base, shifted)) if (ctrl || alt) && !typed.is_ascii() => {
            if mods.has(Mods::SHIFT) {
                shifted
            } else {
                base
            }
        }
        _ => typed,
    };
    let mut bytes = if alt { vec![ESC] } else { Vec::new() };
    match ctrl.then(|| control(char)).flatten() {
        Some(byte) => bytes.push(byte),
        None => push_char(&mut bytes, char),
    }
    bytes
}

/// What Ctrl makes of `char`, as Xlib's `XkbToControl` has it for xterm. `None` leaves it as
/// it is.
fn control(char: char) -> Option<u8> {
    Some(match char {
        '@'..='~' | ' ' => char as u8 & 0x1f,
        '2' => 0,
        '3'..='7' => char as u8 - b'3' + 0x1b,
        '8' => 0x7f,
        '/' => 0x1f,
        _ => return None,
    })
}

fn legacy_named(named: Named, mods: Mods, modes: &Modes) -> Vec<u8> {
    let held = mods.held();
    let alt = mods.has(Mods::ALT);
    let control_key = |bytes: &[u8]| {
        // Cmd has no encoding for these either.
        if mods.has(Mods::SUPER) {
            return Vec::new();
        }
        let mut sent = if alt { vec![ESC] } else { Vec::new() };
        sent.extend_from_slice(bytes);
        sent
    };
    match named {
        Named::Enter if modes.newline => control_key(b"\r\n"),
        Named::Enter => control_key(b"\r"),
        Named::Tab if mods.has(Mods::SHIFT) => control_key(b"\x1b[Z"),
        Named::Tab => control_key(b"\t"),
        Named::Backspace if mods.has(Mods::CTRL) => control_key(b"\x08"),
        Named::Backspace => control_key(b"\x7f"),
        Named::Escape => control_key(b"\x1b"),
        Named::Up => cursor_key(b'A', held, modes),
        Named::Down => cursor_key(b'B', held, modes),
        Named::Right => cursor_key(b'C', held, modes),
        Named::Left => cursor_key(b'D', held, modes),
        Named::Begin => cursor_key(b'E', held, modes),
        Named::End => cursor_key(b'F', held, modes),
        Named::Home => cursor_key(b'H', held, modes),
        Named::Insert => tilde(2, held),
        Named::Delete => tilde(3, held),
        Named::PageUp => tilde(5, held),
        Named::PageDown => tilde(6, held),
        // xterm's F16, as kitty has it.
        Named::Menu => tilde(29, held),
        Named::F(n) => function_key(n, held),
        // Keys xterm sends nothing for. kitty sends them with the protocol's numbers, which
        // its specification lets a terminal leave out.
        Named::CapsLock
        | Named::ScrollLock
        | Named::NumLock
        | Named::PrintScreen
        | Named::Pause
        | Named::Media(_)
        | Named::Modifier(_) => Vec::new(),
    }
}

/// The arrows, Home, End and Begin: `CSI final`, or `SS3 final` in DECCKM, or `CSI 1;m final`
/// with modifiers.
fn cursor_key(final_byte: u8, held: u8, modes: &Modes) -> Vec<u8> {
    if held == 0 {
        let introducer = if modes.application_cursor { b'O' } else { b'[' };
        vec![ESC, introducer, final_byte]
    } else {
        let mut bytes = format!("\x1b[1;{}", held + 1).into_bytes();
        bytes.push(final_byte);
        bytes
    }
}

/// `CSI n ~`, or `CSI n;m ~` with modifiers.
fn tilde(number: u32, held: u8) -> Vec<u8> {
    if held == 0 {
        format!("\x1b[{number}~").into_bytes()
    } else {
        format!("\x1b[{number};{}~", held + 1).into_bytes()
    }
}

/// F1 to F35. F13 to F24 are Shift and F1 to F12, and F25 to F35 Ctrl and F1 to F11, as
/// `xterm-256color`'s terminfo has them.
fn function_key(n: u8, held: u8) -> Vec<u8> {
    let (n, held) = match n {
        13..=24 => (n - 12, held | Mods::SHIFT),
        25.. => (n - 24, held | Mods::CTRL),
        _ => (n, held),
    };
    match n {
        1..=4 => {
            let final_byte = b"PQRS"[usize::from(n - 1)];
            if held == 0 {
                vec![ESC, b'O', final_byte]
            } else {
                let mut bytes = format!("\x1b[1;{}", held + 1).into_bytes();
                bytes.push(final_byte);
                bytes
            }
        }
        _ => tilde(TILDES[usize::from(n - 5)], held),
    }
}

/// The final byte of the keypad's key in DECKPAM, after SS3.
fn pad_final(pad: Pad) -> Option<u8> {
    Some(match pad {
        Pad::Digit(digit) => b'p' + digit,
        Pad::Decimal => b'n',
        Pad::Divide => b'o',
        Pad::Multiply => b'j',
        Pad::Subtract => b'm',
        Pad::Add => b'k',
        Pad::Enter => b'M',
        Pad::Equal => b'X',
        Pad::Separator => b'l',
        _ => return None,
    })
}

/// The key a keypad key stands for when it types nothing.
fn pad_named(pad: Pad) -> Option<Named> {
    Some(match pad {
        Pad::Enter => Named::Enter,
        Pad::Left => Named::Left,
        Pad::Right => Named::Right,
        Pad::Up => Named::Up,
        Pad::Down => Named::Down,
        Pad::PageUp => Named::PageUp,
        Pad::PageDown => Named::PageDown,
        Pad::Home => Named::Home,
        Pad::End => Named::End,
        Pad::Insert => Named::Insert,
        Pad::Delete => Named::Delete,
        Pad::Begin => Named::Begin,
        _ => return None,
    })
}

// ---------------------------------------------------------------------------------------
// kitty
// ---------------------------------------------------------------------------------------

/// What a key is in kitty's encoder: a functional key, or a key that types a character.
#[derive(Clone, Copy)]
enum Kitty {
    Functional(Functional),
    Char(char),
}

/// A functional key, keypad keys among them.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Functional {
    Named(Named),
    Pad(Pad),
}

/// kitty's `encode_glfw_key_event`.
fn kitty(press: &Press, flags: Keyboard, cursor_key_mode: bool) -> Vec<u8> {
    let disambiguate = flags.contains(Keyboard::DISAMBIGUATE);
    let all_keys = flags.contains(Keyboard::ALL_KEYS);
    if !all_keys && press.is_modifier() {
        return Vec::new();
    }
    let text = press.text();
    let key = match press.key {
        Key::Char(char) => Kitty::Char(char),
        Key::Named(named) => Kitty::Functional(Functional::Named(named)),
        // The keypad is the keys it stands for until a flag tells them apart.
        Key::Pad(pad) if !disambiguate && !all_keys => match (pad_named(pad), pad.char()) {
            (Some(named), _) => Kitty::Functional(Functional::Named(named)),
            (None, Some(char)) => Kitty::Char(char),
            (None, None) => Kitty::Functional(Functional::Pad(pad)),
        },
        Key::Pad(pad) => Kitty::Functional(Functional::Pad(pad)),
    };
    if !all_keys && !text.is_empty() && press.kind != Kind::Release {
        return text.as_bytes().to_vec();
    }
    if !flags.contains(Keyboard::EVENT_TYPES) && press.kind == Kind::Release {
        return Vec::new();
    }
    match key {
        Kitty::Functional(functional) => {
            kitty_functional(functional, press, flags, cursor_key_mode)
        }
        Kitty::Char(char) => kitty_char(char, press, flags),
    }
}

/// The fields of kitty's `CSI … u` and its letter and `~` forms.
struct Fields<'a> {
    number: u32,
    shifted: Option<char>,
    base: Option<char>,
    alternates: bool,
    mods: u8,
    kind: Kind,
    actions: bool,
    text: &'a str,
    final_byte: char,
}

impl<'a> Fields<'a> {
    /// kitty's `init_encoding_data`.
    fn new(number: u32, press: &'a Press, flags: Keyboard) -> Self {
        let shifted = press.shifted();
        let base = press.base();
        let shift = press.mods.has(Mods::SHIFT);
        let alternates = flags.contains(Keyboard::ALTERNATE_KEYS)
            && ((shifted.is_some() && shift) || base.is_some());
        let text = if flags.contains(Keyboard::ASSOCIATED_TEXT) {
            press.text()
        } else {
            ""
        };
        Self {
            number,
            shifted: shifted.filter(|_| alternates && shift),
            base: base.filter(|_| alternates),
            alternates,
            mods: press.mods.0,
            kind: press.kind,
            actions: flags.contains(Keyboard::EVENT_TYPES) && press.kind != Kind::Press,
            text,
            final_byte: 'u',
        }
    }

    /// kitty's `serialize`.
    fn serialize(&self) -> Vec<u8> {
        let mut out = String::from("\x1b[");
        let second = self.mods != 0 || self.actions;
        let third = !self.text.is_empty();
        if self.number != 1 || self.alternates || second || third {
            let _ = write!(out, "{}", self.number);
        }
        if self.alternates {
            out.push(':');
            if let Some(shifted) = self.shifted {
                let _ = write!(out, "{}", u32::from(shifted));
            }
            if let Some(base) = self.base {
                let _ = write!(out, ":{}", u32::from(base));
            }
        }
        if second || third {
            out.push(';');
            if second {
                let _ = write!(out, "{}", u16::from(self.mods) + 1);
            }
            if self.actions {
                let action = match self.kind {
                    Kind::Press => 1,
                    Kind::Repeat => 2,
                    Kind::Release => 3,
                };
                let _ = write!(out, ":{action}");
            }
        }
        if third {
            for (index, char) in self.text.chars().enumerate() {
                let separator = if index == 0 { ';' } else { ':' };
                let _ = write!(out, "{separator}{}", u32::from(char));
            }
        }
        out.push(self.final_byte);
        out.into_bytes()
    }
}

/// kitty's `encode_function_key`.
fn kitty_functional(
    key: Functional,
    press: &Press,
    flags: Keyboard,
    cursor_key_mode: bool,
) -> Vec<u8> {
    let disambiguate = flags.contains(Keyboard::DISAMBIGUATE);
    let all_keys = flags.contains(Keyboard::ALL_KEYS);
    let legacy_mode = !flags.contains(Keyboard::EVENT_TYPES) && !disambiguate && !all_keys;
    let mods = press.mods;
    let release = press.kind == Kind::Release;
    let named = match key {
        Functional::Named(named) => Some(named),
        Functional::Pad(_) => None,
    };

    if cursor_key_mode && legacy_mode && mods.0 == 0 {
        let final_byte = match key {
            Functional::Named(Named::Up) => Some(b'A'),
            Functional::Named(Named::Down) => Some(b'B'),
            Functional::Named(Named::Right) => Some(b'C'),
            Functional::Named(Named::Left) => Some(b'D'),
            Functional::Pad(Pad::Begin) | Functional::Named(Named::Begin) => Some(b'E'),
            Functional::Named(Named::End) => Some(b'F'),
            Functional::Named(Named::Home) => Some(b'H'),
            _ => None,
        };
        if let Some(final_byte) = final_byte {
            return vec![ESC, b'O', final_byte];
        }
    }
    // Enter, Tab and Backspace send what they always have, so `reset` can still be typed
    // after a program that set the flags ended without resetting them.
    let plain = |named: Option<Named>| match named {
        Some(Named::Enter | Named::Backspace | Named::Tab) if release => Some(Vec::new()),
        Some(Named::Enter) => Some(b"\r".to_vec()),
        Some(Named::Backspace) => Some(b"\x7f".to_vec()),
        Some(Named::Tab) => Some(b"\t".to_vec()),
        _ => None,
    };
    if mods.0 == 0 {
        if !disambiguate && !all_keys && named == Some(Named::Escape) {
            return vec![ESC];
        }
        if legacy_mode {
            if let Some(Named::F(n @ 1..=4)) = named {
                return vec![ESC, b'O', b"PQRS"[usize::from(n - 1)]];
            }
        }
        if !all_keys {
            if let Some(bytes) = plain(named) {
                return bytes;
            }
        }
    } else if legacy_mode {
        if let Some(bytes) = kitty_legacy_functional(named, mods) {
            return bytes;
        }
    }
    if mods.held() == 0 && !all_keys {
        if let Some(bytes) = plain(named) {
            return bytes;
        }
    }

    let (number, final_byte) = match key {
        Functional::Named(named) => match named {
            Named::Escape => (27, 'u'),
            Named::Enter => (13, 'u'),
            Named::Tab => (9, 'u'),
            Named::Backspace => (127, 'u'),
            Named::Insert => (2, '~'),
            Named::Delete => (3, '~'),
            Named::Left => (1, 'D'),
            Named::Right => (1, 'C'),
            Named::Up => (1, 'A'),
            Named::Down => (1, 'B'),
            Named::PageUp => (5, '~'),
            Named::PageDown => (6, '~'),
            Named::Home => (1, 'H'),
            Named::End => (1, 'F'),
            Named::F(1) => (1, 'P'),
            Named::F(2) => (1, 'Q'),
            Named::F(3) => (13, '~'),
            Named::F(4) => (1, 'S'),
            Named::F(n @ 5..=12) => (TILDES[usize::from(n - 5)], '~'),
            Named::F(n) => (57376 + u32::from(n - 13), 'u'),
            // kitty's KP_BEGIN, which is also the key for Begin.
            Named::Begin => (1, 'E'),
            // xterm's F16, in legacy mode.
            Named::Menu if legacy_mode => (29, '~'),
            Named::Menu => (57363, 'u'),
            Named::CapsLock => (57358, 'u'),
            Named::ScrollLock => (57359, 'u'),
            Named::NumLock => (57360, 'u'),
            Named::PrintScreen => (57361, 'u'),
            Named::Pause => (57362, 'u'),
            Named::Media(number) | Named::Modifier(number) => (number, 'u'),
        },
        Functional::Pad(pad) => match pad {
            Pad::Digit(digit) => (57399 + u32::from(digit), 'u'),
            Pad::Decimal => (57409, 'u'),
            Pad::Divide => (57410, 'u'),
            Pad::Multiply => (57411, 'u'),
            Pad::Subtract => (57412, 'u'),
            Pad::Add => (57413, 'u'),
            Pad::Enter => (57414, 'u'),
            Pad::Equal => (57415, 'u'),
            Pad::Separator => (57416, 'u'),
            Pad::Left => (57417, 'u'),
            Pad::Right => (57418, 'u'),
            Pad::Up => (57419, 'u'),
            Pad::Down => (57420, 'u'),
            Pad::PageUp => (57421, 'u'),
            Pad::PageDown => (57422, 'u'),
            Pad::Home => (57423, 'u'),
            Pad::End => (57424, 'u'),
            Pad::Insert => (57425, 'u'),
            Pad::Delete => (57426, 'u'),
            Pad::Begin => (1, 'E'),
        },
    };
    let mut fields = Fields::new(number, press, flags);
    // Functional keys have no alternates.
    fields.alternates = false;
    fields.shifted = None;
    fields.base = None;
    fields.final_byte = final_byte;
    fields.serialize()
}

/// kitty's `legacy_functional_key_encoding_with_modifiers`.
fn kitty_legacy_functional(named: Option<Named>, mods: Mods) -> Option<Vec<u8>> {
    let alt = mods.has(Mods::ALT);
    let mut prefix = if alt { vec![ESC] } else { Vec::new() };
    let main: &[u8] = match named? {
        Named::Enter => b"\r",
        Named::Escape => b"\x1b",
        Named::Backspace if mods.has(Mods::CTRL) => b"\x08",
        Named::Backspace => b"\x7f",
        Named::Tab if mods.has(Mods::SHIFT) => {
            prefix.push(ESC);
            b"[Z"
        }
        Named::Tab => b"\t",
        _ => return None,
    };
    prefix.extend_from_slice(main);
    Some(prefix)
}

/// kitty's `encode_key`, for a key that types a character.
fn kitty_char(char: char, press: &Press, flags: Keyboard) -> Vec<u8> {
    let fields = Fields::new(u32::from(char), press, flags);
    let simple = !fields.actions && !fields.alternates && fields.text.is_empty();
    let mods = press.mods.0;
    if simple {
        if mods == 0 {
            if flags.contains(Keyboard::ALL_KEYS) {
                return fields.serialize();
            }
            let mut bytes = Vec::new();
            push_char(&mut bytes, char);
            return bytes;
        }
        if !flags.contains(Keyboard::DISAMBIGUATE) && !flags.contains(Keyboard::ALL_KEYS) {
            let shifted = press.shifted();
            if is_legacy_ascii(char) || shifted.is_some_and(is_legacy_ascii) {
                if let Some(bytes) = kitty_legacy_char(char, shifted, mods) {
                    return bytes;
                }
            }
            let base = press.base();
            let ctrl_or_alt = [Mods::CTRL, Mods::ALT, Mods::CTRL | Mods::ALT].contains(&mods);
            if let Some(base) =
                base.filter(|&base| ctrl_or_alt && !is_legacy_ascii(char) && is_legacy_ascii(base))
            {
                if let Some(bytes) = kitty_legacy_char(base, None, mods) {
                    return bytes;
                }
            }
        }
    }
    fields.serialize()
}

/// kitty's `encode_printable_ascii_key_legacy`.
fn kitty_legacy_char(char: char, shifted: Option<char>, mods: u8) -> Option<Vec<u8>> {
    let mut key = char;
    let mut left = mods;
    if mods & Mods::SHIFT != 0 {
        if let Some(shifted) = shifted.filter(|&shifted| {
            shifted != char && (mods & Mods::CTRL == 0 || !char.is_ascii_lowercase())
        }) {
            key = shifted;
            left &= !Mods::SHIFT;
        }
    }
    let ctrled = || {
        let key = key as u8;
        match key {
            b' ' | b'@' | b'2' => 0,
            b'/' | b'7' | b'_' => 31,
            b'3' | b'[' => 27,
            b'4' | b'\\' => 28,
            b'5' | b']' => 29,
            b'6' | b'^' | b'~' => 30,
            b'8' | b'?' => 127,
            b'a'..=b'z' => key - b'a' + 1,
            _ => key,
        }
    };
    let ascii = key.is_ascii();
    Some(if mods == Mods::SHIFT {
        let mut bytes = Vec::new();
        push_char(&mut bytes, key);
        bytes
    } else if left == Mods::ALT {
        let mut bytes = vec![ESC];
        push_char(&mut bytes, key);
        bytes
    } else if left == Mods::CTRL && ascii {
        vec![ctrled()]
    } else if left == Mods::CTRL | Mods::ALT && ascii {
        vec![ESC, ctrled()]
    } else if key == ' ' && left == Mods::CTRL | Mods::SHIFT {
        vec![ctrled()]
    } else if key == ' ' && left == Mods::ALT | Mods::SHIFT {
        vec![ESC, b' ']
    } else {
        return None;
    })
}

/// kitty's `is_legacy_ascii_key`: the keys of a US keyboard, unshifted and shifted.
fn is_legacy_ascii(char: char) -> bool {
    char.is_ascii_lowercase()
        || char.is_ascii_digit()
        || "!@#$%^&*()`~-_=+[{]}\\|;:'\",<.>/? ".contains(char)
}

fn push_char(bytes: &mut Vec<u8>, char: char) {
    bytes.extend_from_slice(char.encode_utf8(&mut [0; 4]).as_bytes());
}
