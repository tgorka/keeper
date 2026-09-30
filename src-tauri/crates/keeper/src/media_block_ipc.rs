//! The `keeper-media` block's command surface (Epic 88, AD-351…AD-357): a
//! call site over `keeper_core::notes::media_block` and
//! `keeper_core::transcription::media`.
//!
//! The editor hands every command a block's body verbatim, or a note's text,
//! and gets back a view model or text to splice. Nothing here reads a key:
//! this module answers the world's questions the core asks — which drive a
//! note is in and how a path joins under it (`browse::resolve`, AD-65), where
//! the recordings index says a recording is now — and does the reads and the
//! writes (AD-55/AD-56).

use std::path::{Path, PathBuf};
use std::sync::Arc;

use keeper_core::notes::frontmatter::Frontmatter;
use keeper_core::notes::media_block::{
    self, Adoption, BlockRefusal, ClipWindow, LineEditVm, MarkerEditReq, MediaAdoptionVm,
    MediaClipVm, MediaMarkerHitVm, MediaPickReq, Source,
};
use keeper_core::notes::recording_note::{self, SESSION_KEY};
use keeper_core::platform::Platform;
use keeper_core::recording::SessionManifest;
use keeper_core::transcription::media::{self, MediaLookup, RecordingPlace};
use keeper_core::transcription::plan::{transcript_paths_for, SESSION_TRANSCRIPT_JSON};
use keeper_core::transcription::vm::MediaBlockVm;
use keeper_core::transcription::Transcript;
use keeper_core::vm::{IpcError, RecordingNoteTargetKind};
use tauri::State;

use crate::ipc::AppState;
use crate::notes_vault;
use crate::transcribe_ipc::{read_transcript, refused, synced_folders};

/// What a block in a note outside every synced folder says (UX-DR124).
const NO_DRIVE_SENTENCE: &str = "keeper draws media only in notes inside a synced folder.";

/// Where the recordings index says the recording `session_id` is now, the
/// answer `recording_note_targets` and `keeper-recording://` give (Story
/// 42.4), so a block and its player cannot disagree about it.
pub(crate) fn recording_place(
    platform: &Arc<dyn Platform>,
    session_id: &str,
) -> Option<RecordingPlace> {
    let data_dir = platform.data_dir().ok()?;
    let destination_root = crate::ipc::effective_destination_dir(&data_dir, platform);
    let targets =
        crate::ipc::recording_note_targets_in(&data_dir, &destination_root, session_id).ok()??;
    let folder = targets
        .into_iter()
        .find(|target| target.kind == RecordingNoteTargetKind::Folder)?;
    Some(RecordingPlace {
        session_id: session_id.to_owned(),
        folder: PathBuf::from(folder.absolute_path),
        relative_folder: folder.relative_path,
    })
}

/// The shell's answers to [`MediaLookup`], for a note in the drive
/// `profile_id` — or in none.
struct ShellLookup {
    platform: Arc<dyn Platform>,
    drive_root: Option<PathBuf>,
    folders: Vec<(String, PathBuf)>,
}

impl ShellLookup {
    fn new(platform: &Arc<dyn Platform>, profile_id: Option<&str>) -> Self {
        // Canonical, as `browse::resolve` answers, so a drive reached through
        // a symlink still recognises its own files.
        let folders: Vec<(String, PathBuf)> = synced_folders(platform)
            .into_iter()
            .map(|(id, root)| {
                let root = root.canonicalize().unwrap_or(root);
                (id, root)
            })
            .collect();
        let drive_root = profile_id.and_then(|profile_id| {
            folders
                .iter()
                .find(|(id, _)| id == profile_id)
                .map(|(_, root)| root.clone())
        });
        Self {
            platform: Arc::clone(platform),
            drive_root,
            folders,
        }
    }

    /// `absolute` as a path relative to this note's drive, `/`-joined.
    fn drive_relative(&self, absolute: &Path) -> Option<String> {
        let inside = absolute.strip_prefix(self.drive_root.as_ref()?).ok()?;
        let segments: Option<Vec<&str>> = inside
            .components()
            .map(|component| match component {
                std::path::Component::Normal(name) => name.to_str(),
                _ => None,
            })
            .collect();
        Some(segments?.join("/"))
    }
}

