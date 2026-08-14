//! Which on-disk contract a session follows, and the vocabulary the flat one
//! speaks (FR-256, AD-119, AD-120).
//!
//! The original contract gave every kind of file its own directory: the record
//! in `README.md`, pointers in `refs/`, reusable text in `prompts/`. That makes
//! *where a file sits* decide *what it is*, so moving a file changes its
//! meaning and a file can only ever be one thing.
//!
//! The flat contract inverts it: one pool of markdown at the session root, each
//! file declaring its own kind in frontmatter as a tag (AD-120). `artifacts/`
//! and `workspace/` survive, because they are the two subtrees that are not
//! markdown and whose difference is about *versioning*, not about kind.
//!
//! Both contracts are live at once and neither is being deprecated on a
//! timetable: the zones are real folders on the operator's drives, migration is
//! a verb someone chooses, and a session nobody migrates must keep working
//! forever. So everything here is a *reader's* rule — given bytes and names,
//! which parser answers — and the answer is derived, never stored (AD-110).

/// The navigation file the flat contract puts at a session's root: how to read
/// this folder, written for whoever — or whatever — is handed it.
///
/// The flat shape's known cost is that a session folder is an undifferentiated
/// pile of markdown until something reads the tags. This file is the mitigation,
/// which is why its presence is also the shape signal: it is the one file whose
/// existence means "someone has stated how to read this folder".
pub const AGENTS: &str = "AGENTS.md";

/// The flat contract's record — what the folder-shaped session kept in
/// `README.md`. Summary, decisions, and the `## Promote` table.
pub const ABOUT: &str = "about.md";

/// Which on-disk contract one session follows.
///
/// Two variants and no `Unknown`: every directory that [`super::model::classify`]
/// calls a session is readable as one shape or the other, and a third answer
/// would push a decision the reader can make onto every call site.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Shape {
    /// One markdown pool at the session root; kinds are tags (AD-120).
    Flat,
    /// `README.md` plus `refs/` and `prompts/` — the original contract.
    Folder,
}

impl Shape {
    /// The wire spelling. A string rather than a bool on the VM because the set
    /// may grow and a field named `flat` cannot carry a third answer.
    pub fn as_str(self) -> &'static str {
        match self {
            Shape::Flat => "flat",
            Shape::Folder => "folder",
        }
    }
}

/// Decide a session's shape from its top-level entry names alone.
///
/// `top_level` is the session directory's own entries — names, not paths, not
/// recursive. The shell already reads this to compute freshness, so detection
/// costs no extra IO.
///
/// **Presence, not absence.** The predicate is "does a file the flat contract
/// writes exist", for three reasons:
///
/// - Absence of `refs/` cannot tell a migrated session from a brand-new empty
///   one, and the live zone has an empty `refs/` holding only `.gitkeep`.
/// - Parsing `README.md` for a `## Log` would make every board row pay a parse
///   before it knows which parser to use, and would let a session flip shape
///   because someone typed a heading into prose.
/// - A file keeper's own template writes is positive evidence, and it is what
///   the operator sees in Finder.
///
/// `AGENTS` **or** `ABOUT`, not both: a hand-built flat session may start with
/// either, and requiring both would misclassify an honest one.
///
/// A folder holding `README.md` *and* `AGENTS.md` reads as [`Shape::Flat`].
/// That is deliberate and it is the safe direction: `AGENTS.md` exists only
/// because migration wrote it or a person did, and both mean "read me as flat".
/// The residual README becomes an ordinary untagged file in the pool — which
/// the detail surfaces as *unfiled*, so a half-finished migration is visible
/// rather than merely survivable. The other default would hide every migrated
/// log behind a `## Log` section that no longer exists.
pub fn shape(top_level: &[String]) -> Shape {
    let has = |name: &str| top_level.iter().any(|entry| entry == name);
    if has(AGENTS) || has(ABOUT) {
        Shape::Flat
    } else {
        Shape::Folder
    }
}

/// The kinds a session markdown file can declare, as tags rather than folders
/// (AD-120).
///
/// A closed set, like [`crate::notes::query`]'s flag list is closed: these five
/// are what the zone's predefined spaces select, and an open set would mean the
/// board could not name its own columns. A file may of course carry any other
/// tags it likes — they are ordinary tags and the query language reaches them;
/// what is closed is the set of kinds keeper itself surfaces.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum KindTag {
    /// The session's record — one per session, normally.
    About,
    /// One sitting. Filenames carry `YYYY-MM-DD-HHMM` so the pool self-sorts.
    Log,
    /// Reusable text worth keeping.
    Prompt,
    /// A pointer at something that lives elsewhere.
    Ref,
    /// A unit of work, carrying `status` and `order`.
    Task,
}

