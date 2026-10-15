//! Settings Store (SRS §6.1).
//!
//! Loads, validates, saves and announces settings.
//!
//! Settings live here and nowhere else (ARC-05): the file, its format, every default in
//! Appendix B and every validation rule. It names no other component. The App Shell passes
//! each change on to whoever it concerns, and tabs receive theirs as messages (ARC-04).
//!
//! The store is created with a function to call on each change, which is how a change
//! travels up without the store knowing who hears it. A change is announced the same way
//! whether it came from [`SettingsStore::update`] or from a hand edit of the file, which a
//! thread of the store's own watches for (SET-04).
//!
//! The file, `config.toml`, is read and written in `settings/file.rs`. DESIGN.md §7 says
//! what is in it.

mod file;
#[cfg(test)]
mod tests;

use std::collections::BTreeMap;
use std::fmt;
use std::mem;
use std::ops::RangeInclusive;
use std::path::PathBuf;
use std::sync::{Arc, Mutex, MutexGuard, PoisonError, Weak};
use std::thread;
use std::time::Duration;

use file::ConfigFile;

/// How often the watcher looks at the file.
const WATCH_EVERY: Duration = Duration::from_millis(250);

/// The sizes the Font size setting takes, in points (Appendix B).
pub const FONT_SIZES: RangeInclusive<f64> = 6.0..=72.0;

/// What a notice says is done about a file that cannot be read: at startup, and later.
const USING_DEFAULTS: &str = "Using the defaults";
const KEEPING_SETTINGS: &str = "Keeping the current settings";

/// Every setting in Appendix B, as loaded from the configuration file. A change takes effect
/// immediately unless the field says otherwise (SET-02).
#[derive(Clone, Debug, PartialEq)]
pub struct Settings {
    pub theme: Theme,
    /// The font's family name. Whether it is installed is for the App Shell to find out.
    pub font: String,
    /// In points.
    pub font_size: f64,
    pub font_ligatures: bool,
    pub vim_mode: bool,
    /// On still means reduced when the OS asks, which the Animator decides.
    pub animations: bool,
    /// Takes effect at the next launch.
    pub splash_screen: bool,
    /// Programs may override it.
    pub cursor_style: CursorStyle,
    pub cursor_blink: bool,
    /// In lines. Shrinking it drops the oldest lines.
    pub scrollback: u32,
    /// The executable and its arguments, or `None` for the user's default shell (SW-01).
    /// Takes effect in new tabs.
    pub shell: Option<Vec<String>>,
    /// Takes effect in new tabs.
    pub shell_integration: bool,
    /// Takes effect at the next command line.
    pub command_line_editor: bool,
    pub command_history: bool,
    pub new_tab_directory: NewTabDirectory,
    pub confirm_close: bool,
    pub bell: Bell,
    pub clipboard_write: Permission,
    pub clipboard_read: Permission,
    pub paste_warning: bool,
    /// Each word and what it expands into.
    pub abbreviations: BTreeMap<String, String>,
}

impl Default for Settings {
    /// The defaults of Appendix B.
    fn default() -> Self {
        Self {
            theme: Theme::System,
            font: "JetBrains Mono".to_string(),
            font_size: 13.0,
            font_ligatures: true,
            vim_mode: true,
            animations: true,
            splash_screen: true,
            cursor_style: CursorStyle::Block,
            cursor_blink: true,
            scrollback: 10_000,
            shell: None,
            shell_integration: true,
            command_line_editor: true,
            command_history: true,
            new_tab_directory: NewTabDirectory::Current,
            confirm_close: true,
            bell: Bell::Visual,
            clipboard_write: Permission::Allow,
            clipboard_read: Permission::Deny,
            paste_warning: true,
            abbreviations: BTreeMap::new(),
        }
    }
}

