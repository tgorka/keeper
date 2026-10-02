//! Agents — named souls that live in a drive and work in its sessions
//! (Epics 89–99, `ARCHITECTURE-AGENTS.md`).
//!
//! An agent is a soul and a home drive running on a provider bot (AD-360):
//! "bot" stays the provider's word (`crate::bots`), and an agent *runs on* one.
//! Everything in this module is a decision over text the host read — no
//! function here opens a file inside a zone (the host walks the drive through
//! `keeper_sync::browse::resolve`, AD-65) — except the session log's writer and
//! reader and the `.keeper/` index, which own their own files (AD-366).
pub mod drive;
pub mod home;
pub mod label;
pub mod memory;
pub mod prompt;
pub mod skills;
pub mod soul;
pub mod zone;
