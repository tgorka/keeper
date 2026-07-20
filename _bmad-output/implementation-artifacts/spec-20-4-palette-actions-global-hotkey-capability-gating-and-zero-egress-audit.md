---
title: 'Story 20.4: Palette Actions, Global Hotkey, Capability-Gating & Zero-Egress Audit'
type: 'feature'
created: '2026-07-19'
status: 'done'
baseline_revision: 'e2d347a4284d7c0d22ba7089d3d344271be59913'
final_revision: '64ccc2b'
review_loop_iteration: 1
followup_review_recommended: false
context:
  - '{project-root}/docs/project-context.md'
  - '{project-root}/_bmad-output/implementation-artifacts/epic-20-context.md'
warnings: ['multiple-goals', 'oversized']
---

<intent-contract>

## Intent

**Problem:** Recording can only be started/stopped from the Recording view's Start button and the tray Stop; there is no palette verb and no optional global hotkey, so reaching capture is slow. And two epic-exit invariants are asserted only by prose: that **no** recording surface renders on macOS < 13.0 / iOS (capability-gating), and that the record→stop→recover cycle contacts **no** network host (zero-egress). Both need executable proof.

**Approach:** (1) Register three capability-gated palette actions — "Start Recording", "Stop Recording", "Open Recordings Folder" — behind the existing `requires_recording` registry gate, routed through a small shared `recording-control` frontend module. (2) Add an optional, **unset-by-default** second OS-global hotkey ("Start / stop recording") assignable in Settings → Shortcuts with the same validate→register→persist→rollback discipline and soft conflict detection as the summon hotkey, plus a cross-hotkey clash warning; its press emits an event a frontend hook turns into start-with-current-selections / stop. (3) Add a **capability-gating audit** (extend the palette gate test + a `recording_supported()` version-floor test, alongside the existing per-surface absent-when-off tests) and a **zero-egress audit** (a source-scan test proving the `keeper-rec` sidecar makes no network call, a source-scan test proving the recording UI carries no upload/share/transcription/cloud affordance, and a `docs/egress.md` note that the phase adds no host).

## Boundaries & Constraints

**Always:**
- Palette actions register **only** behind the `recording` capability (`requires_recording: true`, dropped by `query_actions` / `registry_sections` when the flag is off), exactly like the existing `open-recording` action. Titles Title-Cased to match the registry convention (`"Open Recording"`), category `"Recording"`, `shortcut: None`.
- The recording hotkey is a **second, independent** global binding stored under a new `hotkey.recording` key; default is the **empty string = unset** (no OS registration, `active=false`). It never collides with or overwrites the summon binding (`hotkey.global`).
- Reuse the summon hotkey's discipline for the recording binding: `parse` before touching registration; on OS-refusal or persist-failure restore the previous binding and return `Err` (never persist a refused accelerator); logs carry accelerator strings only. `recording_hotkey_set` requires a modifier (via the existing `acceleratorFromEvent`) so a bare single key can never bind — the "no single-key verb" guard (UX-DR29).
- Start (palette + hotkey) reads the **module-level capture stores** (`selectedRecordingTarget`, `systemAudioEnabled`, `micEnabled`, `micDeviceId`, `webcamEnabled`, `cameraDeviceId`) — the exact singletons the Start button and banner Restart read — and calls `recordingStart(...)`; a permission-blocked start surfaces through the existing 18.4 loud-failure pipeline, never silently. Stop routes through the existing `recordingStop()`.
- New Rust hotkey glue lives in the `keeper` shell (`hotkey.rs`/`ipc.rs`); `keeper-core` only stores the opaque accelerator string and stays Tauri-free. New commands are desktop-only with mobile `Unsupported` stubs.
- All quality gates green: `bun run check`, `bun run check:rust`, `bun run test:rust`. No `.unwrap()`/`.expect()` in production paths; no `any` in TS; `import type` for type-only imports.

**Block If:**
- Delivering the recording hotkey would require mutating or overloading the summon binding's `hotkey.global` key / single-binding `HotkeyVm` semantics in a way that breaks Story 9.4 (the design is a *separate* `hotkey.recording` key + a distinct press handler — do not multiplex one registration).
- The zero-egress audit discovers the recording path (sidecar, recording IPC, or recording UI) actually contacts a host or ships an upload/share/transcription/cloud affordance — that is a real egress leak, not a test to soften.

