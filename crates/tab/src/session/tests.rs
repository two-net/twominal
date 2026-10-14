//! Tests of the Session (SW-01, SW-02, FISH-01, FISH-02, TERM-15, SEC-03). Those that start a
//! shell run `/bin/sh` on a real pseudo-terminal, so they are for macOS and Linux.

use std::fs;
use std::sync::atomic::AtomicUsize;

use super::*;

/// How long a test waits for a shell, or a thread, before it fails.
const PATIENCE: Duration = Duration::from_secs(10);

/// A folder for one test, removed when the test ends.
struct Folder(PathBuf);

impl Folder {
    fn new() -> Self {
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let number = NEXT.fetch_add(1, Ordering::Relaxed);
        let name = format!("twominal-session-{}-{number}", std::process::id());
        let path = env::temp_dir().join(name);
        fs::create_dir_all(&path).unwrap();
        Self(path)
    }
}

impl Drop for Folder {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

#[test]
fn a_utf8_locale_only_when_none_is_set() {
    fn given(environment: &[(&str, &str)]) -> Option<(&'static str, &'static str)> {
        locale(|key| {
            environment
                .iter()
                .find(|(name, _)| *name == key)
                .map(|(_, value)| OsStr::new(*value))
        })
    }
    let fallback = Some(("LANG", "C.UTF-8"));

    assert_eq!(given(&[]), fallback);
    assert_eq!(given(&[("LANG", ""), ("LC_ALL", "")]), fallback);
    // Only these three decide how characters are read.
    assert_eq!(given(&[("LC_MESSAGES", "fr_FR.UTF-8")]), fallback);

    assert_eq!(given(&[("LANG", "en_NZ.UTF-8")]), None);
    assert_eq!(given(&[("LC_CTYPE", "UTF-8")]), None);
    // The user's choice, even one that is not UTF-8.
    assert_eq!(given(&[("LC_ALL", "C")]), None);
}

#[test]
fn the_exit_waits_for_the_last_output() {
    // Whether the shell can end before its output is read is the OS's choice, so the shell
    // tests may not see this. The reader's states are set here by hand.
    let settled = |reading: &Arc<Reading>| {
        let (sender, receiver) = mpsc::channel();
        let reading = reading.clone();
        thread::spawn(move || {
            reading.settle();
            let _ = sender.send(());
        });
        receiver
    };

    // The reader has come to the end of the output.
    let reading = Arc::new(Reading::default());
    reading.set(State::Done);
    assert!(settled(&reading).recv_timeout(PATIENCE).is_ok());

    // The reader is passing output on to a slow tab, for longer than the grace.
    let reading = Arc::new(Reading::default());
    let receiver = settled(&reading);
    thread::sleep(3 * GRACE);
    assert!(
        receiver.try_recv().is_err(),
        "the exit came before the output"
    );
    reading.set(State::Waiting(Instant::now()));
    reading.set(State::Busy);
    thread::sleep(3 * GRACE);
    assert!(
        receiver.try_recv().is_err(),
        "the exit came before the output"
    );
    reading.set(State::Done);
    assert!(receiver.recv_timeout(PATIENCE).is_ok());

    // The reader waits for output, and none comes.
    let reading = Arc::new(Reading::default());
    let started = Instant::now();
    reading.set(State::Waiting(started));
    assert!(settled(&reading).recv_timeout(PATIENCE).is_ok());
    assert!(started.elapsed() >= GRACE, "{:?}", started.elapsed());
}

/// Makes the file at `path` executable, as a program must be on macOS and Linux.
fn make_executable(path: &Path) {
    fs::write(path, "").unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(path, fs::Permissions::from_mode(0o755)).unwrap();
    }
}

#[test]
fn the_shell_setting_names_the_shell() {
    let words = vec!["/bin/zsh".to_string(), "-l".to_string()];
    assert_eq!(Shell::from(Some(words.clone())), Shell::Command(words));
    assert_eq!(Shell::from(None), Shell::Default);
    assert_eq!(Shell::from(Some(Vec::new())), Shell::Default);
}

#[test]
fn a_missing_program_is_not_found() {
    let (first, second) = (Folder::new(), Folder::new());
    let path = env::join_paths([&first.0, &second.0]).unwrap();
    let problem = |program: &str| problem(OsStr::new(program), Some(&path), None, &first.0);
    let not_found = |searched: &[&PathBuf]| Problem::NotFound {
        searched: searched.iter().map(|folder| folder.to_path_buf()).collect(),
    };

    assert_eq!(problem("fish"), Some(not_found(&[&first.0, &second.0])));
    // A path is looked for there, and nowhere else.
    let missing = second.0.join("fish");
    assert_eq!(problem(missing.to_str().unwrap()), Some(not_found(&[])));
    assert_eq!(problem("bin/fish"), Some(not_found(&[])));
    // With no `PATH` there is nowhere to look.
    assert_eq!(
        super::problem(OsStr::new("fish"), None, None, &first.0),
        Some(not_found(&[]))
    );
}

#[test]
fn a_program_that_is_there_is_found() {
    let (first, second) = (Folder::new(), Folder::new());
    let path = env::join_paths([&first.0, &second.0]).unwrap();
    let problem = |program: &str| problem(OsStr::new(program), Some(&path), None, &first.0);

    make_executable(&second.0.join("fish"));
    assert_eq!(problem("fish"), None);
    let full = second.0.join("fish");
    assert_eq!(problem(full.to_str().unwrap()), None);
    // A relative path is relative to the shell's directory.
    fs::create_dir(first.0.join("bin")).unwrap();
    make_executable(&first.0.join("bin").join("zsh"));
    assert_eq!(problem("bin/zsh"), None);
    assert_eq!(problem("./bin/zsh"), None);
}

#[test]
fn a_folder_cannot_run() {
    let folder = Folder::new();
    fs::create_dir(folder.0.join("fish")).unwrap();
    let path = env::join_paths([&folder.0]).unwrap();
    let cannot = Some(Problem::CannotRun("it is a folder".to_string()));

    assert_eq!(
        problem(OsStr::new("fish"), Some(&path), None, &folder.0),
        cannot
    );
    let full = folder.0.join("fish");
    assert_eq!(problem(full.as_os_str(), None, None, &folder.0), cannot);
}

#[cfg(unix)]
#[test]
fn a_file_that_is_not_executable_cannot_run() {
    let (first, second) = (Folder::new(), Folder::new());
    fs::write(first.0.join("fish"), "").unwrap();
    let cannot = Some(Problem::CannotRun("it is not executable".to_string()));

    let path = env::join_paths([&first.0]).unwrap();
    assert_eq!(
        problem(OsStr::new("fish"), Some(&path), None, &first.0),
        cannot
    );
    let full = first.0.join("fish");
    assert_eq!(problem(full.as_os_str(), None, None, &first.0), cannot);

    // One that can run, further along `PATH`, is the one the OS starts.
    make_executable(&second.0.join("fish"));
    let path = env::join_paths([&first.0, &second.0]).unwrap();
    assert_eq!(
        problem(OsStr::new("fish"), Some(&path), None, &first.0),
        None
    );
}

