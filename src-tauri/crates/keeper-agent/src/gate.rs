//! A gate's host side (AD-415, story 99.1): the desk that takes its
//! outside system's requests, keeps their deadlines and says each step in
//! the room.
//!
//! A request is taken where it arrives, before it waits for the session's
//! worker (codemap Q10): [`Desk::take`] decides it from the gate's
//! [`Ledger`] — refused `mode` or `busy` with no turn, ticketed at once
//! when it is async, or a turn by its deadline when it is sync — and arms
//! the deadline's timer on the homeserver's clock, so a request queued
//! behind a turn still gets its ticket on time. The worker writes each
//! step's `gate` line; a ticket the timer posted waits on the desk
//! ([`Desk::drain`]) until the worker writes it. A request's one entry
//! decides which of its answer and its ticket went first: whoever holds
//! it sends, and the other then sees what was sent — so a turn's late
//! `reply` becomes the ticket's `result`, and a ticket never follows an
//! answer.
//!
//! Every message the desk sends itself carries only the gate's own words
//! and the request's name, and still passes `check_sink(Room)` with the
//! gate's opening label against the room's members at that moment; the
//! turn's `reply` passed it with the session's label before it reaches
//! [`Desk::reply`]. Nothing here listens: the timer is a tokio timer.

use std::collections::{HashMap, VecDeque};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use keeper_core::agents::gate::{content, said, Admission, GateRequest, Ledger};
use keeper_core::agents::home::GateConfig;
use keeper_core::agents::label::{check_sink, Label, SinkVerdict};
use keeper_core::agents::log::{GateBody, GateReason, GateState};
use keeper_core::agents::matrix::AgentMatrixError;
use matrix_sdk::ruma::{EventId, OwnedEventId, OwnedUserId, TransactionId};
use serde_json::{json, Value};
use tokio::sync::Mutex as Held;
use ulid::Ulid;

use crate::agent::Arrived;
use crate::claims::ServerClock;
use crate::matrix_sink::EditPort;
use crate::rooms::Known;
use crate::sinks::room_audience;

/// How often a gate's message is tried before it is given up.
const SEND_TRIES: u32 = 5;
/// The wait between tries when the homeserver named none.
const SEND_BACKOFF: Duration = Duration::from_millis(500);

/// How many taken requests a desk remembers by their event.
const TAKEN_KEPT: usize = 4096;

/// How long, by the server's clock, after the gate said one refusal of a
/// request it took no turn for — `mode` or `busy` — it says no other: the
/// rest are logged, not echoed into the room (review R99S-04).
pub const REFUSAL_WINDOW_MS: u64 = 60_000;

/// The tickets open across one agent's gate sessions on this host, by
/// session: what its `[limits].max_concurrent_sessions` bounds (99.1 #6),
/// whichever of its gates holds them.
#[derive(Debug, Default)]
pub struct OpenTickets(Mutex<HashMap<String, usize>>);

/// What [`Desk::reply`] says when no request of that name waits.
pub const NO_SUCH_REQUEST: &str = "No request of that name waits for an answer here.";
/// What a second `reply` to one request says.
pub const ANSWERED_ALREADY: &str =
    "This request has had its answer, or was refused; nothing was sent.";
/// What a `reply` the homeserver did not take says.
pub const UNSENT: &str = "The answer could not be sent to the room; try reply again.";

/// What became of a request as it was taken.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Took {
    /// A turn answers it; a sync one's timer is armed.
    Turn,
    /// Ticketed at once (async): its ticket's id and the server time it was
    /// paid at.
    Ticketed { ticket: String, at: u64 },
    /// Not taken: no turn. `requested` when it paid its request token
    /// first (its `request` line is written), at `at`.
    Refused {
        reason: GateReason,
        requested: bool,
        at: u64,
    },
}

/// One request as the desk took it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Taken {
    pub request: GateRequest,
    pub took: Took,
}

/// A step the desk said in the room by itself — a ticket or a refusal at a
/// deadline — whose line the worker writes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Fact {
    pub line: GateBody,
    /// The gate's message that said it, when the homeserver took it.
    pub event: Option<OwnedEventId>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum State {
    /// Neither answered nor ticketed yet.
    Waiting,
    /// Ticketed: a reply now is its result.
    Ticketed,
    /// Answered, given its result, or refused.
    Done,
}

