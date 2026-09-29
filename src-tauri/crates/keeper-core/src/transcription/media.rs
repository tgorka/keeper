//! What the transcript viewer's player plays: each media file on the
//! transcript's timeline, the camera filmed beside it, and its audio tracks —
//! located in a synced folder, because `keeper-file://` serves the webview
//! only from one (AD-74).

use std::path::{Component, Path, PathBuf};

use super::model::{SourcePart, Transcript};
use super::vm::{
    MediaAudioTrackVm, MediaRef, MediaRefKind, TranscriptMediaPartVm, TranscriptMediaVm,
};
use crate::archive::recordings_fts::kind_for_file_name;
use crate::file_asset::is_servable_path;
use crate::recording::{SessionManifest, CAMERA_SEGMENT_STEM_PREFIX, PARTIAL_SEGMENT_SUFFIX};
use crate::vm::RecordingNoteTargetKind;

/// The player's model for `transcript`, which lives in `dir`. `manifest` is
/// the session's when the transcript is a recording's: it pairs each screen
/// segment with the camera segment of the same index. `folders` are the
/// synced folders, `(profile id, local path)`; a file in none of them gets
/// no reference.
pub fn transcript_media(
    transcript: &Transcript,
    dir: &Path,
    manifest: Option<&SessionManifest>,
    folders: &[(String, PathBuf)],
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
    let parts: Vec<TranscriptMediaPartVm> = parts
        .iter()
        .map(|part| TranscriptMediaPartVm {
            file: part.file.clone(),
            offset: part.offset,
            duration: part.duration,
            screen: media_ref(&dir.join(&part.file), folders),
            camera: manifest
                .and_then(|manifest| camera_for(manifest, &part.file))
                .map(|camera| dir.join(camera))
                .filter(|camera| camera.is_file())
                .and_then(|camera| media_ref(&camera, folders)),
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
        })
        .collect();
    TranscriptMediaVm {
        has_camera: parts.iter().any(|part| part.camera.is_some()),
        has_screen: parts.iter().any(|part| {
            part.screen
                .as_ref()
                .is_some_and(|screen| screen.kind == MediaRefKind::Video)
        }),
        parts,
    }
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

/// `path` as the synced folder holding it (the innermost, when folders
/// nest) and the `/`-joined path inside it — when the webview may be served
/// it at all.
fn media_ref(path: &Path, folders: &[(String, PathBuf)]) -> Option<MediaRef> {
    let name = path.file_name()?.to_str()?;
    let kind = match kind_for_file_name(name) {
        RecordingNoteTargetKind::Video => MediaRefKind::Video,
        RecordingNoteTargetKind::Audio => MediaRefKind::Audio,
        _ => return None,
    };
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
    let relative_path = segments.join("/");
    is_servable_path(&relative_path).then(|| MediaRef {
        profile_id: profile_id.clone(),
        relative_path,
        kind,
    })
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
        );

        assert!(media.has_camera && media.has_screen);
        let second = &media.parts[1];
        assert_eq!((second.offset, second.duration), (60.0, 30.0));
        assert_eq!(
            second.screen,
            Some(MediaRef {
                profile_id: "inner".to_owned(),
                relative_path: "2026/a b/screen-0001.mov".to_owned(),
                kind: MediaRefKind::Video,
            }),
            "the innermost synced folder"
        );
        assert_eq!(
            second
                .camera
                .as_ref()
                .map(|camera| camera.relative_path.as_str()),
            Some("2026/a b/camera-0001.mov")
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
        let media = transcript_media(&t, &root, None, &[("p".to_owned(), root.clone())]);
        assert_eq!(
            media.parts.len(),
            1,
            "a file from before parts were recorded"
        );
        assert_eq!(media.parts[0].duration, 42.0);
        assert_eq!(
            media.parts[0].screen.as_ref().map(|screen| screen.kind),
            Some(MediaRefKind::Audio)
        );
        assert!(!media.has_screen && !media.has_camera);
        assert!(
            media.parts[0].audio_tracks.is_empty(),
            "mixed tracks offer no choice"
        );

        let elsewhere = transcript_media(&t, &root, None, &[("p".to_owned(), root.join("sub"))]);
        assert_eq!(elsewhere.parts[0].screen, None);
        std::fs::remove_dir_all(&root).expect("cleanup");
    }
}
