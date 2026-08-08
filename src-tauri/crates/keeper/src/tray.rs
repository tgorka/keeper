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
use std::sync::{Arc, Mutex, MutexGuard, OnceLock};

use keeper_core::vm::{
    RecordingDurabilityState, RecordingDurabilityVm, RecordingStatusVm, RecordingUiState,
};
use keeper_sync::progress::TraySyncState;
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

/// The menu item ids for the notes section (Story 36.7, FR-102, AD-61).
///
/// Every one of them exists in the menu built at tray creation and never in a
/// replacement menu: a Linux tray menu cannot be swapped after it is set, so an
/// affordance absent from the first menu can never appear. Labels mutate on the
/// retained handles in [`NotesItems`]; the slot count is fixed forever.
const NOTE_NEW_ID: &str = "tray-note-new";
const NOTE_CAPTURE_ID: &str = "tray-note-capture";
const NOTE_JOURNAL_ID: &str = "tray-note-journal";
const NOTE_UNREAD_ID: &str = "tray-note-unread";

/// The five recent-note slots, in order. Five exist from the first build whether
/// or not five notes do — fewer touched notes means disabled slots, not a shorter
/// menu.
const NOTE_RECENT_IDS: [&str; 5] = [
    "tray-note-recent-0",
    "tray-note-recent-1",
    "tray-note-recent-2",
    "tray-note-recent-3",
    "tray-note-recent-4",
];

/// How many recent slots the menu carries, for the composer that fills them.
pub const NOTE_RECENT_SLOTS: usize = NOTE_RECENT_IDS.len();

/// How many characters of a note title a slot label carries.
const NOTE_TITLE_CAP: usize = 40;

/// The label a recent slot with no note shows.
const NOTE_EMPTY_SLOT: &str = "\u{2014}";

/// What the tray should currently say about notes — composed in Rust, so the
/// tray, the palette and the window can never word the same fact differently.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct NotesTray {
    /// The active vault, when there is one. `None` means no folder is flagged,
    /// which is a legible state and not a disabled section: New Note and Today's
    /// Journal stay enabled and open Settings → Sync, because the action is
    /// achievable.
    pub vault_id: Option<String>,
    /// The five most recently touched notes, newest first: `(note id, title)`.
    pub recent: Vec<(String, String)>,
    /// How many notes in the active vault are unread (FR-113).
    pub unread: u32,
    /// Whether the global capture shortcut is actually registered. When it is
    /// not, the item's label says so — the truth belongs where the user is, not
    /// only where they configured it (UX-DR43).
    pub hotkey_registered: bool,
}

/// The retained notes menu items, held so their labels can be mutated.
///
/// `set_text` and `set_enabled` only — never `set_menu` (AD-61). Cloned handles
/// are cheap and the mutation dispatches to the main thread internally, which is
/// why the tray lock is never held across one.
#[derive(Clone)]
struct NotesItems {
    new_note: MenuItem<Wry>,
    capture: MenuItem<Wry>,
    journal: MenuItem<Wry>,
    recent: Vec<MenuItem<Wry>>,
    unread: MenuItem<Wry>,
}

/// The bundled record-dot menu-bar icon shown while a recording is live (Story
/// 18.1; template rendering Story 21.4). Decoded per transition via
/// [`tray_glyph`] — a decode failure just keeps the current icon.
///
/// All three tray glyphs are macOS TEMPLATE images (monochrome black + alpha,
/// the brand speech-bubble mark from the app icon): the menu bar colors them
/// white/black per appearance and highlights them natively, so states must
/// read from glyph SHAPE — bubble outline = idle, filled dot in the bubble =
/// recording, filled bubble with a punched-out exclamation = error. Off macOS
/// no tray host recolours anything, so [`tray_glyph`] repaints the same shape
/// for the panel; the shape-not-colour rule holds on every platform.
const RECORDING_ICON_PNG: &[u8] = include_bytes!("../icons/tray-recording-template.png");

/// The bundled error badge shown while a failed session holds the tray (Story
/// 18.4): the filled brand bubble carrying a punched-out exclamation mark, so
/// the fault reads at a glance and stays visually distinct from the plain
/// record dot. Same base dimensions as [`RECORDING_ICON_PNG`]; decoded per
/// transition via [`Image::from_bytes`] — a decode failure just keeps the
/// current icon.
const ERROR_ICON_PNG: &[u8] = include_bytes!("../icons/tray-error-template.png");

/// The idle (presence-only) template glyph — the brand speech-bubble outline.
/// Replaces the colored app icon so the menu bar stays native (Story 21.4).
const IDLE_ICON_PNG: &[u8] = include_bytes!("../icons/tray-idle-template.png");

/// The folder-sync glyph set (Story 29.2, AD-51), generated from the idle mark
/// by `scripts/gen-tray-sync-icons.ts` so the brand outline is identical and
/// only the interior differs.
///
/// Template images again: the menu bar recolours them, so **state must read
/// from SHAPE, never colour**. Armed is a static cycle in the bubble interior;
/// the four transfer states carry a mark in the bottom-right corner instead —
/// an arrow up, an arrow down, both, or circular arrows for work with nothing on
/// the wire.
///
/// These four replaced a four-frame rotating ring. The animation said "something
/// is happening" and nothing about *what*, so a 40 GB upload and a directory scan
/// drew the same picture; a direction says it without moving, which is also why
/// the tray no longer advances a frame counter.
const SYNC_ARMED_ICON_PNG: &[u8] = include_bytes!("../icons/tray-sync-template.png");
const SYNC_UP_ICON_PNG: &[u8] = include_bytes!("../icons/tray-sync-up-template.png");
const SYNC_DOWN_ICON_PNG: &[u8] = include_bytes!("../icons/tray-sync-down-template.png");
const SYNC_UPDOWN_ICON_PNG: &[u8] = include_bytes!("../icons/tray-sync-updown-template.png");
const SYNC_REFRESH_ICON_PNG: &[u8] = include_bytes!("../icons/tray-sync-refresh-template.png");
const SYNC_PAUSED_ICON_PNG: &[u8] = include_bytes!("../icons/tray-sync-paused-template.png");
const SYNC_WARNING_ICON_PNG: &[u8] = include_bytes!("../icons/tray-sync-warning-template.png");

/// The colour the authored glyph is repainted in on every platform that is not
/// macOS — the keeper mark's bright tone, chosen because it separates from both
/// a dark and a light panel.
#[cfg(not(target_os = "macos"))]
const PANEL_GLYPH_RGB: [u8; 3] = [0x3E, 0xCF, 0xAE];

/// The one-pixel contrast halo grown around the repainted glyph — the mark's
/// deep tone. On a dark panel it disappears into the background; on a light one
/// it is what stops the bright mark dissolving into white.
#[cfg(not(target_os = "macos"))]
const PANEL_HALO_RGB: [u8; 3] = [0x06, 0x23, 0x1C];

/// How opaque an authored pixel must be to grow a halo. Below this the glyph is
/// already fading out and an outline would only thicken it.
#[cfg(not(target_os = "macos"))]
const PANEL_HALO_THRESHOLD: u8 = 96;

