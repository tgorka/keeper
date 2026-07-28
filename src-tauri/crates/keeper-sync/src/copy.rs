//! One-time verified file copy (Story 33.1, AD-C1 … AD-C6).
//!
//! A copy here is a **job, never a relationship** (AD-C1): it walks a source
//! once, moves the bytes, reports what happened to each file, and changes
//! nothing about either folder afterwards. No profile, no journal, no state.
//!
//! # What makes it different from `cp`
//!
//! `write()` returning success is not evidence that the bytes are correct on
//! the other side, and every ordinary copy tool stops there. This one does not:
//! each file is hashed while it is streamed out, and the bytes that were
//! written are then **read back from disk and hashed again** (AD-C2). A file
//! counts as [`CopyOutcome::Copied`] only when that second, independent digest
//! matches. Verification is not optional; a copy that cannot be proven is a
//! failure, not a success with a caveat.
//!
//! Nothing partial is ever visible (AD-C3): bytes are staged into a temp file
//! **in the destination's own directory** — same filesystem, so the publishing
//! rename is atomic — and the temp is unlinked on drop, so any failure or
//! cancellation leaves the destination exactly as it was.
//!
//! An existing destination is never silently eaten (AD-C4): identical content
//! is reported as [`CopyOutcome::Identical`] and not rewritten, differing
//! content is reported as [`CopyOutcome::Collision`] and left alone, and
//! [`CopyOptions::replace_existing`] replaces the old file only *after* the new
//! bytes have passed verification.
//!
//! # Shape of the arguments
//!
//! `destination` is **always a directory**, created if missing. A directory
//! source copies its *contents* into it (`src/a/b` → `dst/a/b`); a file source
//! copies that one file into it (`src.txt` → `dst/src.txt`). There is no
//! is-this-a-file-or-a-folder guessing, and [`CopyEntry::path`] is always
//! source-relative so the report reads as the user's own tree.
//!
//! # What is carried, and what is not
//!
//! Carried: the bytes, the modification time, and the executable bit.
//!
//! **Not** carried, deliberately, and this module will not pretend otherwise:
//! extended attributes, ACLs, resource forks, ownership, creation time, or the
//! rest of the group/other permission bits. That is `ditto`/`rsync -X`
//! territory (Epic 33 "Deferred"). Symbolic links are **not followed** — a
//! followed link can escape the tree being copied or loop forever — and each
//! one is reported as [`CopyOutcome::Failed`] naming it, because a silently
//! missing file is worse than a refusal a user can see. The same goes for
//! FIFOs, sockets and device nodes (opening a FIFO would block forever) and for
//! macOS dataless iCloud placeholders (opening one silently materializes a
//! file that may be gigabytes).
//!
//! # Blocking
//!
//! [`copy_verified`] is **synchronous and long-running** — it is the whole
//! point that it hashes every byte twice. Async callers MUST run it on
//! `tokio::task::spawn_blocking`, the same rule [`crate::lfs::store`] carries.

use std::fs::File;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Instant;

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::error::{Result, SyncError};
use crate::lfs::basic::{ProgressCoalescer, DEFAULT_PROGRESS_INTERVAL};
use crate::stability::{is_dataless, FileSample};

/// Read granularity for every hashing and copying loop in this module.
///
/// Bounded on purpose (AD-C5, NFR-23): a one-time copy is precisely the
/// operation a user points at a 50 GB video, and sizing a buffer from the file
/// would make that a 50 GB allocation. 128 KiB is the same constant
/// [`crate::lfs::store`] uses, so the two byte-moving paths in this crate hold
/// the same amount at once and were measured against the same envelope.
const HASH_CHUNK_BYTES: usize = 128 * 1024;

/// What happened to one file.
///
/// Internally tagged like [`crate::engine::PendingReason`]: the UI reads this as
/// a discriminated union, and `kind` rather than `outcome` keeps it from
/// nesting as `outcome.outcome` inside a [`CopyEntry`]. `rename_all_fields` is
/// what reaches the payload of a struct variant — without it `reason` would be
/// fine but any future multi-word field would cross the boundary as snake_case
/// while every neighbouring field is camelCase.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(
    rename_all = "camelCase",
    rename_all_fields = "camelCase",
    tag = "kind"
)]
pub enum CopyOutcome {
    /// Written, published, and proven by an independent re-read (AD-C2).
    Copied,
    /// The destination already held byte-identical content, so nothing was
    /// written. Honest and fast: a re-run of a finished copy touches nothing.
    Identical,
    /// The destination held *different* content and was left exactly as it was.
    /// Only [`CopyOptions::replace_existing`] turns this into a replacement.
    Collision,
    /// Nothing was published. `reason` names the file's own problem — a refused
    /// symlink, a source that changed mid-read, a digest that did not match, an
    /// unreadable file — because a count of failures without reasons is not a
    /// report (AD-C6).
    Failed { reason: String },
}

/// One file's line in the report.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CopyEntry {
    /// Source-relative, so the report reads as the user's own tree.
    pub path: String,
    /// The source file's size. Present for every outcome — a collision the user
    /// has to think about is much easier to judge with the size in front of
    /// them. Refused entries (symlinks, device nodes) carry `0`: there are no
    /// file bytes to copy.
    pub bytes: u64,
    pub outcome: CopyOutcome,
}

/// The result of a whole job.
#[derive(Debug, Default, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CopyReport {
    /// One line per file, in the order the source tree was walked.
    pub entries: Vec<CopyEntry>,
    /// Bytes actually written and verified. Folded from `entries` at the end of
    /// the job rather than counted alongside them (AD-C6), so the summary
    /// cannot disagree with the lines it summarizes.
    pub bytes_copied: u64,
}

/// The one choice a job offers.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CopyOptions {
    /// Replace a destination file whose content differs — but only once the new
    /// bytes have passed verification (AD-C4). Defaults to `false`: the classic
    /// tool that eats the newer file is the failure mode this guards.
    pub replace_existing: bool,
}

/// One progress update for a job.
///
/// The totals are real: they come from a pre-walk of the source, not from a
/// running guess, so a bar never has to invent a denominator.
#[derive(Debug, Default, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CopyProgress {
    /// Entries decided so far. Reaches `files_total` on an uncancelled job.
    pub files_done: u64,
    /// Every entry the report will hold, refusals included.
    pub files_total: u64,
    /// Progress through the plan's bytes — monotonic, and not the same figure
    /// as [`CopyReport::bytes_copied`]: a file found identical or colliding
    /// still has to be read to reach that verdict.
    pub bytes_done: u64,
    /// Sum of the sizes of every regular file in the plan.
    pub bytes_total: u64,
    /// Source-relative path of the file in flight, `None` between files.
    pub current: Option<String>,
}

