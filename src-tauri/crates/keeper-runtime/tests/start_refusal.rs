use std::process::{Command, Output};
use std::sync::Arc;

use keeper_app_api::{ModuleState, ModulesVm};
use keeper_runtime::{app_module, AppInfo, Runtime};
use keeper_runtime_api::{
    Bindings, CallContext, CliHint, Effect, Kind, Manifest, Module, OperationSpec, ProposalForm,
    Read, Requirement, Scope, Write,
};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::json;

const SCENARIO_ENV: &str = "KEEPER_RUNTIME_SCENARIO";

#[derive(Debug, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct SimpleIn {
    name: String,
}

fn schema<T: JsonSchema>() -> schemars::Schema {
    schemars::schema_for!(T)
}

const fn read_op(id: &'static str) -> OperationSpec {
    OperationSpec {
        id,
        kind: Kind::Query,
        effect: Effect::Read,
        summary: "fixture read",
        scope: Scope::App,
        proposal: ProposalForm::None,
        input: schema::<SimpleIn>,
        output: schema::<serde_json::Value>,
        concise: &[],
        page: None,
        cli: CliHint {
            words: &["fixture", "op"],
            positional: &[],
        },
        wire_forbid: &[],
        wire_fix: &[],
        to_proposal: None,
        wire_gate: None,
        vectors: &[],
    }
}

static ALPHA_OPS: &[OperationSpec] = &[read_op("alpha.ping")];
static DUP_OPS: &[OperationSpec] = &[read_op("dup.ping"), read_op("dup.ping")];
static UNBOUND_OPS: &[OperationSpec] = &[read_op("unbound.ping")];
static EXTRA_OPS: &[OperationSpec] = &[read_op("extra.ping")];
static BAD_VIA_OPS: &[OperationSpec] = &[
    OperationSpec {
        id: "badvia.apply",
        kind: Kind::Command,
        effect: Effect::Write,
        summary: "write without to_proposal",
        scope: Scope::App,
        proposal: ProposalForm::Via("badvia.propose"),
        input: schema::<SimpleIn>,
        output: schema::<serde_json::Value>,
        concise: &[],
        page: None,
        cli: CliHint {
            words: &["badvia", "apply"],
            positional: &[],
        },
        wire_forbid: &[],
        wire_fix: &[],
        to_proposal: None,
        wire_gate: None,
        vectors: &[],
    },
    OperationSpec {
        id: "badvia.propose",
        kind: Kind::Command,
        effect: Effect::Write,
        summary: "proposal target",
        scope: Scope::App,
        proposal: ProposalForm::Itself,
        input: schema::<SimpleIn>,
        output: schema::<serde_json::Value>,
        concise: &[],
        page: None,
        cli: CliHint {
            words: &["badvia", "propose"],
            positional: &[],
        },
        wire_forbid: &[],
        wire_fix: &[],
        to_proposal: None,
        wire_gate: None,
        vectors: &[],
    },
];
static BAD_WIRE_OPS: &[OperationSpec] = &[OperationSpec {
    id: "badwire.ping",
    kind: Kind::Query,
    effect: Effect::Read,
    summary: "bad wire_forbid pointer",
    scope: Scope::App,
    proposal: ProposalForm::None,
    input: schema::<SimpleIn>,
    output: schema::<serde_json::Value>,
    concise: &[],
    page: None,
    cli: CliHint {
        words: &["badwire", "ping"],
        positional: &[],
    },
    wire_forbid: &["/nonexistent/field"],
    wire_fix: &[],
    to_proposal: None,
    wire_gate: None,
    vectors: &[],
}];

static REQ_MISSING: &[Requirement] = &[Requirement {
    module: "ghost",
    optional: false,
}];
static REQ_OPTIONAL: &[Requirement] = &[Requirement {
    module: "optional_mod",
    optional: true,
}];
static REQ_CYCLE_A: &[Requirement] = &[Requirement {
    module: "cycle_b",
    optional: false,
}];
static REQ_CYCLE_B: &[Requirement] = &[Requirement {
    module: "cycle_a",
    optional: false,
}];

struct Fixture {
    id: &'static str,
    requires: &'static [Requirement],
    operations: &'static [OperationSpec],
    bind_mode: BindMode,
}

#[derive(Clone, Copy)]
enum BindMode {
    Normal,
    SkipAll,
    AddUnknown(&'static str),
    WriteAll,
}

impl Module for Fixture {
    fn manifest(&self) -> Manifest {
        Manifest {
            id: self.id,
            requires: self.requires,
            operations: self.operations,
        }
    }

