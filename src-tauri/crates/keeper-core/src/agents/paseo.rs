//! Coding through Paseo (AD-407, FR-811): what keeper makes of a
//! `role = "paseo"` MCP server — makistack's broker, whose four verbs start,
//! steer and report on coding runs whose product is a pull request.
//!
//! Pure. The broker's verbs are classified by four fixed rows ([`row`]) and
//! nothing else it lists is offered; the two that start or steer work are a
//! person's decision every time, their card's sentence keeper's
//! ([`summary`]) naming who the prompt reaches, and the approval the
//! declassification of exactly the prompt to exactly that audience
//! ([`reach`], [`bind_audience`]). The two reads carry no byte of the
//! session: an `agentId` is polled only when this session's own successful
//! `create_agent` on the same broker was answered with it — a [`Started`]
//! record the host writes on that result's log line — or when the session
//! is the one keeper made to follow it ([`followed`]). Every answer is shown,
//! logged and recorded only as its projection ([`answer`]), made from the
//! whole answer before anything is cut for display: the broker's eight
//! fields, credential-bearing values withheld, `prUrl` only as its
//! canonical link with no credential in its query; an answer of any other
//! shape is not passed on at all ([`UNREAD`]). A run is followed by a
//! scheduled card the host makes ([`follow_session`], [`follow_card`]) and,
//! once its status is terminal ([`is_terminal`]), recorded in
//! `artifacts/paseo-<agentId>.md` ([`artifact`]) and told to the
//! conversation that started it under one completion ([`completion_id`],
//! [`COMPLETION`]).

use std::collections::BTreeSet;

use matrix_sdk::ruma::{OwnedRoomId, UserId};
use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};
use ulid::Ulid;

use crate::agents::label::{Label, Readers};
use crate::agents::redact::redact_secrets;
use crate::agents::run::shown;
use crate::agents::session::{SessionAgent, SessionKind, SessionReply};
use crate::agents::tier::Tier;

/// The broker's verbs (`paseo-mcp.py` `VERBS`, asserted there at import).
pub const VERBS: [&str; 4] = [
    "create_agent",
    "get_agent_status",
    "list_agents",
    "send_agent_prompt",
];

/// The only keys any verb returns (`paseo-mcp.py` `RESPONSE_FIELDS`): the
/// artifact holds these and nothing else.
pub const FIELDS: [&str; 8] = [
    "agentId",
    "workspaceId",
    "title",
    "status",
    "provider",
    "createdAt",
    "updatedAt",
    "prUrl",
];

/// Why any other tool the broker lists is not offered.
pub const FOUR_VERBS: &str = "the broker has exactly four verbs";

/// How often a follow card asks for its run's status.
pub const FOLLOW_SCHEDULE: &str = "every 10m";

/// The follow session's one card.
pub const FOLLOW_CARD: &str = "follow.md";

/// What a value the broker answered becomes when it bears a credential.
pub const WITHHELD: &str = "withheld: it carries a credential";

/// What a link that bears a credential becomes inside a longer text.
const WITHHELD_LINK: &str = "[a link with a credential, withheld]";

/// What the model and the log read in place of a successful answer that is
/// not the broker's `agent` or `agents` record — of another shape, or not
/// JSON: none of its words (R96PA2-01).
pub const UNREAD: &str = "keeper does not pass on this answer of Paseo's: it is not the broker's `agent` or `agents` record, so none of it is shown.";

/// The names of a link's query or fragment parameters that carry a
/// credential: a parameter whose decoded name, read without case and with
/// its punctuation dropped (`access_token`, `X-Amz-Signature`), is one of
/// these or ends with one makes the link one keeper does not present.
pub const CREDENTIAL_PARAMS: [&str; 12] = [
    "token",
    "auth",
    "authorization",
    "key",
    "secret",
    "password",
    "passwd",
    "pwd",
    "sig",
    "signature",
    "credential",
    "credentials",
];

/// The schemes whose URLs the URL parser reads an authority in however many
/// `/` and `\` — none included — follow the `:`, and whose authority a `\`
/// ends too. `file` is special as well, but its URLs carry no user or
/// password: one written with `//` is read as any other scheme's.
const AUTHORITY_SCHEMES: [&str; 5] = ["http", "https", "ws", "wss", "ftp"];

/// The key of a run's end notice that says which completion it is and which
/// session it answers (R96PA2-05): what lets a delegating session take it
/// beside the delegation's own reply, once.
pub const COMPLETION: &str = "dev.keeper.agent.paseo_completion";

/// What a follow session's title starts with; the run's id follows.
const FOLLOW_TITLE: &str = "paseo-";

/// The statuses after which a run does nothing more by itself. Paseo's own
/// vocabulary is not documented beside the broker, so the set is closed and
/// read without case; any other status is still running.
pub const TERMINAL: [&str; 13] = [
    "completed",
    "complete",
    "done",
    "finished",
    "succeeded",
    "failed",
    "error",
    "errored",
    "cancelled",
    "canceled",
    "stopped",
    "closed",
    "archived",
];

/// The broker's verb `tool` at its fixed tier (R24(2)): the two reads T0,
/// the two that start or steer work T3; `None` for a fifth.
pub fn row(tool: &str) -> Option<Tier> {
    match tool {
        "list_agents" | "get_agent_status" => Some(Tier::T0),
        "create_agent" | "send_agent_prompt" => Some(Tier::T3),
        _ => None,
    }
}

/// Whether `tool` starts or steers a coding run: a send of its prompt to
/// whoever reads the repository the work lands in.
pub fn mutates(tool: &str) -> bool {
    matches!(tool, "create_agent" | "send_agent_prompt")
}

/// A run the broker identified by `broker` started for a session: what the
/// host writes on the log line of a successful `create_agent` result, and
/// the only thing a later poll's authority is read from (Q14).
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Started {
    /// [`broker`] of the connection the call was sent on.
    pub broker: String,
    /// The `agentId` the broker answered, as it answered it.
    pub id: String,
}

/// Which broker a connection reached: the SHA-256 of its MCP identity
/// (R144, R238) in canonical JSON — its name, role, endpoint or program and
/// pin. Another endpoint behind the same name is another broker.
pub fn broker(identity: &Value) -> String {
    let canonical =
        crate::agents::approval::canonical(identity).unwrap_or_else(|_| identity.to_string());
    crate::agents::approval::sha256_hex(canonical.as_bytes())
}

/// An `agentId` as the broker accepts one (`SAFE_ID`,
/// `^[A-Za-z0-9][A-Za-z0-9._:-]{0,127}$`) that is not shaped like a secret,
/// so it may be shown, logged and named.
pub fn safe_id(id: &str) -> bool {
    let bytes = id.as_bytes();
    !bytes.is_empty()
        && bytes.len() <= 128
        && bytes[0].is_ascii_alphanumeric()
        && bytes
            .iter()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'_' | b':' | b'-'))
        && redact_secrets(id).text == id
}

/// The name an `agentId` is stored under on a drive: the id with each `:`
/// written `~`, which the broker's grammar never holds, so the name is
/// portable to every volume a drive syncs to and still one run's alone.
pub fn stored(id: &str) -> String {
    id.replace(':', "~")
}

/// Who a mutation's prompt goes to, as an approval binds it: `"*"` or the
/// readers' ids.
fn audience_of(readers: &Value) -> String {
    match readers {
        Value::String(star) if star == "*" => "anyone who reads the broker".to_owned(),
        Value::Array(users) => {
            let named: Vec<String> = users
                .iter()
                .filter_map(Value::as_str)
                .map(|user| format!("`{}`", shown(user)))
                .collect();
            if named.is_empty() {
                "nobody named".to_owned()
            } else {
                named.join(", ")
            }
        }
        _ => "an audience keeper cannot name".to_owned(),
    }
}

/// The card's sentence for a call to the broker's `tool` (96.3 #2, S-10):
/// keeper's words from the arguments it is about to send — the workspace or
/// the run — never the prompt, which is the payload the card shows; for a
/// mutation, who the prompt goes to as the approval binds it (`readers`,
/// R96PA-04), and for a start, that following it waits for a person (Q12).
pub fn summary(tool: &str, args: &Value, readers: Option<&Value>) -> Option<String> {
    let named = |key: &str| args.get(key).and_then(Value::as_str).map(shown);
    let to = readers
        .map(audience_of)
        .unwrap_or_else(|| audience_of(&Value::Null));
    match tool {
        "create_agent" => Some(format!(
            "Start a coding run through Paseo in {}; the prompt goes to {to}; following the run waits for a person to allow its follow card",
            match named("workspace") {
                Some(workspace) => format!("`{workspace}`"),
                None => "the broker's current workspace".to_owned(),
            }
        )),
        "send_agent_prompt" => Some(format!(
            "Send a prompt to Paseo run `{}`; the prompt goes to {to}",
            named("agentId").unwrap_or_default()
        )),
        "list_agents" => Some("List Paseo's coding runs".to_owned()),
        "get_agent_status" => Some(format!(
            "Ask Paseo for the status of run `{}`",
            named("agentId").unwrap_or_default()
        )),
        _ => None,
    }
}

