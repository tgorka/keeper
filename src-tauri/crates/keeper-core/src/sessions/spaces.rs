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
use crate::notes::tags;
use crate::sessions::plan::{Plan, PlanStep};
use crate::sessions::pool::{as_index_entry, PoolEntry};
use crate::sessions::shape::{kind_dir, KindHasNoHome, KindTag, Shape};

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

/// The six, in the order a session is read in.
///
/// **The order is the reading order, and it is the operator's own**: what this
/// session is, what is left to do, what happened, what it points at, what it was
/// told, and last what it has not said anything about. It is not alphabetical,
/// and it is not the order the deleted directories happened to sort in — a
/// space's position is a real field now
/// ([`crate::notes::sort::rail_order`]), so it can carry a meaning.
///
/// **They start at 1, not 0.** [`crate::notes::sort::DEFAULT_SPACE_ORDER`] is
/// `0.0` and means *unset*: a space whose file says nothing about its position
/// sorts as zero. Seeding About at zero would make it indistinguishable from
/// every unpositioned space the operator later writes, and the rail would sort
/// the two by name — putting a hand-made "Archive" above the About that is
/// supposed to be read first. One is the smallest number that is a statement.
///
/// **The five that show a kind ask one `tag:` term and nothing else**, which is
/// the flat contract restated as data: a file's kind is what it says it is
/// (AD-120), so the space that shows a kind asks exactly that and infers nothing
/// from a name, a folder or a position. The board's `field:status=` columns are
/// composed on top of the Tasks query rather than baked into it — four spaces
/// would be four places to edit when the tag changes.
///
/// **The sixth asks for the residue**, and it is the same grammar read the other
/// way round: `-tag:about -tag:log -tag:prompt -tag:ref -tag:task` is every kind
/// in [`crate::sessions::shape::KINDS`], negated. A file declaring none of them
/// used to reach the operator as a badge list the detail drew from
/// [`crate::sessions::pool::Pool::unfiled`] — no count, no fold, no row verbs —
/// and it is one ordinary space now, folding and counting like its five
/// siblings. Deriving the string from `KINDS` was the alternative and a `const`
/// cannot format one, so `the_untagged_query_negates_every_kind` zips the two
/// instead: a sixth kind then fails a test rather than quietly leaving its files
/// in a space that claims to hold everything unclaimed.
///
/// **It is a default like the other five, not a mechanism beside them**
/// (AD-121): one file in `_spaces/`, seeded by the same [`plan`], deleted by the
/// same verb, and — because the directory is the ledger — deleted for good.
/// Synthesising it on every read was the rejected alternative, and it would have
/// been the one row on the rail an operator could not rename, reposition, fold
/// or throw away.
pub const DEFAULT_SESSION_SPACES: [DefaultSessionSpace; 6] = [
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
    DefaultSessionSpace {
        key: "untagged",
        name: "Untagged",
        // Every kind in `KINDS`, negated — the residue, and nothing else.
        query: "-tag:about -tag:log -tag:prompt -tag:ref -tag:task",
        // A to Z by the title a person reads — [`sort::SortKey::Name`] is
        // `title_order`, not the filename — because the residue has no order of
        // its own: these files are whatever was dropped in, and a title is the
        // only thing about them keeper can put in a stable row. `modified desc`
        // was the alternative and it is worse here: it would reshuffle the
        // section every time an agent touched one of them, which is exactly the
        // list a person is trying to work through.
        sort: "name asc",
        // The notes rail's Inbox glyph, for the notes rail's reason: `inbox` is
        // already what this product draws over "the honest home of the unfiled"
        // ([`crate::notes::default_spaces`]), and a second glyph for the same
        // idea would make the two rails disagree about what a file nothing has
        // claimed looks like.
        icon: "inbox",
        // Last, which is the whole of the operator's instruction about it: the
        // residue is read after everything that has said what it is.
        order: 6.0,
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
    /// `keeper.folded`, when the file says how the section opens — `None` when
    /// it says nothing (Story 51.3, FR-289).
    ///
    /// Three-valued on purpose, and it is the whole reason this key can coexist
    /// with `sessions.spaces_folded`: the surface layers the person's own hand
    /// over the file's answer over the setting, and a `bool` here would make
    /// "this space says nothing" indistinguishable from "this space says
    /// unfolded" — after which flipping the user-global setting would move
    /// nothing.
    pub folded: Option<bool>,
    /// `keeper.rows`, when the file caps how many rows the section RENDERS —
    /// never how many files the query SELECTS (Story 51.3, FR-290).
    ///
    /// Not [`crate::notes::vm::NoteSpaceVm`]'s `limit`, which narrows the
    /// selection itself. A session holds tens of files, so the cost a selection
    /// cap exists to avoid is not there; what a person wants capped is the
    /// height of a card in a rail, and a section that had *selected* three of
    /// twelve files could not honestly say how many it was not showing.
    pub rows: Option<u32>,
    /// `keeper.create_dir`: the directory this space's creates go INTO,
    /// session-relative — empty when it names none, which is every space until
    /// somebody types one (Story 52.5, FR-309).
    ///
    /// **A destination for writes, and never a source for reads.** AD-120 says a
    /// file's kind is the tag it carries, and this key does not soften that by a
    /// millimetre: [`crate::sessions::pool::read_one`] still derives the kind
    /// from tags alone, so a file sitting in `logs/` tagged `ref` is a reference,
    /// and this space lists what it lists because its QUERY matched a tag. What
    /// the key changes is one thing — where
    /// [`crate::sessions::shape::kind_dir`] puts the next file.
    ///
    /// Stored as the file spells it, trimmed: a directory whose name ends in a
    /// space is a trap, and the path itself is validated where it is used, by
    /// the one guard [`crate::sessions::files::check_dir`], rather than by a
    /// second rule here that would have to agree with it forever.
    pub create_dir: String,
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
        folded: None,
        rows: None,
        create_dir: String::new(),
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
                // Both flattened for `order`'s reason, one arm up — the key
                // decides the reader, so a value keeper cannot use reaches the
                // warning path instead of falling silently through the match and
                // leaving the operator with a key that does nothing.
                ("folded", value) => match read_folded(&value.index_string()) {
                    Some(Ok(folded)) => space.folded = Some(folded),
                    Some(Err(warning)) => space.warnings.push(warning),
                    None => {}
                },
                ("rows", value) => match read_rows(&value.index_string()) {
                    Some(Ok(rows)) => space.rows = Some(rows),
                    Some(Err(warning)) => space.warnings.push(warning),
                    None => {}
                },
                // Trimmed like `icon` and not stored verbatim like `query`,
                // because this one is a PATH: a trailing blank or separator is
                // not part of a directory's name, and `logs/` is the same
                // request as `logs` (`files::dir_rel`'s own rule). A leading
                // `/` is left exactly as written — it makes the path absolute,
                // which is a refusal and not a spelling to repair.
                ("create_dir", FieldValue::Str(dir)) => {
                    space.create_dir = dir.trim().trim_end_matches('/').to_owned();
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

/// How a space opens when nobody has folded it by hand, the sentence saying
/// keeper could not tell, or nothing at all (FR-289).
///
/// **Three answers, and the return shape is what keeps them three.** A
/// `(Option<bool>, Option<String>)` pair could hold a value *and* a complaint
/// about that value, a state every call site would then have to decide about;
/// `Option<Result<_, _>>` cannot spell it. `None` is a key that said nothing —
/// a `folded:` an editor cleared — which is not a fault and behaves exactly as
/// an absent key, [`sort::read_order`]'s rule for the same shape of hole.
///
/// **`true` and `false` only**, in the three spellings
/// [`crate::notes::frontmatter`] itself reads. YAML's `yes` is a string to this
/// crate and to plenty of other readers, and a space file is a file people open
/// in Obsidian: honouring `yes` here would make the fold work in keeper and
/// nowhere else, which is a worse answer than a sentence saying the value was
/// not understood.
fn read_folded(raw: &str) -> Option<Result<bool, String>> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return None;
    }
    match trimmed {
        "true" | "True" | "TRUE" => Some(Ok(true)),
        "false" | "False" | "FALSE" => Some(Ok(false)),
        _ => Some(Err(format!(
            "keeper can't read \"{}\" as a yes or no, so this space opens the way the setting says.",
            sort::clip(trimmed)
        ))),
    }
}

/// How many rows a space renders, the sentence saying keeper could not tell, or
/// nothing at all (FR-290).
///
/// [`read_folded`]'s shape, for [`read_folded`]'s reasons.
///
/// **Positive integers only.** Zero is not a small cap: it is a section with no
/// rows under a header that still says twelve, which is a fold nobody asked for
/// and no control can open. A negative or fractional value is not a cap at all.
/// All three take the road an unreadable `sort` takes — a warning, and the
/// section shows everything it selected — because a cap is presentation, and
/// refusing to render a space over one bad line of its own frontmatter would
/// hide the files it found.
fn read_rows(raw: &str) -> Option<Result<u32, String>> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return None;
    }
    match trimmed.parse::<u32>() {
        Ok(rows) if rows > 0 => Some(Ok(rows)),
        _ => Some(Err(format!(
            "keeper can't read the row limit \"{}\", so this space shows every file it selects.",
            sort::clip(trimmed)
        ))),
    }
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
        // Never seeded: the zone gets the six it was designed around.
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
///
/// **No `folded` and no `rows`** (Story 51.3). The five are the reading order of
/// a session — what it is, what is left, what happened, what it points at, what
/// it was told — and every one of them is meant to be read on arrival, so
/// seeding a fold would be keeper shutting a section before anybody had looked
/// in it. Seeding a cap was the other candidate, for Log, and it is worse: the
/// number would be keeper's guess at how long a sitting is, stamped into the
/// operator's file where they would have to find it to undo it. The person who
/// wants everything shut has `sessions.spaces_folded`, which is what a
/// user-global default is for; the person who wants Log capped writes one key in
/// one file and it stays theirs.
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

