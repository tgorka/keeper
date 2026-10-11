//! The push gateway the person names (Epic 98, story 98.1; AD-412, D-35,
//! NFR-121).
//!
//! keeper ships no gateway and never chooses one (D-1, D-4): the person types
//! the address of a gateway they run — a Sygnal beside their homeserver — into
//! `push.gateway_url`, and the phone hands it to the homeserver as an
//! `event_id_only` pusher. keeper itself never contacts it; the homeserver
//! does, with event ids for this device. This module is the grammar of that
//! one setting, so the setter, the egress row and the pusher read it the same
//! way: blank is "no push"; anything else is an `https://` address with no
//! user name or password whose path is the Matrix push path, which a bare
//! host gets filled in with.
//!
//! It is also what a push shows (story 98.1 #1, #2, #4, #11; AD-412): the
//! notification extension (`keeper-nse`) decrypts the one event a push names
//! and [`classify_notification`] decides — an agent's approval request, in
//! keeper's words, offering *Approve once* only when [`fits`] says the person
//! sees all of it, and the person's proxy's answer, by its newest words
//! ([`latest_text`]); nothing else.

use serde::{Deserialize, Serialize};
use serde_json::Value;
use ts_rs::TS;

use crate::agents::events::ApprovalRequestContent;

/// The path every Matrix push gateway answers at (Push Gateway API,
/// `POST /_matrix/push/v1/notify`), and the only one a pusher may name.
pub const NOTIFY_PATH: &str = "/_matrix/push/v1/notify";

/// A gateway address keeper will register: `https://`, no userinfo, the
/// [`NOTIFY_PATH`], no query or fragment.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Gateway {
    url: url::Url,
}

impl Gateway {
    /// Read the setting as typed. Blank (or only whitespace) is `Ok(None)`,
    /// "no push"; a bare `https://host[:port]` (with or without the trailing
    /// slash) gets the [`NOTIFY_PATH`]; anything else that is not exactly a
    /// gateway address is refused with the sentence the field shows.
    pub fn parse(raw: &str) -> Result<Option<Gateway>, GatewayRefusal> {
        let trimmed = raw.trim();
        if trimmed.is_empty() {
            return Ok(None);
        }
        if !trimmed.contains("://") {
            return Err(GatewayRefusal::NoScheme);
        }
        let mut url = url::Url::parse(trimmed).map_err(|_| GatewayRefusal::NotAnAddress)?;
        if url.scheme() != "https" {
            return Err(GatewayRefusal::NotHttps {
                scheme: url.scheme().to_owned(),
            });
        }
        if !url.username().is_empty() || url.password().is_some() {
            return Err(GatewayRefusal::Credentials);
        }
        if url.query().is_some() || url.fragment().is_some() {
            return Err(GatewayRefusal::NotTheNotifyPath);
        }
        match url.path() {
            "" | "/" => url.set_path(NOTIFY_PATH),
            NOTIFY_PATH => {}
            _ => return Err(GatewayRefusal::NotTheNotifyPath),
        }
        Ok(Some(Gateway { url }))
    }

    /// The whole address, as stored and as the pusher will carry it.
    pub fn as_str(&self) -> &str {
        self.url.as_str()
    }
}

/// Why a typed gateway address is not one keeper will register. The
/// `Display` text is the sentence Settings shows beside the field.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum GatewayRefusal {
    /// No `scheme://` in front — most often a bare host typed without
    /// `https://`.
    #[error(
        "A push gateway address starts with https://, as in \
         https://push.example.org."
    )]
    NoScheme,
    /// Not a URL at all.
    #[error(
        "That is not a web address. A push gateway looks like \
         https://push.example.org/_matrix/push/v1/notify."
    )]
    NotAnAddress,
    /// A scheme other than `https`.
    #[error(
        "keeper registers a push gateway only over https://, and this address starts \
         with {scheme}://."
    )]
    NotHttps {
        /// The scheme the address carried.
        scheme: String,
    },
    /// A user name or password in the address.
    #[error(
        "A push gateway address carries no user name or password: your homeserver \
         would keep it and send it with every push."
    )]
    Credentials,
    /// A path other than the push path, or a query or fragment.
    #[error(
        "A push gateway answers at /_matrix/push/v1/notify. Give its address with that \
         path or none, and nothing after it."
    )]
    NotTheNotifyPath,
}

/// `push.gateway_url` as Settings shows it: what is stored, and — when what
/// is stored is not a gateway keeper will register (no setter writes one, but
/// a synced `settings.toml` can carry any text) — why, in Rust's sentence.
/// Both `None` is "no push".
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct PushGatewayVm {
    /// The stored address, trimmed and never with a user name or password;
    /// `None` when blank, or when a refused value cannot be shown without
    /// what may be one.
    pub url: Option<String>,
    /// Why the stored address will not be registered; `None` when it will.
    pub refusal: Option<String>,
}

impl PushGatewayVm {
    /// The view of a stored value (`None` when the setting is absent).
    pub fn of(stored: Option<&str>) -> PushGatewayVm {
        let stored = stored.map(str::trim).filter(|s| !s.is_empty());
        match stored.map(Gateway::parse) {
            None | Some(Ok(None)) => PushGatewayVm {
                url: None,
                refusal: None,
            },
            Some(Ok(Some(gateway))) => PushGatewayVm {
                url: Some(gateway.as_str().to_owned()),
                refusal: None,
            },
            Some(Err(why)) => PushGatewayVm {
                url: stored.and_then(without_userinfo),
                refusal: Some(why.to_string()),
            },
        }
    }
}

/// A refused stored value as Settings may show it: a URL loses its user name
/// and password; text that is no URL with a host but carries an `@` is not
/// shown at all, since what precedes it may be a password.
fn without_userinfo(stored: &str) -> Option<String> {
    match url::Url::parse(stored) {
        Ok(mut url) if url.has_host() => {
            if url.username().is_empty() && url.password().is_none() {
                return Some(stored.to_owned());
            }
            url.set_username("").ok()?;
            url.set_password(None).ok()?;
            Some(url.into())
        }
        _ if stored.contains('@') => None,
        _ => Some(stored.to_owned()),
    }
}

