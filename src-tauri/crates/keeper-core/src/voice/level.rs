//! The input level (Epic 64, Story 64.3, FR-433, AD-186): a bounded,
//! smoothed number a surface can draw, from the RMS of each tap buffer.
//!
//! The tap on the port hands the recogniser every buffer and, since this
//! story, also hands each buffer's RMS to a [`Meter`]. The meter is the
//! whole of the decision — what the number means, how it moves, how often
//! it may be reported — and it is pure: time comes in as an argument, so the
//! same sequence of buffers gives the same sequence of readings on the dev
//! host as on the audio thread.
//!
//! # What the number means
//!
//! `0.0` is silence and `1.0` is full scale, on a decibel scale with a floor
//! at [`FLOOR_DBFS`]: a level meter is read by eye, and the ear hears in
//! decibels, so a linear RMS would sit near zero for all ordinary speech and
//! jump for a shout. The floor is where a quiet room sits.
//!
//! # How it moves
//!
//! Fast attack, slow release ([`ATTACK`], [`RELEASE`]): a word must show the
//! moment it is spoken, and the fall after it must be slow enough that the
//! meter does not flicker between syllables. Superwhisper's waveform, the
//! reference the owner named, is a diagnostic — "if it stays static, check
//! your input device" — and a meter that jumps at 48 Hz would fail that
//! purpose by being unreadable.
//!
//! # How often it is reported
//!
//! [`Meter::feed`] answers `Some` at most once per [`INTERVAL`] and only
//! when the smoothed value has moved by at least [`MIN_STEP`] since the last
//! answer. The tap runs at roughly 48 buffers a second; the surface needs
//! about 25 readings a second to look alive and nothing above that, and a
//! reading that has not changed is not worth an IPC message. This limiter is
//! what makes the port's sink cheap enough to call from the tap at all.

use std::time::Duration;

/// Below this the meter reads `0.0`. −60 dBFS is where the noise of a quiet
/// room with a laptop microphone sits; speech at a normal distance is
/// −30 … −12 dBFS, so the useful range is the upper half of the meter.
pub const FLOOR_DBFS: f32 = -60.0;

/// Time constant of the rise: about two buffers, so the first syllable of a
/// word registers within the tap's own latency rather than a beat after it.
pub const ATTACK: Duration = Duration::from_millis(40);

/// Time constant of the fall: long enough to bridge the gap between
/// syllables (~100–200 ms in speech) so a word reads as one shape, short
/// enough that the meter is at the floor within a second of silence.
pub const RELEASE: Duration = Duration::from_millis(250);

/// The shortest gap between two readings the meter answers: 25 Hz, the
/// bounded rate AD-186 asks for. Below ~20 Hz the meter looks like it
/// stutters; above ~30 Hz no eye tells the difference.
pub const INTERVAL: Duration = Duration::from_millis(40);

/// The smallest movement worth a reading: 0.6 dB on the 60 dB scale, under
/// the resolution of a meter a few hundred pixels wide.
pub const MIN_STEP: f32 = 0.01;

/// A smoothed, rate-limited level from RMS samples.
///
/// One per capture; the port makes it when the tap is installed and drops
/// it with the tap. `now` on each call is any monotonic clock, in any epoch,
/// as long as it is the same one for the life of the meter.
#[derive(Debug, Clone, Default)]
pub struct Meter {
    /// The smoothed level in `0.0..=1.0`.
    value: f32,
    /// When the last buffer was fed, for the smoothing step.
    fed_at: Option<Duration>,
    /// The last reading answered, and when.
    reported: Option<(f32, Duration)>,
}

impl Meter {
    /// A meter at silence that has heard nothing.
    pub fn new() -> Self {
        Self::default()
    }

    /// The smoothed level right now, whether or not it was reported.
    pub fn value(&self) -> f32 {
        self.value
    }

    /// Feed one buffer's RMS (linear, `0.0` silence, `1.0` full scale)
    /// measured at `now`. Answers the reading to report, or `None` when the
    /// limiter holds it back.
    pub fn feed(&mut self, rms: f32, now: Duration) -> Option<f32> {
        let target = scale(rms);
        self.value = match self.fed_at {
            // The first buffer sets the level outright: there is nothing to
            // smooth from, and a meter that rose from zero over 40 ms would
            // report a silence nobody heard.
            None => target,
            Some(before) => {
                let dt = now.saturating_sub(before);
                let tau = if target > self.value { ATTACK } else { RELEASE };
                self.value + (target - self.value) * step(dt, tau)
            }
        };
        self.fed_at = Some(now);

        let due = match self.reported {
            None => true,
            Some((last, at)) => {
                now.saturating_sub(at) >= INTERVAL && (self.value - last).abs() >= MIN_STEP
            }
        };
        if due {
            self.reported = Some((self.value, now));
            Some(self.value)
        } else {
            None
        }
    }
}

/// RMS to the meter's scale: decibels relative to full scale, floored at
/// [`FLOOR_DBFS`], mapped linearly onto `0.0..=1.0`.
fn scale(rms: f32) -> f32 {
    if rms <= 0.0 {
        return 0.0;
    }
    let dbfs = 20.0 * rms.log10();
    ((dbfs - FLOOR_DBFS) / -FLOOR_DBFS).clamp(0.0, 1.0)
}

/// The fraction of the distance to the target one step of `dt` covers, for
/// a first-order smoother with time constant `tau`: `1 − e^(−dt/τ)`.
fn step(dt: Duration, tau: Duration) -> f32 {
    1.0 - (-dt.as_secs_f32() / tau.as_secs_f32()).exp()
}

/// The RMS of one buffer of float samples, `0.0` for an empty one.
///
/// Here rather than in the port so that what "the level of a buffer" means
/// is written once and tested; the port's only job is to hand the samples
/// over — as a slice for a non-interleaved buffer, as every `stride`-th
/// sample for an interleaved one.
pub fn rms(samples: impl IntoIterator<Item = f32>) -> f32 {
    let (sum, count) = samples
        .into_iter()
        .fold((0.0f32, 0usize), |(sum, count), s| (sum + s * s, count + 1));
    if count == 0 {
        0.0
    } else {
        (sum / count as f32).sqrt()
    }
}
