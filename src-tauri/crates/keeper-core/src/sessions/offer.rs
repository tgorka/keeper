//! What the promote panel offers, file by file, and what binds a person's
//! choice to what they were shown (R216, R234).
//!
//! Rust decides every offer — which file may be promoted where, under what
//! suggested name, and why not when it may not — and composes every
//! destination from the folder and filename the person chose; the panel
//! renders these answers and owns no path or eligibility rule (AD-27). The
//! revisions here are what an archive decision and a row's choice are bound
//! to: a workspace or table that changed between the checklist and the
//! archive is refused, and only the rows whose revision changed lose their
//! choice. What the person did in the panel — the choices, the versions
//! read, the consent — comes back as their intent ([`PanelIntentVm`]) and
//! [`decide`] says what of it still holds: which choices, which reads are
//! current, whether consent stands and whether the checklist is complete.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use ts_rs::TS;

use crate::sessions::model::{ARTIFACTS_DIR, WORKSPACE_DIR};
use crate::sessions::promote::{FileFact, PromoteRowVm, PromoteState, SessionPromoteVm};

/// What an archive whose checklist is out of date says.
pub const SNAPSHOT_CHANGED: &str = "the session's workspace or its ## Promote table changed since this checklist was read, so nothing was archived; review it again.";

/// Where a promotion out into the notes vault goes: the folder the picker
/// starts in and the filename it suggests, both Rust's.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
#[serde(rename_all = "camelCase")]
pub struct DestinationVm {
    /// Drive-relative, the vault or a folder inside it.
    pub folder: String,
    pub name: String,
    /// The destination is the one the file's row already names — a repair
    /// of a missing copy, or a re-promotion of a newer candidate — and is
    /// not chosen again.
    pub fixed: bool,
}

/// The vault copy a harvested note's row names, as the panel found it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
#[serde(rename_all = "camelCase")]
pub struct VaultCopyVm {
    /// Drive-relative.
    pub path: String,
    /// Whether a file is there.
    pub there: bool,
    /// [`copy_revision`] of the copy as read for this panel: the version a
    /// review of it names. `None` when it is not there or was not read
    /// whole (`problem`).
    pub revision: Option<String>,
    pub problem: Option<String>,
}

/// A person's choice about one checklist item: promote it to `target`
/// (session-relative, under `artifacts/`), or skip it — bound to the
/// item's `revision` as they saw it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
#[serde(rename_all = "camelCase")]
pub struct ChoiceVm {
    pub revision: String,
    pub promote: bool,
    pub target: String,
}

/// What the person did with one harvested note in the panel: the versions
/// they read of its candidate (`read`) and of its vault copy (`copy`), and
/// the candidate version they said they reviewed (`consent`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
#[serde(rename_all = "camelCase")]
pub struct NoteIntentVm {
    /// Session-relative.
    pub path: String,
    pub read: Option<String>,
    pub copy: Option<String>,
    pub consent: Option<String>,
}

/// Everything the person did in the panel, as the panel forwards it and as
/// [`decide`] keeps it.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
#[serde(rename_all = "camelCase")]
pub struct PanelIntentVm {
    pub choices: Vec<ChoiceVm>,
    pub notes: Vec<NoteIntentVm>,
}

/// Whether a version the person read is the one the panel shows now.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
#[serde(rename_all = "camelCase")]
pub enum ReadState {
    /// Not read.
    #[default]
    Unread,
    /// Read, and still what is there.
    Current,
    /// Read, and changed since.
    Stale,
}

/// A `workspace/` entry no row names.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
#[serde(rename_all = "camelCase")]
pub struct UnlistedVm {
    /// Session-relative.
    pub source: String,
    /// The `artifacts/` target a promotion suggests, session-relative.
    pub suggested: String,
    /// What a choice about it is bound to: changes when the file does.
    pub revision: String,
    /// Why it cannot be promoted, or `None`.
    pub refused: Option<String>,
    /// The person's choice about it, while it still holds ([`decide`]).
    pub choice: Option<ChoiceVm>,
}

