//! The voice pill: a floating window that shows keeper hears (Epic 64, Story
//! 64.4, FR-436–FR-439, NFR-54, AD-185, AD-187).
//!
//! # What it is
//!
//! A prewarmed, undecorated, ~220×40 pt window at the top centre of the screen
//! keeper's main window is on, drawn while a voice turn runs: the turn's
//! state, the input level, and the words as they are recognised. It exists so
//! that a person who said the phrase with Maps or a browser in front can see
//! that sound is arriving and words are forming without bringing keeper
//! forward — epic 64's third screenshot was a turn whose only sign was a
//! `text-xs` line in a window that was behind everything.
//!
//! # What it never does
//!
//! It never takes focus and never activates keeper. `focusable(false)`
//! overrides `canBecomeKeyWindow`, `focused(false)` orders it front without
//! making it key, and [`set_ignore_cursor_events`] makes it click-through, so
//! a click on the pill lands on whatever is under it. It is `always_on_top`
//! (`NSFloatingWindowLevel`) and `visible_on_all_workspaces`, which joins
//! every *normal* Space. **It cannot appear over another app's full-screen
//! Space**: that needs an `NSPanel` subclass, which is a git dependency in a
//! signed build, and is deferred with its cost named (AD-187, a DW row). It
//! is **not transparent**: `transparent(true)` needs `macOSPrivateApi`,
//! which keeper does not enable, so the pill is an opaque rectangle with a
//! hairline, not a rounded glass token.
//!
//! # When it exists, and when it is seen
//!
//! Created hidden at boot by [`install`], on the desktop only, and **only
//! when [`crate::voice_reach::present`] is true** — the one runtime answer
//! every reach surface reads (AD-179). A port that answers `unsupported`
//! never creates the window at all, which is AD-27's absence: no webview, no
//! document, nothing to be inert. That is also why the window is built here
//! rather than declared in `tauri.conf.json` beside `quick-capture`: a static
//! declaration is created on every boot of every desktop, including the ones
//! whose port cannot listen.
//!
//! Shown while the turn is anywhere but idle-unarmed, hidden when it returns
//! there. Armed-and-waiting — `idle { listeningForWake: true }` — is worth a
//! glance and not a fixture (AD-185): the pill shows for [`ARMED_GLANCE`]
//! when the turn *arrives* at that state and then hides, and a snapshot that
//! re-states it (a pane remount re-pushes the current state) does not glance
//! again. A failure is the same shape with a longer clock
//! ([`FAILURE_LINGER`]): `Failed` is a state the turn leaves only on the next
//! trigger, and a sentence floating over every app until then would be the
//! fixture AD-185 refuses. [`step`] is the whole of that decision, pure over
//! the previous [`Face`] and the new snapshot, and it is unit-tested here on
//! the gate machine because this crate does not build on Linux (AD-55/AD-56).
//!
//! # How it learns what to draw
//!
//! `voice_ipc::push` hands every snapshot to [`observe`] before it streams
//! it to the pane's channel — a Rust-side fan-out, not a second `voice_watch`
//! registration. The watcher slot is deliberately one deep (a remounted pane
//! replaces its predecessor rather than doubling every snapshot), so a pill
//! that registered through `voice_watch` would evict the pane, and a map of
//! watchers would change `voice_unwatch`'s replace-on-remount semantics for
//! the surface that relies on them. Rust has to see every snapshot anyway to
//! decide show and hide, so the document's feed comes from the same place:
//! an `emit_to` of the snapshot to this one window, under
//! [`VOICE_STATE_EVENT`].
//!
//! # The one rule about threads
//!
//! [`observe`] is called with the voice lock held, from whichever thread
//! moved the turn — the main thread for a sync command, the async runtime
//! for a port callback. Every window *getter* (`is_visible`, `outer_size`,
//! `current_monitor`) is a round trip to the main thread that blocks until
//! it answers, and a sync command runs *on* the main thread while holding
//! that same lock. A getter under the lock from the runtime is therefore a
//! deadlock waiting for its first level reading. So [`observe`] does nothing
//! but hand the snapshot to `run_on_main_thread`, where the getters run
//! directly and the voice lock is never taken; and the pill's own visibility
//! is tracked in [`PILL`] rather than asked of the window.

