//! Promotion, as the session runtime does it (FR-243, FR-244, FR-808,
//! AD-391, AD-404): the panel's facts, a promotion into the session's
//! `artifacts/`, a promotion of an artifact out into the drive's notes
//! vault, and a person's review of a harvested note's vault copy.
//!
//! The README's `## Promote` table is the contract every one of them keeps:
//! a promotion is its row plus a copy ([`promote::upsert_row`]), and the
//! panel renders the table ([`promote::promote_panel`]). The row is
//! written first, in the same held zone, so a promotion that fails after it
//! leaves a row whose state says so and which promoting again completes; a
//! promotion out also keeps the version it admitted until both have landed
//! ([`Pending`]). Nothing here composes a path: session files are where
//! [`landing`] says, drive files where `keeper-sync`'s `browse::resolve_known`,
//! `browse::landing` and [`WriteScope`] say (AD-65).
//!
//! Who a promotion out reaches is decided here, never by the shell
//! ([`audience`]): the session's label from its whole log, the drive's
//! audience from its `_drive.toml` checked against this device's pin; any
//! of them that cannot be established refuses the promotion.

use std::io::Read as _;
use std::path::{Path, PathBuf};

use keeper_core::agents::agentd::DrivePin;
use keeper_core::agents::drive;
use keeper_core::agents::knowledge;
use keeper_core::agents::label::{
    check_sink, Integrity, Label, LabelVm, Readers, Sink, SinkVerdict,
};
use keeper_core::agents::log::reader::read_session;
use keeper_core::agents::log::{LineBody, LOG_DIR};
use keeper_core::agents::mount::pin_matches;
use keeper_core::agents::session::{self as agent_session, parse_session_agent_toml};
use keeper_core::sessions::model::{ARTIFACTS_DIR, README, WORKSPACE_DIR};
use keeper_core::sessions::plan::{sha256_hex, Plan, PlanStep};
use keeper_core::sessions::promote::{self, KnowledgeFile, PanelFacts, SessionPromoteVm};
use keeper_sync::browse;
use keeper_sync::files_write::{collides, WriteRefusal, WriteRoute, WriteScope};
use keeper_sync::git::history;
use keeper_sync::stability::{read_verified, verify_while_reading, FileSample};
use keeper_sync::SyncProfile;

use crate::ports::VaultWriter;
use crate::sessions::exec::{self, ExecError};
use crate::sessions::lock::ZoneLock;
use crate::sessions::scan;
use crate::sessions::verbs::VerbError;
use crate::sessions::write::landing;

/// What a promotion of a file written within the stability window says.
pub const STILL_WRITING: &str = "still being written; try again in a moment";

/// What a review of a note not promoted into the vault says (R139).
pub const NOT_PROMOTED: &str =
    "a person's review is written into the vault's copy of a note; promote it to notes first.";

/// What a promotion of a harvested note that names no version says: the
/// promotion is the person's review of what they read (R95K-06).
pub const UNREVIEWED: &str = "promoting a harvested note into the notes vault is a person's review of the version they read, and this one names no version; open it and promote it from there.";

/// What a promotion of a harvested note with no person to record says.
pub const NO_REVIEWER: &str = "promoting a harvested note records the person who reviewed it, and no person was found to record; nothing was promoted.";

/// The most files the panel lists under `workspace/` or reads under
/// `artifacts/knowledge/`.
const WALK_CAP: usize = 4096;

/// How many of a synced file's commits the panel reads back to find when
/// what it says last changed.
const HISTORY_DEPTH: usize = 16;

/// The most of a harvested note's vault copy the panel reads to tell
/// whether it is still the copy the note published: sixteen times what a
/// knowledge note holds, far past any note with its reviews.
const COPY_BYTES: u64 = 16 * knowledge::MAX_NOTE_BYTES as u64;

/// How many times a row is spliced into a README someone else is editing.
const ROW_ATTEMPTS: usize = 3;

/// Where a promotion out lands: the drive, its vault and who reads it.
pub struct OutOf<'a> {
    /// The drive the session is in; a promotion never leaves it.
    pub profile: &'a SyncProfile,
    pub vault: &'a dyn VaultWriter,
    /// This device's pin of the drive's audience (R28 S-15) — `None` when
    /// it holds none — or why its pins could not be read: what the drive's
    /// `_drive.toml` is checked against.
    pub pin: Result<Option<&'a DrivePin>, String>,
}

/// Who a session's work may reach and who reads its drive (AD-391), as
/// far as it can be established.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Audience {
    /// An agent's session's label after every `label` line of its whole
    /// log; a person's session's, the drive's at opening.
    pub label: Label,
    /// Whether the session is an agent's: the panel shows its label chip.
    pub agent: bool,
    /// Who reads the drive: its pinned readers, or anyone for a drive that
    /// declares no audience and pins none.
    pub drive_readers: Readers,
    /// The session's log as the label was read from it: each chunk's name
    /// and size.
    frontier: Vec<(String, u64)>,
}

impl Audience {
    /// Why a promotion out of the session into its drive is refused —
    /// its label keeps it from some of the drive's readers — or `None`.
    pub fn refusal(&self) -> Option<String> {
        match check_sink(
            &self.label,
            &Sink::DriveWrite {
                drive_readers: self.drive_readers.clone(),
            },
        ) {
            SinkVerdict::Allow => None,
            SinkVerdict::Block { reason, .. } => Some(reason),
        }
    }
}

