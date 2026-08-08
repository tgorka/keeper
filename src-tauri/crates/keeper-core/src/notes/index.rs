//! The note index: a model, not a database (epic 35, story 35.4).
//!
//! A personal vault tops out around ten thousand files, so the whole index is a
//! few megabytes of strings held in memory and rebuilt from disk on demand. That
//! is a deliberate position: a SQLite table beside `sync.db` would buy nothing at
//! this size and would cost a cache-invalidation bug forever, because every
//! external write by Obsidian, an agent or a `git checkout` happens behind its
//! back. What persists is only the advisory `<vault>/.keeper/index.json`
//! ([`IndexCache`]), and a cache that disagrees with disk is a rescan, never an
//! error (AD-57).
//!
//! The load-bearing property here is **incrementality**. Story 38.1 feeds this
//! module one changed path at a time and NFR-28 caps the steady-state cost of
//! absorbing one, so [`IndexBuilder::apply`] must never rebuild the tag tree or
//! the link graph from scratch. It does not: the snapshot keeps posting lists
//! (tag path → note count, link key → source note ids, link key → owner note ids)
//! and mutates exactly the buckets the changed note touches. Absorbing an
//! [`NoteDelta::Upsert`] costs O(tags + links of that one note), never O(vault).
//!
//! Publication is copy-on-write. The builder holds an `Arc<IndexSnapshot>` and
//! mutates it through [`Arc::make_mut`], so a delta that lands while no reader
//! holds the previous snapshot is applied in place with no copy at all; a reader
//! holding one forces exactly one clone, which is the unavoidable price of
//! handing out an immutable snapshot.
//!
//! Everything here is pure: plain values in, plain values out. No filesystem, no
//! `gix`, no `tauri` — the shell (`keeper::notes_vault`) does the IO and hands
//! this module finished [`IndexEntry`] values.

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::sync::Arc;

use serde::{Deserialize, Serialize};

use crate::notes::tags::TagNode;

/// Schema version of the on-disk [`IndexCache`]. Bump it whenever the meaning or
/// the shape of an [`IndexEntry`] field changes; the loader's only response to a
/// mismatch is discard-and-cold-scan, so a bump is always safe and never a
/// migration.
pub const INDEX_SCHEMA: u32 = 1;

/// The `IndexEntry.fields` key carrying the note's provenance class, written by
/// the reconciler from the trailers of the last commit touching the file
/// (AD-63): `local`, `agent` or `remote`.
///
/// It is namespaced under `keeper.` because `fields` is otherwise the note's own
/// frontmatter, and a user is entitled to a frontmatter key called `origin`. An
/// absent value reads as `local` — a note nobody has committed yet is this
/// device's (see [`crate::notes::query`]'s `origin:` predicate).
pub const FIELD_ORIGIN: &str = "keeper.origin";

/// The `IndexEntry.fields` key carrying the `Keeper-Device` label of the device
/// that last wrote the note. Backs `origin:device:<label>`. Namespaced for the
/// same reason as [`FIELD_ORIGIN`].
pub const FIELD_DEVICE: &str = "keeper.device";

/// The `IndexEntry.fields` key carrying the last time *this device* opened the
/// note, as `YYYY-MM-DD` or RFC3339. Backs `date:touched`.
///
/// It lives in `fields` rather than on [`IndexEntry`] because the contract froze
/// the entry's field list and a per-device timestamp is not derivable from the
/// file. When it is absent the `date:touched` predicate degrades to `modified`
/// rather than failing — a missing local fact must not break a shared space.
pub const FIELD_TOUCHED: &str = "keeper.touched";

/// The separator [`crate::notes::frontmatter::FieldValue::index_string`] uses to
/// flatten a list field into the single string stored in [`IndexEntry::fields`].
///
/// A newline, because a frontmatter scalar is single-line by construction and so
/// can never contain one — whereas a comma appears inside ordinary values
/// ("Doe, Jane"). The `field:` predicate splits on this to implement the
/// "`=` against a list means contains" rule.
pub const FIELD_LIST_SEPARATOR: char = '\n';

/// One indexed note: everything a list row, a space query or a link lookup needs,
/// without reopening the file.
///
/// `size`, `mtime_ns` and `ino` are the revalidation triple. Nothing downstream
/// trusts a cached entry without comparing them against one `lstat`, which is why
/// they are carried here rather than derived: a match adopts the cached parse, a
/// mismatch re-parses that one note, and neither path costs a read of the other
/// 9 999 files.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct IndexEntry {
    /// The note's stable ULID, minted into frontmatter, which is what lets links,
    /// pins, unread marks and history survive a rename (FR-97).
    pub id: String,
    /// Vault-relative path with `/` separators, e.g. `journal/2026/2026-08-02.md`.
    /// The identity the filesystem uses, and the key deltas arrive against.
    pub path: String,
    /// Display title: frontmatter `title`, else the first heading, else the stem.
    pub title: String,
    /// File size in bytes (revalidation triple).
    pub size: u64,
    /// Modification time in nanoseconds since the Unix epoch (revalidation
    /// triple). Nanoseconds because a second-granularity mtime cannot tell two
    /// writes inside the same second apart, which is exactly the case an editor's
    /// save-then-format produces.
    pub mtime_ns: i128,
    /// Inode number (revalidation triple). Catches the atomic rename-into-place
    /// an editor performs, where size and mtime can both be unchanged.
    pub ino: u64,
    /// Creation time, ms since the Unix epoch: frontmatter `created`, else the
    /// first commit touching the file, else the file's birth time.
    pub created_ms: i64,
    /// Last-modification time, ms since the Unix epoch: frontmatter `updated`,
    /// else the file mtime.
    pub updated_ms: i64,
    /// Normalised tag paths, the union of frontmatter `tags` and inline `#a/b`
    /// tags, sorted (see [`crate::notes::tags::note_tags`]).
    pub tags: Vec<String>,
    /// Frontmatter fields flattened to strings for querying, plus the reserved
    /// `keeper.*` keys above. Stringified through
    /// [`crate::notes::frontmatter::FieldValue::index_string`], so a list is
    /// [`FIELD_LIST_SEPARATOR`]-joined.
    pub fields: BTreeMap<String, String>,
    /// Outbound link targets exactly as written in the body — a title, an alias
    /// or a vault-relative path. Resolution to a note happens here, in the index,
    /// not at extraction time, because a link may point at a note that does not
    /// exist yet.
    pub links: Vec<String>,
    /// Index-computed booleans, as strings so the set can grow without a schema
    /// bump: `pinned`, `archived`, `unread`, `conflict`, `journal`, `template`,
    /// `space`, `capture`, `orphan`, `unstable_identity`, `unparsed`. Backs the
    /// `is:` predicate.
    pub flags: Vec<String>,
    /// A short body excerpt for the list row, so rendering a window of rows never
    /// touches the filesystem.
    pub snippet: String,
}

