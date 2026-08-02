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
    /// `tag path → notes under it`, where every ancestor prefix is its own key.
    /// A note tagged `project/keeper` increments `project` and `project/keeper`
    /// once each, so a parent's count is the number of distinct notes in its
    /// subtree — the number the tag chip promises when you click it, and the same
    /// set the `tag:` predicate's segment-prefix rule matches.
    tag_counts: BTreeMap<String, u32>,
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

    /// The hierarchical tag tree with counts (FR-104).
    ///
    /// Projected from the maintained `tag_counts` map, so this is O(distinct
    /// tags) — hundreds — and never O(notes). Building it iteratively rather than
    /// recursively is not style: tag paths come out of user files, and a
    /// pathologically deep one must cost stack space we chose, not stack space it
    /// chose.
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
        for tag in tag_closure(&entry.tags) {
            *self.tag_counts.entry(tag).or_insert(0) += 1;
        }
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
        for tag in tag_closure(&entry.tags) {
            // Read, decide, then write — never hold a `get_mut` borrow across the
            // `remove` that retires the same key.
            let current = self.tag_counts.get(&tag).copied().unwrap_or(0);
            if current <= 1 {
                self.tag_counts.remove(&tag);
            } else {
                self.tag_counts.insert(tag, current - 1);
            }
        }
        for key in entry.link_keys() {
            discard_posting(&mut self.aliases, &key, &entry.id);
        }
        for target in &entry.links {
            discard_posting(&mut self.link_sources, &link_key(target), &entry.id);
        }
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

/// Every tag path a note contributes to, including ancestors, deduped.
///
/// A note tagged both `project` and `project/keeper` contributes *one* to
/// `project`, not two, which is why this is a set rather than a flat count.
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
}
