//! The notes driving adapter: every `notes_*` command, the four channel
//! subscriptions, and the `NotesError` → `IpcError` edge.
//!
//! No decisions live here. Every command validates its arguments against the
//! vault registry — which is the real containment, because Tauri's capability
//! ACL gates core and plugin commands and not the application's own (AD-60) —
//! and then hands the work to `notes_vault` (effects) or `keeper_core::notes`
//! (rules).
//!
//! Two shapes are worth knowing before reading the rest.
//!
//! **Bodies stream; lists project** (AD-58). A note body is never a command
//! return value: `notes_open` subscribes a `Channel<NoteBodyBatch>` that opens
//! with a full `Reset` and then pushes `External` / `Diverged` / `Renamed` /
//! `Gone` as the file changes underneath. The list carries view models only,
//! windowed to the visible range, with a `total` beside it so a scrollbar needs
//! no 10 000 rows.
//!
//! **The echo is suppressed by revision, not by muting a path.** Every body
//! subscription remembers the exact bytes it last delivered or wrote. A watcher
//! event whose disk revision equals that is keeper's own write coming back and is
//! dropped; anything else is genuinely external. That is strictly safer than
//! muting a path around a write, because a real external write landing inside the
//! muting window is not swallowed by it.
//!
//! `#[cfg(not(desktop))]` twins for the five desktop-only commands live at the
//! bottom, so the `invoke_handler` list is identical on every target and
//! `cargo check --target aarch64-apple-ios` stays green.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, LazyLock, Mutex, MutexGuard};
use std::time::Duration;

use keeper_core::notes::frontmatter::{FieldValue, Frontmatter};
use keeper_core::notes::index::{IndexEntry, IndexSnapshot};
use keeper_core::notes::vm::{
    NoteAttachmentVm, NoteBodyBatch, NoteChangeBatch, NoteConflictChoiceReq, NoteConflictVm,
    NoteCreateReq, NoteDiffVm, NoteFlag, NoteFolderVm, NoteIndexProgressVm, NoteLinkTargetVm,
    NoteListOp, NoteListVm, NoteQueryCheckVm, NoteQueryReq, NoteRefVm, NoteRevisionVm, NoteRowVm,
    NoteSearchBatch, NoteSearchHitVm, NoteSearchReq, NoteSpaceReq, NoteSpaceVm, NoteTagNodeVm,
    NoteTagTreeVm, NoteTemplateVm, NoteVaultSettingsReq, NoteVaultVm, NoteWriteVm,
};
use keeper_core::notes::{naming, query, search, templates, NotesError};
use keeper_core::vm::{IpcError, IpcErrorCode, TagVocabularyEntryVm, TagVocabularyVm};
use keeper_sync::profile::{NotesCadence, NotesConfig};
use tauri::ipc::Channel;
use tauri::{AppHandle, Emitter, Manager, State};

use crate::ipc::AppState;
use crate::notes_vault::{self, HeadRevision, Vault};

/// How often a changes subscription may speak, at most.
///
/// A 500-file agent run costs about four messages per second rather than five
/// hundred (AD-58). Together with the reconciler's 150 ms coalescing this is the
/// second half of the NFR-29 budget.
const CHANGE_BATCH_MS: u64 = 250;

/// What a note with no title and no first line is called, in the list and in its
/// filename. Deliberately a word rather than a date: a date is what the filename
/// prefix already says, and repeating it tells the reader nothing.
const UNTITLED: &str = "Untitled";

/// How long a by-id lookup will wait for the reconciler to absorb a note that
/// was written moments ago. Two coalescing windows plus slack: long enough that
/// open-after-create never loses the race, short enough that a genuinely unknown
/// id is still an immediate answer rather than a hang.
const ENTRY_SETTLE: std::time::Duration = std::time::Duration::from_millis(750);

/// How many rows a list window carries when a caller does not say.
const DEFAULT_LIMIT: u32 = 60;

/// The hard ceiling on a list window, so a caller cannot ask for 10 000 rows of
/// JSON and undo AD-58.
const MAX_LIMIT: u32 = 500;

/// How many hits a content search reports before it stops.
const MAX_SEARCH_HITS: usize = 200;

/// How many notes a wikilink autocomplete offers.
const MAX_LINK_TARGETS: usize = 30;

/// The vault-relative directories keeper creates lazily, on first use.
const SPACES_DIR: &str = "spaces";
const TEMPLATES_DIR: &str = "templates";

// ---------------------------------------------------------------------------
// Errors
// ---------------------------------------------------------------------------

/// Fold a notes-domain error into the one envelope the frontend understands.
///
/// The classification is the contract's: a malformed name, query, template or
/// frontmatter block is `InvalidInput` — the caller can fix it — and a missing
/// note or an unknown vault is `Internal`, because by the time a command runs the
/// id came from a view model keeper itself produced.
fn notes_error(error: NotesError) -> IpcError {
    let code = match &error {
        NotesError::Frontmatter { .. }
        | NotesError::Name(_)
        | NotesError::Query { .. }
        | NotesError::Template(_) => IpcErrorCode::NotesInvalid,
        NotesError::NotFound(_) | NotesError::VaultUnknown(_) => IpcErrorCode::Internal,
    };
    IpcError {
        code,
        message: error.to_string(),
        account_id: None,
        retriable: false,
    }
}

/// Resolve a vault id, or refuse.
fn vault_of(vault_id: &str) -> Result<Vault, IpcError> {
    notes_vault::vault(vault_id)
        .ok_or_else(|| notes_error(NotesError::VaultUnknown(vault_id.to_owned())))
}

/// Resolve a note id to its index entry inside a vault.
fn entry_of(vault_id: &str, note_id: &str) -> Result<IndexEntry, IpcError> {
    let snapshot = notes_vault::snapshot(vault_id)
        .ok_or_else(|| notes_error(NotesError::VaultUnknown(vault_id.to_owned())))?;
    snapshot
        .by_id(note_id)
        .cloned()
        .ok_or_else(|| notes_error(NotesError::NotFound(note_id.to_owned())))
}

/// The same lookup, for a caller that may be asking about a note written moments
/// ago (`notes_open` right after `notes_create`).
///
/// The reconciler is the index's only mutator and it coalesces, so a write and
/// the index knowing about it are separated by up to one coalescing window. A
/// caller that just created a note would otherwise be told its own note does not
/// exist — the race we watched happen on the agent-desktop run of 2026-08-03.
/// So a miss waits for the index to change rather than failing on the first
/// look, bounded so a genuinely unknown id still answers promptly.
async fn entry_of_soon(vault_id: &str, note_id: &str) -> Result<IndexEntry, IpcError> {
    if let Ok(entry) = entry_of(vault_id, note_id) {
        return Ok(entry);
    }
    let Some(mut index) = notes_vault::subscribe_index(vault_id) else {
        return entry_of(vault_id, note_id);
    };
    let deadline = tokio::time::Instant::now() + ENTRY_SETTLE;
    while tokio::time::Instant::now() < deadline {
        let remaining = deadline - tokio::time::Instant::now();
        if tokio::time::timeout(remaining, index.changed())
            .await
            .is_err()
        {
            break;
        }
        index.mark_unchanged();
        if let Ok(entry) = entry_of(vault_id, note_id) {
            return Ok(entry);
        }
    }
    entry_of(vault_id, note_id)
}

/// The engine, for the commands that write a profile.
fn engine_of(state: &AppState) -> Result<Arc<keeper_sync::engine::Engine>, IpcError> {
    crate::sync::engine(Arc::clone(&state.platform)).map_err(|error| IpcError {
        code: IpcErrorCode::Unsupported,
        message: error.to_string(),
        account_id: None,
        retriable: false,
    })
}

// ---------------------------------------------------------------------------
// Subscriptions
// ---------------------------------------------------------------------------

/// One open subscription. Dropping it aborts its task.
struct Subscription {
    task: tauri::async_runtime::JoinHandle<()>,
    /// Body subscriptions carry mutable state the save and buffer-report commands
    /// reach; the other three kinds carry none.
    body: Option<Arc<BodySub>>,
}

impl Drop for Subscription {
    fn drop(&mut self) {
        self.task.abort();
    }
}

/// A live editor over one note.
struct BodySub {
    vault_id: String,
    note_id: String,
    channel: Channel<NoteBodyBatch>,
    state: Mutex<BodyState>,
}

/// Split a note into its frontmatter block and its body.
///
/// The block is `source[..body_offset]` — fences, any byte-order mark and the
/// newline after the closing fence included — so the two halves concatenated are
/// the source again, byte for byte. An empty block means the note has none.
fn split_note(source: &str) -> (&str, &str) {
    let (_, body_offset) = Frontmatter::parse(source);
    source.split_at(body_offset)
}

/// Put a document back together from a block and a body.
fn join_note(frontmatter: &str, body: &str) -> String {
    let mut out = String::with_capacity(frontmatter.len() + body.len());
    out.push_str(frontmatter);
    out.push_str(body);
    out
}

/// What Rust holds for a live editor.
///
/// `base` is the exact bytes keeper last wrote or last delivered — the whole
/// document, block included — and is **never re-read from disk**, which is what
/// makes it a true common ancestor and what makes the clean/dirty distinction
/// meaningful (AD-58). `mine` is the editor's buffer, which is the **body alone**,
/// kept current by `notes_buffer_report`.
struct BodyState {
    rel: String,
    base: String,
    rev: String,
    mine: Option<String>,
}

impl BodyState {
    /// The block and the body of `base`.
    fn split(&self) -> (&str, &str) {
        split_note(&self.base)
    }

    /// Whether the editor has unsaved edits. Body against body: the block is not
    /// the editor's to change, so it can never be what makes a buffer dirty.
    fn is_dirty(&self) -> bool {
        self.mine
            .as_ref()
            .is_some_and(|mine| mine.as_str() != self.split().1)
    }
}

static SUBSCRIPTIONS: LazyLock<Mutex<HashMap<String, Subscription>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

/// The last list query per vault, so a changes subscription streams the window
/// the user is actually looking at.
///
/// `notes_subscribe_changes` takes no query — the contract's signature — so the
/// query has to come from somewhere, and the honest source is the last one
/// `notes_list` was asked for. A subscription opened before any list call streams
/// the default window until the first `notes_list` arrives, then follows it.
static LAST_QUERY: LazyLock<Mutex<HashMap<String, NoteQueryReq>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

/// Where the caret goes the first time a freshly created note is opened.
///
/// A template may declare a `{{cursor}}`, and the offset expansion reports is only
/// useful to the command that opens the note *next* — `notes_open`, milliseconds
/// later. Keyed by note id (a ULID, unique across vaults) and **taken** on open,
/// so the hint applies exactly once and the same note opened again tomorrow gets
/// the ordinary caret at the end. Offsets are into the BODY, which is what the
/// body channel carries.
static PENDING_CARET: LazyLock<Mutex<HashMap<String, u32>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

/// How many unopened caret hints are worth keeping. A note created and never
/// opened would otherwise hold its hint for the life of the process, so an agent
/// creating notes from a `{{cursor}}` template in bulk is what this bounds. A
/// dropped hint costs a caret position, never a byte.
const MAX_PENDING_CARETS: usize = 256;

/// Remember where a template asked for the caret in a note just created.
fn remember_caret(note_id: &str, body_offset: usize) {
    let Ok(at) = u32::try_from(body_offset) else {
        return;
    };
    let mut pending = PENDING_CARET
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    if pending.len() >= MAX_PENDING_CARETS {
        pending.clear();
    }
    pending.insert(note_id.to_owned(), at);
}

/// Take the caret a template asked for, if this note has one waiting.
fn take_caret(note_id: &str) -> Option<u32> {
    PENDING_CARET
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .remove(note_id)
}

fn subscriptions() -> MutexGuard<'static, HashMap<String, Subscription>> {
    SUBSCRIPTIONS
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

fn last_queries() -> MutexGuard<'static, HashMap<String, NoteQueryReq>> {
    LAST_QUERY
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// Register a subscription and return its id.
fn register(body: Option<Arc<BodySub>>, task: tauri::async_runtime::JoinHandle<()>) -> String {
    let id = crate::sync_ipc::new_ulid();
    subscriptions().insert(id.clone(), Subscription { task, body });
    id
}

/// Drop a subscription by id. Unknown ids are a no-op, so a double-unsubscribe
/// from a racing unmount is not an error.
fn unregister(subscription_id: &str) {
    subscriptions().remove(subscription_id);
}

fn body_sub(subscription_id: &str) -> Result<Arc<BodySub>, IpcError> {
    subscriptions()
        .get(subscription_id)
        .and_then(|sub| sub.body.clone())
        .ok_or_else(|| notes_error(NotesError::NotFound(subscription_id.to_owned())))
}

// ---------------------------------------------------------------------------
// Projection
// ---------------------------------------------------------------------------

