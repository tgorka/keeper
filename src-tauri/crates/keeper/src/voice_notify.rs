//! The lock-screen banner: what keeper heard, said as a local notification
//! while keeper is not in front (Epic 67, Story 67.2, FR-477–FR-479,
//! AD-207).
//!
//! # Why a notification, beside the island
//!
//! The island (`voice_island`) is the richer surface and the one a free
//! team's build cannot draw. A local notification with no trigger is
//! delivered at once, from any thread, by every build: no entitlement, no
//! App ID. Posted with one fixed identifier ([`IDENTIFIER`]) it replaces
//! itself in place, so the lock screen shows one line that moves —
//! "Heard: …", "Thinking", "Answer: …" — rather than a stack. `.active`
//! lights the screen; no sound, because the answer is the sound.
//!
//! # Where the decision is
//!
//! Which word gets which sentence, how the answer is clipped, and that
//! nothing is posted in front are [`keeper_core::voice::banner`], tested on
//! the dev host (AD-55/AD-56). The word itself is
//! [`keeper_core::voice::island::word`] — the same projection the island
//! uses, so the card and the banner can never disagree. This module holds
//! what was last shown, the answer text the `Speaking` snapshot does not
//! carry, and the four calls into UIKit and UserNotifications.
//!
//! # The one rule about threads
//!
//! "In front" is `UIApplication.applicationState`, a main-thread getter —
//! never the JS lifecycle, which stops running exactly when this matters.
//! [`observe`] is called from `voice_ipc::push` with the voice lock held, so
//! it decides nothing about the application state there: it queues the
//! banner onto the main thread (`run_on_main_thread`, FIFO, the pill's own
//! pattern) and the state is read where it may be. Posting and removing a
//! notification are asynchronous system calls that touch no window; they
//! run on the main thread too, after the read, so the read and the post are
//! one ordered step. Every binding used here is a safe one: nothing in this
//! file is `unsafe`.
//!
//! # What clears it
//!
//! The turn ending (the word going back to `armed`, or to nothing when the
//! switch is off), a barge-in (`listening` again), and keeper coming to the
//! front ([`resumed`], from `lib.rs`'s `RunEvent::Resumed`). A failure's
//! sentence stays until then: the turn leaves `Failed` only on the next
//! trigger, and a reason nobody read is a reason nobody can act on.

use std::sync::{Mutex, OnceLock};

use keeper_core::vm::VoiceStateVm;
use keeper_core::voice::banner::{sentence, should_post, Banner};
use keeper_core::voice::events::VoiceEventKind;
use keeper_core::voice::island::{word, Word};
use keeper_core::voice::Effect;
use objc2::MainThreadMarker;
use objc2_foundation::{NSArray, NSString};
use objc2_ui_kit::{UIApplication, UIApplicationState};
use objc2_user_notifications::{
    UNMutableNotificationContent, UNNotificationInterruptionLevel, UNNotificationRequest,
    UNUserNotificationCenter,
};
use tauri::AppHandle;

/// The one notification's identifier: a request with the same identifier
/// replaces the delivered one in place.
pub const IDENTIFIER: &str = "keeper.voice";

/// The process state.
struct Notify {
    /// The word the latest snapshot had, so a re-pushed snapshot (a pane
    /// remount, a level reading) is nothing.
    showing: Option<Word>,
    /// The answer handed to the synthesiser: the `Speaking` snapshot carries
    /// no text, so the banner's first sentence is kept from the effect.
    answer: String,
    /// Whether a banner is delivered, so a clear is one call when there is
    /// something to clear and no call otherwise. Main thread only.
    posted: bool,
}

static STATE: Mutex<Notify> = Mutex::new(Notify {
    showing: None,
    answer: String::new(),
    posted: false,
});

/// The handle for `run_on_main_thread`, set once from `lib.rs`'s setup.
static APP: OnceLock<AppHandle> = OnceLock::new();

fn state() -> std::sync::MutexGuard<'static, Notify> {
    STATE
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// Keep the app handle (`lib.rs` setup). Idempotent: write-once.
pub fn install(app: &AppHandle) {
    let _ = APP.set(app.clone());
}