/// `binding` — an MCP call's `exec_binding` — with the audience a Paseo
/// mutation's prompt goes to, `readers`: an approval releases the prompt to
/// that audience only, so one that reaches another is drift (R96PA-04). Any
/// other binding is left as it is.
pub fn bind_audience(binding: &mut Value, readers: &Readers) {
    let mutation = binding["role"] == "paseo" && binding["tool"].as_str().is_some_and(mutates);
    if mutation {
        binding["readers"] = serde_json::to_value(readers).unwrap_or(Value::Null);
    }
}

/// What a call of the broker's `tool` with `args` sends of the session
/// (96.3 #7, AD-391, R143/Q5).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Reach {
    /// No byte of the session: it passes whoever reads the broker.
    Nothing,
    /// The prompt, to whoever reads the repository: its T3 approval is the
    /// declassification of exactly that prompt.
    Prompt,
    /// Neither: blocked, not asked, for this reason.
    Blocked(String),
}

/// What `tool` with `args` sends, `polled` being the runs this session may
/// ask this broker about: `list_agents` with no argument, and
/// `get_agent_status` with only the `agentId` of one of `polled`, send
/// nothing of the session.
pub fn reach(tool: &str, args: &Map<String, Value>, polled: &BTreeSet<String>) -> Reach {
    match tool {
        "list_agents" if args.is_empty() => Reach::Nothing,
        "list_agents" => Reach::Blocked(
            "`list_agents` takes no argument, and an argument would carry the session's words to the broker.".to_owned(),
        ),
        "get_agent_status" => match args.get("agentId").and_then(Value::as_str) {
            Some(id) if args.len() == 1 && polled.contains(id) => Reach::Nothing,
            Some(id) if args.len() == 1 => Reach::Blocked(format!(
                "Paseo run `{}` was not started on this broker by this session's own `create_agent`, so its status is not asked for.",
                shown(&sanitized(id))
            )),
            _ => Reach::Blocked(
                "`get_agent_status` takes exactly one `agentId`, which this session's own `create_agent` was given.".to_owned(),
            ),
        },
        tool if mutates(tool) => Reach::Prompt,
        _ => Reach::Blocked(format!("{FOUR_VERBS}.")),
    }
}

/// One run as the broker answered it: its fields among [`FIELDS`], each a
/// scalar, in that order, a value that bears a credential [`WITHHELD`] and
/// `prUrl` its canonical link or withheld; anything else it held is
/// dropped.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Projected {
    fields: Vec<(&'static str, String)>,
}

impl Projected {
    fn get(&self, key: &str) -> Option<&str> {
        self.fields
            .iter()
            .find(|(field, _)| *field == key)
            .map(|(_, value)| value.as_str())
    }

    /// Its `agentId`, when the broker answered one keeper may show and name.
    pub fn id(&self) -> Option<&str> {
        self.get("agentId").filter(|id| safe_id(id))
    }

    pub fn status(&self) -> Option<&str> {
        self.get("status")
    }

    /// Its pull request's canonical link, when it has one ([`link`]).
    pub fn pr_url(&self) -> Option<&str> {
        self.get("prUrl").filter(|link| *link != WITHHELD)
    }

    /// The keys it holds, in [`FIELDS`] order.
    pub fn keys(&self) -> Vec<&'static str> {
        self.fields.iter().map(|(key, _)| *key).collect()
    }

    fn json(&self) -> Value {
        Value::Object(
            self.fields
                .iter()
                .map(|(key, value)| ((*key).to_owned(), Value::String(value.clone())))
                .collect(),
        )
    }
}

/// `raw` as a pull request's link keeper may present: an `http(s)` URL with
/// a host, no credential — no user or password, no secret-shaped part, no
/// query or fragment parameter named for a credential
/// ([`CREDENTIAL_PARAMS`]), and no link inside its query or fragment that
/// carries one, as every other field kept is read ([`bears_credential`],
/// R287) — written as `url` serializes it, never the broker's own string,
/// and only when that string holds no control, whitespace, angle bracket,
/// quote, backtick or backslash, each of which could make the link mean
/// other than it shows.
pub fn link(raw: &str) -> Option<String> {
    if raw
        .chars()
        .any(|c| c.is_control() || c.is_whitespace() || matches!(c, '<' | '>' | '"' | '`' | '\\'))
        || bears_credential(raw)
    {
        return None;
    }
    let url = url::Url::parse(raw).ok()?;
    let plain = matches!(url.scheme(), "http" | "https")
        && url.host_str().is_some()
        && url.username().is_empty()
        && url.password().is_none()
        && !names_a_credential(&url);
    let canonical = url.as_str();
    (plain
        && redact_secrets(canonical).text == canonical
        && !canonical.contains(['<', '>', '`'])
        && !canonical.chars().any(char::is_whitespace))
    .then(|| canonical.to_owned())
}

/// Whether `url`'s query or fragment, parsed and decoded as form
/// parameters, has one named for a credential.
fn names_a_credential(url: &url::Url) -> bool {
    let fragment = url
        .fragment()
        .map(|fragment| url::form_urlencoded::parse(fragment.as_bytes()));
    url.query_pairs()
        .chain(fragment.into_iter().flatten())
        .any(|(name, _)| credential_name(&name))
}

/// Whether a decoded parameter name is one of [`CREDENTIAL_PARAMS`] or ends
/// with one, read without case and with its punctuation dropped.
fn credential_name(name: &str) -> bool {
    let name: String = name
        .chars()
        .filter(char::is_ascii_alphanumeric)
        .map(|c| c.to_ascii_lowercase())
        .collect();
    CREDENTIAL_PARAMS.iter().any(|param| name.ends_with(param))
}

/// Whether `text` holds a link that carries a credential: a user or a
/// password, or a query or fragment parameter named for one.
fn bears_credential(text: &str) -> bool {
    withhold_credentials(text) != text
}

/// Where in `plain` — a text with its tabs and newlines dropped, as the URL
/// parser drops them — the link starts whose query or fragment has a
/// parameter named for a credential ([`credential_name`]), decoded as form
/// parameters. A link's query is read as the URL parser reads it, to the
/// text's end — a space does not end it — and a link inside it names its
/// own parameters, so every `?`, `#` and `&` after the first link's
/// authority starts a parameter; the link it belongs to is the last one
/// that starts before it (R283).
fn credential_parameter(plain: &str) -> Option<usize> {
    let (first, authority, _) = next_link(plain, 0)?;
    let mut starts = vec![first];
    let mut next = authority;
    while next < plain.len() {
        let Some((start, authority, _)) = next_link(plain, next) else {
            break;
        };
        starts.push(start);
        next = authority;
    }
    let mut at = authority + plain[authority..].find(['?', '#'])?;
    while at < plain.len() {
        let name_at = at + 1;
        let end = plain[name_at..]
            .find(['?', '#', '&'])
            .map_or(plain.len(), |end| name_at + end);
        let named = url::form_urlencoded::parse(&plain.as_bytes()[name_at..end])
            .next()
            .is_some_and(|(name, _)| credential_name(&name));
        if named {
            return starts.iter().rev().find(|start| **start < name_at).copied();
        }
        at = end;
    }
    None
}

/// Where the scheme of the `:` at `at` in `text` starts: the run of scheme
/// characters before it, from its first letter on.
fn scheme_start(text: &str, at: usize) -> usize {
    let run = text[..at]
        .char_indices()
        .rev()
        .take_while(|(_, c)| c.is_ascii_alphanumeric() || matches!(c, '+' | '.' | '-'))
        .last()
        .map_or(at, |(index, _)| index);
    text[run..at]
        .find(|c: char| c.is_ascii_alphabetic())
        .map_or(at, |letter| run + letter)
}

