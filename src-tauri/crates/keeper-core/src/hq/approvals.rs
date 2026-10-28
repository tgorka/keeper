//! Approvals as rows (AD-428): what an agent asks a person to allow, read
//! from a session's `approvals` and `tool_calls` rows under the reader-scoped
//! `trace` key, and a decision written as a row. The server's V8 decides who
//! may approve and refuses a tier-4 decision from the claim holder's host; a
//! refusal reaches the person as the server's sentence.
//!
//! An approval exists here only for a member whose membership reads the
//! trace now. The server stops sending them when the flag is withdrawn or
//! the member removed, but rows already here stay until the window evicts
//! them, and a decision still in the outbox pins its row; so every view,
//! and a decision, ask the membership first (D-34, epic invariant 5).
//!
//! An approval is decided only on the exact action it would run: a request
//! whose tool call is not on this device cannot be approved here, and a
//! decision names the binding digest it was shown, so a request that changed
//! since is refused rather than approved blind.
//!
//! A card offers its actions in one menu (109.11, Q109-09 b): *Allow once*,
//! *Always allow …* and *This session only* where [`crate::hq::grants`]
//! offers a standing grant, and *Deny*. Sending a message in a session
//! declines every request there that waits for this person (Q109-12 a,
//! [`decline_pending`]) — the safe direction, and the card says so.

use std::collections::HashMap;
use std::time::Duration;

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use ts_rs::TS;

use crate::hq::grants::{self, AgentGrantOfferVm, SessionFacts};
use crate::hq::port::{Delivery, LinkState, Rejection, RowOp};
use crate::hq::sessions::{
    authored, int, names, object, opt_text, read, reads_trace, text, Source,
};

pub mod write;

/// How long a decision waits for the server before it is reported as
/// waiting on this device.
pub const DECISION_WAIT: Duration = Duration::from_secs(5);

/// An approval's state as the person reads it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub enum AgentApprovalStateVm {
    Pending,
    Approved,
    Denied,
    /// Approved, and the agent has run it.
    Consumed,
    /// Nobody decided before it expired.
    Expired,
}

/// One action of an approval card's menu, in the menu's order.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub enum AgentApprovalActionVm {
    /// Approve this request, this once.
    AllowOnce,
    /// Approve it and write a standing grant for every session the person
    /// shares with the agent in this database (`grant` says what it covers).
    AlwaysAllow,
    /// Approve it and write a grant bound to this session alone.
    SessionOnly,
    Deny,
}

/// One approval of a session.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct AgentApprovalVm {
    pub id: String,
    pub database: String,
    pub session_id: String,
    /// The tool the agent would call; empty until its row is here.
    pub tool_name: String,
    pub tier: String,
    pub summary: String,
    /// The digest the decision is bound to, shown beside the action.
    pub binding_digest: String,
    /// The exact call, its arguments as JSON; absent when the tool call is
    /// not on this device, and then it cannot be approved here.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub action: Option<String>,
    #[ts(type = "number")]
    pub expires_at_ms: i64,
    #[ts(type = "number")]
    pub created_at_ms: i64,
    pub state: AgentApprovalStateVm,
    /// Who decided, by name.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub decided_by: Option<String>,
    /// Whether this person may decide it here, now.
    pub can_decide: bool,
    /// The server refused this person's decision on it (V8, a tier-4 host,
    /// a decision racing another device's): its sentence. The request is
    /// pending again.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub refused: Option<String>,
    /// Rust's line for it.
    pub sentence: String,
    /// What this person may do with it here, in the card menu's order;
    /// empty where they decide nothing.
    pub actions: Vec<AgentApprovalActionVm>,
    /// What *Always allow …* and *This session only* would grant. Absent
    /// wherever neither is offered: at T4, in an untrusted session, for
    /// `delegate`, a drive tool or any tool off the allow-list.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub grant: Option<AgentGrantOfferVm>,
    /// It waits for this person, so a message they send in its session
    /// declines it (Q109-12 a): what the banner above the composer holds.
    pub declined_by_send: bool,
    /// It waits for this person, but at T4 this device cannot decide it
    /// (V8 `hq.v8.t4_other_host`, [`undecidable_here`]): the card offers no
    /// decision and says why, and a message leaves it waiting (DW-E109-27,
    /// R109T-02). The banner counts these.
    #[serde(skip)]
    #[ts(skip)]
    pub undecidable: Option<Undecidable>,
    /// The turn of the parked call it would run (`tool_calls.turn_id`):
    /// where the reading column draws it, in that turn's fold. Absent when
    /// the call is not on this device.
    #[serde(skip)]
    #[ts(skip)]
    pub turn_id: Option<String>,
    /// Who it waits for, while it is pending: what its turn's fold line
    /// names.
    #[serde(skip)]
    #[ts(skip)]
    pub waiting_for: Option<WaitingFor>,
    /// A `drive_write` request's write (109.27): what it changes, the
    /// preview from this device's checkout, and what became of it
    /// ([`write::Writes::draw`]).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub write: Option<write::AgentDriveWriteVm>,
}

