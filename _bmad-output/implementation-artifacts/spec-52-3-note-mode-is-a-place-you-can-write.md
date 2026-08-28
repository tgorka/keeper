# Spec 52.3 — Note mode is a place you can write

story: 52.3
status: review
branch: `work/epic-52-note-mode-writes` (on top of `work/epic-52-rename-follows`)
baseline_revision: c873fa6
final_revision: ''
binds: FR-303, FR-304, FR-305; NFR-27 (the lazy chunk boundary)
sentinel: `MUT52-3`

<intent-contract>

**Three asks, verbatim.** *"note tab w sesions nie ma panelu przyciskow edycji"*,
*"note tab w sesions nie musi renderowac czesci properties jak juz jest powyzej
formularz"*, *"note mode powinien byc default otwierany jezeli jest mozliwy (nie
preview)"*.

**Why the toolbar is missing.** `FormatToolbar` has exactly two mount sites:
`note-editor.tsx:1099` (the Notes surface) and `text-viewer.tsx:264` (the SOURCE
tab). The Note pane is `MarkdownPane` (`raw-rendered-view.tsx:393-490`), built
from `mountMarkdownPreview` alone — and `markdown-preview.ts` imports no writing
tools, so Note mode has no toolbar, no slash menu and no emoji completion. Story
50.3's "one editor host, one set of writing tools" reached the raw editor and not
this one.

**Why properties render twice.** On a file the buffer IS the whole file, YAML block
included — unlike a note, where frontmatter is a separate store field. So
`FileProperties` draws the block as a form (`text-file-frame.tsx:391-395`) and
`MarkdownPane` draws the same bytes as document text. Nothing is wrong with either
renderer; the duplication is that no one hides the block from the view that has a
form above it.

**Always**
- Note mode gets the same writing tools the Notes surface has: the toolbar, the
  slash menu, emoji completion, and `runFormatAction`. One module
  (`editor/writing-tools.ts`), reached the same lazy way (NFR-27) — never a second
  copy.
- When a properties FORM is mounted above the view AND is holding the file's
  block, the Note and Preview panes do not also render those exact bytes as
  text. What travels down is the BLOCK the form holds — Rust's `block_of` over
  the file on disk — never a boolean about whether a form was mounted: a second
  recogniser over the live buffer is how a form and a pane come to disagree.
  Nothing is hidden while that read is in flight, after it refused, when the
  buffer's first bytes are not that block, or when the form draws none of the
  block's own characters (a leading `---` block that is all comments — a
  thematic break — whose prose would then be on no screen but Source). The bytes
  on disk keep it; only the display hides it, and an edit still saves the whole
  file.
- A properties write must not cost the reader text he has typed and not saved.
  The write itself is safe — Rust guards on the block and preserves every other
  byte — so what follows it is a SPLICE of the landed block over the buffer's own
  leading block, and a re-read only when there is nothing in the buffer to lose.
- Note is the default view when it is possible — `noteOffered` at
  `raw-rendered-view.tsx:592` is already exactly that predicate: markdown, note
  mode allowed, not read-only.
- A remembered choice still wins over the default, for as long as the user keeps
  it. The jar is keyed PER FORMAT (`viewModeFor(cookie, format, offered)`), which
  is `view-mode.ts`'s own rule and not a per-file one: his answer for markdown is
  his answer for every markdown file. Changing the default never overrides an
  answer he already gave.

**Block if**
- The file is read-only, oversize, or Rust refused the write: Note is not offered
  and the default falls back to Preview, unchanged.

**Never**
- Never statically import the writing tools from the preview module: that pulls
  ~45 KB of emoji table into the main bundle and defeats the boundary quick
  capture's 300 ms budget stands on.
- Never strip the block from the bytes that get SAVED. Hiding is a view concern.

</intent-contract>

## Code Map

| where | change |
|---|---|
| `src/components/viewers/markdown-preview.ts:70-92` | `MarkdownEditing` gains the writing-tools seam: the extensions to add and a `runFormat` the host can call |
| `src/components/viewers/raw-rendered-view.tsx:393-490,536-543,592-608,695-736` | `MarkdownPane` renders `FormatToolbar` when editing is offered; the default view becomes Note when `noteOffered`; the cookie's remembered mode still wins; `frontmatterInForm` is the form's BLOCK and `leadingFormBlock` decides which bytes of the buffer are it |
| `src/components/viewers/view-mode.ts:58` | `DEFAULT_VIEW_MODE` becomes a function of what is offered rather than a bare constant |
| `src/components/viewers/text-file-frame.tsx:263-352,497-535` | lifts `FileProperties`' loaded block (`onBlock`) and hands it to the panes; splices a landed block over the buffer's own instead of re-reading over unsaved text |
| `src/components/notes/properties-panel.tsx:883-892,977-982` | `FileProperties` reports the block it is holding, `null` included, so the pane and the form cannot disagree about it |
| `src/components/notes/editor/writing-tools.ts` | unchanged — reused, not copied |

## Tasks & Acceptance

| # | acceptance |
|---|---|
| 1 | the Note tab renders the formatting toolbar, and a toolbar action changes the document |
| 2 | the slash menu and emoji completion work in Note mode |
| 3 | the writing tools are still behind a dynamic `import()` — the guard test that pins the two legal mount sites is updated to three, deliberately, and still forbids a static edge |
| 4 | with the properties form HOLDING the block, the Note pane does not show it as text |
| 5 | saving from Note mode writes the whole file, block included — byte-compared |
| 6 | opening a savable markdown file with no remembered choice lands on Note |
| 7 | a file with a remembered `rendered` choice still opens in Preview |
| 8 | a read-only or oversize file still opens in Preview and offers no Note tab |
| 9 | `spec-51-5:62`'s non-goal is marked superseded by this story, in that file |
| 10 | while the form's read is in flight, and after it refuses, the block is document text — hidden only once the form holds it |
| 11 | a file carrying a byte-order mark has its block hidden once, mark included, and a save writes the mark back |
| 12 | a leading `---` block that is all comments (a thematic break above a heading) stays on screen: the form draws none of it |
| 13 | typing `---` ⏎ `---` at the top of an unblocked document does not shorten the shown text under the caret |
| 14 | typing a paragraph and then writing a property keeps the paragraph, and the buffer a Save would write is the landed block in front of it; with nothing typed the file is re-read instead, exactly once |

## Design Notes

_(filled at review)_

## Verification

_(filled at review)_
