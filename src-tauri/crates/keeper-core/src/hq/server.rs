//! The agents server's address, `agents.server_url` (AD-418, D-4).
//!
//! keeper ships no address: the person types the base URL of the server they
//! run, and the databases it serves are discovered from it (`GET /databases`),
//! never typed. This is the grammar of that one setting, so the setter, the
//! egress row, the replica paths and the requests read it the same way: blank
//! is "no server"; anything else is an `http://` or `https://` address with a
//! host, no user name or password, no query and no fragment. A path is kept —
//! a server behind a reverse proxy lives under one — without its trailing
//! slash, so `<base>/databases` and `<base>/<database>` are plain joins.

/// A server address keeper will contact.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ServerUrl {
    /// Normalised: no trailing slash, no query, no fragment, no userinfo.
    base: String,
    /// `host` or `host:port`, as the egress row shows it.
    host: String,
}

impl ServerUrl {
    /// Read the setting as typed. Blank (or only whitespace) is `Ok(None)`,
    /// "no server"; anything else that is not exactly a server address is
    /// refused with the sentence Settings shows beside the field.
    pub fn parse(raw: &str) -> Result<Option<ServerUrl>, ServerRefusal> {
        let trimmed = raw.trim();
        if trimmed.is_empty() {
            return Ok(None);
        }
        if !trimmed.contains("://") {
            return Err(ServerRefusal::NoScheme);
        }
        let url = url::Url::parse(trimmed).map_err(|_| ServerRefusal::NotAnAddress)?;
        if !matches!(url.scheme(), "http" | "https") {
            return Err(ServerRefusal::Scheme {
                scheme: url.scheme().to_owned(),
            });
        }
        if !url.username().is_empty() || url.password().is_some() {
            return Err(ServerRefusal::Credentials);
        }
        if url.query().is_some() || url.fragment().is_some() {
            return Err(ServerRefusal::QueryOrFragment);
        }
        let Some(host) = crate::agents::mcp::url_host(url.as_str()) else {
            return Err(ServerRefusal::NotAnAddress);
        };
        let base = url.as_str().trim_end_matches('/').to_owned();
        Ok(Some(ServerUrl { base, host }))
    }

    /// The base URL, as stored.
    pub fn as_str(&self) -> &str {
        &self.base
    }

    /// `host` or `host:port` — the egress row, never a path.
    pub fn host(&self) -> &str {
        &self.host
    }

    /// A root route of the server (`/databases`, `/pair/redeem`, …).
    pub fn route(&self, path: &str) -> String {
        format!("{}{path}", self.base)
    }

    /// The mount of one database: its syncular base URL.
    pub fn mount(&self, database: &str) -> String {
        format!("{}/{database}", self.base)
    }

    /// The directory this server's replicas live in under `hq/`: the host
    /// with its port, and its path when it has one, made filename-safe, so
    /// two servers never share a replica.
    pub fn dir_name(&self) -> String {
        let without_scheme = self
            .base
            .split_once("://")
            .map_or(self.base.as_str(), |(_, rest)| rest);
        without_scheme
            .chars()
            .map(|c| {
                if c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '_') {
                    c.to_ascii_lowercase()
                } else {
                    '_'
                }
            })
            .collect()
    }
}

/// Why a typed address is not one keeper will contact. The `Display` text is
/// the sentence Settings shows beside the field.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ServerRefusal {
    /// No `scheme://` in front — most often a bare host.
    #[error("A server address starts with https:// or http://, as in https://agents.example.org.")]
    NoScheme,
    /// Not a URL with a host.
    #[error("That is not a web address. A server address looks like https://agents.example.org.")]
    NotAnAddress,
    /// A scheme other than `http` or `https`.
    #[error(
        "keeper reaches your agents server over https:// or http://, and this address starts \
         with {scheme}://."
    )]
    Scheme {
        /// The scheme the address carried.
        scheme: String,
    },
    /// A user name or password in the address.
    #[error(
        "A server address carries no user name or password: this device signs in with the \
         code from one of your other devices."
    )]
    Credentials,
    /// A `?query` or `#fragment`.
    #[error(
        "Give the server's address alone, with nothing after a ? or #: keeper finds your \
         databases from it."
    )]
    QueryOrFragment,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn blank_is_no_server_and_an_address_is_normalised() {
        assert_eq!(ServerUrl::parse("  "), Ok(None));
        let server = ServerUrl::parse(" https://Agents.Example.org/ ")
            .expect("parses")
            .expect("an address");
        assert_eq!(server.as_str(), "https://agents.example.org");
        assert_eq!(server.host(), "agents.example.org");
        assert_eq!(
            server.route("/databases"),
            "https://agents.example.org/databases"
        );
        assert_eq!(
            server.mount("tgdrive"),
            "https://agents.example.org/tgdrive"
        );

        let proxied = ServerUrl::parse("http://100.64.0.7:8790/hq/")
            .expect("parses")
            .expect("an address");
        assert_eq!(proxied.as_str(), "http://100.64.0.7:8790/hq");
        assert_eq!(proxied.host(), "100.64.0.7:8790");
        assert_eq!(proxied.mount("n"), "http://100.64.0.7:8790/hq/n");
        assert_eq!(proxied.dir_name(), "100.64.0.7_8790_hq");
    }

    #[test]
    fn each_refusal_has_its_own_sentence() {
        let cases = [
            ("agents.example.org", ServerRefusal::NoScheme),
            ("https://", ServerRefusal::NotAnAddress),
            (
                "wss://agents.example.org",
                ServerRefusal::Scheme {
                    scheme: "wss".to_owned(),
                },
            ),
            (
                "https://me:pw@agents.example.org",
                ServerRefusal::Credentials,
            ),
            ("https://me@agents.example.org", ServerRefusal::Credentials),
            (
                "https://agents.example.org/?t=1",
                ServerRefusal::QueryOrFragment,
            ),
            (
                "https://agents.example.org/#x",
                ServerRefusal::QueryOrFragment,
            ),
        ];
        for (raw, refusal) in cases {
            assert_eq!(ServerUrl::parse(raw), Err(refusal), "{raw}");
        }
        let sentences: std::collections::BTreeSet<String> = [
            ServerRefusal::NoScheme,
            ServerRefusal::NotAnAddress,
            ServerRefusal::Scheme {
                scheme: "ftp".to_owned(),
            },
            ServerRefusal::Credentials,
            ServerRefusal::QueryOrFragment,
        ]
        .iter()
        .map(ToString::to_string)
        .collect();
        assert_eq!(sentences.len(), 5);
    }

    #[test]
    fn two_ports_or_two_paths_are_two_servers() {
        let dir = |raw: &str| {
            ServerUrl::parse(raw)
                .expect("parses")
                .expect("an address")
                .dir_name()
        };
        assert_ne!(
            dir("https://a.example.org"),
            dir("https://a.example.org:8443")
        );
        assert_ne!(
            dir("https://a.example.org/x"),
            dir("https://a.example.org/y")
        );
        assert!(!dir("https://a.example.org/../x").contains('/'));
    }
}
