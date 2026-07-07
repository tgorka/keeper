//! `keeper-core` — the tauri-free hexagon (AD-6).
//!
//! Owns keeper's Matrix state, IPC view models, and platform ports. It reaches
//! the OS only through the [`platform::Platform`] port and carries no `tauri`
//! dependency anywhere in its tree. New backend code defaults into this crate;
//! the `keeper` shell holds only IPC/plugin/protocol glue.

// matrix-sdk's deeply-nested async futures (Client build + Timeline subscribe)
// overflow rustc's default type-layout recursion depth; raise it for this crate.
#![recursion_limit = "256"]

pub mod account;
pub mod archive;
pub mod auth;
pub mod backup;
pub mod badge;
pub mod bridge;
pub mod bridges;
pub mod demo;
pub mod drafts;
pub mod egress;
pub mod error;
pub mod inbox;
pub mod media;
pub mod notify;
pub mod oauth;
pub mod palette;
pub mod platform;
pub mod registry;
pub mod send;
pub mod signals;
pub mod timeline;
pub mod verification;
pub mod vm;
