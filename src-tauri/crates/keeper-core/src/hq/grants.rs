//! Standing grants (109.11, Q109-09 b, Q109-10 a; the contract's V15 and V8
//! as amended): one person's *Always allow* for one agent, tool and argument
//! scope in one database, optionally bound to one session (*This session
//! only*). A grant stands in for that person's decision on a later request
//! the agent's runtime finds it matches; every use is still its own approval
//! row, with its own digest, consumed once.
//!
//! Everything a grant says is decided here, never in the webview: whether a
//! card offers one at all, the argument scopes it may name, the person-agent
//! key it is filed under ([`crate::hq::vocabulary::person_agent_key`], the
//! server's `personAgentKey`), and the sentences saying how broad it is.
//! A grant is offered only where the server could take it and the runtime
//! could ever apply it:
//! - at a grant tier ([`crate::hq::vocabulary::grant_tiers`]: T2, T3) — never
//!   T4, which another device decides every time;
//! - for a tool on [`GRANTABLE_TOOLS`] — an allow-list, so `delegate`, every
//!   drive tool and any other spelling are never offered;
//! - in a session that has not read outside content (Q109-10 a: a grant
//!   never applies in an untrusted session, so offering one there would be
//!   a promise the runtime does not keep);
//! - in a session still open;
//! - to a person the session's agent serves in this database — the server's
//!   V15 refuses a grant about any other agent (`hq.v15.agent`), and keeper
//!   knows which agents serve the person only from the server itself: the
//!   `person_agent` keys it grants this device's grants window
//!   ([`crate::hq::port::Replica::granted`]). A key not granted, or not yet
//!   pulled, offers nothing.
//!
//! Elsewhere the menu leaves *Always allow …* out rather than disabling it.
//!
//! After it is written, a grant's only change is one revocation by its
//! person; it is never deleted. Revoking stops new uses, not the approvals
//! the grant already issued ([`REVOKE_NOTE`]), and a session-bound grant
//! ends with its session. A refused write is read back from the replica's
//! per-row outcome record ([`crate::hq::port::Replica::row_writes`],
//! [`crate::hq::port::Replica::refused_commits`]), as a refused rename is.

use std::collections::{HashMap, HashSet};

use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};
use ts_rs::TS;

use crate::hq::approvals::{self, AgentDecisionOutcomeVm, Decisions, Refusal};
use crate::hq::port::{Rejection, ReplicaError, RowOp, RowWrites};
use crate::hq::sessions::{
    authored, directory, int, json as json_column, object, opt_text, read, text, truthy,
    SessionNames, Source,
};
use crate::hq::vocabulary;
use crate::hq::{sentence, Hq, HqError};

/// The table.
pub const TABLE: &str = "standing_grants";

/// The tools a grant may name: the server's `GRANTABLE_TOOLS` (V15), tools it
/// knows write no drive. `run` works in the agent's own workspace (D-33);
/// the Paseo broker's two verbs start or steer a run. An allow-list — a tool
/// not here is never offered, however it is spelled.
pub const GRANTABLE_TOOLS: [&str; 3] = [
    "run",
    "mcp__paseo__create_agent",
    "mcp__paseo__send_agent_prompt",
];

/// The tool whose scope names its program and, optionally, its first
/// argument.
const RUN: &str = "run";

/// Programs that run the code they are given (R10911-08, R10911-11): with
/// no argument named, or with an option first — `-c`, `-lc`, `-ec`, `-e`,
/// `-r`, `--eval`, or any flag another flag such as `-c` may follow — a
/// grant of one covers any code. A version suffix counts (`python3.12`).
const INTERPRETERS: [&str; 26] = [
    "sh",
    "bash",
    "zsh",
    "dash",
    "ksh",
    "mksh",
    "ash",
    "fish",
    "tcsh",
    "csh",
    "python",
    "node",
    "deno",
    "bun",
    "perl",
    "ruby",
    "php",
    "lua",
    "osascript",
    "pwsh",
    "powershell",
    "tclsh",
    "Rscript",
    "julia",
    "elixir",
    "erl",
];

/// Subcommands with which an interpreter runs the text that follows.
const EVAL_SUBCOMMANDS: [&str; 4] = ["eval", "repl", "-", "run"];

/// Programs whose first argument is program text: any grant of one covers
/// any code.
const PROGRAM_TEXT: [&str; 5] = ["awk", "gawk", "mawk", "nawk", "busybox"];

