//! How far a transcription job has come, as one number a progress bar can
//! show. The engine reports no progress of its own, so the fraction is an
//! estimate: each part's share of the job is its audio time (times the
//! tracks heard in it), each step's share of a track is fixed, and how far
//! into a step the job is comes from the time spent in it against the time
//! the step is expected to take on that much audio.

use super::vm::TranscriptionPhase;

/// Seconds of work per second of audio, measured on an M-series Mac.
/// Uncalibrated beyond that one machine.
pub const RTF_DECODE: f64 = 0.001;
pub const RTF_ASR: f64 = 0.02;
pub const RTF_DIARIZE: f64 = 0.006;

/// Each step's share of the whole job. The three per-track steps are split
/// over the parts by audio time; matching and writing happen once, at the end.
const SHARE_DECODE: f64 = 0.05;
const SHARE_ASR: f64 = 0.60;
const SHARE_DIARIZE: f64 = 0.25;
const SHARE_FINISH: f64 = 0.10;

/// A step never claims more than this share of itself until it ends: an
/// estimate that ran out must not show a job that is still working as done.
const STEP_CAP: f64 = 0.95;

/// Matching and writing do not scale with audio time; this is what they are
/// expected to take.
const FINISH_EXPECTED_S: f64 = 5.0;
/// No step is expected to take less than this, so a part of unknown length
/// still moves the bar gradually.
const MIN_EXPECTED_S: f64 = 0.5;

/// One part as the job will hear it.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PartLoad {
    /// Seconds of audio.
    pub duration: f64,
    /// Tracks heard one after another.
    pub tracks: usize,
}

/// A job's progress estimate; it never goes backwards.
#[derive(Debug, Clone, PartialEq)]
pub struct Estimate {
    parts: Vec<PartLoad>,
    /// Each part's share of the per-track steps, summing to 1.
    weights: Vec<f64>,
    shown: f32,
}

impl Estimate {
    pub fn new(parts: &[PartLoad]) -> Self {
        let load = |part: &PartLoad| part.duration.max(0.0) * part.tracks.max(1) as f64;
        let total: f64 = parts.iter().map(load).sum();
        let weights = parts
            .iter()
            .map(|part| {
                if total > 0.0 {
                    load(part) / total
                } else {
                    1.0 / parts.len() as f64
                }
            })
            .collect();
        Self {
            parts: parts.to_vec(),
            weights,
            shown: 0.0,
        }
    }

    /// The fraction `elapsed_s` into `phase` of `track` (0-based) of `part`
    /// (0-based), never less than any fraction answered before.
    pub fn at(
        &mut self,
        phase: TranscriptionPhase,
        part: usize,
        track: usize,
        elapsed_s: f64,
    ) -> f32 {
        let raw = match phase {
            TranscriptionPhase::Queued => 0.0,
            TranscriptionPhase::Decoding => self.during(part, track, 0, elapsed_s),
            TranscriptionPhase::Transcribing => self.during(part, track, 1, elapsed_s),
            TranscriptionPhase::Diarizing => self.during(part, track, 2, elapsed_s),
            TranscriptionPhase::Matching | TranscriptionPhase::Writing => {
                1.0 - SHARE_FINISH + SHARE_FINISH * into(elapsed_s, FINISH_EXPECTED_S)
            }
            TranscriptionPhase::Done => 1.0,
            TranscriptionPhase::Failed | TranscriptionPhase::Cancelled => 0.0,
        };
        #[allow(clippy::cast_possible_truncation)]
        let raw = raw.clamp(0.0, 1.0) as f32;
        self.shown = self.shown.max(raw);
        self.shown
    }