/// An artifact, not a harvested note, that may be promoted out into the
/// drive's notes vault.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
#[serde(rename_all = "camelCase")]
pub struct ArtifactOfferVm {
    /// Session-relative, under `artifacts/`.
    pub path: String,
    /// Where it would go; `None` when it may not be promoted out — this
    /// file's reason is `unavailable`, the session's is the panel's
    /// `outRefused`, a drive without a vault the panel's `vault`.
    pub destination: Option<DestinationVm>,
    pub unavailable: Option<String>,
}

/// A harvested note's text as read for the person, and the version it is.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
#[serde(rename_all = "camelCase")]
pub struct NoteTextVm {
    pub text: String,
    /// What a promotion or a review of what was read names: the candidate's
    /// SHA-256, a vault copy's [`copy_revision`].
    pub revision: String,
}

/// The destination `target` (drive-relative) names, split where the picker
/// shows it.
pub fn destination(target: &str, fixed: bool) -> DestinationVm {
    let (folder, name) = target.rsplit_once('/').unwrap_or(("", target));
    DestinationVm {
        folder: folder.to_owned(),
        name: name.to_owned(),
        fixed,
    }
}

/// The drive-relative target a person chose as `folder` (inside `vault`)
/// and `name`, or why it is not one: a name that is empty, a dot name or
/// holds a separator or a control character, or a folder outside the vault.
/// The write that follows checks the vault's fences again.
///
/// # Errors
/// The refusal's sentence.
pub fn compose_target(vault: &str, folder: &str, name: &str) -> Result<String, String> {
    let inside = folder == vault
        || folder
            .strip_prefix(vault)
            .is_some_and(|rest| rest.starts_with('/') && !rest.ends_with('/'));
    if vault.is_empty()
        || !inside
        || folder
            .split('/')
            .any(|part| matches!(part, "" | "." | ".."))
    {
        return Err(format!(
            "{folder} is not a folder of this drive's notes vault ({vault}), so nothing was promoted."
        ));
    }
    if matches!(name, "" | "." | "..")
        || name.contains(['/', '\\'])
        || name.chars().any(char::is_control)
        || name.trim() != name
    {
        return Err(format!(
            "`{name}` is not a filename keeper writes a note under, so nothing was promoted."
        ));
    }
    Ok(format!("{folder}/{name}"))
}

/// The `artifacts/` target a promotion of `source` suggests: its filename,
/// directly under `artifacts/`.
pub fn suggested_artifact(source: &str) -> String {
    let name = source.rsplit('/').next().unwrap_or(source);
    format!("{ARTIFACTS_DIR}/{name}")
}

fn hex(hasher: Sha256) -> String {
    hex::encode(hasher.finalize())
}

/// A `workspace/` entry's stamp that a choice about it is bound to, as the
/// session runtime's inventory writes it: a regular file's
/// `file:<length>:<mtime ns>`, a link's `link:<where it points>`, a
/// folder's `dir`, anything else `other`.
pub fn is_regular(stamp: &str) -> bool {
    stamp.starts_with("file:")
}

/// What a row's choice knows of its target ([`row_revision`]): what it
/// says — its digest, review keys aside, so a tick does not move it — or
/// that it is absent or could not be read.
pub fn target_fact(fact: Option<&Result<FileFact, String>>) -> String {
    match fact {
        None => "absent".to_owned(),
        Some(Ok(fact)) => format!("digest:{}", fact.digest),
        Some(Err(_)) => "unreadable".to_owned(),
    }
}

/// What a choice about the row `source → target` is bound to: its cells,
/// its source as `stamp` saw it and its target as `target` ([`target_fact`])
/// — so a target deleted or replaced takes the choice with it.
pub fn row_revision(
    source: &str,
    target: &str,
    note: &str,
    stamp: Option<&str>,
    target_fact: Option<&str>,
) -> String {
    let mut hasher = Sha256::new();
    for part in [
        source,
        target,
        note,
        stamp.unwrap_or("-"),
        target_fact.unwrap_or("-"),
    ] {
        hasher.update(part.as_bytes());
        hasher.update([0]);
    }
    hex(hasher)
}

