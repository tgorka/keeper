//! Command-palette index + action registry (Story 9.1, epic 9 spine).
//!
//! Two Rust-authoritative pieces answer the single `palette_query` command:
//!
//! - [`PaletteIndex`] — an in-memory projection of **every** room across **all**
//!   signed-in accounts (not just the windowed inbox `MergeState`, which holds only
//!   a recency window ~200/account). Each account's full matrix-sdk room set is
//!   projected into lightweight [`PaletteEntry`]s and refreshed as rooms change.
//!   A query does a linear scan with lowercased substring/subsequence fuzzy scoring;
//!   at ~10k entries this stays well under the 100 ms budget with no trie/FST.
//!
//! - [`palette_actions`] — the static action registry: the sole source of palette
//!   actions, reused by the cheat sheet + native menu bar (Story 9.3). Every
//!   shipped MVP surface (epics 1–8) registers at least one action here.
//!
//! All filtering and ranking live here — the frontend only renders and dispatches
//! by id. Ordering is never re-derived in TypeScript.

use std::collections::HashMap;

use crate::vm::{
    MenuItemVm, MenuSectionVm, PaletteActionVm, PaletteChatVm, PaletteMode, PaletteResultsVm,
};

/// Max rows returned per group (chats / contacts / actions), keeping the render
/// cheap and the wire payload bounded even against a 10k-entry index.
const MAX_RESULTS_PER_GROUP: usize = 20;

/// Minimum query length before chat/contact matching runs. Below this the palette
/// returns the top actions (plus, on the frontend, a `>` hint) — a 1-char query
/// against 10k rooms is noise.
const MIN_CHAT_QUERY_LEN: usize = 2;

/// The category every notes action lives in (Phase 5).
///
/// The notes capability gates a whole *section*, not individual actions, so the
/// gate reads this rather than a per-action flag on [`PaletteActionVm`]: adding a
/// second capability boolean to a view model the frontend already renders would
/// widen the IPC contract for a fact this module can answer itself. Naming the
/// string once is what keeps the gate and the registry entries from drifting —
/// a notes action that landed in some other category would silently survive on
/// iOS, which is the exact bug FR-122 forbids.
const NOTES_CATEGORY: &str = "Notes";

/// The sessions verbs' category (Phase 7, FR-251), gated whole like Notes.
const SESSIONS_CATEGORY: &str = "Sessions";

/// The tasks verbs' category (Epic 57, FR-351/FR-352, AD-137), gated whole like
/// Notes and Sessions and on the same fact: `sync && desktop`. iOS is not a task
/// host at all (`lifecycle.rs` pauses everything on backgrounding), and a build
/// without folder sync has no `sync.db` to keep a task record in — so there is
/// nothing to show rather than something to grey out.
const TASKS_CATEGORY: &str = "Tasks";

/// The bots verbs' category (Epic 61, FR-378/FR-384), gated whole like Notes,
/// Sessions and Tasks — but **not on their flag**.
///
/// Sessions and Tasks deliberately ride the `notes` gate because
/// `CapabilitiesVm.sessions`, `.notes` and AD-137's task condition are three
/// names for one fact (`sync && desktop`), and a second boolean would have said
/// nothing new. This is the case that doc anticipated: `CapabilitiesVm.bots` is
/// `cfg!(desktop)` (`keeper/src/ipc.rs`), with no `sync.db` in it at all. A
/// desktop build with folder sync switched off has no vault, no session board
/// and no task host, and still has a working Bots pane — so borrowing the notes
/// flag here would hide a surface that works, which is the mirror image of the
/// dead affordance AD-27 forbids. Hence the third parameter on
/// [`registry_sections`] and [`PaletteIndex::query`].
const BOTS_CATEGORY: &str = "Bots";

/// The registry ids of the three Recording verbs (Story 20.4, FR-48), named
/// because a fourth surface now dispatches on them.
///
/// The palette, the ⌘? cheat sheet and the native menu bar all read the id off a
/// rendered [`MenuItemVm`] and never spell it. The menu-bar tray cannot: it is
/// built in Rust and has to name the verb it offers (Story 46.16). A title is
/// allowed to change — Story 46.5 renamed the start verb and three surfaces
/// changed with it, precisely because they all read this registry — but an id is
/// the dispatch key `actions.ts` resolves and never does. Naming each once is
/// what keeps the registry entry, the tray projection and the tray's click
/// router from holding three copies of one string.
pub const RECORDING_START_ID: &str = "recording-start";
pub const RECORDING_STOP_ID: &str = "recording-stop";
pub const RECORDING_OPEN_FOLDER_ID: &str = "recording-open-folder";

/// The registry ids of the three Notes verbs the menu-bar tray offers (Story
/// 47.4, DW-195), named for [`RECORDING_START_ID`]'s reason.
///
/// The other three notes actions — `notes-open`, `notes-search` and
/// `notes-switch-vault` — are registered and deliberately absent from the tray,
/// which is why this is three named ids and not "the Notes category". The tray
/// is a menu-bar summary, not a second palette.
pub const NOTES_NEW_ID: &str = "notes-new";
pub const NOTES_CAPTURE_ID: &str = "notes-capture";
pub const NOTES_JOURNAL_ID: &str = "notes-journal-today";

/// One lightweight, non-secret projection of a room held in the [`PaletteIndex`]
/// (Story 9.1). Carries only render + ranking data: the owning account id and hue,
/// the room id, its display name (with a lowercased copy cached for scoring), the
/// DM flag (chat-vs-contact classification), the bridged-network label, and the
/// last-activity timestamp used as the tie-breaker so recent rooms rank first.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PaletteEntry {
    /// Opaque keeper account id this room belongs to.
    pub account_id: String,
    /// The owning account's hue index (0–7) for the hue dot.
    pub hue_index: u8,
    /// Opaque Matrix room id.
    pub room_id: String,
    /// The resolved room display name (rendered verbatim).
    pub display_name: String,
    /// Lowercased display name, cached so scoring never re-lowercases per query.
    pub name_lower: String,
    /// `true` when the room is a direct/DM room — classified as a contact.
    pub is_direct: bool,
    /// The bridged-Network label, or `None` for a native Matrix room.
    pub network: Option<String>,
    /// Last-activity timestamp (ms since the Unix epoch), the recency tie-breaker.
    pub last_activity_ms: i64,
}

impl PaletteEntry {
    /// Build an entry, caching the lowercased display name for scoring.
    pub fn new(
        account_id: String,
        hue_index: u8,
        room_id: String,
        display_name: String,
        is_direct: bool,
        network: Option<String>,
        last_activity_ms: i64,
    ) -> Self {
        let name_lower = display_name.to_lowercase();
        Self {
            account_id,
            hue_index,
            room_id,
            display_name,
            name_lower,
            is_direct,
            network,
            last_activity_ms,
        }
    }

    /// Project this entry into its wire [`PaletteChatVm`].
    fn to_vm(&self) -> PaletteChatVm {
        PaletteChatVm {
            id: format!("{}|{}", self.account_id, self.room_id),
            account_id: self.account_id.clone(),
            room_id: self.room_id.clone(),
            display_name: self.display_name.clone(),
            hue_index: self.hue_index,
            network: self.network.clone(),
            is_direct: self.is_direct,
        }
    }
}

/// The in-memory palette index: every room across every account, keyed per account
/// so one account's rooms can be replaced wholesale on a refresh without disturbing
/// the others. Not a source of truth for room state — it is a queryable projection
/// refreshed from each account's full matrix-sdk room set (Story 9.1).
#[derive(Debug, Default)]
pub struct PaletteIndex {
    /// Per-account room entries. `account_id → its full room set`.
    by_account: HashMap<String, Vec<PaletteEntry>>,
}

impl PaletteIndex {
    /// Construct an empty index (no accounts, no rooms).
    pub fn new() -> Self {
        Self::default()
    }

    /// Replace one account's entire room set (the seed-on-ready / refresh-on-change
    /// path). Replacing wholesale keeps the index consistent with the account's
    /// current full room set and drops rooms that have left it.
    pub fn set_account_rooms(&mut self, account_id: &str, entries: Vec<PaletteEntry>) {
        if entries.is_empty() {
            self.by_account.remove(account_id);
        } else {
            self.by_account.insert(account_id.to_owned(), entries);
        }
    }

    /// Drop an account's entries entirely (sign-out / teardown). Idempotent.
    pub fn remove_account(&mut self, account_id: &str) {
        self.by_account.remove(account_id);
    }

    /// Total indexed entries across all accounts (used in tests / diagnostics).
    pub fn len(&self) -> usize {
        self.by_account.values().map(Vec::len).sum()
    }

    /// Whether the index holds no rooms at all (signed out).
    pub fn is_empty(&self) -> bool {
        self.by_account.values().all(Vec::is_empty)
    }

    /// Iterate every entry across all accounts.
    fn entries(&self) -> impl Iterator<Item = &PaletteEntry> {
        self.by_account.values().flatten()
    }

    /// Answer one palette query against this index (Story 9.1).
    ///
    /// - `Default` mode: at ≥[`MIN_CHAT_QUERY_LEN`] chars, fuzzy-match chats and
    ///   contacts on the display name and return the matching actions too; below
    ///   that (or on no match) chats/contacts are empty and the top registered
    ///   actions are returned so the frontend can show them plus a `>` hint.
    /// - `Action` mode: only actions, ranked with open-chat actions first when
    ///   `open_chat` is set (context-aware).
    ///
    /// `recording` gates the recording capability actions exactly as `open_chat`
    /// gates the open-chat ones: a `requires_recording` action is dropped entirely
    /// when the capability is off (Story 16.3), so it never appears on a platform
    /// that cannot record. `notes` does the same for the whole [`NOTES_CATEGORY`]
    /// section (Phase 5, FR-122) — a build without folder sync has nowhere to put
    /// a vault, so its notes verbs are absent rather than dead (AD-27). `bots`
    /// does the same for [`BOTS_CATEGORY`] (Epic 61, FR-384), on its own flag
    /// rather than the notes one — that const says why.
    ///
    /// Each group is capped to [`MAX_RESULTS_PER_GROUP`]. Pure over the index — no
    /// I/O, no locks — so it is cheap and unit-testable.
    pub fn query(
        &self,
        query: &str,
        mode: PaletteMode,
        open_chat: bool,
        recording: bool,
        notes: bool,
        bots: bool,
    ) -> PaletteResultsVm {
        let needle = query.trim().to_lowercase();

        match mode {
            PaletteMode::Action => PaletteResultsVm {
                contacts: Vec::new(),
                chats: Vec::new(),
                actions: query_actions(&needle, open_chat, recording, notes, bots),
            },
            PaletteMode::Default => {
                let actions = query_actions(&needle, open_chat, recording, notes, bots);
                // A whitespace-only raw query (e.g. "  ") normalizes to an empty
                // needle here; `fuzzy_score("", ...)` would match every room, so treat
                // an effectively-empty needle exactly like the short-query path.
                if needle.chars().count() < MIN_CHAT_QUERY_LEN || needle.trim().is_empty() {
                    // Short/empty query: no chat/contact matches; the frontend shows
                    // the top actions plus a `>` hint.
                    return PaletteResultsVm {
                        contacts: Vec::new(),
                        chats: Vec::new(),
                        actions,
                    };
                }
                let (contacts, chats) = self.query_rooms(&needle);
                PaletteResultsVm {
                    contacts,
                    chats,
                    actions,
                }
            }
        }
    }

    /// Fuzzy-match rooms, split into (contacts, chats) by DM status, each ranked
    /// best-score-first (recency tie-break) and capped. A DM room only appears in
    /// `contacts`; a non-DM room only in `chats` — never both.
    fn query_rooms(&self, needle: &str) -> (Vec<PaletteChatVm>, Vec<PaletteChatVm>) {
        let mut contacts: Vec<(i32, &PaletteEntry)> = Vec::new();
        let mut chats: Vec<(i32, &PaletteEntry)> = Vec::new();
        for entry in self.entries() {
            if let Some(score) = fuzzy_score(needle, &entry.name_lower) {
                if entry.is_direct {
                    contacts.push((score, entry));
                } else {
                    chats.push((score, entry));
                }
            }
        }
        (rank_and_cap(contacts), rank_and_cap(chats))
    }
}

