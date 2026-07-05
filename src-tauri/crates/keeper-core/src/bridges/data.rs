//! Embedded bridge data files: parse, validate, cache (Story 6.1, Epic 6).
//!
//! The three versioned JSON files under `keeper-core/data/` are the single source
//! of truth for the Bridges surface — risk tiers → networks + badge + ack copy,
//! per-network coupling caveats, and the known-bot registry. They are embedded at
//! compile time with `include_str!` (no runtime file-not-found path), parsed once
//! into a process-wide [`OnceLock`], and validated on first access. Every fallible
//! path returns a [`BridgeError`] — this module never `.unwrap()`s a parse.
//!
//! `coupling-caveats.json` (consumed later by FR-44 / Epic 8) and `known-bots.json`
//! (consumed later by Story 6.2 discovery) are parsed and validated here so a
//! malformed seed is caught now; only the risk tiers are projected into the catalog.

use std::collections::HashSet;
use std::sync::OnceLock;

use serde::Deserialize;

use crate::error::BridgeError;
use crate::vm::{BadgeStyle, RiskTier};

/// The schema `version` each embedded data file must declare for this build.
const RISK_TIERS_VERSION: u32 = 1;
/// The schema `version` for `coupling-caveats.json`.
const COUPLING_CAVEATS_VERSION: u32 = 1;
/// The schema `version` for `known-bots.json`.
const KNOWN_BOTS_VERSION: u32 = 1;
/// The schema `version` for `provisioning.json`.
const PROVISIONING_VERSION: u32 = 1;

/// The raw `risk-tiers.json` document.
#[derive(Debug, Deserialize)]
pub struct RiskTiersDoc {
    /// The data-file schema version (`1`); checked by [`validate_risk_tiers`].
    pub version: u32,
    /// Every risk tier, in file order (surfaced and out-of-scope).
    pub tiers: Vec<TierEntry>,
}

/// One tier row in `risk-tiers.json`.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TierEntry {
    /// The stable tier id (`"low"`, `"maintenance"`, `"volatile"`, `"conditional"`,
    /// `"out-of-scope"`).
    pub id: String,
    /// The tier's display label.
    pub label: String,
    /// The badge style for this tier.
    pub badge: BadgeStyle,
    /// Whether connecting a network in this tier requires an acknowledgment gate.
    pub requires_ack: bool,
    /// The acknowledgment / guidance copy for this tier.
    pub acknowledgment: String,
    /// Whether this tier is surfaced as connectable cards (out-of-scope is not).
    pub surfaced: bool,
    /// The networks in this tier.
    pub networks: Vec<NetworkEntry>,
}

/// One network row inside a tier.
#[derive(Debug, Deserialize)]
pub struct NetworkEntry {
    /// The stable network id (e.g. `"whatsapp"`).
    pub id: String,
    /// The network's display name (e.g. `"WhatsApp"`).
    pub name: String,
    /// The glyph initials for the card avatar (e.g. `"WA"`).
    pub glyph: String,
}

/// The raw `coupling-caveats.json` document (consumed later by FR-44 / Epic 8).
#[derive(Debug, Deserialize)]
pub struct CouplingCaveatsDoc {
    /// The data-file schema version (`1`); checked by [`validate_coupling_caveats`].
    pub version: u32,
    /// Every coupling caveat.
    pub caveats: Vec<CouplingCaveat>,
}

/// One coupling caveat — a side effect that connecting a network couples in.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CouplingCaveat {
    /// The network id this caveat applies to.
    pub network_id: String,
    /// The human-readable caveat text.
    pub text: String,
    /// A machine tag naming the coupled surface (e.g. `"read-receipts"`).
    pub applies_to: String,
}

/// The raw `known-bots.json` document (consumed later by Story 6.2 discovery).
#[derive(Debug, Deserialize)]
pub struct KnownBotsDoc {
    /// The data-file schema version (`1`); checked by [`validate_known_bots`].
    pub version: u32,
    /// Every known-bot entry.
    pub bots: Vec<KnownBotEntry>,
}

