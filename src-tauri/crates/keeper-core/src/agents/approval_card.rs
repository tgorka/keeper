//! The approval card as a person's keeper draws it (AD-395, UX-DR136;
//! FR-798): every device's half of 93.3.
//!
//! A request is read beside the timeline, as the header is (R33): only from
//! the session's own agent — the room's creator or the agent its claim
//! names — at an agent's power, never the own user, and sealed by a device
//! the SDK links to its sender (R185). Its `m.replace` edits (a gate's
//! coalesced card, R91) count only from the original sender and only when
//! they keep every record already listed, byte for byte. Each record shows
//! decide buttons only when its `binding_digest` is the digest of what the
//! card shows: the device recomputes it over the inline arguments, or, for
//! an attached action, `agent_approval_payload` does over the file's bytes
//! before anything is approved (R186).
//!
//! The room is folded in its own order, again from the event cache whenever
//! the cache changes other than by appending (R187). A decision in the room
//! is shown, the first one that would count by everything a device can see
//! (an approver, at T4 the requester, this record's digest, a scope it
//! offers), but it never closes the card: whether the deciding device
//! counts is the owning host's call (`trust::decide_trust`), and only the
//! host's own state — its `consumed`, from the requesting agent — or the
//! expiry ends the card.
//!
//! Every rule a card shows — the tier's words, which scopes are offered and
//! what they grant, who alone decides at T4, why this device cannot decide
//! — is here; the front draws the view model.

use std::collections::{BTreeMap, BTreeSet};
use std::sync::RwLock;

use chrono::{DateTime, Utc};
use matrix_sdk::ruma::events::room::EncryptedFile;
use matrix_sdk::ruma::{OwnedRoomId, OwnedUserId, RoomId, UserId};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use tokio::sync::watch;
use ts_rs::TS;

use crate::agents::approval::{session_reach, summary_of, Decision, Scope};
use crate::agents::events::{
    request_records, ApprovalDecisionContent, ApprovalRequestContent, ConsumedContent,
    APPROVAL_CONSUMED, APPROVAL_DECISION, APPROVAL_REQUEST, CONTENT_VERSION,
};
use crate::agents::tier::AgentTool;

/// This device is not verified by its owner's cross-signing identity.
pub const UNVERIFIED: &str = "This device cannot decide: it is not verified. Verify it from another of your devices in Settings › Encryption, then decide here.";
/// The person reading the card is not one of its approvers.
pub const NOT_AN_APPROVER: &str = "You are not one of the people who can decide this.";
/// A T4 card on the device of the app that runs the asking agent (S-22).
pub const THIS_DEVICE: &str = "Decide on another device: this app runs the agent that asks, so it cannot be the one that agrees.";
/// A T4 card naming nobody who asked.
pub const NO_REQUESTER: &str = "Nobody can decide this: it names nobody who asked.";
/// A record whose digest is not over what the card shows (R185).
pub const UNBOUND: &str =
    "keeper cannot decide this here: what the card shows is not what was sent for approval.";
/// Said under a T4 card's decider.
pub const IRREVERSIBLE: &str = "This cannot be undone.";
/// An action whose arguments travel as the request's encrypted file (R86).
pub const ATTACHED: &str =
    "The action is too large to show here: it is attached to the request, whole.";
/// An attached action approved before this device showed it (R186).
pub const UNSEEN: &str =
    "Open the attached action first: keeper approves only what it has shown you.";
/// An attachment that is not the action the request's digest binds.
pub const PAYLOAD_REFUSED: &str =
    "keeper cannot show the attached action: it is not the one sent for approval.";
/// An attachment that could not be fetched or opened.
pub const PAYLOAD_UNAVAILABLE: &str =
    "keeper cannot fetch the attached action now. Try again in a moment.";
/// A card whose action is not attached.
pub const NOT_ATTACHED: &str = "This action is shown on its card; nothing is attached.";
/// A decision for a card that is no longer pending.
pub const NOT_WAITING: &str = "This approval is no longer waiting for a decision.";
/// A decision for a card this device cannot find.
pub const NOT_FOUND: &str = "keeper cannot find this approval in the room.";
/// A decision for another action than the card's.
pub const OTHER_ACTION: &str = "This decision is for something other than what the card shows.";
/// A scope the card does not offer.
pub const SCOPE_NOT_OFFERED: &str = "This approval does not offer that.";
/// What approving once grants.
pub const ONCE_REACH: &str = "Lets this one action run once, exactly as shown.";
/// What approving an attached action for the session grants (R78).
pub const ATTACHED_REACH: &str = "Also lets this session run the same tool again in the same drive, on anything in the attached action's folder, without asking, until the session closes and for at most 24 hours.";

/// The longest note a decision carries, in characters; longer is cut.
pub const NOTE_MAX: usize = 500;

/// The person's answer to one card, as the webview sends it to
/// `agent_approval_decide`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct ApprovalDecideReq {
    /// The card's `id`.
    pub id: String,
    /// The card's `bindingDigest`.
    pub binding_digest: String,
    pub decision: Decision,
    /// One of the card's `scopes` (`once` with a deny).
    pub scope: Scope,
    /// A note for the agent, read with a deny.
    pub note: Option<String>,
}

/// Who reads the card: the own user, whether this device is cross-signed by
/// its owner, and the session rooms whose agent this app hosts (desktop
/// only; a T4 card is judged by its requesting session's room, R187).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Viewer {
    pub own: OwnedUserId,
    pub device_cross_signed: bool,
    pub hosted: BTreeSet<OwnedRoomId>,
}

impl Viewer {
    fn hosts(&self, room: &str) -> bool {
        self.hosted.iter().any(|hosted| hosted.as_str() == room)
    }
}

/// A person a card names: their user id and their name in the room.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct ApprovalPersonVm {
    pub user: String,
    pub name: String,
}

/// A way to approve the card offers: the scope `agent_approval_decide`
/// sends, the button's words, and what approving so grants (R78). Deny is
/// always offered beside them.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct ScopeOfferVm {
    pub scope: Scope,
    pub label: String,
    pub detail: String,
}

/// A declassification card (R25, 92.6): who would read what, by its bytes.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct DeclassifyVm {
    pub readers: Vec<ApprovalPersonVm>,
    pub what: String,
    pub sha256: String,
    pub sentence: String,
}

/// Where the card stands, as the room shows it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(
    tag = "state",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
#[ts(export)]
pub enum ApprovalStateVm {
    /// Waiting for a decision.
    Pending,
    /// A person's decision is in the room and the host has not used it
    /// yet. The card stays decidable: the host may have ignored it (a
    /// device it does not trust, an action that drifted), and a deny ends
    /// nothing in the room (R187).
    Decided {
        decision: Decision,
        scope: Scope,
        by: String,
        by_name: String,
    },
    /// The requesting agent used the approval: the action ran (or was
    /// attempted) once.
    Consumed,
    /// Nobody's decision was used in time.
    Expired,
}

/// One record's card (93.3 acceptance 4).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct ApprovalCardVm {
    /// The record's ULID: what `agent_approval_decide` names.
    pub id: String,
    /// The digest the decision must carry.
    pub binding_digest: String,
    pub tier: u8,
    /// The tier in words.
    pub tier_word: String,
    /// keeper's own sentence for the action, never the model's words.
    pub summary: String,
    /// The tool, as the model called it.
    pub tool: String,
    /// The exact arguments, pretty-printed JSON; `None` when attached, and
    /// for a `run`, which `run` draws whole.
    pub payload: Option<String>,
    /// [`ATTACHED`] and the arguments' SHA-256, when they travel as a file:
    /// `agent_approval_payload` fetches and checks them.
    pub attachment: Option<String>,
    /// The label's readers when the action parked, by name; empty when
    /// anyone reading the room may decide (`anyone`).
    pub approvers: Vec<ApprovalPersonVm>,
    pub anyone: bool,
    /// Who asked, the person who started it first.
    pub chain: Vec<ApprovalPersonVm>,
    /// The ways to approve, in order.
    pub scopes: Vec<ScopeOfferVm>,
    /// When it expires: ms since the Unix epoch.
    #[ts(type = "number")]
    pub expires_at: i64,
    pub state: ApprovalStateVm,
    /// Whether this device shows the decide buttons (AD-27).
    pub can_decide: bool,
    /// Why it does not, while the card is open.
    pub cannot_decide: Option<String>,
    /// Whether to show the way into this device's verification.
    pub verify: bool,
    /// At T4: who alone decides, and that it cannot be undone.
    pub only: Option<String>,
    pub declassify: Option<DeclassifyVm>,
    /// A `run`'s execution as keeper bound it (UX-DR139); `None` for any
    /// other tool, or when the action is attached — [`attached_payload`]
    /// then draws the same view once its file is checked (R231).
    pub run: Option<RunCardVm>,
}

/// What a `run` card draws (UX-DR139, R155, R213, R260): the argv as it
/// runs, one element per line — git's inserted `-c core.hooksPath=/dev/null`
/// with it; the program started, each wrapper a wrapper starts, and the
/// program the last one runs, with the first 12 hex digits of their
/// SHA-256; where it runs; the drives it asked to read and when it is
/// stopped; each piece of code the session holds; and, with network, the
/// workspace it releases. Not drawn: the environment, which is the host's
/// fixed one, and the folders' device and inode.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct RunCardVm {
    pub argv: Vec<String>,
    /// `/usr/bin/cargo · 0123456789ab`.
    pub program: String,
    /// Each wrapper a wrapper starts before the program (`env nice tool`
    /// starts `nice`), the same way, in order.
    pub wrappers: Vec<String>,
    /// The program a wrapper runs, the same way.
    pub wrapped: Option<String>,
    /// `workspace/<folder>`, as it resolved.
    pub cwd: String,
    /// Each drive id the run asked to read, read-only.
    pub reads: Vec<String>,
    /// The seconds after which it is stopped: the one it asked for, or the
    /// default.
    #[ts(type = "number")]
    pub timeout_s: u64,
    /// Each file of the code the session holds, or the inline code's flag,
    /// with its hash: `tool.py · 0123456789ab`.
    pub held: Vec<String>,
    pub network: Option<RunNetworkVm>,
}

