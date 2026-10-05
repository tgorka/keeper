//! `smart_turn::Features` against the features upstream's own code computes
//! for the same audio (`tests/fixtures/smart_turn/README.md`).

use std::path::PathBuf;

use keeper_ported::smart_turn::{Features, FEATURES, FRAMES, MEL_BINS};

/// The largest difference allowed in any one bin.
const TOLERANCE: f32 = 1e-4;

fn fixture(name: &str) -> Vec<u8> {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/smart_turn")
        .join(name);
    std::fs::read(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()))
}

fn samples(name: &str) -> Vec<f32> {
    fixture(name)
        .as_chunks::<2>()
        .0
        .iter()
        .map(|pair| f32::from(i16::from_le_bytes(*pair)) / 32768.0)
        .collect()
}

fn expected(name: &str) -> Vec<f32> {
    fixture(name)
        .as_chunks::<4>()
        .0
        .iter()
        .map(|word| f32::from_le_bytes(*word))
        .collect()
}

/// Where `got` and `want` differ by more than [`TOLERANCE`], or either is
/// not a finite number, as `(bin, frame, got, want)`, the worst first.
fn differences(got: &[f32], want: &[f32]) -> Vec<(usize, usize, f32, f32)> {
    let mut off: Vec<_> = got
        .iter()
        .zip(want)
        .enumerate()
        .filter(|(_, (g, w))| !(g.is_finite() && w.is_finite() && (*g - *w).abs() <= TOLERANCE))
        .map(|(at, (g, w))| (at / FRAMES, at % FRAMES, *g, *w))
        .collect();
    off.sort_by(|a, b| (b.2 - b.3).abs().total_cmp(&(a.2 - a.3).abs()));
    off.truncate(5);
    off
}

/// A NaN or an infinity is never within tolerance of anything — not even
/// of itself — while a difference inside the tolerance still passes.
#[test]
fn smart_turn_parity_refuses_non_finite_bins() {
    let want = [0.0, 0.0, 0.0, f32::NAN, 1.0];
    let got = [
        f32::NAN,
        f32::INFINITY,
        f32::NEG_INFINITY,
        f32::NAN,
        1.0 + TOLERANCE / 2.0,
    ];
    let mut off: Vec<usize> = differences(&got, &want)
        .iter()
        .map(|(bin, frame, _, _)| bin * FRAMES + frame)
        .collect();
    off.sort_unstable();
    assert_eq!(off, vec![0, 1, 2, 3]);
}

/// One extractor for both clips, the long one first, as the end-of-turn
/// worker keeps one: the short clip's padding must not hear the long one.
#[test]
fn smart_turn_features_match_upstream() {
    let mut features = Features::new();
    let mut out = Box::new([0.0f32; FEATURES]);
    for (clip, length) in [("long", 144_160), ("short", 32_640)] {
        let audio = samples(&format!("{clip}.s16"));
        assert_eq!(
            audio.len(),
            length,
            "{clip}: the fixture is the clip it names"
        );
        let want = expected(&format!("{clip}.f32"));
        assert_eq!(want.len(), MEL_BINS * FRAMES);
        features.compute(&audio, &mut out);
        assert_eq!(differences(&out[..], &want), Vec::new(), "{clip}");
    }
}
