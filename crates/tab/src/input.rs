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
//! `shortcut.rs` takes Twominal's own keys: a page of scrolling, Clear (TERM-22), Copy, Paste and
//! Select All, and Find, which opens the find bar: while it has the keys, `find.rs` edits its
//! query with them, and no key reaches the program (TERM-18). `mouse.rs` decides whether the
//! mouse and the wheel are the program's, reported as
//! it asked or as cursor keys, or Twominal's own, for selection, links and scrolling to act on
//! (TERM-10, HW-02). The left button of a gesture that is Twominal's selects, by characters,
//! words, lines or as a block (TERM-17), and on Linux the middle one pastes the primary
//! selection (SW-05). A paste of more than a line into a program that did not ask for bracketed
//! paste waits for the user to confirm it (SEC-06). DESIGN.md §3 has the rules, and the contract
//! the webview half keeps.
//!
//! Filled in by #20 (keys and input methods), #21 (mouse), #26 (scrolling and Clear), #28
//! (selection, copy and paste), #29 (the find bar), #43 (shortcuts) and #36–#37 (Scrollback Vim
//! mode).

mod encode;
mod find;
mod key;
mod mouse;
mod shortcut;
#[cfg(test)]
mod tests;

use terminal_core::{Modes, Search, Selecting, Size};

pub use find::FindBar;

use find::Edit;
use mouse::Mouse;
use shortcut::Shortcut;

/// How many keys taken as shortcuts are remembered while they are down. Any string may come as
/// a key's `code`, so they are not kept without bound.
const TAKEN: usize = 8;

/// One tab's input state: which mode it is in and what the keys mean there.
pub struct InputHandler {
    /// Whose keys and mouse it reads: on macOS Option types characters (DESIGN.md §3), and each
    /// OS has its own modifier for a block selection and its own Mod (Appendix A).
    os: Os,
    /// The text an input method is composing, shown at the cursor until it is committed.
    composing: String,
    mouse: Mouse,
    /// The keys taken as shortcuts that have not come up, by their `code`: their repeats are
    /// taken too, and their releases dropped, so a program never hears a release without its
    /// press.
    taken: Vec<String>,
    /// How the left button's press selects, for the drags and the release of its gesture.
    selecting: Option<Selecting>,
    /// Whether a paste of more than a line waits for the user to confirm it: the Multi-line
    /// paste warning setting (SEC-06).
    paste_warning: bool,
    /// The paste waiting for the user to confirm it.
    waiting: Option<String>,
    /// The find bar, open or not, with the query it keeps (TERM-18).
    find: FindBar,
}

/// The OS whose keys and mouse an Input Handler reads.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Os {
    MacOs,
    Linux,
    Windows,
}

impl Os {
    /// The OS Twominal was built for. A Unix that is not macOS counts as Linux.
    pub fn current() -> Self {
        if cfg!(target_os = "macos") {
            Self::MacOs
        } else if cfg!(windows) {
            Self::Windows
        } else {
            Self::Linux
        }
    }

    /// Whether Option+drag, Ctrl+drag or Alt+drag, as this OS selects a block (Appendix A),
    /// is what `modifiers` hold.
    fn blocks(self, modifiers: Modifiers) -> bool {
        match self {
            Self::MacOs | Self::Windows => modifiers.alt,
            Self::Linux => modifiers.ctrl,
        }
    }
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
    /// Text the user pasted: the webview's `paste`, which on macOS Edit ▸ Paste fires.
    Paste(String),
    /// The webview's `copy`, which on macOS Edit ▸ Copy fires (#31).
    Copy,
    /// The webview's Select All (#31).
    SelectAll,
    /// The window gained focus, or lost it.
    Focus(bool),
    /// A mouse button went down or came up, or the mouse moved: `mousedown`, `mouseup` and
    /// `mousemove` on the terminal area.
    Mouse(MouseEvent),
    /// The wheel turned, or a trackpad scrolled: `wheel`.
    Wheel(WheelEvent),
    /// The find bar's buttons, a click in its field, and the menus' Find item (#31).
    Find(FindEvent),
}

