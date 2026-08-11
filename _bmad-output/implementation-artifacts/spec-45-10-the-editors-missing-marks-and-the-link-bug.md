# Spec 45.10 — The Editor's Missing Marks, and the Link Bug

status: implemented
story: Epic 45, Story 45.10
bindings: FR-184, UX-DR73, DW-165
depends on: 44.9 (the format toolbar and its command shape), 43.9 (the `/` menu),
37.6 (`live-preview.ts`), 37.8 (`mermaid-widget.ts`), 44.15 (`galleryLayer`, the
`StateField` precedent), 45.4 (`markdown-preview.ts` and its DW-165 tripwire)
author: W2Marks

## What shipped

Five marks the toolbar and the `/` menu did not have — subscript, superscript,
underline, a task list whose boxes you can click, and a fenced code block — plus
the half of this story that was a defect rather than a gap: **markdown links did
not render**, and a note containing a `mermaid` fence could not be opened at all.

| file | what changed |
| --- | --- |
| `src/components/notes/editor/format-commands.ts` | five new `FormatAction`s; `toggleUnderline`, `toggleCodeBlock`, `topBlock`, `underlinePair`; `PREFIX` widened to own the checkbox |
| `src/components/notes/format-toolbar.tsx` | five buttons, grouped with their relatives |
| `src/components/notes/editor/slash-menu.ts` | three new rows; `SlashCommand.caret` so a row that inserts a *pair* leaves the caret between the delimiters |
| `src/components/notes/editor/live-preview.ts` | link rendering rebuilt around node PARENT rather than node name; `Subscript`/`Superscript`/`CodeInfo` marks; `<u>` underline; `TaskWidget`; the mermaid block decoration removed from the `ViewPlugin` |
| `src/components/notes/editor/mermaid-widget.ts` | `mermaidLayer()`, a `StateField` — the DW-165 fix |
| `src/components/viewers/markdown-preview.ts` | the DW-165 guard removed, and the dead `failureLine` with it |
| `src/components/viewers/raw-rendered-view.tsx` | `failureLine` plumbing removed (three lines) |

## Check whether it already exists — what I found

Asked before building, as the wave requires. Four findings, two of them
load-bearing:

1. **The `/` menu already had a `Task` row and a `Code fence` row**, since story
   37.6. What it did not have was a *toolbar* button for either, a `formatCommand`
   for either, or any rendering of a task list at all. Both rows also parked the
   caret in the wrong place, which is fixed here.
2. **The parser already emits `Subscript` and `Superscript` nodes.**
   `markdownLanguage` — the base `note-editor.tsx` has loaded since 37.6 — is
   CommonMark + GFM *plus* the Subscript and Superscript extensions. `H~2~O` has
   been parsing into `Subscript` + two `SubscriptMark`s in this editor for eight
   epics; nobody had ever rendered or written one. No grammar was added here and
   no dependency.
3. **The parser already emits `Task` and `TaskMarker`** for `- [ ] ` in a bullet
   list, an ordered list, a blockquote and an indented list. Confirmed against a
   real `EditorView` before writing the widget.
4. **`NoteEditor.onFollowLink` is dead** — see "A dead field I found and did NOT
   activate" below.

## The link defect, diagnosed before it was fixed

Reproduced in a real `EditorView` with `@codemirror/lang-markdown` loaded, then
reported to Main before a line was changed.

**One line was the cause:** `URL: true` in `HIDDEN_MARKS`. The renderer hid
nodes by NAME, and the parser reuses `URL`, `LinkMark` and `LinkLabel` across
constructs where they mean opposite things.