/// Project one index entry into a row.
fn row_of(entry: &IndexEntry, head: Option<&HeadRevision>, unread: bool) -> NoteRowVm {
    NoteRowVm {
        id: entry.id.clone(),
        path: entry.path.clone(),
        title: entry.title.clone(),
        snippet: entry.snippet.clone(),
        tags: entry.tags.clone(),
        updated_ms: entry.updated_ms,
        pinned: has_flag(entry, "pinned"),
        archived: has_flag(entry, "archived"),
        unread,
        conflict: has_flag(entry, "conflict"),
        // A note with no commit yet is this device's, which is what the
        // `origin:` predicate table says absent means.
        origin: head.map_or("local", |head| head.origin.as_str()).to_owned(),
        // The revision the unread mark is cleared against. Filled from the same
        // commit lookup that produced `origin` and `unread`, so accepting from the
        // list cannot acknowledge a revision that moved in between; empty when the
        // note has no commit yet.
        head_rev: head.map(|head| head.rev.clone()).unwrap_or_default(),
    }
}

fn has_flag(entry: &IndexEntry, flag: &str) -> bool {
    entry.flags.iter().any(|existing| existing == flag)
}

/// Project a set of entries into rows, resolving unread state once per call.
fn rows_of(
    platform: &dyn keeper_core::platform::Platform,
    vault_id: &str,
    entries: &[&IndexEntry],
) -> Vec<NoteRowVm> {
    let heads = notes_vault::heads(vault_id).unwrap_or_default();
    entries
        .iter()
        .map(|entry| {
            let head = heads.get(&entry.path);
            let unread = notes_vault::is_unread(platform, head, &entry.id);
            row_of(entry, head, unread)
        })
        .collect()
}

/// The comparison a list is ordered by: pinned first, then most recently updated,
/// then by path so the order is total and a repaint never reshuffles equals.
fn list_order(a: &IndexEntry, b: &IndexEntry) -> std::cmp::Ordering {
    has_flag(b, "pinned")
        .cmp(&has_flag(a, "pinned"))
        .then(b.updated_ms.cmp(&a.updated_ms))
        .then(a.path.cmp(&b.path))
}

/// Whether an entry satisfies the plain (non-space) filter.
///
/// Index-only, deliberately: `text` matches the title, the snippet, the tags and
/// the path, which is what keeps NFR-28's 100 ms list paint true. Full-body
/// matching is `notes_search`, which streams because it reads files.
fn matches_filter(entry: &IndexEntry, req: &NoteQueryReq, head: Option<&HeadRevision>) -> bool {
    if let Some(text) = req.text.as_ref() {
        // Trimmed, so a needle of nothing but whitespace is not a filter. A user
        // resting on the space bar must not empty their own note list.
        let needle = fold(text.trim());
        if !needle.is_empty() {
            let hit = fold(&entry.title).contains(&needle)
                || fold(&entry.snippet).contains(&needle)
                || fold(&entry.path).contains(&needle)
                || entry.tags.iter().any(|tag| fold(tag).contains(&needle));
            if !hit {
                return false;
            }
        }
    }
    // Every chip must match: chips narrow, they do not widen.
    for tag in &req.tags {
        if !entry.tags.iter().any(|existing| tag_matches(existing, tag)) {
            return false;
        }
    }
    if let Some(origin) = req.origin.as_ref() {
        if !origin.is_empty() {
            let actual = head.map_or("local", |head| head.origin.as_str());
            if actual != origin {
                return false;
            }
        }
    }
    for flag in &req.flags {
        if !has_flag(entry, flag) {
            return false;
        }
    }
    // A conflict copy is hidden from the default lens and surfaced as a conflict
    // row instead (FR-116), so it appears only when asked for by name.
    if has_flag(entry, "conflict") && !req.flags.iter().any(|flag| flag == "conflict") {
        return false;
    }
    // So is an archived note (FR-119).
    if has_flag(entry, "archived") && !req.flags.iter().any(|flag| flag == "archived") {
        return false;
    }
    true
}

/// Segment-prefix tag matching: `project` matches `project` and
/// `project/keeper`, and never `projects`.
///
/// The chip is read through the one normalisation (Story 42.5) rather than
/// hand-stripped here, so a chip carrying a hash, a casing or a trailing space
/// selects the node the sidebar named. A chip that is not a tag at all
/// normalises to nothing and matches nothing — an empty chip must not silently
/// select the whole vault.
fn tag_matches(actual: &str, wanted: &str) -> bool {
    let Some(wanted) = keeper_core::notes::tags::normalise(wanted) else {
        return false;
    };
    actual == wanted
        || (actual.len() > wanted.len()
            && actual.starts_with(&wanted)
            && actual.as_bytes().get(wanted.len()) == Some(&b'/'))
}

/// Case-fold for matching. Diacritic folding lives in `search::find`, which is
/// the surface that promises it; this is the cheap index-only comparison.
fn fold(value: &str) -> String {
    value.to_lowercase()
}

/// Build the windowed list for a query.
fn project_list(
    platform: &dyn keeper_core::platform::Platform,
    vault: &Vault,
    req: &NoteQueryReq,
) -> Result<NoteListVm, IpcError> {
    let snapshot = notes_vault::snapshot(&vault.id)
        .ok_or_else(|| notes_error(NotesError::VaultUnknown(vault.id.clone())))?;
    let heads = notes_vault::heads(&vault.id).unwrap_or_default();

    // A space is the query DSL; everything else is the chip filter.
    let space = match req.space_id.as_ref() {
        Some(space_id) if !space_id.is_empty() => Some(space_query(vault, &snapshot, space_id)?),
        _ => None,
    };
    let mut matched: Vec<&IndexEntry> = match space {
        Some(mut parsed) => {
            // Bound to this snapshot so `backlink:` and title-resolved `link:`
            // can be answered at all; the binding is per snapshot revision.
            query::bind_index(&mut parsed, &snapshot);
            let now_ms = notes_vault::local_now_ms();
            snapshot
                .entries()
                .iter()
                .filter(|entry| {
                    let mut body = body_reader(vault, &entry.path);
                    query::eval(&parsed, entry, &mut body, now_ms)
                })
                .collect()
        }
        None => snapshot
            .entries()
            .iter()
            .filter(|entry| matches_filter(entry, req, heads.get(&entry.path)))
            .collect(),
    };
    matched.sort_by(|a, b| list_order(a, b));

    let total = u32::try_from(matched.len()).unwrap_or(u32::MAX);
    let offset = req.offset.min(total);
    let limit = if req.limit == 0 {
        DEFAULT_LIMIT
    } else {
        req.limit.min(MAX_LIMIT)
    };
    let window: Vec<&IndexEntry> = matched
        .into_iter()
        .skip(offset as usize)
        .take(limit as usize)
        .collect();
    Ok(NoteListVm {
        rows: rows_of(platform, &vault.id, &window),
        total,
        offset,
    })
}

/// A lazy body provider for the one predicate that needs bytes.
///
/// `query::eval` calls this only when the query actually contains a `text:` term,
/// and the AST is reordered so index-only predicates run first — so by the time a
/// body is read the candidate set is usually two orders of magnitude smaller.
/// Memoised, so a query with two `text:` terms still reads the file once.
fn body_reader<'a>(vault: &'a Vault, rel: &'a str) -> impl FnMut() -> String + 'a {
    let mut cached: Option<String> = None;
    move || {
        cached
            .get_or_insert_with(|| notes_vault::read_note(vault, rel).unwrap_or_default())
            .clone()
    }
}

// ---------------------------------------------------------------------------
// Spaces
// ---------------------------------------------------------------------------

/// A space's definition, as read from its note.
struct SpaceDef {
    id: String,
    name: String,
    query: String,
    sort: String,
    limit: u32,
}

/// Read the space definition out of a space note's frontmatter.
///
/// The definition lives under the reserved `keeper:` namespace, which the
/// frontmatter parser accepts to **one** level of nesting — so it is
/// `keeper: { space: "<query>", sort: …, limit: … }`, not the deeper
/// `keeper.space.query` the architecture companion's example shows. A nested map
/// is still accepted where the parser hands one back, so the deeper form starts
/// working the day the subset grows, and neither spelling is a parse error.
fn space_def(entry: &IndexEntry, source: &str) -> SpaceDef {
    let (fm, _) = Frontmatter::parse(source);
    let mut def = SpaceDef {
        id: entry.id.clone(),
        name: entry.title.clone(),
        query: String::new(),
        sort: "modified desc".to_owned(),
        limit: MAX_LIMIT,
    };
    let Some(FieldValue::Map(pairs)) = fm.get("keeper") else {
        return def;
    };
    for (key, value) in pairs {
        match (key.as_str(), value) {
            ("space", FieldValue::Str(query)) => def.query = query.clone(),
            ("space", FieldValue::Map(inner)) => {
                for (key, value) in inner {
                    match (key.as_str(), value) {
                        ("query", FieldValue::Str(query)) => def.query = query.clone(),
                        ("sort", FieldValue::Str(sort)) => def.sort = sort.clone(),
                        ("limit", FieldValue::Num(limit)) => def.limit = clamp_limit(*limit),
                        _ => {}
                    }
                }
            }
            ("sort", FieldValue::Str(sort)) => def.sort = sort.clone(),
            ("limit", FieldValue::Num(limit)) => def.limit = clamp_limit(*limit),
            _ => {}
        }
    }
    def
}

/// A limit from frontmatter, clamped into the window the list will honour.
fn clamp_limit(raw: f64) -> u32 {
    if raw <= 0.0 {
        return MAX_LIMIT;
    }
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    let limit = raw.min(f64::from(MAX_LIMIT)) as u32;
    limit
}

/// Parse the query of one space, by note id.
fn space_query(
    vault: &Vault,
    snapshot: &IndexSnapshot,
    space_id: &str,
) -> Result<query::Query, IpcError> {
    let entry = snapshot
        .by_id(space_id)
        .ok_or_else(|| notes_error(NotesError::NotFound(space_id.to_owned())))?;
    let source = notes_vault::read_note(vault, &entry.path).map_err(notes_error)?;
    let def = space_def(entry, &source);
    query::parse(&def.query).map_err(|error| {
        notes_error(NotesError::Query {
            message: error.message,
            token_index: error.token_index,
        })
    })
}

// ---------------------------------------------------------------------------
// Vault commands
// ---------------------------------------------------------------------------

/// Every notes-flagged profile, with its index state and unread count.
#[tauri::command]
pub async fn notes_vaults(state: State<'_, AppState>) -> Result<Vec<NoteVaultVm>, IpcError> {
    let platform = Arc::clone(&state.platform);
    Ok(notes_vault::vaults()
        .iter()
        .map(|vault| {
            let unread = notes_vault::unread_count(platform.as_ref(), &vault.id);
            notes_vault::vault_vm(vault, unread)
        })
        .collect())
}

/// Flag or unflag a synced folder as a vault (FR-94).
///
/// `config: None` unflags, and **removes no files** — a vault is a flag on a
/// profile, so unflagging is a flag write and nothing else (AD-54).
///
/// The config arrives as `NoteVaultSettingsReq` rather than `keeper_sync`'s own
/// `NotesConfig`: the two are field-identical, and `NotesConfig` lives in a crate
/// with no `ts_rs` and no business acquiring one, so the request type the frontend
/// already has generated bindings for is what crosses the wire.
#[tauri::command]
pub async fn notes_vault_flag(
    app: AppHandle,
    state: State<'_, AppState>,
    profile_id: String,
    config: Option<NoteVaultSettingsReq>,
) -> Result<NoteVaultVm, IpcError> {
    let engine = engine_of(&state)?;
    let mut profile = engine
        .list_profiles()
        .map_err(|error| crate::sync_ipc::sync_ipc_error(&error))?
        .into_iter()
        .find(|profile| profile.id == profile_id)
        .ok_or_else(|| notes_error(NotesError::VaultUnknown(profile_id.clone())))?;

    profile.notes = config.map(|req| apply_settings(NotesConfig::default(), &req));
    // Also arm the exclusion FR-121 needs, once, at flag time: `.keeper/` is
    // keeper's cache and must never be committed. `BUILTIN_EXCLUDES` already
    // covers it, so this is belt and braces for a profile whose excludes a user
    // has narrowed.
    if profile.notes.is_some() && !profile.excludes.iter().any(|rule| rule.contains(".keeper")) {
        profile.excludes.push("**/.keeper/**".to_owned());
    }
    engine
        .upsert_profile(&profile)
        .map_err(|error| crate::sync_ipc::sync_ipc_error(&error))?;
    notes_vault::refresh(&app);

    // An unflagged profile has no vault to describe, and a vault whose folder is
    // not mounted right now is not registered either. Both answer with what is
    // true — a view model that says "no longer a vault" — rather than an error the
    // form would have to special-case. `indexed: false` and a zero count are the
    // honest values for a folder keeper is not indexing.
    let Some(vault) = notes_vault::vault(&profile_id) else {
        return Ok(NoteVaultVm {
            id: profile.id.clone(),
            profile_id: profile.id,
            name: profile.name,
            subfolder: String::new(),
            root: profile.local_path.to_string_lossy().into_owned(),
            indexed: false,
            note_count: 0,
            unread_count: 0,
            cadence: notes_vault::cadence_vm(&NotesCadence::default()),
        });
    };
    let unread = notes_vault::unread_count(state.platform.as_ref(), &vault.id);
    Ok(notes_vault::vault_vm(&vault, unread))
}