/// The audience of the session `session` (zone-relative) in the zone
/// `zone` of `out`'s drive, or why it cannot be established — which
/// refuses every promotion out of it (R95K-01…03):
///
/// - whose session it is: no `agent.toml` is a person's session; one that
///   cannot be read, is not a file or does not parse is not "no agent";
/// - who reads the drive: `_drive.toml` as this device pinned it — a
///   declaration that cannot be read, is not pinned here or differs from
///   the pin ([`pin_matches`]) is refused; a drive with neither a
///   declaration nor a pin is anyone's, unless the session is an agent's;
/// - an agent's label: its whole log, read to its frontier now — a chunk
///   that cannot be read, a skipped or torn line, or two hosts at one
///   epoch, and the label is not established.
pub fn audience(zone: &Path, session: &str, out: &OutOf) -> Result<Audience, String> {
    let refused = |why: String| format!("{why}, so nothing is promoted out of {session}.");
    let dir = browse::lexical_join(zone, session).map_err(|error| refused(error.to_string()))?;
    let agent_file = dir.join(agent_session::FILE_NAME);
    let agent = match std::fs::symlink_metadata(&agent_file) {
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
        Err(error) => {
            return Err(refused(format!(
                "whose session this is cannot be established: {} could not be read ({error})",
                agent_session::FILE_NAME
            )))
        }
        Ok(meta) if !meta.is_file() => {
            return Err(refused(format!(
                "whose session this is cannot be established: its {} is not a file",
                agent_session::FILE_NAME
            )))
        }
        Ok(_) => Some(
            std::fs::read_to_string(&agent_file)
                .map_err(|error| error.to_string())
                .and_then(|text| {
                    parse_session_agent_toml(&text).map_err(|refusal| refusal.to_string())
                })
                .map_err(|why| {
                    refused(format!(
                        "whose session this is cannot be established: {why}"
                    ))
                })?,
        ),
    };
    let declared = match out.profile.agents_root() {
        None => None,
        Some(root) => crate::zone::read_text(&root, drive::FILE_NAME)
            .and_then(|text| {
                text.map(|text| drive::parse(&text).map_err(|refusal| refusal.sentence()))
                    .transpose()
            })
            .map_err(|why| refused(format!("who reads this drive cannot be established: {why}")))?,
    };
    let Some(mut decl) = declared else {
        if agent.is_some() || !matches!(out.pin, Ok(None)) {
            return Err(refused(
                "who reads this drive cannot be established: its _drive.toml is not there"
                    .to_owned(),
            ));
        }
        return Ok(Audience {
            label: Label {
                readers: Readers::Anyone,
                integrity: Integrity::Owner,
                local_only: false,
            },
            agent: false,
            drive_readers: Readers::Anyone,
            frontier: Vec::new(),
        });
    };
    let pin = match &out.pin {
        Ok(Some(pin)) => *pin,
        Ok(None) => {
            return Err(refused(format!(
                "who reads {} cannot be established: its readers are not pinned on this device",
                decl.id
            )))
        }
        Err(why) => {
            return Err(refused(format!(
                "who reads {} cannot be established: this device's pins could not be read ({why})",
                decl.id
            )))
        }
    };
    pin_matches(&decl, pin).map_err(|difference| refused(difference.sentence()))?;
    decl.local_only |= pin.local_only;
    let drive_readers = pin.readers();
    let Some(agent) = agent else {
        return Ok(Audience {
            label: Label::opening(&decl, Integrity::Owner),
            agent: false,
            drive_readers,
            frontier: Vec::new(),
        });
    };
    let log = read_session(&dir);
    if let Some(problem) = log.problems.first() {
        return Err(refused(format!(
            "its label is not established: its log could not be read whole ({}: {})",
            problem.chunk, problem.sentence
        )));
    }
    if log.conflicted() {
        return Err(refused(
            "its label is not established: two hosts wrote its log at one epoch".to_owned(),
        ));
    }
    let label = log
        .lines
        .iter()
        .fold(agent.label, |label, line| match &line.body {
            LineBody::Label(body) => body.label(),
            _ => label,
        });
    let frontier = log
        .chunks
        .iter()
        .map(|chunk| (chunk.name.to_string(), chunk.bytes))
        .collect();
    Ok(Audience {
        label,
        agent: true,
        drive_readers,
        frontier,
    })
}

/// The session's log chunks as they are now, each name and size, as
/// [`Audience`] holds them — or why they cannot all be seen: a `log/` that
/// does not list, an entry or a size that does not read, an entry under a
/// chunk's name that is not a file. No `log/` at all is no chunks.
fn frontier(zone: &Path, session: &str) -> Result<Vec<(String, u64)>, String> {
    let dir = browse::lexical_join(zone, session).map_err(|error| error.to_string())?;
    let entries = match std::fs::read_dir(dir.join(LOG_DIR)) {
        Ok(entries) => entries,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(error) => return Err(format!("{LOG_DIR}/ could not be listed: {error}")),
    };
    let mut out = Vec::new();
    for entry in entries {
        let entry =
            entry.map_err(|error| format!("an entry of {LOG_DIR}/ could not be read: {error}"))?;
        let Some(name) = entry
            .file_name()
            .into_string()
            .ok()
            .filter(|name| name.parse::<keeper_core::agents::log::ChunkName>().is_ok())
        else {
            continue;
        };
        let meta = entry
            .metadata()
            .map_err(|error| format!("{LOG_DIR}/{name} could not be read: {error}"))?;
        if !meta.is_file() {
            return Err(format!("{LOG_DIR}/{name} is not a file"));
        }
        out.push((name, meta.len()));
    }
    out.sort();
    Ok(out)
}

/// The promote panel of the session at `session` (zone-relative) in the
/// zone `zone` of `out`'s drive: every row of its table with its state, the
/// workspace files no row names, its harvested notes with whether `me`
/// (`human:<localpart>`) reviewed each one's copy, the label chip of an
/// agent's session, the drive's vault and why a promotion out is refused —
/// its label, or an audience that cannot be established ([`audience`]).
///
/// Only the files a row names are read, streamed, and only harvested notes
/// are read whole, up to a knowledge note's 64 KiB — and the copy a note's
/// row records it published, up to [`COPY_BYTES`], to tell whether it is
/// still that copy; a workspace file no row names is listed by name. What
/// could not be read or listed is said: a row's file as `unknown`, a note
/// with its problem, a folder or the cap in `problems`.
pub fn panel(
    zone: &Path,
    session: &str,
    out: &OutOf,
    me: Option<&str>,
) -> Result<SessionPromoteVm, VerbError> {
    let readme = read_readme(zone, session)?;
    let dir = browse::lexical_join(zone, session)
        .map_err(|_| VerbError::NoSuchSession(session.to_owned()))?;
    let mut facts = PanelFacts::default();
    facts.workspace = walk(&dir, WORKSPACE_DIR, &mut facts.problems);
    let notes = walk(&dir, knowledge::KNOWLEDGE_DIR, &mut facts.problems);
    facts.knowledge = notes
        .into_iter()
        .filter(|rel| rel.ends_with(".md"))
        .map(|rel| knowledge_file(zone, session, rel))
        .collect();
    let root = out.profile.local_path.as_path();
    let zone_in_drive = zone
        .strip_prefix(root)
        .ok()
        .and_then(Path::to_str)
        .map(|prefix| prefix.replace('\\', "/"));
    if let Some(table) = promote::parse(&readme) {
        for row in &table.rows {
            let promote::PromoteRow::Entry { source, target, .. } = row else {
                continue;
            };
            for (cell, in_session) in [
                (source.clone(), true),
                (target.clone(), promote::target_in_session(target)),
            ] {
                if facts.files.contains_key(&cell) {
                    continue;
                }
                let (found, in_drive) = if in_session {
                    (
                        locate(zone, &format!("{session}/{cell}")),
                        zone_in_drive
                            .as_ref()
                            .map(|prefix| format!("{prefix}/{session}/{cell}")),
                    )
                } else {
                    (locate(root, &cell), Some(cell.clone()))
                };
                let fact = match found {
                    Ok(None) => continue,
                    Ok(Some(path)) => {
                        file_fact(&cell, &path, root, in_drive.as_deref(), !in_session)
                    }
                    Err(why) => Err(why),
                };
                facts.files.insert(cell, fact);
            }
        }
        for note in &facts.knowledge {
            let Some((_, target, Some(_))) = promote::entry_of(&table, &note.path) else {
                continue;
            };
            if promote::target_in_session(target) {
                continue;
            }
            if let Ok(Some(path)) = locate(root, target) {
                facts
                    .copies
                    .insert(note.path.clone(), copy_fact(&note.path, target, &path));
            }
        }
    }
    let mut vm = promote::promote_panel(&readme, &facts, me);
    vm.vault = out.vault.subfolder(&out.profile.id);
    match audience(zone, session, out) {
        Ok(audience) => {
            vm.out_refused = audience.refusal();
            vm.label = audience
                .agent
                .then(|| LabelVm::compose(&audience.label, &|user| user.to_string()));
        }
        Err(why) => vm.out_refused = Some(why),
    }
    Ok(vm)
}

