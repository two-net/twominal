//! The native menus (UI-05). Part of the App Shell.
//!
//! The menus hold only what works: each feature adds its own items. DESIGN.md §3 lists what
//! each OS gets and why they differ.
//!
//! No menu or item has a mnemonic. Off macOS a mnemonic would take Alt and its letter, which
//! the command line and programs use.

use tauri::menu::{
    AboutMetadata, CheckMenuItem, CheckMenuItemBuilder, IsMenuItem, Menu, MenuBuilder,
    MenuItemBuilder, SubmenuBuilder, HELP_SUBMENU_ID, WINDOW_SUBMENU_ID,
};

use crate::settings::Theme;

use super::fonts::Zoom;

/// What choosing one of Twominal's own items asks for. The items the OS provides, such as
/// Copy or Minimize, are carried out by the OS.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Action {
    NewWindow,
    /// Off macOS only. There the OS has an item of its own.
    CloseWindow,
    /// Off macOS only. There the OS has an item of its own.
    Quit,
    Theme(Theme),
    /// A font size item: it changes the Font size setting (FONT-02).
    FontSize(Zoom),
}

const NEW_WINDOW: &str = "new-window";
const CLOSE_WINDOW: &str = "close-window";
const QUIT: &str = "quit";

/// The themes in menu order: the item's id, its text and the setting it chooses.
const THEMES: [(&str, &str, Theme); 3] = [
    ("theme-dark", "Dark", Theme::Dark),
    ("theme-light", "Light", Theme::Light),
    ("theme-system", "System", Theme::System),
];

/// The font size items in menu order: the item's id, its text and what it asks for.
const ZOOMS: [(&str, &str, Zoom); 3] = [
    ("font-bigger", "Increase Font Size", Zoom::Bigger),
    ("font-smaller", "Decrease Font Size", Zoom::Smaller),
    ("font-reset", "Reset Font Size", Zoom::Reset),
];

/// What the item with this id asks for. `None` for an item the OS carries out.
pub(super) fn action(id: &str) -> Option<Action> {
    let theme = || THEMES.iter().find(|(item, ..)| *item == id);
    let zoom = || ZOOMS.iter().find(|(item, ..)| *item == id);
    match id {
        NEW_WINDOW => Some(Action::NewWindow),
        CLOSE_WINDOW => Some(Action::CloseWindow),
        QUIT => Some(Action::Quit),
        _ => theme()
            .map(|&(.., theme)| Action::Theme(theme))
            .or_else(|| zoom().map(|&(.., zoom)| Action::FontSize(zoom))),
    }
}

/// New Window's shortcut (Appendix A): Mod+N, where Mod is Cmd on macOS and Ctrl+Shift
/// elsewhere, so that no plain Ctrl+letter is taken from the shell.
pub(super) fn new_window_shortcut(macos: bool) -> &'static str {
    if macos {
        "Cmd+N"
    } else {
        "Ctrl+Shift+N"
    }
}

/// A font size item's shortcut (Appendix A): Mod with =, - or 0, where Mod is Cmd on macOS
/// and Ctrl+Shift elsewhere.
pub(super) fn font_size_shortcut(zoom: Zoom, macos: bool) -> &'static str {
    match (zoom, macos) {
        (Zoom::Bigger, true) => "Cmd+=",
        (Zoom::Smaller, true) => "Cmd+-",
        (Zoom::Reset, true) => "Cmd+0",
        (Zoom::Bigger, false) => "Ctrl+Shift+=",
        (Zoom::Smaller, false) => "Ctrl+Shift+-",
        (Zoom::Reset, false) => "Ctrl+Shift+0",
    }
}

/// The menu items that show state.
pub(super) struct Menus {
    themes: Vec<(Theme, CheckMenuItem<tauri::Wry>)>,
}

