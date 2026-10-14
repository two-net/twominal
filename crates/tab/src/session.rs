//! Session (SRS §6.1).
//!
//! Starts the shell on a pseudo-terminal with shell integration; carries bytes both ways;
//! reports exit.
//!
//! Must not interpret output: bytes go to the Terminal Core untouched, and the Tab does the
//! passing. Shell-specific behavior lives here, in the shell integration, and nowhere else
//! (ARC-05).
//!
//! A session has three threads of its own, so the tab's thread never waits on the shell: a
//! reader, a writer and a waiter. DESIGN.md §3 says what runs, with which environment, and
//! when the end of a shell is reported.
//!
//! A shell that cannot start is reported with why, and with another shell the tab can offer
//! instead (FISH-02).
//!
//! Filled in by #15 (shell integration).

#[cfg(test)]
mod tests;

use std::ffi::{OsStr, OsString};
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Arc, Condvar, Mutex, MutexGuard, PoisonError};
use std::thread;
use std::time::{Duration, Instant};
use std::{env, fmt, fs, iter};

use portable_pty::{native_pty_system, Child, ChildKiller, CommandBuilder, MasterPty, PtySize};
use terminal_core::Size;

/// The terminal a shell is told it runs in (SW-02).
const TERM: &str = "xterm-256color";

/// The locale a shell gets when none is set: UTF-8, and nothing else changed (SW-02).
const UTF8_LOCALE: &str = "C.UTF-8";

/// The most the reader takes from the shell at once.
const CHUNK: usize = 64 * 1024;

/// How long the reader must have waited for output, once the shell has ended, before the
/// end is reported anyway. A program the shell left running can hold the terminal open for
/// as long as it likes.
const GRACE: Duration = Duration::from_millis(100);

/// What a session is started with. All of it comes from the layers above, through the Tab.
pub struct Start {
    /// The shell to run: the Shell setting, or a shell offered when one did not start.
    pub shell: Shell,
    /// Where the shell starts, or `None` for the home directory (FISH-01).
    pub directory: Option<PathBuf>,
    /// The size of the terminal area.
    pub size: Size,
    /// The product's name and version, which the App Shell has from Tauri. The shell sees
    /// them as `TERM_PROGRAM` and `TERM_PROGRAM_VERSION` (SW-02).
    pub name: String,
    pub version: String,
}

/// Which shell a session runs.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Shell {
    /// The user's default shell (SW-01).
    Default,
    /// The OS's own shell, offered when the default cannot start (FISH-02). On macOS and
    /// Linux it is the shell in the user's account entry, else `/bin/sh`, started as a login
    /// shell like the default. On Windows it is `cmd.exe`.
    Fallback,
    /// The Shell setting: the executable and its arguments, as written (FISH-03).
    Command(Vec<String>),
}

impl From<Option<Vec<String>>> for Shell {
    /// The Shell setting's value. Not set, or with no executable, it is the default shell.
    fn from(setting: Option<Vec<String>>) -> Self {
        match setting {
            Some(words) if !words.is_empty() => Self::Command(words),
            _ => Self::Default,
        }
    }
}

impl Shell {
    /// The shell to offer when this one did not start (FISH-02). The Shell setting gives way
    /// to the default shell, or to the OS's own when the default would not start either, and
    /// the default to the OS's own. Nothing comes after the OS's own, so the offers never go
    /// round in a circle.
    fn instead(&self) -> Option<Offer> {
        let shell = match self {
            Self::Command(_) if refused(&program(&Self::Default)).is_none() => Self::Default,
            Self::Command(_) | Self::Default => Self::Fallback,
            Self::Fallback => return None,
        };
        let program = name(&program(&shell));
        Some(Offer { shell, program })
    }
}

/// Why a session did not start.
#[derive(Debug)]
pub enum Error {
    /// The shell could not be started (FISH-02). Another shell may work.
    Shell(NotStarted),
    /// The pseudo-terminal or one of the session's threads could not be made. Another shell
    /// would fare no better.
    Session(io::Error),
}

impl From<io::Error> for Error {
    fn from(error: io::Error) -> Self {
        Self::Session(error)
    }
}

/// A shell that could not be started, and what the tab can offer instead (FISH-02). Shown as
/// it is written, it is one sentence that says what went wrong.
#[derive(Debug)]
pub struct NotStarted {
    /// What was to run: the executable as the Shell setting writes it, or the default shell.
    pub program: String,
    pub problem: Problem,
    /// Another shell found on the system, or `None` when there is nothing left to offer.
    pub instead: Option<Offer>,
}

