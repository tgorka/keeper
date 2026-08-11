---
title: 'Story 43.1: Tab Belongs to the Editor'
type: 'bug'
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
---

<intent-contract>

## Intent

**Problem, as reported:** pressing Tab while editing a note "inserts whitespace and maybe two blank
lines go in".

**Problem, as measured.** The report is a symptom seen through a WKWebView; the cause is one line
long and it is not in this repo's code at all. `note-editor.tsx` assembles `defaultKeymap`,
`historyKeymap` and `completionKeymap`, and **none of the three binds `Tab`** — CodeMirror leaves
the key alone on purpose, because a text box that swallows Tab is a keyboard trap. A mounted editor
was driven in jsdom before anything was changed, and the measurement is unambiguous:

| pressed | document after | `defaultPrevented` |
|---|---|---|
| `Tab` | unchanged | **`false`** |
| `Escape` then `Tab` | unchanged | `false` |

`false` is the whole bug. CodeMirror never claims the keystroke, so it goes on to the web view, and
what a web view does with Tab inside a `contenteditable` is its own business — WebKit edits the DOM
under CodeMirror's feet, and the editor then reads that foreign DOM back into its document. That
read-back is why the owner's symptom is *vague*: "maybe two blank lines" is what a DOM-level
reconciliation looks like from the outside, and it is not something an inserted character would ever
produce. The fix is therefore not "insert an indent"; it is **claim the key**, so the platform never
gets a turn.

**Approach:** one small module holding CodeMirror's own `indentMore` / `indentLess`, spread into the
keymap the editor already has. No new extension slot, no character inserted by hand, and the
accessibility escape hatch left to the library that already implements it.

## Boundaries & Constraints

**Always:**
- Tab is claimed through the **`keymap` facet**. That is what puts the binding behind
  `@codemirror/view`'s `tabFocusMode` check, which is the accessibility escape hatch — `Escape`
  arms a two-second window in which the next Tab is dropped before any handler sees it and the
  browser moves focus.
- Indentation is spaces, never a literal `\t`. Obsidian opens the same file and a tab renders at a
  width the next reader's editor picks.
- Every behavioural claim is asserted against **document text**, and — for the reported symptom —
  against whether the keystroke reached the platform at all.

**Block If:**
- Nothing. This story is contained in the note editor's keymap.

**Never:**
- No raw `keydown` listener on the content DOM. It fires *after* the view has already declined the
  event during the escape window, so it would silently rebuild the keyboard trap.
- No hand-written character insertion, and no second indentation vocabulary beside CodeMirror's.
- No change to `indentUnit` (see Design Notes for why the default is the right default here).

## I/O & Edge-Case Matrix

| Scenario | Input / State | Expected | Error |
|---|---|---|---|
| Plain line | caret in `first paragraph`, Tab | `  first paragraph`; line count unchanged; keystroke claimed | none |
| Repetition | Tab ×3 on one line | six spaces on one line, still one line | none |
| Caret | caret mid-word, Tab | caret rides the insertion, still mid-word | none |
| Outdent | caret in `    alpha`, Shift-Tab | `  alpha` | none |
| Outdent at the margin | caret in an unindented line, Shift-Tab | text byte-identical | none |
| Round trip | Tab then Shift-Tab | document byte-identical to before | none |
| Multi-line selection | partial selection spanning lines 1–3 of 4 | lines 1–3 gain one unit each, line 4 untouched, no line added | none |
| Multi-line outdent | selection over two indented lines | one unit removed from each, not all indentation | none |
| Bullet list | caret in `- beta`, Tab | `  - beta` — nested under `- alpha` | none |
| Task list | caret in `- [ ] beta`, Tab | `  - [ ] beta`, checkbox intact | none |
| Several list items | selection over two items, Tab | both nest | none |
| Ordered list | caret in `2. beta`, Tab | `  2. beta`; a second Tab reaches the nesting column. Named limitation, asserted | none |
| Escape then Tab | `Escape`, then Tab | keystroke **not** claimed, document unchanged — focus leaves | none |
| Escape window spent | `Escape`, Tab, an ordinary key, Tab | claimed again, line indents | none |
| Completion open | `#w` with the tag popup active, Tab | completion accepted (`#work`); no whitespace in front of the `#` | none |
| No completion | popup closed, Tab | ordinary indent | none |
| Through the mounted editor | real `NoteEditor`, Tab at the real content DOM | store buffer reads `  alpha\nbeta\n`; keystroke claimed | none |
| Through the mounted editor | real `NoteEditor`, `Escape` then Tab | keystroke not claimed, buffer unchanged | none |

