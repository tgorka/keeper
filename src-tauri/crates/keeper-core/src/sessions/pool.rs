//! The flat session's markdown pool: bytes in, grouped values out (FR-256).
//!
//! In the flat contract a session is one pool of markdown at its own root, and
//! every file declares what it is in frontmatter (AD-120). This module is the
//! reader for that pool — it parses each file with the notes readers (one
//! parser, one writer, AD-109), decides each file's kind, and groups the pool
//! into the lists the detail renders.
//!
//! It also holds [`log_view`], the one place that knows both contracts: a
//! folder-shaped session's log comes from `## Log` inside its README, a flat
//! one's from its `log`-tagged files, and every caller downstream sees the same
//! three fields either way. That is what makes the flat shape an addition
//! rather than a rewrite — [`crate::sessions::vm::SessionLogEntryVm`] does not
//! change, and neither does anything that renders it.
//!
//! Pure, like the rest of the domain: the shell reads the directory and hands
//! in `(relative path, text)` pairs. Nothing here opens a file, and nothing
//! here writes one — in particular, a file with no `id` is **not** stamped with
//! a fresh ULID. See [`PoolEntry::id`].

use std::collections::BTreeMap;

use crate::notes::frontmatter::Frontmatter;
use crate::notes::index::IndexEntry;
use crate::notes::naming::{is_ulid, note_title};
use crate::notes::order::{read_order, NoteOrder};
use crate::notes::search::fold_cmp;
use crate::notes::tags::note_tags;
use crate::sessions::shape::{KindTag, Shape, TaskStatus};

/// One markdown file of the pool, already read by the shell.
///
/// `rel` is session-relative with `/` separators — `about.md`,
/// `2026-08-12-0930-opened.md`. The pool is the session's own root, so these
/// are bare filenames in practice; the type says `rel` because that is what it
/// is measured against and because `path:` identity quotes it verbatim.
#[derive(Debug, Clone, Copy)]
pub struct PoolFile<'a> {
    pub rel: &'a str,
    pub text: &'a str,
}

/// One parsed member of the pool.
#[derive(Debug, Clone, PartialEq)]
pub struct PoolEntry {
    /// Session-relative path — what opens the file.
    pub rel: String,
    /// The frontmatter `id` when it is a ULID, else `path:<rel>`.
    ///
    /// **keeper never stamps.** A file it did not author keeps its bytes, so a
    /// hand-written or agent-written markdown file is indexed by path and says
    /// so via [`PoolEntry::unstable_identity`]. Minting an id at index time
    /// would mean *opening a session folder mutates every file in it* — the
    /// scan would dirty a real git tree and sync would commit changes nobody
    /// made. This is the same rule the notes index applies (FR-121), stated a
    /// third time only because a third reader now needs it.
    pub id: String,
    /// The id above is path-derived: pins and marks will not survive a rename.
    pub unstable_identity: bool,
    /// Frontmatter `title`, else the first heading or line, else the stem.
    pub title: String,
    /// Frontmatter tags unioned with inline `#a/b`, normalised (one set).
    pub tags: Vec<String>,
    /// Every frontmatter key flattened to a string, exactly as the notes index
    /// flattens one ([`crate::notes::frontmatter::FieldValue::index_string`]).
    ///
    /// Carried so a pool file can answer `field:status=todo` — which is what
    /// the board's four columns *are* (FR-259). Projecting a `PoolEntry` into
    /// an [`crate::notes::index::IndexEntry`] is what runs a space over the
    /// pool, and an entry with an empty `fields` map would make every `field:`
    /// term silently false: the board would draw four empty columns over a
    /// session full of tasks and look like it was working.
    ///
    /// The whole map rather than a `status` slot, because a session's spaces
    /// are ordinary saved queries and the query language reaches any key. A
    /// typed slot here would mean `field:owner=me` works in notes and not in
    /// sessions, for no reason a user could discover.
    pub fields: BTreeMap<String, String>,
    /// Which kind this file declares, or `None` for an unfiled file.
    pub kind: Option<KindTag>,
    /// The task state. `Some` only when `kind == Some(KindTag::Task)`.
    pub status: Option<TaskStatus>,
    /// A `status:` was present but unreadable — the card says so rather than
    /// silently sitting in "to do".
    pub status_unreadable: bool,
    /// Position within its column, **with its provenance** — the whole
    /// [`NoteOrder`], not a bare `f64`.
    ///
    /// Keeping the source is the same decision the notes list already made: a
    /// card sitting third because the file says `order: 3` and a card sitting
    /// third because the file says `order: soon` and was defaulted are two
    /// different facts, and only one of them is worth showing the operator. An
    /// absent key takes the default and is never stamped.
    pub order: NoteOrder,
    /// `YYYY-MM-DD` parsed from a `YYYY-MM-DD-HHMM-slug.md` filename, else "".
    pub date: String,
    /// `HH:MM` from the same filename, else "".
    pub time: String,
    /// Byte offset where the body starts, for the caller that wants prose.
    pub body_at: usize,
    /// The frontmatter block exists but the parser could not model it.
    /// Surfaced, never repaired.
    pub unparsed: bool,
}