/// The five kinds in **declaration order**, which is also precedence order for
/// [`KindTag::of`].
pub const KINDS: [KindTag; 5] = [
    KindTag::About,
    KindTag::Log,
    KindTag::Prompt,
    KindTag::Ref,
    KindTag::Task,
];

impl KindTag {
    /// The tag that declares this kind. Singular, because it labels one file;
    /// the *spaces* that collect them are named in the plural.
    pub fn as_str(self) -> &'static str {
        match self {
            KindTag::About => "about",
            KindTag::Log => "log",
            KindTag::Prompt => "prompt",
            KindTag::Ref => "ref",
            KindTag::Task => "task",
        }
    }

    /// The kind a normalised tag list declares, or `None` for an unfiled file.
    ///
    /// Tags arrive from [`crate::notes::tags::note_tags`], already normalised
    /// and sorted, so this compares against the normalised spelling.
    ///
    /// **First match in [`KINDS`] order wins.** A file tagged both `log` and
    /// `ref` is a log. One file, one kind, decided here — because the
    /// alternative is that the log space and the refs space each answer
    /// separately and the same file appears twice in a board that is supposed
    /// to be a partition. A file that wants to be found both ways is found both
    /// ways by *querying* its tags; its *kind* is still one thing.
    ///
    /// Hierarchy is deliberately not honoured here: `task/blocked` does not
    /// declare `Task`. A kind is an exact tag, because a hierarchical kind
    /// would make `ref/input` and `ref` the same column while `tag:ref` already
    /// matches both for query purposes (that is `tag_covers`' job, not this
    /// function's).
    pub fn of(tags: &[String]) -> Option<KindTag> {
        KINDS
            .into_iter()
            .find(|kind| tags.iter().any(|tag| tag == kind.as_str()))
    }
}

/// The four states a task file can be in — the board's columns (FR-259).
///
/// Closed, and closed on purpose: the columns of a board are its whole
/// grammar, and an open set would mean a typo in one file silently invents a
/// fifth column that nothing else knows how to fill.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TaskStatus {
    /// Not ready to start — being shaped.
    InPreparation,
    /// Ready, waiting.
    Todo,
    /// Finished.
    Done,
    /// Consciously not now. Distinct from `Todo`, because "we decided not to"
    /// is information and dropping it is how a backlog rots.
    Deferred,
}

/// The four states in board order, left to right: work flows in preparation →
/// todo → done, with deferred as the deliberate exit.
pub const STATUSES: [TaskStatus; 4] = [
    TaskStatus::InPreparation,
    TaskStatus::Todo,
    TaskStatus::Done,
    TaskStatus::Deferred,
];