/// Decode a bundled glyph for the tray, adapting it to the host's tray host.
///
/// On macOS the glyphs are TEMPLATE images: monochrome black + alpha, which the
/// menu bar recolours per appearance and inverts on highlight. Black is the
/// correct authoring colour there, so the bytes are used verbatim.
///
/// Nowhere else. `icon_as_template` is documented macOS-only and is silently
/// ignored by every other backend, so an ayatana/StatusNotifier panel or the
/// Windows notification area blits the authored PNG as-is — and a pure-black
/// mark on the default dark panel is a black-on-black silhouette. Off macOS the
/// same authored SHAPE is therefore repainted at decode time in the brand tone
/// with a contrast halo, which reads on a light panel and a dark one without
/// authoring a second icon set or guessing the panel's theme. Colour still
/// carries no information: every state continues to read from the shape.
#[cfg(target_os = "macos")]
fn tray_glyph(png: &[u8]) -> Option<Image<'static>> {
    Image::from_bytes(png).ok()
}

#[cfg(not(target_os = "macos"))]
fn tray_glyph(png: &[u8]) -> Option<Image<'static>> {
    let decoded = Image::from_bytes(png).ok()?;
    let (width, height) = (decoded.width(), decoded.height());
    let painted = paint_for_panel(decoded.rgba(), width, height);
    Some(Image::new_owned(painted, width, height))
}

/// Repaint a monochrome template glyph for a tray host that does not recolour
/// it: every authored pixel becomes [`PANEL_GLYPH_RGB`] at its original alpha,
/// and every transparent pixel touching a sufficiently opaque one becomes a
/// [`PANEL_HALO_RGB`] outline pixel carrying the strongest neighbouring alpha.
///
/// Pure and total: a buffer whose length does not match `width * height * 4` is
/// returned unchanged rather than panicking, because a cosmetic glyph must never
/// be able to take the tray down.
#[cfg(not(target_os = "macos"))]
fn paint_for_panel(src: &[u8], width: u32, height: u32) -> Vec<u8> {
    let (w, h) = (width as usize, height as usize);
    if src.len() != w * h * 4 {
        return src.to_vec();
    }
    let alpha_at = |x: usize, y: usize| src[(y * w + x) * 4 + 3];
    let mut out = vec![0u8; src.len()];
    for y in 0..h {
        for x in 0..w {
            let i = (y * w + x) * 4;
            let alpha = src[i + 3];
            if alpha > 0 {
                out[i..i + 3].copy_from_slice(&PANEL_GLYPH_RGB);
                out[i + 3] = alpha;
                continue;
            }
            // Transparent: grow the halo from the strongest opaque neighbour.
            let mut halo = 0u8;
            for dy in -1i32..=1 {
                for dx in -1i32..=1 {
                    let (nx, ny) = (x as i32 + dx, y as i32 + dy);
                    if nx < 0 || ny < 0 || nx >= w as i32 || ny >= h as i32 {
                        continue;
                    }
                    let neighbour = alpha_at(nx as usize, ny as usize);
                    if neighbour >= PANEL_HALO_THRESHOLD {
                        halo = halo.max(neighbour);
                    }
                }
            }
            if halo > 0 {
                out[i..i + 3].copy_from_slice(&PANEL_HALO_RGB);
                out[i + 3] = halo;
            }
        }
    }
    out
}

/// Install a glyph on a live tray.
///
/// macOS `isTemplate` is a property of the IMAGE, not of the tray, so it has to
/// be re-asserted on every swap or the glyph falls back to plain-alpha
/// rendering. Off macOS the flag is deliberately NOT asserted: [`tray_glyph`]
/// hands back a coloured image there, and claiming template rendering for it
/// would become a lie the day a backend implements the flag.
///
/// Returns whether the glyph actually reached the OS — the icon is cosmetic, so
/// a decode or set failure keeps whatever is already up, but a caller that
/// memoises what it pushed must not memoise a glyph that never landed.
fn set_tray_glyph(tray: &TrayIcon, png: &[u8]) -> bool {
    let Some(image) = tray_glyph(png) else {
        return false;
    };
    if let Err(error) = tray.set_icon(Some(image)) {
        tracing::warn!(%error, "tray: could not set the tray glyph");
        return false;
    }
    #[cfg(target_os = "macos")]
    let _ = tray.set_icon_as_template(true);
    true
}

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
    /// The disabled folder-sync status line, when the sync menu is installed
    /// (Story 29.3). Held for the same reason as `status_item`: each tick
    /// refreshes it with `set_text` rather than rebuilding the menu, so an open
    /// menu stays open and nothing flickers.
    ///
    /// Mutually exclusive with the recording rendering — recording owns the
    /// tray whenever it is live or holding an error, and sync waits.
    sync_item: Option<MenuItem<Wry>>,
    /// What the sync renderer last actually pushed to this tray: the glyph on
    /// screen and the words in the status line (AD-34-1). `None` means nothing
    /// has been pushed yet, so the next sync tick writes unconditionally.
    ///
    /// It lives in the slot rather than a module `static` precisely so that
    /// dropping the tray forgets it — a rebuilt menu-bar item starts on the
    /// idle mark and has to be painted once, whatever the engine happens to be
    /// reporting at that moment.
    sync_memo: Option<SyncMemo>,
    /// The notes items of whichever menu is currently installed (Story 36.7).
    ///
    /// `None` means the installed menu has none — either the notes capability is
    /// off, in which case it is `None` for the process lifetime, or a recording
    /// or error rendering displaced them, in which case the idle restore brings
    /// them back. A `MenuItem` no longer in an installed menu would have the next
    /// tick `set_text` into nothing.
    notes: Option<NotesItems>,
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

/// Build the notes section's items, or `None` when the capability is off.
///
/// Built once per menu and then only mutated (AD-61). Every label starts at its
/// empty-state wording rather than blank, so a tray built before the first index
/// publish reads correctly instead of showing five unexplained gaps.
fn build_notes_items(app: &AppHandle, enabled: bool) -> Option<NotesItems> {
    if !enabled {
        // Omitted at build time on a build where notes is absent, which is
        // correct — and, because the menu is built once, omitted forever.
        return None;
    }
    let recent: Vec<MenuItem<Wry>> = NOTE_RECENT_IDS
        .iter()
        .map(|id| menu_item(app, id, NOTE_EMPTY_SLOT, false))
        .collect::<Option<Vec<_>>>()?;
    Some(NotesItems {
        new_note: menu_item(app, NOTE_NEW_ID, "New Note", true)?,
        capture: menu_item(app, NOTE_CAPTURE_ID, "Quick Capture", true)?,
        journal: menu_item(app, NOTE_JOURNAL_ID, "Today\u{2019}s Journal", true)?,
        recent,
        unread: menu_item(app, NOTE_UNREAD_ID, NOTE_NO_UNREAD, false)?,
    })
}

/// Append the notes section to a menu under construction.
///
/// Order is fixed by UX-DR39/AD-61: the three verbs, a separator, the five recent
/// slots, a separator, the unread line, a separator — all **above** the existing
/// status block and Show/Quit.
fn add_notes_section<'a>(
    builder: MenuBuilder<'a, Wry, AppHandle>,
    items: Option<&'a NotesItems>,
) -> MenuBuilder<'a, Wry, AppHandle> {
    let Some(items) = items else {
        return builder;
    };
    let mut builder = builder
        .items(&[&items.new_note, &items.capture, &items.journal])
        .separator();
    for slot in &items.recent {
        builder = builder.item(slot);
    }
    builder.separator().item(&items.unread).separator()
}

