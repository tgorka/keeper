//! Lifecycle verbs compile to plans; the shell executes them (AD-111).
//!
//! A plan is an ordered list of primitive steps — mkdir, copy, write, splice,
//! move-dir, trash-dir — that the shell's executor runs with a journal row
//! beside it, so a crash mid-verb leaves either a resumable prefix or a clean
//! rollback and never a half-moved session (NFR-38). Compiling here keeps the
//! decisions pure and testable: what a create copies, what a pattern copy
//! takes and refuses, what an archive does in what order, all asserted over
//! plain values with no filesystem.
//!
//! Two invariants every compile function keeps:
//!
//! 1. **Idempotent steps.** `MkDir` succeeds on an existing directory,
//!    `CopyFile` overwrites its target, `MoveDir` succeeds when the source is
//!    gone and the target exists (the move already happened) — which is what
//!    makes resume "run the remaining steps" and nothing cleverer.
//! 2. **The irreversible step is last.** An archive's folder move and a
//!    delete's trash move sort after everything else, so the crash window
//!    before them costs re-running cheap steps and the window after them is
//!    the verb having completed.

use crate::notes::frontmatter::{FieldValue, Frontmatter};
use crate::sessions::model::KEY_CONTINUED_BY;

/// One primitive the executor knows how to run. Paths are zone-relative,
/// `/`-joined; the executor owns joining them onto the zone root.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(tag = "op", rename_all = "camelCase")]
pub enum PlanStep {
    /// Create a directory (and parents). Succeeds if it exists.
    MkDir { path: String },
    /// Copy one file, overwriting the target.
    CopyFile { from: String, to: String },
    /// Write these exact bytes to a file, atomically, overwriting.
    WriteFile { path: String, content: String },
    /// Replace a file's whole content with `content` **only if** its current
    /// content is `expect` — the splice-writer's optimistic guard, so a
    /// concurrent agent write turns into a refusal rather than a lost edit.
    GuardedWrite {
        path: String,
        expect_len: usize,
        content: String,
    },
    /// Move a directory. Succeeds if the source is gone and the target exists.
    MoveDir { from: String, to: String },
    /// Move one file. Succeeds if the source is gone and the target exists.
    ///
    /// [`PlanStep::MoveDir`]'s twin, and a **move** rather than a copy followed
    /// by a delete for two independent reasons. A rename inside one volume is
    /// one atomic metadata write, while copy-then-delete is two writes the sync
    /// watcher reads as a new file plus a deletion — the drive's history would
    /// carry the bytes twice and the old path's provenance would be lost. And a
    /// crash between the two halves leaves the file at both paths, which no
    /// idempotent replay can tell apart from "the copy has not run yet"; a
    /// rename has no such intermediate state, which is what keeps resume
    /// "re-run the remaining steps" (AD-111).
    MoveFile { from: String, to: String },
    /// Move a directory into the zone's `.keeper/trash/<id>/`, recoverable.
    TrashDir { path: String, trash_key: String },
    /// Move one file into the zone's `.keeper/trash/<id>/`, keeping its name,
    /// recoverable.
    ///
    /// [`PlanStep::TrashDir`]'s twin rather than a `remove_file`, and for the
    /// reason the trash exists at all: a space, a log entry and a prompt are
    /// each a file somebody wrote, and a delete button that unlinks bytes is a
    /// delete button nobody presses without a backup first. Keeping the basename
    /// under the key is what makes the recovery obvious — `.keeper/trash/<id>/`
    /// holds `tasks.md`, not an extension-less blob named after a ULID.
    TrashFile { path: String, trash_key: String },
    /// Remove every entry under a directory except `.gitkeep`, writing one if
    /// absent — the zone's "empty the workspace" (FR-245 step 3).
    EmptyDirKeep { path: String },
}

/// A compiled verb: its steps, in execution order.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Plan {
    /// The verb, for the journal row and the log: `create`, `create-from`,
    /// `archive`, `delete`, `unarchive`.
    pub verb: String,
    /// The session this plan is about, zone-relative (its path *before* the
    /// plan runs).
    pub session: String,
    pub steps: Vec<PlanStep>,
}

