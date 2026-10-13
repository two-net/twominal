//! Command Line (SRS §6.1).
//!
//! Edits the command line at the prompt: highlighting, autosuggestions, completions,
//! abbreviations and Prompt Vim mode; hands the finished line to the Session.
//!
//! Must not take input in passthrough, and must not depend on fish. The fish-style
//! command-line features live here and nowhere else (ARC-05).
//!
//! It reaches the shared command history only through a trait defined here, which the
//! History Store implements (ARC-04). Slow work such as completions is asked of the Tab and
//! applied when it arrives, so a key never waits for it (CMD-16).
//!
//! Filled in by #35 (Prompt Vim mode) and by the command-line editor (CMD-01 to CMD-16),
//! which has no issue yet.

/// The command line being edited at one tab's prompt.
pub struct CommandLine;