/// What the user did with the find bar, besides typing into it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FindEvent {
    /// Open the bar, or give it the keys with its query selected: Edit ▸ Find, as Mod+F does.
    Open,
    /// Give the bar the keys: a click in its field.
    Focus,
    /// Go to the match above the current one: its ↑ button.
    Up,
    /// Go to the match below the current one: its ↓ button.
    Down,
    /// Turn letter case counting on or off: its case button.
    Case,
    /// Turn reading the query as a regular expression on or off: its regex button.
    Regex,
    /// Close the bar: its × button.
    Close,
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
    /// The mouse did something the program is not told of and nothing here takes, for Twominal
    /// to act on: open a link or show its target (TERM-20), or open the context menu (UI-04).
    Pointer(Pointer),
    /// The left button selects (TERM-17). Its press starts a selection, by characters, words or
    /// lines as its clicks say or as a block with the OS's modifier, and its drags and its
    /// release move the selection's end. The Pointer says where, and its `kind` which it is.
    Select(Pointer, Selecting),
    /// Copy the selection to the clipboard (SW-05).
    Copy,
    /// Paste the clipboard (SW-05). The Tab reads it and gives it to [`InputHandler::paste`].
    Paste,
    /// Paste the primary selection: a middle-click on Linux (SW-05). The Tab reads it and gives
    /// it to [`InputHandler::paste`].
    PastePrimary,
    /// Select everything.
    SelectAll,
    /// A paste of `lines` lines waits for the user to confirm it, since the program did not ask
    /// for bracketed paste and its line breaks would run what is before them (SEC-06). The Tab
    /// asks, and gives the answer to [`InputHandler::confirm_paste`].
    ConfirmPaste { lines: usize },
    /// The wheel, the trackpad, or Shift+Page Up or Down scrolled the view by this many rows,
    /// toward newer output when positive. Fractional, so a trackpad moves it pixel by pixel
    /// (HW-02, ANIM-06).
    Scroll(f64),
    /// Clear: the screen and the scrollback, keeping the line with the cursor (TERM-22).
    Clear,
    /// The find bar looks for this now, or for nothing: its query or a toggle changed, it
    /// opened, or it closed (TERM-18). The Tab has the Terminal Core search, and the view show
    /// the current match.
    Find(Option<Search>),
    /// Go to the match above the current one, toward older output, or below it when not `up`.
    FindStep { up: bool },
    /// The find bar changed only how it looks: its caret, what is selected or composed in it,
    /// or whether it has the keys. The Renderer draws it.
    FindBar,
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
        Self::on(Os::current())
    }

    /// An Input Handler as on `os`.
    pub fn on(os: Os) -> Self {
        Self {
            os,
            composing: String::new(),
            mouse: Mouse::default(),
            taken: Vec::new(),
            selecting: None,
            paste_warning: true,
            waiting: None,
            find: FindBar::default(),
        }
    }

    /// The grid the mouse falls on, given whenever the terminal area or the font changes it.
    /// Until there is one, the mouse and the wheel mean nothing. A cell that is not a size, of
    /// zero or of no number, is none.
    pub fn resize(&mut self, grid: Grid) {
        self.mouse.resize(grid);
    }

    /// How far up the view's rows are drawn: the part of a row the view is scrolled past its top
    /// one, as the Renderer's last frame drew it (ANIM-06). A Pointer's row is then the row drawn
    /// under it. Reports to programs are of the grid's cells, which the view does not move.
    pub fn set_offset(&mut self, rows: f64) {
        self.mouse.set_offset(rows);
    }

    /// Whether a paste of more than a line into a program that did not ask for bracketed paste
    /// waits for the user to confirm it: the Multi-line paste warning setting (SEC-06). Until it
    /// is called it does, Appendix B's default.
    pub fn set_paste_warning(&mut self, on: bool) {
        self.paste_warning = on;
    }

    /// What `event` means, with the modes the program set. `None` when it means nothing: a
    /// key the program cannot be told of, one it did not ask to hear about, or a move that
    /// stays in its cell. Nothing is waited for, so Esc is sent the moment it comes (TERM-08).
    pub fn handle(&mut self, event: InputEvent, modes: &Modes) -> Option<Action> {
        let bytes = match event {
            InputEvent::Key(event) => {
                let press = key::read(&event, self.os == Os::MacOs)?;
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
            InputEvent::Text(text) if self.find.is_focused() => {
                let edit = self.find.insert(&text);
                return self.found(edit);
            }
            InputEvent::Compose(text) if self.find.is_focused() => {
                let edit = self.find.compose(text);
                return self.found(edit);
            }
            InputEvent::SelectAll if self.find.is_focused() => {
                let edit = self.find.select_all();
                return self.found(edit);
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
            InputEvent::Paste(text) => return self.paste(text, modes),
            InputEvent::Copy => return Some(Action::Copy),
            InputEvent::SelectAll => return Some(Action::SelectAll),
            InputEvent::Focus(focused) if modes.focus => encode::focus(focused).to_vec(),
            InputEvent::Focus(_) => return None,
            InputEvent::Mouse(event) => {
                // A press on the grid gives the keys back to the program, the bar staying open.
                let blurred = event.kind == MouseKind::Down && self.find.is_focused();
                if blurred {
                    self.find.blur();
                }
                return match self.mouse.event(&event, modes) {
                    Some(Action::Pointer(pointer)) => Some(self.point(pointer)),
                    Some(action) => Some(action),
                    None => blurred.then_some(Action::FindBar),
                };
            }
            InputEvent::Wheel(event) => return self.mouse.wheel(&event, modes),
            InputEvent::Find(event) => return self.find_event(event),
        };
        (!bytes.is_empty()).then_some(Action::Send(bytes))
    }

    /// What `press`, the key `event`, means if it is a shortcut, a key for the find bar while it
    /// has the keys, or a key taken as one of those: the action as it goes down or repeats, and
    /// nothing as it comes up. None when it goes to the program. Only a key going down is taken.
    /// While it repeats it stays taken, whatever modifiers are held by then, and a key the program
    /// had stays the program's, so a program never hears a repeat or a release without the
    /// press. While the bar has the keys, the repeats of a key the program had are dropped, and
    /// only its release goes on.
    fn shortcut(
        &mut self,
        event: &KeyEvent,
        press: &key::Press,
        modes: &Modes,
    ) -> Option<Option<Action>> {
        let taken = self.taken.iter().position(|code| *code == event.code);
        let macos = self.os == Os::MacOs;
        let bar = self.find.is_focused();
        match press.kind {
            Kind::Release => {
                self.taken.remove(taken?);
                return Some(None);
            }
            Kind::Repeat => {
                if taken.is_none() {
                    return bar.then_some(None);
                }
                return Some(match shortcut::read(press, modes, macos) {
                    Some(shortcut) => self.act(shortcut),
                    None if bar => self.edit(press),
                    None => None,
                });
            }
            Kind::Press => {}
        }
        let shortcut = shortcut::read(press, modes, macos);
        if shortcut.is_none() && !bar {
            // Down again, as itself: its release, if it was lost, is the program's.
            if let Some(taken) = taken {
                self.taken.remove(taken);
            }
            return None;
        }
        if taken.is_none() {
            if self.taken.len() == TAKEN {
                self.taken.remove(0);
            }
            self.taken.push(event.code.clone());
        }
        Some(match shortcut {
            Some(shortcut) => self.act(shortcut),
            None => self.edit(press),
        })
    }

    /// What `shortcut` does: scroll a page, of as many rows as the grid has, Clear, Copy, Paste,
    /// Select All, or Find. While the find bar has the keys Select All selects its query.
    fn act(&mut self, shortcut: Shortcut) -> Option<Action> {
        match shortcut {
            Shortcut::Page { up } => self
                .mouse
                .page()
                .map(|rows| Action::Scroll(if up { -rows } else { rows })),
            Shortcut::Clear => Some(Action::Clear),
            Shortcut::Copy => Some(Action::Copy),
            Shortcut::Paste => Some(Action::Paste),
            Shortcut::SelectAll if self.find.is_focused() => {
                let edit = self.find.select_all();
                self.found(edit)
            }
            Shortcut::SelectAll => Some(Action::SelectAll),
            Shortcut::Find => self.find_event(FindEvent::Open),
        }
    }

    /// What `press`, a key going down or repeating while the find bar has the keys, does.
    fn edit(&mut self, press: &key::Press) -> Option<Action> {
        let edit = self.find.key(press, self.os == Os::MacOs);
        self.found(edit)
    }

    /// What the user did with the find bar, besides typing into it. Giving the bar the keys ends
    /// the text the grid was composing.
    fn find_event(&mut self, event: FindEvent) -> Option<Action> {
        let open = self.find.is_open();
        let edit = match event {
            FindEvent::Open => {
                self.composing.clear();
                self.find.open()
            }
            FindEvent::Focus if open => {
                self.composing.clear();
                self.find.focus();
                Edit::Look
            }
            FindEvent::Up if open => Edit::Step { up: true },
            FindEvent::Down if open => Edit::Step { up: false },
            FindEvent::Case if open => self.find.toggle_case(),
            FindEvent::Regex if open => self.find.toggle_regex(),
            FindEvent::Close => self.find.close(),
            _ => Edit::Nothing,
        };
        self.found(edit)
    }

    /// The action an edit of the find bar is.
    fn found(&self, edit: Edit) -> Option<Action> {
        match edit {
            Edit::Search => Some(Action::Find(self.find.search())),
            Edit::Look => Some(Action::FindBar),
            Edit::Step { up } => Some(Action::FindStep { up }),
            Edit::Close => Some(Action::Find(None)),
            Edit::Nothing => None,
        }
    }

    /// The find bar: whether it is open and has the keys, its query, caret and toggles, for the
    /// Renderer to draw.
    pub fn find_bar(&self) -> &FindBar {
        &self.find
    }

    /// What `pointer`, of a gesture that is Twominal's, means. The left button selects: its
    /// press as its clicks say, 1, 2 and 3 and round again, or as a block with the OS's modifier,
    /// and its drags and release as the press did. On Linux a press of the middle button pastes
    /// the primary selection. The rest is for links and the context menu.
    fn point(&mut self, pointer: Pointer) -> Action {
        match (pointer.kind, pointer.button) {
            (PointerKind::Press, Some(Button::Left)) => {
                let by = if self.os.blocks(pointer.modifiers) {
                    Selecting::Block
                } else {
                    match pointer.clicks.saturating_sub(1) % 3 {
                        0 => Selecting::Characters,
                        1 => Selecting::Words,
                        _ => Selecting::Lines,
                    }
                };
                self.selecting = Some(by);
                Action::Select(pointer, by)
            }
            (PointerKind::Drag | PointerKind::Release, Some(Button::Left)) => {
                let by = match pointer.kind {
                    PointerKind::Release => self.selecting.take(),
                    _ => self.selecting,
                };
                match by {
                    Some(by) => Action::Select(pointer, by),
                    None => Action::Pointer(pointer),
                }
            }
            (PointerKind::Press, Some(Button::Middle)) if self.os == Os::Linux => {
                Action::PastePrimary
            }
            _ => Action::Pointer(pointer),
        }
    }

    /// What pasting `text` means: the text, bracketed if the program asked (TERM-11), or the
    /// question whether to paste it when it has a line break, the program did not ask, and the
    /// warning is on (SEC-06). A paste waiting for the answer gives way to this one. Text read
    /// from the clipboard for [`Action::Paste`] and [`Action::PastePrimary`] comes here too.
    pub fn paste(&mut self, text: String, modes: &Modes) -> Option<Action> {
        if text.is_empty() {
            return None;
        }
        // Into the find bar while it has the keys, where it runs nothing. The paste waiting stays.
        if self.find.is_focused() {
            let edit = self.find.insert(&text);
            return self.found(edit);
        }
        if self.paste_warning && !modes.bracketed_paste && text.contains(['\n', '\r']) {
            let lines = lines(&text);
            self.waiting = Some(text);
            return Some(Action::ConfirmPaste { lines });
        }
        self.waiting = None;
        Some(Action::Type(encode::paste(&text, modes.bracketed_paste)))
    }

    /// The user's answer to [`Action::ConfirmPaste`]: the paste waiting, encoded by the modes as
    /// they are now, or nothing when the user said no or nothing waits.
    pub fn confirm_paste(&mut self, paste: bool, modes: &Modes) -> Option<Action> {
        let text = self.waiting.take().filter(|_| paste)?;
        Some(Action::Type(encode::paste(&text, modes.bracketed_paste)))
    }

    /// The text an input method is composing, for the Renderer to show at the cursor. Empty
    /// when nothing is being composed there, as while the find bar has the keys: its own is
    /// the bar's.
    pub fn composing(&self) -> &str {
        if self.find.is_focused() {
            ""
        } else {
            &self.composing
        }
    }
}

/// How many lines `text` has, as an editor counts them: a line break at its end starts none.
fn lines(text: &str) -> usize {
    let breaks = text.replace("\r\n", "\n").matches(['\n', '\r']).count();
    let ends = text.ends_with(['\n', '\r']);
    breaks + 1 - usize::from(ends)
}

impl Default for InputHandler {
    fn default() -> Self {
        Self::new()
    }
}