// ---------------------------------------------------------------------------
// What a push shows (98.1 #1, #2, #4, #11; AD-412, S-10, R78)
//
// The gateway carries an event id and nothing else; the notification
// extension (`keeper-nse`) fetches and decrypts that one event and asks
// [`classify_notification`] what, if anything, the person sees. Two kinds of
// event wake the person: an agent's approval request, and an answer of their
// own proxy in its conversation with them. Everything else is dropped.

/// The key the pusher's `default_payload` carries keeper's local account id
/// under, so the extension opens exactly that account's store (codemap §0
/// item 13). Rung 4's pusher writes it; the extension reads it.
pub const PAYLOAD_ACCOUNT: &str = "keeper_account";

/// The longest payload, in characters, a notification shows as its whole
/// body and offers *Approve once* on (98.1 #11).
pub const BODY_MAX_CHARS: usize = 120;

/// How long the extension waits for an answer's newest edit before it shows
/// what it has (98.1 #2): an anchor arrives as its placeholder, and the
/// edits that carry the words do not push (`.m.rule.suppress_edits`).
pub const ANSWER_BUDGET: std::time::Duration = std::time::Duration::from_secs(6);

/// A notification category the app registers, and the actions it offers.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Category {
    /// `KEEPER_APPROVAL`: the whole payload is in the body; *Approve once*
    /// and *Deny*.
    #[serde(rename = "KEEPER_APPROVAL")]
    Approval,
    /// `KEEPER_APPROVAL_OPEN`: the body cannot show all of it, or the tier
    /// is T4; *Deny* only — approving is done in keeper, on the card.
    #[serde(rename = "KEEPER_APPROVAL_OPEN")]
    ApprovalOpen,
}

impl Category {
    /// The identifier iOS matches a notification to its registered category
    /// by.
    pub fn id(self) -> &'static str {
        match self {
            Category::Approval => "KEEPER_APPROVAL",
            Category::ApprovalOpen => "KEEPER_APPROVAL_OPEN",
        }
    }

    /// The actions the category offers, in order.
    pub fn actions(self) -> &'static [NotificationAction] {
        match self {
            Category::Approval => &[NotificationAction::ApproveOnce, NotificationAction::Deny],
            Category::ApprovalOpen => &[NotificationAction::Deny],
        }
    }
}

/// An action on an approval's notification. Both need the phone unlocked
/// (the app registers them so); neither is ever a `session` scope (R78).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum NotificationAction {
    #[serde(rename = "KEEPER_APPROVE_ONCE")]
    ApproveOnce,
    #[serde(rename = "KEEPER_DENY")]
    Deny,
}

impl NotificationAction {
    /// The identifier the app registers the action under.
    pub fn id(self) -> &'static str {
        match self {
            NotificationAction::ApproveOnce => "KEEPER_APPROVE_ONCE",
            NotificationAction::Deny => "KEEPER_DENY",
        }
    }

    /// The action's label on the notification.
    pub fn label(self) -> &'static str {
        match self {
            NotificationAction::ApproveOnce => "Approve once",
            NotificationAction::Deny => "Deny",
        }
    }

    /// The decision this action sends for `approval` — the request
    /// `agent_approval_decide` takes from the card, for exactly the record
    /// and digest the notification showed, always scope `once` (S-10/S-22,
    /// R78: a notification never grants a session). `None` when the
    /// notification did not offer this action — its category does not list
    /// it, or it has none — and for *Approve once* at T4 whatever the
    /// category says (R336), so a handler that dispatches on the action's id
    /// alone, or a replayed `userInfo`, cannot approve what the card would
    /// not.
    pub fn decide(
        self,
        approval: &PushApproval,
    ) -> Option<crate::agents::approval_card::ApprovalDecideReq> {
        let offered = approval
            .category
            .is_some_and(|category| category.actions().contains(&self));
        let decision = match self {
            NotificationAction::ApproveOnce if approval.tier >= 4 => return None,
            NotificationAction::ApproveOnce => crate::agents::approval::Decision::Approve,
            NotificationAction::Deny => crate::agents::approval::Decision::Deny,
        };
        offered.then(|| crate::agents::approval_card::ApprovalDecideReq {
            id: approval.id.clone(),
            binding_digest: approval.binding_digest.clone(),
            decision,
            scope: crate::agents::approval::Scope::Once,
            note: None,
        })
    }
}

/// The approval a notification shows, as its `userInfo` carries it to the
/// action handler: the room the decision goes into, the record, the digest
/// it must carry, the tier, and the category the notification was shown
/// with (`None`: no actions, it only opens keeper).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PushApproval {
    pub room: String,
    pub id: String,
    pub binding_digest: String,
    pub tier: u8,
    pub category: Option<Category>,
}

/// What the person sees.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Shown {
    pub title: String,
    pub subtitle: Option<String>,
    pub body: String,
    /// `None`: a notification with no actions, which opens keeper.
    pub category: Option<Category>,
    /// Notifications of one room group together.
    pub thread_id: String,
    /// The record an approval's actions decide.
    pub approval: Option<PushApproval>,
    /// For an answer: the anchor whose newer edits the extension waits for
    /// ([`ANSWER_BUDGET`], [`latest_text`]).
    pub answer: Option<String>,
}

/// What a decrypted event comes to on the lock screen.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Classified {
    Show(Box<Shown>),
    Drop,
}

