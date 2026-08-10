# Spec 46.12 — Several notes at once

status: implemented
story: 46.12
epic: 46 (The file is the setting)
binds: AD-58 (superseded in part), AD-90, AD-104 (consumed, not changed), UX-DR35, UX-DR39, UX-DR41
supersedes: the `NOTE_PANEL_LIMIT = 1` ruling recorded in specs 45.1, 45.14, 45.15, 45.18, 45.20

## The report

> "the ability to open multiple notes, like in Files."

## What was actually in the way

Not layout. `NOTE_PANEL_LIMIT = 1` was a **model-level refusal**, and the four
epic-45 specs that cite it all say the same thing: `notesEditorStore` was a
module singleton holding one buffer, one base, one `notes_open` subscription.
Two mounted `NoteEditor`s would have taken turns owning it — the second to mount
takes the store, the first shows the second's document under the first's title,
and the first's autosave writes the second's body into the first's file. Data
loss that looks completely correct on screen.

45.1 put the refusal in the **model** rather than in a surface deliberately, so
that no surface could reintroduce it by mounting a second editor of its own.
That decision is why this story is a store rewrite with a layout change attached,
and not the other way round.

The other reason recorded in 45.14/45.15 turned out **not** to apply and is worth
repeating so nobody re-derives it: quick capture is a separate **webview**, so a
separate JS realm, so a separate module registry. A module singleton is
per-realm. N capture windows were always N stores for free, and the lift below
changes nothing about that — `capture-document.tsx` still mounts one editor over
one note in its own document.

## The shape

### 1. The mirror is keyed by note

`src/lib/stores/notes-editor.ts` — one store, `documents: Record<documentKey,
NoteDocument>`, `documentKey(vaultId, noteId)` = `` `${vaultId}\u0000${noteId}` ``
(the `nodeKey` precedent in `files-tree.ts`, same reason: the one byte neither
half can contain). Every reducer takes `(vaultId, noteId, …)`. There is no
ambient "current" document for a caller to forget to check, because there is no
reducer that can be called without a vault and a note.

**Why a keyed map and not the other two shapes** (the brief asked for this to be
justified):

- **A store factory per note.** Identical semantics, plus a registry to find a
  store from outside React, plus a lifecycle to dispose one, plus a `useStore`
  whose store identity changes between renders. That is the keyed map with extra
  parts, and the parts are the ones that break.
- **A context-provided instance.** Puts the buffer out of reach of every
  non-React caller, and there are three: `saveNote` from quick capture's
  dismissal, `exportTarget`'s flush, and the CodeMirror boot closure. It would
  need the registry anyway, and would then have two ways to find one document.
- **A keyed map in one store (chosen).** One subscription root, one reset, one
  place to look. Re-render cost is nil without an equality function: a selector
  reading `documents[key].text` returns an unchanged string when another note is
  edited, so `Object.is` stops the render at the panel boundary.

### 2. One document per note, reference counted

The panel model lets a single click retarget one panel onto what another already
shows, so **two panels can hold the same note**. That must be one buffer with two
views, never two buffers over one file — which would be the singleton's data loss
rebuilt one level down, with the two halves fighting through the conflict
machinery instead of through the store.

So `NoteDocument` carries `views` and `generation`:

- `openNoteDocument` creates or joins, returning whether it created. Only the
  creator calls `notes_open`.
- `dropNoteDocument` decrements, and returns the removed document **only** when
  the last view goes — so the caller flushes and closes from a value rather than
  from a second read of a store that no longer holds it.
- `adoptBodySubscription(v, n, generation, id)` refuses a channel whose document
  was dropped, or dropped and recreated, while the open was in flight, and the
  caller closes the orphan. Not defensive: React's double-invoked effects in
  development do exactly this on every mount.
- **Every other reducer is a no-op on a note with no document.** A straggling
  batch, a resolved save or a fired timer cannot resurrect a closed note — which
  is also what stops a straggler writing into the note that took its place.

The refcount lives in the store rather than in a module `Map` beside it precisely
so `resetNotesEditorStoreForTest()` genuinely resets everything, and so the
shared-document property is assertable.

### 3. The layout is the strip that already exists

