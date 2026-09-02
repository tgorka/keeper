//! Conversations and their messages in `keeper.db` (Story 61.4, FR-378).
//!
//! Two more tables beside [`super::store`]'s, in the same database and under
//! exactly the same rules: WAL, a busy timeout because every caller opens its
//! own short-lived connection, `CREATE TABLE IF NOT EXISTS` on every open so a
//! cold install and an upgrade take the same path, and **no JSON blob in a
//! row** (AD-139).
//!
//! # Why keeper's own store is the truth
//!
//! Hermes keeps real server-side sessions and it is tempting to make them the
//! record. The epic decided against it for three sourced reasons: compression
//! mints a *new* continuation session with a renamed title, the stored-response
//! cache is 100 rows LRU, and Ollama has no session concept at all. So a
//! session is keeper's, and a Hermes `session_id` is persisted *beside* the row
//! ([`BotSession::remote_session_id`]) as a reference rather than as identity.
//!
//! # Every metadata field is its own column
//!
//! `model`, `provider_id`, the three token counts, time-to-first-token, total
//! duration, finish reason, the provider's request id, the tool-call count and
//! the partial flag are seventeen scalar columns and not one snapshot. Story
//! 61.8 shows them, and a caption that can be filtered, sorted and migrated is
//! the difference between a column and a blob. Each number is nullable
//! precisely because an endpoint that omits `usage` is a fact about the
//! endpoint: an absent number stays absent and never becomes zero.
//!
//! # The partial flag is the crash contract
//!
//! An assistant row is inserted **before** the first delta arrives, with
//! `partial = 1`, and is rewritten as the stream lands
//! ([`set_message_content`]). Only [`close_message`] clears the flag. So a
//! stream that dies — a killed process, a dropped socket, a pressed Stop —
//! leaves a row marked partial and never leaves nothing, which is the whole of
//! the epic's "cancellation leaves a partial assistant row persisted and marked
//! partial, never a silently discarded reply".
//!
//! Synchronous throughout, for [`crate::registry`]'s reason: a rusqlite
//! `Connection` is never held across an `.await`.

use std::path::{Path, PathBuf};

use rusqlite::Connection;

use crate::error::{CoreError, PlatformError};

/// Resolve the `keeper.db` path under a data directory — the same file
/// [`crate::registry`] and [`super::store`] use.
fn db_path(data_dir: &Path) -> PathBuf {
    data_dir.join("keeper.db")
}

/// How long a contended writer waits for the write lock. [`super::store`]'s
/// timeout, for its reason: a progressive delta write landing while a session
/// is being archived must wait rather than return `SQLITE_BUSY` to a stream
/// that has nothing useful to say about it.
const BUSY_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(5);

/// Open `keeper.db` in WAL mode, ensuring the conversation schema exists.
/// Every call is idempotent.
///
/// **Deliberately no foreign key onto `bots`.** Two reasons, and the first is
/// mechanical: this opener does not own the `bots` table —
/// [`super::store::open`] does — and the bundled SQLite this crate links
/// enforces declared foreign keys, so a `REFERENCES bots(id)` here fails every
/// insert made through a connection that never created that table. The second
/// is the design: unpinning a bot must **not** take its conversations with it
/// ([`super::store::delete_bot`] leaves them), because a conversation is a
/// record of something that happened and unpinning is not a statement about
/// the past. `bot_messages.session_id` does carry its declaration, because
/// this opener creates both halves — and [`delete_session`] still removes them
/// explicitly inside one transaction, so the order and the atomicity of the
/// cleanup live somewhere a test can read.
fn open(data_dir: &Path) -> Result<Connection, CoreError> {
    std::fs::create_dir_all(data_dir).map_err(|e| {
        CoreError::Platform(PlatformError::DirUnavailable(format!(
            "could not create data dir: {e}"
        )))
    })?;
    let conn = Connection::open(db_path(data_dir))
        .map_err(|e| CoreError::Internal(format!("could not open keeper.db: {e}")))?;
    conn.busy_timeout(BUSY_TIMEOUT)
        .map_err(|e| CoreError::Internal(format!("could not set busy timeout: {e}")))?;
    conn.pragma_update(None, "journal_mode", "WAL")
        .map_err(|e| CoreError::Internal(format!("could not set WAL mode: {e}")))?;
    // One row per conversation (Story 61.4, FR-378). `archived` is an integer
    // flag rather than a nullable timestamp because Story 61.6 makes archiving
    // reversible and a reversal has no honest "unarchived at" to write.
    // `remote_session_id` is the Hermes reference the epic keeps beside the
    // row, nullable because Ollama has no such concept.
    //
    // Any column added later MUST be nullable or carry a DEFAULT and MUST go
    // through an additive `ensure_*` helper rather than into this statement:
    // every INSERT below names its columns, so a NOT NULL column with no
    // default would make an older binary's writes fail against a newer schema
    // (NFR-43's other half).
    conn.execute(
        "CREATE TABLE IF NOT EXISTS bot_sessions(\
            id TEXT PRIMARY KEY, \
            bot_id TEXT NOT NULL, \
            provider_id TEXT NOT NULL, \
            title TEXT NOT NULL, \
            created_ms INTEGER NOT NULL, \
            updated_ms INTEGER NOT NULL, \
            archived INTEGER NOT NULL DEFAULT 0, \
            remote_session_id TEXT\
        )",
        [],
    )
    .map_err(|e| CoreError::Internal(format!("could not ensure bot_sessions schema: {e}")))?;
    // One row per message (Story 61.4, FR-384). `role` and `finish_reason` are
    // TEXT rather than enums for `TaskVm.kind`'s reason: a row a newer keeper
    // wrote must reach the view as the spelling it has, so the pane can show it
    // instead of hiding it (NFR-43).
    conn.execute(
        "CREATE TABLE IF NOT EXISTS bot_messages(\
            id TEXT PRIMARY KEY, \
            session_id TEXT NOT NULL REFERENCES bot_sessions(id), \
            seq INTEGER NOT NULL, \
            role TEXT NOT NULL, \
            content TEXT NOT NULL, \
            model TEXT, \
            provider_id TEXT, \
            prompt_tokens INTEGER, \
            completion_tokens INTEGER, \
            total_tokens INTEGER, \
            ttft_ms INTEGER, \
            duration_ms INTEGER, \
            finish_reason TEXT, \
            request_id TEXT, \
            tool_call_count INTEGER NOT NULL DEFAULT 0, \
            partial INTEGER NOT NULL DEFAULT 0, \
            created_ms INTEGER NOT NULL\
        )",
        [],
    )
    .map_err(|e| CoreError::Internal(format!("could not ensure bot_messages schema: {e}")))?;
    // The only index either table needs: every read of a conversation is
    // "this session's rows, in order", and a bare `SELECT` has unspecified row
    // order in SQLite.
    conn.execute(
        "CREATE INDEX IF NOT EXISTS bot_messages_in_order \
         ON bot_messages (session_id, seq)",
        [],
    )
    .map_err(|e| CoreError::Internal(format!("could not ensure bot_messages index: {e}")))?;
    // Story 61.6's two reads, and neither is served by the index above. The
    // list orders by activity and pages, so `bot_sessions` is read in
    // `updated_ms DESC, id DESC` order; and the activity of a conversation is
    // the newest `created_ms` among its messages, which is a per-session MAX
    // over a column the `(session_id, seq)` index does not carry.
    conn.execute(
        "CREATE INDEX IF NOT EXISTS bot_sessions_by_activity \
         ON bot_sessions (updated_ms DESC, id DESC)",
        [],
    )
    .map_err(|e| CoreError::Internal(format!("could not ensure bot_sessions index: {e}")))?;
    conn.execute(
        "CREATE INDEX IF NOT EXISTS bot_messages_by_time \
         ON bot_messages (session_id, created_ms)",
        [],
    )
    .map_err(|e| CoreError::Internal(format!("could not ensure bot_messages time index: {e}")))?;
    Ok(conn)
}