</intent-contract>

## Code Map

- `src/components/notes/editor/indent-keymap.ts` — **new, and the whole story.** Exports
  `indentBindings`: `indentWithTab` with `acceptCompletion` in front of it. Its doc comment carries
  the two things a future reader would otherwise re-derive the hard way: why the escape hatch must
  not be re-implemented, and why the indent unit is left at CodeMirror's default.
- `src/components/notes/note-editor.tsx` — two lines: one more entry in the boot effect's
  `Promise.all` of dynamic imports, and `...indent.indentBindings` spread into the keymap that was
  already there. The module is loaded through the same `import()` as `live-preview` and friends, so
  the quick-capture window still pays nothing for the editor bundle (NFR-27).
- `src/components/notes/editor/indent-keymap.test.ts` — sixteen document-text assertions over a real
  `EditorView` carrying the editor's real extension stack.
- `src/components/notes/editor/tab-wiring.test.tsx` — **new, and the reason this story is not just a
  module test.** Mounts the actual `NoteEditor`, with its own boot effect and its own extension
  list, and presses Tab at the real content DOM. The document is read back through the app's own
  channel: CodeMirror's update listener → `onEdit` → the notes store.

## Tasks & Acceptance

**Execution:**
- [x] Measure the current behaviour before changing anything, and record the measurement.
- [x] Bind Tab / Shift-Tab through the `keymap` facet using CodeMirror's own commands.
- [x] Keep the accessibility escape hatch, asserted rather than assumed.
- [x] Tests: every matrix row.
- [x] Prove each test fails when the change is reverted.

**Acceptance Criteria:**
- Tab indents, Shift-Tab outdents, Tab over a multi-line selection indents each line, Tab in a list
  nests the item, and `Escape` then Tab still leaves the editor — each asserted against document
  text, none against a keymap table.
- The reported symptom cannot recur: the Tab keydown is claimed by the editor, and the document
  gains no line however many times it is pressed.

**Revert proof.** Each mutation was applied to the shipped source, the two suites were run, and the
source restored. 19 tests pass unmutated.

| Mutation | Failed | Which |
|---|---|---|
| `indentBindings = []` (the pre-story world) | 16 / 19 | every behavioural test in both suites |
| `shift: indentLess` removed | 4 | the three outdent tests and the round trip |
| `acceptCompletion(view) \|\|` removed | 1 | "accepts the completion instead of pushing whitespace in front of it" |
| `...indent.indentBindings` removed from `note-editor.tsx` only | 2 | both `tab-wiring` behaviour tests — the module tests stay green, which is exactly why that suite exists |
| Tab bound to a hand-written `insert: "\t"` | 14 | every indent, list and no-tab assertion |
| Tab bound with `EditorView.domEventHandlers` instead | 1 | only the Shift-Tab test — see Design Notes; this is *not* the trap it looks like |
| Tab bound with a raw `contentDOM.addEventListener` | 1 | "keeps the escape hatch" — and nothing else, in either suite |

## Design Notes

