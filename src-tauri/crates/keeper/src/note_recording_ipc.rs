//! Recording from a note (story 88.9): a `record = "new"` block's widget
//! starts a session linked to its note, and names that session in its own
//! fence the moment the start answers (an ordinary editor edit, saved at once).
//!
//! While it records, the note's `tags` carry `recording` and
//! `recording/<this Mac>`; when it stops, the tags go. keeper never edits the
//! note's body for a recording: every write here is a frontmatter change
//! through [`crate::notes_ipc::amend_block`], safe under an editor that has the
//! note open and is being typed in. A stopped session — finalized, failed, or
//! recovered after a crash — gets the ordinary stub unless its note still
//! names it, on disk or in an open editor, so a recording is never left
//! without a note. Tags a crash left behind are swept whenever nothing
//! records here.
//!
//! The texts are `keeper_core::notes::note_recording`'s; this module reads
//! and writes them, and holds which note the one live session belongs to.

use std::sync::Mutex;
use std::time::{Duration, Instant};

use keeper_core::notes::media_block::{self, Source};
use keeper_core::notes::note_recording;
use keeper_core::notes::NotesError;
use keeper_core::recording::{LinkedNote, SessionManifest};
use keeper_core::vm::IpcError;
use serde::Serialize;

use crate::notes_vault::{self, Vault};
use crate::transcribe_ipc::refused;

/// The note the live session was started from, and the session. One at a
/// time, because keeper records one session at a time.
static ACTIVE: Mutex<Option<Active>> = Mutex::new(None);

/// Held while a start tags its note and while the sweep untags stale ones,
/// so the sweep can never take the tags a start has just added.
static TAGGING: Mutex<()> = Mutex::new(());

/// How long the startup recovery waits for the notes registry to list and
/// index the vaults: the pass runs beside the registry's first build.
pub(crate) const VAULT_WAIT: Duration = Duration::from_secs(30);

#[derive(Debug, Clone)]
struct Active {
    note: LinkedNote,
    session_id: String,
}

fn active() -> std::sync::MutexGuard<'static, Option<Active>> {
    ACTIVE
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

fn tagging() -> std::sync::MutexGuard<'static, ()> {
    TAGGING
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// The note that is recording, as a block's widget shows it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RecordingLinkedNoteVm {
    pub profile_id: String,
    /// Relative to its vault.
    pub path: String,
    /// The session recording into it: the block naming it is the live one.
    pub session_id: String,
    /// The note's title as the notes index has it; its file name when the
    /// index has no entry for it.
    pub title: String,
}

/// This Mac as the tag names it: its host name, slugged as a device name is
/// everywhere else. The host name rather than the sync engine's label,
/// because the startup recovery may run before the engine opens and must
/// remove the tag the start added.
fn device() -> String {
    keeper_core::org_account::layout::device_slug(&keeper_core::config::read_host_label())
}

/// The vault `note` is in, and that the note is there: what a start from a
/// note checks before anything is created.
pub(crate) fn check_start(note: &LinkedNote) -> Result<(), IpcError> {
    let vault = notes_vault::vault(&note.profile_id)
        .ok_or_else(|| refused("This note is not in an open vault, so it cannot record."))?;
    notes_vault::read_note(&vault, &note.path)
        .map(|_| ())
        .map_err(|_| refused("This note is not on this Mac any more, so it cannot record."))
}

/// Session `session_id`, linked to `note`, has started: hold it, and tag the
/// note. Best-effort — the recording is already running, and a note that
/// could not be tagged is a log line.
pub(crate) fn started(note: LinkedNote, session_id: String) {
    let _tagging = tagging();
    if let Some(vault) = notes_vault::vault(&note.profile_id) {
        let device = device();
        match crate::notes_ipc::amend_block(&vault, &note.path, |text| {
            Some(note_recording::with_recording_tags(text, &device))
        }) {
            Ok(_) => tracing::info!(
                profile = %note.profile_id,
                "note recording: the note is tagged as recording"
            ),
            Err(error) => tracing::warn!(
                %error,
                profile = %note.profile_id,
                "note recording: the note could not be tagged; the recording is unaffected"
            ),
        }
    }
    *active() = Some(Active { note, session_id });
}

