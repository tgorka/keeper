//! A person's ask that their proxy open its `main` DM with them (story
//! 101.1, R416).
//!
//! Only a proxy's host makes its DM (`keeper-agentd agents init` →
//! `seed::main_dm`); a room the person made would never be admitted (it is
//! not made by its agent). So with no DM here keeper sends one closed event,
//! [`MAIN_DM_REQUEST`], into the person's control room — the room the proxy
//! made with them in it — and the proxy's host answers it with the same
//! `seed::main_dm` and a [`MAIN_DM_ANSWER`]: the room, or the sentence why
//! not. keeper joins the invite (`main` as direct) and opens the room, and
//! waits for it no longer than [`MAIN_DM_WAIT`].
//!
//! Which request a host answers is [`decide`]'s: one in a control room,
//! naming this copy's agent, from that agent's own `human`, sent within
//! [`MAIN_DM_FRESH`] — by the server's `unsigned.age`, else by its
//! `origin_server_ts` against the host's calibrated server clock, else not
//! at all ([`request_age`], R450). Anyone else is refused with a sentence the
//! asker reads; a request for another agent, or an old one a restarted host
//! reads back, is not this host's to answer.
//!
//! keeper's side is [`open_within`]: one deadline from the moment the person
//! asked over the whole of it — finding the control room, sending, the
//! answered room's join and its direct mark, and every look at the rooms —
//! so nothing it waits on outlives [`MAIN_DM_WAIT`] (R450, as R409 bounds a
//! launch). The answered room is kept until it is joined and direct, so an
//! answer that arrives before its invite is settled when the invite lands;
//! and that one room's history is searched back to its status page after
//! page, so a DM whose status is buried under renewals is admitted on a
//! device that kept no seat for it (R450, review-101rooms-1 03).
//!
//! The request is a control-room state event keyed by the asker, as their
//! presence is: the control room's timeline is the agents' (`events_default`
//! 50), and an encrypted event there reaches the server as
//! `m.room.encrypted` at that level. A state event is not encrypted, so its
//! sender is the homeserver's word, not a device's seal — and that is
//! enough here: the request can only ever make the DM between the agent and
//! its own `human`, the one `keeper-agentd agents init` would make, and it
//! carries nothing but the agent's id.

use std::future::Future;
use std::time::Duration;

use matrix_sdk::ruma::{OwnedRoomId, RoomId, UserId};
use serde_json::Value;
use tokio::time::Instant;

use crate::agents::events::{MainDmRequestContent, CONTENT_VERSION};

/// The longest keeper waits for its proxy's DM after asking for it (R416).
pub const MAIN_DM_WAIT: Duration = Duration::from_secs(30);

/// How old a request a host still answers: one keeper stopped waiting for
/// long ago is not made into a room nobody is looking for.
pub const MAIN_DM_FRESH: Duration = Duration::from_secs(120);

/// A request as the host that received it reads it.
#[derive(Debug, Clone, Copy)]
pub struct Request<'a> {
    /// Whether the room is a control room.
    pub in_control_room: bool,
    pub sender: &'a UserId,
    pub content: &'a Value,
    /// How long ago the server received it, ms, when that can be told
    /// ([`request_age`]); `None` is a request whose freshness cannot be
    /// established, which is not answered.
    pub age_ms: Option<u64>,
}

/// How old a request is, ms: the server's `unsigned.age` plus how long the
/// host held it since, or with no age, the host's calibrated server clock
/// (`server_now`, `None` while uncalibrated) less the event's
/// `origin_server_ts` (`sent_ms`). `None` when neither can be told.
pub fn request_age(
    age_ms: Option<u64>,
    held_ms: u64,
    sent_ms: Option<u64>,
    server_now: Option<u64>,
) -> Option<u64> {
    match (age_ms, sent_ms, server_now) {
        (Some(age), _, _) => Some(age.saturating_add(held_ms)),
        (None, Some(sent), Some(now)) => Some(now.saturating_sub(sent)),
        _ => None,
    }
}