| what the writer typed | what the reader saw, before | why |
| --- | --- | --- |
| `<https://example.com>` | **nothing at all** | `<` and `>` are `LinkMark` (hidden); the `URL` between them IS the text, and was hidden too |
| `https://bare.example` | **nothing at all** | a bare GFM autolink is a lone `URL` node whose text is the link |
| `[the docs](https://example.com)` | `the docs` | correct — the one role where `URL` is a destination |
| `[the docs][1]` | `the docs[1]` | `LinkLabel` was not hidden, so the reference leaked |
| `[1]: https://x.example` | `[1]` | a definition line is metadata, and both its `URL` and its `:` were hidden |
| `![alt](https://host/x.png)` | `alt` | the destination was hidden, so a remote embed looked like a word somebody typed |

The most severe of these is the first two: **the app deleted the user's content
from the screen while the file on disk was intact** — the exact failure UX-DR44
names as the most alarming lie a notes app can tell.

**The fix is to ask the parent, not the name.** `LinkMark` and `LinkLabel` are
punctuation only under `Link`, `Image` or `Autolink`. `URL` is a destination only
under `Link` or `Image`; under `Autolink` it is the text, under `LinkReference` it
is a definition, and under a paragraph it is a bare autolink. Each of those is now
a separate decision with a test.

And because the destination is hidden where it should be, every link now carries
it in a `title`, so hovering answers "where does this go?" without moving the
caret into the line to read the source.

## DW-165 — a mermaid fence crashed the editor

`live-preview.ts:221` supplied `Decoration.replace({ …, block: true })` from a
`ViewPlugin`. CodeMirror refuses a block decoration from a plugin and **throws
out of `EditorView` construction**, so any note containing a ` ```mermaid ` fence
could not be opened. Shipped since story 37.8; invisible for eight epics because
the one suite that built a real view around `livePreview` built it *without* the
markdown language, so `syntaxTree` produced no `FencedCode` and the branch was
never entered.

The fix is `galleryLayer`'s, applied to the block that filed the ledger entry:
`mermaidLayer()`, a `StateField` in `mermaid-widget.ts`, composed into
`livePreview()`'s array beside the plugin so a note still has exactly one
renderer. Scan and reveal are separated the way `galleryLayer` separates them —
moving the caret re-paints from the fences already found, only an edit re-scans.

`markdown-preview.ts`'s guard is gone in the same change, and 45.4's tripwire is
inverted: it now asserts the fence RENDERS, from the same assembly that found the
crash. `mermaidFenceLine` is kept and still exported, because it is how that file
proves it and the renderer read the same grammar.

## The spellings, and what Obsidian does with them

Obsidian reads this vault. The rule the epic set is that a note must not get
*worse* to read outside keeper, and each spelling below was chosen against it.

| mark | keeper writes | Obsidian renders | verdict |
| --- | --- | --- | --- |
| subscript | `H~2~O` | literally `H~2~O` | **accepted.** Un-styled, but legible, unambiguous and losslessly round-tripped. It is also the spelling the parser keeper already loads produces, so nothing was invented. |
| superscript | `x^2^` | literally `x^2^` | **accepted**, same reasoning. |
| underline | `<u>word</u>` | a real underline | **accepted.** The only spelling that means underline and nothing else, in Obsidian, on GitHub and in Pandoc. |
| task list | `- [ ]` / `- [x]` | a real checkbox | **accepted.** GFM. No divergence at all. |
| code fence | ` ``` ` on its own lines | a real code block | **accepted.** No divergence. |

### Rejected, and why

- **`__word__` for underline.** It is *bold* in CommonMark and in Obsidian, and
  this editor's own parser agrees — it reports `StrongEmphasis` for it, so keeper
  could not tell an underline from a bold even if it wanted to. The same file
  would say two different things depending on who opened it. There is a test
  asserting the toolbar never writes `__`.
- **`==word==` for underline.** *Highlight* in Obsidian. A different mark with a
  different meaning.
- **`_word_` for underline.** Italic.
- **`<sub>`/`<sup>` for sub- and superscript.** These render correctly in
  Obsidian, and that is the whole temptation. Rejected because they render as raw
  angle brackets in keeper — this editor deliberately renders no HTML — so they
  fail the same test from the other side, and they would put a *second* HTML
  spelling in the vault beside the one underline already needs.

