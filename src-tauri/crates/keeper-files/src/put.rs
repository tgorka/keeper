//! Put: the bytes go to the store first (the files upload grant, one `PUT`
//! or parts), then `files.put` commits them. A put that cannot reach the
//! server waits in the queue under the request id it was given and is
//! committed by a later [`Files::flush`]. A put the server answers with
//! `conflict` (another device's change came first) keeps the person's
//! bytes as a new entry beside it, named by `keeper-files-api`'s conflict-copy rule,
//! through an ordinary put (116.3's rule): nothing a person put is lost.

use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

use keeper_files_api::rows::{Kind, Lifecycle, Outcome};
use keeper_ports::transfer::Progress;
use serde_json::json;

use crate::command::{refusal, unreachable, Delivered, ReceiptRow};
use crate::queue::{Pending, PendingOp};
use crate::{Files, FilesError, Target};

/// How many conflict copies one put may need before it gives up (each is
/// a fresh name; a second conflict on one means another device raced it).
const COPY_ATTEMPTS: u32 = 8;

/// What became of a put.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PutOutcome {
    /// The server accepted it: the entry holds these bytes.
    Accepted,
    /// Another device's change came first: the bytes are a new entry
    /// `entry_id` named `name` beside `of`.
    ConflictCopy {
        of: String,
        entry_id: String,
        name: String,
    },
    /// The server was not reached; it waits in the queue.
    Queued,
    /// Sent, and its receipt has not reached the replica yet; it stays in
    /// the queue and the next flush asks again under the same request id.
    Sent,
    /// Its source file is gone or no longer holds the bytes it was queued
    /// with: it was taken out of the queue unsent, so it can never hold
    /// the puts behind it.
    Dropped(Dropped),
}

/// Why a queued put was taken out of the queue without a commit.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Dropped {
    /// The source file can no longer be found.
    SourceGone,
    /// The source file no longer hashes to the queued content id.
    SourceChanged,
}

impl Dropped {
    /// The word an answer names it by.
    #[must_use]
    pub fn word(self) -> &'static str {
        match self {
            Self::SourceGone => "source_gone",
            Self::SourceChanged => "source_changed",
        }
    }
}

/// One put and what became of it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PutReport {
    pub request_id: String,
    pub entry_id: String,
    pub content_id: String,
    pub size: u64,
    pub outcome: PutOutcome,
    /// Body bytes this call put on the wire, re-sent parts included.
    pub bytes_sent: u64,
    pub parts_sent: u32,
    /// Parts the store already held (a continued upload).
    pub parts_skipped: u32,
}

fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |since| {
            i64::try_from(since.as_millis()).unwrap_or(i64::MAX)
        })
}

pub(crate) fn mtime_ms(file: &Path) -> Option<i64> {
    let modified = std::fs::metadata(file).ok()?.modified().ok()?;
    let since = modified.duration_since(UNIX_EPOCH).ok()?;
    i64::try_from(since.as_millis()).ok()
}

/// How a send ended.
enum Sent {
    Answer(ReceiptRow),
    Unreached,
    Unseen,
}

