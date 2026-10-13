//! App Shell (SRS §6.1).
//!
//! Starts and stops the app; owns windows, menus and the splash screen.
//!
//! Must not contain terminal logic.
//!
//! This is the only Rust code that names `tauri`. Every command the webview can call and
//! every event sent to it passes through here, so no other component depends on the
//! windowing framework (ARC-02). It is also the one place where the stores and managers are
//! wired together: they never name each other. Its page is `src/app.js`.
//!
//! Windows are opened here, in code, and each has a Tab Manager of its own. The menus are in
//! `app/menu.rs`. DESIGN.md §3 says how windows and menus behave on each OS.
//!
//! The product's name is read from Tauri, which has it from `tauri.conf.json` (GEN-01).
//! DESIGN.md §3 says where the name, the identifiers and the logo are kept.
//!
//! Filled in by #12 (launcher) and #41 (splash).

mod menu;
#[cfg(test)]
mod tests;

use std::fmt::Display;
use std::sync::{Mutex, MutexGuard, PoisonError};
use std::thread;

use tauri::{Emitter, Manager};

use crate::history::HistoryStore;
use crate::settings::{Change, Notice, SettingsStore, Theme};
use crate::tabs::TabManager;
use crate::theme::ThemeManager;

use menu::{Action, Menus};

/// Tells a page what is wrong with the configuration now: every notice, as text. An empty
/// list means nothing is wrong.
const SETTINGS_NOTICES: &str = "settings:notices";

/// What every window's label starts with. `capabilities/default.json` names the windows by
/// it, and a window it does not name cannot talk to Rust.
const LABEL: &str = "window-";

/// How big a window opens, in logical pixels.
const SIZE: (f64, f64) = (800.0, 600.0);

/// How far down and right of the focused window a new one opens, in logical pixels.
const STEP: f64 = 28.0;

/// The running app: the stores and managers, the menus, and every open window.
struct App {
    settings: SettingsStore,
    #[expect(dead_code, reason = "read by the command-line editor (CMD-12)")]
    history: HistoryStore,
    #[expect(dead_code, reason = "read by #38")]
    theme: ThemeManager,
    menus: Menus,
    windows: Mutex<Windows>,
}

