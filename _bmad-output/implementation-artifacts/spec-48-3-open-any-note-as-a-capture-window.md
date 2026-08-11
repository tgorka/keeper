# Spec 48.3 — Open any note as a capture window

story: 48.3
status: implemented, one gate check unverifiable on this box
branch: `work/epic-48` (off `chore/the-epic-46-tail`)
binds: FR-191 (completes Story 45.15); touches FR-192 only by reading it
sentinel: `MUT48-3`

## What the defect actually was

The owner of 0.8.1 reported two things:

> *nie mozna miec wiecej niz jednej notatki*
> *nie widze tez mozliwosci otworzenia istniejacych notatek jak quick capture*

**They are one defect, and it is one missing line.** Story 45.15 built the whole
chain and mounted none of it:

| Link | State before 48.3 |
|---|---|
| `CaptureNoteItem` (`capture-note-item.tsx`) | complete, correct, **rendered nowhere** |
| `openNoteAsCapture` (`use-notes-actions.ts:127`) | complete |
| `openCaptureWindow` → `notesCaptureOpen` (`capture-windows.ts:95`) | complete |
| `notes_capture_open` → `notes_window::open` (`notes_window.rs:225`) | complete, one window label per note |
| the item in a menu a person can open | **absent** |

`grep` for `CaptureNoteItem` across the repository returned two files: the
component, and its own test. For three epics its only importer was a test.

So the **only** capture window obtainable was the statically declared prewarmed
draft — one window, holding one note. That is precisely "I cannot have more than
one note" *and* "I see no way to open an existing note like quick capture", and
they are the same sentence said twice.

Nothing was ever removed. It never arrived. 45.15 listed the component as a
deliverable, never named `note-editor.tsx` among its touched files, and signed
off gate check 2 — *"open a note's actions menu → Open in a capture window"* —
which **could not have passed on any build 45.15 produced.**

### The fix

One import and one line in `note-editor.tsx`:

```tsx
import { CaptureNoteItem } from "@/components/capture/capture-note-item";
…
<CaptureNoteItem vaultId={vaultId} noteId={noteId} />
```

Everything else in this story is the part that stops it happening again, plus
three questions the brief asked me to answer deliberately rather than by
accident.

### Where in the menu, and why not where 45.15 said

45.15's own docblock proposed *"beside Export and above Delete"*. That was a
description of a menu which then held Export and Delete and nothing else. Story
46.5 filled the menu and gave its separator a **meaning**: above it changes what
is *shown*, below it *acts on the note* (`note-editor.tsx:704-706`).

A capture window only shows the note — it is a way of looking at one, not a thing
done to one. So the item sits **above** the separator, between History and Show in
Files, and the group now reads by distance from this pane:

```
Attachments → Properties → History → [Show in Files] → Open in a capture window
   panel        panel      this pane's    out of            another keeper
                             own mode      keeper              window
──────────────────────────── separator ────────────────────────────
Export… ─ Delete note
```

48.3 first placed it between History and Show in Files, reading the group as
"distance from this pane". E48Header's 48.5 landed on top of that and moved it to
**last among the things that show the note**, with a better reason: the four
above it are now a variable-length list whose length the window width decides, so
an item interleaved into it would need a second ordering rule for the widths at
which its neighbours are not rendered. Kept, and the comment in `note-editor.tsx`
says so rather than still claiming the old position.

What this story asserts is therefore the *relative-order* invariant — History <
capture item < Export < Delete — and not an index. That is what let 48.5
restructure the whole header, introduce `PriorityActions` and a render-prop
`menu`, and move this item, without a red; and it is what would go red if the
item ever crossed to the acts side of the separator. Agreed with E48Header on
`hub` before either of us edited the file, and re-verified on their tree
afterwards (mutation A2).

### Question: what happens when the note is already open in a capture window?

**Answer: the item is still offered, with the same label, and the press still
reaches Rust, which raises and focuses the window that is there.** There is no
second label, no disabled state, and no read of the window mirror. Reasons, in
order of weight:

1. **`notes_window::open` is idempotent by identity, not by a flag.** It derives
   one window label per note from `capture_key` and reuses a window only on an
   exact label match (`notes_window.rs:225-284`). One key is one window however
   many times it is asked for; two keys are two windows. Nothing in the UI needs
   to know which case it is in.
