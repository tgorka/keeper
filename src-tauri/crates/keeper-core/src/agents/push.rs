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
//! The gateway reaches the agents server's pushes now (106.6,
//! [`crate::hq::push`]); this module keeps the words and the notification
//! categories those pushes share. The Matrix pusher the phone registered on
//! each account for the agents' Matrix rooms is gone with them (108.2): what
//! is kept of it is its *removal* ([`remove`]), for one release (DW-E108-1),
//! so a phone upgraded from 0.10.x takes its pusher off every account.

/// A pusher's identity on a homeserver — what [`remove`] and a sign-out
/// remove by — named for the shell, which has no `matrix-sdk` of its own.
pub use matrix_sdk::ruma::api::client::push::PusherIds;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use ts_rs::TS;

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
// The agents server's push names one item; what the person sees of it is
// [`crate::hq::push`]'s table, over these words and categories.

/// The longest payload, in characters, a notification shows as its whole
/// body and offers *Approve once* on (98.1 #11).
pub const BODY_MAX_CHARS: usize = 120;

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

/// The body of an approval the notification cannot show whole.
pub(crate) fn open_body(tier: u8) -> String {
    format!(
        "{} — open keeper to read the whole request",
        tier_word(tier)
    )
}

/// Whether `c` could make shown text read as other than it is: a line
/// break or any other control character, or a bidirectional override.
pub(crate) fn misreads(c: char) -> bool {
    c.is_control()
        || matches!(c, '\u{061c}' | '\u{200e}' | '\u{200f}' | '\u{2028}' | '\u{2029}' | '\u{202a}'..='\u{202e}' | '\u{2066}'..='\u{2069}')
}

/// Every character a reader cannot see, as of Unicode 16.0 (R1066K-05):
/// each format character (General_Category `Cf` — U+200B ZERO WIDTH SPACE,
/// U+2060 WORD JOINER, U+00AD SOFT HYPHEN, …) and each
/// Default_Ignorable_Code_Point, which takes in the variation selectors
/// (U+180B…U+180F, U+FE00…U+FE0F, U+E0100…U+E01EF) and the tag characters
/// (U+E0000…U+E007F, with the rest of that plane's ignorable block). The
/// two sets merged into ranges; neither is ever stripped or normalised
/// away, because the request without them is another request.
const HIDDEN: [(char, char); 25] = [
    ('\u{00ad}', '\u{00ad}'),
    ('\u{034f}', '\u{034f}'),
    ('\u{0600}', '\u{0605}'),
    ('\u{061c}', '\u{061c}'),
    ('\u{06dd}', '\u{06dd}'),
    ('\u{070f}', '\u{070f}'),
    ('\u{0890}', '\u{0891}'),
    ('\u{08e2}', '\u{08e2}'),
    ('\u{115f}', '\u{1160}'),
    ('\u{17b4}', '\u{17b5}'),
    ('\u{180b}', '\u{180f}'),
    ('\u{200b}', '\u{200f}'),
    ('\u{202a}', '\u{202e}'),
    ('\u{2060}', '\u{206f}'),
    ('\u{3164}', '\u{3164}'),
    ('\u{fe00}', '\u{fe0f}'),
    ('\u{feff}', '\u{feff}'),
    ('\u{ffa0}', '\u{ffa0}'),
    ('\u{fff0}', '\u{fffb}'),
    ('\u{110bd}', '\u{110bd}'),
    ('\u{110cd}', '\u{110cd}'),
    ('\u{13430}', '\u{1343f}'),
    ('\u{1bca0}', '\u{1bca3}'),
    ('\u{1d173}', '\u{1d17a}'),
    ('\u{e0000}', '\u{e0fff}'),
];

/// Whether `c` is one a reader cannot see ([`HIDDEN`]).
pub(crate) fn hidden(c: char) -> bool {
    HIDDEN
        .iter()
        .any(|(first, last)| (*first..=*last).contains(&c))
}

