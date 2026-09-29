//! What a player plays: each media file on a timeline, the camera filmed
//! beside it, and its audio tracks — for the transcript viewer, and for a
//! media block in a note (AD-353). A file is served from a synced folder,
//! because `keeper-file://` serves the webview only from one (AD-74), or,
//! when it belongs to an indexed recording outside every synced folder, over
//! `keeper-recording://` by the recording's identity.

use std::path::{Component, Path, PathBuf};

use super::engine::{AudioTrackInfo, TrackSelect};
use super::model::{SourcePart, Transcript};
use super::plan::{
    assign_track_roles, is_media_file, plan_for_session, transcript_paths_for,
    SESSION_TRANSCRIPT_JSON,
};
use super::vm::{
    MediaAudioTrackVm, MediaBlockVm, MediaLineVm, MediaMarkerVm, MediaRef, MediaRefKind,
    MediaSpeakerVm, MediaWindowVm, TranscriptMediaPartVm, TranscriptMediaVm,
};
use crate::archive::recordings_fts::kind_for_file_name;
use crate::file_asset::is_servable_path;
use crate::notes::media_block::{self, Block, BlockRefusal, ClipSource, Source, MAX_SRC_BYTES};
use crate::recording::{
    lfs_pointer_media_size, SessionManifest, AUDIO_SEGMENT_STEM_PREFIX, CAMERA_SEGMENT_STEM_PREFIX,
    PARTIAL_SEGMENT_SUFFIX, SEGMENT_STEM_PREFIX,
};
use crate::vm::RecordingNoteTargetKind;

/// Where an indexed recording is now: its folder, and that folder relative
/// to the recordings destination root — what `recording_note_targets`
/// answers, so a Story 40.4 retitle cannot strand a block.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RecordingPlace {
    pub session_id: String,
    pub folder: PathBuf,
    /// `/`-joined; empty for a recording filed at the root.
    pub relative_folder: String,
}

/// The player's model for `transcript`, which lives in `dir`. `manifest` is
/// the session's when the transcript is a recording's: it pairs each screen
/// segment with the camera segment of the same index. `folders` are the
/// synced folders, `(profile id, local path)`. `recording` is where the
/// recording is, when the index knows it: a file in no synced folder is then
/// served by the recording's identity, and otherwise gets no reference.
pub fn transcript_media(
    transcript: &Transcript,
    dir: &Path,
    manifest: Option<&SessionManifest>,
    folders: &[(String, PathBuf)],
    recording: Option<&RecordingPlace>,
) -> TranscriptMediaVm {
    // A transcript from before parts were recorded names its one file.
    let legacy;
    let parts: &[SourcePart] = match (&transcript.source.parts[..], &transcript.source.files[..]) {
        ([], [file]) => {
            legacy = [SourcePart {
                file: file.clone(),
                offset: 0.0,
                duration: transcript.duration,
                tracks: Vec::new(),
            }];
            &legacy
        }
        (parts, _) => parts,
    };
    media_vm(
        parts
            .iter()
            .map(|part| {
                let path = dir.join(&part.file);
                TranscriptMediaPartVm {
                    file: part.file.clone(),
                    offset: part.offset,
                    duration: part.duration,
                    screen: media_ref(&path, folders, recording),
                    camera: manifest
                        .and_then(|manifest| camera_for(manifest, &part.file))
                        .map(|camera| dir.join(camera))
                        .filter(|camera| camera.is_file())
                        .and_then(|camera| media_ref(&camera, folders, recording)),
                    audio_tracks: part
                        .tracks
                        .iter()
                        .filter_map(|track| {
                            Some(MediaAudioTrackVm {
                                index: track.track?,
                                origin: track.origin,
                            })
                        })
                        .collect(),
                    here: is_here(&path),
                }
            })
            .collect(),
    )
}

