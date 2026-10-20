//! Who owns the merged inbox's producers (story 102.5; R519, review
//! R102L7-01, R102L7-02).
//!
//! An inbox subscription starts three producers per account — its room
//! list, its Spaces, its palette rooms — while it is still acquiring the
//! next account's room list. A sign-out that runs meanwhile must find them,
//! and so must a newer subscription that replaces this one. So every
//! producer is spawned held, registered here under its account and its
//! subscription's generation, and let run only if, at that moment, its
//! account's lifetime is live and its subscription is the newest: a
//! teardown that ended the lifetime first finds nothing to stop, one that
//! ends it later finds the producer here.
//!
//! - [`InboxOwners::take_account`] is a teardown's: every producer of the
//!   account, of the subscription published and of one still being built.
//! - [`InboxOwners::begin`] is a new subscription's: it becomes the newest,
//!   and every producer of an older one is handed back to be stopped. An
//!   older subscription still being built then registers nothing more, and
//!   never replaces the newer one's merger or touches what is shared.
//! - [`Owner::holds`] is what a running producer checks before each effect
//!   — an emission, a palette write, its readiness.
//!
//! Whoever takes producers out aborts *and* awaits them ([`stop`]): a
//! dropped `JoinHandle` detaches its task rather than ending it.

use std::collections::HashMap;
use std::future::Future;
use std::sync::{Arc, Mutex, MutexGuard};

use tokio::sync::oneshot;
use tokio::task::JoinHandle;

use super::lifetime::Lifetime;

/// One producer task: whose it is and which subscription started it.
#[derive(Debug)]
struct Owned {
    account: String,
    generation: u64,
    task: JoinHandle<()>,
}

#[derive(Debug, Default)]
struct State {
    /// The newest inbox subscription's generation; `0` before the first.
    newest: u64,
    /// Every producer registered and not taken out yet.
    producers: Vec<Owned>,
    /// Producer tasks of each account that started and have not ended.
    running: HashMap<String, usize>,
}

impl State {
    fn take(&mut self, mut which: impl FnMut(&Owned) -> bool) -> Vec<JoinHandle<()>> {
        let (taken, kept): (Vec<Owned>, Vec<Owned>) = std::mem::take(&mut self.producers)
            .into_iter()
            .partition(|owned| which(owned));
        self.producers = kept;
        taken.into_iter().map(|owned| owned.task).collect()
    }
}

/// Every inbox producer of every account, and which subscription is the
/// newest.
#[derive(Debug, Default, Clone)]
pub(crate) struct InboxOwners {
    state: Arc<Mutex<State>>,
}

/// What a producer checks before each effect: its account's lifetime is
/// still live and its subscription is still the newest.
#[derive(Debug, Clone)]
pub(crate) struct Owner {
    owners: InboxOwners,
    generation: u64,
    life: Lifetime,
}

/// A started producer task, counted as running until it ends or is aborted.
struct Running {
    state: Arc<Mutex<State>>,
    account: String,
}

impl Running {
    fn start(state: &Arc<Mutex<State>>, account: &str) -> Running {
        *lock(state).running.entry(account.to_owned()).or_default() += 1;
        Running {
            state: Arc::clone(state),
            account: account.to_owned(),
        }
    }
}

impl Drop for Running {
    fn drop(&mut self) {
        let mut state = lock(&self.state);
        if let Some(count) = state.running.get_mut(&self.account) {
            *count = count.saturating_sub(1);
            if *count == 0 {
                state.running.remove(&self.account);
            }
        }
    }
}

fn lock(state: &Mutex<State>) -> MutexGuard<'_, State> {
    // Handles and counts: a panic mid-update tears nothing.
    state.lock().unwrap_or_else(|p| p.into_inner())
}