/// One known-bot entry — a network's candidate bot localparts.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct KnownBotEntry {
    /// The network id these localparts belong to.
    pub network_id: String,
    /// The candidate bot localparts (at least one).
    pub localparts: Vec<String>,
}

/// The raw `provisioning.json` document (consumed by Story 6.3 native login).
///
/// An ordered list of base-URL candidate templates with a `{server}` placeholder,
/// probed in order by the [`Provisioning`](crate::bridges::transport::provisioning)
/// transport until one authenticates the provisioning `…/v3/login/flows` endpoint
/// (AD-16: base-URL resolution is a data-driven probe, an implementation detail
/// inside the transport). Never hardcoded in the transport code.
#[derive(Debug, Deserialize)]
pub struct ProvisioningDoc {
    /// The data-file schema version (`1`); checked by [`validate_provisioning`].
    pub version: u32,
    /// The ordered base-URL candidate templates (each with a `{server}` token).
    pub candidates: Vec<String>,
}

/// The compiled-in `risk-tiers.json` bytes.
const RISK_TIERS_JSON: &str = include_str!("../../data/risk-tiers.json");
/// The compiled-in `coupling-caveats.json` bytes.
const COUPLING_CAVEATS_JSON: &str = include_str!("../../data/coupling-caveats.json");
/// The compiled-in `known-bots.json` bytes.
const KNOWN_BOTS_JSON: &str = include_str!("../../data/known-bots.json");
/// The compiled-in `provisioning.json` bytes.
const PROVISIONING_JSON: &str = include_str!("../../data/provisioning.json");

/// Process-wide cache for the parsed-and-validated risk tiers.
static RISK_TIERS: OnceLock<Result<RiskTiersDoc, BridgeError>> = OnceLock::new();
/// Process-wide cache for the parsed-and-validated coupling caveats.
static COUPLING_CAVEATS: OnceLock<Result<CouplingCaveatsDoc, BridgeError>> = OnceLock::new();
/// Process-wide cache for the parsed-and-validated known-bot registry.
static KNOWN_BOTS: OnceLock<Result<KnownBotsDoc, BridgeError>> = OnceLock::new();
/// Process-wide cache for the parsed-and-validated provisioning candidates.
static PROVISIONING: OnceLock<Result<ProvisioningDoc, BridgeError>> = OnceLock::new();

/// Convert a cached `&Result<T, BridgeError>` into a `Result<&T, BridgeError>` so
/// callers get the shared parsed doc on success and a cloned error on failure.
fn as_ref_result<T>(cached: &Result<T, BridgeError>) -> Result<&T, BridgeError> {
    cached.as_ref().map_err(Clone::clone)
}

/// Parse + validate the risk tiers once and return the cached document.
pub fn risk_tiers() -> Result<&'static RiskTiersDoc, BridgeError> {
    let cached = RISK_TIERS.get_or_init(|| {
        let doc: RiskTiersDoc = serde_json::from_str(RISK_TIERS_JSON)
            .map_err(|e| BridgeError::Data(format!("risk-tiers.json failed to parse: {e}")))?;
        validate_risk_tiers(&doc)?;
        Ok(doc)
    });
    as_ref_result(cached)
}

/// Parse + validate the coupling caveats once and return the cached document.
pub fn coupling_caveats() -> Result<&'static CouplingCaveatsDoc, BridgeError> {
    let cached = COUPLING_CAVEATS.get_or_init(|| {
        let doc: CouplingCaveatsDoc = serde_json::from_str(COUPLING_CAVEATS_JSON).map_err(|e| {
            BridgeError::Data(format!("coupling-caveats.json failed to parse: {e}"))
        })?;
        validate_coupling_caveats(&doc)?;
        Ok(doc)
    });
    as_ref_result(cached)
}

/// Parse + validate the known-bot registry once and return the cached document.
pub fn known_bots() -> Result<&'static KnownBotsDoc, BridgeError> {
    let cached = KNOWN_BOTS.get_or_init(|| {
        let doc: KnownBotsDoc = serde_json::from_str(KNOWN_BOTS_JSON)
            .map_err(|e| BridgeError::Data(format!("known-bots.json failed to parse: {e}")))?;
        validate_known_bots(&doc)?;
        Ok(doc)
    });
    as_ref_result(cached)
}