/// Who a pending request waits for, as its turn's fold line names them.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WaitingFor {
    /// This person is one of its approvers.
    You,
    /// Only others are: their names, joined.
    Others(String),
}

impl WaitingFor {
    /// The fold line's words for it.
    pub fn label(&self) -> String {
        match self {
            WaitingFor::You => "Waiting for you".to_owned(),
            WaitingFor::Others(names) => format!("Waiting for {names}"),
        }
    }
}

/// Why a T4 request waiting for this person cannot be decided on this
/// device: what V8's `hq.v8.t4_other_host` refuses, as this replica reads it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Undecidable {
    /// This device is on the host the agent runs on, or its own host is not
    /// recorded: another of the person's devices may decide it.
    AnotherDevice,
    /// The host of the instance holding the session's claim is not known
    /// (or nothing holds it): no device can decide it until it is
    /// (R109T-04).
    HostUnknown,
}

impl Undecidable {
    /// The card's sentence for it.
    pub fn sentence(self) -> &'static str {
        match self {
            Undecidable::AnotherDevice => "Needs a decision from another device.",
            Undecidable::HostUnknown => "Can't be decided until the agent's host is known.",
        }
    }
}

/// How a decision went.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub enum AgentDecisionStateVm {
    /// The server took it.
    Sent,
    /// Written here; sent when the database is reached.
    Waiting,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct AgentDecisionOutcomeVm {
    pub state: AgentDecisionStateVm,
    pub sentence: String,
}

const SELECT: &str = "SELECT a.id AS id, a.session_id AS session_id, a.tier AS tier, \
     a.binding_digest AS binding_digest, a.summary AS summary, a.approvers AS approvers, \
     a.state AS state, a.decided_by_actor AS decided_by_actor, a.decided_by_client AS decided_by_client, a.expires_at_ms AS expires_at_ms, \
     a.created_at_ms AS created_at_ms, a.grant_id AS grant_id, a._sync_version AS version, \
     t.id AS call_id, t.name AS tool_name, t.args AS args, t.turn_id AS turn_id \
     FROM approvals a LEFT JOIN tool_calls t ON t.id = a.tool_call_id";

fn state_of(row: &Map<String, Value>, now_ms: i64) -> AgentApprovalStateVm {
    match text(row, "state") {
        "approved" => AgentApprovalStateVm::Approved,
        "denied" => AgentApprovalStateVm::Denied,
        "consumed" => AgentApprovalStateVm::Consumed,
        "expired" => AgentApprovalStateVm::Expired,
        _ if int(row, "expires_at_ms").is_some_and(|at| at <= now_ms) => {
            AgentApprovalStateVm::Expired
        }
        _ => AgentApprovalStateVm::Pending,
    }
}

/// The exact call an approval runs, as the card shows it.
fn action(row: &Map<String, Value>) -> Option<String> {
    opt_text(row, "call_id")?;
    let args = match row.get("args") {
        Some(Value::String(raw)) => serde_json::from_str(raw).unwrap_or(Value::String(raw.clone())),
        Some(other) => other.clone(),
        None => Value::Null,
    };
    serde_json::to_string_pretty(&args).ok()
}

/// The server's refusals of decisions written here, by database and
/// approval id.
pub type Refusals = HashMap<(String, String), String>;

