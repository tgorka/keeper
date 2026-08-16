//! Folder-shaped session → flat markdown pool, compiled to a plan (FR-257).
//!
//! Migration is a *verb someone chooses*, never something a scan does on the
//! operator's behalf: the zones are real folders on real drives, and a reader
//! that rewrote what it read would turn opening the board into a commit. So
//! this module compiles the same kind of journaled plan every other lifecycle
//! verb compiles, and the shell runs it with the same executor and the same
//! crash-resume story (AD-111).
//!
//! What it converts:
//!
//! - `README.md` → `about.md`, minus its `## Log` section, keeping every other
//!   byte of the record — including the `## Promote` table, which is the
//!   session's contract with the archive checklist and travels verbatim.
//! - each `### <date> — <title>` log entry → one `YYYY-MM-DD-HHMM-slug.md`
//!   tagged `log`, so the pool self-sorts in Finder, in `ls`, and in keeper.
//! - each `refs/*.md` and `prompts/*.md` → a root file with `ref` or `prompt`
//!   added to its tags and **every other byte untouched** (FR-121).
//! - a new `AGENTS.md`, the navigation file the flat contract owes whoever —
//!   or whatever — is handed the folder.
//!
//! ## The order is the safety argument
//!
//! `AGENTS.md` is written *after* every file the flat reader needs and *before*
//! anything is removed, because writing it is the shape flip: the instant it
//! lands, [`crate::sessions::shape::shape`] answers `Flat` and the log is read
//! from the pool rather than from `## Log`. Writing it first would open a window
//! — however short — in which the session reads as flat and has no logs at all.
//! The two `TrashDir` steps sort last for the reason every other verb sorts its
//! irreversible step last: everything before them is safe to re-run.
//!
//! ## What it does not do
//!
//! It does not delete the README. Every link, bookmark and agent instruction in
//! the operator's world points at that filename, so it is rewritten into a
//! three-line signpost instead. It does not touch `artifacts/` or `workspace/`:
//! those are the two subtrees that are not markdown, and the flat contract keeps
//! both (AD-119).

use crate::notes::frontmatter::{FieldValue, Frontmatter};
use crate::notes::naming::slug;
use crate::sessions::model::{log_entries, README};
use crate::sessions::plan::{Plan, PlanStep};
use crate::sessions::shape::{shape, KindTag, Shape, ABOUT, AGENTS};

/// One markdown file the migration carries into the pool, as the shell read it.
///
/// `rel` is session-relative with `/` separators — `refs/inputs.md`. The kind it
/// gains is decided from that prefix, which is the *last* time in this codebase
/// that a file's location decides what it is: the whole point of the flat
/// contract is that after this runs, the tag says it instead.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MigrateFile {
    pub rel: String,
    pub text: String,
}

/// Everything the compiler needs, all of it read by the shell.
///
/// Pure in, pure out: no clock, no id generator, no filesystem. The ULIDs come
/// in from the caller ([`crate::sessions::plan`] has the same shape for the same
/// reason) so that a journal replays the ids it recorded rather than minting new
/// ones on resume — which would leave two files claiming to be the same log.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MigrateInput {
    /// The session, zone-relative: `active/2026-08-10-keeper`.
    pub session: String,
    /// The session directory's own entry names — what decides the shape.
    pub top_level: Vec<String>,
    /// `README.md`'s current bytes. Empty when there is none.
    pub readme: String,
    /// Every `.md` under `refs/` and `prompts/`, in reading order.
    pub carried: Vec<MigrateFile>,
    /// One ULID per file that needs one, in the order [`id_count`] counts them.
    /// Short lists are survivable — see [`id_count`].
    pub ids: Vec<String>,
    /// Today, `YYYY-MM-DD`, for the `created` stamp on files with no date of
    /// their own.
    pub today: String,
}

/// How many ULIDs [`compile_migrate`] will consume for this input.
///
/// The shell calls this, generates that many with `sync_ipc::new_ulid()`, and
/// puts them in [`MigrateInput::ids`]. Splitting the count out keeps the
/// compiler pure without making it a two-phase protocol: it is one cheap,
/// testable function over the same input, and a test asserts it agrees with what
/// the compiler actually takes.
///
/// A short list is not a panic. Files past the end are written **without** an
/// `id`, which degrades them to path identity — exactly what
/// [`crate::sessions::pool::PoolEntry::id`] already models for every file keeper
/// did not author. Losing a stable id is a real cost; losing the migration
/// because the shell miscounted would be a worse one.
pub fn id_count(input: &MigrateInput) -> usize {
    let (_, body_at) = Frontmatter::parse(&input.readme);
    1 + log_entries(&input.readme[body_at..]).len() + input.carried.len()
}

