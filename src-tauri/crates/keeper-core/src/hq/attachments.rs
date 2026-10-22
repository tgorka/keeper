//! A file or photo in an agents session (FR-857, FR-858; AD-429, AD-433):
//! staged on this device, uploaded as a syncular blob of the session's own
//! database before anything references it, and referenced by `attachments`
//! rows committed in one commit with the message whose text is the caption.
//!
//! - **Staging.** By path (Rust reads the file; the composer's paperclip and
//!   drag-and-drop) or by bytes (a paste, which crosses IPC as a raw body).
//!   A file over the contract's bound ([`ATTACHMENT_MAX_BYTES`], read from
//!   `syncular/limits.json`) is refused here, with today's sentence
//!   ([`too_large`]), before anything is uploaded or written. A staged file
//!   waits in this process under a handle ([`Staging`]) — not in the
//!   replica, so nothing of it can reach the server — until the composer's
//!   one Send commits every handle of its selection, with the caption, as
//!   one message ([`Staging::take`], [`attach`]); a handle never committed is
//!   released, and a process that dies holding one leaves nothing behind.
//! - **Upload before reference.** The replica caches the bytes under their
//!   content address (`sha256:<hex>`) and queues the upload; its next round
//!   uploads every queued blob (`PUT /<database>/blobs/<blobId>`) before it
//!   pushes the commit that names them (SPEC §5.9 B4). The staged blob is
//!   pinned in the replica file until the server took the commit, so it
//!   survives a relaunch offline, and the same bytes are one blob however
//!   often they are attached.
//! - **The chips.** Every attachment of a message is a chip
//!   ([`AgentAttachmentVm`]): *uploading* while this device's commit is on
//!   its way to a server it can reach, *waiting* while it cannot reach one,
//!   *sent* once the server has it (always, for anyone else's), *refused*
//!   with the server's reason as a sentence ([`refusal_sentence`]). A chip
//!   opens through the `keeper-hq-blob://` scheme ([`blob_url`]), which serves
//!   the bytes from the replica's cache and downloads them on the first open
//!   — never before.

use std::collections::HashMap;
use std::io::Read;
use std::path::Path;
use std::sync::Mutex;

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use ts_rs::TS;

use crate::asset_url::{self, UrlForm};
use crate::hq::port::{BlobBody, LinkState, Rejection, ReplicaError, RowOp, UploadRefusal};
use crate::hq::send::activity;
use crate::hq::sessions::{
    authored, int, now_ms, object, person_message, read, text, AgentSessionRef, Source,
};
use crate::hq::timeline::session_row;

/// The contract's bounds, `syncular/limits.json`, compiled into keeper.
const LIMITS: &str = include_str!("../../../../../syncular/limits.json");

/// The most bytes one attachment may have: the contract's
/// `attachmentMaxBytes`, which the server's validator reads too, so the
/// device and the server cannot disagree. A malformed `limits.json` fails
/// the build.
pub const ATTACHMENT_MAX_BYTES: u64 = contract_number(LIMITS, b"\"attachmentMaxBytes\"");

/// The whole number `key` (with its quotes) names in the JSON `text`.
const fn contract_number(text: &str, key: &[u8]) -> u64 {
    let text = text.as_bytes();
    let mut at = 0;
    while at + key.len() <= text.len() {
        let mut matched = 0;
        while matched < key.len() && text[at + matched] == key[matched] {
            matched += 1;
        }
        if matched == key.len() {
            let mut i = at + matched;
            while i < text.len() && matches!(text[i], b' ' | b'\t' | b'\r' | b'\n' | b':') {
                i += 1;
            }
            let mut value: u64 = 0;
            let mut digits = 0;
            while i < text.len() && text[i].is_ascii_digit() {
                value = value * 10 + (text[i] - b'0') as u64;
                i += 1;
                digits += 1;
            }
            assert!(digits > 0, "syncular/limits.json: the key holds no number");
            return value;
        }
        at += 1;
    }
    panic!("syncular/limits.json does not name the key")
}

