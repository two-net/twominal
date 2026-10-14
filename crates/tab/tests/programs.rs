//! Real programs, shown by the Terminal Core (TERM-01, TERM-06).
//!
//! Each test starts a program on a pseudo-terminal through the Session, as a tab does, and
//! passes what it writes to a Terminal Core, which answers its queries. The Tab will do that
//! itself with #31; here the test does. A test whose program is not installed says so, and
//! passes.
//!
//! Every program runs with an environment of its own and a home folder of the test's own, so
//! nothing of the user's is read or written. These run on macOS and Linux.

#![cfg(unix)]

use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::mpsc::{sync_channel, Receiver};
use std::time::{Duration, Instant};
use std::{env, fs};

use tab::session::{Output, Session, Shell, Start};
use terminal_core::{Event, Size, TerminalCore};

/// How long a test waits for a program before it fails.
const PATIENCE: Duration = Duration::from_secs(10);

const SIZE: Size = Size { cols: 80, rows: 24 };

/// A folder for one test, removed when the test ends.
struct Folder(PathBuf);

impl Folder {
    fn new() -> Self {
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let number = NEXT.fetch_add(1, Ordering::Relaxed);
        let name = format!("twominal-programs-{}-{number}", std::process::id());
        let path = env::temp_dir().join(name);
        fs::create_dir_all(&path).unwrap();
        // Canonical, so a program sees the path the test gave it.
        Self(fs::canonicalize(path).unwrap())
    }
}

impl Drop for Folder {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

/// Where `program` is installed, or `None`, with a note, when it is not.
fn installed(program: &str) -> Option<String> {
    let path = env::var_os("PATH").unwrap_or_default();
    let found = env::split_paths(&path)
        .chain(["/bin", "/usr/bin", "/usr/local/bin", "/opt/homebrew/bin"].map(PathBuf::from))
        .map(|folder| folder.join(program))
        .find(|file| {
            fs::metadata(file)
                .is_ok_and(|file| file.is_file() && file.permissions().mode() & 0o111 != 0)
        });
    if found.is_none() {
        eprintln!("{program} is not installed, so it is not tested");
    }
    Some(found?.to_str()?.to_string())
}

/// A program on a terminal: its session, and the Terminal Core that shows it.
struct Terminal {
    session: Session,
    output: Receiver<Output>,
    core: TerminalCore,
}

impl Terminal {
    /// Runs `words` in `home`, which is also its home folder, with nothing in its environment
    /// but what a terminal gives, `program`'s folder on the `PATH`, and `environment`.
    fn run(home: &Path, program: &str, words: &[&str], environment: &[(&str, &str)]) -> Self {
        let folder = Path::new(program).parent().unwrap().to_str().unwrap();
        let mut command: Vec<String> =
            ["/usr/bin/env", "-i", "TERM=xterm-256color", "LANG=C.UTF-8"]
                .map(String::from)
                .to_vec();
        command.push(format!("HOME={}", home.display()));
        command.push(format!("PATH={folder}:/usr/bin:/bin:/usr/sbin:/sbin"));
        for folder in ["XDG_CONFIG_HOME", "XDG_DATA_HOME", "XDG_CACHE_HOME"] {
            command.push(format!(
                "{folder}={}/.{}",
                home.display(),
                folder.to_lowercase()
            ));
        }
        command.extend(
            environment
                .iter()
                .map(|(key, value)| format!("{key}={value}")),
        );
        command.extend(words.iter().map(|word| word.to_string()));

        let (sender, output) = sync_channel(64);
        let start = Start {
            shell: Shell::Command(command),
            directory: Some(home.to_path_buf()),
            size: SIZE,
            name: "Twominal".to_string(),
            version: "9.9.9".to_string(),
            integration: false,
            private: None,
        };
        let session = Session::start(start, move |output| {
            let _ = sender.send(output);
        })
        .expect("the program starts");
        Self {
            session,
            output,
            core: TerminalCore::new(SIZE, None),
        }
    }

    /// Shows what the program writes, and answers its queries, until `done` holds.
    fn until(&mut self, what: &str, done: impl Fn(&Self) -> bool) {
        let deadline = Instant::now() + PATIENCE;
        while !done(self) {
            let left = deadline.saturating_duration_since(Instant::now());
            match self.output.recv_timeout(left) {
                Ok(Output::Bytes(bytes)) => self.show(&bytes),
                other => panic!("waiting for {what}, got {other:?}, with\n{}", self.dump()),
            }
        }
    }