/// What finishing a session did about its note.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum NoteFinish {
    /// Not started from a note: the ordinary stub is the session's note.
    NotLinked,
    /// The note still names the session: it is the session's note, no stub.
    Done,
    /// The note is gone, or no longer names the session: write the ordinary
    /// stub instead.
    Stub,
}

/// The vault `profile_id`, waiting up to `wait` for the registry to list it.
fn vault_within(profile_id: &str, wait: Duration) -> Option<Vault> {
    let deadline = Instant::now() + wait;
    loop {
        if let Some(vault) = notes_vault::vault(profile_id) {
            return Some(vault);
        }
        if Instant::now() >= deadline {
            return None;
        }
        std::thread::sleep(Duration::from_millis(500));
    }
}

/// A session has stopped — finalized, failed, or recovered after a crash:
/// take its note's tags away, and say whether the note still names it (no
/// stub) or not (the ordinary stub, a failed session's too).
/// `wait_for_vault` is the startup recovery's: its pass can run before the
/// notes registry lists the vault.
pub(crate) fn finish(manifest: &SessionManifest, wait_for_vault: bool) -> NoteFinish {
    let Some(meta) = manifest.meta.as_ref() else {
        return NoteFinish::NotLinked;
    };
    let Some(note) = meta.linked_note.as_ref() else {
        return NoteFinish::NotLinked;
    };
    // A session with no identity has no stub either (`stub_session_id`), and
    // no block can name it; nor can it be the one held.
    let Some(session_id) = meta.session_id.as_deref() else {
        return NoteFinish::Stub;
    };
    release(session_id);
    let wait = if wait_for_vault {
        VAULT_WAIT
    } else {
        Duration::ZERO
    };
    let Some(vault) = vault_within(&note.profile_id, wait) else {
        tracing::warn!(
            profile = %note.profile_id,
            "note recording: the note's vault is not open, so the recording gets a stub instead"
        );
        return NoteFinish::Stub;
    };
    finish_in(&vault, note, session_id, &device())
}

/// Let go of `session_id` if it is the one held — and only then: a session
/// started in the same note while this one's finalize runs is held on.
fn release(session_id: &str) {
    let mut active = active();
    if active
        .as_ref()
        .is_some_and(|held| held.session_id == session_id)
    {
        *active = None;
    }
}

/// [`finish`] once the vault is found. The tags stay while another session
/// records in the same note — it started there after this one stopped.
fn finish_in(vault: &Vault, note: &LinkedNote, session_id: &str, device: &str) -> NoteFinish {
    {
        let _tagging = tagging();
        let recording_here = active().as_ref().is_some_and(|held| held.note == *note);
        if !recording_here {
            match crate::notes_ipc::amend_block(vault, &note.path, |text| {
                Some(note_recording::without_recording_tags(text, device))
            }) {
                Ok(_) | Err(NotesError::NotFound(_)) => {}
                Err(error) => tracing::warn!(
                    %error,
                    profile = %note.profile_id,
                    "note recording: the note's recording tags could not be removed; the sweep will try again"
                ),
            }
        }
    }
    let disk = notes_vault::read_note(vault, &note.path).ok();
    let live = crate::notes_ipc::live_texts(&vault.id, &note.path);
    if note_recording::names_session(disk.as_deref(), live.iter().map(String::as_str), session_id) {
        tracing::info!(
            profile = %note.profile_id,
            "note recording: the note that recorded names the recording, so it needs no stub"
        );
        NoteFinish::Done
    } else {
        tracing::info!(
            profile = %note.profile_id,
            gone = disk.is_none(),
            "note recording: the note no longer names the recording, so it gets a stub"
        );
        NoteFinish::Stub
    }
}