`NotesPane`'s pane 3 was one `NoteEditor` and a note id — the shape that can hold
exactly one note. It is now `<PanelStrip emptySentence={NOTES_PANEL_EMPTY_SENTENCE} />`,
the same component the Files surface hosts. A second strip would be a second
answer to "N targets side by side", with its own gesture contract, its own focus
rule and its own cookie, all of which would drift.

`NoteRow` gains `onDoubleClick` → `openPanel`. Single click replaces, double click
opens beside: AD-90's pair, copied rather than reinvented.

### 4. `NOTE_PANEL_LIMIT` is deleted, not raised

Agreed with W3Files, who owns `panels.ts` this wave. The constant and the
`target.kind === "note"` branch in `openPanel` are both gone, and the constant's
doc comment became a paragraph above `Panel` recording what it protected and why
nothing needs protecting now. A dead constant would have been a worse artefact
than either the limit or its absence. `vault-link/actions.ts`'s doc comment cited
it and was rewritten too.

## I/O matrix

| Surface | Before | After |
|---|---|---|
| Notes pane, single click a row | active panel shows the note | unchanged |
| Notes pane, double click a row | nothing (no handler existed) | opens the note beside what is open |
| Notes pane, pane 3 | one `NoteEditor`, `noteId` or null | `PanelStrip` — every panel, notes and files alike |
| Notes pane, empty panel | "Pick a note, or write a new one." | "Nothing is open here yet. Click a note to open it." |
| Notes pane, vault switch | editor blanked, note remembered | **note stays on screen**; the list changes |
| `openPanel({kind:"note"})` with a note panel open | retargets that panel | appends a second, like any target |
| Two editors, blur one | wrote whichever note the store held | writes the note that lost focus |
| Two editors, autosave | one timer, one note | one timer per mount, addressed to its note |
| Two panels on the SAME note | impossible | one document, one channel, two views |
| Close/fold one of two panels | n/a | flushes and closes that note only |
| Quick capture (own webview) | one realm, one store | unchanged; `saveNote` now names its note |
| Export a note nobody has open | identity check, no flush | `saveNote` no-ops structurally; check deleted |

## Edge cases

| Case | Answer |
|---|---|
| A batch arrives after the last view unmounted | Ignored. `mutate` no-ops on an absent document. |
| `notes_open` resolves after the document was dropped | `adoptBodySubscription` returns false, caller closes the orphan. |
| Dropped and reopened inside one round trip | Generation mismatch; the stale channel is closed, the live one adopted. |
| Two panels, same note, one closes | `notesClose` is not called; buffer, dirty state and channel survive. |
| Two panels, same note, second mounts over a dirty buffer | Joins the document; shows the unsaved keystrokes. A reset here would throw away words on screen. |
| Note panel whose vault was removed | `NotePanelBody` already says so (`PANEL_NO_VAULT_SENTENCE`). Unchanged. |
| Note panel whose vault is merely not the active one | Stays on screen. See "vault switch" below. |
| First note single-clicked into the empty starting panel, then a second double-clicked | One panel. The first click recorded "displaced nothing" and a run of previews keeps the first such record, so there is nothing to put back. Pre-existing, documented `panels.ts` behaviour; the test models the user pinning the first note instead of fighting it. |
| `noteId === null` (notes pane with nothing open) | Selectors read `EMPTY_NOTE_DOCUMENT`; no branch on "not open yet". |
| Vault id that is another vault id's prefix | Distinct keys — tested. |

### The vault switch, called out

Story 45.x asserted that switching vaults **blanked** the editor and switching
back restored it. That was a consequence of a single editor slot needing to be
told which note. Under a panel model the panel holds the note, and a vault switch
is the same act as a surface switch one level down: it changes the **browser**,
not the panels. `panels.ts`'s own header already says this about surfaces. So a
note panel is no longer hidden when its vault stops being the active one —
nothing about the note changed, and putting away a document the reader
deliberately opened would be this surface answering a question nobody asked. The
row simply stops being marked open, because it is not in this list.
`notes-pane.test.tsx`'s vault-switch test was rewritten to assert the new
behaviour and says why.

## Mutation table