/// How a byte count reads in a sentence: whole kB below a megabyte, one
/// decimal of MB above — rounded up, in `u128`, so the largest size a
/// message may claim reads too (R444/08).
pub fn human(bytes: u64) -> String {
    const MB: u128 = 1024 * 1024;
    let bytes = u128::from(bytes);
    if bytes >= MB {
        let tenths = (bytes * 10).div_ceil(MB);
        format!("{}.{} MB", tenths / 10, tenths % 10)
    } else {
        format!("{} kB", bytes.div_ceil(1024))
    }
}

/// A file over [`ATTACHMENT_MAX_BYTES`]: the sentence the agent host says
/// of one, and the one the composer says before uploading anything.
pub fn too_large(name: &str, bytes: u64) -> String {
    format!(
        "{name} is {} and an agent takes at most {} from one message, so it did not get the file.",
        human(bytes),
        human(ATTACHMENT_MAX_BYTES)
    )
}

/// Why a file was not attached, as the person reads it.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum AttachError {
    #[error("Choose a file to attach.")]
    Nothing,
    /// [`too_large`]'s sentence.
    #[error("{0}")]
    TooLarge(String),
    #[error("keeper could not read {name}: {why}.")]
    Unreadable { name: String, why: String },
    #[error("This conversation is not on this device.")]
    Unknown,
    #[error("This conversation is archived; unarchive it to write in it.")]
    Archived,
    #[error("{0}")]
    Replica(#[from] ReplicaError),
    /// A handle the composer named is no longer staged here.
    #[error("A file of this message is no longer ready to send. Attach it again.")]
    Released,
}

/// A file staged for one message: within the bound by construction.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StagedFile {
    name: String,
    mime: String,
    bytes: Vec<u8>,
}

impl StagedFile {
    pub fn name(&self) -> &str {
        &self.name
    }
    pub fn mime(&self) -> &str {
        &self.mime
    }
    pub fn len(&self) -> u64 {
        self.bytes.len() as u64
    }
    pub fn is_empty(&self) -> bool {
        self.bytes.is_empty()
    }
}

/// The files staged for messages not sent yet, each under the handle the
/// composer holds for it and the session it was staged for. In memory only:
/// what is staged is in no replica, so nothing of it reaches a server, and a
/// process that dies leaves nothing to clean up.
#[derive(Debug, Default)]
pub struct Staging {
    staged: Mutex<HashMap<String, (AgentSessionRef, StagedFile)>>,
}

impl Staging {
    fn files(&self) -> std::sync::MutexGuard<'_, HashMap<String, (AgentSessionRef, StagedFile)>> {
        self.staged
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    /// Hold `file` for a message of `key`; its handle.
    pub fn stage(&self, key: &AgentSessionRef, file: StagedFile) -> String {
        let handle = ulid::Ulid::new().to_string();
        self.files().insert(handle.clone(), (key.clone(), file));
        handle
    }

    /// Every file `handles` name for `key`, in that order, taken out of
    /// staging — all of them, or none and [`AttachError::Released`] when one
    /// is not staged for `key`; the others named are released either way, so
    /// a Send that cannot commit leaves nothing staged behind it.
    pub fn take(
        &self,
        key: &AgentSessionRef,
        handles: &[String],
    ) -> Result<Vec<StagedFile>, AttachError> {
        let mut staged = self.files();
        let mut files = Vec::with_capacity(handles.len());
        let mut missing = false;
        for handle in handles {
            match staged.remove_entry(handle) {
                Some((_, (owner, file))) if owner == *key => files.push(file),
                // Another session's: not this Send's to take or release.
                Some((handle, held)) => {
                    staged.insert(handle, held);
                    missing = true;
                }
                None => missing = true,
            }
        }
        if missing {
            return Err(AttachError::Released);
        }
        if files.is_empty() {
            return Err(AttachError::Nothing);
        }
        Ok(files)
    }

    /// The composer let go of `handles` without sending them.
    pub fn release(&self, key: &AgentSessionRef, handles: &[String]) {
        let mut staged = self.files();
        for handle in handles {
            if staged.get(handle).is_some_and(|(owner, _)| owner == key) {
                staged.remove(handle);
            }
        }
    }

    /// `key` is no longer open anywhere: its composer, and every file it
    /// staged, are gone.
    pub fn release_session(&self, key: &AgentSessionRef) {
        self.files().retain(|_, (owner, _)| owner != key);
    }

