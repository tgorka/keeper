//! Isolated native proof implementation; never a dependency of credential-bearing clients.
#![cfg(any(target_os = "linux", target_os = "macos"))]
pub mod allocation;
pub mod builder;
pub mod confinement;
pub mod engine;
pub mod native;
pub mod schema;
