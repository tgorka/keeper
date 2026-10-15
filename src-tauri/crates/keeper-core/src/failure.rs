//! A failure that happened where no command was waiting for it (Epic 100,
//! R381 as amended by R387, story 100.1 #7).
//!
//! A command's failure travels back to the webview as its `IpcError` and the
//! surface that asked says it. A failure in the background — the wake phrase
//! not re-armed when keeper came back to the front, a synced voice phrase
//! refused, a push registration the homeserver turned down — has no caller
//! to answer. It used to end as a `WARN` line nobody reads. It now ends as
//! that line **and** an entry in [`Failures`], Rust-owned state the webview
//! mirrors through a snapshot-then-changes subscription: a failure that
//! happened before any webview listened is in the snapshot the first one
//! reads, and a reloaded webview reads it again. A failure is keyed by the
//! [`FailureOperation`] that failed — not only by the surface it is said
//! under — and only that operation's next success clears it
//! ([`Failures::recovered`], R410): a wake-phrase re-arm that had nothing to
//! do never acknowledges a synced voice setting keeper refused. Where the
//! operation is per account, one account's recovery leaves another's failure
//! standing.
//!
//! Agents are not a surface here: an agent host's failures reach its room's
//! status line and `keeper.log`, never telemetry (S-19). Nothing here is sent
//! anywhere.

use serde::{Deserialize, Serialize};
use tokio::sync::watch;
use ts_rs::TS;

/// What the person is told when a push registration failed: the homeserver
/// was not told where to send this device's notifications.
pub const PUSH_NOT_REGISTERED: &str =
    "keeper could not turn on push notifications for one of your accounts, so its messages arrive only while keeper is open.";

/// What the person is told when the wake phrase could not be armed again
/// when keeper came back to the front.
pub const WAKE_NOT_REARMED: &str =
    "keeper could not start listening for the wake phrase again. Turn listening off and on to try once more.";

/// Which surface a background failure belongs to, so the webview can say it
/// under that surface's own switch as well as in a toast.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub enum FailureSurface {
    /// Listening for the wake phrase, and spoken turns.
    Voice,
    /// Push notifications (iOS and Android).
    Push,
}

impl FailureSurface {
    fn as_str(self) -> &'static str {
        match self {
            Self::Voice => "voice",
            Self::Push => "push",
        }
    }
}

/// The operation a background failure belongs to (R410). Only its own next
/// success clears it: two operations said under one surface stand and clear
/// apart.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub enum FailureOperation {
    /// Arming the wake phrase again from the persisted choice — when keeper
    /// comes back to the front, or when the person turns listening off and
    /// on, which is the remedy its sentence names.
    VoiceRearm,
    /// Applying the voice settings a sync wrote from the person's other
    /// device: the language, the switch and both phrases.
    VoiceSyncedSettings,
    /// Registering this device for push: the start at launch and every
    /// registration after it, per account where an account's own pusher
    /// failed.
    PushRegistration,
}

impl FailureOperation {
    /// The surface the failure is said under.
    pub fn surface(self) -> FailureSurface {
        match self {
            Self::VoiceRearm | Self::VoiceSyncedSettings => FailureSurface::Voice,
            Self::PushRegistration => FailureSurface::Push,
        }
    }

    fn as_str(self) -> &'static str {
        match self {
            Self::VoiceRearm => "voice-rearm",
            Self::VoiceSyncedSettings => "voice-synced-settings",
            Self::PushRegistration => "push-registration",
        }
    }
}

/// One background failure that has not recovered, as the person reads it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct FailureNoticeVm {
    pub surface: FailureSurface,
    /// The operation that failed; only its own next success clears it.
    pub operation: FailureOperation,
    /// The opaque keeper account id, where the surface is per account (a
    /// push registration); `None` for a failure of the surface as a whole.
    pub account: Option<String>,
    /// A finished, secret-free sentence.
    pub sentence: String,
}

/// Every background failure standing right now, oldest first.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct FailureStateVm {
    pub notices: Vec<FailureNoticeVm>,
}

/// The process's standing background failures: set by [`Failures::notice`],
/// cleared by [`Failures::recovered`] (and, for the wake-phrase re-arm, by
/// [`Failures::rearm_finished`]), read as a snapshot and its changes through
/// [`Failures::subscribe`].
#[derive(Debug)]
pub struct Failures {
    state: watch::Sender<FailureStateVm>,
}