/// The kind a space can create into itself, or `None` (FR-273).
///
/// Derived here rather than in the surface, for the reason the notes rail
/// already states: `keeper.space` is a grammar, and a second reader of it in
/// TypeScript would be a second grammar from the day it was written (AD-20,
/// AD-58). What crosses the boundary is a kind or nothing.
///
/// **Exactly one `tag:` term, or nothing.** Four of the six defaults are
/// single `tag:` queries ([`DEFAULT_SESSION_SPACES`]), and for those "what
/// would a file made here have to be?" has one answer. A second term does not
/// narrow that answer, it breaks it: a create in `tag:log date:today` would
/// write a file the space stops listing at midnight, which is worse than no
/// create at all. [`crate::notes::seed::inherit`] takes the other choice — it
/// seeds the terms it can and lets [`crate::notes::seed::verdict`] say the
/// rest in a sentence — because a note create carries tags, flags and a
/// destination and has somewhere to put a partial answer. A session file verb
/// takes a kind and a title, and there is no remainder it could hold.
///
/// **`about` is refused**, matching `sessions_file_new_kind`: a session has one
/// record, and a second would give [`crate::sessions::shape::shape`] two
/// answers.
///
/// **The parse gate is the same one [`select`] runs**, and it is not
/// decoration. A query that does not parse selects nothing and the space
/// already renders that sentence; the create verb has to be absent in exactly
/// those spaces, and one parser deciding both is what keeps it so — rather
/// than two rules that happen to agree today.
///
/// `None`, therefore, for a tag that is not a kind (`tag:project/alpha`, and
/// `tag:task/*` on the same rule, since [`KindTag::of`] matches a kind exactly),
/// a negated term, a query with structure or two terms, a query that does not
/// parse, and a space that has not been told what to show.
///
/// **`None` is not the same as nothing to say.** [`create_refused`] is the
/// sibling that words the refusal, and it reads the query through the same
/// [`read_create`] this does: About renders no button and a sentence, a space
/// asking for two things renders no button and a different sentence, the
/// Untagged space asks for what is left over and renders a third, and a space
/// asking for an ordinary tag renders neither. Before Story 51.7 all of them
/// rendered nothing at all.
#[must_use]
pub fn creatable_kind(query: &str) -> Option<KindTag> {
    read_create(query).kind
}

/// What one query says about writing into the space it defines, from **one**
/// read of the grammar.
///
/// Private, and one struct rather than four public predicates each parsing
/// again: the four facts are four quarters of one question — what would a file
/// made here be, is the record what this space is about, does the query ask for
/// more than one thing, and does it ask for anything positive at all — and
/// separate readers would parse `keeper.space` four times per space per read to
/// answer them.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct CreateVerdict {
    /// The kind a create here would write — [`creatable_kind`]'s whole answer.
    kind: Option<KindTag>,
    /// Whether the query names the session's record among its terms.
    ///
    /// Every term, not just a lone one: the live About space asks
    /// `tag:about tag:recordings`, and it is still the space a person opens to
    /// read this session's record.
    record: bool,
    /// Whether the query asks for more than one thing.
    many: bool,
    /// Whether every term is a negation, so no term of it could name a kind.
    ///
    /// Not the same fact as a `kind` of `None`, which `tag:project/alpha`
    /// produces too: this one is why the `Untagged` default has no create to
    /// offer, and `tag:project/alpha` is a space that never offered one to miss.
    /// [`create_refused`] tells the two apart and only words the first.
    negated: bool,
}

/// Read one query once, for all four.
fn read_create(query: &str) -> CreateVerdict {
    // Nothing offered and nothing to explain: the two states a space already
    // reports in its own words. A query keeper cannot read carries
    // `SessionSpace::error`, and one that says nothing carries `Selection::error`
    // — a second sentence beside either would be keeper answering a question the
    // person can already see the answer to.
    let silent = CreateVerdict {
        kind: None,
        record: false,
        many: false,
        negated: false,
    };
    if query::parse(query).is_err() {
        return silent;
    }
    let Some(terms) = query::conjunction(query) else {
        return silent;
    };
    // Normalised by the one definition of a tag, so `tag:#TASK` names the kind
    // `tag:task` does — the same fold the index applied to the files this space
    // is selecting, which is what makes the two agree.
    let named = |term: &query::Term| -> Option<KindTag> {
        if term.negated || term.key.as_deref() != Some("tag") {
            return None;
        }
        KindTag::of(&[tags::normalise(&term.value)?])
    };
    // Asked of every term, and `all` over an empty slice is why the emptiness
    // is checked first: a query with no terms at all is the space that has not
    // been told what to show, which already says so in its own words.
    let negated = terms.iter().all(|term| term.negated);
    match terms.as_slice() {
        [] => silent,
        [term] => {
            let kind = named(term);
            CreateVerdict {
                // The one kind the file verb itself refuses, so the create is
                // absent here rather than present and always failing.
                kind: kind.filter(|kind| *kind != KindTag::About),
                record: kind == Some(KindTag::About),
                many: false,
                negated,
            }
        }
        terms => CreateVerdict {
            kind: None,
            record: terms.iter().any(|term| named(term) == Some(KindTag::About)),
            many: true,
            negated,
        },
    }
}

/// Why a space offers no create, and which verb applies instead (FR-298,
/// FR-299).
///
/// [`creatable_kind`] answers *what* a create here would write; this answers
/// *why there is none*, which until Story 51.7 nothing did: the projection only
/// asked about homes for a kind the query had already produced, so the About
/// space — refused three times over — rendered neither a button nor a reason.
/// A section that offers nothing and says nothing is the report that opened the
/// story.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CreateRefused {
    /// The sentence to print where the button would have been, or `None` when
    /// there is a create to offer and when the refusal is nothing a person needs
    /// told.
    pub why: Option<Refusal>,
    /// Whether the verb that applies instead is opening the session's record.
    ///
    /// A flag and not a path, unlike the sentence beside it: this decides
    /// whether a VERB applies, and the file it opens is one fixed name at a
    /// known place that the header already names from the shape. Composing a
    /// second path for it here would be a second answer to "where is the
    /// record".
    pub record: bool,
}

/// A refusal a space's create can meet, worded exactly once.
///
/// Sentences rather than codes, [`KindHasNoHome`]'s own rule, and *that* enum is
/// wrapped rather than restated: a session's contract already owns the wording
/// of "a session has one about record", and this type exists to add the
/// refusals that are the QUERY's rather than the contract's.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum Refusal {
    /// This session's contract keeps nowhere to put that kind — including the
    /// record, which it keeps in exactly one place already.
    #[error(transparent)]
    NoHome(#[from] KindHasNoHome),
    /// The query asks for more than one thing, so there is no single kind a
    /// create could write.
    ///
    /// **"Files below"**, and the direction is load-bearing: Story 52.4 put the
    /// SPACES section above FILES on the detail, so a sentence rendered in a
    /// space that sent a person upwards would send them past the record's header
    /// and out of the surface.
    #[error(
        "this space asks for more than one thing, so there is no single kind a file made here \
         could be: every term has to hold for a file to appear, and a create writes one kind \
         with one tag. Narrow the query to a single `tag:` term to write into this space, or make \
         the file from Files below and tag it so this space picks it up."
    )]
    ManyTerms,
    /// Every term is a negation, so the query names no kind at all — which is
    /// the `Untagged` default's own state and the reason it has a control that
    /// refuses rather than no control (Story 52.4).
    ///
    /// The instruction is the opposite of every other refusal's: a file made
    /// somewhere else appears HERE by default, and leaves when it is told what it
    /// is. That is the whole of what this space is for, so the sentence says it
    /// rather than sending anybody to narrow a query.
    #[error(
        "this space asks for what is left over — every one of its terms is a negation — so it \
         names no kind, and a create writes one kind with one tag. There is nothing a file made \
         here could be: make the file from Files below, and it appears here until you give it a \
         kind tag."
    )]
    Negated,
}

/// Why this space offers no create under this session's contract, and whether
/// the record is the thing it is about.
///
/// **The refusals are reported in the order they are made**, which is the order
/// [`read_create`] meets them and the order a person met them: a two-term query
/// is refused before anything looks at what its terms name. So the live About
/// space — `tag:about tag:recordings` — is explained by [`Refusal::ManyTerms`]
/// and not by the record, which is the honest answer for a query that would
/// still have no single kind if the record were creatable tomorrow.
///
/// **[`Refusal::Negated`] is the one exception to that order**, and it is an
/// exception because arity is not the obstacle it names. `ManyTerms` ends by
/// telling the person to narrow the query to a single `tag:` term; narrowing
/// `-tag:about -tag:log` leaves `-tag:about`, which still names no kind, so the
/// advice would send them round a loop. A query made entirely of negations is
/// refused for what its terms are, however many of them there are.
///
/// `shape` is taken rather than read, because a space definition is zone-level
/// and a contract is one session's: the caller has the session, and the
/// alternative — projecting a sentence per shape onto the zone's definition —
/// is a payload that is wrong for every session but one.
#[must_use]
pub fn create_refused(query: &str, shape: Shape) -> CreateRefused {
    let verdict = read_create(query);
    let why = if let Some(kind) = verdict.kind {
        // The query offers a create, so the only thing left that can refuse it
        // is this session's own contract. No destination override is passed: a
        // space's `create_dir` moves a create the contract already ALLOWS, and
        // cannot make a refused kind creatable, so the refusal is the contract's
        // alone (Story 52.5).
        kind_dir(shape, kind, "").err().map(Refusal::NoHome)
    } else if verdict.negated {
        Some(Refusal::Negated)
    } else if verdict.many {
        Some(Refusal::ManyTerms)
    } else if verdict.record {
        // Always `Err(OnlyOne)` today, and asked rather than asserted: a
        // contract that ever gave the record a home would make this space
        // creatable, and going quiet is then the same silence as before this
        // function existed rather than a sentence that has stopped being true.
        kind_dir(shape, KindTag::About, "")
            .err()
            .map(Refusal::NoHome)
    } else {
        None
    };
    CreateRefused {
        why,
        record: verdict.record,
    }
}

