---
title: '22.7 — The Microphone Stops Recording the Speakers'
type: 'feature'
created: '2026-08-04'
status: 'done'
baseline_revision: 'd36e9e8a6fd472b9ab486acd84558da8f8b4e64f'
review_loop_iteration: 0
followup_review_recommended: false
context:
  - '{project-root}/docs/project-context.md'
  - '{project-root}/docs/recording.md'
warnings: ['oversized']
operator_actions:
  - 'Unlock hesperia (its screen is locked, and a locked session can neither grant nor exercise the microphone TCC the measurement needs).'
  - 'Open keeper, go to Recording, choose the Audio only source, turn System audio OFF and the Microphone ON, and leave Echo cancellation ON. Speakers, no headphones.'
  - 'Start the recording, then run `bash ~/keeper-aec-measure.sh cue` in a shell and follow its prompts: 10 s silent while it plays the far end, 10 s counting aloud with nothing playing, 10 s counting aloud WHILE it plays. Stop the recording when it says done.'
  - 'Run `bash ~/keeper-aec-measure.sh windows aec-on`.'
  - 'Turn Echo cancellation OFF and repeat the identical take, then `bash ~/keeper-aec-measure.sh windows aec-off`.'
  - 'Paste all six figures into the Auto Run Result. Window 1 (far end only) must be at least ~15 dB lower with AEC on — that is the echo going away. Windows 2 and 3 (your voice alone, then your voice over the far end) must stay within a few dB — a big drop there means the unit is attenuating rather than cancelling, i.e. a mute with a nicer name.'
  - 'If windows 2/3 DO drop materially: re-enable the unit AGC in VoiceProcessingMic.start (kAUVoiceIOProperty_VoiceProcessingEnableAGC = 1, which Apple leaves on and this story deliberately turned off), rebuild the sidecar, measure again, and choose between pumping room tone and a quiet voice with both sets of numbers written down. Do not ship the switch on by default until one of the two is acceptable.'
  - 'Run `bash ~/keeper-aec-measure.sh restore` and delete the ~/Movies/keeper/aec-* test sessions.'
---

<intent-contract>

## Intent

**Problem:** Recording on speakers puts audible reverb on the result: the two never-premixed
tracks (AD-36) each carry the far end — once clean from ScreenCaptureKit, once re-captured by
the microphone a few milliseconds late and coloured by the room. Headphones hide it; nothing in
the code fixes it.

**Approach:** Add acoustic echo cancellation on the mic feed via macOS's own
`kAudioUnitSubType_VoiceProcessingIO` ('vpio'), whose echo reference is the *output device's*
mix rather than this process's render — a third microphone producer behind the existing
`micEnabled` branch, feeding the single `appendMicSample` seam, controlled by a persisted
pre-recording switch that is **on by default**.

## Boundaries & Constraints

**Always:**
- The mic reaches the writers through `appendMicSample` (`Capture.swift:886`) and nothing else.
  `micPTSLowerBound`, the 250 ms silence fill, the camera mirror (22.4) and the rotation split
  stay untouched.
- Every mic sample's PTS comes from the same host clock the rest of the session uses:
  `CMClockMakeHostTimeFromSystemUnits(timestamp.mHostTime)`. No clock conversion.
- AEC on ⇒ the mic does **not** ride the SCStream: skip `config.captureMicrophone` /
  `microphoneCaptureDeviceID` (`Capture.swift:522-534`) and do not
  `addStreamOutput(self, type: .microphone, …)` (`Capture.swift:551`).
- The unit runs with AEC + noise suppression on and **AGC off**
  (`kAUVoiceIOProperty_VoiceProcessingEnableAGC = 0`).
- Every failure degrades to today's mic path with a `warning` event — never a failed, silent or
  half-written recording.
- New wire key `echoCancellation` is emitted **only inside the existing `micEnabled` block**, so
  a mic-off session's wire is byte-for-byte unchanged. Sidecar-side absent-default is `true`
  (the first inverted additive default; an older host must not silently disable it).
  `PROTOCOL_VERSION` stays 1.
- Rust recording core stays platform-free: no CoreAudio/AVFoundation token may enter
  `keeper-core/src/recording.rs` (firewall test at `recording.rs:4780`).
- Swift keeps `unsafe`-free business logic in Foundation-only helper files so they stay
  unit-testable; the AudioUnit C API lives in one file.

**Block If:**
- The Swift sidecar cannot be built at all on hesperia (`bun run rec:build` fails for reasons
  unrelated to this change).

