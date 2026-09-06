mod config;
mod host;
mod pty;

use config::{config_get, config_set};
use host::{
    complete_paths, default_shell, git_branch, history_load, history_save, list_commands,
    list_shells,
};
use pty::{pty_kill, pty_park, pty_resize, pty_spawn, pty_unpark, pty_write, PtyHub};
use tauri::{Manager, PhysicalSize, WebviewWindow};

/// Matches `minWidth` / `minHeight` in tauri.conf.json (logical pixels).
const MIN_WINDOW_WIDTH: f64 = 720.0;
const MIN_WINDOW_HEIGHT: f64 = 420.0;

/// Inner size is half the monitor resolution on each axis (¼ of the display).
fn apply_default_window_size(window: &WebviewWindow) {
    if let Some(monitor) = window
        .current_monitor()
        .ok()
        .flatten()
        .or_else(|| window.primary_monitor().ok().flatten())
    {
        let scale = monitor.scale_factor().max(1.0);
        let res = monitor.size();
        let width = (res.width / 2).max((MIN_WINDOW_WIDTH * scale).round() as u32);
        let height = (res.height / 2).max((MIN_WINDOW_HEIGHT * scale).round() as u32);
        let _ = window.set_size(PhysicalSize::new(width, height));
        let _ = window.center();
    }
    let _ = window.show();
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .manage(PtyHub::default())
        .invoke_handler(tauri::generate_handler![
            pty_spawn,
            pty_write,
            pty_resize,
            pty_kill,
            pty_park,
            pty_unpark,
            config_get,
            config_set,
            history_load,
            history_save,
            complete_paths,
            list_commands,
            git_branch,
            default_shell,
            list_shells,
        ])
        .setup(|app| {
            if let Some(window) = app.get_webview_window("main") {
                apply_default_window_size(&window);
            }
            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
