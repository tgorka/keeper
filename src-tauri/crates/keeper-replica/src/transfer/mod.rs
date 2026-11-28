//! The file transfer of one database (story 115.3): a file goes between
//! this device and its database's object store by presigned URLs the
//! server grants, never through the replica's SQLite file, never inside a
//! sync round, never through `syncular-client`'s blob calls. Each request
//! has its own deadline. The plan of an upload in parts is a file in the
//! transfer's plan directory, so a cut, or a kill and restart, resumes from
//! the parts the store holds.
//!
//! - `upload`: read the file once (content id and every part's SHA-256),
//!   ask for a grant, send one `PUT` or the parts, complete.
//! - `download`: stream to `<dest>.part`, resume by `Range`, ask again on
//!   an expired URL, check the SHA-256, rename.
//! - `grants`: the server half over HTTP (`syncular/README.md` § *Objects
//!   in parts*).

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use keeper_ports::transfer::{
    DownloadReference, Downloaded, FileTransfer, Progress, ScopedFileTransfer, TransferGrants,
    UploadTarget, Uploaded,
};
use keeper_ports::{ReplicaError, StoreOrigin, StoreSlot};

use crate::limits::{FILE_PART_BYTES, FILE_PART_THRESHOLD_BYTES};

mod download;
mod grants;
mod upload;

pub use grants::{announced_store, BlobRouteGrants, HttpGrants};

/// The code a URL off the server's and its store's origins fails with.
pub const FOREIGN: &str = "transport.foreign_origin";

/// The sentence of [`FOREIGN`] (story 115.3, amendment 2).
pub const FOREIGN_SENTENCE: &str =
    "the server named a host other than its own or its object store; keeper contacts only those";

/// Whether a presigned blob URL may be fetched: on the store origin last
/// announced for the database, or on the server's own origin.
pub fn blob_url_allowed(url: &url::Url, server: &url::Origin, store: Option<&StoreOrigin>) -> bool {
    &url.origin() == server || store.is_some_and(|store| store.holds(url))
}

/// The refusal of a URL [`blob_url_allowed`] does not allow.
pub fn foreign() -> ReplicaError {
    ReplicaError::Remote {
        code: FOREIGN.to_owned(),
        message: FOREIGN_SENTENCE.to_owned(),
        retryable: false,
    }
}

/// How a transfer is cut into requests.
#[derive(Debug, Clone)]
pub struct TransferConfig {
    /// At most this many bytes go in one `PUT`; above, in parts.
    pub threshold: u64,
    /// The least part size (every part but the last).
    pub part_bytes: u64,
    /// One request's deadline: the connection, the wait for an answer's
    /// head, and each silence while sending or receiving the body (every
    /// write or read restarts it). Never the whole body's, nor the
    /// transfer's: a slow but steady link finishes.
    pub request_timeout: Duration,
    /// Where the plans of uploads in parts are kept: device-local, beside
    /// the file cache, never the replica file.
    pub plan_dir: PathBuf,
}

impl TransferConfig {
    /// The contract's values (`syncular/limits.json`).
    pub fn contract(plan_dir: PathBuf) -> TransferConfig {
        TransferConfig {
            threshold: FILE_PART_THRESHOLD_BYTES,
            part_bytes: FILE_PART_BYTES,
            request_timeout: Duration::from_secs(30),
            plan_dir,
        }
    }

    /// The part size of a file of `size` bytes: `0` for one `PUT`; else
    /// `part_bytes`, or `⌈size / 10 000⌉` rounded up to a MiB when larger.
    pub fn part_size(&self, size: u64) -> u64 {
        if size <= self.threshold {
            return 0;
        }
        let least = size.div_ceil(crate::limits::MAX_PARTS);
        if least <= self.part_bytes {
            self.part_bytes
        } else {
            least.div_ceil(1 << 20) * (1 << 20)
        }
    }
}

/// [`FileTransfer`] over a database's grants and its announced store.
pub struct StoreTransfer {
    pub(crate) grants: Arc<dyn TransferGrants>,
    pub(crate) server: url::Origin,
    pub(crate) store: StoreSlot,
    pub(crate) config: TransferConfig,
    pub(crate) agent: ureq::Agent,
}

impl StoreTransfer {
    /// A transfer for the database mounted at `base_url`, whose server
    /// announces its store into `store`.
    pub fn new(
        grants: Arc<dyn TransferGrants>,
        base_url: &str,
        store: StoreSlot,
        config: TransferConfig,
    ) -> Result<StoreTransfer, ReplicaError> {
        let server = url::Url::parse(base_url)
            .map_err(|error| ReplicaError::Client(format!("the server's address: {error}")))?
            .origin();
        let agent = ureq::Agent::config_builder()
            .timeout_connect(Some(config.request_timeout))
            .timeout_recv_response(Some(config.request_timeout))
            .timeout_per_read(Some(config.request_timeout))
            .timeout_per_write(Some(config.request_timeout))
            .http_status_as_error(false)
            .max_redirects(0)
            .build()
            .new_agent();
        Ok(StoreTransfer {
            grants,
            server,
            store,
            config,
            agent,
        })
    }

    /// `url`, if the origin rule allows it; else [`FOREIGN`].
    pub(crate) fn checked(&self, url: &str) -> Result<url::Url, ReplicaError> {
        let parsed = url::Url::parse(url).map_err(|_| foreign())?;
        if blob_url_allowed(&parsed, &self.server, self.store.get().as_ref()) {
            Ok(parsed)
        } else {
            Err(foreign())
        }
    }
}

impl FileTransfer for StoreTransfer {
    fn upload(
        &self,
        file: &std::path::Path,
        media_type: Option<&str>,
        progress: &mut dyn FnMut(Progress),
    ) -> Result<Uploaded, ReplicaError> {
        upload::run(self, file, media_type, None, progress)
    }

    fn download(
        &self,
        content_id: &str,
        size: u64,
        dest: &std::path::Path,
        progress: &mut dyn FnMut(Progress),
    ) -> Result<Downloaded, ReplicaError> {
        download::run(self, content_id, None, size, dest, progress)
    }
}

impl ScopedFileTransfer for StoreTransfer {
    fn upload_file(
        &self,
        file: &std::path::Path,
        media_type: Option<&str>,
        target: UploadTarget<'_>,
        usage: keeper_ports::transfer::UploadUse,
        progress: &mut dyn FnMut(Progress),
    ) -> Result<Uploaded, ReplicaError> {
        upload::run(self, file, media_type, Some((target, usage)), progress)
    }

    fn download_file(
        &self,
        content_id: &str,
        reference: DownloadReference<'_>,
        size: u64,
        dest: &std::path::Path,
        progress: &mut dyn FnMut(Progress),
    ) -> Result<Downloaded, ReplicaError> {
        download::run(self, content_id, Some(reference), size, dest, progress)
    }
}
