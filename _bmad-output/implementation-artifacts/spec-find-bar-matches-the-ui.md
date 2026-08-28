---
title: "The in-note find bar looks like the rest of the app"
type: 'bugfix'
created: '2026-08-20'
status: 'done'
baseline_commit: '7e3c6585'
review_loop_iteration: 0
context: []
---

<frozen-after-approval reason="human-owned intent — do not modify unless human renegotiates">

## Intent

**Problem:** `⌘F` inside a note opens CodeMirror's stock search panel. Nothing in this repo has ever styled it, so it renders with the browser's own controls: two rows split by a `<br>`, native text inputs, three native checkboxes labelled "match case / regexp / by word", four buttons whose labels are the lowercase words "next / previous / all / replace all", and a `×` glued to the corner. Beside the note list's filter bar — the same job, one row, ghost icon buttons, an `Input` — it reads as a different application.

**Approach:** Replace the panel's presentation, not its behaviour. `search()` takes a `createPanel` option; the panel it returns drives the same exported commands (`findNext`, `findPrevious`, `selectMatches`, `replaceNext`, `replaceAll`, `closeSearchPanel`) over the same `SearchQuery` state. Nothing about matching, regexp semantics, or the keymap changes — only the DOM in front of it, built from the primitives `note-filter-bar.tsx` already uses.

## Boundaries & Constraints

**Always:**
- CodeMirror stays the owner of search state. The panel reads `getSearchQuery` and writes `setSearchQuery`; it keeps no second copy of the query that could drift.
- Every keystroke the stock panel honoured still works: `Enter` / `Shift-Enter` step through matches from the find field, `Enter` in the replace field replaces, `Escape` closes and returns focus to the note, and `runScopeHandlers(view, e, "search-panel")` still gets first refusal so `searchKeymap` bindings keep firing inside the panel.
- Every control keeps an accessible name. The stock panel's text labels become icons, and an icon with no name is a button nobody can use — each gets a `Tooltip` and an `aria-label`, and the three modes report state via `aria-pressed`.
- The panel matches `note-filter-bar.tsx` because that bar is the app's answer to this shape already: `Input` at `h-8`, `Button variant="ghost" size="xs"`, `size-3` icons, `aria-pressed:bg-accent`.

**Ask First:**
- A match counter ("3 of 17"). Genuinely missed, and not a restyle — counting every match on each keystroke is a performance question about large notes that deserves its own answer.
- Bringing find to the file viewers. They mount CodeMirror without `search()` at all, so that is a new feature, not a restyle.

**Never:**
- No reimplementation of search. If a command exists in `@codemirror/search`, this panel calls it.
- No change to `searchKeymap`, to `⌘F` ownership, or to the `top: true` position.
- No new dependency.

## I/O & Edge-Case Matrix

| Scenario | Input / State | Expected Output / Behavior | Error Handling |
|----------|--------------|---------------------------|----------------|
| Open | `⌘F` in a note | one-row bar at the top, find field focused and its text selected | N/A |
| Step | `Enter`, `Shift-Enter` | next / previous match, wrapping as CodeMirror already does | N/A |
| Modes | click `Aa`, `.*`, `ab|` | query re-dispatched with that flag; button reports `aria-pressed` | invalid regexp: CodeMirror's own no-match behaviour, no throw |
| Replace | chevron expands the replace row | `replace` and `replace all` act on the current query | hidden entirely when `state.readOnly` |
| External change | another extension dispatches `setSearchQuery` | the fields show the new query | N/A |
| Close | `Escape` or the `×` | panel closes, caret back in the note | N/A |
| Reopen with a selection | `⌘F` with text selected | the field holds it, as today | N/A |

</frozen-after-approval>

## Code Map

- `src/components/notes/note-editor.tsx:521` -- `cmSearch.search({ top: true })`; gains `createPanel`
- `src/components/notes/editor/find-panel.tsx` -- **new**: the React bar and the `Panel` factory around it
- `src/components/notes/editor/file-embed-host.tsx` -- the house pattern for a React root inside a CodeMirror-owned element; this follows it
- `src/components/notes/note-filter-bar.tsx:235-345` -- the visual contract being matched
- `node_modules/@codemirror/search` -- `createPanel`, the commands, `getSearchQuery` / `setSearchQuery` / `SearchQuery`

## Tasks & Acceptance

**Execution:**
- [x] `src/components/notes/editor/find-panel.tsx` -- `FindBar` component plus `createFindPanel(view): Panel` (`dom`, `mount`, `update`, `destroy`, `top`), rendering through `createRoot`
- [x] same file -- `findPanelTheme`: an `EditorView.theme` that strips `.cm-panels`' hardcoded `#ddd` bottom border and background, so the bar draws its own `border-border` and is correct in dark mode
- [x] `src/components/notes/note-editor.tsx` -- one `find.findBar()` in the extension list, rather than three things a caller has to remember to combine
- [x] `src/components/notes/editor/find-panel.test.tsx` -- the matrix: opens focused, Enter/Shift-Enter step, each mode toggles and re-dispatches, replace row hidden under `readOnly`, Escape closes, an external `setSearchQuery` reaches the fields

**Acceptance Criteria:**
- Given a note with `⌘F` pressed, when the bar appears, then it is one row of `h-8` input and ghost icon buttons matching the note list's filter bar, and no native checkbox or lowercase word-button is rendered.
- Given the bar is open, when `Enter`, `Shift-Enter` and `Escape` are pressed, then the selection moves to the next match, the previous match, and the panel closes with focus back in the note — the same three outcomes the stock panel produced.
- Given each of the three mode buttons, when clicked, then `aria-pressed` flips and the dispatched `SearchQuery` carries the matching flag.
- Given a read-only editor, when the bar opens, then no replace control exists in the DOM.

## Verification

- `bun run typecheck` · `bun run test -- find-panel note-editor` · `bun run lint`
- Manual: `⌘F` in a note, compare against the note list's filter bar in both themes.

## What the checks caught

Three defects that all ten unit tests were happy with. Worth writing down, because each was invisible to a different kind of looking.

**The panel opened underneath the note.** `showPanel` asks the *panel object* where it goes and puts a silent one at the bottom; `search({ top })` positions the slot but does not answer for the panel. The stock `SearchPanel` has a `get top()` forwarding the config, and this one had nothing. Caught by rendering it against the real stylesheet and looking. `PANEL_AT_TOP` is now stated once and read by both, and a test asserts the bar is inside `.cm-panels-top`.

**At 280px the find field collapsed to 22px.** Nine controls in one row, and 280 is exactly the floor the note panel reaches after Story 55.1 — so the two stories together would have produced an unreadable field at the size the previous one just made reachable. Measured across widths in a real engine:

| panel | find field | overflow |
|---|---|---|
| 720 | 448 | 0 |
| 560 | 288 | 0 |
| 420 | 148 | 0 |
| 320 | 160 (wraps to two rows) | 0 |
| 280 | 202 (wraps to two rows) | 0 |

The row wraps and the field has a floor. Two rows of usable controls beat one row of unusable ones.

**The panel opened without the caret.** `mount()` ran a microtask before React had committed, so it selected a field that did not exist yet. Moved into the component, which is the only thing that knows when its input is real — and made `focus()` *then* `select()`, because the stock panel relied on focus arriving as a side effect of `select()` that the spec never promised.
