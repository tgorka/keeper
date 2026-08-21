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
use std::path::{Path, PathBuf};
use std::sync::{Arc, LazyLock, Mutex, MutexGuard};
use std::time::Duration;

use keeper_core::archive::recordings_fts::kind_for_file_name;
use keeper_core::notes::default_spaces::{self, SPACES_DIR};
use keeper_core::notes::embed::{self, NoteEmbedPathVm, NoteEmbedVm};
use keeper_core::notes::frontmatter::{FieldValue, Frontmatter};
use keeper_core::notes::index::{IndexEntry, IndexSnapshot, TagTerms};
use keeper_core::notes::template_update::{
    self, TemplateUpdateAppliedVm, TemplateUpdateApplyReq, TemplateUpdateOfferVm,
    TemplateUpdateResultVm,
};
use keeper_core::notes::vm::{
    NoteAttachSourceVm, NoteAttachTargetVm, NoteAttachmentVm, NoteBodyBatch, NoteBodyVm,
    NoteChangeBatch, NoteConflictChoiceReq, NoteConflictVm, NoteCreateReq, NoteCreateVm, NoteCsvVm,
    NoteDeletePlanVm, NoteDiffVm, NoteFlag, NoteFolderVm, NoteGalleryItemVm, NoteGalleryVm,
    NoteIndexProgressVm, NoteLinkTargetVm, NoteListOp, NoteListVm, NoteQueryCheckVm, NoteQueryReq,
    NoteRefVm, NoteRevisionVm, NoteRowVm, NoteSearchBatch, NoteSearchHitVm, NoteSearchReq,
    NoteSpaceReq, NoteSpaceTermsVm, NoteSpaceVm, NoteTagNodeVm, NoteTagTreeVm, NoteTemplateVm,
    NoteVaultSettingsReq, NoteVaultVm, NoteWriteVm,
};
use keeper_core::notes::{
    attach, counts, csv, naming, order, query, search, seed, sort, tags, templates, widget,
    NotesError,
};
use keeper_core::vm::{
    ExportReceiptVm, IpcError, IpcErrorCode, RecordingNoteTargetKind, TagVocabularyEntryVm,
    TagVocabularyVm,
};
use keeper_sync::browse;
use keeper_sync::profile::{NotesCadence, NotesConfig};
use tauri::ipc::Channel;
use tauri::{AppHandle, Emitter, Manager, State};

use crate::ipc::AppState;
use crate::notes_vault::{self, HeadRevision, Vault, ATTACHMENTS_DIR};

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

/// The longest an icon name may be before it is treated as noise rather than as
/// an icon. Lucide's longest name is well under this.
const MAX_ICON_BYTES: usize = 64;