use std::sync::{Mutex, OnceLock};
use std::time::Duration;

use keeper_core::capture::{clamp_position, WorkArea};
use keeper_core::vm::VoiceStateVm;
use tauri::{
    AppHandle, Emitter, Manager, Monitor, PhysicalPosition, WebviewUrl, WebviewWindow,
    WebviewWindowBuilder,
};

/// The pill window's label: the one string `capabilities/voice.json` scopes
/// its grant to, and the one `emit_to` addresses.
pub const VOICE_WINDOW_LABEL: &str = "voice";

/// The pill's document, the third entry point beside `index.html` and
/// `capture.html`.
const VOICE_DOCUMENT: &str = "voice.html";

/// Emitted to the pill window with every [`VoiceStateVm`] snapshot the turn
/// streams (`keeper://kebab-case`, the convention `notes_window` set).
pub const VOICE_STATE_EVENT: &str = "keeper://voice-state";

/// The pill's size in logical pixels (AD-185's ~220×40 pt).
pub const VOICE_WINDOW_SIZE: (u32, u32) = (220, 40);

/// The gap between the top of the work area and the pill, in logical
/// pixels. The work area already excludes the menu bar — and on a notched
/// Mac the menu bar is the notch's height, so the notch too — and this is
/// the breathing room below it.
const TOP_INSET: u32 = 8;

/// How long the armed-and-waiting glance lasts (AD-185).
const ARMED_GLANCE: Duration = Duration::from_secs(2);

/// How long a failure sentence stays up. `Failed` is a state the turn
/// leaves only on the next trigger, so without a bound the sentence would
/// float over every app until the person spoke again — long enough to read
/// a sentence, not a fixture.
const FAILURE_LINGER: Duration = Duration::from_secs(6);

/// The app handle, set once by [`install`] and only when the window was
/// created. Unset means no pill — an unsupported port — and every
/// [`observe`] is a no-op.
static APP: OnceLock<AppHandle> = OnceLock::new();

/// What the pill is doing, per the module doc.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Face {
    /// Not on screen: the turn is idle and the phrase is not armed.
    Hidden,
    /// On screen: a turn is running.
    Shown,
    /// On screen for [`ARMED_GLANCE`], then hidden: armed and waiting.
    Glance,
    /// On screen for [`FAILURE_LINGER`], then hidden: the turn failed.
    Failed,
}

/// What a snapshot asks of the window, given what it was doing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Step {
    /// Nothing changes; the document is re-fed and that is all.
    Keep,
    /// Place and show.
    Show,
    /// Hide.
    Hide,
    /// Place, show, and hide again after this long unless the face moves
    /// on first.
    Glance(Duration),
}

/// Which face a snapshot wants (AD-185).
#[must_use]
pub fn face(state: &VoiceStateVm) -> Face {
    match state {
        VoiceStateVm::Idle {
            listening_for_wake: false,
            ..
        } => Face::Hidden,
        VoiceStateVm::Idle {
            listening_for_wake: true,
            ..
        } => Face::Glance,
        VoiceStateVm::Listening { .. }
        | VoiceStateVm::Heard { .. }
        | VoiceStateVm::Sending { .. }
        | VoiceStateVm::Speaking => Face::Shown,
        VoiceStateVm::Failed { .. } => Face::Failed,
    }
}

/// The show/hide decision, pure over the previous face and the new snapshot.
///
/// A face that does not change is [`Step::Keep`] whatever the snapshot — so
/// a level reading does not re-place the window, and a re-pushed armed or
/// failed snapshot (a pane remount re-pushes the current state) does not
/// glance again. A glance that follows a shown turn keeps the pill up for
/// the glance and then hides it; a glance that follows nothing shows it for
/// the same glance.
#[must_use]
pub fn step(previous: Face, state: &VoiceStateVm) -> Step {
    let next = face(state);
    if next == previous {
        return Step::Keep;
    }
    match next {
        Face::Hidden => Step::Hide,
        Face::Shown => Step::Show,
        Face::Glance => Step::Glance(ARMED_GLANCE),
        Face::Failed => Step::Glance(FAILURE_LINGER),
    }
}