2. **The label already names the outcome, not the creation.** "Open in a capture
   window" is true whether the window was built or raised. A second label would
   be a second name for one verb.
3. **A state-dependent label would have to come from a mirror the main window
   does not keep.** `hydrateCaptureWindows`/`listenNotesCaptureWindows` are
   called from exactly one place — `capture-window.tsx`'s chrome effect — which
   only runs inside a capture window's own document. In the main window
   `captureWindowsStore.windows` is `null` and `captureWindowFor` answers `null`
   for every key. A label fed by a mirror that misses a window closed from the OS
   would lie about which of two things a press does, and one honest label beats
   two labels of which one is sometimes wrong.
4. **The failure mode of the "obvious improvement" is silence.** Reading the
   mirror and refusing the second press would replace a raise-and-focus with
   nothing at all. `asks again for a note whose window is already open, under the
   one key` exists to make that regression red, and seeds the mirror as if the
   window were open so it is testing the decision and not the empty state.

`capture-windows.ts`'s module doc claimed for three epics that *"the main window
renders the list (so 'open this note as a capture window' can raise the one that
is already there instead of asking for a second)"*. **It never did.** That doc is
corrected in place rather than deleted, because a doc describing a consumer
nobody had written is most of why 45.15 read as finished. Reported as a DW below.

### Question: does the store or `openCaptureWindow` collapse two notes?

**No, and it is now proved rather than assumed.** `openCaptureWindow` forwards
the target and re-reads; `captureWindowsStore` keys nothing by target;
`captureKey` encodes both components (`capture-target.ts:55-60`) so `a`+`b/c` and
`a/b`+`c` cannot alias. Two notes produce `note:v/n1` and `note:v/n2`, two
labels, two windows. Mutation **E** replaces the forwarded target with a
hard-coded draft and kills six named tests across three files.

### Question: 46.12's two views of one note

**Confirmed by reading and by mutation, not assumed.** `openNoteDocument`
increments `views` and *joins* an existing document rather than blanking it;
`dropNoteDocument` decrements and only removes the last view
(`notes-editor.ts:280-323`). Two panels on one note are two views of one buffer.
`keeps one note one window, however many panels are showing it` asserts
`views === 2` and that both panels' items produce the **same** key — so the second
panel raises the first's window instead of putting a rival webview over one
buffer. Mutation **G** breaks the join and that test is the only one in the sweep
filter that notices.

Across *windows* the refcount does not and cannot apply: a capture window is a
separate webview with its own module registry, so a note open in a panel and in a
capture window is two documents in two stores, each with its own `notes_open`
subscription. That degrades to the existing diff/conflict machinery
(`NoteDiffBar`, `notesResolveConflict`), which is 45.15's pre-existing property
and not something this story changes. Reported as a DW.

### What was NOT done, deliberately

- **No main-window subscription to `keeper://notes-capture-windows`.** See
  reason 3 above. The only thing it could buy is a cosmetic label, and it would
  buy it with a mirror that goes stale exactly when it matters. E48Top confirmed
  on `hub` that 48.4's always-on-top toggle lives in the capture window's own
  chrome and does not need it either.
- **No "which window am I?" gate on the item.** It renders in every host that
  mounts `NoteEditor`, including `capture-document.tsx` — i.e. inside a capture
  window. That is deliberate and both readings are true: from a note's capture
  window the verb raises the window you are in, and from the prewarmed draft
  panel it *promotes* the quick note into a window of its own that Escape does
  not hide. Suppressing it would need a third spelling of "which window am I"
  after Rust's label and `captureTargetFromSearch`, to remove an affordance that
  cannot harm anyone.
- **No Rust touched.** Not one line, in any crate. The chain below
  `notesCaptureOpen` already worked; the hole was above it.
- **No new command, no `client.ts`, no `lib.rs`, no `tauri.conf.json`.** Cleared
  with E48Top, who is appending `notes_capture_set_always_on_top` to all four.
- **`capture-note-item.test.tsx` was not rewritten.** Every assertion in it was
  and is true. Its problem is not what it asserts, it is what it *cannot see* —
  and the fix for that is a different file, not a different assertion.
