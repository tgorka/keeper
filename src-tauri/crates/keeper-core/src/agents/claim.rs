//! The claim on a session, as arithmetic over the homeserver's clock
//! (AD-378; story 90.6).
//!
//! One host writes a session: the one whose `dev.keeper.agent.claim` state
//! event the server holds. A holder renews every [`RENEW_EVERY`] with an
//! expiry [`TTL`] ahead; a taker may write `epoch + 1` once the claim is
//! released or its event is [`TTL`] old by `origin_server_ts`, the server's
//! clock and never a host's. A holder that has not confirmed a renewal for
//! [`STOP_WITHOUT_RENEWAL`] stops writing, so with clock skew under the
//! 60 s margin no two hosts write at once; the log's `(epoch, claim)` fence is
//! the backstop past it.
//!
//! A state event is not a compare-and-set: two takers inside one round trip
//! can each read back their own write. So a taker **settles** (S-05) — waits
//! [`settle`], reads the claim from the server again — and proceeds only if
//! the claim is still its own event.

use std::time::{Duration, Instant};

use chrono::{DateTime, SecondsFormat, Utc};
use matrix_sdk::ruma::{OwnedEventId, OwnedUserId};
use serde_json::Value;

use crate::agents::events::ClaimContent;
use crate::agents::matrix::ServerState;

/// How often a holder renews its claim.
pub const RENEW_EVERY: Duration = Duration::from_secs(60);
/// How long a claim lasts past its event's `origin_server_ts`.
pub const TTL: Duration = Duration::from_secs(180);
/// How long a holder writes without a confirmed renewal; [`TTL`] less this is
/// the margin that absorbs clock skew between hosts.
pub const STOP_WITHOUT_RENEWAL: Duration = Duration::from_secs(120);

/// The claim event as the server holds it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ServerClaim {
    pub event_id: OwnedEventId,
    pub sender: OwnedUserId,
    /// The server's time of the event, ms since the Unix epoch.
    pub origin_server_ts: u64,
    pub content: ClaimContent,
}

/// Why a claim event cannot be read.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ClaimRefusal {
    #[error("the claim's content does not read: {0}")]
    Shape(String),
    #[error("the claim's {field} \"{value}\" is not an RFC 3339 time")]
    Time { field: &'static str, value: String },
}

impl ServerClaim {
    /// Read a claim from the server's state event, refusing a content whose
    /// keys or times are not the schema's (a `window` included, S-25).
    pub fn read(state: &ServerState) -> Result<ServerClaim, ClaimRefusal> {
        Ok(ServerClaim {
            event_id: state.event_id.clone(),
            sender: state.sender.clone(),
            origin_server_ts: state.origin_server_ts.get().into(),
            content: read_content(&state.content)?,
        })
    }

    /// Whether this is the claim of `me` — its host's copy, sent by its
    /// agent's user — at `epoch`: a renewal is a new event, so the holder is
    /// named by its content, and only the agent's own user can write it.
    pub fn is_held_by(&self, me: &Claimant, epoch: u64) -> bool {
        !self.content.released
            && self.sender == me.agent
            && self.content.host == me.host
            && self.content.device == me.device
            && self.content.epoch == epoch
    }

    /// When the claim lapses by the server's clock.
    pub fn lapses_at(&self) -> u64 {
        self.origin_server_ts.saturating_add(TTL.as_millis() as u64)
    }
}

/// A claim content, its times checked.
pub fn read_content(content: &Value) -> Result<ClaimContent, ClaimRefusal> {
    let claim: ClaimContent = serde_json::from_value(content.clone())
        .map_err(|error| ClaimRefusal::Shape(error.to_string()))?;
    for (field, value) in [
        ("acquired_at", Some(&claim.acquired_at)),
        ("renewed_at", Some(&claim.renewed_at)),
        ("expires_at", Some(&claim.expires_at)),
        ("window", claim.window.as_ref()),
    ] {
        if let Some(value) = value {
            if DateTime::parse_from_rfc3339(value).is_err() {
                return Err(ClaimRefusal::Time {
                    field,
                    value: value.clone(),
                });
            }
        }
    }
    Ok(claim)
}