impl Default for Failures {
    fn default() -> Self {
        Self {
            state: watch::Sender::new(FailureStateVm::default()),
        }
    }
}

impl Failures {
    /// Log a background failure at `WARN` — the surface, the operation, the
    /// account, the sentence the person is shown and the `cause` — and make it
    /// the standing failure of that operation and account until it recovers.
    /// A second failure of the same operation and account replaces the first.
    pub fn notice(
        &self,
        operation: FailureOperation,
        account: Option<&str>,
        sentence: impl Into<String>,
        cause: &dyn std::fmt::Display,
    ) {
        let sentence = sentence.into();
        let surface = operation.surface();
        tracing::warn!(
            surface = surface.as_str(),
            operation = operation.as_str(),
            account = account.unwrap_or("-"),
            %cause,
            %sentence,
            "a background failure; the person is told"
        );
        self.state.send_if_modified(|state| {
            let told = FailureNoticeVm {
                surface,
                operation,
                account: account.map(str::to_owned),
                sentence,
            };
            match state
                .notices
                .iter_mut()
                .find(|n| n.operation == operation && n.account.as_deref() == account)
            {
                Some(standing) if *standing == told => false,
                Some(standing) => {
                    *standing = told;
                    true
                }
                None => {
                    state.notices.push(told);
                    true
                }
            }
        });
    }

    /// `operation` (for `account`) succeeded: its own standing failure, if
    /// any, is over. Another operation's failure — even one said under the
    /// same surface — and another account's stand.
    pub fn recovered(&self, operation: FailureOperation, account: Option<&str>) {
        self.state.send_if_modified(|state| {
            let before = state.notices.len();
            state
                .notices
                .retain(|n| !(n.operation == operation && n.account.as_deref() == account));
            let cleared = state.notices.len() != before;
            if cleared {
                tracing::info!(
                    surface = operation.surface().as_str(),
                    operation = operation.as_str(),
                    account = account.unwrap_or("-"),
                    "a background failure recovered"
                );
            }
            cleared
        });
    }

    /// A wake-phrase re-arm finished (R410): `Ok(true)` armed the phrase, the
    /// success its own failure waits for; `Ok(false)` had nothing to do — the
    /// phrase already listening, listening off, or the port still refusing —
    /// proves nothing and clears nothing; an error is told with
    /// [`WAKE_NOT_REARMED`].
    pub fn rearm_finished(&self, outcome: Result<bool, &dyn std::fmt::Display>) {
        match outcome {
            Ok(true) => self.recovered(FailureOperation::VoiceRearm, None),
            Ok(false) => {}
            Err(cause) => self.notice(FailureOperation::VoiceRearm, None, WAKE_NOT_REARMED, cause),
        }
    }

    /// The standing failures now, and every change after: read the current
    /// value with `borrow_and_update` before awaiting `changed`, and no change
    /// falls between the two.
    pub fn subscribe(&self) -> watch::Receiver<FailureStateVm> {
        self.state.subscribe()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Arc, Mutex};

    #[derive(Clone, Default)]
    struct Captured(Arc<Mutex<Vec<u8>>>);

    impl std::io::Write for Captured {
        fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
            self.0.lock().expect("captured").extend_from_slice(bytes);
            Ok(bytes.len())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }

    fn sentences(state: &FailureStateVm) -> Vec<(FailureSurface, Option<&str>, &str)> {
        state
            .notices
            .iter()
            .map(|n| (n.surface, n.account.as_deref(), n.sentence.as_str()))
            .collect()
    }

    /// R381: a background failure is both a `WARN` line with its cause and
    /// the sentence the person is shown — never only one of them.
    #[test]
    fn a_background_failure_is_a_warning_and_a_sentence() {
        let captured = Captured::default();
        let writer = captured.clone();
        let subscriber = tracing_subscriber::fmt()
            .with_ansi(false)
            .with_writer(move || writer.clone())
            .finish();
        let failures = Failures::default();
        tracing::subscriber::with_default(subscriber, || {
            failures.notice(
                FailureOperation::PushRegistration,
                Some("acct-1"),
                "keeper could not turn on notifications for tgorka.",
                &"M_FORBIDDEN",
            );
        });

        assert_eq!(
            sentences(&failures.subscribe().borrow()),
            [(
                FailureSurface::Push,
                Some("acct-1"),
                "keeper could not turn on notifications for tgorka."
            )]
        );
        let text = String::from_utf8(captured.0.lock().expect("captured").clone()).expect("utf-8");
        let line = text.lines().next().expect("one line");
        for field in [
            "WARN",
            r#"surface="push""#,
            r#"operation="push-registration""#,
            r#"account="acct-1""#,
            "cause=M_FORBIDDEN",
            "sentence=keeper could not turn on notifications for tgorka.",
        ] {
            assert!(line.contains(field), "{field} missing: {line}");
        }
    }

