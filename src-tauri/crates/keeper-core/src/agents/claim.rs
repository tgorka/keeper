//! The claim on a session, as arithmetic over the homeserver's clock
//! (AD-378; story 90.6).
//!
//! One host writes a session: the one whose `dev.keeper.agent.claim` state
//! event the server holds. A holder renews every [`RENEW_EVERY`] with an
//! expiry [`TTL`] ahead; a taker may write `epoch + 1` once the claim is
//! released or its event is [`TTL`] old by `origin_server_ts`, the server's
//! clock and never a host's. A holder that has not confirmed a renewal for
//! [`STOP_WITHOUT_RENEWAL`] stops writing, so with clock skew under the
//! [`TTL`] − [`STOP_WITHOUT_RENEWAL`] margin no two hosts write at once; the
//! log's `(epoch, claim)` fence is the backstop past it.
//!
//! A renewal is a state event in the session's room, so its cadence is what
//! the room's timeline is made of while nobody speaks (DW-1385: one a minute
//! buried a person's DM under 283 claims in 300 events). So a session's claim
//! may run on the [`LeaseKind::QuarterHour`] lease (R416, R449, R455): renewed
//! every 15 min, stopped after 30, lapsed after 45 — the same proportions.
//! It is an operator's cutover, never automatic: a host built before it reads
//! every claim with the minute lease's 180 s and would take a quarter-hour
//! claim from a holder that is still writing, and such a host can come back
//! from being offline at any time without having said anything first. So the
//! default everywhere is the minute lease, and a host takes the quarter-hour
//! one only when its own configuration turns it on ([`ClaimsLease`]) **and**
//! every live host manifest in its control room advertises it
//! ([`LeaseKind::for_fleet`]). A holder on it that sees a live manifest or a
//! live claim from a host that does not advertise it drops to the minute
//! lease at once and writes nothing until its minute-lease renewal is
//! confirmed. The claim carries its kind in its own times —
//! `expires_at − renewed_at` ([`LeaseKind::of`]) — not in a new key:
//! [`ClaimContent`] refuses keys it does not know, and an older host that
//! cannot read a claim treats its session as nobody's. Every claim in a
//! control room (maintenance, pictures, the DM request) keeps the minute
//! lease. A host that stops cleanly releases its claim, which is free at
//! once; only a host that vanishes holds a session for up to its lease's
//! [`LeaseKind::ttl`].
//!
//! A state event is not a compare-and-set: two takers inside one round trip
//! can each read back their own write. So a taker **settles** (S-05) — waits
//! [`settle`], reads the claim from the server again — and proceeds only if
//! the claim is still its own event.

use std::time::{Duration, Instant};

use chrono::{DateTime, SecondsFormat, Utc};
use matrix_sdk::ruma::{OwnedEventId, OwnedUserId};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use ts_rs::TS;

use crate::agents::events::ClaimContent;
use crate::agents::host::HostManifest;
use crate::agents::matrix::ServerState;

/// How often a holder renews a claim on the minute lease.
pub const RENEW_EVERY: Duration = LeaseKind::Minute.renew_every();
/// How long a minute-lease claim lasts past its event's `origin_server_ts`.
pub const TTL: Duration = LeaseKind::Minute.ttl();
/// How long a holder on the minute lease writes without a confirmed renewal;
/// [`TTL`] less this is the margin that absorbs clock skew between hosts.
pub const STOP_WITHOUT_RENEWAL: Duration = LeaseKind::Minute.stop_without_renewal();

/// What a host manifest's `lease` says when the host reads and writes the
/// [`LeaseKind::QuarterHour`] lease (R449); absent, it does not.
pub const QUARTER_HOUR_LEASE: &str = "quarter-hour";

/// Which lease a host's own configuration lets a session's claim run on
/// (R455): `[claims] lease` in `agentd.toml`, `agents.claims_lease` on a
/// desktop host. `short` — the minute lease — unless the operator turned
/// the quarter-hour one on after every device and agentd serving the agents
/// runs a build that reads it; even then it is taken only while every live
/// host advertises it ([`LeaseKind::for_fleet`]).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "kebab-case")]
#[ts(export)]
pub enum ClaimsLease {
    /// The minute lease, always: the default.
    #[default]
    Short,
    /// The quarter-hour lease, while every live host advertises it.
    QuarterHour,
}

