use tauri::ipc::{Channel, InvokeBody, InvokeResponseBody, Request};
use tauri::{State, Webview};
use uuid::Uuid;

use crate::error::CommandError;
use crate::pty::TerminalSize;
use crate::terminal::{LifecycleEvent, SessionDescriptor, TerminalSessionManager};

const SESSION_ID_HEADER: &str = "X-Twominal-Session-Id";
const MAX_INPUT_BYTES: usize = 64 * 1024;

#[tauri::command(async)]
pub fn terminal_start(
    webview: Webview,
    state: State<'_, TerminalSessionManager>,
    size: TerminalSize,
    output: Channel<InvokeResponseBody>,
    lifecycle: Channel<LifecycleEvent>,
) -> Result<SessionDescriptor, CommandError> {
    state.start(webview.label(), size, output, lifecycle)
}

#[tauri::command]
pub fn terminal_prepare_transfer(
    webview: Webview,
    state: State<'_, TerminalSessionManager>,
    session_id: String,
    target_window_label: String,
) -> Result<String, CommandError> {
    state.prepare_transfer(
        webview.label(),
        parse_session_id(&session_id)?,
        &target_window_label,
    )
}

#[tauri::command]
pub fn terminal_cancel_transfer(
    webview: Webview,
    state: State<'_, TerminalSessionManager>,
    transfer_token: String,
) -> Result<(), CommandError> {
    state.cancel_transfer(webview.label(), parse_session_id(&transfer_token)?)
}

#[tauri::command(async)]
pub fn terminal_attach_transfer(
    webview: Webview,
    state: State<'_, TerminalSessionManager>,
    transfer_token: String,
    size: TerminalSize,
    output: Channel<InvokeResponseBody>,
    lifecycle: Channel<LifecycleEvent>,
    snapshot: Channel<InvokeResponseBody>,
) -> Result<SessionDescriptor, CommandError> {
    state.attach_transfer(
        webview.label(),
        parse_session_id(&transfer_token)?,
        size,
        output,
        lifecycle,
        snapshot,
    )
}

#[tauri::command]
pub fn terminal_write(
    webview: Webview,
    state: State<'_, TerminalSessionManager>,
    request: Request<'_>,
) -> Result<(), CommandError> {
    let session_id = session_id_from_request(&request)?;
    let InvokeBody::Raw(bytes) = request.body() else {
        return Err(CommandError::new(
            "invalid_input_body",
            "Terminal input must be sent as raw bytes.",
        ));
    };
    if bytes.len() > MAX_INPUT_BYTES {
        return Err(CommandError::new(
            "input_too_large",
            "Terminal input must not exceed 65536 bytes per request.",
        ));
    }
    state.write(webview.label(), session_id, bytes.clone())
}

#[tauri::command(async)]
pub fn terminal_resize(
    webview: Webview,
    state: State<'_, TerminalSessionManager>,
    session_id: String,
    size: TerminalSize,
) -> Result<(), CommandError> {
    state.resize(webview.label(), parse_session_id(&session_id)?, size)
}

#[tauri::command]
pub fn terminal_ack_output(
    webview: Webview,
    state: State<'_, TerminalSessionManager>,
    session_id: String,
    bytes: usize,
) -> Result<(), CommandError> {
    state.acknowledge(webview.label(), parse_session_id(&session_id)?, bytes)
}

#[tauri::command(async)]
pub fn terminal_close(
    webview: Webview,
    state: State<'_, TerminalSessionManager>,
    session_id: String,
) -> Result<(), CommandError> {
    state.close(webview.label(), parse_session_id(&session_id)?)
}

fn session_id_from_request(request: &Request<'_>) -> Result<Uuid, CommandError> {
    let value = request
        .headers()
        .get(SESSION_ID_HEADER)
        .and_then(|value| value.to_str().ok())
        .ok_or_else(CommandError::invalid_session_id)?;
    parse_session_id(value)
}

fn parse_session_id(value: &str) -> Result<Uuid, CommandError> {
    Uuid::parse_str(value).map_err(|_| CommandError::invalid_session_id())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_only_valid_session_ids() {
        let id = Uuid::new_v4();
        assert_eq!(parse_session_id(&id.to_string()).unwrap(), id);
        assert_eq!(
            parse_session_id("not-a-session").unwrap_err().code,
            "invalid_session_id"
        );
    }
}