/// Programs that run another program named in their arguments: a grant of
/// one covers whatever program follows, so any code.
const WRAPPERS: [&str; 19] = [
    "env", "sudo", "doas", "su", "runuser", "ssh", "nice", "ionice", "timeout", "nohup", "xargs",
    "stdbuf", "setsid", "chroot", "flock", "time", "command", "exec", "watch",
];

/// What revoking does, said wherever a grant is made or revoked.
pub const REVOKE_NOTE: &str =
    "Revoking a grant stops its new uses. Approvals it has already issued still run.";

/// What no grant ever covers, said beside every offer.
pub const NEVER: &str = "It never applies in a session that has read outside content, at T4, \
     or to another agent, database or program. Each use is still its own approval, recorded \
     under the grant.";

/// The expiries a grant may be given, in days; absent is "until revoked".
pub const EXPIRY_DAYS: [u32; 3] = [1, 7, 30];

const DAY_MS: i64 = 86_400_000;

/// A session as a grant's offer reads it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessionFacts {
    pub agent_actor: String,
    pub title: String,
    /// `sessions.trust` is `untrusted` (V13: it never goes back).
    pub untrusted: bool,
    pub archived: bool,
    /// `status` is `open` and it is not archived: a grant may be bound to it.
    pub open: bool,
    /// The session's agent serves this person in this database: the server
    /// granted their person-agent key to the grants window.
    pub served: bool,
}

/// `session_id`'s row, or `None` while it is not on this device.
pub fn session_facts(source: &Source, session_id: &str) -> Option<SessionFacts> {
    let rows = read(
        source,
        "SELECT agent_actor, title, archived, status, trust FROM sessions WHERE id = ?1",
        &[Value::from(session_id)],
    );
    let row = rows.first()?;
    let archived = truthy(row, "archived");
    let agent = text(row, "agent_actor");
    let key = vocabulary::person_agent_key(&source.actor, agent);
    let served = source
        .replica
        .granted(TABLE, "person_agent")
        .unwrap_or_else(|error| {
            tracing::debug!(database = %source.database, %error, "the granted person-agent keys could not be read");
            Vec::new()
        })
        .contains(&key);
    Some(SessionFacts {
        served,
        agent_actor: agent.to_owned(),
        title: text(row, "title").to_owned(),
        untrusted: text(row, "trust") == "untrusted",
        archived,
        open: text(row, "status") == "open" && !archived,
    })
}

/// One argument scope a grant may name (`args_match`).
#[derive(Debug, Clone, PartialEq, Eq)]
enum Scope {
    /// `run` with this program and this first argument, whatever follows.
    ProgramArg(String, String),
    /// `run` with this program, whatever arguments follow.
    Program(String),
    /// Every call of the tool, whatever its arguments.
    Any,
}

impl Scope {
    fn id(&self) -> &'static str {
        match self {
            Scope::ProgramArg(..) => "programArg",
            Scope::Program(_) => "program",
            Scope::Any => "any",
        }
    }

    /// The contract's `args_match` for it.
    fn args_match(&self) -> Value {
        match self {
            Scope::ProgramArg(program, arg) => json!({ "program": program, "arg": arg }),
            Scope::Program(program) => json!({ "program": program }),
            Scope::Any => json!({}),
        }
    }

    /// The scope a stored `args_match` of `tool` names, if it has the
    /// contract's shape.
    fn of(tool: &str, args_match: &Value) -> Option<Scope> {
        let scope = args_match.as_object()?;
        if tool != RUN {
            return scope.is_empty().then_some(Scope::Any);
        }
        let program = scope.get("program")?.as_str().filter(|p| !p.is_empty())?;
        match scope.get("arg") {
            None => Some(Scope::Program(program.to_owned())),
            Some(Value::String(arg)) => Some(Scope::ProgramArg(program.to_owned(), arg.clone())),
            Some(_) => None,
        }
    }

    /// What it covers, as a phrase: "run “git status …”".
    fn covers(&self, tool: &str) -> String {
        match self {
            Scope::ProgramArg(program, arg) => format!("run “{program} {arg} …”"),
            Scope::Program(program) => format!("run “{program} …”"),
            Scope::Any => format!("call {tool}"),
        }
    }

    /// What follows it that the grant does not look at.
    fn whatever(&self) -> String {
        match self {
            Scope::ProgramArg(_, arg) => format!("whatever follows “{arg}”"),
            Scope::Program(_) => "whatever arguments follow".to_owned(),
            Scope::Any => "whatever its arguments".to_owned(),
        }
    }

    /// The plain warning for a scope that lets a program run whatever code
    /// it is given (R10911-11): an interpreter named alone or with an option
    /// or an eval subcommand first, a program whose first argument is
    /// program text, or a wrapper that runs whatever program follows it.
    fn any_code(&self) -> Option<String> {
        let (program, arg) = match self {
            Scope::Program(program) => (program, None),
            Scope::ProgramArg(program, arg) => (program, Some(arg.as_str())),
            Scope::Any => return None,
        };
        let name = program.rsplit('/').next().unwrap_or(program);
        let is = |known: &str| {
            name == known
                || name
                    .strip_prefix(known)
                    .is_some_and(|v| v.chars().all(|c| c.is_ascii_digit() || c == '.'))
        };
        if WRAPPERS.contains(&name) {
            return Some(format!(
                " That is any code: {name} runs whatever program follows it."
            ));
        }
        let text = PROGRAM_TEXT.iter().any(|known| is(known))
            || (INTERPRETERS.iter().any(|known| is(known))
                && arg.is_none_or(|arg| arg.starts_with('-') || EVAL_SUBCOMMANDS.contains(&arg)));
        text.then(|| format!(" That is any code: {name} runs whatever program text it is given."))
    }
}

