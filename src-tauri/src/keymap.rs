//! Keyboard shortcuts. One registry feeds both the menu and the Keyboard Shortcuts sheet (UI-06),
//! so the sheet always lists what the menu actually does.
//!
//! Only implemented actions are registered; each feature adds its own when it lands.

use std::collections::BTreeMap;

use serde::Serialize;

/// Who handles the shortcut.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum Kind {
    /// Our own menu item; can be rebound.
    Custom,
    /// A standard macOS menu item; listed for completeness.
    Predefined,
}

struct Action {
    id: &'static str,
    label: &'static str,
    group: &'static str,
    accelerator: Option<&'static str>,
    kind: Kind,
}

const fn custom(
    id: &'static str,
    label: &'static str,
    group: &'static str,
    accelerator: Option<&'static str>,
) -> Action {
    Action {
        id,
        label,
        group,
        accelerator,
        kind: Kind::Custom,
    }
}

const fn predefined(
    id: &'static str,
    label: &'static str,
    group: &'static str,
    accelerator: &'static str,
) -> Action {
    Action {
        id,
        label,
        group,
        accelerator: Some(accelerator),
        kind: Kind::Predefined,
    }
}

const TABS: &str = "Windows and tabs";
const TERMINAL: &str = "Terminal";
const APP: &str = "Application";

#[rustfmt::skip]
const ACTIONS: &[Action] = &[
    custom("new-window", "New Window", TABS, Some("CmdOrCtrl+N")),
    custom("new-tab", "New Tab", TABS, Some("CmdOrCtrl+T")),
    custom("close-tab", "Close Tab", TABS, Some("CmdOrCtrl+W")),
    custom("close-window", "Close Window", TABS, Some("CmdOrCtrl+Shift+W")),
    custom("prev-tab", "Previous Tab", TABS, Some("CmdOrCtrl+Shift+BracketLeft")),
    custom("next-tab", "Next Tab", TABS, Some("CmdOrCtrl+Shift+BracketRight")),
    custom("select-tab-1", "Go to Tab 1", TABS, Some("CmdOrCtrl+1")),
    custom("select-tab-2", "Go to Tab 2", TABS, Some("CmdOrCtrl+2")),
    custom("select-tab-3", "Go to Tab 3", TABS, Some("CmdOrCtrl+3")),
    custom("select-tab-4", "Go to Tab 4", TABS, Some("CmdOrCtrl+4")),
    custom("select-tab-5", "Go to Tab 5", TABS, Some("CmdOrCtrl+5")),
    custom("select-tab-6", "Go to Tab 6", TABS, Some("CmdOrCtrl+6")),
    custom("select-tab-7", "Go to Tab 7", TABS, Some("CmdOrCtrl+7")),
    custom("select-tab-8", "Go to Tab 8", TABS, Some("CmdOrCtrl+8")),
    custom("select-tab-9", "Go to Tab 9", TABS, Some("CmdOrCtrl+9")),
    custom("move-tab-left", "Move Tab Left", TABS, None),
    custom("move-tab-right", "Move Tab Right", TABS, None),
    custom("always-show-tab-bar", "Always Show Tab Bar", TABS, None),
    custom("keyboard-shortcuts", "Keyboard Shortcuts", TABS, Some("CmdOrCtrl+Slash")),
    custom("welcome-tour", "Welcome Tour", TABS, None),
    predefined("copy", "Copy", TERMINAL, "CmdOrCtrl+C"),
    predefined("paste", "Paste", TERMINAL, "CmdOrCtrl+V"),
    predefined("select-all", "Select All", TERMINAL, "CmdOrCtrl+A"),
    predefined("minimize", "Minimize", APP, "CmdOrCtrl+M"),
    predefined("fullscreen", "Enter Full Screen", APP, "Ctrl+CmdOrCtrl+F"),
    predefined("hide", "Hide Twominal", APP, "CmdOrCtrl+H"),
    predefined("hide-others", "Hide Others", APP, "CmdOrCtrl+Alt+H"),
    predefined("quit", "Quit Twominal", APP, "CmdOrCtrl+Q"),
];

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Binding {
    pub id: &'static str,
    pub label: &'static str,
    pub group: &'static str,
    /// Accelerator in menu syntax, e.g. `CmdOrCtrl+Shift+BracketLeft`.
    pub accelerator: Option<String>,
    /// The same in macOS glyphs, e.g. `⇧⌘[`.
    pub display: Option<String>,
    pub kind: Kind,
}