/// A client's host as the directory records it; `None` when it has none.
fn host_of(source: &Source, client: &str) -> Option<String> {
    read(
        source,
        "SELECT host FROM clients WHERE id = ?1",
        &[Value::from(client)],
    )
    .first()
    .and_then(|row| opt_text(row, "host"))
    .filter(|host| !host.is_empty())
}

/// V8's tier-4 rule as this device can read it (`hq.v8.t4_other_host`): a
/// T4 decision — an approval or a denial — is taken only from a device on
/// another host than the claim holder's, and refused when either host is
/// unknown. `None` where this device may decide, and for any other tier.
/// The one predicate the card and the banner share.
fn undecidable_here(source: &Source, session_id: &str, tier: &str) -> Option<Undecidable> {
    if tier != "T4" {
        return None;
    }
    let holder = read(
        source,
        "SELECT holder_client FROM claims WHERE session_id = ?1",
        &[Value::from(session_id)],
    )
    .first()
    .and_then(|row| opt_text(row, "holder_client"))
    .and_then(|client| host_of(source, &client));
    match (host_of(source, &source.client), holder) {
        (_, None) => Some(Undecidable::HostUnknown),
        (Some(mine), Some(holder)) if mine != holder => None,
        _ => Some(Undecidable::AnotherDevice),
    }
}

fn vm(
    source: &Source,
    row: &Map<String, Value>,
    session: Option<&SessionFacts>,
    directory: &HashMap<String, String>,
    refusals: &Refusals,
    now_ms: i64,
) -> Option<AgentApprovalVm> {
    let id = opt_text(row, "id")?;
    let state = state_of(row, now_ms);
    let action = action(row);
    let approvers = names(row, "approvers");
    let mine = approvers.contains(&source.actor);
    // An actor the directory has not supplied yet has no name here: its raw
    // id is never shown as one. Directory rows come by the `dir`
    // subscription and requests by trace windows, so a request can arrive
    // first; the name follows once the row does (R604-01).
    let name = |actor: &str| directory.get(actor).cloned();
    // A refused decision of this device's is not a decision: the request
    // reads pending again, however long its optimistic row lingers here.
    let mine_decided = text(row, "decided_by_client") == source.client;
    let refused = (state == AgentApprovalStateVm::Pending || mine_decided)
        .then(|| {
            refusals
                .get(&(source.database.clone(), id.clone()))
                .cloned()
        })
        .flatten();
    let state = if refused.is_some() && state != AgentApprovalStateVm::Expired {
        AgentApprovalStateVm::Pending
    } else {
        state
    };
    let decided_by = opt_text(row, "decided_by_actor")
        .filter(|_| refused.is_none())
        .and_then(|actor| name(&actor));
    let tier = text(row, "tier").to_owned();
    // A T4 request this device cannot decide offers no decision here: the
    // server would refuse either (R109T-02).
    let undecidable = (state == AgentApprovalStateVm::Pending && mine)
        .then(|| undecidable_here(source, text(row, "session_id"), &tier))
        .flatten();
    let can_decide =
        state == AgentApprovalStateVm::Pending && mine && action.is_some() && undecidable.is_none();
    // A grant's own decision names the grant's person (V8 as amended): the
    // card says it came from their standing grant, not a click.
    let by = |verb: &str| {
        let who = decided_by.as_deref().unwrap_or("someone");
        match opt_text(row, "grant_id") {
            Some(_) => format!("{verb} by {who}'s standing grant"),
            None => format!("{verb} by {who}"),
        }
    };
    // The approvers' names, those the directory has; "someone else" stands
    // for every one it has not supplied yet.
    let others = || {
        let mut known: Vec<String> = approvers.iter().filter_map(|a| name(a)).collect();
        if known.is_empty() {
            return "someone else".to_owned();
        }
        if known.len() < approvers.len() {
            known.push("someone else".to_owned());
        }
        known.join(", ")
    };
    let waiting_for = (state == AgentApprovalStateVm::Pending).then(|| {
        if mine {
            WaitingFor::You
        } else {
            WaitingFor::Others(others())
        }
    });
    let sentence = match state {
        AgentApprovalStateVm::Pending if refused.is_some() => format!(
            "The server refused your decision: {}",
            refused.as_deref().unwrap_or_default()
        ),
        AgentApprovalStateVm::Pending if !mine => format!("Waiting for {}.", others()),
        AgentApprovalStateVm::Pending if undecidable.is_some() => undecidable
            .map(Undecidable::sentence)
            .unwrap_or_default()
            .to_owned(),
        AgentApprovalStateVm::Pending if action.is_none() => {
            "keeper does not hold this request's action yet, so it cannot be approved here."
                .to_owned()
        }
        AgentApprovalStateVm::Pending => "Waiting for you.".to_owned(),
        AgentApprovalStateVm::Approved => format!("{}.", by("Approved")),
        AgentApprovalStateVm::Denied => format!("{}.", by("Denied")),
        AgentApprovalStateVm::Consumed => format!("{}; the agent has run it.", by("Approved")),
        AgentApprovalStateVm::Expired => "Expired before anyone decided.".to_owned(),
    };
    let tool_name = text(row, "tool_name").to_owned();
    let grant = match session {
        Some(session) if can_decide => grants::offer(
            &tier,
            &tool_name,
            &crate::hq::sessions::json(row, "args"),
            session,
            name(&session.agent_actor).as_deref(),
            &source.database,
        ),
        _ => None,
    };
    let mut actions = Vec::new();
    if can_decide {
        actions.push(AgentApprovalActionVm::AllowOnce);
        if grant.is_some() {
            actions.push(AgentApprovalActionVm::AlwaysAllow);
            actions.push(AgentApprovalActionVm::SessionOnly);
        }
        actions.push(AgentApprovalActionVm::Deny);
    }
    let waits_for_me = state == AgentApprovalStateVm::Pending
        && mine
        && session.is_some_and(|session| !session.archived);
    // A denial from here would be refused like an approval (V8 checks the
    // host of any T4 decision), so a message must not claim to decline it.
    let undecidable = undecidable.filter(|_| waits_for_me);
    let declined_by_send = waits_for_me && undecidable.is_none();
    Some(AgentApprovalVm {
        id,
        database: source.database.clone(),
        session_id: text(row, "session_id").to_owned(),
        tool_name,
        tier,
        summary: text(row, "summary").to_owned(),
        binding_digest: text(row, "binding_digest").to_owned(),
        action,
        expires_at_ms: int(row, "expires_at_ms").unwrap_or(0),
        created_at_ms: int(row, "created_at_ms").unwrap_or(0),
        state,
        decided_by,
        can_decide,
        refused,
        sentence,
        actions,
        grant,
        declined_by_send,
        undecidable,
        turn_id: opt_text(row, "turn_id"),
        waiting_for,
        write: None,
    })
}

