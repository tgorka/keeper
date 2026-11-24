use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::Arc;

use keeper_app_api::{AppCapabilitiesVm, DomainVm, DomainsVm, HealthState, HealthVm, VersionVm};
use keeper_runtime::{app_module, AppInfo, Runtime};
use keeper_runtime_api::{
    Bindings, CallContext, CliHint, Decide, Effect, ErrorCode, Kind, Manifest, Module,
    OperationSpec, ProposalForm, Read, Scope,
};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::json;

#[derive(Debug, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct CountIn {
    count: u32,
}

#[derive(Debug, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct EmptyIn {}

fn schema<T: JsonSchema>() -> schemars::Schema {
    schemars::schema_for!(T)
}

static FIXTURE_OPS: &[OperationSpec] = &[
    OperationSpec {
        id: "fixture.count",
        kind: Kind::Query,
        effect: Effect::Read,
        summary: "bump counter and echo count",
        scope: Scope::App,
        proposal: ProposalForm::None,
        input: schema::<CountIn>,
        output: schema::<CountIn>,
        concise: &[],
        page: None,
        cli: CliHint {
            words: &["fixture", "count"],
            positional: &[],
        },
        wire_forbid: &[],
        wire_fix: &[],
        to_proposal: None,
        wire_gate: None,
        vectors: &[],
    },
    OperationSpec {
        id: "fixture.approve",
        kind: Kind::Command,
        effect: Effect::Decide,
        summary: "check local caller on decide op",
        scope: Scope::App,
        proposal: ProposalForm::None,
        input: schema::<EmptyIn>,
        output: schema::<serde_json::Value>,
        concise: &[],
        page: None,
        cli: CliHint {
            words: &["fixture", "approve"],
            positional: &[],
        },
        wire_forbid: &[],
        wire_fix: &[],
        to_proposal: None,
        wire_gate: None,
        vectors: &[],
    },
];

struct FixtureModule {
    hits: Arc<AtomicU32>,
}

impl Module for FixtureModule {
    fn manifest(&self) -> Manifest {
        Manifest {
            id: "fixture",
            requires: &[],
            operations: FIXTURE_OPS,
        }
    }

    fn bind(&self, bindings: &mut Bindings) {
        let hits = Arc::clone(&self.hits);
        bindings.read(
            "fixture.count",
            move |_ctx: CallContext<Read>, input: CountIn| {
                let hits = Arc::clone(&hits);
                async move {
                    hits.fetch_add(1, Ordering::SeqCst);
                    Ok(CountIn { count: input.count })
                }
            },
        );

        bindings.decide(
            "fixture.approve",
            move |ctx: CallContext<Decide>, _: EmptyIn| {
                let is_local = ctx.caller().is_local();
                async move { Ok(json!({ "local": is_local })) }
            },
        );
    }

    fn unavailable(&self, id: &str) -> Option<String> {
        (id == "fixture.approve").then(|| "requires local person".to_string())
    }
}

fn test_info() -> AppInfo {
    AppInfo {
        version: "0.12.0".to_string(),
        build: "test-build".to_string(),
        snapshot_digest: "deadbeef".to_string(),
        domains: vec![DomainVm {
            server: "https://hq.example".to_string(),
            domain: "personal".to_string(),
        }],
    }
}

#[tokio::test]
async fn dispatch_validates_inputs_counts_calls_and_mints_local_context() {
    let hits = Arc::new(AtomicU32::new(0));
    let fixture = Arc::new(FixtureModule {
        hits: Arc::clone(&hits),
    });
    let (_runtime, dispatcher) = Runtime::build(test_info(), vec![app_module(), fixture])
        .expect("valid runtime should build");

    // Wrong field type -> INVALID_ARGUMENT naming `count`, handler never runs.
    let err = dispatcher
        .call("fixture.count", json!({ "count": "x" }))
        .await
        .expect_err("string for u32 must fail");
    assert_eq!(err.code, ErrorCode::InvalidArgument);
    assert!(
        err.reason.contains("count"),
        "expected reason to name `count`, got: {:?}",
        err.reason
    );
    assert_eq!(hits.load(Ordering::SeqCst), 0);

    // Unknown field -> INVALID_ARGUMENT, handler still never runs.
    let err = dispatcher
        .call("fixture.count", json!({ "count": 1, "extra": true }))
        .await
        .expect_err("unknown field must fail");
    assert_eq!(err.code, ErrorCode::InvalidArgument);
    assert_eq!(hits.load(Ordering::SeqCst), 0);

    // Valid call -> answers and bumps counter to 1.
    let reply = dispatcher
        .call("fixture.count", json!({ "count": 7 }))
        .await
        .expect("valid input must succeed");
    assert_eq!(reply, json!({ "count": 7 }));
    assert_eq!(hits.load(Ordering::SeqCst), 1);
    assert_eq!(dispatcher.dispatches(), 3);

    // Unknown operation id -> NOT_FOUND.
    let err = dispatcher
        .call("fixture.missing", json!({}))
        .await
        .expect_err("unknown op must return NOT_FOUND");
    assert_eq!(err.code, ErrorCode::NotFound);
    assert_eq!(dispatcher.dispatches(), 4);

    // Decide op receives a local caller.
    let reply = dispatcher
        .call("fixture.approve", json!({}))
        .await
        .expect("decide op must succeed for local caller");
    assert_eq!(reply, json!({ "local": true }));
    assert_eq!(dispatcher.dispatches(), 5);
}

#[tokio::test]
async fn app_module_answers_from_runtime_state() {
    let hits = Arc::new(AtomicU32::new(0));
    let fixture = Arc::new(FixtureModule { hits });
    let (_runtime, dispatcher) = Runtime::build(test_info(), vec![app_module(), fixture])
        .expect("valid runtime should build");

    let version: VersionVm = serde_json::from_value(
        dispatcher
            .call("app.version", json!({}))
            .await
            .expect("app.version"),
    )
    .expect("decode VersionVm");
    assert_eq!(version.version, "0.12.0");
    assert_eq!(version.build, "test-build");
    assert_eq!(version.snapshot_digest, "deadbeef");

    let health: HealthVm = serde_json::from_value(
        dispatcher
            .call("app.health", json!({}))
            .await
            .expect("app.health"),
    )
    .expect("decode HealthVm");
    assert_eq!(health.state, HealthState::Serving);
    assert_eq!(health.sentence.as_deref(), Some("keeper is serving"));

    let domains: DomainsVm = serde_json::from_value(
        dispatcher
            .call("app.domains", json!({}))
            .await
            .expect("app.domains"),
    )
    .expect("decode DomainsVm");
    assert_eq!(domains.domains.len(), 1);
    assert_eq!(domains.domains[0].domain, "personal");

    let caps: AppCapabilitiesVm = serde_json::from_value(
        dispatcher
            .call("app.capabilities", json!({}))
            .await
            .expect("app.capabilities"),
    )
    .expect("decode AppCapabilitiesVm");
    assert_eq!(caps.platform, std::env::consts::OS);
    let approve = caps
        .operations
        .iter()
        .find(|op| op.id == "fixture.approve")
        .expect("fixture.approve in capabilities");
    assert!(!approve.available);
    assert_eq!(approve.reason.as_deref(), Some("requires local person"));
    let count = caps
        .operations
        .iter()
        .find(|op| op.id == "fixture.count")
        .expect("fixture.count in capabilities");
    assert!(count.available);
    assert!(count.reason.is_none());
}
