# Spec 51.5 — Note mode

story: 51.5
status: in-progress
branch: `feat/51-5-note-mode` (on 51.4)
binds: FR-294; AD-88 (one write path per surface), Story 50.3, Story 50.4
sentinel: `MUT51-5`

<intent-contract>

**The ask.** *"nie jest to prawdziwy note edytor jak w notes (chcialem preview, source, note)"*

**Problem.** The file surface has exactly two modes — `Preview` (rendered) and `Source` (raw, editable)
— `raw-rendered-view.tsx:133-141`, default rendered. The owner's first two words are literally those
labels; the third is missing. And the missing mode is **not a new renderer**: the Preview tab already
mounts the note editor's live-preview layer (`markdown-preview.ts:126`), clamped by two facets at
`:118-123` with the reason *"Editing markdown IS the note editor, which has a save path and a conflict
story of its own; wiring an arbitrary file into it would be a second write path, which AD-88 exists to
prevent."*

**That refusal has partial teeth, and this story respects the half that holds.** The half with teeth is
"a second WRITE path". So Note mode adds **no** write path: it edits the same buffer the Source tab
edits and saves through the same explicit Save. The half that does not hold is that rendering needs a
note — it does not: `livePreview`'s only required option is a `vaultId`, which the file surface already
supplies (`""` outside a vault), and note identity is needed by persistence only.

**Approach.** A third mode over one buffer. Preview stays read-only; Source stays raw; Note is
live-preview and editable, and all three share the file's single content state and its Save.

**Always.**
- One buffer, one dirty flag, one Save. Switching modes does not lose an unsaved edit.
- `setContent`, never remount-on-text: `MarkdownPane`'s effect was keyed `[text]`
  (`raw-rendered-view.tsx:348`), which destroys the view — caret, undo stack and scroll — on every
  keystroke, so the buffer is adopted through `TextEditorMount`'s pattern
  (`text-editor-host.ts:292-301`) instead.
- What the pane IS rebuilt on: the mode, the FILE, and the options the decoration layer is built with.
  The review pass found the first cut keyed on `[editable, fileName]` and that is two defects: a file
  whose vault hydrates a frame after the first paint kept the out-of-vault degrade for the life of the
  panel, and two files with one basename in two directories — which story 51.1 makes an ordinary
  session layout — shared one view and one undo stack. The file is therefore the coordinates the
  buffer was **loaded from** (`FileOrigin`, carried out of `useTextBuffer`), never the display name.
- An adoption that fails is a sentence, not an exception: `setContent` answers `null` or the same
  refusal shape construction answers with, and the host falls back to Source out loud (AD-88).
- Note mode is offered for **markdown, writable** files only — the same predicate story 50.3 built,
  including Rust's own `writeRefusal`.
- The remembered mode preference validates against the widened vocabulary, so an old cookie value still
  resolves and a new one is not silently reset (`view-mode.ts:41-57`).
- Outside a vault, the degrades story 50.3 measured are unchanged and already worded: embeds become a
  wikilink with its target, galleries say keeper is not listing here, the CSV table refuses with a
  sentence.

**Block if.**
- The file is not markdown → two modes, as today.
- The buffer is read-only (`workspace/`, an oversize file) → Preview and Source only, and Note is
  absent rather than present-and-refusing.

**Never.**
- Never autosave. There are three recorded refusals, and the write path has no revision guard —
  `syncWriteEntry(profileId, subpath, content)` is last-write-wins (`use-text-file.ts:301`). An
  autosaving live-preview editor over that is a data-loss machine, and the Save button is the guard.
- Never a second renderer, a second markdown extension set, or a second save path.
- ~~Never make Note the default: a person opening a file to read it should not land in an editor.~~
  **SUPERSEDED by story 52.3** (epic 52, item 9): the owner asked for the reverse twice, so Note is
  now the default wherever it is offered. A per-file remembered choice still wins, so nothing he had
  already clicked changed under him.

**I/O and edge-case matrix.**

| # | input | expected |
|---|---|---|
| 1 | a session markdown file | three tabs: Preview, Source, Note; ~~Preview is default~~ → **Note is default since story 52.3** |
| 2 | Note mode, typing | the text renders live and the buffer is dirty; Save writes it |
| 3 | Note mode, an edit, switch to Source | the same text, unsaved, no loss |
| 4 | Source, an edit, switch to Note | the same text, and the caret does not jump to the top |
| 5 | typing in Note | the view is NOT rebuilt (undo still works across ten keystrokes) |
| 6 | Preview | still read-only: typing changes nothing |
| 7 | a `workspace/` markdown file | no Note tab; the read-only sentence unchanged |
| 8 | a `.rs` file | no Note tab, no Preview — unchanged |
| 9 | an oversize file | no Note tab |
| 10 | a cookie holding an old two-value mode | resolves as before |
| 11 | a cookie holding `note` | Note mode is restored |
| 12 | Note mode outside a vault, a file with `![[x]]` | the documented degrade, not a crash |
| 13 | `Mod-s` in Note mode | saves through the same path as Source |
| 14 | the note editor's own suite | untouched and green |
| 15 | an in-vault file whose vault list arrives after the first paint | the pane is rebuilt with the vault; the buffer moving still does not rebuild it |
| 16 | two files named `plan.md`, in `log/` and at the root, opened into one panel | two views: no undo, and no save, reaches back into the other file |
| 17 | the pane refuses a change it is handed | the refusal is a sentence, the source is shown, the panel stands |

</intent-contract>

## Code Map

