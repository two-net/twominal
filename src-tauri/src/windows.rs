//! Windows: creation, focus tracking and per-window bootstrap data.
//!
//! Each window is one webview with its own tab bar; native window tabbing stays off. Tabs are
//! owned by the window's frontend, sessions by `Sessions`.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::Mutex;

use serde::Serialize;
use tauri::{AppHandle, Manager, State, WebviewUrl, WebviewWindow, WebviewWindowBuilder};

use crate::keymap::{self, Binding};
use crate::session::Sessions;
use crate::settings::{Settings, SettingsStore};
use crate::state::StateStore;

#[derive(Default)]
pub struct Windows {
    next: AtomicU32,
    /// The window focused last. Kept after it loses focus to a panel, like macOS's main window.
    focused: Mutex<Option<String>>,
    /// Where each new window's first tab starts.
    initial_cwd: Mutex<HashMap<String, PathBuf>>,
}

impl Windows {
    pub fn focused(&self) -> Option<String> {
        self.focused.lock().unwrap().clone()
    }

    pub fn set_focused(&self, label: &str) {
        *self.focused.lock().unwrap() = Some(label.to_owned());
    }

    pub fn forget(&self, label: &str) {
        let mut focused = self.focused.lock().unwrap();
        if focused.as_deref() == Some(label) {
            *focused = None;
        }
        self.initial_cwd.lock().unwrap().remove(label);
    }
}

/// Opens a window whose first tab starts in `cwd`, or in the home directory.
pub fn open(app: &AppHandle, cwd: Option<PathBuf>) -> tauri::Result<WebviewWindow> {
    let windows = app.state::<Windows>();
    let label = format!("win-{}", windows.next.fetch_add(1, Ordering::Relaxed) + 1);
    if let Some(cwd) = cwd {
        windows
            .initial_cwd
            .lock()
            .unwrap()
            .insert(label.clone(), cwd);
    }
    let window = WebviewWindowBuilder::new(app, &label, WebviewUrl::App("index.html".into()))
        .title(&app.package_info().name)
        .inner_size(800.0, 600.0)
        .min_inner_size(360.0, 200.0)
        .build()?;
    // A new window becomes key; its focus event can arrive before it is registered.
    windows.set_focused(&label);
    Ok(window)
}

/// The window that menu actions go to: the focused one, or the one focused last while
/// something else, such as the About panel, has focus.
pub fn menu_target(app: &AppHandle) -> Option<String> {
    let open = app.webview_windows();
    open.iter()
        .find(|(_, window)| window.is_focused().unwrap_or(false))
        .map(|(label, _)| label.clone())
        .or_else(|| {
            app.state::<Windows>()
                .focused()
                .filter(|label| open.contains_key(label))
        })
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WindowInit {
    initial_cwd: Option<PathBuf>,
    settings: Settings,
    keymap: Vec<Binding>,
    tour_pending: bool,
    debug: bool,
}

/// Everything a window's frontend needs to start.
#[tauri::command]
pub fn window_init(
    window: WebviewWindow,
    windows: State<'_, Windows>,
    settings: State<'_, SettingsStore>,
    state: State<'_, StateStore>,
) -> WindowInit {
    let settings = settings.get();
    WindowInit {
        initial_cwd: windows.initial_cwd.lock().unwrap().remove(window.label()),
        keymap: keymap::bindings(&settings.keymap),
        settings,
        tour_pending: state.take_tour_pending(),
        debug: cfg!(debug_assertions),
    }
}

/// Opens a new window starting in the directory of session `cwd_from`.
#[tauri::command]
pub async fn window_open(
    app: AppHandle,
    sessions: State<'_, Sessions>,
    cwd_from: Option<u64>,
) -> Result<(), String> {
    let cwd = cwd_from.and_then(|id| sessions.cwd(id));
    open(&app, cwd).map(drop).map_err(|err| err.to_string())
}