/// The next link of `text` at or after `from` the URL parser reads an
/// authority in: where its scheme starts, where its authority starts, and
/// whether `\` ends that authority. A special scheme's authority follows
/// its `:` past any number of `/` and `\`; any other scheme's follows `://`.
fn next_link(text: &str, from: usize) -> Option<(usize, usize, bool)> {
    let mut search = from;
    while let Some(found) = text[search..].find(':') {
        let colon = search + found;
        let start = scheme_start(text, colon);
        let rest = &text[colon + 1..];
        if start < colon {
            let scheme = &text[start..colon];
            if AUTHORITY_SCHEMES
                .iter()
                .any(|known| known.eq_ignore_ascii_case(scheme))
            {
                let slashes = rest.len() - rest.trim_start_matches(['/', '\\']).len();
                return Some((start, colon + 1 + slashes, true));
            }
            if rest.starts_with("//") {
                return Some((start, colon + 3, false));
            }
        }
        search = colon + 1;
    }
    None
}

/// Whether `link` — a scheme and an authority holding `@` — has a user or
/// a password as the URL parser reads it, or cannot be read: what cannot be
/// decided is withheld too.
fn credentialed(link: &str) -> bool {
    url::Url::parse(link).map_or(true, |url| {
        !url.username().is_empty() || url.password().is_some()
    })
}

/// The first position at or after `from` in `text` that `stops` — found
/// once, then reused while the positions asked for do not pass it, so a
/// scan whose positions only grow reads each byte once.
struct NextStop<F: Fn(char) -> bool> {
    stops: F,
    found: Option<(usize, Option<usize>)>,
}

impl<F: Fn(char) -> bool> NextStop<F> {
    fn new(stops: F) -> Self {
        NextStop { stops, found: None }
    }

    fn at(&mut self, text: &str, from: usize) -> Option<usize> {
        if let Some((asked, at)) = self.found {
            if asked <= from && at.is_none_or(|at| at >= from) {
                return at;
            }
        }
        let at = text[from..].find(&self.stops).map(|found| from + found);
        self.found = Some((from, at));
        at
    }
}

/// `text` with every link whose authority carries a credential replaced
/// from its scheme to its authority's end: a credential is withheld
/// wherever it stands, not only when it has a known token's shape. Links
/// are found as the URL parser reads them — after it drops every tab and
/// newline; a special scheme's authority after any number of `/` and `\`,
/// none included, another's after `://` ([`next_link`]) — and an authority
/// runs to the first `/`, `?` or `#` (`\` too after a special scheme),
/// never ending at a quote or a space; one holding `@` that the parser
/// cannot read is withheld as well. A link inside an authority that is
/// kept is read too (R96PA2-02, R277). A link whose query or fragment has
/// a parameter named for a credential is withheld from its scheme to the
/// text's end, where the parser's reading of that query ends
/// ([`credential_parameter`], R283).
fn withhold_credentials(text: &str) -> String {
    // The text without tabs and newlines, and where each of its bytes
    // stands in `text`.
    let mut plain = String::with_capacity(text.len());
    let mut from = Vec::with_capacity(text.len() + 1);
    for (index, c) in text.char_indices() {
        if !matches!(c, '\t' | '\n' | '\r') {
            plain.push(c);
            from.extend(index..index + c.len_utf8());
        }
    }
    from.push(text.len());
    let mut special_end = NextStop::new(|c: char| matches!(c, '/' | '?' | '#' | '\\'));
    let mut other_end = NextStop::new(|c: char| matches!(c, '/' | '?' | '#'));
    let mut at_sign = NextStop::new(|c: char| c == '@');
    let mut withheld: Vec<(usize, usize)> = Vec::new();
    let mut next = 0;
    while let Some((start, authority, special)) = next_link(&plain, next) {
        let end = if special {
            special_end.at(&plain, authority)
        } else {
            other_end.at(&plain, authority)
        }
        .unwrap_or(plain.len());
        let holds_at = at_sign.at(&plain, authority).is_some_and(|at| at < end);
        if holds_at && credentialed(&plain[start..end]) {
            withheld.push((from[start], from[end]));
            next = end;
        } else {
            next = authority;
        }
        if next >= plain.len() {
            break;
        }
    }
    let mut tail = credential_parameter(&plain).map(|at| from[at]);
    let mut out = String::with_capacity(text.len());
    let mut kept = 0;
    for (start, end) in withheld {
        if let Some(at) = tail {
            if start >= at {
                break;
            }
            if end > at {
                tail = Some(start);
                break;
            }
        }
        out.push_str(&text[kept..start]);
        out.push_str(WITHHELD_LINK);
        kept = end;
    }
    match tail {
        Some(at) => {
            out.push_str(&text[kept..at]);
            out.push_str(WITHHELD_LINK);
        }
        None => out.push_str(&text[kept..]),
    }
    out
}

/// `text` from the broker as keeper passes on an error: every secret-shaped
/// run redacted and every link with a credential withheld.
pub fn sanitized(text: &str) -> String {
    withhold_credentials(&redact_secrets(text).text)
}

/// A broker error's structured `data` as keeper passes it on: every string
/// in it — each key too — [`sanitized`] as the text it is, before the value
/// is serialized, where a tab or a backslash in a link would otherwise be
/// read as an escape (R277).
pub fn sanitized_value(value: &Value) -> Value {
    match value {
        Value::String(text) => Value::String(sanitized(text)),
        Value::Array(items) => Value::Array(items.iter().map(sanitized_value).collect()),
        Value::Object(object) => Value::Object(
            object
                .iter()
                .map(|(key, value)| (sanitized(key), sanitized_value(value)))
                .collect(),
        ),
        other => other.clone(),
    }
}

/// One field's value as keeper keeps it.
fn kept(key: &str, value: &str) -> String {
    if key == "prUrl" {
        return link(value).unwrap_or_else(|| WITHHELD.to_owned());
    }
    if redact_secrets(value).text != value || bears_credential(value) {
        return WITHHELD.to_owned();
    }
    value.to_owned()
}

/// The run the broker's record `agent` is, projected.
fn projected(agent: &Map<String, Value>) -> Projected {
    let fields = FIELDS
        .iter()
        .filter_map(|key| {
            let value = match agent.get(*key)? {
                Value::String(text) => text.clone(),
                Value::Number(number) => number.to_string(),
                Value::Bool(flag) => flag.to_string(),
                _ => return None,
            };
            Some((*key, kept(key, &value)))
        })
        .collect();
    Projected { fields }
}

/// A broker's answer as keeper passes it on (96.3 #6): what the model, the
/// log and anything after them read, and the runs it named.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Answer {
    pub text: String,
    pub runs: Vec<Projected>,
}

/// A successful answer's whole `text`, as the transport bounded it — never
/// a display's cut of it — as keeper passes it on: the broker's
/// `{"agent": {…}}` or `{"agents": [{…}, …]}` rebuilt from the projected
/// runs alone; anything else, unknown or not JSON, is [`UNREAD`] and names
/// no run.
pub fn answer(text: &str) -> Answer {
    let unread = || Answer {
        text: UNREAD.to_owned(),
        runs: Vec::new(),
    };
    let Ok(Value::Object(object)) = serde_json::from_str::<Value>(text.trim()) else {
        return unread();
    };
    if let Some(Value::Object(agent)) = object.get("agent") {
        let run = projected(agent);
        return Answer {
            text: json!({"agent": run.json()}).to_string(),
            runs: vec![run],
        };
    }
    if let Some(Value::Array(agents)) = object.get("agents") {
        let runs: Vec<Projected> = agents
            .iter()
            .filter_map(Value::as_object)
            .map(projected)
            .collect();
        let shown: Vec<Value> = runs.iter().map(Projected::json).collect();
        return Answer {
            text: json!({"agents": shown}).to_string(),
            runs,
        };
    }
    unread()
}

/// Whether `status` ends a run (read without case).
pub fn is_terminal(status: &str) -> bool {
    TERMINAL
        .iter()
        .any(|terminal| terminal.eq_ignore_ascii_case(status.trim()))
}

/// Where a run's record lands in its follow session.
pub fn artifact_path(id: &str) -> String {
    format!(
        "{}/paseo-{}.md",
        crate::sessions::model::ARTIFACTS_DIR,
        stored(id)
    )
}

/// The record of an ended run (96.3 #5, #6): the broker's eight fields as
/// projected and nothing else, one line each, `prUrl` a link only as its
/// canonical form. The host stamps it as written from outside content.
pub fn artifact(run: &Projected) -> String {
    let id = run.id().unwrap_or_default();
    let mut out = format!("---\ntitle: \"Paseo run {id}\"\n---\n\n# Paseo run `{id}`\n\n");
    for (key, value) in &run.fields {
        let line = match (*key, run.pr_url()) {
            ("prUrl", Some(link)) => format!("- prUrl: <{link}>\n"),
            ("prUrl", None) => format!("- prUrl: {WITHHELD}, or not a plain http(s) link\n"),
            _ => format!("- {key}: `{}`\n", shown(value)),
        };
        out.push_str(&line);
    }
    out
}

