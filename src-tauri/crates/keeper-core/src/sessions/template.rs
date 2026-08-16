//! The template keeper ships (FR-268): the flat contract, as four files.
//!
//! A zone that has its own `_template/` uses it — keeper copies that directory
//! and does not improve on it. This module is what a zone *without* one gets,
//! and it is written in the flat contract because that is the shape keeper now
//! recommends: one pool of markdown, kinds declared as tags (AD-120).
//!
//! **Two functions, and the difference between them is the whole of this
//! module's subtlety.** [`default_template`] renders a *session* — four files,
//! titled and stamped for one particular create. [`zone_skeleton`] renders a
//! *template* — the two files that carry no session-specific bytes. The seeds
//! are examples composed per create, so writing them into `_template/` freezes
//! one session's title and minute into every session made from it afterwards;
//! that shipped once and is what the second function exists to prevent.
//!
//! The four files are not a sample. They are the smallest set that makes the
//! flat shape legible:
//!
//! - `AGENTS.md` — how to read the folder. The flat shape's known cost is that
//!   a session directory is an undifferentiated pile of markdown until
//!   something reads the tags; this file is the mitigation, and it is written
//!   for whoever — or whatever — is handed the folder with no other context.
//! - `about.md` — the record. What the folder-shaped session kept in
//!   `README.md`, including the `## Promote` table.
//! - one seed log — so the log space is not empty on day one, and so the
//!   filename convention is visible as an example rather than only as a rule.
//! - one seed prompt — same reason, for the prompt space.
//!
//! **Why no README.** The operator's instruction was explicit: the navigation
//! file is `AGENTS.md`, and a README beside it would be a second answer to the
//! same question. A folder-shaped session keeps its README forever; a flat one
//! never grows one.
//!
//! Everything here is content, not IO. The caller supplies the title, the date
//! and the timestamp, because the domain has no clock (AD-56) — and because a
//! resumed journal must replay the stamp it recorded rather than a fresh one.

use super::shape::{KindTag, ABOUT, AGENTS};

/// One file of the template, ready to write.
///
/// A pair rather than a path plus a closure: the plan compiler takes bytes, and
/// a template that could not be printed before it is written would be a
/// template nobody could review.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TemplateFile {
    /// Session-relative name. Flat, so never contains a `/`.
    pub name: String,
    pub content: String,
    /// What this file *is*, when it is one of the five kinds.
    ///
    /// Carried beside the bytes because a seed's filename is stamped
    /// (`YYYY-MM-DD-HHMM-opened.md`), so a caller deciding "does the pattern
    /// already supply a log" cannot ask the name — two seeds written a minute
    /// apart have different names and identical meaning. The kind is the thing
    /// that must not be duplicated, so the kind is what travels.
    pub kind: Option<KindTag>,
}

/// The navigation contract, verbatim.
///
/// Written in the zone's own voice — imperative, second person, every rule
/// carrying the reason it exists — because the reader is as often an agent as a
/// person, and a rule without its reason is one an agent will optimise away the
/// first time it is inconvenient.
pub const AGENTS_MD: &str = r#"# How to work in this session

This folder is one work session. Everything in it is markdown in a single flat
pool — there are no `refs/`, `prompts/` or `logs/` directories, and there is no
README. A file's **kind is a tag in its own frontmatter**, not the folder it
sits in, so moving a file never changes what it is.

Read this file first. It is the only file whose job is to tell you how to read
the others.

## Start here, in this order

1. **`about.md`** — what this session is for, and the `## Promote` table.
2. **The tasks** — every file tagged `task`. Its `status` says where it stands:
   `in-preparation`, `todo`, `done`, `deferred`. That is the whole answer to
   "what has been done, what is now, what is next".
3. **The newest logs** — files tagged `log`, named `YYYY-MM-DD-HHMM-<slug>.md`.
   The name sorts, so the newest is the last one alphabetically. Read backwards
   from there until you have enough context; do not read them all.

You do not need to read the refs or the prompts to know where things stand.
They are inputs, not state.

## The kinds

Every markdown file here declares exactly one kind in its frontmatter:

```yaml
---
id: 01J8ZC4H2K
tags: [log]
---
```

| tag | what it is |
| --- | ---------- |
| `about` | the session's record — normally one |
| `task` | a unit of work; also carries `status` and `order` |
| `log` | one sitting, written when that sitting ends |
| `prompt` | reusable text worth keeping |
| `ref` | a pointer at something that lives elsewhere |