    /// How many files are staged, for every session.
    pub fn len(&self) -> usize {
        self.files().len()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

/// A file's name as an attachment carries it: its last path segment, so a
/// name never names a folder.
fn file_name(raw: &str) -> String {
    let last = raw.rsplit(['/', '\\']).next().unwrap_or("").trim();
    if last.is_empty() || last == "." || last == ".." {
        "attachment".to_owned()
    } else {
        last.to_owned()
    }
}

/// Stage the file at `path`: refused over the bound by its size before a
/// byte is read, and at the first byte past it if it grew since.
pub fn stage_path(path: &Path) -> Result<StagedFile, AttachError> {
    let name = file_name(&path.to_string_lossy());
    let unreadable = |error: std::io::Error| AttachError::Unreadable {
        name: name.clone(),
        why: error.to_string(),
    };
    let metadata = std::fs::metadata(path).map_err(unreadable)?;
    if !metadata.is_file() {
        return Err(AttachError::Unreadable {
            name,
            why: "it is not a file".to_owned(),
        });
    }
    if metadata.len() > ATTACHMENT_MAX_BYTES {
        return Err(AttachError::TooLarge(too_large(&name, metadata.len())));
    }
    let mut bytes = Vec::with_capacity(usize::try_from(metadata.len()).unwrap_or(0));
    std::fs::File::open(path)
        .map_err(unreadable)?
        .take(ATTACHMENT_MAX_BYTES + 1)
        .read_to_end(&mut bytes)
        .map_err(unreadable)?;
    if bytes.len() as u64 > ATTACHMENT_MAX_BYTES {
        return Err(AttachError::TooLarge(too_large(&name, bytes.len() as u64)));
    }
    let mime = mime_guess::from_path(path)
        .first_or_octet_stream()
        .essence_str()
        .to_owned();
    Ok(StagedFile { name, mime, bytes })
}

/// Refuse `bytes` bytes named `name` when they pass the bound, with
/// [`too_large`]'s sentence. A caller that holds a body it has not copied yet
/// (the shell's raw IPC body) asks this first, so an oversized paste is
/// refused before anything allocates a second copy of it.
pub fn within_bound(name: &str, bytes: u64) -> Result<(), AttachError> {
    if bytes > ATTACHMENT_MAX_BYTES {
        return Err(AttachError::TooLarge(too_large(&file_name(name), bytes)));
    }
    Ok(())
}

/// Stage pasted `bytes` named `name`, of type `mime` (guessed from the name
/// when blank).
pub fn stage_bytes(name: &str, mime: &str, bytes: Vec<u8>) -> Result<StagedFile, AttachError> {
    within_bound(name, bytes.len() as u64)?;
    let name = file_name(name);
    let mime = match mime.trim() {
        "" => mime_guess::from_path(&name)
            .first_or_octet_stream()
            .essence_str()
            .to_owned(),
        given => given.to_owned(),
    };
    Ok(StagedFile { name, mime, bytes })
}

/// syncular's canonical BlobRef, in its key order.
#[derive(Serialize)]
struct BlobRef<'a> {
    #[serde(rename = "blobId")]
    blob_id: &'a str,
    #[serde(rename = "byteLength")]
    byte_length: u64,
    #[serde(rename = "mediaType", skip_serializing_if = "Option::is_none")]
    media_type: Option<&'a str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    name: Option<&'a str>,
}

/// The `blob_ref` column's value: canonical BlobRef JSON
/// `{blobId, byteLength, mediaType?, name?}`, in that key order.
pub fn blob_ref(
    blob_id: &str,
    byte_length: u64,
    media_type: Option<&str>,
    name: Option<&str>,
) -> String {
    serde_json::to_string(&BlobRef {
        blob_id,
        byte_length,
        media_type,
        name,
    })
    .unwrap_or_default()
}

/// A blob's content address: `sha256:` and its bytes' digest, in lowercase
/// hex — the id the replica stages it under.
fn content_address(bytes: &[u8]) -> String {
    use sha2::Digest;
    format!("sha256:{}", hex::encode(sha2::Sha256::digest(bytes)))
}

/// Attach `files` to a new message in `session_id` whose text is `caption`
/// (it may be empty): each blob staged for upload and the message, its
/// `attachments` rows and the session's last activity committed, in one step
/// of the replica's owner — so no round can upload a blob, and nothing can
/// evict it, before the commit that references it pins it. The message's
/// id.
pub fn attach(
    source: &Source,
    session_id: &str,
    caption: &str,
    files: Vec<StagedFile>,
) -> Result<String, AttachError> {
    if files.is_empty() {
        return Err(AttachError::Nothing);
    }
    let session = session_row(source, session_id).ok_or(AttachError::Unknown)?;
    if session.archived {
        return Err(AttachError::Archived);
    }
    let at = now_ms();
    let message_id = ulid::Ulid::new().to_string();
    let body = if caption.trim().is_empty() {
        ""
    } else {
        caption
    };
    let mut ops = vec![person_message(source, session_id, &message_id, body, at)];
    let mut blobs = Vec::with_capacity(files.len());
    for file in files {
        let StagedFile { name, mime, bytes } = file;
        let size = bytes.len() as u64;
        let blob_id = content_address(&bytes);
        ops.push(RowOp::Upsert {
            table: "attachments".to_owned(),
            values: authored(
                source,
                object(serde_json::json!({
                    "id": ulid::Ulid::new().to_string(),
                    "session_id": session_id,
                    "message_id": message_id,
                    "blob_ref": blob_ref(&blob_id, size, Some(&mime), Some(&name)),
                    "name": name,
                    "mime": mime,
                    "size": size,
                    "created_at_ms": at,
                })),
            ),
            base_version: None,
        });
        blobs.push(BlobBody {
            id: blob_id,
            bytes,
            media_type: Some(mime),
        });
    }
    ops.push(activity(source, session_id, at));
    source.replica.commit_with_blobs(blobs, ops)?;
    source.replica.sync_now();
    Ok(message_id)
}

/// Why the server refused `name` (or the message carrying it), as the person
/// reads it under the chip. Matched by the server's stable codes; any other
/// is said as the server said it.
pub fn refusal_sentence(rejection: &Rejection, name: &str) -> String {
    match rejection.code.as_str() {
        "blob.too_large" | "hq.attachment_size" => format!(
            "{name} is larger than the {} the agents server takes from one message, so the server refused it.",
            human(ATTACHMENT_MAX_BYTES)
        ),
        "blob.hash_mismatch" => format!(
            "The agents server found that {name}'s bytes changed on the way, so it refused them. Attach it again."
        ),
        "blob.not_found" => format!(
            "The agents server does not hold {name}, so it refused the message that names it. Attach it again."
        ),
        "blob.forbidden" => format!(
            "{name} belongs to a conversation you are not in, so the agents server will not give it to this device."
        ),
        "hq.attachment_writer" => format!(
            "Only someone in this conversation can attach a file to it, so the agents server refused {name}."
        ),
        "hq.attachment_message" => format!(
            "The agents server refused {name}: the message it was attached to is not in this conversation."
        ),
        "hq.attachment_blob_ref" => format!(
            "The agents server refused {name}: the message's reference to the file does not match its size."
        ),
        "hq.attachment_blob_size" => format!(
            "The agents server refused {name}: the file it holds is not the size the message says."
        ),
        "hq.v1.author_actor" | "hq.v1.author_client" => format!(
            "The agents server refused {name}: the message was written in another person's or device's name."
        ),
        crate::hq::port::CONFLICT => {
            format!("Another device changed this conversation first, so {name} was not sent.")
        }
        _ => format!("The agents server refused {name}: {rejection}."),
    }
}

/// Where an attachment stands.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub enum AgentAttachmentStateVm {
    /// Written here; on its way to a server this device reaches.
    Uploading,
    /// Written here; waiting for this device to reach the server.
    Waiting,
    /// The server has it.
    Sent,
    /// The server refused it; [`AgentAttachmentVm::sentence`] says why.
    Refused,
    /// Not sent, and not refused itself: the server refused another file of
    /// the same message, so the message went without that one and was
    /// refused whole. [`AgentAttachmentVm::sentence`] names that file.
    Blocked,
}

