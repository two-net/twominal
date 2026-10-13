//! Session (SRS §6.1).
//!
//! Starts the shell on a pseudo-terminal with shell integration; carries bytes both ways;
//! reports exit.
//!
//! Must not interpret output: bytes go to the Terminal Core untouched, and the Tab does the
//! passing. Shell-specific behavior lives here, in the shell integration, and nowhere else
//! (ARC-05).
//!
//! Filled in by #13 (pseudo-terminal), #14 (other shells) and #15 (shell integration).

/// One shell process connected to one tab through a pseudo-terminal.
pub struct Session;
