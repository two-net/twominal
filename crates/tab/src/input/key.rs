//! What a key event from the webview means: which key it was, what it typed and which
//! modifiers were held for it (TERM-08, HW-01).
//!
//! The webview gives the DOM's `key` and `code` as they are, so any string can arrive. One
//! that names no key means nothing, and nothing is sent.

use super::{KeyEvent, Kind};

/// A key event, read.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Press {
    pub(crate) key: Key,
    /// What the key typed, Shift and the layout applied, as the DOM's `key` gave it. Empty for
    /// a key that types nothing.
    pub(crate) typed: String,
    pub(crate) mods: Mods,
    pub(crate) kind: Kind,
    /// The characters the key types on a US keyboard, without Shift and with it: the kitty
    /// keyboard protocol's base layout key, and the key Ctrl and Alt fall back on when the
    /// layout's is not ASCII.
    pub(crate) us: Option<(char, char)>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Key {
    /// A key that types a character. This is the one it types without Shift, lowercase: the
    /// kitty keyboard protocol's key code.
    Char(char),
    Named(Named),
    /// A key of the keypad, which keeps its identity in the kitty keyboard protocol.
    Pad(Pad),
}

/// A key that types no text: the kitty keyboard protocol's functional keys.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Named {
    Escape,
    Enter,
    Tab,
    Backspace,
    Insert,
    Delete,
    Left,
    Right,
    Up,
    Down,
    PageUp,
    PageDown,
    Home,
    End,
    /// The keypad's 5 with NumLock off.
    Begin,
    CapsLock,
    ScrollLock,
    NumLock,
    PrintScreen,
    Pause,
    Menu,
    /// F1 to F35.
    F(u8),
    /// A media key, by its number in the kitty keyboard protocol.
    Media(u32),
    /// A modifier key on its own, by its number in the kitty keyboard protocol.
    Modifier(u32),
}

/// A key of the keypad.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Pad {
    Digit(u8),
    Decimal,
    Divide,
    Multiply,
    Subtract,
    Add,
    Enter,
    Equal,
    Separator,
    // With NumLock off, the keys that move.
    Left,
    Right,
    Up,
    Down,
    PageUp,
    PageDown,
    Home,
    End,
    Insert,
    Delete,
    Begin,
}

/// Which modifiers count for a key, as the kitty keyboard protocol numbers them.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct Mods(pub(crate) u8);

impl Mods {
    pub(crate) const SHIFT: u8 = 1;
    pub(crate) const ALT: u8 = 2;
    pub(crate) const CTRL: u8 = 4;
    /// Cmd on macOS, the Windows key elsewhere. xterm calls it Meta and gives it the same 8.
    pub(crate) const SUPER: u8 = 8;
    pub(crate) const CAPS_LOCK: u8 = 64;
    pub(crate) const NUM_LOCK: u8 = 128;
    pub(crate) const LOCKS: u8 = Self::CAPS_LOCK | Self::NUM_LOCK;

    pub(crate) fn has(self, mods: u8) -> bool {
        self.0 & mods != 0
    }

    /// Without CapsLock and NumLock, which xterm never reports.
    pub(crate) fn held(self) -> u8 {
        self.0 & !Self::LOCKS
    }
}

/// What `event` means. `macos` says whether Option types characters: on macOS it types what
/// the layout gives, and is Alt only for a key that types nothing (DESIGN.md §3).
pub(crate) fn read(event: &KeyEvent, macos: bool) -> Option<Press> {
    let us = us_key(&event.code);
    let held = event.modifiers;
    let (key, typed) = if let Some(pad) = pad(&event.code, &event.key) {
        let typed = match pad.char() {
            Some(_) if is_text(&event.key) => event.key.clone(),
            Some(char) => char.to_string(),
            None => String::new(),
        };
        (Key::Pad(pad), typed)
    } else if let Some(named) = named(&event.key, &event.code) {
        (Key::Named(named), String::new())
    } else if is_text(&event.key) {
        let code = key_code(&event.key, held.shift, us);
        (Key::Char(code), event.key.clone())
    } else {
        return None;
    };

    let mut alt = held.alt;
    let mut ctrl = held.ctrl;
    if !typed.is_empty() {
        if held.alt_graph {
            // AltGr typed it. Windows reports AltGr as Ctrl and Alt too.
            alt = false;
            ctrl = false;
        } else if macos && held.alt && !held.ctrl && !held.meta {
            // Option typed it.
            alt = false;
        }
    }
    let mut mods = 0;
    for (on, bit) in [
        (held.shift, Mods::SHIFT),
        (alt, Mods::ALT),
        (ctrl, Mods::CTRL),
        (held.meta, Mods::SUPER),
        (held.caps_lock, Mods::CAPS_LOCK),
        (held.num_lock, Mods::NUM_LOCK),
    ] {
        if on {
            mods |= bit;
        }
    }
    Some(Press {
        key,
        typed,
        mods: Mods(mods),
        kind: event.kind,
        us,
    })
}

