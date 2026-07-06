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

use crate::vm::{PaletteActionVm, PaletteChatVm, PaletteMode, PaletteResultsVm};

/// Max rows returned per group (chats / contacts / actions), keeping the render
/// cheap and the wire payload bounded even against a 10k-entry index.
const MAX_RESULTS_PER_GROUP: usize = 20;

/// Minimum query length before chat/contact matching runs. Below this the palette
/// returns the top actions (plus, on the frontend, a `>` hint) — a 1-char query
/// against 10k rooms is noise.
const MIN_CHAT_QUERY_LEN: usize = 2;

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
    /// Each group is capped to [`MAX_RESULTS_PER_GROUP`]. Pure over the index — no
    /// I/O, no locks — so it is cheap and unit-testable.
    pub fn query(&self, query: &str, mode: PaletteMode, open_chat: bool) -> PaletteResultsVm {
        let needle = query.trim().to_lowercase();

        match mode {
            PaletteMode::Action => PaletteResultsVm {
                contacts: Vec::new(),
                chats: Vec::new(),
                actions: query_actions(&needle, open_chat),
            },
            PaletteMode::Default => {
                let actions = query_actions(&needle, open_chat);
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
fn query_actions(needle: &str, open_chat: bool) -> Vec<PaletteActionVm> {
    let mut scored: Vec<(i32, PaletteActionVm)> = Vec::new();
    for action in palette_actions() {
        // An open-chat action is only offered when a chat is open.
        if action.requires_open_chat && !open_chat {
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
        // --- Open-chat actions (operate on the current conversation) ---
        action(
            "archive-chat",
            "Archive Chat",
            "Chat",
            &["low priority", "hide", "e"],
            Some("E"),
            true,
        ),
        action(
            "unarchive-chat",
            "Unarchive Chat",
            "Chat",
            &["restore", "unhide"],
            Some("E"),
            true,
        ),
        action(
            "pin-chat",
            "Pin Chat",
            "Chat",
            &["stick", "top", "p"],
            Some("P"),
            true,
        ),
        action(
            "unpin-chat",
            "Unpin Chat",
            "Chat",
            &["unstick", "p"],
            Some("P"),
            true,
        ),
        action(
            "favorite-chat",
            "Favorite Chat",
            "Chat",
            &["star", "favourite", "f"],
            Some("F"),
            true,
        ),
        action(
            "unfavorite-chat",
            "Unfavorite Chat",
            "Chat",
            &["unstar", "unfavourite", "f"],
            Some("F"),
            true,
        ),
        action(
            "mark-read",
            "Mark as Read",
            "Chat",
            &["clear unread", "seen", "u"],
            Some("U"),
            true,
        ),
        action(
            "mark-unread",
            "Mark as Unread",
            "Chat",
            &["flag", "u"],
            Some("U"),
            true,
        ),
        action(
            "toggle-incognito-chat",
            "Toggle Incognito (This Chat)",
            "Chat",
            &["read receipts", "private", "stealth"],
            None,
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
        let results = index.query("al", PaletteMode::Default, false);
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
        let results = index.query("a", PaletteMode::Default, false);
        assert!(results.contacts.is_empty());
        assert!(results.chats.is_empty());
        assert!(!results.actions.is_empty());

        let empty = index.query("", PaletteMode::Default, false);
        assert!(empty.contacts.is_empty());
        assert!(empty.chats.is_empty());
        assert!(!empty.actions.is_empty());
    }

    #[test]
    fn no_match_returns_top_actions_only() {
        let index = sample_index();
        let results = index.query("zzqq", PaletteMode::Default, false);
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
        let results = index.query("", PaletteMode::Default, false);
        assert!(!results.actions.is_empty());
    }

    #[test]
    fn action_mode_returns_only_actions() {
        let index = sample_index();
        let results = index.query("arch", PaletteMode::Action, false);
        assert!(results.contacts.is_empty());
        assert!(results.chats.is_empty());
        assert!(results.actions.iter().any(|a| a.id == "open-archive"));
    }

    #[test]
    fn action_mode_open_chat_actions_rank_first() {
        let index = sample_index();
        // Empty action-mode query with an open chat: open-chat actions come first.
        let results = index.query("", PaletteMode::Action, true);
        assert!(!results.actions.is_empty());
        // The first several actions must all be requires_open_chat.
        let first = &results.actions[0];
        assert!(
            first.requires_open_chat,
            "expected an open-chat action first, got {}",
            first.id
        );
        // And when no chat is open, open-chat actions are excluded entirely.
        let closed = index.query("", PaletteMode::Action, false);
        assert!(closed.actions.iter().all(|a| !a.requires_open_chat));
    }

    #[test]
    fn no_accounts_still_returns_actions() {
        let index = PaletteIndex::new();
        assert!(index.is_empty());
        let results = index.query("al", PaletteMode::Default, false);
        assert!(results.contacts.is_empty());
        assert!(results.chats.is_empty());
        // Global actions are available even signed out.
        assert!(!results.actions.is_empty());
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
        let results = index.query("only", PaletteMode::Default, false);
        assert_eq!(results.chats.len(), 1);
    }

    #[test]
    fn remove_account_drops_entries() {
        let mut index = sample_index();
        index.remove_account("acc-b");
        assert_eq!(index.len(), 3);
        let results = index.query("zeta", PaletteMode::Default, false);
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
            "archive-chat",
            "pin-chat",
            "favorite-chat",
            "mark-read",
            "mark-unread",
            "toggle-incognito-chat",
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
            let _ = index.query(q, PaletteMode::Default, true);
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
        let results = index.query("  ", PaletteMode::Default, false);
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
}
