//! Twominal's own keys (Appendix A), taken before a key is encoded for the program, whatever the
//! kitty keyboard protocol asks, as kitty takes its own. So far a page of scrolling, Clear, Copy,
//! Paste, Select All and Find; #43 makes this the table of every shortcut, which the user can remap
//! (SET-06). DESIGN.md §3 has the rules.

use terminal_core::Modes;

use super::key::{Key, Mods, Named, Pad, Press};

/// What a shortcut does.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Shortcut {
    /// Scrolls the view a page, toward older output when `up`.
    Page { up: bool },
    /// Clears the screen and the scrollback, keeping the cursor's line (TERM-22).
    Clear,
    /// Copies the selection (TERM-17, SW-05).
    Copy,
    /// Pastes the clipboard (SW-05).
    Paste,
    /// Selects everything.
    SelectAll,
    /// Opens the find bar, or gives it the keys (TERM-18).
    Find,
}

/// The shortcut `press` is, if it is one. On `macos` Mod is Cmd, and elsewhere Ctrl+Shift.
pub(crate) fn read(press: &Press, modes: &Modes, macos: bool) -> Option<Shortcut> {
    let held = press.mods.held();
    let page = match press.key {
        Key::Named(Named::PageUp) | Key::Pad(Pad::PageUp) => Some(true),
        Key::Named(Named::PageDown) | Key::Pad(Pad::PageDown) => Some(false),
        _ => None,
    };
    // Shift alone: Ctrl+Shift+Page Up and Down move tabs off macOS (#43). The alternate screen
    // keeps no scrollback, so there the keys are the program's.
    if let Some(up) = page {
        return (held == Mods::SHIFT && !modes.alternate_screen).then_some(Shortcut::Page { up });
    }
    let modifier = if macos {
        Mods::SUPER
    } else {
        Mods::CTRL | Mods::SHIFT
    };
    if held != modifier {
        return None;
    }
    [
        ('k', Shortcut::Clear),
        ('c', Shortcut::Copy),
        ('v', Shortcut::Paste),
        ('a', Shortcut::SelectAll),
        ('f', Shortcut::Find),
    ]
    .into_iter()
    .find_map(|(letter, shortcut)| is(press, letter).then_some(shortcut))
}

/// Whether `press` is the key for `letter`: the key that types it on the layout, or on a layout
/// that types no ASCII there, the key in its place on a US keyboard, as Ctrl takes it.
pub(crate) fn is(press: &Press, letter: char) -> bool {
    match press.key {
        Key::Char(char) if char.is_ascii() => char == letter,
        Key::Char(_) => press.us.is_some_and(|(base, _)| base == letter),
        _ => false,
    }
}
