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
//! The Font and Font size settings become the font every page draws with in `app/fonts.rs`,
//! which also finds out whether the font is installed (FONT-02, SET-05).
//!
//! The product's name is read from Tauri, which has it from `tauri.conf.json` (GEN-01).
//! DESIGN.md §3 says where the name, the identifiers and the logo are kept.
//!
//! The `twominal` command is this executable. Its arguments are read in `app/launcher.rs`,
//! before Tauri starts, and `app/socket.rs` hands a command to the Twominal that is already
//! running (GEN-05).
//!
//! `app/clipboard.rs` is the OS clipboard, and on Linux the primary selection, which each tab is
//! given (SW-05). `app/links.rs` opens links in the OS's default handler for their kind, and
//! shows a file that would run in the file manager instead, for each tab too (SW-07, SEC-07).
//!
//! Filled in by #41 (splash).

mod clipboard;
mod fonts;
mod launcher;
mod links;
mod menu;
#[cfg(unix)]
mod socket;
#[cfg(test)]
mod tests;

use std::env;
use std::fmt::Display;
use std::path::PathBuf;
use std::process::ExitCode;
use std::sync::{Mutex, MutexGuard, PoisonError};
use std::thread;

use tauri::{Emitter, Manager};

use crate::history::HistoryStore;
use crate::settings::{Change, Notice, Settings, SettingsStore, Theme};
use crate::tabs::TabManager;
use crate::theme::ThemeManager;

use launcher::{Command, Start};
use menu::{Action, Menus};

/// Tells a page what is wrong with the configuration now: every notice, as text. An empty
/// list means nothing is wrong.
const SETTINGS_NOTICES: &str = "settings:notices";

/// Tells a page the font to draw with: a CSS family list, and its size in CSS pixels
/// (FONT-02).
const SETTINGS_FONT: &str = "settings:font";

/// Whether the Font size setting is in macOS's points.
const MACOS: bool = cfg!(target_os = "macos");

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
    /// That the Font setting names no installed font, once a scan has found that out. Held
    /// while the notices are sent, so that no list overtakes a newer one.
    font_missing: Mutex<Option<Notice>>,
}

impl App {
    fn windows(&self) -> MutexGuard<'_, Windows> {
        // Every change to the list is one step, so a panic under the lock leaves it whole.
        self.windows.lock().unwrap_or_else(PoisonError::into_inner)
    }

    fn font_missing(&self) -> MutexGuard<'_, Option<Notice>> {
        // Replaced in one step, so a panic under the lock leaves it whole.
        self.font_missing
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
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

