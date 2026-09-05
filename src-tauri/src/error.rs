use serde::Serialize;
use std::fmt;

/// A stable, sanitized error returned across the Tauri IPC boundary.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CommandError {
    pub code: &'static str,
    pub message: String,
}

impl CommandError {
    pub fn new(code: &'static str, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
        }
    }

    pub fn invalid_size() -> Self {
        Self::new(
            "invalid_terminal_size",
            "Terminal rows and columns must be between 1 and 1000.",
        )
    }

    pub fn invalid_session_id() -> Self {
        Self::new("invalid_session_id", "The terminal session ID is invalid.")
    }

    pub fn session_not_found() -> Self {
        Self::new("session_not_found", "The terminal session does not exist.")
    }

    pub fn session_forbidden() -> Self {
        Self::new(
            "session_forbidden",
            "The terminal session belongs to another window.",
        )
    }

    pub fn session_not_running() -> Self {
        Self::new(
            "session_not_running",
            "The terminal session is no longer running.",
        )
    }
}

impl fmt::Display for CommandError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.message)
    }
}

impl std::error::Error for CommandError {}