#[test]
fn windows_names_are_tried_with_each_extension() {
    let names = |extensions: Option<&str>| {
        super::names(OsStr::new("pwsh"), extensions.map(OsStr::new))
            .map(|name| name.into_string().unwrap())
            .collect::<Vec<_>>()
    };
    assert_eq!(names(None), ["pwsh"]);
    assert_eq!(
        names(Some(".COM;.EXE;;.BAT")),
        ["pwsh", "pwsh.COM", "pwsh.EXE", "pwsh.BAT"]
    );

    let folder = Folder::new();
    make_executable(&folder.0.join("pwsh.EXE"));
    let path = env::join_paths([&folder.0]).unwrap();
    let extensions = OsStr::new(".COM;.EXE");
    let found = problem(OsStr::new("pwsh"), Some(&path), Some(extensions), &folder.0);
    assert_eq!(found, None);
}

#[test]
fn a_shell_that_did_not_start_offers_another() {
    // The OS's own shell is the last, so the offers never go round in a circle.
    assert_eq!(Shell::Fallback.instead(), None);

    let offer = Shell::Default.instead().unwrap();
    assert_eq!(offer.shell, Shell::Fallback);
    assert_eq!(offer.program, program(&Shell::Fallback).get_shell());

    // The Shell setting gives way to the default shell, unless that would not start either.
    let offer = Shell::Command(vec!["fish".to_string()]).instead().unwrap();
    if refused(&program(&Shell::Default)).is_none() {
        assert_eq!(offer.shell, Shell::Default);
        assert_eq!(offer.program, name(&program(&Shell::Default)));
    } else {
        assert_eq!(offer.shell, Shell::Fallback);
    }
}

#[test]
fn says_why_a_shell_did_not_start() {
    let said = |program: &str, problem| {
        NotStarted {
            program: program.to_string(),
            problem,
            instead: None,
        }
        .to_string()
    };
    let not_found = |searched: &[&str]| Problem::NotFound {
        searched: searched.iter().map(PathBuf::from).collect(),
    };

    assert_eq!(
        said("/opt/homebrew/bin/fish", not_found(&[])),
        "The shell /opt/homebrew/bin/fish was not found."
    );
    assert_eq!(
        said(
            "fish",
            not_found(&["/usr/bin", "/bin", "/usr/sbin", "/sbin"])
        ),
        "The shell fish was not found in /usr/bin, /bin, /usr/sbin or /sbin. \
         Write its full path in the Shell setting."
    );
    assert_eq!(
        said("fish", not_found(&["/usr/bin", "/bin"])),
        "The shell fish was not found in /usr/bin or /bin. Write its full path in the Shell setting."
    );
    assert_eq!(
        said("fish", not_found(&["/usr/bin"])),
        "The shell fish was not found in /usr/bin. Write its full path in the Shell setting."
    );
    assert_eq!(
        said("/etc", Problem::CannotRun("it is a folder".to_string())),
        "The shell /etc could not be started: it is a folder."
    );
}

#[test]
fn powershell_7_comes_first() {
    let (empty, with_pwsh) = (Folder::new(), Folder::new());
    let pwsh = with_pwsh.0.join("pwsh.exe");
    fs::write(&pwsh, "").unwrap();

    let path = env::join_paths([&empty.0, &with_pwsh.0]).unwrap();
    assert_eq!(powershell(Some(&path)), pwsh);

    let path = env::join_paths([&empty.0]).unwrap();
    assert_eq!(powershell(Some(&path)), PathBuf::from("powershell.exe"));
    assert_eq!(powershell(None), PathBuf::from("powershell.exe"));
}

// Shell integration (FISH-04 to FISH-06).

/// The opening of every report.
const REPORT: &[u8] = b"\x1b]2222;";

/// A report: its key, and its value decoded, when it has one.
type Report = (String, Option<String>);

/// The reports in `text` that carry `nonce`, in order. Each one must keep to the format: no
/// byte outside `!` to `~` but the spaces between the names of a list.
fn reports(text: &[u8], nonce: &str) -> Vec<Report> {
    let mut found = Vec::new();
    let mut rest = text;
    while let Some(at) = rest
        .windows(REPORT.len())
        .position(|window| window == REPORT)
    {
        rest = &rest[at + REPORT.len()..];
        let Some(end) = rest.iter().position(|&byte| byte == 0x07) else {
            break;
        };
        let (body, after) = rest.split_at(end);
        rest = &after[1..];
        assert!(
            body.iter().all(|byte| (b' '..=b'~').contains(byte)),
            "a report holds only printable ASCII: {:?}",
            String::from_utf8_lossy(body)
        );
        let mut fields = body.splitn(3, |&byte| byte == b';');
        if fields.next() != Some(nonce.as_bytes()) {
            continue;
        }
        let key = String::from_utf8_lossy(fields.next().unwrap_or_default()).into_owned();
        found.push((key, fields.next().map(decode)));
    }
    found
}

/// A report's value, with each `%XX` read back as its byte.
fn decode(value: &[u8]) -> String {
    let mut bytes = Vec::new();
    let mut rest = value;
    while let Some((&byte, after)) = rest.split_first() {
        match (byte, after) {
            (b'%', [high, low, after @ ..]) => {
                let hex = std::str::from_utf8(&[*high, *low]).unwrap().to_string();
                bytes.push(u8::from_str_radix(&hex, 16).expect("two hex digits after %"));
                rest = after;
            }
            _ => {
                bytes.push(byte);
                rest = after;
            }
        }
    }
    String::from_utf8(bytes).expect("a value is UTF-8")
}

/// The names in a list report.
fn names(value: &str) -> Vec<&str> {
    value.split(' ').filter(|name| !name.is_empty()).collect()
}

#[test]
fn reports_are_read_back_as_written() {
    let text = b"out\x1b]2222;n1;cwd;/a%20b%3Bc%25d%C3%A9\x07put\x1b]2222;n2;ready\x07\
                 \x1b]2222;n1;aliases;ll la\x07\x1b]2222;n1;ready\x07";
    assert_eq!(
        reports(text, "n1"),
        [
            ("cwd".to_string(), Some("/a b;c%dé".to_string())),
            ("aliases".to_string(), Some("ll la".to_string())),
            ("ready".to_string(), None),
        ]
    );
}

#[test]
fn shells_are_told_by_their_names() {
    use integration::Kind;
    let kind = |program: &str| Kind::of(OsStr::new(program));

    assert_eq!(kind("zsh"), Some(Kind::Zsh));
    assert_eq!(kind("/bin/zsh"), Some(Kind::Zsh));
    assert_eq!(kind("/opt/homebrew/bin/bash"), Some(Kind::Bash));
    assert_eq!(kind("pwsh"), Some(Kind::PowerShell));
    assert_eq!(kind("PWSH.EXE"), Some(Kind::PowerShell));
    assert_eq!(kind("powershell.exe"), Some(Kind::PowerShell));
    assert_eq!(kind("PowerShell"), Some(Kind::PowerShell));
    // These run in passthrough (FISH-03, FISH-08).
    for other in [
        "sh",
        "/bin/sh",
        "dash",
        "fish",
        "/opt/homebrew/bin/fish",
        "cmd.exe",
        "zsh5",
    ] {
        assert_eq!(kind(other), None, "{other}");
    }
}

