//! The saved queries a session is read through (FR-256, AD-119, AD-120, AD-79).
//!
//! The flat contract deleted `refs/`, `prompts/` and the rest, and it had to put
//! something in their place: a folder of undifferentiated markdown is only
//! navigable if something groups it. That something is a **space** — the same
//! saved query notes already have, over the same grammar, edited in the same
//! chip editor. About, Tasks, Log, References and Prompts are five queries, and
//! the directories they replaced were five hard-coded facts.
//!
//! **Where they live: `<zone>/_spaces/<name>.md`, beside `_template/`.**
//!
//! - Not per-session. The five are identical for every session in a zone, so a
//!   per-session copy means editing one query N times and getting it wrong on
//!   the N+1th — and it puts a directory back into the shape whose entire point
//!   is that it has none.
//! - Not built-in-only. AD-79's rule is that a space is a *file* you can rename,
//!   reorder, retitle and throw away. A hard-coded five would be the fixed rail
//!   Story 44.3 spent a whole story deleting, rebuilt one layer down.
//! - `_`-prefixed, so [`crate::sessions::model::skipped`] already hides it from
//!   the session walk. That function's doc comment named `_spaces/` before this
//!   module existed; no new rule was needed, which is the point of having had a
//!   rule rather than a list.
//!
//! **The directory is the ledger** — the one place this deliberately diverges
//! from [`crate::notes::default_spaces`], which carries a JSON ledger of the
//! names keeper has claimed. It needs one because `spaces/` in a vault holds the
//! *user's* spaces from day one and keeper arrives afterwards, so "absent" there
//! is genuinely ambiguous between never-offered and thrown-away. `_spaces/` is a
//! directory keeper introduces and nothing else writes: an absent one has never
//! been seeded, a present one is the operator's, and a space deleted out of it
//! stays deleted because the directory is still standing. One rule, no second
//! file to keep consistent with the first, and the same escape hatch —
//! [`SeedMode::Restore`] fills holes by key, and deleting the whole directory
//! asks for all five again.
//!
//! Pure, like the rest of the sessions domain: it takes bytes and returns
//! values. Reading `_spaces/`, writing into it and stat-ing the pool are the
//! shell's (AD-108).

use std::collections::BTreeSet;

use crate::notes::frontmatter::{FieldValue, Frontmatter};
use crate::notes::index::IndexEntry;
use crate::notes::naming;
use crate::notes::query;
use crate::notes::sort;
use crate::sessions::pool::{as_index_entry, PoolEntry};

/// Where a zone's space definitions live, zone-relative.
///
/// Named here because the shell must not compose a second spelling of it, and
/// because the leading underscore is load-bearing: it is what
/// [`crate::sessions::model::skipped`] keys on, so this directory is invisible
/// to the session walk without a rule of its own.
pub const SPACES_DIR: &str = "_spaces";

/// One default: a saved query with a name, a glyph and a rail position.
///
/// `key` is the identity and the one field the operator cannot change — the
/// name, icon, query, sort and position are all theirs the moment the file
/// exists (AD-79), so none of them can be what "this is the Tasks space" means.
/// It rides in the file's own frontmatter as `keeper.default`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct DefaultSessionSpace {
    /// The stable identity, written to `keeper.default`.
    pub key: &'static str,
    /// The name keeper gives it. Renaming it changes nothing but the name.
    pub name: &'static str,
    /// The query, in the DSL [`crate::notes::query`] parses.
    pub query: &'static str,
    /// The ordering, in the spelling [`crate::notes::sort::read`] reads.
    pub sort: &'static str,
    /// The icon name, from the editor's fixed set.
    pub icon: &'static str,
    /// Rail position.
    pub order: f64,
}

