---
title: "Highlight and emoji, in the formatting toolbar"
type: 'feature'
created: '2026-08-20'
status: 'done'
baseline_commit: 'c02c9dbc'
review_loop_iteration: 0
context: []
---

<frozen-after-approval reason="human-owned intent — do not modify unless human renegotiates">

## Intent

**Problem:** Two things the toolbar does not offer. `==highlight==` is the one common inline mark keeper has no button for — and, more than that, no *support* for: the parser this editor loads is CommonMark + GFM + sub/superscript, and none of those know `==`, so typing it by hand leaves literal equals signs in the rendered note. Emoji can only be reached by typing `:sho`, which requires knowing the shortcode already; there is no way to browse.

**Approach:** Both are additions to `format-toolbar.tsx`, which speaks in `FormatAction` data while `note-editor.tsx` turns each action into a command. Highlight needs one more thing first — a `@lezer/markdown` extension defining the `==` delimiter, modelled on the Strikethrough extension already in that package — after which the existing tables in `live-preview.ts` do the rendering. Emoji reuses `matchEmoji`, the vocabulary `:shortcode:` autocomplete already searches; the picker is a second door into it, not a second copy.

## Boundaries & Constraints

**Always:**
- One markdown configuration. The `==` extension is defined once and passed at both `markdown()` call sites (`note-editor.tsx`, `markdown-preview.ts`) — a highlight that renders in a note and not in the same file opened from Files is the divergence `markdown-preview.ts`'s own header forbids.
- Highlight behaves like every other inline mark: applied over a selection, toggled off when already inside one, delimiters hidden except on the line being edited, exactly as `Strikethrough` is.
- Emoji inserts the **character**, not the shortcode. `emoji-complete.ts` already resolves `:tada:` to 🎉 on commit; a picker that wrote `:tada:` back into the buffer would make the two doors disagree.
- Every button keeps the toolbar's contract: `aria-label` and `title`, and `mousedown` cancelled so the caret never leaves the note.

**Ask First:**
- Skin-tone variants and a recents list. Both are state; this is a picker.
- Any change to `EMOJI_TABLE` or to how it is generated.

**Never:**
- No second markdown renderer and no second emoji vocabulary.
- No new dependency: the `==` extension is ~25 lines against `@lezer/markdown`'s own interface, and the picker uses the toolbar's hand-rolled popover, not a menu primitive (the toolbar header says why).

## I/O & Edge-Case Matrix

| Scenario | Input / State | Expected Output / Behavior | Error Handling |
|----------|--------------|---------------------------|----------------|
| Highlight a selection | `beta` selected, Highlight clicked | `==beta==`, selection preserved | N/A |
| Toggle off | caret inside `==beta==` | delimiters removed | N/A |
| Render | `==beta==` on an unfocused line | highlighted, `==` hidden | N/A |
| Render in a file | the same bytes opened from Files | identical | N/A |
| Not a highlight | `a == b` | untouched: no delimiter pair, no node | N/A |
| Pick an emoji | picker open, 🎉 clicked | `🎉` at the caret, picker closed, caret in the note | N/A |
| Search | "tad" typed in the picker | `matchEmoji` order, closest first | no matches: a sentence, not an empty grid |
| Reopen | picker opened again | the query is cleared | N/A |

</frozen-after-approval>

## Code Map

- `src/components/notes/editor/markdown-marks.ts` -- **new**: the `Highlight` / `HighlightMark` delimiter extension
- `src/components/notes/editor/format-commands.ts:43` -- `FormatAction` gains `{kind:"mark"}` and `{kind:"emoji";text}`; `formatCommand` gains both cases; `MARK` joins the `InlineMark` constants
- `src/components/notes/editor/live-preview.ts:54,66,603` -- `HighlightMark` into `HIDDEN_MARKS`, `Highlight` into `INLINE_CLASSES`, `.cm-lp-mark` into the theme
- `src/components/notes/note-editor.tsx:561` and `src/components/viewers/markdown-preview.ts:349` -- both `markdown()` calls take the extension
- `src/components/notes/format-toolbar.tsx:57,140` -- a Highlighter entry in `DIRECT`; an emoji button and a third hand-rolled `fieldset` panel beside the heading and table ones
- `src/lib/emoji/match.ts` -- `matchEmoji`, unchanged

## Tasks & Acceptance

**Execution:**
- [x] `markdown-marks.ts` + test: `==x==` parses to `Highlight`; `a == b`, `===`, and an unclosed `==` do not
- [x] `format-commands.ts` + test: mark toggles on and off like `STRIKE`; emoji inserts at the caret and replaces a selection
- [x] `live-preview.ts` + test: the delimiters hide off the active line and the span carries `cm-lp-mark`
- [x] both `markdown()` call sites
- [x] `format-toolbar.tsx` + test: the two buttons, the picker's search, its empty state, and that picking closes it

**Acceptance Criteria:**
- Given text selected in a note, when Highlight is clicked, then the text is wrapped in `==` and renders highlighted with its delimiters hidden; clicking again removes them.
- Given the same file opened from the Files pane, when it renders, then the highlight looks the same as it does in the note.
- Given the emoji picker open, when a query is typed and an emoji clicked, then that character is inserted at the caret and the panel closes with focus back in the note.
- Given `a == b` anywhere in a note, when it renders, then nothing is highlighted and no character is hidden.

## Verification

- `bun run typecheck` · `bun run test -- format-commands markdown-marks live-preview format-toolbar` · `bun run lint` · `bun run check:design`
- Manual: highlight a word, unfocus the line, and open the same note's file in the Files pane.

## What the checks caught

**The picker opened on a wall of flags.** `matchEmoji("")` returns the table's head, and `EMOJI_TABLE` is ordered by shortcode — so the first thing the panel showed was `+1`, `100`, `1234`, `8ball`, `a`, `ab`, `abacus` and a run of country flags. Accurate, and nothing anybody came there for. Replaced with a 48-shortcode opening set (faces, hands, the marks a note actually uses), named as **shortcodes** resolved through `emojiFor` so the emoji table stays the only place a character is written down. Found by looking at it.

**The panel and its button both answered to "Emoji".** The two panels beside it are named "Heading level" and "Insert table" precisely so they do not collide with "Heading" and "Table"; the new one broke that convention, which is ambiguous to anything navigating by name. Found because a test could not tell them apart — the same ambiguity a screen reader would have hit.

**Two test helpers configured a parser the app does not load.** `format-commands.test.ts` and `live-preview-marks.test.ts` both built `markdown({ base: markdownLanguage })` with no extensions. Harmless until this story, and then exactly wrong: a helper that configures less than production can pass for a build that fails. Both now pass `MARKDOWN_MARKS`.

**`===x===` is not a special case.** The first draft of the parser test asserted it yields nothing. It actually yields `==x===`, which is what CommonMark's flanking algorithm gives — and precisely what the built-in `~~` gives for `a ~~~x~~~ b`. The test now pins that parity rather than a rule this extension does not have.
