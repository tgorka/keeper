//! The puts waiting for their commit, kept in one device-local JSON file
//! beside the replica (never in it), rewritten whole and renamed into
//! place, so a quit process loses none. Each keeps the request id it was
//! given when it was queued: sending it again is answered from the
//! server's receipt, never applied twice.

use std::path::PathBuf;

use keeper_ports::transfer::{PlacementPlan, StagedPut, UploadTarget};
use serde::{Deserialize, Serialize};

use crate::FilesError;

/// What a waiting put writes.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case")]
pub enum PendingOp {
    /// A new file `entry_id` named `name` in folder `parent_id`.
    Create {
        entry_id: String,
        parent_id: String,
        name: String,
    },
    /// New bytes for file `entry_id` at `expected_revision`.
    Replace {
        entry_id: String,
        expected_revision: String,
        /// Its folder and name when the put was planned: a conflict copy
        /// lands beside it.
        parent_id: String,
        name: String,
    },
}

impl PendingOp {
    pub(crate) fn upload_target(&self) -> UploadTarget<'_> {
        match self {
            Self::Create {
                parent_id, name, ..
            } => UploadTarget::Create { parent_id, name },
            Self::Replace { entry_id, .. } => UploadTarget::Replace { entry_id },
        }
    }
}

/// Older queues used a path here (or null once ready to name). A placement
/// record is distinct: its source may no longer exist and must never be read.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum PendingBody {
    Source(PathBuf),
    Placement {
        plan: PlacementPlan,
        bytes_sent: u64,
        parts_sent: u32,
        parts_skipped: u32,
    },
}

impl PendingBody {
    pub(crate) fn after_stage(stage: StagedPut) -> (String, u64, Option<Self>) {
        let upload = stage.upload;
        let body = stage.placement.map(|plan| Self::Placement {
            plan,
            bytes_sent: upload.bytes_sent,
            parts_sent: upload.parts_sent,
            parts_skipped: upload.parts_skipped,
        });
        (upload.content_id, upload.size, body)
    }
}

/// One put waiting for its commit.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Pending {
    pub request_id: String,
    pub drive: String,
    pub op: PendingOp,
    pub content_id: String,
    pub size: u64,
    pub mtime_ms: Option<i64>,
    /// `None` only once ready for command submission.
    pub body: Option<PendingBody>,
    /// The entry this put is a conflict copy of.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub conflict_of: Option<String>,
}

impl Pending {
    /// The entry the put writes.
    #[must_use]
    pub fn entry_id(&self) -> &str {
        match &self.op {
            PendingOp::Create { entry_id, .. } | PendingOp::Replace { entry_id, .. } => entry_id,
        }
    }

    /// The folder and name the put was planned under.
    #[must_use]
    pub fn place(&self) -> (&str, &str) {
        match &self.op {
            PendingOp::Create {
                parent_id, name, ..
            }
            | PendingOp::Replace {
                parent_id, name, ..
            } => (parent_id, name),
        }
    }
}

/// The queue's file.
#[derive(Debug, Clone)]
pub struct Queue {
    file: PathBuf,
}

impl Queue {
    #[must_use]
    pub fn new(file: PathBuf) -> Self {
        Self { file }
    }

    /// Kept beside the queue, including offline, and isolated from other requests
    /// for the same bytes or target. A queue field is never a pathname component.
    pub(crate) fn handoff(&self, request_id: &str) -> PathBuf {
        use sha2::{Digest, Sha256};
        self.file.with_added_extension(format!(
            "{:x}.handoff",
            Sha256::digest(request_id.as_bytes())
        ))
    }

    pub(crate) fn release_handoff(path: &std::path::Path) -> Result<(), FilesError> {
        match std::fs::remove_file(path) {
            Ok(()) => Ok(()),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(e) => Err(FilesError::storage("release staged handoff", &e)),
        }
    }

    /// Every waiting put, oldest first.
    pub fn load(&self) -> Result<Vec<Pending>, FilesError> {
        match std::fs::read(&self.file) {
            Ok(bytes) => serde_json::from_slice(&bytes)
                .map_err(|error| FilesError::Storage(format!("queue: {error}"))),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(Vec::new()),
            Err(error) => Err(FilesError::storage("queue", &error)),
        }
    }

    /// Replace the queue with `pending`, durably.
    pub fn save<'a>(
        &self,
        pending: impl IntoIterator<Item = &'a Pending>,
    ) -> Result<(), FilesError> {
        let fail = |error: std::io::Error| FilesError::storage("queue", &error);
        if let Some(dir) = self.file.parent() {
            std::fs::create_dir_all(dir).map_err(fail)?;
        }
        let mut staging = self.file.clone().into_os_string();
        staging.push(".tmp");
        let staging = PathBuf::from(staging);
        use serde::Serializer as _;
        {
            use std::io::{BufWriter, Write as _};
            let mut file = BufWriter::new(std::fs::File::create(&staging).map_err(fail)?);
            serde_json::Serializer::pretty(&mut file)
                .collect_seq(pending)
                .map_err(|error| FilesError::Storage(format!("queue: {error}")))?;
            file.flush().map_err(fail)?;
            file.get_ref().sync_all().map_err(fail)?;
        }
        std::fs::rename(&staging, &self.file).map_err(fail)?;
        #[cfg(unix)]
        std::fs::File::open(
            self.file
                .parent()
                .filter(|p| !p.as_os_str().is_empty())
                .unwrap_or(std::path::Path::new(".")),
        )
        .and_then(|dir| dir.sync_all())
        .map_err(fail)?;
        Ok(())
    }

    /// Replace the put whose request id is `request_id` with `next`
    /// (`None` removes it).
    pub fn settle(&self, request_id: &str, next: Option<&Pending>) -> Result<(), FilesError> {
        let all = self.load()?;
        let index = all
            .iter()
            .position(|pending| pending.request_id == request_id)
            .unwrap_or(all.len());
        let (before, after) = all.split_at(index);
        self.save(before.iter().chain(next).chain(after.iter().skip(1)))
    }
}
