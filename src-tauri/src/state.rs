//! App state that is not a setting, such as whether the welcome tour was shown (UI-07).

use std::fs::{self, DirBuilder, OpenOptions};
use std::io::{self, Write};
use std::os::unix::fs::{DirBuilderExt, OpenOptionsExt};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;

use serde::{Deserialize, Serialize};
use tauri::State;

/// Bump when the tour's content changes enough to show it again.
pub const TOUR_VERSION: u32 = 1;

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct AppState {
    pub tour_version_seen: u32,
}

pub struct StateStore {
    path: PathBuf,
    state: Mutex<AppState>,
    /// The tour is offered to one window per launch only.
    tour_offered: AtomicBool,
}

impl StateStore {
    /// Loads the state file. A missing or unreadable file means a first launch.
    pub fn load(path: PathBuf) -> Self {
        let state = fs::read(&path)
            .ok()
            .and_then(|bytes| serde_json::from_slice(&bytes).ok())
            .unwrap_or_default();
        Self {
            path,
            state: Mutex::new(state),
            tour_offered: AtomicBool::new(false),
        }
    }

    /// Whether this window should show the tour: it has not been seen, and no other window of
    /// this launch has been offered it.
    pub fn take_tour_pending(&self) -> bool {
        self.state.lock().unwrap().tour_version_seen < TOUR_VERSION
            && !self.tour_offered.swap(true, Ordering::Relaxed)
    }

    pub fn mark_tour_seen(&self) -> io::Result<()> {
        let mut state = self.state.lock().unwrap();
        state.tour_version_seen = TOUR_VERSION;
        self.save(&state)
    }

    /// Writes atomically, readable by the owner only (SEC-02).
    fn save(&self, state: &AppState) -> io::Result<()> {
        if let Some(dir) = self.path.parent() {
            DirBuilder::new().recursive(true).mode(0o700).create(dir)?;
        }
        let tmp = self.path.with_extension("json.tmp");
        let mut file = OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .mode(0o600)
            .open(&tmp)?;
        file.write_all(&serde_json::to_vec_pretty(state)?)?;
        file.sync_all()?;
        fs::rename(&tmp, &self.path)
    }
}

#[tauri::command]
pub fn tour_mark_seen(state: State<'_, StateStore>) -> Result<(), String> {
    state.mark_tour_seen().map_err(|err| err.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;

    fn temp_path(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("twominal-test-{}-{name}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        dir.join("state.json")
    }

    #[test]
    fn tour_is_pending_once_until_seen() {
        let path = temp_path("tour");
        let store = StateStore::load(path.clone());
        assert!(store.take_tour_pending());
        assert!(!store.take_tour_pending(), "only one window per launch");

        store.mark_tour_seen().unwrap();
        assert!(!StateStore::load(path).take_tour_pending());
    }

    #[test]
    fn state_file_is_private() {
        let path = temp_path("mode");
        StateStore::load(path.clone()).mark_tour_seen().unwrap();
        let mode =
            |path: &std::path::Path| fs::metadata(path).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode(&path), 0o600);
        assert_eq!(mode(path.parent().unwrap()), 0o700);
    }

    #[test]
    fn corrupt_state_counts_as_first_launch() {
        let path = temp_path("corrupt");
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(&path, b"{not json").unwrap();
        assert!(StateStore::load(path).take_tour_pending());
    }
}
