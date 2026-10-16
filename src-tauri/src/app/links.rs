//! Links open in the OS's default handler for their kind (SW-07): the `tab::OsLinks` the App
//! Shell gives each tab with its Host (#31). A tab hands it only a URL that SEC-07 lets open, which
//! the user clicked after seeing where it leads, so this decides how, never whether.
//!
//! Each OS has a command that opens a link as its file manager would, which is given the URL, or a
//! file's path, as one argument, with no shell between: `open` on macOS, `xdg-open` on Linux, and
//! `url.dll`'s handler on Windows. No dependency is added (ARC-06). A link opens on a thread of its
//! own, which waits for the command, so no tab waits and no process is left unreaped.
//!
//! A `file:` link to a program, a script or a launcher is shown in the file manager instead of
//! opened, since opening it would run it, and a program can print an innocent name over such a
//! link. A link to a file is judged and opened by the file it leads to, through every symbolic
//! link, as the OS would open that. Windows is not checked until the first Windows build.
//! DESIGN.md §3 has the rules.

use std::ffi::OsStr;
use std::fs::{self, Metadata};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::thread;

use tab::OsLinks;

/// The extensions of the files that run, or lead elsewhere, when the OS opens them, whether or
/// not an execute bit says so: apps, scripts Terminal runs, its settings, which may run a
/// command, and the shortcuts and locations that open another URL.
#[cfg(target_os = "macos")]
const RUNS: &[&str] = &[
    "app", "command", "terminal", "tool", "webloc", "inetloc", "fileloc", "afploc", "ftploc",
    "mailloc", "newsloc", "url", "jar", "workflow", "action",
];
/// Launchers, programs that run without an execute bit, and the Windows programs Wine runs where
/// it is installed.
#[cfg(all(unix, not(target_os = "macos")))]
const RUNS: &[&str] = &[
    "desktop", "jar", "appimage", "run", "exe", "msi", "bat", "cmd", "com", "lnk", "scr",
];
/// The types `PATHEXT` names by default, and the other types Windows runs or follows: shortcuts,
/// installers, scripts and the like.
#[cfg(windows)]
const RUNS: &[&str] = &[
    "com",
    "exe",
    "bat",
    "cmd",
    "vbs",
    "vbe",
    "js",
    "jse",
    "wsf",
    "wsh",
    "msc",
    "lnk",
    "url",
    "scf",
    "hta",
    "msi",
    "msp",
    "pif",
    "cpl",
    "reg",
    "jar",
    "ps1",
    "scr",
    "chm",
    "xll",
    "gadget",
    "website",
    "diagcab",
    "appinstaller",
    "appx",
    "appxbundle",
    "msix",
    "msixbundle",
    "library-ms",
    "search-ms",
    "appref-ms",
    "application",
    "settingcontent-ms",
];

/// The OS's handlers for links.
pub(super) struct Links;

#[cfg_attr(
    not(test),
    expect(dead_code, reason = "#31 gives it to each tab with its Host")
)]
impl Links {
    pub(super) fn new() -> Self {
        Self
    }
}

impl OsLinks for Links {
    fn open(&self, url: &str) {
        let url = url.to_string();
        // A thread that cannot be made opens nothing, and nothing more can be done.
        let _ = thread::Builder::new()
            .name("link".to_string())
            .spawn(move || run(&url));
    }
}

/// Opens `url` and waits for the command that does it. What the command writes is dropped, and
/// a command that is missing or fails opens nothing.
fn run(url: &str) {
    if let Some(mut command) = command(url) {
        let _ = command
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status();
    }
}

/// The command that opens `url`: the OS's own, or for a file that would run, the one that shows
/// it in the file manager. A file is judged by the file it leads to through every symbolic link,
/// which is what the OS would open, and opened by that path, so that no other reading of the URL
/// opens another. None for a file that is not there.
pub(super) fn command(url: &str) -> Option<Command> {
    let Some(path) = url.strip_prefix("file://") else {
        return Some(open(OsStr::new(url), true));
    };
    let path = usual(fs::canonicalize(path_of(path)?).ok()?)?;
    let metadata = fs::metadata(&path).ok()?;
    Some(if runs(&path, &metadata) {
        reveal(&path)
    } else {
        open(path.as_os_str(), false)
    })
}

/// The path a file link names, what follows its `file://`: up to its query or fragment, with
/// every `%XX` read back as its byte. On Windows `/C:/x` is `C:\x`. None for one that names no
/// path this OS has.
pub(super) fn path_of(url: &str) -> Option<PathBuf> {
    let end = url.find(['?', '#']).unwrap_or(url.len());
    let bytes = decoded(&url[..end]);
    #[cfg(unix)]
    {
        use std::os::unix::ffi::OsStringExt;
        let path = PathBuf::from(std::ffi::OsString::from_vec(bytes));
        path.is_absolute().then_some(path)
    }
    #[cfg(windows)]
    {
        let text = String::from_utf8(bytes).ok()?;
        let path = PathBuf::from(text.strip_prefix('/')?.replace('/', "\\"));
        usual(path).filter(|path| path.is_absolute())
    }
}

