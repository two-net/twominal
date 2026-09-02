mod commands;
mod env_fix;
mod fish;
mod pty;
mod theme;

use pty::PtyManager;
use std::sync::Arc;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    env_fix::init_environment();
    let pty_manager = Arc::new(PtyManager::new());


    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .manage(pty_manager)
        .invoke_handler(tauri::generate_handler![
            commands::shell_exec,
            commands::shell_write,
            commands::shell_cancel,
            commands::pty_spawn,
            commands::pty_write,
            commands::pty_resize,
            commands::pty_kill,
            commands::fish_check_command,
            commands::fish_get_completions,
            commands::fish_get_history,
            commands::fish_add_history,
            commands::get_solar_theme_info,
            commands::get_system_info,
            commands::get_git_branch,
            commands::window_minimize,
            commands::window_toggle_maximize,
            commands::window_close,
            commands::window_create_new,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
