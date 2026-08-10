//! The quick-capture windows (FR-101, FR-191, FR-192, NFR-27, AD-60, UX-DR77).
//!
//! # One prewarmed window, and as many more as the user asks for
//!
//! The **draft** window is declared statically in `tauri.conf.json`, created
//! hidden at startup and never destroyed. That is the whole of NFR-27: the
//! hotkey path is `set_position` → `show` → `set_focus` — three synchronous
//! calls plus one compositor frame — with no webview construction, no bundle
//! load, no React mount and no IPC round trip before the panel is visible.
//! Because the window never unmounts, its editor stays focused while hidden, so
//! `show()` reveals an already-focused live DOM node rather than racing an
//! effect, and a keystroke typed 50 ms after the hotkey has nowhere to go but
//! the buffer. Story 45.15 changes none of that.
//!
//! Every **other** capture window is created on demand, one per note, labelled
//! by [`keeper_core::capture::capture_label`]. They are ordinary windows: they
//! cost a webview each and they are destroyed when closed. Only the draft one
//! is prewarmed, because only the draft one is on the path a hotkey has 300 ms
//! to travel.
//!
//! # What is per-window, and why a map
//!
//! A label is a hash of a capture key, so it cannot be read backwards. The
//! process therefore keeps [`OPEN`]: label → target, written when a window is
//! created and erased when it is destroyed. It is process state and nothing
//! else — it is not persistence, it is not consulted to decide whether a window
//! exists (`get_webview_window` answers that, and it cannot go stale), and it
//! is rebuilt from nothing on the next launch. Its one job is to answer "what is
//! this window holding?" for the list the main window renders.
//!
//! # Rust owns geometry, the webview owns chrome
//!
//! Show, hide, create, destroy, position and focus are all here. The capture
//! webview asks for exactly three window things — hide, close and, when it is
//! unlocked, start-dragging — and its capability file grants those and nothing
//! more.
//!
//! Positioning is monitor-aware and **best-effort by design**: an undecorated
//! always-on-top window cannot place itself on Wayland, so keeper asks and
//! accepts the compositor's answer rather than fighting it (UX-DR43). Nothing
//! in the UI promises a position it cannot deliver.
//!
//! **Story 45.15's lock is inside that rule, not an exception to it**, and the
//! distinction is worth keeping straight because it is the one this module's
//! predecessor stated and a lock icon could very easily have broken. What the
//! lock promises is **movability**: unlocked, the strip becomes a
//! `data-tauri-drag-region` and the *compositor* moves the window, which works
//! everywhere including Wayland. What it does not promise is the restore —
//! putting the window back there next time is a `set_position`, which is
//! exactly the call UX-DR43 says may be refused. So the controls are worded for
//! what they can deliver ("so it can be moved", "where it is") and never
//! "remembers", and a compositor that declines leaves a window the person can
//! still put wherever they like, every time, rather than a promise that quietly
//! fails.

use std::collections::BTreeMap;
use std::sync::{LazyLock, Mutex};

use keeper_core::capture::{
    capture_label, is_capture_label, CaptureTargetVm, CaptureWindowVm, Placement,
    DRAFT_CAPTURE_KEY, DRAFT_CAPTURE_LABEL,
};
use tauri::{
    AppHandle, Emitter, LogicalSize, Manager, PhysicalPosition, Runtime, WebviewUrl, WebviewWindow,
    WebviewWindowBuilder,
};

/// Emitted to **one** capture window after it is shown, so that window's entry
/// point can re-assert focus after a compositor race on Linux.
///
/// Emitted with `emit_to` rather than `emit`, and that is a Story 45.15
/// correction rather than a preference: an app-wide emit told *every* capture
/// window to focus itself whenever *any* one of them was shown, so raising the
/// second window would have yanked focus back and forth between all of them.
/// With one window that bug was unobservable, which is exactly why it survived.
pub const CAPTURE_SHOWN_EVENT: &str = "keeper://notes-capture-shown";

/// Emitted app-wide when the set of capture windows changes, so the main
/// window's list stops claiming a window that has gone. Carries no payload —
/// ids only, per the `keeper://kebab-case` convention — and the listener asks
/// for the list.
pub const CAPTURE_WINDOWS_EVENT: &str = "keeper://notes-capture-windows";

