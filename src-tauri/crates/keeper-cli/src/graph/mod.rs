//! `keeper-cli graph`: a person's device's graph without a window — the
//! headless app-human consumer of the safe graph API against a real
//! keeper-hq (E1). Each run opens the device's persistent on-disk replica of
//! one database and does one thing; its answer is one JSON document under
//! `--json` (one line), otherwise a sentence ([`human`]):
//!
//! - `apply <payload.json>` — a `graph.apply` change (the payload as the
//!   contract writes it) and its outcome, online only;
//! - `enqueue <payload.json>` — persist the change in the device's intent
//!   queue (offline too) and print its request id; nothing is sent;
//! - `annotate <entry_id> [--note <text>] [--tag <t>]…` — a note or tags on
//!   a file's graph node, created once (116.5), online only;
//! - `pending` — the queue's entries: ids, state, cause and age only;
//! - `resume` — restore every open entry from its stored bytes, resolve it
//!   from its receipt (here or `graph.outcome`) and send the identical
//!   request under its own id only when no receipt exists;
//! - `discard <request_id>` — withdraw an entry never sent, suspended or
//!   locked; it is never sent again;
//! - `sync` — wait until the replica holds the domain's graph, then build
//!   or extend the local query projection; a revoked domain is cleaned up
//!   here, and unlocked only by fresh authority;
//! - `query <source> [--params <json>] [--allow-stale]` — one
//!   `KG-Cypher/1` query over the projection (with `--allow-stale`, served
//!   `stale` from the last-authorised generation while the server cannot be
//!   reached, at most a day after the last successful authority read);
//! - `inspect node|edge|content <id> [--include-archived]` — the typed
//!   lookup over the projection (a content only while its owner is active,
//!   unless archived ones are included);
//! - `history [--after <n>] [--limit <n>]` — one page of retained history;
//! - `unreviewed [--after <n>] [--limit <n>]` — one page of the unreviewed
//!   feed (a reviewer's);
//! - `propose <payload.json>` / `decide <payload.json>` — a `graph.propose`
//!   or `graph.decide` payload and its outcome;
//! - `proposal <proposal_id>` — the proposal's revisions as its enumerated
//!   reviewer reads them;
//! - `acknowledge <event_id> <delta_digest>` — a human review of one event;
//! - `preview-reversal <event_id>` — the inverse and what it requires now;
//! - `reverse <event_id> [--approval-id <id>]` — preview, then reverse under
//!   exactly the previewed revisions (refused without a send unless the
//!   preview is ready, or `approval_required` and an approval id is given:
//!   an agent's per-call approval);
//! - `status` — the replica's head and checkpoint, the projection's runtime
//!   admission, the revocation fence and the queue's counts.
//!
//! The queue, its revocation fence and the last read authority persist in
//! `<state>/<database>.graph-queue`, beside the replica. A revocation (a
//! refused link, a revoked subscription, a server refusal) locks the domain
//! before anything else: reads, history and the queue refuse until `sync`
//! sees the replica's purge and fresh authority.
//!
//! The device token is read from `KEEPER_GRAPH_TOKEN`, never from argv, and
//! never printed. The worker is `--worker <absolute path>`, admitted only
//! with the digest (`KEEPER_GRAPH_WORKER_SHA256`) and, on Linux, the runtime
//! ledger (`KEEPER_GRAPH_RUNTIME_LEDGER`) embedded at build time; macOS runs
//! a worker only with `--macos-evidence` (evidence only, OD-C).
//!
//! Exit codes ([`exit_code`]): `0` an `ok` answer, whatever the server's
//! outcome; `2` the invocation or its inputs are wrong (no token, a bad
//! `--url`, an unreadable or refused payload, an unknown entity, a query
//! outside the language); `3` no admitted worker (none given, a digest or
//! ledger not embedded or not matching, an unsupported platform); `4` a
//! reversal that waits for an agent's per-call approval, nothing sent; `1`
//! any other failure.
//!
//! # One list, one dispatcher (story 123.2)
//!
//! The verbs' bodies live in `keeper-graph`'s `module` feature; this file
//! parses 113.5's arguments, opens one replica per `--database` (the
//! composition root opens; the module opens nothing it is not given,
//! SR-5), and reaches the verb through keeper-runtime's local dispatcher by
//! its operation id (`graph.<verb>`), against the first `--database`. With
//! `KEEPER_CLI_DISPATCHES` set, the run's dispatch count goes to stderr.