/// Every setting once: its key in the file, its rule and where it is kept. Loading, saving
/// and comparing all read this list. It is in the order of Appendix B, with the
/// abbreviations last because they are a table in the file.
fn each(visit: &mut impl Visit) {
    visit.word("theme", |s| &mut s.theme);
    visit.text("font", |s| &mut s.font);
    visit.number("font_size", FONT_SIZES, |s| &mut s.font_size);
    visit.flag("font_ligatures", |s| &mut s.font_ligatures);
    visit.flag("vim_mode", |s| &mut s.vim_mode);
    visit.flag("animations", |s| &mut s.animations);
    visit.flag("splash_screen", |s| &mut s.splash_screen);
    visit.word("cursor_style", |s| &mut s.cursor_style);
    visit.flag("cursor_blink", |s| &mut s.cursor_blink);
    visit.count("scrollback", 0..=100_000, |s| &mut s.scrollback);
    visit.command("shell", |s| &mut s.shell);
    visit.flag("shell_integration", |s| &mut s.shell_integration);
    visit.flag("command_line_editor", |s| &mut s.command_line_editor);
    visit.flag("command_history", |s| &mut s.command_history);
    visit.word("new_tab_directory", |s| &mut s.new_tab_directory);
    visit.flag("confirm_close", |s| &mut s.confirm_close);
    visit.word("bell", |s| &mut s.bell);
    visit.word("clipboard_write", |s| &mut s.clipboard_write);
    visit.word("clipboard_read", |s| &mut s.clipboard_read);
    visit.flag("paste_warning", |s| &mut s.paste_warning);
    visit.pairs("abbreviations", |s| &mut s.abbreviations);
}

/// Where a setting is kept in [`Settings`].
type Field<T> = fn(&mut Settings) -> &mut T;

/// A reader of the list in [`each`]. It is shown every setting in turn, by kind.
trait Visit {
    /// On or off.
    fn flag(&mut self, key: &'static str, field: Field<bool>);
    /// One of a few words.
    fn word<W: Word>(&mut self, key: &'static str, field: Field<W>);
    /// A name: any text that is not blank.
    fn text(&mut self, key: &'static str, field: Field<String>);
    /// A number in a range.
    fn number(&mut self, key: &'static str, range: RangeInclusive<f64>, field: Field<f64>);
    /// A whole number in a range.
    fn count(&mut self, key: &'static str, range: RangeInclusive<u32>, field: Field<u32>);
    /// An executable and its arguments, or nothing.
    fn command(&mut self, key: &'static str, field: Field<Option<Vec<String>>>);
    /// Words, each with its expansion.
    fn pairs(&mut self, key: &'static str, field: Field<BTreeMap<String, String>>);
}

/// A setting that is one of a few words.
trait Word: Copy + PartialEq + 'static {
    /// Each value, with the word the file writes for it.
    const WORDS: &'static [(&'static str, Self)];
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Theme {
    Dark,
    Light,
    /// Follows the OS appearance.
    System,
}

impl Word for Theme {
    const WORDS: &'static [(&'static str, Self)] = &[
        ("dark", Self::Dark),
        ("light", Self::Light),
        ("system", Self::System),
    ];
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CursorStyle {
    Block,
    Bar,
    Underline,
}

impl Word for CursorStyle {
    const WORDS: &'static [(&'static str, Self)] = &[
        ("block", Self::Block),
        ("bar", Self::Bar),
        ("underline", Self::Underline),
    ];
}

/// Where a new tab starts.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NewTabDirectory {
    /// In the active tab's working directory.
    Current,
    Home,
}

impl Word for NewTabDirectory {
    const WORDS: &'static [(&'static str, Self)] =
        &[("current", Self::Current), ("home", Self::Home)];
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Bell {
    Visual,
    Audible,
    Off,
}

impl Word for Bell {
    const WORDS: &'static [(&'static str, Self)] = &[
        ("visual", Self::Visual),
        ("audible", Self::Audible),
        ("off", Self::Off),
    ];
}

/// Whether programs may use the clipboard (SEC-04).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Permission {
    Allow,
    Ask,
    Deny,
}

