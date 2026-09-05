//! Story 64.3 (AD-186): the input level is bounded in value and in rate,
//! moves fast up and slowly down, and sits at zero for a quiet room — all
//! decided in `keeper_core::voice::level` and pinned here, because the tap
//! that feeds it runs on an audio thread nobody can test.

use std::time::Duration;

use keeper_core::voice::level::{rms, Meter, ATTACK, FLOOR_DBFS, INTERVAL, MIN_STEP, RELEASE};

/// One tap buffer at 48 kHz: 1024 frames ≈ 21 ms.
const BUFFER: Duration = Duration::from_micros(21_333);

/// Feed `n` buffers of `input` (a function of the buffer index) and collect
/// every reading the meter answers.
fn readings(meter: &mut Meter, n: u32, input: impl Fn(u32) -> f32) -> Vec<f32> {
    (0..n)
        .filter_map(|i| meter.feed(input(i), BUFFER * i))
        .collect()
}

/// N buffers in, at most ⌈N·21 ms / interval⌉ readings out — with an input
/// that never stops moving, so the limiter and not the "only when it moved"
/// rule is what bounds it.
#[test]
fn voice_level_is_rate_limited() {
    let n = 240; // ~5 s of audio
    let mut meter = Meter::new();
    let out = readings(&mut meter, n, |i| if i % 2 == 0 { 0.5 } else { 0.005 });
    let ceiling = (n as f64 * BUFFER.as_secs_f64() / INTERVAL.as_secs_f64()).ceil() as usize;
    assert!(
        out.len() <= ceiling,
        "{} readings from {n} buffers; the ceiling is {ceiling}",
        out.len()
    );
    // And the limiter did not simply swallow the stream: an input that
    // alternates every buffer keeps the meter moving, so readings keep coming
    // at roughly the interval.
    assert!(
        out.len() >= ceiling / 2,
        "{} readings from {n} buffers is too few for a moving input",
        out.len()
    );
    assert!(out.iter().all(|v| (0.0..=1.0).contains(v)), "{out:?}");
}

/// A constant input converges monotonically to one value and, once there,
/// produces no more readings.
#[test]
fn voice_level_converges_monotonically_and_then_goes_quiet() {
    let mut meter = Meter::new();
    // A step from silence to −20 dBFS. The first buffer after the step is
    // taken outright (nothing to smooth from), so start at silence first.
    let silence = meter.feed(0.0, Duration::ZERO);
    assert_eq!(silence, Some(0.0), "the first reading is answered at once");
    let out = readings(&mut meter, 200, |_| 0.1);
    let out: Vec<f32> = out.into_iter().skip_while(|v| *v == 0.0).collect();
    assert!(
        out.len() >= 3,
        "expected a rise over several readings: {out:?}"
    );
    for pair in out.windows(2) {
        assert!(pair[0] < pair[1], "not monotone: {out:?}");
    }
    let target = (20.0 * 0.1f32.log10() - FLOOR_DBFS) / -FLOOR_DBFS; // 2/3
    assert!(
        out.iter().all(|v| *v <= target + 1e-6),
        "overshot {target}: {out:?}"
    );
    assert!(
        (out.last().copied().unwrap_or_default() - target).abs() < 2.0 * MIN_STEP,
        "did not reach {target}: {out:?}"
    );
    // Quiet once converged: the last 100 buffers of the same input answered
    // nothing, so the readings all came from the first second.
    let mut still = meter.clone();
    let late = readings(&mut still, 50, |_| 0.1);
    assert!(late.is_empty(), "a settled level keeps reporting: {late:?}");
}

/// Silence and anything under the floor read as exactly zero; full scale
/// reads as exactly one.
#[test]
fn voice_level_has_a_floor_and_a_ceiling() {
    let mut meter = Meter::new();
    assert_eq!(meter.feed(0.0, Duration::ZERO), Some(0.0));
    let mut meter = Meter::new();
    assert_eq!(
        meter.feed(0.000_5, Duration::ZERO),
        Some(0.0),
        "−66 dBFS is under the floor"
    );
    let mut meter = Meter::new();
    assert_eq!(meter.feed(1.0, Duration::ZERO), Some(1.0));
    let mut meter = Meter::new();
    assert_eq!(meter.feed(3.0, Duration::ZERO), Some(1.0), "clipped input");
}

/// The rise is fast and the fall is slow: after one release time constant
/// of silence the level has dropped to about a third, after one attack time
/// constant of sound it has risen to about two thirds.
#[test]
fn voice_level_attacks_fast_and_releases_slowly() {
    assert!(ATTACK < RELEASE);
    let mut meter = Meter::new();
    meter.feed(1.0, Duration::ZERO);
    // 1 ms steps so the discrete smoother tracks the continuous one closely.
    let step = Duration::from_millis(1);
    let mut now = Duration::ZERO;
    for _ in 0..RELEASE.as_millis() {
        now += step;
        meter.feed(0.0, now);
    }
    let after_release = meter.value();
    assert!(
        (after_release - (-1.0f32).exp()).abs() < 0.02,
        "after one release constant: {after_release}"
    );

    let mut meter = Meter::new();
    meter.feed(0.0, Duration::ZERO);
    let mut now = Duration::ZERO;
    for _ in 0..ATTACK.as_millis() {
        now += step;
        meter.feed(1.0, now);
    }
    let after_attack = meter.value();
    assert!(
        (after_attack - (1.0 - (-1.0f32).exp())).abs() < 0.02,
        "after one attack constant: {after_attack}"
    );
}

/// The RMS of a buffer, as the tap computes it — over a whole buffer or
/// over one channel of an interleaved one.
#[test]
fn voice_level_rms_of_a_buffer() {
    assert_eq!(rms([]), 0.0);
    assert_eq!(rms([0.0; 1024]), 0.0);
    assert!((rms([0.5, -0.5, 0.5, -0.5]) - 0.5).abs() < 1e-6);
    assert!((rms([1.0, 0.0]) - std::f32::consts::FRAC_1_SQRT_2).abs() < 1e-6);
    let interleaved = [0.5f32, 0.0, -0.5, 0.0, 0.5, 0.0];
    assert!((rms(interleaved.iter().copied().step_by(2)) - 0.5).abs() < 1e-6);
}