impl Menus {
    /// Builds the menu bar of this OS, with `theme` checked. `product` is Twominal's name as
    /// the user reads it (GEN-01).
    pub(super) fn build(
        app: &tauri::AppHandle,
        product: &str,
        theme: Theme,
    ) -> tauri::Result<(Menu<tauri::Wry>, Self)> {
        let macos = cfg!(target_os = "macos");

        let mut themes = Vec::new();
        let mut theme_menu = SubmenuBuilder::new(app, "Theme");
        for (id, text, value) in THEMES {
            let item = CheckMenuItemBuilder::with_id(id, text)
                .checked(value == theme)
                .build(app)?;
            theme_menu = theme_menu.item(&item);
            themes.push((value, item));
        }
        let theme_menu = theme_menu.build()?;

        // Never disabled, even at the smallest or largest size: the key of a disabled item
        // would go on to the program.
        let zoom_items = ZOOMS
            .iter()
            .map(|&(id, text, zoom)| {
                MenuItemBuilder::with_id(id, text)
                    .accelerator(font_size_shortcut(zoom, macos))
                    .build(app)
            })
            .collect::<tauri::Result<Vec<_>>>()?;
        let zooms: Vec<&dyn IsMenuItem<tauri::Wry>> =
            zoom_items.iter().map(|item| item as _).collect();

        let new_window = MenuItemBuilder::with_id(NEW_WINDOW, "New Window")
            .accelerator(new_window_shortcut(macos))
            .build(app)?;
        let about = AboutMetadata {
            name: Some(product.to_string()),
            version: Some(app.package_info().version.to_string()),
            // The logo (GEN-03). The dialog of Windows has no place for one.
            icon: Some(tauri::include_image!("./icons/128x128.png")),
            ..Default::default()
        };

        let menu = if macos {
            let twominal = SubmenuBuilder::new(app, product)
                .about_with_text(format!("About {product}"), Some(about))
                .separator()
                .services()
                .separator()
                .hide_with_text(format!("Hide {product}"))
                .hide_others()
                .show_all()
                .separator()
                .quit_with_text(format!("Quit {product}"))
                .build()?;
            let file = SubmenuBuilder::new(app, "File")
                .item(&new_window)
                .separator()
                .close_window()
                .build()?;
            // These act on text fields in the page. Copy, Paste and Select All that act on the
            // terminal come with #31, which first shows a tab (DESIGN.md §3, *Clipboard*).
            let edit = SubmenuBuilder::new(app, "Edit")
                .undo()
                .redo()
                .separator()
                .cut()
                .copy()
                .paste()
                .select_all()
                .build()?;
            let view = SubmenuBuilder::new(app, "View")
                .item(&theme_menu)
                .separator()
                .items(&zooms)
                .separator()
                .fullscreen_with_text("Enter Full Screen")
                .build()?;
            // Under these two ids macOS lists the open windows and adds the search field.
            let window = SubmenuBuilder::with_id(app, WINDOW_SUBMENU_ID, "Window")
                .minimize()
                .maximize()
                .separator()
                .bring_all_to_front()
                .build()?;
            let help = SubmenuBuilder::with_id(app, HELP_SUBMENU_ID, "Help").build()?;
            MenuBuilder::new(app)
                .items(&[&twominal, &file, &edit, &view, &window, &help])
                .build()?
        } else {
            // Close Window and Quit are Twominal's own here: Linux has no ready-made ones.
            // There is no Edit menu yet: the ready-made Copy would take Ctrl+C from the shell,
            // and Twominal's own acts on a tab, which #31 first shows.
            let file = SubmenuBuilder::new(app, "File")
                .item(&new_window)
                .separator()
                .text(CLOSE_WINDOW, "Close Window")
                .text(QUIT, "Quit")
                .build()?;
            let view = SubmenuBuilder::new(app, "View")
                .item(&theme_menu)
                .separator()
                .items(&zooms)
                .build()?;
            let help = SubmenuBuilder::new(app, "Help")
                .about_with_text(format!("About {product}"), Some(about))
                .build()?;
            MenuBuilder::new(app)
                .items(&[&file, &view, &help])
                .build()?
        };

        Ok((menu, Self { themes }))
    }

    /// Checks the item of `theme` and no other.
    pub(super) fn show_theme(&self, theme: Theme) -> tauri::Result<()> {
        for (value, item) in &self.themes {
            item.set_checked(*value == theme)?;
        }
        Ok(())
    }
}
