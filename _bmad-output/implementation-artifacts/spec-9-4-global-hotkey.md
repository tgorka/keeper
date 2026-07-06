---
title: 'Global Hotkey'
type: 'feature'
created: '2026-07-06'
status: 'done'
baseline_revision: '749b2b620ad89c11cdcf9dbafe981bf80ab87914'
final_revision: '92c63b43cd66e424d02a9d481d640cd3f0341573'
review_loop_iteration: 0
followup_review_recommended: false
context:
  - '{project-root}/docs/project-context.md'
  - '{project-root}/_bmad-output/implementation-artifacts/epic-9-context.md'
warnings:
  - oversized
---

<intent-contract>

## Intent

**Problem:** keeper has a full in-window keyboard suite (Epic 9.1–9.3) but no system-wide way to summon it: there is no global hotkey, the `tauri-plugin-global-shortcut` plugin is not wired in, and there is no persisted hotkey setting or reassignment UI. A user in another app cannot raise keeper without clicking its Dock/window (FR-50).

**Approach:** Register an OS-level global shortcut in Rust via `tauri-plugin-global-shortcut` (desktop platform port, AD-25). Its default is `⌃⌥Space`, persisted in the existing `settings` k/v table (`keeper-core::registry`). On press the Rust handler **toggles the main window**: if keeper is the focused window it hides; otherwise it shows/raises/focuses the window and emits `keeper://global-hotkey-activated`, which a frontend hook uses to switch to the Inbox view and move keyboard focus into the Unified Inbox chat list (reusing the Story 9.2 roving-focus mechanism). A new Settings → Shortcuts section reads/reassigns the hotkey through two commands (`hotkey_get`/`hotkey_set`); reassignment validates the accelerator, warns on collision with known macOS system shortcuts, and — when the OS refuses registration — explains what to enable rather than failing silently.

## Boundaries & Constraints

**Always:**
- **OS-level registration, not a window keydown hook.** The hotkey must fire while keeper is backgrounded/hidden, so it is registered with the OS through `tauri-plugin-global-shortcut` (Rust). Only one accelerator is registered at a time.
- **Toggle discriminator is window focus:** pressed while the main window `is_focused()` → `hide()`; otherwise `unminimize` (if needed) → `show()` → `set_focus()` → emit `keeper://global-hotkey-activated`. Idempotent and safe to press repeatedly.
- **Inbox focus reuses Story 9.2, never re-derives it.** The frontend hook switches to Inbox via `primaryViewStore.getState().setView("inbox")` and requests chat-list focus through a new focus-request signal that `chat-list-pane.tsx` consumes to move the roving `focusedKey`/`rowRefs` cursor to the first visible row (fallback: focus the list container when the list is empty). Focus is a UI cursor only — never a source of truth, never re-orders the Rust list.
- **Settings live in Rust.** The accelerator persists via `keeper_core::registry::{get,set}_global_hotkey` in `keeper.db`'s `settings` table under key `hotkey.global`; absent ⇒ the default `Control+Alt+Space`. No JS-writable config store. Commands are one-shot (`domain_verb` snake_case), mirroring `undo_send_window`/`incognito_*`. New Vm derives `Serialize, Deserialize, TS` + `#[serde(rename_all = "camelCase")]` + `#[ts(export)]`.
- **Conflict handling has two honest tiers:** (1) a *soft* warning when the requested accelerator matches a curated list of common macOS system shortcuts — returned in `HotkeyVm.conflict`, assignment still proceeds; (2) a *hard* failure when the OS/plugin refuses to register — the old binding is kept and an error message surfaces. When the currently-stored hotkey is not registered with the OS (`active=false`), the Settings section explains what to enable (macOS Privacy & Security) rather than showing nothing.
- Rust: no `.unwrap()`/bare `.expect()` in production paths, `tracing` only (no `println!`), logs carry ids/accelerator strings never message content. TS: no `any`, `import type`, zustand store as `use<Domain>Store`, `@/*` alias. `keeper://kebab-case` event name. `keeper-core` stays Tauri-free (accelerator *parsing/registration* lives only in the `keeper` shell crate; core stores an opaque string).