/// The player's model for a recording nobody has transcribed yet, from its
/// manifest: the screen or audio segments in index order, each with its
/// camera, its tracks heard as AD-345 hears them, and its length from the
/// capture's own sample bounds when the manifest has them.
pub fn session_media(
    folder: &Path,
    manifest: &SessionManifest,
    folders: &[(String, PathBuf)],
    recording: Option<&RecordingPlace>,
) -> TranscriptMediaVm {
    let mut segments: Vec<_> = manifest
        .segments
        .iter()
        .filter(|segment| {
            (segment.file.starts_with(SEGMENT_STEM_PREFIX)
                || segment.file.starts_with(AUDIO_SEGMENT_STEM_PREFIX))
                && !segment.file.ends_with(PARTIAL_SEGMENT_SUFFIX)
        })
        .collect();
    segments.sort_by_key(|segment| segment.index);
    let devices = manifest.devices;
    let expected: Vec<AudioTrackInfo> = (0..u32::from(devices.system_audio)
        + u32::from(devices.microphone))
        .map(|index| AudioTrackInfo {
            index,
            channels: 0,
            duration_s: 0.0,
        })
        .collect();
    let audio_tracks: Vec<MediaAudioTrackVm> = assign_track_roles(&devices, &expected)
        .into_iter()
        .filter_map(|(select, origin)| match select {
            TrackSelect::Index(index) => Some(MediaAudioTrackVm { index, origin }),
            TrackSelect::MixAll => None,
        })
        .collect();
    let mut offset = 0.0;
    let parts = segments
        .into_iter()
        .map(|segment| {
            let path = folder.join(&segment.file);
            let duration = segment
                .pts_start
                .zip(segment.pts_end)
                .map(|(start, end)| end - start)
                .filter(|duration| duration.is_finite() && *duration > 0.0)
                .unwrap_or(0.0);
            let part = TranscriptMediaPartVm {
                file: segment.file.clone(),
                offset,
                duration,
                screen: media_ref(&path, folders, recording),
                camera: camera_for(manifest, &segment.file)
                    .map(|camera| folder.join(camera))
                    .filter(|camera| camera.is_file())
                    .and_then(|camera| media_ref(&camera, folders, recording)),
                audio_tracks: audio_tracks.clone(),
                here: is_here(&path),
            };
            offset += duration;
            part
        })
        .collect();
    media_vm(parts)
}

fn media_vm(parts: Vec<TranscriptMediaPartVm>) -> TranscriptMediaVm {
    TranscriptMediaVm {
        has_camera: parts.iter().any(|part| part.camera.is_some()),
        has_screen: parts.iter().any(|part| {
            part.screen
                .as_ref()
                .is_some_and(|screen| screen.kind() == MediaRefKind::Video)
        }),
        parts,
    }
}

/// Whether `path`'s bytes are on this device: the file is there and is not
/// a Git LFS pointer standing in for it.
fn is_here(path: &Path) -> bool {
    path.is_file() && lfs_pointer_media_size(path).is_none()
}

/// The camera segment the manifest lists with `file`'s index.
fn camera_for<'m>(manifest: &'m SessionManifest, file: &str) -> Option<&'m str> {
    let index = manifest
        .segments
        .iter()
        .find(|segment| segment.file == file)?
        .index;
    manifest
        .segments
        .iter()
        .find(|segment| {
            segment.index == index
                && segment.file.starts_with(CAMERA_SEGMENT_STEM_PREFIX)
                && !segment.file.ends_with(PARTIAL_SEGMENT_SUFFIX)
        })
        .map(|segment| segment.file.as_str())
}

/// Where the webview is served `path` from: the synced folder holding it
/// (the innermost, when folders nest), else — for a file directly in
/// `recording`'s folder — the recording by its identity; `None` when
/// neither serves it.
fn media_ref(
    path: &Path,
    folders: &[(String, PathBuf)],
    recording: Option<&RecordingPlace>,
) -> Option<MediaRef> {
    let name = path.file_name()?.to_str()?;
    let kind = match kind_for_file_name(name) {
        RecordingNoteTargetKind::Video => MediaRefKind::Video,
        RecordingNoteTargetKind::Audio => MediaRefKind::Audio,
        _ => return None,
    };
    if let Some((profile_id, relative_path)) = inside_folder(path, folders) {
        return is_servable_path(&relative_path).then_some(MediaRef::File {
            profile_id,
            relative_path,
            kind,
        });
    }
    let recording = recording.filter(|place| path.parent() == Some(place.folder.as_path()))?;
    Some(MediaRef::Recording {
        session_id: recording.session_id.clone(),
        relative_path: if recording.relative_folder.is_empty() {
            name.to_owned()
        } else {
            format!("{}/{name}", recording.relative_folder)
        },
        kind,
    })
}

