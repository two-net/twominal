//! Tests of the App Shell: what needs no window (GEN-04, GEN-05, TAB-15, UI-05). The windows
//! and menus themselves, and a window the launcher asks for, are checked by hand (DESIGN.md
//! §8).

use std::ffi::OsString;
use std::path::Path;

use super::launcher::Problem;
use super::menu::{self, Action};
use super::*;

/// A command's arguments, without its name.
fn arguments(words: &[&str]) -> Vec<OsString> {
    words.iter().map(OsString::from).collect()
}

#[test]
fn a_window_is_titled_after_its_active_tab() {
    assert_eq!(window_title(Some("~/code"), "Twominal"), "~/code");
}

#[test]
fn a_window_whose_tab_has_no_title_shows_the_product_name() {
    assert_eq!(window_title(None, "Twominal"), "Twominal");
    assert_eq!(window_title(Some(""), "Twominal"), "Twominal");
    assert_eq!(window_title(Some("  "), "Twominal"), "Twominal");

    // No tab has a title until #34.
    let window = Window {
        label: "window-1".to_string(),
        tabs: TabManager::new(None),
    };
    assert_eq!(window.title("Twominal"), "Twominal");
}

#[test]
fn labels_count_up_and_are_never_reused() {
    let mut windows = Windows::default();
    assert_eq!(windows.next_label(), "window-1");
    assert_eq!(windows.next_label(), "window-2");

    windows.open.clear();
    assert_eq!(windows.next_label(), "window-3");
}

#[test]
fn the_capability_names_every_window() {
    let capability = include_str!("../../capabilities/default.json");
    assert!(
        capability.contains(&format!("\"{LABEL}*\"")),
        "capabilities/default.json must name the windows as `{LABEL}*`"
    );
}

#[test]
fn no_window_comes_from_the_configuration() {
    let config: String = include_str!("../../tauri.conf.json")
        .split_whitespace()
        .collect();
    assert!(
        config.contains("\"windows\":[]"),
        "tauri.conf.json must declare no window: the App Shell opens them in code"
    );
}

#[test]
fn each_menu_item_has_its_action() {
    assert_eq!(menu::action("new-window"), Some(Action::NewWindow));
    assert_eq!(menu::action("close-window"), Some(Action::CloseWindow));
    assert_eq!(menu::action("quit"), Some(Action::Quit));
    assert_eq!(menu::action("theme-dark"), Some(Action::Theme(Theme::Dark)));
    assert_eq!(
        menu::action("theme-light"),
        Some(Action::Theme(Theme::Light))
    );
    assert_eq!(
        menu::action("theme-system"),
        Some(Action::Theme(Theme::System))
    );

    // The OS carries out its own items.
    assert_eq!(menu::action("copy"), None);
}

#[test]
fn new_window_has_the_shortcut_of_each_os() {
    assert_eq!(menu::new_window_shortcut(true), "Cmd+N");
    assert_eq!(menu::new_window_shortcut(false), "Ctrl+Shift+N");
}

#[test]
fn a_theme_setting_becomes_an_os_appearance() {
    assert_eq!(appearance(Theme::Dark), Some(tauri::Theme::Dark));
    assert_eq!(appearance(Theme::Light), Some(tauri::Theme::Light));
    assert_eq!(appearance(Theme::System), None);
}

#[test]
fn the_command_opens_a_window_in_the_directory_it_names() {
    assert_eq!(launcher::parse(arguments(&[])), Ok(Command::Open(None)));
    assert_eq!(
        launcher::parse(arguments(&["code"])),
        Ok(Command::Open(Some(PathBuf::from("code"))))
    );

    // After `--` nothing is an option, and a `-` alone never is one.
    assert_eq!(
        launcher::parse(arguments(&["--", "-x"])),
        Ok(Command::Open(Some(PathBuf::from("-x"))))
    );
    assert_eq!(
        launcher::parse(arguments(&["-"])),
        Ok(Command::Open(Some(PathBuf::from("-"))))
    );
}

