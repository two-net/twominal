//! Tests of the Session (SW-01, SW-02, FISH-01, TERM-15, SEC-03). Those that start a shell
//! run `/bin/sh` on a real pseudo-terminal, so they are for macOS and Linux.

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
        Start {
            shell: shell.map(|words| words.iter().map(|word| word.to_string()).collect()),
            directory,
            size: SIZE,
            name: "Twominal".to_string(),
            version: "9.9.9".to_string(),
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
        let session = spawn(command(&start), start.size, sink).expect("the shell starts");
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
        let session = spawn(command, SIZE, sink).expect("the shell starts");
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
        let _session = spawn(command(&start), start.size, sink).expect("the shell starts");
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
}