/// Where the pill goes: horizontally centred on `area`, `inset` below its
/// top edge, and never off it. Everything in **physical** pixels, matching
/// [`WorkArea`]; the caller converts [`TOP_INSET`] by the monitor's scale.
#[must_use]
pub fn pill_position(size: (u32, u32), area: WorkArea, inset: u32) -> (i32, i32) {
    let free_width = area.size.0.saturating_sub(size.0);
    let centred = i32::try_from(free_width / 2).unwrap_or(0);
    let wanted = (
        area.position.0.saturating_add(centred),
        area.position
            .1
            .saturating_add(i32::try_from(inset).unwrap_or(0)),
    );
    clamp_position(wanted, size, Some(area))
}

/// The pill's process state: what it is doing, whether it is on screen,
/// and which glance is current.
///
/// `face` and `visible` are two things: a glance's timer hides the window
/// and leaves the face as it was, so the next snapshot that re-states the
/// same face (a pane remount) is [`Step::Keep`] and does not re-show it.
struct Pill {
    face: Face,
    visible: bool,
    /// Bumped on every step that is not [`Step::Keep`]; a glance's timer
    /// hides the pill only if this has not moved since it was armed.
    generation: u64,
}

static PILL: Mutex<Pill> = Mutex::new(Pill {
    face: Face::Hidden,
    visible: false,
    generation: 0,
});

fn pill() -> std::sync::MutexGuard<'static, Pill> {
    PILL.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// Create the pill hidden at boot, when voice is a real answer here.
///
/// Called once from `lib.rs`'s `setup`, after the locale is loaded and the
/// hotkey installed — the same gate they read. A refusal from the builder is
/// logged and leaves no window, which is the same absence as an unsupported
/// port: the turn still runs, the pane still shows it, and nothing floats.
pub fn install(app: &AppHandle) {
    if !crate::voice_reach::present() {
        tracing::debug!("voice pill: voice unsupported here; no window created");
        return;
    }
    let built = WebviewWindowBuilder::new(
        app,
        VOICE_WINDOW_LABEL,
        WebviewUrl::App(VOICE_DOCUMENT.into()),
    )
    .title("keeper — voice")
    .inner_size(
        f64::from(VOICE_WINDOW_SIZE.0),
        f64::from(VOICE_WINDOW_SIZE.1),
    )
    .decorations(false)
    .always_on_top(true)
    .visible_on_all_workspaces(true)
    // Never key, never main: the pill is looked at, not used.
    .focusable(false)
    .focused(false)
    .skip_taskbar(true)
    .resizable(false)
    .visible(false)
    .build();
    let window = match built {
        Ok(window) => window,
        Err(error) => {
            tracing::warn!(%error, "voice pill: could not create the window; nothing floats");
            return;
        }
    };
    // Click-through, from the first frame it is ever shown: a click on the
    // pill is a click on whatever is under it. A refusal is logged and the
    // pill still cannot take focus, so the worst case is a click swallowed.
    if let Err(error) = window.set_ignore_cursor_events(true) {
        tracing::warn!(%error, "voice pill: could not make the window click-through");
    }
    let _ = APP.set(app.clone());
    tracing::info!("voice pill: window created hidden");
}

/// Hand one snapshot to the pill (`voice_ipc::push`'s fan-out).
///
/// Never touches the window on the caller's thread — see the module doc's
/// rule about threads. The order of snapshots is kept: `run_on_main_thread`
/// queues onto the event loop in FIFO order, and the caller holds the voice
/// lock while it queues, so two pushes cannot interleave.
pub fn observe(state: &VoiceStateVm) {
    let Some(app) = APP.get() else {
        return;
    };
    let handle = app.clone();
    let snapshot = state.clone();
    if let Err(error) = app.run_on_main_thread(move || apply(&handle, snapshot)) {
        tracing::warn!(%error, "voice pill: could not reach the main thread");
    }
}

/// Re-place the pill on the main window's screen, if it is on screen
/// (`WindowEvent::Moved` on the main window, from `lib.rs`).
///
/// Cheap by construction: one lock and a return while the pill is hidden,
/// which is nearly always, and the placement getters run only while a turn
/// is on screen and keeper is being dragged at the same time.
pub fn follow(app: &AppHandle) {
    if !pill().visible {
        return;
    }
    if let Some(window) = window(app) {
        place(app, &window);
    }
}

