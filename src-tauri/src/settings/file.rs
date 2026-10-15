//! The configuration file, `config.toml` (DESIGN.md §7). Part of the Settings Store.
//!
//! Reading turns the file into [`Settings`], with a [`Notice`] for each thing in it that
//! could not be used. Writing changes only the keys whose setting changed, so the user's
//! comments, their order of keys and the keys this version does not know all stay (SET-04).
//!
//! Which keys there are is not said here: both directions walk the list in [`each`].

use std::collections::BTreeMap;
use std::fs;
use std::io::{self, Write};
use std::ops::RangeInclusive;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

use toml_edit::{Array, DocumentMut, Item, Table, TableLike, TomlError, Value};

use super::{each, Field, Notice, Settings, Visit, Word};

/// The folder in the OS's configuration folder (GEN-02), and the file in it.
const FOLDER: &str = "twominal";
pub(super) const NAME: &str = "config.toml";

/// The longest value a notice quotes.
const QUOTED: usize = 40;

/// `config.toml`, and what was in it when it was last read.
pub(super) struct ConfigFile {
    path: PathBuf,
    /// What the file looked like when it was last read or written.
    stamp: Stamp,
    /// The file as last read: empty if there was none. `None` if it could not be read or is
    /// not TOML, and then it is never written over.
    document: Option<DocumentMut>,
}

/// What a file looks like from outside: when it was last modified, and its length. `None`
/// if there is no file.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) struct Stamp(Option<(SystemTime, u64)>);

impl Stamp {
    fn of(path: &Path) -> Self {
        let file = fs::metadata(path).ok();
        Self(file.map(|file| {
            (
                file.modified().unwrap_or(SystemTime::UNIX_EPOCH),
                file.len(),
            )
        }))
    }
}

impl ConfigFile {
    /// The file in `config_dir`, the OS's configuration folder. Nothing is read yet.
    pub fn new(config_dir: &Path) -> Self {
        Self {
            path: config_dir.join(FOLDER).join(NAME),
            stamp: Stamp(None),
            document: None,
        }
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    /// What the file looks like now, if that is not how it looked when last read or written.
    pub fn changed(&self) -> Option<Stamp> {
        let now = Stamp::of(&self.path);
        (now != self.stamp).then_some(now)
    }

    /// Reads the file: the settings it holds, and a notice for each thing wrong in it. No
    /// file means every default and no notice. `Err` says why the file as a whole could not
    /// be read.
    pub fn read(&mut self) -> Result<(Settings, Vec<Notice>), String> {
        // Taken before the text, so that a save landing in between counts as a change.
        self.stamp = Stamp::of(&self.path);
        self.document = None;

        let unreadable = |error: io::Error| format!("could not be read ({error})");
        let text = match fs::metadata(&self.path) {
            Err(error) if error.kind() == io::ErrorKind::NotFound => String::new(),
            Err(error) => return Err(unreadable(error)),
            Ok(file) if !file.is_file() => return Err("is not a file".to_string()),
            Ok(_) => fs::read_to_string(&self.path).map_err(unreadable)?,
        };
        let text = text.strip_prefix('\u{feff}').unwrap_or(&text);
        let document: DocumentMut = text.parse().map_err(|error| syntax(text, &error))?;

        let read = load(document.as_table());
        self.document = Some(document);
        Ok(read)
    }

    /// Makes the file say `settings`. Only a key whose setting differs from what the file
    /// says now is written, and the rest of the file is left as it was. Returns the notices
    /// the file earns afterwards. `Err` says why nothing was written.
    pub fn write(&mut self, settings: &Settings) -> Result<Vec<Notice>, String> {
        let Some(document) = &self.document else {
            return Err("the change was not saved, because the file could not be read".to_string());
        };
        // On a copy, so that the document stays what the file holds if the write fails.
        let mut document = document.clone();
        let only_comments = document.is_empty();
        // Compared with the file, not with the settings in use: a change that could not be
        // saved earlier is then saved now.
        let (in_file, _) = load(document.as_table());
        each(&mut Save {
            table: document.as_table_mut(),
            previous: in_file,
            next: settings.clone(),
        });
        if only_comments {
            keep_comments_first(&mut document);
        }
        replace(&self.path, &document.to_string())
            .map_err(|error| format!("the change was not saved ({error})"))?;

        self.stamp = Stamp::of(&self.path);
        let (_, notices) = load(document.as_table());
        self.document = Some(document);
        Ok(notices)
    }
}

/// What is wrong with settings that did not come from the file, by the rules the file is
/// held to: they are written out and read back.
pub(super) fn check(settings: &Settings) -> Vec<Notice> {
    let mut document = DocumentMut::new();
    each(&mut Save {
        table: document.as_table_mut(),
        previous: Settings::default(),
        next: settings.clone(),
    });
    let (_, mut notices) = load(document.as_table());
    // Nothing is used instead: the caller turns the whole change down.
    for notice in &mut notices {
        notice.instead.clear();
    }
    notices
}

/// The settings a document holds, with a notice for each value that could not be used and
/// each key that is not a setting.
fn load(table: &Table) -> (Settings, Vec<Notice>) {
    let mut load = Load {
        table,
        settings: Settings::default(),
        notices: Vec::new(),
        keys: Vec::new(),
    };
    each(&mut load);
    for (key, _) in table.iter() {
        if !load.keys.contains(&key) {
            load.notice(key, "not a setting".to_string(), "");
        }
    }
    (load.settings, load.notices)
}

/// Replaces the file with `text` in one step, so that nothing ever reads half a file.
fn replace(path: &Path, text: &str) -> io::Result<()> {
    // A config.toml that is a symbolic link stays one: what it points to is replaced.
    let path = fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
    if let Some(folder) = path.parent() {
        fs::create_dir_all(folder)?;
    }
    // Named after this process, so that two Twominals saving at once do not share it.
    let temporary = path.with_extension(format!("toml.{}.tmp", std::process::id()));
    let written = (|| {
        let mut file = fs::File::create(&temporary)?;
        file.write_all(text.as_bytes())?;
        if let Ok(old) = fs::metadata(&path) {
            // Who may read the file is the user's choice. Failing to copy it is no reason
            // to lose the change.
            let _ = file.set_permissions(old.permissions());
        }
        file.sync_all()?;
        fs::rename(&temporary, &path)
    })();
    if written.is_err() {
        let _ = fs::remove_file(&temporary);
    }
    written
}

// ---------------------------------------------------------------------------------------
// Reading
// ---------------------------------------------------------------------------------------

/// Reads each setting from a document. A setting the document does not name, or names with
/// a value that breaks its rule, keeps its default.
struct Load<'a> {
    table: &'a Table,
    settings: Settings,
    notices: Vec<Notice>,
    /// Every key the list names, to tell the others by.
    keys: Vec<&'static str>,
}

