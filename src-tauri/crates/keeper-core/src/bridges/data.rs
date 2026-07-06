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
/// The schema `version` for `bot-commands.json`.
const BOT_COMMANDS_VERSION: u32 = 1;
/// The schema `version` for `health-signals.json`.
const HEALTH_SIGNALS_VERSION: u32 = 1;
/// The schema `version` for `resolve-support.json`.
const RESOLVE_SUPPORT_VERSION: u32 = 1;
/// The schema `version` for `bbctl.json`.
const BBCTL_VERSION: u32 = 1;

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

/// The raw `bot-commands.json` document (consumed by Story 6.4's `BotDriver`).
///
/// The data-driven Bridge Bot login protocol: a `default` [`BotProtocol`]
/// (login/cancel command strings) every bridgev2 bot works with, plus optional
/// per-network overrides keyed by `networkId`. The command knowledge lives in
/// versioned data (never hardcoded in the transport), so tuning a bot's grammar
/// needs no code change. The schema MAY later carry list-logins / logout / relay
/// command strings as *data* (no code) when Stories 6.5/6.6 add those trait
/// methods — this build reads only login/cancel.
#[derive(Debug, Deserialize)]
pub struct BotCommandsDoc {
    /// The data-file schema version (`1`); checked by [`validate_bot_commands`].
    pub version: u32,
    /// The fallback command protocol every bridgev2 bot works with.
    pub default: BotProtocol,
    /// Optional per-network command overrides (empty when every bot uses the
    /// default). Absent in the JSON → an empty list.
    #[serde(default)]
    pub overrides: Vec<BotProtocolOverride>,
}

/// One per-network override row in `bot-commands.json`: the `networkId` it
/// applies to plus its command [`BotProtocol`].
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BotProtocolOverride {
    /// The network id these commands apply to.
    pub network_id: String,
    /// The command protocol for this network.
    #[serde(flatten)]
    pub protocol: BotProtocol,
}

/// The Bridge Bot command strings for one network (or the default). The commands
/// are sent verbatim as bot chat messages by the `BotDriver`; a leading `!` (the
/// mautrix bot prefix) is NOT assumed here — the data carries the exact string.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BotProtocol {
    /// The command that starts a login (e.g. `login`).
    pub login_command: String,
    /// The command that cancels an in-progress login (e.g. `cancel`).
    pub cancel_command: String,
}

/// The raw `health-signals.json` document (consumed by Story 6.5's health monitor).
///
/// The data-driven bridge-session health grammar: a `default` [`BridgeHealthGrammar`]
/// (disconnected/degraded/healthy markers, the liveness ping command + cadence) every
/// bridgev2 management-room bot works with, plus optional per-network overrides keyed
/// by `networkId`. Grammar knowledge lives in versioned data (never hardcoded in the
/// health monitor), so tuning a bot's health markers or the tick cadence needs no code
/// change.
#[derive(Debug, Deserialize)]
pub struct HealthSignalsDoc {
    /// The data-file schema version (`1`); checked by [`validate_health_signals`].
    pub version: u32,
    /// The fallback grammar every bridgev2 management-room bot works with.
    pub default: BridgeHealthGrammar,
    /// Optional per-network grammar overrides (empty when every bot uses the
    /// default). Absent in the JSON → an empty list.
    #[serde(default)]
    pub overrides: Vec<BridgeHealthGrammarOverride>,
}

/// One per-network override row in `health-signals.json`: the `networkId` it applies
/// to plus its health [`BridgeHealthGrammar`].
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BridgeHealthGrammarOverride {
    /// The network id this grammar applies to.
    pub network_id: String,
    /// The health grammar for this network.
    #[serde(flatten)]
    pub grammar: BridgeHealthGrammar,
}

/// The bridge-session health grammar for one network (or the default). The markers
/// are matched case-insensitively as substrings against a management-room notice's
/// body (never regex — the data carries plain phrases). `ping_command` is sent
/// verbatim as a bot chat message by the liveness tick when `enable_ping` is set;
/// `tick_interval_secs` bounds the fallback cadence (≤ 60 s to meet the detect-in-60s
/// target).
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BridgeHealthGrammar {
    /// Phrases that flip a session to `Disconnected` (e.g. "you have been logged out").
    pub disconnected_markers: Vec<String>,
    /// Phrases that flip a session to `Degraded` (e.g. "reconnecting").
    pub degraded_markers: Vec<String>,
    /// Phrases that recover a session to `Healthy` (e.g. "connected").
    pub healthy_markers: Vec<String>,
    /// The command the liveness tick sends to ping the bot (e.g. `ping`).
    pub ping_command: String,
    /// The bounded liveness-tick cadence in seconds (must be ≥ 1 and ≤ 60).
    #[serde(default = "default_tick_interval_secs")]
    pub tick_interval_secs: u64,
    /// Whether the liveness tick actively pings the bot (vs. only re-checking the
    /// last observed state). Off by default — a passive re-check never spams the
    /// management room.
    #[serde(default)]
    pub enable_ping: bool,
}