**The reported symptom was replaced by a measurement, and the two do not match.** "Whitespace, maybe
two blank lines" describes an *insertion*; the measured cause is an *omission*. Nothing in keeper was
inserting anything — the editor declined the key and WebKit acted on a `contenteditable` that
CodeMirror then had to reconcile. That distinction decided the fix: had the diagnosis stopped at the
symptom, the obvious repair would have been "make Tab insert two spaces instead of whatever it is
inserting", which is the mutation the revert proof shows failing fourteen tests.

**`EditorView.domEventHandlers` is safe; `addEventListener` is not.** The first draft of this module
warned against both. That was wrong and the mutation table is how it was caught: handlers registered
through `domEventHandlers` are dispatched by the view itself, behind the same `tabFocusMode` check as
the keymap, so the escape hatch survives them. A bare `contentDOM.addEventListener` runs *after* the
view has declined the event, and it is the one shape that breaks the hatch — mutation 7 fails exactly
one test, the right one. The doc comment now says the true thing.

**Two spaces, deliberately, and CodeMirror's default is therefore left untouched.** Two is the
CommonMark content column of `- `, `* ` and `- [ ] ` — the markers keeper's own slash menu writes —
so one Tab nests a bullet or a task exactly. It is also *below* the four-space threshold at which
CommonMark turns an indented line into a code block, so Tab on a plain paragraph indents prose and
never silently changes what the paragraph is. The cost is that an ordered item (`1. `, content column
three) needs two presses to reach its nesting column. That trade was taken on purpose: a worse indent
is recoverable, a paragraph that became a code block in Obsidian is not. The limitation is asserted
in a test rather than left to be discovered.

**`acceptCompletion` in front of `indentMore` is not decoration.** `indentMore` inserts at the *line
start*. A Tab pressed while the tag popup or slash menu is open would push whitespace in front of the
very `#` or `/` the popup is matching on — closing the popup and leaving an indent nobody asked for,
which is this story's symptom wearing a different hat. `acceptCompletion` returns `false` when no
completion is active, so the ordinary path is untouched. This is one behaviour beyond the epic's five
named ones, and it is named here rather than smuggled.

**Two suites, because the risk lives in two places.** `indent-keymap.test.ts` asserts what the
commands do. `tab-wiring.test.tsx` asserts that they are *in the editor* — mounting the real
component, mocking only the IPC surface, and reading the document back out of the notes store. The
mutation that deletes the wiring from `note-editor.tsx` leaves every module test green and fails only
the wiring suite, which is the ledger's recurring lesson answered directly: the risk here was in the
impure shell, and it is asserted in the impure shell.

## Deliberately not done

- **No `indentUnit` configuration, and no per-list-marker indent width.** `@codemirror/lang-markdown`
  ships no list-indent command, and computing the parent's content column would mean hand-writing the
  character insertion this story exists to avoid. The ordered-list case is documented instead.
- **No Obsidian-style "indent with tabs" preference.** There is no editor settings surface to hang it
  on, and a preference that exists only in code is a fork of the vocabulary.
- **The slash menu's own defect was found and left alone.** `slashMenuSource` returns
  `from: line.from`, so the popup's match pattern includes the leading `/`; no command label contains
  a slash, the fuzzy matcher rejects every option and the menu never opens. Confirmed by driving the
  real source through a real `EditorView`: the completion state goes `pending` and then straight back
  to inactive. It is a different story's module and a different story's bug; the completion assertion
  here uses `tagCompleteSource`, which does open. **Reported, not fixed.**
- **Nothing about the blur → save → splice path**, which was the other candidate mechanism for
  "two blank lines" and was ruled out by the measurement above.

## What could not be proved on Linux

The originating symptom itself. jsdom implements no `contenteditable` editing behaviour, so an
unclaimed Tab does nothing there — which is why the pre-change measurement shows an unchanged
document and `defaultPrevented: false` rather than the owner's blank lines. What is proved here is
the causal link: before, the key reached the platform; after, it does not, and the document gets
exactly one indent. Reproducing the *WebKit* half of the symptom needs the macOS host and a real
WKWebView, and it is a fact about WebKit rather than about this diff.