/// The synced folder holding `path` — the innermost, when folders nest —
/// and the `/`-joined path inside it.
fn inside_folder(path: &Path, folders: &[(String, PathBuf)]) -> Option<(String, String)> {
    let (profile_id, inside) = folders
        .iter()
        .filter_map(|(id, root)| Some((id, path.strip_prefix(root).ok()?)))
        .min_by_key(|(_, inside)| inside.components().count())?;
    let mut segments = Vec::new();
    for component in inside.components() {
        match component {
            Component::Normal(segment) => segments.push(segment.to_str()?),
            _ => return None,
        }
    }
    Some((profile_id.clone(), segments.join("/")))
}

/// What a clip copied from the transcript viewer names its meeting by
/// (AD-355): the recording's identity when the transcript is a recording's
/// whose manifest carries one, else its path relative to the synced folder
/// holding it — else nothing a note could name.
pub fn clip_source(json: &Path, folders: &[(String, PathBuf)]) -> Result<ClipSource, BlockRefusal> {
    if json.file_name().and_then(|name| name.to_str()) == Some(SESSION_TRANSCRIPT_JSON) {
        let id = json
            .parent()
            .and_then(|folder| SessionManifest::load(folder).ok())
            .and_then(|manifest| manifest.meta)
            .and_then(|meta| meta.session_id)
            .filter(|id| !id.trim().is_empty());
        if let Some(id) = id {
            return Ok(ClipSource::Session(id));
        }
    }
    inside_folder(json, folders)
        .map(|(_, relative)| ClipSource::Transcript(relative))
        .ok_or(BlockRefusal::NotNameable)
}

/// What resolving a block needs from the world, answered by the shell.
pub trait MediaLookup {
    /// `path`, relative to the drive holding the note, joined under that
    /// drive by `browse::resolve` (AD-65) — or its refusal, as a sentence.
    fn join(&self, path: &str) -> Result<PathBuf, String>;
    /// Where the recordings index says a recording is now.
    fn recording(&self, session_id: &str) -> Option<RecordingPlace>;
    /// Every synced folder, `(profile id, local path)`.
    fn folders(&self) -> &[(String, PathBuf)];
}

/// What a block naming a recording says when this Mac's index has no row
/// for it.
pub const UNKNOWN_RECORDING_SENTENCE: &str = "keeper does not know this recording on this Mac.";