// `TEMPLATES_DIR` used to be declared here, beside a comment explaining why
// `spaces/` was not. Story 44.7 gave the template seeder the same shape as the
// space seeder, so the constant moved to `keeper-core` for the same reason
// `SPACES_DIR` lives there: the seeder composes paths under it, and two
// constants spelling one directory is a rename waiting to half-land.

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
        // Straight through from the index, `source` included: the row shows the
        // number the sort used, so the reader can account for the ordering
        // (Story 44.5). Nothing is recomputed here — a second reading of the
        // note's `order` is a second chance to disagree with the sort.
        order: entry.order,
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
/// Index-only, deliberately: the free-text axis is
/// [`IndexEntry::matches_text`], which reads the title, the snippet, the path,
/// the tags and the note's own frontmatter values and never opens a file — that
/// is what keeps NFR-28's 100 ms list paint true. Full-body matching is
/// `notes_search`, which streams because it reads files.
///
/// Both content axes live in `keeper-core` rather than here — free text in
/// [`IndexEntry::matches_text`] and the tag chips in
/// [`IndexEntry::matches_tags`] — so what a chip selects is stated once, in the
/// crate that can be tested on any host (AD-55/AD-56). The tag terms are
/// normalised once per query rather than once per entry, which is why they
/// arrive already folded instead of being read out of `req`. This function keeps
/// only the axes that need the shell's own facts — the commit head behind
/// `origin:`.
fn matches_filter(
    entry: &IndexEntry,
    req: &NoteQueryReq,
    tags: &TagTerms,
    head: Option<&HeadRevision>,
) -> bool {
    if let Some(text) = req.text.as_ref() {
        if !entry.matches_text(text) {
            return false;
        }
    }
    if !entry.matches_tags(tags) {
        return false;
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

/// Case-fold for the two note-picking searches: the wikilink completion prefix
/// and Story 45.13's "which note should these files go into". Cheap on purpose
/// — a prefix is matched on every keystroke against every entry, and a picker's
/// list is a ranked guess rather than a promise about what "café" equals, which
/// is a promise `search::fold_str` makes for the surfaces that need it.
fn fold(value: &str) -> String {
    value.to_lowercase()
}

/// Build the windowed page for a query, and the counts behind it (Story 44.11).
///
/// Three numbers, and keeping them apart is the whole of this function's second
/// job:
///
/// - **matched** — every entry the lens admits. Taken over the whole snapshot,
///   before anything is dropped.
/// - **total** — how many the lens SELECTS: `matched`, capped by the space's
///   `keeper.limit` (see [`counts`]). This is what a count shows and what
///   pagination walks, so a caller cannot page past the cap into notes the
///   space declined.
/// - **the page** — `req.offset`/`req.limit`, how many rows this one read
///   carries over the wire. Never a count of anything, and after Story 44.10
///   never a count of what is rendered either.
fn project_list(
    platform: &dyn keeper_core::platform::Platform,
    vault: &Vault,
    req: &NoteQueryReq,
) -> Result<NoteListVm, IpcError> {
    let snapshot = notes_vault::snapshot(&vault.id)
        .ok_or_else(|| notes_error(NotesError::VaultUnknown(vault.id.clone())))?;
    let heads = notes_vault::heads(&vault.id).unwrap_or_default();

    // A space is the query DSL; everything else is the chip filter. A space also
    // brings its own ordering (Story 44.4) — the value that has sat in
    // `keeper.sort` since Story 37.4 and that nothing read until now — and its
    // own selection cap (Story 44.11), which is the neighbouring value 44.4
    // left where it was.
    let space = match req.space_id.as_ref() {
        Some(space_id) if !space_id.is_empty() => Some(space_lens(vault, &snapshot, space_id)?),
        _ => None,
    };
    let (mut matched, ordering, cap, space_name): (
        Vec<&IndexEntry>,
        Option<sort::SpaceSort>,
        Option<u32>,
        String,
    ) = match space {
        Some(lens) => {
            let mut parsed = lens.query;
            // Bound to this snapshot so `backlink:` and title-resolved `link:`
            // can be answered at all; the binding is per snapshot revision.
            query::bind_index(&mut parsed, &snapshot);
            let now_ms = notes_vault::local_now_ms();
            (
                snapshot
                    .entries()
                    .iter()
                    .filter(|entry| {
                        let mut body = body_reader(vault, &entry.path);
                        query::eval(&parsed, entry, &mut body, now_ms)
                    })
                    .collect(),
                Some(lens.ordering),
                lens.limit,
                lens.name,
            )
        }
        None => {
            // Folded once for the whole walk, not once per entry (NFR-28).
            let tags = TagTerms::new(&req.tags);
            (
                snapshot
                    .entries()
                    .iter()
                    .filter(|entry| matches_filter(entry, req, &tags, heads.get(&entry.path)))
                    .collect(),
                None,
                // The plain lens has no cap and never will: `keeper.limit` is a
                // property of a space note, and there is no file to read one
                // out of when nobody is in a space.
                None,
                String::new(),
            )
        }
    };
    // Inside a space the sort is the WHOLE ordering: pins do not float, because
    // a sort with a hidden first term is not the sort the user chose (AD-81).
    // The plain list is unchanged and still puts pinned first — that rule was
    // never a space's, and taking it away from the default lens would be a
    // different story's decision.
    match ordering {
        Some(ordering) => matched.sort_by(|a, b| sort::compare(ordering, a, b)),
        None => matched.sort_by(|a, b| list_order(a, b)),
    }

    // The cap is applied AFTER the ordering, which is what makes "the twenty
    // most recent" mean what it says: sort first, then keep the first twenty.
    // Capping the unsorted set would keep twenty arbitrary matches and change
    // which ones every time the index walked in a different order.
    let selection = counts::select(matched.len(), cap);
    // A cap that declined notes is the one way this function does nothing on
    // purpose, and a decline nobody is told about is indistinguishable from a
    // vault that simply has fewer notes in it. `debug!` reaches no log the
    // shipped app writes (DW-162), so the level is `keeper-core`'s — asserted
    // there against the floor the app's own filter sets — and this call site
    // matches over it rather than picking one, the shape `seed_default_spaces`
    // uses and for the reason it learned.
    if let Some((level, message)) = selection.report(&space_name) {
        if level <= tracing::Level::WARN {
            tracing::warn!(vault = %vault.id, "notes: {message}");
        } else {
            tracing::info!(vault = %vault.id, "notes: {message}");
        }
    }
    // The page size is the caller's, bounded so nobody can ask for 10 000 rows
    // of JSON and undo AD-58. WHICH rows that page names is `counts::page`'s,
    // in the crate that can prove it: the cap has to bind before the offset, or
    // a second read walks straight into the notes the space declined.
    let size = if req.limit == 0 {
        DEFAULT_LIMIT
    } else {
        req.limit.min(MAX_LIMIT)
    };
    let span = counts::page(selection, req.offset, size);
    let offset = u32::try_from(span.start).unwrap_or(u32::MAX);
    let window: Vec<&IndexEntry> = matched
        .into_iter()
        .take(span.end)
        .skip(span.start)
        .collect();
    Ok(NoteListVm {
        rows: rows_of(platform, &vault.id, &window),
        total: selection.total,
        matched: selection.matched,
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
    /// `keeper.sort`, exactly as stored — never the parsed ordering. What the
    /// list runs comes from [`sort::read`], and what the file says has to
    /// survive a round trip through the editor unrewritten.
    sort: String,
    /// `keeper.limit` — the most notes this space holds, or `None` for a space
    /// that sets no cap (Story 44.11). Read through
    /// [`counts::read_limit`], because "unset" and "capped at the page size"
    /// used to be the same value and that conflation is what made the editor
    /// stamp `limit: 500` into files that never had the key.
    limit: Option<u32>,
    icon: Option<String>,
    /// `keeper.order` — where the space sits in the rail (Story 44.4).
    order: f64,
    /// `keeper.default` — which seeded default this space is, when it is one
    /// (Story 44.3). Never written by the editor; carried through a save so
    /// editing a default does not quietly demote it.
    default_key: Option<String>,
    /// `keeper.template` — the template notes created in this space start from
    /// (Story 44.7, FR-162). A vault-relative path, or a bare name inside the
    /// template directory. `None` when the space hands out no template, which
    /// is the ordinary case.
    template: Option<String>,
    /// `keeper.folder` — where a note created in this space is written
    /// (Story 44.13). `None` leaves the destination to the query, which is what
    /// a `path:` space has always done and what a `tag:` space never could.
    folder: Option<String>,
    /// What keeper could not read in the two presentation keys, already worded
    /// (Story 44.4). Empty for a file keeper understood entirely.
    warnings: Vec<String>,
}

/// Read the space definition out of a space note's frontmatter.
///
/// The definition lives under the reserved `keeper:` namespace, which the
/// frontmatter parser accepts to **one** level of nesting — so it is
/// `keeper: { space: "<query>", sort: …, limit: … }`, not the deeper
/// `keeper.space.query` the architecture companion's example shows. A nested map
/// is still accepted where the parser hands one back, so the deeper form starts
/// working the day the subset grows, and neither spelling is a parse error.
///
/// `sort` and `order` are read through `keeper-core` rather than interpreted
/// here, and both of them can fail *visibly*: this file does not build on Linux
/// (AD-55/AD-56), so the rule that decides what `sort: bananas` does and the
/// sentence the sidebar shows about it are both somewhere they can be proved.
/// All this does is hand over the text and collect what comes back.
fn space_def(entry: &IndexEntry, source: &str) -> SpaceDef {
    let (fm, _) = Frontmatter::parse(source);
    let mut def = SpaceDef {
        id: entry.id.clone(),
        name: entry.title.clone(),
        query: String::new(),
        sort: String::new(),
        limit: None,
        icon: None,
        order: sort::DEFAULT_SPACE_ORDER,
        default_key: None,
        template: None,
        folder: None,
        warnings: Vec::new(),
    };
    let Some(FieldValue::Map(pairs)) = fm.get("keeper") else {
        return def;
    };
    for (key, value) in pairs {
        match (key.as_str(), value) {
            ("space", FieldValue::Str(query)) => def.query = query.clone(),
            // No `("space", FieldValue::Map(_))` arm: `Frontmatter` models one
            // level of nesting and says so at its `lookahead` — a second level
            // is claimed as opaque, never as a map. An arm for it would be code
            // that cannot run, which is worse than an absent feature because it
            // reads as support.
            ("sort", FieldValue::Str(stored)) => def.sort = stored.clone(),
            ("limit", FieldValue::Num(limit)) => def.limit = counts::read_limit(*limit),
            ("icon", FieldValue::Str(icon)) => def.icon = space_icon(icon),
            // Kept as the stored text rather than the validated directory: the
            // editor must show what the file says, and `seed::folder_dest` is
            // the one place that decides whether it can be written to.
            ("folder", FieldValue::Str(dir)) => {
                def.folder = Some(dir.clone()).filter(|d| !d.trim().is_empty());
            }
            // Matched on the key alone and flattened to text, so `order: 2`,
            // `order: "2"` and `order: [a, b]` all reach one reader instead of
            // the first two working and the third being silently absent.
            ("order", value) => {
                let read = sort::read_order(&value.index_string());
                def.order = read.order;
                def.warnings.extend(read.warning);
            }
            _ => {}
        }
    }
    def.warnings.extend(sort::read(&def.sort).warning);
    // The marker is read through `keeper-core`'s one rule rather than a sixth
    // arm here, so the seeder — which reads notes off disk before the index
    // exists — and this cannot disagree about what a default is.
    def.default_key = default_spaces::default_key(pairs);
    // Same reasoning as `default_key` above, and the same shape: the rule for
    // what `keeper.template` means lives beside the code that applies it, so the
    // space editor and the create path cannot disagree about which value is a
    // template and which is a cleared field.
    def.template = templates::space_default_template(&fm);
    def
}

/// The icon name a space carries, or `None` when the key holds nothing usable.
///
/// Trimmed, length-capped, and otherwise passed through **unread**. The fixed
/// set is the editor's, because the set is a set of drawings and this crate has
/// no drawings — so a name Rust does not recognise is not an error here, and a
/// name the editor does not recognise is not rewritten there. A space whose
/// icon was renamed out of the set keeps the name it was given on disk and
/// draws the fallback glyph; the alternative is keeper silently editing a value
/// it did not understand, which is the same mistake as rewriting a query term
/// it could not parse.
///
/// The cap is the one judgement made here: frontmatter is agent-writable and an
/// icon name is a short identifier, so a value this long is a bug rather than
/// an icon, and it has no business reaching a `<title>` in the sidebar.
fn space_icon(raw: &str) -> Option<String> {
    let trimmed = raw.trim();
    if trimmed.is_empty() || trimmed.len() > MAX_ICON_BYTES {
        return None;
    }
    Some(trimmed.to_owned())
}

/// Everything one space's lens decides, by note id: what it selects, how it
/// orders what it selected, and how much of that it keeps.
///
/// One read of the note rather than three. They were separable while the sort
/// and the limit were values nobody applied; now that the list runs both,
/// fetching the query here and either of the others somewhere else would be
/// reads that can disagree if the note changes between them.
struct SpaceLens {
    query: query::Query,
    ordering: sort::SpaceSort,
    /// The space's `keeper.limit`, or `None` for a space that sets no cap.
    limit: Option<u32>,
    /// The space's title, for the one log line a cap that bites has to write.
    name: String,
}

/// The reserved id of the one space that has no note behind it.
///
/// Every other space in the rail is a markdown file somebody wrote. This one is
/// composed on demand from all the others, so it has no path, no frontmatter and
/// nothing to edit or delete — the rail hides those controls for exactly this id.
/// The colon keeps it out of the id space a note can occupy.
pub const UNCATEGORIZED_SPACE_ID: &str = "keeper:uncategorized";

/// The query that selects the notes no space claims.
///
/// It is the negation of every space's query, joined by the implicit AND:
/// `-(tag:inbox) -(path:journal/**) …`. The point of composing it in the query
/// language rather than evaluating membership separately is that there is then
/// only one engine: whatever a space means today, "in no space" means exactly
/// its complement, including the parts of the DSL this function has never heard
/// of.
///
/// Two kinds of space are skipped, and skipping them is what makes the answer
/// right rather than merely defensible:
///
/// * one whose query does not parse — it selects nothing, so it claims nothing,
///   so subtracting it would subtract nothing while risking a composed query
///   that does not parse either;
/// * one whose query nests so deeply that wrapping it in one more bracket would
///   pass [`query::MAX_DEPTH`]. Dropping it makes this space show a few notes it
///   might not have; keeping it would make the whole row fail to parse and show
///   none. A row that is slightly too generous still answers the question.
///
/// An empty vault, or one with no spaces at all, composes to the empty string —
/// which the parser reads as "everything", and everything is the correct answer
/// when nothing has been claimed.
fn uncategorized_query(vault: &Vault, snapshot: &IndexSnapshot) -> String {
    compose_uncategorized(
        snapshot
            .entries()
            .iter()
            .filter(|e| has_flag(e, "space"))
            .map(|entry| {
                let source = notes_vault::read_note(vault, &entry.path).unwrap_or_default();
                space_def(entry, &source).query
            }),
    )
}

/// The composition on its own, away from the vault it usually reads from.
///
/// Split out because this is where every decision lives — which spaces count,
/// how they are joined, what an empty list means — and because a function that
/// needs an index snapshot to be asked one question is a function nobody tests.
fn compose_uncategorized(queries: impl Iterator<Item = String>) -> String {
    queries
        .filter_map(|q| {
            let wrapped = format!("-({q})");
            (query::parse(&q).is_ok() && query::parse(&wrapped).is_ok()).then_some(wrapped)
        })
        .collect::<Vec<_>>()
        .join(" ")
}

/// Read one space's whole lens off its note.
fn space_lens(
    vault: &Vault,
    snapshot: &IndexSnapshot,
    space_id: &str,
) -> Result<SpaceLens, IpcError> {
    if space_id == UNCATEGORIZED_SPACE_ID {
        let composed = uncategorized_query(vault, snapshot);
        // Composed from queries that were each parsed a moment ago, so a failure
        // here is a bug in the composition and not in anybody's file. Reported
        // as a query error rather than unwrapped, because a panic in the note
        // list is a worse answer than a sentence.
        let parsed = query::parse(&composed).map_err(|error| {
            notes_error(NotesError::Query {
                message: error.message,
                token_index: error.token_index,
            })
        })?;
        return Ok(SpaceLens {
            query: parsed,
            ordering: sort::read("").sort,
            limit: None,
            name: "Uncategorized".to_owned(),
        });
    }
    let entry = snapshot
        .by_id(space_id)
        .ok_or_else(|| notes_error(NotesError::NotFound(space_id.to_owned())))?;
    let source = notes_vault::read_note(vault, &entry.path).map_err(notes_error)?;
    let def = space_def(entry, &source);
    let parsed = query::parse(&def.query).map_err(|error| {
        notes_error(NotesError::Query {
            message: error.message,
            token_index: error.token_index,
        })
    })?;
    // A sort keeper cannot read never fails the list — the space still selects
    // what it selects, and the row is already saying the file and the ordering
    // disagree. Refusing here would turn one bad word in frontmatter into an
    // empty pane with an error in it.
    Ok(SpaceLens {
        query: parsed,
        ordering: sort::read(&def.sort).sort,
        limit: def.limit,
        name: def.name,
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
            // A folder keeper is not indexing has no config to read a capture
            // scaffold or tag out of, so both are absent rather than defaulted.
            capture_template: None,
            capture_tag: None,
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

/// What configuring `tag` as the capture tag would do to this vault's spaces
/// (Story 45.16, FR-193).
///
/// One finished sentence per space that would **stop** listing captured notes,
/// composed by [`seed::capture_tag_cost`] over that space's real stored query.
/// An empty list means nothing is displaced.
///
/// **Asked, not reasoned about.** 44.7 wrote down why its shipped templates
/// carry no tags of their own: Inbox is `is:untagged`, so a tag files its notes
/// out of the space that offered them. A capture tag is that hazard aimed at
/// every thought the user captures, and the honest way to show the cost is to
/// run the one evaluator over the note a capture would write — not to hardcode
/// a sentence about Inbox in the webview, which would be wrong the moment
/// somebody edits Inbox's query and silent for the space they wrote themselves
/// (AD-55, AD-58).
///
/// A *preview*: it takes the tag the form is holding rather than the one on
/// disk, so the answer arrives before Save rather than after it.
#[tauri::command]
pub async fn notes_capture_impact(
    vault_id: String,
    tag: Option<String>,
) -> Result<Vec<String>, IpcError> {
    let vault = vault_of(&vault_id)?;
    let snapshot = notes_vault::snapshot(&vault_id)
        .ok_or_else(|| notes_error(NotesError::VaultUnknown(vault_id)))?;
    // The canonical tag, through the same rule the save applies, so the preview
    // answers about the tag that would actually be stored — `#Quick Capture`
    // and `quick-capture` must not preview differently from each other.
    let tag = tag.as_deref().and_then(seed::capture_tag);
    let stamp = now_local();
    let now_ms = notes_vault::local_now_ms();
    Ok(snapshot
        .entries()
        .iter()
        .filter(|entry| has_flag(entry, "space"))
        .filter_map(|entry| {
            let source = notes_vault::read_note(&vault, &entry.path).unwrap_or_default();
            let def = space_def(entry, &source);
            // Which spaces are worth naming is `keeper-core`'s decision, not
            // this command's: it is the difference between a surface that names
            // the one space you are about to lose and one that lists your whole
            // rail. This function reads the spaces and iterates; it decides
            // nothing (AD-55, AD-56).
            seed::capture_tag_cost(&def.name, &def.query, tag.as_deref(), &stamp, now_ms)
        })
        .collect())
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
    if let Some(template) = req.capture_template.as_ref() {
        let trimmed = template.trim();
        config.capture_template = (!trimmed.is_empty()).then(|| trimmed.to_owned());
    }
    // Folded on the way in, never on the way out, so the stored value IS the
    // tag the note will carry and the form cannot show a second spelling of it
    // (AD-34-8). `capture_tag` also refuses the `template` marker, which is why
    // this is a call rather than a trim: a capture tagged `template` would make
    // every captured thought a scaffold (AD-82).
    if let Some(tag) = req.capture_tag.as_ref() {
        config.capture_tag = seed::capture_tag(tag);
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

/// What a gallery says about a folder that is simply not in the vault.
///
/// Worded here rather than in the webview, on this module's standing rule that
/// a finished sentence about the filesystem is composed where the filesystem
/// was asked. It names the two things the reader can act on — the block's own
/// text and the folder's absence — and claims nothing about why.
const GALLERY_MISSING_FOLDER: &str =
    "this folder is not in the vault, so there is nothing to show; check the folder named on the \
     gallery's first line";

/// One folder of the vault, for a note's gallery block (Story 44.15, FR-171,
/// AD-84, AD-65).
///
/// **`notes_tree` cannot answer this and must not learn to.** That command
/// reads the index, and the index holds notes — a folder of four hundred
/// photographs is invisible to it. This reads the disk, once, through
/// [`keeper_sync::browse`], which is the repo's one directory reader: the
/// lexical containment test, the canonicalizing one behind it, the built-in
/// noise filter, the cap and the stable order all come from there rather than
/// from a second `read_dir` written next to it.
///
/// **The frontend never joins a root and a subpath.** `folder` is the text of
/// the block's own callout title; `browse_root` resolves it against the vault
/// root and gets to say no. Each item's `keeper-note://` URL is composed here
/// too, so no path arithmetic happens in the webview at all (AD-65).
///
/// **Nothing is filtered.** Every entry crosses with the kind the one
/// classifier gave it (AD-73), including the ones a gallery will not show. The
/// gallery's rule — media tiles, everything else skipped and counted — belongs
/// to the surface that renders it, and is asserted where that surface's tests
/// run.
///
/// **A folder that cannot be listed is not an error.** A missing folder, an
/// unreadable one and a path that escapes the vault all come back as a
/// `problem` sentence inside a normal reply, because the block has to render
/// something and a rejected promise gives a widget nothing to say. Each is
/// logged at INFO: this command declining to list a folder is a thing the
/// user's log must show, and `debug!` does not reach it (DW-162).
#[tauri::command]
pub async fn notes_gallery(vault_id: String, folder: String) -> Result<NoteGalleryVm, IpcError> {
    let vault = vault_of(&vault_id)?;
    let listing = {
        let root = vault.root.clone();
        let excludes = Arc::clone(&vault.excludes);
        let folder = folder.clone();
        // A directory of hundreds on a network share can take a long time to
        // stat, and the async runtime carries every other vault's watcher.
        tokio::task::spawn_blocking(move || {
            // `Unavailable` and never an empty `Known`: a gallery asks the
            // engine nothing, and an empty known list would mark every photo
            // `Synced` — a claim nobody here is entitled to make.
            browse::browse_root(&root, &folder, &excludes, &browse::PendingView::Unavailable)
        })
        .await
        .map_err(|error| {
            notes_error(NotesError::Name(format!(
                "the gallery folder could not be read: {error}"
            )))
        })?
    };

    let listed = match listing {
        Ok(keeper_sync::browse::BrowseListing::Listed(dir)) => dir,
        Ok(other) => {
            // `Missing` is the only other variant reachable here: `browse_root`
            // asks no volume question, because a vault is not removable media.
            tracing::info!(
                vault = %vault_id,
                folder = %folder,
                listing = ?other,
                "gallery: the folder was not listed",
            );
            return Ok(NoteGalleryVm {
                folder,
                items: Vec::new(),
                truncated: false,
                problem: Some(GALLERY_MISSING_FOLDER.to_owned()),
            });
        }
        Err(refusal) => {
            tracing::info!(
                vault = %vault_id,
                folder = %folder,
                "gallery: {refusal}",
            );
            return Ok(NoteGalleryVm {
                folder,
                items: Vec::new(),
                truncated: false,
                problem: Some(refusal.to_string()),
            });
        }
    };

    let items = listed
        .entries
        .into_iter()
        .map(|entry| {
            // A directory is known from the dirent that listed it; every other
            // kind is the extension table's answer and nobody else's (AD-73).
            let kind = if entry.is_dir {
                RecordingNoteTargetKind::Folder
            } else {
                kind_for_file_name(&entry.name)
            };
            NoteGalleryItemVm {
                url: matches!(
                    kind,
                    RecordingNoteTargetKind::Video
                        | RecordingNoteTargetKind::Image
                        | RecordingNoteTargetKind::Audio
                )
                .then(|| notes_vault::asset_url(&vault_id, &entry.relative_path)),
                name: entry.name,
                rel_path: entry.relative_path,
                kind,
            }
        })
        .collect();

    Ok(NoteGalleryVm {
        folder,
        items,
        truncated: listed.truncated,
        problem: None,
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
                sort_effective: sort::read(&def.sort).sort.canonical(),
                sort: def.sort,
                // Zero is the wire's "no cap" (Story 44.11) — the same value
                // the editor sends back for a space nobody capped, so the
                // round trip writes no `keeper.limit` key.
                limit: def.limit.unwrap_or(0),
                icon: def.icon,
                default_key: def.default_key,
                template: def.template,
                folder: def.folder,
                warnings: def.warnings,
                order: def.order,
                error,
            }
        })
        .collect();
    // The rail is ordered by what each space says, then by name — which is what
    // it sorted by before Story 44.4, so a vault nobody has positioned does not
    // move (FR-157).
    spaces.sort_by(|a, b| sort::rail_order((a.order, a.name.as_str()), (b.order, b.name.as_str())));
    // Composed, not read: the one row in this list with no file behind it.
    //
    // It goes last rather than into the sort, and that is deliberate. Its order
    // is not a position somebody chose and it must not compete with the ones
    // that are — a person who floats a space to the top of the rail has said
    // something, and a synthetic row outranking them would be keeper arguing.
    // The foot of the list is also where it reads best: everything above is a
    // place notes were put, and this is what is left.
    spaces.push(NoteSpaceVm {
        id: UNCATEGORIZED_SPACE_ID.to_owned(),
        name: "Uncategorized".to_owned(),
        query: uncategorized_query(&vault, &snapshot),
        sort_effective: sort::read("").sort.canonical(),
        sort: String::new(),
        limit: 0,
        icon: Some("shapes".to_owned()),
        default_key: None,
        template: None,
        folder: None,
        warnings: Vec::new(),
        order: 0.0,
        error: None,
    });
    Ok(spaces)
}

// ---------------------------------------------------------------------------
// Default spaces (Story 44.3, FR-156, AD-79)
// ---------------------------------------------------------------------------

/// The vault directory, as `keeper-core`'s seeder reads and writes it.
///
/// **Every body here is one call.** That is the point: Story 44.3 first shipped
/// with the ledger read, the directory read, the error classification and the
/// write loop all in this file, all unbuildable on Linux, and it went green on
/// two hosts and did nothing on the owner's vault. The run is
/// [`default_spaces::seed`]'s now, exercised against a real directory in a crate
/// that builds everywhere, and what is left unprovable here is four one-liners.
struct VaultSeedFiles<'a> {
    vault: &'a Vault,
}

impl default_spaces::SeedVault for VaultSeedFiles<'_> {
    fn read(&self, rel: &str) -> std::io::Result<String> {
        // `contained` refuses a path that leaves the vault. Its refusal is not
        // an absence, so it must not arrive at the seeder as `NotFound` — that
        // is the shape of the bug this whole rewrite exists to make impossible.
        let path = notes_vault::contained(self.vault, rel)
            .map_err(|error| std::io::Error::other(error.to_string()))?;
        std::fs::read_to_string(path)
    }

    fn list(&self, rel_dir: &str) -> std::io::Result<Vec<String>> {
        let path = notes_vault::contained(self.vault, rel_dir)
            .map_err(|error| std::io::Error::other(error.to_string()))?;
        let mut out = Vec::new();
        for entry in std::fs::read_dir(path)? {
            out.push(entry?.file_name().to_string_lossy().into_owned());
        }
        Ok(out)
    }

    fn write(&mut self, rel: &str, text: &str) -> std::io::Result<()> {
        // A space or a template is a note and is announced to the reconciler;
        // a ledger is not one, and telling the index to re-read a `.json` it
        // never collects would be a lie about what changed. Both ledgers, or
        // the template one would be announced as a note that does not exist.
        let ledger = rel == default_spaces::LEDGER_REL || rel == templates::TEMPLATE_LEDGER_REL;
        let result = if ledger {
            notes_vault::write_vault_file(self.vault, rel, text)
        } else {
            notes_vault::write_note(self.vault, rel, text)
        };
        result.map_err(|error| std::io::Error::other(error.to_string()))
    }

    fn new_id(&mut self) -> String {
        crate::sync_ipc::new_ulid()
    }

    fn now_local(&self) -> String {
        now_local()
    }

    fn today(&self) -> String {
        today()
    }
}

/// Seed this vault's default spaces, and **say what happened** either way.
///
/// Called from `notes_vault::refresh` for every registered vault — not only a
/// newly registered one. The run is idempotent by construction (it plans against
/// what is on disk), so restricting it to first registration bought one
/// directory listing and cost a failure mode nobody could see: a vault that
/// registered while a read was failing never got another chance in that process.
///
/// The frontend does not drive this. A vault reached only from the tray or the
/// capture window has to be seeded too.
///
/// **Four outcomes, four log lines, and none of them below `INFO`.** The level
/// is [`default_spaces::SeedOutcome::report`]'s, not this call site's, because
/// this call site got it wrong once: the second attempt logged the ordinary
/// outcome at `debug!`, and `debug_log::init` installs `EnvFilter::new("info")`
/// with no `RUST_LOG` anywhere in the macOS app — so a run that did exactly what
/// it should still produced a blank log and a third field report. A refusal is
/// `warn` and names the file and the errno.
pub fn seed_default_spaces(vault: &Vault) {
    let outcome = default_spaces::seed(
        &mut VaultSeedFiles { vault },
        default_spaces::SeedMode::FirstRun,
    );
    // `tracing`'s macros take a const level, so the runtime choice is a match
    // over two arms rather than one `event!`. The *choice* is `keeper-core`'s
    // and is asserted there against the floor the app's own filter sets.
    let (level, message) = outcome.report();
    if level <= tracing::Level::WARN {
        tracing::warn!(vault = %vault.id, "notes: {message}");
    } else {
        tracing::info!(vault = %vault.id, "notes: {message}");
    }
}

/// Seed this vault's default templates, and **say what happened** either way
/// (Story 44.7, FR-161).
///
/// Same port, same mode, same call site and the same rule about levels as
/// [`seed_default_spaces`] — a template lands in somebody's real vault exactly
/// as a space does, so it gets the same care. What differs is the ledger, the
/// three notes and the wording, all of which are `keeper-core`'s.
///
/// Its own ledger and its own run rather than a fourth and fifth entry in the
/// spaces seed: a vault seeded by the build before this one has the spaces
/// ledger and no templates ledger, and that state has to read as "offer the
/// templates". One ledger could not say both.
pub fn seed_default_templates(vault: &Vault) {
    let outcome = templates::seed_templates(
        &mut VaultSeedFiles { vault },
        default_spaces::SeedMode::FirstRun,
    );
    // The level is `keeper-core`'s and is asserted there against the floor the
    // app's own filter sets, because `debug!` reaches no log the shipped app
    // writes (DW-162) and a seed that declined must never be invisible.
    let (level, message) = templates::report_template_seed(&outcome);
    if level <= tracing::Level::WARN {
        tracing::warn!(vault = %vault.id, "notes: {message}");
    } else {
        tracing::info!(vault = %vault.id, "notes: {message}");
    }
}

/// Re-create the default templates this vault is missing (FR-161).
///
/// The user asking, so the ledger does not get a vote — the twin of
/// `notes_spaces_restore_defaults`, and for the same reason: the ledger exists
/// to stop keeper acting on its own, and this is not keeper acting on its own.
#[tauri::command]
pub async fn notes_templates_restore_defaults(vault_id: String) -> Result<u32, IpcError> {
    let vault = vault_of(&vault_id)?;
    let outcome = templates::seed_templates(
        &mut VaultSeedFiles { vault: &vault },
        default_spaces::SeedMode::Restore,
    );
    match outcome {
        default_spaces::SeedOutcome::Wrote(written) => {
            Ok(u32::try_from(written.len()).unwrap_or(u32::MAX))
        }
        default_spaces::SeedOutcome::AlreadySatisfied => Ok(0),
        default_spaces::SeedOutcome::Blocked(why) => Err(notes_error(NotesError::Name(why))),
        default_spaces::SeedOutcome::Stopped { reason, .. } => {
            Err(notes_error(NotesError::Name(reason)))
        }
    }
}

/// Re-create the default spaces this vault is missing (FR-156).
///
/// The user asking, so the ledger does not veto it — and an unreadable ledger
/// does not either, because they are looking at the rail and repairing it is
/// what they pressed. It still only fills holes: a default that is there is not
/// rewritten, and a space of the user's own that already carries a default's
/// name stands that default down exactly as it does on the first run.
///
/// Returns how many notes were written, so the surface can say "nothing was
/// missing" rather than flashing a success at a no-op. A refusal is an error
/// here rather than a log line — a person is waiting for an answer.
#[tauri::command]
pub async fn notes_spaces_restore_defaults(vault_id: String) -> Result<u32, IpcError> {
    let vault = vault_of(&vault_id)?;
    let outcome = default_spaces::seed(
        &mut VaultSeedFiles { vault: &vault },
        default_spaces::SeedMode::Restore,
    );
    match outcome {
        default_spaces::SeedOutcome::Wrote(written) => {
            Ok(u32::try_from(written.len()).unwrap_or(u32::MAX))
        }
        default_spaces::SeedOutcome::AlreadySatisfied => Ok(0),
        default_spaces::SeedOutcome::Blocked(why) => Err(notes_error(NotesError::Name(why))),
        default_spaces::SeedOutcome::Stopped { reason, .. } => {
            Err(notes_error(NotesError::Name(reason)))
        }
    }
}

/// Create or update a space note (FR-105, FR-149).
///
/// The one write behind the space editor, so a rename, an icon and a set of
/// terms land together or not at all — three commands would leave a space
/// renamed but still carrying the terms the user just deleted if the second one
/// failed.
#[tauri::command]
pub async fn notes_space_save(
    vault_id: String,
    space: NoteSpaceReq,
) -> Result<NoteRefVm, IpcError> {
    let vault = vault_of(&vault_id)?;
    // Refuse a broken query at the edge: a space is a surface people run bulk
    // actions from, so storing one that matches nothing silently is worse than
    // saying no. An empty query is one of those failures — `parse` rejects it
    // rather than reading it as "everything" — which is the backstop under the
    // editor's own refusal to save a space with no terms left in it.
    query::parse(&space.query).map_err(|error| {
        notes_error(NotesError::Query {
            message: error.message,
            token_index: error.token_index,
        })
    })?;
    let pairs = vec![
        ("space".to_owned(), FieldValue::Str(space.query.clone())),
        // The canonical spelling of whatever the form had selected. This is the
        // one place a value keeper could not read is rewritten, and it is a
        // repair rather than a rewrite: the editor showed the fallback and said
        // why, so pressing Save is the user agreeing to it.
        ("sort".to_owned(), FieldValue::Str(space.sort.clone())),
    ];
    // Written only when there is one, so a space nobody gave an icon keeps the
    // frontmatter it had rather than growing an empty key to explain. The same
    // rule for the rail position: zero *is* unpositioned, so stamping
    // `order: 0` into every space would claim each of them was placed there.
    //
    // And the same rule for the cap, which until Story 44.11 was the exception:
    // `limit` was written unconditionally from a value the form does not render,
    // and a reader that turned an ABSENT key into the page size meant every
    // space saved once grew a `limit: 500` it had never asked for — a cap the
    // user never set, in a file they can read, that nothing was applying.
    // Now zero is no cap and no cap writes no key.
    let with_presentation = |mut pairs: Vec<(String, FieldValue)>| {
        if let Some(icon) = space.icon.as_deref().and_then(space_icon) {
            pairs.push(("icon".to_owned(), FieldValue::Str(icon)));
        }
        if space.order != sort::DEFAULT_SPACE_ORDER {
            pairs.push(("order".to_owned(), FieldValue::Num(space.order)));
        }
        if space.limit > 0 {
            pairs.push(("limit".to_owned(), FieldValue::Num(f64::from(space.limit))));
        }
        // Same rule as the icon: written only when there is one, so clearing the
        // field removes the key rather than leaving `template: ""` behind for a
        // reader to decide about. `keeper-core` already treats empty and absent
        // as one state; this is what stops the empty one ever being written.
        if let Some(template) = space
            .template
            .as_deref()
            .map(str::trim)
            .filter(|path| !path.is_empty())
        {
            pairs.push((
                templates::SPACE_TEMPLATE_KEY.to_owned(),
                FieldValue::Str(template.to_owned()),
            ));
        }
        // Same rule again: written only when there is one, so clearing the
        // field in the editor removes the key instead of leaving `folder: ""`
        // for a reader to interpret.
        if let Some(folder) = space
            .folder
            .as_deref()
            .map(str::trim)
            .filter(|dir| !dir.is_empty())
        {
            pairs.push(("folder".to_owned(), FieldValue::Str(folder.to_owned())));
        }
        pairs
    };

    // An existing space keeps every byte outside the keys this touches
    // (FR-121): the definition is spliced, and the name is spliced only when it
    // actually changed.
    if let Some(id) = space.id.as_ref().filter(|id| !id.is_empty()) {
        let entry = entry_of(&vault_id, id)?;
        let source = notes_vault::read_note(&vault, &entry.path).map_err(notes_error)?;
        let mut pairs = with_presentation(pairs);
        // `keeper` is spliced whole, so a key this request does not carry is a
        // key the save would delete. `default` is keeper's own marker and the
        // editor has no control for it (Story 44.3) — dropping it here would
        // make editing the seeded Inbox turn it into an ordinary space, and
        // "Restore default spaces" would then offer a second one.
        if let Some(key) = default_spaces::default_key_of(&source) {
            pairs.push(("default".to_owned(), FieldValue::Str(key)));
        }
        let mut updated = Frontmatter::set_in(&source, "keeper", FieldValue::Map(pairs));
        let renamed = space.name != entry.title;
        if renamed {
            // `title` rather than the heading alone, because `note_title` reads
            // frontmatter first and a space's body belongs to whoever last
            // edited it: the key is the only place a name is guaranteed to
            // stick.
            updated = Frontmatter::set_in(&updated, "title", FieldValue::Str(space.name.clone()));
            // Only the heading keeper itself wrote follows; `naming` owns that
            // rule, beside `title_from_body`, which is the line it matches.
            let (_, body_offset) = Frontmatter::parse(&updated);
            if let Some(body) =
                naming::retitle_heading(&updated[body_offset..], &entry.title, &space.name)
            {
                updated = format!("{}{body}", &updated[..body_offset]);
            }
        }
        notes_vault::write_note(&vault, &entry.path, &updated).map_err(notes_error)?;
        let path = if renamed {
            rename_in_place(&vault, &entry.path, &space.name)?
        } else {
            entry.path
        };
        return Ok(NoteRefVm {
            vault_id,
            id: entry.id,
            path,
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
        (
            "keeper".to_owned(),
            FieldValue::Map(with_presentation(pairs)),
        ),
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

/// Rename a note's file to match a new title, keeping it in its own directory.
///
/// The filename half of what [`notes_rename`] does, reused here because a space
/// whose file still says `active-work` while the sidebar says "Archive triage"
/// is a vault that disagrees with the app about what it holds — and the vault is
/// the thing Obsidian and the sync see.
fn rename_in_place(vault: &Vault, from_rel: &str, title: &str) -> Result<String, IpcError> {
    let dir = from_rel
        .rsplit_once('/')
        .map_or(String::new(), |(dir, _)| dir.to_owned());
    let filename = naming::note_filename(title, &today(), &notes_vault::siblings(vault, &dir));
    let to_rel = if dir.is_empty() {
        filename
    } else {
        format!("{dir}/{filename}")
    };
    notes_vault::rename_note(vault, from_rel, &to_rel).map_err(notes_error)?;
    Ok(to_rel)
}

/// A space's stored query, read back into the editor's chip vocabulary
/// (FR-149, UX-DR55).
///
/// Parse-only and pure, beside [`notes_space_validate`] for the same reason: the
/// editor asks about a query before it owns one, and neither question needs the
/// vault. The whole decision — which terms a chip can hold and what happens to a
/// query holding one it cannot — is `keeper-core`'s, so this is a call, not a
/// second opinion.
#[tauri::command]
pub async fn notes_space_terms(query: String) -> Result<NoteSpaceTermsVm, IpcError> {
    query::decompose(&query).map_err(|error| {
        notes_error(NotesError::Query {
            message: error.message,
            token_index: error.token_index,
        })
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

// ---------------------------------------------------------------------------
// Markdown widgets
// ---------------------------------------------------------------------------

/// What one `> [!board]` / `> [!log]` / `> [!refs]` callout draws (FR-264).
///
/// `argument` is the callout's own text, verbatim — this command decides what an
/// empty one means and composes the query from it
/// ([`widget::WidgetKind::effective_query`]), so no query is spliced in the
/// webview (AD-65). The kind decides the ordering too, which is why the rows
/// come back already sorted: a widget that returned an unordered set and let
/// three components each sort it would be three chances to disagree with the
/// session pane the widget mirrors.
///
/// **A broken query is an error, not an empty widget.** A note whose callout has
/// a typo in it must say so, because "no rows" and "your query does not parse"
/// look identical on screen and only one of them is the reader's fault.
///
/// Rejects with: `internal` (unknown vault), `query` (a callout whose argument
/// does not parse — carrying the token index the editor underlines).
#[tauri::command]
pub async fn notes_widget(
    vault_id: String,
    kind: widget::WidgetKind,
    argument: String,
) -> Result<Vec<widget::WidgetRow>, IpcError> {
    let vault = vault_of(&vault_id)?;
    let snapshot = notes_vault::snapshot(&vault_id)
        .ok_or_else(|| notes_error(NotesError::VaultUnknown(vault_id.clone())))?;
    let mut parsed =
        keeper_core::notes::query::parse(&kind.effective_query(&argument)).map_err(|error| {
            notes_error(NotesError::Query {
                message: error.message,
                token_index: error.token_index,
            })
        })?;
    // The same binding `project_list` does, and for the same reason: `backlink:`
    // and a title-resolved `link:` cannot be answered without the index behind
    // them, and the binding is only valid for this snapshot revision.
    query::bind_index(&mut parsed, &snapshot);
    let now_ms = notes_vault::local_now_ms();
    let matched: Vec<&IndexEntry> = snapshot
        .entries()
        .iter()
        .filter(|entry| {
            let mut body = body_reader(&vault, &entry.path);
            query::eval(&parsed, entry, &mut body, now_ms)
        })
        .collect();
    Ok(widget::rows_of(kind, &matched))
}

/// Move one card on a board widget: which column, and where in that column.
///
/// The note-side twin of [`crate::sessions_ipc::sessions_task_move`], and
/// deliberately not a call into it: a session's move compiles a [`Plan`] against
/// a session folder and runs through the sessions executor, while a note is
/// written through the vault's own writer with its own trash and sync ledger.
/// The *arithmetic* is shared — both ask [`order::drop_order`] where a card
/// goes — which is the part that could have drifted.
///
/// `status` is the column's own word rather than a parsed enum: a board widget
/// in an ordinary note has no closed column set, because the note's own callout
/// query decides what it selects. The session board's four are that board's
/// contract, not markdown's.
///
/// **The column is re-read here, not trusted from the drag** — the same reason
/// the session board re-reads: a widget that has been on screen for ten minutes
/// is a widget an agent has had ten minutes to write notes into, and placing a
/// card between two neighbours that have since moved is how a drop lands
/// somewhere nobody chose.
///
/// Both keys are spliced ([`Frontmatter::set_in`]), so each write changes one
/// key and leaves every other byte — key order, comments, CRLF endings, the
/// body — exactly as it was (FR-121). And when the gap between two neighbours
/// cannot be halved, the column is renumbered first and the moved card written
/// **last** (AD-111): a crash halfway leaves a renumbered column and an unmoved
/// card, never a card placed into a numbering that never happened.
///
/// Rejects with: `internal` (unknown vault), `notFound` (a card that is not in
/// the vault any more), `query` (an unparseable callout argument).
#[tauri::command]
pub async fn notes_widget_move(
    vault_id: String,
    kind: widget::WidgetKind,
    argument: String,
    note_id: String,
    status: String,
    index: u32,
) -> Result<(), IpcError> {
    let vault = vault_of(&vault_id)?;
    let moved = entry_of(&vault_id, &note_id)?;
    let rows = notes_widget(vault_id.clone(), kind, argument).await?;

    // The target column as it stands, in rendered order, without the card being
    // moved — which is exactly what `drop_order` needs neighbours out of.
    let column: Vec<&widget::WidgetRow> = rows
        .iter()
        .filter(|row| row.status.as_deref() == Some(status.as_str()) && row.id != moved.id)
        .collect();
    let at = (index as usize).min(column.len());
    let before = at
        .checked_sub(1)
        .and_then(|i| column.get(i))
        .map(|r| r.order);
    let after = column.get(at).map(|r| r.order);

    let placed = match order::drop_order(before, after) {
        Some(placed) => placed,
        None => {
            // The gap collapsed: hand out whole numbers again, keeping the order
            // the reader is looking at and leaving a hole at `at`.
            for (position, row) in column.iter().enumerate() {
                let slot = if position < at {
                    position
                } else {
                    position + 1
                };
                let renumbered = order::renumbered_order(slot);
                // Only the notes whose number actually changes: a write that
                // produces the bytes already on disk is a sync commit nobody
                // made.
                if (row.order - renumbered).abs() <= f64::EPSILON {
                    continue;
                }
                let source = notes_vault::read_note(&vault, &row.path).map_err(notes_error)?;
                notes_vault::write_note(
                    &vault,
                    &row.path,
                    &order::set_order_in(&source, renumbered),
                )
                .map_err(notes_error)?;
            }
            order::renumbered_order(at)
        }
    };

    // Read now, not before the renumber: the splice preserves the bytes on disk
    // *at the moment of writing*, and a source captured earlier would revert an
    // edit made in between.
    let source = notes_vault::read_note(&vault, &moved.path).map_err(notes_error)?;
    let updated = Frontmatter::set_in(
        &source,
        widget::WIDGET_STATUS_KEY,
        FieldValue::Str(status.clone()),
    );
    notes_vault::write_note(&vault, &moved.path, &order::set_order_in(&updated, placed))
        .map_err(notes_error)
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

/// Resolve one wikilink target to the note it names (Story 45.18, FR-196,
/// FR-108).
///
/// **The resolver already existed and nothing could reach it.**
/// `NoteIndexSnapshot::resolve_link` has answered this question since epic 37 —
/// it is what the backlink map is built from — and no command exposed it, so
/// clicking a rendered wikilink has done nothing since 37.6 while the text has
/// been `cursor: pointer` the whole time. This is the missing wire, not a new
/// rule.
///
/// Deliberately NOT [`notes_link_targets`] with an exact-match filter in the
/// webview. That command is a substring search for a completion popup; the
/// resolver folds through `link_key` and answers to a note's id, its
/// vault-relative path, that path without the `.md`, its filename stem AND its
/// title, then breaks ties by path order. `index.rs` says in as many words that
/// two definitions of "what names this note" is a bug waiting to happen: the
/// day the follower and the backlink map disagree, a link opens one note and
/// appears in another note's backlinks, and nothing in the UI can explain why.
///
/// `None` rather than an error for a target nothing answers to: a link to a
/// note that has not been written yet is an ordinary thing to have in a vault,
/// and the surface says so where the link is.
#[tauri::command]
pub async fn notes_resolve_link(
    vault_id: String,
    target: String,
) -> Result<Option<NoteRefVm>, IpcError> {
    let snapshot = notes_vault::snapshot(&vault_id)
        .ok_or_else(|| notes_error(NotesError::VaultUnknown(vault_id.clone())))?;
    Ok(snapshot.resolve_link(&target).map(|entry| NoteRefVm {
        vault_id,
        id: entry.id.clone(),
        path: entry.path.clone(),
        title: entry.title.clone(),
    }))
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

/// Create a note (FR-98, FR-160). No dialog anywhere in the path (UX-DR35).
#[tauri::command]
pub async fn notes_create(vault_id: String, req: NoteCreateReq) -> Result<NoteCreateVm, IpcError> {
    let vault = vault_of(&vault_id)?;
    create_for_space(&vault, &req)
}

/// Create a note the space it was asked for from will actually list
/// (Story 44.6).
///
/// "New note in this space" is a promise about where the note turns up, so the
/// space's own query decides what the note carries — its tags, its folder, its
/// flags — through [`seed::inherit`]. Nothing here reads the query: the whole
/// reading of the DSL stays in `keeper-core`, which is also the only crate that
/// can be tested on this host (AD-55, AD-56).
///
/// **The verdict is taken from the bytes that were written**, not from the
/// seed's intentions: the note is indexed through the reconciler's own parser
/// and the space's query is run over it. A seed that was not enough therefore
/// cannot report success, and a query the seed never touched but creation
/// happened to satisfy (`date:created>=-7d`, `origin:local`) cannot report a
/// failure.
///
/// A create that could not do what was asked **says so at `INFO`**, because a
/// decision only the returned value carries is a decision nobody can debug from
/// a log (DW-162).
fn create_for_space(vault: &Vault, req: &NoteCreateReq) -> Result<NoteCreateVm, IpcError> {
    let mut notices = Vec::new();
    let asked = req.space.as_deref().filter(|id| !id.trim().is_empty());
    let Some(space_id) = asked else {
        let note = create_note(vault, req, &seed::Seed::default(), None, &mut notices)?;
        return Ok(NoteCreateVm { note, notices });
    };
    let Some(space) = space_source(vault, space_id) else {
        // Deleted between the click and the write, or never indexed. The note
        // is still worth having, so this is a log line and not a refusal.
        tracing::info!(
            space = %space_id,
            "notes: the space this note was asked for is not in the index; creating an ordinary note"
        );
        let note = create_note(vault, req, &seed::Seed::default(), None, &mut notices)?;
        return Ok(NoteCreateVm { note, notices });
    };
    // The space's folder overrides whatever the query implied, because it was
    // typed rather than inferred (Story 44.13).
    let seeded = seed::inherit_into(&space.query, space.folder.as_deref());
    // The space's own default template, the middle rung of `template_source`'s
    // three (Story 44.7). Read from the same one read of the space note that
    // produced the query, so a note cannot be seeded from one version of the
    // space and templated from another.
    let note = create_note(vault, req, &seeded, space.template.as_deref(), &mut notices)?;
    match notes_vault::read_note(vault, &note.path) {
        Ok(source) => {
            let (_, body_at) = Frontmatter::parse(&source);
            let body = source.get(body_at..).unwrap_or_default();
            let entry = notes_vault::index_written(&note.path, &source);
            if let Some(sentence) = seed::verdict(
                &space.name,
                &space.query,
                &entry,
                body,
                notes_vault::local_now_ms(),
            ) {
                tracing::info!(
                    space = %space.name,
                    note = %note.path,
                    "notes: {sentence}"
                );
                notices.push(sentence);
            }
        }
        Err(error) => {
            // The note is on disk — `create_note` returned — so this is keeper
            // failing to check its own work, not a failed create. Saying
            // nothing would be the honest half of it; saying it at `INFO` is
            // the rest.
            tracing::info!(
                %error,
                note = %note.path,
                "notes: created the note but could not re-read it to check the space would list it"
            );
        }
    }
    Ok(NoteCreateVm { note, notices })
}

/// The three things a create needs to know about the space it was asked for.
struct SpaceForCreate {
    name: String,
    /// The stored query text, unparsed.
    query: String,
    /// `keeper.template`, the template this space hands out (Story 44.7).
    template: Option<String>,
    folder: Option<String>,
}

/// One space's name, stored query text and default template, or `None` when no
/// space in the index carries that id.
///
/// Deliberately not [`space_lens`]: that one refuses a query that does not
/// parse, which is right for listing (a broken space selects nothing) and wrong
/// here (a broken space must still be able to hold a new note, and the user has
/// to be told which of the two facts they are looking at).
///
/// **One read of the note, three answers.** Fetching the template separately
/// would be a second read that can disagree with the first if the file changes
/// in between — the same reasoning [`space_lens`] gives for taking the query and
/// the sort together.
fn space_source(vault: &Vault, space_id: &str) -> Option<SpaceForCreate> {
    let snapshot = notes_vault::snapshot(&vault.id)?;
    let entry = snapshot.by_id(space_id)?;
    let source = notes_vault::read_note(vault, &entry.path).ok()?;
    let def = space_def(entry, &source);
    Some(SpaceForCreate {
        name: def.name,
        query: def.query,
        template: def.template,
        folder: def.folder,
    })
}

/// The shared create path, used by `notes_create` and by capture.
///
/// `seed` is everything beyond the caller's request that shapes the note: the
/// space's inherited tags, folder and flags (Story 44.6), and the capture
/// mark, which is one of those flags rather than a parameter of its own.
///
/// `space_template` is the template the space hands out, the middle rung of
/// [`template_source`]'s three (Story 44.7).
///
/// `notices` collects the finished sentences the caller must show. It is an
/// out-parameter rather than a return value because the note is created either
/// way: a notice is something to say about a note that exists, never a reason
/// not to write one.
fn create_note(
    vault: &Vault,
    req: &NoteCreateReq,
    seed: &seed::Seed,
    space_template: Option<&str>,
    notices: &mut Vec<String>,
) -> Result<NoteRefVm, IpcError> {
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

    // A template is applied through the pure core, which splits its frontmatter
    // off, resolves the placeholders, drops the `template` tag and hands back
    // the tags and properties the copy inherits (AD-82). The caret offset is
    // into the expanded BODY, which is exactly the space the body channel speaks
    // in — one `\n` away, because the document keeper writes puts a blank line
    // between the block and the body.
    let choice = template_source(vault, req, space_template, seed.capture);
    let (applied, provenance) = match &choice {
        TemplateChoice::Found { rel, source } => {
            let applied = templates::expand(
                source,
                &templates::TemplateCtx {
                    title: title.clone(),
                    id: id.clone(),
                    now_local: now_local(),
                },
            );
            let provenance = templates::provenance_pairs(rel, applied.source_id.as_deref());
            (Some(applied), provenance)
        }
        TemplateChoice::Missing { named } => {
            // The note is still created. The sentence says both halves, or it
            // reads as a failure while the note sits right there.
            notices.push(templates::missing_template_notice(named));
            (None, Vec::new())
        }
        TemplateChoice::None => (None, Vec::new()),
    };
    let (body, caret) = match &applied {
        Some(applied) if body_source.trim().is_empty() => (applied.body.clone(), applied.caret),
        Some(applied) => (format!("{}\n{body_source}", applied.body), applied.caret),
        None => (body_source, None),
    };

    // The caller's destination wins over the space's. A space seeds a folder
    // because its query names one; a caller that names one is answering a
    // question the space only implied, and the more specific answer is the one
    // to obey.
    let dest = req
        .dest
        .clone()
        .or_else(|| seed.dest.clone())
        .unwrap_or_default()
        .trim_matches('/')
        .to_owned();
    let filename = naming::note_filename(&title, &today(), &notes_vault::siblings(vault, &dest));
    let rel = if dest.is_empty() {
        filename
    } else {
        format!("{dest}/{filename}")
    };

    // A create must never overwrite. The uniqueness of `filename` rests on
    // `siblings`, which reports a directory it CANNOT READ as an empty one
    // (`read_dir(...).unwrap_or_default()`) — so a folder keeper cannot list
    // yields a name keeper believes is free, and `write_note`'s `atomic_write`
    // would then replace a note that is already there, in a vault whose next
    // commit carries the replacement to every machine.
    //
    // Unreachable before Story 44.6: every caller passed `dest: None`, so the
    // only directory ever listed was the vault root, and a root keeper cannot
    // read is a vault that is already gone. The space seed is the first thing
    // in this app to choose a subdirectory, which is what makes the backstop
    // worth its four lines now and not before.
    //
    // Refusing rather than picking another name: keeper cannot list the folder
    // it is writing into, so it does not know what else is in there either, and
    // a blank note is the cheapest thing in the app to ask for again.
    if notes_vault::contained(vault, &rel)
        .map_err(notes_error)?
        .exists()
    {
        // `NotesInvalid`, the code every other notes refusal uses — there is no
        // `InvalidInput` in `IpcErrorCode`, which the compiler on this host
        // would have said and could not (DW-170).
        return Err(IpcError {
            code: IpcErrorCode::NotesInvalid,
            message: format!(
                "keeper won't create this note: {rel} is already there and keeper couldn't read \
                 its folder to pick a free name. Nothing has been changed."
            ),
            account_id: None,
            retriable: false,
        });
    }

    let mut pairs = vec![
        ("id".to_owned(), FieldValue::Str(id.clone())),
        ("created".to_owned(), FieldValue::Str(now_local())),
        ("updated".to_owned(), FieldValue::Str(now_local())),
    ];
    // The caller's tags, the space's, and the TEMPLATE's — unioned in that order
    // and de-duplicated through the one definition of a tag, so a space that
    // names `#Work`, a caller that names `work` and a template that names
    // `Work` do not put three spellings of one tag in the file.
    //
    // The template's list arrives from `keeper-core` already stripped of
    // `template` (AD-82): the copy is not a template, and this is the union it
    // would otherwise be smuggled back in through.
    let template_tags: &[String] = applied.as_ref().map_or(&[], |applied| &applied.tags);
    let note_tags = tags::normalise_all(
        req.tags
            .iter()
            .chain(seed.tags.iter())
            .chain(template_tags.iter())
            .map(String::as_str),
    );
    if !note_tags.is_empty() {
        pairs.push((
            "tags".to_owned(),
            FieldValue::List(note_tags.into_iter().map(FieldValue::Str).collect()),
        ));
    }
    // The two boolean flags a space can ask a new note to carry. They are
    // ordinary frontmatter Obsidian shows as properties, which is why the index
    // can read them back and `is:pinned` can be satisfied by creating at all.
    if seed.pinned {
        pairs.push(("pinned".to_owned(), FieldValue::Bool(true)));
    }
    if seed.archived {
        pairs.push(("archived".to_owned(), FieldValue::Bool(true)));
    }
    // The template's own properties, after everything keeper decides and before
    // the reserved map. `keeper-core` already removed the six that belong to the
    // template itself, so what is left is the author's: `status: draft`,
    // `project:`, whatever they put in the scaffold to be filled in.
    if let Some(applied) = &applied {
        pairs.extend(applied.properties.iter().cloned());
    }
    // One reserved map, assembled once. Two `pairs.push(("keeper", …))` would
    // write the key twice, and `Frontmatter`'s reader takes the first — so a
    // captured note made from a template would have silently lost whichever of
    // the two came second.
    let mut reserved = provenance;
    if seed.capture {
        // The reserved namespace's other documented sub-key, so the inbox lens
        // can find unfiled thoughts.
        reserved.push(("capture".to_owned(), FieldValue::Bool(true)));
    }
    if !reserved.is_empty() {
        pairs.push(("keeper".to_owned(), FieldValue::Map(reserved)));
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

/// What a create resolved its template to.
///
/// Three answers, not two. "No template was asked for" and "a template was asked
/// for and is not there" produce the same note and must not produce the same
/// silence: the second is the one the user has to be told about, because they
/// chose a template and did not get it.
enum TemplateChoice {
    /// Nobody named one.
    None,
    /// Found, with the path it was found at — the path that becomes the note's
    /// provenance.
    Found { rel: String, source: String },
    /// Named, and not in the vault.
    Missing { named: String },
}

/// The template a create should apply, if any.
///
/// **Four rungs, most specific first** (FR-161, FR-162, FR-193): the template
/// the caller named, else the template the space hands out, else — for a quick
/// capture only — the vault's capture template, else the vault's configured
/// default. A caller naming one is answering a question the space only implied,
/// a space naming one is answering a question the vault only implied, and the
/// capture template sits between them because it is chosen for a surface.
///
/// A template path that names nothing is **not** a failure: the note is created
/// plain, because losing a thought over a missing scaffold is the wrong trade.
/// It is also not silent — see [`TemplateChoice::Missing`], whose sentence is
/// composed in `keeper-core` and reaches both the log and the caller.
///
/// The rungs and their order live in `keeper-core` ([`templates::rung`]) rather
/// than in this `or_else` chain, because this crate does not compile on the
/// host the decision is proved on (AD-56) — and because the chain used to trim
/// one rung and not the next, so a blank in the caller's slot fell through and
/// a blank in the space's slot swallowed the vault default.
fn template_source(
    vault: &Vault,
    req: &NoteCreateReq,
    space_template: Option<&str>,
    capture: bool,
) -> TemplateChoice {
    // The capture rung applies exactly when this create IS a capture. Derived
    // from the seed's own mark rather than passed as a second parameter, so a
    // future caller cannot acquire a capture's scaffold by forgetting to say
    // it is not one.
    let capture_template = capture
        .then_some(vault.config.capture_template.as_deref())
        .flatten();
    let Some(named) = templates::rung(templates::TemplateRungs {
        named: req.template.as_deref(),
        space: space_template,
        capture: capture_template,
        vault_default: vault.config.default_template.as_deref(),
    })
    .map(str::to_owned) else {
        return TemplateChoice::None;
    };
    // A bare name is a file in the template directory; anything with a slash is
    // already vault-relative, which is what AD-82 buys — a template may live
    // wherever its author put it.
    let rel = if named.contains('/') {
        named.clone()
    } else {
        format!("{}/{named}", templates::TEMPLATES_DIR)
    };
    match notes_vault::read_note(vault, &rel) {
        Ok(source) => TemplateChoice::Found { rel, source },
        Err(error) => {
            // `info!`, not `debug!`: nothing sets `RUST_LOG` in the packaged app,
            // so a `debug!` here is a decision nobody can see on the machine that
            // made it (DW-162). This is a path that declines to act, and a path
            // that declines to act says so.
            tracing::info!(
                %error,
                template = %rel,
                "notes: the named template is not in this vault; creating a plain note"
            );
            TemplateChoice::Missing { named: rel }
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
    // Which template today's entry starts from (Story 45.20, FR-198).
    //
    // This used to be `vault.config.default_template` and nothing else, which
    // is `None` in every vault whose owner never set one — so 44.7's shipped
    // `Journal entry` template was seeded into every vault and reached by
    // nothing. `journal_template` names it when the vault still has it, and
    // names nothing when the user deleted it, which is what lets the ladder
    // below fall through to their configured default instead of stopping on a
    // rung that is not there. The existence check is the shell's one
    // contribution; the decision is `keeper-core`'s and is asserted there.
    let present =
        |candidate: &str| notes_vault::contained(&vault, candidate).is_ok_and(|path| path.exists());
    // No `dest`: the journal path is fixed by the configured template, so the
    // collision counter must not be allowed to move it.
    let req = NoteCreateReq {
        title: Some(title),
        body: None,
        template: templates::journal_template(&present)
            .or_else(|| vault.config.default_template.clone()),
        dest: None,
        tags: Vec::new(),
        // Today's journal is reached by `⌘⌥J`, the tray and the palette, never
        // from a space row, so there is no space to inherit from.
        space: None,
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
    // The journal's template comes from `req.template`, which the caller filled
    // with the shipped journal scaffold or the vault's configured default. No
    // space rung: today's entry is opened by a shortcut, a tray item and the
    // palette, none of which is inside a space.
    // Not a capture: `⌘⌥J` opens today's page, so the capture rung is skipped
    // and the journal falls through to the vault default.
    let found = match template_source(vault, req, None, false) {
        TemplateChoice::Found { rel, source } => Some((rel, source)),
        // A journal entry is created either way — the whole point of `⌘⌥J` is
        // that today's page is always there. `template_source` has already said
        // so at `INFO`; there is no surface here to carry a notice to.
        TemplateChoice::Missing { .. } | TemplateChoice::None => None,
    };
    // The bytes are composed in `keeper-core` (Story 45.20). What used to be
    // thirty lines of frontmatter assembly here was the only part of "today's
    // journal applies its template" that no test on a Linux host could reach,
    // which is exactly why the template never arriving went unnoticed.
    let note = templates::render_journal_note(
        found
            .as_ref()
            .map(|(rel, source)| (rel.as_str(), source.as_str())),
        &title,
        &id,
        &now_local(),
    );
    notes_vault::write_note(vault, rel, &note.text).map_err(notes_error)?;
    if let Some(at) = note.caret {
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

/// Place a note in its list, or return it to the default (Story 44.5, FR-159).
///
/// `order: None` REMOVES the key rather than writing `order: 0`. The two are not
/// the same statement: 0 with the key present claims the user deliberately put
/// this note where every silent note already is, and it would also mean keeper
/// had written a property into a file nobody asked it to touch (FR-121).
///
/// Same splice discipline as [`notes_set_flag`], and for the same reason: the
/// note is Obsidian's file too, and a re-serialisation would reorder somebody
/// else's keys.
#[tauri::command]
pub async fn notes_set_order(
    vault_id: String,
    note_id: String,
    order: Option<f64>,
) -> Result<(), IpcError> {
    let vault = vault_of(&vault_id)?;
    let entry = entry_of(&vault_id, &note_id)?;
    let source = notes_vault::read_note(&vault, &entry.path).map_err(notes_error)?;
    let updated = match order {
        Some(order) => keeper_core::notes::order::set_order_in(&source, order),
        None => keeper_core::notes::order::clear_order_in(&source),
    };
    notes_vault::write_note(&vault, &entry.path, &updated).map_err(notes_error)
}

/// What deleting this note would remove, in the words the confirmation shows
/// (Story 45.17, FR-195, UX-DR78).
///
/// A separate call from the delete, for `sync_delete_plan`'s reason (Story
/// 45.3): the sentences are composed by code that knows what the removal does,
/// so the dialog cannot promise something the command will not do.
///
/// **A read failure fails the call rather than describing the note anyway.**
/// The alternative is a plan built from an empty source, which would silently
/// drop the clause that says a seeded space stays deleted — a confirmation
/// missing the one sentence it exists to carry. A note keeper cannot read is
/// also a note `trash_note` is about to fail on, so nothing is lost by saying
/// so here.
#[tauri::command]
pub async fn notes_delete_plan(
    vault_id: String,
    note_id: String,
) -> Result<NoteDeletePlanVm, IpcError> {
    let vault = vault_of(&vault_id)?;
    let entry = entry_of(&vault_id, &note_id)?;
    let source = notes_vault::read_note(&vault, &entry.path).map_err(notes_error)?;
    // The index says what it is; the file says whose it is. Both are needed and
    // neither substitutes: `space` is a flag the index computes, and
    // `keeper.default` is a marker only the seeder writes.
    Ok(if has_flag(&entry, "space") {
        NoteDeletePlanVm::for_space(
            &entry.title,
            &entry.path,
            default_spaces::default_key_of(&source).as_deref(),
        )
    } else {
        NoteDeletePlanVm::for_note(&entry.title, &entry.path)
    })
}

/// Move a note to the trash and stage its removal (NFR-30). Never an `unlink`.
///
/// **This is also how a space is deleted** (Story 45.17, FR-195). A space is a
/// note, so a second command for it would be a second removal path — and the
/// one that eventually forgot the ledger. Deleting a seeded default records its
/// key as offered, which is what stops the next `refresh` seeding it back:
/// [`default_spaces::record_deleted`] explains why that is the ledger's
/// existing concept rather than a tombstone this story invented.
#[tauri::command]
pub async fn notes_delete(vault_id: String, note_id: String) -> Result<(), IpcError> {
    let vault = vault_of(&vault_id)?;
    let entry = entry_of(&vault_id, &note_id)?;
    // Read BEFORE the bytes move. `keeper.default` lives in the file, and after
    // `trash_note` there is nothing at `entry.path` to read it from.
    let source = notes_vault::read_note(&vault, &entry.path);
    notes_vault::trash_note(&vault, &entry.path)
        .map(drop)
        .map_err(notes_error)?;
    // Unlike the plan above, an unreadable note does NOT fail this: the person
    // asked for the deletion and it has happened. What it must not do is read
    // as "not a default" — that is a wrong answer wearing an absent one's
    // clothes — so the failure becomes the outcome that says keeper could not
    // tell, at WARN, naming the file.
    let record = match &source {
        Ok(text) => default_spaces::record_deleted(&mut VaultSeedFiles { vault: &vault }, text),
        Err(error) => default_spaces::DeleteRecord::Blocked(format!(
            "could not read {} before deleting it, so keeper cannot tell whether it was a \
             default space: {error}",
            entry.path
        )),
    };
    let (level, message) = record.report();
    if level <= tracing::Level::WARN {
        tracing::warn!(vault = %vault.id, "notes: {message}");
    } else {
        tracing::info!(vault = %vault.id, "notes: {message}");
    }
    Ok(())
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
    // The baseline Story 44.8 diffs a template edit against: the template as the
    // user found it when they opened it. Captured on OPEN rather than on save,
    // because the autosave fires every few hundred milliseconds — a baseline
    // taken at save time would be "the text before the last keystroke burst",
    // and the offer would show the tail of an edit instead of the edit.
    remember_template_before(&vault_id, &entry.path, &text);

    channel
        .send(NoteBodyBatch::Reset {
            rev: rev.clone(),
            // The path the subscription was opened on. Already resolved two
            // lines above; withholding it is what left the editor's own header
            // caption blank until the first autosave (Story 45.18).
            path: entry.path.clone(),
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
// Updating notes from their template (Story 44.8, FR-163, UX-DR59)
// ---------------------------------------------------------------------------

/// What a template said when the user opened it, keyed
/// `<vault id>\0<vault-relative path>`.
///
/// **The baseline of an editing session, not of a write.** Captured in
/// `notes_open`, and deliberately not in `notes_save`: the autosave fires a few
/// hundred milliseconds after typing stops, so a save-time baseline would be
/// "the text before the last burst of keystrokes" and the offer would show the
/// tail of an edit rather than the edit. Opening the template again re-takes it,
/// which is the right reset — a session is what a person means by "I changed the
/// template".
///
/// git cannot supply this either: the vault commits on an idle debounce, so at
/// the moment a template is saved `HEAD` is usually the version before the
/// PREVIOUS edit, and diffing against it would re-offer changes the user already
/// decided about last week.
///
/// Nothing here survives a restart, and that is the honest behaviour: keeper
/// offers to propagate an edit it watched happen, never one it inferred.
static TEMPLATE_BEFORE: LazyLock<Mutex<HashMap<String, String>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

/// How many opened templates are remembered at once. A template is a note a
/// person opens, so this is bounded by hands; the cap exists so an agent opening
/// a thousand notes cannot grow the map without limit.
const MAX_TEMPLATE_BEFORE: usize = 32;

fn template_before_key(vault_id: &str, rel: &str) -> String {
    format!("{vault_id}\0{rel}")
}

/// Take the session baseline, if the note being opened is a template.
///
/// Cheap for the overwhelmingly common case — one frontmatter parse of text
/// already in memory — and it does nothing at all for an ordinary note, which is
/// what makes `notes_template_update_preview` free to answer "not a template"
/// without touching the vault.
fn remember_template_before(vault_id: &str, rel: &str, disk: &str) {
    let (fm, _) = Frontmatter::parse(disk);
    if !templates::is_template(&fm) {
        return;
    }
    let mut before = TEMPLATE_BEFORE
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    if before.len() >= MAX_TEMPLATE_BEFORE {
        before.clear();
    }
    before.insert(template_before_key(vault_id, rel), disk.to_owned());
}

fn template_before(vault_id: &str, rel: &str) -> Option<String> {
    TEMPLATE_BEFORE
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .get(&template_before_key(vault_id, rel))
        .cloned()
}

/// What keeper would offer to change in the notes made from a template that was
/// just edited (FR-163, UX-DR59).
///
/// `Ok(None)` means there is nothing to say at all — the note saved was not a
/// template, or keeper did not watch it change. `Ok(Some(offer))` means the
/// surface opens, and the offer itself may carry a `declined` sentence. The two
/// are different: "this is not a template" is not a refusal and must not read as
/// one.
///
/// Nothing here writes. The whole command is reads plus
/// [`keeper_core::notes::template_update`]'s arithmetic, which is what makes
/// "offered, never automatic" a property of the code rather than of the UI.
#[tauri::command]
pub async fn notes_template_update_preview(
    app: AppHandle,
    vault_id: String,
    note_id: String,
) -> Result<Option<TemplateUpdateOfferVm>, IpcError> {
    let vault = vault_of(&vault_id)?;
    let entry = entry_of(&vault_id, &note_id)?;
    let Some(before) = template_before(&vault_id, &entry.path) else {
        return Ok(None);
    };
    let after = notes_vault::read_note(&vault, &entry.path).map_err(notes_error)?;
    if after == before {
        return Ok(None);
    }

    let snapshot = notes_vault::snapshot(&vault_id)
        .ok_or_else(|| notes_error(NotesError::VaultUnknown(vault_id.clone())))?;
    let offer = tokio::task::spawn_blocking(move || {
        build_offer(&app, &vault, &snapshot, &entry, &before, &after)
    })
    .await
    .map_err(|error| notes_error(NotesError::NotFound(error.to_string())))?;

    if let Some(reason) = offer.declined.as_deref() {
        // INFO, not debug: this is the whole of what the user sees keeper do
        // when it decides to do nothing, and `RUST_LOG` is unset on the owner's
        // machine, so a `debug!` here would be a decision nobody could ever
        // observe (DW-162).
        tracing::info!(template = %offer.template_path, "notes: template update declined: {reason}");
    } else {
        tracing::info!(
            template = %offer.template_path,
            notes = offer.notes.len(),
            "notes: offering a template update"
        );
    }
    Ok(Some(offer))
}

/// Resolve the whole offer. Blocking: it reads one file per candidate note and
/// asks git one question for the vault.
fn build_offer(
    app: &AppHandle,
    vault: &Vault,
    snapshot: &IndexSnapshot,
    template: &IndexEntry,
    before: &str,
    after: &str,
) -> TemplateUpdateOfferVm {
    let (fm, _) = Frontmatter::parse(after);
    let template_id = fm.as_string("id").map(str::to_owned);
    let reference = template_update::TemplateRef {
        path: &template.path,
        id: template_id.as_deref(),
    };

    let candidates: Vec<(&IndexEntry, templates::Provenance)> = snapshot
        .entries()
        .iter()
        // A template made from a template is still not its own child: skipping
        // the file being edited is what stops an offer to rewrite itself.
        .filter(|entry| entry.path != template.path)
        .filter_map(|entry| {
            let provenance = template_update::provenance_from_index(&entry.fields);
            template_update::made_from(&provenance, &reference).map(|_| (entry, provenance))
        })
        .collect();

    let found = candidates.len();
    if found == 0 || found > template_update::MAX_OFFER_NOTES {
        return template_update::offer(&template.path, &template.title, found, &[]);
    }

    // One `git status` for the vault. `None` — no git, no repository — means
    // nothing can be proven recoverable, so nothing is offered as such.
    let dirty = notes_vault::uncommitted_paths(app, vault);
    let plans: Vec<template_update::NotePlan> = candidates
        .iter()
        .filter_map(|(entry, provenance)| {
            let source = notes_vault::read_note(vault, &entry.path).ok()?;
            let (_, body) = split_note(&source);
            let recoverability = match dirty.as_ref() {
                Some(dirty) if !dirty.contains(&entry.path) => {
                    template_update::Recoverability::Committed
                }
                Some(_) => template_update::Recoverability::Modified,
                None => template_update::Recoverability::Untracked,
            };
            let stale_path = provenance
                .path
                .as_deref()
                .filter(|recorded| *recorded != template.path)
                .map(str::to_owned);
            Some(template_update::plan_note(
                before,
                after,
                &template_update::NoteInput {
                    id: &entry.id,
                    title: &entry.title,
                    path: &entry.path,
                    body,
                    ctx: note_ctx(entry),
                    stale_path,
                    recoverability,
                },
            ))
        })
        .collect();

    template_update::offer(&template.path, &template.title, found, &plans)
}

/// The expansion context a note was created with, as far as the note still
/// records it.
///
/// `created` is the note's own frontmatter, so `{{date:…}}` is compared against
/// the date the note actually carries rather than today's — which is the whole
/// reason a placeholder line in a year-old journal entry is recognised as the
/// template's and not as something the user typed.
fn note_ctx(entry: &IndexEntry) -> templates::TemplateCtx {
    templates::TemplateCtx {
        title: entry.title.clone(),
        id: entry.id.clone(),
        now_local: entry
            .fields
            .get("created")
            .cloned()
            .unwrap_or_else(now_local),
    }
}

/// Apply the changes the user accepted, note by note (FR-163).
///
/// The plan is rebuilt from disk here rather than trusted from the request: the
/// preview may be minutes old, the notes may have been written in since, and a
/// change that no longer matches must not be applied because a dialog once said
/// it would. The request selects *which* changes; it never carries their text.
#[tauri::command]
pub async fn notes_template_update_apply(
    app: AppHandle,
    vault_id: String,
    req: TemplateUpdateApplyReq,
) -> Result<TemplateUpdateResultVm, IpcError> {
    let vault = vault_of(&vault_id)?;
    let snapshot = notes_vault::snapshot(&vault_id)
        .ok_or_else(|| notes_error(NotesError::VaultUnknown(vault_id.clone())))?;
    let entry = snapshot
        .entries()
        .iter()
        .find(|entry| entry.path == req.template_path)
        .cloned()
        .ok_or_else(|| notes_error(NotesError::NotFound(req.template_path.clone())))?;
    let before = template_before(&vault_id, &entry.path).ok_or_else(|| {
        notes_error(NotesError::Template(
            "keeper no longer knows what this template said before it was edited, so it will \
             not change any note from it. Edit the template again to see a fresh offer."
                .to_owned(),
        ))
    })?;
    let after = notes_vault::read_note(&vault, &entry.path).map_err(notes_error)?;

    tokio::task::spawn_blocking(move || {
        run_template_update(&app, &vault, &snapshot, &entry, &before, &after, &req)
    })
    .await
    .map_err(|error| notes_error(NotesError::NotFound(error.to_string())))
}

/// Write the accepted changes. Blocking; one file read and at most one write per
/// selected note.
fn run_template_update(
    app: &AppHandle,
    vault: &Vault,
    snapshot: &IndexSnapshot,
    template: &IndexEntry,
    before: &str,
    after: &str,
    req: &TemplateUpdateApplyReq,
) -> TemplateUpdateResultVm {
    let dirty = notes_vault::uncommitted_paths(app, vault);
    let mut updated = Vec::new();
    let mut skipped = Vec::new();

    for selection in &req.selections {
        let accepted: Vec<usize> = selection
            .changes
            .iter()
            .map(|index| *index as usize)
            .collect();
        if accepted.is_empty() {
            continue;
        }
        let Some(entry) = snapshot
            .entries()
            .iter()
            .find(|e| e.id == selection.note_id)
        else {
            skipped.push(format!(
                "keeper could not find the note {} any more, so it was left alone.",
                selection.note_id
            ));
            continue;
        };
        let Ok(source) = notes_vault::read_note(vault, &entry.path) else {
            skipped.push(format!(
                "\u{201c}{}\u{201d} could not be read, so it was left alone.",
                entry.title
            ));
            continue;
        };
        let (block, body) = split_note(&source);
        let recoverability = match dirty.as_ref() {
            Some(dirty) if !dirty.contains(&entry.path) => {
                template_update::Recoverability::Committed
            }
            Some(_) => template_update::Recoverability::Modified,
            None => template_update::Recoverability::Untracked,
        };
        let plan = template_update::plan_note(
            before,
            after,
            &template_update::NoteInput {
                id: &entry.id,
                title: &entry.title,
                path: &entry.path,
                body,
                ctx: note_ctx(entry),
                stale_path: None,
                recoverability,
            },
        );
        if let Some(reason) = plan.blocked.as_deref() {
            skipped.push(reason.to_owned());
            continue;
        }
        let Some(new_body) = template_update::apply(body, &plan, &accepted) else {
            skipped.push(format!(
                "\u{201c}{}\u{201d} has changed since the preview, so keeper applied nothing to \
                 it rather than something you have not seen.",
                entry.title
            ));
            continue;
        };
        // Resolved BEFORE the write, so it names the commit holding the note as
        // it is right now — which is exactly what undoing this has to restore.
        let Some(undo_rev) = notes_vault::head_rev_of(app, vault, &entry.path) else {
            skipped.push(format!(
                "\u{201c}{}\u{201d} has no revision keeper could put it back from, so it was \
                 left alone.",
                entry.title
            ));
            continue;
        };
        // The block goes back byte for byte: a template edit changes prose, and
        // `updated` is deliberately NOT restamped — that key means "when someone
        // last wrote in this note", and keeper propagating a heading is not that.
        if let Err(error) =
            notes_vault::write_note(vault, &entry.path, &join_note(block, &new_body))
        {
            skipped.push(format!(
                "\u{201c}{}\u{201d} could not be written ({error}), so it is unchanged.",
                entry.title
            ));
            continue;
        }
        updated.push(TemplateUpdateAppliedVm {
            note_id: entry.id.clone(),
            title: entry.title.clone(),
            applied: u32::try_from(accepted.len()).unwrap_or(u32::MAX),
            undo_rev,
        });
    }

    tracing::info!(
        template = %template.path,
        updated = updated.len(),
        skipped = skipped.len(),
        "notes: applied a template update"
    );
    TemplateUpdateResultVm { updated, skipped }
}

/// Write one note back to the text it had at `rev` (FR-114, FR-163).
///
/// The verb the history panel has always implied and never had: `notes_history`
/// could show you the revision and `notes_diff` could show you what changed, and
/// there was no way to act on either. It is also what makes Story 44.8's
/// "accepting is undoable" true rather than aspirational — the undo of a
/// template update is this command against the revision the apply reported.
///
/// A restore is an ordinary write, so it becomes a revision of its own and is
/// itself undoable. Nothing is destroyed by undoing an undo.
#[tauri::command]
pub async fn notes_restore_revision(
    app: AppHandle,
    vault_id: String,
    note_id: String,
    rev: String,
) -> Result<(), IpcError> {
    let vault = vault_of(&vault_id)?;
    let entry = entry_of(&vault_id, &note_id)?;
    let path = entry.path.clone();
    let text = {
        let app = app.clone();
        let vault = vault.clone();
        let path = path.clone();
        tokio::task::spawn_blocking(move || notes_vault::revision_text(&app, &vault, &path, &rev))
            .await
            .ok()
            .flatten()
    };
    let text = text.ok_or_else(|| {
        notes_error(NotesError::NotFound(format!(
            "{path} has no text at that revision"
        )))
    })?;
    notes_vault::write_note(&vault, &path, &text).map_err(notes_error)
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
/// the working alternative, which needs nothing new: `notes_attach_sources`
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

/// Resolve files a person picked into paths a note can name (Story 45.13,
/// FR-188, FR-189).
///
/// The one resolution behind all three of the story's entry points: a path from
/// the file picker, a path from a Files-pane row, a path from anywhere else the
/// shell can hand one over. The bytes never cross IPC in either direction
/// (AD-58), and the webview never learns where the vault is (AD-65) — it sends
/// absolute paths it was given and receives vault-relative ones it may write.
///
/// **Inside the vault is named, outside the vault is copied in.** Those are the
/// only two answers, and the second is the interesting one. FR-145 forbids an
/// absolute path in a synced artefact, so linking to `~/Desktop/photo.png` is
/// not available: the vault syncs to other machines, where that path names
/// nothing — or names a different file, which is worse than nothing. Refusing
/// the file instead would leave "attach from anywhere" meaning "attach from the
/// vault", which is the thing that already worked. So keeper copies it into
/// `attachments/` under a collision-free name and the note names the copy,
/// which is a file that travels with the note by construction.
///
/// **A file already in the vault is NOT copied.** The command this replaces,
/// `notes_attachment_drop`, copied unconditionally, so attaching a file the
/// vault already held would have made a second copy in `attachments/` and
/// pointed the note at the duplicate. Nothing ever called it, so nobody found
/// out.
///
/// One `NoteAttachSourceVm` per source, in the order given, including for the
/// ones keeper refused: a person who selected six files and got four needs to
/// know which two and why, and a shorter list cannot say.
#[tauri::command]
pub async fn notes_attach_sources(
    vault_id: String,
    sources: Vec<String>,
) -> Result<Vec<NoteAttachSourceVm>, IpcError> {
    let vault = vault_of(&vault_id)?;
    // Blocking: `canonicalize` stats every component and a copy moves bytes.
    // On the async runtime that would stall every other command on this thread
    // for as long as the slowest volume takes to answer.
    tokio::task::spawn_blocking(move || {
        sources
            .iter()
            .map(|source| resolve_attach_source(&vault, Path::new(source)))
            .collect()
    })
    .await
    .map_err(|error| notes_error(NotesError::Name(format!("attach: {error}"))))
}

/// One source, resolved or refused. Never panics and never propagates: a
/// selection of six files must not lose the other five to one unreadable one.
fn resolve_attach_source(vault: &Vault, source: &Path) -> NoteAttachSourceVm {
    let name = source.file_name().map_or_else(
        || source.to_string_lossy().into_owned(),
        |name| name.to_string_lossy().into_owned(),
    );
    let refuse = |why: String| NoteAttachSourceVm {
        name: name.clone(),
        rel_path: None,
        copied: false,
        refusal: Some(why),
    };

    // `metadata`, which follows a symlink, rather than `symlink_metadata`: a
    // symlink is resolved and then judged on where it really points, so one
    // inside the vault pointing outside is copied in rather than named at a
    // path whose bytes are not in the vault at all. A broken one fails here.
    let Ok(meta) = std::fs::metadata(source) else {
        return refuse(format!(
            "keeper could not read {name}, so it did not attach it."
        ));
    };
    if meta.is_dir() {
        // Story 43.5's rule, restated where it is enforced: there is no element
        // for a directory, so an embed of one renders as the link it already was.
        return refuse(format!(
            "{name} is a folder. A note can embed a file, but there is nothing to show for a directory."
        ));
    }
    if !meta.is_file() {
        return refuse(format!(
            "{name} is not a regular file — a device or a pipe — and keeper does not attach one."
        ));
    }

    let (Ok(root), Ok(canonical)) = (vault.root.canonicalize(), source.canonicalize()) else {
        return refuse(format!(
            "keeper could not place {name} against this vault, so it did not attach it."
        ));
    };

    match attach::vault_relative(&root, &canonical) {
        Some(rel) => {
            if notes_vault::is_internal(&rel) {
                return refuse(format!(
                    "{name} is inside a folder keeper, git or Obsidian owns, so it is not an attachment."
                ));
            }
            NoteAttachSourceVm {
                name,
                rel_path: Some(rel),
                copied: false,
                refusal: None,
            }
        }
        None => match notes_vault::import_attachment(vault, &canonical) {
            Ok(written) => NoteAttachSourceVm {
                name,
                rel_path: Some(written.rel_path),
                copied: true,
                refusal: None,
            },
            Err(error) => refuse(format!(
                "keeper could not copy {name} into the vault, so it did not attach it: {error}"
            )),
        },
    }
}

/// Notes a person could attach these files to, searchable (Story 45.13, FR-189).
///
/// `holds` is what makes this a different question from `notes_link_targets`,
/// which is otherwise the same search: a note that already embeds one of these
/// files must not be offered as somewhere to put it, and the surface can only
/// know that if this says so. The rule itself —
/// [`keeper_core::notes::attach::already_attached`] — is the same one
/// `src/lib/notes/attach.ts` applies to the open editor's buffer, pinned to it
/// by `attach-vectors.json`.
///
/// Capped at [`MAX_LINK_TARGETS`], and the cap is load-bearing here in a way it
/// is not for the completion: each candidate costs a file read. A chooser over
/// a ten-thousand-note vault reads thirty notes per query and no more.
#[tauri::command]
pub async fn notes_attach_targets(
    vault_id: String,
    query: String,
    names: Vec<String>,
) -> Result<Vec<NoteAttachTargetVm>, IpcError> {
    let vault = vault_of(&vault_id)?;
    let snapshot = notes_vault::snapshot(&vault_id)
        .ok_or_else(|| notes_error(NotesError::VaultUnknown(vault_id)))?;
    let needle = fold(&query);
    let folded: Vec<String> = names.iter().map(|name| name.to_lowercase()).collect();
    let candidates: Vec<IndexEntry> = snapshot
        .entries()
        .iter()
        .filter(|entry| {
            needle.is_empty()
                || fold(&entry.title).contains(&needle)
                || fold(&entry.path).contains(&needle)
        })
        .take(MAX_LINK_TARGETS)
        .cloned()
        .collect();

    let mut hits: Vec<NoteAttachTargetVm> = tokio::task::spawn_blocking(move || {
        candidates
            .into_iter()
            .map(|entry| {
                // A note that cannot be read holds nothing anyone can prove, so
                // it is offered: refusing to offer it would hide a note because
                // of a transient read error.
                let source = notes_vault::read_note(&vault, &entry.path).unwrap_or_default();
                let holds = attach::already_attached(split_note(&source).1, &folded);
                NoteAttachTargetVm {
                    id: entry.id,
                    title: entry.title,
                    path: entry.path,
                    holds,
                }
            })
            .collect()
    })
    .await
    .map_err(|error| notes_error(NotesError::Name(format!("attach targets: {error}"))))?;
    hits.sort_by(|a, b| a.title.cmp(&b.title));
    Ok(hits)
}

/// A note's body as it is on disk, for a surface that has not opened it
/// (Story 45.13).
///
/// The read half of a read-modify-write on a closed note, and deliberately not
/// a second `notes_open`: there is no subscription, no watcher task and no
/// channel, because the caller wants one answer rather than a stream. `rev`
/// is the whole file's revision and is what [`notes_body_write`] must be given
/// back, so a note that changed in between is conflict-copied rather than
/// clobbered.
#[tauri::command]
pub async fn notes_body_read(vault_id: String, note_id: String) -> Result<NoteBodyVm, IpcError> {
    let vault = vault_of(&vault_id)?;
    let entry = entry_of(&vault_id, &note_id)?;
    let source = notes_vault::read_note(&vault, &entry.path).map_err(notes_error)?;
    Ok(NoteBodyVm {
        rev: notes_vault::content_rev(&source),
        text: split_note(&source).1.to_owned(),
    })
}

/// Write a body back to a note nobody has open (Story 45.13).
///
/// The write half, and it makes exactly the same promises [`notes_save`] makes
/// the editor, through the same three functions: the frontmatter block on disk
/// survives byte for byte except for `updated` ([`save_document`], FR-121); a
/// `base_rev` older than disk means the other side changed, so the disk bytes
/// are written aside as an AD-43 conflict copy **before** this write lands; and
/// nothing is lost either way.
///
/// If the note happens to be open in the editor, this is an external write like
/// any other: the body watcher sees it and the editor adopts it, or raises its
/// diff bar over unsaved edits. That path is Story 37.6's and is not special
/// -cased here — a headless write that announced itself would be a second
/// protocol for the same event.
#[tauri::command]
pub async fn notes_body_write(
    vault_id: String,
    note_id: String,
    text: String,
    base_rev: String,
) -> Result<NoteWriteVm, IpcError> {
    let vault = vault_of(&vault_id)?;
    let rel = entry_of(&vault_id, &note_id)?.path;

    let disk = notes_vault::read_note(&vault, &rel).unwrap_or_default();
    let conflict_copy = if notes_vault::content_rev(&disk) == base_rev || disk.is_empty() {
        None
    } else {
        notes_vault::write_conflict_copy(&vault, &rel, &disk)
    };

    let stamped = save_document(split_note(&disk).0, &text);
    notes_vault::write_note(&vault, &rel, &stamped).map_err(notes_error)?;
    Ok(NoteWriteVm {
        rev: notes_vault::content_rev(&stamped),
        path: rel,
        frontmatter: split_note(&stamped).0.to_owned(),
        conflict_copy,
    })
}

// ---------------------------------------------------------------------------
// Embedded files: CSV tables, and the raw text behind every embed
// ---------------------------------------------------------------------------

/// Resolve a `![[…]]` embed target to the file in this vault it names.
///
/// **The webview never joins a root to a subpath** (AD-65). It hands over the
/// text between the brackets — which is what the user typed, or what the
/// attachments panel wrote — and the candidates are formed by
/// [`embed::candidates`], in the only process that knows where the vault is.
///
/// Two candidates and no search: a resolver that walked the vault looking for a
/// matching name would make which file an embed opens depend on what else is in
/// the vault, and an edit would then write to whichever one it found today.
///
/// The refusal names the paths it tried, because Story 45.12's criterion is
/// that an embed whose file has moved says so where the embed is and names what
/// keeper looked for. The candidate list and the sentence come from the same
/// module so the words cannot describe a search this loop did not run.
fn embed_path(vault: &Vault, target: &str) -> Result<(String, PathBuf), IpcError> {
    match embed_path_opt(vault, target) {
        Some(found) => Ok(found),
        None => Err(notes_error(NotesError::NotFound(embed::not_found_notice(
            target,
            &embed::candidates(target, ATTACHMENTS_DIR),
        )))),
    }
}

/// The same resolution, answering `None` rather than a sentence.
///
/// Split out for [`notes_embed_paths`] (Story 46.11), which asks about a list of
/// targets and wants one answer per target rather than the first refusal. There
/// is no second resolver: a panel that decided for itself which file an embed
/// names would list one file where the viewer opens another and the export
/// carries a third.
fn embed_path_opt(vault: &Vault, target: &str) -> Option<(String, PathBuf)> {
    for rel in embed::candidates(target, ATTACHMENTS_DIR) {
        // `contained_read` is the whole containment check and it is stricter
        // than "inside the vault": it refuses `..`, canonicalises so a symlink
        // out of the vault cannot escape, and — because it stats with
        // `symlink_metadata` — refuses a symlink, a fifo and a device outright.
        // That last part matters more here than it does for a read-only asset:
        // `atomic_write` finishes with a `rename`, which REPLACES a symlink with
        // a regular file rather than writing through it, so an editable table
        // over a symlink would quietly destroy the link on the first edit. It
        // never gets that far.
        //
        // The `is_file` filter is therefore belt-and-braces, and deliberately
        // kept: it makes this function's precondition true on its own terms
        // rather than by depending on which `stat` another module chose.
        let found = crate::note_protocol::contained_read(vault, &rel);
        if let Some(path) = found.filter(|candidate| candidate.is_file()) {
            return Some((rel, path));
        }
    }
    None
}

/// Read a CSV attachment as text, or refuse in a sentence.
///
/// The two refusals are the ones a real vault produces. A file over
/// [`csv::MAX_CSV_BYTES`] is not opened as a table at all — the cells would
/// cross IPC as JSON and land in the DOM, and a 6 GB export is exactly the case
/// `keeper-sync`'s LFS threshold already names. A file that is not UTF-8 is a
/// Latin-1 export, and keeper declines rather than guessing an encoding: a
/// wrong guess would write the wrong bytes back over somebody's data.
///
/// Both are `warn!`, not `debug!`: a refusal is a problem the user is asking
/// about, and only `WARN` and above reach the on-disk log while debug mode is
/// off (see `debug_log::GatedMakeWriter`) — which is the default, and the state
/// the machine is in when the thing goes wrong.
fn read_csv(rel: &str, path: &Path) -> Result<String, IpcError> {
    let size = std::fs::metadata(path)
        .map_err(|error| notes_error(NotesError::NotFound(format!("{rel}: {error}"))))?
        .len();
    if size > csv::MAX_CSV_BYTES {
        let message = csv::too_large_notice(rel, size);
        tracing::warn!(%rel, bytes = size, "notes: csv too large to open as a table");
        return Err(IpcError {
            code: IpcErrorCode::Unsupported,
            message,
            account_id: None,
            retriable: false,
        });
    }
    std::fs::read_to_string(path).map_err(|error| {
        tracing::warn!(%rel, %error, "notes: csv is not valid UTF-8 or could not be read");
        IpcError {
            code: IpcErrorCode::NotesInvalid,
            message: format!(
                "{rel} is not UTF-8 text, so keeper cannot show it as a table without \
                 guessing an encoding — and a wrong guess would write the wrong bytes back"
            ),
            account_id: None,
            retriable: false,
        }
    })
}

/// A CSV attachment projected as a table (Story 44.16, FR-172).
#[tauri::command]
pub async fn notes_csv_read(vault_id: String, target: String) -> Result<NoteCsvVm, IpcError> {
    let vault = vault_of(&vault_id)?;
    let (rel, path) = embed_path(&vault, &target)?;
    let text = read_csv(&rel, &path)?;
    let rev = notes_vault::content_rev(&text);
    Ok(csv::project(&text, rel, rev))
}

/// Write one cell and return the table the file now is (Story 44.16, FR-172).
///
/// **One write path, and it is the vault's own.** `write_vault_file` is the same
/// temp-and-rename `write_note` uses, so a `kill -9` mid-write leaves no torn
/// CSV, and `mark_dirty` is the same announcement `import_attachment` makes so
/// the commit cadence picks the change up. What is deliberately absent is
/// `touch`: that asks the reconciler to re-read a path, and the notes walk never
/// collects a `.csv`, so it would be a request for an index entry that cannot
/// exist. Nothing here reaches into the sync engine — the engine's own
/// `EchoSuppressor` is engine-internal by design, and this write is meant to be
/// seen by the watcher, because a file the user edited is a file that must be
/// committed.
///
/// `rev` is the revision the table was read at. A file that changed underneath —
/// a sync pull, the user's spreadsheet — is refused rather than clobbered, on
/// the same reasoning `notes_save`'s `base_rev` carries; there is no conflict
/// copy here because there is nothing of the user's to lose yet, only a stale
/// table to reload.
#[tauri::command]
pub async fn notes_csv_set_cell(
    vault_id: String,
    target: String,
    rev: String,
    row: u32,
    column: u32,
    value: String,
) -> Result<NoteCsvVm, IpcError> {
    let vault = vault_of(&vault_id)?;
    let (rel, path) = embed_path(&vault, &target)?;
    let text = read_csv(&rel, &path)?;
    let disk_rev = notes_vault::content_rev(&text);
    if disk_rev != rev {
        tracing::warn!(%rel, "notes: csv changed on disk since the table was read");
        return Err(IpcError {
            code: IpcErrorCode::NotesInvalid,
            message: format!(
                "{rel} changed on disk since this table was opened, \
                 so the edit was not applied; the table has been reloaded"
            ),
            account_id: None,
            retriable: true,
        });
    }

    let written = csv::set_cell(&text, row as usize, column as usize, &value).map_err(|error| {
        tracing::info!(%rel, "notes: csv cell edit refused: {error}");
        IpcError {
            code: IpcErrorCode::NotesInvalid,
            message: error.to_string(),
            account_id: None,
            retriable: false,
        }
    })?;

    if written == text {
        // The cell already held this. Saying so out loud rather than writing
        // identical bytes: a no-op write still makes a commit, a sync round and
        // a diff on every machine the vault reaches (DW-162 — a code path that
        // declines to act has to be able to say it declined).
        tracing::info!(%rel, row, column, "notes: csv cell unchanged, nothing written");
        return Ok(csv::project(&text, rel, disk_rev));
    }

    notes_vault::write_vault_file(&vault, &rel, &written).map_err(notes_error)?;
    notes_vault::mark_dirty(&vault.id);
    tracing::info!(%rel, row, column, "notes: csv cell written");
    let next_rev = notes_vault::content_rev(&written);
    Ok(csv::project(&written, rel, next_rev))
}

/// A file embedded in a note, as text an editor can show (Story 45.12, FR-186).
///
/// **The vault-scoped sibling of `sync_read_text`, and it exists because the
/// two surfaces are addressed differently.** Story 45.6's reader takes a sync
/// profile id and a profile-relative subpath; a note holds a notes vault id and
/// the text between a pair of brackets. Deriving one from the other in the
/// webview would be the frontend deciding which folders are vaults, which is
/// the path arithmetic AD-65 forbids, and doing it here would duplicate the
/// profile→vault resolution Story 45.18 owns. So this command answers the
/// question the note can actually ask.
///
/// What is deliberately NOT duplicated is the reading: [`text_file`] is Story
/// 45.6's one reader, with its one size limit and its one "these bytes are not
/// text" answer, so a file too large to edit in a panel is too large to edit in
/// a note and neither surface has its own opinion about where that line is.
#[tauri::command]
pub async fn notes_embed_read(vault_id: String, target: String) -> Result<NoteEmbedVm, IpcError> {
    let vault = vault_of(&vault_id)?;
    let (rel, path) = embed_path(&vault, &target)?;
    let file = keeper_core::text_file::open_text_file(&path).map_err(|error| {
        tracing::warn!(%rel, %error, "notes: could not read an embedded file");
        notes_error(NotesError::NotFound(format!("{rel}: {error}")))
    })?;
    Ok(embed::describe(rel, file))
}

/// Which of these embed targets this vault actually holds (Story 46.11).
///
/// **The panel named Attachments needed a containment check and had none.**
/// Story 46.2 gave it a purely syntactic reader — an embed under `attachments/`
/// is an attachment — because the webview holds the *unsaved* buffer and has no
/// disk, and because the only attach path at the time copied every file into
/// that folder. Story 46.11 makes an in-vault attach point at the file where it
/// already lives, so the prefix stopped being the test: what makes a row is that
/// the note embeds a file and the vault holds it, wherever it lives (epic 46's
/// spine). "The vault holds it" is a `stat`, and this is the `stat`.
///
/// **One resolver, not a second one.** [`embed_path_opt`] is the same function
/// [`notes_embed_read`], [`notes_csv_read`] and the `keeper-note://` protocol
/// resolve through, and it is `embed::candidates` order — so the panel lists the
/// file the viewer opens and `export::plan` carries. A reader that re-derived
/// the candidates in the webview would be the second answer to one question that
/// AD-103 and this story both exist to remove.
///
/// One answer per target, in the order asked, `None` for a target the vault does
/// not hold. Each answer carries the resolved path **and** the file's kind —
/// Story 55.4, so a decoration can draw a photograph inside a note without the
/// webview classifying anything (AD-87) and without reading the file to find
/// out what it is. Never a rejection for a missing file: "this note embeds something
/// that is not here" is a fact the surface must render, and a rejected promise
/// gives it nothing to render — and one moved photograph must not blank the rest
/// of the list.
///
/// No sentence comes back, unlike [`embed_path`]'s refusal. The caller is
/// looking at a list of rows rather than at one broken embed, and Story 45.12
/// already says which paths keeper tried, at the embed, where the person is
/// pointing.
#[tauri::command]
pub async fn notes_embed_paths(
    vault_id: String,
    targets: Vec<String>,
) -> Result<Vec<Option<NoteEmbedPathVm>>, IpcError> {
    let vault = vault_of(&vault_id)?;
    // Blocking: `contained_read` canonicalises, which stats every component, and
    // a note may embed a dozen files. On the async runtime that would stall
    // every other command on this thread for as long as the slowest volume takes.
    tokio::task::spawn_blocking(move || {
        targets
            .iter()
            .map(|target| {
                embed_path_opt(&vault, target).map(|(rel, path)| NoteEmbedPathVm {
                    // From the resolved file's own name, not from the target the
                    // note spells: `![[photo]]` with no extension resolves to
                    // `attachments/photo.png`, and it is the file that decides
                    // what the file is.
                    kind: kind_for_file_name(
                        &path
                            .file_name()
                            .map(|name| name.to_string_lossy().into_owned())
                            .unwrap_or_else(|| rel.clone()),
                    ),
                    rel_path: rel,
                })
            })
            .collect()
    })
    .await
    .map_err(|error| notes_error(NotesError::Name(format!("embed paths: {error}"))))
}

/// Write an embedded file's raw bytes back (Story 45.12, FR-187).
///
/// **The whole buffer, and the same writer everything else in the vault uses.**
/// `write_vault_file` is the temp-and-rename `write_note` and `notes_csv_set_cell`
/// both go through, so a `kill -9` mid-write leaves no torn file, and
/// `mark_dirty` is the same announcement, so the commit cadence picks the change
/// up. There is no `touch`: the notes walk collects `.md` and nothing else, so
/// asking the reconciler to re-read a `.csv` would be asking for an index entry
/// that cannot exist.
///
/// **A note is refused.** [`embed::write_refusal`] holds that rule and its
/// wording. A `.md` written here would bypass `notes_save`'s `base_rev`, its
/// conflict copy and its reindex — so a stale buffer in one machine's embed
/// would silently overwrite a note edited on another, with nothing left behind
/// to recover. The frontend does not route a markdown embed here, and that is
/// exactly why this guard is here: a rule enforced only by the caller that
/// happens to exist today is enforced by nothing.
///
/// No revision is carried, unlike `notes_csv_set_cell`. That command splices one
/// field into bytes it did not read again, so it must prove the bytes have not
/// moved; this is a whole-file save of a buffer the reader is looking at, which
/// is the same contract `sync_write_entry` gives the Files surface for the same
/// file. Two answers to "may I save this" for one file, differing by which
/// surface you opened it from, would be worse than either.
#[tauri::command]
pub async fn notes_embed_write(
    vault_id: String,
    target: String,
    content: String,
) -> Result<(), IpcError> {
    let vault = vault_of(&vault_id)?;
    let (rel, _) = embed_path(&vault, &target)?;
    if let Some(refusal) = embed::write_refusal(&rel, notes_vault::extension(&rel).as_deref()) {
        tracing::warn!(%rel, "notes: refused to write a note through an embed");
        return Err(IpcError {
            code: IpcErrorCode::NotesInvalid,
            message: refusal,
            account_id: None,
            retriable: false,
        });
    }
    notes_vault::write_vault_file(&vault, &rel, &content).map_err(notes_error)?;
    notes_vault::mark_dirty(&vault.id);
    tracing::info!(%rel, bytes = content.len(), "notes: wrote an embedded file");
    Ok(())
}

// ---------------------------------------------------------------------------
// Capture
// ---------------------------------------------------------------------------

/// The note one quick-capture window is holding, creating it when there is
/// none (FR-101, FR-190, AD-93; Story 45.14).
///
/// **This replaced the three `notes_capture_buffer*` commands and
/// `commit_buffer`.** Those existed because quick capture was a textarea whose
/// text had nowhere to live until Escape assembled a note out of it. Quick
/// capture now mounts the real note editor, and a tag is frontmatter on a note
/// while an attachment is a file copied relative to a note's path — neither can
/// be applied to a `String` in the settings table. So the note exists before
/// the first keystroke and the editor's own autosave is the durability, which
/// is strictly stronger than the 300 ms debounce it replaced.
///
/// **Idempotent, and that is the whole design.** A page nobody has written on
/// is handed back unchanged, so summoning the panel and dismissing it without
/// typing leaves nothing behind; the first thought written on a page tears it
/// off, and the next call makes a fresh one. The decision is taken here, from
/// the bytes on disk, rather than from a claim the window makes about itself —
/// a caller that says "I did not type anything" is a caller that can be wrong,
/// and being wrong in one direction litters the vault while being wrong in the
/// other buries one thought under the next.
///
/// `key` names the window (Story 45.15). One global slot would be two capture
/// windows holding each other's note.
///
/// The create goes through [`create_note`] — the one path `notes_create` uses,
/// with 44.6's `notices` out-parameter attached rather than discarded, which is
/// how a capture whose configured template could not be read now says so. The
/// old commit path passed `&mut Vec::new()` because it had no surface to show a
/// sentence on. It has one now.
#[tauri::command]
pub async fn notes_capture_draft(
    state: State<'_, AppState>,
    key: String,
) -> Result<NoteCreateVm, IpcError> {
    resolve_capture_draft(state.platform.as_ref(), &key)
}

/// Hand back this window's untouched page, or make it a new one.
fn resolve_capture_draft(
    platform: &dyn keeper_core::platform::Platform,
    key: &str,
) -> Result<NoteCreateVm, IpcError> {
    let data_dir = platform
        .data_dir()
        .map_err(|error| notes_error(NotesError::Name(error.to_string())))?;
    // Refused up front rather than after the window has taken keystrokes:
    // there is nowhere to put a thought, and a panel that accepts words it
    // cannot keep is the failure capture exists to prevent.
    let vault_id = notes_vault::active_vault(platform).ok_or_else(|| IpcError {
        code: IpcErrorCode::Unsupported,
        message: "no notes vault yet — flag a folder you already sync and it becomes one"
            .to_owned(),
        account_id: None,
        retriable: false,
    })?;
    let vault = vault_of(&vault_id)?;

    let held = keeper_core::registry::get_capture_draft(&data_dir, key)
        .map_err(|error| notes_error(NotesError::Name(error.to_string())))?;
    if let Some(held) = held {
        if let Some(note) = untouched_draft(&vault, &held) {
            return Ok(NoteCreateVm {
                note,
                notices: Vec::new(),
            });
        }
    }

    let mut notices = Vec::new();
    let note = create_note(
        &vault,
        // Spelled out rather than through `blank_note()`, which is
        // `#[cfg(desktop)]`: nothing about resolving a page is desktop-only,
        // and a helper that makes this function unbuildable on another target
        // is a coupling with no reason behind it.
        &NoteCreateReq {
            title: None,
            body: None,
            template: None,
            dest: None,
            tags: Vec::new(),
            space: None,
        },
        // Everything a capture carries, from the one producer (Story 45.16):
        // the reserved `keeper.capture` mark the Inbox lens has always read,
        // plus the vault's configured capture tag when there is one. Both in
        // `keeper-core` because "what is a capture" must have one answer, and
        // because the tag interacts with every space's query — which is a
        // decision this crate cannot prove on the build host (AD-56).
        &seed::capture(vault.config.capture_tag.as_deref()),
        // Capture has no space, so no space template. The vault's capture
        // template — and, failing that, its default — applies inside
        // `template_source`, which takes the capture rung from this seed's own
        // mark.
        None,
        &mut notices,
    )?;

    // What creation put on the page, read back from the file rather than
    // assembled here: `create_note` expands a template, and a pristine snapshot
    // that disagreed with the bytes by one placeholder would make every page
    // look written-on and tear off a fresh note per dismissal.
    //
    // `else` rather than `unwrap_or_default()`. A note keeper wrote and then
    // could not read back is rare — a volume that went away between the two —
    // but defaulting to the empty string would record a scaffolded page as
    // having been created blank, and every later comparison would then read it
    // as written-on. That is the tear-off-every-time failure this snapshot
    // exists to prevent, arrived at from the other side. No pointer at all is
    // honest: the next summon makes one fresh note and says why in the log.
    let Some(pristine) = draft_body(&vault, &note.path) else {
        tracing::info!(
            %key,
            note = %note.path,
            "notes: captured a page but could not read it back; not remembering it, so the next summon makes a fresh one"
        );
        return Ok(NoteCreateVm { note, notices });
    };
    let pointer = keeper_core::registry::CaptureDraft {
        note_id: note.id.clone(),
        pristine,
    };
    if let Err(error) = keeper_core::registry::set_capture_draft(&data_dir, key, Some(&pointer)) {
        // The note exists and is returned. Losing the pointer costs one extra
        // untouched note on the next summon, never the thought — said at `INFO`
        // because a decision only the return value carries is one nobody can
        // debug from a log (DW-162).
        tracing::info!(
            %error,
            %key,
            note = %note.path,
            "notes: captured a page but could not remember it; the next summon makes a fresh one"
        );
    }
    Ok(NoteCreateVm { note, notices })
}

/// This window's held page, when the note is still there and still says exactly
/// what creation put on it.
///
/// `None` for every other case — the note was deleted, the vault has not
/// finished indexing, the file cannot be read, somebody wrote in it — and every
/// one of those means the same thing to the caller: make a fresh page. They are
/// not distinguished because the answer does not differ, and a note that cannot
/// be re-read is not a note capture should hand back.
fn untouched_draft(vault: &Vault, held: &keeper_core::registry::CaptureDraft) -> Option<NoteRefVm> {
    let snapshot = notes_vault::snapshot(&vault.id)?;
    let entry = snapshot.by_id(&held.note_id)?;
    let body = draft_body(vault, &entry.path)?;
    if !held.is_untouched(&body) {
        return None;
    }
    Some(NoteRefVm {
        vault_id: vault.id.clone(),
        id: entry.id.clone(),
        path: entry.path.clone(),
        title: entry.title.clone(),
    })
}

/// One note's body — the file with its frontmatter block taken off.
///
/// The body and not the file, because `updated` is stamped into the block on
/// every write: comparing whole files would call a page written-on the moment
/// anything touched it, including keeper itself.
fn draft_body(vault: &Vault, rel: &str) -> Option<String> {
    let source = notes_vault::read_note(vault, rel).ok()?;
    let (_, body_at) = Frontmatter::parse(&source);
    Some(source.get(body_at..).unwrap_or_default().to_owned())
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
    // The counts last sent, so a change that moves no row but does move the
    // count still reaches the surface. Before Story 44.11 nothing carried the
    // count after the opening `Reset`, and a note that started matching the
    // lens below the page produced no op and no message — invisible then,
    // because nothing showed the number, and a stale count on screen now.
    let mut sent: Option<(u32, u32)> = None;
    // The opening snapshot, then one message per change at most every
    // CHANGE_BATCH_MS. `while let` rather than `loop`: the window read is the
    // condition — a vault that stops answering has nothing left to stream.
    while let Ok(rows) = current_window(platform.as_ref(), &vault) {
        let next = fingerprints(&rows.rows);
        let counts = (rows.total, rows.matched);
        let ops = if previous.is_empty() {
            vec![NoteListOp::Reset { rows: rows.rows }]
        } else {
            diff_ops(&previous, &next, &rows.rows)
        };
        previous = next;
        if (!ops.is_empty() || sent != Some(counts))
            && channel
                .send(NoteChangeBatch {
                    vault_id: vault.id.clone(),
                    ops,
                    total: counts.0,
                    matched: counts.1,
                })
                .is_err()
        {
            // The webview is gone; a closed window unsubscribes itself.
            return;
        }
        sent = Some(counts);
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
        tags: std::collections::BTreeMap::new(),
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
    // The tray creates outside any space, and it has no surface to show a
    // notice on. A missing default template is logged at `INFO` regardless.
    match create_note(
        &vault,
        &blank_note(),
        &seed::Seed::default(),
        None,
        &mut Vec::new(),
    ) {
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
        space: None,
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

/// Hide the quick-capture panel (NFR-27, AD-60).
///
/// Hide, and nothing else. It used to take `commit: bool` and assemble a note
/// out of the panel's text buffer, because until Story 45.14 there was no note
/// to assemble one from. The panel now holds a real note that has been
/// autosaving since before the first keystroke, and the caller force-flushes it
/// (AD-62) before asking for this — so there is nothing left for a flag to
/// mean, and a `commit: false` that hid without writing would be a way to lose
/// words that no longer exists.
#[cfg(desktop)]
#[tauri::command]
pub async fn notes_capture_hide(app: AppHandle) -> Result<(), IpcError> {
    crate::notes_window::hide(&app);
    // Dismissing the panel is how a captured thought most often ends, so the
    // vault's commit cadence is forced here rather than waited for (AD-62).
    notes_vault::flush();
    Ok(())
}

/// Open — or raise — the capture window holding `target` (Story 45.15, FR-191).
///
/// The entry point behind "any note opens as a capture window", and the reason
/// the small window stops being a special kind of note: a capture window is a
/// *view* of a note, addressed the same way a panel addresses one.
///
/// Idempotent by identity. Asking twice for the same note raises the window
/// that is already there, because the label is derived from the target rather
/// than handed out by a counter.
#[cfg(desktop)]
#[tauri::command]
pub async fn notes_capture_open(
    app: AppHandle,
    state: State<'_, AppState>,
    target: keeper_core::capture::CaptureTargetVm,
) -> Result<(), IpcError> {
    let data_dir = capture_data_dir(state.platform.as_ref())?;
    let key = keeper_core::capture::capture_key(&target);
    let placement = keeper_core::registry::get_capture_placement(&data_dir, &key)
        .map_err(|error| notes_error(NotesError::Name(error.to_string())))?;
    crate::notes_window::open(&app, &target, placement);
    Ok(())
}

/// Close the capture window `key` (Story 45.15, FR-191).
///
/// The close button's command, and Escape's. Where it goes — hidden for the
/// prewarmed window, destroyed for any other, main window raised when nothing
/// else is left on screen — is
/// [`keeper_core::capture::plan_close`]'s decision, not this command's.
///
/// The window's geometry is written down on the way out — where it was, and
/// since Story 46.15 how big the user made it. This is the moment a placement
/// is persisted, rather than on every `Moved`/`Resized` event, because a drag
/// emits one event per compositor frame and a settings write per frame would
/// put a sqlite transaction inside a gesture.
#[cfg(desktop)]
#[tauri::command]
pub async fn notes_capture_close(
    app: AppHandle,
    state: State<'_, AppState>,
    key: String,
) -> Result<(), IpcError> {
    let geometry = crate::notes_window::close(&app, &key);
    remember_placement(state.platform.as_ref(), &key, geometry);
    // Dismissing a capture is how a captured thought most often ends, so the
    // vault's commit cadence is forced here rather than waited for (AD-62).
    notes_vault::flush();
    Ok(())
}

/// Lock or unlock the capture window `key` (Story 45.15, FR-192, UX-DR77;
/// Story 46.15).
///
/// Locked is keeper's geometry and a window the user can neither move nor
/// resize; unlocked is the user's, and a window they can do both to. The
/// current position and size are snapshotted on **either** transition rather
/// than only on a gesture, because a person who unlocks a window and never
/// touches it has still said "this is where it goes", and a person who locks
/// one after moving and resizing it has said "keep it *there*" — locking is not
/// a discard button.
///
/// The live window is updated after the write, so the toggle takes effect
/// without a reopen. That is visible on lock: the window snaps back to keeper's
/// own 560×340 while keeping its position, which is deliberate. A locked window
/// IS keeper's size, and the alternative is a window that looks one size now
/// and jumps to another the next time it opens — the same surprise, delivered
/// later and unattached to the click that caused it. The remembered size is
/// kept, so unlocking restores it.
///
/// **Story 48.2 made that last sentence true.** This command used to build the
/// placement inline, merging `live.size.or(stored.size)` on both transitions —
/// so on the *unlock* click, where the live window is the 560×340 the *lock*
/// just normalised it to, it wrote keeper's own size over the user's a moment
/// before anything could restore it. There is no geometry logic left here:
/// [`keeper_core::capture::Placement::relocked`] decides what is written, in
/// the crate that compiles everywhere, and this reads, calls, writes, applies.
#[cfg(desktop)]
#[tauri::command]
pub async fn notes_capture_set_locked(
    app: AppHandle,
    state: State<'_, AppState>,
    key: String,
    locked: bool,
) -> Result<(), IpcError> {
    let data_dir = capture_data_dir(state.platform.as_ref())?;
    let stored = keeper_core::registry::get_capture_placement(&data_dir, &key)
        .map_err(|error| notes_error(NotesError::Name(error.to_string())))?;
    let live = crate::notes_window::geometry_of(&app, &key);
    let placement = stored.relocked(live, locked);
    keeper_core::registry::set_capture_placement(&data_dir, &key, &placement)
        .map_err(|error| notes_error(NotesError::Name(error.to_string())))?;
    crate::notes_window::adopt_placement(&app, &key, placement);
    crate::notes_window::announce(&app);
    Ok(())
}

/// Every capture window open right now, with what it holds and whether it is
/// locked (Story 45.15, FR-191).
///
/// One command for two readers: the main window renders the list, and a capture
/// window finds its own row in it by key. A second "what am I?" command would
/// be a second answer to one question, and the two would disagree the first
/// time a window was closed while another was reading.
#[cfg(desktop)]
#[tauri::command]
pub async fn notes_capture_windows(
    app: AppHandle,
    state: State<'_, AppState>,
) -> Result<Vec<keeper_core::capture::CaptureWindowVm>, IpcError> {
    let data_dir = capture_data_dir(state.platform.as_ref())?;
    Ok(crate::notes_window::list(&app, &|key| {
        keeper_core::registry::get_capture_placement(&data_dir, key).unwrap_or_default()
    }))
}

/// Write down where a capture window is and how big it is, keeping whatever the
/// lock says.
///
/// Called on the way out of a window and on blur, so a geometry survives a quit
/// that nobody asked politely for. Never fails a caller: a placement that could
/// not be stored costs the user a remembered position and size, and refusing to
/// close a window over it would cost them the window.
///
/// A half-answer is written: a platform that reports a size but not a position
/// still gets its size remembered, because the two are independent facts and
/// discarding the readable one would make an unrelated platform quirk look like
/// a bug in the resize. Neither known is nothing to write.
///
/// **And since Story 48.2 a locked window writes nothing at all.** This is the
/// path that made the lock a discard button without anybody pressing the
/// padlock twice: blur fires when a person clicks another app, and a locked
/// window's live geometry is keeper's own — the normalised 560×340, at whatever
/// coordinate the last hotkey press placed it (Story 47.5, DW-198). Merging
/// that over the row wiped both halves of what the user had chosen, so one
/// click elsewhere after locking was enough to lose them for good.
/// [`keeper_core::capture::Placement::observing`] holds the rule, so this path
/// and the lock toggle cannot come to different conclusions about it.
#[cfg(desktop)]
pub(crate) fn remember_placement(
    platform: &dyn keeper_core::platform::Platform,
    key: &str,
    live: keeper_core::capture::Observed,
) {
    if live.position.is_none() && live.size.is_none() {
        return;
    }
    let Ok(data_dir) = platform.data_dir() else {
        return;
    };
    let stored = keeper_core::registry::get_capture_placement(&data_dir, key).unwrap_or_default();
    let placement = stored.observing(live);
    // Nothing learned, nothing written. A locked window lands here on every
    // blur, and a settings write that restates the row it just read is a
    // transaction bought with no information.
    if placement == stored {
        return;
    }
    if let Err(error) = keeper_core::registry::set_capture_placement(&data_dir, key, &placement) {
        tracing::warn!(%error, %key, "notes: could not remember the capture window's geometry");
    }
}

/// The data directory, or a notes-shaped error. Shared by the capture window
/// commands so the three of them cannot disagree about how a missing data
/// directory reads.
#[cfg(desktop)]
fn capture_data_dir(
    platform: &dyn keeper_core::platform::Platform,
) -> Result<std::path::PathBuf, IpcError> {
    platform
        .data_dir()
        .map_err(|error| notes_error(NotesError::Name(error.to_string())))
}

/// Pin or un-pin the capture window `key` (Story 48.4).
///
/// The third button on the chrome strip, beside the lock, and deliberately the
/// same shape of command: read the stored placement, write the one field back,
/// then apply it to the live window so the toggle takes effect without a
/// reopen.
///
/// # Why this is a Rust command and not `getCurrentWindow().setAlwaysOnTop()`
///
/// The webview could not do it. `quick-capture.json` grants no `core:window`
/// permissions at all, so the call would be denied — and denied *quietly*, as
/// a rejected promise inside a click handler nobody awaits. That is Story
/// 46.15's argument for `set_resizable` verbatim, and the reason this story
/// adds no capability: the flag is persisted state that outlives the document,
/// so it has to be written in Rust regardless, and once it is, applying it
/// there too costs one line and removes a whole class of silent failure.
///
/// # Why the geometry is not touched
///
/// Unlike the lock, this changes nothing about where or how big the window is,
/// so it must not snapshot or re-assert either. Reading the live geometry here
/// would re-introduce Story 48.2's defect on a new path: the live window's size
/// is whatever it is at this instant, and merging it over the stored one is how
/// a remembered size gets overwritten by a normalised one.
#[cfg(desktop)]
#[tauri::command]
pub async fn notes_capture_set_always_on_top(
    app: AppHandle,
    state: State<'_, AppState>,
    key: String,
    always_on_top: bool,
) -> Result<(), IpcError> {
    let data_dir = capture_data_dir(state.platform.as_ref())?;
    let stored = keeper_core::registry::get_capture_placement(&data_dir, &key)
        .map_err(|error| notes_error(NotesError::Name(error.to_string())))?;
    let placement = keeper_core::capture::Placement {
        always_on_top,
        ..stored
    };
    keeper_core::registry::set_capture_placement(&data_dir, &key, &placement)
        .map_err(|error| notes_error(NotesError::Name(error.to_string())))?;
    crate::notes_window::set_always_on_top(&app, &key, always_on_top);
    // The chrome reads its pressed state out of the window list, so the list
    // has to be re-announced or the button springs back on the next hydrate.
    crate::notes_window::announce(&app);
    Ok(())
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

/// Copy a note, and every file it shows, to a location the user picked
/// (Story 45.21, FR-199).
///
/// **What "export a note" means, and why.** The note's bytes are copied
/// unchanged and its embedded files are copied to the *same vault-relative
/// paths* beneath a new folder named after the note. Not the markdown alone,
/// which lands somewhere its `![[attachments/photo.png]]` means nothing; and
/// not the markdown with its links rewritten, which would mean the exported
/// file is no longer the note and cannot be diffed against the vault's copy.
/// The neighbourhood is reproduced instead of the links being edited. The full
/// argument is in `keeper_core::notes::export`'s module doc.
///
/// **Three crates, one decision each.** Which files the note needs is
/// `keeper_core::notes::export::plan` — pure, and asked through the same
/// candidate order the embed viewer resolves in, so an export carries the file
/// the note *renders*. Whether and where they may be copied is
/// `keeper_sync::export`. This function is the adapter that holds a vault: it
/// supplies the on-disk probe, which is `note_protocol::contained_read` — the
/// same containment check the embed viewer uses, so a symlink out of the vault
/// cannot be exported by naming it in an embed.
///
/// **The buffer is not consulted, and the surface flushes first.** This reads
/// what is on disk, because an export is a copy of a file. The editor saves
/// continuously and the Export control saves before calling, so what lands is
/// what the person can see — a detail that belongs to the surface, because
/// this command must stay usable for a note nobody has open.
///
/// Runs on the blocking pool: a note with forty photographs is forty copies.
///
/// Rejects with: `internal` (no such vault, no such note, an unreadable note, a
/// destination that is missing / is a file / is inside the vault / already
/// holds the name, or a copy the disk refused). A note whose embed has been
/// moved is NOT a rejection — it exports, and the receipt names what did not
/// go.
#[cfg(desktop)]
#[tauri::command]
pub async fn notes_export(
    vault_id: String,
    note_id: String,
    destination: String,
) -> Result<ExportReceiptVm, IpcError> {
    let vault = vault_of(&vault_id)?;
    let entry = entry_of(&vault_id, &note_id)?;
    let source = notes_vault::read_note(&vault, &entry.path).map_err(notes_error)?;
    let body = split_note(&source).1.to_owned();
    let rel = entry.path.clone();
    let target = PathBuf::from(&destination);

    let named = rel.clone();
    let (plan, done) = tokio::task::spawn_blocking(move || {
        let plan = keeper_core::notes::export::plan(&body, ATTACHMENTS_DIR, &|candidate: &str| {
            crate::note_protocol::contained_read(&vault, candidate).is_some()
        });
        keeper_sync::export::export_note(&vault.root, &rel, &plan.attachments, &target)
            .map(|done| (plan, done))
    })
    .await
    .map_err(|error| {
        notes_error(NotesError::Name(format!(
            "could not export {named}: {error}"
        )))
    })?
    .map_err(|refusal| crate::sync_ipc::export_refused(&refusal))?;

    tracing::info!(
        rel = %named,
        carried = plan.attachments.len(),
        missing = plan.missing.len(),
        "notes: exported a note out of keeper"
    );
    Ok(ExportReceiptVm::note(
        done.path.display().to_string(),
        keeper_sync::export::file_name_of(&named),
        done.written,
        plan,
    ))
}

#[cfg(test)]
mod tests {
    use keeper_core::notes::index::NoteTagTerm;
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
            order: keeper_core::notes::order::NoteOrder::default(),
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
                space: None,
            },
            &seed::Seed::default(),
            None,
            &mut Vec::new(),
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
            order: keeper_core::notes::order::NoteOrder::default(),
        }
    }

    /// The chip axis, exercised through this call site. The rule itself is
    /// `keeper-core`'s and tested there against every spelling and every segment
    /// boundary; what only this side can go wrong at is the wiring — a shell that
    /// folds the chips once per query and then forgets to hand them to the
    /// predicate would filter nothing and look entirely correct doing it.
    #[test]
    fn the_shell_hands_the_chip_terms_to_the_core_predicate() {
        let mut note = entry("a.md", "a");
        note.tags = vec!["client/acme".to_owned(), "draft".to_owned()];
        let req = |terms: &[(&str, NoteTagTerm)]| NoteQueryReq {
            tags: terms
                .iter()
                .map(|(tag, term)| ((*tag).to_owned(), *term))
                .collect(),
            ..default_query()
        };

        let included = req(&[("Client/Acme ", NoteTagTerm::Include)]);
        assert!(matches_filter(
            &note,
            &included,
            &TagTerms::new(&included.tags),
            None
        ));

        let excluded = req(&[("draft", NoteTagTerm::Exclude)]);
        assert!(!matches_filter(
            &note,
            &excluded,
            &TagTerms::new(&excluded.tags),
            None
        ));
    }

    #[test]
    fn the_default_lens_hides_conflict_copies_and_archived_notes() {
        let req = default_query();
        let mut conflict = entry("a.sync-conflict-20260802-120000-mini.md", "a");
        conflict.flags.push("conflict".to_owned());
        assert!(!matches_filter(&conflict, &req, &TagTerms::default(), None));

        let mut archived = entry("b.md", "b");
        archived.flags.push("archived".to_owned());
        assert!(!matches_filter(&archived, &req, &TagTerms::default(), None));

        // Asked for by name, they appear.
        let asked = NoteQueryReq {
            flags: vec!["conflict".to_owned()],
            ..default_query()
        };
        assert!(matches_filter(
            &conflict,
            &asked,
            &TagTerms::default(),
            None
        ));
        // A plain note is in the default lens.
        assert!(matches_filter(
            &entry("c.md", "c"),
            &req,
            &TagTerms::default(),
            None
        ));
    }

    /// The list's free-text axis, exercised through this call site: the predicate
    /// is `keeper-core`'s, and what this asserts is that the shell hands it the
    /// whole entry — including the frontmatter, which is where a recording note
    /// keeps every fact about itself.
    #[test]
    fn a_text_filter_matches_the_title_the_path_the_tags_and_the_frontmatter() {
        let mut note = entry("journal/2026-08-02.md", "Vault as a lens");
        note.tags.push("project/keeper".to_owned());
        note.fields
            .insert("participants".to_owned(), "Ala Kowalska".to_owned());
        let filter = |text: &str| NoteQueryReq {
            text: Some(text.to_owned()),
            ..default_query()
        };
        let hit = |text: &str| matches_filter(&note, &filter(text), &TagTerms::default(), None);
        assert!(hit("vault"));
        assert!(hit("LENS"));
        assert!(hit("journal/"));
        assert!(hit("keeper"));
        // Nowhere in the title, the path, the tags or the body.
        assert!(hit("kowalska"));
        assert!(!hit("nothing here"));
        // An empty needle is not a filter.
        assert!(hit("   "));
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
                capture_template: None,
                capture_tag: None,
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
                capture_template: None,
                capture_tag: None,
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
                capture_template: None,
                capture_tag: None,
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
        assert_eq!(def.limit, Some(500));

        // A note with no definition at all is a space with an empty query, not a
        // parse failure — an empty query is what the UI shows as "not defined
        // yet".
        let bare = space_def(&entry("spaces/new.md", "New"), "---\nid: x\n---\n");
        assert!(bare.query.is_empty());
        assert_eq!(
            bare.limit, None,
            "a space with no `keeper.limit` sets no cap; it does not inherit \
             the page size as one (Story 44.11, DW-163)"
        );
    }

    /// The wiring, not the rule: what a `limit` value MEANS is
    /// `keeper-core::notes::counts::read_limit`'s and is proved there on this
    /// host. What is proved only here is that a `keeper.limit` in frontmatter
    /// reaches that reader, and that a key holding nonsense leaves the space
    /// uncapped rather than capped at something arbitrary.
    #[test]
    fn a_frontmatter_limit_reaches_the_one_reader_of_it() {
        let capped = space_def(
            &entry("spaces/recent.md", "Recent"),
            "---\nid: x\nkeeper:\n  space: 'is:pinned'\n  limit: 20\n---\n",
        );
        assert_eq!(capped.limit, Some(20));

        // Not a number at all: the key is a `Str`, no arm matches, and the
        // space is uncapped. A cap keeper cannot read must never become a cap
        // keeper invented — that would hide notes the file never asked to hide.
        let nonsense = space_def(
            &entry("spaces/odd.md", "Odd"),
            "---\nid: x\nkeeper:\n  space: 'is:pinned'\n  limit: soon\n---\n",
        );
        assert_eq!(nonsense.limit, None);

        let zero = space_def(
            &entry("spaces/zero.md", "Zero"),
            "---\nid: x\nkeeper:\n  space: 'is:pinned'\n  limit: 0\n---\n",
        );
        assert_eq!(zero.limit, None);
    }

    /// Both presentation keys, in the one form the parser can hold, plus what
    /// happens when they hold nonsense.
    ///
    /// Everything the assertions below check about *meaning* is
    /// `keeper-core::notes::sort`'s and is proved there on this host. What is
    /// proved only here — and only on the macOS gate, because this file does not
    /// build on Linux — is the wiring: that `keeper.order` and `keeper.sort`
    /// reach that module at all, and that what it says comes back out.
    #[test]
    fn a_space_definition_reads_its_position_and_its_sort() {
        let positioned = concat!(
            "---\n",
            "id: 01J8ZQ4M7T5R9V3XK2B6C0DFGH\n",
            "keeper:\n",
            "  space: 'is:pinned'\n",
            "  sort: recorded asc\n",
            "  order: -1\n",
            "---\n",
            "\n# Pinned\n"
        );
        let def = space_def(&entry("spaces/pinned.md", "Pinned"), positioned);
        assert_eq!(def.order, -1.0);
        assert_eq!(def.sort, "recorded asc");
        assert!(def.warnings.is_empty());

        // A quoted position is still a position: nobody hand-editing YAML thinks
        // about scalar types, and `order: "2"` is what a template produces.
        let quoted = positioned.replace("  order: -1\n", "  order: '2.5'\n");
        assert_eq!(space_def(&entry("spaces/p.md", "P"), &quoted).order, 2.5);

        // Neither key is required, and a space that names neither says nothing
        // about it — an absent value is not a mistake to report.
        let bare = space_def(&entry("spaces/new.md", "New"), "---\nid: x\n---\n");
        assert_eq!(bare.order, sort::DEFAULT_SPACE_ORDER);
        assert!(bare.sort.is_empty());
        assert!(bare.warnings.is_empty());
    }

    /// The visible half of the fallback, at the seam where it is assembled.
    ///
    /// A space is a file a person and an agent both edit, so both keys WILL hold
    /// something keeper cannot read. The list still runs; the row says so. This
    /// asserts the two sentences reach `warnings` together rather than one of
    /// them being dropped, which would send whoever is fixing the file round the
    /// loop twice.
    #[test]
    fn a_sort_and_a_position_keeper_cannot_read_are_both_reported() {
        let broken = concat!(
            "---\n",
            "id: 01J8ZQ4M7T5R9V3XK2B6C0DFGH\n",
            "keeper:\n",
            "  space: 'is:pinned'\n",
            "  sort: bananas\n",
            "  order: first\n",
            "---\n",
            "\n# Pinned\n"
        );
        let def = space_def(&entry("spaces/pinned.md", "Pinned"), broken);
        assert_eq!(def.warnings.len(), 2, "{:?}", def.warnings);
        assert!(def.warnings.iter().any(|said| said.contains("\"bananas\"")));
        assert!(def.warnings.iter().any(|said| said.contains("\"first\"")));

        // The stored text survives untouched, so the editor can put it back and
        // the sentence can quote it. keeper does not rewrite a value it could
        // not read — the same promise the query and the icon make.
        assert_eq!(def.sort, "bananas");

        // And the space still selects what it selects. A bad word in a
        // presentation key must not turn into an empty pane with an error in it.
        assert_eq!(def.query, "is:pinned");
        assert_eq!(sort::read(&def.sort).sort, sort::DEFAULT_SORT);
    }

    /// The icon sits beside `space` under `keeper:`, at ONE level of nesting,
    /// because one level is all there is. `Frontmatter`'s parser models exactly
    /// one — "a second nesting level. One is all the subset allows"
    /// (`frontmatter.rs`) — and that is not a gap to route around: the parser
    /// exists to leave every byte outside an edited span identical, which a
    /// general YAML model cannot promise. A `keeper.space.icon` would therefore
    /// never be read by anyone, so it is not a form this reads; it is a form
    /// nobody can write.
    #[test]
    fn a_space_definition_reads_its_icon_beside_the_query() {
        let flat = concat!(
            "---\n",
            "id: 01J8ZQ4M7T5R9V3XK2B6C0DFGH\n",
            "keeper:\n",
            "  space: 'tag:a'\n",
            "  icon: star\n",
            "---\n",
            "\n# Active\n"
        );
        assert_eq!(
            space_def(&entry("spaces/active.md", "Active"), flat).icon,
            Some("star".to_owned())
        );

        // Two levels deep is not a second supported spelling — the parser does
        // not produce a map there at all, so the key is invisible. Asserted so
        // that a future reader reaching for nesting learns it here rather than
        // from an icon that silently never appears.
        let too_deep = concat!(
            "---\n",
            "id: 01J8ZQ4M7T5R9V3XK2B6C0DFGH\n",
            "keeper:\n",
            "  space:\n",
            "    query: 'tag:a'\n",
            "    icon: flag\n",
            "---\n",
            "\n# Active\n"
        );
        assert!(space_def(&entry("spaces/active.md", "Active"), too_deep)
            .icon
            .is_none());

        // A space with no icon key is a space with no icon, not an error.
        assert!(
            space_def(&entry("spaces/new.md", "New"), "---\nid: x\n---\n")
                .icon
                .is_none()
        );
    }

    /// An icon name this crate does not recognise is not this crate's business:
    /// the set is the editor's, and rewriting a value keeper did not understand
    /// is the same mistake as rewriting a query term it could not parse. Only
    /// noise is refused.
    #[test]
    fn an_unrecognised_icon_name_survives_and_only_noise_is_refused() {
        assert_eq!(space_icon("sparkles"), Some("sparkles".to_owned()));
        assert_eq!(space_icon("  star  "), Some("star".to_owned()));
        assert_eq!(space_icon(""), None);
        assert_eq!(space_icon("   "), None);
        assert_eq!(space_icon(&"x".repeat(MAX_ICON_BYTES + 1)), None);
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

#[cfg(test)]
mod uncategorized_tests {
    use super::compose_uncategorized;

    fn compose(queries: &[&str]) -> String {
        compose_uncategorized(queries.iter().map(|q| (*q).to_owned()))
    }

    /// A vault where nothing has been claimed: everything is unclaimed. The
    /// parser reads the empty string as "everything", which is the answer.
    #[test]
    fn no_spaces_means_every_note_is_uncategorized() {
        assert_eq!(compose(&[]), "");
    }

    /// The whole idea in one line: the complement of the rows above it, spelled
    /// in the same language those rows are spelled in.
    #[test]
    fn every_space_becomes_one_negated_group() {
        assert_eq!(
            compose(&["tag:inbox", "path:journal/**"]),
            "-(tag:inbox) -(path:journal/**)"
        );
    }

    /// A space whose query does not parse selects nothing, so it claims nothing,
    /// so subtracting it would subtract nothing — and would risk composing a
    /// query that does not parse either, which would take the whole row down
    /// with it.
    #[test]
    fn a_space_that_does_not_parse_claims_nothing_and_is_skipped() {
        assert_eq!(
            compose(&["tag:inbox", "tag:(", "is:pinned"]),
            "-(tag:inbox) -(is:pinned)"
        );
    }

    /// Wrapping costs one level of nesting, so a query already at the limit
    /// cannot be wrapped. Dropping it makes this row a little too generous;
    /// keeping it would make the row fail to parse and show nothing at all.
    #[test]
    fn a_query_too_deep_to_wrap_is_dropped_rather_than_breaking_the_row() {
        let deep = format!("{}is:pinned{}", "(".repeat(64), ")".repeat(64));
        assert_eq!(compose(&["tag:inbox", &deep]), "-(tag:inbox)");
    }
}
