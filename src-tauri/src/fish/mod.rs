use crate::pty::PtyManager;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs::{self, OpenOptions};
use std::io::{BufRead, BufReader, Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::{Arc, Mutex};
use std::thread;
use tauri::{AppHandle, Emitter};

static ACTIVE_PROCESSES: std::sync::LazyLock<Arc<Mutex<HashMap<String, u32>>>> =
    std::sync::LazyLock::new(|| Arc::new(Mutex::new(HashMap::new())));

static ACTIVE_STDIN: std::sync::LazyLock<Arc<Mutex<HashMap<String, std::process::ChildStdin>>>> =
    std::sync::LazyLock::new(|| Arc::new(Mutex::new(HashMap::new())));

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct ShellOutputPayload {
    pub tab_id: String,
    pub data: String,
}

const SHELL_BUILTINS: &[&str] = &[
    "cd", "echo", "export", "exit", "source", "alias", "unalias", "history",
    "set", "unset", "type", "pwd", "pushd", "popd", "dirs", "read",
    "exec", "eval", "bg", "fg", "jobs", "kill", "wait", "disown",
    "shift", "return", "trap", "test", "[", "[[", "builtin", "command",
    "enable", "help", "let", "local", "ulimit", "umask", "hash", "bind",
    "times", "declare", "typeset", "readonly", "printf", "true", "false",
    "colon", ".", "function", "theme", "font", "vim", "matrix", "stack",
    "ligatures", "neofetch", "twominalfetch", "clear", "date", "settings",
    "tabs", "tab"
];

pub const TWOMINAL_SLASH_BUILTINS: &[(&str, &str)] = &[
    ("/help", "Twominal builtin & slash commands guide"),
    ("/theme", "Switch theme mode (dark | light | auto)"),
    ("/font", "Configure typography, size, weight & ligatures"),
    ("/vim", "Toggle Vim modal navigation mode (NORMAL/INSERT)"),
    ("/matrix", "Toggle background matrix digital rain effect"),
    ("/clear", "Clear terminal scrollback buffer"),
    ("/neofetch", "Twominal system & environment info banner"),
    ("/settings", "Open settings & typography modal"),
    ("/stack", "Twominal engine architecture & compilation info"),
    ("/ligatures", "Test or toggle font programming ligatures"),
    ("/history", "Display persistent command execution history"),
    ("/date", "Display current system timestamp"),
    ("/tabs", "Multi-tab management (new, close, next, prev)"),
    ("/exit", "Close active shell tab session"),
];

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct CommandCheckResult {
    pub name: String,
    pub exists: bool,
    pub kind: String, // "builtin", "executable", "path", "none"
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct CompletionItem {
    pub label: String,
    pub value: String,
    pub kind: String, // "file", "dir", "executable", "builtin", "history"
    pub description: Option<String>,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct ShellExecResult {
    pub stdout: String,
    pub stderr: String,
    pub exit_code: i32,
    pub new_cwd: String,
    pub display_cwd: String,
    pub git_branch: Option<String>,
}

pub struct FishEngine;

impl FishEngine {
    pub fn expand_path(raw_path: &str) -> PathBuf {
        let trimmed = raw_path.trim();
        if trimmed.is_empty() {
            return dirs::home_dir().unwrap_or_else(|| PathBuf::from("."));
        }

        if trimmed == "~" {
            dirs::home_dir().unwrap_or_else(|| PathBuf::from("."))
        } else if let Some(stripped) = trimmed.strip_prefix("~/") {
            if let Some(home) = dirs::home_dir() {
                home.join(stripped)
            } else {
                PathBuf::from(trimmed)
            }
        } else if let Some(stripped) = trimmed.strip_prefix("~\\") {
            if let Some(home) = dirs::home_dir() {
                home.join(stripped)
            } else {
                PathBuf::from(trimmed)
            }
        } else {
            PathBuf::from(trimmed)
        }
    }

    pub fn display_path(path: &Path) -> String {
        if let Some(home) = dirs::home_dir() {
            if let Ok(rel) = path.strip_prefix(&home) {
                if rel.as_os_str().is_empty() {
                    return "~".to_string();
                } else {
                    return format!("~/{}", rel.to_string_lossy());
                }
            }
        }
        path.to_string_lossy().to_string()
    }

    pub fn check_command(command: &str) -> CommandCheckResult {
        let trimmed = command.trim();
        if trimmed.is_empty() {
            return CommandCheckResult {
                name: String::new(),
                exists: false,
                kind: "none".to_string(),
            };
        }

        // Check if slash command
        if trimmed == "/" {
            return CommandCheckResult {
                name: trimmed.to_string(),
                exists: true,
                kind: "builtin".to_string(),
            };
        }

        if trimmed.starts_with('/') && !trimmed[1..].contains('/') {
            for &(slash_cmd, _) in TWOMINAL_SLASH_BUILTINS {
                if trimmed == slash_cmd {
                    return CommandCheckResult {
                        name: trimmed.to_string(),
                        exists: true,
                        kind: "builtin".to_string(),
                    };
                }
            }
        }

        // Check if it's a shell builtin
        if SHELL_BUILTINS.contains(&trimmed) {
            return CommandCheckResult {
                name: trimmed.to_string(),
                exists: true,
                kind: "builtin".to_string(),
            };
        }

        // Check if it's a direct file or relative/absolute path
        let path = Self::expand_path(trimmed);
        if trimmed.contains('/') || trimmed.contains('\\') || trimmed.starts_with('~') || trimmed.starts_with('.') {
            if path.exists() {
                return CommandCheckResult {
                    name: trimmed.to_string(),
                    exists: true,
                    kind: "path".to_string(),
                };
            }
        }

        // Check if it exists in PATH
        if let Ok(resolved) = which::which(trimmed) {
            return CommandCheckResult {
                name: trimmed.to_string(),
                exists: true,
                kind: if resolved.is_file() {
                    "executable".to_string()
                } else {
                    "path".to_string()
                },
            };
        }

        CommandCheckResult {
            name: trimmed.to_string(),
            exists: false,
            kind: "none".to_string(),
        }
    }

    pub fn get_completions(cwd_str: &str, prefix: &str) -> Vec<CompletionItem> {
        let mut results: Vec<CompletionItem> = Vec::new();
        let trimmed_prefix = prefix.trim();
        let current_dir = Self::expand_path(cwd_str);

        // 1. If prefix starts with '/', check matching Twominal slash commands
        if trimmed_prefix.starts_with('/') && !trimmed_prefix[1..].contains('/') {
            for &(slash_cmd, desc) in TWOMINAL_SLASH_BUILTINS {
                if slash_cmd.starts_with(trimmed_prefix) {
                    results.push(CompletionItem {
                        label: slash_cmd.to_string(),
                        value: slash_cmd.to_string(),
                        kind: "builtin".to_string(),
                        description: Some(desc.to_string()),
                    });
                }
            }
        }

        // 2. Check matching files & directories
        let (search_dir, file_query, dir_prefix) = if let Some(idx) = trimmed_prefix.rfind(|c| c == '/' || c == '\\') {
            let (dir_part, query) = trimmed_prefix.split_at(idx + 1);
            let expanded_dir = if dir_part.starts_with('~') || dir_part.starts_with('/') || (dir_part.len() >= 2 && dir_part.chars().nth(1) == Some(':')) {
                Self::expand_path(dir_part)
            } else {
                current_dir.join(dir_part)
            };
            (expanded_dir, query, dir_part)
        } else {
            (current_dir.clone(), trimmed_prefix, "")
        };

        if let Ok(entries) = fs::read_dir(&search_dir) {
            for entry in entries.flatten() {
                let file_name = entry.file_name().to_string_lossy().to_string();
                if file_name.starts_with('.') && !file_query.starts_with('.') {
                    continue;
                }
                if file_name.to_lowercase().starts_with(&file_query.to_lowercase()) {
                    let is_dir = if let Ok(ft) = entry.file_type() {
                        if ft.is_dir() {
                            true
                        } else if ft.is_symlink() {
                            fs::metadata(entry.path()).map(|m| m.is_dir()).unwrap_or(false)
                        } else {
                            false
                        }
                    } else {
                        false
                    };
                    let full_val = format!("{}{}{}", dir_prefix, file_name, if is_dir { "/" } else { "" });

                    results.push(CompletionItem {
                        label: file_name.clone(),
                        value: full_val,
                        kind: if is_dir { "dir".to_string() } else { "file".to_string() },
                        description: Some(if is_dir { "Directory" } else { "File" }.to_string()),
                    });
                }
            }
        }

        // 3. If single word prefix (no slashes), check matching builtins and executables
        if !trimmed_prefix.contains('/') && !trimmed_prefix.contains('\\') && !trimmed_prefix.is_empty() {
            for builtin in SHELL_BUILTINS {
                if builtin.starts_with(trimmed_prefix) {
                    results.push(CompletionItem {
                        label: builtin.to_string(),
                        value: builtin.to_string(),
                        kind: "builtin".to_string(),
                        description: Some("Shell Builtin".to_string()),
                    });
                }
            }
        }

        // Sort: slash builtins first, then directories, then alphabetical
        results.sort_by(|a, b| {
            if a.kind == "builtin" && a.label.starts_with('/') && !(b.kind == "builtin" && b.label.starts_with('/')) {
                std::cmp::Ordering::Less
            } else if !(a.kind == "builtin" && a.label.starts_with('/')) && b.kind == "builtin" && b.label.starts_with('/') {
                std::cmp::Ordering::Greater
            } else if a.kind == "dir" && b.kind != "dir" {
                std::cmp::Ordering::Less
            } else if a.kind != "dir" && b.kind == "dir" {
                std::cmp::Ordering::Greater
            } else {
                a.label.to_lowercase().cmp(&b.label.to_lowercase())
            }
        });

        // Limit results to 50
        results.truncate(50);
        results
    }

    pub fn cancel(tab_id: &str) {
        if let Ok(mut stdins) = ACTIVE_STDIN.lock() {
            stdins.remove(tab_id);
        }
        if let Ok(mut procs) = ACTIVE_PROCESSES.lock() {
            if let Some(pid) = procs.remove(tab_id) {
                #[cfg(unix)]
                {
                    unsafe {
                        libc::kill(-(pid as i32), libc::SIGINT);
                        libc::kill(pid as i32, libc::SIGINT);
                        libc::kill(-(pid as i32), libc::SIGKILL);
                        libc::kill(pid as i32, libc::SIGKILL);
                    }
                }
                #[cfg(windows)]
                {
                    let _ = std::process::Command::new("taskkill")
                        .args(["/F", "/T", "/PID", &pid.to_string()])
                        .output();
                }
            }
        }
    }

    pub fn write_stdin(tab_id: &str, data: &str) -> Result<(), String> {
        if let Ok(mut stdins) = ACTIVE_STDIN.lock() {
            if let Some(stdin) = stdins.get_mut(tab_id) {
                stdin.write_all(data.as_bytes()).map_err(|e| e.to_string())?;
                let _ = stdin.flush();
                return Ok(());
            }
        }
        Ok(())
    }

    #[allow(dead_code)]
    pub fn exec(cwd_str: &str, raw_command: &str) -> Result<ShellExecResult, String> {
        Self::exec_with_app(None, None, cwd_str, raw_command)
    }

    #[allow(dead_code)]
    pub fn exec_with_tab(tab_id: Option<&str>, cwd_str: &str, raw_command: &str) -> Result<ShellExecResult, String> {
        Self::exec_with_app(None, tab_id, cwd_str, raw_command)
    }

    pub fn exec_with_app(
        app_handle: Option<&AppHandle>,
        tab_id: Option<&str>,
        cwd_str: &str,
        raw_command: &str,
    ) -> Result<ShellExecResult, String> {
        let trimmed_cmd = raw_command.trim();
        let current_dir = PtyManager::validate_working_dir(Some(cwd_str.to_string()));

        // Handle `cd` command directly
        if trimmed_cmd == "cd" || trimmed_cmd.starts_with("cd ") {
            let target_arg = trimmed_cmd.strip_prefix("cd").unwrap().trim();
            let target_path = if target_arg.is_empty() {
                dirs::home_dir().unwrap_or_else(|| PathBuf::from("."))
            } else {
                let expanded = Self::expand_path(target_arg);
                if expanded.is_absolute() {
                    expanded
                } else {
                    current_dir.join(expanded)
                }
            };

            let canonical = match target_path.canonicalize() {
                Ok(p) => p,
                Err(e) => {
                    return Ok(ShellExecResult {
                        stdout: String::new(),
                        stderr: format!("cd: {}: {}\n", e, target_arg),
                        exit_code: 1,
                        new_cwd: current_dir.to_string_lossy().to_string(),
                        display_cwd: Self::display_path(&current_dir),
                        git_branch: Self::get_git_branch(&current_dir.to_string_lossy()),
                    });
                }
            };

            if !canonical.is_dir() {
                return Ok(ShellExecResult {
                    stdout: String::new(),
                    stderr: format!("cd: not a directory: {}\n", target_arg),
                    exit_code: 1,
                    new_cwd: current_dir.to_string_lossy().to_string(),
                    display_cwd: Self::display_path(&current_dir),
                    git_branch: Self::get_git_branch(&current_dir.to_string_lossy()),
                });
            }

            let new_cwd_str = canonical.to_string_lossy().to_string();
            let display_cwd = Self::display_path(&canonical);
            let git_branch = Self::get_git_branch(&new_cwd_str);

            return Ok(ShellExecResult {
                stdout: String::new(),
                stderr: String::new(),
                exit_code: 0,
                new_cwd: new_cwd_str,
                display_cwd,
                git_branch,
            });
        }

        // Execute via system shell with fallback to valid executable shell
        let shell = PtyManager::find_valid_shell(None);

        let mut cmd = Command::new(&shell);
        #[cfg(unix)]
        {
            use std::os::unix::process::CommandExt;
            cmd.process_group(0);
            cmd.arg("-l").arg("-c").arg(trimmed_cmd);
        }
        #[cfg(windows)]
        {
            cmd.arg("/c").arg(trimmed_cmd);
        }

        cmd.current_dir(&current_dir);
        for (key, value) in std::env::vars() {
            cmd.env(key, value);
        }
        cmd.env("TERM", "xterm-256color");
        cmd.env("COLORTERM", "truecolor");
        cmd.env("TWOMINAL", "1");
        if std::env::var("LANG").is_err() {
            cmd.env("LANG", "en_US.UTF-8");
        }

        cmd.stdin(Stdio::piped());
        cmd.stdout(Stdio::piped());
        cmd.stderr(Stdio::piped());

        let mut child = match cmd.spawn() {
            Ok(c) => c,
            Err(e) => {
                let msg = match e.kind() {
                    std::io::ErrorKind::PermissionDenied => {
                        format!("twominal: permission denied executing '{}': {}. Check file permissions or shell executable access.\n", trimmed_cmd, e)
                    }
                    std::io::ErrorKind::NotFound => {
                        format!("twominal: command not found: {}\n", trimmed_cmd)
                    }
                    _ => format!("twominal: execution error: {}\n", e),
                };
                return Ok(ShellExecResult {
                    stdout: String::new(),
                    stderr: msg,
                    exit_code: 126,
                    new_cwd: current_dir.to_string_lossy().to_string(),
                    display_cwd: Self::display_path(&current_dir),
                    git_branch: Self::get_git_branch(&current_dir.to_string_lossy()),
                });
            }
        };

        let pid = child.id();
        let session_tab_key = tab_id.unwrap_or("default").to_string();
        if let Ok(mut procs) = ACTIVE_PROCESSES.lock() {
            procs.insert(session_tab_key.clone(), pid);
        }
        if let Some(stdin_handle) = child.stdin.take() {
            if let Ok(mut stdins) = ACTIVE_STDIN.lock() {
                stdins.insert(session_tab_key.clone(), stdin_handle);
            }
        }

        let stdout_handle = child.stdout.take();
        let stderr_handle = child.stderr.take();

        let app_handle_stdout = app_handle.cloned();
        let session_tab_key_stdout = session_tab_key.clone();
        let stdout_thread = thread::spawn(move || {
            let mut buf = Vec::new();
            if let Some(mut r) = stdout_handle {
                let mut chunk = [0u8; 4096];
                loop {
                    match r.read(&mut chunk) {
                        Ok(0) => break,
                        Ok(n) => {
                            buf.extend_from_slice(&chunk[..n]);
                            if let Some(ref app) = app_handle_stdout {
                                let data = String::from_utf8_lossy(&chunk[..n]).to_string();
                                let _ = app.emit(
                                    "shell-output",
                                    ShellOutputPayload {
                                        tab_id: session_tab_key_stdout.clone(),
                                        data,
                                    },
                                );
                            }
                        }
                        Err(_) => break,
                    }
                }
            }
            buf
        });

        let app_handle_stderr = app_handle.cloned();
        let session_tab_key_stderr = session_tab_key.clone();
        let stderr_thread = thread::spawn(move || {
            let mut buf = Vec::new();
            if let Some(mut r) = stderr_handle {
                let mut chunk = [0u8; 4096];
                loop {
                    match r.read(&mut chunk) {
                        Ok(0) => break,
                        Ok(n) => {
                            buf.extend_from_slice(&chunk[..n]);
                            if let Some(ref app) = app_handle_stderr {
                                let data = String::from_utf8_lossy(&chunk[..n]).to_string();
                                let _ = app.emit(
                                    "shell-output",
                                    ShellOutputPayload {
                                        tab_id: session_tab_key_stderr.clone(),
                                        data,
                                    },
                                );
                            }
                        }
                        Err(_) => break,
                    }
                }
            }
            buf
        });

        let status = child.wait();
        if let Ok(mut procs) = ACTIVE_PROCESSES.lock() {
            procs.remove(&session_tab_key);
        }
        if let Ok(mut stdins) = ACTIVE_STDIN.lock() {
            stdins.remove(&session_tab_key);
        }

        let stdout_bytes = stdout_thread.join().unwrap_or_default();
        let stderr_bytes = stderr_thread.join().unwrap_or_default();

        let stdout = String::from_utf8_lossy(&stdout_bytes).to_string();
        let stderr = String::from_utf8_lossy(&stderr_bytes).to_string();
        let exit_code = match status {
            Ok(st) => st.code().unwrap_or(if st.success() { 0 } else { 130 }),
            Err(_) => 1,
        };

        let new_cwd_str = current_dir.to_string_lossy().to_string();
        let display_cwd = Self::display_path(&current_dir);
        let git_branch = Self::get_git_branch(&new_cwd_str);

        Ok(ShellExecResult {
            stdout,
            stderr,
            exit_code,
            new_cwd: new_cwd_str,
            display_cwd,
            git_branch,
        })
    }

    fn history_file_path() -> PathBuf {
        dirs::home_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join(".twominal_history")
    }

    pub fn get_history(limit: usize) -> Vec<String> {
        let path = Self::history_file_path();
        if !path.exists() {
            return Vec::new();
        }

        if let Ok(file) = fs::File::open(path) {
            let reader = BufReader::new(file);
            let mut lines: Vec<String> = reader
                .lines()
                .map_while(Result::ok)
                .map(|l| l.trim().to_string())
                .filter(|l| !l.is_empty())
                .collect();

            if lines.len() > limit {
                lines = lines.split_off(lines.len() - limit);
            }
            return lines;
        }

        Vec::new()
    }

    pub fn add_history(command: &str) -> Result<(), String> {
        let trimmed = command.trim();
        if trimmed.is_empty() {
            return Ok(());
        }

        let path = Self::history_file_path();
        let mut file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(path)
            .map_err(|e| format!("Failed to open history file: {}", e))?;

        writeln!(file, "{}", trimmed)
            .map_err(|e| format!("Failed to write history entry: {}", e))?;

        Ok(())
    }

    pub fn get_git_branch(cwd_str: &str) -> Option<String> {
        let mut curr = Self::expand_path(cwd_str);
        if let Ok(canon) = curr.canonicalize() {
            curr = canon;
        }
        loop {
            let git_path = curr.join(".git");
            if git_path.is_dir() {
                let head_file = git_path.join("HEAD");
                if let Ok(content) = fs::read_to_string(head_file) {
                    let trimmed = content.trim();
                    if let Some(branch) = trimmed.strip_prefix("ref: refs/heads/") {
                        return Some(branch.to_string());
                    } else if !trimmed.is_empty() {
                        return Some(trimmed.chars().take(7).collect());
                    }
                }
                return None;
            } else if git_path.is_file() {
                if let Ok(content) = fs::read_to_string(&git_path) {
                    if let Some(gitdir_rel) = content.trim().strip_prefix("gitdir: ") {
                        let gitdir = if Path::new(gitdir_rel).is_absolute() {
                            PathBuf::from(gitdir_rel)
                        } else {
                            curr.join(gitdir_rel)
                        };
                        let head_file = gitdir.join("HEAD");
                        if let Ok(head_content) = fs::read_to_string(head_file) {
                            let trimmed = head_content.trim();
                            if let Some(branch) = trimmed.strip_prefix("ref: refs/heads/") {
                                return Some(branch.to_string());
                            } else if !trimmed.is_empty() {
                                return Some(trimmed.chars().take(7).collect());
                            }
                        }
                    }
                }
                return None;
            }

            if !curr.pop() {
                break;
            }
        }
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_check_builtins() {
        let res = FishEngine::check_command("cd");
        assert!(res.exists);
        assert_eq!(res.kind, "builtin");

        let res2 = FishEngine::check_command("echo");
        assert!(res2.exists);
        assert_eq!(res2.kind, "builtin");
    }

    #[test]
    fn test_check_slash_builtins() {
        let res = FishEngine::check_command("/help");
        assert!(res.exists);
        assert_eq!(res.kind, "builtin");

        let res2 = FishEngine::check_command("/theme");
        assert!(res2.exists);
        assert_eq!(res2.kind, "builtin");

        let res3 = FishEngine::check_command("/");
        assert!(res3.exists);
        assert_eq!(res3.kind, "builtin");
    }

    #[test]
    fn test_slash_completions() {
        let comps = FishEngine::get_completions(".", "/t");
        assert!(comps.iter().any(|c| c.label == "/theme"));
        assert!(comps.iter().any(|c| c.label == "/tabs"));

        let all_slash = FishEngine::get_completions(".", "/");
        assert!(all_slash.iter().any(|c| c.label == "/help"));
        assert!(all_slash.iter().any(|c| c.label == "/matrix"));
    }

    #[test]
    fn test_check_unknown_command() {
        let res = FishEngine::check_command("nonexistent_command_12345_xyz");
        assert!(!res.exists);
        assert_eq!(res.kind, "none");
    }

    #[test]
    fn test_expand_path() {
        let home = dirs::home_dir().unwrap_or_else(|| PathBuf::from("."));
        assert_eq!(FishEngine::expand_path("~"), home);
        assert_eq!(FishEngine::expand_path("~/test"), home.join("test"));
    }

    #[test]
    fn test_shell_exec_echo() {
        let res = FishEngine::exec(".", "echo 'Twominal Test'").expect("exec should succeed");
        assert!(res.stdout.contains("Twominal Test"));
        assert_eq!(res.exit_code, 0);
    }

    #[test]
    fn test_shell_exec_cd() {
        let home = dirs::home_dir().unwrap_or_else(|| PathBuf::from("."));
        let res = FishEngine::exec(".", "cd ~").expect("cd should succeed");
        assert_eq!(res.new_cwd, home.canonicalize().unwrap().to_string_lossy().to_string());
        assert_eq!(res.exit_code, 0);
    }

    #[test]
    fn test_history_persistence() {
        let test_cmd = format!("echo test_{}", std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_millis());
        FishEngine::add_history(&test_cmd).unwrap();
        let history = FishEngine::get_history(50);
        assert!(history.contains(&test_cmd));
    }

    #[test]
    fn test_path_completions() {
        let comps = FishEngine::get_completions(".", "Cargo");
        assert!(comps.iter().any(|c| c.label == "Cargo.toml"));

        let dir_comps = FishEngine::get_completions(".", "sr");
        assert!(dir_comps.iter().any(|c| c.label == "src" && c.kind == "dir" && c.value == "src/"));

        let sub_comps = FishEngine::get_completions(".", "src/f");
        assert!(sub_comps.iter().any(|c| c.label == "fish" && c.kind == "dir" && c.value == "src/fish/"));
    }

    #[test]
    fn test_exec_with_tab_and_cancel() {
        let tab_id = "test-tab-cancel";
        // Spawn a sleep command in a thread and cancel it
        let handle = std::thread::spawn(move || {
            FishEngine::exec_with_tab(Some(tab_id), ".", "sleep 5")
        });

        // Give it a moment to spawn
        std::thread::sleep(std::time::Duration::from_millis(100));
        FishEngine::cancel(tab_id);

        let res = handle.join().expect("thread join").expect("exec result");
        // Process should have been terminated quickly
        assert!(res.exit_code != 0 || res.stderr.contains("signal"));
    }
}

