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

/// One entry of the template, ready to write: bytes, or a directory to make.
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
    /// A **directory** the skeleton names rather than bytes it writes (FR-288):
    /// [`compile_install`] emits a `MkDir` for it and never a `WriteFile`.
    ///
    /// A flag beside the bytes rather than an enum in place of them, because the
    /// installer is the only consumer that ever meets a directory:
    /// [`default_template`]'s entries are all files, and the create path reads
    /// `content` as a field. An enum body would cost every file consumer a match
    /// to express a state only the skeleton has.
    pub dir: bool,
}

impl TemplateFile {
    /// A directory the skeleton carries rather than a file it writes.
    ///
    /// A constructor rather than a literal at each site, so nobody has to write
    /// `content: String::new()` and mean "there are no bytes" — a pair that
    /// would be a lie the moment somebody read it as a file.
    #[must_use]
    pub fn directory(name: &str) -> Self {
        Self {
            name: name.to_owned(),
            content: String::new(),
            kind: None,
            dir: true,
        }
    }
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

**`artifacts/`** and **`workspace/`** are in every session, and the difference
between them is about *versioning*, not about kind:

- **`artifacts/`** — output worth keeping. Versioned and synced. Put here
  anything a future reader should still be able to open.
- **`workspace/`** — scratch. Never synced, never backed up, and
  **keeper will not write here**. Assume everything in it is gone tomorrow.
  If something in here starts to matter, promote it to `artifacts/` and record
  the move in the `## Promote` table in `about.md`.

A new *kind* of thing is a new tag, not a new folder — that is the whole point
of this layout, and it is the rule that does not bend. A directory is still good
for one thing: being a **container**. Somewhere for what is not markdown, or a
folder you made on purpose because there are thirty of something. keeper's own
*New folder* makes one, markdown inside it is read exactly as markdown in the
root is, and each file's kind is still the tag in its own frontmatter — so a
`logs/` whose files carry no `log` tag is a directory nothing lists, which is the
mistake the rule above exists to prevent. The two directories named here are the
exception to the reading: neither is scanned, because one holds output and the
other is scratch.

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
            dir: false,
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
            dir: false,
        },
        TemplateFile {
            name: format!("{stamp}-opened.md"),
            content: format!(
                "{}{}",
                frontmatter(ids[1], date, "log"),
                seed_log_body(title)
            ),
            kind: Some(KindTag::Log),
            dir: false,
        },
        TemplateFile {
            name: format!("{stamp}-handoff.md"),
            content: format!(
                "{}{}",
                frontmatter(ids[2], date, "prompt"),
                SEED_PROMPT_BODY
            ),
            kind: Some(KindTag::Prompt),
            dir: false,
        },
    ]
}

/// What `_template/` gets when the operator adopts keeper's default (FR-268):
/// the navigation contract, an empty record and the two directories every
/// session has — and deliberately **not** the two seed files.
///
/// A template is a *skeleton*, and the seeds are not part of the skeleton —
/// they are examples keeper composes fresh for each new session, with that
/// session's own title and its own timestamp. Writing them into `_template/`
/// froze both: every session made from the adopted template inherited a log
/// saying it was the install's title that had been created, under a filename
/// stamped with the minute the operator pressed the button, *and* got a second
/// seed pair composed beside it. Neither is a template's job.
///
/// **The two directories are** part of the skeleton (FR-288), and they are the
/// list [`super::pattern::standard_dirs`] already keeps rather than a second
/// copy of it: what a create of this shape forces into a new session and what
/// keeper's own template ships have to be the same two names, and a skeleton
/// that named its own pair would be the copy that drifts. `Shape::Flat` because
/// this skeleton *is* flat by its own top-level names — `refs/` and `prompts/`
/// are tag queries in the flat contract, and creating them empty here would put
/// in every new session exactly the two directories its `AGENTS.md` tells the
/// reader not to make.
///
/// The record ships titled `<session title>` rather than with a real one,
/// because [`super::plan::skeleton_from`] copies only its `## ` headings into a
/// new session: the title line exists to be replaced, and saying so in the
/// placeholder is cheaper than a comment nobody reads. The operator can add
/// their own seed log or prompt here afterwards — and if they do, create
/// carries theirs and composes none, which is the same rule this fixes stated
/// from the other side.
pub fn zone_skeleton(date: &str, id: &str) -> Vec<TemplateFile> {
    let mut entries = vec![
        TemplateFile {
            name: AGENTS.to_owned(),
            content: AGENTS_MD.to_owned(),
            kind: None,
            dir: false,
        },
        TemplateFile {
            name: ABOUT.to_owned(),
            content: format!(
                "{}{}",
                frontmatter(id, date, "about"),
                about_body("<session title>", date)
            ),
            kind: Some(KindTag::About),
            dir: false,
        },
    ];
    entries.extend(
        super::pattern::standard_dirs(super::shape::Shape::Flat)
            .iter()
            .copied()
            .map(TemplateFile::directory),
    );
    entries
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
///
/// **A directory entry is made, never trashed-and-rewritten** (FR-288). `MkDir`
/// is idempotent by contract, so an `artifacts/` that is already there needs
/// nothing done to it — and the trash-then-write branch above must not reach it,
/// because a template whose `artifacts/` holds the operator's own files would
/// have the whole directory moved into `.keeper/trash/` for the crime of being
/// present. Recoverable is not the same as untouched.
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
        if file.dir {
            steps.push(super::plan::PlanStep::MkDir { path });
            continue;
        }
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

// ---------------------------------------------------------------------------
// What is INSIDE a template: its files and its folders (FR-284)
// ---------------------------------------------------------------------------
//
// The verbs above act on a template *directory* — install one, rename one. These
// four act on what is in it, and they live here rather than in
// [`super::files`] because that module's rules are a *session's*: it refuses
// `workspace/` because scratch is fenced off from every write (AD-113), and it
// refuses deleting `AGENTS.md`/`about.md` because those two names are what
// [`super::shape::shape`] reads to decide which contract a session follows.
// Neither reason survives the trip into `_template/`: a template's `workspace/`
// is a skeleton directory a create copies, and a template has no shape to flip —
// deleting its `about.md` only changes what the next create carries, which
// [`about_only`] already covers. Reusing those checks would inherit two
// refusals whose reasons are false here, which is worse than restating the one
// rule that is true: stay inside the template.

/// What the shell found at the path an entry verb was pointed at.
///
/// The one fact about the target the domain cannot work out for itself
/// (AD-108), and the only thing the same `rel` compiles differently for: a
/// [`super::plan::PlanStep::MoveFile`] or a `MoveDir`, a `TrashFile` or a
/// `TrashDir`. A bool would have read as a flag at every call site.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EntryKind {
    File,
    Dir,
}

