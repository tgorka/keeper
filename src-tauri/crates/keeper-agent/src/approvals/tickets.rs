//! A gate's tickets on their way to a person and on (AD-415, AD-416;
//! story 99.1 #4, #10; codemap Q2, Q4, Q8).
//!
//! A gate's `delegate` never runs in its turn and never parks it: the turn
//! writes a `delegate requested` line naming its ticket and a fresh
//! approval id, and ends. The host then writes that approval's record —
//! bound to the call's arguments, the ticket and the request — and asks
//! for it on the hour's card in the gate's own room: one
//! `approval.request` per window of an hour, a later ticket added by an
//! `m.replace` carrying the whole list, each record exactly as sent. The
//! people who decide are the home drive's pinned readers with a pinned key
//! ([`keeper_core::agents::gate::approvers`]), never the room's audience.
//! A decision is taken, written and consumed as any approval's is
//! ([`ServedSession::decided`]); once consumed, the host — not a turn —
//! opens the delegation as 92.1 does: the room made inviting the target
//! and the approvers, the brief only after the target joined. A denial or
//! an expiry refuses the ticket in the room.

use keeper_core::agents::gate::{approvers, HourCard, CARD_VERSION};
use keeper_core::agents::log::{DelegateBody, DelegateState, GateReason};
use keeper_core::agents::tier::{self as tier_rules, CallFacts, Context};

use super::*;
use crate::agent::{Deferred, JournalAsk};

/// The hour's card, kept beside the gate session's records.
const CARD_FILE: &str = "card.json";

/// Why a ticket's card could not be sent into its room.
const CARD_UNSENT: &str = "the hour's card could not be sent into the gate's room";

impl ServedSession {
    /// Who decides this gate's tickets (codemap Q2 (a)): the home drive's
    /// pinned readers whose master key this host pins in `[[trust]]`.
    /// Nobody on a host with no pinned trust — a desktop never serves a
    /// gate (Q14).
    pub(crate) fn gate_approvers(&self, deps: &AgentDeps) -> Vec<OwnedUserId> {
        match deps.decisions.as_ref().map(|source| source.anchor()) {
            Some(Anchor::Pinned(trust)) => approvers(&deps.home.drive.readers, trust),
            _ => Vec::new(),
        }
    }

    /// Ask for every ticket handed on whose card a turn's end or a stop
    /// left unasked: its record written once (or the one written already),
    /// then the hour's card. A card the room did not take this time leaves
    /// the ticket unasked, asked again at the next turn's end or start, as
    /// R176 announces a record again; one that cannot be asked at all is
    /// refused in the room, its call's row closed.
    pub(crate) async fn raise_tickets(
        &mut self,
        deps: &AgentDeps,
        port: &Arc<dyn EditPort>,
    ) -> Result<(), ServeError> {
        let unasked: Vec<(String, Deferred)> = self
            .context
            .gate_deferred
            .iter()
            .filter(|(_, deferred)| !deferred.announced && !deferred.decided && !deferred.opened)
            .map(|(ticket, deferred)| (ticket.clone(), deferred.clone()))
            .collect();
        for (ticket, deferred) in unasked {
            let record = match self.read_record(deps, &deferred.approval) {
                Ok((record, _)) => Ok(record),
                Err(_) => self.ticket_record(deps, &ticket, &deferred),
            };
            let asked = match record {
                Ok(record) => {
                    self.due.insert(record.id.clone(), record.expires());
                    self.announce(deps, port, &record, deferred.call_line).await
                }
                Err(reason) => Err(reason),
            };
            match asked {
                Ok(()) => {}
                Err(reason) if reason == CARD_UNSENT => {
                    tracing::info!(%ticket, "agents: a gate's card was not taken by its room; it is asked for again later");
                }
                Err(reason) => {
                    tracing::warn!(%ticket, %reason, "agents: a gate's ticket could not be asked for; it is refused");
                    self.due.remove(&deferred.approval);
                    self.close_row(deps, &deferred.approval, AuditOutcome::Refused);
                    self.refuse_ticket(deps, port, &ticket, &deferred, reason, GateReason::Failed)
                        .await?;
                }
            }
        }
        Box::pin(self.raise_journals(deps, port)).await;
        Ok(())
    }