/// Untag every note in an open vault still tagged as recording on this Mac,
/// when nothing records here: what a crash, a failed untag or a quit left
/// behind. Runs at launch (after up to `wait` for the vaults to be listed and
/// indexed), before each start and after each stop. One log line per note.
pub(crate) fn sweep_stale_tags(wait: Duration) {
    let deadline = Instant::now() + wait;
    loop {
        let vaults = notes_vault::vaults();
        let ready = !vaults.is_empty()
            && vaults
                .iter()
                .all(|vault| notes_vault::is_indexed(&vault.id));
        if ready || Instant::now() >= deadline {
            break;
        }
        std::thread::sleep(Duration::from_millis(500));
    }
    let _tagging = tagging();
    if active().is_some() {
        return;
    }
    let device = device();
    for vault in notes_vault::vaults() {
        let Some(snapshot) = notes_vault::snapshot(&vault.id) else {
            continue;
        };
        for entry in note_recording::tagged_on(snapshot.entries(), &device) {
            match crate::notes_ipc::amend_block(&vault, &entry.path, |text| {
                Some(note_recording::without_recording_tags(text, &device))
            }) {
                Ok(true) => tracing::info!(
                    profile = %vault.id,
                    note = %entry.id,
                    "note recording: a note left tagged as recording on this Mac is untagged"
                ),
                Ok(false) => {}
                Err(error) => tracing::warn!(
                    %error,
                    profile = %vault.id,
                    note = %entry.id,
                    "note recording: a note left tagged as recording on this Mac could not be untagged"
                ),
            }
        }
    }
}

/// The note the live session is recording in, or `None` — what a block asks
/// to know whether it may start, or is the one recording.
#[tauri::command]
pub fn recording_linked_note() -> Option<RecordingLinkedNoteVm> {
    let held = active().clone()?;
    let indexed = notes_vault::snapshot(&held.note.profile_id).and_then(|snapshot| {
        snapshot
            .by_path(&held.note.path)
            .map(|entry| entry.title.clone())
    });
    let title = note_title(indexed.as_deref(), &held.note.path);
    Some(RecordingLinkedNoteVm {
        profile_id: held.note.profile_id,
        path: held.note.path,
        session_id: held.session_id,
        title,
    })
}

/// A note's title as the notes index has it; its file name when the index
/// has none.
fn note_title(indexed: Option<&str>, path: &str) -> String {
    indexed
        .filter(|title| !title.trim().is_empty())
        .map(str::to_owned)
        .unwrap_or_else(|| {
            path.rsplit('/')
                .next()
                .unwrap_or(path)
                .trim_end_matches(".md")
                .to_owned()
        })
}

/// A note naming a recording, as the removal's dialog and answer list it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RecordingNoteRefVm {
    pub vault_id: String,
    /// Relative to its vault.
    pub path: String,
    pub title: String,
}

/// A note keeper could not take a removed recording out of: it still names
/// it, and the person is told which and why.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RecordingNoteFailureVm {
    pub vault_id: String,
    pub path: String,
    pub title: String,
    pub error: String,
}

/// What forgetting a removed recording did to the notes in open vaults.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct NotesForgotten {
    /// Notes keeper wrote: their recording keys, or their blocks.
    pub changed: Vec<RecordingNoteRefVm>,
    /// Notes a live editor has open, whose blocks that editor removes.
    pub open: Vec<RecordingNoteRefVm>,
    /// Notes that could not be changed and still name the recording.
    pub failed: Vec<RecordingNoteFailureVm>,
}

/// Whether `note` names the recording `session_id`: its `session` key, or a
/// media block.
fn names_recording(note: &str, session_id: &str) -> bool {
    note_recording::without_session_keys(note, session_id).is_some()
        || media_block::session_ids(note)
            .iter()
            .any(|id| id.trim() == session_id)
}

/// Every note in an open vault naming the recording `session_id`, with its
/// vault. Each note is read once; one that cannot be read names nothing.
fn notes_naming_in(session_id: &str) -> Vec<(Vault, RecordingNoteRefVm)> {
    let mut found = Vec::new();
    for vault in notes_vault::vaults() {
        let Some(snapshot) = notes_vault::snapshot(&vault.id) else {
            continue;
        };
        for entry in snapshot.entries() {
            let Ok(text) = notes_vault::read_note(&vault, &entry.path) else {
                continue;
            };
            if text.contains(session_id) && names_recording(&text, session_id) {
                found.push((
                    vault.clone(),
                    RecordingNoteRefVm {
                        vault_id: vault.id.clone(),
                        path: entry.path.clone(),
                        title: note_title(Some(&entry.title), &entry.path),
                    },
                ));
            }
        }
    }
    found
}

/// The notes naming the recording `session_id`: what its removal's dialog
/// says will lose the recording.
#[cfg_attr(not(desktop), allow(dead_code))]
pub(crate) fn notes_naming(session_id: &str) -> Vec<RecordingNoteRefVm> {
    notes_naming_in(session_id)
        .into_iter()
        .map(|(_, note)| note)
        .collect()
}

