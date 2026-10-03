//! keeper's one turn loop (AD-367): the seam where `keeper-core`'s decisions
//! and `keeper-sync`'s drive meet, with no window attached.
//!
//! The app links it and fills its ports with a webview channel, the voice
//! turn, the notes vault and the approval sheet; a Linux host links it and
//! fills them with a room. Nothing here names `tauri`, and
//! `bun run check:agent-tauri-free` keeps it that way.
//!
//! - [`turn`] opens a turn: the rows, the replay, the grants, the request.
//! - [`drive`] runs it: the tool loop, the partial row, the stream.
//! - [`host`] is the drive tool host every tool call goes through.
//! - [`task`] is the scheduled bot task, over the same arming.
//! - [`sessions`] is the sessions runtime: one plan at a time per zone.
//! - [`ports`] is what a host process supplies.
//! - [`approval`] is the ask-and-wait a host with a person at it plugs in.
//! - `headless` (unix) is a Linux host's process layer: directories, secrets,
//!   its own engine and provider rows.

pub mod approval;
pub mod drive;
#[cfg(unix)]
pub mod headless;
pub mod host;
pub mod ports;
pub mod sessions;
pub mod task;
pub mod turn;
