//! Settings: the single source of truth for options (ARC-06).
//!
//! Values live in memory with the Appendix C defaults. Reading and watching the settings file
//! (SET-02) comes with the Settings work; components already subscribe to `settings-changed`.

use std::collections::BTreeMap;
use std::sync::RwLock;

use serde::Serialize;
use tauri::{AppHandle, Emitter};

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Settings {
    /// Show the tab bar even with a single tab (UI-02).
    pub always_show_tab_bar: bool,
    /// Scrollback length in lines (TERM-07).
    pub scrollback_lines: usize,
    /// Shell to run instead of the login shell (SI-01).
    pub shell: Option<String>,
    /// Shortcut overrides: action id → accelerator, `""` to unbind (SET-05).
    pub keymap: BTreeMap<String, String>,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            always_show_tab_bar: false,
            scrollback_lines: 10_000,
            shell: None,
            keymap: BTreeMap::new(),
        }
    }
}

#[derive(Default)]
pub struct SettingsStore(RwLock<Settings>);

impl SettingsStore {
    pub fn get(&self) -> Settings {
        self.0.read().unwrap().clone()
    }

    /// Applies `change` and tells every window about the new settings.
    pub fn update(&self, app: &AppHandle, change: impl FnOnce(&mut Settings)) -> Settings {
        let settings = {
            let mut settings = self.0.write().unwrap();
            change(&mut settings);
            settings.clone()
        };
        let _ = app.emit("settings-changed", &settings);
        settings
    }
}
