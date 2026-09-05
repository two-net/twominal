use std::fs::{self, File, OpenOptions};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock, TryLockError};
use std::time::{SystemTime, UNIX_EPOCH};

use serde::Serialize;

const LOG_FILE_NAME: &str = "twominal-events.jsonl";
const LOG_BACKUP_NAME: &str = "twominal-events.jsonl.1";
const MAX_LOG_BYTES: u64 = 1024 * 1024;

static LOGGER: OnceLock<LocalLogger> = OnceLock::new();

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct LogEntry {
    timestamp_ms: u64,
    level: &'static str,
    event: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    code: Option<&'static str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    active_sessions: Option<usize>,
}

struct LocalLogger {
    writer: Mutex<LogWriter>,
    path: PathBuf,
}

struct LogWriter {
    path: PathBuf,
    backup_path: PathBuf,
    file: Option<File>,
    bytes_written: u64,
    max_bytes: u64,
}

pub fn initialize(directory: &Path) -> io::Result<PathBuf> {
    if let Some(logger) = LOGGER.get() {
        return Ok(logger.path.clone());
    }

    let writer = LogWriter::open(directory, MAX_LOG_BYTES)?;
    let path = writer.path.clone();
    let logger = LocalLogger {
        writer: Mutex::new(writer),
        path: path.clone(),
    };

    // Another initialization can only win during an unusual concurrent startup.
    // In that case the installed logger is already suitable for this process.
    if LOGGER.set(logger).is_err() {
        return Ok(LOGGER
            .get()
            .map(|installed| installed.path.clone())
            .unwrap_or(path));
    }

    Ok(path)
}

pub fn info(event: &'static str) {
    record(LogEntry {
        timestamp_ms: timestamp_ms(),
        level: "info",
        event,
        code: None,
        active_sessions: None,
    });
}

pub fn error(event: &'static str, code: &'static str) {
    record(LogEntry {
        timestamp_ms: timestamp_ms(),
        level: "error",
        event,
        code: Some(code),
        active_sessions: None,
    });
}

pub fn sessions(event: &'static str, active_sessions: usize) {
    record(LogEntry {
        timestamp_ms: timestamp_ms(),
        level: "info",
        event,
        code: None,
        active_sessions: Some(active_sessions),
    });
}

pub fn flush() {
    let Some(logger) = LOGGER.get() else {
        return;
    };
    match logger.writer.try_lock() {
        Ok(mut writer) => {
            let _ = writer.flush();
        }
        Err(TryLockError::Poisoned(poisoned)) => {
            let _ = poisoned.into_inner().flush();
        }
        Err(TryLockError::WouldBlock) => {}
    }
}

fn record(entry: LogEntry) {
    let Some(logger) = LOGGER.get() else {
        return;
    };
    match logger.writer.try_lock() {
        Ok(mut writer) => {
            let _ = writer.write(&entry);
        }
        Err(TryLockError::Poisoned(poisoned)) => {
            let _ = poisoned.into_inner().write(&entry);
        }
        // Logging must never introduce a lock inversion in a panic path.
        Err(TryLockError::WouldBlock) => {}
    }
}

fn timestamp_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
        .try_into()
        .unwrap_or(u64::MAX)
}

impl LogWriter {
    fn open(directory: &Path, max_bytes: u64) -> io::Result<Self> {
        fs::create_dir_all(directory)?;
        set_private_directory_permissions(directory)?;

        let path = directory.join(LOG_FILE_NAME);
        let backup_path = directory.join(LOG_BACKUP_NAME);
        let bytes_written = fs::metadata(&path)
            .map(|metadata| metadata.len())
            .unwrap_or(0);
        let mut writer = Self {
            path,
            backup_path,
            file: None,
            bytes_written,
            max_bytes,
        };

        if bytes_written >= max_bytes {
            writer.rotate()?;
        } else {
            writer.file = Some(open_private_file(&writer.path, true)?);
        }
        Ok(writer)
    }

    fn write(&mut self, entry: &LogEntry) -> io::Result<()> {
        let mut line = serde_json::to_vec(entry).map_err(io::Error::other)?;
        line.push(b'\n');

        if self.bytes_written > 0
            && self.bytes_written.saturating_add(line.len() as u64) > self.max_bytes
        {
            self.rotate()?;
        }

        let file = self
            .file
            .as_mut()
            .ok_or_else(|| io::Error::other("Twominal log file is unavailable"))?;
        file.write_all(&line)?;
        file.flush()?;
        self.bytes_written = self.bytes_written.saturating_add(line.len() as u64);
        Ok(())
    }