#[test]
fn the_command_has_help_and_a_version() {
    for help in ["-h", "--help"] {
        assert_eq!(launcher::parse(arguments(&[help])), Ok(Command::Help));
    }
    for version in ["-V", "--version"] {
        assert_eq!(launcher::parse(arguments(&[version])), Ok(Command::Version));
    }
    // Wherever it stands before `--`.
    assert_eq!(
        launcher::parse(arguments(&["code", "--help"])),
        Ok(Command::Help)
    );
    assert_eq!(
        launcher::parse(arguments(&["--", "--help"])),
        Ok(Command::Open(Some(PathBuf::from("--help"))))
    );

    assert_eq!(launcher::version("Twominal", "1.2.3"), "Twominal 1.2.3");
    let help = launcher::help("Twominal", "1.2.3");
    assert!(help.starts_with("Twominal 1.2.3\n"), "{help}");
    for line in [
        "Usage: twominal [DIRECTORY]",
        "  -h, --help     Show this help",
        "  -V, --version  Show the version",
    ] {
        assert!(help.lines().any(|found| found == line), "{help}");
    }
}

#[test]
fn a_command_written_wrongly_is_refused() {
    let unknown = launcher::parse(arguments(&["--nope"])).unwrap_err();
    assert_eq!(unknown, Problem::UnknownOption(OsString::from("--nope")));
    assert_eq!(unknown.status(), 2);
    assert_eq!(
        unknown.message(),
        "twominal: unknown option '--nope'\nTry 'twominal --help'."
    );

    let second = launcher::parse(arguments(&["one", "two"])).unwrap_err();
    assert_eq!(second, Problem::SecondDirectory(OsString::from("two")));
    assert_eq!(second.status(), 2);
}

#[test]
fn a_directory_is_made_absolute_and_must_be_one() {
    // The tests run in the package's folder.
    let here = env::current_dir().unwrap();
    assert_eq!(launcher::locate(Path::new("src")), Ok(here.join("src")));
    assert_eq!(launcher::locate(Path::new(".")), Ok(here.clone()));
    assert_eq!(launcher::locate(&here), Ok(here));

    let missing = launcher::locate(Path::new("no-such-folder")).unwrap_err();
    assert_eq!(
        missing,
        Problem::NoSuchDirectory(PathBuf::from("no-such-folder"))
    );
    assert_eq!(missing.status(), 1);
    assert_eq!(
        missing.message(),
        "twominal: no such directory: no-such-folder"
    );

    let file = launcher::locate(Path::new("Cargo.toml")).unwrap_err();
    assert_eq!(file, Problem::NotADirectory(PathBuf::from("Cargo.toml")));
    assert_eq!(file.status(), 1);

    assert!(launcher::locate(Path::new("")).is_err());
    assert_eq!(
        launcher::read(arguments(&["src"])),
        Ok(Command::Open(Some(env::current_dir().unwrap().join("src"))))
    );
    assert_eq!(
        launcher::read(arguments(&["--version"])),
        Ok(Command::Version)
    );
}

/// The hand-over between two commands (GEN-05). Each test has a folder of its own.
#[cfg(unix)]
mod hand_over {
    use std::ffi::OsStr;
    use std::fs;
    use std::os::unix::ffi::OsStrExt;
    use std::os::unix::fs::PermissionsExt;
    use std::path::{Path, PathBuf};
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::mpsc::{self, Receiver};
    use std::time::Duration;

    use super::super::launcher::Start;
    use super::super::socket::{self, Claim, Endpoint};

    /// Longer than any answer takes.
    const SOON: Duration = Duration::from_secs(5);

    /// A folder for one test, removed when the test ends. Its name is short: the path of a
    /// socket has little room.
    struct Folder(PathBuf);

    impl Folder {
        /// A folder only the user can open, or with `mode` one that others can.
        fn new(mode: u32) -> Self {
            static NEXT: AtomicUsize = AtomicUsize::new(0);
            let number = NEXT.fetch_add(1, Ordering::Relaxed);
            let name = format!("twominal-l-{}-{number}", std::process::id());
            let path = std::env::temp_dir().join(name);
            fs::create_dir_all(&path).unwrap();
            fs::set_permissions(&path, fs::Permissions::from_mode(mode)).unwrap();
            Self(path)
        }

        fn endpoint(&self) -> Endpoint {
            Endpoint::in_folder(&self.0, true).expect("the folder is the user's own")
        }
    }

    impl Drop for Folder {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    /// Makes this the first process at `endpoint`, as a command does when no Twominal runs.
    fn first(endpoint: &Endpoint) -> Claim {
        match endpoint.start(None) {
            Start::First(claim) => claim,
            _ => panic!("no Twominal listens yet, so this one is the first"),
        }
    }

