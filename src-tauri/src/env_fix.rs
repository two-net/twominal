use std::path::Path;
use std::process::{Command, Stdio};
use std::sync::mpsc;
use std::thread;
use std::time::Duration;

/// Initializes and normalizes environment variables (especially PATH and tool roots like DOTNET_ROOT)
/// for macOS GUI app bundles and background processes where launchd provides only minimal PATH.
/// Runs with strict timeout and detached stdio to prevent any TTY hang or permission hang at startup.
pub fn init_environment() {
    #[cfg(unix)]
    {
        // 1. Try to query the user's login shell for their full environment
        // Run in background thread with a strict 1500ms timeout and Stdio::null()
        // so that if shell rc files or tools prompt for TTY input / keychain / network,
        // it NEVER blocks the main app thread or causes "Application Not Responding".
        let shell_candidates = [
            std::env::var("SHELL").unwrap_or_default(),
            "/bin/zsh".to_string(),
            "/bin/bash".to_string(),
            "/bin/sh".to_string(),
        ];

        let valid_shell = shell_candidates
            .into_iter()
            .find(|s| !s.is_empty() && Path::new(s).is_file())
            .unwrap_or_else(|| "/bin/sh".to_string());

        let (tx, rx) = mpsc::channel();
        let shell_for_thread = valid_shell.clone();

        thread::spawn(move || {
            let res = Command::new(&shell_for_thread)
                .args(["-l", "-c", "printenv"])
                .stdin(Stdio::null())
                .stderr(Stdio::null())
                .output();
            let _ = tx.send(res);
        });

        // Wait at most 1500ms
        if let Ok(Ok(output)) = rx.recv_timeout(Duration::from_millis(1500)) {
            if output.status.success() {
                let stdout = String::from_utf8_lossy(&output.stdout);
                for line in stdout.lines() {
                    if let Some((k, v)) = line.split_once('=') {
                        // Inherit essential developer & tool environment variables
                        if matches!(
                            k,
                            "PATH"
                                | "DOTNET_ROOT"
                                | "CARGO_HOME"
                                | "RUSTUP_HOME"
                                | "GOPATH"
                                | "GOROOT"
                                | "JAVA_HOME"
                                | "NVM_DIR"
                                | "NVM_BIN"
                                | "HOMEBREW_PREFIX"
                                | "HOMEBREW_CELLAR"
                                | "HOMEBREW_REPOSITORY"
                                | "LANG"
                                | "LC_ALL"
                                | "LC_CTYPE"
                        ) {
                            std::env::set_var(k, v);
                        }
                    }
                }
            }
        }

        // 2. Ensure standard UTF-8 locale is configured to avoid TTY / encoding crashes
        if std::env::var("LANG").is_err() {
            std::env::set_var("LANG", "en_US.UTF-8");
        }
        if std::env::var("LC_ALL").is_err() {
            std::env::set_var("LC_ALL", "en_US.UTF-8");
        }

        // 3. Ensure standard macOS / Unix binary paths are ALWAYS present in PATH
        let mut current_path = std::env::var("PATH").unwrap_or_default();
        let home = dirs::home_dir().map(|h| h.to_string_lossy().to_string()).unwrap_or_default();

        let standard_paths = [
            "/opt/homebrew/bin",
            "/opt/homebrew/sbin",
            "/usr/local/bin",
            "/usr/local/sbin",
            "/usr/bin",
            "/bin",
            "/usr/sbin",
            "/sbin",
            "/usr/local/share/dotnet",
            &format!("{}/.dotnet", home),
            &format!("{}/.dotnet/tools", home),
            &format!("{}/.cargo/bin", home),
            &format!("{}/.local/bin", home),
            &format!("{}/flutter/bin", home),
            &format!("{}/Library/Android/sdk/platform-tools", home),
        ];

        let existing_segments: std::collections::HashSet<&str> = current_path.split(':').collect();
        let mut prepend_segments: Vec<String> = Vec::new();

        for p in standard_paths {
            if !p.is_empty() && Path::new(p).exists() && !existing_segments.contains(p) {
                prepend_segments.push(p.to_string());
            }
        }

        if !prepend_segments.is_empty() {
            if current_path.is_empty() {
                current_path = prepend_segments.join(":");
            } else {
                current_path = format!("{}:{}", prepend_segments.join(":"), current_path);
            }
            std::env::set_var("PATH", current_path);
        }

        // 4. Ensure DOTNET_ROOT is explicitly set if dotnet exists and DOTNET_ROOT is unset
        if std::env::var("DOTNET_ROOT").is_err() {
            if Path::new("/usr/local/share/dotnet").exists() {
                std::env::set_var("DOTNET_ROOT", "/usr/local/share/dotnet");
            } else if !home.is_empty() && Path::new(&format!("{}/.dotnet", home)).exists() {
                std::env::set_var("DOTNET_ROOT", format!("{}/.dotnet", home));
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_init_environment() {
        init_environment();
        let path = std::env::var("PATH").unwrap_or_default();
        assert!(!path.is_empty());
    }
}