/// The file `rel` under `root`, resolved inside it: `None` only when the
/// disk says it is not there, the sentence when it leads out or cannot be
/// looked at — a folder on the way that may not be searched is not an
/// absence ([`browse::resolve_known`]).
fn locate(root: &Path, rel: &str) -> Result<Option<PathBuf>, String> {
    browse::resolve_known(root, rel)
        .map(|known| known.landed().map(browse::Landing::into_path))
        .map_err(|refusal| format!("{rel}: {refusal}"))
}

/// The panel's fact of the file `cell` at `path`: what it says, streamed,
/// and when that last changed — for a synced file (`in_drive`, its path in
/// the drive at `root`, outside `workspace/`) by its commits ([`History`]),
/// else by its mtime. A file a person's review is written into
/// (`reviewed_here`: a vault copy) is dated by its commits only: a tick
/// moves its mtime without changing what it says, so where its history
/// does not tell, when it changed is not known.
fn file_fact(
    cell: &str,
    path: &Path,
    root: &Path,
    in_drive: Option<&str>,
    reviewed_here: bool,
) -> Result<promote::FileFact, String> {
    let unread = |error: &dyn std::fmt::Display| format!("{cell} could not be read: {error}");
    let meta = std::fs::metadata(path).map_err(|error| unread(&error))?;
    if !meta.is_file() {
        return Err(format!("{cell} is not a file"));
    }
    let mtime_ms = FileSample::of(path)
        .map_err(|error| unread(&error))?
        .map_or(0, |sample| sample.mtime_ms());
    let file = std::fs::File::open(path).map_err(|error| unread(&error))?;
    let digest = promote::digest_of(cell, file).map_err(|error| unread(&error))?;
    let synced = in_drive.filter(|_| !cell.starts_with(&format!("{WORKSPACE_DIR}/")));
    let changed_ms = match synced.map(|in_drive| history_of(root, in_drive, cell, &digest)) {
        Some(History::Changed(ms)) => Some(ms),
        Some(History::Unknown) => None,
        Some(History::Uncommitted) | None if reviewed_here => None,
        Some(History::Uncommitted) | None => Some(mtime_ms),
    };
    Ok(promote::FileFact { changed_ms, digest })
}

/// The [`promote::CopyFact`] of the file at `path`, `target` of the row of
/// the harvested note `source`, read whole — or why it was not: it could
/// not be read, or it holds more than [`COPY_BYTES`].
fn copy_fact(source: &str, target: &str, path: &Path) -> Result<promote::CopyFact, String> {
    let mut bytes = Vec::new();
    std::fs::File::open(path)
        .and_then(|file| file.take(COPY_BYTES + 1).read_to_end(&mut bytes))
        .map_err(|error| format!("{target} could not be read: {error}"))?;
    if bytes.len() as u64 > COPY_BYTES {
        return Err(format!(
            "{target} holds more than the {COPY_BYTES} bytes the panel reads of a note's copy, so whether it is still the copy {source} published is not shown, nor any review in it."
        ));
    }
    Ok(promote::copy_fact(source, &bytes))
}

/// What a synced file's history says of when what it says now changed.
enum History {
    /// The commit time, ms, of the oldest of its latest commits that all
    /// say it — review keys aside, so a tick committed later does not
    /// count — found with the commit before it saying something else, or
    /// with no commit before it.
    Changed(i64),
    /// Its newest commit does not say it (a change not committed yet), or
    /// it has no history here.
    Uncommitted,
    /// Every commit read says it and there may be older ones, or a commit
    /// could not be read: where the change lies is not established.
    Unknown,
}

/// [`History`] of the synced file `in_drive` of the repository at `root`,
/// which says `digest` now, from at most [`HISTORY_DEPTH`] of its commits.
fn history_of(root: &Path, in_drive: &str, cell: &str, digest: &str) -> History {
    let Ok(revisions) = history::file_log(root, in_drive, HISTORY_DEPTH) else {
        return History::Uncommitted;
    };
    let mut changed = None;
    let mut bounded = false;
    for revision in &revisions {
        let blob = match history::blob_at(root, &revision.id, in_drive) {
            Ok(Some(blob)) => blob,
            // Not there at that commit: the change is the one after it.
            Ok(None) => {
                bounded = true;
                break;
            }
            Err(_) => return History::Unknown,
        };
        match promote::digest_of(cell, blob.as_slice()) {
            Ok(held) if held == digest => {
                changed = Some(revision.committed_secs.saturating_mul(1000));
            }
            Ok(_) => {
                bounded = true;
                break;
            }
            Err(_) => return History::Unknown,
        }
    }
    match changed {
        None => History::Uncommitted,
        // Every commit read says it, and the one that changed it may lie
        // past what was read.
        Some(_) if !bounded && revisions.len() >= HISTORY_DEPTH => History::Unknown,
        Some(ms) => History::Changed(ms),
    }
}