    /// The approval record of the ticket `ticket` handed on as `deferred`,
    /// written once: the `delegate` call's arguments as the model sent
    /// them, bound with the ticket and the request it serves; classified as
    /// the call was (a gate's send, T1 raised to T2, once only); under the
    /// gate's opening label, which the request was asked under.
    fn ticket_record(
        &mut self,
        deps: &AgentDeps,
        ticket: &str,
        deferred: &Deferred,
    ) -> Result<ApprovalRecord, String> {
        let args: Value = serde_json::from_str(&deferred.args).map_err(|e| e.to_string())?;
        let request = self
            .context
            .gate_pending
            .iter()
            .find(|(_, open)| open.ticket.as_deref() == Some(ticket))
            .map(|(request, _)| request.clone())
            .ok_or_else(|| "its request is closed".to_owned())?;
        let label = self.context.agent.label.clone();
        self.gate_record(
            deps,
            AgentTool::Delegate,
            (deferred.call_line, &deferred.call_id, &deferred.approval),
            &args,
            serde_json::json!({ "ticket": ticket, "request": request }),
            &label,
        )
    }

    /// The approval record `approval` of a gate's deferred call — the
    /// `tool_call` line `call_line` of wire id `call_id` — written once:
    /// its arguments as the model sent them, bound with `exec_binding`;
    /// classified as the call was (a gate's send, raised once); under
    /// `label`.
    fn gate_record(
        &mut self,
        deps: &AgentDeps,
        tool: AgentTool,
        (call_line, call_id, approval): (Ulid, &str, &str),
        args: &Value,
        exec_binding: Value,
        label: &keeper_core::agents::label::Label,
    ) -> Result<ApprovalRecord, String> {
        fn failed(error: impl std::fmt::Display) -> String {
            error.to_string()
        }
        let agent = self.context.agent.clone();
        let facts = CallFacts {
            gate_send: true,
            ..tier_rules::named_facts(tool, args, CallFacts::default())
        };
        let classification = tier_rules::classify(
            tool,
            &facts,
            &Context::of_session(&agent, agent.label.integrity),
        );
        crate::agent::off_the_runtime(|| self.writer.sync()).map_err(failed)?;
        let place = self
            .writer
            .last_place()
            .cloned()
            .ok_or_else(|| "the session's log has no line to check against".to_owned())?;
        let session_dir = deps.sessions_zone.join(&self.context.session.path);
        let sha = chunk_sha256(&session_dir, &place.chunk, place.through_bytes).map_err(failed)?;
        let session = format!("{}/{}", deps.sessions_subfolder, self.context.session.path);
        let mut record = ApprovalRecord::new(RecordParking {
            id: approval,
            created_at: Utc::now(),
            session: &session,
            session_kind: agent.kind,
            agent: &deps.home.config.id,
            drive: &deps.home.config.drive,
            host: deps.host.as_str(),
            epoch: self.context.epoch,
            call: CallRef {
                line: call_line.to_string(),
                call_id: call_id.to_owned(),
            },
            dispatch_chain: agent
                .dispatch_chain
                .iter()
                .map(|user| user.to_string())
                .collect(),
            checkpoint: Checkpoint {
                chunk: format!("{}/{}", keeper_core::agents::log::LOG_DIR, place.chunk),
                through: place.line.to_string(),
                sha256: sha,
            },
            args,
            exec_binding,
            classification: &classification,
            label,
            preconditions: Preconditions::default(),
        })
        .map_err(failed)?;
        let dir = self.approvals(deps, true).map_err(failed)?;
        let blob = record.externalise_args();
        let text = serde_json::to_string_pretty(&record).map_err(failed)?;
        let id = record.id.clone();
        crate::agent::off_the_runtime(|| {
            if let Some((sha, bytes)) = &blob {
                let blobs = contained(&dir.join("blobs"), true)?;
                match write_once(&blobs, &format!("{sha}.json"), bytes.as_bytes()) {
                    Err(error) if error.kind() != std::io::ErrorKind::AlreadyExists => {
                        return Err(error)
                    }
                    _ => {}
                }
            }
            write_once(&dir, &format!("{id}.json"), text.as_bytes())
        })
        .map_err(failed)?;
        Ok(record)
    }