/// Save the per-vault settings (FR-120).
#[tauri::command]
pub async fn notes_vault_settings_save(
    app: AppHandle,
    state: State<'_, AppState>,
    vault_id: String,
    settings: NoteVaultSettingsReq,
) -> Result<NoteVaultVm, IpcError> {
    let engine = engine_of(&state)?;
    let mut profile = engine
        .list_profiles()
        .map_err(|error| crate::sync_ipc::sync_ipc_error(&error))?
        .into_iter()
        .find(|profile| profile.id == vault_id)
        .ok_or_else(|| notes_error(NotesError::VaultUnknown(vault_id.clone())))?;
    let current = profile.notes.clone().unwrap_or_default();
    profile.notes = Some(apply_settings(current, &settings));
    engine
        .upsert_profile(&profile)
        .map_err(|error| crate::sync_ipc::sync_ipc_error(&error))?;
    notes_vault::refresh(&app);
    let vault = vault_of(&vault_id)?;
    let unread = notes_vault::unread_count(state.platform.as_ref(), &vault.id);
    Ok(notes_vault::vault_vm(&vault, unread))
}

/// Apply a settings request onto an existing config.
///
/// Every field is optional and `None` means **the caller did not express this**,
/// never "reset it" (AD-34-9): a form that does not show a knob must not be able
/// to move it.
///
/// The cadence is clamped here rather than in the engine, because
/// `SyncProfile::validate` takes `&self` and therefore *refuses* an out-of-range
/// value instead of correcting it — so a form that sent 100 ms would fail the save
/// rather than get the floor. Clamping first means the form shows the value
/// actually in force (AD-34-8) and the save always lands.
fn apply_settings(mut config: NotesConfig, req: &NoteVaultSettingsReq) -> NotesConfig {
    if let Some(subfolder) = req.subfolder.as_ref() {
        let trimmed = subfolder.trim().trim_matches('/');
        if !trimmed.is_empty() {
            config.subfolder = trimmed.to_owned();
        }
    }
    if let Some(template) = req.journal_template.as_ref() {
        let trimmed = template.trim();
        if !trimmed.is_empty() {
            config.journal_template = trimmed.to_owned();
        }
    }
    if let Some(template) = req.default_template.as_ref() {
        let trimmed = template.trim();
        config.default_template = (!trimmed.is_empty()).then(|| trimmed.to_owned());
    }
    if let Some(cadence) = req.cadence.as_ref() {
        config.cadence = NotesCadence {
            commit_idle_ms: cadence.commit_idle_ms.max(MIN_COMMIT_IDLE_MS),
            push_interval_ms: cadence.push_interval_ms.max(MIN_PUSH_INTERVAL_MS),
            push_on_blur: cadence.push_on_blur,
        };
    }
    config
}

/// The cadence floors `SyncProfile::validate` enforces, mirrored here so a save
/// is clamped rather than refused.
const MIN_COMMIT_IDLE_MS: u64 = 500;
const MIN_PUSH_INTERVAL_MS: u64 = 5_000;

/// The active vault, or `None` when there is nothing to be active.
///
/// Rust owns this selection because the tray and the capture window are both
/// vault-scoped and neither is the main webview. Not in the contract's command
/// table, which its own registry section (`notes.active_vault`) needs something to
/// read and write — see the report.
#[tauri::command]
pub async fn notes_vault_active(state: State<'_, AppState>) -> Result<Option<String>, IpcError> {
    Ok(notes_vault::active_vault(state.platform.as_ref()))
}

/// Select the active vault (FR-95). A filter change, not a reload.
#[tauri::command]
pub async fn notes_vault_set_active(
    state: State<'_, AppState>,
    vault_id: String,
) -> Result<(), IpcError> {
    notes_vault::set_active_vault(state.platform.as_ref(), &vault_id).map_err(notes_error)
}

/// Drop the index cache and cold-scan; progress arrives on the index channel.
#[tauri::command]
pub async fn notes_index_rebuild(vault_id: String) -> Result<(), IpcError> {
    notes_vault::rebuild(&vault_id).map_err(notes_error)
}

// ---------------------------------------------------------------------------
// Reading the vault
// ---------------------------------------------------------------------------

/// The windowed note list for a query (FR-103).
#[tauri::command]
pub async fn notes_list(
    state: State<'_, AppState>,
    vault_id: String,
    query: NoteQueryReq,
) -> Result<NoteListVm, IpcError> {
    let vault = vault_of(&vault_id)?;
    let list = project_list(state.platform.as_ref(), &vault, &query)?;
    // Remembered so an open changes subscription streams this window rather than
    // a default nobody is looking at.
    last_queries().insert(vault_id, query);
    Ok(list)
}

/// The hierarchical tag tree with per-node counts (FR-104), over both producers
/// of a tag (Story 42.5): notes and recording sessions.
#[tauri::command]
pub async fn notes_tag_tree(vault_id: String) -> Result<NoteTagTreeVm, IpcError> {
    let snapshot = notes_vault::snapshot(&vault_id)
        .ok_or_else(|| notes_error(NotesError::VaultUnknown(vault_id)))?;
    Ok(NoteTagTreeVm {
        nodes: snapshot.tag_tree().iter().map(tag_node_vm).collect(),
    })
}

/// Project one tag-tree node, recursively.
fn tag_node_vm(node: &keeper_core::notes::tags::TagNode) -> NoteTagNodeVm {
    NoteTagNodeVm {
        name: node.name.clone(),
        path: node.path.clone(),
        count: node.count,
        children: node.children.iter().map(tag_node_vm).collect(),
    }
}

/// The flat tag vocabulary, for a completion surface that cannot consume a tree
/// (Story 42.5, FR-143).
///
/// `vault_id` is optional because the surface that most needs this — the
/// recording metadata card — is not inside a vault: a recording belongs to the
/// app, not to one set of notes. Absent, it resolves the active vault, which is
/// the same vault whose sidebar the person is looking at. The vocabulary itself
/// is not vault-scoped in spirit (a recording's tags reach every vault's index),
/// but it is served from a snapshot, and a snapshot belongs to a vault.
///
/// An unknown or absent vault resolves with an empty vocabulary rather than an
/// error. Completion is an affordance: offering nothing is a usable outcome, and
/// rejecting an `invoke` would make the card's tag field print an error for the
/// crime of having no vault behind it.
#[tauri::command]
pub async fn tags_vocabulary(
    state: State<'_, AppState>,
    vault_id: Option<String>,
) -> Result<TagVocabularyVm, IpcError> {
    let resolved = match vault_id {
        Some(id) => Some(id),
        None => notes_vault::active_vault(state.platform.as_ref()),
    };
    let entries: Vec<TagVocabularyEntryVm> = resolved
        .and_then(|id| notes_vault::snapshot(&id))
        .map(|snapshot| {
            snapshot
                .tag_vocabulary()
                .map(|(path, count)| TagVocabularyEntryVm {
                    path: path.to_owned(),
                    count,
                })
                .collect()
        })
        .unwrap_or_default();
    Ok(TagVocabularyVm { entries })
}

/// One level of the physical folder lens (FR-106).
#[tauri::command]
pub async fn notes_tree(
    state: State<'_, AppState>,
    vault_id: String,
    rel_dir: String,
) -> Result<NoteFolderVm, IpcError> {
    let snapshot = notes_vault::snapshot(&vault_id)
        .ok_or_else(|| notes_error(NotesError::VaultUnknown(vault_id.clone())))?;
    let prefix = if rel_dir.is_empty() {
        String::new()
    } else {
        format!("{}/", rel_dir.trim_end_matches('/'))
    };
    let mut dirs: Vec<String> = Vec::new();
    let mut notes: Vec<&IndexEntry> = Vec::new();
    for entry in snapshot.entries() {
        let Some(rest) = entry.path.strip_prefix(&prefix) else {
            continue;
        };
        match rest.split_once('/') {
            // A deeper path contributes its next directory component, once.
            Some((dir, _)) => {
                let dir = dir.to_owned();
                if !dirs.contains(&dir) {
                    dirs.push(dir);
                }
            }
            None => notes.push(entry),
        }
    }
    dirs.sort();
    notes.sort_by(|a, b| list_order(a, b));
    Ok(NoteFolderVm {
        rel_dir,
        dirs,
        notes: rows_of(state.platform.as_ref(), &vault_id, &notes),
    })
}

/// Every space in the vault, each with its parse status (FR-105).
#[tauri::command]
pub async fn notes_spaces(vault_id: String) -> Result<Vec<NoteSpaceVm>, IpcError> {
    let vault = vault_of(&vault_id)?;
    let snapshot = notes_vault::snapshot(&vault_id)
        .ok_or_else(|| notes_error(NotesError::VaultUnknown(vault_id)))?;
    let mut spaces: Vec<NoteSpaceVm> = snapshot
        .entries()
        .iter()
        .filter(|entry| has_flag(entry, "space"))
        .map(|entry| {
            let source = notes_vault::read_note(&vault, &entry.path).unwrap_or_default();
            let def = space_def(entry, &source);
            // A broken query is a warning chip on the row, never a failed
            // command: a space is a file a person or an agent hand-edits.
            let error = query::parse(&def.query).err().map(|error| error.message);
            NoteSpaceVm {
                id: def.id,
                name: def.name,
                query: def.query,
                sort: def.sort,
                limit: def.limit,
                error,
            }
        })
        .collect();
    spaces.sort_by(|a, b| a.name.cmp(&b.name));
    Ok(spaces)
}

/// Create or update a space note (FR-105).
#[tauri::command]
pub async fn notes_space_save(
    vault_id: String,
    space: NoteSpaceReq,
) -> Result<NoteRefVm, IpcError> {
    let vault = vault_of(&vault_id)?;
    // Refuse a broken query at the edge: a space is a surface people run bulk
    // actions from, so storing one that matches nothing silently is worse than
    // saying no.
    query::parse(&space.query).map_err(|error| {
        notes_error(NotesError::Query {
            message: error.message,
            token_index: error.token_index,
        })
    })?;
    let definition = FieldValue::Map(vec![
        ("space".to_owned(), FieldValue::Str(space.query.clone())),
        ("sort".to_owned(), FieldValue::Str(space.sort.clone())),
        (
            "limit".to_owned(),
            FieldValue::Num(f64::from(space.limit.min(MAX_LIMIT))),
        ),
    ]);

    // An existing space keeps every other byte of its note (FR-121): only the
    // definition key is spliced.
    if let Some(id) = space.id.as_ref().filter(|id| !id.is_empty()) {
        let entry = entry_of(&vault_id, id)?;
        let source = notes_vault::read_note(&vault, &entry.path).map_err(notes_error)?;
        let updated = Frontmatter::set_in(&source, "keeper", definition);
        notes_vault::write_note(&vault, &entry.path, &updated).map_err(notes_error)?;
        return Ok(NoteRefVm {
            vault_id,
            id: entry.id,
            path: entry.path,
            title: space.name,
        });
    }

    let id = crate::sync_ipc::new_ulid();
    let filename = naming::note_filename(
        &space.name,
        &today(),
        &notes_vault::siblings(&vault, SPACES_DIR),
    );
    let rel = format!("{SPACES_DIR}/{filename}");
    let front = Frontmatter::serialise_new(&[
        ("id".to_owned(), FieldValue::Str(id.clone())),
        ("created".to_owned(), FieldValue::Str(now_local())),
        ("updated".to_owned(), FieldValue::Str(now_local())),
        ("keeper".to_owned(), definition),
    ]);
    let body = format!("# {}\n", space.name);
    notes_vault::write_note(&vault, &rel, &format!("{front}\n{body}")).map_err(notes_error)?;
    Ok(NoteRefVm {
        vault_id,
        id,
        path: rel,
        title: space.name,
    })
}

/// Parse-only, for the live underline while a query is typed.
#[tauri::command]
pub async fn notes_space_validate(query: String) -> Result<NoteQueryCheckVm, IpcError> {
    Ok(match keeper_core::notes::query::parse(&query) {
        Ok(_) => NoteQueryCheckVm {
            ok: true,
            message: None,
            token_index: None,
            span: None,
        },
        Err(error) => NoteQueryCheckVm {
            ok: false,
            message: Some(error.message),
            token_index: Some(u32::try_from(error.token_index).unwrap_or(u32::MAX)),
            span: Some((
                u32::try_from(error.byte_span.0).unwrap_or(u32::MAX),
                u32::try_from(error.byte_span.1).unwrap_or(u32::MAX),
            )),
        },
    })
}

