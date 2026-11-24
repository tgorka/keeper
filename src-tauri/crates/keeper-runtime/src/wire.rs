//! The dispatcher a wire transport is handed (story 123.3; MA/AD-5 before
//! 123.4's rule): it calls as `Caller::Wire { door }` and admits only the
//! `app` module's reads. Every other operation answers `NOT_FOUND`, the same
//! answer as an id that does not exist, and its handler is never called.

use std::sync::atomic::Ordering;

use keeper_runtime_api::{
    runtime_only, Caller, Door, Effect, ErrorCode, Handler, Kind, OpError, OperationSpec, Reply,
};
use serde_json::Value;

use crate::{AppInfo, Runtime};

/// The one module a wire caller reaches before 123.4.
const REACHABLE_MODULE: &str = "app";

/// The wire's view of a runtime.
#[derive(Clone)]
pub struct WireDispatcher {
    runtime: Runtime,
}

impl WireDispatcher {
    pub(crate) fn new(runtime: Runtime) -> Self {
        Self { runtime }
    }

    /// The running binary's version and catalogue digest.
    #[must_use]
    pub fn info(&self) -> &AppInfo {
        &self.runtime.inner.info
    }

    /// `id`'s declaration when a wire caller may reach it, else `None`.
    #[must_use]
    pub fn reachable(&self, id: &str) -> Option<&'static OperationSpec> {
        let entry = self.runtime.inner.operations.get(id)?;
        let spec = entry.spec;
        (spec.module() == REACHABLE_MODULE
            && spec.effect == Effect::Read
            && spec.kind == Kind::Query
            && matches!(entry.handler, Handler::Read(_)))
        .then_some(spec)
    }

    /// Everything a wire caller may reach, sorted by id (`describe`).
    #[must_use]
    pub fn reachable_operations(&self) -> Vec<&'static OperationSpec> {
        self.runtime
            .inner
            .operations
            .keys()
            .filter_map(|id| self.reachable(id))
            .collect()
    }

    /// Dispatch `id` for a wire caller through `door`.
    pub async fn call(&self, door: Door, id: &str, input: Value) -> Reply {
        let entry = match self.reachable(id) {
            Some(_) => self.runtime.inner.operations.get(id),
            None => None,
        };
        let Some(entry) = entry else {
            return Err(OpError::new(ErrorCode::NotFound, "unknown_operation", id));
        };
        let Handler::Read(handler) = &entry.handler else {
            return Err(OpError::new(ErrorCode::NotFound, "unknown_operation", id));
        };
        self.runtime
            .inner
            .handler_calls
            .fetch_add(1, Ordering::Relaxed);
        let caller = Caller::Wire { door };
        handler(runtime_only::context(caller, runtime_only::read()), input).await
    }
}