mod human;

use crate::output::Printer;
use crate::{EXIT_CONFIG, EXIT_DEFERRED, EXIT_FAILURE, EXIT_OK, EXIT_PREREQUISITE};

use std::path::{Path, PathBuf};
use std::sync::Arc;

use clap::Subcommand;
use keeper_graph::client::units;
use keeper_graph::module::{missing_prerequisite, render, DomainBinding, GraphConfig};
use keeper_ports::server::ServerUrl;
use keeper_ports::{Link, ReplicaSpec, Source};
use keeper_replica::SyncularReplica;
use keeper_runtime_api::{ErrorCode, OpError};
use serde_json::{json, Map, Value};

mod embedded {
    include!(concat!(env!("OUT_DIR"), "/ledger.rs"));
}

/// `keeper-cli graph`'s global arguments, before the verb.
#[derive(clap::Args)]
pub struct Args {
    /// Where the replicas and the projections persist.
    #[arg(long)]
    state: PathBuf,
    /// keeper-hq's base URL (`<url>/<database>` is the mount).
    #[arg(long)]
    url: String,
    /// A database to open; repeat it for more. The verb addresses the first.
    #[arg(long, required = true)]
    database: Vec<String>,
    /// The person's actor id: once, or once per `--database`.
    #[arg(long, required = true)]
    actor: Vec<String>,
    /// This device's client id: once, or once per `--database`.
    #[arg(long, required = true)]
    client: Vec<String>,
    /// The graph worker executable (absolute path).
    #[arg(long)]
    worker: Option<PathBuf>,
    /// macOS only: run the worker under the evidence-only sandbox.
    #[arg(long)]
    macos_evidence: bool,
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Send a `graph.apply` change and print its outcome (online only).
    Apply { payload: PathBuf },
    /// Persist a change in the intent queue, offline too; nothing is sent.
    Enqueue { payload: PathBuf },
    /// Put a note or tags on a file's graph node, creating it once (online only).
    Annotate {
        entry_id: String,
        #[arg(long)]
        note: Option<String>,
        #[arg(long = "tag")]
        tags: Vec<String>,
    },
    /// The queue's entries: ids, state, cause and age only.
    Pending,
    /// Resolve every open entry from its receipt; send it only when none exists.
    Resume,
    /// Withdraw an entry never sent, suspended or locked.
    Discard { request_id: String },
    /// Wait for the domain's graph, then build or extend the query projection.
    Sync {
        #[arg(long, default_value_t = 60)]
        wait_secs: u64,
    },
    /// One `KG-Cypher/1` query over the projection.
    Query {
        source: String,
        #[arg(long, default_value = "{}")]
        params: String,
        #[arg(long)]
        allow_stale: bool,
    },
    /// Look up one node, edge or content over the projection.
    Inspect {
        entity: String,
        id: String,
        #[arg(long)]
        include_archived: bool,
        #[arg(long)]
        allow_stale: bool,
    },
    /// One page of retained history.
    History {
        #[arg(long, default_value = "0")]
        after: String,
        #[arg(long, default_value_t = 100)]
        limit: u32,
    },
    /// One page of the unreviewed feed (a reviewer's).
    Unreviewed {
        #[arg(long, default_value = "0")]
        after: String,
        #[arg(long, default_value_t = 100)]
        limit: u32,
    },
    /// Send a `graph.propose` payload and print its outcome.
    Propose { payload: PathBuf },
    /// Send a `graph.decide` payload and print its outcome.
    Decide { payload: PathBuf },
    /// A proposal's revisions, as its enumerated reviewer reads them.
    Proposal { proposal_id: String },
    /// A human review of one event.
    Acknowledge {
        event_id: String,
        delta_digest: String,
    },
    /// The inverse of an event and what it requires now.
    PreviewReversal { event_id: String },
    /// Preview, then reverse under exactly the previewed revisions.
    Reverse {
        event_id: String,
        /// The per-call approval an agent's reversal needs when the
        /// preview answers `approval_required`.
        #[arg(long)]
        approval_id: Option<String>,
    },
    /// The replica's head and checkpoint, the runtime admission, the fence
    /// and the queue's counts.
    Status,
}