/// Every template in the vault (FR-100).
#[tauri::command]
pub async fn notes_templates(vault_id: String) -> Result<Vec<NoteTemplateVm>, IpcError> {
    let snapshot = notes_vault::snapshot(&vault_id)
        .ok_or_else(|| notes_error(NotesError::VaultUnknown(vault_id)))?;
    let mut templates: Vec<NoteTemplateVm> = snapshot
        .entries()
        .iter()
        .filter(|entry| has_flag(entry, "template"))
        .map(|entry| NoteTemplateVm {
            name: entry.title.clone(),
            path: entry.path.clone(),
        })
        .collect();
    templates.sort_by(|a, b| a.name.cmp(&b.name));
    Ok(templates)
}

/// Wikilink autocomplete (FR-108).
#[tauri::command]
pub async fn notes_link_targets(
    vault_id: String,
    prefix: String,
) -> Result<Vec<NoteLinkTargetVm>, IpcError> {
    let snapshot = notes_vault::snapshot(&vault_id)
        .ok_or_else(|| notes_error(NotesError::VaultUnknown(vault_id)))?;
    let needle = fold(&prefix);
    let mut hits: Vec<NoteLinkTargetVm> = snapshot
        .entries()
        .iter()
        .filter(|entry| {
            needle.is_empty()
                || fold(&entry.title).contains(&needle)
                || fold(&entry.path).contains(&needle)
        })
        .take(MAX_LINK_TARGETS)
        .map(|entry| NoteLinkTargetVm {
            id: entry.id.clone(),
            title: entry.title.clone(),
            path: entry.path.clone(),
        })
        .collect();
    hits.sort_by(|a, b| a.title.cmp(&b.title));
    Ok(hits)
}

/// Every note that links to this one (FR-108).
#[tauri::command]
pub async fn notes_backlinks(
    state: State<'_, AppState>,
    vault_id: String,
    note_id: String,
) -> Result<Vec<NoteRowVm>, IpcError> {
    let snapshot = notes_vault::snapshot(&vault_id)
        .ok_or_else(|| notes_error(NotesError::VaultUnknown(vault_id.clone())))?;
    let mut inbound = snapshot.backlinks(&note_id);
    inbound.sort_by(|a, b| list_order(a, b));
    Ok(rows_of(state.platform.as_ref(), &vault_id, &inbound))
}

// ---------------------------------------------------------------------------
// Writing notes
// ---------------------------------------------------------------------------

/// Today's date, as the filename rule wants it.
fn today() -> String {
    chrono::Local::now().format("%Y-%m-%d").to_string()
}

/// Now, as an ISO-8601 string with offset — Obsidian's date-time property
/// format, so Obsidian shows `created` and `updated` as dates.
fn now_local() -> String {
    chrono::Local::now().to_rfc3339()
}

/// Create a note (FR-98). No dialog anywhere in the path (UX-DR35).
#[tauri::command]
pub async fn notes_create(vault_id: String, req: NoteCreateReq) -> Result<NoteRefVm, IpcError> {
    let vault = vault_of(&vault_id)?;
    create_note(&vault, &req, false)
}

/// The shared create path, used by `notes_create` and by capture.
fn create_note(vault: &Vault, req: &NoteCreateReq, capture: bool) -> Result<NoteRefVm, IpcError> {
    let id = crate::sync_ipc::new_ulid();
    let body_source = req.body.clone().unwrap_or_default();
    // A title the caller gave, else the first line of the body — which is the
    // whole of "no questions at capture time".
    let title = req
        .title
        .clone()
        .filter(|title| !title.trim().is_empty())
        .unwrap_or_else(|| naming::title_from_body(&body_source));
    // A note with neither a title nor a first line is `Untitled`, not today's
    // date: `note_filename` already prefixes the date, so a date-shaped title
    // produced `2026-08-03-2026-08-03.md` — a name that says the same thing twice
    // and says nothing about the note. The first line the user types retitles it.
    let title = if title.trim().is_empty() {
        UNTITLED.to_owned()
    } else {
        title
    };

    // A template is expanded through the pure core, caret offset included. The
    // offset is into the expanded body, which is exactly the space the body channel
    // speaks in — one `\n` away, because the document keeper writes puts a blank
    // line between the block and the body.
    let (body, caret) = match template_source(vault, req) {
        Some(template) => {
            let (expanded, caret) = templates::expand(
                &template,
                &templates::TemplateCtx {
                    title: title.clone(),
                    id: id.clone(),
                    now_local: now_local(),
                },
            );
            if body_source.trim().is_empty() {
                (expanded, caret)
            } else {
                (format!("{expanded}\n{body_source}"), caret)
            }
        }
        None => (body_source, None),
    };

    let dest = req
        .dest
        .clone()
        .unwrap_or_default()
        .trim_matches('/')
        .to_owned();
    let filename = naming::note_filename(&title, &today(), &notes_vault::siblings(vault, &dest));
    let rel = if dest.is_empty() {
        filename
    } else {
        format!("{dest}/{filename}")
    };

    let mut pairs = vec![
        ("id".to_owned(), FieldValue::Str(id.clone())),
        ("created".to_owned(), FieldValue::Str(now_local())),
        ("updated".to_owned(), FieldValue::Str(now_local())),
    ];
    if !req.tags.is_empty() {
        pairs.push((
            "tags".to_owned(),
            FieldValue::List(
                req.tags
                    .iter()
                    .map(|tag| FieldValue::Str(tag.clone()))
                    .collect(),
            ),
        ));
    }
    if capture {
        // The reserved namespace's one documented sub-key, so the inbox lens can
        // find unfiled thoughts.
        pairs.push((
            "keeper".to_owned(),
            FieldValue::Map(vec![("capture".to_owned(), FieldValue::Bool(true))]),
        ));
    }
    let front = Frontmatter::serialise_new(&pairs);
    notes_vault::write_note(vault, &rel, &format!("{front}\n{body}")).map_err(notes_error)?;
    // Only now that the note exists: the caret hint is for whoever opens it, and a
    // hint for a note that was never written would sit there forever. `+ 1` is the
    // blank line between the block and the body, which is the body's first byte.
    if let Some(at) = caret {
        remember_caret(&id, at + 1);
    }
    Ok(NoteRefVm {
        vault_id: vault.id.clone(),
        id,
        path: rel,
        title,
    })
}

/// The template text a create should expand, if any.
///
/// An explicitly named template wins; otherwise the vault's configured default.
/// A template path that names nothing is **not** a failure: the note is created
/// plain, because losing a thought over a missing scaffold is the wrong trade.
fn template_source(vault: &Vault, req: &NoteCreateReq) -> Option<String> {
    let named = req
        .template
        .clone()
        .filter(|path| !path.trim().is_empty())
        .or_else(|| vault.config.default_template.clone())?;
    let rel = if named.contains('/') {
        named
    } else {
        format!("{TEMPLATES_DIR}/{named}")
    };
    match notes_vault::read_note(vault, &rel) {
        Ok(source) => Some(source),
        Err(error) => {
            tracing::info!(%error, "notes: template not found; creating a plain note");
            None
        }
    }
}

/// Open or create today's journal entry (FR-99).
#[tauri::command]
pub async fn notes_journal_today(vault_id: String) -> Result<NoteRefVm, IpcError> {
    let vault = vault_of(&vault_id)?;
    let rel = notes_vault::journal_rel(&vault);
    // Invoked twice in one day, this opens the same file and appends nothing.
    if let Some(snapshot) = notes_vault::snapshot(&vault_id) {
        if let Some(entry) = snapshot.by_path(&rel) {
            return Ok(NoteRefVm {
                vault_id,
                id: entry.id.clone(),
                path: entry.path.clone(),
                title: entry.title.clone(),
            });
        }
    }
    if notes_vault::contained(&vault, &rel)
        .map_err(notes_error)?
        .exists()
    {
        // On disk but not yet indexed (a cold start still parsing): read it
        // rather than overwrite it.
        let source = notes_vault::read_note(&vault, &rel).map_err(notes_error)?;
        let (fm, offset) = Frontmatter::parse(&source);
        return Ok(NoteRefVm {
            vault_id,
            id: fm
                .as_string("id")
                .map_or_else(|| format!("path:{rel}"), str::to_owned),
            title: naming::title_from_body(source.get(offset..).unwrap_or("")),
            path: rel,
        });
    }

    let title = rel
        .rsplit('/')
        .next()
        .unwrap_or(&rel)
        .trim_end_matches(".md")
        .to_owned();
    // No `dest`: the journal path is fixed by the configured template, so the
    // collision counter must not be allowed to move it.
    let req = NoteCreateReq {
        title: Some(title),
        body: None,
        template: vault.config.default_template.clone(),
        dest: None,
        tags: Vec::new(),
    };
    create_journal(&vault, &rel, &req)
}

/// Create the journal note at exactly `rel`.
///
/// Deliberately not `create_note`: the journal's filename comes from the
/// configured path template, so applying the collision counter would put today's
/// entry at `2026-08-02-2.md` and every later lookup would miss it.
fn create_journal(vault: &Vault, rel: &str, req: &NoteCreateReq) -> Result<NoteRefVm, IpcError> {
    let id = crate::sync_ipc::new_ulid();
    let title = req.title.clone().unwrap_or_else(today);
    let (body, caret) = match template_source(vault, req) {
        Some(template) => {
            let (expanded, caret) = templates::expand(
                &template,
                &templates::TemplateCtx {
                    title: title.clone(),
                    id: id.clone(),
                    now_local: now_local(),
                },
            );
            (expanded, caret)
        }
        None => (format!("# {title}\n"), None),
    };
    let front = Frontmatter::serialise_new(&[
        ("id".to_owned(), FieldValue::Str(id.clone())),
        ("created".to_owned(), FieldValue::Str(now_local())),
        ("updated".to_owned(), FieldValue::Str(now_local())),
    ]);
    notes_vault::write_note(vault, rel, &format!("{front}\n{body}")).map_err(notes_error)?;
    if let Some(at) = caret {
        remember_caret(&id, at + 1);
    }
    Ok(NoteRefVm {
        vault_id: vault.id.clone(),
        id,
        path: rel.to_owned(),
        title,
    })
}

/// Retitle a note and rename its file (FR-97).
///
/// The ULID `id` is what keeps links, pins and unread marks intact, so the
/// filename is free to change.
#[tauri::command]
pub async fn notes_rename(
    vault_id: String,
    note_id: String,
    title: String,
) -> Result<NoteRefVm, IpcError> {
    let vault = vault_of(&vault_id)?;
    let entry = entry_of(&vault_id, &note_id)?;
    let dir = entry
        .path
        .rsplit_once('/')
        .map_or(String::new(), |(dir, _)| dir.to_owned());
    let filename = naming::note_filename(&title, &today(), &notes_vault::siblings(&vault, &dir));
    let rel = if dir.is_empty() {
        filename
    } else {
        format!("{dir}/{filename}")
    };
    notes_vault::rename_note(&vault, &entry.path, &rel).map_err(notes_error)?;
    // Retitling also updates the note's own first heading when it carried one,
    // so the file and the list cannot disagree about what the note is called.
    Ok(NoteRefVm {
        vault_id,
        id: entry.id,
        path: rel,
        title,
    })
}

/// Set or clear a note flag (FR-119).
#[tauri::command]
pub async fn notes_set_flag(
    vault_id: String,
    note_id: String,
    flag: NoteFlag,
    on: bool,
) -> Result<(), IpcError> {
    let vault = vault_of(&vault_id)?;
    let entry = entry_of(&vault_id, &note_id)?;
    let key = match flag {
        NoteFlag::Pinned => "pinned",
        NoteFlag::Archived => "archived",
    };
    let source = notes_vault::read_note(&vault, &entry.path).map_err(notes_error)?;
    // A splice, not a re-serialisation: every byte outside this key's span
    // survives (FR-121).
    let updated = if on {
        Frontmatter::set_in(&source, key, FieldValue::Bool(true))
    } else {
        Frontmatter::remove_in(&source, key)
    };
    notes_vault::write_note(&vault, &entry.path, &updated).map_err(notes_error)
}

/// Move a note to the trash and stage its removal (NFR-30). Never an `unlink`.
#[tauri::command]
pub async fn notes_delete(vault_id: String, note_id: String) -> Result<(), IpcError> {
    let vault = vault_of(&vault_id)?;
    let entry = entry_of(&vault_id, &note_id)?;
    notes_vault::trash_note(&vault, &entry.path)
        .map(drop)
        .map_err(notes_error)
}