/// Opens a window, with a Tab Manager of its own (TAB-15). Its first tab starts in
/// `directory`, or with none in the home directory (FISH-01).
fn open_window(
    handle: &tauri::AppHandle,
    directory: Option<PathBuf>,
) -> tauri::Result<tauri::WebviewWindow> {
    let app = handle.state::<App>();
    let label = app.windows().next_label();
    if cfg!(debug_assertions) {
        match &directory {
            Some(directory) => eprintln!("twominal: window {label} in {}", directory.display()),
            None => eprintln!("twominal: window {label} in the home directory"),
        }
    }
    let window = Window {
        label,
        tabs: TabManager::new(directory),
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
    let built = builder.build()?;
    app.windows().open.push(window);
    Ok(built)
}

/// Where a new window goes on macOS: a step down and right of the focused one, so that it
/// does not hide it. When Twominal is not in front, as when the launcher asks, the step is
/// from the newest window. Elsewhere, and with no window open, the OS places it.
fn cascade(handle: &tauri::AppHandle) -> Option<(f64, f64)> {
    if !cfg!(target_os = "macos") {
        return None;
    }
    let from = focused(handle).or_else(|| newest(handle))?;
    let scale = from.scale_factor().ok()?;
    let at = from.outer_position().ok()?.to_logical::<f64>(scale);
    Some((at.x + STEP, at.y + STEP))
}

/// The window the user is working in, if one of Twominal's is in front.
fn focused(handle: &tauri::AppHandle) -> Option<tauri::WebviewWindow> {
    handle
        .webview_windows()
        .into_values()
        .find(|window| window.is_focused().unwrap_or(false))
}

/// The window that was opened last and is still open.
fn newest(handle: &tauri::AppHandle) -> Option<tauri::WebviewWindow> {
    let label = handle.state::<App>().windows().open.last()?.label.clone();
    handle.get_webview_window(&label)
}

/// The launcher asked for a window, from a command in another process (GEN-05).
fn asked(handle: &tauri::AppHandle, directory: Option<PathBuf>) {
    // Building a window puts it before Twominal's others. Twominal itself comes to the front
    // only when the window is given the focus.
    report(open_window(handle, directory).and_then(|window| window.set_focus()));
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
        report(open_window(handle, None).map(drop));
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
                .spawn(move || report(open_window(&handle, None).map(drop)));
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
        Action::FontSize(zoom) => {
            let app = handle.state::<App>();
            // The store saves the size and announces it, and `settings_changed` sends it to
            // the pages. At the smallest or largest size nothing changes.
            let zoomed = |settings: &mut Settings| {
                settings.font_size = fonts::zoomed(settings.font_size, zoom);
            };
            if let Err(notices) = app.settings.update(zoomed) {
                log(&notices);
            }
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
    let font_missing = app.font_missing();
    let notices = texts(&app.settings.notices(), font_missing.as_ref());
    report(window.emit_to(window.label(), SETTINGS_NOTICES, notices));
    let font = fonts::font(&app.settings.settings(), MACOS);
    report(window.emit_to(window.label(), SETTINGS_FONT, font));
}

/// A setting changed, in Twominal or in the configuration file. This is the one place a
/// change is passed on from: to the windows and the menus here, to the Theme Manager with
/// #38, to the Tab Managers with #31, and to the pages, which so far are given the notices
/// (SET-05) and the font (FONT-02).
fn settings_changed(handle: &tauri::AppHandle, change: &Change) {
    log(&change.notices);
    // The file can change before the app has finished starting. Then no font has been
    // looked for yet.
    match handle.try_state::<App>() {
        Some(app) => {
            let mut font_missing = app.font_missing();
            if change.settings.font != change.previous.font {
                // The notice was about the font before.
                *font_missing = None;
                look_for_font(handle, &change.settings.font);
            }
            let notices = texts(&change.notices, font_missing.as_ref());
            report(handle.emit(SETTINGS_NOTICES, notices));
        }
        None => report(handle.emit(SETTINGS_NOTICES, texts(&change.notices, None))),
    }
    let (now, before) = (&change.settings, &change.previous);
    if now.font != before.font || now.font_size != before.font_size {
        report(handle.emit(SETTINGS_FONT, fonts::font(now, MACOS)));
    }
    if change.settings.theme != change.previous.theme {
        handle.set_theme(appearance(change.settings.theme));
        // The file can change before the app has finished starting.
        if let Some(app) = handle.try_state::<App>() {
            report(app.menus.show_theme(change.settings.theme));
        }
    }
}

/// Finds out on a thread of its own whether `font` is installed, and then shows the notice
/// that it is not, or takes it away (SET-05). The bundled font needs no looking for.
fn look_for_font(handle: &tauri::AppHandle, font: &str) {
    if fonts::is_bundled(font) {
        return;
    }
    let handle = handle.clone();
    let font = font.to_string();
    let looking = thread::Builder::new()
        .name("fonts".to_string())
        .spawn(move || {
            let notice = (!fonts::installed(&font)).then(|| Notice::font_missing(&font));
            let app = handle.state::<App>();
            let mut font_missing = app.font_missing();
            // A font chosen since then is looked for by a thread of its own.
            if app.settings.settings().font != font || *font_missing == notice {
                return;
            }
            log(notice.as_slice());
            *font_missing = notice;
            let notices = texts(&app.settings.notices(), font_missing.as_ref());
            report(handle.emit(SETTINGS_NOTICES, notices));
        });
    report(looking.map(drop));
}

/// Every notice about the configuration, as text: the store's, and that the font is not
/// installed.
fn texts(notices: &[Notice], font_missing: Option<&Notice>) -> Vec<String> {
    notices
        .iter()
        .chain(font_missing)
        .map(Notice::to_string)
        .collect()
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

pub fn run() -> ExitCode {
    let context = tauri::generate_context!();

    // The launcher comes first. What it can answer by itself needs no Tauri, and a command
    // that a running Twominal takes never becomes a second app.
    let package = context.package_info();
    let version = package.version.to_string();
    let directory = match launcher::read(env::args_os().skip(1)) {
        Ok(Command::Open(directory)) => directory,
        Ok(Command::Help) => {
            println!("{}", launcher::help(&package.name, &version));
            return ExitCode::SUCCESS;
        }
        Ok(Command::Version) => {
            println!("{}", launcher::version(&package.name, &version));
            return ExitCode::SUCCESS;
        }
        Err(problem) => {
            eprintln!("{}", problem.message());
            return ExitCode::from(problem.status());
        }
    };
    let claim = match launcher::start(directory.as_deref()) {
        Start::HandedOver => return ExitCode::SUCCESS,
        Start::First(claim) => Some(claim),
        Start::Alone => None,
    };

    tauri::Builder::default()
        // The menus are Twominal's own from the start.
        .enable_macos_default_menu(false)
        .setup(move |app| {
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
            let font = settings.settings().font;
            let (menu, menus) = Menus::build(&handle, &app.package_info().name, theme)?;
            app.manage(App {
                settings,
                history: HistoryStore,
                theme: ThemeManager,
                menus,
                windows: Mutex::default(),
                font_missing: Mutex::default(),
            });
            look_for_font(&handle, &font);
            handle.set_menu(menu)?;
            handle.set_theme(appearance(theme));
            open_window(&handle, directory)?;
            // Last, when everything a window needs is there. Commands that came sooner have
            // waited at the socket.
            if let Some(claim) = claim {
                report(claim.serve(move |directory| asked(&handle, directory)));
            }
            Ok(())
        })
        .on_menu_event(menu_chosen)
        .invoke_handler(tauri::generate_handler![app_ready])
        .build(context)
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
    ExitCode::SUCCESS
}