/// The five, in the order a session is read in.
///
/// **The order is the reading order, and it is the operator's own**: what this
/// session is, what is left to do, what happened, what it points at, what it was
/// told. It is not alphabetical, and it is not the order the deleted directories
/// happened to sort in — a space's position is a real field now
/// ([`crate::notes::sort::rail_order`]), so it can carry a meaning.
///
/// **They start at 1, not 0.** [`crate::notes::sort::DEFAULT_SPACE_ORDER`] is
/// `0.0` and means *unset*: a space whose file says nothing about its position
/// sorts as zero. Seeding About at zero would make it indistinguishable from
/// every unpositioned space the operator later writes, and the rail would sort
/// the two by name — putting a hand-made "Archive" above the About that is
/// supposed to be read first. One is the smallest number that is a statement.
///
/// **Each query is one `tag:` term and nothing else**, which is the flat
/// contract restated as data: a file's kind is what it says it is (AD-120), so
/// the space that shows a kind asks exactly that and infers nothing from a name,
/// a folder or a position. The board's `field:status=` columns are composed on
/// top of the Tasks query rather than baked into it — four spaces would be four
/// places to edit when the tag changes.
pub const DEFAULT_SESSION_SPACES: [DefaultSessionSpace; 5] = [
    DefaultSessionSpace {
        key: "about",
        name: "About",
        query: "tag:about",
        // Title, so a session with two about-ish files (the migrated `about.md`
        // and something the operator added) reads in a stable, nameable order
        // rather than reshuffling every time one is touched.
        sort: "name asc",
        icon: "info",
        order: 1.0,
    },
    DefaultSessionSpace {
        key: "tasks",
        name: "Tasks",
        query: "tag:task",
        // The file's own `order:`, because a task list is the one list a person
        // arranges by hand — and it is what the board writes when a card moves.
        sort: "order asc",
        icon: "list-todo",
        order: 2.0,
    },
    DefaultSessionSpace {
        key: "log",
        name: "Log",
        query: "tag:log",
        // Newest first, matching `SessionDetailVm::log`'s own projection: the
        // file on disk stays newest-last, and every review surface reverses.
        sort: "modified desc",
        icon: "history",
        order: 3.0,
    },
    DefaultSessionSpace {
        key: "refs",
        name: "References",
        query: "tag:ref",
        sort: "name asc",
        icon: "link",
        order: 4.0,
    },
    DefaultSessionSpace {
        key: "prompts",
        name: "Prompts",
        query: "tag:prompt",
        // Prompts are named `NN-slug.md` precisely so that a name sort is the
        // running order — the same reason `pool::group` sorts them this way.
        sort: "name asc",
        icon: "message-square",
        order: 5.0,
    },
];

/// The default carrying `key`, if any.
#[must_use]
pub fn by_key(key: &str) -> Option<&'static DefaultSessionSpace> {
    DEFAULT_SESSION_SPACES.iter().find(|space| space.key == key)
}

/// One space definition, read off its file.
///
/// The strings are stored **exactly as the file spells them**, and the parsed
/// forms are computed on demand by [`select`]. That is FR-121 as a data shape:
/// a definition that held only `SpaceSort` could not write `sort: bananas` back
/// out again, so opening a space keeper half-understood and saving it would
/// quietly repair — which is to say silently rewrite — the operator's file.
#[derive(Debug, Clone, PartialEq)]
pub struct SessionSpace {
    /// Zone-relative path (`_spaces/tasks.md`). The key.
    pub rel: String,
    /// `keeper.default`, when this is one of [`DEFAULT_SESSION_SPACES`] — and
    /// `None` for every space the operator wrote, including one that happens to
    /// share a name with a default.
    pub default_key: Option<String>,
    /// Frontmatter `title`, else the first heading, else the filename stem.
    pub name: String,
    /// `keeper.space`, verbatim. Empty when the file names no query, which is a
    /// space that selects nothing rather than a space that selects everything —
    /// see [`select`].
    pub query: String,
    /// `keeper.sort`, verbatim.
    pub sort: String,
    /// `keeper.icon`, trimmed, otherwise unread — the fixed set of glyphs is the
    /// editor's, and a name this crate does not recognise is not an error here.
    pub icon: Option<String>,
    /// `keeper.order`, read through [`crate::notes::sort::read_order`].
    pub order: f64,
    /// What keeper could not read and worked around, already worded. Empty for
    /// a file it understood entirely.
    pub warnings: Vec<String>,
}

/// Read one `_spaces/*.md`.
///
/// Reads the same one-level `keeper:` map `notes_ipc::space_def` reads, key for
/// key, because a session space and a note space are the same kind of file and
/// the editor that opens one opens the other (AD-109). A second shape here would
/// mean the chip editor could save a session space into a form the reader no
/// longer understands.
#[must_use]
pub fn read_one(rel: &str, text: &str) -> SessionSpace {
    let (fm, body_at) = Frontmatter::parse(text);
    let body = text.get(body_at..).unwrap_or("");
    let stem = rel
        .rsplit('/')
        .next()
        .unwrap_or(rel)
        .strip_suffix(".md")
        .unwrap_or(rel);
    let mut space = SessionSpace {
        rel: rel.to_owned(),
        default_key: None,
        name: naming::note_title(fm.as_string("title"), body, stem),
        query: String::new(),
        sort: String::new(),
        icon: None,
        order: sort::DEFAULT_SPACE_ORDER,
        warnings: Vec::new(),
    };
    if let Some(FieldValue::Map(pairs)) = fm.get("keeper") {
        for (key, value) in pairs {
            match (key.as_str(), value) {
                ("space", FieldValue::Str(query)) => space.query.clone_from(query),
                ("sort", FieldValue::Str(stored)) => space.sort.clone_from(stored),
                ("icon", FieldValue::Str(icon)) if !icon.trim().is_empty() => {
                    space.icon = Some(icon.trim().to_owned());
                }
                // Matched on the key alone and flattened, so `order: 2`,
                // `order: "2"` and `order: [a, b]` all reach one reader —
                // `space_def`'s own rule, for its own reason: two of the three
                // working and the third being silently absent is worse than all
                // three being answered.
                ("order", value) => {
                    let read = sort::read_order(&value.index_string());
                    space.order = read.order;
                    space.warnings.extend(read.warning);
                }
                ("default", FieldValue::Str(raw)) => {
                    space.default_key = by_key(raw.trim()).map(|d| d.key.to_owned());
                }
                _ => {}
            }
        }
    }
    space.warnings.extend(sort::read(&space.sort).warning);
    space
}