// ---------------------------------------------------------------------------
// The editor
// ---------------------------------------------------------------------------

/// Open a note for editing, returning a subscription id (AD-58).
///
/// The channel opens with a full `Reset` and then carries diffs for the life of
/// the subscription: an external write arrives as `External` on a clean buffer and
/// as `Diverged` on a dirty one, a rename as `Renamed`, a delete as `Gone`.
///
/// The frontmatter block is sent **beside** the body, never inside it: the editor
/// buffer holds the body alone, the block renders as the typed properties panel
/// (FR-107), and `notes_save` re-joins them. That is what makes "the first
/// keystroke lands above `---`" — observed on the agent-desktop run of 2026-08-03
/// — a shape this code cannot produce rather than a case it has to repair.
#[tauri::command]
pub async fn notes_open(
    vault_id: String,
    note_id: String,
    channel: Channel<NoteBodyBatch>,
) -> Result<String, IpcError> {
    let vault = vault_of(&vault_id)?;
    let entry = entry_of_soon(&vault_id, &note_id).await?;
    let text = notes_vault::read_note(&vault, &entry.path).map_err(notes_error)?;
    let rev = notes_vault::content_rev(&text);
    let (frontmatter, body) = split_note(&text);

    channel
        .send(NoteBodyBatch::Reset {
            rev: rev.clone(),
            frontmatter: frontmatter.to_owned(),
            text: body.to_owned(),
            // A template's `{{cursor}}`, once, and nothing else: without a hint the
            // editor puts the caret at the end of the body, which is where someone
            // continuing a note wants it.
            cursor: take_caret(&note_id),
        })
        .map_err(|error| {
            notes_error(NotesError::NotFound(format!(
                "could not open {}: {error}",
                entry.path
            )))
        })?;

    let sub = Arc::new(BodySub {
        vault_id: vault_id.clone(),
        note_id: note_id.clone(),
        channel,
        state: Mutex::new(BodyState {
            rel: entry.path.clone(),
            base: text,
            rev,
            mine: None,
        }),
    });
    let watcher = Arc::clone(&sub);
    let task = tauri::async_runtime::spawn(async move {
        watch_body(watcher).await;
    });
    Ok(register(Some(sub), task))
}

/// Follow one note for the life of its subscription.
async fn watch_body(sub: Arc<BodySub>) {
    let Some(mut index) = notes_vault::subscribe_index(&sub.vault_id) else {
        return;
    };
    let Some(vault) = notes_vault::vault(&sub.vault_id) else {
        return;
    };
    while index.changed().await.is_ok() {
        let snapshot = index.borrow_and_update().clone();
        let Some(entry) = snapshot.by_id(&sub.note_id) else {
            // The note is gone from the index: deleted, or moved out of the
            // vault. Either way the editor must stop pretending.
            let _ = sub.channel.send(NoteBodyBatch::Gone);
            return;
        };
        let renamed = {
            let state = lock_body(&sub);
            (state.rel != entry.path).then(|| entry.path.clone())
        };
        if let Some(path) = renamed {
            lock_body(&sub).rel = path.clone();
            let _ = sub.channel.send(NoteBodyBatch::Renamed { path });
        }
        let Ok(text) = notes_vault::read_note(&vault, &entry.path) else {
            continue;
        };
        let rev = notes_vault::content_rev(&text);
        let mut state = lock_body(&sub);
        // The echo check: this is the revision we last delivered or wrote, so
        // the change is keeper's own write coming back.
        if state.rev == rev {
            continue;
        }
        let (block, body) = split_note(&text);
        let (block, body) = (block.to_owned(), body.to_owned());
        if state.is_dirty() {
            // Never applied over unsaved edits. The editor raises its inline diff
            // bar; the user's buffer is untouched and `base` still names the true
            // common ancestor.
            state.rev = rev.clone();
            drop(state);
            let _ = sub.channel.send(NoteBodyBatch::Diverged {
                rev,
                frontmatter: block,
                theirs: body,
            });
        } else {
            state.base = text;
            state.rev = rev.clone();
            state.mine = None;
            drop(state);
            let _ = sub.channel.send(NoteBodyBatch::External {
                rev,
                frontmatter: block,
                text: body,
            });
        }
    }
}

fn lock_body(sub: &BodySub) -> MutexGuard<'_, BodyState> {
    sub.state
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// Close an editor subscription.
#[tauri::command]
pub async fn notes_close(subscription_id: String) -> Result<(), IpcError> {
    unregister(&subscription_id);
    Ok(())
}

/// The editor's dirty-text heartbeat.
///
/// Sent after 400 ms of typing idle and immediately on blur, so Rust always knows
/// whether the buffer diverges from what it last delivered — which is what decides
/// `External` against `Diverged` when a write lands underneath. `text` is the
/// **body**, in the same space as the channel's.
#[tauri::command]
pub async fn notes_buffer_report(
    subscription_id: String,
    text: String,
    rev: String,
) -> Result<(), IpcError> {
    let sub = body_sub(&subscription_id)?;
    let mut state = lock_body(&sub);
    // A report against a revision we have moved past is stale — the editor sent
    // it before it applied an external change — so it must not resurrect the old
    // buffer as "mine".
    if state.rev == rev {
        state.mine = Some(text);
    }
    Ok(())
}

/// The bytes a save writes: the editor's body under the block that owns it, with
/// `updated` stamped.
///
/// Named so the composition is testable on its own — `notes_save` cannot be called
/// without a live subscription, and this is the part of it that decides what lands
/// on disk. `updated` is rewritten on every keeper-authored save; `created` never
/// is, because `set_in` touches one key's span and nothing else (FR-121).
fn save_document(frontmatter: &str, body: &str) -> String {
    Frontmatter::set_in(
        &join_note(frontmatter, body),
        "updated",
        FieldValue::Str(now_local()),
    )
}

/// Save a note (FR-112, NFR-30).
///
/// `text` is the **body**. The editor never holds the frontmatter block, so this is
/// where the two halves are re-joined: `frontmatter` is `Some` only when the
/// properties panel rewrote the block, and absent it the block keeper last
/// delivered stands, byte for byte. That is the write half of FR-121 — every key
/// the user did not edit survives — and it is why a caret at offset 0 can no longer
/// cost a note its `id`.
///
/// A `base_rev` older than what is on disk means the other side changed since the
/// editor opened: the disk bytes are written aside as an AD-43-shaped conflict
/// copy **first**, and only then is the buffer written. So a diverged save keeps
/// both sides, and it does so only at save time, never before the user has seen
/// the diff bar.
#[tauri::command]
pub async fn notes_save(
    subscription_id: String,
    text: String,
    base_rev: String,
    frontmatter: Option<String>,
) -> Result<NoteWriteVm, IpcError> {
    let sub = body_sub(&subscription_id)?;
    let vault = vault_of(&sub.vault_id)?;
    let rel = lock_body(&sub).rel.clone();

    let disk = notes_vault::read_note(&vault, &rel).unwrap_or_default();
    let disk_rev = notes_vault::content_rev(&disk);
    let conflict_copy = if disk_rev == base_rev || disk.is_empty() {
        None
    } else {
        notes_vault::write_conflict_copy(&vault, &rel, &disk)
    };

    let stamped = {
        let state = lock_body(&sub);
        save_document(
            frontmatter.as_deref().unwrap_or_else(|| state.split().0),
            &text,
        )
    };
    notes_vault::write_note(&vault, &rel, &stamped).map_err(notes_error)?;
    let rev = notes_vault::content_rev(&stamped);
    let block = split_note(&stamped).0.to_owned();
    {
        let mut state = lock_body(&sub);
        state.base = stamped;
        state.rev = rev.clone();
        state.mine = None;
    }
    Ok(NoteWriteVm {
        rev,
        path: rel,
        frontmatter: block,
        conflict_copy,
    })
}

// ---------------------------------------------------------------------------
// Search
// ---------------------------------------------------------------------------

/// Bounded parallel content scan, streamed as found (FR-118).
#[tauri::command]
pub async fn notes_search(
    vault_id: String,
    req: NoteSearchReq,
    channel: Channel<NoteSearchBatch>,
) -> Result<String, IpcError> {
    let vault = vault_of(&vault_id)?;
    let snapshot = notes_vault::snapshot(&vault_id)
        .ok_or_else(|| notes_error(NotesError::VaultUnknown(vault_id)))?;
    let limit = if req.limit == 0 {
        MAX_SEARCH_HITS
    } else {
        (req.limit as usize).min(MAX_SEARCH_HITS)
    };
    let task = tauri::async_runtime::spawn(async move {
        run_search(&vault, &snapshot, &req.text, limit, &channel).await;
    });
    Ok(register(None, task))
}

/// Walk the index's entries, reading each note once, streaming batches as they
/// fill.
///
/// The scan is bounded two ways — a hit ceiling and the index's own entry set —
/// so it can never be stale (there is nothing to invalidate) and never unbounded.
async fn run_search(
    vault: &Vault,
    snapshot: &IndexSnapshot,
    needle: &str,
    limit: usize,
    channel: &Channel<NoteSearchBatch>,
) {
    if needle.trim().is_empty() {
        let _ = channel.send(NoteSearchBatch {
            done: true,
            hits: Vec::new(),
        });
        return;
    }
    let mut hits: Vec<NoteSearchHitVm> = Vec::new();
    let mut total = 0usize;
    for entry in snapshot.entries() {
        if total >= limit {
            break;
        }
        let Ok(text) = notes_vault::read_note(vault, &entry.path) else {
            continue;
        };
        for hit in search::find(&text, needle, limit - total) {
            hits.push(NoteSearchHitVm {
                id: entry.id.clone(),
                path: entry.path.clone(),
                title: entry.title.clone(),
                line: hit.line,
                snippet: hit.snippet,
            });
            total += 1;
        }
        // Flush a partial batch so results appear as they are found rather than
        // at the end, which is the whole point of streaming them.
        if hits.len() >= 20 {
            if channel
                .send(NoteSearchBatch {
                    done: false,
                    hits: std::mem::take(&mut hits),
                })
                .is_err()
            {
                return;
            }
            // Yield between batches: a 10 000-note scan must not starve the
            // runtime it shares with the editor.
            tokio::task::yield_now().await;
        }
    }
    let _ = channel.send(NoteSearchBatch { done: true, hits });
}

// ---------------------------------------------------------------------------
// History, diffs and unread state
// ---------------------------------------------------------------------------

/// Per-note history, projected from commit trailers (FR-114, AD-63).
#[tauri::command]
pub async fn notes_history(
    app: AppHandle,
    vault_id: String,
    note_id: String,
    limit: u32,
) -> Result<Vec<NoteRevisionVm>, IpcError> {
    let vault = vault_of(&vault_id)?;
    let entry = entry_of(&vault_id, &note_id)?;
    let path = entry.path;
    Ok(
        tokio::task::spawn_blocking(move || notes_vault::revisions(&app, &vault, &path, limit))
            .await
            .unwrap_or_default(),
    )
}

/// The diff of one note between two revisions, or against the working tree.
#[tauri::command]
pub async fn notes_diff(
    app: AppHandle,
    vault_id: String,
    note_id: String,
    from_rev: String,
    to_rev: Option<String>,
) -> Result<NoteDiffVm, IpcError> {
    let vault = vault_of(&vault_id)?;
    let entry = entry_of(&vault_id, &note_id)?;
    let path = entry.path;
    tokio::task::spawn_blocking(move || {
        notes_vault::diff(&app, &vault, &path, &from_rev, to_rev.as_deref())
    })
    .await
    .map_err(|error| notes_error(NotesError::NotFound(error.to_string())))
}

/// Acknowledge a revision, clearing the unread mark and the tray dot (FR-113).
#[tauri::command]
pub async fn notes_mark_read(
    state: State<'_, AppState>,
    vault_id: String,
    note_id: String,
    rev: String,
) -> Result<(), IpcError> {
    // Resolved even though only the id is stored, so an unknown vault or note is
    // refused rather than writing a mark nothing will ever read.
    let _ = vault_of(&vault_id)?;
    let _ = entry_of(&vault_id, &note_id)?;
    notes_vault::mark_read(state.platform.as_ref(), &note_id, &rev).map_err(notes_error)
}

