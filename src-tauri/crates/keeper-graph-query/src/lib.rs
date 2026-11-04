//! Isolated native proof implementation; never a dependency of credential-bearing clients.
#![cfg(any(target_os = "linux", target_os = "macos"))]
pub mod allocation;
pub mod boundary;
pub mod builder;
pub mod confinement;
pub mod engine;
pub mod native;
pub mod schema;

/// Entry points for the separate proof executables (`examples/`), compiled only with
/// the non-default `proof-executables` feature; never part of the shipped worker.
#[cfg(feature = "proof-executables")]
pub mod proof {
    /// The process-wide one-shot callback-permission claim (see `allocation::enter`).
    pub fn enter() {
        crate::allocation::enter()
    }
    /// A reader whose warm-up query and final read/function policy are chosen by a
    /// proof executable, to exercise the retained-statement transition (R3).
    pub fn reader_with_policy(
        connection: rusqlite::Connection,
        warmup: &str,
        warmup_parameters: &str,
        reads: Vec<(&'static str, &'static str)>,
        functions: Vec<&'static str>,
    ) -> crate::engine::Result<crate::engine::Reader> {
        crate::engine::Reader::with_policy(connection, warmup, warmup_parameters, reads, functions)
    }
    /// The worker's exact production policy, for proofs that derive a reduced one.
    pub fn production_policy() -> (
        &'static [(&'static str, &'static str)],
        &'static [&'static str],
    ) {
        crate::engine::policy()
    }
}
