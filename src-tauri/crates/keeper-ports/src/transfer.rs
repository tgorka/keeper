//! A file's transfer between this device and its database's object store
//! (story 115.3): the store origin the server announces, the server half of
//! a transfer (the grants it signs) and the transfer itself. A body goes
//! from a file on disk to presigned URLs and back to a file on disk, never
//! through memory whole, never through the replica's SQLite file and never
//! inside a sync round. The wire is `syncular/README.md` § *Objects in
//! parts*.

use std::path::Path;
use std::sync::{Arc, PoisonError, RwLock};

use crate::ReplicaError;

/// The object store a database's server announced (`GET /databases`'s
/// `store`): `scheme://host[:port]` and nothing else — never a path, a query
/// or a credential.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct StoreOrigin(String);

impl StoreOrigin {
    /// The origin `announced` names: `http`/`https` with a host. A value
    /// carrying a user or password is refused (`None`); a path, query or
    /// fragment is dropped, so a signed query is never kept.
    pub fn parse(announced: &str) -> Option<StoreOrigin> {
        let url = url::Url::parse(announced.trim()).ok()?;
        if !matches!(url.scheme(), "http" | "https")
            || url.host_str().is_none_or(str::is_empty)
            || !url.username().is_empty()
            || url.password().is_some()
        {
            return None;
        }
        Some(StoreOrigin(url.origin().ascii_serialization()))
    }

    /// `scheme://host[:port]`.
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// `host[:port]`, as the egress list shows it.
    pub fn authority(&self) -> &str {
        self.0.split_once("://").map_or(&self.0, |(_, rest)| rest)
    }

    /// Whether `url` is on this origin.
    pub fn holds(&self, url: &url::Url) -> bool {
        url.origin().ascii_serialization() == self.0
    }
}

/// The store origin the server last announced for one database, shared by
/// the device's discovery (which sets it) and the replica's transport and
/// transfer (which read it on every request).
#[derive(Debug, Clone, Default)]
pub struct StoreSlot(Arc<RwLock<Option<StoreOrigin>>>);

impl StoreSlot {
    pub fn get(&self) -> Option<StoreOrigin> {
        self.0
            .read()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }

    pub fn set(&self, store: Option<StoreOrigin>) {
        *self.0.write().unwrap_or_else(PoisonError::into_inner) = store;
    }
}

/// How far a transfer has got, in bytes of the file.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Progress {
    pub done: u64,
    pub total: u64,
}

/// A part of an upload in parts: its number (from 1) and its SHA-256,
/// standard base64 (as `x-amz-checksum-sha256` carries it).
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct PartDigest {
    pub number: u32,
    pub sha256: String,
}

/// The location checked before granting an upload of file bytes.
#[derive(Debug, Clone, Copy, serde::Serialize)]
#[serde(untagged)]
pub enum UploadTarget<'a> {
    Create { parent_id: &'a str, name: &'a str },
    Replace { entry_id: &'a str },
}

/// The existing reference authorizing a file download, including renewals.
#[derive(Debug, Clone, Copy, serde::Serialize)]
#[serde(untagged)]
pub enum DownloadReference<'a> {
    Entry {
        entry_id: &'a str,
    },
    Proposal {
        proposal_id: &'a str,
        proposal_revision: &'a str,
    },
}

/// What the client asks the server to grant for an upload.
#[derive(Debug, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UploadAsk<'a> {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub target: Option<UploadTarget<'a>>,
    /// `sha256:<hex>` of the whole file.
    pub content_id: &'a str,
    pub size: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub media_type: Option<&'a str>,
    /// `0` when the file goes in one `PUT` (size at most
    /// `filePartThresholdBytes`); else every part but the last has this size.
    pub part_bytes: u64,
    /// Empty for one `PUT`.
    pub parts: &'a [PartDigest],
}

/// One presigned URL and when it stops working.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SignedUrl {
    pub url: String,
    pub expires_at_ms: i64,
}

/// A part's presigned `UploadPart` URL.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PartUrl {
    pub number: u32,
    pub url: SignedUrl,
}

/// The server's answer to an upload ask.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum UploadGrant {
    /// The space already holds these bytes: nothing to send.
    Present,
    /// One presigned `PUT` of the whole file.
    Single {
        signed: SignedUrl,
        plan_id: Option<String>,
    },
    /// A plan in parts: its id, and URLs for the parts the client should
    /// send (the server may sign only some; the client asks for the rest).
    Parts { plan_id: String, urls: Vec<PartUrl> },
}

/// A part the store holds for a plan (`ListParts`, checked by the server
/// against the plan).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HeldPart {
    pub number: u32,
    pub size: u64,
}