impl TaskStatus {
    /// The `status:` value as written in frontmatter.
    pub fn as_str(self) -> &'static str {
        match self {
            TaskStatus::InPreparation => "in-preparation",
            TaskStatus::Todo => "todo",
            TaskStatus::Done => "done",
            TaskStatus::Deferred => "deferred",
        }
    }

    /// The column heading.
    pub fn label(self) -> &'static str {
        match self {
            TaskStatus::InPreparation => "In preparation",
            TaskStatus::Todo => "To do",
            TaskStatus::Done => "Done",
            TaskStatus::Deferred => "Deferred",
        }
    }

    /// Read a `status:` value. Trimmed, case-folded, and tolerant of the two
    /// spellings a person actually types for the hyphenated one.
    ///
    /// `None` for anything else — an unreadable status is **reported**, never
    /// coerced to `Todo`. Coercing would put a card the operator cannot account
    /// for into the column that means "start this next", and the misfiled card
    /// would look exactly like a real one.
    pub fn parse(raw: &str) -> Option<TaskStatus> {
        let folded = raw.trim().to_ascii_lowercase();
        match folded.as_str() {
            "in-preparation" | "in preparation" | "in_preparation" => {
                Some(TaskStatus::InPreparation)
            }
            "todo" | "to-do" | "to do" => Some(TaskStatus::Todo),
            "done" => Some(TaskStatus::Done),
            "deferred" => Some(TaskStatus::Deferred),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn names(entries: &[&str]) -> Vec<String> {
        entries.iter().map(|e| (*e).to_owned()).collect()
    }

    /// Detection is presence of a file the flat contract writes — either one,
    /// never a parse, never an absence.
    #[test]
    fn agents_md_or_about_md_makes_a_session_flat() {
        assert_eq!(
            shape(&names(&["AGENTS.md", "artifacts", "workspace"])),
            Shape::Flat
        );
        assert_eq!(
            shape(&names(&["about.md", "2026-08-12-0900-opened.md"])),
            Shape::Flat,
            "a hand-built flat session may start with either file"
        );
        assert_eq!(
            shape(&names(&["README.md", "refs", "prompts", "workspace"])),
            Shape::Folder
        );
        assert_eq!(
            shape(&names(&["README.md"])),
            Shape::Folder,
            "a bare record is the original contract"
        );
        assert_eq!(
            shape(&[]),
            Shape::Folder,
            "an empty folder is not evidence of the new shape"
        );
    }

    /// The half-migrated folder resolves toward flat, so the leftover README
    /// shows up as unfiled rather than shadowing the migrated logs.
    #[test]
    fn a_folder_with_readme_and_agents_reads_as_flat() {
        let half = names(&["README.md", "AGENTS.md", "about.md", "refs"]);
        assert_eq!(shape(&half), Shape::Flat);
    }

    /// An empty `refs/` is exactly what the live zone holds, so absence of a
    /// directory can never be the signal.
    #[test]
    fn an_empty_refs_dir_does_not_make_a_session_flat() {
        assert_eq!(shape(&names(&["README.md", "refs"])), Shape::Folder);
    }

    /// One file, one kind — and the tie is broken by a rule stated once.
    #[test]
    fn kind_tag_of_picks_declaration_order_for_a_doubly_tagged_file() {
        assert_eq!(KindTag::of(&names(&["log"])), Some(KindTag::Log));
        assert_eq!(
            KindTag::of(&names(&["log", "ref"])),
            Some(KindTag::Log),
            "declaration order decides, so a file never fills two columns"
        );
        assert_eq!(
            KindTag::of(&names(&["ref", "about"])),
            Some(KindTag::About),
            "precedence is KINDS order, not the file's own tag order"
        );
        assert_eq!(KindTag::of(&names(&["project/keeper"])), None);
        assert_eq!(KindTag::of(&[]), None, "an untagged file is unfiled");
    }

    /// A kind is an exact tag: hierarchy is the query language's job.
    #[test]
    fn a_hierarchical_tag_does_not_declare_a_kind() {
        assert_eq!(KindTag::of(&names(&["task/blocked"])), None);
        assert_eq!(KindTag::of(&names(&["ref/input"])), None);
    }

    /// The board's grammar is closed, and an unreadable value is reported
    /// rather than filed under "start this next".
    #[test]
    fn task_status_parses_the_four_and_refuses_a_fifth() {
        assert_eq!(
            TaskStatus::parse("in-preparation"),
            Some(TaskStatus::InPreparation)
        );
        assert_eq!(TaskStatus::parse("todo"), Some(TaskStatus::Todo));
        assert_eq!(TaskStatus::parse("done"), Some(TaskStatus::Done));
        assert_eq!(TaskStatus::parse("deferred"), Some(TaskStatus::Deferred));

        assert_eq!(TaskStatus::parse("  TODO "), Some(TaskStatus::Todo));
        assert_eq!(
            TaskStatus::parse("In Preparation"),
            Some(TaskStatus::InPreparation),
            "the spelling a person types is the spelling that works"
        );

        assert_eq!(TaskStatus::parse("blocked"), None);
        assert_eq!(TaskStatus::parse(""), None);
        assert_eq!(
            TaskStatus::parse("todo!"),
            None,
            "near-misses are refused rather than guessed at"
        );
    }

    /// The wire spellings are the frontmatter spellings, and the board order is
    /// the order work moves in.
    #[test]
    fn the_wire_spellings_are_stable() {
        assert_eq!(Shape::Flat.as_str(), "flat");
        assert_eq!(Shape::Folder.as_str(), "folder");
        assert_eq!(
            STATUSES.map(TaskStatus::as_str),
            ["in-preparation", "todo", "done", "deferred"]
        );
        assert_eq!(
            KINDS.map(KindTag::as_str),
            ["about", "log", "prompt", "ref", "task"]
        );
        // Every status round-trips through its own spelling.
        for status in STATUSES {
            assert_eq!(TaskStatus::parse(status.as_str()), Some(status));
        }
    }
}
