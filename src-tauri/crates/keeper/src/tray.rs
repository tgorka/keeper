//! Opt-in menu-bar (tray) presence (Story 10.3, FR-53).
//!
//! When enabled, keeper shows a macOS menu-bar item that keeps the app reachable while
//! the window is hidden: "Show keeper" raises + focuses the main window, "Quit" exits the
//! process. The presence is opt-in (off by default) and persisted in `keeper.db`; the
//! shell creates or destroys the tray icon live off the Settings toggle and rebuilds it at
//! startup when the persisted setting is on.
//!
//! Tray glue is a shell/OS concern (AD-24) — `keeper-core` only owns the persisted
//! *mode/flag*. Every step here is best-effort: a tray build failure is logged at `warn`
//! and the app keeps running (the tray is a convenience, never load-bearing).

use std::sync::{Mutex, OnceLock};

use tauri::menu::{MenuBuilder, MenuItemBuilder};
use tauri::tray::{TrayIcon, TrayIconBuilder};
use tauri::{AppHandle, Manager};

/// The label of the main window (matches `tauri.conf.json` / the default capability).
const MAIN_WINDOW_LABEL: &str = "main";

/// The menu item id for "Show keeper".
const SHOW_ID: &str = "tray-show";
/// The menu item id for "Quit".
const QUIT_ID: &str = "tray-quit";

/// The live tray icon handle, if the tray is currently present. Held so
/// [`set_tray_presence`] can destroy it when the user toggles menu-bar presence off.
/// `None` means no tray is shown (the default). Guarded by a `Mutex` since the toggle
/// command and startup both touch it.
static TRAY: OnceLock<Mutex<Option<TrayIcon>>> = OnceLock::new();

fn tray_slot() -> &'static Mutex<Option<TrayIcon>> {
    TRAY.get_or_init(|| Mutex::new(None))
}

/// Raise and focus the main window (the tray "Show keeper" action + a re-summon path).
/// Best-effort: a missing window or a show/focus failure is logged, never a panic.
pub fn show_main_window(app: &AppHandle) {
    let Some(window) = app.get_webview_window(MAIN_WINDOW_LABEL) else {
        tracing::warn!("tray: main window not found; cannot show");
        return;
    };
    if window.is_minimized().unwrap_or(false) {
        if let Err(error) = window.unminimize() {
            tracing::warn!(%error, "tray: could not unminimize main window");
        }
    }
    if let Err(error) = window.show() {
        tracing::warn!(%error, "tray: could not show main window");
    }
    if let Err(error) = window.set_focus() {
        tracing::warn!(%error, "tray: could not focus main window");
    }
}

/// Build and install the tray icon with a "Show keeper" + "Quit" menu (Story 10.3).
/// Reuses the app's default window icon so no extra asset is bundled. Best-effort — on a
/// menu/tray build failure it logs at `warn` and leaves no tray (the app keeps running).
fn build_tray(app: &AppHandle) -> Option<TrayIcon> {
    let show = match MenuItemBuilder::with_id(SHOW_ID, "Show keeper").build(app) {
        Ok(item) => item,
        Err(error) => {
            tracing::warn!(%error, "tray: could not build 'Show keeper' item");
            return None;
        }
    };
    let quit = match MenuItemBuilder::with_id(QUIT_ID, "Quit").build(app) {
        Ok(item) => item,
        Err(error) => {
            tracing::warn!(%error, "tray: could not build 'Quit' item");
            return None;
        }
    };
    let menu = match MenuBuilder::new(app).items(&[&show, &quit]).build() {
        Ok(menu) => menu,
        Err(error) => {
            tracing::warn!(%error, "tray: could not build tray menu");
            return None;
        }
    };
    let mut builder = TrayIconBuilder::new()
        .menu(&menu)
        .show_menu_on_left_click(true)
        .on_menu_event(|app, event| match event.id().as_ref() {
            SHOW_ID => show_main_window(app),
            QUIT_ID => app.exit(0),
            _ => {}
        });
    // Reuse the bundled default window icon so the tray needs no separate asset.
    if let Some(icon) = app.default_window_icon().cloned() {
        builder = builder.icon(icon);
    }
    match builder.build(app) {
        Ok(tray) => Some(tray),
        Err(error) => {
            tracing::warn!(%error, "tray: could not build tray icon");
            None
        }
    }
}

/// Create or destroy the tray icon to match `enabled` (Story 10.3). Idempotent: enabling
/// when a tray already exists rebuilds it; disabling when none exists is a no-op. Called
/// from the Settings command and at startup for the persisted setting.
pub fn set_tray_presence(app: &AppHandle, enabled: bool) {
    let slot = tray_slot();
    let mut guard = match slot.lock() {
        Ok(guard) => guard,
        Err(poisoned) => poisoned.into_inner(),
    };
    if enabled {
        // Replace any existing tray so a re-enable never leaks a second icon.
        *guard = build_tray(app);
    } else {
        // Dropping the handle removes the tray icon.
        *guard = None;
    }
}