/// The body of a media block, resolved into what its player plays and the
/// transcript's lines inside its window (AD-353, AD-359). Every refusal is a
/// sentence the block shows above its source.
pub fn resolve_block(body: &str, lookup: &dyn MediaLookup) -> Result<MediaBlockVm, String> {
    let block = read_block(body, lookup)?;
    let folders = lookup.folders();
    let mut transcript = None;
    let mut transcript_path = None;
    let mut transcribe_path = None;
    let mut session_id = None;
    let mut fallback_title = None;
    let media = match &block.source {
        Source::Session(id) => {
            let place = lookup
                .recording(id)
                .ok_or_else(|| UNKNOWN_RECORDING_SENTENCE.to_owned())?;
            let manifest = SessionManifest::load(&place.folder).ok();
            let json = transcript_paths_for(&place.folder, true).0;
            if json.is_file() {
                transcript = Some(read_transcript(&json)?);
            }
            session_id = Some(id.clone());
            fallback_title = manifest
                .as_ref()
                .and_then(|manifest| manifest.meta.as_ref()?.title.clone())
                .or_else(|| {
                    transcript
                        .as_ref()
                        .and_then(|t: &Transcript| t.source.title.clone())
                })
                .or_else(|| file_name(&place.folder));
            let media = match (&transcript, &manifest) {
                (Some(t), manifest) => {
                    transcript_media(t, &place.folder, manifest.as_ref(), folders, Some(&place))
                }
                (None, Some(manifest)) => {
                    if plan_for_session(&place.folder, manifest).is_ok() {
                        transcribe_path = Some(path_text(&place.folder));
                    }
                    session_media(&place.folder, manifest, folders, Some(&place))
                }
                (None, None) => {
                    return Err("keeper cannot read this recording's manifest.".to_owned())
                }
            };
            transcript_path = Some(json);
            media
        }
        Source::Transcript(path) => {
            let json = lookup.join(path)?;
            let t = read_transcript(&json)?;
            let dir = json.parent().unwrap_or_else(|| Path::new("")).to_owned();
            let manifest = (json.file_name().and_then(|name| name.to_str())
                == Some(SESSION_TRANSCRIPT_JSON))
            .then(|| SessionManifest::load(&dir).ok())
            .flatten();
            let place = manifest
                .as_ref()
                .and_then(|manifest| manifest.meta.as_ref()?.session_id.clone())
                .and_then(|id| lookup.recording(&id));
            session_id = place.as_ref().map(|place| place.session_id.clone());
            fallback_title = t.source.title.clone();
            let media = transcript_media(&t, &dir, manifest.as_ref(), folders, place.as_ref());
            transcript = Some(t);
            transcript_path = Some(json);
            media
        }
        Source::Parts(parts) => {
            let mut resolved = Vec::with_capacity(parts.len());
            let mut end = 0.0_f64;
            for part in parts {
                let file = lookup.join(&part.file)?;
                let name = file_name(&file).unwrap_or_default();
                if !media_block::is_playable(&name) {
                    return Err(format!("{} is not an audio or video file.", part.file));
                }
                let camera = part
                    .camera
                    .as_deref()
                    .map(|camera| lookup.join(camera))
                    .transpose()?;
                let offset = part.offset.unwrap_or(end);
                let audio_tracks = [
                    (part.system_index(), super::plan::TrackOrigin::System),
                    (
                        part.microphone_index(),
                        super::plan::TrackOrigin::Microphone,
                    ),
                ]
                .into_iter()
                .filter_map(|(index, origin)| {
                    Some(MediaAudioTrackVm {
                        index: index?,
                        origin,
                    })
                })
                .collect();
                resolved.push((part, file, camera, offset, audio_tracks));
                end = offset;
            }
            if let [(_, file, ..)] = &resolved[..] {
                let json = transcript_paths_for(file, false).0;
                if json.is_file() {
                    transcript = Some(read_transcript(&json)?);
                } else if is_media_file(&file_name(file).unwrap_or_default()) && is_here(file) {
                    transcribe_path = Some(path_text(file));
                }
                fallback_title = transcript
                    .as_ref()
                    .and_then(|t| t.source.title.clone())
                    .or_else(|| file_name(file));
                transcript_path = Some(json);
            }
            let single = resolved.len() == 1;
            media_vm(
                resolved
                    .into_iter()
                    .map(
                        |(part, file, camera, offset, audio_tracks)| TranscriptMediaPartVm {
                            file: part.file.clone(),
                            offset,
                            duration: transcript
                                .as_ref()
                                .filter(|_| single)
                                .map_or(0.0, |t| t.duration),
                            screen: media_ref(&file, folders, None),
                            camera: camera.and_then(|camera| media_ref(&camera, folders, None)),
                            audio_tracks,
                            here: is_here(&file),
                        },
                    )
                    .collect(),
            )
        }
        Source::Src(_) => return Err(BlockRefusal::NestedSrc.to_string()),
    };

    let duration = transcript.as_ref().map_or_else(
        || {
            media
                .parts
                .iter()
                .map(|part| part.offset + part.duration)
                .fold(0.0, f64::max)
        },
        |t| t.duration,
    );
    let from = block.from.unwrap_or(0.0);
    // A `to` past the end plays to the end (Media Fragments §6.1.1).
    let to = block.to.filter(|to| duration <= 0.0 || *to < duration);
    let (lines, speakers) = transcript.as_ref().map_or_else(
        || (Vec::new(), Vec::new()),
        |t| {
            let lines = t
                .utterances
                .iter()
                .filter(|u| u.end > from && to.is_none_or(|to| u.start < to))
                .map(|u| MediaLineVm {
                    id: u.id.clone(),
                    speaker: u.speaker.clone(),
                    start: u.start,
                    end: u.end,
                    text: u.text.clone(),
                })
                .collect();
            let speakers = t
                .speakers
                .iter()
                .map(|speaker| MediaSpeakerVm {
                    id: speaker.id.clone(),
                    name: Transcript::display_name(speaker),
                    origin: speaker.origin,
                })
                .collect();
            (lines, speakers)
        },
    );
    Ok(MediaBlockVm {
        title: block.title.clone().or(fallback_title),
        session_id,
        transcribed: transcript.is_some(),
        transcript_path: transcript_path.as_deref().map(path_text),
        transcribe_path,
        duration,
        window: MediaWindowVm { from, to },
        picture: block.picture,
        sound: block.sound,
        media,
        lines,
        speakers,
        markers: block
            .markers
            .iter()
            .map(|marker| MediaMarkerVm {
                name: marker.name.clone(),
                from: marker.from,
                to: marker.to,
            })
            .collect(),
    })
}

