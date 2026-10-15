//! Shell call site of `keeper_core::failure` (Epic 100, R381 as amended by
//! R387 and R410, 100.1 #7): the process's one [`Failures`], told of a
//! background failure where no command was waiting ([`tell`]) and of the
//! same operation's next success ([`recovered`], [`rearm_finished`]), and
//! streamed to the webview as a snapshot then its changes
//! ([`failure_subscribe`]). The decisions, the log line and the state are
//! core's; this holds it and streams it.

use std::collections::HashMap;
use std::sync::{LazyLock, Mutex, PoisonError};

use keeper_core::failure::{FailureOperation, FailureStateVm, Failures};
use keeper_core::vm::IpcError;
use tauri::async_runtime::JoinHandle;
use tauri::ipc::Channel;

/// The standing background failures of this process.
static FAILURES: LazyLock<Failures> = LazyLock::new(Failures::default);

/// The live subscriptions, so an unmounting webview can end its own.
static SUBSCRIPTIONS: LazyLock<Mutex<HashMap<String, JoinHandle<()>>>> =
    LazyLock::new(Mutex::default);

/// Log `cause` with the sentence at `WARN` and make it the standing failure
/// of `operation` (and `account`, where the operation is per account) until
/// it [`recovered`].
pub fn tell(
    operation: FailureOperation,
    account: Option<&str>,
    sentence: impl Into<String>,
    cause: &dyn std::fmt::Display,
) {
    FAILURES.notice(operation, account, sentence, cause);
}

/// `operation` succeeded: its own standing failure is over.
pub fn recovered(operation: FailureOperation, account: Option<&str>) {
    FAILURES.recovered(operation, account);
}

/// A wake-phrase re-arm finished: core's [`Failures::rearm_finished`]
/// decides what that means for its failure.
pub fn rearm_finished(outcome: Result<bool, &dyn std::fmt::Display>) {
    FAILURES.rearm_finished(outcome);
}

/// Stream the standing background failures: the current set at once — a
/// failure told before any webview listened is in it — then every change.
#[tauri::command]
pub fn failure_subscribe(channel: Channel<FailureStateVm>) -> Result<String, IpcError> {
    let mut state = FAILURES.subscribe();
    // Read before the task waits on `changed`, so no change falls between.
    let current = state.borrow_and_update().clone();
    if let Err(error) = channel.send(current) {
        tracing::warn!(%error, "the failure subscription could not start");
    }
    let task = tauri::async_runtime::spawn(async move {
        while state.changed().await.is_ok() {
            let current = state.borrow_and_update().clone();
            if channel.send(current).is_err() {
                return;
            }
        }
    });
    let id = ulid::Ulid::new().to_string();
    SUBSCRIPTIONS
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .insert(id.clone(), task);
    Ok(id)
}

/// End a [`failure_subscribe`] stream. An unknown id is a no-op.
#[tauri::command]
pub fn failure_unsubscribe(subscription_id: String) {
    if let Some(task) = SUBSCRIPTIONS
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .remove(&subscription_id)
    {
        task.abort();
    }
}