- **No `capture-windows.test.ts` edits.** Its three `CaptureWindowVm` literals
  are annotated rather than cast and will break when 48.4 makes `alwaysOnTop`
  required; flagged to E48Top on `hub` and he took them.
- **The seeded window row in the new test is not annotated `CaptureWindowVm`.**
  Only `key` is load-bearing, and annotating it would tie this story's PR to
  whichever epic-48 story next adds a field to the generated type — a commit that
  only compiles at the tip of the stack.
- **`note-editor.tsx`'s header structure, `note-actions.tsx` and
  `pane-header.tsx` are E48Header's (48.5).** I own what goes *in* the menu.

## I/O matrix

### The item, in the real `NoteEditor` header

| State | Actions menu contains | Press → `notesCaptureOpen` |
|---|---|---|
| `capabilities.notes === true` | `Open in a capture window`, above the separator | `{kind:"note", vaultId, noteId}` — the header's own ids |
| `capabilities.notes === false` | item **absent**; siblings unaffected | never called |
| `noteId === null` | no header at all (the no-note branch returns first) | — |
| two panels, two notes | one item each, named per panel | two calls, `note:v/n1` and `note:v/n2` |
| two panels, one note | one item each | two calls, both `note:v/n9` |
| note already in a capture window | item unchanged, still offered | called again, same key → Rust raises and focuses |
| host is a capture window (`capture.html`) | item present | same key as this window → raises itself; from the draft panel, promotes the draft's note |

### `captureKey`, which is what decides how many windows exist

| Target | Key | Window label |
|---|---|---|
| `{kind:"draft"}` | `draft` | the statically declared `quick-capture` |
| `{kind:"note", v, n1}` | `note:v/n1` | `quick-capture-<hash>` |
| `{kind:"note", v, n2}` | `note:v/n2` | a **different** `quick-capture-<hash>` |
| `{kind:"note", "a", "b/c"}` vs `{kind:"note", "a/b", "c"}` | `note:a/b%2Fc` vs `note:a%2Fb/c` | different — the `encodeURIComponent` is load-bearing |

Unchanged by this story; tabulated because "two notes give two windows" is a
claim about this table and nothing else.

## Edge cases

- **A component test cannot see that nothing mounts the component.** This is the
  whole story. `capture-note-item.test.tsx` mounts `CaptureNoteItem` inside a real
  Radix menu and asserts the call, the arguments, per-instance props and the
  capability gate — four honest assertions over a component in no production
  tree. The only test that can see the hole is one that mounts the **real**
  `NoteEditor` and presses the item through the **real** trigger, which is what
  the new file does and what `export-in-the-note-editor.test.tsx` (45.21) already
  did one epic earlier for the same reason.
- **The gate test is the one mutation A cannot kill.** `is absent where a capture
  window cannot exist` passes with the line deleted — correctly, because the item
  is absent either way. Recorded because it is exactly the shape of assertion
  that let 45.15 through: a green absence test over an absent feature. It is
  killed by mutation B instead, and it asserts a *sibling is present* from an open
  menu so `null` cannot mean "the menu never opened".
- **Radix opens on pointer, not click.** Every trigger press is the house pair,
  `pointerDown {button:0, ctrlKey:false}` + `pointerUp {button:0}`, as at
  `note-actions.test.tsx:107` and `note-file-links.test.tsx:207`.
- **Radix unmounts the menu on its own schedule.** The two-panels test presses two
  menus in sequence, so between them it waits for `queryAllByRole("menu")` to be
  empty. Without that the second `findByRole("menu")` can resolve to the *closing*
  first menu, which reads as the second panel's item firing when it was the
  first's, twice — a green test asserting nothing.
- **Two editors on one note have the same accessible trigger name.** The trigger
  carries the note's *title*, so two panels on one note give two identically named
  buttons and `findByRole` throws on the ambiguity. The two-panels test uses
  `findAllByRole` and asserts a length of 2; the two-*notes* test gives each note a
  different body so the titles, and therefore the triggers, are distinguishable —
  which is what makes "each menu opened its own note" checkable at all.
- **`DEFAULT_CAPABILITIES.notes` is `false`.** So in every editor-mounting test
  that does not opt in, the item is absent and no existing assertion moves. That
  is why 44 files and 793 tests were green before this story and stayed green:
  the only suite that sees the item is one that sets the flag. Confirmed the hard
  way by mutation B, which forces the gate open and immediately breaks
  `note-actions.test.tsx > still offers every verb the header used to carry`.
