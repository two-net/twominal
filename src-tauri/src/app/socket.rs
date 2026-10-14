//! The launcher's hand-over (GEN-05). Part of the App Shell.
//!
//! How a `twominal` command reaches the Twominal that is already running: one Unix socket in
//! a folder only the user can open, and beside it a lock that says which process listens.
//! Both are made with the standard library alone (ARC-06), and neither is a network port
//! (SEC-01). DESIGN.md §3 gives the reasons and §7 the place.

use std::env;
use std::ffi::OsStr;
use std::fs::{self, File, TryLockError};
use std::io::{self, ErrorKind, Read, Write};
use std::net::Shutdown;
use std::os::unix::ffi::OsStrExt;
use std::os::unix::fs::MetadataExt;
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::{Path, PathBuf};
use std::thread;
use std::time::{Duration, Instant};

use super::launcher::Start;

/// The version of the exchange, sent first. A Twominal answers only the one it knows.
const VERSION: u8 = 1;

/// The answer: the request was taken, and the window opens.
const TAKEN: u8 = 1;

/// How long a command waits for an answer, or for a Twominal that is still starting.
const PATIENCE: Duration = Duration::from_secs(2);

/// How long a command waits before it looks again.
const PAUSE: Duration = Duration::from_millis(20);

/// More bytes than any path has. A longer request is not one of ours.
const LIMIT: usize = 8 * 1024;

/// Where the commands and a running Twominal meet.
pub(super) struct Endpoint {
    socket: PathBuf,
    lock: PathBuf,
}

impl Endpoint {
    /// This build's endpoint. `None` when there is no folder for it, and then nothing is
    /// handed over.
    pub(super) fn find() -> Option<Self> {
        // macOS gives each user a temporary folder of their own, and Linux one for the
        // session. The shared `/tmp` is never used: there anyone could listen in our place.
        let folder = if cfg!(target_os = "macos") {
            env::temp_dir()
        } else {
            PathBuf::from(env::var_os("XDG_RUNTIME_DIR")?)
        };
        Self::in_folder(&folder, cfg!(debug_assertions))
    }

    /// The endpoint in `folder`, which must be one only its owner can open. A debug build has
    /// names of its own, so that it and an installed Twominal never answer for each other.
    pub(super) fn in_folder(folder: &Path, debug: bool) -> Option<Self> {
        let private =
            fs::metadata(folder).is_ok_and(|folder| folder.is_dir() && folder.mode() & 0o077 == 0);
        let name = if debug { "twominal-dev" } else { "twominal" };
        private.then(|| Self {
            socket: folder.join(format!("{name}.sock")),
            lock: folder.join(format!("{name}.lock")),
        })
    }

    /// Hands the command to the Twominal that listens here, or makes this process that
    /// Twominal.
    pub(super) fn start(&self, directory: Option<&Path>) -> Start {
        let deadline = Instant::now() + PATIENCE;
        loop {
            match UnixStream::connect(&self.socket) {
                // A Twominal that does not answer is not asked again. It opens no window for
                // a command that has stopped waiting.
                Ok(stream) => {
                    return match ask(stream, directory) {
                        Ok(()) => Start::HandedOver,
                        Err(_) => Start::Alone,
                    }
                }
                // Nobody listens: there is no socket, or one left by a Twominal that is gone.
                Err(error)
                    if matches!(
                        error.kind(),
                        ErrorKind::NotFound | ErrorKind::ConnectionRefused
                    ) => {}
                Err(_) => return Start::Alone,
            }
            match Claim::take(self) {
                Ok(Some(claim)) => return Start::First(claim),
                // Another process is the first, and is about to listen.
                Ok(None) if Instant::now() < deadline => thread::sleep(PAUSE),
                _ => return Start::Alone,
            }
        }
    }
}

/// What the first process holds: the socket it listens on, and the lock.
pub(super) struct Claim {
    listener: UnixListener,
    /// Never read. The lock lasts for as long as the file is open, and the OS drops it when
    /// the process ends, however it ends.
    _lock: File,
}