A file may carry any other tags it likes; those are ordinary tags and the query
language reaches them. What is fixed is that one of the five above decides which
list the file appears in. A file with none of them is **unfiled** — that is not
an error, but it does mean nothing will surface it, so add a tag when you notice
one.

## The two directories

Only two, and the difference between them is about *versioning*, not about kind:

- **`artifacts/`** — output worth keeping. Versioned and synced. Put here
  anything a future reader should still be able to open.
- **`workspace/`** — scratch. Never synced, never backed up, and
  **keeper will not write here**. Assume everything in it is gone tomorrow.
  If something in here starts to matter, promote it to `artifacts/` and record
  the move in the `## Promote` table in `about.md`.

Do not create other directories. A new kind of thing is a new tag, not a new
folder — that is the whole point of this layout.

## Writing

- **End every sitting with a log file.** Name it
  `YYYY-MM-DD-HHMM-<short-slug>.md`, tag it `log`, and say what you did, what
  you decided, and what you left undone. The undone part is the one a future
  reader needs most and the one most often skipped.
- **Keep tasks current as you go.** Moving a task to `done` when you finish it
  costs nothing; reconstructing it a week later costs an afternoon.
- **Do not rewrite history.** A log entry that turned out to be wrong gets a
  correction in a newer log, not an edit. The record of what you believed at the
  time is part of the record.
- **Prefer a new small file over a long one.** The pool is queried by tag, so
  ten short logs are easier to navigate than one long one — and cheaper to read
  when only the last two matter.

## Ending a session

Two honest endings:

- **Archive it** — the work is finished or parked, and the record is worth
  keeping. Everything stays readable, filed under the year it closed.
- **Delete it** — there was nothing here worth keeping. Say so in a final log
  first if anyone else might wonder.

"Leave it open forever" is not one of them. An open session that nobody is
working on is a lie the board tells every time you look at it.
"#;

/// The record, as a body — frontmatter is added by the caller, which owns the
/// id and the date.
///
/// The `## Promote` table ships as a header-only scaffold. An empty table is
/// not noise: a session without it cannot record a promotion, and the promote
/// panel refuses to invent one because files are the truth (AD-110).
fn about_body(title: &str, date: &str) -> String {
    format!(
        "# {title}\n\
         \n\
         - **Date:** {date}\n\
         - **Tool/model:**\n\
         - **Goal:**\n\
         \n\
         ## Summary\n\
         \n\
         ## Decisions\n\
         \n\
         ## Promote\n\
         \n\
         | workspace | → artifacts | note |\n\
         | --------- | ----------- | ---- |\n"
    )
}

/// The seed log's body: the first sitting is the one that opened the session.
fn seed_log_body(title: &str) -> String {
    format!(
        "# Opened\n\
         \n\
         Session **{title}** created. Nothing has happened yet — this entry\n\
         exists so the log space is not empty and so the naming convention is\n\
         visible as an example.\n\
         \n\
         Replace or delete it; it is not load-bearing.\n"
    )
}

/// The seed prompt's body.
///
/// A prompt about *this* layout rather than a generic one: the most useful
/// reusable text in a brand-new session is the sentence that hands the folder
/// to an agent, and writing it out once is what makes `AGENTS.md` findable by
/// something that was never told to look for it.
const SEED_PROMPT_BODY: &str = "# Hand this session to an agent\n\
    \n\
    > You have been given a work-session folder. Read `AGENTS.md` in its root\n\
    > first — it states how the folder is organised. Then read `about.md`, the\n\
    > files tagged `task`, and the two newest files tagged `log`. Tell me where\n\
    > things stand before you change anything.\n\
    \n\
    Keep this prompt, adjust it, or delete it.\n";

/// The record's body alone, for a flat create whose pattern carries no
/// `about.md` to inherit a shape from.
///
/// The same renderer the full template uses, exposed rather than duplicated:
/// the fallback record and the seeded one are the same document, and a create
/// path with its own copy of the promote table is how the two start to drift.
/// Frontmatter is the caller's, because the caller owns the id and the lineage.
pub fn about_only(title: &str, date: &str) -> String {
    about_body(title, date)
}