/// What kept a shell from starting.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Problem {
    /// There is no such program. `searched` holds the folders of `PATH` it was looked for in,
    /// and is empty when the program was given as a path.
    NotFound { searched: Vec<PathBuf> },
    /// The program is there, but cannot run. Says why, in the OS's words when only the OS
    /// knows.
    CannotRun(String),
}

/// A shell the tab can offer to open instead of one that did not start (FISH-02).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Offer {
    pub shell: Shell,
    /// What it runs, to name it to the user.
    pub program: String,
}

impl fmt::Display for NotStarted {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "The shell {} ", self.program)?;
        match &self.problem {
            Problem::NotFound { searched } if searched.is_empty() => write!(f, "was not found."),
            // On macOS an app opened from the Dock has a short `PATH`, which is the likeliest
            // reason for a shell to be missing.
            Problem::NotFound { searched } => write!(
                f,
                "was not found in {}. Write its full path in the Shell setting.",
                either(searched)
            ),
            Problem::CannotRun(why) => write!(f, "could not be started: {why}."),
        }
    }
}

/// `folders` as a list to choose from: `a`, `a or b`, `a, b or c`.
fn either(folders: &[PathBuf]) -> String {
    let names: Vec<_> = folders
        .iter()
        .map(|folder| folder.display().to_string())
        .collect();
    match names.split_last() {
        Some((last, [])) => last.clone(),
        Some((last, rest)) => format!("{} or {last}", rest.join(", ")),
        None => String::new(),
    }
}

/// What a session tells its tab.
#[derive(Debug)]
pub enum Output {
    /// What the shell wrote, untouched and in order.
    Bytes(Vec<u8>),
    /// The shell has ended (TAB-06). Comes once, after everything the shell wrote. Only a
    /// program it left running in the background can write after it.
    Exited(io::Result<Exit>),
}

/// How a shell ended.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Exit {
    /// It exited with this status.
    Status(u32),
    /// A signal ended it, named as the OS names it.
    Signal(String),
}

impl Exit {
    /// Whether the shell exited with status 0, which closes its tab (TAB-06).
    pub fn success(&self) -> bool {
        *self == Self::Status(0)
    }
}

impl From<portable_pty::ExitStatus> for Exit {
    fn from(status: portable_pty::ExitStatus) -> Self {
        match status.signal() {
            Some(signal) => Self::Signal(signal.to_string()),
            None => Self::Status(status.exit_code()),
        }
    }
}

/// The tab's function for what the session tells it. The reader and the waiter both call it.
type Sink = Arc<dyn Fn(Output) + Send + Sync>;

/// One shell process connected to one tab through a pseudo-terminal.
pub struct Session {
    /// Kept to change the size. The reader has a handle of its own.
    master: Box<dyn MasterPty + Send>,
    /// To the writer thread.
    input: Sender<Vec<u8>>,
    killer: Box<dyn ChildKiller + Send + Sync>,
    /// Set when the waiter has the shell's exit status. Its process ID may then belong to
    /// another process, so it is not signalled again.
    ended: Arc<AtomicBool>,
}

impl Session {
    /// Starts the shell. `output` is called with what the shell writes and, last, with how it
    /// ended, from threads of the session's own. It may block: the reader then stops reading,
    /// and the shell waits (PERF-07).
    ///
    /// A shell that cannot start is an [`Error::Shell`], which says why and what to offer
    /// instead (FISH-02).
    pub fn start(
        start: Start,
        output: impl Fn(Output) + Send + Sync + 'static,
    ) -> Result<Self, Error> {
        spawn(&start.shell, command(&start), start.size, Arc::new(output))
    }

    /// Passes `bytes` to the shell, without waiting for the shell to read them.
    pub fn write(&self, bytes: Vec<u8>) {
        // The writer stops only when the shell can no longer read, and then the end of the
        // shell is reported through the output function.
        let _ = self.input.send(bytes);
    }

    /// Tells the shell the new size of the terminal area (TERM-15).
    pub fn resize(&self, size: Size) -> io::Result<()> {
        self.master.resize(pty_size(size)).map_err(io::Error::other)
    }
}

impl Drop for Session {
    /// Hangs up the shell, as closing a terminal always has. The shell passes the hangup on
    /// to its jobs.
    fn drop(&mut self) {
        if !self.ended.load(Ordering::Acquire) {
            let _ = self.killer.kill();
        }
    }
}

