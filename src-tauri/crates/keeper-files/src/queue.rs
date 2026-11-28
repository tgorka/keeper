//! The puts waiting for their commit, kept in one device-local JSON file
//! beside the replica (never in it), rewritten whole and renamed into
//! place, so a quit process loses none. Each keeps the request id it was
//! given when it was queued: sending it again is answered from the
//! server's receipt, never applied twice.

use std::path::PathBuf;

use keeper_ports::transfer::UploadTarget;
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

/// One put waiting for its commit.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Pending {
    pub request_id: String,
    pub drive: String,
    pub op: PendingOp,
    pub content_id: String,
    pub size: u64,
    pub mtime_ms: Option<i64>,
    /// The file put, by absolute path, while the store does not have its
    /// bytes yet; `None` once uploaded.
    pub body: Option<PathBuf>,
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
    pub fn save(&self, pending: &[Pending]) -> Result<(), FilesError> {
        let fail = |error: std::io::Error| FilesError::storage("queue", &error);
        if let Some(dir) = self.file.parent() {
            std::fs::create_dir_all(dir).map_err(fail)?;
        }
        let mut staging = self.file.clone().into_os_string();
        staging.push(".tmp");
        let staging = PathBuf::from(staging);
        let bytes = serde_json::to_vec_pretty(pending)
            .map_err(|error| FilesError::Storage(format!("queue: {error}")))?;
        {
            use std::io::Write as _;
            let mut file = std::fs::File::create(&staging).map_err(fail)?;
            file.write_all(&bytes).map_err(fail)?;
            file.sync_all().map_err(fail)?;
        }
        std::fs::rename(&staging, &self.file).map_err(fail)
    }

    /// Replace the put whose request id is `request_id` with `next`
    /// (`None` removes it).
    pub fn settle(&self, request_id: &str, next: Option<Pending>) -> Result<(), FilesError> {
        let mut all = self.load()?;
        match all
            .iter()
            .position(|pending| pending.request_id == request_id)
        {
            Some(index) => match next {
                Some(next) => all[index] = next,
                None => {
                    all.remove(index);
                }
            },
            None => all.extend(next),
        }
        self.save(&all)
    }
}