/// The harvested note `rel` of the session: its size, and its whole text
/// or why the panel does not hold it.
fn knowledge_file(zone: &Path, session: &str, rel: String) -> KnowledgeFile {
    let unread = |error: &dyn std::fmt::Display| format!("{rel} could not be read: {error}");
    let read = || -> Result<(u64, Result<String, String>), String> {
        let path = locate(zone, &format!("{session}/{rel}"))?
            .ok_or_else(|| format!("{rel} is not there any more"))?;
        let bytes = std::fs::metadata(&path)
            .map_err(|error| unread(&error))?
            .len();
        if bytes > knowledge::MAX_NOTE_BYTES as u64 {
            return Ok((
                bytes,
                Err(format!(
                    "{rel} holds {bytes} bytes, more than the {} a knowledge note holds, so it is not shown",
                    knowledge::MAX_NOTE_BYTES
                )),
            ));
        }
        let mut text = String::new();
        let read = std::fs::File::open(&path).and_then(|file| {
            file.take(knowledge::MAX_NOTE_BYTES as u64)
                .read_to_string(&mut text)
        });
        Ok((bytes, read.map(|_| text).map_err(|error| unread(&error))))
    };
    let (bytes, text) = match read() {
        Ok(found) => found,
        Err(why) => (0, Err(why)),
    };
    KnowledgeFile {
        path: rel,
        bytes,
        text,
    }
}

/// Every regular file under the session-relative `top` of the session
/// folder `dir`, session-relative, in name order; links and dotted names
/// are not followed, and the walk stops at [`WALK_CAP`]. A folder that
/// would not list — `top` itself absent aside — and the cap are told in
/// `problems`.
fn walk(dir: &Path, top: &str, problems: &mut Vec<String>) -> Vec<String> {
    let mut out = Vec::new();
    let mut todo = vec![top.to_owned()];
    while let Some(rel) = todo.pop() {
        let Ok(path) = browse::lexical_join(dir, &rel) else {
            continue;
        };
        let entries = match std::fs::read_dir(path) {
            Ok(entries) => entries,
            Err(error) if rel == top && error.kind() == std::io::ErrorKind::NotFound => continue,
            Err(error) => {
                problems.push(format!("{rel}/ could not be listed: {error}"));
                continue;
            }
        };
        let mut names: Vec<(String, std::fs::FileType)> = Vec::new();
        for entry in entries {
            let named = entry.map_err(|error| error.to_string()).and_then(|entry| {
                let kind = entry.file_type().map_err(|error| error.to_string())?;
                let name = entry
                    .file_name()
                    .into_string()
                    .map_err(|_| "a name that is not UTF-8".to_owned())?;
                Ok((name, kind))
            });
            match named {
                Ok((name, kind)) if !name.starts_with('.') || name == ".gitkeep" => {
                    names.push((name, kind));
                }
                Ok(_) => {}
                Err(why) => problems.push(format!("an entry of {rel}/ could not be read: {why}")),
            }
        }
        names.sort_by(|a, b| b.0.cmp(&a.0));
        for (name, kind) in names {
            if out.len() >= WALK_CAP {
                problems.push(format!(
                    "{top}/ holds more than {WALK_CAP} files; the panel lists {WALK_CAP} of them."
                ));
                out.sort();
                return out;
            }
            let child = format!("{rel}/{name}");
            if kind.is_dir() {
                todo.push(child);
            } else if kind.is_file() {
                out.push(child);
            }
        }
    }
    out.sort();
    out
}

/// The file `rel` of the session, when it is there inside the zone.
fn session_file(zone: &Path, session: &str, rel: &str) -> Option<PathBuf> {
    browse::resolve(zone, &format!("{session}/{rel}"))
        .ok()
        .flatten()
}

fn read_readme(zone: &Path, session: &str) -> Result<String, VerbError> {
    let path = session_file(zone, session, README)
        .ok_or_else(|| VerbError::NoSuchSession(session.to_owned()))?;
    std::fs::read_to_string(path).map_err(|error| {
        VerbError::Refused(format!("{session}/{README} could not be read: {error}"))
    })
}

/// The README's bytes with `source → target` recorded as one row — with
/// `published` as the copy it now records ([`promote::upsert_published_row`]),
/// or keeping what it records ([`promote::upsert_row`]) — or the refusal: no
/// table to record it in, or a cell the table cannot hold.
fn recorded(
    readme: &str,
    source: &str,
    target: &str,
    note: &str,
    published: Option<&str>,
) -> Result<String, VerbError> {
    let spliced = match published {
        Some(digest) => promote::upsert_published_row(readme, source, target, note, digest),
        None => promote::upsert_row(readme, source, target, note),
    };
    spliced.map_err(|refusal| VerbError::Refused(refusal.to_string()))
}

/// Record `source → target` in the session's README, then run `then`, as
/// one journaled plan in the held zone — the row first, so a crash or a
/// refusal after it leaves a row whose state says what is missing and
/// which promoting again completes. A README someone edited between its
/// read and its write is read again and the row spliced into that, up to
/// [`ROW_ATTEMPTS`] times: their edit is kept, never written over.
fn record_then(
    held: &ZoneLock,
    session: &str,
    verb: &str,
    row: (&str, &str, &str, Option<&str>),
    then: &[PlanStep],
) -> Result<(), VerbError> {
    let (source, target, note, published) = row;
    let mut attempt = 0;
    loop {
        attempt += 1;
        let readme = read_readme(held.zone(), session)?;
        let updated = recorded(&readme, source, target, note, published)?;
        let mut steps = Vec::with_capacity(then.len() + 1);
        steps.push(PlanStep::guarded(
            format!("{session}/{README}"),
            &readme,
            updated,
        ));
        steps.extend_from_slice(then);
        let ran = exec::run_held(
            Plan {
                verb: verb.to_owned(),
                session: session.to_owned(),
                steps,
            },
            held,
        );
        match ran {
            Err(ExecError::Refused(_))
                if attempt < ROW_ATTEMPTS
                    && read_readme(held.zone(), session).is_ok_and(|now| now != readme) => {}
            ran => return Ok(ran?),
        }
    }
}

