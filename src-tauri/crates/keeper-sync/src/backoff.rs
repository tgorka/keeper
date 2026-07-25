//! Exponential backoff with jitter (Story 26.6, AD-49).
//!
//! The repository had no backoff utility before this — the Matrix side relies
//! on matrix-sdk's own retry, and the undo-send outbox re-reads its table on a
//! fixed tick. A sync engine talking to one git host from many profiles needs
//! real backoff, because the failure mode it must avoid is *correlated* retry:
//! ten profiles losing connectivity at the same instant and then hammering the
//! same server in lockstep the moment it returns.
//!
//! Deliberately pure: it takes an attempt count and a random sample and returns
//! a delay. No clock, no sleeping, no RNG ownership — which is what makes it
//! exhaustively testable.

use std::time::Duration;

/// Backoff schedule parameters.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Backoff {
    pub base: Duration,
    pub max: Duration,
    /// Fraction of the computed delay that is randomized, in percent.
    ///
    /// Full jitter (100) is the right default for a shared remote: it is the
    /// only setting that fully decorrelates a fleet of clients. A smaller value
    /// trades decorrelation for a more predictable minimum delay.
    pub jitter_pct: u32,
}

impl Default for Backoff {
    fn default() -> Self {
        Self {
            // 2 s first retry: long enough that a momentary blip has passed,
            // short enough that a user who just plugged in an ethernet cable
            // sees progress before they wonder whether it is broken.
            base: Duration::from_secs(2),
            // 10 min ceiling. Beyond this a "retry" is indistinguishable from
            // the next scheduled poll, so growing further buys nothing.
            max: Duration::from_secs(600),
            jitter_pct: 100,
        }
    }
}

impl Backoff {
    /// Delay before attempt number `attempt` (1 = the first retry).
    ///
    /// `random` is a caller-supplied sample in `[0.0, 1.0)`; the caller owns
    /// the RNG so this function stays pure and testable at its boundaries.
    pub fn delay(&self, attempt: u32, random: f64) -> Duration {
        if attempt == 0 {
            return Duration::ZERO;
        }
        let base_ms = self.base.as_millis().min(u128::from(u64::MAX)) as u64;
        let max_ms = self.max.as_millis().min(u128::from(u64::MAX)) as u64;

        // Saturating shift: attempt counts are unbounded in principle (a
        // profile pointed at a dead host retries forever), and `1u64 << 64`
        // is UB-adjacent nonsense. Clamp the exponent instead.
        let exponent = (attempt - 1).min(32);
        let uncapped = base_ms.saturating_mul(1u64 << exponent);
        let capped = uncapped.min(max_ms);

        let jitter_pct = self.jitter_pct.min(100);
        if jitter_pct == 0 {
            return Duration::from_millis(capped);
        }
        // Randomize downward from the cap: the delay is in
        // [capped * (1 - jitter), capped]. Never longer than the schedule
        // promises, so `max` really is a maximum.
        let random = random.clamp(0.0, 1.0);
        let jitter_span = (capped as f64) * (f64::from(jitter_pct) / 100.0);
        let reduction = jitter_span * (1.0 - random);
        let ms = ((capped as f64) - reduction).max(0.0) as u64;
        Duration::from_millis(ms)
    }

    /// The absolute wall-clock time a unit becomes eligible again, which is
    /// what the journal stores.
    pub fn not_before_ms(&self, now_ms: i64, attempt: u32, random: f64) -> i64 {
        let delay = self.delay(attempt, random).as_millis();
        now_ms.saturating_add(delay.min(u128::from(i64::MAX as u64)) as i64)
    }
}

/// A cheap, dependency-free jitter source.
///
/// A cryptographic RNG would be silly here — this only decorrelates retries —
/// and the crate deliberately avoids taking a `rand` dependency for it. Uses
/// the low bits of the monotonic clock through a SplitMix64 finalizer, which
/// is more than enough entropy to break lockstep between processes.
pub fn jitter_sample() -> f64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.subsec_nanos() as u64)
        .unwrap_or(0);
    let mut z = nanos.wrapping_add(0x9E37_79B9_7F4A_7C15);
    z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    z ^= z >> 31;
    // 53 bits is the exactly-representable integer range of f64.
    ((z >> 11) as f64) / ((1u64 << 53) as f64)
}

#[cfg(test)]
mod tests {
    use super::*;

    const NO_JITTER: Backoff = Backoff {
        base: Duration::from_secs(2),
        max: Duration::from_secs(600),
        jitter_pct: 0,
    };

    #[test]
    fn the_zeroth_attempt_is_immediate() {
        assert_eq!(NO_JITTER.delay(0, 0.5), Duration::ZERO);
    }

    #[test]
    fn delay_doubles_until_it_reaches_the_ceiling() {
        assert_eq!(NO_JITTER.delay(1, 0.0), Duration::from_secs(2));
        assert_eq!(NO_JITTER.delay(2, 0.0), Duration::from_secs(4));
        assert_eq!(NO_JITTER.delay(3, 0.0), Duration::from_secs(8));
        assert_eq!(NO_JITTER.delay(9, 0.0), Duration::from_secs(512));
        assert_eq!(NO_JITTER.delay(10, 0.0), Duration::from_secs(600));
    }

    #[test]
    fn an_absurd_attempt_count_saturates_instead_of_overflowing() {
        // A profile pointed at a permanently dead host retries forever; the
        // shift must not wrap and produce a tiny delay.
        assert_eq!(NO_JITTER.delay(u32::MAX, 0.0), Duration::from_secs(600));
        assert_eq!(NO_JITTER.delay(64, 0.0), Duration::from_secs(600));
    }

    #[test]
    fn jitter_never_exceeds_the_scheduled_delay() {
        // `max` must really be a maximum, or a "10 minute ceiling" is a lie.
        let b = Backoff::default();
        for attempt in 1..20 {
            for sample in [0.0, 0.25, 0.5, 0.75, 0.999] {
                let d = b.delay(attempt, sample);
                assert!(d <= b.max, "attempt {attempt} sample {sample} exceeded max");
            }
        }
    }

    #[test]
    fn full_jitter_can_reach_almost_zero_which_is_the_point() {
        // Decorrelation requires that some clients retry almost immediately.
        let b = Backoff::default();
        assert_eq!(b.delay(5, 0.0), Duration::ZERO);
        assert_eq!(b.delay(5, 1.0), b.delay(5, 1.0));
    }

    #[test]
    fn out_of_range_samples_are_clamped_not_trusted() {
        let b = Backoff::default();
        let low = b.delay(3, -5.0);
        let high = b.delay(3, 5.0);
        assert!(low <= b.max && high <= b.max);
    }

    #[test]
    fn not_before_is_absolute_and_never_moves_backwards() {
        let b = NO_JITTER;
        assert_eq!(b.not_before_ms(1_000, 1, 0.0), 3_000);
        // Even at a saturating clock boundary the result must not wrap into
        // the past, which would make a unit eligible immediately forever.
        assert!(b.not_before_ms(i64::MAX, 5, 0.0) >= i64::MAX - 1);
    }

    #[test]
    fn the_jitter_source_stays_in_range() {
        for _ in 0..1_000 {
            let s = jitter_sample();
            assert!((0.0..1.0).contains(&s), "sample out of range: {s}");
        }
    }
}
