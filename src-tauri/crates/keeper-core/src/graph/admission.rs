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
use std::sync::{Arc, Mutex, OnceLock, Weak};
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
    contexts: BTreeMap<(String, String), Weak<Semaphore>>,
}

/// The process's worker admission.
pub struct Admission {
    global: Arc<Semaphore>,
    queued: Arc<Semaphore>,
    builds: Arc<Semaphore>,
    keys: Mutex<Keys>,
    live: AtomicUsize,
    peak: AtomicUsize,
    attempts: AtomicUsize,
    calls: AtomicUsize,
}

/// Counters of workers: observations, never enforcement.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AdmissionStats {
    /// Workers alive now.
    pub live_workers: usize,
    /// The most workers alive at once (a high-water mark, not a count).
    pub peak_workers: usize,
    /// Every worker spawn attempted, counted before the attempt whether or
    /// not it succeeds: zero means no worker was ever started.
    pub spawn_attempts: usize,
    /// Every real worker-process `spawn()` call: counted after step 5 (the
    /// spawning thread's last check) passed and immediately before the call,
    /// whether the kernel then refuses it or not. Zero means no worker
    /// process spawn was ever asked of the kernel, which an attempt refused
    /// at step 5 never does. It counts nothing else: the process's own
    /// threads are created with `clone` too and are not counted here.
    pub spawn_calls: usize,
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

/// The process's one admission, once chosen.
static PROCESS: OnceLock<(Process, Arc<Admission>)> = OnceLock::new();

impl Admission {
    /// The process's admission (R15): created on first use for `process`,
    /// the same instance on every later call; a different `Process` asked
    /// for later is refused, so no consumer can build a second set of
    /// bounds.
    pub fn process(process: Process) -> Result<Arc<Self>, QueryError> {
        let (held, admission) = PROCESS.get_or_init(|| (process, Self::new(process)));
        if *held != process {
            return Err(QueryError::Unavailable("admission.process"));
        }
        Ok(admission.clone())
    }

    #[must_use]
    pub(crate) fn new(process: Process) -> Arc<Self> {
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
            attempts: AtomicUsize::new(0),
            calls: AtomicUsize::new(0),
        })
    }

    #[must_use]
    pub fn stats(&self) -> AdmissionStats {
        AdmissionStats {
            live_workers: self.live.load(Ordering::SeqCst),
            peak_workers: self.peak.load(Ordering::SeqCst),
            spawn_attempts: self.attempts.load(Ordering::SeqCst),
            spawn_calls: self.calls.load(Ordering::SeqCst),
        }
    }

    /// Admit one query worker for `principal`: its context,
    /// then its agent, then the process, in FIFO order each, within
    /// [`ADMISSION_DEADLINE`].
    pub(crate) async fn query(&self, principal: &Principal) -> Result<QueryPermit, QueryError> {
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
            // One per context, whatever domain it reads: a context may hold
            // several admitted source domains.
            let key = (principal.agent.clone(), principal.context.clone());
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

    /// Count one spawn attempt; called immediately before the spawn.
    pub(crate) fn attempt(&self) {
        self.attempts.fetch_add(1, Ordering::SeqCst);
    }

    /// Count one real spawn call; called by [`super::runtime::Runtime::spawn`]
    /// after step 5 passed and immediately before `spawn()`.
    pub(crate) fn call(&self) {
        self.calls.fetch_add(1, Ordering::SeqCst);
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
        for (i, context) in ["a", "b", "c", "d"].iter().enumerate() {
            held.push(
                admission
                    .query(&who(&format!("agent-{i}"), context))
                    .await
                    .expect("within the process bound"),
            );
        }
        // A fifth worker of a fifth agent on a fifth domain waits, then is refused.
        assert!(matches!(
            admission.query(&who("agent-9", "ctx")).await,
            Err(QueryError::Limit)
        ));
        held.pop();
        assert!(admission.query(&who("agent-9", "ctx")).await.is_ok());
    }

    #[tokio::test(start_paused = true)]
    async fn per_agent_and_per_context_bounds_hold() {
        let admission = Admission::new(Process::Agentd);
        let first = admission.query(&who("a", "c1")).await.expect("first");
        assert!(matches!(
            admission.query(&who("a", "c1")).await,
            Err(QueryError::Limit)
        ));
        let _second = admission
            .query(&who("a", "c2"))
            .await
            .expect("other context");
        assert!(matches!(
            admission.query(&who("a", "c3")).await,
            Err(QueryError::Limit)
        ));
        drop(first);
        assert!(admission.query(&who("a", "c1")).await.is_ok());
    }

    /// One per context, not per context and domain: a context admitted to a
    /// second domain waits for its first query (the key holds no domain).
    #[tokio::test(start_paused = true)]
    async fn a_second_domain_in_the_same_context_waits() {
        let admission = Admission::new(Process::Agentd);
        let work = admission
            .query(&who("a", "ctx"))
            .await
            .expect("work domain");
        assert!(matches!(
            admission.query(&who("a", "ctx")).await,
            Err(QueryError::Limit)
        ));
        drop(work);
        assert!(admission.query(&who("a", "ctx")).await.is_ok());
    }

    #[test]
    fn the_process_admission_is_one_instance_of_one_kind() {
        let first = Admission::process(Process::Headless).expect("first");
        let again = Admission::process(Process::Headless).expect("same kind");
        assert!(Arc::ptr_eq(&first, &again));
        assert!(matches!(
            Admission::process(Process::Agentd),
            Err(QueryError::Unavailable("admission.process"))
        ));
    }

    #[tokio::test(start_paused = true)]
    async fn headless_holds_two_and_one_builder() {
        let admission = Admission::new(Process::Headless);
        let _a = admission.query(&who("p", "x")).await.expect("a");
        let _b = admission.query(&who("q", "x")).await.expect("b");
        assert!(matches!(
            admission.query(&who("r", "x")).await,
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
                .query(&who(&format!("agent-{}", i % 3), &format!("ctx-{i}")))
                .await
                .expect("a released context frees its slot");
            drop(permit);
        }
    }
}