/// Promote the session's `source` (`workspace/…`) to `target`
/// (`artifacts/…`), both session-relative (FR-243): the source must have
/// stayed as it is for `settle_ms` before `now_ms` and read whole as one
/// version — the sync engine's stability gate, sampled — or the promotion
/// is refused with [`STILL_WRITING`]. Then one plan records its one row in
/// the README, every other byte of which is kept (NFR-39), and copies
/// exactly the bytes that read verified, over a target already there; a
/// source that changed since is refused with the target as it was.
pub fn promote_in(
    zone: &Path,
    session: &str,
    source: &str,
    target: &str,
    note: &str,
    settle_ms: u64,
    now_ms: i64,
) -> Result<(), VerbError> {
    let held = exec::hold(zone)?;
    let source = landing(held.zone(), session, source)?;
    let target = landing(held.zone(), session, target)?;
    let under = |rel: &str, top: &str| {
        rel.split_once('/')
            .is_some_and(|(first, rest)| first == top && !rest.is_empty())
    };
    if !under(&source, WORKSPACE_DIR) || !under(&target, ARTIFACTS_DIR) {
        return Err(VerbError::Refused(format!(
            "a promotion copies a file of workspace/ into artifacts/; {source} → {target} is not one."
        )));
    }
    recorded(
        &read_readme(held.zone(), session)?,
        &source,
        &target,
        note,
        None,
    )?;
    let gone = || VerbError::Refused(format!("{source} is not in this session any more."));
    let still = || VerbError::Refused(format!("{source} is {STILL_WRITING}."));
    let path = session_file(held.zone(), session, &source)
        .filter(|path| path.is_file())
        .ok_or_else(gone)?;
    let sample = FileSample::of(&path).ok().flatten().ok_or_else(gone)?;
    if now_ms.saturating_sub(sample.mtime_ms()) < i64::try_from(settle_ms).unwrap_or(i64::MAX) {
        return Err(still());
    }
    let (sha256, _) = verify_while_reading(&path).map_err(|_| still())?;
    if FileSample::of(&path).ok().flatten() != Some(sample) {
        return Err(still());
    }
    let mut then = Vec::with_capacity(2);
    if let Some((parent, _)) = target.rsplit_once('/') {
        then.push(PlanStep::MkDir {
            path: format!("{session}/{parent}"),
        });
    }
    then.push(PlanStep::CopyChecked {
        from: format!("{session}/{source}"),
        to: format!("{session}/{target}"),
        sha256,
    });
    record_then(
        &held,
        session,
        "promote",
        (&source, &target, note, None),
        &then,
    )
}

/// What a promotion out names.
#[derive(Debug, Clone, Copy)]
pub struct Request<'a> {
    /// Session-relative, under `artifacts/`.
    pub source: &'a str,
    /// Drive-relative, inside the vault.
    pub target: &'a str,
    pub note: &'a str,
    /// The lowercase hex SHA-256 of the source as the person read it;
    /// required for a harvested note, whose promotion is their review.
    pub expected: Option<&'a str>,
}

/// The person a promotion of a harvested note records as its reviewer.
#[derive(Debug, Clone, Copy)]
pub struct Reviewer<'a> {
    /// A Matrix localpart: the review is `human:<person>`'s.
    pub person: &'a str,
    /// When, RFC 3339.
    pub at: &'a str,
}

/// The scope a promotion out writes through: the drive's live vault, its
/// sessions zone and its agents' homes fenced as every drive write is.
fn scope<'p>(profile: &'p SyncProfile, subfolder: Option<&str>) -> WriteScope<'p> {
    WriteScope::new(&profile.name, subfolder)
        .with_sessions(profile.sessions.as_ref().map(|s| s.subfolder.as_str()))
        .with_agents(profile.agents.as_ref().map(|a| a.subfolder.as_str()))
}

/// Refuse `target` (drive-relative) unless it lands on the disk where its
/// path says: a folder link on the way — out of the drive, into an agent's
/// home, into the sessions zone — would carry the write past the fences
/// [`WriteScope`] checked on the path (R95K-04).
fn lands_as_named(root: &Path, target: &str) -> Result<(), VerbError> {
    let landed = browse::landing(root, target)
        .map_err(|refusal| VerbError::Refused(format!("{target}: {refusal}")))?;
    if landed.iter().map(String::as_str).ne(target.split('/')) {
        return Err(VerbError::Refused(format!(
            "{target} leads through a link to {}; keeper writes a note only where its path says, so nothing was written.",
            landed.join("/")
        )));
    }
    Ok(())
}

/// A promotion out between its admission and its end, as the zone keeps it
/// at [`PENDING_REL`]: the exact bytes reviewed and admitted, where they
/// go, and the row that records them — written before the row or the copy
/// lands and cleared only once both have, durably, so whoever holds the
/// zone next publishes that version or says why it no longer can
/// ([`finish_pending`]), never the session's file as it is by then. It
/// grants nothing: whether a file at the target may be replaced is asked
/// of the session's row and the file itself when it is published
/// ([`promote::standing`]), never of this device's record (R244).
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
struct Pending {
    /// Zone-relative.
    session: String,
    /// The id the session's record named at admission ([`scan::session_id`]):
    /// what is at `session` when it is finished must still be that session.
    session_id: String,
    /// Session-relative.
    source: String,
    /// Drive-relative.
    target: String,
    note: String,
    /// The drive the copy is published into.
    profile: String,
    /// The target inside the vault, as [`WriteScope::create`] admitted it.
    vault_relative: String,
    /// What is published, byte for byte.
    text: String,
}

/// Where a promotion out in progress is kept, zone-relative; inside
/// `.keeper/`, so it never syncs.
const PENDING_REL: &str = ".keeper/promote-out.json";

fn keep_pending(zone: &Path, pending: &Pending) -> Result<(), VerbError> {
    let text = serde_json::to_string(pending).map_err(|error| {
        VerbError::Refused(format!("the promotion could not be recorded: {error}"))
    })?;
    exec::write_durable(&zone.join(PENDING_REL), &text).map_err(|error| {
        VerbError::Refused(format!(
            "the promotion could not be recorded ({error}); nothing was promoted."
        ))
    })
}

fn clear_pending(zone: &Path) -> Result<(), VerbError> {
    exec::remove_durable(&zone.join(PENDING_REL)).map_err(|error| {
        VerbError::Refused(format!(
            "the promotion is done, but its record could not be cleared: {error}"
        ))
    })
}

