---
title: 'Cheat Sheet and Native Menu Bar from the Action Registry'
type: 'feature'
created: '2026-07-06'
status: 'done'
baseline_revision: '4567e899395836b291a7a03994ef1b69ecfc82fd'
final_revision: '6ee722b0b2088363a2b0c9ffab7915de66e7f830'
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

**Problem:** keeper has an action registry (`keeper-core::palette::palette_actions()`, Story 9.1) that the ⌘K palette consumes, but epic 9's two remaining discovery surfaces are missing: there is no ⌘? cheat sheet listing every shortcut, and there is **no native macOS menu bar at all** (Tauri's default is used) — so shortcut discovery is invisible to keyboard/VoiceOver users and no automated gate proves palette parity. The registry also lacks toggle-pairing metadata, so any naïve generation would render each toggle key (E/P/F/U) as two ambiguous rows.

**Approach:** Generate both surfaces from a single Rust projection of `palette_actions()`. Add toggle-pairing metadata to the registry and a `registry_sections()` projection that groups actions by category and collapses each toggle pair into one entry. The native menu bar is built in Rust from `registry_sections()` at startup (menu clicks emit a `keeper://menu-action` event the frontend dispatches through the existing `actions.ts` map); the ⌘? cheat sheet is a searchable frontend overlay that renders the same sections via a `cheat_sheet_sections` command. A derived Rust **parity test** asserts every MVP surface has ≥1 registered action (FR-48 release gate), with justified exclusions.

## Boundaries & Constraints

