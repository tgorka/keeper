# Story 45.14 — Quick Capture Is the Note Editor

status: implemented
epic: 45 — Open it, change it, put it back
binds: FR-190, AD-93
depends on: 44.6 (`notes_create`, the one creation path, with `notices`), 44.7 (templates), 37.6 (`NoteEditor`), 36.3/36.4 (the capture window)
feeds: 45.15 (the capture window you own), 45.16 (capture's template and tag)

---

## 0. What was already there, and what was not

The epic asks each story to say what it found, including "nothing". Here it is
four things, and one of them changed the shape of the story.

| Thing | State before | What this story did with it |
|---|---|---|
| `NoteEditor` | Complete. Live preview, format toolbar, `/` menu, emoji, tags, properties, attachments, backlinks, history, conflicts. | Mounted it. Added nothing to it. |
| `create_note` | Already "the shared create path, used by `notes_create` and by capture" — its own doc comment. | Kept. Capture is still a caller. |
| 44.6's `notices` channel | Reached capture and was **thrown away**: `commit_buffer` passed `&mut Vec::new()` with a comment saying capture had no surface to show a sentence on. | Capture has a surface now. The channel is rendered. |
| `listenNotesCaptureShown` | Declared in `client.ts` in Epic 36 and **called from nowhere** — the third of DW-172's dead listeners, named as still dead in 44.6 §0. | Called. It is the belt to the re-arm. |

**The one that changed the shape.** AD-93 says "quick capture mounts `NoteEditor`",
and the obvious reading is that the panel grows a better text widget. It cannot
be that, and the reason is not stylistic:

> **A tag is frontmatter on a note. An attachment is a file copied relative to a
> note's path. Neither can be applied to a `String` in a settings table.**

Quick capture's durable value was `notes.capture_buffer`, a debounced mirror of a
textarea, and a note was assembled out of it at Escape. Nothing can tag that, and
nothing can attach to it. So the note has to exist **before the first keystroke**,
and the whole story follows from that one fact: the creation moves from the end of
a capture to the beginning of one, the buffer stops being the durable thing, and
the durable thing becomes the note file — which is strictly more durable than a
settings row and is what `NoteEditor`'s autosave already maintains.

---

## 1. The design position

### The window is prewarmed; so is the page

NFR-27 gives the panel 300 ms, and `notes_window` spends all of it: the window is
declared in `tauri.conf.json`, created hidden at startup, never destroyed, and the
hotkey path is `set_position` → `show` → `set_focus`. This story applies the same
trick to the document. `useCaptureDraft` resolves the page:

- **at mount** — app startup, with nobody waiting; and
- **immediately after each dismissal**, while the window is hidden.

A resolve on `show` would put an IPC round trip in front of the first keystroke,
which is the one thing this surface may not do. By the time the chord is pressed
the window holds a live, focused `EditorView` on a live note.

The editor's several-hundred-kilobyte chunk is behind `NoteEditor`'s own
`import()` and is fetched in that same startup window. `capture-main.tsx`'s doc
comment used to say the panel imports "no editor… because NFR-27 gives it 300 ms";
that sentence has been replaced rather than quietly left, because it is now false
and a future reader would find it and believe it.

### One creation path, and its notices are shown

`notes_capture_draft` calls `create_note` — the function `notes_create` calls —
with `Seed { capture: true, .. }` (via 45.16's `seed::capture`) and a real
`notices` vector. Capture is a caller of the creation path, never a second one.
The sentence 44.7 composes when a configured template cannot be read now reaches
a person instead of only the log.

### Reuse is decided from the bytes, not from a claim

The obvious way to avoid littering the vault with empty notes is for the window to
say "I did not type anything". A caller that says that can be wrong, and being
wrong costs the user in both directions: reuse a written page and the next thought
lands on top of the last one; tear off an untouched one and every idle press of the
hotkey leaves a file behind.

So Rust decides, from the file. `registry::CaptureDraft { note_id, pristine }`
records what creation actually wrote — read back off the note, not assembled — and
`is_untouched(body)` compares. **Not `is_empty`**: 45.16's capture template makes a
brand-new page non-empty, so emptiness would tear off a fresh page on every
dismissal. Whitespace is trimmed at the ends only, because the round trip through
the editor and `notes_save` is entitled to settle a trailing newline nobody typed,
and interior blank lines between two paragraphs are writing.

### The pointer is keyed from the first commit

`notes.capture_draft.<key>`, not one global slot. One slot is the assumption that
exactly one capture window exists; 45.15 opens several, and two windows sharing a
pointer would each hold the other's note. Keyed from day one rather than
retrofitted.

### The window is quick capture's; the document is not

`CaptureDocument` takes a note and shows it. `CaptureDraftDocument` resolves the
hotkey window's page and then renders `CaptureDocument`. 45.15's chrome arrives
through a `chrome?: (dismiss) => ReactNode` slot **that is handed the document's
own dismissal**, so the close button and Escape are one act: a close button that
hid the window itself would skip the force-flush and the re-arm, and would be a
second spelling of one sentence.

### The webview is a separate realm, so AD-58's singleton is not a constraint

The brief (and my own initial reading) assumed `notesEditorStore` being a module
singleton forced a per-document lift before capture could mount the editor.
**Verified mechanically rather than argued:** `tauri.conf.json` declares
`quick-capture` → `capture.html`, `visible: false`; `capture.html` loads
`/src/capture-main.tsx`; `vite.config.ts` builds it as its own rollup input. A
separate document is a separate JS realm is a separate module registry. The capture
webview gets its own store the moment it imports the module. `NOTE_PANEL_LIMIT`
bounds panels in **one** realm and is a main-window concern; Main made that ruling
binding and 45.15's several windows are N Tauri windows, N webviews, N realms.

---

## 2. I/O matrix

`summon` = press the capture chord. `dismiss` = Escape, ⌘W/Ctrl+W, or 45.15's
close button — one act, three affordances.

| Input | Rust does | The window shows | On disk |
|---|---|---|---|
| App start | resolve: no pointer → create with `Seed { capture: true }`, store `{ note_id, pristine }` | live editor on the new page | one note, `keeper.capture: true`, plus 45.16's tag/template |
| Summon, first ever | nothing (already resolved at mount) | the same live editor, focused | unchanged |
| Summon, type, dismiss | flush; hide; resolve: body ≠ pristine → **tear off**, create a fresh page | the fresh page | the thought is a note; a new empty page exists |
| Summon, type nothing, dismiss | flush (no-op); hide; resolve: body = pristine → **reuse** | the same page, same caret, same undo stack | nothing new |
| Summon, type, delete it all, dismiss | reuse — the body equals the scaffold again | the same page | nothing new |
| Dismiss ×20 without typing | reuse ×20 | the same page | exactly one untouched note, ever |
| Capture template configured (45.16) | pristine = the expanded scaffold | the scaffold, caret at `{{cursor}}` | scaffolded note |
| Template configured but unreadable | creates plain; 44.7's sentence on `notices` | the sentence above the editor, `role="status"` | plain note |
| Bold/underline/task/code applied in capture | — | the marked text | the marks are in the saved bytes |
| Tag chosen in capture's Properties panel | — | the chip | `tags:` in the note's block |
| `/` menu, emoji, wikilink, table in capture | — | as in the notes pane | same bytes |
| Attach a file in capture | `notes_attach_sources` copies/resolves | the receipt banner | `![[rel]]`, the one spelling, never an absolute path (FR-145) |
| App restart | pointer still names the page; untouched → reuse | the same page | nothing new |
| App killed mid-typing | pointer's pristine is stale; body ≠ pristine → tear off | a fresh page | the thought is safe — it is a note, not a buffer |
| No vault flagged | refuse, `Unsupported` | the reason, `role="alert"`, **no editor** | nothing |
| Hide refuses | — | the reason **above** the still-live editor | the words are saved and still on screen |
| Window opened on an existing note (45.15) | nothing — no create | that note in the same editor | unchanged |

---

## 3. Edge cases

| Case | Behaviour | Why |
|---|---|---|
| Escape while the `/` menu, tag chooser or emoji chooser is open | closes the popup; **does not dismiss** | CodeMirror marks the event handled, and the guard is `event.defaultPrevented`. Without it, closing a completion would destroy the surface the person is mid-way through using. |
| ⌘W and Ctrl+W | both dismiss | `metaKey \|\| ctrlKey`, never a platform test — `src/test/no-user-agent-gating.test.ts` scans text, and it is the pair CodeMirror's own `Mod-` resolves to. |
| Focus on a header button when Escape is pressed | dismisses | the listener is on `window`, not on a wrapper element. |
| Draft note deleted outside keeper | pointer names nothing in the index → fresh page | one of four "make a fresh page" cases, deliberately not distinguished: the answer does not differ. |
| Draft note unreadable | fresh page | a note keeper cannot re-read is not one it should hand back. |
| Pointer row corrupt | fresh page, `WARN` once | a settings row that rotted must cost one note, never a refusal to capture. |
| Pointer cleared | fresh page, **silently** | clearing is the ordinary end of every capture; a warning that fires on the ordinary path is a warning nobody reads. The empty string is the cleared value and the reader agrees — pinned by a test on the raw settings row. |
| Note written but not re-readable at create time | **no pointer stored**, `INFO` | `unwrap_or_default()` here would record a scaffolded page as created blank, and every later comparison would read it as written-on — the tear-off-every-time failure, arrived at from the other side. |
| The re-resolve answers the same note | props unchanged, **no remount** | identity is the note's **id**, not the object Rust freshly allocates. A remount would throw away the caret and the undo stack of a page the person is still holding. |
| A notice, then a re-resolve of the same page | the notice **stays** | notices belong to the note that was created, and a reused page had no create to say anything about. Held together with the note for exactly that reason. |
| A notice, then a tear-off | the notice **goes** | a fresh page must not inherit the last one's sentence. |
| Hide fails, then succeeds | the sentence appears, then clears | a window permanently accusing itself of a failure that has stopped happening is a window whose next real failure is invisible. |
| Dismissal with an unsaved buffer | saved **first**, awaited | Rust reads the file to decide "written on?", and the editor autosaves after 1.5 s idle. Without the await the page reads as untouched and the next thought lands underneath this one (AD-62). |
| Two capture windows | two pointers, two pages | keyed from the first commit. |

---

## 4. What changed

**`keeper-core` (compiles and is tested on this host)**

- `registry.rs`: `notes.capture_buffer` and `get/set_capture_buffer` **deleted**.
  `CaptureDraft { note_id, pristine }`, `CaptureDraft::is_untouched`,
  `get_capture_draft(data_dir, key)`, `set_capture_draft(data_dir, key, Option<&…>)`
  over `notes.capture_draft.<key>`. Two tests.

**`keeper` shell (cannot be compiled on Linux — see §7)**

- `notes_ipc.rs`: `notes_capture_buffer`, `notes_capture_buffer_save`,
  `notes_capture_commit`, `commit_buffer` and `read_buffer` **deleted**.
  `notes_capture_draft(key) -> NoteCreateVm` added, with `resolve_capture_draft`,
  `untouched_draft` and `draft_body`. `notes_capture_hide` reduced from
  `(app, state, commit: bool) -> Option<NoteRefVm>` to `(app) -> ()`.
- `ipc.rs`: the mobile twin of `notes_capture_hide` follows it.
- `lib.rs`: three registrations replaced by one.

**Frontend**

- `client.ts`: `notesCaptureBuffer`, `notesCaptureBufferSave`,
  `notesCaptureCommit` **deleted**; `notesCaptureDraft(key)` added;
  `notesCaptureHide()` loses its argument.
- `src/lib/stores/notes-capture.ts` and `src/hooks/use-notes-capture.ts`
  **deleted** — the textarea's buffer mirror and its hook.
- `src/hooks/use-capture-draft.ts` (new): resolves, re-arms, dismisses.
- `src/components/capture/capture-document.tsx` (new): `CaptureDocument`,
  `CaptureDraftDocument`.
- `src/capture-main.tsx`: mounts the document instead of a textarea. (Story 45.15
  then took this file over and added the URL branch and the chrome; the draft
  branch is unchanged.)
- `src/hooks/use-notes-body.ts`: the body of `useNotesBody`'s `save()` callback
  lifted into an exported `saveOpenNote()`; the callback delegates to it. One
  implementation, reachable by a surface with no editor of its own.

---

## 5. Mutation table

Harness `~/.W3Capture/sweep.py`. Every mutation carries a unique sentinel in both
directions; the anchor must appear exactly once, the mutant zero times before and
once after, and the restore is verified by reproducing the original **byte for
byte** before the next mutation runs.

Gates: `bun run test src/components/capture/` and
`cargo test -p keeper-core --lib registry::tests::`.

**27 sweep mutations plus 6 audit probes = 33, all caught. Four survived their
first probe; all four were real and all four are closed.** The two audit probes
(MUT28–MUT33) are in §6 rather than the table below, because they came from
peers' shapes after the sweep was green rather than from my own list.

| # | File | Mutation | First | Now |
|---|---|---|---|---|
| 01 | use-capture-draft | ask for the wrong window's page (`""`) | caught | caught |
| 02 | use-capture-draft | always replace the held page | **SURVIVED** | caught |
| 03 | use-capture-draft | never replace it when one is held | caught | caught |
| 04 | use-capture-draft | do not save before dismissing | caught | caught |
| 05 | use-capture-draft | re-arm before hiding | caught | caught |
| 06 | use-capture-draft | never re-arm | caught | caught |
| 07 | use-capture-draft | swallow a failed resolve | caught | caught |
| 08 | use-capture-draft | never subscribe to `capture-shown` | caught | caught |
| 09 | use-capture-draft | drop the notices | caught | caught |
| 10 | use-capture-draft | swallow a failed hide | **SURVIVED** | caught |
| 11 | capture-document | ignore `defaultPrevented` | caught | caught |
| 12 | capture-document | Escape no longer dismisses | caught | caught |
| 13 | capture-document | ⌘W only, not Ctrl+W | caught | caught |
| 14 | capture-document | do not pass `notices` down | caught | caught |
| 15 | capture-document | pass the note id as the vault id | caught | caught |
| 16 | capture-document | render the empty list instead of the notices | caught | caught |
| 17 | capture-document | never attach the key listener | caught | caught |
| 18 | use-capture-draft | write the hide failure into the page-error slot | caught | caught |
| 19 | use-capture-draft | never clear a stale hide failure | caught | caught |
| 20 | use-notes-body | `saveOpenNote` writes only when clean | caught | caught |
| 21 | use-notes-body | `saveOpenNote` does not adopt its acknowledgement | **SURVIVED** | caught |
| 22 | registry | compare pristine untrimmed | caught | caught |
| 23 | registry | trim only the start | caught | caught |
| 24 | registry | every page is untouched | caught | caught |
| 25 | registry | one global pointer slot | caught | caught |
| 26 | registry | accept a blank note id | caught | caught |
| 27 | registry | clear the pointer as `"null"` | **SURVIVED** | caught |

### The four survivors, and what each would have shipped

**02 — the reuse check was protecting the notices, and nothing said so.** Always
replacing the held page produces the same `noteId`, so `NoteEditor` never
remounts and no assertion of mine could see it. What it does destroy is the
notices: the re-resolve on `capture-shown` answers the same page with none, so a
sentence saying "keeper couldn't read your capture template" would be wiped off
the screen a second after the window appeared. Closed by two tests — a notice
survives a re-resolve of the same page, and does not survive a tear-off.

**10 — a failure sentence written and never read.** `dismiss` set an error on a
failed hide that no test asserted. A capture window that refused to go away would
have looked exactly like one that filed the thought and went away.

**...and writing that test found a real defect the mutation had hidden.** With
one error slot, `dismiss` did `hide → setError → resolve → setError(null)`: the
re-arm succeeds and clears the hide's failure in the same tick. The sentence was
set and erased before a frame was painted, which is indistinguishable from never
saying anything. **Two producers running one after the other cannot share one
slot.** Split into `error` (there is no page — blocks the editor) and
`windowError` (the window would not hide — rendered beside a live page, because
losing a note over a stuck window is capture's one unforgivable act). Mutations 18
and 19 were added to hold the split.

**21 — a contract stated in a doc comment and enforced nowhere.** `saveOpenNote`'s
comment says it adopts its acknowledgement *"so the later unmount flush does not
re-send the same text against a revision the first write has already superseded —
which Rust would read as somebody else's edit and answer with a conflict copy"*.
Removing `markSaved` left `dirty` true, and `useNotesBody`'s unmount flush — which
fires on the very next tick, because the note swaps — re-sent the same text at the
same `rev`. A conflict copy on a note the person just filed. Closed by asserting
**exactly one** write for that subscription.

**27 — an equivalent mutant in the return value, not in the log.** Clearing the
pointer as `"null"` reads back as `None` exactly like `""` does, so no assertion on
`get_capture_draft` could distinguish them. What it changes is that the reader then
takes the *malformed* branch and logs a `WARN` **every time somebody files a
thought** — a warning on the ordinary path, which is how a real warning becomes
invisible. Closed by making a cleared pointer a first-class silent state in the
reader and by asserting the raw stored row is the empty string, so the writer and
the reader are pinned to one spelling of "cleared".

---

## 6. Shape audit

Run **after** the sweep was green, from shapes traded on `hub` rather than from
extending my own list. Reported separately because the sweep could not have
produced any of it.

| # | Shape | Applied to | Result |
|---|---|---|---|
| 1 | **What composes the input?** | My tests hand-build `NoteCreateVm`. | The TS composers (`CaptureDraftDocument` → `CaptureDocument` props, `CapturePanel` → key) are probed by MUT14/15 and door 3. **The Rust composer `resolve_capture_draft` is untested — §7.** |
| 2 | **Did anything press the button?** | Every affordance. | Bold clicked, Attach clicked, a tag chosen through the real chooser, Escape and Ctrl+W pressed, the chrome's close button clicked. No test stops at the offer. |
| 3 | **A contract in a doc comment, enforced nowhere.** | Six claims in files I wrote. | **One real defect** — `saveOpenNote`'s conflict-copy sentence (MUT21). Four now enforced. One unenforceable and said so (`h-full` under jsdom). |
| 4 | **A fallback for a case that cannot happen.** | Every `??` and `unwrap_or` I added. | **One real defect** — `draft_body(...).unwrap_or_default()` at the create site would record a *scaffolded* page as created blank, so every later comparison reads it as written-on and every dismissal tears off a fresh note. Now an `else` that stores no pointer and says why at INFO. Also removed a `?? ""` in the test that could have turned a missing write into an empty string. |
| 5 | **Assert a fixture is opaque before asserting what reading it produces.** | The "scaffolded" pristine fixture. | Already pinned: `!is_untouched("")` fails if `pristine` is empty, so the fixture cannot silently degrade to the blank case. |
| 6 | **A branch reachable only from a second host.** | `CaptureDocument` has two hosts. | **A real gap.** Every behaviour test entered through the draft window; door 2 had only "it renders". Added a test that applies a mark and saves it **through the note-window host**. |
| 7 | **Assert what you handed on, not only what came back.** | Every boundary. | `notesCaptureDraft(key)`, `notesAttachSources(vault, [two paths])`, `notesOpen(vault, note, fn)`, `notesSave(sub, text, rev, block)` all asserted at the call. The attachment fixture carries **two** files, because a mutation keeping only the first passes every single-item fixture. |
| 8 | **Count tests per entry point.** | Three doors. | Was 15 / 2 / 2. The 2 on door 2 were both "does it render". Fixed — see 6. |
| 9 | **Who writes the field on the boring path?** (W3NoteFile) | `pristine`, `notices`. | `pristine` is written by every create — the boring path. `notices` is written only when a create happens, and both the written and the unwritten case are tested. |
| 10 | **A doc comment naming another module's behaviour is an assertion nobody runs.** (W3Chrome) | Six cross-module claims. | All six checked mechanically, not read. The load-bearing one — **the capture webview is a separate realm** — is now verified from `tauri.conf.json`, `capture.html` and `vite.config.ts` rather than argued. It is the claim Main made a binding ruling on. |
| 11 | **Negative anchors, comment-stripped.** (W2Attach) | 18 deleted names + 2 deleted files. | All absent from code. The scan strips comments first, because this spec and those files name every deleted symbol in prose. |
| 12 | **`mockRejectedValue` builds its rejection at CONFIGURE time.** (W3NoteFile) | My two rejecting mocks. | The stale-error test uses `mockImplementation(async () => { throw … })`. Zero unhandled errors. |
| 13 | **A mock is needed exactly when the boot path reaches the name — eventually.** (W2Attach) | The client mock factory. | `notesTemplateUpdatePreview` added with a comment saying it is reached **only on a slow run**, via `TemplateUpdateOffer`'s four-second idle timer, so nobody trims it as unreached. |
| 14 | **A sentinel query must distinguish your leftovers from a sibling's.** | The restore check. | A bare `// MUT` probe fired on three concurrent sweeps and told me nothing. Narrowed to this story's 27 literal ids. |
| 15 | **`git diff` is blind to new files.** (W3Export) | Three of my files are untracked. | The restore check is presence/absence by literal substring over a file list, not a diff. |
| 16 | **A structural assertion and a textual one beside it are one witness, not two.** (W3Recording) | The registry test asserts `get_capture_draft(…) == Some(first)` (fields) *and* the raw settings row `"notes.capture_draft.note:v1/n1" == Some("")` (bytes). | Checked, and the pairing fails **loudly** in both directions: rename the key prefix and the raw read returns `None` where the test wants `Some("")`. The one place where a literal could have gone hollow — the blank-`note_id` fixture — is built through `serde_json::to_string` rather than typed as JSON, precisely so a field rename cannot make it pass on the malformed arm instead of the arm it is testing. |
| 17 | **Two producers that run one after the other cannot share one state slot.** (found here) | `dismiss`: hide → `setError` → re-arm → `setError(null)`. | **One real defect**, and the shape I am contributing back. They do not race — they are sequenced, and the later one *succeeds*, so it clears the earlier one's failure in the same tick. No mutation of the `catch` arm can show it, because the `catch` arm is correct. Split into `error` and `windowError`, partitioned on what the reader must be able to see at once. |
| 18 | **Correct code with an unpinned invariant** (W2Media) — a third category, neither a defect nor a narrowed claim: nothing is broken, and there is a one-character path to breaking it with no test in the way. | `setDraft((held) => …)` is a functional update carrying the previous page, and the plain `setDraft({ … })` a person in a hurry writes reads as perfectly ordinary React. MUT02 mutates the *condition*; this mutates the *form*. | Probed as **MUT28: CAUGHT**, by the notices test and only by it. Pinned — but only because MUT02 survived the first sweep and forced me to work out what the reuse check was protecting. The difference between a defect and an unpinned invariant is whether today's code happens to be right, which is a property of today. **When you find a shared-slot shape the question is not "is it broken" but "what stops it becoming broken", and if the answer is "the author was careful", that is the finding.** |
| 19 | **An absence with no positive witness** (W3Recording, sharpened by W3Chrome). | Six absence assertions in this file. | Five are paired with a positive using the same literal — `notesCaptureDraft` not-called against called-with, `queryByRole("status")` against `findAllByRole("status")`, `.cm-editor` null against `liveEditor()`, no-`/Users/` against the exact expected embed text. **One was not:** `queryByText(CAPTURE_OPENING_LABEL)` asserted the loading label absent and nothing ever asserted it present, so deleting the loading branch outright would have passed. Closed with a test that holds the resolve open, asserts the label IS shown with no editor, then settles it and asserts the label goes. Probed as **MUT29: CAUGHT.** |
| 20 | **`await` is not a success check when the callee catches its own failure.** (W3NoteFile) | `dismiss` does `await saveOpenNote()`, and `saveOpenNote` catches — it has to, because the editor's caption is fed from the same store. | **A real defect, and the last one found.** The sequenced step after it is *hide the window*. On a refused write capture hid the panel with the reason legible only inside the window that had just disappeared, then asked Rust "was this page written on?" about bytes that never reached the disk — which answers no and hands the same page back. The words survived; the person was told nothing. **UX-DR35's error branch — a failed write leaves the text where it is and the panel open — was the old `commitCapture`'s explicit contract and I lost it in the port.** Closed production-side, not test-side: `saveOpenNote` now resolves `boolean`, and `dismiss` returns early on `false`. Probed as MUT30 (ignore the answer), MUT31 (report a refusal as success) and MUT32 (report nothing-to-write as failure) — **all three CAUGHT.** |
| 21 | **When you replace a mechanism, the contracts it kept are not in the diff — and the second half: ask what it did that nobody thought to promise.** (found here; second half W3Chrome) | The deleted textarea's attributes. | **A real regression.** Its `aria-label="Quick capture"` named the typing surface. CodeMirror's content is `role="textbox"` with **no accessible name** — measured, not assumed, by reading the attributes off a live `EditorView`. So porting the panel to the real editor dropped a promise written down nowhere except as an attribute on code deleted wholesale: no failing test, no diff line to review, and a negative anchor would have confirmed the deletion, which was the point. Named once in `NoteEditor` via `contentAttributes` rather than in the capture window — a capture-only label would be the second configuration of one editor that AD-93 exists to prevent, **and the notes pane had the identical gap**. Probed as MUT33: **CAUGHT.** |
| 22 | **Does this thing NAME something, and does anything check the thing it names exists?** (W3Recording) — a reference and a dangling reference are the same bytes, so no render assertion can tell them apart. | Every `aria-controls` / `aria-describedby` / `aria-labelledby` / `htmlFor` / `id` in my three files. | **Clean, and structurally so: there are none.** My surfaces name themselves (`role="alert"`, `role="status"`, the editor's own `aria-label`) and never point at another element, so the hazard has no surface here rather than being avoided by care. Recorded as **checked**, not assumed. |
| 23 | **What did widening a shared vocabulary make false ANYWHERE?** (W3Chrome) — a set is a global fact and a grep scoped to your own file is a local answer. | Naming the shared editor `"Note"` adds a value to the tree's accessible-name vocabulary. | **Clean, checked whole-tree rather than in my own file.** No other control has the accessible name `Note`; the only `getByRole("textbox", { name: "Note" })` is mine; no `getByLabelText` matches it. And because this touches every surface that mounts `NoteEditor`, the consumer set was run rather than reasoned about — 400/401 × 3, the one red disproved by removal. |

---

## 7. What I could not verify here, and why

**The `keeper` shell crate does not build on this host.** Measured, not assumed:
`cargo check -p keeper --lib` fails in `glib-sys`/`gobject-sys` build scripts —
missing system GTK development packages, before a single line of keeper is
compiled. So the following have **never been compiled, let alone run**:

- `notes_capture_draft`, `resolve_capture_draft`, `untouched_draft`, `draft_body`;
- the reduced `notes_capture_hide` and its mobile twin;
- the `lib.rs` registration swap.

Everything decidable was pushed into `keeper-core`, where it is proved:
`CaptureDraft`, `is_untouched` and the keyed pointer have 2 tests and 6 mutations.
What is left in the shell is wiring — read a pointer, call `create_note`, write a
pointer — and its *shape* is asserted from the frontend against a mocked command,
which is a claim about the contract and not about the implementation.

**Nothing has executed in a second webview.** Every test mounts the editor in the
main window's realm under jsdom. The realm argument is verified from the build
configuration (§1) and is the reason there is nothing capture-specific in the
document, but no code of mine has ever run inside `capture.html`.

**No editor chunk has ever been fetched over a real network of modules.** jsdom
resolves the dynamic `import()` synchronously off disk. The NFR-27 claim — that the
chunk lands during the hidden-startup window and not in front of the first
keystroke — is an argument from where the resolve is called, not a measurement.

**A witness inside the test file is a convention, not a seam.** The paired
positive added for `CAPTURE_OPENING_LABEL` (audit 19) is the same weight as
W3Chrome's `no-such-glyph` and W3TagsDelete's `title` scalar: better than an
unpaired absence, worse than a parameter. The durable form is W1Registry's
`resolveViewerComponent(file, {})` — an argument that exists for no reason
except to let the fallback be exercised against an explicitly empty table
forever, so the honest version is the easy one to write.

**I first wrote here that the durable form would be `CaptureDraftDocument`
taking its loading element as a slot. That is wrong and I am striking it rather
than leaving it.** A slot prop makes the element injectable; it does nothing
about the hazard, which is that an absence assertion can lose its subject. W2Media
named the general rule while refusing to build a shared assertion helper for the
same reason: **a seam has to live in the production API, because a test-side
construct can simply not be called.** `resolveViewerComponent(entry, components)`
works because production code cannot bypass the parameter.

So the honest statement for this row is that **I do not have a seam for it.** The
guard is a paired witness inside the test file — a convention, and I have said so
rather than dressing it as more. Four stories in this wave arrived at "owed"
against the same model, and W2Media's framing is the one that should carry into
the retro: these are not four rows of debt, they are one category — every hollow
assertion any of us found was found from the test side, and every durable repair
lives on the other side of that boundary.

**Two reds in the named acceptance command are other stories', attributed.**
`bun run test src/components/notes/ src/components/capture/ src/hooks/ src/capture-main.test.tsx`
→ 954/956, both reds in files I do not own and with zero of my symbols in the
output: `note-file-links.test.tsx` (45.18, announced mid-sweep) and
`properties-panel.test.tsx` (a frontmatter-serialisation assertion whose *title
changed between two of my runs*, so that file is being edited live).

**The shared editor's consumers, because I changed a shared component.** Naming
the editable region touches every surface that mounts `NoteEditor`, so "my 25
tests pass" says nothing. `new-note-caret` + `format-toolbar` +
`src/components/notes/editor/` + `attach-entry-points`: **400/401 across 19
files**, three repeats. The one red is `file-embed.test.tsx > turns a data embed
into a panel…` at 5006 ms — W2Embeds', reported by three peers before I touched
anything. **Verified by removal rather than by argument:** with my line deleted
it failed twice, with it restored it passed twice. The correlation is inverted,
which is what load looks like and what causation does not.

**And one EXIT=1 of my own that was not a test failure, recorded rather than
dropped.** One repeat of my own suite came back 18 failed / 5 passed — not the
timeout pattern, a module-load collapse. Cause: `Transform failed` in
`src/components/notes/editor/text-splice.ts`, a sibling's half-written file,
reached from my suite through `live-preview.ts`, which `NoteEditor` lazily
imports. **The blast radius of a broken file is its transitive importers, not
its feature.** Attributed by reading the error rather than the test names,
confirmed by `bunx esbuild` on that file, and the three repeats recorded above
were taken after it parsed again. A single red under load is not evidence, and
neither is a single green after it. My own scope
is EXIT=0.

### First checks on the macOS gate, in order

1. **Press the chord.** The window must appear with a caret already blinking in a
   CodeMirror editor — not a textarea, not an empty frame. If it is empty, the
   resolve failed and the reason is in the log at `INFO`.
2. **Press the chord and press Escape, five times, typing nothing.** Then look in
   the vault. There must be **exactly one** untouched note, not five. This is the
   single behaviour with no equivalent in the old design and the one most likely
   to be wrong in a way tests cannot see.
3. **Type a thought, press Escape, press the chord again.** The window must be
   blank. If the last thought is still there, the force-flush or the tear-off is
   not working and the next thought will land on top of it.
4. **In a capture window: apply Bold, add a tag in Properties, attach a file from
   the Desktop.** Then open the note in the notes pane and confirm all three are
   in the file, and that the attachment is `![[attachments/…]]` and not an
   absolute path.
5. **Follow an external link from a capture window.** `opener:default` was granted
   for this (Main's ruling); a silent rejection means the capability did not take.

---

## 8. Deliberately NOT done

- **No per-document lift of `notesEditorStore`, and `NOTE_PANEL_LIMIT` is
  untouched.** The brief asked for it; it is not needed, because the capture
  webview is its own realm (§1). Main ruled this binding. The limit bounds panels
  in the main window and is owed to whoever wants two note panels there.
- **No capture-specific editor configuration.** No trimmed toolbar, no hidden
  backlinks panel, no smaller header. The whole point of AD-93 is that there is
  one editor; a capture-only variant is how you get two again. The header is
  cramped in a 560 px window and that is a layout problem for 45.15's chrome, not
  a reason to fork the component.
- **No "discard" affordance, no save button, no title field, no folder picker.**
  UX-DR35 refused these permanently and the reasons did not change when the editor
  arrived.
- **The draft note is not deleted when it is abandoned.** It is reused instead.
  Deleting a note is 45.17's, and an untouched page is not a note anybody lost.
- **Nothing reads the platform.** The close chord is `metaKey || ctrlKey`.
- **No dependency added.**

---

## 9. Ledger

- **DW-172's third dead listener is closed.** `listenNotesCaptureShown` had been
  declared since Epic 36 and called from nowhere; it is now the belt to the
  re-arm, with a test that drives it.
- **Owed, and named rather than remembered:** `resolve_capture_draft` has no test
  of any kind — not because it is hard to test but because the crate holding it
  cannot be compiled on this host. The first person on a macOS box should run the
  five checks in §7 before believing any of §2.
