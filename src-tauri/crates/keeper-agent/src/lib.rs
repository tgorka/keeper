//! The client half of keeper's agents (AD-367, R-HQ-2): what the app does with
//! the drive's sessions and their files, with no window attached.
//!
//! - [`sessions`] is the sessions runtime: the journaled executor, the zone
//!   lock, the scan, the board and the verbs, one plan at a time per zone.
//! - [`promote`] promotes a session's files into its artifacts and its
//!   harvested notes into the drive's vault, and renders the promote panel.
//! - [`surface`] names a note the person looks at by its drive.
//! - [`zone`] reads a file inside a drive's zone.
//!
//! The agents' runtime — the turn loop, the drive tool host, approvals, MCP,
//! the sandboxed `run`, memory and attachments — lives in keeper-hq's
//! `hq-agent` (108.3b) and left keeper in 108.3c, with the ⌘9 bots' turn that
//! was its last caller here (108.3a). Its Matrix half left in 108.2. Nothing
//! here names `tauri`, and `bun run check:agent-tauri-free` keeps it that way.
//! A phone builds all of it.

pub mod promote;
pub mod sessions;
pub mod surface;
pub mod zone;