/// The agent this host's copy runs, as the request is read against it.
#[derive(Debug, Clone, Copy)]
pub struct Proxy<'a> {
    pub agent: &'a UserId,
    /// Its display name, for the sentences.
    pub name: &'a str,
    /// Its `human` (`agent.toml`); `None`: not a proxy.
    pub human: Option<&'a UserId>,
}

/// What a host does with a request.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Verdict {
    /// Not this host's to answer; why, for the debug log.
    Ignore(&'static str),
    /// Answered with this sentence and nothing made.
    Refuse(String),
    /// Open the DM with the sender, the proxy's human.
    Open,
}

/// What `proxy`'s host does with `request`.
pub fn decide(request: &Request<'_>, proxy: &Proxy<'_>) -> Verdict {
    if !request.in_control_room {
        return Verdict::Ignore("a DM request outside a control room");
    }
    let Ok(content) = serde_json::from_value::<MainDmRequestContent>(request.content.clone())
    else {
        return Verdict::Ignore("a DM request that does not read");
    };
    if content.v != CONTENT_VERSION {
        return Verdict::Ignore("a DM request of another version");
    }
    if content.agent != proxy.agent {
        return Verdict::Ignore("a DM request for another agent");
    }
    match request.age_ms {
        None => return Verdict::Ignore("a DM request whose age cannot be told"),
        Some(age) if u128::from(age) > MAIN_DM_FRESH.as_millis() => {
            return Verdict::Ignore("a DM request too old to answer");
        }
        Some(_) => {}
    }
    let name = proxy.name;
    let Some(human) = proxy.human else {
        return Verdict::Refuse(format!(
            "{name} is nobody's proxy, so it has no conversation to open with you."
        ));
    };
    if request.sender != human {
        return Verdict::Refuse(format!(
            "{name} opens its conversation only with {human}, whose proxy it is."
        ));
    }
    Verdict::Open
}

/// The sentence when keeper shares no control room with `name`: the room
/// the request goes into is made by `keeper-agentd agents init`.
pub fn no_control_room_sentence(name: &str) -> String {
    format!(
        "keeper cannot ask {name} for a conversation: you are in no control room with it. Run \
         `keeper-agentd agents init` on its host (it makes the control room and your \
         conversation), then ask again."
    )
}

/// The sentence when `name` did not answer within [`MAIN_DM_WAIT`].
pub fn not_answered_sentence(name: &str) -> String {
    format!(
        "{name} did not open a conversation within {} s: its host may be offline or not yet \
         updated. The conversation is in Agents once it arrives.",
        MAIN_DM_WAIT.as_secs()
    )
}

/// The sentence when the request could not be sent.
pub fn not_sent_sentence(name: &str, error: &str) -> String {
    format!("keeper could not ask {name} for a conversation: {error}")
}

/// The sentence when `name`'s host could not open the DM.
pub fn not_opened_sentence(name: &str, error: &str) -> String {
    format!("{name} could not open a conversation with you: {error}")
}

/// How often keeper looks at the rooms again while it waits for the DM.
pub const LOOK_AGAIN: Duration = Duration::from_secs(2);

/// What the agent's host answered.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Answer {
    /// The DM it opened or found.
    Room(OwnedRoomId),
    /// Why it did not, in its sentence.
    Refused(String),
}

/// What keeper's ask for a DM runs over: the account's rooms, its control
/// room and the host's answers (the account's client; a test's fake).
pub trait DmOpening {
    /// What an opened DM is answered as.
    type Opened;

    /// The agent asked.
    fn agent(&self) -> &UserId;

    /// The agent's name when it was not read in time: its localpart.
    fn fallback_name(&self) -> String {
        self.agent().localpart().to_owned()
    }