/// Rank scored entries best-first (higher score, then more recent, then name for a
/// stable order), cap to [`MAX_RESULTS_PER_GROUP`], and project to VMs.
fn rank_and_cap(mut scored: Vec<(i32, &PaletteEntry)>) -> Vec<PaletteChatVm> {
    scored.sort_by(|a, b| {
        b.0.cmp(&a.0)
            .then_with(|| b.1.last_activity_ms.cmp(&a.1.last_activity_ms))
            .then_with(|| a.1.name_lower.cmp(&b.1.name_lower))
    });
    scored
        .into_iter()
        .take(MAX_RESULTS_PER_GROUP)
        .map(|(_, entry)| entry.to_vm())
        .collect()
}

/// Score `haystack` against the lowercased `needle` (both already lowercased).
///
/// Returns `None` when the needle is not a subsequence of the haystack. A higher
/// score is a better match. A contiguous substring beats a scattered subsequence;
/// a prefix match beats a mid-string one; a shorter haystack (relatively tighter
/// match) beats a longer one. Pure and allocation-free.
fn fuzzy_score(needle: &str, haystack: &str) -> Option<i32> {
    if needle.is_empty() {
        return Some(0);
    }
    // Substring is the strongest signal: contiguous, and prefix-weighted.
    if let Some(byte_pos) = haystack.find(needle) {
        // `find` returns a BYTE offset; convert to a CHAR index so the prefix check
        // and mid-string penalty are in char units (matching `n_len`/`h_len` below).
        // For multi-byte names (emoji/CJK/accented) a byte offset would be > the char
        // index and make prefix/tightness ranking incoherent.
        let pos = haystack[..byte_pos].chars().count();
        let mut score = 1000;
        if pos == 0 {
            score += 500; // prefix match
        } else {
            // Penalize how far into the string the match starts (bounded).
            score -= i32::try_from(pos.min(200)).unwrap_or(200);
        }
        // Reward a relatively tight match (needle covers more of the haystack).
        let n_len = needle.chars().count() as i32;
        let h_len = haystack.chars().count().max(1) as i32;
        score += (n_len * 100) / h_len;
        return Some(score);
    }
    // Fall back to a subsequence match (chars appear in order, gaps allowed).
    subsequence_score(needle, haystack).map(|s| s + 100)
}

/// Score a subsequence match: `Some(score)` when every needle char appears in
/// `haystack` in order, else `None`. Consecutive matched chars are rewarded so a
/// near-contiguous run outranks a widely-scattered one. Pure.
fn subsequence_score(needle: &str, haystack: &str) -> Option<i32> {
    let mut hay = haystack.chars().peekable();
    let mut score = 0;
    let mut prev_matched = false;
    for nc in needle.chars() {
        let mut found = false;
        for hc in hay.by_ref() {
            if hc == nc {
                score += if prev_matched { 10 } else { 1 };
                prev_matched = true;
                found = true;
                break;
            }
            prev_matched = false;
        }
        if !found {
            return None;
        }
    }
    Some(score)
}

/// Match (or, on empty query, list) the registered actions and return them ranked.
///
/// When `open_chat` is set, open-chat actions (those with `requires_open_chat`)
/// rank first — the context-aware ordering the epic mandates. On an empty needle
/// the whole registry is returned in that ranked order (the "top actions" fallback);
/// otherwise only actions whose title or a keyword matches are kept. Each result is
/// capped to [`MAX_RESULTS_PER_GROUP`].
///
/// A `requires_recording` action is dropped entirely when `recording` is off (Story
/// 16.3), mirroring the `requires_open_chat` / `open_chat` gate, and every action in
/// [`NOTES_CATEGORY`] is dropped when `notes` is off (Phase 5, FR-122). `bots` does
/// the same for [`BOTS_CATEGORY`] (Epic 61, FR-384) — its own flag, for the reason
/// that const records.
fn query_actions(
    needle: &str,
    open_chat: bool,
    recording: bool,
    notes: bool,
    bots: bool,
) -> Vec<PaletteActionVm> {
    let mut scored: Vec<(i32, PaletteActionVm)> = Vec::new();
    for action in palette_actions() {
        // An open-chat action is only offered when a chat is open.
        if action.requires_open_chat && !open_chat {
            continue;
        }
        // A recording action is only offered when the recording capability is on.
        if action.requires_recording && !recording {
            continue;
        }
        // The notes section is gated whole, by category (see `NOTES_CATEGORY`).
        if !notes && action.category == NOTES_CATEGORY {
            continue;
        }
        // The bots section likewise (see `BOTS_CATEGORY`), on its own flag.
        if !bots && action.category == BOTS_CATEGORY {
            continue;
        }
        let score = if needle.is_empty() {
            Some(0)
        } else {
            action_score(needle, &action)
        };
        if let Some(mut score) = score {
            // Context ranking: open-chat actions float above global ones.
            if open_chat && action.requires_open_chat {
                score += 10_000;
            }
            scored.push((score, action));
        }
    }
    scored.sort_by(|a, b| b.0.cmp(&a.0).then_with(|| a.1.title.cmp(&b.1.title)));
    scored
        .into_iter()
        .take(MAX_RESULTS_PER_GROUP)
        .map(|(_, action)| action)
        .collect()
}

/// Score an action against the needle over its title and keywords, taking the best
/// field score. `None` when nothing matches. Pure.
fn action_score(needle: &str, action: &PaletteActionVm) -> Option<i32> {
    let title_lower = action.title.to_lowercase();
    let mut best = fuzzy_score(needle, &title_lower);
    for keyword in &action.keywords {
        let kw_lower = keyword.to_lowercase();
        if let Some(score) = fuzzy_score(needle, &kw_lower) {
            // Keyword hits count, but a title match is preferred.
            let score = score - 200;
            best = Some(best.map_or(score, |b| b.max(score)));
        }
    }
    best
}

