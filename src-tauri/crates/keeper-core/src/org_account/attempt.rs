//! The boundary every sign-in and sync run of the organisation account
//! crosses (Epic 100, R387, R410): where a run is cancelled, which step it
//! was on, and how the person's attempt ended.
//!
//! A sign-in ends closed two ways, and both pass through here. The person
//! closes the platform's browser sheet: the flow answers `Cancelled`, the
//! step awaiting it ([`Attempts::step`]) ends with [`AccountError::Cancelled`]
//! and the attempt is closed. Or the person presses Cancel in keeper: the
//! shell's interrupt calls [`Attempts::interrupt`], the run in flight
//! ([`Attempts::run`]) is dropped at whatever step it reached, and that step
//! is logged as closed. Either way the step is logged exactly once — through
//! [`step_failed`], the line every failed step writes — and the attempt is
//! [`AccountAttemptVm::Closed`], never `Done`, whatever the account's state
//! was before. The shell holds one [`Attempts`] and only calls it.

use std::future::Future;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Mutex, PoisonError};

use tokio::sync::Notify;

use super::state::{step_failed, AccountAttemptVm, AccountProblem, AccountStep};
use super::AccountError;

/// How a run under [`Attempts::run`] ended.
#[derive(Debug, PartialEq, Eq)]
pub enum Run<T> {
    /// The work finished, with its own outcome.
    Finished(T),
    /// The person cancelled it while it ran; the step it was on is logged.
    Cancelled,
    /// A stop came while it waited to start; it never started.
    NotStarted,
}

impl Run<AccountAttemptVm> {
    /// How the person's attempt ended: the work's own outcome when it
    /// finished, closed when the person cancelled it or it never started.
    pub fn attempt(self) -> AccountAttemptVm {
        match self {
            Self::Finished(attempt) => attempt,
            Self::Cancelled | Self::NotStarted => AccountAttemptVm::Closed,
        }
    }
}

/// A step that ended the attempt: how it ended, the problem the account
/// shows (none for a closed sheet), and whether the identity provider or
/// policy refused — the person's layers then stop applying.
#[derive(Debug)]
pub struct StepEnded {
    pub attempt: AccountAttemptVm,
    pub problem: Option<AccountProblem>,
    pub refused: bool,
}

/// The cancellation state of the account's runs: one per process, held by
/// the shell.
#[derive(Debug, Default)]
pub struct Attempts {
    /// Ends the run in flight.
    cancel: Notify,
    /// The step the run in flight is on, so a cancel can log it.
    step: Mutex<Option<AccountStep>>,
    /// Bumped by every stop. Work that queued before the bump never starts:
    /// `Notify` only reaches work already running.
    epoch: AtomicU64,
}

impl Attempts {
    /// The stop count to capture before waiting to start; see [`Self::run`].
    pub fn epoch(&self) -> u64 {
        self.epoch.load(Ordering::SeqCst)
    }

    /// Stop everything: work still queued never starts and the run in
    /// flight is cancelled at the step it is on.
    pub fn interrupt(&self) {
        self.epoch.fetch_add(1, Ordering::SeqCst);
        self.cancel.notify_waiters();
    }

    /// The run in flight has reached `step`.
    pub fn at_step(&self, step: AccountStep) {
        self.set_step(Some(step));
    }

    fn set_step(&self, step: Option<AccountStep>) {
        *self.step.lock().unwrap_or_else(PoisonError::into_inner) = step;
    }

    /// Run one step that can end the attempt — a browser sign-in, the
    /// forge's consent — for `account`: an error is logged once at `WARN`
    /// and ends the attempt, closed when the person closed the sheet.
    pub async fn step<T>(
        &self,
        step: AccountStep,
        account: &str,
        work: impl Future<Output = Result<T, AccountError>>,
    ) -> Result<T, StepEnded> {
        self.at_step(step);
        work.await.map_err(|error| StepEnded {
            attempt: AccountAttemptVm::of_error(&error),
            refused: matches!(error, AccountError::Refused(_)),
            problem: step_failed(step, account, error),
        })
    }

    /// Run `work` until it finishes or the person cancels it, unless a stop
    /// came while it queued (`queued_at` is the [`Self::epoch`] from before
    /// it waited to start). A cancel logs the step the run had reached for
    /// `account` once, as closed; the work is dropped without being polled
    /// again, so a step it was awaiting logs nothing more.
    pub async fn run<T>(
        &self,
        queued_at: u64,
        account: Option<&str>,
        work: impl Future<Output = T>,
    ) -> Run<T> {
        let cancelled = self.cancel.notified();
        tokio::pin!(cancelled);
        cancelled.as_mut().enable();
        if self.epoch() != queued_at {
            return Run::NotStarted;
        }
        self.set_step(None);
        let ran = tokio::select! {
            biased;
            () = &mut cancelled => {
                let step = self.step.lock().unwrap_or_else(PoisonError::into_inner).take();
                if let (Some(step), Some(account)) = (step, account) {
                    step_failed(step, account, AccountError::Cancelled);
                }
                Run::Cancelled
            }
            finished = work => Run::Finished(finished),
        };
        self.set_step(None);
        ran
    }
}
