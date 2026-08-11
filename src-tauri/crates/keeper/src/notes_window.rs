//! The quick-capture windows (FR-101, FR-191, FR-192, NFR-27, AD-60, UX-DR77).
//!
//! # One prewarmed window, and as many more as the user asks for
//!
//! The **draft** window is declared statically in `tauri.conf.json`, created
//! hidden at startup and never destroyed. That is the whole of NFR-27: the
//! hotkey path is `is_resizable` → `set_position` → `show` → `set_focus` — at
//! most four synchronous calls plus one compositor frame, and no settings read
//! — with no webview construction, no bundle load, no React mount and no IPC
//! round trip before the panel is visible.
//! Because the window never unmounts, its editor stays focused while hidden, so
//! `show()` reveals an already-focused live DOM node rather than racing an
//! effect, and a keystroke typed 50 ms after the hotkey has nowhere to go but
//! the buffer. Stories 45.15 and 47.5 change none of that: 47.5's addition is
//! the `is_resizable` read, a window attribute rather than a query, and it
//! *removes* the `set_position` for an unlocked window.
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
//! Show, hide, create, destroy, position, size and focus are all here. The
//! capture webview asks for exactly three window things — hide, close and, when
//! it is unlocked, start-dragging — and its capability file grants those and
//! nothing more. **Resizing adds no fourth**: it is a native edge drag against
//! a window attribute this module sets, not a plugin command the webview
//! invokes, so `quick-capture.json` is unchanged by Story 46.15.
//!
//! Positioning is monitor-aware and **best-effort by design**: an undecorated
//! always-on-top window cannot place itself on Wayland, so keeper asks and
//! accepts the compositor's answer rather than fighting it (UX-DR43). Nothing
//! in the UI promises a position it cannot deliver.
//!
//! **Story 45.15's lock is inside that rule, not an exception to it**, and the
//! distinction is worth keeping straight because it is the one this module's
//! predecessor stated and a lock icon could very easily have broken. What the
//! lock promises is **movability**, and since Story 46.15 **resizability**:
//! unlocked, the strip becomes a `data-tauri-drag-region` and the *compositor*
//! moves the window, and the window becomes `set_resizable(true)` so its edges
//! answer a drag. Both work everywhere including Wayland, and for the same
//! reason — neither is a request to put a surface at a coordinate.
//!
//! The **size** survives a restart: adopted once at boot by [`adopt_placement`]
//! and never re-asserted on the hot path, so nothing undoes it.
//!
//! Since Story 47.5 the **position** survives one the same way (DW-198), and
//! the two halves are both needed: [`adopt_position`] puts an unlocked window
//! back at boot, and [`reveal`] stops re-centring it on every later hotkey
//! press. Before that, keeper remembered how big you made the panel and threw
//! away where you put it.
//!
//! **The restore is attempted and still not promised, and that gap is
//! deliberate.** Applying a stored position is `set_position`, which is exactly
//! the call UX-DR43 says a compositor may refuse, so the controls stay worded
//! for what the platform can deliver ("so it can be moved and resized", "where
//! it is") and never "remembers". A compositor that declines leaves a window
//! the person can still put wherever they like, every time, rather than a
//! promise that quietly fails. Which of the two behaviours a person gets is
//! decided by the lock and nothing else — locked follows the pointer between
//! monitors, unlocked stays put — because the lock is already the control that
//! asks that question.
//!
//! # The window's own resize border sits over the chrome
//!
//! On its GTK backend tao hit-tests an undecorated window's resize edges
//! *inside* the surface — `scale_factor() * 5` logical pixels along each edge,
//! guarded by `is_resizable() && !is_maximized()` — and the webview never sees
//! a click that lands there. The capture chrome's close button is flush into
//! the top-right corner, where two of those strips overlap, so an unlocked GTK
//! window turns part of that button into a resize handle (DW-199).
//!
//! [`edge_inset`] is the one place that number is worked out, and it is worked
//! out here rather than in the webview because the webview reads the platform
//! nowhere. The chrome renders an inset it is handed and still knows nothing
//! about GTK.