    /// Everything before step `step` (0 decode, 1 ASR, 2 diarize) of `track`
    /// of `part`, plus the part of that step `elapsed_s` is expected to be.
    fn during(&self, part: usize, track: usize, step: usize, elapsed_s: f64) -> f64 {
        const STEPS: [(f64, f64); 3] = [
            (SHARE_DECODE, RTF_DECODE),
            (SHARE_ASR, RTF_ASR),
            (SHARE_DIARIZE, RTF_DIARIZE),
        ];
        let Some(load) = self.parts.get(part) else {
            return 1.0 - SHARE_FINISH;
        };
        let before: f64 = self.weights[..part].iter().sum::<f64>() * (1.0 - SHARE_FINISH);
        let tracks = load.tracks.max(1);
        let track = track.min(tracks - 1);
        let per_track = self.weights[part] / tracks as f64;
        let done_tracks = per_track * track as f64 * (1.0 - SHARE_FINISH);
        let done_steps: f64 = STEPS[..step].iter().map(|(share, _)| share).sum::<f64>() * per_track;
        let (share, rtf) = STEPS[step];
        let current = share * per_track * into(elapsed_s, load.duration * rtf);
        before + done_tracks + done_steps + current
    }
}

/// How far into a step of `expected_s` seconds `elapsed_s` is, capped.
fn into(elapsed_s: f64, expected_s: f64) -> f64 {
    (elapsed_s.max(0.0) / expected_s.max(MIN_EXPECTED_S)).min(STEP_CAP)
}

#[cfg(test)]
mod tests {
    use super::*;
    use TranscriptionPhase as P;

    fn close(left: f32, right: f64) -> bool {
        (f64::from(left) - right).abs() < 1e-4
    }

    #[test]
    fn steps_split_the_job_by_audio_time_and_their_own_shares() {
        // Part 1: 100 s, one track. Part 2: 300 s, one track. Weights 1/4, 3/4.
        let mut estimate = Estimate::new(&[
            PartLoad {
                duration: 100.0,
                tracks: 1,
            },
            PartLoad {
                duration: 300.0,
                tracks: 1,
            },
        ]);
        assert!(close(estimate.at(P::Decoding, 0, 0, 0.0), 0.0));
        // ASR of part 1 expected 2 s; 1 s in is half of its 0.6 × 0.25.
        assert!(close(
            estimate.at(P::Transcribing, 0, 0, 1.0),
            0.25 * (0.05 + 0.30)
        ));
        // Part 2 starts after part 1's 0.9 × 0.25.
        assert!(close(estimate.at(P::Decoding, 1, 0, 0.0), 0.225));
        assert!(close(estimate.at(P::Matching, 1, 0, 0.0), 0.9));
        assert!(close(estimate.at(P::Done, 0, 0, 0.0), 1.0));
    }

    #[test]
    fn a_step_that_overruns_its_estimate_stops_short_of_its_end() {
        let mut estimate = Estimate::new(&[PartLoad {
            duration: 100.0,
            tracks: 1,
        }]);
        // ASR expected 2 s; after an hour it still claims only 95 % of itself.
        let stuck = estimate.at(P::Transcribing, 0, 0, 3600.0);
        assert!(close(stuck, 0.05 + 0.6 * 0.95));
        assert!(stuck < estimate.clone().at(P::Diarizing, 0, 0, 0.0));
    }

    #[test]
    fn the_estimate_never_goes_backwards() {
        let mut estimate = Estimate::new(&[PartLoad {
            duration: 60.0,
            tracks: 2,
        }]);
        let late = estimate.at(P::Diarizing, 0, 1, 10.0);
        assert!(late > 0.8);
        assert_eq!(
            estimate.at(P::Decoding, 0, 0, 0.0),
            late,
            "an earlier step does not pull it back"
        );
        assert_eq!(estimate.at(P::Failed, 0, 0, 0.0), late);
    }

    #[test]
    fn a_second_track_starts_after_the_first_tracks_share() {
        let mut estimate = Estimate::new(&[PartLoad {
            duration: 60.0,
            tracks: 2,
        }]);
        assert!(close(estimate.at(P::Decoding, 0, 1, 0.0), 0.45));
    }
}
