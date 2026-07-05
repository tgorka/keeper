//! Zero-config, per-Account bridge discovery (Story 6.2, FR-25, AD-16).
//!
//! Replaces Story 6.1's static per-account catalog projection with a real,
//! best-effort discovery pass that merges **three sources** into a per-Network
//! setup/login status, keyed Network × Account:
//!
//! - **(a) `thirdparty/protocols`** — `GET
//!   /_matrix/client/v3/thirdparty/protocols`. Its keys are protocol ids that map
//!   directly to catalog `networkId`s. A homeserver that does not implement it
//!   (404 / `M_UNRECOGNIZED`) is *normal*: the source degrades to empty, never an
//!   error.
//! - **(b) known-bot MXID probe** — a `get_profile` existence check of the
//!   `known-bots.json` localparts (`@{localpart}:{server_name}`). Only probed for
//!   Networks not already found via (a)/(c), to bound round-trips.
//! - **(c) room scan** — `client.joined_rooms()`: an `m.bridge` `protocol.id`
//!   portal room marks a Network **logged in**; a bot management DM (a direct room
//!   whose target is a known bot) marks it as having a bot DM.
//!
//! The **merge is a pure function** ([`merge_discovery`]) over per-Network
//! evidence with a fixed precedence (portal > bot-DM > protocol/mxid > absent), so
//! the whole I/O matrix is unit-tested without a homeserver. All Matrix I/O lives
//! in the impure shell ([`discover`]); a single source failing is logged via
//! `tracing` and skipped — never a panic, never an abort of the whole discovery.
//!
//! Discovered Networks are **catalog-gated**: only Networks present in the 6.1
//! [`catalog`](crate::bridges::catalog) surface as [`DiscoveredBridgeVm`]s; a
//! discovered protocol with no catalog entry is logged and dropped (keeper has no
//! vetted risk data for it). No bot MXID, token, or session material crosses back
//! to the caller — only non-secret network ids and statuses.

use std::collections::{BTreeMap, BTreeSet};

use matrix_sdk::ruma::api::client::profile::get_profile;
use matrix_sdk::ruma::api::client::thirdparty::get_protocols;
use matrix_sdk::ruma::api::error::ErrorKind;
use matrix_sdk::ruma::{OwnedUserId, ServerName, UserId};
use matrix_sdk::Client;

use crate::bridge::room_bridge_protocol_id;
use crate::bridges::data::{self, KnownBotsDoc};
use crate::error::BridgeError;
use crate::vm::{BridgeDiscoveryVm, BridgeNetworkVm, BridgeStatus, DiscoveredBridgeVm};

/// Per-Network evidence gathered by the three sources, fed to the pure
/// [`merge_discovery`]. Keyed by catalog `networkId` in the shell.
///
/// `Default` is all-`false` (no evidence → the Network is not discovered).
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct NetworkEvidence {
    /// Source (a): the Network id appeared in `thirdparty/protocols`.
    pub in_protocols: bool,
    /// Source (b): a known-bot MXID for the Network resolved via `get_profile`.
    pub mxid_resolves: bool,
    /// Source (c): a bot management DM with a known bot exists (no portal).
    pub has_bot_dm: bool,
    /// Source (c): an `m.bridge` portal room for the Network exists.
    pub has_portal: bool,
}

/// The **pure** source-merge: map one Network's gathered [`NetworkEvidence`] to a
/// [`BridgeStatus`], or `None` when the Network is not discovered at all.
///
/// Precedence (honest, evidence-based):
/// 1. a portal room → **logged in** (the Network is bridged and connected);
/// 2. else a bot DM with a known bot → **not logged in** (bridge present, not yet
///    logged into the Network);
/// 3. else present only via `thirdparty/protocols` or a resolving known-bot MXID
///    → **configured**;
/// 4. else absent → `None` (no card).
///
/// This is the sole status-derivation logic; the shell only gathers evidence.
pub fn merge_discovery(evidence: NetworkEvidence) -> Option<BridgeStatus> {
    if evidence.has_portal {
        Some(BridgeStatus::LoggedIn)
    } else if evidence.has_bot_dm {
        Some(BridgeStatus::NotLoggedIn)
    } else if evidence.in_protocols || evidence.mxid_resolves {
        Some(BridgeStatus::Configured)
    } else {
        None
    }
}