| file | change |
|---|---|
| `src/components/viewers/view-mode.ts:41-57` | `ViewMode` widens to `"raw" \| "rendered" \| "note"`; `isViewMode` follows; the default is unchanged |
| `src/components/viewers/markdown-preview.ts:100-140` | the two clamping facets become a parameter; the module doc records that the refusal's write-path half still holds and how |
| `src/components/viewers/raw-rendered-view.tsx:133-141,478-521` | a third tab; `MarkdownPane` gains `setContent` instead of remount-on-text and reports edits upward; it is rebuilt on the mode, the file and the preview options |
| `src/components/viewers/use-text-file.ts` | `FileOrigin` — the profile-or-vault id and the relative path the buffer was read from — carried out of the loader, because the two hosts address a file differently and neither may derive the other's coordinates (AD-65) |
| `src/components/viewers/text-viewer.tsx` | the raw editor is rebuilt per file for the same reason, which it never was |
| `src/components/notes/editor/file-embed-host.tsx` | the embed states the coordinates it can prove: its vault, and the bracket text it was asked for |
| `src/components/viewers/text-file-frame.tsx` | the third mode is offered under the same markdown-and-writable predicate as the writing tools; the loader's `loadedFrom` is handed to the views |
| tests | rows 1–17 in the viewer suites; row 14 is the note editor's own suite, unedited |

## Tasks & Acceptance

- [ ] the mode vocabulary widened, cookie-compatible (rows 10–11)
- [ ] an editable live-preview pane over the shared buffer, with `setContent` (rows 2–5)
- [ ] offered only for writable markdown (rows 7–9)
- [ ] Preview still read-only (row 6); no autosave anywhere
- [ ] `docs/notes.md` / `docs/sessions.md`: three modes and what each is for
- [ ] the pane is rebuilt on the file and on the preview options, and on nothing else (rows 15–16)
- [ ] a refused adoption is reported rather than thrown or swallowed (row 17)

**Acceptance.** The owner opens a session log, presses **Note**, and writes in it the way he writes in
Notes — rendered as he types, saved when he says so.

## Design Notes

**The identity comes out of the loader, not down a second prop chain.** The two hosts do not address a
file the same way — a Files panel holds a sync profile id and a profile-relative subpath, a note embed
holds a notes vault id and the bracket text Rust resolves — and AD-65 forbids deriving either from the
other in the webview. So `TextFileSource` states which container its `label` is relative to,
`useTextBuffer` hands the pair out as `FileOrigin`, and `TextFileFrame` passes what the loader read.
A prop each host assembled for itself could disagree with the bytes it describes; this cannot. The
embed is not made to invent a profile: it states the vault, which is what a note can prove.

**Presence, not identity, for the preview callbacks.** The options `livePreview` is built with are read
once at construction, so the pane is keyed on them — but on the vault id as a value and the five
callbacks as present-or-absent. Keying on closure identity would rebuild the pane on every render for a
host that spells one inline, which is the caret-losing defect that dropping the `[text]` key fixed.

**An external change in Note mode is handled exactly as it is in Source.** Both adopt through one
minimal dispatch into the live view; neither annotates it out of the undo history, so an undo after a
reload reaches back to the reader's own text in both. That parity is deliberate: one buffer under two
views that disagreed about what an outside write means would be worse than either behaviour. The
cross-FILE case is what the file key closes, and it is the case that could lose a file.

**A refused adoption reports, and does not swallow.** `setContent` answers a sentence, the host renders
it and falls back to Source. Propagating would take the panel down through an effect with no `try`
around it; swallowing would leave a live view showing the previous bytes with nothing on screen saying
so, which is how a reader concludes their file changed.

## Verification

`bun run test src/components/viewers` — 11 files, 296 tests, all passing (287 before this pass).
`bun run typecheck` clean. No cargo, no biome, no git; the shell crate is not buildable here.

Mutations, each run and each reverted:

| mutation | outcome |
|---|---|
| `raw-rendered-view.tsx` mount deps → `[editable, fileName]` | 2 failures: rows 15 and 16, and nothing else |
| `raw-rendered-view.tsx` drop the `disposed` guard | 2 failures: the mid-mount unmount and the two-switch test |
| `raw-rendered-view.tsx` drop the adoption's `onOutcome` report | 1 failure: row 17 |
| `markdown-preview.ts` return `null` from the adoption's `catch` (the reviewer's `setContent-swallow`) | 1 failure: "turns an adoption the view refuses into a sentence" |
| `text-viewer.tsx` mount deps → `[language, tools]` | 1 failure: the raw editor's own same-name file test |
| `markdown-preview.ts` remove the import wave's `.catch` | 1 failure: "resolves with a sentence rather than rejecting" |

### What CI found that this box did not

GitHub's macOS runner reported the Frontend job **red with all 4375 tests
passing** and `Errors 17 errors`: `EnvironmentTeardownError: Cannot load
'/src/lib/viewers/registry.ts'`, reached through
`markdown-preview.ts → live-preview.ts → file-embed.ts → registry.ts`. A host was
unmounted while this story's six-module `import()` wave was in flight, the wave
rejected, and the rejection travelled out of `mountMarkdownPreview` — whose own
doc comment three lines above the wave says it never rejects, which is why no
caller wraps it. The same shape in production is an offline reader or a deploy
that moved a chunk: a blank pane with nothing said.

It never reproduced on this container across four full runs — the module runner
here resolves the wave before any unmount — so the fix is verified by the new
`markdown-preview-load-failure.test.ts` (its own file: `markdown-preview.test.ts`
imports the real grammar statically on purpose, and a module mocked for one would
be mocked for both), by the mutation above, and by CI going green on the pushed
branch. Recorded here because it is the second time in two epics that a green
local suite and a green hesperia gate were both blind to a class only the third
toolchain saw.