/// A session's approvals, oldest first. Empty for a member who does not
/// read the trace now, whatever rows this device still holds.
pub fn of_session(
    source: &Source,
    session_id: &str,
    directory: &HashMap<String, String>,
    refusals: &Refusals,
    now_ms: i64,
) -> Vec<AgentApprovalVm> {
    if !reads_trace(source, session_id) {
        return Vec::new();
    }
    let session = grants::session_facts(source, session_id);
    read(
        source,
        &format!("{SELECT} WHERE a.session_id = ?1 ORDER BY a.created_at_ms, a.id"),
        &[Value::from(session_id)],
    )
    .iter()
    .filter_map(|row| vm(source, row, session.as_ref(), directory, refusals, now_ms))
    .collect()
}

/// Every approval this person may decide now, in every held database,
/// oldest first: the decisions panel. Only of sessions whose trace they
/// read now.
pub fn waiting(sources: &[Source], refusals: &Refusals, now_ms: i64) -> Vec<AgentApprovalVm> {
    let mut all: Vec<AgentApprovalVm> = Vec::new();
    for source in sources {
        let directory = crate::hq::sessions::directory(source);
        let reading: std::collections::HashSet<String> = crate::hq::sessions::held(source)
            .into_iter()
            .filter(|membership| membership.reads_trace)
            .map(|membership| membership.id)
            .collect();
        let mut facts: HashMap<String, Option<SessionFacts>> = HashMap::new();
        all.extend(
            read(
                source,
                &format!("{SELECT} WHERE a.state = 'pending' ORDER BY a.created_at_ms, a.id"),
                &[],
            )
            .iter()
            .filter(|row| reading.contains(text(row, "session_id")))
            .filter_map(|row| {
                let id = text(row, "session_id");
                let session = facts
                    .entry(id.to_owned())
                    .or_insert_with(|| grants::session_facts(source, id));
                vm(source, row, session.as_ref(), &directory, refusals, now_ms)
            })
            .filter(|approval| approval.can_decide),
        );
    }
    all.sort_by(|a, b| {
        a.created_at_ms
            .cmp(&b.created_at_ms)
            .then_with(|| a.database.cmp(&b.database))
            .then_with(|| a.id.cmp(&b.id))
    });
    all
}