/// Read a whole `_spaces/` directory, in rail order.
///
/// Position then name, through [`crate::notes::sort::rail_order`] — the rail's
/// one comparison, so a session's spaces and a vault's cannot disagree about
/// what "above" means.
#[must_use]
pub fn read_all(files: &[(&str, &str)]) -> Vec<SessionSpace> {
    let mut spaces: Vec<SessionSpace> = files
        .iter()
        .filter(|(rel, _)| rel.ends_with(".md"))
        .map(|(rel, text)| read_one(rel, text))
        .collect();
    spaces.sort_by(|a, b| sort::rail_order((a.order, &a.name), (b.order, &b.name)));
    spaces
}

/// Why keeper is writing defaults right now.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SeedMode {
    /// Automatic, on a zone that has no `_spaces/` at all.
    FirstRun,
    /// The operator asked for the defaults back. Fills holes in a directory
    /// that already exists.
    Restore,
}

/// Which defaults to write, in [`DEFAULT_SESSION_SPACES`] order.
///
/// `existing` is `None` when the zone has **no `_spaces/` directory** and
/// `Some(spaces)` when it has one — and the difference is the whole rule. An
/// absent directory has never been seeded; a present one is the operator's, and
/// keeper adds nothing to it on its own however empty it looks. Deleting the
/// Prompts space therefore sticks, which is AD-79's requirement, without a
/// ledger file to keep in step with the directory.
///
/// A `Restore` fills what is missing **by key or by folded name**, the two ways
/// a default can already be there: keeper's own, and one the operator wrote and
/// called "Tasks". The name check uses [`naming::slug`]'s fold, the same fold
/// that decides two files cannot share a filename, so `Tasks`, `tasks` and
/// `  TASKS  ` are one name and not three misses.
#[must_use]
pub fn plan(
    mode: SeedMode,
    existing: Option<&[SessionSpace]>,
) -> Vec<&'static DefaultSessionSpace> {
    match (mode, existing) {
        // Never seeded: the zone gets the five it was designed around.
        (_, None) => DEFAULT_SESSION_SPACES.iter().collect(),
        // The directory is the ledger.
        (SeedMode::FirstRun, Some(_)) => Vec::new(),
        (SeedMode::Restore, Some(spaces)) => {
            let present = claimed(spaces);
            DEFAULT_SESSION_SPACES
                .iter()
                .filter(|space| !present.contains(space.key))
                .collect()
        }
    }
}

/// The default keys this zone's `_spaces/` already holds, whoever wrote them.
#[must_use]
pub fn claimed(existing: &[SessionSpace]) -> BTreeSet<String> {
    let keys: BTreeSet<&str> = existing
        .iter()
        .filter_map(|space| space.default_key.as_deref())
        .collect();
    let names: BTreeSet<String> = existing
        .iter()
        .map(|space| naming::slug(&space.name))
        .collect();
    DEFAULT_SESSION_SPACES
        .iter()
        .filter(|space| keys.contains(space.key) || names.contains(&naming::slug(space.name)))
        .map(|space| space.key.to_owned())
        .collect()
}

/// The file name a default is seeded under, zone-relative.
///
/// The bare slug — `_spaces/tasks.md` — and **not**
/// [`naming::note_filename`]'s dated `2026-08-14-tasks.md`, which is what a
/// vault's `spaces/` uses. A note filename carries the date because a vault is
/// a flat pile of thousands of notes and the date is what keeps a listing
/// browsable. `_spaces/` holds five files that are named after what they do, in
/// a directory an operator opens to edit a query, and a date there is noise that
/// makes the interesting part of the name start at character 12.
#[must_use]
pub fn rel_of(space: &DefaultSessionSpace) -> String {
    format!("{SPACES_DIR}/{}.md", naming::slug(space.name))
}

