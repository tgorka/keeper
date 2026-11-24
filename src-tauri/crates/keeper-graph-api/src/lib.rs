//! The graph module's catalogue (story 123.2; A-4): one operation per verb
//! of `keeper-cli graph`, 113.5's seventeen, each naming the
//! `(server, domain)` pair it addresses at `/target`.
//!
//! A crate of its own, not a feature of `keeper-graph-protocol`: cargo
//! unifies features across one build, so a feature there would change the
//! graph worker's digest. keeper-hq does not take this crate.
//!
//! The change, proposal and decide payloads are read by the protocol's
//! hand-written rules, so their schemas are written by hand ([`schemas`])
//! and tested against `syncular/knowledge-graph.vectors.json`. The wire
//! rules — what a wire caller may not set, what the dispatcher sets for it,
//! `to_proposal` and the gate — are data here (DR-6); no handler enforces
//! authority. They run over the committed vectors files under
//! `catalogue/vectors/` (CR-5).

mod gate;
mod inputs;
pub mod schemas;

pub use gate::{propose_gate, to_proposal, wire_gate};
pub use inputs::{
    Acknowledge, Discard, Entity, EventId, Inspect, Page, ProposalRead, Query, Reverse, Sync,
    Target, TargetOnly, Targeted,
};

use keeper_runtime_api::{
    CliHint, Effect, Fixed, Kind, OperationSpec, PageSpec, ProposalForm, Schema, Scope,
};

/// The module's id.
pub const MODULE: &str = "graph";

/// Seven days, the proposal expiry the dispatcher sets for a wire caller.
pub const WIRE_PROPOSAL_TTL_MS: u64 = 7 * 24 * 60 * 60 * 1000;

fn schema_of(value: serde_json::Value) -> Schema {
    // The hand-written schemas are objects by construction.
    Schema::try_from(value).unwrap_or_default()
}

fn change_input() -> Schema {
    schema_of(schemas::change_input())
}
fn propose_input() -> Schema {
    schema_of(schemas::propose_input())
}
fn decide_input() -> Schema {
    schema_of(schemas::decide_input())
}
fn queued() -> Schema {
    schema_of(schemas::queued_answer())
}
fn entries() -> Schema {
    schema_of(schemas::entries_answer())
}
fn sent() -> Schema {
    schema_of(schemas::sent_answer())
}
fn reversed() -> Schema {
    schema_of(schemas::reverse_answer())
}
fn synced() -> Schema {
    schema_of(schemas::sync_answer())
}
fn queried() -> Schema {
    schema_of(schemas::query_answer())
}
fn inspected() -> Schema {
    schema_of(schemas::inspect_answer())
}
fn paged() -> Schema {
    schema_of(schemas::page_answer())
}
fn revisions() -> Schema {
    schema_of(schemas::revisions_answer())
}
fn previewed() -> Schema {
    schema_of(schemas::preview_answer())
}
fn status() -> Schema {
    schema_of(schemas::status_answer())
}
fn derived<T: schemars::JsonSchema>() -> Schema {
    schemars::schema_for!(T)
}

const APPLY_FORBID: &[&str] = &["/approval_id", "/event_time_ms"];

/// An operation's input and output schemas.
type Schemas = (fn() -> Schema, fn() -> Schema);

/// An operation with no wire rules; the table below fills in the rest.
const fn base(
    id: &'static str,
    kind: Kind,
    effect: Effect,
    summary: &'static str,
    (input, output): Schemas,
    words: &'static [&'static str],
    positional: &'static [&'static str],
) -> OperationSpec {
    OperationSpec {
        id,
        kind,
        effect,
        summary,
        scope: Scope::Domain("/target"),
        proposal: ProposalForm::None,
        input,
        output,
        concise: &["/ok"],
        page: None,
        cli: CliHint { words, positional },
        wire_forbid: &[],
        wire_fix: &[],
        to_proposal: None,
        wire_gate: None,
        vectors: &[],
    }
}

const fn read(
    id: &'static str,
    summary: &'static str,
    schemas: Schemas,
    words: &'static [&'static str],
    positional: &'static [&'static str],
) -> OperationSpec {
    base(
        id,
        Kind::Query,
        Effect::Read,
        summary,
        schemas,
        words,
        positional,
    )
}

const fn command(
    id: &'static str,
    effect: Effect,
    summary: &'static str,
    schemas: Schemas,
    words: &'static [&'static str],
    positional: &'static [&'static str],
) -> OperationSpec {
    base(
        id,
        Kind::Command,
        effect,
        summary,
        schemas,
        words,
        positional,
    )
}

const PAGE: Option<PageSpec> = Some(PageSpec {
    cursor: "/after",
    limit: "/limit",
});

