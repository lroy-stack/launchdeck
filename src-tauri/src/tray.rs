//! The system tray: open the window, switch the surface, quit.

use std::sync::Arc;

use tauri::menu::{Menu, MenuItem, PredefinedMenuItem, Submenu};
use tauri::tray::TrayIconBuilder;
use tauri::Manager;

use crate::core::runtime::Runtime;

pub(crate) fn build_tray(app: &tauri::App, runtime: &Arc<Runtime>) -> tauri::Result<()> {
    let open = MenuItem::with_id(app, "open", "Open LaunchDeck", true, None::<&str>)?;
    let surfaces = runtime.snapshot().workspace.surfaces;
    let items: Vec<MenuItem<tauri::Wry>> = surfaces
        .iter()
        .map(|s| {
            MenuItem::with_id(
                app,
                format!("surface:{}", s.id),
                &s.name,
                true,
                None::<&str>,
            )
        })
        .collect::<tauri::Result<_>>()?;
    let item_refs: Vec<&dyn tauri::menu::IsMenuItem<tauri::Wry>> = items
        .iter()
        .map(|i| i as &dyn tauri::menu::IsMenuItem<tauri::Wry>)
        .collect();
    let submenu = Submenu::with_items(app, "Surface", true, &item_refs)?;
    let quit = MenuItem::with_id(app, "quit", "Quit LaunchDeck", true, None::<&str>)?;
    let sep = PredefinedMenuItem::separator(app)?;
    let menu = Menu::with_items(app, &[&open, &submenu, &sep, &quit])?;

    let rt = runtime.clone();
    let mut builder = TrayIconBuilder::new().menu(&menu).tooltip("LaunchDeck");
    if let Some(icon) = app.default_window_icon() {
        builder = builder.icon(icon.clone());
    }
    builder
        .on_menu_event(move |app, event| match event.id().as_ref() {
            "open" => {
                if let Some(w) = app.get_webview_window("main") {
                    let _ = w.show();
                    let _ = w.unminimize();
                    let _ = w.set_focus();
                }
            }
            "quit" => app.exit(0),
            id => {
                if let Some(surface) = id.strip_prefix("surface:") {
                    if let Err(e) = rt.activate_surface(surface, None) {
                        log::warn!("tray: {e}");
                    }
                }
            }
        })
        .build(app)?;
    Ok(())
}
