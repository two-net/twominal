use std::fs::{self, File};
use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, MutexGuard};
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Manager, State};

use crate::error::CommandError;
use crate::storage::write_private_atomically;

const HISTORY_FILE_NAME: &str = "history.json";
const HISTORY_SCHEMA_VERSION: u16 = 1;
const MAX_HISTORY_BYTES: u64 = 512 * 1024;
const MAX_HISTORY_ENTRIES: usize = 1_000;
const MAX_COMMAND_BYTES: usize = 4 * 1024;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct HistoryEntry {
    pub command: String,
    pub last_used_at_ms: u64,
    pub use_count: u32,
}

#[derive(Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct HistoryFile {
    schema_version: u16,
    entries: Vec<HistoryEntry>,
}

impl HistoryFile {
    fn empty() -> Self {
        Self {
            schema_version: HISTORY_SCHEMA_VERSION,
            entries: Vec::new(),
        }
    }

    fn validate(self) -> Result<Self, CommandError> {
        if self.schema_version != HISTORY_SCHEMA_VERSION
            || self.entries.len() > MAX_HISTORY_ENTRIES
            || self.entries.iter().any(|entry| {
                entry.use_count == 0
                    || normalize_command(&entry.command).as_deref() != Some(entry.command.as_str())
            })
        {
            return Err(history_error(
                "history_parse_failed",
                "The command history is malformed or unsupported.",
            ));
        }
        Ok(self)
    }
}

#[derive(Default)]
pub struct HistoryStore {
    gate: Mutex<()>,
}

impl HistoryStore {
    fn lock(&self) -> MutexGuard<'_, ()> {
        self.gate
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }
}

#[tauri::command(async)]
pub fn history_load(
    app: AppHandle,
    state: State<'_, HistoryStore>,
) -> Result<Vec<HistoryEntry>, CommandError> {
    let _guard = state.lock();
    Ok(load_from_path(&history_path(&app)?)?.entries)
}

#[tauri::command(async)]
pub fn history_append(
    app: AppHandle,
    state: State<'_, HistoryStore>,
    command: String,
) -> Result<Option<HistoryEntry>, CommandError> {
    let Some(command) = normalize_command(&command) else {
        return Ok(None);
    };

    let _guard = state.lock();
    let path = history_path(&app)?;
    let mut history = load_from_path(&path)?;
    let previous_count = history
        .entries
        .iter()
        .find(|entry| entry.command == command)
        .map(|entry| entry.use_count)
        .unwrap_or(0);
    history.entries.retain(|entry| entry.command != command);

    let entry = HistoryEntry {
        command,
        last_used_at_ms: current_time_millis(),
        use_count: previous_count.saturating_add(1),
    };
    history.entries.push(entry.clone());
    trim_and_save(&path, &mut history)?;
    Ok(Some(entry))
}

#[tauri::command(async)]
pub fn history_clear(app: AppHandle, state: State<'_, HistoryStore>) -> Result<(), CommandError> {
    let _guard = state.lock();
    match fs::remove_file(history_path(&app)?) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(_) => Err(history_write_error()),
    }
}

fn history_path(app: &AppHandle) -> Result<PathBuf, CommandError> {
    app.path()
        .app_config_dir()
        .map(|directory| directory.join(HISTORY_FILE_NAME))
        .map_err(|_| {
            history_error(
                "history_path_unavailable",
                "The command history directory is unavailable.",
            )
        })
}

fn load_from_path(path: &Path) -> Result<HistoryFile, CommandError> {
    let file = match File::open(path) {
        Ok(file) => file,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return Ok(HistoryFile::empty())
        }
        Err(_) => {
            return Err(history_error(
                "history_read_failed",
                "The command history could not be read.",
            ))
        }
    };

    let mut bytes = Vec::new();
    file.take(MAX_HISTORY_BYTES + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| {
            history_error(
                "history_read_failed",
                "The command history could not be read.",
            )
        })?;
    if bytes.len() as u64 > MAX_HISTORY_BYTES {
        return Err(history_error(
            "history_too_large",
            "The command history exceeds its storage limit.",
        ));
    }

    serde_json::from_slice::<HistoryFile>(&bytes)
        .map_err(|_| {
            history_error(
                "history_parse_failed",
                "The command history is malformed or unsupported.",
            )
        })?
        .validate()
}