/// Build the idle tray menu: the notes section, then "Show keeper" + "Quit"
/// (Story 10.3, Story 36.7).
fn build_idle_menu(app: &AppHandle) -> Option<(Menu<Wry>, Option<NotesItems>)> {
    let notes = build_notes_items(app, notes_capability(app));
    let show = menu_item(app, SHOW_ID, "Show keeper", true)?;
    let quit = menu_item(app, QUIT_ID, "Quit", true)?;
    let builder = add_notes_section(MenuBuilder::new(app), notes.as_ref());
    match builder.items(&[&show, &quit]).build() {
        Ok(menu) => Some((menu, notes)),
        Err(error) => {
            tracing::warn!(%error, "tray: could not build tray menu");
            None
        }
    }
}

/// The label the unread line shows when there is nothing to read.
const NOTE_NO_UNREAD: &str = "No unread note changes";

/// Whether this build shows notes at all (FR-122).
///
/// Read once per menu build rather than cached, because the menu is built rarely
/// and a stale capability would be baked in for the process lifetime.
fn notes_capability(app: &AppHandle) -> bool {
    crate::ipc::notes_capability(app)
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
fn build_tray(app: &AppHandle) -> Option<(TrayIcon, Option<NotesItems>)> {
    let (menu, notes) = build_idle_menu(app)?;
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
            // The notes section (Story 36.7). Routed through the same single
            // router, which is registered on the tray rather than on a menu and
            // therefore survives every `set_menu` — so click routing needed no
            // new mechanism.
            NOTE_NEW_ID => crate::notes_ipc::tray_new_note(app),
            NOTE_CAPTURE_ID => crate::notes_window::show(app),
            NOTE_JOURNAL_ID => crate::notes_ipc::tray_journal_today(app),
            NOTE_UNREAD_ID => crate::notes_ipc::tray_show_unread(app),
            id => {
                if let Some(slot) = NOTE_RECENT_IDS.iter().position(|recent| *recent == id) {
                    crate::notes_ipc::tray_open_recent(app, slot);
                }
            }
        });
    // The idle ring (Story 21.4): the authored monochrome + alpha template on
    // macOS, repainted for the panel everywhere else (see `tray_glyph`). A
    // decode failure falls back to the app icon.
    match tray_glyph(IDLE_ICON_PNG) {
        Some(icon) => {
            builder = builder.icon(icon);
            #[cfg(target_os = "macos")]
            {
                builder = builder.icon_as_template(true);
            }
        }
        None => {
            if let Some(icon) = app.default_window_icon().cloned() {
                builder = builder.icon(icon);
            }
        }
    }
    match builder.build(app) {
        Ok(tray) => Some((tray, notes)),
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
        *guard = tray.map(|(icon, notes)| TrayState {
            icon,
            status_item: None,
            error_rendered: false,
            sync_item: None,
            sync_memo: None,
            notes,
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
    let Some((icon, notes)) = build_tray(app) else {
        return;
    };
    let stored = {
        let mut guard = tray_guard();
        if guard.is_none() {
            *guard = Some(TrayState {
                icon: icon.clone(),
                status_item: None,
                error_rendered: false,
                sync_item: None,
                sync_memo: None,
                notes,
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
    if !set_tray_glyph(tray, RECORDING_ICON_PNG) {
        tracing::warn!("tray: could not install the record-dot glyph");
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
    if !set_tray_glyph(tray, ERROR_ICON_PNG) {
        tracing::warn!("tray: could not install the error badge glyph");
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
    let Some((menu, notes)) = build_idle_menu(app) else {
        return;
    };
    if let Err(error) = tray.set_menu(Some(menu)) {
        tracing::warn!(%error, "tray: could not restore the idle menu");
        return;
    }
    // The icon is cosmetic — a failure here does not desync the flag/menu.
    if !set_tray_glyph(tray, IDLE_ICON_PNG) {
        tracing::warn!("tray: could not restore the idle glyph");
    }
    store_rendered_mode(tray.id(), None, false);
    // The fresh menu carries fresh notes handles; the labels the last tick
    // computed are re-applied to them so a restore does not blank the section
    // until something changes.
    adopt_notes_items(tray.id(), notes);
}

/// Store the rendered-mode flags — the recording status-line item and the
/// error-hold flag (mutually exclusive; both cleared on an idle restore) —
/// back into the tray slot, unless the slot was rebuilt (toggle off→on) while
/// the menu was installed lock-free, in which case the fresh tray's own state
/// wins.
///
/// Whichever rendering this installs displaced the sync menu, so the held sync
/// line and the memo of what sync last pushed go with it: a `MenuItem` that is
/// no longer in any installed menu would have the next sync tick `set_text`
/// into nothing, and a surviving memo would have it believe its glyph is still
/// on screen.
fn store_rendered_mode(tray_id: &TrayIconId, item: Option<MenuItem<Wry>>, error_rendered: bool) {
    let mut guard = tray_guard();
    if let Some(state) = guard.as_mut() {
        if state.icon.id() == tray_id {
            state.status_item = item;
            state.error_rendered = error_rendered;
            state.sync_item = None;
            state.sync_memo = None;
            // The recording and error menus carry no notes section, so the held
            // handles are no longer in any installed menu. The idle restore
            // re-adopts fresh ones.
            state.notes = None;
        }
    }
}

/// Adopt the notes handles of a freshly installed menu and repaint them from the
/// last model this process composed.
///
/// A menu build mints new `MenuItem`s, so the labels the previous tick wrote live
/// on handles that are no longer on screen. Re-applying the retained model is what
/// keeps the section correct across an install without a `set_menu` refresh loop.
fn adopt_notes_items(tray_id: &TrayIconId, notes: Option<NotesItems>) {
    let model = {
        let mut guard = tray_guard();
        let Some(state) = guard.as_mut() else {
            return;
        };
        if state.icon.id() != tray_id {
            return;
        }
        state.notes = notes.clone();
        notes_model().clone()
    };
    if let Some(items) = notes {
        paint_notes(&items, &model);
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
///
/// Story 41.6 reduces durability into that SAME composition and adds nothing to
/// it. A session that is recording fine says exactly what it says today — the
/// menu bar is not the place to narrate a healthy publication, and the banner
/// already carries the reading in full. What the tray owes the user is the
/// exception: a session whose segments are on this Mac and whose push the remote
/// refused rides the warning marker this tray already has for "still recording,
/// something needs your attention", in the recorder's words plus the reason
/// verbatim. Recording keeps the icon (Story 18.1) and sync still never forces
/// presence — nothing here builds or holds a tray.
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
    // PRECEDENCE: a capture warning outranks a publication one. A lost
    // microphone is about the recording being made right now; a refused push is
    // about a recording that is already safely on this disk. Two markers on one
    // line would read as one louder problem rather than two, so the more urgent
    // one takes the line.
    match (
        snapshot.warning.as_deref(),
        durability_warning(&snapshot.durability),
    ) {
        (Some(message), _) => format_warning_line(&base, message),
        (None, Some(message)) => format_warning_line(&base, &message),
        (None, None) => base,
    }
}

/// The durability reading the tray has something to say about, or `None`
/// (Story 41.6, UX-DR48).
///
/// Only one reading qualifies: segments that are committed here but not
/// published, with the reason the engine recorded. `local` is not a problem (a
/// plain-folder destination promised nothing, and a session that has not
/// committed yet is simply early), and `pushed`/`verified` are the good news the
/// banner already carries — announcing either in the menu bar would be sync
/// colonising a surface a recording forced into existence.
///
/// The words are the recorder's, never git's: "recorded, not pushed" is what
/// happened, and the engine's own sentence follows it verbatim rather than being
/// paraphrased into a generic sync error. Pure.
fn durability_warning(durability: &RecordingDurabilityVm) -> Option<String> {
    let reason = durability.detail.as_deref()?;
    if durability.state >= RecordingDurabilityState::Pushed {
        return None;
    }
    Some(format!("recorded, not pushed — {reason}"))
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

    /// Byte offset of pixel `(x, y)` in a `width`-wide RGBA8 buffer. Spelling it
    /// out keeps the fixtures readable and keeps `clippy::identity_op` off the
    /// literal `(1 * 3 + 1) * 4` shape.
    #[cfg(not(target_os = "macos"))]
    fn px(x: usize, y: usize, width: usize) -> usize {
        (y * width + x) * 4
    }

    /// AD-61: off macOS nothing recolours the tray glyph, so the authored
    /// black-on-alpha template is repainted at decode time. The body takes the
    /// brand tone at its original alpha — a pure-black glyph on a dark panel is
    /// the defect this exists to prevent.
    #[cfg(not(target_os = "macos"))]
    #[test]
    fn panel_repaint_recolours_the_body_and_keeps_its_alpha() {
        // 3x3, one opaque pixel dead centre, everything else transparent.
        let mut src = vec![0u8; 3 * 3 * 4];
        src[px(1, 1, 3) + 3] = 200;
        let out = paint_for_panel(&src, 3, 3);

        let centre = px(1, 1, 3);
        assert_eq!(&out[centre..centre + 3], &PANEL_GLYPH_RGB);
        assert_eq!(out[centre + 3], 200, "body alpha is preserved");
    }

    /// The halo is what keeps the repainted glyph legible on a LIGHT panel: the
    /// eight neighbours of an opaque pixel become outline, and nothing further
    /// out is touched.
    #[cfg(not(target_os = "macos"))]
    #[test]
    fn panel_repaint_grows_a_one_pixel_halo_and_no_more() {
        let mut src = vec![0u8; 5 * 5 * 4];
        src[px(2, 2, 5) + 3] = 255;
        let out = paint_for_panel(&src, 5, 5);

        for (x, y) in [
            (1, 1),
            (2, 1),
            (3, 1),
            (1, 2),
            (3, 2),
            (1, 3),
            (2, 3),
            (3, 3),
        ] {
            let i = px(x, y, 5);
            assert_eq!(&out[i..i + 3], &PANEL_HALO_RGB, "halo at ({x},{y})");
            assert_eq!(out[i + 3], 255, "halo alpha at ({x},{y})");
        }
        // Two pixels out is still empty — the outline never thickens.
        let far = px(0, 0, 5);
        assert_eq!(&out[far..far + 4], &[0, 0, 0, 0]);
    }

    /// A faint pixel is already fading out; outlining it would only thicken the
    /// stroke, which the 44x44-at-~22px downscale cannot afford.
    #[cfg(not(target_os = "macos"))]
    #[test]
    fn panel_repaint_does_not_halo_faint_pixels() {
        let mut src = vec![0u8; 3 * 3 * 4];
        src[px(1, 1, 3) + 3] = PANEL_HALO_THRESHOLD - 1;
        let out = paint_for_panel(&src, 3, 3);

        let corner = px(0, 0, 3);
        assert_eq!(&out[corner..corner + 4], &[0, 0, 0, 0]);
    }

    /// A cosmetic glyph must never be able to take the tray down: a buffer that
    /// does not match the declared geometry comes back untouched.
    #[cfg(not(target_os = "macos"))]
    #[test]
    fn panel_repaint_passes_through_a_mismatched_buffer() {
        let src = vec![7u8; 9];
        assert_eq!(paint_for_panel(&src, 3, 3), src);
    }

    /// Every shipped glyph survives the panel repaint at its authored geometry —
    /// the real assets, not a synthetic buffer.
    #[cfg(not(target_os = "macos"))]
    #[test]
    fn every_shipped_glyph_repaints_for_the_panel() {
        for bytes in [
            IDLE_ICON_PNG,
            RECORDING_ICON_PNG,
            ERROR_ICON_PNG,
            SYNC_ARMED_ICON_PNG,
            SYNC_UP_ICON_PNG,
            SYNC_DOWN_ICON_PNG,
            SYNC_UPDOWN_ICON_PNG,
            SYNC_REFRESH_ICON_PNG,
            SYNC_PAUSED_ICON_PNG,
            SYNC_WARNING_ICON_PNG,
        ] {
            let authored = Image::from_bytes(bytes).expect("glyph decodes");
            let painted = tray_glyph(bytes).expect("glyph repaints");
            assert_eq!(painted.width(), authored.width());
            assert_eq!(painted.height(), authored.height());
            // The repaint must actually change something, or the panel still
            // gets a black-on-black silhouette.
            assert_ne!(painted.rgba(), authored.rgba());
        }
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

    // --- Story 41.6: durability, reduced into the existing composition ------

    /// A live recording snapshot with the given durability reading.
    fn recording_with(durability: RecordingDurabilityVm) -> RecordingStatusVm {
        let mut snapshot = RecordingStatusVm::idle();
        snapshot.state = RecordingUiState::Recording;
        snapshot.durability = durability;
        snapshot
    }

    /// A reading of segments that are committed here and whose push the remote
    /// refused — the epic's "recorded, not pushed".
    fn stuck(reason: &str) -> RecordingDurabilityVm {
        RecordingDurabilityVm {
            state: RecordingDurabilityState::Committed,
            detail: Some(reason.to_owned()),
        }
    }

    /// A healthy session's line is EXACTLY today's line. The menu bar does not
    /// narrate a publication that is going fine — every state that is not a
    /// stuck publication leaves the composition untouched.
    #[test]
    fn a_healthy_durability_reading_leaves_the_tray_line_unchanged() {
        let today = status_line(&recording_with(RecordingDurabilityVm::local()));
        assert_eq!(today, "Starting…");
        for state in [
            RecordingDurabilityState::Local,
            RecordingDurabilityState::Committed,
            RecordingDurabilityState::Pushed,
            RecordingDurabilityState::Verified,
        ] {
            let snapshot = recording_with(RecordingDurabilityVm {
                state,
                detail: None,
            });
            assert_eq!(status_line(&snapshot), today, "{state:?}");
        }
        // A reason that arrives once publication already succeeded is not a
        // problem to report either — the bytes are on the drive.
        for state in [
            RecordingDurabilityState::Pushed,
            RecordingDurabilityState::Verified,
        ] {
            let snapshot = recording_with(RecordingDurabilityVm {
                state,
                detail: Some("push rejected: non-fast-forward".to_owned()),
            });
            assert_eq!(status_line(&snapshot), today, "{state:?}");
        }
    }

    /// The killed-network and protected-branch rows: the tray marks the line
    /// with the warning glyph it ALREADY has for a live session that needs
    /// attention (Story 19.4's `⚠`), the recording line stays first because the
    /// session is still recording, and the engine's sentence is carried verbatim
    /// after the recorder's own words. No new glyph was needed or added.
    #[test]
    fn a_refused_push_rides_the_existing_warning_marker_with_the_reason() {
        let snapshot = recording_with(stuck("push rejected: protected branch main"));
        assert_eq!(
            status_line(&snapshot),
            "⚠ Starting… — recorded, not pushed — push rejected: protected branch main"
        );
        assert!(
            snapshot.state.is_live(),
            "a refused push is still a live recording"
        );
        assert!(
            snapshot.error.is_none(),
            "a refused push is not a failed session"
        );
    }

    /// Precedence: a capture warning is about the recording being made right
    /// now, a refused push is about one already safe on this disk. The capture
    /// warning takes the line, and the line carries exactly one marker.
    #[test]
    fn a_capture_warning_outranks_a_publication_one() {
        let mut snapshot = recording_with(stuck("push rejected: non-fast-forward"));
        snapshot.warning = Some("microphone disconnected — no microphone input".to_owned());
        let line = status_line(&snapshot);
        assert_eq!(
            line,
            "⚠ Starting… — microphone disconnected — no microphone input"
        );
        assert_eq!(line.matches('⚠').count(), 1, "two markers read as one");
    }

    /// Only a stuck publication speaks. `local` promised nothing, and
    /// `pushed`/`verified` are the banner's good news, not the menu bar's.
    #[test]
    fn durability_warning_speaks_only_for_a_stuck_publication() {
        assert_eq!(durability_warning(&RecordingDurabilityVm::local()), None);
        assert_eq!(
            durability_warning(&RecordingDurabilityVm {
                state: RecordingDurabilityState::Local,
                detail: Some("no remote configured".to_owned()),
            })
            .as_deref(),
            Some("recorded, not pushed — no remote configured"),
        );
        assert_eq!(
            durability_warning(&stuck("boom")).as_deref(),
            Some("recorded, not pushed — boom")
        );
        assert_eq!(
            durability_warning(&RecordingDurabilityVm {
                state: RecordingDurabilityState::Pushed,
                detail: Some("stale".to_owned()),
            }),
            None
        );
    }

    /// The epic's rule, unchanged: recording wins the icon and sync never forces
    /// presence. Durability is not an input to [`decide_presence`] at all — a
    /// refused push is not a session `error`, so it can neither build a tray, nor
    /// drop one, nor swap the record dot for the error badge. The reduction is
    /// confined to the status line.
    #[test]
    fn a_push_problem_never_changes_presence_or_the_recording_icon() {
        use PresenceAction::*;
        let snapshot = recording_with(stuck("push rejected: protected branch main"));
        let has_error = snapshot.error.is_some();
        for state in [
            RecordingUiState::Recording,
            RecordingUiState::Rotating,
            RecordingUiState::Stopping,
        ] {
            assert_eq!(
                decide_presence(state, has_error, true, false),
                RenderRecording,
                "{state:?} must keep rendering the recording tray"
            );
            assert_eq!(
                decide_presence(state, has_error, false, false),
                ForcePresent,
                "{state:?}"
            );
        }
        // And a settled session with a stuck push still tears the tray down the
        // ordinary way — a publication problem must not hold the menu bar.
        let mut settled = snapshot;
        settled.state = RecordingUiState::Finalized;
        assert_eq!(decide_presence(settled.state, false, true, true), DropTray);
        assert_eq!(
            decide_presence(settled.state, false, true, false),
            RestoreIdle
        );
    }
}

// ---------------------------------------------------------------------------
// Folder sync (Stories 29.2 / 29.3, AD-51)
// ---------------------------------------------------------------------------

/// The sync menu's disabled status line.
const SYNC_STATUS_ID: &str = "tray-sync-status";

/// Pick the sync glyph for a state.
///
/// Pure so the precedence is testable without a tray: **warning outranks
/// transferring outranks activity outranks paused outranks armed**, because a
/// problem must never be hidden by something else happening to be busy.
///
/// Every state is one still asset, so the glyph is a function of state alone —
/// the reason this no longer takes a frame. What used to be carried by motion is
/// now carried by the corner mark, and carried better: an arrow says which way
/// the bytes are going, where a spinning ring only said that something was.
/// `Active` — scanning, committing, verifying — gets the circular arrows, which
/// is the same "working" claim without the promise of a transfer.
fn sync_glyph(state: TraySyncState) -> Option<&'static [u8]> {
    match state {
        TraySyncState::Absent => None,
        TraySyncState::Warning => Some(SYNC_WARNING_ICON_PNG),
        TraySyncState::Transferring => Some(SYNC_UPDOWN_ICON_PNG),
        TraySyncState::Uploading => Some(SYNC_UP_ICON_PNG),
        TraySyncState::Downloading => Some(SYNC_DOWN_ICON_PNG),
        TraySyncState::Active => Some(SYNC_REFRESH_ICON_PNG),
        TraySyncState::Paused => Some(SYNC_PAUSED_ICON_PNG),
        TraySyncState::Armed => Some(SYNC_ARMED_ICON_PNG),
    }
}

/// Which glyph is on the tray, as an identity cheap enough to compare on every
/// tick: the address of the `&'static [u8]` asset [`sync_glyph`] returned.
///
/// Derived from the asset instead of declared beside it, so it cannot drift
/// from the mapping it describes, and so the animation is compared frame by
/// frame for free — a frame IS a different asset. Decoding the image to
/// compare it, or hashing the PNG, would cost more per tick than the repaint
/// this exists to avoid. Two byte-identical assets the linker chose to merge
/// would compare equal, which is the right answer anyway: the same bytes
/// render the same glyph.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct SyncGlyphId(usize);

impl SyncGlyphId {
    fn of(glyph: &'static [u8]) -> Self {
        Self(glyph.as_ptr() as usize)
    }
}

/// What the sync renderer last pushed to a tray (AD-34-1), held in
/// [`TrayState`] so it dies with the tray it describes.
#[derive(Debug, Clone)]
struct SyncMemo {
    /// The glyph that is on screen.
    glyph: SyncGlyphId,
    /// The status line as installed. `Arc<str>` because the memo is cloned out
    /// of the slot on every ~1 Hz tick, and the steady state — the one this
    /// exists to make free — must not allocate to discover it has nothing to
    /// do.
    line: Arc<str>,
}

/// What a sync tick still owes the tray.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct SyncWrites {
    /// `set_icon` plus the `set_icon_as_template` that must follow it.
    icon: bool,
    /// The status line: a `set_text`, or the menu install that carries it.
    text: bool,
}

impl SyncWrites {
    /// A tray nothing has been pushed to yet.
    const BOTH: Self = Self {
        icon: true,
        text: true,
    };

    /// Nothing to push: the tick returns without touching the OS.
    fn is_empty(self) -> bool {
        !self.icon && !self.text
    }
}

/// Diff this tick's rendering against what the tray was last given (AD-34-1).
///
/// Pure, and a returned value rather than an inline `if`, because the property
/// worth testing is a negative — N identical ticks perform ONE write — and a
/// `TrayIcon` cannot be built in a unit test to observe it.
fn sync_writes(
    memo: Option<&SyncMemo>,
    installed: bool,
    glyph: SyncGlyphId,
    line: &str,
) -> SyncWrites {
    match memo {
        // A fresh or rebuilt tray carries the idle mark and no sync menu.
        None => SyncWrites::BOTH,
        Some(memo) => SyncWrites {
            icon: memo.glyph != glyph,
            // A displaced menu has to be reinstalled even when the words did
            // not change, or the sync line never comes back.
            text: !installed || &*memo.line != line,
        },
    }
}

/// How many ~1 s ticks a busy glyph survives after the engine stops reporting
/// one. Long enough to swallow a scan that begins and ends between two ticks,
/// short enough that the menu bar is not lying about what is happening now.
const BUSY_DWELL_TICKS: u8 = 3;

/// The busy state being held, and for how many more ticks.
static SYNC_DWELL: Mutex<Option<(TraySyncState, u8)>> = Mutex::new(None);

/// Smooth the reported state so short work cannot blink the menu bar.
///
/// The engine reports what is true at the instant it is asked. A scan that
/// takes 40 ms is true, and rendering it faithfully at 1 Hz turned the glyph
/// into a strobe: armed, working, armed, working, on a folder where nothing
/// was happening. Slowing the scans (`Engine::scan_is_due`) removed the cause;
/// this removes the symptom for every OTHER short unit of work — a small
/// commit, a fast merge — which no cadence change can prevent.
///
/// Only the fall BACK to rest is delayed. Escalation is immediate: a problem,
/// a transfer, or the last profile going away all render on the tick they
/// happen. Delaying bad news to keep the icon calm would be the dishonest
/// version of this.
fn dwelled(reported: TraySyncState) -> TraySyncState {
    let mut held = SYNC_DWELL
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    match reported {
        // Busy: show it, and arm the hold. Each direction holds as *itself*, so
        // an upload that ends does not decay through a generic "transferring"
        // glyph on its way back to armed.
        TraySyncState::Transferring
        | TraySyncState::Uploading
        | TraySyncState::Downloading
        | TraySyncState::Active => {
            *held = Some((reported, BUSY_DWELL_TICKS));
            reported
        }
        // Something the user must see, or nothing left to render.
        TraySyncState::Warning | TraySyncState::Absent => {
            *held = None;
            reported
        }
        // At rest. Keep showing the recent work until its hold expires.
        TraySyncState::Armed | TraySyncState::Paused => match *held {
            Some((state, ticks)) if ticks > 0 => {
                *held = Some((state, ticks - 1));
                state
            }
            _ => {
                *held = None;
                reported
            }
        },
    }
}

/// Build the sync tray menu: the notes section, a disabled status line, then Show
/// and Quit.
///
/// The notes items are here too, and not only in the idle menu, because this menu
/// replaces that one on the first sync tick — on macOS, where `set_menu` works.
/// Omitting them here would make the section vanish the moment a folder started
/// syncing, which is most of the time.
fn build_sync_menu(
    app: &AppHandle,
    line: &str,
) -> Option<(Menu<Wry>, MenuItem<Wry>, Option<NotesItems>)> {
    let notes = build_notes_items(app, notes_capability(app));
    let status = menu_item(app, SYNC_STATUS_ID, line, false)?;
    let show = menu_item(app, SHOW_ID, "Show keeper", true)?;
    let quit = menu_item(app, QUIT_ID, "Quit", true)?;
    let builder = add_notes_section(MenuBuilder::new(app), notes.as_ref());
    match builder.items(&[&status, &show, &quit]).build() {
        Ok(menu) => Some((menu, status, notes)),
        Err(error) => {
            tracing::warn!(%error, "tray: could not build the sync menu");
            None
        }
    }
}

/// Render the tray from the folder-sync snapshot (Stories 29.2 / 29.3).
///
/// Called by the same ~1 Hz tick that drives recording, immediately after it.
/// Two rules keep the two subsystems from fighting over one icon:
///
/// 1. **Recording always wins.** If a recording status line or the error hold
///    is installed, this returns immediately. A capture in progress is the more
///    urgent thing to show, and it is the one that forced the tray into
///    existence in the first place.
/// 2. **Sync never forces presence.** Unlike a live recording (Story 18.2),
///    sync renders only into a tray the user already opted into. Silently
///    summoning a menu-bar item because a background folder started syncing
///    would be a surprise, and the in-app surfaces carry the same information.
///
/// And one rule about when it writes at all:
///
/// 3. **A write happens on a transition, never on a tick** (AD-34-1). The
///    engine reports the same state second after second; the glyph and the
///    status line are each diffed against what this tray was last given, and a
///    tick that would push neither returns without touching the OS.
///
/// Follows `apply_recording_state`'s lock discipline exactly: handles are
/// cloned out, every `TrayIcon`/`MenuItem` call runs lock-free (each blocks on
/// an internal main-thread dispatch), and the held item — with the memo of what
/// actually landed — is stored back under a fresh lock checked against the
/// tray id.
pub fn apply_sync_state(app: &AppHandle, state: TraySyncState, line: &str) {
    let (tray, sync_item, memo, recording_owns) = {
        let guard = tray_guard();
        match guard.as_ref() {
            Some(tray_state) => (
                Some(tray_state.icon.clone()),
                tray_state.sync_item.clone(),
                tray_state.sync_memo.clone(),
                tray_state.status_item.is_some() || tray_state.error_rendered,
            ),
            None => (None, None, None, false),
        }
    };
    let Some(tray) = tray else {
        return;
    };
    if recording_owns {
        return;
    }

    // Smooth the glyph, never the words: the status line below still reports
    // exactly what the engine said this tick.
    let state = dwelled(state);
    let Some(glyph) = sync_glyph(state) else {
        // No profiles at all: leave the plain idle tray exactly as it was, and
        // tear down a sync menu if one is still installed. `restore_idle` drops
        // the held line and the memo with it, and only once the idle menu is
        // really installed — a failed teardown is then retried next tick
        // instead of forgetting that the sync menu is still on screen.
        if sync_item.is_some() {
            restore_idle(app, &tray);
        }
        return;
    };
    let id = SyncGlyphId::of(glyph);

    // AD-34-1: a write happens on a transition, never on a tick. The engine
    // reports the same state second after second, and this used to decode the
    // same PNG and push it again every second — the flicker the user saw. The
    // icon and the line are diffed apart because they move on different
    // cadences: a pending count changes while the glyph holds still, and
    // neither should drag the other onto the tray.
    let writes = sync_writes(memo.as_ref(), sync_item.is_some(), id, line);
    if writes.is_empty() {
        return;
    }

    let (item, line_landed) = match sync_item {
        // Steady state: refresh the text only. No menu rebuild, no flicker, and
        // an open menu stays open.
        Some(item) => {
            let mut landed = true;
            if writes.text {
                if let Err(error) = item.set_text(line) {
                    tracing::warn!(%error, "tray: could not refresh the sync status line");
                    landed = false;
                }
            }
            (item, landed)
        }
        // First sync tick since the tray went idle: install the menu once. It
        // is built around this line, so no separate text write is owed.
        None => {
            let Some((menu, item, notes)) = build_sync_menu(app, line) else {
                return;
            };
            if let Err(error) = tray.set_menu(Some(menu)) {
                tracing::warn!(%error, "tray: could not install the sync menu");
                return;
            }
            // Fresh menu, fresh notes handles: adopt them and repaint from the
            // retained model, exactly as the idle restore does.
            adopt_notes_items(tray.id(), notes);
            (item, true)
        }
    };

    let glyph_landed = !writes.icon || push_sync_glyph(&tray, glyph);
    // Remember only a tick whose writes actually landed. A failed push has to
    // stay pending, or the next tick would skip its retry and leave the wrong
    // glyph — or a stale claim — on the tray until the state happens to change.
    let pushed = (line_landed && glyph_landed).then(|| SyncMemo {
        glyph: id,
        line: Arc::from(line),
    });
    store_sync_render(tray.id(), Some(item), pushed);
}

/// Decode and install a sync glyph, reporting whether it landed.
///
/// A thin alias over [`set_tray_glyph`] kept for the call site's vocabulary:
/// the icon is cosmetic, so a decode or set failure keeps whatever glyph is
/// already up rather than desyncing the menu that was just installed — but the
/// caller must not memoise a glyph that never reached the OS.
fn push_sync_glyph(tray: &TrayIcon, glyph: &'static [u8]) -> bool {
    set_tray_glyph(tray, glyph)
}

/// The last notes model this process composed, so a freshly installed menu can be
/// repainted from it.
///
/// A module `static` rather than a [`TrayState`] field because it survives the
/// slot's build/drop cycle: the model describes the vault, not the menu-bar item,
/// and a tray toggled off and on again should come back saying the same thing.
static NOTES_MODEL: Mutex<Option<NotesTray>> = Mutex::new(None);

fn notes_slot() -> MutexGuard<'static, Option<NotesTray>> {
    NOTES_MODEL
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// The last composed model, or the empty one.
fn notes_model() -> NotesTray {
    notes_slot().clone().unwrap_or_default()
}

/// Render the notes section from the composed model (Story 36.7, AD-61).
///
/// Called by the same ~1 Hz tick that drives recording and sync. Two rules:
///
/// 1. **Never `set_menu`.** Labels move through `set_text` and enablement through
///    `set_enabled` on the retained handles, so an open menu is not closed by an
///    update and Linux — where the menu cannot be replaced at all — behaves
///    identically to macOS.
/// 2. **A write happens on a change, never on a tick.** The model is diffed
///    against the last one this process pushed, and a tick that would say the same
///    thing returns without touching the OS.
pub fn apply_notes_state(app: &AppHandle, model: &NotesTray) {
    let _ = app;
    {
        let mut slot = notes_slot();
        if slot.as_ref() == Some(model) {
            return;
        }
        *slot = Some(model.clone());
    }
    // Clone the handles out and mutate lock-free: every `MenuItem` call blocks on
    // an internal main-thread dispatch, and `set_tray_presence` — which can run
    // ON the main thread — takes the same lock.
    let items = {
        let guard = tray_guard();
        guard.as_ref().and_then(|state| state.notes.clone())
    };
    if let Some(items) = items {
        paint_notes(&items, model);
    }
}

/// Push one model onto one set of retained handles. Best-effort throughout: a
/// failed label write leaves the previous text, which is stale rather than wrong,
/// and the next change retries.
fn paint_notes(items: &NotesItems, model: &NotesTray) {
    // With no vault the two create verbs stay ENABLED: choosing them opens
    // Settings → Sync, because the action is achievable and a disabled row that
    // explains nothing is worse than a row that takes you where you need to go.
    set_label(&items.new_note, &new_note_label(model));
    set_label(&items.capture, &capture_label(model));
    set_label(&items.journal, &journal_label(model));

    for (slot, item) in items.recent.iter().enumerate() {
        match model.recent.get(slot) {
            Some((_, title)) => {
                set_label(item, &truncate_title(title));
                set_enabled(item, true);
            }
            None => {
                set_label(item, NOTE_EMPTY_SLOT);
                set_enabled(item, false);
            }
        }
    }

    set_label(&items.unread, &unread_label(model.unread));
    set_enabled(&items.unread, model.unread > 0);
}

fn set_label(item: &MenuItem<Wry>, label: &str) {
    if let Err(error) = item.set_text(label) {
        tracing::warn!(%error, "tray: could not update a notes menu label");
    }
}

fn set_enabled(item: &MenuItem<Wry>, enabled: bool) {
    if let Err(error) = item.set_enabled(enabled) {
        tracing::warn!(%error, "tray: could not update a notes menu item's state");
    }
}

/// `New Note`, or the honest wording when there is nowhere to put one yet.
fn new_note_label(model: &NotesTray) -> String {
    if model.vault_id.is_some() {
        "New Note".to_owned()
    } else {
        "New Note\u{2026} (no vault yet)".to_owned()
    }
}

/// `Quick Capture`, carrying the registration failure in words when the global
/// shortcut did not take (UX-DR43): the truth belongs where the user is.
fn capture_label(model: &NotesTray) -> String {
    if model.hotkey_registered {
        "Quick Capture".to_owned()
    } else {
        "Quick Capture \u{2014} hotkey unavailable".to_owned()
    }
}

fn journal_label(model: &NotesTray) -> String {
    if model.vault_id.is_some() {
        "Today\u{2019}s Journal".to_owned()
    } else {
        "Today\u{2019}s Journal\u{2026} (no vault yet)".to_owned()
    }
}

/// The unread line's wording. Singular is spelled out rather than pluralised with
/// a bracketed `(s)`, because this is a sentence a person reads.
fn unread_label(unread: u32) -> String {
    match unread {
        0 => NOTE_NO_UNREAD.to_owned(),
        1 => "1 note changed by your agent".to_owned(),
        many => format!("{many} notes changed by your agent"),
    }
}

/// A note title, capped for the menu.
///
/// Truncated on a character boundary and marked with an ellipsis, so a long title
/// cannot stretch the menu to the width of the screen and a cut is visible as a
/// cut. Pure.
fn truncate_title(title: &str) -> String {
    let trimmed = title.trim();
    if trimmed.is_empty() {
        return NOTE_EMPTY_SLOT.to_owned();
    }
    if trimmed.chars().count() <= NOTE_TITLE_CAP {
        return trimmed.to_owned();
    }
    let kept: String = trimmed.chars().take(NOTE_TITLE_CAP - 1).collect();
    format!("{}\u{2026}", kept.trim_end())
}

/// Store the held sync line and the memo of what actually reached the tray,
/// unless the slot was rebuilt (presence toggled off→on) while the menu was
/// installed lock-free — in which case the fresh tray's own state wins, and
/// this tick's writes went to a menu-bar item that no longer exists.
fn store_sync_render(tray_id: &TrayIconId, item: Option<MenuItem<Wry>>, memo: Option<SyncMemo>) {
    let mut guard = tray_guard();
    if let Some(state) = guard.as_mut() {
        if state.icon.id() == tray_id {
            state.sync_item = item;
            state.sync_memo = memo;
        }
    }
}

#[cfg(test)]
mod sync_tray_tests {
    use super::*;

    #[test]
    fn a_warning_glyph_is_never_masked_by_activity_or_pause() {
        // The precedence that matters: a user must not miss a problem because
        // some other profile happens to be transferring.
        assert_eq!(
            sync_glyph(TraySyncState::Warning).map(<[u8]>::len),
            Some(SYNC_WARNING_ICON_PNG.len())
        );
        assert_eq!(
            sync_glyph(TraySyncState::Paused).map(<[u8]>::len),
            Some(SYNC_PAUSED_ICON_PNG.len())
        );
    }

    #[test]
    fn no_profiles_means_no_sync_glyph_at_all() {
        // Absent must leave the plain idle tray alone rather than claiming it.
        assert!(sync_glyph(TraySyncState::Absent).is_none());
    }
    /// One test, not four: `dwelled` keeps its hold in a process-wide static,
    /// and cargo runs tests in parallel threads that would interleave on it.
    #[test]
    fn a_busy_glyph_is_held_briefly_but_bad_news_is_not() {
        *SYNC_DWELL
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = None;

        // Nothing held: rest renders as rest.
        assert_eq!(dwelled(TraySyncState::Armed), TraySyncState::Armed);

        // A single tick of work, then quiet — the reason the menu bar blinked.
        assert_eq!(dwelled(TraySyncState::Active), TraySyncState::Active);
        for tick in 0..BUSY_DWELL_TICKS {
            assert_eq!(
                dwelled(TraySyncState::Armed),
                TraySyncState::Active,
                "tick {tick} fell back to armed while the hold was live"
            );
        }
        assert_eq!(
            dwelled(TraySyncState::Armed),
            TraySyncState::Armed,
            "the hold expires; it does not pin the glyph forever"
        );

        // A problem during a hold shows at once, and ends the hold.
        assert_eq!(dwelled(TraySyncState::Active), TraySyncState::Active);
        assert_eq!(dwelled(TraySyncState::Warning), TraySyncState::Warning);
        assert_eq!(
            dwelled(TraySyncState::Armed),
            TraySyncState::Armed,
            "a warning clears the hold rather than resuming it"
        );

        // The last profile going away must tear the glyph down immediately,
        // not linger for three seconds on a folder that no longer exists.
        assert_eq!(dwelled(TraySyncState::Active), TraySyncState::Active);
        assert_eq!(dwelled(TraySyncState::Absent), TraySyncState::Absent);

        // Transferring outranks Active and is held in its own right.
        assert_eq!(
            dwelled(TraySyncState::Transferring),
            TraySyncState::Transferring
        );
        assert_eq!(dwelled(TraySyncState::Paused), TraySyncState::Transferring);
    }

    #[test]
    fn every_direction_is_its_own_glyph() {
        // The whole point of the corner mark: three transfer states a user can
        // tell apart. Sharing an asset between any two would silently reduce
        // "which way are the bytes going" back to "something is happening".
        let up = SyncGlyphId::of(sync_glyph(TraySyncState::Uploading).expect("up"));
        let down = SyncGlyphId::of(sync_glyph(TraySyncState::Downloading).expect("down"));
        let both = SyncGlyphId::of(sync_glyph(TraySyncState::Transferring).expect("both"));
        assert_ne!(up, down);
        assert_ne!(up, both);
        assert_ne!(down, both);
    }

    #[test]
    fn working_differs_from_both_armed_and_every_transfer() {
        // Scanning or committing is work, not transfer, so it must not draw as
        // one — but it must still differ from armed, or "busy" and "idle" become
        // the same picture. Armed is the centred ring; this is the corner
        // circular arrows.
        let active = SyncGlyphId::of(sync_glyph(TraySyncState::Active).expect("active"));
        for other in [
            TraySyncState::Armed,
            TraySyncState::Uploading,
            TraySyncState::Downloading,
            TraySyncState::Transferring,
            TraySyncState::Paused,
            TraySyncState::Warning,
        ] {
            assert_ne!(
                active,
                SyncGlyphId::of(sync_glyph(other).expect("a glyph")),
                "active must not share an asset with {other:?}"
            );
        }
    }

    #[test]
    fn every_glyph_decodes_at_the_same_size_as_the_idle_mark() {
        // A differently-sized template would jump in the menu bar on every
        // state change.
        let idle = Image::from_bytes(IDLE_ICON_PNG).expect("idle decodes");
        for bytes in [
            SYNC_ARMED_ICON_PNG,
            SYNC_PAUSED_ICON_PNG,
            SYNC_WARNING_ICON_PNG,
            SYNC_UP_ICON_PNG,
            SYNC_DOWN_ICON_PNG,
            SYNC_UPDOWN_ICON_PNG,
            SYNC_REFRESH_ICON_PNG,
        ] {
            let image = Image::from_bytes(bytes).expect("sync glyph decodes");
            assert_eq!(image.width(), idle.width());
            assert_eq!(image.height(), idle.height());
        }
    }

    #[test]
    fn an_unchanged_tick_writes_nothing_at_all() {
        // The story in one assertion: the engine reports the same state every
        // second, and the tray used to decode and re-push the identical PNG
        // every second with it.
        let glyph = SyncGlyphId::of(sync_glyph(TraySyncState::Armed).expect("armed has a glyph"));
        let line = "Photos — up to date";
        assert_eq!(
            sync_writes(None, false, glyph, line),
            SyncWrites::BOTH,
            "a tray with nothing on it yet owes both writes"
        );
        let memo = SyncMemo {
            glyph,
            line: Arc::from(line),
        };
        for tick in 0..5 {
            assert!(
                sync_writes(Some(&memo), true, glyph, line).is_empty(),
                "tick {tick} repainted a tray that had not changed"
            );
        }
    }

    #[test]
    fn a_change_reaches_the_tray_on_the_very_next_tick() {
        // De-duplication must not become coalescing: there is no dwell here,
        // only a diff, so a real transition costs no extra tick.
        let armed = SyncGlyphId::of(sync_glyph(TraySyncState::Armed).expect("armed"));
        let line = "Photos — up to date";
        let memo = SyncMemo {
            glyph: armed,
            line: Arc::from(line),
        };

        let warning = SyncGlyphId::of(sync_glyph(TraySyncState::Warning).expect("warning"));
        let escalated = sync_writes(Some(&memo), true, warning, line);
        assert!(escalated.icon, "a new glyph must repaint on this tick");
        assert!(
            !escalated.text,
            "the words did not change, so the line is left alone"
        );

        let relabelled = sync_writes(Some(&memo), true, armed, "Photos — 3 waiting to sync");
        assert!(
            relabelled.text,
            "new words must reach the line on this tick"
        );
        assert!(
            !relabelled.icon,
            "the glyph did not change, so the icon is left alone"
        );
    }

    #[test]
    fn a_change_of_direction_repaints_the_icon() {
        // The de-duplication memoises the GLYPH, and that is what makes this
        // work: a direction change is a different asset, so it writes. Were the
        // memo keyed on some coarser "is it transferring" notion instead, an
        // upload turning into a download would keep the old arrow on screen —
        // the tray would be stale in exactly the case the arrows exist for.
        let line = "Photos — Transferring · 40 MB";
        let mut memo = SyncMemo {
            glyph: SyncGlyphId::of(sync_glyph(TraySyncState::Uploading).expect("up")),
            line: Arc::from(line),
        };
        for state in [
            TraySyncState::Downloading,
            TraySyncState::Transferring,
            TraySyncState::Uploading,
        ] {
            let glyph = SyncGlyphId::of(sync_glyph(state).expect("a glyph"));
            assert!(
                sync_writes(Some(&memo), true, glyph, line).icon,
                "{state:?} was skipped, so the tray kept the previous direction"
            );
            memo.glyph = glyph;
        }

        // And the converse, which is the reason the memo exists: an unchanged
        // direction on the next tick must not touch the OS.
        let same = SyncGlyphId::of(sync_glyph(TraySyncState::Uploading).expect("up"));
        assert!(!sync_writes(Some(&memo), true, same, line).icon);
    }

    #[test]
    fn a_rebuilt_tray_is_painted_again() {
        // The memo lives in the tray slot, so toggling presence off→on drops it
        // with the tray: the fresh menu-bar item carries the idle mark and must
        // be painted even though the composed state never changed.
        let glyph = SyncGlyphId::of(sync_glyph(TraySyncState::Active).expect("active"));
        let line = "Photos — Committing — 1/3 files";
        assert_eq!(sync_writes(None, true, glyph, line), SyncWrites::BOTH);
        let memo = SyncMemo {
            glyph,
            line: Arc::from(line),
        };

        // And a menu the recording renderer displaced is reinstalled even when
        // the words are the ones already memoised.
        let restored = sync_writes(Some(&memo), false, glyph, line);
        assert!(restored.text, "a displaced sync menu must be reinstalled");
        assert!(!restored.icon, "its glyph was never taken down");
    }
}
