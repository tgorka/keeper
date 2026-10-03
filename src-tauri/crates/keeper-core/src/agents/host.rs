//! A host's manifest: `dev.keeper.agent.host` in its principal's control room,
//! keyed by the host's slug (AD-374; story 90.6).
//!
//! It says what the host can do — its capabilities, its drives and how much
//! of each is on disk, the bots it resolves, the agents it hosts — and is
//! renewed every 60 s with an expiry 180 s ahead, so placement
//! ([`crate::agents::placement`]) reads only hosts that are alive. A state
//! event is not encrypted, so the manifest names no provider's address: a
//! bot is its [`bot_id`], a digest of its reference (S-33).

use chrono::DateTime;
use matrix_sdk::ruma::{OwnedUserId, UserId};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};

use crate::agents::agentd::fits_slug;
use crate::agents::events::CONTENT_VERSION;
use crate::agents::home::BotRef;

/// How many hex digits of the reference's SHA-256 a bot id keeps.
pub const BOT_ID_HEX: usize = 16;

/// `dev.keeper.agent.host`. A key this build does not know is ignored, so a
/// newer host that adds one at the same `v` is still believed; a higher `v`
/// is not ([`accept`]).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HostManifest {
    pub v: u32,
    pub host: String,
    pub principal: String,
    /// The host's build.
    pub version: String,
    pub always_on: bool,
    /// Capabilities: `sandbox`, `mcp:<name>`, `screen:mac`, `kvm:<id>`, `voice`.
    pub tools: Vec<String>,
    pub drives: Vec<HostDrive>,
    /// [`bot_id`]s of the bots this host resolves.
    pub bots: Vec<String>,
    /// `<drive>/<agent>` of each agent it hosts.
    pub agents: Vec<String>,
    pub renewed_at: String,
    pub expires_at: String,
}

/// A drive as one host has it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HostDrive {
    pub id: String,
    /// Checked out on this host.
    pub present: bool,
    pub materialized: Materialized,
}

/// How much of a drive's content is on the host's disk.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Materialized {
    /// Every file.
    Full,
    /// Some files, the agents zone among them.
    Partial,
    /// Listed, not on disk: no agent can read it here.
    Virtual,
}

/// A bot's id in a manifest: the first [`BOT_ID_HEX`] hex digits of the
/// SHA-256 of its `bot:{kind}:{base}#{target}` reference, the base URL
/// normalised as [`BotRef::parse`] stores it.
pub fn bot_id(bot: &BotRef) -> String {
    let reference = format!(
        "bot:{}:{}#{}",
        bot.kind.as_registry_str(),
        bot.base,
        bot.target
    );
    let mut id = hex::encode(Sha256::digest(reference.as_bytes()));
    id.truncate(BOT_ID_HEX);
    id
}

impl HostManifest {
    /// Whether the host is alive at `server_now` (ms, the server's clock).
    pub fn is_live(&self, server_now: u64) -> bool {
        DateTime::parse_from_rfc3339(&self.expires_at)
            .is_ok_and(|at| u64::try_from(at.timestamp_millis()).is_ok_and(|at| at > server_now))
    }
}

/// Why a manifest is not believed.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum Rejected {
    #[error("the manifest under key {state_key} names host {host}")]
    NotItsKey { state_key: String, host: String },
    #[error("{0} is not one of this principal's agents, so its manifest is not believed")]
    ForeignSender(OwnedUserId),
    #[error("the manifest does not read: {0}")]
    Shape(String),
    #[error("the manifest is version {0}, newer than this host reads")]
    Newer(u64),
}

