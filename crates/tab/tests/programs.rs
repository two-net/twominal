//! Real programs, shown by the Terminal Core, typed into through the Input Handler and drawn by
//! the Renderer (TERM-01 to TERM-06, TERM-08 to TERM-11, FONT-10).
//!
//! Each test starts a program on a pseudo-terminal through the Session, as a tab does, and
//! passes what it writes to a Terminal Core, which answers its queries. Keys, pastes, focus
//! changes, the mouse and the wheel go through the Input Handler, by the modes the program set,
//! as DOM events, on cells of 10 by 20 pixels. The Renderer turns the screen into frames, which
//! are read back as the webview reads them. The Tab will do that itself with #31; here the test
//! does. A test whose program is not installed says so, and passes.
//!
//! Every program runs with an environment of its own and a home folder of the test's own, so
//! nothing of the user's is read or written. These run on macOS and Linux.

#![cfg(unix)]

use std::iter::Peekable;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::str::Chars;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::mpsc::{sync_channel, Receiver};
use std::time::{Duration, Instant};
use std::{env, fs};

use tab::input::{
    Action, DeltaMode, Grid, InputEvent, InputHandler, KeyEvent, Modifiers, MouseEvent, MouseKind,
    WheelEvent,
};
use tab::render::{Measure, Palette, Renderer, Rgb};
use tab::session::{Output, Session, Shell, Start};
use terminal_core::{
    Color, Event, Keyboard, MouseFormat, MouseTracking, Size, Style, TerminalCore, Underline,
};

/// How long a test waits for a program before it fails.
const PATIENCE: Duration = Duration::from_secs(10);

const SIZE: Size = Size { cols: 80, rows: 24 };

/// The size of a cell, in CSS pixels.
const CELL: (f64, f64) = (10.0, 20.0);

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

/// The DOM event of a key going down: `key` is what it typed or its name, at the place a US
/// keyboard has it.
fn press(key: &str, modifiers: Held) -> InputEvent {
    let code = match key {
        " " => "Space".to_string(),
        "[" => "BracketLeft".to_string(),
        "-" => "Minus".to_string(),
        letter if letter.len() == 1 && letter.as_bytes()[0].is_ascii_alphabetic() => {
            format!("Key{}", letter.to_ascii_uppercase())
        }
        letter if letter.len() == 1 && letter.as_bytes()[0].is_ascii_digit() => {
            format!("Digit{letter}")
        }
        name => name.to_string(),
    };
    InputEvent::Key(KeyEvent {
        key: key.to_string(),
        code,
        modifiers: modifiers.0,
        ..KeyEvent::default()
    })
}

/// The DOM's event of the mouse's main button at the middle of the cell at `row` and `col`,
/// counted from 0, with `buttons` held after it.
fn mouse(kind: MouseKind, (row, col): (u16, u16), buttons: u16, modifiers: Held) -> InputEvent {
    InputEvent::Mouse(MouseEvent {
        kind,
        button: 0,
        buttons,
        detail: u32::from(kind != MouseKind::Move),
        x: (f64::from(col) + 0.5) * CELL.0,
        y: (f64::from(row) + 0.5) * CELL.1,
        modifiers: modifiers.0,
    })
}

/// The modifiers held for a key.
#[derive(Clone, Copy, Default)]
struct Held(Modifiers);

fn held() -> Held {
    Held::default()
}

impl Held {
    fn shift(mut self, on: bool) -> Self {
        self.0.shift = on;
        self
    }
    fn ctrl(mut self) -> Self {
        self.0.ctrl = true;
        self
    }
    fn alt(mut self) -> Self {
        self.0.alt = true;
        self
    }
}

/// A program on a terminal: its session, the Terminal Core that shows it and the Input
/// Handler that types into it.
struct Terminal {
    session: Session,
    output: Receiver<Output>,
    core: TerminalCore,
    input: InputHandler,
}

impl Terminal {
    /// Runs `words` in `home`, which is also its home folder, with nothing in its environment
    /// but what a terminal gives, `program`'s folder on the `PATH`, and `environment`.
    fn run(home: &Path, program: &str, words: &[&str], environment: &[(&str, &str)]) -> Self {
        Self::run_sized(home, program, words, environment, SIZE)
    }