### On underline being HTML, given the module's standing refusal

`live-preview.ts` has refused since 37.6 to render HTML in a note body, and the
reason it gives is "there is no HTML sink to inject into". That reason is intact.
The renderer recognises **two literal strings** as delimiters and paints a CSS
class over the text between them, exactly as it does for `**`. Nothing is parsed
as HTML and nothing reaches `innerHTML`. There is a test asserting that `<b>` and
`<script>` in a note body are still the characters the writer typed.

## I/O matrix — the renderer

Caret parked off the line in every row; the reveal rule gives every one of them
its source back when the caret arrives, and that is tested separately.

| source | rendered |
| --- | --- |
| `<https://example.com>` | `https://example.com`, `.cm-lp-link`, `title` = the URL |
| `see https://bare.example now` | unchanged text, the URL marked `.cm-lp-link` with its `title` |
| `see [the docs](https://x) here` | `see the docs here`; `the docs` is `.cm-lp-link`, `title` = `https://x` |
| `see [the docs][1] here` | `see the docs here` |
| `[1]: https://x.example` | unchanged — a definition line is source |
| `![alt](pics/one.png)` | the image widget |
| `![alt](https://host/x.png)` | unchanged, whole — keeper never fetches it, and now says so by showing what it declined |
| `H~2~O` | `H2O`, the `2` in `.cm-lp-sub` |
| `x^2^` | `x2`, the `2` in `.cm-lp-sup` |
| `~~gone~~ and ~low~` | `gone` struck, `low` subscript — the parser tells them apart |
| `an <u>run</u> word` | tags hidden, `run` in `.cm-lp-underline` |
| `a <b>bold</b>` | unchanged — every tag but `<u>` is inert text |
| `an <u>unclosed run` | unchanged — an unpaired tag is left alone |
| `<u></u>` | both tags hidden, no mark (CodeMirror rejects an empty mark decoration) |
| `- [ ] todo` | an unchecked `input.cm-lp-task` |
| `- [x] done` / `1. [X] shouted` | a checked one |
| `` ```ts …``` `` | `.cm-lp-fence` lines; `ts` visible in `.cm-lp-fence-info`; no `.cm-lp-code` |
| `` `snippet` `` | `.cm-lp-code`; no `.cm-lp-fence` |
| ` ```mermaid ` fence | the diagram widget, and the editor **constructs** |

## I/O matrix — the commands

Every action toggles; the second press returns the document the first was handed.

| action | empty selection | `word` selected | pressed again |
| --- | --- | --- | --- |
| subscript | `~~`, caret between | `~word~` | `word` |
| superscript | `^^`, caret between | `^word^` | `word` |
| underline | `<u></u>`, caret between | `<u>word</u>` | `word` |
| task | `- [ ] ` on the line | `- [ ] ` on every touched line | marker removed |
| code block | ` ```\n\n``` `, caret on the middle line | the line fenced, body still selected | body kept, fences dropped |

### Edge cases, and what each one does

| case | behaviour | why |
| --- | --- | --- |
| underline pressed with `<u>word</u>` fully selected | unwraps | without it, selecting what the first press produced and pressing again wraps a second time |
| underline inside `` `code` `` or a fence | not paired | those `<u>`s are not `HTMLTag` nodes, so the parser never offers them |
| `<u>` unclosed three paragraphs up | not paired with a later `</u>` | the search is bounded to one top-level block |
| nested `<u>big <u>word</u> here</u>` | the innermost pair goes | matches how bold-inside-italic already behaves |
| task pressed on `- [ ] a` | `a` | the checkbox is part of the MARKER, so it toggles off whole |
| bullet pressed on `- [ ] a` | `- a` | bullet owns the same prefix group and writes its own marker over the task's |
| heading pressed on `- [x] a` | `# a` | takes the whole marker, never leaving an orphan `[x]` |
| task pressed on `> - [ ] a` | `> a` | the quote group is separate and is not touched |
| task pressed on `1. [ ] a` | `- [ ] a` | one task spelling, whichever list it came from |
| code block on an unterminated fence | unwrapped, one fence line dropped | it is a fence the user is still typing; refusing meant they could not undo it |
| code block on an empty fence | unwrapped without throwing | it has no body, and the two "delete the line break too" ranges would overlap |
| code block over a multi-line selection | every touched line, whole | a fence owns lines |
| task marker clicked | the source flips `[ ]` ↔ `[x]` | position is asked of the view at click time, never captured |
| task marker clicked after text above it grew | still the clicked one | same reason |
| task marker clicked | the caret does NOT move to the line | `mousedown` is cancelled; otherwise the reveal rule would show the source and the checkbox would vanish under the click |