Sentinel `MUT46-12`, one at a time, restored and verified by re-reading the
region and by the file's snapshot hash returning to `#CA3F` after each.

| # | Mutation | Result |
|---|---|---|
| M1 | `documentKey` returns a constant — literally the old singleton | **CAUGHT, 14 tests** across all three suites: separate buffers, separate dirty, per-note save acknowledgement, batch routing, per-note diff bar, per-note error, prefix keys, per-editor subscription, autosave channel, heartbeat channel, blur target, close/flush. |
| M2 | `openNoteDocument` blanks an existing document instead of joining | CAUGHT, 3: "share one document rather than opening a second buffer", "closes the channel only when the last one goes", "open one channel, share one buffer…". |
| M3 | `dropNoteDocument` always deletes, ignoring `views` | CAUGHT, 2: the same two refcount tests. |
| M4 | `adoptBodySubscription` skips the generation check | CAUGHT, 2: both late-subscription tests. |
| M5 | `mutate` creates the document when absent | CAUGHT, 1: "absorbs every reducer without coming back to life". |

`grep -rn MUT46-12 src/` → no matches, run after the sweep and again at the end.

Two mutations named and **not** run, for honesty: removing `onDoubleClick` from
`NoteRow`, and re-adding the `openPanel` note branch. Both are single-assertion
inversions of tests written in the same change
(`NotesPane opening several notes` and `panels.test.ts:126`), so the evidence
they would produce is the same evidence the tests' own construction gives.

## Deliberately NOT done

- **No keyboard "open beside".** The Files tree has none — its pair is pointer
  plus a context-menu item — and inventing `⇧Enter` here would put a gesture in
  the notes list the file list does not answer to. `Enter` still means "replace".
- **No per-panel note browser.** The rail and the list stay singular and follow
  the active vault. Two independent browsers is a different product.
- **No change to `setActiveTarget`'s duplicate behaviour.** It can still leave two
  panels holding one target. For notes that is now correct and supported (one
  document, two views); for files it is two independent buffers where the second
  save wins silently. That is `useTextBuffer`'s shape, W3Files owns it, and it is
  on their deferred list — agreed on the hub, not assumed.
- **No fold-specific code.** W3Files' folded panel renders no body, which is an
  unmount, which is a release. Two tests pin it from this side; the decision that
  a fold must not become a hidden-but-mounted div is theirs and is recorded in
  their spec.
- **No `NOTE_PANEL_LIMIT` replacement.** No cap on note panels at all. A cap would
  need a reason, and the only one there ever was is gone.
- **No `localStorage`, no new dependency.**

## What I could not verify here, and why

Nothing in this story is Rust, and nothing in it touches the `keeper` shell
crate. There is no macOS-only surface and no gate that Linux cannot run.

Two things are verified only through the app's own fakes rather than against a
real backend, which is the same footing every other frontend story in this epic
stands on:

- `notes_open` returning one subscription per note. The tests assert the
  arguments and the subscription ids the app threads; Rust genuinely handing back
  distinct channels for two concurrently open notes is the shell's contract and
  is exercised only when the app runs.
- The flush-then-close ordering on the last release is two unawaited IPC calls in
  sequence, as it was before this story. If Rust ever processes `notes_close`
  ahead of the `notes_save` that preceded it, the last keystrokes before a panel
  closes are lost. That race is **pre-existing and unchanged** — the old code had
  it twice, because two effects each flushed on unmount — and this change reduces
  it to one flush. Named rather than fixed: fixing it means awaiting a write on a
  teardown path, which is a different decision.

## Ordered gate checks

1. `bun run typecheck` — clean for every file in this story. (Three unrelated
   reds existed in `src/components/viewers/text-file-frame.test.tsx` from a
   sibling's in-flight `MarkdownPreviewOptions` change.)
2. `bun run test src/lib/stores/notes-editor.test.ts` — 12/12.
3. `bun run test src/components/notes/two-notes-at-once.test.tsx` — 7/7.
4. `bun run test src/components/notes/ src/components/layout/ src/components/capture/ src/lib/stores/`
   — the story's acceptance run, three consecutive times.
5. `bun run lint` and the full suite — Main runs these once at the end.
