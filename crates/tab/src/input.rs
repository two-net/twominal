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
//! Filled in by #20 (keys and input methods), #21 (mouse), #43 (shortcuts) and #36–#37
//! (Scrollback Vim mode).

/// One tab's input state: which mode it is in and what the keys mean there.
pub struct InputHandler;

/// A key, input-method or mouse event as the webview captured it, before it means anything.
/// The wire format between the two halves of the Input Handler. No other code reads it.
pub enum InputEvent {}