/// What the conversation that started the run is told when it ended.
pub fn ended(run: &Projected) -> String {
    let id = run.id().unwrap_or_default();
    let status = shown(run.status().unwrap_or_default());
    match run.pr_url() {
        Some(link) => format!(
            "Paseo run `{id}` ended `{status}`. Its pull request: <{link}> — recorded in {}.",
            artifact_path(id)
        ),
        None => format!(
            "Paseo run `{id}` ended `{status}` with no pull request link — recorded in {}.",
            artifact_path(id)
        ),
    }
}

/// The id of the session following run `id` that `broker` started for
/// `agent` in `drive`: the same on every host, so a run is followed once,
/// and another broker's run of the same id another session.
pub fn follow_session_id(drive: &str, agent: &str, broker: &str, id: &str) -> Ulid {
    crate::agents::seed::derived_session_id(drive, agent, &format!("paseo:{broker}:{id}"))
}

/// The run `agent`'s session follows on `broker`, when it is the follow
/// session keeper made for it: a scheduled session whose `[reply]` names
/// the run whole — never read back from its title, which may be cut — and
/// whose id is the one [`follow_session_id`] derives for that broker and
/// run; no other session, and no other broker, names one.
pub fn followed<'a>(agent: &'a SessionAgent, broker: &str) -> Option<&'a str> {
    let id = agent.reply.as_ref()?.run.as_deref()?;
    (agent.kind == SessionKind::Scheduled
        && safe_id(id)
        && agent.id == follow_session_id(&agent.drive, &agent.agent, broker, id))
    .then_some(id)
}

/// A follow session's title: `paseo-` and its run's id, cut with `…` to a
/// session title's length when the id is longer (R96PA2-06). Only what the
/// session is shown as: its run is the one its `[reply]` names whole.
pub fn follow_title(id: &str) -> String {
    let title = format!("{FOLLOW_TITLE}{id}");
    if title.chars().count() <= crate::agents::session::TITLE_MAX {
        return title;
    }
    let mut cut: String = title
        .chars()
        .take(crate::agents::session::TITLE_MAX - 1)
        .collect();
    cut.push('…');
    cut
}

/// The session that follows run `id` of `broker`, made by the host of
/// `from`'s session — the one at `from_path` in its zone — after its
/// approved `create_agent` (Q14, R241): the same agent, drive, requester
/// and chain, `kind = scheduled`, no parent, labelled `label` — the
/// session's own joined with the broker's answer — and answering `from`'s
/// conversation, for the run it names, when the run ends (R96PA-10).
pub fn follow_session(
    from: &SessionAgent,
    from_path: &str,
    label: Label,
    broker: &str,
    id: &str,
    room: OwnedRoomId,
    now: chrono::DateTime<chrono::Utc>,
) -> SessionAgent {
    SessionAgent {
        id: follow_session_id(&from.drive, &from.agent, broker, id),
        agent: from.agent.clone(),
        drive: from.drive.clone(),
        kind: SessionKind::Scheduled,
        title: follow_title(id),
        requested_by: from.requested_by.clone(),
        parent: None,
        reply: Some(SessionReply {
            session: from_path.to_owned(),
            room: from.room.clone(),
            run: Some(id.to_owned()),
        }),
        room,
        drives: vec![from.drive.clone()],
        label,
        needs: None,
        pin: None,
        hop: from.hop,
        dispatch_chain: from.dispatch_chain.clone(),
        limits: None,
        workflow: None,
        checkpoints: None,
        outputs: Vec::new(),
        created_at: now,
    }
}

/// The completion of the follow session `session`: the transaction id its
/// end notice is sent under, the same however often and from whichever of
/// the principal's hosts it is sent (R96PA-09).
pub fn completion_id(session: &Ulid) -> String {
    format!("paseo-ended-{session}")
}

/// `content`, a run's end notice, marked as completion `completion` that
/// answers the session whose id is `origin` ([`COMPLETION`]).
pub fn mark_completion(content: &mut Value, completion: &str, origin: &str) {
    content[COMPLETION] = json!({"completion": completion, "session": origin});
}

/// The completion a run's end notice `content` carries, and the id of the
/// session it answers.
pub fn completion_of(content: &Value) -> Option<(&str, &str)> {
    let marked = content.get(COMPLETION)?;
    Some((
        marked.get("completion")?.as_str()?,
        marked.get("session")?.as_str()?,
    ))
}

/// The state event a follow session's own room holds, keyed by its
/// completion id, once a run's end is captured: a lookup index only, never
/// the authority (R279). It says a capture was published — the host
/// places such a session without its broker, and a host whose checkout
/// holds no capture reads the room's timeline before it polls — but which
/// capture binds the end, and whether it was delivered, only the timeline
/// says ([`authority`]): state is replaceable, and a stale write that lands
/// late would otherwise stand for the newest.
pub const CAPTURED: &str = "dev.keeper.agent.paseo_captured";

/// What [`CAPTURED`] says, sent by the session's agent alone.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Captured {
    pub v: u32,
    pub completion: String,
    /// The SHA-256 of the capture's bytes ([`capture_bytes`]).
    pub digest: String,
    /// The message of the room that holds those bytes as an encrypted
    /// file: only the room's own devices read them.
    pub capture: matrix_sdk::ruma::OwnedEventId,
}

/// The key of a follow session's own-room message that publishes a capture
/// of a run's end beside its encrypted `file`: `{"completion", "digest"}`.
pub const CAPTURE: &str = "dev.keeper.agent.paseo_capture";

/// The key of a follow session's own-room message that says a capture's
/// notice was delivered: `{"completion", "capture", "notice"}`.
pub const DELIVERED: &str = "dev.keeper.agent.paseo_delivered";

/// The message that publishes the capture of `completion`, whose bytes
/// hash to `digest`, as the encrypted file `file`.
pub fn capture_message(completion: &str, digest: &str, file: Value) -> Value {
    json!({
        "msgtype": "m.file",
        "body": format!("{completion}.json"),
        "file": file,
        CAPTURE: {"completion": completion, "digest": digest},
    })
}

/// The message that says the notice of `completion` bound to the capture
/// `capture` was delivered as `notice`.
pub fn delivered_message(
    completion: &str,
    capture: &matrix_sdk::ruma::EventId,
    notice: &matrix_sdk::ruma::EventId,
) -> Value {
    json!({
        "msgtype": "m.notice",
        "body": "The run's end was told to the conversation that started it.",
        DELIVERED: {"completion": completion, "capture": capture, "notice": notice},
    })
}

/// Whether `content` publishes a capture of `completion` or says it was
/// delivered.
pub fn marks(content: &Value, completion: &str) -> bool {
    [CAPTURE, DELIVERED]
        .iter()
        .any(|key| content[*key]["completion"].as_str() == Some(completion))
}

/// The capture that binds a run's end: its message, the digest it names,
/// and its encrypted file.
#[derive(Debug, Clone, PartialEq)]
pub struct Binding {
    pub event: matrix_sdk::ruma::OwnedEventId,
    pub digest: String,
    pub file: Value,
}

/// What a follow session's own room establishes of a completion.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Authority {
    /// The first capture its agent published.
    pub capture: Option<Binding>,
    /// The notice a delivery message of its agent names.
    pub delivered: Option<matrix_sdk::ruma::OwnedEventId>,
}

/// What `events` — a follow session's own room's sealed messages marked for
/// `completion` ([`marks`]), oldest first, as the homeserver ordered them —
/// establish when `me` sent them (R279). The capture that binds the end is
/// the FIRST: the timeline only grows, so no write that lands later, from
/// whichever host and however delayed, replaces it. Delivery is appended
/// too, so once any delivery message is there the end is delivered for
/// good. Anyone else's message counts for nothing.
pub fn authority(events: &[Value], me: &UserId, completion: &str) -> Authority {
    let mut found = Authority::default();
    for event in events
        .iter()
        .filter(|event| event["sender"].as_str() == Some(me.as_str()))
    {
        let content = &event["content"];
        let capture = &content[CAPTURE];
        if found.capture.is_none() && capture["completion"].as_str() == Some(completion) {
            found.capture = (|| {
                Some(Binding {
                    event: event["event_id"].as_str()?.try_into().ok()?,
                    digest: capture["digest"].as_str()?.to_owned(),
                    file: content.get("file")?.clone(),
                })
            })();
        }
        let delivered = &content[DELIVERED];
        if found.delivered.is_none() && delivered["completion"].as_str() == Some(completion) {
            found.delivered = delivered["notice"]
                .as_str()
                .and_then(|notice| notice.try_into().ok());
        }
    }
    found
}

