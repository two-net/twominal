//! Window and tab titles (UI-03).
//!
//! The default title shows the current directory (home shortened to `~`) and the foreground
//! program. A program may override it with OSC 0/2; the override lasts until it is reset, or until
//! the process group that set it leaves the foreground.

use std::path::{Path, PathBuf};

use serde::Serialize;

/// Longest title we keep from a program, in characters.
const MAX_TITLE_CHARS: usize = 256;

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize)]
pub struct Titles {
    pub window: String,
    pub tab: String,
}

#[derive(Debug, Default)]
pub struct TitleState {
    /// A program-set title and the foreground process group that set it.
    program_title: Option<(String, Option<i32>)>,
    pgid: Option<i32>,
    program: Option<String>,
    cwd: Option<PathBuf>,
}

impl TitleState {
    /// Records an OSC 0/2 title. `pgid` is the foreground group when the sequence was handled.
    /// An empty title resets to the default.
    pub fn set_program_title(&mut self, title: &str, pgid: Option<i32>) {
        let title = sanitize(title);
        self.program_title = (!title.is_empty()).then_some((title, pgid));
    }

    pub fn reset_program_title(&mut self) {
        self.program_title = None;
    }

    /// Updates the foreground process and its directory. An unreadable directory (`None`) keeps
    /// the last known one.
    pub fn update_process(
        &mut self,
        pgid: Option<i32>,
        program: Option<String>,
        cwd: Option<PathBuf>,
    ) {
        if let Some((_, owner)) = &self.program_title {
            if *owner != pgid {
                self.program_title = None;
            }
        }
        self.pgid = pgid;
        if program.is_some() {
            self.program = program;
        }
        if cwd.is_some() {
            self.cwd = cwd;
        }
    }

    /// The last known working directory of the foreground process.
    pub fn cwd(&self) -> Option<&Path> {
        self.cwd.as_deref()
    }

    pub fn compose(&self, home: &Path, fallback: &str) -> Titles {
        if let Some((title, _)) = &self.program_title {
            return Titles {
                window: title.clone(),
                tab: title.clone(),
            };
        }

        let dir = self.cwd.as_deref().map(|cwd| shorten_home(cwd, home));
        let short_dir = dir.as_deref().map(last_component);
        let join = |dir: Option<&str>| match (dir, self.program.as_deref()) {
            (Some(dir), Some(program)) => format!("{dir} — {program}"),
            (Some(dir), None) => dir.to_owned(),
            (None, Some(program)) => program.to_owned(),
            (None, None) => fallback.to_owned(),
        };
        Titles {
            window: join(dir.as_deref()),
            tab: join(short_dir),
        }
    }
}

/// `/Users/me/src` → `~/src`, `/Users/me` → `~`; other paths are unchanged.
fn shorten_home(path: &Path, home: &Path) -> String {
    match path.strip_prefix(home) {
        Ok(rest) if home != Path::new("/") => {
            if rest.as_os_str().is_empty() {
                "~".to_owned()
            } else {
                format!("~/{}", rest.display())
            }
        }
        _ => path.display().to_string(),
    }
}

/// `~/repos/twominal` → `twominal`; `~` and `/` stay as they are.
fn last_component(dir: &str) -> &str {
    match dir.rsplit_once('/') {
        Some((_, last)) if !last.is_empty() => last,
        _ => dir,
    }
}

/// Program-controlled text: drop control characters and cap the length (SEC-05).
fn sanitize(title: &str) -> String {
    title
        .chars()
        .filter(|c| !c.is_control())
        .take(MAX_TITLE_CHARS)
        .collect::<String>()
        .trim()
        .to_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    const HOME: &str = "/Users/me";

    fn state(pgid: i32, program: &str, cwd: &str) -> TitleState {
        let mut state = TitleState::default();
        state.update_process(Some(pgid), Some(program.into()), Some(cwd.into()));
        state
    }

    fn compose(state: &TitleState) -> Titles {
        state.compose(Path::new(HOME), "Twominal")
    }

    #[test]
    fn default_title_shows_directory_and_program() {
        let titles = compose(&state(10, "zsh", "/Users/me/repos/twominal"));
        assert_eq!(titles.window, "~/repos/twominal — zsh");
        assert_eq!(titles.tab, "twominal — zsh");
    }

    #[test]
    fn home_and_root_stay_readable() {
        assert_eq!(compose(&state(10, "zsh", HOME)).tab, "~ — zsh");
        assert_eq!(compose(&state(10, "zsh", "/")).window, "/ — zsh");
        assert_eq!(compose(&state(10, "zsh", "/tmp")).window, "/tmp — zsh");
        assert_eq!(
            compose(&state(10, "zsh", "/Users/meme")).window,
            "/Users/meme — zsh"
        );
    }

    #[test]
    fn nothing_known_falls_back_to_the_app_name() {
        assert_eq!(compose(&TitleState::default()).window, "Twominal");
    }

    #[test]
    fn program_title_overrides_until_its_process_leaves_the_foreground() {
        let mut state = state(20, "vim", HOME);
        state.set_program_title("main.rs - VIM", Some(20));
        assert_eq!(compose(&state).window, "main.rs - VIM");
        assert_eq!(compose(&state).tab, "main.rs - VIM");

        state.update_process(Some(20), Some("vim".into()), None);
        assert_eq!(compose(&state).window, "main.rs - VIM");

        state.update_process(Some(10), Some("zsh".into()), None);
        assert_eq!(compose(&state).window, "~ — zsh");
    }

    #[test]
    fn empty_title_and_reset_restore_the_default() {
        let mut state = state(10, "zsh", HOME);
        state.set_program_title("custom", Some(10));
        state.set_program_title("", Some(10));
        assert_eq!(compose(&state).window, "~ — zsh");

        state.set_program_title("custom", Some(10));
        state.reset_program_title();
        assert_eq!(compose(&state).window, "~ — zsh");
    }

    #[test]
    fn unreadable_cwd_keeps_the_last_known_one() {
        let mut state = state(10, "zsh", "/tmp");
        state.update_process(Some(30), Some("sudo".into()), None);
        assert_eq!(compose(&state).window, "/tmp — sudo");
    }

    #[test]
    fn program_titles_are_sanitized() {
        let mut state = TitleState::default();
        state.set_program_title("  a\u{1b}[31mb\u{7}c\n ", Some(1));
        assert_eq!(compose(&state).window, "a[31mbc");

        state.set_program_title(&"x".repeat(1000), Some(1));
        assert_eq!(compose(&state).window.chars().count(), MAX_TITLE_CHARS);
    }
}
