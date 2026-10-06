//! A drive's maintenance under its one claim (AD-378, R207, R217).
//!
//! Every drive-wide maintenance job — the nightly consolidation, the weekly
//! curator — runs only while it holds `maintain:<drive>` in the principal's
//! control room ([`claim::maintenance_key`]), so no two of them run on one
//! drive at once, whichever host each runs on and whichever starts first.
//! Each job records the last window it completed under its own key
//! ([`claim::completion_key`]): one job's night never stands for another's.
//!
//! [`maintain`] is the whole execution a job asks for: the completion read
//! before and again after the acquisition, the renewal running as its own
//! task beside the work, the live fence handed to the work for every
//! effect, and the completion recorded only for work that settled under the
//! claim, once its release was accepted — under the claim taken again, and
//! never over a newer window.

use std::future::Future;
use std::sync::Arc;

use chrono::{DateTime, Utc};
use keeper_core::agents::claim::{self, Claimant, ServerClaim};
use keeper_core::agents::matrix::AgentMatrixError;
use keeper_sync::CommitFence;
use serde_json::Value;

use crate::claims::{
    acquire, release, renew, Acquired, ClaimPort, Lease, Moment, Rtt, ServerClock,
};

/// How often a running job looks whether its claim is due a renewal.
const RENEW_POLL: std::time::Duration = std::time::Duration::from_secs(1);

/// What asking for a drive's maintenance came to.
#[derive(Debug)]
pub enum Maintained<T> {
    /// The job's completion names the window already: nothing ran.
    Done,
    /// Another run holds the drive's maintenance claim — of this job or of
    /// another — on the host named.
    HeldBy(String),
    /// The work ran under the claim. `recorded`: it settled, the claim's
    /// release was accepted, and the window it was asked for (if any) is
    /// recorded as completed — only then is the run remembered done.
    Ran { out: T, recorded: bool },
}

/// The window a completion names, when it is one.
fn window_of(claim: &ServerClaim) -> Option<DateTime<Utc>> {
    claim
        .content
        .window
        .as_deref()
        .filter(|_| claim.content.released)
        .and_then(|text| DateTime::parse_from_rfc3339(text).ok())
        .map(|at| at.with_timezone(&Utc))
}

/// The completion `done` holds now, read.
async fn completion(done: &dyn ClaimPort) -> Result<Option<ServerClaim>, AgentMatrixError> {
    Ok(done
        .read()
        .await?
        .and_then(|state| ServerClaim::read(&state).ok()))
}

/// Whether the completion read through `done` covers `window` (epoch ms).
async fn covers(done: &dyn ClaimPort, window: i64) -> Result<bool, AgentMatrixError> {
    let window = DateTime::<Utc>::from_timestamp_millis(window).unwrap_or_default();
    Ok(completion(done)
        .await?
        .as_ref()
        .and_then(window_of)
        .is_some_and(|last| last >= window))
}

/// Record through `done`, as `me`, that `window` (epoch ms) completed —
/// unless the completion read names that window or a later one already.
/// `may_write` is asked after that read, right before the send: a holder
/// whose claim lapsed while it read — another host may have taken the
/// drive and recorded a later window since — sends nothing. The send
/// itself is not fenced: Matrix state has no compare-and-swap, so one held
/// up past the claim's lapse can still land over a later window recorded
/// meanwhile (DW-903). Whether it sent or found the window recorded.
async fn record(
    done: &dyn ClaimPort,
    me: &Claimant,
    clock: &ServerClock,
    window: i64,
    may_write: &(dyn Fn() -> bool + Sync),
) -> Result<bool, AgentMatrixError> {
    let at = DateTime::<Utc>::from_timestamp_millis(window).unwrap_or_default();
    let current = completion(done).await?;
    if current
        .as_ref()
        .and_then(window_of)
        .is_some_and(|last| last >= at)
    {
        return Ok(true);
    }
    if !may_write() {
        return Ok(false);
    }
    let epoch = current
        .map_or(0, |claim| claim.content.epoch)
        .saturating_add(1);
    let now = clock.now();
    let content = me.content(
        epoch,
        now,
        now,
        true,
        Some(claim::rfc3339(u64::try_from(window).unwrap_or(0))),
    );
    done.send(serde_json::to_value(&content).unwrap_or(Value::Null))
        .await
        .map(|_| true)
}

/// Record `window` completed through `done` under the drive's claim taken
/// again through `lease_port`: the release before it was accepted, and the
/// claim is asked once more between the completion's read and its send
/// ([`record`], whose send is not fenced itself: DW-903). Whether it was
/// recorded with that claim held up to the send and handed back after it:
/// `false` for a claim lost before the send, or one whose release is
/// refused — a local answer only, which says nothing of whether a send
/// already made reached the server.
async fn record_held(
    lease_port: &dyn ClaimPort,
    done: &dyn ClaimPort,
    me: &Claimant,
    clock: &ServerClock,
    rtt: &Rtt,
    window: i64,
) -> bool {
    let again = match acquire(lease_port, me, clock, rtt, None).await {
        Ok(Acquired::Won { lease, .. }) => lease,
        _ => return false,
    };
    let recorded = again.may_write()
        && matches!(
            record(done, me, clock, window, &|| again.may_write()).await,
            Ok(true)
        );
    if !matches!(release(lease_port, me, &again, clock, rtt).await, Ok(true)) {
        tracing::warn!("agents: the claim taken to record a drive's maintenance could not be handed back; it lapses");
        return false;
    }
    recorded
}