/// Frontmatter for a file keeper itself authors.
///
/// keeper writes an `id` here and nowhere else: stamping identity into a file
/// someone else wrote is the rule this deliberately does not break — a file
/// without an `id` degrades to a path identity, and that is fine. A file keeper
/// *creates* has no prior identity to respect.
fn frontmatter(id: &str, date: &str, tag: &str) -> String {
    format!("---\nid: {id}\ncreated: {date}\ntags: [{tag}]\n---\n")
}

/// The default template, rendered for one new session.
///
/// `stamp` is `YYYY-MM-DD-HHMM` for the seed log's filename and `date` is
/// `YYYY-MM-DD` for the frontmatter; both come from the shell, because the
/// domain has no clock and because a replayed journal must reproduce the names
/// it recorded rather than invent today's.
///
/// `ids` supplies one id per file in order (about, log, prompt) for the same
/// reason: ULIDs are generated in the shell so a resumed plan replays the ids
/// it wrote (AD-111).
pub fn default_template(title: &str, date: &str, stamp: &str, ids: [&str; 3]) -> Vec<TemplateFile> {
    vec![
        TemplateFile {
            name: AGENTS.to_owned(),
            content: AGENTS_MD.to_owned(),
            // The contract is not one of the five kinds: it describes them.
            kind: None,
        },
        TemplateFile {
            name: ABOUT.to_owned(),
            // `about` is the kind tag; the frontmatter is what makes the About
            // space find it rather than its filename, which is only a
            // convention.
            content: format!(
                "{}{}",
                frontmatter(ids[0], date, "about"),
                about_body(title, date)
            ),
            kind: Some(KindTag::About),
        },
        TemplateFile {
            name: format!("{stamp}-opened.md"),
            content: format!(
                "{}{}",
                frontmatter(ids[1], date, "log"),
                seed_log_body(title)
            ),
            kind: Some(KindTag::Log),
        },
        TemplateFile {
            name: format!("{stamp}-handoff.md"),
            content: format!(
                "{}{}",
                frontmatter(ids[2], date, "prompt"),
                SEED_PROMPT_BODY
            ),
            kind: Some(KindTag::Prompt),
        },
    ]
}

/// What `_template/` gets when the operator adopts keeper's default (FR-268):
/// the navigation contract and an empty record, and deliberately **not** the
/// two seed files.
///
/// A template is a *skeleton*, and the seeds are not part of the skeleton —
/// they are examples keeper composes fresh for each new session, with that
/// session's own title and its own timestamp. Writing them into `_template/`
/// froze both: every session made from the adopted template inherited a log
/// saying it was the install's title that had been created, under a filename
/// stamped with the minute the operator pressed the button, *and* got a second
/// seed pair composed beside it. Neither is a template's job.
///
/// The record ships titled `<session title>` rather than with a real one,
/// because [`super::plan::skeleton_from`] copies only its `## ` headings into a
/// new session: the title line exists to be replaced, and saying so in the
/// placeholder is cheaper than a comment nobody reads. The operator can add
/// their own seed log or prompt here afterwards — and if they do, create
/// carries theirs and composes none, which is the same rule this fixes stated
/// from the other side.
pub fn zone_skeleton(date: &str, id: &str) -> Vec<TemplateFile> {
    vec![
        TemplateFile {
            name: AGENTS.to_owned(),
            content: AGENTS_MD.to_owned(),
            kind: None,
        },
        TemplateFile {
            name: ABOUT.to_owned(),
            content: format!(
                "{}{}",
                frontmatter(id, date, "about"),
                about_body("<session title>", date)
            ),
            kind: Some(KindTag::About),
        },
    ]
}

/// Whether a name a person typed folds to a directory name at all — the
/// question that has to be asked *before* [`crate::notes::naming::slug`], not
/// after it.
///
/// `slug` must always answer with a usable filename, because a note it refused
/// to name would be a note that was lost; a fold that leaves nothing therefore
/// comes back as `untitled` rather than empty. A *template* has the opposite
/// mandate: a name with nothing in it is a name to refuse, and once `slug` has
/// substituted its fallback there is no way left to tell "###" from a template
/// somebody really called *Untitled*. So the shell asks this first.
///
/// The fold's own verdict rather than a re-derived `chars().any(is_alphanumeric)`
/// — which is close and not the rule. The fold drops combining marks, and
/// several of those are alphabetic to `char::is_alphanumeric` (Devanagari vowel
/// signs among them), so a name written only in marks passes that test and still
/// slugs to nothing. Asking the stem is the only test that cannot drift from
/// what the slugger will actually do.
pub fn nameable(name: &str) -> bool {
    !crate::notes::naming::slug_stem(name).is_empty()
}