impl Files {
    /// Put `file` where `target` names: a new file under a drive's folder,
    /// or new bytes for an existing file. Refused before any request when
    /// `keeper-paths` refuses the path or the contract the name. `progress`
    /// hears the upload's bytes.
    pub fn put(
        &self,
        file: &Path,
        target: &Target,
        progress: &mut dyn FnMut(Progress),
    ) -> Result<PutReport, FilesError> {
        let (drive, op) = self.plan(target)?;
        let name = place(&op).1.to_owned();
        let mut sent = (0, 0, 0);
        let uploaded = match &self.transfer {
            Some(transfer) => match transfer.upload(
                file,
                Some(keeper_files_api::names::media_type(&name)),
                progress,
            ) {
                Ok(uploaded) => Some(uploaded),
                Err(error) if unreachable(&error) => None,
                Err(error) => return Err(error.into()),
            },
            None => None,
        };
        let (content_id, size, body) = match uploaded {
            Some(uploaded) => {
                sent = (
                    uploaded.bytes_sent,
                    uploaded.parts_sent,
                    uploaded.parts_skipped,
                );
                (uploaded.content_id, uploaded.size, None)
            }
            None => {
                // The file itself waits for the store: its plan file (if a
                // cut upload left one) still matches it, so the next upload
                // continues rather than starts over. A file changed before
                // then no longer hashes to the queued id and is refused.
                let (hex, size) = crate::cache::digest_file(file)
                    .map_err(|error| FilesError::storage("put", &error))?;
                let body = std::path::absolute(file)
                    .map_err(|error| FilesError::storage("put", &error))?;
                (format!("sha256:{hex}"), size, Some(body))
            }
        };
        let pending = Pending {
            request_id: ulid::Ulid::new().to_string(),
            drive,
            op,
            content_id,
            size,
            mtime_ms: mtime_ms(file),
            body,
            conflict_of: None,
        };
        // A put to the same place that never reached the store (its upload
        // cut, or made offline) was never sent: this put replaces it rather
        // than racing it into a conflict copy of the person's own bytes.
        let mut waiting = self.queue.load()?;
        waiting.retain(|earlier| earlier.body.is_none() || !same_place(&earlier.op, &pending.op));
        waiting.push(pending.clone());
        self.queue.save(&waiting)?;
        let mut report = self.commit(pending)?;
        report.bytes_sent += sent.0;
        report.parts_sent += sent.1;
        report.parts_skipped += sent.2;
        Ok(report)
    }

    /// Commit every waiting put, oldest first; what became of each. Stops
    /// at the first that cannot reach the server (the rest wait behind it,
    /// in order). A queued put whose source disappeared or changed is
    /// dropped and does not stop the flush.
    pub fn flush(&self) -> Result<Vec<PutReport>, FilesError> {
        let mut reports = Vec::new();
        for pending in self.queue.load()? {
            let report = self.commit(pending)?;
            let unreached = report.outcome == PutOutcome::Queued;
            reports.push(report);
            if unreached {
                break;
            }
        }
        Ok(reports)
    }

    /// The drive and operation a put to `target` plans, from the replica.
    pub(crate) fn plan(&self, target: &Target) -> Result<(String, PendingOp), FilesError> {
        match target {
            Target::Entry(_) => {
                let entry = self.resolve(target)?;
                if entry.kind != Kind::File {
                    return Err(FilesError::NotAFile(entry.id));
                }
                let (drive, _) = self.path_of(&entry)?;
                Ok((
                    drive,
                    PendingOp::Replace {
                        entry_id: entry.id,
                        expected_revision: entry.revision,
                        parent_id: entry.parent_id,
                        name: entry.name,
                    },
                ))
            }
            Target::Path { drive, path } => {
                let segments = keeper_paths::plain_segments(path)
                    .map_err(|refusal| FilesError::PathRefused(format!("{refusal:?}")))?;
                let Some(leaf) = segments.last().and_then(|leaf| leaf.to_str()) else {
                    return Err(FilesError::PathRefused(path.clone()));
                };
                keeper_files_api::names::admit_name(leaf)
                    .map_err(|_| FilesError::NameRefused(leaf.to_owned()))?;
                let parent_path = path.rsplit_once('/').map_or("", |(parent, _)| parent);
                let parent = self.resolve_path(drive, parent_path)?;
                if parent.kind != Kind::Dir {
                    return Err(FilesError::NotAFolder(parent.id));
                }
                let (drive, _) = self.path_of(&parent)?;
                let fold = keeper_paths::names::name_fold(leaf);
                let op = match self.child_by_fold(&parent.id, &fold)? {
                    Some(existing) if existing.kind == Kind::Dir => {
                        return Err(FilesError::NotAFile(existing.id));
                    }
                    Some(existing) => PendingOp::Replace {
                        entry_id: existing.id,
                        expected_revision: existing.revision,
                        parent_id: parent.id,
                        name: existing.name,
                    },
                    None => PendingOp::Create {
                        entry_id: ulid::Ulid::new().to_string(),
                        parent_id: parent.id,
                        name: leaf.to_owned(),
                    },
                };
                Ok((drive, op))
            }
        }
    }