/// Everything the entry verbs refuse, with the sentence the operator reads.
///
/// [`super::files::FileVerbError`]'s counterpart, and a separate type for the
/// reason the section header above gives: these are a *template's* refusals, and
/// two of that one's four are about a session's contract rather than a path.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum EntryError {
    #[error(
        "{rel} is not a path inside this template. keeper composes a template's paths from the \
         template's own directory and will not follow one back out of it."
    )]
    Outside { rel: String },

    #[error(
        "that is the template's own directory rather than something inside it. A whole template \
         is made with New template and renamed with the template's own rename — these verbs only \
         touch what is in one."
    )]
    Root,

    #[error(
        "{rel} is a dotfile. The Templates room does not list them and a create does not copy \
         them — .DS_Store above all — so a verb able to name one would act on a file the room \
         says is not there. Remove it in Finder if it is in your way."
    )]
    Dotfile { rel: String },

    #[error(
        "keeper creates .md, .csv and .json files — {rel} is none of those. A template is copied \
         into every session made from it, so a create button here authors exactly what a create \
         button in a session authors, and nothing else."
    )]
    Extension { rel: String },

    /// A rename that would carry a file OUT of the closed set a create is held
    /// to — and only then.
    ///
    /// The relaxation this narrows is deliberate and stays: a rename re-labels
    /// bytes somebody else put in the template, so `logo.png` → `Logo Mark`
    /// keeps its `.png` and is nobody's business. What it may not do is *change*
    /// an extension out of the set, because `about.md` → `about.sh` authors a
    /// `.sh` in a directory every new session copies verbatim — through a keeper
    /// verb, which is exactly what [`Self::Extension`] refuses one variant up.
    #[error(
        "{current} cannot become {rel}: keeper writes .md, .csv and .json files, and a rename \
         that carries one out of that set authors a file a create here would have refused. Type \
         a name with no extension to keep the extension {current} has."
    )]
    ExtensionChanged { current: String, rel: String },

    #[error(
        "\"{typed}\" has nothing in it a file or a folder can be named after — it needs letters \
         or digits."
    )]
    Unnameable { typed: String },
}

/// One template-relative path, normalised, or the refusal for a path that is not
/// inside the template at all.
///
/// **The one guard all four entry verbs share**, and public because the shell
/// asks it first: it has to stat the target to learn whether it is a file or a
/// directory, and that means joining `rel` onto a zone root *before* a compiler
/// has seen it. [`super::files::check_rel`] is public for the same reason and
/// still called again inside its own compilers; so is this. A guard the caller
/// can skip is a guard.
///
/// What it refuses, in this order because the more urgent fact comes first: a
/// path that leaves the template (`..`, an absolute path, a backslash — Windows'
/// separator, which would otherwise smuggle `..\` past a `/`-split), the
/// template root itself, then a dotfile. Traversal outranks the dotfile rule
/// because `../.DS_Store` is an escape first and a stray second.
///
/// # Errors
/// [`EntryError::Outside`], [`EntryError::Root`] or [`EntryError::Dotfile`].
pub fn entry_rel(rel: &str) -> Result<String, EntryError> {
    let typed = rel.trim();
    if typed.starts_with('/') || typed.contains('\\') {
        return Err(EntryError::Outside {
            rel: typed.to_owned(),
        });
    }
    // A trailing slash is how a tree row spells a folder, not a second segment.
    let trimmed = typed.trim_end_matches('/');
    if trimmed.is_empty() || trimmed == "." {
        return Err(EntryError::Root);
    }
    if trimmed
        .split('/')
        .any(|part| part.is_empty() || part == "." || part == "..")
    {
        return Err(EntryError::Outside {
            rel: trimmed.to_owned(),
        });
    }
    if trimmed.split('/').any(|part| part.starts_with('.')) {
        return Err(EntryError::Dotfile {
            rel: trimmed.to_owned(),
        });
    }
    Ok(trimmed.to_owned())
}

