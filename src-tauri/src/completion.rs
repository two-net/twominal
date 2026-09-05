use std::collections::HashSet;
use std::env;
use std::ffi::OsStr;
use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use tauri::{State, Webview};
use uuid::Uuid;

use crate::error::CommandError;
use crate::terminal::TerminalSessionManager;

const MAX_PREFIX_BYTES: usize = 1_024;
const MAX_CWD_BYTES: usize = 4 * 1_024;
const MAX_PATH_DIRECTORIES: usize = 128;
const MAX_SCANNED_ENTRIES: usize = 8_192;
const MAX_COMPLETIONS: usize = 100;

#[derive(Debug, Clone, Copy, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum CompletionKind {
    Executable,
    Path,
    Environment,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CompletionRequest {
    kind: CompletionKind,
    prefix: String,
    cwd: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CompletionCandidate {
    value: String,
    display: String,
    kind: CandidateKind,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
enum CandidateKind {
    Executable,
    Directory,
    File,
    Environment,
}

#[tauri::command]
pub async fn completion_query(
    webview: Webview,
    state: State<'_, TerminalSessionManager>,
    session_id: String,
    request: CompletionRequest,
) -> Result<Vec<CompletionCandidate>, CommandError> {
    validate_prefix(&request.prefix)?;
    let session_id =
        Uuid::parse_str(&session_id).map_err(|_| CommandError::invalid_session_id())?;
    let initial_cwd = state.initial_cwd(webview.label(), session_id)?;
    let CompletionRequest { kind, prefix, cwd } = request;

    tauri::async_runtime::spawn_blocking(move || match kind {
        CompletionKind::Executable => Ok(complete_executables(
            &prefix,
            env::var_os("PATH").as_deref(),
        )),
        CompletionKind::Environment => Ok(complete_environment(&prefix)),
        CompletionKind::Path => {
            let cwd = validated_cwd(cwd.as_deref(), &initial_cwd)?;
            complete_paths(&prefix, &cwd)
        }
    })
    .await
    .map_err(|_| {
        completion_error(
            "completion_worker_failed",
            "The completion worker became unavailable.",
        )
    })?
}

fn validate_prefix(prefix: &str) -> Result<(), CommandError> {
    if prefix.len() > MAX_PREFIX_BYTES || prefix.chars().any(is_unsafe_display_character) {
        return Err(completion_error(
            "completion_invalid_prefix",
            "The completion prefix is invalid or too long.",
        ));
    }
    Ok(())
}

fn validated_cwd(reported: Option<&str>, initial: &Path) -> Result<PathBuf, CommandError> {
    let Some(reported) = reported else {
        return Ok(initial.to_path_buf());
    };
    if reported.is_empty()
        || reported.len() > MAX_CWD_BYTES
        || reported.chars().any(char::is_control)
    {
        return Err(completion_error(
            "completion_invalid_cwd",
            "The terminal working directory is invalid.",
        ));
    }

    let path = Path::new(reported);
    if !path.is_absolute() || !path.is_dir() {
        return Err(completion_error(
            "completion_invalid_cwd",
            "The terminal working directory is unavailable.",
        ));
    }
    Ok(path.to_path_buf())
}

fn complete_executables(prefix: &str, path: Option<&OsStr>) -> Vec<CompletionCandidate> {
    let mut seen = HashSet::new();
    let mut candidates = Vec::new();
    let mut scanned = 0usize;

    'directories: for directory in path
        .map(env::split_paths)
        .into_iter()
        .flatten()
        .take(MAX_PATH_DIRECTORIES)
    {
        let Ok(entries) = fs::read_dir(directory) else {
            continue;
        };
        for entry in entries.flatten() {
            scanned += 1;
            if scanned > MAX_SCANNED_ENTRIES {
                break 'directories;
            }
            let Some(name) = entry.file_name().to_str().map(str::to_owned) else {
                continue;
            };
            if !safe_name(&name)
                || !starts_with_platform(&name, prefix)
                || !is_command_file(&entry.path())
            {
                continue;
            }
            let identity = platform_identity(&name);
            if seen.insert(identity) {
                candidates.push(CompletionCandidate {
                    value: name.clone(),
                    display: name,
                    kind: CandidateKind::Executable,
                });
            }
        }
    }

    sort_and_limit(&mut candidates);
    candidates
}

fn complete_environment(prefix: &str) -> Vec<CompletionCandidate> {
    let mut candidates = env::vars_os()
        .filter_map(|(name, _)| name.to_str().map(str::to_owned))
        .filter(|name| safe_name(name) && starts_with_platform(name, prefix))
        .map(|name| CompletionCandidate {
            value: name.clone(),
            display: name,
            kind: CandidateKind::Environment,
        })
        .collect::<Vec<_>>();
    sort_and_limit(&mut candidates);
    candidates
}

fn complete_paths(prefix: &str, cwd: &Path) -> Result<Vec<CompletionCandidate>, CommandError> {
    let parts = path_parts(prefix);
    let directory = resolve_directory(&parts.directory, cwd);
    let entries = match fs::read_dir(directory) {
        Ok(entries) => entries,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(_) => {
            return Err(completion_error(
                "completion_read_failed",
                "The completion directory could not be read.",
            ))
        }
    };

    let include_hidden = parts.name_prefix.starts_with('.');
    let mut candidates = Vec::new();
    for entry in entries.flatten().take(MAX_SCANNED_ENTRIES) {
        let Some(name) = entry.file_name().to_str().map(str::to_owned) else {
            continue;
        };
        if !safe_name(&name)
            || (!include_hidden && name.starts_with('.'))
            || !starts_with_platform(&name, &parts.name_prefix)
        {
            continue;
        }

        let directory = entry.path().is_dir();
        let suffix = if directory { parts.separator } else { "" };
        let value = format!("{}{}{}", parts.directory, name, suffix);
        candidates.push(CompletionCandidate {
            value,
            display: format!("{name}{suffix}"),
            kind: if directory {
                CandidateKind::Directory
            } else {
                CandidateKind::File
            },
        });
    }

    sort_and_limit(&mut candidates);
    Ok(candidates)
}

struct PathParts {
    directory: String,
    name_prefix: String,
    separator: &'static str,
}

fn path_parts(prefix: &str) -> PathParts {
    let split = prefix
        .char_indices()
        .rev()
        .find(|(_, character)| *character == '/' || cfg!(windows) && *character == '\\');
    match split {
        Some((index, character)) => {
            let boundary = index + character.len_utf8();
            PathParts {
                directory: prefix[..boundary].to_owned(),
                name_prefix: prefix[boundary..].to_owned(),
                separator: if character == '\\' { "\\" } else { "/" },
            }
        }
        None => PathParts {
            directory: String::new(),
            name_prefix: prefix.to_owned(),
            separator: "/",
        },
    }
}

fn resolve_directory(directory: &str, cwd: &Path) -> PathBuf {
    if directory == "~" || directory.starts_with("~/") || directory.starts_with("~\\") {
        let relative = directory
            .strip_prefix("~/")
            .or_else(|| directory.strip_prefix("~\\"))
            .unwrap_or_default();
        return home_directory()
            .unwrap_or_else(|| cwd.to_path_buf())
            .join(relative);
    }

    let path = Path::new(directory);
    if path.is_absolute() {
        path.to_path_buf()
    } else {
        cwd.join(path)
    }
}

fn home_directory() -> Option<PathBuf> {
    #[cfg(windows)]
    {
        env::var_os("USERPROFILE").map(PathBuf::from)
    }
    #[cfg(not(windows))]
    {
        env::var_os("HOME").map(PathBuf::from)
    }
}

#[cfg(unix)]
fn is_command_file(path: &Path) -> bool {
    use std::os::unix::fs::PermissionsExt;
    fs::metadata(path)
        .map(|metadata| metadata.is_file() && metadata.permissions().mode() & 0o111 != 0)
        .unwrap_or(false)
}

#[cfg(windows)]
fn is_command_file(path: &Path) -> bool {
    if !path.is_file() {
        return false;
    }
    let extension = path
        .extension()
        .and_then(OsStr::to_str)
        .map(|value| format!(".{value}"));
    let Some(extension) = extension else {
        return false;
    };
    env::var("PATHEXT")
        .unwrap_or_else(|_| ".COM;.EXE;.BAT;.CMD".to_owned())
        .split(';')
        .any(|candidate| candidate.eq_ignore_ascii_case(&extension))
}

fn safe_name(name: &str) -> bool {
    !name.is_empty() && !name.chars().any(is_unsafe_display_character)
}

fn is_unsafe_display_character(character: char) -> bool {
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

#[cfg(windows)]
fn starts_with_platform(value: &str, prefix: &str) -> bool {
    value.to_lowercase().starts_with(&prefix.to_lowercase())
}

#[cfg(not(windows))]
fn starts_with_platform(value: &str, prefix: &str) -> bool {
    value.starts_with(prefix)
}

#[cfg(windows)]
fn platform_identity(value: &str) -> String {
    value.to_lowercase()
}

#[cfg(not(windows))]
fn platform_identity(value: &str) -> String {
    value.to_owned()
}

fn sort_and_limit(candidates: &mut Vec<CompletionCandidate>) {
    candidates.sort_unstable_by(|left, right| {
        left.display
            .to_lowercase()
            .cmp(&right.display.to_lowercase())
            .then_with(|| left.display.cmp(&right.display))
    });
    candidates.truncate(MAX_COMPLETIONS);
}

fn completion_error(code: &'static str, message: &'static str) -> CommandError {
    CommandError::new(code, message)
}

#[cfg(test)]
mod tests {
    use super::*;
    use uuid::Uuid;

    struct TestDirectory(PathBuf);

    impl TestDirectory {
        fn new() -> Self {
            let path = env::temp_dir().join(format!("twominal-completion-{}", Uuid::new_v4()));
            fs::create_dir(&path).unwrap();
            Self(path)
        }
    }

    impl Drop for TestDirectory {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn splits_path_prefixes_without_losing_the_typed_directory() {
        let parts = path_parts("src/ter");
        assert_eq!(parts.directory, "src/");
        assert_eq!(parts.name_prefix, "ter");
        assert_eq!(parts.separator, "/");

        let parts = path_parts("Cargo");
        assert_eq!(parts.directory, "");
        assert_eq!(parts.name_prefix, "Cargo");
    }

    #[test]
    fn completes_files_and_directories_and_hides_dotfiles_by_default() {
        let directory = TestDirectory::new();
        fs::write(directory.0.join("Cargo.toml"), b"").unwrap();
        fs::write(directory.0.join("README.md"), b"").unwrap();
        fs::write(directory.0.join(".secret"), b"").unwrap();
        fs::create_dir(directory.0.join("src")).unwrap();

        let candidates = complete_paths("", &directory.0).unwrap();
        assert_eq!(
            candidates
                .iter()
                .map(|candidate| candidate.value.as_str())
                .collect::<Vec<_>>(),
            ["Cargo.toml", "README.md", "src/"]
        );
        assert_eq!(
            complete_paths(".s", &directory.0).unwrap()[0].value,
            ".secret"
        );
    }

    #[cfg(unix)]
    #[test]
    fn executable_completion_only_returns_executable_files() {
        use std::os::unix::fs::PermissionsExt;

        let directory = TestDirectory::new();
        let executable = directory.0.join("twominal-tool");
        let plain = directory.0.join("twominal-not-executable");
        fs::write(&executable, b"#!/bin/sh\n").unwrap();
        fs::write(&plain, b"").unwrap();
        fs::set_permissions(&executable, fs::Permissions::from_mode(0o700)).unwrap();

        let candidates = complete_executables("twominal-", Some(directory.0.as_os_str()));
        assert_eq!(candidates.len(), 1);
        assert_eq!(candidates[0].value, "twominal-tool");
    }

    #[test]
    fn rejects_untrusted_prefixes_and_working_directories() {
        assert_eq!(
            validate_prefix("hello\nworld").unwrap_err().code,
            "completion_invalid_prefix"
        );
        assert_eq!(
            validate_prefix("hello\u{202e}world").unwrap_err().code,
            "completion_invalid_prefix"
        );
        assert_eq!(
            validated_cwd(Some("relative"), Path::new("/fallback"))
                .unwrap_err()
                .code,
            "completion_invalid_cwd"
        );
    }

    #[test]
    fn environment_completions_return_names_without_values_or_shell_syntax() {
        let candidates = complete_environment("");
        assert!(!candidates.is_empty());
        assert!(candidates.iter().all(|candidate| {
            candidate.value == candidate.display
                && !candidate.value.starts_with('$')
                && candidate.kind == CandidateKind::Environment
        }));
    }
}