/// Run the per-Account discovery pass against a live `Client` (the impure shell).
///
/// Gathers the three sources, catalog-gates the union, merges each Network's
/// evidence via [`merge_discovery`], and returns a [`BridgeDiscoveryVm`] whose
/// `homeserver` is the account's server name. A *total* transport failure
/// (no resolvable user id / server name to probe against) surfaces as
/// [`BridgeError::Discovery`] (retriable); individual source failures degrade.
pub async fn discover(client: &Client) -> Result<BridgeDiscoveryVm, BridgeError> {
    // The account's own user id yields the server name every bot MXID and the
    // empty-state copy is built from. Absent it, discovery cannot proceed at all.
    let own_user = client.user_id().ok_or_else(|| {
        BridgeError::Discovery("account has no resolved user id (not logged in?)".to_owned())
    })?;
    let server_name = own_user.server_name().to_owned();

    let catalog = crate::bridges::catalog()?;
    let catalog_ids: BTreeSet<&str> = catalog.iter().map(|n| n.network_id.as_str()).collect();
    let known_bots = data::known_bots()?;

    // Source (a): protocols. A missing endpoint (404 / M_UNRECOGNIZED) is normal —
    // degrade to an empty set rather than failing discovery. A genuine transport
    // failure (homeserver unreachable) instead surfaces as a retriable error rather
    // than being silently indistinguishable from "no bridges".
    let protocol_ids = fetch_protocol_ids(client, &catalog_ids).await?;

    // Source (c): scan joined rooms once for portals and bot DMs.
    let scan = scan_rooms(client, known_bots, &catalog_ids, &server_name).await;

    // Assemble per-Network evidence for every catalog Network seen by (a) or (c).
    let mut evidence: BTreeMap<String, NetworkEvidence> = BTreeMap::new();
    for id in &protocol_ids {
        evidence.entry(id.clone()).or_default().in_protocols = true;
    }
    for id in &scan.portals {
        evidence.entry(id.clone()).or_default().has_portal = true;
    }
    for id in &scan.bot_dms {
        evidence.entry(id.clone()).or_default().has_bot_dm = true;
    }

    // Source (b): probe known-bot MXIDs only for catalog Networks NOT already found
    // by (a) or (c), to bound round-trips.
    for bot in &known_bots.bots {
        let network_id = bot.network_id.as_str();
        if !catalog_ids.contains(network_id) || evidence.contains_key(network_id) {
            continue;
        }
        if probe_network_bots(client, &bot.localparts, server_name.as_str()).await {
            evidence
                .entry(network_id.to_owned())
                .or_default()
                .mxid_resolves = true;
        }
    }

    // Merge each Network's evidence to a status (catalog order for a stable list).
    let networks = merge_catalog(&catalog, &evidence);

    Ok(BridgeDiscoveryVm {
        homeserver: server_name.to_string(),
        networks,
    })
}

/// Merge gathered evidence into the catalog-ordered discovered set. Only
/// Networks that both exist in the catalog and merge to a status surface.
fn merge_catalog(
    catalog: &[BridgeNetworkVm],
    evidence: &BTreeMap<String, NetworkEvidence>,
) -> Vec<DiscoveredBridgeVm> {
    let mut networks = Vec::new();
    for network in catalog {
        let ev = evidence
            .get(&network.network_id)
            .copied()
            .unwrap_or_default();
        if let Some(status) = merge_discovery(ev) {
            networks.push(DiscoveredBridgeVm {
                network_id: network.network_id.clone(),
                status,
            });
        }
    }
    networks
}

/// Whether a `thirdparty/protocols` error is the expected "endpoint not
/// implemented" case — degrade to sources (b)+(c) — rather than a real failure
/// that must surface as a retriable discovery error. Only the spec's named kinds
/// (`M_NOT_FOUND` / `M_UNRECOGNIZED`) degrade; a transport failure (no client-API
/// error kind → connection/DNS/TLS) or any other server errcode fails discovery
/// loudly so "couldn't check" is never rendered as the honest "no bridges found".
fn protocols_error_degrades(kind: Option<&ErrorKind>) -> bool {
    matches!(
        kind,
        Some(ErrorKind::NotFound) | Some(ErrorKind::Unrecognized)
    )
}

/// Source (a): fetch the homeserver's `thirdparty/protocols` and keep only keys
/// that are catalog Networks. A missing endpoint (404 / `M_UNRECOGNIZED`) yields an
/// empty set (logged) — degrade to sources (b)+(c). A genuine transport failure or
/// other errcode returns [`BridgeError::Discovery`] (retriable). Uncatalogued
/// protocol ids are logged and dropped.
async fn fetch_protocol_ids(
    client: &Client,
    catalog_ids: &BTreeSet<&str>,
) -> Result<BTreeSet<String>, BridgeError> {
    let mut out = BTreeSet::new();
    match client.send(get_protocols::v3::Request::new()).await {
        Ok(resp) => {
            for protocol_id in resp.protocols.into_keys() {
                if catalog_ids.contains(protocol_id.as_str()) {
                    out.insert(protocol_id);
                } else {
                    tracing::debug!(
                        protocol = %protocol_id,
                        "discovery: thirdparty/protocols listed an uncatalogued network; ignoring"
                    );
                }
            }
        }
        Err(e) => {
            if protocols_error_degrades(e.client_api_error_kind()) {
                // The common, expected case: the homeserver doesn't implement the
                // endpoint. Degrade to the bot probe + room scan.
                tracing::debug!(
                    error = %e,
                    "discovery: thirdparty/protocols not implemented; degrading to bot probe + room scan"
                );
            } else {
                // Transport failure or an unexpected errcode: the homeserver couldn't
                // be reached/queried, so we cannot honestly claim "no bridges".
                tracing::warn!(error = %e, "discovery: thirdparty/protocols request failed");
                return Err(BridgeError::Discovery(
                    "could not reach the homeserver to discover bridges".to_owned(),
                ));
            }
        }
    }
    Ok(out)
}