/// The *Network* chip and the workspace a networked run releases.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct RunNetworkVm {
    /// "This command may reach any host. It sees only this session's
    /// workspace: <n> files, <size>."
    pub sentence: String,
    /// Every file it releases, by path.
    pub files: Vec<String>,
}

/// `text` as a run card shows one argument or one file's name (R231):
/// itself when nothing in it could be misread — not empty, no control or
/// bidirectional character, no quote or backslash, no space at either end
/// — else in double quotes with `\\`, `\"`, `\n`, `\t` and `\u{…}` escapes.
/// So an empty argument, one holding a newline and two arguments, a name
/// that a bidi override would reorder, each shows as what it is; the raw
/// values are what runs and what the digest binds.
fn card_text(text: &str) -> String {
    let bidi = |c: char| matches!(c, '\u{061c}' | '\u{200e}' | '\u{200f}' | '\u{202a}'..='\u{202e}' | '\u{2066}'..='\u{2069}');
    let plain = !text.is_empty()
        && text.trim() == text
        && !text
            .chars()
            .any(|c| c.is_control() || bidi(c) || c == '"' || c == '\\');
    if plain {
        return text.to_owned();
    }
    let mut out = String::from("\"");
    for c in text.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\t' => out.push_str("\\t"),
            c if c.is_control() || bidi(c) => out.push_str(&format!("\\u{{{:x}}}", c as u32)),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

/// `bytes` as a person reads a size.
fn size_words(bytes: u64) -> String {
    match bytes {
        0..=999 => format!("{bytes} bytes"),
        1_000..=999_999 => format!("{:.1} kB", bytes as f64 / 1e3),
        1_000_000..=999_999_999 => format!("{:.1} MB", bytes as f64 / 1e6),
        _ => format!("{:.1} GB", bytes as f64 / 1e9),
    }
}

/// What `request`, a `run` as its digest is over it — inline, or its
/// attachment read back — shows, each argument and name as [`card_text`].
fn run_card(request: &ApprovalRequestContent) -> RunCardVm {
    let binding = &request.action.exec_binding;
    let args = &request.action.args;
    let raw = |value: &Value| value.as_str().unwrap_or("").to_owned();
    let text = |value: &Value| card_text(value.as_str().unwrap_or(""));
    let hashed = |what: String, sha: &Value| {
        let sha = raw(sha);
        format!("{what} · {}", sha.get(..12).unwrap_or(&sha))
    };
    let network = request.preconditions.workspace.as_ref().map(|set| {
        let files: Vec<String> = set["files"]
            .as_array()
            .into_iter()
            .flatten()
            .map(|file| text(&file["path"]))
            .collect();
        RunNetworkVm {
            sentence: format!(
                "This command may reach any host. It sees only this session's workspace: {} {}, {}.",
                files.len(),
                if files.len() == 1 { "file" } else { "files" },
                size_words(set["bytes"].as_u64().unwrap_or(0)),
            ),
            files,
        }
    });
    RunCardVm {
        argv: binding["argv"]
            .as_array()
            .into_iter()
            .flatten()
            .map(text)
            .collect(),
        program: hashed(text(&binding["exe"]), &binding["exe_sha256"]),
        wrappers: binding["wrappers"]
            .as_array()
            .into_iter()
            .flatten()
            .map(|wrapper| hashed(text(&wrapper["path"]), &wrapper["sha256"]))
            .collect(),
        wrapped: binding
            .get("program")
            .map(|program| hashed(text(&program["path"]), &program["sha256"])),
        cwd: card_text(&format!("workspace/{}", raw(&binding["cwd"]))),
        reads: args["read"]
            .as_array()
            .into_iter()
            .flatten()
            .map(text)
            .collect(),
        timeout_s: args["timeout_s"]
            .as_u64()
            .unwrap_or(crate::agents::run::TIMEOUT_DEFAULT_S),
        held: binding["operands"]
            .as_array()
            .into_iter()
            .flatten()
            .map(|operand| {
                let what = operand["path"]
                    .as_str()
                    .or_else(|| operand["inline"].as_str())
                    .unwrap_or("");
                hashed(card_text(what), &operand["sha256"])
            })
            .collect(),
        network,
    }
}

/// One request event's cards: one, or a coalesced card's rows. `id` is the
/// first record's, which the timeline item names.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct ApprovalVm {
    pub id: String,
    pub cards: Vec<ApprovalCardVm>,
}

/// The tier as a card says it.
pub fn tier_word(tier: u8) -> String {
    let words = match tier {
        0 | 1 => "it only reads or changes what can be put back",
        2 => "it changes something that can be put back",
        3 => "it reaches beyond this session: it sends, or changes what runs",
        _ => "it cannot be undone",
    };
    format!("T{tier}: {words}")
}

fn expiry(request: &ApprovalRequestContent) -> DateTime<Utc> {
    DateTime::parse_from_rfc3339(&request.expires_at)
        .map(|at| at.with_timezone(&Utc))
        .unwrap_or(DateTime::<Utc>::MIN_UTC)
}

fn requester(request: &ApprovalRequestContent) -> Option<&str> {
    request.dispatch_chain.first().map(String::as_str)
}

/// How a record's digest stands against what its card shows.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Binding {
    /// The digest is over the inline arguments, and the summary is keeper's
    /// own for them.
    Inline,
    /// The arguments are an encrypted file whose descriptor reads; they
    /// are checked when fetched ([`verify_attached`]).
    Attached,
    /// Neither: no decide buttons.
    Unbound,
}

/// Whether `summary` is keeper's own sentence for `tool` with `args` and
/// the request's `exec_binding`.
fn summary_holds(request: &ApprovalRequestContent, args: &Value) -> bool {
    AgentTool::from_wire(&request.action.tool)
        .is_some_and(|tool| summary_of(tool, args, &request.action.exec_binding) == request.summary)
}

/// How `request`'s digest stands against what its card shows (R185).
pub fn binding(request: &ApprovalRequestContent) -> Binding {
    match &request.file {
        None if request.binds(&request.action.args)
            && summary_holds(request, &request.action.args) =>
        {
            Binding::Inline
        }
        Some(file)
            if request.action.args.is_null()
                && request.file_sha256.is_some()
                && serde_json::from_value::<EncryptedFile>(file.clone()).is_ok() =>
        {
            Binding::Attached
        }
        _ => Binding::Unbound,
    }
}

/// What an attached action's decrypted `bytes` hold, when they are the
/// file the request names and the action its digest and summary bind
/// (R186); else [`PAYLOAD_REFUSED`]. A `run`'s file holds its arguments,
/// its `exec_binding` and the workspace set it releases, all of which the
/// digest is over and the person sees (R213); any other's, the arguments.
pub fn verify_attached(request: &ApprovalRequestContent, bytes: &[u8]) -> Result<Value, String> {
    let refused = || PAYLOAD_REFUSED.to_owned();
    if binding(request) != Binding::Attached {
        return Err(NOT_ATTACHED.to_owned());
    }
    if request.file_sha256.as_deref() != Some(crate::agents::approval::sha256_hex(bytes).as_str()) {
        return Err(refused());
    }
    let payload: Value = serde_json::from_slice(bytes).map_err(|_| refused())?;
    if request.action.tool == AgentTool::Run.as_wire() {
        let (args, exec_binding, workspace) =
            crate::agents::approval::attached_run(payload.clone()).ok_or_else(refused)?;
        let mut whole = request.clone();
        whole.action.exec_binding = exec_binding;
        whole.preconditions.workspace = workspace;
        if !whole.binds(&args) || !summary_holds(&whole, &args) {
            return Err(refused());
        }
        return Ok(payload);
    }
    if !request.binds(&payload) || !summary_holds(request, &payload) {
        return Err(refused());
    }
    Ok(payload)
}

/// An attached action as a device shows it once its bytes are checked
/// (R186, R231): the action as text and, for a `run`, the same typed run
/// view an inline `run` card draws — its argv, programs, folder, held
/// code and, with network, the workspace it releases, counted, sized and
/// listed — built from what the digest binds, never from the card.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct ApprovalPayloadVm {
    /// The action, pretty-printed JSON.
    pub text: String,
    pub run: Option<RunCardVm>,
}

/// What an attached action's `bytes` show (R186, R231): refused as
/// [`verify_attached`] refuses them; else the payload as text and, for a
/// `run`, its run view from the arguments, binding and workspace set the
/// digest was just checked over.
pub fn attached_payload(
    request: &ApprovalRequestContent,
    bytes: &[u8],
) -> Result<ApprovalPayloadVm, String> {
    let payload = verify_attached(request, bytes)?;
    let text = serde_json::to_string_pretty(&payload).map_err(|_| PAYLOAD_REFUSED.to_owned())?;
    let run = (request.action.tool == AgentTool::Run.as_wire())
        .then(|| crate::agents::approval::attached_run(payload))
        .flatten()
        .map(|(args, exec_binding, workspace)| {
            let mut whole = request.clone();
            whole.action.args = args;
            whole.action.exec_binding = exec_binding;
            whole.preconditions.workspace = workspace;
            run_card(&whole)
        });
    Ok(ApprovalPayloadVm { text, run })
}

