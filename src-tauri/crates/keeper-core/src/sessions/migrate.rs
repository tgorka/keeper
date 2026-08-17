//! Folder-shaped session → flat markdown pool, and the record's own rename,
//! each compiled to a plan (FR-257, FR-300, FR-301).
//!
//! Migration is a *verb someone chooses*, never something a scan does on the
//! operator's behalf: the zones are real folders on real drives, and a reader
//! that rewrote what it read would turn opening the board into a commit. So
//! this module compiles the same kind of journaled plan every other lifecycle
//! verb compiles, and the shell runs it with the same executor and the same
//! crash-resume story (AD-111).
//!
//! ## Two verbs, and what each owns
//!
//! [`compile_migrate`] converts the *shape*: a folder-shaped session becomes a
//! flat pool. [`compile_record_rename`] converts one *name*: a session whose
//! record is still `about.md` gets it moved to `README.md` (story 52.1). They do
//! not overlap — `compile_migrate` declines a session with an `about.md` at its
//! root and points at the other verb, because composing a fresh empty record
//! beside a real one is not a migration, it is a loss.
//!
//! What [`compile_migrate`] converts:
//!
//! - `README.md` → `README.md`, minus its `## Log` section and plus the `about`
//!   kind tag, keeping every other byte of the record — including the
//!   `## Promote` table, which is the session's contract with the archive
//!   checklist and travels verbatim. Guarded on the bytes it was planned
//!   against, so a concurrent agent write refuses the migration rather than
//!   losing an edit.
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
//! It does not delete the README, and since story 52.1 it does not need a
//! signpost either: the record IS the README, so every link, bookmark and agent
//! instruction in the operator's world that pointed at that filename still
//! resolves to the record it always named. It does not touch `artifacts/` or
//! `workspace/`: those are the two subtrees that are not markdown, and the flat
//! contract keeps both (AD-119).

use crate::notes::frontmatter::{FieldValue, Frontmatter};
use crate::notes::naming::slug;
use crate::sessions::model::{log_entries, README};
use crate::sessions::plan::{Plan, PlanStep};
use crate::sessions::refs;
use crate::sessions::shape::{kind_dir, shape, KindTag, Shape, ABOUT, AGENTS, KINDS};

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

/// Compile the migration, or `None` when there is no shape conversion to do.
///
/// Idempotence is stated in the return type rather than left to the executor:
/// re-running the verb on a migrated session is not a no-op plan, it is *no
/// plan*, so the UI can grey the button out from the same fact the compiler uses.
///
/// **Two reasons for `None`, and the second one is story 52.1's.** A session that
/// is already [`Shape::Flat`] has nothing to convert. A session holding an
/// `about.md` at its root belongs to [`compile_record_rename`] instead: since the
/// shape predicate narrowed to `AGENTS.md`, such a session reads as
/// [`Shape::Folder`] even though its record is right there, and running the shape
/// conversion over it would compose a fresh, empty `README.md` beside the real
/// record and leave the real one behind as an untagged pool file. Declining is
/// the honest answer: the record rename is the verb that applies, and it is the
/// one the surface should offer.
///
/// `TrashDir` is emitted **only for directories present in
/// [`MigrateInput::top_level`]**. The executor's `TrashDir` is idempotent on
/// replay (source gone, trash present → `Ok`) but errors when the source never
/// existed at all, so the guard has to live at compile time. A session with no
/// `refs/` is not a broken migration; it is a session nobody put a reference in.
pub fn compile_migrate(input: &MigrateInput) -> Option<Plan> {
    if shape(&input.top_level) == Shape::Flat || input.top_level.iter().any(|entry| entry == ABOUT)
    {
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
    // the three structural names and quietly replace it. `about.md` stays
    // reserved after story 52.1 even though nothing is written there: a
    // `refs/about.md` hoisted onto that name would land on a file the record
    // rename is going to move.
    let mut taken: Vec<String> = vec![
        ABOUT.to_owned(),
        AGENTS.to_owned(),
        README.to_owned(),
        "artifacts".to_owned(),
        "workspace".to_owned(),
    ];
    let mut steps = Vec::new();

    // 1. The record, rewritten **where it already is** (story 52.1). The
    //    README's own frontmatter travels whole — `id`, `created`, `pinned` and
    //    the `keeper:` lineage map are the session's identity, and a migration
    //    that dropped them would silently unpin the session and orphan both ends
    //    of every continuation (AD-112). What changes is the `## Log` section,
    //    which becomes one file per entry below, and the `about` kind tag, which
    //    is what makes the About space find this file rather than its name.
    //
    //    Guarded on the bytes it was planned against, because unlike the
    //    `about.md` this used to write, the target is a file that already exists
    //    and holds the operator's prose: a plain write would lose an agent's edit
    //    made between the read and the run. A session with no README at all has
    //    nothing to guard and nothing to lose, and `GuardedWrite` cannot express
    //    that — the executor reads the target first — so that one case is an
    //    ordinary write.
    let title = crate::notes::naming::title_from_body(body);
    let record_id = match fm.as_string("id") {
        Some(existing) if !existing.trim().is_empty() => existing.to_owned(),
        _ => next_id().to_owned(),
    };
    let record_body = without_log_section(body);
    let mut record = format!("{header}{record_body}");
    if !record_id.is_empty() {
        record = Frontmatter::set_in(&record, "id", FieldValue::Str(record_id));
    }
    if fm.as_string("created").is_none() {
        record = Frontmatter::set_in(&record, "created", FieldValue::Str(input.today.clone()));
    }
    record = with_tag(&record, KindTag::About);
    steps.push(if input.readme.is_empty() {
        PlanStep::WriteFile {
            path: at(README),
            content: record,
        }
    } else {
        PlanStep::GuardedWrite {
            path: at(README),
            expect_len: input.readme.len(),
            content: record,
        }
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

    // 5. Irreversible, and therefore last.
    //
    //    The directories are asked of [`kind_dir`] rather than listed here, for
    //    [`carried_kind`]'s reason and to keep its claim true: a fourth
    //    directory added to the folder contract is read back into the pool for
    //    free, and this is the step that would otherwise leave it on disk —
    //    a half-migrated session whose stale kind directory `shape()` cannot
    //    see, because it keys on `AGENTS.md` and not on what is left behind.
    //    `""` for the destination: this asks the CONTRACT what it keeps, and a
    //    space's own directory (Story 52.5) is where a create goes rather than
    //    a directory the migration owns and empties.
    for dir in KINDS
        .into_iter()
        .filter_map(|kind| kind_dir(Shape::Folder, kind, "").ok().flatten())
    {
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

// ---------------------------------------------------------------------------
// The record's own rename (story 52.1, FR-300, FR-301)
// ---------------------------------------------------------------------------

/// One markdown file the pointer pass may rewrite, as the shell read it.
///
/// Split into the session and the path inside it, rather than carried as one
/// zone-relative string, because the two are needed for different things: the
/// plan step names the joined path, and [`refs::rewrite_pointers`] is handed the
/// session-relative one because a pointer resolves *beside the file that holds
/// it* — the same frame `sessions_file_rename` hands it per file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PointerFile {
    /// The session this file belongs to, zone-relative:
    /// `active/2026-08-10-keeper`.
    pub session: String,
    /// Its path inside that session, `/`-joined: `spaces/plan.md`.
    pub rel: String,
    pub text: String,
}

impl PointerFile {
    /// Where this file is, zone-relative — the path a plan step names.
    fn path(&self) -> String {
        format!("{}/{}", self.session, self.rel)
    }
}

/// Everything [`compile_record_rename`] needs, all of it read by the shell.
///
/// Pure in, pure out, [`MigrateInput`]'s rule: no clock, no id generator, no
/// filesystem. Unlike that one it needs no ULIDs, because nothing here composes
/// a file that could want an identity — the record keeps its own, which is the
/// whole point of moving it rather than rewriting it.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct RecordRenameInput {
    /// The session whose record moves, zone-relative.
    pub session: String,
    /// That session directory's own entry names — what decides which case this
    /// is. Names, not paths, not recursive.
    pub top_level: Vec<String>,
    /// `README.md`'s current bytes at that root. Empty when there is none, which
    /// is the ordinary case.
    pub readme: String,
    /// The session's title, for the `AGENTS.md` a hand-built flat session is
    /// owed. Empty is survivable — [`agents_md`] carries its own fallback
    /// heading.
    pub title: String,
    /// Every markdown file in every session of the ZONE, this session's
    /// included: the pointer pass's scope.
    ///
    /// Zone-wide because a link at a session's record can be written in another
    /// session — a continuation names what it continues — and a rename that only
    /// swept its own folder would leave exactly those pointing at a filename
    /// nothing answers to.
    pub pointers: Vec<PointerFile>,
    /// The zone's own path from the drive root — `60-sessions`, and empty when
    /// the zone IS that root.
    ///
    /// The pointer pass needs it because a link written in ANOTHER session can
    /// only reach this record by spelling it in full, from the drive root: that
    /// is [`refs::resolve`]'s third probe — "the target as written", the spelling
    /// the drives' own `AGENTS.md` asks for — and therefore the only cross-session
    /// spelling that resolves at all. Without it the zone-wide pass would rewrite
    /// exactly what a one-session pass rewrites, and the scope's own
    /// justification would be unmet.
    pub prefix: String,
    /// Every session in the zone that still holds an `about.md` at its root,
    /// this one included.
    ///
    /// The pointer pass's one exclusion, and it is a *resolution* fact rather
    /// than a preference: a `[[about]]` written inside a session that has its own
    /// `about.md` resolves to THAT session's record ([`refs::resolve`] probes
    /// beside the file and then beside the session), so rewriting it while that
    /// session is unmigrated would break a link that works. Those files are left
    /// for their own session's rename, which is what makes running the verb over
    /// the whole zone and running it one session at a time end in the same place.
    pub with_about: Vec<String>,
}

/// Why a record rename refused. One variant, because there is one thing that
/// can be in the way.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum RecordRenameError {
    /// A `README.md` that is not an older migration's signpost sits where the
    /// record is going, **in a session that has an `AGENTS.md`**.
    ///
    /// That last clause is the whole of what makes refusing right here and wrong
    /// one case over. With `AGENTS.md` present the session is flat under both
    /// contracts, so `sessions_root::row_for` is reading this `README.md` as the
    /// record right now: two files hold record content and neither is
    /// authoritative. Without it the session read as FLAT before story 52.1
    /// narrowed the shape predicate, so its `about.md` was its record and this
    /// `README.md` was an ordinary pool file — nothing to choose between, and
    /// [`compile_record_rename`] trashes it rather than refusing.
    ///
    /// Both paths in the sentence, because the person reading it has to open one
    /// and decide about the other, and "a file already exists" without a name is
    /// a sentence that sends them looking. What the sentence does NOT say any
    /// more is "move {to} aside": [`crate::sessions::files::check_deletable`]
    /// refuses a delete of `README.md` and `RECORD_NAMES` refuses renaming it, so
    /// the old remedy named a move keeper itself forbids.
    #[error(
        "{from} cannot move to {to}: this session has an AGENTS.md, so keeper already reads {to} \
         as its record — and {from} is where its id, its pinned flag and its keeper: lineage \
         still are. Two files hold record content and keeper will not choose between them: open \
         both, copy what you want to keep into {to}, and take {from} out of the session in Finder \
         once it holds nothing you need. keeper will not delete it for you — it cannot tell your \
         session's only record from a leftover."
    )]
    Collision { from: String, to: String },
}