    /// The DM when it is here now among the agent's own direct rooms,
    /// admitted as the person's: no invite is joined and no other room is
    /// read, so it is quick whatever else the account holds.
    fn found(&self) -> impl Future<Output = Option<Self::Opened>> + Send;

    /// The DM when it is listed here now, admitted as the person's, after
    /// every waiting invite of the person's agents was joined and every
    /// session room read: as slow as the account is full.
    fn listed(&self) -> impl Future<Output = Option<Self::Opened>> + Send;

    /// The agent's name, for the sentences.
    fn name(&self) -> impl Future<Output = String> + Send;

    /// Find the control room, hear its answers, and send the request; the
    /// sentence when it cannot be sent.
    fn ask(&mut self) -> impl Future<Output = Result<(), String>> + Send;

    /// The next answer to the request; pending while there is none.
    fn answer(&self) -> impl Future<Output = Answer> + Send;

    /// Join `room` when it is an invite and mark it direct with the agent:
    /// whether it is both now. A room not here yet is not.
    fn settle(&self, room: &RoomId) -> impl Future<Output = bool> + Send;

    /// Search `room`'s history back to its newest status, page after page,
    /// and admit it: the DM, when it is the person's.
    fn admit(&self, room: &RoomId) -> impl Future<Output = Option<Self::Opened>> + Send;
}

/// Open the person's DM through `port`, everything of it bounded by
/// `deadline` (from the moment the person asked): the room when it is here
/// or once the host's answer has landed and is admitted; otherwise the
/// sentence — the host's refusal, why it could not be asked, or that it did
/// not answer in time — said with a WARN (R381).
pub async fn open_within<P: DmOpening>(
    port: &mut P,
    deadline: Instant,
) -> Result<P::Opened, String> {
    let mut name: Option<String> = None;
    let opened = tokio::time::timeout_at(deadline, attempt(port, &mut name)).await;
    let failed = match opened {
        Ok(Ok(open)) => return Ok(open),
        Ok(Err(sentence)) => sentence,
        Err(_) => not_answered_sentence(&name.unwrap_or_else(|| port.fallback_name())),
    };
    warn_not_opened(port.agent(), &failed);
    Err(failed)
}

/// The deadline passed before the account's client was even looked up —
/// the accounts are held by an activation or a shutdown (R450,
/// review-101rooms-2 02): the sentence that `agent` did not answer in time,
/// said with the same WARN as every other way the DM does not open.
pub fn not_reached_in_time(agent: &UserId) -> String {
    let failed = not_answered_sentence(agent.localpart());
    warn_not_opened(agent, &failed);
    failed
}

fn warn_not_opened(agent: &UserId, failed: &str) {
    tracing::warn!(agent = %agent, reason = %failed, "agents: the person's DM with an agent did not open");
}

