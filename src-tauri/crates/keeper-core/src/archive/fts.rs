//! Offline full-text search over the local archive (Story 5.3, FR-34, NFR-2,
//! AD-12).
//!
//! An FTS5 **external-content** virtual table (`events_fts`, `tokenize='trigram'`)
//! indexes the `body` column of `events`, maintained incrementally by the single
//! archive writer at ingest. Trigram is case-insensitive and language-agnostic
//! (any 3-scalar window, so CJK by construction), which is exactly why queries
//! under 3 Unicode scalar values fall back to an accelerated case-insensitive
//! `LIKE` scan.
//!
//! Everything here is tauri-free and matrix-free: [`search`] runs on a plain
//! [`Connection`] (in production a **fresh read-only connection** opened by the
//! IPC command — WAL permits concurrent readers, so search never touches the
//! writer connection or a live Matrix session and works fully offline). The engine
//! returns [`SearchHitVm`]s carrying the `(account_id, room_id, event_id)`
//! deep-link identifiers the epic AC mandates.

use rusqlite::Connection;

use crate::error::ArchiveError;
use crate::vm::{SearchFilterVm, SearchHitVm};

/// The default (and maximum) number of hits a single [`search`] returns. A bounded
/// result keeps the p95-latency gate honest and the IPC payload small; Story 5.4's
/// UI paginates above this if ever needed.
pub const DEFAULT_LIMIT: i64 = 200;

/// The minimum query length (in Unicode scalar values) the trigram tokenizer can
/// match. A shorter query cannot form a trigram, so [`search`] dispatches to the
/// `LIKE` fallback below this threshold (AD-12).
const TRIGRAM_MIN_CHARS: usize = 3;

/// Archive-native search parameters (Story 5.3). A plain keeper-core struct (the
/// IPC `SearchFilterVm` maps into this), so the engine stays tauri-free.
///
/// Every filter is optional: an empty `account_ids`/`room_ids` list is
/// unrestricted, and `sender`/`start_ts`/`end_ts` are `None` when unset. "Network"
/// is not an archive column — Story 5.4 resolves a Network selection to its
/// `room_ids` set before calling, so the engine never sees bridge state.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SearchFilter {
    /// The user's query text. Length is counted in Unicode scalar values to pick
    /// the trigram-MATCH (≥3) vs `LIKE` (<3) path.
    pub query: String,
    /// Restrict to these keeper account ids; empty ⇒ all accounts.
    pub account_ids: Vec<String>,
    /// Restrict to these room ids; empty ⇒ all rooms. The boundary for both the
    /// "Chat" and "Network" UI filters.
    pub room_ids: Vec<String>,
    /// Restrict to this sender (Matrix user id); `None` ⇒ any sender.
    pub sender: Option<String>,
    /// Lower bound (inclusive) on `origin_ts` in ms since the Unix epoch; `None` ⇒
    /// unbounded below.
    pub start_ts: Option<i64>,
    /// Upper bound (inclusive) on `origin_ts` in ms since the Unix epoch; `None` ⇒
    /// unbounded above.
    pub end_ts: Option<i64>,
    /// Cap on the number of hits; `None` ⇒ [`DEFAULT_LIMIT`]. Clamped to
    /// `[1, DEFAULT_LIMIT]` so a caller can never request an unbounded scan.
    pub limit: Option<i64>,
}

impl From<SearchFilterVm> for SearchFilter {
    /// Map the IPC input VM to the tauri-free engine filter (Story 5.3). A pure
    /// field move — no bridge/session state is involved.
    fn from(vm: SearchFilterVm) -> Self {
        SearchFilter {
            query: vm.query,
            account_ids: vm.account_ids,
            room_ids: vm.room_ids,
            sender: vm.sender,
            start_ts: vm.start_ts,
            end_ts: vm.end_ts,
            limit: vm.limit,
        }
    }
}

