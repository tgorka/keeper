# Spec 53.2 — A chooser that closes when you are done

story: 53.2
status: in-progress
branch: `work/epic-53-tag-chooser-folds` (on top of `work/epic-53-pointer-drag`)
baseline_revision: 8c8a3eb
final_revision: ''
binds: FR-315; UX-DR61 (Story 44.13), narrowed not reversed
sentinel: `MUT53-2`

<intent-contract>

**The ask, verbatim.** *"can you have option to fold back the list of tags when i
stop choosing (in all views)"*

**What is recorded, and what is not.** `tag-combobox.tsx:10-13` — *"this control
renders the list permanently under the field and narrows it as you type. There is
no popup and no expanded/collapsed state, because a list you have to open is a
list nobody browses."* Story 44.13's acceptance says the list stays **browsable**.
That decision is kept: this is not a click-to-open popup. What is absent is any
notion of *done choosing* — the input has no `onBlur` (`:172-201`), there is no
`focus-within`, no outside-click layer, and `choose()` deliberately keeps the list
open (`:127-134`). On the two space editors the chooser is mounted
unconditionally with no `onDismiss` at all (`space-editor.tsx:790`,
`session-space-editor.tsx:794`), so there is **no close path in the product**.

**Always**
- The chooser closes when the user has stopped choosing: focus leaves the combobox
  entirely (`focus-within` with a `relatedTarget` guard), or an outside click, or
  Escape — which already works where a caller passes `onDismiss`.
- It opens on focus and on typing, so it is still browsable without a deliberate
  press. Arrow keys on a closed chooser open it and move the highlight, so the
  keyboard flows the existing tests drive keep working.
- Closing HIDES rather than unmounts, the `FoldSection` property
  (`sidebar-group.tsx:215`, `hidden={folded}`): a hidden body stops claiming
  height, which is the whole of what the owner is asking for.
- Every surface that mounts the chooser gets it, from one change to the one
  component: the note properties panel, the file properties panel (the same
  `TagsProperty`), the notes filter bar, and BOTH space editors — which also gain
  the dismiss they never had.
- A click on an option must still commit: options carry
  `onMouseDown={preventDefault}` (`:238`) precisely so a click cannot blur the
  input, so the close signal must not fire between mousedown and click.

**Block if**
- The user is mid-choice: a query typed, a highlighted row, or focus inside the
  chooser. None of those is *stopped choosing*.

**Never**
- Never convert this into a popover or a cmdk palette. Both exist in the repo and
  both are the shape 44.13 refused.
- Never close on `choose()`: `tag-combobox.tsx:29-32` records that tagging is a
  thing people do several times in a row, and the owner's ask is about *stopping*,
  not about each pick.

</intent-contract>

## Code Map

| where | change |
|---|---|
| `src/components/notes/tag-combobox.tsx:170-246` | an `open` state driven by focus/typing/Escape/outside-click, with the list hidden rather than unmounted; `aria-expanded` becomes real instead of the literal at `:182` |
| `src/components/notes/properties-panel.tsx:1377-1396`, `note-filter-bar.tsx:334-344` | keep their `adding` toggle; the new close is additive |
| `src/components/notes/space-editor.tsx:790`, `src/components/sessions/session-space-editor.tsx:794` | gain the dismiss path they never had |

## Tasks & Acceptance

| # | acceptance |
|---|---|
| 1 | after choosing a tag and moving focus away, the list is not on screen and claims no height |
| 2 | focus returning to the input brings it back, with the vault's own order |
| 3 | typing opens it and narrows it, as today |
| 4 | a click on an option still commits the tag — the mousedown-preventDefault interaction |
| 5 | Escape on an empty query still dismisses where a host passes `onDismiss`, and now also closes the list on the two space editors |
| 6 | an outside click closes it; a click inside does not |
| 7 | the existing keyboard flows still work: ArrowDown/ArrowUp/Enter reach the list from the input |
| 8 | `aria-expanded` reports the real state, and the test that pinned it permanently `true` is re-anchored with a comment naming this story |
| 9 | all five surfaces are covered, and the ones out of scope (the `<datalist>` recording field, the editor's `#` completion, the rail's tag tree, the chosen-chip rows) are named in the spec as already folding or not being choosers |

## Design Notes

_(filled at review)_

## Verification

_(filled at review)_
