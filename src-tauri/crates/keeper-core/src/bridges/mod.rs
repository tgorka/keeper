//! The data-driven bridge catalog (Story 6.1, FR-42, Epic 6).
//!
//! Projects the surfaced tiers of the embedded `risk-tiers.json` into a flat
//! [`Vec<BridgeNetworkVm>`] — one entry per connectable Network, carrying its
//! resolved [`RiskTier`], display label, [`BadgeStyle`], and acknowledgment copy.
//! The out-of-scope tier stays in the data file for completeness but is excluded
//! here (its networks are never surfaced as cards). The catalog is
//! account-agnostic: the frontend keys a card per Network × Account, and later
//! stories (6.2 discovery, 6.5 health) layer live status on top of this static
//! projection. No risk/badge/ack copy is hardcoded outside the data file.

pub mod bbctl;
pub mod data;
pub mod discovery;
pub mod health;
pub mod login;
pub mod transport;

pub use discovery::discover;

use matrix_sdk::ruma::{OwnedUserId, UserId};
use matrix_sdk::{Client, Room};

use crate::error::BridgeError;
use crate::vm::{BridgeNetworkVm, CouplingCaveatVm};

/// Resolve the Bridge Bot MXID for `network_id` on the account's own server (Story
/// 6.4, FR-27). Builds `@{localpart}:{server_name}` from the first `known-bots.json`
/// localpart for the network, using the account's Matrix `server_name` (a Matrix
/// user id — `server_name` is correct here, unlike 6.3's resolved HTTP host).
///
/// Returns [`BridgeError::Bot`] when the account has no user id, the network has no
/// known-bot entry, or the composed MXID is malformed — the caller surfaces it
/// (no silent bot fallback to an unknown bot).
pub fn resolve_bot_mxid(client: &Client, network_id: &str) -> Result<OwnedUserId, BridgeError> {
    let own_user = client.user_id().ok_or_else(|| {
        BridgeError::Bot("account has no resolved user id (not logged in?)".to_owned())
    })?;
    let server_name = own_user.server_name();
    let known = data::known_bots()?;
    let entry = known
        .bots
        .iter()
        .find(|b| b.network_id == network_id)
        .ok_or_else(|| {
            BridgeError::Bot(format!(
                "no known Bridge Bot for {network_id} to fall back to"
            ))
        })?;
    // The registry guarantees ≥1 non-empty localpart (validated on load).
    let localpart = entry.localparts.first().ok_or_else(|| {
        BridgeError::Bot(format!("no known Bridge Bot localpart for {network_id}"))
    })?;
    let mxid = format!("@{localpart}:{server_name}");
    OwnedUserId::try_from(mxid.as_str())
        .map_err(|e| BridgeError::Bot(format!("could not build the Bridge Bot MXID: {e}")))
}

/// Resolve-or-create the Bridge Bot DM `Room` for `network_id` on the account's
/// `client` (Story 6.4, FR-27, UX-DR19), returning the room and the bot's MXID.
///
/// Reuses the discovery DM-scan pattern: find an existing direct room among
/// `client.joined_rooms()` whose target is the bot MXID; otherwise create one via
/// `client.create_dm`. Shared by `BotDriver` construction and the `bridge_bot_room`
/// command. Returns [`BridgeError::Bot`] when the bot is unresolvable or the DM can't
/// be created.
pub async fn resolve_bot_room(
    client: &Client,
    network_id: &str,
) -> Result<(Room, OwnedUserId), BridgeError> {
    let bot_mxid = resolve_bot_mxid(client, network_id)?;
    if let Some(room) = find_bot_dm(client, &bot_mxid).await {
        return Ok((room, bot_mxid));
    }
    let room = client
        .create_dm(&bot_mxid)
        .await
        .map_err(|e| BridgeError::Bot(format!("could not open a chat with the Bridge Bot: {e}")))?;
    Ok((room, bot_mxid))
}

/// Resolve the bot management DM for `network_id` **only if it already exists** — never
/// creating one. This is the passive counterpart of [`resolve_bot_room`], used by the
/// health monitor, which must observe existing conversations without provoking any
/// server-side room creation on a background/launch code path. Returns `None` when the
/// bot MXID can't be resolved or the account has no existing DM with it.
pub async fn find_bot_room(client: &Client, network_id: &str) -> Option<(Room, OwnedUserId)> {
    let bot_mxid = resolve_bot_mxid(client, network_id).ok()?;
    let room = find_bot_dm(client, &bot_mxid).await?;
    Some((room, bot_mxid))
}