/// One conversation with one bot (Story 61.4, FR-381).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BotSession {
    /// The session's opaque id.
    pub id: String,
    /// The bot this conversation is with.
    pub bot_id: String,
    /// The bot's provider, denormalized so a listing can group by tenant
    /// without joining — and kept because a session outlives an edit of the
    /// bot that started it.
    pub provider_id: String,
    /// The title, minted locally from the first user message. Never by a
    /// second model call: a silent extra request to a paid endpoint is exactly
    /// the surprise this app does not ship.
    pub title: String,
    /// When the conversation started, ms since the Unix epoch (UTC).
    pub created_ms: i64,
    /// When it last changed, ms since the Unix epoch (UTC).
    pub updated_ms: i64,
    /// Whether it has been archived (reversibly).
    pub archived: bool,
    /// The remote's own session id, where the far side has one. A reference,
    /// never the truth — see the module docs.
    pub remote_session_id: Option<String>,
}

/// One message of one conversation (Story 61.4, FR-384).
///
/// Everything Story 61.8 will show is here as a scalar, and everything the
/// endpoint may omit is an `Option` so an absent number renders as absent.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BotMessage {
    /// The message's opaque id.
    pub id: String,
    /// The conversation it belongs to.
    pub session_id: String,
    /// Its position in that conversation, ascending from 0. Assigned by
    /// [`append_message`]; the value on an input struct is ignored.
    pub seq: i64,
    /// `user` | `assistant` | `system` | `tool`, stored verbatim.
    pub role: String,
    /// The text. For an assistant row this grows as the stream lands.
    pub content: String,
    /// The model the server says answered, where it said.
    pub model: Option<String>,
    /// The provider that answered, where one did.
    pub provider_id: Option<String>,
    /// Prompt tokens, where the endpoint reported them.
    pub prompt_tokens: Option<i64>,
    /// Completion tokens, where the endpoint reported them.
    pub completion_tokens: Option<i64>,
    /// Total tokens, where the endpoint reported them.
    pub total_tokens: Option<i64>,
    /// Milliseconds to the first delta of any kind — measured, not estimated.
    pub ttft_ms: Option<i64>,
    /// Milliseconds from request to end of stream.
    pub duration_ms: Option<i64>,
    /// Why the model stopped, in the provider's own word where it invented one.
    pub finish_reason: Option<String>,
    /// The provider's completion id.
    pub request_id: Option<String>,
    /// How many tool calls this turn made.
    pub tool_call_count: i64,
    /// Whether this row is still — or forever — incomplete. See the module
    /// docs: the flag is the crash contract, not a progress indicator.
    pub partial: bool,
    /// When the row was written, ms since the Unix epoch (UTC).
    pub created_ms: i64,
}

/// Everything [`close_message`] writes, as one argument.
///
/// A struct rather than thirteen positionals, for
/// `keeper_sync::db::TaskRunClose`'s reason: four `Option<i64>`s and three
/// `Option<&str>`s in a row is a call site nobody can read or safely reorder.
#[derive(Debug, Clone, Default)]
pub struct MessageClose<'a> {
    /// The row to close.
    pub id: &'a str,
    /// The final text.
    pub content: &'a str,
    /// The model that answered, where the server said.
    pub model: Option<&'a str>,
    /// Prompt tokens, where reported.
    pub prompt_tokens: Option<i64>,
    /// Completion tokens, where reported.
    pub completion_tokens: Option<i64>,
    /// Total tokens, where reported.
    pub total_tokens: Option<i64>,
    /// Milliseconds to the first delta.
    pub ttft_ms: Option<i64>,
    /// Milliseconds to the end of the stream.
    pub duration_ms: Option<i64>,
    /// Why it stopped.
    pub finish_reason: Option<&'a str>,
    /// The provider's completion id.
    pub request_id: Option<&'a str>,
    /// How many tool calls the turn made.
    pub tool_call_count: i64,
    /// Whether the row is still incomplete. `true` for a cancelled or broken
    /// stream, which is the case this flag exists for.
    pub partial: bool,
}

