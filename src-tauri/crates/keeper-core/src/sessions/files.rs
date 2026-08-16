//! Making and unmaking one file inside a session (FR-262).
//!
//! The flat contract's promise is that a session is a pool of markdown you can
//! add to — so the surface that shows the pool has to be able to grow it. This
//! module decides three things and performs none of them (AD-108): what a new
//! file may be *called*, what it may be *called about* (its containment rules),
//! and what bytes it starts life with.
//!
//! **Three extensions, and the set is closed.** `.md` because a session is
//! markdown; `.csv` and `.json` because the two things an agent produces beside
//! prose are a table and a payload, and both are text a person can read in a
//! diff. Everything else that belongs in a session arrives by being *put* there
//! — a recording, a screenshot, a built binary — and arrives in `artifacts/`,
//! where a create-file button was never the way in. An open set here would mean
//! keeper offering to author a `.png` it has no bytes for.
//!
//! **Two named verbs on top of the general one.** A log and a prompt are the
//! two files a working session grows constantly, and both have a *correct* name
//! (`YYYY-MM-DD-HHMM-slug.md`) and a *correct* tag that decide whether the
//! zone's spaces will ever list them. Leaving that to whoever is typing means a
//! log file called `notes.md` that no space selects and nobody can find — the
//! flat shape's one real failure mode, made one keystroke wide. So keeper spells
//! those two, and [`new_named`] is what the general button falls back to.
//!
//! **keeper stamps what keeper authors.** [`super::pool::PoolEntry::id`] refuses
//! to mint an id for a file it merely *read*, and that rule is not in tension
//! with this one: a file created here is keeper's own, written this instant, so
//! giving it `id`/`created`/`updated` costs nobody their bytes and buys the file
//! a stable identity that survives a rename. The rule was always "never stamp a
//! file you did not author", and authorship is exactly what this module has.

use std::collections::BTreeSet;

use crate::notes::frontmatter::{FieldValue, Frontmatter};
use crate::notes::naming;
use crate::sessions::plan::{Plan, PlanStep};
use crate::sessions::shape::{KindTag, ABOUT, AGENTS};

/// The `workspace/` fence, spelled session-relative.
///
/// The real fence is `keeper_sync::files_write::WriteScope` and it works on
/// profile-relative subpaths; this is the same refusal asked one scope in, so a
/// plan that would write into scratch is never compiled in the first place. The
/// shell still asks the real one — see [`compile_new`]'s note — because two
/// predicates that must agree should both run, not take turns.
const WORKSPACE: &str = "workspace";

/// What a new file may be. Closed, for the reason in the module header.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NewFileKind {
    Markdown,
    Csv,
    Json,
}

/// Every kind, for a menu that must not go stale when a fourth is added.
pub const NEW_FILE_KINDS: [NewFileKind; 3] =
    [NewFileKind::Markdown, NewFileKind::Csv, NewFileKind::Json];

impl NewFileKind {
    /// The extension, without the dot.
    #[must_use]
    pub fn ext(self) -> &'static str {
        match self {
            NewFileKind::Markdown => "md",
            NewFileKind::Csv => "csv",
            NewFileKind::Json => "json",
        }
    }

    /// The wire spelling — the extension, because that is the word the operator
    /// picked from the menu and there is no second vocabulary to learn.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        self.ext()
    }

    /// Parse the wire spelling. `None` for anything outside the set, which is
    /// what makes an unknown extension a refusal rather than a create.
    #[must_use]
    pub fn parse(raw: &str) -> Option<NewFileKind> {
        match raw
            .trim()
            .trim_start_matches('.')
            .to_ascii_lowercase()
            .as_str()
        {
            "md" | "markdown" => Some(NewFileKind::Markdown),
            "csv" => Some(NewFileKind::Csv),
            "json" => Some(NewFileKind::Json),
            _ => None,
        }
    }
}