impl Load<'_> {
    /// Takes the value of `key` if `parse` accepts it. Otherwise the setting keeps its
    /// default, and a notice gives `parse`'s reason and the default as `show` writes it.
    fn take<T>(
        &mut self,
        key: &'static str,
        field: Field<T>,
        show: impl FnOnce(&T) -> String,
        parse: impl FnOnce(&Item) -> Result<T, String>,
    ) {
        self.keys.push(key);
        let Some(item) = self.table.get(key) else {
            return;
        };
        match parse(item) {
            Ok(value) => *field(&mut self.settings) = value,
            Err(problem) => {
                let default = show(field(&mut self.settings));
                self.notice(key, problem, &format!("Using {default}"));
            }
        }
    }

    fn notice(&mut self, setting: &str, problem: String, instead: &str) {
        self.notices.push(Notice {
            setting: setting.to_string(),
            problem,
            instead: instead.to_string(),
        });
    }
}

impl Visit for Load<'_> {
    fn flag(&mut self, key: &'static str, field: Field<bool>) {
        self.take(key, field, bool::to_string, |item| {
            item.as_bool()
                .ok_or_else(|| expected("true or false", item))
        });
    }

    fn word<W: Word>(&mut self, key: &'static str, field: Field<W>) {
        self.take(
            key,
            field,
            |word| quoted(written(*word)),
            |item| {
                let words: Vec<String> = W::WORDS.iter().map(|(word, _)| quoted(word)).collect();
                let words = words.join(", ");
                let Some(chosen) = item.as_str() else {
                    return Err(expected(&format!("one of {words}"), item));
                };
                W::WORDS
                    .iter()
                    .find(|(word, _)| word.eq_ignore_ascii_case(chosen))
                    .map(|(_, value)| *value)
                    .ok_or_else(|| format!("{} is not one of {words}", found(item)))
            },
        );
    }

    fn text(&mut self, key: &'static str, field: Field<String>) {
        self.take(
            key,
            field,
            |text| quoted(text),
            |item| match item.as_str() {
                Some(text) if !text.trim().is_empty() => Ok(text.to_string()),
                _ => Err(expected("a name", item)),
            },
        );
    }

    fn number(&mut self, key: &'static str, range: RangeInclusive<f64>, field: Field<f64>) {
        self.take(key, field, f64::to_string, |item| {
            let whole = || item.as_integer().map(|whole| whole as f64);
            let number = item
                .as_float()
                .or_else(whole)
                .ok_or_else(|| expected("a number", item))?;
            if range.contains(&number) {
                Ok(number)
            } else {
                Err(outside(item, range.start(), range.end()))
            }
        });
    }

    fn count(&mut self, key: &'static str, range: RangeInclusive<u32>, field: Field<u32>) {
        self.take(key, field, u32::to_string, |item| {
            let count = item
                .as_integer()
                .ok_or_else(|| expected("a whole number", item))?;
            u32::try_from(count)
                .ok()
                .filter(|count| range.contains(count))
                .ok_or_else(|| outside(item, range.start(), range.end()))
        });
    }

    fn command(&mut self, key: &'static str, field: Field<Option<Vec<String>>>) {
        let show = |_: &Option<Vec<String>>| "your default shell".to_string();
        self.take(key, field, show, |item| {
            let Some(words) = item.as_array() else {
                let form = "a list of strings such as [\"/bin/zsh\", \"-l\"]";
                return Err(expected(form, item));
            };
            let words: Option<Vec<String>> = words
                .iter()
                .map(|word| word.as_str().map(str::to_string))
                .collect();
            match words {
                None => Err("the list holds something that is not a string".to_string()),
                Some(words)
                    if words
                        .first()
                        .is_some_and(|program| !program.trim().is_empty()) =>
                {
                    Ok(Some(words))
                }
                Some(_) => Err("the list has no executable".to_string()),
            }
        });
    }

    /// A pair that breaks the rules is left out alone. The others apply.
    fn pairs(&mut self, key: &'static str, field: Field<BTreeMap<String, String>>) {
        self.keys.push(key);
        let Some(item) = self.table.get(key) else {
            return;
        };
        let Some(pairs) = item.as_table_like() else {
            let form = "a table of word = \"expansion\" lines";
            self.notice(key, expected(form, item), "Using none");
            return;
        };
        for (word, expansion) in pairs.iter() {
            let problem = match expansion.as_str() {
                _ if word.is_empty() || word.contains(char::is_whitespace) => {
                    "an abbreviation is one word with no spaces".to_string()
                }
                None => expected("the expansion as a string", expansion),
                Some(expansion) => {
                    field(&mut self.settings).insert(word.to_string(), expansion.to_string());
                    continue;
                }
            };
            self.notice(&format!("{key}.{word}"), problem, "Ignored");
        }
    }
}