#[derive(Debug)]
struct Entry {
    request: String,
    event: OwnedEventId,
    deadline_ms: Option<u32>,
    state: State,
    ticket: Option<String>,
}

#[derive(Debug)]
struct Book {
    ledger: Ledger,
    /// Requests taken and not yet done, by name.
    live: HashMap<String, Arc<Held<Entry>>>,
    /// The requests taken lately, by their event: taking one twice — by
    /// the room's front, then by the worker — answers the same. The
    /// oldest go past [`TAKEN_KEPT`]; by then the worker's log knows them.
    taken: HashMap<OwnedEventId, Taken>,
    taken_order: VecDeque<OwnedEventId>,
    facts: Vec<Fact>,
    /// An async ticket's message, by request, once its send ended: the
    /// event when the homeserver took it ([`Desk::ticketed`]).
    tickets_said: HashMap<String, Option<OwnedEventId>>,
    /// When the gate last said a refusal it took no turn for.
    refused_said_at: Option<u64>,
}

/// One gate session's desk, shared by its room's front and its worker.
pub struct Desk {
    /// The gate's outside system.
    pub peer: OwnedUserId,
    /// Whether the session says its room was made in clear (Q9).
    pub clear: bool,
    deadline_ms_max: u32,
    /// The gate's own user.
    me: OwnedUserId,
    /// The gate's opening label: what a message of the desk's own carries.
    label: Label,
    port: Arc<dyn EditPort>,
    clock: Arc<ServerClock>,
    known: Option<Arc<Known>>,
    /// Whether this host still holds the session's claim.
    may_post: Arc<dyn Fn() -> bool + Send + Sync>,
    /// This session, as the agent's open tickets know it.
    session: String,
    tickets: Arc<OpenTickets>,
    book: Mutex<Book>,
}

impl std::fmt::Debug for Desk {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Desk").field("peer", &self.peer).finish()
    }
}

/// What a desk is opened with.
pub struct DeskSetup {
    pub gate: GateConfig,
    pub clear: bool,
    pub me: OwnedUserId,
    pub label: Label,
    pub port: Arc<dyn EditPort>,
    pub clock: Arc<ServerClock>,
    pub known: Option<Arc<Known>>,
    pub may_post: Arc<dyn Fn() -> bool + Send + Sync>,
    /// The session's path, and the open tickets of its agent's gates.
    pub session: String,
    pub tickets: Arc<OpenTickets>,
}

impl Desk {
    /// A desk over `ledger`, the one the session's `gate` lines rebuilt:
    /// what was paid and what is open survives a restart.
    pub fn open(setup: DeskSetup, ledger: Ledger) -> Arc<Desk> {
        setup
            .tickets
            .0
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .insert(setup.session.clone(), ledger.open());
        Arc::new(Desk {
            peer: setup.gate.peer.clone(),
            clear: setup.clear,
            deadline_ms_max: setup.gate.deadline_ms_max,
            me: setup.me,
            label: setup.label,
            port: setup.port,
            clock: setup.clock,
            known: setup.known,
            may_post: setup.may_post,
            session: setup.session,
            tickets: setup.tickets,
            book: Mutex::new(Book {
                ledger,
                live: HashMap::new(),
                taken: HashMap::new(),
                taken_order: VecDeque::new(),
                facts: Vec::new(),
                tickets_said: HashMap::new(),
                refused_said_at: None,
            }),
        })
    }

