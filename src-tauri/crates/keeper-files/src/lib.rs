//! A domain's drives, read from its replica, with files fetched on demand
//! (story 116.4). The tree is the replica's `files_entries` rows: a folder
//! is listed by `parent_id`, a path is derived by walking `parent_id` to the
//! drive's root and joined only through `keeper-paths`, and a typed path is
//! resolved segment by segment under the stored fold (DF/AD-10, G-11). A
//! file's bytes are fetched only when asked for, into a device-local cache
//! keyed by content id — never inside the replica's SQLite file (DF/AD-13) —
//! and a put uploads its bytes, then commits `files.put`; offline it waits
//! in a device-local queue and is committed on the next flush.
//!
//! A byte grant names the operation's location, not just its digest. Creates
//! name the parent and leaf; replacements name the existing entry. Queue replay
//! uses the stored operation. Cold reads name the entry or the proposal revision
//! together with the content id, including when a signed URL is renewed.
//! A direct put leaves its single staging plan open for verification by the
//! naming command. A proposal or multipart upload needs confirmed placement:
//! `verifying` retains the plan and never submits the command. If the server
//! hides a completed plan and does not attest presence to this caller, the
//! client reports `files.placement_pending` rather than treating 404 as success.
//!
//! - [`tree`]: the listing view model, drives, folders, paths.
//! - [`cache`]: the content-addressed cache and its digest check.
//! - [`queue`]: the puts waiting for their commit.
//! - [`review`]: proposals, decisions, acknowledgements, reversals and
//!   history (story 116.6): a proposal's bytes go up through the upload
//!   grant before `files.propose`, an open revision's come down through the
//!   download grant; the proposer's own revisions are read from the replica.
//! - [`Files`]: one domain of one server, as one device holds it.
//!
//! An implementation (DF/AD-2/3): it depends on the files contract, the
//! ports and the path leaf only.

pub mod cache;
mod command;
mod fetch;
mod put;
pub mod queue;
pub mod review;
pub mod tree;

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use keeper_files_api::refusal::Code;
use keeper_files_api::DomainRef;
use keeper_ports::transfer::ScopedFileTransfer;
use keeper_ports::{ReplicaError, Source, Unit};
use serde_json::Value;

pub use fetch::Fetched;
pub use put::{Dropped, PutOutcome, PutReport};
pub use review::{Change, Committed, Opened, Proposed};
pub use tree::{Listed, Listing};

use crate::cache::Cache;
use crate::queue::Queue;

/// The scope variable of a drive's entries (`<domain>/<drive>`).
pub const DRIVE_VARIABLE: &str = "files_drive";
/// The scope variable of a domain's head (its drives).
pub const DOMAIN_VARIABLE: &str = "files_domain";
/// The scope variable of a caller's own receipts.
pub const CLIENT_VARIABLE: &str = "files_client";
/// The scope variable of a proposer's own proposal revisions.
pub const PROPOSAL_VARIABLE: &str = "files_proposal";
/// The scope variable of a reviewer's proposal inbox.
pub const REVIEWER_VARIABLE: &str = "files_reviewer";

/// A caller's client scope: `<a>:<actor_id><c>:<client_id>`, `a` and `c`
/// the UTF-8 byte lengths of each id (`syncular/README.md`, the client
/// scope; `files_client` is each caller's own `(actor, client)`).
#[must_use]
pub fn client_scope(actor_id: &str, client_id: &str) -> String {
    format!(
        "{}:{actor_id}{}:{client_id}",
        actor_id.len(),
        client_id.len()
    )
}

/// The units a replica of `domain` holds before it knows its drives: the
/// domain's head and this device's own receipts. Each drive is subscribed
/// once the head names it ([`Files::subscribe_drives`]).
#[must_use]
pub fn units(domain: &str, actor: &str, client: &str) -> Vec<Unit> {
    vec![
        Unit::of(DOMAIN_VARIABLE, domain),
        Unit::of(CLIENT_VARIABLE, &client_scope(actor, client)),
        Unit::of(PROPOSAL_VARIABLE, actor),
        Unit::of(REVIEWER_VARIABLE, actor),
    ]
}

/// What a `get`, `put` or `ls` names: a drive and a path in it, or an
/// entry by its id.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Target {
    /// A drive's name and a drive-relative path (`""` is the root).
    Path { drive: String, path: String },
    /// An entry's id.
    Entry(String),
}

