//! The island: a Live Activity that shows what keeper's ear is doing (Epic
//! 65, Story 65.5, FR-453–FR-455, NFR-56, AD-194).
//!
//! # What it is
//!
//! The phone's sibling of the Mac's pill (`voice_window`): a card in the
//! Dynamic Island and on the lock screen, drawn by the `KeeperIsland`
//! widget extension, that says keeper is listening for the phrase, hearing
//! you, thinking, answering, or speaking — and which phrase. What iOS gives
//! for free is the orange microphone dot, which names nobody; this is the
//! only route to keeper's name and state on the lock screen.
//!
//! # Where the decision is, and where it is not
//!
//! ActivityKit is Swift-only — a generic class with no Objective-C surface,
//! so `objc2` cannot reach it — and the Swift it takes lives in
//! `gen/apple/Sources/keeper/KeeperIsland.swift` (the bridge, compiled into
//! the app target) and `gen/apple/KeeperIsland/` (the extension). Neither
//! decides anything, and neither does this module:
//! [`keeper_core::voice::island::step`] is the whole of start/update/end,
//! pure over the previous [`Word`] and the new snapshot, tested in core
//! (AD-55/AD-56). This module holds the process state, the renew timer,
//! and the four `extern "C"` calls the bridge answers.
//!
//! # The eight hours, and the free team
//!
//! The system ends every Live Activity after eight hours. [`RENEW_AFTER`]
//! ends keeper's before that and requests a new one with the same word;
//! when that request is refused — keeper in the background at the time,
//! since Apple starts an activity only for an app in front — the refusal
//! is recorded once and the request is made again the next time keeper
//! comes to the front ([`resumed`], from `lib.rs`'s `RunEvent::Resumed`).
//!
//! `Activity.request` can also refuse for Live Activities switched off for
//! keeper in Settings, an iOS older than 16.2, or — the one nobody has
//! measured for a Personal Team's automatic profile — `unentitled` /
//! `unsupportedTarget`. The bridge returns every refusal as a sentence and
//! it goes into the event ring (`voice_log`, AD-192) as `island:refused`,
//! so the phone's Settings → Bots shows it and the story is closed on a
//! measurement rather than a hope.
//!
//! # The one rule about threads
//!
//! [`observe`] is called from `voice_ipc::push` with the voice lock held.
//! `Activity.request` is a synchronous call to the system that touches no
//! window and no main-thread getter, so it is safe there; `update` and `end`
//! are queued on the Swift side and return at once. This module's own lock
//! ([`ISLAND`]) is never held while the voice lock is taken, so there is no
//! cycle; the renew timer and [`resumed`] take only this one.

use std::ffi::{c_char, CStr, CString};
use std::sync::Mutex;
use std::time::Duration;

use keeper_core::vm::VoiceStateVm;
use keeper_core::voice::events::VoiceEventKind;
use keeper_core::voice::island::{detail, step, word, Step, Word, RENEW_AFTER};

/// The process state: what the activity shows, what the latest snapshot
/// wanted, the phrase the card was started with, and which renew timer is
/// current.
struct Island {
    /// The word on the running activity; `None` when there is none.
    showing: Option<Word>,
    /// The word the latest snapshot asked for, kept so a refused start can
    /// be made again from [`resumed`] without a new snapshot.
    wanted: Option<Word>,
    /// The phrase as the latest idle snapshot gave it (matching form), for
    /// the card's static line.
    phrase: String,
    /// Bumped on every start and end; the renew timer checks it before
    /// acting, so a timer from an activity that has since ended does
    /// nothing.
    generation: u64,
    /// The last refusal recorded, so a start refused for the same reason on
    /// every transition in the background is one row in the ring, not two
    /// hundred.
    refused: Option<String>,
}

static ISLAND: Mutex<Island> = Mutex::new(Island {
    showing: None,
    wanted: None,
    phrase: String::new(),
    generation: 0,
    refused: None,
});

fn island() -> std::sync::MutexGuard<'static, Island> {
    ISLAND
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// Hand one snapshot to the island (`voice_ipc::push`'s fan-out).
pub fn observe(state: &VoiceStateVm) {
    let mut island = island();
    if let VoiceStateVm::Idle { wake, .. } = state {
        island.phrase = wake.clone().unwrap_or_default();
    }
    island.wanted = word(state).filter(|next| *next != Word::Failed);
    match step(island.showing, state) {
        Step::Keep => {}
        Step::Start(next) => start(&mut island, next),
        Step::Update(next) => {
            island.showing = Some(next);
            bridge_update(next, detail(state));
            record("updated", next.as_str());
        }
        Step::End { last, linger } => {
            island.showing = None;
            island.generation = island.generation.wrapping_add(1);
            bridge_end(last, detail(state), linger);
            record("ended", last.as_str());
        }
    }
}

