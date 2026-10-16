//! The find bar (TERM-18, TERM-19): the query typed into it and its toggles. The Terminal Core
//! searches; this edits what it looks for, with the keys an OS's own text fields take. It holds
//! one line, and keeps it, with the toggles, while the bar is closed, so that the tab finds the
//! same again when it opens (TAB-01). DESIGN.md §3 *Find bar* has the keys.

use terminal_core::{Search, QUERY};

use super::key::{Key, Mods, Named, Pad, Press};
use super::shortcut::is;

/// The find bar of one tab.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct FindBar {
    open: bool,
    /// The bar has the keys: what is typed edits the query, and reaches no program.
    focused: bool,
    query: String,
    /// Where the caret is in the query, in bytes, at a character's start.
    caret: usize,
    /// The whole query is selected, so that what is typed takes its place.
    selected: bool,
    case: bool,
    regex: bool,
    /// The text an input method is composing at the caret.
    composing: String,
}

/// What a key or a change does to the bar.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Edit {
    /// The query or a toggle changed, so the search is another.
    Search,
    /// Only how the bar looks changed: the caret, the selection or the composition.
    Look,
    /// Go to the match above the current one, or below it.
    Step { up: bool },
    /// Close the bar.
    Close,
    /// Nothing changed.
    Nothing,
}

impl FindBar {
    pub fn is_open(&self) -> bool {
        self.open
    }

    /// Whether the bar has the keys.
    pub fn is_focused(&self) -> bool {
        self.open && self.focused
    }

    pub fn query(&self) -> &str {
        &self.query
    }

    /// Where the caret is in the query, in bytes.
    pub fn caret(&self) -> usize {
        self.caret
    }

    /// Whether the whole query is selected, so that what is typed takes its place.
    pub fn is_selected(&self) -> bool {
        self.selected
    }

    /// Whether letter case counts.
    pub fn case(&self) -> bool {
        self.case
    }

    /// Whether the query is a regular expression.
    pub fn regex(&self) -> bool {
        self.regex
    }

    /// The text an input method is composing at the caret.
    pub fn composing(&self) -> &str {
        &self.composing
    }

    /// What the bar looks for: None while it is closed, or its query is empty.
    pub fn search(&self) -> Option<Search> {
        (self.open && !self.query.is_empty()).then(|| Search {
            query: self.query.clone(),
            case: self.case,
            regex: self.regex,
        })
    }

    /// Opens the bar, if it was closed, and gives it the keys, its query all selected, as Mod+F
    /// does. The query is the one it had.
    pub(crate) fn open(&mut self) -> Edit {
        let opened = !self.open;
        self.open = true;
        self.focus();
        self.selected = !self.query.is_empty();
        self.caret = self.query.len();
        if opened {
            Edit::Search
        } else {
            Edit::Look
        }
    }

    /// Gives the bar the keys, as a click in its field does.
    pub(crate) fn focus(&mut self) {
        self.focused = true;
    }

    /// Gives the keys back to the program. The bar stays open, and its matches drawn.
    pub(crate) fn blur(&mut self) {
        self.focused = false;
        self.composing.clear();
    }

    /// Closes the bar. Its query and toggles are kept for the next time it opens.
    pub(crate) fn close(&mut self) -> Edit {
        if !self.open {
            return Edit::Nothing;
        }
        self.open = false;
        self.blur();
        self.selected = false;
        Edit::Close
    }

    /// Turns letter case counting on or off.
    pub(crate) fn toggle_case(&mut self) -> Edit {
        self.case = !self.case;
        Edit::Search
    }

    /// Turns reading the query as a regular expression on or off.
    pub(crate) fn toggle_regex(&mut self) -> Edit {
        self.regex = !self.regex;
        Edit::Search
    }

    /// Selects the whole query, as Mod+A does in a text field.
    pub(crate) fn select_all(&mut self) -> Edit {
        if self.query.is_empty() || self.selected {
            return Edit::Nothing;
        }
        self.selected = true;
        Edit::Look
    }

    /// The text an input method is composing changed.
    pub(crate) fn compose(&mut self, text: String) -> Edit {
        if text == self.composing {
            return Edit::Nothing;
        }
        self.composing = text;
        Edit::Look
    }

    /// Puts `text` at the caret, or in place of the query while it is selected: what was typed,
    /// committed or pasted. A query is one line: every control character becomes a space. It
    /// keeps [`QUERY`] characters, and what would go past them is dropped.
    pub(crate) fn insert(&mut self, text: &str) -> Edit {
        let composed = !self.composing.is_empty();
        self.composing.clear();
        let room = if self.selected {
            QUERY
        } else {
            QUERY.saturating_sub(self.query.chars().count())
        };
        let text: String = text
            .chars()
            .map(|char| if char.is_control() { ' ' } else { char })
            .take(room)
            .collect();
        // Nothing to put in, as when an input method commits nothing: the query stays.
        if text.is_empty() {
            return if composed { Edit::Look } else { Edit::Nothing };
        }
        if self.selected {
            self.query.clear();
            self.caret = 0;
            self.selected = false;
        }
        self.query.insert_str(self.caret, &text);
        self.caret += text.len();
        Edit::Search
    }

