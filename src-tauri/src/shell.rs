use std::collections::HashSet;
use std::env;
use std::ffi::{OsStr, OsString};
#[cfg(any(unix, test))]
use std::fs;
use std::path::{Path, PathBuf};

use thiserror::Error;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ShellLaunch {
    pub executable: PathBuf,
    pub arguments: Vec<OsString>,
    pub environment: Vec<(OsString, OsString)>,
    pub display_name: String,
    pub cwd: PathBuf,
    pub shell_integration: bool,
}

#[derive(Clone, Debug)]
pub struct ShellIntegrationPaths {
    root: PathBuf,
}

impl ShellIntegrationPaths {
    pub fn new(root: PathBuf) -> Self {
        Self { root }
    }

    pub fn apply(&self, launch: &mut ShellLaunch) -> bool {
        let shell_name = launch
            .executable
            .file_stem()
            .and_then(OsStr::to_str)
            .unwrap_or_default();

        let applied = if shell_name.eq_ignore_ascii_case("zsh") {
            self.apply_zsh(launch)
        } else if shell_name.eq_ignore_ascii_case("bash") {
            self.apply_bash(launch)
        } else if shell_name.eq_ignore_ascii_case("pwsh")
            || shell_name.eq_ignore_ascii_case("powershell")
        {
            self.apply_powershell(launch)
        } else {
            false
        };

        launch.shell_integration = applied;
        applied
    }

    fn apply_zsh(&self, launch: &mut ShellLaunch) -> bool {
        let integration_script = self.root.join("twominal.zsh");
        let wrapper_directory = self.root.join("zsh");
        if !integration_script.is_file()
            || !wrapper_directory.join(".zshenv").is_file()
            || !wrapper_directory.join(".zprofile").is_file()
            || !wrapper_directory.join(".zshrc").is_file()
        {
            return false;
        }

        let login = launch
            .arguments
            .iter()
            .any(|argument| matches!(argument.to_str(), Some("-l" | "--login" | "-il" | "-li")));
        launch.arguments = vec![OsString::from(if login { "-il" } else { "-i" })];
        launch.environment.push((
            OsString::from("ZDOTDIR"),
            wrapper_directory.into_os_string(),
        ));
        launch.environment.push((
            OsString::from("TWOMINAL_INTEGRATION_SCRIPT"),
            integration_script.into_os_string(),
        ));
        if let Some(user_zdotdir) = env::var_os("ZDOTDIR")
            .filter(|value| !value.is_empty())
            .or_else(|| env::var_os("HOME").filter(|value| !value.is_empty()))
        {
            launch
                .environment
                .push((OsString::from("TWOMINAL_USER_ZDOTDIR"), user_zdotdir));
        }
        true
    }

    fn apply_bash(&self, launch: &mut ShellLaunch) -> bool {
        let integration_script = self.root.join("twominal.bash");
        if !integration_script.is_file() {
            return false;
        }

        let login = launch
            .arguments
            .iter()
            .any(|argument| matches!(argument.to_str(), Some("-l" | "--login" | "-il" | "-li")));
        launch.arguments = vec![
            OsString::from("--init-file"),
            integration_script.into_os_string(),
        ];
        if login {
            launch
                .environment
                .push((OsString::from("TWOMINAL_BASH_LOGIN"), OsString::from("1")));
        }
        true
    }

    fn apply_powershell(&self, launch: &mut ShellLaunch) -> bool {
        let integration_script = self.root.join("twominal.ps1");
        if !integration_script.is_file() {
            return false;
        }

        launch.environment.push((
            OsString::from("TWOMINAL_INTEGRATION_SCRIPT"),
            integration_script.into_os_string(),
        ));
        if !launch
            .arguments
            .iter()
            .any(|argument| argument.eq_ignore_ascii_case("-NoExit"))
        {
            launch.arguments.push(OsString::from("-NoExit"));
        }
        if !launch
            .arguments
            .iter()
            .any(|argument| argument.eq_ignore_ascii_case("-NoProfile"))
        {
            launch.arguments.push(OsString::from("-NoProfile"));
        }
        launch.arguments.extend([
            OsString::from("-Command"),
            OsString::from(". $env:TWOMINAL_INTEGRATION_SCRIPT"),
        ]);
        true
    }
}