/// Everything this module refuses, with the sentence the operator reads.
///
/// Sentences rather than codes: each of these is a thing a person just tried to
/// do, and the only useful answer says what keeper will not do *and why the rule
/// exists*. A `Refused` that said "invalid path" would be a support ticket.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum FileVerbError {
    #[error(
        "{rel} is inside the session's workspace — scratch that is not versioned, not synced, \
         and dies with the session. keeper never writes there; make the file in the session \
         itself, or in artifacts/ if it is output worth keeping."
    )]
    Workspace { rel: String },

    #[error(
        "{rel} is not a path inside this session. A session file is written relative to the \
         session's own folder, and keeper will not follow a path back out of it."
    )]
    Outside { rel: String },

    #[error(
        "keeper creates and deletes .md, .csv and .json files — {rel} is none of those. \
         Anything else belongs in artifacts/, put there by the tool that made it."
    )]
    Extension { rel: String },

    #[error(
        "{rel} is what tells keeper this session is a flat one: deleting it would silently turn \
         the session back into the old folder shape and hide every log behind a section that no \
         longer exists. Rename it in Finder if you really mean to."
    )]
    ShapeFile { rel: String },
}

/// Whether a session-relative path is one this module may write or delete.
///
/// The containment rule, stated once: inside the session, not inside
/// `workspace/`, one of the three extensions, no traversal, no absolute path, no
/// dotfile. `spaces::is_space_path`'s twin and for its reason — the executor's
/// own check only proves a path cannot escape the *zone*, which would happily
/// let a create-file call land in another session's folder.
///
/// Returns the refusal rather than a bool so every caller reports the same
/// sentence; a `bool` here would mean each call site inventing its own.
///
/// # Errors
/// One [`FileVerbError`] per broken rule, in the order above: containment before
/// extension, because "that is not in this session" is the more urgent fact.
pub fn check_rel(rel: &str) -> Result<(), FileVerbError> {
    check_dir(rel)?;
    let ext = rel.rsplit('.').next().unwrap_or_default();
    if !rel.contains('.') || NewFileKind::parse(ext).is_none() {
        return Err(FileVerbError::Extension {
            rel: rel.to_owned(),
        });
    }
    Ok(())
}

/// The same containment rule for a **folder** a new file is going into.
///
/// Split from [`check_rel`] rather than folded into it because the extension
/// rule is the difference: a folder has none, and a `check_rel` that accepted
/// extensionless paths would accept `Makefile` as a file to write. Checking the
/// parent separately also refuses `workspace/` whatever the file is called,
/// instead of relying on the joined path to catch it — the join is the caller's,
/// and a rule that only holds after a caller does the right thing is not a rule.
///
/// # Errors
/// [`FileVerbError::Outside`] for traversal, an absolute path or a dotfolder;
/// [`FileVerbError::Workspace`] for scratch.
pub fn check_dir(rel: &str) -> Result<(), FileVerbError> {
    let owned = || rel.to_owned();
    if rel.is_empty()
        || rel.starts_with('/')
        || rel.contains('\\')
        || rel
            .split('/')
            .any(|part| part.is_empty() || part == "." || part == ".." || part.starts_with('.'))
    {
        return Err(FileVerbError::Outside { rel: owned() });
    }
    if rel == WORKSPACE || rel.starts_with("workspace/") {
        return Err(FileVerbError::Workspace { rel: owned() });
    }
    Ok(())
}

/// [`check_rel`], plus the two names a delete must never touch.
///
/// `AGENTS.md` and `about.md` are not ordinary files: [`super::shape::shape`]
/// reads exactly those two names to decide which contract a session follows, so
/// deleting one flips a flat session back to folder-shaped and every log written
/// as a file becomes invisible behind a `## Log` heading that is not there. The
/// data survives and the session stops rendering — the worst failure shape there
/// is, because nothing looks broken.
///
/// A create has no such rule: naming avoids collisions, so a create can only
/// ever *add* a shape file, and adding one is the direction migration already
/// goes.
///
/// # Errors
/// [`FileVerbError::ShapeFile`] for those two at the session root, or whatever
/// [`check_rel`] refuses.
pub fn check_deletable(rel: &str) -> Result<(), FileVerbError> {
    check_rel(rel)?;
    if rel == AGENTS || rel == ABOUT {
        return Err(FileVerbError::ShapeFile {
            rel: rel.to_owned(),
        });
    }
    Ok(())
}