**Block If:**
- The installed `tauri-plugin-global-shortcut` / Tauri 2.x public API materially diverges from the toggle-on-press + register/unregister/is-registered model assumed here (e.g. no way to attach a press handler to a specific shortcut) — HALT naming the API gap rather than guessing.

**Never:**
- Do not implement the hotkey as a `window.addEventListener("keydown", …)` hook — that cannot fire when keeper is unfocused/hidden (contrast the in-window hooks like `use-view-shortcuts.ts`).
- Do not bind the *in-app* registry chords (⌘K, ⌘1–4, …) as OS-global accelerators; those remain owned by their JS hooks (unchanged). Only the single reassignable summon hotkey is OS-global.
- Do not add the `@tauri-apps/plugin-global-shortcut` JS package — registration/unregistration happen in Rust commands; the frontend only `listen`s to the event and calls `hotkey_get`/`hotkey_set`.
- Do not persist an accelerator the OS refused to register. Do not filter/rank anything in TS. Do not build a general key-remapping system — this story ships exactly one summon/hide hotkey.

## I/O & Edge-Case Matrix

| Scenario | Input / State | Expected Output / Behavior | Error Handling |
|----------|--------------|---------------------------|----------------|
| Summon from another app | hotkey pressed, keeper hidden/backgrounded, registered | main window shows, unminimizes, gets focus; `keeper://global-hotkey-activated` emitted → view = Inbox, focus lands on first Unified Inbox chat row | No error |
| Hide | hotkey pressed while keeper is the focused window | main window `hide()`; no event needed | No error |
| Summon with empty inbox | hotkey pressed, inbox list empty | window shows + focuses; focus-request falls back to the chat-list container (no row to focus) | No error |
| First run / no setting | `hotkey_get` before any assignment | returns default `Control+Alt+Space`, `isDefault=true`, `active` = whether it registered at startup | No error |
| Reassign, valid & free | `hotkey_set("Control+Shift+K")` | old unregistered, new registered + persisted; returns `{accelerator, isDefault:false, active:true, conflict:null}` | No error |
| Reassign, system-shortcut collision | `hotkey_set("Meta+Space")` (Spotlight) | registered + persisted but returns `conflict: "May conflict with Spotlight…"`; UI shows the warning | Soft warning, not failure |
| Reassign, OS refuses | plugin `register` errors | old binding kept & still registered; command returns `Err(IpcError)` with a clear message; nothing persisted | Err surfaced in Settings |
| Reassign, malformed accelerator | `hotkey_set("Foo+")` | rejected before touching registration | `Err(IpcError)` invalid accelerator |
| Permission/registration missing | stored hotkey not registered with OS (`active=false`) | Settings section explains enabling keeper under macOS System Settings → Privacy & Security; not silent | No throw; explanatory UI |
| Reset to default | user clicks Reset | `hotkey_set("Control+Alt+Space")` path; returns `isDefault=true` | As reassign |

</intent-contract>

## Code Map