/// One attachment of a message: the chip.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct AgentAttachmentVm {
    pub id: String,
    pub name: String,
    pub mime: String,
    #[ts(type = "number")]
    pub size: u64,
    /// The size as a sentence says it (`2.4 MB`, `12 kB`).
    pub size_label: String,
    pub state: AgentAttachmentStateVm,
    /// Why the server refused it, or the file it was blocked by; only when
    /// `refused` or `blocked`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub sentence: Option<String>,
    /// Opens it: the `keeper-hq-blob://` URL, in this build's spelling. The
    /// first request downloads it; nothing is downloaded before.
    pub url: String,
}

const ATTACHMENTS: &str = "SELECT id, message_id, blob_ref, name, mime, size, author_client, \
     _sync_version AS version FROM attachments WHERE session_id = ?1 ORDER BY created_at_ms, id";

fn blob_id_of(blob_ref: &Value) -> Option<String> {
    let parsed = match blob_ref {
        Value::String(raw) => serde_json::from_str::<Value>(raw).ok()?,
        other => other.clone(),
    };
    parsed.get("blobId")?.as_str().map(str::to_owned)
}

fn chip(
    source: &Source,
    row: &Map<String, Value>,
    state: AgentAttachmentStateVm,
    sentence: Option<String>,
) -> Option<AgentAttachmentVm> {
    let blob_id = blob_id_of(row.get("blob_ref")?)?;
    let size = int(row, "size")
        .and_then(|n| u64::try_from(n).ok())
        .unwrap_or(0);
    Some(AgentAttachmentVm {
        id: text(row, "id").to_owned(),
        name: text(row, "name").to_owned(),
        mime: text(row, "mime").to_owned(),
        size,
        size_label: human(size),
        state,
        sentence,
        url: blob_url(&source.database, &blob_id),
    })
}