impl App {
    fn windows(&self) -> MutexGuard<'_, Windows> {
        // Every change to the list is one step, so a panic under the lock leaves it whole.
        self.windows.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

/// The open windows, and how many were ever opened.
#[derive(Default)]
struct Windows {
    opened: u64,
    open: Vec<Window>,
}

impl Windows {
    /// A label for the next window. Never one that was used before.
    fn next_label(&mut self) -> String {
        self.opened += 1;
        format!("{LABEL}{}", self.opened)
    }
}

/// One window and its tabs. Tabs never move between windows in 1.0.
struct Window {
    label: String,
    tabs: TabManager,
}

impl Window {
    /// What the title bar shows.
    fn title(&self, product: &str) -> String {
        window_title(self.tabs.active_title(), product)
    }
}

/// A window's title: its active tab's (GEN-04), or the product's name while no tab has one.
fn window_title(tab: Option<&str>, product: &str) -> String {
    match tab {
        Some(title) if !title.trim().is_empty() => title.to_string(),
        _ => product.to_string(),
    }
}

/// The OS appearance a window is given for a Theme setting. `None` follows the OS. Which
/// palette that means is the Theme Manager's to say (#38).
fn appearance(theme: Theme) -> Option<tauri::Theme> {
    match theme {
        Theme::Dark => Some(tauri::Theme::Dark),
        Theme::Light => Some(tauri::Theme::Light),
        Theme::System => None,
    }
}

/// Opens a window, with a Tab Manager of its own (TAB-15).
fn open_window(handle: &tauri::AppHandle) -> tauri::Result<()> {
    let app = handle.state::<App>();
    let window = Window {
        label: app.windows().next_label(),
        tabs: TabManager::new(),
    };
    let page = tauri::WebviewUrl::App("index.html".into());
    let mut builder = tauri::WebviewWindowBuilder::new(handle, &window.label, page)
        .title(window.title(&handle.package_info().name))
        .inner_size(SIZE.0, SIZE.1)
        .theme(appearance(app.settings.settings().theme));
    if let Some((x, y)) = cascade(handle) {
        builder = builder.position(x, y);
    }
    // The list is not locked while the window is built: building it sends events.
    builder.build()?;
    app.windows().open.push(window);
    Ok(())
}

/// Where a new window goes on macOS: a step down and right of the focused one, so that it
/// does not hide it. Elsewhere, and with no window focused, the OS places it.
fn cascade(handle: &tauri::AppHandle) -> Option<(f64, f64)> {
    if !cfg!(target_os = "macos") {
        return None;
    }
    let focused = focused(handle)?;
    let scale = focused.scale_factor().ok()?;
    let at = focused.outer_position().ok()?.to_logical::<f64>(scale);
    Some((at.x + STEP, at.y + STEP))
}

/// The window the user is working in, if one of Twominal's is in front.
fn focused(handle: &tauri::AppHandle) -> Option<tauri::WebviewWindow> {
    handle
        .webview_windows()
        .into_values()
        .find(|window| window.is_focused().unwrap_or(false))
}

/// A window is gone, and its tabs with it.
fn closed(handle: &tauri::AppHandle, label: &str) {
    let app = handle.state::<App>();
    app.windows().open.retain(|window| window.label != label);
}

/// The Dock icon was clicked. With no window open, that opens one.
#[cfg(target_os = "macos")]
fn reopened(handle: &tauri::AppHandle) {
    if handle.state::<App>().windows().open.is_empty() {
        report(open_window(handle));
    }
}

/// One of Twominal's own menu items was chosen.
fn menu_chosen(handle: &tauri::AppHandle, event: tauri::menu::MenuEvent) {
    let Some(action) = menu::action(event.id().as_ref()) else {
        return;
    };
    match action {
        Action::NewWindow => {
            // From a thread of its own: on Windows a webview cannot be created from inside
            // an event handler.
            let handle = handle.clone();
            let opening = thread::Builder::new()
                .name("window".to_string())
                .spawn(move || report(open_window(&handle)));
            report(opening.map(drop));
        }
        Action::CloseWindow => {
            if let Some(window) = focused(handle) {
                report(window.close());
            }
        }
        Action::Quit => handle.exit(0),
        Action::Theme(theme) => {
            let app = handle.state::<App>();
            // The store saves the theme and announces it, and `settings_changed` shows it.
            if let Err(notices) = app.settings.update(|settings| settings.theme = theme) {
                log(&notices);
            }
            // The OS checks or unchecks an item when it is chosen, even if nothing changed.
            report(app.menus.show_theme(app.settings.settings().theme));
        }
    }
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
/// change is passed on from: to the windows and the menus here, to the Theme Manager with
/// #38, to the Tab Managers with #31, and to the pages, which so far show only the notices
/// (SET-05).
fn settings_changed(handle: &tauri::AppHandle, change: &Change) {
    log(&change.notices);
    report(handle.emit(SETTINGS_NOTICES, texts(&change.notices)));
    if change.settings.theme != change.previous.theme {
        handle.set_theme(appearance(change.settings.theme));
        // The file can change before the app has finished starting.
        if let Some(app) = handle.try_state::<App>() {
            report(app.menus.show_theme(change.settings.theme));
        }
    }
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

/// What could not be done for one window or page is no reason to stop.
fn report<E: Display>(done: Result<(), E>) {
    if let Err(error) = done {
        eprintln!("twominal: {error}");
    }
}

pub fn run() {
    tauri::Builder::default()
        // The menus are Twominal's own from the start.
        .enable_macos_default_menu(false)
        .setup(|app| {
            let handle = app.handle().clone();
            let settings = SettingsStore::open(app.path().config_dir().ok(), {
                let handle = handle.clone();
                move |change| settings_changed(&handle, change)
            });
            if cfg!(debug_assertions) {
                if let Some(path) = settings.path() {
                    eprintln!("twominal: settings in {}", path.display());
                }
                log(&settings.notices());
            }
            let theme = settings.settings().theme;
            let (menu, menus) = Menus::build(&handle, &app.package_info().name, theme)?;
            app.manage(App {
                settings,
                history: HistoryStore,
                theme: ThemeManager,
                menus,
                windows: Mutex::default(),
            });
            handle.set_menu(menu)?;
            handle.set_theme(appearance(theme));
            open_window(&handle)?;
            Ok(())
        })
        .on_menu_event(menu_chosen)
        .invoke_handler(tauri::generate_handler![app_ready])
        .build(tauri::generate_context!())
        .expect("Twominal could not start")
        .run(|handle, event| match event {
            tauri::RunEvent::WindowEvent {
                label,
                event: tauri::WindowEvent::Destroyed,
                ..
            } => closed(handle, &label),
            // On macOS an app with no window keeps running. Elsewhere closing the last
            // window quits (TAB-07). Quitting on purpose comes with a code and goes ahead.
            tauri::RunEvent::ExitRequested {
                code: None, api, ..
            } if cfg!(target_os = "macos") => api.prevent_exit(),
            #[cfg(target_os = "macos")]
            tauri::RunEvent::Reopen { .. } => reopened(handle),
            _ => {}
        });
}