- `src-tauri/Cargo.toml` (workspace) + `src-tauri/crates/keeper/Cargo.toml` -- MODIFY: add `tauri-plugin-global-shortcut = "2"` (Tauri 2.11.x). No frontend JS plugin package.
- `src-tauri/crates/keeper/capabilities/default.json` -- MODIFY: add `global-shortcut:allow-register`, `global-shortcut:allow-unregister`, `global-shortcut:allow-is-registered`, `global-shortcut:allow-unregister-all`.
- `src-tauri/crates/keeper-core/src/registry.rs` -- MODIFY: add `HOTKEY_GLOBAL_KEY = "hotkey.global"`, `DEFAULT_GLOBAL_HOTKEY = "Control+Alt+Space"`, `get_global_hotkey(&Path) -> Result<String, CoreError>` (absent ⇒ default), `set_global_hotkey(&Path, &str) -> Result<(), CoreError>` (opaque string; no Tauri parsing). Mirror the `incognito.global` k/v precedent.
- `src-tauri/crates/keeper/src/hotkey.rs` -- NEW: `DEFAULT_HOTKEY` const, `HOTKEY_EVENT = "keeper://global-hotkey-activated"`; `parse(&str) -> Option<Shortcut>`; `known_conflict(&str) -> Option<String>` (curated macOS system-shortcut list); `install(&AppHandle)` (register persisted-or-default at startup, `on_shortcut` handler = `toggle_main_window`, log failures via `tracing`); `toggle_main_window(&AppHandle)` (focus-based show/hide + emit event). Pure helpers (`parse`, `known_conflict`) unit-tested.
- `src-tauri/crates/keeper-core/src/vm.rs` -- MODIFY: add `HotkeyVm { accelerator: String, is_default: bool, active: bool, conflict: Option<String> }` (ts-rs, camelCase).
- `src-tauri/crates/keeper/src/ipc.rs` -- MODIFY: add `hotkey_get(app, state) -> Result<HotkeyVm, IpcError>` and `hotkey_set(app, state, accelerator: String) -> Result<HotkeyVm, IpcError>` (validate → soft `known_conflict` → unregister old / register new → persist on success; hard-fail keeps old & returns `Err`). Funnel via `to_ipc_error`.
- `src-tauri/crates/keeper/src/lib.rs` -- MODIFY: `mod hotkey;`; `.plugin(tauri_plugin_global_shortcut::Builder::new().build())`; call `hotkey::install(app.handle())` in `setup()`; register `hotkey_get`, `hotkey_set` in `generate_handler!`.
- `src/lib/ipc/client.ts` -- MODIFY: add `hotkeyGet(): Promise<HotkeyVm>`, `hotkeySet(accelerator): Promise<HotkeyVm>` wrappers + `HotkeyVm` re-export.
- `src/lib/hotkey.ts` -- NEW: pure helpers `formatAccelerator("Control+Alt+Space") -> "⌃⌥Space"` and `acceleratorFromEvent(KeyboardEvent) -> string | null` (modifiers + non-modifier key). Unit-tested.
- `src/lib/stores/chat-list-focus.ts` -- NEW: `{ focusNonce, requestFocus }` + `useChatListFocusStore` (bump a nonce to request focus).
- `src/hooks/use-global-hotkey.ts` -- NEW: `listen("keeper://global-hotkey-activated")` → `setView("inbox")` + `requestFocus()`; graceful no-op outside Tauri (mirror `use-menu-actions.ts`).
- `src/components/layout/chat-list-pane.tsx` -- MODIFY: subscribe to `focusNonce`; on change, when Inbox is active, set `focusedKey` to the first visible row and `rowRefs.current[0]?.focus()`, else focus the list container (add a container ref). Reuses existing roving-focus state only.
- `src/components/settings/settings-dialog.tsx` -- MODIFY: add a `ShortcutsSection` (mirror `SetupSection`): show the current binding as `Kbd` chips via `formatAccelerator`, a "Change…" capture control that records the next chord (`acceleratorFromEvent`) and calls `hotkeySet`, render `conflict` as a warning and the `active=false` permission explanation, plus a "Reset to default" button.
- `src/components/layout/app-shell.tsx` -- MODIFY: mount `useGlobalHotkey()`.

## Tasks & Acceptance

**Execution:**
- [x] `src-tauri/Cargo.toml` + `crates/keeper/Cargo.toml` -- add `tauri-plugin-global-shortcut` dep. -- plugin availability (cargo-deny must still pass).
- [x] `crates/keeper/capabilities/default.json` -- add the four `global-shortcut:allow-*` permissions. -- registration is otherwise denied.
- [x] `crates/keeper-core/src/registry.rs` -- `get_global_hotkey`/`set_global_hotkey` + key/default consts; unit tests (default when absent, set/get round-trip). -- persistence, Tauri-free.
- [x] `crates/keeper/src/hotkey.rs` (NEW) -- `parse`, `known_conflict`, `install`, `toggle_main_window`, event const; unit tests for `parse` (accepts default, rejects malformed) and `known_conflict` (curated combos warn, novel combos return None). -- OS registration + toggle + conflict logic.
- [x] `crates/keeper-core/src/vm.rs` -- `HotkeyVm`. -- typed IPC contract.
- [x] `crates/keeper/src/ipc.rs` -- `hotkey_get`/`hotkey_set` commands (validate/soft-warn/hard-fail/persist). -- reassignment surface.
- [x] `crates/keeper/src/lib.rs` -- `mod hotkey`; register plugin; `hotkey::install` in setup; add both commands to `generate_handler!`. -- wiring.
- [x] `src/lib/ipc/client.ts` -- `hotkeyGet`/`hotkeySet` + `HotkeyVm` re-export. -- typed IPC.
- [x] `src/lib/hotkey.ts` (NEW) + `src/lib/hotkey.test.ts` -- format + event→accelerator helpers with unit tests (I/O matrix: modifier rendering, malformed/modifier-only returns null). -- pure display/capture logic.
- [x] `src/lib/stores/chat-list-focus.ts` (NEW) -- focus-request nonce store. -- decouples the event hook from the list component.
- [x] `src/hooks/use-global-hotkey.ts` (NEW) + `src/hooks/use-global-hotkey.test.ts` -- listener dispatches `setView("inbox")` + `requestFocus`; graceful outside Tauri. -- makes the emitted event functional.
- [x] `src/components/layout/chat-list-pane.tsx` -- consume `focusNonce` to move roving focus to the first Inbox row (container fallback when empty); test the focus-request behavior. -- fulfills "focus in the chat list".
- [x] `src/components/settings/settings-dialog.tsx` + test -- Shortcuts section: renders current binding, capture calls `hotkeySet`, shows the conflict warning and the `active=false` permission explanation, Reset button. -- reassignment + honest-permission UI.
- [x] `src/components/layout/app-shell.tsx` -- mount `useGlobalHotkey()`. -- activates the hook.