- **`bun run test src/components/notes/` is shared ground this wave.** A run
  mid-way through showed 27 reds in `notes-pane.test.tsx`; the file passed in
  isolation, `notes-pane.tsx` was mid-edit by E48Columns, and `notes-pane.test.tsx`
  mocks `NoteEditor` outright so nothing in this story can reach it. It went green
  again on its own. The mutation sweep therefore used a filter with no
  peer-owned production file in it — see below.

## Mutation table

Baseline established **green in the same command and the same filter as the
sweep, before the sweep**: `bun run test src/components/capture/
src/components/export/ src/lib/stores/capture-windows.test.ts
src/components/notes/note-actions.test.tsx
src/components/notes/note-editor.test.tsx
src/components/notes/note-file-links.test.tsx` → **11 files, 101 tests, EXIT=0**
(102 after the sixth test was added).

Each mutant: `cp` to a byte backup, `sed`, run, restore from the backup, and
**`md5sum` before vs after compared** — printed per row as `restored
byte-identical`, and the harness aborts a row whose `sed` changed nothing (a
mutation that did not apply proves nothing). `git diff` is blind to the new test
file, so that file was checked by name.

| # | Mutation | Caught by (named) | Result |
|---|---|---|---|
| **A** | **the rendered `<CaptureNoteItem/>` line deleted — *this is the story*** | `offers Open in a capture window…`, `gives two notes two windows…`, `asks again for a note whose window is already open…`, `sits with what shows the note…` | ✅ 4 failed |
| B | `if (!notes)` → `if (false)` — the capability gate always passes | `is absent where a capture window cannot exist, proved from an open menu`, `capture-note-item.test.tsx > is absent where a capture window cannot exist`, `note-actions.test.tsx > still offers every verb the header used to carry` | ✅ 3 failed |
| C | `noteId={noteId}` → `noteId={vaultId}` — the item carries the wrong id | `offers Open in a capture window…`, `gives two notes two windows…`, `asks again…` | ✅ 3 failed |
| D | the item moved *below* the separator, after `ExportNoteItem` | `sits with what shows the note, above what acts on it` (and nothing else — the invariant is isolated) | ✅ 1 failed |
| E | `notesCaptureOpen(target)` → `notesCaptureOpen({kind:"draft"})` — the store collapses every target | `offers Open…`, `gives two notes two windows…`, `asks again…`, `capture-note-item.test.tsx > opens the note it was given`, `…> carries the note it is mounted on`, `capture-windows.test.ts > opens the window for the target it was given, then re-reads` | ✅ 6 failed |
| F | mutation A re-run after the sixth test landed | the four from A **plus** `keeps one note one window, however many panels are showing it` | ✅ 5 failed |
| G | `views: held.views + 1` → `views: held.views` — 46.12's refcount stops joining | `keeps one note one window, however many panels are showing it` — **the only test in the filter that notices** | ✅ 1 failed |
| **A2** | **mutation A re-run against E48Header's restructured header — `PriorityActions`, a render-prop `menu`, a variable-length `headerActions.map`, and my item moved to last among the shows** | the same five as F, on the tree that actually ships | ✅ 5 failed |

**8 mutants, 8 caught, 0 survived.** Every restore verified byte-identical by
`md5sum` — and, because `git diff` is blind to a file it has never seen, the new
test file was verified by name and the three restored production files were
re-read through `grep` after the sweep rather than trusted to a remembered edit.

Both shared files were broadcast before being mutated: `note-editor.tsx` to
E48Header and `capture-windows.ts` to E48Top, each of whom held their edits until
I said the window had closed. A2 exists because that discipline is not enough on
its own: E48Header's 48.5 restructure of the same file landed *between* my sweep
and my final run, so the tree mutation A was scored against is not the tree that
ships, and a proof of the story's one line on a superseded structure is not a
proof. A2 re-scores it on theirs. It also independently confirmed E48Header's
claim that my item is invisible to `note-editor.test.tsx` — deleting my line
failed nothing of theirs, because `DEFAULT_CAPABILITIES.notes` is `false` in that
suite — so neither sweep could score a kill against the other's mutant.