/// The name one entry inside a template gets from a name a person typed: the
/// **stem** folds to a slug, and the **extension survives**.
///
/// [`crate::notes::naming::slug`] alone is wrong here, and wrong in a way nobody
/// would notice until a template stopped opening: it folds every character that
/// is not alphanumeric, the dot included, so `Kick Off.md` becomes `kick-off-md`
/// — a file with no extension, which the room still lists and which no editor
/// reads as markdown. [`super::files::new_stamped`] and
/// [`super::files::new_named`] do not help: both take the extension as a
/// separate argument from a closed enum, because both are naming a file whose
/// kind was picked from a menu. Here it is part of what was typed.
///
/// A tail counts as an extension only when it is ASCII alphanumeric, so
/// `v1.2 notes` keeps its dot inside the stem rather than acquiring a
/// `.2-notes`. It is lowercased, through the same fold the stem gets: the closed
/// extension set folds case, the viewer registry keys on the extension, and two
/// spellings of one kind in a directory a create copies verbatim is a difference
/// nobody asked for.
///
/// `fallback_ext` is the extension the entry already has, for a rename whose
/// typed name carries none — a rename renames, it does not decide what kind of
/// file this is. `None` for a folder, and for a create, where an extension the
/// person did not type is one keeper would be inventing.
///
/// `None` when the fold leaves nothing: [`nameable`]'s rule, asked through the
/// same fold and for its reason — `slug` substitutes `untitled` rather than
/// refusing, and a template entry somebody called `###` is one to refuse.
#[must_use]
pub fn entry_name(typed: &str, fallback_ext: Option<&str>) -> Option<String> {
    let typed = typed.trim();
    let (stem, ext) = match typed.rsplit_once('.') {
        Some((stem, ext))
            if !stem.trim().is_empty()
                && !ext.is_empty()
                && ext.chars().all(|c| c.is_ascii_alphanumeric()) =>
        {
            (stem, Some(ext.to_ascii_lowercase()))
        }
        _ => (typed, fallback_ext.map(str::to_ascii_lowercase)),
    };
    let stem = crate::notes::naming::slug_stem(stem);
    if stem.is_empty() {
        return None;
    }
    Some(match ext {
        Some(ext) => format!("{stem}.{ext}"),
        None => stem,
    })
}

/// A compiled entry verb: the plan, and the template-relative path it lands on.
///
/// The path travels back rather than being re-derived, because the shell needs
/// it twice — to ask the disk whether the destination is taken before the plan
/// runs, and to answer the webview with a subpath that opens the result — and a
/// second copy of "slug the stem, keep the extension" would be a second answer
/// to the question [`entry_name`] exists to answer once (AD-65).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EntryPlan {
    pub plan: super::plan::Plan,
    /// Template-relative, normalised, and the *result* of the verb: the new name
    /// for a rename, the created path for a create, the trashed path for a
    /// delete.
    pub rel: String,
}

/// Split a normalised template-relative path into its parent and its last
/// segment, and rejoin the parent onto a new last segment.
///
/// Only the last segment is a name the person is minting; the ones in front of
/// it address directories that are already on the drive, so they travel
/// verbatim. That is the shell's `template_at`/`template_mint` split one level
/// down — a hand-made `_template/Interview Kit/` addresses, and its files do too.
fn rejoin(rel: &str, name: &str) -> String {
    match rel.rsplit_once('/') {
        Some((parent, _)) => format!("{parent}/{name}"),
        None => name.to_owned(),
    }
}

/// Make one file inside a template (FR-284).
///
/// `dir` is the template's zone-relative directory (`_template`, or
/// `_template/<name>`) and `rel` is template-relative; the join happens here so
/// no caller composes a zone path (AD-65).
///
/// **This verb addresses a folder; it never mints one.** The plan is one
/// `WriteFile`, `refs/inputs.md` included. It used to lead with a `MkDir` of the
/// verbatim parent, and that minted a directory [`compile_dir_new`] could never
/// have made: only the last segment goes through [`entry_name`], so
/// `Interview Kit/Kick Off.md` created a folder literally called `Interview Kit`
/// while the same words typed into *New folder* fold to `interview-kit`. One
/// room cannot spell a folder name two ways, and the fix keeps the rule
/// [`rejoin`] already states rather than adding a second fold: parents
/// *address* what is on the drive, and every directory keeper mints is minted by
/// the folder verb. A parent that is not there is therefore refused — by the
/// shell, the only side that can look (AD-108), exactly as a collision is.
///
/// **The file starts empty, and that is a decision.**
/// [`super::files::render_new`] stamps `id`, `created` and a title into a new
/// *session* file, and an id frozen into a template is the bug
/// [`zone_skeleton`] exists to prevent one level down: every session made from
/// the template would inherit the same identity, and the index would see one id
/// on n files. A `.json` is the one exception, for
/// [`super::files::render_new`]'s own reason — an empty file is not valid JSON,
/// so the first tool to read it would fail on a file keeper wrote.
///
/// # Errors
/// [`EntryError::Extension`] outside the closed set, [`EntryError::Unnameable`]
/// for a name that folds to nothing, or whatever [`entry_rel`] refuses.
pub fn compile_file_new(dir: &str, rel: &str) -> Result<EntryPlan, EntryError> {
    let rel = entry_rel(rel)?;
    let typed = rel.rsplit_once('/').map_or(rel.as_str(), |(_, name)| name);
    let name = entry_name(typed, None).ok_or_else(|| EntryError::Unnameable {
        typed: typed.to_owned(),
    })?;
    let kind = super::files::NewFileKind::parse(name.rsplit_once('.').map_or("", |(_, ext)| ext))
        .ok_or_else(|| EntryError::Extension { rel: name.clone() })?;
    let landed = rejoin(&rel, &name);

    Ok(EntryPlan {
        plan: super::plan::Plan {
            verb: "template-file-new".to_owned(),
            session: dir.to_owned(),
            steps: vec![super::plan::PlanStep::WriteFile {
                path: format!("{dir}/{landed}"),
                content: match kind {
                    super::files::NewFileKind::Json => "{}\n".to_owned(),
                    super::files::NewFileKind::Markdown | super::files::NewFileKind::Csv => {
                        String::new()
                    }
                },
            }],
        },
        rel: landed,
    })
}