/// A promotion record `text` that does not read as one, kept as evidence:
/// written durably beside itself under a name of its own, so no later one
/// replaces it, and only then removed; the refusal says where. Until that
/// copy is on the disk the record stays where it is, and every promotion out
/// of the zone and every review is refused until it can be set aside — none
/// passes a record whose only evidence a power cut could still take (R244).
/// Once the copy is durable, a removal whose sync did not finish is done
/// enough: a record a power cut brings back is refused and set aside again.
fn set_aside(zone: &Path, text: &str, why: &str) -> VerbError {
    let kept = format!(".keeper/promote-out.{}.unreadable.json", ulid::Ulid::new());
    let (active, kept_at) = (zone.join(PENDING_REL), zone.join(&kept));
    let set =
        exec::write_durable(&kept_at, text).and_then(|()| match exec::remove_durable(&active) {
            Err(_)
                if active
                    .symlink_metadata()
                    .is_err_and(|error| error.kind() == std::io::ErrorKind::NotFound) =>
            {
                Ok(())
            }
            removed => removed,
        });
    if set.is_err() {
        // The record is still where it was; a copy that is not known to be
        // on the disk is not left to stand in for it.
        let _ = std::fs::remove_file(&kept_at);
    }
    VerbError::Refused(match set {
        Ok(()) => format!(
            "an earlier promotion out of this zone was interrupted, and its record does not read ({why}): it is not finished, and its record is kept at {kept}. Nothing more was promoted."
        ),
        Err(error) => format!(
            "an earlier promotion out of this zone was interrupted, and its record does not read ({why}) and could not be set aside ({error}), so nothing more is promoted out of this zone until it can be."
        ),
    })
}

/// The digest the session's row for `source` — its one row
/// ([`promote::entry_of`]) — records for the copy it published at `target`:
/// `None` when that row names another target, or records no publication,
/// or there is no row.
fn recorded_copy(readme: &str, source: &str, target: &str) -> Option<String> {
    let table = promote::parse(readme)?;
    let (_, named, published) = promote::entry_of(&table, source)?;
    published.filter(|_| named == target).map(str::to_owned)
}

/// Finish the promotion out a crash or a failed write left in the held
/// zone, if there is one: its row recorded and its reviewed bytes
/// published as they were admitted. Refused — the record cleared, nothing
/// written, the row saying what is missing — when that version may no
/// longer go there: its session is no longer where it was (archived,
/// deleted or moved), or another session is there now, the drive's vault
/// is no longer where it was admitted into, the session's audience is no
/// longer established or no longer fits the drive's readers, the target
/// leads through a link, or what is at the target is not known to be
/// nothing, this operation's own bytes or the copy its row records
/// ([`promote::standing`]). A record that does
/// not read is refused and set aside ([`set_aside`]); a write that fails, a
/// session whose record cannot be read, or a vault that is not open, keeps
/// the record for the next try.
fn finish_pending(held: &ZoneLock, out: &OutOf) -> Result<(), VerbError> {
    let path = held.zone().join(PENDING_REL);
    let text = match std::fs::read_to_string(&path) {
        Ok(text) => text,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(error) => {
            return Err(VerbError::Refused(format!(
                "an earlier promotion's record could not be read ({error}), so nothing more is promoted until it can."
            )))
        }
    };
    let pending = match serde_json::from_str::<Pending>(&text) {
        Ok(pending) => pending,
        Err(error) => return Err(set_aside(held.zone(), &text, &error.to_string())),
    };
    let refuse = |why: String| {
        clear_pending(held.zone())?;
        Err(VerbError::Refused(format!(
            "an earlier promotion of {} to {} was interrupted, and the version reviewed then is not published now: {why} Nothing more was promoted; its row says what is missing.",
            pending.source, pending.target
        )))
    };
    let unknown = |what: String| {
        Err(VerbError::Refused(format!(
            "an earlier promotion of {} to {} is not finished, and {what}, so nothing more is promoted out of this zone until it can.",
            pending.source, pending.target
        )))
    };
    if pending.profile != out.profile.id {
        return refuse("it was for another drive.".to_owned());
    }
    match browse::resolve_known(held.zone(), &format!("{}/{README}", pending.session))
        .map(browse::Known::landed)
    {
        Ok(Some(_)) => {}
        Ok(None) => {
            return refuse(format!(
                "{} is not in this zone's sessions any more — archived, deleted or moved.",
                pending.session
            ))
        }
        Err(refusal) => {
            return unknown(format!(
                "{} cannot be looked at ({refusal})",
                pending.session
            ))
        }
    }
    let dir = browse::lexical_join(held.zone(), &pending.session)
        .map_err(|error| VerbError::Refused(error.to_string()))?;
    match scan::recorded_id(&dir) {
        Ok(Some(id)) if id == pending.session_id => {}
        Ok(found) => {
            return refuse(format!(
                "{} is not the session it was admitted from any more ({}), so nothing is written into it.",
                pending.session,
                found.map_or_else(
                    || "its record names no id, so which session it is cannot be established".to_owned(),
                    |id| format!("it is {id}, not {}", pending.session_id)
                )
            ))
        }
        Err(error) => {
            return unknown(format!(
                "which session {} is cannot be read ({error})",
                pending.session
            ))
        }
    }
    let Some(subfolder) = out.vault.subfolder(&out.profile.id) else {
        return unknown("this drive's notes vault is not open".to_owned());
    };
    let (dir, name) = pending
        .target
        .rsplit_once('/')
        .unwrap_or(("", pending.target.as_str()));
    let scope = scope(out.profile, Some(&subfolder));
    if let Err(refusal) = scope.fenced(&pending.target) {
        return refuse(refusal.to_string());
    }
    match scope.create(dir, name) {
        Ok(created) if created.vault_relative == pending.vault_relative => {}
        Ok(created) => {
            return refuse(format!(
                "the drive's notes vault moved since it was admitted — {} is {} inside it now, not {} — so it is not written anywhere it was not admitted to.",
                pending.target, created.vault_relative, pending.vault_relative
            ))
        }
        Err(refusal) => {
            return refuse(format!(
                "the drive's notes vault moved since it was admitted: {refusal}"
            ))
        }
    }
    match audience(held.zone(), &pending.session, out) {
        Ok(audience) => {
            if let Some(reason) = audience.refusal() {
                return refuse(reason);
            }
        }
        Err(why) => return refuse(why),
    }
    match record_then(
        held,
        &pending.session,
        "promote-out",
        (&pending.source, &pending.target, &pending.note, None),
        &[],
    ) {
        Ok(()) => {}
        Err(VerbError::Refused(why)) => return refuse(why),
        Err(error) => return Err(error),
    }
    publish(held, out, &subfolder, &pending)
}