/// The default liveness-tick cadence (seconds) when a grammar omits it — the 60 s
/// detect-within budget.
fn default_tick_interval_secs() -> u64 {
    60
}

/// The raw `resolve-support.json` document (consumed by Story 6.6's new-chat surface).
///
/// The data-driven new-chat resolve-capability grammar: a `default`
/// [`ResolveSupport`] (whether the bridge can resolve an identifier, the input hint,
/// and the placeholder) every bridgev2 network works with, plus optional per-network
/// overrides keyed by `networkId`. A network marked `supported: false` disables the
/// identifier field upfront (before any I/O). Capability knowledge lives in versioned
/// data (never hardcoded in Rust or TS), so tuning a network's hint or marking it
/// unsupported needs no code change.
#[derive(Debug, Deserialize)]
pub struct ResolveSupportDoc {
    /// The data-file schema version (`1`); checked by [`validate_resolve_support`].
    pub version: u32,
    /// The fallback resolve capability every bridgev2 network works with.
    pub default: ResolveSupport,
    /// Optional per-network capability overrides (empty when every network uses the
    /// default). Absent in the JSON → an empty list.
    #[serde(default)]
    pub overrides: Vec<ResolveSupportOverride>,
}

/// One per-network override row in `resolve-support.json`: the `networkId` it applies
/// to plus its [`ResolveSupport`].
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ResolveSupportOverride {
    /// The network id this capability applies to.
    pub network_id: String,
    /// The resolve capability for this network.
    #[serde(flatten)]
    pub support: ResolveSupport,
}

/// The new-chat resolve capability for one network (or the default). `supported`
/// gates the identifier field before any network I/O; `identifier_hint` and
/// `placeholder` drive the input's label/placeholder copy (data, never hardcoded).
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ResolveSupport {
    /// Whether starting a new chat by resolving an identifier is supported here.
    pub supported: bool,
    /// The identifier-field hint copy (e.g. "Phone number, username, or Matrix ID").
    pub identifier_hint: String,
    /// The identifier-field placeholder copy (e.g. "+1 555 123 4567 or @username").
    #[serde(default)]
    pub placeholder: String,
}

/// The raw `bbctl.json` document (consumed by Story 6.7's run-your-own-bridge surface).
///
/// The data-driven `bbctl` self-host capability: a versioned document carrying the
/// guided-`install` steps + docs URL (rendered when the `bbctl` sidecar can't be
/// resolved) and the `networks` that can be self-hosted (each mapping a keeper
/// `networkId` to its `bbctlName` and whether it is `supported`). A network absent
/// from the supported set is never offered. Capability knowledge lives in versioned
/// data (never hardcoded in Rust or TS), so tuning the supported set or the install
/// copy needs no code change — loaded/validated/cached exactly like the other data
/// files.
#[derive(Debug, Deserialize)]
pub struct BbctlDoc {
    /// The data-file schema version (`1`); checked by [`validate_bbctl`].
    pub version: u32,
    /// The guided-install instructions rendered when the sidecar is absent.
    pub install: BbctlInstall,
    /// The self-hostable networks (each mapping a keeper network id to its
    /// `bbctl` name + supported flag).
    pub networks: Vec<BbctlNetwork>,
}

/// The guided-install block of `bbctl.json`: ordered human `steps` and a `docsUrl`
/// pointing at the Beeper self-host documentation.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BbctlInstall {
    /// The ordered install steps (at least one; may repeat prose).
    pub steps: Vec<String>,
    /// The Beeper self-host docs URL.
    pub docs_url: String,
}

/// One self-hostable network row in `bbctl.json`: the keeper `networkId`, its
/// `bbctlName` (the name `bbctl register`/`run` uses), and whether keeper offers
/// running it (`supported`).
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BbctlNetwork {
    /// The keeper network id (e.g. `"signal"`), joined to the 6.1 catalog for
    /// display name/glyph.
    pub network_id: String,
    /// The name `bbctl` uses for this self-hosted bridge (e.g. `"sh-signal"`).
    pub bbctl_name: String,
    /// Whether keeper offers running this network as a self-hosted bridge.
    pub supported: bool,
}

