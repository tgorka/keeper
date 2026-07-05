//! Archive export orchestrator (Story 5.5, FR-35, AD-11) — tauri-free.
//!
//! Reads `archive.db` **only** (via the caller's read-only [`Connection`]) and
//! writes, for a chosen scope, a **lossless JSON** array (every archived row in
//! scope — the provability artifact) plus a **chronological Markdown transcript**
//! (one entry per logical message: sender, timestamp, final edited text, redaction
//! stub, media as relative links). It runs synchronously (rusqlite is sync); the
//! keeper layer drives it on a blocking-safe task and checks the shared cancel
//! flag. Media bytes are copied best-effort via an **injected** resolver so this
//! module never links a `Client`/session — a `None` resolver makes every media
//! item count as skipped. All output lands under one scope subfolder so cancel /
//! failure cleanup is a single `remove_dir_all`.

pub mod json;
pub mod md;

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};

use crate::archive::db::{
    edit_chain, scoped_event_count, scoped_events_chronological, ExportScope, StoredEvent,
};
use crate::archive::{display_body_from_content, ArchiveMedia};
use crate::error::ArchiveError;
use crate::vm::{ExportPhase, ExportProgressVm, ExportRequestVm, ExportScopeKind};

use self::md::TranscriptEntry;

/// The resolver the keeper layer injects to fetch media bytes for a scoped media
/// item (Story 5.5). Given the item's [`ArchiveMedia`] metadata and its owning
/// `event_id`, it returns the bytes when locally resolvable, else `None` (uncached
/// / signed-out) — never an error, so a miss is a skip, never a failure. Kept as a
/// callback so `keeper-core::archive::export` stays tauri-/session-free.
pub type MediaResolver<'a> = dyn Fn(&ArchiveMedia, &str) -> Option<Vec<u8>> + 'a;

/// The result of a completed export (Story 5.5). Returned from [`run_export`] on
/// success; the caller wraps it into the terminal `Completed` [`ExportProgressVm`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExportOutcome {
    /// The written output file paths (JSON and/or Markdown under the scope folder).
    pub output_paths: Vec<String>,
    /// Logical messages (transcript entries) written.
    pub messages_written: u64,
    /// Media items whose bytes were copied into `media/`.
    pub media_copied: u64,
    /// Media items skipped (unresolvable / uncached / no resolver).
    pub media_skipped: u64,
}

/// Cancellation sentinel returned when the export loop observes the cancel flag.
/// The caller distinguishes this from a real error to emit `Cancelled` (not
/// `Failed`) after cleanup.
#[derive(Debug)]
pub enum ExportError {
    /// The job was cancelled (the cancel flag was set); partial output is cleaned.
    Cancelled,
    /// A genuine failure (IO / serialization); partial output is cleaned.
    Failed(ArchiveError),
}

impl From<ArchiveError> for ExportError {
    fn from(e: ArchiveError) -> Self {
        ExportError::Failed(e)
    }
}

/// Map the IPC scope kind + optional ids into the tauri-free [`ExportScope`].
/// Returns an [`ArchiveError::ExportIo`] describing a missing required id (a
/// malformed request) rather than silently widening the scope.
fn scope_from_request(req: &ExportRequestVm) -> Result<ExportScope, ArchiveError> {
    match req.scope {
        ExportScopeKind::Chat => {
            let account_id = req.account_id.clone().ok_or_else(|| {
                ArchiveError::ExportIo("chat export requires an account id".to_owned())
            })?;
            let room_id = req.room_id.clone().ok_or_else(|| {
                ArchiveError::ExportIo("chat export requires a room id".to_owned())
            })?;
            Ok(ExportScope::Chat {
                account_id,
                room_id,
            })
        }
        ExportScopeKind::Account => {
            let account_id = req.account_id.clone().ok_or_else(|| {
                ArchiveError::ExportIo("account export requires an account id".to_owned())
            })?;
            Ok(ExportScope::Account { account_id })
        }
        ExportScopeKind::Everything => Ok(ExportScope::Everything),
    }
}

