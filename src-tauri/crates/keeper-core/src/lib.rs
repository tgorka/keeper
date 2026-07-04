//! `keeper-core` — the tauri-free hexagon (AD-6).
//!
//! Owns keeper's Matrix state, IPC view models, and platform ports. It reaches
//! the OS only through the [`platform::Platform`] port and carries no `tauri`
//! dependency anywhere in its tree. New backend code defaults into this crate;
//! the `keeper` shell holds only IPC/plugin/protocol glue.

pub mod demo;
pub mod error;
pub mod platform;
pub mod vm;