/// The compiled-in `risk-tiers.json` bytes.
const RISK_TIERS_JSON: &str = include_str!("../../data/risk-tiers.json");
/// The compiled-in `coupling-caveats.json` bytes.
const COUPLING_CAVEATS_JSON: &str = include_str!("../../data/coupling-caveats.json");
/// The compiled-in `known-bots.json` bytes.
const KNOWN_BOTS_JSON: &str = include_str!("../../data/known-bots.json");
/// The compiled-in `provisioning.json` bytes.
const PROVISIONING_JSON: &str = include_str!("../../data/provisioning.json");
/// The compiled-in `bot-commands.json` bytes.
const BOT_COMMANDS_JSON: &str = include_str!("../../data/bot-commands.json");
/// The compiled-in `health-signals.json` bytes.
const HEALTH_SIGNALS_JSON: &str = include_str!("../../data/health-signals.json");
/// The compiled-in `resolve-support.json` bytes.
const RESOLVE_SUPPORT_JSON: &str = include_str!("../../data/resolve-support.json");
/// The compiled-in `bbctl.json` bytes.
const BBCTL_JSON: &str = include_str!("../../data/bbctl.json");

/// Process-wide cache for the parsed-and-validated risk tiers.
static RISK_TIERS: OnceLock<Result<RiskTiersDoc, BridgeError>> = OnceLock::new();
/// Process-wide cache for the parsed-and-validated coupling caveats.
static COUPLING_CAVEATS: OnceLock<Result<CouplingCaveatsDoc, BridgeError>> = OnceLock::new();
/// Process-wide cache for the parsed-and-validated known-bot registry.
static KNOWN_BOTS: OnceLock<Result<KnownBotsDoc, BridgeError>> = OnceLock::new();
/// Process-wide cache for the parsed-and-validated provisioning candidates.
static PROVISIONING: OnceLock<Result<ProvisioningDoc, BridgeError>> = OnceLock::new();
/// Process-wide cache for the parsed-and-validated bot-command protocol.
static BOT_COMMANDS: OnceLock<Result<BotCommandsDoc, BridgeError>> = OnceLock::new();
/// Process-wide cache for the parsed-and-validated health-signal grammar.
static HEALTH_SIGNALS: OnceLock<Result<HealthSignalsDoc, BridgeError>> = OnceLock::new();
/// Process-wide cache for the parsed-and-validated resolve-support capability.
static RESOLVE_SUPPORT: OnceLock<Result<ResolveSupportDoc, BridgeError>> = OnceLock::new();
/// Process-wide cache for the parsed-and-validated bbctl self-host capability.
static BBCTL: OnceLock<Result<BbctlDoc, BridgeError>> = OnceLock::new();

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

/// Parse + validate the bot-command protocol once and return the cached doc.
pub fn bot_commands() -> Result<&'static BotCommandsDoc, BridgeError> {
    let cached = BOT_COMMANDS.get_or_init(|| {
        let doc: BotCommandsDoc = serde_json::from_str(BOT_COMMANDS_JSON)
            .map_err(|e| BridgeError::Data(format!("bot-commands.json failed to parse: {e}")))?;
        validate_bot_commands(&doc)?;
        Ok(doc)
    });
    as_ref_result(cached)
}

/// Parse + validate the health-signal grammar once and return the cached doc.
pub fn health_signals() -> Result<&'static HealthSignalsDoc, BridgeError> {
    let cached = HEALTH_SIGNALS.get_or_init(|| {
        let doc: HealthSignalsDoc = serde_json::from_str(HEALTH_SIGNALS_JSON)
            .map_err(|e| BridgeError::Data(format!("health-signals.json failed to parse: {e}")))?;
        validate_health_signals(&doc)?;
        Ok(doc)
    });
    as_ref_result(cached)
}

/// Parse + validate the resolve-support capability once and return the cached doc.
pub fn resolve_support() -> Result<&'static ResolveSupportDoc, BridgeError> {
    let cached = RESOLVE_SUPPORT.get_or_init(|| {
        let doc: ResolveSupportDoc = serde_json::from_str(RESOLVE_SUPPORT_JSON)
            .map_err(|e| BridgeError::Data(format!("resolve-support.json failed to parse: {e}")))?;
        validate_resolve_support(&doc)?;
        Ok(doc)
    });
    as_ref_result(cached)
}

/// Parse + validate the bbctl self-host capability once and return the cached doc.
pub fn bbctl_doc() -> Result<&'static BbctlDoc, BridgeError> {
    let cached = BBCTL.get_or_init(|| {
        let doc: BbctlDoc = serde_json::from_str(BBCTL_JSON)
            .map_err(|e| BridgeError::Data(format!("bbctl.json failed to parse: {e}")))?;
        validate_bbctl(&doc)?;
        Ok(doc)
    });
    as_ref_result(cached)
}