/// The file keeper writes for one default.
///
/// Byte for byte the shape a saved note space has — same reserved `keeper:`
/// map, same `# <name>` body — plus the `order` this rail uses and the marker
/// that makes it a default. `id` and `now` are parameters rather than reads
/// because the domain has neither a clock nor an id generator (AD-108), and
/// because a test then gets the same bytes on every machine.
#[must_use]
pub fn render_note(space: &DefaultSessionSpace, id: &str, now: &str) -> String {
    let front = Frontmatter::serialise_new(&[
        ("id".to_owned(), FieldValue::Str(id.to_owned())),
        ("created".to_owned(), FieldValue::Str(now.to_owned())),
        ("updated".to_owned(), FieldValue::Str(now.to_owned())),
        (
            "keeper".to_owned(),
            FieldValue::Map(vec![
                ("space".to_owned(), FieldValue::Str(space.query.to_owned())),
                ("sort".to_owned(), FieldValue::Str(space.sort.to_owned())),
                ("icon".to_owned(), FieldValue::Str(space.icon.to_owned())),
                ("order".to_owned(), FieldValue::Num(space.order)),
                ("default".to_owned(), FieldValue::Str(space.key.to_owned())),
            ]),
        ),
    ]);
    format!("{front}\n# {}\n", space.name)
}

/// One pool file as a space has to see it.
///
/// The three facts [`crate::notes::query::eval`] needs and a [`PoolEntry`] does
/// not carry: when the file changed (the domain has no clock and does not stat)
/// and its bytes (`text:` reads the real body, never the projection's empty
/// snippet).
#[derive(Debug, Clone, Copy)]
pub struct Candidate<'a> {
    pub entry: &'a PoolEntry,
    /// Modification time in nanoseconds, from the shell's own stat.
    pub mtime_ns: i128,
    /// The file's whole text, for a `text:` term.
    pub text: &'a str,
}

/// What running one space over one session's pool selected.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Selection {
    /// Indices into the candidate slice, in the space's own order.
    pub picked: Vec<usize>,
    /// The query would not parse, already worded. `picked` is then empty.
    ///
    /// **Empty, not everything.** A space whose query broke selecting the whole
    /// pool would look like a working space over a session that had gone wrong,
    /// and the operator would go looking in the wrong place. This is the same
    /// direction notes takes for a broken space (`space_lens` refuses), stated
    /// as a value here because the sessions surface renders several spaces at
    /// once and one bad query must not empty the other four.
    pub error: Option<String>,
}