#[test]
fn integration_is_added_only_to_arguments_that_leave_startup_alone() {
    use integration::Kind;
    let takes = |kind: Kind, arguments: &[&str]| {
        let arguments: Vec<OsString> = arguments.iter().map(OsString::from).collect();
        kind.takes(&arguments)
    };

    for kind in [Kind::Zsh, Kind::Bash] {
        assert!(takes(kind, &[]));
        assert!(takes(kind, &["-l"]));
        assert!(takes(kind, &["--login", "-i"]));
        assert!(!takes(kind, &["-c", "echo hello"]));
        assert!(!takes(kind, &["script.sh"]));
        assert!(!takes(kind, &["-f"]));
        assert!(!takes(kind, &["--norc"]));
    }
    assert!(takes(Kind::PowerShell, &[]));
    assert!(takes(
        Kind::PowerShell,
        &["-NoLogo", "-noprofile", "-LOGIN"]
    ));
    assert!(!takes(Kind::PowerShell, &["-Command", "Get-Date"]));
    assert!(!takes(Kind::PowerShell, &["-File", "script.ps1"]));
}

#[test]
fn powershell_is_given_its_script_encoded() {
    // `-EncodedCommand` takes UTF-16LE, in base64.
    assert_eq!(integration::encoded(""), "");
    assert_eq!(integration::encoded("H"), "SAA=");
    assert_eq!(integration::encoded("Hi"), "SABpAA==");
    assert_eq!(integration::encoded("Hi!"), "SABpACEA");
    assert_eq!(integration::encoded("é"), "6QA=");
}

#[test]
fn every_session_has_a_nonce_of_its_own() {
    let (one, two) = (integration::nonce(), integration::nonce());
    assert_ne!(one, two);
    for nonce in [one, two] {
        assert_eq!(nonce.len(), 32, "{nonce}");
        assert!(
            nonce.bytes().all(|byte| byte.is_ascii_hexdigit()),
            "{nonce}"
        );
    }
}

#[test]
fn the_files_are_written_once_and_kept() {
    let private = Folder::new();
    let folder = integration::files(&private.0).unwrap();
    let zshenv = folder.join("zsh").join(".zshenv");
    let rcfile = folder.join("integration.bash");
    let written = (fs::read(&zshenv).unwrap(), fs::read(&rcfile).unwrap());
    assert!(String::from_utf8_lossy(&written.0).contains("ZDOTDIR"));
    assert!(String::from_utf8_lossy(&written.1).contains("PROMPT_COMMAND"));

    // The same folder again, with nothing left over from writing it.
    assert_eq!(integration::files(&private.0).unwrap(), folder);
    let mut entries: Vec<_> = fs::read_dir(&private.0)
        .unwrap()
        .map(|entry| entry.unwrap().file_name())
        .collect();
    assert_eq!(entries, [folder.file_name().unwrap()]);
    entries = fs::read_dir(&folder)
        .unwrap()
        .map(|entry| entry.unwrap().file_name())
        .collect();
    entries.sort();
    assert_eq!(entries, [OsStr::new("integration.bash"), OsStr::new("zsh")]);

    // A file that no longer holds what it should is written again.
    fs::write(&zshenv, "echo changed").unwrap();
    integration::files(&private.0).unwrap();
    assert_eq!(fs::read(&zshenv).unwrap(), written.0);
}

/// The Start a tab gives for `shell`, with shell integration on and `private` as its folder.
fn integrated_start(shell: Shell, private: &Path) -> Start {
    Start {
        shell,
        directory: None,
        size: Size { cols: 80, rows: 24 },
        name: "Twominal".to_string(),
        version: "9.9.9".to_string(),
        integration: true,
        private: Some(private.to_path_buf()),
    }
}

fn words(words: &[&str]) -> Shell {
    Shell::Command(words.iter().map(|word| word.to_string()).collect())
}

#[test]
fn zsh_is_pointed_at_twominals_zdotdir() {
    let private = Folder::new();
    let start = integrated_start(words(&["/bin/zsh", "-l"]), &private.0);
    let folder = integration::files(&private.0).unwrap();

    let mut command = command(&start);
    command.env_remove("ZDOTDIR");
    let nonce = integrate(&start, &mut command).expect("zsh has integration");
    assert_eq!(command.get_env("twominal_nonce"), Some(OsStr::new(&nonce)));
    assert_eq!(
        command.get_env("ZDOTDIR"),
        Some(folder.join("zsh").as_os_str())
    );
    assert_eq!(command.get_env("twominal_zdotdir"), None);
    // Only the environment changes, so a login shell stays one.
    assert_eq!(command.get_argv(), &["/bin/zsh", "-l"]);

    // The user's own ZDOTDIR, for the script to give back.
    let mut command = super::command(&start);
    command.env("ZDOTDIR", "/home/someone/.config/zsh");
    integrate(&start, &mut command).unwrap();
    assert_eq!(
        command.get_env("twominal_zdotdir"),
        Some(OsStr::new("/home/someone/.config/zsh"))
    );
}

#[test]
fn bash_is_given_its_rcfile() {
    let private = Folder::new();
    let rcfile = integration::files(&private.0)
        .unwrap()
        .join("integration.bash");

    // As the Shell setting, without -l: not a login shell.
    let start = integrated_start(words(&["bash"]), &private.0);
    let mut command = command(&start);
    integrate(&start, &mut command).expect("bash has integration");
    assert_eq!(
        command.get_argv(),
        &[
            OsStr::new("bash"),
            OsStr::new("--rcfile"),
            rcfile.as_os_str()
        ]
    );
    assert_eq!(command.get_env("twominal_login"), None);

    // With -l, the script reads the login files.
    let start = integrated_start(words(&["bash", "-l"]), &private.0);
    let mut command = super::command(&start);
    integrate(&start, &mut command).unwrap();
    assert_eq!(
        command.get_argv()[1..],
        ["--rcfile", rcfile.to_str().unwrap()]
    );
    assert_eq!(command.get_env("twominal_login"), Some(OsStr::new("1")));
}

#[cfg(unix)]
#[test]
fn the_default_shell_gets_integration_as_a_login_shell() {
    let private = Folder::new();
    let start = integrated_start(Shell::Default, &private.0);

    let mut command = command(&start);
    command.env("SHELL", "/bin/bash");
    integrate(&start, &mut command).expect("bash has integration");
    assert_eq!(command.get_argv()[..2], ["/bin/bash", "--rcfile"]);
    assert_eq!(command.get_env("twominal_login"), Some(OsStr::new("1")));

    // zsh keeps its `-zsh`.
    let mut command = super::command(&start);
    command.env("SHELL", "/bin/zsh");
    integrate(&start, &mut command).expect("zsh has integration");
    assert!(command.is_default_prog());

    // A default shell that cannot start is explained, not replaced (FISH-02).
    let mut command = super::command(&start);
    command.env("SHELL", private.0.join("bash"));
    assert_eq!(integrate(&start, &mut command), None);
    assert!(command.is_default_prog());
}

