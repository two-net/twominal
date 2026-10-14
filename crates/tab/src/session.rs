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
//! Filled in by #14 (other shells) and #15 (shell integration).

#[cfg(test)]
mod tests;

use std::env;
use std::ffi::OsStr;
use std::io::{self, Read, Write};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Arc, Condvar, Mutex, MutexGuard, PoisonError};
use std::thread;
use std::time::{Duration, Instant};

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
    /// The Shell setting: the executable and its arguments, or `None` for the user's default
    /// shell (SW-01).
    pub shell: Option<Vec<String>>,
    /// Where the shell starts, or `None` for the home directory (FISH-01).
    pub directory: Option<PathBuf>,
    /// The size of the terminal area.
    pub size: Size,
    /// The product's name and version, which the App Shell has from Tauri. The shell sees
    /// them as `TERM_PROGRAM` and `TERM_PROGRAM_VERSION` (SW-02).
    pub name: String,
    pub version: String,
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
    pub fn start(
        start: Start,
        output: impl Fn(Output) + Send + Sync + 'static,
    ) -> io::Result<Self> {
        spawn(command(&start), start.size, Arc::new(output))
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
    let mut command = match start.shell.as_deref().and_then(<[String]>::split_first) {
        Some((program, arguments)) => {
            let mut command = CommandBuilder::new(program);
            command.args(arguments);
            command
        }
        None => default_shell(),
    };

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

/// The user's default shell (SW-01). portable-pty finds it: `$SHELL`, else the shell in the
/// user's account entry, else `/bin/sh`. It starts it as a login shell, with a `-` before its
/// name as `login` does, and with no command it is interactive.
#[cfg(not(windows))]
fn default_shell() -> CommandBuilder {
    CommandBuilder::new_default_prog()
}

/// Windows records no default shell for a user, so it is PowerShell (SW-01).
#[cfg(windows)]
fn default_shell() -> CommandBuilder {
    CommandBuilder::new(powershell(env::var_os("PATH").as_deref()))
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

/// Starts `command` on a new pseudo-terminal of `size`, with the session's threads.
fn spawn(command: CommandBuilder, size: Size, output: Sink) -> io::Result<Session> {
    let pair = native_pty_system()
        .openpty(pty_size(size))
        .map_err(io::Error::other)?;
    let reader = pair.master.try_clone_reader().map_err(io::Error::other)?;
    let writer = pair.master.take_writer().map_err(io::Error::other)?;
    let mut child = pair
        .slave
        .spawn_command(command)
        .map_err(io::Error::other)?;
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
        return Err(error);
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
        return Err(error);
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
