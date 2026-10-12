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
use crate::agent::Deferred;

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
        fn failed(error: impl std::fmt::Display) -> String {
            error.to_string()
        }
        let args: Value = serde_json::from_str(&deferred.args).map_err(failed)?;
        let request = self
            .context
            .gate_pending
            .iter()
            .find(|(_, open)| open.ticket.as_deref() == Some(ticket))
            .map(|(request, _)| request.clone())
            .ok_or_else(|| "its request is closed".to_owned())?;
        let agent = self.context.agent.clone();
        let facts = CallFacts {
            gate_send: true,
            ..tier_rules::named_facts(AgentTool::Delegate, &args, CallFacts::default())
        };
        let classification = tier_rules::classify(
            AgentTool::Delegate,
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
            id: &deferred.approval,
            created_at: Utc::now(),
            session: &session,
            session_kind: agent.kind,
            agent: &deps.home.config.id,
            drive: &deps.home.config.drive,
            host: deps.host.as_str(),
            epoch: self.context.epoch,
            call: CallRef {
                line: deferred.call_line.to_string(),
                call_id: deferred.call_id.clone(),
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
            args: &args,
            exec_binding: serde_json::json!({ "ticket": ticket, "request": request }),
            classification: &classification,
            label: &agent.label,
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
                .refuse(&request, &event, Some(ticket.to_owned()), reason)
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
}