/// The `SELECT` column list every session read shares, so the column order the
/// mapper assumes is written exactly once.
const SESSION_COLUMNS: &str =
    "id, bot_id, provider_id, title, created_ms, updated_ms, archived, remote_session_id";

/// The `SELECT` column list every message read shares.
const MESSAGE_COLUMNS: &str = "id, session_id, seq, role, content, model, provider_id, \
     prompt_tokens, completion_tokens, total_tokens, ttft_ms, duration_ms, finish_reason, \
     request_id, tool_call_count, partial, created_ms";

/// Insert one conversation (Story 61.4).
///
/// Fails when `id` already exists (PRIMARY KEY), for
/// [`super::store::insert_provider`]'s reason: a collision means the caller
/// minted a duplicate, and overwriting would silently retarget an existing
/// conversation's messages.
pub fn insert_session(data_dir: &Path, session: &BotSession) -> Result<(), CoreError> {
    let conn = open(data_dir)?;
    conn.execute(
        "INSERT INTO bot_sessions(id, bot_id, provider_id, title, created_ms, updated_ms, \
         archived, remote_session_id) VALUES(?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
        rusqlite::params![
            session.id,
            session.bot_id,
            session.provider_id,
            session.title,
            session.created_ms,
            session.updated_ms,
            i64::from(session.archived),
            session.remote_session_id,
        ],
    )
    .map_err(|e| CoreError::Internal(format!("could not insert bot session: {e}")))?;
    Ok(())
}

/// Read one conversation, or `None` when there is none (Story 61.4).
pub fn get_session(data_dir: &Path, session_id: &str) -> Result<Option<BotSession>, CoreError> {
    let conn = open(data_dir)?;
    let mut stmt = conn
        .prepare(&format!(
            "SELECT {SESSION_COLUMNS} FROM bot_sessions WHERE id = ?1"
        ))
        .map_err(|e| CoreError::Internal(format!("could not prepare session read: {e}")))?;
    let mut rows = stmt
        .query_map([session_id], map_session_row)
        .map_err(|e| CoreError::Internal(format!("could not read bot session: {e}")))?;
    match rows.next() {
        None => Ok(None),
        Some(row) => Ok(Some(row.map_err(|e| {
            CoreError::Internal(format!("could not read bot session: {e}"))
        })?)),
    }
}

/// Every conversation, newest activity first (Story 61.4, FR-381).
///
/// `ORDER BY updated_ms DESC, id DESC`: the list a person reads is "what I was
/// last talking about", and the id breaks a tie so two sessions touched inside
/// one millisecond still list identically on every read rather than swapping
/// places between opens.
///
/// `include_archived` widens rather than switches: an archive view wants both,
/// because a list that hid the live ones the moment you asked for the archived
/// ones would be two lists pretending to be one filter.
pub fn list_sessions(
    data_dir: &Path,
    include_archived: bool,
) -> Result<Vec<BotSession>, CoreError> {
    let conn = open(data_dir)?;
    let filter = if include_archived {
        ""
    } else {
        "WHERE archived = 0 "
    };
    let mut stmt = conn
        .prepare(&format!(
            "SELECT {SESSION_COLUMNS} FROM bot_sessions {filter}ORDER BY updated_ms DESC, id DESC"
        ))
        .map_err(|e| CoreError::Internal(format!("could not prepare session listing: {e}")))?;
    let rows = stmt
        .query_map([], map_session_row)
        .map_err(|e| CoreError::Internal(format!("could not list bot sessions: {e}")))?;
    let mut out = Vec::new();
    for row in rows {
        out.push(row.map_err(|e| CoreError::Internal(format!("could not read session: {e}")))?);
    }
    Ok(out)
}

/// Rename one conversation, stamping the change (Story 61.4, FR-381).
///
/// Returns whether a row matched, for [`super::store::update_provider`]'s
/// reason: a rename that saves into nothing and reports success is the
/// affordance AD-27 forbids.
pub fn set_session_title(
    data_dir: &Path,
    session_id: &str,
    title: &str,
    updated_ms: i64,
) -> Result<bool, CoreError> {
    let conn = open(data_dir)?;
    let changed = conn
        .execute(
            "UPDATE bot_sessions SET title = ?2, updated_ms = ?3 WHERE id = ?1",
            rusqlite::params![session_id, title, updated_ms],
        )
        .map_err(|e| CoreError::Internal(format!("could not rename bot session: {e}")))?;
    Ok(changed > 0)
}

/// Archive or unarchive one conversation (Story 61.4, FR-381). Returns whether
/// a row matched.
pub fn set_session_archived(
    data_dir: &Path,
    session_id: &str,
    archived: bool,
    updated_ms: i64,
) -> Result<bool, CoreError> {
    let conn = open(data_dir)?;
    let changed = conn
        .execute(
            "UPDATE bot_sessions SET archived = ?2, updated_ms = ?3 WHERE id = ?1",
            rusqlite::params![session_id, i64::from(archived), updated_ms],
        )
        .map_err(|e| CoreError::Internal(format!("could not archive bot session: {e}")))?;
    Ok(changed > 0)
}

/// Record (or clear) the remote's own session id (Story 61.4). Returns whether
/// a row matched.
///
/// Both directions, including the `None`: a Hermes gateway that compressed a
/// conversation into a successor leaves the old id naming nothing, and a setter
/// that only ever wrote `Some` would make "keeper no longer knows" unreachable.
pub fn set_session_remote_id(
    data_dir: &Path,
    session_id: &str,
    remote_session_id: Option<&str>,
) -> Result<bool, CoreError> {
    let conn = open(data_dir)?;
    let changed = conn
        .execute(
            "UPDATE bot_sessions SET remote_session_id = ?2 WHERE id = ?1",
            rusqlite::params![session_id, remote_session_id],
        )
        .map_err(|e| CoreError::Internal(format!("could not write remote session id: {e}")))?;
    Ok(changed > 0)
}