/// Every conflict copy in the vault, bound to its canonical note (FR-116).
#[tauri::command]
pub async fn notes_conflicts(vault_id: String) -> Result<Vec<NoteConflictVm>, IpcError> {
    let vault = vault_of(&vault_id)?;
    let snapshot = notes_vault::snapshot(&vault_id)
        .ok_or_else(|| notes_error(NotesError::VaultUnknown(vault_id)))?;
    Ok(notes_vault::conflicts(&vault)
        .into_iter()
        .map(|(canonical, copy)| {
            let entry = snapshot.by_path(&canonical);
            // The revision of MY side, in the same space as `NoteWriteVm.rev` and
            // `NoteBodyBatch.rev` — so the resolver can hand it straight back to
            // `notes_save` as a `base_rev` and the staleness check means something.
            let mine_rev = notes_vault::read_note(&vault, &canonical)
                .map(|text| notes_vault::content_rev(&text))
                .unwrap_or_default();
            NoteConflictVm {
                // The row is bound to the CANONICAL note, so a conflict is one
                // row and not two unrelated notes.
                id: entry.map_or_else(|| format!("path:{canonical}"), |entry| entry.id.clone()),
                path: canonical,
                mine_rev,
                theirs_path: copy,
            }
        })
        .collect())
}

/// Resolve a conflict: keep mine, take theirs, or write a merged body (FR-116).
///
/// Every branch removes the conflict copy through the trash, so the bytes of the
/// side that lost are recoverable from `.keeper/trash/` as well as from history.
#[tauri::command]
pub async fn notes_resolve_conflict(
    vault_id: String,
    note_id: String,
    choice: NoteConflictChoiceReq,
) -> Result<NoteRefVm, IpcError> {
    let vault = vault_of(&vault_id)?;
    let entry = entry_of(&vault_id, &note_id)?;
    let copy = notes_vault::conflicts(&vault)
        .into_iter()
        .find(|(canonical, _)| canonical == &entry.path)
        .map(|(_, copy)| copy)
        .ok_or_else(|| {
            notes_error(NotesError::NotFound(format!(
                "no conflict for {}",
                entry.path
            )))
        })?;

    match choice {
        NoteConflictChoiceReq::TakeMine => {}
        NoteConflictChoiceReq::TakeTheirs => {
            let theirs = notes_vault::read_note(&vault, &copy).map_err(notes_error)?;
            notes_vault::write_note(&vault, &entry.path, &theirs).map_err(notes_error)?;
        }
        NoteConflictChoiceReq::Merged { text } => {
            // The resolver aligns bodies, so what comes back is a body. The block on
            // the canonical note is keeper's, and it survives the resolution — losing
            // it would cost the note its `id` and every link, pin and unread mark
            // hanging off it.
            let canonical = notes_vault::read_note(&vault, &entry.path).unwrap_or_default();
            let merged = join_note(split_note(&canonical).0, &text);
            notes_vault::write_note(&vault, &entry.path, &merged).map_err(notes_error)?;
        }
    }
    notes_vault::trash_note(&vault, &copy)
        .map(drop)
        .map_err(notes_error)?;
    Ok(NoteRefVm {
        vault_id,
        id: entry.id,
        path: entry.path,
        title: entry.title,
    })
}

// ---------------------------------------------------------------------------
// Attachments
// ---------------------------------------------------------------------------

/// Import a clipboard image into the vault (FR-110).
///
/// **Not implemented, and honestly so.** Reading an image off the system
/// clipboard needs a clipboard backend — `tauri-plugin-clipboard-manager` or an
/// equivalent — and this build links none: it is absent from `Cargo.toml`, from
/// `Cargo.lock` and from `package.json`, and the phase's dependency rules add no
/// Rust crates. So rather than pretend, this refuses with `Unsupported` and names
/// the working alternative, which needs nothing new: `notes_attachment_drop`
/// takes real paths, from Tauri's own drag-drop event or from the file picker the
/// dialog plugin already provides.
#[tauri::command]
pub async fn notes_attachment_paste(
    vault_id: String,
    note_id: String,
) -> Result<NoteAttachmentVm, IpcError> {
    let _ = vault_of(&vault_id)?;
    let _ = entry_of(&vault_id, &note_id)?;
    Err(IpcError {
        code: IpcErrorCode::Unsupported,
        message: "pasting an image needs a clipboard backend this build does not link; \
                  drop the file onto the editor, or attach it from the file picker"
            .to_owned(),
        account_id: None,
        retriable: false,
    })
}

/// Import dropped files into the vault (FR-110).
///
/// The paths come from Tauri's own `DragDropEvent`, never from JavaScript, and
/// the bytes never cross IPC in either direction (AD-58).
#[tauri::command]
pub async fn notes_attachment_drop(
    vault_id: String,
    note_id: String,
    paths: Vec<String>,
) -> Result<Vec<NoteAttachmentVm>, IpcError> {
    let vault = vault_of(&vault_id)?;
    let _ = entry_of(&vault_id, &note_id)?;
    let mut imported = Vec::with_capacity(paths.len());
    for path in paths {
        let source = PathBuf::from(&path);
        // A directory drop is not an attachment, and a path that is not a regular
        // file is refused rather than copied blindly.
        if !source.is_file() {
            tracing::info!("notes: skipped a dropped path that is not a file");
            continue;
        }
        imported.push(notes_vault::import_attachment(&vault, &source).map_err(notes_error)?);
    }
    Ok(imported)
}

// ---------------------------------------------------------------------------
// Capture
// ---------------------------------------------------------------------------

/// The persisted capture buffer (FR-101).
#[tauri::command]
pub async fn notes_capture_buffer(state: State<'_, AppState>) -> Result<String, IpcError> {
    read_buffer(state.platform.as_ref())
}

/// Read the buffer through the platform port, so the command and the Escape path
/// share one implementation.
fn read_buffer(platform: &dyn keeper_core::platform::Platform) -> Result<String, IpcError> {
    let data_dir = platform
        .data_dir()
        .map_err(|error| notes_error(NotesError::Name(error.to_string())))?;
    keeper_core::registry::get_capture_buffer(&data_dir)
        .map_err(|error| notes_error(NotesError::Name(error.to_string())))
}

/// Persist the capture buffer (FR-101).
///
/// Debounced 300 ms by the caller and never on the critical path. It is in
/// `keeper.db` rather than the webview, so the text survives both dismissal and
/// `kill -9`.
#[tauri::command]
pub async fn notes_capture_buffer_save(
    state: State<'_, AppState>,
    text: String,
) -> Result<(), IpcError> {
    let data_dir = state
        .platform
        .data_dir()
        .map_err(|error| notes_error(NotesError::Name(error.to_string())))?;
    keeper_core::registry::set_capture_buffer(&data_dir, &text)
        .map_err(|error| notes_error(NotesError::Name(error.to_string())))
}

/// Write the capture buffer into the active vault and clear it (FR-101).
///
/// The buffer is cleared **only after** the write returns, so a failure on a
/// read-only volume leaves the text exactly where it was.
#[tauri::command]
pub async fn notes_capture_commit(
    state: State<'_, AppState>,
    text: String,
) -> Result<NoteRefVm, IpcError> {
    commit_buffer(state.platform.as_ref(), text)
}

/// Write `text` into the active vault as a capture note and clear the buffer.
fn commit_buffer(
    platform: &dyn keeper_core::platform::Platform,
    text: String,
) -> Result<NoteRefVm, IpcError> {
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return Err(notes_error(NotesError::Name(
            "nothing to capture".to_owned(),
        )));
    }
    let vault_id = notes_vault::active_vault(platform).ok_or_else(|| IpcError {
        code: IpcErrorCode::Unsupported,
        message: "no notes vault yet — flag a folder you already sync and it becomes one"
            .to_owned(),
        account_id: None,
        retriable: false,
    })?;
    let vault = vault_of(&vault_id)?;
    let reference = create_note(
        &vault,
        &NoteCreateReq {
            title: None,
            body: Some(text),
            template: None,
            dest: None,
            tags: Vec::new(),
        },
        true,
    )?;
    if let Ok(data_dir) = platform.data_dir() {
        if let Err(error) = keeper_core::registry::set_capture_buffer(&data_dir, "") {
            tracing::warn!(%error, "notes: captured the note but could not clear the buffer");
        }
    }
    Ok(reference)
}

// ---------------------------------------------------------------------------
// Streams
// ---------------------------------------------------------------------------

/// Stream list changes for a vault, opening with a `Reset` (AD-8, AD-58).
#[tauri::command]
pub async fn notes_subscribe_changes(
    state: State<'_, AppState>,
    vault_id: String,
    channel: Channel<NoteChangeBatch>,
) -> Result<String, IpcError> {
    let vault = vault_of(&vault_id)?;
    let platform = Arc::clone(&state.platform);
    let task = tauri::async_runtime::spawn(async move {
        stream_changes(platform, vault, channel).await;
    });
    Ok(register(None, task))
}

/// Stop a changes subscription.
///
/// Also stops an index subscription: subscription ids are minted from one space,
/// so this cancels whichever kind the id names. A double-unsubscribe is a no-op.
#[tauri::command]
pub async fn notes_unsubscribe_changes(subscription_id: String) -> Result<(), IpcError> {
    unregister(&subscription_id);
    Ok(())
}

/// The loop behind a changes subscription.
async fn stream_changes(
    platform: Arc<dyn keeper_core::platform::Platform>,
    vault: Vault,
    channel: Channel<NoteChangeBatch>,
) {
    let Some(mut index) = notes_vault::subscribe_index(&vault.id) else {
        return;
    };
    let mut previous: Vec<(String, String)> = Vec::new();
    // The opening snapshot, then one message per change at most every
    // CHANGE_BATCH_MS. `while let` rather than `loop`: the window read is the
    // condition — a vault that stops answering has nothing left to stream.
    while let Ok(rows) = current_window(platform.as_ref(), &vault) {
        let next = fingerprints(&rows.rows);
        let ops = if previous.is_empty() {
            vec![NoteListOp::Reset {
                rows: rows.rows,
                total: rows.total,
            }]
        } else {
            diff_ops(&previous, &next, &rows.rows)
        };
        previous = next;
        if !ops.is_empty()
            && channel
                .send(NoteChangeBatch {
                    vault_id: vault.id.clone(),
                    ops,
                })
                .is_err()
        {
            // The webview is gone; a closed window unsubscribes itself.
            return;
        }
        if index.changed().await.is_err() {
            return;
        }
        // Coalesce the burst behind this wake-up into one message.
        tokio::time::sleep(Duration::from_millis(CHANGE_BATCH_MS)).await;
        index.mark_unchanged();
    }
}

/// The window a changes subscription streams: the last query `notes_list` was
/// asked for on this vault, or the default one.
fn current_window(
    platform: &dyn keeper_core::platform::Platform,
    vault: &Vault,
) -> Result<NoteListVm, IpcError> {
    let req = last_queries()
        .get(&vault.id)
        .cloned()
        .unwrap_or_else(default_query);
    project_list(platform, vault, &req)
}

/// The window with no filter applied.
fn default_query() -> NoteQueryReq {
    NoteQueryReq {
        text: None,
        tags: Vec::new(),
        space_id: None,
        origin: None,
        flags: Vec::new(),
        offset: 0,
        limit: DEFAULT_LIMIT,
    }
}

/// `(id, fingerprint)` per row.
///
/// The fingerprint is the row's own JSON, because `NoteRowVm` carries no
/// equality and every field in it is visible: anything that would repaint a row
/// changes its serialisation, and nothing that would not, does. Sixty rows of a
/// few hundred bytes is a rounding error next to the message it saves.
fn fingerprints(rows: &[NoteRowVm]) -> Vec<(String, String)> {
    rows.iter()
        .map(|row| {
            (
                row.id.clone(),
                serde_json::to_string(row).unwrap_or_default(),
            )
        })
        .collect()
}

/// The minimal op list that turns `previous` into `next`.
fn diff_ops(
    previous: &[(String, String)],
    next: &[(String, String)],
    rows: &[NoteRowVm],
) -> Vec<NoteListOp> {
    let mut ops = Vec::new();
    for (id, _) in previous {
        if !next.iter().any(|(existing, _)| existing == id) {
            ops.push(NoteListOp::Remove { id: id.clone() });
        }
    }
    for (index, ((id, fingerprint), row)) in next.iter().zip(rows.iter()).enumerate() {
        let unchanged = previous
            .get(index)
            .is_some_and(|(prev_id, prev_fingerprint)| {
                prev_id == id && prev_fingerprint == fingerprint
            });
        if !unchanged {
            ops.push(NoteListOp::Upsert {
                index: u32::try_from(index).unwrap_or(u32::MAX),
                row: row.clone(),
            });
        }
    }
    ops
}

/// Stream cold-scan progress for a vault.
#[tauri::command]
pub async fn notes_subscribe_index(
    vault_id: String,
    channel: Channel<NoteIndexProgressVm>,
) -> Result<String, IpcError> {
    let _ = vault_of(&vault_id)?;
    let Some(mut progress) = notes_vault::subscribe_progress(&vault_id) else {
        return Err(notes_error(NotesError::VaultUnknown(vault_id)));
    };
    // Opens with the current phase, so a subscriber that arrives after the scan
    // finished is told so rather than waiting for a change that will not come.
    if let Some(current) = notes_vault::progress(&vault_id) {
        let _ = channel.send(current);
    }
    let task = tauri::async_runtime::spawn(async move {
        while progress.changed().await.is_ok() {
            let snapshot = progress.borrow_and_update().clone();
            if channel.send(snapshot).is_err() {
                return;
            }
        }
    });
    Ok(register(None, task))
}

