# Spec 45.15 — Quick capture is a window you own

status: implemented (frontend + `keeper-core` proved here; `keeper` shell crate never compiled)
story: Epic 45, Story 45.15 — FR-191, FR-192, UX-DR77
owner: W3CaptureWindow

## What this story is

A capture window gains a **close button** (not only Escape), a **lock** that makes
it movable and remembers where it was put, there can be **several at once each
holding its own note**, and **any note opens as one** — so the small window is a
way of *looking* at a note rather than a special kind of note.

## The decision that shaped everything: N windows, not N panes

My brief said `notesEditorStore` being a module singleton forced a per-document
lift and that `NOTE_PANEL_LIMIT = 1` had to be deleted in the same change.
**That was wrong, W3Capture caught it before either of us wrote code, and Main
made the correction binding.** The quick-capture window is a separate *webview*
with its own JS realm (`tauri.conf.json` declares it, `capture.html` boots
`src/capture-main.tsx`, `vite.config.ts` gives it its own rollup input). A module
singleton is per-realm, so N capture windows are N stores for free.

So: **`NOTE_PANEL_LIMIT` stays untouched** — it bounds panels in one realm, which
is `PanelStrip`'s problem and nobody's in wave 3 — and multi-capture is N Tauri
windows. That kept NFR-27's prewarmed window, kept per-window geometry possible
at all, and cost nothing.

The single-window assumption that *was* real turned out to live in Rust, not in
JS. See "every place that assumed one capture window" below.

## Every place that assumed one capture window

Asked for by the brief, and more valuable than the feature. Twelve, in three
classes.

### Fixed by this story

| # | Where | The assumption | What it would have done |
|---|---|---|---|
| 1 | `tauri.conf.json` + `notes_window::CAPTURE_LABEL` | one static label, `"quick-capture"` | a second window had no name |
| 2 | `capabilities/quick-capture.json` `"windows": ["quick-capture"]` | exact label, no glob | **the silent one.** A dynamically created window matches nothing, renders normally, and can invoke no plugin permission — no hide, no close, no drag, no link. The file's own words: "looks like a frontend bug and is not" |
| 3 | `notes_window::window(app)` | took no key; there was only one to find | every window op addressed window #1 |
| 4 | `notes_window::show` / `hide` / `is_visible` | label-implicit | as above |
| 5 | `CAPTURE_SHOWN_EVENT` emitted app-wide with `app.emit` | one listener | **raising the second window would have told *every* capture window to focus itself**, so N windows would fight over focus. Unobservable with one window, which is why it survived. Now `emit_to(label, …)` |
| 6 | `notes_window` had no notion of creating or destroying a window | the window is never destroyed | no second window could exist |
| 7 | no placement storage at all | one window, one automatic position | FR-192 impossible |
| 8 | `keeper_core::registry::{get,set}_capture_buffer` — one global settings slot | one buffer | two windows would clobber one text. **Handled by W3Capture, not me**: 45.14 deleted the buffer and replaced it with `get/set_capture_draft(data_dir, key)` — keyed from day one, with the key vocabulary I own |
| 9 | `src/capture-main.tsx` rendered one component unconditionally | one window, one document | a window opened on a note had nowhere to say so |

### Left alone, deliberately

| # | Where | Why |
|---|---|---|
| 10 | `NOTE_PANEL_LIMIT = 1` in `src/lib/stores/panels.ts` | Main's ruling. It bounds note panels **in one realm**, which multi-capture does not create. Deleting it without the singleton lift is the data-loss bug its own doc comment describes |
| 11 | `hotkey::install_capture` / `tray.rs` raise "the" panel | correct: the global chord and the tray item mean the **prewarmed** window specifically. `notes_capture_show` still means exactly that |
| 12 | `notes_capture_hide(app)` hides the draft window specifically | correct for W3Capture's Escape path. The general verb is my `notes_capture_close(key)` |

### Already there, nobody applied it

Asked for by the brief, including "nothing". Two findings:

- **`listenNotesCaptureShown` had existed since Epic 36 and was called from
  nowhere** — W3Capture found and mounted it in 45.14. My change makes it
  per-window rather than app-wide.
- **Nothing else.** There was no unused geometry code, no dormant window
  manager, no `locked` field anybody had already added. Placement is genuinely
  new.