/// The scopes the card offers: the request's, never `session` above T2.
fn offered(request: &ApprovalRequestContent) -> Vec<Scope> {
    let mut scopes = Vec::new();
    for word in &request.scopes {
        let scope = match word.as_str() {
            "once" => Scope::Once,
            "session" if request.tier <= 2 => Scope::Session,
            _ => continue,
        };
        if !scopes.contains(&scope) {
            scopes.push(scope);
        }
    }
    scopes
}

/// Whether `user` may decide `request` by what a device can see: an
/// approver (anyone when the request names none), and at T4 the requester.
fn may_decide(request: &ApprovalRequestContent, user: &str) -> bool {
    let approver = request.approvers.is_empty() || request.approvers.iter().any(|a| a == user);
    approver && (request.tier < 4 || requester(request) == Some(user))
}

fn parse_user(user: &str) -> Option<OwnedUserId> {
    UserId::parse(user).ok()
}

fn person(user: &str, name: &dyn Fn(&UserId) -> String) -> ApprovalPersonVm {
    ApprovalPersonVm {
        user: user.to_owned(),
        name: parse_user(user).map_or_else(|| user.to_owned(), |id| name(&id)),
    }
}

/// Why `viewer` cannot decide `request` now, and whether verifying this
/// device would let them; `None` when they can.
pub fn refusal(
    request: &ApprovalRequestContent,
    viewer: &Viewer,
    name: &dyn Fn(&UserId) -> String,
) -> Option<(String, bool)> {
    if binding(request) == Binding::Unbound {
        return Some((UNBOUND.to_owned(), false));
    }
    let own = viewer.own.as_str();
    if !request.approvers.is_empty() && !request.approvers.iter().any(|a| a == own) {
        return Some((NOT_AN_APPROVER.to_owned(), false));
    }
    if request.tier >= 4 {
        match requester(request) {
            None => return Some((NO_REQUESTER.to_owned(), false)),
            Some(asked) if asked != own => {
                return Some((only(&person(asked, name).name), false));
            }
            Some(_) => {}
        }
    }
    if !viewer.device_cross_signed {
        return Some((UNVERIFIED.to_owned(), true));
    }
    if request.tier >= 4 && viewer.hosts(&request.room) {
        return Some((THIS_DEVICE.to_owned(), false));
    }
    None
}

/// Who alone decides a T4 card.
pub fn only(name: &str) -> String {
    format!("Only {name} can decide this.")
}

fn declassify(args: &Value, name: &dyn Fn(&UserId) -> String) -> DeclassifyVm {
    let readers: Vec<ApprovalPersonVm> = args["readers"]
        .as_array()
        .map(|readers| {
            readers
                .iter()
                .filter_map(Value::as_str)
                .map(|user| person(user, name))
                .collect()
        })
        .unwrap_or_default();
    let what = args["what"].as_str().unwrap_or("").to_owned();
    let sha256 = args["sha256"].as_str().unwrap_or("").to_owned();
    let names: Vec<&str> = readers.iter().map(|reader| reader.name.as_str()).collect();
    let sentence = format!(
        "Approving lets {} read {what}: exactly these bytes (SHA-256 {}), once. Nothing else of this session reaches them.",
        names.join(", "),
        sha256.get(..12).unwrap_or(&sha256),
    );
    DeclassifyVm {
        readers,
        what,
        sha256,
        sentence,
    }
}

