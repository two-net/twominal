//! Tests of the Input Handler (TERM-08, TERM-09, TERM-11, HW-01): DOM key events, pastes,
//! focus changes and input-method text in, the bytes a program reads out.
//!
//! The legacy cases are what xterm sends for `TERM=xterm-256color`, with the deviations
//! DESIGN.md §3 lists. The kitty cases are kitty's own, from `kitty_tests/keys.py`, given as
//! the DOM would give the key.

use terminal_core::{Keyboard, Modes};

use super::*;

/// A key event on a US keyboard, Linux's unless the test says otherwise.
#[derive(Clone)]
struct Event(KeyEvent);

/// The key whose DOM `key` is `key`, at the place a US keyboard has it.
fn key(key: &str) -> Event {
    Event(KeyEvent {
        key: key.to_string(),
        code: us_code(key),
        ..KeyEvent::default()
    })
}

impl Event {
    fn code(mut self, code: &str) -> Self {
        self.0.code = code.to_string();
        self
    }
    fn shift(mut self) -> Self {
        self.0.modifiers.shift = true;
        self
    }
    fn alt(mut self) -> Self {
        self.0.modifiers.alt = true;
        self
    }
    fn ctrl(mut self) -> Self {
        self.0.modifiers.ctrl = true;
        self
    }
    fn meta(mut self) -> Self {
        self.0.modifiers.meta = true;
        self
    }
    fn alt_graph(mut self) -> Self {
        self.0.modifiers.alt_graph = true;
        self
    }
    fn caps_lock(mut self) -> Self {
        self.0.modifiers.caps_lock = true;
        self
    }
    fn num_lock(mut self) -> Self {
        self.0.modifiers.num_lock = true;
        self
    }
    fn repeat(mut self) -> Self {
        self.0.kind = Kind::Repeat;
        self
    }
    fn release(mut self) -> Self {
        self.0.kind = Kind::Release;
        self
    }
}

/// Where a US keyboard has the key that types `key`, or the DOM's code for a named key.
fn us_code(key: &str) -> String {
    const ROW: &str = "`1234567890-=[]\\;',./";
    const SHIFTED: &str = "~!@#$%^&*()_+{}|:\"<>?";
    const NAMES: [&str; 21] = [
        "Backquote",
        "Digit1",
        "Digit2",
        "Digit3",
        "Digit4",
        "Digit5",
        "Digit6",
        "Digit7",
        "Digit8",
        "Digit9",
        "Digit0",
        "Minus",
        "Equal",
        "BracketLeft",
        "BracketRight",
        "Backslash",
        "Semicolon",
        "Quote",
        "Comma",
        "Period",
        "Slash",
    ];
    let mut chars = key.chars();
    match (chars.next(), chars.next()) {
        (Some(' '), None) => "Space".to_string(),
        (Some(letter), None) if letter.is_ascii_alphabetic() => {
            format!("Key{}", letter.to_ascii_uppercase())
        }
        (Some(char), None) => ROW
            .find(char)
            .or_else(|| SHIFTED.find(char))
            .map_or_else(String::new, |at| NAMES[at].to_string()),
        _ => key.to_string(),
    }
}

/// The modes with the kitty keyboard flags `flags`.
fn flags(flags: u8) -> Modes {
    Modes {
        keyboard: Keyboard::new(flags),
        ..Modes::default()
    }
}

/// What `event` sends with `modes`, on Linux.
fn sent_with(modes: &Modes, event: &Event) -> String {
    sent_on(false, modes, event)
}

fn sent_on(macos: bool, modes: &Modes, event: &Event) -> String {
    match InputHandler::on(macos).handle(InputEvent::Key(event.0.clone()), modes) {
        Some(Action::Send(bytes)) => String::from_utf8(bytes).unwrap(),
        Some(Action::Composing) => panic!("a key composed"),
        None => String::new(),
    }
}

/// What `event` sends with no mode set, on Linux.
fn sent(event: &Event) -> String {
    sent_with(&Modes::default(), event)
}

/// Checks each of `cases`: an event and what it sends.
#[track_caller]
fn check(modes: &Modes, cases: &[(Event, &str)]) {
    for (event, expected) in cases {
        assert_eq!(
            sent_with(modes, event),
            *expected,
            "{:?} with {modes:?}",
            event.0
        );
    }
}

// ---------------------------------------------------------------------------------------
// Keys that type
// ---------------------------------------------------------------------------------------

#[test]
fn a_key_sends_what_it_typed() {
    check(
        &Modes::default(),
        &[
            (key("a"), "a"),
            (key("A").shift(), "A"),
            (key("A").caps_lock(), "A"),
            (key("!").shift(), "!"),
            (key(" "), " "),
            (key(" ").shift(), " "),
            // AZERTY's 2 types é, and a layout may type more than one character at once.
            (key("é").code("Digit2"), "é"),
            (key("ﻻ").code("KeyB"), "ﻻ"),
            (key("e\u{301}").code("KeyE"), "e\u{301}"),
        ],
    );
}

