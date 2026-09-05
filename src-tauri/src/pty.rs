use std::io::{Read, Write};

use portable_pty::{native_pty_system, CommandBuilder, PtySize};
use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::shell::ShellLaunch;

pub const MAX_TERMINAL_DIMENSION: u16 = 1000;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize, Serialize)]
pub struct TerminalSize {
    pub rows: u16,
    pub cols: u16,
}

impl TerminalSize {
    pub fn validate(self) -> bool {
        (1..=MAX_TERMINAL_DIMENSION).contains(&self.rows)
            && (1..=MAX_TERMINAL_DIMENSION).contains(&self.cols)
    }

    fn as_pty_size(self) -> PtySize {
        PtySize {
            rows: self.rows,
            cols: self.cols,
            pixel_width: 0,
            pixel_height: 0,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ProcessExit {
    pub exit_code: u32,
    pub signal: Option<String>,
}

#[derive(Debug, Error)]
#[error("{stage}: {detail}")]
pub struct PtyError {
    stage: &'static str,
    detail: String,
}

impl PtyError {
    fn new(stage: &'static str, error: impl std::fmt::Display) -> Self {
        Self {
            stage,
            detail: error.to_string(),
        }
    }
}

pub trait PtyMaster: Send {
    fn resize(&self, size: TerminalSize) -> Result<(), PtyError>;
}

pub trait PtyChild: Send {
    fn try_wait(&mut self) -> std::io::Result<Option<ProcessExit>>;
    fn wait(&mut self) -> std::io::Result<ProcessExit>;
    fn kill(&mut self) -> std::io::Result<()>;
}

pub struct SpawnedPty {
    pub master: Box<dyn PtyMaster>,
    pub reader: Box<dyn Read + Send>,
    pub writer: Box<dyn Write + Send>,
    pub child: Box<dyn PtyChild>,
}

pub trait PtyBackend: Send + Sync {
    fn spawn(&self, shell: &ShellLaunch, size: TerminalSize) -> Result<SpawnedPty, PtyError>;
}

#[derive(Debug, Default)]
pub struct NativePtyBackend;

impl PtyBackend for NativePtyBackend {
    fn spawn(&self, shell: &ShellLaunch, size: TerminalSize) -> Result<SpawnedPty, PtyError> {
        let pty_system = native_pty_system();
        let pair = pty_system
            .openpty(size.as_pty_size())
            .map_err(|error| PtyError::new("open PTY", error))?;

        let mut command = CommandBuilder::new(&shell.executable);
        for argument in &shell.arguments {
            command.arg(argument);
        }
        command.cwd(&shell.cwd);
        command.env("TERM", "xterm-256color");
        command.env("COLORTERM", "truecolor");
        command.env("TERM_PROGRAM", "Twominal");
        command.env("TERM_PROGRAM_VERSION", env!("CARGO_PKG_VERSION"));
        for (name, value) in &shell.environment {
            command.env(name, value);
        }
        #[cfg(unix)]
        command.env("PWD", &shell.cwd);

        let mut child = pair
            .slave
            .spawn_command(command)
            .map_err(|error| PtyError::new("spawn shell", error))?;
        drop(pair.slave);

        let reader = match pair.master.try_clone_reader() {
            Ok(reader) => reader,
            Err(error) => {
                terminate_spawn_failure(&mut child);
                return Err(PtyError::new("clone PTY reader", error));
            }
        };
        let writer = match pair.master.take_writer() {
            Ok(writer) => writer,
            Err(error) => {
                terminate_spawn_failure(&mut child);
                return Err(PtyError::new("take PTY writer", error));
            }
        };

        Ok(SpawnedPty {
            master: Box::new(NativeMaster(pair.master)),
            reader,
            writer,
            child: Box::new(NativeChild(child)),
        })
    }
}

fn terminate_spawn_failure(child: &mut Box<dyn portable_pty::Child + Send + Sync>) {
    let _ = child.kill();
    let _ = child.wait();
}

struct NativeMaster(Box<dyn portable_pty::MasterPty + Send>);

impl PtyMaster for NativeMaster {
    fn resize(&self, size: TerminalSize) -> Result<(), PtyError> {
        self.0
            .resize(size.as_pty_size())
            .map_err(|error| PtyError::new("resize PTY", error))
    }
}

struct NativeChild(Box<dyn portable_pty::Child + Send + Sync>);

impl PtyChild for NativeChild {
    fn try_wait(&mut self) -> std::io::Result<Option<ProcessExit>> {
        self.0.try_wait().map(|status| {
            status.map(|status| ProcessExit {
                exit_code: status.exit_code(),
                signal: status.signal().map(str::to_owned),
            })
        })
    }

    fn wait(&mut self) -> std::io::Result<ProcessExit> {
        self.0.wait().map(|status| ProcessExit {
            exit_code: status.exit_code(),
            signal: status.signal().map(str::to_owned),
        })
    }

    fn kill(&mut self) -> std::io::Result<()> {
        self.0.kill()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validates_terminal_dimensions() {
        assert!(TerminalSize { rows: 24, cols: 80 }.validate());
        assert!(!TerminalSize { rows: 0, cols: 80 }.validate());
        assert!(!TerminalSize { rows: 24, cols: 0 }.validate());
        assert!(!TerminalSize {
            rows: MAX_TERMINAL_DIMENSION,
            cols: MAX_TERMINAL_DIMENSION + 1,
        }
        .validate());
    }
}
