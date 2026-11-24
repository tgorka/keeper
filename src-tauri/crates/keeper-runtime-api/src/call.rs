//! Who calls, and the class witness a handler is given (DR-9 b; MA-G9,
//! MA-G14).

use crate::Effect;

mod sealed {
    pub trait Sealed {}
}

/// A witness of an operation's declared class. Implemented by the four
/// witnesses below and nothing else.
pub trait Witness: sealed::Sealed + Send + Sync + 'static {
    const EFFECT: Effect;
}

/// The `read` witness: a read facade's methods may take it; nothing mutates with it.
#[derive(Debug)]
pub struct Read(());
/// The `write` witness: a module's mutating facade methods take `&Write`.
#[derive(Debug)]
pub struct Write(());
/// The `decide` witness: minted for [`Caller::Local`] only.
#[derive(Debug)]
pub struct Decide(());
/// The `secret` witness: minted for [`Caller::Local`] only.
#[derive(Debug)]
pub struct Secret(());

impl sealed::Sealed for Read {}
impl sealed::Sealed for Write {}
impl sealed::Sealed for Decide {}
impl sealed::Sealed for Secret {}
impl Witness for Read {
    const EFFECT: Effect = Effect::Read;
}
impl Witness for Write {
    const EFFECT: Effect = Effect::Write;
}
impl Witness for Decide {
    const EFFECT: Effect = Effect::Decide;
}
impl Witness for Secret {
    const EFFECT: Effect = Effect::Secret;
}

/// The proof that a call comes from inside the binary: only
/// `keeper-runtime` makes one, for the dispatcher it hands a composition
/// root.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Local(());

/// The door a wire caller came through.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Door {
    Cli,
    Mcp,
}

/// Who calls.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Caller {
    /// The binary itself: the person at the app, or the command line's own verbs.
    Local(Local),
    /// A process on the socket (123.3), through a door.
    Wire { door: Door },
}

impl Caller {
    #[must_use]
    pub fn is_local(&self) -> bool {
        matches!(self, Self::Local(_))
    }
}

/// What a handler is called with: the caller and the witness of its
/// operation's class. Only `keeper-runtime` makes one.
#[derive(Debug)]
pub struct CallContext<W: Witness> {
    caller: Caller,
    witness: W,
}

impl<W: Witness> CallContext<W> {
    #[must_use]
    pub fn caller(&self) -> &Caller {
        &self.caller
    }

    /// The class witness, for the module's facade.
    #[must_use]
    pub fn witness(&self) -> &W {
        &self.witness
    }
}

#[cfg(feature = "runtime")]
pub(crate) mod mint {
    //! The only way to make a caller, a witness or a call context.
    use super::{CallContext, Caller, Decide, Local, Read, Secret, Witness, Write};

    #[must_use]
    pub fn local() -> Local {
        Local(())
    }
    #[must_use]
    pub fn read() -> Read {
        Read(())
    }
    #[must_use]
    pub fn write() -> Write {
        Write(())
    }
    /// The `decide` witness exists only for a local caller.
    #[must_use]
    pub fn decide(_: &Local) -> Decide {
        Decide(())
    }
    /// The `secret` witness exists only for a local caller.
    #[must_use]
    pub fn secret(_: &Local) -> Secret {
        Secret(())
    }
    #[must_use]
    pub fn context<W: Witness>(caller: Caller, witness: W) -> CallContext<W> {
        CallContext { caller, witness }
    }
}