/// What the extension knows of the event's room and sender.
#[derive(Debug, Clone, Copy)]
pub struct PushContext<'a> {
    /// The signed-in person.
    pub own: &'a matrix_sdk::ruma::UserId,
    /// The room, by its `m.room.create` type (`AgentRoomKind::of`).
    pub room: Option<crate::agents::room::AgentRoomKind>,
    pub room_id: &'a str,
    /// Whether the sender holds an agent's power in the room
    /// (`room::holds_agent_power`).
    pub agent_power: bool,
    /// Whether a device the SDK links to the sender sealed the event
    /// (`room::sealed_by_sender`, R185).
    pub sealed: bool,
    /// The proxy agent `proxy::admits` admits for the room when it is one
    /// of the person's proxy conversations (R72): the only sender whose
    /// answers are shown, there.
    pub answer_agent: Option<&'a matrix_sdk::ruma::UserId>,
    /// How the room names the sender (its display name); [`shown_name`]
    /// screens it.
    pub sender_name: &'a str,
    /// For an edit, the event it replaces, when the extension read it.
    pub replaced: Option<&'a Value>,
}

/// The name a notification gives `sender`: their display name `name`; their
/// user id when the name holds a character that [`misreads`]; keeper's own
/// words when that does too. A banner never carries a line break or a
/// direction override a sender chose.
pub fn shown_name<'a>(name: &'a str, sender: &'a str) -> &'a str {
    [name, sender]
        .into_iter()
        .find(|text| !text.chars().any(misreads))
        .unwrap_or("An agent")
}

/// Whether `content` replaces another event.
fn is_edit(content: &Value) -> bool {
    content["m.relates_to"]["rel_type"] == "m.replace"
}

/// What a decrypted `event` shows (98.1 #1): an approval request from an
/// agent of a session room, sealed by its sender's device — titled with
/// keeper's own summary, its actions by the card's rules
/// ([`approval_card::decidable_by`](crate::agents::approval_card::decidable_by))
/// and [`fits`]; an answer anchor of the person's proxy, from that proxy,
/// in its conversation with them, or an edit of one, from the anchor's
/// sender — shown with its text ([`latest_text`]). No title carries a
/// character that [`misreads`]. Everything else, including a person's
/// look-alike of either, is [`Classified::Drop`].
pub fn classify_notification(event: &Value, ctx: &PushContext<'_>) -> Classified {
    use crate::agents::events::{request_records, APPROVAL_REQUEST, TURN};
    use crate::agents::room::AgentRoomKind;

    let (Some(kind), Some(sender), Some(event_id)) = (
        event["type"].as_str(),
        event["sender"].as_str(),
        event["event_id"].as_str(),
    ) else {
        return Classified::Drop;
    };
    let content = &event["content"];
    if ctx.room != Some(AgentRoomKind::Session)
        || sender == ctx.own.as_str()
        || !ctx.agent_power
        || !ctx.sealed
    {
        return Classified::Drop;
    }
    let agent = shown_name(ctx.sender_name, sender);
    let shown = |title: String, subtitle, body, category, approval, answer| {
        Classified::Show(Box::new(Shown {
            title,
            subtitle,
            body,
            category,
            thread_id: ctx.room_id.to_owned(),
            approval,
            answer,
        }))
    };
    match kind {
        APPROVAL_REQUEST if !is_edit(content) => {
            let Some(records) = request_records(content) else {
                return Classified::Drop;
            };
            let [record] = records.as_slice() else {
                let tier = records.iter().map(|r| r.tier).max().unwrap_or(0);
                return shown(
                    format!("{agent} asks for {} decisions", records.len()),
                    None,
                    open_body(tier),
                    None,
                    None,
                    None,
                );
            };
            let approval = |category| {
                Some(PushApproval {
                    room: ctx.room_id.to_owned(),
                    id: record.id.clone(),
                    binding_digest: record.binding_digest.clone(),
                    tier: record.tier,
                    category,
                })
            };
            let title = match crate::agents::approval_card::binding(record) {
                crate::agents::approval_card::Binding::Unbound => {
                    format!("{agent} asks for a decision")
                }
                _ if record.summary.chars().any(misreads) => {
                    format!("{agent} asks for a decision")
                }
                _ => format!("{agent} asks: {}", record.summary),
            };
            if !crate::agents::approval_card::decidable_by(record, ctx.own.as_str()) {
                // The card would refuse every button: the notification
                // offers none, and a tap opens the card that says why.
                return shown(
                    title,
                    None,
                    open_body(record.tier),
                    None,
                    approval(None),
                    None,
                );
            }
            match payload_line(record).filter(|_| fits(record)) {
                Some(body) => shown(
                    title,
                    Some(crate::agents::approval_card::tier_word(record.tier)),
                    body,
                    Some(Category::Approval),
                    approval(Some(Category::Approval)),
                    None,
                ),
                None => shown(
                    title,
                    None,
                    open_body(record.tier),
                    Some(Category::ApprovalOpen),
                    approval(Some(Category::ApprovalOpen)),
                    None,
                ),
            }
        }
        "m.room.message"
            if ctx
                .answer_agent
                .is_some_and(|proxy| proxy.as_str() == sender) =>
        {
            if !is_edit(content) {
                if content.get(TURN).is_none() {
                    return Classified::Drop;
                }
                let text = content["body"].as_str().unwrap_or("");
                return shown(
                    agent.to_owned(),
                    None,
                    latest_text(text, &[], ANSWER_BUDGET, agent),
                    None,
                    None,
                    Some(event_id.to_owned()),
                );
            }
            let Some(anchor) = ctx.replaced.filter(|original| {
                original["event_id"] == content["m.relates_to"]["event_id"]
                    && original["sender"] == sender
                    && original["content"].get(TURN).is_some()
                    && !is_edit(&original["content"])
            }) else {
                return Classified::Drop;
            };
            let text = content["m.new_content"]["body"].as_str().unwrap_or("");
            shown(
                agent.to_owned(),
                None,
                latest_text(text, &[], ANSWER_BUDGET, agent),
                None,
                None,
                anchor["event_id"].as_str().map(str::to_owned),
            )
        }
        _ => Classified::Drop,
    }
}

/// The body of an approval the notification cannot show whole.
fn open_body(tier: u8) -> String {
    format!(
        "{} — open keeper to read the whole request",
        crate::agents::approval_card::tier_word(tier)
    )
}