/// The name a plainly-created file gets: `<slug>.<ext>`, avoiding `taken`.
///
/// **Undated**, unlike a log. Someone who types "budget" for a `.csv` means a
/// file called `budget.csv`, and a date in front of it would be keeper filing
/// something the operator was naming. The clock goes in a filename when the
/// filename's job is to sort — which is the log's job and nothing else's.
///
/// `taken` is compared case-insensitively for [`naming::note_filename`]'s
/// reason: APFS and NTFS fold case, so two names that differ only in case are
/// one file on the machine the operator is looking at.
#[must_use]
pub fn new_named(title: &str, kind: NewFileKind, taken: &BTreeSet<String>) -> String {
    let stem = naming::slug(title);
    let ext = kind.ext();
    let mut candidate = format!("{stem}.{ext}");
    let mut n = 2;
    while taken.iter().any(|t| t.eq_ignore_ascii_case(&candidate)) {
        candidate = format!("{stem}-{n}.{ext}");
        n += 1;
    }
    candidate
}

/// The name a log or prompt gets: `YYYY-MM-DD-HHMM-<slug>.md`, avoiding `taken`.
///
/// The stamp is what [`super::pool::stamp_of`] reads back, and it is in the
/// *filename* rather than only in frontmatter so the folder sorts itself in
/// Finder, in `ls`, and in any tool that has never heard of keeper. That is the
/// whole argument for the flat shape's naming convention, so keeper's own
/// buttons must produce it exactly.
///
/// `date` is `YYYY-MM-DD` and `time` is `HHMM`, both from the shell — the domain
/// has no clock. A collision appends `-2` *after* the slug, keeping the stamp
/// leading and therefore keeping the sort correct.
#[must_use]
pub fn new_stamped(title: &str, date: &str, time: &str, taken: &BTreeSet<String>) -> String {
    let stem = format!("{date}-{time}-{}", naming::slug(title));
    let mut candidate = format!("{stem}.md");
    let mut n = 2;
    while taken.iter().any(|t| t.eq_ignore_ascii_case(&candidate)) {
        candidate = format!("{stem}-{n}.md");
        n += 1;
    }
    candidate
}

/// The bytes a new file starts with.
///
/// `kind` decides the shape and `tag` decides whether any space will ever list
/// it — `None` for a plain markdown file, which lands in the detail's *unfiled*
/// list and is told so. That is the honest outcome: keeper does not know what an
/// operator's new file is, and guessing `log` would file a stray thought as
/// history.
///
/// The two non-markdown kinds:
///
/// - `.json` is `{}` rather than empty. An empty file is not valid JSON, so the
///   first tool to read it fails on a file keeper wrote — a create button whose
///   output is broken on arrival.
/// - `.csv` really is empty. An empty CSV is a valid CSV with no rows, and a
///   guessed header line would be keeper inventing the operator's columns.
#[must_use]
pub fn render_new(
    kind: NewFileKind,
    tag: Option<KindTag>,
    title: &str,
    id: &str,
    now: &str,
) -> String {
    match kind {
        NewFileKind::Csv => String::new(),
        NewFileKind::Json => "{}\n".to_owned(),
        NewFileKind::Markdown => {
            let mut pairs = vec![
                ("id".to_owned(), FieldValue::Str(id.to_owned())),
                ("created".to_owned(), FieldValue::Str(now.to_owned())),
                ("updated".to_owned(), FieldValue::Str(now.to_owned())),
                ("title".to_owned(), FieldValue::Str(title.to_owned())),
            ];
            if let Some(tag) = tag {
                pairs.push((
                    "tags".to_owned(),
                    FieldValue::List(vec![FieldValue::Str(tag.as_str().to_owned())]),
                ));
            }
            // A task starts in `todo`, written rather than defaulted: the board
            // reads `field:status=<v>`, and a task with no `status` key would
            // match no column and sit in a session nobody can see it in. The
            // other kinds carry no status, because they have no columns.
            if tag == Some(KindTag::Task) {
                pairs.push((
                    "status".to_owned(),
                    FieldValue::Str(crate::sessions::shape::TaskStatus::Todo.as_str().to_owned()),
                ));
            }
            format!("{}\n# {title}\n", Frontmatter::serialise_new(&pairs))
        }
    }
}