    /// Ask for every journal entry a gate's turn deferred (99.2 #2) whose
    /// card a turn's end or a stop left unasked: its record, then the
    /// hour's card, under the session's label as the turn left it. A card
    /// the room did not take this time leaves the entry unasked, asked
    /// again at the next turn's end or start, as a ticket's is; one that
    /// cannot be asked at all — a label the room may not read among them
    /// (R375) — is refused for good, its approval ended, its call's row
    /// closed, and nothing is written.
    async fn raise_journals(&mut self, deps: &AgentDeps, port: &Arc<dyn EditPort>) {
        let unasked: Vec<(String, JournalAsk)> = self
            .context
            .gate_journals
            .iter()
            .filter(|(_, ask)| !ask.announced && !ask.decided)
            .map(|(id, ask)| (id.clone(), ask.clone()))
            .collect();
        for (approval, ask) in unasked {
            let record = match self.read_record(deps, &approval) {
                Ok((record, _)) => Ok(record),
                Err(_) => {
                    let label = self.context.label.clone();
                    serde_json::from_str::<Value>(&ask.args)
                        .map_err(|error| error.to_string())
                        .and_then(|args| {
                            self.gate_record(
                                deps,
                                AgentTool::JournalAppend,
                                (ask.call_line, &ask.call_id, &approval),
                                &args,
                                Value::Null,
                                &label,
                            )
                        })
                }
            };
            let asked = match record {
                Ok(record) => {
                    self.due.insert(record.id.clone(), record.expires());
                    self.announce(deps, port, &record, ask.call_line).await
                }
                Err(reason) => Err(reason),
            };
            let reason = match asked {
                Ok(()) => continue,
                Err(reason) if reason == CARD_UNSENT => {
                    tracing::info!(%approval, "agents: a gate's journal card was not taken by its room; it is asked for again later");
                    continue;
                }
                Err(reason) => reason,
            };
            tracing::warn!(%approval, %reason, "agents: a gate's journal entry could not be asked for; it is refused");
            self.due.remove(&approval);
            self.close_row(deps, &approval, AuditOutcome::Refused);
            let mut body = line(&approval, ApprovalState::Refused);
            body.reason = Some(reason);
            let ended = self
                .writer
                .write(
                    &mut self.context,
                    Some(ask.call_line),
                    None,
                    LineBody::Approval(body),
                )
                .and_then(|_| crate::agent::off_the_runtime(|| self.writer.sync()));
            if let Err(error) = ended {
                tracing::error!(%approval, %error, "agents: a refused journal entry's line could not be written");
            }
        }
    }

