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

/// A pusher's identity on a homeserver — what [`register`] and a sign-out
/// remove by — named for the shell, which has no `matrix-sdk` of its own.
pub use matrix_sdk::ruma::api::client::push::PusherIds;
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

    /// The category iOS names by `id`; `None` for any other.
    pub fn of_id(id: &str) -> Option<Category> {
        [Category::Approval, Category::ApprovalOpen]
            .into_iter()
            .find(|category| category.id() == id)
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

    /// The action iOS names by `id`; `None` for any other.
    pub fn of_id(id: &str) -> Option<NotificationAction> {
        [NotificationAction::ApproveOnce, NotificationAction::Deny]
            .into_iter()
            .find(|action| action.id() == id)
    }

    /// Whether the lock screen draws the action as destructive (UX-DR144:
    /// *Deny*).
    pub fn destructive(self) -> bool {
        self == NotificationAction::Deny
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

// ---------------------------------------------------------------------------
// The pusher (98.1 #3; AD-412, Q4, DW-1230)
//
// The phone hands each account's homeserver an `event_id_only` pusher naming
// the gateway the person typed — read only through [`Gateway::parse`], so a
// refused or blank setting registers nothing and the raw text never reaches
// the homeserver — and the APNs token this build was given.

/// The notification's own words when the extension cannot say more — the
/// app's data has not moved yet, the token expired, an event it could not
/// read (`keeper-nse`'s generic answer): keeper's words, never an event's.
pub const GENERIC_TITLE: &str = "keeper";

/// See [`GENERIC_TITLE`].
pub const GENERIC_BODY: &str = "Open keeper to see what arrived.";

/// The key an approval's notification carries the shown record under, for
/// the action handler (`KeeperNotify` writes it, [`action_decision`] reads
/// it).
pub const APPROVAL_INFO: &str = "keeper_approval";

/// The key only a notification keeper composed on this phone carries
/// (R343): the extension's `show`, or the notification keeper re-posts
/// after an action ([`FINISH_IN_KEEPER`]). The push itself is the
/// homeserver's to write — the pusher's `default_payload` and anything
/// that reaches the gateway — so it can carry this key, and
/// [`APPROVAL_INFO`], [`PAYLOAD_ACCOUNT`] and a category too; the extension
/// therefore removes every keeper key from what arrived on every path, shows
/// keeper's generic words with no category on every path but `show`, and
/// sets this key only there. [`action_decision`] and [`tap_target`] act on
/// nothing without it. What it cannot cover: an extension that ends before
/// it answers at all, where iOS shows the push as it arrived. Then the
/// decision path's own check is the guard — the record must be in the
/// session room as this phone syncs it, with the digest the notification
/// carried ([`crate::agents::approval_card::ApprovalFold::check`]), so
/// nothing is decided that is not a real request with its real digest.
pub const SHOWN_INFO: &str = "keeper_shown";

/// APNs' two environments, which a Sygnal serves as two apps (Q4): the
/// pusher's `app_id` says which one this build's token belongs to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ApnsEnvironment {
    /// The sandbox: a build signed with a development profile.
    Development,
    /// Production: an ad-hoc, TestFlight or App Store build.
    Production,
}

impl ApnsEnvironment {
    /// The pusher's `app_id`: the Sygnal app that holds this environment.
    pub fn app_id(self) -> &'static str {
        match self {
            ApnsEnvironment::Development => "dev.tgorka.keeper.ios.dev",
            ApnsEnvironment::Production => "dev.tgorka.keeper.ios",
        }
    }

    /// The environment this build's signature registers with, from the
    /// bundle's `embedded.mobileprovision`: its `aps-environment`; no
    /// profile at all is an App Store build, which is production; a profile
    /// without the key — a free Personal Team's — is no push (`None`).
    pub fn of_profile(profile: Option<&[u8]>) -> Option<ApnsEnvironment> {
        let Some(profile) = profile else {
            return Some(ApnsEnvironment::Production);
        };
        let key = b"<key>aps-environment</key>";
        let rest = &profile[find(profile, key)? + key.len()..];
        let open = b"<string>";
        let start = find(rest, open)?;
        if !rest[..start].iter().all(u8::is_ascii_whitespace) {
            return None;
        }
        let value = &rest[start + open.len()..];
        match value[..find(value, b"</string>")?].trim_ascii() {
            b"development" => Some(ApnsEnvironment::Development),
            b"production" => Some(ApnsEnvironment::Production),
            _ => None,
        }
    }

    /// The environment from reading the bundle's `embedded.mobileprovision`:
    /// [`Self::of_profile`] over its bytes, and over no profile when the file
    /// does not exist — the App Store's builds carry none. Any other read
    /// error is returned, never read as production: a development build
    /// whose profile could not be read would register its sandbox token with
    /// the production app, and every push to it would fail at Apple. The
    /// safe answer is no pusher, with the error in the log (R343).
    pub fn of_profile_read(
        read: std::io::Result<Vec<u8>>,
    ) -> std::io::Result<Option<ApnsEnvironment>> {
        match read {
            Ok(profile) => Ok(ApnsEnvironment::of_profile(Some(&profile))),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                Ok(ApnsEnvironment::of_profile(None))
            }
            Err(error) => Err(error),
        }
    }
}

