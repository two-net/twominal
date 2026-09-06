use crate::config::data_dir;
use serde::Serialize;
use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::OnceLock;
use std::time::{Duration, Instant};

pub fn default_shell_path() -> String {
    #[cfg(windows)]
    {
        std::env::var("COMSPEC").unwrap_or_else(|_| "powershell.exe".into())
    }
    #[cfg(not(windows))]
    {
        std::env::var("SHELL").unwrap_or_else(|_| "/bin/bash".into())
    }
}

#[tauri::command]
pub fn default_shell() -> String {
    default_shell_path()
}

#[derive(Clone, Serialize)]
pub struct ShellInfo {
    pub path: String,
    pub name: String,
}

#[tauri::command]
pub fn list_shells() -> Vec<ShellInfo> {
    let mut seen = BTreeSet::new();
    let mut out = Vec::new();

    let mut push = |path: String| {
        if path.is_empty() || !seen.insert(path.clone()) {
            return;
        }
        let p = Path::new(&path);
        if !p.is_file() {
            return;
        }
        let name = p
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or(path.as_str())
            .to_string();
        out.push(ShellInfo { path, name });
    };

    push(default_shell_path());

    #[cfg(unix)]
    {
        for c in [
            "/bin/zsh",
            "/bin/bash",
            "/bin/sh",
            "/usr/bin/zsh",
            "/usr/bin/bash",
            "/usr/bin/sh",
            "/usr/local/bin/zsh",
            "/usr/local/bin/bash",
            "/opt/homebrew/bin/zsh",
            "/opt/homebrew/bin/bash",
        ] {
            push(c.to_string());
        }
    }

    #[cfg(windows)]
    {
        if let Ok(system_root) = std::env::var("SystemRoot") {
            push(format!(
                "{system_root}\\System32\\WindowsPowerShell\\v1.0\\powershell.exe"
            ));
            push(format!("{system_root}\\System32\\cmd.exe"));
        }
        if let Ok(program_files) = std::env::var("ProgramFiles") {
            push(format!("{program_files}\\PowerShell\\7\\pwsh.exe"));
        }
        if let Ok(program_files) = std::env::var("ProgramFiles(x86)") {
            push(format!("{program_files}\\PowerShell\\7\\pwsh.exe"));
        }
    }

    out
}

pub fn login_path() -> String {
    static CACHED: OnceLock<String> = OnceLock::new();
    CACHED
        .get_or_init(|| discover_login_path().unwrap_or_else(|| std::env::var("PATH").unwrap_or_default()))
        .clone()
}

fn discover_login_path() -> Option<String> {
    #[cfg(windows)]
    {
        return std::env::var("PATH").ok();
    }
    #[cfg(not(windows))]
    {
        let shell = default_shell_path();
        let out = Command::new(&shell)
            .arg("-lc")
            .arg("printf %s \"$PATH\"")
            .output()
            .ok()?;
        if !out.status.success() {
            return None;
        }
        let s = String::from_utf8_lossy(&out.stdout).trim().to_string();
        if s.is_empty() {
            None
        } else {
            Some(s)
        }
    }
}

fn expand_tilde(s: &str) -> PathBuf {
    if s == "~" || s.starts_with("~/") || s.starts_with("~\\") {
        if let Some(home) = dirs::home_dir() {
            if s == "~" {
                return home;
            }
            return home.join(&s[2..]);
        }
    }
    PathBuf::from(s)
}

fn is_executable(path: &Path) -> bool {
    #[cfg(windows)]
    {
        matches!(
            path.extension().and_then(|e| e.to_str()).map(|e| e.to_ascii_lowercase()),
            Some(ref e) if e == "exe" || e == "bat" || e == "cmd" || e == "com" || e == "ps1"
        )
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::metadata(path)
            .map(|m| m.is_file() && m.permissions().mode() & 0o111 != 0)
            .unwrap_or(false)
    }
}

