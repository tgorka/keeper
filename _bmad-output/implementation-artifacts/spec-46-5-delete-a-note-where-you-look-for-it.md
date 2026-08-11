# Spec 46.5 — Delete a note where you look for it

story: 46.5
status: implemented, partially gated
branch: `feat/epic-46-config-and-the-gaps`
binds: FR-195, UX-DR78 (revises 45.17, 45.18); FR-48 (palette retitle)

## What the defect actually was

The brief said "the `⋯` is unlabelled and hard to find". Halfway through the wave
Main's arithmetic (from `ScoutCaptureMenu`) corrected it: **the `⋯` was not on
screen at all**. `note-editor.tsx`'s header is a single non-wrapping
`flex items-center gap-2` and carried, in order: a `flex-1` title, a truncating
path, a variable-width save caption, and **six** controls — `Attach a file`,
`Attachments`, `Properties`, `History`, a conditional `Show in Files`, and the
actions menu. In the 560 px quick-capture window (`notes_window.rs:91`) that row
overflows by roughly 115 px `[INFERENCE — no layout engine was run; the estimate
is ScoutCaptureMenu's and I did not re-measure]`, and the menu is the group's
LAST child, so it is the first thing past the right edge.

The owner wrote "I still see no way to delete notes." That was literally true,
not a complaint about affordance legibility.

Two independent things were therefore wrong and both are fixed:

1. **Structural.** Group 3 now holds two controls. `Attachments`, `Properties`,
   `History` and `Show in Files` moved into the menu. All four open a panel, a
   surface or a dialog; none is a per-keystroke verb; a menu is what they are
   for. This fixes every width at once — including widths below 560 that
   `notes.capture_placement.<label>` can restore — with no `ResizeObserver`, no
   container query and no host-conditional render.
2. **Legible.** The trigger is the word **`Actions`** instead of a bare
   `MoreHorizontal`. Four text labels and one icon was the report's own
   description; an icon among words reads as decoration.

### Why "Actions", and why a word at all

`bridge-card.tsx:266` is the house's other object-level `DropdownMenu` sitting in
a row of words, and it spells its trigger `Manage` — a word, `size="sm"`,
`variant="ghost"`, with `aria-label={`Manage ${network.name}`}`. That is the
pattern followed rather than inventing a fifth. The icon-only triggers in the
house (`properties-panel.tsx:1129`, `account-footer.tsx:506`,
`phone-header.tsx:93`) are all dense per-row or phone-tier controls, which this
is not.

`Actions` and not `More`, because `NOTE_ACTIONS_LABEL` already made the
accessible name `Actions for <title>`. The visible text is now a **prefix** of
the accessible name, which is what WCAG 2.5.3 (Label in Name) requires and what
lets speech input ask for the control it can see. `More` would have broken that;
mutation **M5** below is exactly that mutation, and it is caught.

### What was NOT done, deliberately

- **No `Delete` button in the toolbar.** A one-press destructive verb in a row
  of toggles is a worse bug than the one being fixed. Delete stays last in the
  menu, behind `NoteDeleteDialog`, unchanged.
- **The delete path itself is untouched.** No change to `notes_delete`,
  `notes_delete_plan`, the trash path, the confirmation copy, or any Rust.
- **The header's grouping/`shrink-0` structure is W1Flicker's (46.4).** I own
  group 3's *contents* only. I wrote no class on the wrapper and changed no
  class on the caption.
- **No layout measurement in tests.** jsdom performs no layout. A test claiming
  to check 560 px would be a lie; the structural assertions below stand in for
  it and the pixels are gate checks.
- **The tray still has no start item.** That is 46.16.
- **`recording-start` was renamed, not re-homed.** Its `id` and its `"Recording"`
  category are unchanged, so `palette.rs`'s three-verb section-count assertion
  still holds. Moving it to `Navigation` would have broken it.
- **`ATTACH_FILE_LABEL` stayed a direct header button.** It is the only control
  that starts *outside* keeper, it is the one thing here that is not "show me
  something about this note", and W1Attach's new test presses it by role
  `button`.

## I/O matrix

### The header (`note-editor.tsx`, group 3)

| State | Group 3 renders | Menu (opened) renders |
|---|---|---|
| Any note open | `Attach a file`, `Actions` | Attachments, Properties, History, [Show in Files], ─, Export…, ─, **Delete note** |
| `noteId === null` | nothing — the no-note branch returns at `:523` before the header | — |
| `vault === null` (vault list unread) | unchanged | `Show in Files` **absent** |
| `filePathForNote(vault, path) === null` (no vault subfolder) | unchanged | `Show in Files` **absent** |
| `path === null` (before the opening `Reset`) | unchanged | `Show in Files` **absent** |
| 560 px capture window | same two controls | same |

### `NoteActions` (`note-actions.tsx`)