/// Believe a manifest only when its `host` is its state key, the sender is
/// one of the principal's agent users, its `v` is not newer than this
/// build's, and it reads as the schema (every bot a bot id, every slug a
/// slug); keys the schema does not name are ignored.
pub fn accept(
    state_key: &str,
    sender: &UserId,
    content: &Value,
    principal_agent_users: &[OwnedUserId],
) -> Result<HostManifest, Rejected> {
    if !principal_agent_users.iter().any(|user| user == sender) {
        return Err(Rejected::ForeignSender(sender.to_owned()));
    }
    if let Some(v) = content["v"]
        .as_u64()
        .filter(|v| *v > u64::from(CONTENT_VERSION))
    {
        return Err(Rejected::Newer(v));
    }
    let manifest: HostManifest = serde_json::from_value(content.clone())
        .map_err(|error| Rejected::Shape(error.to_string()))?;
    if manifest.host != state_key {
        return Err(Rejected::NotItsKey {
            state_key: state_key.to_owned(),
            host: manifest.host,
        });
    }
    if !fits_slug(&manifest.host) || !fits_slug(&manifest.principal) {
        return Err(Rejected::Shape(
            "a host or principal is not a slug".to_owned(),
        ));
    }
    if let Some(bad) = manifest.bots.iter().find(|bot| {
        bot.len() != BOT_ID_HEX || !bot.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f'))
    }) {
        return Err(Rejected::Shape(format!("\"{bad}\" is not a bot id")));
    }
    Ok(manifest)
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    use serde_json::json;

    use super::*;
    use crate::agents::claim::{rfc3339, Claimant};

    fn user(id: &str) -> OwnedUserId {
        OwnedUserId::try_from(id).expect("user")
    }

    fn bot(text: &str) -> BotRef {
        BotRef::parse(text).expect("bot reference")
    }

    fn manifest(host: &str) -> HostManifest {
        HostManifest {
            v: 1,
            host: host.to_owned(),
            principal: "tgorka".to_owned(),
            version: "0.90.0".to_owned(),
            always_on: true,
            tools: vec!["sandbox".to_owned(), "mcp:forge".to_owned()],
            drives: vec![HostDrive {
                id: "tgdrive".to_owned(),
                present: true,
                materialized: Materialized::Full,
            }],
            bots: vec![bot_id(&bot(
                "bot:openai:https://provider.example:8452#claude-opus",
            ))],
            agents: vec!["tgdrive/nixi".to_owned()],
            renewed_at: "2026-10-03T12:00:00Z".to_owned(),
            expires_at: "2026-10-03T12:03:00Z".to_owned(),
        }
    }

    #[test]
    fn a_manifest_whose_host_is_not_its_state_key_is_rejected() {
        let agents = [user("@nixi:example.org")];
        let content = serde_json::to_value(manifest("electra")).expect("serialise");
        assert_eq!(
            accept("electra", &agents[0], &content, &agents).expect("its own key"),
            manifest("electra")
        );
        assert_eq!(
            accept("hesperia", &agents[0], &content, &agents),
            Err(Rejected::NotItsKey {
                state_key: "hesperia".to_owned(),
                host: "electra".to_owned()
            })
        );
    }

    #[test]
    fn a_manifest_from_a_foreign_sender_is_rejected() {
        let agents = [user("@nixi:example.org"), user("@amelia:example.org")];
        let content = serde_json::to_value(manifest("electra")).expect("serialise");
        assert!(accept("electra", &agents[1], &content, &agents).is_ok());
        let stranger = user("@tgorka:example.org");
        assert_eq!(
            accept("electra", &stranger, &content, &agents),
            Err(Rejected::ForeignSender(stranger.clone()))
        );
        let mut address = content;
        address["bots"] = json!(["https://provider.example:8452"]);
        assert!(matches!(
            accept("electra", &agents[0], &address, &agents),
            Err(Rejected::Shape(_))
        ));
    }

    /// A newer agentd that adds a manifest key at the same `v` is still
    /// believed, so an older host does not take it for dead; a higher `v`
    /// is not believed.
    #[test]
    fn a_manifest_with_an_unknown_key_is_believed_and_a_newer_version_is_not() {
        let agents = [user("@nixi:example.org")];
        let mut added = serde_json::to_value(manifest("electra")).expect("serialise");
        added["voices"] = json!(["en"]);
        added["drives"][0]["synced_at"] = json!("2026-10-03T12:00:00Z");
        assert_eq!(
            accept("electra", &agents[0], &added, &agents),
            Ok(manifest("electra"))
        );
        let mut newer = added;
        newer["v"] = json!(2);
        assert_eq!(
            accept("electra", &agents[0], &newer, &agents),
            Err(Rejected::Newer(2))
        );
    }

    #[test]
    fn a_host_is_live_until_its_expiry_by_the_server_clock() {
        let at = DateTime::parse_from_rfc3339("2026-10-03T12:03:00Z")
            .expect("time")
            .timestamp_millis() as u64;
        let host = manifest("electra");
        assert!(host.is_live(at - 1));
        assert!(!host.is_live(at));
    }

    #[test]
    fn a_bot_id_is_the_normalised_references_digest() {
        let slash = bot("bot:openai:https://Provider.example:8452/#claude-opus");
        let plain = bot("bot:openai:https://provider.example:8452#claude-opus");
        assert_eq!(bot_id(&slash), bot_id(&plain));
        assert_eq!(bot_id(&plain).len(), BOT_ID_HEX);
        assert_eq!(
            bot_id(&plain),
            hex::encode(Sha256::digest(
                b"bot:openai:https://provider.example:8452#claude-opus"
            ))[..BOT_ID_HEX]
        );
        assert_ne!(
            bot_id(&plain),
            bot_id(&bot("bot:openai:https://other.example:8452#claude-opus"))
        );
    }

    fn keys(value: &Value) -> BTreeSet<String> {
        value.as_object().expect("object").keys().cloned().collect()
    }

    /// Ambiguity 7 and S-33: the unencrypted control metadata holds exactly
    /// its schema's keys and says at most that work exists — no title, no
    /// path, no text, no provider address.
    #[test]
    fn claims_and_manifests_carry_no_content() {
        let claimant = Claimant {
            host: "electra".to_owned(),
            device: "ELECTRA1".to_owned(),
            agent: user("@nixi:example.org"),
        };
        let claim = serde_json::to_value(claimant.content(
            2,
            1_790_000_000_000,
            1_790_000_060_000,
            false,
            Some(rfc3339(1_790_000_000_000)),
        ))
        .expect("claim");
        assert_eq!(
            keys(&claim),
            [
                "acquired_at",
                "agent",
                "device",
                "epoch",
                "expires_at",
                "host",
                "released",
                "renewed_at",
                "v",
                "window"
            ]
            .map(str::to_owned)
            .into()
        );
        let window = claim["window"].as_str().expect("window");
        assert!(DateTime::parse_from_rfc3339(window).is_ok(), "a timestamp");

        let host = serde_json::to_value(manifest("electra")).expect("manifest");
        assert_eq!(
            keys(&host),
            [
                "agents",
                "always_on",
                "bots",
                "drives",
                "expires_at",
                "host",
                "principal",
                "renewed_at",
                "tools",
                "v",
                "version"
            ]
            .map(str::to_owned)
            .into()
        );
        assert_eq!(
            keys(&host["drives"][0]),
            ["id", "materialized", "present"].map(str::to_owned).into()
        );
        for text in [claim.to_string(), host.to_string()] {
            for leak in ["http", "provider.example", "bot:", "title", "path"] {
                assert!(!text.contains(leak), "{leak} in {text}");
            }
        }
    }
}