/// `path` as the OS's handlers take it. On Windows it must be on a drive, and loses the `\\?\`
/// that a canonical path has before it: a share, which the OS would reach over the network, or a
/// device, is none.
#[cfg(windows)]
fn usual(path: PathBuf) -> Option<PathBuf> {
    use std::path::{Component, Prefix};
    let Some(Component::Prefix(prefix)) = path.components().next() else {
        return None;
    };
    match prefix.kind() {
        Prefix::Disk(_) => Some(path),
        Prefix::VerbatimDisk(_) => Some(PathBuf::from(path.to_str()?.strip_prefix(r"\\?\")?)),
        _ => None,
    }
}

/// `path` as the OS's handlers take it, which on macOS and Linux is as it is.
#[cfg(unix)]
fn usual(path: PathBuf) -> Option<PathBuf> {
    Some(path)
}

/// `text` with every `%XX` read back as its byte.
fn decoded(text: &str) -> Vec<u8> {
    let bytes = text.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut at = 0;
    while at < bytes.len() {
        let hex = bytes.get(at + 1..at + 3).and_then(|hex| {
            let digit = |byte: u8| char::from(byte).to_digit(16);
            Some(digit(hex[0])? * 16 + digit(hex[1])?)
        });
        match (bytes[at], hex) {
            (b'%', Some(byte)) => {
                out.push(byte as u8);
                at += 3;
            }
            (byte, _) => {
                out.push(byte);
                at += 1;
            }
        }
    }
    out
}

/// Whether the OS's handler would run what is at `path`, or follow it to another URL, rather
/// than open it: a file with an execute bit, a bundle or an alias on macOS, a file whose
/// extension says it runs, or anything that is neither a file nor a folder, such as a pipe,
/// which no handler opens as a document.
pub(super) fn runs(path: &Path, metadata: &Metadata) -> bool {
    if !metadata.is_file() && !metadata.is_dir() {
        return true;
    }
    let extension = path
        .extension()
        .and_then(OsStr::to_str)
        .map(str::to_ascii_lowercase);
    if extension
        .as_deref()
        .is_some_and(|extension| RUNS.contains(&extension))
    {
        return true;
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        if metadata.is_file() && metadata.permissions().mode() & 0o111 != 0 {
            return true;
        }
    }
    #[cfg(target_os = "macos")]
    {
        // Every app, installer and plug-in bundle has one, whatever it is named.
        if metadata.is_dir() && path.join("Contents").join("Info.plist").exists() {
            return true;
        }
        if metadata.is_file() && is_alias(path) {
            return true;
        }
    }
    false
}

/// Whether the file at `path` is a Finder alias, which opens what it points to, whatever its
/// name: its data starts as a bookmark's does, `book`, four bytes, then `mark`.
#[cfg(target_os = "macos")]
fn is_alias(path: &Path) -> bool {
    use std::io::Read;
    let mut start = [0; 12];
    let read = fs::File::open(path).and_then(|mut file| file.read_exact(&mut start));
    read.is_ok() && start.starts_with(b"book") && start.ends_with(b"mark")
}

/// The OS's own command for opening `target` in the default handler for its kind: a URL, or
/// when not `url`, the path of a file. Neither starts with `-`, so no command takes it for an
/// option.
fn open(target: &OsStr, url: bool) -> Command {
    #[cfg(target_os = "macos")]
    {
        let mut command = Command::new("/usr/bin/open");
        // `-u` takes a URL as one even when it would also name a file.
        if url {
            command.arg("-u");
        }
        command.arg(target);
        command
    }
    #[cfg(all(unix, not(target_os = "macos")))]
    {
        let _ = url;
        let mut command = Command::new("xdg-open");
        command.arg(target);
        command
    }
    #[cfg(windows)]
    {
        let _ = url;
        let mut command = Command::new(windows_program("System32\\rundll32.exe"));
        command.arg("url.dll,FileProtocolHandler").arg(target);
        command
    }
}

/// The OS's own command for showing `path` in the file manager without opening it. On Linux no
/// file manager is sure to select a file, so the folder that holds it is opened.
fn reveal(path: &Path) -> Command {
    #[cfg(target_os = "macos")]
    {
        let mut command = Command::new("/usr/bin/open");
        command.arg("-R").arg(path);
        command
    }
    #[cfg(all(unix, not(target_os = "macos")))]
    {
        let mut command = Command::new("xdg-open");
        command.arg(path.parent().unwrap_or(path));
        command
    }
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        // A path on Windows holds no quote, so it is written as Explorer reads it.
        let mut command = Command::new(windows_program("explorer.exe"));
        command.raw_arg(format!("/select,\"{}\"", path.display()));
        command
    }
}

/// A program in the Windows folder, which `SystemRoot` names, so no other program of the name
/// on the `PATH` runs in its place.
#[cfg(windows)]
fn windows_program(name: &str) -> PathBuf {
    let root = std::env::var_os("SystemRoot").unwrap_or_else(|| "C:\\Windows".into());
    Path::new(&root).join(name)
}