impl BbctlDoc {
    /// The [`BbctlNetwork`] for `network_id` if it is a known self-hostable network,
    /// else `None` (a network absent from the set is never offered). Returns a clone
    /// so the caller owns a `Send` value.
    pub fn support_for(&self, network_id: &str) -> Option<BbctlNetwork> {
        self.networks
            .iter()
            .find(|n| n.network_id == network_id)
            .cloned()
    }

    /// The self-hostable networks marked `supported`, in file order. A network absent
    /// from this set is not offered in the run-your-own-bridge picker.
    pub fn networks(&self) -> Vec<&BbctlNetwork> {
        self.networks.iter().filter(|n| n.supported).collect()
    }
}

/// Validate the bbctl self-host capability: the schema version must match, the
/// install block must carry at least one non-empty step and a non-empty docs URL,
/// and every network must name a non-empty keeper `networkId` + `bbctlName` with no
/// duplicate `networkId` (a duplicate would make [`BbctlDoc::support_for`]
/// order-dependent).
fn validate_bbctl(doc: &BbctlDoc) -> Result<(), BridgeError> {
    if doc.version != BBCTL_VERSION {
        return Err(BridgeError::Data(format!(
            "bbctl.json unsupported version {} (expected {BBCTL_VERSION})",
            doc.version
        )));
    }
    if doc.install.docs_url.trim().is_empty() {
        return Err(BridgeError::Data(
            "bbctl.json install has an empty docsUrl".to_owned(),
        ));
    }
    if doc.install.steps.is_empty() || doc.install.steps.iter().any(|s| s.trim().is_empty()) {
        return Err(BridgeError::Data(
            "bbctl.json install has no valid steps".to_owned(),
        ));
    }
    let mut network_ids: HashSet<&str> = HashSet::new();
    for network in &doc.networks {
        if network.network_id.trim().is_empty() || network.bbctl_name.trim().is_empty() {
            return Err(BridgeError::Data(
                "bbctl.json has a network with an empty networkId/bbctlName".to_owned(),
            ));
        }
        if !network_ids.insert(network.network_id.as_str()) {
            return Err(BridgeError::Data(format!(
                "bbctl.json has a duplicate networkId {:?}",
                network.network_id
            )));
        }
    }
    Ok(())
}

impl ResolveSupportDoc {
    /// The [`ResolveSupport`] for `network_id`: the matching override if one exists,
    /// else the `default` capability (so every bridgev2 network has a capability
    /// without a per-network row). Returns a clone so the caller owns a `Send` value.
    pub fn support_for(&self, network_id: &str) -> ResolveSupport {
        self.overrides
            .iter()
            .find(|o| o.network_id == network_id)
            .map(|o| o.support.clone())
            .unwrap_or_else(|| self.default.clone())
    }
}

/// Validate the resolve-support capability: the schema version must match, the
/// default capability must be valid, every override must name a non-empty network id
/// with a valid capability, and no override network id may repeat (a duplicate would
/// make [`ResolveSupportDoc::support_for`] order-dependent).
fn validate_resolve_support(doc: &ResolveSupportDoc) -> Result<(), BridgeError> {
    if doc.version != RESOLVE_SUPPORT_VERSION {
        return Err(BridgeError::Data(format!(
            "resolve-support.json unsupported version {} (expected {RESOLVE_SUPPORT_VERSION})",
            doc.version
        )));
    }
    validate_resolve_capability(&doc.default, "default")?;
    let mut network_ids: HashSet<&str> = HashSet::new();
    for over in &doc.overrides {
        if over.network_id.trim().is_empty() {
            return Err(BridgeError::Data(
                "resolve-support.json has an override with an empty networkId".to_owned(),
            ));
        }
        validate_resolve_capability(&over.support, &over.network_id)?;
        if !network_ids.insert(over.network_id.as_str()) {
            return Err(BridgeError::Data(format!(
                "resolve-support.json has a duplicate override networkId {:?}",
                over.network_id
            )));
        }
    }
    Ok(())
}

/// A [`ResolveSupport`] that is `supported` must carry a non-empty identifier hint (a
/// supported network with no hint would render an empty label). An unsupported
/// network's hint is the "not supported" copy and is likewise required — `who` names
/// the offending row (`default` or a network id) for the error copy.
fn validate_resolve_capability(support: &ResolveSupport, who: &str) -> Result<(), BridgeError> {
    if support.identifier_hint.trim().is_empty() {
        return Err(BridgeError::Data(format!(
            "resolve-support.json capability {who:?} has an empty identifierHint"
        )));
    }
    Ok(())
}

impl HealthSignalsDoc {
    /// The health [`BridgeHealthGrammar`] for `network_id`: the matching override if
    /// one exists, else the `default` grammar (so every bridgev2 bot has a grammar
    /// without a per-network row). Returns a clone so the caller owns a `Send` value
    /// it can hold in a `Clone` monitor.
    pub fn grammar_for(&self, network_id: &str) -> BridgeHealthGrammar {
        self.overrides
            .iter()
            .find(|o| o.network_id == network_id)
            .map(|o| o.grammar.clone())
            .unwrap_or_else(|| self.default.clone())
    }
}

