//! A person's corrections to a transcript (AD-347). Pure edits: the shell
//! reads the file, applies one of these, writes it back and re-renders the
//! markdown. None of them touches the bank — confirming a speaker is what
//! the shell turns into a bank sample, from the speaker's clip. Every one
//! marks the transcript [`Transcript::corrected`], so a re-run never
//! overwrites a person's work.

use super::assemble::refresh_clips;
use super::bank::Person;
use super::dictionary::{suggestions, DictionarySuggestion};
use super::model::{MatchStatus, Transcript};
use super::plan::TrackOrigin;
use super::words::Word;

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum CorrectionError {
    #[error("That line is no longer in the transcript ({0}).")]
    UnknownUtterance(String),
    #[error("That speaker is no longer in the transcript ({0}).")]
    UnknownSpeaker(String),
    #[error("A speaker cannot be merged into itself.")]
    SameSpeaker,
    #[error("A line cannot be emptied; reassign it instead.")]
    EmptyText,
    #[error("A line heard on your microphone cannot move to a voice from the call, or back.")]
    CrossOrigin,
}

fn utterance_index(t: &Transcript, id: &str) -> Result<usize, CorrectionError> {
    t.utterances
        .iter()
        .position(|utterance| utterance.id == id)
        .ok_or_else(|| CorrectionError::UnknownUtterance(id.to_owned()))
}

fn speaker_index(t: &Transcript, id: &str) -> Result<usize, CorrectionError> {
    t.speakers
        .iter()
        .position(|speaker| speaker.id == id)
        .ok_or_else(|| CorrectionError::UnknownSpeaker(id.to_owned()))
}

/// The microphone and the call are different tracks: a line, and a voice's
/// embedding, never cross between them. System and mixed audio may.
fn crosses_origin(left: TrackOrigin, right: TrackOrigin) -> bool {
    (left == TrackOrigin::Microphone) != (right == TrackOrigin::Microphone)
}

/// Replace an utterance's text. `asrText` keeps what the recognizer heard;
/// the words keep their timings when the count is unchanged and are spread
/// evenly over the old span when it is not. Returns the one-word
/// substitutions the dictionary could learn from the edit.
pub fn edit_utterance(
    mut t: Transcript,
    id: &str,
    text: &str,
) -> Result<(Transcript, Vec<DictionarySuggestion>), CorrectionError> {
    let index = utterance_index(&t, id)?;
    let tokens: Vec<&str> = text.split_whitespace().collect();
    if tokens.is_empty() {
        return Err(CorrectionError::EmptyText);
    }
    let text = tokens.join(" ");
    let utterance = &mut t.utterances[index];
    if utterance.text == text {
        return Ok((t, Vec::new()));
    }
    let offered = suggestions(&utterance.text, &text);
    if tokens.len() == utterance.words.len() {
        for (word, token) in utterance.words.iter_mut().zip(&tokens) {
            (*token).clone_into(&mut word.text);
        }
    } else {
        let step = (utterance.end - utterance.start) / tokens.len() as f64;
        utterance.words = tokens
            .iter()
            .enumerate()
            .map(|(position, token)| Word {
                text: (*token).to_owned(),
                start: utterance.start + step * position as f64,
                end: utterance.start + step * (position + 1) as f64,
                confidence: 1.0,
            })
            .collect();
    }
    utterance.text = text;
    utterance.edited = true;
    t.corrected = true;
    Ok((t, offered))
}

/// Say an utterance was spoken by another (existing) speaker — one heard on
/// the same side of the call ([`CorrectionError::CrossOrigin`]).
pub fn reassign_utterance(
    mut t: Transcript,
    id: &str,
    speaker: &str,
) -> Result<Transcript, CorrectionError> {
    let index = utterance_index(&t, id)?;
    let target = speaker_index(&t, speaker)?;
    if crosses_origin(t.utterances[index].origin, t.speakers[target].origin) {
        return Err(CorrectionError::CrossOrigin);
    }
    speaker.clone_into(&mut t.utterances[index].speaker);
    settle(&mut t);
    Ok(t)
}

/// Two speakers are one voice: every line of `from` becomes `into`'s. A
/// voice from the microphone and one from the call are never one
/// ([`CorrectionError::CrossOrigin`]), and `into` only takes `from`'s
/// embedding when it has none and both were heard on the same track.
pub fn merge_speakers(
    mut t: Transcript,
    from: &str,
    into: &str,
) -> Result<Transcript, CorrectionError> {
    if from == into {
        return Err(CorrectionError::SameSpeaker);
    }
    let source = speaker_index(&t, from)?;
    let target = speaker_index(&t, into)?;
    let (source_origin, target_origin) = (t.speakers[source].origin, t.speakers[target].origin);
    if crosses_origin(source_origin, target_origin) {
        return Err(CorrectionError::CrossOrigin);
    }
    if t.speakers[target].embedding.is_none() && source_origin == target_origin {
        t.speakers[target].embedding = t.speakers[source].embedding.clone();
    }
    for utterance in t.utterances.iter_mut().filter(|u| u.speaker == from) {
        into.clone_into(&mut utterance.speaker);
    }
    settle(&mut t);
    Ok(t)
}