/// A short, filesystem-safe slug for the scope subfolder name.
fn scope_slug(scope: &ExportScope) -> String {
    match scope {
        // Key on account_id too (like the Account scope): the same room synced under
        // two of your accounts is stored as separate rows, so a chat slug that ignored
        // the account would collide and silently overwrite one export with the other.
        ExportScope::Chat {
            account_id,
            room_id,
        } => format!("chat-{}-{}", sanitize(account_id), sanitize(room_id)),
        ExportScope::Account { account_id } => format!("account-{}", sanitize(account_id)),
        ExportScope::Everything => "everything".to_owned(),
    }
}

/// Sanitize an identifier or filename into a filesystem-safe token: keep
/// alphanumerics, `.`, `-`, `_`; replace every other byte (`:`/`/`/`!`/`$`/space,
/// path separators, control chars) with `_`. Bounded and never empty (a fully
/// stripped input becomes `"file"`).
fn sanitize(s: &str) -> String {
    let cleaned: String = s
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '_') {
                c
            } else {
                '_'
            }
        })
        .collect();
    let trimmed = cleaned.trim_matches('_');
    if trimmed.is_empty() {
        "file".to_owned()
    } else {
        // Cap the length so a pathological id can't blow the filename limit.
        trimmed.chars().take(80).collect()
    }
}

/// Run the export synchronously (Story 5.5). See the module docs for the contract.
///
/// Creates a scope subfolder under `dest_root`, streams the scoped roots
/// chronologically, writes the requested JSON / Markdown, best-effort-copies media
/// via `media`, checks `cancel` between events, and emits `progress` batches. On
/// cancel **or** any error the whole scope subfolder is `remove_dir_all`-ed before
/// returning, so no partial output survives. Returns the [`ExportOutcome`] on
/// success (the caller emits the terminal `Completed` batch).
#[allow(clippy::too_many_arguments)]
pub fn run_export(
    reader: &rusqlite::Connection,
    req: &ExportRequestVm,
    dest_root: &Path,
    honor_deletions: bool,
    progress: &dyn Fn(ExportProgressVm) -> bool,
    cancel: &AtomicBool,
    media: Option<&MediaResolver<'_>>,
    export_id: u64,
) -> Result<ExportOutcome, ExportError> {
    let scope = scope_from_request(req)?;
    let scope_dir = dest_root.join(scope_slug(&scope));

    // Whether the scope folder already existed before we touched it. Cleanup only
    // deletes a folder *we* created — never a pre-existing user folder that happens
    // to collide with the scope slug (e.g. a previous export's output, or an
    // unrelated directory of the same name), so a cancel/failure can't wipe it.
    let pre_existed = scope_dir.exists();

    let result = run_inner(
        reader,
        req,
        &scope,
        &scope_dir,
        honor_deletions,
        progress,
        cancel,
        media,
        export_id,
    );

    if result.is_err() {
        // Best-effort cleanup of partial output before the caller emits the terminal
        // batch (the contract: on cancel/failure partial files are deleted, honestly).
        // A cleanup failure is logged (ids only) but never masks the original error.
        if pre_existed {
            // The folder pre-existed (a previous export's output, or an unrelated
            // user directory of the same name), so we must not wipe it wholesale.
            // Remove only the artifacts *this* export writes into it — never the
            // folder or unrelated sibling files — so partial output can't linger.
            remove_export_artifact(&scope_dir.join("export.json"), export_id);
            remove_export_artifact(&scope_dir.join("transcript.md"), export_id);
            remove_export_artifact(&scope_dir.join("media"), export_id);
        } else if let Err(e) = std::fs::remove_dir_all(&scope_dir) {
            // We created the whole folder, so removing it wholesale is safe.
            if e.kind() != std::io::ErrorKind::NotFound {
                tracing::warn!(export_id, error = %e, "export: partial-output cleanup failed");
            }
        }
    }
    result
}

/// Best-effort removal of a single export artifact (file or directory) during
/// cleanup. A missing path is fine (it was never written); any other error is
/// logged (ids only) but never surfaced — cleanup must not mask the export result.
fn remove_export_artifact(path: &Path, export_id: u64) {
    let result = if path.is_dir() {
        std::fs::remove_dir_all(path)
    } else {
        std::fs::remove_file(path)
    };
    if let Err(e) = result {
        if e.kind() != std::io::ErrorKind::NotFound {
            tracing::warn!(export_id, error = %e, "export: partial-output cleanup failed");
        }
    }
}