/// [`open_within`]'s work, with no bound of its own: `name` is the agent's
/// name once it was read, for the sentence at the deadline.
///
/// Only the agent's own direct rooms are looked at before the request is
/// sent; the whole listing — invites of other rooms joined, every session
/// room read — runs after it, beside the wait for the answer and never in
/// its way, within what is left of the deadline (review-101rooms-2 06). An
/// account full of other rooms cannot spend the ask before it is made.
async fn attempt<P: DmOpening>(
    port: &mut P,
    name: &mut Option<String>,
) -> Result<P::Opened, String> {
    if let Some(open) = port.found().await {
        return Ok(open);
    }
    *name = Some(port.name().await);
    port.ask().await?;
    let port = &*port;
    // Kept until it is joined and direct: an answer may come before the
    // invite does (R450).
    let mut answered: Option<OwnedRoomId> = None;
    let listing = list_after(port, Duration::ZERO);
    tokio::pin!(listing);
    loop {
        tokio::select! {
            answer = port.answer() => match answer {
                Answer::Refused(sentence) => return Err(sentence),
                Answer::Room(room) => answered = Some(room),
            },
            listed = &mut listing => {
                if let Some(open) = listed {
                    return Ok(open);
                }
                listing.set(list_after(port, LOOK_AGAIN));
            }
            () = tokio::time::sleep(LOOK_AGAIN) => {}
        }
        // The listing is polled beside the step too: one left suspended in
        // the middle of a room's read holds the SDK's event cache, whose
        // lock every room shares, and the step's own read of the answered
        // room would wait on it until the deadline.
        let step = async {
            if let Some(room) = &answered {
                if port.settle(room).await {
                    if let Some(open) = port.admit(room).await {
                        return Some(open);
                    }
                }
            }
            // Once answered, the answered room is the one to settle: the
            // agent's other rooms are not read again meanwhile.
            if answered.is_none() {
                return port.found().await;
            }
            None
        };
        tokio::pin!(step);
        let stepped = loop {
            tokio::select! {
                opened = &mut step => break opened,
                listed = &mut listing => {
                    if let Some(open) = listed {
                        return Ok(open);
                    }
                    listing.set(list_after(port, LOOK_AGAIN));
                }
            }
        };
        if let Some(open) = stepped {
            return Ok(open);
        }
    }
}

/// The whole listing of `port`'s rooms, `after` from now.
async fn list_after<P: DmOpening>(port: &P, after: Duration) -> Option<P::Opened> {
    tokio::time::sleep(after).await;
    port.listed().await
}

#[cfg(test)]
pub(crate) mod tests {
    use matrix_sdk::ruma::OwnedUserId;
    use serde_json::json;

    use super::*;

    fn user(id: &str) -> OwnedUserId {
        OwnedUserId::try_from(id).expect("user")
    }

    /// R416: a host opens the DM only for its own agent's human, asking in a
    /// control room, recently; it refuses anyone
    /// else with a sentence naming whose proxy it is, and leaves another
    /// agent's request — or an old one read back — to nobody.
    #[test]
    fn only_the_proxys_own_human_gets_a_dm_and_anyone_else_a_sentence() {
        let (nixi, tola) = (user("@nixi:example.org"), user("@tola:example.org"));
        let (tgorka, marta) = (user("@tgorka:example.org"), user("@marta:example.org"));
        let content = json!({"v": 1, "agent": nixi, "ask": "01K"});
        fn asked<'a>(sender: &'a OwnedUserId, content: &'a Value) -> Request<'a> {
            Request {
                in_control_room: true,
                sender,
                content,
                age_ms: Some(1_000),
            }
        }
        let proxy = Proxy {
            agent: &nixi,
            name: "Nixi",
            human: Some(&tgorka),
        };
        assert_eq!(decide(&asked(&tgorka, &content), &proxy), Verdict::Open);

        let Verdict::Refuse(sentence) = decide(&asked(&marta, &content), &proxy) else {
            panic!("refused");
        };
        assert!(sentence.contains("@tgorka:example.org"), "{sentence}");
        let nobodys = Proxy {
            human: None,
            ..proxy
        };
        assert!(matches!(
            decide(&asked(&tgorka, &content), &nobodys),
            Verdict::Refuse(_)
        ));

        // Tola's host reads the same request and leaves it to Nixi's.
        let tolas = Proxy {
            agent: &tola,
            name: "Tola",
            human: Some(&tgorka),
        };
        assert!(matches!(
            decide(&asked(&tgorka, &content), &tolas),
            Verdict::Ignore(_)
        ));
        let old = Request {
            age_ms: Some(MAIN_DM_FRESH.as_millis() as u64 + 1),
            ..asked(&tgorka, &content)
        };
        assert!(matches!(decide(&old, &proxy), Verdict::Ignore(_)));
        let at_the_limit = Request {
            age_ms: Some(MAIN_DM_FRESH.as_millis() as u64),
            ..asked(&tgorka, &content)
        };
        assert_eq!(
            decide(&at_the_limit, &proxy),
            Verdict::Open,
            "120 s is fresh"
        );
        let ageless = Request {
            age_ms: None,
            ..asked(&tgorka, &content)
        };
        assert!(
            matches!(decide(&ageless, &proxy), Verdict::Ignore(_)),
            "an age nobody can tell is not answered"
        );
        let elsewhere = Request {
            in_control_room: false,
            ..asked(&tgorka, &content)
        };
        assert!(matches!(decide(&elsewhere, &proxy), Verdict::Ignore(_)));
        let newer = json!({"v": 2, "agent": nixi, "ask": "01K"});
        let unread = Request {
            content: &newer,
            ..asked(&tgorka, &content)
        };
        assert!(matches!(decide(&unread, &proxy), Verdict::Ignore(_)));
    }

