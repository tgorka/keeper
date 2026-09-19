---
status: review
baseline_revision: a925c4b
final_revision: ''
---

# Story 76.5 — A bar that speaks in icons

<intent-contract>

## Problem
The existing filter bar spends tag width on text toggles, claims a file-scan posture it does not have, and is absent from the phone's separate search implementation.

## Approach
Keep the existing chip components and filter semantics. Surround them with a scope/tag lane and a fixed icon run, then one search frame with a static state glyph. Adopt the same bar on phone with explicit 44 px geometry. Mirror Rust search state per vault; never infer indexing from a pending query. Keep the count outside the bar.

## Always
Stable accessible names equal tooltip text; all three toggles expose pressed state. Tags retain words and their three-state grammar. Save reserves its slot, and service visibility neither creates a savable space nor participates in Escape. Phone keyboard reading order remains scope, chooser, chips, actions.

## Block If
A control disappears or shrinks under its target floor; an old search-state response is applied to the current vault; a persistence failure looks acknowledged. Browser arithmetic is prediction until measured.

## Never
No second tag grammar, mode picker, spinner animation, architectural slogan, frontend matcher, hidden action, or whole-VM preference write.

## I/O and edge-case matrix

| Input / branch | Observable outcome / proof |
| --- | --- |
| Desktop 240 / 320 / 420 px | Every control present; 104 / 184 / 284 px leading lane; 108 px unwrapped icon run; real-browser probe |
| Phone same widths | 44 px targets, full-width wrapped chip lane, no overflow; probe |
| Long/many chips, long scope | Whole chips wrap; text ellipsizes without swallowing cycle/dismiss; bounded scroll; probe |
| Icon actions | Role + verbatim name and tooltip equality: Add a tag filter, Changed by agent, Pinned only, Hide service files, Save as space, Clear search |
| Three toggles | aria-pressed switches, name stays unchanged; component tests |
| Eye alone / clear filters / Escape | No Save as space, preference untouched; store/component tests |
| Query then pinned then agent then tag then scope + Escape | Existing walk, one action per press; component test |
| Indexing → words → meaning | Loader → Search → Sparkles; description/status follow VM; component test |
| Empty sentence, ready meaning | Status node absent; no retired posture caption |
| Clear and searchRef | Query clears without blur; supplied ref focuses same searchbox |
| Phone requestSearchFocus | Shared searchbox takes focus; phone component test |
| Hidden count 0 / 1 / many, capped selection | Existing countLabel grammar plus hidden clause only above zero; phone test |
| Hydration races click / rejected write | Late read never undoes choice; failure reverts to acknowledged choice and reports error |
| Live index changes with hiding on | Re-read filtered window, do not inject hidden rows; hook regression |

</intent-contract>

## Code Map

