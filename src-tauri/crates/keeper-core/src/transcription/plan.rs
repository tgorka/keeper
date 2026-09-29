//! What to transcribe, in which order, heard as whom (AD-344, AD-345).
//!
//! A recording session is its `screen-*` / `audio-*` segments in index order;
//! camera segments are skipped because they carry the same microphone audio
//! byte for byte (G1 §4). Any other media file is one part, all its audio
//! tracks mixed and diarized. The transcript is written beside the media.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use ts_rs::TS;

use super::engine::{AudioTrackInfo, TrackSelect};
use super::model::SourceKind;
use super::models::is_plain_segment;
use super::words::same_folded;
use crate::recording::{
    lfs_pointer_media_size, ManifestStatus, SessionDevices, SessionManifest,
    AUDIO_SEGMENT_STEM_PREFIX, PARTIAL_SEGMENT_SUFFIX, SEGMENT_STEM_PREFIX,
};

/// A session transcript's file names inside the session folder.
pub const SESSION_TRANSCRIPT_JSON: &str = "transcript.json";
pub const SESSION_TRANSCRIPT_MD: &str = "transcript.md";

/// The suffixes a non-session transcript takes after the media's full name:
/// `meeting.mp4` and `meeting.m4a` in one folder keep two transcripts.
const FILE_TRANSCRIPT_JSON_SUFFIX: &str = ".transcript.json";
const FILE_TRANSCRIPT_MD_SUFFIX: &str = ".transcript.md";

/// What AVFoundation decodes. WebM, Matroska, Ogg and Opus are left out: the
/// engine cannot read them, so offering them would only fail.
const MEDIA_EXTENSIONS: [&str; 11] = [
    "mov", "mp4", "m4a", "mp3", "wav", "aac", "flac", "m4v", "caf", "aiff", "aif",
];

/// What a track was heard as.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "lowercase")]
#[ts(export)]
pub enum TrackOrigin {
    /// A recording's system audio: the far end, diarized.
    System,
    /// A recording's own microphone beside system audio: the person recording.
    Microphone,
    /// Everything else: one room, or a file's tracks mixed, diarized.
    #[default]
    Mixed,
}

#[derive(Debug, Clone, PartialEq)]
pub struct TranscriptionPlan {
    pub out_json: PathBuf,
    pub out_md: PathBuf,
    pub parts: Vec<PlanPart>,
    pub source: SourceKind,
}

#[derive(Debug, Clone, PartialEq)]
pub struct PlanPart {
    pub file: PathBuf,
    /// The roles the part's tracks are expected to play; [`Self::resolve`]
    /// answers for the tracks the file really has.
    pub tracks: Vec<(TrackSelect, TrackOrigin)>,
    /// The session's devices; `None` for an arbitrary file.
    pub devices: Option<SessionDevices>,
}

impl PlanPart {
    /// The roles for the audio tracks the engine found in this file.
    pub fn resolve(&self, probed: &[AudioTrackInfo]) -> Vec<(TrackSelect, TrackOrigin)> {
        match self.devices {
            Some(devices) => assign_track_roles(&devices, probed),
            None if probed.is_empty() => Vec::new(),
            None => vec![(TrackSelect::MixAll, TrackOrigin::Mixed)],
        }
    }

    /// The file's name relative to the transcript (always a sibling).
    pub fn relative_name(&self) -> String {
        self.file
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_default()
    }
}

/// Why something cannot be transcribed.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum PlanRefusal {
    #[error("{name} is not an audio or video file keeper can transcribe.")]
    NotMedia { name: String },
    #[error("{file} is not on this computer yet, only a pointer to it is. Download it first.")]
    MediaNotHere { file: String },
    #[error("{file} is listed in the session but is not in its folder.")]
    MediaMissing { file: String },
    #[error("This session is still recording.")]
    StillRecording,
    #[error("This session recorded no audio.")]
    NoAudio,
}

