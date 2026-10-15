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
//! So far keys, pastes and focus are passthrough: `key.rs` reads what a key event means, on
//! any layout, and `encode.rs` turns it, a paste or a change of focus into the bytes a program
//! expects, by the modes the Terminal Core keeps (TERM-08, TERM-09, TERM-11, HW-01). Before that,
//! `shortcut.rs` takes Twominal's own keys: a page of scrolling and Clear (TERM-22). `mouse.rs`
//! decides whether the mouse and the wheel are the program's, reported as it asked or as cursor
//! keys, or Twominal's own, for selection, links and scrolling to act on (TERM-10, HW-02).
//! DESIGN.md §3 has the rules, and the contract the webview half keeps.
//!
//! Filled in by #20 (keys and input methods), #21 (mouse), #26 (scrolling and Clear), #43
//! (shortcuts) and #36–#37 (Scrollback Vim mode).

mod encode;
mod key;
mod mouse;
mod shortcut;
#[cfg(test)]
mod tests;

use terminal_core::{Modes, Size};

use mouse::Mouse;
use shortcut::Shortcut;

/// How many keys taken as shortcuts are remembered while they are down. Any string may come as
/// a key's `code`, so they are not kept without bound.
const TAKEN: usize = 8;

/// One tab's input state: which mode it is in and what the keys mean there.
pub struct InputHandler {
    /// Whether Option types characters, as on macOS (DESIGN.md §3).
    macos: bool,
    /// The text an input method is composing, shown at the cursor until it is committed.
    composing: String,
    mouse: Mouse,
    /// The keys taken as shortcuts that have not come up, by their `code`: their repeats are
    /// taken too, and their releases dropped, so a program never hears a release without its
    /// press.
    taken: Vec<String>,
}

/// A key, input-method or mouse event as the webview captured it, before it means anything.
/// The wire format between the two halves of the Input Handler. No other code reads it.
#[derive(Clone, Debug, PartialEq)]
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
    /// A mouse button went down or came up, or the mouse moved: `mousedown`, `mouseup` and
    /// `mousemove` on the terminal area.
    Mouse(MouseEvent),
    /// The wheel turned, or a trackpad scrolled: `wheel`.
    Wheel(WheelEvent),
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

/// A mouse event as the DOM gives it.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct MouseEvent {
    pub kind: MouseKind,
    /// The DOM's `button`: the button that went down or came up. 0 is the main one, 1 the
    /// middle, 2 the secondary, 3 back and 4 forward.
    pub button: i16,
    /// The DOM's `buttons`: the buttons held after the event. 1 is the main one, 2 the
    /// secondary, 4 the middle, 8 back and 16 forward.
    pub buttons: u16,
    /// The DOM's `detail`: the count of clicks in quick succession, 2 for a double-click.
    pub detail: u32,
    /// Where the pointer is, in CSS pixels from the top left of the first cell. While a button
    /// is held it may be outside the grid.
    pub x: f64,
    pub y: f64,
    pub modifiers: Modifiers,
}

/// Whether a mouse button went down or came up, or the mouse moved.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum MouseKind {
    #[default]
    Down,
    Up,
    Move,
}

/// A wheel event as the DOM gives it. Down and right are positive.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct WheelEvent {
    pub delta_x: f64,
    pub delta_y: f64,
    /// The DOM's `deltaMode`: what the deltas count.
    pub delta_mode: DeltaMode,
    /// Where the pointer is, as for a mouse event.
    pub x: f64,
    pub y: f64,
    pub modifiers: Modifiers,
}

/// What a wheel event's deltas count.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum DeltaMode {
    /// CSS pixels, which a trackpad gives in small steps: the DOM's 0.
    #[default]
    Pixel,
    /// Lines: the DOM's 1.
    Line,
    /// Pages: the DOM's 2.
    Page,
}

/// The terminal grid as the window shows it: its size in cells, which is the Terminal Core's,
/// and the size of a cell in CSS pixels.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Grid {
    pub size: Size,
    pub cell_width: f64,
    pub cell_height: f64,
}

/// What an input event means.
#[derive(Clone, Debug, PartialEq)]
pub enum Action {
    /// Bytes the user typed, to write to the session: a key going down or repeating, but a
    /// modifier or a lock on its own, committed text and a paste. The view goes back to the
    /// bottom for them (#26).
    Type(Vec<u8>),
    /// Other bytes to write to the session: a key coming up, or a modifier on its own, as the
    /// kitty keyboard protocol may report them, focus, and the mouse and the wheel.
    Send(Vec<u8>),
    /// The text being composed changed, so the Renderer must show it at the cursor (#22).
    Composing,
    /// The mouse did something the program is not told of, for Twominal to act on: select
    /// (TERM-17), open a link or show its target (TERM-20), open the context menu (UI-04) or
    /// paste the primary selection (SW-05).
    Pointer(Pointer),
    /// The wheel, the trackpad, or Shift+Page Up or Down scrolled the view by this many rows,
    /// toward newer output when positive. Fractional, so a trackpad moves it pixel by pixel
    /// (HW-02, ANIM-06).
    Scroll(f64),
    /// Clear: the screen and the scrollback, keeping the line with the cursor (TERM-22).
    Clear,
}