/// The scopes a grant of `tool` may name for a call with `args`, narrowest
/// first: for `run`, its program and first argument, then its program alone;
/// for every other grantable tool, any call. Empty when the call has no
/// shape a scope can name (a `run` with no program).
fn scopes(tool: &str, args: &Value) -> Vec<Scope> {
    if !GRANTABLE_TOOLS.contains(&tool) {
        return Vec::new();
    }
    if tool != RUN {
        return vec![Scope::Any];
    }
    let argv = args.get("argv").and_then(Value::as_array);
    let Some(program) = argv
        .and_then(|argv| argv.first())
        .and_then(Value::as_str)
        .filter(|p| !p.is_empty())
    else {
        return Vec::new();
    };
    let mut out = Vec::new();
    if let Some(arg) = argv.and_then(|argv| argv.get(1)).and_then(Value::as_str) {
        out.push(Scope::ProgramArg(program.to_owned(), arg.to_owned()));
    }
    out.push(Scope::Program(program.to_owned()));
    out
}

/// One scope a card's *Always allow …* may grant.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct AgentGrantScopeVm {
    /// What the write names; Rust derives the scope again from the request.
    pub id: String,
    /// The choice's label: "“git status …”".
    pub label: String,
    /// How broad a standing grant of it is, in plain words.
    pub standing: String,
    /// How broad a grant of it bound to this session is.
    pub session: String,
}

/// One expiry a grant may be given.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct AgentGrantExpiryVm {
    /// Absent: until revoked.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub days: Option<u32>,
    pub label: String,
}

/// What a card's *Always allow …* and *This session only* would grant.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct AgentGrantOfferVm {
    pub agent_name: String,
    pub tool: String,
    /// The highest tier it covers: the request's.
    pub tier: String,
    /// Narrowest first; the first is the default.
    pub scopes: Vec<AgentGrantScopeVm>,
    pub expiries: Vec<AgentGrantExpiryVm>,
    /// What no grant ever covers.
    pub never: String,
    /// What revoking it does.
    pub revoke_note: String,
}

/// Why a request offers no grant, as keeper says it when one is asked for
/// anyway; `None` when it offers one.
fn withheld(tier: &str, tool: &str, session: &SessionFacts) -> Option<String> {
    if tier == "T4" {
        return Some(
            "A T4 request is decided from another of your devices every time; it cannot be \
             always allowed."
                .to_owned(),
        );
    }
    if !vocabulary::grant_tiers().iter().any(|t| t == tier) {
        return Some("Only T2 and T3 requests can be always allowed.".to_owned());
    }
    if !GRANTABLE_TOOLS.contains(&tool) {
        return Some(format!(
            "{tool} cannot be always allowed; allow it once instead."
        ));
    }
    if session.untrusted {
        return Some(
            "This session has read outside content, so nothing in it is always allowed; allow \
             it once instead."
                .to_owned(),
        );
    }
    if !session.open {
        return Some("This session is closed, so nothing in it can be always allowed.".to_owned());
    }
    if !session.served {
        return Some(
            "This agent does not serve you in this database, so what it asks cannot be always \
             allowed for you; allow it once instead."
                .to_owned(),
        );
    }
    None
}