#[test]
fn ctrl_makes_what_xlib_makes() {
    // Xlib's XkbToControl, which xterm uses: @ to ~ and space keep their low five bits, and
    // 2 to 8 and / are the keys of the controls a US keyboard cannot reach. The rest stay.
    let controls = [
        ("@", 0),
        ("`", 0),
        (" ", 0),
        ("2", 0),
        ("a", 1),
        ("z", 26),
        ("[", 27),
        ("{", 27),
        ("3", 27),
        ("\\", 28),
        ("|", 28),
        ("4", 28),
        ("]", 29),
        ("}", 29),
        ("5", 29),
        ("^", 30),
        ("~", 30),
        ("6", 30),
        ("_", 31),
        ("/", 31),
        ("7", 31),
        ("8", 127),
    ];
    for (typed, control) in controls {
        let event = key(typed).ctrl();
        assert_eq!(sent(&event).as_bytes(), [control], "Ctrl+{typed}");
    }
    for typed in ["?", "-", "=", "1", "9", "0", ";", "'", ",", ".", "!"] {
        assert_eq!(sent(&key(typed).ctrl()), typed, "Ctrl+{typed}");
    }
}

#[test]
fn ctrl_and_shift_with_a_letter_is_ctrl_and_the_letter() {
    // As in xterm. kitty sends CSI 97;6u even with no flag set.
    check(
        &Modes::default(),
        &[
            (key("A").ctrl().shift(), "\x01"),
            (key("@").ctrl().shift(), "\0"),
            (key("_").ctrl().shift(), "\x1f"),
        ],
    );
}

#[test]
fn alt_puts_esc_before_the_key() {
    check(
        &Modes::default(),
        &[
            (key("a").alt(), "\x1ba"),
            (key("A").alt().shift(), "\x1bA"),
            (key("a").alt().ctrl(), "\x1b\x01"),
            (key(" ").alt(), "\x1b "),
            (key(".").alt(), "\x1b."),
            (key("é").code("Digit2").alt().ctrl(), "\x1b\0"),
        ],
    );
}

#[test]
fn ctrl_and_alt_take_the_us_key_on_a_layout_without_ascii() {
    // Cyrillic and Greek, so Ctrl+C and the readline keys work, as in kitty.
    check(
        &Modes::default(),
        &[
            (key("с").code("KeyC").ctrl(), "\x03"),
            (key("ψ").code("KeyC").ctrl(), "\x03"),
            (key("и").code("KeyB").alt(), "\x1bb"),
            (key("И").code("KeyB").alt().shift(), "\x1bB"),
            (key("с").code("KeyC"), "с"),
            // With no US key to fall back on, the layout's.
            (key("ü").code("IntlBackslash").alt(), "\x1bü"),
        ],
    );
}

#[test]
fn option_types_characters_on_macos() {
    let mac = |event: Event| sent_on(true, &Modes::default(), &event);
    assert_eq!(mac(key("å").code("KeyA").alt()), "å");
    assert_eq!(mac(key("Å").code("KeyA").alt().shift()), "Å");
    // German: Option+L is @ and Option+5 is [.
    assert_eq!(mac(key("@").code("KeyL").alt()), "@");
    assert_eq!(mac(key("[").code("Digit5").alt()), "[");
    // A key that types nothing keeps Option as Alt, as the command line's keys want.
    assert_eq!(mac(key("ArrowLeft").alt()), "\x1b[1;3D");
    assert_eq!(mac(key("Enter").alt()), "\x1b\r");
    assert_eq!(mac(key("Backspace").alt()), "\x1b\x7f");
    // With Ctrl, Option is Alt, and the key at its place a US key.
    assert_eq!(mac(key("å").code("KeyA").alt().ctrl()), "\x1b\x01");
    // Elsewhere Alt is Alt.
    assert_eq!(sent(&key("a").alt()), "\x1ba");
}

#[test]
fn altgr_types_characters() {
    check(
        &Modes::default(),
        &[
            // Linux reports AltGr alone, Windows as Ctrl and Alt too.
            (key("@").code("KeyQ").alt_graph(), "@"),
            (key("@").code("KeyQ").alt_graph().ctrl().alt(), "@"),
            (key("€").code("KeyE").alt_graph(), "€"),
        ],
    );
}

#[test]
fn cmd_with_a_character_sends_nothing() {
    // xterm's Meta has no encoding for these, and Terminal.app and iTerm2 send nothing.
    for event in [
        key("a").meta(),
        key("A").meta().shift(),
        key("Enter").meta(),
        key("Tab").meta(),
        key("Backspace").meta(),
        key("Escape").meta(),
    ] {
        assert_eq!(sent(&event), "", "{:?}", event.0);
    }
    // Keys with modifiers in their sequence take it as xterm's Meta, 8.
    assert_eq!(sent(&key("ArrowLeft").meta()), "\x1b[1;9D");
    assert_eq!(sent(&key("F5").meta().shift()), "\x1b[15;10~");
}

#[test]
fn what_is_not_a_key_sends_nothing() {
    for name in [
        "Dead",
        "Unidentified",
        "Process",
        "",
        "Help",
        "Fn",
        "F0",
        "F01",
        "F36",
        "BrowserBack",
    ] {
        assert_eq!(sent(&key(name)), "", "{name}");
    }
}

// ---------------------------------------------------------------------------------------
// Keys that type nothing
// ---------------------------------------------------------------------------------------

