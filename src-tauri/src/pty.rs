use crate::host::{default_shell_path, login_path};
use base64::engine::general_purpose::STANDARD;
use base64::Engine;
use portable_pty::{native_pty_system, CommandBuilder, MasterPty, PtySize};
use serde::Serialize;
use std::collections::HashMap;
use std::io::{Read, Write};
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use tauri::{AppHandle, Emitter, State};
use uuid::Uuid;

const MAX_BACKLOG: usize = 2 * 1024 * 1024;

pub struct Session {
    writer: Mutex<Box<dyn Write + Send>>,
    master: Mutex<Box<dyn MasterPty + Send>>,
    child: Mutex<Box<dyn portable_pty::Child + Send>>,
    parked: AtomicBool,
    exited: AtomicBool,
    snapshot: Mutex<Option<String>>,
    backlog: Mutex<Vec<u8>>,
}

#[derive(Default)]
pub struct PtyHub {
    sessions: Mutex<HashMap<String, Arc<Session>>>,
}

#[derive(Clone, Serialize)]
struct PtyPayload {
    id: String,
    data: String,
}

#[derive(Clone, Serialize)]
struct PtyExit {
    id: String,
}

#[derive(Clone, Serialize)]
pub struct ParkedPty {
    pub snapshot: String,
    pub backlog: String,
    pub exited: bool,
}

fn build_command(shell: Option<String>, cwd: Option<String>) -> CommandBuilder {
    let program = shell
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .unwrap_or("")
        .to_string();
    let program = if program.is_empty() {
        default_shell_path()
    } else {
        program
    };

    let mut cmd = CommandBuilder::new(&program);
    let base = Path::new(&program)
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or("");
    #[cfg(unix)]
    {
        if matches!(base, "bash" | "zsh" | "sh") {
            cmd.arg("-l");
        }
    }
    #[cfg(windows)]
    {
        let _ = base;
    }

    cmd.env("TERM", "xterm-256color");
    cmd.env("COLORTERM", "truecolor");
    cmd.env("TERM_PROGRAM", "Twominal");
    let path = login_path();
    if !path.is_empty() {
        cmd.env("PATH", path);
    }

    if let Some(c) = cwd.as_deref().map(str::trim).filter(|s| !s.is_empty()) {
        cmd.cwd(c);
    } else if let Some(home) = dirs::home_dir() {
        cmd.cwd(home);
    }
    cmd
}

fn get_session(hub: &PtyHub, id: &str) -> Result<Arc<Session>, String> {
    hub.sessions
        .lock()
        .map_err(|e| e.to_string())?
        .get(id)
        .cloned()
        .ok_or_else(|| "no such session".to_string())
}

fn push_backlog(session: &Session, chunk: &[u8]) {
    let Ok(mut backlog) = session.backlog.lock() else {
        return;
    };
    if backlog.len() + chunk.len() > MAX_BACKLOG {
        let drop_n = backlog.len() + chunk.len() - MAX_BACKLOG;
        backlog.drain(..drop_n);
    }
    backlog.extend_from_slice(chunk);
}

