//! From engine answers to a transcript — pure, so every rule here is tested
//! without a Mac (AD-345, AD-346, AD-347).
//!
//! Per part: pieces become words, each word goes to the diarization cluster
//! it overlaps most (the nearest one when it overlaps none), and clusters are
//! linked across parts by embedding so one voice keeps one id through a
//! two-hour recording cut into segments. The microphone beside system audio
//! is the person recording (`ME`) — when it was diarized, the one voice on it
//! that is the bank's self person (or, failing that, talks most), with any
//! other voice in the room a speaker of its own; where it merely echoes the
//! far end it is dropped. Then the dictionary, then the bank.

use std::collections::{HashMap, HashSet};

use super::bank::{dot, normalized, Bank, DictionaryTerm, SUGGEST};
use super::dictionary;
use super::engine::{AsrOutput, DiarOutput, DiarSegment, TranscriptionLanguage};
use super::model::{
    AppliedTerm, ClipRef, EngineStamp, MatchStatus, Speaker, Transcript, TranscriptSource,
    Utterance, SELF_SPEAKER_ID, TRANSCRIPT_VERSION,
};
use super::plan::TrackOrigin;
use super::words::{fold, join_words, same_folded, words_from_tokens, Word};

/// Clusters in different parts are one voice at or above this cosine (AD-346).
/// Uncalibrated.
pub const LINK: f32 = 0.60;
/// A silence longer than this (seconds) ends an utterance.
pub const UTTERANCE_GAP_S: f64 = 1.5;
/// An utterance never holds more words than this.
pub const UTTERANCE_MAX_WORDS: usize = 40;
/// A microphone utterance is echo only when it holds at least this many
/// words — a short reply over the far end is too easily "the same words"…
pub const ECHO_MIN_TOKENS: usize = 4;
/// …one system utterance covers at least this share of its time…
pub const ECHO_OVERLAP: f64 = 0.5;
/// …and the longest common run of words, in order, with the system speech
/// around it covers at least this share of its words (AD-345).
pub const ECHO_LCS: f64 = 0.6;
/// A bank clip is 2–15 s (AD-343).
pub const CLIP_MIN_S: f64 = 2.0;
pub const CLIP_MAX_S: f64 = 15.0;

/// How far around a microphone utterance system words are compared for echo.
const ECHO_SLACK_S: f64 = 0.5;

/// One track of one part, as the engine heard it. Times inside are relative
/// to the part; `offset` places it on the transcript timeline.
#[derive(Debug, Clone, PartialEq)]
pub struct PartResult {
    pub offset: f64,
    pub origin: TrackOrigin,
    pub asr: AsrOutput,
    pub diar: Option<DiarOutput>,
}

