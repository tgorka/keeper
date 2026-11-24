//! keeper's module runtime: the registry, the start check, the dispatcher
//! and the `app` module's handlers.

mod app;
mod check;
mod error;
mod wire;

use std::collections::BTreeMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

use keeper_app_api::{DomainVm, ModuleState};
use keeper_runtime_api::{
    runtime_only, Caller, ErrorCode, Handler, Module, OpError, OperationSpec, Reply, StartContext,
};
use serde_json::Value;

pub use app::app_module;
pub use error::StartError;
pub use wire::WireDispatcher;

/// Static information about the running binary, answered by `app.version`
/// and `app.domains`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AppInfo {
    pub version: String,
    pub build: String,
    pub snapshot_digest: String,
    /// Domains visible to a local caller; a wire caller receives `[]`.
    pub domains: Vec<DomainVm>,
}

pub(crate) struct RuntimeInner {
    pub info: AppInfo,
    pub module_states: BTreeMap<String, ModuleState>,
    pub operations: BTreeMap<&'static str, check::CheckedEntry>,
    pub dispatches: AtomicU64,
    /// Handlers a wire caller reached (123.3-AC6's count).
    pub handler_calls: AtomicU64,
    _modules: Vec<Arc<dyn Module>>,
}

impl RuntimeInner {
    pub(crate) fn modules(&self) -> Vec<(String, ModuleState)> {
        self.module_states
            .iter()
            .map(|(id, &state)| (id.clone(), state))
            .collect()
    }
}

/// A built, started set of keeper modules.
#[derive(Clone)]
pub struct Runtime {
    inner: Arc<RuntimeInner>,
}

impl Runtime {
    /// Validate `modules`, start them in requirement order, and return the
    /// runtime handle alongside its local dispatcher.
    pub fn build(
        info: AppInfo,
        modules: Vec<Arc<dyn Module>>,
    ) -> Result<(Self, LocalDispatcher), StartError> {
        let checked = check::validate(modules)?;
        let start_context = StartContext::new(checked.present.clone());

        for (id, module) in &checked.start_order {
            if let Err(error) = module.start(&start_context) {
                return Err(StartError::StartFailed {
                    module: (*id).to_string(),
                    error,
                });
            }
        }

        let mut module_states: BTreeMap<String, ModuleState> = BTreeMap::new();
        for id in checked.present {
            module_states.insert(id.to_string(), ModuleState::Ready);
        }
        for id in checked.absent {
            module_states.insert(id, ModuleState::Absent);
        }

        let kept_modules: Vec<Arc<dyn Module>> =
            checked.start_order.into_iter().map(|(_, m)| m).collect();

        let inner = Arc::new(RuntimeInner {
            info,
            module_states,
            operations: checked.operations,
            dispatches: AtomicU64::new(0),
            handler_calls: AtomicU64::new(0),
            _modules: kept_modules,
        });

        for module in &inner._modules {
            app::attach_if_app(module, &inner);
        }

        let runtime = Self { inner };
        let dispatcher = LocalDispatcher {
            runtime: runtime.clone(),
        };
        Ok((runtime, dispatcher))
    }

    /// Every present module's operations, sorted by operation id.
    #[must_use]
    pub fn catalogue(&self) -> Vec<&'static OperationSpec> {
        self.inner
            .operations
            .values()
            .map(|entry| entry.spec)
            .collect()
    }

    /// Present modules (`Ready`) and optional-required-but-absent modules
    /// (`Absent`), sorted by module id.
    #[must_use]
    pub fn modules(&self) -> Vec<(String, ModuleState)> {
        self.inner.modules()
    }

    /// The dispatcher a wire transport is handed: `Caller::Wire`, and only
    /// what a wire caller may reach (story 123.3).
    #[must_use]
    pub fn wire_dispatcher(&self) -> WireDispatcher {
        WireDispatcher::new(self.clone())
    }

    /// How many handlers a wire caller has reached so far.
    #[must_use]
    pub fn wire_handler_calls(&self) -> u64 {
        self.inner.handler_calls.load(Ordering::Relaxed)
    }
}

/// In-process dispatcher minting `Caller::Local` and class witnesses.
#[derive(Clone)]
pub struct LocalDispatcher {
    runtime: Runtime,
}

impl LocalDispatcher {
    /// Dispatch an operation by id as a local caller.
    pub async fn call(&self, id: &str, input: Value) -> Reply {
        self.runtime
            .inner
            .dispatches
            .fetch_add(1, Ordering::Relaxed);

        let Some(entry) = self.runtime.inner.operations.get(id) else {
            return Err(OpError::new(ErrorCode::NotFound, "unknown_operation", id));
        };

        let local = runtime_only::local();
        let caller = Caller::Local(local.clone());
        match &entry.handler {
            Handler::Read(handler) => {
                handler(runtime_only::context(caller, runtime_only::read()), input).await
            }
            Handler::Write(handler) => {
                handler(runtime_only::context(caller, runtime_only::write()), input).await
            }
            Handler::Decide(handler) => {
                let witness = runtime_only::decide(&local);
                handler(runtime_only::context(caller, witness), input).await
            }
            Handler::Secret(handler) => {
                let witness = runtime_only::secret(&local);
                handler(runtime_only::context(caller, witness), input).await
            }
        }
    }

    /// Number of calls dispatched so far.
    #[must_use]
    pub fn dispatches(&self) -> u64 {
        self.runtime.inner.dispatches.load(Ordering::Relaxed)
    }

    /// The underlying runtime handle.
    #[must_use]
    pub fn runtime(&self) -> &Runtime {
        &self.runtime
    }
}