/// Parse + validate the provisioning candidates once and return the cached doc.
pub fn provisioning() -> Result<&'static ProvisioningDoc, BridgeError> {
    let cached = PROVISIONING.get_or_init(|| {
        let doc: ProvisioningDoc = serde_json::from_str(PROVISIONING_JSON)
            .map_err(|e| BridgeError::Data(format!("provisioning.json failed to parse: {e}")))?;
        validate_provisioning(&doc)?;
        Ok(doc)
    });
    as_ref_result(cached)
}

/// Validate the provisioning candidates: the schema version must match and every
/// candidate must be a non-empty template carrying the `{server}` placeholder (a
/// candidate that can't substitute the server would probe a fixed URL, which is a
/// data authoring error the [`Provisioning`](crate::bridges::transport::provisioning)
/// probe relies on being caught here).
fn validate_provisioning(doc: &ProvisioningDoc) -> Result<(), BridgeError> {
    if doc.version != PROVISIONING_VERSION {
        return Err(BridgeError::Data(format!(
            "provisioning.json unsupported version {} (expected {PROVISIONING_VERSION})",
            doc.version
        )));
    }
    if doc.candidates.is_empty() {
        return Err(BridgeError::Data(
            "provisioning.json has no candidates".to_owned(),
        ));
    }
    for candidate in &doc.candidates {
        if candidate.trim().is_empty() || !candidate.contains("{server}") {
            return Err(BridgeError::Data(format!(
                "provisioning.json candidate {candidate:?} is empty or missing the {{server}} placeholder"
            )));
        }
    }
    Ok(())
}

/// Validate the risk-tier document. Enforces the invariants the Bridges surface
/// and later stories rely on:
/// - the schema `version` is the one this build understands (`1`);
/// - every tier has a non-empty id/label, and every network a non-empty
///   id/name/glyph;
/// - a tier that requires an acknowledgment carries non-empty acknowledgment copy;
/// - `surfaced` matches exactly the known [`RiskTier`] tiers — a surfaced tier must
///   map to an enum variant and the out-of-scope (unmapped) tier must not be
///   surfaced, so mis-flagging `surfaced` (e.g. hiding the safety-critical volatile
///   tier) fails loudly here instead of silently dropping risk copy;
/// - a surfaced tier is non-empty; and
/// - no network id is repeated across surfaced tiers (the frontend keys a card per
///   Network × Account, so a duplicate id would collide).
fn validate_risk_tiers(doc: &RiskTiersDoc) -> Result<(), BridgeError> {
    if doc.version != RISK_TIERS_VERSION {
        return Err(BridgeError::Data(format!(
            "risk-tiers.json unsupported version {} (expected {RISK_TIERS_VERSION})",
            doc.version
        )));
    }
    if doc.tiers.is_empty() {
        return Err(BridgeError::Data("risk-tiers.json has no tiers".to_owned()));
    }
    let mut surfaced_ids: HashSet<&str> = HashSet::new();
    for tier in &doc.tiers {
        if tier.id.trim().is_empty() || tier.label.trim().is_empty() {
            return Err(BridgeError::Data(format!(
                "risk-tiers.json tier {:?} has an empty id or label",
                tier.id
            )));
        }
        // `surfaced` must agree with the enum mapping: exactly the known tiers are
        // surfaced, and the unmapped out-of-scope tier is not.
        if tier.surfaced != tier_from_id(&tier.id).is_some() {
            return Err(BridgeError::Data(format!(
                "risk-tiers.json tier {:?} has surfaced={} inconsistent with its known-tier mapping",
                tier.id, tier.surfaced
            )));
        }
        if tier.requires_ack && tier.acknowledgment.trim().is_empty() {
            return Err(BridgeError::Data(format!(
                "risk-tiers.json tier {:?} requires ack but has empty acknowledgment copy",
                tier.id
            )));
        }
        if tier.surfaced && tier.networks.is_empty() {
            return Err(BridgeError::Data(format!(
                "risk-tiers.json surfaced tier {:?} has no networks",
                tier.id
            )));
        }
        for network in &tier.networks {
            if network.id.trim().is_empty()
                || network.name.trim().is_empty()
                || network.glyph.trim().is_empty()
            {
                return Err(BridgeError::Data(format!(
                    "risk-tiers.json tier {:?} has a network with an empty id/name/glyph",
                    tier.id
                )));
            }
            if tier.surfaced && !surfaced_ids.insert(network.id.as_str()) {
                return Err(BridgeError::Data(format!(
                    "risk-tiers.json has a duplicate surfaced network id {:?}",
                    network.id
                )));
            }
        }
    }
    Ok(())
}