| Input | Output |
|---|---|
| `title = "Standup"` | trigger text `Actions`, accessible name `Actions for Standup` |
| `children` | rendered first, in caller order |
| always | `DropdownMenuSeparator`, then `Delete note` (`variant="destructive"`) last |
| select `Delete note` | `notesDeletePlan` only; `NoteDeleteDialog` mounts; **no** `notesDelete` |
| confirm | `notesDelete(vaultId, noteId)`; panel closes |
| cancel / plan rejected | nothing deleted; unchanged from 45.17 |

### `palette.rs` `recording-start`

| Query (recording capability on) | Before | After |
|---|---|---|
| `"new"` | no match | `recording-start` |
| `"new recording"` | no match | `recording-start`, **first** result |
| `"start"` | matched (title) | still matches (keyword) |
| `"record"` | matched | still matches |
| Recording section item count | 3 | 3 |
| `id` / `category` | `recording-start` / `Recording` | unchanged |

Title: `"Start Recording"` → `"New Recording"`. Keywords:
`["record","capture","screen","begin","go live"]` →
`["record","capture","screen","new","start","begin","go live"]`.
`"New X"` is this registry's own word for making one (`notes-new` → `"New Note"`,
`palette.rs:554`); `"Start Recording"` was the outlier, and the outlier is the
one nobody could find.

## Edge cases

- **`ATTACHMENTS_LABEL` names two things.** It is the menu item's word *and*
  `attachments-panel.tsx:224/244`'s `<section aria-label>`. A bare
  `getByRole(name:)` can resolve to the panel once the trigger is a menu item,
  and the failure then reads as "menu item missing" when the item is fine.
  Every query for it is scoped `within(menu).getByRole("menuitem", …)`.
  `PROPERTIES_LABEL` has the identical shape (`properties-panel.tsx:416/432`)
  and is handled the same way; both are now single exported constants used by
  the `<section>` and the item, so they cannot drift.
- **Dangling absence.** `note-file-links.test.tsx`'s three
  `queryByRole(...).toBeNull()` assertions cover 45.18's predicate. Inside an
  unopened Radix menu they would pass while asserting nothing. All three now go
  through `showInFilesIsNotOffered()`, which **opens the menu** and first proves
  a sibling (`Properties`) is present, so `null` can only mean the predicate
  refused. Verified by forcing the predicate true — see M-abs.
- **Radix opens on pointer, not click.** `fireEvent.click` on the trigger does
  nothing in jsdom. Every helper uses the house pair
  (`pointerDown {button:0, ctrlKey:false}` + `pointerUp {button:0}`), as at
  `note-actions.test.tsx:107`, `export-controls.test.tsx:149`,
  `account-footer.test.tsx:68`, `bridge-card.test.tsx:194`.
- **Radix closes the menu on select.** Two consecutive `Show in Files` presses
  (`note-file-links.test.tsx:254-255`, preserved from before) each reopen it.
- **`findByText("Properties")` as a mount barrier.** Seven call sites across
  `format-toolbar`, `new-note-caret`, `emoji-wiring` and `tab-wiring` used the
  word as "the header rendered". It is no longer in the DOM until the menu
  opens; they now wait for `NOTE_ACTIONS_TEXT`, which is unconditional header
  text and is the control this story added.
- **Group 3's child count is variable** (`Show in Files` resolves async on
  `vault`/`path`). Still true, and still only one control's worth — but it is
  now *inside* the menu, so the header row's width no longer changes when it
  arrives. That removes the second, post-first-paint reflow source
  ScoutNotesGaps identified for 46.4.
- **Six menu items, one destructive.** Position alone was a sufficient guard
  when the menu had one item; with six it is not, so `Delete note` now has a
  `DropdownMenuSeparator` above it as well as last place.

## Mutation table

Frontend suites for every row: `note-actions.test.tsx`,
`note-file-links.test.tsx`, `capture-document.test.tsx` (44 tests). Rust rows:
`cargo test -p keeper-core --lib palette::` (26 tests). Each mutant was applied
from a byte backup, run, restored from that backup, and `cmp`-verified; the
harness prints `restored: yes` per row and a post-sweep `grep` for the mutant
strings across `src/` returns nothing.