/// Stamp a conversation as touched (Story 61.4). Returns whether a row
/// matched.
///
/// Its own function rather than a side effect of [`append_message`], because
/// the two are different facts: a message is appended once, and a conversation
/// is touched by an append, a rename, an archive and a retry. Folding them
/// would mean a retry that replaced a row left the list order describing the
/// deleted one.
pub fn touch_session(
    data_dir: &Path,
    session_id: &str,
    updated_ms: i64,
) -> Result<bool, CoreError> {
    let conn = open(data_dir)?;
    let changed = conn
        .execute(
            "UPDATE bot_sessions SET updated_ms = ?2 WHERE id = ?1",
            rusqlite::params![session_id, updated_ms],
        )
        .map_err(|e| CoreError::Internal(format!("could not touch bot session: {e}")))?;
    Ok(changed > 0)
}

/// Delete a conversation and every message in it, atomically (Story 61.4).
///
/// Idempotent — deleting a missing session is not an error, so this is safe on
/// the rollback path of a half-finished create.
///
/// One `BEGIN IMMEDIATE` transaction, not two statements: a failure between
/// them would leave messages whose session is gone, and every one of those
/// rows would then be a message no surface can reach and no delete can find.
pub fn delete_session(data_dir: &Path, session_id: &str) -> Result<(), CoreError> {
    let conn = open(data_dir)?;
    conn.execute_batch("BEGIN IMMEDIATE")
        .map_err(|e| CoreError::Internal(format!("could not begin session delete: {e}")))?;
    let outcome = (|| -> rusqlite::Result<()> {
        conn.execute(
            "DELETE FROM bot_messages WHERE session_id = ?1",
            [session_id],
        )?;
        conn.execute("DELETE FROM bot_sessions WHERE id = ?1", [session_id])?;
        Ok(())
    })();
    match outcome {
        Ok(()) => {
            conn.execute_batch("COMMIT")
                .map_err(|e| CoreError::Internal(format!("could not commit delete: {e}")))?;
            Ok(())
        }
        Err(e) => {
            let _ = conn.execute_batch("ROLLBACK");
            Err(CoreError::Internal(format!(
                "could not delete bot session: {e}"
            )))
        }
    }
}

/// Append one message and return the `seq` it was given (Story 61.4).
///
/// The sequence is assigned here, inside one `BEGIN IMMEDIATE` transaction over
/// `MAX(seq)`, rather than by the caller: two appends racing on one
/// conversation — a user message and the assistant row the stream opens
/// immediately after — would otherwise both read the same maximum and store the
/// same position, and every later read would order them arbitrarily.
///
/// `message.seq` is ignored.
pub fn append_message(data_dir: &Path, message: &BotMessage) -> Result<i64, CoreError> {
    let conn = open(data_dir)?;
    conn.execute_batch("BEGIN IMMEDIATE")
        .map_err(|e| CoreError::Internal(format!("could not begin message append: {e}")))?;
    let outcome = (|| -> rusqlite::Result<i64> {
        let next: i64 = conn.query_row(
            "SELECT COALESCE(MAX(seq) + 1, 0) FROM bot_messages WHERE session_id = ?1",
            [&message.session_id],
            |row| row.get(0),
        )?;
        conn.execute(
            "INSERT INTO bot_messages(id, session_id, seq, role, content, model, provider_id, \
             prompt_tokens, completion_tokens, total_tokens, ttft_ms, duration_ms, \
             finish_reason, request_id, tool_call_count, partial, created_ms) \
             VALUES(?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17)",
            rusqlite::params![
                message.id,
                message.session_id,
                next,
                message.role,
                message.content,
                message.model,
                message.provider_id,
                message.prompt_tokens,
                message.completion_tokens,
                message.total_tokens,
                message.ttft_ms,
                message.duration_ms,
                message.finish_reason,
                message.request_id,
                message.tool_call_count,
                i64::from(message.partial),
                message.created_ms,
            ],
        )?;
        Ok(next)
    })();
    match outcome {
        Ok(seq) => {
            conn.execute_batch("COMMIT")
                .map_err(|e| CoreError::Internal(format!("could not commit append: {e}")))?;
            Ok(seq)
        }
        Err(e) => {
            let _ = conn.execute_batch("ROLLBACK");
            Err(CoreError::Internal(format!(
                "could not append bot message: {e}"
            )))
        }
    }
}

/// Rewrite a message's text, leaving every other column alone (Story 61.4).
///
/// The progressive write: called as deltas land, so the row on disk is never
/// behind the row on screen by more than one flush. It deliberately does **not**
/// touch `partial` — only [`close_message`] clears that — so a process killed
/// between two deltas leaves a row that says, truthfully, that it never
/// finished.
///
/// Returns whether a row matched.
pub fn set_message_content(
    data_dir: &Path,
    message_id: &str,
    content: &str,
) -> Result<bool, CoreError> {
    let conn = open(data_dir)?;
    let changed = conn
        .execute(
            "UPDATE bot_messages SET content = ?2 WHERE id = ?1",
            rusqlite::params![message_id, content],
        )
        .map_err(|e| CoreError::Internal(format!("could not write message content: {e}")))?;
    Ok(changed > 0)
}

/// Write a message's final text and every metadata column at once (Story 61.4,
/// FR-384). Returns whether a row matched.
///
/// One statement because they are one observation: a finish reason without its
/// duration is a verdict with no measurement, and this is the app that refuses
/// to print a number it did not measure. `partial` is a field of the argument
/// rather than a hard-coded `0` precisely because closing a **cancelled** or
/// **broken** stream is the same act — the record is final, and it is final and
/// incomplete.
pub fn close_message(data_dir: &Path, close: MessageClose<'_>) -> Result<bool, CoreError> {
    let conn = open(data_dir)?;
    let changed = conn
        .execute(
            "UPDATE bot_messages SET content = ?2, model = ?3, prompt_tokens = ?4, \
             completion_tokens = ?5, total_tokens = ?6, ttft_ms = ?7, duration_ms = ?8, \
             finish_reason = ?9, request_id = ?10, tool_call_count = ?11, partial = ?12 \
             WHERE id = ?1",
            rusqlite::params![
                close.id,
                close.content,
                close.model,
                close.prompt_tokens,
                close.completion_tokens,
                close.total_tokens,
                close.ttft_ms,
                close.duration_ms,
                close.finish_reason,
                close.request_id,
                close.tool_call_count,
                i64::from(close.partial),
            ],
        )
        .map_err(|e| CoreError::Internal(format!("could not close bot message: {e}")))?;
    Ok(changed > 0)
}

