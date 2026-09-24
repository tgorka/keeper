//! Where repositories come from (AD-333): the account's own forge, the
//! descriptor's `[[forges]]`, and GitHub — through the organisation's broker
//! or keeper's own OAuth App.

use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::org_account::descriptor::{
    AccountDescriptor, ForgeEntryKind, RepoAuthConfig, GITHUB_API_BASE, GITHUB_WEB_BASE,
};
use crate::org_account::settings_sync::url_origin;

pub use crate::org_account::descriptor::ACCOUNT_FORGE_ID;

/// keeper's own public GitHub OAuth App, for the device flow. `None` until
/// one is registered: without it and without an account, nothing appears.
pub const BUILTIN_GITHUB_CLIENT_ID: Option<&str> = None;

/// The GitHub source's id, built-in or from `[[forges]]`.
pub const GITHUB_ID: &str = "github";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "lowercase")]
#[ts(export, rename = "ForgeKindVm")]
pub enum ForgeKind {
    Github,
    Forgejo,
}

/// How keeper gets a token for a source.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export, rename = "TokenViaVm")]
pub enum TokenVia {
    /// The organisation's GitHub broker (makistack `github-broker`).
    Broker,
    /// The account's forge sign-in (`oidc::forge_token`).
    AccountForge,
    /// A device-flow connection kept in the keychain.
    DeviceFlow,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ForgeSource {
    /// `[a-z0-9-]{1,32}`: part of `forge:<id>` and the keychain key.
    pub id: String,
    pub kind: ForgeKind,
    pub name: String,
    pub web_base: String,
    pub api_base: String,
    /// The device-flow OAuth client, when there is one.
    pub client_id: Option<String>,
    pub via: TokenVia,
}

impl ForgeSource {
    /// How a token is got when the broker has no grants for this person:
    /// the device flow when a client id exists, else nothing.
    pub fn own_via(&self) -> Option<TokenVia> {
        match self.via {
            TokenVia::Broker => self.client_id.is_some().then_some(TokenVia::DeviceFlow),
            via => Some(via),
        }
    }

    pub fn host(&self) -> String {
        super::host_of(&self.web_base)
    }

    /// `scheme://host[:port]` of `web_base`: where a drive's remote must be
    /// for this source's token to go to it.
    pub fn origin(&self) -> Option<String> {
        url_origin(&self.web_base)
    }

    /// Where the person reviews (or revokes) keeper's OAuth App on GitHub:
    /// only a GitHub source with a device-flow client has one.
    pub fn apps_url(&self) -> Option<String> {
        let client_id = self.client_id.as_deref()?;
        (self.kind == ForgeKind::Github).then(|| {
            format!(
                "{}/settings/connections/applications/{client_id}",
                self.web_base
            )
        })
    }
}

/// Whether `source`'s token may go to a drive at `remote_url`: the remote is
/// at the source's own `web_base` origin (validated `https`, loopback for
/// tests), so a token never goes to a host the person did not choose it
/// for. Origins compare parsed: case, a default port and userinfo do not
/// matter, while a look-alike host, a trailing dot, another port or plain
/// `http` do.
pub fn remote_on_source(source: &ForgeSource, remote_url: &str) -> bool {
    source
        .origin()
        .is_some_and(|origin| url_origin(remote_url).as_ref() == Some(&origin))
}

/// The forge's root for a descriptor without a forge `issuer`: Forgejo's
/// token endpoint is `<root>/login/oauth/access_token`, and the root may sit
/// under a subpath.
fn forge_root(token_url: &str) -> Option<String> {
    token_url
        .trim_end_matches('/')
        .strip_suffix("/login/oauth/access_token")
        .map(str::to_owned)
        .or_else(|| url_origin(token_url))
}

/// Every source a person can browse, in the order the switcher shows them:
/// the account's forge, the descriptor's `[[forges]]`, then GitHub. A source
/// keeper cannot get a token for is left out (AD-27): a GitHub entry with
/// neither a broker nor a client id. keeper's own client id serves only the
/// entry whose web and API bases are both github.com's.
pub fn sources(
    descriptor: Option<&AccountDescriptor>,
    builtin_github_client_id: Option<&str>,
) -> Vec<ForgeSource> {
    let mut out = Vec::new();
    let broker = descriptor.and_then(|d| d.github_broker.as_ref());
    if let Some(d) = descriptor {
        if let (RepoAuthConfig::Oauth(forge), Some(api_base)) = (&d.config.auth, &d.config.api_base)
        {
            let web_base = forge
                .issuer
                .clone()
                .or_else(|| forge.token_url.as_deref().and_then(forge_root))
                .or_else(|| url_origin(&d.config.url))
                .unwrap_or_else(|| d.config.url.clone());
            out.push(ForgeSource {
                id: ACCOUNT_FORGE_ID.to_owned(),
                kind: ForgeKind::Forgejo,
                name: d.name.clone(),
                web_base: trimmed(&web_base),
                api_base: trimmed(api_base),
                client_id: None,
                via: TokenVia::AccountForge,
            });
        }
        for entry in d
            .forges
            .iter()
            .filter(|entry| entry.kind == ForgeEntryKind::Github)
        {
            let id = entry.source_id();
            let web_base = entry.web_base().unwrap_or(GITHUB_WEB_BASE);
            let api_base = entry.api_base().unwrap_or(GITHUB_API_BASE);
            let on_github_com = web_base.trim_end_matches('/') == GITHUB_WEB_BASE
                && api_base.trim_end_matches('/') == GITHUB_API_BASE;
            let client_id = entry
                .client_id
                .as_deref()
                .or(builtin_github_client_id.filter(|_| on_github_com))
                .map(str::to_owned);
            let via = if id == GITHUB_ID && broker.is_some() {
                TokenVia::Broker
            } else if client_id.is_some() {
                TokenVia::DeviceFlow
            } else {
                continue;
            };
            out.push(ForgeSource {
                id: id.to_owned(),
                kind: ForgeKind::Github,
                name: entry.name.clone().unwrap_or_else(|| "GitHub".to_owned()),
                web_base: trimmed(web_base),
                api_base: trimmed(api_base),
                client_id,
                via,
            });
        }
    }
    if !out.iter().any(|source| source.id == GITHUB_ID)
        && (broker.is_some() || builtin_github_client_id.is_some())
    {
        out.push(ForgeSource {
            id: GITHUB_ID.to_owned(),
            kind: ForgeKind::Github,
            name: "GitHub".to_owned(),
            web_base: GITHUB_WEB_BASE.to_owned(),
            api_base: GITHUB_API_BASE.to_owned(),
            client_id: builtin_github_client_id.map(str::to_owned),
            via: if broker.is_some() {
                TokenVia::Broker
            } else {
                TokenVia::DeviceFlow
            },
        });
    }
    out
}

fn trimmed(url: &str) -> String {
    url.trim_end_matches('/').to_owned()
}

pub fn find<'a>(sources: &'a [ForgeSource], id: &str) -> Option<&'a ForgeSource> {
    sources.iter().find(|source| source.id == id)
}