    /// Shows what the program writes until it ends.
    fn until_exit(&mut self) {
        let deadline = Instant::now() + PATIENCE;
        loop {
            let left = deadline.saturating_duration_since(Instant::now());
            match self.output.recv_timeout(left) {
                Ok(Output::Bytes(bytes)) => self.show(&bytes),
                Ok(Output::Exited(_)) => return,
                Err(error) => panic!("waiting for the end, got {error:?}, with\n{}", self.dump()),
            }
        }
    }

    fn show(&mut self, bytes: &[u8]) {
        for event in self.core.interpret(bytes) {
            if let Event::Reply(reply) = event {
                self.session.write(reply);
            }
        }
    }

    fn keys(&self, keys: &str) {
        self.session.write(keys.as_bytes().to_vec());
    }

    /// The rows of the screen being shown, without their trailing blanks.
    fn rows(&self) -> Vec<String> {
        self.core
            .screen()
            .rows()
            .iter()
            .map(|row| row.text().trim_end().to_string())
            .collect()
    }

    fn shows(&self, text: &str) -> bool {
        self.rows().iter().any(|row| row.contains(text))
    }

    fn alternate(&self) -> bool {
        self.core.modes().alternate_screen
    }

    /// The screen, for a test that failed.
    fn dump(&self) -> String {
        let rows = self.rows();
        let cursor = self.core.cursor();
        format!(
            "{}\ncursor at {}, {}; {:?}",
            rows.join("\n"),
            cursor.row,
            cursor.col,
            self.core.modes()
        )
    }
}

#[test]
fn fish_is_answered_and_runs_a_command() {
    let Some(fish) = installed("fish") else {
        return;
    };
    let home = Folder::new();
    let mut fish = Terminal::run(
        &home.0,
        &fish,
        &[
            &fish,
            "--no-config",
            "--private",
            "--init-command",
            "set -g fish_greeting; function fish_prompt; echo -n 'prompt> '; end",
        ],
        &[],
    );

    // fish asks for the primary device attributes and waits for the answer before it shows
    // its prompt, on the primary screen, ready for a bracketed paste.
    fish.until("the prompt", |fish| {
        fish.shows("prompt>") && fish.core.modes().bracketed_paste
    });
    assert!(!fish.alternate(), "{}", fish.dump());

    fish.keys("math 40 + 2\r");
    // What is typed is echoed, but only fish can write 42.
    fish.until("the answer and the next prompt", |fish| {
        let rows = fish.rows();
        rows.iter().any(|row| row == "42")
            && rows.iter().filter(|row| row.starts_with("prompt>")).count() == 2
    });
    // fish warns when a terminal does not answer, before the prompt it shows.
    assert!(
        !fish.shows("terminal-compatibility") && !fish.shows("waiting for"),
        "{}",
        fish.dump()
    );
}

#[test]
fn vim_shows_a_file_and_gives_the_screen_back() {
    let Some(vim) = installed("vim") else { return };
    let home = Folder::new();
    fs::write(home.0.join("notes.txt"), "alpha\nbeta\n").unwrap();
    let script =
        format!("printf 'before\\n'; '{vim}' -u NONE -N -n -i NONE notes.txt; printf 'after\\n'");
    let mut vim = Terminal::run(&home.0, &vim, &["/bin/sh", "-c", &script], &[]);

    vim.until("the file", |vim| {
        let rows = vim.rows();
        vim.alternate()
            && rows[..2] == ["alpha", "beta"]
            && rows[2..SIZE.rows as usize - 1].iter().all(|row| row == "~")
            && rows[SIZE.rows as usize - 1].contains("notes.txt")
    });

    vim.keys(":q!\r");
    vim.until_exit();
    assert!(!vim.alternate(), "{}", vim.dump());
    assert_eq!(vim.rows()[..3], ["before", "after", ""], "{}", vim.dump());
}

#[test]
fn vim_puts_its_cursor_where_the_core_puts_wide_and_combined_text() {
    let Some(vim) = installed("vim") else { return };
    let home = Folder::new();
    // Wide characters, combining marks as they came, and emoji (TERM-02). Had vim and the core
    // counted a character's cells differently, vim's cursor would miss the bar ending a line.
    let lines = ["日本語の文|", "cafe\u{301} nai\u{308}ve|", "😀 👍 x|"];
    fs::write(home.0.join("wide.txt"), lines.join("\n") + "\n").unwrap();
    let script = format!("'{vim}' -u NONE -N -n -i NONE wide.txt");
    let mut vim = Terminal::run(&home.0, &vim, &["/bin/sh", "-c", &script], &[]);
    vim.until("the file", |vim| {
        vim.alternate() && vim.rows()[..3] == lines
    });

    for (row, col) in [(0, 10), (1, 10), (2, 7)] {
        vim.keys(&format!("{}G$", row + 1));
        vim.until("the cursor on the bar", |vim| {
            let cursor = vim.core.cursor();
            let cell = vim.core.screen().rows()[row].cells()[col];
            (cursor.row, cursor.col) == (row as u16, col as u16) && cell.chars().eq(['|'])
        });
    }
    // vim deletes the wide character from the screen too.
    vim.keys("1G0x");
    vim.until("the first character gone", |vim| {
        vim.rows()[0] == "本語の文|"
    });

    vim.keys(":q!\r");
    vim.until_exit();
}

#[test]
fn less_pages_forward_and_back() {
    let Some(less) = installed("less") else {
        return;
    };
    let home = Folder::new();
    let lines: String = (1..=100).map(|line| format!("line {line}\n")).collect();
    fs::write(home.0.join("lines.txt"), lines).unwrap();
    let script = format!("printf 'before\\n'; '{less}' lines.txt; printf 'after\\n'");
    let mut less = Terminal::run(
        &home.0,
        &less,
        &["/bin/sh", "-c", &script],
        &[("LESSHISTFILE", "-")],
    );

    // A page is the screen but its last row, which is the prompt.
    let page = |first: usize| -> Vec<String> {
        (first..first + SIZE.rows as usize - 1)
            .map(|line| format!("line {line}"))
            .collect()
    };
    less.until("the first page", |less| {
        less.alternate() && less.rows()[..23] == page(1)[..]
    });
    less.keys(" ");
    less.until("the next page", |less| less.rows()[..23] == page(24)[..]);
    less.keys("k");
    less.until("a line back", |less| less.rows()[..23] == page(23)[..]);

    less.keys("q");
    less.until_exit();
    assert!(!less.alternate(), "{}", less.dump());
    assert_eq!(less.rows()[..3], ["before", "after", ""], "{}", less.dump());
}

/// git, unless it is macOS's stand-in that asks to install the developer tools.
fn git() -> Option<String> {
    let git = installed("git")?;
    let tools = !cfg!(target_os = "macos")
        || Command::new("xcode-select")
            .arg("-p")
            .output()
            .is_ok_and(|output| output.status.success());
    if !tools {
        eprintln!("the developer tools are not installed, so git is not tested");
    }
    tools.then_some(git)
}

#[test]
fn git_log_pages_through_less_on_the_primary_screen() {
    let (Some(git), Some(_less)) = (git(), installed("less")) else {
        return;
    };
    let home = Folder::new();
    let in_home = |words: &[&str]| {
        let status = Command::new(&git)
            .args(words)
            .current_dir(&home.0)
            .env_clear()
            .env("HOME", &home.0)
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .env("GIT_AUTHOR_NAME", "Twominal")
            .env("GIT_AUTHOR_EMAIL", "twominal@example.com")
            .env("GIT_AUTHOR_DATE", "2026-10-14T12:00:00Z")
            .env("GIT_COMMITTER_NAME", "Twominal")
            .env("GIT_COMMITTER_EMAIL", "twominal@example.com")
            .env("GIT_COMMITTER_DATE", "2026-10-14T12:00:00Z")
            .status()
            .unwrap();
        assert!(status.success(), "git {words:?}");
    };
    in_home(&["init", "--quiet"]);
    for commit in 1..=30 {
        in_home(&[
            "commit",
            "--quiet",
            "--allow-empty",
            "--message",
            &format!("commit {commit}"),
        ]);
    }

    // git runs less with LESS=FRX: no alternate screen, and the page stays when it ends.
    let mut log = Terminal::run(
        &home.0,
        &git,
        &[&git, "log"],
        &[("GIT_CONFIG_NOSYSTEM", "1")],
    );
    log.until("the newest commits and the prompt", |log| {
        let rows = log.rows();
        rows.iter().any(|row| row.trim() == "commit 30")
            && rows[SIZE.rows as usize - 1].starts_with(':')
    });
    assert!(!log.alternate(), "{}", log.dump());
    assert!(log.rows()[0].starts_with("commit "), "{}", log.dump());

    log.keys("q");
    log.until_exit();
    assert!(!log.alternate(), "{}", log.dump());
    assert!(log.shows("commit 30"), "{}", log.dump());
}

#[test]
fn top_runs_on_the_alternate_screen() {
    let Some(top) = installed("top") else { return };
    let home = Folder::new();
    let script = format!("printf 'before\\n'; '{top}'; printf 'after\\n'");
    let mut top = Terminal::run(&home.0, &top, &["/bin/sh", "-c", &script], &[]);

    top.until("the processes", |top| top.alternate() && top.shows("PID"));
    top.keys("q");
    top.until_exit();
    assert!(!top.alternate(), "{}", top.dump());
    assert_eq!(top.rows()[..3], ["before", "after", ""], "{}", top.dump());
}
