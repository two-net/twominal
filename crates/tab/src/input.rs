//! Input Handler (SRS §6.1).
//!
//! Turns keys and mouse events into shortcuts, command-line edits, encoded input or Vim mode
//! actions.
//!
//! Must not draw anything.
//!
//! This is the half that decides. Its other half, `src/tab/input.js`, captures keyboard,
//! input-method and mouse events in the webview and normalises them into [`InputEvent`]s. It
//! never interprets them. A shortcut found here and the same command chosen from a native
//! menu are one action.
//!
//! So far every event is passthrough: `key.rs` reads what a key event means, on any layout,
//! and `encode.rs` turns it, a paste or a change of focus into the bytes a program expects,
//! by the modes the Terminal Core keeps (TERM-08, TERM-09, TERM-11, HW-01). DESIGN.md §3 has
//! the rules, and the contract the webview half keeps.
//!
//! Filled in by #20 (keys and input methods), #21 (mouse), #43 (shortcuts) and #36–#37
//! (Scrollback Vim mode).

mod encode;
mod key;
#[cfg(test)]
mod tests;

use terminal_core::Modes;

/// One tab's input state: which mode it is in and what the keys mean there.
pub struct InputHandler {
    /// Whether Option types characters, as on macOS (DESIGN.md §3).
    macos: bool,
    /// The text an input method is composing, shown at the cursor until it is committed.
    composing: String,
}

/// A key, input-method or mouse event as the webview captured it, before it means anything.
/// The wire format between the two halves of the Input Handler. No other code reads it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum InputEvent {
    /// A key went down, was held, or came up: `keydown` and `keyup`.
    Key(KeyEvent),
    /// Text an input method, a dead key or the OS committed, such as the emoji picker's.
    Text(String),
    /// The text an input method is composing. Empty when the composition ends.
    Compose(String),
    /// Text the user pasted.
    Paste(String),
    /// The window gained focus, or lost it.
    Focus(bool),
}

/// A key event as the DOM gives it.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct KeyEvent {
    /// The DOM's `key`: what the key typed, such as `a`, `A` or `å`, or the key's name, such as
    /// `Enter`, `ArrowUp`, `F5`, `Shift` or `Dead`.
    pub key: String,
    /// The DOM's `code`: where the key is, named after a US keyboard, such as `KeyA`,
    /// `Numpad5` or `ShiftLeft`.
    pub code: String,
    pub modifiers: Modifiers,
    pub kind: Kind,
}

/// The modifiers held for a key event, and the locks that were on.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Modifiers {
    pub shift: bool,
    /// Alt, which is Option on macOS.
    pub alt: bool,
    pub ctrl: bool,
    /// Cmd on macOS, the Windows key elsewhere.
    pub meta: bool,
    /// AltGr, which types the third character of a key on Linux and Windows.
    pub alt_graph: bool,
    pub caps_lock: bool,
    pub num_lock: bool,
}

/// Whether a key went down, was held down, or came up.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Kind {
    #[default]
    Press,
    Repeat,
    Release,
}

/// What an input event means.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Action {
    /// Bytes to write to the session.
    Send(Vec<u8>),
    /// The text being composed changed, so the Renderer must show it at the cursor (#22).
    Composing,
}

impl InputHandler {
    pub fn new() -> Self {
        Self::on(cfg!(target_os = "macos"))
    }

    /// An Input Handler as on macOS, or as on Linux and Windows.
    fn on(macos: bool) -> Self {
        Self {
            macos,
            composing: String::new(),
        }
    }

    /// What `event` means, with the modes the program set. `None` when it means nothing: a
    /// key the program cannot be told of, or one it did not ask to hear about. Nothing is
    /// waited for, so Esc is sent the moment it comes (TERM-08).
    pub fn handle(&mut self, event: InputEvent, modes: &Modes) -> Option<Action> {
        let bytes = match event {
            InputEvent::Key(event) => {
                let press = key::read(&event, self.macos)?;
                encode::key(&press, modes)
            }
            InputEvent::Text(text) => {
                self.composing.clear();
                text.into_bytes()
            }
            InputEvent::Compose(text) => {
                if text == self.composing {
                    return None;
                }
                self.composing = text;
                return Some(Action::Composing);
            }
            InputEvent::Paste(text) if text.is_empty() => return None,
            InputEvent::Paste(text) => encode::paste(&text, modes.bracketed_paste),
            InputEvent::Focus(focused) if modes.focus => encode::focus(focused).to_vec(),
            InputEvent::Focus(_) => return None,
        };
        (!bytes.is_empty()).then_some(Action::Send(bytes))
    }

    /// The text an input method is composing, for the Renderer to show at the cursor. Empty
    /// when nothing is being composed.
    pub fn composing(&self) -> &str {
        &self.composing
    }
}

impl Default for InputHandler {
    fn default() -> Self {
        Self::new()
    }
}