/// The revision of the `nth` repeat (counted from 1) of a table line whose
/// first occurrence's revision is `revision`: a line written twice is two
/// items of the checklist, each with its own choice (R249).
pub fn occurrence(revision: &str, nth: usize) -> String {
    let mut hasher = Sha256::new();
    hasher.update(revision.as_bytes());
    hasher.update([0]);
    hasher.update(nth.to_string().as_bytes());
    hasher.update([0]);
    hex(hasher)
}

/// Why a repeat of the row `source → target` offers no promotion.
pub fn repeated(source: &str, target: &str) -> String {
    format!(
        "this line repeats an earlier row of the ## Promote table, {source} → {target}, so it promotes nothing of its own; the earlier row's choice decides it."
    )
}

/// What the archive checklist is bound to: the README's bytes, every
/// `workspace/` entry with its `stamp` — a file written, added or removed,
/// a hidden one, a link or a folder too — and every row's target with what
/// it says (`targets`, [`target_fact`]).
pub fn snapshot_revision(
    readme: &str,
    stamps: &BTreeMap<String, String>,
    targets: &BTreeMap<String, String>,
) -> String {
    let mut hasher = Sha256::new();
    hasher.update(readme.as_bytes());
    hasher.update([0]);
    for (path, stamp) in stamps {
        hasher.update(path.as_bytes());
        hasher.update([0]);
        hasher.update(stamp.as_bytes());
        hasher.update([0]);
    }
    // A path is never empty: the lone NUL ends the workspace's entries.
    hasher.update([0]);
    for (target, fact) in targets {
        hasher.update(target.as_bytes());
        hasher.update([0]);
        hasher.update(fact.as_bytes());
        hasher.update([0]);
    }
    hex(hasher)
}

/// The version of a vault copy at the drive-relative `target` holding
/// `text` that a review names: the copy at that path with those bytes, so
/// a row retargeted to another file holding the same text is not it.
pub fn copy_revision(target: &str, text: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(target.as_bytes());
    hasher.update([0]);
    hasher.update(text.as_bytes());
    hex(hasher)
}

fn under(rel: &str, top: &str) -> bool {
    rel.split_once('/')
        .is_some_and(|(first, rest)| first == top && !rest.is_empty())
}

/// Why the table row `row` offers no promotion into the session, or `None`;
/// `source_there` is whether its source is in the session.
pub fn refused_in(row: &PromoteRowVm, source_there: bool) -> Option<String> {
    if row.state == PromoteState::Unreadable {
        return Some(
            "keeper cannot read this line of the ## Promote table, so it promotes nothing from it; correct the line in the README.".to_owned(),
        );
    }
    if row.out {
        return Some(format!(
            "{} was promoted into the drive's notes; promote it again from its notes section.",
            row.source
        ));
    }
    if !source_there {
        return Some(format!("{} is not in this session any more.", row.source));
    }
    if !under(&row.source, WORKSPACE_DIR) || !under(&row.target, ARTIFACTS_DIR) {
        return Some(format!(
            "a promotion copies a file of workspace/ into artifacts/; {} → {} is not one.",
            row.source, row.target
        ));
    }
    None
}

/// Why the unlisted `workspace/` entry `source`, whose stamp is `stamp`,
/// cannot be promoted: it is not a regular file. Skipping it lets the
/// archive remove it with the workspace.
pub fn refused_unlisted(source: &str, stamp: Option<&str>) -> Option<String> {
    (!stamp.is_some_and(is_regular)).then(|| {
        format!(
            "{source} is not a regular file — a link or a special file — so it is not promoted; skipping it lets the archive remove it with the workspace."
        )
    })
}

/// One item of the checklist [`decisions`] decides about: its revision and
/// whether it may only be skipped.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ItemFact {
    pub revision: String,
    pub refused: bool,
}

/// One harvested note [`decisions`] decides about: the versions the panel
/// shows of its candidate and its vault copy.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NoteFact {
    pub path: String,
    pub revision: Option<String>,
    pub copy: Option<String>,
}

/// What [`decisions`] says of one note.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NoteDecision {
    pub path: String,
    pub candidate_read: ReadState,
    pub copy_read: ReadState,
    pub consented: bool,
}

