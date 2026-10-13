//! App Shell (SRS §6.1).
//!
//! Starts and stops the app; owns windows, menus and the splash screen.
//!
//! Must not contain terminal logic.
//!
//! This is the only Rust file that names `tauri`. Every command the webview can call and
//! every event sent to it passes through here, so no other component depends on the
//! windowing framework (ARC-02). It is also the one place where the stores and managers are
//! wired together: they never name each other. Its page is `src/app.js`.
//!
//! Filled in by #10 (windows and menus), #11 (identity), #12 (launcher) and #41 (splash).

use crate::history::HistoryStore;
use crate::settings::SettingsStore;
use crate::tabs::TabManager;
use crate::theme::ThemeManager;

/// The running app: the stores and managers, and every open window.
#[expect(dead_code, reason = "constructed at startup by #9 and #10")]
struct App {
    settings: SettingsStore,
    history: HistoryStore,
    theme: ThemeManager,
    windows: Vec<Window>,
}

/// One window and its tabs. Tabs never move between windows in 1.0.
#[expect(dead_code, reason = "constructed by #10")]
struct Window {
    label: String,
    tabs: TabManager,
}

/// The App Shell's page has loaded and can talk to Rust.
#[tauri::command]
fn app_ready(window: tauri::WebviewWindow) {
    if cfg!(debug_assertions) {
        eprintln!("twominal: window {} ready", window.label());
    }
}

pub fn run() {
    tauri::Builder::default()
        .invoke_handler(tauri::generate_handler![app_ready])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