**Never:**
- Premix the two tracks; enable AGC; enable other-audio ducking (it would attenuate the far end
  in the system-audio track being recorded at the same moment).
- Re-initialize the unit because the **output device** changed mid-session — emit a `warning`
  instead. (Re-initializing after a *mic loss* is required and is a different case: the silence
  fill and `micPTSLowerBound` already cover that gap.)
- Change anything about a mic-off session, or make the Screen Recording / Microphone TCC
  prompts fire twice.
- Bump the sidecar protocol version, add a second mic seam, or touch `matrix`/sync code.

## I/O & Edge-Case Matrix

| Scenario | Input / State | Expected Output / Behavior | Error Handling |
|----------|--------------|---------------------------|----------------|
| Fresh install | no `recording.echo_cancellation` row | getter returns `true`; switch reads on | No error expected |
| Garbage stored value | key = `"maybe"` | getter returns `true` (only `"0"` means off) | Normalize on read, like fps/codec |
| Mic on, AEC on | start | wire mic block carries `"echoCancellation": true`; VPIO producer runs; SCStream mic output not attached | No error expected |
| Mic on, AEC off | start | wire mic block carries `"echoCancellation": false`; today's path verbatim | No error expected |
| Mic off | start, switch either way | wire has no `micEnabled`, no `micDeviceId`, **no `echoCancellation`** | No error expected |
| Old host, new sidecar | `startRecording` params omit `echoCancellation`, `micEnabled: true` | sidecar treats it as `true` | No error expected |
| Aggregate input device | picked mic's transport type is aggregate | skip VPIO, run today's mic path | `warning` `echoCancellationUnavailable` |
| `AudioUnitInitialize` → `-66784` | `kAUVoiceIOErr_UnexpectedNumberOfInputChannels` | same fallback as above | `warning` `echoCancellationUnavailable` |
| Any other VPIO setup OSStatus failure | e.g. device set / format set fails | same fallback as above | `warning` `echoCancellationUnavailable` |
| Default output device changes mid-session | user swaps to AirPods | recording continues, unit untouched | `warning` `echoReferenceStale`, once per session |
| Mic hot-unplug with AEC on | 19.4 path fires | warn + silence-fill + fallback, and the fallback comes up **with AEC still applied** | if the AEC restart fails, fall back to the plain mic session with `echoCancellationUnavailable` |
| Settings write while a session is live | `echoCancellation` differs from stored | request rejected, **nothing** written | `IpcError` `Internal`, `retriable: false`, "echo cancellation cannot be changed while a recording is running" |
| Settings write while live, AEC unchanged | other fields differ | applied normally | No error expected |

</intent-contract>

## Code Map

- `tools/keeper-rec/Package.swift:30-40` -- `linkedFramework` list; needs AudioToolbox + CoreAudio.
- `tools/keeper-rec/Sources/keeper-rec/main.swift:398-473` -- inline NDJSON decode of `startRecording`; `micEnabled` at :433, `micDeviceId` at :434; `captureEngine.start(...)` at :466.
- `tools/keeper-rec/Sources/keeper-rec/Capture.swift:310-316` -- `CaptureEngine.start` signature (the de-facto start struct).
- `…/Capture.swift:467-488` -- `beginCapture`, `wantsMic` seeding, `activeMicDeviceId` snapshot, disconnect observer.
- `…/Capture.swift:515-558` -- `SCStreamConfiguration` audio fields + `.microphone` stream output / `startMicCaptureSession` split.
- `…/Capture.swift:726-735` -- `beginAudioOnlyMicSession` (Story 21.3, all OS versions).
- `…/Capture.swift:785-806` -- per-segment mic AAC input (48 kHz / **2 ch** / 192 kbps); `…:1244-1258` the camera-file twin.
- `…/Capture.swift:826-878` -- `startMicCaptureSession`, the existing out-of-SCStream precedent.
- `…/Capture.swift:886-938` -- `appendMicSample`, the one seam. `…:930` camera mirror.
- `…/Capture.swift:1048-1125` -- silence fill; `:1067` host-clock read; `:1089-1125` mono Int16/48 kHz `makeSilenceBuffer` (reuse its CMSampleBuffer construction).
- `…/Capture.swift:991-1041` -- `handleMicLost` / `attemptMicFallback` (19.4).
- `tools/keeper-rec/Sources/keeper-rec/MicHealth.swift:26-35` -- warning code/message constants pattern; Foundation-only, unit-tested.
- `src-tauri/crates/keeper-core/src/recording.rs:657-662` -- `MicSelection`; `:687-732` `SessionParams`; `:744-789` `start_recording_request` (mic block :771-776).
- `src-tauri/crates/keeper-core/src/registry.rs:1356-1391` -- `recording.scale_percent` key/default/normalize/getter/setter — the shape to copy; `:596-609` a `"1"`/`"0"` bool precedent; `:697-734` `config.json` import (no allow-list, so the new key works free).
- `src-tauri/crates/keeper-core/src/vm.rs:2915-2951` -- `RecordingSettingsVm`.
- `src-tauri/crates/keeper/src/ipc.rs:4589-4634` -- `recording_start` settings reads + `SessionParams` literal; `:5452-5503` `read_recording_settings` / `recording_settings_set`; `:4924` `live_snapshot` for the liveness gate.
- `src/lib/ipc/gen/RecordingSettingsVm.ts` -- ts-rs generated mirror (regenerate, never hand-edit).
- `src/components/recording/recording-audio-controls.tsx:49-98` (copy constants), `:102-116` (`active?: boolean` prop), `:210-248` (mic switch + picker).
- `src/lib/stores/recording-settings.ts:90-135` -- `ensureRecordingSettingsHydrated` / `applyRecordingSettings`.
- `src/components/recording/recording-advanced-controls.tsx:141-148` -- the canonical commit-one-field shape.
- `docs/recording.md:7-22` (What it records), `:65-100` (`config.json` keys), `:102` (Out of scope).

