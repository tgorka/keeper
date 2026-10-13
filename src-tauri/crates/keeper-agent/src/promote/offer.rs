//! What the promote panel offers and what binds a person's choice to what
//! they were shown (R216, R234): every offer, read target, destination and
//! transition is decided here and only rendered by the panel (AD-27).
//!
//! - [`complete`] finishes the panel: each harvested note's vault copy and
//!   where promoting it goes, and the session's other artifacts with what
//!   promoting each out offers or why not.
//! - [`panel_for`] is the panel with what the person did in it decided
//!   ([`offers::decide`]).
//! - [`read_note`] reads a harvested note — its candidate or its vault
//!   copy — whole, with the revision a promotion or a review then names.
//! - [`promote_to`] promotes an artifact out to the folder and filename a
//!   person chose, the target composed here.
//! - [`archive`] archives with the checklist's choices, every row decided,
//!   its promotions run as [`super::promote_in`] runs them in the
//!   archive's own plan, refused when the workspace, the table or a row's
//!   target changed since the checklist was read.

use std::io::Read as _;
use std::path::Path;

use keeper_core::agents::knowledge;
use keeper_core::sessions::model::{ARTIFACTS_DIR, README};
use keeper_core::sessions::offer::{
    self as offers, ArtifactOfferVm, ChoiceVm, NoteTextVm, PanelIntentVm, VaultCopyVm,
};
use keeper_core::sessions::plan::{sha256_hex, Emptying, PlanStep};
use keeper_core::sessions::promote::{
    self, PanelFacts, PromoteRow, PromoteState, SessionPromoteVm,
};
use keeper_sync::browse;

use super::{OutOf, Request, Reviewer};
use crate::sessions::verbs::{self, VerbError};
use crate::sessions::write::landing;

/// Whether the file at `path` is text the notes vault takes: UTF-8 through
/// its last byte, read in pieces so a large artifact is never held whole.
pub(super) fn is_text(path: &Path) -> std::io::Result<bool> {
    let mut file = std::fs::File::open(path)?;
    let mut piece = [0_u8; 8192];
    let mut carry: Vec<u8> = Vec::new();
    loop {
        let read = file.read(&mut piece)?;
        if read == 0 {
            return Ok(carry.is_empty());
        }
        carry.extend_from_slice(&piece[..read]);
        match std::str::from_utf8(&carry) {
            Ok(_) => carry.clear(),
            Err(error) if error.error_len().is_some() => return Ok(false),
            Err(error) => {
                carry.drain(..error.valid_up_to());
            }
        }
    }
}

/// The text of the file at `path` if it is whole within `cap` bytes and
/// UTF-8, else why not.
fn note_text(path: &Path, rel: &str, cap: usize) -> Result<String, String> {
    let bytes = std::fs::metadata(path)
        .map_err(|error| format!("{rel} could not be read: {error}"))?
        .len();
    if bytes > cap as u64 {
        return Err(format!(
            "{rel} holds {bytes} bytes, more than the {cap} keeper reads of a knowledge note, so it is not shown"
        ));
    }
    std::fs::read_to_string(path).map_err(|error| format!("{rel} could not be read: {error}"))
}

/// The vault copy at the drive-relative `target`, as the panel finds it:
/// read whole up to a reviewed note's bound
/// ([`knowledge::MAX_REVIEWED_BYTES`]), so a note at the candidate's cap
/// with its reviews in is read.
fn vault_copy(root: &Path, target: &str) -> VaultCopyVm {
    let (there, read) = match super::locate(root, target) {
        Ok(None) => (false, Ok(None)),
        Ok(Some(path)) => (
            true,
            note_text(&path, target, knowledge::MAX_REVIEWED_BYTES).map(Some),
        ),
        Err(why) => (true, Err(why)),
    };
    VaultCopyVm {
        path: target.to_owned(),
        there,
        revision: read
            .as_ref()
            .ok()
            .and_then(Option::as_ref)
            .map(|text| offers::copy_revision(target, text)),
        problem: read.err(),
    }
}

