//! Native macOS menu bar built from the action registry (Story 9.3, epic 9 spine).
//!
//! keeper ships no hand-maintained menu: the whole application submenu set is
//! derived from `keeper_core::palette::registry_sections()` — the same projection
//! the ⌘? cheat sheet renders and, transitively, the same `palette_actions()`
//! registry the ⌘K palette consumes. Adding or removing a registry action changes
//! this menu automatically (UX-DR15), so the reference can never drift from reality.
//!
//! Two deliberate choices:
//!
//! - **No native accelerators.** Every registry chord (⌘1–4, ⌘N, ⌘⇧F) is already
//!   owned by a shipped JS window hook, and the single-key verbs (E/P/F/U) are
//!   context-scoped chat-list keys. Binding OS accelerators here would double-fire
//!   or hijack typing. The shortcut is shown as menu-item **label text** for
//!   discovery/VoiceOver; the JS hooks stay the sole binding owner.
//! - **Standard menus preserved.** Replacing Tauri's default menu re-adds the
//!   predefined App (About/Quit), Edit (Undo/Redo/Cut/Copy/Paste/Select All), and
//!   Window (Minimize/Zoom) submenus, then appends one generated submenu per
//!   registry category. Losing Copy/Paste would be a regression.
//!
//! A menu click emits [`MENU_ACTION_EVENT`] carrying the clicked item id; the
//! frontend `use-menu-actions` hook resolves the open-chat context + toggle
//! direction and routes it through the existing `dispatchPaletteAction` map — no
//! second dispatch table.

use keeper_core::palette::registry_sections;
use keeper_core::vm::MenuItemVm;
use tauri::menu::{Menu, MenuItem, PredefinedMenuItem, Submenu};
use tauri::{AppHandle, Emitter, Runtime};

/// The event a native menu click emits, carrying the clicked item's canonical id as
/// its payload. `keeper://kebab-case` per the epic event-naming convention.
pub const MENU_ACTION_EVENT: &str = "keeper://menu-action";

/// Build the label a generated menu item shows: `"Open Inbox  ⌘1"` when the item has
/// a shortcut, else just the title. The shortcut is DISPLAY-ONLY (no accelerator is
/// bound) so the JS hooks remain the sole binding owner.
fn item_label(item: &MenuItemVm) -> String {
    match &item.shortcut {
        Some(shortcut) => format!("{}  {}", item.title, shortcut),
        None => item.title.clone(),
    }
}

/// Build the full application menu from [`registry_sections`], with the standard
/// macOS App/Edit/Window submenus preserved. Item ids are the canonical registry
/// (dispatch) ids; a collapsed toggle item carries its positive-direction id, which
/// the frontend flips per the open room's flag.
pub fn build_menu<R: Runtime>(app: &AppHandle<R>) -> tauri::Result<Menu<R>> {
    // --- Standard macOS menus (must survive replacing the default menu). ---
    let app_menu = Submenu::with_items(
        app,
        "keeper",
        true,
        &[
            &PredefinedMenuItem::about(app, None, None)?,
            &PredefinedMenuItem::separator(app)?,
            &PredefinedMenuItem::services(app, None)?,
            &PredefinedMenuItem::separator(app)?,
            &PredefinedMenuItem::hide(app, None)?,
            &PredefinedMenuItem::hide_others(app, None)?,
            &PredefinedMenuItem::show_all(app, None)?,
            &PredefinedMenuItem::separator(app)?,
            &PredefinedMenuItem::quit(app, None)?,
        ],
    )?;

    let edit_menu = Submenu::with_items(
        app,
        "Edit",
        true,
        &[
            &PredefinedMenuItem::undo(app, None)?,
            &PredefinedMenuItem::redo(app, None)?,
            &PredefinedMenuItem::separator(app)?,
            &PredefinedMenuItem::cut(app, None)?,
            &PredefinedMenuItem::copy(app, None)?,
            &PredefinedMenuItem::paste(app, None)?,
            &PredefinedMenuItem::select_all(app, None)?,
        ],
    )?;

    let window_menu = Submenu::with_items(
        app,
        "Window",
        true,
        &[
            &PredefinedMenuItem::minimize(app, None)?,
            &PredefinedMenuItem::maximize(app, None)?,
        ],
    )?;

    let menu = Menu::new(app)?;
    menu.append(&app_menu)?;
    menu.append(&edit_menu)?;

    // --- One generated submenu per registry category (single source of truth). ---
    for section in registry_sections() {
        // Each generated item's id IS its canonical registry dispatch id; no
        // accelerator is bound (the JS hooks own every binding).
        let mut items: Vec<MenuItem<R>> = Vec::with_capacity(section.items.len());
        for item in &section.items {
            items.push(MenuItem::with_id(
                app,
                item.id.clone(),
                item_label(item),
                true,
                None::<&str>,
            )?);
        }
        let refs: Vec<&dyn tauri::menu::IsMenuItem<R>> = items
            .iter()
            .map(|i| i as &dyn tauri::menu::IsMenuItem<R>)
            .collect();
        let submenu = Submenu::with_items(app, &section.category, true, &refs)?;
        menu.append(&submenu)?;
    }

    menu.append(&window_menu)?;
    Ok(menu)
}

/// Handle a native menu click: emit [`MENU_ACTION_EVENT`] with the clicked item's
/// canonical id so the frontend can dispatch it through `dispatchPaletteAction`. The
/// predefined App/Edit/Window items are handled natively by the OS and never reach
/// here as a keeper action; only the generated registry items carry ids we emit. A
/// failed emit is logged (id only, never content) and swallowed — a menu click must
/// never crash the app.
pub fn handle_menu_event<R: Runtime>(app: &AppHandle<R>, id: &str) {
    if let Err(error) = app.emit(MENU_ACTION_EVENT, id) {
        tracing::warn!(menu_action_id = id, %error, "failed to emit menu-action event");
    }
}
