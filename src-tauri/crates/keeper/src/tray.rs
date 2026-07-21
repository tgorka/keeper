//! Opt-in menu-bar (tray) presence (Story 10.3, FR-53) + the live recording
//! state (Story 18.1, AD-33..39).
//!
//! When enabled, keeper shows a macOS menu-bar item that keeps the app reachable while
//! the window is hidden: "Show keeper" raises + focuses the main window, "Quit" exits the
//! process. The presence is opt-in (off by default) and persisted in `keeper.db`; the
//! shell creates or destroys the tray icon live off the Settings toggle and rebuilds it at
//! startup when the persisted setting is on.
//!
//! While a screen-recording session is live (Story 18.1), the same single tray
//! flips to a `recording` rendering: a record-dot icon, a ~1 Hz-refreshed
//! disabled status line (`Recording — 12:34 · segment 3, 412 MB`), and **Stop
//! Recording** / **Open Recordings Folder** items ahead of Show/Quit. The tray
//! is a pure renderer of the Rust-owned [`RecordingStatusVm`] snapshot (driven
//! by the tick in `lib.rs`); on any terminal or idle state it restores the idle
//! icon + menu. macOS's own purple recording pill is system-owned and never
//! touched — the tray only adds what the pill lacks.
//!
//! Forced presence (Story 18.2): an invisible live recording is a bug, so when
//! the opt-in toggle is off (empty slot) and a session is live, the same tick
//! force-builds the tray and remembers that *it* created it
//! ([`FORCED_PRESENCE`]); on the terminal tick the forced tray is dropped,
//! restoring the exact prior configuration. The persisted
//! `system.menu_bar_presence` setting is never written — it stays the source
//! of truth for "prior config".
//!
//! Tray glue is a shell/OS concern (AD-24) — `keeper-core` only owns the persisted
//! *mode/flag*. Every step here is best-effort: a tray build failure is logged at `warn`
//! and the app keeps running (the tray is a convenience, never load-bearing).

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, MutexGuard, OnceLock};

use keeper_core::vm::{RecordingStatusVm, RecordingUiState};
use tauri::image::Image;
use tauri::menu::{Menu, MenuBuilder, MenuItem, MenuItemBuilder};
use tauri::tray::{TrayIcon, TrayIconBuilder, TrayIconId};
use tauri::{AppHandle, Manager, Wry};

/// The label of the main window (matches `tauri.conf.json` / the default capability).
const MAIN_WINDOW_LABEL: &str = "main";

/// The menu item id for "Show keeper".
const SHOW_ID: &str = "tray-show";
/// The menu item id for "Quit".
const QUIT_ID: &str = "tray-quit";
/// The menu item id for "Stop Recording" (Story 18.1).
const STOP_ID: &str = "tray-stop-recording";
/// The menu item id for "Open Recordings Folder" (Story 18.1).
const OPEN_FOLDER_ID: &str = "tray-open-recordings-folder";
/// The menu item id for the disabled elapsed/segment/size line (Story 18.1).
const STATUS_ID: &str = "tray-recording-status";
/// The menu item id for "Show Recording" in the error-hold menu (Story 18.4):
/// raises + focuses the main window, where the Recording view's banner carries
/// the one-click Restart (the tray never restarts a session itself).
const SHOW_RECORDING_ID: &str = "tray-show-recording";
/// The menu item id for "Dismiss Error" in the error-hold menu (Story 18.4):
/// acknowledges the terminal failed session back to idle, releasing the hold.
const DISMISS_ERROR_ID: &str = "tray-dismiss-error";

/// The bundled record-dot menu-bar icon shown while a recording is live (Story
/// 18.1; template rendering Story 21.4). Decoded per transition via
/// [`Image::from_bytes`] (the `image-png` tauri feature) — a decode failure
/// just keeps the current icon.
///
/// All three tray glyphs are macOS TEMPLATE images (monochrome black + alpha):
/// the menu bar colors them white/black per appearance and highlights them
/// natively, so states must read from glyph SHAPE — ring = idle, filled dot in
/// a ring = recording, disc with a punched-out exclamation = error.
const RECORDING_ICON_PNG: &[u8] = include_bytes!("../icons/tray-recording-template.png");

/// The bundled recording-red **filled** error badge shown while a failed session
/// holds the tray (Story 18.4): the same recording-red as the record dot but a
/// filled circle carrying a white exclamation mark, so the fault reads at a
/// glance and stays visually distinct from the plain record dot. Same base
/// dimensions as [`RECORDING_ICON_PNG`]; decoded per transition via
/// [`Image::from_bytes`] — a decode failure just keeps the current icon.
const ERROR_ICON_PNG: &[u8] = include_bytes!("../icons/tray-error-template.png");

/// The idle (presence-only) template glyph — an outline ring. Replaces the
/// colored app icon so the menu bar stays native (Story 21.4).
const IDLE_ICON_PNG: &[u8] = include_bytes!("../icons/tray-idle-template.png");