/// Compile a **create** from a pattern (FR-238, FR-253): copy what
/// [`crate::sessions::pattern::apply`] chose out of `pattern_root` — the
/// zone's `_template`, or an existing session's zone-relative path — into
/// `active/<dir_name>`, then write the stamped README last.
///
/// The pattern root is a parameter rather than a constant because *where a
/// new session is shaped from* is the user's choice now, not the code's: a
/// template create and a continuation differ in which directory the copies
/// read from and in nothing else. Making the shell rewrite copy sources after
/// the fact — which it used to — meant the plan briefly described a copy that
/// was never going to happen, and a journal is worth exactly as much as the
/// plan it replays.
///
/// `copies` arrives directory-before-contents, so the steps are safe to run
/// top to bottom; the README write is last so a pattern that carries its own
/// README loses to the stamped one without this function knowing it did.
pub fn compile_create(
    dir_name: &str,
    pattern_root: &str,
    copies: &[(String, bool)],
    readme: &str,
) -> Plan {
    compile_create_shaped(
        dir_name,
        pattern_root,
        copies,
        &[],
        &[(super::model::README.to_owned(), readme.to_owned())],
    )
}

/// The general form: copy the pattern, then write `stamped` — one entry per
/// file keeper composes itself, session-relative name and bytes (FR-268).
///
/// The shape-aware seam, and the only one. A folder-shaped create stamps one
/// `README.md`; a flat one stamps `AGENTS.md`, that same `README.md` and its two
/// seed files. Everything else about a create — where the copies come from, that
/// directories precede their contents, that the stamped files are written last
/// so a pattern carrying its own copy loses to them — is identical, and putting
/// the difference here rather than in the shell is what keeps it to one line of
/// divergence instead of two create paths that drift.
///
/// Stamped writes are last and in the order given. `MkDir` and `WriteFile` are
/// both idempotent, so the whole plan stays replayable (AD-111).
///
/// **`expanded` is how a placeholder reaches the new session** (FR-292):
/// `(source-relative path, the bytes that path arrives with)` for each pattern
/// file whose `{{token}}`s the caller has already resolved. Such a file
/// compiles to a [`PlanStep::WriteFile`] instead of a [`PlanStep::CopyFile`];
/// every other file still copies byte for byte, and a caller with nothing to
/// expand passes `&[]` and gets exactly the plan it got before.
///
/// **The resolved bytes travel in the plan, not the context that produced
/// them.** A plan is journaled and replayed (AD-111), and a replay that
/// re-expanded would have to re-read the clock — so a session resumed at
/// 00:01 would get yesterday's `{{date}}` in one file and today's in the next.
/// Carrying `TemplateCtx` in the plan instead would still leave the *renderer*
/// as a version-to-version variable. Bytes are the only form of the answer
/// that cannot change between the write and the replay, which is the same
/// reason `stamped` has always carried bytes rather than a title and a date.
///
/// The cost is a fatter journal row for a template full of placeholders, and
/// it is bounded by the caller: nothing stops it passing only the files
/// expansion actually changed.
pub fn compile_create_shaped(
    dir_name: &str,
    pattern_root: &str,
    copies: &[(String, bool)],
    expanded: &[(String, String)],
    stamped: &[(String, String)],
) -> Plan {
    let target = format!("active/{dir_name}");
    let mut steps = vec![PlanStep::MkDir {
        path: target.clone(),
    }];
    for (rel, is_dir) in copies {
        if *is_dir {
            steps.push(PlanStep::MkDir {
                path: format!("{target}/{rel}"),
            });
            continue;
        }
        match expanded.iter().find(|(name, _)| name == rel) {
            Some((_, content)) => steps.push(PlanStep::WriteFile {
                path: format!("{target}/{rel}"),
                content: content.clone(),
            }),
            None => steps.push(PlanStep::CopyFile {
                from: format!("{pattern_root}/{rel}"),
                to: format!("{target}/{rel}"),
            }),
        }
    }
    for (name, content) in stamped {
        steps.push(PlanStep::WriteFile {
            path: format!("{target}/{name}"),
            content: content.clone(),
        });
    }
    Plan {
        verb: "create".to_owned(),
        session: target,
        steps,
    }
}

/// What a pattern copy takes from a source session (FR-239): structure only.
///
/// Kept as the one-line spelling of [`crate::sessions::pattern::apply`] for
/// the session kind — the rule itself, and the *reasons* the skipped files
/// carry, live there so the picker's preview and the plan read one value.
pub fn pattern_copies(source_files: &[(String, bool)]) -> Vec<(String, bool)> {
    crate::sessions::pattern::apply(crate::sessions::pattern::PatternKind::Session, source_files)
        .copies
}