    /// What `press`, a key going down or repeating while the bar has the keys, does to it. On
    /// `macos` a word is Option's and the whole line Cmd's, and elsewhere a word is Ctrl's.
    pub(crate) fn key(&mut self, press: &Press, macos: bool) -> Edit {
        let held = press.mods.held();
        let word = if macos { Mods::ALT } else { Mods::CTRL };
        let named = match press.key {
            Key::Named(named) => Some(named),
            Key::Pad(Pad::Enter) => Some(Named::Enter),
            Key::Pad(Pad::Left) => Some(Named::Left),
            Key::Pad(Pad::Right) => Some(Named::Right),
            Key::Pad(Pad::Home) => Some(Named::Home),
            Key::Pad(Pad::End) => Some(Named::End),
            Key::Pad(Pad::Delete) => Some(Named::Delete),
            _ => None,
        };
        // Shift selects in a text field. The bar selects only the whole query, so Shift moves.
        let unshifted = held & !Mods::SHIFT;
        let toggles = if macos {
            Mods::ALT | Mods::SUPER
        } else {
            Mods::ALT
        };
        match named {
            Some(Named::Enter) if held & !Mods::SHIFT == 0 => Edit::Step {
                up: held != Mods::SHIFT,
            },
            Some(Named::Escape) if held == 0 => self.close(),
            Some(Named::Backspace) => match unshifted {
                0 => self.delete(self.before(), self.caret),
                mods if mods == word => self.delete(self.word_before(), self.caret),
                Mods::SUPER if macos => self.delete(0, self.caret),
                _ => Edit::Nothing,
            },
            Some(Named::Delete) => match unshifted {
                0 => self.delete(self.caret, self.after()),
                mods if mods == word => self.delete(self.caret, self.word_after()),
                _ => Edit::Nothing,
            },
            Some(Named::Left) => match unshifted {
                0 => self.go(self.before(), false),
                mods if mods == word => self.go(self.word_before(), false),
                Mods::SUPER if macos => self.go(0, false),
                _ => Edit::Nothing,
            },
            Some(Named::Right) => match unshifted {
                0 => self.go(self.after(), true),
                mods if mods == word => self.go(self.word_after(), true),
                Mods::SUPER if macos => self.go(self.query.len(), true),
                _ => Edit::Nothing,
            },
            Some(Named::Home) if unshifted == 0 => self.go(0, false),
            Some(Named::End) if unshifted == 0 => self.go(self.query.len(), true),
            Some(_) => Edit::Nothing,
            None if held == toggles && is(press, 'c') => self.toggle_case(),
            None if held == toggles && is(press, 'r') => self.toggle_regex(),
            None => match press.text() {
                "" => Edit::Nothing,
                text => self.insert(text),
            },
        }
    }

    /// Moves the caret to `to`. While the whole query is selected, Left and Right go to its
    /// start and its end, as a text field's do, which `end` says.
    fn go(&mut self, to: usize, end: bool) -> Edit {
        let to = if self.selected {
            if end {
                self.query.len()
            } else {
                0
            }
        } else {
            to
        };
        if to == self.caret && !self.selected {
            return Edit::Nothing;
        }
        self.caret = to;
        self.selected = false;
        Edit::Look
    }

    /// Deletes the query from `from` to `to`, or the whole query while it is selected.
    fn delete(&mut self, from: usize, to: usize) -> Edit {
        let range = if self.selected {
            0..self.query.len()
        } else {
            from..to
        };
        self.selected = false;
        if range.is_empty() {
            return Edit::Look;
        }
        self.caret = range.start;
        self.query.replace_range(range, "");
        Edit::Search
    }

    /// Where the character before the caret starts.
    fn before(&self) -> usize {
        self.query[..self.caret]
            .char_indices()
            .next_back()
            .map_or(0, |(at, _)| at)
    }

    /// Where the character after the caret ends.
    fn after(&self) -> usize {
        self.query[self.caret..]
            .chars()
            .next()
            .map_or(self.caret, |char| self.caret + char.len_utf8())
    }

    /// Where the word before the caret starts: past the blanks before it, then the rest.
    fn word_before(&self) -> usize {
        let before = &self.query[..self.caret];
        let word = before.trim_end_matches(char::is_whitespace);
        word.trim_end_matches(|char: char| !char.is_whitespace())
            .len()
    }

    /// Where the word after the caret ends: past the blanks after it, then the rest.
    fn word_after(&self) -> usize {
        let after = &self.query[self.caret..];
        let word = after.trim_start_matches(char::is_whitespace);
        let rest = word.trim_start_matches(|char: char| !char.is_whitespace());
        self.query.len() - rest.len()
    }
}