/// Why a files call did not do what it was asked.
#[derive(Debug, thiserror::Error)]
pub enum FilesError {
    /// `keeper-paths` refused the typed path; nothing was read or sent.
    #[error("path refused: {0}")]
    PathRefused(String),
    /// The name a put would give is not a name the contract admits.
    #[error("name refused: {0}")]
    NameRefused(String),
    /// No live entry, or no drive, answers the target.
    #[error("not found: {0}")]
    NotFound(String),
    /// The target is a file where a folder was asked for.
    #[error("not a folder: {0}")]
    NotAFolder(String),
    /// The target is a folder where a file was asked for.
    #[error("not a file: {0}")]
    NotAFile(String),
    /// The bytes are not on this device and it has no route to the store.
    #[error("offline: {0}")]
    Offline(String),
    /// The server refused the change for good (a refusal code of the
    /// contract other than `conflict`), and its detail.
    #[error("refused: {code:?}")]
    Refused { code: Code, detail: Option<Value> },
    /// The command was sent, but its receipt did not reach the replica within
    /// `receipt_wait`.
    #[error("sent; no receipt yet: {0}")]
    Unanswered(String),
    /// A row of the replica does not have the contract's shape.
    #[error("unreadable row: {0}")]
    Row(String),
    /// The cache, the queue or a local file could not be read or written.
    #[error("storage: {0}")]
    Storage(String),
    #[error(transparent)]
    Replica(#[from] ReplicaError),
}

impl FilesError {
    pub(crate) fn storage(what: &str, error: &std::io::Error) -> Self {
        Self::Storage(format!("{what}: {error}"))
    }
}

/// Where a device keeps one domain's files state, beside (never inside) its
/// replica file: the cache and the put queue.
#[derive(Debug, Clone)]
pub struct Layout {
    /// The content-addressed cache's directory.
    pub cache_dir: PathBuf,
    /// The put queue's file.
    pub queue_file: PathBuf,
}

impl Layout {
    /// `cache/` and `files-queue.json` under `state_dir`.
    #[must_use]
    pub fn under(state_dir: &Path) -> Self {
        Self {
            cache_dir: state_dir.join("cache"),
            queue_file: state_dir.join("files-queue.json"),
        }
    }
}

/// One domain of one server, as this device holds it.
pub struct Files {
    at: DomainRef,
    source: Source,
    transfer: Option<Arc<dyn ScopedFileTransfer>>,
    cache: Cache,
    queue: Queue,
    receipt_wait: Duration,
}

impl Files {
    /// `source`'s domain on `server`. `transfer` is `None` on a device with
    /// no route to the store (offline): reads answer from the replica and
    /// the cache, and a put waits in the queue.
    pub fn new(
        server: &str,
        source: Source,
        transfer: Option<Arc<dyn ScopedFileTransfer>>,
        layout: &Layout,
    ) -> Result<Self, FilesError> {
        Ok(Self {
            at: DomainRef {
                server: server.to_owned(),
                domain: source.database.clone(),
            },
            cache: Cache::open(&layout.cache_dir)?,
            queue: Queue::new(layout.queue_file.clone()),
            source,
            transfer,
            receipt_wait: Duration::from_secs(10),
        })
    }

    /// How long a commit waits for its receipt to reach the replica before
    /// it is left in the queue as sent (default 10 s).
    #[must_use]
    pub fn with_receipt_wait(mut self, wait: Duration) -> Self {
        self.receipt_wait = wait;
        self
    }

    /// The `(server, domain)` this acts on.
    #[must_use]
    pub fn at(&self) -> &DomainRef {
        &self.at
    }

    /// The device's cache.
    #[must_use]
    pub fn cache(&self) -> &Cache {
        &self.cache
    }

    /// The puts waiting for their commit, oldest first.
    pub fn pending(&self) -> Result<Vec<queue::Pending>, FilesError> {
        self.queue.load()
    }

    /// Subscribe each drive the head names (its `files_drive` value); the
    /// drives.
    pub fn subscribe_drives(&self) -> Result<Vec<keeper_files_api::view::DriveVm>, FilesError> {
        let drives = self.drives()?;
        for drive in &drives {
            self.source.replica.subscribe(Unit::of(
                DRIVE_VARIABLE,
                &keeper_files_api::ids::drive_key(&self.at.domain, &drive.name),
            ))?;
        }
        Ok(drives)
    }

    fn client_scope(&self) -> String {
        client_scope(&self.source.actor, &self.source.client)
    }
}