/// Compile the record's rename: `about.md` → `README.md`, as one journaled plan.
///
/// **An empty plan is the no-op**, not `None` as [`compile_migrate`] uses. The
/// difference is what the caller does with it: migration's `None` greys a button
/// out, while this verb is run across a zone and "this session needed nothing"
/// has to compose with "that one needed three steps". An empty `steps` runs to
/// completion doing nothing, which is what a replayable executor wants anyway.
///
/// ## The six cases, from the files
///
/// - **Already migrated, or folder-shaped** — no `about.md` at the root, so
///   nothing to move and no pointer to rewrite: an empty plan.
/// - **keeper-created flat** (`AGENTS.md` + `about.md`) — one
///   [`PlanStep::MoveFile`].
/// - **Half-migrated** (`AGENTS.md` + `about.md` + the signpost an older
///   `compile_migrate` left where the README was) — the signpost is *trashed*
///   into `.keeper/trash/`, never unlinked, and only then is the move legal.
/// - **Hand-built flat** (`about.md`, no `AGENTS.md`) — `AGENTS.md` is WRITTEN
///   first. Without that step the session reads as [`Shape::Folder`] the instant
///   the record lands at `README.md`, which is a folder-shaped session with a
///   flat pool: every log invisible behind a `## Log` heading that is not there.
/// - **Hand-built flat with somebody's `README.md` in it** (`about.md` +
///   `README.md`, no `AGENTS.md`) — the README is trashed under its own key, the
///   signpost's own recoverable move. Two facts make that the honest answer
///   rather than a clobber: with no `AGENTS.md` this session read as
///   [`Shape::Flat`] before story 52.1 narrowed the shape predicate, so
///   `about.md` was its record and that `README.md` was a pool file no reader
///   ever took for one; and every other exit is now closed — both verbs used to
///   decline this shape ([`compile_migrate`] returns `None` for an `about.md` at
///   the root, this one refused), while
///   [`crate::sessions::files::check_deletable`] and `files::RECORD_NAMES` refuse
///   to delete or rename a `README.md`. The only way out was Finder, for a shape
///   keeper's own predicate change created.
/// - **A `README.md` in the way of a session that HAS its `AGENTS.md`** —
///   refused, naming both paths. There the README is what the board is already
///   rendering as the record, so the two files genuinely compete and keeper
///   chooses neither. Never clobbered.
///
/// ## Why the move is last
///
/// Every other step here is idempotent on replay: `WriteFile` overwrites,
/// `TrashFile` answers `Ok` when the source is gone and the trash holds it, and
/// `GuardedWrite` recognises its own output. `MoveFile` is the one that is not —
/// `sessions_exec` deliberately refuses a `MoveFile` whose target exists, because
/// its only previous caller ran plans it had just stat'd and a gone source there
/// means a stale list rather than a completed move. So the move sorts last, the
/// place this codebase already puts a step whose replay is not free
/// ([`crate::sessions::plan`]'s second invariant): a crash after it leaves the
/// verb *complete*, and the resume that re-runs it refuses loudly over work that
/// is already done rather than over work that still needs doing.
///
/// # Errors
/// [`RecordRenameError::Collision`] when a foreign `README.md` is in the way.
pub fn compile_record_rename(input: &RecordRenameInput) -> Result<Plan, RecordRenameError> {
    let session = input.session.as_str();
    let at = |rel: &str| format!("{session}/{rel}");
    let has = |name: &str| input.top_level.iter().any(|entry| entry == name);
    let plan = |steps: Vec<PlanStep>| Plan {
        verb: "record-rename".to_owned(),
        session: input.session.clone(),
        steps,
    };

    if !has(ABOUT) {
        return Ok(plan(Vec::new()));
    }

    let mut steps = Vec::new();

    // 1. The shape file a hand-built session never had. Before the move, or the
    //    move is what turns the session folder-shaped.
    if !has(AGENTS) {
        steps.push(PlanStep::WriteFile {
            path: at(AGENTS),
            content: agents_md(&input.title),
        });
    }

    // 2. Whatever is standing on the destination. A signpost is an older
    //    migration's own output; a foreign README in a session with no AGENTS.md
    //    is a pool file under the contract that session was written under. Both
    //    are TRASHED — into `.keeper/trash/` under distinct keys, so a person who
    //    disagrees gets their bytes back and can see which of the two happened.
    //    A foreign README beside an AGENTS.md is the one shape keeper refuses:
    //    see [`RecordRenameError::Collision`].
    if has(README) {
        let signpost = is_signpost(&input.readme);
        if !signpost && has(AGENTS) {
            return Err(RecordRenameError::Collision {
                from: at(ABOUT),
                to: at(README),
            });
        }
        let why = if signpost {
            "record-signpost"
        } else {
            "foreign-readme"
        };
        steps.push(PlanStep::TrashFile {
            path: at(README),
            trash_key: format!("{}-{why}", session.replace('/', "-")),
        });
    }

    // 3. The prose that names the record, zone-wide. Before the move for no
    //    reason but the move being last; a pointer rewritten a moment early
    //    dangles for the length of one rename.
    steps.extend(pointer_rewrites(input));

    // 4. The rename itself, and the only step that cannot be replayed for free.
    steps.push(PlanStep::MoveFile {
        from: at(ABOUT),
        to: at(README),
    });

    Ok(plan(steps))
}