    fn bind(&self, bindings: &mut Bindings) {
        match self.bind_mode {
            BindMode::Normal => {
                for spec in self.operations {
                    bindings.read(spec.id, |_ctx: CallContext<Read>, _: SimpleIn| async move {
                        Ok(json!({ "ok": true }))
                    });
                }
            }
            BindMode::SkipAll => {}
            BindMode::AddUnknown(extra_id) => {
                for spec in self.operations {
                    bindings.read(spec.id, |_ctx: CallContext<Read>, _: SimpleIn| async move {
                        Ok(json!({ "ok": true }))
                    });
                }
                bindings.read(
                    extra_id,
                    |_ctx: CallContext<Read>, _: SimpleIn| async move { Ok(json!({ "ok": true })) },
                );
            }
            BindMode::WriteAll => {
                for spec in self.operations {
                    bindings.write(
                        spec.id,
                        |_ctx: CallContext<Write>, _: SimpleIn| async move {
                            Ok(json!({ "ok": true }))
                        },
                    );
                }
            }
        }
    }
}

fn info() -> AppInfo {
    AppInfo {
        version: "0.12.0".to_string(),
        build: "test".to_string(),
        snapshot_digest: "0000".to_string(),
        domains: Vec::new(),
    }
}

fn scenario_modules(name: &str) -> Vec<Arc<dyn Module>> {
    match name {
        "missing_required" => vec![
            app_module(),
            Arc::new(Fixture {
                id: "alpha",
                requires: REQ_MISSING,
                operations: ALPHA_OPS,
                bind_mode: BindMode::Normal,
            }),
        ],
        "optional_absent" => vec![
            app_module(),
            Arc::new(Fixture {
                id: "alpha",
                requires: REQ_OPTIONAL,
                operations: ALPHA_OPS,
                bind_mode: BindMode::Normal,
            }),
        ],
        "duplicate_op" => vec![
            app_module(),
            Arc::new(Fixture {
                id: "dup",
                requires: &[],
                operations: DUP_OPS,
                bind_mode: BindMode::Normal,
            }),
        ],
        "unbound_op" => vec![
            app_module(),
            Arc::new(Fixture {
                id: "unbound",
                requires: &[],
                operations: UNBOUND_OPS,
                bind_mode: BindMode::SkipAll,
            }),
        ],
        "unknown_binding" => vec![
            app_module(),
            Arc::new(Fixture {
                id: "extra",
                requires: &[],
                operations: EXTRA_OPS,
                bind_mode: BindMode::AddUnknown("extra.ghost"),
            }),
        ],
        "bad_via" => vec![
            app_module(),
            Arc::new(Fixture {
                id: "badvia",
                requires: &[],
                operations: BAD_VIA_OPS,
                bind_mode: BindMode::WriteAll,
            }),
        ],
        "bad_wire" => vec![
            app_module(),
            Arc::new(Fixture {
                id: "badwire",
                requires: &[],
                operations: BAD_WIRE_OPS,
                bind_mode: BindMode::Normal,
            }),
        ],
        "cycle" => vec![
            app_module(),
            Arc::new(Fixture {
                id: "cycle_a",
                requires: REQ_CYCLE_A,
                operations: &[],
                bind_mode: BindMode::Normal,
            }),
            Arc::new(Fixture {
                id: "cycle_b",
                requires: REQ_CYCLE_B,
                operations: &[],
                bind_mode: BindMode::Normal,
            }),
        ],
        other => panic!("unknown scenario: {other}"),
    }
}

#[test]
fn scenario_runner() {
    let Ok(scenario) = std::env::var(SCENARIO_ENV) else {
        return;
    };
    match Runtime::build(info(), scenario_modules(&scenario)) {
        Err(err) => {
            eprintln!("start refused: {err}");
            std::process::exit(3);
        }
        Ok((_, dispatcher)) => {
            let rt = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .expect("tokio current_thread runtime");
            let out = rt
                .block_on(dispatcher.call("app.modules", json!({})))
                .expect("app.modules should answer");
            println!("{out}");
            std::process::exit(0);
        }
    }
}

fn run_scenario(name: &str) -> Output {
    let exe = std::env::current_exe().expect("current_exe");
    Command::new(exe)
        .args(["--exact", "scenario_runner", "--nocapture"])
        .env(SCENARIO_ENV, name)
        .output()
        .expect("spawn scenario_runner")
}

#[test]
fn missing_required_module_exits_3_and_names_both_modules() {
    let out = run_scenario("missing_required");
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert_eq!(out.status.code(), Some(3), "stderr: {stderr}");
    assert!(stderr.contains("start refused:"), "stderr: {stderr}");
    assert!(stderr.contains("alpha"), "stderr: {stderr}");
    assert!(stderr.contains("ghost"), "stderr: {stderr}");
}

#[test]
fn missing_optional_module_exits_0_and_reports_absent() {
    let out = run_scenario("optional_absent");
    let stdout = String::from_utf8_lossy(&out.stdout);
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert_eq!(
        out.status.code(),
        Some(0),
        "stdout: {stdout}\nstderr: {stderr}"
    );
    let json_line = stdout
        .lines()
        .find(|line| line.trim_start().starts_with('{'))
        .unwrap_or_else(|| panic!("expected JSON line in stdout, got: {stdout}"));
    let modules: ModulesVm = serde_json::from_str(json_line).expect("parse ModulesVm");
    let opt = modules
        .modules
        .iter()
        .find(|m| m.id == "optional_mod")
        .expect("optional_mod in app.modules");
    assert_eq!(opt.state, ModuleState::Absent);
    let alpha = modules
        .modules
        .iter()
        .find(|m| m.id == "alpha")
        .expect("alpha in app.modules");
    assert_eq!(alpha.state, ModuleState::Ready);
}

#[test]
fn start_check_refusals_exit_3_naming_module_and_problem() {
    let cases = [
        ("duplicate_op", &["dup", "dup.ping"][..]),
        ("unbound_op", &["unbound", "unbound.ping"][..]),
        ("unknown_binding", &["extra", "extra.ghost"][..]),
        ("bad_via", &["badvia", "badvia.apply"][..]),
        ("bad_wire", &["badwire", "/nonexistent/field"][..]),
        ("cycle", &["cycle_a", "cycle_b"][..]),
    ];

    for (scenario, expected_tokens) in cases {
        let out = run_scenario(scenario);
        let stderr = String::from_utf8_lossy(&out.stderr);
        assert_eq!(
            out.status.code(),
            Some(3),
            "scenario {scenario} should exit 3, stderr: {stderr}"
        );
        assert!(
            stderr.contains("start refused:"),
            "scenario {scenario} missing prefix, stderr: {stderr}"
        );
        for token in expected_tokens {
            assert!(
                stderr.contains(token),
                "scenario {scenario} stderr should mention '{token}', got: {stderr}"
            );
        }
    }
}