    /// Runs `words` as `run` does, on a terminal of `size`.
    fn run_sized(
        home: &Path,
        program: &str,
        words: &[&str],
        environment: &[(&str, &str)],
        size: Size,
    ) -> Self {
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
            size,
            name: "Twominal".to_string(),
            version: "9.9.9".to_string(),
            integration: false,
            private: None,
        };
        let session = Session::start(start, move |output| {
            let _ = sender.send(output);
        })
        .expect("the program starts");
        let mut input = InputHandler::new();
        input.resize(Grid {
            size,
            cell_width: CELL.0,
            cell_height: CELL.1,
        });
        Self {
            session,
            output,
            core: TerminalCore::new(size, None),
            input,
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

    /// Passes `event` through the Input Handler, with the modes the program set, and writes
    /// what it sends.
    fn input(&mut self, event: InputEvent) {
        if let Some(Action::Send(bytes)) = self.input.handle(event, self.core.modes()) {
            self.session.write(bytes);
        }
    }

    /// The mouse's main button going down at the cell at `row` and `col`, from 0, and coming
    /// up where `to` is, through each cell between them on its row.
    fn drag(&mut self, (row, col): (u16, u16), to: (u16, u16), modifiers: Held) {
        self.input(mouse(MouseKind::Down, (row, col), 1, modifiers));
        let mut at = col;
        while at != to.1 {
            at = if at < to.1 { at + 1 } else { at - 1 };
            self.input(mouse(MouseKind::Move, (row, at), 1, modifiers));
        }
        self.input(mouse(MouseKind::Move, to, 1, modifiers));
        self.input(mouse(MouseKind::Up, to, 0, modifiers));
    }

    fn click(&mut self, at: (u16, u16), modifiers: Held) {
        self.drag(at, at, modifiers);
    }

    /// The wheel turned down `pixels`, or up when negative, over the top left cell.
    fn wheel(&mut self, pixels: f64) {
        self.input(InputEvent::Wheel(WheelEvent {
            delta_y: pixels,
            delta_mode: DeltaMode::Pixel,
            x: 5.0,
            y: 10.0,
            ..WheelEvent::default()
        }));
    }

    /// Types `text` a key at a time, each as the DOM reports it on a US keyboard.
    fn type_text(&mut self, text: &str) {
        for char in text.chars() {
            let shift = char.is_ascii_uppercase();
            self.input(press(&char.to_string(), held().shift(shift)));
        }
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
fn vim_draws_in_the_colors_and_attributes_it_asks_for() {
    let Some(vim) = installed("vim") else { return };
    let home = Folder::new();
    fs::write(home.0.join("notes.txt"), "alpha\nbeta\n").unwrap();
    // 24-bit colors and attributes (TERM-03, TERM-04). vim paints the background by erasing,
    // since xterm-256color says the terminal erases in the background in use (bce).
    let mut vim = Terminal::run(
        &home.0,
        &vim,
        &[
            &vim,
            "-u",
            "NONE",
            "-N",
            "-n",
            "-i",
            "NONE",
            "--cmd",
            "set termguicolors",
            "-c",
            "highlight Normal guifg=#c0c0c0 guibg=#203040",
            "-c",
            "highlight NonText guifg=#ff8000 cterm=bold,italic,underline gui=bold,italic,underline",
            "notes.txt",
        ],
        &[],
    );
    let normal = Style {
        foreground: Color::Rgb(0xc0, 0xc0, 0xc0),
        background: Color::Rgb(0x20, 0x30, 0x40),
        ..Style::default()
    };
    let tilde = Style {
        foreground: Color::Rgb(0xff, 0x80, 0x00),
        bold: true,
        italic: true,
        underline: Underline::Single,
        ..normal
    };
    vim.until("the file in its colors", |vim| {
        let rows = vim.core.screen().rows();
        let painted = rows.iter().all(|row| {
            row.cells()
                .iter()
                .all(|cell| cell.style().background == normal.background)
        });
        vim.alternate()
            && vim.rows()[..3] == ["alpha", "beta", "~"]
            && painted
            && *rows[0].cells()[0].style() == normal
            && *rows[2].cells()[0].style() == tilde
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
    // git writes the commit's line in yellow, and less passes it through (TERM-03).
    let commit = log.core.screen().rows()[0].cells()[0];
    assert_eq!(commit.style().foreground, Color::Palette(3), "{commit:?}");

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

// ---------------------------------------------------------------------------------------
// Typing into programs
// ---------------------------------------------------------------------------------------

#[test]
fn fish_key_reader_tells_apart_what_the_kitty_protocol_tells_apart() {
    let Some(reader) = installed("fish_key_reader") else {
        return;
    };
    let home = Folder::new();
    let mut reader = Terminal::run(&home.0, &reader, &[&reader, "--continuous"], &[]);

    // fish asks whether the protocol is there, and sets its flags when it is (TERM-09):
    // disambiguation and the alternate keys.
    reader.until("the protocol", |reader| {
        reader.core.modes().keyboard == Keyboard::new(5)
    });

    // Tab and Ctrl+I are one byte without the protocol, and so are Esc and the start of every
    // Alt key. Each is a key of its own here. Alt is on a key that types nothing, since on
    // macOS Option types characters.
    for (key, modifiers, bind) in [
        ("Tab", held(), "bind tab "),
        ("i", held().ctrl(), "bind ctrl-i "),
        ("Escape", held(), "bind escape "),
        ("Backspace", held().alt(), "bind alt-backspace "),
        ("-", held().ctrl(), "bind ctrl-minus "),
    ] {
        reader.input(press(key, modifiers));
        reader.until(bind, |reader| reader.shows(bind));
    }
}

#[test]
fn fish_edits_with_its_own_keys_through_the_kitty_protocol() {
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
    fish.until("the prompt, with the protocol on", |fish| {
        fish.shows("prompt>") && fish.core.modes().keyboard == Keyboard::new(5)
    });

    // Ctrl+W deletes a word, as fish's own key (USE-03), and Enter runs the line.
    fish.type_text("echo one two");
    fish.until("the line", |fish| fish.shows("prompt> echo one two"));
    fish.input(press("w", held().ctrl()));
    fish.until("the word gone", |fish| {
        fish.rows().iter().any(|row| row == "prompt> echo one")
    });
    fish.input(press("Enter", held()));
    fish.until("the output", |fish| {
        fish.rows().iter().any(|row| row == "one")
    });

    // fish turns the protocol off for the commands it runs, so cat reads its line and Ctrl+D
    // as xterm sends them, and on again at the next prompt. sh says when cat has the terminal.
    fish.type_text("sh -c 'echo ready; cat'");
    fish.input(press("Enter", held()));
    fish.until("cat", |fish| {
        fish.core.modes().keyboard.is_legacy() && fish.rows().iter().any(|row| row == "ready")
    });
    fish.type_text("hello");
    fish.input(press("Enter", held()));
    fish.until("the line, echoed and written back", |fish| {
        fish.rows().iter().filter(|row| *row == "hello").count() == 2
    });
    fish.input(press("d", held().ctrl()));
    fish.until("the next prompt, with the protocol on", |fish| {
        let prompts = fish
            .rows()
            .iter()
            .filter(|row| row.starts_with("prompt>"))
            .count();
        prompts == 3 && fish.core.modes().keyboard == Keyboard::new(5)
    });
}

#[test]
fn zsh_takes_a_bracketed_paste_into_its_line_without_running_it() {
    let Some(zsh) = installed("zsh") else { return };
    let home = Folder::new();
    let mut zsh = Terminal::run(&home.0, &zsh, &[&zsh, "-f", "-i"], &[("PS1", "prompt> ")]);
    zsh.until("the prompt, ready for a bracketed paste", |zsh| {
        zsh.shows("prompt>") && zsh.core.modes().bracketed_paste
    });

    // TERM-11. Had the paste's line break run the first line, its output would come before
    // the Z typed after the paste.
    zsh.input(InputEvent::Paste("echo one\necho two".to_string()));
    zsh.type_text("Z");
    zsh.until("the pasted lines in the line editor", |zsh| {
        zsh.shows("echo twoZ")
    });
    assert!(
        zsh.shows("prompt> echo one") && !zsh.rows().iter().any(|row| row == "one"),
        "{}",
        zsh.dump()
    );

    zsh.input(press("Backspace", held()));
    zsh.input(press("Enter", held()));
    zsh.until("both lines run", |zsh| {
        let rows = zsh.rows();
        rows.iter().any(|row| row == "one") && rows.iter().any(|row| row == "two")
    });
}

#[test]
fn vim_moves_with_the_cursor_keys_and_leaves_insert_mode_on_esc() {
    let Some(vim) = installed("vim") else { return };
    let home = Folder::new();
    fs::write(home.0.join("notes.txt"), "").unwrap();
    let script =
        format!("'{vim}' -u NONE -N -n -i NONE --cmd 'set ttimeout ttimeoutlen=10' notes.txt");
    let mut vim = Terminal::run(&home.0, &vim, &["/bin/sh", "-c", &script], &[]);

    // vim turns on the cursor keys' and the keypad's application modes (TERM-08), so Left
    // is SS3 D.
    vim.until("the file", |vim| {
        let modes = vim.core.modes();
        vim.alternate() && modes.application_cursor && modes.application_keypad
    });
    vim.type_text("iab");
    vim.input(press("ArrowLeft", held()));
    vim.type_text("X");
    vim.until("the text", |vim| vim.rows()[0] == "aXb");
    // Esc goes at once, alone, so vim leaves insert mode and takes the command after it.
    vim.input(press("Escape", held()));
    vim.type_text(":wq");
    vim.input(press("Enter", held()));
    vim.until_exit();
    assert_eq!(
        fs::read_to_string(home.0.join("notes.txt")).unwrap(),
        "aXb\n"
    );
}

#[test]
fn vim_hears_the_focus_come_and_go() {
    let Some(vim) = installed("vim") else { return };
    let home = Folder::new();
    let script = format!(
        "'{vim}' -u NONE -N -n -i NONE --cmd 'set ttimeout ttimeoutlen=10' \
         -c 'autocmd FocusLost * call setline(1, \"lost\")' \
         -c 'autocmd FocusGained * call setline(1, \"gained\")'"
    );
    let mut vim = Terminal::run(&home.0, &vim, &["/bin/sh", "-c", &script], &[]);

    // vim asks to be told of focus (TERM-11).
    vim.until("focus reporting", |vim| {
        vim.alternate() && vim.core.modes().focus
    });
    vim.input(InputEvent::Focus(false));
    vim.until("FocusLost", |vim| vim.rows()[0] == "lost");
    vim.input(InputEvent::Focus(true));
    vim.until("FocusGained", |vim| vim.rows()[0] == "gained");

    vim.type_text(":q!");
    vim.input(press("Enter", held()));
    vim.until_exit();
}

// ---------------------------------------------------------------------------------------
// The mouse and the wheel
// ---------------------------------------------------------------------------------------

/// vim editing `file` with the mouse on and Esc read at once.
fn vim_with_the_mouse(vim: &str, file: &str) -> String {
    format!("'{vim}' -u NONE -N -n -i NONE --cmd 'set mouse=a ttimeout ttimeoutlen=10' {file}")
}

impl Terminal {
    /// Whether the program asked for drags reported with SGR's extended coordinates.
    fn reports_sgr_drags(&self) -> bool {
        let modes = self.core.modes();
        modes.mouse == MouseTracking::Drag && modes.mouse_format == MouseFormat::Sgr
    }

    fn cursor_at(&self, row: u16, col: u16) -> bool {
        let cursor = self.core.cursor();
        (cursor.row, cursor.col) == (row, col)
    }
}

#[test]
fn vim_asks_for_sgr_reports_and_keeps_underline_colors() {
    let Some(vim) = installed("vim") else { return };
    let home = Folder::new();
    fs::write(home.0.join("notes.txt"), "alpha\n").unwrap();
    let script = vim_with_the_mouse(&vim, "notes.txt");
    let mut vim = Terminal::run(&home.0, &vim, &["/bin/sh", "-c", &script], &[]);

    // vim asks for SGR reports with drags (TERM-10). Since 9.0.1168 its own entry for xterm
    // says it may; before, DA2's version had to be 277 or more. And it keeps the code for
    // underline colors, which it empties when the version is from 95 to 2499 (TERM-05).
    vim.until("SGR reports", |vim| vim.reports_sgr_drags());
    vim.keys(":echo &ttymouse empty(&t_8u)\r");
    vim.until("what vim chose", |vim| {
        vim.rows()[SIZE.rows as usize - 1] == "sgr 0"
    });

    vim.keys("\x1b:q!\r");
    vim.until_exit();
}

#[test]
fn vim_moves_its_cursor_and_selects_with_the_mouse() {
    let Some(vim) = installed("vim") else { return };
    let home = Folder::new();
    fs::write(home.0.join("notes.txt"), "alpha\nbeta\ngamma\n").unwrap();
    let script = vim_with_the_mouse(&vim, "notes.txt");
    let mut vim = Terminal::run(&home.0, &vim, &["/bin/sh", "-c", &script], &[]);
    vim.until("SGR reports", |vim| {
        vim.reports_sgr_drags() && vim.rows()[1..3] == ["beta", "gamma"]
    });

    // A click puts the cursor there.
    vim.click((2, 3), held());
    vim.until("the cursor on the m", |vim| vim.cursor_at(2, 3));
    // A drag selects, and d deletes what it selected: "et".
    vim.drag((1, 1), (1, 2), held());
    vim.until("the cursor at the drag's end", |vim| vim.cursor_at(1, 2));
    vim.keys("d");
    vim.until("the selection gone", |vim| vim.rows()[1] == "ba");
    // With Shift the mouse is not vim's, so the cursor stays and x deletes the a.
    vim.click((2, 0), held().shift(true));
    vim.keys("x");
    vim.until("the a gone", |vim| vim.rows()[1] == "b");
    assert_eq!(vim.rows()[2], "gamma", "{}", vim.dump());

    vim.keys("\x1b:q!\r");
    vim.until_exit();
}

#[test]
fn vim_scrolls_with_the_wheel() {
    let Some(vim) = installed("vim") else { return };
    let home = Folder::new();
    let lines: String = (1..=100).map(|line| format!("line {line}\n")).collect();
    fs::write(home.0.join("lines.txt"), lines).unwrap();
    let script = vim_with_the_mouse(&vim, "lines.txt");
    let mut vim = Terminal::run(&home.0, &vim, &["/bin/sh", "-c", &script], &[]);
    vim.until("SGR reports", |vim| {
        vim.reports_sgr_drags() && vim.rows()[1] == "line 2"
    });

    // A notch of 100 pixels is one step, which vim scrolls 3 lines by ('mousescroll').
    vim.wheel(100.0);
    vim.until("3 lines down", |vim| vim.rows()[0] == "line 4");
    // A trackpad's few pixels at a time make a step once they make a row.
    for _ in 0..3 {
        vim.wheel(7.0);
    }
    vim.until("3 more", |vim| vim.rows()[0] == "line 7");
    vim.wheel(-100.0);
    vim.until("3 lines up", |vim| vim.rows()[0] == "line 4");

    vim.keys("\x1b:q!\r");
    vim.until_exit();
}

#[test]
fn vim_takes_a_click_past_column_223() {
    let Some(vim) = installed("vim") else { return };
    let home = Folder::new();
    // SGR's coordinates go on where the legacy format's stop (TERM-10).
    let line = format!("{}|{}", "-".repeat(250), "-".repeat(40));
    fs::write(home.0.join("wide.txt"), format!("{line}\n")).unwrap();
    let script = vim_with_the_mouse(&vim, "wide.txt");
    let size = Size {
        cols: 300,
        rows: 24,
    };
    let mut vim = Terminal::run_sized(&home.0, &vim, &["/bin/sh", "-c", &script], &[], size);
    vim.until("SGR reports and the wide line", |vim| {
        vim.reports_sgr_drags() && vim.rows()[0] == line
    });

    vim.click((0, 250), held());
    vim.until("the cursor on the bar", |vim| vim.cursor_at(0, 250));

    vim.keys("\x1b:q!\r");
    vim.until_exit();
}

#[test]
fn less_scrolls_with_the_wheel_through_the_cursor_keys() {
    let Some(less) = installed("less") else {
        return;
    };
    let home = Folder::new();
    let lines: String = (1..=100).map(|line| format!("line {line}\n")).collect();
    fs::write(home.0.join("lines.txt"), lines).unwrap();
    let mut less = Terminal::run(
        &home.0,
        &less,
        &[&less, "lines.txt"],
        &[("LESSHISTFILE", "-")],
    );
    let top = |less: &Terminal| less.rows()[0].clone();
    less.until("the first page", |less| {
        less.alternate() && top(less) == "line 1"
    });
    assert_eq!(less.core.modes().mouse, MouseTracking::Off);

    // less did not ask for the mouse, so the wheel is the cursor keys on the alternate screen,
    // a key a row, in the application mode less turned on (1007).
    less.wheel(60.0);
    less.until("3 lines down", |less| top(less) == "line 4");
    less.wheel(-20.0);
    less.until("a line up", |less| top(less) == "line 3");

    less.keys("q");
    less.until_exit();
}

#[test]
fn less_with_its_mouse_option_scrolls_by_reported_wheel_events() {
    let Some(less) = installed("less") else {
        return;
    };
    let home = Folder::new();
    let lines: String = (1..=100).map(|line| format!("line {line}\n")).collect();
    fs::write(home.0.join("lines.txt"), lines).unwrap();
    let mut less = Terminal::run(
        &home.0,
        &less,
        &[&less, "--mouse", "lines.txt"],
        &[("LESSHISTFILE", "-")],
    );
    let top = |less: &Terminal| less.rows()[0].clone();
    less.until("the first page, with the mouse reported", |less| {
        less.core.modes().mouse != MouseTracking::Off && top(less) == "line 1"
    });

    // Each report is a step of one line, as --wheel-lines has it: three rows turned at once
    // are one report.
    less.wheel(60.0);
    less.until("a line down", |less| top(less) == "line 2");
    less.wheel(20.0);
    less.until("another", |less| top(less) == "line 3");
    less.wheel(-100.0);
    less.until("a line up", |less| top(less) == "line 2");

    less.keys("q");
    less.until_exit();
}

// ---------------------------------------------------------------------------------------
// The Renderer
// ---------------------------------------------------------------------------------------

/// A palette whose ANSI colors are told apart at a glance.
fn palette() -> Palette {
    Palette {
        foreground: Rgb::new(200, 200, 200),
        background: Rgb::new(10, 10, 10),
        ansi: std::array::from_fn(|index| Rgb::new(index as u8, 0, 100 + index as u8)),
    }
}

/// What the webview measures of a terminal area of 80 by 24 cells of 10 by 20 pixels, at scale 1.
fn measure() -> Measure {
    Measure {
        scale: 1.0,
        font: "monospace".to_string(),
        font_size: 15.0,
        advance: 10.0,
        ascent: 15.0,
        descent: 5.0,
        width: 800.0,
        height: 480.0,
    }
}

/// A JSON value, as the webview reads a frame.
#[derive(Clone, Debug, PartialEq)]
enum Json {
    Null,
    Bool(bool),
    Number(f64),
    String(String),
    Array(Vec<Json>),
    Object(Vec<(String, Json)>),
}

impl Json {
    /// Reads `text`, which must be one JSON value and nothing more.
    fn read(text: &str) -> Self {
        let mut reader = Reader(text.chars().peekable());
        let value = reader.value();
        reader.space();
        assert_eq!(reader.0.next(), None, "text after the value in {text}");
        value
    }

    fn get(&self, key: &str) -> Option<&Json> {
        match self {
            Json::Object(entries) => entries
                .iter()
                .find(|(name, _)| name == key)
                .map(|(_, value)| value),
            _ => None,
        }
    }

    fn items(&self) -> &[Json] {
        match self {
            Json::Array(items) => items,
            other => panic!("{other:?} is not an array"),
        }
    }

    fn number(&self) -> f64 {
        match self {
            Json::Number(number) => *number,
            other => panic!("{other:?} is not a number"),
        }
    }

    fn text(&self) -> &str {
        match self {
            Json::String(text) => text,
            other => panic!("{other:?} is not a string"),
        }
    }
}

/// Reads JSON as `JSON.parse` does, but for the escapes no frame needs.
struct Reader<'a>(Peekable<Chars<'a>>);

impl Reader<'_> {
    fn space(&mut self) {
        while self
            .0
            .next_if(|char| matches!(char, ' ' | '\t' | '\n' | '\r'))
            .is_some()
        {}
    }

    fn expect(&mut self, word: &str) {
        for char in word.chars() {
            assert_eq!(self.0.next(), Some(char), "in {word}");
        }
    }

    fn value(&mut self) -> Json {
        self.space();
        match self.0.peek().copied() {
            Some('{') => {
                self.0.next();
                let mut entries = Vec::new();
                self.space();
                if self.0.next_if_eq(&'}').is_some() {
                    return Json::Object(entries);
                }
                loop {
                    self.space();
                    let key = self.string();
                    self.space();
                    self.expect(":");
                    entries.push((key, self.value()));
                    self.space();
                    match self.0.next() {
                        Some(',') => {}
                        Some('}') => return Json::Object(entries),
                        other => panic!("{other:?} in an object"),
                    }
                }
            }
            Some('[') => {
                self.0.next();
                let mut items = Vec::new();
                self.space();
                if self.0.next_if_eq(&']').is_some() {
                    return Json::Array(items);
                }
                loop {
                    items.push(self.value());
                    self.space();
                    match self.0.next() {
                        Some(',') => {}
                        Some(']') => return Json::Array(items),
                        other => panic!("{other:?} in an array"),
                    }
                }
            }
            Some('"') => Json::String(self.string()),
            Some('t') => {
                self.expect("true");
                Json::Bool(true)
            }
            Some('f') => {
                self.expect("false");
                Json::Bool(false)
            }
            Some('n') => {
                self.expect("null");
                Json::Null
            }
            _ => {
                let mut number = String::new();
                while let Some(char) = self
                    .0
                    .next_if(|char| matches!(char, '-' | '+' | '.' | 'e' | 'E' | '0'..='9'))
                {
                    number.push(char);
                }
                // What Rust reads but JSON does not: `.5`, and `05`.
                let leading = number.trim_start_matches('-');
                let zeros = leading.starts_with('0')
                    && leading[1..].starts_with(|c: char| c.is_ascii_digit());
                assert!(!(leading.starts_with('.') || zeros), "{number}");
                Json::Number(
                    number
                        .parse()
                        .unwrap_or_else(|_| panic!("{number:?} is no number")),
                )
            }
        }
    }

    fn string(&mut self) -> String {
        self.expect("\"");
        let mut text = String::new();
        loop {
            match self.0.next().expect("the string ends") {
                '"' => return text,
                '\\' => match self.0.next() {
                    Some('u') => {
                        let hex: String = (0..4).map(|_| self.0.next().unwrap()).collect();
                        let unit = u32::from_str_radix(&hex, 16).unwrap();
                        text.push(char::from_u32(unit).expect("a character, not half of one"));
                    }
                    Some(char @ ('"' | '\\' | '/')) => text.push(char),
                    other => panic!("\\{other:?} in a frame"),
                },
                char if char < ' ' => panic!("{char:?} is not escaped"),
                char => text.push(char),
            }
        }
    }
}

/// The colors a frame names, as the webview writes them.
fn hex(color: Rgb) -> String {
    format!("#{:02x}{:02x}{:02x}", color.red, color.green, color.blue)
}

/// The runs of `row` of `frame`: each one's column, text, count if it has one, color and flags.
/// A count of 0 is a run drawn as one so that ligatures form.
fn runs(frame: &Json, row: usize) -> Vec<(usize, String, Option<usize>, String, usize)> {
    let colors = frame.get("colors").unwrap().items();
    let styles = frame.get("styles").unwrap().items();
    let row = frame
        .get("rows")
        .unwrap()
        .items()
        .iter()
        .find(|found| found.items()[0].number() as usize == row);
    row.unwrap_or_else(|| panic!("no row {row:?} in {frame:?}"))
        .items()[2]
        .items()
        .iter()
        .map(|run| {
            let run = run.items();
            let style = styles[run[1].number() as usize].items();
            (
                run[0].number() as usize,
                run[2].text().to_string(),
                run.get(3).map(|cells| cells.number() as usize),
                colors[style[0].number() as usize].text().to_string(),
                style[1].number() as usize,
            )
        })
        .collect()
}

#[test]
fn the_renderer_draws_what_sh_prints() {
    let home = Folder::new();
    // SGR's colors of three kinds and attributes; box drawing, blocks and a powerline arrow;
    // a wide character, and a combining mark.
    let script = "printf '\x1b[31mred\x1b[0m \x1b[38;5;208morange\x1b[0m \x1b[38;2;1;2;3mrgb\x1b[0m \x1b[1;3;4mfat\x1b[0m\\n'; \
                  printf '┌─┬─┐ ╔═╦═╗ ▀▄█░ \u{E0B0} 界 e\u{301}!\\n'";
    let mut sh = Terminal::run(&home.0, "/bin/sh", &["/bin/sh", "-c", script], &[]);
    sh.until_exit();
    assert!(sh.shows("fat") && sh.shows("界"), "{}", sh.dump());

    let mut renderer = Renderer::new(palette());
    let layout = renderer.measure(&measure()).unwrap();
    assert_eq!(layout.size, SIZE);
    let frame = Json::read(&renderer.frame(&sh.core, "").unwrap().json());
    let full = frame.get("full").expect("a first frame with the layout");
    assert_eq!(
        full.get("cell"),
        Some(&Json::Array(vec![Json::Number(10.0), Json::Number(20.0)]))
    );
    assert_eq!(
        frame.get("rows").unwrap().items().len(),
        usize::from(SIZE.rows)
    );

    let foreground = hex(palette().foreground);
    assert_eq!(
        runs(&frame, 0),
        [
            (0, "red".to_string(), Some(0), hex(palette().ansi[1]), 0),
            (4, "orange".to_string(), Some(0), "#ff8700".to_string(), 0),
            (11, "rgb".to_string(), Some(0), "#010203".to_string(), 0),
            // Bold, italic and underlined: 1, 2 and a single underline's 8.
            (15, "fat".to_string(), Some(0), foreground.clone(), 11),
        ]
    );
    let lines = "┌─┬─┐ ╔═╦═╗ ▀▄█░ \u{E0B0}";
    assert_eq!(
        runs(&frame, 1),
        [
            (0, lines.to_string(), None, foreground.clone(), 0),
            (19, "界".to_string(), Some(2), foreground.clone(), 0),
            (22, "e\u{301}".to_string(), Some(1), foreground.clone(), 0),
            (23, "!".to_string(), None, foreground, 0),
        ]
    );
    let shapes = frame.get("shapes").unwrap();
    for char in lines.chars().filter(|char| *char != ' ') {
        let shape = shapes
            .get(&char.to_string())
            .unwrap_or_else(|| panic!("no shape for {char}"));
        assert!(!shape.items().is_empty(), "{char}");
    }
    assert_eq!(shapes.get("界"), None);
}

#[test]
fn the_renderer_paints_vims_colors_and_the_background_it_erases_in() {
    let Some(vim) = installed("vim") else { return };
    let home = Folder::new();
    fs::write(home.0.join("notes.txt"), "alpha\nbeta\n").unwrap();
    let mut vim = Terminal::run(
        &home.0,
        &vim,
        &[
            &vim,
            "-u",
            "NONE",
            "-N",
            "-n",
            "-i",
            "NONE",
            "--cmd",
            "set termguicolors",
            "-c",
            "highlight Normal guifg=#c0c0c0 guibg=#203040",
            "-c",
            "highlight NonText guifg=#ff8000 cterm=bold,italic,underline gui=bold,italic,underline",
            "notes.txt",
        ],
        &[],
    );
    vim.until("the file in its colors", |vim| {
        let rows = vim.core.screen().rows();
        vim.alternate()
            && vim.rows()[..3] == ["alpha", "beta", "~"]
            && rows.iter().all(|row| {
                row.cells()
                    .iter()
                    .all(|cell| cell.style().background == Color::Rgb(0x20, 0x30, 0x40))
            })
    });

    let mut renderer = Renderer::new(palette());
    renderer.measure(&measure()).unwrap();
    let frame = Json::read(&renderer.frame(&vim.core, "").unwrap().json());
    let colors = frame.get("colors").unwrap().items();
    for row in frame.get("rows").unwrap().items() {
        // Every row is one span of vim's background, the whole width.
        let spans = row.items()[1].items();
        assert_eq!(spans.len(), 3, "{row:?}");
        assert_eq!(
            (spans[0].number(), spans[1].number()),
            (0.0, f64::from(SIZE.cols))
        );
        assert_eq!(colors[spans[2].number() as usize].text(), "#203040");
    }
    assert_eq!(
        runs(&frame, 0),
        [(0, "alpha".to_string(), Some(0), "#c0c0c0".to_string(), 0)]
    );
    // vim writes the rest of the row in NonText's attributes, so its underline runs on.
    let tilde = format!("~{}", " ".repeat(usize::from(SIZE.cols) - 1));
    assert_eq!(
        runs(&frame, 2),
        [(0, tilde, Some(0), "#ff8000".to_string(), 11)]
    );

    vim.keys(":q!\r");
    vim.until_exit();
}

#[test]
fn typing_at_a_prompt_sends_one_row_in_a_small_frame() {
    let home = Folder::new();
    let mut sh = Terminal::run(&home.0, "/bin/sh", &["/bin/sh", "-i"], &[("PS1", "$ ")]);
    sh.until("the prompt", |sh| sh.rows()[0] == "$");
    let mut renderer = Renderer::new(palette());
    renderer.measure(&measure()).unwrap();
    let first = renderer.frame(&sh.core, "").unwrap().json();
    assert!(Json::read(&first).get("full").is_some());

    sh.keys("echo hi");
    sh.until("the line typed", |sh| sh.rows()[0] == "$ echo hi");
    let json = renderer.frame(&sh.core, "").unwrap().json();
    let frame = Json::read(&json);
    assert_eq!(frame.get("full"), None);
    assert_eq!(frame.get("rows").unwrap().items().len(), 1);
    assert_eq!(runs(&frame, 0)[0].1, "$ echo hi");
    // Under 8 KB, a channel message goes to the webview in one step (DESIGN.md §9).
    assert!(json.len() < 8 * 1024, "{} bytes", json.len());

    sh.keys("\rexit\r");
    sh.until_exit();
}