/// A sink for copy progress. `false` means the receiver is gone and stops it
/// being called again — matching [`crate::progress::ProgressSink`]'s contract.
pub type CopySink = Box<dyn Fn(CopyProgress) -> bool + Send + Sync>;

/// Copy `source` into the `destination` directory, proving every file.
///
/// Returns the per-file report. Cancellation is not an error: on cancel the
/// walk stops promptly and the report of what was already finished is returned,
/// with no temp file and no partial destination left behind.
///
/// Only a failure that makes the *job* impossible — an unreadable source root,
/// a destination directory that cannot be created — comes back as `Err`. A
/// single unreadable or unwritable file is one [`CopyOutcome::Failed`] entry,
/// never the end of a ten-thousand-file copy (AD-C6).
pub fn copy_verified(
    source: &Path,
    destination: &Path,
    options: &CopyOptions,
    progress: Option<&CopySink>,
    cancel: &AtomicBool,
) -> Result<CopyReport> {
    copy_verified_hooked(source, destination, options, progress, cancel, &mut |_| {})
}

/// Fired once per source file, right after its first chunk has been consumed.
///
/// The hook exists so the torn-read and mid-file-cancel invariants can be tested
/// deterministically: there is no other way to mutate a file *during* a
/// synchronous read, and a thread-and-sleep test of either would be flakier
/// than no test at all. This is the same escape hatch — and the same reasoning
/// — as `stability::verify_while_reading_hooked`. Production passes an empty
/// closure, which compiles away.
type ChunkHook<'a> = &'a mut dyn FnMut(&Path);

/// The body of [`copy_verified`], with a per-file first-chunk hook.
fn copy_verified_hooked(
    source: &Path,
    destination: &Path,
    options: &CopyOptions,
    progress: Option<&CopySink>,
    cancel: &AtomicBool,
    hook: ChunkHook<'_>,
) -> Result<CopyReport> {
    let plan = plan_copy(source)?;
    std::fs::create_dir_all(destination)
        .map_err(|err| SyncError::io("create copy destination", destination, err))?;

    let mut reporter = Reporter::new(progress, &plan);
    let mut report = CopyReport {
        entries: Vec::with_capacity(plan.files_total as usize),
        bytes_copied: 0,
    };

    // Publish the totals before any byte moves, so a surface never has to
    // render a bar whose denominator it does not have yet.
    reporter.emit(true);

    for item in &plan.items {
        if cancel.load(Ordering::Relaxed) {
            break;
        }
        match item {
            PlanItem::Dir { rel } => {
                // Fatal rather than a per-file failure: if a directory cannot be
                // created at the destination then nothing beneath it can be
                // written either, and N identical errors are not a report.
                let dir = destination.join(rel);
                std::fs::create_dir_all(&dir)
                    .map_err(|err| SyncError::io("create copy directory", dir, err))?;
            }
            PlanItem::Refused { rel, reason } => {
                report.entries.push(CopyEntry {
                    path: display(rel),
                    bytes: 0,
                    outcome: CopyOutcome::Failed {
                        reason: reason.clone(),
                    },
                });
                reporter.finish(0);
            }
            PlanItem::File { rel, bytes } => {
                reporter.begin(display(rel));
                let src = plan.root.join(rel);
                let dst = destination.join(rel);
                let step = match copy_one(&src, &dst, options, cancel, &mut reporter, hook) {
                    Ok(step) => step,
                    // One file's I/O problem is one line in the report.
                    Err(err) => Step::Done(CopyOutcome::Failed {
                        reason: err.to_string(),
                    }),
                };
                match step {
                    Step::Cancelled => break,
                    Step::Done(outcome) => {
                        report.entries.push(CopyEntry {
                            path: display(rel),
                            bytes: *bytes,
                            outcome,
                        });
                        reporter.finish(*bytes);
                    }
                }
            }
        }
    }

    report.bytes_copied = report
        .entries
        .iter()
        .filter(|entry| matches!(entry.outcome, CopyOutcome::Copied))
        .map(|entry| entry.bytes)
        .sum();
    reporter.emit(true);
    Ok(report)
}

/// Whether the walk should carry on.
enum Step {
    Done(CopyOutcome),
    /// The cancel flag was seen; nothing was published and nothing is staged.
    Cancelled,
}

/// Copy one regular file, deciding against whatever is already at `dst`.
fn copy_one(
    src: &Path,
    dst: &Path,
    options: &CopyOptions,
    cancel: &AtomicBool,
    reporter: &mut Reporter<'_>,
    hook: ChunkHook<'_>,
) -> Result<Step> {
    // Re-stat rather than trusting the pre-walk: between the plan and here the
    // file may have grown, and comparing the destination against a stale length
    // would report a collision for content that in fact matches.
    let source_meta = std::fs::symlink_metadata(src)
        .map_err(|err| SyncError::io("stat copy source", src, err))?;

    let existing = match std::fs::symlink_metadata(dst) {
        Ok(meta) => Some(meta),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => None,
        Err(err) => return Err(SyncError::io("stat copy destination", dst, err)),
    };

    let Some(existing) = existing else {
        return stage(src, dst, Publish::Fresh, cancel, reporter, hook);
    };

    if !existing.is_file() {
        // Never rename over a directory or a symlink: the first cannot be
        // replaced by a file at all, and the second would write through to
        // wherever it points, outside the destination the user chose.
        return Ok(Step::Done(CopyOutcome::Failed {
            reason: format!(
                "destination already exists and is {}",
                describe_kind(&existing)
            ),
        }));
    }

    if existing.len() != source_meta.len() {
        // A different length proves different content without reading a byte —
        // the cheap half of AD-C4, and the common case for a collision.
        if !options.replace_existing {
            return Ok(Step::Done(CopyOutcome::Collision));
        }
        return stage(src, dst, Publish::Replacing, cancel, reporter, hook);
    }

    // Same length: only digests can tell the two apart. The destination is
    // hashed first because it is the cheaper half to abandon — if it cannot be
    // read there is no point streaming the source at all.
    let Some((destination_digest, _)) = hash_written(dst, cancel)? else {
        return Ok(Step::Cancelled);
    };
    match stream_source(src, None, cancel, reporter, hook)? {
        Streamed::Cancelled => Ok(Step::Cancelled),
        Streamed::Changed { reason } => Ok(Step::Done(CopyOutcome::Failed { reason })),
        Streamed::Read { digest, .. } if digest == destination_digest => {
            // Nothing is written, so the destination's mtime, ctime and inode
            // all survive untouched — which is what makes a re-run cheap and
            // provably non-destructive.
            Ok(Step::Done(CopyOutcome::Identical))
        }
        Streamed::Read { .. } if !options.replace_existing => {
            Ok(Step::Done(CopyOutcome::Collision))
        }
        // The second read of the source is deliberate: paying for it here keeps
        // the identical case — a re-run over a whole tree — from having to
        // stage a full temp copy of every file just to discover it changed
        // nothing.
        Streamed::Read { .. } => stage(src, dst, Publish::Replacing, cancel, reporter, hook),
    }
}

