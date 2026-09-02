---
title: 'Story 61.5: the answer is readable'
type: 'feature'
created: '2026-09-02'
status: 'review'
baseline_revision: '9e06c2b'
review_loop_iteration: 0
followup_review_recommended: true
context:
  - '{project-root}/docs/project-context.md'
---

binds: FR-380 — AD-153; the `note_protocol.rs:31-34` position consumed ("a note that auto-fetches a URL an agent wrote is a tracking pixel")
depends on: 61.4 (`bot-answer.tsx` and its `{ body: string }` prop shape; `bot-message.tsx:72` is unchanged)

<intent-contract>

## Intent

**Problem:** 61.4's `bot-answer.tsx` printed the reply as plain text. A model's answer is prose and code, and rendering it is exactly
where a chat surface reaches for `dangerouslySetInnerHTML` and a remote image. **An answer is prose and code, and nothing else is executed.**

**Approach:** A pure parser, `src/lib/bots-markdown.ts`, runs `@lezer/markdown` (configured with only `Table` and `Strikethrough`) into
plain data blocks; `bot-answer.tsx` turns blocks into React elements. No new dependency, no HTML string anywhere, no `<a>`, no `<img>`,
nothing fetched. Keys are position-derived and each block carries its own `source`, so a settled block keeps its DOM node across the
deltas that follow it.

## Boundaries & Constraints

**Always** (each invariant with the tests that pin it):
- An HTML block, an inline HTML tag, an entity, and anything outside the subset render as their **literal source characters**; entities
  stay undecoded on purpose — decoding is the first half of rendering HTML — *renders an HTML block as its own source, never as a construct*,
  *renders an inline HTML tag as the characters the model sent*, *leaves an HTML entity undecoded — decoding is the first half of rendering
  HTML*, *renders a script tag the model wrote as text, never as a node*.
- A link is text, not a destination: the label plus the URL in muted parentheses when it differs; no `href` — *keeps a link's label and its
  URL, and hands back no destination of its own*, *shows a link's label and its URL, and creates no destination*.
- No character of the source is lost, whatever the input — *loses no character of the source, whatever the input*, *unescapes a backslash
  escape to the one character it meant*.
- An unterminated fence mid-stream is already a code block (Lezer runs it to the end of the document); the block closes itself and says in
  `data-closed` that it is open. An unterminated `**` is a text run and cannot retroactively bold the rest of the answer (Lezer emits
  emphasis only once the closing run arrives) — *closes an unterminated fence itself and says it is open*, *closes an unterminated fence
  itself rather than swallowing what arrived*, *does not bold the rest of an answer from an unterminated marker*, *does not bold an answer
  retroactively from a marker still being typed*.
- Keys derive from **nothing that changes per token** — not content, not length, not a hash; `BotBlock` is `memo`ised on `(kind, source)`,
  so a delta that grows the last block re-renders the last block only, and the final prefix parses to exactly what the whole answer parses
  to — *gives a settled block the same key at every prefix that contains it*, *keys blocks by position, so a growing answer never renumbers
  its start*, *keeps the DOM node of a settled block across the deltas that follow it* (proved by `toBe` on the same `<p>`, which a remount
  fails and markup equality would pass), *parses the final prefix to exactly what the whole answer parses to*, *lands on the same DOM whether
  it arrived at once or token by token*.
- A 200 kB answer parses inside a stated budget (~110 ms measured, 1 s bound) and renders inside one (~1.4 s jsdom, 10 s bound); `sliceText`
  binary-searches the document-ordered mark ranges — the naive linear form was quadratic, 1.9 s per delta at 200 kB — *parses a 200 kB
  answer inside a stated budget*, *renders a 200 kB answer inside a stated budget*.
- A fenced block has a language label (`BOT_CODE_PLAIN_LABEL` when none) and a copy control; a refused clipboard says nothing and shows
  no error, the `recording-row.tsx` / `properties-panel.tsx` shape — *labels its language and puts its text on the clipboard, confirming it*,
  *says nothing and shows no error when the webview refuses the clipboard*; the subset itself — the nine construct tests in
  `bots-markdown.test.ts` and *renders the supported constructs as elements, not as punctuation*.

**Block If:** nothing. **Never:** `dangerouslySetInnerHTML`; a third markdown dependency; an `<img>`; an `<a>`; a `<script>` as a node; a
change to `bot-message.tsx`, `client.ts`, `mock-shell.ts`, `stores/bots.ts`, `bots-pane.tsx`, Rust, `index.css` or `DESIGN.md`.

## I/O & Edge-Case Matrix