/// The inner body (see [`run_export`]); split out so the caller's cleanup wraps
/// every early return uniformly.
#[allow(clippy::too_many_arguments)]
fn run_inner(
    reader: &rusqlite::Connection,
    req: &ExportRequestVm,
    scope: &ExportScope,
    scope_dir: &Path,
    honor_deletions: bool,
    progress: &dyn Fn(ExportProgressVm) -> bool,
    cancel: &AtomicBool,
    media: Option<&MediaResolver<'_>>,
    export_id: u64,
) -> Result<ExportOutcome, ExportError> {
    check_cancel(cancel)?;

    std::fs::create_dir_all(scope_dir).map_err(|e| {
        ExportError::Failed(ArchiveError::ExportIo(format!(
            "could not create export folder: {e}"
        )))
    })?;

    // Read the row set and the provability denominator from ONE consistent snapshot
    // (a read transaction), so the "emitted JSON count == scoped archive count"
    // guarantee holds even if the single writer ingests concurrently between the two
    // queries on this read-only connection (WAL gives the transaction a stable view).
    let (all_rows, total_rows) = {
        let tx = reader.unchecked_transaction().map_err(|e| {
            ExportError::Failed(ArchiveError::Sqlite(format!(
                "could not begin export read transaction: {e}"
            )))
        })?;
        let rows = scoped_events_chronological(&tx, scope)?;
        let count = scoped_event_count(&tx, scope)?;
        (rows, count)
    };
    // A real runtime guard (not a debug-only assert) on the provability invariant:
    // the snapshot makes this always hold, but a divergence must fail the export
    // rather than silently ship an "incomplete lossless" artifact in a release build.
    if all_rows.len() as i64 != total_rows {
        return Err(ExportError::Failed(ArchiveError::ExportIo(format!(
            "export row/count mismatch: {} rows vs {} counted",
            all_rows.len(),
            total_rows
        ))));
    }

    // The transcript is keyed on logical messages: the roots are the non-edit rows
    // (a row that is not an `m.replace`). `total_messages` counts them.
    let roots: Vec<&StoredEvent> = all_rows
        .iter()
        .filter(|r| r.rel_type.as_deref() != Some("m.replace"))
        .collect();
    let total_messages = roots.len() as u64;

    let mut output_paths: Vec<String> = Vec::new();

    // --- JSON: emit every scoped row, losslessly. ---
    if req.json {
        check_cancel(cancel)?;
        let json = json::render_json(&all_rows)?;
        let path = scope_dir.join("export.json");
        std::fs::write(&path, json).map_err(|e| {
            ExportError::Failed(ArchiveError::ExportIo(format!(
                "could not write export.json: {e}"
            )))
        })?;
        output_paths.push(path.to_string_lossy().into_owned());
    }

    // --- Markdown transcript + best-effort media copy. ---
    let mut messages_written: u64 = 0;
    let mut media_copied: u64 = 0;
    let mut media_skipped: u64 = 0;
    let mut entries: Vec<TranscriptEntry> = Vec::with_capacity(roots.len());
    let media_dir = scope_dir.join("media");

    for root in &roots {
        check_cancel(cancel)?;

        // Resolve the final edited text through the version chain, gated by the
        // honor-deletions policy so a redacted root renders a stub, never content.
        let (body, redacted) = resolve_final_body(reader, root, honor_deletions)?;

        // Media: derive a relative link from the row's metadata (always emitted),
        // and best-effort-copy the bytes when a resolver returns them.
        let media_link = match parse_media(root) {
            Some(meta) => {
                let link = media_relative_link(&root.event_id, &meta);
                copy_media_bytes(
                    media,
                    &meta,
                    &root.event_id,
                    &media_dir,
                    &link,
                    &mut media_copied,
                    &mut media_skipped,
                )?;
                Some(link)
            }
            None => None,
        };

        entries.push(TranscriptEntry {
            sender: root.sender.clone(),
            timestamp: root.origin_ts,
            body: if redacted {
                "_[message deleted]_".to_owned()
            } else {
                body
            },
            media_link,
        });
        messages_written += 1;

        // A `Running` progress heartbeat per message; a closed channel (the caller
        // unsubscribed) simply drops the batch.
        progress(ExportProgressVm {
            export_id,
            phase: ExportPhase::Running,
            messages_written,
            total_messages: Some(total_messages),
            media_copied,
            media_skipped,
            output_paths: Vec::new(),
            error: None,
        });
    }

    if req.markdown {
        check_cancel(cancel)?;
        let title = markdown_title(scope);
        // Honest disclosure in the artifact itself: when media is referenced but no
        // bytes were included (no resolver / uncached / signed-out), say so — a
        // reader of the transcript should know the `media/…` links are references,
        // not files that travelled with the export.
        let media_note = if media_copied == 0 && media_skipped > 0 {
            Some("Media files are referenced by link but were not included in this export.")
        } else {
            None
        };
        let doc = md::render_markdown(&title, &entries, media_note);
        let path = scope_dir.join("transcript.md");
        std::fs::write(&path, doc).map_err(|e| {
            ExportError::Failed(ArchiveError::ExportIo(format!(
                "could not write transcript.md: {e}"
            )))
        })?;
        output_paths.push(path.to_string_lossy().into_owned());
    }

    check_cancel(cancel)?;

    Ok(ExportOutcome {
        output_paths,
        messages_written,
        media_copied,
        media_skipped,
    })
}