#[derive(Debug, Error)]
pub enum ShellDetectionError {
    #[error("no supported executable shell was found")]
    NotFound,
    #[error("no usable startup directory was found")]
    NoStartupDirectory,
}

pub fn detect_default_shell() -> Result<ShellLaunch, ShellDetectionError> {
    let executable = detect_shell_executable().ok_or(ShellDetectionError::NotFound)?;
    let cwd = startup_directory().ok_or(ShellDetectionError::NoStartupDirectory)?;
    let display_name = shell_display_name(&executable);

    Ok(ShellLaunch {
        arguments: shell_arguments(&executable),
        environment: Vec::new(),
        executable,
        display_name,
        cwd,
        shell_integration: false,
    })
}

#[cfg(unix)]
fn detect_shell_executable() -> Option<PathBuf> {
    let mut candidates = Vec::new();
    if let Some(shell) = env::var_os("SHELL").filter(|value| !value.is_empty()) {
        candidates.push(PathBuf::from(shell));
    }

    if let Ok(contents) = fs::read_to_string("/etc/shells") {
        candidates.extend(parse_unix_shells(&contents));
    }
    candidates.push(PathBuf::from("/bin/sh"));

    select_first_executable(candidates, env::var_os("PATH").as_deref(), is_executable)
}

#[cfg(windows)]
fn detect_shell_executable() -> Option<PathBuf> {
    let path = env::var_os("PATH");
    let mut candidates = vec![PathBuf::from("pwsh.exe")];

    if let Some(system_root) = env::var_os("SystemRoot") {
        candidates.push(
            PathBuf::from(system_root)
                .join("System32")
                .join("WindowsPowerShell")
                .join("v1.0")
                .join("powershell.exe"),
        );
    }
    if let Some(comspec) = env::var_os("COMSPEC").filter(|value| !value.is_empty()) {
        candidates.push(PathBuf::from(comspec));
    }
    candidates.push(PathBuf::from("cmd.exe"));

    select_first_executable(candidates, path.as_deref(), is_executable)
}

#[cfg(not(any(unix, windows)))]
fn detect_shell_executable() -> Option<PathBuf> {
    None
}

fn select_first_executable<I, F>(
    candidates: I,
    path: Option<&OsStr>,
    mut executable: F,
) -> Option<PathBuf>
where
    I: IntoIterator<Item = PathBuf>,
    F: FnMut(&Path) -> bool,
{
    let mut seen = HashSet::new();

    for candidate in candidates {
        for resolved in resolve_candidate(&candidate, path) {
            if is_fish_executable(&resolved) {
                continue;
            }
            let identity = normalized_identity(&resolved);
            if seen.insert(identity) && executable(&resolved) {
                return Some(resolved);
            }
        }
    }
    None
}

fn is_fish_executable(path: &Path) -> bool {
    path.file_stem()
        .and_then(OsStr::to_str)
        .map(|name| name.eq_ignore_ascii_case("fish"))
        .unwrap_or(false)
}

fn resolve_candidate(candidate: &Path, path: Option<&OsStr>) -> Vec<PathBuf> {
    if candidate.is_absolute() || candidate.components().count() > 1 {
        return vec![candidate.to_path_buf()];
    }

    path.map(env::split_paths)
        .into_iter()
        .flatten()
        .map(|directory| directory.join(candidate))
        .collect()
}

#[cfg(windows)]
fn normalized_identity(path: &Path) -> OsString {
    path.as_os_str().to_string_lossy().to_lowercase().into()
}

#[cfg(not(windows))]
fn normalized_identity(path: &Path) -> OsString {
    path.as_os_str().to_owned()
}