## Tasks & Acceptance

**Execution:**
- [ ] `tools/keeper-rec/Package.swift` -- add `AudioToolbox` and `CoreAudio` to the executable target's `linkedFramework` list -- VPIO and the default-output-device query need them.
- [ ] `tools/keeper-rec/Sources/keeper-rec/EchoCancellation.swift` (new) -- Foundation-only: `warningCode = "echoCancellationUnavailable"`, `staleReferenceCode = "echoReferenceStale"`, their messages, and a pure `EchoCancellation.decide(requested:isAggregateInput:initStatus:)` returning use/fallback + optional warning -- keeps the decisions unit-testable without hardware, mirroring `MicHealth`.
- [ ] `tools/keeper-rec/Sources/keeper-rec/VoiceProcessingMic.swift` (new) -- the 'vpio' producer: instantiate, bind bus 1 to the picked/default input and bus 0 to the default output device, enable IO on both, AGC off, mono 48 kHz Int16 output format on bus 1, a silence-emitting render callback on bus 0, an input callback that renders bus 1 and hands a `CMSampleBuffer` (host-clock PTS) to a `mediaQueue` sink; plus start/stop and a `kAudioHardwarePropertyDefaultOutputDevice` listener that fires the stale-reference warning once. All `OSStatus` mapped to thrown errors -- one file owns the C API.
- [ ] `tools/keeper-rec/Sources/keeper-rec/Capture.swift` -- thread `echoCancellation` from `start` through `beginCapture` and `beginAudioOnlyMicSession`; when active, skip the SCStream mic config/output and run `VoiceProcessingMic` into `appendMicSample`; add session-scoped `micTrackChannels` (1 when AEC was active at capture start, else 2) used by both writers; make `attemptMicFallback` restart the AEC producer first and only then the plain session; stop/teardown the unit and its listener in `stop()` -- one producer added behind the existing branch, every other mic invariant untouched.
- [ ] `tools/keeper-rec/Sources/keeper-rec/main.swift` -- decode `echoCancellation` beside `micEnabled` with `?? true` and pass it to `captureEngine.start` -- absent must mean on.
- [ ] `tools/keeper-rec/Tests/keeper-recTests/EchoCancellationTests.swift` (new) -- cover the decision table (off requested, aggregate input, `-66784`, other OSStatus, happy path) -- hardware-free coverage of every fallback branch.
- [ ] `src-tauri/crates/keeper-core/src/registry.rs` -- `RECORDING_ECHO_CANCELLATION_KEY = "recording.echo_cancellation"`, default on, `get_/set_recording_echo_cancellation` (`"0"` ⇒ off, anything else ⇒ on) + a `_defaults_on_and_round_trips` test -- read-side normalization matches every other recording setting.
- [ ] `src-tauri/crates/keeper-core/src/vm.rs` -- add `echo_cancellation: bool` to `RecordingSettingsVm` with a doc comment in the neighbours' style.
- [ ] `src-tauri/crates/keeper-core/src/recording.rs` -- add `echo_cancellation: bool` to `MicSelection`, emit `echoCancellation` inside the existing `if let Some(mic)` wire block, update every `SessionParams`/`MicSelection` literal, and add `start_recording_request_*` tests for on/off/mic-off -- the key must never appear on a mic-off wire.
- [ ] `src-tauri/crates/keeper/src/ipc.rs` -- read/write the new field in `read_recording_settings` / `recording_settings_set`, reject a *changed* `echo_cancellation` while a session is live **before any write**, and populate `MicSelection.echo_cancellation` from the registry in `recording_start`; tests for the guard and for start-time population.
- [ ] `src/components/recording/recording-audio-controls.tsx` -- an "Echo cancellation" `Switch` under the mic device picker, bound to the settings store (hydrate on mount, commit via `applyRecordingSettings`), disabled when unhydrated, when the mic is off, or when the card is not `active`; exported label/caption/testid constants; sentence-case copy naming the mono + noise-suppression cost and "Applies to the next Recording Session."
- [ ] `src/components/recording/recording-audio-controls.test.tsx` -- assert: on by default, persists through `recordingSettingsSet`, disabled while not `active`, disabled while the mic is off, and that a rejected write reverts.
- [ ] `src/lib/ipc/gen/RecordingSettingsVm.ts` + every TS test literal building a `RecordingSettingsVm` (`recording-settings-controls.test.tsx`, `recording-destination-controls.test.tsx`, `recording-advanced-controls.test.tsx`) -- regenerate / add the field so typecheck passes.
- [ ] `docs/recording.md` -- a new "Audio processing" section: what the switch does, and the honest costs (mono mic signal, non-defeatable voice-band noise suppression, device native rate, no aggregate input devices, no mid-session output-device follow); add `recording.echo_cancellation` to the known-keys list and the JSON example.