    /// R387 (FIX 04): a failure told before any webview subscribed is in the
    /// snapshot the first subscriber reads — a startup failure is not lost
    /// to a listener that attached later.
    #[test]
    fn a_failure_before_any_subscriber_is_in_the_first_snapshot() {
        let failures = Failures::default();
        failures.notice(
            FailureOperation::PushRegistration,
            None,
            "Push notifications could not start on this phone.",
            &"plugin missing",
        );
        let mut late = failures.subscribe();
        assert_eq!(
            sentences(&late.borrow_and_update()),
            [(
                FailureSurface::Push,
                None,
                "Push notifications could not start on this phone."
            )]
        );
    }

    /// R387 (FIX 05): the same operation's next success clears its failure,
    /// and a subscriber is told; one account's success leaves another
    /// account's failure standing.
    #[tokio::test]
    async fn a_success_clears_its_own_failure_and_only_its_own() {
        let failures = Failures::default();
        failures.notice(
            FailureOperation::PushRegistration,
            Some("a"),
            PUSH_NOT_REGISTERED,
            &"refused",
        );
        failures.notice(
            FailureOperation::PushRegistration,
            Some("b"),
            PUSH_NOT_REGISTERED,
            &"refused",
        );
        failures.notice(FailureOperation::VoiceRearm, None, "not re-armed", &"busy");
        let mut watcher = failures.subscribe();
        assert_eq!(watcher.borrow_and_update().notices.len(), 3);

        failures.recovered(FailureOperation::PushRegistration, Some("a"));
        watcher.changed().await.expect("told of the recovery");
        assert_eq!(
            sentences(&watcher.borrow_and_update()),
            [
                (FailureSurface::Push, Some("b"), PUSH_NOT_REGISTERED),
                (FailureSurface::Voice, None, "not re-armed"),
            ]
        );

        failures.recovered(FailureOperation::VoiceRearm, None);
        watcher.changed().await.expect("told of the recovery");
        assert_eq!(
            sentences(&watcher.borrow_and_update()),
            [(FailureSurface::Push, Some("b"), PUSH_NOT_REGISTERED)]
        );
    }

    fn standing(failures: &Failures) -> Vec<(FailureSurface, FailureOperation, String)> {
        failures
            .subscribe()
            .borrow()
            .notices
            .iter()
            .map(|n| (n.surface, n.operation, n.sentence.clone()))
            .collect()
    }

    /// R410: a failure is cleared only by the success of the operation that
    /// failed, never by another operation said under the same surface. A
    /// synced stop phrase keeper refused stands through a re-arm that had
    /// nothing to do and through one that armed — neither applied the synced
    /// settings; only applying them clears it. The re-arm's own failure
    /// stands through a re-arm that did nothing, and clears on one that armed.
    #[test]
    fn only_the_operation_that_failed_clears_its_failure() {
        const SYNCED: &str = "A stop phrase from your other device is one keeper cannot use.";
        let synced = (
            FailureSurface::Voice,
            FailureOperation::VoiceSyncedSettings,
            SYNCED.to_owned(),
        );
        let rearm = (
            FailureSurface::Voice,
            FailureOperation::VoiceRearm,
            WAKE_NOT_REARMED.to_owned(),
        );
        let failures = Failures::default();
        failures.notice(
            FailureOperation::VoiceSyncedSettings,
            None,
            SYNCED,
            &"stop refused",
        );

        // keeper back in front, the old phrase still listening: nothing to do.
        failures.rearm_finished(Ok(false));
        assert_eq!(standing(&failures), vec![synced.clone()]);

        failures.rearm_finished(Err(&"the stored phrase is unreadable"));
        assert_eq!(standing(&failures), [synced.clone(), rearm.clone()]);
        failures.rearm_finished(Ok(false));
        assert_eq!(standing(&failures), [synced.clone(), rearm]);

        failures.rearm_finished(Ok(true));
        assert_eq!(standing(&failures), [synced]);

        failures.recovered(FailureOperation::VoiceSyncedSettings, None);
        assert!(standing(&failures).is_empty());
    }
}