    /// Ask for `record`, a gate's ticket whose call is the `tool_call` line
    /// `call_line`, on the hour's card (99.1 #10, codemap Q8): added by an
    /// edit carrying the whole list to the card whose hour it falls in and
    /// whose budget it fits, else the first record of a new card; the card
    /// kept beside the records, then its `approval requested` line naming
    /// the card's own event — where the room is read forward from for its
    /// `consumed`. The card passes the room's sink under the record's label
    /// at the send.
    pub(super) async fn on_the_card(
        &mut self,
        deps: &AgentDeps,
        port: &Arc<dyn EditPort>,
        record: &ApprovalRecord,
        call_line: Ulid,
        file: Option<Value>,
    ) -> Result<(), String> {
        fn failed(error: impl std::fmt::Display) -> String {
            error.to_string()
        }
        let approvers = self.gate_approvers(deps);
        if approvers.is_empty() {
            return Err(NOBODY_TO_ASK.to_owned());
        }
        let content = request_content(
            record,
            self.context.agent.room.as_ref(),
            approvers.iter().map(|person| person.to_string()).collect(),
            file,
        );
        let dir = self.approvals(deps, true).map_err(failed)?;
        let now = self
            .gate_desk
            .as_ref()
            .map_or_else(crate::claims::wall_ms, |desk| desk.now());
        let kept: Option<HourCard> =
            crate::agent::off_the_runtime(|| read_text_in(&dir, CARD_FILE))
                .ok()
                .and_then(|text| serde_json::from_str(&text).ok());
        let joins = kept.filter(|card| card.takes(now, &content));
        let value = match &joins {
            Some(card) => card.edit(&content),
            None => HourCard::content(std::slice::from_ref(&content)),
        };
        let gate = self.gate_under(deps, port, record.label.clone());
        gate.admit(APPROVAL_REQUEST, value.to_string().as_bytes())
            .await?;
        let sent = crate::matrix_sink::deliver_unless_narrowed(
            port.as_ref(),
            APPROVAL_REQUEST,
            &gate,
            &|narrowed| (!narrowed).then(|| value.clone()),
            Instant::now(),
        )
        .await
        .map(|(sent, _)| sent)
        .ok_or_else(|| CARD_UNSENT.to_owned())?;
        let card = match joins {
            Some(mut card) => {
                card.records.push(content);
                card
            }
            None => HourCard {
                v: CARD_VERSION,
                event: sent,
                opened_at: now,
                records: vec![content],
            },
        };
        let text = serde_json::to_string_pretty(&card).map_err(failed)?;
        crate::agent::off_the_runtime(|| write_replacing(&dir, CARD_FILE, text.as_bytes()))
            .map_err(failed)?;
        self.writer
            .write(
                &mut self.context,
                Some(call_line),
                Some(card.event.clone()),
                LineBody::Approval(line(&record.id, ApprovalState::Requested)),
            )
            .map_err(failed)?;
        crate::agent::off_the_runtime(|| self.writer.sync()).map_err(failed)?;
        Ok(())
    }

    /// A gate ticket's approval `pending` settled (codemap Q4): consumed
    /// here, the host hands the request on — the delegation's room made,
    /// the brief to follow its target's join; denied, expired or refused,
    /// the ticket is refused in the room with why. Its call's one row
    /// closes either way (R172).
    pub(super) async fn ticket_settled(
        &mut self,
        deps: &AgentDeps,
        port: &Arc<dyn EditPort>,
        pending: &Pending,
        settled: Settled,
    ) {
        let Some((ticket, deferred)) = self
            .context
            .gate_deferred
            .iter()
            .find(|(_, deferred)| deferred.approval == pending.id && !deferred.opened)
            .map(|(ticket, deferred)| (ticket.clone(), deferred.clone()))
        else {
            return;
        };
        let refusal = match settled {
            Settled::Run(call, _, _) => match self.open_ticket(deps, &ticket, &call).await {
                Ok(()) => {
                    self.close_row(deps, &pending.id, AuditOutcome::Ok);
                    return;
                }
                Err(reason) => {
                    tracing::warn!(%ticket, %reason, "agents: an approved gate ticket could not be handed on");
                    (not_done(&reason), GateReason::Failed, AuditOutcome::Failed)
                }
            },
            Settled::Refuse(said) => {
                let reason = if said == DENIED {
                    GateReason::Denied
                } else if said == EXPIRED || said == APPROVED_UNUSED {
                    GateReason::Expired
                } else {
                    GateReason::Failed
                };
                (said, reason, AuditOutcome::Refused)
            }
        };
        let (said, reason, outcome) = refusal;
        self.close_row(deps, &pending.id, outcome);
        if let Err(error) = self
            .refuse_ticket(deps, port, &ticket, &deferred, said, reason)
            .await
        {
            tracing::error!(%ticket, %error, "agents: a gate's ticket could not be refused");
        }
    }