/// Validate the coupling caveats: every caveat must carry a non-empty network id,
/// text, and `appliesTo` tag.
fn validate_coupling_caveats(doc: &CouplingCaveatsDoc) -> Result<(), BridgeError> {
    if doc.version != COUPLING_CAVEATS_VERSION {
        return Err(BridgeError::Data(format!(
            "coupling-caveats.json unsupported version {} (expected {COUPLING_CAVEATS_VERSION})",
            doc.version
        )));
    }
    for caveat in &doc.caveats {
        if caveat.network_id.trim().is_empty()
            || caveat.text.trim().is_empty()
            || caveat.applies_to.trim().is_empty()
        {
            return Err(BridgeError::Data(
                "coupling-caveats.json has a caveat with an empty field".to_owned(),
            ));
        }
    }
    Ok(())
}

/// Validate the known-bot registry: every entry must carry a non-empty network id
/// and at least one non-empty localpart, and no network id may repeat (Story 6.2
/// joins the registry to the catalog by `networkId`, so a duplicate entry would
/// make that join ambiguous).
fn validate_known_bots(doc: &KnownBotsDoc) -> Result<(), BridgeError> {
    if doc.version != KNOWN_BOTS_VERSION {
        return Err(BridgeError::Data(format!(
            "known-bots.json unsupported version {} (expected {KNOWN_BOTS_VERSION})",
            doc.version
        )));
    }
    let mut network_ids: HashSet<&str> = HashSet::new();
    for bot in &doc.bots {
        if bot.network_id.trim().is_empty() {
            return Err(BridgeError::Data(
                "known-bots.json has an entry with an empty networkId".to_owned(),
            ));
        }
        if bot.localparts.is_empty() || bot.localparts.iter().any(|lp| lp.trim().is_empty()) {
            return Err(BridgeError::Data(format!(
                "known-bots.json entry {:?} has no valid localparts",
                bot.network_id
            )));
        }
        if !network_ids.insert(bot.network_id.as_str()) {
            return Err(BridgeError::Data(format!(
                "known-bots.json has a duplicate networkId {:?}",
                bot.network_id
            )));
        }
    }
    Ok(())
}