/// Run one space over one session's pool (AD-65, AD-7).
///
/// **The one evaluator and the one comparator**, reached through
/// [`as_index_entry`]. A session space is an ordinary saved query — same
/// grammar, same chips, same editor — so running it must not mean a second
/// implementation of `tag:`, `field:` and `is:`, nor a second reading of
/// `sort: name asc`. A parallel pair is how `tag:ref` would come to mean one
/// thing in a note and another in a session, with neither surface obviously
/// wrong.
///
/// An **empty query selects nothing**, which is the asymmetry worth stating: an
/// empty *query string* is not the empty *query*, which matches everything. A
/// space whose `keeper.space` key was cleared or was never written is a space
/// that has not been told what to show, and showing it the whole session is the
/// answer most likely to be mistaken for a working one.
///
/// A sort keeper cannot read never fails the run — it falls back to
/// [`crate::notes::sort::DEFAULT_SORT`] and the space still selects what it
/// selects, with [`SessionSpace::warnings`] already carrying the sentence.
/// Refusing there would turn one bad word in frontmatter into an empty pane.
#[must_use]
pub fn select(space: &SessionSpace, candidates: &[Candidate<'_>], now_ms: i64) -> Selection {
    if space.query.trim().is_empty() {
        return Selection {
            picked: Vec::new(),
            error: Some(format!(
                "\"{}\" doesn't say what to show yet, so it shows nothing.",
                space.name
            )),
        };
    }
    let parsed = match query::parse(&space.query) {
        Ok(parsed) => parsed,
        Err(error) => {
            return Selection {
                picked: Vec::new(),
                error: Some(error.message),
            }
        }
    };
    let ordering = sort::read(&space.sort).sort;

    // Projected once per candidate rather than once per comparison: `compare`
    // is called O(n log n) times and each projection clones the entry's tags
    // and fields.
    let projected: Vec<IndexEntry> = candidates
        .iter()
        .map(|c| as_index_entry(c.entry, c.mtime_ns))
        .collect();

    let mut picked: Vec<usize> = (0..candidates.len())
        .filter(|&i| {
            let text = candidates[i].text;
            let entry = candidates[i].entry;
            // Built per candidate and read at most once, by the one predicate
            // that needs bytes. The body is a borrow of the shell's own buffer
            // until `eval` asks, which it only does for a `text:` term.
            let mut body = || entry.body(text).to_owned();
            query::eval(&parsed, &projected[i], &mut body, now_ms)
        })
        .collect();

    // Total by construction: the space's own comparison, then the path, which is
    // unique. A comparator returning `Equal` for two distinct files would let
    // hash order reshuffle the list between launches — `pool::group`'s rule.
    picked.sort_by(|&a, &b| {
        sort::compare(ordering, &projected[a], &projected[b])
            .then_with(|| projected[a].path.cmp(&projected[b].path))
    });

    Selection {
        picked,
        error: None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sessions::pool::{read_one as read_pool_one, PoolFile};

    const NOW: i64 = 1_760_000_000_000;

    fn pool_file<'a>(rel: &'a str, text: &'a str) -> PoolFile<'a> {
        PoolFile { rel, text }
    }

    /// Run a space over a hand-built pool, returning the paths it picked in the
    /// order it picked them. Deliberately through the real readers on both
    /// sides: a test that hand-built `PoolEntry`s would prove the sort works and
    /// nothing about whether a real file reaches it.
    fn run(space: &SessionSpace, files: &[(&str, &str)]) -> Result<Vec<String>, String> {
        let entries: Vec<PoolEntry> = files
            .iter()
            .map(|(rel, text)| read_pool_one(pool_file(rel, text)))
            .collect();
        let candidates: Vec<Candidate<'_>> = entries
            .iter()
            .enumerate()
            .map(|(i, entry)| Candidate {
                entry,
                // Ascending, so a `modified` sort has something to order by and
                // a later file in the slice is a newer one.
                mtime_ns: 1_700_000_000_000_000_000 + (i as i128) * 1_000_000_000,
                text: files[i].1,
            })
            .collect();
        let selection = select(space, &candidates, NOW);
        match selection.error {
            Some(error) => Err(error),
            None => Ok(selection
                .picked
                .into_iter()
                .map(|i| candidates[i].entry.rel.clone())
                .collect()),
        }
    }

    /// A space as the seeder writes it, read back the way the shell reads one.
    /// Both directions in one helper, so every assertion below is over bytes
    /// that made a round trip rather than over a struct built in this file.
    fn seeded(key: &str) -> SessionSpace {
        let default = by_key(key).unwrap_or_else(|| panic!("no default {key}"));
        let text = render_note(
            default,
            "01J5AAAAAAAAAAAAAAAAAAAAAA",
            "2026-08-14T10:00:00+02:00",
        );
        read_one(&rel_of(default), &text)
    }

    // -----------------------------------------------------------------------
    // The five
    // -----------------------------------------------------------------------

    /// The seeded set has to be usable the moment it lands: every query parses,
    /// every sort reads without a warning, every key is unique, every icon is a
    /// plausible glyph name, and no position is the "unset" zero.
    ///
    /// One test rather than five, because the failure is the same failure — a
    /// zone seeded with a rail of broken rows on first open — and a constant
    /// array is exactly the thing that goes wrong by an edit rather than by a
    /// bug.
    #[test]
    fn every_default_is_usable_the_moment_it_is_written() {
        let mut keys: Vec<&str> = Vec::new();
        for space in &DEFAULT_SESSION_SPACES {
            assert!(
                query::parse(space.query).is_ok(),
                "{} stores an unparseable query: {}",
                space.key,
                space.query
            );
            let read = sort::read(space.sort);
            assert!(
                read.warning.is_none(),
                "{} stores a sort keeper cannot read: {} ({:?})",
                space.key,
                space.sort,
                read.warning
            );
            assert!(
                space.order != sort::DEFAULT_SPACE_ORDER,
                "{} is seeded at the position that means 'unset'",
                space.key
            );
            assert!(
                space
                    .icon
                    .chars()
                    .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-'),
                "{} names {:?}, which is not a lucide key",
                space.key,
                space.icon
            );
            keys.push(space.key);
        }
        let unique = keys.len();
        keys.sort_unstable();
        keys.dedup();
        assert_eq!(keys.len(), unique, "two defaults share a key");
    }

    /// The rail renders them in reading order — what this is, what is left, what
    /// happened, what it points at, what it was told — and **not**
    /// alphabetically, which is what an unpositioned set would do.
    ///
    /// Written down because the alphabetical order (About, Log, Prompts,
    /// References, Tasks) is a plausible-looking accident: it is what the rail
    /// falls back to the moment every `order` becomes zero, and nothing else
    /// would fail if that happened.
    #[test]
    fn the_rail_reads_in_reading_order_and_not_alphabetically() {
        let files: Vec<(String, String)> = DEFAULT_SESSION_SPACES
            .iter()
            .map(|space| {
                (
                    rel_of(space),
                    render_note(
                        space,
                        "01J5AAAAAAAAAAAAAAAAAAAAAA",
                        "2026-08-14T10:00:00+02:00",
                    ),
                )
            })
            .collect();
        let borrowed: Vec<(&str, &str)> = files
            .iter()
            .map(|(rel, text)| (rel.as_str(), text.as_str()))
            .collect();
        let names: Vec<String> = read_all(&borrowed)
            .into_iter()
            .map(|space| space.name)
            .collect();
        assert_eq!(names, ["About", "Tasks", "Log", "References", "Prompts"]);

        let mut alphabetical = names.clone();
        alphabetical.sort();
        assert_ne!(names, alphabetical, "the positions are doing nothing");
    }

    /// A seeded file is an ordinary space file. If this drifts, the chip editor
    /// opens one of the five and cannot read its query.
    #[test]
    fn a_seeded_space_is_the_shape_the_editor_reads() {
        let text = render_note(
            by_key("tasks").expect("the Tasks default exists"),
            "01J5AAAAAAAAAAAAAAAAAAAAAA",
            "2026-08-14T10:00:00+02:00",
        );
        assert_eq!(
            text,
            concat!(
                "---\n",
                "id: 01J5AAAAAAAAAAAAAAAAAAAAAA\n",
                "created: 2026-08-14T10:00:00+02:00\n",
                "updated: 2026-08-14T10:00:00+02:00\n",
                "keeper:\n",
                "  space: tag:task\n",
                "  sort: order asc\n",
                "  icon: list-todo\n",
                "  order: 2\n",
                "  default: tasks\n",
                "---\n",
                "\n",
                "# Tasks\n"
            )
        );

        let read = read_one("_spaces/tasks.md", &text);
        assert_eq!(read.name, "Tasks");
        assert_eq!(read.query, "tag:task");
        assert_eq!(read.sort, "order asc");
        assert_eq!(read.icon.as_deref(), Some("list-todo"));
        assert_eq!(read.order, 2.0);
        assert_eq!(read.default_key.as_deref(), Some("tasks"));
        assert!(read.warnings.is_empty(), "{:?}", read.warnings);
    }

    /// The filename is the slug and carries no date — the one place this
    /// deliberately departs from a vault's `spaces/`.
    #[test]
    fn a_default_is_seeded_under_its_own_name() {
        let rels: Vec<String> = DEFAULT_SESSION_SPACES.iter().map(rel_of).collect();
        assert_eq!(
            rels,
            [
                "_spaces/about.md",
                "_spaces/tasks.md",
                "_spaces/log.md",
                "_spaces/references.md",
                "_spaces/prompts.md",
            ]
        );
    }

    // -----------------------------------------------------------------------
    // The directory is the ledger
    // -----------------------------------------------------------------------

    /// A zone with no `_spaces/` gets all five; one that has the directory gets
    /// nothing added to it, however empty it is.
    #[test]
    fn an_absent_directory_is_seeded_and_a_present_one_is_the_operators() {
        let keys: Vec<&str> = plan(SeedMode::FirstRun, None)
            .iter()
            .map(|space| space.key)
            .collect();
        assert_eq!(keys, ["about", "tasks", "log", "refs", "prompts"]);

        // Present and empty: the operator deleted all five, and an automatic run
        // must not put them back. This is the assertion the JSON ledger exists
        // to make in notes; here the directory makes it.
        assert!(plan(SeedMode::FirstRun, Some(&[])).is_empty());
        assert!(plan(SeedMode::FirstRun, Some(&[seeded("about")])).is_empty());
    }

    /// Restore fills holes — and only holes.
    #[test]
    fn restore_writes_the_missing_and_leaves_the_present_alone() {
        let present = [seeded("about"), seeded("log")];
        let keys: Vec<&str> = plan(SeedMode::Restore, Some(&present))
            .iter()
            .map(|space| space.key)
            .collect();
        assert_eq!(keys, ["tasks", "refs", "prompts"]);

        let all: Vec<SessionSpace> = DEFAULT_SESSION_SPACES
            .iter()
            .map(|space| seeded(space.key))
            .collect();
        assert!(
            plan(SeedMode::Restore, Some(&all)).is_empty(),
            "pressing it twice is a no-op"
        );
    }

    /// A default is claimed by its key — which survives a rename — or by its
    /// folded name, which is how an operator's own "Tasks" stands keeper's down
    /// rather than getting a second one beside it.
    #[test]
    fn a_default_is_claimed_by_its_key_or_by_its_folded_name() {
        let mut renamed = seeded("tasks");
        renamed.name = "Work items".to_owned();
        assert!(
            claimed(&[renamed.clone()]).contains("tasks"),
            "the marker is the identity (AD-79)"
        );
        assert!(plan(SeedMode::Restore, Some(&[renamed]))
            .iter()
            .all(|space| space.key != "tasks"));

        for spelling in ["Tasks", "tasks", "  TASKS  "] {
            let mine = SessionSpace {
                name: spelling.to_owned(),
                default_key: None,
                ..seeded("about")
            };
            assert!(
                claimed(&[mine]).contains("tasks"),
                "{spelling:?} is the Tasks name"
            );
        }

        // A name that folds to something else is a different space and claims
        // nothing — the fold is the filename rule, so `Task` is not `Tasks`.
        let mine = SessionSpace {
            name: "Task".to_owned(),
            default_key: None,
            ..seeded("about")
        };
        assert!(claimed(&[mine]).is_empty(), "`Task` is not `Tasks`");
    }

    // -----------------------------------------------------------------------
    // Running one over a pool
    // -----------------------------------------------------------------------

    const ABOUT: &str = "---\ntags: [about]\n---\n# What this is\n";
    const LOG_A: &str = "---\ntags: [log]\n---\n# Opened\n\nthe migration landed\n";
    const LOG_B: &str = "---\ntags: [log]\n---\n# Closed\n";
    const TASK_1: &str = "---\ntags: [task]\nstatus: todo\norder: 2\n---\n# Write it\n";
    const TASK_2: &str = "---\ntags: [task]\nstatus: done\norder: 1\n---\n# Read it\n";
    const PROMPT: &str = "---\ntags: [prompt]\n---\n# 01 Kickoff\n";
    const REF: &str = "---\ntags: [ref]\n---\n# The spec\n";
    const UNFILED: &str = "# Something someone dropped in\n";

    fn session() -> Vec<(&'static str, &'static str)> {
        vec![
            ("about.md", ABOUT),
            ("2026-08-12-0900-opened.md", LOG_A),
            ("2026-08-13-1700-closed.md", LOG_B),
            ("write-it.md", TASK_1),
            ("read-it.md", TASK_2),
            ("01-kickoff.md", PROMPT),
            ("the-spec.md", REF),
            ("README.md", UNFILED),
        ]
    }

    /// **The whole point of the module, end to end.** Each of the five picks
    /// exactly the files of its kind out of one real pool, and nothing else —
    /// including the unfiled `README.md`, which is what a half-migrated session
    /// leaves behind and which no space may quietly adopt.
    #[test]
    fn each_default_space_selects_its_own_kind_and_nothing_else() {
        let files = session();
        for (key, expected) in [
            ("about", vec!["about.md"]),
            ("tasks", vec!["read-it.md", "write-it.md"]),
            (
                "log",
                vec!["2026-08-13-1700-closed.md", "2026-08-12-0900-opened.md"],
            ),
            ("refs", vec!["the-spec.md"]),
            ("prompts", vec!["01-kickoff.md"]),
        ] {
            assert_eq!(
                run(&seeded(key), &files).unwrap_or_else(|e| panic!("{key}: {e}")),
                expected,
                "the {key} space"
            );
        }
    }

    /// Each space's `sort` is doing work, and the two that matter are asserted
    /// against the case that would look right by accident.
    ///
    /// Tasks by the file's own `order:` — `read-it` sits above `write-it`
    /// despite sorting after it by name and by mtime, so a fallback to either
    /// would fail here. Log by modified **desc** — the newest first, which is
    /// the reverse of both the name order and the pool's own slice order.
    #[test]
    fn the_sorts_are_the_files_own_and_not_the_slice_order() {
        let files = session();
        assert_eq!(
            run(&seeded("tasks"), &files).expect("tasks"),
            ["read-it.md", "write-it.md"],
            "order: 1 comes before order: 2"
        );
        assert_eq!(
            run(&seeded("log"), &files).expect("log"),
            ["2026-08-13-1700-closed.md", "2026-08-12-0900-opened.md"],
            "newest first"
        );
    }

    /// A hand-written space is an ordinary saved query over the same pool: the
    /// board's own column, a free-text term reading real bytes, and a negation.
    ///
    /// This is what makes the five *defaults* rather than *the feature*.
    #[test]
    fn a_hand_written_space_reaches_the_whole_query_language() {
        let files = session();
        let space = |query: &str| SessionSpace {
            query: query.to_owned(),
            ..seeded("about")
        };
        assert_eq!(
            run(&space("tag:task field:status=todo"), &files).expect("a column"),
            ["write-it.md"]
        );
        assert_eq!(
            run(&space("tag:task -field:status=done"), &files).expect("a negation"),
            ["write-it.md"]
        );
        assert_eq!(
            run(&space("text:migration"), &files).expect("free text"),
            ["2026-08-12-0900-opened.md"],
            "`text:` reads the real body, not the projection's empty snippet"
        );
        assert!(
            run(&space("tag:nobody"), &files)
                .expect("a query that parses")
                .is_empty(),
            "selecting nothing is an answer, not an error"
        );
    }

    /// A query that will not parse selects **nothing** and says why. Selecting
    /// everything would look like a working space over a session that had gone
    /// wrong, and send the operator looking in the wrong place.
    ///
    /// `is:task` is the mistake worth using here rather than a synthetic one:
    /// it is the query a person writes on their first day with this feature,
    /// having seen `tag:task` work — and `IS_FLAGS` is closed, so keeper refuses
    /// it and names the eleven flags it does know.
    #[test]
    fn a_broken_query_selects_nothing_and_carries_the_sentence() {
        for query in ["is:task", "(tag:log", "tag:log |"] {
            let broken = SessionSpace {
                query: query.to_owned(),
                ..seeded("about")
            };
            let error = run(&broken, &session()).expect_err("a refusal");
            assert!(!error.is_empty(), "{query} refused with no sentence");
        }
    }

    /// A query that parses but matches nothing is **not** an error, however
    /// obviously mistyped. `tag:---` normalises to no tag, which no file has —
    /// a search that finds nothing, which is the query language's own rule and
    /// must not become a refusal on the way through a space.
    #[test]
    fn a_query_that_merely_finds_nothing_is_not_a_refusal() {
        let space = SessionSpace {
            query: "tag:---".to_owned(),
            ..seeded("about")
        };
        assert!(run(&space, &session()).expect("not a refusal").is_empty());
    }

    /// An empty `keeper.space` is a space nobody has told what to show, and it
    /// shows nothing — not everything, which is what the empty *query* means.
    #[test]
    fn a_space_with_no_query_shows_nothing_and_names_itself() {
        for query in ["", "   "] {
            let unset = SessionSpace {
                query: query.to_owned(),
                name: "Untitled space".to_owned(),
                ..seeded("about")
            };
            let error = run(&unset, &session()).expect_err("a refusal");
            assert!(error.contains("Untitled space"), "{error}");
        }
    }

    /// A sort keeper cannot read never empties the pane: the space still selects
    /// what it selects, falls back to the default ordering, and the warning was
    /// already collected when the file was read.
    #[test]
    fn an_unreadable_sort_still_lists_and_the_warning_was_collected_at_read() {
        let text = concat!(
            "---\n",
            "keeper:\n",
            "  space: tag:log\n",
            "  sort: bananas\n",
            "---\n",
            "\n# Log\n"
        );
        let space = read_one("_spaces/log.md", text);
        assert_eq!(space.warnings.len(), 1, "{:?}", space.warnings);
        assert_eq!(
            run(&space, &session()).expect("it still lists"),
            ["2026-08-13-1700-closed.md", "2026-08-12-0900-opened.md"],
            "DEFAULT_SORT is modified desc"
        );
    }

    /// A file that is not a space file is not an error. It names no query, so it
    /// shows nothing and says so — the same arm an emptied key takes, because
    /// they are the same fact.
    #[test]
    fn a_stray_markdown_file_in_the_directory_is_not_a_space() {
        let space = read_one("_spaces/notes-to-self.md", "# Notes to self\n\nremember\n");
        assert_eq!(space.name, "Notes to self");
        assert!(space.query.is_empty());
        assert!(space.default_key.is_none());
        assert!(run(&space, &session()).is_err());
    }

    /// A `keeper.default` this build does not know names no default, so it never
    /// silently stands a real one down.
    #[test]
    fn an_unrecognised_default_marker_claims_nothing() {
        let text = concat!(
            "---\n",
            "keeper:\n",
            "  space: tag:someday\n",
            "  default: someday\n",
            "---\n",
            "\n# Someday\n"
        );
        let space = read_one("_spaces/someday.md", text);
        assert!(space.default_key.is_none());
        assert!(claimed(&[space]).is_empty());
    }

    /// The order is total: two files that tie on the sort key are separated by
    /// their path, so the list does not reshuffle between launches.
    #[test]
    fn two_files_that_tie_on_the_sort_are_still_ordered() {
        let same = "---\ntags: [task]\norder: 1\ntitle: Same\n---\n# Same\n";
        let files = vec![("b.md", same), ("a.md", same)];
        assert_eq!(
            run(&seeded("tasks"), &files).expect("tasks"),
            ["a.md", "b.md"]
        );
    }
}