    /// Hand the ticket `ticket` on as its approved `call` binds it (92.1,
    /// codemap Q4): its target resolved and still one of the gate's
    /// delegates, the room made inviting it and the approvers as
    /// observers, watched, and its `delegate opened` line under the call's
    /// `tool_call` line — whose arguments the brief is composed from once
    /// the target joins (F5).
    async fn open_ticket(
        &mut self,
        deps: &AgentDeps,
        ticket: &str,
        call: &chat::ToolCall,
    ) -> Result<(), String> {
        let rooms = self
            .delegations
            .clone()
            .ok_or_else(|| "this host reaches no delegation rooms".to_owned())?;
        let from = self.delegator(deps);
        let gate = from
            .gate
            .clone()
            .ok_or_else(|| "this session is no gate".to_owned())?;
        let named = crate::delegate::agent_of(&call.arguments_raw)
            .ok_or_else(|| "its arguments do not read".to_owned())?;
        let known = rooms.known();
        let target = crate::delegate::resolve(&known, &named)?;
        if !keeper_core::agents::gate::selects(&gate.delegates, &target.drive, &target.id) {
            return Err(crate::delegate::not_a_delegate(&named));
        }
        let invites = crate::delegate::gate_invites(&target.matrix_user, &gate.approvers);
        let name = keeper_core::agents::delegation::session_title(&target.id, Utc::now());
        let room = rooms
            .create(
                keeper_core::agents::session::SessionKind::Delegated,
                &name,
                invites,
                vec![target.matrix_user.clone()],
            )
            .await
            .map_err(|error| format!("the delegation's room could not be made: {error}"))?;
        rooms.watch(&room, &from.room, from.kind);
        let call_line = self
            .context
            .gate_deferred
            .get(ticket)
            .map(|deferred| deferred.call_line);
        self.writer
            .write(
                &mut self.context,
                call_line,
                None,
                LineBody::Delegate(DelegateBody {
                    id: ticket.to_owned(),
                    to: target.matrix_user.to_string(),
                    room: Some(room),
                    child: None,
                    state: DelegateState::Opened,
                    reason: None,
                    reply: None,
                    window: None,
                    approval: None,
                }),
            )
            .map_err(|error| error.to_string())?;
        crate::agent::off_the_runtime(|| self.writer.sync()).map_err(|error| error.to_string())
    }

    /// Refuse the ticket `ticket`, handed on as `deferred`, for `reason`:
    /// its `delegate refused` line saying why, then the request refused in
    /// the room against the ticket, its line under that one.
    async fn refuse_ticket(
        &mut self,
        deps: &AgentDeps,
        port: &Arc<dyn EditPort>,
        ticket: &str,
        deferred: &Deferred,
        said: String,
        reason: GateReason,
    ) -> Result<(), ServeError> {
        let refused = self.writer.write(
            &mut self.context,
            Some(deferred.call_line),
            None,
            LineBody::Delegate(DelegateBody {
                id: ticket.to_owned(),
                to: deferred.to.clone(),
                room: None,
                child: None,
                state: DelegateState::Refused,
                reason: Some(said),
                reply: None,
                window: None,
                approval: None,
            }),
        )?;
        let open = self
            .context
            .gate_pending
            .iter()
            .find(|(_, open)| open.ticket.as_deref() == Some(ticket))
            .and_then(|(request, open)| Some((request.clone(), open.event.clone()?)));
        if let (Some((request, event)), Some(desk)) = (open, self.desk(deps, port)) {
            let (line, sent) = desk
                .refuse(&request, Some(&event), Some(ticket.to_owned()), reason)
                .await;
            self.writer.write(
                &mut self.context,
                Some(refused.id),
                sent,
                LineBody::Gate(line),
            )?;
        }
        crate::agent::off_the_runtime(|| self.writer.sync())?;
        Ok(())
    }

