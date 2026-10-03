//! The session verbs a host runs against a zone (FR-238..FR-248, AD-111,
//! AD-112, AD-368): create, archive, delete, unarchive.
//!
//! Each finds its session **by id, on the disk, while holding the zone** — not
//! from a board's last scan — so a verb right after a create finds what the
//! create made. Holding the zone first finishes any plan a crash left in it
//! ([`exec::hold`]), so nothing can move between the look and the plan. A
//! create takes the caller's id, so a retried create finds the session the
//! first attempt made rather than making a second (FR-778). The clock is the
//! caller's too (AD-56): a verb is handed its moment.

use std::path::Path;

use keeper_core::sessions::vm::SessionRowVm;

use super::exec::{self, ExecError};
use super::scan;

/// Why a verb did nothing.
#[derive(Debug, thiserror::Error)]
pub enum VerbError {
    /// The id names no session in the zone.
    #[error("no such session: {0}")]
    NoSuchSession(String),
    /// The pattern id names no template.
    #[error("no such template: {0}")]
    NoSuchTemplate(String),
    /// The session is not in a state this verb applies to.
    #[error("{0}")]
    Refused(String),
    /// The executor refused or failed.
    #[error(transparent)]
    Exec(#[from] ExecError),
}

/// What a create is asked for.
pub struct CreateReq {
    /// The new session's id: the caller's, so a retry names the same session.
    pub id: ulid::Ulid,
    /// The title, trimmed here.
    pub title: String,
    /// `None` or `"_template"` for the zone's skeleton, `"_template/<name>"`
    /// for a named template, or a session's id to continue that session.
    pub pattern_id: Option<String>,
    /// The one clock read the whole create uses.
    pub now: chrono::DateTime<chrono::Local>,
}

/// What a create did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CreateOutcome {
    /// A new session, at this zone-relative path, under this title.
    Created {
        /// Zone-relative path.
        path: String,
        /// Title.
        title: String,
    },
    /// The id already named a session; nothing was written.
    Existed {
        /// Zone-relative path.
        path: String,
        /// Title, as the existing session has it.
        title: String,
    },
}

/// One session by id, read from the disk now: `active/*` and `archive/*/*`.
///
/// Two folders can carry one id — a sync-conflict copy of a session, say. The
/// one found is the one the board lists first: active before archived, then
/// pinned, then the freshest record, ties in walk order — the order the
/// board's snapshot is sorted in, so the row a person pressed is the one acted
/// on.
pub fn find(zone: &Path, session_id: &str) -> Option<SessionRowVm> {
    scan::session_dirs(zone)
        .into_iter()
        .filter_map(|rel| {
            let status = keeper_core::sessions::model::classify(&rel)?;
            let dir = zone.join(&rel);
            (scan::session_id(&dir, &rel) == session_id)
                .then(|| scan::row_for(&dir, &rel, status))
                .flatten()
        })
        .min_by(scan::board_order)
}

