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
pub mod approval_card;
pub mod card;
pub mod claim;
pub mod copy;
pub mod delegation;
pub mod device;
pub mod doorbell;
pub mod drive;
pub mod events;
pub mod focus;
pub mod home;
pub mod host;
pub mod index;
pub mod label;
pub mod log;
pub mod matrix;
pub mod memory;
pub mod mount;
pub mod pins;
pub mod placement;
pub mod presence;
pub mod prompt;
pub mod proxy;
pub mod redact;
pub mod room;
pub mod seed;
pub mod session;
pub mod skills;
pub mod soul;
pub mod spoken;
pub mod surface;
pub mod tier;
pub mod trust;
pub mod workflow;
pub mod zone;