impl IndexEntry {
    /// Whether this entry carries `flag` (the `is:` predicate's storage).
    pub fn has_flag(&self, flag: &str) -> bool {
        self.flags.iter().any(|f| f.as_str() == flag)
    }

    /// Every key something can use to link to this note: its id, its
    /// vault-relative path, that path without the `.md`, its filename stem and
    /// its title — all folded through [`link_key`].
    ///
    /// One definition, used by the alias posting list, the backlink lookup and
    /// the query layer's `link:` binding alike. Two definitions of "what names
    /// this note" is a bug waiting to happen: the moment the resolver and the
    /// backlink map disagree, a link renders but its source never appears in the
    /// target's backlinks, and nothing in the UI can explain why.
    pub fn link_keys(&self) -> BTreeSet<String> {
        let mut keys = BTreeSet::new();
        for raw in [
            self.id.as_str(),
            self.path.as_str(),
            self.title.as_str(),
            path_stem(&self.path),
        ] {
            let key = link_key(raw);
            if !key.is_empty() {
                keys.insert(key);
            }
        }
        keys
    }
}

/// An immutable, publishable view of one vault's index.
///
/// Readers take an `Arc` of this out of a `tokio::sync::watch` and are never
/// blocked by a write, because the single reconciler task that owns the
/// [`IndexBuilder`] is the only mutator. Entries are kept sorted by `path`, which
/// makes `entries()` deterministic and turns [`Self::by_path`] into a binary
/// search — deliberately *not* a `path → index` map, because every insertion
/// would then have to renumber it, turning an O(1) delta into an O(vault) one.
#[derive(Debug, Clone, Default)]
pub struct IndexSnapshot {
    /// Every note, sorted by `path`.
    entries: Vec<IndexEntry>,
    /// `note id → path`. Indirect through the path on purpose: a Vec shift moves
    /// entries but never changes a path, so this map survives every insertion
    /// untouched.
    id_to_path: HashMap<String, String>,
    /// `tag path → how many things are under it`, where every ancestor prefix is
    /// its own key. Something tagged `project/keeper` increments `project` and
    /// `project/keeper` once each, so a parent's count is the number of distinct
    /// things in its subtree — the number the tag chip promises when you click
    /// it, and the same set the `tag:` predicate's segment-prefix rule matches.
    ///
    /// **Two producers feed this, and the count is their sum** (Story 42.5,
    /// FR-143). Notes arrive through [`Self::project`] / [`Self::retract`];
    /// recording sessions arrive through [`Self::upsert_recording_tags`] /
    /// [`Self::remove_recording_tags`]. Both go through the same
    /// [`Self::credit_tags`] / [`Self::debit_tags`] pair over the same
    /// [`tag_closure`], which is what makes them one vocabulary rather than two
    /// that agree for a while. A node that says 7 means 7 things.
    tag_counts: BTreeMap<String, u32>,
    /// `recording session id → the normalised tags that session contributes`.
    ///
    /// The recording producer's equivalent of `entries`: the builder needs the
    /// PREVIOUS tag list of a session to retract it before crediting the new
    /// one, or re-reporting an unchanged session would inflate every count it
    /// touches. Sessions are hundreds, their tags a handful each, so this is a
    /// few kilobytes — and it is the only reason a recording upsert stays
    /// O(that session's tags) instead of O(archive).
    recording_tags: BTreeMap<String, Vec<String>>,
    /// `link key → the ids of the notes that answer to that key`. A note answers
    /// to its id, its path, its path without `.md`, its filename stem and its
    /// title, so a wikilink written any of those ways resolves.
    aliases: HashMap<String, BTreeSet<String>>,
    /// `link key → the ids of the notes whose body links to that key`. The
    /// backlink posting list; mutated per changed note, never rebuilt.
    link_sources: HashMap<String, BTreeSet<String>>,
}

impl IndexSnapshot {
    /// Every note, sorted by path.
    pub fn entries(&self) -> &[IndexEntry] {
        &self.entries
    }

    /// Look a note up by its stable id.
    pub fn by_id(&self, id: &str) -> Option<&IndexEntry> {
        let path = self.id_to_path.get(id)?;
        self.by_path(path)
    }

    /// Look a note up by its vault-relative path.
    pub fn by_path(&self, path: &str) -> Option<&IndexEntry> {
        self.slot_for(path).ok().map(|at| &self.entries[at])
    }

    /// Where `path` sits in `entries`, or where it would go — the one place the
    /// sorted-vector invariant is turned into an index.
    fn slot_for(&self, path: &str) -> Result<usize, usize> {
        self.entries.binary_search_by(|e| e.path.as_str().cmp(path))
    }