/// What runs, where, and with which environment.
fn command(start: &Start) -> CommandBuilder {
    let mut command = program(&start.shell);

    command.env("TERM", TERM);
    command.env("COLORTERM", "truecolor");
    command.env("TERM_PROGRAM", &start.name);
    command.env("TERM_PROGRAM_VERSION", &start.version);
    if let Some((key, value)) = locale(|key| command.get_env(key)) {
        command.env(key, value);
    }

    match &start.directory {
        // The shell takes `PWD` as its directory when it names the same place, so a path
        // through a symbolic link stays as it was typed.
        Some(directory) => {
            command.cwd(directory);
            command.env("PWD", directory);
        }
        // portable-pty starts the shell in the home directory, and the shell works out the
        // rest. Twominal's own `PWD` is where Twominal was started.
        None => command.env_remove("PWD"),
    }
    command
}

/// What runs for `shell`, before its environment and directory are set.
fn program(shell: &Shell) -> CommandBuilder {
    match shell {
        Shell::Command(words) => match words.split_first() {
            Some((program, arguments)) => {
                let mut command = CommandBuilder::new(program);
                command.args(arguments);
                command
            }
            None => default_shell(),
        },
        Shell::Default => default_shell(),
        Shell::Fallback => fallback_shell(),
    }
}

/// The user's default shell (SW-01). portable-pty finds it: `$SHELL`, else the shell in the
/// user's account entry, else `/bin/sh`. A `$SHELL` that cannot run is [`refused`] before
/// that. It starts it as a login shell, with a `-` before its name as `login` does, and with
/// no command it is interactive.
#[cfg(not(windows))]
fn default_shell() -> CommandBuilder {
    CommandBuilder::new_default_prog()
}

/// Windows records no default shell for a user, so it is PowerShell (SW-01).
#[cfg(windows)]
fn default_shell() -> CommandBuilder {
    CommandBuilder::new(powershell(env::var_os("PATH").as_deref()))
}

/// The OS's own shell (FISH-02). portable-pty finds it as it finds the default shell, but
/// without `$SHELL`: the shell in the user's account entry if it can run, else `/bin/sh`. It
/// starts as a login shell too.
#[cfg(not(windows))]
fn fallback_shell() -> CommandBuilder {
    let mut command = CommandBuilder::new_default_prog();
    command.env_remove("SHELL");
    command
}

/// What `%ComSpec%` names, else `cmd.exe`: portable-pty starts it when given no program.
#[cfg(windows)]
fn fallback_shell() -> CommandBuilder {
    CommandBuilder::new_default_prog()
}

/// What `command` runs, as the user would name it.
fn name(command: &CommandBuilder) -> String {
    match command.get_argv().first() {
        Some(program) => program.to_string_lossy().into_owned(),
        None => command.get_shell(),
    }
}

/// The program `command` would start as the default shell, and what is wrong with it, before
/// it is started (FISH-02). When `$SHELL` names a shell that cannot run, portable-pty would
/// start another in its place without a word. With `$SHELL` not set the default is the shell
/// in the user's account entry, and that is not looked at here.
#[cfg(not(windows))]
fn refused(command: &CommandBuilder) -> Option<(String, Problem)> {
    if !command.is_default_prog() {
        return None;
    }
    let shell = command.get_env("SHELL").filter(|shell| !shell.is_empty())?;
    let problem = problem_at(Path::new(shell))?;
    Some((shell.to_string_lossy().into_owned(), problem))
}

/// Windows has no `$SHELL`. Its default shell is PowerShell, which is started as any program.
#[cfg(windows)]
fn refused(_command: &CommandBuilder) -> Option<(String, Problem)> {
    None
}

/// What is wrong with the program `command` runs, as far as can be seen without running it.
/// `None` when it looks as if it can run, and only the OS knows why it did not.
fn problem_of(command: &CommandBuilder) -> Option<Problem> {
    let program = command.get_argv().first()?;
    // A relative path is relative to the shell's directory, which portable-pty chooses so.
    let home = command.get_env(if cfg!(windows) { "USERPROFILE" } else { "HOME" });
    let directory = command
        .get_cwd()
        .map(OsString::as_os_str)
        .filter(|directory| Path::new(directory).is_dir())
        .or(home)
        .unwrap_or_default();
    let extensions = if cfg!(windows) {
        command.get_env("PATHEXT")
    } else {
        None
    };
    problem(
        program,
        command.get_env("PATH"),
        extensions,
        Path::new(directory),
    )
}