    /// A gate's approval `pending` settled (codemap Q4, 99.2 #2, #3): a held
    /// result's, a journal entry's or a ticket's, each by its own path.
    pub(super) async fn gate_settled(
        &mut self,
        deps: &AgentDeps,
        port: &Arc<dyn EditPort>,
        pending: &Pending,
        settled: Settled,
    ) {
        let held = self
            .context
            .gate_held
            .iter()
            .find(|(_, approval)| **approval == pending.id)
            .map(|(request, _)| request.clone());
        if let Some(request) = held {
            Box::pin(self.held_settled(deps, port, pending, &request, settled)).await;
        } else if self.context.gate_journals.contains_key(&pending.id) {
            self.journal_settled(deps, pending, settled);
        } else {
            Box::pin(self.ticket_settled(deps, port, pending, settled)).await;
        }
    }

    /// The gate's server ms now: its desk's clock, else this host's.
    fn gate_now(&self) -> u64 {
        self.gate_desk
            .as_ref()
            .map_or_else(crate::claims::wall_ms, |desk| desk.now())
    }

    /// A ticket's result the room's sink blocked, parked on the
    /// `declassify` record `parked` names (99.2 #3): its call answered that
    /// it waits, the request's `held` line naming the record, then the
    /// record and its request in the approvers' proxy DMs — the drive's
    /// owner's, at T4 — never the gate room. Nothing is said in the room.
    /// Where nobody can be asked the result is refused `unreleased` now.
    pub(crate) async fn hold_result(
        &mut self,
        deps: &AgentDeps,
        port: &Arc<dyn EditPort>,
        serving: &crate::gate::Serving,
        parked: ParkedTurn,
    ) -> Result<(), ServeError> {
        let approval = parked.parking.approval.to_string();
        let ticket = self
            .context
            .gate_pending
            .get(&serving.request)
            .and_then(|open| open.ticket.clone());
        let mut held = crate::gate::refused_line(
            &serving.request,
            ticket,
            GateReason::Unreleased,
            self.gate_now(),
        );
        held.state = keeper_core::agents::log::GateState::Held;
        held.reason = None;
        held.approval = Some(approval.clone());
        self.writer.write(
            &mut self.context,
            Some(parked.call_line),
            None,
            LineBody::Gate(held),
        )?;
        crate::agent::off_the_runtime(|| self.writer.sync())?;
        let asked = self.park(deps, port, &parked).await;
        // The call's result says what became of it; the round's later calls
        // never run: the turn ended at its result.
        let first = match &asked {
            Ok(()) => ToolOutcome::Parked {
                approval: parked.parking.approval,
            },
            Err(reason) => ToolOutcome::Refused {
                reason: if reason == NOBODY_TO_ASK {
                    NOBODY_TO_ASK.to_owned()
                } else {
                    format!("This could not wait for a person: {reason}")
                },
            },
        };
        let mut answers = vec![(parked.call_line, parked.parking.call_id.clone(), first)];
        for (line_id, wire) in &parked.rest {
            answers.push((
                *line_id,
                wire.id.clone(),
                ToolOutcome::Refused {
                    reason: NOT_RUN.to_owned(),
                },
            ));
        }
        let label = self.context.label.clone();
        for (line_id, call_id, outcome) in answers {
            let (word, truncated) = crate::agent::result_word(&outcome);
            self.writer.write(
                &mut self.context,
                Some(line_id),
                None,
                LineBody::ToolResult(keeper_core::agents::log::ToolResultBody {
                    call_id,
                    outcome: word,
                    content: render_result(&outcome),
                    truncated,
                    label: label.clone(),
                    paseo: None,
                    effect: None,
                }),
            )?;
        }
        crate::agent::off_the_runtime(|| self.writer.sync())?;
        if let Err(reason) = asked {
            tracing::warn!(request = %serving.request, %reason, "agents: a gate's held result could not be asked for; it is refused");
            self.due.remove(&approval);
            self.close_row(deps, &approval, AuditOutcome::Refused);
            self.refuse_held(
                port,
                deps,
                &serving.request,
                parked.call_line,
                GateReason::Unreleased,
            )
            .await?;
        }
        Ok(())
    }