/// The drive-relative target the row of `source` names out of the session:
/// its one row ([`promote::entry_of`]), so a second row naming the source
/// never lends the panel a target the commands do not take.
fn out_target(table: Option<&promote::PromoteTable>, source: &str) -> Option<String> {
    let (_, target, _) = promote::entry_of(table?, source)?;
    (!promote::target_in_session(target)).then(|| target.to_owned())
}

/// Finish the panel `vm` of the session at `session` (zone-relative) in
/// `zone`, whose README is `readme`: each harvested note's vault copy as it
/// is, and where promoting it goes — the vault, under its own name, for a
/// note not promoted yet; its row's target to restore a missing copy or
/// publish a newer candidate — and the session's other artifacts, each with
/// its destination or why it may not be promoted out (not text the vault
/// takes, no table to record it). Nothing is offered out where the drive
/// has no vault or the session's audience refuses it (`vm.out_refused`).
pub fn complete(vm: &mut SessionPromoteVm, readme: &str, zone: &Path, session: &str, out: &OutOf) {
    let root = out.profile.local_path.as_path();
    let table = promote::parse(readme);
    let vault = vm.vault.clone().filter(|_| vm.out_refused.is_none());
    let no_table = (!vm.has_table).then(|| promote::RowRefusal::NoTable.to_string());
    for note in &mut vm.knowledge {
        let target = out_target(table.as_ref(), &note.path);
        note.copy = target.as_deref().map(|target| vault_copy(root, target));
        let Some(vault) = vault.as_deref().filter(|_| note.problem.is_none()) else {
            continue;
        };
        note.unavailable.clone_from(&no_table);
        if no_table.is_some() {
            continue;
        }
        // A file at the row's target that is not the copy the note published
        // is never replaced (`promote::standing`): nothing is offered there,
        // and the note says why.
        if note.foreign_copy.is_some() {
            note.unavailable.clone_from(&note.foreign_copy);
            continue;
        }
        note.destination = match (target, note.state) {
            (None, _) => Some(offers::destination(
                &format!("{vault}/{}", file_name(&note.path)),
                false,
            )),
            (Some(target), Some(PromoteState::MissingTarget | PromoteState::Stale)) => {
                Some(offers::destination(&target, true))
            }
            (Some(_), _) => None,
        };
    }
    let Ok(dir) = browse::lexical_join(zone, session) else {
        return;
    };
    let artifacts = super::walk(&dir, ARTIFACTS_DIR, &mut vm.problems);
    vm.artifacts = artifacts
        .into_iter()
        .filter(|rel| !knowledge::is_note(rel) && rel.rsplit('/').next() != Some(".gitkeep"))
        .map(|path| {
            let unavailable = match is_text(&dir.join(&path)) {
                Ok(true) => no_table.clone(),
                Ok(false) => Some(format!(
                    "{path} is not a text file, and the notes vault takes text."
                )),
                Err(error) => Some(format!("{path} could not be read: {error}")),
            };
            let destination = match (&vault, &unavailable) {
                (Some(vault), None) => Some(match out_target(table.as_ref(), &path) {
                    Some(target) => offers::destination(&target, true),
                    None => offers::destination(&format!("{vault}/{}", file_name(&path)), false),
                }),
                _ => None,
            };
            ArtifactOfferVm {
                path,
                destination,
                unavailable,
            }
        })
        .collect();
}

fn file_name(rel: &str) -> &str {
    rel.rsplit('/').next().unwrap_or(rel)
}