/// Create a session from a pattern (FR-238, FR-239, FR-253, AD-112, AD-116).
///
/// Two questions in — the title, and what to shape it from — and one folder
/// out. A continuation copies structure only, with `continues`/`continued-by`
/// written into BOTH READMEs, an archived source included, because files are
/// truth and a lineage only an index knew would be invisible to `cat`, to
/// Obsidian and to the agent.
pub fn create(zone: &Path, req: CreateReq) -> Result<CreateOutcome, VerbError> {
    use keeper_core::sessions::pattern::{self, PatternKind};
    use keeper_core::sessions::{model, plan, spaces, template};

    let held = exec::hold(zone)?;
    let id = req.id.to_string();
    if let Some(existing) = find(zone, &id) {
        return Ok(CreateOutcome::Existed {
            path: existing.path,
            title: existing.title,
        });
    }
    let CreateReq {
        id: _,
        title,
        pattern_id,
        now,
    } = req;
    let title = title.trim().to_owned();
    let now_local = now.to_rfc3339();
    let date = now.format("%Y-%m-%d").to_string();
    let stamp = format!("{date}-{}", now.format("%H%M"));
    let dir_name = model::session_dir_name(&title, &date, &taken_names(zone));

    // Which pattern, resolved to the one thing the plan needs: a zone-relative
    // directory to copy out of, and the kind that decides what travels. The
    // domain owns the id→source question (AD-108) — a `_template/<name>` id is
    // a template, not a session path, and an id keeper cannot join onto the
    // zone is refused here rather than reinterpreted downstream.
    let resolved = pattern::resolve(pattern_id.as_deref()).ok_or_else(|| {
        VerbError::NoSuchTemplate(pattern_id.as_deref().unwrap_or_default().to_owned())
    })?;
    let (kind, pattern_root, source) = match &resolved {
        pattern::PatternSource::Template { root } => (PatternKind::Template, root.clone(), None),
        pattern::PatternSource::Session { id: source_id } => {
            let row =
                find(zone, source_id).ok_or_else(|| VerbError::NoSuchSession(source_id.clone()))?;
            (PatternKind::Session, row.path.clone(), Some(row))
        }
    };
    let pattern_dir = zone.join(&pattern_root);
    // The zone skeleton does not carry its own named templates into a session.
    // Only the bare `_template` root can hold them, so nothing else pays for
    // the read.
    let excluded = if pattern_root == model::TEMPLATE_DIR {
        named_templates(zone)
    } else {
        Vec::new()
    };

    // Which contract the new session is born into: the pattern's own. A flat
    // template begets a flat session and a folder-shaped one begets a folder —
    // the shape is a property of the thing being copied, never a preference
    // asked of the user, because a session whose files say one thing and whose
    // shape says another is unreadable by both readers.
    let pattern_files = pattern::without_dirs(&pattern_files(&pattern_dir), &excluded);
    let pattern_top: Vec<String> = pattern_files
        .iter()
        .filter(|(rel, _)| !rel.contains('/'))
        .map(|(rel, _)| rel.clone())
        .collect();
    let shape = keeper_core::sessions::shape::shape(&pattern_top);
    let flat = shape == keeper_core::sessions::shape::Shape::Flat;

    // The stamped record. The pattern's own headings, empty, with the title and
    // date in place — a template record that grows a section grows it for every
    // new session, and a continued session inherits the shape it earned. Falling
    // back to the shipped default (FR-268) when the pattern has none to inherit
    // from.
    //
    // **Both names are read, and `record_at` says which one is the record.**
    // Story 52.1 made `README.md` the record's name under both contracts; it did
    // not move anybody's files, so a pattern — a `_template/` or a source session
    // — can still be keeping its record at `about.md` until
    // `sessions_record_migrate` has swept the zone. The two reads answer two
    // questions with one pair of bytes: the headings this create inherits, and
    // (for a create-FROM, below) the file the lineage append is guarded on and
    // written to. The choice is the domain's, because the sharp case is the
    // half-migrated one where a name picked by order picks an old signpost.
    let pattern_readme =
        std::fs::read_to_string(pattern_dir.join(keeper_core::sessions::model::README)).ok();
    let pattern_about =
        std::fs::read_to_string(pattern_dir.join(keeper_core::sessions::shape::ABOUT)).ok();
    let pattern_record = keeper_core::sessions::migrate::record_at(
        pattern_readme.as_deref(),
        pattern_about.as_deref(),
    );
    let body = match (pattern_record, flat) {
        (Some((_, text)), _) => {
            let (_, body_at) = keeper_core::notes::frontmatter::Frontmatter::parse(text);
            plan::skeleton_from(&text[body_at..], &title, &date)
        }
        // No record to inherit: the default template's own record body, reached
        // through the same renderer so the two cannot drift.
        (None, true) => template::about_only(&title, &date),
        (None, false) => plan::skeleton_from(
            "# <session title>\n\n## Summary\n\n## Log\n\n## Promote\n\n| workspace | → artifacts | note |\n| --------- | ----------- | ---- |\n",
            &title,
            &date,
        ),
    };
    // The record's own tag, so the About space finds it by what it declares
    // rather than by its filename (AD-120). Only the flat contract has kinds.
    let kind_line = if flat { "tags: [about]\n" } else { "" };
    let readme = match &source {
        // continues: baked into the new record's frontmatter at birth (AD-112).
        Some(row) => format!(
            "---\nid: {id}\ncreated: {date}\n{kind_line}keeper:\n  session-continues: [{}]\n---\n{body}",
            row.id
        ),
        None => format!("---\nid: {id}\ncreated: {date}\n{kind_line}---\n{body}"),
    };

    // What travels. In the flat contract a file's kind is a tag inside it, so
    // the decision needs the pool — read here, by the host, because the domain
    // opens nothing (AD-108). Bounded by the same walk the preview already
    // pays for: root markdown only, and `artifacts/`/`workspace/` are decided
    // by path without being read.
    let kinds = if flat {
        flat_kinds(&pattern_dir, &pattern_files)
    } else {
        std::collections::BTreeMap::new()
    };
    let outcome = pattern::apply_with_kinds(kind, &pattern_files, |rel| kinds.get(rel).copied());
    let copies = outcome.copies;

    // What keeper composes rather than copies. Folder-shaped: the record alone.
    // Flat: the record, always the navigation contract, and — only for a
    // session with nothing to inherit — the two seed files (FR-268).
    //
    // The split is the rule stated once: `AGENTS.md` is a *contract*, so a flat
    // session without one is unreadable and keeper supplies it whenever the
    // pattern did not. The seed log and seed prompt are *examples*, and a
    // continuation is not short of examples — it was made from a session that
    // has real ones. Seeding it anyway would put a "Nothing has happened yet"
    // log at the top of a session continuing months of work.
    let mut stamped = vec![(model::README.to_owned(), readme.clone())];
    if flat {
        let carried: std::collections::BTreeSet<&str> =
            copies.iter().map(|(rel, _)| rel.as_str()).collect();
        // What the pattern already supplies, by KIND — not by filename. A seed
        // is named `YYYY-MM-DD-HHMM-opened.md`, so a template holding one and
        // keeper composing another produce two different names for the same
        // thing and a filename test never fires: the session lands with two
        // "Opened" logs, one of them stamped with a minute that has nothing to
        // do with it. The kind is what may not be duplicated, so the kind is
        // what is compared.
        let carried_kinds: std::collections::BTreeSet<keeper_core::sessions::shape::KindTag> =
            copies
                .iter()
                .filter_map(|(rel, _)| kinds.get(rel).copied())
                .collect();
        // Minted here rather than handed in: they name the seed files, not
        // the session, and nothing retries by them.
        let ulids: Vec<String> = (0..3).map(|_| crate::turn::new_id()).collect();
        let seeds =
            template::default_template(&title, &date, &stamp, [&ulids[0], &ulids[1], &ulids[2]]);
        for file in seeds {
            let is_contract = file.name == keeper_core::sessions::shape::AGENTS;
            if file.name == model::README
                || carried.contains(file.name.as_str())
                || file.kind.is_some_and(|kind| carried_kinds.contains(&kind))
                || (!is_contract && source.is_some())
            {
                continue;
            }
            stamped.push((file.name, file.content));
        }
    }

    // The placeholders a template's markdown carries. This side reads the
    // bytes and supplies the context — the clock and the session's id are the
    // caller's (AD-56) — and `pattern::expansions` decides everything else
    // (AD-108), so what a `{{title}}` becomes is provable on a host where this
    // crate does not build.
    //
    // The `expands` test is applied here too, as an optimisation rather than a
    // second rule: it is what stops a template's `.png` being read into memory
    // at all. A file keeper cannot read as UTF-8 is simply not offered, and
    // copies byte for byte as it always did.
    let ctx = keeper_core::notes::templates::TemplateCtx {
        title: title.clone(),
        id: id.clone(),
        now_local,
    };
    let markdown: Vec<(String, String)> = copies
        .iter()
        .filter(|(rel, is_dir)| !*is_dir && pattern::expands(rel))
        .filter_map(|(rel, _)| {
            Some((
                rel.clone(),
                std::fs::read_to_string(pattern_dir.join(rel)).ok()?,
            ))
        })
        .collect();
    let expanded = pattern::expansions(&markdown, &ctx);

    let mut compiled = match &source {
        None => plan::compile_create_shaped(&dir_name, &pattern_root, &copies, &expanded, &stamped),
        Some(row) => {
            // The SOURCE's record: which file it is, and the bytes in it. The
            // lineage append is a `GuardedWrite`, so the two have to be the same
            // read — `pattern_dir` IS this source session's directory (the
            // `PatternSource::Session` arm above), and `record_at` picked the
            // record out of the two names it can be under. A name assumed instead
            // of read fails twice over on a zone nobody has swept yet: on an
            // unmigrated source the guard reads a `README.md` that is not there
            // and the executor refuses with an errno *after* the new session is
            // already on disk (a stray session and no lineage pair, AD-112's own
            // loss), and on a half-migrated one the lengths agree and the lineage
            // lands in an old signpost.
            let (record_name, record_text) = pattern_record.unwrap_or((model::README, ""));
            plan::compile_create_from_shaped(
                &dir_name,
                &row.path,
                record_name,
                record_text,
                &id,
                &copies,
                &expanded,
                &stamped,
            )
        }
    };
    // The spaces the template offers the ZONE (FR-291). Never the session:
    // AD-121 refused a per-session copy of a query, and `pattern::apply` keeps
    // these out of `copies` for that reason — `outcome.seeds` is non-empty only
    // for a template.
    //
    // **Only into a `_spaces/` that already exists.** An absent one is the
    // signal `sessions_spaces` reads to write the zone the defaults it was
    // designed around ("the directory is the ledger"); a create that minted the
    // directory to drop one template space into it would consume that signal,
    // and the zone would never be offered the rest. So the create fills
    // holes and `sessions_spaces` digs the well — and the one zone this can
    // decline for says so rather than seeding nothing in silence.
    let space_seeds = if outcome.seeds.is_empty() {
        Vec::new()
    } else {
        let read = scan::read_zone_spaces(zone);
        let seeded = read.seeded;
        let existing = read.spaces;
        if seeded {
            let mut sources: Vec<(String, String)> = Vec::new();
            for rel in &outcome.seeds {
                match std::fs::read_to_string(pattern_dir.join(rel)) {
                    Ok(text) => sources.push((rel.clone(), text)),
                    // One space the zone does not gain, said out loud. Never a
                    // refusal: a create must not fail over a file it was only
                    // being offered.
                    Err(error) => tracing::warn!("{rel} was not seeded: {error}"),
                }
            }
            let borrowed: Vec<(&str, &str)> = sources
                .iter()
                .map(|(rel, text)| (rel.as_str(), text.as_str()))
                .collect();
            let planned = spaces::plan_template_spaces(&pattern_root, &borrowed, &existing);
            for sentence in &planned.skipped {
                tracing::warn!("{sentence}");
            }
            planned.seeds
        } else {
            tracing::warn!(
                "this zone has no {}/ yet, so the template's spaces were not seeded — they are offered to a zone that already has its own",
                spaces::SPACES_DIR
            );
            Vec::new()
        }
    };
    // Appended after every write into the new session, because the seed lands
    // OUTSIDE it: a crash before these steps leaves the zone exactly as it was
    // and the new session still readable, through the spaces the zone already
    // had. Inside the create's own plan rather than as a second `spaces-seed`
    // verb, so one press is one journal row and a resume finishes what it began
    // (AD-111).
    compiled
        .steps
        .extend(spaces::template_seed_steps(&space_seeds));
    compiled.verb = if source.is_some() {
        "create-from".to_owned()
    } else {
        "create".to_owned()
    };
    let session_path = compiled.session.clone();
    exec::run_held(compiled, &held)?;
    Ok(CreateOutcome::Created {
        path: session_path,
        title,
    })
}

