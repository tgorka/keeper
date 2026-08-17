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
- `setContent` on mode switch, never remount-on-text: `MarkdownPane`'s effect is keyed `[text]`
  (`raw-rendered-view.tsx:348`), which would destroy the view — caret, undo stack and scroll — on every
  keystroke. Note mode adopts `TextEditorMount`'s pattern (`text-editor-host.ts:292-301`).
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
- Never make Note the default: a person opening a file to read it should not land in an editor.

**I/O and edge-case matrix.**

| # | input | expected |
|---|---|---|
| 1 | a session markdown file | three tabs: Preview, Source, Note; Preview is default |
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

</intent-contract>

## Code Map

| file | change |
|---|---|
| `src/components/viewers/view-mode.ts:41-57` | `ViewMode` widens to `"raw" \| "rendered" \| "note"`; `isViewMode` follows; the default is unchanged |
| `src/components/viewers/markdown-preview.ts:100-140` | the two clamping facets become a parameter; the module doc records that the refusal's write-path half still holds and how |
| `src/components/viewers/raw-rendered-view.tsx:133-141,478-521` | a third tab; `MarkdownPane` gains `setContent` instead of remount-on-text and reports edits upward |
| `src/components/viewers/text-file-frame.tsx` | the third mode is offered under the same markdown-and-writable predicate as the writing tools |
| tests | rows 1–13 in the viewer suites; row 14 is the note editor's own suite, unedited |

## Tasks & Acceptance

- [ ] the mode vocabulary widened, cookie-compatible (rows 10–11)
- [ ] an editable live-preview pane over the shared buffer, with `setContent` (rows 2–5)
- [ ] offered only for writable markdown (rows 7–9)
- [ ] Preview still read-only (row 6); no autosave anywhere
- [ ] `docs/notes.md` / `docs/sessions.md`: three modes and what each is for

**Acceptance.** The owner opens a session log, presses **Note**, and writes in it the way he writes in
Notes — rendered as he types, saved when he says so.

## Design Notes

_(filled at review)_

## Verification

_(filled at review)_