## Architecture

```
keeper_core::capture        pure, total, compiles everywhere
  DRAFT_CAPTURE_KEY "draft" | CaptureTargetVm{Draft,Note} | CaptureWindowVm
  capture_key(target)   -> "draft" | "note:<enc vault>/<enc note>"
  capture_search(target)-> "" | "?vault=…&note=…"     (the window's URL)
  capture_label(key)    -> "quick-capture" | "quick-capture-<16 hex FNV-1a>"
  CAPTURE_LABEL_GLOB, is_capture_label
  Placement{locked, position} encode/decode (total)
  plan_close(key, other_windows_visible) -> ClosePlan{destroy, raise_main}

keeper_core::registry       notes.capture_placement.<key>
  get_capture_placement / set_capture_placement

keeper::notes_window        Tauri calls only
  show/hide (draft)  open(target, placement)  close(key)->pos
  position_of  list(locked_fn)  announce  key_for_label
  OPEN: label -> target, process state, seeded with the draft window

keeper::notes_ipc           notes_capture_open / _close / _set_locked / _windows
                            + remember_placement (blur & close)

src/lib/capture-target.ts   the key, and the URL PARSER (no composer — see below)
src/lib/stores/capture-windows.ts   the mirror of the open set
src/components/capture/capture-window.tsx   CaptureWindowChrome, CaptureNoteWindow,
                                            useCaptureDismissKeys
src/components/capture/capture-note-item.tsx   "Open in a capture window"
src/capture-main.tsx        one entry point, branches on its own URL
```

### Why the label is a hash

A Tauri label is matched against a capability glob and must be charset-legal; a
note id is arbitrary. The label is `quick-capture-<FNV-1a of the key>` —
deterministic across restarts, so "is this note already open?" is a
`get_webview_window` call and not a table that goes stale, and always inside the
glob. It is pinned to a **literal** in the tests, because the contract is that
it survives a *rebuild*.

### Why the key is percent-encoded

Without it, vault `a` / note `b/c` and vault `a/b` / note `c` produce one key —
and two different notes then share one window, one draft pointer and one
remembered position. Note ids come from paths; slashes are ordinary.

### Where the position lives, and why in Rust

Tauri owns geometry, and the placement must be known *before* the webview loads
or the window jumps. The brief offered `document.cookie` (the codebase's
mechanism for UI state) — **rejected, with the reason**: a cookie is per-document
and a capture window's document is destroyed when it closes, and the prewarmed
window must be placed by `show()` before any JS runs. It is a settings row,
keyed by capture key, beside 45.14's draft pointer.

It is written on **blur and on close**, not on `Moved`: a drag emits one event
per compositor frame and a settings write per frame puts a sqlite transaction
inside a gesture. Cost of that choice, stated where it lands: a position moved
and then lost to `kill -9` is not remembered.

## I/O and edge-case matrix