const FORGE_CREDENTIAL_PREFIX: &str = "forge:";

/// The credential source a drive added from `source` uses: the account's
/// forge token (`account`, which epic 85 routes by host) or `forge:<id>`.
pub fn credential_for(source: &ForgeSource) -> String {
    match source.via {
        TokenVia::AccountForge => "account".to_owned(),
        _ => format!("{FORGE_CREDENTIAL_PREFIX}{}", source.id),
    }
}

/// The source id of a `forge:<id>` credential source.
pub fn forge_credential_id(value: &str) -> Option<&str> {
    value
        .strip_prefix(FORGE_CREDENTIAL_PREFIX)
        .filter(|id| valid_source_id(id))
}

pub(crate) fn valid_source_id(id: &str) -> bool {
    (1..=32).contains(&id.len())
        && id
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::org_account::descriptor::parse_json;

    fn descriptor(extra: &str) -> AccountDescriptor {
        parse_json(&format!(
            r#"{{ "version": 1, "id": "acme", "name": "Acme",
                 "auth": {{ "issuer": "https://id.acme.dev", "client_id": "keeper" }},
                 "config": {{
                    "url": "https://git.acme.dev/git/people/keeper-config.git",
                    "api_base": "https://git.acme.dev/git/api/v1",
                    "auth": {{ "mode": "oauth", "issuer": "https://git.acme.dev/git",
                              "client_id": "c", "scope": "openid write:repository" }}
                 }} {extra} }}"#
        ))
        .expect("descriptor")
    }

    fn row(s: &ForgeSource) -> (&str, ForgeKind, TokenVia, &str, Option<&str>) {
        (
            s.id.as_str(),
            s.kind,
            s.via,
            s.web_base.as_str(),
            s.client_id.as_deref(),
        )
    }

    #[test]
    fn nothing_appears_without_an_account_or_a_github_client_id() {
        assert!(sources(None, None).is_empty());
        let github = sources(None, Some("Iv1.builtin"));
        assert_eq!(
            github.iter().map(row).collect::<Vec<_>>(),
            [(
                "github",
                ForgeKind::Github,
                TokenVia::DeviceFlow,
                "https://github.com",
                Some("Iv1.builtin")
            )]
        );
        assert_eq!(github[0].api_base, "https://api.github.com");
    }

    #[test]
    fn the_account_forge_comes_first_and_the_broker_serves_github() {
        let plain = descriptor("");
        assert_eq!(
            sources(Some(&plain), None)
                .iter()
                .map(row)
                .collect::<Vec<_>>(),
            [(
                "account-forge",
                ForgeKind::Forgejo,
                TokenVia::AccountForge,
                "https://git.acme.dev/git",
                None
            )]
        );

        let brokered = descriptor(r#", "github_broker": { "url": "https://b.acme.dev:8455" }"#);
        let rows = sources(Some(&brokered), Some("Iv1.builtin"));
        assert_eq!(
            rows.iter().map(row).collect::<Vec<_>>(),
            [
                (
                    "account-forge",
                    ForgeKind::Forgejo,
                    TokenVia::AccountForge,
                    "https://git.acme.dev/git",
                    None
                ),
                (
                    "github",
                    ForgeKind::Github,
                    TokenVia::Broker,
                    "https://github.com",
                    Some("Iv1.builtin")
                ),
            ]
        );
        assert_eq!(rows[1].own_via(), Some(TokenVia::DeviceFlow));
        let no_fallback = sources(Some(&brokered), None);
        assert_eq!(no_fallback[1].via, TokenVia::Broker);
        assert_eq!(no_fallback[1].own_via(), None);
    }

    #[test]
    fn forges_entries_become_sources_only_when_keeper_can_get_a_token() {
        let mut d = descriptor(
            r#", "forges": [
                { "kind": "github", "id": "ghe", "name": "Acme GitHub",
                  "web_base": "https://ghe.acme.dev", "api_base": "https://ghe.acme.dev/api/v3",
                  "client_id": "Iv1.ghe" },
                { "kind": "github", "id": "ghe-nokey", "web_base": "https://ghe2.acme.dev",
                  "api_base": "https://ghe2.acme.dev/api/v3" },
                { "kind": "github", "name": "Work GitHub" }
            ]"#,
        );
        // Validation refuses this entry; `sources()` must not lend it keeper's
        // own client either, or the person's github.com token would go to
        // the collector.
        d.forges.push(crate::org_account::descriptor::ForgeEntry {
            kind: ForgeEntryKind::Github,
            id: Some("collector".to_owned()),
            name: None,
            web_base: None,
            api_base: Some("https://collector.evil".to_owned()),
            client_id: None,
        });
        let rows = sources(Some(&d), Some("Iv1.builtin"));
        assert_eq!(
            rows.iter().map(row).collect::<Vec<_>>(),
            [
                (
                    "account-forge",
                    ForgeKind::Forgejo,
                    TokenVia::AccountForge,
                    "https://git.acme.dev/git",
                    None
                ),
                (
                    "ghe",
                    ForgeKind::Github,
                    TokenVia::DeviceFlow,
                    "https://ghe.acme.dev",
                    Some("Iv1.ghe")
                ),
                // The built-in client belongs to github.com only.
                (
                    "github",
                    ForgeKind::Github,
                    TokenVia::DeviceFlow,
                    "https://github.com",
                    Some("Iv1.builtin")
                ),
            ]
        );
        assert_eq!(rows[2].name, "Work GitHub");
    }

    #[test]
    fn a_subpath_forge_without_an_issuer_is_rooted_at_its_token_endpoint() {
        let d = parse_json(
            r#"{ "version": 1, "id": "acme", "name": "Acme",
                 "auth": { "issuer": "https://id.acme.dev", "client_id": "keeper" },
                 "config": {
                    "url": "https://git.acme.dev/git/people/keeper-config.git",
                    "api_base": "https://git.acme.dev/git/api/v1",
                    "auth": { "mode": "oauth", "client_id": "c", "scope": "write:repository",
                              "authorize_url": "https://git.acme.dev/git/login/oauth/authorize",
                              "token_url": "https://git.acme.dev/git/login/oauth/access_token",
                              "user_url": "{api_base}/user" }
                 } }"#,
        )
        .expect("descriptor");
        assert_eq!(
            sources(Some(&d), None)[0].web_base,
            "https://git.acme.dev/git"
        );
    }

    #[test]
    fn credentials_name_the_source_and_github_links_its_app_review() {
        let d = descriptor("");
        let all = sources(Some(&d), Some("Iv1.builtin"));
        assert_eq!(credential_for(&all[0]), "account");
        assert_eq!(credential_for(&all[1]), "forge:github");
        assert_eq!(forge_credential_id("forge:github"), Some("github"));
        for bad in ["forge:", "forge:GitHub", "account", "forge:a/b"] {
            assert_eq!(forge_credential_id(bad), None, "{bad}");
        }
        assert_eq!(
            all[1].apps_url().as_deref(),
            Some("https://github.com/settings/connections/applications/Iv1.builtin")
        );
        assert_eq!(all[0].apps_url(), None);
    }

    #[test]
    fn a_remote_is_on_a_source_only_at_its_exact_origin() {
        let github = &sources(None, Some("Iv1.builtin"))[0];
        for remote in [
            "https://github.com/o/r.git",
            "https://GitHub.com/o/r",
            "https://github.com:443/o/r.git",
            "https://x-access-token:t@github.com/o/r.git",
        ] {
            assert!(remote_on_source(github, remote), "{remote}");
        }
        for remote in [
            "http://github.com/o/r.git",
            "https://github.com:8443/o/r.git",
            "https://github.com.evil.com/o/r.git",
            "https://github.com@evil.com/o/r.git",
            "https://github.com./o/r.git",
            "https://gitlab.com/o/r.git",
            "git@github.com:o/r.git",
            "",
        ] {
            assert!(!remote_on_source(github, remote), "{remote}");
        }
    }
}
