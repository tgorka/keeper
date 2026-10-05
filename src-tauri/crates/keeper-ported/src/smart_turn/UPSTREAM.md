repository: https://github.com/huggingface/transformers
commit: a906d3c4b65095f2308b6a6a193e934d03b8eb5d
licence: Apache-2.0
copyright: Copyright 2022 The HuggingFace Inc. team (feature_extraction_whisper.py); Copyright 2023 The HuggingFace Inc. team and the librosa & torchaudio authors (audio_utils.py)
files read: src/transformers/models/whisper/feature_extraction_whisper.py (last changed bb3ffb97), src/transformers/audio_utils.py, src/transformers/feature_extraction_sequence_utils.py, LICENSE; and, for the call contract only, pipecat-ai/smart-turn at 4786657e inference.py and audio_utils.py (BSD-2-Clause, Copyright (c) 2024–2025, Daily)
ported: WhisperFeatureExtractor's numpy path as smart-turn calls it — zero_mean_unit_var_norm over the padded 8 s, window_function(400, "hann"), spectrogram (reflect-centred, hop 160, power 2, mel_floor 1e-10, log10), mel_filter_bank(201, 80, 0, 8000, 16000, norm="slaney", mel_scale="slaney") with hertz_to_mel, mel_to_hertz and _create_triangular_filter_bank, and _np_extract_fbank_features' last-frame drop, max-minus-8 floor and (x + 4) / 4 (`mod.rs`); the window smart-turn's inference.py prepares (keep the last 8 s, left-pad with zeros), reproduced from that contract in keeper's own code, not ported
not ported: the torch path (_torch_extract_fbank_features), batching, attention masks, dither, pre-emphasis, other windows and mel scales, chunk lengths other than 8 s, the inference itself (keeper runs the model through ONNX Runtime in the shell) and any weights
changed: Python and numpy to Rust; one fixed configuration instead of parameters; the 400-point FFT is rustfft's in f64, its bins rounded to f32 before squaring as numpy's complex64 spectrum is; each filter keeps only its non-zero span; every buffer is allocated once per `Features`. Golden fixtures from upstream's own code path: `tests/fixtures/smart_turn/`.
revisit: when keeper's end-of-turn model changes family or input (a Smart Turn release whose inference.py calls the extractor differently), or transformers changes the Whisper numpy front end — re-run `tests/fixtures/smart_turn/generate.py` at the new commit and update this record.

# smart_turn

Apache-2.0 §4(b): `mod.rs` is a modified file; its header names the upstream files and what was
changed. transformers ships no NOTICE file, so §4(d) carries nothing.

Smart Turn (pipecat-ai/smart-turn, BSD-2-Clause) contributes no code here, and none to the fixture
generator (`tests/fixtures/smart_turn/generate.py`): its `inference.py` pads or truncates the audio
to its last 8 s and hands it to transformers' extractor. That contract — which audio, and which
arguments — is what this module and the generator reproduce, each in keeper's own code; the
arithmetic is transformers'. The generator and its README are keeper's; the clips they describe
are LibriSpeech's, CC BY 4.0, attributed in that README.
The model (`smart-turn-v3.2-cpu.onnx`, huggingface.co/pipecat-ai/smart-turn-v3 at f766f81d,
BSD-2-Clause, trained on pipecat-ai/smart-turn-data-v3.1/v3.2, CC-BY-4.0) is the account's, in
`_models/smart-turn-v3/`, never in this crate.