    /// The hierarchical tag tree with counts (FR-104), over BOTH producers
    /// (Story 42.5).
    ///
    /// Projected from the maintained `tag_counts` map, so this is O(distinct
    /// tags) — hundreds — and never O(notes + recordings). Nothing here knows
    /// which producer contributed what, and that is the design: the tree is the
    /// vocabulary, and a count is the sum of everything behind it. Building it
    /// iteratively rather than recursively is not style: tag paths come out of
    /// user files, and a pathologically deep one must cost stack space we chose,
    /// not stack space it chose.
    pub fn tag_tree(&self) -> Vec<TagNode> {
        let mut roots: Vec<TagNode> = Vec::new();
        // The chain of ancestors currently open, deepest last. `tag_counts` is a
        // BTreeMap, and `/` (0x2F) sorts below every character a tag segment can
        // start with, so a parent always arrives immediately before its subtree
        // and `projects` always after all of `project/…`.
        let mut open: Vec<TagNode> = Vec::new();
        for (path, count) in &self.tag_counts {
            while matches!(open.last(), Some(top) if !is_tag_descendant(path, &top.path)) {
                close_tag_node(&mut open, &mut roots);
            }
            open.push(TagNode {
                name: last_tag_segment(path).to_owned(),
                path: path.clone(),
                count: *count,
                children: Vec::new(),
            });
        }
        while !open.is_empty() {
            close_tag_node(&mut open, &mut roots);
        }
        roots
    }

    /// Every note that links to the note with this id, sorted by path (FR-108).
    ///
    /// A note is reached through any of its keys, so a body that writes
    /// `[[Vault as a lens]]` and one that writes `[[notes/vault-as-a-lens]]` both
    /// count. Self-links are excluded: a note is not its own backlink.
    pub fn backlinks(&self, id: &str) -> Vec<&IndexEntry> {
        let Some(entry) = self.by_id(id) else {
            return Vec::new();
        };
        let mut sources: BTreeSet<&str> = BTreeSet::new();
        for key in entry.link_keys() {
            let Some(ids) = self.link_sources.get(&key) else {
                continue;
            };
            for source in ids {
                // A note is not its own backlink.
                if source.as_str() != id {
                    sources.insert(source.as_str());
                }
            }
        }
        let mut rows: Vec<&IndexEntry> =
            sources.into_iter().filter_map(|s| self.by_id(s)).collect();
        rows.sort_by(|a, b| a.path.cmp(&b.path));
        rows
    }

    /// Resolve a raw link target — a title, an alias or a vault-relative path —
    /// to the note it names, or `None` when nothing answers to it (FR-108/FR-109).
    ///
    /// Ambiguity is resolved by path order rather than reported: two notes titled
    /// "Meeting" both answer to `[[Meeting]]`, and a link that renders as *a*
    /// meeting is better than a link that renders as an error.
    pub fn resolve_link(&self, target: &str) -> Option<&IndexEntry> {
        let ids = self.aliases.get(&link_key(target))?;
        ids.iter()
            .filter_map(|id| self.by_id(id))
            .min_by(|a, b| a.path.cmp(&b.path))
    }

    /// Number of indexed notes.
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Whether the index holds no notes at all (a vault before its first scan).
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Insert or replace one note, keyed by path.
    ///
    /// The previous version at that path is retracted from every posting list
    /// first, so counts can never double-count a re-index of the same file.
    fn upsert(&mut self, entry: IndexEntry) {
        self.remove_path(&entry.path);
        self.project(&entry);
        let at = self.slot_for(&entry.path).unwrap_or_else(|at| at);
        self.entries.insert(at, entry);
    }

    /// Drop the note at `path`, retracting it from every posting list. A path the
    /// index never knew is a no-op, because a `Remove` can legitimately arrive
    /// for a file that was excluded, unparsed or already gone.
    fn remove_path(&mut self, path: &str) {
        let Ok(at) = self.slot_for(path) else {
            return;
        };
        let removed = self.entries.remove(at);
        self.retract(&removed);
    }

    /// Add one entry to the posting lists. O(its own tags + links).
    fn project(&mut self, entry: &IndexEntry) {
        self.id_to_path.insert(entry.id.clone(), entry.path.clone());
        self.credit_tags(&entry.tags);
        for key in entry.link_keys() {
            self.aliases
                .entry(key)
                .or_default()
                .insert(entry.id.clone());
        }
        for target in &entry.links {
            self.link_sources
                .entry(link_key(target))
                .or_default()
                .insert(entry.id.clone());
        }
    }

    /// Remove one entry from the posting lists, dropping buckets that empty and
    /// tag counts that reach zero — the latter is what makes "removing the last
    /// note carrying a tag removes the tag from the tree" true.
    fn retract(&mut self, entry: &IndexEntry) {
        // Only clear the id mapping if it still points at *this* entry: a rename
        // seen as upsert-then-remove would otherwise erase the new location.
        if self.id_to_path.get(&entry.id) == Some(&entry.path) {
            self.id_to_path.remove(&entry.id);
        }
        self.debit_tags(&entry.tags);
        for key in entry.link_keys() {
            discard_posting(&mut self.aliases, &key, &entry.id);
        }
        for target in &entry.links {
            discard_posting(&mut self.link_sources, &link_key(target), &entry.id);
        }
    }

    /// Add one thing's tags — and every ancestor of them — to `tag_counts`.
    ///
    /// **One of exactly two functions that may touch `tag_counts` upward.** Both
    /// producers call it, which is what makes "the count is the sum" structural
    /// rather than a thing two code paths remember to agree on.
    fn credit_tags(&mut self, tags: &[String]) {
        for tag in tag_closure(tags) {
            *self.tag_counts.entry(tag).or_insert(0) += 1;
        }
    }

    /// Take one thing's tags back out of `tag_counts`, retiring any key that
    /// reaches zero.
    ///
    /// **The decrement path, shared by both producers.** Retiring the key rather
    /// than leaving a zero is what makes "removing the last carrier of a tag
    /// removes the tag from the tree" true — and, because ancestors are credited
    /// separately, what lets a leaf go while a sibling keeps the parent alive.
    fn debit_tags(&mut self, tags: &[String]) {
        for tag in tag_closure(tags) {
            // Read, decide, then write — never hold a `get_mut` borrow across the
            // `remove` that retires the same key.
            let current = self.tag_counts.get(&tag).copied().unwrap_or(0);
            if current <= 1 {
                self.tag_counts.remove(&tag);
            } else {
                self.tag_counts.insert(tag, current - 1);
            }
        }
    }