/// Validate the health-signal grammar: the schema version must match, the default
/// grammar must be valid, every override must name a non-empty network id with a
/// valid grammar, and no override network id may repeat (a duplicate would make
/// [`HealthSignalsDoc::grammar_for`] order-dependent).
fn validate_health_signals(doc: &HealthSignalsDoc) -> Result<(), BridgeError> {
    if doc.version != HEALTH_SIGNALS_VERSION {
        return Err(BridgeError::Data(format!(
            "health-signals.json unsupported version {} (expected {HEALTH_SIGNALS_VERSION})",
            doc.version
        )));
    }
    validate_health_grammar(&doc.default, "default")?;
    let mut network_ids: HashSet<&str> = HashSet::new();
    for over in &doc.overrides {
        if over.network_id.trim().is_empty() {
            return Err(BridgeError::Data(
                "health-signals.json has an override with an empty networkId".to_owned(),
            ));
        }
        validate_health_grammar(&over.grammar, &over.network_id)?;
        if !network_ids.insert(over.network_id.as_str()) {
            return Err(BridgeError::Data(format!(
                "health-signals.json has a duplicate override networkId {:?}",
                over.network_id
            )));
        }
    }
    Ok(())
}

/// A [`BridgeHealthGrammar`] must carry a non-empty ping command, a tick cadence in
/// `[1, 60]` (the detect-within-60s budget), and no empty marker phrase — `who`
/// names the offending row (`default` or a network id) for the error copy.
fn validate_health_grammar(grammar: &BridgeHealthGrammar, who: &str) -> Result<(), BridgeError> {
    if grammar.ping_command.trim().is_empty() {
        return Err(BridgeError::Data(format!(
            "health-signals.json grammar {who:?} has an empty pingCommand"
        )));
    }
    if grammar.tick_interval_secs == 0 || grammar.tick_interval_secs > 60 {
        return Err(BridgeError::Data(format!(
            "health-signals.json grammar {who:?} has tickIntervalSecs {} outside [1, 60]",
            grammar.tick_interval_secs
        )));
    }
    for markers in [
        &grammar.disconnected_markers,
        &grammar.degraded_markers,
        &grammar.healthy_markers,
    ] {
        if markers.iter().any(|m| m.trim().is_empty()) {
            return Err(BridgeError::Data(format!(
                "health-signals.json grammar {who:?} has an empty marker phrase"
            )));
        }
    }
    Ok(())
}

impl BotCommandsDoc {
    /// The command [`BotProtocol`] for `network_id`: the matching override if one
    /// exists, else the `default` protocol (so every bridgev2 bot works without a
    /// per-network row). Returns a clone so the caller owns a `Send` value it can
    /// hold in a `Clone` transport.
    pub fn protocol_for(&self, network_id: &str) -> BotProtocol {
        self.overrides
            .iter()
            .find(|o| o.network_id == network_id)
            .map(|o| o.protocol.clone())
            .unwrap_or_else(|| self.default.clone())
    }
}

/// Validate the bot-command protocol: the schema version must match, the default
/// protocol must carry non-empty login/cancel commands, every override must name a
/// non-empty network id with non-empty commands, and no override network id may
/// repeat (a duplicate would make [`BotCommandsDoc::protocol_for`] order-dependent).
fn validate_bot_commands(doc: &BotCommandsDoc) -> Result<(), BridgeError> {
    if doc.version != BOT_COMMANDS_VERSION {
        return Err(BridgeError::Data(format!(
            "bot-commands.json unsupported version {} (expected {BOT_COMMANDS_VERSION})",
            doc.version
        )));
    }
    validate_bot_protocol(&doc.default, "default")?;
    let mut network_ids: HashSet<&str> = HashSet::new();
    for over in &doc.overrides {
        if over.network_id.trim().is_empty() {
            return Err(BridgeError::Data(
                "bot-commands.json has an override with an empty networkId".to_owned(),
            ));
        }
        validate_bot_protocol(&over.protocol, &over.network_id)?;
        if !network_ids.insert(over.network_id.as_str()) {
            return Err(BridgeError::Data(format!(
                "bot-commands.json has a duplicate override networkId {:?}",
                over.network_id
            )));
        }
    }
    Ok(())
}