/// One [`PlanStep::GuardedWrite`] per zone file whose prose names the record.
///
/// Guarded rather than written, for [`crate::sessions::files::Rewrite`]'s reason:
/// the bytes were read when the plan was compiled, and a file an agent has
/// touched since should refuse the migration rather than lose the edit.
///
/// ## Two spellings, because a session file can be named two ways
///
/// **`about.md`, beside the file that says it.** [`refs::rewrite_pointers`]
/// matches the bare name, its wikilink stem, and either written relative to the
/// holding file's own directory — the three forms [`refs::resolve`] probes first.
/// This is the spelling almost every pointer uses, and it is the one the
/// [`RecordRenameInput::with_about`] exclusion protects: `[[about]]` inside a
/// session that still has its own `about.md` resolves to THAT session's record,
/// so it is left for that session's own run.
///
/// **`60-sessions/active/…/about.md`, from the drive root.** The zone-wide scope
/// is justified by a continuation link crossing sessions, and this is the only
/// spelling by which it can: bare `about.md` in another session's file resolves
/// beside that file and then beside that session, never here, so a genuine
/// cross-session pointer has to name the record in full — `candidates`' third
/// probe, "the target as written", which is what the drives' own `AGENTS.md`
/// asks for. Without this pass the zone-wide sweep reached exactly what a
/// per-session sweep reaches and the scope paid for nothing.
///
/// That second pass is **not** subject to the `with_about` exclusion, and for the
/// exclusion's own reason: a path naming this session's folder resolves to this
/// session's record no matter who wrote it, so there is no working link to
/// preserve. It is skipped only when it would be the first pass repeated — a zone
/// at the drive root spells both the same way, and [`refs::rewrite_pointers`]
/// answers `None` for `from == to` anyway.
///
/// **The record itself is never in here.** Its bytes travel verbatim — that is
/// the whole reason this verb moves the file instead of recomposing it — so a
/// pointer the record holds *at itself* survives the rename stale. That is the
/// deliberate half of the trade: one dangling link inside one file, against a
/// guarantee that every byte of frontmatter, every `keeper:` lineage entry and
/// the `pinned` flag arrive untouched.
fn pointer_rewrites(input: &RecordRenameInput) -> Vec<PlanStep> {
    let record = format!("{}/{}", input.session, ABOUT);
    let from_root = |rel: &str| match input.prefix.as_str() {
        "" => format!("{}/{rel}", input.session),
        prefix => format!("{prefix}/{}/{rel}", input.session),
    };
    let (qualified_from, qualified_to) = (from_root(ABOUT), from_root(README));
    input
        .pointers
        .iter()
        .filter(|file| file.path() != record)
        .filter_map(|file| {
            let beside = file.session == input.session || !input.with_about.contains(&file.session);
            let mut rewritten = if beside {
                refs::rewrite_pointers(&file.text, &file.rel, ABOUT, README)
            } else {
                None
            };
            if let Some(next) = refs::rewrite_pointers(
                rewritten.as_deref().unwrap_or(&file.text),
                &file.rel,
                &qualified_from,
                &qualified_to,
            ) {
                rewritten = Some(next);
            }
            rewritten.map(|content| PlanStep::GuardedWrite {
                path: file.path(),
                expect_len: file.text.len(),
                content,
            })
        })
        .collect()
}

/// Whether these bytes are the redirect an older `compile_migrate` wrote where
/// the README used to be.
///
/// **Two facts together, and the pair is the whole test:** the file is tagged
/// `ref` — the tag that migration gave its signpost so it would not sit
/// permanently in `unfiled` — and it names `about.md`. A README a person wrote
/// carries their own frontmatter and does not send its reader to another file in
/// the same folder; one that somehow does both is *trashed* rather than unlinked,
/// so the cost of guessing wrong is a file in `.keeper/trash/`.
///
/// Recognised rather than regenerated. Story 52.1 deleted the writer, and
/// comparing against a regenerated copy would have made a reader of old bytes
/// depend on a writer nothing calls — dead code kept alive by one caller, which
/// is how a "temporary" compatibility path becomes permanent.
fn is_signpost(readme: &str) -> bool {
    let (fm, body_at) = Frontmatter::parse(readme);
    let tagged = fm
        .as_list("tags")
        .is_some_and(|tags| tags.iter().any(|tag| tag == KindTag::Ref.as_str()));
    tagged && readme[body_at..].contains(ABOUT)
}

