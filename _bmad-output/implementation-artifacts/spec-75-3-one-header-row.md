---
status: in-progress
baseline_revision: 16f7912
final_revision: ''
---

# Story 75.3 — One header row

<intent-contract>

## Problem

The window draws two stacked headers: `capture-window.tsx:119` hand-rolls a 32 px strip (the conditional drag region, DW-199's Rust-measured corner inset, and pin / lock / close), and `NoteEditor` mounts the shared 40 px `PaneHeader` underneath it with the identity, the save caption and `PriorityActions` + `…`. Seventy-two pixels of chrome above a capture panel whose default height is 340.

The mechanism to merge them already exists and is unused here: `PaneHeader` carries Story 50.1's **frame group**, built for window controls inside a surface header, and `NoteEditor`'s `frame` prop is documented as "absent for capture hosts" (`note-editor.tsx:342-351`).

## Approach

Pass the chrome as the frame group. Do not invent a second header.

## Always

The drag region and the corner inset travel with the close button: they are properties of the window, not of the strip that used to hold them.

## Block If

The row cannot hold everything: the **status caption** gives first (transient, and repeated in the `…` menu), the identity title truncates second.

## Never

Never drop an action to make room — a control that disappears at a width is a control nobody can rely on. Never move `CAPTURE_MIN_SIZE` silently: if the arithmetic says the floor must rise, it rises in this change with its derivation comment (`capture.rs:274-289`) rewritten, not left describing a two-row header that no longer exists.

## I/O and edge-case matrix

| Width | What the row shows |
| --- | --- |
| 560 px (`CAPTURE_DEFAULT_SIZE`) | everything; the scout's arithmetic leaves ~112 px of title |
| the minimum, whatever it ends as | title (truncated) + three actions + three window controls; caption dropped |
| a title long enough to overflow | truncates; never pushes an action out |

</intent-contract>

## Code Map

- `src/components/capture/capture-window.tsx:119-227` — `CaptureWindowChrome`: the `h-8` strip, the conditional `data-tauri-drag-region` (`:167`), the DW-199 inset (`:184-187`), the three buttons (`:198-227`).
- `src/components/capture/capture-document.tsx:205` — the chrome slot; `:66-79` — the real `NoteEditor`.
- `src/components/notes/note-editor.tsx:969` — the `PaneHeader` mount; `:342-351` — the `frame` prop's doc.
- `src/components/layout/pane-header.tsx:368-380` — the frame group; `:170-227` — `paneHeaderActionsBudget`; `:150` — `PANE_HEADER_IDENTITY_MIN_PX = 160`; `:323` — the `h-10` row.
- Tests that pin the current structure and must move rather than be deleted: `capture-window.test.tsx:189` (close-last / DW-199), `:209` (conditional drag region), `:224` (inset), `capture-document.test.tsx:581` (the chrome slot's dismissal contract), `note-editor.test.tsx:721,740,764` (the frame group).
- `keeper-core/src/capture.rs:272` `CAPTURE_DEFAULT_SIZE = (560, 340)`, `:289` `CAPTURE_MIN_SIZE = (320, 240)` and the header-budget derivation at `:274-289`.

## Tasks & Acceptance

Acceptance, verbatim from the epic: *one header row holds title, save caption, the three document actions and the three window controls, measured in a real browser at the default 560 px width and at whatever the minimum width ends up being — no element narrower than its own content, nothing outside the window, no second row; the close button still sits clear of the platform's corner; dragging the header still moves the window where it did; the four structural tests named in the triage pass against the merged header.*

## Measured in a real browser

The acceptance sentence above asks for a real layout engine, because jsdom lays
nothing out and every number in the Code Map is arithmetic until one runs. The
harness is `dev/probe/capture.html` + `dev/probe/capture.tsx` — the real
`CaptureNoteWindow` over `dev/mock-shell.ts` — served by this repo's own vite and
driven in Chrome 154 on the macOS host over tunnelled CDP (the dev container has
no runnable browser). Widths are `page.setViewport`, so they are the window's,
not a stylesheet's.

| viewport | headers | row | identity | caption | actions | frame | overflow |
| --- | --- | --- | --- | --- | --- | --- | --- |
| 560 × 340 (`CAPTURE_DEFAULT_SIZE`) | 1 | 40 px | 195 | 125 | 104 | 88 | 0 |
| 400 × 240 (`CAPTURE_MIN_SIZE`) | 1 | 40 px | 160 | 0 | 104 | 88 | 0 |
| 400 × 240, write refused | 1 | 40 px | 35 | 125 | 104 | 88 | 0 |

Four facts the table settles:

1. **One row, 40 px, at every width** — the second header is gone rather than
   hidden, and `document.documentElement.scrollWidth` never exceeds the window,
   so nothing is outside it and nothing is clipped.
2. **The actions group is 104 px, not the 112 the first derivation assumed.**
   `PriorityActions` renders its leading pair with no gap between them, so
   attach + properties is 64 and not 72. `CAPTURE_MIN_SIZE`'s ledger said 112
   and summed to 408 while claiming 400; with the measured 104 the ledger sums
   to the constant it justifies. The constant itself was right.
3. **At the floor the caption gives and the identity stops at exactly its
   160 px floor** — 12 + 160 + 8 + 0 + 8 + 104 + 8 + 88 + 12 = 400, the row
   AD-260 predicted, with the close button's full 12 px gutter intact.
4. **A refused write reverses that trade, and fits.** AD-260 justified squeezing
   the caption on the premise that the save state is "repeated in the … menu";
   it is not, and this caption is the only place a capture window says why a
   write was refused (UX-DR35). So `PaneHeaderStatus.unsqueezable` keeps its box
   and releases the identity floor instead: 12 + 0 + 8 + 125 + 8 + 104 + 8 + 88
   + 12 = 365 of demands, 35 px left for the title, no overflow. Screenshot at
   2× confirms the reason on screen, the title as `U…`, and all six controls
   present.

What it does not settle: below roughly 490 px the sentence is ellipsised to its
slot (125 px of 685 measured), readable in full only on the slot's `title`. If
the owner wants the whole sentence at every width, the save state has to go into
the `…` menu — which is what AD-260 assumed already existed, and is a product
decision rather than a layout one.