impl Command {
    /// The verb as it is typed.
    fn name(&self) -> &'static str {
        match self {
            Self::Apply { .. } => "apply",
            Self::Enqueue { .. } => "enqueue",
            Self::Annotate { .. } => "annotate",
            Self::Pending => "pending",
            Self::Resume => "resume",
            Self::Discard { .. } => "discard",
            Self::Sync { .. } => "sync",
            Self::Query { .. } => "query",
            Self::Inspect { .. } => "inspect",
            Self::History { .. } => "history",
            Self::Unreviewed { .. } => "unreviewed",
            Self::Propose { .. } => "propose",
            Self::Decide { .. } => "decide",
            Self::Proposal { .. } => "proposal",
            Self::Acknowledge { .. } => "acknowledge",
            Self::PreviewReversal { .. } => "preview-reversal",
            Self::Reverse { .. } => "reverse",
            Self::Status => "status",
        }
    }
}

fn fail(code: &str, detail: &str) -> Value {
    json!({"ok": false, "error": code, "detail": detail})
}

/// A payload file as an input: its JSON (read by the contract's value
/// rules) beside `target`. `opened`: whether the addressed replica opened;
/// when it did not, the payload is not read, so the replica's refusal
/// answers first, as it always has.
fn payload_input(path: &PathBuf, target: Value, opened: bool) -> Result<Value, Value> {
    let mut input = Map::new();
    if opened {
        let text = std::fs::read(path).map_err(|e| fail("invalid_change", &e.to_string()))?;
        let value = keeper_graph::protocol::value::parse(&text)
            .map_err(|refusal| fail(refusal.code(), "payload"))?;
        match value {
            // `target` is no member of any payload: the contract refuses it.
            Value::Object(fields) if !fields.contains_key("target") => input = fields,
            _ => return Err(fail("invalid_change", "payload")),
        }
    }
    input.insert("target".into(), target);
    Ok(Value::Object(input))
}

