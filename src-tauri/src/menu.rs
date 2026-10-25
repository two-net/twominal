//! The app menu (UI-01). Items and shortcuts come from the keymap.
//!
//! Menu actions go to the window the user last focused, whose frontend decides what they mean.
//! Only actions that need no window are handled here.

use tauri::menu::{
    AboutMetadata, CheckMenuItem, CheckMenuItemBuilder, Menu, MenuBuilder, MenuEvent, MenuItem,
    MenuItemBuilder, PredefinedMenuItem, SubmenuBuilder, HELP_SUBMENU_ID, WINDOW_SUBMENU_ID,
};
use tauri::{AppHandle, Emitter, EventTarget, Manager, Wry};

use crate::keymap;
use crate::settings::SettingsStore;
use crate::windows;

/// Menu items whose state follows settings.
pub struct MenuHandles {
    always_show_tab_bar: CheckMenuItem<Wry>,
}

/// Builds the menu and sets it as the app menu.
pub fn install(app: &AppHandle) -> tauri::Result<()> {
    let settings = app.state::<SettingsStore>().get();
    let bindings = keymap::bindings(&settings.keymap);
    let name = app.package_info().name.clone();

    let item = |id: &str| -> tauri::Result<MenuItem<Wry>> {
        let binding = keymap::find(&bindings, id);
        let mut builder = MenuItemBuilder::with_id(id, binding.label);
        if let Some(accelerator) = &binding.accelerator {
            builder = builder.accelerator(accelerator);
        }
        builder.build(app)
    };

    let about = AboutMetadata {
        name: Some(name.clone()),
        version: Some(app.package_info().version.to_string()),
        copyright: app.config().bundle.copyright.clone(),
        icon: app.default_window_icon().cloned(),
        ..Default::default()
    };
    // Item texts name the app explicitly so they read "Twominal" in dev builds too.
    let app_menu = SubmenuBuilder::new(app, &name)
        .item(&PredefinedMenuItem::about(
            app,
            Some(&format!("About {name}")),
            Some(about),
        )?)
        .separator()
        .item(&PredefinedMenuItem::services(app, None)?)
        .separator()
        .item(&PredefinedMenuItem::hide(
            app,
            Some(&format!("Hide {name}")),
        )?)
        .item(&PredefinedMenuItem::hide_others(app, None)?)
        .item(&PredefinedMenuItem::show_all(app, None)?)
        .separator()
        .item(&PredefinedMenuItem::quit(
            app,
            Some(&format!("Quit {name}")),
        )?)
        .build()?;

    // Custom close items: the predefined Close Window would take ⌘W from Close Tab.
    let shell = SubmenuBuilder::new(app, "Shell")
        .item(&item("new-window")?)
        .item(&item("new-tab")?)
        .separator()
        .item(&item("close-tab")?)
        .item(&item("close-window")?)
        .build()?;

    let edit = SubmenuBuilder::new(app, "Edit")
        .copy()
        .paste()
        .select_all()
        .build()?;

    let always_show_tab_bar = CheckMenuItemBuilder::with_id(
        "always-show-tab-bar",
        keymap::find(&bindings, "always-show-tab-bar").label,
    )
    .checked(settings.always_show_tab_bar)
    .build(app)?;
    let mut view = SubmenuBuilder::new(app, "View")
        .item(&always_show_tab_bar)
        .separator()
        .fullscreen();
    if cfg!(debug_assertions) {
        view = view
            .separator()
            .text("show-test-overlay", "Show Test Overlay");
    }
    let view = view.build()?;

    let mut go_to_tab = SubmenuBuilder::new(app, "Go to Tab");
    for n in 1..=9 {
        go_to_tab = go_to_tab.item(&item(&format!("select-tab-{n}"))?);
    }
    let window = SubmenuBuilder::with_id(app, WINDOW_SUBMENU_ID, "Window")
        .minimize()
        .maximize()
        .separator()
        .item(&item("prev-tab")?)
        .item(&item("next-tab")?)
        .item(&go_to_tab.build()?)
        .separator()
        .item(&item("move-tab-left")?)
        .item(&item("move-tab-right")?)
        .separator()
        .item(&PredefinedMenuItem::bring_all_to_front(app, None)?)
        .build()?;

    let help = SubmenuBuilder::with_id(app, HELP_SUBMENU_ID, "Help")
        .item(&item("keyboard-shortcuts")?)
        .item(&item("welcome-tour")?)
        .build()?;

    let menu: Menu<Wry> = MenuBuilder::new(app)
        .items(&[&app_menu, &shell, &edit, &view, &window, &help])
        .build()?;
    app.set_menu(menu)?;
    app.manage(MenuHandles {
        always_show_tab_bar,
    });
    Ok(())
}

pub fn handle(app: &AppHandle, event: MenuEvent) {
    let id = event.id().as_ref();
    if id == "always-show-tab-bar" {
        let settings = app.state::<SettingsStore>().update(app, |settings| {
            settings.always_show_tab_bar = !settings.always_show_tab_bar
        });
        let _ = app
            .state::<MenuHandles>()
            .always_show_tab_bar
            .set_checked(settings.always_show_tab_bar);
        return;
    }

    match windows::menu_target(app) {
        Some(label) => {
            let _ = app.emit_to(EventTarget::webview_window(label), "menu-action", id);
        }
        None if matches!(id, "new-window" | "new-tab") => {
            let _ = windows::open(app, None);
        }
        None => {}
    }
}
