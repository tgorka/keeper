# Spec 50.3 — One editor host, one set of writing tools

story: 50.3
status: reviewed and fixed; two review defects closed (a toolbar over an unmounted editor, and the `workspace/` markdown hole), typecheck clean, the scoped suites green, four mutations killed exactly their tests
branch: `feat/50-3-one-editor-host` (on top of 50.2)
binds: FR-282; FR-233 (the claim this begins to make true), AD-88 (the rendered view stays read-only)
sentinel: `MUT50-3`

<intent-contract>

**The ask, verbatim.** *"nie widze tez otwierania w okienku notes"* — read against what was measured:
a session file can never be a vault note (the overlap validator refuses it, epic 50 §2), so what is
missing is not the vault and not primarily a window. It is the **writing tools**.

**Problem.** FR-233 promises that a session's text files open "in the full notes editor (properties
panel, format toolbar, slash menu, mermaid, tables, embeds)"
(`prds/prd-keeper-2026-07-03/phase-7-sessions.md:190-193`). Measured against a `kind: "note"` target, a
`kind: "file"` target has the rendered markdown tab and mermaid, and **lacks** the format toolbar
(`note-editor.tsx:1076`), the slash menu (`note-editor.tsx:469,565`), and tag/wikilink/emoji
completion (`note-editor.tsx:560-570`) — all mounted only by the note editor.

`text-editor-host.ts:8-13` records why: *"a second editor configuration is how two surfaces end up with
different tab behaviour … what the note editor adds on top (live preview, wikilinks, the slash menu,
the notes store) is markdown-and-a-note specific and deliberately stays there"*. **That rule is right
and its premise is what changed.** A session log is markdown a person writes prose into. So the
extensions **move** into the shared host — they are not copied — and the note editor keeps them by
using the host, not by owning them.

**Approach.** Lift the three vault-free writing tools into the shared text host, behind a flag the
caller sets, so one configuration serves both surfaces.

**Always.**
- **Move, never copy.** After this story there is exactly one definition of the format toolbar's
  actions, the slash menu source and the emoji source. If a copy is left behind, the story has made
  the drift it exists to prevent.
- Only the **vault-free** tools move. Wikilink completion, tag completion, embeds and CSV need vault
  coordinates a sessions zone cannot produce (`text-file-viewer.tsx:104` — the vault id is always
  `null` there), and they stay note-only.
- The tools appear for **markdown** only, decided by the registry's own `format` verdict
  (`registry.ts:205-211`), never by sniffing an extension a second time.
- The note editor's behaviour is **byte-identical** afterwards. Its suite is the guard.
- AD-88 holds: the rendered tab stays read-only (`markdown-preview.ts:117-119`), so the toolbar and
  slash menu belong to the Source tab.

**Block if.**
- The file is not markdown → no toolbar, no slash menu. A `.rs` file in `workspace/` gets the code
  editor it already has, and gains nothing.
- The buffer is read-only (`workspace/` is refused for writes structurally, AD-113) → the tools are
  absent, not present-and-failing.

**Never.**
- Never a second `FormatAction` list, a second slash-menu source, or a second emoji list.
- Never move `livePreview` or the notes store into the host. A file has no note id, no subscription
  and no autosave; a live-preview editor over a manually-saved buffer is a different story with a
  different contract.
- Never make the tools available where a save cannot follow.

**I/O and edge-case matrix.** Every row is a test.

| # | input | expected |
|---|---|---|
| 1 | a session `README.md` on a file target, Source tab | the format toolbar renders, and pressing **Bold** wraps the selection |
| 2 | the same, typing `/` at line start | the slash menu opens with the same items the note editor offers |
| 3 | the same, typing `:sm` | emoji completion offers the same list |
| 4 | a `.rs` file from `workspace/` | no toolbar, no slash menu, no emoji — the code editor unchanged |
| 5 | the rendered tab of a markdown file | still read-only; no toolbar acting on it (AD-88) |
| 6 | a note on a note target | every tool still present and behaving as before — the note suite passes unchanged |
| 7 | the toolbar's actions | one definition, imported by both surfaces; a repo-wide search finds no second copy |
| 8 | a read-only file buffer | tools absent |
| 9 | Save (`Mod-s`) after a toolbar edit | the file writes through the existing file-save path, unchanged |
| 10 | the slash menu's insertions on a file | identical text to the same insertion in a note |