use std::collections::BTreeMap;
use std::sync::{LazyLock, Mutex};

use keeper_core::capture::{
    capture_label, chrome_edge_inset, is_capture_label, plan_show_position, CaptureTargetVm,
    CaptureWindowVm, EdgeResize, Placement, ShowPosition, CAPTURE_DEFAULT_SIZE, CAPTURE_MIN_SIZE,
    DRAFT_CAPTURE_KEY, DRAFT_CAPTURE_LABEL,
};
use tauri::{
    AppHandle, Emitter, LogicalSize, Manager, Monitor, PhysicalPosition, Runtime, WebviewUrl,
    WebviewWindow, WebviewWindowBuilder,
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

/// A capture window's two sizes — keeper's own [`CAPTURE_DEFAULT_SIZE`] and the
/// floor [`CAPTURE_MIN_SIZE`] a user may resize one to — live in
/// `keeper_core::capture`, not here, and they are the same numbers
/// `tauri.conf.json` gives the prewarmed window, so the second window is the
/// same window and not a differently sized cousin. They are over there because
/// [`Placement::window_size`] has to answer with them, and that answer is the
/// one part of a capture window's geometry that can be tested on a machine
/// where this crate does not compile (AD-55/AD-56).
///
/// This is the only thing left on this side: they are stored as plain integers
/// of logical pixels, and every Tauri sizing call wants a typed `f64` pair.
fn logical(size: (u32, u32)) -> LogicalSize<f64> {
    LogicalSize::new(f64::from(size.0), f64::from(size.1))
}

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

/// Show and focus the draft panel — the hotkey and tray path
/// (`notes_capture_show`).
///
/// Kept as a no-argument entry point because its two callers are an OS shortcut
/// handler and a tray menu handler, neither of which has a placement to hand
/// in. A *remembered* placement arrives through [`open`], which the frontend
/// calls with one read from the settings table.
///
/// Since Story 47.5 it places the panel only when the panel is keeper's to
/// place — see [`reveal`] for the whole of that decision and why it costs the
/// hot path no settings read.
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
                .inner_size(CAPTURE_DEFAULT_SIZE.0.into(), CAPTURE_DEFAULT_SIZE.1.into())
                // The floor, set on the window itself rather than only in the
                // clamp: a `min_inner_size` is refused by the compositor
                // mid-drag, so the user never gets to a size keeper would then
                // have to argue with on the next open.
                .min_inner_size(CAPTURE_MIN_SIZE.0.into(), CAPTURE_MIN_SIZE.1.into())
                .decorations(false)
                .always_on_top(true)
                .skip_taskbar(true)
                // Story 46.15: resizability follows the lock, from the first
                // frame. Built `false` and flipped below would give an unlocked
                // window one frame in which its edges do not answer.
                .resizable(!placement.locked)
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
    // The size is NOT re-asserted unconditionally here any more (Story 46.15).
    // A locked window is still normalised — a compositor may have resized it,
    // and every locked capture window is the same window — but an unlocked one
    // is left at the size the user gave it, which is the whole of the feature.
    // Which of those two this is, is `Placement::window_size`'s decision, made
    // in `keeper_core` where it is tested.
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
/// Returns the window's last geometry so the caller can persist it — its
/// position and, since Story 46.15, the size the user resized it to. Read
/// *before* the window goes away, because a destroyed window has no geometry to
/// ask for and a hidden one may report the origin.
pub fn close(app: &AppHandle, key: &str) -> Geometry {
    let Some(window) = window(app, key) else {
        return Geometry::default();
    };
    let geometry = geometry(&window);
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
    geometry
}

/// What a capture window's geometry is worth remembering, in the units each
/// half is remembered in (see [`Placement`]): the position physical, the size
/// logical.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Geometry {
    /// The top-left in physical pixels, or `None` when the platform will not
    /// say.
    pub position: Option<(i32, i32)>,
    /// The inner size in logical pixels, or `None` when the platform will not
    /// say.
    pub size: Option<(u32, u32)>,
}

