//! The launcher: the `twominal` command (GEN-05). Part of the App Shell.
//!
//! Reads the command's arguments and says what it was asked to do. All of it happens before
//! Tauri starts, so the help, the version and every problem need no display.
//!
//! A command for a window is handed to the Twominal that is already running, if one is. The
//! hand-over itself is in `app/socket.rs`. DESIGN.md §3 says how the command behaves.

use std::ffi::OsString;
use std::fmt;
use std::path::{Path, PathBuf};

#[cfg(unix)]
pub(super) use super::socket::Claim;

/// What the command was asked to do.
#[derive(Debug, PartialEq, Eq)]
pub(super) enum Command {
    /// Open a window. Its first tab starts in the directory, or with none in the home
    /// directory (FISH-01).
    Open(Option<PathBuf>),
    Help,
    Version,
}

/// Why the command does not do what it was asked.
#[derive(Debug, PartialEq, Eq)]
pub(super) enum Problem {
    UnknownOption(OsString),
    SecondDirectory(OsString),
    NoSuchDirectory(PathBuf),
    NotADirectory(PathBuf),
}

impl Problem {
    /// The command's exit status: 2 if it was written wrongly, 1 if what it names is not
    /// there.
    pub(super) fn status(&self) -> u8 {
        match self {
            Self::UnknownOption(_) | Self::SecondDirectory(_) => 2,
            Self::NoSuchDirectory(_) | Self::NotADirectory(_) => 1,
        }
    }

    /// What the command prints on the standard error.
    pub(super) fn message(&self) -> String {
        match self.status() {
            2 => format!("twominal: {self}\nTry 'twominal --help'."),
            _ => format!("twominal: {self}"),
        }
    }
}

impl fmt::Display for Problem {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnknownOption(option) => {
                write!(f, "unknown option '{}'", option.to_string_lossy())
            }
            Self::SecondDirectory(directory) => {
                write!(
                    f,
                    "more than one directory: '{}'",
                    directory.to_string_lossy()
                )
            }
            Self::NoSuchDirectory(path) => write!(f, "no such directory: {}", path.display()),
            Self::NotADirectory(path) => write!(f, "not a directory: {}", path.display()),
        }
    }
}

/// Reads the command's arguments, without the command's own name. A directory comes back
/// absolute and is known to be one.
pub(super) fn read(arguments: impl IntoIterator<Item = OsString>) -> Result<Command, Problem> {
    match parse(arguments)? {
        Command::Open(Some(given)) => Ok(Command::Open(Some(locate(&given)?))),
        command => Ok(command),
    }
}

/// What the arguments ask for, read left to right. A directory comes back as it was written.
pub(super) fn parse(arguments: impl IntoIterator<Item = OsString>) -> Result<Command, Problem> {
    let mut directory = None;
    let mut options = true;
    for argument in arguments {
        if options {
            match argument.as_encoded_bytes() {
                b"--" => {
                    options = false;
                    continue;
                }
                b"-h" | b"--help" => return Ok(Command::Help),
                b"-V" | b"--version" => return Ok(Command::Version),
                // A `-` alone is a name like any other.
                [b'-', _, ..] => return Err(Problem::UnknownOption(argument)),
                _ => {}
            }
        }
        if directory.is_some() {
            return Err(Problem::SecondDirectory(argument));
        }
        directory = Some(PathBuf::from(argument));
    }
    Ok(Command::Open(directory))
}

/// The directory the command was given, as an absolute path. A relative one is relative to
/// where the command was run, which only this process knows. No symbolic link is resolved, so
/// the path stays as it was typed.
pub(super) fn locate(given: &Path) -> Result<PathBuf, Problem> {
    match std::path::absolute(given) {
        Ok(directory) if directory.is_dir() => Ok(directory),
        Ok(file) if file.exists() => Err(Problem::NotADirectory(given.to_path_buf())),
        _ => Err(Problem::NoSuchDirectory(given.to_path_buf())),
    }
}

/// What `--version` prints. `product` is Twominal's name as the user reads it (GEN-01).
pub(super) fn version(product: &str, version: &str) -> String {
    format!("{product} {version}")
}

/// What `--help` prints.
pub(super) fn help(product: &str, version: &str) -> String {
    let title = self::version(product, version);
    let does =
        format!("Opens a new {product} window. Its first tab starts in DIRECTORY, or in the");
    [
        title.as_str(),
        "",
        "Usage: twominal [DIRECTORY]",
        "",
        does.as_str(),
        "home directory when none is given.",
        "",
        "Options:",
        "  -h, --help     Show this help",
        "  -V, --version  Show the version",
    ]
    .join("\n")
}

/// What became of a command for a window.
#[cfg_attr(
    not(unix),
    allow(dead_code, reason = "nothing is handed over off Unix yet")
)]
pub(super) enum Start {
    /// A running Twominal took it and opens the window. This process is done.
    HandedOver,
    /// No Twominal was running. This process is now the one the next commands reach.
    First(Claim),
    /// Nothing can be handed over here. This process runs on its own.
    Alone,
}

/// Hands the command to a running Twominal, or finds that this process is the first.
#[cfg(unix)]
pub(super) fn start(directory: Option<&Path>) -> Start {
    super::socket::Endpoint::find().map_or(Start::Alone, |endpoint| endpoint.start(directory))
}

/// Off Unix nothing is handed over yet (DESIGN.md §3): every command is its own process.
#[cfg(not(unix))]
pub(super) fn start(_directory: Option<&Path>) -> Start {
    Start::Alone
}

/// Off Unix no process is ever the first, so there is none of these.
#[cfg(not(unix))]
pub(super) enum Claim {}

#[cfg(not(unix))]
impl Claim {
    pub(super) fn serve(
        self,
        _open: impl Fn(Option<PathBuf>) + Send + 'static,
    ) -> std::io::Result<()> {
        match self {}
    }
}