impl Press {
    /// What the key types for the kitty keyboard protocol: nothing while Ctrl, Alt or Super is
    /// held, nothing with a control character in it, and nothing when the key comes up, as
    /// kitty has it.
    pub(crate) fn text(&self) -> &str {
        let control = self.typed.chars().any(char::is_control);
        if control
            || self.kind == Kind::Release
            || self.mods.has(Mods::CTRL | Mods::ALT | Mods::SUPER)
        {
            ""
        } else {
            &self.typed
        }
    }

    /// Whether the key is a modifier or a lock, which kitty reports only with every key.
    pub(crate) fn is_modifier(&self) -> bool {
        matches!(
            self.key,
            Key::Named(Named::Modifier(_) | Named::CapsLock | Named::ScrollLock | Named::NumLock)
        )
    }

    /// What the key types with Shift, if Shift is held and changes it: the kitty keyboard
    /// protocol's shifted key.
    pub(crate) fn shifted(&self) -> Option<char> {
        let Key::Char(code) = self.key else {
            return None;
        };
        let typed = self.typed.chars().next()?;
        (self.mods.has(Mods::SHIFT) && typed != code).then_some(typed)
    }

    /// The key at this place on a US keyboard, if it is another: the kitty keyboard protocol's
    /// base layout key.
    pub(crate) fn base(&self) -> Option<char> {
        let Key::Char(code) = self.key else {
            return None;
        };
        self.us.map(|(base, _)| base).filter(|&base| base != code)
    }
}

impl Pad {
    /// The character the key types, if it types one: it does with NumLock on, and Enter never
    /// does. The layout may type another, such as a comma for the decimal point.
    pub(crate) fn char(self) -> Option<char> {
        Some(match self {
            Self::Digit(digit) => char::from(b'0' + digit),
            Self::Decimal => '.',
            Self::Divide => '/',
            Self::Multiply => '*',
            Self::Subtract => '-',
            Self::Add => '+',
            Self::Equal => '=',
            Self::Separator => ',',
            _ => return None,
        })
    }
}

/// Whether the DOM's `key` is text rather than the name of a key: one character, or several
/// that are not an ASCII name such as `Enter`.
fn is_text(key: &str) -> bool {
    let mut chars = key.chars();
    match (chars.next(), chars.next()) {
        (None, _) => false,
        (Some(_), None) => true,
        _ => !key.chars().all(|char| char.is_ascii_alphanumeric()),
    }
}

/// The kitty keyboard protocol's code for a key that typed `typed`: what it types without
/// Shift. A cased character's lowercase, so CapsLock changes nothing. Otherwise, with Shift,
/// the US keyboard's character when the layout's matches it there, as on `1` for `!`. A
/// webview cannot read the layout, so on another the shifted character stands (DESIGN.md §9).
fn key_code(typed: &str, shift: bool, us: Option<(char, char)>) -> char {
    let first = typed.chars().next().unwrap_or(' ');
    let mut lower = first.to_lowercase();
    if let (Some(lower), None) = (lower.next(), lower.next()) {
        if lower != first {
            return lower;
        }
    }
    match us {
        Some((base, shifted)) if shift && shifted == first => base,
        _ => first,
    }
}

