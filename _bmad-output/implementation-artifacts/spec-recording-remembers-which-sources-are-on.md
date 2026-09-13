---
title: 'Recording remembers which sources are on'
type: 'feature'
created: '2026-09-13'
status: 'done'
review_loop_iteration: 1
followup_review_recommended: true
context: []
baseline_revision: 'b6750ac0be00e81277b17aa61c3f308523e81477'
warnings: []
---

<intent-contract>

## Intent

**Problem:** Three of the four capture controls on the pre-record cards are session-scoped stores created at module load: system audio defaults on, the microphone and the camera default **off**, and none of the three is written anywhere (`src/lib/stores/recording-audio.ts:26`, `recording-mic.ts:34-35`, `recording-webcam.ts:34-35`). Every relaunch therefore throws away what the person chose — the owner's report is "every restart of the app these settings are coming back to different defaults" — and the defaults he wants are microphone **on** (system default input), camera **on** (system default camera), system audio **on**, echo cancellation **off**.

**Approach:** Promote the three capture toggles to persisted user-global settings carried by the existing `RecordingSettingsVm` (the VM that already carries `echo_cancellation`), defaulting to **on**, and seed the session stores from that answer once per launch. The device pickers stay ephemeral at "System default input"/"System default camera". Echo cancellation is already OFF by default and persisted — it is verified here, not changed.