/// Check the shared cancel flag; return [`ExportError::Cancelled`] when set so the
/// caller cleans up and emits the `Cancelled` terminal batch.
fn check_cancel(cancel: &AtomicBool) -> Result<(), ExportError> {
    if cancel.load(Ordering::Relaxed) {
        Err(ExportError::Cancelled)
    } else {
        Ok(())
    }
}

/// Resolve a root's final display body through its edit chain, gated by the
/// honor-deletions policy. Returns `(body, redacted)`: when the current version is
/// redacted and deletions are honored, `redacted` is `true` and the body is empty
/// (the caller renders a stub). Otherwise the latest version's display text.
fn resolve_final_body(
    conn: &rusqlite::Connection,
    root: &StoredEvent,
    honor_deletions: bool,
) -> Result<(String, bool), ArchiveError> {
    // The chain is original + edits ordered oldest→newest; the last is current.
    let chain = edit_chain(conn, &root.account_id, &root.event_id)?;
    // Honor-deletions gating mirrors the FTS engine (Story 5.3, `fts.rs`) exactly so
    // the transcript and search never disagree on what is withheld:
    //   1. A remote redaction marks the *root* row → the whole logical message is
    //      withheld (a stub), never any version's content.
    //   2. A redacted *version* is never shown as the representative → the current
    //      text is the latest **non-redacted** version. Without this, redacting only
    //      the latest edit (whose row carries `redacted_ts`, while the root's is NULL)
    //      would leak that edited-away content into a shareable transcript.
    // With honoring off, retention wins: the latest version (even if redacted) shows.
    if honor_deletions && root.redacted_ts.is_some() {
        return Ok((String::new(), true));
    }
    let current = if honor_deletions {
        chain
            .iter()
            .rev()
            .find(|e| e.redacted_ts.is_none())
            .unwrap_or(root)
    } else {
        chain.last().unwrap_or(root)
    };
    let body = display_body_from_content(&current.content_json);
    Ok((body, false))
}

/// Parse a row's `media_json` into [`ArchiveMedia`], or `None` for a non-media /
/// unparseable row (a malformed blob is treated as no media — never a failure).
fn parse_media(row: &StoredEvent) -> Option<ArchiveMedia> {
    let raw = row.media_json.as_deref()?;
    serde_json::from_str::<ArchiveMedia>(raw).ok()
}

/// The relative `media/<event_id>-<sanitized_filename>` link for a media item.
/// Derived purely from metadata (no bytes), so it is emitted whether or not the
/// bytes are copied.
fn media_relative_link(event_id: &str, meta: &ArchiveMedia) -> String {
    let name = meta
        .filename
        .as_deref()
        .filter(|f| !f.trim().is_empty())
        .unwrap_or("media");
    format!("media/{}-{}", sanitize(event_id), sanitize(name))
}