/// Archive a session (FR-245, AD-111): the compiled checklist decision —
/// promotes to run, whether to empty the workspace — executed with the move
/// last, journaled, resumable. `year` is the caller's clock.
pub fn archive(
    zone: &Path,
    session_id: &str,
    promotes: Vec<(String, String)>,
    empty_workspace: bool,
    year: i32,
) -> Result<(), VerbError> {
    use keeper_core::sessions::plan;

    let held = exec::hold(zone)?;
    let row =
        find(zone, session_id).ok_or_else(|| VerbError::NoSuchSession(session_id.to_owned()))?;
    if row.status != "active" {
        return Err(VerbError::Refused(
            "only an active session can be archived".to_owned(),
        ));
    }
    let compiled = plan::compile_archive(
        &row.path,
        &plan::ArchiveDecision {
            promotes,
            empty_workspace,
            year,
        },
    );
    Ok(exec::run_held(compiled, &held)?)
}

/// Delete a session into the zone's trash (FR-246, FR-247): recoverable,
/// never an unlink, workspace included.
pub fn delete(zone: &Path, session_id: &str) -> Result<(), VerbError> {
    let held = exec::hold(zone)?;
    let row =
        find(zone, session_id).ok_or_else(|| VerbError::NoSuchSession(session_id.to_owned()))?;
    let compiled = keeper_core::sessions::plan::compile_delete(&row.path, session_id);
    Ok(exec::run_held(compiled, &held)?)
}