/// `body` read, with the file its `src` names read under it.
pub fn read_block(body: &str, lookup: &dyn MediaLookup) -> Result<Block, String> {
    let block = media_block::parse(body).map_err(|refusal| refusal.to_string())?;
    let Source::Src(path) = &block.source else {
        return Ok(block);
    };
    let file = lookup.join(path)?;
    let size = std::fs::metadata(&file)
        .map_err(|error| format!("{path} could not be read: {error}"))?
        .len();
    if size > MAX_SRC_BYTES {
        return Err(format!(
            "{path} is larger than 64 KiB, too large to be a block's configuration."
        ));
    }
    let text = std::fs::read_to_string(&file)
        .map_err(|error| format!("{path} could not be read: {error}"))?;
    let named = media_block::parse_src(&text).map_err(|refusal| format!("{path}: {refusal}"))?;
    block.over_src(named).map_err(|refusal| refusal.to_string())
}

fn read_transcript(json: &Path) -> Result<Transcript, String> {
    let raw = std::fs::read_to_string(json)
        .map_err(|error| format!("The transcript could not be read: {error}"))?;
    Transcript::from_json(&raw).map_err(|error| error.to_string())
}

fn file_name(path: &Path) -> Option<String> {
    Some(path.file_name()?.to_str()?.to_owned())
}