/// When the soonest pending approval in any held database expires, after
/// `now_ms`: the moment every view of it changes with no row changing.
pub fn next_expiry(sources: &[Source], now_ms: i64) -> Option<i64> {
    sources
        .iter()
        .flat_map(|source| {
            read(
                source,
                "SELECT min(expires_at_ms) AS at FROM approvals \
                 WHERE state = 'pending' AND expires_at_ms > ?1",
                &[Value::from(now_ms)],
            )
        })
        .filter_map(|row| int(&row, "at"))
        .min()
}

/// A decision written here whose outcome the server has not told yet.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct Owed {
    database: String,
    approval_id: String,
}

/// The decisions written here, by commit id, until the server has answered
/// them; a refused one is kept, and shown, until the request is decided
/// again or leaves `pending`. Kept in a file beside the replicas, so an
/// answer that arrives after a relaunch still reaches the person.
pub struct Decisions {
    path: std::path::PathBuf,
    owed: std::sync::Mutex<HashMap<String, Owed>>,
}

impl Decisions {
    pub fn load(path: std::path::PathBuf) -> Decisions {
        let owed = std::fs::read_to_string(&path)
            .ok()
            .and_then(|raw| serde_json::from_str(&raw).ok())
            .unwrap_or_default();
        Decisions {
            path,
            owed: std::sync::Mutex::new(owed),
        }
    }

    fn save(&self, owed: &HashMap<String, Owed>) {
        let written = serde_json::to_string(owed)
            .map_err(std::io::Error::other)
            .and_then(|raw| std::fs::write(&self.path, raw));
        if let Err(error) = written {
            tracing::warn!(%error, "the agents decisions waiting for the server could not be kept");
        }
    }

    /// Remember `commit` as this device's decision on `approval_id`,
    /// replacing an earlier one.
    pub fn record(&self, commit: &str, database: &str, approval_id: &str) {
        let mut owed = self
            .owed
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        owed.retain(|_, o| !(o.database == database && o.approval_id == approval_id));
        owed.insert(
            commit.to_owned(),
            Owed {
                database: database.to_owned(),
                approval_id: approval_id.to_owned(),
            },
        );
        self.save(&owed);
    }

    /// The refusals the server has sent so far; decisions it took, or that
    /// its replica no longer knows, or whose request left `pending`, are
    /// forgotten here.
    pub fn refusals(&self, sources: &[Source]) -> Refusals {
        let mut owed = self
            .owed
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let mut refusals = Refusals::new();
        let before = owed.len();
        owed.retain(|commit, o| {
            let Some(source) = sources.iter().find(|s| s.database == o.database) else {
                // A database not held now: kept until it is.
                return true;
            };
            match source.replica.delivery(commit) {
                Ok(Delivery::Pending) | Err(_) => true,
                Ok(Delivery::Applied | Delivery::Unknown) => false,
                Ok(Delivery::Refused(rejection)) => {
                    let still_pending = !read(
                        source,
                        "SELECT 1 FROM approvals WHERE id = ?1 \
                         AND (state = 'pending' OR decided_by_client = ?2)",
                        &[
                            Value::from(o.approval_id.as_str()),
                            Value::from(source.client.as_str()),
                        ],
                    )
                    .is_empty();
                    if still_pending {
                        refusals.insert(
                            (o.database.clone(), o.approval_id.clone()),
                            refused_because(&rejection),
                        );
                    }
                    still_pending
                }
            }
        });
        if owed.len() != before {
            self.save(&owed);
        }
        refusals
    }
}