/// The live tray, if menu-bar presence is currently on. `None` means no tray is
/// shown (the default). Guarded by a `Mutex` since the Settings toggle command,
/// startup, and the ~1 Hz recording tick all touch it.
struct TrayState {
    /// The tray icon handle; dropping it removes the menu-bar item.
    icon: TrayIcon,
    /// The disabled elapsed/segment/size line while the recording menu is
    /// installed (`None` in the idle Show/Quit menu — this doubles as the
    /// rendered-mode flag). Held so each tick refreshes the text via
    /// `set_text` — no menu rebuild, no flicker, an open menu stays open; the
    /// whole menu is swapped only on an idle↔recording transition.
    status_item: Option<MenuItem<Wry>>,
    /// Whether the Story 18.4 error-hold rendering (error badge + failed-reason
    /// menu) is currently installed. Mutually exclusive with `status_item`; the
    /// error line is static (the terminal `error` never changes), so the tick
    /// only needs this flag to avoid rebuilding the menu every second.
    error_rendered: bool,
}

/// The single tray slot (see [`TrayState`]).
static TRAY: OnceLock<Mutex<Option<TrayState>>> = OnceLock::new();

/// Whether the current tray was created by [`apply_recording_state`] forcing
/// presence for a live recording (Story 18.2) rather than by the user's FR-53
/// opt-in toggle. A module `static` (not a [`TrayState`] field) so it survives
/// the slot's build/drop cycle. Cleared by any explicit [`set_tray_presence`]
/// call — the user then owns the tray's presence again; if a recording is
/// still live, the next ~1 Hz tick simply re-forces it.
static FORCED_PRESENCE: AtomicBool = AtomicBool::new(false);

fn tray_slot() -> &'static Mutex<Option<TrayState>> {
    TRAY.get_or_init(|| Mutex::new(None))
}

/// Lock the tray slot, recovering a poisoned lock — the slot holds plain
/// handles with no invariant a mid-write panic could break, and a tray concern
/// must never panic the app.
fn tray_guard() -> MutexGuard<'static, Option<TrayState>> {
    match tray_slot().lock() {
        Ok(guard) => guard,
        Err(poisoned) => poisoned.into_inner(),
    }
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

/// Fire the graceful recording stop from the tray (Story 18.1): the identical
/// idempotent one-shot trigger the `recording_stop` command fires — the current
/// segment finalizes and the session reaches its terminal state.
fn stop_recording(app: &AppHandle) {
    let state = app.state::<crate::ipc::AppState>();
    crate::ipc::stop_active_recording(&state);
}

/// Dismiss the held recording error from the tray (Story 18.4): the identical
/// acknowledge the `recording_acknowledge` command runs — a terminal failed
/// session clears back to idle (a live session is a strict no-op), and the next
/// ~1 Hz tick then restores/drops the tray per the 18.2 lifecycle.
fn dismiss_recording_error(app: &AppHandle) {
    let state = app.state::<crate::ipc::AppState>();
    let _ = crate::ipc::acknowledge_recording(&state);
}

/// Reveal the current session folder (`output_path`) in the OS file manager
/// (Story 18.1), via the same opener plugin as the export "Reveal in Finder".
/// Best-effort: no session folder or a reveal failure is logged, never a panic.
fn open_recordings_folder(app: &AppHandle) {
    let state = app.state::<crate::ipc::AppState>();
    let snapshot = crate::ipc::recording_snapshot(&state);
    let Some(path) = snapshot.output_path else {
        tracing::warn!("tray: no recording session folder to open");
        return;
    };
    if let Err(error) = tauri_plugin_opener::reveal_item_in_dir(&path) {
        tracing::warn!(%error, "tray: could not reveal the recordings folder");
    }
}

/// Build one tray menu item, logging the failed label and returning `None` on a
/// build failure (the caller then leaves the tray/menu unchanged).
fn menu_item(app: &AppHandle, id: &str, label: &str, enabled: bool) -> Option<MenuItem<Wry>> {
    match MenuItemBuilder::with_id(id, label)
        .enabled(enabled)
        .build(app)
    {
        Ok(item) => Some(item),
        Err(error) => {
            tracing::warn!(%error, label, "tray: could not build menu item");
            None
        }
    }
}

/// Build the idle tray menu: "Show keeper" + "Quit" (Story 10.3).
fn build_idle_menu(app: &AppHandle) -> Option<Menu<Wry>> {
    let show = menu_item(app, SHOW_ID, "Show keeper", true)?;
    let quit = menu_item(app, QUIT_ID, "Quit", true)?;
    match MenuBuilder::new(app).items(&[&show, &quit]).build() {
        Ok(menu) => Some(menu),
        Err(error) => {
            tracing::warn!(%error, "tray: could not build tray menu");
            None
        }
    }
}