/// The graph's catalogue: the epic's table, carried whole.
pub static OPERATIONS: &[OperationSpec] = &[
    OperationSpec {
        proposal: ProposalForm::Via("graph.propose"),
        wire_forbid: APPLY_FORBID,
        to_proposal: Some(to_proposal),
        wire_gate: Some(wire_gate),
        vectors: &[
            "catalogue/vectors/graph.apply.to_proposal.json",
            "catalogue/vectors/graph.apply.wire_gate.json",
        ],
        ..command(
            "graph.apply",
            Effect::Write,
            "send a change to the graph and print its outcome (online only)",
            (change_input, sent),
            &["graph", "apply"],
            &[],
        )
    },
    OperationSpec {
        wire_forbid: APPLY_FORBID,
        wire_gate: Some(wire_gate),
        vectors: &["catalogue/vectors/graph.enqueue.wire_gate.json"],
        ..command(
            "graph.enqueue",
            Effect::Write,
            "persist a change in the intent queue, offline too; nothing is sent",
            (change_input, queued),
            &["graph", "enqueue"],
            &[],
        )
    },
    read(
        "graph.pending",
        "the queue's entries: ids, state, cause and age only",
        (derived::<TargetOnly>, entries),
        &["graph", "pending"],
        &[],
    ),
    command(
        "graph.resume",
        Effect::Write,
        "resolve every open queue entry from its receipt; send it only when none exists",
        (derived::<TargetOnly>, entries),
        &["graph", "resume"],
        &[],
    ),
    command(
        "graph.discard",
        Effect::Decide,
        "withdraw a queue entry never sent, suspended or locked",
        (derived::<Discard>, queued),
        &["graph", "discard"],
        &["/request_id"],
    ),
    command(
        "graph.sync",
        Effect::Write,
        "wait for the domain's graph, then build or extend the query projection",
        (derived::<Sync>, synced),
        &["graph", "sync"],
        &[],
    ),
    read(
        "graph.query",
        "one KG-Cypher/1 query over the projection",
        (derived::<Query>, queried),
        &["graph", "query"],
        &["/source"],
    ),
    read(
        "graph.inspect",
        "look up one node, edge or content",
        (derived::<Inspect>, inspected),
        &["graph", "inspect"],
        &["/entity", "/id"],
    ),
    OperationSpec {
        page: PAGE,
        ..read(
            "graph.history",
            "one page of retained history",
            (derived::<Page>, paged),
            &["graph", "history"],
            &[],
        )
    },
    OperationSpec {
        page: PAGE,
        ..read(
            "graph.unreviewed",
            "one page of the unreviewed feed (a reviewer's)",
            (derived::<Page>, paged),
            &["graph", "unreviewed"],
            &[],
        )
    },
    OperationSpec {
        proposal: ProposalForm::Itself,
        wire_forbid: &[
            "/reviewers",
            "/base_revision",
            "/expires_at_ms",
            "/proposal_id",
            "/change/approval_id",
            "/change/event_time_ms",
        ],
        wire_fix: &[
            ("/reviewers", Fixed::DeviceActor),
            ("/base_revision", Fixed::Null),
            ("/expires_at_ms", Fixed::ExpiresIn(WIRE_PROPOSAL_TTL_MS)),
            ("/proposal_id", Fixed::FreshUlid),
        ],
        wire_gate: Some(propose_gate),
        vectors: &["catalogue/vectors/graph.propose.wire_gate.json"],
        ..command(
            "graph.propose",
            Effect::Write,
            "send a proposal and print its outcome",
            (propose_input, sent),
            &["graph", "propose"],
            &[],
        )
    },
    command(
        "graph.decide",
        Effect::Decide,
        "approve or reject one proposal revision and print the outcome",
        (decide_input, sent),
        &["graph", "decide"],
        &[],
    ),
    read(
        "graph.proposal",
        "a proposal's revisions, as its enumerated reviewer reads them",
        (derived::<ProposalRead>, revisions),
        &["graph", "proposal"],
        &["/proposal_id"],
    ),
    command(
        "graph.acknowledge",
        Effect::Decide,
        "a human review of one event",
        (derived::<Acknowledge>, sent),
        &["graph", "acknowledge"],
        &["/event_id", "/delta_digest"],
    ),
    read(
        "graph.preview_reversal",
        "the inverse of an event and what it requires now",
        (derived::<EventId>, previewed),
        &["graph", "preview-reversal"],
        &["/event_id"],
    ),
    command(
        "graph.reverse",
        Effect::Decide,
        "preview, then reverse under exactly the previewed revisions",
        (derived::<Reverse>, reversed),
        &["graph", "reverse"],
        &["/event_id"],
    ),
    read(
        "graph.status",
        "the replica's head and checkpoint, the runtime admission, the fence and the queue's counts",
        (derived::<TargetOnly>, status),
        &["graph", "status"],
        &[],
    ),
];