#[test]
fn enter_tab_backspace_and_esc_are_control_characters() {
    // As kitty's legacy cases have them, which are xterm's.
    let lnm = Modes {
        newline: true,
        ..Modes::default()
    };
    check(
        &Modes::default(),
        &[
            (key("Enter"), "\r"),
            (key("Enter").alt(), "\x1b\r"),
            (key("Enter").shift(), "\r"),
            (key("Enter").ctrl(), "\r"),
            (key("Enter").ctrl().alt(), "\x1b\r"),
            (key("Tab"), "\t"),
            (key("Tab").shift(), "\x1b[Z"),
            (key("Tab").alt(), "\x1b\t"),
            (key("Tab").ctrl(), "\t"),
            (key("Tab").ctrl().shift(), "\x1b[Z"),
            (key("Tab").alt().shift(), "\x1b\x1b[Z"),
            (key("Backspace"), "\x7f"),
            (key("Backspace").shift(), "\x7f"),
            (key("Backspace").ctrl(), "\x08"),
            (key("Backspace").alt(), "\x1b\x7f"),
            (key("Backspace").ctrl().alt(), "\x1b\x08"),
            (key("Escape"), "\x1b"),
            (key("Escape").alt(), "\x1b\x1b"),
            (key("Escape").ctrl(), "\x1b"),
            (key("Escape").shift(), "\x1b"),
        ],
    );
    check(
        &lnm,
        &[(key("Enter"), "\r\n"), (key("Enter").alt(), "\x1b\r\n")],
    );
}

#[test]
fn esc_is_sent_alone_and_at_once() {
    // TERM-08 and VIM-05: one event, one byte, nothing held back for what may follow.
    let mut input = InputHandler::on(false);
    assert_eq!(
        input.handle(InputEvent::Key(key("Escape").0), &Modes::default()),
        Some(Action::Send(vec![0x1b]))
    );
}

#[test]
fn cursor_keys_follow_decckm() {
    let decckm = Modes {
        application_cursor: true,
        ..Modes::default()
    };
    let plain = [
        ("ArrowUp", 'A'),
        ("ArrowDown", 'B'),
        ("ArrowRight", 'C'),
        ("ArrowLeft", 'D'),
        ("Clear", 'E'),
        ("End", 'F'),
        ("Home", 'H'),
    ];
    for (name, final_byte) in plain {
        assert_eq!(sent(&key(name)), format!("\x1b[{final_byte}"), "{name}");
        assert_eq!(
            sent_with(&decckm, &key(name)),
            format!("\x1bO{final_byte}"),
            "{name}"
        );
        // With modifiers, the same in both modes.
        for (event, m) in [
            (key(name).shift(), 2),
            (key(name).alt(), 3),
            (key(name).alt().shift(), 4),
            (key(name).ctrl(), 5),
            (key(name).ctrl().shift(), 6),
            (key(name).ctrl().alt(), 7),
            (key(name).ctrl().alt().shift(), 8),
            (key(name).meta(), 9),
        ] {
            let expected = format!("\x1b[1;{m}{final_byte}");
            assert_eq!(sent(&event), expected, "{:?}", event.0);
            assert_eq!(sent_with(&decckm, &event), expected, "{:?}", event.0);
        }
        // The locks are not modifiers here.
        assert_eq!(
            sent(&key(name).caps_lock().num_lock()),
            format!("\x1b[{final_byte}")
        );
    }
}

#[test]
fn editing_and_function_keys_are_xterms() {
    check(
        &Modes::default(),
        &[
            (key("Insert"), "\x1b[2~"),
            (key("Delete"), "\x1b[3~"),
            (key("PageUp"), "\x1b[5~"),
            (key("PageDown"), "\x1b[6~"),
            (key("PageUp").shift(), "\x1b[5;2~"),
            (key("Delete").ctrl(), "\x1b[3;5~"),
            (key("F1"), "\x1bOP"),
            (key("F2"), "\x1bOQ"),
            (key("F3"), "\x1bOR"),
            (key("F4"), "\x1bOS"),
            (key("F1").shift(), "\x1b[1;2P"),
            (key("F4").ctrl(), "\x1b[1;5S"),
            (key("F5"), "\x1b[15~"),
            (key("F6"), "\x1b[17~"),
            (key("F7"), "\x1b[18~"),
            (key("F8"), "\x1b[19~"),
            (key("F9"), "\x1b[20~"),
            (key("F10"), "\x1b[21~"),
            (key("F11"), "\x1b[23~"),
            (key("F12"), "\x1b[24~"),
            (key("F5").ctrl(), "\x1b[15;5~"),
            // terminfo's kf13 to kf24: Shift and F1 to F12. kf25 to kf35: Ctrl and F1 to F11.
            (key("F13"), "\x1b[1;2P"),
            (key("F16"), "\x1b[1;2S"),
            (key("F17"), "\x1b[15;2~"),
            (key("F24"), "\x1b[24;2~"),
            (key("F20").ctrl(), "\x1b[19;6~"),
            (key("F25"), "\x1b[1;5P"),
            (key("F35"), "\x1b[23;5~"),
            // xterm's F16.
            (key("ContextMenu"), "\x1b[29~"),
            (key("ContextMenu").shift(), "\x1b[29;2~"),
        ],
    );
}