/// The capture window's document. Query-less for the draft window, so its URL
/// is byte-for-byte what `tauri.conf.json` declares.
const CAPTURE_DOCUMENT: &str = "capture.html";

/// Width and height of a capture window, in logical pixels — the same numbers
/// `tauri.conf.json` gives the draft window, so the second window is the same
/// window and not a differently sized cousin.
const CAPTURE_SIZE: (f64, f64) = (560.0, 340.0);

/// How far down the focused monitor's work area an *unplaced* panel sits, as a
/// fraction of the monitor height.
///
/// A fifth of the way down rather than centred: a capture panel is a thing you
/// type into and dismiss, and vertical centring puts it exactly where a
/// person's eyes are already busy with whatever they were reading.
const TOP_FRACTION: f64 = 0.2;

/// Label → what that window is holding. Process state; see the module doc.
///
/// Seeded with the draft window rather than filled in at startup, because that
/// window is declared in `tauri.conf.json` and never passes through [`open`].
/// Seeding it here means there is no ordering to get wrong: the map answers for
/// the prewarmed window from the first read, including one that happens before
/// `setup` has run.
static OPEN: LazyLock<Mutex<BTreeMap<String, CaptureTargetVm>>> = LazyLock::new(|| {
    Mutex::new(BTreeMap::from([(
        DRAFT_CAPTURE_LABEL.to_owned(),
        CaptureTargetVm::Draft,
    )]))
});

/// The window for a capture key, or `None` when it does not exist.
///
/// For the draft key `None` means the static declaration and this module have
/// drifted — logged loudly, because a capture path that silently does nothing
/// is the failure mode that looks like a frontend bug and is not. For any other
/// key `None` is ordinary: the window has not been opened yet.
fn window<R: Runtime>(app: &AppHandle<R>, key: &str) -> Option<WebviewWindow<R>> {
    let label = capture_label(key);
    let window = app.get_webview_window(&label);
    if window.is_none() && key == DRAFT_CAPTURE_KEY {
        tracing::warn!(
            label = %label,
            "notes: the quick-capture window is not declared; capture is unavailable"
        );
    }
    window
}

/// Whether the draft panel is currently visible. Read by the hotkey, which
/// toggles.
pub fn is_visible<R: Runtime>(app: &AppHandle<R>) -> bool {
    window(app, DRAFT_CAPTURE_KEY).is_some_and(|window| window.is_visible().unwrap_or(false))
}

/// Position, show and focus the draft panel — the hotkey and tray path
/// (`notes_capture_show`).
///
/// Kept as a no-argument entry point because its two callers are an OS shortcut
/// handler and a tray menu handler, neither of which has a placement to hand
/// in. It shows the panel where keeper last knew to put it; a *remembered*
/// placement arrives through [`open`], which the frontend calls with one read
/// from the settings table.
pub fn show<R: Runtime>(app: &AppHandle<R>) {
    let Some(window) = window(app, DRAFT_CAPTURE_KEY) else {
        return;
    };
    reveal(app, &window, DRAFT_CAPTURE_KEY, None);
}

/// Hide the draft panel (`notes_capture_hide`). Never destroys it — the
/// window's whole value is that it already exists.
pub fn hide<R: Runtime>(app: &AppHandle<R>) {
    let Some(window) = window(app, DRAFT_CAPTURE_KEY) else {
        return;
    };
    if let Err(error) = window.hide() {
        tracing::warn!(%error, "notes: could not hide the capture panel");
    }
}