    /// R450 (review-101rooms-1 10): a request's age is the server's
    /// `unsigned.age` plus what the host held it; without one, the host's
    /// calibrated server clock against the event's `origin_server_ts`; with
    /// neither — no age and an uncalibrated clock — it cannot be told.
    #[test]
    fn a_requests_age_comes_from_the_server_or_not_at_all() {
        let fresh = MAIN_DM_FRESH.as_millis() as u64;
        assert_eq!(request_age(Some(1_000), 500, None, None), Some(1_500));
        assert_eq!(
            request_age(None, 500, Some(10_000), Some(10_000 + fresh)),
            Some(fresh)
        );
        assert_eq!(
            request_age(None, 0, Some(10_000), Some(10_000 + fresh + 1)),
            Some(fresh + 1)
        );
        assert_eq!(
            request_age(None, 0, Some(10_000), None),
            None,
            "uncalibrated"
        );
        assert_eq!(request_age(None, 0, None, Some(10_000)), None);
    }

    /// What the bounded open is run over in these tests.
    struct Fake {
        agent: OwnedUserId,
        /// The DM is here from the start, among the agent's own rooms.
        here: bool,
        /// Finding the control room never ends.
        ask_hangs: bool,
        /// The whole listing never ends: an account full of other rooms.
        listing_hangs: bool,
        asked: std::sync::atomic::AtomicBool,
        /// The whole listing was started before the request was sent.
        listed_before_asking: std::sync::atomic::AtomicBool,
        answers: std::sync::Mutex<std::collections::VecDeque<Answer>>,
        /// `settle` answers "not yet" this many times, then "settled".
        settle_late: std::sync::atomic::AtomicU32,
        /// The join never answers.
        settle_hangs: bool,
        settled: std::sync::atomic::AtomicU32,
        /// The SDK's event cache, one lock every room shares: the listing
        /// holds it across a second of reading, and admitting needs it.
        cache: Option<tokio::sync::Mutex<()>>,
        /// How long the host takes to answer.
        answer_after: Duration,
    }

    impl Fake {
        fn new() -> Fake {
            Fake {
                agent: user("@nixi:example.org"),
                here: false,
                ask_hangs: false,
                listing_hangs: false,
                asked: std::sync::atomic::AtomicBool::new(false),
                listed_before_asking: std::sync::atomic::AtomicBool::new(false),
                answers: std::sync::Mutex::new(std::collections::VecDeque::new()),
                settle_late: std::sync::atomic::AtomicU32::new(0),
                settle_hangs: false,
                settled: std::sync::atomic::AtomicU32::new(0),
                cache: None,
                answer_after: Duration::ZERO,
            }
        }

        fn answering(mut self, answer: Answer) -> Fake {
            self.answers.get_mut().expect("answers").push_back(answer);
            self
        }
    }

    impl DmOpening for Fake {
        type Opened = OwnedRoomId;

        fn agent(&self) -> &UserId {
            &self.agent
        }

        async fn found(&self) -> Option<OwnedRoomId> {
            self.here.then(|| room("!here:example.org"))
        }