/// Build the error-hold tray menu (Story 18.4): the disabled
/// `Recording failed — <reason>` line, then **Show Recording** (raises the
/// window, where the banner's one-click Restart lives), **Open Recordings
/// Folder**, and **Dismiss Error** (→ acknowledge), then Quit. The line is
/// static — a terminal `error` never changes — so no held item is returned.
fn build_error_menu(app: &AppHandle, line: &str) -> Option<Menu<Wry>> {
    let status = menu_item(app, STATUS_ID, line, false)?;
    let show_recording = menu_item(app, SHOW_RECORDING_ID, "Show Recording", true)?;
    let open = menu_item(app, OPEN_FOLDER_ID, "Open Recordings Folder", true)?;
    let dismiss = menu_item(app, DISMISS_ERROR_ID, "Dismiss Error", true)?;
    let quit = menu_item(app, QUIT_ID, "Quit", true)?;
    let menu = MenuBuilder::new(app)
        .item(&status)
        .separator()
        .items(&[&show_recording, &open, &dismiss])
        .separator()
        .items(&[&quit])
        .build();
    match menu {
        Ok(menu) => Some(menu),
        Err(error) => {
            tracing::warn!(%error, "tray: could not build error tray menu");
            None
        }
    }
}

/// Build the recording tray menu (Story 18.1): the disabled status `line`, then
/// Stop Recording + Open Recordings Folder, then the idle Show/Quit pair.
/// Returns the menu together with the held status item so the ~1 Hz tick can
/// refresh the line via `set_text`.
fn build_recording_menu(app: &AppHandle, line: &str) -> Option<(Menu<Wry>, MenuItem<Wry>)> {
    let status = menu_item(app, STATUS_ID, line, false)?;
    let stop = menu_item(app, STOP_ID, "Stop Recording", true)?;
    let open = menu_item(app, OPEN_FOLDER_ID, "Open Recordings Folder", true)?;
    let show = menu_item(app, SHOW_ID, "Show keeper", true)?;
    let quit = menu_item(app, QUIT_ID, "Quit", true)?;
    let menu = MenuBuilder::new(app)
        .item(&status)
        .separator()
        .items(&[&stop, &open])
        .separator()
        .items(&[&show, &quit])
        .build();
    match menu {
        Ok(menu) => Some((menu, status)),
        Err(error) => {
            tracing::warn!(%error, "tray: could not build recording tray menu");
            None
        }
    }
}

