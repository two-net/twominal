use crate::fish::FishEngine;
use portable_pty::{native_pty_system, CommandBuilder, MasterPty, PtySize};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::thread;
use tauri::{AppHandle, Emitter};

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct PtyOutputPayload {
    pub id: String,
    pub data: String,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct PtyExitPayload {
    pub id: String,
    pub exit_code: Option<u32>,
}

pub struct PtySession {
    pub master: Box<dyn MasterPty + Send>,
    pub writer: Box<dyn Write + Send>,
    pub child: Box<dyn portable_pty::Child + Send>,
}

#[derive(Default)]
pub struct PtyManager {
    pub sessions: Arc<Mutex<HashMap<String, PtySession>>>,
}

impl PtyManager {
    pub fn new() -> Self {
        Self {
            sessions: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    /// Resolves an executable shell, with fallback to standard system shells to avoid permission denied or not found errors.
    pub fn find_valid_shell(shell_override: Option<String>) -> String {
        let mut candidates: Vec<String> = Vec::new();

        if let Some(s) = shell_override {
            let trimmed = s.trim();
            if !trimmed.is_empty() {
                candidates.push(trimmed.to_string());
            }
        }

        if let Ok(env_shell) = std::env::var("SHELL") {
            let trimmed = env_shell.trim();
            if !trimmed.is_empty() {
                candidates.push(trimmed.to_string());
            }
        }

        if cfg!(target_os = "windows") {
            if let Ok(comspec) = std::env::var("COMSPEC") {
                candidates.push(comspec);
            }
            candidates.push("powershell.exe".to_string());
            candidates.push("cmd.exe".to_string());
        } else {
            candidates.push("/bin/zsh".to_string());
            candidates.push("/bin/bash".to_string());
            candidates.push("/bin/sh".to_string());
            candidates.push("/usr/bin/zsh".to_string());
            candidates.push("/usr/bin/bash".to_string());
        }

        for candidate in candidates {
            if candidate.is_empty() {
                continue;
            }

            if cfg!(target_os = "windows") {
                if which::which(&candidate).is_ok() || Path::new(&candidate).is_file() {
                    return candidate;
                }
            } else {
                let path = Path::new(&candidate);
                if path.is_file() {
                    #[cfg(unix)]
                    {
                        use std::os::unix::fs::PermissionsExt;
                        if let Ok(meta) = path.metadata() {
                            if meta.permissions().mode() & 0o111 != 0 {
                                return candidate;
                            }
                        }
                    }
                    #[cfg(not(unix))]
                    return candidate;
                } else if let Ok(found) = which::which(&candidate) {
                    return found.to_string_lossy().to_string();
                }
            }
        }

        if cfg!(target_os = "windows") {
            "cmd.exe".to_string()
        } else {
            "/bin/sh".to_string()
        }
    }

    /// Validates and ensures working directory is accessible, fallback to home directory if not.
    pub fn validate_working_dir(cwd: Option<String>) -> PathBuf {
        let home = dirs::home_dir().unwrap_or_else(|| PathBuf::from("."));
        let target = match cwd {
            Some(ref dir) if !dir.trim().is_empty() => FishEngine::expand_path(dir),
            _ => home.clone(),
        };

        if target.is_dir() {
            target
        } else {
            home
        }
    }

    pub fn spawn(
        &self,
        app_handle: AppHandle,
        id: String,
        cols: u16,
        rows: u16,
        cwd: Option<String>,
        shell_override: Option<String>,
        command: Option<String>,
    ) -> Result<(), String> {
        let pty_system = native_pty_system();
        let pair = pty_system
            .openpty(PtySize {
                rows: rows.max(1),
                cols: cols.max(1),
                pixel_width: 0,
                pixel_height: 0,
            })
            .map_err(|e| format!("Failed to open PTY device (/dev/ptmx): {}. Ensure terminal permissions are granted.", e))?;

        let shell = Self::find_valid_shell(shell_override);
        let mut cmd = CommandBuilder::new(&shell);

        #[cfg(unix)]
        {
            if let Some(ref c) = command {
                if !c.trim().is_empty() {
                    cmd.arg("-l");
                    cmd.arg("-c");
                    cmd.arg(c.trim());
                } else {
                    cmd.arg("-l");
                }
            } else {
                cmd.arg("-l");
            }
        }

        #[cfg(windows)]
        {
            if let Some(ref c) = command {
                if !c.trim().is_empty() {
                    cmd.arg("/c");
                    cmd.arg(c.trim());
                }
            }
        }

        // Inherit environment variables
        for (key, value) in std::env::vars() {
            cmd.env(key, value);
        }
        cmd.env("TERM", "xterm-256color");
        cmd.env("COLORTERM", "truecolor");
        cmd.env("TWOMINAL", "1");
        if std::env::var("LANG").is_err() {
            cmd.env("LANG", "en_US.UTF-8");
        }

        let working_dir = Self::validate_working_dir(cwd);
        cmd.cwd(working_dir);

        let child = pair
            .slave
            .spawn_command(cmd)
            .map_err(|e| {
                format!(
                    "Failed to spawn shell '{}': {}. Verify shell binary exists and has execution permissions.",
                    shell, e
                )
            })?;

        // Drop the slave handle now that the child is spawned
        drop(pair.slave);

        let master = pair.master;
        let mut reader = master
            .try_clone_reader()
            .map_err(|e| format!("Failed to clone PTY reader: {}", e))?;
        let writer = master
            .take_writer()
            .map_err(|e| format!("Failed to take PTY writer: {}", e))?;

        let session = PtySession {
            master,
            writer,
            child,
        };

        {
            let mut sessions = self.sessions.lock().map_err(|e| e.to_string())?;
            sessions.insert(id.clone(), session);
        }

        // Spawn reader background thread for streaming PTY stdout to frontend
        let session_id = id.clone();
        let sessions_map = Arc::clone(&self.sessions);
        thread::spawn(move || {
            let mut buffer = [0u8; 8192];
            loop {
                match reader.read(&mut buffer) {
                    Ok(0) => {
                        // EOF
                        break;
                    }
                    Ok(n) => {
                        let data = String::from_utf8_lossy(&buffer[..n]).to_string();
                        let payload = PtyOutputPayload {
                            id: session_id.clone(),
                            data,
                        };
                        let _ = app_handle.emit("pty-output", payload);
                    }
                    Err(_) => {
                        break;
                    }
                }
            }

            // Cleanup session on exit and retrieve exit code if possible
            let mut exit_code = None;
            if let Ok(mut sessions) = sessions_map.lock() {
                if let Some(mut session) = sessions.remove(&session_id) {
                    if let Ok(status) = session.child.wait() {
                        exit_code = Some(status.exit_code());
                    }
                }
            }

            let _ = app_handle.emit(
                "pty-exit",
                PtyExitPayload {
                    id: session_id.clone(),
                    exit_code,
                },
            );
        });

        Ok(())
    }

    pub fn write(&self, id: &str, data: &str) -> Result<(), String> {
        let mut sessions = self.sessions.lock().map_err(|e| e.to_string())?;
        if let Some(session) = sessions.get_mut(id) {
            session
                .writer
                .write_all(data.as_bytes())
                .map_err(|e| format!("Failed to write to PTY: {}", e))?;
            let _ = session.writer.flush();
            Ok(())
        } else {
            Err(format!("Session '{}' not found", id))
        }
    }

    pub fn resize(&self, id: &str, cols: u16, rows: u16) -> Result<(), String> {
        let mut sessions = self.sessions.lock().map_err(|e| e.to_string())?;
        if let Some(session) = sessions.get_mut(id) {
            session
                .master
                .resize(PtySize {
                    rows: rows.max(1),
                    cols: cols.max(1),
                    pixel_width: 0,
                    pixel_height: 0,
                })
                .map_err(|e| format!("Failed to resize PTY: {}", e))?;
            Ok(())
        } else {
            Err(format!("Session '{}' not found", id))
        }
    }

    pub fn kill(&self, id: &str) -> Result<(), String> {
        let mut sessions = self.sessions.lock().map_err(|e| e.to_string())?;
        if let Some(mut session) = sessions.remove(id) {
            let _ = session.child.kill();
            Ok(())
        } else {
            Ok(())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_find_valid_shell() {
        let shell = PtyManager::find_valid_shell(None);
        assert!(!shell.is_empty());
        assert!(Path::new(&shell).exists() || which::which(&shell).is_ok());
    }

    #[test]
    fn test_validate_working_dir() {
        let home = dirs::home_dir().unwrap_or_else(|| PathBuf::from("."));
        let dir = PtyManager::validate_working_dir(None);
        assert_eq!(dir, home);

        let invalid = PtyManager::validate_working_dir(Some("/nonexistent_path_12345_xyz".to_string()));
        assert_eq!(invalid, home);
    }

    #[test]
    fn test_pty_command_output() {
        let pty_system = native_pty_system();
        let pair = pty_system
            .openpty(PtySize {
                rows: 24,
                cols: 80,
                pixel_width: 0,
                pixel_height: 0,
            })
            .unwrap();

        let shell = PtyManager::find_valid_shell(None);
        let mut cmd = CommandBuilder::new(&shell);
        cmd.arg("-l");
        cmd.arg("-c");
        cmd.arg("echo 'HELLO_PTY'");

        let mut child = pair.slave.spawn_command(cmd).unwrap();
        drop(pair.slave);

        let mut reader = pair.master.try_clone_reader().unwrap();
        let mut output = String::new();
        let mut buf = [0u8; 1024];

        loop {
            match reader.read(&mut buf) {
                Ok(0) => break,
                Ok(n) => {
                    output.push_str(&String::from_utf8_lossy(&buf[..n]));
                }
                Err(_) => break,
            }
        }
        let _ = child.wait();
        println!("PTY OUTPUT RECEIVED: {:?}", output);
        assert!(output.contains("HELLO_PTY"));
    }
}
