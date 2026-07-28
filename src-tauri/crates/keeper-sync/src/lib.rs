//! `keeper-sync` — git-protocol folder synchronization (AD-40 … AD-53).
//!
//! Synchronizes a local folder with a remote git repository: bidirectionally,
//! or one-way into a review lane. Built on **gitoxide** for everything it does
//! well, with a thin audited shim over the `git` binary for the four things it
//! cannot do (push, worktree mutation, sparse-checkout patterns, gc — AD-41).
//!
//! # Boundaries
//!
//! This crate carries **no `tauri`** and **no `keeper-core`** (AD-40):
//!
//! * tauri-free, so `keeper-syncd` can link it on a headless server;
//! * core-free, so the daemon never inherits matrix-sdk and the iOS build never
//!   inherits gitoxide.
//!
//! It reaches the OS only through [`platform::SyncPlatform`], and it owns a
//! self-contained [`error::SyncError`] that the Tauri shell folds into its
//! existing `IpcError` envelope.
//!
//! # Shape
//!
//! The engine holds **policy**: what to fetch, what is complete enough to
//! commit, what a conflict becomes, when to retry. The host holds everything
//! platform-shaped: timeouts, free-space probes, task ownership, tray
//! rendering, and the quit bound.
//!
//! # Guarantees
//!
//! * **Nothing is lost.** Every network unit is journaled before it is
//!   attempted and cleared only once its effect is durable ([`db`]).
//! * **Nothing is half-read.** A file is committed only after it passes the
//!   four-tier completeness gate, of which only verify-on-read is a proof
//!   ([`stability`]).
//! * **Nothing waits on a human to converge.** Divergence produces conflict
//!   copies, never a modal ([`git::conflict`]).
//! * **An absent volume is never a deletion** ([`volume`]).

pub mod backoff;
pub mod copy;
pub mod db;
pub mod engine;
pub mod error;
pub mod exclude;
pub mod git;
pub mod lfs;
pub mod platform;
pub mod profile;
pub mod progress;
pub mod provenance;
pub mod stability;
pub mod volume;
pub mod watch;

pub use copy::{
    copy_verified, CopyEntry, CopyOptions, CopyOutcome, CopyProgress, CopyReport, CopySink,
};
pub use db::{ActivityKind, ActivityRow};
pub use engine::{
    Engine, ParkedUnit, PendingFile, PendingReason, ProblemReport, SyncOutcome, VerifyReport,
};
pub use error::{Result, SyncError};
pub use platform::SyncPlatform;
pub use profile::{ProfileState, SyncDirection, SyncLane, SyncProfile};
pub use progress::{SyncPhase, SyncProgress, SyncStatus};
pub use provenance::{Provenance, SyncSource};

/// Version string stamped into provenance trailers and the LFS user agent.
pub const AGENT: &str = concat!("keeper-sync/", env!("CARGO_PKG_VERSION"));
