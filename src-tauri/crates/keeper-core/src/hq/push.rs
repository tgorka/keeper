//! A push from the agents server (AD-432, D-35, R-HQ-44; story 106.6):
//! what the phone registers, what a push names, what it shows, and what its
//! actions and taps do.
//!
//! The server's reaction posts **ids only** to the gateway the person runs —
//! the database, the session and the item, under [`PAYLOAD`] — and the phone
//! reads the one item it names with its own token
//! (`GET /<database>/push/item/{kind}/{id}`, [`PushedItem::url`]). What the
//! lock screen shows is decided here, on the phone, from that row:
//!
//! | item | shows |
//! |---|---|
//! | a pending approval this person may decide | keeper's words; *Approve once* and *Deny* only when the whole request fits the notification ([`fits`]) and is below T4, else *Deny* alone |
//! | an approval already decided, expired, or not this person's | keeper's words, no action |
//! | an answer: an agent's message naming no `kind` | the agent and its newest words — the final row is the answer's last word |
//! | anything else (an ask, a person's message, this person's own) | nothing |
//!
//! A row that cannot be read — no server or pairing on this device, the
//! server or tailnet unreachable, a refusal, an answer naming another item —
//! is the generic notification (`agents::push::GENERIC_TITLE`), with no
//! actions. The same item may arrive twice; a notification is the item's,
//! so the second replaces the first ([`replaces`], [`PushedItem::tag`]).
//!
//! The push target the server posts to — the person's gateway, this
//! device's token and app — is [`PushTarget`], registered through
//! `PUT /clients/self/push-target` ([`crate::hq::Hq::set_push_target`]); a
//! cleared `push.gateway_url` removes it.

use std::collections::BTreeMap;
use std::path::Path;

use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};

use crate::agents::approval_card::tier_word;
use crate::agents::push::{
    misreads, open_body, shown_name, ActionRefusal, ActionTap, ApnsEnvironment, Category, Gateway,
    NotificationAction, PushOpenVm, BODY_MAX_CHARS, SHOWN_INFO,
};
use crate::hq::routes::{is_database_name, Routes};
use crate::hq::server::ServerUrl;
use crate::hq::sessions::{int, json as json_column, names, opt_text, text, AgentSessionRef};
use crate::hq::{lock, Hq, HqError};

/// The key the server's reaction names the item under, in the gateway's
/// `default_payload` (iOS reads it from `userInfo`).
pub const PAYLOAD: &str = "keeper_hq";

/// The key only a notification keeper composed carries the item under — for
/// a tap ([`tap_target`]) and for folding a second delivery ([`replaces`]).
pub const ITEM_INFO: &str = "keeper_hq_item";

/// The key an approval's notification carries the shown request under, for
/// the action handler ([`action_decision`]).
pub const APPROVAL_INFO: &str = "keeper_hq_approval";

/// The longest id keeper reads from a push: the server pushes no item whose
/// serialized ids pass 1 KiB.
const ID_MAX: usize = 1024;

/// What a pushed item is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ItemKind {
    Approval,
    Message,
}

impl ItemKind {
    pub fn as_str(self) -> &'static str {
        match self {
            ItemKind::Approval => "approval",
            ItemKind::Message => "message",
        }
    }
}

/// The item a push names: its database, session, kind and id — nothing else
/// (D-35).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PushedItem {
    pub database: String,
    pub session_id: String,
    pub kind: ItemKind,
    pub item_id: String,
}

fn is_id(id: &str) -> bool {
    !id.is_empty() && id.len() <= ID_MAX && !id.chars().any(misreads)
}

impl PushedItem {
    /// The item `payload` (the `keeper_hq` object) names; `None` for anything
    /// that is not exactly one: a database name keeper would not put in a
    /// path, an empty or overlong id, an unknown kind.
    pub fn of_payload(payload: &Value) -> Option<PushedItem> {
        let item: PushedItem = serde_json::from_value(payload.clone()).ok()?;
        (is_database_name(&item.database) && is_id(&item.session_id) && is_id(&item.item_id))
            .then_some(item)
    }

    /// The item a UnifiedPush message names: the gateway's `notify` body as
    /// the distributor forwards it, its one device's `default_payload`.
    pub fn of_notify(message: &[u8]) -> Option<PushedItem> {
        let message: Value = serde_json::from_slice(message).ok()?;
        Self::of_payload(&message["notification"]["devices"][0]["data"]["default_payload"][PAYLOAD])
    }

    /// Where its row is read: `<server>/<database>/push/item/<kind>/<id>`,
    /// the id one escaped path segment.
    pub fn url(&self, server: &ServerUrl) -> Option<url::Url> {
        let mut url = url::Url::parse(&server.mount(&self.database)).ok()?;
        url.path_segments_mut().ok()?.pop_if_empty().extend([
            "push",
            "item",
            self.kind.as_str(),
            &self.item_id,
        ]);
        Some(url)
    }