/// Write this template into a zone's `_template/` — the verb that updates the
/// drive's own copy (FR-268).
///
/// **Why a verb and not a one-off script.** The zone lives on a synced drive
/// whose history keeper owns; a template pasted in by hand is a write keeper's
/// watcher sees as somebody else's, and the operator's own rule is that nothing
/// hand-runs git in there. So the same plan/journal/exec path every other
/// lifecycle verb uses writes it, and the drive gets one commit with keeper's
/// provenance on it.
///
/// **It never clobbers silently.** A name already present is moved into the
/// zone's trash first and then rewritten, so an edited `AGENTS.md` — the file
/// most likely to have been improved by hand — is recoverable rather than
/// gone. `dest` is zone-relative (`_template`, or `_template/<name>` for a
/// named one) and `present` is what the caller found there, because the domain
/// opens nothing (AD-108).
pub fn compile_install(
    dest: &str,
    files: &[TemplateFile],
    present: &[String],
    trash_key: &str,
) -> super::plan::Plan {
    let mut steps = vec![super::plan::PlanStep::MkDir {
        path: dest.to_owned(),
    }];
    for file in files {
        let path = format!("{dest}/{}", file.name);
        if present.iter().any(|name| name == &file.name) {
            steps.push(super::plan::PlanStep::TrashFile {
                path: path.clone(),
                trash_key: trash_key.to_owned(),
            });
        }
        steps.push(super::plan::PlanStep::WriteFile {
            path,
            content: file.content.clone(),
        });
    }
    super::plan::Plan {
        verb: "template-install".to_owned(),
        session: dest.to_owned(),
        steps,
    }
}

