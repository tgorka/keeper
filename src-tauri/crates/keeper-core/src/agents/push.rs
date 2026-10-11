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

use serde::{Deserialize, Serialize};
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
}