/// Every message of one conversation, in order (Story 61.4, FR-382).
pub fn list_messages(data_dir: &Path, session_id: &str) -> Result<Vec<BotMessage>, CoreError> {
    let conn = open(data_dir)?;
    let mut stmt = conn
        .prepare(&format!(
            "SELECT {MESSAGE_COLUMNS} FROM bot_messages WHERE session_id = ?1 ORDER BY seq"
        ))
        .map_err(|e| CoreError::Internal(format!("could not prepare message listing: {e}")))?;
    let rows = stmt
        .query_map([session_id], map_message_row)
        .map_err(|e| CoreError::Internal(format!("could not list bot messages: {e}")))?;
    let mut out = Vec::new();
    for row in rows {
        out.push(row.map_err(|e| CoreError::Internal(format!("could not read message: {e}")))?);
    }
    Ok(out)
}

/// Delete one message (Story 61.4). Idempotent.
///
/// Retry's other half: the pane replaces a failed assistant row rather than
/// appending a second one beside it, because two answers to one question is a
/// record nobody can read.
pub fn delete_message(data_dir: &Path, message_id: &str) -> Result<(), CoreError> {
    let conn = open(data_dir)?;
    conn.execute("DELETE FROM bot_messages WHERE id = ?1", [message_id])
        .map_err(|e| CoreError::Internal(format!("could not delete bot message: {e}")))?;
    Ok(())
}

// ---------------------------------------------------------------------------
// Story 61.6 — the list, the archive, and the title
// ---------------------------------------------------------------------------

/// The longest title [`mint_title`] will produce, counted in characters.
///
/// Sixty is what one line of the conversation list renders. It is a character
/// count and not a byte count because the clamp must never split a scalar.
pub const MAX_TITLE_CHARS: usize = 60;

/// What a conversation with nothing quotable in its first message is called.
pub const UNTITLED_SESSION: &str = "Untitled conversation";

/// How many rows one page of the list may carry, whatever the caller asks for.
///
/// A ceiling rather than a suggestion, for `recordings_fts`'s reason: a caller
/// that asks for everything is asking the webview to mount everything, and the
/// count a person actually needs is the `total` beside the page.
pub const MAX_SESSION_PAGE: usize = 200;

/// The page size a caller that names none gets.
pub const DEFAULT_SESSION_PAGE: usize = 50;

/// Mint a conversation's title from the person's own first message
/// (Story 61.6, FR-381).
///
/// **Locally, and never by a second model call** — the epic closed that door
/// (DW-211): a silent extra request to a paid endpoint is exactly the surprise
/// this app does not ship, and it would also mean a conversation had no name
/// until a network round trip finished.
///
/// Four rules, each of which a list row would otherwise carry as a defect:
///
/// 1. **One line.** The first non-blank line of the message and nothing after
///    it, so a pasted paragraph cannot put a newline into a row.
/// 2. **No emoji.** `DESIGN.md:358-361` allows emoji as content and bans it as
///    chrome, and a title in a list is chrome. The message keeps the user's
///    words verbatim — this strips them from the *name* only.
/// 3. **One space between words.** Runs of whitespace (and the gaps a stripped
///    emoji leaves) collapse, so a row cannot render a ragged gap.
/// 4. **Clamped on a character boundary**, with an ellipsis that is part of the
///    budget rather than added past it.
///
/// A message with nothing left after all four is [`UNTITLED_SESSION`] rather
/// than an empty string: an untitled conversation is a fact, and a zero-width
/// title is a row nobody can click.
pub fn mint_title(text: &str) -> String {
    let Some(line) = text.lines().find(|line| !line.trim().is_empty()) else {
        return UNTITLED_SESSION.to_owned();
    };
    let mut words = String::with_capacity(line.len());
    let mut pending_space = false;
    for ch in line.chars().filter(|ch| !is_pictographic(*ch)) {
        if ch.is_whitespace() {
            pending_space = !words.is_empty();
            continue;
        }
        if pending_space {
            words.push(' ');
            pending_space = false;
        }
        words.push(ch);
    }
    if words.is_empty() {
        return UNTITLED_SESSION.to_owned();
    }
    if words.chars().count() <= MAX_TITLE_CHARS {
        return words;
    }
    let kept: String = words.chars().take(MAX_TITLE_CHARS - 1).collect();
    format!("{}…", kept.trim_end())
}

/// Whether a scalar belongs to the emoji and pictograph blocks.
///
/// A closed list of ranges rather than a Unicode-property dependency, because
/// the property tables would be a new crate for a rule this app applies in one
/// place, and the ranges below do not move: the blocks are allocated. Included
/// are the two symbol planes, dingbats, the miscellaneous-symbol block,
/// variation selectors (the `U+FE0F` that turns a text glyph into an emoji
/// one), the zero-width joiner sequences are built from, the keycap enclosure
/// and the tag characters flag sequences use.
fn is_pictographic(ch: char) -> bool {
    matches!(ch as u32,
        0x2600..=0x27BF
            | 0x2B00..=0x2BFF
            | 0x20E3
            | 0xFE00..=0xFE0F
            | 0x200D
            | 0x1F000..=0x1FAFF
            | 0xE0020..=0xE007F)
}

/// Which conversations a list asks for (Story 61.6).
///
/// Three positions and not a boolean, matching the sessions board's own
/// `All | Active | Archived` chips (`sessions-pane.tsx:106-110`): a person who
/// archives things wants to look at the archive *alone* sometimes, and
/// [`list_sessions`]'s widening `include_archived` cannot express that.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SessionScope {
    /// Everything not archived — the list you open the pane onto.
    #[default]
    Live,
    /// Live and archived together.
    All,
    /// The archive alone.
    Archived,
}