/// The session's audio segments, in index order, heard per AD-345.
pub fn plan_for_session(
    folder: &Path,
    manifest: &SessionManifest,
) -> Result<TranscriptionPlan, PlanRefusal> {
    if manifest.status == ManifestStatus::Recording {
        return Err(PlanRefusal::StillRecording);
    }
    let devices = manifest.devices;
    if !devices.system_audio && !devices.microphone {
        return Err(PlanRefusal::NoAudio);
    }
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
    if segments.is_empty() {
        return Err(PlanRefusal::NoAudio);
    }
    let expected: Vec<AudioTrackInfo> = (0..u32::from(devices.system_audio)
        + u32::from(devices.microphone))
        .map(|index| AudioTrackInfo {
            index,
            channels: 0,
            duration_s: 0.0,
        })
        .collect();
    let tracks = assign_track_roles(&devices, &expected);
    let mut parts = Vec::with_capacity(segments.len());
    for segment in segments {
        let path = folder.join(&segment.file);
        if !is_plain_segment(&segment.file) || !path.is_file() {
            return Err(PlanRefusal::MediaMissing {
                file: segment.file.clone(),
            });
        }
        if lfs_pointer_media_size(&path).is_some() {
            return Err(PlanRefusal::MediaNotHere {
                file: segment.file.clone(),
            });
        }
        parts.push(PlanPart {
            file: path,
            tracks: tracks.clone(),
            devices: Some(devices),
        });
    }
    let (out_json, out_md) = transcript_paths_for(folder, true);
    Ok(TranscriptionPlan {
        out_json,
        out_md,
        parts,
        source: SourceKind::Recording,
    })
}

/// Any audio or video file: one part, every audio track mixed.
pub fn plan_for_file(path: &Path) -> Result<TranscriptionPlan, PlanRefusal> {
    let name = path
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_default();
    if !is_media_file(&name) {
        return Err(PlanRefusal::NotMedia { name });
    }
    if !path.is_file() {
        return Err(PlanRefusal::MediaMissing { file: name });
    }
    if lfs_pointer_media_size(path).is_some() {
        return Err(PlanRefusal::MediaNotHere { file: name });
    }
    let (out_json, out_md) = transcript_paths_for(path, false);
    Ok(TranscriptionPlan {
        out_json,
        out_md,
        parts: vec![PlanPart {
            file: path.to_path_buf(),
            tracks: vec![(TrackSelect::MixAll, TrackOrigin::Mixed)],
            devices: None,
        }],
        source: SourceKind::File,
    })
}

/// AD-345: with system audio AND the microphone, the first audio track is
/// system audio and the second the microphone (keeper-rec's add order). A
/// microphone alone is a room — several people — so it is diarized like any
/// mix; only beside system audio is it the person recording. A session that
/// asked for both but holds one track cannot say which it is, so that track
/// is diarized as a mix.
pub fn assign_track_roles(
    devices: &SessionDevices,
    tracks: &[AudioTrackInfo],
) -> Vec<(TrackSelect, TrackOrigin)> {
    let index = |position: usize| {
        tracks
            .get(position)
            .map(|track| TrackSelect::Index(track.index))
    };
    match (devices.system_audio, devices.microphone) {
        (true, true) => match (index(0), index(1)) {
            (Some(system), Some(microphone)) => vec![
                (system, TrackOrigin::System),
                (microphone, TrackOrigin::Microphone),
            ],
            (Some(only), None) => vec![(only, TrackOrigin::Mixed)],
            _ => Vec::new(),
        },
        (true, false) => index(0)
            .map(|system| vec![(system, TrackOrigin::System)])
            .unwrap_or_default(),
        (false, true) => index(0)
            .map(|room| vec![(room, TrackOrigin::Mixed)])
            .unwrap_or_default(),
        (false, false) => Vec::new(),
    }
}

/// Whether `name`'s extension is one of the audio/video formats transcription reads.
pub fn is_media_file(name: &str) -> bool {
    Path::new(name)
        .extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| {
            MEDIA_EXTENSIONS
                .iter()
                .any(|known| same_folded(known, extension))
        })
}

/// `(json, md)` for a session folder (`transcript.*` inside it) or a media
/// file (`<name.ext>.transcript.*` beside it).
pub fn transcript_paths_for(path: &Path, is_session: bool) -> (PathBuf, PathBuf) {
    if is_session {
        return (
            path.join(SESSION_TRANSCRIPT_JSON),
            path.join(SESSION_TRANSCRIPT_MD),
        );
    }
    let name = path
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_default();
    let parent = path.parent().unwrap_or_else(|| Path::new(""));
    (
        parent.join(format!("{name}{FILE_TRANSCRIPT_JSON_SUFFIX}")),
        parent.join(format!("{name}{FILE_TRANSCRIPT_MD_SUFFIX}")),
    )
}

/// The transcript already written for a session folder or media file.
pub fn existing_transcript(path: &Path) -> Option<PathBuf> {
    let (json, _) = transcript_paths_for(path, path.is_dir());
    json.is_file().then_some(json)
}

/// The session folder or media file a transcription job rewrites the
/// transcript at `json` from — the inverse of [`transcript_paths_for`] —
/// when it is still there: a session folder still holding its manifest, a
/// media file beside its transcript.
pub fn transcript_source(json: &Path) -> Option<PathBuf> {
    let name = json.file_name()?.to_str()?;
    let parent = json.parent()?;
    if name == SESSION_TRANSCRIPT_JSON {
        return parent
            .join("manifest.json")
            .is_file()
            .then(|| parent.to_owned());
    }
    let media = parent.join(name.strip_suffix(FILE_TRANSCRIPT_JSON_SUFFIX)?);
    media.is_file().then_some(media)
}

