//! What the server dispatches through: the dispatcher's steps (MA/AD-5) as
//! the envelope sees them. keeper-runtime's [`WireDispatcher`] is the one a
//! binary hands over; tests hand fixture operations.

use std::future::Future;
use std::pin::Pin;

use keeper_runtime::WireDispatcher;
use keeper_runtime_api::{Door, Effect, ErrorCode, Kind, OpError, Reply};
use keeper_wire_api::{Detail, OperationEntry};
use serde_json::Value;

/// A boxed future.
pub type BoxFuture<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;
/// A stream's items.
pub type Stream = Pin<Box<dyn futures_core::Stream<Item = Reply> + Send + 'static>>;

/// The dispatcher a wire caller reaches.
pub trait Dispatch: Send + Sync + 'static {
    /// `app_version` of the server's `hello` and of `describe`.
    fn app_version(&self) -> String;
    /// The catalogue digest of the server's `hello` and of `describe`.
    fn catalogue_digest(&self) -> String;
    /// The kind of `operation` when this caller may reach it; `None` is
    /// `NOT_FOUND`, the same answer as an id that does not exist.
    fn kind(&self, door: Door, operation: &str) -> Option<Kind>;
    /// A query or a command. Called only for an operation [`Self::kind`]
    /// admitted, and for a command only once the start was decided.
    fn call(&self, door: Door, operation: &str, input: Value) -> BoxFuture<'static, Reply>;
    /// A stream.
    fn watch(&self, door: Door, operation: &str, input: Value) -> Stream;
    /// What this caller can see through `door`, sorted by id.
    fn describe(&self, door: Door, detail: Detail) -> Vec<OperationEntry>;
}

impl Dispatch for WireDispatcher {
    fn app_version(&self) -> String {
        self.info().version.clone()
    }

    fn catalogue_digest(&self) -> String {
        self.info().snapshot_digest.clone()
    }

    fn kind(&self, _door: Door, operation: &str) -> Option<Kind> {
        self.reachable(operation).map(|spec| spec.kind)
    }

    fn call(&self, door: Door, operation: &str, input: Value) -> BoxFuture<'static, Reply> {
        let dispatcher = self.clone();
        let operation = operation.to_owned();
        Box::pin(async move { dispatcher.call(door, &operation, input).await })
    }

    fn watch(&self, _door: Door, operation: &str, _input: Value) -> Stream {
        // No stream is reachable before 123.4.
        let error = OpError::new(ErrorCode::NotFound, "unknown_operation", operation);
        Box::pin(Once(Some(Err(error))))
    }

    fn describe(&self, _door: Door, detail: Detail) -> Vec<OperationEntry> {
        let schema = |f: fn() -> keeper_runtime_api::Schema| {
            (detail == Detail::Full).then(|| f().to_value())
        };
        self.reachable_operations()
            .into_iter()
            .map(|spec| OperationEntry {
                id: spec.id.to_owned(),
                kind: spec.kind.as_str().to_owned(),
                effect: spec.effect.as_str().to_owned(),
                summary: spec.summary.to_owned(),
                input_schema: schema(spec.input),
                output_schema: schema(spec.output),
                // Before 123.4 only reads are reachable.
                write_as: (spec.effect == Effect::Write).then(|| "refused".to_owned()),
            })
            .collect()
    }
}

/// A stream of one item.
struct Once(Option<Reply>);

impl futures_core::Stream for Once {
    type Item = Reply;

    fn poll_next(
        mut self: Pin<&mut Self>,
        _cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Option<Reply>> {
        std::task::Poll::Ready(self.0.take())
    }
}