/// Whether publishing this file will destroy something.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Publish {
    /// Nothing is at the destination.
    Fresh,
    /// A different file is there and the job was created with `replace`.
    Replacing,
}

/// Stream `src` into a temp file beside `dst`, prove the written bytes by
/// reading them back, and publish by rename.
///
/// **Where the verifying read happens depends on what is at risk.** With
/// nothing at the destination the file is published first and the *destination*
/// is re-read (AD-C2) — a mismatch simply deletes it, and there was never
/// anything to lose. When an existing file is about to be replaced the staged
/// temp is re-read *before* the rename instead (AD-C4): destroying a user's
/// file for bytes we have not yet proven is the one unrecoverable mistake this
/// module could make. Either way the bytes that end up at the destination have
/// been hashed twice, from two independent reads.
fn stage(
    src: &Path,
    dst: &Path,
    publish: Publish,
    cancel: &AtomicBool,
    reporter: &mut Reporter<'_>,
    hook: ChunkHook<'_>,
) -> Result<Step> {
    // The temp lives in the destination's OWN directory so the publishing
    // rename is same-filesystem and therefore atomic (AD-C3). A shared scratch
    // directory would silently degrade to a copy across a mount boundary — and
    // a copy job is exactly the operation that crosses mount boundaries.
    let parent = dst.parent().unwrap_or_else(|| Path::new("."));
    let mut staged = tempfile::NamedTempFile::new_in(parent)
        .map_err(|err| SyncError::io("create copy temp file", parent, err))?;
    let staged_path = staged.path().to_path_buf();

    let (digest, bytes, source_meta) = match stream_source(
        src,
        Some((staged.as_file_mut(), &staged_path)),
        cancel,
        reporter,
        hook,
    )? {
        // Dropping the temp unlinks it, so a cancelled or torn file leaves the
        // destination directory exactly as it was.
        Streamed::Cancelled => return Ok(Step::Cancelled),
        Streamed::Changed { reason } => return Ok(Step::Done(CopyOutcome::Failed { reason })),
        Streamed::Read {
            digest,
            bytes,
            meta,
        } => (digest, bytes, meta),
    };

    staged
        .flush()
        .map_err(|err| SyncError::io("flush copy temp file", &staged_path, err))?;
    // Push the bytes to the device before verifying them. Without this the
    // verifying read can be served entirely out of the page cache we just
    // filled, which proves the write path and nothing at all about the file
    // that will still be there tomorrow — and "it arrived intact" is the only
    // claim this feature makes.
    staged
        .as_file()
        .sync_all()
        .map_err(|err| SyncError::io("sync copy temp file", &staged_path, err))?;

    // Applied before publication so the file is never briefly visible with the
    // wrong mode or timestamp.
    carry_metadata(&source_meta, staged.as_file(), &staged_path)?;

    if publish == Publish::Replacing {
        let Some(written) = hash_written(&staged_path, cancel)? else {
            return Ok(Step::Cancelled);
        };
        if written.0 != digest || written.1 != bytes {
            return Ok(Step::Done(CopyOutcome::Failed {
                reason: mismatch((&digest, bytes), &written),
            }));
        }
        staged
            .persist(dst)
            .map_err(|err| SyncError::io("publish copied file", dst, err.error))?;
        return Ok(Step::Done(CopyOutcome::Copied));
    }

    staged
        .persist(dst)
        .map_err(|err| SyncError::io("publish copied file", dst, err.error))?;

    let Some(verified) = hash_written(dst, cancel)? else {
        // Cancelled mid-verification. The file is complete but unproven, and an
        // unproven file must not be left claiming to be a copy — nothing was
        // here before us, so removing it is safe and leaves the destination
        // clean.
        discard(dst);
        return Ok(Step::Cancelled);
    };
    if verified.0 != digest || verified.1 != bytes {
        discard(dst);
        return Ok(Step::Done(CopyOutcome::Failed {
            reason: mismatch((&digest, bytes), &verified),
        }));
    }
    Ok(Step::Done(CopyOutcome::Copied))
}

/// What a source read produced.
enum Streamed {
    Read {
        digest: String,
        /// The length the hasher itself saw. Compared alongside the digest when
        /// the written bytes are read back, which is the same both-or-nothing
        /// check `LfsStore::insert_verified` makes: a truncation that somehow
        /// preserved a digest would still be caught.
        bytes: u64,
        /// The source's metadata as `fstat` saw it *after* the read — the same
        /// sample the change detection compared, so the mode and mtime carried
        /// to the destination provably belong to the bytes we hashed.
        meta: std::fs::Metadata,
    },
    /// The source changed under the read, so the bytes hashed never existed as
    /// one coherent version of the file. Nothing may be published from them.
    Changed {
        reason: String,
    },
    Cancelled,
}

/// Read `path` in bounded chunks, hashing it and optionally writing it out.
///
/// `fstat`s the **open descriptor** before and after — not the path, because
/// re-stat-ing the path would reopen a TOCTOU window every time — exactly the
/// discipline `stability::verify_while_reading` uses. A size, mtime, ctime or
/// inode change across the read means the bytes are not a coherent version of
/// the file, and the entry fails rather than publishing a Frankenstein copy.
fn stream_source(
    path: &Path,
    mut out: Option<(&mut File, &Path)>,
    cancel: &AtomicBool,
    reporter: &mut Reporter<'_>,
    hook: ChunkHook<'_>,
) -> Result<Streamed> {
    let mut file = File::open(path).map_err(|err| SyncError::io("open copy source", path, err))?;
    let before = FileSample::from_metadata(
        &file
            .metadata()
            .map_err(|err| SyncError::io("fstat copy source", path, err))?,
    );

    let mut hasher = Sha256::new();
    let mut buf = vec![0u8; HASH_CHUNK_BYTES];
    let mut bytes: u64 = 0;
    let mut hooked = false;
    loop {
        if cancel.load(Ordering::Relaxed) {
            return Ok(Streamed::Cancelled);
        }
        let read = file
            .read(&mut buf)
            .map_err(|err| SyncError::io("read copy source", path, err))?;
        if read == 0 {
            break;
        }
        let chunk = &buf[..read];
        hasher.update(chunk);
        if let Some((sink, sink_path)) = out.as_mut() {
            sink.write_all(chunk)
                .map_err(|err| SyncError::io("write copy temp file", *sink_path, err))?;
        }
        bytes = bytes.saturating_add(read as u64);
        reporter.streamed(bytes);
        if !hooked {
            hooked = true;
            hook(path);
        }
    }

    let after_meta = file
        .metadata()
        .map_err(|err| SyncError::io("fstat copy source", path, err))?;
    let after = FileSample::from_metadata(&after_meta);
    if before != after {
        return Ok(Streamed::Changed {
            reason: format!(
                "the source changed while it was being read \
                 (size {} → {}, mtime_ns {} → {}, inode {} → {})",
                before.size, after.size, before.mtime_ns, after.mtime_ns, before.inode, after.inode
            ),
        });
    }

    Ok(Streamed::Read {
        digest: hex::encode(hasher.finalize()),
        bytes,
        meta: after_meta,
    })
}