    /// One notification per item: a second delivery of it replaces the
    /// first (Android's tag; the iPhone's [`replaces`]).
    pub fn tag(&self) -> String {
        format!("{}/{}/{}", self.database, self.kind.as_str(), self.item_id)
    }

    /// The session the item is in.
    pub fn session(&self) -> AgentSessionRef {
        AgentSessionRef {
            database: self.database.clone(),
            session_id: self.session_id.clone(),
        }
    }

    fn same_item(&self, other: &PushedItem) -> bool {
        self.database == other.database && self.kind == other.kind && self.item_id == other.item_id
    }
}

/// The row the server answered for `pushed` (`{database, kind, session_id,
/// item_id, row}`), when the answer is about exactly that item; else why it
/// is not.
pub fn item_row(pushed: &PushedItem, body: &[u8]) -> Result<Map<String, Value>, String> {
    let answer: Value =
        serde_json::from_slice(body).map_err(|_| "the server's answer is not JSON".to_owned())?;
    let Value::Object(row) = answer["row"].clone() else {
        return Err("the server's answer holds no row".to_owned());
    };
    let named = answer["database"] == pushed.database.as_str()
        && answer["kind"] == pushed.kind.as_str()
        && answer["session_id"] == pushed.session_id.as_str()
        && answer["item_id"] == pushed.item_id.as_str()
        && text(&row, "id") == pushed.item_id
        && text(&row, "session_id") == pushed.session_id;
    if !named {
        return Err("the server answered about another item".to_owned());
    }
    Ok(row)
}

/// The approval a notification shows, as its `userInfo` carries it to the
/// action handler: the session it is in, the request, the digest the
/// decision must carry, its tier, and the category it was shown with
/// (`None`: no actions).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HqPushApproval {
    pub database: String,
    pub session_id: String,
    pub id: String,
    pub binding_digest: String,
    pub tier: u8,
    pub category: Option<Category>,
}

/// What the person sees for an item.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HqShown {
    pub title: String,
    pub subtitle: Option<String>,
    pub body: String,
    /// `None`: no actions; a tap opens the session.
    pub category: Option<Category>,
    /// Notifications of one session group together.
    pub thread_id: String,
    /// The item, for a tap and for folding a second delivery.
    pub item: PushedItem,
    /// The request an approval's actions decide.
    pub approval: Option<HqPushApproval>,
}

/// What a read item comes to on the lock screen.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HqClassified {
    Show(Box<HqShown>),
    Drop,
}

/// The tier a row names (`T0`…`T4`); `None` for anything else.
fn tier_of(row: &Map<String, Value>) -> Option<u8> {
    text(row, "tier")
        .strip_prefix('T')
        .and_then(|n| n.parse::<u8>().ok())
        .filter(|n| *n <= 4)
}

/// Whether a notification may offer *Approve once* on the approval `row`:
/// the person sees the whole request there and it is below T4. The item the
/// server pushes is the approval row alone, so the request is its `summary`
/// — keeper's own sentence for the call, written by the runtime — shown as
/// the body: one line of at most [`BODY_MAX_CHARS`] characters with no line
/// break, other control character or bidirectional override. An unknown
/// tier is never below T4.
pub fn fits(row: &Map<String, Value>) -> bool {
    let summary = text(row, "summary");
    tier_of(row).is_some_and(|tier| tier < 4)
        && !summary.trim().is_empty()
        && !summary.chars().any(misreads)
        && summary.chars().count() <= BODY_MAX_CHARS
        && opt_text(row, "tool_call_id").is_some()
}

/// The words of an answer whose text is not in its row.
fn answered(agent: &str) -> String {
    format!("{agent} answered; open keeper to read it.")
}