/// What is wrong with `program`, or `None` when it looks as if it can run. A program with no
/// folder in its name is looked for in each folder of `path`, as written and with each of
/// `extensions` added (Windows' `PATHEXT`). Any other is a path, relative to `directory`.
fn problem(
    program: &OsStr,
    path: Option<&OsStr>,
    extensions: Option<&OsStr>,
    directory: &Path,
) -> Option<Problem> {
    if Path::new(program).parent() != Some(Path::new("")) {
        return problem_at(&directory.join(program));
    }
    let searched: Vec<PathBuf> = path.into_iter().flat_map(env::split_paths).collect();
    let mut cannot = None;
    for folder in &searched {
        for name in names(program, extensions) {
            match problem_at(&folder.join(name)) {
                None => return None,
                Some(Problem::NotFound { .. }) => {}
                Some(problem) => {
                    cannot.get_or_insert(problem);
                }
            }
        }
    }
    Some(cannot.unwrap_or(Problem::NotFound { searched }))
}

/// The names `program` is looked for by: as written, then with each of `extensions` added.
/// They are separated by `;`, as in Windows' `PATHEXT`.
fn names(program: &OsStr, extensions: Option<&OsStr>) -> impl Iterator<Item = OsString> {
    let extensions = extensions.map(OsStr::to_string_lossy).unwrap_or_default();
    let added: Vec<OsString> = extensions
        .split(';')
        .filter(|extension| !extension.is_empty())
        .map(|extension| {
            let mut name = program.to_os_string();
            name.push(extension);
            name
        })
        .collect();
    iter::once(program.to_os_string()).chain(added)
}

/// What is wrong with the file at `path` as a program, or `None` when it looks as if it can
/// run.
fn problem_at(path: &Path) -> Option<Problem> {
    let why = match fs::metadata(path) {
        Err(_) => {
            return Some(Problem::NotFound {
                searched: Vec::new(),
            })
        }
        Ok(file) if file.is_dir() => "it is a folder",
        Ok(file) if !executable(&file) => "it is not executable",
        Ok(_) => return None,
    };
    Some(Problem::CannotRun(why.to_string()))
}

#[cfg(unix)]
fn executable(file: &fs::Metadata) -> bool {
    use std::os::unix::fs::PermissionsExt;
    file.permissions().mode() & 0o111 != 0
}

/// Windows runs a file by the extension of its name, not by a permission.
#[cfg(not(unix))]
fn executable(_file: &fs::Metadata) -> bool {
    true
}

/// PowerShell 7 or later if `pwsh.exe` is on `path`, otherwise the Windows PowerShell that
/// ships with Windows (SW-01).
#[cfg_attr(
    not(any(windows, test)),
    expect(dead_code, reason = "only Windows has no default shell of its own")
)]
fn powershell(path: Option<&OsStr>) -> PathBuf {
    path.into_iter()
        .flat_map(env::split_paths)
        .map(|directory| directory.join("pwsh.exe"))
        .find(|pwsh| pwsh.is_file())
        .unwrap_or_else(|| PathBuf::from("powershell.exe"))
}

/// The variable to set, and its value, when no locale is set (SW-02). A locale the user did
/// set is theirs, even one that is not UTF-8. `get` reads the shell's environment.
fn locale<'a>(get: impl Fn(&str) -> Option<&'a OsStr>) -> Option<(&'static str, &'static str)> {
    let set = ["LC_ALL", "LC_CTYPE", "LANG"]
        .into_iter()
        .any(|key| get(key).is_some_and(|value| !value.is_empty()));
    (!set).then_some(("LANG", UTF8_LOCALE))
}

/// A size the pseudo-terminal is given. Never zero, which some programs cannot handle.
fn pty_size(size: Size) -> PtySize {
    PtySize {
        rows: size.rows.max(1),
        cols: size.cols.max(1),
        pixel_width: 0,
        pixel_height: 0,
    }
}

