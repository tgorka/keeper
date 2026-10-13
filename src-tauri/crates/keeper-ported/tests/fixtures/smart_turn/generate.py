"""Regenerate the smart_turn golden fixtures from upstream's own code path.

Run with a Python that has `numpy`, `pyarrow`, `soundfile` and `transformers`
and no torch, so the feature extractor takes its numpy path:

    python generate.py <librispeech_asr_dummy clean/validation parquet>

Each clip is written as `<name>.s16` (16 kHz mono, little-endian i16, as the
corpus stores it) and its features as `<name>.f32` (little-endian f32,
`[80][800]`, bin-major). The input is the one smart-turn's `inference.py` at
4786657e hands the extractor — the clip's last 8 s, left-padded with zeros —
prepared here in keeper's own code from that contract, then
`WhisperFeatureExtractor(chunk_length=8)` with the same arguments.
"""

import io
import sys

import numpy as np
import pyarrow.parquet as pq
import soundfile as sf
import transformers
from transformers import WhisperFeatureExtractor

CLIPS = {
    "short": "1272-135031-0012",
    "long": "1272-128104-0005",
}

RATE = 16000
SAMPLES = 8 * RATE


def last_eight_seconds(audio):
    """The window the end-of-turn model hears: the clip's last 8 s, with
    zeros before a shorter clip."""
    window = np.zeros(SAMPLES, dtype=audio.dtype)
    kept = audio[-SAMPLES:]
    window[SAMPLES - len(kept):] = kept
    return window


def main(parquet):
    rows = {row["id"]: row for row in pq.read_table(parquet).to_pylist()}
    extractor = WhisperFeatureExtractor(chunk_length=8)
    for name, clip in CLIPS.items():
        pcm, rate = sf.read(io.BytesIO(rows[clip]["audio"]["bytes"]), dtype="int16")
        assert rate == 16000
        pcm.astype("<i2").tofile(f"{name}.s16")
        audio = last_eight_seconds(pcm.astype(np.float32) / 32768.0)
        features = extractor(
            audio,
            sampling_rate=16000,
            return_tensors="np",
            padding="max_length",
            max_length=8 * 16000,
            truncation=True,
            do_normalize=True,
        ).input_features.squeeze(0).astype("<f4")
        assert features.shape == (80, 800)
        features.tofile(f"{name}.f32")
        print(name, clip, len(pcm), features.min(), features.max())
    print("transformers", transformers.__version__, "numpy", np.__version__)


if __name__ == "__main__":
    main(sys.argv[1])
