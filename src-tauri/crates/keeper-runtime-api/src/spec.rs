//! An operation, declared once as data (FR-E123-1; MA/AD-1, DR-6).

use serde_json::Value;

use crate::OpError;

/// How an operation answers.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Kind {
    /// One answer, nothing changed.
    Query,
    /// One answer, something may change.
    Command,
    /// A stream of answers.
    Stream,
}

/// What an operation may do (the architecture's *effect class*). There is
/// no default: every operation names its own.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Effect {
    /// Changes nothing outside keeper's own local caches.
    Read,
    /// Changes the person's data in a way the person could make, review or reverse.
    Write,
    /// A person's judgement on a change or a request. Never reachable from the wire.
    Decide,
    /// Reveals or handles a secret. Never reachable from the wire.
    Secret,
}

/// Whose data an operation addresses.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Scope {
    /// keeper itself: no domain.
    App,
    /// One domain: the JSON pointer, in the input, of the object holding the
    /// `server` and `domain` pair (e.g. `/target`).
    Domain(&'static str),
}

/// How a `write` the wire may not make directly becomes a proposal.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ProposalForm {
    /// The operation is itself the proposal.
    Itself,
    /// Through the named `write` command of the same module, by `to_proposal`.
    Via(&'static str),
    /// It has none: a gate's `ProposeOnly` refuses.
    None,
}

/// A value the dispatcher writes into a wire caller's input, whatever the
/// caller sent (`wire_fix`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Fixed {
    /// The device's own actor id, as a one-element list.
    DeviceActor,
    /// JSON `null`.
    Null,
    /// A fresh ULID.
    FreshUlid,
    /// Now plus this many milliseconds.
    ExpiresIn(u64),
}

/// A wire gate's answer over an input and the replica's [`Facts`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Gate {
    Pass,
    Refuse(String),
    ProposeOnly(String),
}

/// An edge as the replica knows it (CR-4).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EdgeFacts {
    pub edge_type: String,
    /// The kind of each end, when the replica knows that node.
    pub from_kind: Option<String>,
    pub to_kind: Option<String>,
}

/// What a wire gate may ask of the replica: a node's kind, an edge's type
/// and the kinds of its ends. `None`: not known here.
pub trait Facts {
    fn node_kind(&self, id: &str) -> Option<String>;
    fn edge(&self, id: &str) -> Option<EdgeFacts>;
}

/// `to_proposal`: a pure function from a `Via` operation's input to its
/// proposal operation's input, before the dispatcher's fixed fields.
pub type ToProposalFn = fn(&Value) -> Result<Value, OpError>;
/// `wire_gate`: a pure gate over the input and the replica's facts.
pub type GateFn = fn(&Value, &dyn Facts) -> Gate;

/// How a listing pages: JSON pointers into the input and the output.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PageSpec {
    /// The input's cursor.
    pub cursor: &'static str,
    /// The input's page size.
    pub limit: &'static str,
}

/// How the command line spells an operation.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CliHint {
    /// The words after `keeper-cli`, e.g. `["graph", "preview-reversal"]`.
    pub words: &'static [&'static str],
    /// The input pointers given as positional arguments, in order.
    pub positional: &'static [&'static str],
}

/// One operation (FR-E123-1).
#[derive(Clone, Copy, Debug)]
pub struct OperationSpec {
    /// `<module>.<verb>`.
    pub id: &'static str,
    pub kind: Kind,
    pub effect: Effect,
    pub summary: &'static str,
    pub scope: Scope,
    pub proposal: ProposalForm,
    /// The input's JSON Schema (2020-12).
    pub input: fn() -> schemars::Schema,
    /// The output's JSON Schema (2020-12).
    pub output: fn() -> schemars::Schema,
    /// Output pointers a concise rendering keeps.
    pub concise: &'static [&'static str],
    pub page: Option<PageSpec>,
    pub cli: CliHint,
    /// Input pointers a wire caller may not set.
    pub wire_forbid: &'static [&'static str],
    /// Input pointers the dispatcher sets for a wire caller.
    pub wire_fix: &'static [(&'static str, Fixed)],
    pub to_proposal: Option<ToProposalFn>,
    pub wire_gate: Option<GateFn>,
    /// Repository-root paths of the vectors files `to_proposal` and
    /// `wire_gate` are run over (CR-5); the snapshot carries their digests.
    pub vectors: &'static [&'static str],
}

impl OperationSpec {
    /// The module part of the id (before the first `.`).
    #[must_use]
    pub fn module(&self) -> &'static str {
        self.id
            .split_once('.')
            .map_or(self.id, |(module, _)| module)
    }
}

impl Kind {
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Query => "query",
            Self::Command => "command",
            Self::Stream => "stream",
        }
    }
}

impl Effect {
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Read => "read",
            Self::Write => "write",
            Self::Decide => "decide",
            Self::Secret => "secret",
        }
    }
}