/// Everything about a transcript that is not heard.
#[derive(Debug, Clone, PartialEq)]
pub struct AssembleContext {
    pub source: TranscriptSource,
    pub created_at: String,
    pub engine: EngineStamp,
    pub language: TranscriptionLanguage,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
enum Voice {
    Me,
    Cluster(usize),
}

struct Cluster {
    origin: TrackOrigin,
    /// Sum of the unit embeddings linked into it.
    sum: Option<Vec<f32>>,
}

struct Heard {
    word: Word,
    voice: Voice,
    /// The part's offset: an utterance never spans two files.
    part: f64,
}

struct Draft {
    voice: Voice,
    part: f64,
    words: Vec<Word>,
}

impl Draft {
    fn start(&self) -> f64 {
        self.words.first().map_or(0.0, |word| word.start)
    }
    fn end(&self) -> f64 {
        self.words.last().map_or(0.0, |word| word.end)
    }
}

/// The transcript for `parts`.
pub fn assemble(
    parts: &[PartResult],
    bank: Option<&Bank>,
    dictionary: &[DictionaryTerm],
    ctx: AssembleContext,
) -> Transcript {
    let mut order: Vec<&PartResult> = parts.iter().collect();
    order.sort_by(|a, b| a.offset.total_cmp(&b.offset));
    let own_voice = bank.and_then(|bank| {
        let me = bank.self_person()?;
        bank.centroids(&ctx.engine.embedding)
            .into_iter()
            .find(|(id, _)| *id == me.id)
            .map(|(_, centroid)| centroid)
    });

    let mut clusters: Vec<Cluster> = Vec::new();
    let mut heard: Vec<Heard> = Vec::new();
    let undiarized = DiarOutput::default();
    // Sum of the unit embeddings of every microphone cluster taken as `ME`.
    let mut me_sum: Option<Vec<f32>> = None;
    for part in order {
        let words = words_from_tokens(&part.asr.tokens);
        let microphone = part.origin == TrackOrigin::Microphone;
        let diar = match &part.diar {
            Some(diar) => diar,
            // A microphone nobody diarized is the person recording, whole.
            None if microphone => {
                heard.extend(words.into_iter().map(|word| Heard {
                    word: shifted(word, part.offset),
                    voice: Voice::Me,
                    part: part.offset,
                }));
                continue;
            }
            None => &undiarized,
        };
        let labels: Vec<&str> = words
            .iter()
            .map(|word| label_for(word.start, word.end, &diar.segments))
            .collect();
        let me = microphone.then(|| self_label(&words, &labels, diar, own_voice.as_deref()));
        if let Some(unit) = me.and_then(|me| cluster_unit(diar, me)) {
            add_unit(&mut me_sum, unit);
        }
        let others: Vec<&str> = labels
            .iter()
            .copied()
            .filter(|label| Some(*label) != me)
            .collect();
        let linked = link_part(&mut clusters, part, &others);
        heard.extend(words.into_iter().zip(&labels).map(|(word, label)| Heard {
            word: shifted(word, part.offset),
            voice: if Some(*label) == me {
                Voice::Me
            } else {
                Voice::Cluster(linked[label])
            },
            part: part.offset,
        }));
    }

    let origin_of = |voice: Voice| match voice {
        Voice::Me => TrackOrigin::Microphone,
        Voice::Cluster(cluster) => clusters[cluster].origin,
    };
    // Every microphone line — the person recording or anyone beside them —
    // is checked against the far end for echo.
    let (mine, theirs): (Vec<Heard>, Vec<Heard>) = heard
        .into_iter()
        .partition(|item| origin_of(item.voice) == TrackOrigin::Microphone);
    let theirs = drafts(theirs);
    let mine: Vec<Draft> = drafts(mine)
        .into_iter()
        .filter(|draft| !is_echo(draft, &theirs))
        .collect();
    let mut all: Vec<Draft> = theirs.into_iter().chain(mine).collect();
    all.sort_by(|a, b| {
        a.start()
            .total_cmp(&b.start())
            .then(a.end().total_cmp(&b.end()))
    });

    // S1, S2… in order of first appearance on the whole timeline; a voice
    // whose every line was echo is no speaker.
    let mut numbered: Vec<usize> = Vec::new();
    for draft in &all {
        if let Voice::Cluster(index) = draft.voice {
            if !numbered.contains(&index) {
                numbered.push(index);
            }
        }
    }
    let ids: HashMap<Voice, String> = numbered
        .iter()
        .enumerate()
        .map(|(rank, index)| (Voice::Cluster(*index), format!("S{}", rank + 1)))
        .chain(std::iter::once((Voice::Me, SELF_SPEAKER_ID.to_owned())))
        .collect();

    let mut applied: Vec<AppliedTerm> = Vec::new();
    let utterances: Vec<Utterance> = all
        .into_iter()
        .enumerate()
        .map(|(index, draft)| {
            let asr_text = join_words(&draft.words);
            let (words, fired) = dictionary::apply(&draft.words, dictionary);
            for term in fired {
                match applied
                    .iter_mut()
                    .find(|entry| entry.to == term.to && same_folded(&entry.from, &term.from))
                {
                    Some(entry) => entry.count += term.count,
                    None => applied.push(term),
                }
            }
            Utterance {
                id: format!("u{}", index + 1),
                speaker: ids[&draft.voice].clone(),
                origin: origin_of(draft.voice),
                start: draft.start(),
                end: draft.end(),
                text: join_words(&words),
                asr_text,
                edited: false,
                words,
            }
        })
        .collect();

    let mut speakers = Vec::new();
    if utterances.iter().any(|u| u.speaker == SELF_SPEAKER_ID) {
        let me = bank.and_then(Bank::self_person);
        speakers.push(Speaker {
            id: SELF_SPEAKER_ID.to_owned(),
            origin: TrackOrigin::Microphone,
            person_id: me.map(|person| person.id.clone()),
            name: me.map(|person| person.name.clone()),
            status: MatchStatus::Me,
            score: None,
            candidates: Vec::new(),
            embedding: me_sum.as_deref().and_then(normalized),
            clip: None,
        });
    }
    for index in &numbered {
        let cluster = &clusters[*index];
        let embedding = cluster.sum.as_deref().and_then(normalized);
        let matched = match (bank, &embedding) {
            (Some(bank), Some(embedding)) => {
                Some(bank.match_speaker(embedding, &ctx.engine.embedding))
            }
            _ => None,
        };
        let (status, person_id, name, score, candidates) = match matched {
            Some(m) => (m.status, m.person_id, m.name, m.score, m.candidates),
            None => (MatchStatus::Unknown, None, None, None, Vec::new()),
        };
        speakers.push(Speaker {
            id: ids[&Voice::Cluster(*index)].clone(),
            origin: cluster.origin,
            person_id,
            name,
            status,
            score,
            candidates,
            embedding,
            clip: None,
        });
    }

    let heard_until = utterances.iter().map(|u| u.end).fold(0.0, f64::max);
    let duration = ctx
        .source
        .parts
        .iter()
        .map(|part| part.offset + part.duration)
        .fold(heard_until, f64::max);
    let mut transcript = Transcript {
        version: TRANSCRIPT_VERSION,
        source: ctx.source,
        created_at: ctx.created_at,
        engine: ctx.engine,
        language: ctx.language,
        duration,
        speakers,
        utterances,
        dictionary_applied: applied,
        corrected: false,
    };
    refresh_clips(&mut transcript);
    transcript
}

/// Recompute every speaker's clip — after assembly, and after a correction
/// moved utterances between speakers.
pub fn refresh_clips(transcript: &mut Transcript) {
    let clips: Vec<Option<ClipRef>> = transcript
        .speakers
        .iter()
        .map(|speaker| best_clip(transcript, &speaker.id))
        .collect();
    for (speaker, clip) in transcript.speakers.iter_mut().zip(clips) {
        speaker.clip = clip;
    }
}

fn shifted(word: Word, offset: f64) -> Word {
    Word {
        start: word.start + offset,
        end: word.end + offset,
        ..word
    }
}

/// The cluster a word belongs to: most overlap, else the nearest segment,
/// else (no diarization) the part's one anonymous voice `""`.
fn label_for(start: f64, end: f64, segments: &[DiarSegment]) -> &str {
    let mut overlap: Vec<(&str, f64)> = Vec::new();
    for segment in segments {
        let shared = end.min(segment.end) - start.max(segment.start);
        if shared > 0.0 {
            match overlap
                .iter_mut()
                .find(|(label, _)| *label == segment.speaker)
            {
                Some(entry) => entry.1 += shared,
                None => overlap.push((&segment.speaker, shared)),
            }
        }
    }
    if let Some((label, _)) =
        overlap
            .iter()
            .copied()
            .reduce(|best, next| if next.1 > best.1 { next } else { best })
    {
        return label;
    }
    let middle = (start + end) / 2.0;
    let distance = |segment: &DiarSegment| {
        if middle < segment.start {
            segment.start - middle
        } else if middle > segment.end {
            middle - segment.end
        } else {
            0.0
        }
    };
    segments
        .iter()
        .reduce(|best, next| {
            if distance(next) < distance(best) {
                next
            } else {
                best
            }
        })
        .map_or("", |segment| segment.speaker.as_str())
}

/// The label of the person recording among a diarized microphone part's
/// clusters: the one closest to the bank's self person at or above
/// [`SUGGEST`], else the one heard for the longest (the earliest on a tie).
fn self_label<'a>(
    words: &[Word],
    labels: &[&'a str],
    diar: &DiarOutput,
    own_voice: Option<&[f32]>,
) -> &'a str {
    let mut spoken: Vec<(&'a str, f64)> = Vec::new();
    for (word, label) in words.iter().zip(labels) {
        match spoken.iter_mut().find(|(seen, _)| *seen == *label) {
            Some(entry) => entry.1 += word.end - word.start,
            None => spoken.push((*label, word.end - word.start)),
        }
    }
    let recognized = own_voice.and_then(|own| {
        spoken
            .iter()
            .filter_map(|(label, _)| {
                let unit = cluster_unit(diar, label)?;
                (unit.len() == own.len()).then(|| (dot(&unit, own), *label))
            })
            .filter(|(cosine, _)| *cosine >= SUGGEST)
            .reduce(|best, next| if next.0 > best.0 { next } else { best })
    });
    if let Some((_, label)) = recognized {
        return label;
    }
    spoken
        .into_iter()
        .reduce(|best, next| if next.1 > best.1 { next } else { best })
        .map_or("", |(label, _)| label)
}

/// A diarization cluster's unit embedding.
fn cluster_unit(diar: &DiarOutput, label: &str) -> Option<Vec<f32>> {
    diar.speakers
        .iter()
        .find(|speaker| speaker.speaker == label)
        .and_then(|speaker| normalized(&speaker.embedding))
}

/// Add a unit embedding to a running sum; one of another length is ignored.
fn add_unit(sum: &mut Option<Vec<f32>>, unit: Vec<f32>) {
    match sum {
        Some(sum) if sum.len() == unit.len() => {
            for (total, value) in sum.iter_mut().zip(&unit) {
                *total += value;
            }
        }
        Some(_) => {}
        None => *sum = Some(unit),
    }
}

/// Map a part's local cluster labels onto global clusters: each local
/// cluster joins the most similar earlier cluster at or above [`LINK`]
/// (one-to-one within a part, best pairs first), or starts a new one.
fn link_part<'a>(
    clusters: &mut Vec<Cluster>,
    part: &PartResult,
    labels: &[&'a str],
) -> HashMap<&'a str, usize> {
    let mut locals: Vec<&str> = Vec::new();
    for label in labels {
        if !locals.contains(label) {
            locals.push(label);
        }
    }
    let units: Vec<Option<Vec<f32>>> = locals
        .iter()
        .map(|label| cluster_unit(part.diar.as_ref()?, label))
        .collect();