**Acceptance Criteria:**
- Given a fresh install, when the Recording view loads, then the Echo cancellation switch reads on; and given it is toggled off and the app restarts, when the view loads, then it reads off.
- Given a live Recording Session, when the user attempts to change echo cancellation, then the control is disabled and a direct `recording_settings_set` with a different value is rejected without writing anything.
- Given a session started with the mic on and echo cancellation on, when the sidecar starts capture, then no `.microphone` stream output is attached and mic samples still arrive through `appendMicSample` with monotonic host-clock PTS.
- Given echo cancellation is on and the mic is hot-unplugged mid-session, when the 19.4 fallback runs, then a `warning` is emitted, the gap is silence-filled, and the recovered mic feed is still echo-cancelled.
- Given echo cancellation is on and the default output device changes mid-session, when the change is observed, then exactly one `echoReferenceStale` warning is emitted and no unit is re-initialized.
- Given the webcam and mic are both on with echo cancellation on, when the session ends, then `camera-####.mov` carries the same processed audio as the screen file's mic track.
- **Given hesperia on built-in speakers with the mic on, when a known clip is played from another process (`afplay`) during two otherwise identical recordings — one with AEC on, one off — then the far end in the mic track is at least ~15 dB quieter in the AEC-on take, and both measured figures are written into the Auto Run Result.** If the reduction is absent, do not ship an inert switch: default it off and record the finding instead.

## Design Notes

**Why VPIO and nothing else** (settled; do not relitigate): `SCStreamConfiguration` exposes no
echo knob on macOS 15 or 26; `AVCaptureDevice.preferredMicrophoneMode` is `class, readonly`;
Apple ships no offline canceller; WebRTC's AEC3 needs a far-end reference keeper does not own.
VPIO works here precisely because its reference is the output device's mix — it builds a private
aggregate spanning input and output — which is why it can cancel audio *another app* renders.
That property is the one premise the hardware measurement above must confirm.

**The bus-0 render callback must emit nothing.** keeper is a recorder; it renders no far end.
Chromium's `kMacSystemEchoCanceller` does exactly this — zero the buffers and set
`kAudioUnitRenderAction_OutputIsSilence` — so the unit gets its reference from the device
without keeper putting a sound into the room:

```swift
private let renderSilence: AURenderCallback = { flags, _, _, _, frames, ioData in
    flags.pointee.insert(.unitRenderAction_OutputIsSilence)
    guard let ioData else { return noErr }
    for buffer in UnsafeMutableAudioBufferListPointer(ioData) {
        memset(buffer.mData, 0, Int(buffer.mDataByteSize))
    }
    return noErr
}
```