#[test]
fn keys_xterm_has_no_sequence_for_send_nothing() {
    for event in [
        key("Shift").code("ShiftLeft").shift(),
        key("Control").code("ControlRight").ctrl(),
        key("Alt").code("AltLeft").alt(),
        key("Meta").code("MetaLeft").meta(),
        key("AltGraph").code("AltRight"),
        key("CapsLock"),
        key("NumLock"),
        key("ScrollLock"),
        key("PrintScreen"),
        key("Pause"),
        key("MediaPlayPause"),
        key("AudioVolumeUp"),
    ] {
        assert_eq!(sent(&event), "", "{:?}", event.0);
    }
}

#[test]
fn the_keypad_follows_deckpam() {
    let pad = |key_name: &str, code: &str| key(key_name).code(code);
    let deckpam = Modes {
        application_keypad: true,
        ..Modes::default()
    };
    let both = Modes {
        application_keypad: true,
        application_cursor: true,
        ..Modes::default()
    };
    // Without DECKPAM, what each key types or stands for.
    check(
        &Modes::default(),
        &[
            (pad("5", "Numpad5").num_lock(), "5"),
            (pad("+", "NumpadAdd"), "+"),
            (pad("Enter", "NumpadEnter"), "\r"),
            (pad("ArrowUp", "Numpad8"), "\x1b[A"),
            (pad("Clear", "Numpad5"), "\x1b[E"),
            (pad("PageUp", "Numpad9").shift(), "\x1b[5;2~"),
            // A German keypad's decimal point.
            (pad(",", "NumpadDecimal").num_lock(), ","),
            (pad("5", "Numpad5").ctrl(), "\x1d"),
        ],
    );
    // With it, the VT100's sequences, but for a key that types a character while NumLock is
    // on, as in xterm.
    check(
        &deckpam,
        &[
            (pad("5", "Numpad5"), "\x1bOu"),
            (pad("0", "Numpad0"), "\x1bOp"),
            (pad("9", "Numpad9"), "\x1bOy"),
            (pad(".", "NumpadDecimal"), "\x1bOn"),
            (pad("/", "NumpadDivide"), "\x1bOo"),
            (pad("*", "NumpadMultiply"), "\x1bOj"),
            (pad("-", "NumpadSubtract"), "\x1bOm"),
            (pad("+", "NumpadAdd"), "\x1bOk"),
            (pad("=", "NumpadEqual"), "\x1bOX"),
            (pad(",", "NumpadComma"), "\x1bOl"),
            (pad("Enter", "NumpadEnter"), "\x1bOM"),
            (pad("Enter", "NumpadEnter").num_lock(), "\x1bOM"),
            (pad("5", "Numpad5").num_lock(), "5"),
            (pad("5", "Numpad5").ctrl(), "\x1d"),
            (pad("ArrowUp", "Numpad8"), "\x1b[A"),
        ],
    );
    check(&both, &[(pad("ArrowUp", "Numpad8"), "\x1bOA")]);
    // The Mac's keypad Clear is where NumLock is elsewhere.
    assert_eq!(sent(&pad("Clear", "NumLock")), "");
}

#[test]
fn a_held_key_repeats_and_its_release_sends_nothing() {
    check(
        &Modes::default(),
        &[
            (key("a").repeat(), "a"),
            (key("ArrowUp").repeat(), "\x1b[A"),
            (key("a").release(), ""),
            (key("Enter").release(), ""),
            (key("Escape").release(), ""),
        ],
    );
}

// ---------------------------------------------------------------------------------------
// The kitty keyboard protocol
// ---------------------------------------------------------------------------------------

#[test]
fn kitty_keeps_the_legacy_keys_with_no_flag_that_changes_them() {
    // kitty's mods_test, with a flag that leaves the keys legacy: alternate keys alone.
    let modes = flags(0b100);
    check(
        &modes,
        &[
            (key("Enter"), "\r"),
            (key("Enter").alt(), "\x1b\r"),
            (key("Enter").ctrl(), "\r"),
            (key("Escape").alt(), "\x1b\x1b"),
            (key("Backspace").ctrl().alt(), "\x1b\x08"),
            (key("Tab").alt().shift(), "\x1b\x1b[Z"),
            (key("Insert"), "\x1b[2~"),
            (key("Insert").shift(), "\x1b[2;2~"),
            (key("F3"), "\x1bOR"),
            (key("F3").shift(), "\x1b[13;2~"),
            (key("F5").ctrl(), "\x1b[15;5~"),
            (key("ArrowUp"), "\x1b[A"),
            (key("ArrowUp").ctrl(), "\x1b[1;5A"),
            (key("Home"), "\x1b[H"),
            (key("Home").shift(), "\x1b[1;2H"),
        ],
    );
    // Reporting events is not legacy: a key with modifiers is an escape code.
    check(
        &flags(0b10),
        &[
            (key("Enter"), "\r"),
            (key("Enter").alt(), "\x1b[13;3u"),
            (key("F3"), "\x1b[13~"),
        ],
    );
}