/// Compile the migration, or `None` when this session is already flat.
///
/// Idempotence is stated in the return type rather than left to the executor:
/// re-running the verb on a migrated session is not a no-op plan, it is *no
/// plan*, so the UI can grey the button out from the same fact the compiler uses.
///
/// `TrashDir` is emitted **only for directories present in
/// [`MigrateInput::top_level`]**. The executor's `TrashDir` is idempotent on
/// replay (source gone, trash present → `Ok`) but errors when the source never
/// existed at all, so the guard has to live at compile time. A session with no
/// `refs/` is not a broken migration; it is a session nobody put a reference in.
pub fn compile_migrate(input: &MigrateInput) -> Option<Plan> {
    if shape(&input.top_level) == Shape::Flat {
        return None;
    }

    let session = input.session.as_str();
    let at = |rel: &str| format!("{session}/{rel}");
    let (fm, body_at) = Frontmatter::parse(&input.readme);
    let header = &input.readme[..body_at];
    let body = &input.readme[body_at..];

    let mut ids = input.ids.iter();
    let mut next_id = || ids.next().map(String::as_str).unwrap_or("");

    // Reserved before anything is named, so no carried file can land on one of
    // the three structural names and quietly replace it.
    let mut taken: Vec<String> = vec![
        ABOUT.to_owned(),
        AGENTS.to_owned(),
        README.to_owned(),
        "artifacts".to_owned(),
        "workspace".to_owned(),
    ];
    let mut steps = Vec::new();

    // 1. The record. The README's own frontmatter travels whole — `id`,
    //    `created`, `pinned` and the `keeper:` lineage map are the session's
    //    identity, and a migration that dropped them would silently unpin the
    //    session and orphan both ends of every continuation (AD-112).
    let title = crate::notes::naming::title_from_body(body);
    let about_id = match fm.as_string("id") {
        Some(existing) if !existing.trim().is_empty() => existing.to_owned(),
        _ => next_id().to_owned(),
    };
    let record_body = without_log_section(body);
    let mut about = format!("{header}{record_body}");
    if !about_id.is_empty() {
        about = Frontmatter::set_in(&about, "id", FieldValue::Str(about_id));
    }
    if fm.as_string("created").is_none() {
        about = Frontmatter::set_in(&about, "created", FieldValue::Str(input.today.clone()));
    }
    about = with_tag(&about, KindTag::About);
    steps.push(PlanStep::WriteFile {
        path: at(ABOUT),
        content: about,
    });

    // 2. One file per log entry. The README recorded a date and never a time,
    //    so the minute is synthesised from the entry's position *within its
    //    date* — `0000`, `0001`, … — because the filename is what the pool sorts
    //    by, and a run of identical stamps would let two sittings from one day
    //    reshuffle against the order the operator wrote them in.
    let entries = log_entries(body);
    let mut nth_on_date: Vec<(String, usize)> = Vec::new();
    for (date, entry_title, entry_body) in &entries {
        let index = match nth_on_date.iter_mut().find(|(d, _)| d == date) {
            Some((_, count)) => {
                *count += 1;
                *count
            }
            None => {
                nth_on_date.push((date.clone(), 0));
                0
            }
        };
        let name = unique(
            &format!("{date}-{}-{}.md", clock(index), slug(entry_title)),
            &mut taken,
        );
        steps.push(PlanStep::WriteFile {
            path: at(&name),
            content: log_file(next_id(), date, entry_title, entry_body),
        });
    }

    // 3. The carried pointers and prompts, hoisted to the root with one tag
    //    added and every other byte left alone. This is the step that makes the
    //    flat shape a *rename plus a tag* rather than a rewrite: the operator's
    //    prose survives verbatim, which is the only reason it is safe to run
    //    against a live drive.
    for file in &input.carried {
        let Some(kind) = carried_kind(&file.rel) else {
            continue;
        };
        let stem = file.rel.rsplit('/').next().unwrap_or(&file.rel);
        let name = unique(stem, &mut taken);
        steps.push(PlanStep::WriteFile {
            path: at(&name),
            content: stamped(&file.text, next_id(), kind),
        });
    }

    // 4. The shape flip. Every file the flat reader needs already exists; from
    //    the byte this lands, the session reads as flat.
    steps.push(PlanStep::WriteFile {
        path: at(AGENTS),
        content: agents_md(&title),
    });

    // 5. The signpost, guarded on the README's current length so a concurrent
    //    agent write refuses the migration instead of losing an edit.
    //
    //    It is tagged `ref`, which is not a dodge: a redirect is a pointer at
    //    something that lives elsewhere, which is this codebase's definition of
    //    a reference. Leaving it untagged would put one permanent row in
    //    `unfiled` on every session that ever migrated, and `unfiled` is worth
    //    more than that — with the stub filed, a non-empty `unfiled` means
    //    exactly one thing: a migration that stopped between steps 4 and 5 and
    //    left the *original* README behind. The signal PR 1 wanted survives,
    //    and it now fires only when something is actually wrong.
    steps.push(PlanStep::GuardedWrite {
        path: at(README),
        expect_len: input.readme.len(),
        content: readme_stub(&title),
    });

    // 6. Irreversible, and therefore last.
    for dir in ["refs", "prompts"] {
        if input.top_level.iter().any(|entry| entry == dir) {
            steps.push(PlanStep::TrashDir {
                path: at(dir),
                trash_key: format!("{}-{dir}", session.replace('/', "-")),
            });
        }
    }

    Some(Plan {
        verb: "migrate".to_owned(),
        session: input.session.clone(),
        steps,
    })
}

