//! The `app` module's contract (story 123.2): five `read` operations of
//! scope `App`, answered by `keeper-runtime` itself.
//!
//! - `app.version`: version, build, and the digest of the one committed
//!   catalogue snapshot embedded at build (SR-20);
//! - `app.modules`: each module `ready`, `absent` or `failed`;
//! - `app.health`: `serving`, `degraded` or `stopped` — with a sentence for
//!   a local caller only;
//! - `app.capabilities`: the platform, and for each operation whether it is
//!   available here and, when not, why (an operation is never silently
//!   absent on one platform);
//! - `app.domains`: the caller's visible domains — nothing for a wire
//!   caller before 123.4.
//!
//! **Extension point (epic 124).** 124.1 adds the shell's contributions —
//! views, settings sections, categories and slots — to this crate as new
//! modules beside [`OPERATIONS`]; nothing here anticipates their shape.

use keeper_runtime_api::{CliHint, Effect, Kind, OperationSpec, ProposalForm, Scope};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use ts_rs::TS;

/// The module's id.
pub const MODULE: &str = "app";

/// Every `app` operation takes no argument.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema, TS)]
#[serde(deny_unknown_fields)]
pub struct NoInput {}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema, TS)]
pub struct VersionVm {
    pub version: String,
    pub build: String,
    /// SHA-256 (hex) of `catalogue/snapshot.json` as embedded at build.
    pub snapshot_digest: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "snake_case")]
pub enum ModuleState {
    Ready,
    Absent,
    Failed,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema, TS)]
pub struct ModuleStatusVm {
    pub id: String,
    pub state: ModuleState,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema, TS)]
pub struct ModulesVm {
    pub modules: Vec<ModuleStatusVm>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "snake_case")]
pub enum HealthState {
    Serving,
    Degraded,
    Stopped,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema, TS)]
pub struct HealthVm {
    pub state: HealthState,
    /// Why, for a local caller; always `None` to a wire caller.
    pub sentence: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema, TS)]
pub struct OperationAvailabilityVm {
    pub id: String,
    pub available: bool,
    /// Why not, when it is not.
    pub reason: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema, TS)]
pub struct AppCapabilitiesVm {
    /// `std::env::consts::OS`.
    pub platform: String,
    pub operations: Vec<OperationAvailabilityVm>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema, TS)]
pub struct DomainVm {
    pub server: String,
    pub domain: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema, TS)]
pub struct DomainsVm {
    pub domains: Vec<DomainVm>,
}

fn schema<T: JsonSchema>() -> schemars::Schema {
    schemars::schema_for!(T)
}

const fn op(
    id: &'static str,
    summary: &'static str,
    words: &'static [&'static str],
    output: fn() -> schemars::Schema,
) -> OperationSpec {
    OperationSpec {
        id,
        kind: Kind::Query,
        effect: Effect::Read,
        summary,
        scope: Scope::App,
        proposal: ProposalForm::None,
        input: schema::<NoInput>,
        output,
        concise: &[],
        page: None,
        cli: CliHint {
            words,
            positional: &[],
        },
        wire_forbid: &[],
        wire_fix: &[],
        to_proposal: None,
        wire_gate: None,
        vectors: &[],
    }
}

/// The `app` module's catalogue.
pub static OPERATIONS: &[OperationSpec] = &[
    op(
        "app.version",
        "keeper's version, build and catalogue snapshot digest",
        &["app", "version"],
        schema::<VersionVm>,
    ),
    op(
        "app.modules",
        "each module of this build: ready, absent or failed",
        &["app", "modules"],
        schema::<ModulesVm>,
    ),
    op(
        "app.health",
        "whether keeper is serving, degraded or stopped",
        &["app", "health"],
        schema::<HealthVm>,
    ),
    op(
        "app.capabilities",
        "the platform, and which operations are available here and why not",
        &["app", "capabilities"],
        schema::<AppCapabilitiesVm>,
    ),
    op(
        "app.domains",
        "the caller's visible domains",
        &["app", "domains"],
        schema::<DomainsVm>,
    ),
];