/// Find an existing direct room whose target is `bot_mxid` among the account's
/// joined rooms (best-effort — a store error reading directness is logged and the
/// room skipped, mirroring discovery's DM scan).
async fn find_bot_dm(client: &Client, bot_mxid: &UserId) -> Option<Room> {
    for room in client.joined_rooms() {
        match room.is_direct().await {
            Ok(true) => {
                if room
                    .direct_targets()
                    .iter()
                    .filter_map(|t| t.as_user_id())
                    .any(|user_id| user_id == bot_mxid)
                {
                    return Some(room);
                }
            }
            Ok(false) => {}
            Err(e) => {
                tracing::debug!(error = %e, "bot-room resolve: could not read room directness; skipping");
            }
        }
    }
    None
}

/// Build the flat bridge catalog from the embedded risk-tier data.
///
/// Flattens every *surfaced* tier's networks into [`BridgeNetworkVm`]s in file
/// order, resolving the tier enum, label, badge style, and ack copy from the tier
/// row. `ack_copy` is `Some(acknowledgment)` iff the tier `requires_ack`. Returns a
/// [`BridgeError`] only if the embedded data fails to parse or validate (never a
/// panic).
pub fn catalog() -> Result<Vec<BridgeNetworkVm>, BridgeError> {
    let doc = data::risk_tiers()?;
    let mut out = Vec::new();
    for tier in &doc.tiers {
        if !tier.surfaced {
            continue;
        }
        // A surfaced tier must resolve to a known enum variant; the out-of-scope id
        // is the only unsurfaced one, already skipped above.
        let Some(risk_tier) = data::tier_from_id(&tier.id) else {
            return Err(BridgeError::Data(format!(
                "risk-tiers.json surfaced tier {:?} has no known RiskTier mapping",
                tier.id
            )));
        };
        let ack_copy = if tier.requires_ack {
            Some(tier.acknowledgment.clone())
        } else {
            None
        };
        for network in &tier.networks {
            out.push(BridgeNetworkVm {
                network_id: network.id.clone(),
                name: network.name.clone(),
                glyph: network.glyph.clone(),
                tier: risk_tier,
                tier_label: tier.label.clone(),
                badge_style: tier.badge,
                requires_ack: tier.requires_ack,
                ack_copy: ack_copy.clone(),
            });
        }
    }
    Ok(out)
}