| Input | Output | Why |
|---|---|---|
| `capture_key(Draft)` | `"draft"` | the storage key 45.14's draft pointer and this story's placement share |
| `capture_key(Note{a, b/c})` | `note:a/b%2Fc` | injective; see above |
| `capture_key(Note{"", ""})` | `note:/` | total; a blank id is a key, not a panic |
| `capture_search(Draft)` | `""` | the prewarmed window's URL is byte-for-byte `tauri.conf.json`'s |
| `capture_search(Note{"a+b","c"})` | `?vault=a%2Bb&note=c` | `URLSearchParams` decodes `+` as a space; unescaped, the window opens on a vault nobody has |
| `capture_label("draft")` | `quick-capture` | the static declaration |
| `capture_label("note:v/n")` | `quick-capture-da85850f1ff52f94` | pinned to a literal |
| `is_capture_label("main-quick-capture-…")` | `false` | prefix, not substring — a foreign window must not get a placement row |
| `Placement::decode("")` | `{locked:true, position:None}` | absent row and default are the same picture |
| `Placement::decode("free 12 banana")` | `{locked:false, position:None}` | the readable half kept; **a fabricated axis is worse than none** |
| `Placement::decode("banana")` | default | anything but `free` leaves keeper in control |
| `Placement{locked:true, position:Some}` | kept | unlock, drag, lock again means "keep it *there*"; locking is not a discard button |
| `plan_close("draft", *)` | `destroy:false` | hidden, never destroyed — NFR-27's prewarm |
| `plan_close(key, false)` | `raise_main:true` | undecorated + `skipTaskbar`: destroying the only visible window leaves a running app with no surface and, on a desktop with no tray, nothing to click |
| `plan_close(key, true)` | `raise_main:false` | never steal focus from three open captures |
| `captureTargetFromSearch("")` | `Draft` | |
| `captureTargetFromSearch("?note=n")` | `Draft` | **half a target is refused, not completed.** A note id is unique only inside its vault; guessing one opens a *different* note under this note's name |
| `captureTargetFromSearch("?theme=dark&vault=v&note=n&t=1")` | `Note{v,n}` | survives parameters it did not write |
| chrome, lock unknown | renders as locked | a live drag region for one frame turns a click aimed at Close into a window move |
| close button, note window | `saveOpenNote()` **awaited**, then `notes_capture_close(ownKey)` | the window is destroyed; a write still travelling loses the last 1.5 s (AD-62) |
| close button, draft window | the document's own `dismiss` | one act, not two spellings |
| Escape / Ctrl+W | same act as the close button | |
| Escape already `defaultPrevented` | ignored | CodeMirror marks it handled when it closes the `/` menu, the tag chooser or emoji — otherwise dismissing a popup destroys the window |
| `CaptureNoteItem` with `capabilities.notes === false` | renders `null` | the same flag `use-notes-shortcut.ts` gates ⌘⌥K on |

## Verification

**TypeScript, my scope, run by name, three consecutive repeats:**
`bun run test src/lib/capture-target.test.ts src/lib/stores/capture-windows.test.ts src/components/capture/capture-window.test.tsx src/components/capture/capture-note-item.test.tsx src/capture-main.test.tsx src/test/capture-capability.test.ts`
→ **EXIT=0, 44/44 ×3, zero unhandled errors.**

**The acceptance command, run literally** — `bun run test src/lib/stores/ src/components/notes/` plus my files — is reported with attribution rather than as a single green: the tree carried live sweeps and mid-edit files from five siblings throughout (`text-splice.ts` transform error, `note-file-links.test.tsx`, `space-list.test.tsx`, `properties-panel.test.tsx` retitled between two runs). My six suites are green three times in isolation; every red observed carried zero of my symbols.

**Rust:** `cargo test -p keeper-core --lib -- capture::tests registry::tests::two_capture_windows` → **EXIT=0, 14/14 + 1.** Note the second filter: the two-window placement-independence test lives in `registry` and is part of this story's acceptance.

`tsc --noEmit`: zero errors in any file this story touches.

### Two capture windows, asserted on both

The acceptance criterion says "two capture windows hold two different notes and
typing in one does not change the other, asserted on both buffers". With N
realms the *buffer* is per-webview by construction and unobservable from jsdom,
so it is asserted on the two values this story actually owns, in both languages:

- **Rust** — `two_capture_windows_remember_two_placements_independently`: two
  keys, two placements, and after each write **both** rows are read back. Moving
  one must not place a window nobody has moved; placing the second must not move
  the first.
- **TypeScript** — the store test holds three windows with three targets and two
  different lock states and reads each by key; the chrome test drives the second
  window's lock and asserts the call carries *its* key; `CaptureNoteWindow`
  renders twice in one realm and both `CaptureDocument` props are asserted.

## Mutation table

48 mutations, 48 caught. Sentinel `MUTW315_<id>`, unique in both directions;
harness in `~/.W3CaptureWindow/`, restore verified by sha256 against a snapshot
taken immediately before each edit, never by an anchor grep.

**Three survived their first probe. All three were real holes and all three are
closed.**