/// What of a person's intent still holds over the panel as it is now.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Decisions {
    /// The intent kept: choices about items still at the revision they
    /// were made on (a promotion only where one is offered), reads of notes
    /// still in the panel, consent only to the version read and shown.
    pub intent: PanelIntentVm,
    pub notes: Vec<NoteDecision>,
    /// Every item has a choice.
    pub complete: bool,
}

fn read_state(read: Option<&str>, shown: Option<&str>) -> ReadState {
    match read {
        None => ReadState::Unread,
        Some(read) if Some(read) == shown => ReadState::Current,
        Some(_) => ReadState::Stale,
    }
}

/// The transitions of the panel's choices and reviews (R234): a choice
/// holds only for an item at the revision it was made on — a changed or
/// gone item loses it, and an item that offers no promotion keeps only a
/// skip — the last choice about an item wins; a read is current while the
/// panel shows that version; consent holds only to the candidate version
/// read and shown, and is dropped otherwise; the checklist is complete when
/// every item has a choice.
pub fn decisions(items: &[ItemFact], notes: &[NoteFact], intent: &PanelIntentVm) -> Decisions {
    let mut chosen: BTreeMap<&str, &ChoiceVm> = BTreeMap::new();
    for choice in &intent.choices {
        chosen.insert(choice.revision.as_str(), choice);
    }
    let choices: Vec<ChoiceVm> = items
        .iter()
        .filter_map(|item| {
            chosen
                .get(item.revision.as_str())
                .filter(|choice| !(choice.promote && item.refused))
                .map(|choice| (*choice).clone())
        })
        .collect();
    let complete = choices.len() == items.len();
    let mut kept = Vec::new();
    let decided = notes
        .iter()
        .map(|note| {
            let held = intent
                .notes
                .iter()
                .rev()
                .find(|held| held.path == note.path);
            let read = held.and_then(|held| held.read.as_deref());
            let copy = held.and_then(|held| held.copy.as_deref());
            let candidate_read = read_state(read, note.revision.as_deref());
            let consented = candidate_read == ReadState::Current
                && held.and_then(|held| held.consent.as_deref()) == read;
            if held.is_some() {
                kept.push(NoteIntentVm {
                    path: note.path.clone(),
                    read: read.map(str::to_owned),
                    copy: copy.map(str::to_owned),
                    consent: read.filter(|_| consented).map(str::to_owned),
                });
            }
            NoteDecision {
                path: note.path.clone(),
                candidate_read,
                copy_read: read_state(copy, note.copy.as_deref()),
                consented,
            }
        })
        .collect();
    Decisions {
        intent: PanelIntentVm {
            choices,
            notes: kept,
        },
        notes: decided,
        complete,
    }
}

/// The panel `vm` with `intent` decided ([`decisions`]): each row's and
/// unlisted file's choice, each note's reads and consent, the intent kept
/// and whether the checklist is complete.
pub fn decide(vm: &mut SessionPromoteVm, intent: &PanelIntentVm) {
    let items: Vec<ItemFact> = vm
        .rows
        .iter()
        .map(|row| (&row.revision, row.refused.is_some()))
        .chain(
            vm.unlisted
                .iter()
                .map(|file| (&file.revision, file.refused.is_some())),
        )
        .map(|(revision, refused)| ItemFact {
            revision: revision.clone(),
            refused,
        })
        .collect();
    let notes: Vec<NoteFact> = vm
        .knowledge
        .iter()
        .map(|note| NoteFact {
            path: note.path.clone(),
            revision: note.revision.clone(),
            copy: note.copy.as_ref().and_then(|copy| copy.revision.clone()),
        })
        .collect();
    let decided = decisions(&items, &notes, intent);
    let choice = |revision: &str| {
        decided
            .intent
            .choices
            .iter()
            .find(|choice| choice.revision == revision)
            .cloned()
    };
    for row in &mut vm.rows {
        row.choice = choice(&row.revision);
    }
    for file in &mut vm.unlisted {
        file.choice = choice(&file.revision);
    }
    for (note, decision) in vm.knowledge.iter_mut().zip(&decided.notes) {
        note.candidate_read = decision.candidate_read;
        note.copy_read = decision.copy_read;
        note.consented = decision.consented;
    }
    vm.complete = decided.complete;
    vm.intent = decided.intent;
}