/// What `row` — the item `pushed` names, read with `own`'s token — shows at
/// `now_ms`.
pub fn classify(
    pushed: &PushedItem,
    row: &Map<String, Value>,
    own: &str,
    now_ms: i64,
) -> HqClassified {
    let author = text(row, "author_actor");
    let agent = shown_name(author, author);
    let shown = |title: String, subtitle, body, category, approval| {
        HqClassified::Show(Box::new(HqShown {
            title,
            subtitle,
            body,
            category,
            thread_id: format!("{}/{}", pushed.database, pushed.session_id),
            item: pushed.clone(),
            approval,
        }))
    };
    match pushed.kind {
        ItemKind::Approval => {
            let title = format!("{agent} asks for a decision");
            let tier = tier_of(row);
            let pending = text(row, "state") == "pending"
                && int(row, "expires_at_ms").is_some_and(|at| at > now_ms);
            if !pending {
                let body = match text(row, "state") {
                    "pending" | "expired" => "It expired before anyone decided.",
                    _ => "It was already decided; open keeper to see how.",
                };
                return shown(title, None, body.to_owned(), None, None);
            }
            let approval = |category| {
                Some(HqPushApproval {
                    database: pushed.database.clone(),
                    session_id: pushed.session_id.clone(),
                    id: pushed.item_id.clone(),
                    binding_digest: text(row, "binding_digest").to_owned(),
                    tier: tier.unwrap_or(4),
                    category,
                })
            };
            let open = |tier: Option<u8>| match tier {
                Some(tier) => open_body(tier),
                None => "Open keeper to read the whole request.".to_owned(),
            };
            if !names(row, "approvers").iter().any(|a| a == own)
                || text(row, "binding_digest").is_empty()
            {
                // Not this person's to decide here: no action; a tap opens
                // the session, where the card says why.
                return shown(title, None, open(tier), None, approval(None));
            }
            if fits(row) {
                shown(
                    title,
                    tier.map(tier_word),
                    text(row, "summary").to_owned(),
                    Some(Category::Approval),
                    approval(Some(Category::Approval)),
                )
            } else {
                shown(
                    title,
                    None,
                    open(tier),
                    Some(Category::ApprovalOpen),
                    approval(Some(Category::ApprovalOpen)),
                )
            }
        }
        ItemKind::Message => {
            let is_answer = text(row, "role") == "agent"
                && json_column(row, "metadata")
                    .get("kind")
                    .is_none_or(Value::is_null)
                && !author.is_empty()
                && author != own;
            if !is_answer {
                return HqClassified::Drop;
            }
            let words = text(row, "body");
            let body =
                if words.trim().is_empty() || words.trim() == crate::agents::events::PLACEHOLDER {
                    answered(agent)
                } else {
                    words.to_owned()
                };
            shown(agent.to_owned(), None, body, None, None)
        }
    }
}

/// A notification already on the lock screen, as the extension hands it
/// over: its identifier, and the item it shows when keeper composed it.
#[derive(Debug, Clone, Deserialize)]
pub struct Delivered {
    pub id: String,
    #[serde(default)]
    pub item: Value,
}

/// The notifications on the lock screen that show `pushed` already: a
/// second delivery of an item replaces them, so the item is shown once.
pub fn replaces(delivered: &[Delivered], pushed: &PushedItem) -> Vec<String> {
    delivered
        .iter()
        .filter(|one| PushedItem::of_payload(&one.item).is_some_and(|item| item.same_item(pushed)))
        .map(|one| one.id.clone())
        .collect()
}

/// The `userInfo` keys keeper sets on a notification it composed for
/// `shown`: [`SHOWN_INFO`], the item and, for an approval, the request.
pub fn composed_info(shown: &HqShown) -> Value {
    let mut info = json!({
        SHOWN_INFO: true,
        ITEM_INFO: shown.item,
    });
    if let Some(approval) = &shown.approval {
        info[APPROVAL_INFO] = json!(approval);
    }
    info
}

/// Whether `user_info` is a notification keeper composed for an agents
/// server item.
pub fn is_composed(user_info: &Value) -> bool {
    composed_item(user_info).is_some()
}

fn composed_item(user_info: &Value) -> Option<PushedItem> {
    if user_info[SHOWN_INFO] != Value::Bool(true) {
        return None;
    }
    PushedItem::of_payload(&user_info[ITEM_INFO])
}

/// Where tapping the notification whose `userInfo` this is opens: the
/// session of the item it showed; `None` for one keeper did not compose.
pub fn tap_target(user_info: &Value) -> Option<PushOpenVm> {
    composed_item(user_info).map(|item| PushOpenVm::Session {
        database: item.database,
        session_id: item.session_id,
    })
}

/// The keys a notification keeper re-posts after an action carries, so a
/// tap on it still opens the session: [`SHOWN_INFO`] and the item, nothing
/// that decides.
pub fn reposted_info(user_info: &Value) -> Option<Value> {
    composed_item(user_info).map(|item| json!({ SHOWN_INFO: true, ITEM_INFO: item }))
}

/// The decision an action on an item's notification sends.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HqActionDecision {
    pub session: AgentSessionRef,
    pub approval_id: String,
    pub binding_digest: String,
    pub approve: bool,
}