/// What the editor asked to be written to one space.
///
/// Everything the form can change and nothing it cannot. There is no `default`
/// field, deliberately: `keeper.default` is keeper's own marker, the editor has
/// no control for it, and a request that could carry it would be a request that
/// could turn a hand-written space into a seeded one — after which "Restore
/// defaults" would stop offering the real thing.
#[derive(Debug, Clone, PartialEq)]
pub struct SpaceEdit {
    pub name: String,
    /// The query, exactly as the chips composed it.
    pub query: String,
    /// The canonical `<key> <dir>`. Writing this is what makes saving a space
    /// whose stored sort keeper could not read a *repair*: the form showed the
    /// fallback and said why, so Save is the operator agreeing to it.
    pub sort: String,
    pub icon: Option<String>,
    pub order: f64,
    /// How the section opens when nobody has folded it by hand, or `None` to
    /// write no key — the editor's checkbox in its third, unset state.
    pub folded: Option<bool>,
    /// How many rows the section renders, or `None` to write no key — the
    /// editor's cap box, empty.
    pub rows: Option<u32>,
    /// The directory this space's creates go into, or empty for none — the
    /// editor's destination box, left blank (Story 52.5, FR-309).
    ///
    /// Sent on every save for the reason [`SessionSpace::folded`] is: this
    /// function's map REPLACES the file's, so a form that omitted the field
    /// would delete the operator's destination the next time they renamed the
    /// space.
    pub create_dir: String,
}

/// The `keeper:` map both renderers write, minus `default`.
///
/// One builder rather than two identical lists, and that is this story's whole
/// defence: [`render_edit`] REPLACES the map, so a key that reaches
/// [`render_new`] and not [`render_edit`] is a key the first Save destroys. Two
/// hand-kept lists agreed for five keys and would have stopped agreeing at the
/// sixth — which is precisely how `folded` would come to survive being created
/// and not survive being edited.
///
/// **An absent answer writes no key.** A space nobody gave an icon, a position,
/// a fold or a cap keeps the frontmatter it had rather than growing empty keys
/// to explain; and `order: 0` is the sharpest case, since zero *is*
/// unpositioned and stamping it into every space would claim each had been
/// placed. `notes_ipc::notes_space_save`'s rule, for its reasons.
///
/// The order is the one the reader has always written: what the space shows,
/// how it shows it, then how it opens. `folded` and `rows` join at the end of
/// the presentation keys rather than beside `space`, so an operator's diff of an
/// old file against a re-saved one is two added lines and not a reshuffle.
fn keeper_pairs(edit: &SpaceEdit) -> Vec<(String, FieldValue)> {
    let mut pairs = vec![
        ("space".to_owned(), FieldValue::Str(edit.query.clone())),
        ("sort".to_owned(), FieldValue::Str(edit.sort.clone())),
    ];
    if let Some(icon) = edit
        .icon
        .as_deref()
        .map(str::trim)
        .filter(|i| !i.is_empty())
    {
        pairs.push(("icon".to_owned(), FieldValue::Str(icon.to_owned())));
    }
    if edit.order != sort::DEFAULT_SPACE_ORDER {
        pairs.push(("order".to_owned(), FieldValue::Num(edit.order)));
    }
    if let Some(folded) = edit.folded {
        pairs.push(("folded".to_owned(), FieldValue::Bool(folded)));
    }
    if let Some(rows) = edit.rows {
        pairs.push(("rows".to_owned(), FieldValue::Num(f64::from(rows))));
    }

    // Last of the keys, after the presentation ones, because it is the only one
    // that is about WRITING rather than about what the space shows or how: an
    // operator diffing an old definition against a re-saved one sees one line
    // added at the end and not a reshuffle. Empty writes no key at all —
    // `icon`'s rule — so a space nobody gave a destination keeps the
    // frontmatter it had.
    if !edit.create_dir.is_empty() {
        pairs.push((
            "create_dir".to_owned(),
            FieldValue::Str(edit.create_dir.clone()),
        ));
    }
    pairs
}

/// Rewrite an existing space's definition, preserving every other byte
/// (FR-121).
///
/// `source` is the file as it stands. Only the `keeper:` map and — when the name
/// actually changed — `title` and keeper's own heading are touched; prose,
/// unknown keys, and key order elsewhere all survive, because a space file is
/// something a person opens in Obsidian and writes in.
///
/// **The filename never moves.** A note space renames its file, because a
/// vault's filenames are derived from titles and the index re-resolves the id.
/// Here the path *is* the id ([`crate::sessions::vm::SessionSpaceVm::id`]): there
/// is no index to re-resolve it, so renaming the file would silently break every
/// reference the surface is holding — and the operator gains nothing, since
/// `_spaces/` is five files they navigate by their titles anyway.
///
/// `keeper.default` is carried through from `source` rather than from the edit,
/// which is what stops editing the seeded Tasks space from turning it into an
/// ordinary one and making Restore offer a second copy.
#[must_use]
pub fn render_edit(rel: &str, source: &str, edit: &SpaceEdit) -> String {
    // Read rather than trust the request: `default_key` and the *old* name both
    // come from the file, and the old name is what decides whether the body's
    // heading is keeper's to retitle or the operator's to leave alone. `rel` is
    // passed so the name falls back to the real filename stem — the same name
    // the rail was showing — when the file carries neither `title` nor heading.
    let existing = read_one(rel, source);
    let mut pairs = keeper_pairs(edit);
    if let Some(key) = existing.default_key {
        pairs.push(("default".to_owned(), FieldValue::Str(key)));
    }

    let mut updated = Frontmatter::set_in(source, "keeper", FieldValue::Map(pairs));
    if edit.name != existing.name {
        // `title` rather than the heading alone: `note_title` reads frontmatter
        // first, and a space's body belongs to whoever last edited it, so the
        // key is the only place a name is guaranteed to stick.
        updated = Frontmatter::set_in(&updated, "title", FieldValue::Str(edit.name.clone()));
        let (_, body_at) = Frontmatter::parse(&updated);
        if let Some(body) = naming::retitle_heading(&updated[body_at..], &existing.name, &edit.name)
        {
            updated = format!("{}{body}", &updated[..body_at]);
        }
    }
    updated
}

/// The file a brand-new hand-made space is written as.
///
/// [`render_note`]'s twin for a space that is not one of the defaults, and it
/// differs in exactly one key: no `default`, because nothing here is a default.
/// Same `keeper:` map otherwise, so the reader, the editor and the seeder all
/// see one shape.
#[must_use]
pub fn render_new(edit: &SpaceEdit, id: &str, now: &str) -> String {
    let keeper = keeper_pairs(edit);
    let front = Frontmatter::serialise_new(&[
        ("id".to_owned(), FieldValue::Str(id.to_owned())),
        ("created".to_owned(), FieldValue::Str(now.to_owned())),
        ("updated".to_owned(), FieldValue::Str(now.to_owned())),
        ("title".to_owned(), FieldValue::Str(edit.name.clone())),
        ("keeper".to_owned(), FieldValue::Map(keeper)),
    ]);
    format!("{front}\n# {}\n", edit.name)
}

/// The zone-relative path a new space is written to, avoiding `taken`.
///
/// The title's slug, then `-2`, `-3` … — [`naming::slug`]'s fold decides the
/// collision, which is the same fold [`claimed`] uses, so a space the operator
/// calls "Tasks" lands beside the seeded one rather than silently overwriting it.
/// That function already guarantees a usable stem for a name that folds to
/// nothing and for a name Windows reserves, which is why there is no second
/// fallback here: a note and a space that cannot be named would be named
/// differently, and one of the two spellings would be the one nobody tested.
///
/// [`rel_of`]'s undated form, for [`rel_of`]'s reason — five files named after
/// what they do, in a directory the operator opens by hand.
#[must_use]
pub fn rel_for_new(name: &str, taken: &BTreeSet<String>) -> String {
    let stem = naming::slug(name);
    let mut candidate = format!("{SPACES_DIR}/{stem}.md");
    let mut n = 2;
    while taken.contains(&candidate) {
        candidate = format!("{SPACES_DIR}/{stem}-{n}.md");
        n += 1;
    }
    candidate
}

/// Whether a path is one this module is allowed to write or delete.
///
/// The containment rule, stated once and asked by the shell before every write:
/// directly inside `_spaces/`, a `.md`, no traversal, no nesting. The executor
/// has its own zone-relative check, but that one only proves a path cannot
/// *escape the zone* — it would happily let `sessions_space_delete` be handed
/// `active/2026-08-14-keeper/about.md` and remove somebody's session record.
#[must_use]
pub fn is_space_path(rel: &str) -> bool {
    let Some(stem) = rel.strip_prefix(&format!("{SPACES_DIR}/")) else {
        return false;
    };
    !stem.is_empty()
        && stem.ends_with(".md")
        && !stem.contains('/')
        && stem != ".md"
        && !stem.starts_with('.')
}