/// A file not sent because the server refused `refused` (other files of
/// the same message), as the person reads it under the chip.
pub fn blocked_sentence(name: &str, refused: &[String]) -> String {
    let others = match refused {
        [] => "another file".to_owned(),
        [one] => one.clone(),
        [init @ .., last] => format!("{} and {last}", init.join(", ")),
    };
    format!(
        "{name} was not sent: the agents server refused {others}, attached to the same message. Attach {name} again."
    )
}

/// How the server's refusals of uploads bear on one commit's rows: which
/// rows carry a refused blob (and why), and which rows' commit holds one.
struct UploadRefusals<'a> {
    refused: HashMap<&'a str, &'a UploadRefusal>,
    in_commit: HashMap<&'a str, &'a UploadRefusal>,
}

impl<'a> UploadRefusals<'a> {
    fn new(refusals: &'a [UploadRefusal]) -> UploadRefusals<'a> {
        let mut refused = HashMap::new();
        let mut in_commit = HashMap::new();
        for refusal in refusals {
            for row in &refusal.refused_rows {
                refused.insert(row.as_str(), refusal);
            }
            for row in &refusal.commit_rows {
                in_commit.insert(row.as_str(), refusal);
            }
        }
        UploadRefusals { refused, in_commit }
    }

    /// The state and sentence of the row `id` named `name`, when an upload
    /// refusal bears on it; `names` gives the name of a row by id, for a
    /// file of the same message that was refused.
    fn chip(
        &self,
        id: &str,
        name: &str,
        names: &HashMap<&str, &str>,
    ) -> Option<(AgentAttachmentStateVm, String)> {
        if let Some(refusal) = self.refused.get(id) {
            return Some((
                AgentAttachmentStateVm::Refused,
                refusal_sentence(&refusal.rejection, name),
            ));
        }
        let refusal = self.in_commit.get(id)?;
        let mut refused: Vec<String> = self
            .refused
            .iter()
            .filter(|(_, other)| other.commit_id == refusal.commit_id)
            .filter_map(|(row, _)| names.get(row).map(|name| (*name).to_owned()))
            .collect();
        refused.sort();
        refused.dedup();
        Some((
            AgentAttachmentStateVm::Blocked,
            blocked_sentence(name, &refused),
        ))
    }
}

/// The chips of every message of `session_id` the replica holds, by message
/// id, each message's in the order they were attached. This device's file
/// whose upload the server refused reads *refused* with the reason; another
/// file of that message *blocked*, naming it; every other file of this
/// device's not yet sent *uploading* or *waiting* — never refused for a
/// file it is not.
pub fn chips(source: &Source, session_id: &str) -> HashMap<String, Vec<AgentAttachmentVm>> {
    let rows = read(source, ATTACHMENTS, &[Value::from(session_id)]);
    if rows.is_empty() {
        return HashMap::new();
    }
    let reaching = matches!(
        source.replica.state().link,
        LinkState::Connecting | LinkState::Connected
    );
    let held = source.replica.upload_refusals();
    let refusals = UploadRefusals::new(&held);
    let names: HashMap<&str, &str> = rows
        .iter()
        .map(|row| (text(row, "id"), text(row, "name")))
        .collect();
    let mut by_message: HashMap<String, Vec<AgentAttachmentVm>> = HashMap::new();
    for row in &rows {
        let pending = text(row, "author_client") == source.client
            && int(row, "version").is_some_and(|version| version < 0);
        let refused = refusals.chip(text(row, "id"), text(row, "name"), &names);
        let (state, sentence) = match (refused, pending) {
            (_, false) => (AgentAttachmentStateVm::Sent, None),
            (Some((state, sentence)), true) => (state, Some(sentence)),
            (None, true) if reaching => (AgentAttachmentStateVm::Uploading, None),
            (None, true) => (AgentAttachmentStateVm::Waiting, None),
        };
        if let Some(chip) = chip(source, row, state, sentence) {
            by_message
                .entry(text(row, "message_id").to_owned())
                .or_default()
                .push(chip);
        }
    }
    by_message
}

/// A message of this device's whose commit the server refused, with its
/// attachments: gone from the replica, kept by its outcome journal.
#[derive(Debug, Clone, PartialEq)]
pub struct RefusedMessage {
    /// The `messages` row as it was written.
    pub values: Map<String, Value>,
    pub attachments: Vec<AgentAttachmentVm>,
}

/// The messages of `session_id` this device attached files to whose commit
/// the server refused, its refusal standing, each chip with the reason: a
/// file whose upload the server refused with that refusal, the message's
/// other files *blocked* by it.
pub fn refused_messages(source: &Source, session_id: &str) -> Vec<RefusedMessage> {
    let commits = match source.replica.refused_commits("attachments") {
        Ok(commits) => commits,
        Err(error) => {
            tracing::debug!(database = %source.database, %error, "refused attachments could not be read");
            return Vec::new();
        }
    };
    let held = source.replica.upload_refusals();
    let refusals = UploadRefusals::new(&held);
    let in_session = |values: &Map<String, Value>| text(values, "session_id") == session_id;
    commits
        .into_iter()
        .filter_map(|commit| {
            let values = commit
                .rows
                .iter()
                .find(|(table, values)| table == "messages" && in_session(values))
                .map(|(_, values)| values.clone())?;
            let files: Vec<&Map<String, Value>> = commit
                .rows
                .iter()
                .filter(|(table, values)| table == "attachments" && in_session(values))
                .map(|(_, values)| values)
                .collect();
            let names: HashMap<&str, &str> = files
                .iter()
                .map(|row| (text(row, "id"), text(row, "name")))
                .collect();
            let attachments = files
                .iter()
                .filter_map(|row| {
                    let (state, sentence) = refusals
                        .chip(text(row, "id"), text(row, "name"), &names)
                        .unwrap_or_else(|| {
                            (
                                AgentAttachmentStateVm::Refused,
                                refusal_sentence(&commit.rejection, text(row, "name")),
                            )
                        });
                    chip(source, row, state, Some(sentence))
                })
                .collect::<Vec<_>>();
            (!attachments.is_empty()).then_some(RefusedMessage {
                values,
                attachments,
            })
        })
        .collect()
}

/// The scheme a chip opens through.
pub const SCHEME: &str = "keeper-hq-blob";
/// Its fixed host.
const HOST: &str = "blob";

/// The URL that opens `blob_id` of `database`, in this build's spelling
/// (`keeper-hq-blob://blob/<database>/<blobId>`).
pub fn blob_url(database: &str, blob_id: &str) -> String {
    blob_url_in(UrlForm::HOST, database, blob_id)
}

/// [`blob_url`] in a given spelling.
pub fn blob_url_in(form: UrlForm, database: &str, blob_id: &str) -> String {
    let encode = |segment: &str| {
        percent_encoding::utf8_percent_encode(segment, percent_encoding::NON_ALPHANUMERIC)
            .to_string()
    };
    asset_url::compose(
        form,
        SCHEME,
        &format!("{HOST}/{}/{}", encode(database), encode(blob_id)),
    )
}

/// The database and blob a `keeper-hq-blob://` URL names, in either
/// spelling; `None` for anything else. A query or fragment is ignored.
pub fn parse_blob_url(raw: &str) -> Option<(String, String)> {
    let rest = asset_url::strip(raw, SCHEME)?;
    let rest = rest.split(['?', '#']).next()?;
    let mut segments = rest.split('/');
    if segments.next()? != HOST {
        return None;
    }
    let decode = |segment: &str| {
        percent_encoding::percent_decode_str(segment)
            .decode_utf8()
            .ok()
            .map(|s| s.into_owned())
            .filter(|s| !s.is_empty())
    };
    let database = decode(segments.next()?)?;
    let blob_id = decode(segments.next()?)?;
    if segments.next().is_some() || !is_blob_id(&blob_id) {
        return None;
    }
    Some((database, blob_id))
}

/// `sha256:` and 64 lowercase hex digits.
fn is_blob_id(blob_id: &str) -> bool {
    blob_id.strip_prefix("sha256:").is_some_and(|hex| {
        hex.len() == 64
            && hex
                .bytes()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
    })
}

/// The `Content-Type` a blob is served with: what the webview can show
/// as itself — pictures, sound, video, PDF, plain text — and everything
/// else as bytes to save, so no attachment ever runs as a page or a script
/// (HTML and SVG among them).
pub fn served_mime(mime: &str) -> &'static str {
    match mime.trim().to_ascii_lowercase().as_str() {
        "image/png" => "image/png",
        "image/jpeg" => "image/jpeg",
        "image/gif" => "image/gif",
        "image/webp" => "image/webp",
        "image/heic" => "image/heic",
        "image/heif" => "image/heif",
        "audio/mpeg" => "audio/mpeg",
        "audio/mp4" => "audio/mp4",
        "audio/x-m4a" => "audio/mp4",
        "audio/wav" | "audio/x-wav" => "audio/wav",
        "audio/ogg" => "audio/ogg",
        "video/mp4" => "video/mp4",
        "video/quicktime" => "video/quicktime",
        "video/webm" => "video/webm",
        "application/pdf" => "application/pdf",
        "text/plain" | "text/csv" | "text/markdown" | "application/json" | "application/yaml"
        | "text/yaml" => "text/plain; charset=utf-8",
        _ => "application/octet-stream",
    }
}