/// Where the capture window for `key` is and how big it is right now, or an
/// empty [`Geometry`] when it is not open or the platform will not say.
///
/// Read at dismissal and on blur rather than on every `Moved`/`Resized` event:
/// a drag emits one event per compositor frame and a settings write per frame
/// would put a sqlite transaction inside a gesture.
pub fn geometry_of(app: &AppHandle, key: &str) -> Geometry {
    let Some(window) = window(app, key) else {
        return Geometry::default();
    };
    geometry(&window)
}

/// [`geometry_of`] against a window already in hand.
fn geometry<R: Runtime>(window: &WebviewWindow<R>) -> Geometry {
    Geometry {
        position: window
            .outer_position()
            .ok()
            .map(|position| (position.x, position.y)),
        // `inner_size`, not `outer_size`: `set_size` sets the inner size, so
        // reading the outer one would grow the window by whatever frame the
        // platform draws on every save-and-restore cycle. On an undecorated
        // window the two are usually equal — "usually" is not a contract.
        size: window.scale_factor().ok().and_then(|scale| {
            window
                .inner_size()
                .ok()
                .map(|size| size.to_logical::<u32>(scale))
                .map(|size| (size.width, size.height))
        }),
    }
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
                chrome_inset: edge_inset(&window),
            })
        })
        .collect();
    // Stable order: the list renders as a list, and a set iterated in hash
    // order would reshuffle itself every time anything changed.
    windows.sort_by(|a, b| a.key.cmp(&b.key));
    windows
}

