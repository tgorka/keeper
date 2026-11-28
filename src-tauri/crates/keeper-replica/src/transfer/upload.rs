//! Blocking file upload to presigned store URLs (story 115.3).
//!
//! Reads the source file once in bounded chunks to compute its content id and
//! per-part SHA-256 digests, persists a device-local plan file for multipart
//! uploads so cuts resume from held parts, checks every presigned URL against
//! the announced origin rule before contacting it, and streams each body range
//! directly from disk.

use std::collections::{HashMap, HashSet};
use std::fs::{self, File};
use std::io::{self, Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::time::UNIX_EPOCH;

use base64::engine::general_purpose::STANDARD as BASE64;
use base64::Engine as _;
use keeper_ports::transfer::{
    Completion, PartDigest, Progress, SignedUrl, UploadAsk, UploadGrant, UploadTarget, UploadUse,
    Uploaded,
};
use keeper_ports::ReplicaError;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use super::StoreTransfer;

/// Buffer size for the single-pass digest scan (1 MiB).
const SCAN_CHUNK_BYTES: usize = 1024 * 1024;

/// Server error code when a persisted multipart plan is no longer known.
const PLAN_UNKNOWN: &str = "files.plan_unknown";

/// Error code when the object store refuses a PUT.
const PART_REFUSED: &str = "files.part_refused";

/// Error code when the server omits a requested part URL.
const PART_URL_MISSING: &str = "files.part_url_missing";

/// Error code for a transport-level failure (cut, timeout, reset).
const TRANSPORT_FAILED: &str = "transport.failed";

/// Whole-file and per-part SHA-256 digests computed in one pass.
#[derive(Debug, Clone, PartialEq, Eq)]
struct FileDigest {
    hex: String,
    content_id: String,
    whole_b64: String,
    size: u64,
    parts: Vec<PartDigest>,
    file_len: u64,
    file_modified_ms: i64,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
enum PlanPhase {
    #[default]
    Uploading,
    Completing,
    // The owner completion was ambiguous, and a target-bound grant did not attest presence.
    // Do not mint another upload plan every time this queued operation is retried.
    Unconfirmed,
}

/// Device-local multipart upload plan persisted under `plan_dir`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct PlanFile {
    #[serde(default)]
    phase: PlanPhase,
    content_id: String,
    size: u64,
    part_bytes: u64,
    plan_id: String,
    parts: Vec<String>,
    file_len: u64,
    file_modified_ms: i64,
}

/// Reader wrapper counting bytes handed to an HTTP request.
struct CountingReader<R> {
    inner: R,
    count: u64,
}

impl<R: Read> Read for CountingReader<R> {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        let n = self.inner.read(buf)?;
        self.count += n as u64;
        Ok(n)
    }
}

impl PlanFile {
    fn new(digest: &FileDigest, part_bytes: u64, plan_id: String, phase: PlanPhase) -> Self {
        Self {
            phase,
            content_id: digest.content_id.clone(),
            size: digest.size,
            part_bytes,
            plan_id,
            parts: digest
                .parts
                .iter()
                .map(|part| part.sha256.clone())
                .collect(),
            file_len: digest.file_len,
            file_modified_ms: digest.file_modified_ms,
        }
    }
}