## A dead field I found and did NOT activate

`NoteEditor` accepts `onFollowLink`, wires it into `livePreview`'s `onOpenLink`,
and **nobody passes it**. Neither `notes-pane.tsx` nor `panel-strip.tsx` supplies
it, so clicking a wikilink in a note has done nothing since story 37.6 — while
`.cm-lp-wikilink` has been styled `cursor: pointer` the whole time.

I did not wire it. Following an external `https://` link needs a capability
decision (webview → system browser, through the bundled opener the login screen
already uses), and the wave's own lesson is that the first story to set a
long-dead field is the one that exposes what the field was hiding. Reported to
Main; it wants its own item.

What I did do instead is make the affordance honest as far as rendering can:
every link carries its destination in a `title`, so the reader can at least read
where it goes.

## Deliberately NOT done

- **Clicking a link opens it.** See above. Rendering was the reported defect;
  navigation is a capability decision and a dead field's problem.
- **Syntax highlighting inside a code fence.** Still absent, still on purpose —
  colour that implies highlighting would be a lie. The language name is visible
  now, which is the honest half.
- **Rendering any HTML tag other than `<u>`/`</u>`.** Two literal delimiter
  strings, no parser, no sink. `<b>` and `<script>` stay text, with a test.
- **Fetching a remote image.** Still refused; the change is that a note now
  *shows* the source it declined instead of showing only the alt text.
- **A keybinding for any of the five.** 44.9 shipped none for its eight, and
  adding a second convention for the new ones would be the divergence this epic
  keeps warning about.
- **Closing an unterminated fence for you.** Toggling it off is what the button
  means; auto-closing is an editing behaviour, not a formatting one.
- **`~~~` (tilde) fences.** The parser reads them and `mermaidLayer` finds them
  — a tilde mermaid fence renders. The *command* writes backticks, because a
  vault whose fences are spelled two ways diffs badly.

## Mutation proof

Harness in `~/.W2Marks/`. Baseline green before and after at the verdict's
scope — 173 tests across `live-preview-marks.test.ts`, `format-commands.test.ts`,
`slash-menu.test.ts`, `live-preview.test.ts`, `format-toolbar.test.tsx`,
`markdown-preview.test.ts` and `raw-rendered-view.test.tsx`. Every anchor
re-grepped **by name** after restore, never by checksum.

**31 mutations, 31 caught** (one after the test and the code were both fixed).