#[test]
fn powershell_is_given_its_script_on_the_command_line() {
    let start = integrated_start(words(&["pwsh", "-NoLogo"]), Path::new("/nowhere"));
    let mut command = command(&start);
    let nonce = integrate(&start, &mut command).expect("PowerShell has integration");
    let argv = command.get_argv();
    assert_eq!(argv[..3], ["pwsh", "-NoLogo", "-NoExit"]);
    assert_eq!(argv[3], "-EncodedCommand");
    assert!(argv[4].to_str().unwrap().len() > 1000);
    assert_eq!(command.get_env("twominal_nonce"), Some(OsStr::new(&nonce)));

    // With integration off, Windows still sends the script, which sets UTF-8 (SW-02), but
    // no nonce, so nothing is reported.
    let mut start = start;
    start.integration = false;
    let mut command = super::command(&start);
    assert_eq!(integrate(&start, &mut command), None);
    assert_eq!(command.get_argv().len() > 2, cfg!(windows));
    assert_eq!(command.get_env("twominal_nonce"), None);
}

#[test]
fn no_integration_leaves_the_command_as_it_was() {
    let private = Folder::new();
    let unchanged = |start: &Start| {
        let mut command = command(start);
        let before = (
            command.get_argv().clone(),
            command.get_env("ZDOTDIR").map(OsStr::to_os_string),
        );
        assert_eq!(integrate(start, &mut command), None);
        assert_eq!(
            (
                command.get_argv().clone(),
                command.get_env("ZDOTDIR").map(OsStr::to_os_string)
            ),
            before
        );
        assert_eq!(command.get_env("twominal_nonce"), None);
    };

    // The setting is off (FISH-06).
    for shell in [words(&["zsh"]), words(&["bash"])] {
        let mut start = integrated_start(shell, &private.0);
        start.integration = false;
        unchanged(&start);
    }
    // Twominal has no integration for the shell (FISH-03), or not with these arguments.
    for shell in [
        words(&["/bin/sh"]),
        words(&["fish"]),
        words(&["bash", "-c", "echo hello"]),
        words(&["zsh", "-f"]),
        words(&["pwsh", "-Command", "Get-Date"]),
    ] {
        unchanged(&integrated_start(shell, &private.0));
    }
    // zsh and bash read their integration from a file, which needs a folder.
    for shell in [words(&["zsh"]), words(&["bash"])] {
        let mut start = integrated_start(shell, &private.0);
        start.private = None;
        unchanged(&start);
    }
}

/// The tests that start a shell.
#[cfg(unix)]
mod shells {
    use std::sync::mpsc::{sync_channel, RecvTimeoutError};

    use super::*;

    /// The size a test's terminal starts with.
    const SIZE: Size = Size {
        cols: 100,
        rows: 30,
    };

    fn start_with(shell: Option<&[&str]>, directory: Option<PathBuf>) -> Start {
        let words = shell.map(|words| words.iter().map(|word| word.to_string()).collect());
        Start {
            shell: Shell::from(words),
            directory,
            size: SIZE,
            name: "Twominal".to_string(),
            version: "9.9.9".to_string(),
            integration: false,
            private: None,
        }
    }

    /// A sink that a test reads from, holding at most `room` messages before the session waits.
    fn sink(room: usize) -> (Sink, Receiver<Output>) {
        let (sender, receiver) = sync_channel(room);
        let sink: Sink = Arc::new(move |output| {
            let _ = sender.send(output);
        });
        (sink, receiver)
    }

    /// Starts `shell` in `directory`, and returns what the session says.
    fn run_in(shell: &[&str], directory: Option<PathBuf>) -> (Session, Receiver<Output>) {
        let (sink, output) = sink(16);
        let start = start_with(Some(shell), directory);
        let session =
            spawn(&start.shell, command(&start), start.size, None, sink).expect("the shell starts");
        (session, output)
    }

    fn run(shell: &[&str]) -> (Session, Receiver<Output>) {
        run_in(shell, None)
    }

    /// Everything the session says until the shell has ended: the output as text, and how the
    /// shell ended.
    fn finish(output: &Receiver<Output>) -> (String, Exit) {
        let mut text = Vec::new();
        loop {
            match output
                .recv_timeout(PATIENCE)
                .expect("the shell ends in time")
            {
                Output::Bytes(bytes) => text.extend(bytes),
                Output::Exited(exit) => {
                    let exit = exit.expect("the shell's exit status");
                    return (String::from_utf8_lossy(&text).into_owned(), exit);
                }
            }
        }
    }

    /// Reads output until it holds `wanted`. What a test types is echoed back, so `wanted` must
    /// be something only the shell can write.
    fn until(output: &Receiver<Output>, wanted: &str) {
        let mut text = Vec::new();
        while !String::from_utf8_lossy(&text).contains(wanted) {
            match output.recv_timeout(PATIENCE) {
                Ok(Output::Bytes(bytes)) => text.extend(bytes),
                other => panic!(
                    "waiting for {wanted:?} after {:?}, got {other:?}",
                    String::from_utf8_lossy(&text)
                ),
            }
        }
    }

    #[test]
    fn bytes_go_both_ways() {
        let (session, output) = run(&["/bin/sh"]);
        session.write(b"echo $((40 + 2))\n".to_vec());
        until(&output, "42");
        session.write(b"exit 3\n".to_vec());
        assert_eq!(finish(&output).1, Exit::Status(3));
    }

    #[test]
    fn the_shell_gets_the_terminal_environment() {
        let script =
            r#"printf '%s|%s|%s|%s' "$TERM" "$COLORTERM" "$TERM_PROGRAM" "$TERM_PROGRAM_VERSION""#;
        let (_session, output) = run(&["/bin/sh", "-c", script]);
        let (text, exit) = finish(&output);
        assert_eq!(text, "xterm-256color|truecolor|Twominal|9.9.9");
        assert!(exit.success());
    }

    #[test]
    fn the_default_shell_is_a_login_shell() {
        let mut command = command(&start_with(None, None));
        assert!(command.is_default_prog());

        // Whichever shell the user has, the test runs `sh`, which is the same everywhere.
        command.env("SHELL", "/bin/sh");
        let (sink, output) = sink(16);
        let session = spawn(&Shell::Default, command, SIZE, None, sink).expect("the shell starts");
        session.write(b"echo \"[$0]\"\n".to_vec());
        until(&output, "[-sh]");
    }

    #[test]
    fn starts_in_the_given_directory() {
        // On macOS the temporary folder is reached through a symbolic link, `/var`, and the
        // path stays as it was given.
        let folder = Folder::new();
        let (_session, output) = run_in(&["/bin/sh", "-c", "pwd"], Some(folder.0.clone()));
        assert_eq!(finish(&output).0.trim_end(), folder.0.to_str().unwrap());
    }

    #[test]
    fn starts_at_home_without_a_directory() {
        let home = fs::canonicalize(env::var_os("HOME").expect("a home directory")).unwrap();
        let (_session, output) = run(&["/bin/sh", "-c", "pwd -P"]);
        assert_eq!(finish(&output).0.trim_end(), home.to_str().unwrap());
    }

    #[test]
    fn tells_the_shell_its_size() {
        let (session, output) = run(&["/bin/sh"]);
        session.write(b"stty size\n".to_vec());
        until(&output, "30 100");

        session
            .resize(Size {
                cols: 120,
                rows: 40,
            })
            .unwrap();
        session.write(b"stty size\n".to_vec());
        until(&output, "40 120");

        session.resize(Size { cols: 0, rows: 0 }).unwrap();
        session.write(b"stty size\n".to_vec());
        until(&output, "1 1");
    }

