//! Agents — named souls that live in a drive and work in its sessions
//! (Epics 89–99, `ARCHITECTURE-AGENTS.md`).
//!
//! An agent is a soul and a home drive running on a provider bot (AD-360):
//! "bot" stays the provider's word (`crate::bots`), and an agent *runs on* one.
//! Everything in this module is a decision over text the host read — no
//! function here opens a file inside a zone (the host walks the drive through
//! `keeper_sync::browse::resolve`, AD-65) — except the session log's writer and
//! reader and the `.keeper/` index, which own their own files (AD-366).
pub mod agentd;
pub mod approval;
pub mod card;
pub mod console;
pub mod drive;
pub mod home;
pub mod index;
pub mod knowledge;
pub mod kvm;
pub mod label;
pub mod log;
pub mod mcp;
pub mod model;
pub mod mount;
pub mod paseo;
pub mod push;
pub mod redact;
pub mod run;
pub mod screen;
pub mod seed;
pub mod session;
pub mod soul;
pub mod tier;
pub mod unifiedpush;
pub mod web_search;
pub mod zone;