/// Whether `c` may not stand in a request a person approves from the lock
/// screen: it [`misreads`], or it is [`hidden`]. Such a request is never
/// rendered with escapes either: it is refused *Approve once* (R1066K-05).
pub(crate) fn unreadable(c: char) -> bool {
    misreads(c) || hidden(c)
}

/// Whether any string in `value` — a key or a value, at any depth — holds a
/// character that is [`unreadable`].
pub(crate) fn holds_unreadable(value: &Value) -> bool {
    match value {
        Value::String(text) => text.chars().any(unreadable),
        Value::Array(items) => items.iter().any(holds_unreadable),
        Value::Object(map) => map
            .iter()
            .any(|(key, value)| key.chars().any(unreadable) || holds_unreadable(value)),
        _ => false,
    }
}

/// The notification's own words when the extension cannot say more — the
/// app's data has not moved yet, the token expired, an event it could not
/// read (`keeper-nse`'s generic answer): keeper's words, never an event's.
pub const GENERIC_TITLE: &str = "keeper";

/// See [`GENERIC_TITLE`].
pub const GENERIC_BODY: &str = "Open keeper to see what arrived.";

/// The key only a notification keeper composed on this phone carries
/// (R343): the extension's `show`, or the notification keeper re-posts
/// after an action ([`FINISH_IN_KEEPER`]). The push itself is the
/// homeserver's to write — the pusher's `default_payload` and anything
/// that reaches the gateway — so it can carry this key and a category too;
/// the extension therefore removes every keeper key from what arrived on
/// every path, shows keeper's generic words with no category on every path
/// but `show`, and sets this key only there. A tap and an action act on
/// nothing without it ([`crate::hq::push`]).
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
/// APNs pushkey (base64) and the environment's app — what the iPhone
/// registered before 108.2, and what [`remove`] takes off.
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

/// Take the pusher `ids` names off `client`'s account (DW-E108-1): the
/// phone registered one for the agents' Matrix rooms before 108.2, and a
/// phone upgraded from such a build removes it from each account once. A
/// pusher the homeserver does not hold is no error there.
pub async fn remove(
    client: &matrix_sdk::Client,
    ids: matrix_sdk::ruma::api::client::push::PusherIds,
) -> Result<(), matrix_sdk::Error> {
    client.pusher().delete(ids).await?;
    Ok(())
}

/// The tier as a notification says it.
pub fn tier_word(tier: u8) -> String {
    let words = match tier {
        0 | 1 => "it only reads or changes what can be put back",
        2 => "it changes something that can be put back",
        3 => "it reaches beyond this session: it sends, or changes what runs",
        _ => "it cannot be undone",
    };
    format!("T{tier}: {words}")
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

/// Where tapping a keeper notification opens (UX-DR144): a session in the
/// Agents window, named by its database (106.6, AD-433).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
#[ts(export)]
pub enum PushOpenVm {
    /// A session of the agents server.
    Session {
        database: String,
        session_id: String,
    },
}

/// Where tapping any notification keeper composed opens: the session of an
/// agents server item ([`crate::hq::push::tap_target`]).
pub fn open_target(user_info: &Value) -> Option<PushOpenVm> {
    crate::hq::push::tap_target(user_info)
}

/// What a notification keeper re-posts after an action carries of the one
/// acted on, so a tap on it still opens where that one did; `None` when
/// keeper did not compose that one.
pub fn reposted_info(user_info: &Value) -> Option<Value> {
    crate::hq::push::reposted_info(user_info)
}

/// The group a re-posted notification joins: the session's.
pub fn reposted_group(user_info: &Value) -> Option<String> {
    match open_target(user_info)? {
        PushOpenVm::Session {
            database,
            session_id,
        } => Some(format!("{database}/{session_id}")),
    }
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

    // --- The app id (98.1 #3) ---

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
}