/// The grant a request's card may offer, or `None` where *Always allow …*
/// is left out: [`withheld`], or a call whose arguments no scope names.
/// `agent_name` is `None` while the directory has not supplied the agent's
/// row: the offer then says "this agent", never its raw id (R604-01).
pub fn offer(
    tier: &str,
    tool: &str,
    args: &Value,
    session: &SessionFacts,
    agent_name: Option<&str>,
    database: &str,
) -> Option<AgentGrantOfferVm> {
    if withheld(tier, tool, session).is_some() {
        return None;
    }
    let scopes = scopes(tool, args);
    if scopes.is_empty() {
        return None;
    }
    let (agent_name, subject) = match agent_name {
        Some(name) => (name, name),
        None => ("this agent", "This agent"),
    };
    Some(AgentGrantOfferVm {
        agent_name: agent_name.to_owned(),
        tool: tool.to_owned(),
        tier: tier.to_owned(),
        scopes: scopes
            .iter()
            .map(|scope| AgentGrantScopeVm {
                id: scope.id().to_owned(),
                label: match scope {
                    Scope::ProgramArg(program, arg) => format!("“{program} {arg} …”"),
                    Scope::Program(program) => format!("“{program} …”"),
                    Scope::Any => format!("{tool}, any arguments"),
                },
                standing: format!(
                    "{subject} may {} in every session you share with it in {database}, \
                     without asking — {}.{}",
                    scope.covers(tool),
                    scope.whatever(),
                    scope.any_code().unwrap_or_default()
                ),
                session: format!(
                    "{subject} may {} in this session, without asking — {}.{}",
                    scope.covers(tool),
                    scope.whatever(),
                    scope.any_code().unwrap_or_default()
                ),
            })
            .collect(),
        expiries: std::iter::once(AgentGrantExpiryVm {
            days: None,
            label: "Until you revoke it".to_owned(),
        })
        .chain(EXPIRY_DAYS.iter().map(|days| AgentGrantExpiryVm {
            days: Some(*days),
            label: match days {
                1 => "For a day".to_owned(),
                7 => "For a week".to_owned(),
                n => format!("For {n} days"),
            },
        }))
        .collect(),
        never: NEVER.to_owned(),
        revoke_note: REVOKE_NOTE.to_owned(),
    })
}

/// The row a grant writes (V15): this person and the session's agent under
/// their person-agent key, from this device, unrevoked, made at `now_ms`.
#[allow(clippy::too_many_arguments)]
fn grant_op(
    source: &Source,
    agent: &str,
    session_id: Option<&str>,
    tool: &str,
    scope: &Scope,
    tier: &str,
    expires_at_ms: Option<i64>,
    now_ms: i64,
) -> RowOp {
    RowOp::Upsert {
        table: TABLE.to_owned(),
        values: authored(
            source,
            object(json!({
                "id": ulid::Ulid::new().to_string(),
                "person_agent_key": vocabulary::person_agent_key(&source.actor, agent),
                "person_actor": source.actor,
                "agent_actor": agent,
                "session_id": session_id,
                "tool": tool,
                "args_match": scope.args_match().to_string(),
                "max_tier": tier,
                "expires_at_ms": expires_at_ms,
                "revoked_at_ms": Value::Null,
                "granted_by_client": source.client,
                "created_at_ms": now_ms,
            })),
        ),
        base_version: None,
    }
}

/// What a card's *Always allow …* or *This session only* asks for.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AlwaysAllow {
    pub session_id: String,
    pub approval_id: String,
    pub binding_digest: String,
    /// One of the offer's [`AgentGrantScopeVm::id`].
    pub scope: String,
    pub session_only: bool,
    /// One of [`EXPIRY_DAYS`], or absent for "until revoked".
    pub expires_in_days: Option<u32>,
}

