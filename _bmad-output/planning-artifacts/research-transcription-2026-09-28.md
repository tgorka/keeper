---
name: 'keeper'
type: research
topic: 'meeting transcription on the Mac — segmented transcripts, speaker diarization, a synced voices bank and dictionary, corrections, transcription after recording'
decision: 'where transcription runs (this Mac, a NAS, or a cloud API), which recogniser and diarizer, through which runtime and binding, where the models come from, and what keeper must change in its own promise that "recording transcribes nothing"'
status: final
created: '2026-09-28'
run_folder_note: 'the digests below were read as files of the coordinating session (R1 and G1–G4 under /tmp/e87-*.md, the contract as local://epic87-contract.md); filing them under research/ is the coordinator’s step'
digests:
  - R1 — fluidaudio-rs and FluidAudio, read at fluidaudio-rs main bd1f0f3, FluidAudio v0.14.8 (56607d9) and main 20d4f0b, crates.io and the Hugging Face API (`/tmp/e87-R1FluidAudioRs.md`)
context:
  - G1 — recordings, drives, the finalize hook and the tracks in a recorded file (path:line)
  - G2 — the account's config repository and the owner's drives (path:line)
  - G3 — house conventions, numbering ceilings, every place the old promise is stated and the four gates
  - G4 — the port recipe, IPC and ts-rs, capabilities, jobs, cargo-deny, the check scripts
  - the coordinator's contract, `local://epic87-contract.md` (AD-339…AD-350 as pinned)
---

# Research — Transcription on this Mac: segments, speakers, a synced voices bank and a dictionary

**Evidence grades.** `[SOURCE]` = an external primary source, read on 2026-09-28, cited with its publisher and URL
(or R1's section, when R1 read it). `[REPO]` = read in this worktree; line numbers are at `dfa843e3` (origin/main,
the base the build wave started from) unless a line says otherwise. `[INFERENCE]` = reasoning over cited facts,
with no source of its own. `[UNVERIFIED]` = looked for and not established; never to be repeated as fact. §12 lists
every one of those and what was tried.

**How to cite this document.** Sections are numbered `§N.M` and are stable. Cite as
`research-transcription-2026-09-28.md §5.2`, the convention `research-ai-chat-2026-09-02.md` set.

**What this document is not.** It does not design the feature. `epic-87-your-meetings-transcribed-on-this-mac.md`
does, and the coordinator pinned AD-339…AD-350 before this was written. This document is the evidence under them.
Where the evidence pulls against a pinned choice, it says so in place (§5.4, §7.5, §9.2).

---

## 0. Reading guide

| Decision | What it rests on | Sections |
| --- | --- | --- |
| **AD-339** — in process, through a vendored fork of `fluidaudio-rs` | the published crate drops timings and embeddings and cannot load by path; the build needs Swift on macOS; thread-safety and concurrency crashes | §6.1–§6.3, §6.5 |
| **AD-340** — Parakeet TDT 0.6B v3 and pyannote community-1 | English and Polish accuracy; timings; embeddings from the diarizer's own run; Nemotron 3's cap and its missing embeddings | §4, §5 |
| **AD-341** — models from the config repository over git LFS | sizes; precompiled `.mlmodelc`; the layout FluidAudio expects; its download-and-purge fallback; the config repository carries no LFS today | §2.3, §7 |
| **AD-342**, **AD-343** — a voices drive; one file per fact | the recordings role family; the sync engine's conflict copies; the model-prefix folder | §2.2, §9.1, §9.3 |
| **AD-344** — transcripts beside the media | the session folder layout; D-21's derived index | §2.1, §9.4 |
| **AD-345** — the microphone is its own track | keeper-rec's writer order; no track labels | §10 |
| **AD-346**, **AD-347** — matching thresholds; corrections; a dictionary after recognition | embedding space; no calibration source; CTC boosting's English spotter and its regressions | §8, §9.2, §9.4 |
| **AD-348**, **AD-349** — after recording; the capability | the finalize order; the macOS 14 diarizer crash; Intel Macs | §2.1, §6.5 |
| **AD-350** — the promise changes | six statements and four gates; the owner's instruction | §2.5, §11 |

---

## 1. The question, and how it was answered

### 1.1 The question

The owner asked on 2026-09-28, in Polish (verbatim in the epic), for:
- transcription of recordings, optionally automatic after a meeting, and of any audio or video file;
- segmented transcripts with speaker ids;
- a bank of voice embeddings to recognise people in later meetings, synced through a drive;
- corrections of both the words and the people, where a correction may update the bank;
- all of it inside keeper's Rust, in English and possibly Polish, mostly on Zoom and Google Meet video, and offline.

A second message the same day set the constraints this research had to fit:
- no sidecar, and `fluidaudio-rs` on the Mac;
- a voices folder chosen per drive like recordings, with the model as a prefix;
- a dictionary kept the same way;
- the microphone track used where possible;
- the promise "recording transcribes nothing" changed;
- models distributed through the config repository;
- no NAS option;
- voice clips kept in the bank.

### 1.2 Method

- **R1**, one research lane, read `fluidaudio-rs` and FluidAudio at pinned revisions, crates.io and the Hugging Face API, and ran `cargo build` of the crate on the Linux dev host.
- **G1–G4**, four grounding lanes, read this worktree.
- **The coordinator read these external sources on 2026-09-28, and this pass re-read them:**
  - NVIDIA's *Nemotron 3 Diarization* blog post;
  - the Nemotron 3 Diarization, Nemotron 3.5 ASR and Parakeet TDT 0.6B v3 model cards;
  - the NeMo Speech and NeMo-Speech.cpp repositories;
  - CodeSOTA's speech-to-text table;
  - the FluidAudio repository.
- **The Engine lane** reported on 2026-09-28 how its fork loads models, and the fork's bridge appeared in the worktree while this was written. §7.5 grades each part separately: the bridge's code is `[REPO]`; FluidAudio's own `loadLocal` is `[UNVERIFIED]`.

### 1.3 Grades, and what this pass re-verified

- **Re-read on 2026-09-28 in this pass:** every external `[SOURCE]` in §3–§6, and these repository facts:
  - `RecordingSink::finalize` at `keeper/src/ipc.rs:5911`;
  - `capabilities` at `ipc.rs:1456`, with `recording: crate::macos_version::recording_supported()` at `:1473`;
  - `SyncProfile.recordings` at `keeper-sync/src/profile/mod.rs:1154`, and `recordings_root` at `:1350`;
  - `FilesFolderRoleVm` at `keeper-core/src/vm.rs:4159`;
  - `FOLDER_ROLE_ICON` at `src/components/layout/files-pane.tsx:621`;
  - zero `lfs` hits in `keeper-sync/src/config_repo.rs`;
  - the promise sentences in §2.5.
- **Everything else `[REPO]`** is G1–G4's reading, cited as they recorded it.

---

## 2. What keeper has today

### 2.1 Recording: the session, its files, and where it ends `[REPO]`