/// Compile a **create-from** (FR-239, AD-112): the structural copy plus the
/// two lineage writes — `continues` into the new README (the caller bakes it
/// in before compiling) and `continued-by` appended into the SOURCE README,
/// including when the source is archived: files are truth, and a lineage the
/// index alone knew would be invisible to the agent and to Obsidian.
///
/// `source_readme` is the source's current bytes; the append is compiled as a
/// [`PlanStep::GuardedWrite`] against their length, so a concurrent edit of
/// the source refuses and the verb re-plans rather than clobbering.
pub fn compile_create_from(
    dir_name: &str,
    source_session: &str,
    source_readme: &str,
    new_id: &str,
    copies: &[(String, bool)],
    readme: &str,
) -> Plan {
    compile_create_from_shaped(
        dir_name,
        source_session,
        super::model::README,
        source_readme,
        new_id,
        copies,
        &[],
        &[(super::model::README.to_owned(), readme.to_owned())],
    )
}

/// The general form of [`compile_create_from`] (FR-268): the shaped create
/// plus the lineage append, written to the source's record.
///
/// **`record_name` is the name the bytes in `source_record` were READ from, and
/// the two travel together for that reason.** Story 52.1 removed it, on the
/// argument that both contracts call the record `README.md` now and the caller
/// therefore has nothing to get wrong. That is true of what keeper *writes* and
/// false of what is on the operator's drives: a flat session created before that
/// story keeps its record in `about.md` until `sessions_record_migrate` has swept
/// the zone, and continuing one is a thing an operator does on day one. With the
/// name fixed, the shell read the source's `README.md` — absent — and compiled
/// `GuardedWrite { path: "<source>/README.md", expect_len: 0 }`, which
/// `sessions_exec` reads before writing and refuses with a raw ENOENT; the append
/// is pushed AFTER the create steps, so the operator got an errno, a new session
/// already on disk, and no `continues`/`continued-by` pair — precisely the loss
/// AD-112 exists to prevent. Worse in the half-migrated shape, where an old
/// README signpost is still there: the lengths agree, and the lineage lands in
/// the signpost.
///
/// A guard read from one file and written to another is a guard that always
/// mismatches, so this takes the name rather than assuming it.
// Eight, and each one is a distinct fact about the create the caller already
// holds separately. Bundling them into a `CreateRequest` would obscure that
// `copies`, `expanded` and `stamped` are three different answers to "where do
// these bytes come from" — which is the one thing a reader of this function has
// to keep straight.
#[allow(clippy::too_many_arguments)]
pub fn compile_create_from_shaped(
    dir_name: &str,
    source_session: &str,
    record_name: &str,
    source_record: &str,
    new_id: &str,
    copies: &[(String, bool)],
    expanded: &[(String, String)],
    stamped: &[(String, String)],
) -> Plan {
    let mut plan = compile_create_shaped(dir_name, source_session, copies, expanded, stamped);
    plan.verb = "create-from".to_owned();
    // The source-side lineage append, byte-preserving outside the key.
    let updated = append_lineage(source_record, KEY_CONTINUED_BY, new_id);
    plan.steps.push(PlanStep::GuardedWrite {
        path: format!("{source_session}/{record_name}"),
        expect_len: source_record.len(),
        content: updated,
    });
    plan
}

/// A README's frontmatter with `id` appended to the `keeper.<key>` flow list
/// (creating the block, the map or the key as needed), every other byte
/// preserved (NFR-39). The canonical spelling is the flow list — see
/// [`crate::sessions::model::KEY_CONTINUES`].
pub fn append_lineage(readme: &str, key: &str, id: &str) -> String {
    let (fm, _) = Frontmatter::parse(readme);
    let mut pairs = match fm.get("keeper") {
        Some(FieldValue::Map(pairs)) => pairs.clone(),
        _ => Vec::new(),
    };
    let mut ids: Vec<FieldValue> = match pairs.iter().find(|(k, _)| k == key) {
        Some((_, FieldValue::List(items))) => items.clone(),
        Some((_, scalar)) => vec![scalar.clone()],
        None => Vec::new(),
    };
    if ids.iter().any(|existing| existing.index_string() == id) {
        return readme.to_owned();
    }
    ids.push(FieldValue::Str(id.to_owned()));
    match pairs.iter_mut().find(|(k, _)| k == key) {
        Some(entry) => entry.1 = FieldValue::List(ids),
        None => pairs.push((key.to_owned(), FieldValue::List(ids))),
    }
    Frontmatter::set_in(readme, "keeper", FieldValue::Map(pairs))
}

