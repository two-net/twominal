//! Tests of the App Shell: what needs no window (GEN-04, GEN-05, TAB-15, UI-05). The windows
//! and menus themselves, and a window the launcher asks for, are checked by hand (DESIGN.md
//! §8). So are the OS clipboard and opening links, with ignored tests, since they are the user's
//! (SW-05, SW-07).

use std::ffi::OsString;
use std::path::Path;

use tab::{OsClipboard, OsLinks, Selection};

use super::clipboard::Clipboard;
use super::fonts::{self, Zoom};
use super::launcher::Problem;
use super::links;
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

    assert_eq!(
        menu::action("font-bigger"),
        Some(Action::FontSize(Zoom::Bigger))
    );
    assert_eq!(
        menu::action("font-smaller"),
        Some(Action::FontSize(Zoom::Smaller))
    );
    assert_eq!(
        menu::action("font-reset"),
        Some(Action::FontSize(Zoom::Reset))
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
fn the_font_size_items_have_the_shortcuts_of_each_os() {
    // Appendix A. No plain Ctrl+letter is taken from the shell.
    let shortcuts = |macos| {
        [Zoom::Bigger, Zoom::Smaller, Zoom::Reset].map(|zoom| menu::font_size_shortcut(zoom, macos))
    };
    assert_eq!(shortcuts(true), ["Cmd+=", "Cmd+-", "Cmd+0"]);
    assert_eq!(
        shortcuts(false),
        ["Ctrl+Shift+=", "Ctrl+Shift+-", "Ctrl+Shift+0"]
    );
}

#[test]
fn the_font_size_items_step_a_whole_point_as_far_as_the_setting_goes() {
    assert_eq!(fonts::zoomed(13.0, Zoom::Bigger), 14.0);
    assert_eq!(fonts::zoomed(13.0, Zoom::Smaller), 12.0);
    // A size the file gave in part of a point goes to the next whole one.
    assert_eq!(fonts::zoomed(13.5, Zoom::Bigger), 14.0);
    assert_eq!(fonts::zoomed(13.5, Zoom::Smaller), 13.0);
    // At either end it stays.
    assert_eq!(fonts::zoomed(72.0, Zoom::Bigger), 72.0);
    assert_eq!(fonts::zoomed(71.5, Zoom::Bigger), 72.0);
    assert_eq!(fonts::zoomed(6.0, Zoom::Smaller), 6.0);
    assert_eq!(fonts::zoomed(6.5, Zoom::Smaller), 6.0);
    // Reset is the default of Appendix B, whatever the size.
    for size in [6.0, 13.0, 20.5, 72.0] {
        assert_eq!(fonts::zoomed(size, Zoom::Reset), 13.0);
    }
}

/// Settings with the Font setting `font` at `size` points.
fn font_settings(font: &str, size: f64) -> Settings {
    Settings {
        font: font.to_string(),
        font_size: size,
        ..Settings::default()
    }
}

#[test]
fn the_font_falls_back_to_the_bundled_one_and_then_the_os() {
    assert_eq!(
        fonts::font(&font_settings("Fira Code", 13.0), true).0,
        r#""Fira Code", "JetBrains Mono", monospace"#
    );
    // The bundled font is named once, whatever its letter case.
    for bundled in ["JetBrains Mono", "jetbrains mono", " JETBRAINS MONO "] {
        assert_eq!(
            fonts::font(&font_settings(bundled, 13.0), true).0,
            r#""JetBrains Mono", monospace"#
        );
    }
    // The name as written, but for the spaces around it.
    assert_eq!(
        fonts::font(&font_settings("  Iosevka Term  ", 13.0), true).0,
        r#""Iosevka Term", "JetBrains Mono", monospace"#
    );
}

#[test]
fn any_font_name_is_a_css_string() {
    let family = |name: &str| fonts::font(&font_settings(name, 13.0), true).0;
    assert_eq!(
        family(r#"Say "hi""#),
        r#""Say \"hi\"", "JetBrains Mono", monospace"#
    );
    assert_eq!(
        family(r"back\slash"),
        r#""back\\slash", "JetBrains Mono", monospace"#
    );
    // A control character is written as its code, ended by a space, so the `font` that
    // carries it can still be read.
    assert_eq!(family("a\nb"), r#""a\a b", "JetBrains Mono", monospace"#);
    assert_eq!(
        family("a\u{7f}\u{9b}b"),
        r#""a\7f \9b b", "JetBrains Mono", monospace"#
    );
    // Nothing else is escaped.
    assert_eq!(
        family("Sarasa Mono 更纱 ✓"),
        r#""Sarasa Mono 更纱 ✓", "JetBrains Mono", monospace"#
    );
}

#[test]
fn points_are_css_pixels_as_each_os_counts_them() {
    // macOS counts 72 to the inch, and Linux and Windows 96.
    for points in [6.0, 13.0, 72.0] {
        assert_eq!(fonts::font(&font_settings("Menlo", points), true).1, points);
    }
    assert_eq!(fonts::font(&font_settings("Menlo", 6.0), false).1, 8.0);
    assert_eq!(fonts::font(&font_settings("Menlo", 12.0), false).1, 16.0);
    assert_eq!(fonts::font(&font_settings("Menlo", 72.0), false).1, 96.0);
}

#[test]
fn the_notice_that_a_font_is_missing_joins_the_stores() {
    let store = [Notice {
        setting: "theme".to_string(),
        problem: "\"blue\" is not one of \"dark\", \"light\", \"system\"".to_string(),
        instead: "Using \"system\"".to_string(),
    }];
    assert_eq!(texts(&store, None).len(), 1);
    assert_eq!(
        texts(&store, Some(&Notice::font_missing("Nope"))),
        [
            store[0].to_string(),
            "font: \"Nope\" is not an installed font. Using \"JetBrains Mono\".".to_string(),
        ]
    );
}

#[test]
fn the_bundled_font_is_always_installed() {
    // So it is never looked for.
    assert!(fonts::is_bundled("JetBrains Mono"));
    assert!(fonts::is_bundled(" jetbrains MONO "));
    assert!(fonts::installed("jetbrains mono"));
    assert!(!fonts::is_bundled("JetBrains Mono NL"));
}

#[cfg(target_os = "macos")]
#[test]
fn installed_fonts_are_found_by_their_family_names() {
    // Every Mac has Menlo.
    assert!(fonts::installed("Menlo"));
    assert!(fonts::installed("menlo"));
    assert!(!fonts::installed("No Font Is Named This 23"));

    // Read from the file as well as by fontdb.
    let menlo = std::fs::read("/System/Library/Fonts/Menlo.ttc").unwrap();
    assert!(fonts::has_family(&menlo, 0, "MENLO"));
    assert!(!fonts::has_family(&menlo, 0, "Menlo Regular"));
    assert!(!fonts::has_family(b"not a font", 0, "Menlo"));
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
/// SW-05, by hand on each OS: `cargo test -p twominal -- --ignored clipboard`. It writes the
/// user's clipboard, and the primary selection on Linux, and gives each back what it held.
#[test]
#[ignore = "it writes the user's clipboard"]
fn the_clipboard_gives_back_what_was_written() {
    let clipboard = Clipboard::new();
    let text = "Twominal's clipboard test: é, 界 and 👍🏽\nand a second line";
    for which in [Selection::Clipboard, Selection::Primary] {
        let held = clipboard.read(which);
        clipboard.write(which, text);
        let read = clipboard.read(which);
        if let Some(held) = &held {
            clipboard.write(which, held);
        }
        match which {
            Selection::Primary if !cfg!(target_os = "linux") => assert_eq!(read, None),
            _ => assert_eq!(read.as_deref(), Some(text), "{which:?}"),
        }
    }
}

/// A folder for one test of opening links, removed when the test ends.
#[cfg(unix)]
struct Files(std::path::PathBuf);

#[cfg(unix)]
impl Files {
    fn new() -> Self {
        use std::sync::atomic::{AtomicUsize, Ordering};
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let number = NEXT.fetch_add(1, Ordering::Relaxed);
        let name = format!("twominal-links-{}-{number}", std::process::id());
        let path = env::temp_dir().join(name);
        std::fs::create_dir_all(&path).unwrap();
        // As a link is opened: through every symbolic link, as macOS's temporary folder is one.
        Self(std::fs::canonicalize(path).unwrap())
    }

    /// A file named `name` in the folder, with the permissions `mode`.
    fn file(&self, name: &str, mode: u32) -> std::path::PathBuf {
        use std::os::unix::fs::PermissionsExt;
        let path = self.0.join(name);
        std::fs::write(&path, "echo hi\n").unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(mode)).unwrap();
        path
    }

    /// Whether the file manager is shown `name` rather than the OS opening it.
    fn runs(&self, name: &str) -> bool {
        let path = self.0.join(name);
        links::runs(&path, &std::fs::metadata(&path).unwrap())
    }
}

#[cfg(unix)]
impl Drop for Files {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

#[cfg(unix)]
#[test]
fn a_file_link_is_read_back_as_its_path() {
    use std::path::PathBuf;
    assert_eq!(
        links::path_of("/tmp/a.txt"),
        Some(PathBuf::from("/tmp/a.txt"))
    );
    assert_eq!(
        links::path_of("/tmp/a%20b/%E7%95%8C?x=1#y"),
        Some(PathBuf::from("/tmp/a b/界"))
    );
    // Bytes that are no UTF-8 name a file too, as they may on Linux.
    assert_eq!(
        links::path_of("/tmp/%FF").map(|path| path.into_os_string().into_encoded_bytes()),
        Some(b"/tmp/\xff".to_vec())
    );
    assert_eq!(
        links::path_of("/tmp/100%"),
        Some(PathBuf::from("/tmp/100%"))
    );
    // `%` with no two hex digits after it is itself.
    assert_eq!(
        links::path_of("/tmp/%+1%2"),
        Some(PathBuf::from("/tmp/%+1%2"))
    );
    assert_eq!(links::path_of("tmp/a"), None);
}

#[cfg(unix)]
#[test]
fn a_file_that_would_run_is_told_from_one_that_opens() {
    let files = Files::new();
    files.file("notes.txt", 0o644);
    files.file("script.sh", 0o755);
    files.file("mine", 0o744);
    assert!(!files.runs("notes.txt"));
    assert!(files.runs("script.sh"));
    assert!(files.runs("mine"));
    std::fs::create_dir(files.0.join("folder")).unwrap();
    assert!(!files.runs("folder"));
    // By their extension, in any letter case, whatever their permissions.
    let extensions: &[&str] = if cfg!(target_os = "macos") {
        &[
            "command", "TERMINAL", "webloc", "fileloc", "inetloc", "MAILLOC", "url", "jar", "tool",
        ]
    } else {
        // Wine runs Windows' programs where it is installed.
        &["desktop", "Desktop", "jar", "AppImage", "run", "exe", "lnk"]
    };
    for extension in extensions {
        let name = format!("a.{extension}");
        files.file(&name, 0o644);
        assert!(files.runs(&name), "{name}");
    }
    if cfg!(target_os = "macos") {
        // A bundle, by its name or by what it holds.
        std::fs::create_dir(files.0.join("Some.app")).unwrap();
        assert!(files.runs("Some.app"));
        std::fs::create_dir_all(files.0.join("Plugin/Contents")).unwrap();
        files.file("Plugin/Contents/Info.plist", 0o644);
        assert!(files.runs("Plugin"));
        // A Finder alias, whatever it is named: its data starts as a bookmark's.
        std::fs::write(
            files.0.join("an alias.txt"),
            b"book\0\0\0\0mark\0\0\0\08\0\0\0",
        )
        .unwrap();
        assert!(files.runs("an alias.txt"));
        std::fs::write(files.0.join("book.txt"), b"bookkeeping").unwrap();
        assert!(!files.runs("book.txt"));
    }
    // A pipe is no document, and opening it would wait for whoever writes it.
    let made = std::process::Command::new("mkfifo")
        .arg(files.0.join("pipe"))
        .status()
        .unwrap();
    assert!(made.success());
    assert!(files.runs("pipe"));
}

#[cfg(unix)]
#[test]
fn each_link_opens_with_the_oss_own_command() {
    use std::ffi::{OsStr, OsString};
    let files = Files::new();
    let notes = files.file("my notes.txt", 0o644);
    let script = files.file("script.sh", 0o755);
    let words = |url: &str| -> Option<Vec<OsString>> {
        links::command(url).map(|command| {
            let mut words = vec![command.get_program().to_owned()];
            words.extend(command.get_args().map(OsStr::to_owned));
            words
        })
    };
    let expected = |words: &[&OsStr]| Some(words.iter().map(|word| word.to_os_string()).collect());
    let macos = cfg!(target_os = "macos");
    let open = OsStr::new(if macos { "/usr/bin/open" } else { "xdg-open" });

    // A URL is handed over as it is, and on macOS taken as a URL even if it names a file too.
    let url = "https://example.com/a?b=c&d";
    let as_url: &[&OsStr] = if macos {
        &[open, OsStr::new("-u"), OsStr::new(url)]
    } else {
        &[open, OsStr::new(url)]
    };
    assert_eq!(words(url), expected(as_url));
    // A file that opens is opened by the path that was looked at, read back from the URL.
    let link = format!("file://{}/my%20notes.txt?x#y", files.0.display());
    assert_eq!(words(&link), expected(&[open, notes.as_os_str()]));
    // A script is shown, not run: selected in Finder on macOS, its folder opened on Linux.
    let link = format!("file://{}", script.display());
    let shown: &[&OsStr] = if macos {
        &[open, OsStr::new("-R"), script.as_os_str()]
    } else {
        &[open, files.0.as_os_str()]
    };
    assert_eq!(words(&link), expected(shown));
    // A symbolic link is judged, and opened, by the file it leads to, as the OS would open that:
    // one named as a document that leads to a program is shown, not run.
    let payload = files.file(
        if macos {
            "payload.terminal"
        } else {
            "payload.desktop"
        },
        0o644,
    );
    std::os::unix::fs::symlink(&payload, files.0.join("readme.txt")).unwrap();
    let link = format!("file://{}/readme.txt", files.0.display());
    let shown: &[&OsStr] = if macos {
        &[open, OsStr::new("-R"), payload.as_os_str()]
    } else {
        &[open, files.0.as_os_str()]
    };
    assert_eq!(words(&link), expected(shown));
    std::os::unix::fs::symlink(&notes, files.0.join("doc")).unwrap();
    let link = format!("file://{}/doc", files.0.display());
    assert_eq!(words(&link), expected(&[open, notes.as_os_str()]));
    // A file that is not there is not opened.
    assert_eq!(words(&format!("file://{}/gone", files.0.display())), None);
}

/// SW-07, by hand on each OS: `cargo test -p twominal -- --ignored links`. It opens a web page in
/// the user's browser, and shows a script in the file manager, which must not run it (SEC-07).
#[cfg(unix)]
#[test]
#[ignore = "it opens what the user sees"]
fn links_open_in_the_oss_own_handlers() {
    let files = Files::new();
    let script = files.file("twominal-should-not-run.command", 0o755);
    // Run, it would leave a file behind.
    let ran = files.0.join("ran");
    std::fs::write(&script, format!("#!/bin/sh\ntouch '{}'\n", ran.display())).unwrap();
    let os = links::Links::new();
    os.open("https://example.com/");
    os.open(&format!("file://{}", script.display()));
    // Each opens on a thread of its own, and the file manager reads the folder after that. The
    // folder goes when the test ends.
    std::thread::sleep(std::time::Duration::from_secs(5));
    assert!(!ran.exists(), "the script ran");
}

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
