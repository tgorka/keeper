# Epic 53 — The gesture, the fold, and the folder

created: '2026-08-17'
source: the owner's seventh field report, filed against the merged epic-52 build on hesperia (main 8c8a3eb). Five items. One librarian read Tauri's own source; five read-only scouts measured the rest before this spine was written.
binds: FR-314…FR-322 (allocated here), AD-102 (overridden here, explicitly), AD-120, AD-121, DW-37

## Why this epic exists

Item 4 is the third attempt at the same bug, and the first two failed because **the
JavaScript was never the problem**. That is now settled from source, not guessed.
Item 3 turns out not to be a code defect at all. Item 5's feature shipped last
night and did nothing for him, for a reason worth writing down.

| # | the owner's words | verdict | what it means |
|---|---|---|---|
| 1 | fold back the list of tags when I stop choosing, in all views | **deliberate** (list) + **absent** (fold) | `tag-combobox.tsx:10-13` records "a list you have to open is a list nobody browses" (Story 44.13, UX-DR61). The list may stay browsable; what is absent is any close on blur, outside-click or choose — and on both space editors there is **no close path at all** |
| 2 | fold the properties and the info note; the save bar and top bar can be one | **absent** + **deliberate** + **absent** | the notes surface already has the properties fold (`note-editor.tsx:364`); the file surface never got it. The caveat is protected by **AD-102**, quoted below. The title is rendered twice, and the notes surface already proved the merge |
| 3 | the About space requires `recordings` and `about` — change it to only `about` | **not a code defect** | `spaces.rs:126` has always been `tag:about`. The two-term query is in **his** `_spaces/about.md`, and a persisted file shadows the default completely |
| 4 | tasks cannot be dropped into another column; only the dropdown works | **broken, in the platform** | Tauri's OS drop handler intercepts `performDragOperation:` in Rust and returns `YES` **without calling super**, so WKWebView never performs the drop and the page's `drop` never fires |
| 5 | space notes are created in the main folder, not the space's subfolder; the template does not create it either | **deliberate** (a knob) | 52.5 shipped `create_dir` defaulting to empty, and empty is specified as "exactly today's behaviour". He was handed a switch nobody set |

## Item 4, settled from source

`tauri-runtime-wry-2.11.4/src/lib.rs:4862-4896` installs a drag-drop handler whose
body ends in a bare `true` — it always claims the event. `wry-0.55.1`'s macOS
`WryWebView` implements `NSDraggingDestination` **on the WKWebView subclass
itself** (`class/wry_web_view.rs:77-112`) and only forwards to `super` when that
closure returns `false` (`wkwebview/drag_drop.rs:88-95`). So `dragstart` and
`dragover` fire — they are page-internal — and the terminal
`performDragOperation:` is swallowed in Rust before WebKit sees it. The config
doc-comment claiming this matters only on Windows is wrong; upstream tauri#14373
is the maintainer-facing report of that doc bug.

**Two ways out, and we are taking the second.**

The obvious one is `dragDropEnabled: false` on the `main` window. It works, and it
costs `conversation-pane.tsx:814-848` — Story 3.7's *drop an OS file on the chat to
attach it*, the app's only consumer of `onDragDropEvent`. That is a feature the
owner did not ask to lose.

So the board's gesture moves to **pointer events**, which that handler does not
touch. This is not a novel mechanism here: `setPointerCapture` is already the
idiom in five live places — `ui/resizable-columns.tsx:202`,
`hooks/use-swipe-actions.ts:187`, `layout/phone-shell.tsx:355,435,511` and
`layout/pins-strip.tsx:157`. It also ends the blindness that let this ship broken
twice: jsdom has no drag session and can never see an HTML5 drop, but it *can*
drive pointer events, so the fix is testable in the suite that already runs.

While in there: the column's `<ul>` does not fill its box (`task-board.tsx:401`
vs `:364-366`), so the padding, the header and everything below `min-h-16` were
never drop targets — and the highlight was drawn on the *non-droppable* wrapper.
A dead zone that looks live.

## Item 2 overrides AD-102, and says so

`files_write.rs:675-679` — *"**Before, and not after.** An edit that quietly does
less than the vault path does is strictly worse than the refusal it replaces: a
person who finds out after saving that this file has no history has already lost
the thing history would have given them."* Restated at `vm.rs:4233-4235` and
`viewers/types.ts:276-282`, and mutation-pinned by
`text-file-viewer.test.tsx:467`.

The decision is right and is not being deleted. It is being **narrowed**: the
standing fact stays on screen before the first keystroke, as **one line composed
in Rust** beside `unmanaged_caveat` — never paraphrased in the webview, which
`types.ts:276-277` forbids — expanding to the full four lines on request. What
the owner gets back is 77px; what he keeps is the sentence that says the file has
no history.

## Item 3 is a repair, not a default change

Editing `DEFAULT_SESSION_SPACES` would be a **no-op** for him and for every
existing zone: `spaces::read_one` builds a space entirely from its own file
(`spaces.rs:305`) and consults the defaults only to validate the `default:`
marker (`:344`); `plan` never re-seeds a zone that has a `_spaces/` directory
(`:452-469`), and *Restore default spaces* skips anything claimed (`:472-487`).
AD-121's "the directory is the ledger" is exactly why.

So no silent rewrite. The About row already renders a disabled create carrying
Rust's `ManyTerms` sentence — *"this space asks for more than one thing"* — and
that sentence gains a **repair the owner presses**: narrow this space to the one
term its default asks for. Visible, one press, his hand on it, and it works for
any over-specified space, not just his.

## Item 5 gets a default, and a way to reach a zone that already exists

Two facts make it safe, both measured: a flat session's pool scan **does** descend
subdirectories (`read_ref_sources` → `markdown_rels(dir, true)`,
`sessions_root.rs:1272`), so a file written to `tasks/` is still read back and
still matched by `tag:task`; and `shape()` keys on `AGENTS.md` alone
(`shape.rs:98-101`), so new subdirectories cannot flip a flat session's contract.

The default destinations are therefore per-kind and Flat-only. And because his
`_spaces/*.md` carry **no** `keeper.create_dir` key at all, absent must mean *ask
the default* rather than *the session root* — that distinction is the whole of
what reaches an existing zone without rewriting his files.

## Stack order

    53.1  a card you can drag with a finger        (pointer gesture, dead drop zone, DW-37)
    53.2  a chooser that closes when you are done  (tag-combobox + five surfaces)
    53.3  one title bar, and two folds             (properties, the caveat's short form, the merge)
    53.4  a space you can narrow in one press      (the ManyTerms repair)
    53.5  a create that lands where the space says  (per-kind defaults, absent-vs-empty, the template)

53.4 and 53.5 both touch `spaces.rs`, so they are ordered. Everything else is
disjoint and the branches stay linear only for review's sake.