/// Approve the request and write the grant it offers, in one commit — the
/// grant is refused with the decision, so a refused *Always allow* leaves
/// the request waiting and the card says why — then wait as a decision
/// does ([`approvals::decide`]). The scope is derived again here from the
/// request's own call, never taken from the webview. Blocking.
pub fn always_allow(
    source: &Source,
    decisions: &Decisions,
    ask: &AlwaysAllow,
    now_ms: i64,
) -> Result<AgentDecisionOutcomeVm, Refusal> {
    let row = approvals::decidable(
        source,
        decisions,
        &ask.session_id,
        &ask.approval_id,
        &ask.binding_digest,
        true,
        now_ms,
    )?;
    let session = session_facts(source, &ask.session_id)
        .ok_or_else(|| Refusal("This conversation is not on this device.".to_owned()))?;
    let tool = text(&row, "tool_name");
    let tier = text(&row, "tier");
    if let Some(why) = withheld(tier, tool, &session) {
        return Err(Refusal(why));
    }
    let scope = scopes(tool, &json_column(&row, "args"))
        .into_iter()
        .find(|scope| scope.id() == ask.scope)
        .ok_or_else(|| Refusal("This request does not offer that scope.".to_owned()))?;
    let expires_at_ms = match ask.expires_in_days {
        None => None,
        Some(days) if EXPIRY_DAYS.contains(&days) => Some(now_ms + i64::from(days) * DAY_MS),
        Some(_) => return Err(Refusal("A grant cannot be given that expiry.".to_owned())),
    };
    let bound = ask.session_only.then_some(ask.session_id.as_str());
    let grant = grant_op(
        source,
        &session.agent_actor,
        bound,
        tool,
        &scope,
        tier,
        expires_at_ms,
        now_ms,
    );
    // An earlier refused grant of the same scope is set aside by this one.
    let args_match = scope.args_match();
    let supersede = source
        .replica
        .refused_commits(TABLE)
        .unwrap_or_default()
        .into_iter()
        .filter(|refused| {
            refused.rows.iter().any(|(table, values)| {
                table == TABLE
                    && text(values, "agent_actor") == session.agent_actor
                    && text(values, "tool") == tool
                    && opt_text(values, "session_id").as_deref() == bound
                    && json_column(values, "args_match") == args_match
            })
        })
        .map(|refused| refused.commit_id)
        .collect();
    let decision = approvals::decision_op(source, &ask.approval_id, true, int(&row, "version"));
    approvals::settle(
        source,
        decisions,
        vec![grant, decision],
        &ask.approval_id,
        supersede,
        if ask.session_only {
            "Allowed, and allowed for the rest of this session."
        } else {
            "Allowed, and always allowed from now on."
        },
    )
}

/// Revoke this person's grant `grant_id`: its one change (V15). An earlier
/// refused revocation of it is set aside, in the revocation's own step
/// ([`crate::hq::port::Replica::mutate_superseding`]).
pub fn revoke(source: &Source, grant_id: &str, now_ms: i64) -> Result<(), String> {
    let rows = read(
        source,
        "SELECT person_actor, revoked_at_ms, _sync_version AS version FROM standing_grants \
         WHERE id = ?1",
        &[Value::from(grant_id)],
    );
    let Some(row) = rows.first() else {
        return Err("This grant is not on this device.".to_owned());
    };
    if text(row, "person_actor") != source.actor {
        return Err("Only the person who made a grant can revoke it.".to_owned());
    }
    if int(row, "revoked_at_ms").is_some() {
        return Err("This grant is already revoked.".to_owned());
    }
    let standing: Vec<String> = source
        .replica
        .row_writes(TABLE, grant_id, None)
        .map(|writes| writes.refused.into_iter().map(|r| r.commit_id).collect())
        .unwrap_or_default();
    source
        .replica
        .mutate_superseding(
            vec![RowOp::Patch {
                table: TABLE.to_owned(),
                values: authored(
                    source,
                    object(json!({ "id": grant_id, "revoked_at_ms": now_ms })),
                ),
                base_version: int(row, "version").filter(|v| *v > 0),
            }],
            standing,
        )
        .map_err(|error| error.to_string())?;
    source.replica.sync_now();
    Ok(())
}

