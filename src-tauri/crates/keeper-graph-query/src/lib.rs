//! The separate graph worker: single-use, confined, never a dependency of a
//! credential-bearing consumer. `keeper_core::graph` launches its
//! executable. See `README.md` for its roles and exits, and
//! docs/constraints-and-limitations.md for its audited `unsafe` (U01–U11).
#![cfg(any(target_os = "linux", target_os = "macos"))]
pub mod allocation;
pub mod boundary;
pub mod builder;
#[cfg(target_os = "linux")]
pub mod class;
pub mod confinement;
pub mod engine;
pub mod native;
pub mod schema;