/// Hash a file we are about to trust: the bytes just written, or the existing
/// destination being compared against.
///
/// `None` means the cancel flag was seen partway. Deliberately not
/// [`stream_source`]: there is no source to catch changing here, and an `fstat`
/// pair over a file whose mode and mtime we set ourselves would only re-report
/// our own writes.
fn hash_written(path: &Path, cancel: &AtomicBool) -> Result<Option<(String, u64)>> {
    let mut file =
        File::open(path).map_err(|err| SyncError::io("open for verification", path, err))?;
    let mut hasher = Sha256::new();
    let mut buf = vec![0u8; HASH_CHUNK_BYTES];
    let mut bytes: u64 = 0;
    loop {
        if cancel.load(Ordering::Relaxed) {
            return Ok(None);
        }
        let read = file
            .read(&mut buf)
            .map_err(|err| SyncError::io("read for verification", path, err))?;
        if read == 0 {
            break;
        }
        hasher.update(&buf[..read]);
        bytes = bytes.saturating_add(read as u64);
    }
    Ok(Some((hex::encode(hasher.finalize()), bytes)))
}

/// Carry the modification time and the executable bit onto the staged file.
///
/// The mode is set explicitly rather than copied wholesale: a temp file is born
/// `0600`, so leaving it alone would publish copies only their owner can read,
/// while copying the source's full mode would carry setuid, setgid and sticky
/// bits this module never promised to reproduce. The process umask is not
/// consulted — reading it portably needs `unsafe`, which the workspace denies,
/// and a copy that lands unreadable is a worse failure than one that ignores a
/// restrictive umask.
#[cfg(unix)]
fn carry_metadata(source: &std::fs::Metadata, target: &File, target_path: &Path) -> Result<()> {
    use std::os::unix::fs::PermissionsExt as _;

    let executable = source.permissions().mode() & 0o111 != 0;
    let mode = if executable { 0o755 } else { 0o644 };
    target
        .set_permissions(std::fs::Permissions::from_mode(mode))
        .map_err(|err| SyncError::io("set copy permissions", target_path, err))?;
    carry_mtime(source, target, target_path)
}

#[cfg(not(unix))]
fn carry_metadata(source: &std::fs::Metadata, target: &File, target_path: &Path) -> Result<()> {
    // No executable bit to carry: the platform derives it from the extension.
    carry_mtime(source, target, target_path)
}

fn carry_mtime(source: &std::fs::Metadata, target: &File, target_path: &Path) -> Result<()> {
    // A filesystem that cannot report an mtime is not a reason to fail a copy
    // whose bytes are correct; the destination simply keeps the time it was
    // written at.
    let Ok(modified) = source.modified() else {
        return Ok(());
    };
    target
        .set_modified(modified)
        .map_err(|err| SyncError::io("set copy mtime", target_path, err))
}

/// Remove a destination we published but could not prove.
///
/// Best effort by design: the entry is already being reported as failed, and a
/// second error about the cleanup would bury the one that matters.
fn discard(path: &Path) {
    if let Err(err) = std::fs::remove_file(path) {
        if err.kind() != std::io::ErrorKind::NotFound {
            tracing::warn!(path = %path.display(), %err, "could not remove unverified copy");
        }
    }
}

/// The one message AD-C2 exists to be able to produce.
fn mismatch(sent: (&str, u64), read_back: &(String, u64)) -> String {
    format!(
        "the bytes written did not read back as the bytes sent: sent sha256 {} over {} bytes, \
         read sha256 {} over {} bytes",
        sent.0, sent.1, read_back.0, read_back.1
    )
}

/// Everything the job will do, discovered before it does any of it.
struct Plan {
    /// The directory every `rel` resolves against on the source side. For a
    /// file source this is that file's parent, which makes `root.join(rel)` the
    /// file itself and lets one code path serve both shapes.
    root: PathBuf,
    items: Vec<PlanItem>,
    /// Entries the report will hold: files plus refusals. Directories are not
    /// entries — they are recreated, not copied.
    files_total: u64,
    bytes_total: u64,
}

enum PlanItem {
    /// A directory to recreate at the destination. Always precedes everything
    /// inside it, which is what lets the copy skip a `create_dir_all` per file.
    Dir {
        rel: PathBuf,
    },
    File {
        rel: PathBuf,
        bytes: u64,
    },
    /// Something this module refuses to copy, carried into the plan so it is
    /// counted in the totals and reported by name instead of vanishing.
    Refused {
        rel: PathBuf,
        reason: String,
    },
}