/// Best-effort copy of a media item's bytes into `<scope>/media/…` (Story 5.5).
///
/// With no resolver, or when the resolver returns `None`, the item is **skipped**
/// and counted (`media_skipped`) — never fatal; the Markdown link + JSON metadata
/// are emitted regardless. On a real write failure the whole export fails (mapped
/// through [`ArchiveError::ExportIo`]) so the caller cleans up.
#[allow(clippy::too_many_arguments)]
fn copy_media_bytes(
    media: Option<&MediaResolver<'_>>,
    meta: &ArchiveMedia,
    event_id: &str,
    media_dir: &Path,
    relative_link: &str,
    media_copied: &mut u64,
    media_skipped: &mut u64,
) -> Result<(), ExportError> {
    // `None` resolver ⇒ every media item is skipped (AD-11: no session).
    let Some(resolve) = media else {
        *media_skipped += 1;
        return Ok(());
    };
    let Some(bytes) = resolve(meta, event_id) else {
        // Uncached / signed-out / resolver error ⇒ skip and count.
        *media_skipped += 1;
        return Ok(());
    };
    std::fs::create_dir_all(media_dir).map_err(|e| {
        ExportError::Failed(ArchiveError::ExportIo(format!(
            "could not create media folder: {e}"
        )))
    })?;
    // `relative_link` is `media/<file>`; strip the folder prefix for the on-disk
    // name under `media_dir`.
    let file_name = relative_link
        .strip_prefix("media/")
        .unwrap_or(relative_link);
    let path = media_dir.join(file_name);
    std::fs::write(&path, bytes).map_err(|e| {
        ExportError::Failed(ArchiveError::ExportIo(format!(
            "could not write media file: {e}"
        )))
    })?;
    *media_copied += 1;
    Ok(())
}

/// A human title for the Markdown transcript, derived from the scope.
fn markdown_title(scope: &ExportScope) -> String {
    match scope {
        ExportScope::Chat { room_id, .. } => format!("Transcript — {room_id}"),
        ExportScope::Account { account_id } => format!("Transcript — account {account_id}"),
        ExportScope::Everything => "Transcript — all conversations".to_owned(),
    }
}

