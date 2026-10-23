//! One account's lifetime in this process (story 102.5; R516, review
//! R102L6-01, R102L6-02).
//!
//! Everything account-scoped the inbox starts — its producers — is started
//! by work that awaits: an activation, a room list's acquisition. A sign-out or
//! a teardown that runs meanwhile cannot stop what is not registered yet, so
//! that work carries a [`Lifetime`] taken *before* it awaits anything, and
//! every place it publishes something checks it:
//!
//! - a teardown [`AccountLifetimes::close`]s the account the moment it
//!   begins: every lifetime taken before ends, and none taken while it runs
//!   is ever live;
//! - only an activation [`AccountLifetimes::open`]s a lifetime, and never one
//!   that ended or one of an account that is closing — a closed account is
//!   live again only by a genuine new activation, never by old work that
//!   resumes late.
//!
//! The inbox producers are registered with their lifetime in
//! [`super::inbox_owners`] before they run, and check it before each effect
//! (R519): what a teardown stops is everything that can still act.
//!
//! The state is a few numbers per account behind one `std` mutex, never held
//! across an await.

use std::collections::HashMap;
use std::sync::{Arc, Mutex, MutexGuard};

#[derive(Debug, Default, Clone, Copy)]
struct Life {
    /// Bumped when a teardown begins and again when it ends.
    epoch: u64,
    /// An activation opened the present epoch.
    open: bool,
    /// Teardowns running now.
    closing: usize,
}

#[derive(Debug, Default)]
struct Shared {
    lives: Mutex<HashMap<String, Life>>,
    #[cfg(test)]
    pauses: Mutex<Vec<Armed>>,
}

impl Shared {
    fn lives(&self) -> MutexGuard<'_, HashMap<String, Life>> {
        // Plain numbers: a panic mid-update tears nothing.
        self.lives.lock().unwrap_or_else(|p| p.into_inner())
    }

    fn life(&self, account: &str) -> Life {
        self.lives().get(account).copied().unwrap_or_default()
    }
}

/// Every account's lifetime, by account id.
#[derive(Debug, Default, Clone)]
pub(crate) struct AccountLifetimes {
    shared: Arc<Shared>,
}

/// One account's lifetime as some work took it: live while an activation
/// opened it and no teardown began since.
#[derive(Debug, Clone)]
pub(crate) struct Lifetime {
    shared: Arc<Shared>,
    account: String,
    epoch: u64,
}

/// Every account's lifetime as it stood at one moment: what a subscription
/// takes before it reads which accounts there are.
pub(crate) struct Seen {
    shared: Arc<Shared>,
    epochs: HashMap<String, u64>,
}

/// An account's teardown, from its beginning until this drops.
#[must_use = "the account is closing until this drops"]
pub(crate) struct Closing {
    shared: Arc<Shared>,
    account: String,
}

/// The lifetime ended, or its account is closing: nothing may start in it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Ended;

impl AccountLifetimes {
    /// Every account's lifetime now.
    pub(crate) fn seen(&self) -> Seen {
        let epochs = self
            .shared
            .lives()
            .iter()
            .map(|(account, life)| (account.clone(), life.epoch))
            .collect();
        Seen {
            shared: Arc::clone(&self.shared),
            epochs,
        }
    }

    /// A lifetime of `account` taken and opened at once: an activation's
    /// own, under the accounts lock.
    pub(crate) fn begin(&self, account: &str) -> Result<Lifetime, Ended> {
        let mut lives = self.shared.lives();
        let life = lives.entry(account.to_owned()).or_default();
        if life.closing > 0 {
            return Err(Ended);
        }
        life.open = true;
        Ok(Lifetime {
            shared: Arc::clone(&self.shared),
            account: account.to_owned(),
            epoch: life.epoch,
        })
    }

    /// Open `life` for an activation (or confirm the account it found live,
    /// under the accounts lock): refused once it ended.
    pub(crate) fn open(&self, life: &Lifetime) -> Result<(), Ended> {
        let mut lives = self.shared.lives();
        let state = lives.entry(life.account.clone()).or_default();
        if state.closing > 0 || state.epoch != life.epoch {
            return Err(Ended);
        }
        state.open = true;
        Ok(())
    }

    /// `account`'s teardown begins: every lifetime of it ends now, and none
    /// is opened until the returned guard drops.
    pub(crate) fn close(&self, account: &str) -> Closing {
        let mut lives = self.shared.lives();
        let life = lives.entry(account.to_owned()).or_default();
        life.epoch += 1;
        life.open = false;
        life.closing += 1;
        Closing {
            shared: Arc::clone(&self.shared),
            account: account.to_owned(),
        }
    }
}

impl Drop for Closing {
    fn drop(&mut self) {
        let mut lives = self.shared.lives();
        let life = lives.entry(self.account.clone()).or_default();
        // A lifetime taken while it closed never opens: its epoch is gone.
        life.epoch += 1;
        life.closing = life.closing.saturating_sub(1);
    }
}