/// A person confirmed who a speaker is.
pub fn assign_speaker(
    mut t: Transcript,
    speaker: &str,
    person: &Person,
) -> Result<Transcript, CorrectionError> {
    let index = speaker_index(&t, speaker)?;
    let entry = &mut t.speakers[index];
    entry.person_id = Some(person.id.clone());
    entry.name = Some(person.name.clone());
    entry.status = MatchStatus::Confirmed;
    entry.candidates.clear();
    t.corrected = true;
    Ok(t)
}

/// Name a speaker in this transcript only; an empty name clears it.
pub fn rename_speaker_label(
    mut t: Transcript,
    speaker: &str,
    name: &str,
) -> Result<Transcript, CorrectionError> {
    let index = speaker_index(&t, speaker)?;
    let name = name.trim();
    t.speakers[index].name = (!name.is_empty()).then(|| name.to_owned());
    t.corrected = true;
    Ok(t)
}

/// After lines moved: re-pick every clip — the lines a clip came from may
/// have just gone to someone else. A speaker left without a line is kept,
/// with its embedding, person and identity, so the move can be undone.
fn settle(t: &mut Transcript) {
    t.corrected = true;
    refresh_clips(t);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::transcription::assemble::tests::{asr, ctx};
    use crate::transcription::assemble::{assemble, best_clip, PartResult};
    use crate::transcription::bank::tests::person;
    use crate::transcription::engine::{DiarOutput, DiarSegment, DiarSpeaker};

    fn transcript() -> Transcript {
        let segments = [("a", 0.0, 3.0), ("b", 5.0, 9.0)]
            .map(|(speaker, start, end)| DiarSegment {
                speaker: speaker.to_owned(),
                start,
                end,
            })
            .to_vec();
        let mut output = asr("we met tom gorker today", 0.0, 0.5);
        output
            .tokens
            .extend(asr("nice to meet you all", 5.0, 0.5).tokens);
        assemble(
            &[PartResult {
                offset: 0.0,
                origin: TrackOrigin::Mixed,
                asr: output,
                diar: Some(DiarOutput {
                    segments,
                    speakers: Vec::new(),
                }),
            }],
            None,
            &[],
            ctx(&[]),
        )
    }

    #[test]
    fn an_edit_keeps_what_was_heard_and_marks_itself() {
        let t = transcript();
        let (t, offered) = edit_utterance(t, "u1", "We met Tom Gorka today.").expect("edit");
        let u = &t.utterances[0];
        assert_eq!(u.text, "We met Tom Gorka today.");
        assert_eq!(u.asr_text, "we met tom gorker today");
        assert!(u.edited);
        assert_eq!(u.words[3].text, "Gorka");
        assert_eq!(
            (u.words[3].start, u.words[3].end),
            (1.5, 1.9),
            "same count keeps timings"
        );
        assert_eq!(offered.len(), 1);
        assert_eq!(
            (offered[0].from.as_str(), offered[0].to.as_str()),
            ("gorker", "Gorka")
        );

        let (t, _) = edit_utterance(t, "u1", "Hello").expect("edit");
        let u = &t.utterances[0];
        assert_eq!(u.words.len(), 1);
        assert_eq!((u.words[0].start, u.words[0].end), (u.start, u.end));
        assert_eq!(
            u.asr_text, "we met tom gorker today",
            "asrText survives every edit"
        );
        assert_eq!(
            edit_utterance(t, "u1", "  "),
            Err(CorrectionError::EmptyText)
        );
    }

    #[test]
    fn a_speaker_left_without_lines_is_kept_so_the_move_can_be_undone() {
        let t = transcript();
        assert_eq!(t.speakers.len(), 2);
        let t = reassign_utterance(t, "u2", "S1").expect("reassign");
        assert!(t.utterances.iter().all(|u| u.speaker == "S1"));
        assert_eq!(t.speakers.len(), 2, "S2 stays, lineless");
        assert_eq!(t.speaker("S2").and_then(|s| s.clip.clone()), None);
        let t = reassign_utterance(t, "u2", "S2").expect("and back");
        assert_eq!(t.utterances[1].speaker, "S2");
        assert!(matches!(
            reassign_utterance(t, "u2", "S9"),
            Err(CorrectionError::UnknownSpeaker(_))
        ));
    }

    #[test]
    fn merging_and_assigning_speakers() {
        let t = merge_speakers(transcript(), "S2", "S1").expect("merge");
        assert_eq!(t.speakers.len(), 2);
        assert!(t.utterances.iter().all(|u| u.speaker == "S1"));
        assert_eq!(
            merge_speakers(t.clone(), "S1", "S1"),
            Err(CorrectionError::SameSpeaker)
        );
        let ada = person("A", "Ada", false, "");
        let t = assign_speaker(t, "S1", &ada).expect("assign");
        let s1 = t.speaker("S1").expect("S1");
        assert_eq!(s1.status, MatchStatus::Confirmed);
        assert_eq!(
            (s1.person_id.as_deref(), s1.name.as_deref()),
            (Some("A"), Some("Ada"))
        );
        let t = rename_speaker_label(t, "S1", "  ").expect("rename");
        assert_eq!(t.speaker("S1").and_then(|s| s.name.clone()), None);
    }

    #[test]
    fn every_correction_marks_the_transcript_corrected() {
        let fresh = transcript();
        assert!(!fresh.corrected);
        let ada = person("A", "Ada", false, "");
        let corrections: [Transcript; 5] = [
            edit_utterance(fresh.clone(), "u1", "hello")
                .expect("edit")
                .0,
            reassign_utterance(fresh.clone(), "u2", "S1").expect("reassign"),
            merge_speakers(fresh.clone(), "S2", "S1").expect("merge"),
            assign_speaker(fresh.clone(), "S1", &ada).expect("assign"),
            rename_speaker_label(fresh.clone(), "S1", "Bo").expect("rename"),
        ];
        for (index, t) in corrections.iter().enumerate() {
            assert!(t.corrected, "correction {index}");
        }
        let (unchanged, _) =
            edit_utterance(fresh.clone(), "u1", &fresh.utterances[0].text).expect("no-op");
        assert!(!unchanged.corrected, "saving the same text changes nothing");
    }

    /// The call's voice S1 (embedding [1, 0]) and the microphone's ME.
    fn call_and_mic() -> Transcript {
        const TRACKS: &[(Option<u32>, TrackOrigin)] = &[
            (Some(0), TrackOrigin::System),
            (Some(1), TrackOrigin::Microphone),
        ];
        assemble(
            &[
                PartResult {
                    offset: 0.0,
                    origin: TrackOrigin::System,
                    asr: asr("one two three four five six seven eight", 0.0, 0.5),
                    diar: Some(DiarOutput {
                        segments: vec![DiarSegment {
                            speaker: "x".to_owned(),
                            start: 0.0,
                            end: 4.0,
                        }],
                        speakers: vec![DiarSpeaker {
                            speaker: "x".to_owned(),
                            embedding: vec![1.0, 0.0],
                        }],
                    }),
                },
                PartResult {
                    offset: 0.0,
                    origin: TrackOrigin::Microphone,
                    // Longer than the call's stretch: only the track rule
                    // keeps it out of the call voice's clip.
                    asr: asr("my own words here now again and again once more", 10.0, 0.6),
                    diar: None,
                },
            ],
            None,
            &[],
            ctx(&[("screen-0000.mov", 0.0, 30.0, TRACKS)]),
        )
    }

    #[test]
    fn a_line_or_a_voice_never_crosses_between_the_microphone_and_the_call() {
        let t = call_and_mic();
        let mic_line = t
            .utterances
            .iter()
            .find(|u| u.speaker == "ME")
            .map(|u| u.id.clone())
            .expect("a mic line");
        assert_eq!(
            merge_speakers(t.clone(), "S1", "ME"),
            Err(CorrectionError::CrossOrigin)
        );
        assert_eq!(
            merge_speakers(t.clone(), "ME", "S1"),
            Err(CorrectionError::CrossOrigin)
        );
        assert_eq!(
            reassign_utterance(t.clone(), &mic_line, "S1"),
            Err(CorrectionError::CrossOrigin)
        );
        assert_eq!(
            reassign_utterance(t.clone(), "u1", "ME"),
            Err(CorrectionError::CrossOrigin)
        );
        assert_eq!(
            CorrectionError::CrossOrigin.to_string(),
            "A line heard on your microphone cannot move to a voice from the call, or back."
        );
    }

    #[test]
    fn a_clip_is_cut_only_from_lines_heard_on_the_speakers_own_track() {
        let mut t = call_and_mic();
        // A file from before lines carried their track, or a hand edit: the
        // microphone's line filed under the call's voice.
        for utterance in t.utterances.iter_mut().filter(|u| u.speaker == "ME") {
            "S1".clone_into(&mut utterance.speaker);
        }
        let clip = best_clip(&t, "S1").expect("clip");
        assert_eq!(clip.track, Some(0));
        assert!(
            clip.start < 1.0 && clip.end < 4.5,
            "the call's own stretch, not the longer mic line: {clip:?}"
        );
    }

    #[test]
    fn a_merge_within_one_side_carries_the_embedding_only_on_the_same_track() {
        let mut t = call_and_mic();
        let mut s2 = t.speaker("S1").expect("S1").clone();
        s2.id = "S2".to_owned();
        s2.origin = TrackOrigin::Mixed;
        s2.embedding = None;
        t.speakers.push(s2);
        let merged = merge_speakers(t.clone(), "S1", "S2").expect("system into mixed");
        assert_eq!(
            merged.speaker("S2").and_then(|s| s.embedding.clone()),
            None,
            "a system voice's embedding does not become a mixed one's"
        );
        t.speakers.last_mut().expect("S2").origin = TrackOrigin::System;
        let merged = merge_speakers(t, "S1", "S2").expect("same track");
        assert!(merged
            .speaker("S2")
            .and_then(|s| s.embedding.clone())
            .is_some());
    }
}