| # | Mutation | Caught by | Result |
|---|---|---|---|
| M1 | `title: "New Recording"` → `"Start Recording"` | `palette::tests::the_start_verb_answers_the_word_people_search_for` (`palette.rs:1192`) | ✅ 1 failed |
| M2 | keywords drop `"new"`+`"start"` | same test (`palette.rs:1209`) | ✅ 1 failed |
| M3 | trigger back to icon-only (`{NOTE_ACTIONS_TEXT}` → `<span aria-hidden>⋯</span>`) | `puts a word on the trigger…` + `leaves two controls in the header…` | ✅ 2 failed |
| M4 | `Properties` moved back out of the menu into the header row | `leaves two controls…`, `still offers every verb…`, both `capture-document` tests, all three `note-file-links` absence tests | ✅ 7 failed |
| M5 | `NOTE_ACTIONS_TEXT = "More"` (visible word not a prefix of the accessible name) | `puts a word on the trigger…` | ✅ 1 failed |
| M7 | `Delete note` rendered *before* `{children}` | `still offers every verb…` + 45.17's `renders another story's item above the destructive one` | ✅ 2 failed |
| M8 | `History` item deleted from the menu | `still offers every verb…` | ✅ 1 failed |
| M-abs | `Show in Files` predicate forced true (`{(true \|\| (…)) && (`) | all three `note-file-links` absence tests | ✅ 3 failed — the absences are live, not dangling |

**8 mutants, 8 caught, 0 survived.** M6 (deleting the `DropdownMenuSeparator`)
was considered and deliberately not run: a separator carries no `menuitem` role,
so no honest assertion distinguishes it and a test that could would be asserting
the diff. It is a visual guard and is listed under gate checks instead.

## What I could not verify here, and why

**jsdom performs no layout.** Nothing in this repo's test environment can
observe that the row fits 560 px — which is the defect. `src/test/setup.ts`'s
rect shim deliberately stops at the CodeMirror boundary and is not a layout
engine. The tests assert the *structure the fit follows from* (two controls in
group 3, identified by label, not just counted) and say so in their own
docblock. Everything below is a human gate check, in order:

1. **macOS build.** The `keeper` shell crate does not link on Linux (no
   GTK/webkit), so `cargo build/check/clippy -p keeper` was never run. I touched
   no Rust outside `keeper-core/src/palette.rs`, which **was** compiled and
   tested here (`cargo test -p keeper-core --lib palette::`, 26 passed).
2. **Open a note in the main pane.** Header reads
   `<title> … <path> … <caption> | Attach a file | Actions`. The word `Actions`
   is visible; there is no bare `⋯`.
3. **Press `Actions`.** Six items: Attachments, Properties, History, Show in
   Files, ─, Export…, ─, **Delete note** in destructive red, last, below a rule.
4. **Press `Delete note`.** The confirmation appears. Nothing is deleted until
   you confirm. Cancel leaves the note.
5. **The 560 px case, which is the reported one.** Open quick capture (⌘⌥N /
   the configured chord). The window is 560 px. Confirm `Actions` is fully on
   screen with nothing clipped at the right edge, and that Properties opens from
   it (`capture-document.test.tsx` proves the wiring; only the pixels are
   unverified).
6. **The persisted-placement case** (ScoutConfig's point): resize the capture
   window narrower than 560, type, save, close, reopen — `notes.capture_placement`
   restores the width you left, so the owner may be running a width neither Main
   nor I picked. Confirm the header degrades by truncating the *title/path*
   group and never clips group 3.
7. **The palette.** ⌘K → type `new` → **New Recording** appears and is the top
   answer for `new recording`. Type `start` → it is still there. The `Recording`
   section still holds exactly three verbs, one submenu from `Open Recordings`
   in `Navigation`.
8. **Screen reader / speech input** (unverified, no AT on this box): the trigger
   should be announced `Actions for <title>` and should respond to "click
   Actions". The prefix relation is asserted in code; the announcement is not.

## Files changed

- `src/components/notes/note-actions.tsx` — word trigger, `NOTE_ACTIONS_TEXT`,
  separator above the destructive item, docblock rewritten to record why 45.17's
  line moved.
- `src/components/notes/note-editor.tsx` — group 3 collapsed to two controls;
  four verbs became `DropdownMenuItem`s inside `NoteActions` (contents only —
  W1Flicker owns the wrapper and the caption).
- `src/components/notes/properties-panel.tsx` — `PROPERTIES_LABEL` exported and
  used by both `<section aria-label>`s.
- `src/components/notes/note-history-panel.tsx` — `NOTE_HISTORY_LABEL` exported.
- `src/lib/recording-control.ts` — docblock named the old palette title.
- `src-tauri/crates/keeper-core/src/palette.rs` — `recording-start` retitled and
  re-keyworded; new test `the_start_verb_answers_the_word_people_search_for`.
- Tests re-anchored: `note-actions.test.tsx` (+3 new),
  `note-file-links.test.tsx`, `attach-entry-points.test.tsx`,
  `attachments-panel.test.tsx` (the `editorWithPanel` helper only),
  `capture-document.test.tsx`, `format-toolbar.test.tsx`,
  `new-note-caret.test.tsx`, `editor/emoji-wiring.test.tsx`,
  `editor/tab-wiring.test.tsx`.

## Gate results on this box

- `bun run test src/components/notes/ src/components/command-palette/` — **41
  files, 725 tests, exit 0, three consecutive runs.**
- `bun run test src/components/capture/ src/components/export/` — 6 files, 62
  tests, exit 0.
- `cargo test -p keeper-core --lib palette::` — 26 passed, 0 failed, including
  `open_recording_present_iff_recording_capability_on`'s three-verb
  section-count assertion.
- `bunx tsc --noEmit` — clean.
- Formatter and linter deliberately not run (Main runs them once at the end).
