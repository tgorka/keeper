//! The voice port's record (Epic 65, Story 65.3, FR-448, FR-449, AD-192):
//! one process-wide [`VoiceEvents`] ring, the call every feeding site makes
//! ([`record`]), and the command the phone reads it through.
//!
//! **No decisions live here.** What is kept, how many, and in what order is
//! [`keeper_core::voice::events`]; this module holds the instance and the
//! clock. The ring has its own lock rather than living under `voice_ipc`'s,
//! because the ports feed it from their worker threads — where the turn's
//! lock must never be taken (`voice_ipc`'s module doc) — and a feed is a
//! push behind a mutex nobody holds for longer than that.

use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};

use keeper_core::vm::{IpcError, VoiceEventVm};
use keeper_core::voice::events::{VoiceEventKind, VoiceEvents};
use keeper_core::voice::{Effect, TurnState, WakePhrase};

/// The one ring for the process. Every target: a desktop with no port
/// records the turn's transitions and refusals all the same.
static EVENTS: Mutex<VoiceEvents> = Mutex::new(VoiceEvents::new());

/// Record one thing the port did, now. Never blocks on anything but the
/// ring's own lock, so it is safe from the port's worker and from under
/// `voice_ipc`'s lock alike; a poisoned ring is read through, because a
/// diagnostic that refused to record after a panic elsewhere would be the
/// one record nobody could read.
pub fn record(kind: VoiceEventKind, detail: Option<String>) {
    let at_ms = match SystemTime::now().duration_since(UNIX_EPOCH) {
        Ok(since) => i64::try_from(since.as_millis()).unwrap_or(i64::MAX),
        Err(_) => 0,
    };
    EVENTS
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .push(at_ms, kind, detail);
}

/// What a turn transition amounted to, recorded from `voice_ipc::transition`:
/// the state the turn moved to when it moved — a partial transcript that
/// leaves it in `Listening` is not a move, or the ring would be nothing but
/// partials — with the failure's reason or the final transcript as the
/// detail; the wake match that took an idle turn into `Listening`, since the
/// event the port sent was a transcript and the match is `Turn`'s; and the
/// answer handed to the synthesiser.
pub fn transition(before: VoiceEventKind, state: &TurnState, effects: &[Effect]) {
    let after = VoiceEventKind::turn(state);
    if before == VoiceEventKind::turn(&TurnState::Idle)
        && matches!(state, TurnState::Listening { .. })
    {
        record(VoiceEventKind::WakeMatched, None);
    }
    if after != before {
        let detail = match state {
            TurnState::Failed { reason } => Some(reason.clone()),
            TurnState::Heard { text } => Some(text.clone()),
            _ => None,
        };
        record(after, detail);
    }
    if effects
        .iter()
        .any(|effect| matches!(effect, Effect::Speak(_)))
    {
        record(VoiceEventKind::Spoken, None);
    }
}

/// The switch took effect on the port: armed for `wake`, or disarmed when
/// there is none.
pub fn armed(wake: Option<&WakePhrase>) {
    match wake {
        Some(wake) => record(VoiceEventKind::Armed, Some(wake.as_str().to_owned())),
        None => record(VoiceEventKind::Disarmed, None),
    }
}

/// The most recent `limit` events, newest first (FR-449). A view of memory:
/// nothing is read from disk and nothing is sent anywhere.
#[tauri::command]
pub fn voice_events(limit: usize) -> Result<Vec<VoiceEventVm>, IpcError> {
    Ok(EVENTS
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .newest(limit))
}