    fn flush(&mut self) -> io::Result<()> {
        if let Some(file) = self.file.as_mut() {
            file.flush()?;
        }
        Ok(())
    }

    fn rotate(&mut self) -> io::Result<()> {
        self.file.take();
        match fs::remove_file(&self.backup_path) {
            Ok(()) => {}
            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
            Err(error) => return Err(error),
        }
        match fs::rename(&self.path, &self.backup_path) {
            Ok(()) => {}
            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
            Err(error) => {
                self.file = open_private_file(&self.path, true).ok();
                return Err(error);
            }
        }
        self.file = Some(open_private_file(&self.path, false)?);
        self.bytes_written = 0;
        Ok(())
    }
}

fn open_private_file(path: &Path, append: bool) -> io::Result<File> {
    let mut options = OpenOptions::new();
    options.create(true).write(true);
    if append {
        options.append(true);
    } else {
        options.truncate(true);
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let file = options.open(path)?;
    set_private_file_permissions(path)?;
    Ok(file)
}

#[cfg(unix)]
fn set_private_directory_permissions(directory: &Path) -> io::Result<()> {
    use std::os::unix::fs::PermissionsExt;
    fs::set_permissions(directory, fs::Permissions::from_mode(0o700))
}

#[cfg(not(unix))]
fn set_private_directory_permissions(_directory: &Path) -> io::Result<()> {
    Ok(())
}

#[cfg(unix)]
fn set_private_file_permissions(path: &Path) -> io::Result<()> {
    use std::os::unix::fs::PermissionsExt;
    fs::set_permissions(path, fs::Permissions::from_mode(0o600))
}

#[cfg(not(unix))]
fn set_private_file_permissions(_path: &Path) -> io::Result<()> {
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temporary_directory(label: &str) -> PathBuf {
        std::env::temp_dir().join(format!("twominal-log-{label}-{}", uuid::Uuid::new_v4()))
    }

    fn test_entry(event: &'static str) -> LogEntry {
        LogEntry {
            timestamp_ms: 42,
            level: "info",
            event,
            code: None,
            active_sessions: Some(2),
        }
    }

    #[test]
    fn writes_a_small_structured_privacy_preserving_schema() {
        let directory = temporary_directory("schema");
        let mut writer = LogWriter::open(&directory, 4096).unwrap();
        writer.write(&test_entry("application_started")).unwrap();
        drop(writer);

        let contents = fs::read_to_string(directory.join(LOG_FILE_NAME)).unwrap();
        let value: serde_json::Value = serde_json::from_str(contents.trim()).unwrap();
        assert_eq!(value["timestampMs"], 42);
        assert_eq!(value["level"], "info");
        assert_eq!(value["event"], "application_started");
        assert_eq!(value["activeSessions"], 2);
        assert_eq!(value.as_object().unwrap().len(), 4);

        fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn rotates_to_one_bounded_backup() {
        let directory = temporary_directory("rotation");
        let mut writer = LogWriter::open(&directory, 320).unwrap();
        for _ in 0..20 {
            writer
                .write(&test_entry("terminal_session_started"))
                .unwrap();
        }
        drop(writer);

        let current = fs::metadata(directory.join(LOG_FILE_NAME)).unwrap().len();
        let backup = fs::metadata(directory.join(LOG_BACKUP_NAME)).unwrap().len();
        assert!(current <= 320, "current log grew to {current} bytes");
        assert!(backup <= 320, "backup log grew to {backup} bytes");
        assert_eq!(fs::read_dir(&directory).unwrap().count(), 2);

        fs::remove_dir_all(directory).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn restricts_log_file_and_directory_permissions() {
        use std::os::unix::fs::PermissionsExt;

        let directory = temporary_directory("permissions");
        let writer = LogWriter::open(&directory, 4096).unwrap();
        drop(writer);

        let directory_mode = fs::metadata(&directory).unwrap().permissions().mode() & 0o777;
        let file_mode = fs::metadata(directory.join(LOG_FILE_NAME))
            .unwrap()
            .permissions()
            .mode()
            & 0o777;
        assert_eq!(directory_mode, 0o700);
        assert_eq!(file_mode, 0o600);

        fs::remove_dir_all(directory).unwrap();
    }
}