/// The word the file writes for a value.
fn written<W: Word>(value: W) -> &'static str {
    let word = W::WORDS.iter().find(|(_, other)| *other == value);
    word.map_or("", |(word, _)| word)
}

fn expected(what: &str, item: &Item) -> String {
    format!("expected {what}, found {}", found(item))
}

fn outside(item: &Item, least: impl ToString, most: impl ToString) -> String {
    let (least, most) = (least.to_string(), most.to_string());
    format!("{} is not between {least} and {most}", found(item))
}

/// Text as a notice shows it: quoted, and cut short if it is long.
pub(super) fn shown(text: &str) -> String {
    short(quoted(text))
}

/// A value as a notice shows it: as the file writes it if that is short, otherwise by kind.
fn found(item: &Item) -> String {
    let written = match item {
        Item::Value(Value::String(text)) => quoted(text.value()),
        Item::Value(Value::Integer(number)) => number.display_repr().into_owned(),
        Item::Value(Value::Float(number)) => number.display_repr().into_owned(),
        Item::Value(Value::Boolean(flag)) => flag.display_repr().into_owned(),
        Item::Value(Value::Datetime(date)) => date.display_repr().into_owned(),
        Item::Value(Value::Array(_)) | Item::ArrayOfTables(_) => return "a list".to_string(),
        _ => return "a table".to_string(),
    };
    short(written)
}

/// `written`, or its start if it is long.
fn short(written: String) -> String {
    if written.chars().count() <= QUOTED {
        written
    } else {
        let start: String = written.chars().take(QUOTED).collect();
        format!("{start}…")
    }
}

fn quoted(text: &str) -> String {
    format!("\"{text}\"")
}