/// Where `needle` first starts in `hay`.
fn find(hay: &[u8], needle: &[u8]) -> Option<usize> {
    hay.windows(needle.len())
        .position(|window| window == needle)
}

/// The pusher's identity on the homeserver: the token as Sygnal reads an
/// APNs pushkey (base64) and the environment's app.
pub fn pusher_ids(
    environment: ApnsEnvironment,
    token: &[u8],
) -> matrix_sdk::ruma::api::client::push::PusherIds {
    use base64::Engine as _;
    matrix_sdk::ruma::api::client::push::PusherIds::new(
        base64::engine::general_purpose::STANDARD.encode(token),
        environment.app_id().to_owned(),
    )
}

/// The pusher this phone registers on one account (98.1 #3): an `http`
/// pusher to the person's gateway, `event_id_only`, whose `default_payload`
/// asks iOS to run the notification extension (`mutable-content`), gives
/// the alert it shows when the extension cannot say more, and names the
/// local account whose store the extension opens ([`PAYLOAD_ACCOUNT`]).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PusherSpec {
    gateway: Gateway,
    environment: ApnsEnvironment,
    token: Vec<u8>,
    account: String,
    device: String,
}

impl PusherSpec {
    /// The pusher for `account` over `stored`, `push.gateway_url` as the
    /// registry holds it (raw: a synced `settings.toml` can carry any
    /// text). Read only through [`Gateway::parse`]: blank, absent or
    /// refused is no pusher, as is a build with no token yet (DW-1230).
    pub fn of_setting(
        stored: Option<&str>,
        environment: ApnsEnvironment,
        token: &[u8],
        account: &str,
        device: &str,
    ) -> Option<PusherSpec> {
        if token.is_empty() || account.is_empty() {
            return None;
        }
        let gateway = Gateway::parse(stored?).ok()??;
        Some(PusherSpec {
            gateway,
            environment,
            token: token.to_vec(),
            account: account.to_owned(),
            device: device.to_owned(),
        })
    }

    /// The pusher's identity ([`pusher_ids`]).
    pub fn ids(&self) -> matrix_sdk::ruma::api::client::push::PusherIds {
        pusher_ids(self.environment, &self.token)
    }

    /// The pusher as the homeserver's `POST /pushers/set` takes it.
    pub fn pusher(&self) -> matrix_sdk::ruma::api::client::push::Pusher {
        use matrix_sdk::ruma::api::client::push::{PusherInit, PusherKind};
        use matrix_sdk::ruma::push::{HttpPusherData, PushFormat};

        let mut data = HttpPusherData::new(self.gateway.as_str().to_owned());
        data.format = Some(PushFormat::EventIdOnly);
        data.data.insert(
            "default_payload".to_owned(),
            serde_json::json!({
                "aps": {
                    "mutable-content": 1,
                    "alert": { "title": GENERIC_TITLE, "body": GENERIC_BODY },
                },
                PAYLOAD_ACCOUNT: self.account,
            }),
        );
        PusherInit {
            ids: self.ids(),
            kind: PusherKind::Http(data),
            app_display_name: "keeper".to_owned(),
            device_display_name: self.device.clone(),
            profile_tag: None,
            lang: "en".to_owned(),
        }
        .into()
    }
}

/// What [`register`] did on one account.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Registered {
    /// The pusher is set.
    Set,
    /// Any pusher of this phone's is removed.
    Removed,
}

