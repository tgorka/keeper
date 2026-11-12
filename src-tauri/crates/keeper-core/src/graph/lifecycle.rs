//! One server, domain and actor's revocation fence (112.1): once this
//! device learns that its graph authority for a domain was revoked, graph
//! exposure is locked first and cleaned up after, and a crash at any point
//! restarts locked.
//!
//! On a revocation signal ([`Lifecycle::revoke`]), in this order:
//! 1. the in-memory gate closes and the `graph_fence` row is committed in
//!    the domain's queue file, before anything else;
//! 2. every open queue entry is locked ([`super::queue::State::Disabled`]),
//!    identity and payload kept;
//! 3. [`GraphReads::revoke`] kills and reaps in-flight workers and purges
//!    the projection store;
//! 4. once the replica's scope purge is observed for every graph
//!    subscription (the graph tables, the receipts and the proposals: the
//!    server's scope revocation, which the Syncular client applies
//!    atomically), the fence records `purged` ([`Lifecycle::observe_purge`]).
//!
//! From step 1 on, [`Lifecycle::gate`] refuses: callers check it before any
//! read, result, receipt, queue or history disclosure. Steps 2 and 3 are
//! resumable: [`Lifecycle::open`] finds a persisted fence and starts
//! locked, and [`Lifecycle::resume`] runs them again. A failed step is a
//! locked failure, never an unlock.
//!
//! Only a regrant clears the fence ([`Lifecycle::regrant`] for a person,
//! [`Lifecycle::regrant_admitted`] for an agent), and only from `purged`,
//! with fresh authority for this domain (a successful `read_status`, or an
//! unexpired `admit_read`), granted subscriptions and a new, unrevoked
//! [`GraphReads`]. A fence learned from a
//! refusal before the server's revoke frame stays `revoked` and never
//! unlocks. Locked entries stay locked after a regrant: they are never sent
//! again, only resolved by their own outcome or discarded.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, MutexGuard, PoisonError};

use super::outcome::GraphOutcome;
use super::protocol::wire::{Authority, ReadStatus};
use super::query::{AdmittedRead, GraphReads, QueryError};
use super::queue::{self, AuthorityRow, FenceState, GraphQueue, Next, QueueError, State};
use super::source::{Checkpoint, Revocation, ScopeState};

