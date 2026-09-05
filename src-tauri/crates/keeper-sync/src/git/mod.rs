//! The git engine: gitoxide for everything it does, one audited `git` shim for
//! the four things it cannot (AD-41).
//!
//! # Why the split exists
//!
//! `gix 0.86` covers the whole read/write-locally half of synchronization —
//! clone, fetch, checkout, `status`, index read/write, object and commit
//! creation, `.gitattributes` and filter resolution, progress and interruption
//! — in pure Rust with a permissive license. It does **not** cover four things,
//! and none of them are coming:
//!
//! | Need | Upstream state |
//! | --- | --- |
//! | **push** | Tracking issue #306 closed `NOT_PLANNED` on 2026-07-22 and demoted to an unreviewed discussion. `gix::push` is a config enum; `Connection` exposes only `ref_map()` and `prepare_fetch()`. |
//! | **worktree add / remove / prune** | `gix::worktree` is read-only: `Proxy` lists, inspects and opens, and nothing mutates. |
//! | **sparse-checkout patterns** | Nothing in gitoxide reads `.git/info/sparse-checkout`; `gix_index::access::sparse` has zero consumers in the tree. |
//! | **gc / repack** | No maintenance path exists at any layer. |
//!
//! `git2`/`libgit2-sys` would supply all four and is **banned**: it declares
//! `MIT OR Apache-2.0` in crate metadata while vendoring GPL-2.0-with-linking-
//! exception C, so `cargo deny` would pass while the license firewall was
//! breached. Shelling out to the `git` binary is the honest option, and
//! [`cli`] is the only place in the workspace that does it.
//!
//! # Layout
//!
//! * [`repo`] — open, clone, `index.sparse=false` enforcement, status.
//! * [`fetch`] — fetching, the credential callback, fast-forward analysis.
//! * [`commit`] — worktree → index → tree → commit, with provenance trailers.
//! * [`conflict`] — the pure AD-43 convergence policy.
//! * [`cli`] — push, worktree mutation, sparse patterns, gc.
//! * [`push_http`] — the phone's push: a pack from `gix-pack` handed to
//!   `git-receive-pack` over smart HTTP, because iOS spawns no process (AD-202).
//! * [`history`] — the phone's `git log` / `show` / `diff` / `status` for one
//!   path, in-process, for the notes surfaces (Story 66.4).
//! * [`resolve`] — which of the machine's `git` binaries [`cli`] gets to drive.
//!
//! Two hazards documented in [`repo`] are load-bearing rather than defensive:
//! gitoxide silently drops repo-local `filter.*` configuration under the trust
//! level a pendrive repository gets by default, and `gix::status` hard-fails on
//! a true sparse index. Both would corrupt or stall a profile with no error
//! message at all.

pub mod cli;
pub mod commit;
pub mod conflict;
pub mod fetch;
pub mod history;
pub mod push_http;
pub mod repo;
pub mod resolve;

pub use cli::{GitCapabilities, GitCli};
pub use commit::StagedChange;
pub use conflict::{ChangeKind, Resolution, Side};
pub use fetch::{Credential, FetchOptions, FetchOutcome, TransferProgress};
pub use push_http::PushReport;
pub use repo::{RepoStatus, UnreadablePath};
pub use resolve::{GitChoice, GitOrigin, GitReject, GitRejection, GitRequest, GitResolution};