    let mut pairs: Vec<(f32, usize, usize)> = Vec::new();
    for (local, unit) in units.iter().enumerate() {
        let Some(unit) = unit else { continue };
        for (index, cluster) in clusters.iter().enumerate() {
            if cluster.origin != part.origin {
                continue;
            }
            let Some(centroid) = cluster.sum.as_deref().and_then(normalized) else {
                continue;
            };
            if centroid.len() != unit.len() {
                continue;
            }
            let cosine = dot(unit, &centroid);
            if cosine >= LINK {
                pairs.push((cosine, local, index));
            }
        }
    }
    pairs.sort_by(|a, b| b.0.total_cmp(&a.0));
    let mut assigned: HashMap<usize, usize> = HashMap::new();
    let mut taken: HashSet<usize> = HashSet::new();
    for (_, local, index) in pairs {
        if !assigned.contains_key(&local) && taken.insert(index) {
            assigned.insert(local, index);
        }
    }

    let mut mapping = HashMap::with_capacity(locals.len());
    for (local, (label, unit)) in locals.iter().zip(units).enumerate() {
        let index = match assigned.get(&local) {
            Some(&index) => index,
            None => {
                clusters.push(Cluster {
                    origin: part.origin,
                    sum: None,
                });
                clusters.len() - 1
            }
        };
        if let Some(unit) = unit {
            add_unit(&mut clusters[index].sum, unit);
        }
        mapping.insert(*label, index);
    }
    mapping
}