impl SessionScope {
    /// The `WHERE` fragment this scope contributes, `""` when it constrains
    /// nothing.
    fn predicate(self) -> &'static str {
        match self {
            SessionScope::Live => "bot_sessions.archived = 0",
            SessionScope::All => "",
            SessionScope::Archived => "bot_sessions.archived = 1",
        }
    }
}

/// One page of the conversation list, as asked for (Story 61.6, FR-381).
#[derive(Debug, Clone, Default)]
pub struct SessionQuery {
    /// Free text, matched case-insensitively against the title **and** every
    /// message body. Empty is no text predicate at all — not a `LIKE '%%'`
    /// wearing that name.
    pub text: String,
    /// Which conversations to consider.
    pub scope: SessionScope,
    /// How many rows to return. `0` means [`DEFAULT_SESSION_PAGE`]; anything
    /// above [`MAX_SESSION_PAGE`] is clamped to it.
    pub limit: usize,
}

/// One row of the conversation list.
///
/// The session, plus the two facts a row renders that are not columns of it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessionListRow {
    /// The stored conversation.
    pub session: BotSession,
    /// When anything last happened to this conversation: the newest message's
    /// `created_ms` or the session's own `updated_ms`, whichever is later.
    ///
    /// **Never null and never zero.** A conversation with no messages has an
    /// activity — it was created, and it may have been renamed or archived
    /// since — so falling back to `updated_ms` is what stops a brand-new or
    /// emptied conversation sorting to the bottom of the list as if nothing had
    /// ever happened to it. Taking the *later* of the two is what stops a
    /// rename moving a row backwards past conversations older than it.
    pub latest_activity_ms: i64,
    /// How many messages it holds. A row with none is a conversation that was
    /// opened and never asked anything, which is a state worth reading.
    pub message_count: i64,
}

/// A page of rows and the size of the set it was cut from (Story 61.6).
///
/// `total` is counted by the same predicates with no `LIMIT`, so it is the real
/// number of matches and not the number of rows returned — the distinction
/// `count-label.ts:4-18` exists to keep unfoolable on the frontend.
#[derive(Debug, Clone, Default)]
pub struct SessionPage {
    /// The rows, newest activity first.
    pub rows: Vec<SessionListRow>,
    /// How many conversations matched, whether or not they fitted.
    pub total: i64,
}

/// The activity expression, written once because the list orders by it and the
/// row renders it.
const LATEST_ACTIVITY_SQL: &str =
    "MAX(bot_sessions.updated_ms, COALESCE((SELECT MAX(m.created_ms) \
     FROM bot_messages m WHERE m.session_id = bot_sessions.id), bot_sessions.updated_ms))";

/// How many messages a conversation holds.
const MESSAGE_COUNT_SQL: &str =
    "(SELECT COUNT(*) FROM bot_messages m WHERE m.session_id = bot_sessions.id)";

/// The free-text predicate: the title, or any message body (Story 61.6).
///
/// `LIKE` over the two columns, which is what AD-154 decided: FTS5 is AD-12's
/// answer if the archive ever needs it, and a trigram index over a table this
/// size would be a second copy of every answer the user has ever received.
/// Both sides are lowercased so the match is case-insensitive beyond the ASCII
/// range `LIKE` handles by itself, and the needle's own metacharacters are
/// escaped ([`escape_like`]) so a search for `100%` finds the text and not
/// every row.
const TEXT_PREDICATE_SQL: &str =
    "(LOWER(bot_sessions.title) LIKE '%' || LOWER(?) || '%' ESCAPE '\\' \
     OR EXISTS (SELECT 1 FROM bot_messages m WHERE m.session_id = bot_sessions.id \
     AND LOWER(m.content) LIKE '%' || LOWER(?) || '%' ESCAPE '\\'))";

/// One page of conversations, newest activity first (Story 61.6, FR-381).
///
/// Ordering is `latest_activity DESC, id DESC` — the activity because "what I
/// was last talking about" is the question a conversation list answers, and the
/// id to break a tie so two conversations touched inside one millisecond do not
/// swap places between two reads of the same data.
pub fn search_sessions(data_dir: &Path, query: &SessionQuery) -> Result<SessionPage, CoreError> {
    let conn = open(data_dir)?;
    let mut clauses: Vec<&str> = Vec::new();
    let mut params: Vec<String> = Vec::new();
    let scope = query.scope.predicate();
    if !scope.is_empty() {
        clauses.push(scope);
    }
    if !query.text.is_empty() {
        clauses.push(TEXT_PREDICATE_SQL);
        let needle = escape_like(&query.text);
        params.push(needle.clone());
        params.push(needle);
    }
    let where_sql = if clauses.is_empty() {
        String::new()
    } else {
        format!("WHERE {} ", clauses.join(" AND "))
    };
    let bound: Vec<&dyn rusqlite::ToSql> =
        params.iter().map(|p| p as &dyn rusqlite::ToSql).collect();

    // The total first, from the same predicates and no limit: a count taken
    // from the page would be the number of rows the reader can already see.
    let total: i64 = conn
        .query_row(
            &format!("SELECT COUNT(*) FROM bot_sessions {where_sql}"),
            bound.as_slice(),
            |r| r.get(0),
        )
        .map_err(|e| CoreError::Internal(format!("could not count bot sessions: {e}")))?;

    let limit = if query.limit == 0 {
        DEFAULT_SESSION_PAGE
    } else {
        query.limit.min(MAX_SESSION_PAGE)
    };
    let sql = format!(
        "SELECT {SESSION_COLUMNS}, {LATEST_ACTIVITY_SQL} AS latest_activity_ms, \
         {MESSAGE_COUNT_SQL} AS message_count FROM bot_sessions {where_sql}\
         ORDER BY latest_activity_ms DESC, bot_sessions.id DESC LIMIT {limit}"
    );
    let mut stmt = conn
        .prepare(&sql)
        .map_err(|e| CoreError::Internal(format!("could not prepare session search: {e}")))?;
    let read = stmt
        .query_map(bound.as_slice(), |r| {
            Ok(SessionListRow {
                session: map_session_row(r)?,
                latest_activity_ms: r.get(8)?,
                message_count: r.get(9)?,
            })
        })
        .map_err(|e| CoreError::Internal(format!("could not search bot sessions: {e}")))?;
    let mut rows = Vec::new();
    for row in read {
        rows.push(row.map_err(|e| CoreError::Internal(format!("could not read session: {e}")))?);
    }
    Ok(SessionPage { rows, total })
}