    /// Refuse the held request `request` for `reason` in the room, against
    /// its ticket, its line under the call's `call_line`. Without its
    /// request's event or the gate's desk the line is still written — the
    /// request closed, nothing said in the room.
    async fn refuse_held(
        &mut self,
        port: &Arc<dyn EditPort>,
        deps: &AgentDeps,
        request: &str,
        call_line: Ulid,
        reason: GateReason,
    ) -> Result<(), ServeError> {
        if let Some(open) = self.context.gate_pending.get(request).cloned() {
            let (line, sent) = match (open.event.as_deref(), self.desk(deps, port)) {
                (Some(event), Some(desk)) => {
                    desk.refuse(request, Some(event), open.ticket, reason).await
                }
                (event, desk) => {
                    tracing::warn!(%request, event = event.is_some(), desk = desk.is_some(), "agents: a held gate result is refused without a word in its room: its request's event or the gate's desk is gone");
                    match desk {
                        Some(desk) => desk.refuse(request, None, open.ticket, reason).await,
                        None => (
                            crate::gate::refused_line(
                                request,
                                open.ticket,
                                reason,
                                self.gate_now(),
                            ),
                            None,
                        ),
                    }
                }
            };
            self.writer.write(
                &mut self.context,
                Some(call_line),
                sent,
                LineBody::Gate(line),
            )?;
        }
        crate::agent::off_the_runtime(|| self.writer.sync())?;
        Ok(())
    }

    /// A held result's declassification settled (99.2 #3). Consumed, the
    /// result goes as the ticket's one `result` — exactly the bytes the
    /// record names, and only while the room as it is now is within the
    /// label it was held under widened by the readers the person let read
    /// it (R193, R194); else it is refused `unreleased`. Denied, it is
    /// refused `withheld`; expired or refused, `unreleased`. The call's one
    /// row closes either way.
    async fn held_settled(
        &mut self,
        deps: &AgentDeps,
        port: &Arc<dyn EditPort>,
        pending: &Pending,
        request: &str,
        settled: Settled,
    ) {
        let reason = match settled {
            Settled::Run(call, Some(released), _) => {
                match self
                    .release(deps, port, pending, request, &call, &released)
                    .await
                {
                    Ok(()) => {
                        self.close_row(deps, &pending.id, AuditOutcome::Ok);
                        return;
                    }
                    Err(why) => {
                        tracing::warn!(%request, %why, "agents: a released gate result was not sent");
                        GateReason::Unreleased
                    }
                }
            }
            Settled::Run(..) => GateReason::Unreleased,
            Settled::Refuse(said) if said == DENIED => GateReason::Withheld,
            Settled::Refuse(_) => GateReason::Unreleased,
        };
        self.close_row(deps, &pending.id, AuditOutcome::Refused);
        if let Err(error) = self
            .refuse_held(port, deps, request, pending.call_line, reason)
            .await
        {
            tracing::error!(%request, %error, "agents: a held gate result could not be refused");
        }
    }

    /// Send the held result of `request`, `call` as its turn made it, on
    /// its consumed declassification `released`: its redacted text's bytes
    /// as the record names them, into the room only while the room is
    /// within the approved label; its lines under the call's.
    async fn release(
        &mut self,
        deps: &AgentDeps,
        port: &Arc<dyn EditPort>,
        pending: &Pending,
        request: &str,
        call: &chat::ToolCall,
        released: &Released,
    ) -> Result<(), String> {
        let text = call
            .arguments
            .as_ref()
            .and_then(|args| args["text"].as_str())
            .ok_or_else(|| CHANGED.to_owned())?;
        let text = keeper_core::agents::redact::redact_secrets(text).text;
        if approval::sha256_hex(text.as_bytes()) != released.sha256 {
            return Err(CHANGED.to_owned());
        }
        let (record, _) = self.read_record(deps, &pending.id)?;
        let approved = keeper_core::agents::label::approved_label(&record.label, &released.readers);
        let members = port.members().await.map_err(|error| error.to_string())?;
        let known = self.delegations.as_ref().map(|rooms| rooms.known());
        let sink = crate::sinks::room_audience(
            members,
            known.as_deref(),
            std::slice::from_ref(&deps.home.config.matrix_user),
        );
        if check_sink(&approved, &sink) != SinkVerdict::Allow {
            return Err(crate::agent::AUDIENCE_MOVED.to_owned());
        }
        let (event, ticket) = self
            .context
            .gate_pending
            .get(request)
            .and_then(|open| Some((open.event.clone()?, open.ticket.clone()?)))
            .ok_or_else(|| "its request is closed".to_owned())?;
        let desk = self
            .desk(deps, port)
            .ok_or_else(|| "this session has no desk".to_owned())?;
        desk.call_back(request, event, ticket);
        let lines = desk.reply(request, &text).await?;
        for fact in lines {
            self.writer
                .write(
                    &mut self.context,
                    Some(pending.call_line),
                    fact.event,
                    LineBody::Gate(fact.line),
                )
                .map_err(|error| error.to_string())?;
        }
        crate::agent::off_the_runtime(|| self.writer.sync()).map_err(|error| error.to_string())
    }