impl Command {
    /// The operation this verb is, and its input.
    fn request(&self, target: Value, opened: bool) -> Result<(&'static str, Value), Value> {
        let t = target.clone();
        Ok(match self {
            Self::Apply { payload } => ("graph.apply", payload_input(payload, target, opened)?),
            Self::Enqueue { payload } => ("graph.enqueue", payload_input(payload, target, opened)?),
            Self::Annotate {
                entry_id,
                note,
                tags,
            } => (
                "graph.annotate",
                json!({"target": t, "entry_id": entry_id, "note": note, "tags": tags}),
            ),
            Self::Propose { payload } => ("graph.propose", payload_input(payload, target, opened)?),
            Self::Decide { payload } => ("graph.decide", payload_input(payload, target, opened)?),
            Self::Pending => ("graph.pending", json!({"target": t})),
            Self::Resume => ("graph.resume", json!({"target": t})),
            Self::Status => ("graph.status", json!({"target": t})),
            Self::Discard { request_id } => (
                "graph.discard",
                json!({"target": t, "request_id": request_id}),
            ),
            Self::Sync { wait_secs } => {
                ("graph.sync", json!({"target": t, "wait_secs": wait_secs}))
            }
            Self::Query {
                source,
                params,
                allow_stale,
            } => (
                "graph.query",
                json!({"target": t, "source": source, "params": params, "allow_stale": allow_stale}),
            ),
            Self::Inspect {
                entity,
                id,
                include_archived,
                allow_stale,
            } => (
                "graph.inspect",
                json!({"target": t, "entity": entity, "id": id,
                       "include_archived": include_archived, "allow_stale": allow_stale}),
            ),
            Self::History { after, limit } => (
                "graph.history",
                json!({"target": t, "after": after, "limit": limit}),
            ),
            Self::Unreviewed { after, limit } => (
                "graph.unreviewed",
                json!({"target": t, "after": after, "limit": limit}),
            ),
            Self::Proposal { proposal_id } => (
                "graph.proposal",
                json!({"target": t, "proposal_id": proposal_id}),
            ),
            Self::Acknowledge {
                event_id,
                delta_digest,
            } => (
                "graph.acknowledge",
                json!({"target": t, "event_id": event_id, "delta_digest": delta_digest}),
            ),
            Self::PreviewReversal { event_id } => (
                "graph.preview_reversal",
                json!({"target": t, "event_id": event_id}),
            ),
            Self::Reverse {
                event_id,
                approval_id,
            } => (
                "graph.reverse",
                json!({"target": t, "event_id": event_id, "approval_id": approval_id}),
            ),
        })
    }
}

/// Each `--database` with its actor and client: one value applies to every
/// database, otherwise one per database, in order. A database named twice
/// is refused: one replica per database.
fn domains(
    database: &[String],
    actor: &[String],
    client: &[String],
) -> Result<Vec<(String, String, String)>, Value> {
    let n = database.len();
    let mut seen = std::collections::BTreeSet::new();
    if let Some(repeated) = database.iter().find(|d| !seen.insert(d.as_str())) {
        return Err(fail(
            "invalid_arguments",
            &format!("--database {repeated} is given more than once"),
        ));
    }
    let pick = |values: &[String], i: usize| match values.len() {
        1 => values.first().cloned(),
        len if len == n => values.get(i).cloned(),
        _ => None,
    };
    (0..n)
        .map(|i| match (pick(actor, i), pick(client, i)) {
            (Some(actor), Some(client)) => Ok((database[i].clone(), actor, client)),
            _ => Err(fail(
                "invalid_arguments",
                "--actor and --client are given once, or once per --database",
            )),
        })
        .collect()
}

/// Open one database's replica (the composition root's part, SR-5).
fn open(
    state: &Path,
    url: &str,
    database: &str,
    actor: &str,
    client: &str,
    token: Option<&str>,
) -> Result<Source, OpError> {
    let token = token.ok_or_else(|| {
        OpError::new(
            ErrorCode::Refused,
            "unauthorised",
            "KEEPER_GRAPH_TOKEN is required",
        )
    })?;
    let unavailable = |e: &dyn std::fmt::Display| {
        OpError::new(ErrorCode::Unavailable, "storage_unavailable", e.to_string())
    };
    std::fs::create_dir_all(state).map_err(|e| unavailable(&e))?;
    let replica = SyncularReplica::open(ReplicaSpec {
        database: database.to_owned(),
        path: state.join(format!("{database}.sqlite")),
        client_id: Some(client.to_owned()),
        link: Some(Link {
            base_url: format!("{}/{database}", url.trim_end_matches('/')),
            token: token.to_owned(),
            store: Default::default(),
        }),
        units: units(database, actor, client),
        watch_presence: None,
    })
    .map_err(|e| unavailable(&e))?;
    Ok(Source {
        database: database.to_owned(),
        replica: Arc::new(replica),
        actor: actor.to_owned(),
        client: client.to_owned(),
    })
}