/// The catalog network ids found by the joined-room scan (source c).
#[derive(Debug, Default)]
struct RoomScan {
    /// Networks with an `m.bridge` portal room.
    portals: BTreeSet<String>,
    /// Networks with a bot management DM (known bot direct target).
    bot_dms: BTreeSet<String>,
}

/// Source (c): scan the account's joined rooms once. A room carrying `m.bridge`
/// with a catalog `protocol.id` is a portal for that Network; a direct room whose
/// target is a known bot localpart (on this server) is a bot management DM.
async fn scan_rooms(
    client: &Client,
    known_bots: &KnownBotsDoc,
    catalog_ids: &BTreeSet<&str>,
    own_server: &ServerName,
) -> RoomScan {
    // localpart → networkId, restricted to catalog Networks (join key).
    let mut bot_localparts: BTreeMap<&str, &str> = BTreeMap::new();
    for bot in &known_bots.bots {
        if !catalog_ids.contains(bot.network_id.as_str()) {
            continue;
        }
        for localpart in &bot.localparts {
            bot_localparts.insert(localpart.as_str(), bot.network_id.as_str());
        }
    }

    let mut scan = RoomScan::default();
    for room in client.joined_rooms() {
        // Portal: an m.bridge state event with a catalog protocol.id.
        if let Some(protocol_id) = room_bridge_protocol_id(&room).await {
            if catalog_ids.contains(protocol_id.as_str()) {
                scan.portals.insert(protocol_id);
            } else {
                tracing::debug!(
                    protocol = %protocol_id,
                    "discovery: portal room bridges an uncatalogued network; ignoring"
                );
            }
        }

        // Bot management DM: a direct room whose target is a known bot on this
        // account's server. `is_direct` reads the store; a store error is logged
        // and the room skipped (best-effort).
        match room.is_direct().await {
            Ok(true) => {
                for target in room.direct_targets() {
                    if let Some(user_id) = target.as_user_id() {
                        if let Some(network_id) =
                            bot_network_for(user_id, &bot_localparts, own_server)
                        {
                            scan.bot_dms.insert(network_id.to_owned());
                        }
                    }
                }
            }
            Ok(false) => {}
            Err(e) => {
                tracing::debug!(error = %e, "discovery: could not read room directness; skipping");
            }
        }
    }
    scan
}

/// Resolve a DM target `user_id` to a catalog networkId iff its localpart is a
/// known bot AND it lives on the account's own server (a bot on a foreign server
/// is not this account's management DM). `own_server` is resolved once by the
/// caller from the account's user id, so this stays a pure lookup.
fn bot_network_for<'a>(
    user_id: &UserId,
    bot_localparts: &BTreeMap<&str, &'a str>,
    own_server: &ServerName,
) -> Option<&'a str> {
    if user_id.server_name() != own_server {
        return None;
    }
    bot_localparts.get(user_id.localpart()).copied()
}