#[tauri::command]
pub fn list_commands() -> Vec<String> {
    static CACHE: OnceLock<(Instant, Vec<String>)> = OnceLock::new();
    if let Some((at, names)) = CACHE.get() {
        if at.elapsed() < Duration::from_secs(30) {
            return names.clone();
        }
    }
    let mut set = BTreeSet::new();
    let path = login_path();
    let sep = if cfg!(windows) { ';' } else { ':' };
    for dir in path.split(sep).filter(|s| !s.is_empty()) {
        let Ok(entries) = fs::read_dir(dir) else { continue };
        for entry in entries.flatten() {
            let path = entry.path();
            if is_executable(&path) {
                if let Some(name) = path.file_name().and_then(|n| n.to_str()) {
                    set.insert(name.to_string());
                }
            }
        }
    }
    let names: Vec<String> = set.into_iter().collect();
    let _ = CACHE.set((Instant::now(), names.clone()));
    names
}

#[tauri::command]
pub fn complete_paths(cwd: String, prefix: String) -> Vec<String> {
    let cwd = expand_tilde(&cwd);
    let raw = prefix.replace('\\', "/");
    let ends_dir = raw.ends_with('/');
    let (dir, file_prefix, out_prefix) = if raw.is_empty() {
        (cwd.clone(), String::new(), String::new())
    } else if ends_dir {
        let dir = if Path::new(&raw).is_absolute() || raw.starts_with("~/") || raw == "~/" {
            expand_tilde(&raw)
        } else {
            cwd.join(&raw)
        };
        (dir, String::new(), raw)
    } else {
        let path = if Path::new(&raw).is_absolute() || raw.starts_with("~/") || raw == "~" {
            expand_tilde(&raw)
        } else {
            cwd.join(&raw)
        };
        let file_prefix = path
            .file_name()
            .and_then(|s| s.to_str())
            .unwrap_or("")
            .to_string();
        let parent = path.parent().map(|p| p.to_path_buf()).unwrap_or(cwd);
        let out_prefix = match raw.rfind('/') {
            Some(i) => raw[..=i].to_string(),
            None => String::new(),
        };
        (parent, file_prefix, out_prefix)
    };

    let Ok(entries) = fs::read_dir(&dir) else {
        return Vec::new();
    };
    let show_hidden = file_prefix.starts_with('.');
    let needle = file_prefix.to_ascii_lowercase();
    let mut out = Vec::new();
    for entry in entries.flatten() {
        let name = entry.file_name();
        let Some(name) = name.to_str() else { continue };
        if name == "." || name == ".." {
            continue;
        }
        if !show_hidden && name.starts_with('.') {
            continue;
        }
        if !needle.is_empty() && !name.to_ascii_lowercase().starts_with(&needle) {
            continue;
        }
        let mut item = format!("{out_prefix}{name}");
        if entry.file_type().map(|t| t.is_dir()).unwrap_or(false) {
            item.push('/');
        }
        out.push(item);
        if out.len() >= 400 {
            break;
        }
    }
    out.sort();
    out.truncate(80);
    out
}

#[tauri::command]
pub fn git_branch(cwd: String) -> Option<String> {
    let dir = expand_tilde(&cwd);
    let out = Command::new("git")
        .args(["rev-parse", "--abbrev-ref", "HEAD"])
        .current_dir(&dir)
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    let s = String::from_utf8_lossy(&out.stdout).trim().to_string();
    if s.is_empty() || s == "HEAD" {
        None
    } else {
        Some(s)
    }
}

#[tauri::command]
pub fn history_load() -> Result<Vec<String>, String> {
    let path = data_dir()?.join("history");
    if !path.exists() {
        return Ok(Vec::new());
    }
    let text = fs::read_to_string(path).map_err(|e| e.to_string())?;
    Ok(text.lines().map(|l| l.to_string()).filter(|l| !l.is_empty()).collect())
}

#[tauri::command]
pub fn history_save(entries: Vec<String>) -> Result<(), String> {
    let path = data_dir()?.join("history");
    let cap = 2000.min(entries.len());
    let slice = &entries[entries.len() - cap..];
    fs::write(path, slice.join("\n")).map_err(|e| e.to_string())
}