/// The patch a decision writes (V8): the state, and this person and this
/// device as the deciders, on the version that was read — a decision racing
/// another device's conflicts rather than overwriting it.
pub fn decision_op(
    source: &Source,
    approval_id: &str,
    approve: bool,
    version: Option<i64>,
) -> RowOp {
    RowOp::Patch {
        table: "approvals".to_owned(),
        values: authored(
            source,
            object(serde_json::json!({
                "id": approval_id,
                "state": if approve { "approved" } else { "denied" },
                "decided_by_actor": source.actor,
                "decided_by_client": source.client,
            })),
        ),
        // An optimistic row (version below one) has no server version yet.
        base_version: version.filter(|v| *v > 0),
    }
}

/// Why a decision was not written, as the person reads it.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("{0}")]
pub struct Refusal(pub String);

/// Why the server refused a decision, in keeper's words where its code is
/// one keeper knows (matched exactly); any other as the server said it. It
/// follows "The server refused your decision: ".
pub fn refused_because(rejection: &Rejection) -> String {
    match rejection.code.as_str() {
        "hq.v8.not_reader" => {
            "you no longer read this session's thinking and tool calls, so its requests \
             are not yours to decide."
                .to_owned()
        }
        // *Always allow …* writes its grant in the decision's commit, so the
        // grant's refusal is the decision's.
        code if code.starts_with("hq.v15.") => grants::refused_because(rejection),
        _ if rejection.message.is_empty() => rejection.code.clone(),
        _ => rejection.message.clone(),
    }
}

/// keeper's own refusal of a decision by a person who does not read the
/// session's trace now, before anything is written.
pub const NOT_A_READER: &str = "You no longer read this session's thinking and tool calls, \
     so its requests are not yours to decide.";

/// Write this person's decision on `approval_id`, bound to the digest they
/// were shown, then wait up to [`DECISION_WAIT`] for the server: taken, or
/// waiting on this device, or refused with the server's sentence. The
/// commit is kept in `decisions`, so an answer that comes later is shown on
/// the request. Blocking.
pub fn decide(
    source: &Source,
    decisions: &Decisions,
    session_id: &str,
    approval_id: &str,
    binding_digest: &str,
    approve: bool,
    now_ms: i64,
) -> Result<AgentDecisionOutcomeVm, Refusal> {
    let row = decidable(
        source,
        decisions,
        session_id,
        approval_id,
        binding_digest,
        approve,
        now_ms,
    )?;
    // Approving a drive write is carrying it out ([`write::Writes::allow`]);
    // a bare approval here would leave it approved and never written.
    if approve && text(&row, "tool_name") == crate::agents::drive_write::TOOL {
        return Err(Refusal(write::ON_ITS_CARD.to_owned()));
    }
    let op = decision_op(source, approval_id, approve, int(&row, "version"));
    settle(
        source,
        decisions,
        vec![op],
        approval_id,
        Vec::new(),
        if approve { "Approved." } else { "Denied." },
    )
}

/// The request `approval_id` of `session_id` as this person may decide it
/// now — on the digest they were shown, while it waits for them, and to
/// approve only with its exact call here — or keeper's refusal, before
/// anything is written.
pub(crate) fn decidable(
    source: &Source,
    decisions: &Decisions,
    session_id: &str,
    approval_id: &str,
    binding_digest: &str,
    approve: bool,
    now_ms: i64,
) -> Result<Map<String, Value>, Refusal> {
    // A member who does not read the trace holds no request to decide, even
    // where its row is still here (withdrawn, removed, or pinned by the
    // outbox).
    if !reads_trace(source, session_id) {
        return Err(Refusal(NOT_A_READER.to_owned()));
    }
    let rows = read(
        source,
        &format!("{SELECT} WHERE a.id = ?1 AND a.session_id = ?2"),
        &[Value::from(approval_id), Value::from(session_id)],
    );
    let Some(row) = rows.into_iter().next() else {
        return Err(Refusal(
            "This request is not on this device any more.".to_owned(),
        ));
    };
    if text(&row, "binding_digest") != binding_digest {
        return Err(Refusal(
            "This request changed since it was shown; look at it again before deciding.".to_owned(),
        ));
    }
    let refused_here = decisions
        .refusals(std::slice::from_ref(source))
        .contains_key(&(source.database.clone(), approval_id.to_owned()));
    match state_of(&row, now_ms) {
        AgentApprovalStateVm::Pending => {}
        AgentApprovalStateVm::Approved | AgentApprovalStateVm::Denied if refused_here => {}
        AgentApprovalStateVm::Expired => {
            return Err(Refusal(
                "This request expired before it was decided.".to_owned(),
            ))
        }
        _ => return Err(Refusal("This request was already decided.".to_owned())),
    }
    if !names(&row, "approvers").contains(&source.actor) {
        return Err(Refusal("This request is not yours to decide.".to_owned()));
    }
    if approve && action(&row).is_none() {
        return Err(Refusal(
            "keeper does not hold this request's action yet, so it cannot be approved here."
                .to_owned(),
        ));
    }
    Ok(row)
}