/// The harvested note `path` (session-relative) read whole: its candidate,
/// or — `copy` — the vault copy its row names, with the revision of exactly
/// the bytes read (a copy's [`offers::copy_revision`], at its path). Refused
/// for anything but a harvested note, for a copy that is not there or
/// leads through a link, and for a file not UTF-8 or larger than keeper
/// reads — a candidate past a knowledge note's cap, a copy past a reviewed
/// note's.
pub fn read_note(
    zone: &Path,
    session: &str,
    path: &str,
    copy: bool,
    out: &OutOf,
) -> Result<NoteTextVm, VerbError> {
    let path = landing(zone, session, path)?;
    if !knowledge::is_note(&path) {
        return Err(VerbError::Refused(format!(
            "{path} is not a harvested note: those live under {}/.",
            knowledge::KNOWLEDGE_DIR
        )));
    }
    let (file, rel, cap) = if copy {
        let readme = super::read_readme(zone, session)?;
        let target = out_target(promote::parse(&readme).as_ref(), &path)
            .ok_or_else(|| VerbError::Refused(super::NOT_PROMOTED.to_owned()))?;
        let root = out.profile.local_path.as_path();
        super::lands_as_named(root, &target)?;
        let found = super::locate(root, &target).map_err(VerbError::Refused)?;
        (found, target, knowledge::MAX_REVIEWED_BYTES)
    } else {
        let found =
            super::locate(zone, &format!("{session}/{path}")).map_err(VerbError::Refused)?;
        (found, path, knowledge::MAX_NOTE_BYTES)
    };
    let file = file.ok_or_else(|| VerbError::Refused(format!("{rel} is not there any more.")))?;
    let text = note_text(&file, &rel, cap).map_err(VerbError::Refused)?;
    Ok(NoteTextVm {
        revision: if copy {
            offers::copy_revision(&rel, &text)
        } else {
            sha256_hex(&text)
        },
        text,
    })
}

/// Promote the session's artifact `source` out into the drive's notes vault
/// at the `folder` and `name` a person chose — composed into the target
/// here, inside the vault only ([`offers::compose_target`]) — by
/// [`super::promote_out`]: a harvested note as the version `expected` the
/// person read, recorded `knowledge`; any other artifact under the note its
/// row already has.
#[allow(clippy::too_many_arguments)]
pub fn promote_to(
    zone: &Path,
    session: &str,
    source: &str,
    folder: &str,
    name: &str,
    expected: Option<&str>,
    reviewer: Option<&Reviewer>,
    out: &OutOf,
) -> Result<(), VerbError> {
    let vault = out.vault.subfolder(&out.profile.id).ok_or_else(|| {
        VerbError::Refused("this drive has no notes vault, so nothing was promoted.".to_owned())
    })?;
    let target = offers::compose_target(&vault, folder, name).map_err(VerbError::Refused)?;
    let note = if knowledge::is_note(source) {
        "knowledge".to_owned()
    } else {
        promote::parse(&super::read_readme(zone, session)?)
            .and_then(|table| {
                table.rows.into_iter().find_map(|row| match row {
                    PromoteRow::Entry {
                        source: from, note, ..
                    } if from == source => Some(note),
                    _ => None,
                })
            })
            .unwrap_or_default()
    };
    super::promote_out(
        zone,
        session,
        &Request {
            source,
            target: &target,
            note: &note,
            expected,
        },
        reviewer,
        out,
    )
}

/// The promote panel of the session at `session` (zone-relative) in `zone`
/// ([`super::panel`]) with what the person did in it, `intent`, decided
/// ([`offers::decide`]): the choices that still hold, whether each read is
/// current, whether consent stands, whether the checklist is complete.
pub fn panel_for(
    zone: &Path,
    session: &str,
    out: &OutOf,
    me: Option<&str>,
    intent: &PanelIntentVm,
) -> Result<SessionPromoteVm, VerbError> {
    let mut vm = super::panel(zone, session, out, me)?;
    offers::decide(&mut vm, intent);
    Ok(vm)
}

/// An archive checklist's decision: a choice about every row and unlisted
/// file, the revision of the checklist they were made on, and whether to
/// empty the workspace.
#[derive(Debug, Clone, Copy)]
pub struct Archive<'a> {
    /// The person's choices ([`SessionPromoteVm::intent`]'s), one per item
    /// of the checklist.
    pub choices: &'a [ChoiceVm],
    /// [`SessionPromoteVm::revision`] as the person reviewed it.
    pub revision: &'a str,
    pub empty_workspace: bool,
    /// The close year: `archive/<year>/`.
    pub year: i32,
    /// The drive's root, where a row's target out of the session is.
    pub root: &'a Path,
}

