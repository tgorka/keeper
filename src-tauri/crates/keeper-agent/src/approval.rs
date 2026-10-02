//! The approval round trip (Story 61.10, FR-387) for a host with a person at
//! it: the ask travels down the turn's own stream as
//! [`BotStreamEvent::ApprovalAsked`], and the tool call blocks on a one-shot
//! sender registered under the ask's id until [`answer`] is called with it.
//!
//! The stream is whatever [`TurnSink`] the turn streams into — a pane's
//! channel for a typed turn, the app-wide spoken-stream event for a spoken
//! one — so every turn that has somewhere to show an ask can be answered, and
//! the answer comes back the same way whichever sink carried the ask.

use std::collections::HashMap;
use std::sync::mpsc::{RecvTimeoutError, SyncSender};
use std::sync::{Arc, LazyLock, Mutex, MutexGuard};
use std::time::Duration;

use keeper_core::bots::chat::CancelSignal;
use keeper_core::vm::{BotApprovalRequestVm, BotStreamEvent};

use crate::ports::{ApprovalPort, TurnSink};

/// How long a blocked approval waits between looks at its cancel signal: the
/// cadence at which Stop releases a turn waiting on a sheet nobody will
/// answer.
const APPROVAL_POLL: Duration = Duration::from_millis(250);

/// Approvals waiting on a person, keyed by request id. An entry outlives
/// nothing: the asking side removes it when it has its answer, or when its
/// turn was stopped.
fn asks() -> MutexGuard<'static, HashMap<String, SyncSender<bool>>> {
    static ASKS: LazyLock<Mutex<HashMap<String, SyncSender<bool>>>> =
        LazyLock::new(|| Mutex::new(HashMap::new()));
    // A poisoned lock means a driver panicked mid-answer. The map holds
    // senders and nothing else, so there is no torn state to protect and
    // refusing every later ask would be the worse failure.
    ASKS.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// Answer the tool call waiting on `request_id`.
///
/// Idempotent: an id nobody is waiting on — answered twice, or belonging to a
/// turn Stop already released — is a no-op, and the default for an ask nobody
/// answers is a refusal, in the waiting side.
pub fn answer(request_id: &str, approved: bool) {
    if let Some(answer) = asks().remove(request_id) {
        // A receiver that is gone was a turn that stopped waiting; the answer
        // then changes nothing, which is what a late answer should change.
        let _ = answer.send(approved);
    }
}

/// Ask down a turn's stream, wait, and obey.
///
/// The wait is a blocking receive inside `tokio::task::block_in_place`, which
/// hands this worker's slot to another thread for the duration rather than
/// starving the runtime while a person reads a sheet. It ends on the answer,
/// on Stop (the cancel signal is looked at every [`APPROVAL_POLL`]), or on a
/// sink with nobody listening — and every way it ends other than an explicit
/// `true` is a refusal.
pub struct SinkApprover {
    sink: Arc<dyn TurnSink>,
}

impl SinkApprover {
    /// An approver whose asks go down `sink`.
    pub fn new(sink: Arc<dyn TurnSink>) -> Self {
        Self { sink }
    }
}

impl ApprovalPort for SinkApprover {
    fn ask(&self, request: BotApprovalRequestVm, signal: &CancelSignal) -> bool {
        let request_id = request.request_id.clone();
        let (answer, waiting) = std::sync::mpsc::sync_channel::<bool>(1);
        asks().insert(request_id.clone(), answer);
        if !self.sink.event(BotStreamEvent::ApprovalAsked {
            request: Box::new(request),
        }) {
            asks().remove(&request_id);
            return false;
        }
        let approved = tokio::task::block_in_place(|| loop {
            match waiting.recv_timeout(APPROVAL_POLL) {
                Ok(approved) => break approved,
                Err(RecvTimeoutError::Timeout) if !signal.is_cancelled() => {}
                Err(_) => break false,
            }
        });
        asks().remove(&request_id);
        approved
    }
}