**Acceptance Criteria:**
- Given the default `⌃⌥Space` and macOS registration succeeded, when it is pressed while keeper is backgrounded or hidden, then the main window raises with focus in the Unified Inbox chat list; and when pressed while keeper is the focused window, it hides (FR-50).
- Given Settings → Shortcuts, when the user reassigns the hotkey to a combo that collides with a known macOS system shortcut, then the assignment is accepted but a conflict warning is shown; and when the OS refuses the registration, the previous binding stays active and the failure is explained (FR-50).
- Given the stored hotkey is not currently registered with the OS, when the user opens Settings → Shortcuts, then it explains what to enable (macOS Privacy & Security) instead of showing nothing or failing silently (FR-50).
- Given the single OS-global summon hotkey, when it is added/reassigned, then the in-app registry chords (⌘K, ⌘1–4, ⌘?) remain owned by their existing JS hooks and are unaffected (no double-binding).

## Spec Change Log

No `bad_spec` loopback occurred; this section is intentionally empty.

## Review Triage Log

### 2026-07-06 — Follow-up review pass
- intent_gap: 0
- bad_spec: 0
- patch: 2: (high 0, medium 1, low 1)
- defer: 0
- reject: 15
- addressed_findings:
  - `[medium]` `[patch]` The Settings capture path could produce a *modifier-less* accelerator: `acceleratorFromEvent` mapped a bare key (a fat-fingered `Tab`/`Enter`/letter/`Space` while the capture field was armed) to an accelerator, which `hotkey_set` would register OS-wide and hijack that key in every app. Added a "≥1 modifier required" guard so a modifier-less press returns `null` (capture keeps waiting) instead of binding a bare key. Covered by a new unit test (`hotkey.test.ts`).
  - `[low]` `[patch]` `hotkey_set` persisted only after the OS accepted the new binding, but a persist failure *after* OS acceptance left the new hotkey live this session while the stored value stayed old (`hotkey_get` would then report `active=false` for a working key and startup would revert it). Added a rollback: on persist failure, unregister the new shortcut and re-register the previous one before returning `Err`, mirroring the existing OS-refusal restore path (`ipc.rs`).