/// How much room this window's own resize border needs on the chrome strip
/// (Story 47.5, DW-199).
///
/// **This function is the platform test, and it is here so that no other one
/// exists.** The webview reads the platform nowhere — `src/test/
/// no-user-agent-gating.test.ts` enforces that — so the chrome cannot decide
/// its own inset from a user agent, and it must not: the number is
/// `scale_factor() * 5`, and a CSS constant of 5 would be exactly half the
/// border on a 2× display and a phantom gap in the two states where tao does
/// not hit-test at all. The webview gets a number and still knows nothing about
/// GTK. [`chrome_edge_inset`] holds the arithmetic and its four states are
/// asserted in `keeper-core`, on a machine this crate does not build on.
///
/// `inside_client_area` is a compile-time fact and never a runtime probe: tao
/// hit-tests inside the surface only on its GTK backend.
///
/// Every read is best-effort in the direction that keeps the control clickable:
/// a window that will not say whether it is resizable is treated as locked (no
/// border to dodge, so no gap over nothing), and one that will not name a scale
/// factor gets one border's worth rather than none.
fn edge_inset<R: Runtime>(window: &WebviewWindow<R>) -> u32 {
    chrome_edge_inset(EdgeResize {
        inside_client_area: cfg!(all(unix, not(target_os = "macos"))),
        resizable: window.is_resizable().unwrap_or(false),
        maximized: window.is_maximized().unwrap_or(false),
        // Rounded UP, and through `f64`, because under-insetting leaves part of
        // the close button on a resize handle and over-insetting costs a pixel
        // of padding. GTK's own scale factor is an integer, so on the platform
        // this number is for the rounding never fires.
        scale: window.scale_factor().unwrap_or(1.0).ceil().max(1.0) as u32,
    })
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

/// Place, size, show and focus a window, then tell it so.
///
/// `placement` is `None` for the hotkey and tray path, which has no settings
/// read in front of it (NFR-27) and therefore knows nothing about this window's
/// remembered geometry. That is not the same as "the default placement": a
/// caller with nothing to say must **change nothing but the position**, because
/// resizability and size were already set — at boot by [`adopt_placement`], and
/// on every toggle by the lock command — and re-asserting a default over them
/// would silently relock and shrink a window the user unlocked and resized.
///
/// **And since Story 47.5 it may change nothing at all** (DW-198). A caller
/// with nothing to say used to re-centre the panel unconditionally, so the size
/// a person chose survived every hotkey press and the position they chose
/// survived none: "it remembers how big I made it but not where I put it". The
/// answer is the lock's, because the lock is already the setting that asks this
/// question — locked follows the pointer, unlocked stays put — and it is read
/// off `is_resizable()`, the very attribute [`apply_resizability`] writes at
/// boot and on every toggle. One window-attribute read, no sqlite, so the hot
/// path is still three synchronous calls (NFR-27). A window that will not say
/// is placed, which is exactly what it did before this change.
fn reveal<R: Runtime>(
    app: &AppHandle<R>,
    window: &WebviewWindow<R>,
    key: &str,
    placement: Option<Placement>,
) {
    match placement {
        Some(placement) => apply_placement(window, placement),
        None => {
            let unlocked = window.is_resizable().unwrap_or(false);
            if plan_show_position(unlocked) == ShowPosition::Place {
                position(window);
            }
        }
    }
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

/// Give the capture window for `key` the resizability and size its stored
/// placement asks for, without showing it or moving it.
///
/// Two callers, one act. **At boot**, for the prewarmed window: it is declared
/// `resizable: false` in `tauri.conf.json` and created before anything has read
/// the settings, so without this a person who unlocked it yesterday finds it
/// unlocked-looking and unresizable today — the lock reduced to a label, which
/// is the exact failure Story 46.15 exists to fix. Done here, once, rather than
/// in [`show`]: the hotkey path is three synchronous calls and a settings read
/// does not belong in front of it (NFR-27).
///
/// **On the lock toggle**, against the live window, so unlocking takes effect
/// without a reopen.
pub fn adopt_placement<R: Runtime>(app: &AppHandle<R>, key: &str, placement: Placement) {
    let Some(window) = window(app, key) else {
        return;
    };
    apply_resizability(&window, placement);
    apply_size(&window, placement);
}

/// Put the prewarmed panel back where its stored placement says, once, at boot
/// (Story 47.5, DW-198).
///
/// **Separate from [`adopt_placement`] because it has one caller and that is the
/// point.** `adopt_placement` also runs on every lock toggle, and applying a
/// stored position there would make *unlocking* teleport a window the person is
/// looking at: the panel is wherever the last hotkey press put it, the row
/// holds wherever they dragged it to before they locked it, and a click on a
/// padlock is not a request to move a window.
///
/// Best-effort exactly as [`apply_placement`]'s position arm is: `set_position`
/// is the one call UX-DR43 says a compositor may refuse, so a refusal is logged
/// at debug and the person keeps a window they can still put where they like.
/// [`Placement::adopted_position`] decides *whether* there is anything to ask
/// for — this converts units and asks.
pub fn adopt_position<R: Runtime>(app: &AppHandle<R>, key: &str, placement: Placement) {
    let Some((x, y)) = placement.adopted_position() else {
        return;
    };
    let Some(window) = window(app, key) else {
        return;
    };
    if let Err(error) = window.set_position(PhysicalPosition::new(x, y)) {
        tracing::debug!(%error, %key, "notes: the compositor placed the capture panel itself");
    }
}

/// Put a window where its placement says — or where keeper would put it — at
/// the size its placement says, and let the user move and resize it or not.
///
/// Order is load-bearing. Resizability first, because a platform may refuse a
/// size outside a non-resizable window's constraints. Size before position,
/// because [`position`] centres the window against its own measured width: swap
/// the two and a window that was just resized is centred as the size it used to
/// be.
fn apply_placement<R: Runtime>(window: &WebviewWindow<R>, placement: Placement) {
    apply_resizability(window, placement);
    apply_size(window, placement);
    match placement.position {
        Some((x, y)) => {
            if let Err(error) = window.set_position(PhysicalPosition::new(x, y)) {
                tracing::debug!(%error, "notes: the compositor placed the capture panel itself");
            }
        }
        None => position(window),
    }
}

/// The lock's second verb (Story 46.15). Unlocked means movable **and**
/// resizable; locked means neither.
///
/// This one is not inside UX-DR43's best-effort rule, and the distinction is
/// worth keeping straight because the module doc spends a paragraph on the
/// other half. What Wayland refuses is `set_position` — a request to put a
/// surface at a coordinate, which a compositor owns. Resizability is a window
/// *attribute*: it is `gtk_window_set_resizable` on GTK, a style mask on macOS
/// and a window style on Windows, and the edge-drag hit-testing every backend
/// does for an undecorated window re-reads that attribute per event rather than
/// capturing it at creation. So the lock can promise this one everywhere.
fn apply_resizability<R: Runtime>(window: &WebviewWindow<R>, placement: Placement) {
    if let Err(error) = window.set_resizable(!placement.locked) {
        tracing::warn!(
            %error,
            locked = placement.locked,
            "notes: could not set the capture panel's resizability"
        );
    }
}

/// Size a window from its placement, or leave it exactly as it is.
///
/// Every part of that decision — normalise a locked window, restore an unlocked
/// one, touch neither when nothing is remembered, and never restore a size the
/// screen cannot show — belongs to [`Placement::window_size`], which lives in
/// `keeper_core` and is tested there. This function converts units and calls it.
fn apply_size<R: Runtime>(window: &WebviewWindow<R>, placement: Placement) {
    let Some(size) = placement.window_size(logical_work_area(window)) else {
        return;
    };
    if let Err(error) = window.set_size(logical(size)) {
        tracing::debug!(%error, "notes: the compositor sized the capture panel itself");
    }
}

/// The usable area of the monitor this panel belongs on, in **logical** pixels,
/// or `None` when the platform will not name a monitor.
///
/// Logical because that is the unit a remembered size is stored in, and mixing
/// the two here is the bug this function exists to make impossible: clamping a
/// logical 900 against a physical 2880 on a 2× display would silently allow a
/// window twice as wide as the screen.
fn logical_work_area<R: Runtime>(window: &WebviewWindow<R>) -> Option<(u32, u32)> {
    let monitor = focused_monitor(window)?;
    let area = monitor
        .work_area()
        .size
        .to_logical::<u32>(monitor.scale_factor());
    Some((area.width, area.height))
}

/// The monitor a capture panel belongs on: the one under the pointer, else the
/// one the window is already on, else the primary one.
///
/// The **focused** monitor, not the primary one: on a two-screen desk the panel
/// has to appear where the user is looking. `current_monitor` reports the
/// monitor the window is on, which for a hidden window is where it was last
/// placed, so the cursor's monitor is preferred when the platform can name it.
fn focused_monitor<R: Runtime>(window: &WebviewWindow<R>) -> Option<Monitor> {
    let monitor = match window.cursor_position() {
        Ok(cursor) => window
            .monitor_from_point(cursor.x, cursor.y)
            .ok()
            .flatten()
            .or_else(|| window.current_monitor().ok().flatten()),
        Err(_) => window.current_monitor().ok().flatten(),
    };
    monitor.or_else(|| window.primary_monitor().ok().flatten())
}

/// Place the panel horizontally centred on the focused monitor, a fifth of the
/// way down its work area.
fn position<R: Runtime>(window: &WebviewWindow<R>) {
    let Some(monitor) = focused_monitor(window) else {
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
    // may actually occupy. Physical throughout, matching `outer_size` and the
    // physical coordinate `set_position` takes.
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