    /// Insert or replace one recording session's contribution to the tag tree
    /// (Story 42.5).
    ///
    /// `tags` must already be canonical — the archive normalises at its own
    /// boundary ([`crate::archive::recordings::RecordingRow::from_manifest`]),
    /// and re-normalising here would be the second place the rule lives. What
    /// this owns is the posting arithmetic, and it is the same arithmetic
    /// [`Self::upsert`] does for a note: retract the previous version first, so
    /// re-reporting an unchanged session is idempotent and a session that lost a
    /// tag actually loses it.
    ///
    /// A session whose tags all normalise away is remembered with an empty list
    /// rather than forgotten, so a later report that gives it tags still knows
    /// there was nothing to retract.
    fn upsert_recording_tags(&mut self, session_id: String, tags: Vec<String>) {
        if let Some(previous) = self.recording_tags.get(&session_id).cloned() {
            self.debit_tags(&previous);
        }
        self.credit_tags(&tags);
        self.recording_tags.insert(session_id, tags);
    }

    /// Drop one recording session's contribution. A session the index never saw
    /// is a no-op, on [`Self::remove_path`]'s terms.
    fn remove_recording_tags(&mut self, session_id: &str) {
        if let Some(previous) = self.recording_tags.remove(session_id) {
            self.debit_tags(&previous);
        }
    }

    /// Every known tag path with its count, ascending — the flat vocabulary a
    /// completion surface offers (Story 42.5, FR-143).
    ///
    /// The same numbers [`Self::tag_tree`] projects, from the same map, because
    /// a completion list that disagreed with the sidebar about what exists would
    /// be the third vocabulary. Ancestors are included as their own entries, so
    /// `client` is offered beside `client/acme`: a person narrowing by hand
    /// wants the parent as much as the leaf.
    pub fn tag_vocabulary(&self) -> impl Iterator<Item = (&str, u32)> + '_ {
        self.tag_counts
            .iter()
            .map(|(path, count)| (path.as_str(), *count))
    }
}

/// Drop `id` from the posting list at `key`, removing the bucket when it empties
/// so an abandoned key never keeps a `HashMap` slot alive for the process's life.
fn discard_posting(map: &mut HashMap<String, BTreeSet<String>>, key: &str, id: &str) {
    let emptied = match map.get_mut(key) {
        Some(bucket) => {
            bucket.remove(id);
            bucket.is_empty()
        }
        None => false,
    };
    if emptied {
        map.remove(key);
    }
}

/// Move the deepest open tag node into its parent (or into the roots).
fn close_tag_node(open: &mut Vec<TagNode>, roots: &mut Vec<TagNode>) {
    if let Some(done) = open.pop() {
        match open.last_mut() {
            Some(parent) => parent.children.push(done),
            None => roots.push(done),
        }
    }
}

/// Whether `path` is a strict tag descendant of `ancestor` (`a/b` under `a`, but
/// `ab` not under `a`).
fn is_tag_descendant(path: &str, ancestor: &str) -> bool {
    path.strip_prefix(ancestor)
        .is_some_and(|rest| rest.starts_with('/'))
}

/// The last `/`-separated segment of a tag path — its display name.
fn last_tag_segment(path: &str) -> &str {
    path.rsplit('/').next().unwrap_or(path)
}

/// Every tag path one thing contributes to, including ancestors, deduped.
///
/// A note tagged both `project` and `project/keeper` contributes *one* to
/// `project`, not two, which is why this is a set rather than a flat count. The
/// recording producer counts through the very same closure (Story 42.5), so a
/// session and a note tagged alike weigh exactly the same in every node.
fn tag_closure(tags: &[String]) -> BTreeSet<String> {
    let mut closure = BTreeSet::new();
    for tag in tags {
        // Every prefix that ends at a separator, then the tag itself.
        for (at, _) in tag.match_indices('/') {
            if at > 0 {
                closure.insert(tag[..at].to_owned());
            }
        }
        if !tag.is_empty() {
            closure.insert(tag.clone());
        }
    }
    closure
}

/// The filename of a vault-relative path with its extension removed.
fn path_stem(path: &str) -> &str {
    let file = path.rsplit('/').next().unwrap_or(path);
    file.strip_suffix(".md").unwrap_or(file)
}

/// Fold a raw link target into the key both posting lists agree on.
///
/// Wikilinks are written the way a human thinks of the note, so the same target
/// arrives as `Vault as a Lens`, `notes/vault as a lens.md`, `./notes/vault as a
/// lens` and `Vault as a Lens#Why`. All four have to hash to one key or backlinks
/// silently under-report, which is worse than being slightly permissive.
pub fn link_key(target: &str) -> String {
    let mut key = target.trim();
    // A section or block reference addresses a place *inside* a note; the note is
    // still the link target.
    if let Some((head, _)) = key.split_once('#') {
        key = head.trim_end();
    }
    let key = key.replace('\\', "/");
    let key = key.trim_start_matches("./").trim_start_matches('/');
    let key = key.strip_suffix(".md").unwrap_or(key);
    key.trim().to_lowercase()
}