/// A [`BotProtocol`] must carry non-empty login and cancel command strings —
/// `who` names the offending row (`default` or a network id) for the error copy.
fn validate_bot_protocol(protocol: &BotProtocol, who: &str) -> Result<(), BridgeError> {
    if protocol.login_command.trim().is_empty() || protocol.cancel_command.trim().is_empty() {
        return Err(BridgeError::Data(format!(
            "bot-commands.json protocol {who:?} has an empty login/cancel command"
        )));
    }
    Ok(())
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
        bot_commands().expect("bot commands parse");
        health_signals().expect("health signals parse");
        resolve_support().expect("resolve support parse");
        bbctl_doc().expect("bbctl parse");
    }

    fn bbctl_network(id: &str, supported: bool) -> BbctlNetwork {
        BbctlNetwork {
            network_id: id.to_owned(),
            bbctl_name: format!("sh-{id}"),
            supported,
        }
    }

    fn bbctl_install() -> BbctlInstall {
        BbctlInstall {
            steps: vec!["install bbctl".to_owned()],
            docs_url: "https://example.org/docs".to_owned(),
        }
    }

    #[test]
    fn bbctl_doc_has_supported_and_unsupported_networks() {
        let doc = bbctl_doc().expect("bbctl parse");
        assert!(
            !doc.networks().is_empty(),
            "at least one supported self-host network must be declared"
        );
        assert!(
            doc.networks.iter().any(|n| !n.supported),
            "at least one network must be marked unsupported"
        );
    }

    #[test]
    fn bbctl_support_for_returns_only_known_networks() {
        let doc = bbctl_doc().expect("bbctl parse");
        assert!(
            doc.support_for("no-such-network").is_none(),
            "an unknown network must not resolve"
        );
        // Every declared network resolves to itself.
        for network in &doc.networks {
            let resolved = doc
                .support_for(&network.network_id)
                .expect("declared network resolves");
            assert_eq!(resolved.bbctl_name, network.bbctl_name);
        }
    }

    #[test]
    fn bbctl_networks_excludes_unsupported() {
        let doc = bbctl_doc().expect("bbctl parse");
        assert!(
            doc.networks().iter().all(|n| n.supported),
            "networks() must only include supported entries"
        );
    }

    #[test]
    fn rejects_unsupported_bbctl_version() {
        let doc = BbctlDoc {
            version: BBCTL_VERSION + 1,
            install: bbctl_install(),
            networks: vec![bbctl_network("signal", true)],
        };
        assert!(err_msg(validate_bbctl(&doc)).contains("unsupported version"));
    }

    #[test]
    fn rejects_bbctl_empty_docs_url() {
        let doc = BbctlDoc {
            version: BBCTL_VERSION,
            install: BbctlInstall {
                docs_url: "   ".to_owned(),
                ..bbctl_install()
            },
            networks: vec![bbctl_network("signal", true)],
        };
        assert!(err_msg(validate_bbctl(&doc)).contains("empty docsUrl"));
    }

    #[test]
    fn rejects_bbctl_empty_steps() {
        let doc = BbctlDoc {
            version: BBCTL_VERSION,
            install: BbctlInstall {
                steps: vec![],
                ..bbctl_install()
            },
            networks: vec![bbctl_network("signal", true)],
        };
        assert!(err_msg(validate_bbctl(&doc)).contains("no valid steps"));
    }

    #[test]
    fn rejects_bbctl_network_with_empty_field() {
        let doc = BbctlDoc {
            version: BBCTL_VERSION,
            install: bbctl_install(),
            networks: vec![BbctlNetwork {
                network_id: "signal".to_owned(),
                bbctl_name: "  ".to_owned(),
                supported: true,
            }],
        };
        assert!(err_msg(validate_bbctl(&doc)).contains("empty networkId/bbctlName"));
    }

    #[test]
    fn rejects_duplicate_bbctl_network_id() {
        let doc = BbctlDoc {
            version: BBCTL_VERSION,
            install: bbctl_install(),
            networks: vec![
                bbctl_network("signal", true),
                bbctl_network("signal", false),
            ],
        };
        assert!(err_msg(validate_bbctl(&doc)).contains("duplicate networkId"));
    }

    fn resolve_capability() -> ResolveSupport {
        ResolveSupport {
            supported: true,
            identifier_hint: "Phone number or username".to_owned(),
            placeholder: "+1 555 123 4567".to_owned(),
        }
    }

    #[test]
    fn resolve_support_default_is_supported_with_a_hint() {
        let doc = resolve_support().expect("resolve support parse");
        assert!(doc.default.supported, "default must be supported");
        assert!(
            !doc.default.identifier_hint.trim().is_empty(),
            "default identifier hint must be non-empty"
        );
    }

    #[test]
    fn resolve_support_for_falls_back_to_default() {
        let doc = resolve_support().expect("resolve support parse");
        // An unknown network with no override resolves to the default capability.
        let support = doc.support_for("no-such-network");
        assert_eq!(support.supported, doc.default.supported);
        assert_eq!(support.identifier_hint, doc.default.identifier_hint);
    }

    #[test]
    fn resolve_support_for_prefers_an_override() {
        let doc = ResolveSupportDoc {
            version: RESOLVE_SUPPORT_VERSION,
            default: resolve_capability(),
            overrides: vec![ResolveSupportOverride {
                network_id: "slack".to_owned(),
                support: ResolveSupport {
                    supported: false,
                    identifier_hint: "not supported on Slack".to_owned(),
                    placeholder: String::new(),
                },
            }],
        };
        assert!(!doc.support_for("slack").supported);
        assert!(doc.support_for("whatsapp").supported);
    }

    #[test]
    fn resolve_support_marks_an_unsupported_network() {
        // At least one genuinely-unsupported network must be declared upfront so the
        // dialog's "not supported" gate has data to drive it.
        let doc = resolve_support().expect("resolve support parse");
        assert!(
            doc.overrides.iter().any(|o| !o.support.supported),
            "at least one network must be marked unsupported"
        );
    }

    #[test]
    fn rejects_unsupported_resolve_support_version() {
        let doc = ResolveSupportDoc {
            version: RESOLVE_SUPPORT_VERSION + 1,
            default: resolve_capability(),
            overrides: vec![],
        };
        assert!(err_msg(validate_resolve_support(&doc)).contains("unsupported version"));
    }

    #[test]
    fn rejects_resolve_capability_with_empty_hint() {
        let doc = ResolveSupportDoc {
            version: RESOLVE_SUPPORT_VERSION,
            default: ResolveSupport {
                identifier_hint: "   ".to_owned(),
                ..resolve_capability()
            },
            overrides: vec![],
        };
        assert!(err_msg(validate_resolve_support(&doc)).contains("empty identifierHint"));
    }

    #[test]
    fn rejects_duplicate_resolve_support_override_network_id() {
        let over = |id: &str| ResolveSupportOverride {
            network_id: id.to_owned(),
            support: resolve_capability(),
        };
        let doc = ResolveSupportDoc {
            version: RESOLVE_SUPPORT_VERSION,
            default: resolve_capability(),
            overrides: vec![over("whatsapp"), over("whatsapp")],
        };
        assert!(err_msg(validate_resolve_support(&doc)).contains("duplicate override networkId"));
    }

    fn health_grammar() -> BridgeHealthGrammar {
        BridgeHealthGrammar {
            disconnected_markers: vec!["logged out".to_owned()],
            degraded_markers: vec!["reconnecting".to_owned()],
            healthy_markers: vec!["connected".to_owned()],
            ping_command: "ping".to_owned(),
            tick_interval_secs: 60,
            enable_ping: false,
        }
    }

    #[test]
    fn health_signals_default_has_all_marker_classes() {
        let doc = health_signals().expect("health signals parse");
        assert!(
            !doc.default.disconnected_markers.is_empty(),
            "default disconnected markers must be non-empty"
        );
        assert!(
            !doc.default.degraded_markers.is_empty(),
            "default degraded markers must be non-empty"
        );
        assert!(
            !doc.default.healthy_markers.is_empty(),
            "default healthy markers must be non-empty"
        );
        assert!(
            doc.default.tick_interval_secs >= 1 && doc.default.tick_interval_secs <= 60,
            "default tick cadence must be within the 60s budget"
        );
    }

    #[test]
    fn health_grammar_for_falls_back_to_default() {
        let doc = health_signals().expect("health signals parse");
        // An unknown network with no override resolves to the default grammar.
        let grammar = doc.grammar_for("no-such-network");
        assert_eq!(grammar.ping_command, doc.default.ping_command);
    }

    #[test]
    fn health_grammar_for_prefers_an_override() {
        let doc = HealthSignalsDoc {
            version: HEALTH_SIGNALS_VERSION,
            default: health_grammar(),
            overrides: vec![BridgeHealthGrammarOverride {
                network_id: "whatsapp".to_owned(),
                grammar: BridgeHealthGrammar {
                    healthy_markers: vec!["whatsapp connected".to_owned()],
                    ..health_grammar()
                },
            }],
        };
        assert_eq!(
            doc.grammar_for("whatsapp").healthy_markers,
            vec!["whatsapp connected".to_owned()]
        );
        assert_eq!(
            doc.grammar_for("signal").healthy_markers,
            vec!["connected".to_owned()]
        );
    }

    #[test]
    fn rejects_unsupported_health_signals_version() {
        let doc = HealthSignalsDoc {
            version: HEALTH_SIGNALS_VERSION + 1,
            default: health_grammar(),
            overrides: vec![],
        };
        assert!(err_msg(validate_health_signals(&doc)).contains("unsupported version"));
    }

    #[test]
    fn rejects_health_grammar_with_empty_ping_command() {
        let doc = HealthSignalsDoc {
            version: HEALTH_SIGNALS_VERSION,
            default: BridgeHealthGrammar {
                ping_command: "   ".to_owned(),
                ..health_grammar()
            },
            overrides: vec![],
        };
        assert!(err_msg(validate_health_signals(&doc)).contains("empty pingCommand"));
    }

    #[test]
    fn rejects_health_grammar_with_out_of_range_tick() {
        let doc = HealthSignalsDoc {
            version: HEALTH_SIGNALS_VERSION,
            default: BridgeHealthGrammar {
                tick_interval_secs: 120,
                ..health_grammar()
            },
            overrides: vec![],
        };
        assert!(err_msg(validate_health_signals(&doc)).contains("outside [1, 60]"));
    }

    #[test]
    fn rejects_health_grammar_with_empty_marker() {
        let doc = HealthSignalsDoc {
            version: HEALTH_SIGNALS_VERSION,
            default: BridgeHealthGrammar {
                healthy_markers: vec!["   ".to_owned()],
                ..health_grammar()
            },
            overrides: vec![],
        };
        assert!(err_msg(validate_health_signals(&doc)).contains("empty marker phrase"));
    }

    #[test]
    fn rejects_duplicate_health_override_network_id() {
        let over = |id: &str| BridgeHealthGrammarOverride {
            network_id: id.to_owned(),
            grammar: health_grammar(),
        };
        let doc = HealthSignalsDoc {
            version: HEALTH_SIGNALS_VERSION,
            default: health_grammar(),
            overrides: vec![over("whatsapp"), over("whatsapp")],
        };
        assert!(err_msg(validate_health_signals(&doc)).contains("duplicate override networkId"));
    }

    #[test]
    fn bot_commands_default_has_login_and_cancel() {
        let doc = bot_commands().expect("bot commands parse");
        assert!(
            !doc.default.login_command.trim().is_empty(),
            "default login command must be non-empty"
        );
        assert!(
            !doc.default.cancel_command.trim().is_empty(),
            "default cancel command must be non-empty"
        );
    }

    #[test]
    fn bot_commands_protocol_for_falls_back_to_default() {
        let doc = bot_commands().expect("bot commands parse");
        // An unknown network with no override resolves to the default protocol.
        let proto = doc.protocol_for("no-such-network");
        assert_eq!(proto.login_command, doc.default.login_command);
        assert_eq!(proto.cancel_command, doc.default.cancel_command);
    }

    #[test]
    fn bot_commands_protocol_for_prefers_an_override() {
        let doc = BotCommandsDoc {
            version: BOT_COMMANDS_VERSION,
            default: BotProtocol {
                login_command: "login".to_owned(),
                cancel_command: "cancel".to_owned(),
            },
            overrides: vec![BotProtocolOverride {
                network_id: "whatsapp".to_owned(),
                protocol: BotProtocol {
                    login_command: "login qr".to_owned(),
                    cancel_command: "cancel".to_owned(),
                },
            }],
        };
        assert_eq!(doc.protocol_for("whatsapp").login_command, "login qr");
        assert_eq!(doc.protocol_for("signal").login_command, "login");
    }

    #[test]
    fn rejects_unsupported_bot_commands_version() {
        let doc = BotCommandsDoc {
            version: BOT_COMMANDS_VERSION + 1,
            default: BotProtocol {
                login_command: "login".to_owned(),
                cancel_command: "cancel".to_owned(),
            },
            overrides: vec![],
        };
        assert!(err_msg(validate_bot_commands(&doc)).contains("unsupported version"));
    }

    #[test]
    fn rejects_bot_commands_default_with_empty_command() {
        let doc = BotCommandsDoc {
            version: BOT_COMMANDS_VERSION,
            default: BotProtocol {
                login_command: "   ".to_owned(),
                cancel_command: "cancel".to_owned(),
            },
            overrides: vec![],
        };
        assert!(err_msg(validate_bot_commands(&doc)).contains("empty login/cancel"));
    }

    #[test]
    fn rejects_duplicate_bot_commands_override_network_id() {
        let over = |id: &str| BotProtocolOverride {
            network_id: id.to_owned(),
            protocol: BotProtocol {
                login_command: "login".to_owned(),
                cancel_command: "cancel".to_owned(),
            },
        };
        let doc = BotCommandsDoc {
            version: BOT_COMMANDS_VERSION,
            default: BotProtocol {
                login_command: "login".to_owned(),
                cancel_command: "cancel".to_owned(),
            },
            overrides: vec![over("whatsapp"), over("whatsapp")],
        };
        assert!(err_msg(validate_bot_commands(&doc)).contains("duplicate override networkId"));
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