/// Publish `pending`'s bytes at its target through the vault at
/// `subfolder` — the one its target was checked against — then record the
/// copy in its row ([`promote::upsert_published_row`]) and clear its record:
/// only once the writer says the copy is durable, a target that already
/// holds exactly them included. Only a target the disk says is not there,
/// the copy the session's row records for this source
/// ([`promote::standing`]), or exactly these bytes — this operation's own
/// write, which a crash left without its row's receipt — is written: every
/// pending operation was admitted onto one of the first two
/// ([`promote_out`]), so identical bytes only ever finish it, never adopt a
/// file. Refused, the record cleared, when a link would carry the write,
/// another file is at the target, or what is there cannot be read.
fn publish(
    held: &ZoneLock,
    out: &OutOf,
    subfolder: &str,
    pending: &Pending,
) -> Result<(), VerbError> {
    let root = out.profile.local_path.as_path();
    let refuse = |why: String| {
        clear_pending(held.zone())?;
        Err(VerbError::Refused(format!(
            "{} is not published at {}: {why} Nothing was written there; its row says what is missing.",
            pending.source, pending.target
        )))
    };
    if let Err(VerbError::Refused(why)) = lands_as_named(root, &pending.target) {
        return refuse(why);
    }
    let at = browse::lexical_join(root, &pending.target)
        .map_err(|error| VerbError::Refused(error.to_string()))?;
    let recorded = recorded_copy(
        &read_readme(held.zone(), &pending.session)?,
        &pending.source,
        &pending.target,
    );
    match std::fs::read(&at) {
        // This operation's own copy, already there: written again all the
        // same, so it is durable before its record goes.
        Ok(there) if there == pending.text.as_bytes() => {}
        Ok(there) => {
            if let Err(loss) = promote::standing(&pending.source, recorded.as_deref(), &there) {
                return refuse(loss.explain(&pending.source, &pending.target));
            }
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => {
            return refuse(format!(
                "what is there could not be read ({error}), and keeper writes over a file only when it knows what it replaces."
            ))
        }
    }
    out.vault
        .write(
            &out.profile.id,
            subfolder,
            &pending.vault_relative,
            &pending.text,
        )
        .map_err(|error| {
            VerbError::Refused(format!(
                "{error}; its row is recorded and the reviewed version kept, so the next promotion out of this drive's sessions finishes it."
            ))
        })?;
    let digest = promote::copy_digest(&pending.source, pending.text.as_bytes());
    record_then(
        held,
        &pending.session,
        "promote-out",
        (&pending.source, &pending.target, &pending.note, Some(&digest)),
        &[],
    )
    .map_err(|error| {
        VerbError::Refused(format!(
            "{} is published at {}, but its row could not record the copy ({error}); the reviewed version is kept, so the next promotion out of this drive's sessions records it.",
            pending.source, pending.target
        ))
    })?;
    clear_pending(held.zone())
}

/// Promote the session's artifact `request.source` (session-relative, under
/// `artifacts/`) out into the drive's notes vault at `request.target`
/// (drive-relative) (FR-808): refused when the session's [`audience`]
/// cannot be established or its label may not reach the drive's readers
/// (NFR-115), with the sink's reason; refused outside the vault with
/// [`WriteScope::create`]'s sentence (DW-390), anywhere a plain path inside
/// this drive does not lead and wherever a link would carry it.
///
/// The bytes published are the ones read verified, under a label read
/// from a log that did not grow while they were read. A harvested note is
/// published as the person read it — `request.expected` names that
/// version, and a candidate that has changed since is refused — with
/// `reviewer`'s canonical review in the vault copy (R139, R212); the
/// candidate is never touched. Once admitted, the exact bytes, the target
/// and the row are kept in the zone ([`Pending`]) before either lands: the
/// row `| source | target | note |` (R138) first, then the copy, then the
/// row records the copy it published, then the record is cleared — so a
/// crash or a failed write anywhere between is finished, with that version,
/// by the next promotion out of the zone, or refused with why
/// ([`finish_pending`]). A target already there is replaced only when it is
/// the copy this source's row records as the one it published
/// ([`promote::standing`]) — a re-promotion — and never otherwise, the
/// same bytes there included (R244, R253): refused with why when that row
/// names the target ([`promote::CopyLoss::explain`]), as a name taken when
/// it does not.
pub fn promote_out(
    zone: &Path,
    session: &str,
    request: &Request,
    reviewer: Option<&Reviewer>,
    out: &OutOf,
) -> Result<(), VerbError> {
    let target = request.target;
    let held = exec::hold(zone)?;
    finish_pending(&held, out)?;
    let audience = audience(held.zone(), session, out).map_err(VerbError::Refused)?;
    if let Some(reason) = audience.refusal() {
        return Err(VerbError::Refused(reason));
    }
    let source = landing(held.zone(), session, request.source)?;
    if !source
        .split_once('/')
        .is_some_and(|(top, rest)| top == ARTIFACTS_DIR && !rest.is_empty())
    {
        return Err(VerbError::Refused(format!(
            "only an artifact is promoted into the notes vault; {source} is not under artifacts/."
        )));
    }
    let harvested = knowledge::is_note(&source);
    let not_text = || {
        VerbError::Refused(format!(
            "{source} is not a text file of this session, and the notes vault takes text."
        ))
    };
    let path = session_file(held.zone(), session, &source)
        .filter(|path| path.is_file())
        .ok_or_else(not_text)?;
    let sample = FileSample::of(&path).ok().flatten().ok_or_else(not_text)?;
    let bytes = read_verified(&path, Some(&sample))
        .map_err(|_| VerbError::Refused(format!("{source} is {STILL_WRITING}.")))?;
    let text = String::from_utf8(bytes).map_err(|_| not_text())?;
    match (request.expected, harvested) {
        (None, true) => return Err(VerbError::Refused(UNREVIEWED.to_owned())),
        (Some(expected), _) if expected != sha256_hex(&text) => {
            return Err(VerbError::Refused(format!(
                "{source} changed since it was read for this promotion; read it again before promoting it."
            )))
        }
        _ => {}
    }
    match frontier(held.zone(), session) {
        Ok(now) if now == audience.frontier => {}
        Ok(_) => {
            return Err(VerbError::Refused(format!(
                "{session}'s log grew while this was promoted, so its label may have changed; try again."
            )))
        }
        Err(why) => {
            return Err(VerbError::Refused(format!(
                "its label is not established: its log could not be read whole ({why}), so nothing is promoted out of {session}."
            )))
        }
    }
    let published = match (harvested, reviewer) {
        (true, Some(reviewer)) => knowledge::review(&text, reviewer.person, reviewer.at, true),
        (true, None) => return Err(VerbError::Refused(NO_REVIEWER.to_owned())),
        (false, _) => text,
    };
    let readme = read_readme(held.zone(), session)?;
    recorded(&readme, &source, target, request.note, None)?;
    let refused = |refusal: WriteRefusal| VerbError::Refused(refusal.to_string());
    let Some(subfolder) = out.vault.subfolder(&out.profile.id) else {
        return Err(refused(WriteRefusal::NoVault {
            profile_name: out.profile.name.clone(),
        }));
    };
    let scope = scope(out.profile, Some(&subfolder));
    let (dir, name) = target.rsplit_once('/').unwrap_or(("", target));
    let created = scope.create(dir, name).map_err(refused)?;
    // `create` asks only the vault: a vault holding the agents or sessions
    // zone must not let a promotion write a skill, a home's memory or a
    // session's scratch, which change only through their own writers.
    scope.fenced(target).map_err(refused)?;
    let root = out.profile.local_path.as_path();
    lands_as_named(root, target)?;
    let directory =
        browse::lexical_join(root, dir).map_err(|error| VerbError::Refused(error.to_string()))?;
    let session_id = scan::session_id(
        &browse::lexical_join(held.zone(), session)
            .map_err(|error| VerbError::Refused(error.to_string()))?,
        session,
    );
    if directory.is_dir() && collides(&directory, name).map_err(refused)? {
        let row = promote::parse(&readme).and_then(|table| {
            promote::entry_of(&table, &source)
                .filter(|(_, named, _)| *named == target)
                .map(|(_, _, published)| published.map(str::to_owned))
        });
        let there = browse::lexical_join(root, target)
            .ok()
            .and_then(|at| std::fs::read(at).ok());
        let (Some(recorded), Some(there)) = (row, there) else {
            return Err(refused(WriteRefusal::NameTaken {
                name: name.to_owned(),
            }));
        };
        if let Err(loss) = promote::standing(&source, recorded.as_deref(), &there) {
            return Err(VerbError::Refused(loss.explain(&source, target)));
        }
    }
    let pending = Pending {
        session: session.to_owned(),
        session_id,
        source,
        target: target.to_owned(),
        note: request.note.to_owned(),
        profile: out.profile.id.clone(),
        vault_relative: created.vault_relative,
        text: published,
    };
    keep_pending(held.zone(), &pending)?;
    if let Err(error) = record_then(
        &held,
        session,
        "promote-out",
        (&pending.source, target, request.note, None),
        &[],
    ) {
        // Nothing was published: the record goes with the refusal.
        clear_pending(held.zone())?;
        return Err(error);
    }
    publish(&held, out, &subfolder, &pending)
}

/// A person's *Reviewed by me* (`reviewed`) or its untick on the harvested
/// note at `path` (session-relative), after its promotion: written into the
/// vault copy its row names (R139), by [`knowledge::review`] as `person` (a
/// Matrix localpart) at `at`, through the vault's guarded amend — composed
/// from the copy as it is at the write, so an edit or another review that
/// landed meanwhile is kept, never written over. The candidate the agent's
/// host writes is never touched. A promotion out the zone has not finished
/// is finished first ([`finish_pending`]). Refused with [`NOT_PROMOTED`]
/// before the note is promoted into the vault, wherever a link would carry
/// it, and when what is at the row's target is not the copy the row records
/// as the one it published ([`promote::standing`]), as it is at the write —
/// refused with why ([`promote::CopyLoss::explain`]): a review is never
/// written into a file the note did not put there (R244, R252).
pub fn review(
    zone: &Path,
    session: &str,
    path: &str,
    person: &str,
    at: &str,
    reviewed: bool,
    out: &OutOf,
) -> Result<(), VerbError> {
    let held: ZoneLock = exec::hold(zone)?;
    finish_pending(&held, out)?;
    let path = landing(held.zone(), session, path)?;
    if !knowledge::is_note(&path) {
        return Err(VerbError::Refused(format!(
            "{path} is not a harvested note: those live under {}/.",
            knowledge::KNOWLEDGE_DIR
        )));
    }
    let readme = read_readme(held.zone(), session)?;
    let (target, recorded) = promote::parse(&readme)
        .and_then(|table| {
            promote::entry_of(&table, &path)
                .filter(|(_, target, _)| !promote::target_in_session(target))
                .map(|(_, target, published)| (target.to_owned(), published.map(str::to_owned)))
        })
        .ok_or_else(|| VerbError::Refused(NOT_PROMOTED.to_owned()))?;
    drop(held);
    let subfolder = out.vault.subfolder(&out.profile.id);
    let scope = scope(out.profile, subfolder.as_deref());
    let root = out.profile.local_path.as_path();
    let WriteRoute::Vault {
        path: vault_path, ..
    } = scope
        .route(Some(()), root, &target)
        .map_err(|refusal| VerbError::Refused(refusal.to_string()))?
    else {
        return Err(VerbError::Refused(NOT_PROMOTED.to_owned()));
    };
    if browse::resolve_known(root, &target)
        .map_err(|refusal| VerbError::Refused(refusal.to_string()))?
        .landed()
        .is_none()
    {
        return Err(VerbError::Refused(format!(
            "{target} is not in the vault any more."
        )));
    }
    lands_as_named(root, &target)?;
    let lost = std::cell::Cell::new(None);
    out.vault
        .amend(&out.profile.id, vault_path.as_str(), &|text| {
            lost.set(promote::standing(&path, recorded.as_deref(), text.as_bytes()).err());
            if lost.get().is_some() {
                return None;
            }
            let updated = knowledge::review(text, person, at, reviewed);
            (updated != text).then_some(updated)
        })
        .map_err(VerbError::Refused)?;
    if let Some(loss) = lost.get() {
        return Err(VerbError::Refused(loss.explain(&path, &target)));
    }
    Ok(())
}

#[cfg(test)]
mod tests;