/// Whether a host may write a new claim over `current` at `server_now` (ms):
/// there is none, it is released, or its event is [`TTL`] old.
pub fn may_acquire(current: Option<&ServerClaim>, server_now: u64) -> bool {
    match current {
        None => true,
        Some(claim) => claim.content.released || server_now >= claim.lapses_at(),
    }
}

/// The epoch a taker writes over `current`: one more, starting at 1 (epoch 0
/// is the era before claims, C5).
pub fn next_epoch(current: Option<&ServerClaim>) -> u64 {
    current.map_or(1, |claim| claim.content.epoch.saturating_add(1))
}

/// Whether a holder whose last confirmed renewal was at
/// `last_confirmed_renewal` must stop writing at `now` (the host's monotonic
/// clock: the interval is what counts, not the time of day; a lease also
/// checks the wall clock, which keeps running while the machine sleeps).
pub fn holder_must_stop(last_confirmed_renewal: Instant, now: Instant) -> bool {
    now.saturating_duration_since(last_confirmed_renewal) >= STOP_WITHOUT_RENEWAL
}

/// How long a taker waits before it reads the claim again (S-05): twice the
/// longest round trip it has measured to the homeserver, or one completed
/// `/sync` round, whichever is longer.
pub fn settle(longest_rtt: Duration, sync_round: Duration) -> Duration {
    longest_rtt.saturating_mul(2).max(sync_round)
}

/// `ms` since the Unix epoch as RFC 3339 UTC, to the millisecond.
pub fn rfc3339(ms: u64) -> String {
    DateTime::<Utc>::from_timestamp_millis(i64::try_from(ms).unwrap_or(i64::MAX))
        .unwrap_or(DateTime::<Utc>::MAX_UTC)
        .to_rfc3339_opts(SecondsFormat::Millis, true)
}

/// Who writes a claim: one copy of one agent on one host.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Claimant {
    pub host: String,
    pub device: String,
    pub agent: OwnedUserId,
}

impl Claimant {
    /// The content of a claim at `epoch`, acquired at `acquired_at` (ms),
    /// written at `server_now` (ms), lasting [`TTL`]; `released` hands it back.
    pub fn content(
        &self,
        epoch: u64,
        acquired_at: u64,
        server_now: u64,
        released: bool,
        window: Option<String>,
    ) -> ClaimContent {
        ClaimContent {
            v: crate::agents::events::CONTENT_VERSION,
            host: self.host.clone(),
            device: self.device.clone(),
            agent: self.agent.clone(),
            epoch,
            acquired_at: rfc3339(acquired_at),
            renewed_at: rfc3339(server_now),
            expires_at: rfc3339(if released {
                server_now
            } else {
                server_now.saturating_add(TTL.as_millis() as u64)
            }),
            released,
            window,
        }
    }
}

#[cfg(test)]
mod tests {
    use matrix_sdk::ruma::{MilliSecondsSinceUnixEpoch, UInt};
    use serde_json::json;

    use super::*;

    const T0: u64 = 1_790_000_000_000;

    fn claimant() -> Claimant {
        Claimant {
            host: "electra".to_owned(),
            device: "ELECTRA1".to_owned(),
            agent: OwnedUserId::try_from("@nixi:example.org").expect("user"),
        }
    }

    fn claim(epoch: u64, at: u64, released: bool) -> ServerClaim {
        ServerClaim {
            event_id: OwnedEventId::try_from(format!("$c{epoch}:example.org")).expect("event"),
            sender: claimant().agent,
            origin_server_ts: at,
            content: claimant().content(epoch, at, at, released, None),
        }
    }