/// Move an archived session back to `active/` (FR-248). Lineage is never
/// rewritten (AD-112).
pub fn unarchive(zone: &Path, session_id: &str) -> Result<(), VerbError> {
    let held = exec::hold(zone)?;
    let row =
        find(zone, session_id).ok_or_else(|| VerbError::NoSuchSession(session_id.to_owned()))?;
    if row.status != "archived" {
        return Err(VerbError::Refused(
            "only an archived session can be unarchived".to_owned(),
        ));
    }
    let compiled = keeper_core::sessions::plan::compile_unarchive(&row.path);
    Ok(exec::run_held(compiled, &held)?)
}

/// The `(dir-relative path, is_dir)` facts a pattern copy needs — one walk,
/// used for the zone's `_template/` and for a source session alike, because
/// [`keeper_core::sessions::pattern::apply`] is what tells them apart.
pub fn pattern_files(dir: &std::path::Path) -> Vec<(String, bool)> {
    fn walk(dir: &std::path::Path, prefix: &str, out: &mut Vec<(String, bool)>) {
        let Ok(entries) = std::fs::read_dir(dir) else {
            return;
        };
        for entry in entries.flatten() {
            let name = entry.file_name().to_string_lossy().into_owned();
            if name.starts_with('.') && name != ".gitkeep" {
                continue;
            }
            let rel = if prefix.is_empty() {
                name.clone()
            } else {
                format!("{prefix}/{name}")
            };
            let Ok(file_type) = entry.file_type() else {
                continue;
            };
            if file_type.is_dir() {
                out.push((rel.clone(), true));
                walk(&entry.path(), &rel, out);
            } else {
                out.push((rel, false));
            }
        }
    }
    let mut out = Vec::new();
    walk(dir, "", &mut out);
    out
}