**15 rejected** (noise / by-design / unreachable): `active` from the plugin's self-scoped `is_registered` is the honest documented proxy (true OS-wide conflict detection isn't exposed) — not a false signal; `known_conflict`'s `Meta+*` arms and its full-string/order sensitivity are harmless because the capture path is the only producer and always emits canonical `Super`-spelled, canonically-ordered accelerators; the `formatAccelerator` modifier-only/last-key-wins render gaps are unreachable because a stored accelerator is always the validated default or a previously-registered (parse-valid, key-bearing) value; the errored-inbox focus branch is unreachable because `errored` is set only when the subscribe *rejects* (no rooms loaded ⇒ `visibleRooms` empty ⇒ the container fallback correctly fires); the `hotkeyGet`/`hotkeySet` load race can't occur because Change…/Reset are disabled until the load resolves; the `pendingFocusRef` leak cannot steal focus (the completion effect's `document.activeElement === containerRef` guard abandons the request once focus moved); the multi-window toggle-inversion needs a second keeper window (single-window app); the Reset-during-capture and capture-focus-to-body concerns are benign/cosmetic a11y; re-registering an already-registered accelerator can't happen (the only bound shortcut is `previous`, unregistered first); the stacked live-regions and the three-place default-constant duplication are informational; and the missing Rust test for the OS-refusal path needs a live Tauri app (documented residual risk).

### 2026-07-06 — Review pass
- intent_gap: 0
- bad_spec: 0
- patch: 6: (high 0, medium 1, low 5)
- defer: 0
- reject: 10
- addressed_findings:
  - `[medium]` `[patch]` Cold-start focus request was dropped when the Inbox raised before its first batch streamed in (focus effect keyed only on `focusNonce`, so it parked on the container and never retried) — added a pending-focus latch that completes onto the first row once rooms arrive, guarded to only act while focus is still on the container (never steals focus the user moved). Covered by a new completion test (`chat-list-pane.test.tsx`).
  - `[low]` `[patch]` `known_conflict` omitted `Control+Space` (macOS input-source switch), one token from the `⌃⌥Space` default — added it with a warning + test (`hotkey.rs`).
  - `[low]` `[patch]` `capabilities/default.json` granted the unused `global-shortcut:allow-unregister-all` — removed it (least-privilege IPC surface).
  - `[low]` `[patch]` `hotkey_set`'s restore-previous path discarded its result with `let _ =` — now logs a `tracing::warn!` if the restore also fails (the `active=false` UX already explains the no-hotkey state) (`ipc.rs`).
  - `[low]` `[patch]` The `Pressed → toggle_main_window` handler literal was copy-pasted in three places (install + set + restore) — extracted a shared named `on_shortcut_event` fn used by all three (`hotkey.rs`, `ipc.rs`).
  - `[low]` `[patch]` The Settings "Reset to default" button hardcoded `"Control+Alt+Space"` — replaced with a shared `DEFAULT_GLOBAL_HOTKEY` constant exported from `src/lib/hotkey.ts` (`settings-dialog.tsx`).

**10 rejected** (noise / by-design / unreachable): `active` derived from the plugin's `is_registered` map is a legitimate proxy (register only succeeds when the OS accepts, and the same accelerator string re-parses to the same shortcut id) — not a false-negative; the focus-vs-visibility toggle discriminator and the lack of rapid-press debounce are the documented Design-Note decision and idempotent on the macOS-first target; the capture flow can't bind Escape/Tab chords (standard capture UX; unusual to bind) and the sub-frame double-`assign` is already guarded against state corruption by `writeId`; the `formatAccelerator` last-key-wins / glyph-dedup / modifier-only / empty-aria-label robustness gaps are unreachable because `keeper-core` only ever stores the validated default or a previously-registered accelerator; and the reordered/Option-prefixed conflict-bypass can't occur because the capture path always emits modifiers in canonical order and `hotkey_set` is only called from that UI.

## Design Notes

- **Why focus, not visibility, is the toggle discriminator:** the AC says "pressed while focused → hide; pressed while backgrounded/hidden → raise." A window can be *visible but not focused* (another app on top); `is_focused()` correctly raises in that case, whereas an `is_visible()` check would wrongly hide it.
- **Why a focus-request nonce store:** Story 9.2's chat-list focus is local component state (`focusedKey`/`rowRefs` in `chat-list-pane.tsx`) with no external entry point. A tiny `chatListFocusStore.requestFocus()` (nonce bump) lets the global-hotkey event hook ask the list to focus its first row without lifting the roving state out of the component or re-deriving ordering.
- **Why conflict detection is two-tier:** true OS-wide "is this combo taken by another app?" is not exposed by the plugin. The honest, testable contract is: a curated list flags *common* macOS system shortcuts as a soft warning (`known_conflict`), and an actual `register` failure is the hard signal (kept old binding + error). macOS `RegisterEventHotKey` does not require Accessibility permission, so the `active=false` explanation covers "could not register for any reason" rather than asserting a specific permission API.
- **Curated system-shortcut set** (starting point for `known_conflict`): `Meta+Space` (Spotlight), `Meta+Tab`/`Meta+Shift+Tab` (app switcher), `Meta+Q`, `Control+Up`/`Control+Down` (Mission Control), `Control+Left`/`Control+Right` (Spaces). Extendable; each maps to a short human message.

## Verification

**Commands:**
- `bun run check:rust` -- expected: rustfmt clean + clippy `-D warnings` (new `hotkey.rs`, `HotkeyVm`, registry fns, commands).
- `bun run test:rust` -- expected: `parse`/`known_conflict` and registry default/round-trip tests pass.
- `bun run check` -- expected: Biome + `tsc --noEmit` + Vitest pass, including `hotkey.ts`, `use-global-hotkey`, chat-list focus-request, and Shortcuts-section suites; regenerated `HotkeyVm.ts` binding compiles.
- `cargo deny check` (from `src-tauri/`) -- expected: `tauri-plugin-global-shortcut` passes the license firewall.

**Manual checks:**
- Launch the app, switch focus to another app, press `⌃⌥Space`: keeper raises with the Inbox chat list focused. Press it again while keeper is focused: it hides. In Settings → Shortcuts, reassign to a new combo and confirm it works from another app; try `⌘Space` and confirm the Spotlight-conflict warning; deny/So the registration fails and confirm the explanation appears.

## Auto Run Result

Status: done

### 2026-07-06 — Follow-up review pass

A second independent adversarial review (Blind Hunter + Edge Case Hunter) ran on the full diff since baseline `749b2b6`. Triage: 0 intent_gap, 0 bad_spec, **2 patches** (1 medium, 1 low), 0 deferred, 15 rejected. No spec loopback.

**Patches applied this pass:**
- `[medium]` `src/lib/hotkey.ts` (+ `hotkey.test.ts`) — `acceleratorFromEvent` now requires ≥1 modifier, so a bare key captured in Settings → Shortcuts (e.g. an accidental `Tab`/`Enter` while the capture field is armed) no longer becomes a modifier-less OS-global hotkey that would hijack that key system-wide. A modifier-less press returns `null` and capture keeps waiting.
- `[low]` `src-tauri/crates/keeper/src/ipc.rs` — `hotkey_set` now rolls the OS registration back to the previous binding when persisting the newly-accepted accelerator fails, so the live global shortcut and the stored value cannot diverge (previously a post-accept persist failure left the new key live while `hotkey_get` reported the old one as `active=false`).

**Verification (all re-run green):** `bun run check` (Biome + `tsc --noEmit` + **886/886** Vitest, incl. the new modifier-guard test) PASS; `bun run check:rust` (rustfmt + clippy `-D warnings`) PASS; `bun run test:rust` (cargo-nextest, **677/677**) PASS. `cargo deny` unaffected — no dependency change this pass.

**Follow-up review recommended:** false — both fixes are small and localized (a directly unit-tested input-validation guard, and a Rust rollback branch that mirrors the already-reviewed OS-refusal restore path in the same function); no broad or subtly-interacting change that would benefit from another independent pass.

### Original run

**Summary:** Implemented Story 9.4 — a system-wide global summon/hide hotkey (FR-50, AD-25). Added `tauri-plugin-global-shortcut` and wired it into the Tauri Builder + `setup()`, registering a single OS-level shortcut (default `⌃⌥Space`) whose press handler toggles the main window on focus (`is_focused()` → `hide()`; otherwise unminimize/`show()`/`set_focus()` + emit `keeper://global-hotkey-activated`). The accelerator persists in `keeper-core::registry` (`settings` k/v, key `hotkey.global`; core stays Tauri-free, storing an opaque string). A new `HotkeyVm` (ts-rs) plus `hotkey_get`/`hotkey_set` commands expose read + reassignment: `hotkey_set` validates the accelerator, computes a soft `known_conflict` warning for common macOS system shortcuts, unregisters the old binding and registers the new one, persisting only on OS acceptance (a hard OS refusal restores the previous binding and returns `Err`). A frontend `use-global-hotkey` hook listens for the event → switches to Inbox + requests chat-list focus via a new focus-request nonce store that `chat-list-pane.tsx` consumes (reusing Story 9.2 roving focus, container fallback + cold-start completion latch). A Settings → Shortcuts section renders the binding as glyph chips, captures a new chord to reassign, shows the conflict warning, and — when the binding is not registered with the OS (`active=false`) — explains what to enable rather than failing silently.

**Files changed (one-liners):**
- `src-tauri/Cargo.toml` + `crates/keeper/Cargo.toml` — added `tauri-plugin-global-shortcut = "2"`.
- `src-tauri/crates/keeper/capabilities/default.json` — added `global-shortcut:allow-{register,unregister,is-registered}`.
- `src-tauri/crates/keeper-core/src/registry.rs` — `get_global_hotkey`/`set_global_hotkey` + key/default consts (opaque string) + tests.
- `src-tauri/crates/keeper-core/src/vm.rs` — `HotkeyVm { accelerator, isDefault, active, conflict }` (ts-rs, camelCase).
- `src-tauri/crates/keeper/src/hotkey.rs` (new) — `parse`, `known_conflict`, shared `on_shortcut_event`, `install`, `toggle_main_window`, event/default consts + tests.
- `src-tauri/crates/keeper/src/ipc.rs` — `hotkey_get`/`hotkey_set` commands + `hotkey_vm` helper.
- `src-tauri/crates/keeper/src/lib.rs` — `mod hotkey`, plugin registration, `hotkey::install` in setup, both commands in `generate_handler!`.
- `src/lib/hotkey.ts` (new) + test — `formatAccelerator`, `acceleratorFromEvent`, `DEFAULT_GLOBAL_HOTKEY`.
- `src/lib/stores/chat-list-focus.ts` (new) — focus-request nonce store.
- `src/hooks/use-global-hotkey.ts` (new) + test — event listener → setView("inbox") + requestFocus.
- `src/components/layout/chat-list-pane.tsx` + test — consume the nonce to focus the first Inbox row (container fallback + cold-start completion latch).
- `src/components/settings/settings-dialog.tsx` + test — `ShortcutsSection` (chips, capture/reassign, conflict warning, active=false explanation, reset).
- `src/lib/ipc/client.ts` — `hotkeyGet`/`hotkeySet` + `HotkeyVm` re-export.
- `src/components/layout/app-shell.tsx` — mounts `useGlobalHotkey()`.
- `src/lib/ipc/gen/HotkeyVm.ts` — regenerated ts-rs binding.

**Review findings:** 2 adversarial reviewers (Blind Hunter + Edge Case Hunter) on the full diff since baseline `749b2b6`. Triage: 0 intent_gap, 0 bad_spec, **6 patches** (1 medium, 5 low), 0 deferred, 10 rejected. No spec loopback. See the Review Triage Log for the itemized reasoning. The one medium patch fixes the cold-start focus latch so first-row focus lands even when the hotkey raises before the inbox has streamed in.

**Follow-up review recommended:** true — the medium patch adds a new focus-completion effect with focus-steal guards (concurrency/focus logic worth an independent look), alongside a capability-surface reduction and several cleanups.

**Verification:** all gates independently re-run and green after the patches — `bun run check:rust` (rustfmt + clippy `-D warnings`) PASS; `bun run test:rust` (cargo-nextest, **677/677**, incl. the new `hotkey` parse/known_conflict + registry tests) PASS; `bun run check` (Biome + `tsc --noEmit` + **885/885** Vitest incl. hotkey.ts / use-global-hotkey / chat-list focus + latch / Shortcuts-section suites, + keeper-core stays Tauri-free) PASS; `cargo deny check licenses` (from `src-tauri/`) `licenses ok` (the new plugin passes the firewall).

**Residual risks:** (1) The window show/hide/focus toggle can only be exercised manually (jsdom can't drive OS windows); the frontend event→focus path and the Rust helpers' pure logic are unit-tested, but the actual raise/hide is manual-check only. (2) Global-shortcut registration honesty depends on the OS accepting `RegisterEventHotKey`; when it doesn't, `active=false` drives the Settings explanation rather than a silent failure. (3) The plugin's accelerator grammar spells ⌘ as `Super`/`Command` (no `Meta` token); the capture path emits `Super` and `known_conflict` recognizes all spellings, but a hand-edited DB value outside that grammar would render best-effort and read `active=false`.