/// The decision `tap` sends: only on a notification keeper composed, only
/// an action its category offered — the category iOS showed it with must be
/// the one keeper chose — and never *Approve once* at T4, whatever else it
/// carries. It is checked once more where it is written: the request must be
/// in the session's replica with that digest, below T4 for an approval
/// ([`crate::hq::approvals::decide_pushed`]).
pub fn action_decision(tap: &ActionTap) -> Result<HqActionDecision, ActionRefusal> {
    let item = composed_item(&tap.user_info).ok_or(ActionRefusal::NoApproval)?;
    let approval: HqPushApproval = serde_json::from_value(tap.user_info[APPROVAL_INFO].clone())
        .map_err(|_| ActionRefusal::NoApproval)?;
    if item.kind != ItemKind::Approval
        || approval.database != item.database
        || approval.session_id != item.session_id
        || approval.id != item.item_id
    {
        return Err(ActionRefusal::NoApproval);
    }
    let action = NotificationAction::of_id(&tap.action).ok_or(ActionRefusal::UnknownAction)?;
    let offered = approval
        .category
        .filter(|category| Category::of_id(&tap.category) == Some(*category))
        .is_some_and(|category| category.actions().contains(&action));
    let approve = match action {
        NotificationAction::ApproveOnce if approval.tier >= 4 => {
            return Err(ActionRefusal::NotOffered)
        }
        NotificationAction::ApproveOnce => true,
        NotificationAction::Deny => false,
    };
    if !offered {
        return Err(ActionRefusal::NotOffered);
    }
    Ok(HqActionDecision {
        session: item.session(),
        approval_id: approval.id,
        binding_digest: approval.binding_digest,
        approve,
    })
}

// ---------------------------------------------------------------------------
// The device credential the extension reads with

/// The token and actor of this device's pairing with the server that holds
/// `pushed`'s database, chosen from `pairings` — the keychain's
/// `agents-server/<base URL>` items, as the notification extension reads
/// them — by the replica this device holds of that database under
/// `data_dir`. Reads only; `None` when no pairing, or more than one, does.
pub fn credential_for(
    data_dir: &Path,
    pushed: &PushedItem,
    pairings: &BTreeMap<String, String>,
) -> Result<(ServerUrl, String, String), String> {
    let mut found = Vec::new();
    for (key, stored) in pairings {
        let Some(raw) = key.strip_prefix(crate::hq::KEYCHAIN_PREFIX) else {
            continue;
        };
        let Ok(Some(server)) = ServerUrl::parse(raw) else {
            continue;
        };
        let Ok(pairing) = serde_json::from_str::<crate::hq::Pairing>(stored) else {
            continue;
        };
        let holds =
            crate::hq::databases::replica(data_dir, &server, &pushed.database, &pairing.actor_id)
                .is_file();
        let removed = crate::hq::databases::Authorization::load(data_dir, &server).removed;
        if holds && !removed {
            found.push((server, pairing.token, pairing.actor_id));
        }
    }
    match found.len() {
        1 => found.pop().ok_or_else(|| "no pairing".to_owned()),
        0 => Err(format!(
            "this device is paired with no agents server that holds {}",
            pushed.database
        )),
        _ => Err(format!(
            "more than one agents server this device is paired with holds {}",
            pushed.database
        )),
    }
}

// ---------------------------------------------------------------------------
// The push target (106.6; 103.4's `PUT /clients/self/push-target`)

/// Which push service a target is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum PushPlatform {
    Ios,
    Android,
}

/// Where the server's reaction posts for this device: the person's gateway
/// (read only through [`Gateway::parse`]), this device's pushkey and the app
/// the gateway serves it as.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PushTarget {
    platform: PushPlatform,
    pushkey: String,
    app_id: String,
    gateway: Gateway,
}

impl PushTarget {
    /// The iPhone's: `push.gateway_url` as stored (raw — a synced settings
    /// file can carry any text), the APNs token as Sygnal reads a pushkey
    /// (base64), the environment's app. Blank, absent or refused gateway, or
    /// no token yet, is no target — which removes one.
    pub fn apns(
        stored: Option<&str>,
        environment: ApnsEnvironment,
        token: &[u8],
    ) -> Option<PushTarget> {
        if token.is_empty() {
            return None;
        }
        let gateway = Gateway::parse(stored?).ok()??;
        let ids = crate::agents::push::pusher_ids(environment, token);
        Some(PushTarget {
            platform: PushPlatform::Ios,
            pushkey: ids.pushkey,
            app_id: ids.app_id,
            gateway,
        })
    }

    /// Android's: the Matrix gateway discovery found on the UnifiedPush
    /// endpoint's own server, and the endpoint as the pushkey.
    pub fn unifiedpush(
        gateway: Gateway,
        endpoint: &crate::agents::unifiedpush::Endpoint,
    ) -> PushTarget {
        PushTarget {
            platform: PushPlatform::Android,
            pushkey: endpoint.as_str().to_owned(),
            app_id: crate::agents::unifiedpush::APP_ID.to_owned(),
            gateway,
        }
    }

    /// The body of `PUT /clients/self/push-target`.
    pub fn body(&self) -> Value {
        json!({
            "platform": self.platform,
            "pushkey": self.pushkey,
            "app_id": self.app_id,
            "gateway_url": self.gateway.as_str(),
        })
    }
}

// ---------------------------------------------------------------------------
// Reading an item, registering the target, deciding from the lock screen