/// One change to absorb. The reconciler produces these; the builder applies them.
///
/// `Upsert` carries its entry boxed: an `IndexEntry` is ~240 bytes of strings and
/// vectors while the other two variants are a path and nothing, and a watcher
/// burst moves a queue of these. The indirection costs one allocation per changed
/// note — which the parse that produced the entry already paid many times over.
#[derive(Debug, Clone)]
pub enum NoteDelta {
    /// A note appeared or changed. Keyed by `path`: the entry replaces whatever
    /// was at that path.
    Upsert(Box<IndexEntry>),
    /// A note is gone from `path` (deleted, or moved away — the move's other half
    /// arrives as its own `Upsert`).
    Remove {
        /// Vault-relative path of the note that vanished.
        path: String,
    },
    /// Everything the index believes is suspect — a watcher overflow, a
    /// `broadcast` lag, a manual rebuild. Empties the index so the cold scan that
    /// follows starts from nothing rather than merging into stale state.
    Rescan,
}

/// One change to the tag tree's SECOND producer: the recording archive (Story
/// 42.5, FR-143).
///
/// A sibling of [`NoteDelta`] rather than a variant of it, because a recording
/// is not a note and never will be an [`IndexEntry`] — it has no vault-relative
/// path, no body, no links. What the two share is the only thing they need to:
/// the posting arithmetic on `tag_counts`, which both reach through
/// [`IndexBuilder`] and nothing else. There is no parallel path into the counts
/// for a recording to drift down.
///
/// The tags carried here are already canonical (see
/// [`crate::notes::tags::normalise_all`]); the archive normalises at its own
/// boundary and the index takes it at its word, because a second normalisation
/// here would be a second place the rule lives.
#[derive(Debug, Clone)]
pub enum RecordingTagDelta {
    /// A session appeared, or its tags changed. Keyed by session id: the list
    /// replaces whatever that session last contributed.
    Upsert {
        /// The session's stable id (Story 40.3), which survives a retitle.
        session_id: String,
        /// The session's canonical tags, deduplicated.
        tags: Vec<String>,
    },
    /// A session no longer contributes tags — it left the archive, or the index
    /// is being reseeded and this one was not in the seed.
    Remove {
        /// The session that stopped contributing.
        session_id: String,
    },
}

/// The single mutator of one vault's index.
///
/// One reconciler task owns one builder, so no lock is needed anywhere in this
/// module. Publication is copy-on-write through [`Arc::make_mut`]: a delta
/// applied while nobody holds the last snapshot mutates it in place.
#[derive(Debug, Default)]
pub struct IndexBuilder {
    snapshot: Arc<IndexSnapshot>,
}

impl IndexBuilder {
    /// An empty index.
    pub fn new() -> Self {
        Self::default()
    }

    /// Build from a finished scan (or from a validated `.keeper/index.json`).
    ///
    /// Sorts once and projects once rather than looping through `upsert`: a cold
    /// scan of ten thousand notes through sorted insertion would memmove the tail
    /// of the vector ten thousand times, which is precisely the O(vault²) shape
    /// story 35.6's five-second cold-index budget cannot afford.
    ///
    /// Duplicate paths are impossible from a real scan but cheap to survive, so
    /// the last entry for a path wins rather than corrupting the counts.
    pub fn from_entries(mut entries: Vec<IndexEntry>) -> Self {
        entries.sort_by(|a, b| a.path.cmp(&b.path));
        // `dedup_by` keeps the first of each run; swapping makes the *last*
        // arrival the survivor without cloning it.
        entries.dedup_by(|later, kept| {
            if later.path == kept.path {
                std::mem::swap(later, kept);
                true
            } else {
                false
            }
        });
        let mut snapshot = IndexSnapshot::default();
        for entry in &entries {
            snapshot.project(entry);
        }
        snapshot.entries = entries;
        Self {
            snapshot: Arc::new(snapshot),
        }
    }

    /// Absorb one change.
    ///
    /// Incremental by contract, not by accident (story 35.4, NFR-28): the tag
    /// counts and both link posting lists are edited in the buckets this note
    /// touches and nowhere else. No sibling entry is read or written.
    pub fn apply(&mut self, delta: NoteDelta) {
        let snapshot = Arc::make_mut(&mut self.snapshot);
        match delta {
            NoteDelta::Upsert(entry) => snapshot.upsert(*entry),
            NoteDelta::Remove { path } => {
                snapshot.remove_path(&path);
            }
            NoteDelta::Rescan => *snapshot = IndexSnapshot::default(),
        }
    }

    /// The current snapshot, cheap to hand to any number of readers.
    pub fn snapshot(&self) -> Arc<IndexSnapshot> {
        Arc::clone(&self.snapshot)
    }

    /// Absorb one change from the recording producer (Story 42.5).
    ///
    /// The counterpart of [`Self::apply`], and incremental on the same terms:
    /// exactly the tag buckets this session touches are edited, so absorbing one
    /// finalized recording costs O(its own tags) and never O(archive). This is
    /// the ONLY way a recording reaches the tag tree.
    pub fn apply_recording_tags(&mut self, delta: RecordingTagDelta) {
        let snapshot = Arc::make_mut(&mut self.snapshot);
        match delta {
            RecordingTagDelta::Upsert { session_id, tags } => {
                snapshot.upsert_recording_tags(session_id, tags);
            }
            RecordingTagDelta::Remove { session_id } => {
                snapshot.remove_recording_tags(&session_id);
            }
        }
    }
}