    #[test]
    fn reports_how_the_shell_ended() {
        let exit = |script| finish(&run(&["/bin/sh", "-c", script]).1).1;

        assert!(exit("exit 0").success());
        assert_eq!(exit("exit 7"), Exit::Status(7));
        let killed = exit("kill -TERM $$");
        assert!(matches!(killed, Exit::Signal(_)), "{killed:?}");
        assert!(!killed.success());
    }

    #[test]
    fn exit_comes_after_the_last_output() {
        // Room for one message, and a tab that is slow to start reading. The reader passes on
        // `one`, waits with `two` for room, and the shell ends while `three` is still unread.
        let (sink, output) = sink(1);
        let script = "printf one; sleep 0.2; printf two; sleep 0.2; printf three";
        let start = start_with(Some(&["/bin/sh", "-c", script]), None);
        let _session =
            spawn(&start.shell, command(&start), start.size, None, sink).expect("the shell starts");
        thread::sleep(Duration::from_secs(1));

        let (text, exit) = finish(&output);
        assert_eq!(text, "onetwothree");
        assert!(exit.success());
        assert_eq!(
            output.recv_timeout(PATIENCE).unwrap_err(),
            RecvTimeoutError::Disconnected,
            "nothing comes after the exit"
        );
    }

    #[test]
    fn exit_is_reported_while_a_leftover_process_holds_the_terminal() {
        // The `sleep` ignores the hangup the shell's end brings, so it can keep the terminal open
        // after the shell. Whether it does is the OS's choice.
        let started = Instant::now();
        let script = "(trap '' HUP; sleep 5) & printf left; exit 0";
        let (_session, output) = run(&["/bin/sh", "-c", script]);
        let (text, exit) = finish(&output);
        assert_eq!(text, "left");
        assert!(exit.success());
        assert!(
            started.elapsed() < Duration::from_secs(3),
            "{:?}",
            started.elapsed()
        );
    }

    #[test]
    fn runs_with_the_users_own_privileges() {
        let own = std::process::Command::new("id").arg("-u").output().unwrap();
        let (_session, output) = run(&["/bin/sh", "-c", "id -u"]);
        assert_eq!(
            finish(&output).0.trim_end(),
            String::from_utf8_lossy(&own.stdout).trim_end()
        );
    }

    #[test]
    fn closing_hangs_up_the_shell() {
        // `sleep` reads nothing, so only the hangup can end this shell early.
        let (session, output) = run(&["/bin/sh", "-c", "echo $((6 * 7)); sleep 30"]);
        until(&output, "42");
        let closed = Instant::now();
        drop(session);

        let (_, exit) = finish(&output);
        assert!(matches!(exit, Exit::Signal(_)), "{exit:?}");
        assert!(
            closed.elapsed() < Duration::from_secs(5),
            "{:?}",
            closed.elapsed()
        );
    }

    /// Starts `command`, which runs `shell`, and returns why it did not start.
    fn refusal(shell: &Shell, command: CommandBuilder) -> NotStarted {
        let (sink, _output) = sink(16);
        match spawn(shell, command, SIZE, None, sink) {
            Err(Error::Shell(not_started)) => not_started,
            Err(Error::Session(error)) => panic!("the session failed: {error}"),
            Ok(_) => panic!("the shell started"),
        }
    }

    #[test]
    fn a_shell_setting_that_names_no_file_is_explained() {
        // AT-16: the other tabs keep working.
        let (other, output) = run(&["/bin/sh"]);

        let folder = Folder::new();
        let missing = folder.0.join("fish");
        let start = start_with(Some(&[missing.to_str().unwrap(), "-l"]), None);
        let refused = refusal(&start.shell, command(&start));
        assert_eq!(refused.program, missing.to_str().unwrap());
        assert_eq!(refused.problem, Problem::NotFound { searched: vec![] });
        let offered = refused.instead.expect("another shell is offered").shell;
        assert!(
            matches!(offered, Shell::Default | Shell::Fallback),
            "{offered:?}"
        );

        other.write(b"echo $((40 + 2))\n".to_vec());
        until(&output, "42");
    }

    #[test]
    fn says_where_a_shell_was_looked_for() {
        let folder = Folder::new();
        let start = start_with(Some(&["fish"]), None);
        let mut command = command(&start);
        command.env("PATH", &folder.0);
        let refused = refusal(&start.shell, command);
        assert_eq!(refused.program, "fish");
        assert_eq!(
            refused.problem,
            Problem::NotFound {
                searched: vec![folder.0.clone()]
            }
        );
    }

    #[test]
    fn a_shell_that_cannot_run_is_explained() {
        let folder = Folder::new();
        let file = folder.0.join("fish");
        fs::write(&file, "").unwrap();
        let start = start_with(Some(&[file.to_str().unwrap()]), None);
        let refused = refusal(&start.shell, command(&start));
        assert_eq!(
            refused.problem,
            Problem::CannotRun("it is not executable".to_string())
        );
    }

    #[test]
    fn a_missing_default_shell_is_explained_not_replaced() {
        // The user's login shell was a fish that has since been removed. portable-pty alone
        // would start `/bin/sh` in its place, and say nothing.
        let folder = Folder::new();
        let missing = folder.0.join("fish");
        let mut command = command(&start_with(None, None));
        command.env("SHELL", &missing);
        let refused = refusal(&Shell::Default, command);
        assert_eq!(refused.program, missing.to_str().unwrap());
        assert_eq!(refused.problem, Problem::NotFound { searched: vec![] });
        let offer = refused.instead.expect("another shell is offered");
        assert_eq!(offer.shell, Shell::Fallback);
        assert!(Path::new(&offer.program).is_file(), "{}", offer.program);
    }

    #[test]
    fn the_default_shell_is_checked_before_it_starts() {
        let mut command = program(&Shell::Default);
        command.env("SHELL", "/bin/sh");
        assert_eq!(refused(&command), None);
        command.env("SHELL", "");
        assert_eq!(refused(&command), None);
        command.env("SHELL", "/");
        let folder = Some((
            "/".to_string(),
            Problem::CannotRun("it is a folder".to_string()),
        ));
        assert_eq!(refused(&command), folder);

        // A shell the Shell setting names is started, and the OS says what is wrong with it.
        let mut command = program(&Shell::Command(vec!["/bin/sh".to_string()]));
        command.env("SHELL", "/");
        assert_eq!(refused(&command), None);
    }

    #[test]
    fn the_os_shell_is_a_login_shell_found_without_shell() {
        // It is the account's own shell, which differs from machine to machine, so it is not
        // started here.
        let command = program(&Shell::Fallback);
        assert!(command.is_default_prog());
        assert_eq!(command.get_env("SHELL"), None);
        assert!(Path::new(&command.get_shell()).is_file());
        assert_eq!(refused(&command), None);
    }

    // Shell integration with real shells (FISH-04 to FISH-06). Each test needs its shell, and
    // says so when this machine does not have it.

