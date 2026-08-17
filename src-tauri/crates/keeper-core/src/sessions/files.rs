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
use crate::sessions::model::{ARTIFACTS_DIR, README};
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

/// The three names a rename never touches, session-relative.
///
/// [`super::shape::shape`] reads `AGENTS.md` and `about.md` off the session's own
/// listing to decide which contract the folder follows, and `README.md` is the
/// folder-shaped record every reader addresses by name — [`super::model::README`]
/// itself, the `## Promote` table, the session's pins. Renaming one of these
/// would not break a link; it would break the *session*.
const RECORD_NAMES: [&str; 3] = [AGENTS, ABOUT, README];

/// How many bytes of a stamped filename are the stamp: `YYYY-MM-DD` is ten and
/// `HHMM` is four, with a separator on each side of the time.
///
/// The reverse of [`new_stamped`]'s `format!("{date}-{time}-{slug}")`, and it
/// lives here rather than beside [`super::pool`]'s reader because this module is
/// the *writer* — the arithmetic above is one line from the `format!` it
/// describes. `pool::stamp_of` reads the same fifteen characters for a different
/// answer (a clock, not an offset), so one shared helper would have to hand both
/// back to two callers that each want one.
const STAMP_LEN: usize = 16;

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

    #[error(
        "\"{typed}\" has nothing in it a folder can be named after — it needs letters or digits. \
         keeper folds a folder name to a slug and will not invent one for you."
    )]
    Unnameable { typed: String },

    #[error(
        "\"{typed}\" has nothing in it a filename can be named after — it needs letters or \
         digits. keeper will not invent a name, and it has not written the title either: \
         a file renamed halfway is worse than one not renamed at all."
    )]
    UnnameableTitle { typed: String },

    #[error(
        "renaming {rel} would overwrite {taken}, which is already in this session. keeper \
         never renames onto bytes somebody else wrote — retitle it to something that does \
         not collide, or move {taken} out of the way first."
    )]
    Collision { rel: String, taken: String },

    #[error(
        "{rel} is promoted output. A rename rewrites the session's own markdown and never \
         a deliverable in artifacts/: that name is the promotion contract the record's \
         table states, and a reference inside an artifact is a reference from the artifact \
         rather than from the session."
    )]
    Artifact { rel: String },
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