/// The bytes a capture is kept as wherever it travels: its `pending` line's
/// body as canonical JSON — the completion, the starting room, the
/// record's path, bytes and SHA-256, and the notice.
pub fn capture_bytes(captured: &crate::agents::log::PaseoBody) -> Vec<u8> {
    let pending = crate::agents::log::PaseoBody {
        state: crate::agents::log::PaseoState::Pending,
        event: None,
        ..captured.clone()
    };
    let value = serde_json::to_value(&pending).unwrap_or(Value::Null);
    crate::agents::approval::canonical(&value)
        .unwrap_or_else(|_| value.to_string())
        .into_bytes()
}

/// The capture of `completion` that `bytes` hold, when their SHA-256 is
/// `digest` and the record they carry is the one their SHA-256 names.
pub fn capture_of(
    bytes: &[u8],
    digest: &str,
    completion: &str,
) -> Option<crate::agents::log::PaseoBody> {
    if crate::agents::approval::sha256_hex(bytes) != digest {
        return None;
    }
    serde_json::from_slice::<crate::agents::log::PaseoBody>(bytes)
        .ok()
        .filter(|captured| {
            captured.state == crate::agents::log::PaseoState::Pending
                && captured.completion == completion
                && captured.content.is_some()
                && captured.record.as_deref().is_some_and(|record| {
                    crate::agents::approval::sha256_hex(record.as_bytes()) == captured.sha256
                })
        })
}