    /// Where `program` is installed, or `None`, with a note, when it is not.
    fn installed(program: &str) -> Option<PathBuf> {
        let path = env::var_os("PATH").unwrap_or_default();
        let found = env::split_paths(&path)
            .chain(["/bin", "/usr/bin", "/usr/local/bin", "/opt/homebrew/bin"].map(PathBuf::from))
            .map(|folder| folder.join(program))
            .find(|file| problem_at(file).is_none());
        if found.is_none() {
            eprintln!("{program} is not installed, so its shell integration is not tested");
        }
        found
    }

    /// The question .NET asks, at startup and at each prompt, about where the cursor is. A
    /// terminal answers it, and here the test does.
    const WHERE: &[u8] = b"\x1b[6n";

    /// How a test writes a question to a shell.
    #[derive(Clone, Copy)]
    enum Syntax {
        Posix,
        PowerShell,
    }

    /// A shell started with shell integration, as a tab starts it, in a home folder of the
    /// test's own.
    struct Integrated {
        session: Session,
        output: Receiver<Output>,
        /// Everything the shell has written.
        text: Vec<u8>,
        /// How many of the shell's questions about the cursor have been answered.
        answered: usize,
        /// How many `ready` reports the test has taken.
        taken: usize,
        nonce: String,
        syntax: Syntax,
    }

    impl Integrated {
        /// Starts `shell` in `home`, which is also its home folder, with `private` for the
        /// integration's files. `change` changes the environment first.
        fn start(
            shell: Shell,
            syntax: Syntax,
            home: &Path,
            private: &Path,
            change: impl FnOnce(&mut CommandBuilder),
        ) -> Self {
            let mut start = start_with(None, Some(home.to_path_buf()));
            start.shell = shell;
            start.integration = true;
            start.private = Some(private.to_path_buf());
            let mut command = command(&start);
            command.env("HOME", home);
            command.env("PATH", "/usr/bin:/bin:/usr/sbin:/sbin");
            command.env_remove("ZDOTDIR");
            command.env_remove("XDG_CONFIG_HOME");
            // PowerShell keeps its caches and history there. They are not the user's
            // configuration, and out of the home folder they leave it as it was.
            command.env("XDG_CACHE_HOME", private.join("cache"));
            command.env("XDG_DATA_HOME", private.join("data"));
            change(&mut command);

            let reports = integrate(&start, &mut command);
            let nonce = reports.clone().expect("the shell has shell integration");
            let (sink, output) = sink(64);
            let session =
                spawn(&start.shell, command, start.size, reports, sink).expect("the shell starts");
            Self {
                session,
                output,
                text: Vec::new(),
                answered: 0,
                taken: 0,
                nonce,
                syntax,
            }
        }

        /// Reads what the shell writes until `done` holds, and answers its questions about
        /// the cursor on the way.
        fn read_until(&mut self, what: &str, done: impl Fn(&Self) -> bool) {
            while !done(self) {
                match self.output.recv_timeout(PATIENCE) {
                    Ok(Output::Bytes(bytes)) => self.take(bytes),
                    other => panic!(
                        "waiting for {what}, got {other:?} after {:?}",
                        String::from_utf8_lossy(&self.text)
                    ),
                }
            }
        }

        /// Keeps what the shell wrote, and answers its questions about the cursor.
        fn take(&mut self, bytes: Vec<u8>) {
            self.text.extend(bytes);
            let asked = self
                .text
                .windows(WHERE.len())
                .filter(|window| *window == WHERE)
                .count();
            for _ in self.answered..asked {
                self.session.write(b"\x1b[1;1R".to_vec());
            }
            self.answered = asked;
        }

        fn reports(&self) -> Vec<Report> {
            reports(&self.text, &self.nonce)
        }

        /// The reports after the last `ready` taken, up to and with the next.
        fn next_ready(&mut self) -> Vec<Report> {
            let taken = self.taken;
            let readies = |reports: &[Report]| -> Vec<usize> {
                (0..reports.len())
                    .filter(|&i| reports[i].0 == "ready")
                    .collect()
            };
            self.read_until("ready", |tab| readies(&tab.reports()).len() > taken);
            self.taken += 1;
            let reports = self.reports();
            let at = readies(&reports);
            let from = taken.checked_sub(1).map_or(0, |last| at[last] + 1);
            let reports = reports[from..=at[taken]].to_vec();
            for (key, value) in &reports {
                assert_eq!(value.is_none(), key == "ready", "{key} {value:?}");
            }
            reports
        }

        /// Types `line`, then Enter.
        fn type_line(&self, line: &str) {
            self.session.write(format!("{line}\r").into_bytes());
        }

