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
//! 4. once the replica's scope purge is observed (the server's scope
//!    revocation, which the Syncular client applies atomically), the fence
//!    records `purged` ([`Lifecycle::observe_purge`]).
//!
//! From step 1 on, [`Lifecycle::gate`] refuses: callers check it before any
//! read, result, receipt, queue or history disclosure. Steps 2 and 3 are
//! resumable: [`Lifecycle::open`] finds a persisted fence and starts
//! locked, and [`Lifecycle::resume`] runs them again. A failed step is a
//! locked failure, never an unlock.
//!
//! Only a regrant clears the fence ([`Lifecycle::regrant`]), and only from
//! `purged`, with a successful read authority for this domain, granted
//! subscriptions and a new, unrevoked [`GraphReads`]. A fence learned from a
//! refusal before the server's revoke frame stays `revoked` and never
//! unlocks. Locked entries stay locked after a regrant: they are never sent
//! again, only resolved by their own outcome or discarded.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, MutexGuard, PoisonError};

use super::protocol::wire::ReadStatus;
use super::query::{GraphReads, QueryError};
use super::queue::{FenceState, GraphQueue, QueueError};
use super::source::{Checkpoint, ScopeState};

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
}

impl Lifecycle {
    /// Take over `queue`. A fence persisted before a restart locks at once.
    pub fn open(queue: GraphQueue) -> Result<Self, LifecycleError> {
        let locked = queue.fence()?.is_some();
        Ok(Self {
            queue: Mutex::new(queue),
            locked: AtomicBool::new(locked),
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

    /// The fence's persisted state.
    pub fn fence(&self) -> Result<Option<FenceState>, LifecycleError> {
        Ok(self.queue().fence()?)
    }

    /// A revocation signal: lock, persist the fence, lock the queue, then
    /// kill the domain's workers and purge its projection (`reads`, when
    /// this process holds them). Idempotent.
    pub async fn revoke(
        &self,
        reads: Option<&GraphReads>,
        now_ms: i64,
    ) -> Result<(), LifecycleError> {
        self.locked.store(true, Ordering::SeqCst);
        self.queue().persist_fence(now_ms)?;
        self.cleanup(reads, now_ms).await
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
        self.cleanup(reads, now_ms).await
    }

    async fn cleanup(&self, reads: Option<&GraphReads>, now_ms: i64) -> Result<(), LifecycleError> {
        let locked = self.queue().disable_open(now_ms)?;
        if locked > 0 {
            tracing::info!(locked, "graph: a revocation locked queued intents");
        }
        if let Some(reads) = reads {
            reads.revoke().await?;
        }
        Ok(())
    }

    /// Record the replica's scope purge once its subscriptions read
    /// revoked (`checkpoint`): whether the fence is now `purged`.
    pub fn observe_purge(
        &self,
        checkpoint: Checkpoint,
        now_ms: i64,
    ) -> Result<bool, LifecycleError> {
        let queue = self.queue();
        match queue.fence()? {
            None => Err(LifecycleError::NotFenced),
            Some(FenceState::Purged) => Ok(true),
            Some(FenceState::Revoked) if checkpoint.scope == ScopeState::Revoked => {
                queue.mark_purged(now_ms)?;
                Ok(true)
            }
            Some(FenceState::Revoked) => Ok(false),
        }
    }

    /// A regrant: clear the fence only from `purged`, with `status` a
    /// successful read authority for this domain, `checkpoint` granted
    /// subscriptions and `reads` a new, unrevoked instance. Locked entries
    /// stay locked.
    pub fn regrant(
        &self,
        status: &ReadStatus,
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
        if status.domain != *domain
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

#[cfg(test)]
#[path = "lifecycle_tests.rs"]
mod tests;