/// Walk the source and decide, up front, what the job consists of.
///
/// The pre-walk exists so `files_total` and `bytes_total` are facts rather than
/// guesses (AC 5): a surface must never claim a total it does not have. It is
/// also what makes the job's order deterministic — see [`children_of`].
fn plan_copy(source: &Path) -> Result<Plan> {
    let root_meta = std::fs::symlink_metadata(source)
        .map_err(|err| SyncError::io("stat copy source", source, err))?;

    if !root_meta.is_dir() {
        // A single-file (or refused) source: the plan's root is its parent, so
        // its one relative path is the file's own name and it lands as
        // `<destination>/<name>`.
        let Some(name) = source.file_name() else {
            return Err(SyncError::Config(format!(
                "copy source has no file name: {}",
                source.display()
            )));
        };
        let rel = PathBuf::from(name);
        let item = classify(source, rel)?;
        let bytes_total = match &item {
            PlanItem::File { bytes, .. } => *bytes,
            _ => 0,
        };
        return Ok(Plan {
            root: source
                .parent()
                .unwrap_or_else(|| Path::new(""))
                .to_path_buf(),
            items: vec![item],
            files_total: 1,
            bytes_total,
        });
    }

    let mut items = Vec::new();
    let mut stack = match children_of(source, Path::new("")) {
        Ok(children) => children,
        Err(err) => return Err(SyncError::io("read copy source directory", source, err)),
    };
    // Reversed so `pop` yields the sorted order, and children are pushed on top
    // of their siblings — a true pre-order walk, which is what guarantees a
    // directory's `Dir` item precedes every file inside it.
    stack.reverse();

    while let Some(rel) = stack.pop() {
        let absolute = source.join(&rel);
        let meta = match std::fs::symlink_metadata(&absolute) {
            Ok(meta) => meta,
            Err(err) => {
                items.push(PlanItem::Refused {
                    rel,
                    reason: format!("could not be inspected: {err}"),
                });
                continue;
            }
        };
        if meta.is_dir() && !meta.is_symlink() {
            match children_of(source, &rel) {
                Ok(mut children) => {
                    items.push(PlanItem::Dir { rel });
                    children.reverse();
                    stack.extend(children);
                }
                Err(err) => items.push(PlanItem::Refused {
                    rel,
                    reason: format!("directory could not be read: {err}"),
                }),
            }
            continue;
        }
        items.push(classify(&absolute, rel)?);
    }

    let files_total = items
        .iter()
        .filter(|item| !matches!(item, PlanItem::Dir { .. }))
        .count() as u64;
    let bytes_total = items
        .iter()
        .filter_map(|item| match item {
            PlanItem::File { bytes, .. } => Some(*bytes),
            _ => None,
        })
        .sum();

    Ok(Plan {
        root: source.to_path_buf(),
        items,
        files_total,
        bytes_total,
    })
}

/// Decide what one non-directory path is, and whether it can be copied.
fn classify(absolute: &Path, rel: PathBuf) -> Result<PlanItem> {
    let meta = std::fs::symlink_metadata(absolute)
        .map_err(|err| SyncError::io("stat copy source", absolute, err))?;

    if meta.is_symlink() {
        return Ok(PlanItem::Refused {
            rel,
            reason: "a symbolic link, which is skipped rather than followed: \
                     following it could copy files from outside the source tree, \
                     or loop forever"
                .into(),
        });
    }
    if !meta.is_file() {
        return Ok(PlanItem::Refused {
            rel,
            reason: format!("{}, which cannot be copied", describe_kind(&meta)),
        });
    }
    if is_dataless(absolute)? {
        return Ok(PlanItem::Refused {
            rel,
            reason: "a dataless iCloud placeholder; copying it would materialize \
                     the file from the network"
                .into(),
        });
    }
    Ok(PlanItem::File {
        rel,
        bytes: meta.len(),
    })
}

/// The sorted relative paths directly inside `root/rel`.
///
/// Sorted because `read_dir` order is arbitrary: a report a human reads, and a
/// job that can be cancelled halfway, both need the same tree to produce the
/// same sequence twice.
fn children_of(root: &Path, rel: &Path) -> std::io::Result<Vec<PathBuf>> {
    let mut children = Vec::new();
    for entry in std::fs::read_dir(root.join(rel))? {
        children.push(rel.join(entry?.file_name()));
    }
    children.sort();
    Ok(children)
}

/// Name what something is, for a refusal a user can act on.
fn describe_kind(meta: &std::fs::Metadata) -> &'static str {
    let kind = meta.file_type();
    if kind.is_symlink() {
        return "a symbolic link";
    }
    if kind.is_dir() {
        return "a directory";
    }
    if kind.is_file() {
        return "a regular file";
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::FileTypeExt as _;
        if kind.is_fifo() {
            // Not pedantry: opening a FIFO blocks until a writer appears, which
            // would hang the whole job forever.
            return "a named pipe";
        }
        if kind.is_socket() {
            return "a socket";
        }
        if kind.is_block_device() {
            return "a block device";
        }
        if kind.is_char_device() {
            return "a character device";
        }
    }
    "not a regular file"
}

fn display(rel: &Path) -> String {
    rel.to_string_lossy().into_owned()
}

/// Turns the job's own bookkeeping into [`CopyProgress`] events.
struct Reporter<'a> {
    sink: Option<&'a CopySink>,
    /// Set once a sink has answered `false`. The job carries on — it is user-
    /// requested work, not a UI subscription — but the sink is never called
    /// again, matching `lfs::basic::Reporter`.
    detached: bool,
    coalescer: ProgressCoalescer,
    files_total: u64,
    bytes_total: u64,
    files_done: u64,
    /// Bytes of every entry already decided.
    bytes_base: u64,
    /// High-water mark within the file in flight. A high-water mark rather than
    /// the latest figure for the same reason `progress::TransferTally` keeps
    /// one: a file whose content has to be compared before it can be replaced
    /// is streamed twice, and a bar that walks backwards reads as a bug.
    file_peak: u64,
    current: Option<String>,
}

impl<'a> Reporter<'a> {
    fn new(sink: Option<&'a CopySink>, plan: &Plan) -> Self {
        Self {
            sink,
            detached: false,
            coalescer: ProgressCoalescer::new(DEFAULT_PROGRESS_INTERVAL),
            files_total: plan.files_total,
            bytes_total: plan.bytes_total,
            files_done: 0,
            bytes_base: 0,
            file_peak: 0,
            current: None,
        }
    }

    fn begin(&mut self, rel: String) {
        self.current = Some(rel);
        self.file_peak = 0;
    }

    fn streamed(&mut self, bytes: u64) {
        self.file_peak = self.file_peak.max(bytes);
        self.emit(false);
    }

    fn finish(&mut self, planned: u64) {
        self.files_done += 1;
        self.bytes_base = self.bytes_base.saturating_add(planned);
        self.file_peak = 0;
        self.current = None;
    }