impl MediaLookup for ShellLookup {
    fn join(&self, path: &str) -> Result<PathBuf, String> {
        let root = self
            .drive_root
            .as_ref()
            .ok_or_else(|| NO_DRIVE_SENTENCE.to_owned())?;
        match keeper_sync::browse::resolve(root, path) {
            Ok(Some(joined)) => Ok(joined),
            Ok(None) => Err(format!("{path} is not in this drive.")),
            Err(refusal) => Err(format!("{refusal}.")),
        }
    }

    fn recording(&self, session_id: &str) -> Option<RecordingPlace> {
        recording_place(&self.platform, session_id)
    }

    fn folders(&self) -> &[(String, PathBuf)] {
        &self.folders
    }
}

/// The transcript a read block plays, when it has one: the recording's
/// `transcript.json`, the named transcript, or a single part's own.
fn transcript_of(
    block: &media_block::Block,
    lookup: &ShellLookup,
) -> Result<Option<Transcript>, String> {
    let json = match &block.source {
        Source::Session(id) => lookup
            .recording(id)
            .map(|place| transcript_paths_for(&place.folder, true).0),
        Source::Transcript(path) => Some(lookup.join(path)?),
        Source::Parts(parts) => match &parts[..] {
            [part] => Some(transcript_paths_for(&lookup.join(&part.file)?, false).0),
            _ => None,
        },
        Source::Src(_) => return Err(BlockRefusal::NestedSrc.to_string()),
        Source::Record => None,
    };
    json.filter(|json| json.is_file())
        .map(|json| read_transcript(&json))
        .transpose()
}

/// A media block's body, resolved: what it plays and the transcript's lines
/// inside its window (AD-353, AD-359). `profile_id` is the drive holding the
/// note; without one, a block naming a path says keeper cannot draw it here.
/// Every refusal is the sentence the block shows above its source.
#[tauri::command]
pub async fn media_block_resolve(
    state: State<'_, AppState>,
    profile_id: Option<String>,
    source: String,
) -> Result<MediaBlockVm, IpcError> {
    let platform = Arc::clone(&state.platform);
    crate::ipc::off_async_runtime(move || {
        let lookup = ShellLookup::new(&platform, profile_id.as_deref());
        media::resolve_block(&source, &lookup).map_err(refused)
    })
    .await?
}

/// The body with one marker added, renamed or removed, every other byte as
/// it was (AD-354) — for the editor to splice over the block's range.
#[tauri::command]
pub fn media_block_edit(source: String, edit: MarkerEditReq) -> Result<String, IpcError> {
    media_block::edit(&source, &edit).map_err(|refusal| refused(refusal.to_string()))
}

/// A clip of the block `source` for `[from, to)`, as typed (AD-355). Called
/// as the person types, so a bad time is a sentence and nothing else.
#[tauri::command]
pub async fn media_block_clip(
    state: State<'_, AppState>,
    profile_id: Option<String>,
    source: String,
    from: Option<String>,
    to: Option<String>,
    words: bool,
) -> Result<MediaClipVm, IpcError> {
    let platform = Arc::clone(&state.platform);
    crate::ipc::off_async_runtime(move || {
        let window = ClipWindow::parse(from.as_deref(), to.as_deref())
            .map_err(|refusal| refused(refusal.to_string()))?;
        let lookup = ShellLookup::new(&platform, profile_id.as_deref());
        // The words are a courtesy: a block whose transcript cannot be read
        // still copies, without them.
        let transcript = media::read_block(&source, &lookup)
            .and_then(|block| transcript_of(&block, &lookup))
            .unwrap_or_else(|error| {
                tracing::debug!(%error, "media block: a clip without its words");
                None
            });
        media_block::clip_block(&source, window, transcript.as_ref(), words)
            .map_err(|refusal| refused(refusal.to_string()))
    })
    .await?
}

/// A clip of the transcript at `path` from the viewer: the whole of it with
/// no window (*Copy as note embed*), or `[from, to)` (*Copy clip from
/// here…*). It names the recording by identity when it can (AD-355).
#[tauri::command]
pub async fn transcript_clip(
    state: State<'_, AppState>,
    path: String,
    from: Option<String>,
    to: Option<String>,
    words: bool,
) -> Result<MediaClipVm, IpcError> {
    let platform = Arc::clone(&state.platform);
    crate::ipc::off_async_runtime(move || {
        let window = ClipWindow::parse(from.as_deref(), to.as_deref())
            .map_err(|refusal| refused(refusal.to_string()))?;
        let path = PathBuf::from(path);
        let folders: Vec<(String, PathBuf)> = synced_folders(&platform);
        let source =
            media::clip_source(&path, &folders).map_err(|refusal| refused(refusal.to_string()))?;
        let transcript = read_transcript(&path).map_err(refused)?;
        Ok(media_block::clip_transcript(
            &source,
            window,
            &transcript,
            words,
        ))
    })
    .await?
}