impl Seen {
    /// `account`'s lifetime as it stood when this was taken.
    pub(crate) fn of(&self, account: &str) -> Lifetime {
        Lifetime {
            shared: Arc::clone(&self.shared),
            account: account.to_owned(),
            epoch: self.epochs.get(account).copied().unwrap_or(0),
        }
    }
}

impl Lifetime {
    pub(crate) fn account(&self) -> &str {
        &self.account
    }

    /// Opened by an activation, and no teardown began since.
    pub(crate) fn is_live(&self) -> bool {
        let life = self.shared.life(&self.account);
        life.epoch == self.epoch && life.open && life.closing == 0
    }

    /// A teardown began since it was taken: it can never be live.
    pub(crate) fn ended(&self) -> bool {
        let life = self.shared.life(&self.account);
        life.epoch != self.epoch || life.closing > 0
    }
}

/// Where tests hold account work, to begin a teardown in between. Outside
/// tests, passing one does nothing (and `account.rs` carries no test-only
/// code of its own: its production is source-scanned up to its one test
/// module).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Pause {
    /// A subscription is about to acquire an account's room list.
    Acquiring,
    /// A subscription built every producer, none published.
    Publishing,
    /// An activation built everything, its lifetime not checked.
    CheckingLife,
    /// A teardown ended the lifetime; the account's inbox producers are
    /// not stopped yet.
    StoppingProducers,
    /// A teardown is about to take the account out.
    Detaching,
}

impl AccountLifetimes {
    /// Stop here if a test armed `at`; nothing outside tests.
    pub(crate) async fn pause(&self, at: Pause) {
        #[cfg(test)]
        pause(&self.shared, at, None).await;
        #[cfg(not(test))]
        let _ = at;
    }
}

impl Lifetime {
    /// Stop here if a test armed `at` (for this account or any); nothing
    /// outside tests.
    pub(crate) async fn pause(&self, at: Pause) {
        #[cfg(test)]
        pause(&self.shared, at, Some(&self.account)).await;
        #[cfg(not(test))]
        let _ = at;
    }
}

#[cfg(test)]
#[derive(Debug)]
struct Armed {
    at: Pause,
    /// Only this account's work stops there; any account's when `None`.
    account: Option<String>,
    reached: tokio::sync::oneshot::Sender<()>,
    release: tokio::sync::oneshot::Receiver<()>,
}

/// A pause armed by [`AccountLifetimes::pause_at`]: `reached` resolves when
/// the work stops there, and it goes on once `release` is sent or dropped.
#[cfg(test)]
pub(crate) struct Paused {
    pub(crate) reached: tokio::sync::oneshot::Receiver<()>,
    pub(crate) release: tokio::sync::oneshot::Sender<()>,
}

#[cfg(test)]
impl AccountLifetimes {
    /// Hold the next work that reaches `at`.
    pub(crate) fn pause_at(&self, at: Pause) -> Paused {
        self.arm(at, None)
    }

    /// Hold the next work of `account` that reaches `at`.
    pub(crate) fn pause_at_for(&self, at: Pause, account: &str) -> Paused {
        self.arm(at, Some(account.to_owned()))
    }

    fn arm(&self, at: Pause, account: Option<String>) -> Paused {
        let (reached_tx, reached) = tokio::sync::oneshot::channel();
        let (release, release_rx) = tokio::sync::oneshot::channel();
        self.shared
            .pauses
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .push(Armed {
                at,
                account,
                reached: reached_tx,
                release: release_rx,
            });
        Paused { reached, release }
    }
}

#[cfg(test)]
async fn pause(shared: &Shared, at: Pause, account: Option<&str>) {
    let armed = {
        let mut pauses = shared.pauses.lock().unwrap_or_else(|p| p.into_inner());
        pauses
            .iter()
            .position(|armed| {
                armed.at == at
                    && armed
                        .account
                        .as_deref()
                        .is_none_or(|only| Some(only) == account)
            })
            .map(|at| pauses.remove(at))
    };
    if let Some(armed) = armed {
        let _ = armed.reached.send(());
        let _ = armed.release.await;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// R516: a teardown ends every lifetime taken before it; one taken
    /// while it runs never opens, even once it ended; only a new
    /// activation is live again, and old work is not.
    #[test]
    fn a_closed_lifetime_is_reopened_only_by_a_new_activation() {
        let lives = AccountLifetimes::default();
        let before = lives.seen().of("acct");
        assert!(!before.is_live(), "nothing activated it yet");
        lives.open(&before).expect("an activation opens it");
        assert!(before.is_live());

        let closing = lives.close("acct");
        assert!(before.ended() && !before.is_live(), "the teardown began");
        assert!(!lives.seen().of("other").ended(), "another account's own");
        let during = lives.seen().of("acct");
        assert_eq!(lives.open(&during), Err(Ended), "closing");
        assert_eq!(lives.begin("acct").err(), Some(Ended), "closing");
        drop(closing);

        assert_eq!(lives.open(&before), Err(Ended), "old work stays ended");
        assert_eq!(
            lives.open(&during),
            Err(Ended),
            "so does work begun while it closed"
        );
        let again = lives.begin("acct").expect("a new activation");
        assert!(again.is_live() && !before.is_live() && !during.is_live());
    }
}