/// Create the external-content trigram FTS table for `body` if it does not exist,
/// and — only on fresh creation — populate it once from the existing `events` rows
/// via the FTS5 `'rebuild'` command (Story 5.3).
///
/// Idempotent: on a DB that already has `events_fts`, this is a no-op (no rebuild,
/// so incremental rows added since are never clobbered). `'rebuild'` runs only when
/// the table was just created, which pairs with `backfill_missing_bodies` having
/// already populated every `body` before this is called from `open_archive_db`.
///
/// The create + one-time `'rebuild'` run inside a single transaction: were a crash
/// to land between them, an existing-but-empty `events_fts` would make the
/// exists-check below skip the rebuild forever, silently hiding the whole
/// pre-existing corpus from search. All-or-nothing avoids that — a crash rolls the
/// table back and the next open recreates + rebuilds it.
///
/// A runtime failure of `CREATE VIRTUAL TABLE … USING fts5(… tokenize='trigram')`
/// means the bundled SQLite lacks FTS5 or the trigram tokenizer — a genuine
/// blocker surfaced as an [`ArchiveError::Sqlite`].
pub fn ensure_fts(conn: &Connection) -> Result<(), ArchiveError> {
    if fts_table_exists(conn)? {
        // Already created (and rebuilt) on a prior open; a second 'rebuild' would
        // clobber incremental rows added since, so this is a no-op.
        return Ok(());
    }
    conn.execute_batch("BEGIN IMMEDIATE")
        .map_err(|e| ArchiveError::Sqlite(format!("could not begin fts setup: {e}")))?;
    let setup = (|| {
        conn.execute(
            "CREATE VIRTUAL TABLE IF NOT EXISTS events_fts USING fts5(\
                body, content='events', content_rowid='rowid', tokenize='trigram')",
            [],
        )
        .map_err(|e| ArchiveError::Sqlite(format!("could not create events_fts: {e}")))?;
        // Fresh creation: populate the index once from the (already backfilled)
        // `body` column. `'rebuild'` reads external content by column name.
        conn.execute("INSERT INTO events_fts(events_fts) VALUES('rebuild')", [])
            .map_err(|e| ArchiveError::Sqlite(format!("could not rebuild events_fts: {e}")))?;
        Ok::<(), ArchiveError>(())
    })();
    match setup {
        Ok(()) => conn
            .execute_batch("COMMIT")
            .map_err(|e| ArchiveError::Sqlite(format!("could not commit fts setup: {e}"))),
        Err(e) => {
            let _ = conn.execute_batch("ROLLBACK");
            Err(e)
        }
    }
}

/// Whether the `events_fts` virtual table already exists (its shadow rows appear in
/// `sqlite_master` as a `table` named `events_fts`).
fn fts_table_exists(conn: &Connection) -> Result<bool, ArchiveError> {
    let count: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM sqlite_master WHERE type = 'table' AND name = 'events_fts'",
            [],
            |r| r.get(0),
        )
        .map_err(|e| ArchiveError::Sqlite(format!("could not check events_fts: {e}")))?;
    Ok(count > 0)
}

/// Incrementally index one just-inserted row's `body` into `events_fts` (Story
/// 5.3). Called by the single writer only when the base `INSERT OR IGNORE` actually
/// added a row (rows-affected == 1), keyed by that row's `rowid`. A text-less row
/// (empty `body`) is skipped — nothing to index — so a genuinely empty message is
/// stored but never indexed. Re-synced duplicates never reach here, so a row is
/// never double-indexed.
pub fn index_body(conn: &Connection, rowid: i64, body: &str) -> Result<(), ArchiveError> {
    if body.is_empty() {
        return Ok(());
    }
    conn.execute(
        "INSERT INTO events_fts(rowid, body) VALUES (?1, ?2)",
        rusqlite::params![rowid, body],
    )
    .map(|_| ())
    .map_err(|e| ArchiveError::Sqlite(format!("could not index body: {e}")))
}