/// Take the removed recording `session_id` out of every note in an open
/// vault ([`crate::notes_ipc::forget_session`]). A note that cannot be
/// written is reported, and logged: the recording is already gone.
#[cfg_attr(not(desktop), allow(dead_code))]
pub(crate) fn forget_session(session_id: &str) -> NotesForgotten {
    let mut forgotten = NotesForgotten::default();
    for (vault, note) in notes_naming_in(session_id) {
        match crate::notes_ipc::forget_session(&vault, &note.path, session_id) {
            Ok(done) => {
                if done.open {
                    forgotten.open.push(note.clone());
                }
                if done.changed {
                    forgotten.changed.push(note);
                }
            }
            Err(error) => {
                tracing::warn!(
                    %error,
                    profile = %vault.id,
                    "recording removal: a note naming the removed recording could not be changed"
                );
                forgotten.failed.push(RecordingNoteFailureVm {
                    vault_id: note.vault_id,
                    path: note.path,
                    title: note.title,
                    error: error.to_string(),
                });
            }
        }
    }
    forgotten
}

/// What a media block's body is, as far as recording goes: whether it
/// records here (`record = "new"`), the session it names, if any, and
/// whether that session is the one recording now (`live`) — into this very
/// note (`here`) — for the widget to draw the recorder, the live view or the
/// player in one round trip, without TypeScript reading a key.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MediaBlockRecordingVm {
    pub records: bool,
    pub session: Option<String>,
    pub live: bool,
    pub here: bool,
}

/// `source` as [`MediaBlockRecordingVm`] says it, for the block in the note
/// `path` of vault `profile_id` (`None` for a block outside a note).
#[tauri::command]
pub fn media_block_recording(
    source: String,
    profile_id: String,
    path: Option<String>,
) -> MediaBlockRecordingVm {
    let (records, session) = match media_block::parse(&source).map(|block| block.source) {
        Ok(Source::Record) => (true, None),
        Ok(Source::Session(id)) => (false, Some(id.trim().to_owned())),
        _ => (false, None),
    };
    let held = active().clone();
    let live = held
        .as_ref()
        .zip(session.as_deref())
        .is_some_and(|(held, session)| held.session_id == session);
    let here = live
        && held.as_ref().is_some_and(|held| {
            held.note.profile_id == profile_id && Some(&held.note.path) == path.as_ref()
        });
    MediaBlockRecordingVm {
        records,
        session,
        live,
        here,
    }
}

/// The body of the `record = "new"` block `source`, naming `session_id`: what
/// the widget that pressed Start splices over its own fence once the start
/// has answered.
#[tauri::command]
pub fn media_block_record_started(source: String, session_id: String) -> Result<String, IpcError> {
    note_recording::finish_record_body(&source, &session_id)
        .map_err(|refusal| refused(refusal.to_string()))
}