    fn book(&self) -> std::sync::MutexGuard<'_, Book> {
        self.book.lock().unwrap_or_else(|p| p.into_inner())
    }

    /// Pay `request`'s ticket at `at` in `book`, counting the tickets the
    /// agent's other gate sessions hold open (99.1 #6); what this session
    /// holds is told to them either way.
    fn pay_ticket(&self, book: &mut Book, request: &str, at: u64) -> bool {
        let mut open = self.tickets.0.lock().unwrap_or_else(|p| p.into_inner());
        let elsewhere = open
            .iter()
            .filter(|(session, _)| **session != self.session)
            .map(|(_, held)| *held)
            .sum();
        let paid = book.ledger.ticket(request, at, elsewhere);
        open.insert(self.session.clone(), book.ledger.open());
        paid
    }

    /// `request`'s ticket closed, or the request was refused — here, or
    /// in the room by another copy of the agent before a takeover.
    pub fn close(&self, request: &str) {
        let mut book = self.book();
        book.ledger.close(request);
        self.tickets
            .0
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .insert(self.session.clone(), book.ledger.open());
    }

    /// The server's time now, ms.
    pub fn now(&self) -> u64 {
        self.clock.now()
    }

    /// Take `arrived` as a request (99.1 #2, #3, #6, #9): `None` when it is
    /// none. The first take decides and starts what it needs — the refusal
    /// or the ticket said in the room, the deadline's timer — and every
    /// later one answers the same. A name the gate has open already gives
    /// way to the event's id, so two requests never share one.
    pub fn take(self: &Arc<Self>, arrived: &Arrived) -> Option<Taken> {
        if let Some(taken) = self.book().taken.get(&arrived.event_id) {
            return Some(taken.clone());
        }
        let now = self.clock.now();
        let value = json!({
            "type": "m.room.message",
            "sender": arrived.sender,
            "event_id": arrived.event_id,
            "origin_server_ts": arrived.origin_server_ts.unwrap_or(now),
            "content": arrived.content,
        });
        let mut request = GateRequest::from_event(&value, &self.peer, self.deadline_ms_max)?;
        // The sender's homeserver stamps `origin_server_ts`: a time past
        // this server's now is not believed, so a forged one neither
        // refills the hour's allowance nor arms a far deadline (R99S-02).
        request.server_ts = request.server_ts.min(now);
        let mut book = self.book();
        if book.live.contains_key(&request.request) || book.ledger.is_open(&request.request) {
            request.request = request.event.to_string();
        }
        let entry = |state, ticket| {
            Arc::new(Held::new(Entry {
                request: request.request.clone(),
                event: request.event.clone(),
                deadline_ms: request.deadline_ms,
                state,
                ticket,
            }))
        };
        let took = match book.ledger.admit(&request) {
            Admission::Refused(reason) => {
                self.refuse_untaken(&mut book, &request, reason, request.server_ts);
                Took::Refused {
                    reason,
                    requested: false,
                    at: request.server_ts,
                }
            }
            Admission::Ticket => {
                if self.pay_ticket(&mut book, &request.request, now) {
                    let ticket = Ulid::new().to_string();
                    let held = entry(State::Ticketed, Some(ticket.clone()));
                    book.live.insert(request.request.clone(), Arc::clone(&held));
                    // Held before the post starts: a reply waits for the
                    // ticket to be said, and is its result; the worker
                    // writes its line with what the send gave.
                    if let Ok(guard) = held.try_lock_owned() {
                        let line = ticket_line(&request.request, &ticket, None, now);
                        let (desk, event) = (Arc::clone(self), request.event.clone());
                        tokio::spawn(async move {
                            let _held = guard;
                            let said = desk.say(&line, &event, true).await;
                            desk.book().tickets_said.insert(line.request, said);
                        });
                    }
                    Took::Ticketed { ticket, at: now }
                } else {
                    self.refuse_untaken(&mut book, &request, GateReason::Busy, now);
                    Took::Refused {
                        reason: GateReason::Busy,
                        requested: true,
                        at: now,
                    }
                }
            }
            Admission::Turn { deadline_at } => {
                let held = entry(State::Waiting, None);
                book.live.insert(request.request.clone(), Arc::clone(&held));
                if let Some(at) = deadline_at {
                    let desk = Arc::clone(self);
                    tokio::spawn(async move {
                        let wait = at.saturating_sub(desk.clock.now());
                        tokio::time::sleep(Duration::from_millis(wait)).await;
                        desk.deadline(held).await;
                    });
                }
                Took::Turn
            }
        };
        let taken = Taken { request, took };
        book.taken.insert(arrived.event_id.clone(), taken.clone());
        book.taken_order.push_back(arrived.event_id.clone());
        if book.taken_order.len() > TAKEN_KEPT {
            if let Some(oldest) = book.taken_order.pop_front() {
                book.taken.remove(&oldest);
            }
        }
        Some(taken)
    }

    /// Say a refusal of `request` that took no turn — unless the gate said
    /// one within [`REFUSAL_WINDOW_MS`]: then it is only logged, so a peer
    /// past its allowance is not echoed into the room message by message.
    fn refuse_untaken(
        self: &Arc<Self>,
        book: &mut Book,
        request: &GateRequest,
        reason: GateReason,
        at: u64,
    ) {
        let now = self.clock.now();
        if book
            .refused_said_at
            .is_some_and(|said| now < said.saturating_add(REFUSAL_WINDOW_MS))
        {
            tracing::info!(
                request = %request.event,
                reason = reason.as_str(),
                "agents: a gate's refusal was not said: it said one within the minute"
            );
            return;
        }
        book.refused_said_at = Some(now);
        let line = refused_line(&request.request, None, reason, at);
        let (desk, event) = (Arc::clone(self), request.event.clone());
        tokio::spawn(async move {
            desk.say(&line, &event, true).await;
        });
    }

    /// The message of `request`'s async ticket, once its send ended: its
    /// event when the homeserver took it, `None` when nothing was said.
    pub async fn ticketed(&self, request: &str) -> Option<OwnedEventId> {
        let held = self.book().live.get(request).cloned();
        if let Some(held) = held {
            drop(held.lock().await);
        }
        self.book().tickets_said.remove(request).flatten()
    }

    /// A sync request's deadline came (Q10): unless it was answered first,
    /// its ticket — paid from the hour's allowance and the agent's sessions —
    /// or, past them, its refusal `busy`. The turn working it goes on.
    async fn deadline(&self, held: Arc<Held<Entry>>) {
        let mut entry = held.lock().await;
        if entry.state != State::Waiting {
            return;
        }
        let now = self.clock.now();
        let paid = {
            let mut book = self.book();
            self.pay_ticket(&mut book, &entry.request, now)
        };
        let line = if paid {
            let ticket = Ulid::new().to_string();
            entry.ticket = Some(ticket.clone());
            entry.state = State::Ticketed;
            ticket_line(&entry.request, &ticket, entry.deadline_ms, now)
        } else {
            entry.state = State::Done;
            refused_line(&entry.request, None, GateReason::Busy, now)
        };
        let event = self.say(&line, &entry.event, true).await;
        let mut book = self.book();
        if !paid {
            book.live.remove(&entry.request);
        }
        book.facts.push(Fact { line, event });
    }

    /// The turn's `reply` to `request` (codemap Q6), its sink already
    /// passed: the answer while it is not ticketed, else the ticket's
    /// result. Its lines — the ticket the timer said first, if its line is
    /// not written yet, then its own — or why nothing was sent.
    pub async fn reply(&self, request: &str, text: &str) -> Result<Vec<Fact>, String> {
        let held = self
            .book()
            .live
            .get(request)
            .cloned()
            .ok_or_else(|| NO_SUCH_REQUEST.to_owned())?;
        let mut entry = held.lock().await;
        let now = self.clock.now();
        let line = match entry.state {
            State::Waiting => GateBody {
                request: entry.request.clone(),
                state: GateState::Answer,
                ticket: None,
                deadline_ms: entry.deadline_ms,
                server_ts: now,
                reason: None,
                text: None,
            },
            State::Ticketed => GateBody {
                request: entry.request.clone(),
                state: GateState::Result,
                ticket: entry.ticket.clone(),
                deadline_ms: None,
                server_ts: now,
                reason: None,
                text: None,
            },
            State::Done => return Err(ANSWERED_ALREADY.to_owned()),
        };
        let body = content(&line, &entry.event, text);
        let Some(event) = self.send(body).await else {
            return Err(UNSENT.to_owned());
        };
        entry.state = State::Done;
        self.close(&entry.request);
        let mut lines = self.facts_of(&entry.request);
        lines.push(Fact {
            line,
            event: Some(event),
        });
        Ok(lines)
    }

    /// The steps the timer said about `request` whose lines are not
    /// written yet, taken off the desk: they go before what follows them.
    fn facts_of(&self, request: &str) -> Vec<Fact> {
        let mut book = self.book();
        let (of, rest) = std::mem::take(&mut book.facts)
            .into_iter()
            .partition(|fact| fact.line.request == request);
        book.facts = rest;
        of
    }

    /// `request`'s turn ended (Q6): a request it neither answered nor gave
    /// a result is refused `no_answer`, its ticket with it. The lines still
    /// to write of it, in order.
    pub async fn finish(&self, request: &str) -> Vec<Fact> {
        let Some(held) = self.book().live.remove(request) else {
            return self.facts_of(request);
        };
        let mut entry = held.lock().await;
        let mut lines = self.facts_of(request);
        if entry.state == State::Done {
            return lines;
        }
        entry.state = State::Done;
        let line = refused_line(
            &entry.request,
            entry.ticket.clone(),
            GateReason::NoAnswer,
            self.clock.now(),
        );
        let event = self.say(&line, &entry.event, true).await;
        self.close(&entry.request);
        lines.push(Fact { line, event });
        lines
    }

    /// Refuse `request`, whose event is `event`, for `reason` now — a turn
    /// a stop cut off (C6) — closing its ticket. Its line and event.
    pub async fn refuse(
        &self,
        request: &str,
        event: &EventId,
        ticket: Option<String>,
        reason: GateReason,
    ) -> (GateBody, Option<OwnedEventId>) {
        let held = self.book().live.remove(request);
        if let Some(held) = held {
            held.lock().await.state = State::Done;
        }
        let line = refused_line(request, ticket, reason, self.clock.now());
        let sent = self.say(&line, event, true).await;
        self.close(request);
        (line, sent)
    }

    /// The steps said by the timer since the last drain, oldest first.
    pub fn drain(&self) -> Vec<Fact> {
        std::mem::take(&mut self.book().facts)
    }

    /// Say `line` about the request `event` in the room, in the gate's own
    /// words; `check` asks the room's sink first. The event, when the
    /// homeserver took it.
    async fn say(&self, line: &GateBody, event: &EventId, check: bool) -> Option<OwnedEventId> {
        if check && !self.reaches_room().await {
            return None;
        }
        let body = said(
            line.state,
            &line.request,
            line.ticket.as_deref(),
            line.reason,
        );
        self.send(content(line, event, &body)).await
    }

    /// Whether the gate's opening label reaches the room as it is now.
    async fn reaches_room(&self) -> bool {
        let members = match self.port.members().await {
            Ok(members) => members,
            Err(error) => {
                tracing::warn!(%error, "agents: a gate's room could not be read, so nothing was said there");
                return false;
            }
        };
        let sink = room_audience(
            members,
            self.known.as_deref(),
            std::slice::from_ref(&self.me),
        );
        match check_sink(&self.label, &sink) {
            SinkVerdict::Allow => true,
            SinkVerdict::Block { reason, .. } => {
                tracing::warn!(%reason, "agents: a gate's message was not said in its room");
                false
            }
        }
    }

    /// Send `content` while this host holds the claim, tried a few times
    /// under one transaction.
    async fn send(&self, content: Value) -> Option<OwnedEventId> {
        let txn = TransactionId::new();
        for _ in 0..SEND_TRIES {
            if !(self.may_post)() {
                return None;
            }
            match self
                .port
                .send("m.room.message", content.clone(), txn.clone())
                .await
            {
                Ok(event) => return Some(event),
                Err(AgentMatrixError::RateLimited { retry_after_ms }) => {
                    let wait = retry_after_ms.map_or(SEND_BACKOFF, Duration::from_millis);
                    tokio::time::sleep(wait).await;
                }
                Err(error) => {
                    tracing::warn!(%error, "agents: a gate's message was not taken; trying again");
                    tokio::time::sleep(SEND_BACKOFF).await;
                }
            }
        }
        None
    }
}

/// A `ticket` line.
pub fn ticket_line(request: &str, ticket: &str, deadline_ms: Option<u32>, at: u64) -> GateBody {
    GateBody {
        request: request.to_owned(),
        state: GateState::Ticket,
        ticket: Some(ticket.to_owned()),
        deadline_ms,
        server_ts: at,
        reason: None,
        text: None,
    }
}

/// A `refused` line.
pub fn refused_line(
    request: &str,
    ticket: Option<String>,
    reason: GateReason,
    at: u64,
) -> GateBody {
    GateBody {
        request: request.to_owned(),
        state: GateState::Refused,
        ticket,
        deadline_ms: None,
        server_ts: at,
        reason: Some(reason),
        text: None,
    }
}

/// The request a gate's turn answers, and its desk: what its `reply` sends
/// through.
#[derive(Clone)]
pub struct Serving {
    pub desk: Arc<Desk>,
    pub request: String,
}