| # | mutation | caught by |
| --- | --- | --- |
| 1 | hide every `URL`, not only a destination | shows an angle-bracket autolink…; shows a bare URL…; leaves a link reference definition alone; carries the destination in a title |
| 2 | stop hiding a reference link's label | shows a reference link's text without leaking its label |
| 3 | drop the destination title | carries the destination in a title, on both link shapes |
| 4 | drop the title on a bare autolink | carries the destination in a title, on both link shapes |
| 5 | treat a remote image URL as a local asset | leaves a remote image as its whole source |
| 6 | ignore the tick in the source | renders each marker as a checkbox reflecting the source; recognises an uppercase tick |
| 7 | write the task marker back unchanged | all five task-toggle tests |
| 8 | capture position zero instead of asking the view | all five task-toggle tests |
| 9 | let the click move the caret | does not let a click on the box move the caret onto its line |
| 10 | pair underline on `<b>` | hides the two tags and underlines what is between them |
| 11 | never close an underline pair | hides the two tags and underlines what is between them |
| 12 | give `Subscript` the emphasis class | renders `H~2~O`…; does not confuse `~sub~` with `~~strike~~` |
| 13 | label a fence's language as inline code | keeps the language visible…; gives a fence its own line class and no inline-code mark |
| 14 | unregister `mermaidLayer()` | all four DW-165 tests, in both suites |
| 15 | look for the wrong fence info string | all four DW-165 tests |
| 16 | never re-paint on a selection change | gives the diagram its source back when the caret is inside |
| 17 | shrink `topBlock`'s search range to a point | six underline tests, including two toolbar ones |
| 18 | refuse a pair the selection contains | takes underline off when the delimiters are inside the selection |
| 19 | leave the checkbox out of the marker group | six task tests |
| 20 | write `* [ ] ` instead of `- [ ] ` | four task tests |
| 21 | **unwrap on a single `CodeMark`** | **SURVIVED — see below** |
| 22 | park the code-block caret before the fence | leaves a caret on the empty middle line; keeps the body selected |
| 23 | give subscript strikethrough's token | five, including the `/`-menu-vs-toolbar parity test |
| 24 | ignore the slash row's caret offset | all five caret tests |
| 25 | spell the menu's underline `____` | inserts Underline…; spells each mark exactly as the toolbar's command spells it |
| 26 | drop the Task list toolbar button | Task list writes a checkbox…; does not let a button take focus |
| 27 | point the Code block button at inline code | Code block fences the selection, where Inline code backticks it in place |
| 28 | assume a closer that is not there | both unterminated-fence tests |
| 29 | cut to the closer's line end regardless | both unterminated-fence tests |
| 30 | never find the closer | five code-block tests |
| 31 | refuse to unwrap at all | seven code-block tests |

### The survivor, and what it exposed

Mutation 21 changed `marks.length >= 2` to `>= 1` and nothing failed. The test
asserted "the result still contains `alpha`, and is not exactly `alpha\n`" — and
the mutant satisfied both by leaving a stray blank line where the fence had been.

Two things were wrong. The assertion was too weak, and is now the whole document.
And the guard it defended was the wrong shape: refusing to unwrap an unterminated
fence meant a user who had typed ` ``` ` and started writing could not press the
button again to undo it. An unterminated fence now toggles off like any other,
dropping the one fence line it has and nothing else. Mutations 28–31 attack the
new shape; all four are caught.

### The deletion trap, hit and recorded

Mutation 14 deletes a line. Its inverse, `replace("", anchor)`, found 36,906
occurrences of the empty string, the uniqueness guard refused, and
**`mermaidLayer()` was left out of `live-preview.ts`'s extension array** — a live
mutant in a worktree six other agents were editing. The guard refusing loudly is
what caught it; it was restored by hand and every anchor re-grepped by name.
Deletion mutations after that leave a unique sentinel behind, so the inverse
replacement is as targeted as the forward one.

No `#[ts(export)]` type was mutated, so `src/lib/ipc/gen` is untouched.

## What I could not verify here, and why

- **Nothing was compiled by `cargo`, `tsc`, `biome` or the full suite.** The
  brief reserves those for the gate. This story is frontend-only and adds no
  Rust, so the `keeper` shell crate's Linux build problem does not arise — but
  `bun run typecheck` and `bun run lint` have not run over these files, and the
  five-agent-concurrent state of this worktree means the gate is the first place
  the whole tree is seen at once.
- **No pixel was looked at.** jsdom does no layout. `.cm-lp-sub`'s
  `vertical-align: sub` at `0.75em`, the checkbox's three-column footprint, and
  the fence-info's size are asserted only as the classes being present on the
  right text. Whether a subscript actually sits below the baseline, and whether
  ticking a box reflows the paragraph, needs a real browser.