#[cfg(unix)]
fn is_executable(path: &Path) -> bool {
    use std::os::unix::fs::PermissionsExt;

    fs::metadata(path)
        .map(|metadata| metadata.is_file() && metadata.permissions().mode() & 0o111 != 0)
        .unwrap_or(false)
}

#[cfg(windows)]
fn is_executable(path: &Path) -> bool {
    path.is_file()
}

#[cfg(unix)]
fn parse_unix_shells(contents: &str) -> Vec<PathBuf> {
    contents
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty() && !line.starts_with('#'))
        .map(PathBuf::from)
        .filter(|path| path.is_absolute())
        .collect()
}

fn startup_directory() -> Option<PathBuf> {
    home_directory()
        .filter(|path| path.is_dir())
        .or_else(|| env::current_dir().ok().filter(|path| path.is_dir()))
}

#[cfg(unix)]
fn home_directory() -> Option<PathBuf> {
    env::var_os("HOME")
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
}

#[cfg(windows)]
fn home_directory() -> Option<PathBuf> {
    if let Some(profile) = env::var_os("USERPROFILE").filter(|value| !value.is_empty()) {
        return Some(PathBuf::from(profile));
    }

    let drive = env::var_os("HOMEDRIVE")?;
    let path = env::var_os("HOMEPATH")?;
    let mut home = PathBuf::from(drive);
    home.push(path);
    Some(home)
}

#[cfg(not(any(unix, windows)))]
fn home_directory() -> Option<PathBuf> {
    None
}

fn shell_display_name(executable: &Path) -> String {
    executable
        .file_stem()
        .or_else(|| executable.file_name())
        .unwrap_or_else(|| OsStr::new("shell"))
        .to_string_lossy()
        .into_owned()
}