/// Keeper is back in front (`RunEvent::Resumed`): a start refused while it
/// was not — the renew, or an arm from the port's own resume — is made now.
pub fn resumed() {
    let mut island = island();
    if island.showing.is_none() {
        if let Some(next) = island.wanted {
            start(&mut island, next);
        }
    }
}

/// Request the activity for `next`, record the outcome, and arm the renew
/// timer on success.
fn start(island: &mut Island, next: Word) {
    match bridge_start(next, &island.phrase) {
        Ok(()) => {
            island.showing = Some(next);
            island.refused = None;
            island.generation = island.generation.wrapping_add(1);
            let generation = island.generation;
            record("started", next.as_str());
            tauri::async_runtime::spawn(async move {
                tokio::time::sleep(RENEW_AFTER).await;
                renew(generation);
            });
        }
        Err(reason) => {
            island.showing = None;
            if island.refused.as_deref() != Some(reason.as_str()) {
                tracing::warn!(%reason, "voice: the Live Activity was refused");
                record("refused", &reason);
                island.refused = Some(reason);
            }
        }
    }
}

/// The renew timer fired: end the activity Apple would end at eight hours
/// and request a fresh one with the same word. A stale generation — the
/// activity ended or was replaced since — is nothing.
fn renew(generation: u64) {
    let mut island = island();
    if island.generation != generation {
        return;
    }
    let Some(showing) = island.showing else {
        return;
    };
    island.showing = None;
    bridge_end(showing, "", Duration::ZERO);
    start(&mut island, showing);
}

/// One row in the ring: `island:<what>` with the word or the refusal.
fn record(what: &'static str, detail: &str) {
    crate::voice_log::record(VoiceEventKind::Island(what), Some(detail.to_owned()));
}

// The bridge, `gen/apple/Sources/keeper/KeeperIsland.swift`: four `@_cdecl`
// functions compiled into the app target, which `libapp.a`'s undefined
// symbols resolve against at the app's link. `keeper_island_start` returns
// `NULL` for success, else a sentence it allocated with `strdup` that
// `keeper_island_free` releases. (A plain comment: rustdoc has no page for
// an extern block, and a `///` here is an error under `-D warnings`.)
extern "C" {
    fn keeper_island_start(word: *const c_char, phrase: *const c_char) -> *mut c_char;
    fn keeper_island_update(word: *const c_char, detail: *const c_char);
    fn keeper_island_end(word: *const c_char, detail: *const c_char, linger_seconds: u32);
    fn keeper_island_free(reason: *mut c_char);
}

/// A Rust string as a C string, with an interior NUL — impossible for a
/// word, unlikely for a sentence — replaced by nothing rather than a panic.
fn c(text: &str) -> CString {
    CString::new(text).unwrap_or_default()
}

/// `Activity.request` through the bridge: `Ok` when the card is up, else
/// the refusal in a sentence.
#[allow(unsafe_code)]
fn bridge_start(word: Word, phrase: &str) -> Result<(), String> {
    let word = c(word.as_str());
    let phrase = c(phrase);
    // SAFETY: both pointers are to NUL-terminated buffers that outlive the
    // call; the bridge copies them into Swift strings before returning. The
    // returned pointer is either null or a buffer the bridge allocated with
    // `strdup`, read once here and released by the bridge's own
    // `keeper_island_free`, never by Rust's allocator.
    let reason = unsafe { keeper_island_start(word.as_ptr(), phrase.as_ptr()) };
    if reason.is_null() {
        return Ok(());
    }
    // SAFETY: non-null, NUL-terminated, and owned by this call until freed
    // on the next line.
    let sentence = unsafe { CStr::from_ptr(reason) }
        .to_string_lossy()
        .into_owned();
    // SAFETY: the pointer `keeper_island_start` returned, freed exactly once.
    unsafe { keeper_island_free(reason) };
    Err(sentence)
}

/// `Activity.update` through the bridge; queued on the Swift side.
#[allow(unsafe_code)]
fn bridge_update(word: Word, detail: &str) {
    let word = c(word.as_str());
    let detail = c(detail);
    // SAFETY: NUL-terminated buffers that outlive the call; the bridge copies
    // them before it queues the update.
    unsafe { keeper_island_update(word.as_ptr(), detail.as_ptr()) };
}

/// `Activity.end` through the bridge; queued on the Swift side.
#[allow(unsafe_code)]
fn bridge_end(word: Word, detail: &str, linger: Duration) {
    let word = c(word.as_str());
    let detail = c(detail);
    let linger = u32::try_from(linger.as_secs()).unwrap_or(u32::MAX);
    // SAFETY: as in `bridge_update`.
    unsafe { keeper_island_end(word.as_ptr(), detail.as_ptr(), linger) };
}