- **The session folder.** Folders are rendered from `recording.path_template` (default `{yyyy}/{yyyy}-{mm}-{dd} {HH}{MM} {slug}`, `keeper-core/src/recording/path_template.rs:123`). Each holds:
  - `manifest.json`;
  - `screen-####.mov`, or `audio-####.m4a` for audio-only sessions;
  - `camera-####.mov` when the webcam is on;
  - `events.log` in debug mode (G1 §3.4).
- **The manifest** (`recording.rs:1328-1364`, version 1, camelCase) records:
  - the devices (`systemAudio`, `microphone`, `camera`);
  - each segment's `index`, `file`, `bytes`, `track` (`screen|camera|audio`) and PTS bounds.

  It holds no absolute path (G1 §3.3).
- **The finalize order** (`ipc.rs:5911-5969`): set the status → set the end time → `reconcile_from_dir` → one atomic `manifest.write()` → `archive.finalized` → `request_push(SessionEnd)` → `note_stub_at_finalize` (`:5965-5968`). After the note stub the session is complete and its segments are final. This is the one place a finished session can queue work (G1 §3.2).
- **Pointer-only media.** A drive with `MediaPolicy::PointerOnly` holds LFS pointers for the media. `folder_holds_pointer_segments` is the existing "was this recorded here?" test (G1, cross-cutting notes).
- **Recovery.** Salvaged sessions come back through `recover_orphaned_sessions` at boot and before a recording (G1 §3.2). No transcription hook exists on that path.

### 2.2 Drives and their roles `[REPO]`

- **The role pattern.** A drive "keeps recordings" through `SyncProfile.recordings: Option<RecordingsConfig { subfolder, media, push }>` (`profile/mod.rs:1154`; default subfolder `recordings`, `:213`).
  - It can also be declared in the folder's `.keeper/keeper.toml` as `[folder.recordings] subfolder = "…"` (`profile/folder.rs`).
  - It crosses IPC as `SyncProfileVm.recordings` and `recordingsSubfolder` (`sync_ipc.rs:199`, `:212`), and `SyncProfileReq.recordings` and `recordingsSubfolder` (`:769`, `:779`), applied in `parse_req` (`:1151-1168`).
  - Validation refuses and never corrects (`mod.rs:556-625`).
  - The notes vault and the tasks ledger follow the same pattern (AD-298, epic 80).
- **Files marks role folders from configuration, never from a name.** Rust decides the role:
  - `FilesFolderRoleVm` (`vm.rs:4159`) has `notesVault | recordings | tasks`;
  - `role_of` matches the whole path, case-insensitively.

  The webview draws the glyph: `FOLDER_ROLE_ICON` (`files-pane.tsx:621-625`) and `FOLDER_ROLE_TITLE` (`:629-633`).
- **The owner's drives** (G2 §3):
  - `tgdrive` (recordings under `40-media/recordings`);
  - `tgdrive-light` (the same remote on the internal disk);
  - `neuradrive` (recordings under `70-comms/meetings`);
  - all three on electra's Forgejo, with LFS in use.

  Both drives use numbered zones, and `70-comms` exists on tgdrive (the `/workspace/tgdrive` mirror, `[UNVERIFIED]` whether that mirror is complete).

### 2.3 The config repository carries text, not assets `[REPO]`

- **What it is.** One git repository per organisation account, cloned to `<data_dir>/account/<id>/repo` (`account_ipc.rs:285-291`). The module doc says "bytes in and out, nothing more (AD-312)" (`keeper-sync/src/config_repo.rs:1-10`).
- **How it writes.**
  - `clone_or_fetch` hard-resets the worktree to `origin/<branch>` at every sync.
  - Writes are whole in-memory blobs (`Write { rel, bytes: Vec<u8> }`, `:148-158`; `write_blob`, `:260-262`).
  - The file mentions `lfs` nowhere. This pass counted 0 hits.
- **No precedent.** Nothing but TOML has ever been distributed through it (G2 §1).
- **The drive engine has its own git-LFS client** (`keeper-sync/src/lfs/`: `pointer`, `store`, `endpoint`, `batch`, `basic`, `hydrate`, …), and the config repository can use it. That client is the binary-distribution machinery this codebase already trusts (G2 §1).
- **Its hosts are already disclosed.** Its host sits in `docs/egress.md`'s organisation-account row: "the settings repository and its `api_base`" (`docs/egress.md:27`).

### 2.4 The recipe for a platform capability `[REPO]`

- **Voice is the model** (G4 §1):
  - a port trait in keeper-core with no Apple symbol (`voice/mod.rs:238`);
  - refusals as a typed enum whose sentences core writes;
  - a shell implementation gated at file level (`voice_macos.rs`, `#![cfg(target_os = "macos")]`), in which one worker thread owns the framework objects and each trait call is a message;
  - an ungated IPC adapter choosing the port per target, with an `AbsentPort` answering `Unsupported`, so every command is registered on every target (`voice_ipc.rs:124-163`).
- **Capabilities are runtime probes.** `recording` is a `sw_vers` probe, not a `cfg` (`ipc.rs:1473`; `macos_version.rs`). The webview gates on `CapabilitiesVm` and never sniffs the platform (`no-user-agent-gating.test.ts`).
- **A cancellable streamed job** is `export_start`/`export_cancel`:
  - an id is returned at once and the worker runs in `spawn_blocking`;
  - progress batches go over a `Channel`, with exactly one terminal batch;
  - a cancel flag is checked between steps (`ipc.rs:2709-2878`).
- **The check scripts** forbid three dependencies (G4 §6):
  - `tauri` in `keeper-core` (`check:core-tauri-free`);
  - `gix`/`keeper-sync` in `keeper-core` (`check:core-sync-free`);
  - any change to generated bindings (`bindings:check`).
- **`unsafe`.** `unsafe_code` is denied workspace-wide.
- **Dependency sources.** `deny.toml` allows MIT and Apache-2.0, and a git source needs an explicit `allow-git` entry (G4 §2).

### 2.5 The promise this epic changes `[REPO]`