**PTS.** The input callback receives an `AudioTimeStamp`; use
`CMClockMakeHostTimeFromSystemUnits(ts.mHostTime)` for the sample PTS. That is the same domain
as `CMClockGetTime(CMClockGetHostTimeClock())` at `Capture.swift:1067` and as ScreenCaptureKit's
PTS, so the existing `micPTSLowerBound` trim, the silence fill and the rotation split all keep
working with no conversion.

**`micTrackChannels` is decided once, at capture start, and never changes** — including after a
fallback. Otherwise a mid-session fallback would hand the rotation a segment whose mic track has
a different channel count than its predecessor.

## Verification

**Commands:**
- `bun run check:rust` -- expected: clean `cargo fmt --check` + `clippy --all-targets -- -D warnings`.
- `bun run test:rust` -- expected: all nextest targets pass, including the new registry, wire and ipc-guard tests and the unchanged `recording.rs:4780` platform-free firewall.
- `bun run check` -- expected: biome + tsc + vitest green, including the new audio-controls assertions.
- `ssh mac 'cd ~/prj/keeper && git fetch && git checkout <this branch> && bun run rec:test'` -- expected: the Swift test target passes with `EchoCancellationTests`.
- `ssh mac 'cd ~/prj/keeper && bun run rec:build'` -- expected: the sidecar compiles and links against AudioToolbox/CoreAudio and the smoke script still exits 0.

**Manual checks (if no CLI):**
- The hardware measurement on hesperia (speakers, built-in mic, no headphones): drive the built
  sidecar directly over its stdio protocol for two mic-on recordings while `afplay` plays a known
  clip from a separate process, once with `echoCancellation: true` and once `false`; measure the
  far-end level in each mic track (e.g. `ffmpeg -af volumedetect` or `astats` over the same
  window) and record both figures. This needs the Mac unlocked, its output volume audible, and a
  Microphone TCC grant for the launching process — if any of those cannot be satisfied from an
  automated session, the story is complete except for this measurement, and it is owed to the
  operator.

## Auto Run Result

Status: **awaiting-operator** — implemented, gated, and exercised on real hardware at every layer
except the one that needs a room and an unlocked screen.

**Ran, and passed.** Rust: `keeper-core` 1010, `keeper` 241 (both include the new registry, wire
and ipc-guard tests). Frontend: 1761 across 157 files, `tsc` clean, `cargo clippy --workspace
--all-targets -- -D warnings` clean *including* the shell crate (a user-space GTK sysroot makes
that possible on Linux). Swift on hesperia (macOS 26.5.2, arm64): `swift build` clean and 57 tests
green, `EchoCancellationTests` among them.

**Ran on the metal, driving the real sidecar over its NDJSON contract** (no GUI needed, so it works
with the screen locked): a microphone-only audio-only session goes `starting → preflight →
recording → stopping → finalized`, writes a playable finalized `.m4a`, and emits no error and no
fallback warning. The observable signature of the feature is there:

| `echoCancellation` | mic track |
| --- | --- |
| `true` | `aac, 1 channel` |
| `false` | `aac, 2 channels` |

Mono is what the voice-processing unit produces, so a mono track means the `'vpio'` producer — not
the ScreenCaptureKit leg — really is the source, that it accepted the device, and that
`micTrackChannels` reached both writers. No `echoCancellationUnavailable` warning was emitted, so
the unit initialized on the first try against the built-in input.

**Measured on 2026-08-04 17:05–17:08 local, hesperia unlocked, built-in speakers and built-in
mic, driven through the app** (both takes: Audio-only source, system audio off, mic on, `afplay`
playing the same clip from another process for 10 s starting ~2 s in, output volume 45). Two takes
differing in nothing but the switch:

| window | AEC **off** | AEC **on** | Δ mean |
| --- | --- | --- | --- |
| far end playing (2–12 s) | mean **−36.6** dB, max −18.6 | mean **−60.4** dB, max −36.0 | **−23.8 dB** |
| nothing playing (14–22 s) | mean **−38.9** dB, max −17.9 | mean **−65.4** dB, max −42.0 | **−26.5 dB** |
| whole take | mean −38.8 dB, max −17.9 (2 ch) | mean −61.0 dB, max −31.7 (1 ch) | −22.2 dB |

**The premise holds: the far end is 24 dB down in the microphone track.** Apple's voice-processing
unit does cancel audio a DIFFERENT process played through the speakers — keeper renders nothing,
and the leakage still collapsed. That was the one assumption the whole story rested on, and it is
now a number rather than an inference from Chromium's source.