/// The effective bindings: defaults with the user's overrides applied. An empty override unbinds.
pub fn bindings(overrides: &BTreeMap<String, String>) -> Vec<Binding> {
    ACTIONS
        .iter()
        .map(|action| {
            let accelerator = match (action.kind, overrides.get(action.id)) {
                (Kind::Custom, Some(custom)) => Some(custom.trim())
                    .filter(|a| !a.is_empty())
                    .map(str::to_owned),
                _ => action.accelerator.map(str::to_owned),
            };
            Binding {
                id: action.id,
                label: action.label,
                group: action.group,
                display: accelerator.as_deref().map(display),
                accelerator,
                kind: action.kind,
            }
        })
        .collect()
}

/// Looks up the label and accelerator of an action for building its menu item.
pub fn find<'a>(bindings: &'a [Binding], id: &str) -> &'a Binding {
    bindings
        .iter()
        .find(|binding| binding.id == id)
        .unwrap_or_else(|| panic!("unknown action {id}"))
}

/// Renders an accelerator in macOS glyphs, modifiers in the standard ⌃⌥⇧⌘ order.
pub fn display(accelerator: &str) -> String {
    let mut parts: Vec<&str> = accelerator.split('+').map(str::trim).collect();
    let key = parts.pop().unwrap_or_default();
    let has = |names: &[&str]| {
        parts
            .iter()
            .any(|part| names.iter().any(|name| part.eq_ignore_ascii_case(name)))
    };

    let mut out = String::new();
    for (names, glyph) in [
        (&["Ctrl", "Control"][..], '⌃'),
        (&["Alt", "Option"][..], '⌥'),
        (&["Shift"][..], '⇧'),
        (
            &[
                "Cmd",
                "Command",
                "CmdOrCtrl",
                "CommandOrControl",
                "Super",
                "Meta",
            ][..],
            '⌘',
        ),
    ] {
        if has(names) {
            out.push(glyph);
        }
    }
    out.push_str(&key_glyph(key));
    out
}

fn key_glyph(key: &str) -> String {
    let named = match key {
        "BracketLeft" => "[",
        "BracketRight" => "]",
        "Slash" => "/",
        "Backslash" => "\\",
        "Comma" => ",",
        "Period" => ".",
        "Semicolon" => ";",
        "Quote" => "'",
        "Backquote" => "`",
        "Minus" => "-",
        "Equal" => "=",
        "Enter" | "Return" => "↩",
        "Tab" => "⇥",
        "Space" => "Space",
        "Backspace" => "⌫",
        "Delete" => "⌦",
        "Escape" | "Esc" => "⎋",
        "ArrowUp" | "Up" => "↑",
        "ArrowDown" | "Down" => "↓",
        "ArrowLeft" | "Left" => "←",
        "ArrowRight" | "Right" => "→",
        _ => {
            let bare = key
                .strip_prefix("Key")
                .or_else(|| key.strip_prefix("Digit"))
                .unwrap_or(key);
            return bare.to_uppercase();
        }
    };
    named.to_owned()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;
    use std::str::FromStr;

    use muda::accelerator::Accelerator;

    #[test]
    fn every_default_accelerator_parses_and_none_collide() {
        let mut seen = HashSet::new();
        for binding in bindings(&BTreeMap::new()) {
            let Some(accelerator) = binding.accelerator else {
                continue;
            };
            let parsed = Accelerator::from_str(&accelerator).unwrap_or_else(|err| {
                panic!("{}: {accelerator:?} does not parse: {err}", binding.id)
            });
            assert!(seen.insert(parsed), "{} reuses {accelerator}", binding.id);
        }
    }

    #[test]
    fn display_uses_macos_glyph_order() {
        assert_eq!(display("CmdOrCtrl+Shift+BracketLeft"), "⇧⌘[");
        assert_eq!(display("CmdOrCtrl+Slash"), "⌘/");
        assert_eq!(display("Ctrl+CmdOrCtrl+F"), "⌃⌘F");
        assert_eq!(display("CmdOrCtrl+Alt+H"), "⌥⌘H");
        assert_eq!(display("CmdOrCtrl+1"), "⌘1");
        assert_eq!(display("Shift+Alt+KeyT"), "⌥⇧T");
    }

    #[test]
    fn overrides_rebind_and_unbind_custom_actions_only() {
        let overrides = BTreeMap::from([
            ("new-tab".to_owned(), "CmdOrCtrl+Alt+T".to_owned()),
            ("close-tab".to_owned(), String::new()),
            ("quit".to_owned(), "CmdOrCtrl+X".to_owned()),
        ]);
        let bindings = bindings(&overrides);
        assert_eq!(find(&bindings, "new-tab").display.as_deref(), Some("⌥⌘T"));
        assert_eq!(find(&bindings, "close-tab").accelerator, None);
        assert_eq!(find(&bindings, "quit").display.as_deref(), Some("⌘Q"));
    }
}