- **Mermaid never actually drew.** Under jsdom the widget degrades — mermaid
  calls `getComputedTextLength`, which jsdom lacks — so what is proved is that
  the `EditorView` constructs, the widget is placed, and the fence's source is
  replaced. That the diagram is *correct* is 37.8's, and unchanged.
- **Obsidian was not opened.** Every claim in the spellings table above is about
  what CommonMark, GFM and Obsidian's documented markdown do with those bytes, and
  is verified here only to the extent that keeper's own parser agrees (which it
  does for `~`, `^`, `- [ ]` and `__`-is-bold — that last one is asserted in a
  test). Nobody in this wave has an Obsidian install to check against.
- **The `title` attribute is not a hover test.** It is asserted as an attribute
  on the mark; that a tooltip appears is the browser's.
- **Two spurious reds during the sweep**, in tests unrelated to the mutation
  applied, neither reproducible, both matching the empty-`DOMRectList` flake
  W2Table diagnosed. No verdict rested on them — every mutation was caught by
  its own test, and the final green here is judged by **exit code**, not by the
  summary line: a jsdom throw escaping CodeMirror's measure pass takes the exit
  code while the summary still prints passes. Three exit-0 repeats of the
  173-test scope, re-run after the wave's later edits landed on the same files.
- **`format-toolbar.test.tsx` now installs `withRangeRects` in `beforeAll` with
  the undo in `afterAll`**, replacing a hand-rolled shim that returned an EMPTY
  rect list and installed itself only `if (!Range.prototype.getClientRects)`.
  W2Emoji argued install-and-leave was the only shape that survives a file
  mounting the real `NoteEditor`; measured, that is true of `afterEach` and not
  of `afterAll` — this file mounts `NoteEditor` in 12 of its 15 tests and exits
  0 with the undo paired, four times over.
- **The shim survives in one other file, and since that is an absence claim it
  is stated with the query the next person would write.** `Range\.prototype\.
  getClientRects` over `src/`, minus `src/test/layout.ts`, finds
  `attachments-panel.test.tsx` and nothing else. Two earlier "it is gone from
  `src/` entirely" claims were claims about a command rather than about the
  tree: the harness's grep takes a Rust regex, and an unescaped
  `if (!Range.prototype.getClientRects)` reads its parentheses as a capture
  group, so it can never match text containing a literal `(` — it returns a
  clean no-match that looks like success. **An absence asserted with an
  unescaped-metacharacter regex is not an absence.** The remaining file is
  W2Attach's, not mine.
- **The mechanism behind that shim, corrected and then measured.** Several
  specs in this wave (including an earlier draft of this one) say the copies
  were order-dependent — in force or not depending on which file the worker had
  already run — and that pairing everyone else's undo made the hazard certain.
  That is wrong, and W2Table caught it: vitest defaults to `isolate: true` and
  `vitest.config.ts` overrides neither `isolate` nor `pool`. W2Table derived
  that from the installed package and said so; I measured it, because the
  claim was written down as fact. Two probe files in one run: file A installs a
  marker on `Range.prototype`, file B reports what it can see. B saw
  `marker: false` and `getClientRects: undefined` at its own start. **Every test
  file gets a clean `Range.prototype`, so `if (!Range.prototype.getClientRects)`
  was always true and the order-dependence was never real.** The defect is
  solely the EMPTY `DOMRectList` those copies install — a measure pass that runs
  reads `rects[0]` as undefined and throws. What is load-dependent is only
  whether an animation frame elapses during a test at all, which is why a busy
  box turned a permanent latent fault into an occasional red.
- **Reds elsewhere in `src/components/notes/editor/` during this story were
  W2Embeds' and W2Media's in flight** — the `CsvTableWidget` → `FileEmbedWidget`
  lift, and at one point 22 tests failing on a single missing import. The same
  directory run was green on both files with all of this story's `live-preview.ts`
  changes already applied.