</intent-contract>

## Code Map

| file | change |
|---|---|
| `src/components/notes/format-toolbar.tsx` | stays where it is; its `FormatAction`/`runFormat` pair (`note-editor.tsx:284-286`) moves to a module both surfaces import. Name it in the repo's own voice |
| `src/components/viewers/text-editor-host.ts:8-13,340-395` | the host takes the markdown writing tools behind an option; the module doc is rewritten to say what changed and why the premise moved |
| `src/components/notes/note-editor.tsx:469,560-570,565,1076` | consumes the moved definitions instead of owning them. **No behaviour change** |
| `src/components/viewers/text-file-frame.tsx:225-240` | the toolbar's place in the file frame, Source tab only, beside the existing Save |
| `src/components/viewers/text-file-viewer.tsx` | passes the markdown verdict and the read-only flag through |
| `src/components/viewers/raw-rendered-view.tsx` | the Source/rendered split decides where the toolbar mounts |
| tests | `text-file-viewer.test.tsx` (or the frame's suite) for rows 1–5, 8–10; the note editor's existing suite is row 6, unchanged; row 7 is a grep assertion in the test that owns the moved module |

## Tasks & Acceptance

- [x] the toolbar action pair, the slash-menu source and the emoji source live in one place
- [x] the shared host mounts them for markdown, writable buffers only
- [x] the note editor imports them and its suite passes untouched
- [x] rows 1–10 covered; row 7 asserts there is no second copy
- [x] `docs/sessions.md` / `docs/notes.md`: what a session markdown file can do, and what still needs a vault

**Acceptance.** Opening a session log from a space gives a person the toolbar, the slash menu and emoji
completion they have in Notes; a `workspace/` source file is unchanged; and there is exactly one
definition of each tool in the repository.

## Design Notes

**Deviations from the plan above, each decided on evidence.**

1. **What moved is the WIRING, because the definitions were already one module each.** The plan
   reads as if `runFormat`, the slash source and the emoji source needed extracting from
   `note-editor.tsx`. Measured: `formatCommand`/`FormatAction` were already in
   `editor/format-commands.ts`, `slashMenuSource` in `editor/slash-menu.ts`, `emojiCompleteSource`
   and `emojiShortcodeCommit` in `editor/emoji-complete.ts`. What the note editor OWNED was the
   configuration — the one `autocompletion({ override: […] })` call, the `emojiShortcodeCommit()`
   beside it, and the one-line translation `formatCommand(action)(view)`. That is what moved, into
   `src/components/notes/editor/writing-tools.ts`, and it is why the guard test asserts the shape of
   the import graph rather than counting copies of a list.
2. **The shared module lives in `notes/editor/`, not in `viewers/`.** `text-editor-host.ts` already
   imports `../notes/editor/indent-keymap` and `raw-rendered-view.tsx` already imports
   `notes/editor/csv-table`, so that folder is where a shared editor extension already lives. A
   third home for "extensions two surfaces share" would have been the drift this story exists to
   prevent, wearing a directory name.
3. **The vault sources are the caller's argument, not a nullable vault id.**
   `markdownWritingTools(vaultSources)` composes ONE `autocompletion()` from the caller's sources
   plus the shared ones, in the note editor's existing order (wikilink, tags, slash, emoji). The
   alternative — a `vaultId: string | null` parameter inside the shared module — would have put a
   branch there that one of the two callers could only ever take the null side of.
4. **The toolbar mounts in `TextEditorSurface`, not in `TextFileFrame`'s save bar.** The Code Map
   put it in the frame beside Save. The frame holds no view, and a toolbar acts on one: mounting it
   there means passing an editor handle UP two levels, which is exactly how a press lands in a view
   that the tab switch has already replaced. So the frame decides (it holds both halves of the
   verdict) and the surface that owns the view mounts. AD-88 then holds structurally rather than by
   a second rule about tabs: `RawRenderedView` renders the editor only on Source, so the rendered
   tab has no toolbar because it has no editor.
5. **`TextFileViewer` needed no change.** The Code Map has it "passing the markdown verdict and the
   read-only flag through". Both facts are already below it: `entry.format` and `savable` are the
   frame's own, and `savable` is the flag the Save button already stands on. Adding a prop for a
   verdict the frame can derive would have been a second source of truth for one question.
6. **`writingTools` is a REQUIRED option on `mountTextEditor` and an optional prop above it.** The
   host has exactly one caller, so requiring it costs nothing and makes a future third surface answer
   the question. `TextEditorSurfaceProps.writingTools` defaults to `false` because that component is
   also mounted over buffers nobody classified (a paste, a note embed).
7. **`TextEditorMount.runFormat` is `null` rather than a no-op when the tools were not asked for.**
   A no-op function is a toolbar press that goes nowhere, silently — the shape this epic keeps
   finding. Null says "this buffer has no writing tools".
   **Corrected by review.** The story went on to claim that the null therefore made it impossible to
   render "a toolbar over a view that cannot run it", because the surface draws the toolbar from the
   same `tools` const that produced the mount option. That second half was false. `tools` is known at
   the first render; the mount is assigned only after `await mountTextEditor(...)` resolves behind six
   dynamic imports, so the toolbar painted immediately and stayed clickable over an empty host for the
   whole of that window — and again, guaranteed, on every `[language, tools]` rebuild, whose cleanup
   nulls the mount while `tools` is still true. Every press in either window reached
   `mountRef.current?.runFormat?.(action)` and was silently discarded: exactly the defect the null was
   chosen to prevent, arriving through the door the null cannot watch. The fix is a `mounted` state
   set beside `mountRef.current = mount` and cleared in the cleanup, with the toolbar rendered from
   `tools && mounted` (`text-viewer.tsx:119,194,204,245`). The null is still right and still
   necessary — it answers for a mount built without the extensions, which the gate cannot see — it was
   simply never sufficient on its own.
8. **The read-only rule is `savable`, and `savable` now asks the LOCATION as well as the format.**
   "Never make the tools available where a save cannot follow" is implemented as *the tools appear
   exactly where Save appears*.
   **Corrected by review.** As shipped, `savable` knew only the FORMAT's verdict (`entry.writable`)
   and the size guard, and this spec argued that a `workspace/` markdown file was therefore left as it
   was — with the justification, written into `text-file-frame.tsx`, that "what keeps the tools off it
   here is that it is not markdown". `workspace/` markdown is the documented normal case
   (`docs/sessions.md:411`, and this repo's canonical fixture is `workspace/iter-3.md`), so the
   sentence was false and the Block-if line above it — "the buffer is read-only (`workspace/` is
   refused for writes structurally, AD-113) → the tools are absent" — was unimplemented: such a file
   got the toolbar, the slash menu, emoji completion and a Save button over a buffer every write
   refuses, and was not marked read-only either. The only test that looked like it covered this used
   `main.rs`, so it proved that a `.rs` is not markdown and stayed green over the hole.
   The fix threads the verdict that already existed rather than inventing one. `sync_browse` builds
   its `WriteScope` with the profile's sessions zone named (`sync_ipc.rs:2117-2127`), so every listing
   row already carries the fence's own refusal — `FilesWriteVm.writable`/`reason`, composed by
   `keeper_sync::files_write::WriteRefusal::SessionWorkspace`. `panel-strip.tsx:248-254` now passes it
   on as `ViewerFile.writeRefusal`, beside the `writeCaveat` it already passed, and
   `text-file-frame.tsx:286-288` folds it into `refusal`. Everything else falls out of that one input:
   no Save bar, no toolbar, no slash menu, no properties panel, `readOnly` on the editor, and Rust's
   sentence in the read-only notice. This is not the frontend deciding which volumes are writable —
   the decision arrived from Rust on the row the panel opened — and the deleted objection, that the
   frame "has no location-writability flag", described a prop that was missing rather than a rule.
   A `workspace/` **source** file — the spec's own row 4 — is now covered twice over: not markdown,
   and not writable.
9. **Two now-stale sentences were corrected where they were written down.**
   `text-editor-host.ts`'s module doc said the slash menu "deliberately stays" in the note editor;
   it now records what moved and why the premise, not the rule, changed. `emoji-complete.ts` said its
   menu was a source "in the note editor's existing `autocompletion()` call"; it now names the shared
   module. No behaviour in either file changed.

## Verification

**Ran here, on Linux, in this worktree.** No `cargo`, no formatter, no git writes — this story
touches no Rust. The scoped `vitest` runs were authorised by the coordinator. The rows below marked
**(fix)** were re-run after the review; the rest are the original story's and are unchanged.

| command | result |
|---|---|
| `node node_modules/typescript/bin/tsc --noEmit` **(fix)** | clean. **The original entry here was wrong and is corrected**: it claimed the tree carried one error, `session-templates.test.tsx:52`, attributed to story 50.2. Re-run on the stack tip, that line typechecks — all three symbols it imports exist and are used — and 50.2's own gate list, which reported `typecheck` green, was right. The only errors in the tree at the time of this re-run were the two in-flight review fixes for 50.1 and 50.2 (a `noHome` field and an `isDir` field whose fixtures had not yet caught up), neither of them 50.3's and neither of them this line. A stack that merges carrying a known typecheck error attributed to the wrong story is how the next CI failure gets blamed on the wrong commit, which is why this row is corrected rather than dropped |
| `bun run test src/components/viewers src/components/notes/editor/writing-tools.test.ts` **(fix)** | 12 files, 270 tests, 0 failed — the story's own scope, plus the four tests the review fixes added |
| `bun run test src/lib/viewers src/components/layout/panel-strip.test.tsx` **(fix)** | 6 files, 103 tests, 0 failed — the registry and the panel host, which the `writeRefusal` field crosses |
| `bun run test src/components/notes/editor/writing-tools.test.ts src/components/viewers/text-file-viewer.test.tsx src/components/viewers/text-viewer.test.tsx` | 3 files, 66 tests, 0 failed |
| `bun run test src/components/notes/editor/writing-tools.test.ts src/components/viewers` | 12 files, 260 tests, 0 failed — every viewer suite, including the frame's and the toggle's |
| `bun run test src/components/notes src/components/capture src/components/export/export-in-the-note-editor.test.tsx src/components/layout/panel-strip.test.tsx src/capture-main.test.tsx` | 49 files, 900 tests, 0 failed — **row 6.** Not one test file under `src/components/notes/` was edited by this story (`git status` shows only `note-editor.tsx` and `emoji-complete.ts` modified there) |

**Mutations run, each restored and the restore verified by reading the diff** (sentinel `MUT50-3`):

| mutation | tests killed |
|---|---|
| `text-file-frame.tsx`: `writingTools` forced `false` | 5 — every positive row (toolbar+Bold, Save after a toolbar edit, the slash menu, the slash insertion, emoji). The four absence rows stayed green, which is what makes them meaningful rather than vacuous |
| `text-editor-host.ts`: `writing.markdownWritingTools()` → `[]` | exactly 3 — the slash menu, the slash insertion and emoji; the two toolbar tests still passed. The extension mount and the toolbar are pinned independently |
| **(fix)** `text-viewer.tsx`: the toolbar gate back to `{tools ? …}` | exactly 2, both new — *draws no toolbar over the host until the editor is inside it* and *takes the toolbar away for the whole of a rebuild, not just its start*. The other 20 tests in the file stayed green, which is the measurement that says the old suite could not have caught this |
| **(fix)** `text-file-frame.tsx`: `refusal` back to `entry.writable ? null : …` | exactly 1, new — *gives a workspace markdown file no toolbar, no menu, no Save, and says why*. Nothing else in `src/components/viewers` moved, including the `.rs` test that was standing in for it |
| **(fix)** `panel-strip.tsx`: `writeRefusal: null` | exactly 1, new — *hands the viewer the row's write verdict, so a refused file opens read-only*. This is the wire no viewer suite can see, because every viewer test builds its own `ViewerFile` |

**Rows of the matrix, and the test that owns each.**

| row | test |
|---|---|
| 1 | `text-file-viewer.test.tsx` — *has the format toolbar, and Bold wraps the selection*; and **(fix)** `text-viewer.test.tsx` — *draws no toolbar over the host until the editor is inside it* and *takes the toolbar away for the whole of a rebuild, not just its start*, which pin WHEN it renders and not only whether |
| 2 | *opens the slash menu, offering the commands a note offers* (compared against `SLASH_COMMANDS` itself) |
| 3 | *completes an emoji shortcode, and commits one typed in full* (compared against `matchEmoji("sm")`) |
| 4 | *gives a workspace source file no toolbar, no menu and no shortcodes* — `startCompletion()` is `false`, and `:tada:` stays six characters |
| 5 | *keeps the rendered tab read-only, with no toolbar over it (AD-88)* |
| 6 | the note editor's own 49 suites above, unedited |
| 7 | `writing-tools.test.ts` — the repo scan: one declaration each, one `autocompletion(` call site, one toolbar component, two lazy importers and no third |
| 8 | *offers no tools over a markdown format keeper will not write*, *…over a file only the first part of which was read*, and `text-viewer.test.tsx` — *withholds them from a buffer nobody can write*. **(fix)** The row's real case, `workspace/` markdown, is now covered rather than stood in for by a `.rs`: *gives a workspace markdown file no toolbar, no menu, no Save, and says why*, with *withholds them for the refusal and not for the word workspace in the path* beside it so the claim is about the fence rather than a path segment, and `panel-strip.test.tsx` — *hands the viewer the row's write verdict, so a refused file opens read-only* — carrying the verdict the whole way from the listing |
| 9 | *saves what the toolbar wrote, through the file's own write path* — asserts `syncWriteEntry` receives `# Session\n\n**alpha**\n` |
| 10 | *inserts exactly the text the same command inserts in a note* — asserted against the shared table's own `text()`, with no surviving `/` |

**Where the review fixes landed**, for a reader diffing this against the original commit:

| file | change |
|---|---|
| `src/components/viewers/text-viewer.tsx:119,194,204,245` | `mounted` state, set beside `mountRef.current = mount` and cleared in the cleanup; the toolbar renders on `tools && mounted`. The comment that claimed neither window was reachable is replaced by one that names both and says which gate closes each |
| `src/lib/viewers/types.ts:288-317` | `ViewerFile.writeRefusal`, required and nullable, beside `writeCaveat` |
| `src/components/layout/panel-strip.tsx:248-254` | fills it from the listing row Rust already sends |
| `src/components/viewers/text-file-viewer.tsx:188` | passes it to the frame |
| `src/components/viewers/text-file-frame.tsx:196-214,286-288` | takes it as a prop and folds it into `refusal`, so Save, the toolbar, the menu, the properties panel and `readOnly` all follow from one input |
| `src/components/viewers/text-editor-host.ts:24-34,304-314` | the module doc's `workspace/` sentence now describes a guard that exists, and `runFormat`'s doc says what the null does and does not answer for |

**Owed to nobody.** There is no Rust in this story and no shell-crate code, so nothing here is
deferred to hesperia.