/// The `<vault>/.keeper/index.json` document. Serde only — the shell does the IO
/// and owns the temp-then-rename write.
///
/// Every field in it is derivable from the files it describes, which is what
/// makes it advisory: a bad `schema`, a `vault_id` that belongs to another vault,
/// a truncated file or any parse failure all take the same branch — discard and
/// cold-scan (AD-57). There is no repair path because there is nothing worth
/// repairing.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct IndexCache {
    /// [`INDEX_SCHEMA`] at write time. Anything else means discard.
    pub schema: u32,
    /// The vault this cache belongs to; a mismatch means discard.
    pub vault_id: String,
    /// When the cache was written, ms since the Unix epoch. Diagnostics only —
    /// freshness is decided per entry by the `(size, mtime_ns, ino)` triple, not
    /// by this.
    pub built_ms: i64,
    /// Every entry as of the write.
    pub entries: Vec<IndexEntry>,
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A minimal entry; tests override only what they are about.
    fn entry(id: &str, path: &str, title: &str) -> IndexEntry {
        IndexEntry {
            id: id.to_owned(),
            path: path.to_owned(),
            title: title.to_owned(),
            size: 1,
            mtime_ns: 1,
            ino: 1,
            created_ms: 0,
            updated_ms: 0,
            tags: Vec::new(),
            fields: BTreeMap::new(),
            links: Vec::new(),
            flags: Vec::new(),
            snippet: String::new(),
        }
    }

    fn tagged(id: &str, path: &str, tags: &[&str]) -> IndexEntry {
        let mut e = entry(id, path, path);
        e.tags = tags.iter().map(|t| (*t).to_owned()).collect();
        e
    }

    /// Find a node by full tag path anywhere in the tree.
    fn find<'a>(nodes: &'a [TagNode], path: &str) -> Option<&'a TagNode> {
        for node in nodes {
            if node.path == path {
                return Some(node);
            }
            if let Some(hit) = find(&node.children, path) {
                return Some(hit);
            }
        }
        None
    }

    #[test]
    fn entries_are_path_sorted_and_addressable_both_ways() {
        let builder =
            IndexBuilder::from_entries(vec![entry("b", "z.md", "Zed"), entry("a", "a.md", "Ay")]);
        let snap = builder.snapshot();
        let paths: Vec<&str> = snap.entries().iter().map(|e| e.path.as_str()).collect();
        assert_eq!(paths, ["a.md", "z.md"], "entries() is deterministic");
        assert_eq!(snap.by_id("b").map(|e| e.path.as_str()), Some("z.md"));
        assert_eq!(snap.by_path("a.md").map(|e| e.id.as_str()), Some("a"));
        assert_eq!(snap.by_id("nope"), None);
        assert_eq!(snap.len(), 2);
        assert!(!snap.is_empty());
    }

    #[test]
    fn a_parent_tag_counts_distinct_notes_in_its_subtree_not_tag_occurrences() {
        // A note tagged both `project` and `project/keeper` is ONE note under
        // `project`. Counting occurrences would make the chip promise two rows
        // and then show one.
        let builder = IndexBuilder::from_entries(vec![
            tagged("a", "a.md", &["project", "project/keeper"]),
            tagged("b", "b.md", &["project/keeper"]),
            tagged("c", "c.md", &["projects"]),
        ]);
        let tree = builder.snapshot().tag_tree();
        assert_eq!(find(&tree, "project").map(|n| n.count), Some(2));
        assert_eq!(find(&tree, "project/keeper").map(|n| n.count), Some(2));
        // `projects` is a sibling root, never a child of `project`.
        assert_eq!(find(&tree, "projects").map(|n| n.count), Some(1));
        let project = find(&tree, "project").expect("project node");
        assert_eq!(project.name, "project");
        assert_eq!(project.children.len(), 1, "only keeper hangs off project");
    }

    #[test]
    fn an_upsert_updates_tags_and_backlinks_without_touching_siblings() {
        // Story 35.4's acceptance: absorbing one change must not rewrite the
        // vault. Asserted by heap identity — a sibling's `title` buffer pointer
        // is stable across the delta iff nothing rewrote that entry. (A Vec
        // reallocation moves the `String` structs but never their buffers, so
        // this stays true even when the insert grows the vector.)
        let mut builder = IndexBuilder::from_entries(vec![
            tagged("a", "a.md", &["alpha"]),
            tagged("b", "b.md", &["beta"]),
        ]);
        let sibling_buffer = {
            let snap = builder.snapshot();
            let sibling = snap.by_path("a.md").expect("sibling present");
            sibling.title.as_ptr()
        };

        let mut changed = tagged("b", "b.md", &["beta/two"]);
        changed.links = vec!["a.md".to_owned()];
        builder.apply(NoteDelta::Upsert(Box::new(changed)));

        let snap = builder.snapshot();
        assert_eq!(
            snap.by_path("a.md")
                .expect("sibling survives")
                .title
                .as_ptr(),
            sibling_buffer,
            "the untouched note was not rewritten"
        );
        // The changed note's own postings did move.
        let tree = snap.tag_tree();
        assert!(find(&tree, "beta/two").is_some(), "new tag is in the tree");
        assert_eq!(
            find(&tree, "beta").map(|n| n.count),
            Some(1),
            "the parent survives with the subtree count"
        );
        let inbound = snap.backlinks("a");
        assert_eq!(inbound.len(), 1);
        assert_eq!(inbound[0].id, "b");
        assert_eq!(snap.backlinks("b").len(), 0, "links are directional");
    }

    #[test]
    fn removing_the_last_note_with_a_tag_drops_it_but_a_sibling_keeps_the_parent() {
        let mut builder = IndexBuilder::from_entries(vec![
            tagged("a", "a.md", &["work/keeper"]),
            tagged("b", "b.md", &["work/other"]),
            tagged("c", "c.md", &["solo"]),
        ]);

        builder.apply(NoteDelta::Remove {
            path: "a.md".to_owned(),
        });
        let tree = builder.snapshot().tag_tree();
        assert!(
            find(&tree, "work/keeper").is_none(),
            "the leaf's last carrier left, so the leaf left"
        );
        let work = find(&tree, "work").expect("parent survives a surviving sibling");
        assert_eq!(work.count, 1);
        assert_eq!(work.children.len(), 1);

        builder.apply(NoteDelta::Remove {
            path: "c.md".to_owned(),
        });
        let tree = builder.snapshot().tag_tree();
        assert!(find(&tree, "solo").is_none(), "an emptied root disappears");
        assert_eq!(tree.len(), 1, "only `work` is left");
    }

    #[test]
    fn a_link_resolves_through_title_path_and_stem_and_retracts_on_change() {
        let mut linker = entry("l", "notes/linker.md", "Linker");
        linker.links = vec![
            "Vault as a Lens".to_owned(),
            "notes/other.md".to_owned(),
            "third#Section".to_owned(),
        ];
        let builder_entries = vec![
            entry("t", "notes/vault-as-a-lens.md", "Vault as a Lens"),
            entry("o", "notes/other.md", "Other"),
            entry("x", "third.md", "Third"),
            linker,
        ];
        let mut builder = IndexBuilder::from_entries(builder_entries);
        let snap = builder.snapshot();
        for id in ["t", "o", "x"] {
            let inbound = snap.backlinks(id);
            assert_eq!(inbound.len(), 1, "{id} has one backlink");
            assert_eq!(inbound[0].id, "l");
        }
        assert_eq!(
            snap.resolve_link("VAULT AS A LENS").map(|e| e.id.as_str()),
            Some("t"),
            "resolution is case-folded"
        );
        assert_eq!(snap.resolve_link("nothing here"), None);

        // Rewriting the note with one link left retracts the other two.
        let mut trimmed = entry("l", "notes/linker.md", "Linker");
        trimmed.links = vec!["notes/other.md".to_owned()];
        builder.apply(NoteDelta::Upsert(Box::new(trimmed)));
        let snap = builder.snapshot();
        assert_eq!(snap.backlinks("o").len(), 1, "the kept link stays");
        assert!(snap.backlinks("t").is_empty(), "the dropped link is gone");
        assert!(snap.backlinks("x").is_empty(), "the dropped link is gone");
    }

    #[test]
    fn rescan_empties_the_index_rather_than_merging_into_stale_state() {
        let mut builder = IndexBuilder::from_entries(vec![tagged("a", "a.md", &["x"])]);
        builder.apply(NoteDelta::Rescan);
        let snap = builder.snapshot();
        assert!(snap.is_empty());
        assert!(snap.tag_tree().is_empty(), "posting lists went with it");
        assert_eq!(snap.by_id("a"), None);
    }

    #[test]
    fn a_published_snapshot_is_never_mutated_under_its_reader() {
        // The copy-on-write publication has to be honest: a reader holding the
        // previous Arc must keep seeing the previous vault.
        let mut builder = IndexBuilder::from_entries(vec![entry("a", "a.md", "Ay")]);
        let held = builder.snapshot();
        builder.apply(NoteDelta::Upsert(Box::new(entry("b", "b.md", "Bee"))));
        assert_eq!(held.len(), 1, "the held snapshot did not grow");
        assert_eq!(builder.snapshot().len(), 2);
    }

    #[test]
    fn the_cache_round_trips_as_camel_case_json() {
        let cache = IndexCache {
            schema: INDEX_SCHEMA,
            vault_id: "v".to_owned(),
            built_ms: 1_700_000_000_000,
            entries: vec![entry("a", "a.md", "Ay")],
        };
        let json = serde_json::to_string(&cache).expect("serialize cache");
        assert!(json.contains("\"vaultId\":\"v\""), "json was: {json}");
        assert!(json.contains("\"mtimeNs\":1"), "json was: {json}");
        let back: IndexCache = serde_json::from_str(&json).expect("deserialize cache");
        assert_eq!(back.entries, cache.entries);
    }

    // -----------------------------------------------------------------------
    // Story 42.5: the tag tree's second producer
    // -----------------------------------------------------------------------

    /// Report one recording the way the archive does: through the one
    /// normalisation, from the text the user actually typed.
    fn record(builder: &mut IndexBuilder, session_id: &str, typed: &[&str]) {
        builder.apply_recording_tags(RecordingTagDelta::Upsert {
            session_id: session_id.to_owned(),
            tags: crate::notes::tags::normalise_all(typed.iter().copied()),
        });
    }

    /// A note tagged the way a vault does: through the same one normalisation.
    fn note_tagged(id: &str, path: &str, typed: &[&str]) -> IndexEntry {
        let mut e = entry(id, path, path);
        e.tags = crate::notes::tags::normalise_all(typed.iter().copied());
        e
    }

    #[test]
    fn a_recording_and_a_note_tagged_differently_land_on_one_tree_node() {
        // AC1, asserted over the TREE and not over the normaliser: a recording
        // tagged `Client/Acme ` and a note tagged `client/acme` are one node
        // carrying both, not two nodes carrying one each. This is the defect the
        // whole story exists to delete.
        let mut builder = IndexBuilder::from_entries(vec![note_tagged(
            "n1",
            "notes/renewal.md",
            &["client/acme"],
        )]);
        record(&mut builder, "01DEVICE-01CALL", &["Client/Acme "]);

        let tree = builder.snapshot().tag_tree();
        let client = find(&tree, "client").expect("the `client` root exists");
        assert_eq!(
            client.children.len(),
            1,
            "one node under `client`, not one per casing: {:?}",
            client.children.iter().map(|c| &c.path).collect::<Vec<_>>()
        );
        assert_eq!(
            find(&tree, "client/acme").map(|n| n.count),
            Some(2),
            "the note and the recording are both under it"
        );
    }

    #[test]
    fn a_tag_nodes_count_is_the_sum_of_every_producer_behind_it() {
        // AC4 and the matrix's counts row: 2 notes and 3 recordings under
        // `client/acme` is 5. A node that says 5 means 5 things.
        let mut builder = IndexBuilder::from_entries(vec![
            note_tagged("n1", "a.md", &["client/acme"]),
            note_tagged("n2", "b.md", &["client/acme/renewal"]),
        ]);
        record(&mut builder, "s1", &["client/acme"]);
        record(&mut builder, "s2", &["Client/Acme"]);
        record(&mut builder, "s3", &["client/acme/renewal"]);

        let tree = builder.snapshot().tag_tree();
        assert_eq!(find(&tree, "client/acme").map(|n| n.count), Some(5));
        // The matrix's hierarchy row: each ancestor is counted once per thing,
        // so the parent is 5 as well — not 5 plus the two that named a child.
        assert_eq!(find(&tree, "client").map(|n| n.count), Some(5));
        assert_eq!(find(&tree, "client/acme/renewal").map(|n| n.count), Some(2));
    }

    #[test]
    fn removing_the_last_recording_under_a_leaf_takes_the_leaf_and_leaves_the_parent() {
        // AC3 and the matrix's last-recording-removed row. The sibling is what
        // makes this a real test: a decrement that took the parent with the leaf
        // would also pass an assertion that only checked the leaf.
        let mut builder = IndexBuilder::new();
        record(&mut builder, "s1", &["client/acme"]);
        record(&mut builder, "s2", &["client/other"]);
        assert!(find(&builder.snapshot().tag_tree(), "client/acme").is_some());

        builder.apply_recording_tags(RecordingTagDelta::Remove {
            session_id: "s1".to_owned(),
        });

        let tree = builder.snapshot().tag_tree();
        assert!(
            find(&tree, "client/acme").is_none(),
            "the leaf's last carrier left, so the leaf left"
        );
        assert_eq!(
            find(&tree, "client").map(|n| n.count),
            Some(1),
            "the sibling keeps the parent alive, at the sibling's count"
        );
        assert!(find(&tree, "client/other").is_some());

        // And the last carrier of the parent takes the whole branch.
        builder.apply_recording_tags(RecordingTagDelta::Remove {
            session_id: "s2".to_owned(),
        });
        assert!(builder.snapshot().tag_tree().is_empty());
    }

    #[test]
    fn a_note_keeps_a_tag_alive_after_its_last_recording_leaves() {
        // The cross-producer half of AC3: the decrement is over ONE map, so a
        // producer retracting its last carrier must not remove a tag the other
        // producer still carries.
        let mut builder =
            IndexBuilder::from_entries(vec![note_tagged("n1", "a.md", &["client/acme"])]);
        record(&mut builder, "s1", &["client/acme"]);
        builder.apply_recording_tags(RecordingTagDelta::Remove {
            session_id: "s1".to_owned(),
        });

        let tree = builder.snapshot().tag_tree();
        assert_eq!(
            find(&tree, "client/acme").map(|n| n.count),
            Some(1),
            "the note still carries it"
        );
    }

    #[test]
    fn re_reporting_a_session_replaces_its_tags_instead_of_adding_them() {
        // Every recording is reported twice in the ordinary course of things —
        // once at start and once at finalize — and rebuilds report it again.
        // Without the retract-first, one session would count as three.
        let mut builder = IndexBuilder::new();
        record(&mut builder, "s1", &["client/acme"]);
        record(&mut builder, "s1", &["client/acme"]);
        assert_eq!(
            find(&builder.snapshot().tag_tree(), "client/acme").map(|n| n.count),
            Some(1),
            "the same session reported twice is still one session"
        );

        // A session whose tags changed between the two reports gives the old one
        // back rather than keeping both.
        record(&mut builder, "s1", &["internal"]);
        let tree = builder.snapshot().tag_tree();
        assert!(
            find(&tree, "client/acme").is_none(),
            "the tag it no longer carries is gone"
        );
        assert_eq!(find(&tree, "internal").map(|n| n.count), Some(1));
    }

    #[test]
    fn a_recording_whose_tags_all_normalise_away_contributes_no_node() {
        // The matrix's empty-after-normalising row, at the producer's end: `  `
        // and `///` are not tags, so they are not an empty node in the sidebar.
        let mut builder = IndexBuilder::new();
        record(&mut builder, "s1", &["  ", "///", "#---"]);
        assert!(
            builder.snapshot().tag_tree().is_empty(),
            "nothing normalised, so nothing was counted"
        );

        // The session is still known, so giving it real tags later retracts
        // nothing and counts once.
        record(&mut builder, "s1", &["Acme", "acme"]);
        assert_eq!(
            find(&builder.snapshot().tag_tree(), "acme").map(|n| n.count),
            Some(1),
            "the duplicate collapsed before it was ever counted"
        );
    }

    #[test]
    fn the_flat_vocabulary_is_the_tree_by_another_projection() {
        // The completion surface and the sidebar must never disagree about what
        // exists or how many carry it, which is only guaranteed while both read
        // the one posting map.
        let mut builder =
            IndexBuilder::from_entries(vec![note_tagged("n1", "a.md", &["client/acme/renewal"])]);
        record(&mut builder, "s1", &["Client/Acme"]);
        let snapshot = builder.snapshot();

        let vocabulary: Vec<(String, u32)> = snapshot
            .tag_vocabulary()
            .map(|(path, count)| (path.to_owned(), count))
            .collect();
        assert_eq!(
            vocabulary,
            vec![
                ("client".to_owned(), 2),
                ("client/acme".to_owned(), 2),
                ("client/acme/renewal".to_owned(), 1),
            ],
            "ancestors are offered too, ascending, with the tree's own counts"
        );

        let tree = snapshot.tag_tree();
        for (path, count) in &vocabulary {
            assert_eq!(
                find(&tree, path).map(|n| n.count),
                Some(*count),
                "`{path}` disagrees between the vocabulary and the tree"
            );
        }
    }

    #[test]
    fn a_rescan_empties_the_recording_postings_with_everything_else() {
        // `Rescan` means "everything I believe is suspect". Leaving recording
        // postings behind would make the reconciler's reseed double them.
        let mut builder = IndexBuilder::new();
        record(&mut builder, "s1", &["client/acme"]);
        builder.apply(NoteDelta::Rescan);
        assert!(builder.snapshot().tag_tree().is_empty());

        // And the emptied index has forgotten the session, so a reseed that
        // reports it again counts it once rather than refusing to.
        record(&mut builder, "s1", &["client/acme"]);
        assert_eq!(
            find(&builder.snapshot().tag_tree(), "client/acme").map(|n| n.count),
            Some(1)
        );
    }
}
