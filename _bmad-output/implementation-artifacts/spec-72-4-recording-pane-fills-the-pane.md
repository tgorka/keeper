---
title: 'The Recording pane fills the pane'
type: 'bugfix'
created: '2026-09-16'
status: review
baseline_revision: a8eb9c2
final_revision: ''
---

<intent-contract>

## Intent

**Problem:** The Recording body inherits UX-DR29's centered 720px conversation measure, leaving its cards narrow inside a wide desktop pane. UX-DR93 explicitly retires that measure for this body.

**Approach:** Remove `mx-auto max-w-[720px]` from the body, retaining `w-full`, `gap-6`, and `p-6`. Keep the cards full width; bound only metadata field rows where the wider body would otherwise stretch text inputs.

**Always:** Keep the header, scroll behavior, card order, form behavior, and all other panes unchanged. Apply any necessary readability measure inside a card, never to the body.

**Block If:** A necessary fix crosses the assigned Recording files; coordinate ownership before editing.

**Never:** Change the phone tier, the conversation measure, backend policy, tooltips, Settings, Sync, or Files.

## I/O & Edge-Case Matrix

| Input / state | Expected result | Verification |
| --- | --- | --- |
| Wide desktop pane | Body fills available width with unchanged 24px padding and gaps; cards span its inner width | One rendered class-drift guard; coordinator's 1500px Mac screenshot |
| Metadata fields, including an added custom row | Form card spans the pane; label/input rows have a local measure and remain left-aligned | Same guard checks the field-row measure and full-width card |
| Narrow pane below 720px | Removing the old max-width changes no body width; local field measure yields to available space | CSS chain inspection; screenshot must check no overflow |
| Header | Existing header geometry and Start action remain unchanged | Same guard checks existing header classes |
| Phone tier | No Recording surface mounts | Read the exhaustive phone-shell surface branches |

</intent-contract>

## Code Map

- `src/components/layout/recording-pane.tsx:201,309-312` — full-width header and constrained body.
- `src/components/recording/recording-meta-fields.tsx:71-171` — four unrestricted text field rows and repeatable custom pairs, shared by next-session and completion editors.
- `src/components/recording/recording-meta-card.tsx:41-55` — full-width card and content; no change planned.
- `src/components/ui/input.tsx:12` — inputs fill their parent (`w-full`).
- `src/components/ui/card.tsx:15,68` — small-card padding is 16px each side.
- `src/components/layout/settings-pane.tsx:35-36`, `files-pane.tsx:3262-3263` — full-width comparison bodies, read-only.
- `src/components/layout/sync-pane.tsx:876-882` — precedent for a measure local to a form rather than its body, read-only.
- `src/components/layout/phone-shell.tsx:934-985` — exhaustive phone surface routing, no Recording.
- `src/components/notes/note-column-width.test.ts:18-21` — class-drift guard precedent; not a claim that jsdom measures layout.

## Tasks & Acceptance

- [x] Remove the body's centering and width cap without changing its padding or gap.
- [x] Keep form-field measures local and record their exact rationale.
- [x] Add one rendered drift guard and prove it fails against the old body and unbounded fields.
- [x] Run `bunx vitest run src/components/layout/recording-pane.test.tsx` and record exact counts.
- [x] Confirm phone routing and the below-720px constraint behavior by inspection.
- [ ] Coordinator captures the 1500px Mac screenshot.

**Acceptance:** a test proves the recording body has no `max-w-*`/`mx-auto` pair while the header and Settings/Files stay as they are; a screenshot at 1500 px on the Mac shows the card and the form spanning the pane.

## Design Notes

The body is now `flex w-full flex-col gap-6 p-6`. All card backgrounds and headers span the body, retaining the existing vertical rhythm and 24px body padding. There is no new grid, card rearrangement, or centered reading column.

The one new measure is **640px on each metadata field row**: Title, Participants, Program / session note, Tags, and each custom name/value pair. This is the previous input budget: 720px body minus 48px body padding minus 32px small-card padding. Keeping it on the existing row wrappers preserves both label alignment and custom-pair proportions, without limiting the form card itself. `w-full` lets each row shrink below that ceiling. The shared field component also protects the completion card's details editor. No extra wrapper or duplicated field implementation was introduced.