/// Archive the session `session_id` with the checklist's choices (FR-245,
/// R216, R234): refused, with nothing changed, unless the checklist is the
/// one the person read ([`offers::snapshot_revision`] — the README, every
/// `workspace/` entry the emptying removes, hidden or not, and every row's
/// target) and every item has its one choice ([`offers::archive_promotions`]),
/// or when the workspace cannot be listed whole. Each promotion is admitted
/// as [`super::promote_in`] admits one — a settled `workspace/` file into
/// `artifacts/`, never into `workspace/` or onto the README — its row
/// recorded in the README in a form the table reads back and its verified
/// bytes copied, all in the archive's one journaled plan before the guarded
/// emptying ([`verbs::archive_with`]) and the move. That emptying carries
/// what every target of a row whose source it removes said when the
/// checklist was read — or, promoted, the bytes the plan's own copy writes
/// there — and is refused, on its run or on a resume, once one says
/// anything else (R249).
pub fn archive(
    zone: &Path,
    session_id: &str,
    decision: &Archive,
    settle_ms: u64,
    now_ms: i64,
) -> Result<(), VerbError> {
    verbs::archive_with(
        zone,
        session_id,
        decision.empty_workspace,
        decision.year,
        |held, session, workspace| {
            if !workspace.problems.is_empty() {
                return Err(VerbError::Refused(format!(
                    "{}; the workspace cannot be checked whole, so nothing was archived.",
                    workspace.problems.join("; ")
                )));
            }
            let readme = super::read_readme(held.zone(), session)?;
            let mut facts = PanelFacts {
                files: super::row_files(held.zone(), session, decision.root, &readme, false),
                ..PanelFacts::default()
            };
            super::workspace_facts(&mut facts, workspace.entries.clone());
            let checklist = promote::promote_panel(&readme, &facts, None);
            if checklist.revision != decision.revision {
                return Err(VerbError::Refused(offers::SNAPSHOT_CHANGED.to_owned()));
            }
            let promotes = offers::archive_promotions(&checklist, decision.choices)
                .map_err(VerbError::Refused)?;
            let mut emptying = Emptying {
                zone_in_drive: super::zone_in_drive(held.zone(), decision.root),
                ..Emptying::default()
            };
            for row in &checklist.rows {
                if !facts.stamps.contains_key(&row.source) {
                    continue;
                }
                let fact = offers::target_fact(facts.files.get(&row.target));
                if promote::target_in_session(&row.target) {
                    emptying
                        .targets
                        .insert(format!("{session}/{}", row.target), fact);
                } else {
                    emptying.drive_targets.insert(row.target.clone(), fact);
                }
            }
            let table = promote::parse(&readme);
            let mut updated = readme.clone();
            let mut steps = Vec::with_capacity(2 * promotes.len() + 1);
            for (source, target) in &promotes {
                let note = table
                    .as_ref()
                    .and_then(|table| {
                        table.rows.iter().find_map(|row| match row {
                            PromoteRow::Entry {
                                source: from, note, ..
                            } if from == source => Some(note.as_str()),
                            _ => None,
                        })
                    })
                    .unwrap_or_default();
                let (source, target, then) =
                    super::admit_in(held, session, source, target, note, settle_ms, now_ms)?;
                updated = super::recorded(&updated, &source, &target, note, None)?;
                steps.extend(then);
            }
            for step in &steps {
                if let PlanStep::CopyChecked { to, sha256, .. } = step {
                    emptying
                        .targets
                        .insert(to.clone(), format!("sha256:{sha256}"));
                }
            }
            if !promotes.is_empty() {
                steps.insert(
                    0,
                    PlanStep::guarded(format!("{session}/{README}"), &readme, updated),
                );
            }
            Ok((steps, emptying))
        },
    )
}