/// Starts `command`, which runs `shell`, on a new pseudo-terminal of `size`, with the
/// session's threads.
fn spawn(
    shell: &Shell,
    command: CommandBuilder,
    size: Size,
    output: Sink,
) -> Result<Session, Error> {
    let not_started = |program, problem| {
        Error::Shell(NotStarted {
            program,
            problem,
            instead: shell.instead(),
        })
    };
    if let Some((program, problem)) = refused(&command) {
        return Err(not_started(program, problem));
    }

    let pair = native_pty_system()
        .openpty(pty_size(size))
        .map_err(io::Error::other)?;
    let reader = pair.master.try_clone_reader().map_err(io::Error::other)?;
    let writer = pair.master.take_writer().map_err(io::Error::other)?;
    let mut child = match pair.slave.spawn_command(command.clone()) {
        Ok(child) => child,
        Err(error) => {
            let problem = problem_of(&command).unwrap_or(Problem::CannotRun(error.to_string()));
            return Err(not_started(name(&command), problem));
        }
    };
    // Now only the shell may hold the other end. While Twominal holds it too, the reader
    // never comes to the end of the output.
    drop(pair.slave);

    let (input, queued) = mpsc::channel();
    let reading = Arc::new(Reading::default());
    let started = thread::Builder::new()
        .name("session-writer".to_string())
        .spawn(move || write(writer, queued))
        .and_then(|_| {
            let (output, reading) = (output.clone(), reading.clone());
            thread::Builder::new()
                .name("session-reader".to_string())
                .spawn(move || read(reader, &*output, &reading))
        });
    if let Err(error) = started {
        let _ = child.kill();
        let _ = child.wait();
        return Err(error.into());
    }

    let mut killer = child.clone_killer();
    let ended = Arc::new(AtomicBool::new(false));
    let waiting = thread::Builder::new()
        .name("session-waiter".to_string())
        .spawn({
            let ended = ended.clone();
            move || wait(child, &*output, &reading, &ended)
        });
    if let Err(error) = waiting {
        let _ = killer.kill();
        return Err(error.into());
    }

    Ok(Session {
        master: pair.master,
        input,
        killer,
        ended,
    })
}

/// The reader thread: passes on what the shell writes, until the end of the output.
fn read(mut reader: Box<dyn Read + Send>, output: &dyn Fn(Output), reading: &Reading) {
    let mut buffer = vec![0; CHUNK];
    loop {
        reading.set(State::Waiting(Instant::now()));
        let read = reader.read(&mut buffer);
        reading.set(State::Busy);
        match read {
            // portable-pty reads the error Linux gives at the end as the end.
            Ok(0) => break,
            Ok(length) => output(Output::Bytes(buffer[..length].to_vec())),
            Err(error) if error.kind() == io::ErrorKind::Interrupted => {}
            // Nothing more can be read. The shell's end is still reported by the waiter.
            Err(_) => break,
        }
    }
    reading.set(State::Done);
}

/// The writer thread: passes on what the tab writes, until the session is dropped or the
/// shell can no longer read.
fn write(mut writer: Box<dyn Write + Send>, queued: Receiver<Vec<u8>>) {
    for bytes in queued {
        if writer
            .write_all(&bytes)
            .and_then(|()| writer.flush())
            .is_err()
        {
            break;
        }
    }
}

/// The waiter thread: reports how the shell ended, after the last of its output (TAB-06).
fn wait(
    mut child: Box<dyn Child + Send + Sync>,
    output: &dyn Fn(Output),
    reading: &Reading,
    ended: &AtomicBool,
) {
    let exit = child.wait().map(Exit::from);
    ended.store(true, Ordering::Release);
    reading.settle();
    output(Output::Exited(exit));
}

/// What the reader is doing, so that the waiter can tell when the shell's output has all
/// been passed on.
#[derive(Default)]
struct Reading {
    state: Mutex<State>,
    changed: Condvar,
}

#[derive(Clone, Copy, Default)]
enum State {
    /// Passing on what it read.
    #[default]
    Busy,
    /// Waiting for the shell to write, since then.
    Waiting(Instant),
    /// At the end of the output.
    Done,
}

impl Reading {
    fn set(&self, state: State) {
        *self.lock() = state;
        self.changed.notify_all();
    }

    /// Returns when everything the shell wrote before it ended has been passed on: when the
    /// reader is at the end of the output, or has waited [`GRACE`] for more. While it is busy
    /// passing output on, a slow tab included, this keeps waiting.
    fn settle(&self) {
        let mut state = self.lock();
        loop {
            state = match *state {
                State::Done => return,
                State::Busy => self
                    .changed
                    .wait(state)
                    .unwrap_or_else(PoisonError::into_inner),
                State::Waiting(since) => {
                    let left = GRACE.saturating_sub(since.elapsed());
                    if left.is_zero() {
                        return;
                    }
                    self.changed
                        .wait_timeout(state, left)
                        .unwrap_or_else(PoisonError::into_inner)
                        .0
                }
            };
        }
    }

    fn lock(&self) -> MutexGuard<'_, State> {
        // A state is one value, so a panic under the lock leaves it whole.
        self.state.lock().unwrap_or_else(PoisonError::into_inner)
    }
}