This is a code-derived wide-layout judgment, not a claimed screenshot observation: the shared `Input` fills its parent, so removing the body's cap without a local measure would expand these text inputs to the pane width minus padding. At a 1500px pane that is 1420px rather than the former 640px. The coordinator must validate the actual app's available pane width and visual result on the Mac.

Existing measures elsewhere are unchanged: Destination's path/template text inputs remain `w-64`, its folder select remains `w-48`, and Segmenting's numeric inputs remain `w-24`. Status cards, permission rows, source lists, microphone/camera selectors and the rest of the card surfaces retain full available width. No changes to metadata card markup, header, Settings, Files, Sync, phone routing, colors, focus behavior, loading/error handling, or backend state.

One new test guards the rendered body/card/header class contract and the four metadata rows plus an interactively added custom row. No existing test was deleted or reworded. It deliberately tests classes because the requested regression is class drift; it does not claim jsdom establishes layout.

## Verification

**Commands run by this slice:**

- `jq -r '.text' /tmp/triage/ScoutRecordingWidth.md` — decoded the JSON-wrapped triage; its full decoded output was read.
- `bunx vitest run src/components/layout/recording-pane.test.tsx` — initial attempt failed before tests because this worktree had no frontend dependencies (`@vitejs/plugin-react` / `vitest/config` unavailable). The coordinator subsequently hydrated dependencies; this slice did not install packages or edit dependency files.
- The same command, after implementation and dependency hydration — **1 file passed, 43 tests passed**.
- The same command with only the old `mx-auto ... max-w-[720px]` body chain restored — **1 file failed, 42 tests passed, 1 failed**. The new guard failed at `recording-pane.test.tsx:353`: expected the body class not to match the centering/maximum-width regex; received `mx-auto flex w-full max-w-[720px] flex-col gap-6 p-6`.
- The same command with the full-width body restored but the Title row's local measure removed — **1 file failed, 42 tests passed, 1 failed**. The new guard failed at `recording-pane.test.tsx:357`: expected `w-full max-w-[640px]`, received `flex flex-col gap-1.5`.
- The same command after both mutations were restored — **1 file passed, 43 tests passed**, exit 0.
- `bunx vitest run src/components/recording/recording-meta-card.test.tsx` — **1 file passed, 7 tests passed**, exit 0. Existing field editing/refill behavior remains covered.

**Inspection:** `phone-shell.tsx:934-985` renders an exhaustive surface branch with no Recording pane, and `src/lib/phone-surfaces.ts:29-36,51-65` contains no recording surface and returns `null` for unsupported primary views. Phone routing is untouched. Below 720px of available pane width the former `w-full max-w-[720px]` had no limiting effect; removing it leaves the body at the same width, and a below-720px body gives the field rows less than 640px after its existing padding, so their new ceiling is inert too.

The Recording header and the comparison Settings/Files files were read and left unchanged. No Rust changed; no Rust build or binding regeneration was needed. No repo-wide gates, formatters, or git operations were run by this slice.

**Still owed by the coordinator:** a **1500px Mac screenshot**, with computed body/card/field widths if available. It must show the Recording cards spanning the available pane inside the unchanged 24px body padding, the Next session form card spanning that same width, metadata inputs left-aligned at no more than 640px rather than stretched across the window, the custom name/value row fitting that measure, and the unchanged full-width header. Inspect the completed-session details editor as well because it uses the same measured fields. At a narrower desktop width the fields must shrink without horizontal clipping. The class guard and passing jsdom suite do not establish pixel geometry, so this spec remains in progress until the coordinator records that visual acceptance.

## Shipped in

PR #362 of stack #364 (epic 72), branch `epic72/surfaces`. The macOS gate (`bun run check:rust:macos`) passed on hesperia over the stack tip, which is where the `keeper` shell crate compiles at all.