fn shell_arguments(executable: &Path) -> Vec<OsString> {
    let name = executable
        .file_stem()
        .and_then(OsStr::to_str)
        .unwrap_or_default();

    #[cfg(target_os = "macos")]
    if name.eq_ignore_ascii_case("zsh") || name.eq_ignore_ascii_case("bash") {
        return vec![OsString::from("-l")];
    }

    #[cfg(windows)]
    if name.eq_ignore_ascii_case("pwsh") || name.eq_ignore_ascii_case("powershell") {
        return vec![OsString::from("-NoLogo")];
    }

    let _ = name;
    Vec::new()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(unix)]
    #[test]
    fn parses_only_absolute_non_comment_shell_entries() {
        let shells = parse_unix_shells(
            "# approved shells\n\n/bin/zsh\n  /usr/local/bin/bash  \nrelative-shell\n",
        );

        assert_eq!(
            shells,
            vec![
                PathBuf::from("/bin/zsh"),
                PathBuf::from("/usr/local/bin/bash")
            ]
        );
    }

    #[test]
    fn preserves_candidate_order_and_skips_non_executables() {
        let candidates = vec![PathBuf::from("/one"), PathBuf::from("/two")];
        let selected = select_first_executable(candidates, None, |path| path == Path::new("/two"));

        assert_eq!(selected, Some(PathBuf::from("/two")));
    }

    #[test]
    fn resolves_bare_programs_against_path_in_order() {
        let joined = env::join_paths([PathBuf::from("/first"), PathBuf::from("/second")]).unwrap();
        let selected = select_first_executable(
            vec![PathBuf::from("twominal-test-shell")],
            Some(joined.as_os_str()),
            |path| path == Path::new("/second/twominal-test-shell"),
        );

        assert_eq!(selected, Some(PathBuf::from("/second/twominal-test-shell")));
    }

    #[test]
    fn deduplicates_candidates_before_probing() {
        let mut probes = Vec::new();
        let selected = select_first_executable(
            vec![PathBuf::from("/same"), PathBuf::from("/same")],
            None,
            |path| {
                probes.push(path.to_path_buf());
                false
            },
        );

        assert_eq!(selected, None);
        assert_eq!(probes, vec![PathBuf::from("/same")]);
    }

    #[test]
    fn derives_a_readable_shell_name() {
        assert_eq!(shell_display_name(Path::new("/bin/zsh")), "zsh");
        assert_eq!(shell_display_name(Path::new("cmd.exe")), "cmd");
    }

    #[test]
    fn leaves_unknown_shell_arguments_untouched() {
        assert!(shell_arguments(Path::new("/opt/bin/custom-shell")).is_empty());
    }

    #[test]
    fn never_selects_fish_even_when_it_is_executable() {
        let selected = select_first_executable(
            vec![PathBuf::from("/bin/fish"), PathBuf::from("/bin/zsh")],
            None,
            |_| true,
        );

        assert_eq!(selected, Some(PathBuf::from("/bin/zsh")));
    }

    #[test]
    fn injects_only_complete_supported_shell_integrations() {
        let directory = IntegrationTestDirectory::new();
        let integration = ShellIntegrationPaths::new(directory.path.clone());

        let mut zsh = test_launch("/bin/zsh", vec![OsString::from("-l")]);
        assert!(integration.apply(&mut zsh));
        assert_eq!(zsh.arguments, [OsString::from("-il")]);
        assert!(zsh.shell_integration);
        assert!(zsh
            .environment
            .iter()
            .any(|(name, _)| name == "TWOMINAL_INTEGRATION_SCRIPT"));

        let mut bash = test_launch("/bin/bash", vec![OsString::from("-l")]);
        assert!(integration.apply(&mut bash));
        assert_eq!(bash.arguments[0], "--init-file");
        assert!(bash
            .environment
            .iter()
            .any(|(name, value)| name == "TWOMINAL_BASH_LOGIN" && value == "1"));

        let mut powershell = test_launch("C:/PowerShell/pwsh.exe", Vec::new());
        assert!(integration.apply(&mut powershell));
        assert!(powershell.arguments.contains(&OsString::from("-NoProfile")));
        assert!(powershell.arguments.contains(&OsString::from("-NoExit")));
        assert!(powershell
            .environment
            .iter()
            .any(|(name, _)| name == "TWOMINAL_INTEGRATION_SCRIPT"));

        let mut unknown = test_launch("/bin/nushell", Vec::new());
        assert!(!integration.apply(&mut unknown));
        assert!(!unknown.shell_integration);
        assert!(unknown.arguments.is_empty());
    }

    #[test]
    fn missing_integration_assets_never_change_the_shell_launch() {
        let integration = ShellIntegrationPaths::new(PathBuf::from("/missing/twominal-assets"));
        let original = test_launch("/bin/zsh", vec![OsString::from("-l")]);
        let mut launch = original.clone();

        assert!(!integration.apply(&mut launch));
        assert_eq!(launch, original);
    }

    fn test_launch(executable: &str, arguments: Vec<OsString>) -> ShellLaunch {
        ShellLaunch {
            executable: PathBuf::from(executable),
            arguments,
            environment: Vec::new(),
            display_name: Path::new(executable)
                .file_stem()
                .unwrap()
                .to_string_lossy()
                .into_owned(),
            cwd: PathBuf::from("/tmp"),
            shell_integration: false,
        }
    }

    struct IntegrationTestDirectory {
        path: PathBuf,
    }

    impl IntegrationTestDirectory {
        fn new() -> Self {
            let path =
                env::temp_dir().join(format!("twominal-integration-{}", uuid::Uuid::new_v4()));
            fs::create_dir_all(path.join("zsh")).unwrap();
            for relative in [
                "twominal.zsh",
                "twominal.bash",
                "twominal.ps1",
                "zsh/.zshenv",
                "zsh/.zprofile",
                "zsh/.zshrc",
            ] {
                fs::write(path.join(relative), b"# test\n").unwrap();
            }
            Self { path }
        }
    }

    impl Drop for IntegrationTestDirectory {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.path);
        }
    }
}