impl PoolEntry {
    /// The entry's prose, given the same text the entry was parsed from.
    ///
    /// Takes the text back rather than owning a copy: a pool is read on every
    /// detail open and most callers want the metadata only.
    pub fn body<'a>(&self, text: &'a str) -> &'a str {
        text.get(self.body_at..).unwrap_or("")
    }
}

/// Project a pool file into what [`crate::notes::query::eval`] already takes.
///
/// **The one evaluator, asked a second question.** A session's spaces are the
/// same saved queries notes have — same grammar, same chips, same editor — so
/// running one over the pool must not mean a second implementation of `tag:`,
/// `field:` and `is:`. It means handing the existing one an entry. A parallel
/// evaluator is how `tag:ref` would come to mean one thing in a note and
/// another in a session, and neither surface would be obviously wrong.
///
/// `mtime_ns` is the shell's — the domain has no clock and does not stat — and
/// `updated_ms` is derived from it so `date:` and a `modified` sort answer
/// something rather than treating every file as born at the epoch. The
/// frontmatter `created`/`updated` keys still win where present, because
/// [`crate::notes::query::resolve_date`] reads `fields` first; this is only the
/// floor beneath them.
///
/// Three of [`IndexEntry`]'s fields are deliberately **not** filled:
///
/// - `size` and `ino` are the revalidation triple's other two thirds, and the
///   pool has no cache to revalidate: it is read whole on every detail open.
///   Zeros here are honest — nothing reads them on this path — where a
///   plausible-looking guess would be a number some later cache could trust.
/// - `links` is empty, so `link:`/`backlink:` select nothing in a session
///   space. That is the same degradation an unbound `link:` already takes in
///   notes (see [`crate::notes::query::bind_index`]): those two predicates are
///   answered by a whole-index resolver, and a session pool is not an index.
///   Extracting outbound links here would make `link:` half-work — matching
///   literal paths but never a `[[Wikilink]]` — which is worse than not
///   matching, because a half-working predicate reads as a working one.
///
/// `flags` carries what the pool actually knows: `unstable_identity` and
/// `unparsed`, which are facts about the file. It does **not** carry `pinned`,
/// `journal`, `template` or the rest — those are notes-vault facts, and
/// `is:pinned` in a session space is therefore false rather than wrong.
pub fn as_index_entry(entry: &PoolEntry, mtime_ns: i128) -> IndexEntry {
    let mut flags = Vec::new();
    if entry.unstable_identity {
        flags.push("unstable_identity".to_owned());
    }
    if entry.unparsed {
        flags.push("unparsed".to_owned());
    }
    let updated_ms = i64::try_from(mtime_ns / 1_000_000).unwrap_or(i64::MAX);
    IndexEntry {
        id: entry.id.clone(),
        path: entry.rel.clone(),
        title: entry.title.clone(),
        size: 0,
        mtime_ns,
        ino: 0,
        created_ms: updated_ms,
        updated_ms,
        tags: entry.tags.clone(),
        fields: entry.fields.clone(),
        links: Vec::new(),
        flags,
        // Empty rather than a body excerpt, and this is the one place the two
        // projections differ on purpose: `notes_vault` builds a snippet because
        // its list renders one without reopening the file, and the pool's
        // caller already holds the text (`PoolEntry::body` takes it back). A
        // `text:` term still reads the real body through `eval`'s `body`
        // closure, so nothing is lost — only a copy of every file's first 240
        // characters that no session surface asks for.
        snippet: String::new(),
        order: entry.order,
    }
}

/// The pool, grouped by kind, each list already in its own display order.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Pool {
    pub about: Vec<PoolEntry>,
    /// Newest first.
    pub logs: Vec<PoolEntry>,
    pub prompts: Vec<PoolEntry>,
    pub refs: Vec<PoolEntry>,
    /// By `order`, then title.
    pub tasks: Vec<PoolEntry>,
    /// Files declaring no kind: the migration residue and the hand-dropped
    /// file. Empty for a clean session, and non-empty is the signal that
    /// something is not filed yet.
    pub unfiled: Vec<PoolEntry>,
}

