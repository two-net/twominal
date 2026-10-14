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
//! Filled in by #16 (Terminal Core), #20 (Input Handler), #22 (Renderer) and #31 (tabs).

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
