//! `keeper-ported` — upstream code rewritten in Rust (AD-396, FR-767).
//!
//! The rules this crate exists to keep:
//! - **No keeper dependency, no network, no async runtime, no tauri.**
//!   `bun run check:ported-pure` asserts it over `cargo tree`.
//! - **One module per upstream**, each with an `UPSTREAM.md` naming the
//!   repository, commit, licence and what was ported, changed and left out.
//!   `tests/upstream.rs` fails a module without one, or one whose licence is
//!   not on `deny.toml`'s allow-list.
//! - **A module lands with its first consumer** (P14): nothing here is ported
//!   ahead of the keeper code that calls it.

pub mod agentskills;
pub mod bmad;
pub mod hermes;