/// Resolve the scope subfolder path under a destination root (for the caller / a
/// test to locate the output without re-deriving the slug).
pub fn scope_output_dir(req: &ExportRequestVm, dest_root: &Path) -> Result<PathBuf, ArchiveError> {
    let scope = scope_from_request(req)?;
    Ok(dest_root.join(scope_slug(&scope)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::archive::db::open_archive_db;

    fn temp_dir() -> PathBuf {
        let mut dir = std::env::temp_dir();
        dir.push(format!(
            "keeper-export-test-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(0)
        ));
        dir
    }

    #[allow(clippy::too_many_arguments)]
    fn insert(
        conn: &rusqlite::Connection,
        account_id: &str,
        event_id: &str,
        room_id: &str,
        origin_ts: i64,
        content_json: &str,
        media_json: Option<&str>,
        relates_to: Option<&str>,
        rel_type: Option<&str>,
        redacted_ts: Option<i64>,
    ) {
        conn.execute(
            "INSERT INTO events(account_id, event_id, room_id, sender, origin_ts, event_type, \
             content_json, media_json, inserted_ts, relates_to_event_id, rel_type, redacted_ts) \
             VALUES (?1, ?2, ?3, '@u:e.org', ?4, 'm.room.message', ?5, ?6, ?4, ?7, ?8, ?9)",
            rusqlite::params![
                account_id,
                event_id,
                room_id,
                origin_ts,
                content_json,
                media_json,
                relates_to,
                rel_type,
                redacted_ts
            ],
        )
        .expect("insert");
    }

    fn chat_req(account_id: &str, room_id: &str, dest: &str) -> ExportRequestVm {
        ExportRequestVm {
            scope: ExportScopeKind::Chat,
            account_id: Some(account_id.to_owned()),
            room_id: Some(room_id.to_owned()),
            json: true,
            markdown: true,
            include_media: false,
            destination_dir: dest.to_owned(),
        }
    }

    fn no_progress() -> impl Fn(ExportProgressVm) -> bool {
        |_| true
    }

    #[test]
    fn lossless_json_count_equals_scoped_rows_and_md_has_final_text() {
        let dir = temp_dir();
        let conn = open_archive_db(&dir).expect("open");
        // Root + two edits (edit chain) + a message in a different room.
        insert(
            &conn,
            "acctA",
            "$orig",
            "!r1",
            100,
            r#"{"body":"v1"}"#,
            None,
            None,
            None,
            None,
        );
        insert(
            &conn,
            "acctA",
            "$edit1",
            "!r1",
            200,
            r#"{"m.new_content":{"body":"v2"}}"#,
            None,
            Some("$orig"),
            Some("m.replace"),
            None,
        );
        insert(
            &conn,
            "acctA",
            "$other",
            "!r2",
            150,
            r#"{"body":"elsewhere"}"#,
            None,
            None,
            None,
            None,
        );
        let out_root = dir.join("out");
        let req = chat_req("acctA", "!r1", out_root.to_str().expect("utf8 path"));
        let cancel = AtomicBool::new(false);
        let outcome = run_export(
            &conn,
            &req,
            &out_root,
            false,
            &no_progress(),
            &cancel,
            None,
            1,
        )
        .expect("export ok");
        // Scoped rows for !r1 = 2 (orig + edit); JSON is lossless over both.
        let json_path = out_root.join("chat-acctA-r1").join("export.json");
        let json = std::fs::read_to_string(&json_path).expect("json written");
        let parsed: serde_json::Value = serde_json::from_str(&json).expect("parse");
        assert_eq!(
            parsed.as_array().expect("array").len(),
            2,
            "lossless: 2 rows"
        );
        // One transcript entry (the root), showing the latest edited text v2.
        assert_eq!(outcome.messages_written, 1);
        let md_path = out_root.join("chat-acctA-r1").join("transcript.md");
        let md = std::fs::read_to_string(&md_path).expect("md written");
        assert!(md.contains("v2"), "final edited text shown");
        assert!(!md.contains("elsewhere"), "other room excluded");
        drop(conn);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn redacted_root_renders_stub_when_honoring_but_json_retains_row() {
        let dir = temp_dir();
        let conn = open_archive_db(&dir).expect("open");
        insert(
            &conn,
            "acctA",
            "$e1",
            "!r1",
            100,
            r#"{"body":"secret"}"#,
            None,
            None,
            None,
            Some(999),
        );
        let out_root = dir.join("out");
        let req = chat_req("acctA", "!r1", out_root.to_str().expect("utf8 path"));
        let cancel = AtomicBool::new(false);
        run_export(
            &conn,
            &req,
            &out_root,
            true,
            &no_progress(),
            &cancel,
            None,
            1,
        )
        .expect("ok");
        let scope_dir = out_root.join("chat-acctA-r1");
        let md = std::fs::read_to_string(scope_dir.join("transcript.md")).expect("md");
        assert!(md.contains("message deleted"), "stub shown");
        assert!(!md.contains("secret"), "withheld content not in transcript");
        let json = std::fs::read_to_string(scope_dir.join("export.json")).expect("json");
        assert!(
            json.contains("secret"),
            "JSON still retains the row (lossless)"
        );
        drop(conn);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn honoring_shows_prior_version_not_a_redacted_latest_edit() {
        // A message edited to "v2", then the *edit* (not the root) is remotely
        // redacted. With honoring on, the transcript must show the surviving prior
        // version "v1" — never the redacted-away "v2".
        let dir = temp_dir();
        let conn = open_archive_db(&dir).expect("open");
        insert(
            &conn,
            "acctA",
            "$orig",
            "!r1",
            100,
            r#"{"body":"v1"}"#,
            None,
            None,
            None,
            None,
        );
        insert(
            &conn,
            "acctA",
            "$edit1",
            "!r1",
            200,
            r#"{"m.new_content":{"body":"v2"}}"#,
            None,
            Some("$orig"),
            Some("m.replace"),
            Some(999), // the edit row itself is redacted
        );
        let out_root = dir.join("out");
        let req = chat_req("acctA", "!r1", out_root.to_str().expect("utf8 path"));
        let cancel = AtomicBool::new(false);
        run_export(
            &conn,
            &req,
            &out_root,
            true, // honor deletions
            &no_progress(),
            &cancel,
            None,
            1,
        )
        .expect("ok");
        let md = std::fs::read_to_string(out_root.join("chat-acctA-r1").join("transcript.md"))
            .expect("md");
        assert!(md.contains("v1"), "surviving prior version shown");
        assert!(
            !md.contains("v2"),
            "redacted-away latest edit must not leak into transcript"
        );
        drop(conn);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn cancel_preserves_a_preexisting_scope_folder() {
        // A cancelled/failed export must clean up only output it created — never a
        // pre-existing user folder that collides with the scope slug.
        let dir = temp_dir();
        let conn = open_archive_db(&dir).expect("open");
        insert(
            &conn,
            "acctA",
            "$e1",
            "!r1",
            100,
            r#"{"body":"hi"}"#,
            None,
            None,
            None,
            None,
        );
        let out_root = dir.join("out");
        let scope_dir = out_root.join("chat-acctA-r1");
        std::fs::create_dir_all(&scope_dir).expect("pre-create scope dir");
        let sentinel = scope_dir.join("keep-me.txt");
        std::fs::write(&sentinel, b"user data").expect("sentinel");
        let req = chat_req("acctA", "!r1", out_root.to_str().expect("utf8 path"));
        let cancel = AtomicBool::new(true); // trips on the first check
        let err = run_export(
            &conn,
            &req,
            &out_root,
            false,
            &no_progress(),
            &cancel,
            None,
            1,
        )
        .expect_err("cancelled");
        assert!(matches!(err, ExportError::Cancelled));
        assert!(
            sentinel.exists(),
            "pre-existing folder + its contents must survive cleanup"
        );
        drop(conn);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn cancel_midrun_removes_partial_output_but_spares_preexisting_files() {
        // The scope folder pre-exists (with an unrelated user file) AND the export
        // writes export.json before a mid-run cancel. Cleanup must delete the partial
        // output *this* export created while leaving the pre-existing folder and its
        // files intact — a pre-existing folder must not cause cleanup to be skipped
        // wholesale (the contract: on cancel/failure partial files are deleted).
        let dir = temp_dir();
        let conn = open_archive_db(&dir).expect("open");
        insert(
            &conn,
            "acctA",
            "$e1",
            "!r1",
            100,
            r#"{"body":"hi"}"#,
            None,
            None,
            None,
            None,
        );
        let out_root = dir.join("out");
        let scope_dir = out_root.join("chat-acctA-r1");
        std::fs::create_dir_all(&scope_dir).expect("pre-create scope dir");
        let sentinel = scope_dir.join("keep-me.txt");
        std::fs::write(&sentinel, b"user data").expect("sentinel");
        let req = chat_req("acctA", "!r1", out_root.to_str().expect("utf8 path"));
        // Cancel starts unset; the first Running heartbeat fires after export.json is
        // written, and the closure sets the flag so the export trips before writing
        // transcript.md — leaving a partial export.json on disk to be cleaned up.
        let cancel = AtomicBool::new(false);
        let progress = |_vm: ExportProgressVm| {
            cancel.store(true, Ordering::Relaxed);
            true
        };
        let err = run_export(&conn, &req, &out_root, false, &progress, &cancel, None, 1)
            .expect_err("cancelled mid-run");
        assert!(matches!(err, ExportError::Cancelled));
        assert!(
            sentinel.exists(),
            "pre-existing user file must survive cleanup"
        );
        assert!(
            !scope_dir.join("export.json").exists(),
            "partial output this export wrote must be cleaned up, not left behind"
        );
        assert!(
            !scope_dir.join("transcript.md").exists(),
            "no transcript was written before the cancel"
        );
        drop(conn);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn none_resolver_skips_all_media_but_emits_link() {
        let dir = temp_dir();
        let conn = open_archive_db(&dir).expect("open");
        let media_json = r#"{"mxc":"mxc://e.org/abc","filename":"cat.png"}"#;
        insert(
            &conn,
            "acctA",
            "$m1",
            "!r1",
            100,
            r#"{"body":"cat.png","msgtype":"m.image"}"#,
            Some(media_json),
            None,
            None,
            None,
        );
        let out_root = dir.join("out");
        let mut req = chat_req("acctA", "!r1", out_root.to_str().expect("utf8 path"));
        req.include_media = true;
        let cancel = AtomicBool::new(false);
        let outcome = run_export(
            &conn,
            &req,
            &out_root,
            false,
            &no_progress(),
            &cancel,
            None,
            1,
        )
        .expect("ok");
        assert_eq!(outcome.media_skipped, 1, "None resolver ⇒ skipped");
        assert_eq!(outcome.media_copied, 0);
        let md = std::fs::read_to_string(out_root.join("chat-acctA-r1").join("transcript.md"))
            .expect("md");
        assert!(md.contains("media/"), "relative media link emitted anyway");
        assert!(
            md.contains("not included in this export"),
            "honest note that media bytes were omitted"
        );
        // No bytes were copied, so the media dir must not exist.
        assert!(!out_root.join("chat-acctA-r1").join("media").exists());
        drop(conn);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn resolver_copies_bytes_and_counts_copied() {
        let dir = temp_dir();
        let conn = open_archive_db(&dir).expect("open");
        let media_json = r#"{"mxc":"mxc://e.org/abc","filename":"cat.png"}"#;
        insert(
            &conn,
            "acctA",
            "$m1",
            "!r1",
            100,
            r#"{"body":"cat.png","msgtype":"m.image"}"#,
            Some(media_json),
            None,
            None,
            None,
        );
        let out_root = dir.join("out");
        let mut req = chat_req("acctA", "!r1", out_root.to_str().expect("utf8 path"));
        req.include_media = true;
        let cancel = AtomicBool::new(false);
        let resolver = |_m: &ArchiveMedia, _e: &str| Some(vec![1u8, 2, 3]);
        let boxed: &MediaResolver<'_> = &resolver;
        let outcome = run_export(
            &conn,
            &req,
            &out_root,
            false,
            &no_progress(),
            &cancel,
            Some(boxed),
            1,
        )
        .expect("ok");
        assert_eq!(outcome.media_copied, 1);
        assert_eq!(outcome.media_skipped, 0);
        assert!(out_root.join("chat-acctA-r1").join("media").exists());
        drop(conn);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn cancel_mid_run_deletes_partial_output() {
        let dir = temp_dir();
        let conn = open_archive_db(&dir).expect("open");
        insert(
            &conn,
            "acctA",
            "$e1",
            "!r1",
            100,
            r#"{"body":"hi"}"#,
            None,
            None,
            None,
            None,
        );
        let out_root = dir.join("out");
        let req = chat_req("acctA", "!r1", out_root.to_str().expect("utf8 path"));
        // Pre-set the cancel flag so the very first check trips.
        let cancel = AtomicBool::new(true);
        let err = run_export(
            &conn,
            &req,
            &out_root,
            false,
            &no_progress(),
            &cancel,
            None,
            1,
        )
        .expect_err("cancelled");
        assert!(matches!(err, ExportError::Cancelled));
        // The scope subfolder was cleaned up.
        assert!(
            !out_root.join("chat-acctA-r1").exists(),
            "partial output removed"
        );
        drop(conn);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn everything_scope_spans_all_accounts_chronologically() {
        let dir = temp_dir();
        let conn = open_archive_db(&dir).expect("open");
        insert(
            &conn,
            "acctA",
            "$a1",
            "!r1",
            300,
            r#"{"body":"a-late"}"#,
            None,
            None,
            None,
            None,
        );
        insert(
            &conn,
            "acctB",
            "$b1",
            "!r9",
            100,
            r#"{"body":"b-early"}"#,
            None,
            None,
            None,
            None,
        );
        let out_root = dir.join("out");
        let req = ExportRequestVm {
            scope: ExportScopeKind::Everything,
            account_id: None,
            room_id: None,
            json: true,
            markdown: true,
            include_media: false,
            destination_dir: out_root.to_string_lossy().into_owned(),
        };
        let cancel = AtomicBool::new(false);
        let outcome = run_export(
            &conn,
            &req,
            &out_root,
            false,
            &no_progress(),
            &cancel,
            None,
            1,
        )
        .expect("ok");
        assert_eq!(outcome.messages_written, 2, "both accounts' roots");
        let md =
            std::fs::read_to_string(out_root.join("everything").join("transcript.md")).expect("md");
        let early = md.find("b-early").expect("b-early");
        let late = md.find("a-late").expect("a-late");
        assert!(early < late, "chronological across accounts");
        drop(conn);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn empty_scope_yields_valid_empty_outputs() {
        let dir = temp_dir();
        let conn = open_archive_db(&dir).expect("open");
        let out_root = dir.join("out");
        let req = chat_req("acctA", "!nope", out_root.to_str().expect("utf8 path"));
        let cancel = AtomicBool::new(false);
        let outcome = run_export(
            &conn,
            &req,
            &out_root,
            false,
            &no_progress(),
            &cancel,
            None,
            1,
        )
        .expect("ok");
        assert_eq!(outcome.messages_written, 0);
        let scope_dir = out_root.join("chat-acctA-nope");
        let json = std::fs::read_to_string(scope_dir.join("export.json")).expect("json");
        assert_eq!(json.trim(), "[]", "valid empty JSON array");
        assert!(scope_dir.join("transcript.md").exists(), "header-only md");
        drop(conn);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