// ---------------------------------------------------------------------------
// The tray
// ---------------------------------------------------------------------------

/// Emitted when the tray asks the main window to open a note. Carries a
/// `NoteRefVm` — ids only, and a type the frontend already has generated bindings
/// for, so this is not the one hand-written payload shape in the IPC layer.
#[cfg(desktop)]
pub const NOTES_OPEN_NOTE_EVENT: &str = "keeper://notes-open-note";

/// Emitted when the tray's unread line is chosen: open the notes view with the
/// origin filter armed. Carries the vault id as a bare string.
#[cfg(desktop)]
pub const NOTES_SHOW_UNREAD_EVENT: &str = "keeper://notes-show-unread";

/// Compose what the tray should say about notes (Story 36.7).
///
/// Called by the ~1 Hz tick that already drives the recording and sync renderings,
/// and deliberately cheap and total: it reads the published index and never builds
/// an engine, because painting a menu label must not open a database.
#[cfg(desktop)]
pub fn tray_snapshot(app: &AppHandle) -> crate::tray::NotesTray {
    let state = app.state::<AppState>();
    let platform = state.platform.as_ref();
    let vault_id = notes_vault::active_vault(platform);
    let recent = vault_id
        .as_deref()
        .and_then(notes_vault::snapshot)
        .map(|snapshot| {
            let mut entries: Vec<&IndexEntry> = snapshot
                .entries()
                .iter()
                .filter(|entry| !has_flag(entry, "conflict") && !has_flag(entry, "archived"))
                .collect();
            // Last touched, newest first — `updated_ms` is exactly that, and it
            // survives a restart because it comes from the note rather than from a
            // session list.
            entries.sort_by(|a, b| b.updated_ms.cmp(&a.updated_ms).then(a.path.cmp(&b.path)));
            entries
                .into_iter()
                .take(crate::tray::NOTE_RECENT_SLOTS)
                .map(|entry| (entry.id.clone(), entry.title.clone()))
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    let unread = vault_id
        .as_deref()
        .map_or(0, |id| notes_vault::unread_count(platform, id));
    crate::tray::NotesTray {
        vault_id,
        recent,
        unread,
        hotkey_registered: capture_hotkey_registered(app),
    }
}

/// Whether the OS actually holds the capture shortcut right now.
///
/// Re-probed rather than remembered, because a shortcut can be taken by another
/// application after keeper registered it, and the tray label is where the user
/// finds that out (UX-DR43).
#[cfg(desktop)]
fn capture_hotkey_registered(app: &AppHandle) -> bool {
    use tauri_plugin_global_shortcut::GlobalShortcutExt as _;

    let Ok(data_dir) = app.state::<AppState>().platform.data_dir() else {
        return false;
    };
    let Ok(accelerator) = keeper_core::registry::get_capture_hotkey(&data_dir) else {
        return false;
    };
    crate::hotkey::capture_shortcut(&accelerator)
        .is_some_and(|shortcut| app.global_shortcut().is_registered(shortcut))
}

/// The tray's **New Note**: create an empty note in the active vault and raise the
/// window onto it.
///
/// With no vault flagged this raises the window and emits nothing, which is what
/// sends the user to the empty state that offers Settings → Sync — the action is
/// achievable, so the item is never disabled (UX empty-state table).
#[cfg(desktop)]
pub fn tray_new_note(app: &AppHandle) {
    crate::tray::show_main_window(app);
    let Some(vault) = active_vault_of(app) else {
        return;
    };
    match create_note(&vault, &blank_note(), false) {
        Ok(reference) => emit_open(app, &reference),
        Err(error) => {
            tracing::warn!(message = %error.message, "notes: tray could not create a note");
        }
    }
}

/// The tray's **Today's Journal** (FR-99).
#[cfg(desktop)]
pub fn tray_journal_today(app: &AppHandle) {
    crate::tray::show_main_window(app);
    let Some(vault) = active_vault_of(app) else {
        return;
    };
    let app = app.clone();
    let vault_id = vault.id.clone();
    tauri::async_runtime::spawn(async move {
        match notes_journal_today(vault_id).await {
            Ok(reference) => emit_open(&app, &reference),
            Err(error) => {
                tracing::warn!(message = %error.message, "notes: tray could not open the journal");
            }
        }
    });
}

/// The tray's recent slot `index`.
#[cfg(desktop)]
pub fn tray_open_recent(app: &AppHandle, slot: usize) {
    crate::tray::show_main_window(app);
    let snapshot = tray_snapshot(app);
    let Some((note_id, _)) = snapshot.recent.get(slot) else {
        return;
    };
    let Some(vault_id) = snapshot.vault_id.as_ref() else {
        return;
    };
    let Ok(entry) = entry_of(vault_id, note_id) else {
        return;
    };
    emit_open(
        app,
        &NoteRefVm {
            vault_id: vault_id.clone(),
            id: entry.id,
            path: entry.path,
            title: entry.title,
        },
    );
}

/// The tray's unread line: raise the window with the origin filter armed.
#[cfg(desktop)]
pub fn tray_show_unread(app: &AppHandle) {
    crate::tray::show_main_window(app);
    let Some(vault_id) = notes_vault::active_vault(app.state::<AppState>().platform.as_ref())
    else {
        return;
    };
    if let Err(error) = app.emit(NOTES_SHOW_UNREAD_EVENT, vault_id) {
        tracing::warn!(%error, "notes: could not emit the show-unread event");
    }
}

/// The active vault, for a tray action.
#[cfg(desktop)]
fn active_vault_of(app: &AppHandle) -> Option<Vault> {
    let vault_id = notes_vault::active_vault(app.state::<AppState>().platform.as_ref())?;
    notes_vault::vault(&vault_id)
}

/// An empty note request — no title, no body, no template, no destination. The
/// whole of "nothing is asked" (UX-DR35).
#[cfg(desktop)]
fn blank_note() -> NoteCreateReq {
    NoteCreateReq {
        title: None,
        body: None,
        template: None,
        dest: None,
        tags: Vec::new(),
    }
}

/// Tell the window which note to select.
#[cfg(desktop)]
fn emit_open(app: &AppHandle, reference: &NoteRefVm) {
    if let Err(error) = app.emit(NOTES_OPEN_NOTE_EVENT, reference) {
        tracing::warn!(%error, "notes: could not emit the open-note event");
    }
}

// ---------------------------------------------------------------------------
// Desktop-only commands
// ---------------------------------------------------------------------------

/// Show the quick-capture panel (NFR-27).
#[cfg(desktop)]
#[tauri::command]
pub async fn notes_capture_show(app: AppHandle) -> Result<(), IpcError> {
    crate::notes_window::show(&app);
    Ok(())
}

/// Hide the quick-capture panel; `commit: true` is Escape, which saves first.
///
/// Returns the note it wrote, or `None` when there was nothing to write — an
/// empty Escape is not an error, and with no vault flagged the text stays in the
/// durable buffer (FR-101).
#[cfg(desktop)]
#[tauri::command]
pub async fn notes_capture_hide(
    app: AppHandle,
    state: State<'_, AppState>,
    commit: bool,
) -> Result<Option<NoteRefVm>, IpcError> {
    let mut written = None;
    if commit {
        let platform = state.platform.as_ref();
        let buffer = read_buffer(platform)?;
        if !buffer.trim().is_empty() {
            match commit_buffer(platform, buffer) {
                Ok(reference) => written = Some(reference),
                Err(error) => {
                    // The panel still hides and the buffer stays intact: a failed
                    // flush must not also lose the thought (NFR-30).
                    tracing::warn!(message = %error.message, "notes: capture flush failed");
                }
            }
        }
    }
    crate::notes_window::hide(&app);
    // Hiding the panel after a capture is the most common way a captured thought
    // ends, so it is a force-flush point (AD-62).
    notes_vault::flush();
    Ok(written)
}

/// Reveal a note's real path in the OS file manager (UX-DR38).
#[cfg(desktop)]
#[tauri::command]
pub async fn notes_reveal(vault_id: String, note_id: String) -> Result<(), IpcError> {
    let vault = vault_of(&vault_id)?;
    let entry = entry_of(&vault_id, &note_id)?;
    let path = notes_vault::contained(&vault, &entry.path).map_err(notes_error)?;
    tauri_plugin_opener::reveal_item_in_dir(&path).map_err(|error| {
        notes_error(NotesError::Name(format!(
            "could not reveal {}: {error}",
            entry.path
        )))
    })
}

/// Open a linked file with its default application (FR-109).
///
/// A file link may point anywhere inside the profile's synced folder and nowhere
/// else, so the path is re-derived and canonicalized here before `opener` sees it
/// — the webview never hands over a path keeper does not validate.
#[cfg(desktop)]
#[tauri::command]
pub async fn notes_open_file(vault_id: String, rel_path: String) -> Result<(), IpcError> {
    let vault = vault_of(&vault_id)?;
    let path = contained_in_profile(&vault, &rel_path)?;
    tauri_plugin_opener::open_path(&path, None::<&str>).map_err(|error| {
        notes_error(NotesError::Name(format!(
            "could not open {rel_path}: {error}"
        )))
    })
}

/// Resolve a path inside the vault, or inside the profile folder that contains
/// it, refusing anything outside both.
#[cfg(desktop)]
fn contained_in_profile(vault: &Vault, rel_path: &str) -> Result<PathBuf, IpcError> {
    // A vault-relative path is the common case and gets the strict check.
    if let Some(path) = crate::note_protocol::contained_read(vault, rel_path) {
        return Ok(path);
    }
    // Otherwise it may be profile-relative: a note may link to a file beside the
    // vault, which keeper opens and never rewrites.
    let candidate = vault.local_path.join(rel_path);
    let canonical = candidate
        .canonicalize()
        .map_err(|error| notes_error(NotesError::NotFound(format!("{rel_path}: {error}"))))?;
    if !canonical.starts_with(&vault.local_path) || !canonical.is_file() {
        return Err(notes_error(NotesError::Name(format!(
            "{rel_path} is outside the synced folder"
        ))));
    }
    Ok(canonical)
}

#[cfg(test)]
mod tests {
    use keeper_core::notes::vm::NoteCadenceVm;

    use super::*;

    fn entry(path: &str, title: &str) -> IndexEntry {
        IndexEntry {
            id: format!("path:{path}"),
            path: path.to_owned(),
            title: title.to_owned(),
            size: 0,
            mtime_ns: 0,
            ino: 0,
            created_ms: 0,
            updated_ms: 0,
            tags: Vec::new(),
            fields: std::collections::BTreeMap::new(),
            links: Vec::new(),
            flags: Vec::new(),
            snippet: String::new(),
        }
    }

    /// A vault rooted at a real scratch directory, following `notes_vault`'s
    /// convention — `tempfile` is not a dependency of this crate.
    fn test_vault(tag: &str) -> Vault {
        let root = std::env::temp_dir().join(format!(
            "keeper-ipc-{tag}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map_or(0, |since| since.as_nanos())
        ));
        std::fs::create_dir_all(&root).expect("vault root");
        let root = root.canonicalize().expect("canonical vault root");
        Vault {
            id: "01VAULT".to_owned(),
            name: "mind".to_owned(),
            root: root.clone(),
            local_path: root,
            config: NotesConfig::default(),
            excludes: Arc::new(keeper_sync::exclude::ExcludeSet::new(&[]).expect("excludes")),
        }
    }

    /// The bug DW-N1 named, driven through the real create and save code against a
    /// real file: create a note, type at the very first offset the editor can
    /// produce, save.
    ///
    /// Under the old shape the editor buffer opened with `---` at offset 0, so this
    /// keystroke landed ABOVE the block and the saved file carried a stray block in
    /// its body. Now the buffer is the body alone: offset 0 is the body's first byte,
    /// and the block cannot be reached from the editor at all.
    #[test]
    fn typing_at_the_first_offset_cannot_disturb_the_block() {
        let vault = test_vault("caret");
        let created = create_note(
            &vault,
            &NoteCreateReq {
                title: Some("Vault as a lens".to_owned()),
                body: None,
                template: None,
                dest: None,
                tags: Vec::new(),
            },
            false,
        )
        .expect("create the note");

        // What `notes_open` delivers.
        let opened = notes_vault::read_note(&vault, &created.path).expect("read the new note");
        let (block, body) = split_note(&opened);
        assert!(
            block.contains(&format!("id: {}", created.id)),
            "block: {block:?}"
        );
        assert!(!body.contains("---"), "the editor gets no fence: {body:?}");

        // The keystroke that used to break the note, at the only offset that could.
        let typed = format!("The folder is the database.{body}");
        let saved = save_document(block, &typed);
        notes_vault::write_note(&vault, &created.path, &saved).expect("save the note");

        let on_disk = notes_vault::read_note(&vault, &created.path).expect("re-read the note");
        let (kept, kept_body) = split_note(&on_disk);
        assert_eq!(
            on_disk.matches("---\n").count(),
            2,
            "exactly one block, opened and closed once: {on_disk:?}"
        );
        assert_eq!(
            kept.matches(&format!("id: {}", created.id)).count(),
            1,
            "the id is in the block, once: {on_disk:?}"
        );
        assert!(
            kept.contains("created:"),
            "and `created` survived the write: {kept:?}"
        );
        assert!(
            kept_body.starts_with("The folder is the database."),
            "the keystroke landed at the top of the BODY: {kept_body:?}"
        );

        std::fs::remove_dir_all(&vault.root).ok();
    }

    fn row(id: &str, title: &str) -> NoteRowVm {
        NoteRowVm {
            id: id.to_owned(),
            path: format!("{id}.md"),
            title: title.to_owned(),
            snippet: String::new(),
            tags: Vec::new(),
            updated_ms: 0,
            pinned: false,
            archived: false,
            unread: false,
            conflict: false,
            origin: "local".to_owned(),
            head_rev: String::new(),
        }
    }

    #[test]
    fn a_tag_chip_matches_by_segment_and_never_by_prefix() {
        assert!(tag_matches("project", "project"));
        assert!(tag_matches("project/keeper", "project"));
        assert!(
            !tag_matches("projects", "project"),
            "a longer word is a different tag"
        );
        assert!(!tag_matches("project", "project/keeper"));
        // A chip may arrive with its hash still on it.
        assert!(tag_matches("project/keeper", "#project"));
        // Story 42.5: and through the one normalisation, so a chip carrying the
        // casing or the trailing space a recording's card showed selects the
        // node the sidebar named.
        assert!(tag_matches("client/acme", "Client/Acme "));
        assert!(tag_matches("client/acme/renewal", "CLIENT//ACME/"));
        // A chip that is not a tag selects nothing rather than everything.
        assert!(!tag_matches("client/acme", "///"));
        assert!(!tag_matches("client/acme", ""));
    }

    #[test]
    fn the_default_lens_hides_conflict_copies_and_archived_notes() {
        let req = default_query();
        let mut conflict = entry("a.sync-conflict-20260802-120000-mini.md", "a");
        conflict.flags.push("conflict".to_owned());
        assert!(!matches_filter(&conflict, &req, None));

        let mut archived = entry("b.md", "b");
        archived.flags.push("archived".to_owned());
        assert!(!matches_filter(&archived, &req, None));

        // Asked for by name, they appear.
        let asked = NoteQueryReq {
            flags: vec!["conflict".to_owned()],
            ..default_query()
        };
        assert!(matches_filter(&conflict, &asked, None));
        // A plain note is in the default lens.
        assert!(matches_filter(&entry("c.md", "c"), &req, None));
    }

    #[test]
    fn a_text_filter_matches_the_title_the_path_and_the_tags() {
        let mut note = entry("journal/2026-08-02.md", "Vault as a lens");
        note.tags.push("project/keeper".to_owned());
        let filter = |text: &str| NoteQueryReq {
            text: Some(text.to_owned()),
            ..default_query()
        };
        assert!(matches_filter(&note, &filter("vault"), None));
        assert!(matches_filter(&note, &filter("LENS"), None));
        assert!(matches_filter(&note, &filter("journal/"), None));
        assert!(matches_filter(&note, &filter("keeper"), None));
        assert!(!matches_filter(&note, &filter("nothing here"), None));
        // An empty needle is not a filter.
        assert!(matches_filter(&note, &filter("   "), None));
    }

    /// The editor never sees a `---`, which is what makes "the first keystroke
    /// landed above the block" unrepresentable rather than repaired. The split has
    /// to be lossless for that to be safe, so this is the property that matters:
    /// block + body is the document, byte for byte.
    #[test]
    fn a_note_splits_into_a_block_and_a_body_and_joins_back_byte_for_byte() {
        let note = "---\nid: 01KZ3J05WAFCNG61J8B2G95BAJ\ncreated: 2026-08-03T10:16:37+00:00\n---\n\nVault as a lens\n\nThe folder is the database.\n";

        let (block, body) = split_note(note);

        assert_eq!(
            block,
            "---\nid: 01KZ3J05WAFCNG61J8B2G95BAJ\ncreated: 2026-08-03T10:16:37+00:00\n---\n"
        );
        // The body opens with the blank line the writer put between the two, and
        // holds no fence at all: there is nothing for a caret to land in front of.
        assert_eq!(body, "\nVault as a lens\n\nThe folder is the database.\n");
        assert!(!body.contains("---"), "the body carries no fence: {body:?}");
        assert_eq!(join_note(block, body), note);
    }

    /// A note that never had a block is all body, and joining adds nothing. Writing
    /// one back must not invent frontmatter it did not have — `set_in` is the only
    /// thing that adds a block, and only for a key it was asked to write.
    #[test]
    fn a_note_without_a_block_is_all_body() {
        let (block, body) = split_note("just a body\n");
        assert_eq!(block, "");
        assert_eq!(body, "just a body\n");
        assert_eq!(join_note(block, body), "just a body\n");
    }

    /// The block the editor cannot reach is the block a save keeps. A body that
    /// opens with a fence of its own — a thematic break the user typed on line one —
    /// is body text, and joining must not let it pass for frontmatter that replaces
    /// keeper's.
    #[test]
    fn joining_keeps_keepers_block_even_when_the_body_starts_with_a_fence() {
        let block = "---\nid: 01AAA\n---\n";
        let joined = join_note(block, "---\nnot frontmatter\n");

        assert!(
            joined.starts_with(block),
            "keeper's block leads: {joined:?}"
        );
        let (kept, body) = split_note(&joined);
        assert_eq!(
            kept, block,
            "and it is still the frontmatter after the join"
        );
        assert_eq!(body, "---\nnot frontmatter\n");
    }

    /// A template's `{{cursor}}` is the only caret hint there is, it survives the
    /// hop from create to open, and it is spent once.
    #[test]
    fn a_caret_hint_is_taken_exactly_once() {
        let note = "01KCARET0000000000000000TEST";
        assert_eq!(take_caret(note), None, "no hint before a create");

        remember_caret(note, 17);
        assert_eq!(take_caret(note), Some(17));
        assert_eq!(
            take_caret(note),
            None,
            "reopening the note gets the ordinary caret at the end"
        );
    }

    #[test]
    fn pinned_notes_sort_first_and_the_order_is_total() {
        let mut pinned = entry("old.md", "old");
        pinned.flags.push("pinned".to_owned());
        pinned.updated_ms = 1;
        let mut fresh = entry("new.md", "new");
        fresh.updated_ms = 100;
        assert_eq!(list_order(&pinned, &fresh), std::cmp::Ordering::Less);
        // Equal on every other key, path breaks the tie, so a repaint never
        // reshuffles two notes that changed in the same millisecond.
        let a = entry("a.md", "same");
        let b = entry("b.md", "same");
        assert_eq!(list_order(&a, &b), std::cmp::Ordering::Less);
        assert_eq!(list_order(&b, &a), std::cmp::Ordering::Greater);
    }

    #[test]
    fn the_first_batch_is_a_reset_and_later_ones_are_diffs() {
        let rows = vec![row("1", "one"), row("2", "two")];
        let first = fingerprints(&rows);
        // An unchanged window owes nothing.
        assert!(diff_ops(&first, &first, &rows).is_empty());

        // One row's title changed: one upsert, at its index.
        let edited = vec![row("1", "one"), row("2", "TWO")];
        let next = fingerprints(&edited);
        let ops = diff_ops(&first, &next, &edited);
        assert_eq!(ops.len(), 1);
        match &ops[0] {
            NoteListOp::Upsert { index, row } => {
                assert_eq!(*index, 1);
                assert_eq!(row.title, "TWO");
            }
            other => panic!("expected an upsert, got {other:?}"),
        }

        // A row left the window: one remove.
        let shorter = vec![row("1", "one")];
        let ops = diff_ops(&first, &fingerprints(&shorter), &shorter);
        assert!(ops
            .iter()
            .any(|op| matches!(op, NoteListOp::Remove { id } if id == "2")));
    }

    #[test]
    fn a_cadence_below_the_floor_is_clamped_rather_than_refused() {
        // `SyncProfile::validate` refuses an out-of-range cadence, so a form that
        // sent 100 ms would fail the save instead of getting the floor.
        let clamped = apply_settings(
            NotesConfig::default(),
            &NoteVaultSettingsReq {
                subfolder: None,
                journal_template: None,
                default_template: None,
                cadence: Some(NoteCadenceVm {
                    commit_idle_ms: 100,
                    push_interval_ms: 1_000,
                    push_on_blur: false,
                }),
            },
        );
        assert_eq!(clamped.cadence.commit_idle_ms, MIN_COMMIT_IDLE_MS);
        assert_eq!(clamped.cadence.push_interval_ms, MIN_PUSH_INTERVAL_MS);
        assert!(!clamped.cadence.push_on_blur);
    }

    #[test]
    fn an_unexpressed_setting_leaves_the_stored_value_alone() {
        // AD-34-9: a form that does not show a knob must not be able to move it.
        let stored = NotesConfig {
            subfolder: "second-brain".to_owned(),
            journal_template: "daily/{yyyy}-{mm}-{dd}.md".to_owned(),
            default_template: Some("note.md".to_owned()),
            ..Default::default()
        };
        let after = apply_settings(
            stored.clone(),
            &NoteVaultSettingsReq {
                subfolder: None,
                journal_template: None,
                default_template: None,
                cadence: None,
            },
        );
        assert_eq!(after.subfolder, stored.subfolder);
        assert_eq!(after.journal_template, stored.journal_template);
        assert_eq!(after.default_template, stored.default_template);
        assert_eq!(after.cadence, stored.cadence);

        // An explicit empty default template clears it — that IS an expression.
        let cleared = apply_settings(
            stored,
            &NoteVaultSettingsReq {
                subfolder: None,
                journal_template: None,
                default_template: Some(String::new()),
                cadence: None,
            },
        );
        assert_eq!(cleared.default_template, None);
    }

    #[test]
    fn a_space_definition_reads_the_one_level_form_and_the_nested_one() {
        let flat = concat!(
            "---\n",
            "id: 01J8ZQ4M7T5R9V3XK2B6C0DFGH\n",
            "keeper:\n",
            "  space: 'tag:project/keeper -tag:archive'\n",
            "  sort: modified desc\n",
            "  limit: 500\n",
            "---\n",
            "\n# Active keeper work\n"
        );
        let def = space_def(&entry("spaces/active.md", "Active keeper work"), flat);
        assert_eq!(def.query, "tag:project/keeper -tag:archive");
        assert_eq!(def.sort, "modified desc");
        assert_eq!(def.limit, 500);

        // A note with no definition at all is a space with an empty query, not a
        // parse failure — an empty query is what the UI shows as "not defined
        // yet".
        let bare = space_def(&entry("spaces/new.md", "New"), "---\nid: x\n---\n");
        assert!(bare.query.is_empty());
    }

    #[test]
    fn a_frontmatter_limit_is_clamped_into_the_window_the_list_will_honour() {
        assert_eq!(clamp_limit(200.0), 200);
        assert_eq!(clamp_limit(100_000.0), MAX_LIMIT);
        assert_eq!(clamp_limit(0.0), MAX_LIMIT);
        assert_eq!(clamp_limit(-5.0), MAX_LIMIT);
    }

    /// Dirtiness is a body-against-body question. `base` is the whole document,
    /// `mine` is the editor's buffer, and the block between them is not the editor's
    /// to change — so a buffer holding exactly the delivered body is clean, block or
    /// no block.
    #[test]
    fn a_dirty_buffer_is_the_one_that_differs_from_what_we_delivered() {
        let mut state = BodyState {
            rel: "a.md".to_owned(),
            base: "---\nid: 01AAA\n---\nhello".to_owned(),
            rev: "5-x".to_owned(),
            mine: None,
        };
        assert!(!state.is_dirty(), "no report yet is not dirty");
        state.mine = Some("hello".to_owned());
        assert!(
            !state.is_dirty(),
            "a buffer identical to the body we delivered is not dirty"
        );
        // The whole document is NOT what the editor holds: a buffer that somehow
        // carried the block would be a buffer that had diverged.
        state.mine = Some(state.base.clone());
        assert!(state.is_dirty());
        state.mine = Some("hello world".to_owned());
        assert!(state.is_dirty());
    }
}