/// The static action registry — the sole source of palette actions (Story 9.1,
/// epic 9 spine). Every shipped MVP surface (epics 1–8) registers at least one
/// action here; the cheat sheet + native menu bar (Story 9.3) consume this same
/// list. Each action's `id` is the dispatch key the frontend `actions.ts` map
/// resolves to a handler (view switch, dialog open, or Rust `invoke`).
///
/// `requires_open_chat` marks actions that operate on the currently-open chat
/// (Archive, Pin, …); the frontend disables them when no chat is open and the
/// query ranks them first in action mode. Shortcut chips mirror the existing
/// keyboard bindings; `None` means the action is palette-only.
pub fn palette_actions() -> Vec<PaletteActionVm> {
    // Non-toggle actions: `toggle_group` is `None`. Every action built through this
    // closure is `requires_recording: false`; the single recording action is
    // constructed inline below (Story 16.3).
    let action = |id: &str,
                  title: &str,
                  category: &str,
                  keywords: &[&str],
                  shortcut: Option<&str>,
                  requires_open_chat: bool| PaletteActionVm {
        id: id.to_owned(),
        title: title.to_owned(),
        category: category.to_owned(),
        keywords: keywords.iter().map(|k| (*k).to_owned()).collect(),
        shortcut: shortcut.map(str::to_owned),
        requires_open_chat,
        requires_recording: false,
        toggle_group: None,
    };

    // Toggle actions: the two directions of a pair share a `toggle_group` so both
    // surfaces (cheat sheet + native menu, Story 9.3) collapse them into one row.
    let toggle = |id: &str,
                  title: &str,
                  category: &str,
                  keywords: &[&str],
                  shortcut: Option<&str>,
                  group: &str| PaletteActionVm {
        id: id.to_owned(),
        title: title.to_owned(),
        category: category.to_owned(),
        keywords: keywords.iter().map(|k| (*k).to_owned()).collect(),
        shortcut: shortcut.map(str::to_owned),
        requires_open_chat: true,
        requires_recording: false,
        toggle_group: Some(group.to_owned()),
    };

    vec![
        // --- Navigation (view switches) ---
        action(
            "open-inbox",
            "Open Inbox",
            "Navigation",
            &["unified", "chats", "home"],
            Some("⌘1"),
            false,
        ),
        action(
            "open-archive",
            "Open Archive",
            "Navigation",
            &["low priority", "hidden"],
            Some("⌘2"),
            false,
        ),
        action(
            "open-approval",
            "Open Approval Pane",
            "Navigation",
            &["drafts", "airlock", "pending"],
            Some("⌘3"),
            false,
        ),
        action(
            "open-bridges",
            "Open Bridges",
            "Navigation",
            &["networks", "connect", "integrations"],
            Some("⌘4"),
            false,
        ),
        // The Recording view (Story 16.3): a `requires_recording` navigation action,
        // gated exactly like `open-bridges` but dropped from every surface when the
        // recording capability is off (desktop macOS ≥ 13.0 only). Built inline so
        // the shared `action` closure can keep `requires_recording: false`.
        PaletteActionVm {
            id: "open-recording".to_owned(),
            title: "Open Recording".to_owned(),
            category: "Navigation".to_owned(),
            keywords: ["record", "screen", "capture"]
                .iter()
                .map(|k| (*k).to_owned())
                .collect(),
            shortcut: Some("⌘5".to_owned()),
            requires_open_chat: false,
            requires_recording: true,
            toggle_group: None,
        },
        // The Recordings archive (Story 42.3, Story 45.20, FR-198). A SECOND
        // navigation action beside `open-recording`, not a rename of it: the
        // capture surface and the browser over what capture produced are two
        // primary views (`recording` and `recordings`), the sidebar has carried
        // two entries for them since 42.3, and the menu bar carried one — so
        // the one surface a person reaches for after recording was the one
        // surface the menu could not open.
        //
        // No shortcut chip: `⌘5` belongs to the capture surface and is bound by
        // a shipped JS hook. A chip here would either teach a chord that opens
        // the other view or claim a second one nothing binds, and this registry
        // is where the cheat sheet learns what to promise.
        //
        // `requires_recording`, exactly like `open-recording` and for the same
        // reason the sidebar gates both on one flag: a browser over recordings
        // this build cannot make is a puzzle, not a surface.
        PaletteActionVm {
            id: "open-recordings".to_owned(),
            title: "Open Recordings".to_owned(),
            category: "Navigation".to_owned(),
            keywords: ["record", "archive", "browse", "sessions", "library"]
                .iter()
                .map(|k| (*k).to_owned())
                .collect(),
            shortcut: None,
            requires_open_chat: false,
            requires_recording: true,
            toggle_group: None,
        },
        // --- Recording verbs (Story 20.4, FR-48): capability-gated exactly like
        // `open-recording` — `requires_recording: true` drops them from every
        // surface (palette, cheat sheet, native menu) when recording is off
        // (desktop macOS ≥ 13.0 only). Palette-only (`shortcut: None`): no
        // single-key verb exists on this surface (UX-DR29). Built inline so the
        // shared `action` closure keeps `requires_recording: false`.
        // Story 46.5: titled "New Recording", not "Start Recording". The owner
        // searched the palette for "new recording" and found nothing, one
        // submenu away from "Open Recordings" — which only navigates. "New X"
        // is this registry's own word for making one (`notes-new` → "New
        // Note"); "Start Recording" was the outlier, and the outlier is the
        // one nobody could find. `id`, `category` and the count below are
        // untouched: moving it out of Recording would break the section, and
        // the id is what `actions.ts` dispatches on.
        //
        // `start` joins the keywords because it just left the title, and `new`
        // joins it even though the title now carries it — pinning the owner's
        // word here means the next retitle cannot silently take it away again,
        // which is precisely how this defect was made.
        PaletteActionVm {
            id: RECORDING_START_ID.to_owned(),
            title: "New Recording".to_owned(),
            category: "Recording".to_owned(),
            keywords: [
                "record", "capture", "screen", "new", "start", "begin", "go live",
            ]
            .iter()
            .map(|k| (*k).to_owned())
            .collect(),
            shortcut: None,
            requires_open_chat: false,
            requires_recording: true,
            toggle_group: None,
        },
        PaletteActionVm {
            id: RECORDING_STOP_ID.to_owned(),
            title: "Stop Recording".to_owned(),
            category: "Recording".to_owned(),
            keywords: ["record", "capture", "end", "finish", "finalize"]
                .iter()
                .map(|k| (*k).to_owned())
                .collect(),
            shortcut: None,
            requires_open_chat: false,
            requires_recording: true,
            toggle_group: None,
        },
        PaletteActionVm {
            id: RECORDING_OPEN_FOLDER_ID.to_owned(),
            title: "Open Recordings Folder".to_owned(),
            category: "Recording".to_owned(),
            keywords: ["record", "reveal", "finder", "destination", "files"]
                .iter()
                .map(|k| (*k).to_owned())
                .collect(),
            shortcut: None,
            requires_open_chat: false,
            requires_recording: true,
            toggle_group: None,
        },
        // --- Notes (Phase 5, FR-98/99/100/101/107/118): the whole section is
        // dropped when the notes capability is off, gated by `NOTES_CATEGORY`
        // rather than per action, because notes is one surface and a build
        // without folder sync has nowhere to put a vault. Shortcut chips mirror
        // the bindings the notes surfaces register; they are display-only labels
        // here, as everywhere else in this registry.
        action(
            NOTES_NEW_ID,
            "New Note",
            NOTES_CATEGORY,
            &["note", "write", "compose", "markdown"],
            Some("⌘⌥N"),
            false,
        ),
        action(
            NOTES_CAPTURE_ID,
            "Quick Capture",
            NOTES_CATEGORY,
            &["note", "capture", "scratch", "inbox", "jot"],
            Some("⌘⌥K"),
            false,
        ),
        action(
            NOTES_JOURNAL_ID,
            "Today's Journal",
            NOTES_CATEGORY,
            &["note", "journal", "daily", "today", "diary"],
            Some("⌘⌥J"),
            false,
        ),
        action(
            "notes-open",
            "Open Note",
            NOTES_CATEGORY,
            &["note", "find", "jump", "go to"],
            Some("⌘P"),
            false,
        ),
        action(
            "notes-search",
            "Search Notes",
            NOTES_CATEGORY,
            &["note", "find", "content", "full text", "grep"],
            Some("⌘⇧F"),
            false,
        ),
        action(
            "notes-switch-vault",
            "Switch Vault",
            NOTES_CATEGORY,
            &["note", "vault", "folder", "change vault"],
            Some("⌘⌥V"),
            false,
        ),
        // --- Sessions (Phase 7, FR-251): gated by category exactly as Notes
        // is, on the sessions capability — the same construction over the same
        // sync substrate, so the same absence rule (FR-223).
        action(
            "sessions-view",
            "Sessions",
            SESSIONS_CATEGORY,
            &["session", "work", "llm", "agent", "board"],
            Some("⌘7"),
            false,
        ),
        action(
            "sessions-new",
            "New Session",
            SESSIONS_CATEGORY,
            &["session", "start", "create", "work"],
            None,
            false,
        ),
        action(
            "sessions-log-today",
            "Log Today in Session",
            SESSIONS_CATEGORY,
            &["session", "log", "entry", "journal", "today"],
            Some("⌘⌥L"),
            false,
        ),
        // --- Tasks (Epic 57, FR-351/FR-352, AD-137): gated by category like
        // Notes and Sessions, on the same `sync && desktop` fact.
        //
        // This entry is the fix, not decoration. The complaint that opened the
        // epic was literally "nie widzę w menu croon like job schedules" — *I
        // do not see it in the menu* — and the native macOS menu bar is built
        // by looping `registry_sections` and making one `Submenu` per category
        // (`keeper/src/menu.rs:108-127`). So registering here is what puts a
        // `Tasks` menu on the bar, a `Tasks` block on the ⌘? cheat sheet and a
        // `Tasks` row in ⌘K, from one string each.
        //
        // ⌘8 is the first free number: ⌘1–⌘7 are taken (sessions holds ⌘7), and
        // the chip is a display-only label here as everywhere in this registry
        // — the frontend hook owns the binding.
        action(
            "tasks-view",
            "Tasks",
            TASKS_CATEGORY,
            &[
                "task",
                "schedule",
                "cron",
                "job",
                "housekeeping",
                "release",
                "timer",
            ],
            Some("⌘8"),
            false,
        ),
        // --- Bots (Epic 61, FR-384): its own category on its own gate, for the
        // reason `BOTS_CATEGORY` records.
        //
        // One entry, and it is a preference rather than a navigation: the pane
        // has ⌘9 and a sidebar row already, and what had no second surface was
        // the metadata toggle. A toggle reachable only from the pane it changes
        // is a toggle you have to be looking at the answer to switch on, which
        // is precisely when you have already stopped reading. Palette-only
        // (`shortcut: None`): no chord is claimed for it — ⌘1–⌘9 are spoken for
        // and a preference does not earn a modifier cluster of its own.
        //
        // Titled with the direction it moves rather than the state it is in,
        // because the registry renders one row whichever way the setting sits
        // and "Show" would be a lie half the time.
        action(
            "bots-toggle-metadata",
            "Toggle Answer Details",
            BOTS_CATEGORY,
            &[
                "bot",
                "model",
                "tokens",
                "metadata",
                "details",
                "finish reason",
                "request id",
            ],
            None,
            false,
        ),
        // --- Global actions (dialogs / commands) ---
        action(
            "new-chat",
            "New Chat",
            "Chats",
            &["compose", "message", "start conversation"],
            Some("⌘N"),
            false,
        ),
        action(
            "open-search",
            "Search Messages",
            "Chats",
            &["find", "archive search", "history"],
            Some("⌘⇧F"),
            false,
        ),
        action(
            "start-export",
            "Start Export",
            "Archive",
            &["backup", "download", "save transcript"],
            None,
            false,
        ),
        action(
            "add-account",
            "Add Account",
            "Accounts",
            &["sign in", "login", "connect account"],
            None,
            false,
        ),
        action(
            "toggle-incognito-global",
            "Toggle Incognito (Global)",
            "Privacy",
            &["read receipts", "private", "stealth"],
            None,
            false,
        ),
        // Story 13.6: the non-gesture twin of the phone pull-to-refresh — kicks
        // every live account's sync loop through the single Rust `sync_now` entry.
        action(
            "sync-now",
            "Sync Now",
            "Accounts",
            &["sync", "refresh", "reconnect", "pull to refresh"],
            None,
            false,
        ),
        // --- Open-chat actions (operate on the current conversation) ---
        // Toggle pairs share a `toggle_group`; the cheat sheet + native menu render
        // each pair as ONE row, resolving direction from the open room's flag.
        toggle(
            "archive-chat",
            "Archive Chat",
            "Chat",
            &["low priority", "hide", "e"],
            Some("E"),
            "archive",
        ),
        toggle(
            "unarchive-chat",
            "Unarchive Chat",
            "Chat",
            &["restore", "unhide"],
            Some("E"),
            "archive",
        ),
        toggle(
            "pin-chat",
            "Pin Chat",
            "Chat",
            &["stick", "top", "p"],
            Some("P"),
            "pin",
        ),
        toggle(
            "unpin-chat",
            "Unpin Chat",
            "Chat",
            &["unstick", "p"],
            Some("P"),
            "pin",
        ),
        toggle(
            "favorite-chat",
            "Favorite Chat",
            "Chat",
            &["star", "favourite", "f"],
            Some("F"),
            "favorite",
        ),
        toggle(
            "unfavorite-chat",
            "Unfavorite Chat",
            "Chat",
            &["unstar", "unfavourite", "f"],
            Some("F"),
            "favorite",
        ),
        toggle(
            "mark-read",
            "Mark as Read",
            "Chat",
            &["clear unread", "seen", "u"],
            Some("U"),
            "read",
        ),
        toggle(
            "mark-unread",
            "Mark as Unread",
            "Chat",
            &["flag", "u"],
            Some("U"),
            "read",
        ),
        action(
            "toggle-incognito-chat",
            "Toggle Incognito (This Chat)",
            "Chat",
            &["read receipts", "private", "stealth"],
            None,
            true,
        ),
        // Per-Chat notification mode (Story 10.2). Three discrete targets rather than a
        // two-direction toggle pair, so each is a plain `action` (not a `toggle_group`)
        // — the single-key `m` verb + the chat context menu cover direction. They share
        // the `m` shortcut chip so the cheat sheet surfaces the verb once per target.
        action(
            "mute-chat",
            "Mute Chat",
            "Chat",
            &["silence", "notifications off", "m"],
            Some("M"),
            true,
        ),
        action(
            "mention-only-chat",
            "Mentions Only (This Chat)",
            "Chat",
            &["mention only", "keywords", "m"],
            Some("M"),
            true,
        ),
        action(
            "unmute-chat",
            "Unmute Chat",
            "Chat",
            &["notifications on", "all messages", "m"],
            Some("M"),
            true,
        ),
        action(
            "export-chat",
            "Export This Chat",
            "Chat",
            &["backup", "download", "save transcript"],
            None,
            true,
        ),
    ]
}

/// The stable category order the derived surfaces (cheat sheet + native menu,
/// Story 9.3) present. Categories are rendered in this order; any category present
/// in `palette_actions()` but missing here is appended last (alphabetically) so a
/// newly-added category is never silently dropped.
const CATEGORY_ORDER: &[&str] = &[
    "Navigation",
    "Chats",
    "Archive",
    "Accounts",
    "Privacy",
    // The capability-gated recording verbs (Story 20.4); the whole category
    // vanishes with the `recording` flag off, so its position only matters on
    // desktop macOS ≥ 13.
    "Recording",
    // The capability-gated notes verbs (Phase 5); like Recording, the whole
    // category vanishes with its flag off, so this position only matters on a
    // desktop build that has folder sync.
    NOTES_CATEGORY,
    // The capability-gated sessions verbs (Phase 7), directly after Notes —
    // the sibling surface over the same sync substrate (FR-251).
    SESSIONS_CATEGORY,
    // The capability-gated tasks view (Epic 57, FR-351/FR-352), directly after
    // Sessions: the third surface over that same sync substrate, and the third
    // category this one flag opens or closes whole.
    TASKS_CATEGORY,
    // The capability-gated bots verb (Epic 61, FR-384), after Tasks and before
    // Chat. Its flag is its own, not the one the three above share.
    BOTS_CATEGORY,
    "Chat",
];