/// The server's answer to a completion.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Completion {
    /// Read back, hashed to its content id, placed at its content key.
    Placed,
    /// Completed from the plan; the read-back runs off the request path.
    Verifying,
}

/// The server half of a transfer: the grants it signs after checking who
/// the caller is. Every call is one request with its own deadline.
pub trait TransferGrants: Send + Sync {
    /// Ask to upload a file.
    fn grant_upload(&self, ask: &UploadAsk<'_>) -> Result<UploadGrant, ReplicaError>;
    /// Fresh URLs for these parts of a plan.
    fn part_urls(&self, plan_id: &str, numbers: &[u32]) -> Result<Vec<PartUrl>, ReplicaError>;
    /// The parts the store holds for a plan.
    fn held_parts(&self, plan_id: &str) -> Result<Vec<HeldPart>, ReplicaError>;
    /// Complete a plan, from the server's plan, never from the client.
    fn complete(&self, plan_id: &str) -> Result<Completion, ReplicaError>;
    /// A presigned `GET` of a content id the caller may read.
    fn grant_download(
        &self,
        content_id: &str,
        reference: Option<DownloadReference<'_>>,
    ) -> Result<SignedUrl, ReplicaError>;
}

/// What an upload did. `bytes_sent` counts body bytes put on the wire,
/// re-sent parts included (the transport's byte counter, AC17).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Uploaded {
    pub content_id: String,
    pub size: u64,
    pub bytes_sent: u64,
    pub parts_sent: u32,
    /// Parts the store already held when this run began (a resume).
    pub parts_skipped: u32,
    pub completion: Option<Completion>,
}

/// What a download did. `bytes_fetched` counts body bytes received;
/// `grants` how many download URLs were asked for (more than one after an
/// expired URL).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Downloaded {
    pub size: u64,
    pub bytes_fetched: u64,
    pub grants: u32,
}

/// The device's transfer of one database's files. Both calls block; run
/// them off any sync round's thread.
pub trait FileTransfer: Send + Sync {
    /// Upload `file`: read once to compute its content id and part
    /// digests, then sent as one `PUT` or in parts. A plan in progress is
    /// kept in the transfer's plan directory (never the replica file) and
    /// resumed from what the store holds.
    fn upload(
        &self,
        file: &Path,
        media_type: Option<&str>,
        progress: &mut dyn FnMut(Progress),
    ) -> Result<Uploaded, ReplicaError>;

    /// Download `content_id` (`size` bytes) to `dest`: streamed to
    /// `dest` + `.part`, resumed by `Range` from the bytes already there, on
    /// a fresh URL when the old one expired; its SHA-256 checked before it
    /// is renamed to `dest`.
    fn download(
        &self,
        content_id: &str,
        size: u64,
        dest: &Path,
        progress: &mut dyn FnMut(Progress),
    ) -> Result<Downloaded, ReplicaError>;
}

/// The plan has not positively confirmed placement; retry without submitting the change.
pub const PLACEMENT_PENDING: &str = "files.placement_pending";

/// Originals of the grant fields needed to confirm placement after the local source is gone.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct PlacementPlan {
    pub plan_id: String,
    pub part_bytes: u64,
    pub parts: Vec<PartDigest>,
    pub presence_unconfirmed: bool,
}

/// Body transfer finished. A put with a plan must durably queue it before confirming placement.
#[derive(Debug)]
pub struct StagedPut {
    pub upload: Uploaded,
    pub placement: Option<PlacementPlan>,
}

/// File operations cannot ask for bytes without naming the target or reference.
pub trait ScopedFileTransfer: Send + Sync {
    fn stage_put(
        &self,
        file: &Path,
        media_type: Option<&str>,
        target: UploadTarget<'_>,
        progress: &mut dyn FnMut(Progress),
    ) -> Result<StagedPut, ReplicaError>;

    fn confirm_put(
        &self,
        plan: &mut PlacementPlan,
        content_id: &str,
        size: u64,
        media_type: Option<&str>,
        target: UploadTarget<'_>,
    ) -> Result<(), ReplicaError>;

    fn upload_proposal(
        &self,
        file: &Path,
        media_type: Option<&str>,
        target: UploadTarget<'_>,
        progress: &mut dyn FnMut(Progress),
    ) -> Result<Uploaded, ReplicaError>;

    fn download_file(
        &self,
        content_id: &str,
        reference: DownloadReference<'_>,
        size: u64,
        dest: &Path,
        progress: &mut dyn FnMut(Progress),
    ) -> Result<Downloaded, ReplicaError>;
}