/// Escape SQL `LIKE` metacharacters (`\`, `%`, `_`) so a substring scan matches
/// them literally. Paired with `ESCAPE '\'` in [`TEXT_PREDICATE_SQL`].
///
/// `archive::fts` and `archive::recordings_fts` each carry these same nine
/// lines, and the note there applies here too: a dependency edge between three
/// unrelated search engines to share a rule that cannot change — SQL's `LIKE`
/// metacharacters are fixed — would cost more than the duplication, which all
/// three sides test.
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

/// Map a `SELECT SESSION_COLUMNS` row.
fn map_session_row(r: &rusqlite::Row<'_>) -> rusqlite::Result<BotSession> {
    Ok(BotSession {
        id: r.get(0)?,
        bot_id: r.get(1)?,
        provider_id: r.get(2)?,
        title: r.get(3)?,
        created_ms: r.get(4)?,
        updated_ms: r.get(5)?,
        archived: r.get::<_, i64>(6)? != 0,
        remote_session_id: r.get(7)?,
    })
}

/// Map a `SELECT MESSAGE_COLUMNS` row.
fn map_message_row(r: &rusqlite::Row<'_>) -> rusqlite::Result<BotMessage> {
    Ok(BotMessage {
        id: r.get(0)?,
        session_id: r.get(1)?,
        seq: r.get(2)?,
        role: r.get(3)?,
        content: r.get(4)?,
        model: r.get(5)?,
        provider_id: r.get(6)?,
        prompt_tokens: r.get(7)?,
        completion_tokens: r.get(8)?,
        total_tokens: r.get(9)?,
        ttft_ms: r.get(10)?,
        duration_ms: r.get(11)?,
        finish_reason: r.get(12)?,
        request_id: r.get(13)?,
        tool_call_count: r.get(14)?,
        partial: r.get::<_, i64>(15)? != 0,
        created_ms: r.get(16)?,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A scratch directory no other test can land in — the pid, a nanosecond
    /// stamp AND a process-wide counter, because two threads asking inside one
    /// clock tick otherwise open the same SQLite file. Carried verbatim from
    /// [`super::super::store`]'s test module rather than reaching for
    /// `tempfile`, which is not a dev-dependency of this crate.
    fn temp_dir() -> PathBuf {
        static COUNTER: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let n = COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let mut dir = std::env::temp_dir();
        dir.push(format!(
            "keeper-bot-session-test-{}-{}-{n}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(0)
        ));
        dir
    }

    fn session(id: &str, updated_ms: i64) -> BotSession {
        BotSession {
            id: id.to_owned(),
            bot_id: "bot-1".to_owned(),
            provider_id: "prov-1".to_owned(),
            title: "What is in my drive".to_owned(),
            created_ms: 1_700_000_000_000,
            updated_ms,
            archived: false,
            remote_session_id: None,
        }
    }

    fn message(id: &str, session_id: &str, role: &str, content: &str) -> BotMessage {
        BotMessage {
            id: id.to_owned(),
            session_id: session_id.to_owned(),
            seq: -1,
            role: role.to_owned(),
            content: content.to_owned(),
            model: None,
            provider_id: None,
            prompt_tokens: None,
            completion_tokens: None,
            total_tokens: None,
            ttft_ms: None,
            duration_ms: None,
            finish_reason: None,
            request_id: None,
            tool_call_count: 0,
            partial: false,
            created_ms: 1_700_000_000_000,
        }
    }

    /// The schema is created on first touch and creating it again is a no-op,
    /// so a cold install and an upgrade take the same path.
    #[test]
    fn a_bot_session_store_opens_twice_on_one_directory() {
        let dir = temp_dir();
        insert_session(&dir, &session("s1", 1)).expect("insert");
        insert_session(&dir, &session("s2", 2)).expect("insert again");
        assert_eq!(
            list_sessions(&dir, true).expect("list").len(),
            2,
            "both rows survive a second open"
        );
    }

    /// Newest activity first, with the id breaking a tie — the order a
    /// conversation list is read in.
    #[test]
    fn bot_sessions_list_newest_activity_first() {
        let dir = temp_dir();
        insert_session(&dir, &session("old", 100)).expect("insert");
        insert_session(&dir, &session("new", 300)).expect("insert");
        insert_session(&dir, &session("mid", 200)).expect("insert");
        let ids: Vec<String> = list_sessions(&dir, false)
            .expect("list")
            .into_iter()
            .map(|row| row.id)
            .collect();
        assert_eq!(ids, vec!["new", "mid", "old"]);
    }

    /// Archiving is reversible, and the default listing does not show an
    /// archived conversation while the widened one does.
    #[test]
    fn an_archived_bot_session_leaves_the_default_listing_and_comes_back() {
        let dir = temp_dir();
        insert_session(&dir, &session("s1", 1)).expect("insert");
        assert!(
            set_session_archived(&dir, "s1", true, 5).expect("archive"),
            "a real row matched"
        );
        assert!(list_sessions(&dir, false).expect("list").is_empty());
        assert_eq!(list_sessions(&dir, true).expect("list").len(), 1);
        set_session_archived(&dir, "s1", false, 6).expect("unarchive");
        assert_eq!(list_sessions(&dir, false).expect("list").len(), 1);
    }

    /// A write against an id that names nothing reports that, rather than
    /// succeeding silently (AD-27).
    #[test]
    fn writing_a_missing_bot_session_reports_no_match() {
        let dir = temp_dir();
        assert!(!set_session_title(&dir, "ghost", "x", 1).expect("rename"));
        assert!(!set_session_archived(&dir, "ghost", true, 1).expect("archive"));
        assert!(!touch_session(&dir, "ghost", 1).expect("touch"));
        assert!(!set_session_remote_id(&dir, "ghost", Some("r")).expect("remote"));
    }

    /// The sequence is the store's to assign, and it ascends from zero per
    /// conversation rather than globally.
    #[test]
    fn appending_to_a_bot_session_assigns_the_sequence() {
        let dir = temp_dir();
        insert_session(&dir, &session("s1", 1)).expect("insert");
        insert_session(&dir, &session("s2", 2)).expect("insert");
        assert_eq!(
            append_message(&dir, &message("m1", "s1", "user", "hello")).expect("append"),
            0
        );
        assert_eq!(
            append_message(&dir, &message("m2", "s1", "assistant", "")).expect("append"),
            1
        );
        assert_eq!(
            append_message(&dir, &message("m3", "s2", "user", "hi")).expect("append"),
            0,
            "each conversation numbers its own rows"
        );
        let rows = list_messages(&dir, "s1").expect("list");
        assert_eq!(
            rows.iter().map(|m| m.seq).collect::<Vec<_>>(),
            vec![0, 1],
            "read back in order"
        );
    }

    /// The crash contract: a row inserted partial and grown by the progressive
    /// write stays partial until something closes it.
    #[test]
    fn a_bot_session_stream_that_never_closes_leaves_a_partial_row() {
        let dir = temp_dir();
        insert_session(&dir, &session("s1", 1)).expect("insert");
        let mut row = message("m1", "s1", "assistant", "");
        row.partial = true;
        append_message(&dir, &row).expect("append");
        assert!(set_message_content(&dir, "m1", "Half a sen").expect("write"));
        let stored = list_messages(&dir, "s1").expect("list");
        assert_eq!(stored[0].content, "Half a sen");
        assert!(
            stored[0].partial,
            "the progressive write must not clear the flag"
        );
    }

    /// Closing writes every metadata column at once, and a cancelled stream is
    /// closed *and* still partial.
    #[test]
    fn closing_a_bot_session_message_writes_its_metadata() {
        let dir = temp_dir();
        insert_session(&dir, &session("s1", 1)).expect("insert");
        let mut row = message("m1", "s1", "assistant", "");
        row.partial = true;
        append_message(&dir, &row).expect("append");
        assert!(close_message(
            &dir,
            MessageClose {
                id: "m1",
                content: "Half a sentence",
                model: Some("llama4:8b"),
                prompt_tokens: Some(12),
                completion_tokens: None,
                total_tokens: None,
                ttft_ms: Some(240),
                duration_ms: Some(1_800),
                finish_reason: Some("cancelled"),
                request_id: Some("chatcmpl-7"),
                tool_call_count: 2,
                partial: true,
            },
        )
        .expect("close"));
        let stored = list_messages(&dir, "s1").expect("list");
        let stored = &stored[0];
        assert_eq!(stored.model.as_deref(), Some("llama4:8b"));
        assert_eq!(stored.prompt_tokens, Some(12));
        assert_eq!(
            stored.completion_tokens, None,
            "a number the endpoint did not report stays absent, never zero"
        );
        assert_eq!(stored.ttft_ms, Some(240));
        assert_eq!(stored.duration_ms, Some(1_800));
        assert_eq!(stored.finish_reason.as_deref(), Some("cancelled"));
        assert_eq!(stored.request_id.as_deref(), Some("chatcmpl-7"));
        assert_eq!(stored.tool_call_count, 2);
        assert!(stored.partial, "a stopped answer is final and incomplete");
    }

    /// An unknown role is stored and read back verbatim (NFR-43): a row a newer
    /// keeper wrote reaches the view as the spelling it has.
    #[test]
    fn an_unknown_bot_session_role_survives_the_round_trip() {
        let dir = temp_dir();
        insert_session(&dir, &session("s1", 1)).expect("insert");
        append_message(&dir, &message("m1", "s1", "developer", "x")).expect("append");
        let stored = list_messages(&dir, "s1").expect("list");
        assert_eq!(stored[0].role, "developer");
    }

    /// Deleting a conversation takes its messages with it, in one act, and
    /// deleting a missing one is not an error.
    #[test]
    fn deleting_a_bot_session_takes_its_messages() {
        let dir = temp_dir();
        insert_session(&dir, &session("s1", 1)).expect("insert");
        append_message(&dir, &message("m1", "s1", "user", "hello")).expect("append");
        append_message(&dir, &message("m2", "s1", "assistant", "hi")).expect("append");
        delete_session(&dir, "s1").expect("delete");
        assert!(get_session(&dir, "s1").expect("read").is_none());
        assert!(list_messages(&dir, "s1").expect("list").is_empty());
        delete_session(&dir, "s1").expect("deleting again is a no-op");
    }

    /// Retry replaces rather than appends: the failed row is removed, and the
    /// next append takes the position it vacated.
    #[test]
    fn deleting_one_bot_session_message_frees_its_position() {
        let dir = temp_dir();
        insert_session(&dir, &session("s1", 1)).expect("insert");
        append_message(&dir, &message("m1", "s1", "user", "hello")).expect("append");
        append_message(&dir, &message("m2", "s1", "assistant", "half")).expect("append");
        delete_message(&dir, "m2").expect("delete");
        assert_eq!(
            append_message(&dir, &message("m3", "s1", "assistant", "")).expect("append"),
            1,
            "the retried answer takes the failed one's place"
        );
        let rows = list_messages(&dir, "s1").expect("list");
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[1].id, "m3");
    }

    /// The remote id is a reference keeper can also forget.
    #[test]
    fn a_bot_session_remembers_and_forgets_the_remote_id() {
        let dir = temp_dir();
        insert_session(&dir, &session("s1", 1)).expect("insert");
        set_session_remote_id(&dir, "s1", Some("hermes-42")).expect("set");
        assert_eq!(
            get_session(&dir, "s1")
                .expect("read")
                .expect("row")
                .remote_session_id
                .as_deref(),
            Some("hermes-42")
        );
        set_session_remote_id(&dir, "s1", None).expect("clear");
        assert_eq!(
            get_session(&dir, "s1")
                .expect("read")
                .expect("row")
                .remote_session_id,
            None
        );
    }
}