impl Claim {
    /// Makes this process the first. `None` when another process already is.
    pub(super) fn take(endpoint: &Endpoint) -> io::Result<Option<Self>> {
        let lock = File::options()
            .create(true)
            .truncate(false)
            .write(true)
            .open(&endpoint.lock)?;
        match lock.try_lock() {
            Ok(()) => {}
            Err(TryLockError::WouldBlock) => return Ok(None),
            Err(TryLockError::Error(error)) => return Err(error),
        }
        // A socket left by a Twominal that is gone. Its owner held the lock, so nobody
        // listens on it now.
        match fs::remove_file(&endpoint.socket) {
            Err(error) if error.kind() != ErrorKind::NotFound => return Err(error),
            _ => {}
        }
        let listener = UnixListener::bind(&endpoint.socket)?;
        Ok(Some(Self {
            listener,
            _lock: lock,
        }))
    }

    /// Answers every command from now on, on a thread of its own, and calls `open` with the
    /// directory each one asks for. Commands that came before this have waited at the socket.
    pub(super) fn serve(self, open: impl Fn(Option<PathBuf>) + Send + 'static) -> io::Result<()> {
        let listening = move || {
            // Both move to this thread. Naming only the listener would leave the lock behind,
            // to be dropped while Twominal still listens.
            let Self { listener, _lock } = self;
            for stream in listener.incoming() {
                match stream.and_then(hear) {
                    Ok(directory) => open(directory),
                    // One command that went wrong does not stop the next. The pause is for a
                    // socket that fails every time.
                    Err(_) => thread::sleep(PAUSE),
                }
            }
        };
        thread::Builder::new()
            .name("launcher".to_string())
            .spawn(listening)
            .map(drop)
    }
}

/// Sends a command's request and waits for the answer.
fn ask(mut stream: UnixStream, directory: Option<&Path>) -> io::Result<()> {
    stream.set_read_timeout(Some(PATIENCE))?;
    stream.set_write_timeout(Some(PATIENCE))?;
    stream.write_all(&encode(directory))?;
    // The request ends where the writing does.
    stream.shutdown(Shutdown::Write)?;
    let mut answer = [0];
    stream.read_exact(&mut answer)?;
    if answer == [TAKEN] {
        Ok(())
    } else {
        Err(ErrorKind::InvalidData.into())
    }
}

/// Reads one command's request and answers it. Gives the directory to open a window in.
fn hear(mut stream: UnixStream) -> io::Result<Option<PathBuf>> {
    stream.set_read_timeout(Some(PATIENCE))?;
    stream.set_write_timeout(Some(PATIENCE))?;
    let mut request = Vec::new();
    (&stream).take(LIMIT as u64 + 1).read_to_end(&mut request)?;
    let directory = decode(&request).ok_or(ErrorKind::InvalidData)?;
    // The answer goes first. A command that has stopped waiting runs on its own, and then
    // this write fails and no second window opens here.
    stream.write_all(&[TAKEN])?;
    Ok(directory)
}

/// A request: the version, then the directory's bytes. None of them means the home
/// directory.
pub(super) fn encode(directory: Option<&Path>) -> Vec<u8> {
    let mut request = vec![VERSION];
    if let Some(directory) = directory {
        request.extend_from_slice(directory.as_os_str().as_bytes());
    }
    request
}

/// The directory a request asks for. `None` if the request is not one this Twominal
/// understands.
pub(super) fn decode(request: &[u8]) -> Option<Option<PathBuf>> {
    let (&VERSION, directory) = request.split_first()? else {
        return None;
    };
    if request.len() > LIMIT {
        return None;
    }
    if directory.is_empty() {
        return Some(None);
    }
    // The command made the path absolute, where it knew what it was relative to.
    let directory = Path::new(OsStr::from_bytes(directory));
    directory
        .is_absolute()
        .then(|| Some(directory.to_path_buf()))
}
