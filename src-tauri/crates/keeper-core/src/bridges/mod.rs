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

pub mod data;
pub mod discovery;

pub use discovery::discover;

use crate::error::BridgeError;
use crate::vm::BridgeNetworkVm;

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
}