        /// What `expression` comes to in the shell, written in its syntax as it would be
        /// inside double quotes. Takes the `ready` that follows.
        fn ask(&mut self, expression: &str) -> String {
            let from = self.text.len();
            self.type_line(&match self.syntax {
                Syntax::Posix => format!(r#"echo "<$((40 + 2))>{expression}</$((40 + 2))>""#),
                Syntax::PowerShell => format!(r#""<$(40 + 2)>{expression}</$(40 + 2)>""#),
            });
            // What was typed is echoed, but only the shell can write 42.
            let answer = |tab: &Self| {
                let text = String::from_utf8_lossy(&tab.text[from..]).into_owned();
                let start = text.find("<42>")? + "<42>".len();
                let end = start + text[start..].find("</42>")?;
                Some(text[start..end].to_string())
            };
            self.read_until(expression, |tab| answer(tab).is_some());
            self.next_ready();
            answer(self).unwrap()
        }

        /// Runs `line`, which fails, and checks that the user's own prompt shows that it did.
        fn fails(&mut self, line: &str, prompt: &str) {
            let from = self.text.len();
            self.type_line(line);
            self.next_ready();
            self.read_until("the user's prompt", |tab| {
                String::from_utf8_lossy(&tab.text[from..]).contains(prompt)
            });
        }

        /// Ends the shell with `exit`, and returns how it ended.
        fn exit(mut self) -> Exit {
            self.type_line("exit 0");
            loop {
                match self.output.recv_timeout(PATIENCE).expect("the shell ends") {
                    Output::Bytes(bytes) => self.take(bytes),
                    Output::Exited(exit) => return exit.expect("the shell's exit status"),
                }
            }
        }
    }

    /// The keys of `reports`, but the cmdlets, which PowerShell lists when it is idle.
    fn keys(reports: &[Report]) -> Vec<&str> {
        reports
            .iter()
            .map(|(key, _)| key.as_str())
            .filter(|&key| key != "cmdlets")
            .collect()
    }

    /// The value of the last `key` among `reports`.
    fn value(reports: &[Report], key: &str) -> String {
        reports
            .iter()
            .rev()
            .find(|(found, _)| found == key)
            .and_then(|(_, value)| value.clone())
            .unwrap_or_else(|| panic!("no {key} report in {reports:?}"))
    }

    /// Every file under `folder`, with what it holds and when it last changed.
    fn snapshot(folder: &Path) -> Vec<(PathBuf, Vec<u8>, std::time::SystemTime)> {
        let mut files = Vec::new();
        for entry in fs::read_dir(folder).unwrap() {
            let path = entry.unwrap().path();
            let metadata = fs::symlink_metadata(&path).unwrap();
            if metadata.is_dir() {
                files.extend(snapshot(&path));
            } else {
                files.push((
                    path.clone(),
                    fs::read(&path).unwrap(),
                    metadata.modified().unwrap(),
                ));
            }
        }
        files.sort();
        files
    }

    /// A home folder, and a folder in it whose name needs encoding in a report.
    fn home() -> (Folder, PathBuf, PathBuf) {
        let folder = Folder::new();
        // Canonical, so the path the shell reports is the one the test expects, symbolic
        // links or not.
        let home = fs::canonicalize(&folder.0).unwrap();
        let odd = home.join("a b;c%dé");
        fs::create_dir(&odd).unwrap();
        (folder, home, odd)
    }

    /// Writes the user's files in `home`: each `(name, text)`, in folders as the name says.
    fn user_files(home: &Path, files: &[(&str, &str)]) {
        for (name, text) in files {
            let path = home.join(name);
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(path, text).unwrap();
        }
    }

    /// The user's zsh files. Each says it was read, and none saves history, so the shell
    /// leaves the home folder as it was.
    const ZSH_FILES: [(&str, &str); 4] = [
        (".zshenv", "loaded+=(zshenv)\n"),
        (".zprofile", "loaded+=(zprofile)\n"),
        (
            ".zshrc",
            "loaded+=(zshrc)\n\
             unset HISTFILE\n\
             PROMPT='[user %?]%# '\n\
             alias fixture-alias='echo fixture'\n\
             fixture_function() { :; }\n\
             user_precmd() { hooked=$(( hooked + 1 )); }\n\
             precmd_functions+=(user_precmd)\n",
        ),
        (".zlogin", "loaded+=(zlogin)\n"),
    ];

    #[test]
    fn zsh_reports_and_leaves_the_users_files_alone() {
        let Some(zsh) = installed("zsh") else { return };
        let (_folder, home, odd) = home();
        let private = Folder::new();
        user_files(&home, &ZSH_FILES);
        let before = snapshot(&home);

        // The default shell, as a login shell.
        let mut tab = Integrated::start(
            Shell::Default,
            Syntax::Posix,
            &home,
            &private.0,
            |command| {
                command.env("SHELL", &zsh);
                command.env("ODD_FOLDER", &odd);
            },
        );

        // FISH-05: everything at the first prompt, then that it is ready.
        let first = tab.next_ready();
        assert_eq!(
            keys(&first),
            ["cwd", "path", "aliases", "functions", "ready"]
        );
        assert_eq!(value(&first, "cwd"), home.to_str().unwrap());
        assert!(names(&value(&first, "aliases")).contains(&"fixture-alias"));
        let functions = value(&first, "functions");
        assert!(
            names(&functions).contains(&"fixture_function"),
            "{functions}"
        );
        assert!(!functions.contains("_twominal"), "{functions}");

        // Then only what changed.
        tab.type_line(r#"cd "$ODD_FOLDER" && alias twotest='echo hi'"#);
        let second = tab.next_ready();
        assert_eq!(keys(&second), ["cwd", "aliases", "ready"]);
        assert_eq!(value(&second, "cwd"), odd.to_str().unwrap());
        assert!(names(&value(&second, "aliases")).contains(&"twotest"));
        assert_eq!(value(&first, "path"), tab.ask("$PATH"));

        // FISH-04: the user's files are read as usual, in a login shell, with ZDOTDIR as the
        // user left it. Their own prompt and hook still work, with $? as it was.
        assert_eq!(tab.ask("${(j:,:)loaded}"), "zshenv,zprofile,zshrc,zlogin");
        assert_eq!(tab.ask("$([[ -o login ]] && echo login)"), "login");
        assert_eq!(tab.ask("${ZDOTDIR-unset}"), "unset");
        assert_ne!(tab.ask("${hooked:-0}"), "0");
        assert_eq!(tab.ask("$(env | grep -c '^twominal_')"), "0");
        tab.fails("false", "[user 1]");

        assert!(tab.exit().success());
        assert_eq!(snapshot(&home), before, "the user's files are left alone");
    }

    /// A report as the Terminal Core reads it, in the shape this file's own reader gives.
    fn as_written(event: terminal_core::Event) -> Report {
        use terminal_core::Report::*;
        let text = |bytes: Vec<u8>| Some(String::from_utf8(bytes).unwrap());
        let (key, value) = match event {
            terminal_core::Event::Report(report) => match report {
                Directory(path) => ("cwd", text(path)),
                SearchPath(path) => ("path", text(path)),
                Aliases(names) => ("aliases", Some(names.join(" "))),
                Functions(names) => ("functions", Some(names.join(" "))),
                Cmdlets(names) => ("cmdlets", Some(names.join(" "))),
                Ready => ("ready", None),
            },
            reply => panic!("not a report: {reply:?}"),
        };
        (key.to_string(), value)
    }

    #[test]
    fn the_terminal_core_reads_the_reports_as_written() {
        let Some(zsh) = installed("zsh") else { return };
        let (_folder, home, _) = home();
        let private = Folder::new();
        // Enough functions for a report of tens of kilobytes (DESIGN.md §7).
        let functions: String = (0..3000)
            .map(|number| format!("fixture_function_{number:04}() {{ :; }}\n"))
            .collect();
        user_files(
            &home,
            &[(".zshrc", &format!("unset HISTFILE\n{functions}"))],
        );

        let mut tab = Integrated::start(
            Shell::Command(vec![zsh.to_str().unwrap().to_string()]),
            Syntax::Posix,
            &home,
            &private.0,
            |_| {},
        );
        let first = tab.next_ready();
        assert!(value(&first, "functions").len() > 60_000);

        let mut core = terminal_core::TerminalCore::new(SIZE, Some(&tab.nonce));
        let read: Vec<Report> = core
            .interpret(&tab.text)
            .into_iter()
            .filter(|event| matches!(event, terminal_core::Event::Report(_)))
            .map(as_written)
            .collect();
        assert_eq!(read, tab.reports());
        assert!(tab.exit().success());
    }

    #[test]
    fn zsh_keeps_the_users_own_zdotdir() {
        let Some(zsh) = installed("zsh") else { return };
        let (_folder, home, _) = home();
        let private = Folder::new();
        let zdotdir = home.join(".config").join("zsh");
        user_files(
            &home,
            &[
                (".config/zsh/.zshenv", "loaded+=(zshenv)\n"),
                (".config/zsh/.zshrc", "loaded+=(zshrc)\nunset HISTFILE\n"),
            ],
        );
        let before = snapshot(&home);

        let mut tab = Integrated::start(
            Shell::Command(vec![zsh.to_str().unwrap().to_string()]),
            Syntax::Posix,
            &home,
            &private.0,
            |command| command.env("ZDOTDIR", &zdotdir),
        );
        tab.next_ready();
        assert_eq!(tab.ask("${ZDOTDIR-unset}"), zdotdir.to_str().unwrap());
        assert_eq!(tab.ask("${(j:,:)loaded}"), "zshenv,zshrc");
        assert!(tab.exit().success());
        assert_eq!(snapshot(&home), before);
    }

    /// The user's bash files, which say they were read, and save no history.
    const BASH_FILES: [(&str, &str); 3] = [
        (
            ".bash_profile",
            "loaded=\"${loaded}bash_profile,\"\n\
             unset HISTFILE\n\
             PS1='[user $?]\\$ '\n\
             alias fixture-alias='echo fixture'\n\
             fixture_function() { :; }\n\
             PROMPT_COMMAND='hooked=$(( hooked + 1 ))'\n",
        ),
        (".bashrc", "loaded=\"${loaded}bashrc,\"\nunset HISTFILE\n"),
        (".profile", "loaded=\"${loaded}profile,\"\n"),
    ];

    #[test]
    fn bash_reports_and_leaves_the_users_files_alone() {
        let Some(bash) = installed("bash") else {
            return;
        };
        let (_folder, home, odd) = home();
        let private = Folder::new();
        user_files(&home, &BASH_FILES);
        let before = snapshot(&home);

        let mut tab = Integrated::start(
            Shell::Default,
            Syntax::Posix,
            &home,
            &private.0,
            |command| {
                command.env("SHELL", &bash);
                command.env("ODD_FOLDER", &odd);
            },
        );

        let first = tab.next_ready();
        assert_eq!(
            keys(&first),
            ["cwd", "path", "aliases", "functions", "ready"]
        );
        assert_eq!(value(&first, "cwd"), home.to_str().unwrap());
        assert!(names(&value(&first, "aliases")).contains(&"fixture-alias"));
        let functions = value(&first, "functions");
        assert!(
            names(&functions).contains(&"fixture_function"),
            "{functions}"
        );
        assert!(!functions.contains("_twominal"), "{functions}");

        tab.type_line(r#"cd "$ODD_FOLDER" && alias twotest='echo hi'"#);
        let second = tab.next_ready();
        assert_eq!(keys(&second), ["cwd", "aliases", "ready"]);
        assert_eq!(value(&second, "cwd"), odd.to_str().unwrap());
        assert!(names(&value(&second, "aliases")).contains(&"twotest"));
        assert_eq!(value(&first, "path"), tab.ask("$PATH"));

        // A login shell's files, read in bash's own order: .bash_profile, and so not
        // .bash_login, .profile or .bashrc.
        assert_eq!(tab.ask("$loaded"), "bash_profile,");
        assert_ne!(tab.ask("${hooked:-0}"), "0");
        assert_eq!(tab.ask("$(env | grep -c '^twominal_')"), "0");
        tab.fails("false", "[user 1]");

        assert!(tab.exit().success());
        assert_eq!(snapshot(&home), before, "the user's files are left alone");
    }

    #[test]
    fn bash_that_is_not_a_login_shell_reads_bashrc() {
        let Some(bash) = installed("bash") else {
            return;
        };
        let (_folder, home, _) = home();
        let private = Folder::new();
        user_files(&home, &BASH_FILES);

        let shell = Shell::Command(vec![bash.to_str().unwrap().to_string()]);
        let mut tab = Integrated::start(shell, Syntax::Posix, &home, &private.0, |_| {});
        tab.next_ready();
        assert_eq!(tab.ask("$loaded"), "bashrc,");
        assert!(tab.exit().success());
    }

    #[test]
    fn powershell_reports_and_leaves_the_users_files_alone() {
        let Some(pwsh) = installed("pwsh") else {
            return;
        };
        let (_folder, home, odd) = home();
        let private = Folder::new();
        user_files(
            &home,
            &[(
                ".config/powershell/Microsoft.PowerShell_profile.ps1",
                "$global:loaded += @('profile')\n\
                 Set-Alias fixture-alias Get-Date\n\
                 function global:fixture_function {}\n\
                 function global:prompt { \"[user $(if ($?) { 0 } else { 1 })]> \" }\n",
            )],
        );
        let before = snapshot(&home);

        let shell = Shell::Command(vec![pwsh.to_str().unwrap().to_string()]);
        let mut tab = Integrated::start(shell, Syntax::PowerShell, &home, &private.0, |command| {
            command.env("ODD_FOLDER", &odd);
        });

        let first = tab.next_ready();
        assert_eq!(
            keys(&first),
            ["cwd", "path", "aliases", "functions", "ready"]
        );
        assert_eq!(value(&first, "cwd"), home.to_str().unwrap());
        assert!(names(&value(&first, "aliases")).contains(&"fixture-alias"));
        let functions = value(&first, "functions");
        assert!(
            names(&functions).contains(&"fixture_function"),
            "{functions}"
        );
        assert!(!functions.contains("__twominal"), "{functions}");

        // The cmdlets come once PowerShell is idle, so they never delay the first prompt.
        assert!(!first.iter().any(|(key, _)| key == "cmdlets"));
        tab.read_until("cmdlets", |tab| {
            tab.reports().iter().any(|(key, _)| key == "cmdlets")
        });
        let cmdlets = value(&tab.reports(), "cmdlets");
        assert!(names(&cmdlets).contains(&"Get-ChildItem"), "{cmdlets}");

        tab.type_line("Set-Location -LiteralPath $env:ODD_FOLDER; Set-Alias twotest Get-Date");
        let second = tab.next_ready();
        assert_eq!(keys(&second), ["cwd", "aliases", "ready"]);
        assert_eq!(value(&second, "cwd"), odd.to_str().unwrap());
        assert!(names(&value(&second, "aliases")).contains(&"twotest"));
        assert_eq!(value(&first, "path"), tab.ask("$env:PATH"));

        assert_eq!(tab.ask("$($global:loaded -join ',')"), "profile");
        assert_eq!(
            tab.ask("$(@(Get-ChildItem Env: | Where-Object Name -like 'twominal*').Count)"),
            "0"
        );
        tab.fails("Get-Item -LiteralPath /nowhere-at-all", "[user 1]> ");

        assert!(tab.exit().success());
        assert_eq!(snapshot(&home), before, "the user's files are left alone");
    }

    #[test]
    fn with_integration_off_nothing_is_reported() {
        let Some(zsh) = installed("zsh") else { return };
        let (_folder, home, _) = home();
        let private = Folder::new();
        user_files(&home, &ZSH_FILES);

        let mut start = start_with(Some(&[zsh.to_str().unwrap()]), Some(home.clone()));
        start.private = Some(private.0.clone());
        let mut command = command(&start);
        command.env("HOME", &home);
        command.env_remove("ZDOTDIR");
        assert_eq!(integrate(&start, &mut command), None);
        let (sink, output) = sink(16);
        let session =
            spawn(&start.shell, command, start.size, None, sink).expect("the shell starts");
        assert_eq!(session.reports(), None);

        // The user's files are still read.
        session.write(b"echo \"<$((40 + 2))>${(j:,:)loaded}\"; exit\n".to_vec());
        let (text, exit) = finish(&output);
        assert!(text.contains("<42>zshenv,zshrc"), "{text:?}");
        assert!(!text.contains("\x1b]2222;"), "{text:?}");
        assert!(exit.success());
    }

    #[test]
    fn sh_has_no_integration() {
        let private = Folder::new();
        let mut start = start_with(Some(&["/bin/sh"]), None);
        start.integration = true;
        start.private = Some(private.0.clone());
        let mut command = command(&start);
        assert_eq!(integrate(&start, &mut command), None);

        let (sink, output) = sink(16);
        let session =
            spawn(&start.shell, command, start.size, None, sink).expect("the shell starts");
        assert_eq!(session.reports(), None);
        session.write(b"exit\n".to_vec());
        let (text, _) = finish(&output);
        assert!(!text.contains("\x1b]2222;"), "{text:?}");
    }
}