/// Map a data-file tier id to its [`RiskTier`] enum, or `None` for the
/// out-of-scope tier (which has no surfaced variant).
pub fn tier_from_id(id: &str) -> Option<RiskTier> {
    match id {
        "low" => Some(RiskTier::Low),
        "maintenance" => Some(RiskTier::Maintenance),
        "volatile" => Some(RiskTier::Volatile),
        "conditional" => Some(RiskTier::Conditional),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn all_data_files_parse_and_validate() {
        risk_tiers().expect("risk tiers parse");
        coupling_caveats().expect("coupling caveats parse");
        known_bots().expect("known bots parse");
        provisioning().expect("provisioning parse");
    }

    #[test]
    fn provisioning_candidates_all_carry_server_placeholder() {
        let doc = provisioning().expect("provisioning parse");
        assert!(!doc.candidates.is_empty(), "must have candidates");
        for candidate in &doc.candidates {
            assert!(
                candidate.contains("{server}"),
                "candidate {candidate:?} must carry the {{server}} placeholder"
            );
        }
    }

    #[test]
    fn rejects_unsupported_provisioning_version() {
        let doc = ProvisioningDoc {
            version: PROVISIONING_VERSION + 1,
            candidates: vec!["https://{server}/_matrix/provision".to_owned()],
        };
        assert!(err_msg(validate_provisioning(&doc)).contains("unsupported version"));
    }

    #[test]
    fn rejects_provisioning_candidate_without_placeholder() {
        let doc = ProvisioningDoc {
            version: PROVISIONING_VERSION,
            candidates: vec!["https://fixed.example.org/_matrix/provision".to_owned()],
        };
        assert!(err_msg(validate_provisioning(&doc)).contains("placeholder"));
    }

    #[test]
    fn rejects_empty_provisioning_candidates() {
        let doc = ProvisioningDoc {
            version: PROVISIONING_VERSION,
            candidates: vec![],
        };
        assert!(err_msg(validate_provisioning(&doc)).contains("no candidates"));
    }

    #[test]
    fn volatile_tier_requires_ack_with_non_empty_copy_low_does_not() {
        let doc = risk_tiers().expect("risk tiers parse");
        let volatile = doc
            .tiers
            .iter()
            .find(|t| t.id == "volatile")
            .expect("volatile tier present");
        assert!(volatile.requires_ack, "volatile must require ack");
        assert!(
            !volatile.acknowledgment.trim().is_empty(),
            "volatile ack copy must be non-empty"
        );

        let low = doc
            .tiers
            .iter()
            .find(|t| t.id == "low")
            .expect("low tier present");
        assert!(!low.requires_ack, "low must not require ack");
    }

    #[test]
    fn out_of_scope_tier_is_present_but_not_surfaced() {
        let doc = risk_tiers().expect("risk tiers parse");
        let oos = doc
            .tiers
            .iter()
            .find(|t| t.id == "out-of-scope")
            .expect("out-of-scope tier present");
        assert!(!oos.surfaced, "out-of-scope must not be surfaced");
    }

    #[test]
    fn every_network_has_non_empty_name_and_glyph() {
        let doc = risk_tiers().expect("risk tiers parse");
        for tier in &doc.tiers {
            for network in &tier.networks {
                assert!(!network.name.trim().is_empty(), "empty name in {}", tier.id);
                assert!(
                    !network.glyph.trim().is_empty(),
                    "empty glyph in {}",
                    tier.id
                );
            }
        }
    }

    #[test]
    fn every_known_bot_entry_has_at_least_one_localpart() {
        let doc = known_bots().expect("known bots parse");
        assert!(!doc.bots.is_empty(), "known bots must not be empty");
        for bot in &doc.bots {
            assert!(
                !bot.localparts.is_empty(),
                "network {} must have >=1 localpart",
                bot.network_id
            );
        }
    }

    #[test]
    fn coupling_caveat_seed_covers_whatsapp_read_receipts() {
        let doc = coupling_caveats().expect("coupling caveats parse");
        assert!(
            doc.caveats
                .iter()
                .any(|c| c.network_id == "whatsapp" && c.applies_to == "read-receipts"),
            "WhatsApp read-receipt coupling seed must be present"
        );
    }

    #[test]
    fn malformed_json_surfaces_a_bridge_error_not_a_panic() {
        // Prove the parse path is fallible (the production path parses the embedded
        // file through the same `serde_json::from_str` → `BridgeError::Data`).
        let bad = "{ not valid json";
        let parsed: Result<RiskTiersDoc, _> = serde_json::from_str(bad);
        assert!(parsed.is_err(), "malformed JSON must fail to parse");
        let err = BridgeError::Data("risk-tiers.json failed to parse: x".to_owned());
        assert!(matches!(err, BridgeError::Data(_)));
    }

    // --- Negative validation tests -------------------------------------------
    //
    // The embedded data is correct, so the happy-path tests above never drive a
    // validator to `Err`. These build deliberately *bad* in-memory docs and prove
    // every rejection branch fires — the safety-critical invariants (surfaced ⇔
    // known tier, ack-required-but-empty, duplicate ids, version) must fail loudly,
    // not silently drop risk copy.

    fn network(id: &str) -> NetworkEntry {
        NetworkEntry {
            id: id.to_owned(),
            name: format!("{id} Name"),
            glyph: "XX".to_owned(),
        }
    }

    /// A valid single-network `low` tier (surfaced, no ack) as a baseline to mutate.
    fn low_tier() -> TierEntry {
        TierEntry {
            id: "low".to_owned(),
            label: "Low".to_owned(),
            badge: BadgeStyle::Secondary,
            requires_ack: false,
            acknowledgment: String::new(),
            surfaced: true,
            networks: vec![network("matrix")],
        }
    }

    fn err_msg(result: Result<(), BridgeError>) -> String {
        match result {
            Err(BridgeError::Data(msg)) => msg,
            Err(other) => panic!("expected a Data validation error, got: {other}"),
            Ok(()) => panic!("expected validation to fail, but it passed"),
        }
    }

    #[test]
    fn rejects_unsupported_risk_tiers_version() {
        let doc = RiskTiersDoc {
            version: RISK_TIERS_VERSION + 1,
            tiers: vec![low_tier()],
        };
        assert!(err_msg(validate_risk_tiers(&doc)).contains("unsupported version"));
    }

    #[test]
    fn rejects_empty_risk_tiers() {
        let doc = RiskTiersDoc {
            version: RISK_TIERS_VERSION,
            tiers: vec![],
        };
        assert!(err_msg(validate_risk_tiers(&doc)).contains("no tiers"));
    }

    #[test]
    fn rejects_empty_tier_id_or_label() {
        let mut tier = low_tier();
        tier.label = "   ".to_owned();
        let doc = RiskTiersDoc {
            version: RISK_TIERS_VERSION,
            tiers: vec![tier],
        };
        assert!(err_msg(validate_risk_tiers(&doc)).contains("empty id or label"));
    }

    #[test]
    fn rejects_surfaced_flag_inconsistent_with_known_tier_mapping() {
        // A surfaced tier whose id is not a known `RiskTier` (would silently drop
        // its risk copy from the catalog) must fail loudly.
        let mut tier = low_tier();
        tier.id = "made-up".to_owned();
        tier.surfaced = true;
        let doc = RiskTiersDoc {
            version: RISK_TIERS_VERSION,
            tiers: vec![tier],
        };
        assert!(
            err_msg(validate_risk_tiers(&doc)).contains("inconsistent with its known-tier mapping")
        );

        // The mirror case: a *known* tier flagged `surfaced: false` (e.g. hiding the
        // safety-critical volatile tier) is equally rejected.
        let mut hidden = low_tier();
        hidden.surfaced = false;
        let doc = RiskTiersDoc {
            version: RISK_TIERS_VERSION,
            tiers: vec![hidden],
        };
        assert!(
            err_msg(validate_risk_tiers(&doc)).contains("inconsistent with its known-tier mapping")
        );
    }

    #[test]
    fn rejects_requires_ack_with_empty_acknowledgment() {
        let mut tier = low_tier();
        tier.id = "volatile".to_owned();
        tier.requires_ack = true;
        tier.acknowledgment = "   ".to_owned();
        let doc = RiskTiersDoc {
            version: RISK_TIERS_VERSION,
            tiers: vec![tier],
        };
        assert!(err_msg(validate_risk_tiers(&doc)).contains("requires ack but has empty"));
    }

    #[test]
    fn rejects_surfaced_tier_with_no_networks() {
        let mut tier = low_tier();
        tier.networks = vec![];
        let doc = RiskTiersDoc {
            version: RISK_TIERS_VERSION,
            tiers: vec![tier],
        };
        assert!(err_msg(validate_risk_tiers(&doc)).contains("has no networks"));
    }

    #[test]
    fn rejects_network_with_empty_field() {
        let mut tier = low_tier();
        tier.networks = vec![NetworkEntry {
            id: "matrix".to_owned(),
            name: "Matrix".to_owned(),
            glyph: "   ".to_owned(),
        }];
        let doc = RiskTiersDoc {
            version: RISK_TIERS_VERSION,
            tiers: vec![tier],
        };
        assert!(err_msg(validate_risk_tiers(&doc)).contains("empty id/name/glyph"));
    }

    #[test]
    fn rejects_duplicate_surfaced_network_id() {
        // Same network id across two surfaced tiers would collide the per-Network
        // card key on the frontend.
        let low = low_tier();
        let mut volatile = low_tier();
        volatile.id = "volatile".to_owned();
        volatile.label = "Volatile".to_owned();
        volatile.requires_ack = true;
        volatile.acknowledgment = "risky".to_owned();
        volatile.networks = vec![network("matrix")]; // duplicate id
        let doc = RiskTiersDoc {
            version: RISK_TIERS_VERSION,
            tiers: vec![low, volatile],
        };
        assert!(err_msg(validate_risk_tiers(&doc)).contains("duplicate surfaced network id"));
    }

    #[test]
    fn accepts_unknown_tier_id_only_when_not_surfaced() {
        // The out-of-scope tier (unknown id, `surfaced: false`) is the one legal
        // unmapped tier; validation accepts it.
        let doc = RiskTiersDoc {
            version: RISK_TIERS_VERSION,
            tiers: vec![
                low_tier(),
                TierEntry {
                    id: "out-of-scope".to_owned(),
                    label: "Out of scope".to_owned(),
                    badge: BadgeStyle::Outline,
                    requires_ack: false,
                    acknowledgment: String::new(),
                    surfaced: false,
                    networks: vec![network("wechat")],
                },
            ],
        };
        assert!(validate_risk_tiers(&doc).is_ok());
    }

    #[test]
    fn rejects_unsupported_coupling_caveats_version() {
        let doc = CouplingCaveatsDoc {
            version: COUPLING_CAVEATS_VERSION + 1,
            caveats: vec![],
        };
        assert!(err_msg(validate_coupling_caveats(&doc)).contains("unsupported version"));
    }

    #[test]
    fn rejects_coupling_caveat_with_empty_field() {
        let doc = CouplingCaveatsDoc {
            version: COUPLING_CAVEATS_VERSION,
            caveats: vec![CouplingCaveat {
                network_id: "whatsapp".to_owned(),
                text: "   ".to_owned(),
                applies_to: "read-receipts".to_owned(),
            }],
        };
        assert!(err_msg(validate_coupling_caveats(&doc)).contains("empty field"));
    }

    #[test]
    fn rejects_unsupported_known_bots_version() {
        let doc = KnownBotsDoc {
            version: KNOWN_BOTS_VERSION + 1,
            bots: vec![],
        };
        assert!(err_msg(validate_known_bots(&doc)).contains("unsupported version"));
    }

    #[test]
    fn rejects_known_bot_with_empty_network_id() {
        let doc = KnownBotsDoc {
            version: KNOWN_BOTS_VERSION,
            bots: vec![KnownBotEntry {
                network_id: "  ".to_owned(),
                localparts: vec!["whatsappbot".to_owned()],
            }],
        };
        assert!(err_msg(validate_known_bots(&doc)).contains("empty networkId"));
    }

    #[test]
    fn rejects_known_bot_with_no_valid_localparts() {
        let doc = KnownBotsDoc {
            version: KNOWN_BOTS_VERSION,
            bots: vec![KnownBotEntry {
                network_id: "whatsapp".to_owned(),
                localparts: vec!["   ".to_owned()],
            }],
        };
        assert!(err_msg(validate_known_bots(&doc)).contains("no valid localparts"));
    }

    #[test]
    fn rejects_duplicate_known_bot_network_id() {
        let doc = KnownBotsDoc {
            version: KNOWN_BOTS_VERSION,
            bots: vec![
                KnownBotEntry {
                    network_id: "whatsapp".to_owned(),
                    localparts: vec!["whatsappbot".to_owned()],
                },
                KnownBotEntry {
                    network_id: "whatsapp".to_owned(),
                    localparts: vec!["wa-bot".to_owned()],
                },
            ],
        };
        assert!(err_msg(validate_known_bots(&doc)).contains("duplicate networkId"));
    }
}
