//! A module: its manifest and its handlers (MA/AD-2).

use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

use serde::de::DeserializeOwned;
use serde::Serialize;
use serde_json::Value;

use crate::{
    CallContext, Decide, Effect, ErrorCode, OpError, OperationSpec, Read, Secret, Witness, Write,
};

/// A boxed `std` future, as every handler returns.
pub type BoxFuture<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;
/// A boxed stream, for `Stream` operations.
pub type BoxStream<'a, T> = Pin<Box<dyn futures_core::Stream<Item = T> + Send + 'a>>;
/// An operation's answer.
pub type Reply = Result<Value, OpError>;
/// A handler of class `W`, over the already-checked JSON input.
pub type HandlerFn<W> =
    Arc<dyn Fn(CallContext<W>, Value) -> BoxFuture<'static, Reply> + Send + Sync>;

/// One handler, tagged with the class whose witness it is given.
#[derive(Clone)]
pub enum Handler {
    Read(HandlerFn<Read>),
    Write(HandlerFn<Write>),
    Decide(HandlerFn<Decide>),
    Secret(HandlerFn<Secret>),
}

impl Handler {
    #[must_use]
    pub fn effect(&self) -> Effect {
        match self {
            Self::Read(_) => Effect::Read,
            Self::Write(_) => Effect::Write,
            Self::Decide(_) => Effect::Decide,
            Self::Secret(_) => Effect::Secret,
        }
    }
}

/// A module another one needs.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Requirement {
    pub module: &'static str,
    /// An optional requirement that is absent is recorded, not refused.
    pub optional: bool,
}

/// What a module declares about itself.
#[derive(Clone, Copy, Debug)]
pub struct Manifest {
    /// The module's id: every operation id is `<id>.<verb>`.
    pub id: &'static str,
    pub requires: &'static [Requirement],
    pub operations: &'static [OperationSpec],
}

/// What a module learns when it starts.
#[derive(Clone, Debug, Default)]
pub struct StartContext {
    present: Vec<&'static str>,
}

impl StartContext {
    #[must_use]
    pub fn new(present: Vec<&'static str>) -> Self {
        Self { present }
    }

    /// Whether `module` is in this build.
    #[must_use]
    pub fn has(&self, module: &str) -> bool {
        self.present.contains(&module)
    }
}

/// A module of keeper, as a binary composes it.
pub trait Module: Send + Sync {
    fn manifest(&self) -> Manifest;
    /// Bind one handler to each of the manifest's operations.
    fn bind(&self, bindings: &mut Bindings);
    /// Called once, in requirement order, after the start check passed.
    fn start(&self, _: &StartContext) -> Result<(), OpError> {
        Ok(())
    }
    /// Why operation `id` is not available in this build or on this host,
    /// or `None` when it is (`app.capabilities`). An operation is present
    /// and unavailable, never silently absent.
    fn unavailable(&self, _id: &str) -> Option<String> {
        None
    }
}

/// The handlers a module binds, by operation id.
#[derive(Default)]
pub struct Bindings {
    entries: Vec<(&'static str, Handler)>,
}

/// Decode `input` as `I` (a type that denies unknown fields refuses them
/// here), naming the field that fails.
fn decode<I: DeserializeOwned>(input: Value) -> Result<I, OpError> {
    serde_path_to_error::deserialize(input).map_err(|error| {
        let path = error.path().to_string();
        OpError::invalid_argument(path, error.into_inner().to_string())
    })
}

fn typed<W, I, O, F, Fut>(f: F) -> HandlerFn<W>
where
    W: Witness,
    I: DeserializeOwned + Send + 'static,
    O: Serialize + 'static,
    F: Fn(CallContext<W>, I) -> Fut + Send + Sync + 'static,
    Fut: Future<Output = Result<O, OpError>> + Send + 'static,
{
    Arc::new(move |context, input| match decode::<I>(input) {
        Err(error) => Box::pin(async move { Err(error) }),
        Ok(input) => {
            let answer = f(context, input);
            Box::pin(async move {
                let output = answer.await?;
                serde_json::to_value(output)
                    .map_err(|e| OpError::new(ErrorCode::Internal, "output", e.to_string()))
            })
        }
    })
}

impl Bindings {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Bind a ready-made handler.
    pub fn bind(&mut self, id: &'static str, handler: Handler) {
        self.entries.push((id, handler));
    }

    /// Bind a `read` handler; its input is decoded as `I` before it runs.
    pub fn read<I, O, F, Fut>(&mut self, id: &'static str, f: F)
    where
        I: DeserializeOwned + Send + 'static,
        O: Serialize + 'static,
        F: Fn(CallContext<Read>, I) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = Result<O, OpError>> + Send + 'static,
    {
        self.bind(id, Handler::Read(typed(f)));
    }

    /// Bind a `write` handler.
    pub fn write<I, O, F, Fut>(&mut self, id: &'static str, f: F)
    where
        I: DeserializeOwned + Send + 'static,
        O: Serialize + 'static,
        F: Fn(CallContext<Write>, I) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = Result<O, OpError>> + Send + 'static,
    {
        self.bind(id, Handler::Write(typed(f)));
    }

    /// Bind a `decide` handler.
    pub fn decide<I, O, F, Fut>(&mut self, id: &'static str, f: F)
    where
        I: DeserializeOwned + Send + 'static,
        O: Serialize + 'static,
        F: Fn(CallContext<Decide>, I) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = Result<O, OpError>> + Send + 'static,
    {
        self.bind(id, Handler::Decide(typed(f)));
    }

    /// Bind a `secret` handler.
    pub fn secret<I, O, F, Fut>(&mut self, id: &'static str, f: F)
    where
        I: DeserializeOwned + Send + 'static,
        O: Serialize + 'static,
        F: Fn(CallContext<Secret>, I) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = Result<O, OpError>> + Send + 'static,
    {
        self.bind(id, Handler::Secret(typed(f)));
    }

    #[must_use]
    pub fn into_entries(self) -> Vec<(&'static str, Handler)> {
        self.entries
    }
}