impl ClaimsLease {
    /// The value as `agentd.toml` and the settings key spell it.
    pub fn as_str(self) -> &'static str {
        match self {
            ClaimsLease::Short => "short",
            ClaimsLease::QuarterHour => QUARTER_HOUR_LEASE,
        }
    }

    /// `short` or `quarter-hour`; anything else is not a choice.
    pub fn parse(raw: &str) -> Option<ClaimsLease> {
        match raw.trim() {
            "short" => Some(ClaimsLease::Short),
            QUARTER_HOUR_LEASE => Some(ClaimsLease::QuarterHour),
            _ => None,
        }
    }
}

/// How long a claim lasts and how often its holder renews it. Each keeps the
/// same proportions: a holder stops writing after two missed renewals and
/// anyone may take the claim after three, so the margin between the two is a
/// whole renewal.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LeaseKind {
    /// Renewed every 60 s, stopped after 120 s, lapsed after 180 s: what
    /// every host reads.
    Minute,
    /// Renewed every 15 min, stopped after 30, lapsed after 45 (R416): only
    /// while every live host reads it (R449).
    QuarterHour,
}

impl LeaseKind {
    /// How often its holder renews it.
    pub const fn renew_every(self) -> Duration {
        match self {
            LeaseKind::Minute => Duration::from_secs(60),
            LeaseKind::QuarterHour => Duration::from_secs(15 * 60),
        }
    }

    /// How long its holder writes without a confirmed renewal: two renewals.
    pub const fn stop_without_renewal(self) -> Duration {
        Duration::from_secs(2 * self.renew_every().as_secs())
    }

    /// How long a claim lasts past its event's `origin_server_ts`: three
    /// renewals.
    pub const fn ttl(self) -> Duration {
        Duration::from_secs(3 * self.renew_every().as_secs())
    }

    /// The kind `content` was written on: a claim whose expiry is a
    /// quarter-hour lease's TTL or more past its renewal is one; a released
    /// claim, a minute one and one whose times do not read are minute ones.
    pub fn of(content: &ClaimContent) -> LeaseKind {
        let at = |value: &str| {
            DateTime::parse_from_rfc3339(value)
                .ok()
                .map(|at| at.timestamp_millis())
        };
        match (at(&content.renewed_at), at(&content.expires_at)) {
            (Some(renewed), Some(expires))
                if !content.released
                    && expires.saturating_sub(renewed)
                        >= LeaseKind::QuarterHour.ttl().as_millis() as i64 =>
            {
                LeaseKind::QuarterHour
            }
            _ => LeaseKind::Minute,
        }
    }