    /// Upload what `pending` still owes the store, send its command and
    /// read its receipt; settle the queue by the answer.
    fn commit(&self, mut pending: Pending) -> Result<PutReport, FilesError> {
        let mut report = PutReport {
            request_id: pending.request_id.clone(),
            entry_id: pending.entry_id().to_owned(),
            content_id: pending.content_id.clone(),
            size: pending.size,
            outcome: PutOutcome::Queued,
            bytes_sent: 0,
            parts_sent: 0,
            parts_skipped: 0,
        };
        for _ in 0..COPY_ATTEMPTS {
            if let Some(body) = pending.body.clone() {
                let Some(transfer) = &self.transfer else {
                    return Ok(report);
                };
                match crate::cache::digest_file(&body) {
                    Ok((hex, _)) if format!("sha256:{hex}") == pending.content_id => {}
                    Ok(_) => {
                        self.queue.settle(&pending.request_id, None)?;
                        report.outcome = PutOutcome::Dropped(Dropped::SourceChanged);
                        return Ok(report);
                    }
                    Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                        self.queue.settle(&pending.request_id, None)?;
                        report.outcome = PutOutcome::Dropped(Dropped::SourceGone);
                        return Ok(report);
                    }
                    Err(error) => return Err(FilesError::storage("put", &error)),
                }
                let name = pending.place().1.to_owned();
                match transfer.upload(
                    &body,
                    Some(keeper_files_api::names::media_type(&name)),
                    &mut |_| {},
                ) {
                    Ok(uploaded) => {
                        if uploaded.content_id != pending.content_id {
                            self.queue.settle(&pending.request_id, None)?;
                            report.outcome = PutOutcome::Dropped(Dropped::SourceChanged);
                            return Ok(report);
                        }
                        report.bytes_sent += uploaded.bytes_sent;
                        report.parts_sent += uploaded.parts_sent;
                        report.parts_skipped += uploaded.parts_skipped;
                        pending.body = None;
                        self.queue
                            .settle(&pending.request_id, Some(pending.clone()))?;
                    }
                    Err(error) if unreachable(&error) => return Ok(report),
                    Err(error) => return Err(error.into()),
                }
            }
            match self.send(&pending)? {
                Sent::Unreached => return Ok(report),
                Sent::Unseen => {
                    report.outcome = PutOutcome::Sent;
                    return Ok(report);
                }
                Sent::Answer(receipt) => match receipt.outcome {
                    Outcome::Accepted => {
                        self.queue.settle(&pending.request_id, None)?;
                        report.request_id.clone_from(&pending.request_id);
                        report.entry_id = pending.entry_id().to_owned();
                        report.outcome = match &pending.conflict_of {
                            Some(of) => PutOutcome::ConflictCopy {
                                of: of.clone(),
                                entry_id: pending.entry_id().to_owned(),
                                name: pending.place().1.to_owned(),
                            },
                            None => PutOutcome::Accepted,
                        };
                        return Ok(report);
                    }
                    Outcome::Rejected => {
                        self.queue.settle(&pending.request_id, None)?;
                        return Err(refusal(receipt.reason.as_deref(), receipt.detail));
                    }
                    Outcome::Conflict => {
                        let copy = self.conflict_copy(&pending)?;
                        self.queue.settle(&pending.request_id, Some(copy.clone()))?;
                        pending = copy;
                    }
                    Outcome::Proposed => {
                        return Err(FilesError::Row(format!(
                            "receipt outcome {:?}",
                            receipt.outcome
                        )));
                    }
                },
            }
        }
        Err(FilesError::Storage(format!(
            "{COPY_ATTEMPTS} conflict copies of {} were refused",
            report.entry_id
        )))
    }

    /// The put that keeps `pending`'s bytes after a conflict: a new entry
    /// beside the one it meant to write (where that entry lives now, if it
    /// is still live), named by 116.3's one conflict-copy rule
    /// (`keeper_files_api::names::conflict_copy_name`) against the folder's
    /// live children; `None` from it is this put's refusal.
    fn conflict_copy(&self, pending: &Pending) -> Result<Pending, FilesError> {
        let (planned_parent, planned_name) = pending.place();
        let original_id = pending
            .conflict_of
            .clone()
            .unwrap_or_else(|| pending.entry_id().to_owned());
        let original = self.entry(&original_id)?;
        let parent_id = match &original {
            Some(entry) if entry.lifecycle == Lifecycle::Live && !entry.is_root() => {
                entry.parent_id.clone()
            }
            _ => planned_parent.to_owned(),
        };
        // A copy of a copy is still named after the original.
        let base = match (&pending.conflict_of, &original) {
            (Some(_), Some(entry)) => entry.name.clone(),
            _ => planned_name.to_owned(),
        };
        let stamp = keeper_paths::conflict_stamp(now_ms());
        let taken: Vec<String> = self
            .children(&parent_id)?
            .into_iter()
            .map(|child| child.name_fold)
            .collect();
        let name =
            keeper_files_api::names::conflict_copy_name(&base, &stamp, &self.source.client, &taken)
                .ok_or_else(|| FilesError::Storage(format!("no free conflict name for {base}")))?;
        Ok(Pending {
            request_id: ulid::Ulid::new().to_string(),
            drive: pending.drive.clone(),
            op: PendingOp::Create {
                entry_id: ulid::Ulid::new().to_string(),
                parent_id,
                name,
            },
            content_id: pending.content_id.clone(),
            size: pending.size,
            mtime_ms: pending.mtime_ms,
            body: None,
            conflict_of: Some(original_id),
        })
    }

    /// Send `pending`'s `files.put` and read its answer.
    fn send(&self, pending: &Pending) -> Result<Sent, FilesError> {
        let mut operation = match &pending.op {
            PendingOp::Create {
                entry_id,
                parent_id,
                name,
            } => json!({
                "op": "create",
                "id": entry_id,
                "kind": "file",
                "parent_id": parent_id,
                "name": name,
                "content_id": pending.content_id,
                "size": pending.size,
            }),
            PendingOp::Replace {
                entry_id,
                expected_revision,
                ..
            } => json!({
                "op": "replace",
                "id": entry_id,
                "expected_revision": expected_revision,
                "content_id": pending.content_id,
                "size": pending.size,
            }),
        };
        if let (Some(mtime), Some(fields)) = (pending.mtime_ms, operation.as_object_mut()) {
            fields.insert("mtime_ms".to_owned(), json!(mtime));
        }
        match self.deliver(
            "files.put",
            &pending.request_id,
            json!({"operation": operation}),
        )? {
            Delivered::Unreached => Ok(Sent::Unreached),
            Delivered::Unseen => Ok(Sent::Unseen),
            Delivered::Receipt(row) => Ok(Sent::Answer(row)),
        }
    }
}

pub(crate) fn place(op: &PendingOp) -> (&str, &str) {
    match op {
        PendingOp::Create {
            parent_id, name, ..
        }
        | PendingOp::Replace {
            parent_id, name, ..
        } => (parent_id, name),
    }
}

/// Whether two puts write the same place: one entry, or one name (under
/// the fold) in one folder.
fn same_place(a: &PendingOp, b: &PendingOp) -> bool {
    match (a, b) {
        (
            PendingOp::Create {
                parent_id: parent_a,
                name: name_a,
                ..
            },
            PendingOp::Create {
                parent_id: parent_b,
                name: name_b,
                ..
            },
        ) => {
            parent_a == parent_b
                && keeper_paths::names::name_fold(name_a) == keeper_paths::names::name_fold(name_b)
        }
        (PendingOp::Replace { entry_id: a, .. }, PendingOp::Replace { entry_id: b, .. }) => a == b,
        _ => false,
    }
}