/// The effects of a transition (`voice_ipc::transition`, before the
/// snapshot is pushed): an answer handed to the synthesiser is kept for the
/// `Speaking` banner that follows it.
pub fn answer(effects: &[Effect]) {
    for effect in effects {
        if let Effect::Speak(text) = effect {
            state().answer.clone_from(text);
        }
    }
}

/// Hand one snapshot to the banner (`voice_ipc::push`'s fan-out).
///
/// Decides the word and its sentence here, under the voice lock — pure
/// work — and queues the application-state read and the post onto the
/// main thread. The order of snapshots is kept: `run_on_main_thread`
/// queues FIFO and the caller holds the voice lock while it queues.
pub fn observe(snapshot: &VoiceStateVm) {
    let next = word(snapshot);
    let banner = {
        let mut state = state();
        if state.showing == next {
            return;
        }
        state.showing = next;
        let detail = match snapshot {
            VoiceStateVm::Heard { text, .. } => text.as_str(),
            VoiceStateVm::Failed { reason } => reason.as_str(),
            VoiceStateVm::Speaking => state.answer.as_str(),
            _ => "",
        };
        next.and_then(|next| sentence(next, detail).map(|banner| (next, banner)))
    };
    on_main(move |main| match banner {
        Some((next, banner)) if should_post(in_front(main), next) => post(next, &banner),
        _ => clear(),
    });
}

/// Keeper is back in front (`RunEvent::Resumed`): whatever the lock screen
/// said, the pane says now.
pub fn resumed() {
    on_main(|_| clear());
}

/// Queue `job` onto the main thread, where UIKit's getters may be read.
/// Before setup there is no handle and nothing to show it on.
fn on_main(job: impl FnOnce(MainThreadMarker) + Send + 'static) {
    let Some(app) = APP.get() else {
        return;
    };
    let result = app.run_on_main_thread(move || {
        // `run_on_main_thread` is the main thread by contract; the marker
        // is the compile-time proof `sharedApplication` asks for, and a
        // contract that ever changed would say so here rather than read a
        // main-thread property from the wrong thread.
        let Some(main) = MainThreadMarker::new() else {
            tracing::warn!("voice banner: run_on_main_thread did not run on the main thread");
            return;
        };
        job(main);
    });
    if let Err(error) = result {
        tracing::warn!(%error, "voice banner: could not reach the main thread");
    }
}

/// Whether keeper is the app in front. `Inactive` — the lock screen coming
/// down, Notification Centre pulled over the app — counts as not in front:
/// the pane is not what the person is looking at.
fn in_front(main: MainThreadMarker) -> bool {
    UIApplication::sharedApplication(main).applicationState() == UIApplicationState::Active
}

/// Post `banner` under [`IDENTIFIER`], replacing the delivered one, and
/// record it. Main thread; the system call itself is asynchronous.
fn post(next: Word, banner: &Banner) {
    let content = UNMutableNotificationContent::new();
    content.setTitle(&NSString::from_str(&banner.title));
    content.setBody(&NSString::from_str(&banner.body));
    content.setInterruptionLevel(UNNotificationInterruptionLevel::Active);
    let request = UNNotificationRequest::requestWithIdentifier_content_trigger(
        &NSString::from_str(IDENTIFIER),
        &content,
        None,
    );
    // The completion handler is omitted, as the badge's is (`ipc.rs`): a
    // refusal — permission denied under Settings > keeper > Notifications —
    // means no banner, never a failed turn, and the ring row below says one
    // was asked for.
    UNUserNotificationCenter::currentNotificationCenter()
        .addNotificationRequest_withCompletionHandler(&request, None);
    state().posted = true;
    record(next.as_str());
}

/// Remove the delivered banner, if there is one, and record it.
fn clear() {
    let was_posted = std::mem::replace(&mut state().posted, false);
    if !was_posted {
        return;
    }
    let identifiers = NSArray::from_retained_slice(&[NSString::from_str(IDENTIFIER)]);
    UNUserNotificationCenter::currentNotificationCenter()
        .removeDeliveredNotificationsWithIdentifiers(&identifiers);
    record("cleared");
}

/// One row in the ring: `notified` with the word, or `cleared`.
fn record(detail: &str) {
    crate::voice_log::record(VoiceEventKind::Notified, Some(detail.to_owned()));
}