/// The single projection both discovery surfaces consume (Story 9.3, epic 9 spine).
///
/// Derived purely from [`palette_actions`]: groups the registry by `category` in the
/// stable [`CATEGORY_ORDER`], preserving each category's registry order, and collapses
/// every toggle pair (two actions sharing a `toggle_group`) into a single unambiguous
/// [`MenuItemVm`] — the canonical (first-seen, positive) direction's id, a combined
/// "Archive / Unarchive Chat" title, and the shared shortcut. The native menu builder
/// and the `cheat_sheet_sections` command both call this, so the two surfaces provably
/// never drift from the palette (UX-DR15). Pure — no I/O, no state.
///
/// `recording` gates the recording capability actions (Story 16.3): when off, every
/// `requires_recording` action is dropped before grouping, so the cheat sheet and
/// native menu omit the recording action exactly as the palette does — the single
/// registry keeps all three surfaces consistent without any per-platform logic.
/// `notes` does the same for the whole [`NOTES_CATEGORY`] section (Phase 5,
/// FR-122): with it off the section is absent from the cheat sheet and the native
/// menu bar, not greyed out in them. The sessions and tasks sections ride the
/// SAME flag (Phase 7, FR-223; Epic 57, FR-352): `CapabilitiesVm.sessions` is
/// computed from the identical condition as `notes` (sync && desktop), and
/// AD-137 gates tasks on that very condition too — iOS is not a task host and a
/// build without folder sync has no `sync.db` to keep a task record in. So one
/// gate parameter serves all three categories: a second boolean here would be
/// two names for one fact, and it would change this signature and both call
/// sites (`menu.rs`'s builder and the `cheat_sheet_sections` command) to say
/// nothing new. **`bots` is the day that came.** Epic 61's pane is gated on
/// `CapabilitiesVm.bots`, which is `cfg!(desktop)` and has no `sync.db` in it:
/// a desktop build with folder sync switched off has no vault, no session board
/// and no task host, and still has a working Bots pane. Riding the `notes` flag
/// would therefore hide a section whose surface works — the mirror image of the
/// dead affordance AD-27 forbids — so this signature grew a third parameter
/// rather than a fourth name for one fact.
pub fn registry_sections(recording: bool, notes: bool, bots: bool) -> Vec<MenuSectionVm> {
    let actions: Vec<PaletteActionVm> = palette_actions()
        .into_iter()
        .filter(|action| recording || !action.requires_recording)
        .filter(|action| {
            notes
                || (action.category != NOTES_CATEGORY
                    && action.category != SESSIONS_CATEGORY
                    && action.category != TASKS_CATEGORY)
        })
        .filter(|action| bots || action.category != BOTS_CATEGORY)
        .collect();

    // Preserve first-appearance order of categories, then sort by CATEGORY_ORDER
    // (unlisted categories sort last, alphabetically, but keep their inner order).
    let mut category_order: Vec<String> = Vec::new();
    for action in &actions {
        if !category_order.contains(&action.category) {
            category_order.push(action.category.clone());
        }
    }
    let rank = |category: &str| {
        CATEGORY_ORDER
            .iter()
            .position(|c| *c == category)
            .unwrap_or(CATEGORY_ORDER.len())
    };
    category_order.sort_by(|a, b| rank(a).cmp(&rank(b)).then_with(|| a.cmp(b)));

    category_order
        .into_iter()
        .map(|category| {
            let mut items: Vec<MenuItemVm> = Vec::new();
            // Track which toggle groups already emitted their (canonical) row so the
            // second direction of a pair collapses into it instead of adding a row.
            let mut seen_groups: Vec<String> = Vec::new();
            for action in actions.iter().filter(|a| a.category == category) {
                match &action.toggle_group {
                    Some(group) => {
                        if seen_groups.contains(group) {
                            // The pair's canonical row already exists — skip the
                            // opposite direction (its title is folded in below).
                            continue;
                        }
                        seen_groups.push(group.clone());
                        // Combine the two directions' titles into one label, e.g.
                        // "Archive / Unarchive Chat". Find the paired action to
                        // extract its distinguishing verb.
                        let title = combined_toggle_title(&actions, action, group);
                        items.push(MenuItemVm {
                            id: action.id.clone(),
                            title,
                            shortcut: action.shortcut.clone(),
                            toggle_group: Some(group.clone()),
                            requires_open_chat: action.requires_open_chat,
                        });
                    }
                    None => items.push(MenuItemVm {
                        id: action.id.clone(),
                        title: action.title.clone(),
                        shortcut: action.shortcut.clone(),
                        toggle_group: None,
                        requires_open_chat: action.requires_open_chat,
                    }),
                }
            }
            MenuSectionVm { category, items }
        })
        .collect()
}

/// Which tray rendering is being built (Story 46.16).
///
/// The menu-bar tray is not one menu: it swaps a whole menu per state, and a verb
/// in the wrong one is a defect in both directions. A start verb that lingered
/// into the live-session menu would offer a second recording; a verb missing from
/// a rendering that *replaces* the idle menu — the folder-sync one does, on the
/// first sync tick (Story 29.2) — would silently disappear from the menu bar for
/// as long as a folder is syncing, which is most of the time.
///
/// So membership is a pure function of the rendering, decided and tested here,
/// rather than four hand-built menus in the shell each remembering what belongs
/// in it — none of which can be compiled, let alone tested, off macOS.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TrayMenu {
    /// Presence only (Story 10.3): no live session, no folder syncing.
    Idle,
    /// The folder-sync rendering (Story 29.2), which REPLACES the idle menu on
    /// the first sync tick — so it carries whatever the idle menu carries.
    Sync,
    /// A live session (Story 18.1).
    Recording,
    /// The hold over a terminal failed session (Story 18.4).
    Error,
}

/// The registry ids one tray rendering offers. Membership only — the order the
/// tray shows them in is the registry's (see [`tray_recording_verbs`]).
///
/// - **Idle / Sync**: start, and nothing else. A stop and a folder reveal belong
///   to a session that exists; the idle tray had no way to begin one at all,
///   which is the absence Story 46.16 fills.
/// - **Recording**: stop + the folder reveal (Story 18.1). Not start: there is
///   one session, and a second start item is the lingering-verb bug.
/// - **Error**: the folder reveal only. The one-click restart over a terminal
///   failed session lives on the window's banner, and the tray deliberately
///   never restarts a session itself (Story 18.4) — a start verb here would be a
///   second restart path with different words.
fn tray_verb_ids(menu: TrayMenu) -> &'static [&'static str] {
    match menu {
        TrayMenu::Idle | TrayMenu::Sync => &[RECORDING_START_ID],
        TrayMenu::Recording => &[RECORDING_STOP_ID, RECORDING_OPEN_FOLDER_ID],
        TrayMenu::Error => &[RECORDING_OPEN_FOLDER_ID],
    }
}

/// The recording verbs the menu-bar tray offers in `menu` — the tray's rendering
/// of the same [`registry_sections`] projection the native menu bar and the ⌘?
/// cheat sheet render (Story 46.16).
///
/// The tray was the one discovery surface that did not project this registry: it
/// hand-built every label. That is why Story 46.5's rename of the start verb
/// reached the palette, the cheat sheet and the menu bar in a single edit and
/// reached the tray not at all — and why the tray had no start verb to rename in
/// the first place. Membership per rendering is [`tray_verb_ids`]; the ORDER and
/// the WORDS are the registry's, taken by filtering the projection in place, so
/// the tray cannot order or word a verb differently from the menu bar.
///
/// `recording` is the capability flag (`recording_supported()` in the shell:
/// desktop macOS ≥ 13 only). With it off every `requires_recording` action is
/// already gone from the projection, so the tray offers no recording verb at all
/// — absent, not disabled, exactly as on the other three surfaces (FR-66,
/// AD-35). Pure — no I/O, no state.
pub fn tray_recording_verbs(menu: TrayMenu, recording: bool) -> Vec<MenuItemVm> {
    let wanted = tray_verb_ids(menu);
    // `notes: false`: no RECORDING verb is a notes action. The tray's notes
    // section is projected separately by [`tray_notes_labels`], because its
    // three labels are composed — a suffix per model tick on a base word — and
    // its items are built once and mutated (AD-61) rather than rebuilt per
    // menu. A verb that failed to resolve for this or any other reason is
    // caught by the test that counts what each rendering asks for against what
    // it gets.
    registry_sections(recording, false, false)
        .into_iter()
        .flat_map(|section| section.items)
        .filter(|item| wanted.contains(&item.id.as_str()))
        .collect()
}

/// What the tray's notes section knows about the vault when it paints (Story
/// 47.4, DW-195).
///
/// Two bools rather than the shell's `NotesTray`, because this crate cannot see
/// that type and must not learn to: everything else on it — the five recent
/// slots, the unread count — is model data the registry has no word for. These
/// two are the only facts that change a *verb's* wording.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct TrayNotesState {
    /// A vault is registered, so a create has somewhere to go.
    pub vault: bool,
    /// The quick-capture global shortcut registered (UX-DR43).
    pub hotkey_registered: bool,
}

/// The three labels the tray's notes section shows, by NAME.
///
/// Named fields and not a `Vec`, and that is the whole defence against the one
/// hazard widening the tray's projection carries. The recording verbs take the
/// registry's ORDER because they are rebuilt per menu; the notes items are built
/// once and only mutated (AD-61 — a Linux tray menu cannot be swapped after it
/// is set), so a positional projection would move a label onto the wrong handle
/// the day somebody reorders the registry. A field cannot slip.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TrayNotesLabels {
    pub new_note: String,
    pub capture: String,
    pub journal: String,
}

/// The registry's word for each verb the tray's notes section offers (Story
/// 47.4, DW-195).
///
/// Story 46.16 made the tray project this registry for the RECORDING verbs, so
/// Story 46.5's rename reached the menu bar. The notes section was left
/// hand-typed and its three words were spelled twice — and had already drifted:
/// the tray wrote `Today’s Journal` with a typographic apostrophe against this
/// registry's `Today's Journal`, two surfaces disagreeing about a title UX-DR42
/// says is one. The registry wins, verbatim, including the plain apostrophe:
/// the palette matches keystrokes against this title, and a curly apostrophe
/// nobody's keyboard types would be a search regression bought for a glyph.
///
/// **Only the label.** The tray's own "Open Recordings Folder" reveals the LIVE
/// session's output path while the registry's verb reveals the CONFIGURED
/// destination — same words, two different folders — so a projection that
/// carried anything but words would quietly re-point a verb. This carries
/// words. The click handlers, the ids the tray dispatches on and the enabled
/// state are all still the tray's.
///
/// `notes` is the capability flag (FR-122). `None` means the tray builds no
/// notes section, which is the right answer to both of its causes — a build
/// that ships no notes surface, and a registry that could not answer for one of
/// its own ids. The second is not a state; it is a bug, and the test below is
/// what makes it one. Pure — no I/O, no state.
pub fn tray_notes_labels(notes: bool) -> Option<TrayNotesLabels> {
    let items = registry_sections(false, notes, false)
        .into_iter()
        .find(|section| section.category == NOTES_CATEGORY)?
        .items;
    let title = |id: &str| {
        items
            .iter()
            .find(|item| item.id == id)
            .map(|item| item.title.clone())
    };
    Some(TrayNotesLabels {
        new_note: title(NOTES_NEW_ID)?,
        capture: title(NOTES_CAPTURE_ID)?,
        journal: title(NOTES_JOURNAL_ID)?,
    })
}

impl TrayNotesLabels {
    /// The words for one model tick, composed on the registry's base.
    ///
    /// The composition is the tray's own and stays: `new_note_label` returns the
    /// honest "no vault yet" wording instead of the bare verb, and `capture`
    /// carries the global-shortcut registration failure (UX-DR43). What changed
    /// is where the BASE word comes from. It lives here rather than in `tray.rs`
    /// for AD-56's reason: the shell crate does not build on two of the three
    /// hosts this repo is written on, so a sentence composed there is prose and
    /// a sentence composed here is a test.
    ///
    /// Both suffixes are appended, never substituted, so a retitle reaches the
    /// empty state too — the failure mode being avoided is a registry rename
    /// that fixes three surfaces and leaves the menu bar saying the old word the
    /// moment a vault is missing.
    pub fn painted(&self, state: TrayNotesState) -> Self {
        Self {
            new_note: no_vault_yet(&self.new_note, state.vault),
            capture: if state.hotkey_registered {
                self.capture.clone()
            } else {
                format!("{} \u{2014} hotkey unavailable", self.capture)
            },
            journal: no_vault_yet(&self.journal, state.vault),
        }
    }
}

/// A create verb with nowhere to create, said out loud.
///
/// The two create verbs stay ENABLED without a vault — choosing one opens
/// Settings → Sync, because the action is achievable and a disabled row that
/// explains nothing is worse than a row that takes you where you need to go — so
/// the wording is the only thing that can carry the state.
fn no_vault_yet(base: &str, vault: bool) -> String {
    if vault {
        base.to_owned()
    } else {
        format!("{base}\u{2026} (no vault yet)")
    }
}