impl Word for Permission {
    const WORDS: &'static [(&'static str, Self)] = &[
        ("allow", Self::Allow),
        ("ask", Self::Ask),
        ("deny", Self::Deny),
    ];
}

/// Something wrong with the configuration, for the user to read (SET-05).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Notice {
    /// The setting's key as the file writes it, or the file's name when the file as a whole
    /// is the problem.
    pub setting: String,
    /// What is wrong with it.
    pub problem: String,
    /// What is done about it, such as `Using 13`. Empty when there is nothing to add.
    pub instead: String,
}

impl Notice {
    /// That the Font setting names no installed font, which the App Shell finds out. The
    /// bundled font, the default, is drawn instead (SET-05).
    pub fn font_missing(font: &str) -> Self {
        Self {
            setting: "font".to_string(),
            problem: format!("{} is not an installed font", file::shown(font)),
            instead: format!("Using {}", file::shown(&Settings::default().font)),
        }
    }

    /// A notice about the file as a whole.
    fn file(problem: String, instead: &str) -> Self {
        Self {
            setting: file::NAME.to_string(),
            problem,
            instead: instead.to_string(),
        }
    }
}

impl fmt::Display for Notice {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}: {}.", self.setting, self.problem)?;
        if !self.instead.is_empty() {
            write!(f, " {}.", self.instead)?;
        }
        Ok(())
    }
}

/// What the store announces: the settings now, the settings before, and what is wrong with
/// the configuration now.
#[derive(Clone, Debug)]
pub struct Change {
    pub settings: Settings,
    pub previous: Settings,
    pub notices: Vec<Notice>,
}

/// Every setting in Appendix B, kept in step with the configuration file.
pub struct SettingsStore {
    shared: Arc<Shared>,
}

/// What the store and its watcher both hold.
struct Shared {
    state: Mutex<State>,
    announce: Box<dyn Fn(&Change) + Send + Sync>,
}

struct State {
    settings: Settings,
    notices: Vec<Notice>,
    /// `None` when the OS names no configuration folder. Nothing is read and nothing saved.
    file: Option<ConfigFile>,
}

impl SettingsStore {
    /// Loads the settings from `twominal/config.toml` in `config_dir`, the OS's configuration
    /// folder, and starts watching the file.
    ///
    /// Never fails: whatever is wrong with the file becomes a notice (SET-05). From then on
    /// `announce` is called with each change, on the thread that made it. It may call the
    /// store.
    pub fn open(
        config_dir: Option<PathBuf>,
        announce: impl Fn(&Change) + Send + Sync + 'static,
    ) -> Self {
        let store = Self::read(config_dir, announce);
        store.watch(WATCH_EVERY);
        store
    }

    /// [`Self::open`] without the watcher.
    fn read(
        config_dir: Option<PathBuf>,
        announce: impl Fn(&Change) + Send + Sync + 'static,
    ) -> Self {
        let mut state = State {
            settings: Settings::default(),
            notices: Vec::new(),
            file: config_dir.as_deref().map(ConfigFile::new),
        };
        if state.file.is_some() {
            // Nobody is listening yet: the first settings are asked for, not announced.
            state.load(USING_DEFAULTS);
        } else {
            let problem = "the OS names no configuration folder".to_string();
            let instead = "Using the defaults, and changes are not saved";
            state.notices.push(Notice::file(problem, instead));
        }
        Self {
            shared: Arc::new(Shared {
                state: Mutex::new(state),
                announce: Box::new(announce),
            }),
        }
    }