/// Why a lifecycle step failed. The domain stays locked.
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum LifecycleError {
    #[error(transparent)]
    Queue(#[from] QueueError),
    /// The projection could not be purged.
    #[error(transparent)]
    Query(#[from] QueryError),
    /// No fence stands: there is nothing to resume or clear.
    #[error("no revocation fence stands for this domain")]
    NotFenced,
    /// The replica's scope purge has not been observed: the fence stays.
    #[error("the replica's scope purge has not been observed")]
    PurgePending,
    /// The regrant's evidence is not fresh authority for this domain.
    #[error("no fresh read authority for this domain")]
    NotFresh,
}

/// One server, domain and actor's graph lifecycle over its queue file.
pub struct Lifecycle {
    queue: Mutex<GraphQueue>,
    locked: AtomicBool,
    /// The `observed_at` of a kept authority found to be from the future:
    /// never trusted again in this process, whether or not its durable
    /// drop committed, until a successful online `read_status` is kept.
    /// Not persisted: after a restart only a committed drop keeps it
    /// untrusted (owner decision 112.1 G3, 2026-11-11).
    distrusted: Mutex<Option<i64>>,
}

impl Lifecycle {
    /// Take over `queue`. A fence persisted before a restart locks at once.
    pub fn open(queue: GraphQueue) -> Result<Self, LifecycleError> {
        let locked = queue.fence()?.is_some();
        Ok(Self {
            queue: Mutex::new(queue),
            locked: AtomicBool::new(locked),
            distrusted: Mutex::new(None),
        })
    }

    fn queue(&self) -> MutexGuard<'_, GraphQueue> {
        self.queue.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// Whether graph exposure of this domain is locked.
    #[must_use]
    pub fn is_locked(&self) -> bool {
        self.locked.load(Ordering::SeqCst)
    }

    /// The disclosure gate: `forbidden` while locked.
    pub fn gate(&self) -> Result<(), QueryError> {
        if self.is_locked() {
            return Err(QueryError::Forbidden);
        }
        Ok(())
    }

    /// The domain this lifecycle fences.
    #[must_use]
    pub fn domain(&self) -> String {
        self.queue().binding().domain.clone()
    }

    /// Keep the authority a successful online `read_status` answered at
    /// `observed_at_ms` (device wall clock, UTC ms): the only writer of the
    /// row offline reads compare with.
    pub fn record_authority(
        &self,
        authority: &Authority,
        observed_at_ms: i64,
    ) -> Result<(), LifecycleError> {
        self.queue().record_authority(authority, observed_at_ms)?;
        *self
            .distrusted
            .lock()
            .unwrap_or_else(PoisonError::into_inner) = None;
        Ok(())
    }

    /// The last kept read authority.
    pub fn authority(&self) -> Result<Option<AuthorityRow>, LifecycleError> {
        Ok(self.queue().authority()?)
    }

    /// Drop the kept authority observed at `observed_at_ms`: it was found
    /// to be from the future, and only a successful online `read_status`
    /// keeps one again ([`GraphQueue::invalidate_authority`]). It is
    /// distrusted first, in memory, so that a drop that fails to commit
    /// still fails closed ([`Lifecycle::trusts`]).
    pub fn invalidate_authority(&self, observed_at_ms: i64) -> Result<(), LifecycleError> {
        *self
            .distrusted
            .lock()
            .unwrap_or_else(PoisonError::into_inner) = Some(observed_at_ms);
        Ok(self.queue().invalidate_authority(observed_at_ms)?)
    }

    /// Whether the kept authority observed at `observed_at_ms` may still
    /// authorise an offline read: not one found in the future since the last
    /// successful online `read_status`.
    #[must_use]
    pub fn trusts(&self, observed_at_ms: i64) -> bool {
        *self
            .distrusted
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            != Some(observed_at_ms)
    }

    /// The fence's persisted state.
    pub fn fence(&self) -> Result<Option<FenceState>, LifecycleError> {
        Ok(self.queue().fence()?)
    }

    /// How many queued intents are in each state (domain-safe; allowed
    /// while locked).
    pub fn counts(
        &self,
    ) -> Result<std::collections::BTreeMap<super::queue::State, usize>, LifecycleError> {
        Ok(self.queue().counts()?)
    }

    /// A revocation signal: lock, persist the fence, lock the queue, then
    /// kill the domain's workers and purge its projection (`reads`, when
    /// this process holds them). Idempotent.
    pub async fn revoke(
        &self,
        reads: Option<&GraphReads>,
        now_ms: i64,
    ) -> Result<(), LifecycleError> {
        self.lock(now_ms)?;
        if let Some(reads) = reads {
            reads.revoke().await?;
        }
        Ok(())
    }

    /// The synchronous half of [`Lifecycle::revoke`], for a signal learned
    /// where no projection is held (a send or a receipt lookup): close the
    /// gate, persist the fence, lock every open entry. The projection's
    /// purge is left to the next [`Lifecycle::resume`] (the fence stands
    /// until then, and a restart starts locked).
    pub fn lock(&self, now_ms: i64) -> Result<(), LifecycleError> {
        self.locked.store(true, Ordering::SeqCst);
        let queue = self.queue();
        queue.persist_fence(now_ms)?;
        Self::disable(&queue, now_ms)
    }

    /// Run the fence's cleanup again (after a restart, or a failed step):
    /// lock every open entry and purge the projection.
    pub async fn resume(
        &self,
        reads: Option<&GraphReads>,
        now_ms: i64,
    ) -> Result<(), LifecycleError> {
        if self.fence()?.is_none() {
            return Err(LifecycleError::NotFenced);
        }
        self.locked.store(true, Ordering::SeqCst);
        Self::disable(&self.queue(), now_ms)?;
        if let Some(reads) = reads {
            reads.revoke().await?;
        }
        Ok(())
    }

    fn disable(queue: &GraphQueue, now_ms: i64) -> Result<(), LifecycleError> {
        let locked = queue.disable_open(now_ms)?;
        if locked > 0 {
            tracing::info!(locked, "graph: a revocation locked queued intents");
        }
        Ok(())
    }

    /// Apply what `outcome` means for the entry `request_id` in `from`
    /// ([`queue::next`], the one policy) and answer the state it is in now.
    /// A revoking outcome locks the domain first: the fence is persisted and
    /// every open entry disabled before the outcome itself is recorded. A
    /// transport failure, `graph_unavailable` or an unknown outcome leaves
    /// the entry where it is. Allowed while locked: an entry is always
    /// resolved by its own outcome.
    pub fn settle(
        &self,
        request_id: &str,
        from: State,
        outcome: &GraphOutcome,
        now_ms: i64,
    ) -> Result<State, LifecycleError> {
        let next = queue::next(from, outcome);
        // A locked entry is only resolved by its own outcome (or withdrawn);
        // nothing else moves it, and a further refusal only re-locks.
        if from == State::Disabled && !matches!(next, Next::Resolve { .. } | Next::Revoke) {
            return Ok(from);
        }
        match next {
            Next::Resolve {
                to,
                resolution,
                revokes,
            } => {
                let mut from = from;
                if revokes {
                    self.lock(now_ms)?;
                    if from != State::Disabled && !from.is_terminal() {
                        from = State::Disabled;
                    }
                }
                self.queue()
                    .resolve(request_id, from, to, &resolution, now_ms)?;
                Ok(to)
            }
            Next::Revoke => {
                self.lock(now_ms)?;
                Ok(State::Disabled)
            }
            Next::Mark(to) if to == from => Ok(from),
            Next::Mark(to) => {
                self.queue()
                    .mark(request_id, from, to, Some(outcome_cause(outcome)), now_ms)?;
                Ok(to)
            }
            Next::RefuseLocally => {
                self.queue()
                    .refuse_locally(request_id, outcome_cause(outcome), now_ms)?;
                Ok(State::RefusedLocally)
            }
            Next::Retry(_) | Next::Lookup => Ok(from),
        }
    }

    /// Record the replica's scope purge once every graph subscription it
    /// holds — the graph tables, the receipts and the proposals — reads
    /// revoked (`revocation`, [`super::source::revocation_with`]): whether
    /// the fence is now `purged`. A partial revocation keeps it `revoked`.
    pub fn observe_purge(
        &self,
        revocation: Revocation,
        now_ms: i64,
    ) -> Result<bool, LifecycleError> {
        let queue = self.queue();
        match queue.fence()? {
            None => Err(LifecycleError::NotFenced),
            Some(FenceState::Purged) => Ok(true),
            Some(FenceState::Revoked) if revocation == Revocation::Purged => {
                queue.mark_purged(now_ms)?;
                Ok(true)
            }
            Some(FenceState::Revoked) => Ok(false),
        }
    }

    /// A person's regrant: clear the fence only from `purged`, with
    /// `status` a successful read authority for this domain, `checkpoint`
    /// granted subscriptions and `reads` a new, unrevoked instance. Locked
    /// entries stay locked.
    pub fn regrant(
        &self,
        status: &ReadStatus,
        checkpoint: Checkpoint,
        reads: &GraphReads,
    ) -> Result<(), LifecycleError> {
        self.clear(|domain| status.domain == domain, checkpoint, reads)
    }

    /// An agent's regrant (CC-9: "a successful `read_status`/`admit_read`"):
    /// an agent's credential is refused `graph.read_status`, so its fresh
    /// authority is the server's `graph.admit_read` answer, `read`, for this
    /// domain and not yet expired on this device's clock. Otherwise exactly
    /// [`Lifecycle::regrant`] (one shared rule): only from `purged`
    /// ([`LifecycleError::PurgePending`] while `revoked`,
    /// [`LifecycleError::NotFenced`] with no fence), and
    /// [`LifecycleError::NotFresh`] unless `checkpoint` is granted
    /// subscriptions and `reads` a new, unrevoked instance of this domain.
    /// Locked entries stay locked.
    ///
    /// Neither [`AdmittedRead::admit`] nor [`GraphReads::open`] consults the
    /// lifecycle: both work while the domain is fenced, which is how a caller
    /// obtains the evidence. The caller re-registers revoked subscriptions
    /// only after [`Lifecycle::observe_purge`] returned `true` (`purged`):
    /// re-registered earlier, they read granted before the purge was seen,
    /// and the fence stays `revoked` for good.
    pub fn regrant_admitted(
        &self,
        read: &AdmittedRead,
        checkpoint: Checkpoint,
        reads: &GraphReads,
    ) -> Result<(), LifecycleError> {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(u64::MAX, |d| {
                u64::try_from(d.as_millis()).unwrap_or(u64::MAX)
            });
        let unexpired = read.expires_at_ms() > now;
        self.clear(
            |domain| unexpired && read.domain() == domain,
            checkpoint,
            reads,
        )
    }

    /// Clear the fence for a regrant whose authority `fresh` accepts for
    /// this domain (the one rule both forms share).
    fn clear(
        &self,
        fresh: impl FnOnce(&str) -> bool,
        checkpoint: Checkpoint,
        reads: &GraphReads,
    ) -> Result<(), LifecycleError> {
        let queue = self.queue();
        match queue.fence()? {
            None => return Err(LifecycleError::NotFenced),
            Some(FenceState::Revoked) => return Err(LifecycleError::PurgePending),
            Some(FenceState::Purged) => {}
        }
        let domain = &queue.binding().domain;
        if !fresh(domain)
            || reads.domain() != domain
            || reads.is_revoked()
            || checkpoint.scope != ScopeState::Granted
        {
            return Err(LifecycleError::NotFresh);
        }
        queue.clear_fence()?;
        self.locked.store(false, Ordering::SeqCst);
        Ok(())
    }

    /// Use the queue while not locked (enqueue, restore, list, resolve).
    pub fn with_queue<T>(
        &self,
        f: impl FnOnce(&mut GraphQueue) -> Result<T, QueueError>,
    ) -> Result<T, LifecycleError> {
        if self.is_locked() {
            return Err(QueueError::Fenced.into());
        }
        Ok(f(&mut self.queue())?)
    }
}

/// The domain-safe cause an outcome leaves on an entry it moves.
fn outcome_cause(outcome: &GraphOutcome) -> &'static str {
    match outcome {
        GraphOutcome::Pending { cause } => cause.as_str(),
        GraphOutcome::PendingVisibility { .. } => "pending_visibility",
        _ => "outcome",
    }
}
#[cfg(test)]
#[path = "lifecycle_tests.rs"]
mod tests;