/// What each markdown file of a **flat** pattern declares itself to be
/// (FR-268, AD-120).
///
/// The flat contract puts a file's kind in its frontmatter, so the question
/// "does this file travel into a new session" cannot be answered from the path
/// the way `prompts/**` answers it in the folder contract. The domain decides
/// what each kind means; this only reads the bytes it needs to ask (AD-108).
///
/// **The reader's own rule about where markdown lives, asked rather than
/// restated.** Every directory on the way to a file is put to
/// [`super::scan::scans_markdown`] — the one list, which is
/// [`super::scan::UNSCANNED_DIRS`] plus the dotted prefix. So a
/// `ref`-tagged file in a `spaces/` the operator made is classified and travels,
/// exactly as the pool, every space, References and the detail already list it
/// (FR-285), while `artifacts/` and `workspace/` are still decided by path and
/// never opened — which is what keeps "make a session like this one" from
/// costing a walk of a folder holding a video render.
///
/// A second spelling of that rule here is how the create side and the read side
/// come to disagree, silently: reading root markdown only left a byte-identical
/// file travelling from the session root and staying behind from `spaces/`,
/// classified `None` and therefore `SkipReason::Loose`. The suffix is folded for
/// the same reason — the walk reads a `.MD` file, so this reads one too.
///
/// An unreadable file is simply absent from the map, and an absent kind is
/// `Loose`: it stays behind, which is the safe direction.
pub fn flat_kinds(
    dir: &std::path::Path,
    files: &[(String, bool)],
) -> std::collections::BTreeMap<String, keeper_core::sessions::shape::KindTag> {
    use keeper_core::sessions::pool::{read_one, PoolFile};

    files
        .iter()
        .filter(|(rel, is_dir)| {
            !*is_dir
                && rel.to_lowercase().ends_with(".md")
                && rel
                    .split('/')
                    .rev()
                    .skip(1)
                    .all(super::scan::scans_markdown)
        })
        .filter_map(|(rel, _)| {
            let text = std::fs::read_to_string(dir.join(rel)).ok()?;
            let entry = read_one(PoolFile {
                rel,
                text: text.as_str(),
            });
            entry.kind.map(|kind| (rel.clone(), kind))
        })
        .collect()
}