/// Whether `c` could make shown text read as other than it is: a line
/// break or any other control character, or a bidirectional override.
fn misreads(c: char) -> bool {
    c.is_control()
        || matches!(c, '\u{061c}' | '\u{200e}' | '\u{200f}' | '\u{2028}' | '\u{2029}' | '\u{202a}'..='\u{202e}' | '\u{2066}'..='\u{2069}')
}

/// Whether any string in `value` — a key or a value, at any depth — holds a
/// character that [`misreads`].
fn holds_misreading(value: &Value) -> bool {
    match value {
        Value::String(text) => text.chars().any(misreads),
        Value::Array(items) => items.iter().any(holds_misreading),
        Value::Object(map) => map
            .iter()
            .any(|(key, value)| key.chars().any(misreads) || holds_misreading(value)),
        _ => false,
    }
}

/// The payload as a notification's body shows it: the exact arguments the
/// digest binds, as one line of JSON; `None` when they travel as a file.
fn payload_line(record: &ApprovalRequestContent) -> Option<String> {
    if record.file.is_some() {
        return None;
    }
    serde_json::to_string(&record.action.args).ok()
}

/// Whether a Paseo broker's prompt verb is what `record` would call: the
/// prompt is the payload, and it goes on to a model (R143, AD-407).
fn is_paseo_prompt(record: &ApprovalRequestContent) -> bool {
    record.action.tool == crate::agents::tier::AgentTool::Mcp.as_wire()
        && record.action.exec_binding["role"] == "paseo"
        && matches!(
            crate::agents::mcp::bound_tool(&record.action.exec_binding),
            Some((_, "create_agent" | "send_agent_prompt"))
        )
}

/// Whether a notification may offer *Approve once* on `record` (98.1 #11,
/// S-10): the person sees all of it there. Below T4; not a Paseo prompt;
/// the digest and summary bind the inline arguments (`Binding::Inline`, the
/// card's own test); not a networked `run`, whose released workspace the
/// body cannot list; and the arguments as the body shows them are one line
/// of at most [`BODY_MAX_CHARS`] characters in which no string holds a line
/// break, another control character or a bidirectional override.
pub fn fits(record: &ApprovalRequestContent) -> bool {
    record.tier < 4
        && !is_paseo_prompt(record)
        && crate::agents::approval_card::binding(record)
            == crate::agents::approval_card::Binding::Inline
        && record.preconditions.workspace.is_none()
        && !holds_misreading(&record.action.args)
        && payload_line(record).is_some_and(|line| line.chars().count() <= BODY_MAX_CHARS)
}

/// What an answer shows while its agent is still writing it.
pub fn answering(agent: &str) -> String {
    format!("{agent} is answering")
}

/// The text of a streamed answer the notification shows (98.1 #2): the
/// newest edit seen within `budget` (each edit with when, since the push
/// arrived, it was seen; in the room's order), else the anchor's text — and
/// never the placeholder: a body of only `…` (or nothing) is
/// [`answering`].
pub fn latest_text(
    anchor: &str,
    edits_seen: &[(std::time::Duration, &str)],
    budget: std::time::Duration,
    agent: &str,
) -> String {
    let text = edits_seen
        .iter()
        .filter(|(seen, _)| *seen <= budget)
        .map(|(_, text)| *text)
        .next_back()
        .unwrap_or(anchor);
    let bare = text.trim();
    if bare.is_empty() || bare == crate::agents::events::PLACEHOLDER {
        answering(agent)
    } else {
        text.to_owned()
    }
}