/// Search the archive, returning at most one [`SearchHitVm`] per logical message
/// (Story 5.3, FR-34).
///
/// Dispatch on `filter.query` length in Unicode scalar values: ≥3 uses the trigram
/// `events_fts MATCH` index; <3 (including an empty query) falls back to a
/// case-insensitive `body LIKE '%q%'` scan over `events`. The query text is always
/// bound as a single parameter — for MATCH it is wrapped in double quotes so text
/// like `AND`/`OR`/`*` is matched literally, never parsed as an FTS operator.
///
/// Filters (`account_ids`/`room_ids`/`sender`/`start_ts`/`end_ts`) are all optional
/// (an empty id list is unrestricted). When `honor_deletions` is `true`, rows with
/// `redacted_ts` set are excluded (content stays on disk — this only gates
/// retrieval). Results are deduplicated to one hit per chain root
/// (`relates_to_event_id` if present, else `event_id`) — so a match on a prior edit
/// version returns a single hit whose `event_id` is the chain root — ordered by
/// `origin_ts DESC` and capped at the (clamped) limit.
pub fn search(
    conn: &Connection,
    filter: &SearchFilter,
    honor_deletions: bool,
) -> Result<Vec<SearchHitVm>, ArchiveError> {
    let limit = filter
        .limit
        .unwrap_or(DEFAULT_LIMIT)
        .clamp(1, DEFAULT_LIMIT);
    let use_trigram = filter.query.chars().count() >= TRIGRAM_MIN_CHARS;

    // Assemble the WHERE clause and its bound params. The base row source differs by
    // path: trigram joins `events_fts` on rowid; LIKE scans `events` directly.
    let mut params: Vec<Box<dyn rusqlite::ToSql>> = Vec::new();
    let mut clauses: Vec<String> = Vec::new();

    let base_from = if use_trigram {
        // Bind the whole query as one double-quoted FTS string so operator-like text
        // is matched literally. Any embedded double quotes are doubled to stay inside
        // the quoted string.
        let match_arg = format!("\"{}\"", filter.query.replace('"', "\"\""));
        clauses.push("events_fts MATCH ?".to_owned());
        params.push(Box::new(match_arg));
        "events JOIN events_fts ON events_fts.rowid = events.rowid"
    } else {
        // Case-insensitive substring scan. LIKE is ASCII-case-insensitive by default;
        // lowercasing both sides makes it case-insensitive for the ASCII range without
        // depending on the query being pre-normalized. The query's LIKE
        // metacharacters (`%`, `_`, `\`) are escaped (with `ESCAPE '\'`) so a short
        // query like "a%" matches the literal text, not a wildcard. An empty query
        // matches every row (every body contains the empty substring), bounded by the
        // limit.
        clauses.push("LOWER(events.body) LIKE '%' || LOWER(?) || '%' ESCAPE '\\'".to_owned());
        params.push(Box::new(escape_like(&filter.query)));
        "events"
    };

    if !filter.account_ids.is_empty() {
        clauses.push(format!(
            "events.account_id IN ({})",
            placeholders(filter.account_ids.len())
        ));
        for id in &filter.account_ids {
            params.push(Box::new(id.clone()));
        }
    }
    if !filter.room_ids.is_empty() {
        clauses.push(format!(
            "events.room_id IN ({})",
            placeholders(filter.room_ids.len())
        ));
        for id in &filter.room_ids {
            params.push(Box::new(id.clone()));
        }
    }
    if let Some(sender) = &filter.sender {
        clauses.push("events.sender = ?".to_owned());
        params.push(Box::new(sender.clone()));
    }
    if let Some(start_ts) = filter.start_ts {
        clauses.push("events.origin_ts >= ?".to_owned());
        params.push(Box::new(start_ts));
    }
    if let Some(end_ts) = filter.end_ts {
        clauses.push("events.origin_ts <= ?".to_owned());
        params.push(Box::new(end_ts));
    }
    if honor_deletions {
        // Retrieval-gating only (content stays on disk): withhold remotely-redacted
        // content in two complementary ways.
        //
        // 1. Exclude the matched row itself if *it* is redacted, so a redacted edit
        //    version can never become a message's representative hit. Rows are ordered
        //    newest-first and deduped to the first seen per chain root, so without this
        //    a redacted edit of a *surviving* original would surface as the hit — with
        //    its edited-away body and `redacted = true` even though honoring is on,
        //    leaking withheld content and violating the `SearchHitVm.redacted` contract.
        clauses.push("events.redacted_ts IS NULL".to_owned());
        // 2. Exclude the whole logical message when its chain *root* is redacted. A
        //    remote redaction marks the original row (the message's own event id); an
        //    edit row's own `redacted_ts` is NULL, so clause (1) alone would let an
        //    edited-then-redacted message leak back in through a surviving, un-redacted
        //    edit sibling. Gate on the chain root's redaction too (account-scoped, so a
        //    same event id under another account is unaffected).
        clauses.push(
            "NOT EXISTS (SELECT 1 FROM events root \
             WHERE root.account_id = events.account_id \
               AND root.event_id = COALESCE(events.relates_to_event_id, events.event_id) \
               AND root.redacted_ts IS NOT NULL)"
                .to_owned(),
        );
    }

    let where_sql = clauses.join(" AND ");
    // Read a generous window so chain-root dedup (below) still yields up to `limit`
    // distinct logical messages even when several matched rows share a chain root.
    // The scan stays bounded — trigram MATCH / the LIKE index keep it cheap.
    let scan_cap = limit.saturating_mul(4);
    let sql = format!(
        "SELECT events.account_id, events.room_id, events.event_id, \
                events.relates_to_event_id, events.sender, events.body, \
                events.origin_ts, events.redacted_ts \
         FROM {base_from} \
         WHERE {where_sql} \
         ORDER BY events.origin_ts DESC \
         LIMIT {scan_cap}"
    );

    let mut stmt = conn
        .prepare(&sql)
        .map_err(|e| ArchiveError::Sqlite(format!("could not prepare search: {e}")))?;
    let param_refs: Vec<&dyn rusqlite::ToSql> = params.iter().map(|p| p.as_ref()).collect();
    let rows = stmt
        .query_map(param_refs.as_slice(), |r| {
            Ok(SearchRow {
                account_id: r.get(0)?,
                room_id: r.get(1)?,
                event_id: r.get(2)?,
                relates_to_event_id: r.get::<_, Option<String>>(3)?,
                sender: r.get(4)?,
                body: r.get(5)?,
                origin_ts: r.get(6)?,
                redacted_ts: r.get::<_, Option<i64>>(7)?,
            })
        })
        .map_err(|e| ArchiveError::Sqlite(format!("could not run search: {e}")))?;

    let mut hits: Vec<SearchHitVm> = Vec::new();
    // Dedup per logical message, scoped by account: the same Matrix event id can
    // legitimately exist under two of the user's accounts (both joined to one room),
    // and those are distinct, account-attributed results that must not collapse into
    // one. Key on (account_id, chain root), not the bare event id.
    let mut seen_roots: std::collections::HashSet<(String, String)> =
        std::collections::HashSet::new();
    for row in rows {
        let row =
            row.map_err(|e| ArchiveError::Sqlite(format!("could not read search row: {e}")))?;
        // One hit per logical message: dedup on the chain root (the edit target when
        // this row is an edit, else the row's own event id). The returned `event_id`
        // is that root so every version deep-links to the same timeline item.
        let root = row
            .relates_to_event_id
            .clone()
            .unwrap_or_else(|| row.event_id.clone());
        if !seen_roots.insert((row.account_id.clone(), root.clone())) {
            continue;
        }
        hits.push(SearchHitVm {
            account_id: row.account_id,
            room_id: row.room_id,
            event_id: root,
            sender: row.sender,
            body: row.body,
            timestamp: row.origin_ts,
            redacted: row.redacted_ts.is_some(),
        });
        if hits.len() as i64 >= limit {
            break;
        }
    }
    Ok(hits)
}