/// The window, or `None` when [`install`] did not create it.
fn window(app: &AppHandle) -> Option<WebviewWindow> {
    app.get_webview_window(VOICE_WINDOW_LABEL)
}

/// Feed the document, then show, hide or glance per [`step`]. Main thread
/// only.
fn apply(app: &AppHandle, snapshot: VoiceStateVm) {
    let Some(window) = window(app) else {
        return;
    };
    // The document first, the window second: the eval reaches the webview
    // before the show is processed, so a pill that comes up comes up already
    // saying the new state rather than the last one for a frame.
    if let Err(error) = app.emit_to(VOICE_WINDOW_LABEL, VOICE_STATE_EVENT, &snapshot) {
        tracing::warn!(%error, "voice pill: could not emit the snapshot");
    }
    let (step, generation) = {
        let mut pill = pill();
        let step = step(pill.face, &snapshot);
        if step != Step::Keep {
            pill.face = face(&snapshot);
            pill.visible = step != Step::Hide;
            pill.generation = pill.generation.wrapping_add(1);
        }
        (step, pill.generation)
    };
    match step {
        Step::Keep => {}
        Step::Hide => hide(&window),
        Step::Show => {
            place(app, &window);
            show(&window);
        }
        Step::Glance(linger) => {
            place(app, &window);
            show(&window);
            let handle = app.clone();
            tauri::async_runtime::spawn(async move {
                tokio::time::sleep(linger).await;
                let stale = pill().generation != generation;
                if stale {
                    return;
                }
                let inner = handle.clone();
                let result = handle.run_on_main_thread(move || {
                    let handle = inner;
                    // The face stays: a re-push of the same state after the
                    // glance is `Keep`, not a second glance. Only the
                    // window goes.
                    let mut pill = pill();
                    if pill.generation != generation {
                        return;
                    }
                    pill.visible = false;
                    drop(pill);
                    // `self::window`, not the `window` local that `apply` bound
                    // above and this closure would otherwise resolve to.
                    if let Some(window) = self::window(&handle) {
                        hide(&window);
                    }
                });
                if let Err(error) = result {
                    tracing::warn!(%error, "voice pill: could not end the glance");
                }
            });
        }
    }
}

fn show(window: &WebviewWindow) {
    if let Err(error) = window.show() {
        tracing::warn!(%error, "voice pill: could not show the window");
    }
}

fn hide(window: &WebviewWindow) {
    if let Err(error) = window.hide() {
        tracing::warn!(%error, "voice pill: could not hide the window");
    }
}

/// Put the pill at the top centre of the screen the main window is on — or
/// the primary screen when keeper is hidden or minimised, which is the
/// reach case: the phrase said with keeper nowhere on screen. Main thread
/// only; every call here is a getter.
///
/// Best-effort, like every placement in `notes_window`: a compositor that
/// declines `set_position` leaves the pill where it was created, which is
/// still on a screen.
fn place(app: &AppHandle, window: &WebviewWindow) {
    let Some(monitor) = screen(app, window) else {
        tracing::debug!("voice pill: no monitor geometry; leaving the window where it is");
        return;
    };
    let Ok(size) = window.outer_size() else {
        return;
    };
    let area = monitor.work_area();
    let area = WorkArea {
        position: (area.position.x, area.position.y),
        size: (area.size.width, area.size.height),
    };
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    let inset = (f64::from(TOP_INSET) * monitor.scale_factor()).round() as u32;
    let (x, y) = pill_position((size.width, size.height), area, inset);
    if let Err(error) = window.set_position(PhysicalPosition::new(x, y)) {
        tracing::debug!(%error, "voice pill: the compositor placed the window itself");
    }
}