impl InboxOwners {
    fn state(&self) -> MutexGuard<'_, State> {
        lock(&self.state)
    }

    /// A new inbox subscription begins: its generation, now the newest, and
    /// every producer of the older ones, to be [`stop`]ped.
    pub(crate) fn begin(&self) -> (u64, Vec<JoinHandle<()>>) {
        let mut state = self.state();
        state.newest += 1;
        let generation = state.newest;
        (
            generation,
            state.take(|owned| owned.generation < generation),
        )
    }

    /// Subscription `generation` ends if it is still the newest: none is
    /// then, and its producers are handed back to be [`stop`]ped, with the
    /// generation that is current now. `None` when a newer one began (it
    /// took them).
    pub(crate) fn retire(&self, generation: u64) -> Option<(u64, Vec<JoinHandle<()>>)> {
        let mut state = self.state();
        if state.newest != generation {
            return None;
        }
        state.newest += 1;
        let none = state.newest;
        Some((none, state.take(|owned| owned.generation <= generation)))
    }

    /// Subscription `generation` is still the newest.
    pub(crate) fn is_current(&self, generation: u64) -> bool {
        self.state().newest == generation
    }

    /// Run `change` of something every subscription shares (the launch's
    /// readiness, the palette) only while `generation` is the newest, so no
    /// subscription that began after it is undone (review R102L7-02).
    pub(crate) fn if_current<R>(&self, generation: u64, change: impl FnOnce() -> R) -> Option<R> {
        let state = self.state();
        (state.newest == generation).then(change)
    }

    /// What a producer of subscription `generation` checks, `life` its
    /// account's lifetime as the subscription took it.
    pub(crate) fn owner(&self, generation: u64, life: &Lifetime) -> Owner {
        Owner {
            owners: self.clone(),
            generation,
            life: life.clone(),
        }
    }

    /// Spawn `producer` for `owner`'s account, held; register it, and let it
    /// run, only if its lifetime is live and its subscription the newest —
    /// both checked under the lock a teardown and a newer subscription take
    /// producers out under. Refused, it is aborted and awaited without
    /// having run, and this answers false.
    pub(crate) async fn spawn(
        &self,
        owner: &Owner,
        producer: impl Future<Output = ()> + Send + 'static,
    ) -> bool {
        let (start, started) = oneshot::channel::<()>();
        let (state, account) = (Arc::clone(&self.state), owner.life.account().to_owned());
        let task = tokio::spawn(async move {
            if started.await.is_err() {
                return;
            }
            let _running = Running::start(&state, &account);
            producer.await;
        });
        let refused = {
            let mut state = self.state();
            if state.newest == owner.generation && owner.life.is_live() {
                state.producers.push(Owned {
                    account: owner.life.account().to_owned(),
                    generation: owner.generation,
                    task,
                });
                // Let it run while still under the lock: a teardown that
                // takes it out next aborts a task that may have begun.
                let _ = start.send(());
                None
            } else {
                Some(task)
            }
        };
        match refused {
            Some(task) => {
                stop(vec![task]).await;
                false
            }
            None => true,
        }
    }

    /// Every producer of `account`, whichever subscription started it,
    /// to be [`stop`]ped: its teardown's.
    pub(crate) fn take_account(&self, account: &str) -> Vec<JoinHandle<()>> {
        self.state().take(|owned| owned.account == account)
    }

    /// Every producer subscription `generation` registered, to be
    /// [`stop`]ped: its own, once a newer subscription replaced it.
    pub(crate) fn take_generation(&self, generation: u64) -> Vec<JoinHandle<()>> {
        self.state().take(|owned| owned.generation == generation)
    }
}

impl Owner {
    /// Its account's lifetime is live and its subscription the newest: a
    /// producer that finds otherwise has no effect, and stops.
    pub(crate) fn holds(&self) -> bool {
        self.owners.is_current(self.generation) && self.life.is_live()
    }
}

/// Abort every task, then await each: none of them runs once this returns.
pub(crate) async fn stop(tasks: Vec<JoinHandle<()>>) {
    for task in &tasks {
        task.abort();
    }
    for task in tasks {
        let _ = task.await;
    }
}

#[cfg(test)]
impl InboxOwners {
    /// Producer tasks of `account` that started and have not ended.
    pub(crate) fn running(&self, account: &str) -> usize {
        self.state().running.get(account).copied().unwrap_or(0)
    }

    /// Producers of `account` registered and not taken out.
    pub(crate) fn registered(&self, account: &str) -> usize {
        self.state()
            .producers
            .iter()
            .filter(|owned| owned.account == account)
            .count()
    }
}

#[cfg(test)]
impl Owner {
    /// A producer's owner whose account was just activated and whose
    /// subscription is the only one.
    pub(crate) fn live(account: &str) -> Owner {
        let lifetimes = super::lifetime::AccountLifetimes::default();
        let life = lifetimes.begin(account).expect("a fresh account opens");
        let owners = InboxOwners::default();
        let (generation, _) = owners.begin();
        owners.owner(generation, &life)
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::super::lifetime::AccountLifetimes;
    use super::*;

    /// A producer that runs until it is stopped.
    fn forever() -> impl Future<Output = ()> + Send + 'static {
        std::future::pending()
    }

    async fn settled(owners: &InboxOwners, account: &str, running: usize) -> bool {
        for _ in 0..200 {
            if owners.running(account) == running {
                return true;
            }
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
        owners.running(account) == running
    }

    /// R519: a producer of a lifetime that ended, or of a subscription a
    /// newer one replaced, never runs; one registered before either is
    /// taken out by its account's teardown or by the newer subscription.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn a_producer_runs_only_while_its_lifetime_and_subscription_hold() {
        let lifetimes = AccountLifetimes::default();
        let owners = InboxOwners::default();
        let life = lifetimes.begin("a").expect("activated");
        let (first, _) = owners.begin();
        let owner = owners.owner(first, &life);
        assert!(owners.spawn(&owner, forever()).await, "a live one runs");
        assert!(settled(&owners, "a", 1).await, "it started");
        assert!(owner.holds());

        let (second, older) = owners.begin();
        assert_eq!(older.len(), 1, "the newer subscription takes it out");
        stop(older).await;
        assert_eq!(owners.running("a"), 0, "stopped and awaited");
        assert!(!owner.holds(), "a replaced subscription holds nothing");
        assert!(
            !owners.spawn(&owner, forever()).await,
            "the replaced subscription registers nothing more"
        );
        assert_eq!(owners.registered("a"), 0);

        let newer = owners.owner(second, &life);
        assert!(owners.spawn(&newer, forever()).await);
        let closing = lifetimes.close("a");
        assert!(!newer.holds(), "a closing account holds nothing");
        assert!(
            !owners.spawn(&newer, forever()).await,
            "nothing starts once the teardown began"
        );
        stop(owners.take_account("a")).await;
        assert_eq!(owners.running("a"), 0, "the teardown stopped it");
        drop(closing);
        assert!(
            !owners.spawn(&newer, forever()).await,
            "nor after it: only a new activation's lifetime is live"
        );
        assert_eq!(owners.running("a"), 0);
    }
}