/// The graph module's configuration for `serve`: no worker (A-5).
pub(crate) fn serve_config(state: &Path) -> GraphConfig {
    GraphConfig {
        state: state.to_path_buf(),
        worker: None,
        macos_evidence: false,
        worker_sha256: option_env!("KEEPER_GRAPH_WORKER_SHA256"),
        ledger: embedded::LEDGER,
    }
}

/// Each database with its actor and client, and its binding.
pub(crate) type Opened = (Vec<(String, String, String)>, Vec<DomainBinding>);

/// One replica per `--database`, kept linked (the composition root's part,
/// SR-5): the verbs and `serve` open them alike.
pub(crate) fn open_domains(
    state: &Path,
    url: &str,
    database: &[String],
    actor: &[String],
    client: &[String],
) -> Result<Opened, Value> {
    let triples = domains(database, actor, client)?;
    let token = std::env::var("KEEPER_GRAPH_TOKEN").ok();
    let bindings = triples
        .iter()
        .map(|(database, actor, client)| DomainBinding {
            server: url.to_owned(),
            database: database.clone(),
            actor: actor.clone(),
            client: client.clone(),
            source: open(state, url, database, actor, client, token.as_deref()),
        })
        .collect();
    Ok((triples, bindings))
}

/// The run: open, compose, dispatch once. The answer and the dispatch count.
async fn run(args: Args) -> (Value, u64) {
    if let Err(error) = crate::state::claim(&args.state) {
        return (fail("state_not_ours", &error), 0);
    }
    let (triples, bindings) = match open_domains(
        &args.state,
        &args.url,
        &args.database,
        &args.actor,
        &args.client,
    ) {
        Ok(opened) => opened,
        Err(error) => return (error, 0),
    };
    let opened = bindings.first().is_some_and(|b| b.source.is_ok());
    let target = json!({"server": args.url, "domain": triples[0].0});
    let (id, input) = match args.command.request(target, opened) {
        Ok(request) => request,
        Err(error) => return (error, 0),
    };
    let config = GraphConfig {
        state: args.state.clone(),
        worker: args.worker.clone(),
        macos_evidence: args.macos_evidence,
        worker_sha256: option_env!("KEEPER_GRAPH_WORKER_SHA256"),
        ledger: embedded::LEDGER,
    };
    let dispatcher = match crate::compose::build(config, bindings) {
        Ok((_, dispatcher)) => dispatcher,
        Err(error) => return (fail("start_refused", &error.to_string()), 0),
    };
    let answer = render(&dispatcher.call(id, input).await);
    (answer, dispatcher.dispatches())
}

/// Run one verb, print its answer through `printer` and return its exit
/// code.
pub fn main(args: Args, printer: &Printer) -> u8 {
    let verb = args.command.name();
    let address_valid = ServerUrl::parse(&args.url).ok().flatten().is_some();
    let (answer, dispatches) = match tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
    {
        Ok(runtime) => runtime.block_on(run(args)),
        Err(e) => (fail("storage_unavailable", &e.to_string()), 0),
    };
    if std::env::var_os("KEEPER_CLI_DISPATCHES").is_some() {
        // A diagnostic, on stderr: stdout stays the answer alone.
        eprintln!("keeper-runtime dispatches: {dispatches}");
    }
    printer.answer(&answer, |answer| human::sentence(verb, answer));
    exit_code(&answer, address_valid)
}