/// Split a `YYYY-MM-DD-HHMM-slug.md` filename into its date and time.
///
/// The flat contract puts the clock in the filename so the folder sorts itself
/// in Finder, in `ls`, and in any tool that has never heard of keeper. That is
/// the whole reason logs are named this way, so reading it back is a string
/// operation and not a parse of the file's contents.
///
/// Returns `("", "")` for anything that is not shaped that way — an ordinary
/// name is not an error, it just carries no clock.
fn stamp_of(rel: &str) -> (String, String) {
    let name = rel.rsplit('/').next().unwrap_or(rel);
    let bytes = name.as_bytes();
    // YYYY-MM-DD-HHMM is 15 characters, and the separators are load-bearing.
    let shaped = bytes.len() >= 15
        && bytes[..4].iter().all(u8::is_ascii_digit)
        && bytes[4] == b'-'
        && bytes[5..7].iter().all(u8::is_ascii_digit)
        && bytes[7] == b'-'
        && bytes[8..10].iter().all(u8::is_ascii_digit)
        && bytes[10] == b'-'
        && bytes[11..15].iter().all(u8::is_ascii_digit);
    if !shaped {
        return (String::new(), String::new());
    }
    (
        name[..10].to_owned(),
        format!("{}:{}", &name[11..13], &name[13..15]),
    )
}

/// The filename stem, for a title fallback.
fn stem(rel: &str) -> &str {
    let name = rel.rsplit('/').next().unwrap_or(rel);
    name.strip_suffix(".md").unwrap_or(name)
}

/// Parse one pool file.
pub fn read_one(file: PoolFile<'_>) -> PoolEntry {
    let (fm, body_at) = Frontmatter::parse(file.text);
    let body = file.text.get(body_at..).unwrap_or("");

    let id = match fm.as_string("id") {
        Some(id) if is_ulid(id) => id.to_owned(),
        _ => format!("path:{}", file.rel),
    };
    let unstable_identity = id.starts_with("path:");

    // The same three-branch ladder every other surface shows, from the function
    // whose doc comment exists because two copies of it had already drifted.
    let title = note_title(fm.as_string("title"), body, stem(file.rel));

    let tags = note_tags(&fm, body);
    let kind = KindTag::of(&tags);

    // `status` is read only for a task. A `status:` on a log file is somebody's
    // own frontmatter and none of the board's business — reading it anyway
    // would put a card in a column for a file that is not a card.
    let (status, status_unreadable) = if kind == Some(KindTag::Task) {
        match fm.as_string("status") {
            Some(raw) if !raw.trim().is_empty() => match TaskStatus::parse(raw) {
                Some(status) => (Some(status), false),
                None => (None, true),
            },
            // No status at all is not an error: an unstated task is waiting,
            // which is exactly what `todo` means. Only a *present but
            // unreadable* value is worth a warning.
            _ => (Some(TaskStatus::Todo), false),
        }
    } else {
        (None, false)
    };

    let (date, time) = stamp_of(file.rel);

    // Flattened the same way `notes_vault`'s projection flattens one, through
    // the same `index_string` — so `field:` asks one question of a note and of
    // a session file, and a list-valued key joins on the same separator in both.
    let mut fields = BTreeMap::new();
    for key in fm.keys() {
        if let Some(value) = fm.get(key) {
            fields.insert(key.to_owned(), value.index_string());
        }
    }

    PoolEntry {
        rel: file.rel.to_owned(),
        id,
        unstable_identity,
        title,
        tags,
        fields,
        kind,
        status,
        status_unreadable,
        order: read_order(&fm),
        date,
        time,
        body_at,
        unparsed: fm.unparsed().is_some(),
    }
}