/// Commit `ops` — a decision on `approval_id`, and whatever goes in its
/// commit — record it in `decisions`, set the standing refusals `supersede`
/// names aside, and wait up to [`DECISION_WAIT`] for the server: `sent`
/// once it took it, waiting on this device, or refused with its sentence.
pub(crate) fn settle(
    source: &Source,
    decisions: &Decisions,
    ops: Vec<RowOp>,
    approval_id: &str,
    supersede: Vec<String>,
    sent: &str,
) -> Result<AgentDecisionOutcomeVm, Refusal> {
    let commit = source
        .replica
        .mutate_superseding(ops, supersede)
        .map_err(|error| Refusal(error.to_string()))?;
    decisions.record(&commit, &source.database, approval_id);
    source.replica.sync_now();
    // Only a connected replica can hear back within the wait.
    let deadline = if source.replica.state().link == LinkState::Connected {
        std::time::Instant::now() + DECISION_WAIT
    } else {
        std::time::Instant::now()
    };
    loop {
        match source.replica.delivery(&commit) {
            Ok(Delivery::Applied) => {
                return Ok(AgentDecisionOutcomeVm {
                    state: AgentDecisionStateVm::Sent,
                    sentence: sent.to_owned(),
                })
            }
            Ok(Delivery::Refused(rejection)) => {
                return Err(Refusal(format!(
                    "The server refused your decision: {}",
                    refused_because(&rejection)
                )))
            }
            Ok(Delivery::Pending | Delivery::Unknown) | Err(_) => {}
        }
        if std::time::Instant::now() >= deadline {
            return Ok(AgentDecisionOutcomeVm {
                state: AgentDecisionStateVm::Waiting,
                sentence: format!(
                    "Decided here; it reaches {} when this device does.",
                    source.database
                ),
            });
        }
        std::thread::sleep(Duration::from_millis(25));
    }
}

/// One request as the banner showed it: what a message sent from under that
/// banner declines (Q109-12 a, R10911-01).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct ShownApprovalVm {
    pub id: String,
    pub binding_digest: String,
}

/// Decline the requests of `session_id` the banner showed — `shown`, each
/// at the digest it was shown with — that still wait for this person, as a
/// message sent from under that banner does (Q109-12 a). A request that
/// arrived after the banner was drawn, or changed since, is left alone: the
/// person was never told the message declines it. Each denial is the card's
/// own decision ([`decision_op`]) in its own commit, so a refusal of one (a
/// tier-4 host, a race) neither refuses the others nor the message, and is
/// recorded in `decisions`, so a refusal reaches its card. Written before
/// the message, never waited for. The ids declined; or, when one could not
/// be written on this device, the sentence saying which still waits — the
/// message must then not be sent (R10911-06).
pub fn decline_pending(
    source: &Source,
    decisions: &Decisions,
    session_id: &str,
    shown: &[ShownApprovalVm],
    now_ms: i64,
) -> Result<Vec<String>, String> {
    if shown.is_empty() || !reads_trace(source, session_id) {
        return Ok(Vec::new());
    }
    let rows = read(
        source,
        &format!("{SELECT} WHERE a.session_id = ?1 ORDER BY a.created_at_ms, a.id"),
        &[Value::from(session_id)],
    );
    let refusals = decisions.refusals(std::slice::from_ref(source));
    let directory = HashMap::new();
    let session = grants::session_facts(source, session_id);
    let mut declined = Vec::new();
    for row in &rows {
        let Some(approval) = vm(source, row, session.as_ref(), &directory, &refusals, now_ms)
        else {
            continue;
        };
        let was_shown = shown
            .iter()
            .any(|s| s.id == approval.id && s.binding_digest == approval.binding_digest);
        if !was_shown || !approval.declined_by_send {
            continue;
        }
        let op = decision_op(source, &approval.id, false, int(row, "version"));
        match source.replica.mutate(vec![op]) {
            Ok(commit) => {
                decisions.record(&commit, &source.database, &approval.id);
                declined.push(approval.id);
            }
            Err(error) => {
                tracing::warn!(database = %source.database, %error, "a request a message declines could not be declined");
                return Err(format!(
                    "keeper could not decline “{}” on this device, so your message was not \
                     sent; the request still waits for you ({error}).",
                    approval.summary
                ));
            }
        }
    }
    Ok(declined)
}