## What I could not verify here, and why

**The `keeper` shell crate does not build on Linux** (no GTK/webkit), so
`cargo build/check/clippy/test -p keeper` was never run. I touched **no Rust at
all**, in any crate, so nothing in this story needs it — but the claim the story
is about ("two windows appear") is a claim about a compositor, and no test in this
repository can observe a window. `notes_window::open`'s reuse-by-label logic was
read, not compiled.

What *is* verified here is the whole chain from a rendered menu item to the
argument handed to `notes_capture_open`, and the key that argument produces. That
is the boundary Rust is on the other side of, and it is where every mutation
above was caught.

Gate checks, in order, on the Mac:

1. **Open a note in the notes pane → press `Actions`.** The menu reads:
   Attachments, Properties, History, **Open in a capture window**, [Show in
   Files], ─, Export…, ─, Delete note. *This is 45.15's gate check 2, and this is
   the first build on which it can pass.*
2. **Press it.** A 560×340 undecorated window appears holding that note, with the
   note's real text in it — not a blank draft.
3. **Now do it for a second, different note.** **Two windows on screen at once**,
   each holding its own note. This is the owner's headline report and the window
   itself cannot be checked on this box. What *can* be, and now has been, is
   everything either side of the compositor: the tests above prove two distinct
   keys reach Rust, and the `keeper-core` half — which compiles here — was run
   green by E48Lock during 48.2's sweep in this same worktree:
   `registry::tests::two_capture_windows_remember_two_placements_independently`
   = 1 passed / 0 failed, plus `cargo test -p keeper-core --lib capture::` =
   45/0. So two keys really do address two independent registry entries. The one
   link still read-but-not-compiled is `notes_window::open`'s reuse-by-label
   branch itself, which lives in the `keeper` shell crate and cannot be built on
   Linux by anyone this wave.
4. **Press it again on a note that already has a window.** The existing window is
   raised and focused. **No second window for that note.** If it is on another
   macOS Space, note what focusing actually does — that is the one place the
   decision above could read as "silently did nothing", and it is unobservable
   from Linux.
5. **From inside a capture window on note A, press `Actions` → the item.** The
   window you are in comes to the front. Nothing else happens.
6. **From the prewarmed draft panel (⌘⌥K / the configured chord), press it.** The
   draft's note gets a capture window of its own, which Escape does not hide. If
   this reads as confusing rather than as a promotion, it is one line to gate and
   the reasoning is recorded above.
7. **A build with sync off or on mobile**, where `capabilities.notes` is false:
   the item is absent from the menu, and the menu is otherwise intact.
8. **A note open in a panel *and* in a capture window.** Type in both, save both.
   Expect the diff bar / conflict path, not silent loss. Pre-existing 45.15
   behaviour, now reachable; DW below.

## Files changed

- `src/components/notes/note-editor.tsx` — **the story**: one import, one
  `<CaptureNoteItem/>` in the Actions children above the separator, and the
  comment recording why it is on that side of the rule. Nothing else; the header
  structure is E48Header's.
- `src/components/capture/capture-note-item.tsx` — docblock only, no behaviour.
  Corrects "beside Export and above Delete", records that the component went
  three epics unrendered and why a component test could not see it, and states
  the already-open decision and its reasons.
- `src/lib/stores/capture-windows.ts` — docblock only, no behaviour. Replaces the
  false "the main window renders the list" claim with what is actually true and
  why 48.3 left it that way.
