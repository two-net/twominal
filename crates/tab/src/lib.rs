//! The tab tier (SRS §6.1): everything one tab owns.
//!
//! A [`Tab`] owns exactly one Session, Terminal Core, Command Line, Input Handler and
//! Renderer (TAB-01) and shares nothing mutable with any other tab (ARC-04). It runs on its
//! own thread and needs no window. The layers above reach it only through its [`Handle`].
//!
//! This file is the one place where the tab components are wired together (§6.2). The
//! component modules never name each other (ARC-02): what they exchange passes through here.
//!
//! A tab knows nothing of the stores and managers above it. Settings and theme arrive as
//! messages, and the shared command history through a trait the Command Line defines.
//!
//! What the Terminal Core hears besides the screen is the Tab's to act on: replies go back to
//! the Session, questions about colors are answered with the Renderer's, and titles, the working
//! directory, the bell and the clipboard go up, as DESIGN.md §3 *What a program tells the tab*
//! says (TERM-12, TERM-13).
//!
//! So is what the Input Handler makes of the mouse and of Copy, Paste and Select All: the Tab
//! selects in the Terminal Core where the Renderer drew the row under the mouse, copies what is
//! selected to the [`OsClipboard`], and gives what it pastes back to the Input Handler (TERM-17,
//! SW-05, SEC-06), as DESIGN.md §3 *What the Tab does with the mouse and the clipboard* says.
//!
//! And links: before each frame the Tab gives the Renderer where the Input Handler says the mouse
//! is, so the link there is underlined and where it leads shown, and a click that opens one hands
//! the [`OsLinks`] the URL the Renderer showed there (TERM-20, SW-07, SEC-07), as DESIGN.md §3
//! *What the Tab does with links* says.
//!
//! Filled in by #16 (Terminal Core), #20 (Input Handler), #22 (Renderer), #28 (the clipboard), #30
//! (links) and #31 (tabs).

pub mod command_line;
pub mod input;
pub mod render;
pub mod session;

use std::sync::mpsc::Sender;

use command_line::CommandLine;
use input::InputHandler;
use render::Renderer;
use session::Session;
use terminal_core::TerminalCore;

pub use terminal_core::Selection;

/// Names a tab for as long as the app runs. Never reused.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct TabId(u64);

/// One tab: a shell session and everything needed to show it and type into it.
#[expect(
    dead_code,
    reason = "the components are wired together by #16, #20, #22 and #31"
)]
pub struct Tab {
    session: Session,
    core: TerminalCore,
    command_line: CommandLine,
    input: InputHandler,
    renderer: Renderer,
}

/// All that the layers above hold of a tab: its name and a way to tell it things. Holding
/// only this, the Tab Manager cannot reach into a tab's session or screen.
#[expect(dead_code, reason = "used by the Tab Manager in #31")]
pub struct Handle {
    id: TabId,
    messages: Sender<Message>,
}

/// What a tab can be told: input, output from its session, new settings or theme, that a
/// frame was drawn, to close. Each variant arrives with the feature that sends it.
pub enum Message {}

/// The OS's clipboard and, on Linux, its primary selection (SW-05): the first part of the Host
/// the App Shell gives each tab (#31). The user's copies and pastes (TERM-17) and what programs
/// ask of the clipboard (OSC 52, SEC-04) both go through it. Any tab's thread may call it.
pub trait OsClipboard: Send + Sync {
    /// Puts `text` on `which`. Off Linux there is no primary selection, and it takes nothing.
    fn write(&self, which: Selection, text: &str);

    /// The text `which` holds. None when it holds none, or no text, and off Linux for the
    /// primary selection.
    fn read(&self, which: Selection) -> Option<String>;
}

/// The OS's default handler for each kind of link (SW-07): the second part of the Host the App
/// Shell gives each tab (#31). Any tab's thread may call it.
pub trait OsLinks: Send + Sync {
    /// Opens `url` as the OS opens a link of its kind, and returns at once. `url` is one SEC-07
    /// lets open, as [`terminal_core::opening`] made it of a link the Renderer showed the user
    /// before the click: one word of printable ASCII.
    fn open(&self, url: &str);
}