/// What an archive whose checklist lacks a choice says.
pub fn undecided(source: &str) -> String {
    format!(
        "{source} has no choice yet; promote or skip every row before archiving, so nothing was archived."
    )
}

/// The promotions an archive runs, `(source, target)` session-relative,
/// from `choices` over the checklist `vm` as it is now: refused unless every
/// item has exactly the one choice the person made about it at its current
/// revision, and a promotion only of an item that offers one — so an
/// explicitly decided checklist is told from one with no decisions at all.
///
/// # Errors
/// The refusal's sentence.
pub fn archive_promotions(
    vm: &SessionPromoteVm,
    choices: &[ChoiceVm],
) -> Result<Vec<(String, String)>, String> {
    let items: Vec<(&str, &str, Option<&String>)> = vm
        .rows
        .iter()
        .map(|row| {
            let name = if row.source.is_empty() {
                row.raw.as_deref().unwrap_or_default()
            } else {
                &row.source
            };
            (row.revision.as_str(), name, row.refused.as_ref())
        })
        .chain(vm.unlisted.iter().map(|file| {
            (
                file.revision.as_str(),
                file.source.as_str(),
                file.refused.as_ref(),
            )
        }))
        .collect();
    let mut by_revision: BTreeMap<&str, &ChoiceVm> = BTreeMap::new();
    for choice in choices {
        if !items
            .iter()
            .any(|(revision, ..)| *revision == choice.revision)
            || by_revision
                .insert(choice.revision.as_str(), choice)
                .is_some()
        {
            return Err(SNAPSHOT_CHANGED.to_owned());
        }
    }
    let mut promotes = Vec::new();
    for (revision, name, refused) in items {
        let choice = by_revision.get(revision).ok_or_else(|| undecided(name))?;
        if !choice.promote {
            continue;
        }
        if let Some(refused) = refused {
            return Err(refused.clone());
        }
        promotes.push((name.to_owned(), choice.target.clone()));
    }
    Ok(promotes)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The person's folder and filename become a target only inside the
    /// vault and only as one plain name; anything else is refused, never
    /// repaired into a path the person did not choose.
    #[test]
    fn a_target_is_composed_only_inside_the_vault() {
        assert_eq!(
            compose_target("10-notes", "10-notes/knowledge", "taxes.md").as_deref(),
            Ok("10-notes/knowledge/taxes.md")
        );
        assert_eq!(
            compose_target("10-notes", "10-notes", "taxes.md").as_deref(),
            Ok("10-notes/taxes.md")
        );
        for (folder, name) in [
            ("10-notes-old", "a.md"),
            ("30-work", "a.md"),
            ("10-notes/../30-work", "a.md"),
            ("10-notes/", "a.md"),
            ("10-notes", "../a.md"),
            ("10-notes", "a/b.md"),
            ("10-notes", "a\\b.md"),
            ("10-notes", ".."),
            ("10-notes", ""),
            ("10-notes", " a.md"),
            ("10-notes", "a\n.md"),
        ] {
            assert!(
                compose_target("10-notes", folder, name).is_err(),
                "{folder} + {name}"
            );
        }
    }

    /// A choice's revision moves with the file it is about, with its
    /// target and with the row's cells; the checklist's moves with the
    /// README, with every workspace entry and with every row's target, and
    /// with nothing else.
    #[test]
    fn revisions_move_only_with_what_they_bind() {
        let at = |stamp: Option<&str>, target: Option<&str>| {
            row_revision("workspace/a.md", "artifacts/a.md", "", stamp, target)
        };
        let row = at(Some("file:4:1"), Some("digest:x"));
        assert_eq!(row, at(Some("file:4:1"), Some("digest:x")));
        assert_ne!(row, at(Some("file:5:2"), Some("digest:x")));
        assert_ne!(row, at(None, Some("digest:x")));
        assert_ne!(row, at(Some("file:4:1"), Some("absent")), "the target went");
        assert_ne!(row, at(Some("file:4:1"), Some("digest:y")), "replaced");
        assert_ne!(
            row,
            row_revision(
                "workspace/a.md",
                "artifacts/b.md",
                "",
                Some("file:4:1"),
                Some("digest:x")
            )
        );

        let mut stamps = BTreeMap::from([("workspace/a.md".to_owned(), "file:4:1".to_owned())]);
        let mut targets = BTreeMap::from([("artifacts/a.md".to_owned(), "digest:x".to_owned())]);
        let before = snapshot_revision("# r\n", &stamps, &targets);
        assert_eq!(before, snapshot_revision("# r\n", &stamps, &targets));
        assert_ne!(before, snapshot_revision("# r2\n", &stamps, &targets));
        targets.insert("artifacts/a.md".to_owned(), "absent".to_owned());
        assert_ne!(before, snapshot_revision("# r\n", &stamps, &targets));
        targets.insert("artifacts/a.md".to_owned(), "digest:x".to_owned());
        stamps.insert("workspace/.draft.md".to_owned(), "file:0:2".to_owned());
        assert_ne!(
            before,
            snapshot_revision("# r\n", &stamps, &targets),
            "a hidden file"
        );
    }

    fn choice(revision: &str, promote: bool) -> ChoiceVm {
        ChoiceVm {
            revision: revision.to_owned(),
            promote,
            target: "artifacts/a.md".to_owned(),
        }
    }

    fn item(revision: &str, refused: bool) -> ItemFact {
        ItemFact {
            revision: revision.to_owned(),
            refused,
        }
    }

    fn intent(choices: Vec<ChoiceVm>, notes: Vec<NoteIntentVm>) -> PanelIntentVm {
        PanelIntentVm { choices, notes }
    }

    fn held(read: Option<&str>, copy: Option<&str>, consent: Option<&str>) -> NoteIntentVm {
        NoteIntentVm {
            path: "artifacts/knowledge/n.md".to_owned(),
            read: read.map(str::to_owned),
            copy: copy.map(str::to_owned),
            consent: consent.map(str::to_owned),
        }
    }

    fn note(revision: Option<&str>, copy: Option<&str>) -> NoteFact {
        NoteFact {
            path: "artifacts/knowledge/n.md".to_owned(),
            revision: revision.map(str::to_owned),
            copy: copy.map(str::to_owned),
        }
    }

    /// R234 (R95P2-06): a choice holds only at the revision it was made on;
    /// a refused item keeps only a skip; the last choice wins; complete
    /// only when every item has one.
    #[test]
    fn a_choice_holds_only_for_the_item_it_was_made_on() {
        let items = [item("a", false), item("b", true)];
        let decided = decisions(
            &items,
            &[],
            &intent(
                vec![
                    choice("a", false),
                    choice("a", true),
                    choice("b", true),
                    choice("gone", false),
                ],
                vec![],
            ),
        );
        assert_eq!(decided.intent.choices, [choice("a", true)]);
        assert!(!decided.complete, "b's promotion is not offered");

        let decided = decisions(
            &items,
            &[],
            &intent(vec![choice("a", true), choice("b", false)], vec![]),
        );
        assert!(decided.complete);
        let changed = [item("a2", false), item("b", true)];
        let decided = decisions(&changed, &[], &decided.intent);
        assert_eq!(decided.intent.choices, [choice("b", false)]);
        assert!(!decided.complete, "the changed row lost its choice");
        assert!(decisions(&[], &[], &PanelIntentVm::default()).complete);
    }

    /// R234 (R95P2-06): consent holds only to the candidate version read
    /// and shown; a newer candidate makes the read stale and drops the
    /// consent; a copy read is current only while the copy is that version.
    #[test]
    fn consent_holds_only_to_the_version_read_and_shown() {
        let notes = [note(Some("v1"), Some("c1"))];
        let consented = decisions(
            &[],
            &notes,
            &intent(vec![], vec![held(Some("v1"), Some("c1"), Some("v1"))]),
        );
        assert_eq!(
            consented.notes[0],
            NoteDecision {
                path: "artifacts/knowledge/n.md".to_owned(),
                candidate_read: ReadState::Current,
                copy_read: ReadState::Current,
                consented: true,
            }
        );
        let newer = decisions(&[], &[note(Some("v2"), Some("c2"))], &consented.intent);
        assert_eq!(
            (
                newer.notes[0].candidate_read,
                newer.notes[0].copy_read,
                newer.notes[0].consented
            ),
            (ReadState::Stale, ReadState::Stale, false)
        );
        assert_eq!(newer.intent.notes, [held(Some("v1"), Some("c1"), None)]);
        let unread = decisions(
            &[],
            &notes,
            &intent(vec![], vec![held(None, None, Some("v1"))]),
        );
        assert!(!unread.notes[0].consented, "consent without a read");
        let other = decisions(
            &[],
            &notes,
            &intent(vec![], vec![held(Some("v1"), None, Some("v0"))]),
        );
        assert!(!other.notes[0].consented, "consent to another version");
        let unreadable = decisions(
            &[],
            &[note(None, None)],
            &intent(vec![], vec![held(Some("v1"), Some("c1"), Some("v1"))]),
        );
        assert_eq!(unreadable.notes[0].candidate_read, ReadState::Stale);
        assert!(!unreadable.notes[0].consented);
    }

    /// R234 (R95P2-07): the revisions, the decisions and the reader bounds
    /// every mirror of this module must agree on (the text test's verdicts
    /// are the session runtime's, `keeper-agent`'s `promote::offer`) — `promote-vectors.json`, which `dev/mock-shell.ts`'s test
    /// loads as well, its digests computed independently of both.
    #[test]
    fn every_promote_vector_matches() {
        let vectors: serde_json::Value =
            serde_json::from_str(include_str!("promote-vectors.json")).expect("vectors");
        let text = |value: &serde_json::Value| value.as_str().map(str::to_owned);
        let rows = vectors["rowRevision"].as_array().expect("rows");
        for row in rows {
            assert_eq!(
                Some(row_revision(
                    row["source"].as_str().expect("source"),
                    row["target"].as_str().expect("target"),
                    row["note"].as_str().expect("note"),
                    row["stamp"].as_str(),
                    row["targetFact"].as_str(),
                )),
                text(&row["revision"]),
                "{row}"
            );
        }
        for snapshot in vectors["snapshotRevision"].as_array().expect("snapshots") {
            let map = |key: &str| -> BTreeMap<String, String> {
                serde_json::from_value(snapshot[key].clone()).expect("map")
            };
            assert_eq!(
                Some(snapshot_revision(
                    snapshot["readme"].as_str().expect("readme"),
                    &map("stamps"),
                    &map("targets"),
                )),
                text(&snapshot["revision"]),
                "{snapshot}"
            );
        }
        for copy in vectors["copyRevision"].as_array().expect("copies") {
            assert_eq!(
                Some(copy_revision(
                    copy["target"].as_str().expect("target"),
                    copy["text"].as_str().expect("text"),
                )),
                text(&copy["revision"]),
                "{copy}"
            );
        }
        for case in vectors["decisions"].as_array().expect("decisions") {
            let items: Vec<ItemFact> =
                serde_json::from_value(case["items"].clone()).expect("items");
            let notes: Vec<NoteFact> =
                serde_json::from_value(case["notes"].clone()).expect("notes");
            let intent: PanelIntentVm =
                serde_json::from_value(case["intent"].clone()).expect("intent");
            let expected: Decisions =
                serde_json::from_value(case["expected"].clone()).expect("expected");
            assert_eq!(decisions(&items, &notes, &intent), expected, "{case}");
        }
        assert_eq!(
            vectors["maxNoteBytes"].as_u64(),
            Some(crate::agents::knowledge::MAX_NOTE_BYTES as u64)
        );
        assert_eq!(
            vectors["maxReviewedBytes"].as_u64(),
            Some(crate::agents::knowledge::MAX_REVIEWED_BYTES as u64)
        );
    }
}