- `src/components/notes/note-filter-bar.tsx:119-165` — unchanged tag chip; `:168-359` — bar rebuilt.
- `src/components/ui/tooltip.tsx:61-83` — IconHint; `src/components/layout/icon-hints.test.ts:49-81` — sweep convention (the task's settings path was stale).
- `src/lib/stores/notes-filters.ts:184-295,342-365` — filter state, Escape, query composition.
- `src/components/notes/notes-pane.tsx:178-194,466-498,635-645` — mount, rail and count.
- `src/components/notes/notes-phone-pane.tsx:279-309` — retired hand-rolled search and scan strip.
- `src/lib/stores/notes-list.ts:39-97` — count mirror; ownership extended by Main via hub.
- `src/hooks/use-notes-changes.ts` — query key and filtered refresh; ownership extended by Main via hub.
- `dev/probe/main.tsx:1-56`, `dev/probe/capture.tsx:27-48` — real-module harness precedents.

## Tasks & Acceptance

Acceptance, verbatim from the epic: at 240 px every control is present, no element is narrower than its content, nothing overflows the column, the chips wrap and the icon run does not (real-browser measurement in `dev/probe`, the numbers in the spec); every icon button is found by `getByRole("button", { name })` with the same name it had as text, toggles report `pressed`, and each tooltip's text equals the name (NFR-69, asserted; targets measured ≥ 24 px desktop / ≥ 44 px phone in the probe); the state glyph shows *indexing* while `NoteIndexProgressVm` says so and *words* after; no caption is rendered with an empty status; the phone's search field is the bar's, focused by `requestSearchFocus` as before, with the scan strip's job done by the glyph; `Save as space` still names the space from the chips (`notes-pane.tsx:371-385`); the Esc walk (`:232-244`) is unchanged.

## Design Notes

UX-DR94/97/98/99 govern. Frozen implementation contract supersedes the epic's transitional index VM: `NoteSearchStateVm` supplies all three glyph phases in this wave. The scope/tag lane uses descendant sizing only here, keeping FilterChip and TagFilterChip untouched for their other consumers. On phone named grid areas give scope a leading full-width row and chips a trailing full-width row without changing DOM order. Off controls use muted foreground; pressed controls use existing accent pair; no new palette.

Research: `research-notes-search-2026-09-19.md` §2.2 grounds the retired caption's mismatch with the old index-only list path; the UX spine owns exact copy and geometry.

- Review: status sentences remain unabridged (CSS truncates only to available width); `words` with incomplete embedding counts reads “Indexing meaning n/N chunks” while retaining the Words glyph; search-read errors take precedence.
- Buttons use `icon-xs` on desktop; the shared search ref is memoised and Settings closes disconnect its pending focus observer.
- Width-blind search-row tag overflow remains the UX-DR95-permitted deliberate simplification; focus-ring clipping was inferred rather than measured, so no unverified geometry change is included.
- Search-status subscription failure retains its existing action alert; automatic retry/clearing is deliberately not added in this scoped pass (a subscription retry policy is outside the assigned changes).

## Measured in a real browser

**Predicted; measured numbers pending.** Desktop: 24 px insets + leading lane + 4 px gap + 108 px run. At 240 / 320 / 420, leading lane is 104 / 184 / 284. Every icon target is 24 × 24. Phone: 16 px insets + 44 px chooser + flexible spacer + 176 px run; minimum 236 px, leaving 4 / 84 / 184 px spacer at 240 / 320 / 420. Chip lane is 224 / 304 / 404 px; every action/clear target is 44 × 44. Actual probe and command are recorded in Verification when authored.

## Verification

Main subsequently permitted targeted vitest runs. The seven-file scoped command
below passed **113 tests** before mutation. The mutation `savable = !hideServiceFiles || …`
made `keeps the eye out of Save and the Escape walk` fail at the role/name assertion:
`expected <button aria-label=\"Save as space\">…</button> to be null`, exit 1.
Restoration returned the exact pre-mutation source snapshot; the restored run is
recorded below. The existing origin/pinned role+name tests were not changed.

```sh
bunx vitest run src/components/notes/note-filter-bar.test.tsx src/lib/stores/notes-filters.test.ts src/lib/stores/notes-list.test.ts src/lib/stores/notes-search-state.test.ts src/hooks/use-notes-changes.test.ts src/components/notes/notes-pane.test.tsx src/components/notes/notes-phone-pane.test.tsx
```

### Browser command (coordinator runs on the Mac)

Serve this worktree with `bun x vite --host 0.0.0.0 --port 8133`; start the
existing collector separately with `bun run dev/probe/collector.ts`. Services
are supervised with hub when run by the harness. Then:

```sh
PROBE_ENTRY=notes-filter.html bash dev/probe/measure.sh notes-desktop 'tier=desktop&chips=long&phase=meaning' 240 320 420
PROBE_ENTRY=notes-filter.html bash dev/probe/measure.sh notes-phone 'tier=phone&chips=long&phase=meaning' 240 320 420
```

Repeat with `chips=none|many`, `phase=indexing|words|refused` and `theme=dark`
for the state matrix. The entry fixes the **allocated bar width**, independent
of Chrome's minimum viewport width. It mounts the real `NoteFilterBar` with
the shipped CSS and mock-shell vocabulary. Its beacon and `#PROBE` report every
control rectangle and pressed state, lane/action rectangles, wrapped chip
rectangles, under-target controls, horizontal overflow, real save activation,
toggle presses, searchRef focus and clear-retains-focus. `overflow=[]`,
`underTarget=[]`, identical action-row y coordinates and true gesture checks
are the expected results, not claimed measurements.

No browser is available to this lane. Geometry remains **predicted; measured
numbers pending**. Full formatting/lint/typecheck, generated bindings, the
project-wide test gate and all Rust/macOS integration remain coordinator-owned.
The folder-scope exception is explicitly waived as DW-266 (spec-76-4).


Review-fix verification: `bunx vitest run` over use-notes-changes, note-filter-bar, search-settings, editor/search-marks, note-editor, note-row, notes-filters, notes-list, notes-pane and notes-phone-pane: **10 files, 178 tests passed**. Reverting the rejected-read failure handler made the new search-mode regression fail with `expected null to be 'Search failed.'`; restoration returned the original source snapshot and the final suite passed. A throwaway IPC smoke also exercised every new dev/mock-shell fixture including settings readback and both channel batches: **1 passed**, then removed. This pass proves component behavior and mock IPC, not browser layout or the native shell; no visual verification was performed.
**Restored final scoped run:** 7 test files, **114 passed**, exit 0 (2026-09-19).
This includes waiting for preference hydration before claiming the count. The
probe driver also passed `sh -n dev/probe/measure.sh`; this checks shell syntax,
not browser layout. No temporary mutation or throwaway script remains.