    /// A gate's journal entry's approval settled (99.2 #2): consumed, the
    /// host writes the entry — quoted as data — under the label it was
    /// asked under, into the home its readers read, and a `session`
    /// decision lets this session's later entries go for a day; denied,
    /// expired or refused, nothing is written. The call's one row closes
    /// either way.
    fn journal_settled(&mut self, deps: &AgentDeps, pending: &Pending, settled: Settled) {
        let Some(ask) = self.context.gate_journals.remove(&pending.id) else {
            return;
        };
        let Settled::Run(call, ..) = settled else {
            self.close_row(deps, &pending.id, AuditOutcome::Refused);
            return;
        };
        let label = match self.read_record(deps, &pending.id) {
            Ok((record, _)) => record.label,
            Err(reason) => {
                tracing::warn!(approval = %pending.id, %reason, "agents: a gate's journal entry's record does not read; nothing is written");
                self.close_row(deps, &pending.id, AuditOutcome::Refused);
                return;
            }
        };
        let home = Readers::Only(deps.home.drive.readers.clone());
        if check_sink(&label, &Sink::MemoryWrite { home_readers: home }) != SinkVerdict::Allow {
            self.close_row(deps, &pending.id, AuditOutcome::Refused);
            return;
        }
        let memory = crate::memory::MemoryTools::new(
            crate::memory::MemoryHome {
                agent: deps.home.config.id.clone(),
                dir: deps.home.dir.clone(),
                dir_rel: crate::agent::drive_relative(&deps.drive_root, &deps.home.dir),
                zone: deps.home.zone.clone(),
                host: deps.host.clone(),
            },
            crate::memory::MemorySession {
                dir: format!("{}/{}", deps.sessions_subfolder, self.context.session.path),
                slug: self
                    .context
                    .session
                    .path
                    .rsplit('/')
                    .next()
                    .unwrap_or_default()
                    .to_owned(),
                kind: SessionKind::Gate,
                parent_kind: None,
                review: false,
            },
        );
        let lease = self.writer.lease();
        let may_write = move || lease.as_ref().is_none_or(|lease| lease.may_write());
        let outcome = memory.run(&call, &label, &may_write);
        for line in memory.take_lines() {
            if let Err(error) =
                self.writer
                    .write(&mut self.context, Some(ask.call_line), None, line)
            {
                tracing::error!(approval = %pending.id, %error, "agents: a gate's journal line could not be written");
            }
        }
        if let Err(error) = crate::agent::off_the_runtime(|| self.writer.sync()) {
            tracing::error!(%error, "agents: the session's log could not be synced");
        }
        self.close_row(deps, &pending.id, crate::sinks::audit_outcome(&outcome));
        let session_scope = self
            .read_decision(deps, &pending.id)
            .is_some_and(|decision| decision.scope == approval::Scope::Session);
        if session_scope && matches!(outcome, ToolOutcome::Answered { .. }) {
            self.context.gate_journal = Some(keeper_core::agents::gate::JournalAllowance::granted(
                &pending.id,
                self.gate_now(),
            ));
        }
    }
}