/// The README skeleton a new session starts from when built from a pattern
/// rather than the template: the source's section headings, empty, with the
/// standard bullets under the title (FR-239 — headings, never content).
pub fn skeleton_from(source_body: &str, title: &str, date: &str) -> String {
    let mut out = format!("# {title}\n\n- **Date:** {date}\n- **Tool/model:**\n- **Goal:**\n");
    for line in source_body.lines() {
        let trimmed = line.trim_end();
        if trimmed.starts_with("## ") {
            out.push('\n');
            out.push_str(trimmed);
            out.push('\n');
            // The Promote section keeps its table scaffold: a session without
            // the table cannot record a promotion, and the panel refuses to
            // invent one (files are truth).
            if trimmed == "## Promote" {
                out.push_str(
                    "\n| workspace | → artifacts | note |\n| --------- | ----------- | ---- |\n",
                );
            }
        }
    }
    out
}

/// Compile a **log-today** append (FR-240): today's `### <date> — ` entry
/// under `## Log`, newest last per the zone's convention, creating the
/// section at the end when absent. A guarded write, for the same
/// concurrent-agent reason as the lineage append.
pub fn compile_log_today(session: &str, readme: &str, date: &str) -> Option<(Plan, usize)> {
    // Already an entry for today: the verb is "open and place the caret",
    // not "write" — the caller learns that from the None.
    let heading = format!("### {date}");
    if readme
        .lines()
        .any(|line| line.trim_start().starts_with(&heading))
    {
        return None;
    }
    let entry = format!("### {date} — \n");
    let (updated, caret) = match log_section_end(readme) {
        Some(at) => {
            let mut out = String::with_capacity(readme.len() + entry.len() + 1);
            out.push_str(&readme[..at]);
            let needs_gap = !readme[..at].ends_with("\n\n");
            if needs_gap {
                out.push('\n');
            }
            out.push_str(&entry);
            let caret = out.len() - 1;
            out.push_str(&readme[at..]);
            (out, caret)
        }
        None => {
            let mut out = readme.to_owned();
            if !out.ends_with('\n') {
                out.push('\n');
            }
            out.push_str("\n## Log\n\n");
            out.push_str(&entry);
            let caret = out.len() - 1;
            (out, caret)
        }
    };
    Some((
        Plan {
            verb: "log-today".to_owned(),
            session: session.to_owned(),
            steps: vec![PlanStep::GuardedWrite {
                path: format!("{session}/README.md"),
                expect_len: readme.len(),
                content: updated,
            }],
        },
        caret,
    ))
}

/// What an archive checklist resolved to, step by step (FR-245): the caller
/// (the checklist UI, through the shell) has already walked promotes and
/// warnings with the user; this compiles the *fs half* it settled on.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ArchiveDecision {
    /// Promote copies to run first: `(workspace source, artifacts target)`,
    /// session-relative. Skipped rows are simply absent.
    pub promotes: Vec<(String, String)>,
    /// Whether to empty `workspace/` (leave `.gitkeep`). Skippable-with-
    /// warning per the checklist; the move is not.
    pub empty_workspace: bool,
    /// The close year — `archive/<year>/` is the destination.
    pub year: i32,
}

/// Compile an **archive** (FR-245, AD-111): promotes, then the workspace
/// emptying, then — last, always last — the folder move. The executor runs
/// promote copies through the stability gate; a parked copy pauses the plan
/// rather than half-copying (AD-111 detail).
pub fn compile_archive(session: &str, decision: &ArchiveDecision) -> Plan {
    let name = session.rsplit('/').next().unwrap_or(session);
    let mut steps = Vec::new();
    for (from, to) in &decision.promotes {
        steps.push(PlanStep::CopyFile {
            from: format!("{session}/{from}"),
            to: format!("{session}/{to}"),
        });
    }
    if decision.empty_workspace {
        steps.push(PlanStep::EmptyDirKeep {
            path: format!("{session}/workspace"),
        });
    }
    steps.push(PlanStep::MkDir {
        path: format!("archive/{}", decision.year),
    });
    steps.push(PlanStep::MoveDir {
        from: session.to_owned(),
        to: format!("archive/{}/{name}", decision.year),
    });
    Plan {
        verb: "archive".to_owned(),
        session: session.to_owned(),
        steps,
    }
}

/// Compile a **delete** (FR-247): one trash move, recoverable, never an
/// unlink. The trash key is the session's id so a recovery can find it.
pub fn compile_delete(session: &str, trash_key: &str) -> Plan {
    Plan {
        verb: "delete".to_owned(),
        session: session.to_owned(),
        steps: vec![PlanStep::TrashDir {
            path: session.to_owned(),
            trash_key: trash_key.to_owned(),
        }],
    }
}