| Scenario | Input / State | Expected Output / Behavior | Error Handling |
|----------|--------------|---------------------------|----------------|
| Paragraph / headings | text with single newlines; ATX and setext | `<p class="whitespace-pre-wrap break-words">` keeping the model's breaks; real `<h1>`…`<h6>` tuned under the pane's headings | — |
| Emphasis / inline code | bold, italic, strikethrough, `code` | `<strong>` / `<em>` / `<s>` / `<code>` chip | — |
| Fenced / indented code | ```` ```ts ```` …; four-space block | bordered block, label, copy control, `data-closed`; same block de-indented, no language, always closed | unterminated → renders, marked open |
| Lists / quote / rule / table | nested, ordered; `>`; `---`; GFM table | `<ol start=N>` / `<ul>` (an item is a block container); `<blockquote>` with markers stripped on continuation lines; `<hr>`; `<table>` with `<thead>`/`<tbody>` | — |
| Link / escape | `[label](url)`; `\*` | label, URL in muted parentheses if different, no `<a>`; the one character it meant | — |
| HTML / entity | `<script>`, `<b>`, `&amp;` | literal characters | never a node, never decoded |
| Clipboard refused | `writeText` rejects | nothing said, no error | swallowed |

</intent-contract>

## Code Map

| file | state | what shipped |
|---|---|---|
| `src/lib/bots-markdown.ts` | new, 454 | `parseAnswer`, `MdBlock`, `MdInline`, `MdListItem`, `BOT_CODE_PLAIN_LABEL`, `BOT_CODE_COPY_LABEL`, `BOT_CODE_COPIED_LABEL`; no React, no HTML string |
| `src/components/bots/bot-answer.tsx` | rewritten, 301 | blocks → elements; `BotBlock` memo on `(kind, source)`; `HEADING_TAG`; code chrome; prop shape `{ body: string }` unchanged |
| `src/lib/bots-markdown.test.ts` | new, 227 | 21 tests: the subset, the outside-the-subset escape, the streaming properties, the 200 kB budget |
| `src/components/bots/bot-answer.test.tsx` | new, 192 | 11 tests: DOM constructs, script-as-text, copy (+ refused clipboard), open fence, token-by-token vs one-shot, node identity, 200 kB render budget |

Shared files touched: none. The coordinator has nothing to integrate from this story.

## Tasks & Acceptance

**Execution:** [x] the parser over the lezer stack already in `package.json` · [x] the renderer · [x] the copy control and language label ·
[x] the streaming properties proved at the parser and at the DOM · [x] the budget tests. **Acceptance Criteria:** prose and code, no remote
fetch, no HTML injection — **met**; fenced blocks get a copy control and a language label — **met**; an unterminated fence renders as code
and closes itself — **met**; a 200 kB reply does not lock the webview — **met** in jsdom, within the stated bounds.

## Design Notes

**Divergence from AD-153, flagged.** The architecture says an `http(s)` image "renders as its alt text and a link the user may open". The
shipped renderer creates **no** link element at all — a link is its label plus its URL as text — and an image is text by the same rule.
Stricter than the AD; AD-153 should record it, or the coordinator should say whether an open-in-browser affordance is wanted.

## Verification

- `bunx vitest run src/lib/bots-markdown.test.ts src/components/bots/bot-answer.test.tsx` → 2 files, 32 tests. `bunx tsc --noEmit` zero errors
  in the four files; biome clean (three index-key suppressions carry house-style reasons, matching `edit-history-popover.tsx` /
  `document-viewer.tsx`). `bots-pane.test.tsx` still 9/9 with the new renderer mounted. Coordinator's tree-wide gate: 5466 frontend tests
  green, lint, typecheck clean, `node scripts/check-design.mjs` at zero violations (the `bot-answer.tsx:50` type-scale finding 61.7 observed mid-wave is gone).

| Mutation | Test that failed |
|---|---|
| (a) `fenceIsClosed` returns `true` unconditionally | *closes an unterminated fence itself and says it is open*; *closes an unterminated fence itself rather than swallowing what arrived* |
| (b) `toBlock`'s `default` returns an empty paragraph instead of a `literal` block | *renders an HTML block as its own source, never as a construct*; *renders a script tag the model wrote as text, never as a node* |
| (c1) key becomes `${prefix}${index}c${blockLength}` | *keys blocks by position, so a growing answer never renumbers its start* — the DOM-identity test survives (a settled block's own length does not change), which is why c2 was run |
| (c2) key includes the whole answer's length | *keys blocks by position…*; *keeps the DOM node of a settled block across the deltas that follow it* |

`diff` against the pre-mutation copies empty; 32/32 and biome clean after restore. **Not verified here:** nothing rendered in a real WebView —
code chrome, heading scale, table overflow and the blockquote rule are untouched by an eye or a pixel, and jsdom applies no Tailwind, so a
class resolving to nothing passes every test. The clipboard is jsdom's mock. The performance numbers are this container under vitest, not the
shipped WebView under a 30-tokens-per-second stream; the memo's saving during a live stream is argued, not measured. Real model output may
hold constructs — reference links, footnotes, math, mermaid fences — that land in the `literal` path: visible and honest, unstyled.