/// One stream's words, cut into utterances on a change of voice or file, a
/// silence over [`UTTERANCE_GAP_S`], or [`UTTERANCE_MAX_WORDS`].
fn drafts(mut heard: Vec<Heard>) -> Vec<Draft> {
    heard.sort_by(|a, b| a.word.start.total_cmp(&b.word.start));
    let mut out: Vec<Draft> = Vec::new();
    for item in heard {
        let continues = out.last().is_some_and(|draft| {
            draft.voice == item.voice
                && draft.part == item.part
                && draft.words.len() < UTTERANCE_MAX_WORDS
                && item.word.start - draft.end() <= UTTERANCE_GAP_S
        });
        match out.last_mut() {
            Some(draft) if continues => draft.words.push(item.word),
            _ => out.push(Draft {
                voice: item.voice,
                part: item.part,
                words: vec![item.word],
            }),
        }
    }
    out
}

/// The words of `words` in order, folded and without punctuation.
fn tokens<'w>(words: impl IntoIterator<Item = &'w Word>) -> Vec<String> {
    words
        .into_iter()
        .flat_map(|word| word.text.split_whitespace())
        .map(|token| {
            fold(
                &token
                    .chars()
                    .filter(|c| c.is_alphanumeric())
                    .collect::<String>(),
            )
        })
        .filter(|token| !token.is_empty())
        .collect()
}

/// The length of the longest common subsequence of two word sequences.
fn common_run(left: &[String], right: &[String]) -> usize {
    let mut row = vec![0usize; right.len() + 1];
    for word in left {
        let mut diagonal = 0;
        for (j, other) in right.iter().enumerate() {
            let above = row[j + 1];
            row[j + 1] = if word == other {
                diagonal + 1
            } else {
                above.max(row[j])
            };
            diagonal = above;
        }
    }
    row[right.len()]
}

/// AD-345: a microphone utterance is the speakers leaking into the mic when
/// it is long enough to tell ([`ECHO_MIN_TOKENS`]), one far-end utterance
/// covers at least half its time, and the far end said mostly its words in
/// its order. Order is what tells an echo from a reply: "yes that is it" over
/// "so is that it then yes okay" shares every word but not their sequence.
fn is_echo(mine: &Draft, theirs: &[Draft]) -> bool {
    let own = tokens(&mine.words);
    if own.len() < ECHO_MIN_TOKENS {
        return false;
    }
    let (start, end) = (mine.start(), mine.end());
    let length = (end - start).max(1e-3);
    let covered = theirs
        .iter()
        .map(|draft| (end.min(draft.end()) - start.max(draft.start())).max(0.0))
        .fold(0.0, f64::max);
    if covered / length < ECHO_OVERLAP {
        return false;
    }
    let near = tokens(
        theirs
            .iter()
            .flat_map(|draft| &draft.words)
            .filter(|word| word.end >= start - ECHO_SLACK_S && word.start <= end + ECHO_SLACK_S),
    );
    common_run(&own, &near) as f64 / own.len() as f64 >= ECHO_LCS
}