/// The key the DOM's `key` names, if it is one that types no text.
fn named(key: &str, code: &str) -> Option<Named> {
    // The Mac's keypad Clear is where NumLock is elsewhere.
    if code == "NumLock" {
        return Some(Named::NumLock);
    }
    let right = code.ends_with("Right");
    let side = |left: u32| Named::Modifier(if right { left + 6 } else { left });
    Some(match key {
        "Escape" | "Esc" => Named::Escape,
        "Enter" => Named::Enter,
        "Tab" => Named::Tab,
        "Backspace" => Named::Backspace,
        "Insert" => Named::Insert,
        "Delete" | "Del" => Named::Delete,
        "ArrowLeft" | "Left" => Named::Left,
        "ArrowRight" | "Right" => Named::Right,
        "ArrowUp" | "Up" => Named::Up,
        "ArrowDown" | "Down" => Named::Down,
        "PageUp" => Named::PageUp,
        "PageDown" => Named::PageDown,
        "Home" => Named::Home,
        "End" => Named::End,
        "Clear" => Named::Begin,
        "CapsLock" => Named::CapsLock,
        "ScrollLock" => Named::ScrollLock,
        "NumLock" => Named::NumLock,
        "PrintScreen" => Named::PrintScreen,
        "Pause" => Named::Pause,
        "ContextMenu" | "Apps" => Named::Menu,
        "MediaPlay" => Named::Media(57428),
        "MediaPause" => Named::Media(57429),
        "MediaPlayPause" => Named::Media(57430),
        "MediaStop" => Named::Media(57432),
        "MediaFastForward" => Named::Media(57433),
        "MediaRewind" => Named::Media(57434),
        "MediaTrackNext" => Named::Media(57435),
        "MediaTrackPrevious" => Named::Media(57436),
        "MediaRecord" => Named::Media(57437),
        "AudioVolumeDown" => Named::Media(57438),
        "AudioVolumeUp" => Named::Media(57439),
        "AudioVolumeMute" => Named::Media(57440),
        // Each on the left, and 6 more on the right.
        "Shift" => side(57441),
        "Control" => side(57442),
        "Alt" => side(57443),
        // Cmd and the Windows key, which the kitty keyboard protocol calls Super.
        "Meta" | "OS" | "Super" => side(57444),
        "Hyper" => side(57445),
        // ISO_Level3_Shift.
        "AltGraph" => Named::Modifier(57453),
        _ => {
            let number = key.strip_prefix('F')?;
            if number.starts_with('0') {
                return None;
            }
            match number.parse() {
                Ok(n @ 1..=35) => Named::F(n),
                _ => return None,
            }
        }
    })
}

/// The keypad's key, if `code` is on the keypad. With NumLock off, `key` names the key it
/// moves by instead of a character.
fn pad(code: &str, key: &str) -> Option<Pad> {
    let name = code.strip_prefix("Numpad")?;
    let moves = match key {
        "ArrowLeft" | "Left" => Some(Pad::Left),
        "ArrowRight" | "Right" => Some(Pad::Right),
        "ArrowUp" | "Up" => Some(Pad::Up),
        "ArrowDown" | "Down" => Some(Pad::Down),
        "PageUp" => Some(Pad::PageUp),
        "PageDown" => Some(Pad::PageDown),
        "Home" => Some(Pad::Home),
        "End" => Some(Pad::End),
        "Insert" => Some(Pad::Insert),
        "Delete" | "Del" => Some(Pad::Delete),
        "Clear" => Some(Pad::Begin),
        _ => None,
    };
    if moves.is_some() {
        return moves;
    }
    Some(match name {
        "Decimal" => Pad::Decimal,
        "Divide" => Pad::Divide,
        "Multiply" => Pad::Multiply,
        "Subtract" => Pad::Subtract,
        "Add" => Pad::Add,
        "Enter" => Pad::Enter,
        "Equal" => Pad::Equal,
        "Comma" => Pad::Separator,
        _ => match name.as_bytes() {
            [digit @ b'0'..=b'9'] => Pad::Digit(digit - b'0'),
            _ => return None,
        },
    })
}

/// What the key at `code` types on a US keyboard, without Shift and with it.
fn us_key(code: &str) -> Option<(char, char)> {
    if let Some(letter) = code.strip_prefix("Key") {
        if let [letter @ b'A'..=b'Z'] = letter.as_bytes() {
            let letter = char::from(*letter);
            return Some((letter.to_ascii_lowercase(), letter));
        }
        return None;
    }
    if let Some(digit) = code.strip_prefix("Digit") {
        if let [digit @ b'0'..=b'9'] = digit.as_bytes() {
            let shifted = b")!@#$%^&*("[usize::from(digit - b'0')];
            return Some((char::from(*digit), char::from(shifted)));
        }
        return None;
    }
    Some(match code {
        "Minus" => ('-', '_'),
        "Equal" => ('=', '+'),
        "BracketLeft" => ('[', '{'),
        "BracketRight" => (']', '}'),
        "Backslash" => ('\\', '|'),
        "Semicolon" => (';', ':'),
        "Quote" => ('\'', '"'),
        "Backquote" => ('`', '~'),
        "Comma" => (',', '<'),
        "Period" => ('.', '>'),
        "Slash" => ('/', '?'),
        "Space" => (' ', ' '),
        _ => return None,
    })
}