/// Show the capture window for `target`, creating it if it does not exist
/// (FR-191).
///
/// Idempotent by identity rather than by a flag: a second call for the same
/// target finds the same label and raises the window that is already there, so
/// "open this note as a capture window" twice is one window with focus and not
/// two windows with one note.
///
/// `placement` is the remembered one, read by the caller — this module does no
/// storage, because the shell does not compile on every machine and a placement
/// rule nobody can build is a placement rule nobody can check (AD-55/AD-56).
pub fn open(app: &AppHandle, target: &CaptureTargetVm, placement: Placement) {
    let key = keeper_core::capture::capture_key(target);
    let label = capture_label(&key);
    remember_target(&label, target);
    let existing = app.get_webview_window(&label);
    let window = match existing {
        Some(window) => window,
        None => {
            if key == DRAFT_CAPTURE_KEY {
                // The static declaration and this module have drifted.
                // Deliberately NOT rebuilt here: a replacement would paper over
                // a config error and quietly cost NFR-27 its prewarm, so every
                // later capture would pay a webview construction that nobody
                // could account for.
                tracing::warn!(
                    label = %label,
                    "notes: the quick-capture window is not declared; capture is unavailable"
                );
                return;
            }
            let url = format!(
                "{CAPTURE_DOCUMENT}{}",
                keeper_core::capture::capture_search(target)
            );
            let built = WebviewWindowBuilder::new(app, &label, WebviewUrl::App(url.into()))
                .title("keeper — quick note")
                .inner_size(CAPTURE_SIZE.0, CAPTURE_SIZE.1)
                .decorations(false)
                .always_on_top(true)
                .skip_taskbar(true)
                .resizable(false)
                .visible(false)
                .build();
            match built {
                Ok(window) => window,
                Err(error) => {
                    forget_target(&label);
                    tracing::warn!(%error, %label, "notes: could not create a capture window");
                    return;
                }
            }
        }
    };
    // Belt and braces on a window that already existed: a compositor may have
    // resized it, and every capture window is the same window.
    let _ = window.set_size(LogicalSize::new(CAPTURE_SIZE.0, CAPTURE_SIZE.1));
    reveal(app, &window, &key, Some(placement));
    announce(app);
}

/// Close the capture window for `key` (FR-191).
///
/// Follows [`keeper_core::capture::plan_close`], which decides the two things
/// that are not obvious: the draft window is hidden rather than destroyed, and
/// closing the last visible window raises the main one so the app is never
/// running with nothing on screen and no taskbar entry.
///
/// Returns the window's last position so the caller can persist it. Read
/// *before* the window goes away, because a destroyed window has no geometry to
/// ask for and a hidden one may report the origin.
pub fn close(app: &AppHandle, key: &str) -> Option<(i32, i32)> {
    let Some(window) = window(app, key) else {
        return None;
    };
    let position = window
        .outer_position()
        .ok()
        .map(|position| (position.x, position.y));
    let plan = keeper_core::capture::plan_close(key, other_windows_visible(app, key));
    if plan.destroy {
        forget_target(&capture_label(key));
        if let Err(error) = window.destroy() {
            tracing::warn!(%error, %key, "notes: could not close the capture window");
        }
    } else if let Err(error) = window.hide() {
        tracing::warn!(%error, %key, "notes: could not hide the capture window");
    }
    if plan.raise_main {
        crate::tray::show_main_window(app);
    }
    announce(app);
    position
}

/// Where the capture window for `key` is right now, or `None` when it is not
/// open or the platform will not say.
///
/// Read at dismissal rather than on every `Moved` event: a drag emits one event
/// per compositor frame and a settings write per frame would put a sqlite
/// transaction inside a gesture.
pub fn position_of(app: &AppHandle, key: &str) -> Option<(i32, i32)> {
    window(app, key)?
        .outer_position()
        .ok()
        .map(|position| (position.x, position.y))
}

/// Every capture window that exists right now (FR-191).
///
/// Built from the live window list rather than from [`OPEN`], so a window
/// destroyed by the OS cannot linger in the answer; [`OPEN`] supplies only the
/// target, which a label cannot be read backwards into.
pub fn list(app: &AppHandle, locked: &dyn Fn(&str) -> bool) -> Vec<CaptureWindowVm> {
    let open = OPEN.lock().map(|open| open.clone()).unwrap_or_default();
    let mut windows: Vec<CaptureWindowVm> = app
        .webview_windows()
        .into_iter()
        .filter(|(label, _)| is_capture_label(label))
        .filter_map(|(label, window)| {
            // A capture-shaped label this process did not open is SKIPPED, not
            // guessed. Defaulting it to the draft target would put a second row
            // called `draft` in the list, and every reader keys on that string.
            let target = open.get(&label)?.clone();
            let key = keeper_core::capture::capture_key(&target);
            Some(CaptureWindowVm {
                locked: locked(&key),
                key,
                target,
                visible: window.is_visible().unwrap_or(false),
            })
        })
        .collect();
    // Stable order: the list renders as a list, and a set iterated in hash
    // order would reshuffle itself every time anything changed.
    windows.sort_by(|a, b| a.key.cmp(&b.key));
    windows
}

