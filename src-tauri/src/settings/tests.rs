//! Tests of the Settings Store (SET-02 to SET-05, AT-13). Each has a configuration folder
//! of its own.

use std::fs;
use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::mpsc::{self, Receiver};
use std::time::Duration;

use super::*;

/// A configuration folder for one test, removed when the test ends.
struct Folder(PathBuf);

impl Folder {
    fn new() -> Self {
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let number = NEXT.fetch_add(1, Ordering::Relaxed);
        let name = format!("twominal-settings-{}-{number}", std::process::id());
        let path = std::env::temp_dir().join(name);
        fs::create_dir_all(&path).unwrap();
        Self(path)
    }

    /// A folder whose `config.toml` holds `text`.
    fn with(text: &str) -> Self {
        let folder = Self::new();
        folder.write(text);
        folder
    }

    fn file(&self) -> PathBuf {
        self.0.join("twominal").join("config.toml")
    }

    fn write(&self, text: &str) {
        fs::create_dir_all(self.file().parent().unwrap()).unwrap();
        fs::write(self.file(), text).unwrap();
    }

    fn text(&self) -> String {
        fs::read_to_string(self.file()).unwrap()
    }

    /// A store on this folder that reads the file only when a test tells it to, and every
    /// change it announces.
    fn open(&self) -> (SettingsStore, Receiver<Change>) {
        let (sender, changes) = mpsc::channel();
        let store = SettingsStore::read(Some(self.0.clone()), move |change| {
            let _ = sender.send(change.clone());
        });
        (store, changes)
    }
}