/// The best stretch of `speaker` to keep as a voice clip: the longest run of
/// their consecutive utterances (same file, no silence over the utterance
/// gap) that no other speaker talks over, at least [`CLIP_MIN_S`] long, cut
/// to its middle [`CLIP_MAX_S`] — located in the file and track the speaker
/// was heard on. Only lines heard on the speaker's own track count as theirs:
/// a clip cut from one track never pairs with a voice from another.
pub fn best_clip(transcript: &Transcript, speaker: &str) -> Option<ClipRef> {
    let origin = transcript.speaker(speaker)?.origin;
    let theirs = |utterance: &Utterance| utterance.speaker == speaker && utterance.origin == origin;
    let parts = &transcript.source.parts;
    let part_of = |time: f64| {
        parts
            .iter()
            .filter(|part| part.offset <= time + 1e-6)
            .max_by(|a, b| a.offset.total_cmp(&b.offset))
    };

    let mut ordered: Vec<&Utterance> = transcript.utterances.iter().collect();
    ordered.sort_by(|a, b| a.start.total_cmp(&b.start));
    let mut runs: Vec<(f64, f64)> = Vec::new();
    let mut previous: Option<&Utterance> = None;
    for utterance in ordered {
        if !theirs(utterance) {
            previous = Some(utterance);
            continue;
        }
        let joins = previous.is_some_and(|before| {
            theirs(before)
                && utterance.start - before.end <= UTTERANCE_GAP_S
                && part_of(before.start).map(|p| p.offset)
                    == part_of(utterance.start).map(|p| p.offset)
        });
        match runs.last_mut() {
            Some(run) if joins => run.1 = run.1.max(utterance.end),
            _ => runs.push((utterance.start, utterance.end)),
        }
        previous = Some(utterance);
    }

    let others: Vec<(f64, f64)> = transcript
        .utterances
        .iter()
        .filter(|u| !theirs(u))
        .map(|u| (u.start, u.end))
        .collect();
    let (start, end) = runs
        .into_iter()
        .filter(|(start, end)| end - start >= CLIP_MIN_S)
        .filter(|(start, end)| !others.iter().any(|(s, e)| s < end && e > start))
        .max_by(|a, b| (a.1 - a.0).total_cmp(&(b.1 - b.0)))?;
    let (start, end) = if end - start > CLIP_MAX_S {
        let middle = (start + end) / 2.0;
        (middle - CLIP_MAX_S / 2.0, middle + CLIP_MAX_S / 2.0)
    } else {
        (start, end)
    };
    let part = part_of(start)?;
    let track = part.tracks.iter().find(|track| track.origin == origin)?;
    Some(ClipRef {
        file: part.file.clone(),
        track: track.track,
        start: (start - part.offset).max(0.0),
        end: end - part.offset,
    })
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::transcription::bank::tests::{apply, person, scratch};
    use crate::transcription::bank::{
        sample_path, BankPlan, BankWrite, EmbeddingSample, SampleSource,
    };
    use crate::transcription::engine::{AsrToken, DiarSpeaker};
    use crate::transcription::model::{PartTrack, SourceKind, SourcePart};

    /// Tokens for `text`, one word per `step` seconds from `start`.
    pub(crate) fn asr(text: &str, start: f64, step: f64) -> AsrOutput {
        AsrOutput {
            text: text.to_owned(),
            confidence: 0.9,
            tokens: text
                .split_whitespace()
                .enumerate()
                .map(|(index, word)| AsrToken {
                    text: format!("▁{word}"),
                    start: start + index as f64 * step,
                    end: start + index as f64 * step + step * 0.8,
                    confidence: 0.9,
                })
                .collect(),
        }
    }

    fn diar(segments: &[(&str, f64, f64)], speakers: &[(&str, Vec<f32>)]) -> DiarOutput {
        DiarOutput {
            segments: segments
                .iter()
                .map(|(speaker, start, end)| DiarSegment {
                    speaker: (*speaker).to_owned(),
                    start: *start,
                    end: *end,
                })
                .collect(),
            speakers: speakers
                .iter()
                .map(|(speaker, embedding)| DiarSpeaker {
                    speaker: (*speaker).to_owned(),
                    embedding: embedding.clone(),
                })
                .collect(),
        }
    }

    /// `(file, offset, duration, tracks)` of one source part.
    pub(crate) type PartSpec<'a> = (&'a str, f64, f64, &'a [(Option<u32>, TrackOrigin)]);

    pub(crate) fn ctx(parts: &[PartSpec]) -> AssembleContext {
        AssembleContext {
            source: TranscriptSource {
                kind: SourceKind::Recording,
                files: parts.iter().map(|(file, ..)| (*file).to_owned()).collect(),
                parts: parts
                    .iter()
                    .map(|(file, offset, duration, tracks)| SourcePart {
                        file: (*file).to_owned(),
                        offset: *offset,
                        duration: *duration,
                        tracks: tracks
                            .iter()
                            .map(|(track, origin)| PartTrack {
                                track: *track,
                                origin: *origin,
                            })
                            .collect(),
                    })
                    .collect(),
                title: None,
            },
            created_at: "2026-09-28T10:00:00+02:00".to_owned(),
            engine: EngineStamp {
                asr: "parakeet-tdt-0.6b-v3".to_owned(),
                diarizer: "speaker-diarization".to_owned(),
                embedding: "m1".to_owned(),
            },
            language: TranscriptionLanguage::Auto,
        }
    }

    const SYSTEM_AND_MIC: &[(Option<u32>, TrackOrigin)] = &[
        (Some(0), TrackOrigin::System),
        (Some(1), TrackOrigin::Microphone),
    ];

    fn part(
        offset: f64,
        origin: TrackOrigin,
        asr: AsrOutput,
        diar: Option<DiarOutput>,
    ) -> PartResult {
        PartResult {
            offset,
            origin,
            asr,
            diar,
        }
    }

    fn said(transcript: &Transcript) -> Vec<(&str, &str)> {
        transcript
            .utterances
            .iter()
            .map(|u| (u.speaker.as_str(), u.text.as_str()))
            .collect()
    }

    #[test]
    fn a_word_goes_to_the_segment_it_overlaps_most_or_the_nearest_one() {
        let segments = diar(&[("a", 0.0, 1.0), ("b", 1.0, 3.0), ("a", 5.0, 6.0)], &[]).segments;
        assert_eq!(
            label_for(0.8, 1.5, &segments),
            "b",
            "0.5 s in b beats 0.2 s in a"
        );
        assert_eq!(label_for(0.2, 1.1, &segments), "a");
        assert_eq!(
            label_for(4.2, 4.4, &segments),
            "a",
            "in the gap, 5.0 is nearer than 3.0"
        );
        assert_eq!(label_for(3.4, 3.6, &segments), "b");
        assert_eq!(label_for(1.0, 2.0, &[]), "", "no diarization: one voice");
    }

    #[test]
    fn utterances_split_on_speaker_change_gap_and_length() {
        let text = "one two three four five";
        let t = assemble(
            &[part(
                0.0,
                TrackOrigin::Mixed,
                AsrOutput {
                    tokens: asr(text, 0.0, 0.5)
                        .tokens
                        .into_iter()
                        .chain(asr("six seven", 10.0, 0.5).tokens)
                        .collect(),
                    ..AsrOutput::default()
                },
                Some(diar(
                    &[("a", 0.0, 1.0), ("b", 1.0, 3.0), ("b", 9.0, 12.0)],
                    &[("a", vec![1.0, 0.0]), ("b", vec![0.0, 1.0])],
                )),
            )],
            None,
            &[],
            ctx(&[]),
        );
        assert_eq!(
            said(&t),
            [
                ("S1", "one two"),
                ("S2", "three four five"),
                ("S2", "six seven")
            ]
        );
        let long: Vec<String> = (0..45).map(|i| format!("w{i}")).collect();
        let t = assemble(
            &[part(
                0.0,
                TrackOrigin::Mixed,
                asr(&long.join(" "), 0.0, 0.3),
                None,
            )],
            None,
            &[],
            ctx(&[]),
        );
        assert_eq!(t.utterances.len(), 2);
        assert_eq!(t.utterances[0].words.len(), UTTERANCE_MAX_WORDS);
    }

    #[test]
    fn one_voice_keeps_its_id_across_parts_and_a_new_voice_gets_the_next() {
        let ada = vec![1.0, 0.1, 0.0];
        let bo = vec![0.0, 1.0, 0.0];
        let cy = vec![0.0, 0.0, 1.0];
        let t = assemble(
            &[
                part(
                    0.0,
                    TrackOrigin::System,
                    asr("hello there general", 0.0, 1.0),
                    Some(diar(
                        &[("x", 0.0, 1.9), ("y", 1.9, 3.0)],
                        &[("x", ada.clone()), ("y", bo)],
                    )),
                ),
                part(
                    60.0,
                    TrackOrigin::System,
                    asr("new voice first then ada", 0.0, 1.0),
                    Some(diar(
                        // Part-local labels mean nothing across parts.
                        &[("x", 0.0, 3.9), ("y", 3.9, 5.0)],
                        &[("x", cy), ("y", vec![0.9, 0.2, 0.05])],
                    )),
                ),
            ],
            None,
            &[],
            ctx(&[]),
        );
        assert_eq!(
            said(&t),
            [
                ("S1", "hello there"),
                ("S2", "general"),
                ("S3", "new voice first then"),
                ("S1", "ada")
            ]
        );
        let s1 = t
            .speaker("S1")
            .and_then(|s| s.embedding.clone())
            .expect("embedding");
        assert!(
            (s1.iter().map(|v| v * v).sum::<f32>() - 1.0).abs() < 1e-5,
            "normalized"
        );
    }

    #[test]
    fn echo_is_dropped_but_a_real_mic_utterance_over_other_speech_is_kept() {
        let t = assemble(
            &[
                part(
                    0.0,
                    TrackOrigin::System,
                    asr("can you see my screen now", 0.0, 0.4),
                    Some(diar(&[("x", 0.0, 3.0)], &[("x", vec![1.0, 0.0])])),
                ),
                part(
                    0.0,
                    TrackOrigin::Microphone,
                    AsrOutput {
                        tokens: asr("can you see my screen", 0.05, 0.4)
                            .tokens
                            .into_iter()
                            .chain(asr("yes it looks fine", 4.0, 0.4).tokens)
                            .collect(),
                        ..AsrOutput::default()
                    },
                    None,
                ),
            ],
            None,
            &[],
            ctx(&[]),
        );
        assert_eq!(
            said(&t),
            [
                ("S1", "can you see my screen now"),
                ("ME", "yes it looks fine")
            ],
            "the echo of the far end is gone"
        );
        let t = assemble(
            &[
                part(
                    0.0,
                    TrackOrigin::System,
                    asr("can you see my screen now", 0.0, 0.4),
                    Some(diar(&[("x", 0.0, 3.0)], &[("x", vec![1.0, 0.0])])),
                ),
                part(
                    0.0,
                    TrackOrigin::Microphone,
                    asr("sorry I have to interrupt", 0.2, 0.4),
                    None,
                ),
            ],
            None,
            &[],
            ctx(&[]),
        );
        assert_eq!(
            said(&t),
            [
                ("S1", "can you see my screen now"),
                ("ME", "sorry I have to interrupt")
            ],
            "talking over someone is not echo"
        );
    }

    #[test]
    fn a_reply_with_the_far_ends_words_in_another_order_is_crosstalk_not_echo() {
        let with_mic = |mic: &str, at: f64| {
            assemble(
                &[
                    part(
                        0.0,
                        TrackOrigin::System,
                        asr("so is that it then yes okay", 9.0, 0.4),
                        Some(diar(&[("x", 9.0, 12.0)], &[("x", vec![1.0, 0.0])])),
                    ),
                    part(0.0, TrackOrigin::Microphone, asr(mic, at, 0.4), None),
                ],
                None,
                &[],
                ctx(&[]),
            )
        };
        let t = with_mic("yes that is it", 9.6);
        assert_eq!(
            said(&t),
            [
                ("S1", "so is that it then yes okay"),
                ("ME", "yes that is it")
            ],
            "the same words out of order are a reply"
        );
        let t = with_mic("yes okay", 11.05);
        assert!(
            said(&t).contains(&("ME", "yes okay")),
            "a back-channel over the same words is too short to call echo"
        );
        let t = with_mic("is that it then yes", 9.45);
        assert_eq!(
            said(&t),
            [("S1", "so is that it then yes okay")],
            "the far end's words in its order are its echo"
        );
    }

    /// A bank at `root` with one `m1` sample per `(id, name, is_self, vector)`.
    fn bank_of(root: &std::path::Path, people: &[(&str, &str, bool, Vec<f32>)]) -> Bank {
        let mut plan = BankPlan::default();
        for (id, name, is_self, vector) in people {
            let p = person(id, name, *is_self, "2026-09-01T00:00:00Z");
            plan.writes.push(BankWrite {
                rel_path: format!("people/{id}.json"),
                bytes: serde_json::to_vec(&p).expect("json"),
            });
            let sample = EmbeddingSample {
                version: 1,
                model: "m1".to_owned(),
                person: (*id).to_owned(),
                clip: "C".to_owned(),
                vector: vector.clone(),
                source: SampleSource::default(),
                added_at: String::new(),
            };
            plan.writes.push(BankWrite {
                rel_path: sample_path("m1", id, "C"),
                bytes: serde_json::to_vec(&sample).expect("json"),
            });
        }
        apply(root, &plan);
        Bank::load(root)
    }

    #[test]
    fn the_bank_names_speakers_and_the_mic_is_its_self_person() {
        let root = scratch();
        let bank = bank_of(
            &root,
            &[
                ("A", "Ada", false, vec![1.0, 0.0]),
                ("M", "Me Myself", true, vec![0.0, 1.0]),
            ],
        );
        let parts = [
            part(
                0.0,
                TrackOrigin::System,
                asr("one two three four five six seven", 0.0, 0.5),
                Some(diar(&[("x", 0.0, 4.0)], &[("x", vec![0.95, 0.05])])),
            ),
            part(
                0.0,
                TrackOrigin::Microphone,
                asr("my own words here now", 10.0, 0.6),
                None,
            ),
        ];
        let context = ctx(&[("screen-0000.mov", 0.0, 30.0, SYSTEM_AND_MIC)]);
        let t = assemble(&parts, Some(&bank), &[], context.clone());
        let s1 = t.speaker("S1").expect("S1");
        assert_eq!(
            (s1.status, s1.name.as_deref()),
            (MatchStatus::Auto, Some("Ada"))
        );
        let me = t.speaker("ME").expect("ME");
        assert_eq!(me.status, MatchStatus::Me);
        assert_eq!(me.person_id.as_deref(), Some("M"));
        let clip = me.clip.clone().expect("the mic has a clip");
        assert_eq!(
            (clip.file.as_str(), clip.track),
            ("screen-0000.mov", Some(1)),
            "the mic's clip is on the mic track"
        );
        assert!((clip.start - 10.0).abs() < 1e-9 && (clip.end - 12.88).abs() < 1e-9);
        assert_eq!(s1.clip.as_ref().map(|clip| clip.track), Some(Some(0)));
        assert_eq!(t.duration, 30.0);

        let unbanked = assemble(&parts, None, &[], context);
        let s1 = unbanked.speaker("S1").expect("S1");
        assert_eq!((s1.status, &s1.person_id), (MatchStatus::Unknown, &None));
        assert_eq!(unbanked.speaker("ME").and_then(|me| me.name.clone()), None);
        std::fs::remove_dir_all(&root).expect("cleanup");
    }

    /// The far end (x) at 0 s, then two voices on the microphone: `b` says
    /// two words first, `a` talks longer after it.
    fn a_room_on_the_mic() -> [PartResult; 2] {
        [
            part(
                0.0,
                TrackOrigin::System,
                asr("hello from the far end", 0.0, 0.5),
                Some(diar(&[("x", 0.0, 3.0)], &[("x", vec![1.0, 0.0, 0.0])])),
            ),
            part(
                0.0,
                TrackOrigin::Microphone,
                AsrOutput {
                    tokens: asr("me too", 4.0, 0.5)
                        .tokens
                        .into_iter()
                        .chain(asr("yes I am here and listening now", 6.0, 0.5).tokens)
                        .collect(),
                    ..AsrOutput::default()
                },
                Some(diar(
                    &[("b", 4.0, 5.0), ("a", 6.0, 10.0)],
                    &[("a", vec![0.0, 1.0, 0.0]), ("b", vec![0.0, 0.0, 1.0])],
                )),
            ),
        ]
    }

    #[test]
    fn without_a_self_person_the_voice_that_talks_most_on_the_mic_is_me() {
        let t = assemble(
            &a_room_on_the_mic(),
            None,
            &[],
            ctx(&[("screen-0000.mov", 0.0, 30.0, SYSTEM_AND_MIC)]),
        );
        assert_eq!(
            said(&t),
            [
                ("S1", "hello from the far end"),
                ("S2", "me too"),
                ("ME", "yes I am here and listening now"),
            ],
            "the other voice in the room is numbered with the call's"
        );
        let s2 = t.speaker("S2").expect("S2");
        assert_eq!(s2.origin, TrackOrigin::Microphone);
        assert_eq!(t.utterances[1].origin, TrackOrigin::Microphone);
        assert_eq!(
            t.speaker("ME").and_then(|me| me.embedding.clone()),
            Some(vec![0.0, 1.0, 0.0]),
            "ME carries its cluster's embedding"
        );
    }

    #[test]
    fn the_mic_voice_that_is_the_self_person_is_me_and_the_others_are_matched() {
        let root = scratch();
        let bank = bank_of(
            &root,
            &[
                ("A", "Ada", false, vec![0.0, 1.0, 0.0]),
                ("M", "Me Myself", true, vec![0.0, 0.1, 1.0]),
            ],
        );
        let t = assemble(
            &a_room_on_the_mic(),
            Some(&bank),
            &[],
            ctx(&[("screen-0000.mov", 0.0, 30.0, SYSTEM_AND_MIC)]),
        );
        assert_eq!(
            said(&t),
            [
                ("S1", "hello from the far end"),
                ("ME", "me too"),
                ("S2", "yes I am here and listening now"),
            ],
            "the bank's me beats who talks most"
        );
        let me = t.speaker("ME").expect("ME");
        assert_eq!(
            (me.status, me.person_id.as_deref()),
            (MatchStatus::Me, Some("M"))
        );
        let s2 = t.speaker("S2").expect("S2");
        assert_eq!(
            (s2.status, s2.name.as_deref(), s2.origin),
            (MatchStatus::Auto, Some("Ada"), TrackOrigin::Microphone)
        );
        std::fs::remove_dir_all(&root).expect("cleanup");
    }

    #[test]
    fn another_voice_on_the_mic_that_only_echoes_the_far_end_is_no_speaker() {
        let t = assemble(
            &[
                part(
                    0.0,
                    TrackOrigin::System,
                    asr("can you see my screen now", 0.0, 0.4),
                    Some(diar(&[("x", 0.0, 3.0)], &[("x", vec![1.0, 0.0])])),
                ),
                part(
                    0.0,
                    TrackOrigin::Microphone,
                    AsrOutput {
                        tokens: asr("can you see my screen", 0.05, 0.4)
                            .tokens
                            .into_iter()
                            .chain(asr("yes it looks fine to me thanks", 4.0, 0.4).tokens)
                            .collect(),
                        ..AsrOutput::default()
                    },
                    Some(diar(
                        &[("b", 0.0, 2.5), ("a", 4.0, 7.0)],
                        &[("a", vec![0.0, 1.0]), ("b", vec![0.6, 0.8])],
                    )),
                ),
            ],
            None,
            &[],
            ctx(&[]),
        );
        assert_eq!(
            said(&t),
            [
                ("S1", "can you see my screen now"),
                ("ME", "yes it looks fine to me thanks")
            ]
        );
        assert_eq!(
            t.speakers.len(),
            2,
            "no speaker for a voice left with no line"
        );
    }

    #[test]
    fn the_dictionary_rewrites_text_but_asr_text_keeps_what_was_heard() {
        let term = DictionaryTerm {
            version: 1,
            id: "T".to_owned(),
            text: "Gorka".to_owned(),
            aliases: vec!["gorker".to_owned()],
            created_at: String::new(),
        };
        let t = assemble(
            &[part(
                0.0,
                TrackOrigin::Mixed,
                asr("hi gorker", 0.0, 0.5),
                None,
            )],
            None,
            &[term],
            ctx(&[]),
        );
        assert_eq!(t.utterances[0].text, "hi Gorka");
        assert_eq!(t.utterances[0].asr_text, "hi gorker");
        assert_eq!(t.dictionary_applied[0].count, 1);
    }

    #[test]
    fn the_best_clip_is_the_longest_stretch_nobody_talks_over_capped_at_fifteen_seconds() {
        let words = |from: f64, to: f64| -> String {
            let count = ((to - from) / 0.5) as usize;
            vec!["la"; count].join(" ")
        };
        let t = assemble(
            &[
                part(
                    100.0,
                    TrackOrigin::System,
                    AsrOutput {
                        tokens: asr(&words(0.0, 30.0), 0.0, 0.5)
                            .tokens
                            .into_iter()
                            .chain(asr(&words(40.0, 45.0), 40.0, 0.5).tokens)
                            .collect(),
                        ..AsrOutput::default()
                    },
                    Some(diar(&[("x", 0.0, 50.0)], &[("x", vec![1.0])])),
                ),
                part(
                    100.0,
                    TrackOrigin::Microphone,
                    asr("wait what", 10.0, 0.5),
                    None,
                ),
            ],
            None,
            &[],
            ctx(&[
                ("screen-0000.mov", 0.0, 100.0, SYSTEM_AND_MIC),
                ("screen-0001.mov", 100.0, 100.0, SYSTEM_AND_MIC),
            ]),
        );
        let clip = t.speaker("S1").and_then(|s| s.clip.clone()).expect("clip");
        assert_eq!(clip.file, "screen-0001.mov");
        assert_eq!(clip.track, Some(0));
        // 0–19.9 s (40 words) is talked over by the mic at 10 s; 20–29.9 s and
        // 40–44.9 s are clean, and the longer one wins.
        assert!(
            (clip.start - 20.0).abs() < 1e-9 && (clip.end - 29.9).abs() < 1e-9,
            "{clip:?}"
        );
        assert_eq!(
            t.speaker("ME").and_then(|s| s.clip.clone()),
            None,
            "under 2 s is no clip"
        );

        let long = assemble(
            &[part(
                0.0,
                TrackOrigin::Mixed,
                asr(&words(0.0, 30.0), 0.0, 0.5),
                None,
            )],
            None,
            &[],
            ctx(&[("a.m4a", 0.0, 30.0, &[(None, TrackOrigin::Mixed)])]),
        );
        let clip = long.speakers[0].clip.clone().expect("clip");
        assert!((clip.end - clip.start - CLIP_MAX_S).abs() < 1e-9);
        assert_eq!(clip.track, None);
    }
}