/// A blob opened through its chip.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OpenedBlob {
    pub bytes: Vec<u8>,
    /// [`served_mime`] of the attachment's type.
    pub mime: &'static str,
    pub name: String,
}

/// Why a blob was not opened.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum OpenError {
    /// No attachment this replica holds names it.
    #[error("No attachment here names this file.")]
    NotHere,
    /// The download failed: the server's refusal or the network.
    #[error("{0}")]
    Fetch(String),
}

/// Open `blob_id` of `source`'s database: only one an attachment row the
/// replica holds names, from the cache, or downloaded now — the first open.
pub fn open_blob(source: &Source, blob_id: &str) -> Result<OpenedBlob, OpenError> {
    let rows = read(
        source,
        "SELECT name, mime FROM attachments WHERE json_extract(blob_ref, '$.blobId') = ?1 LIMIT 1",
        &[Value::from(blob_id)],
    );
    let row = rows.first().ok_or(OpenError::NotHere)?;
    let name = text(row, "name").to_owned();
    let bytes = source.replica.fetch_blob(blob_id).map_err(|error| {
        let message = error.to_string();
        let code = message.split(':').next().unwrap_or_default().trim();
        if code.starts_with("blob.") {
            OpenError::Fetch(refusal_sentence(&Rejection::new(code, &message), &name))
        } else {
            OpenError::Fetch(format!("keeper could not download {name}: {message}"))
        }
    })?;
    Ok(OpenedBlob {
        bytes,
        mime: served_mime(text(row, "mime")),
        name,
    })
}

#[cfg(test)]
#[path = "attachments_tests.rs"]
mod tests;
