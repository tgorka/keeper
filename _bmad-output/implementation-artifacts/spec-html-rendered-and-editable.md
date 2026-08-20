---
title: "HTML has a rendered half, and its text can be edited"
type: 'feature'
created: '2026-08-20'
status: 'done'
baseline_commit: 'f0378b65'
review_loop_iteration: 0
context: []
---

<frozen-after-approval reason="human-owned intent — do not modify unless human renegotiates">

## Intent

**Problem:** `.html` is a `sourceRow` — raw text and nothing else. A Files panel shows the markup, a note embeds it as a link, and there is nowhere in keeper that shows what the page says. Markdown has Preview, CSV has a table, JSON has a structure list; HTML has the angle brackets.

**Approach:** A fourth `RenderedView`, built the way this repo already insists HTML be handled — **`textContent`, never `innerHTML`**. `DOMParser` produces an inert document (no script runs, no image loads), and the view is then built node by node through an allowlist with `createElement`, so no markup string ever reaches a live parser.

Editing is the rendered **text**, not the markup: each text run carries the source offsets it came from, and an edit splices those bytes and hands the whole new file to `onChange` — the same path the Source tab already writes through. No new command, no new write contract.

## Boundaries & Constraints

**Always:**
- Nothing is fetched. Ever. `keeper never fetches a remote URL` is NFR-11, and it is why a remote `<img>` is shown as its address rather than loaded — the same answer `live-preview.ts` already gives a remote image in a note.
- No `innerHTML`, no `<script>`, no `<iframe>`, no event-handler attributes, no `javascript:`. The allowlist is a list of what is kept, so an element nobody thought about is dropped rather than passed.
- **A splice is verified before it is applied.** The text at the recorded offsets must still be the text the node was built from; if it is not, the edit is refused with a sentence and the file is untouched. A mapping bug must cost an edit, never a file.
- Byte fidelity everywhere else: a file opened, edited in one paragraph and saved differs by that paragraph and nothing else. No reserialisation of the document.
- The Source tab stays exactly what it was, and is still the tab that can change anything (AD-88).

**Ask First:**
- Editing anything but text: attributes, structure, adding or deleting elements.
- Rendering CSS. A `<style>` block's content is not shown and not applied; styling arbitrary CSS into the app's own DOM is a separate question with its own answer.
- Making the JSON structure view editable. It says it is read-only and why; changing that is its own story.

**Never:**
- No new dependency, and no HTML parser of our own beyond a text-run scanner.
- Nothing rendered from a string. The DOM is constructed, element by element.

## I/O & Edge-Case Matrix

| Scenario | Input / State | Expected Output / Behavior | Error Handling |
|----------|--------------|---------------------------|----------------|
| A page | `<h1>Title</h1><p>Body</p>` | a heading and a paragraph | N/A |
| Edit | the paragraph's text changed | those bytes replaced, the rest identical | N/A |
| Script | `<script>alert(1)</script>` | dropped; nothing runs, nothing shown | N/A |
| Remote image | `<img src="https://x/p.png">` | the address as text, not a request | N/A |
| Handler | `<div onclick="…">` | element kept, attribute dropped | N/A |
| Unknown element | `<custom-thing>text</custom-thing>` | the text, in a neutral wrapper | N/A |
| Entity | `&amp;` in the source | renders `&`; editing splices the **new text**, and the entity is gone by the reader's own act | N/A |
| Drifted offsets | the buffer changed under the view | refuse the edit, say so | never a silent write |
| Fragment | no `<html>`/`<body>` | rendered as a fragment | N/A |
| Empty file | `""` | an empty view, not a failure | N/A |
| In a note | `![[page.html]]` | the same panel every other data embed gets | N/A |

</frozen-after-approval>

## Code Map

- `src/lib/viewers/types.ts:81` -- `RenderedView` gains `"html"`
- `src/lib/viewers/registry.ts` -- `html`/`htm` move from `sourceRow` to a `textRow` with `rendered: "html"`
- `src/components/viewers/html-view.ts` -- **new**: the text-run scanner, the allowlist build, and the verified splice
- `src/components/viewers/raw-rendered-view.tsx` -- an `HtmlPane` beside `StructurePane`, and `html` in the tab table
- `src/components/notes/editor/file-embed.ts:127` -- `"html"` joins table and structure, so a note embed gets the panel

## Tasks & Acceptance

**Execution:**
- [x] `html-view.ts` + test: the scanner's offsets, the allowlist's drops, the entity case, the verified splice and its refusal
- [x] the registry and the union
- [x] `raw-rendered-view.tsx` + test: the tab exists, the text is editable, an edit reaches `onChange` with the whole file
- [x] `file-embed.ts` + test: `![[page.html]]` gets a panel

**Acceptance Criteria:**
- Given an `.html` file, when it is opened in a Files panel, then Rendered shows the page's text and structure, and Source still shows the markup unchanged.
- Given a `<script>`, a remote `<img>` and an `onclick`, when the file renders, then nothing executes, nothing is fetched, and the address is visible as text.
- Given a paragraph edited in the rendered view, when the change lands, then `onChange` receives the whole file with those bytes replaced and every other byte identical.
- Given the buffer no longer matches what the view was built from, when an edit is attempted, then it is refused with a sentence and no write happens.
- Given a note embedding `![[page.html]]`, when it renders, then it gets the same panel a `.csv` embed gets.

## Verification

- `bun run typecheck` · `bun run test` (**and its error count, not only its pass count**) · `bun run lint` · `bun run check:design`
- Manual: an `.html` file with a heading, a link, a remote image and a script.

## What the work turned up

**It rendered safely and looked like nothing.** The first build was correct on every safety case — script dropped, image not fetched, handler gone — and Tailwind's preflight had removed every element default, so the heading was body text, the list had no markers and the table's cells ran together. A "Page" view where a heading is not a heading is a viewer that renders and shows nothing. Found by looking at it in a real engine, and fixed with a scoped style block built from the theme's tokens: the elements it styles are exactly the elements the allowlist can produce, so the two lists are one list.

**HTML left the `source` format, by that union's own rule.** `ViewerFormat` collapses every programming language into `source` because "they differ in `language` and in nothing else a surface acts on". HTML now differs in something a surface acts on — it has a rendered half — and the remembered-view cookie is keyed by format, so leaving it there would file "I prefer the Page tab" under a key every `.rs` also reads.

**A suppression that suppressed nothing.** The effect's dependency comment was written as a `biome-ignore`, and Biome reported it unused: that rule is not enabled here. The reason was worth keeping and the suppression was not — a suppression that suppresses nothing is a comment pretending to be a guarantee.

**Whitespace runs are runs.** The newline after a `</p>` is a real text node with a real source range, so it keeps its index — the mapping is by order — but it is not an edit target. An invisible place to type is a place to lose a keystroke.

**Verified in a real engine, not only in jsdom.** Retyping a paragraph produced exactly one changed run: `<p>Rewritten by the reader<a href="…">link</a> and <strong>bold</strong>.</p>` — the anchor, the bold and every byte outside that run untouched.