    /// Starts answering, and gives every directory a window is asked for.
    fn serve(claim: Claim) -> Receiver<Option<PathBuf>> {
        let (sender, asked) = mpsc::channel();
        claim
            .serve(move |directory| {
                let _ = sender.send(directory);
            })
            .unwrap();
        asked
    }

    #[test]
    fn the_first_command_listens_and_the_next_hand_over() {
        let folder = Folder::new(0o700);
        let endpoint = folder.endpoint();
        let asked = serve(first(&endpoint));

        let code = Path::new("/home/two/code");
        assert!(matches!(endpoint.start(Some(code)), Start::HandedOver));
        assert_eq!(asked.recv_timeout(SOON), Ok(Some(code.to_path_buf())));

        assert!(matches!(endpoint.start(None), Start::HandedOver));
        assert_eq!(asked.recv_timeout(SOON), Ok(None));
    }

    #[test]
    fn only_one_process_is_the_first() {
        let folder = Folder::new(0o700);
        let endpoint = folder.endpoint();
        let claim = first(&endpoint);
        assert!(matches!(Claim::take(&endpoint), Ok(None)));

        // Still so while it answers: the lock moved to the thread with the listener.
        let asked = serve(claim);
        assert!(matches!(Claim::take(&endpoint), Ok(None)));
        assert!(matches!(endpoint.start(None), Start::HandedOver));
        assert_eq!(asked.recv_timeout(SOON), Ok(None));
    }

    #[test]
    fn what_a_twominal_that_is_gone_left_is_replaced() {
        let folder = Folder::new(0o700);
        let endpoint = folder.endpoint();
        // As when the process ends: the lock is free, and the socket's file stays.
        drop(first(&endpoint));
        assert_eq!(fs::read_dir(&folder.0).unwrap().count(), 2);

        let asked = serve(first(&endpoint));
        assert!(matches!(endpoint.start(None), Start::HandedOver));
        assert_eq!(asked.recv_timeout(SOON), Ok(None));
    }

    #[test]
    fn a_command_that_gets_no_answer_opens_the_only_window() {
        let folder = Folder::new(0o700);
        let endpoint = folder.endpoint();
        // A Twominal that listens and does not answer yet.
        let claim = first(&endpoint);
        assert!(matches!(
            endpoint.start(Some(Path::new("/late"))),
            Start::Alone
        ));

        // When it does answer, the command has gone, and no window opens for it.
        let asked = serve(claim);
        assert!(matches!(endpoint.start(None), Start::HandedOver));
        assert_eq!(asked.recv_timeout(SOON), Ok(None));
        assert!(asked.try_recv().is_err());
    }

    #[test]
    fn a_debug_build_has_an_endpoint_of_its_own() {
        let folder = Folder::new(0o700);
        let installed = Endpoint::in_folder(&folder.0, false).unwrap();
        let _installed = first(&installed);
        let _debug = first(&folder.endpoint());
        assert_eq!(fs::read_dir(&folder.0).unwrap().count(), 4);
    }

    #[test]
    fn a_folder_others_can_open_is_not_used() {
        for mode in [0o755, 0o770, 0o701] {
            let folder = Folder::new(mode);
            assert!(Endpoint::in_folder(&folder.0, true).is_none(), "{mode:o}");
        }
        let folder = Folder::new(0o700);
        assert!(Endpoint::in_folder(&folder.0.join("missing"), true).is_none());
    }

    #[test]
    fn a_request_carries_the_directory_whatever_its_bytes() {
        assert_eq!(socket::decode(&socket::encode(None)), Some(None));
        for bytes in [&b"/home/two/code"[..], b"/caf\xc3\xa9", b"/not\xffutf8"] {
            let directory = Path::new(OsStr::from_bytes(bytes));
            assert_eq!(
                socket::decode(&socket::encode(Some(directory))),
                Some(Some(directory.to_path_buf()))
            );
        }
    }

    #[test]
    fn a_request_that_is_not_understood_is_not_answered() {
        // Nothing at all, another version, a path relative to nobody knows what, and more
        // than any path holds.
        assert_eq!(socket::decode(b""), None);
        assert_eq!(socket::decode(b"\x02/home"), None);
        assert_eq!(socket::decode(b"\x01code"), None);
        let long = PathBuf::from(format!("/{}", "a".repeat(8 * 1024)));
        assert_eq!(socket::decode(&socket::encode(Some(&long))), None);
    }
}