/// One raw matched `events` row read before chain-root dedup.
struct SearchRow {
    account_id: String,
    room_id: String,
    event_id: String,
    relates_to_event_id: Option<String>,
    sender: String,
    body: String,
    origin_ts: i64,
    redacted_ts: Option<i64>,
}

/// Build `?, ?, …` with `n` placeholders for an `IN (…)` clause.
fn placeholders(n: usize) -> String {
    std::iter::repeat_n("?", n).collect::<Vec<_>>().join(", ")
}

/// Escape SQL `LIKE` metacharacters (`\`, `%`, `_`) so a short-query substring scan
/// matches them literally. Paired with `ESCAPE '\'` in the `LIKE` clause: the
/// backslash escapes itself and the two wildcards, and nothing else is special.
fn escape_like(query: &str) -> String {
    let mut out = String::with_capacity(query.len());
    for ch in query.chars() {
        if matches!(ch, '\\' | '%' | '_') {
            out.push('\\');
        }
        out.push(ch);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::archive::db::open_archive_db;
    use crate::archive::ArchiveEvent;
    use std::path::PathBuf;

    fn temp_dir() -> PathBuf {
        let mut dir = std::env::temp_dir();
        dir.push(format!(
            "keeper-archive-fts-unit-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(0)
        ));
        dir
    }

    fn ev(event_id: &str, origin_ts: i64, body: &str) -> ArchiveEvent {
        ArchiveEvent {
            account_id: "acctA".to_owned(),
            event_id: event_id.to_owned(),
            room_id: "!room:e.org".to_owned(),
            sender: "@u:e.org".to_owned(),
            origin_ts,
            event_type: "m.room.message".to_owned(),
            content_json: format!(r#"{{"body":"{body}"}}"#),
            body: body.to_owned(),
            media: None,
            relates_to_event_id: None,
            rel_type: None,
        }
    }

    /// Insert an event + index it exactly as the writer does.
    fn insert_and_index(conn: &Connection, e: &ArchiveEvent) {
        let rowid = crate::archive::db::insert_event(conn, e, None, 0)
            .expect("insert")
            .expect("row inserted");
        index_body(conn, rowid, &e.body).expect("index");
    }

    /// The dispatch boundary is exactly 3 Unicode scalar values: a 3-char query
    /// uses trigram MATCH, a 2-char query the LIKE fallback. Both find the row.
    #[test]
    fn dispatch_boundary_at_two_versus_three_chars() {
        let dir = temp_dir();
        let conn = open_archive_db(&dir).expect("open");
        insert_and_index(&conn, &ev("$e1", 100, "hello world"));
        // 3 chars → trigram.
        let three = SearchFilter {
            query: "ell".to_owned(),
            ..Default::default()
        };
        assert_eq!(search(&conn, &three, false).expect("search").len(), 1);
        // 2 chars → LIKE fallback still finds the substring.
        let two = SearchFilter {
            query: "he".to_owned(),
            ..Default::default()
        };
        assert_eq!(search(&conn, &two, false).expect("search").len(), 1);
        drop(conn);
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// An empty-body row is stored but not indexed: no indexed document is written
    /// for it (its `rowid` never appears in the FTS index's per-document shadow
    /// table). A non-empty row alongside it *is* indexed, proving the skip is
    /// body-conditional, not a blanket no-op.
    #[test]
    fn empty_body_is_stored_but_not_indexed() {
        let dir = temp_dir();
        let conn = open_archive_db(&dir).expect("open");
        // A genuinely text-less row (media-only): stored, but index_body skips it.
        let mut empty = ev("$empty", 100, "");
        empty.content_json = r#"{"msgtype":"m.image"}"#.to_owned();
        let empty_rowid = crate::archive::db::insert_event(&conn, &empty, None, 0)
            .expect("insert")
            .expect("inserted");
        index_body(&conn, empty_rowid, &empty.body).expect("index skips empty");
        // A text row for contrast — this one IS indexed.
        insert_and_index(&conn, &ev("$full", 200, "indexed content here"));

        // `events_fts_docsize` holds one row per *indexed* document, keyed by the
        // content rowid. External-content `COUNT(*) FROM events_fts` counts the
        // content table (events), so it is not a signal for "was indexed".
        let indexed_docs: i64 = conn
            .query_row("SELECT COUNT(*) FROM events_fts_docsize", [], |r| r.get(0))
            .expect("count docsize");
        assert_eq!(indexed_docs, 1, "only the non-empty body is indexed");
        let empty_indexed: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM events_fts_docsize WHERE id = ?1",
                rusqlite::params![empty_rowid],
                |r| r.get(0),
            )
            .expect("count empty doc");
        assert_eq!(empty_indexed, 0, "empty body row must not be indexed");
        drop(conn);
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// A match on a prior edit version dedups to one hit whose `event_id` is the
    /// chain root.
    #[test]
    fn chain_root_dedup_returns_single_hit() {
        let dir = temp_dir();
        let conn = open_archive_db(&dir).expect("open");
        insert_and_index(&conn, &ev("$orig", 100, "alpha original"));
        let mut edit = ev("$edit", 200, "alpha edited");
        edit.relates_to_event_id = Some("$orig".to_owned());
        edit.rel_type = Some("m.replace".to_owned());
        insert_and_index(&conn, &edit);
        // "alpha" matches BOTH versions, but the result dedups to one hit rooted at
        // $orig.
        let hits = search(
            &conn,
            &SearchFilter {
                query: "alpha".to_owned(),
                ..Default::default()
            },
            false,
        )
        .expect("search");
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].event_id, "$orig");
        drop(conn);
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Honoring remote deletions withholds a message whose original was redacted even
    /// when a surviving, un-redacted edit sibling matches the query. A remote
    /// redaction marks the original row only, so a naive per-row `redacted_ts IS NULL`
    /// gate would leak the message back in through its edit.
    #[test]
    fn honor_deletions_excludes_edited_then_redacted_message() {
        let dir = temp_dir();
        let conn = open_archive_db(&dir).expect("open");
        insert_and_index(&conn, &ev("$orig", 100, "banana original"));
        let mut edit = ev("$edit", 200, "banana edited");
        edit.relates_to_event_id = Some("$orig".to_owned());
        edit.rel_type = Some("m.replace".to_owned());
        insert_and_index(&conn, &edit);
        // Redact the ORIGINAL (as a remote redaction does); the edit row is untouched.
        crate::archive::db::mark_redacted(&conn, "acctA", "$orig", 300).expect("redact");
        let q = SearchFilter {
            query: "banana".to_owned(),
            ..Default::default()
        };
        // honor OFF: still retrievable (one deduped hit).
        assert_eq!(search(&conn, &q, false).expect("search").len(), 1);
        // honor ON: the whole logical message is withheld — the edit must not leak it.
        assert_eq!(search(&conn, &q, true).expect("search").len(), 0);
        drop(conn);
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Honoring remote deletions never surfaces a redacted *edit* version, even when
    /// the message's original (chain root) survives. Rows are ordered newest-first, so
    /// a redacted edit would otherwise become the representative hit — leaking its
    /// edited-away body with `redacted = true` while honoring is on (violating the
    /// `SearchHitVm.redacted` contract). The surviving original must represent the
    /// message instead, and text that exists only in the redacted edit must not match.
    #[test]
    fn honor_deletions_hides_redacted_edit_of_surviving_root() {
        let dir = temp_dir();
        let conn = open_archive_db(&dir).expect("open");
        insert_and_index(&conn, &ev("$orig", 100, "grape original"));
        let mut edit = ev("$edit", 200, "grape confidential");
        edit.relates_to_event_id = Some("$orig".to_owned());
        edit.rel_type = Some("m.replace".to_owned());
        insert_and_index(&conn, &edit);
        // Redact the EDIT row itself; the original (chain root) stays intact.
        crate::archive::db::mark_redacted(&conn, "acctA", "$edit", 300).expect("redact");

        // A term in both versions: honor ON returns exactly one hit, represented by the
        // surviving original — never the redacted edit's body or flag.
        let shared = SearchFilter {
            query: "grape".to_owned(),
            ..Default::default()
        };
        let hits = search(&conn, &shared, true).expect("search");
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].event_id, "$orig");
        assert_eq!(hits[0].body, "grape original");
        assert!(
            !hits[0].redacted,
            "honor ON must never return a redacted hit"
        );

        // A term that exists ONLY in the redacted edit is withheld under honor ON, but
        // still retrievable with honoring off (content stays on disk).
        let edit_only = SearchFilter {
            query: "confidential".to_owned(),
            ..Default::default()
        };
        assert_eq!(search(&conn, &edit_only, true).expect("search").len(), 0);
        assert_eq!(search(&conn, &edit_only, false).expect("search").len(), 1);
        drop(conn);
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// The same Matrix event id under two accounts (both joined to one room) yields
    /// two distinct, account-attributed hits — dedup is scoped per account, not on the
    /// bare event id.
    #[test]
    fn cross_account_same_event_id_not_deduped() {
        let dir = temp_dir();
        let conn = open_archive_db(&dir).expect("open");
        let mut a = ev("$shared", 100, "cherry pie");
        a.account_id = "acctA".to_owned();
        let mut b = ev("$shared", 100, "cherry pie");
        b.account_id = "acctB".to_owned();
        insert_and_index(&conn, &a);
        insert_and_index(&conn, &b);
        let hits = search(
            &conn,
            &SearchFilter {
                query: "cherry".to_owned(),
                ..Default::default()
            },
            false,
        )
        .expect("search");
        assert_eq!(
            hits.len(),
            2,
            "same event id under two accounts is two results"
        );
        drop(conn);
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// A short (<3 char) query containing a LIKE metacharacter is matched literally:
    /// each of the three escapable characters (`%`, `_`, and the `\` escape char
    /// itself) matches only the body that literally contains it, never as a wildcard.
    #[test]
    fn short_query_wildcard_is_literal() {
        let dir = temp_dir();
        let conn = open_archive_db(&dir).expect("open");
        insert_and_index(&conn, &ev("$pct", 100, "a%b"));
        insert_and_index(&conn, &ev("$any", 200, "axb"));
        insert_and_index(&conn, &ev("$und", 300, "a_b"));
        insert_and_index(&conn, &ev("$bs", 400, r"a\b"));

        // "%" is literal, not a match-any wildcard: only the "a%b" row.
        let pct = search(
            &conn,
            &SearchFilter {
                query: "a%".to_owned(),
                ..Default::default()
            },
            false,
        )
        .expect("search");
        assert_eq!(pct.len(), 1, "% is matched literally");
        assert_eq!(pct[0].event_id, "$pct");

        // "_" is literal, not a single-char wildcard: only the "a_b" row (an unescaped
        // "_" would spuriously match "axb", "a%b", and "a\\b" too).
        let und = search(
            &conn,
            &SearchFilter {
                query: "a_".to_owned(),
                ..Default::default()
            },
            false,
        )
        .expect("search");
        assert_eq!(und.len(), 1, "_ is matched literally");
        assert_eq!(und[0].event_id, "$und");

        // The ESCAPE char "\" is itself escaped: only the "a\\b" row.
        let bs = search(
            &conn,
            &SearchFilter {
                query: "a\\".to_owned(),
                ..Default::default()
            },
            false,
        )
        .expect("search");
        assert_eq!(bs.len(), 1, "backslash is matched literally");
        assert_eq!(bs[0].event_id, "$bs");
        drop(conn);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