The statement "recording transcribes nothing" lives in these places:
- **`docs/decisions.md:155-157`** (inside D-4's bullet *Why voice and the wake word are Epic 62*): "The recording feature's promise that it transcribes nothing is stated in six places and enforced by a source scan, and a voice surface that borrowed the recording pipeline would make that promise false."
- **`docs/egress.md:231-240`** (§ *Screen recording adds no egress*): "there is no upload, share-link, transcription, or cloud affordance anywhere in the recording feature."
- **`docs/recording.md:3-5`:** "**Nothing uploads** — the recording feature adds zero network destinations". This one does not mention transcription, and it stays true.
- **`AGENTS.md:144`:** the bots rule forbids "a palette verb the zero-egress scan would read as an upload or a transcription".
- **`src/components/recording/zero-egress.test.ts:76-82`:** `transc${"ri"}` in any case, with no honest-copy exemption. G1 placed this file at `src/test/`; it is under `src/components/recording/`.
- **`src/test/bots-surface-stays-out-of-recording.test.ts:5-6`,** a doc comment: "The recording feature promises, in six places, that it uploads, shares and transcribes nothing".
- **Planning records of their time:**
  - `epic-16-context.md:23`, `epic-19-context.md:29` and `epic-20-context.md:25`;
  - `spec-20-3-…:39`, and `spec-20-4-…:39`, `:104`;
  - `epic-61-…:48`;
  - `brainstorm-recording-sync-archive-2026-08-05/.memlog.md:133`.
- **`keeper-core/src/notes/recording_note.rs:15`:** "No transcription, no summarisation, no inference of any kind". This one is about the note stub keeper writes, and it stays true, because the transcript is a separate file.

Gates that stay as they are:
- `keeper_rec_sidecar_sources_are_network_free` (`keeper/src/zero_egress.rs:41`; the sidecar is untouched);
- `voice_on_device` (`keeper-core/tests/voice_on_device.rs`). It scans `keeper-core/src/voice/**` and `voice*.rs` in the shell by prefix, so a transcription file must not be named `voice*` (G3 §4).

### 2.6 Nothing transcribes today `[REPO]`

A grep for `transcri|diariz|speaker|fluidaudio` over the sources hits only docs, tests, egress guards and the voice assistant's own on-device recogniser (G1 cross-cutting; G2 §3). keeper has:
- no speaker bank;
- no dictionary or lexicon;
- no correction UI.

---

## 3. Where it runs: this Mac, a NAS, or the cloud

### 3.1 The three options

| | **This Mac** (FluidAudio in process) | **A NAS or home server** (NeMo Speech or NeMo-Speech.cpp `serve`) | **A cloud API** (e.g. ElevenLabs Scribe, AssemblyAI, Deepgram, Zoom Scribe) |
| --- | --- | --- | --- |
| Where meeting audio goes | nowhere; it stays in the drive the person chose | uploaded over the tailnet to the NAS | uploaded to a third party |
| New egress | none: models come from the config repository's host, already listed (§7.4) | a new row, the NAS host | a new third-party row, plus an API key |
| Server component | none | yes, and AGENTS.md says keeper is "a **client only**" (quoted at epic-86:40, D-28) | the vendor's |
| Best diarizer reachable | pyannote community-1 now; Nemotron 3 later through FluidAudio 0.17 (§5.1) | Nemotron 3, the top of VoiceArena (§5.1) | built in (CodeSOTA: "Diarization and speaker labeling built-in") `[SOURCE]` |
| Embeddings for a voices bank | 256-d, from the same diarizer run (§5.2) | not from Nemotron 3, which emits only speaker-activity probabilities (§5.1); a second model would be needed | vendor-specific, not portable `[INFERENCE]` |
| Polish | Parakeet v3: FLEURS pl WER 7.31, MLS pl 7.28 (§4.1) | Nemotron 3.5: FLEURS pl 15.15 at best (§4.2); Parakeet v3 is also runnable (§6.4) | vendor-specific `[UNVERIFIED]` |
| Offline | yes, once the models are fetched | only on the home network | no |
| Platforms | Apple Silicon Macs on macOS 15+ only (§6.5) | any client, the phone included | any client |
| Setup | `_models/` in the config repository, once | a GPU or CPU host, a service, updates and monitoring | an account, a key and a bill |

Sources for the rows:
- NeMo Speech needs "Python 3.12 or above … PyTorch 2.7 or above … NVIDIA GPU + CUDA (required for training; recommended for inference)" `[SOURCE]` (NVIDIA, https://github.com/NVIDIA-NeMo/Speech).
- NeMo-Speech.cpp's `nemo-speech serve` binds `127.0.0.1:8080` by default and exposes "documented OpenAI-compatible subsets" plus realtime WebSocket transcription `[SOURCE]` (NVIDIA, https://github.com/NVIDIA/NeMo-Speech.cpp).

### 3.2 Why this Mac won

1. **The owner asked for it,** three times: "offline processing"; "Chce zeby to bylo wewnatrz w rust code"; "Opcja NAS - nie rob opcji nas skoro mozna miec wszystko lokalnie". The last is a rejection, not a preference: no NAS option is built.
2. **The promise survives.** Recording still adds no network destination (§11). A NAS or a cloud API would each add a row that `docs/egress.md` exists to refuse unless someone chose it (D-4), and keeper would be the one choosing.
3. **The bank needs embeddings from the diarizer's own model.** A speaker cluster must be comparable with the bank's vectors. community-1 gives both from one run, and Nemotron 3 gives neither (§5).
4. **Polish is better on the local recogniser.** Parakeet v3's FLEURS Polish WER is 7.31 against Nemotron 3.5's 15.15 (§4.4).
5. **keeper is a client.** A NAS path makes keeper depend on a server the person must run. That is the shape D-28 refused for tokens, and nothing here argues differently for audio `[INFERENCE]`.

The costs this choice accepts, each recorded in the epic's *What stays out*:
- the Mac is the only platform (DW-335);
- a laptop does the compute;
- the models are about 0.5 GB (§7.1);
- the strongest diarizer measured waits (DW-331).

### 3.3 What was not weighed

- **A cloud fallback** when the Mac cannot transcribe. D-5's rule for voice ("a server fallback is not a revisit, it is a new row in `docs/egress.md` that this decision refuses to write", `docs/decisions.md:245-247`) applies unchanged `[INFERENCE]`.

---

## 4. Speech recognition

### 4.1 Parakeet TDT 0.6B v3 `[SOURCE]` (NVIDIA, https://huggingface.co/nvidia/parakeet-tdt-0.6b-v3)

- **Coverage.** 600M parameters, 25 European languages including Polish (`pl`), language detected automatically. The card says "It extends the parakeet-tdt-0.6b-v2 model by expanding language support from English to 25 European languages. The model automatically detects the language of the audio and transcribes it without requiring additional prompting."
- **Output.** Automatic punctuation and capitalisation, "Accurate **word-level** and **segment-level** timestamps", and long audio "up to 24 minutes … with full attention (on A100 80GB) or up to 3 hours with local attention".
- **Licence:** CC-BY-4.0.
- **English WER on the eight Open ASR datasets** (the card's `model-index`): AMI 11.31, Earnings-22 11.42, GigaSpeech 9.59, LibriSpeech clean 1.93 and other 3.59, SPGI 3.97, TED-LIUM 2.75, VoxPopuli 6.14.
- **Polish WER:** FLEURS `pl_pl` 7.31, and Multilingual LibriSpeech `polish` 7.28.
- **The CoreML port** is `FluidInference/parakeet-tdt-0.6b-v3-coreml` (CC-BY-4.0, base `nvidia/parakeet-tdt-0.6b-v3`). FluidAudio's own benchmark gives FLEURS Polish WER 8.6%, CER 2.8% and RTFx 190 on an M4 Pro `[SOURCE]` (R1 §2, FluidAudio `Documentation/Benchmarks.md` L32).

### 4.2 Nemotron 3.5 ASR `[SOURCE]` (NVIDIA, https://huggingface.co/nvidia/nemotron-3.5-asr-streaming-0.6b)

- **What it is.** A 600M-parameter cache-aware FastConformer-RNNT streaming model, released 2026-06-04 under OpenMDW-1.1. It transcribes "**40 language-locales** from a single model through language-ID prompt conditioning, with optional automatic language detection".
- **Polish is in the *broad-coverage* tier (13 locales),** not the *transcription-ready* one (19 locales).
- **Polish FLEURS WER by chunk size.**
  - With the language given: 19.88 at 80 ms, 18.92 at 160 ms, 17.48 at 320 ms, 16.61 at 560 ms and **15.15 at 1.12 s**.
  - Auto-detected: 22.65 at 80 ms and **16.55 at 1.12 s**.
  - English at 1.12 s, language given: 7.91.
- **Its own card points English elsewhere:** "We would recommend Nemotron ASR Streaming (English) model for English-only transcription use cases."

### 4.3 Where Parakeet v3 sits on the Open ASR table

- **CodeSOTA's register** `[SOURCE]` (https://codesota.com/speech-to-text; the page says "Updated 2026-09-27"; its figures come "from the HF Open ASR Leaderboard (en_shortform), accessed 2026-05-22") ranks 19 models by mean WER over the eight English datasets:
  - #1 Granite Speech 4.1 2B, 5.33;
  - Cohere Transcribe, 5.42;
  - Canary-Qwen-2.5B, 5.63;
  - Qwen3-ASR-1.7B, 5.76;
  - **Parakeet TDT 0.6B v2, 6.05**;
  - Canary 1B, 6.50;
  - Parakeet TDT 1.1B, 7.02;
  - Whisper Large v3, 7.44;
  - **Whisper Large v3 Turbo, 7.83** (last).
- **Parakeet v3 is not on the table.**
- **The mean of v3's own eight card figures (§4.1) is 50.70 / 8 ≈ 6.34** `[INFERENCE]`, computed on the same eight datasets. That places it:
  - behind v2 (English-only, 6.05);
  - ahead of Canary 1B (6.50) and both Whisper large models (7.44, 7.83).

  The card's figures and the leaderboard's harness were not checked for identical normalisation `[UNVERIFIED]`.
- **The open models ahead of it are all larger (1B–8B parameters), except the English-only Parakeet v2. The others ahead of it are cloud APIs** `[INFERENCE, from the table's Params and Kind columns]`.
- **FluidAudio ports some of the leaders.** Its documentation includes `Documentation/ASR/Cohere.md`, and it ships Qwen3-ASR 0.6B (the table ranks the 1.7B) `[SOURCE, the FluidAudio repository tree; R1 §3]`. No Granite port was seen.
- **Not read here** `[UNVERIFIED]`:
  - the Cohere port's languages (Polish in particular), its size, and its accuracy through FluidAudio;
  - whether a 2B Granite runs on the Neural Engine at useful speed.

  Parakeet v3 stays the choice on what is measured: Polish WER, token timings, about 0.5 GB, and the binding the owner named.

### 4.4 Polish, specifically

- **Parakeet v3** is the best-measured of the three (§4.1–§4.2): FLEURS 7.31 on the card, and 8.6% in FluidAudio's own run.
- **Nemotron 3.5** is twice as wrong on FLEURS Polish at its best setting (15.15).
- **"Language" on Parakeet v3 is a script filter, not a selector.** FluidAudio's `language:` argument filters top-K tokens by script (Latin or Cyrillic) and has a `.polish` case. It exists "to stop Cyrillic tokens leaking into Polish" `[SOURCE]` (R1 §2, `TokenLanguageFilter.swift#L4-L32`, issue #512).
- **The published crate never passes it** (R1 §2).
- **An older bug.** In 0.14.8, `.polish` also triggered an English blocklist that corrupted Polish words ("but" → "bud"). It was fixed by #847, merged 2026-08-11 (R1 §2). That puts the fix in 0.15.x or later, and so in 0.17.4 `[INFERENCE]`; the exact tag was not checked `[UNVERIFIED]`.

### 4.5 The choice (AD-340)

- **Parakeet TDT 0.6B v3, int8 CoreML,** with `transcription.language` = `auto | en | pl` passed through as the token-language filter.
- **Rejected:**
  - Whisper, which is worse on English on the table and needs its own CoreML compile path (`research-ai-chat-2026-09-02.md` §10.3);
  - Nemotron 3.5, which is weaker on Polish and built for streaming;
  - the leaders of the English table (Granite Speech 4.1 2B, Cohere Transcribe), which are about three times Parakeet's size and scored on English only. FluidAudio ports Cohere, but that port's Polish, size and speed were not established (§4.3).

---

## 5. Diarization and speaker embeddings

### 5.1 Nemotron 3 Diarization `[SOURCE]` (NVIDIA blog, https://huggingface.co/blog/nvidia/nemotron-diarization, 2026-09-23; card, https://huggingface.co/nvidia/Nemotron-3-Diarization)

- **The model.** Open weights, 100M parameters, released 2026-09-23 under **OpenMDW-1.1**. Streaming and offline from one checkpoint:
  - recommended input buffers of 30.4 s (offline-style), 1.04 s, 0.64 s and 0.32 s;
  - "With chunked inference, the maximum audio duration is not limited."
- **The cap:** "Supporting up to eight speakers".
- **VoiceArena.** It ranked **#1 in VoiceArena's initial Diarization-Bench with 14.72% DER**, against 19.3% for the next system, "among 12 systems and 17 total system configurations evaluated across 139 English-language conversations totaling approximately 22 hours", with overlap scored, system speech detection and no collar. The blog notes: "These initial results may change as Voice Arena completes its Version 1 evaluation."
- **Against its own predecessor** (Sortformer 4spk v2.1), at a 1.04 s buffer: an unweighted mean relative DER reduction of 41.0% across eight evaluation conditions. On two-speaker CALLHOME it is slightly worse (5.98% against 5.68%).
- **Output: speaker-activity probabilities only.**
  - The output is "a [T, 8] floating-point tensor". The card lists "**Output Type:** Other: Numerical tensor."
  - The channels "are anonymous labels, not real-world identities: the model can report that speaker_2 spoke from one timestamp to another, but it does not determine that speaker_2 is a particular person. Downstream applications can map these anonymous channel IDs to explicit speaker identities by pairing timestamps with … active speaker verification models."
  - It emits **no speaker embedding**.
- **Training data** is mostly English, with Mandarin and Indic sets, and simulated mixtures "spanning 21 languages" from licensed David AI audio. Polish meetings are not in the evaluation list.
- **Runtimes** for it: NeMo (Python), 🤗 Transformers, NeMo-Speech.cpp (`nemo-speech diarize meeting.wav`), and Argmax Pro SDK 3 on Apple devices.
- **FluidAudio added it in v0.17.0** (tag dated 2026-09-23, `Sources/FluidAudio/Diarizer/Nemotron3/`). Its presets are about 199 MB each, and a split w8a8 variant is 100 MB `[SOURCE]` (R1 §2, §3). The published crate exposes none of it (R1 §2).

### 5.2 pyannote community-1 through FluidAudio's offline pipeline `[SOURCE]` (R1 §2, §3)

- **The pipeline.** `OfflineDiarizerManager`: pyannote community-1 segmentation, the FBank, the embedding and PLDA models, then VBx clustering.
- **Its output** includes, per segment, `TimedSpeakerSegment.embedding`, a `speakerDatabase`, and `ChunkEmbedding.embedding256` / `rho128` (FluidAudio 0.14.8, `Diarizer/Core/DiarizerTypes.swift#L161-L200`).
- **Embeddings are 256-d,** the same size as `SpeakerManager.embeddingSize = 256`.
- **It declares no speaker cap.** No source read here states a community-1 cap, and the contract relies on there being none (AD-340) `[UNVERIFIED as a documented guarantee]`.
- **Files:** `Segmentation.mlmodelc` 6.0 MB, `FBank.mlmodelc` 1.8 MB, `Embedding.mlmodelc` 13.5 MB, `PldaRho.mlmodelc` 0.2 MB and `plda-parameters.json`, about **22 MB** in total.
- **Two embedding models, not interchangeable.** The legacy pyannote-3.1/WeSpeaker pipeline's `extractSpeakerEmbedding` also yields 256-d vectors, from `wespeaker_v2.mlmodelc`. R1 warns they "should not be assumed comparable" with community-1's `[INFERENCE, R1]`. This is why the bank keys embeddings by model (AD-343, §9.1), and why a single-clip embedding must come from the same community-1 embedding model the diarizer uses (the `SpeechEngine::embed` contract).
- **Not measured.** community-1's DER on meeting audio through FluidAudio, compared with Nemotron 3's, was not measured by any source read here `[UNVERIFIED]`.

### 5.3 The other diarizers in FluidAudio `[SOURCE]` (R1 §2)

- **Sortformer:** streaming, **at most 4 speakers**. The crate's `diarize_file_with_models` takes an `.mlpackage` and hard-codes `quality_score = 1.0`.
- **LS-EEND:** at most 10 speakers, streaming. Not exposed.
- **Neither yields embeddings** through the crate.

### 5.4 The choice (AD-340), and where the evidence pulls against it

- **community-1 is the diarizer,** because it is the one that gives the bank its vectors from the same run, with no speaker cap.
- **The evidence pulls the other way on accuracy.** Nemotron 3 is the best-measured diarizer available (§5.1), and FluidAudio 0.17.4 already carries it.
- **Using Nemotron 3 later needs one of two things** `[INFERENCE]`:
  - a separate embedding pass per speaker, with community-1's embedding model on each speaker's longest clean spans, so the bank stays in one embedding space; or
  - a bank keyed by a second embedding model.
- **The cap matters for meetings of more than eight.**
- **Recorded as DW-331,** with those two paths as its revisit.

---

## 6. Runtimes and bindings

### 6.1 FluidAudio `[SOURCE]` (FluidInference, https://github.com/FluidInference/FluidAudio)

- **What it is.** "a Swift SDK for fully local, low-latency audio AI on Apple devices, with inference offloaded to the Apple Neural Engine (ANE)". Licence **Apache-2.0**, Swift 6.0+, platforms macOS and iOS.
- **Its features** include Parakeet TDT v3 ("25 European languages"), "Speaker Diarization (Online + Offline)", "Speaker Embedding Extraction … you can use this for speaker identification", Silero VAD, and a mirror override (`REGISTRY_URL`) for anyone who cannot reach Hugging Face.
- **It names `fluidaudio-rs`** as its official "Rust / Tauri" wrapper (`cargo add fluidaudio-rs`).
- **Its showcase** lists many Mac meeting apps built this way. Two do exactly keeper's two-track shape:
  - *Meeting Transcriber*, with "dual-track speaker diarization";
  - *echo99*, which "records the mic and system audio as separate tracks and transcribes them entirely on-device".
- **Current version:** v0.17.4 (tag dated 2026-09-24) `[SOURCE]` (R1 summary).

### 6.2 `fluidaudio-rs` as published `[SOURCE]` (R1 §1–§2)

- **Versions.** crates.io's newest is **0.14.1** (2026-05-11). GitHub main says 0.14.8, pinning FluidAudio 0.14.8, and was never published. FluidAudio itself is at 0.17.4.
- **Licence:** MIT. The binding is hand-written `@_cdecl` / `extern "C"` FFI, not swift-bridge.
- **The build.** `build.rs` always runs `swift build`, needs Xcode 16 / Swift 6, and fetches FluidAudio through SwiftPM.
  - **It fails on Linux with no stub.** R1 ran it: `Failed to run swift build: Os { code: 2, kind: NotFound }`.
  - Its feature flags gate nothing.
- **The API drops what keeper needs:**
  - ASR returns only `text, confidence, duration, processing_time, rtfx`, so token timings are discarded;
  - diarization drops embeddings and `speakerDatabase`;
  - there is no language argument and no vocabulary;
  - Parakeet and the offline diarizer cannot be loaded from a directory, because every init downloads.
- **0.14.1 also carries the stale-decoder-state bug.** A second `transcribe_file` returns `". Hello world…"`, and the fix, PR #15, is on main only.
- **Errors** are `print()`ed to stdout. Rust receives fixed strings.
- **Thread safety is asserted, not enforced.** It declares `unsafe impl Send/Sync` over unsynchronised Swift `var`s.

### 6.3 Why a vendored fork (AD-339) `[INFERENCE, over §6.2]`

What keeper must have and the crate cannot give, at any published or unpublished revision:
- token timings, for words and utterances;
- per-speaker embeddings, for the bank;
- loading by path, so keeper downloads nothing itself (§7.5);
- a newer FluidAudio, for the Polish blocklist fix and a later Nemotron 3 path.

Options:
- **A git dependency on upstream main** fixes the decoder-state bug and nothing else.
- **Upstreaming the bridge changes first** puts the epic behind another project's review.
- **A fork vendored at `tools/fluidaudio-rs/`**, outside `src-tauri/` so it is never a workspace member and never builds on Linux, keeps the MIT licence and attribution. It also keeps its `unsafe` FFI inside the dependency. The divergence is recorded as DW-339.

### 6.4 NeMo Speech and NeMo-Speech.cpp `[SOURCE]`

- **NeMo Speech** (NVIDIA, https://github.com/NVIDIA-NeMo/Speech; Apache-2.0) is a Python/PyTorch framework for training and inference, with CUDA recommended for inference. It is a server-side runtime `[INFERENCE]`: nothing a Tauri app embeds. It is relevant only to the rejected NAS option (§3.1).
- **NeMo-Speech.cpp** (NVIDIA, https://github.com/NVIDIA/NeMo-Speech.cpp; Apache-2.0 code) is "A lightweight native C++ runtime for the NVIDIA Nemotron Speech model family", with "native inference powered by ggml".
  - It runs Nemotron 3.5 ASR, Parakeet TDT 0.6B v3, Sortformer v2 and Nemotron 3 Diarization, "standalone or combined with ASR".
  - It ships a CLI, a local server, and a native SDK with "stable C headers, shared libraries, and an exported CMake package".
  - Its CLI "downloads the pinned default Nemotron 3.5 GGUF from Hugging Face and verifies its size and SHA-256" on first use. "Local GGUF paths continue to work without downloading anything."
- **Why not the C SDK in process** `[INFERENCE]`:
  - the owner named `fluidaudio-rs`;
  - ggml runs on Metal or the CPU, where FluidAudio targets the Neural Engine;
  - its diarizers are Sortformer-family and give no embeddings, so the bank would still need a second model;
  - it would bring a C++/CMake build into keeper's.

  It is the natural engine for a future Linux or Windows port (DW-335).

### 6.5 Known issues that shape the port `[SOURCE]` (R1 §5)

- **Concurrency.** Running ASR and the diarizer concurrently caused `EXC_BAD_ACCESS` in `libBNNS` (FluidAudio issue #661, on macOS 14, 15 and 26, closed `not_planned`). Hence NFR-106: one worker thread owns the handle, calls are serialised, and ASR and diarization never overlap.
- **macOS 14.** `OfflineDiarizerManager` crashed in BNNS 1,200 of 1,200 times on macOS 14.8.7 and 0 times on macOS 15 and 26 (issue #878, an OS bug that serialising does not help). Hence AD-349's floor of **macOS 15**.
- **Intel Macs.** Parakeet "will fail to load on Intel Macs", and FluidAudio throws `unsupportedPlatform("Parakeet models require Apple Silicon")`. Hence **Apple Silicon**.
- **Every call blocks,** with no progress callback and no cancellation. Hence a worker thread, and a cancel checked between phases (AD-339, the job in 87.5).
- **First-load compile.** The first load pays a device-specific ANE compile ("30s+" for v3 cold, per FluidAudio's `TDT-CTC-110M.md`), cached by the OS and keyed to the model's path `[INFERENCE, R1]`. Moving or re-staging the model directory recompiles, so hydration writes in place and never moves `<data_dir>/models/`.
- **Memory.** `transcribe_samples` needs the whole buffer, and peak RSS for Parakeet v3 is `[UNVERIFIED]`. The minimum input is 0.3 s. The port's `decode(range)` lets the job work per part (segment file) rather than per session.
- **Audio input.** FluidAudio decodes with `AVAudioFile` and has no `AVAssetReader` code. Whether `.mov`/`.mp4` with a video track open through it is `[UNVERIFIED]` (R1 §5). The port's `decode(media, track, range)` owns that, and the Engine lane's fork must prove it on hesperia (87.4).
- **Linking.** Whether a Tauri binary links cleanly against the crate ("Swift compatibility library symbols") is `[UNVERIFIED]` until the first Mac build (R1, *Not established*).

---

## 7. Models: files, layout, licences, distribution

### 7.1 Files and sizes `[SOURCE]` (R1 §3, from the Hugging Face tree API)

- **Parakeet v3, int8 (the default).** About **483 MB**:
  - `Preprocessor.mlmodelc` 0.5 MB;
  - `Encoder.mlmodelc` 446.2 MB;
  - `Decoder.mlmodelc` 23.6 MB;
  - `JointDecisionv3.mlmodelc` 12.7 MB;
  - `parakeet_vocab.json` 0.2 MB.

  The whole Hugging Face repository is 3.59 GB. FluidAudio fetches only what it needs.
- **community-1:** about **22 MB** (§5.2).
- **The set:** about **505 MB**.
- **They are precompiled.** The repositories ship `.mlmodelc` directories, and loading goes straight to `MLModel(contentsOf:)` with no `compileModel` step. That answers the owner's "chyba ze musza byc skompilowane": they need no compiling before distribution, only the on-device ANE compile at first load (§6.5).

### 7.2 The layout FluidAudio expects `[SOURCE]` (R1 §3; verified against 0.14.8's code, not its docs)

```
<root>/parakeet-tdt-0.6b-v3/{Preprocessor,Encoder,Decoder,JointDecisionv3}.mlmodelc + parakeet_vocab.json
<root>/speaker-diarization/{Segmentation,FBank,Embedding,PldaRho}.mlmodelc + plda-parameters.json
```

- **Folder names.** They are the Hugging Face repository names without `-coreml`.
- **The docs contradict the code twice.** `ManualModelLoading.md` names the folder `parakeet-tdt-0.6b-v3-coreml` and the joint `JointDecision.mlmodelc`. Following the doc literally makes FluidAudio "find nothing and download".
- **The contract's `required_paths`** (`models.rs`) names exactly the layout above, down to each `coremldata.bin`.

### 7.3 Licences `[SOURCE]` (R1 §3)

| Hugging Face repository | Licence | Notes |
| --- | --- | --- |
| `FluidInference/parakeet-tdt-0.6b-v3-coreml` | CC-BY-4.0 | base `nvidia/parakeet-tdt-0.6b-v3`, CC-BY-4.0 |
| `FluidInference/speaker-diarization-coreml` | "scoped-cc-by-4.0" (`NOTICE.md`) | CC-BY-4.0 covers only the community-1 artifacts (Segmentation, FBank, Embedding, PLDA, PldaRho, `plda-parameters.json`, `xvector-transform.json`). Attribution must name pyannote, WeSpeaker, BUT Speech@FIT and Fluid Inference. The legacy files "must be evaluated separately". Open issue #927 asks about provenance for commercial redistribution. |
| `FluidInference/nemotron-3-diarization-coreml` | OpenMDW-1.1 | not used (DW-331) |
| `FluidInference/parakeet-ctc-110m-coreml` | CC-BY-4.0, English | not used (DW-332) |

- **keeper's bundle carries none of these files** (D-5 holds; §7.4). They travel in the organisation's config repository, so attribution travels with them (DW-341).
- **The code licences.** FluidAudio is Apache-2.0 and `fluidaudio-rs` is MIT. Both are on `deny.toml`'s allow list (G4 §2).

### 7.4 Distribution through the config repository (AD-341)

- **The layout.** `_models/` in the account's config repository:
  - tracked by git LFS (`_models/** filter=lfs diff=lfs merge=lfs -text`);
  - named by `_models/models.toml`.
- **Why it is not a person.** "Directories whose names start with `_` are not people" (`docs/account.md:452-453`) `[REPO]`, so `_models/` sits beside `_template/` without a layout change.
- **The clone may hold pointers or bytes.** keeper's config clone uses gix with no LFS (§2.3). Whether gix's checkout leaves the pointer text or runs a configured smudge filter for that clone is `[UNVERIFIED]`. The contract's `hydrate_lfs_dir` handles both: a pointer is downloaded, and a non-pointer file is copied.
- **Hosts.**
  - The LFS endpoint is `<remote_url>/info/lfs`, on the settings repository's host that `docs/egress.md:27` already lists `[REPO]`.
  - The batch API's `actions.download.href` is the server's to name. `docs/egress.md:44` already warns that for LFS "It is usually the same host, and it is not guaranteed to be" `[REPO]`.
  - Whether electra's Forgejo returns hrefs on its own host is `[UNVERIFIED]`. The owner's drives on the same Forgejo fetch LFS today (§2.2), so no new host is expected `[INFERENCE]`.
- **Rejected alternatives:**
  - **FluidAudio's own download from Hugging Face** is a new destination the promise refuses (§11).
  - **Bundling** would add about 0.5 GB to every release and update, and D-5 says no bundled weights.
  - **Raw blobs in the config repository without LFS** would be written through in-memory `Vec<u8>` blobs (§2.3) and stay in every device's history forever `[INFERENCE]`.

### 7.5 FluidAudio's download fallback, and how the fork must avoid it

- **The fallback** `[SOURCE]` (R1 §2, *Pitfall*):
  - FluidAudio 0.14.8's `DownloadUtils.loadModels` retries a failed load "by deleting the repo directory and downloading again" (`DownloadUtils.swift#L126-L150`);
  - `OfflineDiarizerManager.prepareModels` calls `purgeDiarizerRepo(at:)` on failure;
  - `AsrModels.load(from:)` goes through `loadModelsOnce`, which downloads when files are missing.

  A corrupt or incomplete staged set could therefore be wiped, and Hugging Face contacted `[INFERENCE, R1]`.
- **The contract's rule.** "the engine is never asked to load a set that is not fully present" (AD-341), checked by `missing()` before `load`. That covers absent files, not corrupt ones.
- **The fork's bridge, read in the worktree while this was written** (`tools/fluidaudio-rs/swift/FluidAudioBridge.swift`, in-flight Engine-lane code, current line numbers) `[REPO]`:
  - `loadAsr` calls `requireFiles` and then `AsrModels.loadLocal(from: directory, version: .v3, encoderPrecision: .int8)` (`:128-134`);
  - `loadDiarizer` calls `requireFiles`, then builds `OfflineDiarizerModels` by hand from `loadCompiledModel` for each `.mlmodelc` and its own parse of `plda-parameters.json`, then calls `OfflineDiarizerManager.initialize(models:)` (`:169-185`). Its comment says this is "so its download-and-purge fallback is never reachable";
  - `runDiarizer` refuses before `process(audio:)` when no models are set, because `process` "would fall back to `prepareModels()` — a download" (`:204-208`);
  - the fork's `NOTICE` says the same (`tools/fluidaudio-rs/NOTICE:23-27`).
- **Still `[UNVERIFIED]`:** that FluidAudio 0.17.4's own `AsrModels.loadLocal` never reaches a download. The Engine lane reports it is "new in 0.17.x", reads exactly that directory, touches no ModelHub and throws when a file is missing. FluidAudio's source for it was not read in this pass, and 87.4's review settles it.
- **What 87.4's acceptance says,** because D-29's promise rests on it: every model is loaded by path and never through FluidAudio's download or prepare helpers, and a load failure is an `EngineError` that deletes nothing.

---

## 8. The dictionary: vocabulary boosting against a post-recognition map

### 8.1 FluidAudio's CTC vocabulary boosting `[SOURCE]` (R1 §4)

- **What it is.** CTC word-spotting plus rescoring (NeMo, arXiv:2406.07096). The TDT decoder itself is not biased. Parakeet 0.6B needs a separate CTC encoder, `parakeet-ctc-110m` (about 97.5–102 MB).
- **Where it is exposed.** In 0.14.8, only on `SlidingWindowAsrManager`. On main (after #862, 2026-08-19), on `UnifiedAsrManager` too. The crate exposes none of it.
- **Guidance:** "1-50 terms Excellent … 100-230 terms Tested"; terms of at least 4 characters.
- **The spotter is English.** `parakeet-ctc-110m-coreml` is tagged `en`, and its base `nvidia/parakeet-tdt_ctc-110m` is English. On Polish it is `[UNVERIFIED]` and likely weak `[INFERENCE, R1]`.
- **It misfires on English too.** Issue #967 (open, 2026-09-26): on 500 English dictations with a 51-term list, "278 of 500 transcripts changed, many of them wrongly" ("Maya" became "EMEA").

### 8.2 The choice (AD-347)

- **Terms are applied after recognition:** whole-word, case-insensitive alias → text, multi-word aliases allowed, each replacement recorded in the transcript (`dictionaryApplied`).
- **Cost:** no second encoder, no language limit, and a replacement is visible and reversible.
- **Limit:** a name the recogniser mangled beyond every alias is not recovered. That is DW-332's revisit.
- **Edits feed the dictionary only with consent.** A person's edits only *suggest* terms, as single-word substitutions, because a dictionary that learned silently would rewrite later transcripts without anyone agreeing `[INFERENCE]`.

---

## 9. Speaker identity: the bank, matching, corrections

### 9.1 One embedding space per model

- **Vectors from different models are not comparable** (§5.2).
- **So embeddings live under `embeddings/<embedding-model-id>/<person-id>/<clip-id>.json`,** and the clips (16 kHz mono WAV, 2–15 s) are model-agnostic.
- **A model change** re-derives embeddings from the clips (`missing_embeddings(model)`) instead of mixing spaces.
- **The owner asked for exactly this:** "zrob pdofolder lub prefix dla modelu, jezeli pozniej zmienie model", and "klipy głosowe w banku - tak trzymaj".

### 9.2 Matching thresholds

- **The contract's constants:**
  - `AUTO_MATCH = 0.70`, `SUGGEST = 0.50`, cosine against a person's normalised centroid;
  - `LINK = 0.60`, for joining clusters across parts.
- **No calibration source.** No source read here calibrates cosine thresholds for community-1's 256-d embeddings on keeper's audio `[UNVERIFIED]`. The values are a starting point, recorded as DW-333.
- **What limits the damage.** A wrong `auto` match is one click to correct, and only an explicit confirmation writes to the bank (AD-347). A miscalibration costs corrections, not bank corruption `[INFERENCE]`.

### 9.3 One fact per file, for sync

- **Drives sync by git.** A file changed on both sides is kept alongside as a `.sync-conflict-…` copy ("{} file(s) changed on both sides — your version was kept alongside as .sync-conflict-…", `keeper-sync/src/engine.rs:9383`, read in this worktree) `[REPO]`.
- **So the bank never shares a file between facts** `[INFERENCE]`:
  - a person, a clip, an embedding, a term and a tombstone are each their own file;
  - two devices adding different people, samples or terms never touch the same path.
- **A tombstone stops resurrection.** Without one, a device that has not seen a deletion would push the person's files back.
- **Reads tolerate bad files.** A read skips unreadable files and names them, so one bad file does not blank the bank.

### 9.4 Transcripts are files, and the index stays derived

- **Transcripts sit beside the media** they describe: `transcript.json` and `transcript.md` in a session folder, or `<stem>.transcript.json` and `.md` beside any other file. They sync with the drive and move with the folder.
- **The index is derived.** D-21 ("A search index is derived and disposable; the model is still the files", `docs/decisions.md:1097`) and the archive's own doc ("Deleting `archive.db` loses nothing the manifests do not carry", `archive/recordings.rs:1049-1051`) mean archive FTS can index transcripts later but never hold them `[REPO]`. Indexing is DW-338.

---

## 10. The recording's two audio tracks `[REPO]` (G1 §4)

- **The writer order is the track order.** keeper-rec's `makeSegmentWriter` (`tools/keeper-rec/Sources/keeper-rec/Capture.swift:916-1005`) adds its inputs in this order:
  - video (unless audio-only);
  - **system audio** (AAC 48 kHz, stereo);
  - **the microphone** (AAC 48 kHz, mono under echo cancellation, else stereo).
- **The docs say the microphone is its own track:** "its **own separate track** (never premixed with system audio …)" (`docs/recording.md:14-16`; AD-36, Story 19.3).
- **The camera file** carries "byte-for-byte the same processed audio" as the microphone (`Capture.swift:1441-1460`), so transcribing it would duplicate the microphone.
- **No track labels are written.** A grep for `languageCode|AVMetadataItem|mediaSelection|trackAssoci` over the sidecar finds nothing, and the manifest records devices but not track positions.
- **So role by position is a heuristic** `[INFERENCE, G1]`. With both devices on, the first audio track is system audio and the second is the microphone. A mono channel count is a hint only under echo cancellation.
- **Recorded as DW-340.**

---

## 11. The promise change (AD-350, D-29)

- **What stays literally true:**
  - recording adds **no network destination**;
  - the sidecar is untouched;
  - its network scan and every functional token in `zero-egress.test.ts` stay (`XMLHttpRequest`, `fetch(`, `WebSocket`, `sendBeacon`, `EventSource`, `http(s)://`, `axios`), with `Upload`, `Share` and `Cloud`.
- **What changes:**
  - transcription happens, on this Mac;
  - the `transcri` token leaves the gate;
  - the sentences in §2.5 that say otherwise are reworded or superseded.
- **Transcripts, clips, embeddings and terms** are files in the drive the person chose, and leave the Mac only as that drive's sync already carries its files.
- **The one network action transcription adds** is fetching the model files from the config repository's host, which is already listed (§7.4).
- **Two alternatives were weighed and rejected** `[INFERENCE]`:
  - **Keeping the old promise by renaming the feature** ("notes from audio") is the evasion the gate exists to catch.
  - **An exemption list in the gate** would be two rules for one word.

---

## 12. Not established (`[UNVERIFIED]` inventory)

| Claim | What was tried | Where it gets settled |
| --- | --- | --- |
| The fork links into the Tauri app on macOS 15/26 (the "Swift compatibility library symbols" issue) | R1 had no Mac | 87.4, on hesperia |
| Peak memory, wall time and first-load ANE compile for Parakeet v3 plus community-1 on the owner's Mac | no source measures keeper's hardware | 87.4, on hesperia |
| `.mov`/`.mp4` with a video track decode per audio track through the fork | R1: FluidAudio has no `AVAssetReader` path | 87.4, on hesperia |
| FluidAudio 0.17.4's `AsrModels.loadLocal` never reaches a download (the bridge's own paths are read, §7.5) | FluidAudio's source for `loadLocal` was not read; the Engine lane reports it | 87.4 review |
| community-1 has no speaker cap as a documented guarantee | none of the sources read states a cap or its absence | 87.4, with a meeting of more than eight speakers if one exists |
| community-1's DER on meeting audio against Nemotron 3's | no common benchmark in the sources | DW-331 |
| Cosine thresholds 0.70/0.50/0.60 for community-1 embeddings | no calibration source | DW-333 |
| Whether gix checks out the config clone's LFS files as pointers or as bytes | `config_repo.rs` has no LFS code; filter behaviour of the gix fork was not read | 87.3 on hesperia (hydration handles both) |
| Whether electra's Forgejo returns LFS object hrefs on its own host | not probed | 87.3 on hesperia |
| Which FluidAudio tag first carries the Polish blocklist fix (#847) | merged 2026-08-11; tags not checked | 87.4 |
| Parakeet v3's card WERs and the leaderboard's normalisation are identical, so "≈6.34" is comparable | not checked | none needed for the choice |
| The Cohere Transcribe port's languages, size and accuracy through FluidAudio; whether a 2B Granite runs usefully on the Neural Engine | FluidAudio's tree shows `Documentation/ASR/Cohere.md`, not read; no Granite port seen | a revisit of AD-340 if Polish accuracy disappoints |
| Nemotron 3 Diarization on Polish meetings | its evaluation list has no Polish set | DW-331 |
| `speaker-diarization-coreml` provenance for redistribution (#927) | the issue is open | DW-341 |

---

## 13. Sources

**External, read on 2026-09-28:**
- NVIDIA, *Know Who Spoke When: Build Real-Time, Multi-Speaker AI with NVIDIA Nemotron 3 Diarization* — <https://huggingface.co/blog/nvidia/nemotron-diarization>
- NVIDIA, Nemotron 3 Diarization model card (OpenMDW-1.1) — <https://huggingface.co/nvidia/Nemotron-3-Diarization>
- NVIDIA, Nemotron 3.5 ASR model card (OpenMDW-1.1) — <https://huggingface.co/nvidia/nemotron-3.5-asr-streaming-0.6b>
- NVIDIA, Parakeet TDT 0.6B v3 model card (CC-BY-4.0) — <https://huggingface.co/nvidia/parakeet-tdt-0.6b-v3>
- NVIDIA, NeMo Speech — <https://github.com/NVIDIA-NeMo/Speech>
- NVIDIA, NeMo-Speech.cpp — <https://github.com/NVIDIA/NeMo-Speech.cpp>
- CodeSOTA, *Speech-to-Text Benchmarks* — <https://codesota.com/speech-to-text>
- FluidInference, FluidAudio (Apache-2.0) — <https://github.com/FluidInference/FluidAudio>
- FluidInference, fluidaudio-rs (MIT) — <https://github.com/FluidInference/fluidaudio-rs>, through R1
- FluidAudio issues #661, #878, #927 (on `speaker-diarization-coreml`), #967, #847 and #862, through R1

**Digests:**
- R1, `/tmp/e87-R1FluidAudioRs.md`, pinned at fluidaudio-rs `bd1f0f3`, FluidAudio `v0.14.8` (`56607d9`) and main `20d4f0b`;
- G1–G4, `/tmp/e87-G{1RecordingDrives,2ConfigRepo,3Conventions,4ShellPatterns}.md`.

**Repository, at `dfa843e3`:**
- `src-tauri/crates/keeper/src/{ipc,lib,sync_ipc,account_ipc,voice_ipc,voice_macos,macos_version,zero_egress}.rs`;
- `src-tauri/crates/keeper-core/src/{vm,registry,recording,notes/recording_note}.rs`, `keeper-core/src/config/keys.rs`, `keeper-core/src/voice/`;
- `src-tauri/crates/keeper-sync/src/{config_repo,profile/mod,profile/folder}.rs`, `keeper-sync/src/lfs/`;
- `tools/keeper-rec/Sources/keeper-rec/Capture.swift`;
- `src/components/layout/files-pane.tsx`, `src/components/recording/zero-egress.test.ts`, `src/test/bots-surface-stays-out-of-recording.test.ts`;
- `docs/{decisions,egress,recording,account,constraints-and-limitations}.md`, `AGENTS.md`;
- `_bmad-output/planning-artifacts/research-ai-chat-2026-09-02.md` §10.3 (the earlier on-device STT survey).