/// The note body `text` without the media blocks naming the removed
/// recording `session_id` (and the words under them), or `None` when none
/// does: what an open note's buffer becomes when that recording is removed,
/// whichever view it is in.
#[tauri::command]
pub fn media_block_without_session(text: String, session_id: String) -> Option<String> {
    media_block::without_session_blocks(&text, &session_id)
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use keeper_sync::exclude::ExcludeSet;
    use keeper_sync::profile::NotesConfig;

    use super::*;

    const ID: &str = "01KYDKP6SN2HR4SJBJ9JTBVC2Z-01KYDM0000000000000000000A";
    const DEVICE: &str = "hesperia";
    const NOTE: &str = "meetings/standup.md";

    fn vault(tag: &str) -> Vault {
        let root = std::env::temp_dir().join(format!(
            "keeper-note-recording-{tag}-{}-{}",
            std::process::id(),
            notes_vault::now_ms()
        ));
        std::fs::create_dir_all(root.join("meetings")).expect("vault root");
        let root = root.canonicalize().expect("canonical vault root");
        Vault {
            id: format!("01VAULT-{tag}"),
            name: "mind".to_owned(),
            root: root.clone(),
            local_path: root,
            config: NotesConfig::default(),
            excludes: Arc::new(ExcludeSet::new(&[]).expect("built-in excludes")),
        }
    }

    fn link(vault: &Vault) -> LinkedNote {
        LinkedNote {
            profile_id: vault.id.clone(),
            path: NOTE.to_owned(),
        }
    }

    fn recording_note(body: &str) -> String {
        note_recording::with_recording_tags(&format!("---\ntitle: Standup\n---\n{body}"), DEVICE)
    }

    #[test]
    fn a_stopped_recording_whose_block_names_it_loses_its_tags_and_needs_no_stub() {
        let vault = vault("named");
        let body = format!("Agenda.\n\n```keeper-media\n# mine\nsession = \"{ID}\"\n```\n");
        std::fs::write(vault.root.join(NOTE), recording_note(&body)).expect("note");

        assert_eq!(
            finish_in(&vault, &link(&vault), ID, DEVICE),
            NoteFinish::Done
        );
        assert_eq!(
            notes_vault::read_note(&vault, NOTE).expect("note"),
            format!("---\ntitle: Standup\n---\n{body}"),
            "only the tags went; the body is the note's"
        );
    }

    #[test]
    fn a_note_deleted_while_it_recorded_falls_back_to_the_stub() {
        let vault = vault("deleted");
        assert_eq!(
            finish_in(&vault, &link(&vault), ID, DEVICE),
            NoteFinish::Stub
        );
        assert!(!vault.root.join(NOTE).exists(), "nothing is recreated");
    }

    #[test]
    fn a_note_whose_block_was_removed_loses_its_tags_and_the_recording_gets_a_stub() {
        let vault = vault("no-block");
        std::fs::write(vault.root.join(NOTE), recording_note("Only words.\n")).expect("note");

        assert_eq!(
            finish_in(&vault, &link(&vault), ID, DEVICE),
            NoteFinish::Stub
        );
        assert_eq!(
            notes_vault::read_note(&vault, NOTE).expect("note"),
            "---\ntitle: Standup\n---\nOnly words.\n"
        );
    }

    #[test]
    fn a_record_block_is_classified_and_named_by_rust() {
        let vault = vault("classified");
        let at = || Some(NOTE.to_owned());
        assert_eq!(
            media_block_recording("record = \"new\"\n".to_owned(), vault.id.clone(), at()),
            MediaBlockRecordingVm {
                records: true,
                session: None,
                live: false,
                here: false,
            }
        );
        assert_eq!(
            media_block_recording(format!("session = \"{ID}\"\n"), vault.id.clone(), at())
                .session
                .as_deref(),
            Some(ID)
        );
        assert_eq!(
            media_block_record_started("# c\nrecord = \"new\"\n".to_owned(), ID.to_owned())
                .expect("named"),
            format!("# c\nsession = \"{ID}\"\n")
        );
    }

    /// One test, because `ACTIVE` is the process's: a session started in the
    /// same note while the one before it finalizes keeps its hold and its
    /// tags, and its block is the live one there and nowhere else.
    #[test]
    fn a_newer_session_in_the_same_note_survives_the_older_ones_finish() {
        const NEWER: &str = "01KYDKP6SN2HR4SJBJ9JTBVC2Z-01KYDM0000000000000000000B";
        let vault = vault("newer");
        let tagged = recording_note(&format!("```keeper-media\nsession = \"{ID}\"\n```\n"));
        std::fs::write(vault.root.join(NOTE), &tagged).expect("note");
        *active() = Some(Active {
            note: link(&vault),
            session_id: NEWER.to_owned(),
        });
        let block = format!("session = \"{NEWER}\"\n");
        let classify = |path: &str| {
            media_block_recording(block.clone(), vault.id.clone(), Some(path.to_owned()))
        };
        assert!(
            classify(NOTE).here,
            "the newer session's block is the live one"
        );
        let elsewhere = classify("meetings/retro.md");
        assert!(elsewhere.live && !elsewhere.here);

        release(ID);
        assert_eq!(
            finish_in(&vault, &link(&vault), ID, DEVICE),
            NoteFinish::Done
        );

        assert!(
            active()
                .as_ref()
                .is_some_and(|held| held.session_id == NEWER),
            "the older session's finish let go of nothing it did not hold"
        );
        assert_eq!(
            notes_vault::read_note(&vault, NOTE).expect("note"),
            tagged,
            "the tags stay while the newer session records here"
        );
        release(NEWER);
        assert!(active().is_none());
    }
}