/// Build the flat coupling-caveats catalog from the embedded data file (Story 8.2,
/// FR-44).
///
/// Projects every caveat in `coupling-caveats.json` into a [`CouplingCaveatVm`] in
/// file order — the stable `network_id`, the human-readable `text`, and the
/// `applies_to` machine tag. Read-only and account-agnostic: the frontend joins a
/// caveat to the open room's Network by `network_id`. Returns a [`BridgeError`] only
/// if the embedded data fails to parse or validate (never a panic). Mirrors
/// [`catalog()`].
pub fn coupling_caveats_catalog() -> Result<Vec<CouplingCaveatVm>, BridgeError> {
    let doc = data::coupling_caveats()?;
    Ok(doc
        .caveats
        .iter()
        .map(|caveat| CouplingCaveatVm {
            network_id: caveat.network_id.clone(),
            text: caveat.text.clone(),
            applies_to: caveat.applies_to.clone(),
        })
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::vm::RiskTier;

    #[test]
    fn catalog_builds_and_excludes_out_of_scope() {
        let catalog = catalog().expect("catalog builds");
        assert!(!catalog.is_empty(), "catalog must not be empty");
        // No out-of-scope network id leaks into the surfaced catalog.
        assert!(
            catalog.iter().all(|n| n.network_id != "wechat"
                && n.network_id != "x-dm-api"
                && n.network_id != "imessage-no-mac"),
            "out-of-scope networks must be excluded"
        );
    }

    #[test]
    fn ack_copy_present_iff_requires_ack() {
        let catalog = catalog().expect("catalog builds");
        for network in &catalog {
            assert_eq!(
                network.requires_ack,
                network.ack_copy.is_some(),
                "ackCopy must be Some iff requiresAck for {}",
                network.network_id
            );
        }
    }

    #[test]
    fn volatile_network_carries_ban_acknowledgment() {
        let catalog = catalog().expect("catalog builds");
        let instagram = catalog
            .iter()
            .find(|n| n.network_id == "instagram")
            .expect("instagram present");
        assert_eq!(instagram.tier, RiskTier::Volatile);
        assert!(instagram.requires_ack);
        let ack = instagram.ack_copy.as_deref().expect("ack copy present");
        assert!(
            ack.contains("Terms of Service"),
            "volatile ack must mention ToS: {ack}"
        );
    }

    #[test]
    fn low_risk_network_needs_no_ack() {
        let catalog = catalog().expect("catalog builds");
        let matrix = catalog
            .iter()
            .find(|n| n.network_id == "matrix")
            .expect("matrix present");
        assert_eq!(matrix.tier, RiskTier::Low);
        assert!(!matrix.requires_ack);
        assert!(matrix.ack_copy.is_none());
    }

    #[test]
    fn catalog_matches_addendum_2_surfaced_set() {
        use std::collections::BTreeSet;
        let catalog = catalog().expect("catalog builds");
        let ids_for = |tier: RiskTier| -> BTreeSet<String> {
            catalog
                .iter()
                .filter(|n| n.tier == tier)
                .map(|n| n.network_id.clone())
                .collect()
        };
        let set = |ids: &[&str]| ids.iter().map(|s| (*s).to_owned()).collect::<BTreeSet<_>>();
        // The exact surfaced set per tier, locked to addendum §2. A future data-file
        // edit that drops or moves a network fails here instead of silently drifting.
        assert_eq!(
            ids_for(RiskTier::Low),
            set(&["matrix", "telegram", "google"])
        );
        assert_eq!(
            ids_for(RiskTier::Maintenance),
            set(&["signal", "whatsapp", "discord", "slack"])
        );
        assert_eq!(
            ids_for(RiskTier::Volatile),
            set(&["instagram", "messenger", "linkedin", "xchat"])
        );
        assert_eq!(ids_for(RiskTier::Conditional), set(&["imessage"]));
        // Exactly those networks are surfaced — nothing else leaked in.
        assert_eq!(catalog.len(), 3 + 4 + 4 + 1);
    }

    #[test]
    fn known_bot_network_ids_are_all_catalog_networks() {
        use std::collections::HashSet;
        let catalog = catalog().expect("catalog builds");
        let catalog_ids: HashSet<&str> = catalog.iter().map(|n| n.network_id.as_str()).collect();
        let bots = data::known_bots().expect("known bots parse");
        // Story 6.2 joins known-bots to the catalog by networkId; every seed entry
        // must name a surfaced catalog network so the future join can't miss.
        for bot in &bots.bots {
            assert!(
                catalog_ids.contains(bot.network_id.as_str()),
                "known-bots networkId {:?} is not a surfaced catalog network",
                bot.network_id
            );
        }
    }

    #[test]
    fn coupling_caveat_network_ids_are_all_catalog_networks() {
        use std::collections::HashSet;
        let catalog = catalog().expect("catalog builds");
        let catalog_ids: HashSet<&str> = catalog.iter().map(|n| n.network_id.as_str()).collect();
        let caveats = data::coupling_caveats().expect("coupling caveats parse");
        // Epic 8 (FR-44) joins coupling caveats to the catalog by networkId; a typo'd
        // caveat networkId would parse, validate, and then silently never match — so
        // pin every seed caveat to a real surfaced network here.
        for caveat in &caveats.caveats {
            assert!(
                catalog_ids.contains(caveat.network_id.as_str()),
                "coupling-caveats networkId {:?} is not a surfaced catalog network",
                caveat.network_id
            );
        }
    }

    #[test]
    fn coupling_caveats_catalog_projects_seeded_caveats() {
        let caveats = coupling_caveats_catalog().expect("coupling caveats catalog builds");
        assert!(
            !caveats.is_empty(),
            "coupling caveats catalog must not be empty"
        );
        // Every projected caveat must carry non-empty text and a real networkId — a
        // caveat with blank copy would render an empty inline hint (FR-44).
        for caveat in &caveats {
            assert!(
                !caveat.text.trim().is_empty(),
                "caveat text must be non-empty for {}",
                caveat.network_id
            );
            assert!(
                !caveat.network_id.trim().is_empty(),
                "caveat networkId must be non-empty"
            );
        }
        // The WhatsApp read-receipt coupling seed (Story 6.1) is the one FR-44 surfaces
        // inline; pin that it projects with non-empty text.
        let whatsapp = caveats
            .iter()
            .find(|c| c.network_id == "whatsapp")
            .expect("whatsapp coupling caveat present");
        assert!(
            !whatsapp.text.trim().is_empty(),
            "whatsapp caveat text must be non-empty"
        );
    }
}