#[test]
fn kitty_disambiguates_esc_and_keys_with_modifiers() {
    // kitty_tests/keys.py, "test disambiguate".
    let modes = flags(0b1);
    check(
        &modes,
        &[
            (key("a"), "a"),
            (key("A").shift(), "A"),
            (key("Escape"), "\x1b[27u"),
            (key("Enter"), "\r"),
            (key("Enter").shift(), "\x1b[13;2u"),
            (key("Tab"), "\t"),
            (key("Backspace"), "\x7f"),
            (key("Tab").shift(), "\x1b[9;2u"),
            (key("a").ctrl(), "\x1b[97;5u"),
            (key("a").alt(), "\x1b[97;3u"),
            (key("A").ctrl().shift(), "\x1b[97;6u"),
            (key("A").alt().shift(), "\x1b[97;4u"),
            (key(" ").ctrl(), "\x1b[32;5u"),
            (key("PageUp").code("Numpad9"), "\x1b[57421u"),
            (key("PageUp").code("Numpad9").ctrl(), "\x1b[57421;5u"),
            (key("0").code("Numpad0").ctrl(), "\x1b[57399;5u"),
            (key("ArrowUp"), "\x1b[A"),
            (key("ArrowUp").ctrl(), "\x1b[1;5A"),
            // The keypad's keys that type still type.
            (key("0").code("Numpad0").num_lock(), "0"),
            (key("Enter").code("NumpadEnter"), "\x1b[57414u"),
            (key("F1"), "\x1b[P"),
            (key("F3"), "\x1b[13~"),
            (key("F13"), "\x1b[57376u"),
            (key("F35"), "\x1b[57398u"),
            (key("ContextMenu"), "\x1b[57363u"),
            (key("MediaPlayPause"), "\x1b[57430u"),
            (key("Clear"), "\x1b[E"),
            (key("a").meta(), "\x1b[97;9u"),
            // Modifiers and locks are told only with every key.
            (key("Shift").code("ShiftLeft").shift(), ""),
            (key("CapsLock"), ""),
            // The keys are only told as they go down.
            (key("Escape").release(), ""),
            (key("a").ctrl().release(), ""),
        ],
    );
}

#[test]
fn kitty_reports_the_locks_with_every_escape_code() {
    // A lock is a modifier as soon as a flag is set, but never for a key that types.
    let modes = flags(0b1);
    check(
        &modes,
        &[
            (key("A").caps_lock(), "A"),
            (key("a").ctrl().caps_lock(), "\x1b[97;69u"),
            (key("ArrowUp").num_lock(), "\x1b[1;129A"),
            (key("Escape").caps_lock().num_lock(), "\x1b[27;193u"),
            // Enter, Tab and Backspace stay legacy with only the locks.
            (key("Enter").num_lock(), "\r"),
        ],
    );
}

#[test]
fn kitty_drops_decckm() {
    let modes = Modes {
        application_cursor: true,
        ..flags(0b1)
    };
    check(
        &modes,
        &[(key("ArrowUp"), "\x1b[A"), (key("Home"), "\x1b[H")],
    );
    // With only flags that leave the keys legacy, DECCKM still counts.
    let modes = Modes {
        application_cursor: true,
        ..flags(0b100)
    };
    check(&modes, &[(key("ArrowUp"), "\x1bOA")]);
}

#[test]
fn kitty_reports_repeats_and_releases() {
    // kitty_tests/keys.py, "test event type reporting". A key that types sends its text
    // as it goes down and repeats, as in kitty, whose test gives the key no text.
    check(
        &flags(0b10),
        &[
            (key("a"), "a"),
            (key("a").repeat(), "a"),
            (key("a").ctrl().repeat(), "\x1b[97;5:2u"),
            (key("a").release(), "\x1b[97;1:3u"),
            (key("A").shift().release(), "\x1b[97;2:3u"),
        ],
    );
    check(
        &flags(0b11),
        &[
            (key("Backspace"), "\x7f"),
            (key("Backspace").release(), ""),
            (key("Enter").num_lock().caps_lock(), "\r"),
            (key("Enter").num_lock().caps_lock().release(), ""),
            // A modified Enter has its release.
            (key("Enter").shift().release(), "\x1b[13;2:3u"),
            (key("ArrowUp").release(), "\x1b[1;1:3A"),
            (key("Escape").repeat(), "\x1b[27;1:2u"),
        ],
    );
}

#[test]
fn kitty_reports_the_shifted_and_base_layout_keys() {
    // kitty_tests/keys.py, "test alternate key reporting", and fish's 5.
    check(&flags(0b100), &[(key("a"), "a"), (key("A").shift(), "A")]);
    check(
        &flags(0b101),
        &[
            (key("A").ctrl().shift(), "\x1b[97:65;6u"),
            (key("!").ctrl().shift(), "\x1b[49:33;6u"),
            // AZERTY's a is a US q, and Cyrillic с a US c.
            (key("a").code("KeyQ").ctrl(), "\x1b[97::113;5u"),
            (key("A").code("KeyQ").ctrl().shift(), "\x1b[97:65:113;6u"),
            (key("с").code("KeyC").ctrl(), "\x1b[1089::99;5u"),
            // Functional keys have none.
            (key("ArrowUp").shift(), "\x1b[1;2A"),
            // What fish_key_reader is shown.
            (key("Tab"), "\t"),
            (key("i").ctrl(), "\x1b[105;5u"),
            (key("Escape"), "\x1b[27u"),
            (key("[").alt(), "\x1b[91;3u"),
        ],
    );
}