/// The plan that writes one space — new when `source` is `None`, a rewrite
/// otherwise.
///
/// A plan rather than a bare string, so a space write goes through the same
/// journaled executor every other write does (AD-111) and the shell keeps
/// executing rather than deciding (AD-108). `MkDir` leads, because a `Restore`
/// into a zone that never had `_spaces/` must create it, and the step is
/// idempotent when it did.
///
/// Not a `GuardedWrite`: the optimistic guard exists for `README.md`, which an
/// agent may be appending to while the operator edits it. `_spaces/` is edited
/// by one human in one editor, and a guard there would turn "your file changed
/// on disk" into a refusal with nothing useful to do about it.
#[must_use]
pub fn compile_save(
    rel: &str,
    source: Option<&str>,
    edit: &SpaceEdit,
    id: &str,
    now: &str,
) -> Plan {
    let content = match source {
        Some(text) => render_edit(rel, text, edit),
        None => render_new(edit, id, now),
    };
    Plan {
        verb: "space-save".to_owned(),
        session: rel.to_owned(),
        steps: vec![
            PlanStep::MkDir {
                path: SPACES_DIR.to_owned(),
            },
            PlanStep::WriteFile {
                path: rel.to_owned(),
                content,
            },
        ],
    }
}

/// The plan that removes one space: a trash move, recoverable.
///
/// The whole plan is the irreversible step, which AD-111 puts last and here
/// makes the only one.
#[must_use]
pub fn compile_delete(rel: &str, trash_key: &str) -> Plan {
    Plan {
        verb: "space-delete".to_owned(),
        session: rel.to_owned(),
        steps: vec![PlanStep::TrashFile {
            path: rel.to_owned(),
            trash_key: trash_key.to_owned(),
        }],
    }
}

/// The plan that seeds defaults into `_spaces/` (FR-261).
///
/// `ids` and `now` come from the shell for the usual reason — the domain has
/// neither a clock nor an id generator — and are zipped positionally against
/// [`plan`]'s output, so a caller that supplies too few ids seeds fewer spaces
/// rather than reusing one id for two files.
#[must_use]
pub fn compile_seed(defaults: &[&'static DefaultSessionSpace], ids: &[String], now: &str) -> Plan {
    let mut steps = vec![PlanStep::MkDir {
        path: SPACES_DIR.to_owned(),
    }];
    for (space, id) in defaults.iter().zip(ids) {
        steps.push(PlanStep::WriteFile {
            path: rel_of(space),
            content: render_note(space, id, now),
        });
    }
    Plan {
        verb: "spaces-seed".to_owned(),
        session: SPACES_DIR.to_owned(),
        steps,
    }
}

// ---------------------------------------------------------------------------
// The spaces a TEMPLATE offers a zone (FR-291)
// ---------------------------------------------------------------------------
//
// AD-121 refused **per-session** spaces — "per-session means editing one query
// N times and reintroduces a folder into a shape whose point is that there are
// none" — and that refusal stands: nothing below ever writes inside
// `active/<session>/`. What a template's `_spaces/` does is offer the ZONE the
// queries a session made from that template wants to be read through, which is
// the same directory [`compile_seed`] writes into and the same one file per
// query the operator already edits by hand.
//
// The seed is **additive and never destructive**, and that is the whole safety
// argument. A zone's `_spaces/` is the operator's; a create is a routine verb
// they press many times a week, and one that could rewrite a query they had
// tuned would be a verb they learned to fear. So a template can only fill a
// hole, never change what is standing in one.

/// One space a template offers the zone, as two zone-relative paths.
///
/// Paths rather than bytes, so the seed compiles to the same
/// [`PlanStep::CopyFile`] the rest of the create is made of: the template file
/// IS the definition, verbatim, and re-rendering it through [`render_new`]
/// would drop the prose, the unknown keys and the key order that
/// [`render_edit`] goes out of its way to preserve one directory over.
///
/// **No fresh `id` is stamped into the copy**, which is the one thing this
/// deliberately does not borrow from [`compile_seed`]. Nothing in the sessions
/// domain reads a space's `id` — [`read_one`] does not even look at it, and a
/// space's identity is its path ([`crate::sessions::vm::SessionSpaceVm::id`])
/// — while stamping identity into a file keeper did not author is the rule
/// [`crate::sessions::template::compile_file_new`] states from the other side.
/// The rejected alternative was `Frontmatter::set_in(text, "id", …)` per seed:
/// it costs the plan the whole file's bytes, needs a ULID per candidate from
/// the shell, and buys an identity no reader asks for.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TemplateSpace {
    /// Zone-relative source inside the template, e.g.
    /// `_template/house/_spaces/tasks.md`.
    pub from: String,
    /// Zone-relative destination, `_spaces/<name>.md`.
    pub to: String,
}

/// What a template's `_spaces/` would do to one zone.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct TemplateSpacesPlan {
    /// What the zone gains, in candidate order.
    pub seeds: Vec<TemplateSpace>,
    /// Why keeper could not use an entry, one finished sentence each.
    ///
    /// Only the entries that need **explaining**. A candidate the zone already
    /// has is the ordinary case and says nothing — a create from a template
    /// shipping all five spaces into a zone that has all five would otherwise
    /// log five sentences every time, which is how a log stops being read.
    /// [`plan`] says nothing about a directory it declines to touch either, and
    /// for the same reason.
    pub skipped: Vec<String>,
}

/// Which of a template's `_spaces/` entries this zone gains on a create
/// (FR-291).
///
/// `pattern_root` is the template's zone-relative directory and `candidates` is
/// `(template-relative path, bytes)` for each entry
/// [`crate::sessions::pattern::PatternOutcome::seeds`] found — the shell reads
/// them, this decides (AD-108). The join happens here so no caller composes a
/// zone path (AD-65).
///
/// **A candidate must be a space definition keeper can actually run**, and the
/// gate is the one [`select`] already applies to a space on the rail: directly
/// inside `_spaces/`, a `.md`, naming a query, and a query that parses. Those
/// are not style rules. A space with no query "doesn't say what to show yet,
/// so it shows nothing", and a query that does not parse selects nothing —
/// seeding either would hand the zone a permanent empty row that keeper itself
/// wrote. The create still succeeds, because a typo in a template's space file
/// is not a reason to refuse somebody a session; the sentence says which file
/// and why.
///
/// **A hole is filled; nothing standing is touched.** A candidate is declined
/// when the zone already holds that path, or a space folding to that name, or
/// the same `keeper.default` key — [`claimed`]'s two-way rule widened by the
/// path, because a template's `_spaces/tasks.md` landing on a zone's
/// `_spaces/tasks.md` would overwrite it however differently the two are
/// titled. The candidates are folded against each other on the same rule, so a
/// template holding two files that both call themselves Tasks seeds the first
/// and explains the second.
#[must_use]
pub fn plan_template_spaces(
    pattern_root: &str,
    candidates: &[(&str, &str)],
    existing: &[SessionSpace],
) -> TemplateSpacesPlan {
    let mut taken_paths: BTreeSet<String> = existing.iter().map(|s| s.rel.clone()).collect();
    let mut taken_names: BTreeSet<String> =
        existing.iter().map(|s| naming::slug(&s.name)).collect();
    let mut taken_keys: BTreeSet<String> = existing
        .iter()
        .filter_map(|s| s.default_key.clone())
        .collect();

    let mut out = TemplateSpacesPlan::default();
    for (rel, text) in candidates {
        if !is_space_path(rel) {
            out.skipped.push(format!(
                "{rel} was not seeded: a template's spaces are `.md` files directly inside `{SPACES_DIR}/`."
            ));
            continue;
        }
        let space = read_one(rel, text);
        if space.query.trim().is_empty() {
            out.skipped.push(format!(
                "{rel} doesn't say what to show, so it was not seeded."
            ));
            continue;
        }
        if let Err(error) = query::parse(&space.query) {
            out.skipped
                .push(format!("{rel} was not seeded: {}", error.message));
            continue;
        }
        let name = naming::slug(&space.name);
        let claims_key = space
            .default_key
            .as_ref()
            .is_some_and(|key| taken_keys.contains(key));
        if taken_paths.contains(*rel) || taken_names.contains(&name) || claims_key {
            continue;
        }
        taken_paths.insert((*rel).to_owned());
        taken_names.insert(name);
        if let Some(key) = space.default_key {
            taken_keys.insert(key);
        }
        out.seeds.push(TemplateSpace {
            from: format!("{pattern_root}/{rel}"),
            to: (*rel).to_owned(),
        });
    }
    out
}