- `src/components/capture/capture-in-the-note-editor.test.tsx` — **new.** Six
  tests over the real `NoteEditor` and the real trigger: the item exists and opens
  the header's own note; two notes give two distinct keys; one note gives one key
  from however many panels (with 46.12's refcount asserted); an already-open note
  is asked for again under the same key; the item is absent without
  `capabilities.notes`, proved from an open menu; and it sits on the shows side of
  the separator.
- `src/hooks/use-notes-actions.ts` — **unchanged.** `openNoteAsCapture` was
  already correct; it is listed because I own it and inspected it.

## Deferred work to land (numbers are Main's)

- **DW — the main window does not mirror the capture-window list.**
  `hydrateCaptureWindows`/`listenNotesCaptureWindows` run only inside a capture
  window's own document, so `captureWindowsStore` is `null` in the main window and
  `captureWindowFor` answers `null` for every key. Consequence today: no
  main-window surface can indicate which notes already have a window (this
  story's item does not need to — see reasons above), and any future main-window
  UI that reads a per-window flag out of that store will silently read nothing.
  The doc that claimed otherwise is fixed; the absent consumer is not.
- **DW — one note, two webviews, two independent buffers.** A note open in a
  notes panel and in a capture window is two `NoteEditor`s in two module
  registries; 46.12's `views` refcount is per-document and cannot span windows.
  Concurrent edits land in the diff/conflict path rather than being merged.
  Pre-existing since 45.15 and now reachable from the UI, so it is worth a
  decision: either the second view should be read-only, or the conflict path
  should be checked for this specific case on the Mac (gate check 8).
- **DW — 45.15's gate check 2 was signed off unrun.** Not a code defect. The
  process hole is that a story listed a component as a deliverable, never listed
  the file that would render it, and signed a gate check whose subject did not
  exist in any tree. `capture-note-item.test.tsx` was green throughout. Worth a
  rule: a component deliverable is not done until some non-test file imports it,
  which is a `grep` a review can run.

## Gate results on this box

- `bun run test src/components/notes/ src/components/capture/
  src/lib/stores/capture-windows.test.ts` — **45 files, 811 tests, EXIT=0, three
  consecutive runs**, on the final tree (including E48Header's 48.5 restructure
  and E48Columns' 48.1 columns), inside the freeze Main scheduled.
- Mutation sweep filter — 11 files, 101→102→110 tests as the wave grew, EXIT=0
  baseline established in that exact command and filter *before* each sweep; 8
  mutants, 8 caught, 0 survived.
- The new file alone — `bun run test
  src/components/capture/capture-in-the-note-editor.test.tsx` — 6 tests, EXIT=0.
- `bun run typecheck` — **no error in any file this story touches.** The three
  errors present are E48Top's in-flight `alwaysOnTop` field against
  `capture-windows.test.ts`'s annotated fixtures, which he has taken.
- `cargo` — deliberately not run. No Rust changed.
- Formatter and linter deliberately not run; Main runs them once at the end.
  Import order and the 100-column width were matched by hand to `biome.json`.

### Five spoiled runs, and why none of them was this story

Three frontend agents shared this checkout at the same phase, and five attempts
at the three-consecutive gate were spoiled by 35 reds: 27 + 4 in
`notes-pane.test.tsx`, 3 in `note-editor.test.tsx`, 1 in
`editor/live-preview-marks.test.ts`. **Not one was in a file this story touches**
— but "all the reds are in other people's files" is also exactly what a real
regression looks like when your change is a one-line render inside a shared
component tree (Main's warning, and 46.2's near-miss). So it was checked rather
than attributed, on three independent legs:

1. **The files were being edited by someone else.** `git diff --stat --
   src/components/notes/notes-pane.tsx` → 124 insertions / 104 deletions, none of
   them mine; E48Columns later confirmed the 27 as his M1 mutant
   (`useSurfaceColumn` at the top of `NotesPane`), and E48Header confirmed the 3
   as his M6/M6b.
2. **The suite cannot reach this story's change at all.**
   `notes-pane.test.tsx:18` does `vi.mock("@/components/notes/note-editor")` and
   substitutes a stub `<div>`, so neither `note-editor.tsx` nor `CaptureNoteItem`
   is ever loaded in it. Structural, not circumstantial.
3. **The line never moved between the greens and the reds.** The rendered
   `<CaptureNoteItem/>` was present in every green run and every red one. A red
   that appears and disappears while the suspect never changes is not caused by
   the suspect.

`editor/live-preview-marks.test.ts` got the same treatment: `live-preview.ts` is
unmodified, and the file passes 31/31 five runs out of five in isolation, so that
one was load rather than any mutant.

The general lesson, which Main adopted as a session rule: **concurrent edits
resolve fine; concurrent verification does not.** A sweep needs the rest of the
tree to hold still for the length of the sweep, not just for the length of a
write. Every mutation row above was scored inside a window broadcast to the
owner of the file, and A2 exists because one restructure still slipped between a
sweep and its final run.
