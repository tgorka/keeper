//! The built-in `app` module: answers from the runtime's own state.

use std::collections::HashMap;
use std::sync::{Arc, LazyLock, Mutex, OnceLock, Weak};

use keeper_app_api::{
    AppCapabilitiesVm, DomainsVm, HealthState, HealthVm, ModuleStatusVm, ModulesVm, NoInput,
    OperationAvailabilityVm, VersionVm, MODULE, OPERATIONS,
};
use keeper_runtime_api::{Bindings, ErrorCode, Manifest, Module, OpError};

use crate::RuntimeInner;

type StateSlot = Arc<OnceLock<Weak<RuntimeInner>>>;

/// The state slot of every live `app` module, by its address: the runtime
/// that composes one fills its slot (`attach_if_app`).
type Slots = Mutex<HashMap<usize, Weak<OnceLock<Weak<RuntimeInner>>>>>;

static APP_SLOTS: LazyLock<Slots> = LazyLock::new(|| Mutex::new(HashMap::new()));

struct AppModule {
    state: StateSlot,
}

impl Drop for AppModule {
    fn drop(&mut self) {
        let key = std::ptr::from_ref(self) as usize;
        if let Ok(mut map) = APP_SLOTS.lock() {
            map.remove(&key);
        }
    }
}

/// Create the `app` module; its handlers read the built runtime's state.
#[must_use]
pub fn app_module() -> Arc<dyn Module> {
    let state: StateSlot = Arc::new(OnceLock::new());
    let module = Arc::new(AppModule {
        state: Arc::clone(&state),
    });
    let key = Arc::as_ptr(&module) as usize;
    if let Ok(mut map) = APP_SLOTS.lock() {
        map.insert(key, Arc::downgrade(&state));
    }
    module
}

/// Attach `inner` to `module` when it is an `app` module made by [`app_module`].
pub(crate) fn attach_if_app(module: &Arc<dyn Module>, inner: &Arc<RuntimeInner>) {
    let key = Arc::as_ptr(module).cast::<()>() as usize;
    let slot = APP_SLOTS
        .lock()
        .ok()
        .and_then(|map| map.get(&key).and_then(Weak::upgrade));
    if let Some(slot) = slot {
        let _ = slot.set(Arc::downgrade(inner));
    }
}

fn runtime_state(slot: &StateSlot) -> Result<Arc<RuntimeInner>, OpError> {
    slot.get().and_then(Weak::upgrade).ok_or_else(|| {
        OpError::new(
            ErrorCode::Internal,
            "runtime_unavailable",
            "runtime is not initialized",
        )
    })
}

impl Module for AppModule {
    fn manifest(&self) -> Manifest {
        Manifest {
            id: MODULE,
            requires: &[],
            operations: OPERATIONS,
        }
    }

    fn bind(&self, bindings: &mut Bindings) {
        let state = Arc::clone(&self.state);
        bindings.read("app.version", move |_ctx, _: NoInput| {
            let state = Arc::clone(&state);
            async move {
                let inner = runtime_state(&state)?;
                Ok(VersionVm {
                    version: inner.info.version.clone(),
                    build: inner.info.build.clone(),
                    snapshot_digest: inner.info.snapshot_digest.clone(),
                })
            }
        });

        let state = Arc::clone(&self.state);
        bindings.read("app.modules", move |_ctx, _: NoInput| {
            let state = Arc::clone(&state);
            async move {
                let inner = runtime_state(&state)?;
                let modules = inner
                    .modules()
                    .into_iter()
                    .map(|(id, state)| ModuleStatusVm { id, state })
                    .collect();
                Ok(ModulesVm { modules })
            }
        });

        let state = Arc::clone(&self.state);
        bindings.read("app.health", move |ctx, _: NoInput| {
            let state = Arc::clone(&state);
            let is_local = ctx.caller().is_local();
            async move {
                let _inner = runtime_state(&state)?;
                Ok(HealthVm {
                    state: HealthState::Serving,
                    sentence: is_local.then(|| "keeper is serving".to_string()),
                })
            }
        });

        let state = Arc::clone(&self.state);
        bindings.read("app.capabilities", move |_ctx, _: NoInput| {
            let state = Arc::clone(&state);
            async move {
                let inner = runtime_state(&state)?;
                let operations = inner
                    .operations
                    .values()
                    .map(|entry| {
                        let reason = entry.module.unavailable(entry.spec.id);
                        OperationAvailabilityVm {
                            id: entry.spec.id.to_string(),
                            available: reason.is_none(),
                            reason,
                        }
                    })
                    .collect();
                Ok(AppCapabilitiesVm {
                    platform: std::env::consts::OS.to_string(),
                    operations,
                })
            }
        });

        let state = Arc::clone(&self.state);
        bindings.read("app.domains", move |ctx, _: NoInput| {
            let state = Arc::clone(&state);
            let is_local = ctx.caller().is_local();
            async move {
                let inner = runtime_state(&state)?;
                let domains = if is_local {
                    inner.info.domains.clone()
                } else {
                    Vec::new()
                };
                Ok(DomainsVm { domains })
            }
        });
    }
}