/// Compile an **unarchive** (FR-248): one move back to `active/`. Lineage is
/// never rewritten (AD-112) — the verb is a location change and nothing else.
pub fn compile_unarchive(session: &str) -> Plan {
    let name = session.rsplit('/').next().unwrap_or(session);
    Plan {
        verb: "unarchive".to_owned(),
        session: session.to_owned(),
        steps: vec![PlanStep::MoveDir {
            from: session.to_owned(),
            to: format!("active/{name}"),
        }],
    }
}

/// Byte offset where the `## Log` section ends — the start of the next `## `
/// heading after it, or EOF. `None` when there is no Log section.
fn log_section_end(body: &str) -> Option<usize> {
    let mut offset = 0;
    let mut in_log = false;
    for line in body.split_inclusive('\n') {
        let trimmed = line.trim_end_matches(['\n', '\r']).trim();
        if in_log && trimmed.starts_with("## ") {
            return Some(offset);
        }
        if trimmed == "## Log" {
            in_log = true;
        }
        offset += line.len();
    }
    in_log.then_some(body.len())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sessions::model::lineage;

    const TEMPLATE: &[(&str, bool)] = &[
        ("README.md", false),
        ("workspace", true),
        ("artifacts", true),
        ("refs", true),
        ("prompts", true),
    ];

    /// A create copies the template verbatim, then overwrites the README —
    /// so a template that grows files keeps working, and the stamped README
    /// always wins. The mkdir comes first, the write last.
    #[test]
    fn a_create_copies_the_template_and_stamps_the_readme_last() {
        let files: Vec<(String, bool)> = TEMPLATE
            .iter()
            .map(|(rel, dir)| ((*rel).to_owned(), *dir))
            .collect();
        let plan = compile_create("2026-08-12-research", "_template", &files, "# research\n");
        assert_eq!(plan.verb, "create");
        assert_eq!(plan.session, "active/2026-08-12-research");
        assert!(
            matches!(&plan.steps[0], PlanStep::MkDir { path } if path == "active/2026-08-12-research")
        );
        assert!(matches!(
            plan.steps.last(),
            Some(PlanStep::WriteFile { path, content })
                if path == "active/2026-08-12-research/README.md" && content == "# research\n"
        ));
        assert!(plan.steps.iter().any(|step| matches!(step,
            PlanStep::CopyFile { from, .. } if from == "_template/README.md")));
    }

    /// Every copy reads from the pattern the user picked (FR-253). The shell
    /// used to rewrite copy sources after compiling; a plan that named a
    /// source it did not mean was a journal that replayed the wrong thing.
    #[test]
    fn copies_read_from_the_pattern_the_caller_named() {
        let plan = compile_create(
            "2026-08-12-continuation",
            "archive/2025/2025-03-01-old",
            &[
                ("prompts".to_owned(), true),
                ("prompts/01-scope.md".to_owned(), false),
            ],
            "# continuation\n",
        );
        assert!(plan.steps.iter().any(|step| matches!(step,
            PlanStep::CopyFile { from, to }
                if from == "archive/2025/2025-03-01-old/prompts/01-scope.md"
                && to == "active/2026-08-12-continuation/prompts/01-scope.md")));
        assert!(
            !plan.steps.iter().any(|step| matches!(step,
                PlanStep::CopyFile { from, .. } if from.starts_with("_template/"))),
            "a session pattern never reads the template"
        );
    }

    /// The pattern copy is structure-only (FR-239): prompts and refs travel,
    /// artifacts and workspace contents never do.
    #[test]
    fn a_pattern_copy_takes_prompts_and_refs_and_refuses_output_and_scratch() {
        let source = vec![
            ("README.md".to_owned(), false),
            ("prompts".to_owned(), true),
            ("prompts/01-scope.md".to_owned(), false),
            ("refs".to_owned(), true),
            ("refs/pointer.md".to_owned(), false),
            ("artifacts".to_owned(), true),
            ("artifacts/final-report.md".to_owned(), false),
            ("workspace".to_owned(), true),
            ("workspace/scratch.csv".to_owned(), false),
        ];
        let copies = pattern_copies(&source);
        let names: Vec<&str> = copies.iter().map(|(rel, _)| rel.as_str()).collect();
        assert!(names.contains(&"prompts/01-scope.md"));
        assert!(names.contains(&"refs/pointer.md"));
        assert!(
            !names.contains(&"artifacts/final-report.md"),
            "output stays"
        );
        assert!(!names.contains(&"workspace/scratch.csv"), "scratch stays");
        assert!(!names.contains(&"README.md"), "prose never travels");
        // The four standard directories exist in every pattern copy.
        for dir in ["workspace", "artifacts", "refs", "prompts"] {
            assert!(names.contains(&dir));
        }
    }

    /// The lineage append writes the flow-list spelling the parser models,
    /// round-trips through `lineage()`, dedupes, and leaves every other byte
    /// alone (NFR-39 asserted by reconstruction).
    #[test]
    fn the_lineage_append_round_trips_and_preserves_the_rest() {
        let readme =
            "---\nid: 01J5AAAAAAAAAAAAAAAAAAAAAA\npinned: true\n---\n# keeper\n\nbody text\n";
        let updated = append_lineage(readme, KEY_CONTINUED_BY, "01J6BBBBBBBBBBBBBBBBBBBBBB");
        let (fm, _) = Frontmatter::parse(&updated);
        assert!(fm.unparsed().is_none(), "the write parses clean: {updated}");
        assert_eq!(
            lineage(&fm).continued_by,
            vec!["01J6BBBBBBBBBBBBBBBBBBBBBB"]
        );
        assert!(updated.contains("pinned: true\n"), "siblings survive");
        assert!(
            updated.ends_with("# keeper\n\nbody text\n"),
            "the body survives"
        );
        // Appending the same id again is a no-op, not a duplicate.
        let again = append_lineage(&updated, KEY_CONTINUED_BY, "01J6BBBBBBBBBBBBBBBBBBBBBB");
        assert_eq!(again, updated);
    }

    /// A create-from carries the guarded source-side write, fenced on the
    /// source README's current length so a concurrent agent edit refuses.
    #[test]
    fn a_create_from_guards_the_source_readme_write() {
        let source_readme = "---\nid: 01J5AAAAAAAAAAAAAAAAAAAAAA\n---\n# old\n";
        let plan = compile_create_from(
            "2026-08-12-continuation",
            "archive/2025/2025-03-01-old",
            source_readme,
            "01J6BBBBBBBBBBBBBBBBBBBBBB",
            &[("prompts".to_owned(), true)],
            "# continuation\n",
        );
        assert_eq!(plan.verb, "create-from");
        let Some(PlanStep::GuardedWrite {
            path,
            expect_len,
            content,
        }) = plan.steps.last()
        else {
            panic!("the source write is the last step");
        };
        assert_eq!(path, "archive/2025/2025-03-01-old/README.md");
        assert_eq!(*expect_len, source_readme.len());
        assert!(content.contains("session-continued-by"));
    }

    /// Continuing a session whose record has NOT been migrated yet — which is the
    /// state of every flat session on the operator's drives the day story 52.1
    /// ships.
    ///
    /// The append has to land on the file the bytes came out of. When the name was
    /// a constant, the shell read `<source>/README.md` (absent), got `""`, and
    /// compiled `GuardedWrite { path: "<source>/README.md", expect_len: 0 }`:
    /// `sessions_exec` reads the target before writing and maps the ENOENT to
    /// `Refused`, so the step failed with an errno *after* the create steps had
    /// already put the new session on disk — a stray session and no
    /// `continues`/`continued-by` pair, the loss AD-112 exists to prevent.
    ///
    /// The half-migrated shape is the sharper one: an old migration's README
    /// signpost is still sitting where the record used to be, so `expect_len`
    /// MATCHES the bytes read from it, the guard is satisfied, and the lineage is
    /// appended into a three-line redirect instead of into the record. Nothing in
    /// this crate can see that happen — it is decided by which file the shell
    /// read — which is exactly why the name and the bytes are one pair of
    /// arguments here, and why `sessions_ipc` reads them together.
    #[test]
    fn a_create_from_an_unmigrated_source_appends_to_the_record_that_exists() {
        const ABOUT: &str = "about.md";
        let source_record =
            "---\nid: 01J5AAAAAAAAAAAAAAAAAAAAAA\ntags: [about]\n---\n# old flat session\n";
        let plan = compile_create_from_shaped(
            "2026-08-17-continuation",
            "active/2026-03-01-old",
            ABOUT,
            source_record,
            "01J6BBBBBBBBBBBBBBBBBBBBBB",
            &[],
            &[],
            &[(super::super::model::README.to_owned(), "# new\n".to_owned())],
        );
        let Some(PlanStep::GuardedWrite {
            path,
            expect_len,
            content,
        }) = plan.steps.last()
        else {
            panic!("the source write is the last step: {:?}", plan.steps);
        };
        assert_eq!(
            path, "active/2026-03-01-old/about.md",
            "the lineage is appended to the file the guard's bytes were read from"
        );
        assert_eq!(*expect_len, source_record.len());
        assert!(
            content.contains("session-continued-by")
                && content.contains("01J6BBBBBBBBBBBBBBBBBBBBBB"),
            "{content}"
        );

        // Row for the migrated source, from the same function: the name is what
        // decides, and nothing else in the plan changes with it.
        let migrated = compile_create_from_shaped(
            "2026-08-17-continuation",
            "active/2026-03-01-old",
            super::super::model::README,
            source_record,
            "01J6BBBBBBBBBBBBBBBBBBBBBB",
            &[],
            &[],
            &[(super::super::model::README.to_owned(), "# new\n".to_owned())],
        );
        assert!(
            matches!(migrated.steps.last(), Some(PlanStep::GuardedWrite { path, .. })
                if path == "active/2026-03-01-old/README.md"),
            "{:?}",
            migrated.steps.last()
        );
        assert_eq!(migrated.steps.len(), plan.steps.len());
    }

    /// The skeleton takes headings, never prose, and re-scaffolds the
    /// Promote table so the new session can record promotions from day one.
    #[test]
    fn a_skeleton_takes_headings_and_rebuilds_the_promote_table() {
        let source = "# old title\n\n## Summary\n\nSecret prose.\n\n## Key decisions\n\n- old\n\n## Log\n\n### 2026-01-01 — x\n\n## Promote\n\n| workspace | → artifacts | note |\n| --- | --- | --- |\n| workspace/a | artifacts/a | v1 |\n";
        let out = skeleton_from(source, "new title", "2026-08-12");
        assert!(out.starts_with("# new title\n"));
        assert!(out.contains("## Summary"));
        assert!(out.contains("## Promote"));
        assert!(out.contains("| workspace | → artifacts | note |"));
        assert!(!out.contains("Secret prose"), "prose never travels");
        assert!(!out.contains("workspace/a"), "old promotions never travel");
        assert!(!out.contains("2026-01-01"), "old log entries never travel");
    }

    /// An archive plan runs promotes, then the emptying, then the move —
    /// and the move is LAST, which is the whole crash-safety argument
    /// (NFR-38): everything before it re-runs; after it the verb is done.
    #[test]
    fn an_archive_plan_moves_the_folder_last() {
        let plan = compile_archive(
            "active/2026-08-10-keeper",
            &ArchiveDecision {
                promotes: vec![(
                    "workspace/draft.md".to_owned(),
                    "artifacts/report.md".to_owned(),
                )],
                empty_workspace: true,
                year: 2026,
            },
        );
        assert_eq!(plan.verb, "archive");
        assert!(matches!(&plan.steps[0], PlanStep::CopyFile { from, to }
            if from == "active/2026-08-10-keeper/workspace/draft.md"
            && to == "active/2026-08-10-keeper/artifacts/report.md"));
        assert!(matches!(&plan.steps[1], PlanStep::EmptyDirKeep { path }
            if path == "active/2026-08-10-keeper/workspace"));
        assert!(
            matches!(plan.steps.last(), Some(PlanStep::MoveDir { from, to })
            if from == "active/2026-08-10-keeper" && to == "archive/2026/2026-08-10-keeper")
        );
    }

    /// Delete is one recoverable trash move; unarchive is one move back and
    /// never touches lineage (the plan holds no write step at all).
    #[test]
    fn delete_trashes_and_unarchive_moves_back_without_writes() {
        let del = compile_delete("active/x", "01J5AAAAAAAAAAAAAAAAAAAAAA");
        assert_eq!(del.steps.len(), 1);
        assert!(matches!(&del.steps[0], PlanStep::TrashDir { trash_key, .. }
            if trash_key == "01J5AAAAAAAAAAAAAAAAAAAAAA"));

        let un = compile_unarchive("archive/2025/2025-03-01-taxes");
        assert_eq!(un.steps.len(), 1, "a location change and nothing else");
        assert!(matches!(&un.steps[0], PlanStep::MoveDir { to, .. }
            if to == "active/2025-03-01-taxes"));
    }

    /// Log-today appends newest-last inside the Log section, creates the
    /// section when missing, guards on length, and refuses a second entry
    /// for the same day by answering None.
    #[test]
    fn log_today_appends_newest_last_and_is_once_per_day() {
        let readme = "# s\n\n## Log\n\n### 2026-08-10 — opened\n\ntext\n\n## Follow-ups\n\n- x\n";
        let (plan, caret) =
            compile_log_today("active/s", readme, "2026-08-12").expect("a new day writes");
        let Some(PlanStep::GuardedWrite {
            content,
            expect_len,
            ..
        }) = plan.steps.first()
        else {
            panic!("a guarded write");
        };
        assert_eq!(*expect_len, readme.len());
        let log_at = content.find("### 2026-08-12 — ").expect("today's entry");
        let old_at = content.find("### 2026-08-10").expect("the old entry stays");
        let followups_at = content
            .find("## Follow-ups")
            .expect("the next section stays");
        assert!(
            old_at < log_at && log_at < followups_at,
            "newest last, inside the section"
        );
        assert_eq!(
            &content[caret - 1..caret + 1],
            " \n",
            "the caret sits at the entry's end"
        );

        // Same day again: not a write.
        assert!(compile_log_today("active/s", content, "2026-08-12").is_none());

        // No Log section at all: created at the end.
        let bare = "# s\n";
        let (plan2, _) = compile_log_today("active/s", bare, "2026-08-12").expect("writes");
        let Some(PlanStep::GuardedWrite { content, .. }) = plan2.steps.first() else {
            panic!("a guarded write");
        };
        assert!(content.contains("## Log\n\n### 2026-08-12 — \n"));
    }

    /// An expanded pattern file compiles to a `WriteFile` carrying the resolved
    /// bytes, and everything beside it still copies. The bytes ride in the plan
    /// rather than the context that produced them, so a replay of the journal
    /// row writes the same file without asking the clock again (AD-111).
    #[test]
    fn an_expanded_pattern_file_is_written_and_the_rest_still_copies() {
        let copies = vec![
            ("refs".to_owned(), true),
            ("refs/inputs.md".to_owned(), false),
            ("logo.png".to_owned(), false),
            ("plain.md".to_owned(), false),
        ];
        let plan = compile_create_shaped(
            "2026-08-17-ship-it",
            "_template/house",
            &copies,
            &[("refs/inputs.md".to_owned(), "# Ship it\n".to_owned())],
            &[("README.md".to_owned(), "# stamped\n".to_owned())],
        );
        assert!(
            plan.steps.contains(&PlanStep::WriteFile {
                path: "active/2026-08-17-ship-it/refs/inputs.md".to_owned(),
                content: "# Ship it\n".to_owned(),
            }),
            "the expanded file is written, not copied: {:?}",
            plan.steps
        );
        assert!(
            !plan.steps.iter().any(|step| matches!(
                step,
                PlanStep::CopyFile { to, .. } if to.ends_with("refs/inputs.md")
            )),
            "and never both — a copy then a write is dead work the preview must explain"
        );
        for rel in ["logo.png", "plain.md"] {
            assert!(
                plan.steps.contains(&PlanStep::CopyFile {
                    from: format!("_template/house/{rel}"),
                    to: format!("active/2026-08-17-ship-it/{rel}"),
                }),
                "{rel} copies byte for byte"
            );
        }
        // The stamped record is still last, and is not an expansion candidate:
        // it is composed from the pattern's headings and never copied at all.
        assert_eq!(
            plan.steps.last(),
            Some(&PlanStep::WriteFile {
                path: "active/2026-08-17-ship-it/README.md".to_owned(),
                content: "# stamped\n".to_owned(),
            })
        );
    }

    /// A caller with nothing to expand gets exactly the plan it always got —
    /// the seam is additive, and a template of ordinary prose pays nothing.
    #[test]
    fn no_expansions_means_the_plan_is_unchanged() {
        let copies = vec![("notes.md".to_owned(), false)];
        let stamped = [("README.md".to_owned(), "# r\n".to_owned())];
        let bare = compile_create_shaped("2026-08-17-s", "_template", &copies, &[], &stamped);
        assert_eq!(
            bare.steps,
            vec![
                PlanStep::MkDir {
                    path: "active/2026-08-17-s".to_owned()
                },
                PlanStep::CopyFile {
                    from: "_template/notes.md".to_owned(),
                    to: "active/2026-08-17-s/notes.md".to_owned(),
                },
                PlanStep::WriteFile {
                    path: "active/2026-08-17-s/README.md".to_owned(),
                    content: "# r\n".to_owned(),
                },
            ]
        );
    }
}