        async fn listed(&self) -> Option<OwnedRoomId> {
            use std::sync::atomic::Ordering;
            if !self.asked.load(Ordering::SeqCst) {
                self.listed_before_asking.store(true, Ordering::SeqCst);
            }
            if self.listing_hangs {
                std::future::pending::<()>().await;
            }
            if let Some(cache) = &self.cache {
                let _reading = cache.lock().await;
                tokio::time::sleep(Duration::from_secs(1)).await;
            }
            self.here.then(|| room("!here:example.org"))
        }

        async fn name(&self) -> String {
            "Nixi".to_owned()
        }

        async fn ask(&mut self) -> Result<(), String> {
            if self.ask_hangs {
                std::future::pending::<()>().await;
            }
            self.asked.store(true, std::sync::atomic::Ordering::SeqCst);
            Ok(())
        }

        async fn answer(&self) -> Answer {
            tokio::time::sleep(self.answer_after).await;
            let next = self.answers.lock().expect("answers").pop_front();
            match next {
                Some(answer) => answer,
                None => std::future::pending().await,
            }
        }

        async fn settle(&self, _room: &RoomId) -> bool {
            use std::sync::atomic::Ordering;
            if self.settle_hangs {
                std::future::pending::<()>().await;
            }
            self.settled.fetch_add(1, Ordering::Relaxed);
            self.settle_late
                .try_update(Ordering::Relaxed, Ordering::Relaxed, |n| n.checked_sub(1))
                .is_err()
        }

        async fn admit(&self, room: &RoomId) -> Option<OwnedRoomId> {
            if let Some(cache) = &self.cache {
                let _reading = cache.lock().await;
            }
            Some(room.to_owned())
        }
    }

    fn room(id: &str) -> OwnedRoomId {
        OwnedRoomId::try_from(id).expect("room")
    }