/// The plan that writes one new file into a session.
///
/// `session` is the session's zone-relative folder (`active/2026-08-14-keeper`)
/// and `rel` is session-relative; the join happens here so no caller composes a
/// zone path (AD-65). `MkDir` leads only when the file is going into a subfolder
/// — the session's own directory exists by definition, and a plan step that
/// re-creates it would be noise in every journal row.
///
/// A plain `WriteFile` rather than a guarded one: the collision was already
/// avoided by [`new_named`] or [`new_stamped`] against a listing read a moment
/// earlier, and the remaining window is two people creating the same filename in
/// the same second on the same drive. `README.md` gets a guard because an *agent*
/// appends to it continuously; nothing appends to a file that does not exist yet.
///
/// The shell asks `WriteScope::in_session_workspace` as well as [`check_rel`]
/// before compiling. Two predicates that must agree should both run: this one
/// keeps the plan honest with no zone knowledge, that one is the fence the whole
/// product is measured against (AD-113).
///
/// # Errors
/// Whatever [`check_rel`] refuses — the plan is not compiled for a path keeper
/// will not write.
pub fn compile_new(session: &str, rel: &str, content: &str) -> Result<Plan, FileVerbError> {
    check_rel(rel)?;
    let mut steps = Vec::new();
    if let Some((parent, _)) = rel.rsplit_once('/') {
        steps.push(PlanStep::MkDir {
            path: format!("{session}/{parent}"),
        });
    }
    steps.push(PlanStep::WriteFile {
        path: format!("{session}/{rel}"),
        content: content.to_owned(),
    });
    Ok(Plan {
        verb: "file-new".to_owned(),
        session: session.to_owned(),
        steps,
    })
}