/// What the parser says is wrong, with where in the file it is.
fn syntax(text: &str, error: &TomlError) -> String {
    let message = error.message().lines().collect::<Vec<_>>().join(", ");
    let message = message.trim_end_matches('.');
    let Some(at) = error.span().map(|span| span.start.min(text.len())) else {
        return message.to_string();
    };
    let before = &text[..at];
    let line = before.matches('\n').count() + 1;
    let column = before
        .rsplit('\n')
        .next()
        .unwrap_or_default()
        .chars()
        .count()
        + 1;
    format!("line {line}, column {column}: {message}")
}

// ---------------------------------------------------------------------------------------
// Writing
// ---------------------------------------------------------------------------------------

/// Writes into a document each setting that differs between two [`Settings`], and touches
/// nothing else in it.
struct Save<'a> {
    table: &'a mut Table,
    previous: Settings,
    next: Settings,
}

impl Save<'_> {
    /// Writes `key` if its setting changed: the value `write` gives, or no key for `None`.
    fn put<T: PartialEq>(
        &mut self,
        key: &'static str,
        field: Field<T>,
        write: impl FnOnce(&T) -> Option<Value>,
    ) {
        let next = field(&mut self.next);
        if field(&mut self.previous) == next {
            return;
        }
        match write(next) {
            Some(value) => set(self.table, key, value),
            None => {
                self.table.remove(key);
            }
        }
    }
}

impl Visit for Save<'_> {
    fn flag(&mut self, key: &'static str, field: Field<bool>) {
        self.put(key, field, |flag| Some(Value::from(*flag)));
    }

    fn word<W: Word>(&mut self, key: &'static str, field: Field<W>) {
        self.put(key, field, |word| Some(Value::from(written(*word))));
    }

    fn text(&mut self, key: &'static str, field: Field<String>) {
        self.put(key, field, |text| Some(Value::from(text.as_str())));
    }

    /// A whole number is written without a fraction: `14`, not `14.0`.
    fn number(&mut self, key: &'static str, _: RangeInclusive<f64>, field: Field<f64>) {
        self.put(key, field, |number| {
            Some(if number.fract() == 0.0 {
                Value::from(*number as i64)
            } else {
                Value::from(*number)
            })
        });
    }

    fn count(&mut self, key: &'static str, _: RangeInclusive<u32>, field: Field<u32>) {
        self.put(key, field, |count| Some(Value::from(i64::from(*count))));
    }

    fn command(&mut self, key: &'static str, field: Field<Option<Vec<String>>>) {
        self.put(key, field, |command| {
            let words = command.as_ref()?;
            Some(Value::Array(words.iter().collect::<Array>()))
        });
    }

    /// Pairs are added, changed and removed one by one, so that the others keep their
    /// comments and a pair the store could not use stays where the user wrote it.
    fn pairs(&mut self, key: &'static str, field: Field<BTreeMap<String, String>>) {
        let (previous, next) = (field(&mut self.previous), field(&mut self.next));
        if previous == next {
            return;
        }
        if !self.table.get(key).is_some_and(Item::is_table_like) {
            self.table.insert(key, Item::Table(Table::new()));
        }
        let Some(pairs) = self.table.get_mut(key).and_then(Item::as_table_like_mut) else {
            return;
        };
        for word in previous.keys().filter(|word| !next.contains_key(*word)) {
            pairs.remove(word);
        }
        for (word, expansion) in next.iter() {
            if previous.get(word) != Some(expansion) {
                set(pairs, word, Value::from(expansion.as_str()));
            }
        }
    }
}

/// A file with no keys holds its comments as what follows the last key. Left there, they
/// would end up below the first key written. This puts them back above it.
fn keep_comments_first(document: &mut DocumentMut) {
    let mut comments = document.trailing().as_str().unwrap_or_default().to_string();
    if comments.trim().is_empty() {
        return;
    }
    if !comments.ends_with('\n') {
        comments.push('\n');
    }
    let Some((mut key, item)) = document.iter_mut().next() else {
        return;
    };
    match item.as_table_mut() {
        Some(table) => table.decor_mut().set_prefix(comments),
        None => key.leaf_decor_mut().set_prefix(comments),
    }
    document.set_trailing("");
}

/// Gives `key` a value. A key that is already there keeps its place and its comments.
fn set(table: &mut dyn TableLike, key: &str, mut value: Value) {
    match table.get_mut(key) {
        Some(item) => {
            if let Some(old) = item.as_value() {
                *value.decor_mut() = old.decor().clone();
            }
            *item = Item::Value(value);
        }
        None => {
            table.insert(key, Item::Value(value));
        }
    }
}