**Always:**
- **Single source of truth:** both the native menu and the cheat sheet derive from `registry_sections()`, which is derived from `palette_actions()`. No hand-maintained shortcut list anywhere; adding/removing a registry action must automatically change both surfaces (UX-DR15).
- **Menu execution reuses the existing dispatch:** menu clicks route through the same frontend `dispatchPaletteAction(id, context)` map (`src/components/command-palette/actions.ts`) the palette uses — no second dispatch table. Open-chat context is `roomsStore.getState().selected` (exactly as `command-palette.tsx`'s `runAction` does); when it is `null`, `requires_open_chat` handlers already no-op.
- **Toggle pairs collapse to one entry** in both surfaces using the new pairing metadata (`archive`/`unarchive`→E, `pin`/`unpin`→P, `favorite`/`unfavorite`→F, `mark-read`/`mark-unread`→U). A collapsed menu item resolves its direction at click time from the open room's current flag (`isArchived`/`isPinned`/`isFavourite`/`effectiveIsUnread` on the `roomsStore.rooms` entry matching `selected`), mirroring the chat-list verb logic — never re-derived independently.
- **Preserve the standard macOS menus:** replacing the default menu must re-add the predefined App (About/Quit ⌘Q), Edit (Undo/Redo/Cut/Copy/Paste/Select All), and Window (Minimize/Zoom) menus, then append one generated submenu per registry category. Losing Copy/Paste is a regression.
- **New Vm types:** `#[derive(Serialize, Deserialize, TS)]` + `#[serde(rename_all = "camelCase")]` + `#[ts(export)]`. Rust: no `.unwrap()`/bare `.expect()` in production paths, `tracing` only, logs carry ids not content. TS: no `any`, `import type`, zustand store as `use<Domain>Store`, `@/*` alias. `keeper://kebab-case` event names.
- **⌘? hook** follows the shipped shortcut-hook pattern (window keydown, `event.isComposing` early-return, `preventDefault`, `ctrl` accepted alongside `meta`); mirrors `use-command-palette-shortcut.ts` (no text-field guard — it is a chord). The cheat sheet is a single modal overlay (depth ≤ 1).

**Block If:**
- A registry action's `id` has no handler in `actions.ts` such that the menu could dispatch a dead id — HALT naming the id (should not occur; the 19 ids are all mapped today).

**Never:**
- Do not bind native OS accelerators on the generated menu items. Every registry chord (⌘1–4, ⌘N, ⌘⇧F) is already owned by a shipped JS window hook, and the single-key verbs (E/P/F/U) are context-scoped list keys that must not become global; binding accelerators would double-fire or hijack typing. The shortcut is shown as menu-item **label text** for discovery/VoiceOver; the JS hooks remain the sole binding owner. (Migrating chords to native-bound accelerators and retiring the duplicate hooks is a documented future improvement, out of scope here.)
- Do not add new palette actions for surfaces lacking a clean cold-open entry point (device verification, key backup) or backend (mute) — the parity test records these as justified exclusions (consistent with 9.1's Block-If and the deferred-work ledger). Do not build the global hotkey (9.4) or change palette query/ranking behavior.
- Do not filter/rank chat or action *results* in TypeScript (AD-20). The cheat sheet's search box is a plain substring filter over the static registry reference (presentation), not result ranking.
- No new ordering/state held in a TS store as source of truth; the cheat sheet fetches sections fresh on open.

## I/O & Edge-Case Matrix

| Scenario | Input / State | Expected Output / Behavior | Error Handling |
|----------|--------------|---------------------------|----------------|
| Open cheat sheet | `⌘?` pressed (not composing) | Searchable overlay opens listing all shortcuts grouped by category, each row = title + Kbd chip; `preventDefault` | No error |
| Toggle collapse | cheat sheet rendered | Each toggle pair shows as ONE row (e.g. "Archive / Unarchive Chat" · `E`), not two ambiguous rows | No error |
| Cheat sheet search | overlay open, type `"arch"` | Rows filter by substring over title/category/shortcut; category headings for matching rows remain | No error |
| Menu item (nav) | click "Open Inbox" in menu | `keeper://menu-action` emits `open-inbox`; frontend `dispatchPaletteAction("open-inbox", …)` → `setView("inbox")` | No error |
| Menu item (toggle) | click "Archive / Unarchive Chat", open room `isArchived=true` | Resolver reads flag → dispatches `unarchive-chat` on the open room | No error |
| Menu item, no open chat | click a `requires_open_chat` item, `selected=null` | `dispatchPaletteAction(id, null)` → handler no-ops (item stays enabled) | No error |
| Standard menus | app running | App/Edit/Window menus present; Copy/Paste/Quit work | No error |
| Parity gate | Rust test over `palette_actions()` | Every enumerated MVP surface has ≥1 action OR is in the justified-exclusion allowlist; else the test FAILS | Test failure surfaces drift |
| No accelerators | press `E` while typing in composer | No menu action fires (no native accelerator bound); JS list-verb hook still governs `e` only when a row is focused | No error |

</intent-contract>

## Code Map

- `src-tauri/crates/keeper-core/src/vm.rs` -- MODIFY: add `toggle_group: Option<String>` to `PaletteActionVm`; add `MenuSectionVm { category, items: Vec<MenuItemVm> }` and `MenuItemVm { id, title, shortcut: Option<String>, toggle_group: Option<String>, requires_open_chat }` (ts-rs, camelCase).
- `src-tauri/crates/keeper-core/src/palette.rs` -- MODIFY: set `toggle_group` on the 8 paired actions (groups `archive`/`pin`/`favorite`/`read`); add `registry_sections() -> Vec<MenuSectionVm>` (group `palette_actions()` by category in a stable order, collapse each `toggle_group` into one item with a combined title + shared shortcut + canonical id); add tests (collapsing, ordering, parity).
- `src-tauri/crates/keeper/src/ipc.rs` -- MODIFY: add `cheat_sheet_sections() -> Vec<MenuSectionVm>` returning `keeper_core::palette::registry_sections()`.
- `src-tauri/crates/keeper/src/menu.rs` -- NEW: build the native macOS menu (predefined App/Edit/Window submenus + one generated submenu per registry category, item labels `"<title>  <shortcut>"`, item ids = action/canonical ids) and the `on_menu_event` handler that `emit`s `keeper://menu-action` with the clicked item id.
- `src-tauri/crates/keeper/src/lib.rs` -- MODIFY: `mod menu;`; register `cheat_sheet_sections` in `generate_handler!`; build + set the menu and wire `on_menu_event` in `setup()`.
- `src/lib/ipc/client.ts` -- MODIFY: add `cheatSheetSections(): Promise<MenuSectionVm[]>` wrapper.
- `src/lib/stores/cheat-sheet.ts` -- NEW: zustand store `{ isOpen, open, close, toggle }` + `useCheatSheetStore` (mirror `command-palette.ts`).
- `src/hooks/use-cheat-sheet-shortcut.ts` -- NEW: `⌘?` toggles the store (IME-guarded, `preventDefault`, `ctrl` parity).
- `src/hooks/use-menu-actions.ts` -- NEW: `listen("keeper://menu-action")`; resolve open-chat context + toggle direction from `roomsStore`, call `dispatchPaletteAction`.
- `src/components/cheat-sheet/cheat-sheet-overlay.tsx` -- NEW: searchable `CommandDialog` grouped by category (title + `Kbd` chip), fetches `cheatSheetSections()` on open; read-only reference.
- `src/components/layout/app-shell.tsx` -- MODIFY: mount `useCheatSheetShortcut()`, `useMenuActions()`, and `<CheatSheetOverlay/>`.
- `src/components/command-palette/actions.ts`, `src/lib/stores/rooms.ts` -- REFERENCE only: `dispatchPaletteAction`, `selected`, `rooms`/flags, `effectiveIsUnread` already exist.

## Tasks & Acceptance

**Execution:**
- [x] `src-tauri/crates/keeper-core/src/vm.rs` -- add `toggle_group` to `PaletteActionVm`; add `MenuSectionVm`/`MenuItemVm`. -- typed contract shared by menu + cheat sheet.
- [x] `src-tauri/crates/keeper-core/src/palette.rs` -- populate `toggle_group`; add `registry_sections()` (category grouping + toggle collapse). -- the single projection both surfaces consume.
- [x] `src-tauri/crates/keeper-core/src/palette.rs` (tests) -- unit-test `registry_sections` collapsing (4 pairs → 4 single rows, shared shortcut, both ids retained/canonical), category ordering, and the **parity test**: enumerate MVP surfaces (epics 1–8) and assert each maps to ≥1 registered action or a justified-exclusion allowlist (device verification, key backup, mute — documented), failing on any uncovered surface. -- FR-48 release gate + I/O matrix.
- [x] `src-tauri/crates/keeper/src/ipc.rs` (+ `lib.rs`) -- add `cheat_sheet_sections` command; register it. -- frontend data source.
- [x] `src-tauri/crates/keeper/src/menu.rs` (+ `lib.rs`) -- build the native menu from `registry_sections()` (standard menus preserved), wire `on_menu_event` → `keeper://menu-action`. -- native discovery surface.
- [x] `src/lib/ipc/client.ts` -- add `cheatSheetSections` wrapper. -- typed IPC call.
- [x] `src/lib/stores/cheat-sheet.ts` + `src/hooks/use-cheat-sheet-shortcut.ts` -- open-state store + `⌘?` toggle. -- entry point.
- [x] `src/hooks/use-menu-actions.ts` -- listen for `keeper://menu-action`, resolve context/toggle direction, dispatch. -- makes the menu functional via the shared dispatch.
- [x] `src/components/cheat-sheet/cheat-sheet-overlay.tsx` + mount in `src/components/layout/app-shell.tsx` (with the two hooks) -- searchable grouped overlay. -- the ⌘? surface.
- [x] `src/components/cheat-sheet/cheat-sheet-overlay.test.tsx`, `src/hooks/use-cheat-sheet-shortcut.test.ts`, `src/hooks/use-menu-actions.test.ts` -- Vitest: ⌘? opens/toggles + guards + preventDefault; overlay renders grouped sections and collapses toggle pairs to one row and filters on search; menu-action listener dispatches by id and resolves toggle direction from room flag, no-ops on null context. -- I/O matrix coverage.

**Acceptance Criteria:**
- Given the app anywhere, when `⌘?` is pressed, then a searchable overlay opens listing every shortcut grouped by category, generated from the same registry the palette consumes — with no hand-maintained list and each toggle pair shown as a single unambiguous row (FR-49, UX-DR15).
- Given the app runs on macOS, when the menu bar is shown, then every registered command appears as a native menu item (labeled with its shortcut) under a category submenu, the standard App/Edit/Window menus still work, and clicking an item runs the same handler the palette would via `dispatchPaletteAction` (NFR-14, UX-DR15).
- Given a collapsed toggle menu item and an open chat, when it is clicked, then the correct direction fires for that chat's current state; and when no chat is open, a `requires_open_chat` item no-ops without error.
- Given a release audit, when the parity test runs, then it passes only if every MVP UI surface is reachable through ≥1 registered palette action or is in the documented justified-exclusion allowlist, failing if a new surface ships without an action (FR-48 release gate).

## Spec Change Log

No `bad_spec` loopback occurred; this section is intentionally empty.

## Review Triage Log

### 2026-07-06 — Review pass
- intent_gap: 0
- bad_spec: 0
- patch: 0
- defer: 2: (high 0, medium 0, low 2)
- reject: 19
- addressed_findings:
  - none

Two adversarial reviewers (Blind Hunter + Edge Case Hunter) on the full diff since baseline `4567e89`. No code changed this pass — no patch or bad_spec loopback → `addressed_findings: none`. **2 deferred** (appended to `deferred-work.md`): (1) the native-menu collapsed-toggle direction resolves only from the inbox window, so "Unarchive"/etc. is unreachable from the menu for archived/out-of-window open rooms (bounded/idempotent; rooted in the same missing generic per-room state already deferred for the palette's both-directions rendering); (2) ⌘? stacks over an open palette because there is no dialog-precedence coordinator (the same pre-existing "modal depth ≤ 1 unenforced" architecture deferred from 9.1; the cheat sheet is one more uncoordinated sibling). **19 rejected:** `combined_toggle_title`'s underflow/`"/"`-garbage and `registry_sections`' 3+/1-member-group cases are all unreachable given registry invariants (both reviewers walked the arithmetic and confirmed it is panic-free; the `covers_all_actions` test guards pairing); always-enabled `requires_open_chat` menu items that no-op are the documented I/O-matrix design decision; no-text-field-guard on ⌘? and non-US-layout `?` match 9.1/9.2's by-design rejections for global chords; the double-read in `resolveMenuActionId`/dispatch is synchronous with no divergence; category-name/duplicate-id/empty-section collisions require registry states that cannot occur (ids and categories are unique and every category has actions); the label-text shortcut / VoiceOver, `maximize`-vs-`zoom`, single-letter cheat-sheet search, and swallowed never-failing fetch are cosmetic or consequences of the documented no-accelerator design; and the menu.rs test-coverage and parity-test-rigor critiques are test-style notes (the parity test meets the AC's "checklist or test" and its exclusion-count tripwire guards silent exclusion growth).

## Design Notes

- **Why a shared `registry_sections()` and not two generators:** the epic's "single source of truth" is only real if both surfaces read one derivation. The Rust menu builder and the `cheat_sheet_sections` command both call `registry_sections()`, which itself reads `palette_actions()` — three layers, one source, provable no-drift. The frontend never re-groups or re-orders.
- **Why no native accelerators:** every registry chord already has a shipped JS hook (`use-view-shortcuts`, `use-command-palette-shortcut`, `use-new-chat-shortcut`, `use-search-shortcuts`) and the verbs E/P/F/U are context-scoped list keys. Binding OS accelerators would double-fire (e.g. ⌘K toggle would net to no-op) or hijack typing (E). The menu is a *discovery + accessibility + click-to-run* surface; the macOS menu bar is inherently keyboard- and VoiceOver-navigable without per-item key equivalents, and showing the chord as label text satisfies NFR-14 discovery. This keeps the JS hooks the untouched sole binding owner (no 9.1/9.2 regression).
- **Toggle direction resolution:** a collapsed menu item emits its canonical id (e.g. `archive-chat`); `use-menu-actions` looks up the open room in `roomsStore.rooms` and flips to `unarchive-chat` when `isArchived`, exactly as the chat-list verbs pick direction — reused, not re-derived. The cheat sheet is read-only reference (no dispatch), so it needs no direction resolution.
- **Parity as a test, not a promise:** 9.1 self-certified coverage with a hand-picked id list. Here the test enumerates the epic-1–8 surface inventory and asserts each is covered or explicitly excluded, so a future surface added without an action breaks the build — the FR-48 gate becomes mechanical.

## Verification

**Commands:**
- `bun run check:rust` -- expected: rustfmt clean + clippy `-D warnings` passes (new `menu.rs`, `registry_sections`, Vm types).
- `bun run test:rust` -- expected: `registry_sections` collapsing/ordering tests + the parity test pass.
- `bun run check` -- expected: Biome + tsc + Vitest pass, including the cheat-sheet overlay, ⌘? hook, and menu-action listener suites; regenerated `MenuSectionVm.ts`/`MenuItemVm.ts` bindings compile.

**Manual checks:**
- Launch the app: the macOS menu bar shows category submenus mirroring the palette actions with shortcut labels; Copy/Paste/Quit still work; clicking "Open Archive" switches views; clicking "Archive / Unarchive Chat" with a chat open toggles it. Press `⌘?`: the cheat sheet lists every shortcut grouped, toggle pairs as single rows, and search narrows the list.

## Auto Run Result

Status: done

**Summary:** Implemented Story 9.3 — the ⌘? cheat sheet and the native macOS menu bar, both generated from a single Rust projection of the Story-9.1 action registry, plus toggle-pairing metadata and the FR-48 palette-parity gate. Added `toggle_group` to `PaletteActionVm` (set on the 4 toggle pairs: archive/pin/favorite/read) and a `registry_sections()` projection that groups `palette_actions()` by category in a stable order and collapses each toggle pair into one entry (e.g. "Archive / Unarchive Chat"). The native menu bar is built in Rust from `registry_sections()` at startup — standard App/Edit/Window predefined menus preserved, one generated submenu per category, shortcut shown in the item label text (no OS accelerators bound, so the shipped JS shortcut hooks remain the sole binding owner and nothing double-fires); a menu click emits `keeper://menu-action` which the frontend routes through the existing `dispatchPaletteAction`. The ⌘? cheat sheet is a searchable, read-only `CommandDialog` grouped by category that renders the same sections via a new `cheat_sheet_sections` command. A derived Rust parity test enumerates the epic-1–8 surfaces and asserts each is covered by ≥1 registered action or is in a documented justified-exclusion allowlist (device verification, key backup, mute), failing on drift.

**Files changed (one-liners):**
- `src-tauri/crates/keeper-core/src/vm.rs` — added `toggle_group` to `PaletteActionVm`; new `MenuSectionVm`/`MenuItemVm` (ts-rs, camelCase).
- `src-tauri/crates/keeper-core/src/palette.rs` — `toggle_group` on the 8 paired actions; `registry_sections()` + `combined_toggle_title()`; collapsing/ordering/coverage + FR-48 parity tests.
- `src-tauri/crates/keeper/src/ipc.rs` (+ `lib.rs`) — `cheat_sheet_sections` command; built+set the native menu and wired `on_menu_event` in `setup()`; `mod menu`.
- `src-tauri/crates/keeper/src/menu.rs` (new) — registry-derived native menu builder + `keeper://menu-action` emitter.
- `src/lib/ipc/client.ts` — `cheatSheetSections()` wrapper + Menu*Vm re-exports.
- `src/lib/stores/cheat-sheet.ts` (new) — `{isOpen,open,close,toggle}` store.
- `src/hooks/use-cheat-sheet-shortcut.ts` (new) — ⌘? toggle (IME-guarded, preventDefault, ctrl parity).
- `src/hooks/use-menu-actions.ts` (new) — `keeper://menu-action` listener + `resolveMenuActionId` toggle-direction resolver.
- `src/components/cheat-sheet/cheat-sheet-overlay.tsx` (new) — searchable grouped read-only overlay.
- `src/components/layout/app-shell.tsx` — mounted the two hooks + `<CheatSheetOverlay/>`.
- `src/components/command-palette/command-palette.test.tsx` — added `toggleGroup` to the test action factory.
- Tests: `use-cheat-sheet-shortcut.test.ts`, `use-menu-actions.test.ts`, `cheat-sheet-overlay.test.tsx` (new).
- `src/lib/ipc/gen/{PaletteActionVm,MenuSectionVm,MenuItemVm}.ts` — regenerated ts-rs bindings.

**Review findings:** 2 adversarial reviewers (Blind Hunter + Edge Case Hunter). Triage: 0 intent_gap, 0 bad_spec, **0 patches**, **2 deferred**, 19 rejected. No spec loopback. See the Review Triage Log for the itemized reasoning. The two deferrals are genuine but bounded and rooted in capabilities already deferred elsewhere (generic per-room toggle state; a shared dialog-precedence coordinator).

**Follow-up review recommended:** false — this review pass made zero code changes, so there is nothing new to independently re-review.

**Verification:** all gates independently re-run and green — `bun run check:rust` (rustfmt + clippy `-D warnings`) PASS; `bun run test:rust` (cargo-nextest, **671/671**, incl. the new `registry_sections` collapsing/ordering/coverage + FR-48 parity tests and regenerated ts-rs bindings) PASS; `bun run check` (Biome + `tsc --noEmit` + **864/864** Vitest incl. the cheat-sheet-overlay / ⌘? hook / menu-action suites, + keeper-core stays tauri-free) PASS.

**Residual risks:** (1) The native menu's collapsed toggle items can't reach the negative direction ("Unarchive"/etc.) for archived or out-of-inbox-window open rooms (deferred — bounded/idempotent, needs generic per-room state). (2) ⌘? can stack over an already-open palette/dialog (deferred — the same uncoordinated-dialog architecture flagged for ⌘K). (3) Menu item shortcuts are shown as label text, not OS-bound accelerators (deliberate, to avoid double-firing the shipped JS hooks); migrating chords to native-bound accelerators and retiring the duplicate hooks is a documented future improvement. (4) Parity exclusions (device verification, key backup, mute) are deliberate documented gaps guarded by the exclusion-count tripwire.