#[test]
fn kitty_reports_every_key() {
    // kitty_tests/keys.py, "test report all keys".
    check(
        &flags(0b1000),
        &[
            (key("a"), "\x1b[97u"),
            (key("a").repeat(), "\x1b[97u"),
            (key("a").ctrl(), "\x1b[97;5u"),
            (key("ArrowUp"), "\x1b[A"),
            (key("Shift").code("ShiftLeft"), "\x1b[57441u"),
            (key("Enter"), "\x1b[13u"),
            (key("Enter").ctrl(), "\x1b[13;5u"),
            (key("Tab"), "\x1b[9u"),
            (key("Backspace"), "\x1b[127u"),
            (key("Escape"), "\x1b[27u"),
            (key("A").caps_lock(), "\x1b[97;65u"),
            (key("0").code("Numpad0").num_lock(), "\x1b[57399;129u"),
        ],
    );
    // A modifier key has its own bit as it goes down, and not as it comes up, as the DOM
    // reports it.
    check(
        &flags(0b1010),
        &[
            (key("Shift").code("ShiftLeft").shift(), "\x1b[57441;2u"),
            (key("Shift").code("ShiftLeft").release(), "\x1b[57441;1:3u"),
            (key("Control").code("ControlRight").ctrl(), "\x1b[57448;5u"),
            (key("Meta").code("MetaLeft").meta(), "\x1b[57444;9u"),
            (key("Alt").code("AltRight").alt(), "\x1b[57449;3u"),
            (key("AltGraph").code("AltRight").alt_graph(), "\x1b[57453u"),
            (key("CapsLock").caps_lock(), "\x1b[57358;65u"),
            (key("Enter").release(), "\x1b[13;1:3u"),
        ],
    );
}

#[test]
fn kitty_reports_the_text_with_every_key() {
    // kitty_tests/keys.py, "test embed text".
    check(
        &flags(0b11000),
        &[
            (key("a"), "\x1b[97;;97u"),
            (key("A").shift(), "\x1b[97;2;65u"),
            (key("E\u{301}").code("KeyE").shift(), "\x1b[101;2;69:769u"),
            // Never with Ctrl, Alt or Super, and never as the key comes up.
            (key("a").ctrl(), "\x1b[97;5u"),
            (key("a").alt(), "\x1b[97;3u"),
            (key("a").meta(), "\x1b[97;9u"),
            (key("Enter"), "\x1b[13u"),
        ],
    );
    check(
        &flags(0b11111),
        &[
            (key("A").shift(), "\x1b[97:65;2;65u"),
            (key("A").shift().release(), "\x1b[97:65;2:3u"),
            (key("é").code("Digit2"), "\x1b[233::50;;233u"),
        ],
    );
}

#[test]
fn kitty_reports_what_option_typed_on_macos() {
    let mac = |modes: &Modes, event: Event| sent_on(true, modes, &event);
    assert_eq!(mac(&flags(0b1), key("å").code("KeyA").alt()), "å");
    assert_eq!(
        mac(&flags(0b11101), key("å").code("KeyA").alt()),
        "\x1b[229::97;;229u"
    );
    assert_eq!(mac(&flags(0b1), key("ArrowLeft").alt()), "\x1b[1;3D");
}

#[test]
fn kitty_legacy_letters_without_disambiguation() {
    // kitty_tests/keys.py's legacy letter tests, which apply while no flag that changes them
    // is set. Here that is alternate keys alone, with no Shift or base key to report.
    let modes = flags(0b100);
    check(
        &modes,
        &[
            (key("a").alt(), "\x1ba"),
            (key("a").ctrl(), "\x01"),
            (key("a").ctrl().alt(), "\x1b\x01"),
            (key("2").ctrl(), "\0"),
            (key("8").ctrl(), "\x7f"),
            (key("`").ctrl(), "`"),
            (key(" ").ctrl(), "\0"),
            (key(" ").alt(), "\x1b "),
            (key("i").ctrl().shift(), "\x1b[105;6u"),
        ],
    );
}

// ---------------------------------------------------------------------------------------
// Every key reaches the program (HW-01)
// ---------------------------------------------------------------------------------------