/// Parse the whole pool, in the order the shell supplied it.
pub fn read(files: &[PoolFile<'_>]) -> Vec<PoolEntry> {
    files.iter().copied().map(read_one).collect()
}

/// Group parsed entries by kind, each list sorted for its own purpose.
///
/// Order is decided **once per kind**, here, because each list answers a
/// different question:
///
/// - **logs** — newest first, by filename descending. The name carries
///   `YYYY-MM-DD-HHMM`, so this is a string sort that happens to be
///   chronological, and it matches the folder-shaped contract's own projection
///   (the file stays newest-last; the *review surface* reverses).
/// - **tasks** — by `order` through [`f64::total_cmp`], then folded title.
///   The same two terms [`crate::notes::order::cmp_order`] uses, for the same
///   reason: a comparator that returned `Equal` for two distinct files would
///   let hash order reshuffle the board between launches.
/// - **about, prompts, refs** — by folded filename ascending. Prompts are
///   numbered `NN-slug.md` precisely so that this sort is the useful one.
///
/// Every sort ends on `rel`, which is unique by construction, so all four are
/// total.
pub fn group(entries: Vec<PoolEntry>) -> Pool {
    let mut pool = Pool::default();
    for entry in entries {
        match entry.kind {
            Some(KindTag::About) => pool.about.push(entry),
            Some(KindTag::Log) => pool.logs.push(entry),
            Some(KindTag::Prompt) => pool.prompts.push(entry),
            Some(KindTag::Ref) => pool.refs.push(entry),
            Some(KindTag::Task) => pool.tasks.push(entry),
            None => pool.unfiled.push(entry),
        }
    }

    let by_name = |a: &PoolEntry, b: &PoolEntry| fold_cmp(&a.rel, &b.rel);
    pool.about.sort_by(by_name);
    pool.prompts.sort_by(by_name);
    pool.refs.sort_by(by_name);
    pool.unfiled.sort_by(by_name);
    pool.logs.sort_by(|a, b| by_name(b, a));
    pool.tasks.sort_by(|a, b| {
        a.order
            .value
            .total_cmp(&b.order.value)
            .then_with(|| fold_cmp(&a.title, &b.title))
            .then_with(|| a.rel.cmp(&b.rel))
    });
    pool
}

/// Read and group in one call — what the shell actually wants.
pub fn read_pool(files: &[PoolFile<'_>]) -> Pool {
    group(read(files))
}

/// Root `.md` names that could be logs, newest first — from the names alone.
///
/// The board draws one row per session and needs each session's newest log
/// line. Reading every markdown file in every session to find it would make
/// opening the board cost the whole zone; this narrows the field to the files
/// whose *name* carries a `YYYY-MM-DD-HHMM` stamp and orders them so the shell
/// can read the first one or two, stop, and still be right.
///
/// Only a candidate list, never a verdict: the tag decides whether a file is a
/// log, and a stamped name is not a tag. The shell reads down this list until
/// [`read_one`] yields `kind == Some(Log)`. A session whose logs are all named
/// something else simply has no cheap answer, and the board says nothing rather
/// than guessing — which is the same thing the folder shape does when its
/// README has no `## Log`.
pub fn log_candidates(names: &[String]) -> Vec<&str> {
    let mut out: Vec<&str> = names
        .iter()
        .map(String::as_str)
        .filter(|name| name.ends_with(".md") && !stamp_of(name).0.is_empty())
        .collect();
    out.sort_by(|a, b| fold_cmp(b, a));
    out
}

/// The session's log, whichever contract it follows.
///
/// Returns `(date, title, body)` triples, **newest first**, which is
/// [`crate::sessions::model::log_entries`]' own triple and the order the detail
/// already renders. One function, so the two contracts cannot drift into two
/// different ideas of what a log entry is.
///
/// - [`Shape::Folder`] — today's parse of `## Log`, reversed. Byte-identical
///   behaviour: the old path is not touched, only routed to.
/// - [`Shape::Flat`] — one triple per `log`-tagged file, already newest-first
///   from [`group`]. The date comes from the filename's stamp; a log file
///   whose name carries no stamp still appears, with an empty date, because
///   losing a sitting because it was named unusually would be worse than
///   showing one without a date.
pub fn log_view(shape: Shape, readme_body: &str, pool: &Pool) -> Vec<(String, String, String)> {
    match shape {
        Shape::Folder => {
            let mut entries = crate::sessions::model::log_entries(readme_body);
            entries.reverse();
            entries
        }
        Shape::Flat => pool
            .logs
            .iter()
            .map(|entry| (entry.date.clone(), entry.title.clone(), String::new()))
            .collect(),
    }
}

/// [`log_view`] for the flat shape, with bodies filled in from the pool's own
/// bytes.
///
/// Split from [`log_view`] because the body is the expensive half: the board
/// wants dates and titles for a hundred sessions, the detail wants prose for
/// one. `texts` is indexed in step with [`Pool::logs`].
pub fn log_view_with_bodies(pool: &Pool, texts: &[&str]) -> Vec<(String, String, String)> {
    pool.logs
        .iter()
        .enumerate()
        .map(|(index, entry)| {
            let body = texts
                .get(index)
                .map(|text| entry.body(text).trim().to_owned())
                .unwrap_or_default();
            (entry.date.clone(), entry.title.clone(), body)
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::notes::order::{NoteOrderSource, DEFAULT_NOTE_ORDER};

    const ULID: &str = "01J5AAAAAAAAAAAAAAAAAAAAAA";

    fn file<'a>(rel: &'a str, text: &'a str) -> PoolFile<'a> {
        PoolFile { rel, text }
    }

    /// keeper does not rewrite frontmatter it did not author: a file with no
    /// id is indexed by path and says so.
    #[test]
    fn a_file_with_no_id_is_indexed_by_path_and_flagged_unstable() {
        let entry = read_one(file("notes.md", "---\ntags: [ref]\n---\n# Notes\n"));
        assert_eq!(entry.id, "path:notes.md");
        assert!(entry.unstable_identity);

        let hand_written = read_one(file("scratch.md", "just prose, no frontmatter\n"));
        assert_eq!(hand_written.id, "path:scratch.md");
        assert!(hand_written.unstable_identity);
        assert_eq!(hand_written.kind, None, "an untagged file declares nothing");
    }

    /// A real ULID is kept verbatim; a foreign `id` is not trusted as one.
    #[test]
    fn a_26_char_ulid_is_kept_verbatim() {
        let entry = read_one(file(
            "a.md",
            &format!("---\nid: {ULID}\ntags: [log]\n---\n# x\n"),
        ));
        assert_eq!(entry.id, ULID);
        assert!(!entry.unstable_identity);

        // Another tool's `id` is somebody else's key, not ours.
        let foreign = read_one(file("b.md", "---\nid: task-1234\n---\n# x\n"));
        assert_eq!(foreign.id, "path:b.md");
        assert!(foreign.unstable_identity);
    }

    /// The title ladder: explicit, then the body's heading, then the stem.
    #[test]
    fn the_title_falls_back_from_frontmatter_to_heading_to_stem() {
        let explicit = read_one(file("a.md", "---\ntitle: Stated\n---\n# Heading\n"));
        assert_eq!(explicit.title, "Stated");

        let heading = read_one(file("b.md", "---\ntags: [log]\n---\n# Heading\n"));
        assert_eq!(heading.title, "Heading");

        let bare = read_one(file("2026-08-12-0930-opened.md", "---\ntags: [log]\n---\n"));
        assert_eq!(
            bare.title, "2026-08-12-0930-opened",
            "a nameless file still has a name on disk"
        );
    }

    /// The clock lives in the filename so the folder sorts itself.
    #[test]
    fn log_view_flat_shape_reads_date_and_time_from_the_filename() {
        let entry = read_one(file(
            "2026-08-12-0930-opened.md",
            "---\ntags: [log]\n---\n# Opened\n",
        ));
        assert_eq!(entry.date, "2026-08-12");
        assert_eq!(entry.time, "09:30");

        let unstamped = read_one(file("thoughts.md", "---\ntags: [log]\n---\n# x\n"));
        assert_eq!(unstamped.date, "");
        assert_eq!(unstamped.time, "");

        // Near-misses carry no clock rather than a wrong one.
        assert_eq!(
            stamp_of("2026-08-12-opened.md"),
            (String::new(), String::new())
        );
        assert_eq!(
            stamp_of("20260812-0930-x.md"),
            (String::new(), String::new())
        );
    }

    /// Newest first, by the name that carries the clock.
    #[test]
    fn logs_sort_newest_first_by_filename() {
        let pool = read_pool(&[
            file(
                "2026-08-10-0900-first.md",
                "---\ntags: [log]\n---\n# first\n",
            ),
            file(
                "2026-08-12-1400-third.md",
                "---\ntags: [log]\n---\n# third\n",
            ),
            file(
                "2026-08-11-1030-second.md",
                "---\ntags: [log]\n---\n# second\n",
            ),
        ]);
        let names: Vec<&str> = pool.logs.iter().map(|e| e.rel.as_str()).collect();
        assert_eq!(
            names,
            [
                "2026-08-12-1400-third.md",
                "2026-08-11-1030-second.md",
                "2026-08-10-0900-first.md"
            ]
        );
    }

    /// The board's order is the fact the operator set, then a tiebreak a
    /// reader can account for, then a term that makes it total.
    #[test]
    fn tasks_sort_by_order_then_title_using_total_cmp() {
        let pool = read_pool(&[
            file("c.md", "---\ntags: [task]\norder: 2\n---\n# Zebra\n"),
            file("a.md", "---\ntags: [task]\norder: 1.5\n---\n# Apple\n"),
            file("b.md", "---\ntags: [task]\norder: 1.5\n---\n# Banana\n"),
        ]);
        let titles: Vec<&str> = pool.tasks.iter().map(|e| e.title.as_str()).collect();
        assert_eq!(
            titles,
            ["Apple", "Banana", "Zebra"],
            "fractional orders sort between whole ones — what drag-to-reorder writes"
        );
    }

    /// An absent `order` is the default and is never stamped — and the entry
    /// carries *why* it holds the position it holds.
    #[test]
    fn an_absent_order_key_is_the_default_and_is_never_stamped() {
        let text = "---\ntags: [task]\n---\n# Task\n";
        let entry = read_one(file("t.md", text));
        assert_eq!(entry.order.value, DEFAULT_NOTE_ORDER);
        assert_eq!(entry.order.source, NoteOrderSource::Default);
        assert!(
            !text.contains("order:"),
            "reading a pool must not rewrite the file it read"
        );

        let stated = read_one(file("u.md", "---\ntags: [task]\norder: 3\n---\n# Task\n"));
        assert_eq!(stated.order.source, NoteOrderSource::Own);

        // Defaulted-because-unreadable is a different fact from never-stated,
        // and the board can say so rather than quietly disagreeing with the file.
        let broken = read_one(file(
            "v.md",
            "---\ntags: [task]\norder: soon\n---\n# Task\n",
        ));
        assert_eq!(broken.order.value, DEFAULT_NOTE_ORDER);
        assert_eq!(broken.order.source, NoteOrderSource::Unreadable);
    }

    /// A task with no status is waiting; one with an unreadable status says so
    /// rather than sitting silently in the wrong column.
    #[test]
    fn an_unreadable_status_is_reported_rather_than_coerced() {
        let ok = read_one(file("a.md", "---\ntags: [task]\nstatus: done\n---\n# x\n"));
        assert_eq!(ok.status, Some(TaskStatus::Done));
        assert!(!ok.status_unreadable);

        let missing = read_one(file("b.md", "---\ntags: [task]\n---\n# x\n"));
        assert_eq!(
            missing.status,
            Some(TaskStatus::Todo),
            "unstated means waiting"
        );
        assert!(!missing.status_unreadable);

        let broken = read_one(file(
            "c.md",
            "---\ntags: [task]\nstatus: blocked\n---\n# x\n",
        ));
        assert_eq!(broken.status, None);
        assert!(broken.status_unreadable);

        // A non-task's `status` is somebody else's frontmatter.
        let log = read_one(file(
            "d.md",
            "---\ntags: [log]\nstatus: blocked\n---\n# x\n",
        ));
        assert_eq!(log.status, None);
        assert!(!log.status_unreadable, "not a card, not a warning");
    }

    /// Kinds partition the pool, and what declares nothing is visible.
    #[test]
    fn an_untagged_root_md_lands_in_unfiled() {
        let pool = read_pool(&[
            file("about.md", "---\ntags: [about]\n---\n# About\n"),
            file("README.md", "# Left over from migration\n"),
            file("2026-08-12-0900-x.md", "---\ntags: [log]\n---\n# x\n"),
        ]);
        assert_eq!(pool.about.len(), 1);
        assert_eq!(pool.logs.len(), 1);
        assert_eq!(pool.unfiled.len(), 1);
        assert_eq!(pool.unfiled[0].rel, "README.md");
    }

    /// AD-120, from the read side, and the guard Story 50.1 owes its own fix.
    ///
    /// 50.1 teaches the create verb where a folder-shaped session keeps each
    /// kind — `refs/` for a reference, `prompts/` for a prompt
    /// (`shape::kind_dir`). The risk that change introduces is that somebody
    /// concludes the directory is now what makes a file a reference, and drops
    /// the tag from the write. It does not: the reader derives the kind from
    /// tags alone and never looks at the path, so an untagged file in `refs/`
    /// is *unfiled* — present in the pool, listed by no space, and named by the
    /// detail's Unfiled notice. The directory is where a create PUTS a file;
    /// the tag is what makes it that kind.
    #[test]
    fn a_file_in_refs_without_the_tag_is_still_unfiled() {
        let pool = read_pool(&[
            file("refs/tagged.md", "---\ntags: [ref]\n---\n# Tagged\n"),
            file("refs/bare.md", "# Someone wrote this by hand\n"),
            file("prompts/bare.md", "# And this one\n"),
        ]);

        assert_eq!(pool.refs.len(), 1, "only the tagged one is a reference");
        assert_eq!(pool.refs[0].rel, "refs/tagged.md");
        assert!(pool.prompts.is_empty(), "`prompts/` confers nothing either");
        let unfiled: Vec<&str> = pool.unfiled.iter().map(|e| e.rel.as_str()).collect();
        assert_eq!(unfiled, ["prompts/bare.md", "refs/bare.md"]);
        // Said once more at the level the mistake would be made at: `read_one`
        // is handed the `refs/` prefix and still answers `None`.
        assert_eq!(read_one(file("refs/bare.md", "# x\n")).kind, None);
    }

    /// Inline tags count: one tag set, however it was written.
    #[test]
    fn an_inline_tag_declares_a_kind_too() {
        let entry = read_one(file("x.md", "# Pointer\n\nSee #ref for the details.\n"));
        assert_eq!(entry.kind, Some(KindTag::Ref));
        assert!(entry.tags.iter().any(|t| t == "ref"));
    }

    /// Broken frontmatter is surfaced, never repaired.
    #[test]
    fn unparseable_frontmatter_is_flagged_not_repaired() {
        let entry = read_one(file("x.md", "---\ntags: [log\n  broken: [\n---\n# x\n"));
        assert!(entry.unparsed, "the file says what it says");
    }

    /// The no-regression test: the folder-shaped log is exactly what it was,
    /// reversed for review — the same bytes through the same parser.
    #[test]
    fn log_view_folder_shape_matches_log_entries_reversed() {
        let body = "# s\n\n## Log\n\n### 2026-08-10 — opened\n\nfirst\n\n### 2026-08-11 — closed\n\nsecond\n";
        let mut expected = crate::sessions::model::log_entries(body);
        expected.reverse();

        let got = log_view(Shape::Folder, body, &Pool::default());
        assert_eq!(got, expected);
        assert_eq!(got[0].0, "2026-08-11", "newest first for review");
        assert_eq!(got[0].1, "closed");
        assert_eq!(got[1].2, "first", "bodies survive the projection");
    }

    /// The flat log reads the pool, and the same triple comes out.
    #[test]
    fn log_view_flat_shape_reads_the_pool() {
        let pool = read_pool(&[
            file(
                "2026-08-10-0900-opened.md",
                "---\ntags: [log]\n---\n# opened\n",
            ),
            file(
                "2026-08-11-1700-closed.md",
                "---\ntags: [log]\n---\n# closed\n",
            ),
        ]);
        let got = log_view(Shape::Flat, "", &pool);
        assert_eq!(got.len(), 2);
        assert_eq!(
            got[0],
            ("2026-08-11".to_owned(), "closed".to_owned(), String::new())
        );
        assert_eq!(got[1].0, "2026-08-10");

        // With bodies, in the same order as the pool's own list.
        let texts = [
            "---\ntags: [log]\n---\n# closed\n\nwrapped up\n",
            "---\ntags: [log]\n---\n# opened\n\nstarted\n",
        ];
        let full = log_view_with_bodies(&pool, &texts);
        assert_eq!(full[0].2, "# closed\n\nwrapped up");
        assert_eq!(full[1].2, "# opened\n\nstarted");
    }

    /// The empty pool is not an error in either shape.
    #[test]
    fn an_empty_session_has_an_empty_log_in_both_shapes() {
        assert!(log_view(Shape::Flat, "", &Pool::default()).is_empty());
        assert!(log_view(Shape::Folder, "", &Pool::default()).is_empty());
    }

    /// The board's cheap path: stamped names, newest first, and nothing else
    /// offered — so the shell reads one or two files per session instead of the
    /// whole pool.
    #[test]
    fn log_candidates_are_stamped_names_newest_first() {
        let names: Vec<String> = [
            "about.md",
            "AGENTS.md",
            "2026-08-10-0000-opened.md",
            "2026-08-12-0900-shipped.md",
            "2026-08-11-1330-review.md",
            "README.md",
            "2026-08-12-notes.md",      // dated but no time: not the log naming
            "2026-08-13-0800-plan.txt", // stamped, but not markdown
            "artifacts",
        ]
        .iter()
        .map(|s| (*s).to_owned())
        .collect();

        assert_eq!(
            log_candidates(&names),
            [
                "2026-08-12-0900-shipped.md",
                "2026-08-11-1330-review.md",
                "2026-08-10-0000-opened.md",
            ],
            "only the full YYYY-MM-DD-HHMM markdown names, newest first"
        );
        assert!(log_candidates(&[]).is_empty());
    }

    /// A candidate is a name, not a verdict: the tag still decides. A stamped
    /// file that declares another kind must not be mistaken for the newest log.
    #[test]
    fn a_stamped_name_is_not_itself_a_log() {
        let entry = read_one(file(
            "2026-08-12-0900-shipped.md",
            "---\ntags: [ref]\n---\n# Shipped\n",
        ));
        assert_eq!(entry.kind, Some(KindTag::Ref));
        assert_eq!(
            log_candidates(&["2026-08-12-0900-shipped.md".to_owned()]).len(),
            1,
            "it is offered to the shell, which then reads it and moves on"
        );
    }

    // -----------------------------------------------------------------------
    // The projection: a pool file, seen by the one query evaluator
    // -----------------------------------------------------------------------

    /// Run a real query over a real pool file, through the real evaluator.
    ///
    /// Deliberately not a hand-built `IndexEntry`: the whole claim being tested
    /// is that the *projection* is faithful, and an entry written by hand in
    /// this file would test only that `eval` still works.
    fn selects(query: &str, rel: &str, text: &str) -> bool {
        let entry = read_one(file(rel, text));
        let projected = as_index_entry(&entry, 0);
        let parsed = crate::notes::query::parse(query)
            .unwrap_or_else(|err| panic!("parse `{query}`: {}", err.message));
        let mut body = || entry.body(text).to_owned();
        crate::notes::query::eval(&parsed, &projected, &mut body, 0)
    }

    /// The five zone spaces are five `tag:` queries, and each one has to select
    /// the kind it names off a real pool file.
    #[test]
    fn the_five_default_space_queries_select_their_own_kind() {
        for (query, tag) in [
            ("tag:about", "about"),
            ("tag:task", "task"),
            ("tag:log", "log"),
            ("tag:ref", "ref"),
            ("tag:prompt", "prompt"),
        ] {
            let text = format!("---\ntags: [{tag}]\n---\n# x\n");
            assert!(selects(query, "a.md", &text), "{query} should select {tag}");
            // And selects nothing else: five spaces that all matched everything
            // would look identical to five that all worked.
            for (other, _) in [
                ("tag:about", "about"),
                ("tag:task", "task"),
                ("tag:log", "log"),
                ("tag:ref", "ref"),
                ("tag:prompt", "prompt"),
            ] {
                if other != query {
                    assert!(!selects(other, "a.md", &text), "{other} matched {tag}");
                }
            }
        }
    }

    /// The board's whole grammar: four columns are four `field:status=` terms
    /// over the same `tag:task` set. If the projection dropped `fields`, every
    /// column would be empty and the board would look like an empty session
    /// rather than a broken projection — which is why this asserts the
    /// *positive* case rather than only that nothing crashes.
    #[test]
    fn a_field_term_selects_a_task_by_its_status() {
        let todo = "---\ntags: [task]\nstatus: todo\n---\n# Write it\n";
        assert!(selects("tag:task field:status=todo", "t.md", todo));
        assert!(!selects("tag:task field:status=done", "t.md", todo));
        assert!(selects("tag:task field:status!=done", "t.md", todo));

        // The four board columns partition a status-carrying pool: exactly one
        // of them holds this card.
        let hits = crate::sessions::shape::STATUSES
            .iter()
            .filter(|status| {
                selects(
                    &format!("tag:task field:status={}", status.as_str()),
                    "t.md",
                    todo,
                )
            })
            .count();
        assert_eq!(hits, 1, "a card sits in exactly one column");
    }

    /// A task whose file states no status is `Todo` in [`PoolEntry::status`] —
    /// but its *file* says nothing, so `field:status=todo` does not select it.
    ///
    /// That divergence is deliberate and worth pinning: the default lives in
    /// the reader, where it is a display decision, and never in `fields`, where
    /// it would be keeper answering a query about frontmatter with a value the
    /// frontmatter does not contain. The board fills its Todo column from the
    /// grouped pool, which has the default; a hand-written `field:status=todo`
    /// space asks the file, and gets the file's answer.
    #[test]
    fn an_absent_status_is_defaulted_for_the_board_but_never_invented_in_fields() {
        let bare = "---\ntags: [task]\n---\n# Untriaged\n";
        let entry = read_one(file("t.md", bare));
        assert_eq!(entry.status, Some(TaskStatus::Todo), "the reader defaults");
        assert!(
            !entry.fields.contains_key("status"),
            "the projection never invents a key the file does not have"
        );
        assert!(!selects("field:status=todo", "t.md", bare));
        assert!(
            selects("-field:status=done", "t.md", bare),
            "the negated form is how a space asks for `not done, or unstated`"
        );
    }

    /// `text:` still reads the real body: the projection carries no snippet, so
    /// a free-text term that silently matched nothing would be the failure.
    #[test]
    fn a_text_term_reads_the_body_through_the_projection() {
        let text = "---\ntags: [log]\n---\n# Opened\n\nthe migration landed\n";
        assert!(selects("text:migration", "l.md", text));
        assert!(selects("tag:log text:migration", "l.md", text));
        assert!(!selects("text:rollback", "l.md", text));
    }

    /// Two facts the pool knows travel as flags; the notes-vault ones do not,
    /// and are false rather than wrong.
    #[test]
    fn the_projection_carries_only_the_flags_the_pool_can_know() {
        let hand_written = as_index_entry(&read_one(file("x.md", "prose\n")), 0);
        assert!(hand_written.has_flag("unstable_identity"));
        assert!(!hand_written.has_flag("pinned"), "not a notes-vault fact");
        assert!(!hand_written.has_flag("template"));

        let broken = as_index_entry(&read_one(file("y.md", "---\ntags: [log\n---\n# y\n")), 0);
        assert!(broken.has_flag("unparsed"));

        // `is:` over a flag the pool cannot know is false, not an error: the
        // query parses, selects nothing, and says nothing untrue.
        assert!(!selects("is:pinned", "x.md", "prose\n"));
    }

    /// `link:` and `backlink:` degrade to selecting nothing, exactly as an
    /// unbound one does in notes — never to a half-working literal match.
    #[test]
    fn link_predicates_select_nothing_rather_than_half_working() {
        let text = "---\ntags: [ref]\n---\n# R\n\nsee [[Vault as a Lens]] and notes/vault.md\n";
        assert!(as_index_entry(&read_one(file("r.md", text)), 0)
            .links
            .is_empty());
        assert!(!selects("link:notes/vault.md", "r.md", text));
        assert!(!selects("backlink:notes/vault.md", "r.md", text));
    }

    /// The shell's mtime becomes the entry's timestamps, so a `modified` sort
    /// and a `date:` term have something true to work from — and frontmatter
    /// still wins over it, because `resolve_date` reads `fields` first.
    #[test]
    fn the_shells_mtime_floors_the_timestamps_and_frontmatter_outranks_it() {
        let entry = read_one(file("a.md", "---\ntags: [log]\n---\n# x\n"));
        let projected = as_index_entry(&entry, 1_700_000_000_000_000_000);
        assert_eq!(projected.updated_ms, 1_700_000_000_000);
        assert_eq!(projected.created_ms, 1_700_000_000_000);

        let stated = read_one(file(
            "b.md",
            "---\ntags: [log]\nupdated: 2020-01-02\n---\n# x\n",
        ));
        let projected = as_index_entry(&stated, 1_700_000_000_000_000_000);
        assert_eq!(
            crate::notes::query::resolve_date(crate::notes::query::DateField::Modified, &projected),
            crate::notes::query::stamp_ms("2020-01-02").expect("a readable stamp"),
            "the file's own word outranks the filesystem's"
        );
    }
}
