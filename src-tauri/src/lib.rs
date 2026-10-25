mod keymap;
mod menu;
mod platform;
mod session;
mod settings;
mod state;
mod term_core;
mod title;
mod windows;

use tauri::webview::PageLoadEvent;
use tauri::{Manager, RunEvent, WindowEvent};

use session::Sessions;
use settings::SettingsStore;
use state::StateStore;
use windows::Windows;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .manage(Sessions::default())
        .manage(SettingsStore::default())
        .manage(Windows::default())
        .setup(|app| {
            let state_path = app.path().app_data_dir()?.join("state.json");
            app.manage(StateStore::load(state_path));
            menu::install(app.handle())?;
            windows::open(app.handle(), None)?;
            Ok(())
        })
        .on_menu_event(menu::handle)
        .on_window_event(|window, event| match event {
            WindowEvent::Focused(true) => window.state::<Windows>().set_focused(window.label()),
            WindowEvent::Destroyed => {
                window.state::<Sessions>().close_window(window.label());
                window.state::<Windows>().forget(window.label());
            }
            _ => {}
        })
        .on_page_load(|webview, payload| {
            // A reload starts the page over with no tabs, so its old shells would be orphaned.
            if payload.event() == PageLoadEvent::Started {
                webview.state::<Sessions>().close_window(webview.label());
            }
        })
        .invoke_handler(tauri::generate_handler![
            session::session_spawn,
            session::session_input,
            session::session_paste,
            session::session_resize,
            session::session_scroll,
            session::session_set_visible,
            session::session_close,
            state::tour_mark_seen,
            windows::window_init,
            windows::window_open,
        ])
        .build(tauri::generate_context!())
        .expect("error while building tauri application")
        .run(|app, event| match event {
            // Like other Mac apps, keep running when the last window closes.
            RunEvent::ExitRequested {
                code: None, api, ..
            } => api.prevent_exit(),
            #[cfg(target_os = "macos")]
            RunEvent::Reopen {
                has_visible_windows: false,
                ..
            } => {
                let _ = windows::open(app, None);
            }
            _ => {}
        });
}