/// Every key a DOM can report, as a US keyboard has them, and the keypad with NumLock on and
/// off.
fn every_key() -> Vec<Event> {
    let mut keys: Vec<Event> = "abcdefghijklmnopqrstuvwxyz`1234567890-=[]\\;',./ "
        .chars()
        .map(|char| key(&char.to_string()))
        .collect();
    for name in [
        "Escape",
        "Enter",
        "Tab",
        "Backspace",
        "Insert",
        "Delete",
        "ArrowLeft",
        "ArrowRight",
        "ArrowUp",
        "ArrowDown",
        "PageUp",
        "PageDown",
        "Home",
        "End",
        "Clear",
        "ContextMenu",
    ] {
        keys.push(key(name));
    }
    keys.extend((1..=35).map(|n| key(&format!("F{n}"))));
    for (typed, moves, code) in [
        ("0", "Insert", "Numpad0"),
        ("1", "End", "Numpad1"),
        ("2", "ArrowDown", "Numpad2"),
        ("3", "PageDown", "Numpad3"),
        ("4", "ArrowLeft", "Numpad4"),
        ("5", "Clear", "Numpad5"),
        ("6", "ArrowRight", "Numpad6"),
        ("7", "Home", "Numpad7"),
        ("8", "ArrowUp", "Numpad8"),
        ("9", "PageUp", "Numpad9"),
        (".", "Delete", "NumpadDecimal"),
    ] {
        keys.push(key(typed).code(code).num_lock());
        keys.push(key(moves).code(code));
    }
    for (typed, code) in [
        ("/", "NumpadDivide"),
        ("*", "NumpadMultiply"),
        ("-", "NumpadSubtract"),
        ("+", "NumpadAdd"),
        ("=", "NumpadEqual"),
        (",", "NumpadComma"),
        ("Enter", "NumpadEnter"),
    ] {
        keys.push(key(typed).code(code));
    }
    keys
}

/// `event` with each of the 16 sets of Shift, Alt, Ctrl and Cmd held.
fn with_every_modifier(event: &Event) -> impl Iterator<Item = Event> + '_ {
    (0..16).map(move |bits: u8| {
        let mut event = event.clone();
        let held = &mut event.0.modifiers;
        held.shift = bits & 1 != 0;
        held.alt = bits & 2 != 0;
        held.ctrl = bits & 4 != 0;
        held.meta = bits & 8 != 0;
        if held.shift && !event.0.code.starts_with("Numpad") {
            // What a US keyboard types with Shift.
            let key = event.0.key.clone();
            event.0.key = match key.as_str() {
                "`" => "~",
                "1" => "!",
                "2" => "@",
                "3" => "#",
                "4" => "$",
                "5" => "%",
                "6" => "^",
                "7" => "&",
                "8" => "*",
                "9" => "(",
                "0" => ")",
                "-" => "_",
                "=" => "+",
                "[" => "{",
                "]" => "}",
                "\\" => "|",
                ";" => ":",
                "'" => "\"",
                "," => "<",
                "." => ">",
                "/" => "?",
                // A letter, or a name, which Shift leaves.
                other if other.chars().count() == 1 => {
                    event.0.key = other.to_uppercase();
                    return event;
                }
                other => other,
            }
            .to_string();
        }
        event
    })
}

#[test]
fn with_every_key_reported_every_key_and_modifier_reaches_the_program() {
    for modes in [flags(0b1000), flags(0b11000), flags(0b11111)] {
        let releases = modes.keyboard.contains(Keyboard::EVENT_TYPES);
        for key in every_key() {
            for event in with_every_modifier(&key) {
                assert_ne!(
                    sent_with(&modes, &event),
                    "",
                    "{:?} with {modes:?}",
                    event.0
                );
                let released = sent_with(&modes, &event.clone().release());
                assert_eq!(
                    released.is_empty(),
                    !releases,
                    "{:?} with {modes:?}",
                    event.0
                );
            }
        }
    }
}

#[test]
fn without_the_protocol_every_key_xterm_can_send_reaches_the_program() {
    // But for Cmd with a key that sends a character, which xterm cannot encode (DESIGN.md §3).
    for modes in [
        Modes::default(),
        Modes {
            application_cursor: true,
            application_keypad: true,
            ..Modes::default()
        },
    ] {
        for key in every_key() {
            for event in with_every_modifier(&key) {
                let sent = sent_with(&modes, &event);
                let typed_with_cmd = event.0.modifiers.meta
                    && (event.0.key.chars().count() == 1
                        || ["Enter", "Tab", "Backspace", "Escape"].contains(&event.0.key.as_str()));
                if typed_with_cmd {
                    assert_eq!(sent, "", "{:?}", event.0);
                } else {
                    assert_ne!(sent, "", "{:?} with {modes:?}", event.0);
                }
            }
        }
    }
}

#[test]
fn any_strings_from_the_webview_are_safe() {
    // The webview's strings are not trusted to name a key. Nothing may panic, whatever they
    // are and whatever the modes.
    let pieces = [
        "",
        "a",
        "A",
        "é",
        "ß",
        "İ",
        "ﬃ",
        "👍🏽",
        "\u{301}",
        "\0",
        "\x1b",
        "\x7f",
        "\u{9b}",
        "Key",
        "KeyA",
        "KeyZZ",
        "Digit",
        "Digit9",
        "Numpad",
        "Numpad5",
        "NumpadEnter",
        "Numpad55",
        "F",
        "F1",
        "F35",
        "F99",
        "F-1",
        "F+3",
        "Enter",
        "Dead",
        "Shift",
        "ShiftRight",
        "Right",
        "Clear",
        "NumLock",
        "Meta",
        "OS",
        "AltGraph",
        " ",
        "  ",
    ];
    let mut seed: u64 = 0x2545_f491_4f6c_dd1d;
    let mut next = |bound: usize| {
        seed ^= seed << 13;
        seed ^= seed >> 7;
        seed ^= seed << 17;
        (seed % bound as u64) as usize
    };
    let mut input = InputHandler::on(false);
    let mut mac = InputHandler::on(true);
    for _ in 0..20_000 {
        let mut string = || {
            (0..next(3))
                .map(|_| pieces[next(pieces.len())])
                .collect::<String>()
        };
        let event = KeyEvent {
            key: string(),
            code: string(),
            modifiers: Modifiers {
                shift: next(2) == 1,
                alt: next(2) == 1,
                ctrl: next(2) == 1,
                meta: next(2) == 1,
                alt_graph: next(2) == 1,
                caps_lock: next(2) == 1,
                num_lock: next(2) == 1,
            },
            kind: [Kind::Press, Kind::Repeat, Kind::Release][next(3)],
        };
        let modes = Modes {
            keyboard: Keyboard::new(next(128) as u8),
            application_cursor: next(2) == 1,
            application_keypad: next(2) == 1,
            newline: next(2) == 1,
            ..Modes::default()
        };
        input.handle(InputEvent::Key(event.clone()), &modes);
        mac.handle(InputEvent::Key(event), &modes);
    }
}