/// A mouse event that is Twominal's: where it fell on the grid, and with what.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Pointer {
    pub kind: PointerKind,
    /// The button that went down or came up, or the lowest held while dragging. None for a
    /// hover.
    pub button: Option<Button>,
    /// The cell, counted from the top left of the screen. Outside it while dragging past its
    /// edge, so a selection can scroll on.
    pub row: i32,
    pub col: i32,
    /// Whether the pointer is in the right half of the cell, where a selection takes the cell.
    pub right_half: bool,
    /// The count of clicks in quick succession of the press that started the gesture.
    pub clicks: u32,
    pub modifiers: Modifiers,
}

/// What the mouse did, of a gesture that is Twominal's.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PointerKind {
    Press,
    /// It moved with a button held.
    Drag,
    Release,
    /// It moved with no button held.
    Hover,
}

/// A mouse button, in the order xterm numbers them.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Button {
    Left,
    Middle,
    Right,
    Back,
    Forward,
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
            mouse: Mouse::default(),
            taken: Vec::new(),
        }
    }

    /// The grid the mouse falls on, given whenever the terminal area or the font changes it.
    /// Until there is one, the mouse and the wheel mean nothing. A cell that is not a size, of
    /// zero or of no number, is none.
    pub fn resize(&mut self, grid: Grid) {
        self.mouse.resize(grid);
    }

    /// What `event` means, with the modes the program set. `None` when it means nothing: a
    /// key the program cannot be told of, one it did not ask to hear about, or a move that
    /// stays in its cell. Nothing is waited for, so Esc is sent the moment it comes (TERM-08).
    pub fn handle(&mut self, event: InputEvent, modes: &Modes) -> Option<Action> {
        let bytes = match event {
            InputEvent::Key(event) => {
                let press = key::read(&event, self.macos)?;
                if let Some(action) = self.shortcut(&event, &press, modes) {
                    return action;
                }
                let bytes = encode::key(&press, modes);
                if bytes.is_empty() {
                    return None;
                }
                let typed = press.kind != Kind::Release && !press.is_modifier();
                return Some(if typed {
                    Action::Type(bytes)
                } else {
                    Action::Send(bytes)
                });
            }
            InputEvent::Text(text) => {
                self.composing.clear();
                return (!text.is_empty()).then(|| Action::Type(text.into_bytes()));
            }
            InputEvent::Compose(text) => {
                if text == self.composing {
                    return None;
                }
                self.composing = text;
                return Some(Action::Composing);
            }
            InputEvent::Paste(text) if text.is_empty() => return None,
            InputEvent::Paste(text) => {
                return Some(Action::Type(encode::paste(&text, modes.bracketed_paste)));
            }
            InputEvent::Focus(focused) if modes.focus => encode::focus(focused).to_vec(),
            InputEvent::Focus(_) => return None,
            InputEvent::Mouse(event) => return self.mouse.event(&event, modes),
            InputEvent::Wheel(event) => return self.mouse.wheel(&event, modes),
        };
        (!bytes.is_empty()).then_some(Action::Send(bytes))
    }

    /// What `press`, the key `event`, means if it is a shortcut or a key taken as one: the
    /// shortcut's action as it goes down or repeats, and nothing as it comes up. None when it is
    /// neither, and goes to the program. Only a key going down is taken. While it repeats it
    /// stays taken, whatever modifiers are held by then, and a key the program had stays the
    /// program's, so a program never hears a repeat or a release without the press.
    fn shortcut(
        &mut self,
        event: &KeyEvent,
        press: &key::Press,
        modes: &Modes,
    ) -> Option<Option<Action>> {
        let taken = self.taken.iter().position(|code| *code == event.code);
        match press.kind {
            Kind::Release => {
                self.taken.remove(taken?);
                return Some(None);
            }
            Kind::Repeat => {
                taken?;
                let shortcut = shortcut::find(press, modes, self.macos);
                return Some(shortcut.and_then(|shortcut| self.act(shortcut)));
            }
            Kind::Press => {}
        }
        let Some(shortcut) = shortcut::find(press, modes, self.macos) else {
            // Down again, as itself: its release, if it was lost, is the program's.
            if let Some(taken) = taken {
                self.taken.remove(taken);
            }
            return None;
        };
        if taken.is_none() {
            if self.taken.len() == TAKEN {
                self.taken.remove(0);
            }
            self.taken.push(event.code.clone());
        }
        Some(self.act(shortcut))
    }

    /// What `shortcut` does: scroll a page, of as many rows as the grid has, or Clear.
    fn act(&self, shortcut: Shortcut) -> Option<Action> {
        match shortcut {
            Shortcut::Page { up } => self
                .mouse
                .page()
                .map(|rows| Action::Scroll(if up { -rows } else { rows })),
            Shortcut::Clear => Some(Action::Clear),
        }
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
