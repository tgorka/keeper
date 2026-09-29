# Epic 87 — Your meetings, transcribed on this Mac

created: '2026-09-28'
source: the owner's two messages of 2026-09-28, in Polish (verbatim below). Other inputs:
- one research lane, R1 (`/tmp/e87-R1FluidAudioRs.md`), on `fluidaudio-rs` and FluidAudio at pinned revisions;
- four grounding lanes, G1–G4 (recordings and drives; the config repository; house conventions and the zero-egress gates; the shell's port, IPC and job patterns);
- the coordinator's reading of NVIDIA's Nemotron 3 Diarization post and card, the Nemotron 3.5 ASR and Parakeet TDT v3 cards, NeMo Speech, NeMo-Speech.cpp, CodeSOTA's speech-to-text table and FluidAudio. These are synthesised, graded, in `research-transcription-2026-09-28.md`.

The coordinator froze the model as `local://epic87-contract.md` before the build wave. The decisions below are that contract's AD drafts, written out. Line numbers are at `dfa843e3` (origin/main, PR #410), the commit the build wave started from, unless a line says otherwise.
binds: FR-746…FR-758 and NFR-105…NFR-109 (allocated here); AD-339…AD-350; UX-DR121…UX-DR123; DW-331…DW-345 (DW-331…DW-342 allocated in *What stays out*; DW-343…DW-345 opened by the review wave, see *Review-wave amendments*); D-29 (drafted at the end, for `docs/decisions.md`).
- **The previous ceilings:**
  - FR-745, NFR-104, AD-338 and UX-DR120 (epic 86, its `binds:` line);
  - DW-330 (`deferred-work.md:6818`);
  - D-28 (`docs/decisions.md:1374`).
- **No earlier allocation.** A grep on 2026-09-28 found no allocation of any of these ids.
  - **What was searched:** `_bmad-output`, `docs`, `src`, `src-tauri/crates`, `tools`, `AGENTS.md`, `README.md` and `CLAUDE.md`, for `epic-87`, `epic87`, FR-746…FR-759, NFR-105…NFR-114, AD-339…AD-354, UX-DR121…UX-DR129, DW-331…DW-349 and `D-29`.
  - **What it found:** range statements in three earlier headers:
    - epic-76:5's `## D-19`…`## D-29`;
    - epic-85:5's NFR-99…NFR-109 and AD-326…AD-339;
    - epic-86:12's own grep ranges.
  - **And this epic's own in-flight code,** citing AD-342 and AD-348 (`keeper-core/src/vm.rs:4168`, `keeper-core/src/registry.rs:2868`, current worktree).
see-also:
- D-4 (the endpoint is yours; the sentence at `docs/decisions.md:155-157` that D-29 supersedes), D-5 (voice on the device, no bundled weights, no server fallback), D-21 (a search index is derived), D-28 (keeper runs no server);
- AD-27 (absent rather than disabled), AD-36 (the microphone's own track, Story 19.3), AD-40 (the crate split), AD-55 (no platform `cfg` in keeper-core), AD-166 (on-device recognition only), AD-298 (a drive role marked like the notes vault), AD-312 (the config repository is bytes in and out);
- `docs/egress.md` § *Screen recording adds no egress*, `docs/recording.md`, `docs/account.md` § *The config repository*.

## The owner's ask

Verbatim, as the contract records it (the `[...]` elision is the contract's):

> chcialbym dodac mozliwosc transkrypcji recordings w keeperze, (w opcji transrypcja automatyczna po spotkaniu), ale rowniez miec mozliwosc transkrypcji dowolnego pliku audio/video w opcjach. Chcialbym miec segmentowane transkrypcje oraz speaker id, oraz bank ids (embedings?) zeby rozpoznac ludzi na nastepnych meetingach oraz chce miec mozliwosc synchronizacji tego banku id osob (w drive). chcialbym miec mozliwosc poprawienia resultatow (wynik traslacji oraz match osob) - po poprawieniu moze byc update basy danych osob id. Chce zeby to bylo wewnatrz w rust code. [...] Chce miec eng, ewentualnie pl jezyk, wiekszosc bedzie video z wideokonferencji jak zoom, google meet. offline processing.

The second message, the same day:

> zaplanuj i zainplementuj calos (uzyj bmad). Uwagi
> - unikaj sidelcarale uzyj na mac (uzyj fluidaudio-rs)
> - wybierz folder dla przechowywania voices (zrob pdofolder lub prefix dla modelu, jezeli pozniej zmienie model) w opcjach - tak jak w przypadku recordings (tak samo zaznacz czy dany drive zachowuje voices) - wtedy tam mozna translacje zrobic - jezeli dany drive ma trascribing - recording ma mozlowosc automatyczny transcribing po nagraniu. W files wybierz ikone do transcribing folder - wybierz gdzie bedzie ten folder w strukturze tgdrive i neuradrive. Z default trasnkrypcja bedzie wlaczona jezeli mozliwa
> - jezeli jest mozliwe to uzyje sciezki mikrofonu
> - prywatnosc i rodo - nie trzeba teraz sie przejmowac
> - przechowuj tez slownik (nazwiska, zargon itp) jak voices
> - Obietnica „recording nic nie transkrybuje" - zmien obietnice - zmiana decyzji
> - Pobieranie modeli - umiesc model w config repo - zeby latwo zrobic dystrybucje - chyba ze musza byc skompilowane
> - Opcja NAS - nie rob opcji nas skoro mozna miec wszystko lokalnie
> - klipy głosowe w banku - tak trzymaj
> Zrob prs na stacku gh (gh stack) - zrob pr ready to merge.

In other words, twenty-eight asks. Each is answered in the table below.

## The verdict, ask by ask

| # | The ask (verbatim) | Verdict | How it is met | Mechanism |
| --- | --- | --- | --- | --- |
| 1 | "transkrypcji recordings w keeperze" | **met** | A finished recording session is transcribed into `transcript.json` and `transcript.md` in its own folder. | AD-344, AD-348; 87.1, 87.5 |
| 2 | "(w opcji transrypcja automatyczna po spotkaniu)" | **met** | *Transcribe after recording* (`transcription.after_recording`, user-global, default on) enqueues a job when the session ends. | AD-348; 87.5, 87.6 |
| 3 | "transkrypcji dowolnego pliku audio/video w opcjach" | **met** | *Transcribe* on a media file's row in Files, and *Transcribe a file…* in Settings › Transcription. The transcript is `<name.ext>.transcript.json` and `<name.ext>.transcript.md` beside the file, the extension kept (`meeting.mp4.transcript.json`; A10). | AD-344; 87.5, 87.6 |
| 4 | "segmentowane transkrypcje" | **met** | Utterances with start, end, speaker and word timings. An utterance breaks at a speaker change, a gap over 1.5 s, or 40 words. | AD-344; 87.1 |
| 5 | "speaker id" | **met** | The system or mixed audio is diarized into `S1`, `S2`…, and the microphone is `ME`. | AD-340, AD-345; 87.1, 87.4 |
| 6 | "bank ids (embedings?) zeby rozpoznac ludzi na nastepnych meetingach" | **met** | A voices bank of people, voice clips and 256-d embeddings. Each new speaker is matched to a person (`auto` at ≥ 0.70, `suggested` at ≥ 0.50, else `unknown`). | AD-343, AD-346; 87.2 |
| 7 | "synchronizacji tego banku id osob (w drive)" | **met** | The bank is files in a drive the person marks as keeping voices, one file per fact, so the drive's own sync merges devices. | AD-342, AD-343; 87.2 |
| 8 | "poprawienia resultatow (wynik traslacji oraz match osob)" | **met** | Edit an utterance's text, move an utterance to another speaker, merge speakers, rename, and assign a speaker to a person. "traslacji" is read as the transcription's result; translation is not built (*Open questions*, Q6). | AD-347; 87.1, 87.5, 87.6 |
| 9 | "po poprawieniu moze byc update basy danych osob id" | **met** | Confirming a speaker as a person stores that speaker's best clip and its embedding in the bank. Nothing else writes to the bank. | AD-347; 87.5 |
| 10 | "wewnatrz w rust code" | **met, with a stated limit** | The pipeline, the file formats, the bank, the dictionary and the corrections are keeper-core Rust. Inference runs in Apple's CoreML, through FluidAudio (Swift), behind a Rust crate in keeper's process. | AD-339; 87.1, 87.4 |
| 11 | "eng, ewentualnie pl jezyk" | **met** | Parakeet TDT 0.6B v3 covers English and Polish (FLEURS Polish WER 7.31). `transcription.language` is `auto`, `en` or `pl`. | AD-340; 87.1, 87.4 |
| 12 | "wiekszosc bedzie video z wideokonferencji jak zoom, google meet" | **met, proof owed** | A recording's system-audio track is diarized, and any other video file has all its audio tracks mixed. Decoding `.mov`/`.mp4` per track is the fork's job and is proven on hesperia. | AD-345; 87.4 |
| 13 | "offline processing" | **met** | Transcription contacts no network host. The only network step is fetching the models once from the config repository's host, which is already disclosed. | AD-341, AD-350; NFR-105 |
| 14 | "unikaj sidecar … uzyj fluidaudio-rs" | **met, through a fork** | In process, through a vendored fork of `fluidaudio-rs` at `tools/fluidaudio-rs/`. The published crate cannot return timings or embeddings, or load by path. | AD-339; 87.4 |
| 15 | "wybierz folder dla przechowywania voices (zrob pdofolder lub prefix dla modelu …)" | **met** | A drive keeps voices in a subfolder (default `voices`). Embeddings sit under `embeddings/<model-id>/`, so a model change re-embeds from the clips. | AD-342, AD-343; 87.2 |
| 16 | "w opcjach - tak jak w przypadku recordings (tak samo zaznacz czy dany drive zachowuje voices)" | **met** | The folder form's switch "This folder keeps voices" and its subfolder, and `[folder.voices]` in the folder's `.keeper/keeper.toml`, both exactly like recordings. | AD-342; 87.2, 87.6 |
| 17 | "jezeli dany drive ma trascribing - recording ma mozlowosc automatyczny transcribing po nagraniu" | **met** | The hook runs only when the finished session's destination drive keeps voices. | AD-348; 87.5 |
| 18 | "W files wybierz ikone do transcribing folder" | **met** | The voices folder carries `AudioLines` and the title "Where voices and the dictionary are kept". | UX-DR123; 87.6 |
| 19 | "wybierz gdzie bedzie ten folder w strukturze tgdrive i neuradrive" | **met, applied on hesperia** | `70-comms/voices` on both drives, beside neuradrive's `70-comms/meetings`. It is set on the owner's drives, not in code (owed). | AD-342 |
| 20 | "Z default trasnkrypcja bedzie wlaczona jezeli mozliwa" | **met** | `after_recording` defaults to on. It takes effect when the Mac can transcribe, the models are ready and the destination drive keeps voices. | AD-348, AD-349 |
| 21 | "jezeli jest mozliwe to uzyje sciezki mikrofonu" | **met** | The microphone track is transcribed on its own and attributed to the person marked as me. Echo of system speech is dropped, and the camera file is ignored. | AD-345; 87.1 |
| 22 | "prywatnosc i rodo - nie trzeba teraz sie przejmowac" | **deferred by the owner** | Recorded so it is findable. | DW-342 |
| 23 | "przechowuj tez slownik (nazwiska, zargon itp) jak voices" | **met** | A dictionary beside the voices, one file per term, applied after recognition, with suggestions offered from a person's edits. | AD-343, AD-347; 87.2 |
| 24 | "Obietnica „recording nic nie transkrybuje" - zmien obietnice - zmiana decyzji" | **met** | D-29 replaces the promise: recording adds no network destination, and it transcribes on this Mac. The gate and the docs change with it. | AD-350, D-29; 87.7 |
| 25 | "Pobieranie modeli - umiesc model w config repo … chyba ze musza byc skompilowane" | **met** | `_models/` in the config repository, over git LFS, hydrated and verified by keeper. The `.mlmodelc` files come precompiled, so nothing is compiled before distribution. | AD-341; 87.3 |
| 26 | "Opcja NAS - nie rob opcji nas" | **met by absence** | No NAS, no server and no cloud option. | D-29; research §3 |
| 27 | "klipy głosowe w banku - tak trzymaj" | **met** | Clips are kept, model-agnostic, as 16 kHz mono WAV of 2–15 s. | AD-343 |
| 28 | "Zrob prs na stacku gh (gh stack) - zrob pr ready to merge." | **process** | Four rungs (*Stack*, below). The coordinator's. | — |

## What the triage found

| Need | Verdict | Evidence |
| --- | --- | --- |
| Transcription, diarization, a speaker bank, a dictionary | **absent** | A grep for `transcri\|diariz\|speaker\|fluidaudio` over the sources hits only docs, tests, egress guards and the voice assistant's on-device recogniser (G1; G2 §3). |
| A place a finished session can queue work | **present** | `RecordingSink::finalize` (`keeper/src/ipc.rs:5911-5969`) runs, in order: the terminal manifest write, `archive.finalized`, `request_push(SessionEnd)` and `note_stub_at_finalize` (`:5965-5968`). |
| Knowing which audio track is the microphone | **present by order only** | keeper-rec adds system audio, then the microphone (`tools/keeper-rec/Sources/keeper-rec/Capture.swift:950-990`). It writes no track metadata, and the manifest records devices, not track positions (G1 §4). |
| A drive role: flag, folder file, form switch, Files glyph | **present for recordings, notes and tasks** | `SyncProfile.recordings` (`keeper-sync/src/profile/mod.rs:1154`); `[folder.recordings]` (`profile/folder.rs`); `SyncProfileVm`/`SyncProfileReq` (`keeper/src/sync_ipc.rs:199`, `:212`, `:769`, `:779`); `FilesFolderRoleVm` (`keeper-core/src/vm.rs:4159`); `FOLDER_ROLE_ICON` and `FOLDER_ROLE_TITLE` (`src/components/layout/files-pane.tsx:621-633`). |
| Large files through the config repository | **absent** | `keeper-sync/src/config_repo.rs` has no LFS code (0 hits) and writes whole in-memory blobs (`:148-158`, `:260-262`). keeper-sync's own LFS client (`keeper-sync/src/lfs/`) serves drives. |
| A platform port that decides nothing | **present** | Voice: the trait at `keeper-core/src/voice/mod.rs:238`; `voice_macos.rs` with one worker thread; `voice_ipc.rs:124-163` choosing a port per target, with `AbsentPort`. |
| A runtime capability probe | **present** | `recording: crate::macos_version::recording_supported()` (`ipc.rs:1473`), memoised (`macos_version.rs:63-75`). |
| A cancellable job streamed over a channel | **present** | `export_start`/`export_cancel` (`ipc.rs:2709-2878`). |
| A Rust binding that returns timings and embeddings and loads by path | **absent** | `fluidaudio-rs` 0.14.1 drops token timings and embeddings, and cannot load Parakeet or the offline diarizer from a directory (R1 §2). |
| The promise, and its gates | **present, contrary** | `docs/decisions.md:155-157`; `src/components/recording/zero-egress.test.ts:76-82` (`transc${"ri"}` in any case); `AGENTS.md:144`; `docs/egress.md:231-240` (research §2.5). |

## The one sentence

**keeper records meetings and can say nothing about what was said in them, and it promised in six places that it never would.** The owner wants the opposite: offline, in keeper's process, on the Mac, with the people in the room recognised from a bank that syncs like any drive.

**The fix:**
- **An engine.** A vendored fork of `fluidaudio-rs` runs Parakeet TDT v3 and pyannote community-1 on the Neural Engine, in process, behind a keeper-core port.
- **A pipeline.** Pure keeper-core Rust turns tokens and speaker segments into a transcript: utterances, speakers matched against the bank, the dictionary applied, echo dropped.
- **Files.** Transcripts sit beside the media. The bank and the dictionary are one file per fact in a drive marked as keeping voices. The models come from the account's config repository over LFS.
- **A promise.** D-29 replaces "recording transcribes nothing" with "recording adds no network destination; it transcribes on this Mac".

## What earlier decisions said, and what this epic amends

| The earlier decision | What it said | What this epic needs | The amendment |
| --- | --- | --- | --- |
| **D-4's bullet** *Why voice and the wake word are Epic 62* (`docs/decisions.md:155-157`) | "The recording feature's promise that it transcribes nothing is stated in six places and enforced by a source scan" | Recordings are transcribed. | **Superseded (AD-350, D-29).** The sentence gains a pointer to D-29. |
| **Story 20.4, FR-76** (`zero-egress.test.ts:76-82`) | `transcri` in any case, in any scanned recording file, fails the build | Recording code and copy say "transcribe". | **Amended (AD-350).** `transcri` leaves the gate. Every functional network token, and `Upload`, `Share` and `Cloud`, stay. |
| **`AGENTS.md:144`** | no bots palette verb the scan "would read as an upload or a transcription" | The scan no longer reads transcription. | **Reworded (AD-350).** The bots rule itself is unchanged. |
| **`docs/egress.md:231-240`**, and the comment at `bots-surface-stays-out-of-recording.test.ts:5-6` | "no upload, share-link, transcription, or cloud affordance" | Transcription is an affordance. | **Reworded (87.7).** "No network host" holds word for word. |
| **D-5** | keeper ships no weights; recognition is on the device; a server fallback is refused | Models for transcription. | **Held.** The bundle carries no weights; the organisation's config repository does (AD-341). On-device only, and no fallback, extends to transcription (NFR-105). |
| **AD-27** | a surface that cannot act is absent, not disabled | A Mac without the models. | **Held (AD-349).** No capability means no surface. Missing models are fixable, so the surface is present with a sentence and *Fetch models*. |
| **AD-40, AD-55** | keeper-core has no tauri, no sync engine and no platform `cfg` | A pure pipeline. | **Held (NFR-109).** `keeper_core::transcription` names no platform, tauri or keeper-sync. |
| **AD-312** (`config_repo.rs:1-10`) | the config repository is bytes in and out, small TOML | Distributing about 505 MB of models. | **Extended (AD-341).** `_models/` is LFS-tracked. The clone keeps what gix checks out, and hydration fetches the bytes into `<data_dir>/models/`. |
| **The recordings role family** (`RecordingsConfig`, `profile/mod.rs:531-552`), and AD-298 | a drive is marked as holding recordings, notes or tasks in its profile and folder file | A drive that keeps voices. | **Extended (AD-342).** `voices` joins the family, with the same validation and folder-file block. |
| **AD-36** (Story 19.3) | the microphone is its own unmixed track | The person's own speech, labelled for free. | **Relied on (AD-345).** DW-340 records that the track's position is the only label. |
| **D-21** | a search index is derived; the files are the model | Transcripts. | **Held (AD-344).** A transcript is a file. Indexing it is DW-338. |
| **`notes/recording_note.rs:15`** | the note stub carries "No transcription, no summarisation, no inference" | — | **Held.** The stub is unchanged, and the transcript is its own file. |

## Decisions this epic takes

The coordinator pinned these before the build wave (`local://epic87-contract.md`, *Pinned decisions*). The contract's own sections hold the API names; this text does not restate them.

- **AD-339: The engine runs in keeper's process on macOS, through a vendored fork of `fluidaudio-rs`.**

  **Binds:** FR-747, FR-749, FR-758; NFR-105, NFR-106, NFR-109; Story 87.4.

  **Decision:**
  - **The fork.** keeper vendors a fork of `FluidInference/fluidaudio-rs` at `tools/fluidaudio-rs/`. The original MIT `LICENSE` is kept, and a `NOTICE` names FluidInference. Its bridge is extended to return:
    - token timings (piece, start, end, confidence);
    - the diarizer's segments and one 256-d embedding per speaker;
    - a single-clip embedding;
    - the audio-track list, and per-track decoding to 16 kHz mono with a time range;
    - loading every model by path.

    FluidAudio is pinned at **0.17.4**.
  - **Linking.** The fork is a path dependency in the keeper shell's `[target.'cfg(target_os = "macos")'.dependencies]`. It lives outside `src-tauri/`, so it is never a workspace member: Linux never builds it, and workspace lints never apply to it. Its `unsafe` FFI stays inside it, and keeper's own code stays free of `unsafe`.
  - **The shell port.** `transcribe_macos.rs` (`#![cfg(target_os = "macos")]`, `MacSpeechEngine`) implements keeper-core's `SpeechEngine`.
    - One worker thread owns the FluidAudio handle, every call is a message to it, and ASR and diarization never run concurrently.
    - Every model loads by path, never through FluidAudio's download or prepare helpers.
    - A load failure is an `EngineError` that deletes nothing (research §7.5).
  - **No sidecar.**

  **Why not the obvious alternative:**
  - **`fluidaudio-rs = "0.14.1"` from crates.io** (research §6.2):
    - it discards token timings, so there are no words or utterances;
    - it discards embeddings, so there is no bank;
    - every init downloads from Hugging Face, which breaks D-29;
    - it carries the stale-decoder-state bug, and pins FluidAudio 0.14.8.
  - **A git dependency on upstream main** fixes only the decoder bug.
  - **A Swift sidecar** like keeper-rec: the owner refused it ("unikaj sidecar").
  - **NeMo-Speech.cpp's C SDK in process** runs on ggml rather than the Neural Engine, its diarizers give no embeddings, and the owner named `fluidaudio-rs` (research §6.4).

- **AD-340: Parakeet TDT 0.6B v3 recognises; pyannote community-1 diarizes and embeds.**

  **Binds:** FR-749, FR-751, FR-755; Stories 87.1, 87.4.

  **Decision:**
  - **ASR** is Parakeet TDT 0.6B v3, CoreML int8 (`FluidInference/parakeet-tdt-0.6b-v3-coreml`, CC-BY-4.0): English and Polish, with token timings.
  - **Diarization and speaker embeddings** come from pyannote community-1's offline pipeline (`FluidInference/speaker-diarization-coreml`, CC-BY-4.0 on those files). It has no speaker cap, and its 256-d embeddings come from the same run.
  - **`transcription.language`** (`auto | en | pl`, default `auto`, user-global) is passed to Parakeet as its token-language filter.
  - **Nemotron 3 Diarization is not used** in this epic (DW-331).

  **Why not the obvious alternative:**
  - **Whisper large-v3 or turbo,** the familiar choice, is worse on English (7.44 and 7.83 mean WER on the Open ASR table, against about 6.34 computed for v3), and would need its own CoreML path (research §4.3, §4.5).
  - **Nemotron 3.5 ASR** is 40 locales in one model, but Polish is only broad-coverage there (FLEURS 15.15 against Parakeet v3's 7.31; research §4.2).
  - **The table's leaders,** Granite Speech 4.1 2B (5.33) and Cohere Transcribe (5.42), are 2B speech models scored on English only. FluidAudio ports Cohere, but that port's Polish, size and speed were not established (research §4.3).
  - **Nemotron 3 Diarization** tops VoiceArena (14.72% DER), but it caps at eight speakers and emits no embedding. The bank would need a second model whose vectors are not comparable (research §5.1, §5.4).

- **AD-341: The models come from the account's config repository, over git LFS.**

  **Binds:** FR-756; NFR-105; Story 87.3.

  **Decision:**
  - **The repository.** The config repository carries `_models/` under `_models/** filter=lfs diff=lfs merge=lfs -text`, with `_models/models.toml` naming the set (`[asr] dir`, `[diarizer] dir`, `[embedding] id`).
  - **Hydration.** keeper-sync's own LFS client, with the config repository's auth, hydrates `_models/` into `<data_dir>/models/`:
    - after each successful config sync, and on *Fetch models*, one at a time, on a blocking thread of its own that holds neither keeper's blocking pool nor the account gate (H1);
    - each file's sha256 and size are verified, and it is written atomically;
    - a state file of path → oid skips files already fetched;
    - the LFS endpoint is derived from the repository's URL alone; a `.lfsconfig` in the clone is never read, so the credential reaches no other host (S1);
    - a completion marker, `.keeper-models-complete.json` (a digest over every file's oid), is removed before the first change and written last, only by a run that placed everything (S2).
  - **No compile step.** The `.mlmodelc` directories are precompiled.
  - **Loading.** The engine is never asked to load a set `missing()` finds incomplete, or one whose completion marker does not match what the clone names (S2). A changed marker reloads the engine at the next job's start, never mid-job (H10).
  - **No download from Hugging Face,** ever, from keeper.

  **Why not the obvious alternative:**
  - **Letting FluidAudio download from Hugging Face on first use,** as the crate does, adds a destination D-29 refuses. FluidAudio's retry also deletes and re-downloads (research §7.5).
  - **Bundling the models** adds about 505 MB to every release and every update, and D-5 says keeper ships no weights.
  - **Committing raw blobs without LFS** writes them as in-memory `Vec<u8>` through `config_repo.rs`, and keeps them in every device's history forever.
  - **A drive:** the models are not the person's files. The config repository is the channel the owner named ("umiesc model w config repo - zeby latwo zrobic dystrybucje").

- **AD-342: A drive keeps voices the way it keeps recordings.**

  **Binds:** FR-746; Stories 87.2, 87.6; UX-DR123.

  **Decision:**
  - **The field.** `SyncProfile.voices: Option<VoicesConfig { subfolder: String }>`, default subfolder `voices`, `#[serde(default)]`. `None` means "keeps no voices".
  - **The folder file.** It can be declared in the folder's `.keeper/keeper.toml` as `[folder.voices] subfolder = "…"`, read like `[folder.recordings]`.
  - **IPC.** `SyncProfileVm.voices` and `voicesSubfolder`; `SyncProfileReq.voices` and `voicesSubfolder`, where `None` means not expressed.
  - **The form.** The folder form gains the switch "This folder keeps voices".
  - **Validation** refuses, never corrects: an empty, absolute or `..` path, one that names no folder (`.`, `./`, `a/..`; S3), an overlap with the notes vault, or equality with the recordings root. `voices_root()` is the one definition of where a profile's voices live.
  - **The name.** A drive with voices is a **transcribing drive**.
  - **The owner's drives.** tgdrive and neuradrive take `70-comms/voices`.

  **Why not the obvious alternative:**
  - **A machine-local settings key naming a folder:** it would not travel, and `docs/recording.md:333-340` already rules that a subfolder belongs to the folder, not the machine.
  - **A fixed place in the config repository:** the bank is the person's data, grows with clips, and belongs with their drives' LFS.
  - **Inside the recordings folder:** a recordings root may be pointer-only on a device, and voices must exist on drives that hold no recordings.

- **AD-343: The bank is one file per fact, and a tombstone keeps a deleted person deleted.**

  **Binds:** FR-751, FR-753, FR-754; NFR-107; Story 87.2.

  **Decision.** Inside `<drive>/<voices subfolder>/`:
  - `people/<person-id>.json`: `{version:1, id, name, aliases:[], self:bool, createdAt, updatedAt}`, with a ULID id;
  - `clips/<person-id>/<clip-id>.wav`: 16 kHz mono PCM16, 2–15 s, with a ULID id. Clips are model-agnostic;
  - `clips/<person-id>/<clip-id>.json`: `{version:1, person, clip, source:{transcript, start, end}}`, beside a clip stored before any model embedded it, so its span is known without an embedding (A4);
  - `embeddings/<embedding-model-id>/<person-id>/<clip-id>.json`: `{version:1, model, person, clip, vector:[256 f32], source:{transcript, start, end}, addedAt}`. This is the model prefix: a model change re-embeds from the clips;
  - `dictionary/<term-id>.json`: `{version:1, id, text, aliases:[], createdAt}`;
  - `tombstones/<person-id>.json`: `{id, deletedAt, mergedInto?}`. A deletion removes the person's people, clips and embeddings files, and every read ignores a tombstoned person. A merge sets `mergedInto`, and every read counts what still arrives under the merged-away id as the survivor's (A7).

  Core plans every write and delete, and the shell executes them. A read skips unreadable files and names them; a file whose `version` is above 1 is skipped, and no planner rewrites it (A6). A span already in the bank, in any model, is never stored twice: under another person it moves (A4).

  **Why not the obvious alternative:**
  - **One `bank.json` or a SQLite file:** two devices that each add a person produce a `.sync-conflict-…` copy on every concurrent change (`keeper-sync/src/engine.rs:9383`), and a database does not merge at all.
  - **Deleting without a tombstone:** a device that has not seen the deletion pushes the person back.

- **AD-344: Transcripts are files beside the media, and the JSON is the record.**

  **Binds:** FR-747, FR-748, FR-749; NFR-108; Story 87.1.

  **Decision:**
  - **Where.** A recording session folder gets `transcript.json` and `transcript.md`. Any other media file gets `<name.ext>.transcript.json` and `<name.ext>.transcript.md` beside it, the extension kept (`meeting.mp4.transcript.json`), so two files that share a stem never share a transcript (A10).
  - **The record.** The JSON (`TRANSCRIPT_VERSION = 1`, camelCase, the same type the viewer receives over IPC) is the source of truth. The markdown is re-rendered from it on every save.

  **Why not the obvious alternative:**
  - **Transcripts in `archive.db`:** the archive is a cache ("Deleting `archive.db` loses nothing the manifests do not carry"; D-21), so it cannot hold the record.
  - **In the voices folder:** apart from the media they describe, they would break when a session is moved.
  - **Markdown only:** edits and speaker assignments need structure, and markdown is a view.

- **AD-345: In a recording, the microphone is its own speaker.**

  **Binds:** FR-750; Stories 87.1, 87.5.

  **Decision:**
  - **Track roles.** With both system audio and the microphone on, the first audio track is system audio and the second is the microphone (keeper-rec's writer order, G1 §4).
  - **The microphone** is transcribed on its own, as speaker `ME`, origin `microphone`, attributed to the bank's `self` person ("You" until one is marked).
  - **The system track** is transcribed and diarized.
  - **Echo.** A microphone utterance is dropped as echo only when it has at least 4 tokens, the one system utterance that covers it most overlaps at least 50% of its span, **and** the longest common subsequence of its tokens and the system tokens around it covers at least 60% of its tokens (A9). Word order is what tells an echo from a reply.
  - **Camera segments** are ignored, because they carry the same microphone audio.
  - **Any other file** has all its audio tracks mixed and fully diarized.

  **Why not the obvious alternative:**
  - **Diarizing a mix of both tracks** makes the person's own voice one more anonymous cluster, and echo doubles words. The microphone track is a certain label, for free.
  - **The camera file's microphone track** is byte-for-byte the same audio, so it would be counted twice.
  - **Telling the tracks apart by channel count** works only under echo cancellation.

- **AD-346: A speaker is matched against a person's centroid, and the thresholds are stated.**

  **Binds:** FR-749, FR-751; Stories 87.1, 87.2, 87.5.

  **Decision:**
  - **The comparison.** A speaker cluster's embedding is compared by cosine with each person's centroid: the normalised mean of that person's vectors for the current embedding model.
  - **The thresholds.**
    - `AUTO_MATCH = 0.70` assigns the speaker (`auto`);
    - `SUGGEST = 0.50` offers the person as a candidate (`suggested`);
    - anything lower is `unknown`.
  - **Across parts.** Clusters in different parts (segment files) are linked when their centroids' cosine is ≥ `LINK = 0.60`.
  - **The constants are uncalibrated** (DW-333).

  **Why not the obvious alternative:**
  - **Enrolling every unknown speaker as a new person** fills the bank with noise. The owner tied bank updates to correction.
  - **FluidAudio's own speaker database or known-speaker APIs** are per-run and in memory, not a synced bank. Some of them also use a different embedding model (research §5.2).

- **AD-347: Only a confirmation writes to the bank, and the dictionary works after recognition.**

  **Binds:** FR-752, FR-753; NFR-108; Stories 87.1, 87.2, 87.5, 87.6.

  **Decision:**
  - **What writes to the bank.** Assigning a speaker to a person, existing or new, stores that speaker's best clip (the longest non-overlapped span of the lines heard on the speaker's own track, clamped to 2–15 s; A2) and its embedding. While a job holds the engine the clip is stored alone and the next job embeds it (H6); when no clip can be had, no sample is stored and the person is still recorded (H5). Nothing else writes to the bank.
  - **Text edits** never touch the bank. They keep the ASR text, and a single-word substitution is offered as a dictionary suggestion that the person may accept.
  - **Dictionary terms** are applied after ASR as whole-word, case-insensitive (Unicode folding, A8) alias → text replacements, recorded in the transcript. A term with punctuation of its own (`C++`, `.NET`) matches the whole token (A1).
  - **CTC vocabulary boosting is not used** (DW-332).

  **Why not the obvious alternative:**
  - **Updating the bank on every correction:** a text edit says nothing about a voice.
  - **FluidAudio's CTC boosting:** its spotter is English, it needs a second encoder of about 100 MB, and issue #967 reports it changed 278 of 500 English transcripts, many wrongly (research §8.1).
  - **Learning the dictionary silently from edits** would rewrite later transcripts that nobody agreed to.

- **AD-348: A finished recording is transcribed after it ends, best effort.**

  **Binds:** FR-748; NFR-106; Story 87.5.

  **Decision:**
  - **The setting.** `transcription.after_recording` (bool, default **true**, user-global).
  - **The hook.** `RecordingSink::finalize` enqueues a transcription job after the note stub when all three hold:
    - the session's destination drive keeps voices;
    - the capability is true;
    - the models are ready.

    It is best effort and never blocks finalize.
  - **No automatic retry.** A session that could not be transcribed then (keeper quit, models missing) is not retried automatically (DW-334). *Transcribe* in Files does it.

  **Why not the obvious alternative:**
  - **Transcribing inside finalize:** finalize runs on the recording's driver task, and an hour's meeting takes minutes. A quit would hang.
  - **A launch-time scan for untranscribed sessions** competes with the index rebuild, and a job interrupted by every quit would re-run forever.
  - **A per-drive switch:** the owner asked for one option. The drive role already decides where.

- **AD-349: The capability is a runtime answer, and the models are a state.**

  **Binds:** FR-757; Stories 87.4, 87.5, 87.6.

  **Decision:**
  - **The capability.** `CapabilitiesVm.transcription: bool` is true on macOS on Apple Silicon at 15 or later. It is probed at runtime like `recording`. macOS 14 is excluded because the community-1 diarizer crashes there (FluidAudio #878).
  - **The models' state is separate,** from `transcription_status`: `ready`, `missing`, `fetching`, `failed` or `noAccount`, with a sentence. The surface can then say "models not fetched yet".

  **Why not the obvious alternative:**
  - **A compile-time `cfg` alone** would show the surface on Intel Macs, where Parakeet does not load, and on macOS 14, where the diarizer crashes 1,200 times in 1,200.
  - **Folding the models into the capability** would hide a surface whose fix is one button (AD-27 is about what cannot act, not what is not ready).

- **AD-350: The promise changes (D-29).**

  **Binds:** NFR-105; Story 87.7; D-29.

  **Decision:**
  - **What still holds:** recording adds no network destination.
  - **What is new:** it transcribes, on this Mac only.
  - **The gate.** The `transcri` affordance token leaves `zero-egress.test.ts`, and every network token stays. `AGENTS.md:144` is reworded.
  - **Models.** Model hydration reaches only the config repository's host, already in `docs/egress.md`.
  - **No NAS or server option, and no cloud fallback.**

  **Why not the obvious alternative:**
  - **Keeping the old promise and calling the feature something else** ("notes from audio") is exactly the evasion the gate exists to catch.
  - **An exemption list in the gate** would be two rules for one word.
  - **Deleting the gate** would lose its network-token protection too.

- **UX-DR121: The transcript viewer.**

  **Binds:** AD-344, AD-346, AD-347; FR-749, FR-752, FR-753, FR-758; Stories 87.5, 87.6.

  **Rule** (the design lane owns the details and conformance with DESIGN.md):
  - **Where it opens.** Opening `transcript.json` or `*.transcript.json` in Files opens the viewer through the viewer registry. So do *Open transcript* on a row, and the job strip's *Open transcript* once a job is done; the viewer never opens by itself (F3). A transcript the viewer cannot read opens in the text or JSON viewer (F7).
  - **The header:** the session title or file name, the date, the duration, the language, and the engine line.
  - **The speakers legend** has one row per speaker:
    - the name, or its label (`Speaker 1`…, `You`);
    - a status: *Matched* with its score, *Suggested* with its candidates, *Confirmed*, *Unknown* or *You*;
    - actions: *This is…* (candidates first, then every person, then *New person…*), *Rename*, and *Merge into…*.

    A refusal, such as no voices drive or a move across the microphone and the call (A2), shows Rust's sentence in place. A speaker left with no lines is hidden from the legend and still offered as a target (A3, F9).
  - **Utterances** show the time, the speaker and the text.
    - The text edits in place. An edited utterance is marked, and its ASR text stays reachable.
    - The speaker is changed from the utterance's own menu.
  - **Dictionary suggestions.** After an edit that returns suggestions, one inline row per suggestion: "Add *to* for *from* to the dictionary", with accept and dismiss.
  - **While a job for this path runs:** the phase, part `n` of `m`, and *Cancel*. A failure shows its sentence and *Try again*.
  - **`dev/mock-shell.ts`** covers every status, a running job, a failure and a refusal.

- **UX-DR122: Settings › Transcription, and the Recording toggle.**

  **Binds:** AD-341, AD-343, AD-348, AD-349; FR-747, FR-754, FR-755, FR-756, FR-757; Story 87.6.

  **Rule:**
  - **Where it exists.** The section exists only where `transcription` is true (AD-27).
  - **Models:** the state's sentence; *Fetch models* when `missing` or `failed`; the missing files, folded; `noAccount` pointing to Settings › Account.
  - **Language:** *Automatic*, *English* or *Polish*.
  - **Transcribe after recording:** a switch, with the sentence that it applies to sessions saved to a drive that keeps voices.
  - **One block per voices drive** (its name and subfolder):
    - **People:** the name, a *You* badge, the sample count, and a note when a person has no sample for the current model. Actions: *Rename*, *Mark as me*, *Merge into…*, and *Delete*, which is confirmed and says it removes their clips on every device.
    - **The dictionary:** terms with their aliases; add, edit and delete.
  - **No voices drive:** a sentence pointing to Settings › Sync.
  - **Transcribe a file…** opens a file picker, then shows progress inline and *Open transcript* when it is done.
  - **Settings › Recording** shows the same *Transcribe after recording* switch, only when `transcription` is true and the destination drive keeps voices.

- **UX-DR123: Files, and the folder form.**

  **Binds:** AD-342, AD-344; FR-746, FR-747; Story 87.6.

  **Rule:**
  - **The folder form:** the switch "This folder keeps voices" and a subfolder input (default `voices`), mirroring the recordings switch and its notes. Validation's sentences show in place.
  - **Files:**
    - `FOLDER_ROLE_ICON.voices = AudioLines`;
    - `FOLDER_ROLE_TITLE.voices = "Where voices and the dictionary are kept"`.
  - **Row actions:** *Transcribe* where `transcription` is true and the row is `transcribable` (a media file whose content is here, or a session folder whose audio segments all are; Q1, Q4, A14), and *Open transcript* wherever a transcript exists, whatever the capability (F6). Both come after the state verbs (Fetch, Open, Release, Pin) and before *Reveal* and *Copy path* (F1), in the context menu at every width, the house pattern (`files-pane.tsx:2961-2992`).

### Alternatives this plan rejected

- **A NAS or home-server engine** (NeMo Speech, or NeMo-Speech.cpp `serve`). The owner refused it, and it would be a server and a new destination (research §3).
- **A cloud transcription API.** It hands meeting audio to a third party, against "offline processing" and D-4/D-5's posture (research §3.1).
- **The published `fluidaudio-rs`.** It has no timings, no embeddings and no loading by path, and it has a decoder bug (research §6.2).
- **A Swift sidecar.** The owner refused it.
- **Whisper, Nemotron 3.5 ASR, or the English table's 2B leaders,** each for the reason in AD-340.
- **Nemotron 3 Diarization now:** an eight-speaker cap and no embeddings (DW-331).
- **CTC vocabulary boosting:** an English spotter, and measured regressions (DW-332).
- **Models from Hugging Face at first use, or in the bundle** (AD-341).
- **One bank file, or a database** (AD-343).
- **Transcripts in `archive.db`** (AD-344).
- **Diarizing the microphone mixed with system audio** (AD-345).
- **Enrolling unknown speakers automatically** (AD-346).
- **Updating the bank from text edits** (AD-347).
- **Transcribing inside finalize, or rescanning at launch** (AD-348).
- **Renaming the feature to keep the old promise** (AD-350).

## Verified facts, with sources

The evidence, graded, is `research-transcription-2026-09-28.md`. The facts the decisions rest on:

- **Parakeet TDT 0.6B v3** [SOURCE, NVIDIA card]:
  - 25 European languages including Polish, with the language detected automatically;
  - word- and segment-level timestamps; CC-BY-4.0;
  - FLEURS Polish WER 7.31, and MLS Polish 7.28 (research §4.1).
- **Nemotron 3.5 ASR** [SOURCE, NVIDIA card]: 40 locales under OpenMDW-1.1, with Polish in the broad-coverage tier. FLEURS Polish WER is 15.15 with the language given and 16.55 auto-detected, both at 1.12 s chunks (research §4.2).
- **The Open ASR table** [SOURCE, CodeSOTA; leaderboard figures accessed 2026-05-22]:
  - Granite Speech 4.1 2B 5.33 leads;
  - Parakeet TDT v2 is at 6.05, Whisper Large v3 at 7.44 and Whisper Large v3 Turbo at 7.83;
  - v3's own eight card figures average about 6.34 [INFERENCE] (research §4.3).
- **Nemotron 3 Diarization** [SOURCE, NVIDIA blog and card]:
  - 100M parameters, at most eight speakers, OpenMDW-1.1, released 2026-09-23;
  - #1 on VoiceArena's initial Diarization-Bench at 14.72% DER;
  - output is a `[T, 8]` probability tensor with anonymous channels and no embedding (research §5.1).
- **FluidAudio** [SOURCE, FluidInference]: Apache-2.0, on the Neural Engine. It names `fluidaudio-rs` as its Rust/Tauri wrapper, is at 0.17.4, and carries Nemotron 3 from 0.17.0 (research §6.1).
- **`fluidaudio-rs`** [SOURCE, R1] (research §6.2):
  - 0.14.1 on crates.io and 0.14.8 unpublished on main;
  - it drops timings and embeddings, cannot load by path, and does not build on Linux.
- **Two known crashes** [SOURCE, R1] (research §6.5):
  - concurrent ASR and diarization (#661);
  - the offline diarizer on macOS 14 (#878).
- **Model sizes** [SOURCE, R1 from the Hugging Face tree]: Parakeet v3 int8 is about 483 MB and community-1 about 22 MB. Both are precompiled `.mlmodelc` (research §7.1).
- **The fork's model loading.** The bridge loads ASR through `AsrModels.loadLocal(from:)`, builds the diarizer from `MLModel(contentsOf:)` and `initialize(models:)`, and refuses to diarize before that [REPO, `tools/fluidaudio-rs/swift/FluidAudioBridge.swift:128-134`, `:169-185`, `:204-208`, read while this was written]. That FluidAudio's own `loadLocal` never downloads is the Engine lane's report [UNVERIFIED] (research §7.5).

### Not established

- whether the fork links into the Tauri app;
- peak memory and first-load time;
- `.mov`/`.mp4` decoding per track;
- community-1's speaker cap as a documented guarantee;
- calibrated thresholds;
- whether electra's Forgejo serves LFS objects from its own host.

research §12 lists each, and where it gets settled.

## Requirements allocated here

| id | statement | story | AD |
| --- | --- | --- | --- |
| FR-746 | A drive can be marked in Settings › Sync as keeping voices, with a subfolder (default `voices`). The same declaration is `[folder.voices] subfolder = "…"` in the folder's `.keeper/keeper.toml`. A subfolder that is empty or names no folder (`.`, `a/..`), is absolute, escapes with `..`, overlaps the notes vault or equals the recordings root is refused with a sentence. Files marks the voices folder with its own glyph and title. | 87.2, 87.6 | AD-342, UX-DR123 |
| FR-747 | On a Mac that can transcribe, any audio or video file AVFoundation decodes (`mov mp4 m4a mp3 wav aac flac m4v caf aiff aif`) whose content is on this Mac can be transcribed from its Files row or from Settings › Transcription. Its transcript is written beside it as `<name.ext>.transcript.json` and `<name.ext>.transcript.md` (`meeting.mp4.transcript.json`), from all its audio tracks mixed and diarized. A file with a transcript offers *Open transcript*. | 87.1, 87.5, 87.6 | AD-344, AD-345, UX-DR122, UX-DR123 |
| FR-748 | When a recording session ends and its destination drive keeps voices, keeper transcribes it without being asked, while *Transcribe after recording* is on (the default) and the Mac can transcribe with its models ready. The transcript is `transcript.json` and `transcript.md` in the session folder. Ending the recording never waits for it. A session whose media are LFS pointers on this device is refused with a sentence. | 87.5, 87.6 | AD-348, AD-344 |
| FR-749 | A transcript is a list of utterances, each with a speaker, a start, an end, its text and its words' timings. An utterance breaks at a speaker change, a gap over 1.5 s, or 40 words. Speakers are `S1`, `S2`… from diarization and `ME` from the microphone, each with a match status. A session's parts form one timeline, with speakers linked across parts. | 87.1, 87.4 | AD-340, AD-344, AD-346 |
| FR-750 | In a recording with system audio and the microphone, the microphone track is transcribed on its own and attributed to the person marked as me (or "You"), and the system track is diarized. Microphone words that echo system speech are dropped, and the camera file is not transcribed. | 87.1, 87.5 | AD-345 |
| FR-751 | A transcribing drive holds a voices bank: people (name, aliases, whether it is me), voice clips of 2–15 s, and embeddings kept under a folder named for the embedding model. It is one file per fact, so the drive's sync merges devices without conflicts. A deleted person stays deleted on every device. A new transcript's speakers are matched to people: assigned at a cosine of 0.70 or more, suggested at 0.50 or more, and unknown otherwise. When the embedding model changes, the embeddings are derived again from the clips. | 87.2, 87.5 | AD-343, AD-346 |
| FR-752 | In the transcript viewer a person can edit an utterance's text (its recognised text is kept), move an utterance to another speaker, merge two speakers, rename a speaker, and say who a speaker is: an existing person or a new one. A line heard on the microphone never moves to a voice from the call, or back. Saying who a speaker is stores that speaker's best clip and its embedding in the bank (the clip alone while a job holds the engine, embedded by the next job), and nothing else writes to the bank. | 87.1, 87.5, 87.6 | AD-347, UX-DR121 |
| FR-753 | A transcribing drive holds a dictionary of names and jargon, each term a text with aliases and one file per term, beside the voices. After recognition, each alias is replaced whole-word and case-insensitively by its term's text, and the transcript records every replacement. An edit that substitutes single words offers them as dictionary suggestions the person may accept. | 87.2, 87.5, 87.6 | AD-343, AD-347, UX-DR121 |
| FR-754 | Settings › Transcription lists each transcribing drive's people, who can be renamed, marked as me (one at most), merged and deleted, and its dictionary, whose terms can be added, edited and deleted. | 87.2, 87.5, 87.6 | AD-343, UX-DR122 |
| FR-755 | `transcription.language` (`auto`, `en` or `pl`, default `auto`) and `transcription.after_recording` (default on) are user-global settings. They are generated into `docs/settings-keys.md` and travel with the person's settings. | 87.1, 87.6 | AD-340, AD-348 |
| FR-756 | The recognition and diarization models come from the account's config repository, under `_models/` (git LFS, named by `_models/models.toml`). keeper fetches them into its data directory after each config sync and on *Fetch models*, checks each file's sha256 and size, and skips files it already has. A set is used only when it is complete and is the one the config clone names. Settings › Transcription shows their state (ready, missing, fetching, failed, no account), its sentence, and the missing files. | 87.3, 87.6 | AD-341, UX-DR122 |
| FR-757 | Transcription surfaces exist only on a Mac with Apple Silicon and macOS 15 or later (`CapabilitiesVm.transcription`), and are absent everywhere else. Settings › Recording shows *Transcribe after recording* only there, and only when the destination drive keeps voices. | 87.4, 87.5, 87.6 | AD-349, UX-DR122 |
| FR-758 | A transcription is a job with phases (queued, decoding, transcribing, diarizing, matching, writing, done, failed, cancelled) and a part count, streamed to whoever started it and cancellable. Jobs run one at a time, and a cancelled or failed job writes no transcript. | 87.5, 87.6 | AD-339, AD-348, UX-DR121 |
| NFR-105 | **On this Mac, and nowhere else.** Transcription contacts no network host. Audio, transcripts, clips, embeddings and the dictionary leave the Mac only as files in the person's own drive, through that drive's sync. Model files come only from the account's config repository host, through the LFS endpoint derived from its URL (a `.lfsconfig` in it is never read), and the LFS object addresses that server returns; `docs/egress.md` already lists that host. keeper never asks Hugging Face for a model, and never asks the engine to load an incomplete or half-updated set. There is no NAS or server option, and no cloud fallback. | 87.3, 87.4, 87.7 | AD-339, AD-341, AD-350 |
| NFR-106 | **One engine, one call at a time.** One worker thread owns the FluidAudio handle. Calls are serialised, and recognition and diarization never overlap. Transcription jobs run one at a time. Finalizing a recording never waits on a job. | 87.4, 87.5 | AD-339, AD-348 |
| NFR-107 | **The bank survives sync.** Every bank fact is its own file, and a write creates or replaces exactly one file. Two devices adding people, samples or terms never produce a conflict copy. A tombstone outranks any stale copy of a person. A read tolerates unreadable files and names them. | 87.2 | AD-343 |
| NFR-108 | **A transcript keeps what was said and what was written.** `transcript.json` is versioned (`version: 1`) and is the only record. Every save writes it atomically and then re-renders the markdown. An edited utterance keeps its recognised text. | 87.1, 87.5 | AD-344, AD-347 |
| NFR-109 | **keeper's code stays safe and its core stays pure.** The FFI lives in the vendored `tools/fluidaudio-rs`, outside the workspace, and keeper's own crates carry no new `unsafe`. `keeper_core::transcription` has no platform `cfg`, no tauri and no keeper-sync (`check:core-tauri-free`, `check:core-sync-free`). The shell's macOS port decides nothing. | 87.1, 87.4 | AD-339 |

**Held, not restated:**
- NFR-11: `docs/egress.md` is the record of every destination. Transcription adds no row; the settings repository's row gains the model files in its *what for*.
- NFR-50: voice recognition is on-device, and `voice_on_device` is unchanged. No transcription file is named `voice*`, so that scan does not pick one up.
- FR-76: the recording zero-egress audit. It is amended, not dropped (AD-350).

## Open questions for the coordinator

Each has the reading this plan builds to, marked as such, so no lane is blocked. None is resolved silently. The coordinator ruled on all six in the review wave (*Review-wave amendments*, § *The coordinator's rulings on Q1–Q6*); the readings below are the plan's, kept as written.

- **Q1. *Transcribe* on a recording session in Files.**
  - **Gap.** AD-348 sends untranscribed sessions to "the Files action". The contract offers the action only on media files, and `FilesEntryVm` carries no "this folder is a session" flag. A click on `screen-0000.mov` inside a session would give `screen-0000.mov.transcript.json` from one segment, not the session's transcript.
  - **Plan's reading:** *Transcribe* is also offered on a folder that holds a `manifest.json`, and `transcription_start(<session folder>)` plans the whole session. How the row learns that a folder is a session is the shell's and the front's to agree.
- **Q2. Transcribing over an edited transcript.**
  - **Gap.** The contract names `existing_transcript` but no rule for it.
  - **Plan's reading:** a new job refuses, with a sentence, to replace a transcript that has an edited utterance or a confirmed speaker, and replaces one that has neither.
- **Q3. A source scan for NFR-105.**
  - **Gap.** Voice's promise is enforced by `voice_on_device`, which fails on any network token in the voice sources. The contract adds no such scan over `keeper-core/src/transcription/**`, `transcribe*.rs` or `tools/fluidaudio-rs/swift/`. Without one, NFR-105 is asserted, not enforced.
  - **Plan's reading:** build it, in 87.7, if the coordinator agrees. It is not in any story's acceptance until then.
- **Q4. One list of media extensions.**
  - **Gap.** `is_media_file` is Rust's list. The Files row decides which rows show *Transcribe*, and the viewer registry has its own media kinds.
  - **Plan's reading:** the row uses Rust's list, delivered with the listing or the status. A second list in TypeScript would drift.
- **Q5. Re-embedding on a model change.**
  - **Gap.** The contract provides `missing_embeddings(model)` but names no caller.
  - **Plan's reading:** before matching, the job embeds each bank clip that has no embedding for the current model and writes it (87.5).
- **Q6. "wynik traslacji".**
  - **Plan's reading:** the transcription's result (a typo, as in "translacje zrobic" for the transcribing drive). No translation is built. If the owner meant translation, it is a new ask.

## Review-wave amendments

Three reviews read the built wave, one each for the core, the shell and the front (`/tmp/e87-revcore.txt`, `/tmp/e87-revshell.txt`, `/tmp/e87-revfront.txt`). The coordinator froze their findings as amendments (`local://epic87-fixwave.md`), and every one is implemented. Where an amendment changed a decision, a requirement or an acceptance line in this document, that line now says what was built and names the amendment. This section records what changed and why.

Before the wave: the Linux gates were green; on hesperia `check:rust:macos` compiled the shell, clippy `-D warnings` was clean, and the keeper lib's 547 tests passed.

### Core (keeper-core)

- **A1. Dictionary terms with punctuation of their own.** A term or alias whose text carries punctuation (`C++`, `.NET`) matches the whole token, lowercased, after stripping only the edge punctuation the term itself lacks. Plain terms keep core matching. The word's own surrounding punctuation is kept, never doubled: `C++,` stays `C++,`, and `c++` becomes `C++`.
- **A2. A line keeps the track it was heard on.** `Utterance.origin: TrackOrigin` is `#[serde(default)]`; a file written without it takes its speaker's origin on load (`Transcript::from_json`).
  - `best_clip` uses only the lines heard on the speaker's own track.
  - Merging speakers, or reassigning a line, between a microphone speaker and a system or mixed one is refused with `CorrectionError::CrossOrigin`: "A line heard on your microphone cannot move to a voice from the call, or back."
  - An embedding never moves between speakers of different origin.
- **A3. A speaker with no lines is kept.** `settle` no longer drops zero-line speakers. A merge or a reassign leaves the emptied speaker in `speakers`, with its embedding, person and identity, so the move can be undone. The viewer hides it from the legend and still offers it as a reassign target (F9).
- **A4. The same span is never stored twice.** `Bank::add_sample` first looks for an existing sample with the same `SampleSource { transcript, start, end }`, start and end within 0.01 s.
  - **Deviation from the amendment as written:** the lane dedupes **across every embedding model**, and among clips still awaiting an embedding, not only within the same model. A span heard once is one clip, whichever model embedded it.
  - Under the same person the plan only adds this model's embedding when it is missing, and is empty otherwise.
  - Under another person the clip, its record and every model's embedding move to this person: written first, then the old files deleted. This is how a wrong confirmation is corrected.
  - `Bank::add_clip_only(person, clip_wav, source)` stores a clip without an embedding, for the next job's `missing_embeddings` to embed, with the same dedupe.
  - **Deviation:** such a clip carries its span in a model-free record beside the WAV, `clips/<person>/<clip>.json` (`{version, person, clip, source}`). Without it a clip-only sample could not be recognised as the same span before any model had embedded it.
- **A5. Re-embedding is deterministic.** `plan_embedding` takes the span from any other model's sample of the clip, or from its clip record. `addedAt` is the clip's earliest sample's in any model, else the time in the clip's ULID, as RFC 3339. Two devices re-embedding one clip therefore plan the same file, and the shell writes it only when the target file is absent.
- **A6. A bank file from a newer keeper is left alone.** A file whose `version` is above 1 is skipped on load with a warning, and a planner that would rewrite or delete it refuses with `BankError::NewerFile`.
- **A7. A merge is followed on every device.** The tombstone gains `mergedInto` (`#[serde(default)]`), which `merge_people` sets. `Bank::load` re-homes in memory the clips and embeddings still filed under a merged-away person onto the survivor, following chains of merges and stopping on a cycle, so centroids and sample counts include them.
- **A8. Case folding is Unicode** (`to_lowercase`) everywhere in `transcription/`, through one helper. No `eq_ignore_ascii_case` remains there.
- **A9. The echo rule.** A microphone utterance is dropped only when all three hold:
  - it has at least 4 tokens;
  - one system utterance, the one that covers it most, overlaps at least 50% of its span (**deviation:** the lane measures the single most-covering utterance, not the union of all system speech);
  - the longest common subsequence of its tokens and the system tokens within 0.5 s of it covers at least 60% of its tokens.

  This replaces the plan's "≥ 50% overlap and token Jaccard ≥ 0.5", which dropped a reply that shared the far end's words out of order. The reviewer's crosstalk case, "so is that it then yes okay" over "yes that is it", is kept.
- **A10. A file's transcript keeps the file's extension.** `transcript_paths_for(file)` gives `<name.ext>.transcript.json` and `.md`: `meeting.mp4` → `meeting.mp4.transcript.json`, so `call.mov` and `call.m4a` no longer share one transcript. Session folders keep `transcript.json`, and `existing_transcript` follows.
- **A11. Every correction marks the transcript.** `Transcript.corrected: bool` (`#[serde(default)]`) is set by every function in `corrections.rs`. `Transcript::is_corrected()` is `corrected`, or any edited utterance, or any confirmed speaker.
- **A12. Only what AVFoundation decodes.** `MEDIA_EXTENSIONS` drops `webm`, `mkv`, `ogg` and `opus`. The list is `mov mp4 m4a mp3 wav aac flac m4v caf aiff aif`.
- **A13.** `VoicesDriveVm` gains `localPath`, the drive's local path.
- **A14. *Transcribe* is offered only where the bytes are.** `FilesEntryVm.transcribable` is false for a file whose content is not here (an LFS pointer, virtual or materialising) and for a session folder any of whose audio segments is not here. **Deviation:** the listing cannot see a folder's segments from `FilesEntryFacts`, so the shell probes a session folder once and passes the answer as a `media_here` fact.

### Sync (keeper-sync)

- **S1. The models' LFS endpoint is the config repository's own.** `hydrate_lfs_dir` derives the endpoint from `remote_url` alone and never reads a `.lfsconfig`. The repository credential is therefore never sent to a host that someone with write access to the repository named. Drives still honour `.lfsconfig`; the rule is the config repository's only.
- **S2. A model set is ready, or it is not.**
  - After a fully successful hydration, `dest/.keeper-models-complete.json` is written last and atomically: `{ "digest": … }`, the sha256 over the sorted `path\toid\n` lines of every file under `_models/` (a pointer's oid, or a regular file's sha256).
  - It is removed before the first change to `dest`, and on any failure or cancel.
  - `hydration_is_current(clone_dir, rel_dir, dest)` recomputes the digest from the clone and compares.
  - Readiness is `missing()` empty **and** `hydration_is_current`, so a half-updated set (a new encoder beside an old decoder) is never loaded.
- **S3.** `VoicesConfig::validate` also refuses a subfolder with no normal component (`.`, `./`, `a/..`), which would name the profile root.

### Shell (the keeper crate and the fork)

- **H1.** Model hydration no longer runs through `on_blocking_pool`. It is `tokio::task::spawn_blocking` with `Handle::block_on`, stopped by its own interrupt, and it never holds `RUNTIME.blocking` or the account gate, so a first download of hundreds of megabytes does not hold every sync.
- **H2.** Every transcription, voices and dictionary command that touches the disk, `sync.db` or the registry is `async` and runs its body on the blocking pool.
- **H3.** One poison-tolerant `static TRANSCRIPT_WRITES: Mutex<()>` serialises every read-modify-write of a transcript (corrections, assign) and the job's final write. The job re-runs `refuse_overwrite` under it just before writing. `assign` re-reads the transcript under the lock and applies `assign_speaker` to the fresh copy.
- **H4.** The job's refusal to overwrite (Q2) uses `Transcript::is_corrected()`.
- **H5.** `assign` cuts and decodes first. When a clip or its embedding cannot be had, it stores no sample and logs why, and still creates the person and records it in the transcript. Creating the person and writing the sample run as one combined plan, after decoding.
- **H6.** When an embedding is needed while a transcription job holds the engine, `assign` stores the clip with `add_clip_only` (A4) rather than wait.
- **H7.** `write_atomic` stages to `.keeper.<ulid>.tmp`, a name keeper-sync excludes.
- **H8.** The account's drive record (`account_settings.rs`, `drive_record` and the restore path) carries `voices` like `recordings`, so a drive's voices role travels in `drives.toml` and comes back on restore.
- **H9.** The models' state is `ready` whenever readiness (S2) holds, even while the fetch each sync starts is checking it again, and the fetch flag is reset by a drop guard. A complete set that is not the one the clone names reads `missing`, with its own sentence: "The transcription models on this Mac are not the ones your account holds now; keeper brings them up to date after the next sync."
- **H10.** The worker's load key includes the completion marker's digest, and a changed digest reloads. Loads happen only at a job's start and on assign, never mid-job.
- **H11.** The Swift bridge's `emitJSON` checks `JSONSerialization.isValidJSONObject` and replaces non-finite numbers before serialising; anything else is a bridge error.
- **H12.** The shell fills A13's `localPath` and A14's facts, and A10's naming reaches the listing through `existing_transcript`.

### Front

- **F1.** *Transcribe* shows only when `entry.transcribable`. Both transcription verbs sit after the state verbs (Fetch, Open, Release, Pin) and before *Reveal* and *Copy path*.
- **F2.** When a job is done, the parent listing reloads: `load(profileId, parentSubpath)` on the desktop, a re-read on the phone.
- **F3.** The transcript viewer no longer opens by itself when a job ends. The job strip's *Open transcript* stays.
- **F4.** After assign, or a merge of people, the result reaches `transcriptionStore.people[profileId]`, and Settings' People list refreshes when the viewer closes.
- **F5.** Every *Cancel* in a form is `type="button"`.
- **F6.** *Open transcript* shows whenever `entry.transcript` is set, whatever `capabilities.transcription` says: reading a transcript needs no engine.
- **F7.** When `transcriptRead` fails, the viewer falls back to the text or JSON viewer.
- **F8.** The front matches a path to its bank by `VoicesDriveVm.localPath`, with a trailing-separator prefix check.
- **F9.** Speakers with no lines are hidden in the legend and offered as reassign targets (A3). A cross-origin refusal shows Rust's sentence (A2).
- **F10.** The fixtures and the mock shell carry `Utterance.origin`, `Transcript.corrected`, `VoicesDriveVm.localPath` and the `<name.ext>.transcript.json` naming.

### The coordinator's rulings on Q1–Q6

| Q | Ruling | Where it is built |
| --- | --- | --- |
| Q1 | **Yes.** *Transcribe* is offered on a folder holding `manifest.json`, and `transcription_start` on that folder plans the whole session. | The listing's session-folder probe; `FilesEntryVm.transcribable` (A14); F1 |
| Q2 | **Refuse**, with a sentence, to replace a corrected transcript; replace one nobody touched. "Corrected" is now `Transcript::is_corrected()`: any correction, not only an edit or a confirmation. | A11, H3, H4 |
| Q3 | **Yes.** `keeper-core/tests/transcription_on_device.rs` reads `keeper_core::transcription`, the shell's `transcribe*.rs` and the bridge's Swift and Rust, and fails on any network API token and on any FluidAudio download entry point in the bridge. | 87.7; `AGENTS.md:145`; `docs/egress.md` § *Transcription adds no egress* |
| Q4 | **One media list, in Rust.** `is_media_file` decides, and the row reads `FilesEntryVm.transcribable`; TypeScript keeps no list. | A12, A14, F1 |
| Q5 | **The job re-embeds** each bank clip that has no embedding for the current model, before matching. | 87.5; A4, A5 |
| Q6 | **No translation.** "traslacji" is the transcription's result. | *What stays out* |

### The macOS floor and the Swift runtime

- **`minimumSystemVersion` rises from 11.0 to 14.0** (`src-tauri/crates/keeper/tauri.conf.json`, `bundle.macOS`). 14 is FluidAudio's floor (`tools/fluidaudio-rs/Package.swift`, `.macOS(.v14)`), and its static Swift library is linked into the app, so below 14 keeper would not launch. Transcription itself still needs 15 (AD-349). An installed keeper on 11–13 that auto-updates would receive a bundle it cannot open: DW-345, reported to the owner for a decision.
- **The rpath link-arg.** The static Swift library imports `@rpath/libswift_Concurrency.dylib`, which since macOS 12 lives in the OS at `/usr/lib/swift`. Cargo does not forward a dependency's `rustc-link-arg` to the crate that links it, so both `tools/fluidaudio-rs/build.rs` and the keeper crate's `build.rs` emit `-Wl,-rpath,/usr/lib/swift` on macOS. Without it the app aborts at launch with "Library not loaded: @rpath/libswift_Concurrency.dylib".

### Deferred by the review wave

The coordinator wrote these into `deferred-work.md`, which is their source of truth:
- **DW-343. Each engine call copies the audio twice more on its way into FluidAudio.** The samples cross to the worker thread by `to_vec()`, and the bridge builds a Swift array from them. A 30-minute part is about 115 MB per copy, which the 1.46 GB peak measured on hesperia already includes; a three-hour file would peak near 2 GB. Revisit when a long file is transcribed on a small Mac: `Arc<[f32]>` in the trait.
- **DW-344. Model hydration reads the config clone outside the account gate.** A pointer torn by a concurrent hard reset would be copied as a plain file. The completion marker would then disagree with the clone, so readiness stays false until the next fetch repairs it: one failed load, never a wrong model. Revisit if a torn read is ever logged: snapshot the pointers under the gate.
- **DW-345. A Mac on macOS 11–13 that auto-updates receives a bundle it cannot open.** The updater feed has no OS gate. The owner's Macs run macOS 26–27. Revisit before a release reaches someone on 11–13: keep a last 11.0-floor build in the feed, or load the engine lazily so the floor can return to 11.0.

## Field report 2026-09-29

The owner used the build on hesperia (verbatim): "W wypowiedzi Kelly zmieniłem s1 i s2 jako Kelly ale nie wszędzie jest zmergowame np w md pliku. Nie widzę też opcji rozdzielenia wypowiedzi na więcej albo dodanie po. Nie widzę też opcji w opcjach głównych do translacji z dowolnego pliku albo z pliku w drivie. Gdzie są zapisywane informacje o osobach do identyfikacji. Przy ścieżce z mikrofonu czasami zdarza się że są 2 osoby a jeszcze zadziej więcej niż 2."

**Evidence.** `tgdrive/40-media/recordings/2026/2026-09-23 17.29 kelly-sync/transcript.json`: S1 and S2 both `confirmed`, one `personId` (Kelly Chang); every Kelly line on S1, none on S2; `transcript.md`'s legend still listed "Kelly Chang (S1)" and "Kelly Chang (S2)".

The coordinator froze seven amendments (`local://epic87-followup.md`), built as story 87.8:

| # | The finding | What was built |
| --- | --- | --- |
| B1 | Two speakers confirmed as one person stay two. | `corrections::assign_speaker`: when another speaker heard on the same track, with lines, already names that person, the assigned speaker's lines move to it (a merge) and it is the one confirmed. The microphone and the call stay two speakers. The bank sample is still cut from the assigned speaker's own clip, before the merge. |
| B2 | The markdown legend lists a lineless speaker. | `render::markdown`'s legend lists only speakers with at least one line, as the viewer does. |
| B3 | No way to split a line. | `corrections::split_utterance` and `transcript_split_utterance({path, utteranceId, wordIndex})`; the viewer's *Split…*. |
| B4 | No way to add a line after another. | `corrections::insert_utterance_after` and `transcript_insert_utterance({path, afterId, speakerId, text})`; the viewer's *Add a line after*. New ids are `u<highest + 1>`, never reused. |
| B5 | Two (rarely more) people share the microphone. | The job diarizes the microphone track too. In `assemble`, a diarized microphone part's voice that matches the bank's self person (cosine ≥ `SUGGEST`) is `ME`, else its longest talker; every other microphone voice is a numbered speaker with origin `microphone`, matched against the bank. `ME` carries its voice's embedding. Echo dedupe covers every microphone line. Confirming a microphone voice other than `ME` never marks anyone as me. |
| B6 | No main-menu way to transcribe any file. | One registry verb, *Transcribe a File…* (`transcription-transcribe-file`), in Recording: menu bar, ⌘K and ⌘?, gated by id on the shell's transcription probe (`TRANSCRIPTION_ACTION_IDS`, a fifth gate on `registry_sections` and a sixth on `PaletteIndex::query`). It opens Settings › Transcription's picker; the job's progress is a toast with *Open transcript*. |
| B7 | "Where is what identifies people kept?" | `docs/transcription.md` says it: `<drive>/<voices subfolder>/` — `people/`, `clips/`, `embeddings/<model>/`, `dictionary/`, `tombstones/`; plus split, add, microphone diarization, the same-person merge and the menu verb. |

## Stories

Every story names its rung in the four-rung stack (*Stack*, below).
- **The shell and the fork are by inspection.** Everything under `src-tauri/crates/keeper/**` and `tools/fluidaudio-rs/**` awaits CI's macOS job and the Mac gate on hesperia.
- **Generated bindings** (`src/lib/ipc/gen/*.ts`) are regenerated by the ts-export run and never hand-edited.
- **Every new core, sync and front behaviour test is mutation-proved:** mutate, run, restore, and read the diff to confirm the restore.

### 87.1 — The transcript format and the pure pipeline
**Intent:** "segmentowane transkrypcje oraz speaker id … Chce zeby to bylo wewnatrz w rust code". **Rung:** **epic87-core** (lane Core). AD-344, AD-345, AD-346, AD-347, AD-340's language setting.
**Files:**
- `keeper-core/src/transcription/mod.rs`, `engine.rs`, `model.rs`, `plan.rs`, `words.rs`, `assemble.rs`, `corrections.rs`, `render.rs` and `vm.rs` (new);
- `keeper-core/src/lib.rs` (`pub mod transcription;`);
- `keeper-core/src/config/keys.rs` (two `KeySpec`s), and `keeper-core/src/registry.rs` (two getter and setter pairs);
- `docs/settings-keys.md` (regenerated);
- the new `src/lib/ipc/gen/*.ts` files.

**Acceptance:**
- *The port* (`engine.rs`): `SpeechEngine`, `AudioTrackInfo`, `TrackSelect`, `AsrToken`, `AsrOutput`, `DiarSegment`, `DiarSpeaker`, `DiarOutput`, `TranscriptionLanguage` (serde `auto|en|pl`), `EngineUnavailable` with `sentence()` for each variant, and `EngineError`, as the contract names them. No platform noun appears outside `sentence()`.
- *Words* (mutation-proved): `words_from_tokens` starts a word at each `▁` piece and joins the following pieces into it. A word spans its first piece's start to its last piece's end.
- *Assembly* (mutation-proved):
  - a word goes to the speaker whose segment overlaps it most, else to the nearest segment;
  - an utterance breaks on a speaker change, on a gap of more than 1.5 s (exactly 1.5 s does not break), and after 40 words;
  - part offsets shift every time;
  - clusters in two parts link at a centroid cosine of 0.60 and stay apart just below it;
  - the microphone becomes `ME` with origin `microphone`;
  - a microphone utterance is dropped only at ≥ 4 tokens, ≥ 50% of its span overlapped by one system utterance, **and** a token LCS covering ≥ 60% of its tokens, and kept when any of them falls short, the reviewer's crosstalk case included (A9);
  - dictionary replacements are counted in `dictionaryApplied`;
  - matching gives `auto` at 0.70, `suggested` with candidates at 0.50, and `unknown` below.
- *`best_clip`* returns the longest non-overlapped span of the lines heard on the speaker's own track (A2), clamped to 2–15 s, and `None` when no span reaches 2 s.
- *The plan* (mutation-proved):
  - `plan_for_session` takes `screen-*` and `audio-*` segments in index order, ignores `camera-*`, and refuses pointer-only media with `PlanRefusal::MediaNotHere`;
  - `plan_for_file` mixes all tracks;
  - `assign_track_roles` gives system then microphone with both devices on, and the microphone alone or the system alone with one; an arbitrary file is `MixAll` and `Mixed`;
  - `is_media_file` accepts exactly FR-747's extensions, case-insensitively (A12);
  - `transcript_paths_for` gives the session and file names of AD-344.
- *Corrections* (mutation-proved):
  - an edit sets `edited`, keeps `asrText`, and re-splits words evenly over the old span when the word count changes;
  - reassign, merge (every utterance moves; the `from` speaker stays, with no lines, so the move can be undone; A3), assign (`confirmed`, with `personId` and `name`) and rename, each setting `corrected` (A11);
  - a reassign or a merge across the microphone and the call is `CorrectionError::CrossOrigin` (A2);
  - an unknown utterance or speaker id is a `CorrectionError`.
- *Rendering:* the title, date, duration, speaker legend, one `**[hh:mm:ss] Name:** text` line per utterance, and the engine footer. The same transcript renders byte-identically twice.
- *The file format:* camelCase, `version: 1`, and a JSON round trip that loses nothing. `edited`, `asrText`, `dictionaryApplied`, `corrected` and each utterance's `origin` survive, and a file without `origin` takes its speaker's on load (A2, A11).
- *Settings:* `transcription.after_recording` (default true) and `transcription.language` (default `auto`, other values refused) are user-global. `docs/settings-keys.md` is regenerated, and its drift test passes.
- *Purity:* `check:core-tauri-free` and `check:core-sync-free` pass. `keeper-core` gains no platform `cfg`.
- *Bindings:* `bindings:check` is green on this rung.

**binds:** FR-749, FR-750, FR-752, FR-755, NFR-108, NFR-109, AD-340, AD-344, AD-345, AD-346, AD-347

### 87.2 — The voices bank and the dictionary
**Intent:** "bank ids (embedings?) … synchronizacji tego banku id osob (w drive)"; "wybierz folder dla przechowywania voices (zrob pdofolder lub prefix dla modelu …)"; "przechowuj tez slownik … jak voices"; "klipy głosowe w banku - tak trzymaj". **Rung:**
- the bank, the dictionary and the voices role are on **epic87-core** (lanes Core and Sync);
- the shell's `SyncProfileVm`/`SyncProfileReq` fields and the listing's roles are on **epic87-surface**.

AD-342, AD-343, AD-346, AD-347.
**Files:**
- `keeper-core/src/transcription/bank.rs` and `dictionary.rs`;
- `keeper-core/src/vm.rs` (`FilesFolderRoleVm::Voices`, `FilesFolderRoles.voices_subfolder`, `role_of`);
- `keeper-sync/src/profile/mod.rs` and `profile/folder.rs` (`VoicesConfig`, `DEFAULT_VOICES_SUBFOLDER`, `SyncProfile.voices`, `validate`, `voices_root`);
- `keeper/src/sync_ipc.rs` (the VM and request fields, `parse_req`, `files_listing_vm`).

**Acceptance:**
- *Layout:* each path follows AD-343 exactly, with embeddings under `embeddings/<model>/<person>/<clip>.json`.
- *Loading* (mutation-proved): `Bank::load` skips a malformed or unreadable file, or one a newer keeper wrote (A6), and names it in its warnings. A deleted person, with their clips and embeddings, is invisible to every read; the clips and embeddings of a person merged into another count as the survivor's (A7).
- *Centroids and matching* (mutation-proved): the centroid is the normalised mean of one person's vectors for the given model only, and another model's vectors are ignored. `match_speaker` gives `auto` at 0.70, `suggested` at 0.50 (with candidates), and `unknown` below.
- *Planners* (mutation-proved):
  - `create_person` mints a ULID and one file;
  - `rename_person` rewrites one file;
  - `set_self` leaves at most one `self`, rewriting both files when it moves;
  - `delete_person` writes a tombstone and deletes the person's people, clips and embeddings files;
  - `merge_people` moves the clips and embeddings to `into` and tombstones `from` with `mergedInto` (A7);
  - `add_sample` writes one clip and one embedding under the model's folder for a span not yet in the bank; a span already there, in any model, is not stored again, and moves when it was another person's (A4);
  - `add_clip_only` does the same without an embedding, with the span in `clips/<person>/<clip>.json` (A4), and `plan_embedding` plans the same file on every device (A5);
  - `missing_embeddings` lists exactly the clips without an embedding for the model.
- *WAV:* `wav_bytes` and `wav_samples` round-trip 16 kHz mono PCM16, and a malformed header is an error.
- *Dictionary* (mutation-proved):
  - `apply` replaces whole words case-insensitively, multi-word aliases included, and never inside a word;
  - `suggestions` offers single-word substitutions only, and ignores case-only and punctuation-only changes;
  - the term planners write and delete one file each.
- *Profile* (mutation-proved):
  - `VoicesConfig` defaults to `voices`;
  - `[folder.voices]` is read in both key spellings, like recordings;
  - `validate` refuses empty, absolute, `..`, notes-vault overlap and recordings-root equality;
  - `voices_root()` joins the local path and the trimmed subfolder;
  - a stored profile without `voices` reads as `None`.
- *Roles:* `role_of` marks the voices folder by its configured path, case-insensitively, and never by its name.
- *Shell, by inspection:* `SyncProfileVm.voicesSubfolder` is always in force (stored or default). `SyncProfileReq`'s `None` leaves the stored value alone. `parse_req` mirrors recordings, and `files_listing_vm` passes the voices subfolder.

**binds:** FR-746, FR-751, FR-753, FR-754, NFR-107, AD-342, AD-343, AD-346, AD-347

### 87.3 — Models from the config repository
**Intent:** "Pobieranie modeli - umiesc model w config repo - zeby latwo zrobic dystrybucje - chyba ze musza byc skompilowane". **Rung:**
- `models.rs` is on **epic87-core** (lane Core), and the hydration is on **epic87-core** too (lane Sync);
- the shell's trigger and the status are on **epic87-surface**.

AD-341.
**Files:**
- `keeper-core/src/transcription/models.rs`;
- `keeper-sync/src/config_repo.rs` and `keeper-sync/src/config_repo/hydrate.rs` (`hydrate_lfs_dir`), with `keeper-sync/tests/config_repo_hydrate.rs`;
- `keeper/src/account_ipc.rs` (the trigger), and `keeper/src/transcribe_ipc.rs` (the models' state);
- `docs/account.md` (the layout gains `_models/`; see 87.7).

**Acceptance:**
- *The set* (mutation-proved):
  - `ModelSet::default()` is `parakeet-tdt-0.6b-v3`, `speaker-diarization` and `pyannote-community-1`;
  - `from_toml` reads `[asr] dir`, `[diarizer] dir` and `[embedding] id`, and refuses a malformed file;
  - `required_paths` names exactly the contract's files, each `coremldata.bin` included;
  - `missing` names each absent file relative to the root, and nothing when the set is complete.
- *Hydration* (mutation-proved, against a loopback fake LFS server):
  - a pointer is fetched through keeper-sync's batch and basic client from the endpoint derived from `remote_url` alone (`…/info/lfs`), with the repository's auth; a `.lfsconfig` naming another host is never asked (S1);
  - a wrong sha256 or size is refused and leaves no file;
  - a write is atomic;
  - a non-pointer file is copied;
  - a file the `.keeper-hydrate.json` state file records as holding its oid is skipped without rehashing;
  - `.keeper-models-complete.json` is written last, only after a full success, and removed on any failure or cancel; `hydration_is_current` is false once the clone names another set (S2);
  - a file in the destination that the source lacks is left alone;
  - an interrupt stops between files;
  - the report counts downloaded and skipped files and bytes.
- *No other host:* no Hugging Face address appears in keeper's code or in the fork's bridge (a grep, recorded in the review).
- *Shell, by inspection:* hydration runs after each successful config sync and on `transcription_models_fetch`, one at a time, on its own blocking thread, outside keeper's blocking pool and the account gate (H1). The status reports `fetching`, `ready`, `missing`, `failed` or `noAccount`, with a sentence. `ready` holds while a fetch re-checks a current set, and a complete but outdated set reads `missing` with its own sentence (H9).
- *On hesperia (owed):*
  - the owner's config repository gets `.gitattributes`, `_models/models.toml` and the two model directories;
  - after a sync, `<data_dir>/models/` holds the set and the status reads `ready`;
  - the LFS object addresses stay on electra's host.

**binds:** FR-756, NFR-105, AD-341

### 87.4 — The on-device engine
**Intent:** "unikaj sidecar … uzyj fluidaudio-rs"; "Chce zeby to bylo wewnatrz w rust code"; "offline processing". **Rung:** **epic87-engine** (lane Engine). AD-339, AD-340, AD-349.
**Files:**
- `tools/fluidaudio-rs/**` (new, the vendored fork);
- `keeper/src/transcribe_macos.rs` (new);
- `keeper/Cargo.toml` (the macOS table's path dependency);
- `src-tauri/Cargo.toml` (a workspace `exclude`, only if needed), and `src-tauri/deny.toml` (only if needed).

**Acceptance:**
- *The fork:*
  - it keeps the MIT `LICENSE` and adds a `NOTICE` naming FluidInference;
  - FluidAudio is pinned at 0.17.4;
  - the bridge returns token timings, per-speaker 256-d embeddings, a single-clip embedding, the track list, and per-track decoding to 16 kHz mono with a range, for video containers too.
- *Loading* (research §7.5): every model loads by path, never through FluidAudio's download or prepare helpers. Files are checked before loading. A load failure is an `EngineError` whose sentence names what failed, and it deletes nothing.
- *Threads:* one worker thread owns the handle, and every `SpeechEngine` call is a message to it. Recognition and diarization never overlap.
- *Availability:* `NeedsAppleSilicon` on Intel, `NeedsNewerMacos { minimum: "15" }` on 14, and `ModelsMissing { missing }` when `missing()` is not empty. `AbsentEngine` answers `Unsupported` elsewhere (87.5).
- *The build:* on Linux the workspace neither builds nor lints the fork; it is a macOS-only path dependency outside `src-tauri/`. `cargo deny check` passes. keeper's own crates gain no `unsafe`.
- *On hesperia (owed):*
  - the fork builds and links into the app (research §6.5);
  - an English two-speaker file and a Polish file transcribe with token timings;
  - diarization returns an embedding per speaker;
  - a recorded `.mov` decodes its system and microphone tracks apart;
  - first-load time and peak memory are recorded in the review.

**binds:** FR-747, FR-749, NFR-105, NFR-106, NFR-109, AD-339, AD-340, AD-349

### 87.5 — Transcription jobs, commands and the after-recording hook
**Intent:** "(w opcji transrypcja automatyczna po spotkaniu)"; "transkrypcji dowolnego pliku audio/video w opcjach"; "po poprawieniu moze byc update basy danych osob id". **Rung:** **epic87-surface** (the shell). AD-343, AD-346, AD-347, AD-348, AD-349.
**Files:**
- `keeper/src/transcribe_ipc.rs` (new: `platform_engine()` per target, `AbsentEngine`, the job registry and worker queue, the commands);
- `keeper/src/lib.rs` (the module, and the registration in the shared literal);
- `keeper/src/ipc.rs` (the hook in `RecordingSink::finalize`, after the note stub; `capabilities().transcription`);
- `keeper-core/src/vm.rs` (`CapabilitiesVm.transcription`) and its generated binding.

**Acceptance (the shell, by inspection):**
- **Registration.** Every contract command is registered on every target, in the shared `keeper_with_commands!` literal. `command-registration.test.ts` is green.
- **The job:**
  - `transcription_start(path, channel)` returns an id at once, and the job runs on the one transcription worker thread;
  - it refuses, with a sentence and a terminal `failed` batch, when the capability is false, when models are missing, or when the media are pointers;
  - it plans (a session for a folder with a manifest, a file otherwise), then loads the models and, per part, decodes, transcribes in the chosen language, and diarizes the system or mixed track;
  - it embeds each bank clip that has no embedding for the current model and writes it (Q5), assembles, matches, and writes the JSON atomically and then the markdown;
  - it streams phases with part `n` of `m`, and exactly one terminal batch;
  - `transcription_cancel` stops it between phases, and no transcript is written;
  - jobs run one at a time.
- **The voices drive for a path:**
  - the enabled profile with voices whose local path contains it;
  - else the first enabled voices drive;
  - with none, speakers stay `unknown`, and assignment is refused with a sentence.
- **Corrections:**
  - each `transcript_*` command reads the JSON, applies keeper-core's correction, and writes the JSON and the markdown;
  - `transcript_assign_speaker` with `newName` creates the person first;
  - confirming decodes the speaker's `best_clip` range, embeds it, and executes the person's creation and `add_sample`'s writes as one plan (H5). While a job holds the engine it stores the clip with `add_clip_only` (H6); when no clip can be had it stores no sample and still records the person (H5);
  - every read-modify-write of a transcript, and the job's final write, holds `TRANSCRIPT_WRITES`; the job re-checks Q2 with `is_corrected()` just before it writes (H3, H4);
  - the answer is a `CorrectionResultVm` with the dictionary suggestions.
- **The bank and the dictionary.** `voices_*` and `dictionary_*` execute core's planned writes and deletes inside the voices root, and nowhere else. The drive's own sync carries them.
- **The hook.** `RecordingSink::finalize` enqueues only when all four hold:
  - `transcription.after_recording` is on;
  - the session's destination profile keeps voices;
  - the capability is true;
  - the models are `ready`.

  It never blocks finalize, and a failure is logged.
- **The capability** is true only on Apple Silicon at macOS 15 or later, probed like `recording` and memoised.
- *On hesperia (owed):* a recorded Google Meet call with the microphone on is transcribed after it ends, with the owner's words as *You*. A speaker assigned in the viewer appears as files in `70-comms/voices` and reaches the other clone after a sync.

**binds:** FR-747, FR-748, FR-750, FR-751, FR-752, FR-753, FR-754, FR-757, FR-758, NFR-106, NFR-108, AD-343, AD-346, AD-347, AD-348, AD-349

### 87.6 — The surfaces
**Intent:** "W files wybierz ikone do transcribing folder"; "mozliwosc poprawienia resultatow (wynik traslacji oraz match osob)"; "w opcjach - tak jak w przypadku recordings". **Rung:** **epic87-surface** (the front). UX-DR121, UX-DR122, UX-DR123.
**Files:**
- the viewer and the settings section under `src/components/transcription/` (new). No transcription file goes under `src/components/recording/` or takes a `recording-` or `use-record` prefix, so the recording gate's globs never claim it;
- `src/components/settings/settings-dialog.tsx` (the section, gated on `transcription`);
- `src/components/settings/recording-settings-controls.tsx` (the toggle);
- `src/components/sync/add-folder-form.tsx` (the voices switch and subfolder);
- `src/components/layout/files-pane.tsx` (the glyph and title; *Transcribe* and *Open transcript*);
- `src/lib/viewers/registry.ts` (the viewer);
- `src/lib/ipc/client.ts` (the wrappers; the channel's `onmessage` is armed before `invoke`);
- `src/lib/stores/capabilities.ts` (`transcription: false` by default), the fixtures, and `dev/mock-shell.ts`.

**Acceptance (mutation-proved):**
- **Absence.** With `transcription` false, the section, *Transcribe* and the recording toggle are absent; *Open transcript* stays wherever a transcript exists (F6).
- **The folder form** shows the voices switch and subfolder, and sends `voices` and `voicesSubfolder`. A refusal shows its sentence.
- **Files:**
  - the voices folder shows `AudioLines` and its title;
  - *Transcribe* shows where the row is `transcribable`: media files whose content is here (Q4) and session folders whose audio is (Q1, A14), after the state verbs (F1);
  - *Open transcript* shows where one exists;
  - opening a transcript file opens the viewer.
- **The viewer:**
  - it renders each status with its score and candidates;
  - assigning to an existing person and to a new one calls `transcript_assign_speaker` with the right arguments;
  - rename, merge, edit and reassign each call their command and render the returned transcript;
  - suggestions render after an edit, and accepting one calls `dictionary_accept_suggestion`;
  - a running job shows its phase and part, and *Cancel* calls `transcription_cancel`;
  - an answer for another path is dropped.
- **Settings › Transcription:**
  - each models state renders its sentence, and *Fetch models* shows for `missing` and `failed`;
  - the language and after-recording controls write through `transcription_settings_set`;
  - people and dictionary actions call their commands;
  - no voices drive renders the pointer to Settings › Sync.
- **The recording toggle** shows only when the destination drive keeps voices.
- **Checks.** `bun run check:design` passes on the new files. Real-browser proof of the viewer and the section is owed (the `prove-a-keeper-frontend-change-in-a-real-browser` procedure).

**binds:** FR-746, FR-747, FR-748, FR-752, FR-753, FR-754, FR-755, FR-756, FR-757, FR-758, UX-DR121, UX-DR122, UX-DR123

### 87.7 — The promise changes
**Intent:** "Obietnica „recording nic nie transkrybuje" - zmien obietnice - zmiana decyzji"; "Opcja NAS - nie rob opcji nas". **Rung:**
- D-29 and this document are on **epic87-plan**;
- the gate edit and the docs ride **epic87-surface**, at or below the first scanned recording file that says "transcri", and with the feature they describe.

AD-350, NFR-105.
**Files:**
- `docs/decisions.md` (D-29 after D-28, and a pointer on D-4's sentence at `:155-157`);
- `src/components/recording/zero-egress.test.ts`;
- the comment at `src/test/bots-surface-stays-out-of-recording.test.ts:5-6`;
- `AGENTS.md:144`;
- `docs/egress.md` (§ *Screen recording adds no egress*; the organisation-account row's *what for*);
- `docs/recording.md` (`:3-5`, and a § *Transcription*);
- `docs/account.md` (the config repository layout gains `_models/`, with its `.gitattributes` rule, `models.toml` and the licence files that travel with the models);
- `docs/constraints-and-limitations.md` (the vendored fork's FFI, outside the workspace and outside the shell's `unsafe` inventory).

**Acceptance:**
- *The gate* (mutation-proved):
  - `transcri` is no longer forbidden;
  - every functional network token, and `Upload`, `Share` and `Cloud`, still are: a `fetch(` added to a scanned file still fails;
  - the non-vacuity floor holds;
  - the test's title and comment name what it forbids now.
- *`AGENTS.md:144`* keeps the bots rule, and drops "or a transcription".
- *The docs say it:* recording adds no network destination; transcription runs on this Mac; models come from the config repository's host; there is no NAS, no server and no cloud fallback.
- *`docs/egress.md`:* no row is added. `about-section.test.tsx`'s mirror of *On this Mac* stays green. The release workflow's per-tag egress diff shows the reworded section, as intended.
- *`decisions.md`:* D-4's sentence gains "(superseded by D-29)". The planning records that restated the old promise are not edited.
- *Unchanged:* `keeper_rec_sidecar_sources_are_network_free` and `voice_on_device`.

**binds:** NFR-105, AD-350

### 87.8 — The field report
**Intent:** the owner's field report of 2026-09-29 (*Field report 2026-09-29*). **Rung:** the core half on **epic87-core**, the shell and front halves on **epic87-surface**.

AD-345, AD-347, AD-27.
**Files:**
- `keeper-core/src/transcription/{corrections,render,assemble,mod}.rs`;
- `keeper-core/src/palette.rs`, `keeper-core/src/account.rs` (the transcription gate);
- `keeper/src/transcribe_ipc.rs` (the microphone diarized; the two commands; only `ME` marks me), `keeper/src/lib.rs` (registration), `keeper/src/ipc.rs` and `keeper/src/menu.rs` (the gate);
- the viewer, the command-palette handler, client wrappers and mock shell under `src/**` and `dev/**`;
- `docs/transcription.md`, `docs/recording.md`.

**Acceptance:**
- *B1* (mutation-proved): confirming a second speaker of one track as a person another speaker with lines already names leaves one speaker with every line, confirmed; the md legend has one row; a lineless namesake takes nothing; a microphone and a call speaker naming one person stay two.
- *B2:* a speaker without a line is absent from the markdown legend.
- *B3* (bounds mutation-proved): a split at word *k* keeps words `[..k]`, times and texts from the words, cuts an untouched line's `asrText` at *k* when the recogniser's words are the line's one for one, otherwise keeps it whole on the first half; refuses *k* = 0 and *k* ≥ the word count; the new id is one past the highest.
- *B4:* an added line is `edited`, has no `asrText` and no words, starts and ends at the previous line's end clamped to the next line's start, takes its speaker's origin; empty text and an unknown speaker or line are refused.
- *B5* (the `ME` choice mutation-proved): with no self person the longest talker on the microphone is `ME` and another voice is a numbered microphone speaker; with a self person its voice is `ME` even when it talks less, and the other voice is matched against the bank; a microphone voice whose every line was echo is no speaker; an undiarized microphone part is all `ME`.
- *B6:* the verb is in the Recording section and found by ⌘K where transcription runs, absent where it does not, and the recording verbs are unaffected.
- Every correction, split and insert included, marks the transcript `corrected`.

**binds:** AD-345, AD-347, NFR-108

## What stays out

- **A NAS, a home server or a cloud engine.** The owner refused it, and D-29 refuses it.
- **Translation.** Not asked for (Q6).
- **A choice of models in Settings.** `_models/models.toml` names the set, and that file is the operator's.
- **Consent, notices and GDPR (RODO) handling for voice prints.** Deferred by the owner (DW-342).
- **A transcript in the session's note stub.** The stub stays as `notes/recording_note.rs:15` describes it.

Deferred, with the ledger entries allocated here so a later planner finds them. The coordinator applies them to `_bmad-output/implementation-artifacts/deferred-work.md`, and that ledger is then the source of truth. The review wave opened DW-343…DW-345 in the ledger directly; *Review-wave amendments* summarises them.

```markdown
### DW-331: Nemotron 3 Diarization is not used, and keeper offers one diarizer.

origin: epic 87's plan, 2026-09-28 (AD-340)
location: `tools/fluidaudio-rs/` (the bridge binds only the community-1 offline pipeline), `src-tauri/crates/keeper-core/src/transcription/models.rs` (`ModelSet.diarizer_dir = "speaker-diarization"`)
reason: NVIDIA's Nemotron 3 Diarization (released 2026-09-23, OpenMDW-1.1) ranks first on VoiceArena's initial Diarization-Bench at 14.72% DER against 19.3% for the next system, and FluidAudio carries it from v0.17.0. keeper uses pyannote community-1 instead, for two reasons. Nemotron 3 handles at most eight speakers. It also emits only speaker-activity probabilities and no embedding, so the voices bank would need a second embedding model, and a vector from another model cannot be compared with the bank's (research-transcription-2026-09-28.md §5). There is no setting to choose a diarizer: with one engine that feeds the bank, a second choice would only be a way to break matching. Revisit when meetings of more than four people diarize poorly, or when the owner asks: run Nemotron 3 for the segments and community-1's embedding model on each speaker's longest clean spans, so the bank stays in one embedding space. Then measure both on the owner's own meetings, including Polish ones, which Nemotron 3's evaluation does not cover.
status: open

### DW-332: Dictionary terms do not bias recognition; FluidAudio's CTC vocabulary boosting is not used.

origin: epic 87's plan, 2026-09-28 (AD-347)
location: `src-tauri/crates/keeper-core/src/transcription/dictionary.rs` (`apply`, after recognition), `tools/fluidaudio-rs/` (no vocabulary binding)
reason: FluidAudio can bias Parakeet toward a term list by CTC word-spotting and rescoring. That needs a second encoder, `parakeet-ctc-110m` (about 100 MB), whose model is English (its card is tagged `en`). The feature's open upstream issue #967 reports that on 500 English dictations with 51 terms, 278 transcripts changed, many wrongly. keeper applies the dictionary after recognition instead: whole-word, case-insensitive alias → text, recorded in the transcript. A name the recogniser mangled beyond every alias is not recovered. Revisit when a multilingual spotter exists, or when the owner reports names the aliases cannot catch: bind `configureVocabularyBoosting` in the fork, and measure it on Polish before shipping it.
status: open

### DW-333: The speaker-matching thresholds are uncalibrated.

origin: epic 87's plan, 2026-09-28 (AD-346)
location: `src-tauri/crates/keeper-core/src/transcription/bank.rs` and `assemble.rs` (`AUTO_MATCH = 0.70`, `SUGGEST = 0.50`, `LINK = 0.60`)
reason: No source read for this epic calibrates cosine thresholds for pyannote community-1's 256-d embeddings on meeting audio, so the three constants are starting values, not measurements. A threshold set too low assigns the wrong person automatically. A threshold set too high leaves known people unknown, or splits one speaker in two across segment files. The first costs one click to correct, and nothing reaches the bank without a person's confirmation (AD-347). Revisit after the owner has confirmed people in about twenty meetings: compute the same-person and different-person cosine distributions from the bank's own confirmed samples, and set the constants from them.
status: open

### DW-334: A session that finished while keeper was quitting, or before the models arrived, is not transcribed later by itself.

origin: epic 87's plan, 2026-09-28 (AD-348)
location: `src-tauri/crates/keeper/src/ipc.rs` (`RecordingSink::finalize`, the enqueue), `src-tauri/crates/keeper/src/transcribe_ipc.rs` (the queue, held in memory)
reason: The after-recording hook enqueues a job only at finalize, and only when the capability and the models are ready. The queue lives in memory. A session is left untranscribed, with nothing to say so but a missing `transcript.json`, if keeper quits mid-job, if a session finishes before the models are fetched, or if a session is salvaged at boot. A launch-time scan for such sessions would walk every recordings root and compete with the index rebuild. A job interrupted by every quit would also re-run forever. *Transcribe* in Files transcribes such a session on request. Revisit if the owner finds untranscribed meetings: a marker written at enqueue and removed at a terminal phase would let a launch pass find exactly the interrupted ones.
status: open

### DW-335: Transcription runs only on Apple Silicon Macs with macOS 15 or later; the phone, Linux and Windows have no engine.

origin: epic 87's plan, 2026-09-28 (AD-339, AD-349)
location: `src-tauri/crates/keeper/src/transcribe_ipc.rs` (`platform_engine`, `AbsentEngine`), `src-tauri/crates/keeper/src/transcribe_macos.rs`
reason: The engine is FluidAudio through a Swift bridge. `fluidaudio-rs` does not build without Swift on macOS, Parakeet does not load on Intel Macs, and FluidAudio's offline diarizer crashes on macOS 14 (upstream issue #878, an OS bug). Everywhere else `CapabilitiesVm.transcription` is false and the surfaces are absent. The transcripts, the bank and the dictionary still sync to every device as files. Revisit when the owner asks for another platform. The phone could use FluidAudio's iOS support through the same fork. Linux and Windows could use NeMo-Speech.cpp's C SDK (Apache-2.0, ggml), which needs an embedding model beside its diarizers to feed the same bank.
status: open

### DW-336: Nothing is transcribed while a meeting is being recorded.

origin: epic 87's plan, 2026-09-28 (AD-348)
location: `src-tauri/crates/keeper/src/ipc.rs` (`RecordingSink::finalize`, the only trigger)
reason: Transcription starts after the session ends, from the finished segment files. A live transcript would need three things: the audio while it is captured, which keeper-rec only writes to files; FluidAudio's streaming recognisers, which the fork does not bind; and a streaming diarizer, where Sortformer caps at four speakers and Nemotron 3 at eight. It would also compete with the capture itself for the Neural Engine. Revisit when the owner asks for captions during a call.
status: open

### DW-337: A transcript is not summarised.

origin: epic 87's plan, 2026-09-28
location: `src-tauri/crates/keeper-core/src/transcription/render.rs` (the markdown carries the transcript only)
reason: The owner asked for transcripts, speakers and corrections, not summaries. A summary needs a language model, and the only model endpoints keeper talks to are the bot providers a person configured (D-4). Sending a meeting there is a flow of meeting content that nobody has chosen, even though it adds no host. Revisit when the owner asks for it: a *Summarise* action that sends a transcript's text to a provider the person picks, and says so.
status: open

### DW-338: Transcripts are not in the recordings search index.

origin: epic 87's plan, 2026-09-28 (AD-344)
location: `src-tauri/crates/keeper-core/src/archive/recordings.rs` (`rebuild_from_disk`), `src-tauri/crates/keeper-core/src/archive/recordings_fts.rs`
reason: The recordings browser searches `archive.db` and its FTS, which are rebuilt from what the manifests carry. Transcripts are files beside the media (AD-344), and nothing indexes their text yet, so a word said in a meeting cannot be found from the recordings browser. The index is derived and disposable (D-21), so adding transcripts later loses nothing. Revisit when the owner asks to search what was said: index each `transcript.json`'s utterance text and speaker names in `rebuild_from_disk`, and again when a transcript is saved.
status: open

### DW-339: keeper builds against a vendored fork of fluidaudio-rs that upstream does not carry.

origin: epic 87's plan, 2026-09-28 (AD-339)
location: `tools/fluidaudio-rs/` (the fork: `LICENSE` MIT kept, `NOTICE` naming FluidInference), `src-tauri/crates/keeper/Cargo.toml` (the macOS path dependency)
reason: The newest `fluidaudio-rs` on crates.io (0.14.1) discards token timings and speaker embeddings, cannot load models from a directory, and carries a stale-decoder-state bug. It also pins FluidAudio 0.14.8, while FluidAudio is at 0.17.4. keeper's fork extends the bridge and moves FluidAudio forward, so every later FluidAudio fix and model reaches keeper only when someone bumps the fork. Revisit when upstream `fluidaudio-rs` exposes token timings, per-speaker embeddings and loading by path at a current FluidAudio: move back to it, or offer the fork's changes upstream.
status: open

### DW-340: The microphone track is found by its position in the file, because keeper-rec writes no track labels.

origin: epic 87's plan, 2026-09-28 (AD-345)
location: `src-tauri/crates/keeper-core/src/transcription/plan.rs` (`assign_track_roles`), `tools/keeper-rec/Sources/keeper-rec/Capture.swift:950-990` (the writer's input order)
reason: keeper-rec adds system audio first and the microphone second, and writes no track metadata. The manifest records which devices were on, not where their tracks sit. "The second audio track is the microphone" holds only while the writer keeps that order. A change to the writer would silently attribute other people's speech to the person marked as me. Revisit if keeper-rec's writer changes: have it record track roles in `manifest.json`, and read them before falling back to order.
status: open

### DW-341: The model files' licences travel with the config repository, and one upstream provenance question is open.

origin: epic 87's plan, 2026-09-28 (AD-341)
location: the account's config repository, `_models/` (the operator's), `src-tauri/crates/keeper-core/src/transcription/render.rs` (the transcript's engine footer)
reason: Parakeet TDT v3's CoreML port is CC-BY-4.0. The community-1 files in `speaker-diarization-coreml` are CC-BY-4.0 under a scoped NOTICE that requires attribution to pyannote, WeSpeaker, BUT Speech@FIT and Fluid Inference. keeper ships none of these files. The organisation distributes them through its config repository, so their licence and NOTICE files must travel in `_models/` beside them. keeper names the models in each transcript's footer, and nowhere shows their licences. Upstream issue #927 asks for provenance and licensing details for commercial redistribution of those artifacts, and is open. Revisit when #927 is answered, or when keeper adds a model credit to Settings › About.
status: open

### DW-342: Voice prints are personal data, and consent and GDPR handling are deferred by the owner.

origin: epic 87's plan, 2026-09-28 (the owner: "prywatnosc i rodo - nie trzeba teraz sie przejmowac")
location: `src-tauri/crates/keeper-core/src/transcription/bank.rs` (people, clips, embeddings)
reason: The voices bank keeps other people's voice clips and speaker embeddings, and those identify them. Under the GDPR (RODO), biometric data processed to identify a person is a special category (Art. 9). keeper asks nobody's consent, shows no notice, and has no per-person export. Deleting a person removes their files and leaves a tombstone, which erases them on every device the drive reaches. That is the only control. The owner decided on 2026-09-28 not to address this now. Revisit before the bank holds anyone outside the owner's own meetings, or when the owner asks: a consent note per person, an export, and a retention rule.
status: open
```

## The failure shape this epic must not repeat

**A promise stretched.** The old promise was a sentence and a gate, and this epic removes one word from the gate. What replaces the word is NFR-105:
- no network call in the transcription path;
- no model from anywhere but the config repository's host;
- no audio or voice outside the person's drive.

A review that finds any of the following is a blocker:
- a Hugging Face address or download helper reachable from keeper;
- a transcription command that sends bytes anywhere;
- a model load that can purge and re-fetch.

**A bank that eats itself.** The bank is shared by every device that syncs the drive. A review that finds any of the following is a blocker:
- a write that rewrites a file two facts share;
- a deletion without a tombstone;
- a read that fails on one bad file;
- a correction that writes to the bank without a person's confirmation.

**An engine that takes keeper down.** FluidAudio crashes when recognition and diarization overlap, and its diarizer crashes on macOS 14. A review that finds any of the following is a blocker:
- a second thread touching the handle;
- two jobs at once;
- the capability true below macOS 15 or on Intel;
- finalize waiting on a job.

**A transcript that loses the person's work.** A review that finds any of the following is a blocker:
- a save that is not atomic;
- an edit that drops the recognised text;
- a job that silently replaces a transcript a person corrected (Q2).

## Sprint-status entry

The coordinator applies this under `development_status:`, above the epic-86 block, in `_bmad-output/implementation-artifacts/sprint-status.yaml`. The paste-ready copy is `local://epic87-ledger-blocks.md` (a). The applied entry later gained one comment line for the review wave.

```yaml
  # Epic 87: the owner's two messages of 2026-09-28 (Polish, verbatim in the epic) — transcribe recordings (optionally automatically after a meeting) and any audio/video file, into segmented transcripts with speaker ids; a bank of voice embeddings synced in a drive to recognise people in later meetings; correct words and speaker matches, and a confirmed match updates the bank; inside keeper's Rust; English and maybe Polish; mostly Zoom/Meet video; offline. Second message: no sidecar, fluidaudio-rs on the Mac; a voices folder chosen per drive like recordings, embeddings under a model prefix; a Files glyph for it and a place in tgdrive and neuradrive (70-comms/voices); on by default where possible; use the microphone track; privacy/RODO not now; a dictionary kept like voices; change the "recording transcribes nothing" promise; models from the config repo unless they must be compiled (they are precompiled .mlmodelc); no NAS option; keep voice clips; a gh stack, ready to merge.
  # Stack rungs: epic87-plan (the epic file, research-transcription-2026-09-28.md, this entry, DW-331…DW-342, D-29 in docs/decisions.md), then epic87-core (keeper-core transcription/: engine port, models, plan, words, assemble, bank, dictionary, corrections, render, vm; the two settings keys and settings-keys.md; FilesFolderRoleVm::Voices; keeper-sync's voices profile role, [folder.voices] and hydrate_lfs_dir; bindings; plus the one-line shell and front hunks the new fields force: the FilesFolderRoles literal in sync_ipc.rs and the FOLDER_ROLE_ICON/TITLE records), then epic87-engine (the vendored tools/fluidaudio-rs fork at FluidAudio 0.17.4 and keeper/src/transcribe_macos.rs, the macOS path dependency), then epic87-surface (transcribe_ipc.rs with AbsentEngine, jobs and commands; the finalize hook; CapabilitiesVm.transcription; account_ipc's model hydration; sync_ipc's voices fields; the viewer, Settings › Transcription, the Files actions, the folder-form switch and the Recording toggle; the zero-egress gate edit, AGENTS.md:144 and the docs). The shell crate and the fork are by inspection on Linux and await CI's macOS job and the hesperia gate.
  # Contract: local://epic87-contract.md (AD-339…AD-350 pinned by the coordinator). Open questions Q1–Q6 are in the epic (Files' Transcribe on a session folder; replacing an edited transcript; a source scan enforcing NFR-105; one media-extension list; the re-embed call site; "traslacji" read as transcription).
  # Owed on hesperia: the fork builds and links into the app; the owner's config repo gets _models/ (LFS, models.toml, licence files) and hydration reads ready; tgdrive and neuradrive get [folder.voices] subfolder = "70-comms/voices"; a recorded Meet call transcribes after recording with the owner as You; a Polish file transcribes; a confirmed speaker's clip and embedding reach the other clone; first-load time and peak memory recorded; LFS object addresses stay on electra. Front's real-browser proof of the viewer and Settings › Transcription is owed too.
  epic-87: in-progress
  87-1-the-transcript-format-and-the-pure-pipeline: in-progress
  87-2-the-voices-bank-and-the-dictionary: in-progress
  87-3-models-from-the-config-repository: in-progress
  87-4-the-on-device-engine: in-progress
  87-5-transcription-jobs-commands-and-the-after-recording-hook: in-progress
  87-6-the-surfaces: in-progress
  87-7-the-promise-changes: in-progress
  # DW-331…DW-342 are opened by this plan.

```

## docs/decisions.md entry

The coordinator applies this to `docs/decisions.md` after D-28, and adds the pointer "(superseded by D-29)" to D-4's sentence at `:155-157`. The paste-ready copy is `local://epic87-ledger-blocks.md` (c). The applied entry is the source of truth, and it has moved on since this draft: the macOS minimum's rise to 14.0, the models' endpoint and readiness (S1, S2), a file's transcript name (A10), and DW-343…DW-345.

```markdown
## D-29 — A recording is transcribed on this Mac, and still reaches no new destination

The owner asked keeper to transcribe meetings — a recording after it ends, by default, and any
audio or video file on request — into segmented transcripts with speakers; to recognise people
in later meetings from a voices bank kept in a drive; to let a person correct both the words and
the people; and to do it offline, inside keeper, on the Mac. Until then keeper promised the
opposite: the recording feature "transcribes nothing", a sentence restated in six places and
enforced by a source scan (D-4, *Why voice and the wake word are Epic 62*). On 2026-09-28 the
owner changed that promise ("Obietnica „recording nic nie transkrybuje" - zmien obietnice -
zmiana decyzji"). This entry records the promise that replaces it, so the old one is not
re-argued and the new one is not stretched.

keeper will transcribe on this Mac, and it will **not** send audio, a transcript or a voice
anywhere, run or call a transcription server, offer a NAS or cloud option, or fetch a model from
anyone but the person's own organisation.

- **What changes:** a finished recording whose drive keeps voices is transcribed after it ends,
  while *Transcribe after recording* is on (the default) and the Mac can do it; any audio or
  video file can be transcribed from Files or Settings. A transcript is `transcript.json` and
  `transcript.md` beside the media. Speakers are diarized and matched against a voices bank —
  people, voice clips, and embeddings kept per model — and names and jargon against a
  dictionary, both kept one file per fact in a drive the person marks as keeping voices. A
  person's confirmed correction adds a voice sample to the bank; nothing else writes to it.
  (AD-339…AD-350; FR-746…FR-758; NFR-105…NFR-109; Epic 87)
- **What stays true: recording adds no network destination.** The capture sidecar is untouched
  and its network scan stays. The recording frontend's scan keeps every network token and its
  `Upload`, `Share` and `Cloud` words, and loses only `transcri` (AD-350). Transcripts, clips,
  embeddings and terms are files in the drive the person chose, and leave the Mac only as that
  drive's sync already carries its files.
- **Where the models come from:** the account's config repository, under `_models/`, tracked by
  git LFS and fetched by keeper's own LFS client from the host `docs/egress.md` already lists
  for the settings repository. Each file is checked against its sha256 before use, and the
  engine loads only a complete set, by path. keeper makes no request to Hugging Face. D-5 holds:
  keeper's bundle carries no weights, and the organisation that runs the account chooses to
  distribute them. Without an account there are no models, and Settings says so. (AD-341)
- **Why on this Mac, and not a NAS or the cloud:** the owner asked for offline processing and
  refused a NAS option ("nie rob opcji nas skoro mozna miec wszystko lokalnie"). A NAS would make
  keeper depend on a server the person runs, and add a destination; a cloud API would hand
  meeting audio to a third party. keeper stays a client. (research-transcription-2026-09-28.md §3)
- **Why in keeper's process, and not a sidecar:** the owner asked for no sidecar and for
  `fluidaudio-rs`. The published crate cannot return word timings or speaker embeddings, or load
  models from a directory, so keeper vendors a fork at `tools/fluidaudio-rs/` (MIT, attribution
  kept) over FluidAudio 0.17.4. Its FFI stays in that dependency, and keeper's own code stays
  free of `unsafe`. It runs on Apple Silicon with macOS 15 or later, and nowhere else yet.
  (AD-339, AD-349; DW-335, DW-339)
- **What it supersedes:** the sentence in D-4 that the recording feature transcribes nothing
  (`docs/decisions.md:155-157`), and with it `docs/egress.md` § *Screen recording adds no
  egress*, `AGENTS.md:144`'s "or a transcription", the `transcri` token in
  `src/components/recording/zero-egress.test.ts`, and the comment in
  `src/test/bots-surface-stays-out-of-recording.test.ts`. The planning records that restated the
  old promise (the epic 16, 19 and 20 contexts, specs 20.3 and 20.4, epic 61) are records of
  their time and stay as written; this entry is their pointer. The note stub keeper writes for a
  session still carries no transcription (`notes/recording_note.rs`); the transcript is its own
  file.
- **What is deferred, not refused:** Nemotron 3 Diarization (DW-331), vocabulary boosting
  (DW-332), calibrated matching (DW-333), a later pass over sessions the hook missed (DW-334), an
  engine on the phone, Linux or Windows (DW-335), live transcription (DW-336), summaries
  (DW-337), transcripts in search (DW-338), and consent and GDPR handling for voice prints,
  which the owner deferred (DW-342).
- **Revisit triggers:** upstream `fluidaudio-rs` carrying token timings, embeddings and loading
  by path (DW-339); a Nemotron 3 path that keeps the bank in one embedding space (DW-331); a
  platform other than the Mac asking for transcription (DW-335). None of them reopens the second
  paragraph: a transcription server, a NAS option or a cloud fallback is not a revisit, it is a
  new row in `docs/egress.md` that this decision refuses to write.
- **Status / owner:** decided by the owner on 2026-09-28. Owner is the architect. Epic 87
  implements it: `keeper_core::transcription`, keeper-sync's voices role and model hydration, the
  vendored `tools/fluidaudio-rs`, and the shell's `transcribe_macos` and `transcribe_ipc`.
```

## Stack

Four rungs, by layer as in epics 80–86, with the engine on its own rung because only a Mac can build it:
1. **`epic87-plan`:**
   - this document and `research-transcription-2026-09-28.md`;
   - the ledgers: the sprint-status entry, `deferred-work.md` DW-331…DW-342, and D-29 in `docs/decisions.md` with the pointer on D-4's sentence.
2. **`epic87-core`:**
   - keeper-core's `transcription/` (every file), `lib.rs`, `config/keys.rs`, `registry.rs`, the regenerated `docs/settings-keys.md`, and `vm.rs`'s `FilesFolderRoleVm::Voices` and `FilesFolderRoles.voices_subfolder`;
   - keeper-sync's `profile/mod.rs` and `profile/folder.rs` (voices), and `config_repo.rs` with `config_repo/hydrate.rs` and its test;
   - the regenerated bindings.

   **Hunks from other layers that ride this rung,** because the new fields break existing code:
   - the `FilesFolderRoles { … }` literal in `keeper/src/sync_ipc.rs` (`files_listing_vm`) gains `voices_subfolder`;
   - any `SyncProfile` literal in `keeper/src` without `..` gains `voices`;
   - `FOLDER_ROLE_ICON` and `FOLDER_ROLE_TITLE` (`files-pane.tsx:621`, `:629`, both `Record<FilesFolderRoleVm, …>`) gain `voices`.

   Without them the rung does not compile on CI's macOS job, or does not typecheck. `bindings:check` must be green on this rung alone.
3. **`epic87-engine`:**
   - `tools/fluidaudio-rs/**` and `keeper/src/transcribe_macos.rs`;
   - the macOS table line in `keeper/Cargo.toml`, and the workspace `exclude` or `deny.toml` lines if needed.

   **Stack-time decision:** the coordinator decides where `mod transcribe_macos;` in `lib.rs` goes. It rides this rung only if the module compiles warning-free with no caller. Otherwise it rides `epic87-surface` with its first caller.
4. **`epic87-surface`:**
   - the shell: `transcribe_ipc.rs`, the registration, the finalize hook, `capabilities().transcription` with `CapabilitiesVm.transcription` and its binding, `account_ipc`'s hydration trigger, and `sync_ipc`'s voices fields;
   - the front: the viewer, Settings › Transcription, the Files actions, the folder-form switch, the Recording toggle, the client wrappers, `capabilities.ts`, fixtures and `dev/mock-shell.ts`;
   - 87.7's gate edit, `AGENTS.md:144`, and the docs (`egress.md`, `recording.md`, `account.md`, `constraints-and-limitations.md`).