    #[test]
    fn the_claim_arithmetic_holds() {
        assert!(may_acquire(None, T0), "no claim");
        assert_eq!(next_epoch(None), 1, "the first epoch is 1");

        let live = claim(4, T0, false);
        assert!(may_acquire(Some(&claim(4, T0, true)), T0), "released");
        assert!(!may_acquire(Some(&live), T0), "live");
        let ttl = TTL.as_millis() as u64;
        assert!(
            !may_acquire(Some(&live), T0 + ttl - 1),
            "a ms before it lapses"
        );
        assert!(may_acquire(Some(&live), T0 + ttl), "lapsed at +180 s");
        assert_eq!(next_epoch(Some(&live)), 5);

        // Expiry is the event's server time, never the content's: a holder
        // whose clock runs ahead cannot buy itself time by writing a later
        // `expires_at`.
        let mut ahead = live.clone();
        ahead.content.expires_at = rfc3339(T0 + 10 * ttl);
        assert!(may_acquire(Some(&ahead), T0 + ttl));

        let renewed = Instant::now();
        assert!(!holder_must_stop(
            renewed,
            renewed + Duration::from_secs(119)
        ));
        assert!(holder_must_stop(
            renewed,
            renewed + Duration::from_secs(120)
        ));
        // The margin between stopping and the earliest takeover.
        assert_eq!(TTL - STOP_WITHOUT_RENEWAL, Duration::from_secs(60));
    }

    #[test]
    fn settle_is_the_larger_of_two_rtts_and_a_sync_round() {
        let ms = Duration::from_millis;
        assert_eq!(settle(ms(40), ms(30)), ms(80));
        assert_eq!(settle(ms(40), ms(500)), ms(500));
        assert_eq!(settle(ms(250), ms(500)), ms(500));
        assert_eq!(settle(ms(260), ms(500)), ms(520));
    }

    fn state(content: Value) -> ServerState {
        ServerState {
            event_id: OwnedEventId::try_from("$c:example.org").expect("event"),
            sender: claimant().agent,
            origin_server_ts: MilliSecondsSinceUnixEpoch(UInt::new(T0).expect("ts")),
            content,
        }
    }

    #[test]
    fn the_claim_carries_the_window_it_runs() {
        let without = claimant().content(2, T0, T0, false, None);
        let value = serde_json::to_value(&without).expect("serialise");
        assert!(value.get("window").is_none());
        let read = ServerClaim::read(&state(value)).expect("read");
        assert_eq!(read.content, without);
        assert_eq!(read.origin_server_ts, T0);

        let window = "2026-10-03T07:00:00Z".to_owned();
        let with = claimant().content(2, T0, T0, false, Some(window.clone()));
        let value = serde_json::to_value(&with).expect("serialise");
        assert_eq!(value["window"], json!(window));
        assert_eq!(
            ServerClaim::read(&state(value.clone()))
                .expect("read")
                .content,
            with
        );

        let mut bad = value;
        bad["window"] = json!("tomorrow morning");
        assert_eq!(
            ServerClaim::read(&state(bad)),
            Err(ClaimRefusal::Time {
                field: "window",
                value: "tomorrow morning".to_owned()
            })
        );
    }

    #[test]
    fn a_claim_is_held_by_its_host_device_and_epoch_until_released() {
        let held = claim(3, T0, false);
        let me = claimant();
        assert!(held.is_held_by(&me, 3));
        let other = |change: fn(&mut Claimant)| {
            let mut other = claimant();
            change(&mut other);
            other
        };
        let copy = other(|c| c.device = "ELECTRA2".to_owned());
        assert!(!held.is_held_by(&copy, 3), "another copy");
        let host = other(|c| c.host = "hesperia".to_owned());
        assert!(!held.is_held_by(&host, 3), "another host");
        assert!(!held.is_held_by(&me, 2), "an older epoch");
        assert!(!claim(3, T0, true).is_held_by(&me, 3));
        // Another user with power in the room writes a claim naming this
        // copy: it is not this copy's.
        let forged = ServerClaim {
            sender: OwnedUserId::try_from("@amelia:example.org").expect("user"),
            ..held
        };
        assert!(!forged.is_held_by(&me, 3), "another sender");
    }
}