// ---------------------------------------------------------------------------------------
// Input methods, pastes and focus
// ---------------------------------------------------------------------------------------

#[test]
fn an_input_method_composes_then_commits() {
    let mut input = InputHandler::on(true);
    let modes = Modes::default();
    assert_eq!(
        input.handle(InputEvent::Compose("に".to_string()), &modes),
        Some(Action::Composing)
    );
    assert_eq!(input.composing(), "に");
    assert_eq!(
        input.handle(InputEvent::Compose("に".to_string()), &modes),
        None
    );
    assert_eq!(
        input.handle(InputEvent::Compose("日本".to_string()), &modes),
        Some(Action::Composing)
    );
    assert_eq!(
        input.handle(InputEvent::Compose(String::new()), &modes),
        Some(Action::Composing)
    );
    assert_eq!(
        input.handle(InputEvent::Text("日本".to_string()), &modes),
        Some(Action::Send("日本".as_bytes().to_vec()))
    );
    assert_eq!(input.composing(), "");
}

#[test]
fn committed_text_is_sent_as_it_is_in_every_mode() {
    // A dead key's é, and an input method's text, as kitty sends them.
    for modes in [Modes::default(), flags(0b1), flags(0b11111)] {
        let mut input = InputHandler::on(false);
        input.handle(InputEvent::Compose("´".to_string()), &modes);
        assert_eq!(
            input.handle(InputEvent::Text("é".to_string()), &modes),
            Some(Action::Send("é".as_bytes().to_vec()))
        );
        assert_eq!(input.composing(), "");
        assert_eq!(input.handle(InputEvent::Text(String::new()), &modes), None);
    }
}

fn pasted(text: &str, bracketed: bool) -> Option<Action> {
    let modes = Modes {
        bracketed_paste: bracketed,
        ..Modes::default()
    };
    InputHandler::on(false).handle(InputEvent::Paste(text.to_string()), &modes)
}

#[test]
fn a_paste_is_bracketed_when_the_program_asked() {
    assert_eq!(
        pasted("echo hi", true),
        Some(Action::Send(b"\x1b[200~echo hi\x1b[201~".to_vec()))
    );
    assert_eq!(
        pasted("echo hi", false),
        Some(Action::Send(b"echo hi".to_vec()))
    );
    assert_eq!(pasted("", true), None);
}

#[test]
fn a_paste_breaks_lines_as_enter_does() {
    let expected = b"one\rtwo\rthree\rfour\r\r".to_vec();
    for bracketed in [false, true] {
        let Some(Action::Send(bytes)) = pasted("one\ntwo\r\nthree\rfour\n\n", bracketed) else {
            panic!("nothing pasted");
        };
        let bytes = if bracketed {
            bytes[6..bytes.len() - 6].to_vec()
        } else {
            bytes
        };
        assert_eq!(bytes, expected);
    }
}

#[test]
fn a_paste_types_no_control_character() {
    // So pasted text cannot end the bracket early and run what follows, nor act on the
    // program: every control but tab and the line breaks becomes a space, C1 among them.
    assert_eq!(
        pasted("a\x1b[201~rm -rf ~\x03\x04\t\x7f\u{9b}201~é", true),
        Some(Action::Send(
            "\x1b[200~a [201~rm -rf ~  \t  201~é\x1b[201~"
                .as_bytes()
                .to_vec()
        ))
    );
    assert_eq!(
        pasted("\x00\x08\x1a", false),
        Some(Action::Send(b"   ".to_vec()))
    );
}

#[test]
fn focus_is_reported_when_the_program_asked() {
    let focus = Modes {
        focus: true,
        ..Modes::default()
    };
    let mut input = InputHandler::on(false);
    assert_eq!(
        input.handle(InputEvent::Focus(true), &focus),
        Some(Action::Send(b"\x1b[I".to_vec()))
    );
    assert_eq!(
        input.handle(InputEvent::Focus(false), &focus),
        Some(Action::Send(b"\x1b[O".to_vec()))
    );
    assert_eq!(
        input.handle(InputEvent::Focus(false), &Modes::default()),
        None
    );
}