/// Which of the two record names a session's record is actually at, given both
/// files' bytes as the shell read them — and those bytes back, so a caller that
/// needs to guard a write on them cannot pair the name with a second read.
///
/// `README.md` is the record under both contracts (story 52.1), and this exists
/// because saying so did not move anybody's files: until
/// `sessions_record_migrate` has swept a zone, a flat session written before that
/// story keeps its record at `about.md`, and any caller that must WRITE to the
/// record has to know which. A caller that only reads the record can take
/// `README.md` and degrade; a caller compiling a
/// [`crate::sessions::plan::PlanStep::GuardedWrite`] cannot, because a guard read
/// from one file and written to another always mismatches — and in the
/// half-migrated shape it does something worse than mismatch, see below.
///
/// **The `id` is the discriminator, not the filename order.** A README-first
/// preference gets the half-migrated session wrong: `AGENTS.md` + `about.md` + the
/// signpost an older `compile_migrate` left where the README was. There a README
/// exists, so a name chosen by order names the signpost — whose length matches its
/// own bytes, so a guarded write onto it is *accepted*, and a session's lineage
/// lands in a three-line redirect. The record is the file carrying the session's
/// identity, which is the same fact `sessions_root::row_for` degrades on when it
/// finds none, so that is what is asked.
///
/// `None` only when the session has neither file, which is a session with no
/// record at all — the shape `row_for` renders from a `path:` id.
#[must_use]
pub fn record_at<'a>(
    readme: Option<&'a str>,
    about: Option<&'a str>,
) -> Option<(&'static str, &'a str)> {
    let carries_id = |text: &str| {
        let (fm, _) = Frontmatter::parse(text);
        fm.get("id")
            .is_some_and(|id| !id.index_string().trim().is_empty())
    };
    match (readme, about) {
        // Both present: the README wins unless it is the one without an identity
        // and the `about.md` has one. Neither having one is the degraded session,
        // and there the contract's own name is the honest answer.
        (Some(readme), Some(about)) => Some(if carries_id(readme) || !carries_id(about) {
            (README, readme)
        } else {
            (ABOUT, about)
        }),
        (Some(readme), None) => Some((README, readme)),
        (None, Some(about)) => Some((ABOUT, about)),
        (None, None) => None,
    }
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
///
/// **The inverse of [`kind_dir`], and derived from it rather than written
/// beside it.** That function is the authoritative direction — where a create
/// of a given kind goes under a given shape — and this asks it once per kind
/// instead of restating `refs/ → Ref`. A fourth directory added there therefore
/// arrives here for free, which is the whole reason the two are not two
/// tables: a mapping that exists twice is a mapping that will disagree with
/// itself the first time one half is extended.
///
/// [`Shape::Folder`] because that is the shape being migrated *away from*; a
/// flat session keeps everything at the root and has no directory to read a
/// kind out of.
///
/// [`KINDS`] order decides a tie, as it does in [`KindTag::of`] — though no two
/// directories in the folder contract nest, so no tie is reachable today.
fn carried_kind(rel: &str) -> Option<KindTag> {
    KINDS
        .into_iter()
        .find(|kind| match kind_dir(Shape::Folder, *kind, "") {
            // `strip_prefix` plus the separator rather than
            // `starts_with("refs/")`: no allocation, and `refsy/x.md` is a
            // different folder that a bare `starts_with(dir)` would match.
            Ok(Some(dir)) => rel
                .strip_prefix(dir)
                .is_some_and(|rest| rest.starts_with('/')),
            // A kind with no directory under this contract cannot be carried
            // out of one, and the root is not a directory a file is "leaving".
            Ok(None) | Err(_) => false,
        })
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
         1. `README.md` — what this session is for, what was decided, and the promote table.\n\
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
         `artifacts/` and record the move in `README.md`'s promote table.\n\n\
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
    ///
    /// Row 2 is the same assertion for a new reason (story 52.1): an `about.md`
    /// at the root used to make the session read Flat, and now makes it the
    /// *record rename's* session. Either way this verb declines.
    #[test]
    fn an_already_flat_session_compiles_to_nothing() {
        assert!(compile_migrate(&input(LIVE, &["AGENTS.md", "artifacts"], &[])).is_none());
        assert!(compile_migrate(&input(LIVE, &["about.md"], &[])).is_none());
        assert!(
            compile_migrate(&input(LIVE, &["AGENTS.md", "about.md", "README.md"], &[])).is_none(),
            "story 52.1: a half-migrated session is the record rename's, not this verb's"
        );
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
        assert!(
            position("/README.md") < position("/AGENTS.md"),
            "story 52.1: the record is rewritten where it is, and that write used to be an \
             about.md — it still lands before the shape flip"
        );
        assert!(position("2026-08-10-0000-opened.md") < position("/AGENTS.md"));
        assert!(position("/inputs.md") < position("/AGENTS.md"));
        assert!(position("/AGENTS.md") < position("/refs"));

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
    /// produces a well-formed record. Story 52.1: that record is the `README.md`
    /// itself, rewritten in place, where it used to be a new `about.md`.
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

        let about = find("/README.md");
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
            .find(|(path, _)| path.ends_with("/README.md"))
            .expect("the record")
            .1;
        let (fm, body_at) = Frontmatter::parse(about);
        assert!(fm.unparsed().is_none(), "the write parses clean: {about}");
        assert!(fm.as_string("id").is_some(), "authored, so stamped");
        assert_eq!(fm.as_string("created"), Some("2026-08-13"));
        assert_eq!(fm.as_list("tags"), Some(vec!["about".to_owned()]));
        assert!(about[body_at..].starts_with("# keeper — rolling work session"));
    }

    /// The session's identity is not a thing a migration gets to change: an
    /// existing id, `pinned`, and both lineage directions stay in the record
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
            .find(|(path, _)| path.ends_with("/README.md"))
            .expect("the record")
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

    /// The migration empties **every** directory the folder contract keeps,
    /// because it asks [`kind_dir`] for the list rather than holding one.
    ///
    /// This is the other half of [`carried_kind`]'s claim that a fourth
    /// directory "arrives here for free": reading a kind back out of it is
    /// worthless if the directory is then left on disk by the verb whose job
    /// was to empty it. Written as a set comparison against the mapping itself
    /// rather than against `["refs", "prompts"]`, so a fourth directory turns
    /// this green instead of turning it into the second table to update.
    #[test]
    fn the_migration_trashes_exactly_the_directories_the_mapping_names() {
        let dirs: Vec<&str> = KINDS
            .into_iter()
            .filter_map(|kind| kind_dir(Shape::Folder, kind, "").ok().flatten())
            .collect();
        assert!(!dirs.is_empty(), "the folder contract keeps directories");

        let mut top_level = vec!["README.md", "artifacts", "workspace"];
        top_level.extend(dirs.iter().copied());
        let plan = compile_migrate(&input("# s\n", &top_level, &[])).expect("plan");

        let mut trashed: Vec<&str> = plan
            .steps
            .iter()
            .filter_map(|step| match step {
                PlanStep::TrashDir { path, .. } => Some(path.as_str()),
                _ => None,
            })
            .collect();
        trashed.sort_unstable();
        let mut expected: Vec<String> = dirs
            .iter()
            .map(|dir| format!("active/2026-08-10-keeper/{dir}"))
            .collect();
        expected.sort_unstable();
        assert_eq!(trashed, expected);
        // `artifacts/` and `workspace/` are not kind directories and the flat
        // contract keeps both (AD-119), so neither is ever a trash step.
        assert!(
            !trashed
                .iter()
                .any(|path| path.ends_with("artifacts") || path.ends_with("workspace")),
            "the two non-markdown subtrees survive migration: {trashed:?}"
        );
    }

    /// The record write is guarded on the README's current length, so an agent
    /// editing it while the operator migrates refuses rather than losing the edit.
    ///
    /// **Story 52.1 inverted what this file becomes.** It asserted the README was
    /// rewritten into a three-line signpost pointing at `about.md`. The record IS
    /// the README now, so the guarded write carries the record itself — and the
    /// old assertion, that the content names `about.md`, is exactly the thing that
    /// must no longer be true.
    #[test]
    fn the_record_is_rewritten_in_place_and_guarded() {
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
        assert!(
            content.contains("State as of opening. Two tracks."),
            "the record's own prose, not a redirect to it"
        );
        assert!(
            !content.contains("about.md"),
            "story 52.1: there is no signpost, because there is nowhere to point: {content}"
        );
        assert!(
            content.contains("keeper — rolling work session"),
            "and the session keeps its name"
        );

        // One write to the README and one only: the record used to be composed
        // into `about.md` and the README overwritten a second time, and a plan
        // with two writes to one path is a plan whose order decides the outcome.
        assert_eq!(
            plan.steps
                .iter()
                .filter(|step| matches!(
                    step,
                    PlanStep::WriteFile { path, .. } | PlanStep::GuardedWrite { path, .. }
                        if path.ends_with("/README.md")
                ))
                .count(),
            1
        );
    }

    /// A folder-shaped session with no README at all still migrates: there is
    /// nothing to guard, so the record is an ordinary write.
    ///
    /// `GuardedWrite` reads its target before comparing, so guarding a file that
    /// does not exist would refuse the migration of a session whose whole content
    /// is a `refs/` folder.
    #[test]
    fn a_session_with_no_readme_writes_its_record_unguarded() {
        let plan = compile_migrate(&input("", &["refs"], &[("refs/a.md", "# a\n")]))
            .expect("a session that is nothing but references still migrates");
        assert!(
            matches!(
                &plan.steps[0],
                PlanStep::WriteFile { path, .. } if path == "active/2026-08-10-keeper/README.md"
            ),
            "an absent README is written, not guarded: {:?}",
            plan.steps[0]
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
                .filter(|(path, _)| !path.ends_with("/AGENTS.md"))
                .count();
            assert_eq!(expected, used, "one id per authored pool file");
            // Every one of them actually carries the id it was given.
            for (path, content) in writes(&plan) {
                if path.ends_with("/AGENTS.md") {
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
            .filter(|(path, _)| !path.ends_with("/AGENTS.md"))
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
        //
        // Story 52.1 changed the ORDER as a consequence of the rename: the pool
        // sorts by folded name, so `README.md` now sorts after `AGENTS.md` where
        // `about.md` sorted before it. Asserted rather than papered over, because
        // a surface that wants the record first has to sort for it and not hope.
        assert_eq!(
            pool.about
                .iter()
                .map(|e| e.rel.as_str())
                .collect::<Vec<_>>(),
            ["AGENTS.md", "README.md"],
            "the folded-name sort, which after the rename puts the navigation file first"
        );
        assert_eq!(pool.logs.len(), 3, "every sitting became a file");
        assert_eq!(
            pool.logs[0].date, "2026-08-12",
            "newest first, including the half-written one"
        );
        assert_eq!(pool.logs[2].title, "opened");
        assert_eq!(
            pool.refs.iter().map(|e| e.rel.as_str()).collect::<Vec<_>>(),
            ["inputs.md"],
            "story 52.1: the carried pointer and nothing else — the README signpost \
             that used to be filed here does not exist any more"
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

    /// The two directions are one mapping (Story 50.1).
    ///
    /// `kind_dir` says where a create of a kind goes; this says which kind a
    /// file gains from the directory it is leaving. They are derived from one
    /// another, and this is the test that would fail if someone re-introduced
    /// the second table: every directory the forward mapping hands out round
    /// trips, and a folder that merely *starts with* one of those names does
    /// not.
    #[test]
    fn carrying_a_file_out_of_a_directory_inverts_the_create_mapping() {
        for kind in KINDS {
            let Ok(Some(dir)) = kind_dir(Shape::Folder, kind, "") else {
                continue;
            };
            assert_eq!(
                carried_kind(&format!("{dir}/inputs.md")),
                Some(kind),
                "{dir} is where a {} create goes, so a file there is one",
                kind.as_str()
            );
        }
        // A prefix is not a directory: `refsy/` and a root-level file are both
        // outside the contract, and a `starts_with` that forgot the separator
        // would file the first as a reference.
        assert_eq!(carried_kind("refsy/inputs.md"), None);
        assert_eq!(carried_kind("references.md"), None);
        assert_eq!(carried_kind("artifacts/out.md"), None);
        // The kinds the folder contract has no directory for cannot be carried
        // out of one either — there is nowhere they could have been.
        assert_eq!(carried_kind("tasks/a.md"), None);
        assert_eq!(carried_kind("logs/a.md"), None);
    }

    // -----------------------------------------------------------------------
    // The record's own rename (story 52.1) — the spec's acceptance table
    // -----------------------------------------------------------------------

    /// The session under test, and a second one that links at its record.
    const SESSION: &str = "active/2026-08-10-keeper";
    const OTHER: &str = "active/2026-08-01-old";

    /// A record somebody edited by hand, with everything a recomposition would
    /// quietly normalise: trailing whitespace, a tab, a `pinned` flag, a
    /// `keeper:` lineage map and a promote table.
    const HAND_EDITED: &str = "---\nid: 01J5AAAAAAAAAAAAAAAAAAAAAA\ncreated: 2026-08-10\n\
                               pinned: true\ntags: [about]\nkeeper:\n  \
                               session-continues: [01J4ZZZZZZZZZZZZZZZZZZZZZZ]\n---\n\
                               # keeper — rolling work session\n\n\
                               A line with trailing spaces   \nand a tab\there.\n\n\
                               ## Promote\n\n| workspace | → artifacts | note |\n\
                               | --------- | ----------- | ---- |\n";

    /// The three-line redirect an older `compile_migrate` wrote where the README
    /// was, reproduced here because story 52.1 deleted the writer — this is the
    /// shape `is_signpost` has to recognise on somebody's live drive.
    const SIGNPOST: &str = "---\ntags: [ref]\n---\n# keeper\n\n\
                            This session follows the flat contract: the record moved to \
                            [about.md](about.md), and every other file says what it is in its own \
                            frontmatter `tags:`. Read [AGENTS.md](AGENTS.md) first.\n";

    /// The zone's own place on the drive — what a pointer in another session has
    /// to spell to reach this record at all.
    const ZONE: &str = "60-sessions";

    fn rename_input(top_level: &[&str]) -> RecordRenameInput {
        RecordRenameInput {
            session: SESSION.to_owned(),
            top_level: top_level.iter().map(|s| (*s).to_owned()).collect(),
            readme: String::new(),
            title: "keeper — rolling work session".to_owned(),
            pointers: Vec::new(),
            prefix: ZONE.to_owned(),
            with_about: vec![SESSION.to_owned()],
        }
    }

    fn at(session: &str, rel: &str) -> String {
        format!("{session}/{rel}")
    }

    /// The plan's effect on a zone, as a map of zone-relative path → bytes.
    ///
    /// A four-step interpreter, and deliberately only the four steps
    /// [`compile_record_rename`] emits, with the semantics `sessions_exec` gives
    /// them: a guarded write that recognises its own output, a trash that keeps
    /// the basename under its key, a move that refuses an occupied target. It
    /// exists because this crate touches no filesystem by rule — `text_file`'s
    /// own note says as much — and the acceptance this verb owes is a claim about
    /// *bytes after a run*, not about a list of steps.
    fn apply(zone: &mut std::collections::BTreeMap<String, String>, plan: &Plan) {
        for step in &plan.steps {
            match step {
                PlanStep::WriteFile { path, content } => {
                    zone.insert(path.clone(), content.clone());
                }
                PlanStep::GuardedWrite {
                    path,
                    expect_len,
                    content,
                } => {
                    let current = zone
                        .get(path)
                        .expect("a guarded write reads its target first");
                    assert!(
                        current.len() == *expect_len || current == content,
                        "{path} changed under the plan"
                    );
                    zone.insert(path.clone(), content.clone());
                }
                PlanStep::TrashFile { path, trash_key } => {
                    let name = path.rsplit('/').next().expect("a basename");
                    let bytes = zone
                        .remove(path)
                        .expect("a trash step names a file that is there");
                    zone.insert(format!(".keeper/trash/{trash_key}/{name}"), bytes);
                }
                PlanStep::MoveFile { from, to } => {
                    assert!(
                        !zone.contains_key(to),
                        "{to} is occupied — the move would refuse"
                    );
                    let bytes = zone
                        .remove(from)
                        .expect("a move names a file that is there");
                    zone.insert(to.clone(), bytes);
                }
                other => panic!("this verb does not emit {other:?}"),
            }
        }
    }

    /// One session's top-level entry names, out of a zone map — what `shape`
    /// reads.
    fn top_level_of(
        zone: &std::collections::BTreeMap<String, String>,
        session: &str,
    ) -> Vec<String> {
        let prefix = format!("{session}/");
        zone.keys()
            .filter_map(|path| path.strip_prefix(&prefix))
            .filter(|rel| !rel.contains('/'))
            .map(str::to_owned)
            .collect()
    }

    /// Row 2. A keeper-created flat session moves its record and does nothing
    /// else, and the bytes that arrive at `README.md` are the bytes that were at
    /// `about.md` — every one of them, because a rename carries no content and
    /// therefore cannot normalise any.
    #[test]
    fn a_keeper_created_flat_session_moves_its_record_and_every_byte_survives() {
        let input = rename_input(&[AGENTS, ABOUT, "artifacts", "workspace"]);
        let plan = compile_record_rename(&input).expect("nothing is in the way");

        assert_eq!(plan.verb, "record-rename");
        assert_eq!(plan.session, SESSION);
        assert_eq!(
            plan.steps,
            vec![PlanStep::MoveFile {
                from: at(SESSION, ABOUT),
                to: at(SESSION, README),
            }],
            "one move, and nothing that carries bytes"
        );

        let mut zone =
            std::collections::BTreeMap::from([(at(SESSION, ABOUT), HAND_EDITED.to_owned())]);
        apply(&mut zone, &plan);
        assert_eq!(
            zone.get(&at(SESSION, README)).map(String::as_str),
            Some(HAND_EDITED),
            "byte for byte: trailing spaces, the tab, the promote table"
        );
        assert!(
            !zone.contains_key(&at(SESSION, ABOUT)),
            "and the old name is gone rather than duplicated"
        );

        // The three facts the board would silently lose to a recomposition.
        let (fm, _) = Frontmatter::parse(&zone[&at(SESSION, README)]);
        assert_eq!(fm.as_string("id"), Some("01J5AAAAAAAAAAAAAAAAAAAAAA"));
        assert_eq!(fm.as_bool("pinned"), Some(true));
        assert_eq!(
            crate::sessions::model::lineage(&fm).continues,
            vec!["01J4ZZZZZZZZZZZZZZZZZZZZZZ"]
        );
    }

    /// Row 3. A hand-built `about.md`-only session is written its `AGENTS.md`
    /// FIRST, and reads [`Shape::Flat`] afterwards.
    ///
    /// Without that step the move alone would leave a session whose record is in
    /// the right place and whose shape says folder — every log invisible behind a
    /// `## Log` heading that is not there. The last assertion is the proof that
    /// the write is what prevents it, rather than a coincidence of the listing.
    #[test]
    fn a_hand_built_session_gets_agents_md_before_the_move_and_still_reads_flat() {
        let input = rename_input(&[ABOUT, "2026-08-12-0900-opened.md"]);
        assert_eq!(
            shape(&input.top_level),
            Shape::Folder,
            "story 52.1: which is exactly why AGENTS.md has to be written"
        );

        let plan = compile_record_rename(&input).expect("nothing is in the way");
        assert!(
            matches!(
                &plan.steps[0],
                PlanStep::WriteFile { path, .. } if *path == at(SESSION, AGENTS)
            ),
            "the shape file is step one: {:?}",
            plan.steps
        );
        assert!(
            matches!(plan.steps.last(), Some(PlanStep::MoveFile { .. })),
            "and the move is last: {:?}",
            plan.steps
        );

        let mut zone = std::collections::BTreeMap::from([
            (at(SESSION, ABOUT), HAND_EDITED.to_owned()),
            (
                at(SESSION, "2026-08-12-0900-opened.md"),
                "---\ntags: [log]\n---\n# opened\n".to_owned(),
            ),
        ]);
        apply(&mut zone, &plan);

        let after = top_level_of(&zone, SESSION);
        assert_eq!(
            shape(&after),
            Shape::Flat,
            "the session still reads flat: {after:?}"
        );
        let without_agents: Vec<String> = after
            .iter()
            .filter(|name| *name != AGENTS)
            .cloned()
            .collect();
        assert_eq!(
            shape(&without_agents),
            Shape::Folder,
            "and the AGENTS.md write is the only reason it does"
        );
        assert_eq!(
            zone.get(&at(SESSION, README)).map(String::as_str),
            Some(HAND_EDITED),
            "the record still travelled verbatim"
        );
    }

    /// Row 4. A half-migrated session trashes the signpost before it moves, and
    /// the trash holds it — recoverable, never unlinked.
    #[test]
    fn a_half_migrated_session_trashes_the_signpost_first_and_the_trash_holds_it() {
        let mut input = rename_input(&[AGENTS, ABOUT, README]);
        input.readme = SIGNPOST.to_owned();

        let plan = compile_record_rename(&input).expect("a signpost is not a collision");
        let step_at = |want: fn(&PlanStep) -> bool| {
            plan.steps
                .iter()
                .position(want)
                .unwrap_or_else(|| panic!("no such step: {:?}", plan.steps))
        };
        assert!(
            step_at(|step| matches!(step, PlanStep::TrashFile { .. }))
                < step_at(|step| matches!(step, PlanStep::MoveFile { .. })),
            "the destination is cleared before the move, or the move refuses itself"
        );

        let mut zone = std::collections::BTreeMap::from([
            (at(SESSION, ABOUT), HAND_EDITED.to_owned()),
            (at(SESSION, README), SIGNPOST.to_owned()),
        ]);
        apply(&mut zone, &plan);

        assert_eq!(
            zone.get(".keeper/trash/active-2026-08-10-keeper-record-signpost/README.md")
                .map(String::as_str),
            Some(SIGNPOST),
            "recoverable in the zone's trash: {:?}",
            zone.keys().collect::<Vec<_>>()
        );
        assert_eq!(
            zone.get(&at(SESSION, README)).map(String::as_str),
            Some(HAND_EDITED),
            "and the record is what is at the name now"
        );
    }

    /// Row 5. A session with no `about.md` is untouched — including its prose,
    /// which is what would otherwise be rewritten for a rename that is not
    /// happening.
    #[test]
    fn a_session_with_nothing_to_rename_compiles_an_empty_plan() {
        for top in [
            vec![README, "refs", "prompts"],
            vec![AGENTS, README],
            vec![AGENTS],
        ] {
            let mut input = rename_input(&top);
            input.readme = "# a record somebody wrote\n".to_owned();
            input.with_about = Vec::new();
            input.pointers = vec![PointerFile {
                session: SESSION.to_owned(),
                rel: "2026-08-11-0900-note.md".to_owned(),
                text: "See [[about]].\n".to_owned(),
            }];
            let plan = compile_record_rename(&input).expect("nothing to refuse");
            assert!(
                plan.steps.is_empty(),
                "{top:?} has no about.md to move: {:?}",
                plan.steps
            );
        }
    }

    /// Row 6. A `README.md` nobody's migration wrote refuses the move **in a session
    /// that has its `AGENTS.md`**, naming both paths, and the three near-misses are
    /// refused too: the signpost is recognised by its `ref` tag AND its pointer,
    /// never by either alone.
    ///
    /// The remedy the sentence offers is asserted, not just its presence. The
    /// original said "move {to} aside", which is the one move keeper forbids —
    /// `check_deletable` refuses a delete of `README.md` and `RECORD_NAMES`
    /// refuses renaming it — so a refusal that said it sent the operator to press
    /// two buttons that both decline.
    #[test]
    fn a_readme_that_is_not_a_signpost_refuses_and_names_both_paths() {
        let mut input = rename_input(&[AGENTS, ABOUT, README]);
        input.readme =
            "---\nid: 01J5CCCCCCCCCCCCCCCCCCCCCC\n---\n# The session, as written\n".to_owned();

        let error = compile_record_rename(&input).expect_err("a foreign README is in the way");
        assert_eq!(
            error,
            RecordRenameError::Collision {
                from: at(SESSION, ABOUT),
                to: at(SESSION, README),
            }
        );
        let said = error.to_string();
        assert!(
            said.contains(&at(SESSION, ABOUT)) && said.contains(&at(SESSION, README)),
            "both paths, because the person has to open one and decide about the other: {said}"
        );
        assert!(
            said.contains("AGENTS.md") && said.contains("already reads"),
            "the true reason: keeper is reading the README as this session's record: {said}"
        );
        assert!(
            !said.contains("aside"),
            "and never a remedy keeper's own verbs refuse: {said}"
        );

        // Tagged `ref` but pointing at nothing: somebody's own notes file.
        input.readme = "---\ntags: [ref]\n---\n# Links I keep here\n".to_owned();
        assert!(compile_record_rename(&input).is_err());
        // The signpost's prose without its tag: a README that mentions the record.
        input.readme = "# see [about.md](about.md) for the record\n".to_owned();
        assert!(compile_record_rename(&input).is_err());
        // Both together is the signpost, and only then.
        input.readme = SIGNPOST.to_owned();
        assert!(compile_record_rename(&input).is_ok());
    }

    /// The `{about.md, README.md}` session, which had no way forward at all.
    ///
    /// Exactly two of the eight `{AGENTS.md, about.md, README.md}` combinations
    /// changed meaning when story 52.1 narrowed `shape()` to `has(AGENTS)`, and
    /// this is the one the spec did not cover: a hand-built flat session somebody
    /// dropped a README into (a create is unrestricted, `files::compile_new`).
    /// Before, it was [`Shape::Flat`] with `about.md` for a record. After, it is
    /// [`Shape::Folder`] and `sessions_root::row_for` reads the record out of a
    /// different file — while the id, the pins and the lineage stay in `about.md`.
    /// Then NEITHER verb moved it: [`compile_migrate`] declines an `about.md` at
    /// the root and this one refused, and the refusal's remedy was a move
    /// `check_deletable` and `RECORD_NAMES` both forbid. The only exit was Finder.
    ///
    /// So the foreign README gets the signpost's treatment — trashed under its own
    /// key, recoverable, distinguishable in `.keeper/trash/` from a signpost — and
    /// the session ends the run genuinely flat, with its own record at `README.md`
    /// and every byte of it intact.
    #[test]
    fn a_hand_built_session_with_a_foreign_readme_trashes_it_instead_of_stranding_the_session() {
        const FOREIGN: &str = "# Notes to whoever finds this\n\nNothing to do with keeper.\n";

        let mut input = rename_input(&[ABOUT, README]);
        input.readme = FOREIGN.to_owned();

        let plan = compile_record_rename(&input).expect("the way out is not a refusal");
        assert_eq!(
            plan.steps,
            vec![
                PlanStep::WriteFile {
                    path: at(SESSION, AGENTS),
                    content: agents_md(&input.title),
                },
                PlanStep::TrashFile {
                    path: at(SESSION, README),
                    trash_key: "active-2026-08-10-keeper-foreign-readme".to_owned(),
                },
                PlanStep::MoveFile {
                    from: at(SESSION, ABOUT),
                    to: at(SESSION, README),
                },
            ],
            "{:?}",
            plan.steps
        );

        // What the zone holds afterwards: the record where every reader now looks,
        // byte for byte, and somebody's README recoverable under a key that says
        // which branch took it.
        let mut zone = std::collections::BTreeMap::from([
            (at(SESSION, ABOUT), HAND_EDITED.to_owned()),
            (at(SESSION, README), FOREIGN.to_owned()),
        ]);
        apply(&mut zone, &plan);
        assert_eq!(zone[&at(SESSION, README)], HAND_EDITED);
        assert_eq!(
            zone[".keeper/trash/active-2026-08-10-keeper-foreign-readme/README.md"], FOREIGN,
            "trashed, never unlinked: the cost of keeper guessing wrong is one file to restore"
        );
        assert!(!zone.contains_key(&at(SESSION, ABOUT)));
        assert_eq!(
            shape(&top_level_of(&zone, SESSION)),
            Shape::Flat,
            "and the session reads flat, which is the whole point of writing AGENTS.md first"
        );

        // A signpost still says so: the two branches are told apart in the trash,
        // because a person restoring one wants to know which happened.
        input.readme = SIGNPOST.to_owned();
        let signposted = compile_record_rename(&input).expect("a signpost is not a collision");
        assert!(
            signposted.steps.contains(&PlanStep::TrashFile {
                path: at(SESSION, README),
                trash_key: "active-2026-08-10-keeper-record-signpost".to_owned(),
            }),
            "{:?}",
            signposted.steps
        );
    }

    /// The spelling the zone-wide scope is actually justified by.
    ///
    /// Row 7 proves a BARE `about.md` in another session is rewritten, and the
    /// doc justified sweeping the whole zone with "a continuation link crosses
    /// sessions" — but a bare name cannot cross one. `refs::resolve` probes it
    /// beside the file that holds it and then beside that file's OWN session, so
    /// `about.md` written in `active/2026-08-01-old` names that session's record
    /// and never this one. A pointer that genuinely reaches across has to spell
    /// the record from the drive root, which is `candidates`' third probe and the
    /// form the drives' own `AGENTS.md` asks for. Nothing rewrote it before this
    /// test, so the zone-wide pass reached exactly what a one-session pass reaches
    /// and the scope was paid for and unused.
    ///
    /// Both link spellings, the wikilink stem, and a promote-table cell — the
    /// three things `rewrite_pointers` knows how to follow — plus the one case the
    /// `with_about` exclusion must NOT swallow: a qualified path names THIS
    /// session's folder, so it resolves here whoever wrote it, even a session
    /// still holding its own unmigrated record.
    #[test]
    fn a_cross_session_pointer_spelled_from_the_drive_root_is_rewritten() {
        const UNMIGRATED: &str = "active/2026-07-01-older";

        let mut input = rename_input(&[AGENTS, ABOUT]);
        input.with_about = vec![SESSION.to_owned(), UNMIGRATED.to_owned()];
        input.pointers = vec![
            PointerFile {
                session: OTHER.to_owned(),
                rel: "2026-08-02-0900-handed-over.md".to_owned(),
                text: format!(
                    "Continues [the record]({ZONE}/{SESSION}/{ABOUT}), see \
                     [[{ZONE}/{SESSION}/about]].\n\n## Promote\n\n\
                     | workspace | → artifacts | note |\n| --------- | ----------- | ---- |\n\
                     | {ZONE}/{SESSION}/{ABOUT} | out.md | the record |\n"
                ),
            },
            PointerFile {
                session: UNMIGRATED.to_owned(),
                rel: "2026-07-02-0900-note.md".to_owned(),
                text: format!(
                    "Its own record: [[about]]. Ours: [ours]({ZONE}/{SESSION}/{ABOUT}).\n"
                ),
            },
        ];

        let plan = compile_record_rename(&input).expect("nothing is in the way");
        let of = |path: &str| {
            plan.steps.iter().find_map(|step| match step {
                PlanStep::GuardedWrite {
                    path: at, content, ..
                } if at == path => Some(content.clone()),
                _ => None,
            })
        };

        let crossed = of(&at(OTHER, "2026-08-02-0900-handed-over.md"))
            .expect("a pointer spelled from the drive root is one the resolver resolves");
        assert!(
            crossed.contains(&format!("[the record]({ZONE}/{SESSION}/{README})")),
            "the markdown destination: {crossed}"
        );
        assert!(
            crossed.contains(&format!("[[{ZONE}/{SESSION}/README]]")),
            "the wikilink, which names the stem: {crossed}"
        );
        assert!(
            crossed.contains(&format!("{ZONE}/{SESSION}/{README} |")),
            "and the promote row, which no link parser can see: {crossed}"
        );
        assert!(
            !crossed.contains(ABOUT),
            "nothing left naming the old file: {crossed}"
        );

        let unmigrated = of(&at(UNMIGRATED, "2026-07-02-0900-note.md"))
            .expect("the qualified spelling is rewritten even where the bare one is not");
        assert!(
            unmigrated.contains("[[about]]"),
            "that session's own record still resolves for it: {unmigrated}"
        );
        assert!(
            unmigrated.contains(&format!("[ours]({ZONE}/{SESSION}/{README})")),
            "and ours is followed: {unmigrated}"
        );

        // A zone AT the drive root spells both the same way, and the two passes
        // then have nothing to disagree about.
        input.prefix = String::new();
        input.pointers = vec![PointerFile {
            session: OTHER.to_owned(),
            rel: "note.md".to_owned(),
            text: format!("See [it]({SESSION}/{ABOUT}).\n"),
        }];
        let rooted = compile_record_rename(&input).expect("nothing is in the way");
        assert!(
            rooted.steps.iter().any(|step| matches!(
                step,
                PlanStep::GuardedWrite { content, .. }
                    if content == &format!("See [it]({SESSION}/{README}).\n")
            )),
            "{:?}",
            rooted.steps
        );
    }

    /// Row 7. A pointer at the record written in ANOTHER session is rewritten by
    /// the zone-wide pass — the reason the pass is zone-wide at all, since a
    /// continuation names what it continues.
    ///
    /// Three other facts in one test, because they are the same decision seen
    /// from three sides: this session's own prose is rewritten, the record itself
    /// never is (its bytes travel verbatim, so a self-link is left stale on
    /// purpose), and a session still holding its own `about.md` is left for its
    /// own rename — `[[about]]` there resolves to THAT session's record.
    #[test]
    fn a_pointer_in_another_session_is_rewritten_and_an_unmigrated_ones_is_not() {
        const UNMIGRATED: &str = "active/2026-07-01-older";
        const SELF_LINK: &str = "---\ntags: [about]\n---\n# keeper\n\nSee [about.md](about.md).\n";

        let mut input = rename_input(&[AGENTS, ABOUT]);
        input.with_about = vec![SESSION.to_owned(), UNMIGRATED.to_owned()];
        input.pointers = vec![
            PointerFile {
                session: OTHER.to_owned(),
                rel: "2026-08-02-0900-handed-over.md".to_owned(),
                text: "Picked up from [[about]] and [the record](about.md).\n".to_owned(),
            },
            PointerFile {
                session: SESSION.to_owned(),
                rel: "spaces/plan.md".to_owned(),
                text: "Decided in [the record](about.md).\n".to_owned(),
            },
            PointerFile {
                session: SESSION.to_owned(),
                rel: ABOUT.to_owned(),
                text: SELF_LINK.to_owned(),
            },
            PointerFile {
                session: UNMIGRATED.to_owned(),
                rel: "2026-07-02-0900-note.md".to_owned(),
                text: "Its own record: [[about]].\n".to_owned(),
            },
        ];

        let plan = compile_record_rename(&input).expect("nothing is in the way");
        let rewritten: Vec<(&str, &str)> = plan
            .steps
            .iter()
            .filter_map(|step| match step {
                PlanStep::GuardedWrite { path, content, .. } => {
                    Some((path.as_str(), content.as_str()))
                }
                _ => None,
            })
            .collect();
        let of = |path: &str| {
            rewritten
                .iter()
                .find(|(candidate, _)| *candidate == path)
                .map(|(_, content)| *content)
        };

        assert_eq!(
            of(&at(OTHER, "2026-08-02-0900-handed-over.md")),
            Some("Picked up from [[README]] and [the record](README.md).\n"),
            "a continuation's link crosses sessions, and both spellings are rewritten"
        );
        assert_eq!(
            of(&at(SESSION, "spaces/plan.md")),
            Some("Decided in [the record](README.md).\n"),
            "and so is this session's own prose, in a subdirectory"
        );
        assert_eq!(
            of(&at(UNMIGRATED, "2026-07-02-0900-note.md")),
            None,
            "a session that still holds its own about.md keeps a link that works"
        );
        assert_eq!(
            of(&at(SESSION, ABOUT)),
            None,
            "the record is moved, never rewritten — which is what makes row 2 true"
        );

        // The stale self-link, stated rather than hidden: it is the price of the
        // byte-for-byte guarantee, and one dangling link inside one file is the
        // cheaper half of that trade.
        let mut zone = std::collections::BTreeMap::from([
            (at(SESSION, ABOUT), SELF_LINK.to_owned()),
            (
                at(SESSION, "spaces/plan.md"),
                "Decided in [the record](about.md).\n".to_owned(),
            ),
            (
                at(OTHER, "2026-08-02-0900-handed-over.md"),
                "Picked up from [[about]] and [the record](about.md).\n".to_owned(),
            ),
        ]);
        apply(&mut zone, &plan);
        assert!(
            zone[&at(SESSION, README)].contains("(about.md)"),
            "the record's own bytes are untouched, self-link included"
        );
    }

    /// The migration is one plan through the existing executor, and every step
    /// before the move replays for free (AD-111). The move sorts last for exactly
    /// that reason: `sessions_exec` refuses a `MoveFile` onto an occupied target,
    /// so a resume that re-runs it refuses over a verb that has already finished
    /// rather than over one that still has work to do.
    #[test]
    fn every_step_but_the_last_is_replayable_and_the_last_is_the_move() {
        let mut input = rename_input(&[ABOUT, README]);
        input.readme = SIGNPOST.to_owned();
        input.pointers = vec![PointerFile {
            session: SESSION.to_owned(),
            rel: "2026-08-11-0900-note.md".to_owned(),
            text: "See [[about]].\n".to_owned(),
        }];

        let plan = compile_record_rename(&input).expect("a signpost is not a collision");
        let (last, rest) = plan.steps.split_last().expect("a non-empty plan");
        assert!(matches!(last, PlanStep::MoveFile { .. }), "{last:?}");
        for step in rest {
            assert!(
                matches!(
                    step,
                    PlanStep::WriteFile { .. }
                        | PlanStep::GuardedWrite { .. }
                        | PlanStep::TrashFile { .. }
                ),
                "{step:?} is not a step this plan can replay"
            );
        }
        // All four cases at once: the write, the trash, the rewrite, the move.
        assert_eq!(plan.steps.len(), 4, "{:?}", plan.steps);
    }

    /// Which file a session's record is in, for the one caller that must WRITE to
    /// it: `sessions_ipc`'s create-from, whose lineage append is a guarded write.
    ///
    /// The half-migrated row is the one that matters. A name chosen by filename
    /// order picks the signpost, whose `expect_len` matches its own bytes — so the
    /// guard is satisfied and the session's `continued-by` is appended into a
    /// redirect instead of into its record. Asking for the `id` is what keeps the
    /// two apart.
    #[test]
    fn the_record_is_the_file_carrying_the_identity_not_the_first_name_tried() {
        let record = "---\nid: 01J5AAAAAAAAAAAAAAAAAAAAAA\ntags: [about]\n---\n# keeper\n";

        // Migrated, or folder-shaped: the contract's own name, holding the id.
        assert_eq!(record_at(Some(record), None), Some((README, record)));
        // Unmigrated flat: nothing at README.md at all.
        assert_eq!(record_at(None, Some(record)), Some((ABOUT, record)));
        // Half-migrated: both files, and only one of them is the session.
        assert_eq!(
            record_at(Some(SIGNPOST), Some(record)),
            Some((ABOUT, record)),
            "the signpost is not the record, however much it looks like one to a \
             filename test"
        );
        // Both carry an id — the shape `compile_record_rename` refuses to choose
        // between. A reader has to answer something, and the contract's name is
        // what every other reader in the codebase already uses.
        let older = "---\nid: 01J4ZZZZZZZZZZZZZZZZZZZZZZ\n---\n# older\n";
        assert_eq!(record_at(Some(record), Some(older)), Some((README, record)));
        // Neither carries one: a session on path identity, and the name that
        // renders is still the one to write to.
        assert_eq!(
            record_at(Some("# hand written\n"), Some("# also hand written\n")),
            Some((README, "# hand written\n"))
        );
        // No record at all.
        assert_eq!(record_at(None, None), None);
    }
}
