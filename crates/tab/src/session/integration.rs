//! Shell integration (FISH-04 to FISH-06): what Twominal tells a shell at startup so that it
//! reports to Twominal, without touching any file of the user's.
//!
//! Which shells get it, and how each is given its script, is decided here and nowhere else
//! (ARC-05). The scripts are in `integration/`. DESIGN.md §3 says how each shell is started,
//! and §7 what the reports are.

use std::ffi::{OsStr, OsString};
use std::hash::{BuildHasher, DefaultHasher, Hash, Hasher, RandomState};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::{fs, io, process};

use portable_pty::CommandBuilder;

/// zsh reads it as `.zshenv`, from the folder `ZDOTDIR` names.
const ZSH: &str = include_str!("integration/integration.zsh");
/// bash reads it as its `--rcfile`.
const BASH: &str = include_str!("integration/integration.bash");
/// PowerShell is given it as `-EncodedCommand`.
const POWERSHELL: &str = include_str!("integration/integration.ps1");

/// A shell Twominal has shell integration for (FISH-04).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Kind {
    Zsh,
    Bash,
    PowerShell,
}

impl Kind {
    /// The shell `program` runs, told by its file name, in any letter case and with or without
    /// `.exe`. `None` for a shell Twominal has no integration for, which runs in passthrough
    /// (FISH-03).
    pub(super) fn of(program: &OsStr) -> Option<Self> {
        let name = Path::new(program)
            .file_name()?
            .to_str()?
            .to_ascii_lowercase();
        match name.strip_suffix(".exe").unwrap_or(&name) {
            "zsh" => Some(Self::Zsh),
            "bash" => Some(Self::Bash),
            "pwsh" | "powershell" => Some(Self::PowerShell),
            _ => None,
        }
    }

    /// Whether `arguments`, from the Shell setting, leave the shell's startup as it is, so that
    /// integration can be added to them. Any other argument, such as a command to run, and the
    /// shell runs as written, without integration.
    pub(super) fn takes(self, arguments: &[OsString]) -> bool {
        arguments.iter().all(|argument| match self {
            Self::Zsh | Self::Bash => matches!(argument.to_str(), Some("-l" | "--login" | "-i")),
            Self::PowerShell => ["-NoLogo", "-NoProfile", "-Login", "-l", "-Interactive"]
                .iter()
                .any(|known| argument.eq_ignore_ascii_case(known)),
        })
    }
}

/// Applies shell integration to `command`, when `on` (the Shell integration setting, FISH-06)
/// and Twominal has it for the shell `command` runs. Returns the nonce its reports carry, or
/// `None` when the shell runs without it. `private` is a folder only the user can open, where
/// the files bash and zsh read are kept. Without it they run without integration.
///
/// PowerShell on Windows is told to use UTF-8 even when integration is off (SW-02).
pub(super) fn apply(
    command: &mut CommandBuilder,
    on: bool,
    private: Option<&Path>,
) -> Option<String> {
    let default = command.is_default_prog();
    let (program, arguments) = match command.get_argv().split_first() {
        Some((program, arguments)) => (program.clone(), arguments.to_vec()),
        // portable-pty's default shell, which it starts as a login shell.
        None => (OsString::from(command.get_shell()), Vec::new()),
    };
    let kind = Kind::of(&program).filter(|kind| kind.takes(&arguments))?;
    let utf8 = kind == Kind::PowerShell && cfg!(windows);
    if !(on || utf8) {
        return None;
    }

    match kind {
        Kind::Zsh => {
            let folder = files(private?).ok()?;
            // Given back by the script before anything else reads it.
            if let Some(zdotdir) = command.get_env("ZDOTDIR").map(OsStr::to_os_string) {
                command.env("twominal_zdotdir", zdotdir);
            }
            command.env("ZDOTDIR", folder.join("zsh"));
        }
        Kind::Bash => {
            let folder = files(private?).ok()?;
            // bash ignores `--rcfile` in a login shell, so the script reads the login files.
            let login = default
                || arguments
                    .iter()
                    .any(|argument| argument == "-l" || argument == "--login");
            if login {
                command.env("twominal_login", "1");
            }
            let rcfile = folder.join("integration.bash").into_os_string();
            *command.get_argv_mut() = vec![program, "--rcfile".into(), rcfile];
        }
        Kind::PowerShell => {
            let argv = command.get_argv_mut();
            argv.extend(["-NoExit", "-EncodedCommand"].map(OsString::from));
            argv.push(encoded(POWERSHELL).into());
        }
    }
    if !on {
        return None;
    }
    let nonce = nonce();
    command.env("twominal_nonce", &nonce);
    Some(nonce)
}

/// 128 bits no output from elsewhere can know, in hex. The standard library seeds each
/// [`RandomState`] from the OS, so no dependency is needed (ARC-06).
pub(super) fn nonce() -> String {
    let half = || RandomState::new().hash_one(process::id());
    format!("{:016x}{:016x}", half(), half())
}

/// Writes the files bash and zsh read into a folder of their own in `private`, unless they are
/// there already, and returns that folder. It is named after what the files hold, so a file
/// once written never changes, and two versions of Twominal never read each other's.
pub(super) fn files(private: &Path) -> io::Result<PathBuf> {
    let mut hasher = DefaultHasher::new();
    (ZSH, BASH).hash(&mut hasher);
    let folder = private.join(format!("twominal-{:016x}", hasher.finish()));
    keep(&folder.join("zsh").join(".zshenv"), ZSH)?;
    keep(&folder.join("integration.bash"), BASH)?;
    Ok(folder)
}

/// Makes the file at `path` hold `text`, unless it does already. The text is written beside
/// it and renamed into place, so a shell starting at the same moment never reads half a file.
fn keep(path: &Path, text: &str) -> io::Result<()> {
    if fs::read(path).is_ok_and(|held| held == text.as_bytes()) {
        return Ok(());
    }
    let folder = path.parent().unwrap_or(Path::new(""));
    fs::create_dir_all(folder)?;
    static NEXT: AtomicUsize = AtomicUsize::new(0);
    let number = NEXT.fetch_add(1, Ordering::Relaxed);
    let temporary = folder.join(format!(".{}-{number}.part", process::id()));
    fs::write(&temporary, text)
        .and_then(|()| fs::rename(&temporary, path))
        .inspect_err(|_| {
            let _ = fs::remove_file(&temporary);
        })
}

/// `script` as PowerShell's `-EncodedCommand` takes it: UTF-16LE, in base64. It then needs no
/// quoting, which Windows and PowerShell each do their own way.
pub(super) fn encoded(script: &str) -> String {
    let bytes: Vec<u8> = script.encode_utf16().flat_map(u16::to_le_bytes).collect();
    base64(&bytes)
}

fn base64(bytes: &[u8]) -> String {
    const DIGITS: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut text = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let bits = chunk.iter().enumerate().fold(0u32, |bits, (i, &byte)| {
            bits | u32::from(byte) << (16 - 8 * i)
        });
        for i in 0..4 {
            text.push(if i <= chunk.len() {
                char::from(DIGITS[(bits >> (18 - 6 * i) & 63) as usize])
            } else {
                '='
            });
        }
    }
    text
}
