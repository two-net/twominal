//! Tests of the App Shell: what needs no window (GEN-04, TAB-15, UI-05). The windows and
//! menus themselves are checked by hand (DESIGN.md §8).

use super::menu::{self, Action};
use super::*;

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
        tabs: TabManager::new(),
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