/// Tell every window that the set of capture windows changed.
pub fn announce(app: &AppHandle) {
    if let Err(error) = app.emit(CAPTURE_WINDOWS_EVENT, ()) {
        tracing::warn!(%error, "notes: could not emit the capture-windows event");
    }
}

/// The capture key of the window called `label`, or `None` when this process
/// did not open it.
///
/// A label is a hash of a key and cannot be read backwards, so this is the only
/// direction available — which is why the window-event handler asks here rather
/// than deriving. `None` for the draft window is impossible in practice (it is
/// registered at startup) but is answered honestly rather than guessed, because
/// guessing `draft` for an unknown capture label would write one window's
/// position onto another's row.
pub fn key_for_label(label: &str) -> Option<String> {
    OPEN.lock()
        .ok()?
        .get(label)
        .map(keeper_core::capture::capture_key)
}

/// Register a target against its label so [`list`] can name it.
fn remember_target(label: &str, target: &CaptureTargetVm) {
    if let Ok(mut open) = OPEN.lock() {
        open.insert(label.to_owned(), target.clone());
    }
}

/// Forget a destroyed window's target.
fn forget_target(label: &str) {
    if let Ok(mut open) = OPEN.lock() {
        open.remove(label);
    }
}

/// Whether any window other than the capture window for `key` is on screen.
///
/// "Other than" is the load-bearing half: asked without it, a window that is
/// still visible at the moment it is being closed answers "yes, something is
/// visible" about itself, and the main window is never raised.
fn other_windows_visible(app: &AppHandle, key: &str) -> bool {
    let closing = capture_label(key);
    app.webview_windows()
        .into_iter()
        .any(|(label, window)| label != closing && window.is_visible().unwrap_or(false))
}

/// Place, show and focus a window, then tell it so.
fn reveal<R: Runtime>(
    app: &AppHandle<R>,
    window: &WebviewWindow<R>,
    key: &str,
    placement: Option<Placement>,
) {
    apply_placement(window, placement.unwrap_or_default());
    if let Err(error) = window.show() {
        tracing::warn!(%error, %key, "notes: could not show the capture panel");
        return;
    }
    // `set_focus` after `show`: focusing a hidden window is a no-op on every
    // backend, and the panel exists to receive a keystroke.
    if let Err(error) = window.set_focus() {
        tracing::warn!(%error, %key, "notes: could not focus the capture panel");
    }
    if let Err(error) = app.emit_to(window.label(), CAPTURE_SHOWN_EVENT, ()) {
        tracing::warn!(%error, %key, "notes: could not emit the capture-shown event");
    }
}

/// Put a window where its placement says, or where keeper would put it.
fn apply_placement<R: Runtime>(window: &WebviewWindow<R>, placement: Placement) {
    // A locked window is not movable by the user, so it is `resizable(false)`
    // plus no drag region in the webview; there is no platform flag to set
    // here. What the lock changes on this side is only whether keeper is
    // allowed to move the window out from under a position the user chose.
    match placement.position {
        Some((x, y)) => {
            if let Err(error) = window.set_position(PhysicalPosition::new(x, y)) {
                tracing::debug!(%error, "notes: the compositor placed the capture panel itself");
            }
        }
        None => position(window),
    }
}

