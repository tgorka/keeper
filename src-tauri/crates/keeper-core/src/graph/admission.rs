//! Process-wide admission of graph workers (contract §5 *Initial limits*;
//! R15). One [`Admission`] per process is shared by every domain's
//! [`super::query::GraphReads`], so the bounds hold per process, not per
//! projection root: at most 4 query workers in an agentd (2 in a headless
//! driver), 2 per agent, 1 per context, and at most one builder. Waiting is
//! fair (FIFO) and bounded: at most [`MAX_QUEUED`] queries wait, and a
//! query that is not admitted within its deadline is `query_limit`, never
//! queued without end.

use std::collections::BTreeMap;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, Weak};
use std::time::Duration;

use tokio::sync::{OwnedSemaphorePermit, Semaphore};

use super::query::QueryError;

/// Query workers per agentd process.
pub const AGENTD_WORKERS: usize = 4;
/// Query workers per headless driver process.
pub const HEADLESS_WORKERS: usize = 2;
/// Query workers per agent.
pub const PER_AGENT: usize = 2;
/// Query workers per context.
pub const PER_CONTEXT: usize = 1;
/// Builders per process. A second build is refused, never queued.
pub const MAX_BUILDERS: usize = 1;
/// Queries waiting for admission per process.
pub const MAX_QUEUED: usize = 16;
/// Distinct contexts holding or waiting for a worker at once.
pub const MAX_CONTEXTS: usize = 64;
/// How long a query may wait for admission.
pub const ADMISSION_DEADLINE: Duration = Duration::from_secs(3);

/// Which kind of process this is.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Process {
    /// The agent runtime: [`AGENTD_WORKERS`].
    Agentd,
    /// A headless driver: [`HEADLESS_WORKERS`].
    Headless,
}

/// Who a query worker is for: an agent's context, or a person's device
/// (one principal, one context).
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) struct Principal {
    pub(crate) agent: String,
    pub(crate) context: String,
}

#[derive(Default)]
struct Keys {
    agents: BTreeMap<String, Weak<Semaphore>>,
    contexts: BTreeMap<(String, String, String), Weak<Semaphore>>,
}

/// The process's worker admission.
pub struct Admission {
    global: Arc<Semaphore>,
    queued: Arc<Semaphore>,
    builds: Arc<Semaphore>,
    keys: Mutex<Keys>,
    live: AtomicUsize,
    peak: AtomicUsize,
}

/// Counters of live workers: observations, never enforcement.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AdmissionStats {
    pub live_workers: usize,
    pub peak_workers: usize,
}

/// A query worker's admission, held until the worker is reaped.
pub(crate) struct QueryPermit {
    _context: OwnedSemaphorePermit,
    _agent: OwnedSemaphorePermit,
    _global: OwnedSemaphorePermit,
}

/// A builder's admission.
pub(crate) struct BuildPermit(#[allow(dead_code)] OwnedSemaphorePermit);

/// One live worker, counted while it exists.
pub(crate) struct Live<'a>(&'a Admission);

impl Drop for Live<'_> {
    fn drop(&mut self) {
        self.0.live.fetch_sub(1, Ordering::SeqCst);
    }
}

fn shared<K: Ord + Clone>(
    map: &mut BTreeMap<K, Weak<Semaphore>>,
    key: &K,
    permits: usize,
) -> Arc<Semaphore> {
    if let Some(held) = map.get(key).and_then(Weak::upgrade) {
        return held;
    }
    let fresh = Arc::new(Semaphore::new(permits));
    map.insert(key.clone(), Arc::downgrade(&fresh));
    fresh
}

impl Admission {
    #[must_use]
    pub fn new(process: Process) -> Arc<Self> {
        Arc::new(Self {
            global: Arc::new(Semaphore::new(match process {
                Process::Agentd => AGENTD_WORKERS,
                Process::Headless => HEADLESS_WORKERS,
            })),
            queued: Arc::new(Semaphore::new(MAX_QUEUED)),
            builds: Arc::new(Semaphore::new(MAX_BUILDERS)),
            keys: Mutex::new(Keys::default()),
            live: AtomicUsize::new(0),
            peak: AtomicUsize::new(0),
        })
    }

    #[must_use]
    pub fn stats(&self) -> AdmissionStats {
        AdmissionStats {
            live_workers: self.live.load(Ordering::SeqCst),
            peak_workers: self.peak.load(Ordering::SeqCst),
        }
    }