/// Source (b): probe a Network's candidate bot localparts via `get_profile` until
/// one resolves. `Ok` → present; `Err` with `M_NOT_FOUND` → absent (not an error);
/// any other / unknown error kind → skip that probe and log it (best-effort). A
/// malformed MXID skips too.
async fn probe_network_bots(client: &Client, localparts: &[String], server_name: &str) -> bool {
    for localpart in localparts {
        let mxid = format!("@{localpart}:{server_name}");
        let user_id = match OwnedUserId::try_from(mxid.as_str()) {
            Ok(user_id) => user_id,
            Err(e) => {
                tracing::debug!(error = %e, "discovery: skipping malformed bot mxid");
                continue;
            }
        };
        match client.send(get_profile::v3::Request::new(user_id)).await {
            Ok(_) => return true,
            Err(e) => match e.client_api_error_kind() {
                Some(ErrorKind::NotFound) => {}
                _ => {
                    tracing::debug!(
                        error = %e,
                        "discovery: bot mxid probe inconclusive; skipping this localpart"
                    );
                }
            },
        }
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The full I/O matrix, exercised on the pure merge — no homeserver needed.
    #[test]
    fn portal_beats_everything_logged_in() {
        let ev = NetworkEvidence {
            in_protocols: true,
            mxid_resolves: true,
            has_bot_dm: true,
            has_portal: true,
        };
        assert_eq!(merge_discovery(ev), Some(BridgeStatus::LoggedIn));
    }

    #[test]
    fn bot_dm_no_portal_is_not_logged_in() {
        let ev = NetworkEvidence {
            has_bot_dm: true,
            // even with protocol/mxid evidence, a bot DM (no portal) is not-logged-in
            in_protocols: true,
            mxid_resolves: true,
            has_portal: false,
        };
        assert_eq!(merge_discovery(ev), Some(BridgeStatus::NotLoggedIn));
    }

    #[test]
    fn protocol_only_is_configured() {
        let ev = NetworkEvidence {
            in_protocols: true,
            ..NetworkEvidence::default()
        };
        assert_eq!(merge_discovery(ev), Some(BridgeStatus::Configured));
    }

    #[test]
    fn resolving_mxid_only_is_configured() {
        let ev = NetworkEvidence {
            mxid_resolves: true,
            ..NetworkEvidence::default()
        };
        assert_eq!(merge_discovery(ev), Some(BridgeStatus::Configured));
    }

    #[test]
    fn no_evidence_is_absent() {
        assert_eq!(merge_discovery(NetworkEvidence::default()), None);
    }

    #[test]
    fn precedence_portal_over_bot_dm() {
        let ev = NetworkEvidence {
            has_bot_dm: true,
            has_portal: true,
            ..NetworkEvidence::default()
        };
        // Portal wins even though a bot DM also exists.
        assert_eq!(merge_discovery(ev), Some(BridgeStatus::LoggedIn));
    }

    #[test]
    fn precedence_bot_dm_over_protocol_and_mxid() {
        let ev = NetworkEvidence {
            has_bot_dm: true,
            in_protocols: true,
            mxid_resolves: true,
            has_portal: false,
        };
        assert_eq!(merge_discovery(ev), Some(BridgeStatus::NotLoggedIn));
    }

    #[test]
    fn merge_catalog_gates_and_orders_and_drops_uncatalogued() {
        // A tiny catalog stand-in in a fixed order.
        let catalog = vec![
            sample_network("matrix"),
            sample_network("whatsapp"),
            sample_network("signal"),
        ];
        let mut evidence: BTreeMap<String, NetworkEvidence> = BTreeMap::new();
        // whatsapp: portal → logged in.
        evidence.insert(
            "whatsapp".to_owned(),
            NetworkEvidence {
                has_portal: true,
                ..NetworkEvidence::default()
            },
        );
        // signal: protocol only → configured.
        evidence.insert(
            "signal".to_owned(),
            NetworkEvidence {
                in_protocols: true,
                ..NetworkEvidence::default()
            },
        );
        // An uncatalogued network with evidence must NOT surface.
        evidence.insert(
            "bogusnet".to_owned(),
            NetworkEvidence {
                in_protocols: true,
                ..NetworkEvidence::default()
            },
        );

        let discovered = merge_catalog(&catalog, &evidence);
        // matrix has no evidence → dropped; only whatsapp + signal, in catalog order.
        assert_eq!(discovered.len(), 2);
        assert_eq!(discovered[0].network_id, "whatsapp");
        assert_eq!(discovered[0].status, BridgeStatus::LoggedIn);
        assert_eq!(discovered[1].network_id, "signal");
        assert_eq!(discovered[1].status, BridgeStatus::Configured);
    }

    #[test]
    fn protocols_error_only_degrades_on_endpoint_unsupported() {
        // The expected "endpoint not implemented" kinds degrade to sources (b)+(c).
        assert!(protocols_error_degrades(Some(&ErrorKind::NotFound)));
        assert!(protocols_error_degrades(Some(&ErrorKind::Unrecognized)));
        // A transport failure has no client-API error kind — it must NOT degrade, so a
        // genuinely unreachable homeserver surfaces a retriable error instead of a
        // false "no bridges found".
        assert!(!protocols_error_degrades(None));
    }

    fn sample_network(id: &str) -> BridgeNetworkVm {
        use crate::vm::{BadgeStyle, RiskTier};
        BridgeNetworkVm {
            network_id: id.to_owned(),
            name: id.to_owned(),
            glyph: "XX".to_owned(),
            tier: RiskTier::Low,
            tier_label: "Low".to_owned(),
            badge_style: BadgeStyle::Secondary,
            requires_ack: false,
            ack_copy: None,
        }
    }
}
