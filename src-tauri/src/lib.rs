mod commands;
mod completion;
mod config;
mod error;
mod history;
mod logging;
mod pty;
mod shell;
mod storage;
mod terminal;

use shell::ShellIntegrationPaths;
use tauri::path::BaseDirectory;
use tauri::webview::PageLoadEvent;
use tauri::{AppHandle, Manager, RunEvent, WindowEvent};
use terminal::TerminalSessionManager;

struct ApplicationCleanupGuard {
    app_handle: AppHandle,
}

impl ApplicationCleanupGuard {
    fn new(app_handle: AppHandle) -> Self {
        Self { app_handle }
    }
}

impl Drop for ApplicationCleanupGuard {
    fn drop(&mut self) {
        self.app_handle
            .state::<TerminalSessionManager>()
            .close_all();
        logging::info("application_stopped");
        logging::flush();
    }
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let app = tauri::Builder::default()
        .setup(|app| {
            if let Ok(directory) = app.path().app_log_dir() {
                let _ = logging::initialize(&directory);
            }
            logging::info("application_started");

            let integration = app
                .path()
                .resolve("shell-integration", BaseDirectory::Resource)
                .ok()
                .map(ShellIntegrationPaths::new);
            app.manage(TerminalSessionManager::new(integration));
            app.manage(history::HistoryStore::default());
            install_panic_hook();
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            config::config_load,
            config::config_save,
            history::history_load,
            history::history_append,
            history::history_clear,
            completion::completion_query,
            commands::terminal_start,
            commands::terminal_prepare_transfer,
            commands::terminal_cancel_transfer,
            commands::terminal_attach_transfer,
            commands::terminal_write,
            commands::terminal_resize,
            commands::terminal_ack_output,
            commands::terminal_close,
        ])
        .on_page_load(|webview, payload| {
            if payload.event() == PageLoadEvent::Started {
                logging::info("webview_navigation_started");
                webview
                    .state::<TerminalSessionManager>()
                    .close_owner(webview.label());
            }
        })
        .on_window_event(|window, event| {
            if matches!(event, WindowEvent::Destroyed) {
                logging::info("application_window_destroyed");
                window
                    .state::<TerminalSessionManager>()
                    .close_owner(window.label());
            }
        })
        .build(tauri::generate_context!())
        .expect("error while building Twominal");

    let _cleanup_guard = ApplicationCleanupGuard::new(app.handle().clone());
    app.run(|app_handle, event| {
        if matches!(event, RunEvent::Exit) {
            app_handle.state::<TerminalSessionManager>().close_all();
            logging::info("application_exit_requested");
            logging::flush();
        }
    });
}

fn install_panic_hook() {
    let previous_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |panic_info| {
        // The cleanup guard runs after stack unwinding, when application locks are
        // no longer held. The hook itself remains lock-safe and records no payload.
        logging::error("application_panicked", "unexpected_panic");
        logging::flush();
        previous_hook(panic_info);
    }));
}