/// The steps a create appends to seed [`plan_template_spaces`]' answer.
///
/// Steps rather than a [`Plan`], because these are not a verb of their own:
/// they belong to the create the operator asked for, in its journal row, so a
/// resumed create finishes the seeding it started instead of leaving the zone
/// half-offered. Compiling a second `spaces-seed` plan beside the create would
/// journal two verbs for one press, and a crash between them would be a state
/// neither row describes.
///
/// Empty in, empty out — no bare `MkDir` for a template that offers nothing,
/// so an ordinary create's plan is byte-identical to what it was.
#[must_use]
pub fn template_seed_steps(seeds: &[TemplateSpace]) -> Vec<PlanStep> {
    if seeds.is_empty() {
        return Vec::new();
    }
    let mut steps = vec![PlanStep::MkDir {
        path: SPACES_DIR.to_owned(),
    }];
    steps.extend(seeds.iter().map(|seed| PlanStep::CopyFile {
        from: seed.from.clone(),
        to: seed.to.clone(),
    }));
    steps
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sessions::pool::{read_one as read_pool_one, PoolFile};
    use crate::sessions::shape::KINDS;

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
    // The six
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
    /// happened, what it points at, what it was told, and last what has said
    /// nothing about itself — and **not** alphabetically, which is what an
    /// unpositioned set would do.
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
        assert_eq!(
            names,
            ["About", "Tasks", "Log", "References", "Prompts", "Untagged"]
        );
        assert_eq!(
            names.last().map(String::as_str),
            Some("Untagged"),
            "the residue is read last, which is the whole of the instruction about it"
        );

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
                "_spaces/untagged.md",
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
        assert_eq!(
            keys,
            ["about", "tasks", "log", "refs", "prompts", "untagged"]
        );

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
        assert_eq!(keys, ["tasks", "refs", "prompts", "untagged"]);

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

    /// **The whole point of the module, end to end.** Each of the six picks
    /// exactly the files of its kind out of one real pool, and nothing else —
    /// and the unfiled `README.md`, which is what a half-migrated session leaves
    /// behind and which no space of a kind may quietly adopt, is picked by the
    /// one space whose whole job is the residue.
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
            ("untagged", vec!["README.md"]),
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
    /// This is what makes the six *defaults* rather than *the feature*.
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

    /// Rows 1 and 4 of the matrix. `ref` is named beside `task` because it is
    /// the kind no button anywhere in the app offered before this verb: a space
    /// that collects something nothing can create is a space that stays as
    /// empty as the template left it.
    #[test]
    fn the_kind_a_space_creates_is_the_one_tag_it_asks_for() {
        assert_eq!(creatable_kind("tag:task"), Some(KindTag::Task));
        assert_eq!(creatable_kind("tag:ref"), Some(KindTag::Ref));
    }

    /// Row 2. `sessions_file_new_kind` refuses `about` — a session has one
    /// record — so the space that collects it must offer nothing rather than a
    /// button whose only outcome is the refusal sentence.
    #[test]
    fn the_about_space_offers_no_create_because_a_session_has_one_record() {
        assert_eq!(creatable_kind("tag:about"), None);
    }

    /// Row 3, and the rule under it: a create takes a kind and a title, so it
    /// has nowhere to put a second term and must not pretend otherwise. The
    /// matrix's own spelling is refused twice over — `AND` is a bareword in
    /// this grammar and `date:today` is not a comparison — so the parseable
    /// pairs beside it are what carries the rule.
    #[test]
    fn a_query_that_asks_for_more_than_one_thing_creates_nothing() {
        for query in [
            "tag:log AND date:today",
            "tag:log date:modified>=-1d",
            "tag:task tag:log",
        ] {
            assert_eq!(creatable_kind(query), None, "{query}");
        }
    }

    /// Row 5, plus the space that was never told what to show. The three broken
    /// queries are `a_broken_query_selects_nothing_and_carries_the_sentence`'s
    /// own: a section rendering a fault sentence, or nothing at all, must not
    /// also render a create.
    #[test]
    fn a_query_the_space_could_not_run_offers_no_create_either() {
        for query in ["is:task", "(tag:log", "tag:log |", "", "   "] {
            assert_eq!(creatable_kind(query), None, "{query:?}");
        }
    }

    /// Row 6. A kind is an exact tag: `tag:project/alpha` is an ordinary tag,
    /// `tag:task/*` is the subtree without its own node, `tag:tasks` is the
    /// plural nobody files under, and `tag:---` is not a tag at all.
    #[test]
    fn a_tag_that_is_not_a_kind_creates_nothing() {
        for query in ["tag:project/alpha", "tag:task/*", "tag:tasks", "tag:---"] {
            assert_eq!(creatable_kind(query), None, "{query}");
        }
    }

    /// `-tag:task` is the space of everything that is *not* a task, and the one
    /// file a create there could write is the one file it excludes. A group or
    /// an `|` has structure, and a term lifted out of a disjunction is not a
    /// term the whole query requires.
    #[test]
    fn a_negated_or_grouped_query_creates_nothing() {
        for query in [
            "-tag:task",
            "- tag:task",
            "(tag:task)",
            "tag:task | tag:log",
        ] {
            assert_eq!(creatable_kind(query), None, "{query}");
        }
    }

    /// The same fold the index applied to the files this space is selecting, so
    /// a query spelled by hand names the kind its own listing already shows.
    #[test]
    fn the_tag_is_folded_the_way_the_index_folds_it() {
        for query in ["  tag:task  ", "tag:TASK", "tag:#Task", "tag:task/"] {
            assert_eq!(creatable_kind(query), Some(KindTag::Task), "{query}");
        }
    }

    /// Every kind reachable from the space that collects it, so a sixth added
    /// to [`KINDS`] later cannot be silently uncreatable — and the six spaces
    /// an operator actually meets on first run, which is the acceptance
    /// sentence: References, Tasks, Log and Prompts can be written into, About
    /// and Untagged cannot.
    #[test]
    fn every_kind_but_about_is_reachable_from_the_space_that_collects_it() {
        for kind in KINDS {
            let query = format!("tag:{}", kind.as_str());
            let expected = (kind != KindTag::About).then_some(kind);
            assert_eq!(creatable_kind(&query), expected, "{query}");
        }
        // The strings as they cross to the surface, which is what a button's
        // presence is decided on.
        let offered: Vec<Option<&str>> = DEFAULT_SESSION_SPACES
            .iter()
            .map(|space| creatable_kind(space.query).map(KindTag::as_str))
            .collect();
        assert_eq!(
            offered,
            [
                None,
                Some("task"),
                Some("log"),
                Some("ref"),
                Some("prompt"),
                // The residue names no kind, so nothing can be written into it.
                None
            ]
        );
    }

    /// Rows 1 and 9. About offers no create and now says WHY, in the contract's
    /// own words: the assertion is that the sentence IS
    /// [`KindHasNoHome::OnlyOne`]'s, byte for byte, and not a second wording of
    /// it composed here or in the surface. Both shapes, because a session has one
    /// record under either contract.
    #[test]
    fn the_about_space_says_a_session_has_one_record_in_the_contracts_own_words() {
        for shape in [Shape::Flat, Shape::Folder] {
            let refused = create_refused("tag:about", shape);
            assert_eq!(
                refused.why,
                Some(Refusal::NoHome(KindHasNoHome::OnlyOne {
                    shape,
                    kind: KindTag::About,
                })),
                "{}",
                shape.as_str()
            );
            assert_eq!(
                refused.why.map(|why| why.to_string()),
                kind_dir(shape, KindTag::About, "")
                    .err()
                    .map(|no_home| no_home.to_string()),
                "the sentence is the contract's, not this module's"
            );
            assert!(
                refused.record,
                "and the verb that applies instead is opening the record"
            );
        }
    }

    /// Row 2. The live About space asks `tag:about tag:recordings`, and the
    /// first refusal in the chain is the QUERY's: two terms, so there is no
    /// single kind a create could write. Reported instead of the record's
    /// refusal because it is the one that would survive the record becoming
    /// creatable — and because it is the refusal the person actually met.
    #[test]
    fn a_space_that_asks_for_more_than_one_thing_says_so() {
        let refused = create_refused("tag:about tag:recordings", Shape::Folder);
        assert_eq!(refused.why, Some(Refusal::ManyTerms));
        assert!(
            refused.record,
            "and it is still the space this session's record is in"
        );
        let sentence = refused.why.expect("a refusal").to_string();
        assert!(sentence.contains("more than one thing"), "{sentence}");

        // A two-term query naming no record offers the same sentence and no
        // record verb: the sentence is about the query's shape, the verb is
        // about what it names. `tag:log tag:task` rather than
        // `tag:log date:today` — the latter does not parse, and an unparseable
        // query is a space that already says why through its own `error`, which
        // is the case `a_space_that_already_says_why_is_left_to_say_it` owns.
        let plain = create_refused("tag:log tag:task", Shape::Flat);
        assert_eq!(plain.why, Some(Refusal::ManyTerms));
        assert!(!plain.record);
    }

    /// The refusals Story 50.1 already projected are untouched, and a space with
    /// a home is still refused nothing — the half of this projection that was
    /// working before the reporter existed.
    #[test]
    fn a_kind_this_contract_keeps_nowhere_is_still_the_refusal_it_was() {
        assert_eq!(
            create_refused("tag:task", Shape::Folder).why,
            Some(Refusal::NoHome(KindHasNoHome::NoDirectory {
                shape: Shape::Folder,
                kind: KindTag::Task,
            })),
        );
        assert_eq!(
            create_refused("tag:log", Shape::Folder).why,
            Some(Refusal::NoHome(KindHasNoHome::NotAFile {
                shape: Shape::Folder,
                kind: KindTag::Log,
            })),
        );
        for (query, shape) in [("tag:ref", Shape::Folder), ("tag:task", Shape::Flat)] {
            let refused = create_refused(query, shape);
            assert_eq!(refused.why, None, "{query}");
            assert!(!refused.record, "{query}");
        }
    }

    /// What a space already explains for itself is not explained a second time.
    /// An unreadable query is on [`SessionSpace::error`], a query that says
    /// nothing is on [`Selection::error`], and a space asking for an ordinary tag
    /// never offered a create to miss — three states where a sentence here would
    /// be keeper answering a question the person can already see answered.
    ///
    /// `-tag:about` used to be a fourth entry in this list and is now
    /// [`Refusal::Negated`]'s: a negated query says nothing about itself
    /// anywhere else, so silence there was the "offers nothing and says nothing"
    /// state Story 51.7 exists to remove. See
    /// `a_query_of_negations_is_refused_for_naming_no_kind`.
    #[test]
    fn a_space_that_already_says_why_is_left_to_say_it() {
        for query in ["", "   ", "(tag:log", "tag:log |", "tag:project/alpha"] {
            let refused = create_refused(query, Shape::Flat);
            assert_eq!(refused.why, None, "{query:?}");
            assert!(!refused.record, "{query:?}");
        }
    }

    /// Row 6. A query made entirely of negations names no kind, so its create is
    /// present-and-refused rather than absent, and the sentence says which of
    /// the two things a person can do instead.
    ///
    /// **Whatever the arity**, which is the ordering claim: one negation and
    /// five negations meet the same refusal, because narrowing a negated query
    /// does not produce a creatable one. `ManyTerms` would have told the person
    /// to narrow to a single `tag:` term, which is a loop.
    ///
    /// And **not** where a positive term survives: `tag:task -tag:done` is two
    /// things and is refused for being two things, unchanged.
    #[test]
    fn a_query_of_negations_is_refused_for_naming_no_kind() {
        for query in [
            "-tag:about",
            "-tag:task",
            "-tag:project/alpha",
            by_key("untagged")
                .expect("the Untagged default exists")
                .query,
        ] {
            let refused = create_refused(query, Shape::Flat);
            assert_eq!(refused.why, Some(Refusal::Negated), "{query:?}");
            assert!(
                !refused.record,
                "a negation names nothing, so it does not name the record either: {query:?}"
            );
            assert_eq!(creatable_kind(query), None, "{query:?}");
            let sentence = refused.why.expect("a refusal").to_string();
            assert!(sentence.contains("left over"), "{sentence}");
            assert!(
                !sentence.contains("Narrow the query"),
                "the advice that sends a negated query round a loop: {sentence}"
            );
        }

        // A surviving positive term is still the arity refusal, so the exception
        // above is exactly as narrow as it says.
        assert_eq!(
            create_refused("tag:task -tag:done", Shape::Flat).why,
            Some(Refusal::ManyTerms)
        );
    }

    /// Rows 3 and 5, at the level the space is defined. The `Untagged` query is
    /// every kind in [`KINDS`] negated, and this zips the two rather than
    /// restating the string: a sixth kind added to `KINDS` then fails HERE
    /// instead of quietly leaving its files in a space that claims to hold
    /// everything unclaimed.
    #[test]
    fn the_untagged_query_negates_every_kind() {
        let space = by_key("untagged").expect("the Untagged default exists");
        let terms = query::conjunction(space.query).expect("a flat conjunction");
        assert_eq!(terms.len(), KINDS.len(), "{}", space.query);
        for (term, kind) in terms.iter().zip(KINDS) {
            assert!(term.negated, "{}", term.source);
            assert_eq!(term.key.as_deref(), Some("tag"), "{}", term.source);
            assert_eq!(term.value, kind.as_str(), "{}", term.source);
        }
        assert!(query::parse(space.query).is_ok(), "{}", space.query);
    }

    /// Row 3, over a real pool: the space picks the kindless file and nothing
    /// else, including the file that carries a tag which is not a kind — which
    /// is the case `is:untagged` would have got wrong, and the reason the query
    /// is the negation and not that flag.
    #[test]
    fn the_untagged_space_picks_what_declares_no_kind_including_the_merely_tagged() {
        let mut files = session();
        files.push((
            "pasted.md",
            "---\ntags: [project/alpha]\n---\n# Pasted in\n",
        ));
        assert_eq!(
            run(&seeded("untagged"), &files).expect("untagged"),
            // `name asc` is the TITLE's order, so "Pasted in" precedes
            // "Something someone dropped in" — asserted the way the space
            // actually draws it rather than the way the filenames sort, which is
            // the mistake this spelling invites.
            ["pasted.md", "README.md"],
            "and the `project/alpha` file is unclaimed by any kind"
        );

        // Row 4's other half, at this level: a session where every file declares
        // a kind gives the space nothing to show, which is what the surface then
        // keys its absence on.
        let filed: Vec<(&str, &str)> = session()
            .into_iter()
            .filter(|(rel, _)| *rel != "README.md")
            .collect();
        assert_eq!(
            run(&seeded("untagged"), &filed).expect("untagged"),
            Vec::<String>::new()
        );
    }

    /// Row 7 (AD-121). The residue space is a default like the other five, so
    /// the directory-is-the-ledger rule already covers it: a zone that has
    /// `_spaces/` and no `untagged.md` is a zone somebody deleted it out of, and
    /// an automatic run adds nothing back. Restore — asked for by hand — is the
    /// only way it returns.
    #[test]
    fn an_untagged_space_the_operator_deleted_stays_deleted() {
        let kept: Vec<SessionSpace> = DEFAULT_SESSION_SPACES
            .iter()
            .filter(|space| space.key != "untagged")
            .map(|space| seeded(space.key))
            .collect();

        assert!(
            plan(SeedMode::FirstRun, Some(&kept)).is_empty(),
            "the next scan must not put it back"
        );
        assert_eq!(
            plan(SeedMode::Restore, Some(&kept))
                .iter()
                .map(|space| space.key)
                .collect::<Vec<_>>(),
            ["untagged"],
            "and pressing Restore is how a person asks for it again"
        );
    }

    /// The record verb and the create can never be offered together, which is
    /// what lets the surface put one in the other's slot: a query that names the
    /// record is refused a create by definition. About is the one default that
    /// carries the verb, under both contracts.
    #[test]
    fn the_record_verb_and_a_create_are_never_offered_together() {
        for space in DEFAULT_SESSION_SPACES {
            for shape in [Shape::Flat, Shape::Folder] {
                let refused = create_refused(space.query, shape);
                assert!(
                    !(refused.record && creatable_kind(space.query).is_some()),
                    "{} under {}",
                    space.query,
                    shape.as_str()
                );
            }
        }
        let carriers: Vec<&str> = DEFAULT_SESSION_SPACES
            .iter()
            .filter(|space| create_refused(space.query, Shape::Folder).record)
            .map(|space| space.key)
            .collect();
        assert_eq!(carriers, ["about"]);
    }

    fn edit(name: &str, query: &str) -> SpaceEdit {
        SpaceEdit {
            name: name.to_owned(),
            query: query.to_owned(),
            sort: "name asc".to_owned(),
            icon: Some("list-todo".to_owned()),
            order: 2.0,
            folded: None,
            rows: None,
            create_dir: String::new(),
        }
    }

    /// Story 52.5, acceptance 7: the destination survives a save and a read, and
    /// clearing the box removes the key rather than writing an empty one — the
    /// space is then back to today's behaviour with nothing left in the file to
    /// explain.
    #[test]
    fn a_destination_round_trips_and_clearing_it_writes_no_key() {
        let source = render_new(
            &SpaceEdit {
                create_dir: "logs".to_owned(),
                ..edit("Log", "tag:log")
            },
            "01ABC",
            "2026-08-17",
        );
        assert!(source.contains("create_dir: logs"), "{source}");
        assert_eq!(read_one("_spaces/log.md", &source).create_dir, "logs");

        let cleared = render_edit(
            "_spaces/log.md",
            &source,
            &SpaceEdit {
                create_dir: String::new(),
                ..edit("Log", "tag:log")
            },
        );
        assert!(
            !cleared.contains("create_dir"),
            "an empty destination writes no key: {cleared}"
        );
        assert_eq!(read_one("_spaces/log.md", &cleared).create_dir, "");
    }

    /// A destination is a path, so what is read is the path and not the
    /// operator's whitespace: `logs/` is the same request as `logs`
    /// (`files::dir_rel`'s rule), and a key holding blanks names nothing.
    #[test]
    fn a_destination_is_read_as_the_directory_it_names() {
        for (written, expected) in [
            ("logs", "logs"),
            ("logs/", "logs"),
            ("notes/2026", "notes/2026"),
            ("  logs  ", "logs"),
            ("   ", ""),
        ] {
            let text =
                format!("---\nkeeper:\n  space: tag:log\n  create_dir: {written}\n---\n# Log\n");
            assert_eq!(
                read_one("_spaces/log.md", &text).create_dir,
                expected,
                "{written:?}"
            );
        }
        // A file that says nothing about a destination is every space until
        // somebody types one, and it must read as empty rather than as absent-
        // and-therefore-something.
        assert_eq!(
            read_one(
                "_spaces/log.md",
                "---\nkeeper:\n  space: tag:log\n---\n# Log\n"
            )
            .create_dir,
            ""
        );
    }

    /// Story 52.5, acceptance 6 — AD-120 with a directory in play. A file inside
    /// a space's own directory is the kind its TAG declares: the reference filed
    /// into `logs/` is picked by the References space and never by the Log one,
    /// and both spaces find it there because the pool reads markdown wherever it
    /// sits (FR-285) and the query matches a tag rather than a path.
    #[test]
    fn a_file_in_a_spaces_directory_is_the_kind_its_tag_says() {
        let files = vec![
            ("logs/2026-08-17-0900-the-spec.md", REF),
            ("logs/2026-08-17-0901-opened.md", LOG_A),
        ];
        assert_eq!(
            run(&seeded("refs"), &files).expect("the References space"),
            ["logs/2026-08-17-0900-the-spec.md"],
            "a `ref` in logs/ is a reference"
        );
        assert_eq!(
            run(&seeded("log"), &files).expect("the Log space"),
            ["logs/2026-08-17-0901-opened.md"],
            "and the directory adopted nothing"
        );
    }

    /// FR-121 in one assertion: prose, unknown keys and key order all survive a
    /// save that changed one thing. A space file is something the operator opens
    /// in Obsidian, and an editor that reformats it is an editor they stop
    /// trusting with the parts keeper does not understand.
    #[test]
    fn saving_a_space_leaves_every_other_byte_alone() {
        let source = concat!(
            "---\n",
            "id: 01ABC\n",
            "cssclass: board\n",
            "keeper:\n",
            "  space: tag:task\n",
            "  sort: order asc\n",
            "  icon: list-todo\n",
            "  order: 2\n",
            "  default: tasks\n",
            "---\n",
            "\n# Tasks\n",
            "\nWhat is left, and what is in flight.\n"
        );
        let saved = render_edit(
            "_spaces/tasks.md",
            source,
            &SpaceEdit {
                sort: "order asc".to_owned(),
                ..edit("Tasks", "tag:task -field:status=done")
            },
        );
        assert!(saved.contains("cssclass: board"));
        assert!(saved.contains("id: 01ABC"));
        assert!(saved.contains("What is left, and what is in flight."));
        let reread = read_one("_spaces/tasks.md", &saved);
        assert_eq!(reread.query, "tag:task -field:status=done");
        assert_eq!(reread.name, "Tasks");
    }

    /// Editing a seeded space must not un-seed it. `keeper.default` comes off
    /// the file, never off the request, so renaming Tasks to "Backlog" leaves it
    /// the Tasks default and Restore does not offer a second copy.
    #[test]
    fn editing_a_default_keeps_its_marker_and_retitles_the_heading() {
        let source = render_note(by_key("tasks").expect("tasks"), "01ABC", "2026-08-14");
        let saved = render_edit("_spaces/tasks.md", &source, &edit("Backlog", "tag:task"));
        let reread = read_one("_spaces/tasks.md", &saved);
        assert_eq!(reread.default_key.as_deref(), Some("tasks"));
        assert_eq!(reread.name, "Backlog");
        assert!(saved.contains("# Backlog"), "heading followed the title");
    }

    /// A body the operator wrote is theirs. `retitle_heading` refuses anything
    /// but a lone matching heading, so the name changes in frontmatter and the
    /// prose is left exactly as typed.
    #[test]
    fn renaming_does_not_touch_a_body_the_operator_wrote() {
        let source = "---\nkeeper:\n  space: tag:ref\n---\n\n## Notes\n\nMine.\n";
        let saved = render_edit("_spaces/refs.md", source, &edit("Sources", "tag:ref"));
        assert!(saved.contains("## Notes\n\nMine.\n"));
        assert_eq!(read_one("_spaces/refs.md", &saved).name, "Sources");
    }

    /// Zero is *unset*, so it is never written: a space nobody positioned keeps
    /// the frontmatter it had rather than growing a key that claims it was
    /// placed first. Same rule for an icon nobody chose.
    #[test]
    fn an_unset_position_and_a_missing_icon_write_no_key() {
        let saved = render_new(
            &SpaceEdit {
                icon: Some("   ".to_owned()),
                order: sort::DEFAULT_SPACE_ORDER,
                ..edit("Archive", "tag:archive")
            },
            "01ABC",
            "2026-08-14",
        );
        assert!(!saved.contains("icon:"), "{saved}");
        assert!(!saved.contains("order:"), "{saved}");
        assert!(
            !saved.contains("default:"),
            "a new space is nobody's default"
        );
        let reread = read_one("_spaces/archive.md", &saved);
        assert_eq!(reread.order, sort::DEFAULT_SPACE_ORDER);
        assert!(reread.icon.is_none());
        assert!(reread.warnings.is_empty());
    }

    /// **The trap test** (Story 51.3, row 9). `render_edit` replaces the whole
    /// `keeper:` map, so a key the editor does not carry is destroyed by the
    /// first unrelated Save. The edit here changes only the query, and both new
    /// keys have to come back out of the bytes.
    ///
    /// Read back through `read_one` rather than grepped for `folded: true`: the
    /// destroyed-key failure and the written-but-unreadable failure are
    /// different bugs, and only the round trip catches the second.
    #[test]
    fn an_unrelated_save_keeps_the_fold_and_the_row_cap() {
        let source = concat!(
            "---\n",
            "keeper:\n",
            "  space: tag:log\n",
            "  sort: modified desc\n",
            "  folded: true\n",
            "  rows: 5\n",
            "---\n",
            "# Log\n"
        );
        let stored = read_one("_spaces/log.md", source);
        assert_eq!(stored.folded, Some(true));
        assert_eq!(stored.rows, Some(5));

        // What the editor sends when somebody retyped the query and touched
        // nothing else: the form seeds these two from what it read, exactly as
        // it seeds the sort.
        let saved = render_edit(
            "_spaces/log.md",
            source,
            &SpaceEdit {
                sort: "modified desc".to_owned(),
                folded: stored.folded,
                rows: stored.rows,
                ..edit("Log", "tag:log -field:status=done")
            },
        );
        let reread = read_one("_spaces/log.md", &saved);
        assert_eq!(reread.query, "tag:log -field:status=done");
        assert_eq!(reread.folded, Some(true), "{saved}");
        assert_eq!(reread.rows, Some(5), "{saved}");
        assert!(reread.warnings.is_empty(), "{:?}", reread.warnings);
    }

    /// Row 11: setting both writes exactly two more keys, and they read back.
    /// `folded: false` is a statement and not an absence — it is what beats the
    /// user-global setting — so it is written where `order: 0` would not be.
    #[test]
    fn the_editor_writes_exactly_the_two_keys_it_was_given() {
        let saved = render_new(
            &SpaceEdit {
                folded: Some(false),
                rows: Some(3),
                ..edit("Archive", "tag:archive")
            },
            "01ABC",
            "2026-08-14",
        );
        assert!(saved.contains("folded: false"), "{saved}");
        assert!(saved.contains("rows: 3"), "{saved}");
        let reread = read_one("_spaces/archive.md", &saved);
        assert_eq!(reread.folded, Some(false));
        assert_eq!(reread.rows, Some(3));
        assert!(reread.warnings.is_empty());
    }

    /// Row 10: a space the form left alone grows neither key. Both renderers,
    /// because it is the pair of them that has to agree — a `render_new` that
    /// wrote `folded: false` for "unset" would make every hand-made space
    /// override the setting the moment it was created.
    #[test]
    fn a_space_that_says_nothing_about_opening_writes_no_key() {
        let unset = edit("Archive", "tag:archive");
        let created = render_new(&unset, "01ABC", "2026-08-14");
        let saved = render_edit(
            "_spaces/archive.md",
            "---\nkeeper:\n  space: tag:archive\n---\n# Archive\n",
            &unset,
        );
        for text in [&created, &saved] {
            assert!(!text.contains("folded:"), "{text}");
            assert!(!text.contains("rows:"), "{text}");
        }
        let reread = read_one("_spaces/archive.md", &created);
        assert!(reread.folded.is_none());
        assert!(reread.rows.is_none());
    }

    /// Row 7 and row 8: an unreadable value is a WARNING and the space still
    /// works. Zero is in the list because it is the value most likely to be
    /// typed on purpose — and a cap of zero is a section with no rows under a
    /// header that still counts them.
    #[test]
    fn an_unreadable_fold_or_cap_warns_and_changes_nothing() {
        for (line, expect) in [
            ("rows: 0", "row limit"),
            ("rows: -2", "row limit"),
            ("rows: many", "row limit"),
            ("rows: 2.5", "row limit"),
            ("folded: yes", "yes or no"),
            ("folded: 1", "yes or no"),
        ] {
            let text = format!("---\nkeeper:\n  space: tag:log\n  {line}\n---\n# Log\n");
            let space = read_one("_spaces/log.md", &text);
            assert_eq!(space.warnings.len(), 1, "{line}: {:?}", space.warnings);
            assert!(
                space.warnings[0].contains(expect),
                "{line}: {}",
                space.warnings[0]
            );
            assert!(space.folded.is_none(), "{line}");
            assert!(space.rows.is_none(), "{line}");
            // Still a working space: the query is what it says, and the section
            // renders it.
            assert_eq!(space.query, "tag:log");
        }
    }

    /// A key an editor cleared is not a fault — the same hole `read_order`
    /// forgives, and for the same reason: `rows:` with nothing after it is what
    /// a form writes on its way to writing no key at all.
    #[test]
    fn an_empty_fold_or_cap_is_silence_and_not_a_warning() {
        let space = read_one(
            "_spaces/log.md",
            "---\nkeeper:\n  space: tag:log\n  folded:\n  rows:\n---\n# Log\n",
        );
        assert!(space.warnings.is_empty(), "{:?}", space.warnings);
        assert!(space.folded.is_none());
        assert!(space.rows.is_none());
    }

    /// Row 12: a restored default carries what the default says, which is
    /// nothing about either key — the five are meant to be read on arrival, and
    /// `sessions.spaces_folded` is where "shut them all" lives.
    #[test]
    fn a_seeded_default_says_nothing_about_folding_or_capping() {
        for space in &DEFAULT_SESSION_SPACES {
            let text = render_note(space, "01ABC", "2026-08-14");
            assert!(!text.contains("folded:"), "{}: {text}", space.key);
            assert!(!text.contains("rows:"), "{}: {text}", space.key);
            let reread = seeded(space.key);
            assert!(reread.folded.is_none(), "{}", space.key);
            assert!(reread.rows.is_none(), "{}", space.key);
        }
    }

    /// A hand-made space called "Tasks" lands beside the seeded one instead of
    /// overwriting it — the collision is decided by the same fold [`claimed`]
    /// uses, so `Tasks` and `tasks` are one name here too.
    #[test]
    fn a_new_space_never_lands_on_a_taken_name() {
        let taken: BTreeSet<String> = ["_spaces/tasks.md", "_spaces/tasks-2.md"]
            .into_iter()
            .map(str::to_owned)
            .collect();
        assert_eq!(rel_for_new("Tasks", &taken), "_spaces/tasks-3.md");
        assert_eq!(rel_for_new("  TASKS  ", &taken), "_spaces/tasks-3.md");
    }

    /// A name that folds away entirely still has to become a file, and `.md` is
    /// not a filename. `naming::slug` already guarantees this for notes; the
    /// assertion is here to catch a future rel_for_new that stops going through
    /// it.
    #[test]
    fn a_name_that_folds_to_nothing_still_gets_a_filename() {
        let rel = rel_for_new("???", &BTreeSet::new());
        assert!(is_space_path(&rel), "{rel}");
        assert_eq!(rel, format!("{SPACES_DIR}/{}.md", naming::slug("???")));
    }

    /// A save into a zone that never had `_spaces/` creates it. The step is
    /// idempotent, so the same plan shape serves a first Restore and an
    /// ordinary edit — one path, and no "does the directory exist" question at
    /// the call site.
    #[test]
    fn a_save_makes_the_directory_before_it_writes_into_it() {
        let plan = compile_save(
            "_spaces/archive.md",
            None,
            &edit("Archive", "tag:archive"),
            "01A",
            "2026-08-14",
        );
        assert_eq!(
            plan.steps.first(),
            Some(&PlanStep::MkDir {
                path: SPACES_DIR.to_owned()
            })
        );
        let PlanStep::WriteFile { path, content } = &plan.steps[1] else {
            panic!("second step writes the file: {:?}", plan.steps[1]);
        };
        assert_eq!(path, "_spaces/archive.md");
        assert_eq!(read_one(path, content).query, "tag:archive");
    }

    /// A delete is a trash move, not an unlink: a space is a file somebody
    /// wrote, and the whole plan being the irreversible step is AD-111 with
    /// nothing before it to undo.
    #[test]
    fn a_delete_trashes_rather_than_unlinks() {
        let plan = compile_delete("_spaces/prompts.md", "01TRASH");
        assert_eq!(
            plan.steps,
            [PlanStep::TrashFile {
                path: "_spaces/prompts.md".to_owned(),
                trash_key: "01TRASH".to_owned()
            }]
        );
    }

    /// Seeding writes one file per default, each with its own id — a zip, so
    /// too few ids seeds fewer spaces rather than giving two files one id.
    #[test]
    fn seeding_gives_every_default_its_own_file_and_id() {
        let defaults = plan(SeedMode::FirstRun, None);
        let ids: Vec<String> = (0..defaults.len()).map(|i| format!("01ID{i}")).collect();
        let seeded = compile_seed(&defaults, &ids, "2026-08-14");
        let written: Vec<&str> = seeded
            .steps
            .iter()
            .filter_map(|step| match step {
                PlanStep::WriteFile { path, .. } => Some(path.as_str()),
                _ => None,
            })
            .collect();
        assert_eq!(written.len(), DEFAULT_SESSION_SPACES.len());
        assert!(written.iter().all(|rel| is_space_path(rel)), "{written:?}");

        let short = compile_seed(&defaults, &ids[..2], "2026-08-14");
        assert_eq!(short.steps.len(), 3, "mkdir plus two writes");
    }

    /// The containment rule. The executor only proves a path cannot escape the
    /// zone, which would still let a delete take a session's own `about.md`.
    #[test]
    fn only_paths_directly_inside_the_spaces_directory_are_writable() {
        assert!(is_space_path("_spaces/tasks.md"));
        assert!(!is_space_path("_spaces/nested/tasks.md"));
        assert!(!is_space_path("_spaces/.md"));
        assert!(!is_space_path("_spaces/.hidden.md"));
        assert!(!is_space_path("_spaces/tasks.txt"));
        assert!(!is_space_path("_spaces"));
        assert!(!is_space_path("active/2026-08-14-keeper/about.md"));
        assert!(!is_space_path("../_spaces/tasks.md"));
    }

    // -----------------------------------------------------------------------
    // The spaces a template offers a zone (FR-291)
    // -----------------------------------------------------------------------

    /// A zone's `_spaces/` as `plan_template_spaces` wants it: read through the
    /// one reader, so a test can never assert against a shape the reader would
    /// not produce.
    fn zone(files: &[(&str, &str)]) -> Vec<SessionSpace> {
        read_all(files)
    }

    /// Row 1. The zone lacks Tasks, the template has one, the zone gains it —
    /// as a copy out of the template, into the zone's own `_spaces/`, never
    /// into the session (AD-121).
    #[test]
    fn a_template_seeds_a_space_the_zone_does_not_have() {
        let existing = zone(&[(
            "_spaces/log.md",
            "---\nkeeper:\n  space: tag:log\n---\n# Log\n",
        )]);
        let planned = plan_template_spaces(
            "_template/house",
            &[(
                "_spaces/tasks.md",
                "---\ntitle: Tasks\nkeeper:\n  space: tag:task\n---\n# Tasks\n",
            )],
            &existing,
        );
        assert!(planned.skipped.is_empty(), "{:?}", planned.skipped);
        assert_eq!(
            planned.seeds,
            vec![TemplateSpace {
                from: "_template/house/_spaces/tasks.md".to_owned(),
                to: "_spaces/tasks.md".to_owned(),
            }]
        );
        assert_eq!(
            template_seed_steps(&planned.seeds),
            vec![
                PlanStep::MkDir {
                    path: "_spaces".to_owned()
                },
                PlanStep::CopyFile {
                    from: "_template/house/_spaces/tasks.md".to_owned(),
                    to: "_spaces/tasks.md".to_owned(),
                },
            ],
            "journaled with the create, and never a write inside the session"
        );
    }

    /// Row 2, three ways. The zone's own edited space always wins — whether the
    /// two agree about the path, about the folded name, or about the default
    /// key. A create is pressed many times a week, and one that could rewrite a
    /// tuned query is one an operator learns to fear.
    #[test]
    fn the_zones_own_space_is_never_overwritten() {
        let by_path = zone(&[(
            "_spaces/tasks.md",
            "---\ntitle: Chores\nkeeper:\n  space: tag:task date:today\n---\n# Chores\n",
        )]);
        let by_name = zone(&[(
            "_spaces/mine.md",
            "---\ntitle: TASKS\nkeeper:\n  space: tag:task\n---\n# TASKS\n",
        )]);
        let by_key = zone(&[(
            "_spaces/whatever.md",
            "---\ntitle: Backlog\nkeeper:\n  space: tag:task\n  default: tasks\n---\n# Backlog\n",
        )]);
        let candidate = &[(
            "_spaces/tasks.md",
            "---\ntitle: Tasks\nkeeper:\n  space: tag:task\n  default: tasks\n---\n# Tasks\n",
        )][..];
        for existing in [by_path, by_name, by_key] {
            let planned = plan_template_spaces("_template", candidate, &existing);
            assert!(planned.seeds.is_empty(), "the zone's file stands");
            assert!(
                planned.skipped.is_empty(),
                "and says nothing about it — that is the ordinary case"
            );
        }
    }

    /// Row 3. An entry keeper cannot run is skipped with a sentence naming it,
    /// and the caller is handed a plan it can still execute: a typo in a
    /// template's space file is not a reason to refuse somebody a session.
    #[test]
    fn an_unusable_entry_is_skipped_with_a_sentence_and_the_rest_still_seeds() {
        let planned = plan_template_spaces(
            "_template",
            &[
                ("_spaces/silent.md", "---\ntitle: Silent\n---\n# Silent\n"),
                (
                    "_spaces/broken.md",
                    "---\ntitle: Broken\nkeeper:\n  space: \"nope:x\"\n---\n# Broken\n",
                ),
                (
                    "_spaces/nested/deep.md",
                    "---\nkeeper:\n  space: tag:log\n---\n# Deep\n",
                ),
                ("_spaces/notes.txt", "---\nkeeper:\n  space: tag:log\n---\n"),
                (
                    "_spaces/refs.md",
                    "---\ntitle: References\nkeeper:\n  space: tag:ref\n---\n# References\n",
                ),
            ],
            &[],
        );
        assert_eq!(
            planned.seeds,
            vec![TemplateSpace {
                from: "_template/_spaces/refs.md".to_owned(),
                to: "_spaces/refs.md".to_owned(),
            }],
            "the good one still lands"
        );
        assert_eq!(planned.skipped.len(), 4, "{:?}", planned.skipped);
        for name in ["silent.md", "broken.md", "deep.md", "notes.txt"] {
            assert!(
                planned.skipped.iter().any(|line| line.contains(name)),
                "the sentence names the file: {:?}",
                planned.skipped
            );
        }
        assert!(
            planned
                .skipped
                .iter()
                .any(|line| line.contains("doesn't say what to show")),
            "a space with no query would be a permanently empty row keeper wrote"
        );
    }

    /// Two candidates that fold onto one name: the first lands, the second is
    /// explained. Without folding the candidates against each other, a template
    /// could seed two files into the rail that are the same space twice.
    #[test]
    fn two_template_spaces_of_one_name_seed_once() {
        let planned = plan_template_spaces(
            "_template",
            &[
                (
                    "_spaces/tasks.md",
                    "---\ntitle: Tasks\nkeeper:\n  space: tag:task\n---\n# Tasks\n",
                ),
                (
                    "_spaces/tasks-again.md",
                    "---\ntitle: TASKS\nkeeper:\n  space: tag:task\n---\n# TASKS\n",
                ),
            ],
            &[],
        );
        assert_eq!(planned.seeds.len(), 1);
        assert_eq!(planned.seeds[0].to, "_spaces/tasks.md");
    }

    /// A template that offers nothing leaves the create's plan byte-identical
    /// to what it was — no bare `MkDir` for a directory nobody is writing into.
    #[test]
    fn offering_nothing_appends_nothing() {
        assert!(template_seed_steps(&[]).is_empty());
    }
}