/// The recordings the media blocks of `body` (a whole note) name — what the
/// attachments panel counts as in the note (AD-357).
#[tauri::command]
pub fn media_block_sources(body: String) -> Vec<String> {
    media_block::session_ids(&body)
}

/// The grammar's keys, where each may stand, what its value is and what it
/// does: what the editor offers while a person writes a block by hand.
#[tauri::command]
pub fn media_block_schema() -> media_block::MediaBlockSchemaVm {
    media_block::schema()
}

/// Why the block body `source` does not read, placed on the key or line it
/// is about; `None` when it reads.
#[tauri::command]
pub fn media_block_check(source: String) -> Option<media_block::MediaBlockProblemVm> {
    media_block::check(&source)
}

/// Where `[[note#name]]` lands in the note `body`: the media block holding a
/// marker called `name` and its time, or `None` (AD-354). A block naming a
/// `src` file is read with it.
#[tauri::command]
pub async fn media_block_find_marker(
    state: State<'_, AppState>,
    profile_id: Option<String>,
    body: String,
    name: String,
) -> Result<Option<MediaMarkerHitVm>, IpcError> {
    let platform = Arc::clone(&state.platform);
    crate::ipc::off_async_runtime(move || {
        let lookup = ShellLookup::new(&platform, profile_id.as_deref());
        let blocks: Vec<Option<media_block::Block>> = media_block::blocks(&body)
            .iter()
            .map(|found| media::read_block(&found.body, &lookup).ok())
            .collect();
        media_block::find_marker(blocks.iter().map(Option::as_ref), &name)
    })
    .await
}

/// "Play in a player" on the embed `target` at line `line` of the note
/// `body` in the vault `profile_id` (N2). In a recording note, an embed of
/// the recording's own media collapses every such embed into one block
/// naming the recording; any other media file becomes a one-part block
/// naming it relative to the drive. The editor applies the edits in one
/// transaction.
#[tauri::command]
pub async fn media_block_for_embed(
    state: State<'_, AppState>,
    profile_id: String,
    body: String,
    line: u32,
    target: String,
) -> Result<Vec<LineEditVm>, IpcError> {
    let platform = Arc::clone(&state.platform);
    crate::ipc::off_async_runtime(move || {
        let line = usize::try_from(line).map_err(|_| refused("That line is not in the note."))?;
        let target = target
            .split('|')
            .next()
            .unwrap_or_default()
            .trim()
            .to_owned();
        let (front, _) = Frontmatter::parse(&body);
        if recording_note::is_recording_note(&front) {
            let session_id = front
                .as_string(SESSION_KEY)
                .unwrap_or_default()
                .trim()
                .to_owned();
            let mut media: Vec<String> = front
                .as_list("files")
                .unwrap_or_default()
                .into_iter()
                .filter(|file| media_block::is_playable(file))
                .collect();
            if let Ok(data_dir) = platform.data_dir() {
                let root = crate::ipc::effective_destination_dir(&data_dir, &platform);
                if let Ok(Some(targets)) =
                    crate::ipc::recording_note_targets_in(&data_dir, &root, &session_id)
                {
                    media.extend(
                        targets
                            .into_iter()
                            .filter(|target| {
                                matches!(
                                    target.kind,
                                    RecordingNoteTargetKind::Video | RecordingNoteTargetKind::Audio
                                )
                            })
                            .map(|target| target.relative_path),
                    );
                }
            }
            let edits = media_block::session_embeds_to_block(&body, &session_id, line, |target| {
                media.iter().any(|file| file == target)
            });
            if !edits.is_empty() {
                return Ok(edits);
            }
        }
        let vault = notes_vault::vault(&profile_id).ok_or_else(|| refused(NO_DRIVE_SENTENCE))?;
        let (_, absolute) = crate::notes_ipc::embed_path_opt(&vault, &target).ok_or_else(|| {
            refused(format!(
                "{target}: this note embeds a file the vault does not have."
            ))
        })?;
        let name = absolute
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_default();
        if !media_block::is_playable(&name) {
            return Err(refused(format!("{name} is not an audio or video file.")));
        }
        let lookup = ShellLookup::new(&platform, Some(&profile_id));
        let relative = lookup
            .drive_relative(&absolute)
            .ok_or_else(|| refused(NO_DRIVE_SENTENCE))?;
        media_block::file_embed_to_block(&body, line, &target, &relative)
            .map(|edit| vec![edit])
            .ok_or_else(|| refused(format!("There is no embed of {target} on that line.")))
    })
    .await?
}

