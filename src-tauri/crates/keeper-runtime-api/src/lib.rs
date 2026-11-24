//! keeper's module catalogue contract (MA/AD-1, MA/AD-2; story 123.2).
//!
//! Every operation a module offers is declared once, as data — an
//! [`OperationSpec`] — with its effect class and the rules a caller on the
//! wire cannot escape (DR-6). A module is a [`Module`]: its [`Manifest`]
//! (its id, what it requires, its catalogue) and its [`Bindings`], one
//! handler per operation. `keeper-runtime` checks the list a binary composes
//! when it starts and reaches every operation through its dispatcher, by id.
//!
//! **The class witness** (DR-9 b; MA-G14). A handler is called with a
//! [`CallContext`] carrying a witness of its operation's declared class
//! ([`Read`], [`Write`], [`Decide`], [`Secret`]). A module's facade asks
//! for the witness its methods need, so a `read` handler holds nothing with
//! which to call a mutating method. Only `keeper-runtime` mints witnesses,
//! [`CallContext`]s and [`Caller::Local`]: their fields are private, and the
//! minting functions (`runtime_only`) exist only under this crate's
//! `runtime` feature, which `keeper-runtime` alone enables
//! (`scripts/check-graph-boundary.py`).
//!
//! A contract (MA-G1): no `tokio`, no storage, no transport.

mod call;
mod error;
mod module;
mod schema;
mod spec;

pub use call::{CallContext, Caller, Decide, Door, Local, Read, Secret, Witness, Write};
pub use error::{ErrorCode, OpError};
pub use module::{
    Bindings, BoxFuture, BoxStream, Handler, HandlerFn, Manifest, Module, Reply, Requirement,
    StartContext,
};
pub use schema::{enums, has_pointer, snapshot_entry};
pub use spec::{
    CliHint, EdgeFacts, Effect, Facts, Fixed, Gate, GateFn, Kind, OperationSpec, PageSpec,
    ProposalForm, Scope, ToProposalFn,
};

/// The JSON Schema type every operation's input and output is described
/// with (JSON Schema 2020-12, `schemars` 1.x).
pub use schemars::Schema;

/// Minting, for `keeper-runtime` only (MA-G9, MA-G14).
#[cfg(feature = "runtime")]
pub mod runtime_only {
    pub use crate::call::mint::*;
}
