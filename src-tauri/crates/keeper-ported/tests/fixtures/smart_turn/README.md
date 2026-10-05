# smart_turn golden fixtures

`smart_turn_features_match_upstream` (`tests/smart_turn.rs`) compares
`keeper_ported::smart_turn::Features` with these files, within 1e-4 per bin.

| file | what |
| --- | --- |
| `short.s16` | LibriSpeech `1272-135031-0012`, "where is my brother now", 2.04 s (32,640 samples) — padded to 8 s on the left |
| `long.s16` | LibriSpeech `1272-128104-0005`, 9.01 s (144,160 samples) — truncated to its last 8 s |
| `short.f32`, `long.f32` | the features upstream computes for each, `[80][800]` bin-major, little-endian f32 |
| `generate.py` | the script that wrote all four |

The waveforms are 16 kHz mono little-endian i16, exactly as the corpus stores them; both sides
read a sample as `i16 / 32768`.

**How the features were made (2026-10-05).** `python generate.py <parquet>` with Python 3.12,
`transformers` 5.18.0 (tag `v5.18.0`), `numpy` 2.5.3, and no torch, so `WhisperFeatureExtractor`
took its numpy path (`_np_extract_fbank_features`), the one smart-turn's
`requirements_inference.txt` installs. The call is the one smart-turn's `inference.py` makes at
`4786657e`: the clip's last 8 s, left-padded with zeros, then
`WhisperFeatureExtractor(chunk_length=8)(audio, sampling_rate=16000, return_tensors="np",
padding="max_length", max_length=128000, truncation=True, do_normalize=True)`. The script
prepares that window in its own code from the contract (`last_eight_seconds`); it contains no
smart-turn code. The fixtures were written by an earlier revision of the script that carried
smart-turn's `truncate_audio_to_last_n_seconds` (BSD-2-Clause); on 2026-10-05 the two were checked
to produce identical arrays (same dtype, shape and every sample, under numpy 2.5.3) for clips of
1, 32,640, 127,999, 128,000, 128,001 and 144,160 samples, so the fixtures stand.

**The audio.** From the LibriSpeech ASR corpus (Vassil Panayotov, Guoguo Chen, Daniel Povey,
Sanjeev Khudanpur; openslr.org/12), licensed CC BY 4.0, read from Hugging Face's
`hf-internal-testing/librispeech_asr_dummy` at revision `5be91486e11a2d616f4ec5db8d3fd248585ac07a`
(`clean/validation-00000-of-00001.parquet`, sha256
`4e69a06fa5edc90921e5e7e39a7084881f8b3ed9c805c574f4f39c6fde27c603`). The two clips are unchanged
apart from being stored as raw samples.

sha256:

```
7f8441bfa16399affc55725cad534b3a922a8249281a8057e0060b0251e32674  short.s16
da7233631be52abe59532ee53d405d90048b5b56655bb671e4e08967002129d1  short.f32
0b04bfab58601e3acb9bf6d06575a4470f651e23557efcb5e578b66b1ee3eaa4  long.s16
3ba1ad3c118fe6e74eb4abc514aba461b67e6542a923c051f8200af813cbdcd8  long.f32
```