/// `HHMM` for the nth entry of a date, counting from midnight in minutes.
///
/// Total for the first 1440 entries of one day and monotonic throughout, which
/// is all the property the sort needs.
fn clock(index: usize) -> String {
    let minutes = index.min(24 * 60 - 1);
    format!("{:02}{:02}", minutes / 60, minutes % 60)
}

/// A name not already in `taken`, appending `-2`, `-3`, … until it is free, and
/// recording the answer.
///
/// Case-insensitive for the reason [`crate::notes::naming::note_filename`] is:
/// APFS and NTFS fold case, so two files differing only in case are one file on
/// the machine the operator is looking at, and finding that out during a sync
/// push is far worse than finding it out here.
fn unique(name: &str, taken: &mut Vec<String>) -> String {
    let (stem, ext) = match name.rsplit_once('.') {
        Some((stem, ext)) => (stem, format!(".{ext}")),
        None => (name, String::new()),
    };
    let mut candidate = name.to_owned();
    let mut n = 1;
    while taken.iter().any(|t| t.eq_ignore_ascii_case(&candidate)) {
        n += 1;
        candidate = format!("{stem}-{n}{ext}");
    }
    taken.push(candidate.clone());
    candidate
}

/// Which kind a carried file gains, from the directory it is leaving.
fn carried_kind(rel: &str) -> Option<KindTag> {
    if rel.starts_with("refs/") {
        Some(KindTag::Ref)
    } else if rel.starts_with("prompts/") {
        Some(KindTag::Prompt)
    } else {
        None
    }
}

/// A body with its `## Log` section cut out, and nothing else touched.
///
/// The section runs from its heading to the next `## ` heading or the end. The
/// blank line that separated it from what follows is consumed with it, so
/// removing the middle section of a record does not leave a double gap where it
/// used to be.
fn without_log_section(body: &str) -> String {
    let mut out = String::with_capacity(body.len());
    let mut in_log = false;
    for line in body.split_inclusive('\n') {
        let trimmed = line.trim_end_matches(['\n', '\r']).trim();
        if trimmed.starts_with("## ") {
            in_log = trimmed == "## Log";
        }
        if !in_log {
            out.push_str(line);
        }
    }
    // The record now ends where the Log used to begin; one trailing newline is
    // the shape every other writer here leaves behind.
    while out.ends_with("\n\n") {
        out.pop();
    }
    if !out.is_empty() && !out.ends_with('\n') {
        out.push('\n');
    }
    out
}

/// `source` with `kind`'s tag added to its `tags` list, every other byte the
/// same (FR-121). Already-tagged files are returned untouched.
fn with_tag(source: &str, kind: KindTag) -> String {
    let (fm, _) = Frontmatter::parse(source);
    let mut tags = fm.as_list("tags").unwrap_or_default();
    if tags.iter().any(|tag| tag == kind.as_str()) {
        return source.to_owned();
    }
    tags.push(kind.as_str().to_owned());
    Frontmatter::set_in(
        source,
        "tags",
        FieldValue::List(tags.into_iter().map(FieldValue::Str).collect()),
    )
}

/// A carried file with its kind tag and, when it has none of its own, an `id`.
fn stamped(source: &str, id: &str, kind: KindTag) -> String {
    let (fm, _) = Frontmatter::parse(source);
    let mut out = source.to_owned();
    if fm.as_string("id").is_none() && !id.is_empty() {
        out = Frontmatter::set_in(&out, "id", FieldValue::Str(id.to_owned()));
    }
    with_tag(&out, kind)
}

/// One migrated log entry as a file.
fn log_file(id: &str, date: &str, title: &str, body: &str) -> String {
    let mut pairs = Vec::new();
    if !id.is_empty() {
        pairs.push(("id".to_owned(), FieldValue::Str(id.to_owned())));
    }
    pairs.push(("created".to_owned(), FieldValue::Str(date.to_owned())));
    pairs.push((
        "tags".to_owned(),
        FieldValue::List(vec![FieldValue::Str(KindTag::Log.as_str().to_owned())]),
    ));
    let mut out = Frontmatter::serialise_new(&pairs);
    if title.is_empty() {
        // A heading the operator never wrote is not invented here. The entry
        // keeps whatever prose it had, and the pool falls back to the filename
        // — which carries the date, so the sitting is still identifiable.
        out.push_str(body);
    } else {
        out.push_str(&format!("# {title}\n"));
        if !body.is_empty() {
            out.push('\n');
            out.push_str(body);
        }
    }
    if !out.ends_with('\n') {
        out.push('\n');
    }
    out
}