    fn emit(&mut self, force: bool) {
        let Some(sink) = self.sink else {
            return;
        };
        if self.detached {
            return;
        }
        if !force && !self.coalescer.should_emit(Instant::now()) {
            return;
        }
        // Clamped: a file that grew between the pre-walk and the read would
        // otherwise push the bar past its own total. That file is about to fail
        // the change check anyway.
        let bytes_done = self
            .bytes_base
            .saturating_add(self.file_peak)
            .min(self.bytes_total);
        let live = sink(CopyProgress {
            files_done: self.files_done,
            files_total: self.files_total,
            bytes_done,
            bytes_total: self.bytes_total,
            current: self.current.clone(),
        });
        if !live {
            self.detached = true;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Arc, Mutex};

    fn off() -> AtomicBool {
        AtomicBool::new(false)
    }

    fn write_file(path: &Path, bytes: &[u8]) {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).expect("create parent");
        }
        std::fs::write(path, bytes).expect("write file");
    }

    fn read_file(path: &Path) -> Vec<u8> {
        std::fs::read(path).expect("read file")
    }

    /// Bytes that span several [`HASH_CHUNK_BYTES`] reads, so the chunk loop —
    /// and cancellation inside it — is actually exercised.
    fn wide(seed: u8) -> Vec<u8> {
        (0..HASH_CHUNK_BYTES * 3 + 17)
            .map(|i| (i as u8).wrapping_mul(31).wrapping_add(seed))
            .collect()
    }

    /// A source tree with nesting, an empty directory, a large file and a
    /// small one.
    fn source_tree(root: &Path) {
        write_file(&root.join("alpha.txt"), b"alpha");
        write_file(&root.join("nested/beta.bin"), &wide(1));
        write_file(&root.join("nested/deeper/gamma.txt"), b"gamma");
        std::fs::create_dir_all(root.join("empty")).expect("create empty dir");
    }

    fn outcomes(report: &CopyReport) -> Vec<(&str, &CopyOutcome)> {
        report
            .entries
            .iter()
            .map(|entry| (entry.path.as_str(), &entry.outcome))
            .collect()
    }

    fn entry<'r>(report: &'r CopyReport, path: &str) -> &'r CopyEntry {
        report
            .entries
            .iter()
            .find(|entry| entry.path == path)
            .unwrap_or_else(|| panic!("no entry for {path}; got {:?}", outcomes(report)))
    }

    /// Every path under `root`, relative, so a test can assert on the whole
    /// shape of a destination rather than the files it thought to name.
    fn tree_paths(root: &Path) -> Vec<String> {
        let mut found = Vec::new();
        let mut stack = vec![root.to_path_buf()];
        while let Some(dir) = stack.pop() {
            for entry in std::fs::read_dir(&dir).expect("read dir") {
                let path = entry.expect("dir entry").path();
                if path.is_dir() {
                    stack.push(path.clone());
                }
                found.push(
                    path.strip_prefix(root)
                        .unwrap_or(&path)
                        .to_string_lossy()
                        .into_owned(),
                );
            }
        }
        found.sort();
        found
    }

    #[test]
    fn a_copied_tree_matches_byte_for_byte_and_every_entry_is_copied() {
        let dir = tempfile::tempdir().expect("tempdir");
        let source = dir.path().join("src");
        let destination = dir.path().join("dst");
        source_tree(&source);

        let report = copy_verified(&source, &destination, &CopyOptions::default(), None, &off())
            .expect("copy");

        assert_eq!(
            outcomes(&report),
            vec![
                ("alpha.txt", &CopyOutcome::Copied),
                ("nested/beta.bin", &CopyOutcome::Copied),
                ("nested/deeper/gamma.txt", &CopyOutcome::Copied),
            ]
        );
        for rel in ["alpha.txt", "nested/beta.bin", "nested/deeper/gamma.txt"] {
            assert_eq!(
                read_file(&destination.join(rel)),
                read_file(&source.join(rel)),
                "{rel} did not arrive byte-for-byte"
            );
        }
        // An empty directory is part of the tree's shape, so it is recreated
        // even though it produces no entry.
        assert!(
            destination.join("empty").is_dir(),
            "empty dir not recreated"
        );
        assert_eq!(tree_paths(&source), tree_paths(&destination));

        let expected: u64 = report.entries.iter().map(|entry| entry.bytes).sum();
        assert_eq!(report.bytes_copied, expected);
    }

    #[test]
    fn an_identical_destination_is_reported_and_never_rewritten() {
        let dir = tempfile::tempdir().expect("tempdir");
        let source = dir.path().join("src");
        let destination = dir.path().join("dst");
        source_tree(&source);

        copy_verified(&source, &destination, &CopyOptions::default(), None, &off()).expect("first");

        // The full identity tuple, not just the mtime: a rewrite that preserved
        // the mtime would still move the ctime and the inode, and this has to
        // catch that too.
        let before: Vec<_> = ["alpha.txt", "nested/beta.bin"]
            .iter()
            .map(|rel| {
                FileSample::of(&destination.join(rel))
                    .expect("stat")
                    .expect("present")
            })
            .collect();

        let report = copy_verified(&source, &destination, &CopyOptions::default(), None, &off())
            .expect("second");

        assert!(
            report
                .entries
                .iter()
                .all(|entry| entry.outcome == CopyOutcome::Identical),
            "expected every entry identical, got {:?}",
            outcomes(&report)
        );
        assert_eq!(report.bytes_copied, 0, "an identical run writes nothing");

        let after: Vec<_> = ["alpha.txt", "nested/beta.bin"]
            .iter()
            .map(|rel| {
                FileSample::of(&destination.join(rel))
                    .expect("stat")
                    .expect("present")
            })
            .collect();
        assert_eq!(before, after, "an identical destination was rewritten");
    }

    #[test]
    fn a_differing_destination_collides_and_its_bytes_survive() {
        let dir = tempfile::tempdir().expect("tempdir");
        let source = dir.path().join("src");
        let destination = dir.path().join("dst");
        source_tree(&source);

        // One collision proven by length, one that only a digest can see.
        write_file(
            &destination.join("alpha.txt"),
            b"a much longer thing entirely",
        );
        let same_length = b"GAMMA";
        write_file(&destination.join("nested/deeper/gamma.txt"), same_length);

        let report = copy_verified(&source, &destination, &CopyOptions::default(), None, &off())
            .expect("copy");

        assert_eq!(entry(&report, "alpha.txt").outcome, CopyOutcome::Collision);
        assert_eq!(
            entry(&report, "nested/deeper/gamma.txt").outcome,
            CopyOutcome::Collision
        );
        assert_eq!(
            read_file(&destination.join("alpha.txt")),
            b"a much longer thing entirely"
        );
        assert_eq!(
            read_file(&destination.join("nested/deeper/gamma.txt")),
            same_length
        );
        // The file that had no collision still copied: one refusal never stops
        // the job.
        assert_eq!(
            entry(&report, "nested/beta.bin").outcome,
            CopyOutcome::Copied
        );
        assert_eq!(report.bytes_copied, entry(&report, "nested/beta.bin").bytes);
        // A collision reports the size the user would be overwriting.
        assert_eq!(entry(&report, "alpha.txt").bytes, 5);
    }

    #[test]
    fn replace_existing_replaces_a_colliding_file() {
        let dir = tempfile::tempdir().expect("tempdir");
        let source = dir.path().join("src");
        let destination = dir.path().join("dst");
        source_tree(&source);
        write_file(&destination.join("alpha.txt"), b"stale");
        let stale = FileSample::of(&destination.join("alpha.txt"))
            .expect("stat")
            .expect("present");

        let options = CopyOptions {
            replace_existing: true,
        };
        let report = copy_verified(&source, &destination, &options, None, &off()).expect("copy");

        assert_eq!(entry(&report, "alpha.txt").outcome, CopyOutcome::Copied);
        assert_eq!(read_file(&destination.join("alpha.txt")), b"alpha");
        let fresh = FileSample::of(&destination.join("alpha.txt"))
            .expect("stat")
            .expect("present");
        assert_ne!(
            stale.inode, fresh.inode,
            "a replacement must arrive by rename, not by writing in place"
        );
    }

    #[test]
    fn replace_existing_keeps_the_old_file_when_the_new_bytes_are_not_provable() {
        let dir = tempfile::tempdir().expect("tempdir");
        let source = dir.path().join("src");
        let destination = dir.path().join("dst");
        write_file(&source.join("alpha.txt"), &wide(2));
        write_file(
            &destination.join("alpha.txt"),
            b"the file the user already had",
        );

        // The source is mutated after its first chunk, so the staged bytes are
        // never provable. With `replace_existing` on, the ordering of AD-C4 is
        // the only thing standing between the user and a destroyed file.
        let mut torn = |path: &Path| {
            let mut file = std::fs::OpenOptions::new()
                .append(true)
                .open(path)
                .expect("reopen source");
            file.write_all(b"appended mid-read").expect("append");
        };
        let options = CopyOptions {
            replace_existing: true,
        };
        let report = copy_verified_hooked(&source, &destination, &options, None, &off(), &mut torn)
            .expect("copy");

        let CopyOutcome::Failed { reason } = &entry(&report, "alpha.txt").outcome else {
            panic!("expected a failure, got {:?}", outcomes(&report));
        };
        assert!(
            reason.contains("changed while it was being read"),
            "unhelpful reason: {reason}"
        );
        assert_eq!(
            read_file(&destination.join("alpha.txt")),
            b"the file the user already had",
            "the old file was destroyed for bytes that were never proven"
        );
        assert_eq!(report.bytes_copied, 0);
        assert!(temp_files(&destination).is_empty());
    }

    #[test]
    fn a_source_that_changes_during_the_read_fails_that_entry() {
        let dir = tempfile::tempdir().expect("tempdir");
        let source = dir.path().join("src");
        let destination = dir.path().join("dst");
        source_tree(&source);

        let mut torn = |path: &Path| {
            if !path.ends_with("beta.bin") {
                return;
            }
            let mut file = std::fs::OpenOptions::new()
                .append(true)
                .open(path)
                .expect("reopen source");
            file.write_all(b"appended mid-read").expect("append");
        };
        let report = copy_verified_hooked(
            &source,
            &destination,
            &CopyOptions::default(),
            None,
            &off(),
            &mut torn,
        )
        .expect("copy");

        let CopyOutcome::Failed { reason } = &entry(&report, "nested/beta.bin").outcome else {
            panic!("expected a failure, got {:?}", outcomes(&report));
        };
        assert!(
            reason.contains("changed while it was being read"),
            "unhelpful reason: {reason}"
        );
        assert!(
            !destination.join("nested/beta.bin").exists(),
            "a torn read was published anyway"
        );
        assert!(temp_files(&destination).is_empty());
        // Its neighbours are unaffected.
        assert_eq!(entry(&report, "alpha.txt").outcome, CopyOutcome::Copied);
    }

    /// Anything under `root` that looks like staging scratch rather than a
    /// copied file.
    fn temp_files(root: &Path) -> Vec<String> {
        tree_paths(root)
            .into_iter()
            .filter(|path| {
                Path::new(path)
                    .file_name()
                    .map(|name| name.to_string_lossy().starts_with(".tmp"))
                    .unwrap_or(false)
            })
            .collect()
    }

    #[test]
    fn a_cancel_mid_tree_leaves_no_temp_and_no_partial_file() {
        let dir = tempfile::tempdir().expect("tempdir");
        let source = dir.path().join("src");
        let destination = dir.path().join("dst");
        source_tree(&source);

        let cancel = AtomicBool::new(false);
        // Cancel once the second file is genuinely mid-stream: `beta.bin` spans
        // several chunks, so this lands inside the chunk loop rather than
        // between two files.
        let mut mid_file = |path: &Path| {
            if path.ends_with("beta.bin") {
                cancel.store(true, Ordering::Relaxed);
            }
        };
        let report = copy_verified_hooked(
            &source,
            &destination,
            &CopyOptions::default(),
            None,
            &cancel,
            &mut mid_file,
        )
        .expect("copy");

        assert_eq!(
            outcomes(&report),
            vec![("alpha.txt", &CopyOutcome::Copied)],
            "the cancelled file must not be reported at all"
        );
        assert_eq!(
            temp_files(&destination),
            Vec::<String>::new(),
            "staging scratch survived a cancel"
        );
        assert!(
            !destination.join("nested/beta.bin").exists(),
            "a partially streamed file was left at the destination"
        );
        // What did finish is whole.
        assert_eq!(
            read_file(&destination.join("alpha.txt")),
            read_file(&source.join("alpha.txt"))
        );
    }

    #[test]
    fn a_cancel_before_the_first_file_copies_nothing() {
        let dir = tempfile::tempdir().expect("tempdir");
        let source = dir.path().join("src");
        let destination = dir.path().join("dst");
        source_tree(&source);

        let report = copy_verified(
            &source,
            &destination,
            &CopyOptions::default(),
            None,
            &AtomicBool::new(true),
        )
        .expect("copy");

        assert!(report.entries.is_empty());
        assert_eq!(report.bytes_copied, 0);
        assert_eq!(tree_paths(&destination), Vec::<String>::new());
    }

    #[test]
    fn the_pre_walk_totals_match_what_the_report_accounts_for() {
        let dir = tempfile::tempdir().expect("tempdir");
        let source = dir.path().join("src");
        let destination = dir.path().join("dst");
        source_tree(&source);
        #[cfg(unix)]
        std::os::unix::fs::symlink(source.join("alpha.txt"), source.join("link.txt"))
            .expect("symlink");

        // `CopySink` is a `'static` boxed closure, so an observing test shares
        // its buffer through an `Arc` rather than borrowing a local.
        let seen: Arc<Mutex<Vec<CopyProgress>>> = Arc::new(Mutex::new(Vec::new()));
        let report = {
            let recorded = Arc::clone(&seen);
            let sink: CopySink = Box::new(move |event| {
                recorded.lock().expect("sink lock").push(event);
                true
            });
            copy_verified(
                &source,
                &destination,
                &CopyOptions::default(),
                Some(&sink),
                &off(),
            )
            .expect("copy")
        };

        let events = seen.lock().expect("sink lock").clone();
        let first = events.first().expect("an opening event with the totals");
        let last = events.last().expect("a closing event");

        // Totals are known before any byte moves and never move afterwards.
        assert_eq!(first.files_done, 0);
        assert_eq!(first.bytes_done, 0);
        assert_eq!(first.files_total, last.files_total);
        assert_eq!(first.bytes_total, last.bytes_total);

        assert_eq!(
            last.files_total as usize,
            report.entries.len(),
            "the pre-walk promised a different number of entries than the report holds"
        );
        assert_eq!(last.files_done, last.files_total);
        let accounted: u64 = report.entries.iter().map(|entry| entry.bytes).sum();
        assert_eq!(last.bytes_total, accounted);
        assert_eq!(last.bytes_done, last.bytes_total);
        assert_eq!(last.current, None, "the closing event names no file");
    }

    #[cfg(unix)]
    #[test]
    fn a_symlink_is_refused_by_name_rather_than_followed() {
        let dir = tempfile::tempdir().expect("tempdir");
        let source = dir.path().join("src");
        let destination = dir.path().join("dst");
        write_file(&source.join("alpha.txt"), b"alpha");
        // Points outside the source tree: following it would copy a file the
        // user never selected.
        write_file(&dir.path().join("outside.txt"), b"not part of the job");
        std::os::unix::fs::symlink(dir.path().join("outside.txt"), source.join("escape.txt"))
            .expect("symlink");

        let report = copy_verified(&source, &destination, &CopyOptions::default(), None, &off())
            .expect("copy");

        let CopyOutcome::Failed { reason } = &entry(&report, "escape.txt").outcome else {
            panic!("expected a refusal, got {:?}", outcomes(&report));
        };
        assert!(
            reason.contains("symbolic link"),
            "a refusal must name what it refused: {reason}"
        );
        assert_eq!(entry(&report, "escape.txt").bytes, 0);
        assert!(
            !destination.join("escape.txt").exists(),
            "the symlink target was copied through"
        );
    }

    #[cfg(unix)]
    #[test]
    fn the_executable_bit_and_the_mtime_are_carried() {
        use std::os::unix::fs::PermissionsExt as _;

        let dir = tempfile::tempdir().expect("tempdir");
        let source = dir.path().join("src");
        let destination = dir.path().join("dst");
        write_file(&source.join("run.sh"), b"#!/bin/sh\necho hi\n");
        write_file(&source.join("notes.txt"), b"plain");
        std::fs::set_permissions(
            source.join("run.sh"),
            std::fs::Permissions::from_mode(0o755),
        )
        .expect("chmod");

        copy_verified(&source, &destination, &CopyOptions::default(), None, &off()).expect("copy");

        let script = std::fs::metadata(destination.join("run.sh")).expect("stat script");
        let notes = std::fs::metadata(destination.join("notes.txt")).expect("stat notes");
        assert_eq!(script.permissions().mode() & 0o777, 0o755);
        assert_eq!(
            notes.permissions().mode() & 0o777,
            0o644,
            "a copy must not inherit the temp file's owner-only mode"
        );
        assert_eq!(
            script.modified().expect("dst mtime"),
            std::fs::metadata(source.join("run.sh"))
                .expect("stat source")
                .modified()
                .expect("src mtime")
        );
    }

    #[test]
    fn a_file_source_lands_under_the_destination_directory() {
        let dir = tempfile::tempdir().expect("tempdir");
        let source = dir.path().join("one.txt");
        let destination = dir.path().join("dst");
        write_file(&source, b"just the one");

        let report = copy_verified(&source, &destination, &CopyOptions::default(), None, &off())
            .expect("copy");

        assert_eq!(outcomes(&report), vec![("one.txt", &CopyOutcome::Copied)]);
        assert_eq!(read_file(&destination.join("one.txt")), b"just the one");
    }

    #[test]
    fn a_missing_source_fails_the_job_rather_than_reporting_an_entry() {
        let dir = tempfile::tempdir().expect("tempdir");
        let err = copy_verified(
            &dir.path().join("nope"),
            &dir.path().join("dst"),
            &CopyOptions::default(),
            None,
            &off(),
        )
        .expect_err("a source that is not there is not a per-file problem");
        assert_eq!(err.code(), "io");
    }

    #[test]
    fn a_destination_occupied_by_a_directory_fails_that_entry_only() {
        let dir = tempfile::tempdir().expect("tempdir");
        let source = dir.path().join("src");
        let destination = dir.path().join("dst");
        source_tree(&source);
        std::fs::create_dir_all(destination.join("alpha.txt")).expect("occupy with a directory");

        let report = copy_verified(&source, &destination, &CopyOptions::default(), None, &off())
            .expect("copy");

        let CopyOutcome::Failed { reason } = &entry(&report, "alpha.txt").outcome else {
            panic!("expected a failure, got {:?}", outcomes(&report));
        };
        assert!(reason.contains("a directory"), "unhelpful reason: {reason}");
        assert_eq!(
            entry(&report, "nested/beta.bin").outcome,
            CopyOutcome::Copied
        );
    }

    #[test]
    fn a_sink_that_declines_is_never_called_again() {
        let dir = tempfile::tempdir().expect("tempdir");
        let source = dir.path().join("src");
        let destination = dir.path().join("dst");
        source_tree(&source);

        let calls = Arc::new(std::sync::atomic::AtomicU64::new(0));
        let report = {
            let counted = Arc::clone(&calls);
            let sink: CopySink = Box::new(move |_| {
                counted.fetch_add(1, Ordering::Relaxed);
                false
            });
            copy_verified(
                &source,
                &destination,
                &CopyOptions::default(),
                Some(&sink),
                &off(),
            )
            .expect("copy")
        };

        assert_eq!(calls.load(Ordering::Relaxed), 1, "the receiver is gone");
        // A detached receiver never stops the work: it is what the user asked
        // for, not a UI subscription.
        assert!(report
            .entries
            .iter()
            .all(|entry| entry.outcome == CopyOutcome::Copied));
    }

    #[test]
    fn the_outcome_crosses_the_boundary_as_a_tagged_union() {
        let json = serde_json::to_string(&CopyEntry {
            path: "nested/beta.bin".into(),
            bytes: 12,
            outcome: CopyOutcome::Failed {
                reason: "nope".into(),
            },
        })
        .expect("serialize");
        assert_eq!(
            json,
            r#"{"path":"nested/beta.bin","bytes":12,"outcome":{"kind":"failed","reason":"nope"}}"#
        );
        assert_eq!(
            serde_json::to_string(&CopyOutcome::Identical).expect("serialize"),
            r#"{"kind":"identical"}"#
        );
    }
}
