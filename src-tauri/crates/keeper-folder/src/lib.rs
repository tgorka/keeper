//! keeper's folder machinery (story 119.1, decision 3; DF/AD-13).
//!
//! What a folder on disk needs whatever keeps it in step with a remote: the
//! watcher ([`watch`]), the completeness gate ([`stability`]) with its
//! excludes ([`exclude`]), the volume marker ([`volume`]), the open-file probe
//! ([`platform`]), the virtualization grammar ([`virtual_policy`]), the
//! anomaly line ([`anomaly`]), the export copy ([`export`]), the person's
//! writers on a routed target ([`files_write`]), the convergence matrix
//! ([`conflict`]), the release rules ([`release`]), the LFS pointer format
//! ([`pointer`]) and the names keeper's own machinery writes ([`control`]).
//!
//! Moved out of `keeper-sync` without a change of behaviour, so `keeper-files`
//! and `keeper-agent` can use it without linking `keeper-sync` (DF/AD-2).
//! `keeper-sync` re-exports every module under its old path until 121.4
//! removes it. The leaf rule: `keeper-paths` is this crate's one first-party
//! dependency (`bun run check:module-edges`, role `folder-leaf`).

pub mod anomaly;
pub mod conflict;
pub mod control;
pub mod error;
pub mod exclude;
pub mod export;
pub mod files_write;
pub mod platform;
pub mod pointer;
pub mod release;
pub mod stability;
pub mod virtual_policy;
pub mod volume;
pub mod watch;

pub use error::{FolderError, Result, Retriability};