/// Whether `client`'s person is in any agent session room: the accounts a
/// phone registers its pusher on — on any other, every event would wake the
/// extension only for it to show nothing (DW-408).
pub fn holds_session_rooms(client: &matrix_sdk::Client) -> bool {
    client.joined_rooms().iter().any(|room| {
        crate::agents::room::AgentRoomKind::of(room.room_type().as_ref())
            == Some(crate::agents::room::AgentRoomKind::Session)
    })
}

/// Set `spec` on `client`'s account when it holds agent session rooms,
/// else remove the pusher `ids` names. `append`: one phone signed in to two
/// accounts on one homeserver registers the same token and app twice, and
/// without it the second registration would remove the first.
pub async fn register(
    client: &matrix_sdk::Client,
    ids: matrix_sdk::ruma::api::client::push::PusherIds,
    spec: Option<&PusherSpec>,
) -> Result<Registered, matrix_sdk::Error> {
    match spec.filter(|_| holds_session_rooms(client)) {
        Some(spec) => {
            client.pusher().set(spec.pusher(), true).await?;
            Ok(Registered::Set)
        }
        None => {
            client.pusher().delete(ids).await?;
            Ok(Registered::Removed)
        }
    }
}

// ---------------------------------------------------------------------------
// The lock screen's actions and taps (98.1 #4, UX-DR144; Q2, Q3)

/// How long a decision from the lock screen is tried before the person is
/// asked to finish it in keeper: iOS gives an action about thirty seconds.
pub const ACTION_BUDGET: std::time::Duration = std::time::Duration::from_secs(20);

/// What a re-posted notification says when a decision from the lock screen
/// could not be sent in [`ACTION_BUDGET`].
pub const FINISH_IN_KEEPER: &str = "Open keeper to finish deciding.";

/// What the app's notification delegate hands Rust when the person picks an
/// action: the notification's category, the action, and its `userInfo`.
#[derive(Debug, Clone, Deserialize)]
pub struct ActionTap {
    pub category: String,
    pub action: String,
    pub user_info: Value,
}

/// The decision an action sends: the account, the session room, the
/// request.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ActionDecision {
    pub account: String,
    pub room: String,
    pub req: crate::agents::approval_card::ApprovalDecideReq,
}

/// Why an action decides nothing.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ActionRefusal {
    /// The notification carries no approval of keeper's.
    #[error("This notification names no approval keeper showed.")]
    NoApproval,
    /// An action keeper never registered.
    #[error("keeper offers no such action.")]
    UnknownAction,
    /// An action the notification's category does not offer, or *Approve
    /// once* on a T4 record.
    #[error("This notification does not offer that; decide on the card in keeper.")]
    NotOffered,
}

/// The decision `tap` sends (98.1 #4, S-10): the record and digest the
/// notification showed, scope `once`, and only an action it offered — only
/// on a notification keeper composed ([`SHOWN_INFO`]: a push as the gateway
/// sent it decides nothing, whatever category and record it names), the
/// category iOS showed it with must be the one the extension chose for the
/// record, and [`NotificationAction::decide`] refuses anything that category
/// does not list and *Approve once* at T4, whatever else the notification
/// carried. What it returns is checked once more where it is sent: the record
/// must be in the session room with that digest
/// (`AccountManager::agent_approval_decide_notified`).
pub fn action_decision(tap: &ActionTap) -> Result<ActionDecision, ActionRefusal> {
    let account = composed_account(&tap.user_info).ok_or(ActionRefusal::NoApproval)?;
    let approval: PushApproval = serde_json::from_value(tap.user_info[APPROVAL_INFO].clone())
        .map_err(|_| ActionRefusal::NoApproval)?;
    let action = NotificationAction::of_id(&tap.action).ok_or(ActionRefusal::UnknownAction)?;
    if Category::of_id(&tap.category) != approval.category {
        return Err(ActionRefusal::NotOffered);
    }
    let req = action.decide(&approval).ok_or(ActionRefusal::NotOffered)?;
    Ok(ActionDecision {
        account: account.to_owned(),
        room: approval.room,
        req,
    })
}

/// The local account a notification keeper composed names: keeper's id (a
/// ULID) under [`PAYLOAD_ACCOUNT`] beside [`SHOWN_INFO`]; nothing for a push
/// as the gateway sent it.
fn composed_account(user_info: &Value) -> Option<&str> {
    if user_info[SHOWN_INFO] != Value::Bool(true) {
        return None;
    }
    user_info[PAYLOAD_ACCOUNT]
        .as_str()
        .filter(|account| ulid::Ulid::from_string(account).is_ok())
}

