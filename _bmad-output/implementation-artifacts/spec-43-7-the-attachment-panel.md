---
title: 'Story 43.7: The Attachment Panel'
type: 'feature'
created: '2026-08-09'
status: 'review'
blocking_condition: ''
baseline_revision: '9f7150d'
final_revision: ''
review_loop_iteration: 0
followup_review_recommended: false
context:
  - '{project-root}/docs/project-context.md'
  - '{project-root}/_bmad-output/planning-artifacts/epic-43-a-note-can-show-you-the-file.md'
  - '{project-root}/_bmad-output/implementation-artifacts/spec-43-5-one-attachment-vocabulary.md'
  - '{project-root}/_bmad-output/implementation-artifacts/spec-43-9-the-slash-menu-can-open.md'
  - '{project-root}/_bmad-output/implementation-artifacts/spec-42-4-the-note-stub.md'
---

<intent-contract>

## Intent

**Problem:** the two ends of this feature have been finished for two stories and nothing joins them.
Story 42.4 writes `session:`, `recording:` and `files:` into a recording note and the properties
panel renders those paths with actions; Story 43.5 taught the body to turn `![[…]]` into a player,
an image, an audio element or a chip. The only route from the first to the second is to type `![[`
and a path — a path written relative to the recordings destination root, which is not the frame the
user's Finder window shows them and not a string anybody remembers. So the panel knows the files, the
body can show the files, and the user has to retype what keeper already knows.

**Approach:** one panel in the editor, beside the properties one, listing the note's own attachments
with a single control each. Pressing it writes `![[<the note's own path>]]` at the caret. That is
all it does: it composes no path, resolves no file, requests no byte and renders nothing. The
insertion goes through a new `insertAtCursor` on the editor runtime as an ordinary local edit, so
it enters the undo history and is reported to Rust exactly as a keystroke would be.

## Boundaries & Constraints

**Always:**
- What the panel writes is the ordinary Obsidian embed and nothing else. No marker, no attribute, no
  wrapper — a note built with this panel is byte-identical to one typed by hand, which is the only
  reason it stays a note Obsidian renders and the only reason `recording-embed.ts` recognises it.
- The path written is the NOTE's own, from its `files:` key. Never the index's current path, even
  when Story 40.4 has renamed the folder underneath: the note is written in one frame and must stay
  in it.
- No absolute path is rendered or held (FR-145). The row shows the file name, its tooltip is the
  note's relative path, and unlike the properties panel this surface never even reads
  `absolutePath` — it acts on no file.
- The frontend joins no path onto any root (AD-65). Every string it writes came out of the note.
- The kind on a row is Rust's answer from `recording_note_targets`. When Rust cannot answer, the row
  claims no kind rather than reading the extension itself.
- The insertion is the user's edit: in the history, reported through `onEdit`, and it hands focus
  back to the editor.

**Never:**
- No new IPC command; `recording_note_targets` already answers this.
- No second embed syntax and no second embed regex. `WIKILINK` is imported from the editor's module.
- No writing to the note's frontmatter. This panel changes the body and only the body.
- No listing of the session folder's contents. That is a file browser, and Story 43.8 is the file
  browser.
- No new dependency, and no edit to `recording-embed.ts`, `live-preview.ts` or `files-pane.tsx`.

## I/O & Edge-Case Matrix