/// Why the server refused a grant's write, in plain words where its code is
/// one keeper knows (matched exactly); any other as the server said it.
pub fn refused_because(rejection: &Rejection) -> String {
    let words = match rejection.code.as_str() {
        "hq.v15.agent" => "that agent does not serve you in this database.",
        "hq.v15.session" => {
            "that session is closed or archived, or you are not a person in it, so nothing can be \
             allowed in it alone."
        }
        "hq.v15.tool" => "this tool cannot be always allowed.",
        "hq.v15.tier" => "only T2 and T3 requests can be always allowed.",
        "hq.v15.args_match" => "the server does not take that scope for this tool.",
        "hq.v15.created_at" => {
            "this device's clock is too far from the server's; check the time and try again."
        }
        "hq.v15.expires_at" => "its expiry was not after the moment it was made.",
        "hq.v15.revoked_once" => "it was already revoked.",
        "hq.v15.immutable" | "hq.v15.revocation" | "hq.v15.delete" => {
            "a grant can only be revoked, once; nothing else about it changes."
        }
        "hq.v15.person_only"
        | "hq.v15.person_actor"
        | "hq.v15.key"
        | "hq.v15.granted_by"
        | "hq.v15.new_revoked" => "the server did not take it as yours, from this device.",
        _ if rejection.message.is_empty() => return rejection.code.clone(),
        _ => return rejection.message.clone(),
    };
    words.to_owned()
}

/// A grant's state as the person reads it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub enum AgentGrantStateVm {
    /// It decides new requests it matches.
    Active,
    /// Written here; it takes effect once the server has it.
    Waiting,
    /// Revoked by its person (or, once the server says so, with its
    /// session).
    Revoked,
    /// Bound to a session archived or deleted since: it ended with it and
    /// stays ended, even if the session is reopened (V15).
    Ended,
    Expired,
    /// Made on a device that has since been signed out (its `clients` row
    /// revoked): the server refuses every approval it would make
    /// (`hq.v8.grant_credential`), so it decides nothing. Still revocable,
    /// so the person can clear it (RG-08).
    Inactive,
    /// The server refused to write it.
    Refused,
}

/// One grant of Settings › Agents › *Always allowed*.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct AgentGrantVm {
    pub id: String,
    pub database: String,
    pub agent_actor: String,
    pub agent_name: String,
    pub tool: String,
    /// What it covers, in plain words, breadth included.
    pub scope: String,
    /// The session it is bound to, by title; absent for a standing grant.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub session: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional, type = "number")]
    pub expires_at_ms: Option<i64>,
    #[ts(type = "number")]
    pub created_at_ms: i64,
    pub state: AgentGrantStateVm,
    /// Rust's line for it.
    pub sentence: String,
    /// The server refused this person's revocation of it: its sentence.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub refused: Option<String>,
    pub can_revoke: bool,
}

/// Settings › Agents › *Always allowed*.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct AgentGrantsVm {
    /// Newest first, every held database.
    pub grants: Vec<AgentGrantVm>,
    /// What revoking does.
    pub note: String,
}

/// What a grant of `tool` with `args_match` covers, in plain words.
fn scope_sentence(tool: &str, args_match: &Value) -> String {
    match Scope::of(tool, args_match) {
        Some(scope) => {
            let covers = scope.covers(tool);
            let mut first = covers.chars();
            let covers = first
                .next()
                .map(|c| c.to_uppercase().chain(first).collect::<String>())
                .unwrap_or_default();
            format!(
                "{covers}, {}{}",
                scope.whatever(),
                scope
                    .any_code()
                    .map_or(String::new(), |warning| format!(".{warning}"))
            )
        }
        None => tool.to_owned(),
    }
}

fn grant_vm(
    source: &Source,
    values: &Map<String, Value>,
    names: &HashMap<String, String>,
) -> AgentGrantVm {
    let agent = text(values, "agent_actor").to_owned();
    let tool = text(values, "tool").to_owned();
    AgentGrantVm {
        id: text(values, "id").to_owned(),
        database: source.database.clone(),
        agent_name: names.get(&agent).cloned().unwrap_or_else(|| agent.clone()),
        scope: scope_sentence(&tool, &json_column(values, "args_match")),
        agent_actor: agent,
        tool,
        session: None,
        expires_at_ms: int(values, "expires_at_ms"),
        created_at_ms: int(values, "created_at_ms").unwrap_or(0),
        state: AgentGrantStateVm::Active,
        sentence: String::new(),
        refused: None,
        can_revoke: false,
    }
}