| id | mutation | first | note |
|---|---|---|---|
| 01–07 | draft key, both encode calls, `/` unreserved, lower-case escapes, draft label, label prefix | CAUGHT | |
| **08** | `starts_with` → `contains` in `is_capture_label` | **SURVIVED** | my negatives were all *shorter* than the prefix. A foreign label **containing** it — `main-quick-capture-0000…` — would have made the window-event handler write a capture placement row for somebody else's window. Two assertions added |
| **09** | FNV offset basis + 1 | **SURVIVED** | `a_label_is_the_same_in_every_process` recomputed the hash with `fnv1a64`, so both sides moved together — **the assertion had become a tautology.** Now pinned to two literals, with the reason: change this number and every remembered placement is orphaned under a name nothing looks up |
| 10–16 | default lock, encode/decode inversion, half-readable position, `plan_close` both fields, glob/prefix disagreement | CAUGHT | |
| 17–19 | placement key ignores its key / always reads draft / collides with the draft-pointer prefix | CAUGHT | |
| 20–26 | TS draft key, both `encodeURIComponent`s, draft URL, swapped params, `||`→`&&`, note id = vault id | CAUGHT | |
| 27–31 | `captureWindowFor` answers row 0, failed read blanks the list, open/close/lock ignore their argument | CAUGHT | |
| 32–35 | unknown reads unlocked, always-draggable, lock writes current value, dead close button | CAUGHT | |
| **36** | `await saveOpenNote()` → `void saveOpenNote()` | **SURVIVED** | the test asserted **invocation order**, which `void` preserves. The contract is that the save has *landed*: this window is destroyed, so a write in flight when the webview goes away is the thought lost. Rewritten to hold the save on a deferred and assert the close has **not** happened until it resolves |
| 37–40 | wrong self-key, wrong note handed to the document, Ctrl+W dead, `defaultPrevented` guard removed | CAUGHT | |
| 41–42 | note window renders the draft page, draft chrome wired to nothing | CAUGHT | |
| 43–45 | menu item opens the wrong note, gate removed, action ignores its note | CAUGHT | |

## Shape audit — reported separately, as required

The sweep is a list of lines I already decided were load-bearing. These are the
probes it could not contain. Shapes taken from Main and from seven peers; **not
one of the findings came from extending my own list.**

1. **What composes the input?** — the window's URL was composed in Rust
   (`notes_window`, uncompilable here) and parsed in TS, pinned to each other by
   *nothing but matching prose*. **Moved `capture_search` + its encoder into
   `keeper_core::capture` and added `search` to the shared vector table**, so
   Rust asserts it composes each vector's URL and TS asserts it parses back to
   the target. Real gap, closed.

2. **A second spelling with one producer.** While doing (1): `captureSearch` in
   TypeScript **was never called by production code** — Rust is the only thing
   that creates a window. Worse, it disagreed: `URLSearchParams` writes `+` where
   Rust writes `%20`, both decode the same, which is exactly what makes such a
   drift invisible. **Deleted**, with the reason written where it was.

3. **Did anything press the button?** — every control is pressed with a real
   callback and the *call* asserted: close, lock (both directions, both keys),
   the menu item, Escape, Ctrl+W.

4. **A contract stated in a doc comment and enforced nowhere.** Three found,
   three now enforced: the capability `windows` glob (`src/test/capture-capability.test.ts`,
   which runs on Linux where the shell crate cannot); the capability
   *description* against its own permissions — the file used to say "No opener,
   no dialog" and now grants both; and the TS↔Rust key, by vector table.

5. **A fallback for a case that cannot happen.** `notes_window::list` defaulted
   an unknown capture label to `Draft`, which would have put a **second row
   keyed `draft`** in a list every reader keys on. Now skipped. Re-probed the
   line after removing it.

6. **Two-item collections everywhere.** Every fixture in the store and chrome
   tests holds two or three windows with different targets *and* different lock
   states — the pair a `slice(0,1)` or an "answer row 0" mutation cannot survive
   (mutation 27 confirms).