fn path_text(path: &Path) -> String {
    path.to_string_lossy().into_owned()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::transcription::assemble::tests::ctx;
    use crate::transcription::model::{EngineStamp, SourceKind, TranscriptSource};
    use crate::transcription::plan::TrackOrigin;
    use crate::transcription::TranscriptionLanguage;

    fn transcript(source: TranscriptSource, duration: f64) -> Transcript {
        Transcript {
            version: 1,
            source,
            created_at: String::new(),
            engine: EngineStamp {
                asr: String::new(),
                diarizer: String::new(),
                embedding: String::new(),
            },
            language: TranscriptionLanguage::Auto,
            duration,
            speakers: Vec::new(),
            utterances: Vec::new(),
            dictionary_applied: Vec::new(),
            corrected: false,
        }
    }

    fn scratch() -> PathBuf {
        let dir = std::env::temp_dir().join(format!("keeper-media-{}", ulid::Ulid::new()));
        std::fs::create_dir_all(&dir).expect("mkdir");
        dir
    }

    #[test]
    fn a_session_pairs_each_screen_part_with_its_cameras_index_in_its_synced_folder() {
        let root = scratch();
        let session = root.join("40-media/2026/a b");
        std::fs::create_dir_all(&session).expect("mkdir");
        for file in [
            "screen-0000.mov",
            "screen-0001.mov",
            "camera-0001.mov",
            "camera-0000.mov",
        ] {
            std::fs::write(session.join(file), b"x").expect("write");
        }
        let manifest: SessionManifest = serde_json::from_value(serde_json::json!({
            "version": 1, "session": "s", "status": "finalized",
            "captureTarget": {"kind": "display"},
            "devices": {"systemAudio": true, "microphone": true, "camera": true},
            "segments": [
                {"index": 0, "file": "screen-0000.mov", "bytes": 1, "track": "screen"},
                {"index": 1, "file": "screen-0001.mov", "bytes": 1, "track": "screen"},
                {"index": 1, "file": "camera-0001.mov", "bytes": 1, "track": "camera"},
            ],
        }))
        .expect("manifest");
        const TRACKS: &[(Option<u32>, TrackOrigin)] = &[
            (Some(0), TrackOrigin::System),
            (Some(1), TrackOrigin::Microphone),
        ];
        let source = ctx(&[
            ("screen-0000.mov", 0.0, 60.0, TRACKS),
            ("screen-0001.mov", 60.0, 30.0, TRACKS),
        ])
        .source;
        let folders = [
            ("outer".to_owned(), root.clone()),
            ("inner".to_owned(), root.join("40-media")),
        ];
        let media = transcript_media(
            &transcript(source, 90.0),
            &session,
            Some(&manifest),
            &folders,
            None,
        );

        assert!(media.has_camera && media.has_screen);
        let second = &media.parts[1];
        assert_eq!((second.offset, second.duration), (60.0, 30.0));
        assert_eq!(
            second.screen,
            Some(MediaRef::File {
                profile_id: "inner".to_owned(),
                relative_path: "2026/a b/screen-0001.mov".to_owned(),
                kind: MediaRefKind::Video,
            }),
            "the innermost synced folder"
        );
        assert_eq!(
            second.camera,
            Some(MediaRef::File {
                profile_id: "inner".to_owned(),
                relative_path: "2026/a b/camera-0001.mov".to_owned(),
                kind: MediaRefKind::Video,
            })
        );
        assert_eq!(
            media.parts[0].camera, None,
            "camera-0000.mov is on disk but not in the manifest"
        );
        assert_eq!(
            second.audio_tracks,
            [
                MediaAudioTrackVm {
                    index: 0,
                    origin: TrackOrigin::System
                },
                MediaAudioTrackVm {
                    index: 1,
                    origin: TrackOrigin::Microphone
                },
            ]
        );
        std::fs::remove_dir_all(&root).expect("cleanup");
    }

    #[test]
    fn a_plain_audio_file_is_one_audio_part_and_outside_every_folder_it_has_no_reference() {
        let root = scratch();
        let source = TranscriptSource {
            kind: SourceKind::File,
            files: vec!["memo.m4a".to_owned()],
            parts: Vec::new(),
            title: None,
        };
        let t = transcript(source, 42.0);
        let media = transcript_media(&t, &root, None, &[("p".to_owned(), root.clone())], None);
        assert_eq!(
            media.parts.len(),
            1,
            "a file from before parts were recorded"
        );
        assert_eq!(media.parts[0].duration, 42.0);
        assert_eq!(
            media.parts[0].screen.as_ref().map(MediaRef::kind),
            Some(MediaRefKind::Audio)
        );
        assert!(!media.has_screen && !media.has_camera);
        assert!(
            media.parts[0].audio_tracks.is_empty(),
            "mixed tracks offer no choice"
        );

        let elsewhere =
            transcript_media(&t, &root, None, &[("p".to_owned(), root.join("sub"))], None);
        assert_eq!(elsewhere.parts[0].screen, None);
        std::fs::remove_dir_all(&root).expect("cleanup");
    }

    /// A lookup over a scratch drive: joins refuse what escapes it, the way
    /// `browse::resolve` does, and one recording is indexed.
    struct Drive {
        root: PathBuf,
        folders: Vec<(String, PathBuf)>,
        recording: Option<RecordingPlace>,
    }

    impl MediaLookup for Drive {
        fn join(&self, path: &str) -> Result<PathBuf, String> {
            if path.starts_with('/') || path.split('/').any(|segment| segment == "..") {
                return Err(format!("{path} is outside this drive."));
            }
            Ok(self.root.join(path))
        }
        fn recording(&self, session_id: &str) -> Option<RecordingPlace> {
            self.recording
                .clone()
                .filter(|place| place.session_id == session_id)
        }
        fn folders(&self) -> &[(String, PathBuf)] {
            &self.folders
        }
    }

    const SESSION: &str = "01DEV-01SES";

    fn utterance(id: &str, start: f64, end: f64) -> crate::transcription::model::Utterance {
        crate::transcription::model::Utterance {
            id: id.to_owned(),
            speaker: "S1".to_owned(),
            origin: TrackOrigin::System,
            start,
            end,
            text: format!("line {id}"),
            asr_text: String::new(),
            edited: false,
            words: vec![crate::transcription::words::Word {
                text: "x".to_owned(),
                start,
                end,
                confidence: 1.0,
            }],
        }
    }

    /// A finalized two-part session outside every synced folder, indexed.
    fn recorded(with_transcript: bool) -> (Drive, PathBuf) {
        let root = scratch();
        let recordings = root.join("recordings");
        let folder = recordings.join("2026/kelly");
        std::fs::create_dir_all(&folder).expect("mkdir");
        for file in ["screen-0000.mov", "screen-0001.mov"] {
            std::fs::write(folder.join(file), b"x").expect("write");
        }
        let manifest = serde_json::json!({
            "version": 1, "session": "kelly", "status": "finalized",
            "captureTarget": {"kind": "display"},
            "devices": {"systemAudio": true, "microphone": true, "camera": false},
            "segments": [
                {"index": 0, "file": "screen-0000.mov", "bytes": 1, "track": "screen", "ptsStart": 10.0, "ptsEnd": 70.0},
                {"index": 1, "file": "screen-0001.mov", "bytes": 1, "track": "screen"},
            ],
            "meta": {"sessionId": SESSION, "title": "Kelly sync"},
        });
        std::fs::write(folder.join("manifest.json"), manifest.to_string()).expect("manifest");
        if with_transcript {
            let mut t = transcript(
                TranscriptSource {
                    kind: SourceKind::Recording,
                    files: vec!["screen-0000.mov".to_owned()],
                    parts: Vec::new(),
                    title: None,
                },
                100.0,
            );
            t.utterances = vec![utterance("u1", 0.0, 9.0), utterance("u2", 10.0, 20.0)];
            std::fs::write(folder.join("transcript.json"), t.to_json().expect("json"))
                .expect("transcript");
        }
        let drive = Drive {
            folders: vec![("drive".to_owned(), root.join("drive"))],
            recording: Some(RecordingPlace {
                session_id: SESSION.to_owned(),
                folder: folder.clone(),
                relative_folder: "2026/kelly".to_owned(),
            }),
            root: root.join("drive"),
        };
        std::fs::create_dir_all(&drive.root).expect("drive");
        (drive, folder)
    }

    #[test]
    fn a_session_with_a_transcript_is_ready_and_plays_by_its_identity() {
        let (drive, folder) = recorded(true);
        let vm = resolve_block(
            &format!("session = \"{SESSION}\"\nfrom = 10\nto = 500"),
            &drive,
        )
        .expect("resolves");
        assert!(vm.transcribed);
        assert_eq!(vm.transcribe_path, None);
        assert_eq!(
            vm.transcript_path.as_deref(),
            Some(folder.join("transcript.json").to_string_lossy().as_ref())
        );
        assert_eq!(vm.title.as_deref(), Some("Kelly sync"));
        assert_eq!(
            vm.lines
                .iter()
                .map(|line| line.id.as_str())
                .collect::<Vec<_>>(),
            ["u2"],
            "only the lines overlapping [from, to)"
        );
        assert_eq!(vm.window.to, None, "a `to` past the end plays to the end");
        assert_eq!(
            vm.media.parts[0].screen,
            Some(MediaRef::Recording {
                session_id: SESSION.to_owned(),
                relative_path: "2026/kelly/screen-0000.mov".to_owned(),
                kind: MediaRefKind::Video,
            })
        );
        let json = serde_json::to_string(&vm).expect("serialize");
        for field in ["words", "embedding", "candidates", "asrText"] {
            assert!(
                !json.contains(field),
                "{field} crossed into the block's model"
            );
        }
        std::fs::remove_dir_all(drive.root.parent().expect("root")).expect("cleanup");
    }

    #[test]
    fn a_session_without_one_plays_its_manifest_and_names_where_it_will_be() {
        let (drive, folder) = recorded(false);
        let vm = resolve_block(&format!("session = \"{SESSION}\""), &drive).expect("resolves");
        assert!(!vm.transcribed);
        assert_eq!(
            vm.transcript_path.as_deref(),
            Some(folder.join("transcript.json").to_string_lossy().as_ref())
        );
        assert_eq!(
            vm.transcribe_path.as_deref(),
            Some(folder.to_string_lossy().as_ref())
        );
        let parts: Vec<(f64, f64)> = vm
            .media
            .parts
            .iter()
            .map(|part| (part.offset, part.duration))
            .collect();
        assert_eq!(
            parts,
            [(0.0, 60.0), (60.0, 0.0)],
            "PTS bounds, else unknown"
        );
        assert_eq!(
            vm.media.parts[0].audio_tracks,
            [
                MediaAudioTrackVm {
                    index: 0,
                    origin: TrackOrigin::System
                },
                MediaAudioTrackVm {
                    index: 1,
                    origin: TrackOrigin::Microphone
                }
            ]
        );
        assert!(vm.lines.is_empty());
        assert_eq!(
            resolve_block("session = \"01OTHER-01X\"", &drive),
            Err(UNKNOWN_RECORDING_SENTENCE.to_owned())
        );
        std::fs::remove_dir_all(drive.root.parent().expect("root")).expect("cleanup");
    }

    #[test]
    fn a_part_is_served_from_its_synced_folder_and_a_pointer_is_not_here() {
        let root = scratch();
        std::fs::create_dir_all(root.join("talks")).expect("mkdir");
        std::fs::write(root.join("talks/a.mp4"), b"x").expect("write");
        std::fs::write(
            root.join("talks/b.m4a"),
            b"version https://git-lfs.github.com/spec/v1\noid sha256:ab\nsize 123\n",
        )
        .expect("pointer");
        let drive = Drive {
            root: root.clone(),
            folders: vec![("drive".to_owned(), root.clone())],
            recording: None,
        };
        let vm = resolve_block(
            "[[part]]\nfile = \"talks/a.mp4\"\nsystem = 1\n\n[[part]]\nfile = \"talks/b.m4a\"\noffset = 30\n",
            &drive,
        )
        .expect("resolves");
        assert_eq!(
            vm.media.parts[0].screen,
            Some(MediaRef::File {
                profile_id: "drive".to_owned(),
                relative_path: "talks/a.mp4".to_owned(),
                kind: MediaRefKind::Video,
            })
        );
        assert_eq!(
            vm.media.parts[0].audio_tracks,
            [MediaAudioTrackVm {
                index: 0,
                origin: TrackOrigin::System
            }],
            "track 1 is the file's first audio track"
        );
        assert!(vm.media.parts[0].here);
        assert!(!vm.media.parts[1].here, "a pointer is not the file");
        assert_eq!(vm.media.parts[1].offset, 30.0);
        assert_eq!(vm.transcript_path, None, "two parts have no one transcript");

        let single = resolve_block("[[part]]\nfile = \"talks/a.mp4\"", &drive).expect("one");
        assert_eq!(
            single.transcript_path.as_deref(),
            Some(
                root.join("talks/a.mp4.transcript.json")
                    .to_string_lossy()
                    .as_ref()
            )
        );
        assert_eq!(
            single.transcribe_path.as_deref(),
            Some(root.join("talks/a.mp4").to_string_lossy().as_ref())
        );
        std::fs::remove_dir_all(&root).expect("cleanup");
    }

    #[test]
    fn a_path_that_escapes_the_drive_is_refused_with_the_joins_sentence() {
        let root = scratch();
        let drive = Drive {
            root: root.clone(),
            folders: Vec::new(),
            recording: None,
        };
        assert_eq!(
            resolve_block("transcript = \"../elsewhere/transcript.json\"", &drive),
            Err("../elsewhere/transcript.json is outside this drive.".to_owned())
        );
        std::fs::remove_dir_all(&root).expect("cleanup");
    }

    #[test]
    fn a_src_file_is_read_under_the_drive_and_capped_at_64_kib() {
        let (drive, _) = recorded(false);
        std::fs::write(
            drive.root.join("kelly.toml"),
            format!("session = \"{SESSION}\"\n[[marker]]\nname = \"Shared\"\nat = 5\n"),
        )
        .expect("src");
        let vm = resolve_block(
            "src = \"kelly.toml\"\ntitle = \"Mine\"\n[[marker]]\nname = \"Own\"\nat = 7\n",
            &drive,
        )
        .expect("resolves");
        assert_eq!(vm.title.as_deref(), Some("Mine"));
        assert_eq!(
            vm.markers
                .iter()
                .map(|m| m.name.as_str())
                .collect::<Vec<_>>(),
            ["Shared", "Own"]
        );
        assert_eq!(
            resolve_block(
                "src = \"kelly.toml\"\n[[marker]]\nname = \"shared\"\nat = 7\n",
                &drive
            ),
            Err("There is already a moment called shared in this block.".to_owned())
        );
        let big = format!("session = \"{SESSION}\"\n#{}\n", "x".repeat(64 * 1024));
        std::fs::write(drive.root.join("big.toml"), big).expect("big");
        assert_eq!(
            resolve_block("src = \"big.toml\"", &drive),
            Err(
                "big.toml is larger than 64 KiB, too large to be a block's configuration."
                    .to_owned()
            )
        );
        std::fs::write(drive.root.join("nested.toml"), "src = \"kelly.toml\"").expect("nested");
        assert_eq!(
            resolve_block("src = \"nested.toml\"", &drive),
            Err(format!("nested.toml: {}", BlockRefusal::NestedSrc))
        );
        std::fs::remove_dir_all(drive.root.parent().expect("root")).expect("cleanup");
    }

    #[test]
    fn a_viewer_clip_names_the_session_else_the_synced_path_else_refuses() {
        let (drive, folder) = recorded(true);
        assert_eq!(
            clip_source(&folder.join("transcript.json"), &[]),
            Ok(ClipSource::Session(SESSION.to_owned()))
        );
        let file = drive.root.join("talks/a.mp4.transcript.json");
        assert_eq!(
            clip_source(&file, &drive.folders),
            Ok(ClipSource::Transcript(
                "talks/a.mp4.transcript.json".to_owned()
            ))
        );
        assert_eq!(
            clip_source(Path::new("/elsewhere/a.transcript.json"), &drive.folders),
            Err(BlockRefusal::NotNameable)
        );
        std::fs::remove_dir_all(drive.root.parent().expect("root")).expect("cleanup");
    }
}