**This overturns written decisions, on the owner's instruction.** Story 19.3 (`spec-19-3-…:26,42`), Story 20.1 (`spec-20-1-…:27,42`) and the epic contexts state mic/camera off-by-default and deliberately non-persisted, because off-by-default is what makes AD-36's lazy-permission contract true ("permission is requested only when the user enables the source, never preemptively"). The half of that contract this spec keeps is the load-bearing half: **nothing is requested from render, ever**. The half it gives up is off-by-default — so on a machine that has not granted Microphone or Camera, Start is blocked and names the blocker until the person grants it or turns the source off (Story 20.2's `can_start`), and turning it off now sticks.

## Boundaries & Constraints

**Always:** Rust owns the defaults and the normalization; an absent key reads **on** for all three, a stored `"0"` reads off, and anything else reads the default. No permission is requested from render or from Start — only from an explicit enable or the pre-flight row's own action. A person's change is persisted, so it survives the relaunch; the stores stay the surface the Start click reads, and are seeded from the stored answer before a start. Device selection stays ephemeral and defaults to the system device.

**Block If:** honouring a default-on source would require requesting Microphone/Camera permission at render or at Start, or silently starting a session with a source the person can see is on but which is not actually captured.

**Never:** No persisting device ids (a remembered id that vanished is worse than the system default). No change to echo cancellation's default — it stays OFF (owner decision 2026-08-05, `epics.md:3402`). No new Settings → Recording rows: the pre-record cards remain the control surface. No weakening of Story 20.2's `can_start` gate, and no auto-granting or pre-emptive prompting of TCC.

## I/O & Edge-Case Matrix

| Scenario | Input / State | Expected Output / Behavior | Error Handling |
|----------|--------------|---------------------------|----------------|
| Fresh install | no rows in `settings` | System audio, Microphone and Camera all render on; devices read "System default …"; echo off | No error expected |
| Person turns the camera off | toggle off, relaunch | Camera renders off and the next session records no camera | Failed write: the session still honours the toggle; the card surfaces the settings-store error path |
| Stored `"0"` / `"1"` | hand-edited or app-written row | `false` / `true` respectively | — |
| Stored garbage (`"maybe"`, `""`) | hand-edited `config.json` | reads the default (**on**) | Never an error — read-side normalization, like `recording.fps` |
| Start from the palette/hotkey with no card ever opened | stores never hydrated | the stored answers are hydrated before the start reads them | Read failure: the shipped defaults apply |
| `recording_start` called with the arg absent | `system_audio`/`microphone_enabled`/`camera_enabled` = `None` | Rust falls back to the **stored** preference, not a hard-coded constant | Read failure propagates as today's IPC error |
| Mic on, Microphone permission never requested | fresh Mac | no prompt from render; Start disabled naming Microphone, with the row's Request-permission action | Unchanged Story 20.2 behaviour |

</intent-contract>

## Code Map

- `src/lib/stores/recording-audio.ts:25-28`, `recording-mic.ts:33-38`, `recording-webcam.ts:33-38` -- the three ephemeral stores; their module docs state the non-persistence being overturned.
- `src/lib/stores/recording-settings.ts:79-197` -- the `RecordingSettingsVm` mirror: `ensureRecordingSettingsHydrated`, `refreshRecordingSettings`, `applyRecordingSettings` (optimistic write + confirmed echo + revert), `lastConfirmed`.
- `src/components/recording/recording-audio-controls.tsx:188-235,244-270` -- the system-audio Switch, the mic Switch (`onMicToggle`, the one lazy-permission trigger) and the echo Switch's persist shape to copy.
- `src/components/recording/recording-webcam-controls.tsx:170-179` -- the webcam Switch + camera picker.
- `src/lib/recording-control.ts:32-45` -- the palette/hotkey start; reads the three stores imperatively.
- `src-tauri/crates/keeper-core/src/registry.rs:1922-1947` -- `recording.echo_cancellation`'s key/default/normalization: the shape the three new keys copy (inverted default).
- `src-tauri/crates/keeper-core/src/config/keys.rs:674-685` -- the `KeySpec` for a `recording.*` user-global `Flag01`.
- `src-tauri/crates/keeper-core/src/vm.rs:3360-3483` -- `RecordingSettingsVm` (ts-rs `#[ts(export)]` → `src/lib/ipc/gen/RecordingSettingsVm.ts`).
- `src-tauri/crates/keeper/src/ipc.rs:6466,6473,6481-6487` -- `recording_start`'s `unwrap_or` fallbacks; `:10120-10209` `read_recording_settings`; `:10658-10715` `recording_settings_set`.
- `dev/mock-shell.ts` -- the `recording_settings_get/set` fixtures the dev harness serves.

## Tasks & Acceptance

**Execution:**
- [ ] `src-tauri/crates/keeper-core/src/registry.rs` -- add `recording.system_audio`, `recording.microphone`, `recording.camera` with `*_DEFAULT: bool = true` consts, getters (`"1"` ⇒ true, `"0"` ⇒ false, absent/garbage ⇒ default) and setters; plus a round-trip test covering absent, both values and garbage.
- [ ] `src-tauri/crates/keeper-core/src/config/keys.rs` -- three `KeySpec`s (`UserGlobal`, `Settable::AnyLayer`, `Flag01`, default `"1"`, an example) beside the other `recording.*` keys.
- [ ] `src-tauri/crates/keeper-core/src/vm.rs` -- `RecordingSettingsVm` gains `system_audio`, `microphone`, `camera` (docs naming the default and the ephemeral device selection); fix the stale `echo_cancellation` doc that still says "true (the default)".
- [ ] `src-tauri/crates/keeper/src/ipc.rs` -- `read_recording_settings` serves the three; `recording_settings_set` writes them; `recording_start`'s three `unwrap_or` fallbacks read the stored preference instead of constants; extend the settings read/write tests.
- [ ] `docs/settings-keys.md` -- regenerate (never hand-edit).
- [ ] `docs/recording.md` -- document the three keys, the new defaults, the owner decision of 2026-09-13 overturning mic/camera off-by-default, and what a default-on source means for a machine without the TCC grant.
- [ ] `src/lib/stores/recording-settings.ts` -- seed the three capture stores from the first confirmed VM (once per launch, never re-seeding over a live choice) and expose the write-through helper the switches use.
- [ ] `src/lib/stores/recording-audio.ts`, `recording-mic.ts`, `recording-webcam.ts` -- module-load defaults become on/on/on (matching Rust), module docs rewritten to say where the answer now lives; the test resets follow.
- [ ] `src/components/recording/recording-audio-controls.tsx` -- the system-audio and mic switches persist their new value; the mic's lazy permission request stays bound to an explicit enable only.
- [ ] `src/components/recording/recording-webcam-controls.tsx` -- same for the webcam switch.
- [ ] `src/lib/recording-control.ts` -- await the settings hydration before reading the stores, so a palette/hotkey start with no card ever opened uses the stored answers.
- [ ] `dev/mock-shell.ts` -- the recording-settings fixture carries the three fields and round-trips them.
- [ ] Frontend tests -- update every suite pinned to the old defaults (`recording-mic.test.ts`, `recording-webcam.test.ts`, `recording-audio.test.ts`, `recording-audio-controls.test.tsx`, `recording-webcam-controls.test.tsx`, the `RecordingSettingsVm` fixtures) and add: the seeding of a stored `false`, the write-through on toggle, and that render still requests no permission.

**Acceptance Criteria:**
- Given a fresh install, when the Recording view opens, then System audio, Microphone and Camera all read on, the pickers read the system default device, and Echo cancellation reads off.
- Given the camera is turned off and the app is relaunched, when the Recording view opens, then the camera reads off and a session started from the palette records no camera.
- Given the Recording view is rendered with the microphone on and permission never requested, when the card mounts, then no permission prompt is issued and Start remains gated by Story 20.2 naming Microphone.

## Spec Change Log

**The answers left `RecordingSettingsVm` and took their own command pair** (review pass, both lanes independently). The Approach above says the three flags ride on the existing settings VM; they do not. `recording_settings_set` takes that VM *whole*, and a whole-VM write is a DESTINATION decision — `destination_choice` names one key and clears the other — plus a recordings-index rebuild. The VM it takes is a lossy READ: an unresolvable profile (paused, drive out, `git` briefly unusable) degrades to `kind: Folder` with the plain-folder root. So a camera toggle made in that window would have written that degraded answer into `recording.destination_dir`, cleared `recording.destination_profile_id`, and sent every later recording to the boot disk with nothing on screen saying so — a bigger, quieter failure than the one this spec set out to end, behind the two most-clicked switches on the card. They are now `RecordingCaptureSourcesVm` + `recording_capture_sources_get`/`_set`, and the settings VM no longer carries them.

**The write is a PATCH, not the triple.** Writing all three would persist the shipped defaults for the two sources whose stored answers this launch had not read yet — the spec's own bug, re-entered through its fix. `RecordingCaptureSourcesPatchVm` carries `Option<bool>` per source and `write_capture_sources` writes only what it is given.

**An audio-only target records no camera.** Not in the spec as written, and reachable only because this change makes the camera default ON: the sidecar's two audio-only sub-modes disagreed (with system audio it wrote `camera-####.mov` for a session the person asked to be audio-only; without it, it wrote nothing while the manifest claimed `camera: true`). Both halves are the spec's own Block If. `recording_start` now resolves `camera_on && !audio_only`, and the Webcam card says so instead of promising a file nobody writes.

**A start no longer waits on the read.** The spec's matrix row said the palette/hotkey path hydrates before sampling the stores; that made a hung settings read a silent no-op where a start used to happen. A source nobody has answered this launch is sent as `undefined` and decided in Rust from the stored preference, which is what that fallback is for.

**The "card surfaces the settings-store error path" half of the failed-write row is withdrawn.** There is no such surface on the pre-record cards and inventing one is a different change. The honest behaviour is implemented and tested instead: a failed write leaves the session value standing (the switch the person is looking at is what this session applies), and no later write re-persists it.

## Review Triage Log

Two lanes in parallel (adversarial, edge-case), both read-only, over the working tree.

**Patched (8).** (1) HIGH, both lanes — the destination clobber above. (2) HIGH — three index rebuilds for three clicks; gone with the narrow command. (3) HIGH — the audio-only camera contradiction. (4) MEDIUM — the seeding one-shot was launch-wide, so toggling one switch early threw away the other two stored answers; now per source, with the write patched to match. (5) MEDIUM — a failed write left the mirror and the switch disagreeing and the next write re-persisted the stale value; each source now writes only itself. (6) MEDIUM — `resetRecordingSettingsForTest` never re-armed the seeding one-shot, so every case after the first in a file asserted nothing (found by mutation testing before the lanes reported). (7) MEDIUM — the mount permission probe ran before seeding, costing a second `keeper-rec` spawn per launch on any machine that had turned a source off; it now waits for the (deduped, never-rejecting) read. (8) MEDIUM — `start_time_mic_selection_takes_echo_cancellation_from_the_registry` re-implemented a composition this change deleted and asserted a local against itself; trimmed to the wire assertion that tests something.

**Accepted as-is (3).** A switch shows the shipped default for one IPC round-trip before the stored answer lands — the Design Notes' imperative-store property is worth more than a disabled switch at first paint, and the window is a single read. A config-file layer can pin a key so a toggle appears to do nothing — now visible, because the switch takes the effective answer back, and documented in `docs/recording.md`. The three switches stay live during a live session — pre-existing, and now safe: the write touches three keys and the running sidecar bound its sources at Start.

**Out of scope (1).** `DestinationRefusal::UnknownProfile` had been deleted from the enum while still constructed and matched — an unrelated, uncompilable-on-Linux break that would have failed CI's macOS job. Restored before the lanes ran.

## Design Notes

The three toggles keep their module-level stores rather than reading a VM directly: the Start click, the palette verb and the hotkey all read them imperatively and outlive view remounts (`recording-control.ts`). The stores are therefore **seeded mirrors** — the compile-time default equals the Rust default, the stored answer overwrites it once per source, and every toggle writes its own source through.

Seeding is per source and compares against the value each store held when the read was ISSUED: a source that moved while the read was in flight was moved by a person, in front of a card that is live from first paint, and seeding it would undo their click and then persist the undo.

Device ids stay ephemeral deliberately: a remembered id for a camera that is not plugged in today would be reconciled away at the next enumeration (`isCameraSelectionAvailable`), so persisting it buys a stale value and an extra failure mode.

## Verification

**Commands:**
- `bun run lint && bun run typecheck && bun run test` -- expected: green; the VM field addition surfaces every stale fixture as a type error.
- `cargo nextest run --manifest-path src-tauri/Cargo.toml -p keeper-core -p keeper-sync` -- expected: green, incl. the new registry round-trips and the `config::keys` classification/doc guards.
- `cargo test -p keeper-core --lib config::keys::tests::docs::regenerate -- --ignored` -- regenerates `docs/settings-keys.md`.
- `bun run check:rust:macos` (hesperia) -- the shell crate half: `recording_start`'s fallbacks and the settings read/write live there and cannot be compiled on the Linux host.

**Manual checks:**
- The dev harness under a real browser: all three switches render on at first paint, turning one off and reloading shows it off (the harness round-trips the settings VM), and the pickers read the system-default labels.