impl Drop for Folder {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn texts(notices: &[Notice]) -> Vec<String> {
    notices.iter().map(Notice::to_string).collect()
}

/// Settings that differ from the default in every field.
fn all_changed() -> Settings {
    Settings {
        theme: Theme::Dark,
        font: "Fira Code".to_string(),
        font_size: 14.5,
        font_ligatures: false,
        vim_mode: false,
        animations: false,
        splash_screen: false,
        cursor_style: CursorStyle::Bar,
        cursor_blink: false,
        scrollback: 0,
        shell: Some(vec!["/bin/bash".to_string(), "--login".to_string()]),
        shell_integration: false,
        command_line_editor: false,
        command_history: false,
        new_tab_directory: NewTabDirectory::Home,
        confirm_close: false,
        bell: Bell::Off,
        clipboard_write: Permission::Ask,
        clipboard_read: Permission::Allow,
        paste_warning: false,
        abbreviations: BTreeMap::from([
            ("gco".to_string(), "git checkout".to_string()),
            ("l".to_string(), "ls -la".to_string()),
        ]),
    }
}

const ALL_CHANGED: &str = r#"
theme = "Dark"
font = "Fira Code"
font_size = 14.5
font_ligatures = false
vim_mode = false
animations = false
splash_screen = false
cursor_style = "bar"
cursor_blink = false
scrollback = 0
shell = ["/bin/bash", "--login"]
shell_integration = false
command_line_editor = false
command_history = false
new_tab_directory = "home"
confirm_close = false
bell = "off"
clipboard_write = "ask"
clipboard_read = "allow"
paste_warning = false

[abbreviations]
gco = "git checkout"
l = "ls -la"
"#;

// ---------------------------------------------------------------------------------------
// Loading (SET-03, SET-05)
// ---------------------------------------------------------------------------------------

#[test]
fn the_defaults_are_those_of_appendix_b() {
    let settings = Settings::default();
    assert_eq!(settings.theme, Theme::System);
    assert_eq!(settings.font, "JetBrains Mono");
    assert_eq!(settings.font_size, 13.0);
    assert!(settings.font_ligatures);
    assert!(settings.vim_mode);
    assert!(settings.animations);
    assert!(settings.splash_screen);
    assert_eq!(settings.cursor_style, CursorStyle::Block);
    assert!(settings.cursor_blink);
    assert_eq!(settings.scrollback, 10_000);
    assert_eq!(settings.shell, None);
    assert!(settings.shell_integration);
    assert!(settings.command_line_editor);
    assert!(settings.command_history);
    assert_eq!(settings.new_tab_directory, NewTabDirectory::Current);
    assert!(settings.confirm_close);
    assert_eq!(settings.bell, Bell::Visual);
    assert_eq!(settings.clipboard_write, Permission::Allow);
    assert_eq!(settings.clipboard_read, Permission::Deny);
    assert!(settings.paste_warning);
    assert!(settings.abbreviations.is_empty());
}

#[test]
fn no_file_means_the_defaults_and_no_notice() {
    let folder = Folder::new();
    let (store, _changes) = folder.open();

    assert_eq!(store.settings(), Settings::default());
    assert_eq!(texts(&store.notices()), [] as [&str; 0]);
    assert_eq!(store.path(), Some(folder.file()));
    assert!(
        !folder.file().parent().unwrap().exists(),
        "nothing is created"
    );
}

#[test]
fn an_empty_file_means_the_defaults() {
    let folder = Folder::with("# nothing yet\n");
    let (store, _changes) = folder.open();

    assert_eq!(store.settings(), Settings::default());
    assert!(store.notices().is_empty());
}

#[test]
fn a_file_sets_every_setting_it_names() {
    let folder = Folder::with(ALL_CHANGED);
    let (store, _changes) = folder.open();

    assert_eq!(store.settings(), all_changed());
    assert_eq!(texts(&store.notices()), [] as [&str; 0]);
}

/// AT-13: a notice names the setting, its default is used, and the rest of the file applies.
#[test]
fn a_bad_value_falls_back_alone() {
    let folder = Folder::with("theme = \"light\"\nfont_size = 200\nvim_mode = false\n");
    let (store, _changes) = folder.open();

    let settings = store.settings();
    assert_eq!(settings.font_size, 13.0);
    assert_eq!(settings.theme, Theme::Light);
    assert!(!settings.vim_mode);
    let notices = store.notices();
    assert_eq!(notices[0].setting, "font_size");
    assert_eq!(
        texts(&notices),
        ["font_size: 200 is not between 6 and 72. Using 13."]
    );
}

#[test]
fn each_kind_of_bad_value_is_named() {
    let cases = [
        (
            "theme = \"blue\"",
            "theme: \"blue\" is not one of \"dark\", \"light\", \"system\". Using \"system\".",
        ),
        (
            "theme = 3",
            "theme: expected one of \"dark\", \"light\", \"system\", found 3. Using \"system\".",
        ),
        (
            "cursor_blink = \"yes\"",
            "cursor_blink: expected true or false, found \"yes\". Using true.",
        ),
        (
            "font = \"\"",
            "font: expected a name, found \"\". Using \"JetBrains Mono\".",
        ),
        (
            "[font]\nsize = 14",
            "font: expected a name, found a table. Using \"JetBrains Mono\".",
        ),
        (
            "font_size = \"big\"",
            "font_size: expected a number, found \"big\". Using 13.",
        ),
        (
            "font_size = 5.9",
            "font_size: 5.9 is not between 6 and 72. Using 13.",
        ),
        (
            "font_size = nan",
            "font_size: nan is not between 6 and 72. Using 13.",
        ),
        (
            "scrollback = 100_001",
            "scrollback: 100_001 is not between 0 and 100000. Using 10000.",
        ),
        (
            "scrollback = -1",
            "scrollback: -1 is not between 0 and 100000. Using 10000.",
        ),
        (
            "scrollback = 1.5",
            "scrollback: expected a whole number, found 1.5. Using 10000.",
        ),
        (
            "shell = \"zsh\"",
            "shell: expected a list of strings such as [\"/bin/zsh\", \"-l\"], found \"zsh\". Using your default shell.",
        ),
        (
            "shell = []",
            "shell: the list has no executable. Using your default shell.",
        ),
        (
            "shell = [\"zsh\", 1]",
            "shell: the list holds something that is not a string. Using your default shell.",
        ),
        (
            "abbreviations = 3",
            "abbreviations: expected a table of word = \"expansion\" lines, found 3. Using none.",
        ),
        ("font_sise = 14", "font_sise: not a setting."),
    ];

    for (file, notice) in cases {
        let folder = Folder::with(file);
        let (store, _changes) = folder.open();
        assert_eq!(texts(&store.notices()), [notice], "{file}");
        assert_eq!(store.settings(), Settings::default(), "{file}");
    }
}

#[test]
fn a_bad_abbreviation_is_left_out_alone() {
    let folder = Folder::with(
        "[abbreviations]\ngco = \"git checkout\"\ngst = 3\n\"g co\" = \"git checkout\"\n",
    );
    let (store, _changes) = folder.open();

    let abbreviations = store.settings().abbreviations;
    assert_eq!(abbreviations.len(), 1);
    assert_eq!(abbreviations["gco"], "git checkout");
    assert_eq!(
        texts(&store.notices()),
        [
            "abbreviations.gst: expected the expansion as a string, found 3. Ignored.",
            "abbreviations.g co: an abbreviation is one word with no spaces. Ignored.",
        ]
    );
}

#[test]
fn a_long_value_is_cut_short_in_its_notice() {
    let folder = Folder::with(&format!("theme = \"{}\"", "x".repeat(500)));
    let (store, _changes) = folder.open();

    let notice = store.notices()[0].to_string();
    assert!(notice.starts_with("theme: \"xxx"), "{notice}");
    assert!(notice.len() < 200, "{notice}");
}

#[test]
fn a_font_that_is_not_installed_has_a_notice_like_the_others() {
    // The App Shell finds out, and the store words it (SET-05).
    let notice = Notice::font_missing("Nope");
    assert_eq!(notice.setting, "font");
    assert_eq!(
        notice.to_string(),
        "font: \"Nope\" is not an installed font. Using \"JetBrains Mono\"."
    );

    let notice = Notice::font_missing(&"x".repeat(500)).to_string();
    assert!(notice.starts_with("font: \"xxx"), "{notice}");
    assert!(notice.len() < 200, "{notice}");
}

#[test]
fn a_file_that_is_not_toml_means_the_defaults_at_startup() {
    let folder = Folder::with("theme = \"dark\"\nfont_size = \n");
    let (store, _changes) = folder.open();

    assert_eq!(store.settings(), Settings::default());
    let notices = store.notices();
    assert_eq!(notices.len(), 1);
    assert_eq!(notices[0].setting, "config.toml");
    let notice = notices[0].to_string();
    assert!(
        notice.starts_with("config.toml: line 2, column 13: "),
        "{notice}"
    );
    assert!(notice.ends_with(". Using the defaults."), "{notice}");
    assert!(!notice.contains('\n'), "{notice}");
}

#[test]
fn a_file_that_cannot_be_read_means_the_defaults() {
    let folder = Folder::new();
    fs::create_dir_all(folder.file()).unwrap();
    let (store, _changes) = folder.open();
    assert_eq!(store.settings(), Settings::default());
    assert_eq!(
        texts(&store.notices()),
        ["config.toml: is not a file. Using the defaults."]
    );

    let folder = Folder::new();
    fs::create_dir_all(folder.file().parent().unwrap()).unwrap();
    fs::write(folder.file(), [b't', 0xff, 0xfe]).unwrap();
    let (store, _changes) = folder.open();
    assert_eq!(store.settings(), Settings::default());
    let notice = store.notices()[0].to_string();
    assert!(
        notice.starts_with("config.toml: could not be read ("),
        "{notice}"
    );
}

#[test]
fn no_configuration_folder_means_the_defaults() {
    let store = SettingsStore::read(None, |_| {});

    assert_eq!(store.settings(), Settings::default());
    assert_eq!(store.path(), None);
    assert_eq!(
        texts(&store.notices()),
        ["config.toml: the OS names no configuration folder. Using the defaults, and changes are not saved."]
    );

    store
        .update(|settings| settings.theme = Theme::Dark)
        .unwrap();
    assert_eq!(store.settings().theme, Theme::Dark);
}

// ---------------------------------------------------------------------------------------
// Hand edits (SET-02, SET-04)
// ---------------------------------------------------------------------------------------

#[test]
fn a_hand_edit_is_announced() {
    let folder = Folder::with("theme = \"dark\"\n");
    let (store, changes) = folder.open();
    assert!(changes.try_recv().is_err(), "loading announces nothing");

    folder.write("theme = \"light\"\nfont_size = 15\n");
    store.shared.reload();

    let change = changes.try_recv().expect("the edit is announced");
    assert_eq!(change.previous.theme, Theme::Dark);
    assert_eq!(change.settings.theme, Theme::Light);
    assert_eq!(change.settings.font_size, 15.0);
    assert!(change.notices.is_empty());
    assert_eq!(store.settings(), change.settings);
    assert!(changes.try_recv().is_err(), "once");
}

#[test]
fn an_edit_that_changes_no_setting_is_not_announced() {
    let folder = Folder::with("theme = \"dark\"\n");
    let (store, changes) = folder.open();

    folder.write("# darker\ntheme = \"DARK\"\nfont_size = 13.0\n");
    store.shared.reload();

    assert!(changes.try_recv().is_err());
}

/// AT-13, while Twominal runs: the notice comes with the bad value and goes with it.
#[test]
fn a_notice_comes_and_goes_with_the_bad_value() {
    let folder = Folder::with("font_size = 14\n");
    let (store, changes) = folder.open();

    folder.write("font_size = 200\n");
    store.shared.reload();
    let change = changes.try_recv().unwrap();
    assert_eq!(change.settings.font_size, 13.0);
    assert_eq!(
        texts(&change.notices),
        ["font_size: 200 is not between 6 and 72. Using 13."]
    );

    folder.write("font_size = 16\n");
    store.shared.reload();
    let change = changes.try_recv().unwrap();
    assert_eq!(change.settings.font_size, 16.0);
    assert!(change.notices.is_empty());
    assert!(store.notices().is_empty());
}

#[test]
fn a_file_broken_while_running_keeps_the_settings_in_use() {
    let folder = Folder::with("theme = \"dark\"\n");
    let (store, changes) = folder.open();

    folder.write("theme = \"dark\nfont_size = 15\n");
    store.shared.reload();
    let change = changes.try_recv().unwrap();
    assert_eq!(change.settings.theme, Theme::Dark);
    assert_eq!(change.settings, change.previous);
    let notice = change.notices[0].to_string();
    assert!(
        notice.starts_with("config.toml: line 1, column "),
        "{notice}"
    );
    assert!(
        notice.ends_with(". Keeping the current settings."),
        "{notice}"
    );

    folder.write("theme = \"dark\"\nfont_size = 15\n");
    store.shared.reload();
    let change = changes.try_recv().unwrap();
    assert_eq!(change.settings.font_size, 15.0);
    assert!(change.notices.is_empty());
}

#[test]
fn deleting_the_file_returns_to_the_defaults() {
    let folder = Folder::with(ALL_CHANGED);
    let (store, changes) = folder.open();

    fs::remove_file(folder.file()).unwrap();
    store.shared.reload();

    let change = changes.try_recv().unwrap();
    assert_eq!(change.previous, all_changed());
    assert_eq!(change.settings, Settings::default());
    assert!(store.notices().is_empty());
}

#[test]
fn the_watcher_applies_a_saved_edit() {
    let folder = Folder::new();
    let (store, changes) = folder.open();
    store.watch(Duration::from_millis(5));

    folder.write("theme = \"light\"\n");
    let change = changes
        .recv_timeout(Duration::from_secs(10))
        .expect("the edit is announced");
    assert_eq!(change.settings.theme, Theme::Light);

    // The store's own save is announced once, by the save. It is not read back as an edit.
    store.update(|settings| settings.vim_mode = false).unwrap();
    assert!(!changes.try_recv().unwrap().settings.vim_mode);
    assert!(changes.recv_timeout(Duration::from_millis(100)).is_err());

    fs::remove_file(folder.file()).unwrap();
    let change = changes
        .recv_timeout(Duration::from_secs(10))
        .expect("the removal is announced");
    assert_eq!(change.settings, Settings::default());
}

// ---------------------------------------------------------------------------------------
// Saving (SET-02, SET-03)
// ---------------------------------------------------------------------------------------

#[test]
fn update_saves_and_announces() {
    let folder = Folder::new();
    let (store, changes) = folder.open();

    store
        .update(|settings| settings.theme = Theme::Dark)
        .unwrap();

    assert_eq!(folder.text(), "theme = \"dark\"\n");
    let change = changes.try_recv().expect("the change is announced");
    assert_eq!(change.previous.theme, Theme::System);
    assert_eq!(change.settings.theme, Theme::Dark);
    assert_eq!(store.settings().theme, Theme::Dark);
    assert!(changes.try_recv().is_err(), "once");
}

#[test]
fn update_that_changes_nothing_writes_nothing() {
    let folder = Folder::new();
    let (store, changes) = folder.open();

    store
        .update(|settings| settings.theme = Theme::System)
        .unwrap();

    assert!(!folder.file().exists());
    assert!(changes.try_recv().is_err());
}

#[test]
fn every_setting_survives_a_save() {
    let folder = Folder::new();
    let (store, _changes) = folder.open();
    store.update(|settings| *settings = all_changed()).unwrap();

    let (again, _changes) = folder.open();
    assert_eq!(again.settings(), all_changed());
    assert_eq!(texts(&again.notices()), [] as [&str; 0]);

    // And back: a setting returned to its default is written too, not left as it was.
    again
        .update(|settings| *settings = Settings::default())
        .unwrap();
    let (last, _changes) = folder.open();
    assert_eq!(last.settings(), Settings::default());
    assert_eq!(texts(&last.notices()), [] as [&str; 0]);
}

#[test]
fn update_keeps_the_rest_of_the_file() {
    let folder = Folder::with(
        "# Twominal settings\n\
         \n\
         # Smaller on the laptop\n\
         font_size = 12 # points\n\
         theme = \"light\"\n\
         future_key = [1, 2]\n\
         \n\
         [abbreviations]\n\
         # git\n\
         gco = \"git checkout\"\n\
         gst = \"git status\"\n",
    );
    let (store, _changes) = folder.open();

    store
        .update(|settings| {
            settings.font_size = 14.0;
            settings.vim_mode = false;
            settings.abbreviations.remove("gst");
            settings
                .abbreviations
                .insert("l".to_string(), "ls -la".to_string());
        })
        .unwrap();

    assert_eq!(
        folder.text(),
        "# Twominal settings\n\
         \n\
         # Smaller on the laptop\n\
         font_size = 14 # points\n\
         theme = \"light\"\n\
         future_key = [1, 2]\n\
         vim_mode = false\n\
         \n\
         [abbreviations]\n\
         # git\n\
         gco = \"git checkout\"\n\
         l = \"ls -la\"\n"
    );
    assert_eq!(
        texts(&store.notices()),
        ["future_key: not a setting."],
        "the key the store does not know is still there"
    );
}

#[test]
fn update_keeps_comments_above_the_first_key() {
    let folder = Folder::with("# My settings\n# theme = \"light\"\n");
    let (store, _changes) = folder.open();

    store
        .update(|settings| settings.theme = Theme::Dark)
        .unwrap();

    assert_eq!(
        folder.text(),
        "# My settings\n# theme = \"light\"\ntheme = \"dark\"\n"
    );

    // The same when the first thing written is a table, and the file ends without a line break.
    let folder = Folder::with("# My settings");
    let (store, _changes) = folder.open();
    store
        .update(|settings| {
            let abbreviations = &mut settings.abbreviations;
            abbreviations.insert("gco".to_string(), "git checkout".to_string());
        })
        .unwrap();
    assert_eq!(
        folder.text(),
        "# My settings\n[abbreviations]\ngco = \"git checkout\"\n"
    );
}

#[test]
fn update_writes_only_what_changed() {
    let folder = Folder::with("font_size = 200\n");
    let (store, _changes) = folder.open();

    store
        .update(|settings| settings.theme = Theme::Dark)
        .unwrap();
    assert_eq!(folder.text(), "font_size = 200\ntheme = \"dark\"\n");
    assert_eq!(
        texts(&store.notices()),
        ["font_size: 200 is not between 6 and 72. Using 13."]
    );

    store.update(|settings| settings.font_size = 14.0).unwrap();
    assert_eq!(folder.text(), "font_size = 14\ntheme = \"dark\"\n");
    assert!(store.notices().is_empty(), "the bad value is gone");
}

#[test]
fn update_turns_down_a_value_that_breaks_a_rule() {
    let folder = Folder::with("theme = \"dark\"\n");
    let (store, changes) = folder.open();

    let turned_down = store.update(|settings| {
        settings.theme = Theme::Light;
        settings.font_size = 100.0;
        settings.shell = Some(Vec::new());
        settings
            .abbreviations
            .insert("g co".to_string(), "git checkout".to_string());
    });

    assert_eq!(
        texts(&turned_down.unwrap_err()),
        [
            "font_size: 100 is not between 6 and 72.",
            "shell: the list has no executable.",
            "abbreviations.g co: an abbreviation is one word with no spaces.",
        ]
    );
    assert_eq!(store.settings().theme, Theme::Dark, "nothing changed");
    assert_eq!(folder.text(), "theme = \"dark\"\n");
    assert!(changes.try_recv().is_err());
}

#[test]
fn update_does_not_write_over_a_file_it_could_not_read() {
    let broken = "theme = \"dark\nfont_size = 15\n";
    let folder = Folder::with(broken);
    let (store, changes) = folder.open();

    store.update(|settings| settings.vim_mode = false).unwrap();

    assert_eq!(folder.text(), broken);
    assert!(!store.settings().vim_mode, "the change still applies");
    let change = changes.try_recv().unwrap();
    assert_eq!(change.notices.len(), 2);
    assert_eq!(
        change.notices[1].to_string(),
        "config.toml: the change was not saved, because the file could not be read."
    );

    // The same notice is not stacked up by the next change.
    store
        .update(|settings| settings.animations = false)
        .unwrap();
    assert_eq!(store.notices().len(), 2);
}

#[cfg(unix)]
#[test]
fn a_change_that_could_not_be_saved_is_saved_with_the_next() {
    use std::os::unix::fs::PermissionsExt;

    let folder = Folder::with("theme = \"dark\"\n");
    let (store, _changes) = folder.open();
    let parent = folder.file().parent().unwrap().to_path_buf();
    let lock = |mode| fs::set_permissions(&parent, fs::Permissions::from_mode(mode)).unwrap();

    lock(0o555);
    if fs::File::create(parent.join("probe")).is_ok() {
        // Whoever runs this test may write anywhere, so no save can be made to fail.
        lock(0o755);
        return;
    }
    store.update(|settings| settings.vim_mode = false).unwrap();
    lock(0o755);

    assert_eq!(folder.text(), "theme = \"dark\"\n");
    assert!(!store.settings().vim_mode, "the change still applies");
    let notice = store.notices()[0].to_string();
    assert!(
        notice.starts_with("config.toml: the change was not saved ("),
        "{notice}"
    );

    store
        .update(|settings| settings.animations = false)
        .unwrap();
    assert_eq!(
        folder.text(),
        "theme = \"dark\"\nvim_mode = false\nanimations = false\n"
    );
    assert!(store.notices().is_empty());
}

#[test]
fn update_edits_the_file_as_it_is_now() {
    let folder = Folder::new();
    let (store, changes) = folder.open();

    // Saved by hand since the store last read the file, and not yet noticed by the watcher.
    folder.write("theme = \"light\"\n");
    store.update(|settings| settings.vim_mode = false).unwrap();

    assert_eq!(folder.text(), "theme = \"light\"\nvim_mode = false\n");
    let settings = store.settings();
    assert_eq!(settings.theme, Theme::Light);
    assert!(!settings.vim_mode);
    assert_eq!(changes.try_iter().count(), 2, "the edit, then the update");
}

#[test]
fn update_saves_the_shell_and_removes_it() {
    let folder = Folder::new();
    let (store, _changes) = folder.open();

    let shell = vec!["/bin/zsh".to_string(), "-l".to_string()];
    store
        .update(|settings| settings.shell = Some(shell))
        .unwrap();
    assert_eq!(folder.text(), "shell = [\"/bin/zsh\", \"-l\"]\n");

    store.update(|settings| settings.shell = None).unwrap();
    assert_eq!(folder.text(), "");
}

#[cfg(unix)]
#[test]
fn saving_follows_a_symbolic_link() {
    let folder = Folder::new();
    let target = folder.0.join("dotfiles.toml");
    fs::write(&target, "font_size = 12\n").unwrap();
    fs::create_dir_all(folder.file().parent().unwrap()).unwrap();
    std::os::unix::fs::symlink(&target, folder.file()).unwrap();
    let (store, _changes) = folder.open();
    assert_eq!(store.settings().font_size, 12.0);

    store
        .update(|settings| settings.theme = Theme::Dark)
        .unwrap();

    let link = fs::symlink_metadata(folder.file()).unwrap();
    assert!(link.file_type().is_symlink(), "still a link");
    assert_eq!(
        fs::read_to_string(&target).unwrap(),
        "font_size = 12\ntheme = \"dark\"\n"
    );
}
