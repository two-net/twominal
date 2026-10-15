//! Tests of the Input Handler (TERM-08 to TERM-11, HW-01, HW-02): DOM key events, pastes,
//! focus changes, input-method text and mouse and wheel events in, the bytes a program reads
//! or what Twominal does out.
//!
//! The legacy cases are what xterm sends for `TERM=xterm-256color`, with the deviations
//! DESIGN.md §3 lists. The kitty cases are kitty's own, from `kitty_tests/keys.py`, given as
//! the DOM would give the key. The mouse's reports are xterm's, from `button.c`.

use terminal_core::{Keyboard, Modes, MouseFormat, MouseTracking};

use super::*;

/// macOS, or Linux, whose keys are Windows' too but for the block selection's modifier.
fn os(macos: bool) -> Os {
    if macos {
        Os::MacOs
    } else {
        Os::Linux
    }
}

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
    match InputHandler::on(os(macos)).handle(InputEvent::Key(event.0.clone()), modes) {
        Some(Action::Type(bytes) | Action::Send(bytes)) => String::from_utf8(bytes).unwrap(),
        Some(
            action @ (Action::Composing
            | Action::Pointer(_)
            | Action::Select(..)
            | Action::Scroll(_)
            | Action::Clear
            | Action::Copy
            | Action::Paste
            | Action::PastePrimary
            | Action::SelectAll
            | Action::ConfirmPaste { .. }),
        ) => {
            panic!("a key meant {action:?}")
        }
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
    // As in xterm. kitty sends CSI 98;6u even with no flag set. Ctrl+Shift with A, C, K and V
    // are shortcuts off macOS.
    check(
        &Modes::default(),
        &[
            (key("B").ctrl().shift(), "\x02"),
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
    let mut input = InputHandler::on(Os::Linux);
    assert_eq!(
        input.handle(InputEvent::Key(key("Escape").0), &Modes::default()),
        Some(Action::Type(vec![0x1b]))
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
            (key("PageUp").ctrl(), "\x1b[5;5~"),
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
            (pad("PageUp", "Numpad9").ctrl(), "\x1b[5;5~"),
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
            // kitty's case is A, which is Select All here (Appendix A).
            (key("B").ctrl().shift(), "\x1b[98;6u"),
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
            // kitty's case is A, which is Select All here (Appendix A).
            (key("B").ctrl().shift(), "\x1b[98:66;6u"),
            (key("!").ctrl().shift(), "\x1b[49:33;6u"),
            // AZERTY's a is a US q, its z a US w, and Cyrillic с a US c.
            (key("a").code("KeyQ").ctrl(), "\x1b[97::113;5u"),
            (key("Z").code("KeyW").ctrl().shift(), "\x1b[122:90:119;6u"),
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
    // But for Twominal's shortcuts.
    for modes in [flags(0b1000), flags(0b11000), flags(0b11111)] {
        let releases = modes.keyboard.contains(Keyboard::EVENT_TYPES);
        for key in every_key() {
            for event in with_every_modifier(&key).filter(|event| !is_shortcut(event, &modes)) {
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
    // But for Cmd with a key that sends a character, which xterm cannot encode (DESIGN.md §3),
    // and Twominal's shortcuts.
    for modes in [
        Modes::default(),
        Modes {
            application_cursor: true,
            application_keypad: true,
            ..Modes::default()
        },
    ] {
        for key in every_key() {
            for event in with_every_modifier(&key).filter(|event| !is_shortcut(event, &modes)) {
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
    let mut input = InputHandler::on(Os::Linux);
    let mut mac = InputHandler::on(Os::MacOs);
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
    let mut input = InputHandler::on(Os::MacOs);
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
        Some(Action::Type("日本".as_bytes().to_vec()))
    );
    assert_eq!(input.composing(), "");
}

#[test]
fn committed_text_is_sent_as_it_is_in_every_mode() {
    // A dead key's é, and an input method's text, as kitty sends them.
    for modes in [Modes::default(), flags(0b1), flags(0b11111)] {
        let mut input = InputHandler::on(Os::Linux);
        input.handle(InputEvent::Compose("´".to_string()), &modes);
        assert_eq!(
            input.handle(InputEvent::Text("é".to_string()), &modes),
            Some(Action::Type("é".as_bytes().to_vec()))
        );
        assert_eq!(input.composing(), "");
        assert_eq!(input.handle(InputEvent::Text(String::new()), &modes), None);
    }
}

/// What pasting `text` sends, with the multi-line warning off.
fn pasted(text: &str, bracketed: bool) -> Option<Action> {
    let modes = Modes {
        bracketed_paste: bracketed,
        ..Modes::default()
    };
    let mut input = InputHandler::on(Os::Linux);
    input.set_paste_warning(false);
    input.handle(InputEvent::Paste(text.to_string()), &modes)
}

/// An Input Handler with the multi-line warning on, as by default.
fn warning() -> InputHandler {
    InputHandler::on(Os::Linux)
}

#[test]
fn a_paste_of_lines_waits_for_the_user_when_the_program_did_not_ask_for_brackets() {
    // SEC-06: the line breaks would run what is before them.
    let off = Modes::default();
    let mut input = warning();
    assert_eq!(
        input.handle(InputEvent::Paste("ls\nrm x\n".to_string()), &off),
        Some(Action::ConfirmPaste { lines: 2 })
    );
    assert_eq!(
        input.confirm_paste(true, &off),
        Some(Action::Type(b"ls\rrm x\r".to_vec()))
    );
    // Once only.
    assert_eq!(input.confirm_paste(true, &off), None);
    // A no drops it.
    input.paste("a\r\nb".to_string(), &off);
    assert_eq!(input.confirm_paste(false, &off), None);
    assert_eq!(input.confirm_paste(true, &off), None);
    // A carriage return alone is a line break too, and one at the end starts no line.
    for (text, lines) in [("a\rb", 2), ("a\n", 1), ("\n", 1), ("a\r\n\r\nb", 3)] {
        assert_eq!(
            warning().paste(text.to_string(), &off),
            Some(Action::ConfirmPaste { lines }),
            "{text:?}"
        );
    }
}

#[test]
fn a_paste_that_cannot_run_anything_goes_at_once() {
    let off = Modes::default();
    let bracketed = Modes {
        bracketed_paste: true,
        ..Modes::default()
    };
    // One line, a program that asked for brackets, or the warning turned off.
    assert_eq!(
        warning().paste("echo hi".to_string(), &off),
        Some(Action::Type(b"echo hi".to_vec()))
    );
    assert_eq!(
        warning().paste("a\nb".to_string(), &bracketed),
        Some(Action::Type(b"\x1b[200~a\rb\x1b[201~".to_vec()))
    );
    let mut input = warning();
    input.set_paste_warning(false);
    assert_eq!(
        input.paste("a\nb".to_string(), &off),
        Some(Action::Type(b"a\rb".to_vec()))
    );
    assert_eq!(warning().paste(String::new(), &off), None);
}

#[test]
fn a_paste_waiting_gives_way_and_goes_as_the_modes_are_then() {
    let off = Modes::default();
    let bracketed = Modes {
        bracketed_paste: true,
        ..Modes::default()
    };
    let mut input = warning();
    input.paste("a\nb".to_string(), &off);
    input.paste("c\nd".to_string(), &off);
    // The program asked for brackets while the user was asked.
    assert_eq!(
        input.confirm_paste(true, &bracketed),
        Some(Action::Type(b"\x1b[200~c\rd\x1b[201~".to_vec()))
    );
    // A paste that goes at once leaves none waiting.
    input.paste("a\nb".to_string(), &off);
    input.paste("e".to_string(), &off);
    assert_eq!(input.confirm_paste(true, &off), None);
}

#[test]
fn a_paste_is_bracketed_when_the_program_asked() {
    assert_eq!(
        pasted("echo hi", true),
        Some(Action::Type(b"\x1b[200~echo hi\x1b[201~".to_vec()))
    );
    assert_eq!(
        pasted("echo hi", false),
        Some(Action::Type(b"echo hi".to_vec()))
    );
    assert_eq!(pasted("", true), None);
}

#[test]
fn a_paste_breaks_lines_as_enter_does() {
    let expected = b"one\rtwo\rthree\rfour\r\r".to_vec();
    for bracketed in [false, true] {
        let Some(Action::Type(bytes)) = pasted("one\ntwo\r\nthree\rfour\n\n", bracketed) else {
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
        Some(Action::Type(
            "\x1b[200~a [201~rm -rf ~  \t  201~é\x1b[201~"
                .as_bytes()
                .to_vec()
        ))
    );
    assert_eq!(
        pasted("\x00\x08\x1a", false),
        Some(Action::Type(b"   ".to_vec()))
    );
}

#[test]
fn focus_is_reported_when_the_program_asked() {
    let focus = Modes {
        focus: true,
        ..Modes::default()
    };
    let mut input = InputHandler::on(Os::Linux);
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

// ---------------------------------------------------------------------------------------
// Shortcuts (Appendix A) and what is typed
// ---------------------------------------------------------------------------------------

/// Whether `event` is one of Twominal's shortcuts on Linux, which the program never hears.
fn is_shortcut(event: &Event, modes: &Modes) -> bool {
    super::key::read(&event.0, false)
        .is_some_and(|press| super::shortcut::find(&press, modes, false).is_some())
}

/// What `event` means to an Input Handler as on macOS or not, with a grid of 24 rows.
fn shortcut_on(macos: bool, modes: &Modes, event: &Event) -> Option<Action> {
    let mut input = InputHandler::on(os(macos));
    input.resize(GRID);
    input.handle(InputEvent::Key(event.0.clone()), modes)
}

#[test]
fn shift_and_the_page_keys_scroll_a_page_on_the_primary_screen() {
    for modes in [Modes::default(), flags(0b11111)] {
        for (event, rows) in [
            (key("PageUp").shift(), -24.0),
            (key("PageDown").shift(), 24.0),
            (key("PageUp").code("Numpad9").shift(), -24.0),
            (key("PageDown").code("Numpad3").shift(), 24.0),
        ] {
            assert_eq!(
                shortcut_on(false, &modes, &event),
                Some(Action::Scroll(rows)),
                "{:?} with {modes:?}",
                event.0
            );
        }
    }
    // With no grid yet a page is no rows, and the key is still taken.
    let event = key("PageUp").shift();
    assert_eq!(sent(&event), "");
}

#[test]
fn on_the_alternate_screen_shift_and_the_page_keys_are_the_programs() {
    let alternate = Modes {
        alternate_screen: true,
        ..Modes::default()
    };
    let event = key("PageUp").shift();
    assert_eq!(
        shortcut_on(false, &alternate, &event),
        Some(Action::Type(b"\x1b[5;2~".to_vec()))
    );
    // Ctrl+Shift with them moves tabs off macOS (#43), so they never scroll.
    let event = key("PageUp").ctrl().shift();
    assert_eq!(sent(&event), "\x1b[5;6~");
}

#[test]
fn clear_is_cmd_k_on_macos_and_ctrl_shift_k_elsewhere() {
    for modes in [Modes::default(), flags(0b11111)] {
        assert_eq!(
            shortcut_on(true, &modes, &key("k").meta()),
            Some(Action::Clear)
        );
        assert_eq!(
            shortcut_on(false, &modes, &key("K").ctrl().shift()),
            Some(Action::Clear)
        );
        // On a layout that types no ASCII there, the key in K's place on a US keyboard.
        assert_eq!(
            shortcut_on(false, &modes, &key("Л").code("KeyK").ctrl().shift()),
            Some(Action::Clear)
        );
        // On one that types another letter there, the key that types k.
        assert_eq!(
            shortcut_on(false, &modes, &key("K").code("KeyV").ctrl().shift()),
            Some(Action::Clear)
        );
        assert_ne!(
            shortcut_on(false, &modes, &key("T").code("KeyK").ctrl().shift()),
            Some(Action::Clear)
        );
    }
    // Each OS's keys only.
    assert_ne!(
        shortcut_on(true, &Modes::default(), &key("K").ctrl().shift()),
        Some(Action::Clear)
    );
    assert_ne!(
        shortcut_on(false, &Modes::default(), &key("k").meta()),
        Some(Action::Clear)
    );
    assert_eq!(sent(&key("k").ctrl()), "\x0b");
}

#[test]
fn copy_paste_and_select_all_are_mod_with_c_v_and_a() {
    // Cmd on macOS, and Ctrl+Shift elsewhere (Appendix A), under the kitty protocol too.
    for (letter, action) in [
        ("c", Action::Copy),
        ("v", Action::Paste),
        ("a", Action::SelectAll),
    ] {
        let upper = letter.to_uppercase();
        let code = format!("Key{upper}");
        for modes in [Modes::default(), flags(0b11111)] {
            assert_eq!(
                shortcut_on(true, &modes, &key(letter).meta()),
                Some(action.clone())
            );
            assert_eq!(
                shortcut_on(false, &modes, &key(&upper).ctrl().shift()),
                Some(action.clone())
            );
            // On a layout that types no ASCII there, the key in its place on a US keyboard.
            assert_eq!(
                shortcut_on(false, &modes, &key("Ж").code(&code).ctrl().shift()),
                Some(action.clone())
            );
            // On one that types another letter there, the key that types it.
            assert_eq!(
                shortcut_on(true, &modes, &key(letter).code("KeyJ").meta()),
                Some(action.clone())
            );
            assert_ne!(
                shortcut_on(false, &modes, &key("J").code(&code).ctrl().shift()),
                Some(action.clone())
            );
        }
        // Each OS's keys only.
        assert_ne!(
            shortcut_on(true, &Modes::default(), &key(&upper).ctrl().shift()),
            Some(action.clone())
        );
        assert_ne!(
            shortcut_on(false, &Modes::default(), &key(letter).meta()),
            Some(action)
        );
    }
    // Ctrl with the letter alone is still the program's.
    assert_eq!(sent(&key("c").ctrl()), "\x03");
    assert_eq!(sent(&key("v").ctrl()), "\x16");
    assert_eq!(sent(&key("a").ctrl()), "\x01");
}

#[test]
fn the_webviews_copy_and_select_all_are_copy_and_select_all() {
    // What Edit ▸ Copy and Select All fire in the webview (#31).
    let mut input = InputHandler::on(Os::MacOs);
    let modes = Modes::default();
    assert_eq!(input.handle(InputEvent::Copy, &modes), Some(Action::Copy));
    assert_eq!(
        input.handle(InputEvent::SelectAll, &modes),
        Some(Action::SelectAll)
    );
}

#[test]
fn a_key_taken_repeats_as_taken_and_its_release_is_dropped() {
    // Every key and its release are reported.
    let modes = flags(0b11111);
    let mut input = InputHandler::on(Os::Linux);
    input.resize(GRID);
    let mut handle = |event: Event| input.handle(InputEvent::Key(event.0), &modes);
    assert_eq!(handle(key("PageUp").shift()), Some(Action::Scroll(-24.0)));
    assert_eq!(
        handle(key("PageUp").shift().repeat()),
        Some(Action::Scroll(-24.0))
    );
    // Shift came up first: the key is still taken.
    assert_eq!(handle(key("PageUp").release()), None);
    // Down again as itself, its release is the program's.
    assert!(matches!(handle(key("PageUp")), Some(Action::Type(_))));
    assert!(matches!(
        handle(key("PageUp").release()),
        Some(Action::Send(_))
    ));
    // Shift let go while the key repeats: it stays taken, and scrolls no more.
    assert_eq!(handle(key("PageUp").shift()), Some(Action::Scroll(-24.0)));
    assert_eq!(handle(key("PageUp").repeat()), None);
    assert_eq!(handle(key("PageUp").release()), None);
    // Shift held while a key the program had repeats: it stays the program's, so the program
    // hears it come up.
    assert!(matches!(handle(key("PageUp")), Some(Action::Type(_))));
    assert!(matches!(
        handle(key("PageUp").shift().repeat()),
        Some(Action::Type(_))
    ));
    assert!(matches!(
        handle(key("PageUp").release()),
        Some(Action::Send(_))
    ));
}

#[test]
fn what_is_typed_is_told_from_what_is_reported() {
    let every = flags(0b11111);
    let mut input = InputHandler::on(Os::Linux);
    let mut handle = |event: InputEvent, modes: &Modes| input.handle(event, modes);
    // Keys going down or repeating take the view to the bottom.
    assert_eq!(
        handle(InputEvent::Key(key("a").0), &Modes::default()),
        Some(Action::Type(b"a".to_vec()))
    );
    assert!(matches!(
        handle(InputEvent::Key(key("a").repeat().0), &every),
        Some(Action::Type(_))
    ));
    // Releases and modifiers on their own do not, so Shift does not undo Shift+Page Up.
    assert!(matches!(
        handle(InputEvent::Key(key("a").release().0), &every),
        Some(Action::Send(_))
    ));
    assert!(matches!(
        handle(
            InputEvent::Key(key("Shift").code("ShiftLeft").shift().0),
            &every
        ),
        Some(Action::Send(_))
    ));
    assert_eq!(
        handle(InputEvent::Text("é".to_string()), &Modes::default()),
        Some(Action::Type("é".as_bytes().to_vec()))
    );
    assert_eq!(
        handle(InputEvent::Paste("ls".to_string()), &Modes::default()),
        Some(Action::Type(b"ls".to_vec()))
    );
    let focus = Modes {
        focus: true,
        ..Modes::default()
    };
    assert_eq!(
        handle(InputEvent::Focus(true), &focus),
        Some(Action::Send(b"\x1b[I".to_vec()))
    );
}

// ---------------------------------------------------------------------------------------
// The mouse (TERM-10, HW-02)
// ---------------------------------------------------------------------------------------

/// A screen of 80 by 24 cells of 10 by 20 CSS pixels.
const GRID: Grid = Grid {
    size: Size { cols: 80, rows: 24 },
    cell_width: 10.0,
    cell_height: 20.0,
};

/// A screen wider and taller than any format's coordinates reach.
const HUGE: Grid = Grid {
    size: Size {
        cols: 3000,
        rows: 3000,
    },
    ..GRID
};

fn mouse_input(grid: Grid) -> InputHandler {
    let mut input = InputHandler::on(Os::Linux);
    input.resize(grid);
    input
}

/// The modes of a program that asked for `mouse` reports in `format`.
fn tracking(mouse: MouseTracking, format: MouseFormat) -> Modes {
    Modes {
        mouse,
        mouse_format: format,
        ..Modes::default()
    }
}

/// The DOM's bit in `buttons` for its `button`.
fn dom_bit(button: i16) -> u16 {
    match button {
        0 => 1,
        1 => 4,
        2 => 2,
        3 => 8,
        4 => 16,
        _ => 0,
    }
}

/// A mouse event in the left half of the cell at `row` and `col`, counted from 0.
#[derive(Clone, Copy)]
struct Click(MouseEvent);

fn at(kind: MouseKind, row: i32, col: i32) -> Click {
    Click(MouseEvent {
        kind,
        x: f64::from(col) * 10.0 + 2.0,
        y: f64::from(row) * 20.0 + 10.0,
        ..MouseEvent::default()
    })
}

/// The main button going down.
fn down(row: i32, col: i32) -> Click {
    at(MouseKind::Down, row, col).button(0)
}

/// The main button coming up.
fn up(row: i32, col: i32) -> Click {
    at(MouseKind::Up, row, col).button(0)
}

/// The mouse moving with `buttons` held, as the DOM's `buttons` has them.
fn moved(row: i32, col: i32, buttons: u16) -> Click {
    let mut event = at(MouseKind::Move, row, col);
    event.0.buttons = buttons;
    event
}

impl Click {
    /// The button, and `buttons` as the DOM gives them with it alone.
    fn button(mut self, button: i16) -> Self {
        self.0.button = button;
        self.0.buttons = if self.0.kind == MouseKind::Down {
            dom_bit(button)
        } else {
            0
        };
        self
    }
    fn buttons(mut self, buttons: u16) -> Self {
        self.0.buttons = buttons;
        self
    }
    fn x(mut self, x: f64) -> Self {
        self.0.x = x;
        self
    }
    fn y(mut self, y: f64) -> Self {
        self.0.y = y;
        self
    }
    fn detail(mut self, detail: u32) -> Self {
        self.0.detail = detail;
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
}

/// A wheel event in the cell at row 1 and column 2, down and right positive.
fn wheel(delta_x: f64, delta_y: f64, delta_mode: DeltaMode) -> WheelEvent {
    WheelEvent {
        delta_x,
        delta_y,
        delta_mode,
        x: 25.0,
        y: 30.0,
        modifiers: Modifiers::default(),
    }
}

/// A wheel turned down `pixels`, as a trackpad or a notch gives them.
fn pixels(pixels: f64) -> WheelEvent {
    wheel(0.0, pixels, DeltaMode::Pixel)
}

/// What `event` sends: the bytes, or none when it means nothing.
#[track_caller]
fn bytes(input: &mut InputHandler, modes: &Modes, event: impl Into<InputEvent>) -> Vec<u8> {
    match input.handle(event.into(), modes) {
        Some(Action::Send(bytes)) => bytes,
        None => Vec::new(),
        Some(action) => panic!("{action:?} is not for the program"),
    }
}

/// The Pointer action `event` means.
#[track_caller]
fn pointer(input: &mut InputHandler, modes: &Modes, event: impl Into<InputEvent>) -> Pointer {
    match input.handle(event.into(), modes) {
        Some(Action::Pointer(pointer) | Action::Select(pointer, _)) => pointer,
        other => panic!("{other:?} is no Pointer action"),
    }
}

/// How `event`, a gesture of the left button that is Twominal's, selects, and where.
#[track_caller]
fn selects(
    input: &mut InputHandler,
    modes: &Modes,
    event: impl Into<InputEvent>,
) -> (Pointer, Selecting) {
    match input.handle(event.into(), modes) {
        Some(Action::Select(pointer, by)) => (pointer, by),
        other => panic!("{other:?} selects nothing"),
    }
}

/// Checks that each event in turn sends what it is paired with. A hover is Twominal's, and
/// sends nothing.
#[track_caller]
fn reports(modes: &Modes, cases: &[(InputEvent, &[u8])]) {
    let mut input = mouse_input(HUGE);
    for (event, expected) in cases {
        let sent = match input.handle(event.clone(), modes) {
            Some(Action::Send(bytes)) => bytes,
            Some(Action::Pointer(Pointer {
                kind: PointerKind::Hover,
                ..
            }))
            | None => Vec::new(),
            Some(action) => panic!("{action:?} for {event:?} is not for the program"),
        };
        assert_eq!(sent, *expected, "{event:?} with {modes:?}");
    }
}

/// Checks that each event, the first a handler is given, sends what it is paired with.
#[track_caller]
fn first_reports(modes: &Modes, cases: &[(Click, &[u8])]) {
    for (event, expected) in cases {
        let sent = bytes(&mut mouse_input(HUGE), modes, *event);
        assert_eq!(sent, *expected, "{:?} with {modes:?}", event.0);
    }
}

impl From<Click> for InputEvent {
    fn from(click: Click) -> Self {
        InputEvent::Mouse(click.0)
    }
}

impl From<WheelEvent> for InputEvent {
    fn from(event: WheelEvent) -> Self {
        InputEvent::Wheel(event)
    }
}

#[test]
fn before_the_grid_is_known_the_mouse_means_nothing() {
    let modes = tracking(MouseTracking::Motion, MouseFormat::Sgr);
    let mut input = InputHandler::on(Os::Linux);
    assert_eq!(input.handle(down(0, 0).into(), &modes), None);
    assert_eq!(input.handle(pixels(100.0).into(), &modes), None);
    // A cell of no size, or of no number, is no grid.
    for (width, height) in [
        (0.0, 20.0),
        (10.0, -1.0),
        (f64::NAN, 20.0),
        (10.0, f64::INFINITY),
    ] {
        input.resize(Grid {
            cell_width: width,
            cell_height: height,
            ..GRID
        });
        assert_eq!(input.handle(down(0, 0).into(), &modes), None);
        assert_eq!(input.handle(pixels(100.0).into(), &Modes::default()), None);
    }
    input.resize(GRID);
    assert_eq!(
        bytes(&mut input, &modes, down(0, 0)),
        b"\x1b[<0;1;1M".to_vec()
    );
}

#[test]
fn a_click_is_reported_as_xterm_reports_it() {
    // Each button's press, and its release as 3, with the column and the row after it, from
    // 1, each a byte 32 more than its number.
    let modes = tracking(MouseTracking::Click, MouseFormat::Legacy);
    reports(
        &modes,
        &[
            (down(0, 0).into(), b"\x1b[M !!"),
            (up(0, 0).into(), b"\x1b[M#!!"),
            (down(1, 2).button(1).into(), b"\x1b[M!#\""),
            (up(1, 2).button(1).into(), b"\x1b[M##\""),
            (down(23, 79).button(2).into(), b"\x1b[M\"p8"),
            (up(23, 79).button(2).into(), b"\x1b[M#p8"),
            // Back and forward are X's buttons 8 and 9.
            (down(0, 0).button(3).into(), b"\x1b[M\xa0!!"),
            (up(0, 0).button(3).into(), b"\x1b[M#!!"),
            (down(0, 0).button(4).into(), b"\x1b[M\xa1!!"),
            (up(0, 0).button(4).into(), b"\x1b[M#!!"),
        ],
    );
}

#[test]
fn sgr_reports_say_which_button_came_up() {
    let modes = tracking(MouseTracking::Click, MouseFormat::Sgr);
    reports(
        &modes,
        &[
            (down(0, 0).into(), b"\x1b[<0;1;1M"),
            (up(0, 0).into(), b"\x1b[<0;1;1m"),
            (down(2, 4).button(2).into(), b"\x1b[<2;5;3M"),
            (up(2, 4).button(2).into(), b"\x1b[<2;5;3m"),
            (down(2, 4).button(1).into(), b"\x1b[<1;5;3M"),
            (up(2, 4).button(1).into(), b"\x1b[<1;5;3m"),
            (down(2, 4).button(3).into(), b"\x1b[<128;5;3M"),
            (up(2, 4).button(3).into(), b"\x1b[<128;5;3m"),
            (down(2, 4).button(4).into(), b"\x1b[<129;5;3M"),
            (up(2, 4).button(4).into(), b"\x1b[<129;5;3m"),
            // Nothing stops them: SGR's coordinates are extended (TERM-10).
            (down(2999, 2999).into(), b"\x1b[<0;3000;3000M"),
            (up(2999, 2999).into(), b"\x1b[<0;3000;3000m"),
        ],
    );
}

#[test]
fn urxvt_and_utf8_reports() {
    reports(
        &tracking(MouseTracking::Click, MouseFormat::Urxvt),
        &[
            (down(0, 0).into(), b"\x1b[32;1;1M"),
            (up(0, 0).into(), b"\x1b[35;1;1M"),
            (down(2, 4).button(3).ctrl().into(), b"\x1b[176;5;3M"),
            (up(2, 4).button(3).ctrl().into(), b"\x1b[51;5;3M"),
            (down(2999, 2999).into(), b"\x1b[32;3000;3000M"),
        ],
    );
    // The legacy format's bytes, but past 94 two bytes of UTF-8, the button's code too.
    reports(
        &tracking(MouseTracking::Click, MouseFormat::Utf8),
        &[
            (down(0, 0).into(), b"\x1b[M !!"),
            (up(0, 0).into(), b"\x1b[M#!!"),
            (down(0, 100).into(), b"\x1b[M \xc2\x85!"),
            (up(0, 100).into(), b"\x1b[M#\xc2\x85!"),
            (down(0, 0).button(3).into(), b"\x1b[M\xc2\xa0!!"),
        ],
    );
}

#[test]
fn legacy_coordinates_stop_at_223_with_a_zero_byte() {
    // xterm clamps a coordinate to its limit, and writes the limit as 0: what the byte became
    // before there was a limit.
    let modes = tracking(MouseTracking::Click, MouseFormat::Legacy);
    first_reports(
        &modes,
        &[
            (down(0, 221), b"\x1b[M \xfe!"),
            (down(0, 222), b"\x1b[M \xff!"),
            (down(0, 223), b"\x1b[M \x00!"),
            (down(0, 2000), b"\x1b[M \x00!"),
            (down(222, 0), b"\x1b[M !\xff"),
            (down(500, 500), b"\x1b[M \x00\x00"),
        ],
    );
}

#[test]
fn utf8_coordinates_stop_at_2015() {
    let modes = tracking(MouseTracking::Click, MouseFormat::Utf8);
    first_reports(
        &modes,
        &[
            (down(0, 93), b"\x1b[M ~!"),
            (down(0, 94), b"\x1b[M \x7f!"),
            (down(0, 95), b"\x1b[M \xc2\x80!"),
            (down(0, 2014), b"\x1b[M \xdf\xbf!"),
            (down(0, 2015), b"\x1b[M \x00!"),
            (down(2999, 2999), b"\x1b[M \x00\x00"),
        ],
    );
}

#[test]
fn x10_reports_only_presses_of_three_buttons() {
    // Mode 9: no release, no move, no modifier, no wheel, and none of back and forward.
    let modes = tracking(MouseTracking::Press, MouseFormat::Legacy);
    reports(
        &modes,
        &[
            (down(0, 0).into(), b"\x1b[M !!"),
            (moved(0, 3, 1).into(), b""),
            (up(0, 3).into(), b""),
            (down(1, 1).button(2).ctrl().alt().into(), b"\x1b[M\"\"\""),
            (up(1, 1).button(2).into(), b""),
            (down(1, 1).button(1).into(), b"\x1b[M!\"\""),
            (up(1, 1).button(1).into(), b""),
            (down(1, 1).button(3).into(), b""),
            (up(1, 1).button(3).into(), b""),
            (pixels(100.0).into(), b""),
        ],
    );
    // Not even on the alternate screen: xterm takes the wheel.
    let mut input = mouse_input(GRID);
    let full_screen = Modes {
        alternate_screen: true,
        ..modes
    };
    assert_eq!(input.handle(pixels(100.0).into(), &full_screen), None);
}

#[test]
fn drags_are_reported_in_1002_and_every_move_in_1003() {
    // 1000: presses and releases. A drag is the program's, and not reported.
    reports(
        &tracking(MouseTracking::Click, MouseFormat::Sgr),
        &[
            (down(0, 0).into(), b"\x1b[<0;1;1M"),
            (moved(0, 1, 1).into(), b""),
            (up(0, 1).into(), b"\x1b[<0;2;1m"),
            (moved(0, 2, 0).into(), b""),
        ],
    );
    // 1002: moves while a button is held, 32 added.
    reports(
        &tracking(MouseTracking::Drag, MouseFormat::Sgr),
        &[
            (down(0, 0).into(), b"\x1b[<0;1;1M"),
            (moved(0, 1, 1).into(), b"\x1b[<32;2;1M"),
            (moved(1, 1, 1).into(), b"\x1b[<32;2;2M"),
            (up(1, 1).into(), b"\x1b[<0;2;2m"),
            (moved(1, 2, 0).into(), b""),
        ],
    );
    // 1003: every move, with no button 3.
    reports(
        &tracking(MouseTracking::Motion, MouseFormat::Legacy),
        &[
            (moved(0, 0, 0).into(), b"\x1b[MC!!"),
            (down(0, 0).into(), b"\x1b[M !!"),
            (moved(0, 1, 1).into(), b"\x1b[M@\"!"),
            (up(0, 1).into(), b"\x1b[M#\"!"),
            (moved(0, 2, 0).into(), b"\x1b[MC#!"),
        ],
    );
}

#[test]
fn a_move_is_reported_only_in_another_cell() {
    let modes = tracking(MouseTracking::Motion, MouseFormat::Sgr);
    reports(
        &modes,
        &[
            (moved(0, 0, 0).x(1.0).into(), b"\x1b[<35;1;1M"),
            (moved(0, 0, 0).x(9.0).into(), b""),
            (moved(0, 0, 0).x(11.0).into(), b"\x1b[<35;2;1M"),
            // A press is where the next move is reported from, as in xterm.
            (down(1, 1).into(), b"\x1b[<0;2;2M"),
            (moved(1, 1, 1).x(19.0).into(), b""),
            (moved(1, 2, 1).into(), b"\x1b[<32;3;2M"),
        ],
    );
}

#[test]
fn motion_names_the_lowest_button_held() {
    // xterm's FirstBitN: the middle button before the right one.
    let modes = tracking(MouseTracking::Drag, MouseFormat::Sgr);
    reports(
        &modes,
        &[
            (down(0, 0).button(2).into(), b"\x1b[<2;1;1M"),
            (down(0, 0).button(1).buttons(2 | 4).into(), b"\x1b[<1;1;1M"),
            (moved(0, 1, 2 | 4).into(), b"\x1b[<33;2;1M"),
            (up(0, 1).button(1).buttons(2).into(), b"\x1b[<1;2;1m"),
            (moved(0, 2, 2).into(), b"\x1b[<34;3;1M"),
            (up(0, 2).button(2).into(), b"\x1b[<2;3;1m"),
        ],
    );
}

#[test]
fn alt_and_ctrl_are_reported_and_option_is_alt_on_macos() {
    // xterm's Meta, which is Alt, is 8, and Ctrl 16.
    let modes = tracking(MouseTracking::Drag, MouseFormat::Sgr);
    reports(
        &modes,
        &[
            (down(0, 0).alt().into(), b"\x1b[<8;1;1M"),
            (moved(0, 1, 1).alt().ctrl().into(), b"\x1b[<56;2;1M"),
            (up(0, 1).ctrl().into(), b"\x1b[<16;2;1m"),
            (
                WheelEvent {
                    modifiers: Modifiers {
                        ctrl: true,
                        alt: true,
                        ..Modifiers::default()
                    },
                    ..pixels(20.0)
                }
                .into(),
                b"\x1b[<89;3;2M",
            ),
        ],
    );
    // Option is Alt on a key that types nothing, and a click types nothing.
    let mut mac = InputHandler::on(Os::MacOs);
    mac.resize(GRID);
    assert_eq!(
        bytes(&mut mac, &modes, down(0, 0).alt()),
        b"\x1b[<8;1;1M".to_vec()
    );
}

#[test]
fn shift_keeps_the_gesture_for_selection() {
    // Shift+drag selects while a program has the mouse (TERM-10, Appendix A), and Shift may be
    // let go before the button: the gesture stays Twominal's.
    let modes = tracking(MouseTracking::Motion, MouseFormat::Sgr);
    let mut input = mouse_input(GRID);
    let press = pointer(&mut input, &modes, down(1, 2).shift().detail(1));
    assert_eq!(
        press,
        Pointer {
            kind: PointerKind::Press,
            button: Some(Button::Left),
            row: 1,
            col: 2,
            right_half: false,
            clicks: 1,
            modifiers: Modifiers {
                shift: true,
                ..Modifiers::default()
            },
        }
    );
    let drag = pointer(&mut input, &modes, moved(1, 5, 1));
    assert_eq!(
        (drag.kind, drag.button, drag.col),
        (PointerKind::Drag, Some(Button::Left), 5)
    );
    let release = pointer(&mut input, &modes, up(1, 6));
    assert_eq!((release.kind, release.col), (PointerKind::Release, 6));
    // A move with Shift is Twominal's too, and one without the program's again.
    assert_eq!(
        pointer(&mut input, &modes, moved(1, 7, 0).shift()).kind,
        PointerKind::Hover
    );
    assert_eq!(
        bytes(&mut input, &modes, down(1, 7)),
        b"\x1b[<0;8;2M".to_vec()
    );
}

#[test]
fn cmd_keeps_the_gesture_for_links() {
    // Cmd+click opens a link on macOS (TERM-20), and xterm's reports have no Cmd.
    let modes = tracking(MouseTracking::Click, MouseFormat::Sgr);
    let mut input = mouse_input(GRID);
    let press = pointer(&mut input, &modes, down(3, 4).meta());
    assert_eq!(
        (press.kind, press.row, press.col),
        (PointerKind::Press, 3, 4)
    );
    assert!(press.modifiers.meta);
    assert_eq!(
        pointer(&mut input, &modes, up(3, 4).meta()).kind,
        PointerKind::Release
    );
    // The wheel with Cmd scrolls the view.
    let turned = WheelEvent {
        modifiers: Modifiers {
            meta: true,
            ..Modifiers::default()
        },
        ..pixels(40.0)
    };
    assert_eq!(
        input.handle(turned.into(), &modes),
        Some(Action::Scroll(2.0))
    );
}

#[test]
fn a_gesture_stays_with_whoever_it_started_with() {
    // Shift pressed during the program's drag does not take it.
    let modes = tracking(MouseTracking::Drag, MouseFormat::Sgr);
    reports(
        &modes,
        &[
            (down(0, 0).into(), b"\x1b[<0;1;1M"),
            (moved(0, 1, 1).shift().into(), b"\x1b[<32;2;1M"),
            (
                down(0, 1).button(2).buttons(1 | 2).shift().into(),
                b"\x1b[<2;2;1M",
            ),
            (
                up(0, 1).button(2).buttons(1).shift().into(),
                b"\x1b[<2;2;1m",
            ),
            (up(0, 1).shift().into(), b"\x1b[<0;2;1m"),
        ],
    );
    // When the program stops asking in the middle of its gesture, the rest goes nowhere.
    let mut input = mouse_input(GRID);
    bytes(&mut input, &modes, down(0, 0));
    let off = Modes::default();
    assert_eq!(input.handle(moved(0, 3, 1).into(), &off), None);
    assert_eq!(input.handle(up(0, 3).into(), &off), None);
    // And Twominal's stays Twominal's when the program starts asking.
    let mut input = mouse_input(GRID);
    pointer(&mut input, &off, down(0, 0));
    assert_eq!(
        pointer(&mut input, &modes, up(0, 0)).kind,
        PointerKind::Release
    );
}

#[test]
fn a_gesture_whose_release_was_lost_is_released() {
    // The button came up where the window did not see it. The program hears the release
    // where the mouse is now, before what the event itself sends.
    let modes = tracking(MouseTracking::Drag, MouseFormat::Sgr);
    reports(
        &modes,
        &[
            (down(0, 0).into(), b"\x1b[<0;1;1M"),
            (moved(0, 3, 0).into(), b"\x1b[<0;4;1m"),
            (moved(0, 4, 0).into(), b""),
            (down(0, 0).into(), b"\x1b[<0;1;1M"),
            (down(1, 1).button(2).into(), b"\x1b[<0;2;2m\x1b[<2;2;2M"),
            (up(1, 1).button(2).into(), b"\x1b[<2;2;2m"),
        ],
    );
    // Twominal's gesture ends where the move is.
    let mut input = mouse_input(GRID);
    let off = Modes::default();
    pointer(&mut input, &off, down(0, 0).detail(2));
    let release = pointer(&mut input, &off, moved(2, 3, 0));
    assert_eq!(
        (
            release.kind,
            release.button,
            release.row,
            release.col,
            release.clicks
        ),
        (PointerKind::Release, Some(Button::Left), 2, 3, 2)
    );
    assert_eq!(
        pointer(&mut input, &off, moved(2, 4, 0)).kind,
        PointerKind::Hover
    );
}

#[test]
fn a_button_going_down_again_was_let_go_unseen() {
    // The DOM never presses a button that is down, so its release was lost.
    let modes = tracking(MouseTracking::Drag, MouseFormat::Sgr);
    reports(
        &modes,
        &[
            (down(0, 0).into(), b"\x1b[<0;1;1M"),
            (down(1, 1).into(), b"\x1b[<0;2;2m\x1b[<0;2;2M"),
            (up(1, 1).into(), b"\x1b[<0;2;2m"),
        ],
    );
    // Twominal's gesture is released, and ownership is chosen again at the next press.
    let mut input = mouse_input(GRID);
    pointer(&mut input, &modes, down(0, 0).shift());
    assert_eq!(
        pointer(&mut input, &modes, down(1, 1)).kind,
        PointerKind::Release
    );
    // A program that stopped asking while its gesture was lost hears nothing, and the press is
    // Twominal's.
    let mut input = mouse_input(GRID);
    bytes(&mut input, &modes, down(0, 0));
    let off = Modes::default();
    assert_eq!(
        pointer(&mut input, &off, down(1, 1)).kind,
        PointerKind::Press
    );
}

#[test]
fn an_event_that_cannot_go_with_a_lost_release_is_dropped() {
    // One action an event: the release, and what the event sends the program with it, but
    // nothing that is Twominal's. The event is then as if it had not come.
    let modes = tracking(MouseTracking::Drag, MouseFormat::Sgr);
    let mut input = mouse_input(GRID);
    bytes(&mut input, &modes, down(0, 0));
    assert_eq!(
        bytes(&mut input, &modes, down(1, 1).button(2).shift()),
        b"\x1b[<0;2;2m".to_vec()
    );
    // So the right button's drag and release are of no gesture.
    assert_eq!(
        pointer(&mut input, &modes, moved(1, 2, 2).shift()).kind,
        PointerKind::Hover
    );
    assert_eq!(
        input.handle(up(1, 2).button(2).shift().into(), &modes),
        None
    );
    // Twominal's gesture is released, and the press that showed it was lost is dropped.
    let off = Modes::default();
    let mut input = mouse_input(GRID);
    pointer(&mut input, &off, down(0, 0));
    let release = pointer(&mut input, &off, down(1, 1).button(2));
    assert_eq!(
        (release.kind, release.button),
        (PointerKind::Release, Some(Button::Left))
    );
    assert_eq!(input.handle(up(1, 1).button(2).into(), &off), None);
}

#[test]
fn a_release_without_a_press_is_not_reported() {
    // A button pressed outside the grid, then let go over it, is no gesture here.
    reports(
        &tracking(MouseTracking::Drag, MouseFormat::Sgr),
        &[
            (moved(0, 1, 1).into(), b""),
            (up(0, 1).into(), b""),
            (up(0, 1).button(9).into(), b""),
        ],
    );
    // With every move asked for, the move counts as one with no button.
    reports(
        &tracking(MouseTracking::Motion, MouseFormat::Sgr),
        &[
            (moved(0, 1, 1).into(), b"\x1b[<35;2;1M"),
            (up(0, 1).into(), b""),
        ],
    );
    let mut input = mouse_input(GRID);
    assert_eq!(input.handle(up(0, 1).into(), &Modes::default()), None);
}

#[test]
fn reports_clamp_positions_outside_the_grid() {
    // While a button is held the mouse is followed outside the grid, and xterm clamps it.
    let modes = tracking(MouseTracking::Drag, MouseFormat::Sgr);
    let mut input = mouse_input(GRID);
    let cases: [(Click, &[u8]); 4] = [
        (down(1, 1), b"\x1b[<0;2;2M"),
        (moved(0, 0, 1).x(-50.0).y(-50.0), b"\x1b[<32;1;1M"),
        (moved(0, 0, 1).x(5000.0).y(5000.0), b"\x1b[<32;80;24M"),
        (up(0, 0).x(801.0).y(-1.0), b"\x1b[<0;80;1m"),
    ];
    for (event, expected) in cases {
        assert_eq!(bytes(&mut input, &modes, event), expected.to_vec());
    }
}

#[test]
fn local_gestures_say_which_cell_and_half() {
    let off = Modes::default();
    let mut input = mouse_input(GRID);
    let place = |pointer: Pointer| (pointer.row, pointer.col, pointer.right_half);
    // The left half of the cell at row 1 and column 1, then its right half.
    let press = pointer(&mut input, &off, down(0, 0).x(14.0).y(25.0).detail(3));
    assert_eq!(place(press), (1, 1, false));
    let drag = pointer(&mut input, &off, moved(0, 0, 1).x(16.0).y(25.0));
    assert_eq!(
        (drag.kind, place(drag), drag.clicks),
        (PointerKind::Drag, (1, 1, true), 3)
    );
    // No action for a move that stays in the half.
    assert_eq!(
        input.handle(moved(0, 0, 1).x(19.0).y(39.0).into(), &off),
        None
    );
    // Outside the grid it is not clamped, so a selection can scroll on.
    let drag = pointer(&mut input, &off, moved(0, 0, 1).x(-15.0).y(-30.0));
    assert_eq!(place(drag), (-2, -2, true));
    let release = pointer(&mut input, &off, up(0, 0).x(805.0).y(500.0));
    assert_eq!(
        (release.kind, place(release), release.clicks),
        (PointerKind::Release, (25, 80, true), 3)
    );
    // With no button, a move is a hover, for a link's target (TERM-20).
    let hover = pointer(&mut input, &off, moved(3, 3, 0));
    assert_eq!(
        (hover.kind, hover.button, place(hover)),
        (PointerKind::Hover, None, (3, 3, false))
    );
    assert_eq!(input.handle(moved(3, 3, 0).into(), &off), None);
    // Cmd going down over a link shows where it goes, without a move to another cell.
    assert!(
        pointer(&mut input, &off, moved(3, 3, 0).meta())
            .modifiers
            .meta
    );
    // A right click, for the context menu (UI-04), and off Linux a middle one, which pastes
    // the primary selection there (SW-05).
    for (os, button) in [
        (Os::Linux, Button::Right),
        (Os::MacOs, Button::Middle),
        (Os::Windows, Button::Middle),
    ] {
        let mut input = InputHandler::on(os);
        input.resize(GRID);
        let dom = if button == Button::Right { 2 } else { 1 };
        let press = pointer(&mut input, &off, down(3, 3).button(dom));
        assert_eq!(
            (press.kind, press.button),
            (PointerKind::Press, Some(button))
        );
        pointer(&mut input, &off, up(3, 3).button(dom));
    }
}

#[test]
fn the_left_button_selects_by_its_clicks() {
    // TERM-17: a drag by characters, a double-click by words, a triple-click by lines, and
    // round again, as the DOM counts on.
    let off = Modes::default();
    for (clicks, by) in [
        (0, Selecting::Characters),
        (1, Selecting::Characters),
        (2, Selecting::Words),
        (3, Selecting::Lines),
        (4, Selecting::Characters),
        (5, Selecting::Words),
        (6, Selecting::Lines),
    ] {
        let mut input = mouse_input(GRID);
        let (press, selecting) = selects(&mut input, &off, down(1, 2).detail(clicks));
        assert_eq!(
            (press.kind, press.row, press.col, selecting),
            (PointerKind::Press, 1, 2, by)
        );
        // The drags and the release select as the press did.
        let (drag, selecting) = selects(&mut input, &off, moved(3, 4, 1));
        assert_eq!(
            (drag.kind, drag.row, drag.col, selecting),
            (PointerKind::Drag, 3, 4, by)
        );
        let (release, selecting) = selects(&mut input, &off, up(3, 5));
        assert_eq!(
            (release.kind, release.col, selecting),
            (PointerKind::Release, 5, by)
        );
    }
}

#[test]
fn each_os_selects_a_block_with_its_own_modifier() {
    // Option+drag on macOS, Ctrl+drag on Linux and Alt+drag on Windows (Appendix A). Option is
    // Alt to the DOM.
    let off = Modes::default();
    for (os, block, other) in [
        (Os::MacOs, down(0, 0).alt(), down(0, 0).ctrl()),
        (Os::Linux, down(0, 0).ctrl(), down(0, 0).alt()),
        (Os::Windows, down(0, 0).alt(), down(0, 0).ctrl()),
    ] {
        let mut input = InputHandler::on(os);
        input.resize(GRID);
        assert_eq!(
            selects(&mut input, &off, block.detail(2)).1,
            Selecting::Block
        );
        // The modifier counts at the press only.
        assert_eq!(
            selects(&mut input, &off, moved(1, 1, 1)).1,
            Selecting::Block
        );
        assert_eq!(selects(&mut input, &off, up(1, 1)).1, Selecting::Block);
        assert_eq!(
            selects(&mut input, &off, other).1,
            Selecting::Characters,
            "{os:?}"
        );
    }
}

#[test]
fn shift_selects_while_a_program_has_the_mouse() {
    // Appendix A, and a block with the OS's modifier as well.
    let modes = tracking(MouseTracking::Drag, MouseFormat::Sgr);
    let mut input = mouse_input(GRID);
    assert_eq!(
        selects(&mut input, &modes, down(0, 0).shift().detail(2)).1,
        Selecting::Words
    );
    input.handle(up(0, 0).into(), &modes);
    assert_eq!(
        selects(&mut input, &modes, down(0, 0).shift().ctrl()).1,
        Selecting::Block
    );
    input.handle(up(0, 0).into(), &modes);
    // Without Shift the program hears it, Ctrl included.
    assert_eq!(
        bytes(&mut input, &modes, down(0, 0).ctrl()),
        b"\x1b[<16;1;1M".to_vec()
    );
}

#[test]
fn a_lost_release_of_the_left_button_ends_the_selection() {
    let off = Modes::default();
    let mut input = mouse_input(GRID);
    selects(&mut input, &off, down(0, 0).detail(3));
    let (release, by) = selects(&mut input, &off, moved(2, 3, 0));
    assert_eq!((release.kind, by), (PointerKind::Release, Selecting::Lines));
    // The right button's is no selection's.
    pointer(&mut input, &off, down(0, 0).button(2));
    let release = input.handle(moved(1, 1, 0).into(), &off);
    assert!(
        matches!(
            release,
            Some(Action::Pointer(Pointer {
                kind: PointerKind::Release,
                button: Some(Button::Right),
                ..
            }))
        ),
        "{release:?}"
    );
    // A left press while the right button is held selects, and its drags too.
    pointer(&mut input, &off, down(0, 0).button(2));
    assert_eq!(
        selects(&mut input, &off, down(0, 1).buttons(1 | 2)).1,
        Selecting::Characters
    );
    assert_eq!(
        selects(&mut input, &off, moved(0, 2, 1 | 2)).0.kind,
        PointerKind::Drag
    );
}

#[test]
fn the_middle_button_pastes_the_primary_selection_on_linux() {
    // SW-05, Appendix A.
    let off = Modes::default();
    let mut input = mouse_input(GRID);
    assert_eq!(
        input.handle(down(2, 2).button(1).into(), &off),
        Some(Action::PastePrimary)
    );
    pointer(&mut input, &off, up(2, 2).button(1));
    // A program that asked for the mouse hears it, as in xterm, unless Shift is held.
    let modes = tracking(MouseTracking::Click, MouseFormat::Sgr);
    assert_eq!(
        bytes(&mut input, &modes, down(2, 2).button(1)),
        b"\x1b[<1;3;3M".to_vec()
    );
    input.handle(up(2, 2).button(1).into(), &modes);
    assert_eq!(
        input.handle(down(2, 2).button(1).shift().into(), &modes),
        Some(Action::PastePrimary)
    );
}

#[test]
fn a_pointer_is_on_the_row_drawn_under_it() {
    // The view is scrolled half a row past its top row, so its rows are drawn half a row up
    // (ANIM-06). Cells are 20 pixels high.
    let off = Modes::default();
    let mut input = mouse_input(GRID);
    input.set_offset(0.5);
    for (y, row) in [
        (5.0, 0),
        (9.9, 0),
        (10.0, 1),
        (35.0, 2),
        (-5.0, 0),
        (-15.0, -1),
    ] {
        let (press, _) = selects(&mut input, &off, down(0, 0).y(y));
        assert_eq!(press.row, row, "{y}");
        input.handle(up(0, 0).y(y).into(), &off);
    }
    // Reports are of the grid's cells, which the view does not move.
    let modes = tracking(MouseTracking::Click, MouseFormat::Sgr);
    assert_eq!(
        bytes(&mut input, &modes, down(0, 0).y(15.0)),
        b"\x1b[<0;1;1M".to_vec()
    );
    // Anything but a part of a row is no offset.
    for offset in [1.0, -0.25, f64::NAN, f64::INFINITY] {
        let mut input = mouse_input(GRID);
        input.set_offset(offset);
        assert_eq!(
            selects(&mut input, &off, down(0, 0).y(15.0)).0.row,
            0,
            "{offset}"
        );
    }
}

#[test]
fn a_wheel_notch_is_one_report() {
    // However many rows the OS makes of a notch, a program hears one step, and scrolls by its
    // own: 100 pixels is five rows here.
    let modes = tracking(MouseTracking::Click, MouseFormat::Sgr);
    reports(
        &modes,
        &[
            (pixels(100.0).into(), b"\x1b[<65;3;2M"),
            (pixels(-100.0).into(), b"\x1b[<64;3;2M"),
            (wheel(0.0, 3.0, DeltaMode::Line).into(), b"\x1b[<65;3;2M"),
            (wheel(0.0, -1.0, DeltaMode::Page).into(), b"\x1b[<64;3;2M"),
        ],
    );
    reports(
        &tracking(MouseTracking::Click, MouseFormat::Legacy),
        &[(pixels(100.0).into(), b"\x1b[Ma#\"")],
    );
}

#[test]
fn trackpad_pixels_add_up_to_rows() {
    // A trackpad gives a few pixels at a time: a report each time they make a row of 20.
    let modes = tracking(MouseTracking::Click, MouseFormat::Sgr);
    let mut input = mouse_input(GRID);
    let sent: Vec<bool> = (0..9)
        .map(|_| !bytes(&mut input, &modes, pixels(7.0)).is_empty())
        .collect();
    assert_eq!(
        sent,
        [false, false, true, false, false, true, false, false, true]
    );
}

#[test]
fn tenths_of_a_row_make_a_row() {
    // Ten turns of 2 pixels of 20 make a row, though in floating point they come to a hair
    // less.
    let modes = tracking(MouseTracking::Click, MouseFormat::Sgr);
    let mut input = mouse_input(GRID);
    let sent: Vec<bool> = (0..10)
        .map(|_| !bytes(&mut input, &modes, pixels(2.0)).is_empty())
        .collect();
    assert_eq!(sent.iter().position(|&sent| sent), Some(9));
    let full_screen = Modes {
        alternate_screen: true,
        ..Modes::default()
    };
    let keys: Vec<Vec<u8>> = (0..10)
        .map(|_| bytes(&mut input, &full_screen, wheel(0.0, 0.1, DeltaMode::Line)))
        .collect();
    assert_eq!(keys[9], b"\x1b[B".to_vec());
    assert!(keys[..9].iter().all(Vec::is_empty));
}

#[test]
fn a_flick_is_not_owed_later() {
    // A large turn is one report, and what was left over is no more than part of a row, so
    // the program does not keep scrolling once the trackpad stops.
    let modes = tracking(MouseTracking::Click, MouseFormat::Sgr);
    reports(
        &modes,
        &[
            (pixels(250.0).into(), b"\x1b[<65;3;2M"),
            (pixels(5.0).into(), b""),
            (pixels(5.0).into(), b"\x1b[<65;3;2M"),
            (pixels(1.0).into(), b""),
        ],
    );
}

#[test]
fn the_wheel_turning_back_starts_again() {
    // What was turned one way does not have to be undone the other.
    let modes = tracking(MouseTracking::Click, MouseFormat::Sgr);
    reports(
        &modes,
        &[
            (pixels(15.0).into(), b""),
            (pixels(-15.0).into(), b""),
            (pixels(-10.0).into(), b"\x1b[<64;3;2M"),
        ],
    );
}

#[test]
fn line_and_page_deltas() {
    let modes = tracking(MouseTracking::Click, MouseFormat::Sgr);
    reports(
        &modes,
        &[
            (wheel(0.0, 0.5, DeltaMode::Line).into(), b""),
            (wheel(0.0, 0.5, DeltaMode::Line).into(), b"\x1b[<65;3;2M"),
        ],
    );
    // The view scrolls by rows, a page being the screen's.
    let mut input = mouse_input(GRID);
    let off = Modes::default();
    for (event, rows) in [
        (pixels(10.0), 0.5),
        (pixels(-60.0), -3.0),
        (wheel(0.0, 2.0, DeltaMode::Line), 2.0),
        (wheel(0.0, -1.0, DeltaMode::Page), -24.0),
    ] {
        assert_eq!(input.handle(event.into(), &off), Some(Action::Scroll(rows)));
    }
}

#[test]
fn only_the_leading_axis_is_reported() {
    // A trackpad drifts: only the axis the wheel turned most counts, sideways 66 and 67.
    let modes = tracking(MouseTracking::Click, MouseFormat::Sgr);
    reports(
        &modes,
        &[
            (wheel(30.0, 10.0, DeltaMode::Pixel).into(), b"\x1b[<67;3;2M"),
            (wheel(2.0, 40.0, DeltaMode::Pixel).into(), b"\x1b[<65;3;2M"),
            (wheel(-30.0, 0.0, DeltaMode::Pixel).into(), b"\x1b[<66;3;2M"),
            // Sideways drift adds nothing up.
            (wheel(9.0, 20.0, DeltaMode::Pixel).into(), b"\x1b[<65;3;2M"),
            (wheel(9.0, 20.0, DeltaMode::Pixel).into(), b"\x1b[<65;3;2M"),
            // The pixels are compared, not the cells: 12 is less than 20, though more columns
            // than rows.
            (wheel(12.0, 20.0, DeltaMode::Pixel).into(), b"\x1b[<65;3;2M"),
        ],
    );
    // The view does not scroll sideways.
    let mut input = mouse_input(GRID);
    assert_eq!(
        input.handle(wheel(30.0, 0.0, DeltaMode::Pixel).into(), &Modes::default()),
        None
    );
}

#[test]
fn alternate_scroll_sends_the_cursor_keys() {
    // On the alternate screen, a program that did not ask for the mouse gets a cursor key for
    // each row, as the key sends it (1007).
    let full_screen = Modes {
        alternate_screen: true,
        ..Modes::default()
    };
    reports(
        &full_screen,
        &[
            (pixels(60.0).into(), b"\x1b[B\x1b[B\x1b[B"),
            (pixels(-25.0).into(), b"\x1b[A"),
            (pixels(-10.0).into(), b""),
            (pixels(-5.0).into(), b"\x1b[A"),
            (wheel(0.0, 1.0, DeltaMode::Line).into(), b"\x1b[B"),
            // Sideways is no key.
            (wheel(-30.0, 0.0, DeltaMode::Pixel).into(), b""),
        ],
    );
    // In application cursor mode, as less and vim have it.
    reports(
        &Modes {
            application_cursor: true,
            ..full_screen
        },
        &[(pixels(40.0).into(), b"\x1bOB\x1bOB")],
    );
    // With the kitty keyboard protocol, as kitty sends them: released too, when asked.
    reports(
        &Modes {
            keyboard: Keyboard::new(0b11),
            ..full_screen
        },
        &[(pixels(-20.0).into(), b"\x1b[A\x1b[1;1:3A")],
    );
    // No more than a screen of keys, however far it turned.
    let mut input = mouse_input(GRID);
    assert_eq!(
        bytes(&mut input, &full_screen, pixels(1e9)),
        b"\x1b[B".repeat(24)
    );
    // On the primary screen, or with 1007 off, the view scrolls instead.
    for modes in [
        Modes::default(),
        Modes {
            alternate_scroll: false,
            ..full_screen
        },
    ] {
        assert_eq!(
            input.handle(pixels(60.0).into(), &modes),
            Some(Action::Scroll(3.0))
        );
    }
}

#[test]
fn the_wheel_scrolls_locally_otherwise() {
    // With Shift over a program that has the mouse, the view scrolls. macOS and Chromium turn
    // a wheel sideways while Shift is held, so a sideways turn is rows then.
    let modes = tracking(MouseTracking::Motion, MouseFormat::Sgr);
    let mut input = mouse_input(GRID);
    let shift = Modifiers {
        shift: true,
        ..Modifiers::default()
    };
    for (event, rows) in [
        (pixels(40.0), 2.0),
        (wheel(40.0, 0.0, DeltaMode::Pixel), 2.0),
        (wheel(-1.0, 0.0, DeltaMode::Line), -1.0),
    ] {
        let event = WheelEvent {
            modifiers: shift,
            ..event
        };
        assert_eq!(
            input.handle(event.into(), &modes),
            Some(Action::Scroll(rows))
        );
    }
    // Shift on the alternate screen scrolls the view too, not the program.
    let full_screen = Modes {
        alternate_screen: true,
        ..Modes::default()
    };
    let event = WheelEvent {
        modifiers: shift,
        ..pixels(40.0)
    };
    assert_eq!(
        input.handle(event.into(), &full_screen),
        Some(Action::Scroll(2.0))
    );
}

#[test]
fn nothing_the_webview_sends_can_panic_or_flood() {
    // The webview's numbers are not trusted. Whatever they are, with whatever modes and grid,
    // nothing panics, and no event sends more than a screen of cursor keys.
    let numbers = [
        0.0,
        -0.0,
        0.4,
        -1.0,
        19.9,
        20.0,
        1e300,
        -1e300,
        f64::MIN_POSITIVE,
        f64::MAX,
        f64::NAN,
        f64::INFINITY,
        f64::NEG_INFINITY,
    ];
    let cells = [10.0, 1e-300, 1e300, 0.0, -5.0, f64::NAN];
    let sizes = [(0, 0), (1, 1), (80, 24), (u16::MAX, u16::MAX)];
    let mut seed: u64 = 0x9e37_79b9_7f4a_7c15;
    let mut next = |bound: usize| {
        seed ^= seed << 13;
        seed ^= seed >> 7;
        seed ^= seed << 17;
        (seed % bound as u64) as usize
    };
    let mut input = mouse_input(GRID);
    let mut screen = GRID.size.rows;
    for step in 0..50_000 {
        if step % 500 == 0 {
            let (cols, rows) = sizes[next(sizes.len())];
            screen = rows;
            input.resize(Grid {
                size: Size { cols, rows },
                cell_width: cells[next(cells.len())],
                cell_height: cells[next(cells.len())],
            });
        }
        let modifiers = Modifiers {
            shift: next(4) == 0,
            alt: next(2) == 1,
            ctrl: next(2) == 1,
            meta: next(4) == 0,
            ..Modifiers::default()
        };
        let event = if next(3) == 0 {
            InputEvent::Wheel(WheelEvent {
                delta_x: numbers[next(numbers.len())],
                delta_y: numbers[next(numbers.len())],
                delta_mode: [DeltaMode::Pixel, DeltaMode::Line, DeltaMode::Page][next(3)],
                x: numbers[next(numbers.len())],
                y: numbers[next(numbers.len())],
                modifiers,
            })
        } else {
            InputEvent::Mouse(MouseEvent {
                kind: [MouseKind::Down, MouseKind::Up, MouseKind::Move][next(3)],
                button: [-1, 0, 1, 2, 3, 4, 5, 99, i16::MIN][next(9)],
                buttons: [0, 1, 2, 3, 31, 0xffff][next(6)],
                detail: [0, 1, 2, 3, u32::MAX][next(5)],
                x: numbers[next(numbers.len())],
                y: numbers[next(numbers.len())],
                modifiers,
            })
        };
        let modes = Modes {
            mouse: [
                MouseTracking::Off,
                MouseTracking::Press,
                MouseTracking::Click,
                MouseTracking::Drag,
                MouseTracking::Motion,
            ][next(5)],
            mouse_format: [
                MouseFormat::Legacy,
                MouseFormat::Utf8,
                MouseFormat::Sgr,
                MouseFormat::Urxvt,
            ][next(4)],
            alternate_screen: next(2) == 1,
            alternate_scroll: next(2) == 1,
            application_cursor: next(2) == 1,
            keyboard: Keyboard::new(next(32) as u8),
            ..Modes::default()
        };
        match input.handle(event.clone(), &modes) {
            // A cursor key with the kitty protocol's release is 13 bytes, and a report or
            // the releases of the five buttons less than 200.
            Some(Action::Type(bytes) | Action::Send(bytes)) => assert!(
                bytes.len() <= usize::from(screen) * 13 + 200,
                "{} bytes for {event:?}",
                bytes.len()
            ),
            Some(Action::Scroll(rows)) => assert!(rows.is_finite(), "{rows} for {event:?}"),
            _ => {}
        }
    }
}