/// Rename a named template — one directory move, and nothing else (FR-271).
///
/// `from` and `to` are zone-relative (`_template/<name>`), decided by the shell,
/// which is the only side that can see whether the source is a directory and
/// whether the destination is taken. The refusals live there for that reason
/// (AD-108); what is left here is the *shape* of the write, and it is one step.
///
/// **Why a compiled plan at all**, for a move a single `fs::rename` would do:
/// the zone is a synced drive whose history keeper owns, so the write has to
/// carry a journal row and land as one keeper-provenanced commit like every
/// other lifecycle verb. [`compile_unarchive`](super::plan::compile_unarchive)
/// is the precedent — also a one-`MoveDir` verb, also compiled rather than
/// hand-run — and following it is what keeps resume "re-run the remaining
/// steps" instead of a second recovery story for renames.
///
/// `MoveDir` is idempotent by contract: a resumed journal whose move already
/// happened succeeds, so the rename replays safely.
pub fn compile_rename(from: &str, to: &str) -> super::plan::Plan {
    super::plan::Plan {
        verb: "template-rename".to_owned(),
        session: from.to_owned(),
        steps: vec![super::plan::PlanStep::MoveDir {
            from: from.to_owned(),
            to: to.to_owned(),
        }],
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::notes::frontmatter::Frontmatter;
    use crate::notes::tags::note_tags;
    use crate::sessions::shape::{shape, KindTag, Shape};

    fn rendered() -> Vec<TemplateFile> {
        default_template(
            "Round two",
            "2026-08-14",
            "2026-08-14-1030",
            ["01J8A", "01J8B", "01J8C"],
        )
    }

    /// The template's own output must read as flat, or a session born from it
    /// would be parsed by the wrong reader on its first render.
    #[test]
    fn a_session_built_from_the_default_reads_as_flat() {
        let names: Vec<String> = rendered().into_iter().map(|file| file.name).collect();
        assert_eq!(shape(&names), Shape::Flat);
        assert!(
            !names.iter().any(|name| name == "README.md"),
            "the navigation file is AGENTS.md; a README would be a second answer"
        );
    }

    /// Every file the template writes declares its kind, because a file that
    /// did not would land in `unfiled` — in a brand-new session, from keeper's
    /// own template, which would teach the wrong lesson on day one.
    #[test]
    fn every_seeded_file_declares_a_kind() {
        let expected = [
            None,
            Some(KindTag::About),
            Some(KindTag::Log),
            Some(KindTag::Prompt),
        ];
        for (file, want) in rendered().into_iter().zip(expected) {
            let (fm, body_at) = Frontmatter::parse(&file.content);
            let tags = note_tags(&fm, &file.content[body_at..]);
            assert_eq!(KindTag::of(&tags), want, "{} declares {want:?}", file.name);
        }
    }

    /// The flat pool sorts by filename, so the seed log has to carry the stamp
    /// its own rule describes — otherwise the first example in every new
    /// session contradicts `AGENTS.md`.
    #[test]
    fn the_seed_log_is_named_for_the_convention_it_documents() {
        let files = rendered();
        let log = &files[2];
        assert_eq!(log.name, "2026-08-14-1030-opened.md");
        assert!(
            AGENTS_MD.contains("YYYY-MM-DD-HHMM-<slug>.md"),
            "the rule and the example must be the same rule"
        );
    }

    /// An empty promote table is the zone's scaffold, and the record has to
    /// carry it or a promotion has nowhere to be written.
    #[test]
    fn the_record_ships_the_promote_scaffold_and_the_title() {
        let files = rendered();
        let about = &files[1];
        assert_eq!(about.name, "about.md");
        assert!(about.content.contains("# Round two"));
        assert!(about.content.contains("- **Date:** 2026-08-14"));
        assert!(about.content.contains("## Promote"));
        assert!(about.content.contains("| workspace | → artifacts | note |"));
    }

    /// The ids are the caller's, in order, so a replayed journal writes the
    /// same three files it wrote the first time.
    #[test]
    fn the_caller_owns_the_ids_and_the_clock() {
        let files = rendered();
        assert!(files[1].content.contains("id: 01J8A"));
        assert!(files[2].content.contains("id: 01J8B"));
        assert!(files[3].content.contains("id: 01J8C"));
        // Rendering twice with the same inputs is byte-identical: nothing in
        // here reads a clock or a random source.
        assert_eq!(rendered(), files);
    }

    /// Installing over an existing template must be recoverable: the file most
    /// likely to be already there is the one somebody edited on purpose.
    #[test]
    fn installing_over_an_edited_template_trashes_before_it_writes() {
        let files = rendered();
        let plan = compile_install(
            "_template",
            &files,
            &[AGENTS.to_owned(), "stray.md".to_owned()],
            "01J8Z",
        );
        assert_eq!(plan.verb, "template-install");
        assert_eq!(plan.session, "_template");
        assert_eq!(
            plan.steps.first(),
            Some(&crate::sessions::plan::PlanStep::MkDir {
                path: "_template".to_owned()
            })
        );
        // The one name that was present is trashed, and immediately rewritten.
        let trashes: Vec<&String> = plan
            .steps
            .iter()
            .filter_map(|step| match step {
                crate::sessions::plan::PlanStep::TrashFile { path, .. } => Some(path),
                _ => None,
            })
            .collect();
        assert_eq!(trashes, vec!["_template/AGENTS.md"]);
        // `stray.md` is not ours: an install replaces this template's files and
        // leaves everything else in the directory alone.
        let writes: Vec<&String> = plan
            .steps
            .iter()
            .filter_map(|step| match step {
                crate::sessions::plan::PlanStep::WriteFile { path, .. } => Some(path),
                _ => None,
            })
            .collect();
        assert_eq!(writes.len(), files.len());
        assert!(!writes.iter().any(|path| path.ends_with("stray.md")));
    }

    /// A rename is a location change and nothing else: one move, the two names
    /// in the two fields, and a verb the journal can name. Anything extra in
    /// this plan would be a rename that also edited the template it moved.
    #[test]
    fn a_rename_is_one_move_and_nothing_else() {
        let plan = compile_rename("_template/interview", "_template/kick-off");
        assert_eq!(plan.verb, "template-rename");
        // The session a journal row is about is the path *before* the plan runs.
        assert_eq!(plan.session, "_template/interview");
        assert_eq!(
            plan.steps,
            vec![crate::sessions::plan::PlanStep::MoveDir {
                from: "_template/interview".to_owned(),
                to: "_template/kick-off".to_owned(),
            }],
            "from and to must not be swapped: this move is not reversible by resume"
        );
    }

    /// The trap [`nameable`] exists for, asserted from both sides: the slugger
    /// answers `untitled` for a name with nothing in it, so a caller that
    /// slugged first and then tested for empty would mint `_template/untitled`
    /// and call it the operator's name. That shipped once.
    #[test]
    fn a_name_with_nothing_in_it_is_refused_before_the_fallback_hides_it() {
        assert!(!nameable("###"));
        assert!(!nameable("   "));
        assert!(!nameable("🎉"));
        // …and this is why the test cannot be "did the slug come back empty".
        assert_eq!(crate::notes::naming::slug("###"), "untitled");
        // A name somebody really typed still passes, fallback word included.
        assert!(nameable("Kick Off"));
        assert!(nameable("untitled"));
        assert!(nameable("v1.2"));
    }

    /// `AGENTS.md` is a contract with a reader who has no other context, so the
    /// facts it must state are worth asserting rather than trusting to review.
    #[test]
    fn the_navigation_file_states_the_load_bearing_rules() {
        for required in [
            "artifacts/",
            "workspace/",
            "keeper will not write here",
            "in-preparation",
            "unfiled",
        ] {
            assert!(
                AGENTS_MD.contains(required),
                "AGENTS.md must state {required:?} — an agent handed this folder has no other source"
            );
        }
        for kind in ["about", "task", "log", "prompt", "ref"] {
            assert!(
                AGENTS_MD.contains(&format!("`{kind}`")),
                "AGENTS.md must name the {kind} kind"
            );
        }
    }

    /// Found by driving the real UI: pressing "Write keeper's template into
    /// this zone" wrote the two seed files into `_template/`, so every session
    /// created from the adopted template inherited a log reading "Session **New
    /// session** created" — the install-time title, frozen — under a filename
    /// stamped with the minute the button was pressed.
    ///
    /// A template is a skeleton. The seeds are examples keeper composes per
    /// create, with that session's own title; they are not part of it.
    #[test]
    fn the_zone_skeleton_carries_no_seed_files_to_freeze() {
        let files = zone_skeleton("2026-08-14", "01J8A");
        let names: Vec<&str> = files.iter().map(|file| file.name.as_str()).collect();
        assert_eq!(names, vec![AGENTS, ABOUT]);

        // Nothing here is stamped with an install-time minute, and nothing
        // states a title that a later session would inherit as its own.
        for file in &files {
            assert!(
                !file.name.contains("-opened.md") && !file.name.contains("-handoff.md"),
                "{} is a seed and must not live in _template/",
                file.name
            );
            assert!(
                !file.content.contains("created. Nothing has happened yet"),
                "{} freezes a title into the template",
                file.name
            );
        }

        // Still a flat session by its own top-level names, so a create from it
        // takes the flat path — a skeleton that read as folder-shaped would be
        // a worse bug than the one this fixes.
        let top: Vec<String> = files.iter().map(|file| file.name.clone()).collect();
        assert_eq!(shape(&top), Shape::Flat);
    }

    /// The other half of the same defect: the create path deduped what the
    /// pattern already supplies **by filename**, and a seed's filename carries
    /// a `YYYY-MM-DD-HHMM` stamp — so a template holding a log and keeper
    /// composing one produced two different names for the same kind and the
    /// test never fired. The kind is what may not be duplicated.
    #[test]
    fn a_seed_is_identified_by_its_kind_not_its_stamped_name() {
        let monday = default_template("A", "2026-08-10", "2026-08-10-0900", ["1", "2", "3"]);
        let friday = default_template("B", "2026-08-14", "2026-08-14-1700", ["4", "5", "6"]);

        let log_of = |files: &[TemplateFile]| {
            files
                .iter()
                .find(|file| file.kind == Some(KindTag::Log))
                .expect("the default template ships a seed log")
                .name
                .clone()
        };
        // Same kind, different names — which is exactly why a name comparison
        // could not see the duplicate.
        assert_ne!(log_of(&monday), log_of(&friday));

        // Every file declares what it is, and the kinds are the ones the pool
        // reader would derive from the frontmatter each file actually carries.
        for file in &friday {
            let derived = {
                let (fm, _) = Frontmatter::parse(&file.content);
                KindTag::of(&note_tags(&fm, &file.content))
            };
            assert_eq!(
                file.kind, derived,
                "{}'s declared kind must match the tag in its own bytes",
                file.name
            );
        }
    }
}
