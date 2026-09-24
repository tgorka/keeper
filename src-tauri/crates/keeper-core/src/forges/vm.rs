//! What the browse sheet renders (AD-338).

use serde::{Deserialize, Serialize};
use ts_rs::TS;

use super::source::{ForgeKind, ForgeSource, TokenVia};
use super::tokens::{self, ForgeError};
use crate::org_account::descriptor::AccountDescriptor;
use crate::org_account::session;
use crate::platform::Platform;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub enum ForgeStateVm {
    Connected,
    NotConnected,
    NeedsSignIn,
    Unreachable,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct ForgeSourceVm {
    pub id: String,
    pub kind: ForgeKind,
    pub name: String,
    pub host: String,
    pub via: TokenVia,
    pub state: ForgeStateVm,
    pub login: Option<String>,
    /// Why the source is not usable right now, as a sentence.
    pub sentence: Option<String>,
    /// The credential source a drive added from here stores: `account` or
    /// `forge:<id>`.
    pub credential: String,
    /// A device-flow client exists, so Connect (and Disconnect) are offered
    /// whatever `via` says.
    pub can_connect: bool,
    /// Where to review keeper's OAuth App on GitHub, for a source that has
    /// one.
    pub apps_url: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct ForgeRepoVm {
    pub full_name: String,
    pub owner: String,
    pub name: String,
    pub description: Option<String>,
    pub private: bool,
    pub fork: bool,
    pub archived: bool,
    pub template: bool,
    pub mirror: bool,
    pub default_branch: String,
    pub clone_url: String,
    pub web_url: String,
    #[ts(type = "number | null")]
    pub updated_ms: Option<i64>,
    #[ts(type = "number | null")]
    pub size_kb: Option<u64>,
    pub can_push: bool,
    /// A drive of it only downloads.
    pub pull_only: bool,
    /// Why it only downloads, as the row says it.
    pub pull_only_sentence: Option<String>,
    /// This device's drives on this repository, by name.
    pub added_as: Vec<String>,
    /// Other devices (slugs) that sync it, from `drives.toml`.
    pub elsewhere: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct ForgeOwnerVm {
    pub login: String,
    pub is_you: bool,
    #[ts(type = "number")]
    pub count: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct ForgeNoticeVm {
    pub sentence: String,
    pub link: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct ForgeReposVm {
    pub source_id: String,
    /// Grouped: you first, then other owners A→Z; by name within a group.
    pub repos: Vec<ForgeRepoVm>,
    pub owners: Vec<ForgeOwnerVm>,
    pub notices: Vec<ForgeNoticeVm>,
    pub truncated: bool,
    #[ts(type = "number | null")]
    pub fetched_ms: Option<i64>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct DeviceCodeVm {
    pub user_code: String,
    pub verification_uri: String,
    #[ts(type = "number")]
    pub expires_in: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct ForgeAddItem {
    pub full_name: String,
    pub drive_name: String,
    pub folder: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct ForgeAddReq {
    pub source_id: String,
    pub base_folder: Option<String>,
    pub repos: Vec<ForgeAddItem>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct ForgeAddResultVm {
    pub full_name: String,
    pub profile_id: Option<String>,
    pub sentence: Option<String>,
}

/// One way a drive's git requests can sign in, as the add-folder form offers
/// it: the value `sync_credential_source_set` stores, its label, and what it
/// means in one sentence.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct CredentialChoiceVm {
    pub value: String,
    pub label: String,
    pub detail: String,
}

/// The sign-ins a drive at one remote may use besides a token of its own.
/// Rust decides: a sign-in never goes to a host it does not belong to.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct CredentialChoicesVm {
    /// The account, when the remote is on one of its own hosts.
    pub account: Option<CredentialChoiceVm>,
    /// Repository sources whose origin the remote is at.
    pub forges: Vec<CredentialChoiceVm>,
}

/// Where drives added from the person's repositories go.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct DriveFolderVm {
    pub path: String,
    /// The person chose it; otherwise it is keeper's default, `~/keeper/git`.
    pub chosen: bool,
}

/// What a drive at `remote_url` may sign in with. The account only on its
/// own hosts (`AccountDescriptor::serves_remote`), a repository source only
/// at its own origin (`remote_on_source`); the account's forge is the
/// account choice, not a second one.
pub fn credential_choices(
    account: Option<&AccountDescriptor>,
    sources: &[ForgeSource],
    remote_url: &str,
) -> CredentialChoicesVm {
    let account_choice =
        account
            .filter(|d| d.serves_remote(remote_url))
            .map(|d| CredentialChoiceVm {
                value: "account".to_owned(),
                label: format!("Use my {} account", d.name),
                detail: format!(
                "keeper signs this folder's git requests in with your {} sign-in, so no token is \
                 stored for it.",
                d.name
            ),
            });
    let forges = sources
        .iter()
        .filter(|source| super::remote_on_source(source, remote_url))
        .filter_map(|source| {
            let value = super::credential_for(source);
            (value != "account").then(|| forge_choice(source, account, value))
        })
        .collect();
    CredentialChoicesVm {
        account: account_choice,
        forges,
    }
}

fn forge_choice(
    source: &ForgeSource,
    account: Option<&AccountDescriptor>,
    value: String,
) -> CredentialChoiceVm {
    match (source.via, account) {
        (TokenVia::Broker, Some(d)) => CredentialChoiceVm {
            value,
            label: format!("{} access through {}", source.name, d.name),
            detail: format!(
                "{} gives keeper a one-hour {} token for this repository only, as you; \
                 nothing is stored for this folder.",
                d.name, source.name
            ),
        },
        _ => CredentialChoiceVm {
            value,
            label: format!("Sign in with {}", source.name),
            detail: format!(
                "keeper uses your {} connection, so no token is stored for this folder.",
                source.name
            ),
        },
    }
}

/// A source's row in the switcher, without the network. `error` is what the
/// last listing, token or connect call returned, when it failed.
pub fn source_vm(
    platform: &dyn Platform,
    source: &ForgeSource,
    account: Option<&AccountDescriptor>,
    error: Option<&ForgeError>,
) -> ForgeSourceVm {
    let (state, login, sentence) = match error {
        Some(error) => state_of_error(source, error),
        None => state_now(platform, source, account),
    };
    ForgeSourceVm {
        id: source.id.clone(),
        kind: source.kind,
        name: source.name.clone(),
        host: source.host(),
        via: source.via,
        state,
        login,
        sentence,
        credential: super::credential_for(source),
        can_connect: source.client_id.is_some(),
        apps_url: source.apps_url(),
    }
}

fn state_of_error(
    source: &ForgeSource,
    error: &ForgeError,
) -> (ForgeStateVm, Option<String>, Option<String>) {
    match error {
        ForgeError::NeedsConnect => (ForgeStateVm::NotConnected, None, None),
        ForgeError::NeedsSignIn(sentence) => {
            (ForgeStateVm::NeedsSignIn, None, Some(sentence.clone()))
        }
        ForgeError::Unreachable(sentence)
        | ForgeError::NoAccess(sentence)
        | ForgeError::Refused(sentence)
        | ForgeError::Internal(sentence) => {
            let sentence = if matches!(error, ForgeError::Internal(_)) {
                format!("keeper couldn't list {}'s repositories.", source.name)
            } else {
                sentence.clone()
            };
            (ForgeStateVm::Unreachable, None, Some(sentence))
        }
    }
}

fn state_now(
    platform: &dyn Platform,
    source: &ForgeSource,
    account: Option<&AccountDescriptor>,
) -> (ForgeStateVm, Option<String>, Option<String>) {
    let sign_in = |name: &str| {
        (
            ForgeStateVm::NeedsSignIn,
            None,
            Some(format!("Sign in to {name} to see these repositories.")),
        )
    };
    match source.via {
        TokenVia::DeviceFlow => match tokens::load_session(platform, source) {
            Ok(Some(stored)) => (
                ForgeStateVm::Connected,
                Some(stored.login).filter(|login| !login.is_empty()),
                None,
            ),
            _ => (ForgeStateVm::NotConnected, None, None),
        },
        TokenVia::AccountForge => {
            let Some(d) = account else {
                return sign_in(&source.name);
            };
            if !matches!(session::identity(platform, d), Ok(Some(_))) {
                return sign_in(&d.name);
            }
            match session::load_bound_forge(platform, d) {
                Ok(Some(forge)) => (ForgeStateVm::Connected, Some(forge.login), None),
                _ => (
                    ForgeStateVm::NeedsSignIn,
                    None,
                    Some("Reconnect the repository to see your repositories.".to_owned()),
                ),
            }
        }
        TokenVia::Broker => match account {
            Some(d) if matches!(session::identity(platform, d), Ok(Some(_))) => {
                (ForgeStateVm::Connected, None, None)
            }
            Some(d) => sign_in(&d.name),
            None => sign_in("your account"),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::forges::testing;
    use crate::forges::tokens::{store_session, StoredForgeToken};

    fn source(via: TokenVia) -> ForgeSource {
        ForgeSource {
            id: "vm-src".to_owned(),
            kind: ForgeKind::Github,
            name: "GitHub".to_owned(),
            web_base: "https://github.com".to_owned(),
            api_base: "https://api.github.com".to_owned(),
            client_id: Some("Iv1.c".to_owned()),
            via,
        }
    }

    #[test]
    fn a_source_s_state_follows_its_connection_and_the_last_error() {
        let p = testing::FakePlatform::default();
        let device = source(TokenVia::DeviceFlow);
        assert_eq!(
            source_vm(&p, &device, None, None).state,
            ForgeStateVm::NotConnected
        );
        store_session(
            &p,
            &device,
            &StoredForgeToken {
                access_token: "t".to_owned(),
                refresh_token: None,
                expires_ms: None,
                login: "tgorka".to_owned(),
                client_id: "Iv1.c".to_owned(),
            },
        )
        .expect("seed");
        let connected = source_vm(&p, &device, None, None);
        assert_eq!(
            (
                connected.state,
                connected.login.as_deref(),
                connected.host.as_str()
            ),
            (ForgeStateVm::Connected, Some("tgorka"), "github.com")
        );

        let broker = source(TokenVia::Broker);
        assert_eq!(
            source_vm(&p, &broker, None, None).state,
            ForgeStateVm::NeedsSignIn
        );
        let d = testing::signed_in_account(&p, "");
        assert_eq!(
            source_vm(&p, &broker, Some(&d), None).state,
            ForgeStateVm::Connected
        );
        let none = ForgeError::NoAccess(
            "Your account has no GitHub access on b.acme.dev. Ask its administrator to add you."
                .to_owned(),
        );
        let vm = source_vm(&p, &broker, Some(&d), Some(&none));
        assert_eq!(vm.state, ForgeStateVm::Unreachable);
        assert_eq!(vm.sentence.as_deref(), Some(none.to_string().as_str()));
        assert_eq!(
            source_vm(&p, &broker, Some(&d), Some(&ForgeError::NeedsConnect)).state,
            ForgeStateVm::NotConnected
        );
    }

    #[test]
    fn connect_is_offered_by_the_client_id_not_by_how_tokens_come() {
        let p = testing::FakePlatform::default();
        let brokered = source(TokenVia::Broker);
        let vm = source_vm(&p, &brokered, None, None);
        assert!(vm.can_connect);
        assert_eq!(vm.credential, "forge:vm-src");
        assert_eq!(
            vm.apps_url.as_deref(),
            Some("https://github.com/settings/connections/applications/Iv1.c")
        );
        let broker_only = ForgeSource {
            client_id: None,
            ..brokered
        };
        let vm = source_vm(&p, &broker_only, None, None);
        assert!(!vm.can_connect);
        assert_eq!(vm.apps_url, None);
    }

    /// The hesperia report (2026-09-24): a github.com drive offered "Use my
    /// makistack account", which sent the account's sign-in to GitHub.
    #[test]
    fn a_github_drive_is_offered_github_and_never_the_account() {
        let p = testing::FakePlatform::default();
        let d = testing::signed_in_account(&p, "");
        let broker = source(TokenVia::Broker);

        let github = credential_choices(
            Some(&d),
            std::slice::from_ref(&broker),
            "https://github.com/o/r.git",
        );
        assert_eq!(github.account, None);
        assert_eq!(
            github
                .forges
                .iter()
                .map(|c| (c.value.as_str(), c.label.as_str()))
                .collect::<Vec<_>>(),
            [("forge:vm-src", "GitHub access through Acme")]
        );

        let own = credential_choices(Some(&d), &[broker], "https://git.acme.dev/people/notes.git");
        assert_eq!(
            own.account.as_ref().map(|c| c.label.as_str()),
            Some("Use my Acme account")
        );
        assert!(own.forges.is_empty());

        let device = source(TokenVia::DeviceFlow);
        let plain = credential_choices(None, &[device], "https://github.com/o/r");
        assert_eq!(plain.forges[0].label, "Sign in with GitHub");
    }
}