fn trim_and_save(path: &Path, history: &mut HistoryFile) -> Result<(), CommandError> {
    if history.entries.len() > MAX_HISTORY_ENTRIES {
        history
            .entries
            .drain(..history.entries.len() - MAX_HISTORY_ENTRIES);
    }

    loop {
        let mut bytes = serde_json::to_vec_pretty(history).map_err(|_| history_write_error())?;
        bytes.push(b'\n');
        if bytes.len() as u64 <= MAX_HISTORY_BYTES {
            return write_private_atomically(path, &bytes).map_err(|_| history_write_error());
        }
        if history.entries.is_empty() {
            return Err(history_write_error());
        }
        history.entries.remove(0);
    }
}

fn normalize_command(command: &str) -> Option<String> {
    if command.is_empty()
        || command.starts_with(char::is_whitespace)
        || command.len() > MAX_COMMAND_BYTES
        || command.chars().any(is_unsafe_history_character)
    {
        return None;
    }

    let normalized = command.trim_end();
    (!normalized.is_empty()).then(|| normalized.to_owned())
}

fn is_unsafe_history_character(character: char) -> bool {
    character.is_control()
        || matches!(
            character,
            '\u{061c}'
                | '\u{200e}'
                | '\u{200f}'
                | '\u{202a}'..='\u{202e}'
                | '\u{2066}'..='\u{206f}'
                | '\u{feff}'
        )
}

fn current_time_millis() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
        .try_into()
        .unwrap_or(u64::MAX)
}

fn history_error(code: &'static str, message: &'static str) -> CommandError {
    CommandError::new(code, message)
}

fn history_write_error() -> CommandError {
    history_error(
        "history_write_failed",
        "The command history could not be saved.",
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use uuid::Uuid;

    struct TestDirectory(PathBuf);

    impl TestDirectory {
        fn new() -> Self {
            let path = std::env::temp_dir().join(format!("twominal-history-{}", Uuid::new_v4()));
            fs::create_dir(&path).expect("create history test directory");
            Self(path)
        }

        fn path(&self) -> PathBuf {
            self.0.join(HISTORY_FILE_NAME)
        }
    }

    impl Drop for TestDirectory {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn normalizes_safe_single_line_commands_and_honors_private_prefixes() {
        assert_eq!(
            normalize_command("git status  "),
            Some("git status".to_owned())
        );
        assert_eq!(normalize_command(" git status"), None);
        assert_eq!(normalize_command("printf 'a\\nb'\n"), None);
        assert_eq!(normalize_command("\tsecret"), None);
        assert_eq!(normalize_command("echo\u{202e}unsafe"), None);
        assert_eq!(normalize_command(&"x".repeat(MAX_COMMAND_BYTES + 1)), None);
    }

    #[test]
    fn missing_history_is_empty_and_malformed_history_is_rejected() {
        let directory = TestDirectory::new();
        let path = directory.path();
        assert!(load_from_path(&path).unwrap().entries.is_empty());

        fs::write(&path, b"not json").unwrap();
        assert_eq!(
            load_from_path(&path).unwrap_err().code,
            "history_parse_failed"
        );
    }

    #[test]
    fn trims_oldest_entries_and_round_trips_stats() {
        let directory = TestDirectory::new();
        let path = directory.path();
        let mut history = HistoryFile::empty();
        for index in 0..=MAX_HISTORY_ENTRIES {
            history.entries.push(HistoryEntry {
                command: format!("echo {index}"),
                last_used_at_ms: index as u64,
                use_count: 1,
            });
        }

        trim_and_save(&path, &mut history).unwrap();
        let loaded = load_from_path(&path).unwrap();
        assert_eq!(loaded.entries.len(), MAX_HISTORY_ENTRIES);
        assert_eq!(loaded.entries.first().unwrap().command, "echo 1");
        assert_eq!(loaded.entries.last().unwrap().command, "echo 1000");
    }

    #[cfg(unix)]
    #[test]
    fn history_is_written_with_owner_only_permissions() {
        use std::os::unix::fs::PermissionsExt;

        let directory = TestDirectory::new();
        let path = directory.path();
        let mut history = HistoryFile::empty();
        history.entries.push(HistoryEntry {
            command: "pwd".to_owned(),
            last_used_at_ms: 1,
            use_count: 1,
        });
        trim_and_save(&path, &mut history).unwrap();

        assert_eq!(
            fs::metadata(path).unwrap().permissions().mode() & 0o777,
            0o600
        );
    }
}
