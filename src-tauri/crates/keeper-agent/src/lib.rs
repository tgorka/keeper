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
//! - [`grants`] is where a turn's grants come from: the app's rows or an
//!   agent's own `agent.toml`.
//! - [`agent`] is an agent's turn in a session, over its held context;
//!   [`writer`] writes the session's log, [`zone`] reads the drives' zones,
//!   [`rooms`] decides invites and whose words are a turn, and
//!   [`matrix_sink`] streams an answer as paced edits.
//! - [`claims`] is which host writes a session: epoch claims with a settle
//!   and a lease the writer checks; `hosts` (unix) is the placement, claims
//!   and host manifest a host process runs each tick.
//! - `runtime` (unix) is a Linux host's run loop; `desktop` (unix) is the
//!   app's host over the app's own drives, pins and keychain.
//! - `headless` (unix) is a Linux host's process layer: directories, secrets,
//!   its own engine and provider rows.
//! - [`seed`] writes an agents zone's seed and a new agent, never over a
//!   file, and makes a proxy's DM with its `main` session.
//! - [`surface`] names a note the person looks at by its drive.
//! - [`delegate`] hands work to another agent and answers a delegation.
//! - [`stewards`] makes a steward's triage and harvest sessions and says
//!   which closed sessions wake her harvest.
//! - [`doorbell`] rings the hosts that should fetch a push of agent work,
//!   and fetches when it is rung.

// matrix-sdk's sync future is deep enough to need it, as in keeper-core.
#![recursion_limit = "256"]

pub mod agent;
pub mod approval;
pub mod approvals;
pub mod cards;
pub mod claims;
pub mod delegate;
#[cfg(unix)]
pub mod desktop;
#[cfg(unix)]
pub mod doorbell;
pub mod drive;
pub mod grants;
#[cfg(unix)]
pub mod headless;
pub mod host;
#[cfg(unix)]
pub mod hosts;
pub mod matrix_sink;
pub mod ports;
pub mod rooms;
#[cfg(unix)]
pub mod runtime;
pub mod seed;
pub mod sessions;
pub mod sinks;
pub mod stewards;
pub mod surface;
pub mod task;
pub mod turn;
pub mod writer;
pub mod zone;