/// Place the panel horizontally centred on the focused monitor, a fifth of the
/// way down its work area.
///
/// The **focused** monitor, not the primary one: on a two-screen desk the panel
/// has to appear where the user is looking. `current_monitor` reports the
/// monitor the window is on, which for a hidden window is where it was last
/// placed, so the cursor's monitor is preferred when the platform can name it.
fn position<R: Runtime>(window: &WebviewWindow<R>) {
    let monitor = match window.cursor_position() {
        Ok(cursor) => window
            .monitor_from_point(cursor.x, cursor.y)
            .ok()
            .flatten()
            .or_else(|| window.current_monitor().ok().flatten()),
        Err(_) => window.current_monitor().ok().flatten(),
    };
    let Some(monitor) = monitor.or_else(|| window.primary_monitor().ok().flatten()) else {
        // No monitor information at all (a headless session, or a compositor that
        // does not answer). `center: true` in the static config is the fallback,
        // and it is a perfectly good answer.
        tracing::debug!("notes: no monitor geometry; leaving the capture panel where it is");
        return;
    };
    let Ok(size) = window.outer_size() else {
        return;
    };
    // The work area, not the raw resolution: it excludes the macOS menu bar and
    // a Linux panel, so the offset below is measured against the space a window
    // may actually occupy.
    let area = monitor.work_area();
    let x = area.position.x + centred(area.size.width, size.width);
    let y = area.position.y + offset_from_top(area.size.height, size.height);
    if let Err(error) = window.set_position(PhysicalPosition::new(x, y)) {
        // Wayland refuses this, and that is not a fault: the compositor places
        // the panel and everything else about it is unchanged (UX-DR43).
        tracing::debug!(%error, "notes: the compositor placed the capture panel itself");
    }
}

/// The left edge that centres `window_width` inside `area_width`.
///
/// Saturating and signed, because a panel wider than the monitor — a 560 px panel
/// on a tiny virtual display — must land at the left edge rather than at a
/// negative coordinate that some backends reject outright.
fn centred(area_width: u32, window_width: u32) -> i32 {
    let free = area_width.saturating_sub(window_width);
    i32::try_from(free / 2).unwrap_or(0)
}

/// The top edge, [`TOP_FRACTION`] of the way down, clamped so the panel is always
/// fully on screen.
fn offset_from_top(area_height: u32, window_height: u32) -> i32 {
    let free = area_height.saturating_sub(window_height);
    #[allow(clippy::cast_possible_truncation, clippy::cast_precision_loss)]
    let wanted = (f64::from(area_height) * TOP_FRACTION) as u32;
    i32::try_from(wanted.min(free)).unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The capability file this window's authority comes from. Read at compile
    /// time so a rename cannot leave the assertion pointing at nothing.
    const CAPABILITY: &str = include_str!("../capabilities/quick-capture.json");

    #[test]
    fn a_panel_is_centred_horizontally_on_its_monitor() {
        assert_eq!(centred(1920, 560), 680);
        assert_eq!(centred(560, 560), 0, "an exact fit sits flush");
    }

    #[test]
    fn a_panel_wider_than_the_monitor_lands_at_the_edge_rather_than_off_it() {
        assert_eq!(
            centred(400, 560),
            0,
            "a negative left edge is refused by some backends"
        );
    }

    #[test]
    fn the_top_offset_is_a_fifth_down_and_never_pushes_the_panel_off_screen() {
        assert_eq!(offset_from_top(1080, 340), 216);
        // A short monitor clamps to the last row that keeps the panel whole
        // rather than placing its title strip below the screen.
        assert_eq!(offset_from_top(400, 340), 60);
        assert_eq!(offset_from_top(340, 340), 0, "an exact fit sits at the top");
        assert_eq!(
            offset_from_top(100, 340),
            0,
            "an impossible fit sits at the top"
        );
    }

    /// The silent failure Story 45.15 could most easily have shipped: a second
    /// capture window whose label the capability does not cover renders
    /// normally and can invoke no window permission at all — it cannot hide,
    /// cannot close, cannot be dragged and cannot follow a link.
    ///
    /// Mirrored in `src/test/capture-capability.test.ts`, which runs on every
    /// machine; this half runs only where the shell compiles.
    #[test]
    fn the_capability_covers_every_label_this_module_can_create() {
        assert!(
            CAPABILITY.contains(&format!("\"{DRAFT_CAPTURE_LABEL}\"")),
            "the prewarmed window's exact label must stay listed"
        );
        assert!(
            CAPABILITY.contains(&format!("\"{}\"", keeper_core::capture::CAPTURE_LABEL_GLOB)),
            "a dynamically created capture window matches nothing without the glob"
        );
    }
}
