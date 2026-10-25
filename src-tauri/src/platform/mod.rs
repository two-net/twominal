//! Thin platform layer: the only place that talks to the OS directly (POR-02).

use std::collections::HashMap;
use std::io;
use std::path::PathBuf;

use alacritty_terminal::event::WindowSize;
use alacritty_terminal::tty::{self, Options, Pty, Shell};

#[cfg(target_os = "macos")]
mod macos;
#[cfg(target_os = "macos")]
pub use macos::{foreground_pgid, process_cwd, process_name};

#[cfg(not(target_os = "macos"))]
mod fallback {
    use std::os::fd::RawFd;
    use std::path::PathBuf;

    pub fn foreground_pgid(_fd: RawFd) -> Option<i32> {
        None
    }

    pub fn process_name(_pid: i32) -> Option<String> {
        None
    }

    pub fn process_cwd(_pid: i32) -> Option<PathBuf> {
        None
    }
}
#[cfg(not(target_os = "macos"))]
pub use fallback::{foreground_pgid, process_cwd, process_name};

/// What a new pseudo-terminal should run.
pub struct PtySpec {
    /// Program and arguments to run instead of the user's login shell.
    pub shell: Option<(String, Vec<String>)>,
    pub cwd: PathBuf,
    pub size: WindowSize,
    pub app_version: String,
}

/// Starts the shell on a new pseudo-terminal with the SI-02 environment.
///
/// `window_id` only feeds alacritty's `ALACRITTY_WINDOW_ID`/`WINDOWID`, which it always sets.
pub fn spawn_pty(spec: PtySpec, window_id: u64) -> io::Result<Pty> {
    let options = Options {
        shell: spec.shell.map(|(program, args)| Shell::new(program, args)),
        working_directory: Some(spec.cwd),
        drain_on_exit: true,
        env: shell_env(&spec.app_version, |key| std::env::var_os(key).is_some()),
    };
    tty::new(&options, spec.size, window_id)
}

fn shell_env(app_version: &str, is_set: impl Fn(&str) -> bool) -> HashMap<String, String> {
    let mut env = HashMap::from([
        ("TERM".into(), "xterm-256color".into()),
        ("COLORTERM".into(), "truecolor".into()),
        ("TERM_PROGRAM".into(), "Twominal".into()),
        ("TERM_PROGRAM_VERSION".into(), app_version.into()),
    ]);
    if !["LANG", "LC_ALL", "LC_CTYPE"].into_iter().any(is_set) {
        env.insert("LANG".into(), "en_US.UTF-8".into());
    }
    env
}

/// The user's home directory.
pub fn home_dir() -> PathBuf {
    std::env::var_os("HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("/"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shell_env_follows_si_02() {
        let env = shell_env("1.2.3", |_| false);
        assert_eq!(env["TERM"], "xterm-256color");
        assert_eq!(env["COLORTERM"], "truecolor");
        assert_eq!(env["TERM_PROGRAM"], "Twominal");
        assert_eq!(env["TERM_PROGRAM_VERSION"], "1.2.3");
        assert_eq!(env["LANG"], "en_US.UTF-8");
    }

    #[test]
    fn existing_locale_is_kept() {
        let env = shell_env("1.2.3", |key| key == "LC_CTYPE");
        assert!(!env.contains_key("LANG"));
    }
}