/// The text `event` carries when it is an edit of `anchor` from the
/// anchor's `sender`, sealed by a device of theirs: what [`latest_text`]
/// may show. Anyone else's edit of the anchor is not the answer.
pub fn answer_edit<'a>(
    event: &'a Value,
    anchor: &str,
    sender: &str,
    sealed: bool,
) -> Option<&'a str> {
    let content = &event["content"];
    (sealed
        && event["sender"] == sender
        && is_edit(content)
        && content["m.relates_to"]["event_id"] == anchor)
        .then(|| content["m.new_content"]["body"].as_str())
        .flatten()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parsed(raw: &str) -> Result<Option<String>, GatewayRefusal> {
        Gateway::parse(raw).map(|g| g.map(|g| g.as_str().to_owned()))
    }

    /// Q4: blank is no push; an `https://` address is the gateway, a bare
    /// host gaining the push path; `http://`, a user name or password,
    /// another path, a query or a fragment, and text that is no address are
    /// each refused with their own sentence.
    #[test]
    fn a_gateway_is_https_with_the_push_path_and_nothing_else() {
        assert_eq!(parsed(""), Ok(None));
        assert_eq!(parsed("  \t"), Ok(None));
        let full = "https://push.example.org/_matrix/push/v1/notify";
        assert_eq!(parsed(full), Ok(Some(full.to_owned())));
        assert_eq!(parsed(&format!("  {full}\n")), Ok(Some(full.to_owned())));
        assert_eq!(
            parsed("https://push.example.org"),
            Ok(Some(full.to_owned()))
        );
        assert_eq!(
            parsed("https://Push.Example.org/"),
            Ok(Some(full.to_owned()))
        );
        assert_eq!(
            parsed("https://push.example.org:8443"),
            Ok(Some(
                "https://push.example.org:8443/_matrix/push/v1/notify".to_owned()
            ))
        );

        assert_eq!(
            parsed("http://push.example.org/_matrix/push/v1/notify"),
            Err(GatewayRefusal::NotHttps {
                scheme: "http".to_owned()
            })
        );
        assert_eq!(
            parsed("https://me:hunter2@push.example.org"),
            Err(GatewayRefusal::Credentials)
        );
        assert_eq!(
            parsed("https://me@push.example.org"),
            Err(GatewayRefusal::Credentials)
        );
        for other in [
            "https://push.example.org/notify",
            "https://push.example.org/_matrix/push/v1/notify/",
            "https://push.example.org/_matrix/push/v1/notify?token=x",
            "https://push.example.org/?token=x",
            "https://push.example.org/_matrix/push/v1/notify#x",
        ] {
            assert_eq!(
                parsed(other),
                Err(GatewayRefusal::NotTheNotifyPath),
                "{other}"
            );
        }
        for no_scheme in [
            "push.example.org",
            "push.example.org:8443",
            "push.example.org/x",
        ] {
            assert_eq!(
                parsed(no_scheme),
                Err(GatewayRefusal::NoScheme),
                "{no_scheme}"
            );
        }
        assert_eq!(parsed("https://"), Err(GatewayRefusal::NotAnAddress));
    }

    /// What Settings shows: the stored address as keeper will register it,
    /// or — for a value no setter wrote (a layer file) — the value as stored
    /// with the refusal; blank is no push.
    #[test]
    fn settings_shows_the_stored_gateway_or_why_it_will_not_be_used() {
        let none = PushGatewayVm {
            url: None,
            refusal: None,
        };
        assert_eq!(PushGatewayVm::of(None), none);
        assert_eq!(PushGatewayVm::of(Some(" ")), none);
        assert_eq!(
            PushGatewayVm::of(Some("https://push.example.org")),
            PushGatewayVm {
                url: Some("https://push.example.org/_matrix/push/v1/notify".to_owned()),
                refusal: None,
            }
        );
        assert_eq!(
            PushGatewayVm::of(Some(" http://push.example.org ")),
            PushGatewayVm {
                url: Some("http://push.example.org".to_owned()),
                refusal: Some(
                    GatewayRefusal::NotHttps {
                        scheme: "http".to_owned()
                    }
                    .to_string()
                ),
            }
        );
        // A refused value is never shown with its user name or password.
        assert_eq!(
            PushGatewayVm::of(Some("https://me:hunter2@push.example.org")),
            PushGatewayVm {
                url: Some("https://push.example.org/".to_owned()),
                refusal: Some(GatewayRefusal::Credentials.to_string()),
            }
        );
        assert_eq!(
            PushGatewayVm::of(Some("http://me:hunter2@push.example.org/x")),
            PushGatewayVm {
                url: Some("http://push.example.org/x".to_owned()),
                refusal: Some(
                    GatewayRefusal::NotHttps {
                        scheme: "http".to_owned()
                    }
                    .to_string()
                ),
            }
        );
        assert_eq!(
            PushGatewayVm::of(Some("me:hunter2@push.example.org")),
            PushGatewayVm {
                url: None,
                refusal: Some(GatewayRefusal::NoScheme.to_string()),
            }
        );
    }

    // --- What a push shows (98.1 #1, #2, #4, #11) ---

    use crate::agents::approval::{binding_digest, summary_of, Decision, Scope};
    use crate::agents::events::{
        APPROVAL_DECISION, CLAIM, DELEGATE, HEARD, SCOPE, STATUS, SURFACE_REQUEST, SURFACE_RESULT,
        TURN,
    };
    use crate::agents::room::AgentRoomKind;
    use crate::agents::tier::AgentTool;
    use matrix_sdk::ruma::{OwnedUserId, UserId};
    use serde_json::json;
    use std::time::Duration;

    const TGORKA: &str = "@tgorka:example.org";
    const NIXI: &str = "@nixi:example.org";
    const ROOM: &str = "!session:example.org";

    fn me() -> OwnedUserId {
        OwnedUserId::try_from(TGORKA).expect("user")
    }

    /// `request` with its summary and digest made over `args`, as a host
    /// makes them.
    fn sealed(mut request: Value, args: &Value) -> Value {
        let tool = request["action"]["tool"].as_str().expect("tool").to_owned();
        request["action"]["args"] = args.clone();
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

    fn request_of(tier: u8, tool: &str, args: Value, exec_binding: Value) -> Value {
        sealed(
            json!({
                "v": 1, "id": "01A", "session": "60-sessions/active/2026-10-05-chat",
                "room": ROOM, "agent": "nixi", "tier": tier, "summary": "",
                "action": {"tool": tool, "args": null, "exec_binding": exec_binding},
                "checkpoint_sha256": "c0ffee",
                "preconditions": {"files": [], "workspace": null, "screen": null,
                    "max_staleness_s": null},
                "binding_digest": "",
                "scopes": ["once"],
                "expires_at": "2026-10-06T12:00:00.000Z",
                "approvers": [TGORKA],
                "dispatch_chain": [TGORKA, NIXI],
            }),
            &args,
        )
    }

    fn write(tier: u8, text: &str) -> Value {
        request_of(
            tier,
            "drive_write",
            json!({"profile": "tgdrive", "path": "notes/plan.md", "content": text}),
            Value::Null,
        )
    }

    fn run_binding(argv: &Value) -> Value {
        json!({
            "host": "electra", "argv": argv, "cwd": "", "env": [],
            "exe": "/usr/bin/git", "exe_sha256": "e".repeat(64), "operands": [],
        })
    }

    fn run(tier: u8, argv: Value) -> Value {
        let binding = run_binding(&argv);
        request_of(tier, "run", json!({"argv": argv}), binding)
    }

    fn mcp(role: Option<&str>, tool: &str) -> Value {
        let mut binding = json!({"server": "broker", "transport": "url", "url": "https://b.example",
            "tool": tool, "definition_sha256": "d", "tier": 3});
        if let Some(role) = role {
            binding["role"] = json!(role);
        }
        request_of(3, "mcp", json!({"prompt": "fix the bug"}), binding)
    }

    fn record(value: &Value) -> ApprovalRequestContent {
        ApprovalRequestContent::deserialize(value).expect("record")
    }

    fn event(kind: &str, id: &str, sender: &str, content: Value) -> Value {
        json!({"type": kind, "event_id": id, "sender": sender, "content": content})
    }

    /// A session room of the person's proxy Nixi: Nixi holds an agent's
    /// power, the event is sealed by Nixi's device, the room is a proxy
    /// conversation `proxy::admits` admits with Nixi as its agent.
    fn ctx<'a>(own: &'a UserId, proxy: &'a UserId) -> PushContext<'a> {
        PushContext {
            own,
            room: Some(AgentRoomKind::Session),
            room_id: ROOM,
            agent_power: true,
            sealed: true,
            answer_agent: Some(proxy),
            sender_name: "Nixi",
            replaced: None,
        }
    }

    fn shown(classified: Classified) -> Shown {
        match classified {
            Classified::Show(shown) => *shown,
            Classified::Drop => panic!("dropped"),
        }
    }

    fn approval_of(value: &Value, category: Option<Category>) -> Option<PushApproval> {
        Some(PushApproval {
            room: ROOM.to_owned(),
            id: value["id"].as_str().expect("id").to_owned(),
            binding_digest: value["binding_digest"].as_str().expect("d").to_owned(),
            tier: u8::try_from(value["tier"].as_u64().expect("tier")).expect("u8"),
            category,
        })
    }

    /// 98.1 #1: only an agent's approval request and the proxy's answer
    /// wake the person, in keeper's words; everything else, and a person's
    /// look-alike of either, is dropped.
    #[test]
    fn push_classification_table() {
        let own = me();
        let proxy = OwnedUserId::try_from(NIXI).expect("user");
        let base = ctx(&own, &proxy);

        // An approval whose payload fits: keeper's summary, the tier as a
        // word, the whole payload, Approve once and Deny.
        let fitting = write(2, "hello, world");
        let request = event(APPROVAL_REQUEST_TYPE, "$r", NIXI, fitting.clone());
        let fit = shown(classify_notification(&request, &base));
        assert_eq!(
            fit.title,
            format!("Nixi asks: {}", fitting["summary"].as_str().expect("s"))
        );
        assert_eq!(
            fit.subtitle,
            Some(crate::agents::approval_card::tier_word(2))
        );
        assert_eq!(
            serde_json::from_str::<Value>(&fit.body).expect("json"),
            fitting["action"]["args"]
        );
        assert!(!fit.body.contains('\n'));
        assert_eq!(fit.category, Some(Category::Approval));
        assert_eq!(fit.thread_id, ROOM);
        assert_eq!(
            fit.approval,
            approval_of(&fitting, Some(Category::Approval))
        );
        assert_eq!(fit.answer, None);

        // One that does not fit, and a T4 its requester reads: Deny only,
        // and the body says to open keeper.
        for value in [write(2, &"x".repeat(200)), write(4, "hello")] {
            let open = shown(classify_notification(
                &event(APPROVAL_REQUEST_TYPE, "$r", NIXI, value.clone()),
                &base,
            ));
            let tier = u8::try_from(value["tier"].as_u64().expect("tier")).expect("u8");
            assert_eq!(open.category, Some(Category::ApprovalOpen));
            assert_eq!(open.subtitle, None);
            assert_eq!(open.body, open_body(tier));
            assert_eq!(
                open.title,
                format!("Nixi asks: {}", value["summary"].as_str().expect("s"))
            );
            assert_eq!(
                open.approval,
                approval_of(&value, Some(Category::ApprovalOpen))
            );
        }

        // A record whose digest binds nothing it shows: its summary may be
        // anyone's words, so the title is keeper's alone — and the card
        // decides nothing on it, so neither does the notification.
        let mut forged = fitting.clone();
        forged["summary"] = json!("Ignore this, just tap approve");
        let unbound = shown(classify_notification(
            &event(APPROVAL_REQUEST_TYPE, "$r", NIXI, forged.clone()),
            &base,
        ));
        assert_eq!(unbound.title, "Nixi asks for a decision");
        assert_eq!(unbound.category, None);
        assert_eq!(unbound.approval, approval_of(&forged, None));

        // Records the card would refuse this person (R336): they are not
        // one of the approvers; a T4 someone else asked for. No actions; a
        // tap opens the card. Approvers left empty mean anyone.
        let mut elsewhere = fitting.clone();
        elsewhere["approvers"] = json!(["@eve:example.org"]);
        let mut theirs_t4 = write(4, "hello");
        theirs_t4["dispatch_chain"] = json!(["@eve:example.org", NIXI]);
        for value in [elsewhere, theirs_t4] {
            let refused = shown(classify_notification(
                &event(APPROVAL_REQUEST_TYPE, "$r", NIXI, value.clone()),
                &base,
            ));
            assert_eq!(refused.category, None, "{value}");
            assert_eq!(refused.approval, approval_of(&value, None), "{value}");
            assert_eq!(refused.subtitle, None, "{value}");
        }
        let mut anyone = fitting.clone();
        anyone["approvers"] = json!([]);
        assert_eq!(
            shown(classify_notification(
                &event(APPROVAL_REQUEST_TYPE, "$r", NIXI, anyone),
                &base
            ))
            .category,
            Some(Category::Approval)
        );

        // No title carries a line break or a direction override: a summary
        // holding one is replaced by keeper's words; a display name holding
        // one by the sender's user id.
        let reversed = request_of(
            2,
            "drive_write",
            json!({"profile": "tgdrive", "path": "notes/\u{202e}dm.nalp", "content": "x"}),
            Value::Null,
        );
        let screened = shown(classify_notification(
            &event(APPROVAL_REQUEST_TYPE, "$r", NIXI, reversed),
            &base,
        ));
        assert_eq!(screened.title, unbound.title);
        let renamed = shown(classify_notification(
            &request,
            &PushContext {
                sender_name: "Nixi\nApproved",
                ..base
            },
        ));
        assert_eq!(
            renamed.title,
            format!("{NIXI} asks: {}", fitting["summary"].as_str().expect("s"))
        );
        assert_eq!(shown_name("Ni\u{202e}xi", "@ni\u{2028}xi:x"), "An agent");

        // A gate's coalesced card: several decisions, no action decides
        // them all.
        let coalesced = json!({"v": 1, "records": [write(2, "a"), write(3, "b")]});
        let many = shown(classify_notification(
            &event(APPROVAL_REQUEST_TYPE, "$r", NIXI, coalesced),
            &base,
        ));
        assert_eq!(many.title, "Nixi asks for 2 decisions");
        assert_eq!(many.category, None);
        assert_eq!(many.approval, None);

        // The proxy's answer anchor: the placeholder is never shown.
        let anchor_content =
            json!({"msgtype": "m.text", "body": "…", TURN: {"session": "s", "line": "l"}});
        let anchor = event("m.room.message", "$a", NIXI, anchor_content.clone());
        let answer = shown(classify_notification(&anchor, &base));
        assert_eq!(answer.title, "Nixi");
        assert_eq!(answer.body, answering("Nixi"));
        assert_eq!(answer.category, None);
        assert_eq!(answer.approval, None);
        assert_eq!(answer.answer.as_deref(), Some("$a"));

        // Its edit, read beside the anchor it replaces: the edit's words.
        let edit = event(
            "m.room.message",
            "$e",
            NIXI,
            crate::agents::events::edit_content(
                &matrix_sdk::ruma::OwnedEventId::try_from("$a").expect("id"),
                "Done: the plan is written.",
            ),
        );
        let replaced = PushContext {
            replaced: Some(&anchor),
            ..base
        };
        let edited = shown(classify_notification(&edit, &replaced));
        assert_eq!(edited.body, "Done: the plan is written.");
        assert_eq!(edited.answer.as_deref(), Some("$a"));
        // An edit whose anchor was not read, or is someone else's.
        assert_eq!(classify_notification(&edit, &base), Classified::Drop);
        let theirs = event("m.room.message", "$a", TGORKA, anchor_content.clone());
        assert_eq!(
            classify_notification(
                &edit,
                &PushContext {
                    replaced: Some(&theirs),
                    ..base
                }
            ),
            Classified::Drop
        );

        // A person's look-alike: no agent power; the own user; unsealed; a
        // room that is not an agent session; an answer outside the proxy's
        // conversation; another agent-power sender's answer in it, or its
        // edit beside its own anchor (R336: the admitted proxy only).
        let eve_anchor = event(
            "m.room.message",
            "$a",
            "@eve:example.org",
            anchor_content.clone(),
        );
        let eve_edit = event(
            "m.room.message",
            "$e",
            "@eve:example.org",
            edit["content"].clone(),
        );
        for (event, ctx) in [
            (
                anchor.clone(),
                PushContext {
                    agent_power: false,
                    ..base
                },
            ),
            (
                request.clone(),
                PushContext {
                    agent_power: false,
                    ..base
                },
            ),
            (
                event("m.room.message", "$a", TGORKA, anchor_content.clone()),
                base,
            ),
            (
                request.clone(),
                PushContext {
                    sealed: false,
                    ..base
                },
            ),
            (
                anchor.clone(),
                PushContext {
                    sealed: false,
                    ..base
                },
            ),
            (request.clone(), PushContext { room: None, ..base }),
            (
                request.clone(),
                PushContext {
                    room: Some(AgentRoomKind::Control),
                    ..base
                },
            ),
            (
                anchor.clone(),
                PushContext {
                    answer_agent: None,
                    ..base
                },
            ),
            (eve_anchor.clone(), base),
            (
                eve_edit,
                PushContext {
                    replaced: Some(&eve_anchor),
                    ..base
                },
            ),
        ] {
            assert_eq!(
                classify_notification(&event, &ctx),
                Classified::Drop,
                "{event}"
            );
        }

        // Every other agent event, a request's edit, a message without the
        // turn marker, a brief, and an ordinary message: dropped.
        let mut request_edit = fitting.clone();
        request_edit["m.relates_to"] = json!({"rel_type": "m.replace", "event_id": "$r0"});
        for other in [
            event(STATUS, "$s", NIXI, json!({"v": 1, "run": "running"})),
            event(SCOPE, "$s", NIXI, json!({"drives": []})),
            event(SURFACE_REQUEST, "$s", NIXI, json!({"v": 1})),
            event(SURFACE_RESULT, "$s", NIXI, json!({"v": 1})),
            event(HEARD, "$s", NIXI, json!({"v": 1})),
            event("dev.keeper.agent.doorbell", "$s", NIXI, json!({"v": 1})),
            event(APPROVAL_DECISION, "$s", NIXI, json!({"id": "01A"})),
            event(CLAIM, "$s", NIXI, json!({"v": 1})),
            event(APPROVAL_REQUEST_TYPE, "$r", NIXI, request_edit),
            event(
                "m.room.message",
                "$m",
                NIXI,
                json!({"msgtype": "m.text", "body": "a notice"}),
            ),
            event(
                "m.room.message",
                "$m",
                NIXI,
                json!({"msgtype": "m.text", "body": "a brief", DELEGATE: {"v": 1}}),
            ),
        ] {
            assert_eq!(
                classify_notification(&other, &base),
                Classified::Drop,
                "{other}"
            );
        }
        let ordinary = event(
            "m.room.message",
            "$m",
            NIXI,
            json!({"msgtype": "m.text", "body": "hi"}),
        );
        assert_eq!(
            classify_notification(
                &ordinary,
                &PushContext {
                    room: None,
                    answer_agent: None,
                    ..base
                }
            ),
            Classified::Drop
        );
    }

    const APPROVAL_REQUEST_TYPE: &str = crate::agents::events::APPROVAL_REQUEST;

    /// 98.1 #2: the newest edit seen within the budget, else the anchor's
    /// text, and never the placeholder.
    #[test]
    fn push_waits_for_the_latest_edit() {
        let s = Duration::from_secs;
        let budget = s(6);
        let edits = [
            (s(1), "Looking"),
            (s(3), "Looking at the plan"),
            (s(7), "Done."),
        ];
        let waiting = answering("Nixi");
        assert_eq!(
            latest_text("…", &edits, budget, "Nixi"),
            "Looking at the plan"
        );
        assert_eq!(latest_text("…", &edits, s(7), "Nixi"), "Done.");
        assert_eq!(latest_text("…", &edits[..1], budget, "Nixi"), "Looking");
        assert_eq!(latest_text("…", &edits[2..], budget, "Nixi"), waiting);
        assert_eq!(latest_text("…", &[], budget, "Nixi"), waiting);
        assert_eq!(latest_text(" … ", &[], budget, "Nixi"), waiting);
        assert_eq!(latest_text("", &[], budget, "Nixi"), waiting);
        assert_eq!(latest_text("Hello.", &[], budget, "Nixi"), "Hello.");
        assert_eq!(
            latest_text("Hello.", &[(s(0), "…")], budget, "Nixi"),
            waiting
        );

        // Which edits count: the anchor's, by its sender, sealed.
        let edit = |sender: &str, target: &str| {
            json!({"type": "m.room.message", "sender": sender, "event_id": "$e",
            "content": crate::agents::events::edit_content(
                &matrix_sdk::ruma::OwnedEventId::try_from(target).expect("id"),
                "words",
            )})
        };
        assert_eq!(
            answer_edit(&edit(NIXI, "$a"), "$a", NIXI, true),
            Some("words")
        );
        assert_eq!(answer_edit(&edit(NIXI, "$a"), "$a", NIXI, false), None);
        assert_eq!(answer_edit(&edit(TGORKA, "$a"), "$a", NIXI, true), None);
        assert_eq!(answer_edit(&edit(NIXI, "$b"), "$a", NIXI, true), None);
    }

    /// 98.1 #11 (S-10): Approve once is offered only where the person can
    /// see all of it.
    #[test]
    fn notification_offers_approve_only_when_the_payload_fits() {
        let fits_value = |value: &Value| fits(&record(value));
        // A three-element argv at T3.
        assert!(fits_value(&run(3, json!(["git", "log", "-1"]))));
        // The boundary: 120 characters fit, 121 do not.
        let at = |chars: usize| {
            let overhead = serde_json::to_string(&json!({"argv": ["echo", ""]}))
                .expect("json")
                .chars()
                .count();
            run(3, json!(["echo", "x".repeat(chars - overhead)]))
        };
        let line = |value: &Value| {
            serde_json::to_string(&value["action"]["args"])
                .expect("json")
                .chars()
                .count()
        };
        assert_eq!(line(&at(120)), 120);
        assert!(fits_value(&at(120)));
        assert_eq!(line(&at(121)), 121);
        assert!(!fits_value(&at(121)));
        // An element holding a line break, or a bidirectional override.
        assert!(!fits_value(&run(3, json!(["printf", "a\nb"]))));
        assert!(!fits_value(&run(3, json!(["cat", "\u{202e}gnp.exe"]))));
        // Paseo's prompt verbs, by the server's role, not its name.
        assert!(!fits_value(&mcp(Some("paseo"), "create_agent")));
        assert!(!fits_value(&mcp(Some("paseo"), "send_agent_prompt")));
        assert!(fits_value(&mcp(Some("paseo"), "list_agents")));
        assert!(fits_value(&mcp(None, "create_agent")));
        // Any T4 record.
        assert!(fits_value(&run(3, json!(["git", "push"]))));
        assert!(!fits_value(&run(4, json!(["git", "push"]))));
        // A networked run: its released workspace is not in the body.
        let mut networked = run(3, json!(["curl", "-I", "x"]));
        networked["preconditions"]["workspace"] = json!({"sha256": "s", "bytes": 1, "files": []});
        let networked = sealed(networked.clone(), &networked["action"]["args"].clone());
        assert!(!fits_value(&networked));
    }

    /// 98.1 #4 (Q3, R336): the categories offer exactly their actions, and
    /// an action decides only when the notification offered it — the
    /// record shown, scope `once`, never `session`, never another record;
    /// *Approve once* never at T4, whatever the carried category says.
    #[test]
    fn notification_actions_decide_once_for_the_record_shown() {
        assert_eq!(
            Category::Approval.actions(),
            [NotificationAction::ApproveOnce, NotificationAction::Deny]
        );
        assert_eq!(Category::ApprovalOpen.actions(), [NotificationAction::Deny]);
        let shown_as = |tier: u8, category: Option<Category>| PushApproval {
            room: ROOM.to_owned(),
            id: "01A".to_owned(),
            binding_digest: "d1".to_owned(),
            tier,
            category,
        };
        for (action, decision) in [
            (NotificationAction::ApproveOnce, Decision::Approve),
            (NotificationAction::Deny, Decision::Deny),
        ] {
            let req = action
                .decide(&shown_as(3, Some(Category::Approval)))
                .expect("offered");
            assert_eq!(req.id, "01A");
            assert_eq!(req.binding_digest, "d1");
            assert_eq!(req.decision, decision);
            assert_eq!(req.scope, Scope::Once);
            assert_eq!(req.note, None);
        }

        let approve = NotificationAction::ApproveOnce;
        let deny = NotificationAction::Deny;
        // Shown with Deny only: Approve once is not an answer to it.
        let open = shown_as(3, Some(Category::ApprovalOpen));
        assert_eq!(approve.decide(&open), None);
        assert_eq!(
            deny.decide(&open).map(|req| req.decision),
            Some(Decision::Deny)
        );
        // Shown with no actions (the card would refuse): nothing decides.
        let refused = shown_as(3, None);
        assert_eq!(approve.decide(&refused), None);
        assert_eq!(deny.decide(&refused), None);
        // A T4 carried as the approving category (a replayed or forged
        // `userInfo`): still no Approve; Deny stands.
        let forged = shown_as(4, Some(Category::Approval));
        assert_eq!(approve.decide(&forged), None);
        assert_eq!(
            deny.decide(&forged).map(|req| req.decision),
            Some(Decision::Deny)
        );
    }
}