/// Where tapping a keeper notification opens (UX-DR144): its account and
/// session room.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct PushOpenVm {
    pub account_id: String,
    pub room_id: String,
}

/// Where tapping the notification whose `userInfo` this is opens: the
/// approval's room, else the room the push named, in the account it names;
/// `None` for a notification keeper did not compose ([`SHOWN_INFO`]).
pub fn tap_target(user_info: &Value) -> Option<PushOpenVm> {
    let account = composed_account(user_info)?;
    let room = user_info[APPROVAL_INFO]["room"]
        .as_str()
        .or_else(|| user_info["room_id"].as_str())?;
    matrix_sdk::ruma::RoomId::parse(room).ok()?;
    Some(PushOpenVm {
        account_id: account.to_owned(),
        room_id: room.to_owned(),
    })
}

/// The categories the app registers with iOS, as its delegate's Swift reads
/// them: each with its actions in order, every action needing the phone
/// unlocked and *Deny* destructive (UX-DR144).
pub fn registered_categories() -> Value {
    [Category::Approval, Category::ApprovalOpen]
        .into_iter()
        .map(|category| {
            serde_json::json!({
                "id": category.id(),
                "actions": category.actions().iter().map(|action| serde_json::json!({
                    "id": action.id(),
                    "label": action.label(),
                    "destructive": action.destructive(),
                })).collect::<Vec<_>>(),
            })
        })
        .collect()
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

    // --- The pusher, the lock screen's actions and taps (98.1 #3, #4) ---

    const ACCOUNT: &str = "01J9ZK0000000000000000ACCT";

    /// 98.1 #3: the pusher carries nothing but an id — `http`,
    /// `event_id_only`, the app of the build's APNs environment, the token
    /// as base64, the gateway as `Gateway::parse` reads it, and a
    /// `default_payload` that runs the extension, says keeper's own words if
    /// it cannot say more, and names the local account. A setting that is
    /// not a gateway keeper registers — `http://`, a user name, blank,
    /// absent — and a build with no token yet register nothing (DW-1230).
    #[test]
    fn pusher_spec() {
        let token = [0xde, 0xad, 0xbe, 0xef];
        let spec = PusherSpec::of_setting(
            Some(" https://push.example.org "),
            ApnsEnvironment::Development,
            &token,
            ACCOUNT,
            "kalypso",
        )
        .expect("a gateway keeper registers");
        let pusher = serde_json::to_value(spec.pusher()).expect("serialises");
        assert_eq!(pusher["kind"], "http");
        assert_eq!(pusher["app_id"], "dev.tgorka.keeper.ios.dev");
        assert_eq!(pusher["pushkey"], "3q2+7w==");
        assert_eq!(pusher["device_display_name"], "kalypso");
        assert_eq!(
            pusher["data"],
            json!({
                "url": "https://push.example.org/_matrix/push/v1/notify",
                "format": "event_id_only",
                "default_payload": {
                    "aps": {
                        "mutable-content": 1,
                        "alert": {"title": GENERIC_TITLE, "body": GENERIC_BODY},
                    },
                    "keeper_account": ACCOUNT,
                },
            })
        );
        let production = PusherSpec::of_setting(
            Some("https://push.example.org"),
            ApnsEnvironment::Production,
            &token,
            ACCOUNT,
            "kalypso",
        )
        .expect("production");
        assert_eq!(
            serde_json::to_value(production.pusher()).expect("serialises")["app_id"],
            "dev.tgorka.keeper.ios"
        );
        assert_eq!(
            production.ids(),
            pusher_ids(ApnsEnvironment::Production, &token)
        );

        for refused in [
            Some("http://push.example.org/_matrix/push/v1/notify"),
            Some("https://me:hunter2@push.example.org"),
            Some("push.example.org"),
            Some("  "),
            None,
        ] {
            assert_eq!(
                PusherSpec::of_setting(
                    refused,
                    ApnsEnvironment::Development,
                    &token,
                    ACCOUNT,
                    "kalypso"
                ),
                None,
                "{refused:?}"
            );
        }
        assert_eq!(
            PusherSpec::of_setting(
                Some("https://push.example.org"),
                ApnsEnvironment::Development,
                &[],
                ACCOUNT,
                "kalypso"
            ),
            None,
            "no token yet"
        );
    }

    /// Q4: the app id follows the build's signature — the embedded profile's
    /// `aps-environment`; no profile is an App Store build; a profile with
    /// no push entitlement registers nothing.
    #[test]
    fn the_app_id_follows_the_profiles_aps_environment() {
        let profile = |entitlements: &str| {
            format!(
                "\u{30}\u{82}binary<?xml version=\"1.0\"?><plist><dict>\
                 <key>Entitlements</key><dict>{entitlements}</dict></dict></plist>\u{a0}"
            )
            .into_bytes()
        };
        assert_eq!(
            ApnsEnvironment::of_profile(Some(&profile(
                "<key>aps-environment</key>\n\t<string>development</string>"
            ))),
            Some(ApnsEnvironment::Development)
        );
        assert_eq!(
            ApnsEnvironment::of_profile(Some(&profile(
                "<key>aps-environment</key><string>production</string>"
            ))),
            Some(ApnsEnvironment::Production)
        );
        assert_eq!(
            ApnsEnvironment::of_profile(None),
            Some(ApnsEnvironment::Production)
        );
        for none in [
            "<key>get-task-allow</key><true/>",
            "<key>aps-environment</key><string>staging</string>",
            "<key>aps-environment</key><true/><string>development</string>",
        ] {
            assert_eq!(
                ApnsEnvironment::of_profile(Some(&profile(none))),
                None,
                "{none}"
            );
        }
    }

    /// R343 (review 98apns-1, finding 05): only a missing profile is an App
    /// Store build; a profile that exists but cannot be read is an error the
    /// shell logs, registering no pusher — never production by default.
    #[test]
    fn an_unreadable_profile_is_no_environment_not_production() {
        use std::io::{Error, ErrorKind};
        assert_eq!(
            ApnsEnvironment::of_profile_read(Err(Error::from(ErrorKind::NotFound))).ok(),
            Some(Some(ApnsEnvironment::Production))
        );
        assert_eq!(
            ApnsEnvironment::of_profile_read(Ok(
                b"<key>aps-environment</key><string>development</string>".to_vec()
            ))
            .ok(),
            Some(Some(ApnsEnvironment::Development))
        );
        for kind in [
            ErrorKind::PermissionDenied,
            ErrorKind::Interrupted,
            ErrorKind::Other,
        ] {
            let read = ApnsEnvironment::of_profile_read(Err(Error::from(kind)));
            assert_eq!(read.map_err(|error| error.kind()), Err(kind), "{kind:?}");
        }
    }

    /// A tap on `action` of a notification iOS showed with `category`, whose
    /// record the extension showed with that same category.
    fn tap(category: &str, action: &str, tier: u8) -> ActionTap {
        ActionTap {
            category: category.to_owned(),
            action: action.to_owned(),
            user_info: json!({
                "keeper_account": ACCOUNT,
                "keeper_shown": true,
                "room_id": ROOM,
                "keeper_approval": {
                    "room": ROOM, "id": "01A", "binding_digest": "d1", "tier": tier,
                    "category": Category::of_id(category),
                },
            }),
        }
    }

    /// 98.1 #4, S-10: an action decides only what its notification's
    /// category offers — *Approve once* on `KEEPER_APPROVAL` below T4,
    /// *Deny* on both — for the record and digest shown, scope `once`;
    /// *Approve once* on `KEEPER_APPROVAL_OPEN` or on a T4 record, a
    /// category iOS names other than the one the record was shown with, an
    /// action keeper never registered, and a notification without keeper's
    /// record or account decide nothing.
    #[test]
    fn a_lock_screen_action_decides_only_what_its_category_offers() {
        let approved =
            action_decision(&tap("KEEPER_APPROVAL", "KEEPER_APPROVE_ONCE", 3)).expect("offered");
        assert_eq!(approved.account, ACCOUNT);
        assert_eq!(approved.room, ROOM);
        assert_eq!(approved.req.id, "01A");
        assert_eq!(approved.req.binding_digest, "d1");
        assert_eq!(approved.req.decision, Decision::Approve);
        assert_eq!(approved.req.scope, Scope::Once);
        for category in ["KEEPER_APPROVAL", "KEEPER_APPROVAL_OPEN"] {
            let denied = action_decision(&tap(category, "KEEPER_DENY", 4)).expect("deny");
            assert_eq!(denied.req.decision, Decision::Deny, "{category}");
            assert_eq!(denied.req.scope, Scope::Once, "{category}");
        }

        assert_eq!(
            action_decision(&tap("KEEPER_APPROVAL_OPEN", "KEEPER_APPROVE_ONCE", 2)),
            Err(ActionRefusal::NotOffered)
        );
        assert_eq!(
            action_decision(&tap("KEEPER_APPROVAL", "KEEPER_APPROVE_ONCE", 4)),
            Err(ActionRefusal::NotOffered)
        );
        assert_eq!(
            action_decision(&tap("SOMETHING_ELSE", "KEEPER_DENY", 2)),
            Err(ActionRefusal::NotOffered)
        );
        let mut relabelled = tap("KEEPER_APPROVAL_OPEN", "KEEPER_APPROVE_ONCE", 2);
        relabelled.user_info["keeper_approval"]["category"] = json!("KEEPER_APPROVAL");
        assert_eq!(
            action_decision(&relabelled),
            Err(ActionRefusal::NotOffered),
            "shown with Deny only, whatever the userInfo says it was"
        );
        assert_eq!(
            action_decision(&tap("KEEPER_APPROVAL", "KEEPER_APPROVE_SESSION", 2)),
            Err(ActionRefusal::UnknownAction)
        );
        let mut foreign = tap("KEEPER_APPROVAL", "KEEPER_DENY", 2);
        foreign.user_info["keeper_account"] = json!("../other");
        assert_eq!(action_decision(&foreign), Err(ActionRefusal::NoApproval));
        let mut unshown = tap("KEEPER_APPROVAL", "KEEPER_DENY", 2);
        unshown.user_info["keeper_approval"] = json!({"room": ROOM, "id": "01A"});
        assert_eq!(action_decision(&unshown), Err(ActionRefusal::NoApproval));
    }

    /// R343 (review 98apns-1, finding 01): a push as the gateway sent it —
    /// a keeper account, a keeper category and a record with a digest, for
    /// an event the extension could not read — decides nothing, whichever
    /// action iOS reports: the extension never composed it, so it carries no
    /// `keeper_shown`, or carries one that is not keeper's `true`. Nor does
    /// a tap on it open a room.
    #[test]
    fn a_push_keeper_did_not_compose_decides_nothing() {
        let forged = json!({
            "aps": {"alert": {"title": "keeper", "body": "Rotate the prod keys"},
                    "category": "KEEPER_APPROVAL", "mutable-content": 1},
            "keeper_account": ACCOUNT,
            "room_id": ROOM,
            "event_id": "$bogus",
            "keeper_approval": {
                "room": ROOM, "id": "01A", "binding_digest": "d1", "tier": 1,
                "category": "KEEPER_APPROVAL",
            },
        });
        let mut planted = forged.clone();
        planted["keeper_shown"] = json!("true");
        for user_info in [forged, planted] {
            for action in ["KEEPER_APPROVE_ONCE", "KEEPER_DENY"] {
                let tap = ActionTap {
                    category: "KEEPER_APPROVAL".to_owned(),
                    action: action.to_owned(),
                    user_info: user_info.clone(),
                };
                assert_eq!(
                    action_decision(&tap),
                    Err(ActionRefusal::NoApproval),
                    "{action} on {user_info}"
                );
            }
            assert_eq!(tap_target(&user_info), None, "{user_info}");
        }
    }

    /// UX-DR144: tapping opens the session room — the approval's when the
    /// notification shows one, else the room the push named — in the account
    /// it names; a notification that is not keeper's push opens nothing.
    #[test]
    fn a_tap_opens_the_session_room_the_notification_names() {
        let open = |account_id: &str, room_id: &str| {
            Some(PushOpenVm {
                account_id: account_id.to_owned(),
                room_id: room_id.to_owned(),
            })
        };
        assert_eq!(
            tap_target(&json!({
                "keeper_account": ACCOUNT, "keeper_shown": true, "room_id": "!pushed:example.org",
                "keeper_approval": {"room": ROOM},
            })),
            open(ACCOUNT, ROOM)
        );
        assert_eq!(
            tap_target(&json!({"keeper_account": ACCOUNT, "keeper_shown": true, "room_id": ROOM})),
            open(ACCOUNT, ROOM)
        );
        for not_ours in [
            json!({"keeper_shown": true, "room_id": ROOM}),
            json!({"keeper_account": "smoke", "keeper_shown": true, "room_id": ROOM}),
            json!({"keeper_account": ACCOUNT, "keeper_shown": true}),
            json!({"keeper_account": ACCOUNT, "keeper_shown": true, "room_id": "not a room"}),
        ] {
            assert_eq!(tap_target(&not_ours), None, "{not_ours}");
        }
    }
}
