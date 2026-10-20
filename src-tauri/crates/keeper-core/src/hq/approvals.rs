//! Approvals as rows (AD-428): what an agent asks a person to allow, read
//! from a session's `approvals` and `tool_calls` rows under the reader-scoped
//! `trace` key, and a decision written as a row. The server's V8 decides who
//! may approve and refuses a tier-4 decision from the claim holder's host; a
//! refusal reaches the person as the server's sentence.
//!
//! An approval is decided only on the exact action it would run: a request
//! whose tool call is not on this device cannot be approved here, and a
//! decision names the binding digest it was shown, so a request that changed
//! since is refused rather than approved blind.

use std::collections::HashMap;
use std::time::Duration;

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use ts_rs::TS;

use crate::hq::port::{Delivery, LinkState, RowOp};
use crate::hq::sessions::{authored, int, names, object, opt_text, read, text, Source};

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
    /// Rust's line for it.
    pub sentence: String,
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
     a.state AS state, a.decided_by_actor AS decided_by_actor, a.expires_at_ms AS expires_at_ms, \
     a.created_at_ms AS created_at_ms, a._sync_version AS version, \
     t.id AS call_id, t.name AS tool_name, t.args AS args \
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

fn vm(
    source: &Source,
    row: &Map<String, Value>,
    directory: &HashMap<String, String>,
    now_ms: i64,
) -> Option<AgentApprovalVm> {
    let id = opt_text(row, "id")?;
    let state = state_of(row, now_ms);
    let action = action(row);
    let approvers = names(row, "approvers");
    let mine = approvers.contains(&source.actor);
    let name = |actor: &str| {
        directory
            .get(actor)
            .cloned()
            .unwrap_or_else(|| actor.to_owned())
    };
    let decided_by = opt_text(row, "decided_by_actor").map(|actor| name(&actor));
    let can_decide = state == AgentApprovalStateVm::Pending && mine && action.is_some();
    let sentence = match state {
        AgentApprovalStateVm::Pending if !mine => format!(
            "Waiting for {}.",
            approvers
                .iter()
                .map(|a| name(a))
                .collect::<Vec<_>>()
                .join(", ")
        ),
        AgentApprovalStateVm::Pending if action.is_none() => {
            "keeper does not hold this request's action yet, so it cannot be approved here."
                .to_owned()
        }
        AgentApprovalStateVm::Pending => "Waiting for you.".to_owned(),
        AgentApprovalStateVm::Approved => format!(
            "Approved by {}.",
            decided_by.as_deref().unwrap_or("someone")
        ),
        AgentApprovalStateVm::Denied => {
            format!("Denied by {}.", decided_by.as_deref().unwrap_or("someone"))
        }
        AgentApprovalStateVm::Consumed => format!(
            "Approved by {}; the agent has run it.",
            decided_by.as_deref().unwrap_or("someone")
        ),
        AgentApprovalStateVm::Expired => "Expired before anyone decided.".to_owned(),
    };
    Some(AgentApprovalVm {
        id,
        database: source.database.clone(),
        session_id: text(row, "session_id").to_owned(),
        tool_name: text(row, "tool_name").to_owned(),
        tier: text(row, "tier").to_owned(),
        summary: text(row, "summary").to_owned(),
        binding_digest: text(row, "binding_digest").to_owned(),
        action,
        expires_at_ms: int(row, "expires_at_ms").unwrap_or(0),
        created_at_ms: int(row, "created_at_ms").unwrap_or(0),
        state,
        decided_by,
        can_decide,
        sentence,
    })
}

/// A session's approvals, oldest first. Empty for a member who does not
/// read the trace: the server never sends them.
pub fn of_session(
    source: &Source,
    session_id: &str,
    directory: &HashMap<String, String>,
    now_ms: i64,
) -> Vec<AgentApprovalVm> {
    read(
        source,
        &format!("{SELECT} WHERE a.session_id = ?1 ORDER BY a.created_at_ms, a.id"),
        &[Value::from(session_id)],
    )
    .iter()
    .filter_map(|row| vm(source, row, directory, now_ms))
    .collect()
}

/// Every approval this person may decide now, in every held database,
/// oldest first: the decisions panel.
pub fn waiting(sources: &[Source], now_ms: i64) -> Vec<AgentApprovalVm> {
    let mut all: Vec<AgentApprovalVm> = Vec::new();
    for source in sources {
        let directory = crate::hq::sessions::directory(source);
        all.extend(
            read(
                source,
                &format!("{SELECT} WHERE a.state = 'pending' ORDER BY a.created_at_ms, a.id"),
                &[],
            )
            .iter()
            .filter_map(|row| vm(source, row, &directory, now_ms))
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

/// Why a decision was not written, as the person reads it.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("{0}")]
pub struct Refusal(pub String);

/// Write this person's decision on `approval_id`, bound to the digest they
/// were shown, then wait up to [`DECISION_WAIT`] for the server: taken, or
/// waiting on this device, or refused with the server's sentence. Blocking.
pub fn decide(
    source: &Source,
    session_id: &str,
    approval_id: &str,
    binding_digest: &str,
    approve: bool,
    now_ms: i64,
) -> Result<AgentDecisionOutcomeVm, Refusal> {
    let rows = read(
        source,
        &format!("{SELECT} WHERE a.id = ?1 AND a.session_id = ?2"),
        &[Value::from(approval_id), Value::from(session_id)],
    );
    let Some(row) = rows.first() else {
        return Err(Refusal(
            "This request is not on this device any more.".to_owned(),
        ));
    };
    if text(row, "binding_digest") != binding_digest {
        return Err(Refusal(
            "This request changed since it was shown; look at it again before deciding.".to_owned(),
        ));
    }
    match state_of(row, now_ms) {
        AgentApprovalStateVm::Pending => {}
        AgentApprovalStateVm::Expired => {
            return Err(Refusal(
                "This request expired before it was decided.".to_owned(),
            ))
        }
        _ => return Err(Refusal("This request was already decided.".to_owned())),
    }
    if !names(row, "approvers").contains(&source.actor) {
        return Err(Refusal("This request is not yours to decide.".to_owned()));
    }
    if approve && action(row).is_none() {
        return Err(Refusal(
            "keeper does not hold this request's action yet, so it cannot be approved here."
                .to_owned(),
        ));
    }
    // The version read, so a decision racing another device's conflicts
    // instead of overwriting it; an optimistic row has none yet.
    let base_version = int(row, "version").filter(|v| *v > 0);
    let commit = source
        .replica
        .mutate(vec![RowOp::Patch {
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
            base_version,
        }])
        .map_err(|error| Refusal(error.to_string()))?;
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
                    sentence: if approve { "Approved." } else { "Denied." }.to_owned(),
                })
            }
            Ok(Delivery::Refused(sentence)) => return Err(Refusal(sentence)),
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