impl ApprovalCardVm {
    /// The card for `request` in `state`, as `viewer` reads them, naming
    /// users as `name` does.
    pub fn of(
        request: &ApprovalRequestContent,
        state: ApprovalStateVm,
        viewer: &Viewer,
        name: &dyn Fn(&UserId) -> String,
    ) -> ApprovalCardVm {
        let attached = request.file.is_some();
        let run = request.action.tool == AgentTool::Run.as_wire();
        // A run is drawn as its typed view alone (R247): its arguments as
        // raw JSON would show a bidi override as the character it is.
        let payload = (!attached && !run)
            .then(|| serde_json::to_string_pretty(&request.action.args).ok())
            .flatten();
        let attachment = attached.then(|| match &request.file_sha256 {
            Some(sha) => format!("{ATTACHED} SHA-256 {sha}."),
            None => ATTACHED.to_owned(),
        });
        let scopes = offered(request);
        let session_offered = scopes.contains(&Scope::Session);
        let scopes = scopes
            .into_iter()
            .map(|scope| match scope {
                Scope::Once => ScopeOfferVm {
                    scope,
                    label: if session_offered {
                        "Approve once"
                    } else {
                        "Approve"
                    }
                    .to_owned(),
                    detail: ONCE_REACH.to_owned(),
                },
                Scope::Session => ScopeOfferVm {
                    scope,
                    label: "Approve for this session".to_owned(),
                    detail: match (attached, request.action.tool == AgentTool::Run.as_wire()) {
                        // A run's allowance is its programs and folder,
                        // whatever its size (R146, R231).
                        (true, true) => crate::agents::run::ATTACHED_SESSION_REACH.to_owned(),
                        (true, false) => ATTACHED_REACH.to_owned(),
                        (false, _) => session_reach(&request.action.tool, &request.action.args),
                    },
                },
            })
            .collect();
        let open = matches!(
            state,
            ApprovalStateVm::Pending | ApprovalStateVm::Decided { .. }
        );
        let refused = open.then(|| refusal(request, viewer, name)).flatten();
        let only = (request.tier >= 4).then(|| match requester(request) {
            Some(asked) => format!("{} {IRREVERSIBLE}", only(&person(asked, name).name)),
            None => format!("{NO_REQUESTER} {IRREVERSIBLE}"),
        });
        ApprovalCardVm {
            id: request.id.clone(),
            binding_digest: request.binding_digest.clone(),
            tier: request.tier,
            tier_word: tier_word(request.tier),
            summary: request.summary.clone(),
            tool: request.action.tool.clone(),
            payload,
            attachment,
            approvers: request
                .approvers
                .iter()
                .map(|user| person(user, name))
                .collect(),
            anyone: request.approvers.is_empty(),
            chain: request
                .dispatch_chain
                .iter()
                .map(|user| person(user, name))
                .collect(),
            scopes,
            expires_at: expiry(request).timestamp_millis(),
            can_decide: open && refused.is_none(),
            verify: refused.as_ref().is_some_and(|(_, verify)| *verify),
            cannot_decide: refused.map(|(sentence, _)| sentence),
            state,
            only,
            declassify: (request.action.tool == "declassify")
                .then(|| declassify(&request.action.args, name)),
            run: (run && !attached).then(|| run_card(request)),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct Request {
    event: String,
    sender: OwnedUserId,
    records: Vec<ApprovalRequestContent>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct Decided {
    sender: OwnedUserId,
    content: ApprovalDecisionContent,
}

/// Who sent an event a fold reads: the own user, whether a user holds an
/// agent's power in the room, and whether they are the session's own agent
/// (R185).
pub struct Senders<'a> {
    pub own: &'a UserId,
    pub agent: &'a dyn Fn(&UserId) -> bool,
    pub owner: &'a dyn Fn(&UserId) -> bool,
}

/// A room's approvals, folded from its events in the room's order.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ApprovalFold {
    requests: Vec<Request>,
    /// Every person's decision on each id, in the room's order.
    decisions: BTreeMap<String, Vec<Decided>>,
    /// Who sent a `consumed` for each id.
    consumed: BTreeMap<String, BTreeSet<OwnedUserId>>,
}

/// The internal state of one record before names are resolved.
enum State<'a> {
    Pending,
    Decided(&'a Decided),
    Consumed,
    Expired,
}

impl ApprovalFold {
    /// Fold one event (decrypted, as JSON), the next in the room's order;
    /// `sealed` says whether a device the SDK links to its sender sealed
    /// it. Returns whether anything changed.
    pub fn apply(&mut self, event: &Value, sealed: bool, senders: &Senders<'_>) -> bool {
        let kind = event["type"].as_str().unwrap_or("");
        let (Some(id), Some(sender)) = (
            event["event_id"].as_str(),
            event["sender"].as_str().and_then(parse_user),
        ) else {
            return false;
        };
        let content = &event["content"];
        match kind {
            APPROVAL_REQUEST => {
                if !sealed
                    || *sender == *senders.own
                    || !(senders.agent)(&sender)
                    || !(senders.owner)(&sender)
                {
                    return false;
                }
                self.request(id, sender, content)
            }
            APPROVAL_DECISION => {
                if !sealed || (senders.agent)(&sender) {
                    return false;
                }
                let Ok(decision) = ApprovalDecisionContent::deserialize(content) else {
                    return false;
                };
                self.decisions
                    .entry(decision.id.clone())
                    .or_default()
                    .push(Decided {
                        sender,
                        content: decision,
                    });
                true
            }
            APPROVAL_CONSUMED => {
                // A state event: the server lets only an agent's power send
                // it; which agent counts is the request's (R75).
                if !(senders.agent)(&sender) {
                    return false;
                }
                match ConsumedContent::deserialize(content) {
                    Ok(consumed)
                        if consumed.v == CONTENT_VERSION
                            && event["state_key"].as_str() == Some(consumed.id.as_str()) =>
                    {
                        self.consumed.entry(consumed.id).or_default().insert(sender)
                    }
                    _ => false,
                }
            }
            _ => false,
        }
    }

    fn listed(&self, id: &str) -> bool {
        self.requests
            .iter()
            .any(|request| request.records.iter().any(|record| record.id == id))
    }

    fn request(&mut self, event: &str, sender: OwnedUserId, content: &Value) -> bool {
        let relation = &content["m.relates_to"];
        if relation["rel_type"] == "m.replace" {
            let Some(target) = relation["event_id"].as_str() else {
                return false;
            };
            let Some(new) = request_records(&content["m.new_content"]) else {
                return false;
            };
            let Some(at) = self.requests.iter().position(|r| r.event == target) else {
                return false;
            };
            let held = &self.requests[at];
            // Only the original sender adds records, and never drops or
            // changes one already listed.
            if held.sender != sender
                || !held
                    .records
                    .iter()
                    .all(|record| new.iter().any(|edited| edited == record))
            {
                return false;
            }
            let added: Vec<ApprovalRequestContent> = new
                .into_iter()
                .filter(|record| !self.listed(&record.id))
                .fold(Vec::new(), |mut added, record| {
                    if !added
                        .iter()
                        .any(|a: &ApprovalRequestContent| a.id == record.id)
                    {
                        added.push(record);
                    }
                    added
                });
            if added.is_empty() {
                return false;
            }
            self.requests[at].records.extend(added);
            return true;
        }
        let Some(records) = request_records(content) else {
            return false;
        };
        let mut fresh: Vec<ApprovalRequestContent> = Vec::new();
        for record in records {
            if !self.listed(&record.id) && !fresh.iter().any(|f| f.id == record.id) {
                fresh.push(record);
            }
        }
        if fresh.is_empty() {
            return false;
        }
        self.requests.push(Request {
            event: event.to_owned(),
            sender,
            records: fresh,
        });
        true
    }

    fn records(&self) -> impl Iterator<Item = (&Request, &ApprovalRequestContent)> {
        self.requests
            .iter()
            .flat_map(|request| request.records.iter().map(move |record| (request, record)))
    }

    /// The record `id` names, as its card shows it.
    pub fn record(&self, id: &str) -> Option<&ApprovalRequestContent> {
        self.records()
            .map(|(_, record)| record)
            .find(|record| record.id == id)
    }

    fn state(
        &self,
        request: &Request,
        record: &ApprovalRequestContent,
        now: DateTime<Utc>,
    ) -> State<'_> {
        if self
            .consumed
            .get(&record.id)
            .is_some_and(|by| by.contains(&request.sender))
        {
            return State::Consumed;
        }
        if now >= expiry(record) {
            return State::Expired;
        }
        let offered = offered(record);
        let decided = self.decisions.get(&record.id).and_then(|decisions| {
            decisions.iter().find(|d| {
                d.content.binding_digest == record.binding_digest
                    && offered.contains(&d.content.scope)
                    && may_decide(record, d.sender.as_str())
            })
        });
        match decided {
            Some(d) => State::Decided(d),
            None => State::Pending,
        }
    }

    fn state_vm(
        &self,
        request: &Request,
        record: &ApprovalRequestContent,
        now: DateTime<Utc>,
        name: &dyn Fn(&UserId) -> String,
    ) -> ApprovalStateVm {
        match self.state(request, record, now) {
            State::Pending => ApprovalStateVm::Pending,
            State::Consumed => ApprovalStateVm::Consumed,
            State::Expired => ApprovalStateVm::Expired,
            State::Decided(d) => ApprovalStateVm::Decided {
                decision: d.content.decision,
                scope: d.content.scope,
                by: d.sender.to_string(),
                by_name: name(&d.sender),
            },
        }
    }

    /// Every request's cards, in the room's order, as `viewer` reads them
    /// at `now`.
    pub fn approvals(
        &self,
        viewer: &Viewer,
        now: DateTime<Utc>,
        name: &dyn Fn(&UserId) -> String,
    ) -> Vec<ApprovalVm> {
        self.requests
            .iter()
            .map(|request| ApprovalVm {
                id: request.records[0].id.clone(),
                cards: request
                    .records
                    .iter()
                    .map(|record| {
                        let state = self.state_vm(request, record, now, name);
                        ApprovalCardVm::of(record, state, viewer, name)
                    })
                    .collect(),
            })
            .collect()
    }

    /// The users the cards name, for the caller to resolve.
    pub fn named(&self) -> Vec<OwnedUserId> {
        let mut users = BTreeSet::new();
        for (_, record) in self.records() {
            let readers = record.action.args["readers"].as_array();
            let declassified = readers.into_iter().flatten().filter_map(Value::as_str);
            for user in record
                .approvers
                .iter()
                .map(String::as_str)
                .chain(record.dispatch_chain.iter().map(String::as_str))
                .chain(declassified)
            {
                if let Some(user) = parse_user(user) {
                    users.insert(user);
                }
            }
        }
        for decided in self.decisions.values().flatten() {
            users.insert(decided.sender.clone());
        }
        users.into_iter().collect()
    }

    /// When the next card still open turns expired, after `now`.
    pub fn next_expiry(&self, now: DateTime<Utc>) -> Option<DateTime<Utc>> {
        self.records()
            .filter(|(request, record)| {
                matches!(
                    self.state(request, record, now),
                    State::Pending | State::Decided(_)
                )
            })
            .map(|(_, record)| expiry(record))
            .filter(|at| *at > now)
            .min()
    }

    /// Whether `viewer` may send a decision on `id` with `binding_digest`
    /// and `scope` at `now`: the card's own rules, with the sentence it
    /// shows when not. A decision already in the room does not close it
    /// (R187).
    pub fn check(
        &self,
        viewer: &Viewer,
        id: &str,
        binding_digest: &str,
        scope: Scope,
        now: DateTime<Utc>,
        name: &dyn Fn(&UserId) -> String,
    ) -> Result<&ApprovalRequestContent, String> {
        let Some((request, record)) = self.records().find(|(_, record)| record.id == id) else {
            return Err(NOT_FOUND.to_owned());
        };
        if !matches!(
            self.state(request, record, now),
            State::Pending | State::Decided(_)
        ) {
            return Err(NOT_WAITING.to_owned());
        }
        if record.binding_digest != binding_digest {
            return Err(OTHER_ACTION.to_owned());
        }
        if !offered(record).contains(&scope) {
            return Err(SCOPE_NOT_OFFERED.to_owned());
        }
        match refusal(record, viewer, name) {
            Some((sentence, _)) => Err(sentence),
            None => Ok(record),
        }
    }

    /// Whether `viewer` may send the decision `req` at `now`, `shown`
    /// saying whether this app has shown an attached action with a
    /// binding digest: [`Self::check`], and an approve of an attached
    /// action not shown refused ([`UNSEEN`], R186) — so nothing is approved
    /// that [`attached_payload`] did not check and show first.
    pub fn decide(
        &self,
        viewer: &Viewer,
        req: &ApprovalDecideReq,
        now: DateTime<Utc>,
        name: &dyn Fn(&UserId) -> String,
        shown: &dyn Fn(&str) -> bool,
    ) -> Result<&ApprovalRequestContent, String> {
        let record = self.check(viewer, &req.id, &req.binding_digest, req.scope, now, name)?;
        if req.decision == Decision::Approve
            && binding(record) == Binding::Attached
            && !shown(&record.binding_digest)
        {
            return Err(UNSEEN.to_owned());
        }
        Ok(record)
    }
}

/// The session rooms whose agent this app hosts now: the desktop's agents
/// host replaces them on each tick; on the phone they stay empty. A T4 card
/// whose requesting session is one of them is never decided from this app
/// (S-22).
#[derive(Debug, Default)]
pub struct HostedRooms {
    rooms: RwLock<BTreeSet<OwnedRoomId>>,
    changes: watch::Sender<u64>,
}

impl HostedRooms {
    /// Replace every hosted room with `rooms`, by id; an id that does not
    /// read is no room.
    pub fn replace<'a>(&self, rooms: impl IntoIterator<Item = &'a str>) {
        let rooms: BTreeSet<OwnedRoomId> = rooms
            .into_iter()
            .filter_map(|room| RoomId::parse(room).ok())
            .collect();
        {
            let mut held = self
                .rooms
                .write()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            if *held == rooms {
                return;
            }
            *held = rooms;
        }
        self.changes.send_modify(|n| *n = n.wrapping_add(1));
    }

    /// The rooms hosted now.
    pub fn rooms(&self) -> BTreeSet<OwnedRoomId> {
        self.rooms
            .read()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .clone()
    }