7. **A branch reachable only from a second host.** `CaptureWindowChrome` has two
   hosts — the draft window (via `capture-main`'s `chrome` slot) and
   `CaptureNoteWindow`. Both are driven; mutation 42 exists because the draft
   host's wiring is a different line from the note host's.

8. **Door count.** Doors into this story: the hotkey → prewarmed window (1 test,
   `capture-main`), the note menu → a new window (3), the close button (3, one
   per host plus the wiring), Escape/⌘W (3), the lock (3), the capability (6),
   restart → placement restored (1, Rust). The thinnest door was **the note
   menu**, which is the door the story's own title describes; it got its own
   file rather than being folded into the chrome suite.

9. **A doc comment naming another module's behaviour** (W3Chrome). Three of mine
   checked mechanically rather than read: `use-notes-shortcut.ts` really does
   gate ⌘⌥K on `capabilities.notes` (so my menu item's gate is that flag and not
   an invented one); `cookie-writer.ts` really is the single assignment site
   (which is *why* placement is not a cookie); `capture-document.tsx` really
   exports `CaptureDocument` and `CaptureDraftDocument` with the props I render.

10. **An absence with no witness** (W3Recording/W3TagsDelete). My
    `expect(…).not.toHaveAttribute("data-tauri-drag-region")` is paired in the
    same test with the positive on the unlocked window — one `rerender`, two
    states, same representation. Checked rather than assumed.

### Late finding: a contract the replaced module kept (W3Capture's shape)

*When you replace a mechanism, the contracts the old one kept are not in the
diff.* I rewrote `notes_window.rs` whole, so its promises are in a file that no
longer exists. Read the old doc comment rather than my diff. Five promises,
four carried unchanged (never destroy the prewarmed window; the three-call
hotkey path; the window never unmounts; monitor-aware best-effort placement).
The fifth is **UX-DR43: "Nothing in the UI promises a position it cannot
deliver"** — and a lock icon is the most obvious way to break it, because
`set_position` is exactly the call Wayland refuses.

Kept, by wording the controls for what they can deliver. **The lock promises
movability**, which is the compositor's own drag and works everywhere; it does
not promise the restore. Hence "Unlock this window so it can be moved" and "Lock
this window where it is", and never "remembers". A compositor that declines
leaves a window the person can still put where they like every time, rather than
a promise that quietly fails. Written into the module doc so the next reader
finds it where the old one was.

**So the acceptance line "the position survives a restart" is narrowed and
should be read as: the position is recorded and re-applied best-effort, on every
platform whose compositor accepts `set_position`.** Gate check 4 below is where
that gets measured, and on a Wayland session it is expected to be the
compositor's placement rather than mine.

### Late finding: a refused write must not destroy the window (W3NoteFile's shape, via W3Capture)

*`await` is not a success check when the callee catches its own failure.*
`CaptureNoteWindow.dismiss` did `await saveOpenNote()` and then closed.
`saveOpenNote` catches — the editor's caption is fed from the same store — so
the await proved only that it finished.

**This window is destroyed, not hidden.** The prewarmed window merely hides on a
refused write, so the words survive in a buffer on a page that is handed back;
here the webview, the buffer and the unsaved text go with it, and nothing says
why, because the only surface that could have said anything is the one that just
vanished. W3Capture widened `saveOpenNote` to `Promise<boolean>` for the same
defect in 45.14; a refused save now **cancels the close** here. The window stays,
the words stay in front of the person, and the reason is already on screen from
`markSaveFailed`. One write, one error channel (UX-DR35).

Closed with a test that refuses the write, asserts the close did not happen and
the document is still mounted, then presses Escape and asserts it still does not.
Probed both ways — MUTW315_46 (ignore the answer) and _47 (invert the guard) —
**both caught, by that test only.**

### Late finding: a name with nothing checking what it names (W3Recording's shape)

*Does this thing name something, and does anything check the thing it names
exists? A reference and a dangling reference are the same bytes.*

`capture.html` was named **twice**: by `tauri.conf.json` for the prewarmed
window, and by `notes_window.rs`'s `CAPTURE_DOCUMENT` for every window this
story creates. Nothing made the two agree. Rename the file and the prewarmed
window follows its declaration while **every window opened on a note loads
nothing** — the works-at-the-root, breaks-everywhere-else shape, and a blank
window says nothing about why.

`src/test/capture-capability.test.ts` now reads both files and asserts the
literals match. Probed by renaming the Rust constant: **caught**, by that
assertion only. It runs on Linux, which is the point — the half that would have
been broken is the half the shell crate owns and this box cannot compile.

### Last edit, landed under the stand-down, and its honest state

W2Media generalised the `capture.html` finding: **before this wave there was one
namer and nothing to disagree; adding the second namer created the hazard.** Run
against the rest of this story's cross-boundary names, that leaves two more —
the event names, each written twice, once in `notes_window.rs` where it is
emitted and once in `client.ts` where it is listened for. A `listen` on a name
nothing emits is the quietest failure in the codebase: no throw, no rejection,
no log, just a listener that never fires. 45.15 doubled the exposure by adding
the second event.

`src/test/capture-capability.test.ts` now reads both files and pins both pairs,
and asserts the shown event is emitted with `emit_to` rather than `emit` — the
per-window correction that only matters once several windows exist.

**These three assertions are green (47/47 ×3) but NOT mutation-probed.** The
stand-down landed between writing them and running the probe, and the probe was
never started, so the tree never saw a sentinel for them — verified: zero
`MUTW315` anywhere, `emit_to`, both event literals and `CAPTURE_DOCUMENT` all
intact. They are the only assertions in this story whose failure mode is
untested, and that is stated here rather than folded into the caught count:
**the mutation table is 48/48; these three are additional and unprobed.**

## Deliberately NOT done

- **`NOTE_PANEL_LIMIT` is not deleted and the editor store is not lifted.**
  Main's ruling. It is a main-window concern; multi-capture does not create it.
- **The position is not a cookie**, though `column-widths.ts` and `panels.ts` are
  the precedent my brief pointed at. Reason given above: a capture window's
  document does not outlive the window, and the prewarmed window must be placed
  before any JS runs.
- **No `Moved`-event persistence.** Blur and close only; the cost is named.
- **No drag on a locked window and no `set-position` for the webview.** A window
  that can place itself can place itself off screen.
- **`useCaptureDismissKeys` is exported but `CaptureDraftDocument` still has its
  own identical inline handler.** I own the hook; that file is W3Capture's and
  they were mid-sweep. One import and one call closes it — **owed, not done.**
- **No "focus the window that already holds this note" affordance in the menu.**
  Rust already raises it, so the item is correct; changing its *label* when the
  window exists would need the main window to poll the list. Not asked for.
- **A *derived* capability check.** `capture-capability.test.ts` names each
  permission by hand, so the next permission somebody adds needs a line nobody
  is forced to write; Rust exporting the set the capability must cover would
  remove that. **Owed, not done** — and recorded as an aspiration rather than as
  a defect, per W3Chrome's caveat to their own standard. *When the checking form
  and the convenient form are the same form, nobody has to remember* — but some
  references have a query-shaped witness and some have none, and knowing which
  class you are in is what tells you where the discipline has to live. This
  contract spans a JSON config file and a Rust crate, so an assertion that reads
  both and pins the literals is the only witness that exists for it today —
  **and it runs on Linux, which is precisely the half the shell crate cannot
  give us.**

## What I could not verify here, and why

The `keeper` shell crate **does not build on Linux** — `cargo check -p keeper`
fails in `glib-sys`/`gobject-sys` build scripts before one line of keeper
compiles. Everything decidable was pushed into `keeper-core` and is proved there
(15 tests). What has **never been compiled or run**:

- `notes_window.rs` in full: window creation, destroy, `emit_to`, `start_dragging`
  authority, `outer_position`, the `OPEN` map.
- The four commands `notes_capture_open` / `_close` / `_set_locked` / `_windows`,
  their four mobile twins, and `remember_placement`.
- The `lib.rs` window-event arm that persists a capture window's position on blur.
- **The capability file's effect.** `capture-capability.test.ts` proves the file
  *says* the right thing and `keeper_core::capture` proves every derived label
  matches the glob; nothing here proves Tauri agrees. `notes_window.rs` carries
  the `include_str!` assertion for the machine that can run it.

**Gate checks, in order:**

1. `cargo check -p keeper` — nothing below is meaningful until this passes.
2. **Open a note's actions menu → "Open in a capture window". A second window
   must appear AND its close button must work.** If it appears and the buttons
   do nothing, the capability glob did not take — that is the silent failure this
   whole naming scheme exists to prevent, and it looks exactly like a frontend
   bug.
3. Open a *second* note the same way. Two windows, two notes; type in one and
   confirm the other's text does not move. Then press the hotkey — three windows.
4. Unlock one (the lock icon), drag it somewhere deliberate, click away, quit,
   relaunch, reopen that note as a capture window. **It must come back where you
   put it.** Then lock it and confirm it stays there and cannot be dragged.
5. Close the last visible window with the main window hidden to the tray. The
   main window must come up rather than the app becoming invisible.
6. Press the chord and close the prewarmed window with the **button**, then
   immediately press the chord again. It must be instant — if there is a visible
   delay, the close button destroyed the prewarmed window instead of hiding it
   and NFR-27 is gone.