/// The signpost the README becomes: a pointer, tagged as one.
fn readme_stub(title: &str) -> String {
    let heading = if title.is_empty() { "Session" } else { title };
    format!(
        "---\ntags: [ref]\n---\n\
         # {heading}\n\n\
         This session follows the flat contract: the record moved to \
         [about.md](about.md), and every other file says what it is in its own \
         frontmatter `tags:`. Read [AGENTS.md](AGENTS.md) first.\n"
    )
}

/// The navigation file: how to read a flat session, written for whoever — or
/// whatever — is handed the folder.
///
/// This is the mitigation for the flat contract's one real cost. A folder of
/// undifferentiated markdown is opaque to Finder, to `ls`, and to an agent given
/// nothing but a path; a file that states the convention makes it legible to all
/// three. It is written in the zone's own voice — imperative, second person,
/// reasons attached to rules — because the audience is someone about to change
/// things, and a rule without its reason gets optimised away.
///
/// Public because [`crate::sessions`]' template writes the same text for a new
/// session: one contract, stated once.
pub fn agents_md(title: &str) -> String {
    let heading = if title.is_empty() {
        "this session"
    } else {
        title
    };
    format!(
        "---\ntags: [about]\n---\n\
         # How to work in {heading}\n\n\
         This folder is one flat pool of markdown. Every `.md` file here says what it is in \
         its own frontmatter `tags:` — there are no per-kind subfolders, so **read the tags, \
         not the paths**.\n\n\
         ## Start here\n\n\
         1. `about.md` — what this session is for, what was decided, and the promote table.\n\
         2. Files tagged `task` — what is in flight. Each carries `status:` \
         (`in-preparation`, `todo`, `done`, `deferred`) and `order:`.\n\
         3. Files tagged `log`, newest first — they are named \
         `YYYY-MM-DD-HHMM-slug.md`, so the newest sorts last in `ls` and first in keeper.\n\n\
         ## The tags\n\n\
         | tag | what it marks |\n\
         | --- | --- |\n\
         | `about` | the session's record — normally one file |\n\
         | `log` | one sitting: what happened, what changed |\n\
         | `task` | a unit of work, with `status:` and `order:` |\n\
         | `prompt` | reusable text worth keeping |\n\
         | `ref` | a pointer at something that lives elsewhere |\n\n\
         A file may carry any other tags too; these five are only the ones this folder's \
         views collect.\n\n\
         ## The two directories\n\n\
         - `artifacts/` — output worth keeping. Versioned and synced. Put finished things here.\n\
         - `workspace/` — scratch. **Not versioned, not backed up, and it dies with the \
         session.** Nothing in it is safe. Promote anything you want to keep into \
         `artifacts/` and record the move in `about.md`'s promote table.\n\n\
         ## When you finish a sitting\n\n\
         Write a new `log` file — `YYYY-MM-DD-HHMM-slug.md`, tagged `log` — saying what you \
         did and what the next person needs to know. Update the `status:` of any task you \
         moved. A sitting that ends without a log is a sitting nobody else can pick up.\n"
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sessions::pool::{read_pool, PoolFile};
    use crate::sessions::shape::TaskStatus;

    /// The live zone's own README, byte for byte, including the two hazards it
    /// carries: a half-written entry whose title is empty and which is followed
    /// *immediately* by the next `##` heading with no blank line, and a promote
    /// table that is header rows only.
    const LIVE: &str = "# keeper — rolling work session\n\n\
- **Date:** 2026-08-10\n\
- **Tool/model:** Claude Code (Opus 5)\n\
- **Goal:** keeper the app and tgdrive the data\n\n\
## Summary\n\n\
State as of opening. Two tracks.\n\n\
## Log\n\n\
### 2026-08-10 — opened\n\n\
Set up the zone.\n\n\
### 2026-08-11 — shipped 0.6.5\n\n\
Release drafted; DMG attached.\n\n\
### 2026-08-12 — \n\
## Promote\n\n\
| workspace | → artifacts | note |\n\
| --------- | ----------- | ---- |\n";

    fn input(readme: &str, top_level: &[&str], carried: &[(&str, &str)]) -> MigrateInput {
        let mut input = MigrateInput {
            session: "active/2026-08-10-keeper".to_owned(),
            top_level: top_level.iter().map(|s| (*s).to_owned()).collect(),
            readme: readme.to_owned(),
            carried: carried
                .iter()
                .map(|(rel, text)| MigrateFile {
                    rel: (*rel).to_owned(),
                    text: (*text).to_owned(),
                })
                .collect(),
            ids: Vec::new(),
            today: "2026-08-13".to_owned(),
        };
        input.ids = (0..id_count(&input))
            .map(|n| format!("01J5{:022}", n))
            .collect();
        input
    }

    fn writes(plan: &Plan) -> Vec<(&str, &str)> {
        plan.steps
            .iter()
            .filter_map(|step| match step {
                PlanStep::WriteFile { path, content } => Some((path.as_str(), content.as_str())),
                PlanStep::GuardedWrite { path, content, .. } => {
                    Some((path.as_str(), content.as_str()))
                }
                _ => None,
            })
            .collect()
    }

    /// Migrating a migrated session is not an empty plan, it is no plan — so
    /// the button greys out from the same fact the compiler uses.
    #[test]
    fn an_already_flat_session_compiles_to_nothing() {
        assert!(compile_migrate(&input(LIVE, &["AGENTS.md", "artifacts"], &[])).is_none());
        assert!(compile_migrate(&input(LIVE, &["about.md"], &[])).is_none());
        assert!(compile_migrate(&input(LIVE, &["README.md", "refs"], &[])).is_some());
    }

    /// The shape flip lands after every file the flat reader needs and before
    /// anything is removed. This is the whole crash-safety argument: there is
    /// no instant at which the session reads as flat and has no logs.
    #[test]
    fn agents_md_is_written_after_the_pool_and_before_any_removal() {
        let plan = compile_migrate(&input(
            LIVE,
            &["README.md", "refs", "prompts"],
            &[("refs/inputs.md", "# Inputs\n")],
        ))
        .expect("a folder session migrates");

        let position = |needle: &str| {
            plan.steps
                .iter()
                .position(|step| match step {
                    PlanStep::WriteFile { path, .. } | PlanStep::GuardedWrite { path, .. } => {
                        path.ends_with(needle)
                    }
                    PlanStep::TrashDir { path, .. } => path.ends_with(needle),
                    _ => false,
                })
                .unwrap_or_else(|| panic!("no step for {needle}"))
        };
        assert!(position("/about.md") < position("/AGENTS.md"));
        assert!(position("2026-08-10-0000-opened.md") < position("/AGENTS.md"));
        assert!(position("/inputs.md") < position("/AGENTS.md"));
        assert!(position("/AGENTS.md") < position("/README.md"));
        assert!(position("/README.md") < position("/refs"));

        // Irreversible last, both of them, after every write.
        let first_trash = plan
            .steps
            .iter()
            .position(|step| matches!(step, PlanStep::TrashDir { .. }))
            .expect("a trash step");
        assert!(
            plan.steps[first_trash..]
                .iter()
                .all(|step| matches!(step, PlanStep::TrashDir { .. })),
            "nothing is written after the point of no return"
        );
    }

    /// The live README's three hazards, all survived: the empty-title entry
    /// becomes a real file rather than being dropped, the header-only promote
    /// table is copied verbatim, and a README with no frontmatter at all still
    /// produces a well-formed `about.md`.
    #[test]
    fn the_live_readme_migrates_without_losing_a_byte_that_matters() {
        let plan = compile_migrate(&input(LIVE, &["README.md", "refs", "prompts"], &[]))
            .expect("migrates");
        let files = writes(&plan);
        let find = |suffix: &str| {
            files
                .iter()
                .find(|(path, _)| path.ends_with(suffix))
                .unwrap_or_else(|| panic!("no write for {suffix}"))
                .1
        };

        let about = find("/about.md");
        assert!(about.contains("## Summary"), "the record survives");
        assert!(about.contains("State as of opening. Two tracks."));
        assert!(
            about.contains("| workspace | → artifacts | note |"),
            "an empty promote table is the zone's scaffold, not noise"
        );
        assert!(
            !about.contains("## Log"),
            "the log left the record: {about}"
        );
        assert!(!about.contains("shipped 0.6.5"), "and so did its entries");
        assert!(
            about.contains("- **Goal:** keeper the app and tgdrive the data"),
            "the header bullets are prose and travel whole"
        );

        // The half-written entry is a file, not a casualty.
        let untitled = find("2026-08-12-0000-untitled.md");
        assert!(untitled.contains("tags:"));
        assert!(
            !untitled.contains("## Promote"),
            "the entry stops at the next section: {untitled}"
        );

        assert!(find("2026-08-10-0000-opened.md").contains("Set up the zone."));
        // `0.6.5` folds to `0-6-5`: the slug keeps digits and turns every other
        // character into one separator, which is `slug_stem`'s rule and not this
        // module's to reinterpret.
        assert!(find("2026-08-11-0000-shipped-0-6-5.md").contains("Release drafted; DMG attached."));
    }

    /// A README with no frontmatter — which is what the live zone has — gets a
    /// fresh block, an id and a `created`, and the body is still the body.
    #[test]
    fn a_record_with_no_frontmatter_gains_one_rather_than_being_left_bare() {
        let plan = compile_migrate(&input(LIVE, &["README.md"], &[])).expect("migrates");
        let about = writes(&plan)
            .into_iter()
            .find(|(path, _)| path.ends_with("/about.md"))
            .expect("about")
            .1;
        let (fm, body_at) = Frontmatter::parse(about);
        assert!(fm.unparsed().is_none(), "the write parses clean: {about}");
        assert!(fm.as_string("id").is_some(), "authored, so stamped");
        assert_eq!(fm.as_string("created"), Some("2026-08-13"));
        assert_eq!(fm.as_list("tags"), Some(vec!["about".to_owned()]));
        assert!(about[body_at..].starts_with("# keeper — rolling work session"));
    }

    /// The session's identity is not a thing a migration gets to change: an
    /// existing id, `pinned`, and both lineage directions move to `about.md`
    /// verbatim, because the board reads the record and would otherwise
    /// silently unpin the session and orphan every continuation (AD-112).
    #[test]
    fn identity_pins_and_lineage_travel_into_the_record() {
        let readme = "---\nid: 01J5AAAAAAAAAAAAAAAAAAAAAA\ncreated: 2026-08-10\npinned: true\n\
                      keeper:\n  session-continued-by: [01J6BBBBBBBBBBBBBBBBBBBBBB]\n---\n\
                      # keeper\n\n## Log\n\n### 2026-08-10 — opened\n\nx\n";
        let plan = compile_migrate(&input(readme, &["README.md"], &[])).expect("migrates");
        let about = writes(&plan)
            .into_iter()
            .find(|(path, _)| path.ends_with("/about.md"))
            .expect("about")
            .1;
        let (fm, _) = Frontmatter::parse(about);
        assert_eq!(
            fm.as_string("id"),
            Some("01J5AAAAAAAAAAAAAAAAAAAAAA"),
            "the id is kept, never reminted — it is the session"
        );
        assert_eq!(fm.as_bool("pinned"), Some(true));
        assert_eq!(
            fm.as_string("created"),
            Some("2026-08-10"),
            "a stated creation date is not overwritten with today"
        );
        assert_eq!(
            crate::sessions::model::lineage(&fm).continued_by,
            vec!["01J6BBBBBBBBBBBBBBBBBBBBBB"]
        );
    }

    /// A carried file gains one tag and loses nothing — the property that makes
    /// this safe to run against a live drive (FR-121).
    #[test]
    fn carried_files_gain_a_tag_and_keep_every_other_byte() {
        let plan = compile_migrate(&input(
            "# s\n",
            &["README.md", "refs", "prompts"],
            &[
                (
                    "refs/inputs.md",
                    "---\ntitle: Inputs\nsource: interview\n---\n# Inputs\n\nSee [[Vault as a lens]].\n",
                ),
                ("prompts/01-scope.md", "# Scope\n\nYou are a…\n"),
            ],
        ))
        .expect("migrates");
        let files = writes(&plan);
        let inputs = files
            .iter()
            .find(|(path, _)| path.ends_with("/inputs.md"))
            .expect("hoisted to the root")
            .1;
        let (fm, body_at) = Frontmatter::parse(inputs);
        assert_eq!(fm.as_list("tags"), Some(vec!["ref".to_owned()]));
        assert_eq!(
            fm.as_string("source"),
            Some("interview"),
            "siblings survive"
        );
        assert_eq!(
            &inputs[body_at..],
            "# Inputs\n\nSee [[Vault as a lens]].\n",
            "the prose is byte-identical"
        );

        let (scope_path, scope) = files
            .iter()
            .find(|(path, _)| path.ends_with("/01-scope.md"))
            .expect("prompts hoist too");
        let (fm, body_at) = Frontmatter::parse(scope);
        assert_eq!(fm.as_list("tags"), Some(vec!["prompt".to_owned()]));
        assert_eq!(
            &scope[body_at..],
            "# Scope\n\nYou are a…\n",
            "a file with no frontmatter gains a block and keeps its body"
        );
        // The claim this used to make was `scope.contains("01-scope.md") || true`
        // — a tautology over the file's CONTENT, where the author meant its
        // PATH. It asserted nothing, and the older clippy on the macOS gate is
        // what found it. The real claim is about the hoisted name: the numbered
        // stem survives, because it is what the prompts space sorts by, and the
        // folder does not, because the flat contract has no `prompts/`.
        assert!(
            !scope_path.contains("/prompts/"),
            "a hoisted prompt leaves the folder behind"
        );
        assert!(
            files.iter().any(|(path, _)| path.ends_with("/01-scope.md")),
            "the NN- prefix survives the hoist, because it is the sort key"
        );
    }

    /// Two files with the same basename in different source directories are two
    /// files at the root, not one file written twice.
    #[test]
    fn a_basename_collision_makes_a_second_name_rather_than_an_overwrite() {
        let plan = compile_migrate(&input(
            "# s\n",
            &["README.md", "refs", "prompts"],
            &[
                ("refs/notes.md", "# a\n"),
                ("prompts/notes.md", "# b\n"),
                ("refs/about.md", "# c\n"),
            ],
        ))
        .expect("migrates");
        let paths: Vec<&str> = writes(&plan).into_iter().map(|(path, _)| path).collect();
        assert!(paths.iter().any(|p| p.ends_with("/notes.md")));
        assert!(paths.iter().any(|p| p.ends_with("/notes-2.md")));
        assert!(
            paths.iter().any(|p| p.ends_with("/about-2.md")),
            "the record's own name is reserved before anything is hoisted: {paths:?}"
        );
        // And nothing is written to the same path twice.
        let mut seen: Vec<&str> = paths.clone();
        seen.sort_unstable();
        seen.dedup();
        assert_eq!(seen.len(), paths.len(), "no path is written twice");
    }

    /// Same-date entries keep the order the operator wrote them in, because the
    /// filename is what the pool sorts by and identical stamps would let them
    /// drift.
    #[test]
    fn several_entries_on_one_date_get_increasing_stamps() {
        let readme =
            "# s\n\n## Log\n\n### 2026-08-10 — first\n\na\n\n### 2026-08-10 — second\n\nb\n\n\
             ### 2026-08-10 — third\n\nc\n";
        let plan = compile_migrate(&input(readme, &["README.md"], &[])).expect("migrates");
        let paths: Vec<&str> = writes(&plan).into_iter().map(|(path, _)| path).collect();
        assert!(paths
            .iter()
            .any(|p| p.ends_with("2026-08-10-0000-first.md")));
        assert!(paths
            .iter()
            .any(|p| p.ends_with("2026-08-10-0001-second.md")));
        assert!(paths
            .iter()
            .any(|p| p.ends_with("2026-08-10-0002-third.md")));
        assert_eq!(clock(0), "0000");
        assert_eq!(clock(59), "0059");
        assert_eq!(clock(60), "0100", "the hour carries");
    }

    /// `TrashDir` errors on a source that never existed, so the guard is here
    /// rather than in the executor: a session with no `refs/` is not broken.
    #[test]
    fn only_directories_that_exist_are_trashed() {
        let plan = compile_migrate(&input("# s\n", &["README.md", "prompts"], &[])).expect("plan");
        let trashed: Vec<&str> = plan
            .steps
            .iter()
            .filter_map(|step| match step {
                PlanStep::TrashDir { path, .. } => Some(path.as_str()),
                _ => None,
            })
            .collect();
        assert_eq!(trashed, ["active/2026-08-10-keeper/prompts"]);

        let neither = compile_migrate(&input("# s\n", &["README.md"], &[])).expect("plan");
        assert!(
            !neither
                .steps
                .iter()
                .any(|step| matches!(step, PlanStep::TrashDir { .. })),
            "nothing to remove is not an error"
        );
    }

    /// The README write is guarded on its current length, so an agent editing
    /// it while the operator migrates refuses rather than losing the edit.
    #[test]
    fn the_readme_becomes_a_guarded_signpost() {
        let plan = compile_migrate(&input(LIVE, &["README.md"], &[])).expect("migrates");
        let Some(PlanStep::GuardedWrite {
            path,
            expect_len,
            content,
        }) = plan
            .steps
            .iter()
            .find(|step| matches!(step, PlanStep::GuardedWrite { .. }))
        else {
            panic!("the README write is guarded");
        };
        assert_eq!(path, "active/2026-08-10-keeper/README.md");
        assert_eq!(*expect_len, LIVE.len());
        assert!(content.contains("about.md"), "it points at the record");
        assert!(content.contains("AGENTS.md"), "and at the navigation file");
        assert!(
            content.contains("keeper — rolling work session"),
            "and keeps the session's name so a link still lands somewhere legible"
        );
    }

    /// `id_count` is the shell's contract, so it has to agree with what the
    /// compiler consumes — asserted, not assumed.
    #[test]
    fn id_count_matches_what_the_plan_consumes() {
        let cases: Vec<MigrateInput> = vec![
            input(LIVE, &["README.md", "refs"], &[("refs/a.md", "# a\n")]),
            input("# s\n", &["README.md"], &[]),
            input(
                "# s\n\n## Log\n\n### 2026-01-01 — x\n\nb\n",
                &["README.md", "prompts"],
                &[("prompts/01.md", "# p\n")],
            ),
        ];
        for case in cases {
            let expected = id_count(&case);
            let plan = compile_migrate(&case).expect("migrates");
            let used = writes(&plan)
                .iter()
                .filter(|(path, _)| !path.ends_with("/AGENTS.md") && !path.ends_with("/README.md"))
                .count();
            assert_eq!(expected, used, "one id per authored pool file");
            // Every one of them actually carries the id it was given.
            for (path, content) in writes(&plan) {
                if path.ends_with("/AGENTS.md") || path.ends_with("/README.md") {
                    continue;
                }
                let (fm, _) = Frontmatter::parse(content);
                assert!(fm.as_string("id").is_some(), "{path} carries an id");
            }
        }
    }

    /// Running out of ids degrades to path identity rather than panicking — the
    /// same degradation the pool already models for a file keeper did not write.
    #[test]
    fn a_short_id_list_degrades_rather_than_failing() {
        let mut short = input(LIVE, &["README.md"], &[]);
        short.ids.truncate(1);
        let plan = compile_migrate(&short).expect("still migrates");
        let files: Vec<(&str, &str)> = writes(&plan)
            .into_iter()
            .filter(|(path, _)| !path.ends_with("/AGENTS.md") && !path.ends_with("/README.md"))
            .collect();
        let with_id = files
            .iter()
            .filter(|(_, content)| Frontmatter::parse(content).0.as_string("id").is_some())
            .count();
        assert_eq!(with_id, 1, "the ids that existed were used");
        for (path, content) in &files {
            let (fm, _) = Frontmatter::parse(content);
            assert!(fm.unparsed().is_none(), "{path} still parses clean");
        }
    }

    /// The end-to-end property: run the plan's writes into a pool and the
    /// reader sees the session the operator had — one record, three sittings
    /// newest-first, the pointer filed as a reference.
    #[test]
    fn the_migrated_pool_reads_back_as_the_session_it_was() {
        let plan = compile_migrate(&input(
            LIVE,
            &["README.md", "refs", "prompts"],
            &[
                ("refs/inputs.md", "# Inputs\n"),
                ("prompts/01-scope.md", "# Scope\n"),
            ],
        ))
        .expect("migrates");

        let written: Vec<(String, String)> = writes(&plan)
            .into_iter()
            .map(|(path, content)| {
                let rel = path
                    .strip_prefix("active/2026-08-10-keeper/")
                    .expect("session-relative")
                    .to_owned();
                (rel, content.to_owned())
            })
            .collect();
        let files: Vec<PoolFile<'_>> = written
            .iter()
            .map(|(rel, text)| PoolFile { rel, text })
            .collect();
        let pool = read_pool(&files);

        // Two `about` files, and that is the right answer rather than a leak:
        // the record and the navigation file are both orienting documents, and
        // the About space is where someone opening this session should find
        // both. `Pool::about` is a list for exactly this reason.
        assert_eq!(
            pool.about
                .iter()
                .map(|e| e.rel.as_str())
                .collect::<Vec<_>>(),
            ["about.md", "AGENTS.md"],
            "the record before the navigation file, which is the order someone \
             opening the session wants and which the case-folded name sort gives \
             for free"
        );
        assert_eq!(pool.logs.len(), 3, "every sitting became a file");
        assert_eq!(
            pool.logs[0].date, "2026-08-12",
            "newest first, including the half-written one"
        );
        assert_eq!(pool.logs[2].title, "opened");
        assert_eq!(
            pool.refs.iter().map(|e| e.rel.as_str()).collect::<Vec<_>>(),
            ["inputs.md", "README.md"],
            "the signpost is filed as the pointer it is, and sorts by folded name"
        );
        assert_eq!(pool.prompts.len(), 1);
        assert!(pool.tasks.is_empty(), "a folder session had no board");
        assert!(
            pool.unfiled.is_empty(),
            "a completed migration files everything it wrote: {:?}",
            pool.unfiled.iter().map(|e| &e.rel).collect::<Vec<_>>()
        );
        for entry in pool.logs.iter().chain(&pool.about) {
            assert!(!entry.unparsed, "{} parses clean", entry.rel);
        }
    }

    /// The navigation file says the things the folder cannot say for itself,
    /// and names the two directories with the one fact that actually costs
    /// people work.
    #[test]
    fn the_agents_file_states_the_contract_it_exists_to_state() {
        let text = agents_md("keeper — rolling work session");
        assert!(text.contains("keeper — rolling work session"));
        for tag in [
            KindTag::About,
            KindTag::Log,
            KindTag::Prompt,
            KindTag::Ref,
            KindTag::Task,
        ] {
            assert!(text.contains(&format!("`{}`", tag.as_str())), "{tag:?}");
        }
        for status in [
            TaskStatus::InPreparation,
            TaskStatus::Todo,
            TaskStatus::Done,
            TaskStatus::Deferred,
        ] {
            assert!(text.contains(status.as_str()), "{status:?}");
        }
        assert!(text.contains("artifacts/"));
        assert!(
            text.contains("dies with the session"),
            "the workspace warning is the one line that saves real work"
        );
        // It is itself a pool member, and it declares a kind rather than
        // landing in `unfiled` on every migrated session forever.
        let (fm, _) = Frontmatter::parse(&text);
        assert_eq!(fm.as_list("tags"), Some(vec!["about".to_owned()]));
    }

    /// Cutting the Log out of the middle of a record does not leave a hole
    /// where it used to be.
    #[test]
    fn removing_the_log_section_leaves_the_record_well_formed() {
        let body = "# s\n\n## Summary\n\ntext\n\n## Log\n\n### 2026-01-01 — x\n\nb\n\n## Promote\n\n| a |\n";
        let out = without_log_section(body);
        assert_eq!(out, "# s\n\n## Summary\n\ntext\n\n## Promote\n\n| a |\n");

        // A record whose Log is last ends cleanly rather than with a dangling gap.
        assert_eq!(
            without_log_section("# s\n\n## Log\n\n### 2026-01-01 — x\n\nb\n"),
            "# s\n"
        );
        // No Log at all is the identity.
        assert_eq!(
            without_log_section("# s\n\n## Summary\n"),
            "# s\n\n## Summary\n"
        );
    }
}