/// The block for what the person picked, ready to insert at the caret (N3):
/// a recording by its identity; a transcript by its path — by its
/// recording's identity when it is one; a recording's own file by the
/// recording; any other media file as one part.
#[tauri::command]
pub async fn media_block_compose(
    state: State<'_, AppState>,
    profile_id: String,
    pick: MediaPickReq,
) -> Result<String, IpcError> {
    let platform = Arc::clone(&state.platform);
    crate::ipc::off_async_runtime(move || match pick {
        MediaPickReq::Session { session_id } => {
            if recording_place(&platform, &session_id).is_none() {
                return Err(refused(media::UNKNOWN_RECORDING_SENTENCE));
            }
            Ok(media_block::session_block(&session_id))
        }
        MediaPickReq::File { relative_path } => {
            let lookup = ShellLookup::new(&platform, Some(&profile_id));
            let absolute = lookup.join(&relative_path).map_err(refused)?;
            let name = absolute
                .file_name()
                .map(|name| name.to_string_lossy().into_owned())
                .unwrap_or_default();
            let folder = if absolute.is_dir() {
                Some(absolute.as_path())
            } else {
                absolute.parent()
            };
            let session = folder
                .and_then(|folder| SessionManifest::load(folder).ok())
                .filter(|manifest| {
                    absolute.is_dir()
                        || name == SESSION_TRANSCRIPT_JSON
                        || manifest.segments.iter().any(|segment| segment.file == name)
                })
                .and_then(|manifest| manifest.meta?.session_id)
                .filter(|id| !id.trim().is_empty());
            if let Some(id) = session {
                return Ok(media_block::session_block(&id));
            }
            if name.ends_with(".json") && name.contains("transcript") {
                return Ok(media_block::transcript_block(&relative_path));
            }
            if media_block::is_playable(&name) {
                return Ok(media_block::part_block(&relative_path));
            }
            Err(refused(format!(
                "{name} is not a recording, a transcript or an audio or video file."
            )))
        }
        MediaPickReq::NewRecording => Ok(media_block::record_block()),
    })
    .await?
}

/// Rewrite every recording stub of the vault `profile_id` whose body still
/// holds its recording's per-file embeds, exactly as the stub composer wrote
/// them, into the one block a stub carries now (N5). Idempotent; every other
/// byte of a note stays; a note whose embeds were edited by hand is left
/// alone and named. `dry_run` counts without writing, for the confirmation.
#[tauri::command]
pub async fn recording_notes_adopt_media_block(
    profile_id: String,
    dry_run: bool,
) -> Result<MediaAdoptionVm, IpcError> {
    crate::ipc::off_async_runtime(move || {
        let vault = notes_vault::vault(&profile_id)
            .ok_or_else(|| refused("That vault is not open on this Mac."))?;
        let snapshot = notes_vault::snapshot(&profile_id)
            .ok_or_else(|| refused("The notes are still being read; try again in a moment."))?;
        let mut answer = MediaAdoptionVm {
            changed: 0,
            skipped: Vec::new(),
        };
        for entry in snapshot.entries() {
            if !entry.fields.contains_key(SESSION_KEY) {
                continue;
            }
            let text = notes_vault::read_note(&vault, &entry.path)
                .map_err(|error| refused(error.to_string()))?;
            match media_block::adopt(&text) {
                Adoption::Untouched => {}
                Adoption::Skipped => answer.skipped.push(entry.path.clone()),
                Adoption::Changed(adopted) => {
                    if !dry_run {
                        notes_vault::write_note(&vault, &entry.path, &adopted)
                            .map_err(|error| refused(error.to_string()))?;
                    }
                    answer.changed += 1;
                }
            }
        }
        Ok(answer)
    })
    .await?
}
