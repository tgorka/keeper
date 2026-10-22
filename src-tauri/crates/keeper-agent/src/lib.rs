//! keeper's one turn loop (AD-367): the seam where `keeper-core`'s decisions
//! and `keeper-sync`'s drive meet, with no window attached.
//!
//! The app links it and fills its ports with a webview channel, the voice
//! turn, the notes vault and the approval sheet; keeper-hq's runtime links
//! it and fills them with the agents' database. Nothing here names `tauri`, and
//! `bun run check:agent-tauri-free` keeps it that way.
//!
//! - [`turn`] opens a turn: the rows, the replay, the grants, the request.
//! - [`drive`] runs it: the tool loop, the partial row, the stream.
//! - [`served`] runs an agent's turn from a person's message with no room:
//!   the agent's loop and limits, streamed into the host's sink.
//! - [`host`] is the drive tool host every tool call goes through.
//! - [`task`] is the scheduled bot task, over the same arming.
//! - [`sessions`] is the sessions runtime: one plan at a time per zone.
//! - [`ports`] is what a host process supplies.
//! - [`promote`] promotes a session's files into its artifacts and its
//!   harvested notes into the drive's vault, and renders the promote panel.
//! - [`approval`] is the ask-and-wait a host with a person at it plugs in.
//! - [`parked`] is the park and resume of an approval behind a record port,
//!   for a host whose record is a row rather than a room's file and event.
//! - [`grants`] is where a turn's grants come from: the app's rows or an
//!   agent's own `agent.toml`.
//! - [`approvals`] is what a call that waited for a person is told when it
//!   was not done; [`agent`] the sentence a cut-off turn ends with.
//! - [`zone`] reads the drives' zones.
//! - `headless` (unix) is a Linux host's process layer: directories, secrets,
//!   its own engine and provider rows.
//! - [`seed`] writes an agents zone's seed and a new agent, never over a
//!   file.
//! - [`surface`] names a note the person looks at by its drive.
//! - [`stewards`] makes a steward's triage and harvest sessions and says
//!   which closed sessions wake her harvest.
//!
//! The agents' Matrix half — the Linux host's run loop over rooms, an
//! agent's turn served from its room, the claims, placement and host
//! manifests, the room sink and gates, the doorbell, delegation and asks
//! through rooms, the maintenance jobs under a room's claim, This Mac as a
//! host — left keeper in 108.2: the agents' runtime serves from the agents'
//! database in keeper-hq.
//!
//! **A phone builds the read half only** (story 98.2, AD-413): under
//! `cfg(phone)` (`build.rs` — an iOS or Android target) the run writer in
//! [`cards`] and every module built on it are absent, so a phone code path
//! that names one does not compile. [`sessions`] — the scan, the board and the
//! journaled executor a card move runs through — stays.

// matrix-sdk's sync future is deep enough to need it, as in keeper-core.
#![recursion_limit = "256"]

#[cfg(not(phone))]
pub mod agent;
pub mod approval;
#[cfg(not(phone))]
pub mod approvals;
#[cfg(not(phone))]
pub mod attachments;
pub mod bmad;
#[cfg(not(phone))]
pub mod cards;
pub mod drive;
pub mod grants;
#[cfg(unix)]
pub mod headless;
pub mod host;
pub mod kvm;
#[cfg(not(phone))]
pub mod mcp;
pub mod memory;
#[cfg(not(phone))]
pub mod parked;
pub mod ports;
pub mod previews;
pub mod promote;
pub mod run;
#[cfg(not(phone))]
pub mod search;
#[cfg(not(phone))]
pub mod seed;
#[cfg(not(phone))]
pub mod served;
pub mod sessions;
pub mod skills;
pub mod stewards;
pub mod surface;
pub mod task;
pub mod turn;
#[cfg(not(phone))]
pub mod web;
pub mod zone;