    /// The WARN lines `run` logs.
    pub(crate) async fn warned<T>(run: impl Future<Output = T>) -> (T, String) {
        #[derive(Clone, Default)]
        struct Captured(std::sync::Arc<std::sync::Mutex<Vec<u8>>>);
        impl std::io::Write for Captured {
            fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
                self.0.lock().expect("captured").extend_from_slice(bytes);
                Ok(bytes.len())
            }
            fn flush(&mut self) -> std::io::Result<()> {
                Ok(())
            }
        }
        let captured = Captured::default();
        let writer = captured.clone();
        let subscriber = tracing_subscriber::fmt()
            .with_ansi(false)
            .with_max_level(tracing::Level::WARN)
            .with_writer(move || writer.clone())
            .finish();
        let _default = tracing::subscriber::set_default(subscriber);
        let out = run.await;
        let text = String::from_utf8(captured.0.lock().expect("captured").clone()).expect("utf-8");
        (out, text)
    }

    /// R450 (review-101rooms-1 06): finding the control room that never
    /// answers ends at the one deadline set when the person asked — with
    /// the sentence and a WARN.
    #[tokio::test(start_paused = true)]
    async fn a_discovery_that_never_answers_ends_at_the_deadline() {
        let mut port = Fake {
            ask_hangs: true,
            ..Fake::new()
        };
        let asked = Instant::now();
        let (opened, log) = warned(open_within(&mut port, asked + MAIN_DM_WAIT)).await;
        assert_eq!(opened, Err(not_answered_sentence("Nixi")));
        assert_eq!(asked.elapsed(), MAIN_DM_WAIT);
        assert!(
            log.contains("WARN") && log.contains("did not open"),
            "{log}"
        );
    }

    /// R450 (review-101rooms-1 06): a join or history search after the
    /// answer that never returns ends at the same deadline, said and
    /// warned the same way.
    #[tokio::test(start_paused = true)]
    async fn a_join_that_never_answers_after_the_answer_ends_at_the_deadline() {
        let mut port = Fake {
            settle_hangs: true,
            ..Fake::new()
        };
        port.answers
            .get_mut()
            .expect("answers")
            .push_back(Answer::Room(room("!dm:example.org")));
        let asked = Instant::now();
        let (opened, log) = warned(open_within(&mut port, asked + MAIN_DM_WAIT)).await;
        assert_eq!(opened, Err(not_answered_sentence("Nixi")));
        assert_eq!(asked.elapsed(), MAIN_DM_WAIT);
        assert!(log.contains("WARN"), "{log}");
    }

    /// R450 (review-101rooms-1 09): an answer that lands before its invite
    /// is kept and settled once the room is here — the room opens — and a
    /// refusal is the host's sentence at once.
    #[tokio::test(start_paused = true)]
    async fn an_answer_before_its_invite_is_kept_until_the_room_settles() {
        let mut port = Fake {
            settle_late: std::sync::atomic::AtomicU32::new(3),
            ..Fake::new()
        };
        let dm = room("!dm:example.org");
        port.answers
            .get_mut()
            .expect("answers")
            .push_back(Answer::Room(dm.clone()));
        let asked = Instant::now();
        assert_eq!(open_within(&mut port, asked + MAIN_DM_WAIT).await, Ok(dm));
        assert_eq!(port.settled.load(std::sync::atomic::Ordering::Relaxed), 4);
        assert!(asked.elapsed() < MAIN_DM_WAIT);

        let mut refused = Fake::new();
        refused
            .answers
            .get_mut()
            .expect("answers")
            .push_back(Answer::Refused(
                "Nixi opens its conversation only with you.".to_owned(),
            ));
        assert_eq!(
            open_within(&mut refused, Instant::now() + MAIN_DM_WAIT).await,
            Err("Nixi opens its conversation only with you.".to_owned())
        );
        let mut here = Fake {
            here: true,
            ask_hangs: true,
            ..Fake::new()
        };
        assert_eq!(
            open_within(&mut here, Instant::now() + MAIN_DM_WAIT).await,
            Ok(room("!here:example.org")),
            "a DM already here is not asked for"
        );
    }

    /// R450 (review-101rooms-2 06): an account full of other rooms — the
    /// whole listing never ends — does not hold the request back: it is sent
    /// before the listing starts, and the answered room opens within the
    /// deadline while the listing still runs.
    #[tokio::test(start_paused = true)]
    async fn a_slow_listing_of_other_rooms_never_holds_the_request_back() {
        let dm = room("!dm:example.org");
        let mut port = Fake {
            listing_hangs: true,
            ..Fake::new()
        }
        .answering(Answer::Room(dm.clone()));
        let asked = Instant::now();
        assert_eq!(open_within(&mut port, asked + MAIN_DM_WAIT).await, Ok(dm));
        assert!(asked.elapsed() < MAIN_DM_WAIT, "{:?}", asked.elapsed());
        assert!(
            !port
                .listed_before_asking
                .load(std::sync::atomic::Ordering::SeqCst),
            "the request went first"
        );
    }

    /// A listing caught in the middle of a room's read holds the event
    /// cache every room shares; the answered room's admission waits on it,
    /// so the listing is still driven while the answered room settles and
    /// the DM opens once that read ends, not at the deadline.
    #[tokio::test(start_paused = true)]
    async fn a_listing_holding_the_cache_does_not_starve_the_answered_room() {
        let dm = room("!dm:example.org");
        let mut port = Fake {
            cache: Some(tokio::sync::Mutex::new(())),
            answer_after: Duration::from_millis(100),
            ..Fake::new()
        }
        .answering(Answer::Room(dm.clone()));
        let asked = Instant::now();
        assert_eq!(open_within(&mut port, asked + MAIN_DM_WAIT).await, Ok(dm));
        assert!(asked.elapsed() < LOOK_AGAIN, "{:?}", asked.elapsed());
    }
}