/// Read the item `pushed` names from `server` with `token` — one request —
/// and classify it for `own` at `now_ms`. `Err` is why it could not be
/// read: the generic notification.
pub async fn read_item(
    routes: &dyn Routes,
    server: &ServerUrl,
    token: &str,
    own: &str,
    pushed: &PushedItem,
    now_ms: i64,
) -> Result<HqClassified, String> {
    let body = routes
        .push_item(server, token, pushed)
        .await
        .map_err(|error| error.to_string())?;
    let row = item_row(pushed, &body)?;
    Ok(classify(pushed, &row, own, now_ms))
}

impl Hq {
    /// Where the server's push reaction posts for this device; `None`
    /// removes it. Sent now when the device is paired and not removed, and
    /// again whenever a discovery opens it to a server that has not taken
    /// this wish from this device.
    pub fn set_push_target(&self, target: Option<PushTarget>) {
        lock(&self.inner).push_target = Some(target);
        self.tell_push_target();
    }

    /// Send the push target wish, in the background, unless the server
    /// already took it from this device. Requests go one at a time, each
    /// sending the wish as it is when its turn comes.
    pub(super) fn tell_push_target(&self) {
        let Ok(runtime) = tokio::runtime::Handle::try_current() else {
            tracing::debug!("no runtime yet: the push target goes at the next discovery");
            return;
        };
        let me = self.me.clone();
        let task = runtime.spawn(async move {
            let Some(hq) = me.upgrade() else { return };
            let _turn = hq.push_turn.lock().await;
            let (server, pairing, wanted, told) = {
                let inner = lock(&hq.inner);
                let (Some(wanted), Some(server), Some(pairing)) = (
                    inner.push_target.clone(),
                    inner.server.clone(),
                    inner.pairing.clone(),
                ) else {
                    return;
                };
                if inner.authorization.removed {
                    return;
                }
                let told = (
                    server.as_str().to_owned(),
                    pairing.client_id.clone(),
                    wanted.clone(),
                );
                if inner.push_told.as_ref() == Some(&told) {
                    return;
                }
                (server, pairing, wanted, told)
            };
            let answer = {
                let _contact = hq.contacts.hold(&server);
                hq.routes
                    .set_push_target(&server, &pairing.token, wanted.as_ref())
                    .await
            };
            match answer {
                Ok(()) => {
                    tracing::info!(
                        registered = wanted.is_some(),
                        "the agents server took this device's push target"
                    );
                    lock(&hq.inner).push_told = Some(told);
                }
                Err(error) => tracing::warn!(
                    %error,
                    "the agents server did not take this device's push target; it is sent again at the next discovery"
                ),
            }
        });
        lock(&self.inner).tasks.push(task);
    }

    /// Read and classify the item a push names, with this device's pairing
    /// — the app's half on Android, where the push reaches keeper itself.
    pub async fn read_pushed(&self, pushed: &PushedItem) -> Result<HqClassified, String> {
        let (server, pairing) = self.paired().map_err(|error| error.to_string())?;
        let _contact = self.contacts.hold(&server);
        read_item(
            self.routes.as_ref(),
            &server,
            &pairing.token,
            &pairing.actor_id,
            pushed,
            crate::hq::sessions::now_ms(),
        )
        .await
    }