#[tauri::command]
pub fn pty_spawn(
    app: AppHandle,
    hub: State<'_, PtyHub>,
    cols: u16,
    rows: u16,
    shell: Option<String>,
    cwd: Option<String>,
) -> Result<String, String> {
    let pty_system = native_pty_system();
    let pair = pty_system
        .openpty(PtySize {
            rows: rows.max(2),
            cols: cols.max(2),
            pixel_width: 0,
            pixel_height: 0,
        })
        .map_err(|e| format!("open pty: {e}"))?;

    let cmd = build_command(shell, cwd);
    let mut reader = pair
        .master
        .try_clone_reader()
        .map_err(|e| format!("pty reader: {e}"))?;
    let writer = pair
        .master
        .take_writer()
        .map_err(|e| format!("pty writer: {e}"))?;
    let child = pair
        .slave
        .spawn_command(cmd)
        .map_err(|e| format!("spawn shell: {e}"))?;
    drop(pair.slave);

    let id = Uuid::new_v4().to_string();
    let emit_id = id.clone();
    let handle = app.clone();

    let session = Arc::new(Session {
        writer: Mutex::new(writer),
        master: Mutex::new(pair.master),
        child: Mutex::new(child),
        parked: AtomicBool::new(false),
        exited: AtomicBool::new(false),
        snapshot: Mutex::new(None),
        backlog: Mutex::new(Vec::new()),
    });
    let reader_session = session.clone();

    std::thread::Builder::new()
        .name(format!("pty-{emit_id}"))
        .spawn(move || {
            let mut buf = [0u8; 8192];
            loop {
                match reader.read(&mut buf) {
                    Ok(0) => break,
                    Ok(n) => {
                        if reader_session.parked.load(Ordering::SeqCst) {
                            push_backlog(&reader_session, &buf[..n]);
                        } else {
                            let payload = PtyPayload {
                                id: emit_id.clone(),
                                data: STANDARD.encode(&buf[..n]),
                            };
                            let _ = handle.emit("pty-output", payload);
                        }
                    }
                    Err(e) if e.kind() == std::io::ErrorKind::Interrupted => continue,
                    Err(_) => break,
                }
            }
            reader_session.exited.store(true, Ordering::SeqCst);
            let _ = handle.emit("pty-exit", PtyExit { id: emit_id });
        })
        .map_err(|e| e.to_string())?;

    hub.sessions
        .lock()
        .map_err(|e| e.to_string())?
        .insert(id.clone(), session);
    Ok(id)
}

#[tauri::command]
pub fn pty_write(hub: State<'_, PtyHub>, id: String, data: String) -> Result<(), String> {
    let session = get_session(&hub, &id)?;
    let mut writer = session.writer.lock().map_err(|e| e.to_string())?;
    writer.write_all(data.as_bytes()).map_err(|e| e.to_string())?;
    writer.flush().map_err(|e| e.to_string())
}

#[tauri::command]
pub fn pty_resize(hub: State<'_, PtyHub>, id: String, cols: u16, rows: u16) -> Result<(), String> {
    let session = get_session(&hub, &id)?;
    let master = session.master.lock().map_err(|e| e.to_string())?;
    master
        .resize(PtySize {
            rows: rows.max(2),
            cols: cols.max(2),
            pixel_width: 0,
            pixel_height: 0,
        })
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub fn pty_kill(hub: State<'_, PtyHub>, id: String) -> Result<(), String> {
    let mut sessions = hub.sessions.lock().map_err(|e| e.to_string())?;
    if let Some(session) = sessions.remove(&id) {
        let _ = session.child.lock().map_err(|e| e.to_string())?.kill();
    }
    Ok(())
}

#[tauri::command]
pub fn pty_park(hub: State<'_, PtyHub>, id: String, snapshot: String) -> Result<(), String> {
    let session = get_session(&hub, &id)?;
    session.parked.store(true, Ordering::SeqCst);
    *session.snapshot.lock().map_err(|e| e.to_string())? = Some(snapshot);
    Ok(())
}

#[tauri::command]
pub fn pty_unpark(hub: State<'_, PtyHub>, id: String) -> Result<ParkedPty, String> {
    let session = get_session(&hub, &id)?;
    session.parked.store(false, Ordering::SeqCst);
    let snapshot = session
        .snapshot
        .lock()
        .map_err(|e| e.to_string())?
        .take()
        .unwrap_or_default();
    let backlog = {
        let mut buf = session.backlog.lock().map_err(|e| e.to_string())?;
        let encoded = STANDARD.encode(&*buf);
        buf.clear();
        encoded
    };
    Ok(ParkedPty {
        snapshot,
        backlog,
        exited: session.exited.load(Ordering::SeqCst),
    })
}