| Scenario | Input / State | Expected | Error |
|---|---|---|---|
| The note's own files | `files:` lists two of the session's four | exactly two rows, in the note's order | none |
| A file only the index has | `camera-0000.mov` in the folder, not in `files:` | no row — it is the session's file, not this note's attachment | none |
| The session folder | `recording:` | never a row: no element renders a directory | none |
| Insert | press the row's control | `![[<the note's own path>]]` at the caret, verbatim | none |
| Insert after a retitle | note says `…/standup/…`, index says `…/standup retitled/…` | the NOTE's path is written | none |
| Caret mid-document | caret at offset 5 of `alpha\nbeta\n` | `alpha![[…]]\nbeta\n` — not appended, not at 0 | none |
| Focus | the click took focus to the button | focus returns to the editor; the next keystroke types | none |
| Already embedded | body holds `![[…/screen-0000.mov]]` | the row reads "In the note"; no control exists to press | none |
| Embedded under the old name | body holds the retitled path, `files:` the written one | still "In the note" — one file, matched by name | none |
| A plain wikilink | body holds `[[…/screen-0000.mov]]` | still offered: a mention is not an embed | none |
| Kind | index places the file | the row says `video` / `image` / `audio` / `file` | none |
| A file the index cannot place | listed in `files:`, gone from disk | no kind claimed; still listed and still insertable | none |
| Session unresolved | `recording_note_targets` → `null` or throws | every row still listed and insertable; one line says keeper cannot locate the session | swallowed |
| Answer still in flight | the resolve has not returned | rows listed and insertable, no kind, and NO "can't locate" line — that is a claim, and it is not true yet | none |
| The next note, before its answer | panel stays mounted, session changes | the previous session's kinds are dropped, not carried onto a file with the same name | none |
| No `session:` | any other note | one sentence: this is not a recording note. Not an empty list, and no IPC call | none |
| `session:` but no `files:` | a session that closed no segment | a different sentence: this recording note lists no files | none |
| Hand-written scalar `files:` | `files: a/b.mov` | one row — a human edited the block and meant the one file | none |
| Nested map under `files:` | a shape this reader does not have | no rows, rather than a guess | none |
| Pressed before the editor chunk lands | runtime not yet built | nothing is written; no throw — by construction (`runtimeRef.current?.`), not asserted | none |

</intent-contract>

## Code Map

- `src/components/notes/attachments-panel.tsx` — **new, and the whole of the story.** Reads the
  note's `files:` through `readFrontmatter`, asks `recording_note_targets` what each file is, and
  renders one row each. `noteAttachments` and `embeddedAttachmentNames` are the two exported
  readings; the embed string itself is spelled at its single call site, so the test asserts the
  bytes rather than agreeing with a helper.
- `src/components/notes/note-editor.tsx` — three seams and nothing else: `insertAtCursor` on
  `EditorRuntime` (`state.replaceSelection`, unannotated, then `focus()`), an `Attachments` header
  button beside `Properties`, and the panel below the properties panel.
- `src/components/notes/attachments-panel.test.tsx` — **new.** Seventeen tests in two halves; the
  second mounts the real `NoteEditor`.

Not touched: `recording-embed.ts`, `live-preview.ts`, `slash-menu.ts`, `properties-panel.tsx`
(imported, not edited), anything in Rust, anything in `src/lib/ipc/gen/`.

## Tasks & Acceptance

**Execution:**
- [x] A panel listing the note's own attachments, with the kind Rust decided.
- [x] Insertion at the caret, as the user's own edit, through a new runtime method.
- [x] Duplication made unavailable rather than guarded: the control is absent once the body has it.
- [x] Two distinct empty states, one per fact.
- [x] The slash-menu question decided and written down (see Design Notes).
- [x] Tests: every matrix row reachable in jsdom, half of them through the real editor.
- [x] Reverts proved: sixteen mutations, each applied, run, and watched to fail.

**Acceptance Criteria:**
- `bun run test src/components/notes/attachments-panel.test.tsx` passes: 17 tests.
- The neighbours still pass: `editor/tab-wiring.test.tsx` and `properties-panel.test.tsx` — 33 tests
  together with this file's. `notes-pane.test.tsx` mocks `NoteEditor` out entirely and is unaffected
  by this story; it was red at the time of writing from a sibling wave-2 edit to
  `note-filter-bar.tsx` (`onRemove is not a function`), which is not this story's.
- The panel lists exactly the note's own attachments; inserting writes the text a user would type,
  at the cursor; inserting the same attachment twice is not possible; the no-`session:` state is its
  own sentence.

## Design Notes

### Why a panel and not more slash commands

Story 43.9 made `/` open for the first time three hours before this story started, and the risk is
real: two affordances that both put things into a note is how a surface stops being obvious. So the
split had to be a boundary somebody can state, not a coincidence of who built what. It is:

**`/` answers "what can this editor insert?". The panel answers "what does this note have?".** The
first is a closed table of six literal insertions, the same in every note in every vault. The second
is a per-note fact that comes out of the note's own frontmatter. A slash command cannot know a note's
attachments, and a command table that changed shape per note would stop being a table the user can
learn.

Four things follow from that, each of which would have had to be relitigated to put attachments
behind `/`:

1. **The trigger grammar.** `/` fires at the start of an otherwise-empty line with nothing after the
   caret, and Story 37.6 called that non-negotiable. But an embed of the whiteboard photo belongs in
   the middle of the sentence describing the whiteboard. Attachments-as-slash-commands would be
   attachments you can only insert on a blank line, or a widened trigger — and widening it is a
   change to a grammar two stories have just defended.
2. **Synchrony.** Every slash command is `(now: Date) => string`, computed at accept time.
   Attachments come from an IPC call that can answer `null`. `slashMenuSource` is synchronous today;
   making it async for one row's sake would put the whole menu's open latency behind a call that
   nothing else in it needs.
3. **The states a row has to carry.** "Already in this note" and "keeper can't locate this file" are
   the two most useful things the panel says, and a completion menu has nowhere to say them. A
   completion offers; it does not report. The first of those states is also how duplication is made
   impossible here, and there is no equivalent move inside a menu.
4. **The empty states.** "This is not a recording note" is the answer for most notes in the vault.
   A menu's answer to "nothing to offer" is to not appear, which is the exact confusion the epic's
   AC names: silence reads as "no attachments" when the truth is "not that kind of note".

**What I did not do, and would do if this comes back.** If attachments ever need to be reachable
without the mouse, the honest form is a completion source on `![[` over the session's targets — an
extension of `wikilink.ts`'s existing grammar, triggered by the syntax the user is already typing.
That keeps the trigger tied to the thing being written, needs no new table in the slash menu, and
composes with this panel rather than competing with it. It is a different story because it is a
different question (how do I type this?) from the one this story answers (what does this note have?).

### The other decisions

**The list is the note's `files:`, not the session folder's contents.** The index legitimately holds
files the note does not list — a rotation that closed after the stub was written, a file dropped into
the folder by hand. Offering those would make this panel a second file browser inside the editor,
with no tree, no exclusions and none of the rules Story 43.8 wrote for exactly that surface. "The
note's own attachments" is the AC's wording and it is also the smaller, defensible thing: this panel
is about the note.

**It writes the note's path, never the index's.** After a Story 40.4 retitle these disagree, and the
index's is the one that is "correct" on disk today. Writing it in anyway would leave the note holding
two frames for one session — a `files:` list under the old name and a body embed under the new — and
the next reader would have to work out which is authoritative. The note is one document written in
one frame. The widget resolves by file name precisely so the older frame keeps working, which is what
makes deferring to the note safe rather than merely tidy.

**Duplication is unavailable, not guarded.** The row renders either the control or the words "In the
note", from one boolean. There is no second check inside the click handler, because there is no
second press to check: a control that does not exist cannot be pressed twice. This is 43.3's shape
applied to a different problem — include-and-exclude was made unrepresentable rather than resolved by
precedence, and so is embed-it-twice.

**"Already there" is matched by file name.** Same join key as `attachmentTargetFor` and the
properties panel's `targetFor`, same reason: the note's frame and the index's frame disagree after a
retitle. It also means the panel's idea of "already embedded" and the widget's idea of "this embed is
that file" cannot drift — if they did, the panel would offer a second embed that renders the same
video twice.

**`WIKILINK` is imported, not respelled.** The epic's hard rule is "no second embed syntax", and a
second regex for the one syntax is how a second syntax begins. The import costs nothing that matters:
`wikilink.ts`'s CodeMirror imports are type-only and its one runtime import is the IPC client this
file already pulls in, so nothing from `@codemirror/*` enters the eager chunk. This is the opposite
call from 43.5's — that story respelled two label strings rather than import React into a CodeMirror
widget — and the two agree on the principle: import when the shared thing is the meaning, respell
when the shared thing is a word and the import would drag a runtime across a boundary.

**`insertAtCursor` is `replaceSelection` and is deliberately unannotated.** `applyExternal` and
`placeCaret` both carry `Transaction.remote` and `addToHistory.of(false)`, because they are keeper
acting on someone else's behalf. This one is the user acting: annotated `remote` it would be invisible
to the update listener, which means invisible to `onEdit`, which means the attachment would appear in
the buffer and never reach the file. It also would not be undoable. The one-line difference is the
whole semantic of the method and the doc comment says so.

**Focus goes back to the editor.** A click on a panel button takes focus off the content DOM and
fires the editor's blur-save. Without the explicit `focus()` the insert lands and the next thing the
user types goes nowhere. Cheap to write, easy to omit, and it is the difference between inserting and
interrupting.

**The kind is displayed as a word, not an icon.** `files-pane.tsx` has a `KIND_ICON` table keyed on
the wire type; copying it here would be the second table for one vocabulary — the thing 43.5 exists
to prevent — and the panel's job is to say what will land, which a word does without a legend.

### What the reverts proved

Sixteen mutations. Each was applied to the shipped source, the suite run, the source restored. All
17 tests pass unmutated.

| Mutation | Failed | Caught by |
|---|---|---|
| Insert at the end of the document instead of at the caret | 2 | `writes the embed at the caret…`, `leaves the other attachment insertable…` |
| Drop the focus hand-back | 1 | `writes the embed at the caret, and hands focus back` |
| Unwire the panel from the runtime (`onInsert` writes nothing) | 3 | all three real-editor insertion tests |
| Always offer Insert; never notice the body has it | 3 | `cannot write the same attachment twice`, `offers no insert…`, `counts an embed written under the folder's old name…` |
| Match an existing embed by whole path, not by file name | 3 | the same three |
| Treat a plain `[[wikilink]]` as an embed | 1 | `does not mistake a plain wikilink for an embed` |
| List the session folder's files instead of the note's | 14 | including `lists exactly the note's own attachments…` and `hands out the text a user would type…` |
| Insert the file name instead of the note's own path | 4 | `hands out the text a user would type…` and all three editor tests |
| Drop the no-session sentence and fall through to a list | 2 | `tells a note with no session…`, `says the note is not a recording note…` (real editor) |
| Say the same sentence for both empty states | 1 | `says something different again when a recording note lists no files` |
| Drop the "can't locate this session" line | 2 | `still lists and still inserts…`, `makes no claim about the session while the answer is still in flight` |
| Guess a kind for a file the index cannot place | 1 | `claims no kind for a file the index cannot place…` |
| List `recording:` as an attachment too | 7 | `lists exactly the note's own attachments…` and six others |
| Ignore a hand-written scalar `files:` | 1 | `reads a hand-written scalar files: as the one attachment it plainly is` |
| Say "can't locate" while the answer is still in flight | 1 | `makes no claim about the session while the answer is still in flight` |
| Keep the previous note's targets when the session changes | 1 | `does not label the next note's files with this note's kinds` |

The focus mutation is the row worth reading. The assertion passed under mutation the first time it
was written, because `fireEvent.click` moves no focus in jsdom — so `document.activeElement` was
still the content DOM whether or not anything had handed it back. The test now focuses the button
first, the way a real pointer does on the way down, and asserts that it did. That is the difference
between an assertion and a decoration, and it was found by mutating rather than by reading.

### What could NOT be verified here, stated plainly

- **Nothing was run in a real webview.** jsdom renders no video, decodes no image and lays nothing
  out. The real-editor half proves the text reaches the buffer through a real `EditorView`; it does
  not prove that the `<video>` the widget then builds for that text plays. That path is 43.5's and
  shipped.
- **The caret assertion is about document offsets, not about a rendered caret.** CodeMirror's measure
  pass is shimmed away in jsdom (the same `getClientRects` shim `tab-wiring.test.tsx` uses). What is
  asserted is that the insert landed at offset 5 of `alpha\nbeta\n` rather than at the end — which is
  the failure a user would see — not that the blinking caret was visually where they left it.
- **Focus is jsdom's `activeElement`, not a real focus ring.** A browser's focus behaviour around a
  contenteditable losing and regaining focus mid-click is more intricate than jsdom's; what is proved
  is that `editorView.focus()` is called and takes effect, not that no frame of focus flicker exists.
- **The very first paint is not observable here, and one mutation proved it.** Changing the
  `useState` initial value from `undefined` to `null` — which in a browser would flash "keeper can't
  locate this session" for the frame between commit and the first effect, because `useEffect` runs
  after paint — failed nothing. Testing Library's `render` wraps the mount in `act`, so effects have
  already run by the first assertion. What the suite does prove is the longer window: the whole time
  the IPC call is in flight, which is the one a user would actually see. The initial value is correct
  by reasoning, not by test, and it is written down here rather than claimed as covered.
- **No Rust ran.** This story adds none and changes none. `recording_note_targets` is exercised
  through a mock at the client boundary, exactly as `properties-panel.test.tsx` exercises it.

## Deliberately not done

- **A keyboard shortcut for the panel.** Properties has `Mod-Shift-p`; an `Attachments` binding would
  need a free key nobody else in this wave is claiming, and picking one blind while three agents edit
  the same keymap is how two stories bind the same chord. The header button is reachable and the
  panel is not a hot path.
- **Inserting the session folder.** `recording:` names a directory; there is no element for one, so an
  embed of it renders as the link it already was. Offering it would be offering a no-op.
- **Padding the insertion with newlines.** The panel writes `![[…]]` and nothing around it, because
  that is what "byte-identical to what a user would type" means. A user who wants it on its own line
  presses Return, exactly as they do for everything else in the buffer.
- **Reordering, removing or adding to `files:`.** This panel does not write frontmatter. Editing the
  list is the properties panel's surface, and it deliberately keeps those two keys read-only because
  they are keeper's record of where bytes landed (Story 42.4).
- **Embeds inside code fences.** `embeddedAttachmentNames` scans the whole body, so an `![[…]]`
  written inside a ``` fence counts as present and the row will read "In the note". `keeper-core`'s
  link extractor skips code and this does not; matching it would mean a markdown parse in a React
  render on every keystroke. The consequence is one row saying "In the note" when the reader can see
  it is not — visible, harmless, and reversible by deleting the fence's line. Worth revisiting only
  if somebody actually writes an embed inside a fence in a recording note.
- **Live reaction to a session folder changing on disk.** The targets are resolved once per note, as
  the properties panel resolves them. A file that appears while the note is open needs the note
  reopened — and it would not be in `files:` anyway, so it is not this panel's row to show.