    /// The lease a host takes or renews a session's claim on at `server_now`
    /// (R449, R455): the quarter-hour one only when `setting` turns it on,
    /// at least one host manifest is live in the control room (`hosts`,
    /// this host's own among them), every live one says it reads it, and no
    /// live claim this host sees (`claims`) was written on the minute lease
    /// by a host whose manifest does not say so — such a host is running
    /// now, whether or not its manifest has arrived. Otherwise the minute
    /// one.
    pub fn for_fleet(
        setting: ClaimsLease,
        hosts: &[HostManifest],
        claims: &[ServerClaim],
        server_now: u64,
    ) -> LeaseKind {
        if setting == ClaimsLease::Short {
            return LeaseKind::Minute;
        }
        let advertises = |host: &HostManifest| host.lease.as_deref() == Some(QUARTER_HOUR_LEASE);
        let mut live = hosts
            .iter()
            .filter(|host| host.is_live(server_now))
            .peekable();
        if live.peek().is_none() || !live.all(advertises) {
            return LeaseKind::Minute;
        }
        let old_writer = claims.iter().any(|claim| {
            !claim.content.released
                && server_now < claim.lapses_at()
                && LeaseKind::of(&claim.content) == LeaseKind::Minute
                && !hosts
                    .iter()
                    .any(|host| host.host == claim.content.host && advertises(host))
        });
        if old_writer {
            LeaseKind::Minute
        } else {
            LeaseKind::QuarterHour
        }
    }
}

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

    /// When the claim lapses by the server's clock: its lease's TTL after
    /// its event, whatever expiry the content names.
    pub fn lapses_at(&self) -> u64 {
        self.origin_server_ts
            .saturating_add(LeaseKind::of(&self.content).ttl().as_millis() as u64)
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

/// The state key of `drive`'s one maintenance claim in the principal's
/// control room: every drive-wide maintenance job — the nightly
/// consolidation, the weekly curator — runs only under it, so no two of them
/// run on one drive at once (AD-378, R207).
pub fn maintenance_key(drive: &str) -> String {
    format!("maintain:{drive}")
}

/// The state key under which maintenance `job` records, as a released
/// claim, the last window it completed on `drive`: each job's own, so one
/// job's night never stands for another's.
pub fn completion_key(job: &str, drive: &str) -> String {
    format!("{job}:{drive}")
}

/// The state key of the claim in the principal's control room under which
/// one host at a time publishes `agent`'s picture as its Matrix profile
/// avatar (story 100.4, R399): only its holder uploads, and it reads the
/// agent's record again once it holds it.
pub fn picture_key(agent: &matrix_sdk::ruma::UserId) -> String {
    format!("picture:{agent}")
}

/// The state key of the claim in the person's control room under which one
/// host at a time answers a request for `agent`'s `main` DM (story 101.1,
/// R416): two hosts serving one proxy never make two DMs.
pub fn main_dm_key(agent: &matrix_sdk::ruma::UserId) -> String {
    format!("main-dm:{agent}")
}

/// Whether a host may write a new claim over `current` at `server_now` (ms):
/// there is none, it is released, or its event is its lease's TTL old.
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

/// Whether a holder on a `kind` lease whose last confirmed renewal was at
/// `last_confirmed_renewal` must stop writing at `now` (the host's monotonic
/// clock: the interval is what counts, not the time of day; a lease also
/// checks the wall clock, which keeps running while the machine sleeps).
pub fn holder_must_stop(kind: LeaseKind, last_confirmed_renewal: Instant, now: Instant) -> bool {
    now.saturating_duration_since(last_confirmed_renewal) >= kind.stop_without_renewal()
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
    /// The content of a minute-lease claim at `epoch`, acquired at
    /// `acquired_at` (ms), written at `server_now` (ms), lasting [`TTL`];
    /// `released` hands it back.
    pub fn content(
        &self,
        epoch: u64,
        acquired_at: u64,
        server_now: u64,
        released: bool,
        window: Option<String>,
    ) -> ClaimContent {
        self.content_on(
            LeaseKind::Minute,
            epoch,
            acquired_at,
            server_now,
            released,
            window,
        )
    }

    /// [`Self::content`] on a `kind` lease: its expiry is that lease's TTL
    /// ahead, which is how a reader tells the kind ([`LeaseKind::of`]).
    pub fn content_on(
        &self,
        kind: LeaseKind,
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
                server_now.saturating_add(kind.ttl().as_millis() as u64)
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
        assert!(may_acquire(Some(&live), T0 + ttl), "lapsed at its TTL");
        assert_eq!(next_epoch(Some(&live)), 5);

        // Expiry is the event's server time, never the content's: a holder
        // whose clock runs ahead cannot buy itself time by writing a later
        // `expires_at`.
        let mut ahead = live.clone();
        ahead.content.expires_at = rfc3339(T0 + 10 * ttl);
        assert!(may_acquire(Some(&ahead), T0 + ttl));

        let renewed = Instant::now();
        assert!(!holder_must_stop(
            LeaseKind::Minute,
            renewed,
            renewed + STOP_WITHOUT_RENEWAL - Duration::from_secs(1)
        ));
        assert!(holder_must_stop(
            LeaseKind::Minute,
            renewed,
            renewed + STOP_WITHOUT_RENEWAL
        ));
    }

    fn manifest(host: &str, lease: Option<&str>, expires_at: u64) -> HostManifest {
        HostManifest {
            v: 1,
            host: host.to_owned(),
            principal: "tgorka".to_owned(),
            version: "0.101.0".to_owned(),
            always_on: true,
            tools: Vec::new(),
            drives: Vec::new(),
            bots: Vec::new(),
            agents: Vec::new(),
            renewed_at: rfc3339(expires_at.saturating_sub(180_000)),
            expires_at: rfc3339(expires_at),
            picture_failures: Default::default(),
            lease: lease.map(str::to_owned),
            web_search: None,
        }
    }

    /// A claim `host` wrote on the minute lease at `at`.
    fn minute_claim_by(host: &str, at: u64, released: bool) -> ServerClaim {
        let claimant = Claimant {
            host: host.to_owned(),
            device: host.to_uppercase(),
            agent: OwnedUserId::try_from("@nixi:example.org").expect("user"),
        };
        ServerClaim {
            event_id: OwnedEventId::try_from("$claim:example.org").expect("event"),
            sender: claimant.agent.clone(),
            origin_server_ts: at,
            content: claimant.content(2, at, at, released, None),
        }
    }

    /// R449, R455: the quarter-hour lease is taken only where the host's own
    /// setting turns it on — never by default, whatever the manifests say —
    /// and while every live host says it reads it; a live host that does
    /// not keeps the whole fleet on the minute lease, a lapsed manifest
    /// counts for nothing, with no live manifest at all it is the minute
    /// lease, and so it is beside a live minute claim from a host that does
    /// not advertise it — whether or not its manifest arrived.
    #[test]
    fn the_quarter_hour_lease_waits_for_the_setting_and_every_live_host() {
        let on = ClaimsLease::QuarterHour;
        let live = T0 + 60_000;
        let new = |host| manifest(host, Some(QUARTER_HOUR_LEASE), live);
        let fleet = [new("electra"), new("hesperia")];
        assert_eq!(
            LeaseKind::for_fleet(on, &fleet, &[], T0),
            LeaseKind::QuarterHour
        );
        assert_eq!(
            LeaseKind::for_fleet(ClaimsLease::default(), &fleet, &[], T0),
            LeaseKind::Minute,
            "the default, every manifest advertising it"
        );
        assert_eq!(
            LeaseKind::for_fleet(on, &[new("electra"), manifest("argo", None, live)], &[], T0),
            LeaseKind::Minute,
            "an old host is live"
        );
        assert_eq!(
            LeaseKind::for_fleet(on, &[new("electra"), manifest("argo", None, T0)], &[], T0),
            LeaseKind::QuarterHour,
            "the old host's manifest lapsed"
        );
        assert_eq!(
            LeaseKind::for_fleet(
                on,
                &[new("electra"), manifest("argo", Some("hour"), live)],
                &[],
                T0
            ),
            LeaseKind::Minute,
            "a lease this build does not know"
        );
        assert_eq!(LeaseKind::for_fleet(on, &[], &[], T0), LeaseKind::Minute);

        let argo_now = minute_claim_by("argo", T0 - 1_000, false);
        assert_eq!(
            LeaseKind::for_fleet(on, &fleet, &[argo_now], T0),
            LeaseKind::Minute,
            "an old host writes a claim before its manifest arrives"
        );
        assert_eq!(
            LeaseKind::for_fleet(on, &fleet, &[minute_claim_by("argo", T0, true)], T0),
            LeaseKind::QuarterHour,
            "a released claim is history"
        );
        assert_eq!(
            LeaseKind::for_fleet(
                on,
                &fleet,
                &[minute_claim_by("argo", T0 - TTL.as_millis() as u64, false)],
                T0
            ),
            LeaseKind::QuarterHour,
            "a lapsed claim is history"
        );
        assert_eq!(
            LeaseKind::for_fleet(
                on,
                &fleet,
                &[minute_claim_by("hesperia", T0 - 1_000, false)],
                T0
            ),
            LeaseKind::QuarterHour,
            "a minute claim from a host that reads the quarter-hour lease"
        );
    }

    /// R416, R449: a claim says its own lease, and a reader honours it — a
    /// quarter-hour claim lapses 45 min after its event, a minute one after
    /// 180 s — with the same proportions on both: two missed renewals stop
    /// the holder, a whole renewal before anyone may take over.
    #[test]
    fn a_claim_lapses_by_the_lease_it_says() {
        for kind in [LeaseKind::Minute, LeaseKind::QuarterHour] {
            assert_eq!(kind.stop_without_renewal(), kind.renew_every() * 2);
            assert_eq!(kind.ttl(), kind.renew_every() * 3);
            let claim = ServerClaim {
                content: claimant().content_on(kind, 1, T0, T0, false, None),
                ..claim(1, T0, false)
            };
            assert_eq!(LeaseKind::of(&claim.content), kind);
            let ttl = kind.ttl().as_millis() as u64;
            let stop = kind.stop_without_renewal().as_millis() as u64;
            assert!(
                !may_acquire(Some(&claim), T0 + stop),
                "{kind:?} at its stop"
            );
            assert!(!may_acquire(Some(&claim), T0 + ttl - 1), "{kind:?}");
            assert!(may_acquire(Some(&claim), T0 + ttl), "{kind:?} lapsed");
        }
        assert_eq!(
            LeaseKind::QuarterHour.renew_every(),
            Duration::from_secs(15 * 60)
        );
        assert_eq!(LeaseKind::Minute.renew_every(), Duration::from_secs(60));
        let released = claimant().content_on(LeaseKind::QuarterHour, 1, T0, T0, true, None);
        assert_eq!(LeaseKind::of(&released), LeaseKind::Minute);
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