/// Build the collapsed toggle title for a pair into one unambiguous label.
///
/// Factors out the words the two direction titles share as a common word-prefix and
/// word-suffix, then joins the two differing middles with `" / "`. Examples:
/// - `"Archive Chat"` + `"Unarchive Chat"` → `"Archive / Unarchive Chat"`
///   (shared suffix `Chat`; middles `Archive` / `Unarchive`).
/// - `"Mark as Read"` + `"Mark as Unread"` → `"Mark as Read / Unread"`
///   (shared prefix `Mark as`; middles `Read` / `Unread`).
///
/// The canonical direction's middle comes first so the row reads in the positive
/// direction. Falls back to the canonical title alone if the pair's second direction
/// is somehow absent (defensive — the registry always ships both directions).
fn combined_toggle_title(
    actions: &[PaletteActionVm],
    canonical: &PaletteActionVm,
    group: &str,
) -> String {
    let Some(other) = actions
        .iter()
        .find(|a| a.toggle_group.as_deref() == Some(group) && a.id != canonical.id)
    else {
        return canonical.title.clone();
    };

    let a: Vec<&str> = canonical.title.split_whitespace().collect();
    let b: Vec<&str> = other.title.split_whitespace().collect();

    // Longest shared leading run of whole words.
    let mut prefix = 0;
    while prefix < a.len() && prefix < b.len() && a[prefix] == b[prefix] {
        prefix += 1;
    }
    // Longest shared trailing run of whole words (not overlapping the prefix).
    let mut suffix = 0;
    while suffix < a.len() - prefix
        && suffix < b.len() - prefix
        && a[a.len() - 1 - suffix] == b[b.len() - 1 - suffix]
    {
        suffix += 1;
    }

    let shared_prefix = a[..prefix].join(" ");
    let a_middle = a[prefix..a.len() - suffix].join(" ");
    let b_middle = b[prefix..b.len() - suffix].join(" ");
    let shared_suffix = a[a.len() - suffix..].join(" ");

    let middle = format!("{a_middle} / {b_middle}");
    [shared_prefix, middle, shared_suffix]
        .into_iter()
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>()
        .join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(
        account: &str,
        hue: u8,
        room: &str,
        name: &str,
        is_direct: bool,
        ts: i64,
    ) -> PaletteEntry {
        PaletteEntry::new(
            account.to_owned(),
            hue,
            room.to_owned(),
            name.to_owned(),
            is_direct,
            None,
            ts,
        )
    }

    fn sample_index() -> PaletteIndex {
        let mut index = PaletteIndex::new();
        index.set_account_rooms(
            "acc-a",
            vec![
                entry("acc-a", 0, "!alice:x", "Alice Anderson", true, 100),
                entry("acc-a", 0, "!alpha:x", "Alpha Team", false, 90),
                entry("acc-a", 0, "!bob:x", "Bob Builder", true, 80),
            ],
        );
        index.set_account_rooms(
            "acc-b",
            vec![
                entry("acc-b", 3, "!algo:x", "Algorithms Study", false, 70),
                entry("acc-b", 3, "!zeta:x", "Zeta Squad", false, 60),
            ],
        );
        index
    }

    #[test]
    fn default_filter_splits_chats_and_contacts() {
        let index = sample_index();
        let results = index.query("al", PaletteMode::Default, false, false, false, false);
        // "al" matches Alice (contact), Alpha (chat), Algorithms (chat).
        assert!(results
            .contacts
            .iter()
            .any(|c| c.display_name == "Alice Anderson"));
        assert!(results.chats.iter().any(|c| c.display_name == "Alpha Team"));
        assert!(results
            .chats
            .iter()
            .any(|c| c.display_name == "Algorithms Study"));
        // A DM is never in chats.
        assert!(!results.chats.iter().any(|c| c.is_direct));
        assert!(results.contacts.iter().all(|c| c.is_direct));
        // Actions still come back on a default query.
        assert!(!results.actions.is_empty());
        // Hue + composite id are carried.
        let alice = results
            .contacts
            .iter()
            .find(|c| c.display_name == "Alice Anderson")
            .expect("alice present");
        assert_eq!(alice.hue_index, 0);
        assert_eq!(alice.id, "acc-a|!alice:x");
    }

    #[test]
    fn short_query_returns_no_rooms_but_top_actions() {
        let index = sample_index();
        let results = index.query("a", PaletteMode::Default, false, false, false, false);
        assert!(results.contacts.is_empty());
        assert!(results.chats.is_empty());
        assert!(!results.actions.is_empty());

        let empty = index.query("", PaletteMode::Default, false, false, false, false);
        assert!(empty.contacts.is_empty());
        assert!(empty.chats.is_empty());
        assert!(!empty.actions.is_empty());
    }

    #[test]
    fn no_match_returns_top_actions_only() {
        let index = sample_index();
        let results = index.query("zzqq", PaletteMode::Default, false, false, false, false);
        assert!(results.contacts.is_empty());
        assert!(results.chats.is_empty());
        // Empty needle inside actions? No — "zzqq" matches no action either, so
        // the actions list is the matched (empty) set for a non-empty needle.
        assert!(results.actions.is_empty());
    }

    #[test]
    fn no_match_default_short_still_shows_actions() {
        // The frontend's "no-match shows top actions" is served by the <2-char and
        // empty-needle path (top actions) — a real no-match keeps actions honest.
        let index = sample_index();
        let results = index.query("", PaletteMode::Default, false, false, false, false);
        assert!(!results.actions.is_empty());
    }

    #[test]
    fn action_mode_returns_only_actions() {
        let index = sample_index();
        let results = index.query("arch", PaletteMode::Action, false, false, false, false);
        assert!(results.contacts.is_empty());
        assert!(results.chats.is_empty());
        assert!(results.actions.iter().any(|a| a.id == "open-archive"));
    }

    #[test]
    fn action_mode_open_chat_actions_rank_first() {
        let index = sample_index();
        // Empty action-mode query with an open chat: open-chat actions come first.
        let results = index.query("", PaletteMode::Action, true, false, false, false);
        assert!(!results.actions.is_empty());
        // The first several actions must all be requires_open_chat.
        let first = &results.actions[0];
        assert!(
            first.requires_open_chat,
            "expected an open-chat action first, got {}",
            first.id
        );
        // And when no chat is open, open-chat actions are excluded entirely.
        let closed = index.query("", PaletteMode::Action, false, false, false, false);
        assert!(closed.actions.iter().all(|a| !a.requires_open_chat));
    }

    #[test]
    fn no_accounts_still_returns_actions() {
        let index = PaletteIndex::new();
        assert!(index.is_empty());
        let results = index.query("al", PaletteMode::Default, false, false, false, false);
        assert!(results.contacts.is_empty());
        assert!(results.chats.is_empty());
        // Global actions are available even signed out.
        assert!(!results.actions.is_empty());
    }

    /// Every `requires_recording` action the registry ships — `open-recording`
    /// (Story 16.3), `open-recordings` (Story 45.20) and the three recording
    /// verbs (Story 20.4, FR-48/FR-66).
    ///
    /// The archive entry is in this list rather than in a test of its own so it
    /// is gated by the same assertions as its four siblings: a navigation entry
    /// that survived the capability going off would offer a browser over
    /// recordings the build cannot make.
    const RECORDING_ACTION_IDS: [&str; 5] = [
        "open-recording",
        "open-recordings",
        "recording-start",
        "recording-stop",
        "recording-open-folder",
    ];

    #[test]
    fn open_recording_present_iff_recording_capability_on() {
        // Story 16.3 + 20.4: every recording action appears in the palette
        // exactly when the recording capability is on, across both query modes
        // and the registry projection (cheat sheet + native menu) — absent, not
        // disabled, when the capability is off (FR-66, AD-35).
        let index = sample_index();

        // Action mode, empty needle → the whole (ungated) registry: recording on
        // includes each action, recording off drops each.
        let on = index.query("", PaletteMode::Action, false, true, false, false);
        let off = index.query("", PaletteMode::Action, false, false, false, false);
        for id in RECORDING_ACTION_IDS {
            assert!(
                on.actions.iter().any(|a| a.id == id),
                "{id} present when recording is on"
            );
            assert!(
                !off.actions.iter().any(|a| a.id == id),
                "{id} absent when recording is off"
            );
        }

        // A direct query honors the same gate for every recording action
        // ("record" fuzzy-matches all four titles/keywords).
        let queried_on = index.query("record", PaletteMode::Action, false, true, false, false);
        let queried_off = index.query("record", PaletteMode::Action, false, false, false, false);
        for id in RECORDING_ACTION_IDS {
            assert!(
                queried_on.actions.iter().any(|a| a.id == id),
                "{id} matches a direct query when recording is on"
            );
            assert!(
                !queried_off.actions.iter().any(|a| a.id == id),
                "{id} never surfaces on a query when recording is off"
            );
        }

        // The registry projection (both discovery surfaces) gates them too:
        // `open-recording` lives in Navigation, the verbs in their own
        // Recording section — present with the flag on…
        let sections_on = registry_sections(true, false, false);
        let all_on: Vec<&str> = sections_on
            .iter()
            .flat_map(|s| s.items.iter().map(|i| i.id.as_str()))
            .collect();
        for id in RECORDING_ACTION_IDS {
            assert!(
                all_on.contains(&id),
                "registry projection includes {id} when recording is on"
            );
        }
        let recording_section = sections_on
            .iter()
            .find(|s| s.category == "Recording")
            .expect("Recording section present when recording is on");
        assert_eq!(
            recording_section.items.len(),
            3,
            "the three recording verbs share the Recording section"
        );
        // …and the whole category (plus the Navigation entry) vanishes off.
        let sections_off = registry_sections(false, false, false);
        assert!(
            !sections_off.iter().any(|s| s.category == "Recording"),
            "no Recording section when recording is off"
        );
        let all_off: Vec<&str> = sections_off
            .iter()
            .flat_map(|s| s.items.iter().map(|i| i.id.as_str()))
            .collect();
        for id in RECORDING_ACTION_IDS {
            assert!(
                !all_off.contains(&id),
                "registry projection omits {id} when recording is off"
            );
        }
    }

    #[test]
    fn the_start_verb_answers_the_word_people_search_for() {
        // Story 46.5. The owner went looking for "new recording", found
        // nothing, and concluded the verb did not exist — it did, titled
        // "Start Recording", one submenu away from "Open Recordings", which
        // only navigates.
        //
        // Queried, not read off the struct: asserting `title == "New
        // Recording"` would restate the diff and pass on a title nothing can
        // find. What was broken is the search, so the search is what is
        // asserted. "new" and "new recording" both missed under the old title
        // and both of its old keywords ("begin", "go live") — neither string
        // is a subsequence of either.
        let index = sample_index();
        for needle in ["new", "new recording"] {
            let hits = index.query(needle, PaletteMode::Action, false, true, false, false);
            let ids: Vec<&str> = hits.actions.iter().map(|a| a.id.as_str()).collect();
            assert!(
                ids.contains(&"recording-start"),
                "{needle:?} finds the start verb: {ids:?}"
            );
        }
        // And it is the FIRST answer, not a tail hit under the navigation
        // entry the owner already found by accident.
        let top = index.query(
            "new recording",
            PaletteMode::Action,
            false,
            true,
            false,
            false,
        );
        assert_eq!(
            top.actions.first().map(|a| a.id.as_str()),
            Some("recording-start"),
            "the start verb leads the results: {:?}",
            top.actions
                .iter()
                .map(|a| a.id.as_str())
                .collect::<Vec<_>>()
        );
        // The word that left the title still finds it: a rename that dropped
        // "start" from both places would trade one lost vocabulary for another.
        let by_old_word = index.query("start", PaletteMode::Action, false, true, false, false);
        assert!(
            by_old_word
                .actions
                .iter()
                .any(|a| a.id == "recording-start"),
            "\"start\" still finds it"
        );
        // A rename must not become a re-home: `palette.rs`'s section-count
        // assertion above is what moving this item to Navigation would break,
        // and this says out loud that the category is load-bearing.
        let recording = registry_sections(true, false, false)
            .into_iter()
            .find(|s| s.category == "Recording")
            .expect("Recording section present");
        assert!(
            recording.items.iter().any(|i| i.id == "recording-start"),
            "the renamed verb stays in the Recording section"
        );
    }

    #[test]
    fn the_archive_entry_is_a_second_navigation_action_beside_the_capture_one() {
        // Story 45.20: the reported gap was that the menu bar could open the
        // capture surface and not the archive. Two DISTINCT ids in Navigation,
        // with two distinct titles, is what closes it — a test that only asked
        // "is there a recordings entry" would pass on a renamed `open-recording`
        // and the menu would still have one item where it needs two.
        let actions = palette_actions();
        let nav: Vec<(&str, &str)> = actions
            .iter()
            .filter(|a| a.category == "Navigation")
            .map(|a| (a.id.as_str(), a.title.as_str()))
            .collect();
        assert!(
            nav.contains(&("open-recording", "Open Recording")),
            "the capture surface keeps its entry: {nav:?}"
        );
        assert!(
            nav.contains(&("open-recordings", "Open Recordings")),
            "the archive gains one: {nav:?}"
        );

        let archive = actions
            .iter()
            .find(|a| a.id == "open-recordings")
            .expect("open-recordings is registered");
        // No chip: ⌘5 is the capture surface's and nothing binds a second chord.
        // A chip here is a promise the cheat sheet would print and no hook keeps.
        assert_eq!(archive.shortcut, None, "the archive claims no chord");
        assert!(archive.requires_recording, "gated like every sibling");
        assert!(!archive.requires_open_chat);
        assert_eq!(archive.toggle_group, None);

        // Both entries reachable from one search, because "recordings" is what
        // a person types when they want either.
        let index = sample_index();
        let hits = index.query("recordings", PaletteMode::Action, false, true, false, false);
        let ids: Vec<&str> = hits.actions.iter().map(|a| a.id.as_str()).collect();
        assert!(ids.contains(&"open-recordings"), "searchable: {ids:?}");
    }

    /// The six notes actions the registry ships (Phase 5, build contract §1).
    const NOTES_ACTION_IDS: [&str; 6] = [
        "notes-new",
        "notes-capture",
        "notes-journal-today",
        "notes-open",
        "notes-search",
        "notes-switch-vault",
    ];

    #[test]
    fn the_notes_section_is_present_iff_the_notes_capability_is_on() {
        // FR-122: on a build without notes the surface is *absent*, not disabled
        // — in the palette, the cheat sheet and the native menu alike, because
        // all three read this one registry.
        let index = sample_index();

        let on = index.query("", PaletteMode::Action, false, false, true, false);
        let off = index.query("", PaletteMode::Action, false, false, false, false);
        for id in NOTES_ACTION_IDS {
            assert!(
                on.actions.iter().any(|a| a.id == id),
                "{id} present when notes is on"
            );
            assert!(
                !off.actions.iter().any(|a| a.id == id),
                "{id} absent when notes is off"
            );
        }

        // A direct query honors the same gate ("note" matches every title or
        // keyword in the section).
        let queried_on = index.query("note", PaletteMode::Action, false, false, true, false);
        let queried_off = index.query("note", PaletteMode::Action, false, false, false, false);
        for id in NOTES_ACTION_IDS {
            assert!(
                queried_on.actions.iter().any(|a| a.id == id),
                "{id} matches a direct query when notes is on"
            );
            assert!(
                !queried_off.actions.iter().any(|a| a.id == id),
                "{id} never surfaces on a query when notes is off"
            );
        }

        // The registry projection gates the whole category, and the two
        // capability flags are independent of each other.
        let sections_on = registry_sections(false, true, false);
        let notes_section = sections_on
            .iter()
            .find(|s| s.category == NOTES_CATEGORY)
            .expect("Notes section present when notes is on");
        assert_eq!(
            notes_section.items.len(),
            NOTES_ACTION_IDS.len(),
            "the whole section projects, and nothing else joins it"
        );
        assert!(
            !sections_on.iter().any(|s| s.category == "Recording"),
            "notes on does not drag the recording section in with it"
        );
        let sections_off = registry_sections(true, false, false);
        assert!(
            !sections_off.iter().any(|s| s.category == NOTES_CATEGORY),
            "no Notes section when notes is off"
        );
        let all_off: Vec<&str> = sections_off
            .iter()
            .flat_map(|s| s.items.iter().map(|i| i.id.as_str()))
            .collect();
        for id in NOTES_ACTION_IDS {
            assert!(
                !all_off.contains(&id),
                "registry projection omits {id} when notes is off"
            );
        }
    }

    #[test]
    fn every_notes_action_carries_its_documented_shortcut_chip() {
        // The chips are the contract's, and the cheat sheet is the only place a
        // user learns them — a silently wrong label teaches a chord that does
        // nothing.
        let expected = [
            ("notes-new", "⌘⌥N"),
            ("notes-capture", "⌘⌥K"),
            ("notes-journal-today", "⌘⌥J"),
            ("notes-open", "⌘P"),
            ("notes-search", "⌘⇧F"),
            ("notes-switch-vault", "⌘⌥V"),
        ];
        let actions = palette_actions();
        for (id, chip) in expected {
            let action = actions
                .iter()
                .find(|a| a.id == id)
                .unwrap_or_else(|| panic!("{id} is registered"));
            assert_eq!(action.category, NOTES_CATEGORY, "{id} is in the section");
            assert_eq!(action.shortcut.as_deref(), Some(chip), "{id} chip");
            assert!(!action.requires_open_chat, "{id} does not need a chat");
            assert!(!action.requires_recording, "{id} is not a recording action");
        }
    }

    #[test]
    fn the_tasks_section_is_present_iff_the_tasks_capability_is_on() {
        // FR-352, AD-137: tasks are desktop-gated the way notes and sessions
        // are, so on a build without them the whole section is *absent* from the
        // ⌘? cheat sheet and the native menu bar rather than greyed out in them
        // — both are built from this one projection.
        let off = registry_sections(false, false, false);
        assert!(
            !off.iter().any(|s| s.category == TASKS_CATEGORY),
            "no Tasks section when the gate is off"
        );
        assert!(
            !off.iter()
                .flat_map(|s| s.items.iter())
                .any(|i| i.id == "tasks-view"),
            "and tasks-view reached no other section either"
        );

        let on = registry_sections(false, true, false);
        let tasks = on
            .iter()
            .find(|s| s.category == TASKS_CATEGORY)
            .expect("Tasks section present when the gate is on");
        let ids: Vec<&str> = tasks.items.iter().map(|i| i.id.as_str()).collect();
        assert_eq!(
            ids,
            vec!["tasks-view"],
            "the whole section projects, and nothing else joins it"
        );
        assert!(
            !on.iter().any(|s| s.category == "Recording"),
            "the tasks gate does not drag the recording section in with it"
        );
    }

    #[test]
    fn the_tasks_view_carries_the_cmd_8_chip_the_menu_bar_renders() {
        // The complaint that opened epic 57 was "nie widzę w menu croon like
        // job schedules" — *I do not see it in the menu*. `menu.rs` builds one
        // native submenu per `registry_sections` category and labels each item
        // `title  shortcut`, so this registry row IS the fix: the title and the
        // chip asserted here are the two strings the owner reads off the macOS
        // menu bar.
        let actions = palette_actions();
        let tasks = actions
            .iter()
            .find(|a| a.id == "tasks-view")
            .expect("tasks-view is registered");
        assert_eq!(tasks.title, "Tasks");
        assert_eq!(tasks.category, TASKS_CATEGORY);
        assert_eq!(tasks.shortcut.as_deref(), Some("⌘8"), "⌘8 is the chip");
        assert!(!tasks.requires_open_chat, "a task list needs no open chat");
        assert!(!tasks.requires_recording, "not a recording verb");
        assert_eq!(tasks.toggle_group, None, "one direction, not a pair");

        // Exactly one action claims ⌘8: a chip the cheat sheet prints twice
        // teaches a chord that does one of two things.
        let claimants: Vec<&str> = actions
            .iter()
            .filter(|a| a.shortcut.as_deref() == Some("⌘8"))
            .map(|a| a.id.as_str())
            .collect();
        assert_eq!(claimants, vec!["tasks-view"], "⌘8 has a single owner");

        // And it survives into the projection the menu builder consumes, chip
        // and all — the registry entry alone would prove nothing about menu.rs.
        let item = registry_sections(false, true, false)
            .into_iter()
            .find(|s| s.category == TASKS_CATEGORY)
            .and_then(|s| s.items.into_iter().find(|i| i.id == "tasks-view"))
            .expect("tasks-view projects into the Tasks submenu");
        assert_eq!(item.title, "Tasks");
        assert_eq!(item.shortcut.as_deref(), Some("⌘8"));
    }

    #[test]
    fn the_bots_section_rides_its_own_flag_and_not_the_notes_one() {
        // FR-384. The point of the third parameter: `CapabilitiesVm.bots` is
        // `cfg!(desktop)`, so a desktop build with folder sync off has no notes,
        // no sessions and no tasks — and a working Bots pane. If the section
        // rode the notes flag, that build would lose a verb whose surface works.
        let neither = registry_sections(false, false, false);
        assert!(
            !neither.iter().any(|s| s.category == BOTS_CATEGORY),
            "no Bots section when the bots gate is off"
        );

        let notes_only = registry_sections(false, true, false);
        assert!(
            !notes_only.iter().any(|s| s.category == BOTS_CATEGORY),
            "the notes gate does not open the Bots section"
        );

        let bots_only = registry_sections(false, false, true);
        let section = bots_only
            .iter()
            .find(|s| s.category == BOTS_CATEGORY)
            .expect("Bots section present on its own flag, with notes off");
        let ids: Vec<&str> = section.items.iter().map(|i| i.id.as_str()).collect();
        assert_eq!(ids, vec!["bots-toggle-metadata"]);
        assert!(
            !bots_only.iter().any(|s| s.category == NOTES_CATEGORY),
            "and it does not drag the Notes section in with it"
        );
    }

    #[test]
    fn the_metadata_toggle_is_findable_by_the_words_a_person_would_type() {
        // The pane's own control is on the pane; this entry exists so somebody
        // reading an answer that will not explain itself can reach the toggle
        // without knowing where it lives. That only works if the search finds
        // it, so the keywords are asserted rather than assumed.
        let index = sample_index();
        for needle in ["tokens", "details", "answer details", "request id"] {
            let hits = index.query(needle, PaletteMode::Action, false, false, false, true);
            let ids: Vec<&str> = hits.actions.iter().map(|a| a.id.as_str()).collect();
            assert!(
                ids.contains(&"bots-toggle-metadata"),
                "{needle:?} should find the toggle, got {ids:?}"
            );
        }
        // And with the gate off it is absent from the query too, not merely from
        // the menu projection.
        let off = index.query("", PaletteMode::Action, false, false, false, false);
        assert!(
            !off.actions.iter().any(|a| a.id == "bots-toggle-metadata"),
            "the ⌘K list drops it with the capability off"
        );

        let action = palette_actions()
            .into_iter()
            .find(|a| a.id == "bots-toggle-metadata")
            .expect("bots-toggle-metadata is registered");
        assert_eq!(action.title, "Toggle Answer Details");
        assert_eq!(
            action.shortcut, None,
            "no chord is claimed for a preference"
        );
        assert!(!action.requires_open_chat);
        assert!(!action.requires_recording);
        assert_eq!(action.toggle_group, None, "one row, not a pair");
    }

    #[test]
    fn set_account_rooms_replaces_wholesale() {
        let mut index = sample_index();
        assert_eq!(index.len(), 5);
        index.set_account_rooms(
            "acc-a",
            vec![entry("acc-a", 0, "!only:x", "Only Room", false, 1)],
        );
        assert_eq!(index.len(), 3); // 1 (acc-a) + 2 (acc-b)
        let results = index.query("only", PaletteMode::Default, false, false, false, false);
        assert_eq!(results.chats.len(), 1);
    }

    #[test]
    fn remove_account_drops_entries() {
        let mut index = sample_index();
        index.remove_account("acc-b");
        assert_eq!(index.len(), 3);
        let results = index.query("zeta", PaletteMode::Default, false, false, false, false);
        assert!(results.chats.is_empty());
    }

    #[test]
    fn empty_account_rooms_removes_account() {
        let mut index = sample_index();
        index.set_account_rooms("acc-a", Vec::new());
        assert_eq!(index.len(), 2);
    }

    #[test]
    fn registry_covers_shipped_surfaces() {
        let ids: Vec<String> = palette_actions().into_iter().map(|a| a.id).collect();
        for expected in [
            "open-inbox",
            "open-archive",
            "open-approval",
            "open-bridges",
            "new-chat",
            "open-search",
            "start-export",
            "add-account",
            "toggle-incognito-global",
            "sync-now",
            "archive-chat",
            "pin-chat",
            "favorite-chat",
            "mark-read",
            "mark-unread",
            "toggle-incognito-chat",
            "mute-chat",
            "mention-only-chat",
            "unmute-chat",
        ] {
            assert!(
                ids.contains(&expected.to_owned()),
                "missing action {expected}"
            );
        }
        // Ids are unique.
        let mut sorted = ids.clone();
        sorted.sort();
        sorted.dedup();
        assert_eq!(sorted.len(), ids.len(), "duplicate action id in registry");
    }

    #[test]
    fn registry_sections_collapse_toggle_pairs_to_one_row() {
        let sections = registry_sections(false, false, false);
        let chat = sections
            .iter()
            .find(|s| s.category == "Chat")
            .expect("Chat section present");

        // Each of the four toggle groups appears exactly once as a collapsed row.
        for group in ["archive", "pin", "favorite", "read"] {
            let matching: Vec<&MenuItemVm> = chat
                .items
                .iter()
                .filter(|i| i.toggle_group.as_deref() == Some(group))
                .collect();
            assert_eq!(
                matching.len(),
                1,
                "toggle group {group} should collapse to one row, got {}",
                matching.len()
            );
        }

        // The archive row carries the CANONICAL (positive) id and the shared shortcut,
        // and its combined title names both directions.
        let archive = chat
            .items
            .iter()
            .find(|i| i.toggle_group.as_deref() == Some("archive"))
            .expect("archive row present");
        assert_eq!(archive.id, "archive-chat", "canonical id retained");
        assert_eq!(archive.shortcut.as_deref(), Some("E"), "shared shortcut");
        assert!(
            archive.title.contains("Archive") && archive.title.contains("Unarchive"),
            "combined title names both directions, got {:?}",
            archive.title
        );

        // read pair collapses too, canonical = mark-read, shortcut U.
        let read = chat
            .items
            .iter()
            .find(|i| i.toggle_group.as_deref() == Some("read"))
            .expect("read row present");
        assert_eq!(read.id, "mark-read");
        assert_eq!(read.shortcut.as_deref(), Some("U"));
        assert!(
            read.title.contains("Read") && read.title.contains("Unread"),
            "combined read title, got {:?}",
            read.title
        );

        // No un-collapsed opposite direction leaked as its own row.
        for opposite in [
            "unarchive-chat",
            "unpin-chat",
            "unfavorite-chat",
            "mark-unread",
        ] {
            assert!(
                !chat.items.iter().any(|i| i.id == opposite),
                "opposite direction {opposite} must be folded into its pair"
            );
        }
    }

    #[test]
    fn registry_sections_no_toggle_group_left_uncollapsed() {
        // Across ALL sections, every toggle group appears exactly once.
        let sections = registry_sections(false, false, false);
        let mut counts: std::collections::HashMap<String, usize> = std::collections::HashMap::new();
        for section in &sections {
            for item in &section.items {
                if let Some(group) = &item.toggle_group {
                    *counts.entry(group.clone()).or_insert(0) += 1;
                }
            }
        }
        assert_eq!(counts.len(), 4, "exactly four toggle groups");
        for (group, count) in counts {
            assert_eq!(count, 1, "group {group} collapsed to a single item");
        }
    }

    #[test]
    fn registry_sections_ordered_by_category() {
        let sections = registry_sections(false, false, false);
        let categories: Vec<&str> = sections.iter().map(|s| s.category.as_str()).collect();
        assert_eq!(
            categories,
            vec![
                "Navigation",
                "Chats",
                "Archive",
                "Accounts",
                "Privacy",
                "Chat"
            ],
            "categories rendered in the stable CATEGORY_ORDER"
        );
        // Every section is non-empty (no phantom category).
        assert!(sections.iter().all(|s| !s.items.is_empty()));
    }

    #[test]
    fn registry_sections_covers_all_actions() {
        // Every registered action id is reachable through a section item: a
        // non-toggle action maps to its own item; a toggle action maps to its
        // group's collapsed item (by canonical id or by group membership). This
        // proves the projection drops nothing. Every capability flag is on so
        // the gated Recording, Notes and Bots actions are in the sections and in
        // the `palette_actions()` set alike.
        let sections = registry_sections(true, true, true);
        let section_ids: Vec<String> = sections
            .iter()
            .flat_map(|s| s.items.iter().map(|i| i.id.clone()))
            .collect();
        let section_groups: Vec<String> = sections
            .iter()
            .flat_map(|s| s.items.iter().filter_map(|i| i.toggle_group.clone()))
            .collect();
        for action in palette_actions() {
            let covered = section_ids.contains(&action.id)
                || action
                    .toggle_group
                    .as_ref()
                    .is_some_and(|g| section_groups.contains(g));
            assert!(covered, "action {} not reachable via a section", action.id);
        }
    }

    /// FR-48 release-gate parity test (Story 9.3).
    ///
    /// Enumerates every MVP UI surface shipped in epics 1–8 and asserts each is
    /// reachable through ≥1 registered `palette_actions()` id, OR is on the
    /// documented justified-exclusion allowlist. A new surface that ships without a
    /// registered action (and without a justified exclusion) FAILS this test — the
    /// parity gate becomes mechanical rather than a hand-maintained promise.
    #[test]
    fn parity_every_mvp_surface_has_an_action_or_is_excluded() {
        let ids: Vec<String> = palette_actions().into_iter().map(|a| a.id).collect();
        let has = |id: &str| ids.iter().any(|i| i == id);

        // Each row: (surface label, covering action ids). A surface is covered when
        // at least ONE of its listed ids is registered. Grounded in the actual
        // shipped actions and the surfaces they route to (see actions.ts).
        let surfaces: &[(&str, &[&str])] = &[
            // Epic 4 — Unified Inbox and its views.
            ("Unified Inbox view", &["open-inbox"]),
            ("Archive view", &["open-archive"]),
            // Epic 4 — chat-row triage verbs (archive/pin/favourite/read).
            (
                "Archive/unarchive a chat",
                &["archive-chat", "unarchive-chat"],
            ),
            ("Pin/unpin a chat", &["pin-chat", "unpin-chat"]),
            (
                "Favourite/unfavourite a chat",
                &["favorite-chat", "unfavorite-chat"],
            ),
            ("Mark chat read/unread", &["mark-read", "mark-unread"]),
            // Epic 5 — Local Archive search + export.
            ("Archive search", &["open-search"]),
            ("Export (whole archive)", &["start-export"]),
            ("Export this chat", &["export-chat"]),
            // Epic 6 — Bridges surface + new chat.
            ("Bridges view", &["open-bridges"]),
            ("New chat", &["new-chat"]),
            // Epic 1/2 — account onboarding.
            ("Add an account", &["add-account"]),
            // Epic 7 — Approval Pane (draft airlock).
            ("Approval Pane view", &["open-approval"]),
            // Epic 8 — Incognito (global + per-chat).
            ("Toggle Incognito globally", &["toggle-incognito-global"]),
            ("Toggle Incognito for a chat", &["toggle-incognito-chat"]),
            // Epic 10 — per-Chat mute / mention-only / unmute (Story 10.2).
            (
                "Mute / mention-only / unmute a chat",
                &["mute-chat", "mention-only-chat", "unmute-chat"],
            ),
            // Epic 13 — pull-to-refresh's non-gesture path (Story 13.6).
            ("Sync now (kick the sync loop)", &["sync-now"]),
        ];

        // Justified exclusions — surfaces intentionally NOT registered as palette
        // actions, with rationale. Consistent with 9.1's Block-If and the
        // deferred-work ledger. These are asserted to STAY excluded (documented),
        // not asserted covered.
        //   - Device verification: no clean cold-open entry point; auto-opens on an
        //     incoming request / from Settings, not a palette-dispatchable surface.
        //   - Key backup: same — no cold-open entry point; driven from Settings and
        //     the recovery-key modal lifecycle.
        //   (Mute shipped in Story 10.2: the `mute-chat` / `mention-only-chat` /
        //   `unmute-chat` actions dispatch `chat_notify_mode_set`, so it is now a covered
        //   surface above rather than a justified exclusion.)
        let excluded: &[&str] = &["device-verification", "key-backup"];
        assert_eq!(excluded.len(), 2, "the documented exclusion set is stable");

        for (surface, covering) in surfaces {
            let covered = covering.iter().any(|id| has(id));
            assert!(
                covered,
                "MVP surface {surface:?} has no registered palette action \
                 (expected one of {covering:?}); register an action or add it to the \
                 justified-exclusion allowlist with a rationale"
            );
        }
    }

    #[test]
    fn substring_beats_subsequence() {
        // "cat" as a substring should outrank "cat" scattered as a subsequence.
        let contiguous = fuzzy_score("cat", "cathedral").expect("substring");
        let scattered = fuzzy_score("cat", "carpet tack").expect("subsequence");
        assert!(contiguous > scattered);
    }

    #[test]
    fn prefix_beats_midstring() {
        let prefix = fuzzy_score("al", "alpha").expect("prefix");
        let mid = fuzzy_score("al", "canal").expect("midstring");
        assert!(prefix > mid);
    }

    #[test]
    fn latency_under_100ms_at_10k_entries() {
        use std::time::Instant;

        // Build a synthetic 10k-entry index across a few accounts.
        let mut index = PaletteIndex::new();
        for acc in 0..5 {
            let account_id = format!("acc-{acc}");
            let mut entries = Vec::with_capacity(2000);
            for i in 0..2000 {
                let is_direct = i % 3 == 0;
                entries.push(entry(
                    &account_id,
                    (acc % 8) as u8,
                    &format!("!room{acc}_{i}:x"),
                    &format!("Room {acc} Number {i} Channel"),
                    is_direct,
                    i as i64,
                ));
            }
            index.set_account_rooms(&account_id, entries);
        }
        assert_eq!(index.len(), 10_000);

        // Each query is a single keystroke's worth of work; enforce the PER-QUERY
        // budget (a per-keystroke bound), not an aggregate average.
        let queries = ["ro", "roo", "chan", "number 1", "zzz"];
        for q in queries {
            let start = Instant::now();
            let _ = index.query(q, PaletteMode::Default, true, false, false, false);
            let elapsed = start.elapsed();
            assert!(
                elapsed.as_millis() < 100,
                "10k-entry palette query {q:?} too slow: {elapsed:?}"
            );
        }
    }

    #[test]
    fn whitespace_only_query_returns_no_rooms() {
        // "  " normalizes to an empty needle; it must NOT match every room (which a
        // bare `fuzzy_score("", ...)` would), and instead fall back to top actions.
        let index = sample_index();
        let results = index.query("  ", PaletteMode::Default, false, false, false, false);
        assert!(results.contacts.is_empty(), "whitespace matched contacts");
        assert!(results.chats.is_empty(), "whitespace matched chats");
        assert!(!results.actions.is_empty(), "top actions should still show");
    }

    #[test]
    fn non_ascii_prefix_beats_midstring() {
        // Multi-byte (accented) prefix must outrank a mid-string match. With byte
        // offsets the mid-string `pos` would be understated and mis-rank these.
        let prefix = fuzzy_score("é", "élan").expect("prefix");
        let mid = fuzzy_score("é", "café société").expect("midstring");
        assert!(
            prefix > mid,
            "non-ASCII prefix ({prefix}) should beat mid-string ({mid})"
        );

        // Same with a CJK leading char.
        let cjk_prefix = fuzzy_score("東", "東京タワー").expect("prefix");
        let cjk_mid = fuzzy_score("東", "会社 東京").expect("midstring");
        assert!(
            cjk_prefix > cjk_mid,
            "CJK prefix ({cjk_prefix}) should beat mid-string ({cjk_mid})"
        );
    }

    // --- The tray projection (Story 46.16) ---------------------------------

    /// Every tray rendering, so a fifth cannot be added without deciding what it
    /// offers: a new variant makes this array fail to compile-length-match below.
    const TRAY_MENUS: [TrayMenu; 4] = [
        TrayMenu::Idle,
        TrayMenu::Sync,
        TrayMenu::Recording,
        TrayMenu::Error,
    ];

    fn tray_ids(menu: TrayMenu) -> Vec<String> {
        tray_recording_verbs(menu, true)
            .into_iter()
            .map(|item| item.id)
            .collect()
    }

    #[test]
    fn the_tray_offers_the_start_verb_exactly_where_a_session_can_begin() {
        // The reported absence: from the menu-bar icon there was no way to begin
        // a recording in ANY state. It belongs in the two renderings that mean
        // "no session" — and the sync one is not optional, because it REPLACES
        // the idle menu on the first sync tick, so a verb only in the idle menu
        // vanishes for as long as a folder syncs.
        assert_eq!(tray_ids(TrayMenu::Idle), [RECORDING_START_ID]);
        assert_eq!(
            tray_ids(TrayMenu::Sync),
            [RECORDING_START_ID],
            "the sync menu replaces the idle one and must carry the same verb"
        );

        // …and nowhere else. A start item that lingered into a live session
        // would offer a second recording; one in the error hold would be a
        // second restart path beside the banner's (Story 18.4).
        for menu in [TrayMenu::Recording, TrayMenu::Error] {
            let ids = tray_ids(menu);
            assert!(
                !ids.iter().any(|id| id == RECORDING_START_ID),
                "{menu:?} must not offer a start verb: {ids:?}"
            );
        }

        // The session-scoped verbs stay where Story 18.1/18.4 put them, in the
        // registry's own order (start, stop, folder → filtered in place).
        assert_eq!(
            tray_ids(TrayMenu::Recording),
            [RECORDING_STOP_ID, RECORDING_OPEN_FOLDER_ID]
        );
        assert_eq!(tray_ids(TrayMenu::Error), [RECORDING_OPEN_FOLDER_ID]);
    }

    #[test]
    fn the_tray_shows_the_registrys_own_words_for_every_verb_it_projects() {
        // The point of the story. Story 46.5 renamed the start verb in the
        // registry and three surfaces changed; the tray did not, because it
        // hand-typed its labels. Asserting the projected title EQUALS the
        // registry's title — never the literal "New Recording" — is what makes
        // the tray the fourth surface: a hand-typed label would satisfy a
        // `== "New Recording"` assertion today and drift on the next rename.
        let registry = palette_actions();
        for menu in TRAY_MENUS {
            for item in tray_recording_verbs(menu, true) {
                let action = registry
                    .iter()
                    .find(|action| action.id == item.id)
                    .unwrap_or_else(|| {
                        panic!(
                            "{menu:?} projects {}, which the registry does not ship",
                            item.id
                        )
                    });
                assert_eq!(
                    item.title, action.title,
                    "{menu:?} shows {}'s registry words",
                    item.id
                );
                // The tray renders `title` alone (its labels carry no chords).
                // These verbs are palette-only by design (UX-DR29); if one ever
                // gained a chord, the tray would be dropping it silently.
                assert_eq!(
                    item.shortcut, None,
                    "{} claims a chord the tray's label would drop",
                    item.id
                );
            }
        }
    }

    #[test]
    fn every_verb_a_tray_rendering_names_reaches_the_projection() {
        // A rendering that names an id the registry no longer ships — renamed
        // away, re-homed out of the projection, deleted — would render a shorter
        // menu with no error anywhere. The count is the guard.
        for menu in TRAY_MENUS {
            assert_eq!(
                tray_recording_verbs(menu, true).len(),
                tray_verb_ids(menu).len(),
                "{menu:?} resolves every id it names"
            );
        }
    }

    #[test]
    fn no_tray_rendering_projects_a_recording_verb_when_the_capability_is_off() {
        // Absent, not disabled (FR-66, AD-35): the same gate the palette, the
        // cheat sheet and the native menu bar apply, which is what makes the
        // shell's `recording_supported()` check the same check rather than a
        // second one that could disagree.
        for menu in TRAY_MENUS {
            assert!(
                tray_recording_verbs(menu, false).is_empty(),
                "{menu:?} offers nothing when recording is off"
            );
        }
    }

    // --- The tray's notes section (Story 47.4, DW-195) ---------------------

    /// The three the tray shows, paired with the field each lands on. Written
    /// down once so every assertion below reads the same pairing, and so adding
    /// a fourth is an edit here rather than three edits scattered.
    fn tray_notes_pairs(labels: &TrayNotesLabels) -> [(&'static str, &str); 3] {
        [
            (NOTES_NEW_ID, labels.new_note.as_str()),
            (NOTES_CAPTURE_ID, labels.capture.as_str()),
            (NOTES_JOURNAL_ID, labels.journal.as_str()),
        ]
    }

    #[test]
    fn every_label_the_trays_notes_section_shows_is_the_registrys_own_word() {
        // The point of DW-195, and asserted the way 46.16 asserted its half:
        // against the registry entry, never against a literal. `assert_eq!(new_note,
        // "New Note")` would pass today on a hand-typed label and drift on the
        // next rename, which is the defect rather than the test for it.
        let registry = palette_actions();
        let labels = tray_notes_labels(true).expect("the notes section projects");
        for (id, shown) in tray_notes_pairs(&labels) {
            let action = registry
                .iter()
                .find(|action| action.id == id)
                .unwrap_or_else(|| panic!("the tray projects {id}, which the registry drops"));
            assert_eq!(shown, action.title, "{id} shows its registry words");
            assert_eq!(
                action.category, NOTES_CATEGORY,
                "{id} projects out of the section the capability gates"
            );
        }
    }

    #[test]
    fn a_retitled_notes_verb_reaches_the_tray_in_every_state_it_can_be_in() {
        // The failure DW-195 names: a rename that reaches the palette, the
        // cheat sheet and the menu bar and stops at the menu bar icon. Every
        // composed label has to CONTAIN the registry's word — including the two
        // empty states, which is where a suffix substituted for the base word
        // instead of appended to it would hide the old spelling.
        let labels = tray_notes_labels(true).expect("the notes section projects");
        for state in [
            TrayNotesState {
                vault: true,
                hotkey_registered: true,
            },
            TrayNotesState {
                vault: false,
                hotkey_registered: true,
            },
            TrayNotesState {
                vault: true,
                hotkey_registered: false,
            },
            TrayNotesState::default(),
        ] {
            let painted = labels.painted(state);
            for ((id, base), (_, shown)) in tray_notes_pairs(&labels)
                .into_iter()
                .zip(tray_notes_pairs(&painted))
            {
                assert!(
                    shown.starts_with(base),
                    "{id} in {state:?} shows {shown:?}, which does not start with the \
                     registry's {base:?}"
                );
            }
        }
    }

    #[test]
    fn the_two_empty_states_are_said_in_words_and_only_on_the_verbs_they_are_about() {
        // UX-DR43 and the no-vault wording, kept exactly as `tray.rs` composed
        // them before this moved — the words a user reads are the story here,
        // and a projection that silently dropped a suffix would look like a
        // working tray that lies about a hotkey that never registered.
        let labels = tray_notes_labels(true).expect("the notes section projects");

        let settled = labels.painted(TrayNotesState {
            vault: true,
            hotkey_registered: true,
        });
        assert_eq!(settled, labels, "a settled vault adds nothing at all");

        let no_vault = labels.painted(TrayNotesState {
            vault: false,
            hotkey_registered: true,
        });
        assert_eq!(
            no_vault.new_note,
            format!("{}\u{2026} (no vault yet)", labels.new_note)
        );
        assert_eq!(
            no_vault.journal,
            format!("{}\u{2026} (no vault yet)", labels.journal)
        );
        assert_eq!(
            no_vault.capture, labels.capture,
            "capture works without a vault — it is the verb that MAKES one usable"
        );

        let no_hotkey = labels.painted(TrayNotesState {
            vault: true,
            hotkey_registered: false,
        });
        assert_eq!(
            no_hotkey.capture,
            format!("{} \u{2014} hotkey unavailable", labels.capture)
        );
        assert_eq!(no_hotkey.new_note, labels.new_note);
        assert_eq!(no_hotkey.journal, labels.journal);

        // Both at once: a fresh install with no vault and a chord some other app
        // already holds. Neither suffix eats the other.
        let neither = labels.painted(TrayNotesState::default());
        assert_eq!(
            neither.capture,
            format!("{} \u{2014} hotkey unavailable", labels.capture)
        );
        assert_eq!(neither.new_note, no_vault.new_note);
    }

    #[test]
    fn the_tray_projects_three_notes_verbs_and_not_the_section() {
        // DW-195's own warning, and 46.16's before it: `notes-open`,
        // `notes-search` and `notes-switch-vault` are registered and
        // deliberately absent from the menu bar. A projection that widened to
        // "the Notes category" would grow the tray by three rows nobody asked
        // for, silently, on the next registry addition.
        let section: Vec<String> = registry_sections(false, true, false)
            .into_iter()
            .find(|section| section.category == NOTES_CATEGORY)
            .expect("the Notes section projects")
            .items
            .into_iter()
            .map(|item| item.id)
            .collect();
        assert_eq!(
            section.len(),
            NOTES_ACTION_IDS.len(),
            "the section is the whole registry's; the tray takes three of it"
        );
        for absent in ["notes-open", "notes-search", "notes-switch-vault"] {
            assert!(
                section.iter().any(|id| id == absent),
                "{absent} is registered — this test would be vacuous otherwise"
            );
        }
        let labels = tray_notes_labels(true).expect("the notes section projects");
        let shown: Vec<&str> = tray_notes_pairs(&labels)
            .iter()
            .map(|(_, label)| *label)
            .collect();
        for absent in ["Open Note", "Search Notes", "Switch Vault"] {
            assert!(
                !shown.contains(&absent),
                "{absent} reached the tray: {shown:?}"
            );
        }
    }

    #[test]
    fn the_tray_builds_no_notes_section_when_the_capability_is_off() {
        // Absent, not disabled (FR-122), and the same gate the other three
        // surfaces apply — `registry_sections`' own, not a second one that could
        // disagree with it.
        assert_eq!(tray_notes_labels(false), None);
        assert!(
            tray_notes_labels(true).is_some(),
            "…and it is the flag doing it, not an unconditional None"
        );
    }

    #[test]
    fn the_notes_labels_land_on_the_handle_they_belong_to_whatever_the_registry_order() {
        // The hazard that makes this a struct of named fields rather than a
        // Vec. The tray's items are built once and only mutated (AD-61), so
        // their slot order is the TRAY's; a positional projection would move
        // "Quick Capture" onto the New Note handle the day the registry is
        // reshuffled. Pinned by matching each field against its own id's title
        // rather than against position.
        let registry = palette_actions();
        let title = |id: &str| {
            registry
                .iter()
                .find(|action| action.id == id)
                .map(|action| action.title.clone())
                .unwrap_or_else(|| panic!("{id} is registered"))
        };
        let labels = tray_notes_labels(true).expect("the notes section projects");
        assert_eq!(labels.new_note, title(NOTES_NEW_ID));
        assert_eq!(labels.capture, title(NOTES_CAPTURE_ID));
        assert_eq!(labels.journal, title(NOTES_JOURNAL_ID));
        // And the three are distinct, so a field that read another field's id
        // could not pass the three assertions above by accident.
        assert_ne!(labels.new_note, labels.capture);
        assert_ne!(labels.capture, labels.journal);
        assert_ne!(labels.new_note, labels.journal);
    }
}