    /// Watches the file from a thread of its own, until the store is dropped. Editors save
    /// by writing in place, by renaming a new file over the old one, or in several steps.
    /// Asking for the file by its path sees all three alike.
    fn watch(&self, every: Duration) {
        let shared = Arc::downgrade(&self.shared);
        let watcher = move || {
            // A save may be several writes. The file is read once it has looked the same twice.
            let mut seen = None;
            loop {
                thread::sleep(every);
                let Some(shared) = Weak::upgrade(&shared) else {
                    return;
                };
                let changed = shared.state().file.as_ref().and_then(ConfigFile::changed);
                if changed.is_some() && changed == seen {
                    shared.reload();
                }
                seen = changed;
            }
        };
        // Without the thread, a hand edit applies at the next launch. That is no reason to
        // stop Twominal from starting.
        let _ = thread::Builder::new()
            .name("settings".to_string())
            .spawn(watcher);
    }

    /// The settings in use.
    pub fn settings(&self) -> Settings {
        self.shared.state().settings.clone()
    }

    /// What is wrong with the configuration now. Empty when nothing is.
    pub fn notices(&self) -> Vec<Notice> {
        self.shared.state().notices.clone()
    }

    /// Where the configuration file is, whether or not it exists.
    pub fn path(&self) -> Option<PathBuf> {
        let state = self.shared.state();
        state.file.as_ref().map(|file| file.path().to_path_buf())
    }

    /// Changes settings. `edit` is given the settings in use and changes what it likes. The
    /// result is saved to the file and announced. `edit` must not call the store.
    ///
    /// If a value breaks a rule the file is held to, nothing changes, and the notices say
    /// which and why.
    pub fn update(&self, edit: impl FnOnce(&mut Settings)) -> Result<(), Vec<Notice>> {
        let mut changes = Vec::new();
        let outcome = {
            let mut state = self.shared.state();
            // The edit is made to the file as it is now, not as it was last read.
            if state.file.as_ref().is_some_and(|f| f.changed().is_some()) {
                changes.extend(state.load(KEEPING_SETTINGS));
            }
            let mut next = state.settings.clone();
            edit(&mut next);
            let problems = file::check(&next);
            if problems.is_empty() {
                changes.extend(state.save(next));
                Ok(())
            } else {
                Err(problems)
            }
        };
        for change in &changes {
            (self.shared.announce)(change);
        }
        outcome
    }
}

impl Shared {
    fn state(&self) -> MutexGuard<'_, State> {
        // A panic under the lock cannot leave the settings half-changed: every change
        // replaces them in one step. So the lock is still good.
        self.state.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// Reads the file again and announces what that changed. The lock is let go before the
    /// announcement, so that whoever hears it may call the store.
    fn reload(&self) {
        let change = self.state().load(KEEPING_SETTINGS);
        if let Some(change) = change {
            (self.announce)(&change);
        }
    }
}

impl State {
    /// Reads the file and takes what it says. If the file as a whole cannot be read, the
    /// settings stay as they are and a notice says so, ending with `kept`. Returns the
    /// change, if anything changed.
    fn load(&mut self, kept: &str) -> Option<Change> {
        let file = self.file.as_mut()?;
        let (settings, notices) = match file.read() {
            Ok(read) => read,
            Err(problem) => (self.settings.clone(), vec![Notice::file(problem, kept)]),
        };
        self.replace(settings, notices)
    }

    /// Takes `next` as the settings and writes them to the file. A change that cannot be
    /// saved still applies, with a notice that it was not saved.
    fn save(&mut self, next: Settings) -> Option<Change> {
        if next == self.settings {
            return None;
        }
        let mut notices = self.notices.clone();
        if let Some(file) = &mut self.file {
            match file.write(&next) {
                Ok(found) => notices = found,
                Err(problem) => {
                    let notice = Notice::file(problem, "");
                    if !notices.contains(&notice) {
                        notices.push(notice);
                    }
                }
            }
        }
        self.replace(next, notices)
    }

    fn replace(&mut self, settings: Settings, notices: Vec<Notice>) -> Option<Change> {
        if settings == self.settings && notices == self.notices {
            return None;
        }
        let previous = mem::replace(&mut self.settings, settings);
        self.notices = notices;
        Some(Change {
            settings: self.settings.clone(),
            previous,
            notices: self.notices.clone(),
        })
    }
}