    /// Admit one query worker for `principal` over `domain`: its context,
    /// then its agent, then the process, in FIFO order each, within
    /// [`ADMISSION_DEADLINE`].
    pub(crate) async fn query(
        &self,
        principal: &Principal,
        domain: &str,
    ) -> Result<QueryPermit, QueryError> {
        let _waiting = self
            .queued
            .clone()
            .try_acquire_owned()
            .map_err(|_| QueryError::Limit)?;
        let (context, agent) = {
            let mut keys = self
                .keys
                .lock()
                .map_err(|_| QueryError::Unavailable("admission"))?;
            keys.agents.retain(|_, w| w.strong_count() > 0);
            keys.contexts.retain(|_, w| w.strong_count() > 0);
            let key = (
                principal.agent.clone(),
                principal.context.clone(),
                domain.to_owned(),
            );
            if !keys.contexts.contains_key(&key) && keys.contexts.len() >= MAX_CONTEXTS {
                return Err(QueryError::Limit);
            }
            (
                shared(&mut keys.contexts, &key, PER_CONTEXT),
                shared(&mut keys.agents, &principal.agent, PER_AGENT),
            )
        };
        let admit = async {
            let a = context.acquire_owned().await;
            let b = agent.acquire_owned().await;
            let c = self.global.clone().acquire_owned().await;
            match (a, b, c) {
                (Ok(a), Ok(b), Ok(c)) => Ok(QueryPermit {
                    _context: a,
                    _agent: b,
                    _global: c,
                }),
                _ => Err(QueryError::Unavailable("admission")),
            }
        };
        tokio::time::timeout(ADMISSION_DEADLINE, admit)
            .await
            .map_err(|_| QueryError::Limit)?
    }

    /// Admit the process's one builder, or refuse at once.
    pub(crate) fn build(&self) -> Result<BuildPermit, QueryError> {
        self.builds
            .clone()
            .try_acquire_owned()
            .map(BuildPermit)
            .map_err(|_| QueryError::Limit)
    }

    /// Count one live worker until the guard drops.
    pub(crate) fn live(&self) -> Live<'_> {
        let now = self.live.fetch_add(1, Ordering::SeqCst) + 1;
        self.peak.fetch_max(now, Ordering::SeqCst);
        Live(self)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn who(agent: &str, context: &str) -> Principal {
        Principal {
            agent: agent.to_owned(),
            context: context.to_owned(),
        }
    }

    /// R15: the bounds are the process's, whatever domain (projection root)
    /// a query is for.
    #[tokio::test(start_paused = true)]
    async fn the_process_bound_holds_across_domains() {
        let admission = Admission::new(Process::Agentd);
        let mut held = Vec::new();
        for (i, domain) in ["a", "b", "c", "d"].iter().enumerate() {
            held.push(
                admission
                    .query(&who(&format!("agent-{i}"), "ctx"), domain)
                    .await
                    .expect("within the process bound"),
            );
        }
        // A fifth worker of a fifth agent on a fifth domain waits, then is refused.
        assert!(matches!(
            admission.query(&who("agent-9", "ctx"), "e").await,
            Err(QueryError::Limit)
        ));
        held.pop();
        assert!(admission.query(&who("agent-9", "ctx"), "e").await.is_ok());
    }

    #[tokio::test(start_paused = true)]
    async fn per_agent_and_per_context_bounds_hold() {
        let admission = Admission::new(Process::Agentd);
        let first = admission.query(&who("a", "c1"), "d").await.expect("first");
        assert!(matches!(
            admission.query(&who("a", "c1"), "d").await,
            Err(QueryError::Limit)
        ));
        let _second = admission
            .query(&who("a", "c2"), "d")
            .await
            .expect("other context");
        assert!(matches!(
            admission.query(&who("a", "c3"), "d").await,
            Err(QueryError::Limit)
        ));
        drop(first);
        assert!(admission.query(&who("a", "c1"), "d").await.is_ok());
    }

    #[tokio::test(start_paused = true)]
    async fn headless_holds_two_and_one_builder() {
        let admission = Admission::new(Process::Headless);
        let _a = admission.query(&who("p", "x"), "d1").await.expect("a");
        let _b = admission.query(&who("q", "x"), "d2").await.expect("b");
        assert!(matches!(
            admission.query(&who("r", "x"), "d3").await,
            Err(QueryError::Limit)
        ));
        let build = admission.build().expect("builder");
        assert!(matches!(admission.build(), Err(QueryError::Limit)));
        drop(build);
        assert!(admission.build().is_ok());
    }

    /// Released contexts are forgotten, so a long-lived process never
    /// runs out of context slots.
    #[tokio::test(start_paused = true)]
    async fn released_contexts_do_not_accumulate() {
        let admission = Admission::new(Process::Agentd);
        for i in 0..(MAX_CONTEXTS * 3) {
            let permit = admission
                .query(&who(&format!("agent-{}", i % 3), &format!("ctx-{i}")), "d")
                .await
                .expect("a released context frees its slot");
            drop(permit);
        }
    }
}