/// A lease's renewal, running as its own task: the work beside it — its
/// blocking parts included — cannot starve it.
struct Renewal {
    stop: Option<tokio::sync::oneshot::Sender<()>>,
    task: tokio::task::JoinHandle<()>,
}

impl Renewal {
    /// Renew `lease` through `port` whenever it is due; a renewal that is
    /// refused or fails loses the lease, so the fence every effect asks
    /// ([`Lease::may_write`]) says no from then on.
    fn start(
        port: &Arc<dyn ClaimPort>,
        me: &Claimant,
        lease: &Arc<Lease>,
        clock: &Arc<ServerClock>,
        rtt: &Arc<Rtt>,
    ) -> Renewal {
        let (stop, mut stopped) = tokio::sync::oneshot::channel::<()>();
        let (port, me, lease, clock, rtt) = (
            Arc::clone(port),
            me.clone(),
            Arc::clone(lease),
            Arc::clone(clock),
            Arc::clone(rtt),
        );
        let task = tokio::spawn(async move {
            while !lease.is_lost() {
                tokio::select! {
                    _ = &mut stopped => return,
                    () = tokio::time::sleep(RENEW_POLL) => {}
                }
                if lease.renewal_due(Moment::now())
                    && !matches!(
                        renew(port.as_ref(), &me, &lease, &clock, &rtt).await,
                        Ok(true)
                    )
                {
                    tracing::warn!("agents: a drive's maintenance claim could not be renewed; nothing more is written");
                    lease.lose();
                }
            }
        });
        Renewal {
            stop: Some(stop),
            task,
        }
    }

    /// Stop it between renewals — never inside one, whose write could
    /// land after the release — and wait until it has.
    async fn stop(mut self) {
        if let Some(stop) = self.stop.take() {
            let _ = stop.send(());
        }
        let _ = (&mut self.task).await;
    }
}

impl Drop for Renewal {
    /// A holder dropped before its work ended renews nothing more.
    fn drop(&mut self) {
        self.task.abort();
    }
}

/// Run `work` while `lease` is renewed through `port` whenever it is due,
/// by a task of its own with its own handles on the port, the lease and
/// the clocks: however the work blocks, the renewal runs. A renewal that
/// is refused or fails loses the lease, so the fence every effect asks
/// ([`Lease::may_write`]) says no from then on. Once the work ends the
/// renewal is stopped, between two renewals, and waited for.
pub async fn holding<T>(
    port: &Arc<dyn ClaimPort>,
    me: &Claimant,
    lease: &Arc<Lease>,
    clock: &Arc<ServerClock>,
    rtt: &Arc<Rtt>,
    work: impl Future<Output = T>,
) -> T {
    let renewal = Renewal::start(port, me, lease, clock, rtt);
    let out = work.await;
    renewal.stop().await;
    out
}

/// Run `work` as `me` under the drive's maintenance claim, taken through
/// `lease_port`, for `window` (epoch ms) of the job whose completion
/// `done_port` reads and records; `window` = `None` runs whatever the
/// completion says and records none — a person's decisions carried out
/// between nights.
///
/// The completion is read first, and again once the claim is won: a run
/// that completed the window between the two makes it [`Maintained::Done`],
/// and the claim goes back. `work` gets the live fence — the lease may still
/// write — to ask before every effect, and answers what it came to and
/// whether it settled. Settled work whose claim was still this host's and
/// whose release was accepted is recorded under the claim taken again
/// ([`Maintained::Ran`]'s `recorded`).
pub async fn maintain<T, W, Fut>(
    lease_port: &Arc<dyn ClaimPort>,
    done_port: &dyn ClaimPort,
    me: &Claimant,
    clock: &Arc<ServerClock>,
    rtt: &Arc<Rtt>,
    window: Option<i64>,
    work: W,
) -> Result<Maintained<T>, AgentMatrixError>
where
    W: FnOnce(CommitFence) -> Fut,
    Fut: Future<Output = (T, bool)>,
{
    if let Some(window) = window {
        if covers(done_port, window).await? {
            return Ok(Maintained::Done);
        }
    }
    let lease = match acquire(lease_port.as_ref(), me, clock, rtt, None).await? {
        Acquired::Won { lease, .. } => lease,
        Acquired::HeldElsewhere | Acquired::Yielded => {
            let holder = lease_port
                .read()
                .await?
                .and_then(|state| ServerClaim::read(&state).ok())
                .map(|claim| claim.content.host)
                .unwrap_or_default();
            return Ok(Maintained::HeldBy(holder));
        }
    };
    if let Some(window) = window {
        if covers(done_port, window).await? {
            release(lease_port.as_ref(), me, &lease, clock, rtt).await?;
            return Ok(Maintained::Done);
        }
    }
    let fence: CommitFence = {
        let lease = Arc::clone(&lease);
        Arc::new(move || lease.may_write())
    };
    let (out, settled) = holding(lease_port, me, &lease, clock, rtt, work(fence)).await;
    // Settled work under a claim still this host's, handed back as its own:
    // only then is the window recorded — a release refused says another
    // host may be running the drive now.
    let held = lease.may_write();
    let released = matches!(
        release(lease_port.as_ref(), me, &lease, clock, rtt).await,
        Ok(true)
    );
    let recorded = settled
        && held
        && released
        && match window {
            Some(window) => {
                record_held(lease_port.as_ref(), done_port, me, clock, rtt, window).await
            }
            None => true,
        };
    if settled && !recorded {
        tracing::warn!("agents: a drive's maintenance settled but its completion could not be recorded under its claim; it is tried again");
    }
    Ok(Maintained::Ran { out, recorded })
}