    /// Write the decision an action on a notification sends, through the
    /// session's replica, waiting up to `budget` for the request to be here
    /// — after a background launch the replica has only just started
    /// syncing — and for the server to take it. Blocking.
    pub fn decide_pushed(
        &self,
        decision: &HqActionDecision,
        budget: std::time::Duration,
    ) -> Result<crate::hq::approvals::AgentDecisionOutcomeVm, HqError> {
        let outcome = crate::hq::approvals::decide_pushed(
            || self.source(&decision.session.database).ok(),
            self.decisions()?,
            decision,
            std::time::Instant::now() + budget,
        )
        .map_err(|refusal| HqError::Sentence(refusal.to_string()));
        self.touch();
        outcome
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const ANA: &str = "ana";

    fn pushed(kind: ItemKind) -> PushedItem {
        PushedItem {
            database: "tgdrive".to_owned(),
            session_id: "ana.1.3".to_owned(),
            kind,
            item_id: "01JAPPROVAL0000000000000001".to_owned(),
        }
    }

    fn row(values: Value) -> Map<String, Value> {
        let Value::Object(map) = values else {
            panic!("an object")
        };
        map
    }

    /// An approval row as the server's item route answers it.
    fn approval(tier: &str, summary: &str, state: &str) -> Map<String, Value> {
        row(json!({
            "id": "01JAPPROVAL0000000000000001", "session_id": "ana.1.3",
            "page_key": "ana.1.3/2026-10", "tool_call_id": "call-1", "tier": tier,
            "binding_digest": "d1", "summary": summary, "approvers": "[\"ana\"]",
            "state": state, "decided_by_actor": null, "decided_by_client": null,
            "consumed_by_client": null, "expires_at_ms": 2_000, "created_at_ms": 1_000,
            "author_actor": "nixie", "author_client": "nixie.electra",
        }))
    }

    fn show(classified: HqClassified) -> HqShown {
        match classified {
            HqClassified::Show(shown) => *shown,
            HqClassified::Drop => panic!("dropped"),
        }
    }

    #[test]
    fn a_push_names_an_item_by_ids_and_nothing_else_is_read_as_one() {
        let payload = json!({"database": "tgdrive", "session_id": "ana.1.3",
                             "kind": "approval", "item_id": "01X"});
        assert_eq!(
            PushedItem::of_payload(&payload).map(|p| p.tag()),
            Some("tgdrive/approval/01X".to_owned())
        );
        for bad in [
            json!({"database": "../x", "session_id": "s", "kind": "approval", "item_id": "i"}),
            json!({"database": "tgdrive", "session_id": "", "kind": "approval", "item_id": "i"}),
            json!({"database": "tgdrive", "session_id": "s", "kind": "ask", "item_id": "i"}),
            json!({"database": "tgdrive", "session_id": "s", "kind": "message", "item_id": "a\nb"}),
            json!({"database": "tgdrive", "session_id": "s", "kind": "message",
                   "item_id": "x".repeat(ID_MAX + 1)}),
        ] {
            assert_eq!(PushedItem::of_payload(&bad), None, "{bad}");
        }
        // Android: the gateway's notify body as ntfy forwards it.
        let notify = json!({"notification": {"event_id": "01X", "room_id": "ana.1.3",
            "devices": [{"app_id": "a", "pushkey": "p", "data": {"format": "event_id_only",
            "default_payload": {"keeper_hq": payload}}}]}});
        assert_eq!(
            PushedItem::of_notify(notify.to_string().as_bytes()),
            PushedItem::of_payload(&payload)
        );
        assert_eq!(PushedItem::of_notify(b"{\"notification\":{}}"), None);
    }

    #[test]
    fn the_item_is_read_at_its_own_database_with_its_id_one_path_segment() {
        let server = ServerUrl::parse("https://hq.example.org/agents/")
            .expect("parses")
            .expect("an address");
        let mut item = pushed(ItemKind::Message);
        item.item_id = "a/b c".to_owned();
        assert_eq!(
            item.url(&server).map(String::from),
            Some("https://hq.example.org/agents/tgdrive/push/item/message/a%2Fb%20c".to_owned())
        );
    }

    #[test]
    fn an_answer_about_another_item_is_not_read() {
        let item = pushed(ItemKind::Approval);
        let answer = |session: &str, row_id: &str| {
            json!({"database": "tgdrive", "kind": "approval", "session_id": session,
                   "item_id": item.item_id, "row": {"id": row_id, "session_id": session}})
            .to_string()
        };
        assert!(item_row(&item, answer("ana.1.3", &item.item_id).as_bytes()).is_ok());
        assert!(item_row(&item, answer("ana.1.4", &item.item_id).as_bytes()).is_err());
        assert!(item_row(&item, answer("ana.1.3", "other").as_bytes()).is_err());
        assert!(item_row(&item, b"<html>").is_err());
        let mut neuradrive = item.clone();
        neuradrive.database = "neuradrive".to_owned();
        assert!(item_row(&neuradrive, answer("ana.1.3", &item.item_id).as_bytes()).is_err());
    }

    #[test]
    fn an_approval_that_fits_below_t4_offers_approve_once_in_keepers_words() {
        let shown = show(classify(
            &pushed(ItemKind::Approval),
            &approval("T2", "Run `git status` in keeper", "pending"),
            ANA,
            1_500,
        ));
        assert_eq!(shown.title, "nixie asks for a decision");
        assert_eq!(shown.subtitle.as_deref(), Some(tier_word(2).as_str()));
        assert_eq!(shown.body, "Run `git status` in keeper");
        assert_eq!(shown.category, Some(Category::Approval));
        assert_eq!(shown.thread_id, "tgdrive/ana.1.3");
        let approval = shown.approval.expect("the request");
        assert_eq!(
            (
                approval.id.as_str(),
                approval.binding_digest.as_str(),
                approval.tier
            ),
            ("01JAPPROVAL0000000000000001", "d1", 2)
        );
        assert_eq!(approval.category, Some(Category::Approval));
    }

    #[test]
    fn t4_a_long_or_misreading_request_or_an_unknown_tier_offers_deny_alone() {
        let long = "x".repeat(BODY_MAX_CHARS + 1);
        let exact = "x".repeat(BODY_MAX_CHARS);
        for (tier, summary) in [
            ("T4", "Delete the backups"),
            ("T2", long.as_str()),
            ("T2", "line one\nline two"),
            ("T2", "safe \u{202e}txt.exe"),
            ("T9", "list files"),
            ("high", "list files"),
            ("T2", "   "),
        ] {
            let shown = show(classify(
                &pushed(ItemKind::Approval),
                &approval(tier, summary, "pending"),
                ANA,
                1_500,
            ));
            assert_eq!(
                shown.category,
                Some(Category::ApprovalOpen),
                "{tier} {summary:?}"
            );
            assert!(
                shown.body.to_lowercase().contains("open keeper"),
                "{}",
                shown.body
            );
            assert_eq!(
                shown.approval.and_then(|a| a.category),
                Some(Category::ApprovalOpen)
            );
        }
        // The boundary itself fits.
        let shown = show(classify(
            &pushed(ItemKind::Approval),
            &approval("T3", &exact, "pending"),
            ANA,
            1_500,
        ));
        assert_eq!(shown.category, Some(Category::Approval));
        // No tool call named: nothing to bind the decision to.
        let mut no_call = approval("T1", "list files", "pending");
        no_call.insert("tool_call_id".to_owned(), Value::Null);
        assert!(!fits(&no_call));
    }

    #[test]
    fn an_approval_not_this_persons_decided_or_expired_offers_nothing() {
        let mut other = approval("T1", "list files", "pending");
        other.insert("approvers".to_owned(), json!("[\"ben\"]"));
        let shown = show(classify(&pushed(ItemKind::Approval), &other, ANA, 1_500));
        assert_eq!(shown.category, None);
        assert_eq!(shown.approval.and_then(|a| a.category), None);

        for (state, now) in [("approved", 1_500), ("denied", 1_500), ("pending", 2_000)] {
            let shown = show(classify(
                &pushed(ItemKind::Approval),
                &approval("T1", "list files", state),
                ANA,
                now,
            ));
            assert_eq!(shown.category, None, "{state}");
            assert_eq!(shown.approval, None, "{state}");
            assert!(!shown.body.contains("list files"), "{state}");
        }
    }

    fn message(role: &str, author: &str, body: &str, metadata: Value) -> Map<String, Value> {
        row(json!({
            "id": "01JAPPROVAL0000000000000001", "session_id": "ana.1.3",
            "page_key": "ana.1.3/2026-10", "turn_id": "t1", "role": role, "body": body,
            "body_blob": null,
            "metadata": if metadata.is_null() { Value::Null } else { Value::from(metadata.to_string()) },
            "created_at_ms": 1_000, "author_actor": author, "author_client": "c",
        }))
    }

    #[test]
    fn the_proxys_answer_shows_its_newest_words_and_an_ask_or_a_persons_does_not() {
        let item = pushed(ItemKind::Message);
        let shown = show(classify(
            &item,
            &message(
                "agent",
                "nixie",
                "The build is green.",
                json!({"model": "m"}),
            ),
            ANA,
            0,
        ));
        assert_eq!(
            (shown.title.as_str(), shown.body.as_str()),
            ("nixie", "The build is green.")
        );
        assert_eq!((shown.category, shown.approval), (None, None));
        let blob = show(classify(
            &item,
            &message("agent", "nixie", "", Value::Null),
            ANA,
            0,
        ));
        assert_eq!(blob.body, "nixie answered; open keeper to read it.");

        for dropped in [
            message("agent", "nixie", "Which one?", json!({"kind": "ask"})),
            message("person", "ben", "hi", Value::Null),
            message("agent", ANA, "mine", Value::Null),
        ] {
            assert_eq!(classify(&item, &dropped, ANA, 0), HqClassified::Drop);
        }
    }

    #[test]
    fn a_second_delivery_of_an_item_replaces_the_first() {
        let item = pushed(ItemKind::Approval);
        let other = PushedItem {
            item_id: "01JOTHER".to_owned(),
            ..item.clone()
        };
        let in_neuradrive = PushedItem {
            database: "neuradrive".to_owned(),
            ..item.clone()
        };
        let delivered = [
            Delivered {
                id: "first".to_owned(),
                item: json!(item),
            },
            Delivered {
                id: "other".to_owned(),
                item: json!(other),
            },
            Delivered {
                id: "elsewhere".to_owned(),
                item: json!(in_neuradrive),
            },
            Delivered {
                id: "matrix".to_owned(),
                item: Value::Null,
            },
        ];
        assert_eq!(replaces(&delivered, &item), ["first"]);
        assert!(replaces(&[], &item).is_empty());
    }

    fn tap(category: &str, action: &str, user_info: Value) -> ActionTap {
        ActionTap {
            category: category.to_owned(),
            action: action.to_owned(),
            user_info,
        }
    }

    #[test]
    fn an_action_decides_only_what_keeper_showed_and_never_approves_t4() {
        let shown = show(classify(
            &pushed(ItemKind::Approval),
            &approval("T2", "list files", "pending"),
            ANA,
            1_500,
        ));
        let info = composed_info(&shown);
        let decision =
            action_decision(&tap("KEEPER_APPROVAL", "KEEPER_APPROVE_ONCE", info.clone()))
                .expect("decides");
        assert_eq!(
            decision,
            HqActionDecision {
                session: AgentSessionRef {
                    database: "tgdrive".to_owned(),
                    session_id: "ana.1.3".to_owned(),
                },
                approval_id: "01JAPPROVAL0000000000000001".to_owned(),
                binding_digest: "d1".to_owned(),
                approve: true,
            }
        );
        assert!(
            !action_decision(&tap("KEEPER_APPROVAL", "KEEPER_DENY", info.clone()))
                .expect("denies")
                .approve
        );
        // Another category than the one keeper chose, an unknown action.
        assert_eq!(
            action_decision(&tap(
                "KEEPER_APPROVAL_OPEN",
                "KEEPER_APPROVE_ONCE",
                info.clone()
            )),
            Err(ActionRefusal::NotOffered)
        );
        assert_eq!(
            action_decision(&tap("KEEPER_APPROVAL", "KEEPER_SNOOZE", info.clone())),
            Err(ActionRefusal::UnknownAction)
        );
        // A push as the gateway sent it decides nothing.
        let mut uncomposed = info.clone();
        uncomposed[SHOWN_INFO] = Value::Bool(false);
        assert_eq!(
            action_decision(&tap("KEEPER_APPROVAL", "KEEPER_DENY", uncomposed)),
            Err(ActionRefusal::NoApproval)
        );

        // T4: Deny is offered, Approve once never — whatever the userInfo says.
        let t4 = show(classify(
            &pushed(ItemKind::Approval),
            &approval("T4", "Delete the backups", "pending"),
            ANA,
            1_500,
        ));
        let t4_info = composed_info(&t4);
        assert!(
            action_decision(&tap("KEEPER_APPROVAL_OPEN", "KEEPER_DENY", t4_info.clone())).is_ok()
        );
        assert_eq!(
            action_decision(&tap(
                "KEEPER_APPROVAL_OPEN",
                "KEEPER_APPROVE_ONCE",
                t4_info.clone()
            )),
            Err(ActionRefusal::NotOffered)
        );
        let mut forged = t4_info;
        forged[APPROVAL_INFO]["category"] = json!("KEEPER_APPROVAL");
        assert_eq!(
            action_decision(&tap("KEEPER_APPROVAL", "KEEPER_APPROVE_ONCE", forged)),
            Err(ActionRefusal::NotOffered)
        );
    }

    #[test]
    fn a_tap_opens_the_items_session_and_a_repost_keeps_it() {
        let shown = show(classify(
            &pushed(ItemKind::Message),
            &message("agent", "nixie", "done", Value::Null),
            ANA,
            0,
        ));
        let info = composed_info(&shown);
        let session = PushOpenVm::Session {
            database: "tgdrive".to_owned(),
            session_id: "ana.1.3".to_owned(),
        };
        assert_eq!(tap_target(&info), Some(session.clone()));
        let reposted = reposted_info(&info).expect("composed");
        assert_eq!(tap_target(&reposted), Some(session));
        assert_eq!(reposted.get(APPROVAL_INFO), None);
        assert_eq!(tap_target(&json!({ITEM_INFO: shown.item})), None);
    }

    #[test]
    fn the_push_target_is_the_persons_gateway_and_this_devices_token() {
        let target = PushTarget::apns(
            Some("https://push.example.org"),
            ApnsEnvironment::Development,
            &[1, 2, 3],
        )
        .expect("a target");
        assert_eq!(
            target.body(),
            json!({"platform": "ios", "pushkey": "AQID",
                   "app_id": "dev.tgorka.keeper.ios.dev",
                   "gateway_url": "https://push.example.org/_matrix/push/v1/notify"})
        );
        for stored in [None, Some(""), Some("http://push.example.org")] {
            assert_eq!(
                PushTarget::apns(stored, ApnsEnvironment::Production, &[1]),
                None,
                "{stored:?}"
            );
        }
        assert_eq!(
            PushTarget::apns(
                Some("https://push.example.org"),
                ApnsEnvironment::Production,
                &[]
            ),
            None
        );
        let endpoint =
            crate::agents::unifiedpush::Endpoint::parse("https://ntfy.example.org/upX?up=1")
                .expect("an endpoint");
        let gateway = Gateway::parse("https://ntfy.example.org")
            .expect("parses")
            .expect("a gateway");
        assert_eq!(
            PushTarget::unifiedpush(gateway, &endpoint).body(),
            json!({"platform": "android", "pushkey": "https://ntfy.example.org/upX?up=1",
                   "app_id": "dev.tgorka.keeper.android",
                   "gateway_url": "https://ntfy.example.org/_matrix/push/v1/notify"})
        );
    }
}