/// The named templates a zone offers (FR-266): every `_template/<name>/` that
/// holds a record file, in name order.
///
/// Named rather than counted: what makes a directory under `_template/` a
/// template of its own and not a part of the skeleton is
/// [`keeper_core::sessions::pattern::is_named_template`]'s question, asked
/// against that directory's own top-level names. This reads them; the domain
/// decides (AD-108).
pub fn named_templates(zone: &std::path::Path) -> Vec<String> {
    use keeper_core::sessions::pattern;

    let template_dir = zone.join(keeper_core::sessions::model::TEMPLATE_DIR);
    let Ok(entries) = std::fs::read_dir(&template_dir) else {
        return Vec::new();
    };
    let mut out: Vec<String> = entries
        .flatten()
        .filter(|entry| {
            let name = entry.file_name().to_string_lossy().into_owned();
            let is_dir = entry.file_type().map(|kind| kind.is_dir()).unwrap_or(false);
            pattern::could_be_named_template(&name, is_dir)
        })
        .filter(|entry| {
            let top_level: Vec<String> = std::fs::read_dir(entry.path())
                .map(|inner| {
                    inner
                        .flatten()
                        .map(|child| child.file_name().to_string_lossy().into_owned())
                        .collect()
                })
                .unwrap_or_default();
            pattern::is_named_template(&top_level)
        })
        .map(|entry| entry.file_name().to_string_lossy().into_owned())
        .collect();
    // Sorted, because `read_dir` order is the filesystem's business and the
    // picker's rows must not move between two reads of an unchanged zone.
    out.sort();
    out
}

/// The folder names already taken in `active/`, for the collision counter.
pub fn taken_names(zone: &std::path::Path) -> Vec<String> {
    std::fs::read_dir(zone.join("active"))
        .map(|entries| {
            entries
                .flatten()
                .map(|entry| entry.file_name().to_string_lossy().into_owned())
                .collect()
        })
        .unwrap_or_default()
}