/// keeper's refusal of *Approve once* from a notification on a request the
/// replica holds at T4 (or at a tier it cannot read): approving it is done
/// on the card in keeper, from another of the person's devices (AD-428).
pub const NOT_FROM_A_NOTIFICATION: &str =
    "This request cannot be approved from a notification; decide on the card in keeper.";

/// Whether `source` holds what deciding `approval_id` needs: the person
/// reads the session's trace, the request is here and — to approve — so is
/// the exact call it runs.
fn here(source: &Source, session_id: &str, approval_id: &str, approve: bool) -> bool {
    reads_trace(source, session_id)
        && read(
            source,
            &format!("{SELECT} WHERE a.id = ?1 AND a.session_id = ?2"),
            &[Value::from(approval_id), Value::from(session_id)],
        )
        .first()
        .is_some_and(|row| !approve || action(row).is_some())
}

/// The decision an action on a notification sends (106.6), written as
/// [`decide`] writes the card's: once the session's replica — `source_of`,
/// asked again while it starts — holds the request, or at `deadline`
/// whatever it holds, so the refusal says why. *Approve once* on a request
/// the replica holds at T4 is refused whatever the notification carried. A
/// decision the server has not taken by then is reported as not sent — the
/// lock screen's action ends with it — though it stays written here and
/// goes when the device next reaches the database. Blocking.
pub fn decide_pushed(
    source_of: impl Fn() -> Option<Source>,
    decisions: &Decisions,
    decision: &crate::hq::push::HqActionDecision,
    deadline: std::time::Instant,
) -> Result<AgentDecisionOutcomeVm, Refusal> {
    let session_id = decision.session.session_id.as_str();
    let approval_id = decision.approval_id.as_str();
    let source = loop {
        let source = source_of();
        let ready = source
            .as_ref()
            .is_some_and(|source| here(source, session_id, approval_id, decision.approve));
        if ready || std::time::Instant::now() >= deadline {
            break source.ok_or_else(|| {
                Refusal(format!(
                    "This device does not hold {} yet.",
                    decision.session.database
                ))
            })?;
        }
        std::thread::sleep(Duration::from_millis(50));
    };
    if decision.approve {
        let row = read(
            &source,
            &format!("{SELECT} WHERE a.id = ?1"),
            &[Value::from(approval_id)],
        );
        let tier = row.first().map(|row| text(row, "tier").to_owned());
        let below_t4 = tier.as_deref().is_some_and(|tier| {
            tier.strip_prefix('T')
                .and_then(|n| n.parse::<u8>().ok())
                .is_some_and(|n| n < 4)
        });
        // A drive write is allowed only where its preview is seen and this
        // device carries it out: on its card.
        let writes = row
            .first()
            .is_some_and(|row| text(row, "tool_name") == crate::agents::drive_write::TOOL);
        if (tier.is_some() && !below_t4) || writes {
            return Err(Refusal(NOT_FROM_A_NOTIFICATION.to_owned()));
        }
    }
    let outcome = decide(
        &source,
        decisions,
        session_id,
        approval_id,
        &decision.binding_digest,
        decision.approve,
        crate::hq::sessions::now_ms(),
    )?;
    match outcome.state {
        AgentDecisionStateVm::Sent => Ok(outcome),
        AgentDecisionStateVm::Waiting => Err(Refusal(outcome.sentence)),
    }
}