/// The folder path a *New folder* press lands on: the last segment folded, the
/// ones in front of it addressing what is already there — or the refusal for a
/// folder keeper will not make (FR-287).
///
/// **A session folder name folds, and that is a decision rather than an
/// inheritance.** `Interview Kit` becomes `interview-kit`, through the same
/// [`naming::slug_stem`] fold [`super::template::entry_name`] puts a template's
/// folder names through. Templates folded and sessions had no precedent, so the
/// rule is stated here: every name keeper writes into a session is already a slug
/// ([`new_named`], [`new_stamped`]), and a pool holding
/// `Interview Kit/2026-08-14-1030-opened.md` would be one directory spelled
/// unlike every other name in the session it sits in.
///
/// **The whole segment folds here, extension and all**, where
/// [`super::template::entry_name`] keeps a trailing `.md`. That is the one half
/// of the fold not shared, and it is deliberate on both sides: a template's
/// *New folder* shares its room with a *New file* whose field takes a filename,
/// so one fold has to serve both; a session's create dialog takes its extension
/// from a menu and never types one, so there is no extension here to preserve.
/// A directory called `notes.md` in a pool that now reads subdirectories for
/// markdown (FR-285) is a trap, not a folder.
///
/// **Only the last segment folds.** The ones in front of it address directories
/// already on the drive — `template::rejoin`'s rule one scope out, for its
/// reason: folding an addressed `Interview Kit/` somebody made in Finder would
/// mint a second directory beside it instead of writing into it.
///
/// Public because the shell asks it first: it needs the folded path to compose
/// the profile-relative subpath it puts to `WriteScope`, and a caller folding
/// again itself would be the second namer (AD-65). [`compile_dir_new`] asks it
/// again anyway — a guard the caller can skip is a guard.
///
/// # Errors
/// Whatever [`check_dir`] refuses, asked of the typed path **and** of the folded
/// one, so `Workspace` folds to `workspace` and is refused as scratch rather
/// than created. [`FileVerbError::Unnameable`] when the fold leaves nothing.
pub fn dir_rel(rel: &str) -> Result<String, FileVerbError> {
    // Trailing separators only: `log/` is the same request as `log`, while
    // trimming a LEADING one would turn `/etc` into a path this accepts.
    let rel = rel.trim().trim_end_matches('/');
    // Nothing typed is a naming failure and not a containment one — `check_dir`
    // would answer "not a path inside this session" about a field left blank.
    if rel.is_empty() {
        return Err(FileVerbError::Unnameable {
            typed: rel.to_owned(),
        });
    }
    check_dir(rel)?;
    let (parent, typed) = match rel.rsplit_once('/') {
        Some((parent, last)) => (Some(parent), last),
        None => (None, rel),
    };
    let name = naming::slug_stem(typed);
    if name.is_empty() {
        return Err(FileVerbError::Unnameable {
            typed: typed.to_owned(),
        });
    }
    let folded = match parent {
        Some(parent) => format!("{parent}/{name}"),
        None => name,
    };
    check_dir(&folded)?;
    Ok(folded)
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

/// [`check_rel`], plus the directory a rename neither moves nor rewrites.
///
/// `workspace/` is already refused by [`check_rel`] — scratch, AD-113 — and this
/// adds `artifacts/` for a different reason: a promotion is *"a copy under a
/// stable name, listed here"* ([`super::promote`]), so the name is the contract
/// the record's table records, and rewriting a link inside a deliverable would
/// be keeper editing output it did not write.
///
/// Asked of the file being renamed **and** of every file whose pointers are
/// rewritten, because it is one rule and both ends of a rename can break it. The
/// reader already declines to walk either directory
/// (`sessions_root::UNSCANNED_DIRS`), so this is the second of the two
/// predicates that must agree, not a new one: a rule that holds only because a
/// scan happened to skip a folder is a rule the day somebody calls this
/// directly.
///
/// # Errors
/// [`FileVerbError::Artifact`] for promoted output, or whatever [`check_rel`]
/// refuses.
pub fn check_rewritable(rel: &str) -> Result<(), FileVerbError> {
    check_rel(rel)?;
    if rel
        .strip_prefix(ARTIFACTS_DIR)
        .is_some_and(|rest| rest.starts_with('/'))
    {
        return Err(FileVerbError::Artifact {
            rel: rel.to_owned(),
        });
    }
    Ok(())
}

/// Whether this file's *name* follows its title.
///
/// False for the three names in [`RECORD_NAMES`], and that asymmetry is the
/// point: the record's title is editable and its filename is not. Refusing the
/// title edit as well would make the one file whose title is the session's own
/// headline the one file whose headline cannot be changed — so the rename verb
/// asks this, writes the title either way, and moves nothing when the answer is
/// no.
///
/// The whole session-relative path is compared, [`check_deletable`]'s way: a
/// `notes/README.md` somebody wrote is not the record, and it renames.
#[must_use]
pub fn renames(rel: &str) -> bool {
    !RECORD_NAMES.contains(&rel)
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

/// The `YYYY-MM-DD-HHMM-` a stamped stem leads with, when it has one. `None` for
/// an ordinary name, which carries no clock and needs none kept.
fn stamped_prefix(stem: &str) -> Option<&str> {
    let bytes = stem.as_bytes();
    let shaped = bytes.len() >= STAMP_LEN
        && bytes[..4].iter().all(u8::is_ascii_digit)
        && bytes[4] == b'-'
        && bytes[5..7].iter().all(u8::is_ascii_digit)
        && bytes[7] == b'-'
        && bytes[8..10].iter().all(u8::is_ascii_digit)
        && bytes[10] == b'-'
        && bytes[11..15].iter().all(u8::is_ascii_digit)
        && bytes[15] == b'-';
    // `get` rather than an index behind the flag: the slice is the answer, and a
    // range that cannot panic is worth more than one guarded by a `bool` two
    // lines up.
    if shaped {
        stem.get(..STAMP_LEN)
    } else {
        None
    }
}

/// Rejoin a folded name onto the folder it came out of.
fn rejoin(dir: Option<&str>, name: &str) -> String {
    match dir {
        Some(dir) => format!("{dir}/{name}"),
        None => name.to_owned(),
    }
}

/// The name `rel` takes when its title becomes `new_title` — or the refusal for
/// a rename keeper will not make (FR-295).
///
/// **The stamp survives, the slug is replaced.** A log's leading
/// `YYYY-MM-DD-HHMM-` is what makes the pool sort itself in Finder, in `ls` and
/// in any tool that has never heard of keeper ([`new_stamped`]), so a rename that
/// re-stamped with today's clock would file yesterday's entry as today's work.
/// The directory and the extension are untouched for the same reason they are
/// untouched by a title edit: neither is anything the title says.
///
/// **A collision refuses rather than counting.** [`new_named`] appends `-2`
/// because a *create* has no expectation about its name; a rename does — the
/// person typed a title and expects the file to be called after it, and handing
/// them `kick-off-2.md` would be keeper answering a different question. So the
/// refusal names the file that is in the way, which is the only fact that makes
/// it actionable.
///
/// `taken` is the destination folder's names, read fresh by the shell, compared
/// case-insensitively for [`new_named`]'s reason: APFS and NTFS fold case, so two
/// names differing only in case are one file on the machine in front of the
/// person. A candidate that folds onto `rel`'s *own* name is not a collision with
/// itself — that is a title edit that changes no name, or a case-only rename,
/// which [`PlanStep::MoveFile`]'s same-file carve-out is what lets through.
///
/// # Errors
/// [`FileVerbError::UnnameableTitle`] for a title that folds to nothing,
/// [`FileVerbError::Collision`] for a name already in the folder, or whatever
/// [`check_rewritable`] refuses about either path.
pub fn rename_target(
    rel: &str,
    new_title: &str,
    taken: &BTreeSet<String>,
) -> Result<String, FileVerbError> {
    check_rewritable(rel)?;
    // `slug_stem` rather than `slug`, and the difference IS the refusal: `slug`
    // answers `untitled` for a title with nothing in it, so a rename that folded
    // first and tested for empty afterwards would file somebody's file under
    // keeper's fallback word and call it the name they chose. Asked here, and the
    // name composed with `slug` below, so a fold landing on an MS-DOS device name
    // still gets the suffix `new_named` gives it.
    if naming::slug_stem(new_title).is_empty() {
        return Err(FileVerbError::UnnameableTitle {
            typed: new_title.to_owned(),
        });
    }
    let (dir, name) = match rel.rsplit_once('/') {
        Some((dir, name)) => (Some(dir), name),
        None => (None, rel),
    };
    // `check_rewritable` proved there is an extension and that it is one of the
    // three; this is that fact read back, not a second parse of it.
    let (stem, ext) = name
        .rsplit_once('.')
        .ok_or_else(|| FileVerbError::Extension {
            rel: rel.to_owned(),
        })?;
    let stamp = stamped_prefix(stem).unwrap_or_default();
    let candidate = format!("{stamp}{}.{ext}", naming::slug(new_title));

    if candidate.eq_ignore_ascii_case(name) {
        return Ok(rejoin(dir, &candidate));
    }
    if let Some(clash) = taken
        .iter()
        .find(|existing| existing.eq_ignore_ascii_case(&candidate))
    {
        return Err(FileVerbError::Collision {
            rel: rel.to_owned(),
            taken: rejoin(dir, clash),
        });
    }
    let to = rejoin(dir, &candidate);
    check_rewritable(&to)?;
    Ok(to)
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

/// The plan that makes one folder inside a session (FR-287): one `MkDir`.
///
/// `session` is the session's zone-relative folder (`active/2026-08-14-keeper`)
/// and `rel` is session-relative; the join happens here so no caller composes a
/// zone path (AD-65), and the name is folded by [`dir_rel`] so no caller spells
/// one either.
///
/// **Idempotent by contract** ([`PlanStep::MkDir`]): asking for a folder that is
/// already there succeeds and changes nothing. That is the right answer rather
/// than a refusal — `artifacts/` is exactly the name somebody types without
/// looking first, and "it is already there" is not a failure to report.
///
/// **One step for a nested path**, because `MkDir` makes parents: `a/b/c` is one
/// plan and one journal row rather than three.
///
/// **No `.gitkeep`.** [`super::pattern::is_placeholder`] exists for FILE-list
/// copies, where an empty directory would not survive one; a `MkDir` step holds
/// its own directory open.
///
/// **Never [`super::template::compile_dir_new`] aimed at a session.** That
/// module's guards deliberately carry no `workspace/` refusal — its own section
/// header records the inverse rule, because a template's `workspace/` is a
/// skeleton directory a create copies — so pointed at a live session it would
/// compile `MkDir active/s/workspace/whatever` straight through the fence AD-113
/// puts around scratch.
///
/// The shell asks `WriteScope::in_session_workspace` about the folded path as
/// well as this: [`compile_new`]'s note, for its reason.
///
/// # Errors
/// Whatever [`dir_rel`] refuses — the plan is not compiled for a folder keeper
/// will not make.
pub fn compile_dir_new(session: &str, rel: &str) -> Result<Plan, FileVerbError> {
    let rel = dir_rel(rel)?;
    Ok(Plan {
        verb: "dir-new".to_owned(),
        session: session.to_owned(),
        steps: vec![PlanStep::MkDir {
            path: format!("{session}/{rel}"),
        }],
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

/// One file a rename rewrites: where it is, how long it was when it was read,
/// and the bytes it gets.
///
/// The length is the guard [`PlanStep::GuardedWrite`] takes, and it is the
/// *read's* own length rather than anything computed here: the shell read these
/// bytes to decide what to write, so what the guard is aimed at is a write that
/// landed between that read and this plan running.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Rewrite {
    /// Session-relative path, as it is **before** the move.
    pub rel: String,
    /// Length of the bytes this rewrite was computed from.
    pub expect_len: usize,
    /// The bytes to write.
    pub content: String,
}

/// The plan that renames one session file and fixes what pointed at it
/// (FR-295, FR-296).
///
/// `rel` is where the file is now, `to` is where it is going — both
/// session-relative, both decided by [`rename_target`] — and `rewrites` is every
/// file whose bytes change, the renamed file's own title write included, each
/// addressed at the path it has *before* the move. One plan, one journal row:
/// either the file moved and the pointers followed, or neither happened
/// (NFR-38).
///
/// **The title write is in here rather than in a `sync_write_frontmatter` call
/// beside it.** A rename is one act from the person's point of view — they
/// changed the title — and two commands would mean two journal rows and a window
/// in which the file says `Kick Off` and is still called `untitled`. That window
/// is precisely the *"half of it would be worse than none"* `docs/sessions.md`
/// refused a rename over, so the answer is one plan and not two verbs called in
/// order.
///
/// **`to == rel` is the record's case and compiles to no move at all**, which is
/// what makes [`renames`] a fact the caller reads rather than a branch it writes:
/// the plan is then the title write alone, journaled the same way.
///
/// **[`PlanStep::MoveFile`] is last.** That is AD-111's rule, and here it is also
/// the only order that resumes: a re-run guarded write meets its own output and
/// returns `Ok` (`sessions_exec`'s idempotency-before-guard branch), so the move
/// is the one step a resume has left to do. Moving first would leave a resumable
/// prefix in which every remaining rewrite is addressed at a path that has gone.
///
/// One window stays open and is worth naming rather than hiding: a crash between
/// the rename landing and the journal recording it makes the resumed move fail on
/// a source that is no longer there. The tree is *consistent* in that window —
/// fully renamed, pointers rewritten — and the resume reports the disk's error
/// over it. Teaching `MoveFile` to read "source gone, target present" as "already
/// done" would close it, and `sessions_exec` argues at length against exactly
/// that: the same test is satisfied by a neighbour a rename must never be told it
/// ate.
///
/// # Errors
/// Whatever [`check_rewritable`] refuses about either path or about any
/// rewrite's — the plan is not compiled for a file keeper will not move and not
/// for a pointer it will not touch.
pub fn compile_rename(
    session: &str,
    rel: &str,
    to: &str,
    rewrites: &[Rewrite],
) -> Result<Plan, FileVerbError> {
    check_rewritable(rel)?;
    check_rewritable(to)?;
    let mut steps = Vec::with_capacity(rewrites.len() + 1);
    for rewrite in rewrites {
        check_rewritable(&rewrite.rel)?;
        steps.push(PlanStep::GuardedWrite {
            path: format!("{session}/{}", rewrite.rel),
            expect_len: rewrite.expect_len,
            content: rewrite.content.clone(),
        });
    }
    if to != rel {
        steps.push(PlanStep::MoveFile {
            from: format!("{session}/{rel}"),
            to: format!("{session}/{to}"),
        });
    }
    Ok(Plan {
        verb: "file-rename".to_owned(),
        session: session.to_owned(),
        steps,
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

    // -----------------------------------------------------------------------
    // A folder somebody makes (FR-287) — the spec's matrix, rows 1-6. Row 7 is
    // the shell's (an unknown root or session), and rows 8-12 belong to the
    // template and the pattern.
    // -----------------------------------------------------------------------

    /// Rows 1, 2 and 6. One `MkDir`, a verb the journal can name, and parents in
    /// the same plan — `MkDir` makes them and succeeds on what is already there,
    /// so a second press changes nothing and needs no second answer.
    #[test]
    fn a_new_folder_is_one_mkdir_the_journal_can_replay() {
        let plan = compile_dir_new("active/s", "log").expect("a session may hold a log/");
        assert_eq!(plan.verb, "dir-new");
        assert_eq!(plan.session, "active/s");
        assert_eq!(
            plan.steps,
            vec![PlanStep::MkDir {
                path: "active/s/log".to_owned()
            }],
            "a folder verb that wrote a file would be writing something nobody asked for"
        );
        // Row 2: the same request twice is the same plan, and `MkDir` absorbs
        // the second run rather than failing it (`plan.rs`'s first invariant).
        assert_eq!(
            compile_dir_new("active/s", "log").expect("idempotent by contract"),
            plan
        );
        // Row 6: nested parents arrive in ONE step, because `MkDir` creates
        // them. Three steps would be three journal rows for one press.
        let deep = compile_dir_new("active/s", "a/b/c").expect("parents are made by MkDir");
        assert_eq!(
            deep.steps,
            vec![PlanStep::MkDir {
                path: "active/s/a/b/c".to_owned()
            }]
        );
        // And no placeholder: `is_placeholder` exists for file-list copies, and
        // a `.gitkeep` here would be a file keeper invented.
        assert!(!deep
            .steps
            .iter()
            .any(|step| matches!(step, PlanStep::WriteFile { .. })));
    }

    /// Row 3. The fold, asserted — templates fold and a session now folds the
    /// same way, so one zone does not spell one directory two ways.
    ///
    /// The half that is NOT shared is asserted too: a template entry keeps a
    /// trailing extension and a session folder does not, because a session's
    /// create dialog takes its extension from a menu and a directory called
    /// `notes.md` in a pool that reads subdirectories is a trap.
    #[test]
    fn a_session_folder_name_folds_the_way_a_templates_does() {
        assert_eq!(dir_rel("Interview Kit").as_deref(), Ok("interview-kit"));
        assert_eq!(dir_rel("  Log  ").as_deref(), Ok("log"));
        assert_eq!(
            dir_rel("log/").as_deref(),
            Ok("log"),
            "a trailing / is noise"
        );
        assert_eq!(dir_rel("Café Notes").as_deref(), Ok("cafe-notes"));
        // The last segment folds; the ones in front address what is on the
        // drive, so a folder somebody made in Finder is written INTO rather
        // than duplicated beside.
        assert_eq!(
            dir_rel("Interview Kit/Kick Off").as_deref(),
            Ok("Interview Kit/kick-off")
        );
        // The divergence from the template fold, asserted from both sides so
        // that nobody "fixes" one of the two into the other by accident: a
        // template's folder keeps a dotted tail (`v1.2` is a folder called
        // `v1.2`), and a session's folds it away, because a session directory
        // that reads as a filename is a trap in a pool that walks
        // subdirectories for markdown.
        assert_eq!(dir_rel("v1.2").as_deref(), Ok("v1-2"));
        assert_eq!(
            crate::sessions::template::entry_name("v1.2", None).as_deref(),
            Some("v1.2")
        );
        assert_eq!(dir_rel("Kick Off.md").as_deref(), Ok("kick-off-md"));
        // And the plan lands on the folded name, not on what was typed.
        let plan = compile_dir_new("active/s", "Interview Kit").expect("nameable");
        assert_eq!(
            plan.steps,
            vec![PlanStep::MkDir {
                path: "active/s/interview-kit".to_owned()
            }]
        );
    }

    /// Row 4. Scratch is fenced (AD-113), and a folder there would invite writes
    /// the engine refuses — including a `Workspace` that only becomes the fenced
    /// name after the fold, which is why the folded path is checked as well.
    #[test]
    fn no_folder_is_made_inside_the_workspace() {
        assert!(matches!(
            dir_rel("workspace"),
            Err(FileVerbError::Workspace { .. })
        ));
        assert!(matches!(
            dir_rel("workspace/x"),
            Err(FileVerbError::Workspace { .. })
        ));
        assert!(
            matches!(dir_rel("Workspace"), Err(FileVerbError::Workspace { .. })),
            "the fold is what makes this the fenced directory, so the fold is checked"
        );
        assert!(compile_dir_new("active/s", "workspace/iter-3").is_err());
        // A folder merely *named* like the workspace is not in it, and
        // `artifacts/` is the place the refusal itself points at.
        assert_eq!(dir_rel("workspace-notes").as_deref(), Ok("workspace-notes"));
        assert_eq!(dir_rel("artifacts").as_deref(), Ok("artifacts"));
    }

    /// Row 5. Refused before anything is opened — the domain performs no IO
    /// (AD-108), so these never reach a `create_dir_all`.
    #[test]
    fn a_folder_path_cannot_walk_out_of_the_session() {
        for rel in [
            "../escape",
            "/abs",
            ".hidden",
            "log/../../etc",
            "a/.git",
            "side\\ways",
        ] {
            assert!(
                matches!(dir_rel(rel), Err(FileVerbError::Outside { .. })),
                "{rel} must not be a folder keeper makes"
            );
            assert!(compile_dir_new("active/s", rel).is_err());
        }
    }

    /// The trap `template::nameable` exists for, asked one module over: `slug`
    /// answers `untitled` for a name with nothing in it, so a verb that folded
    /// first and tested for empty afterwards would mint `untitled/` and call it
    /// the operator's name.
    #[test]
    fn a_name_with_nothing_in_it_is_no_folder_name() {
        for typed in ["###", "🎉", "   "] {
            assert!(
                matches!(dir_rel(typed), Err(FileVerbError::Unnameable { .. })),
                "{typed} folds to nothing and must be refused, not renamed"
            );
        }
        // …and this is why the test cannot be "did the fold come back empty".
        assert_eq!(crate::notes::naming::slug("###"), "untitled");
        // A name somebody really typed still passes, fallback word included.
        assert_eq!(dir_rel("untitled").as_deref(), Ok("untitled"));
    }

    // -----------------------------------------------------------------------
    // Renaming one file, and the pointers that named it (Story 51.6)
    // -----------------------------------------------------------------------

    /// Row 1. The stamp is the pool's sort order, so a retitle keeps it and
    /// replaces only the part of the name the title decides.
    #[test]
    fn a_stamped_name_keeps_its_stamp_and_changes_its_slug() {
        assert_eq!(
            rename_target(
                "2026-08-16-1812-untitled.md",
                "Kick Off",
                &taken(&["2026-08-16-1812-untitled.md"])
            )
            .as_deref(),
            Ok("2026-08-16-1812-kick-off.md")
        );
        // In a subfolder: the folder is not something the title says either.
        assert_eq!(
            rename_target("refs/2026-08-16-1812-untitled.md", "Kick Off", &taken(&[])).as_deref(),
            Ok("refs/2026-08-16-1812-kick-off.md")
        );
        // A name that carries no clock takes the whole slug, and keeps the
        // extension the create chose from a menu.
        assert_eq!(
            rename_target("budget.csv", "Q3 Budget", &taken(&[])).as_deref(),
            Ok("q3-budget.csv")
        );
    }

    /// Row 4. A title with nothing in it is refused rather than folded, and the
    /// refusal says the title was not written either — the sentence is the
    /// contract, because `slug` would happily have answered `untitled`.
    #[test]
    fn a_title_that_folds_to_nothing_refuses_rather_than_inventing_a_name() {
        for typed in ["###", "🎉", "   "] {
            assert!(
                matches!(
                    rename_target("2026-08-16-1812-untitled.md", typed, &taken(&[])),
                    Err(FileVerbError::UnnameableTitle { .. })
                ),
                "{typed} folds to nothing and must be refused, not filed under a fallback"
            );
        }
        // …and this is why the test cannot be "did the fold come back empty".
        assert_eq!(crate::notes::naming::slug("###"), "untitled");
        assert!(
            rename_target("2026-08-16-1812-untitled.md", "###", &taken(&[]))
                .expect_err("a title that folds to nothing must refuse")
                .to_string()
                .contains("has not written the title either"),
            "the refusal has to say the title did not land, or a reader will assume it did"
        );
    }

    /// Row 5. Refused, and the refusal names the file that would have been
    /// overwritten — a create counts up, a rename does not.
    #[test]
    fn a_rename_onto_a_neighbour_refuses_and_names_it() {
        let error = rename_target(
            "refs/2026-08-16-1812-untitled.md",
            "Kick Off",
            &taken(&["2026-08-16-1812-kick-off.md"]),
        )
        .expect_err("a name already in the folder must refuse");
        assert_eq!(
            error,
            FileVerbError::Collision {
                rel: "refs/2026-08-16-1812-untitled.md".to_owned(),
                taken: "refs/2026-08-16-1812-kick-off.md".to_owned(),
            }
        );
        // Case-folded, because APFS and NTFS are: the collision is real on the
        // machine in front of the person.
        assert!(matches!(
            rename_target("a.md", "Kick Off", &taken(&["KICK-OFF.MD"])),
            Err(FileVerbError::Collision { .. })
        ));
    }

    /// A title edit that lands on the name the file already has is not a
    /// collision with itself, and neither is a case-only one.
    #[test]
    fn a_retitle_onto_its_own_name_is_not_a_collision() {
        assert_eq!(
            rename_target("kick-off.md", "Kick Off!", &taken(&["kick-off.md"])).as_deref(),
            Ok("kick-off.md")
        );
        assert_eq!(
            rename_target("Kick-Off.md", "kick off", &taken(&["Kick-Off.md"])).as_deref(),
            Ok("kick-off.md"),
            "a case-only rename is `MoveFile`'s same-file carve-out, not a refusal"
        );
    }

    /// Row 6. The record's title is editable and its filename is not — `shape()`
    /// reads two of these names, and the third is what every reader of a
    /// folder-shaped session addresses.
    #[test]
    fn the_record_and_the_contract_file_keep_their_names() {
        for rel in ["about.md", "AGENTS.md", "README.md"] {
            assert!(!renames(rel), "{rel} must keep the name shape() reads");
        }
        // A file that merely shares the word is an ordinary pool file.
        assert!(renames("notes/README.md"));
        assert!(renames("about-the-client.md"));
        // And the plan for one is the title write alone: no move, still journaled.
        let plan = compile_rename(
            "active/s",
            "about.md",
            "about.md",
            &[Rewrite {
                rel: "about.md".to_owned(),
                expect_len: 12,
                content: "---\ntitle: Kick Off\n---\n".to_owned(),
            }],
        )
        .expect("the record's title write compiles");
        assert_eq!(plan.verb, "file-rename");
        assert_eq!(plan.steps.len(), 1);
        assert!(matches!(plan.steps[0], PlanStep::GuardedWrite { .. }));
    }

    /// Row 7. The fence's own sentence, asked one scope in from where it is
    /// enforced — both of a rename's ends and every pointer it would rewrite.
    #[test]
    fn the_workspace_and_artifacts_are_neither_renamed_nor_rewritten() {
        assert!(matches!(
            rename_target("workspace/iter-3.md", "Kick Off", &taken(&[])),
            Err(FileVerbError::Workspace { .. })
        ));
        assert!(matches!(
            rename_target("artifacts/report.md", "Kick Off", &taken(&[])),
            Err(FileVerbError::Artifact { .. })
        ));
        assert!(matches!(
            check_rewritable("artifacts/2026/report.md"),
            Err(FileVerbError::Artifact { .. })
        ));
        // A file merely *named* like either directory is in neither.
        assert!(check_rewritable("artifacts-index.md").is_ok());
        // And a pointer rewrite aimed at output is refused by the compiler, not
        // only by the scan that declines to read it.
        assert!(matches!(
            compile_rename(
                "active/s",
                "a.md",
                "b.md",
                &[Rewrite {
                    rel: "artifacts/report.md".to_owned(),
                    expect_len: 1,
                    content: String::new(),
                }],
            ),
            Err(FileVerbError::Artifact { .. })
        ));
    }

    /// Row 8. Every guarded write first, the move last: a resume re-runs the
    /// writes against their own output (a no-op) and has exactly the move left.
    /// The inverse order would leave a prefix whose remaining steps address a
    /// path that has already gone.
    #[test]
    fn the_move_sorts_after_every_rewrite_so_a_resume_has_one_step_left() {
        let plan = compile_rename(
            "active/2026-08-16-keeper",
            "2026-08-16-1812-untitled.md",
            "2026-08-16-1812-kick-off.md",
            &[
                Rewrite {
                    rel: "2026-08-16-1812-untitled.md".to_owned(),
                    expect_len: 30,
                    content: "titled".to_owned(),
                },
                Rewrite {
                    rel: "README.md".to_owned(),
                    expect_len: 40,
                    content: "pointed".to_owned(),
                },
            ],
        )
        .expect("a rename with one pointer compiles");

        assert_eq!(plan.verb, "file-rename");
        assert_eq!(plan.session, "active/2026-08-16-keeper");
        assert_eq!(
            plan.steps,
            vec![
                PlanStep::GuardedWrite {
                    path: "active/2026-08-16-keeper/2026-08-16-1812-untitled.md".to_owned(),
                    expect_len: 30,
                    content: "titled".to_owned(),
                },
                PlanStep::GuardedWrite {
                    path: "active/2026-08-16-keeper/README.md".to_owned(),
                    expect_len: 40,
                    content: "pointed".to_owned(),
                },
                PlanStep::MoveFile {
                    from: "active/2026-08-16-keeper/2026-08-16-1812-untitled.md".to_owned(),
                    to: "active/2026-08-16-keeper/2026-08-16-1812-kick-off.md".to_owned(),
                },
            ],
            "the renamed file is written at the path it still has, and the move is last"
        );
    }
}