/// Upload `file` to the database's object store using presigned URLs.
pub(super) fn run(
    t: &StoreTransfer,
    file: &Path,
    media_type: Option<&str>,
    scope: Option<(UploadTarget<'_>, UploadUse)>,
    progress: &mut dyn FnMut(Progress),
) -> Result<Uploaded, ReplicaError> {
    let meta = fs::metadata(file)
        .map_err(|error| ReplicaError::Client(format!("stat {}: {error}", file.display())))?;
    let part_bytes = t.config.part_size(meta.len());
    let digest = digest_file(file, part_bytes)?;

    let target = scope.map(|(target, _)| target);
    let proposal = scope.is_some_and(|(_, usage)| usage == UploadUse::Proposal);
    let ask = UploadAsk {
        target,
        content_id: &digest.content_id,
        size: digest.size,
        media_type,
        part_bytes,
        parts: &digest.parts,
    };
    let plan_path = (part_bytes > 0 || proposal)
        .then(|| plan_file_path(&t.config.plan_dir, &digest.hex, target));
    if let Some(path) = plan_path.as_deref() {
        if let Some(mut plan) = load_matching_plan(path, &digest, part_bytes) {
            if plan.phase != PlanPhase::Uploading {
                confirm_placement(t, &ask, &mut plan, path)?;
                return Ok(Uploaded {
                    content_id: digest.content_id,
                    size: digest.size,
                    bytes_sent: 0,
                    parts_sent: 0,
                    parts_skipped: u32::try_from(digest.parts.len()).unwrap_or(u32::MAX),
                    completion: Some(Completion::Placed),
                });
            }
            match t.grants.held_parts(&plan.plan_id) {
                Ok(held) => {
                    let mut held_set = HashSet::new();
                    let mut done = 0u64;
                    for part in held {
                        if part.number >= 1 && (part.number as usize) <= digest.parts.len() {
                            let (_, expected_len) =
                                part_range(digest.size, part_bytes, part.number);
                            if part.size == expected_len && held_set.insert(part.number) {
                                done += expected_len;
                            }
                        }
                    }

                    let missing: Vec<u32> = digest
                        .parts
                        .iter()
                        .map(|p| p.number)
                        .filter(|n| !held_set.contains(n))
                        .collect();

                    let mut url_map = HashMap::new();
                    if !missing.is_empty() {
                        for part_url in t.grants.part_urls(&plan.plan_id, &missing)? {
                            url_map.insert(part_url.number, part_url.url);
                        }
                    }

                    return finish_multipart(
                        t, file, &digest, &ask, &mut plan, path, &held_set, done, url_map, progress,
                    );
                }
                Err(ReplicaError::Remote { ref code, .. }) if code == PLAN_UNKNOWN => {
                    remove_plan_file(path);
                }
                Err(error) => return Err(error),
            }
        }
    }

    match t.grants.grant_upload(&ask)? {
        UploadGrant::Present => Ok(Uploaded {
            content_id: digest.content_id,
            size: digest.size,
            bytes_sent: 0,
            parts_sent: 0,
            parts_skipped: 0,
            completion: None,
        }),
        UploadGrant::Single { signed, plan_id } => {
            if proposal && plan_id.is_none() {
                return Err(ReplicaError::Client(
                    "proposal upload lacks a staging plan".to_owned(),
                ));
            }
            let url = t.checked(&signed.url)?;
            let (status, bytes_sent) = put_range(t, &url, file, 0, digest.size, &digest.whole_b64)?;
            if status != 200 {
                return Err(ReplicaError::Remote {
                    code: PART_REFUSED.to_owned(),
                    message: format!("the store refused the upload: {status}"),
                    retryable: false,
                });
            }
            progress(Progress {
                done: digest.size,
                total: digest.size,
            });
            // Direct naming reads back an open single plan. A nested proposal does not.
            let completion =
                if let (true, Some(id), Some(path)) = (proposal, plan_id, plan_path.as_deref()) {
                    let mut plan = PlanFile::new(&digest, 0, id, PlanPhase::Completing);
                    write_plan_file(&t.config.plan_dir, path, &plan)?;
                    confirm_placement(t, &ask, &mut plan, path)?;
                    Some(Completion::Placed)
                } else {
                    None
                };
            Ok(Uploaded {
                content_id: digest.content_id,
                size: digest.size,
                bytes_sent,
                parts_sent: 0,
                parts_skipped: 0,
                completion,
            })
        }
        UploadGrant::Parts { plan_id, urls } => {
            let plan_path = plan_path.ok_or_else(|| {
                ReplicaError::Client("unexpected multipart grant for single PUT".to_owned())
            })?;
            let mut plan = PlanFile::new(&digest, part_bytes, plan_id, PlanPhase::Uploading);
            write_plan_file(&t.config.plan_dir, &plan_path, &plan)?;

            let mut url_map: HashMap<u32, SignedUrl> =
                urls.into_iter().map(|p| (p.number, p.url)).collect();
            let missing: Vec<u32> = digest
                .parts
                .iter()
                .map(|p| p.number)
                .filter(|n| !url_map.contains_key(n))
                .collect();
            if !missing.is_empty() {
                for part_url in t.grants.part_urls(&plan.plan_id, &missing)? {
                    url_map.insert(part_url.number, part_url.url);
                }
            }

            let held_set = HashSet::new();
            finish_multipart(
                t, file, &digest, &ask, &mut plan, &plan_path, &held_set, 0, url_map, progress,
            )
        }
    }
}

/// Send remaining multipart parts, complete the plan, and remove the plan file.
#[allow(clippy::too_many_arguments)]
fn finish_multipart(
    t: &StoreTransfer,
    file: &Path,
    digest: &FileDigest,
    ask: &UploadAsk<'_>,
    plan: &mut PlanFile,
    plan_path: &Path,
    held_set: &HashSet<u32>,
    mut done: u64,
    url_map: HashMap<u32, SignedUrl>,
    progress: &mut dyn FnMut(Progress),
) -> Result<Uploaded, ReplicaError> {
    let part_bytes = ask.part_bytes;
    let plan_id = &plan.plan_id;
    let mut bytes_sent = 0u64;
    let mut parts_sent = 0u32;
    let parts_skipped = u32::try_from(held_set.len()).unwrap_or(u32::MAX);

    for part in &digest.parts {
        if held_set.contains(&part.number) {
            continue;
        }
        let (offset, len) = part_range(digest.size, part_bytes, part.number);
        let signed = url_map
            .get(&part.number)
            .ok_or_else(|| ReplicaError::Remote {
                code: PART_URL_MISSING.to_owned(),
                message: format!("the server did not sign part {}", part.number),
                retryable: false,
            })?;

        let url = t.checked(&signed.url)?;
        let (mut status, sent) = put_range(t, &url, file, offset, len, &part.sha256)?;
        bytes_sent += sent;

        if status == 403 {
            let fresh = t.grants.part_urls(plan_id, &[part.number])?;
            let retry = fresh
                .into_iter()
                .find(|p| p.number == part.number)
                .ok_or_else(|| ReplicaError::Remote {
                    code: PART_URL_MISSING.to_owned(),
                    message: format!("the server did not sign part {}", part.number),
                    retryable: false,
                })?;
            let retry_url = t.checked(&retry.url.url)?;
            let (retry_status, retry_sent) =
                put_range(t, &retry_url, file, offset, len, &part.sha256)?;
            bytes_sent += retry_sent;
            status = retry_status;
        }

        if status != 200 {
            return Err(ReplicaError::Remote {
                code: PART_REFUSED.to_owned(),
                message: format!("the store refused part {}: {status}", part.number),
                retryable: false,
            });
        }

        parts_sent += 1;
        done += len;
        progress(Progress {
            done,
            total: digest.size,
        });
    }

    if parts_sent == 0 && done > 0 {
        progress(Progress {
            done,
            total: digest.size,
        });
    }

    let completion = if ask.target.is_some() {
        // Persist before sending complete: a lost response must resume confirmation, not the PUTs.
        plan.phase = PlanPhase::Completing;
        write_plan_file(&t.config.plan_dir, plan_path, plan)?;
        confirm_placement(t, ask, plan, plan_path)?;
        Completion::Placed
    } else {
        let completion = t.grants.complete(plan_id)?;
        remove_plan_file(plan_path);
        completion
    };

    Ok(Uploaded {
        content_id: digest.content_id.clone(),
        size: digest.size,
        bytes_sent,
        parts_sent,
        parts_skipped,
        completion: Some(completion),
    })
}

/// `verifying` is not permission to name the bytes. Once placed, some servers hide the
/// plan with the same 404 as an absent plan. Only an authorized `present` can disambiguate it.
fn confirm_placement(
    t: &StoreTransfer,
    ask: &UploadAsk<'_>,
    plan: &mut PlanFile,
    path: &Path,
) -> Result<(), ReplicaError> {
    for delay_ms in [0, 25, 100, 250] {
        if delay_ms != 0 {
            std::thread::sleep(std::time::Duration::from_millis(delay_ms));
        }
        match t.grants.complete(&plan.plan_id) {
            Ok(Completion::Placed) => {
                remove_plan_file(path);
                return Ok(());
            }
            Ok(Completion::Verifying) => continue,
            Err(ReplicaError::Remote { code, .. }) if code == PLAN_UNKNOWN => {
                if plan.phase != PlanPhase::Unconfirmed {
                    if matches!(t.grants.grant_upload(ask)?, UploadGrant::Present) {
                        remove_plan_file(path);
                        return Ok(());
                    }
                    plan.phase = PlanPhase::Unconfirmed;
                    write_plan_file(&t.config.plan_dir, path, plan)?;
                }
                break;
            }
            Err(error) => return Err(error),
        }
    }
    Err(ReplicaError::Remote {
        code: keeper_ports::transfer::PLACEMENT_PENDING.to_owned(),
        message: format!("placement of upload plan {} is not confirmed", plan.plan_id),
        retryable: true,
    })
}

/// Stream `len` bytes from `file` at `offset` in one presigned `PUT`.
fn put_range(
    t: &StoreTransfer,
    url: &url::Url,
    file: &Path,
    offset: u64,
    len: u64,
    checksum_b64: &str,
) -> Result<(u16, u64), ReplicaError> {
    let req = t
        .agent
        .put(url.as_str())
        .header("content-length", len.to_string())
        .header("x-amz-checksum-sha256", checksum_b64);

    if len == 0 {
        let response = req.send(ureq::SendBody::none()).map_err(transport_error)?;
        return Ok((response.status().as_u16(), 0));
    }

    let mut handle = File::open(file)
        .map_err(|error| ReplicaError::Client(format!("open {}: {error}", file.display())))?;
    if offset > 0 {
        handle
            .seek(SeekFrom::Start(offset))
            .map_err(|error| ReplicaError::Client(format!("seek {}: {error}", file.display())))?;
    }
    let mut reader = CountingReader {
        inner: handle.take(len),
        count: 0,
    };
    let response = req
        .send(ureq::SendBody::from_reader(&mut reader))
        .map_err(transport_error)?;
    Ok((response.status().as_u16(), reader.count))
}

fn transport_error(error: ureq::Error) -> ReplicaError {
    ReplicaError::Unheard {
        code: TRANSPORT_FAILED.to_owned(),
        message: error.to_string(),
        retryable: true,
    }
}

/// Read `file` once in 1 MiB chunks, computing whole-file and per-part SHA-256.
fn digest_file(file: &Path, part_bytes: u64) -> Result<FileDigest, ReplicaError> {
    let mut handle = File::open(file)
        .map_err(|error| ReplicaError::Client(format!("open {}: {error}", file.display())))?;
    let meta = handle
        .metadata()
        .map_err(|error| ReplicaError::Client(format!("stat {}: {error}", file.display())))?;
    let file_len = meta.len();
    let file_modified_ms = modified_ms(&meta)?;

    let mut buf = vec![0u8; SCAN_CHUNK_BYTES];
    let mut whole = Sha256::new();
    let mut part_hasher = Sha256::new();
    let mut part_pos = 0u64;
    let mut part_num = 1u32;
    let mut parts = Vec::new();
    let mut total_read = 0u64;

    loop {
        let n = handle
            .read(&mut buf)
            .map_err(|error| ReplicaError::Client(format!("read {}: {error}", file.display())))?;
        if n == 0 {
            break;
        }
        let mut slice = &buf[..n];
        whole.update(slice);
        total_read += n as u64;

        if part_bytes > 0 {
            while !slice.is_empty() {
                let rem = usize::try_from(part_bytes - part_pos).unwrap_or(usize::MAX);
                let take = slice.len().min(rem);
                part_hasher.update(&slice[..take]);
                part_pos += take as u64;
                slice = &slice[take..];
                if part_pos == part_bytes {
                    let sum = std::mem::take(&mut part_hasher).finalize();
                    parts.push(PartDigest {
                        number: part_num,
                        sha256: BASE64.encode(sum),
                    });
                    part_num += 1;
                    part_pos = 0;
                }
            }
        }
    }

    if part_bytes > 0 && part_pos > 0 {
        let sum = part_hasher.finalize();
        parts.push(PartDigest {
            number: part_num,
            sha256: BASE64.encode(sum),
        });
    }

    let whole_sum = whole.finalize();
    let hex = hex::encode(whole_sum);
    let content_id = format!("sha256:{hex}");
    let whole_b64 = BASE64.encode(whole_sum);

    Ok(FileDigest {
        hex,
        content_id,
        whole_b64,
        size: total_read,
        parts,
        file_len,
        file_modified_ms,
    })
}

/// Byte offset and length of 1-based part `number`.
fn part_range(size: u64, part_bytes: u64, number: u32) -> (u64, u64) {
    let offset = u64::from(number - 1) * part_bytes;
    let len = part_bytes.min(size.saturating_sub(offset));
    (offset, len)
}

fn modified_ms(meta: &fs::Metadata) -> Result<i64, ReplicaError> {
    let modified = meta
        .modified()
        .map_err(|error| ReplicaError::Client(format!("stat mtime: {error}")))?;
    let ms = match modified.duration_since(UNIX_EPOCH) {
        Ok(d) => i64::try_from(d.as_millis()).unwrap_or(i64::MAX),
        Err(e) => -i64::try_from(e.duration().as_millis()).unwrap_or(i64::MAX),
    };
    Ok(ms)
}

fn plan_file_path(plan_dir: &Path, hex: &str, target: Option<UploadTarget<'_>>) -> PathBuf {
    // A plan belongs to the grant issued for its target, not just to the bytes.
    let Some(target) = target else {
        return plan_dir.join(format!("{hex}.plan.json"));
    };
    let mut key = Sha256::new();
    match target {
        UploadTarget::Create { parent_id, name } => {
            key.update(b"create");
            key.update((parent_id.len() as u64).to_be_bytes());
            key.update(parent_id);
            key.update(name);
        }
        UploadTarget::Replace { entry_id } => {
            key.update(b"replace");
            key.update(entry_id);
        }
    }
    plan_dir.join(format!("{hex}.{:x}.plan.json", key.finalize()))
}

/// Load a persisted plan if it exists and still matches the file on disk.
/// Stale or unreadable plan files are removed.
fn load_matching_plan(plan_path: &Path, digest: &FileDigest, part_bytes: u64) -> Option<PlanFile> {
    let raw = match fs::read(plan_path) {
        Ok(bytes) => bytes,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return None,
        Err(error) => {
            tracing::warn!(path = %plan_path.display(), %error, "discarding unreadable upload plan");
            remove_plan_file(plan_path);
            return None;
        }
    };
    let plan: PlanFile = match serde_json::from_slice(&raw) {
        Ok(plan) => plan,
        Err(error) => {
            tracing::warn!(path = %plan_path.display(), %error, "discarding malformed upload plan");
            remove_plan_file(plan_path);
            return None;
        }
    };
    let matches = plan.content_id == digest.content_id
        && plan.size == digest.size
        && plan.part_bytes == part_bytes
        && plan.file_len == digest.file_len
        && plan.file_modified_ms == digest.file_modified_ms
        && plan.parts.len() == digest.parts.len();
    if !matches {
        remove_plan_file(plan_path);
        return None;
    }
    Some(plan)
}

fn write_plan_file(plan_dir: &Path, plan_path: &Path, plan: &PlanFile) -> Result<(), ReplicaError> {
    fs::create_dir_all(plan_dir).map_err(|error| {
        ReplicaError::Client(format!("create plan dir {}: {error}", plan_dir.display()))
    })?;
    let bytes = serde_json::to_vec(plan)
        .map_err(|error| ReplicaError::Client(format!("encode upload plan: {error}")))?;
    let tmp_path = plan_path.with_extension("json.tmp");
    fs::write(&tmp_path, bytes).map_err(|error| {
        ReplicaError::Client(format!("write upload plan {}: {error}", tmp_path.display()))
    })?;
    fs::rename(&tmp_path, plan_path).map_err(|error| {
        ReplicaError::Client(format!(
            "commit upload plan {}: {error}",
            plan_path.display()
        ))
    })?;
    Ok(())
}

fn remove_plan_file(plan_path: &Path) {
    if let Err(error) = fs::remove_file(plan_path) {
        if error.kind() != io::ErrorKind::NotFound {
            tracing::warn!(path = %plan_path.display(), %error, "failed to remove upload plan");
        }
    }
}

#[cfg(test)]
mod tests {
    use std::io::Write;

    use super::*;

    fn sha256_b64(bytes: &[u8]) -> String {
        BASE64.encode(Sha256::digest(bytes))
    }

    fn sha256_hex(bytes: &[u8]) -> String {
        hex::encode(Sha256::digest(bytes))
    }

    fn write_temp(bytes: &[u8]) -> tempfile::NamedTempFile {
        let mut f = tempfile::NamedTempFile::new().expect("create temp file");
        f.write_all(bytes).expect("write temp file");
        f.flush().expect("flush temp file");
        f
    }

    #[test]
    fn digest_file_part_boundaries() {
        let part_bytes = 4u64;
        for size in [0usize, 3, 4, 5] {
            let data: Vec<u8> = (0..size)
                .map(|i| (i as u8).wrapping_mul(31).wrapping_add(7))
                .collect();
            let tmp = write_temp(&data);
            let got = digest_file(tmp.path(), part_bytes).expect("digest temp file");

            assert_eq!(got.size, size as u64);
            assert_eq!(got.file_len, size as u64);
            assert_eq!(got.hex, sha256_hex(&data));
            assert_eq!(got.content_id, format!("sha256:{}", sha256_hex(&data)));
            assert_eq!(got.whole_b64, sha256_b64(&data));

            let expected_parts: Vec<PartDigest> = data
                .chunks(part_bytes as usize)
                .enumerate()
                .map(|(idx, chunk)| PartDigest {
                    number: (idx + 1) as u32,
                    sha256: sha256_b64(chunk),
                })
                .collect();
            assert_eq!(got.parts, expected_parts, "size={size}");
        }
    }

    #[test]
    fn digest_file_single_put_has_no_parts() {
        let data = b"hello world";
        let tmp = write_temp(data);
        let got = digest_file(tmp.path(), 0).expect("digest temp file");
        assert_eq!(got.size, data.len() as u64);
        assert!(got.parts.is_empty());
        assert_eq!(got.content_id, format!("sha256:{}", sha256_hex(data)));
        assert_eq!(got.whole_b64, sha256_b64(data));
    }

    #[test]
    fn part_range_boundaries() {
        assert_eq!(part_range(3, 4, 1), (0, 3));
        assert_eq!(part_range(4, 4, 1), (0, 4));
        assert_eq!(part_range(5, 4, 1), (0, 4));
        assert_eq!(part_range(5, 4, 2), (4, 1));
    }

    #[test]
    fn plan_file_json_uses_contract_camel_case_keys() {
        let plan = PlanFile {
            phase: PlanPhase::Uploading,
            content_id: "sha256:abc".to_owned(),
            size: 10,
            part_bytes: 4,
            plan_id: "p1".to_owned(),
            parts: vec!["a==".to_owned(), "b==".to_owned()],
            file_len: 10,
            file_modified_ms: 1234,
        };
        let val = serde_json::to_value(&plan).expect("serialize plan");
        assert_eq!(val["contentId"], "sha256:abc");
        assert_eq!(val["size"], 10);
        assert_eq!(val["partBytes"], 4);
        assert_eq!(val["planId"], "p1");
        assert_eq!(val["parts"][0], "a==");
        assert_eq!(val["fileLen"], 10);
        assert_eq!(val["fileModifiedMs"], 1234);
    }
}