/// The monitor the main window is on while it is on screen, else the
/// primary one, else whichever the pill itself is on.
fn screen(app: &AppHandle, window: &WebviewWindow) -> Option<Monitor> {
    let main = app
        .get_webview_window("main")
        .filter(|main| main.is_visible().unwrap_or(false) && !main.is_minimized().unwrap_or(false));
    main.and_then(|main| main.current_monitor().ok().flatten())
        .or_else(|| window.primary_monitor().ok().flatten())
        .or_else(|| window.current_monitor().ok().flatten())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn idle(armed: bool) -> VoiceStateVm {
        VoiceStateVm::Idle {
            wake: Some("nixie".to_owned()),
            listening_for_wake: armed,
        }
    }

    fn listening(level: Option<f32>) -> VoiceStateVm {
        VoiceStateVm::Listening {
            heard: String::new(),
            level,
        }
    }

    #[test]
    fn a_turn_is_shown_and_idle_unarmed_is_hidden() {
        assert_eq!(face(&idle(false)), Face::Hidden);
        assert_eq!(face(&listening(None)), Face::Shown);
        assert_eq!(
            face(&VoiceStateVm::Heard {
                text: "hi".to_owned(),
                level: Some(0.2)
            }),
            Face::Shown
        );
        assert_eq!(
            face(&VoiceStateVm::Sending { answering: false }),
            Face::Shown
        );
        assert_eq!(face(&VoiceStateVm::Speaking), Face::Shown);
        assert_eq!(
            face(&VoiceStateVm::Failed {
                reason: "no".to_owned()
            }),
            Face::Failed
        );
    }

    #[test]
    fn arming_is_a_glance_and_not_a_fixture() {
        // Switch on: a glance. The same snapshot again (a pane remount
        // re-pushes it): nothing, or the pill would flash on every mount.
        assert_eq!(step(Face::Hidden, &idle(true)), Step::Glance(ARMED_GLANCE));
        assert_eq!(step(Face::Glance, &idle(true)), Step::Keep);
        // A turn that ends re-armed: the pill is up, stays for the glance,
        // then the timer hides it.
        assert_eq!(step(Face::Shown, &idle(true)), Step::Glance(ARMED_GLANCE));
        // The glance's timer has fired and left the face alone: the phrase
        // being heard shows the pill again.
        assert_eq!(step(Face::Glance, &listening(Some(0.1))), Step::Show);
    }

    #[test]
    fn a_failure_lingers_long_enough_to_read_and_then_goes() {
        let failed = VoiceStateVm::Failed {
            reason: "The microphone is in use.".to_owned(),
        };
        assert_eq!(step(Face::Shown, &failed), Step::Glance(FAILURE_LINGER));
        assert_eq!(step(Face::Hidden, &failed), Step::Glance(FAILURE_LINGER));
        // `Failed` is left only on the next trigger, so a re-push must not
        // bring the sentence back.
        assert_eq!(step(Face::Failed, &failed), Step::Keep);
        assert_eq!(step(Face::Failed, &listening(None)), Step::Show);
        assert_eq!(step(Face::Failed, &idle(false)), Step::Hide);
        assert!(FAILURE_LINGER > ARMED_GLANCE);
    }

    #[test]
    fn a_level_reading_changes_nothing_about_the_window() {
        assert_eq!(step(Face::Shown, &listening(Some(0.3))), Step::Keep);
        assert_eq!(step(Face::Shown, &listening(Some(0.9))), Step::Keep);
        assert_eq!(step(Face::Shown, &VoiceStateVm::Speaking), Step::Keep);
    }

    #[test]
    fn leaving_the_turn_hides_and_starting_one_shows() {
        assert_eq!(step(Face::Shown, &idle(false)), Step::Hide);
        assert_eq!(step(Face::Glance, &idle(false)), Step::Hide);
        assert_eq!(step(Face::Hidden, &idle(false)), Step::Keep);
        assert_eq!(step(Face::Hidden, &listening(None)), Step::Show);
        assert_eq!(step(Face::Glance, &listening(None)), Step::Show);
    }

    #[test]
    fn the_pill_sits_top_centre_below_the_menu_bar() {
        // A 2× display: work area starts below a 74 px menu bar, the pill
        // is 440×80 physical and the inset 16.
        let area = WorkArea {
            position: (0, 74),
            size: (2880, 1726),
        };
        assert_eq!(pill_position((440, 80), area, 16), (1220, 90));
    }

    #[test]
    fn the_pill_follows_a_second_screen_and_stays_on_it() {
        let area = WorkArea {
            position: (2880, -200),
            size: (1920, 1055),
        };
        assert_eq!(pill_position((220, 40), area, 8), (3730, -192));
        // Wider than the screen: flush against the near edge, not negative.
        assert_eq!(pill_position((4000, 40), area, 8), (2880, -192));
    }
}
