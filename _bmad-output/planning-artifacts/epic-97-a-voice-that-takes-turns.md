# Epic 97 — A voice that takes turns

created: '2026-10-02'
status: planned 2026-10-02; build follows in story order on the agents stack
source: the owner's round 3 of 2026-10-02 ("**Private option** (keeps D-5 - yes for the option"), and the voice option the program put to the owner, pinned by the coordinator as P13 and rulings R14 and R20; the coordinator accepted this epic's open-question readings in ruling R24 ((10) and (11) by name) and the consistency review of 2026-10-02 as ruling R29 (`_bmad-output/planning-artifacts/agents-review-consistency-2026-10-02.md`; F10, F14 and F22 land here). The rulings are in `_bmad-output/planning-artifacts/agents-coordinator-decisions-2026-10-02.md`. Other inputs:
- `_bmad-output/planning-artifacts/architecture/architecture-keeper-2026-07-03/ARCHITECTURE-AGENTS.md` — AD-410 and AD-411, binding; AD-384 (a spoken turn to the main agent, epic 91) beneath them;
- `_bmad-output/planning-artifacts/research-agents-2026-10-02.md` — §8.1 (what "private" means), §8.4 (Silero, Smart Turn, interruption semantics), §8.6–§8.8 (on-device recognition, keeper's voice and model loading today, what P13 changes), §8.9's latency budget, §13 #35–#38;
- the digest D3 (*Inventory B*: on-device inference today) and G5 §2 (voice duplex, echo, barge-in), as cited.

Line numbers are in the `agents-plan` worktree on 2026-10-02.
binds: FR-814…FR-816; NFR-114; NFR-119 (97.1's model licences); AD-410, AD-411 (allocated by the architecture, not here); UX-DR142…UX-DR143; D-36. Deferred items in DW-402…DW-406.
- **The previous ceilings:** the program's (`_bmad-output/planning-artifacts/agents-program-map-2026-10-02.md`, C1): epic 88, AD-359, FR-766, NFR-111, UX-DR126, DW-354, D-30. The architecture allocated AD-360…AD-416, FR-767…FR-822 and NFR-112…NFR-122; D-31…D-36 are in `docs/decisions.md`. This epic allocates no AD, FR, NFR or D number. Its deferred items are DW-402…DW-406 (DW-402 placed by the architecture); its UX decisions are UX-DR142…UX-DR143.
- **No earlier allocation.** On 2026-10-02 a grep of `_bmad-output`, `docs`, `src`, `src-tauri/crates`, `tools`, `AGENTS.md`, `README.md` and `CLAUDE.md` for `DW-E97-` and `UX-DR-E97-` found only DW-402 (`ARCHITECTURE-AGENTS.md` § *What stays out*; `docs/decisions.md` § D-36). This epic's own deferred items start at DW-403, and its UX decisions at UX-DR142. The 2026-10-02 review wave opened no deferred item here.
see-also:
- D-5 (voice is the system's, on the device, armed by a person; `docs/decisions.md:169-215`), D-29 (models from the account's `_models/`), D-36 (`docs/decisions.md` § D-36, amends D-5 for the turn models);
- AD-165…AD-175 (the turn machine and half duplex), AD-205…AD-209 (the iPhone's echo tail gate), AD-208 (what the person said decides what follows), AD-213 (both Apple platforms full duplex), AD-214 (answers spoken sentence by sentence), AD-341 (models hydrated from the config repository);
- NFR-50 and the `voice_on_device` source scan (`keeper-core/tests/voice_on_device.rs`), which every change here must keep green.

## The owner's ask

Verbatim, round 3 (2026-10-02):

> - **Private option** (keeps D-5 - yes for the option

That line is the owner's whole voice instruction in this program. The rest of this epic is what that option was, as the program described it and the coordinator pinned it (P13): "On-device only. Silero VAD + Smart Turn v3 (ONNX) loaded from the owner's config repo `_models/` (D-29 precedent; nothing bundled, nothing downloaded). Backchannel rule before barge-in stops speech. Truncate the assistant turn at the played sentence and log `heard_until`." Ruling R14 made barge-in pause-first; ruling R20 made the turn models a D-29-style amendment of D-5. The story titles' quotes ("mhm" is not an interruption) are the program's, not the owner's.

## The verdict, ask by ask

| # | The ask | Verdict | How it is met | Mechanism |
| --- | --- | --- | --- | --- |
| 1 | "**Private option** (keeps D-5" | **kept, by construction** | Speech still becomes text on the device through the system recogniser with `requiresOnDeviceRecognition`; the new models decide only *when* a turn ends and *whether* an interruption is one; no audio leaves the device; the models come from the person's organisation's `_models/`, never bundled or downloaded from elsewhere. | AD-410, D-36 |
| 2 | P13: end of turn by Silero + Smart Turn | **planned** | A complete sentence ends the turn about 200 ms after speech stops; an incomplete one waits for today's 1800 ms pause. | AD-411; 97.2 |
| 3 | P13 / R14: a backchannel does not stop the agent | **planned, pause-first** | Speech pauses the moment the person speaks; "mhm" resumes it, the stop phrase ends the turn, anything else stops it and becomes the next question. | AD-411; 97.3 |
| 4 | P13: the agent knows where it was stopped | **planned** | The speaking device sends `dev.keeper.agent.heard`; the owning host logs a `heard` line; the next turn's context cuts the answer there and says so. | AD-411; 97.3 |

## What the triage found

| Need | Verdict | Evidence |
| --- | --- | --- |
| End of turn | **present, by a timer** | `END_OF_UTTERANCE_PAUSE = 1800 ms` (`keeper-core/src/voice/turn.rs:66`), applied as `Listening`'s silence budget (`turn.rs:306-312`), because the continuous recogniser never ends an utterance itself (G5 §2). |
| Barge-in | **present, stop-first** | "Barge-in stops speech first" (`turn.rs:18-22`); `(Speaking, SpeechDetected(_)) → Listening, [StopSpeaking, OpenMicrophone]` (`turn.rs:239-244`); pinned by `voice_barge_in_stops_speaking_before_anything_else` (`keeper-core/tests/voice_turn.rs:301`), `voice_driver_orders_port_calls_for_barge_in` (`:1024`) and `voice_half_duplex_barge_in_still_stops_speech_before_listening` (`tests/voice_platform.rs:318`). |
| The ports' barge-in signal | **present** | While speaking, a non-empty transcript is `SpeechDetected` (`keeper/src/voice_ios.rs:1211`; `voice_macos.rs:1130-1131`, only when full duplex); stop is `stopSpeakingAtBoundary(Immediate)` (`voice_ios.rs:2016-2026`, `voice_macos.rs:1858-1865`). There is no pause call and no VAD. |
| Full duplex | **present on both Apple platforms** | `full_duplex: true` for iOS and macOS since AD-213 (`keeper-core/src/voice/platform.rs:92`, `:107`); voice processing on the input node (`voice_ios.rs:1617`, `voice_macos.rs:1475`); an 800 ms tail gate after every utterance (`voice_ios.rs:221`, `voice_macos.rs:231`). |
| Sentences of an answer | **present, without offsets** | `Segmenter` yields trimmed sentences as the answer streams (`keeper-core/src/voice/speech.rs:173-205`); nothing records where each sentence ends in the answer's text. |
| Models from `_models/` | **present, for transcription** | `CONFIG_MODELS_DIR = "_models"` (`transcription/models.rs:19`), `MODELS_TOML` (`:22`), `ModelSet::from_toml` (`:82`), `ModelRole { Asr, Diarizer }` (`:121-124`), `required_paths` and `missing` (`:154-165`), `available` (`:180`), `choose` (`:221`); Settings picks `transcription.asr_model` / `transcription.diarization_model` (`config/keys.rs:963-981`). |
| Models on the phone | **absent** | `spawn_models_fetch` returns unless transcription is supported (`keeper/src/transcribe_ipc.rs:522-524`), and the phone's engine is `AbsentEngine` (`:146-149`), so the phone fetches nothing; hydration takes the whole `_models/` directory (`account_ipc.rs:227-286` → `keeper-sync/src/config_repo/hydrate.rs:132`) — "hundreds of megabytes" (`transcribe_ipc.rs:520-521`). |
| An ONNX runtime | **absent** | No `ort` in `Cargo.lock`; the transcription engine is FluidAudio over Core ML (`.mlmodelc`), macOS only, and cannot host an ONNX model (D3 *Inventory B*). |
| The on-device gate | **present** | `voice_on_device.rs` reads `keeper-core/src/voice/**` and every shell `voice*.rs` and fails on a request without `setRequiresOnDeviceRecognition(true)` or a network API (`:5-9`, `:138-186`). |

## The one sentence

**keeper's voice waits 1.8 seconds after every sentence, stops dead at a "mhm", and never tells the agent where the person stopped listening.** The fix is three small things on the device, none of which sends audio anywhere:
- **Two turn models** from the person's own `_models/`: one that hears speech start and stop (Silero VAD), one that hears whether a sentence is finished (Smart Turn v3).
- **A turn machine that pauses before it stops**, and lets the utterance decide: continue, end, or ask anew.
- **A mark where the person stopped hearing**, carried to the agent's log and into its next turn.

## Decisions this epic implements

D-36 (`docs/decisions.md` § D-36): keeper's voice may take its turns from models the person's organisation distributes, still on the device. AD-410 and AD-411 are the architecture's; this epic restates neither. AD-208 is amended to pause-first by AD-411 (ruling R14); the amendment replaces `turn.rs:18-22`'s sentence and its three pinning tests (97.3).

## What earlier decisions said, and what this epic amends

| The earlier decision | What it said | What this epic needs | The amendment |
| --- | --- | --- | --- |
| **D-5** (`docs/decisions.md:169-215`) | keeper "will not ship a model of its own" | two small models to decide the end of a turn | **Amended by D-36**, as D-29 amended D-4: models from the person's organisation's `_models/`, run on the device; nothing bundled or downloaded elsewhere. Recognition, arming and the no-network rule stand. |
| **AD-208 and `turn.rs:18-22`** | "Barge-in stops speech first." | "mhm" must not stop the answer | **Amended to pause-first** (ruling R14): speech pauses at once; the utterance decides. Intent kept: nothing talks over the person. |
| **`END_OF_UTTERANCE_PAUSE`** (`turn.rs:66`) | the turn ends 1800 ms after the last partial | a turn that ends with the sentence | **Demoted to the fallback** (AD-411): used when the models are absent, or say "not finished". |
| **`spawn_models_fetch`** (`transcribe_ipc.rs:522-524`) | a machine that cannot transcribe fetches no models | the phone needs the turn models | **Scoped (97.1):** a machine fetches the roles it can run — the phone the two turn roles only. |

## Requirements

Copied from the architecture's *Requirements allocated here*; not restated, not renumbered.

| id | statement | epic.story | AD |
| --- | --- | --- | --- |
| FR-814 | The voice activity and end-of-turn models load from the account's `_models/` like the transcription models; nothing is bundled or downloaded elsewhere; without them voice keeps today's pause rule and Settings says what is missing. | 97.1 | AD-410 |
| FR-815 | keeper ends a spoken turn when the sentence is finished, not after a fixed pause. The repo proves the turn machine and the model's decision on recorded audio; on the Mac and the iPhone it holds once 97.2's device runs are recorded in `docs/agents.md` § Measured. | 97.2 | AD-411 |
| FR-816 | "Mhm" while the agent speaks pauses the answer and resumes it; anything else stops it and becomes the next question; when the person stops it, the agent knows the last sentence they heard. | 97.3 | AD-411 |
| NFR-114 | **A spoken turn ends when the sentence does.** With the turn models present, on hesperia and the iPhone, p95 over ≥ 20 turns per device from the voice log's `speech_end_ms` (the VAD's speech-end frame), `utterance_end_ms`, `finish_recognition_ms`, `sent_ms`, `onset_ms`, `pause_ms`, `final_words_ms` and `resume_ms`, recorded in `docs/agents.md` § Measured: a complete sentence reaches `FinishRecognition` within 300 ms of speech end (200 ms hangover plus the model) and is sent at most 600 ms later, never after the 1800 ms pause fallback; a backchannel pauses speech within 150 ms of its onset and resumes it within 300 ms of its speech end when its duration decides it, or of the recogniser's final words when the word list does. Without the models it does not apply. | 97.2, 97.3 | AD-411 |
| NFR-119 | **The licence firewall holds.** Every ported module names a permissive licence in its `UPSTREAM.md`; every new crate passes `cargo deny`; what cargo cannot see is covered in its story — `ort`'s prebuilt ONNX Runtime library is checked by hand in 97.1, and the Android build's Gradle dependencies are listed with their licences in `docs/android.md` and checked by a test in 98.3; AGPL and GPL software (Sygnal, ntfy's GPL option, NanoKVM firmware, Element Call) is run as a separate service or read as a protocol, never linked; every model in `_models/` names its licence. | 89.1, 94.1, 96.5, 97.1, 98.3 | AD-396, AD-410 |

## Built on

The turn machine and both Apple voice ports as they are (Epics 62–68); the `_models/` loader and hydration (Epic 87); talking to the main agent through its room (story 91.4, AD-384), which is where `heard` events go; the session log's `heard` line kind and the replay that builds a turn's context (89.5). Pause-first and end of turn by meaning apply to every spoken turn, ⌘9's included; `heard_until` is recorded only for agent sessions, which have a log (DW-405).

## Open questions for the coordinator

Each has the reading this plan builds to, marked as such. None is resolved silently. The coordinator accepted every reading below (ruling R24, (10) and (11) by name); the consistency review then named NFR-114's clock points (F10), which Q4 and Q8 now cite.

- **Q1. "An engine port method", and which port.** The transcription engine is `SpeechEngine` (`transcription/engine.rs:17-42`): a serialized FluidAudio worker on the Mac, `AbsentEngine` everywhere else, offline file-shaped calls (`decode`, `transcribe`, `diarize`). Turn models run on live 32 ms frames, on the phone too.
  - **Plan's reading:** a sibling port in `keeper-core`, `voice::turn_models::TurnModels`, with two methods — `speech_probability(frame: &[f32; 512]) -> f32` and `turn_complete(window: &[f32]) -> f32` — implemented in the shell over `ort` for macOS and iOS, absent elsewhere. **Rejected:** two more methods on `SpeechEngine` — every transcription engine would have to answer for live audio, and the phone would need a `SpeechEngine` that transcribes nothing.
- **Q2. "no" in the backchannel list.** AD-411's list holds `no` — Polish for "yeah" and English for "no". An English "no" while the agent speaks is an objection, and resuming over it is the failure this epic exists to prevent.
  - **Plan's reading (accepted, R24(10)):** the list is per language, keyed by the voice locale's language (`bots.voice_locale`, `config/keys.rs:484`): `en`: mhm, uh-huh, yeah, yep, aha, okay, right; `pl`: mhm, aha, tak, no, okej, jasne; any other language: mhm, aha. The duration rule (under 600 ms of speech) applies in every language.
- **Q3. "The last sentence actually played."** When the person stops the answer in the middle of sentence 3, did they hear sentence 3?
  - **Plan's reading:** no. `heard_until` is the end of the last sentence the synthesiser *finished*; the interrupted sentence is not counted. An agent that thinks the person heard less repeats a little; one that thinks they heard more skips what they never heard.
- **Q4. The model can end the turn before the recogniser's last words arrive.** Apple's partials trail the audio by a few hundred milliseconds; ending at 200 ms after speech stops can send a question without its last word.
  - **Plan's reading:** `UtteranceEnd` takes the turn to `Finishing { heard }` with `Effect::FinishRecognition` (the request's `endAudio`); `FinalHeard` then sends, and if it has not come within 600 ms the turn sends what it heard. NFR-114's 300 ms is measured from the VAD's speech-end frame (`speech_end_ms`) to `FinishRecognition` (`finish_recognition_ms`), the moment keeper stops waiting for more speech; the send (`sent_ms`) follows at most 600 ms later. Each turn's clock points are one voice-log record (F10, 97.2 #7).
- **Q5. Without the models, a backchannel's duration cannot be measured,** and the pause can only start at the recogniser's first partial.
  - **Plan's reading:** without the models, pause-first still applies (it needs no model), the backchannel rule is the word list alone, and NFR-114 does not apply (it is stated "with the turn models present").
- **Q6. The phone hydrates nothing today** (`transcribe_ipc.rs:522-524`) **and hydration is whole-directory** (`hydrate.rs:132`).
  - **Plan's reading (accepted, R24(11)):** hydration takes a set of role directories; the Mac keeps fetching everything it can run, the phone only `models.toml` and the `vad` and `smart_turn` directories (about 10 MB, §8.4).
- **Q7. The two models' own pre- and post-processing are not in the research.** Silero's frame size and state shape, and Smart Turn v3's exact input (Whisper log-mel features over the last 8 s, §8.4, R5 (c)), are read from upstream when 97.2 is built.
  - **Plan's reading:** `keeper-ported::smart_turn` ports the feature extraction from the upstream inference script at a pinned commit, named in its `UPSTREAM.md`; its golden test compares features against values computed by the upstream script and committed as a fixture beside the waveform that produced them. No weights are in the fixture.
- **Q8. A false onset pauses the answer.** Echo that voice processing lets through (the iPhone measured enough to need an 800 ms tail gate, AD-205…AD-209) may look like speech to the VAD while keeper is talking.
  - **Plan's reading:** an onset is the VAD's speech probability ≥ 0.5 for three consecutive 32 ms frames (96 ms); `onset_ms` is the first of the three. A pause that the utterance then resolves to nothing — no words, under 250 ms of speech — resumes as a backchannel would, and is counted in the voice log as `false_onset`. The rate is measured on both devices (97.3's device run, #11).

## Stories

Every story names its rung in the stack (*Stack rungs*, below).
- **The shell is by inspection.** Everything under `src-tauri/crates/keeper/**` awaits CI's macOS job and `check:rust:macos` on hesperia; the iOS half also needs `bun run install:ios` to the owner's iPhone.
- **The turn models are not in CI.** They live in the owner's config repository; tests that need the real weights read them from `KEEPER_TEST_MODELS_DIR` and are run on hesperia, named in the PR as such. Every pure test runs everywhere.
- **Every new pure behaviour test is mutation-proved.**
- **Names are suggestions the lanes agree on.**

### 97.1 — Turn models from `_models/`

**Intent:** "**Private option** (keeps D-5"; P13's "loaded from the owner's config repo `_models/` (D-29 precedent; nothing bundled, nothing downloaded)". **Rung:** **epic97-models**. AD-410; D-36; Q1, Q6.

**Files:**
- `keeper-core/src/transcription/models.rs`: `ModelRole::Vad` and `ModelRole::SmartTurn` (required `model.onnx` each); `ModelSet.vad_dir: Option<String>`, `smart_turn_dir: Option<String>` from optional `[vad] dir` and `[smart_turn] dir`; `turn_required_paths` and `turn_missing`, separate from `required_paths`/`missing`; `choose` for the two roles.
- `keeper-core/src/config/keys.rs` and `docs/settings-keys.md`: `transcription.vad_model`, `transcription.smart_turn_model` (user-global text, blank = the repository's choice), mirroring `:963-981`.
- `keeper-core/src/voice/turn_models.rs` (new): the `TurnModels` port (Q1), `TurnModelsState { Ready | Missing { files } | Refused { sentence } | NoAccount }` and its sentence.
- `keeper-sync/src/config_repo/hydrate.rs`: `hydrate_lfs_dir` takes the set of role directories to hydrate (Q6).
- the shell: `transcribe_ipc.rs`'s fetch (the roles this machine can run), `turn_models.rs` (new, `cfg(any(target_os = "macos", target_os = "ios"))`: `ort` sessions over the two `model.onnx` files, one worker thread, the reply-channel shape of `transcribe_macos.rs`), the turn-models state on `VoiceWakeVm` (`keeper-core/src/vm.rs:7462`, read by `voice_wake_get`, `voice_ipc.rs:634`), Settings › Voice's line (UX-DR142).
- `src-tauri/Cargo.toml`, `keeper/Cargo.toml`: `ort` in the Apple target tables only.
- `docs/transcription.md` § *Models* (the two roles), `docs/constraints-and-limitations.md` (the voice row), `docs/egress.md` § *Voice adds no egress* (unchanged conclusion, one sentence on the turn models' path).

**Acceptance:**
1. **The grammar.** A `models.toml` with `[vad] dir = "silero-vad"` and `[smart_turn] dir = "smart-turn-v3"` parses to those directories; without either section the set has none, and parses as before; a `dir` with a path separator is refused exactly as the ASR role's is. Test: `models_toml_turn_roles` (keeper-core, extending `models.rs`'s tests).
2. **Transcription is untouched.** `required_paths`/`missing` for a set with turn roles list exactly the ASR and diarizer files they listed before; a repository with the turn roles but an incomplete turn set still transcribes. Test: `turn_roles_do_not_change_transcriptions_required_files` — the regression a shared list would cause.
3. **Missing is named.** `turn_missing` lists exactly the absent files, `silero-vad/model.onnx` and/or `smart-turn-v3/model.onnx`; both present → `Ready`; none configured → `Missing` naming the two sections. Test: `turn_missing_lists_exactly_the_absent_files` (pattern of `missing_lists_exactly_the_absent_files`, `models.rs:296`).
4. **Settings picks.** `transcription.vad_model` naming a complete folder replaces the repository's; naming a missing or incomplete one is refused with a sentence naming it and Settings › Transcription — no fallback, as `choose` refuses for ASR (`models.rs:221`). Test: `turn_model_pick_refuses_an_incomplete_folder`; the generated settings-keys table carries both keys (`docs/settings-keys.md`'s existing doc test).
5. **Each machine fetches what it can run.** On the Mac every role is hydrated; on the phone only `models.toml` and the two turn directories; a machine with neither transcription nor voice fetches nothing. Test: `hydrate_lfs_dir_takes_only_the_named_roles` (keeper-sync, against a local bare repository with LFS objects for three role directories — a real repository, the risk being a filter that still downloads everything).
6. **Nothing else is contacted.** The hydration still reaches only the config repository's host (`account_ipc.rs:224-226`); `voice_on_device.rs` stays green with `turn_models.rs` and every changed `voice*.rs` in its scan. Test: the existing scan, its file floor raised by one.
7. **The runtime's licence is recorded.** `ort` passes `cargo deny check`; the ONNX Runtime library it links on macOS and iOS, which is not a cargo dependency (§5.10), has its licence (MIT `[UNVERIFIED]` until read) checked by hand and recorded in `docs/constraints-and-limitations.md` beside its version, as research §13 #35 asks. Proof: the PR quotes the licence file of the exact runtime build linked.
8. **The models' licences travel with them.** `docs/transcription.md` says each role's directory carries its licence file (Silero VAD MIT, Smart Turn v3 BSD-2-Clause), as DW-341 records for the transcription models; keeper reads no licence, and the owner's repository is where it lives.
9. **The state is visible.** `TurnModelsState` reaches Settings › Voice as one line (UX-DR142): *Turn models ready*, or *Turn models missing: `<file>` — keeper waits 1.8 s after you stop*, or the refusal sentence. Test (front, vitest): each state's line through the mock shell; absent where voice is unavailable (AD-27).
10. **They load and answer on real hardware** (shell, `KEEPER_TEST_MODELS_DIR`): on hesperia, both sessions load from the hydrated directory and `speech_probability` over 1 s of silence stays under 0.1 while over a recorded spoken fixture it exceeds 0.5. Test: `turn_models_load_and_answer` (shell, `#[ignore]` without the directory, run by `check:rust:macos` on hesperia with it set — named in the PR).

**Operator actions:**
- [ ] Add `_models/silero-vad/model.onnx` (+ its `LICENSE`) and `_models/smart-turn-v3/model.onnx` (int8, + `LICENSE`) to the owner's config repository as LFS objects, and `[vad] dir = "silero-vad"`, `[smart_turn] dir = "smart-turn-v3"` to `_models/models.toml`; push. (keeper downloads nothing from Hugging Face; this step is the owner's.)
- [ ] On hesperia and on the iPhone: Settings › Voice shows *Turn models ready* after the next sync.

**Shell crate:** yes — the `ort` runtime beside `transcribe_macos.rs`, the iOS port, the fetch's role choice and the view model. Gated on CI's macOS job, CI's iOS compile check (`ci.yml:90`), and `check:rust:macos` plus `bun run install:ios` on hesperia.

**binds:** FR-814, NFR-119, AD-410, D-36, UX-DR142

**As built (rung 1, `agents-97-models`, 2026-10-05; Rust only — the UX-DR142 line is rung 2).** Codemap Q1–Q17 applied as the contract accepted them:
- **Roles and picks:** `ModelRole::{Vad, SmartTurn}` (`model.onnx` each), `ModelSet.vad_dir`/`smart_turn_dir`, `turn_required_paths`/`turn_missing` apart from transcription's lists, `choose_turn` (the refusal names *this device*, the settings key and how to replace or clear it in the account's `settings.toml`, since no picker exists — review R97-08). The two keys are user-global text; Settings has no picker yet (DW-490).
- **Fetch by group (Q5):** `hydrate_lfs_dir` takes a `HydrateGroup` — `transcription` (every folder but the turn folders: the repository's, the picks, and any folder holding a `model.onnx` — R97-03) and `turn` (the repository's and the picks') — each with its own completion marker and state file, and each reading only its own sections of `models.toml` into its digest (`HydrateGroup.manifest`, R97-04); `models::fetch_groups` is the decision. The test is the loopback LFS harness, not a bare repository (codemap §3 row 9).
- **The port (Q7):** `voice::turn_models::{TurnModels, VadStream}` — a stateful VAD stream over 512-sample frames and `turn_complete` over the `[80][800]` features — and `TurnModelsState`, which mirrors transcription's states (adds *fetching* and *failed*, codemap §3 row 19). The shell's `voice_turn_models.rs` (named so `voice_on_device` scans it, codemap §3 row 5) opens both sessions and hands them to core's worker (`spawn_turn_models`); whether to load is `TurnDisk::load` — an account and the current turn generation, the same facts as the line (R97-02), unloaded on forget — only the newest load is published (`LoadSlot`, R97-06), and a graph not named as keeper reads it is refused at load, a stopped worker an error (`TurnGraph`, R97-05); `VoiceWakeVm.turnModels` is optional.
- **Runtime (Q2):** `ort` `=2.0.0-rc.13`, default features off, CPU only, Apple silicon macOS and the arm64 iPhone (not the simulator, R97-07); the static ONNX Runtime 1.28.0 (MIT) is recorded in `NOTICE`, its LICENSE and ThirdPartyNotices are in `licenses/`, and both apps carry them (R97-01); `project.yml` names CoreML and libc++. The file floor of #6 is a name check in `voice_on_device` (codemap §3 row 4), which also forbids ONNX Runtime's model download.
- **Corrections:** "Settings › Voice" is `BotVoiceWake` in Settings › Bots (§3 row 6); Smart Turn is v3.2 (`smart-turn-v3.2-cpu.onnx`, §3 row 12); the two files are 11.0 MB (§3 row 15); #10's test reads the recording from `KEEPER_TEST_SPEECH` and runs by hand, since `check:rust:macos` forwards no environment (§3 row 10).
- **Operator actions, in this order:** (1) add `_models/silero-vad/` (`silero_vad.onnx` from snakers4/silero-vad 1e261b03 as `model.onnx`, 2,327,524 B, sha256 `1a153a22f4509e292a94e67d6f9b85e8deb25b4988682b7e174c65279d8788e3`, + `LICENSE`) and `_models/smart-turn-v3/` (`smart-turn-v3.2-cpu.onnx` from huggingface.co/pipecat-ai/smart-turn-v3 f766f81d as `model.onnx`, 8,679,182 B, sha256 `2bb026316b14a660486a75b1733cd3fbab8c2fd0314dc9af7be49f8cca967e4f`, + `LICENSE`) as LFS objects, two rows in `_models/README.md`, push; (2) **only after every signed-in Mac runs a 97.1 build**, add `[vad] dir = "silero-vad"` and `[smart_turn] dir = "smart-turn-v3"` to `_models/models.toml` and push — an older keeper refuses the whole file (Q4); (3) on hesperia and the iPhone, the voice settings show *Turn models ready* after the next sync.

### 97.2 — End of turn by meaning

**Intent:** P13's "Silero VAD + Smart Turn v3"; FR-815: "keeper ends a spoken turn when the sentence is finished, not after a fixed pause, on the Mac and on the phone." **Rung:** **epic97-turns**. AD-411 (end of turn); AD-396 (`keeper-ported::smart_turn` lands here, with its first consumer); Q4, Q7.

**Files:**
- `keeper-ported/src/smart_turn/` (new) with `UPSTREAM.md` (pipecat-ai/smart-turn, the commit, BSD-2-Clause, "feature extraction only: the Whisper log-mel front end over the last 8 s at 16 kHz; no weights, no inference"): the features as a pure function of samples.
- `keeper-core/src/voice/end_of_turn.rs` (new, pure): `EndOfTurn` — frames in (speech probability per 32 ms frame), events out: onset, speech end, and, after a 200 ms hangover, a request to score the last 8 s; the score (≥ 0.5) decides `UtteranceEnd`; a new onset cancels a pending decision.
- `keeper-core/src/voice/turn.rs`: `TurnEvent::UtteranceEnd`, `TurnState::Finishing { heard }`, `Effect::FinishRecognition` (Q4); `silence_budget` unchanged as the fallback.
- `keeper-core/src/voice/timings.rs` (new, pure): the `turn_end` record — `speech_end_ms`, `utterance_end_ms`, `finish_recognition_ms`, `final_words_ms` (absent when the 600 ms fallback sent), `sent_ms`, `ended_by` (`model` | `pause`) — as the detail of a new `VoiceEventKind::TurnEnd` (`keeper-core/src/voice/events.rs:32`), and `measure(records)`, which turns a set of records into NFR-114's figures and refuses fewer than 20 turns (F10).
- the shell: `voice_macos.rs` and `voice_ios.rs` resample the existing tap to 16 kHz frames, feed `EndOfTurn` while `Listening`, run the score on the turn-models worker, deliver `UtteranceEnd`, and execute `FinishRecognition` as the request's `endAudio`; `voice_log.rs` records each turn's `turn_end`, every clock point on `voice_log::now_ms`'s clock (`keeper/src/voice_log.rs:26`), a VAD frame's time taken when the tap delivers it.
- `docs/transcription.md` is untouched; `docs/ios.md` and the macOS voice chapter gain *How a spoken turn ends*.

**Acceptance:**
1. **The features match upstream.** For two fixture waveforms (2 s and 9 s of recorded speech, the second truncated to its last 8 s), `smart_turn::features` equals the upstream script's output within 1e-4 per bin. Test: `smart_turn_features_match_upstream` (keeper-ported, pure, fixtures committed with the script's commit). `check:ported-pure` and the `UPSTREAM.md` licence test pass.
2. **The detector's timing.** Over synthetic probability sequences: speech end is the first frame below 0.35 after at least 250 ms of speech; the score is requested exactly 200 ms later; a new onset within the hangover cancels it; a score ≥ 0.5 yields `UtteranceEnd`, below yields nothing. Test: `end_of_turn_timing` (pure, frame-exact).
3. **The table.** `(Listening{heard≠""}, UtteranceEnd) → Finishing{heard}, [FinishRecognition]`; `(Finishing, FinalHeard(t)) → Heard{t}` and the send follows as today; `(Finishing, Silence)` after its 600 ms budget → `Heard{heard}`; `(Listening{""}, UtteranceEnd)` is ignored; `UtteranceEnd` in any other state is ignored. Test: extended `keeper-core/tests/voice_turn.rs` (`voice_utterance_end_finishes_recognition`, `voice_finishing_sends_the_final_words`, `voice_finishing_falls_back_after_600_ms`).
4. **The pause stays the fallback.** With no turn models, or a score below 0.5, a turn ends exactly as today, 1800 ms after the last partial (`turn.rs:66`); the existing silence-budget tests are unchanged and green.
5. **No audio leaves the device.** `voice_on_device.rs` stays green with every changed file in its scan; the frames go from the tap to the worker thread and nowhere else (no new egress row).
6. **The phone's runtime links.** CI's iOS compile check passes with `ort` in the iOS target table (`ci.yml:90`); the link into the app is proved by `bun run install:ios` on hesperia (operator).
7. **The clock points are logged and measured** (F10). Every turn writes one `turn_end` record; `measure` gives the p95 of `finish_recognition_ms − speech_end_ms` over `model` turns, the p95 of `sent_ms − finish_recognition_ms`, and every `pause` turn sent with less than 1800 ms of silence before it. Test: `turn_timings_record_and_measure` (pure: a record round-trips through the voice log's detail; `measure` over a fixture set of 24 records gives the hand-computed p95s; 19 records are refused).
8. **On the device — the gate for FR-815 and NFR-114's end-of-turn half** (a device run, F14). On hesperia and on the iPhone, with the turn models present: at least twenty spoken questions per device, about half ending in a complete sentence and half trailing off ("and the second one is…"); `measure` over the voice log gives `finish_recognition_ms − speech_end_ms` p95 ≤ 300 ms on complete sentences and `sent_ms − finish_recognition_ms` ≤ 600 ms; no trailing-off question is sent before the 1800 ms fallback; no question lost its last word (each sent text compared with what was said, Q4). The figures, n, device, OS version, build and date are recorded in `docs/agents.md` § *Measured*; the story is done only when both devices' rows are there.

**Device run (the steps behind #8):**
- [ ] Twenty or more spoken questions each on hesperia and on the iPhone, as #8 describes, with the voice log exported after each run.
- [ ] `measure` run over each export; its figures written into `docs/agents.md` § *Measured*, one row per device.

**Shell crate:** yes — `voice_macos.rs`, `voice_ios.rs`, `voice_log.rs`, the turn-models worker. Gated on CI's macOS job and iOS compile check, `check:rust:macos`, and the device run (#8).

**binds:** FR-815, NFR-114, AD-411, AD-396

### 97.3 — "Mhm" is not an interruption, and the agent knows where you stopped it

**Intent:** P13's "Backchannel rule before barge-in stops speech. Truncate the assistant turn at the played sentence and log `heard_until`"; ruling R14's pause-first. **Rung:** **epic97-turns**. AD-411 (backchannels, `heard`); AD-208 amended; Q2, Q3, Q5, Q8; F10, F14.

**Files:**
- `keeper-core/src/voice/turn.rs`: `TurnState::Paused { heard, speech_ms }`; `Effect::PauseSpeaking`, `Effect::ContinueSpeaking`; the module documentation's first rule rewritten to pause-first.
- `keeper-core/src/voice/backchannel.rs` (new, pure): the per-language list (Q2) and `is_backchannel(words, speech_ms, language)`.
- `keeper-core/src/voice/speech.rs`: `Segmenter` (and epic 91's `AnswerFollower` over it, story 91.4) reports each sentence's end offset in the answer's text, counted in Unicode scalar values.
- `keeper-core/src/voice/timings.rs` (97.2's): the `pause` record — `onset_ms` (the first of the three onset frames), `pause_ms` (`PauseSpeaking` executed), `speech_end_ms`, `final_words_ms` (when the word list decided), `resume_ms` (`ContinueSpeaking` executed; absent on a stop), `decision` (`backchannel_duration` | `backchannel_words` | `stop` | `question` | `false_onset`) — as the detail of a new `VoiceEventKind::Pause`, and its half of `measure`.
- `keeper-core/src/agents/heard.rs` (new, pure): the `dev.keeper.agent.heard` content, its validation against the session, and the replay rule that truncates the `assistant` line at `heard_until` with the note `[The person stopped listening here.]`.
- `keeper-agent`: the owning host's handler — validate, append a `heard` line, nothing else.
- the shell: both ports execute `PauseSpeaking` (`pauseSpeakingAtBoundary(Immediate)`) and `ContinueSpeaking` (`continueSpeaking`), report each utterance's finish (an `AVSpeechSynthesizerDelegate`, replacing the `isSpeaking` poll for this purpose), and feed VAD onsets as `SpeechDetected` while speaking; `agents_ipc.rs` (91.4's) sends the `heard` event through `send_agent_event` (91.2's, `keeper-core/src/account.rs`) into the room of the `Agent` voice target the turn spoke to.
- `docs/ios.md` and the macOS voice chapter: *When you talk over an answer*.

**Acceptance:**
1. **Pause first.** `(Speaking, SpeechDetected(w))` → `Paused { heard: w }`, `[PauseSpeaking]` — `StopSpeaking` is not among the effects. The three tests that pinned stop-first (`voice_turn.rs:301`, `:1024`; `voice_platform.rs:318`) are rewritten to pin pause-first, not deleted; on the half-duplex absent port the microphone rule (`may_record`, `turn.rs:326-329`) is unchanged.
2. **The utterance decides.** From `Paused`: a backchannel (the word list for the voice locale's language, or under 600 ms of speech with the models present) → `Speaking`, `[ContinueSpeaking]`; the stop phrase → `Idle`, `[StopSpeaking, ReleaseMicrophone]`; anything else → `Heard{words}`, `[StopSpeaking]`, and it is sent as the next question; an onset that resolves to no words and under 250 ms → `Speaking`, `[ContinueSpeaking]`, logged `false_onset` (Q8). Test: `voice_paused_utterance_decides` (keeper-core), one case per branch, in English and Polish.
3. **"no" depends on the language.** `is_backchannel(["no"], 400, "pl")` is true, `("en")` false; `["mhm"]` is true in every language; a two-word utterance is never a backchannel by the list. Test: `backchannel_list_is_per_language` (pure).
4. **Without the models.** No VAD: the pause starts at the first non-empty partial while speaking (the ports' existing `SpeechDetected`), and the duration rule is off; the decision is the word list's. Test: `voice_backchannel_without_models_uses_the_list_only`.
5. **Offsets.** `Segmenter` fed `"One. Two three!\nFour"` in arbitrary chunks reports ends 4, 15 and, at flush, 20, in Unicode scalar values; a multi-byte answer (`"Zażółć. Gęślą."`) reports 7 and 14. Test: `segmenter_reports_sentence_ends` (pure).
6. **Where the person stopped.** On a stop (the stop phrase, or a non-backchannel utterance), the device sends `dev.keeper.agent.heard { anchor, heard_until, sentence, reason }` where `sentence` is the count of sentences the synthesiser finished and `heard_until` the end offset of the last of them (Q3), `0` when none finished. Test: `heard_until_counts_finished_sentences` (pure, over a sequence of finish callbacks and a stop mid-sentence).
7. **The host logs it, once, and only from the person.** The owning host accepts a `heard` event only from a device of the session's own person, for an anchor that is an answer in this room, with `heard_until` ≤ that answer's length; it appends one `heard` line (the event id in `matrix_event`, so a redelivery is not logged twice); anything else is ignored with a log line. Test: `heard_event_is_validated_and_logged_once` (keeper-agent, against the Synapse test homeserver: the person's device, a second user, a forged anchor, a redelivery). Risk: a real Matrix server's redelivery, not a mocked one.
8. **The next turn's context is cut.** Replay of a session whose last `assistant` line has a `heard` line truncates that message's text at `heard_until` and appends `[The person stopped listening here.]`; an answer with no `heard` line is replayed whole; the log line itself keeps the whole answer. Test: `replay_truncates_at_heard_until` (pure, keeper-core, over a fixture log).
9. **The room shows it.** 91.1's timeline admits `dev.keeper.agent.heard` — its test `agent_rooms_admit_status_and_scope_and_nothing_else` leaves `.heard` to "their stories", which is this one — and draws a mark in the answer at `heard_until` (UX-DR143). Tests: that admission test extended; (front, vitest) the mark's position for a fixture answer and `heard` event.
10. **The clock points are logged and measured** (F10). Every pause writes one `pause` record; `measure` gives the p95 of `pause_ms − onset_ms`, of `resume_ms − speech_end_ms` over `backchannel_duration` pauses and of `resume_ms − final_words_ms` over `backchannel_words` pauses, and the count of `false_onset` records per ten minutes of speaking. Test: `pause_timings_record_and_measure` (pure, a fixture set of 24 records per decision with hand-computed p95s).
11. **On the device — the gate for NFR-114's backchannel half** (a device run, F14). On hesperia and on the iPhone, with the turn models present: `measure` over the steps below gives `pause_ms − onset_ms` p95 ≤ 150 ms and the resume p95 ≤ 300 ms for each decision kind; the stops and the question behave as listed; the `false_onset` count is recorded, and more than one in ten minutes is reported to the coordinator before 97.3 closes. The figures, n, device, OS version, build and date are recorded in `docs/agents.md` § *Measured*; the story is done only when both devices' rows are there.

**Device run (the steps behind #11, hesperia and the iPhone, with the turn models present):**
- [ ] During long spoken answers, say "mhm" and "yeah" (in English) and "no" and "tak" (in Polish), at least twenty backchannels per device in all: each pauses and resumes, and the answer is never stopped.
- [ ] Say "stop" mid-answer: speech stops; the agent room shows the mark after the last finished sentence; the next question's answer refers to nothing past it.
- [ ] Say "no, wait" (English) mid-answer: speech stops and "no, wait" is sent as the next question.
- [ ] Ten minutes of an answer read aloud with nobody speaking: the voice log's `false_onset` records counted (Q8).
- [ ] `measure` run over each export; its figures written into `docs/agents.md` § *Measured*.

**Shell crate:** yes — both voice ports (pause, continue, the synthesiser delegate, `heard`) and `voice_log.rs`'s `pause` records. Gated on CI's macOS job and iOS compile check, `check:rust:macos`, and the device run (#11).

**binds:** FR-816, NFR-114, AD-411, UX-DR143

## UX decisions

- **UX-DR142 — the turn models' line in Settings › Voice.** One line under the voice switch: *Turn models ready* / *Turn models missing: `<file>` — keeper waits 1.8 s after you stop* / the refusal sentence. No control: the models are the organisation's (`_models/`), and the picks live in Settings › Transcription beside the other model picks.
- **UX-DR143 — where the person stopped listening.** In the agent room, an answer that was stopped shows a thin rule at `heard_until` with *You stopped listening here*; the text after it stays, dimmed. Nothing when the answer was heard to its end.

## What stays out

- **Voice on a server** (Kyutai, Moshi, a LiveKit SFU on electra) — DW-402 (architecture); P4's revisit trigger.
- **Hosted speech-to-speech** (OpenAI Realtime, GPT-Live, Gemini Live) — refused (D-36; the architecture's *What stays out*).
- **Android voice** — epic 98 (98.4), with the same turn models where its runtime allows.
- **A different recogniser.** Apple's `SFSpeechRecognizer` stays; Apple's `SpeechAnalyzer` (§8.6) is DW-406.

Deferred, with the ledger entries opened here; each is in full in `_bmad-output/implementation-artifacts/deferred-work.md`:
- DW-403 — the backchannel rule is a word list and a duration, not an acoustic model.
- DW-404 — `heard_until` is sentence-granular.
- DW-405 — ⌘9 conversations record no `heard_until`.
- DW-406 — Apple's `SpeechAnalyzer` is not adopted.

## The failure shape this epic must not repeat

**A voice that leaves the device.** A review that finds any of the following is a blocker:
- a model file fetched from anywhere but the account's config repository, or bundled;
- audio frames passed to anything but the turn-models worker;
- `voice_on_device.rs`'s file floor lowered, or a voice file excluded from it.

**An answer that talks over the person, or stops for "mhm".** A review that finds any of the following is a blocker:
- `StopSpeaking` before `PauseSpeaking` on a barge-in;
- a resume after an utterance the backchannel rule did not accept;
- `heard_until` counted past a sentence the synthesiser did not finish.

## Sprint-status entry

The coordinator applied this epic's entry under `development_status:` in `_bmad-output/implementation-artifacts/sprint-status.yaml`, above the epic-96 block; the 2026-10-02 review wave's changes are recorded in that entry.

## Stack rungs

On top of epic 96's last rung. Each compiles alone.
1. **`epic97-models`** — 97.1: `models.rs`'s two roles; the two settings keys; the `TurnModels` port; `hydrate_lfs_dir`'s role filter; the shell's `ort` runtime, fetch and view model (named in the PR as awaiting CI's macOS job / `check:rust:macos`); Settings › Voice's line; the docs.
2. **`epic97-turns`** — 97.2 and 97.3: `keeper-ported/src/smart_turn/` (with its first consumer, AD-396); `end_of_turn.rs`, `backchannel.rs`, `heard.rs`, `timings.rs` and the two voice-log record kinds; the turn table's `Finishing` and `Paused`; the segmenter's offsets; the rewritten pinning tests; `keeper-agent`'s `heard` handler; both voice ports; the agent room's mark. 97.2 alone would be a regression-free rung, but 97.3's table rewrite and 97.2's share `turn.rs`'s table and its tests, so they ride one rung. The device runs (97.2 #8, 97.3 #11) close their stories after the rung merges, by their rows in `docs/agents.md` § *Measured*.