**And the owner's worry is NOT resolved by these two takes — the data is ambiguous in exactly the
way they predicted.** The second window had nothing playing, so what dropped by 26.5 dB there was
room tone: the *same* order as the far-end reduction. Two readings that move together cannot
separate "cancelled the echo" from "attenuated the microphone", because **neither take contains a
near-end voice.** A 26 dB drop in ambient is normal for a VoIP-grade noise suppressor and would be
welcome; it is also exactly what a 25 dB across-the-board attenuation looks like. Nothing here
distinguishes them.

**Still owed, and it is 20 seconds of a human counting aloud:** windows 2 and 3 of
`keeper-aec-measure.sh` (voice alone, then voice over the far end). If the voice holds within a few
dB of the AEC-off take, the feature is a canceller and ships on by default. If the voice drops like
the room tone did, it is an attenuator, and the AGC decision reopens (see `operator_actions`).

**Three defects fixed on the way**, all older than this story and all in the microphone-only
audio-only path, which had never worked end to end: a double `startWriting()` that aborted the
sidecar at start, a `guard let stream` in `stop()` that reported failure after a good recording and
left the writer unfinalized, and a non-idempotent `stop()` that aborted at the end once the second
bug stopped masking it (the host asks to stop twice — the request, then EOF). Found by trying to
take the measurement above; each one is described in its own commit.

**The open risk, named by the owner before any measurement existed: the feature could "work" by
being too quiet.** Two mechanisms, both real, both pointing the same way on speakers:

1. **The residual-echo suppressor is most aggressive during double talk.** A canceller subtracts a
   linear estimate of the echo and then a nonlinear stage suppresses what is left. That stage cannot
   tell "residual echo" from "near-end speech arriving at the same moment", so the louder the
   speakers, the more it clamps — and the operator's voice is what gets clamped.
2. **This story turned the unit's AGC OFF on purpose**, and Apple ships it ON. The reasoning stands
   for a recorder (AGC rides room tone up and down between phrases), but it also removes the makeup
   gain VPIO's own design assumes sits after the suppressor. Net effect on speakers: quieter voice
   than the unprocessed mic path delivers today.

The first version of this spec's measurement could not have caught either one: it played the far end
into silence and compared one number, which a canceller and a mute produce identically. The
acceptance and the harness are now a three-window take — far end alone, voice alone, both at once —
precisely so "the echo went away" and "the microphone went away" cannot be confused, and so the
AGC decision is settled by six figures rather than by taste.

## Decision, 2026-08-05 — the switch ships OFF

The owner recorded five sessions of their own on hesperia (screen + system audio +
mic + camera, HEVC) and settled it. The mic track's channel count records which
way each take ran, and the levels record what it cost:

| take | mic track | mic mean | mic peak |
| --- | --- | --- | --- |
| 00.45.21 | 1 ch — **AEC on** | −38.3 dB | **−10.0 dB** |
| 00.47.26 | 2 ch — AEC off | −32.5 dB | −8.7 dB |
| 00.49.44 | 2 ch — AEC off | −44.2 dB | −12.7 dB |
| 00.51.27 | 1 ch — **AEC on** | −43.2 dB | −16.5 dB |

Worth recording because it answers the "is it just quieter?" question with a
voice in the room rather than a tone: **the peaks survive** (−10.0 vs −8.7 dB on
the loud pair), while the mean falls. That is the shape of a canceller plus a
noise suppressor — speech through, floor down — not of a blanket attenuator. The
22.4 mirror also checks out: `camera-0000.mov`'s mic track matches the screen
file's to 0.1 dB in every take.

**And the switch still ships off**, by the owner's call after listening. The
reasoning is sound and worth keeping: the cancellation is genuine (~24 dB off the
far end, measured), but it is not free — the track becomes mono and carries
voice-band noise suppression that cannot be disabled separately, which is a
quality change on every recording, whereas the reverb it fixes happens only on
speakers. Headphones remove the echo path with no processing at all. So the
processing is opt-IN: `RECORDING_ECHO_CANCELLATION_DEFAULT = false`, only a
literal `"1"` reads as on, absent means off on the wire too (an older sidecar
records the plain mic), and `docs/recording.md` states both the 24 dB and the
costs so the choice is informed.

What that leaves genuinely unmeasured, and now unimportant: the double-talk window
(voice *while* the far end plays). It only matters to someone who turns the switch
on, the AGC escape hatch is documented in `operator_actions` if they find it
wanting, and no default rides on it any more.