/// One database's grants by this person, and the ones the server refused
/// to write.
fn of_source(source: &Source, now_ms: i64) -> Vec<AgentGrantVm> {
    let names = directory(source);
    // A session's handle reads as the list's (R-HQ-86): two sessions sharing
    // the words are told apart by their session numbers here too.
    let handles = SessionNames::read(source);
    let rows = read(
        source,
        "SELECT g.id AS id, g.agent_actor AS agent_actor, g.session_id AS session_id, \
         g.tool AS tool, g.args_match AS args_match, g.expires_at_ms AS expires_at_ms, \
         g.revoked_at_ms AS revoked_at_ms, g.created_at_ms AS created_at_ms, \
         s.id AS session_row, s.title AS session_title, s.archived AS session_archived, \
         c.name AS device_name, c.revoked_at_ms AS device_revoked_at_ms \
         FROM standing_grants g LEFT JOIN sessions s ON s.id = g.session_id \
         LEFT JOIN clients c ON c.id = g.granted_by_client \
         WHERE g.person_actor = ?1",
        &[Value::from(source.actor.as_str())],
    );
    let held: HashSet<String> = rows.iter().map(|row| text(row, "id").to_owned()).collect();
    let mut out = Vec::new();
    for row in &rows {
        let mut grant = grant_vm(source, row, &names);
        let session_id = opt_text(row, "session_id");
        grant.session = session_id
            .as_deref()
            .map(|id| opt_text(row, "session_title").unwrap_or_else(|| handles.handle(id)));
        let writes = source
            .replica
            .row_writes(TABLE, &grant.id, None)
            .unwrap_or_else(|error| {
                tracing::debug!(database = %source.database, %error, "a grant's delivery could not be read");
                RowWrites::default()
            });
        let db = &source.database;
        let revoked = int(row, "revoked_at_ms").is_some();
        let expired = grant.expires_at_ms.is_some_and(|at| at <= now_ms);
        // Its session's row here, archived: the server ended the grant with
        // it, whether or not its revocation (R3) has reached this device yet.
        // A session row merely absent (the person left or was removed, or it
        // has not arrived yet) says nothing of the grant (R10911-03).
        let present = opt_text(row, "session_row").is_some();
        let ended = session_id.is_some() && present && truthy(row, "session_archived");
        // The device it was made on, signed out since (RG-08). A suspended or
        // lapsed device cannot be told apart here: the directory carries only
        // `revoked_at_ms` (docs/agents.md, known limits).
        let signed_out = int(row, "device_revoked_at_ms").is_some();
        (grant.state, grant.sentence) = if revoked && !writes.pending.is_empty() {
            (
                AgentGrantStateVm::Revoked,
                format!("Revoking; it stops once {db} has it."),
            )
        } else if ended {
            (
                AgentGrantStateVm::Ended,
                "Ended with its session; it stays ended if the session is reopened.".to_owned(),
            )
        } else if revoked {
            (AgentGrantStateVm::Revoked, "Revoked.".to_owned())
        } else if expired {
            (AgentGrantStateVm::Expired, "Expired.".to_owned())
        } else if signed_out {
            let device = opt_text(row, "device_name").map_or_else(
                || "a device".to_owned(),
                |name| format!("“{name}”, a device"),
            );
            (
                AgentGrantStateVm::Inactive,
                format!(
                    "Inactive: made on {device} that was signed out, so it decides nothing. \
                     Revoke it to clear it."
                ),
            )
        } else if !writes.pending.is_empty() {
            (
                AgentGrantStateVm::Waiting,
                format!("Written here; it takes effect once {db} has it."),
            )
        } else {
            let sentence = match &grant.session {
                Some(_) if !present => {
                    "Allowed in one session only; that session is not on this device.".to_owned()
                }
                Some(title) => format!("Allowed in “{title}” only."),
                None => format!(
                    "Always allowed in every session with {} in {db}.",
                    grant.agent_name
                ),
            };
            (AgentGrantStateVm::Active, sentence)
        };
        grant.can_revoke = !revoked && !expired && !ended;
        grant.refused = writes.refused.first().filter(|_| !revoked).map(|refusal| {
            format!(
                "The server refused revoking it: {}",
                refused_because(&refusal.rejection)
            )
        });
        out.push(grant);
    }
    let refused = source.replica.refused_commits(TABLE).unwrap_or_else(|error| {
        tracing::debug!(database = %source.database, %error, "the refused grants could not be read");
        Vec::new()
    });
    for commit in refused {
        for (table, values) in &commit.rows {
            // An insert carries its key; a revocation names only the row.
            let written_here = table == TABLE
                && values.contains_key("person_agent_key")
                && text(values, "person_actor") == source.actor;
            if !written_here || held.contains(text(values, "id")) {
                continue;
            }
            let mut grant = grant_vm(source, values, &names);
            grant.session = opt_text(values, "session_id").map(|id| handles.handle(&id));
            grant.state = AgentGrantStateVm::Refused;
            grant.sentence = format!(
                "The server refused this grant: {}",
                refused_because(&commit.rejection)
            );
            out.push(grant);
        }
    }
    out
}

