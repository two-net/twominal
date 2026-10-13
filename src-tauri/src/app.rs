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

use tauri::{Emitter, Manager};

use crate::history::HistoryStore;
use crate::settings::{Change, Notice, SettingsStore};
use crate::tabs::TabManager;
use crate::theme::ThemeManager;

/// Tells a page what is wrong with the configuration now: every notice, as text. An empty
/// list means nothing is wrong.
const SETTINGS_NOTICES: &str = "settings:notices";

/// The running app: the stores and managers, and every open window.
#[expect(
    dead_code,
    reason = "history, theme and windows are read by #10 and #38"
)]
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
fn app_ready(window: tauri::WebviewWindow, app: tauri::State<App>) {
    if cfg!(debug_assertions) {
        eprintln!("twominal: window {} ready", window.label());
    }
    // Whatever is wrong with the configuration was found before this page could listen.
    let notices = texts(&app.settings.notices());
    report(window.emit_to(window.label(), SETTINGS_NOTICES, notices));
}

/// A setting changed, in Twominal or in the configuration file. This is the one place a
/// change is passed on from: to the Theme Manager with #38, to the Tab Managers with #31,
/// and to the pages, which so far show only the notices (SET-05).
fn settings_changed(app: &tauri::AppHandle, change: &Change) {
    log(&change.notices);
    report(app.emit(SETTINGS_NOTICES, texts(&change.notices)));
}

fn texts(notices: &[Notice]) -> Vec<String> {
    notices.iter().map(Notice::to_string).collect()
}

fn log(notices: &[Notice]) {
    if cfg!(debug_assertions) {
        eprintln!("twominal: settings notices: {}", notices.len());
        for notice in notices {
            eprintln!("twominal:   {notice}");
        }
    }
}

/// A page that cannot be told something is no reason to stop.
fn report(sent: tauri::Result<()>) {
    if let Err(error) = sent {
        eprintln!("twominal: {error}");
    }
}

pub fn run() {
    tauri::Builder::default()
        .setup(|app| {
            let handle = app.handle().clone();
            let settings = SettingsStore::open(app.path().config_dir().ok(), move |change| {
                settings_changed(&handle, change)
            });
            if cfg!(debug_assertions) {
                if let Some(path) = settings.path() {
                    eprintln!("twominal: settings in {}", path.display());
                }
                log(&settings.notices());
            }
            app.manage(App {
                settings,
                history: HistoryStore,
                theme: ThemeManager,
                windows: Vec::new(),
            });
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![app_ready])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