**Never:**
- No upload, share-link, transcription, or cloud affordance anywhere in the recording UI; no new network host from the recording feature — the per-release egress inventory diff for the phase stays empty.
- No single-key verb on this surface; `Esc` never stops a recording (do not add such a binding); stopping is always explicit.
- Do not rebuild the tray Stop / Open-Recordings-Folder (18.1/18.2) or the summon hotkey (9.4) — Stop stays one click from the tray regardless; only add, verify, and reuse.
- Do not stream large payloads over IPC; the new commands pass only scalar accelerator strings / a folder-reveal trigger.

## I/O & Edge-Case Matrix

| Scenario | Input / State | Expected Output / Behavior | Error Handling |
|----------|--------------|---------------------------|----------------|
| Palette, recording capable | `recording=true`, query "record" | "Start Recording", "Stop Recording", "Open Recordings Folder" listed | — |
| Palette, not capable | `recording=false` (iOS / macOS <13) | All three (and `open-recording`) absent, not disabled | — |
| Palette Start | dispatch `recording-start` | `recordingStart(current store selections)` fires; view switches to Recording | start throws → failed snapshot via 18.4 pipeline; never crashes |
| Palette Stop | dispatch `recording-stop`, session live | `recordingStop()`; tray/banner reflect stopping→finalized | not live → idempotent no-op |
| Palette Open folder | dispatch `recording-open-folder` | Reveals the effective destination dir (nearest existing ancestor if the dir isn't created yet) | reveal fails → funnelled `IpcError`, logged |
| Set recording hotkey | valid chord `⌃⌥R`, OS accepts | Persisted under `hotkey.recording`; `active=true`; VM returned | malformed → reject before registration; OS refuses → restore previous, `Err` |
| Recording hotkey unset (default) | no stored value | VM `accelerator=""`, `active=false`, `isDefault=true`; nothing registered | — |
| Clear recording hotkey | user unsets | Unregister + persist `""`; `active=false` | persist fails → funnelled `IpcError` |
| Recording hotkey == summon | chord equals `hotkey.global` | Soft conflict warning "Conflicts with the Summon keeper hotkey"; OS-refusal of the duplicate → `Err` with previous restored | — |
| Recording hotkey pressed, idle | event `keeper://recording-hotkey-toggled`, no live session | `toggleRecording()` starts with current selections | not capable / not Tauri → no-op |
| Recording hotkey pressed, live | event fired, session live | `toggleRecording()` stops | — |
| Settings Shortcuts, not capable | `recording=false` | The "Start / stop recording" row is absent | — |
| Zero-egress sidecar scan | `keeper-rec/Sources/**/*.swift` | No `URLSession`/`URLRequest`/`NWConnection`/`Network`/`http`/socket token | token found → test fails loudly |
| Zero-egress UI scan | recording frontend sources | No `upload`/`share`/`transcrib`/`cloud`/`fetch(`/`XMLHttpRequest`/`http` affordance token | token found → test fails loudly |

</intent-contract>

## Code Map

- `src-tauri/crates/keeper-core/src/registry.rs` -- Add `HOTKEY_RECORDING_KEY = "hotkey.recording"` (default `""` = unset) + `get_recording_hotkey(data_dir) -> Result<String, CoreError>` / `set_recording_hotkey(data_dir, &str)`, mirroring `get_/set_global_hotkey` (registry.rs:759). Round-trip/absent-default/overwrite tests.
- `src-tauri/crates/keeper/src/hotkey.rs` -- Add `RECORDING_HOTKEY_EVENT = "keeper://recording-hotkey-toggled"`; a named press handler `on_recording_shortcut_event` (emits the event on `Pressed`, never toggles the window); `install_recording(app)` (reads `hotkey.recording`; empty ⇒ register nothing; else `parse` + `on_shortcut`; best-effort, never panics). Reuse `parse` + `known_conflict`. Unit-test: empty accelerator installs nothing; a valid one parses.
- `src-tauri/crates/keeper/src/ipc.rs` -- Add desktop commands (+ mobile `Unsupported` stubs): `recording_hotkey_get`, `recording_hotkey_set(accelerator)` (validate→unregister-old→register-new-with-recording-handler→persist→rollback, same shape as `hotkey_set` at ipc.rs:2285; reject empty — use clear to unset), `recording_hotkey_clear` (unregister current + persist `""`). Add `recording_hotkey_vm(app, accelerator, summon)`: `is_default = accelerator.is_empty()`, `active = !empty && parse && is_registered`, `conflict = known_conflict(acc).or(non-empty && acc == summon ? Some("Conflicts with the Summon keeper hotkey.") : None)`. Reuse `HotkeyVm`. Add `recording_reveal_folder()` (desktop) — resolve `effective_destination_dir` (ipc.rs:4371), reveal it or its nearest existing ancestor via the opener; mobile stub `Unsupported`. Command tests over temp dirs.
- `src-tauri/crates/keeper/src/lib.rs` -- Register `recording_hotkey_get`, `recording_hotkey_set`, `recording_hotkey_clear`, `recording_reveal_folder` in `generate_handler!`; call `hotkey::install_recording(app.handle())` in `setup()` beside `hotkey::install` (lib.rs:128).
- `src/lib/ipc/client.ts` -- Add `recordingHotkeyGet()`, `recordingHotkeySet(accelerator)`, `recordingHotkeyClear()`, `recordingRevealFolder()` wrappers (reuse `HotkeyVm`).
- `src/lib/recording-control.ts` (new) -- `startRecordingWithCurrentSelections()` (reads the six capture stores, calls `recordingStart`), `stopRecording()` (→ `recordingStop`), `toggleRecording()` (query `recordingStatus`; `isLiveRecording` ? stop : start). The single shared entry both the palette handlers and the hotkey hook call. Colocated test.
- `src/hooks/use-recording-hotkey.ts` (new) -- Listen for `keeper://recording-hotkey-toggled`; on fire call `toggleRecording()`. Early no-op when `!capabilities.recording` or outside Tauri, mirroring `use-global-hotkey.ts`. Colocated test.
- `src/components/layout/app-shell.tsx` -- Call `useRecordingHotkey()` beside `useGlobalHotkey()` (app-shell.tsx:86).
- `src/components/command-palette/actions.ts` -- Add handlers `"recording-start"` (→ `startRecordingWithCurrentSelections()` + `primaryViewStore.setView("recording")`), `"recording-stop"` (→ `stopRecording()`), `"recording-open-folder"` (→ `recordingRevealFolder()`).
- `src-tauri/crates/keeper-core/src/palette.rs` -- Register three inline `requires_recording: true` actions (`recording-start`/`recording-stop`/`recording-open-folder`, category `"Recording"`, no shortcut) beside `open-recording` (palette.rs:436). Extend `open_recording_present_iff_recording_capability_on` (palette.rs:904) to assert all three toggle with the capability across `query`/`registry_sections`.
- `src/components/settings/settings-dialog.tsx` -- In `ShortcutsSection` add a second "Start / stop recording" row, rendered only when `capabilities.recording`: capture→`recordingHotkeySet`, a Clear button→`recordingHotkeyClear`, "Not set" when `accelerator===""`, conflict + inactive notes reused. Factor the capture control if it keeps the diff clean; do not regress the summon row.
- `src-tauri/crates/keeper/src/macos_version.rs` -- Confirm/add a `recording_supported()` version-floor unit test (macOS <13 / non-macOS ⇒ false) — the source of `CapabilitiesVm.recording`.
- Zero-egress audits: a Rust source-scan test over `tools/keeper-rec/Sources/**/*.swift` (no network token) and a Vitest source-scan test over the recording frontend sources (no upload/share/transcription/cloud/http token). Both anchor paths on the crate/project root and build forbidden tokens by concatenation so they never self-match (mirroring `dependency_firewall_holds`, recording.rs:4627).
- `docs/egress.md` -- Add a sentence: screen recording is fully local — it contacts no network host, so the per-release egress inventory diff for the recording phase is empty.

## Tasks & Acceptance

**Execution:**
- [x] `src-tauri/crates/keeper-core/src/registry.rs` -- Add `hotkey.recording` key + `get_/set_recording_hotkey` (default `""`) + round-trip/absent-default tests.
- [x] `src-tauri/crates/keeper/src/hotkey.rs` -- Add `RECORDING_HOTKEY_EVENT`, `on_recording_shortcut_event`, `install_recording` (empty ⇒ no-op) + tests.
- [x] `src-tauri/crates/keeper/src/ipc.rs` -- Add `recording_hotkey_get/set/clear` (+ `recording_hotkey_vm` with summon cross-conflict) and `recording_reveal_folder` (desktop + mobile stubs); command tests (set→get round-trip, clear→unset, empty rejected by set, cross-summon conflict surfaced, reveal nearest-ancestor).
- [x] `src-tauri/crates/keeper/src/lib.rs` -- Register the four commands + call `install_recording` in `setup()`.
- [x] `src/lib/ipc/client.ts` -- Add the four TS wrappers.
- [x] `src/lib/recording-control.ts` -- New shared start/stop/toggle module reading the capture stores + IPC; test each path (start reads stores, toggle stops when live / starts when idle).
- [x] `src/hooks/use-recording-hotkey.ts` -- New event hook calling `toggleRecording`, gated on `capabilities.recording`; test (fires toggle on event; no-ops when off / outside Tauri; cleans up listener).
- [x] `src/components/layout/app-shell.tsx` -- Mount `useRecordingHotkey()`.
- [x] `src/components/command-palette/actions.ts` -- Add the three recording handlers; extend `actions`/palette dispatch tests.
- [x] `src-tauri/crates/keeper-core/src/palette.rs` -- Register the three `requires_recording` actions; extend the capability-gating test to all three.
- [x] `src/components/settings/settings-dialog.tsx` -- Add the capability-gated recording-hotkey row; extend `settings-dialog.test.tsx` (assign, clear, conflict, and **absent when `recording=false`**).
- [x] `src-tauri/crates/keeper/src/macos_version.rs` -- Ensure a `recording_supported()` version-floor test exists (add if missing).
- [x] Zero-egress audits -- Add the Rust `keeper-rec` no-network source scan and the Vitest recording-UI no-egress-affordance source scan; update `docs/egress.md`.

**Acceptance Criteria:**
- Given the `recording` flag is on, when the Command Palette renders, then "Start Recording", "Stop Recording", and "Open Recordings Folder" are offered and dispatch (start reads the current capture selections and switches to the Recording view; stop calls `recordingStop`; open folder reveals the effective destination) — and with the flag off all three are absent (FR-48/FR-66).
- Given Settings → Shortcuts, when a user assigns a global Start/Stop Recording chord, then it registers with the OS and persists under `hotkey.recording` (unset by default), a soft conflict warning shows for a curated system shortcut or a clash with the summon hotkey, an OS-refused chord restores the previous binding and reports the error, and the row is absent when recording is not capable (FR-50).
- Given a recording is live, when the recording hotkey fires (or Stop is dispatched), then the session stops; and Stop remains one click from the tray regardless (FR-74) — and no single-key verb exists and `Esc` never stops a recording (UX-DR29).
- Given the capability-gating audit, when tests run, then `recording_supported()` returns false below macOS 13 / on non-macOS, and every recording surface (sidebar, Settings incl. the new hotkey row, palette, tray) is proven absent (not disabled) when the flag is off (FR-66, AD-35).
- Given the zero-egress audit, when tests run, then the `keeper-rec` sidecar sources contain no network API, the recording UI sources contain no upload/share/transcription/cloud affordance, and `docs/egress.md` records that recording adds no host so the phase egress diff is empty (FR-76).

## Spec Change Log

No spec amendments — the review produced no `intent_gap` or `bad_spec` findings (no loopback). All addressed findings were localized `patch`es applied directly to the diff.

## Review Triage Log

### 2026-07-19 — Review pass 1
- intent_gap: 0
- bad_spec: 0
- patch: 3: (high 0, medium 1, low 2)
- defer: 0
- reject: 7
- addressed_findings:
  - `[medium]` `[patch]` Both reviewers: the frontend zero-egress source-scan (`src/components/recording/zero-egress.test.ts`) covered only the recording `*.tsx` components plus a hand-maintained six-file list, so an egress affordance added to `src/lib/stores/recording-*.ts`, `src/lib/recording-*.ts`, or an un-listed `src/hooks/use-record*.ts` would pass **vacuously** — narrowing the epic-20 exit invariant it claims to enforce. Replaced the hand-list with prefix-globbing (`scanDir`) over the recording-namespaced lib/stores/hooks so new recording sources are scanned automatically; verified the newly-covered files carry no forbidden token.
  - `[low]` `[patch]` Edge Case Hunter: `recording_hotkey_conflict` compared the recording accelerator to the summon binding with `==`, so a case-differing spelling (`control+alt+space` vs `Control+Alt+Space`) — the same binding to the OS — silently missed the soft clash warning. Switched to `eq_ignore_ascii_case` (matching `known_conflict`'s normalization) and added a differently-cased summon-clash assertion.
  - `[low]` `[patch]` Edge Case Hunter: `nearest_existing_ancestor` accepted any existing path via `.exists()`, so a destination nested under a regular file would hand `reveal_item_in_dir` a non-directory. Tightened to `.is_dir()` (root is always a directory, so the fn stays total) and added a file-ancestor test.
- rejected (7): the palette/hotkey Start bypassing the mic/camera pre-flight *disabled-button* gate (by design per the spec's Always constraint — a permission-blocked start surfaces honestly through the 18.4 loud-failure pipeline, satisfying "never silently no-op"); `toggleRecording` re-entrancy (the Rust `recording_run` start-guard slot lock rejects a concurrent second start — no double session); the Title-case-only affordance token match (the case-insensitive functional-network tokens already catch any real leak, and case-insensitive affordance words would false-trip the mandated honest lowercase copy — documented rationale); the cross-summon *hard* block (BH verified `tauri-plugin-global-shortcut` refuses a duplicate registration and the rollback is correct — the soft warning matches the summon-hotkey precedent); the `recording-stop` palette verb not switching to the Recording view (deliberate — stop is idempotent and feedback comes from the completion card + tray); the settings capture blur/loading behaviors (mirror the pre-existing summon row exactly — consistent, not introduced by this diff); the `use-recording-hotkey` capability-flip stale-listener concern (verified handled — the `[recording]` effect dep runs cleanup/unlisten before the false-branch early return).

## Design Notes

**Second independent global binding, not a multiplexed one.** Story 9.4 registers exactly one summon accelerator under `hotkey.global` with a window-toggle handler. The recording hotkey is a *parallel* facility: a new `hotkey.recording` key (default `""`), a distinct `on_recording_shortcut_event` that emits an event instead of toggling the window, and `install_recording` that registers nothing when unset. `recording_hotkey_set` copies `hotkey_set`'s exact rollback discipline (validate → unregister old → register new → persist → restore-on-any-failure) so the live OS state and the stored value never diverge. Conflict detection reuses `known_conflict` and adds one cross-check: a non-empty recording accelerator equal to the summon accelerator warns (and the OS refuses the duplicate registration — the hard signal — restoring the previous binding).

**Start is frontend-driven because the config lives in JS.** `recordingStart` needs the target/audio/mic/camera selections, which live in module-level singleton stores (read imperatively by the Start button and banner Restart — surviving view remounts). Stop is already pure-Rust via the tray, but to keep one code path the hotkey emits an event and `use-recording-hotkey` calls `toggleRecording()`, which asks Rust for the authoritative live state (`recordingStatus`) and then stops or starts-with-current-selections. `recording-control.ts` centralizes this so the palette handlers and the hook share one tested implementation. The webview runs even while the window is hidden, so a backgrounded global-hotkey press still reaches the hook (as the summon hotkey already relies on).

**Audits mirror the existing firewall pattern.** The zero-egress source scans copy `dependency_firewall_holds` (recording.rs:4627): forbidden tokens built by string concatenation so the scan file never self-matches, paths anchored on `CARGO_MANIFEST_DIR` / the project root. The sidecar scan is the strongest evidence — `keeper-rec` is the only recording-specific process and is confirmed network-free (ScreenCaptureKit + AVAssetWriter + stdio NDJSON only). The capability-gating audit is the union of the extended palette gate test, the `recording_supported()` floor test, and the existing per-surface absent-when-off tests (sidebar-pane, settings-dialog, app-shell) plus the new hotkey-row absence test.

## Verification

**Commands:**
- `bun run check` -- expected: biome + tsc + vitest green, incl. the new `recording-control`, `use-recording-hotkey`, palette-handler, settings hotkey-row (assign/clear/conflict/absent-when-off), and recording-UI zero-egress source-scan tests.
- `bun run check:rust` -- expected: `cargo fmt --check` + `clippy --all-targets -- -D warnings` clean; no `.unwrap()` in new production paths.
- `bun run test:rust` -- expected: new registry, hotkey, command, palette-gating, `recording_supported()` floor, and the `keeper-rec` no-network source-scan tests pass; `dependency_firewall_holds` still green.

**Manual checks (if no CLI):**
- Palette (`⌘K` → "record"): the three actions appear only with recording capable; Start begins capture with the current source/audio/mic/webcam selections and lands on the Recording view; Open Recordings Folder reveals the destination in Finder. Settings → Shortcuts: assign a Start/Stop chord, confirm it toggles capture globally, clear it back to "Not set"; assigning the summon combo shows the clash warning. Inspect `docs/egress.md`: recording adds no host.

## Auto Run Result

Status: **done**

### Summary
Delivered all four legs of Story 20.4. **Palette actions:** three capability-gated verbs — "Start Recording", "Stop Recording", "Open Recordings Folder" (`requires_recording: true`, dropped from `query_actions` + `registry_sections` when the flag is off) — routed through a shared `recording-control` module; Start reads the module-level capture stores (the same singletons the Start button + banner Restart read) and switches to the Recording view; Open Recordings Folder reveals the effective destination (or its nearest existing directory ancestor). **Global hotkey:** an optional, unset-by-default second OS-global "Start / stop recording" binding under a new `hotkey.recording` registry key, assignable in Settings → Shortcuts with a Clear control, soft conflict detection (curated system shortcuts ∪ a cross-check against the summon binding), and the summon hotkey's exact validate→unregister-old→register-new→persist→rollback discipline; its press emits `keeper://recording-hotkey-toggled`, which a capability-gated `use-recording-hotkey` hook turns into a stop-if-live / start-with-current-selections toggle. **Capability-gating audit:** a `supports_recording(major)` macOS-13 floor test plus the extended palette gate test, alongside the existing per-surface absent-when-off tests and the new settings-row-absent-when-off test. **Zero-egress audit:** a Rust source scan proving the `keeper-rec` Swift sidecar makes no network call, a Vitest source scan proving the recording UI carries no upload/share/transcription/cloud affordance or network call, and a `docs/egress.md` note that recording adds no host (empty phase egress diff). `keeper-core` stays Tauri-free (opaque string k/v only); new commands are desktop-gated with mobile `Unsupported` stubs; the summon hotkey and tray Stop are reused untouched.

### Files changed
- `src-tauri/crates/keeper-core/src/registry.rs` — `hotkey.recording` key + `get_/set_recording_hotkey` (unset default `""`) + tests.
- `src-tauri/crates/keeper-core/src/palette.rs` — three `requires_recording` actions (`recording-start`/`recording-stop`/`recording-open-folder`, category "Recording") + `"Recording"` in `CATEGORY_ORDER`; gating test extended to all four ids.
- `src-tauri/crates/keeper/src/hotkey.rs` — `RECORDING_HOTKEY_EVENT`, `on_recording_shortcut_event` (emits, never touches the window), `install_recording` (unset ⇒ no-op) + tests.
- `src-tauri/crates/keeper/src/ipc.rs` — `recording_hotkey_get/set/clear` (+ `recording_hotkey_vm`/`recording_hotkey_conflict`/`validate_recording_hotkey`) and `recording_reveal_folder` (+ `nearest_existing_ancestor`), desktop + mobile stubs; command/pure-helper tests.
- `src-tauri/crates/keeper/src/lib.rs` — four commands registered; `install_recording` called in `setup()`; declares `#[cfg(test)] mod zero_egress`.
- `src-tauri/crates/keeper/src/macos_version.rs` — floor factored into pure `supports_recording(major)` + `recording_floor_is_macos_13` test.
- `src-tauri/crates/keeper/src/zero_egress.rs` (new, test-only) — `keeper-rec` Swift no-network source scan.
- `src/lib/ipc/client.ts` — `recordingHotkeyGet/Set/Clear`, `recordingRevealFolder` wrappers.
- `src/lib/recording-control.ts` (new) — `startRecordingWithCurrentSelections` / `stopRecording` / `toggleRecording` + tests.
- `src/hooks/use-recording-hotkey.ts` (new) — event listener → `toggleRecording`, capability-gated, inert outside Tauri + tests.
- `src/components/layout/app-shell.tsx` — mounts `useRecordingHotkey()`.
- `src/components/command-palette/actions.ts` — three recording handlers + tests.
- `src/components/settings/settings-dialog.tsx` — capability-gated `RecordingShortcutRow` (assign/clear/conflict/inactive) + tests.
- `src/components/recording/zero-egress.test.ts` (new) — recording-UI no-egress-affordance source scan (prefix-globbed coverage).
- `docs/egress.md` — "Screen recording adds no egress" note.

### Review findings breakdown
- **Two reviewers (Blind Hunter adversarial + Edge Case Hunter, Opus), 1 pass.** Both confirmed: `keeper-core` stays Tauri-free; no `.unwrap()`/bare `.expect()` in new production paths; the recording binding is separate from the summon binding and mirrors its rollback discipline; palette actions are absent (not disabled) when the capability is off in both projection paths; Start reads the six capture stores; the Rust sidecar scan is non-vacuous. Blind Hunter independently verified the OS refuses a duplicate hotkey registration and the rollback is correct.
- **3 patches applied** (1 medium, 2 low): broadened the zero-egress UI source scan from a hand-list to prefix-globbing so recording stores/hooks/lib are covered (no vacuous pass); made the cross-summon conflict check case-insensitive; tightened `nearest_existing_ancestor` to require a real directory. Each is covered by a new/extended test.
- **7 rejected** — see the Review Triage Log (chiefly: the by-design pre-flight-vs-loud-failure divergence, the Rust-guarded start re-entrancy, the documented Title-case affordance rationale, the verified OS-refusal for cross-summon, and pre-existing/consistent settings-row behavior).
- **0 intent_gap, 0 bad_spec, 0 defer.** `followup_review_recommended: false` — three localized fixes (a test-coverage broadening + two tiny guards), no production behavior/API/security/data consequence.

### Verification
- `bun run check:rust` — PASS (`cargo fmt --check` + `clippy --all-targets -- -D warnings`, zero warnings; no `.unwrap()` in new production paths).
- `bun run test:rust` — PASS (990/990; incl. the new registry/hotkey/command/palette-gating, `recording_floor_is_macos_13`, `keeper-rec` no-network scan, and the patched conflict/reveal tests; `dependency_firewall_holds` green).
- `bun run check` — PASS (biome clean, tsc clean, vitest 1474/1474 incl. the new recording-control/hotkey/palette/settings and broadened zero-egress source-scan tests; core-tauri-free guard green).

### Residual risks
- The recording hotkey's OS-registration legs (`recording_hotkey_set/clear`) aren't exercised end-to-end (no `AppHandle`/global-shortcut plugin in unit tests — same untested seam as the shipped summon `hotkey_set`); the decisions (validate/conflict/persist/nearest-ancestor) are factored pure and fully covered.
- The zero-egress guarantee is enforced by source scans (sidecar network-free, recording UI affordance-free); genuine end-to-end egress capture during a record→stop→recover cycle is a dogfooding/CI-runtime check owned by 20.5/20.6 and the release egress-diff gate.
- Palette/hotkey Start intentionally surfaces a permission-blocked start through the 18.4 loud-failure pipeline rather than the Recording view's disabled-button pre-flight gate — honest and never silent, but not byte-identical UX to the in-view Start.