/// Every held database's grants by this person, newest first.
pub fn list(sources: &[Source], now_ms: i64) -> AgentGrantsVm {
    let mut grants: Vec<AgentGrantVm> = sources
        .iter()
        .flat_map(|source| of_source(source, now_ms))
        .collect();
    grants.sort_by(|a, b| {
        b.created_at_ms
            .cmp(&a.created_at_ms)
            .then_with(|| a.database.cmp(&b.database))
            .then_with(|| b.id.cmp(&a.id))
    });
    AgentGrantsVm {
        grants,
        note: REVOKE_NOTE.to_owned(),
    }
}

/// Ask the server again for the grants window's units it revoked
/// (R10911-10). A unit is revoked when the agent does not serve the person
/// at that pull, and syncular-client never pulls a revoked unit again while
/// it stays in the window — so an agent that starts serving the person later
/// would never be offered a grant on this device, nor would the grants the
/// person made about it elsewhere ever list here. Taking the units out of
/// the window and putting them back subscribes each afresh; the next pull
/// asks the server, which grants it or revokes it again. The units asked
/// again.
pub fn reask_revoked(source: &Source) -> Result<Vec<String>, ReplicaError> {
    let revoked = source.replica.revoked_units(TABLE)?;
    if revoked.is_empty() {
        return Ok(revoked);
    }
    let base = crate::hq::sessions::window(TABLE);
    let units = source.replica.window_state(&base)?.units;
    let kept: Vec<String> = units
        .iter()
        .filter(|unit| !revoked.contains(unit))
        .cloned()
        .collect();
    source.replica.set_window(&base, &kept)?;
    source.replica.set_window(&base, &units)?;
    tracing::debug!(database = %source.database, units = revoked.len(), "asked the server again for revoked grants units");
    Ok(revoked)
}

/// When the soonest of this person's live grants expires, after `now_ms`:
/// the moment the grants list changes with no row changing (R10911-05).
pub fn next_expiry(sources: &[Source], now_ms: i64) -> Option<i64> {
    sources
        .iter()
        .flat_map(|source| {
            read(
                source,
                "SELECT min(expires_at_ms) AS at FROM standing_grants \
                 WHERE person_actor = ?1 AND revoked_at_ms IS NULL AND expires_at_ms > ?2",
                &[Value::from(source.actor.as_str()), Value::from(now_ms)],
            )
        })
        .filter_map(|row| int(&row, "at"))
        .min()
}

/// The units of the grants window: the person-agent key of this person with
/// every agent of the database's directory. The server grants the keys of
/// the agents that serve them (the `person_agent` scope) and leaves the rest
/// out.
pub fn units(source: &Source) -> Vec<String> {
    let mut keys: Vec<String> = read(source, "SELECT id FROM actors WHERE kind = 'agent'", &[])
        .iter()
        .filter_map(|row| opt_text(row, "id"))
        .map(|agent| vocabulary::person_agent_key(&source.actor, &agent))
        .collect();
    keys.sort();
    keys.dedup();
    keys
}

impl Hq {
    /// Settings › Agents › *Always allowed*.
    pub fn grants(&self) -> AgentGrantsVm {
        list(&self.sources(), crate::hq::sessions::now_ms())
    }

    /// A card's *Always allow …* or *This session only*: the request
    /// approved and the grant written. Blocking: it waits briefly for the
    /// server.
    pub fn always_allow(
        &self,
        database: &str,
        ask: &AlwaysAllow,
    ) -> Result<AgentDecisionOutcomeVm, HqError> {
        let source = self.source(database)?;
        let outcome = always_allow(
            &source,
            self.decisions()?,
            ask,
            crate::hq::sessions::now_ms(),
        )
        .map_err(sentence);
        self.touch();
        outcome
    }

    /// Revoke one of this person's grants.
    pub fn revoke_grant(&self, database: &str, grant_id: &str) -> Result<(), HqError> {
        let source = self.source(database)?;
        revoke(&source, grant_id, crate::hq::sessions::now_ms()).map_err(HqError::Sentence)?;
        self.touch();
        Ok(())
    }
}

#[cfg(test)]
#[path = "grants_tests.rs"]
mod tests;