/// Build and install the tray icon with the idle "Show keeper" + "Quit" menu
/// (Story 10.3). Reuses the app's default window icon so the idle state needs no
/// extra asset. The single `on_menu_event` handler covers the recording items
/// too (Story 18.1) — it is registered on the tray, not a menu, so it survives
/// every `set_menu` swap. Best-effort — on a menu/tray build failure it logs at
/// `warn` and leaves no tray (the app keeps running).
fn build_tray(app: &AppHandle) -> Option<TrayIcon> {
    let menu = build_idle_menu(app)?;
    let mut builder = TrayIconBuilder::new()
        .menu(&menu)
        .show_menu_on_left_click(true)
        .on_menu_event(|app, event| match event.id().as_ref() {
            // Story 18.4: "Show Recording" raises the same main window (the
            // Recording view's banner carries the one-click Restart there).
            SHOW_ID | SHOW_RECORDING_ID => show_main_window(app),
            QUIT_ID => app.exit(0),
            STOP_ID => stop_recording(app),
            OPEN_FOLDER_ID => open_recordings_folder(app),
            DISMISS_ERROR_ID => dismiss_recording_error(app),
            _ => {}
        });
    // The idle template ring (Story 21.4): monochrome + alpha, flagged as a
    // template so macOS renders it white/black per menu-bar appearance like
    // every native icon. A decode failure falls back to the app icon.
    match Image::from_bytes(IDLE_ICON_PNG) {
        Ok(icon) => {
            builder = builder.icon(icon).icon_as_template(true);
        }
        Err(_) => {
            if let Some(icon) = app.default_window_icon().cloned() {
                builder = builder.icon(icon);
            }
        }
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
/// from the Settings command and at startup for the persisted setting. A rebuild lands in
/// the idle rendering; a live recording re-applies its state on the next ~1 Hz tick.
pub fn set_tray_presence(app: &AppHandle, enabled: bool) {
    // Any explicit presence call supersedes a recording-forced tray (Story
    // 18.2): the user (or the startup replay of the persisted setting) now
    // owns presence. Toggling off mid-recording leaves an invisible live
    // recording for at most ~1 s — the next tick re-forces the tray.
    FORCED_PRESENCE.store(false, Ordering::Relaxed);
    let tray = if enabled { build_tray(app) } else { None };
    let mut guard = tray_guard();
    if enabled {
        // Replace any existing tray so a re-enable never leaks a second icon.
        *guard = tray.map(|icon| TrayState {
            icon,
            status_item: None,
            error_rendered: false,
        });
    } else {
        // Dropping the handle removes the tray icon.
        *guard = None;
    }
}

/// What this tick does to the tray slot — the pure decision returned by
/// [`decide_presence`] (Story 18.2); [`apply_recording_state`] performs the
/// matching GUI side effect best-effort.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PresenceAction {
    /// Live recording (or a failed+error hold) with no tray: force-build the
    /// tray (and mark it ours); the render then follows the snapshot.
    ForcePresent,
    /// Live recording, tray present: Story 18.1's recording render/refresh.
    RenderRecording,
    /// `Failed` with an `error`, tray present (Story 18.4): hold the tray in
    /// the error rendering — badge + reason line — instead of dropping it.
    RenderError,
    /// Terminal/idle and *we* forced the tray: drop it (restore prior config).
    DropTray,
    /// Terminal/idle, user-owned tray: Story 18.1's idle restore.
    RestoreIdle,
    /// Terminal/idle, no tray: nothing to do.
    Noop,
}

/// Decide this tick's presence action from the authoritative snapshot state
/// (+ whether it carries a fault `error`) plus the current slot/forced flags
/// (Story 18.2 + 18.4). Pure — the unit-testable seam between the recording
/// state machine and the GUI side effects. A live state with an absent slot
/// force-builds regardless of `forced` (a forced tray that vanished
/// mid-recording self-heals the same way); `Failed` **with an error** is the
/// Story 18.4 error-hold: the tray must NOT drop the instant the fault happens
/// — it renders the error badge + reason (force-building if somehow absent)
/// until the fault clears (acknowledge → idle, or a restart back to live), at
/// which point the ordinary 18.2 restore/drop below runs. Every other
/// terminal/idle state drops the slot only when *we* created it.
fn decide_presence(
    state: RecordingUiState,
    has_error: bool,
    present: bool,
    forced: bool,
) -> PresenceAction {
    if state.is_live() {
        if present {
            PresenceAction::RenderRecording
        } else {
            PresenceAction::ForcePresent
        }
    } else if state == RecordingUiState::Failed && has_error {
        // Story 18.4: hold-in-error — never drop/restore while failed+error.
        if present {
            PresenceAction::RenderError
        } else {
            PresenceAction::ForcePresent
        }
    } else if !present {
        // `state.is_terminal()` from here down (the exact complement).
        PresenceAction::Noop
    } else if forced {
        PresenceAction::DropTray
    } else {
        PresenceAction::RestoreIdle
    }
}

/// Render the tray from the authoritative recording snapshot (Story 18.1 +
/// 18.2) — the ~1 Hz tick in `lib.rs` calls this every second. The pure
/// [`decide_presence`] picks this tick's action: a live session with an empty
/// slot (opt-in toggle off) force-builds the tray (Story 18.2, marked via
/// [`FORCED_PRESENCE`]); a terminal/idle state drops a forced tray (restoring
/// the prior configuration) or restores the user-owned tray to idle (18.1).
/// On a live state with a present tray the icon/menu swap fires only on the
/// idle→recording transition while the disabled line refreshes every tick.
/// Best-effort throughout — a force-build failure just retries next tick.
///
/// Lock discipline (deadlock-critical): every `TrayIcon`/`MenuItem` call below
/// blocks on an internal main-thread dispatch, and [`set_tray_presence`] — which
/// can run *on* the main thread (startup, a sync command) — takes the same tray
/// lock. So the guard is never held across those calls: handles are cloned out,
/// mutations (including the drop of a forced tray) run lock-free, and the new
/// status item is stored back under a fresh lock, checked against the tray id
/// in case the slot was rebuilt meanwhile.
pub fn apply_recording_state(app: &AppHandle, snapshot: &RecordingStatusVm) {
    let (tray, status_item, error_rendered) = {
        let guard = tray_guard();
        match guard.as_ref() {
            Some(state) => (
                Some(state.icon.clone()),
                state.status_item.clone(),
                state.error_rendered,
            ),
            None => (None, None, false),
        }
    };
    let forced = FORCED_PRESENCE.load(Ordering::Relaxed);
    let has_error = snapshot.error.is_some();
    match decide_presence(snapshot.state, has_error, tray.is_some(), forced) {
        PresenceAction::ForcePresent => force_present(app, snapshot),
        PresenceAction::RenderRecording => {
            if let Some(tray) = tray {
                render_recording(app, &tray, status_item, snapshot);
            }
        }
        PresenceAction::RenderError => {
            if let Some(tray) = tray {
                render_error(app, &tray, error_rendered, snapshot);
            }
        }
        PresenceAction::DropTray => {
            // Re-check ownership under the tray lock before taking (review
            // patch): `decide_presence` saw `forced` from a snapshot taken
            // earlier, and this tick runs on a worker thread while
            // `set_tray_presence` runs on the main thread. A concurrent
            // `set_tray_presence(true)` (Settings toggle / startup replay)
            // clears `FORCED_PRESENCE` and installs a *user-owned* tray in the
            // window between that snapshot and here; taking unconditionally
            // would drop the user's freshly-installed opt-in tray (and, being
            // terminal, the next tick's `Noop` would never rebuild it). The
            // `set_tray_presence` unlock happens-before this lock, so its
            // `FORCED_PRESENCE = false` store is visible: only take the tray we
            // still own. Drop it OUTSIDE the lock — removing the icon dispatches
            // to the main thread (per the lock discipline above).
            let dropped = {
                let mut guard = tray_guard();
                if FORCED_PRESENCE.swap(false, Ordering::Relaxed) {
                    guard.take()
                } else {
                    None
                }
            };
            drop(dropped);
        }
        PresenceAction::RestoreIdle => {
            // Only an installed recording or error-hold menu needs restoring —
            // an idle tray in a terminal state is already in its prior
            // configuration.
            if let (Some(tray), true) = (tray, status_item.is_some() || error_rendered) {
                restore_idle(app, &tray);
            }
        }
        PresenceAction::Noop => {}
    }
}

/// Force the tray into existence for a live recording (Story 18.2): the FR-53
/// opt-in toggle is off (empty slot) but an invisible live recording is a bug,
/// so build the tray, remember that *we* created it, and render this tick's
/// recording state onto it. The persisted `system.menu_bar_presence` setting
/// is never written — dropping the forced tray on the terminal tick restores
/// the exact prior configuration. Best-effort: a build failure warns (inside
/// [`build_tray`]) and the next ~1 Hz tick retries.
fn force_present(app: &AppHandle, snapshot: &RecordingStatusVm) {
    let Some(icon) = build_tray(app) else {
        return;
    };
    let stored = {
        let mut guard = tray_guard();
        if guard.is_none() {
            *guard = Some(TrayState {
                icon: icon.clone(),
                status_item: None,
                error_rendered: false,
            });
            FORCED_PRESENCE.store(true, Ordering::Relaxed);
            true
        } else {
            false
        }
    };
    if !stored {
        // A tray appeared concurrently (Settings toggle / startup replay) —
        // the user's tray wins; our fresh icon drops here, removing the
        // transient duplicate menu-bar item (outside the lock).
        return;
    }
    // Render this tick's state onto the fresh tray: the error-hold rendering
    // for a failed+error snapshot (Story 18.4 — e.g. the tray vanished in the
    // same instant the fault landed), else the recording rendering (18.1).
    if snapshot.state == RecordingUiState::Failed && snapshot.error.is_some() {
        render_error(app, &icon, false, snapshot);
    } else {
        render_recording(app, &icon, None, snapshot);
    }
}

/// Render the recording state onto a present tray (Story 18.1): on the
/// idle→recording transition swap in the record-dot icon + recording menu; on
/// every later tick refresh only the disabled status line via `set_text` — no
/// menu rebuild, no flicker, an open menu stays open.
fn render_recording(
    app: &AppHandle,
    tray: &TrayIcon,
    status_item: Option<MenuItem<Wry>>,
    snapshot: &RecordingStatusVm,
) {
    let line = status_line(snapshot);
    if let Some(item) = status_item {
        // Already in the recording rendering — refresh only the line.
        if let Err(error) = item.set_text(&line) {
            tracing::warn!(%error, "tray: could not refresh the recording status line");
        }
        return;
    }
    // idle → recording transition: record-dot icon + recording menu.
    match Image::from_bytes(RECORDING_ICON_PNG) {
        Ok(icon) => {
            // macOS `isTemplate` is a property of the IMAGE — re-assert it on
            // every swap or the glyph falls back to plain-alpha rendering.
            if let Err(error) = tray.set_icon(Some(icon)) {
                tracing::warn!(%error, "tray: could not set the record-dot icon");
            }
            let _ = tray.set_icon_as_template(true);
        }
        Err(error) => tracing::warn!(%error, "tray: could not decode the record-dot icon"),
    }
    // A menu build/install failure leaves `status_item` unset, so the next
    // tick simply retries the transition.
    let Some((menu, status)) = build_recording_menu(app, &line) else {
        return;
    };
    if let Err(error) = tray.set_menu(Some(menu)) {
        tracing::warn!(%error, "tray: could not install the recording menu");
        return;
    }
    store_rendered_mode(tray.id(), Some(status), false);
}

/// Hold a present tray in the Story 18.4 error rendering: on the transition
/// (any prior rendering → error) swap in the filled recording-red error badge
/// and the error menu — the disabled `Recording failed — <reason>` line plus
/// Show Recording / Open Recordings Folder / Dismiss Error. Later ticks with
/// the rendering already installed do nothing (the terminal reason is static).
/// Best-effort like every tray mutation: a badge decode/set failure keeps the
/// current icon; a menu build/install failure leaves the flag unset so the
/// next ~1 Hz tick retries the transition.
fn render_error(
    app: &AppHandle,
    tray: &TrayIcon,
    error_rendered: bool,
    snapshot: &RecordingStatusVm,
) {
    if error_rendered {
        // Already holding in error — nothing changes until acknowledge/restart.
        return;
    }
    let reason = snapshot.error.as_deref().unwrap_or("unknown error");
    match Image::from_bytes(ERROR_ICON_PNG) {
        Ok(icon) => {
            // macOS `isTemplate` is a property of the IMAGE — re-assert it on
            // every swap or the glyph falls back to plain-alpha rendering.
            if let Err(error) = tray.set_icon(Some(icon)) {
                tracing::warn!(%error, "tray: could not set the error badge icon");
            }
            let _ = tray.set_icon_as_template(true);
        }
        Err(error) => tracing::warn!(%error, "tray: could not decode the error badge icon"),
    }
    let Some(menu) = build_error_menu(app, &format_error_line(reason)) else {
        return;
    };
    if let Err(error) = tray.set_menu(Some(menu)) {
        tracing::warn!(%error, "tray: could not install the error tray menu");
        return;
    }
    store_rendered_mode(tray.id(), None, true);
}

/// Restore a user-owned tray to the idle rendering after a recording ended
/// (Story 18.1): restore the idle menu FIRST, and only clear the rendered-mode
/// flag once it is actually installed. A menu build/install failure returns
/// with the flag still set and the recording menu still on screen, so the next
/// tick retries the teardown — never clears the flag while the Stop/Open menu
/// is still installed (which would strand it for the app lifetime, since a
/// terminal state with no `status_item` re-enters neither branch).
fn restore_idle(app: &AppHandle, tray: &TrayIcon) {
    let Some(menu) = build_idle_menu(app) else {
        return;
    };
    if let Err(error) = tray.set_menu(Some(menu)) {
        tracing::warn!(%error, "tray: could not restore the idle menu");
        return;
    }
    // The icon is cosmetic — a failure here does not desync the flag/menu.
    let idle = Image::from_bytes(IDLE_ICON_PNG).ok();
    if let Err(error) = tray.set_icon(idle) {
        tracing::warn!(%error, "tray: could not restore the idle icon");
    }
    let _ = tray.set_icon_as_template(true);
    store_rendered_mode(tray.id(), None, false);
}

/// Store the rendered-mode flags — the recording status-line item and the
/// error-hold flag (mutually exclusive; both cleared on an idle restore) —
/// back into the tray slot, unless the slot was rebuilt (toggle off→on) while
/// the menu was installed lock-free, in which case the fresh tray's own state
/// wins.
fn store_rendered_mode(tray_id: &TrayIconId, item: Option<MenuItem<Wry>>, error_rendered: bool) {
    let mut guard = tray_guard();
    if let Some(state) = guard.as_mut() {
        if state.icon.id() == tray_id {
            state.status_item = item;
            state.error_rendered = error_rendered;
        }
    }
}

/// Compose this tick's status line from the authoritative snapshot (Story 18.1):
/// elapsed from the host-reported start instant (clamped to 0 against clock
/// skew), segment index = closed + 1, size summed live from the session folder
/// on disk (the same file-ownership rule as the terminal manifest reconcile).
/// Pre-capture — `preflight`, or no start instant yet — honestly reads
/// "Starting…", never a panic. A sticky session warning (Story 19.4 — e.g. a
/// mic hot-unplug) marks the line via [`format_warning_line`]: the session is
/// still live (presence/`is_live` untouched, same recording icon), so the
/// normal recording line stays and the warning rides on it.
fn status_line(snapshot: &RecordingStatusVm) -> String {
    let started_ms = match snapshot.state {
        RecordingUiState::Recording | RecordingUiState::Rotating | RecordingUiState::Stopping => {
            snapshot.started_at_epoch_ms
        }
        _ => None,
    };
    let base = match started_ms {
        Some(started_ms) => {
            let elapsed_secs = crate::ipc::epoch_ms_now().saturating_sub(started_ms) / 1000;
            // The size figure now rides the shared enriched snapshot (Story 18.3):
            // `recording_snapshot` sums the session's segments once, so the tray and the
            // in-app banner render the identical byte figure — no second on-disk read.
            format_status_line(
                elapsed_secs,
                snapshot.segments_closed,
                snapshot.on_disk_bytes,
            )
        }
        None => "Starting…".to_owned(),
    };
    match snapshot.warning.as_deref() {
        Some(message) => format_warning_line(&base, message),
        None => base,
    }
}

/// Compose the disabled error-hold status line (Story 18.4), naming the honest
/// terminal reason: `"keeper-rec exited"` → `Recording failed — keeper-rec
/// exited`. Pure.
fn format_error_line(reason: &str) -> String {
    format!("Recording failed — {reason}")
}

/// Mark a status line with the sticky, non-dismissible session warning (Story
/// 19.4): the recording line stays first (the session IS still live), the
/// warning message rides behind it under a leading `⚠` marker. Pure:
/// `("Recording — 12:34 · segment 3, 412 MB", "mic lost")` →
/// `⚠ Recording — 12:34 · segment 3, 412 MB — mic lost`.
fn format_warning_line(base: &str, message: &str) -> String {
    format!("⚠ {base} — {message}")
}

/// Format an elapsed duration as `mm:ss` below one hour (minutes unpadded,
/// seconds zero-padded) and `h:mm:ss` from one hour up: 754 → `12:34`,
/// 3723 → `1:02:03`. Pure.
fn format_elapsed(total_secs: u64) -> String {
    let hours = total_secs / 3600;
    let minutes = (total_secs % 3600) / 60;
    let seconds = total_secs % 60;
    if hours > 0 {
        format!("{hours}:{minutes:02}:{seconds:02}")
    } else {
        format!("{minutes}:{seconds:02}")
    }
}

/// Format a byte count in whole decimal MB (`10^6`, matching the `segment_mb`
/// convention), rolling to one-decimal GB at ≥ 1000 MB: 412_800_000 → `412 MB`,
/// 1_290_000_000 → `1.2 GB`. Truncates (never rounds up), so the figure never
/// overstates what has reached disk. Pure, integer-only.
fn format_size(bytes: u64) -> String {
    let mb = bytes / 1_000_000;
    if mb >= 1000 {
        // Whole tenths of a GB: 1_290_000_000 → 12 tenths → "1.2 GB".
        let tenths = bytes / 100_000_000;
        format!("{}.{} GB", tenths / 10, tenths % 10)
    } else {
        format!("{mb} MB")
    }
}

/// Compose the disabled tray status line (Story 18.1): the shown segment index
/// is `segments_closed + 1` (the segment currently being written). Pure:
/// `(754, 2, 412_000_000)` → `Recording — 12:34 · segment 3, 412 MB`.
fn format_status_line(elapsed_secs: u64, segments_closed: u32, bytes: u64) -> String {
    format!(
        "Recording — {} · segment {}, {}",
        format_elapsed(elapsed_secs),
        segments_closed.saturating_add(1),
        format_size(bytes)
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Story 18.2: the `decide_presence` I/O matrix over every state × slot ×
    /// forced combination — live+absent force-builds (even with a stale forced
    /// flag: self-healing), live+present renders (18.1), terminal+present drops
    /// only a forced tray (else the 18.1 idle restore), terminal+absent no-ops.
    /// Story 18.4 carves exactly one exception out of the terminal rows:
    /// `Failed` **with an error** holds (see the dedicated test below); here
    /// every terminal row runs with `has_error = false`, and a live state with
    /// a (stale/irrelevant) error still renders live.
    #[test]
    fn decide_presence_covers_the_io_matrix() {
        use PresenceAction::*;
        use RecordingUiState::*;
        for state in [Preflight, Recording, Rotating, Stopping] {
            for has_error in [false, true] {
                assert_eq!(
                    decide_presence(state, has_error, false, false),
                    ForcePresent,
                    "{state:?} err={has_error}"
                );
                assert_eq!(
                    decide_presence(state, has_error, false, true),
                    ForcePresent,
                    "{state:?} err={has_error}"
                );
                assert_eq!(
                    decide_presence(state, has_error, true, false),
                    RenderRecording,
                    "{state:?} err={has_error}"
                );
                assert_eq!(
                    decide_presence(state, has_error, true, true),
                    RenderRecording,
                    "{state:?} err={has_error}"
                );
            }
        }
        for state in [Idle, Finalized, Recovered, Failed] {
            assert_eq!(
                decide_presence(state, false, true, true),
                DropTray,
                "{state:?}"
            );
            assert_eq!(
                decide_presence(state, false, true, false),
                RestoreIdle,
                "{state:?}"
            );
            assert_eq!(
                decide_presence(state, false, false, false),
                Noop,
                "{state:?}"
            );
            assert_eq!(
                decide_presence(state, false, false, true),
                Noop,
                "{state:?}"
            );
        }
    }

    /// Story 18.4: `Failed` + `error` is the hold-in-error state — the tray
    /// NEVER drops or restores while the fault is unacknowledged, regardless
    /// of who created the tray (forced or user-owned); an absent slot even
    /// force-builds so the fault cannot be invisible.
    #[test]
    fn decide_presence_holds_the_tray_on_failed_with_error() {
        use PresenceAction::*;
        use RecordingUiState::Failed;
        assert_eq!(decide_presence(Failed, true, true, false), RenderError);
        assert_eq!(decide_presence(Failed, true, true, true), RenderError);
        assert_eq!(decide_presence(Failed, true, false, false), ForcePresent);
        assert_eq!(decide_presence(Failed, true, false, true), ForcePresent);
    }

    /// Story 18.4: the non-failed terminals never hold, even with a (stale)
    /// error on the snapshot — only `Failed`+`error` is the hold state, so
    /// Finalized/Recovered/Idle restore/drop exactly as before (18.1/18.2).
    #[test]
    fn decide_presence_non_failed_terminals_never_hold_even_with_error() {
        use PresenceAction::*;
        use RecordingUiState::*;
        for state in [Idle, Finalized, Recovered] {
            assert_eq!(
                decide_presence(state, true, true, true),
                DropTray,
                "{state:?}"
            );
            assert_eq!(
                decide_presence(state, true, true, false),
                RestoreIdle,
                "{state:?}"
            );
            assert_eq!(
                decide_presence(state, true, false, false),
                Noop,
                "{state:?}"
            );
        }
        // Failed WITHOUT an error (defensive: no reason to hold on) follows the
        // ordinary terminal path too.
        assert_eq!(decide_presence(Failed, false, true, true), DropTray);
        assert_eq!(decide_presence(Failed, false, true, false), RestoreIdle);
    }

    /// Story 18.4: the bundled error badge is a valid PNG that decodes through
    /// the same `Image::from_bytes` path the tick uses, at the record-dot's
    /// base dimensions — and it is not byte-identical to the record dot (the
    /// two states must be distinguishable at a glance).
    #[test]
    fn error_badge_asset_decodes_at_the_record_dot_dimensions() {
        let error = Image::from_bytes(ERROR_ICON_PNG).expect("tray-error.png decodes");
        let recording = Image::from_bytes(RECORDING_ICON_PNG).expect("tray-recording.png decodes");
        assert_eq!(error.width(), recording.width());
        assert_eq!(error.height(), recording.height());
        assert_ne!(ERROR_ICON_PNG, RECORDING_ICON_PNG);
    }

    #[test]
    fn format_error_line_names_the_reason() {
        assert_eq!(
            format_error_line("keeper-rec exited unexpectedly"),
            "Recording failed — keeper-rec exited unexpectedly"
        );
    }

    #[test]
    fn format_elapsed_pads_seconds_and_rolls_to_hours() {
        assert_eq!(format_elapsed(0), "0:00");
        assert_eq!(format_elapsed(59), "0:59");
        assert_eq!(format_elapsed(754), "12:34");
        assert_eq!(format_elapsed(3600), "1:00:00");
        assert_eq!(format_elapsed(3723), "1:02:03");
    }

    #[test]
    fn format_size_truncates_decimal_mb_and_rolls_to_gb_at_1000() {
        assert_eq!(format_size(0), "0 MB");
        assert_eq!(format_size(412_800_000), "412 MB");
        assert_eq!(format_size(999_999_999), "999 MB");
        assert_eq!(format_size(1_000_000_000), "1.0 GB");
        assert_eq!(format_size(1_290_000_000), "1.2 GB");
    }

    #[test]
    fn format_status_line_composes_elapsed_segment_and_size() {
        // Live, 2 segments closed (⇒ writing segment 3), 754 s elapsed, 412 MB.
        assert_eq!(
            format_status_line(754, 2, 412_000_000),
            "Recording — 12:34 · segment 3, 412 MB"
        );
    }

    #[test]
    fn status_line_reads_starting_before_capture() {
        // Preflight, and a (theoretical) live state without a start instant,
        // both render the honest pre-capture line — never a panic.
        let mut snapshot = RecordingStatusVm::idle();
        snapshot.state = RecordingUiState::Preflight;
        assert_eq!(status_line(&snapshot), "Starting…");
        snapshot.state = RecordingUiState::Recording;
        assert_eq!(status_line(&snapshot), "Starting…");
    }

    #[test]
    fn format_warning_line_marks_the_line_and_keeps_the_recording_first() {
        assert_eq!(
            format_warning_line(
                "Recording — 12:34 · segment 3, 412 MB",
                "microphone disconnected — using system default input"
            ),
            "⚠ Recording — 12:34 · segment 3, 412 MB — \
             microphone disconnected — using system default input"
        );
    }

    #[test]
    fn status_line_marks_the_sticky_warning() {
        // Story 19.4: a warned snapshot renders the warning-marked line while
        // the presence/`is_live` semantics stay untouched (the state is still
        // live). The no-start-instant base keeps this deterministic.
        let mut snapshot = RecordingStatusVm::idle();
        snapshot.state = RecordingUiState::Recording;
        snapshot.warning = Some("microphone disconnected — no microphone input".to_owned());
        assert_eq!(
            status_line(&snapshot),
            "⚠ Starting… — microphone disconnected — no microphone input"
        );
        assert!(snapshot.state.is_live(), "mic loss is still live/present");
    }
}