/// The exit code an answer earns (keeper-syncd's five; the table is the
/// module doc's). `address_valid`: whether `--url` is an agents server
/// address at all. The replica refuses an invalid one first, as
/// `storage_unavailable`, and that answer stays the driver's byte for byte;
/// it is still a configuration error.
fn exit_code(answer: &Value, address_valid: bool) -> u8 {
    if answer["ok"] == true {
        return EXIT_OK;
    }
    if !address_valid {
        return EXIT_CONFIG;
    }
    let detail = answer["detail"].as_str().unwrap_or_default();
    match answer["error"].as_str().unwrap_or_default() {
        // Not a state directory of keeper-cli's own (XR-11).
        "state_not_ours" => EXIT_PREREQUISITE,
        "approval_required" => EXIT_DEFERRED,
        // Made here only, before anything is opened: no token, no address.
        "unauthorised" | "invalid_server" | "invalid_arguments" => EXIT_CONFIG,
        "projection_unavailable" => match detail.strip_prefix("projection_unavailable: ") {
            Some(reason) if missing_prerequisite(reason) => EXIT_PREREQUISITE,
            _ => EXIT_FAILURE,
        },
        "query_unsupported" => EXIT_CONFIG,
        // A payload or target this run read and refused before sending.
        _ if matches!(detail, "payload" | "target") => EXIT_CONFIG,
        // The payload file itself could not be read.
        "invalid_change" => EXIT_CONFIG,
        _ => EXIT_FAILURE,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use keeper_graph::query::QueryError;

    fn query_error(error: &QueryError) -> Value {
        fail(error.code(), &error.to_string())
    }

    /// The exit codes a wrapper branches on (keeper-syncd's five): an
    /// `ok` answer is `0` whatever the server decided; a bad invocation
    /// or input `2`; a worker this host cannot admit `3`, one admitted
    /// and then failing `1`; a reversal waiting for an approval `4`.
    #[test]
    fn each_answer_earns_its_exit_code() {
        let refused = |error: &str, detail: &str| fail(error, detail);
        let cases = [
            (
                json!({"ok": true, "outcome": {"outcome": "rejected"}}),
                EXIT_OK,
            ),
            (
                refused("unauthorised", "KEEPER_GRAPH_TOKEN is required"),
                EXIT_CONFIG,
            ),
            (
                refused("invalid_server", "--url is not an agents server address"),
                EXIT_CONFIG,
            ),
            (
                refused("invalid_change", "No such file or directory (os error 2)"),
                EXIT_CONFIG,
            ),
            (refused("unsupported_version", "payload"), EXIT_CONFIG),
            (refused("invalid_target", "target"), EXIT_CONFIG),
            (
                refused("query_unsupported", "entity is node, edge or content"),
                EXIT_CONFIG,
            ),
            (
                query_error(&QueryError::Unavailable("worker.path")),
                EXIT_PREREQUISITE,
            ),
            (
                query_error(&QueryError::Unavailable("worker.digest")),
                EXIT_PREREQUISITE,
            ),
            (
                query_error(&QueryError::Unavailable("ledger.absent")),
                EXIT_PREREQUISITE,
            ),
            (
                query_error(&QueryError::Unavailable("rootfs.sha")),
                EXIT_PREREQUISITE,
            ),
            (
                query_error(&QueryError::Unavailable("worker.exit")),
                EXIT_FAILURE,
            ),
            (
                query_error(&QueryError::Unavailable("generation.answer")),
                EXIT_FAILURE,
            ),
            (query_error(&QueryError::Forbidden), EXIT_FAILURE),
            (query_error(&QueryError::SyncPending), EXIT_FAILURE),
            (refused("refused", "Offline"), EXIT_FAILURE),
            (
                json!({"ok": false, "error": "approval_required", "preview": {}}),
                EXIT_DEFERRED,
            ),
            (
                json!({"ok": false, "error": "not_ready", "preview": {}}),
                EXIT_FAILURE,
            ),
        ];
        for (answer, code) in cases {
            assert_eq!(exit_code(&answer, true), code, "{answer}");
        }
        // An address that is none: the replica's refusal, a configuration error.
        let unaddressed = refused(
            "storage_unavailable",
            "the server's address: relative URL without a base",
        );
        assert_eq!(exit_code(&unaddressed, false), EXIT_CONFIG);
        assert_eq!(exit_code(&unaddressed, true), EXIT_FAILURE);
    }
}