/// A recording session folder's transcription facts: whether it can be
/// transcribed now — its manifest loads and the core could plan it (stopped
/// recording, every audio segment here and none a pointer) — and the
/// `transcript.json` already written in it. A manifest read and a head read
/// per audio segment.
pub fn session_facts(folder: &Path) -> (bool, Option<PathBuf>) {
    let transcribable = SessionManifest::load(folder)
        .is_ok_and(|manifest| plan_for_session(folder, &manifest).is_ok());
    let (json, _) = transcript_paths_for(folder, true);
    (transcribable, json.is_file().then_some(json))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn devices(system_audio: bool, microphone: bool) -> SessionDevices {
        SessionDevices {
            system_audio,
            microphone,
            camera: true,
        }
    }

    fn infos(count: u32) -> Vec<AudioTrackInfo> {
        (0..count)
            .map(|index| AudioTrackInfo {
                index,
                channels: 2,
                duration_s: 60.0,
            })
            .collect()
    }

    fn scratch() -> PathBuf {
        let dir = std::env::temp_dir().join(format!("keeper-plan-{}", ulid::Ulid::new()));
        std::fs::create_dir_all(&dir).expect("mkdir");
        dir
    }

    fn manifest(devices: SessionDevices, kind: &str, files: &[(u32, &str)]) -> SessionManifest {
        let segments: Vec<serde_json::Value> = files
            .iter()
            .map(|(index, file)| {
                serde_json::json!({"index": index, "file": file, "bytes": 10, "track": "screen"})
            })
            .collect();
        serde_json::from_value(serde_json::json!({
            "version": 1,
            "session": "s",
            "status": "finalized",
            "captureTarget": {"kind": kind},
            "devices": devices,
            "segments": segments,
        }))
        .expect("manifest")
    }

    #[test]
    fn track_roles_follow_the_devices_the_session_recorded() {
        let both = assign_track_roles(&devices(true, true), &infos(2));
        assert_eq!(
            both,
            [
                (TrackSelect::Index(0), TrackOrigin::System),
                (TrackSelect::Index(1), TrackOrigin::Microphone)
            ]
        );
        assert_eq!(
            assign_track_roles(&devices(true, false), &infos(1)),
            [(TrackSelect::Index(0), TrackOrigin::System)]
        );
        assert_eq!(
            assign_track_roles(&devices(false, true), &infos(1)),
            [(TrackSelect::Index(0), TrackOrigin::Mixed)],
            "a microphone alone is a room, not the person recording"
        );
        assert_eq!(
            assign_track_roles(&devices(true, true), &infos(1)),
            [(TrackSelect::Index(0), TrackOrigin::Mixed)],
            "one track where two were asked for cannot be told apart"
        );
        assert!(assign_track_roles(&devices(false, false), &infos(1)).is_empty());
    }

    #[test]
    fn a_session_plans_its_screen_or_audio_segments_in_order_and_skips_the_camera() {
        let folder = scratch();
        for file in ["screen-0000.mov", "screen-0001.mov", "camera-0000.mov"] {
            std::fs::write(folder.join(file), b"\0\0\0\x18ftypqt  ").expect("write");
        }
        let manifest = manifest(
            devices(true, true),
            "display",
            &[
                (1, "screen-0001.mov"),
                (0, "camera-0000.mov"),
                (0, "screen-0000.mov"),
            ],
        );
        let plan = plan_for_session(&folder, &manifest).expect("plan");
        let names: Vec<String> = plan.parts.iter().map(PlanPart::relative_name).collect();
        assert_eq!(names, ["screen-0000.mov", "screen-0001.mov"]);
        assert_eq!(plan.out_json, folder.join("transcript.json"));
        assert_eq!(plan.source, SourceKind::Recording);
        assert_eq!(
            plan.parts[0].tracks,
            [
                (TrackSelect::Index(0), TrackOrigin::System),
                (TrackSelect::Index(1), TrackOrigin::Microphone)
            ]
        );
        std::fs::remove_dir_all(&folder).expect("cleanup");
    }

    #[test]
    fn an_audio_only_session_plans_its_m4a_segments_with_the_same_roles() {
        let folder = scratch();
        std::fs::write(folder.join("audio-0000.m4a"), b"media").expect("write");
        let manifest = manifest(devices(true, true), "audioOnly", &[(0, "audio-0000.m4a")]);
        let plan = plan_for_session(&folder, &manifest).expect("plan");
        assert_eq!(plan.parts.len(), 1);
        assert_eq!(
            plan.parts[0].resolve(&infos(2)),
            [
                (TrackSelect::Index(0), TrackOrigin::System),
                (TrackSelect::Index(1), TrackOrigin::Microphone)
            ]
        );
        std::fs::remove_dir_all(&folder).expect("cleanup");
    }

    #[test]
    fn pointer_media_is_refused_not_transcribed() {
        let folder = scratch();
        let pointer = "version https://git-lfs.github.com/spec/v1\noid sha256:ab\nsize 799000000\n";
        std::fs::write(folder.join("screen-0000.mov"), pointer).expect("write");
        let manifest = manifest(devices(true, true), "display", &[(0, "screen-0000.mov")]);
        assert_eq!(
            plan_for_session(&folder, &manifest),
            Err(PlanRefusal::MediaNotHere {
                file: "screen-0000.mov".to_owned()
            })
        );
        let file = folder.join("call.m4a");
        std::fs::write(&file, pointer).expect("write");
        assert_eq!(
            plan_for_file(&file),
            Err(PlanRefusal::MediaNotHere {
                file: "call.m4a".to_owned()
            })
        );
        std::fs::remove_dir_all(&folder).expect("cleanup");
    }

    #[test]
    fn a_media_file_is_mixed_and_its_transcript_sits_beside_it() {
        let folder = scratch();
        let file = folder.join("Weekly sync.MP4");
        std::fs::write(&file, b"media").expect("write");
        let plan = plan_for_file(&file).expect("plan");
        assert_eq!(
            plan.out_json,
            folder.join("Weekly sync.MP4.transcript.json")
        );
        assert_eq!(plan.out_md, folder.join("Weekly sync.MP4.transcript.md"));
        assert_eq!(
            plan.parts[0].resolve(&infos(3)),
            [(TrackSelect::MixAll, TrackOrigin::Mixed)]
        );
        assert!(matches!(
            plan_for_file(&folder.join("notes.txt")),
            Err(PlanRefusal::NotMedia { .. })
        ));
        assert_eq!(existing_transcript(&file), None);
        std::fs::write(folder.join("Weekly sync.MP4.transcript.json"), b"{}").expect("write");
        assert_eq!(
            existing_transcript(&file),
            Some(folder.join("Weekly sync.MP4.transcript.json"))
        );
        std::fs::remove_dir_all(&folder).expect("cleanup");
    }

    #[test]
    fn two_media_files_with_one_stem_keep_two_transcripts() {
        let folder = scratch();
        let video = folder.join("meeting.mp4");
        let audio = folder.join("meeting.m4a");
        std::fs::write(&video, b"media").expect("write");
        std::fs::write(&audio, b"media").expect("write");
        std::fs::write(folder.join("meeting.mp4.transcript.json"), b"{}").expect("write");
        assert_eq!(
            existing_transcript(&video),
            Some(folder.join("meeting.mp4.transcript.json"))
        );
        assert_eq!(
            existing_transcript(&audio),
            None,
            "the video's transcript is not the audio's"
        );
        assert_ne!(
            plan_for_file(&video).expect("plan").out_json,
            plan_for_file(&audio).expect("plan").out_json
        );
        std::fs::remove_dir_all(&folder).expect("cleanup");
    }

    #[test]
    fn a_transcript_leads_back_to_the_media_or_session_it_was_written_for() {
        let folder = scratch();
        let video = folder.join("meeting.mp4");
        let (video_json, _) = transcript_paths_for(&video, false);
        assert_eq!(transcript_source(&video_json), None, "the media is gone");
        std::fs::write(&video, b"media").expect("write");
        assert_eq!(transcript_source(&video_json), Some(video.clone()));
        assert_eq!(
            transcript_source(&folder.join(".transcript.json")),
            None,
            "a transcript of a nameless file has no source"
        );
        assert_eq!(transcript_source(&folder.join("meeting.mp4.md")), None);

        let (session_json, _) = transcript_paths_for(&folder, true);
        assert_eq!(
            transcript_source(&session_json),
            None,
            "a folder without a manifest is no session"
        );
        std::fs::write(folder.join("manifest.json"), b"{}").expect("write");
        assert_eq!(transcript_source(&session_json), Some(folder.clone()));
        std::fs::remove_dir_all(&folder).expect("cleanup");
    }

    #[test]
    fn only_formats_the_engine_decodes_are_media() {
        for name in ["a.mov", "a.M4A", "a.mp3", "a.wav", "a.aiff"] {
            assert!(is_media_file(name), "{name}");
        }
        for name in ["a.webm", "a.mkv", "a.ogg", "a.opus", "a.txt", "mov"] {
            assert!(!is_media_file(name), "{name}");
        }
    }
}