/// The follow card of run `id` (96.3 #5, S-21): `every 10m`, assigned to
/// the agent `assignee`, `scheduled_by` its user `agent_user` — an agent's
/// schedule, which runs only once a person allows it — its body the one
/// call each run makes, through `server`.
pub fn follow_card(
    id: &str,
    server: &str,
    assignee: &str,
    agent_user: &UserId,
    requested_by: &UserId,
) -> String {
    let wire = crate::agents::mcp::wire_name(server, "get_agent_status").unwrap_or_default();
    format!(
        "---\ntags: [task]\ntitle: \"Follow Paseo run {id}\"\nstatus: todo\nassignee: {assignee}\nrequested_by: \"{requested_by}\"\nschedule: \"{FOLLOW_SCHEDULE}\"\nscheduled_by: \"{agent_user}\"\n---\n\nAsk Paseo for the status of run `{id}`: call `{wire}` with `agentId` = `{id}`, once. When the run has ended, keeper records it in {}, tells the conversation that started it, and ends this card's schedule.\n",
        artifact_path(id)
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agents::label::Integrity;
    use crate::agents::mcp;

    fn user(id: &str) -> matrix_sdk::ruma::OwnedUserId {
        matrix_sdk::ruma::OwnedUserId::try_from(id).expect("user")
    }

    /// 96.3 #1 (R24(2), AD-407): the four verbs at their fixed tiers,
    /// whatever the broker says of them; a fifth is not offered, and says
    /// why.
    #[test]
    fn paseo_role_classifies_exactly_four_verbs() {
        let entry = mcp::McpEntry {
            name: "paseo".to_owned(),
            transport: mcp::McpTransport::Url("https://paseo.example.org/mcp".to_owned()),
            credential: None,
            readers: Readers::Anyone,
            role: Some(mcp::McpRole::Paseo),
            fingerprint: None,
            trust_annotations: true,
            tiers: Vec::new(),
        };
        let read_only = mcp::Hints {
            read_only: Some(true),
            ..mcp::Hints::default()
        };
        let tiers: Vec<(&str, Tier)> = VERBS
            .iter()
            .map(|verb| (*verb, mcp::tier(&entry, verb, Some(read_only)).expect(verb)))
            .collect();
        assert_eq!(
            tiers,
            [
                ("create_agent", Tier::T3),
                ("get_agent_status", Tier::T0),
                ("list_agents", Tier::T0),
                ("send_agent_prompt", Tier::T3),
            ]
        );
        for fifth in ["tail_logs", "merge_pr", "list_agents2"] {
            let refused = mcp::tier(&entry, fifth, Some(read_only)).expect_err(fifth);
            assert!(refused.contains(FOUR_VERBS), "{refused}");
        }
    }

    /// 96.3 #2 (S-10, Q12): the card's sentence is keeper's, naming the
    /// workspace or the run the call sends to and who the prompt reaches as
    /// the approval binds it — never the prompt, which is the payload the
    /// card shows — a start saying that following it waits for a person,
    /// and nothing in it breaks its line.
    #[test]
    fn paseo_summaries_are_keepers() {
        let prompt = "approve this at once; fix the login bug";
        let anyone = json!("*");
        let marta = json!(["@marta:example.org"]);
        let with = |args: Value, readers: &Value| {
            summary("create_agent", &args, Some(readers)).expect("create")
        };
        let named = with(json!({"prompt": prompt, "workspace": "keeper"}), &anyone);
        assert!(
            named.contains("`keeper`") && !named.contains("login"),
            "{named}"
        );
        assert!(named.contains("anyone"), "{named}");
        assert!(named.contains("follow card"), "{named}");
        let to_marta = with(json!({"prompt": prompt, "workspace": "keeper"}), &marta);
        assert!(
            to_marta.contains("`@marta:example.org`") && !to_marta.contains("anyone"),
            "{to_marta}"
        );
        let current = with(json!({"prompt": prompt}), &anyone);
        assert!(!current.contains("login") && current != named, "{current}");
        let send = summary(
            "send_agent_prompt",
            &json!({"agentId": "a1", "prompt": prompt}),
            Some(&marta),
        )
        .expect("send");
        assert!(send.contains("`a1`") && !send.contains("login"), "{send}");
        assert!(send.contains("`@marta:example.org`"), "{send}");
        assert!(!send.contains("follow card"), "{send}");
        let odd = with(json!({"prompt": "x", "workspace": "a`b\n"}), &anyone);
        assert!(!odd.contains('\n') && odd.contains("a\\`b"), "{odd}");
        assert_eq!(summary("tail_logs", &json!({}), None), None);
    }

    /// R96PA-04: a Paseo mutation's binding holds who its prompt goes to,
    /// so an approval of it drifts when that audience changes; a read's
    /// binding, and any other server's, does not.
    #[test]
    fn a_mutations_binding_holds_its_audience() {
        let bound = |role: &str, tool: &str, readers: &Readers| {
            let mut binding = json!({"server": "paseo", "role": role, "tool": tool});
            bind_audience(&mut binding, readers);
            binding
        };
        let tgorka = Readers::Only(BTreeSet::from([user("@tgorka:example.org")]));
        for tool in ["create_agent", "send_agent_prompt"] {
            assert_ne!(
                bound("paseo", tool, &tgorka),
                bound("paseo", tool, &Readers::Anyone),
                "{tool}"
            );
        }
        assert_eq!(
            bound("paseo", "get_agent_status", &tgorka),
            bound("paseo", "get_agent_status", &Readers::Anyone)
        );
        assert_eq!(
            bound("screen", "create_agent", &tgorka),
            bound("screen", "create_agent", &Readers::Anyone)
        );
    }

    fn args(value: Value) -> Map<String, Value> {
        value.as_object().expect("object").clone()
    }

    /// 96.3 #7 (Q5, Q14): the reads pass only when they carry nothing of
    /// the session; a run this session did not start is blocked, not asked;
    /// the two mutations send the prompt.
    #[test]
    fn paseo_reach_rules() {
        let polled = BTreeSet::from(["a1".to_owned()]);
        assert_eq!(reach("list_agents", &Map::new(), &polled), Reach::Nothing);
        assert!(matches!(
            reach("list_agents", &args(json!({"q": "secret"})), &polled),
            Reach::Blocked(_)
        ));
        assert_eq!(
            reach("get_agent_status", &args(json!({"agentId": "a1"})), &polled),
            Reach::Nothing
        );
        let other = reach("get_agent_status", &args(json!({"agentId": "b2"})), &polled);
        assert!(
            matches!(&other, Reach::Blocked(why) if why.contains("`b2`")),
            "{other:?}"
        );
        assert!(matches!(
            reach(
                "get_agent_status",
                &args(json!({"agentId": "a1", "note": "x"})),
                &polled
            ),
            Reach::Blocked(_)
        ));
        assert!(matches!(
            reach(
                "get_agent_status",
                &args(json!({"agentId": "a1"})),
                &BTreeSet::new()
            ),
            Reach::Blocked(_)
        ));
        for tool in ["create_agent", "send_agent_prompt"] {
            assert_eq!(
                reach(tool, &args(json!({"prompt": "p"})), &polled),
                Reach::Prompt
            );
        }
        assert!(matches!(
            reach("merge", &Map::new(), &polled),
            Reach::Blocked(_)
        ));
    }

    /// The lines of an artifact that hold a field: `- key: …`.
    fn field_lines(text: &str) -> Vec<(&str, &str)> {
        text.lines()
            .filter_map(|line| line.strip_prefix("- "))
            .filter_map(|line| line.split_once(": "))
            .collect()
    }

    /// 96.3 #5, #6: the artifact holds the broker's eight fields only — a
    /// key outside them, a nested value, a token and a link with a
    /// credential are not written — and `prUrl` is a link only when it
    /// carries no credential.
    #[test]
    fn paseo_artifact_holds_only_whitelisted_fields() {
        let answered = answer(
            &json!({"agent": {
                "agentId": "r-42",
                "workspaceId": "ws1",
                "title": "Fix login with ghp_0123456789abcdefghijklmnopqrstuvwxyzAB",
                "status": "completed",
                "provider": "claude/opus",
                "createdAt": "2026-10-07T09:00:00Z",
                "updatedAt": "see https://bot:hunter2@git.example.org/x",
                "prUrl": "https://github.com/tgorka/keeper/pull/7",
                "diff": "--- a/x\n+++ b/x",
                "repoUrl": "https://user:token@github.com/tgorka/keeper.git",
                "logs": ["line"],
            }})
            .to_string(),
        );
        let run = &answered.runs[0];
        assert_eq!(run.keys(), FIELDS);
        let text = artifact(run);
        let keys: Vec<&str> = field_lines(&text).iter().map(|(key, _)| *key).collect();
        assert_eq!(keys, FIELDS);
        assert!(
            text.contains("- prUrl: <https://github.com/tgorka/keeper/pull/7>"),
            "{text}"
        );
        for absent in ["diff", "repoUrl", "token@", "hunter2", "logs", "ghp_0123"] {
            assert!(!text.contains(absent), "{absent}: {text}");
            assert!(
                !answered.text.contains(absent),
                "{absent}: {}",
                answered.text
            );
        }
        let with_userinfo = answer(
            &json!({"agent": {"agentId": "r-42", "prUrl": "https://user:token@github.com/x/pull/1"}})
                .to_string(),
        );
        let run = &with_userinfo.runs[0];
        assert_eq!(run.pr_url(), None);
        assert!(!artifact(run).contains("token@"));
        assert!(
            !with_userinfo.text.contains("token@"),
            "{}",
            with_userinfo.text
        );
        assert_eq!(run.keys(), ["agentId", "prUrl"]);
        assert_eq!(artifact_path("r-42"), "artifacts/paseo-r-42.md");
    }

    /// R96PA-05, R96PA2-01: every answer the model and the log read is the
    /// projection — both success shapes keep the eight fields only, made
    /// from the whole answer however long; an answer of another shape, or
    /// one that is not JSON, passes on none of its words; an error keeps
    /// its words but no token and no link's credential.
    #[test]
    fn every_answer_is_projected() {
        let many = answer(
            &json!({"agents": [
                {"agentId": "a1", "diff": "+secret", "title": "one"},
                {"agentId": "a2", "repoUrl": "https://u:p@h/x", "status": "running"},
            ]})
            .to_string(),
        );
        assert_eq!(many.runs.len(), 2);
        assert_eq!(many.runs[1].id(), Some("a2"));
        let shown: Value = serde_json::from_str(&many.text).expect("json");
        assert_eq!(
            shown,
            json!({"agents": [{"agentId": "a1", "title": "one"}, {"agentId": "a2", "status": "running"}]})
        );
        let error = sanitized(
            "clone failed for https://deploy:s3cr3t@github.com/tgorka/keeper.git with ghp_0123456789abcdefghijklmnopqrstuvwxyzAB; see https://github.com/tgorka",
        );
        for absent in ["s3cr3t", "deploy", "ghp_0123"] {
            assert!(!error.contains(absent), "{absent}: {error}");
        }
        assert!(error.contains("clone failed for"), "{error}");
        assert!(error.contains("https://github.com/tgorka"), "{error}");
        // Longer than any display's cut, an early field outside the eight.
        let long = json!({"agent": {
            "diff": "x".repeat(crate::agents::mcp::SHOWN_MAX),
            "agentId": "a3",
            "status": "running",
        }})
        .to_string();
        assert!(long.len() > crate::agents::mcp::SHOWN_MAX);
        let whole = answer(&long);
        assert_eq!(whole.runs[0].id(), Some("a3"));
        assert_eq!(
            serde_json::from_str::<Value>(&whole.text).expect("json"),
            json!({"agent": {"agentId": "a3", "status": "running"}})
        );
        for unread in [
            "ok: pushed to https://x:y@host/repo, diff --- a/x".to_owned(),
            json!({"result": "ok", "diff": "--- a/x"}).to_string(),
            json!({"agent": "a4", "diff": "--- a/x"}).to_string(),
            "{\"agent\": {\"agentId\": \"a5\", \"diff\": \"--- a/x\"".to_owned(),
            long[..crate::agents::mcp::SHOWN_MAX].to_owned(),
            format!("{} and more", json!({"agent": {"agentId": "a6"}})),
        ] {
            let other = answer(&unread);
            assert!(other.runs.is_empty(), "{unread:.80}");
            assert_eq!(other.text, UNREAD, "{unread:.80}");
        }
    }

    /// R96PA2-02: a link's credential is withheld as the URL parser reads
    /// the link — a quote or a space in the password ends nothing, a tab or
    /// a newline the parser drops splits nothing, and an authority with `@`
    /// it cannot read is withheld too — in a whitelisted field and in an
    /// error's words; a link with no credential, or `@` past its
    /// authority, stays.
    #[test]
    fn a_credential_is_withheld_however_its_link_is_written() {
        for leaky in [
            "https://deploy:s3c'ret@github.com/tgorka/keeper.git",
            "https://deploy:s3c ret@github.com/tgorka/keeper.git",
            "https://deploy:s3c\"ret@github.com/tgorka/keeper.git",
            "https://deploy:s3c\nret@github.com/tgorka/keeper.git",
            "https://deploy:s3c\tret@github.com/tgorka/keeper.git",
            "https:\t//deploy:s3cret@github.com/tgorka/keeper.git",
            "ht\ntps://deploy:s3cret@github.com/tgorka/keeper.git",
            "see https://github.com https://deploy:s3cret@github.com/x",
            "ssh://deploy:s3cret@github.com:22/keeper",
            "https://deploy:s3cret@[github.com/x",
        ] {
            let error = sanitized(&format!("clone failed for {leaky}; retry later"));
            for absent in ["s3c", "deploy"] {
                assert!(!error.contains(absent), "{leaky:?}: {error:?}");
            }
            assert!(error.contains("clone failed for"), "{error:?}");
            assert!(error.contains("; retry later"), "{error:?}");
            let answered = answer(
                &json!({"agent": {"agentId": "r1", "title": format!("Fix it, see {leaky}")}})
                    .to_string(),
            );
            assert_eq!(answered.runs[0].get("title"), Some(WITHHELD), "{leaky:?}");
            assert!(!answered.text.contains("s3c"), "{}", answered.text);
            assert!(!artifact(&answered.runs[0]).contains("s3c"));
        }
        for plain in [
            "see https://github.com/tgorka/keeper/pull/7 by tgorka",
            "https://github.com/x?mail=tgorka@example.org",
            "https://github.com/tgorka/keeper#by@tgorka",
            "write to tgorka@example.org",
        ] {
            assert_eq!(sanitized(plain), plain);
            let answered = answer(&json!({"agent": {"agentId": "r1", "title": plain}}).to_string());
            assert_eq!(answered.runs[0].get("title"), Some(plain));
        }
    }

    /// R283 (R96PA5-04): a link whose decoded query or fragment names a
    /// credential is withheld in every field kept, not only `prUrl` — the
    /// whole field — and in every error keeper passes on or composes, from
    /// the link to the text's end, as the URL parser reads its query: a
    /// space ends nothing, and a link inside it names its own parameters.
    /// Ordinary parameters stay.
    #[test]
    fn a_credential_parameter_is_withheld_from_every_field_and_error() {
        const FIXTURE: &str = "opaque-fixture-value";
        for leaky in [
            "https://git.example/repo?access_token=opaque-fixture-value",
            "https://git.example/repo?ref=main&X-Amz-Signature=opaque-fixture-value",
            "https://git.example/repo#oauth_token=opaque-fixture-value",
            "https://git.example/repo?%61ccess%5Ftoken=opaque-fixture-value",
            "https://git.example/repo?ref=a b&api_key=opaque-fixture-value",
            "https://a.example/?next=https://git.example/r?token=opaque-fixture-value",
            "HTTPS://git.example/repo?ref=main\n&private_token=opaque-fixture-value",
        ] {
            for field in ["title", "workspaceId", "provider", "updatedAt"] {
                let mut agent = json!({"agentId": "r1", "status": "completed"});
                agent[field] = json!(format!("retry {leaky}"));
                let answered = answer(&json!({ "agent": agent }).to_string());
                let run = &answered.runs[0];
                assert_eq!(run.get(field), Some(WITHHELD), "{field}: {leaky:?}");
                assert!(!answered.text.contains(FIXTURE), "{}", answered.text);
                assert!(!artifact(run).contains(FIXTURE), "{field}: {leaky:?}");
            }
            let error = sanitized(&format!("clone failed for {leaky}"));
            assert!(!error.contains(FIXTURE), "{leaky:?}: {error:?}");
            assert!(error.starts_with("clone failed for "), "{error:?}");
            let data = sanitized_value(&json!({"url": leaky, "mirrors": [leaky]})).to_string();
            assert!(!data.contains(FIXTURE), "{data}");
            let Reach::Blocked(why) = reach(
                "get_agent_status",
                &args(json!({"agentId": leaky})),
                &BTreeSet::new(),
            ) else {
                panic!("refused");
            };
            assert!(!why.contains(FIXTURE), "{why}");
        }
        for plain in [
            "see https://git.example/repo?ref=main&page=2 for the fix",
            "https://git.example/search?q=token&sort=updated",
            "https://git.example/repo#readme",
            "a key ?token=x outside any link",
        ] {
            assert_eq!(sanitized(plain), plain);
            let answered = answer(&json!({"agent": {"agentId": "r1", "title": plain}}).to_string());
            assert_eq!(answered.runs[0].get("title"), Some(plain));
        }
    }

    /// R277 (R96PA3-01): an authority is found where the URL parser finds
    /// one — after a special scheme's `:` past any number of `/` and `\`,
    /// none included — and a broker error's structured `data` has its
    /// strings withheld from before it is serialized, so a tab the
    /// serializer writes as `\t` hides no link. Links with no credential,
    /// written those ways too, stay.
    #[test]
    fn an_authority_is_found_where_the_url_parser_finds_it() {
        for leaky in [
            "https:///deploy:s3cret@github.com/x",
            "https:////deploy:s3cret@github.com/x",
            "https:/deploy:s3cret@github.com/x",
            "https:deploy:s3cret@github.com/x",
            "https:\\deploy:s3cret@github.com/x",
            "https:\\\\deploy:s3cret@github.com/x",
            "WSS:/\\/deploy:s3cret@github.com/x",
            "see https:/github.com then ftp:\\deploy:s3cret@github.com/x",
        ] {
            let error = sanitized(&format!("clone failed for {leaky}; retry later"));
            for absent in ["s3c", "deploy"] {
                assert!(!error.contains(absent), "{leaky:?}: {error:?}");
            }
            assert!(error.contains("; retry later"), "{error:?}");
            let answered = answer(
                &json!({"agent": {"agentId": "r1", "title": format!("Fix it, see {leaky}")}})
                    .to_string(),
            );
            assert_eq!(answered.runs[0].get("title"), Some(WITHHELD), "{leaky:?}");
        }
        let data = json!({
            "url": "https://deploy:plain\tsecret@github.com/repo",
            "mirrors": ["https:\\\\ops:hush@github.com/x", 3],
            "https://key:word@github.com/": true,
        });
        let kept = sanitized_value(&data).to_string();
        for absent in ["plain", "secret", "hush", "key:word"] {
            assert!(!kept.contains(absent), "{kept}");
        }
        assert!(kept.contains("\"mirrors\""), "{kept}");
        assert!(kept.contains(",3]"), "{kept}");
        for plain in [
            "see https:/github.com/tgorka/keeper and https:\\\\github.com/x",
            "mailto:tgorka@example.org at 10:30",
            "ssh:/deploy@github.com",
        ] {
            assert_eq!(sanitized(plain), plain);
            let value = json!({"note": plain, "n": 1});
            assert_eq!(sanitized_value(&value), value);
        }
    }

    /// R96PA-06, R96PA2-03, R287 (R96PA6-02): `prUrl` is presented only as
    /// `url`'s own serialization of a plain `http(s)` link; a value with a
    /// control, whitespace, angle bracket or backslash is withheld, and so
    /// is one whose decoded query or fragment names a credential, whatever
    /// its value's shape, or holds a link that does; no value adds a line to
    /// the artifact or a second link to the notice. An ordinary query, a
    /// link in it too, stays.
    #[test]
    fn the_pr_link_is_canonical() {
        assert_eq!(
            link("HTTPS://GitHub.com/tgorka/keeper/pull/7#top").as_deref(),
            Some("https://github.com/tgorka/keeper/pull/7#top")
        );
        for ordinary in [
            "https://github.com/tgorka/keeper/pull/7?tab=files",
            "https://github.com/tgorka/keeper/pull/7?w=1&diff=split#r12",
            "https://example.org/pull/7?page=2&monkeys=3",
            "https://example.org/pull/7?next=https://git.example/r?ref=main",
        ] {
            assert_eq!(link(ordinary).as_deref(), Some(ordinary), "{ordinary}");
        }
        for unsafe_link in [
            "https://example.org/pull/1>\n- diff: injected",
            "https://example.org/pull/1\t",
            "https://example.org/pull/1>",
            "https://example.org/<b>",
            "https://example.org\\pull\\1",
            "https://example.org/pull/1 x",
            "javascript:alert(1)",
            "https://example.org/pull/1?token=ghp_0123456789abcdefghijklmnopqrstuvwxyzAB",
            "https://example.org/pull/7?access_token=plain-deploy-secret",
            "https://example.org/pull/7?tab=files&TOKEN=plain-deploy-secret",
            "https://example.org/pull/7?access%5Ftoken=plain-deploy-secret",
            "https://example.org/pull/7?api-key=plain-deploy-secret",
            "https://example.org/pull/7?client_secret=plain-deploy-secret",
            "https://example.org/pull/7?password=plain-deploy-secret",
            "https://example.org/pull/7?auth=plain-deploy-secret",
            "https://example.org/pull/7?X-Amz-Signature=plain-deploy-secret",
            "https://example.org/pull/7?sig=plain-deploy-secret",
            "https://example.org/pull/7#access_token=plain-deploy-secret",
            "https://a.example/?next=https://git.example/r?token=opaque-fixture-value",
        ] {
            assert_eq!(link(unsafe_link), None, "{unsafe_link:?}");
            let answered = answer(
                &json!({"agent": {"agentId": "r1", "status": "completed", "prUrl": unsafe_link}})
                    .to_string(),
            );
            let run = &answered.runs[0];
            let text = artifact(run);
            let keys: Vec<&str> = field_lines(&text).iter().map(|(key, _)| *key).collect();
            assert_eq!(keys, ["agentId", "status", "prUrl"], "{text}");
            for absent in [
                "injected",
                "ghp_",
                "plain-deploy-secret",
                "opaque-fixture-value",
            ] {
                assert!(!text.contains(absent), "{text}");
                assert!(!answered.text.contains(absent), "{}", answered.text);
            }
            let told = ended(run);
            assert!(!told.contains('<') && !told.contains('\n'), "{told}");
            for absent in ["plain-deploy-secret", "opaque-fixture-value"] {
                assert!(!told.contains(absent), "{told}");
            }
        }
    }

    /// 96.3 #5: a run ends at a terminal status, read without case; any
    /// other is still running.
    #[test]
    fn paseo_terminal_statuses() {
        for status in ["completed", "Failed", " cancelled ", "ERROR"] {
            assert!(is_terminal(status), "{status}");
        }
        for status in ["running", "idle", "queued", "", "completedish"] {
            assert!(!is_terminal(status), "{status}");
        }
    }

    /// R96PA-11: an id the broker accepts is the run's on the wire, colon
    /// and all; its file name is portable and one run's alone; a
    /// secret-shaped id is not one keeper names.
    #[test]
    fn a_colon_id_is_the_brokers() {
        assert!(safe_id("claude:abc-1"));
        let run = answer(&json!({"agent": {"agentId": "claude:abc-1"}}).to_string());
        assert_eq!(run.runs[0].id(), Some("claude:abc-1"));
        assert_eq!(
            artifact_path("claude:abc-1"),
            "artifacts/paseo-claude~abc-1.md"
        );
        assert_ne!(stored("a:b"), stored("a-b"));
        assert_ne!(stored("a:b"), stored("a.b"));
        assert_ne!(stored("a:b"), stored("a_b"));
        for refused in [
            "",
            ":lead",
            "a/b",
            "a b",
            "ghp_0123456789abcdefghijklmnopqrstuvwxyzAB",
        ] {
            assert!(!safe_id(refused), "{refused}");
        }
    }

    /// Q14, R96PA-02: a follow session is known by the id derived for its
    /// broker and its run, and by the run its `[reply]` names; another
    /// session, another run named there, or the same run id on another
    /// broker follows nothing; its title is only what it is shown as; it
    /// answers the conversation that started it, not as a delegation.
    #[test]
    fn a_follow_session_is_its_runs() {
        let from = SessionAgent {
            id: Ulid::new(),
            agent: "amelia".to_owned(),
            drive: "tgdrive".to_owned(),
            kind: SessionKind::Conversation,
            title: "coding".to_owned(),
            requested_by: user("@tgorka:example.org"),
            parent: None,
            reply: None,
            room: OwnedRoomId::try_from("!a:example.org").expect("room"),
            drives: vec!["tgdrive".to_owned()],
            label: Label::top(),
            needs: None,
            pin: None,
            hop: 0,
            dispatch_chain: vec![user("@tgorka:example.org")],
            limits: None,
            workflow: None,
            checkpoints: None,
            outputs: Vec::new(),
            created_at: chrono::Utc::now(),
        };
        let label = Label {
            integrity: Integrity::Untrusted,
            ..Label::top()
        };
        let one = broker(&json!({"server": "paseo", "url": "https://a.example.org/mcp"}));
        let two = broker(&json!({"server": "paseo", "url": "https://b.example.org/mcp"}));
        assert_ne!(one, two);
        let room = OwnedRoomId::try_from("!f:example.org").expect("room");
        let follow = follow_session(
            &from,
            "active/2026-10-07-coding",
            label,
            &one,
            "r-42",
            room,
            chrono::Utc::now(),
        );
        assert_eq!(followed(&follow, &one), Some("r-42"));
        assert_eq!(followed(&follow, &two), None);
        assert_eq!(follow.kind, SessionKind::Scheduled);
        assert_eq!(follow.parent, None);
        assert_eq!(follow.requested_by, from.requested_by);
        assert_eq!(
            follow.reply,
            Some(SessionReply {
                session: "active/2026-10-07-coding".to_owned(),
                room: from.room.clone(),
                run: Some("r-42".to_owned()),
            })
        );
        let forged = SessionAgent {
            id: Ulid::new(),
            ..follow.clone()
        };
        assert_eq!(followed(&forged, &one), None);
        let mut retargeted = follow.clone();
        if let Some(reply) = retargeted.reply.as_mut() {
            reply.run = Some("r-43".to_owned());
        }
        assert_eq!(followed(&retargeted, &one), None);
        let renamed = SessionAgent {
            title: "paseo-r-43".to_owned(),
            ..follow.clone()
        };
        assert_eq!(followed(&renamed, &one), Some("r-42"));
        assert_ne!(
            follow_session_id("tgdrive", "amelia", &one, "r-42"),
            follow_session_id("tgdrive", "amelia", &two, "r-42")
        );
    }

    /// R96PA2-06: a run id as long as the broker's grammar allows, `:` and
    /// all, is followed whole — at 114, 115 and 128 characters the follow
    /// session's `agent.toml` reads back, its title a session title's
    /// length at most, and the run it follows, its record and its notice
    /// name the id as the broker wrote it.
    #[test]
    fn a_long_run_id_is_followed_whole() {
        let from = SessionAgent {
            id: Ulid::new(),
            agent: "amelia".to_owned(),
            drive: "tgdrive".to_owned(),
            kind: SessionKind::Conversation,
            title: "coding".to_owned(),
            requested_by: user("@tgorka:example.org"),
            parent: None,
            reply: None,
            room: OwnedRoomId::try_from("!a:example.org").expect("room"),
            drives: vec!["tgdrive".to_owned()],
            label: Label::top(),
            needs: None,
            pin: None,
            hop: 0,
            dispatch_chain: vec![user("@tgorka:example.org")],
            limits: None,
            workflow: None,
            checkpoints: None,
            outputs: Vec::new(),
            created_at: chrono::Utc::now(),
        };
        let one = broker(&json!({"server": "paseo", "url": "https://a.example.org/mcp"}));
        for length in [114, 115, 128] {
            let id = format!("claude:{}", "r".repeat(length - 7));
            assert!(safe_id(&id), "{length}");
            let follow = follow_session(
                &from,
                "active/2026-10-07-coding",
                Label::top(),
                &one,
                &id,
                OwnedRoomId::try_from("!f:example.org").expect("room"),
                chrono::Utc::now(),
            );
            assert!(
                follow.title.chars().count() <= crate::agents::session::TITLE_MAX,
                "{length}: {}",
                follow.title
            );
            let text = crate::agents::session::compose_session_agent_toml(&follow);
            let read =
                crate::agents::session::parse_session_agent_toml(&text).expect("it reads back");
            assert_eq!(followed(&read, &one), Some(id.as_str()), "{length}");
            let run = answer(&json!({"agent": {"agentId": id, "status": "completed"}}).to_string());
            assert_eq!(run.runs[0].id(), Some(id.as_str()));
            assert!(ended(&run.runs[0]).contains(&format!("`{id}`")));
            assert!(artifact_path(&id).contains(&stored(&id)));
        }
    }

    /// 96.3 #5 (S-21): the follow card is an agent's schedule — every ten
    /// minutes, assigned to the agent, `scheduled_by` its user — whose
    /// card reads by the board's grammar.
    #[test]
    fn the_follow_card_waits_for_a_person() {
        use crate::agents::card::{CardAgent, Field};
        let text = follow_card(
            "r-42",
            "paseo",
            "amelia",
            &user("@amelia:example.org"),
            &user("@tgorka:example.org"),
        );
        let keys = CardAgent::of_text(&text).expect("a card");
        assert_eq!(keys.assignee, Some(Field::Read("amelia".to_owned())));
        assert!(
            matches!(&keys.scheduled_by, Some(Field::Read(by)) if by.as_str() == "@amelia:example.org"),
            "{text}"
        );
        assert!(keys.schedule.is_some());
    }

    /// R279: the capture that binds a run's end is the first its agent
    /// published in the follow room, whatever lands after it; anyone
    /// else's message, and another completion's, count for nothing; one
    /// delivery message of the agent's is delivery for good.
    #[test]
    fn the_first_capture_binds_and_delivery_is_for_good() {
        let me = user("@amelia:example.org");
        let mine = "@amelia:example.org";
        let marta = "@marta:example.org";
        let event = |id: &str, sender: &str, content: Value| json!({"event_id": id, "sender": sender, "content": content});
        let capture = |id: &str, sender: &str, completion: &str, digest: &str| {
            event(
                id,
                sender,
                capture_message(completion, digest, json!({"url": id})),
            )
        };
        let delivered = |id: &str, sender: &str, notice: &str| {
            event(
                id,
                sender,
                delivered_message(
                    "c1",
                    <&matrix_sdk::ruma::EventId>::try_from("$y:example.org").expect("id"),
                    <&matrix_sdk::ruma::EventId>::try_from(notice).expect("id"),
                ),
            )
        };
        let events = [
            capture("$m:example.org", marta, "c1", "forged"),
            capture("$o:example.org", mine, "c2", "other"),
            capture("$y:example.org", mine, "c1", "first"),
            delivered("$f:example.org", marta, "$n0:example.org"),
            capture("$x:example.org", mine, "c1", "late"),
            delivered("$d:example.org", mine, "$n1:example.org"),
            delivered("$e:example.org", mine, "$n2:example.org"),
        ];
        let found = authority(&events, &me, "c1");
        let binding = found.capture.expect("the first capture");
        assert_eq!(binding.event.as_str(), "$y:example.org");
        assert_eq!(binding.digest, "first");
        assert_eq!(binding.file, json!({"url": "$y:example.org"}));
        assert_eq!(
            found.delivered.as_ref().map(|notice| notice.as_str()),
            Some("$n1:example.org")
        );
        // Before any delivery message of the agent's, nothing is delivered.
        let pending = authority(&events[..5], &me, "c1");
        assert_eq!(
            pending.capture.map(|binding| binding.digest).as_deref(),
            Some("first")
        );
        assert_eq!(pending.delivered, None);
        assert_eq!(authority(&events[..2], &me, "c1"), Authority::default());
        assert!(marks(&events[4]["content"], "c1") && marks(&events[5]["content"], "c1"));
        assert!(!marks(&events[1]["content"], "c1"));
    }
}