/// Make one folder inside a template (FR-284) — one `MkDir`, and nothing else.
///
/// **Idempotent by contract**, because `MkDir` is
/// ([`super::plan::PlanStep::MkDir`]): asking for `artifacts/` in a template
/// that has it succeeds and changes nothing. That is the right answer rather
/// than a refusal — the four skeleton directories are exactly the folders an
/// operator will type without checking, and "it is already there" is not a
/// failure to report.
///
/// One step for a nested path too, and the parent in front of it is not this
/// verb's to mint either: `refs/inputs` *addresses* `refs/` exactly as
/// `refs/inputs.md` does in [`compile_file_new`], so the only segment keeper
/// folds is the last one and the shell refuses a parent that is not on the
/// drive. Left to `create_dir_all`, an absent `Interview Kit/` would be spelled
/// verbatim here while the same words typed as a folder name fold to
/// `interview-kit` — one room with two spellings of one directory.
///
/// # Errors
/// [`EntryError::Unnameable`], or whatever [`entry_rel`] refuses. No extension
/// rule: a folder called `v1.2` is a folder.
pub fn compile_dir_new(dir: &str, rel: &str) -> Result<EntryPlan, EntryError> {
    let rel = entry_rel(rel)?;
    let typed = rel.rsplit_once('/').map_or(rel.as_str(), |(_, name)| name);
    let name = entry_name(typed, None).ok_or_else(|| EntryError::Unnameable {
        typed: typed.to_owned(),
    })?;
    let landed = rejoin(&rel, &name);
    Ok(EntryPlan {
        plan: super::plan::Plan {
            verb: "template-dir-new".to_owned(),
            session: dir.to_owned(),
            steps: vec![super::plan::PlanStep::MkDir {
                path: format!("{dir}/{landed}"),
            }],
        },
        rel: landed,
    })
}

/// Rename one file or folder inside a template (FR-284) — one move, in place.
///
/// **Why this is allowed here when `docs/sessions.md` refuses it for a session's
/// files.** That refusal is about link identity: a hand-written file has no `id`
/// and is identified by its path, so renaming it breaks the pins pointing at it.
/// A template has no such graph — nothing pins a template's files, and
/// [`super::plan::compile_create_shaped`] *copies* them rather than referencing
/// them, so the only consumer of the name is a copy that reads the directory
/// fresh. The room already renames a whole template directory, moving every file
/// inside it at once; renaming one of them is strictly less disruptive than the
/// verb it already has.
///
/// The entry stays where it is — only its last segment changes — because moving
/// a file *between* a template's folders is a different verb with a different
/// refusal (a destination in another directory can collide with a name this
/// caller never read), and half of it would be worse than none.
///
/// **A rename may keep any extension, and may not change one out of the closed
/// set.** `logo.png` → `Logo Mark` keeps its `.png`: those bytes are already in
/// the template and already travel into every session made from it, so the set —
/// which governs what keeper *authors* — has nothing to protect there.
/// `about.md` → `about.sh` is the other case, and it authors a `.sh` through a
/// keeper verb in a directory every create copies verbatim, which is precisely
/// what [`compile_file_new`] refuses. So the question is asked exactly when the
/// current extension is inside the set and the typed one is outside it.
///
/// # Errors
/// [`EntryError::Unnameable`], [`EntryError::ExtensionChanged`] for a rename out
/// of the closed set, or whatever [`entry_rel`] refuses. A collision is **not**
/// decided here: the domain opens nothing (AD-108), so the shell asks the disk
/// and `MoveFile`/`MoveDir` refuse it again as they run.
pub fn compile_entry_rename(
    dir: &str,
    rel: &str,
    typed: &str,
    kind: EntryKind,
) -> Result<EntryPlan, EntryError> {
    let rel = entry_rel(rel)?;
    let current = rel.rsplit_once('/').map_or(rel.as_str(), |(_, name)| name);
    // A file keeps the extension it has when the typed name carries none; a
    // folder has none to keep.
    let fallback = match kind {
        EntryKind::File => current.rsplit_once('.').map(|(_, ext)| ext),
        EntryKind::Dir => None,
    };
    let name = entry_name(typed, fallback).ok_or_else(|| EntryError::Unnameable {
        typed: typed.to_owned(),
    })?;
    // The extension may be KEPT freely and not CHANGED out of the set — see this
    // function's own note. Both sides are asked through the closed enum a create
    // is held to, so there is no second spelling of the set here, and `parse`
    // folds case for the same reason `entry_name` does.
    if kind == EntryKind::File
        && current
            .rsplit_once('.')
            .and_then(|(_, ext)| super::files::NewFileKind::parse(ext))
            .is_some()
        && name
            .rsplit_once('.')
            .and_then(|(_, ext)| super::files::NewFileKind::parse(ext))
            .is_none()
    {
        return Err(EntryError::ExtensionChanged {
            current: current.to_owned(),
            rel: name,
        });
    }
    let landed = rejoin(&rel, &name);
    let from = format!("{dir}/{rel}");
    let to = format!("{dir}/{landed}");
    Ok(EntryPlan {
        plan: super::plan::Plan {
            verb: "template-entry-rename".to_owned(),
            session: dir.to_owned(),
            steps: vec![match kind {
                EntryKind::File => super::plan::PlanStep::MoveFile { from, to },
                EntryKind::Dir => super::plan::PlanStep::MoveDir { from, to },
            }],
        },
        rel: landed,
    })
}