    /// A receiver that sees every change after now.
    pub fn subscribe(&self) -> watch::Receiver<u64> {
        self.changes.subscribe()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use chrono::Duration;
    use serde_json::json;

    use crate::agents::approval::{binding_digest, sha256_hex};

    const TGORKA: &str = "@tgorka:example.org";
    const MARTA: &str = "@marta:example.org";
    const NIXI: &str = "@nixi:example.org";
    const TOLA: &str = "@tola:example.org";
    const EVE: &str = "@eve:example.org";
    const ROOM: &str = "!session:example.org";
    const DM: &str = "!dm:example.org";

    fn now() -> DateTime<Utc> {
        DateTime::parse_from_rfc3339("2026-10-05T12:00:00Z")
            .map(|at| at.with_timezone(&Utc))
            .expect("time")
    }

    fn name(user: &UserId) -> String {
        match user.as_str() {
            TGORKA => "Tomasz".to_owned(),
            MARTA => "Marta".to_owned(),
            NIXI => "Nixi".to_owned(),
            other => other.to_owned(),
        }
    }

    /// Nixi and Tola hold an agent's power; Nixi is the session's own.
    fn agent(user: &UserId) -> bool {
        matches!(user.as_str(), NIXI | TOLA)
    }

    fn owner(user: &UserId) -> bool {
        user.as_str() == NIXI
    }

    fn viewer(own: &str) -> Viewer {
        Viewer {
            own: OwnedUserId::try_from(own).expect("user"),
            device_cross_signed: true,
            hosted: BTreeSet::new(),
        }
    }

    fn hosting(room: &str) -> Viewer {
        Viewer {
            hosted: BTreeSet::from([OwnedRoomId::try_from(room).expect("room")]),
            ..viewer(TGORKA)
        }
    }

    /// `request`'s digest and summary made again over what it carries, as
    /// a host makes them.
    fn sealed(mut request: Value, args: &Value) -> Value {
        let tool = request["action"]["tool"].as_str().expect("tool").to_owned();
        request["summary"] = json!(summary_of(
            AgentTool::from_wire(&tool).expect("a tool"),
            args,
            &request["action"]["exec_binding"]
        ));
        request["binding_digest"] = json!(binding_digest(
            request["id"].as_str().expect("id"),
            request["session"].as_str().expect("session"),
            request["agent"].as_str().expect("agent"),
            &tool,
            args,
            &request["action"]["exec_binding"],
            request["checkpoint_sha256"].as_str().expect("checkpoint"),
            &request["preconditions"],
        )
        .expect("digest"));
        request
    }

    fn write_args() -> Value {
        json!({"profile": "tgdrive", "path": "notes/plan.md", "content": "hello, world"})
    }

    fn request(id: &str, tier: u8, scopes: &[&str]) -> Value {
        let args = write_args();
        sealed(
            json!({
                "v": 1, "id": id, "session": "60-sessions/active/2026-10-05-chat",
                "room": ROOM, "agent": "nixi", "tier": tier, "summary": "",
                "action": {"tool": "drive_write", "args": args, "exec_binding": null},
                "checkpoint_sha256": "c0ffee",
                "preconditions": {
                    "files": [{"drive": "tgdrive", "path": "notes/plan.md",
                        "landing": "notes/plan.md", "sha256": null}],
                    "workspace": null, "screen": null, "max_staleness_s": null,
                },
                "binding_digest": "",
                "scopes": scopes,
                "expires_at": "2026-10-06T12:00:00.000Z",
                "approvers": [MARTA, TGORKA],
                "dispatch_chain": [TGORKA, NIXI],
            }),
            &args,
        )
    }

    fn digest_of(request: &Value) -> String {
        request["binding_digest"]
            .as_str()
            .expect("digest")
            .to_owned()
    }

    fn content(value: Value) -> ApprovalRequestContent {
        ApprovalRequestContent::deserialize(&value).expect("request")
    }

    fn card(value: Value, viewer: &Viewer) -> ApprovalCardVm {
        ApprovalCardVm::of(&content(value), ApprovalStateVm::Pending, viewer, &name)
    }

    fn event(kind: &str, id: &str, sender: &str, content: Value) -> Value {
        json!({"type": kind, "event_id": id, "sender": sender, "origin_server_ts": 1, "content": content})
    }

    fn decision(request: &Value, decision: &str, scope: &str) -> Value {
        json!({"id": request["id"], "binding_digest": request["binding_digest"],
            "decision": decision, "scope": scope})
    }

    fn me() -> OwnedUserId {
        OwnedUserId::try_from(TGORKA).expect("user")
    }

    fn apply(fold: &mut ApprovalFold, event: &Value, sealed: bool) -> bool {
        let own = me();
        fold.apply(
            event,
            sealed,
            &Senders {
                own: &own,
                agent: &agent,
                owner: &owner,
            },
        )
    }

    fn consumed(sender: &str, id: &str) -> Value {
        json!({"type": APPROVAL_CONSUMED, "event_id": format!("$c-{sender}-{id}"), "sender": sender,
            "state_key": id, "content": {"v": 1, "id": id, "epoch": 3, "host": "electra"}})
    }

    fn states(fold: &ApprovalFold, at: DateTime<Utc>) -> Vec<ApprovalStateVm> {
        fold.approvals(&viewer(TGORKA), at, &name)
            .into_iter()
            .flat_map(|approval| approval.cards)
            .map(|card| card.state)
            .collect()
    }

    /// 93.3 acceptance 4: the card shows exactly what would run, in
    /// keeper's words, who may decide, how, and what each way grants.
    #[test]
    fn the_card_carries_the_exact_payload() {
        let tgorka = viewer(TGORKA);
        let t2 = card(request("01A", 2, &["once", "session"]), &tgorka);
        assert_eq!(t2.summary, "Write `notes/plan.md` in tgdrive (12 bytes)");
        assert_eq!(t2.tool, "drive_write");
        let payload: Value =
            serde_json::from_str(t2.payload.as_deref().expect("payload")).expect("json");
        assert_eq!(payload, write_args());
        assert_eq!(t2.attachment, None);
        let names = |people: &[ApprovalPersonVm]| -> Vec<String> {
            people.iter().map(|p| p.name.clone()).collect()
        };
        assert_eq!(names(&t2.approvers), ["Marta", "Tomasz"]);
        assert!(!t2.anyone);
        assert_eq!(names(&t2.chain), ["Tomasz", "Nixi"]);
        assert_eq!(t2.binding_digest, digest_of(&request("01A", 2, &["once"])));
        assert_eq!(
            t2.tier_word,
            "T2: it changes something that can be put back"
        );
        assert_eq!(
            t2.expires_at,
            DateTime::parse_from_rfc3339("2026-10-06T12:00:00Z")
                .expect("time")
                .timestamp_millis()
        );
        let offers: Vec<(Scope, &str)> = t2
            .scopes
            .iter()
            .map(|offer| (offer.scope, offer.label.as_str()))
            .collect();
        assert_eq!(
            offers,
            [
                (Scope::Once, "Approve once"),
                (Scope::Session, "Approve for this session")
            ]
        );
        assert_eq!(t2.scopes[0].detail, ONCE_REACH);
        assert!(t2.can_decide && t2.cannot_decide.is_none() && t2.only.is_none());

        // T2 in a `main` session: the host offers `once` alone.
        let main = card(request("01B", 2, &["once"]), &tgorka);
        assert_eq!(main.scopes.len(), 1);
        assert_eq!(main.scopes[0].label, "Approve");
        // T3 never offers the session, whatever the request says.
        let t3 = card(request("01C", 3, &["once", "session"]), &tgorka);
        assert_eq!(
            t3.scopes.iter().map(|s| s.scope).collect::<Vec<_>>(),
            [Scope::Once]
        );
        // T4 names the requester as the one who decides.
        let t4 = card(request("01D", 4, &["once"]), &viewer(MARTA));
        assert_eq!(
            t4.only.as_deref(),
            Some("Only Tomasz can decide this. This cannot be undone.")
        );
        assert_eq!(
            t4.cannot_decide.as_deref(),
            Some("Only Tomasz can decide this.")
        );
        assert!(!t4.can_decide);

        // A declassification names who would read what, by its bytes.
        let args = json!({"readers": [MARTA], "what": "the brief", "sha256": "0123456789abcdef"});
        let mut out = request("01F", 3, &["once"]);
        out["action"] = json!({"tool": "declassify", "args": args, "exec_binding": null});
        let declassify = card(sealed(out, &args), &tgorka);
        assert!(declassify.can_decide);
        let declassify = declassify.declassify.expect("declassify");
        assert_eq!(names(&declassify.readers), ["Marta"]);
        assert_eq!(
            declassify.sentence,
            "Approving lets Marta read the brief: exactly these bytes (SHA-256 0123456789ab), once. Nothing else of this session reaches them."
        );
    }

    /// R96R-17, UX-DR139: a `run` card draws its execution as keeper bound
    /// it, not as the model asked — the argv as it runs, the folder `cwd`
    /// resolved to, the programs and held files with their SHA-256 — and,
    /// with network, the workspace it releases.
    #[test]
    fn a_run_card_draws_what_keeper_bound() {
        let args = json!({"argv": ["env", "./tool", "fetch"], "cwd": "sel", "network": true});
        let binding = json!({
            "host": "electra",
            "argv": ["env", "./tool", "-c", "core.hooksPath=/dev/null", "fetch"],
            "cwd": "repo-a",
            "env": [],
            "exe": "/usr/bin/env",
            "exe_sha256": "0123456789abcdef".repeat(4),
            "program": {"path": "/w/repo-a/tool", "sha256": "fedcba9876543210".repeat(4)},
            "operands": [{"path": "tool", "inline": null, "sha256": "aa".repeat(32)}],
        });
        let mut value = request("01R", 3, &["once"]);
        value["action"] = json!({"tool": "run", "args": args, "exec_binding": binding});
        value["preconditions"]["files"] = json!([]);
        value["preconditions"]["workspace"] = json!({"sha256": "s", "bytes": 41_300,
            "files": [{"path": "a.txt", "sha256": "b"}, {"path": "tool", "sha256": "c"}]});
        let run = card(sealed(value, &args), &viewer(TGORKA))
            .run
            .expect("a run card");
        assert_eq!(
            run.argv,
            ["env", "./tool", "-c", "core.hooksPath=/dev/null", "fetch"]
        );
        assert_eq!(run.program, "/usr/bin/env · 0123456789ab");
        assert_eq!(
            run.wrapped.as_deref(),
            Some("/w/repo-a/tool · fedcba987654")
        );
        assert_eq!(run.cwd, "workspace/repo-a");
        assert_eq!(run.held, ["tool · aaaaaaaaaaaa"]);
        assert!(run.network.is_some());
        // Another tool draws no run.
        assert_eq!(
            card(request("01A", 2, &["once"]), &viewer(TGORKA)).run,
            None
        );
    }

    /// R96R2-11, R96R3-06, R247: each argument and name on a run card shows
    /// as what it is — empty, holding a newline, a tab, a quote, a
    /// backslash, a bidi override or a trailing space — never raw, and each
    /// distinct from the plain argument it could be taken for; a plain one
    /// shows as it is. Nothing the whole inline card carries to the webview
    /// holds the raw override, while its digest still binds the raw bytes.
    #[test]
    fn a_run_card_shows_each_argument_as_what_it_is() {
        let hostile = [
            "printf",
            "",
            "a\nb",
            "a",
            "b",
            "t\tx",
            "say \"hi\"",
            "back\\slash",
            "\u{202e}gnp.exe",
            "x ",
            "\"\"",
        ];
        let args = json!({"argv": hostile});
        let binding = json!({
            "argv": hostile,
            "cwd": "a\nb",
            "exe": "/w/\u{202e}tool",
            "exe_sha256": "e".repeat(64),
            "operands": [{"path": "dir/\u{2066}x", "sha256": "f".repeat(64)}],
        });
        let mut value = request("01R", 2, &["once"]);
        value["action"] = json!({"tool": "run", "args": args, "exec_binding": binding});
        value["preconditions"]["files"] = json!([]);
        let whole = card(sealed(value, &args), &viewer(TGORKA));
        assert!(whole.can_decide, "the digest binds the raw argv");
        let bidi = |c: char| matches!(c, '\u{202a}'..='\u{202e}' | '\u{2066}'..='\u{2069}');
        let sent = serde_json::to_string(&whole).expect("json");
        assert!(!sent.chars().any(bidi), "{sent}");
        let run = whole.run.expect("a run card");
        let raw = |c: char| c.is_control() || bidi(c);
        for shown in run
            .argv
            .iter()
            .chain([&run.program, &run.cwd])
            .chain(&run.held)
        {
            assert!(!shown.chars().any(raw), "{shown:?}");
        }
        let distinct: BTreeSet<&String> = run.argv.iter().collect();
        assert_eq!(distinct.len(), hostile.len(), "{:?}", run.argv);
        assert_eq!(run.argv[0], "printf");
        assert_eq!(run.argv[3], "a");
        assert_ne!(run.argv[1], "");
        assert_ne!(run.argv[9], "x");
    }

    /// R96R4-05, R260: two inline runs alike but for the drives they ask
    /// to read, how long they may take and the wrapper between, draw
    /// differently — each drive (a hostile name escaped like every other
    /// name), the time it is stopped at (the default when it asked for
    /// none), each wrapper with its hash — and nothing the whole card
    /// carries holds a raw override.
    #[test]
    fn a_run_card_draws_the_drives_it_reads_when_it_stops_and_each_wrapper() {
        let sha = |c: &str| c.repeat(64);
        let run_of = |args: Value, wrappers: Value| {
            let mut binding = json!({
                "host": "electra",
                "argv": args["argv"],
                "cwd": "",
                "env": [],
                "exe": "/usr/bin/env",
                "exe_sha256": sha("e"),
                "program": {"path": "/usr/bin/cargo", "sha256": sha("c")},
                "operands": [],
            });
            if !wrappers.is_null() {
                binding["wrappers"] = wrappers;
            }
            let mut value = request("01R", 2, &["once"]);
            value["action"] = json!({"tool": "run", "args": args, "exec_binding": binding});
            value["preconditions"]["files"] = json!([]);
            let whole = card(sealed(value, &args), &viewer(TGORKA));
            assert!(whole.can_decide, "the digest binds the request");
            let sent = serde_json::to_string(&whole).expect("json");
            assert!(!sent.contains('\u{202e}'), "{sent}");
            whole.run.expect("a run card")
        };
        let argv = json!(["env", "nice", "cargo", "test"]);
        let plain = run_of(json!({"argv": argv}), Value::Null);
        assert_eq!(plain.reads, Vec::<String>::new());
        assert_eq!(plain.timeout_s, crate::agents::run::TIMEOUT_DEFAULT_S);
        assert_eq!(plain.wrappers, Vec::<String>::new());
        let wider = run_of(
            json!({"argv": argv, "read": ["tgdrive", "\u{202e}evird"], "timeout_s": 1800}),
            json!([{"path": "/usr/bin/nice", "sha256": sha("a")}]),
        );
        assert_eq!(wider.reads, ["tgdrive", "\"\\u{202e}evird\""]);
        assert_eq!(wider.timeout_s, 1800);
        assert_eq!(wider.wrappers, ["/usr/bin/nice · aaaaaaaaaaaa"]);
    }

    /// R78 (R4-08): approving for the session names the tool, the drive,
    /// the approved path's folder and the lifetime the allowance has.
    #[test]
    fn a_session_approval_says_what_it_grants() {
        let t2 = card(request("01A", 2, &["once", "session"]), &viewer(TGORKA));
        assert_eq!(
            t2.scopes[1].detail,
            "Also lets this session run `drive_write` again in tgdrive, on anything in `notes/`, without asking, until the session closes and for at most 24 hours."
        );
        let args = json!({"profile": "tgdrive", "path": "plan.md", "content": "hi"});
        let mut top = request("01B", 2, &["once", "session"]);
        top["action"]["args"] = args.clone();
        let top = card(sealed(top, &args), &viewer(TGORKA));
        assert!(top.scopes[1]
            .detail
            .contains("on anything in the top folder"));
    }

    /// R185 (R4-01): a record shows decide buttons only when its digest is
    /// over what its card shows — its arguments, keeper's summary of them,
    /// and every other field the digest binds.
    #[test]
    fn a_card_whose_digest_does_not_bind_what_it_shows_cannot_be_decided() {
        let tgorka = viewer(TGORKA);
        let real = request("01A", 2, &["once"]);
        assert!(card(real.clone(), &tgorka).can_decide);
        let mut args = real.clone();
        args["action"]["args"]["content"] = json!("rm -rf, world");
        let mut summary = real.clone();
        summary["summary"] = json!("Read `notes/plan.md` in tgdrive");
        let mut agent = real.clone();
        agent["agent"] = json!("tola");
        let mut pins = real.clone();
        pins["preconditions"]["files"][0]["landing"] = json!("elsewhere.md");
        let mut tool = real.clone();
        tool["action"]["tool"] = json!("drive_edit");
        for (what, forged) in [
            ("arguments", args),
            ("summary", summary),
            ("agent", agent),
            ("preconditions", pins),
            ("tool", tool),
        ] {
            let shown = card(forged.clone(), &tgorka);
            assert!(!shown.can_decide, "{what}");
            assert_eq!(shown.cannot_decide.as_deref(), Some(UNBOUND), "{what}");
            let mut fold = ApprovalFold::default();
            assert!(apply(
                &mut fold,
                &event(APPROVAL_REQUEST, "$r", NIXI, forged),
                true
            ));
            assert_eq!(
                fold.check(&tgorka, "01A", &digest_of(&real), Scope::Once, now(), &name)
                    .err()
                    .as_deref(),
                Some(UNBOUND),
                "{what}"
            );
        }

        // A coalesced edit may add a row; a row it adds is bound or not on
        // its own.
        let mut fold = ApprovalFold::default();
        let records = |rows: Vec<Value>| json!({"v": 1, "records": rows});
        apply(
            &mut fold,
            &event(APPROVAL_REQUEST, "$gate", NIXI, records(vec![real.clone()])),
            true,
        );
        let mut added = request("01B", 2, &["once"]);
        added["action"]["args"]["path"] = json!("notes/other.md");
        let edit = json!({"m.new_content": records(vec![real, added]),
            "m.relates_to": {"rel_type": "m.replace", "event_id": "$gate"}});
        assert!(apply(
            &mut fold,
            &event(APPROVAL_REQUEST, "$e", NIXI, edit),
            true
        ));
        let cards = &fold.approvals(&tgorka, now(), &name)[0].cards;
        assert!(cards[0].can_decide);
        assert!(!cards[1].can_decide);
        assert_eq!(cards[1].cannot_decide.as_deref(), Some(UNBOUND));
    }

    fn attached(id: &str) -> (Value, Vec<u8>) {
        let args = json!({"profile": "tgdrive", "path": "notes/big.md", "content": "x".repeat(32)});
        let bytes = crate::agents::approval::canonical(&args)
            .expect("canonical")
            .into_bytes();
        let mut request = request(id, 2, &["once"]);
        request["action"]["args"] = Value::Null;
        request["file"] = json!({
            "url": "mxc://example.org/x",
            "key": {"kty": "oct", "key_ops": ["encrypt", "decrypt"], "alg": "A256CTR",
                "k": "aWlpaWlpaWlpaWlpaWlpaWlpaWlpaWlpaWlpaWlpaWk", "ext": true},
            "iv": "aWlpaWlpaWlpaWlpaWlpaQ",
            "hashes": {"sha256": "aWlpaWlpaWlpaWlpaWlpaWlpaWlpaWlpaWlpaWlpaWk"},
            "v": "v2",
        });
        request["file_sha256"] = json!(sha256_hex(&bytes));
        let summary = sealed(request.clone(), &args);
        request["summary"] = summary["summary"].clone();
        request["binding_digest"] = summary["binding_digest"].clone();
        (request, bytes)
    }

    /// R186 (R4-04): an attached action is named, its bytes are checked
    /// against the digest when fetched, and a request whose attachment
    /// cannot be one is never decidable.
    #[test]
    fn an_attached_action_is_shown_only_as_the_digest_binds_it() {
        let tgorka = viewer(TGORKA);
        let (large, bytes) = attached("01E");
        let shown = card(large.clone(), &tgorka);
        assert_eq!(shown.payload, None);
        assert_eq!(
            shown.attachment,
            Some(format!(
                "{ATTACHED} SHA-256 {}.",
                large["file_sha256"].as_str().expect("sha")
            ))
        );
        assert!(shown.can_decide);
        let record = content(large.clone());
        assert_eq!(binding(&record), Binding::Attached);
        let args = verify_attached(&record, &bytes).expect("the attached action");
        assert_eq!(args["path"], "notes/big.md");

        // Other bytes than the file the request names.
        let mut other = bytes.clone();
        other[3] ^= 1;
        assert_eq!(
            verify_attached(&record, &other).err().as_deref(),
            Some(PAYLOAD_REFUSED)
        );
        // The named file, but not the action the digest binds.
        let forged = json!({"profile": "tgdrive", "path": "notes/other.md", "content": "y"});
        let forged = crate::agents::approval::canonical(&forged)
            .expect("canonical")
            .into_bytes();
        let mut renamed = large.clone();
        renamed["file_sha256"] = json!(sha256_hex(&forged));
        assert_eq!(
            verify_attached(&content(renamed), &forged).err().as_deref(),
            Some(PAYLOAD_REFUSED)
        );
        // The action the digest binds, but not the file the request names.
        let mut misnamed = large.clone();
        misnamed["file_sha256"] = json!(sha256_hex(b"other bytes"));
        assert_eq!(
            verify_attached(&content(misnamed), &bytes).err().as_deref(),
            Some(PAYLOAD_REFUSED)
        );
        // Not attached at all.
        assert_eq!(
            verify_attached(&content(request("01A", 2, &["once"])), &bytes)
                .err()
                .as_deref(),
            Some(NOT_ATTACHED)
        );

        let mut malformed = large.clone();
        malformed["file"] = json!({"url": "mxc://example.org/x"});
        let mut unnamed = large.clone();
        unnamed["file_sha256"] = Value::Null;
        let mut no_file = large;
        no_file["file"] = Value::Null;
        for (what, broken) in [
            ("a descriptor that is no encrypted file", malformed),
            ("no SHA-256", unnamed),
            ("null arguments without a file", no_file),
        ] {
            let shown = card(broken, &tgorka);
            assert!(!shown.can_decide, "{what}");
            assert_eq!(shown.cannot_decide.as_deref(), Some(UNBOUND), "{what}");
        }
    }

    /// 93.3 acceptance 5's rules: a device that cannot decide shows no
    /// buttons and says why; only an unverified device points at
    /// verification. A T4 card is judged by its requesting session's room,
    /// not the room it is shown in (R187, R4-07).
    #[test]
    fn a_device_that_cannot_decide_says_why() {
        let unverified = Viewer {
            device_cross_signed: false,
            ..viewer(TGORKA)
        };
        let t2 = card(request("01A", 2, &["once"]), &unverified);
        assert!(!t2.can_decide && t2.verify);
        assert_eq!(t2.cannot_decide.as_deref(), Some(UNVERIFIED));

        let eve = card(request("01A", 2, &["once"]), &viewer(EVE));
        assert!(!eve.can_decide && !eve.verify);
        assert_eq!(eve.cannot_decide.as_deref(), Some(NOT_AN_APPROVER));

        let mut anyone = request("01A", 2, &["once"]);
        anyone["approvers"] = json!([]);
        let anyone = card(anyone, &viewer(EVE));
        assert!(anyone.anyone && anyone.can_decide);

        // This app hosts the requesting session: its T4 card is never
        // decided here, wherever it is shown; a T3 card is.
        let t4 = card(request("01D", 4, &["once"]), &hosting(ROOM));
        assert_eq!(t4.cannot_decide.as_deref(), Some(THIS_DEVICE));
        assert!(!t4.can_decide && !t4.verify);
        assert!(card(request("01C", 3, &["once"]), &hosting(ROOM)).can_decide);
        // This app hosts the proxy DM the card is shown in, not the
        // session that asks: it decides.
        assert!(card(request("01D", 4, &["once"]), &hosting(DM)).can_decide);
        // Two sessions' requests in one DM: each by its own session.
        let mut elsewhere = request("01E", 4, &["once"]);
        elsewhere["room"] = json!("!other:example.org");
        assert!(card(elsewhere, &hosting(ROOM)).can_decide);

        let mut nobody = request("01D", 4, &["once"]);
        nobody["dispatch_chain"] = json!([]);
        assert_eq!(
            card(nobody, &viewer(TGORKA)).cannot_decide.as_deref(),
            Some(NO_REQUESTER)
        );

        let ended = ApprovalCardVm::of(
            &content(request("01A", 2, &["once"])),
            ApprovalStateVm::Expired,
            &unverified,
            &name,
        );
        assert!(!ended.can_decide && !ended.verify && ended.cannot_decide.is_none());
    }

    /// R185 (R4-02/R4-06): only the session's own agent's sealed request
    /// is a card, and only that agent's `consumed` ends it — another agent
    /// at the same power changes nothing.
    #[test]
    fn a_card_counts_only_its_own_agents_sealed_request_and_consumed() {
        let real = request("01A", 2, &["once"]);
        let req = |id: &str, sender: &str| event(APPROVAL_REQUEST, id, sender, real.clone());
        let mut fold = ApprovalFold::default();
        assert!(
            !apply(&mut fold, &req("$p", MARTA), true),
            "a person's look-alike"
        );
        assert!(
            !apply(&mut fold, &req("$t", TOLA), true),
            "another agent at the same power"
        );
        assert!(!apply(&mut fold, &req("$c", NIXI), false), "not sealed");
        let own = me();
        assert!(
            !fold.apply(
                &req("$o", TGORKA),
                true,
                &Senders {
                    own: &own,
                    agent: &|_| true,
                    owner: &|_| true
                }
            ),
            "the own user's"
        );
        assert!(apply(&mut fold, &req("$r", NIXI), true));
        assert!(
            !apply(&mut fold, &req("$again", NIXI), true),
            "the id is listed"
        );
        assert_eq!(states(&fold, now()), [ApprovalStateVm::Pending]);

        assert!(
            !apply(&mut fold, &consumed(MARTA, "01A"), false),
            "a person's"
        );
        apply(&mut fold, &consumed(NIXI, "01B"), false);
        assert_eq!(
            states(&fold, now()),
            [ApprovalStateVm::Pending],
            "another id's consumed"
        );
        apply(&mut fold, &consumed(TOLA, "01A"), false);
        assert_eq!(
            states(&fold, now()),
            [ApprovalStateVm::Pending],
            "another agent's consumed"
        );
        assert!(fold
            .check(
                &viewer(TGORKA),
                "01A",
                &digest_of(&real),
                Scope::Once,
                now(),
                &name
            )
            .is_ok());
        assert!(apply(&mut fold, &consumed(NIXI, "01A"), false));
        assert_eq!(
            states(&fold, now() + Duration::days(2)),
            [ApprovalStateVm::Consumed]
        );
        assert_eq!(fold.next_expiry(now()), None);
    }

    /// R187 (R4-03): a decision in the room is shown but never closes the
    /// card — the host may have ignored it — and nothing decided outlives
    /// the expiry unless the agent used it.
    #[test]
    fn a_decision_in_the_room_leaves_the_card_decidable_until_the_host_acts() {
        let real = request("01A", 2, &["once"]);
        let mut fold = ApprovalFold::default();
        apply(
            &mut fold,
            &event(APPROVAL_REQUEST, "$r", NIXI, real.clone()),
            true,
        );
        let decide =
            |id: &str, sender: &str, content: Value| event(APPROVAL_DECISION, id, sender, content);
        // An agent's, an outsider's, another digest's, an unoffered scope's
        // and an unsealed decision are not shown.
        apply(
            &mut fold,
            &decide("$d1", NIXI, decision(&real, "approve", "once")),
            true,
        );
        apply(
            &mut fold,
            &decide("$d2", EVE, decision(&real, "approve", "once")),
            true,
        );
        let mut other = decision(&real, "approve", "once");
        other["binding_digest"] = json!("sha256:else");
        apply(&mut fold, &decide("$d3", MARTA, other), true);
        apply(
            &mut fold,
            &decide("$d4", MARTA, decision(&real, "approve", "session")),
            true,
        );
        apply(
            &mut fold,
            &decide("$d5", MARTA, decision(&real, "approve", "once")),
            false,
        );
        assert_eq!(states(&fold, now()), [ApprovalStateVm::Pending]);

        // Marta's decision from a device the host does not trust: shown,
        // and Tomasz's verified phone still decides.
        apply(
            &mut fold,
            &decide("$d6", MARTA, decision(&real, "approve", "once")),
            true,
        );
        let approved = ApprovalStateVm::Decided {
            decision: Decision::Approve,
            scope: Scope::Once,
            by: MARTA.to_owned(),
            by_name: "Marta".to_owned(),
        };
        assert_eq!(states(&fold, now()), std::slice::from_ref(&approved));
        let tgorka = viewer(TGORKA);
        let card = &fold.approvals(&tgorka, now(), &name)[0].cards[0];
        assert!(card.can_decide);
        assert!(fold
            .check(&tgorka, "01A", &digest_of(&real), Scope::Once, now(), &name)
            .is_ok());
        // A later decision does not displace the first in the room.
        apply(
            &mut fold,
            &decide("$d7", TGORKA, decision(&real, "deny", "once")),
            true,
        );
        assert_eq!(states(&fold, now()), [approved]);

        // Past the expiry nothing is decidable, a deny included.
        let at = DateTime::parse_from_rfc3339("2026-10-06T12:00:00Z")
            .expect("time")
            .with_timezone(&Utc);
        assert_eq!(fold.next_expiry(now()), Some(at));
        assert_eq!(states(&fold, at), [ApprovalStateVm::Expired]);
        assert_eq!(
            fold.check(&tgorka, "01A", &digest_of(&real), Scope::Once, at, &name)
                .err()
                .as_deref(),
            Some(NOT_WAITING)
        );
        assert_eq!(
            states(&fold, at - Duration::milliseconds(1)).len(),
            1,
            "still open a moment before"
        );
        assert!(matches!(
            states(&fold, at - Duration::milliseconds(1))[0],
            ApprovalStateVm::Decided { .. }
        ));

        // A deny that reaches the room after the expiry ends nothing: the
        // card has expired.
        let mut late = ApprovalFold::default();
        apply(
            &mut late,
            &event(APPROVAL_REQUEST, "$r", NIXI, real.clone()),
            true,
        );
        apply(
            &mut late,
            &decide("$late", TGORKA, decision(&real, "deny", "once")),
            true,
        );
        assert_eq!(
            states(&late, at + Duration::minutes(5)),
            [ApprovalStateVm::Expired]
        );
    }

    /// The first decision in the room's order is the one shown, whatever
    /// order the events reached this device in (R187, R4-05): the fold is
    /// made again from the room's order.
    #[test]
    fn the_shown_decision_is_the_first_in_the_rooms_order() {
        let real = request("01A", 2, &["once"]);
        let req = event(APPROVAL_REQUEST, "$r", NIXI, real.clone());
        let older = event(
            APPROVAL_DECISION,
            "$old",
            MARTA,
            decision(&real, "deny", "once"),
        );
        let newer = event(
            APPROVAL_DECISION,
            "$new",
            TGORKA,
            decision(&real, "approve", "once"),
        );
        let fold_of = |events: &[&Value]| {
            let mut fold = ApprovalFold::default();
            for event in events {
                apply(&mut fold, event, true);
            }
            states(&fold, now())
        };
        // The newest window first, then the older page loaded before it.
        assert!(matches!(
            fold_of(&[&req, &newer])[0],
            ApprovalStateVm::Decided {
                decision: Decision::Approve,
                ..
            }
        ));
        assert!(matches!(
            fold_of(&[&req, &older, &newer])[0],
            ApprovalStateVm::Decided {
                decision: Decision::Deny,
                ..
            }
        ));

        // A coalesced card's edit read before its original: no card alone,
        // and its rows once the original is loaded before it.
        let records = |rows: Vec<Value>| json!({"v": 1, "records": rows});
        let gate = event(APPROVAL_REQUEST, "$gate", NIXI, records(vec![real.clone()]));
        let edit = event(
            APPROVAL_REQUEST,
            "$edit",
            NIXI,
            json!({"m.new_content": records(vec![real.clone(), request("01B", 2, &["once"])]),
                "m.relates_to": {"rel_type": "m.replace", "event_id": "$gate"}}),
        );
        assert!(fold_of(&[&edit]).is_empty(), "an edit alone is no card");
        assert_eq!(fold_of(&[&gate, &edit]).len(), 2);
        assert!(fold_of(&[]).is_empty(), "a cache reset to nothing");
    }

    /// At T4 an approver who did not ask decides nothing, and an agent
    /// never decides, even on a card anyone may decide.
    #[test]
    fn an_agent_never_decides_and_at_t4_only_the_requester_does() {
        let mut open = request("01A", 2, &["once"]);
        open["approvers"] = json!([]);
        let t4 = request("01B", 4, &["once"]);
        let mut fold = ApprovalFold::default();
        apply(
            &mut fold,
            &event(APPROVAL_REQUEST, "$a", NIXI, open.clone()),
            true,
        );
        apply(
            &mut fold,
            &event(APPROVAL_REQUEST, "$b", NIXI, t4.clone()),
            true,
        );
        apply(
            &mut fold,
            &event(
                APPROVAL_DECISION,
                "$d1",
                NIXI,
                decision(&open, "approve", "once"),
            ),
            true,
        );
        apply(
            &mut fold,
            &event(
                APPROVAL_DECISION,
                "$d2",
                MARTA,
                decision(&t4, "approve", "once"),
            ),
            true,
        );
        assert_eq!(
            states(&fold, now()),
            [ApprovalStateVm::Pending, ApprovalStateVm::Pending]
        );
        apply(
            &mut fold,
            &event(
                APPROVAL_DECISION,
                "$d3",
                TGORKA,
                decision(&t4, "approve", "once"),
            ),
            true,
        );
        assert!(matches!(
            states(&fold, now())[1],
            ApprovalStateVm::Decided { .. }
        ));
    }

    /// 93.3 acceptance 10 (R91): a coalesced card lists each record, takes
    /// only its sender's edits that keep every listed record, and decides
    /// each record on its own.
    #[test]
    fn a_coalesced_card_lists_each_record_and_decides_one_at_a_time() {
        let mut fold = ApprovalFold::default();
        let records = |ids: &[&str]| json!({"v": 1, "records": ids.iter().map(|id| request(id, 3, &["once"])).collect::<Vec<_>>()});
        assert!(apply(
            &mut fold,
            &event(APPROVAL_REQUEST, "$gate", NIXI, records(&["01A", "01B"])),
            true
        ));
        let edit = |id: &str, sender: &str, new: Value| {
            event(
                APPROVAL_REQUEST,
                id,
                sender,
                json!({"m.new_content": new, "m.relates_to": {"rel_type": "m.replace", "event_id": "$gate"}}),
            )
        };
        // Another sender's edit is not the card's, even the session's other
        // own agent's.
        let own = me();
        assert!(!fold.apply(
            &edit("$e1", TOLA, records(&["01A", "01B", "01X"])),
            true,
            &Senders {
                own: &own,
                agent: &agent,
                owner: &agent
            }
        ));
        // An edit that changes a listed record's digest, or drops one.
        let mut changed = records(&["01A", "01B", "01Y"]);
        changed["records"][1]["binding_digest"] = json!("sha256:moved");
        assert!(!apply(&mut fold, &edit("$e2", NIXI, changed), true));
        assert!(!apply(
            &mut fold,
            &edit("$e3", NIXI, records(&["01A", "01Z"])),
            true
        ));
        assert!(apply(
            &mut fold,
            &edit("$e4", NIXI, records(&["01A", "01B", "01C"])),
            true
        ));

        let approvals = fold.approvals(&viewer(TGORKA), now(), &name);
        assert_eq!(approvals.len(), 1);
        assert_eq!(approvals[0].id, "01A");
        let ids: Vec<&str> = approvals[0].cards.iter().map(|c| c.id.as_str()).collect();
        assert_eq!(ids, ["01A", "01B", "01C"]);

        apply(
            &mut fold,
            &event(
                APPROVAL_DECISION,
                "$d",
                TGORKA,
                decision(&request("01B", 3, &["once"]), "approve", "once"),
            ),
            true,
        );
        apply(&mut fold, &consumed(NIXI, "01C"), false);
        let cards = &fold.approvals(&viewer(TGORKA), now(), &name)[0].cards;
        assert_eq!(cards[0].state, ApprovalStateVm::Pending);
        assert!(matches!(cards[1].state, ApprovalStateVm::Decided { .. }));
        assert_eq!(cards[2].state, ApprovalStateVm::Consumed);
        assert!(cards[0].can_decide && cards[1].can_decide && !cards[2].can_decide);
    }

    /// What `agent_approval_decide` refuses before anything is sent.
    #[test]
    fn a_decision_is_sent_only_for_the_card_as_shown() {
        let real = request("01A", 3, &["once", "session"]);
        let digest = digest_of(&real);
        let mut fold = ApprovalFold::default();
        apply(&mut fold, &event(APPROVAL_REQUEST, "$a", NIXI, real), true);
        let tgorka = viewer(TGORKA);
        let check = |viewer: &Viewer, id: &str, digest: &str, scope: Scope| {
            fold.check(viewer, id, digest, scope, now(), &name)
                .map(|record| record.id.clone())
        };
        assert_eq!(
            check(&tgorka, "01A", &digest, Scope::Once),
            Ok("01A".to_owned())
        );
        assert_eq!(
            check(&tgorka, "01Q", &digest, Scope::Once),
            Err(NOT_FOUND.to_owned())
        );
        assert_eq!(
            check(&tgorka, "01A", "sha256:x", Scope::Once),
            Err(OTHER_ACTION.to_owned())
        );
        assert_eq!(
            check(&tgorka, "01A", &digest, Scope::Session),
            Err(SCOPE_NOT_OFFERED.to_owned())
        );
        assert_eq!(
            check(&viewer(EVE), "01A", &digest, Scope::Once),
            Err(NOT_AN_APPROVER.to_owned())
        );
        let unverified = Viewer {
            device_cross_signed: false,
            ..tgorka.clone()
        };
        assert_eq!(
            check(&unverified, "01A", &digest, Scope::Once),
            Err(UNVERIFIED.to_owned())
        );
    }
}