/// The plan that removes one file from a session: a trash move, recoverable.
///
/// `spaces::compile_delete`'s twin, and for the same reason it is a
/// [`PlanStep::TrashFile`] and not an unlink: a file in a session is something
/// somebody wrote, and a delete button that erases bytes is a delete button
/// nobody presses without making a copy first.
///
/// The whole plan is the irreversible step, which AD-111 puts last and here
/// makes the only one.
///
/// # Errors
/// Whatever [`check_deletable`] refuses — including the two files whose deletion
/// would change the session's shape.
pub fn compile_delete(session: &str, rel: &str, trash_key: &str) -> Result<Plan, FileVerbError> {
    check_deletable(rel)?;
    Ok(Plan {
        verb: "file-delete".to_owned(),
        session: session.to_owned(),
        steps: vec![PlanStep::TrashFile {
            path: format!("{session}/{rel}"),
            trash_key: trash_key.to_owned(),
        }],
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn taken(names: &[&str]) -> BTreeSet<String> {
        names.iter().map(|n| (*n).to_owned()).collect()
    }

    #[test]
    fn the_extension_set_is_closed_and_spelled_one_way() {
        assert_eq!(NewFileKind::parse("md"), Some(NewFileKind::Markdown));
        assert_eq!(NewFileKind::parse(".MD"), Some(NewFileKind::Markdown));
        assert_eq!(NewFileKind::parse("markdown"), Some(NewFileKind::Markdown));
        assert_eq!(NewFileKind::parse("csv"), Some(NewFileKind::Csv));
        assert_eq!(NewFileKind::parse("json"), Some(NewFileKind::Json));
        // The refusals that matter: an executable and an image are exactly what
        // a create-file button must not offer to author.
        assert_eq!(NewFileKind::parse("png"), None);
        assert_eq!(NewFileKind::parse("sh"), None);
        assert_eq!(NewFileKind::parse(""), None);
    }

    /// The fence, asked one scope in from where it is enforced (AD-113).
    #[test]
    fn nothing_is_written_into_the_workspace() {
        assert_eq!(
            check_rel("workspace/iter-3.md"),
            Err(FileVerbError::Workspace {
                rel: "workspace/iter-3.md".to_owned()
            })
        );
        assert!(matches!(
            check_rel("workspace"),
            Err(FileVerbError::Workspace { .. })
        ));
        // A file merely *named* like the workspace is not in it.
        assert!(check_rel("workspace-notes.md").is_ok());
        // artifacts/ is the opposite case: promoted output, versioned, and the
        // place the workspace refusal itself points at.
        assert!(check_rel("artifacts/release-notes.md").is_ok());
    }

    #[test]
    fn a_path_cannot_walk_out_of_the_session() {
        for rel in [
            "../other-session/about.md",
            "/etc/passwd.md",
            "a/../../b.md",
            "sub//deep.md",
            ".hidden.md",
            "",
        ] {
            assert!(
                matches!(check_rel(rel), Err(FileVerbError::Outside { .. })),
                "{rel} must not be writable"
            );
        }
    }

    /// The parent folder is checked as a path in its own right, so `workspace/`
    /// is refused whatever the file inside it would have been called — a rule
    /// that only held after the caller joined correctly would not be a rule.
    #[test]
    fn a_folder_is_checked_without_an_extension_rule() {
        assert!(check_dir("artifacts").is_ok());
        assert!(check_dir("artifacts/2026").is_ok());
        assert!(
            check_rel("artifacts").is_err(),
            "a folder is not a file this module writes"
        );
        assert!(matches!(
            check_dir("workspace"),
            Err(FileVerbError::Workspace { .. })
        ));
        assert!(matches!(
            check_dir("workspace/scratch"),
            Err(FileVerbError::Workspace { .. })
        ));
        assert!(matches!(
            check_dir("../elsewhere"),
            Err(FileVerbError::Outside { .. })
        ));
    }

    #[test]
    fn only_the_three_text_kinds_are_writable() {
        assert!(check_rel("notes.md").is_ok());
        assert!(check_rel("data.csv").is_ok());
        assert!(check_rel("payload.json").is_ok());
        assert!(matches!(
            check_rel("shot.png"),
            Err(FileVerbError::Extension { .. })
        ));
        assert!(matches!(
            check_rel("Makefile"),
            Err(FileVerbError::Extension { .. })
        ));
    }

    /// The sharp one: `shape()` keys on these two names, so deleting either
    /// turns a flat session back into a folder-shaped one and hides every log.
    #[test]
    fn the_two_files_that_decide_the_shape_cannot_be_deleted() {
        for rel in [AGENTS, ABOUT] {
            assert!(
                matches!(check_deletable(rel), Err(FileVerbError::ShapeFile { .. })),
                "{rel} decides the shape and must survive a delete button"
            );
        }
        // Only at the root, and only those two: a file that merely mentions them
        // is an ordinary file.
        assert!(check_deletable("artifacts/about.md").is_ok());
        assert!(check_deletable("about-the-plan.md").is_ok());
        // And creating one is fine — a create can only add a shape file, which
        // is the direction migration already goes.
        assert!(check_rel(AGENTS).is_ok());
    }

    #[test]
    fn a_plain_name_is_undated_and_dodges_what_is_there() {
        let names = taken(&["budget.csv"]);
        assert_eq!(
            new_named("Budget", NewFileKind::Csv, &names),
            "budget-2.csv"
        );
        assert_eq!(
            new_named("Budget", NewFileKind::Markdown, &names),
            "budget.md",
            "a different extension is a different file"
        );
        // APFS folds case, so this is the same file to the operator's Finder.
        let cased = taken(&["Budget.csv"]);
        assert_eq!(
            new_named("budget", NewFileKind::Csv, &cased),
            "budget-2.csv"
        );
    }

    #[test]
    fn a_log_name_leads_with_the_stamp_so_the_folder_sorts_itself() {
        let names = taken(&[]);
        assert_eq!(
            new_stamped("Shipped 0.8.7", "2026-08-14", "0930", &names),
            "2026-08-14-0930-shipped-0-8-7.md"
        );
        // The counter goes after the slug, never between the stamp and the slug:
        // a `-2` in the middle would break the string sort the naming exists for.
        let one = taken(&["2026-08-14-0930-opened.md"]);
        assert_eq!(
            new_stamped("Opened", "2026-08-14", "0930", &one),
            "2026-08-14-0930-opened-2.md"
        );
    }

    /// A real 26-character ULID, because [`naming::is_ulid`] checks the length
    /// and the alphabet: a short id is not merely ugly, it makes the pool fall
    /// back to `path:` identity and mark the file `unstable_identity` — so a
    /// file keeper *did* author would lose its pins on the first rename. The
    /// shell passes `sync_ipc::new_ulid()`, and this is what that looks like.
    const ULID: &str = "01J5AAAAAAAAAAAAAAAAAAAAAA";

    /// What [`crate::sessions::pool`] reads back must be what this wrote —
    /// otherwise the buttons produce files the spaces cannot see.
    #[test]
    fn a_stamped_name_round_trips_through_the_pool_reader() {
        let name = new_stamped("Opened", "2026-08-14", "0930", &taken(&[]));
        let text = render_new(
            NewFileKind::Markdown,
            Some(KindTag::Log),
            "Opened",
            ULID,
            "2026-08-14",
        );
        let pool = crate::sessions::pool::read(&[crate::sessions::pool::PoolFile {
            rel: &name,
            text: &text,
        }]);
        let entry = &pool[0];
        assert_eq!(entry.kind, Some(KindTag::Log));
        assert_eq!(entry.date, "2026-08-14");
        assert_eq!(entry.time, "09:30");
        assert_eq!(entry.title, "Opened");
        assert_eq!(entry.id, ULID);
        assert!(
            !entry.unstable_identity,
            "keeper authored this one, so it keeps its identity across a rename"
        );
    }

    #[test]
    fn a_plain_markdown_file_declares_no_kind_and_is_told_so() {
        let text = render_new(
            NewFileKind::Markdown,
            None,
            "Stray thought",
            ULID,
            "2026-08-14",
        );
        assert!(!text.contains("tags:"), "{text}");
        let pool = crate::sessions::pool::read(&[crate::sessions::pool::PoolFile {
            rel: "stray-thought.md",
            text: &text,
        }]);
        assert_eq!(pool[0].kind, None, "unfiled, which the detail nudges about");
    }

    /// A task with no `status` matches no column, so the board would draw four
    /// empty columns over a session full of tasks and look like it was working.
    #[test]
    fn a_new_task_starts_in_a_column_that_exists() {
        let text = render_new(
            NewFileKind::Markdown,
            Some(KindTag::Task),
            "Migrate the zone",
            ULID,
            "2026-08-14",
        );
        let pool = crate::sessions::pool::read(&[crate::sessions::pool::PoolFile {
            rel: "migrate.md",
            text: &text,
        }]);
        assert_eq!(
            pool[0].status,
            Some(crate::sessions::shape::TaskStatus::Todo)
        );
    }

    #[test]
    fn the_two_non_markdown_kinds_start_valid() {
        assert_eq!(
            render_new(NewFileKind::Csv, None, "Budget", ULID, "2026-08-14"),
            "",
            "an empty CSV is a valid CSV with no rows; a guessed header would be \
             keeper inventing the operator's columns"
        );
        let json = render_new(NewFileKind::Json, None, "Payload", ULID, "2026-08-14");
        assert_eq!(json, "{}\n");
        serde_json::from_str::<serde_json::Value>(&json)
            .expect("a file keeper wrote must not fail the first tool that reads it");
    }

    #[test]
    fn a_create_makes_the_subfolder_but_not_the_session() {
        let plan = compile_new("active/s", "notes.md", "x").expect("writable");
        assert_eq!(
            plan.steps,
            vec![PlanStep::WriteFile {
                path: "active/s/notes.md".to_owned(),
                content: "x".to_owned()
            }],
            "the session's own directory exists by definition"
        );
        let nested = compile_new("active/s", "artifacts/notes.md", "x").expect("writable");
        assert_eq!(
            nested.steps.first(),
            Some(&PlanStep::MkDir {
                path: "active/s/artifacts".to_owned()
            })
        );
    }

    /// Matrix rows 7, 8 and 10 (Story 50.1), at the level this crate can reach.
    ///
    /// `sessions_file_new_kind` composes exactly these four calls, and the shell
    /// crate does not build on every machine this repo is worked in — so the
    /// composition is asserted here, where it is pure. What the command adds on
    /// top is reading the session's own listing to decide its shape, and running
    /// the plan.
    ///
    /// The round trip through the pool reader is the point, and it is the same
    /// argument `a_stamped_name_round_trips_through_the_pool_reader` makes one
    /// directory up: the directory is what puts the file where a folder-shaped
    /// session's pool LOOKS, and the tag is what makes that file a reference
    /// once it is read (AD-120). Either one alone produces a file no space
    /// lists.
    #[test]
    fn a_folder_shaped_create_composes_the_directory_the_name_and_the_tag() {
        use crate::sessions::shape::{kind_dir, Shape};

        let subdir = kind_dir(Shape::Folder, KindTag::Ref)
            .expect("a folder-shaped session has a home for a reference")
            .expect("and it is a subdirectory, not the root");
        let name = new_stamped("Inputs", "2026-08-16", "0900", &taken(&[]));
        let rel = format!("{subdir}/{name}");
        assert_eq!(rel, "refs/2026-08-16-0900-inputs.md");

        let text = render_new(
            NewFileKind::Markdown,
            Some(KindTag::Ref),
            "Inputs",
            ULID,
            "2026-08-16",
        );
        let pool = crate::sessions::pool::read(&[crate::sessions::pool::PoolFile {
            rel: &rel,
            text: &text,
        }]);
        let entry = pool.first().expect("one file in, one entry out");
        assert_eq!(
            entry.kind,
            Some(KindTag::Ref),
            "the tag is what the References space selects on"
        );
        assert_eq!(entry.rel, "refs/2026-08-16-0900-inputs.md");

        // Row 10: `refs/` is created in the same journaled plan, ahead of the
        // write, so a session that has never held a reference does not need a
        // separate step somebody has to remember.
        let plan = compile_new("active/s", &rel, &text).expect("refs/ is writable");
        assert_eq!(
            plan.steps.first(),
            Some(&PlanStep::MkDir {
                path: "active/s/refs".to_owned()
            })
        );
        assert_eq!(plan.steps.len(), 2, "the directory, then the file");

        // Row 8: the flat arm is unchanged — no subdirectory, a bare root name,
        // and no `MkDir` for a directory that exists by definition.
        assert_eq!(kind_dir(Shape::Flat, KindTag::Ref), Ok(None));
        let flat = compile_new("active/s", &name, &text).expect("the session root is writable");
        assert_eq!(flat.steps.len(), 1);
    }

    #[test]
    fn a_delete_is_a_trash_move_and_the_whole_plan() {
        let plan = compile_delete("active/s", "notes.md", "01TRASH").expect("deletable");
        assert_eq!(
            plan.steps,
            vec![PlanStep::TrashFile {
                path: "active/s/notes.md".to_owned(),
                trash_key: "01TRASH".to_owned()
            }]
        );
    }

    #[test]
    fn a_refused_path_compiles_to_no_plan_at_all() {
        assert!(compile_new("active/s", "workspace/iter.md", "x").is_err());
        assert!(compile_delete("active/s", "workspace/iter.md", "01T").is_err());
        assert!(compile_delete("active/s", ABOUT, "01T").is_err());
    }
}