/// Remove one file or folder from a template (FR-284) — a trash move,
/// recoverable.
///
/// `TrashFile`/`TrashDir` and never a `remove_file` or a `remove_dir_all`, for
/// the reason the zone's trash exists: a template is a thing somebody wrote, and
/// a folder delete that erases bytes is one nobody presses without making a copy
/// first. A directory goes whole, which is what makes it recoverable whole.
///
/// The template root is refused by [`entry_rel`] rather than by a check here:
/// "delete the template" is a different verb with a different confirmation, and
/// an empty `rel` reaching a `TrashDir` would be that verb by accident.
///
/// # Errors
/// Whatever [`entry_rel`] refuses — [`EntryError::Root`] above all.
pub fn compile_entry_delete(
    dir: &str,
    rel: &str,
    kind: EntryKind,
    trash_key: &str,
) -> Result<EntryPlan, EntryError> {
    let rel = entry_rel(rel)?;
    let path = format!("{dir}/{rel}");
    let trash_key = trash_key.to_owned();
    Ok(EntryPlan {
        plan: super::plan::Plan {
            verb: "template-entry-delete".to_owned(),
            session: dir.to_owned(),
            steps: vec![match kind {
                EntryKind::File => super::plan::PlanStep::TrashFile { path, trash_key },
                EntryKind::Dir => super::plan::PlanStep::TrashDir { path, trash_key },
            }],
        },
        rel,
    })
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

    /// Row 9. *Write keeper's template into this zone* installs the two
    /// directories as well as the two files (FR-288) — the owner's own
    /// `_template/` had them by luck, and a template without them hands every
    /// session made from it a shape its own `AGENTS.md` describes and it does not
    /// have.
    ///
    /// The sharp half is the second install: a directory already on disk is in
    /// `present`, and the trash-then-write branch must not reach it. `MkDir` is
    /// idempotent, while a `TrashFile` aimed at `artifacts/` would move the
    /// operator's own output into `.keeper/trash/` for the crime of being there.
    #[test]
    fn an_install_makes_the_two_directories_and_never_trashes_one() {
        let files = zone_skeleton("2026-08-14", "01J8A");
        let plan = compile_install(
            "_template",
            &files,
            // Everything already present, directories included — the second
            // press of the button, and the case that must not destroy anything.
            &[
                AGENTS.to_owned(),
                ABOUT.to_owned(),
                "artifacts".to_owned(),
                "workspace".to_owned(),
            ],
            "01J8Z",
        );
        let mkdirs: Vec<&String> = plan
            .steps
            .iter()
            .filter_map(|step| match step {
                crate::sessions::plan::PlanStep::MkDir { path } => Some(path),
                _ => None,
            })
            .collect();
        assert_eq!(
            mkdirs,
            vec!["_template", "_template/workspace", "_template/artifacts"],
            "the template's own directory, then the two every session has"
        );
        let trashes: Vec<&String> = plan
            .steps
            .iter()
            .filter_map(|step| match step {
                crate::sessions::plan::PlanStep::TrashFile { path, .. } => Some(path),
                _ => None,
            })
            .collect();
        assert_eq!(
            trashes,
            vec!["_template/AGENTS.md", "_template/about.md"],
            "only the two FILES are recoverable-then-rewritten"
        );
        // And no bytes are invented for a directory: no `.gitkeep`, no empty
        // file standing in for one.
        let writes: Vec<&String> = plan
            .steps
            .iter()
            .filter_map(|step| match step {
                crate::sessions::plan::PlanStep::WriteFile { path, .. } => Some(path),
                _ => None,
            })
            .collect();
        assert_eq!(writes, vec!["_template/AGENTS.md", "_template/about.md"]);
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
    ///
    /// **Including the amended sentence (Story 51.2).** It used to read *"Do not
    /// create other directories"*, which a *New folder* button on a flat session
    /// contradicts outright — so the sentence now permits a container and a
    /// directory the operator makes deliberately, and keeps the rule the whole
    /// layout rests on: the kind is the tag. Both halves are asserted, because
    /// an amendment nobody pins is a sentence that reverts.
    #[test]
    fn the_navigation_file_states_the_load_bearing_rules() {
        for required in [
            "artifacts/",
            "workspace/",
            "keeper will not write here",
            "in-preparation",
            "unfiled",
            // The half that must survive the amendment.
            "not a new folder",
            // …and the half the amendment adds: keeper itself makes one now.
            "*New folder*",
        ] {
            assert!(
                AGENTS_MD.contains(required),
                "AGENTS.md must state {required:?} — an agent handed this folder has no other source"
            );
        }
        assert!(
            !AGENTS_MD.contains("Do not create other directories"),
            "the flat prohibition is amended, not restored: keeper offers New folder (FR-287), and \
             a contract that forbids what the app's own button does is one an agent reads as noise"
        );
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
        // Row 8. Two files and the two directories every session has (FR-288) —
        // the pair `standard_dirs` already keeps for this shape, so the skeleton
        // and a create cannot disagree about which two.
        assert_eq!(names, vec![AGENTS, ABOUT, "workspace", "artifacts"]);
        assert_eq!(
            &names[2..],
            crate::sessions::pattern::standard_dirs(Shape::Flat),
            "the skeleton's directories are the create's, not a second list"
        );
        for dir in &files[2..] {
            assert!(dir.dir, "{} is a directory to make", dir.name);
            assert!(
                dir.content.is_empty() && dir.kind.is_none(),
                "a directory has no bytes and no kind — .gitkeep is for file-list copies"
            );
        }

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

    // -----------------------------------------------------------------------
    // A template's own files and folders (FR-284) — the spec's matrix, rows
    // 3-12 and 15, at the plan level. Rows 1-2 are the executor's
    // (`keeper/src/sessions_exec.rs`) and rows 13-16 the shell's and the room's.
    // -----------------------------------------------------------------------

    /// The template `_template/test1` the owner actually has, as a plan prefix.
    const NAMED: &str = "_template/test1";

    /// Row 3. A create writes one file, and it writes nothing into it: an `id`
    /// stamped into a template is one every session made from it would inherit.
    #[test]
    fn a_template_file_is_created_empty_at_the_path_asked_for() {
        use crate::sessions::plan::PlanStep;

        let compiled = compile_file_new(NAMED, "notes.md").expect("a legal name");
        assert_eq!(compiled.rel, "notes.md");
        assert_eq!(compiled.plan.verb, "template-file-new");
        assert_eq!(compiled.plan.session, NAMED);
        assert_eq!(
            compiled.plan.steps,
            vec![PlanStep::WriteFile {
                path: "_template/test1/notes.md".to_owned(),
                content: String::new(),
            }]
        );

        // The one exception, and `files::render_new`'s reason for it: an empty
        // file is not valid JSON, so the first tool to read it would fail on a
        // file keeper wrote.
        let json = compile_file_new(NAMED, "payload.json").expect("json is in the set");
        assert_eq!(
            json.plan.steps,
            vec![PlanStep::WriteFile {
                path: "_template/test1/payload.json".to_owned(),
                content: "{}\n".to_owned(),
            }]
        );
    }

    /// Row 4, restated by the fix for the directory this verb used to mint: the
    /// nested path lands where it was typed, in ONE step, and the folder in
    /// front of it is addressed rather than created. A `MkDir` here spelled a
    /// parent verbatim — `Interview Kit` — while *New folder* folds the same
    /// words to `interview-kit`, so the room had two spellings of one directory
    /// name and only one of them could be typed twice.
    #[test]
    fn a_nested_template_file_addresses_its_parent_and_never_mints_one() {
        use crate::sessions::plan::PlanStep;

        let compiled = compile_file_new(NAMED, "refs/inputs.md").expect("a legal path");
        assert_eq!(compiled.rel, "refs/inputs.md");
        assert_eq!(
            compiled.plan.steps,
            vec![PlanStep::WriteFile {
                path: "_template/test1/refs/inputs.md".to_owned(),
                content: String::new(),
            }],
            "one step: the parent addresses, and a create that minted it would \
             spell a folder name this room folds"
        );
        // Not one `MkDir` anywhere in the plan — the assertion above already says
        // so, and this says WHICH absence is load-bearing.
        assert!(
            !compiled
                .plan
                .steps
                .iter()
                .any(|step| matches!(step, PlanStep::MkDir { .. })),
            "the folder verb mints directories; this one addresses them"
        );
        // The typed name folds; the directory in front of it does not, because it
        // names a folder that is already on the drive — a hand-made
        // `Interview Kit/` is addressable, and the shell refuses one that is not
        // there rather than inventing it (`sessions_template_file_new`).
        let typed = compile_file_new(NAMED, "Interview Kit/Kick Off.md").expect("a legal path");
        assert_eq!(typed.rel, "Interview Kit/kick-off.md");
    }

    /// Row 5, and the shape of every containment refusal: nothing is compiled
    /// for a path that leaves the template, so nothing is opened for one either.
    #[test]
    fn a_path_that_leaves_the_template_is_refused_by_every_entry_verb() {
        for rel in [
            "../escape.md",
            "/etc/passwd",
            "refs/../../escape.md",
            "a\\b.md",
        ] {
            assert!(
                matches!(
                    compile_file_new(NAMED, rel),
                    Err(EntryError::Outside { .. })
                ),
                "{rel} must not compile"
            );
            assert!(matches!(
                compile_dir_new(NAMED, rel),
                Err(EntryError::Outside { .. })
            ));
            assert!(matches!(
                compile_entry_rename(NAMED, rel, "safe.md", EntryKind::File),
                Err(EntryError::Outside { .. })
            ));
            assert!(matches!(
                compile_entry_delete(NAMED, rel, EntryKind::File, "01TRASH"),
                Err(EntryError::Outside { .. })
            ));
        }
    }

    /// Row 6. One `MkDir`, which succeeds on a directory that is already there
    /// — so asking for `artifacts/` in a template that has it is not an error to
    /// report, and a nested path is still one step because `create_dir_all`
    /// makes the parents.
    #[test]
    fn a_template_folder_is_one_idempotent_mkdir() {
        use crate::sessions::plan::PlanStep;

        let compiled = compile_dir_new("_template", "artifacts").expect("a legal name");
        assert_eq!(compiled.rel, "artifacts");
        assert_eq!(compiled.plan.verb, "template-dir-new");
        assert_eq!(
            compiled.plan.steps,
            vec![PlanStep::MkDir {
                path: "_template/artifacts".to_owned(),
            }]
        );

        let nested = compile_dir_new(NAMED, "refs/inputs").expect("a legal path");
        assert_eq!(
            nested.plan.steps.len(),
            1,
            "create_dir_all makes the parents"
        );
        // A folder has no extension rule: an interior dot is part of the name.
        assert_eq!(
            compile_dir_new(NAMED, "v1.2").expect("a legal name").rel,
            "v1.2"
        );
    }

    /// Row 7, and the bug the naming helper exists to prevent: `slug` folds the
    /// dot with everything else, so `Kick Off.md` would become `kick-off-md` —
    /// a file with no extension that no editor reads as markdown.
    #[test]
    fn a_renamed_file_folds_its_stem_and_keeps_its_extension() {
        use crate::sessions::plan::PlanStep;

        let compiled = compile_entry_rename(NAMED, "about.md", "Record.md", EntryKind::File)
            .expect("a legal name");
        assert_eq!(compiled.rel, "record.md");
        assert_eq!(compiled.plan.verb, "template-entry-rename");
        assert_eq!(
            compiled.plan.steps,
            vec![PlanStep::MoveFile {
                from: "_template/test1/about.md".to_owned(),
                to: "_template/test1/record.md".to_owned(),
            }]
        );
        assert_eq!(
            compile_entry_rename(NAMED, "about.md", "Kick Off.md", EntryKind::File)
                .expect("a legal name")
                .rel,
            "kick-off.md",
            "the stem folds and the extension survives — never kick-off-md"
        );
        // A typed name with no extension keeps the one the file has: a rename
        // renames, it does not decide what kind of file this is.
        assert_eq!(
            compile_entry_rename(NAMED, "about.md", "Record", EntryKind::File)
                .expect("a legal name")
                .rel,
            "record.md"
        );
        // And the entry stays in its own folder — only the last segment moves.
        assert_eq!(
            compile_entry_rename(
                NAMED,
                "prompts/hand-off.md",
                "Handoff Notes",
                EntryKind::File
            )
            .expect("a legal name")
            .rel,
            "prompts/handoff-notes.md"
        );
        // Row 8's collision is not decided here: the domain opens nothing
        // (AD-108), so the shell asks the disk and `MoveFile` refuses it again as
        // it runs (`sessions_exec.rs`, matrix row 2).
    }

    /// Row 9. A folder rename is a `MoveDir`, so its contents travel by moving
    /// the directory rather than by being enumerated — nothing here can lose a
    /// file it did not know about.
    #[test]
    fn a_renamed_template_folder_moves_the_directory_whole() {
        use crate::sessions::plan::PlanStep;

        let compiled = compile_entry_rename(NAMED, "refs", "References", EntryKind::Dir)
            .expect("a legal name");
        assert_eq!(compiled.rel, "references");
        assert_eq!(
            compiled.plan.steps,
            vec![PlanStep::MoveDir {
                from: "_template/test1/refs".to_owned(),
                to: "_template/test1/references".to_owned(),
            }]
        );
    }

    /// Rows 10 and 11. Both deletes are trash moves keyed by id — recoverable,
    /// and a directory recoverable whole. `remove_dir_all` appears nowhere.
    #[test]
    fn a_deleted_template_entry_goes_to_the_trash_and_never_to_an_unlink() {
        use crate::sessions::plan::PlanStep;

        let file = compile_entry_delete("_template", "README.md", EntryKind::File, "01TRASH")
            .expect("a legal path");
        assert_eq!(file.rel, "README.md");
        assert_eq!(file.plan.verb, "template-entry-delete");
        assert_eq!(
            file.plan.steps,
            vec![PlanStep::TrashFile {
                path: "_template/README.md".to_owned(),
                trash_key: "01TRASH".to_owned(),
            }]
        );

        let dir =
            compile_entry_delete(NAMED, "refs", EntryKind::Dir, "01TRASH").expect("a legal path");
        assert_eq!(
            dir.plan.steps,
            vec![PlanStep::TrashDir {
                path: "_template/test1/refs".to_owned(),
                trash_key: "01TRASH".to_owned(),
            }]
        );
    }

    /// Row 12. An empty `rel` reaching a `TrashDir` would delete the template
    /// itself — the room's own verb, with its own confirmation — so the guard
    /// answers before anything is compiled, and says which verb does that.
    #[test]
    fn the_template_root_is_not_an_entry_a_verb_can_touch() {
        // Not `"/"`: that is an absolute path, and the containment refusal is the
        // more urgent fact about it.
        for rel in ["", "  ", ".", "./"] {
            assert_eq!(
                compile_entry_delete(NAMED, rel, EntryKind::Dir, "01TRASH"),
                Err(EntryError::Root),
                "{rel:?} is the template itself"
            );
        }
        assert_eq!(compile_file_new(NAMED, ""), Err(EntryError::Root));
        assert_eq!(compile_dir_new(NAMED, "."), Err(EntryError::Root));
        assert!(
            EntryError::Root.to_string().contains("New template"),
            "the refusal names the verb that does that instead"
        );
    }

    /// Row 15. `pattern_files` skips every dotfile except `.gitkeep`, so the
    /// room never lists `.DS_Store` and a create never copies it. A verb able to
    /// name one would act on a file the room says is not there — and the same
    /// walk feeds the create, so widening it would widen what every new session
    /// inherits.
    #[test]
    fn no_entry_verb_can_name_a_dotfile() {
        for rel in [
            ".DS_Store",
            "refs/.DS_Store",
            ".gitkeep",
            ".hidden/notes.md",
        ] {
            assert!(
                matches!(
                    compile_entry_delete(NAMED, rel, EntryKind::File, "01TRASH"),
                    Err(EntryError::Dotfile { .. })
                ),
                "{rel} must not be deletable from this room"
            );
            assert!(matches!(
                compile_entry_rename(NAMED, rel, "stray.md", EntryKind::File),
                Err(EntryError::Dotfile { .. })
            ));
            assert!(matches!(
                compile_file_new(NAMED, rel),
                Err(EntryError::Dotfile { .. })
            ));
            assert!(matches!(
                compile_dir_new(NAMED, rel),
                Err(EntryError::Dotfile { .. })
            ));
        }
        // And it cannot be created under a name that folds INTO one either: the
        // fold drops the leading dot rather than preserving it.
        assert_eq!(
            entry_name(".DS_Store", None).expect("folds to something"),
            "ds-store"
        );
    }

    /// The two name refusals, both worded for the field they are about: a fold
    /// that leaves nothing, and an extension keeper will not author.
    #[test]
    fn a_template_entry_needs_a_nameable_name_and_a_known_extension() {
        assert_eq!(
            compile_dir_new(NAMED, "###"),
            Err(EntryError::Unnameable {
                typed: "###".to_owned()
            })
        );
        assert_eq!(
            compile_entry_rename(NAMED, "about.md", "###", EntryKind::File),
            Err(EntryError::Unnameable {
                typed: "###".to_owned()
            })
        );
        // `nameable` and this must not drift: both ask the fold, not a
        // re-derived "contains a letter or digit" (see [`nameable`]'s own note).
        assert!(!nameable("###"));

        for rel in ["run.sh", "logo.png", "Makefile"] {
            assert!(
                matches!(
                    compile_file_new(NAMED, rel),
                    Err(EntryError::Extension { .. })
                ),
                "{rel} is not something keeper authors into a template"
            );
        }
        // A rename is not held to the closed set where the set has nothing to
        // protect: `logo.png` re-labels bytes that are already in the template
        // and already travel into every session made from it, so keeping a
        // `.png` is nobody's business.
        assert_eq!(
            compile_entry_rename(NAMED, "logo.png", "Logo Mark", EntryKind::File)
                .expect("a rename is not a create")
                .rel,
            "logo-mark.png"
        );
        // Renaming a `.png` to a `.md` is the same relaxation from the other
        // side: what the rename cannot do is carry a file OUT of the set, since
        // that authors, through a keeper verb, a file a create here refuses.
        assert_eq!(
            compile_entry_rename(NAMED, "logo.png", "notes.md", EntryKind::File)
                .expect("into the set is not out of it")
                .rel,
            "notes.md"
        );
        for typed in ["about.sh", "about", "About.PNG"] {
            let refused = compile_entry_rename(NAMED, "about.md", typed, EntryKind::File);
            if typed == "about" {
                // The one that is NOT a change: a typed name with no extension
                // keeps the one the file has, so this is still `about.md` and
                // the guard must not read it as leaving the set.
                assert_eq!(
                    refused.expect("no extension typed keeps .md").rel,
                    "about.md"
                );
                continue;
            }
            assert_eq!(
                refused,
                Err(EntryError::ExtensionChanged {
                    current: "about.md".to_owned(),
                    rel: typed.to_ascii_lowercase(),
                }),
                "{typed} would take a template file out of .md/.csv/.json"
            );
        }
    }
}
