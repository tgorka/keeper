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
| `src/components/notes/tag-combobox.tsx:169-215` | the outside-press layer answers to `click`, `auxclick` AND `contextmenu`: `click` is the primary button's event only, so a right or middle press outside reached neither close path and left the list up. `pointerup` is not the signal — it arrives before the browser has settled what a press hit, which is what the mid-press guard exists for |
| `src/components/notes/tag-combobox.tsx:217-251` | Escape's claim, made on the WINDOW in capture while the list is up and only for a key aimed inside the control. The field's handler already prevents this key's default; the window layer makes that claim before `@radix-ui/react-dismissable-layer` reads it at the document, which is how row 5 is delivered on the two dialogs |
| `src/components/notes/tag-combobox.tsx:82-139` | `openOnMount` replaces `inputRef`: the host that reveals the chooser on a press of its own says so in one word, and the control takes the caret and starts unfolded. The browse half used to ride on `inputRef={(node) => node?.focus()}` at three callsites — a focus side effect nothing declared and no host test could see |
| `src/components/notes/properties-panel.tsx:1377-1396`, `note-filter-bar.tsx:334-344` | keep their `adding` toggle; the new close is additive, and each now passes `openOnMount` instead of a focusing ref callback |
| `src/components/notes/space-editor.tsx:790`, `src/components/sessions/session-space-editor.tsx:794` | gain the dismiss path they never had, Escape included, with no prop and no mirrored state of their own |

## Tasks & Acceptance

| # | acceptance |
|---|---|
| 1 | after choosing a tag and moving focus away, the list is not on screen and claims no height |
| 2 | focus returning to the input brings it back, with the vault's own order |
| 3 | typing opens it and narrows it, as today |
| 4 | a click on an option still commits the tag — the mousedown-preventDefault interaction |
| 5 | Escape on an empty query still dismisses where a host passes `onDismiss`, and folds the list on all five surfaces — including the two space editors, where the first press folds and the dialog with its unsaved draft stays on screen, and the second press closes the form. Delivered by the chooser claiming the key ahead of the dialog's dismissable layer rather than by a prop each dialog has to remember |
| 6 | a press outside closes it and a press inside does not — and "press" is any button: `click`, `auxclick` and `contextmenu`, because the primary button's event is not the only way to press |
| 7 | the existing keyboard flows still work: ArrowDown/ArrowUp/Enter reach the list from the input |
| 8 | `aria-expanded` reports the real state, and the test that pinned it permanently `true` is re-anchored with a comment naming this story |
| 9 | all five surfaces are covered, and the ones out of scope (the `<datalist>` recording field, the editor's `#` completion, the rail's tag tree, the chosen-chip rows) are named in the spec as already folding or not being choosers |

## Design Notes

**Escape on the two dialogs: claimed, not delegated (review fix).** The
alternative was an `onOpenChange` on `TagCombobox` plus a `listOpen` boolean and
an `onEscapeKeyDown={(e) => { if (listOpen) e.preventDefault(); }}` in every
dialog that ever mounts the chooser. Both routes depend on exactly the same
Radix contract — `dismissable-layer/dist/index.mjs:88-91` calls
`onEscapeKeyDown` and then dismisses only `if (!event.defaultPrevented)` — so the
prop buys no robustness, only per-host repetition of a fact the chooser already
knows. The window layer is the same idiom as the outside-press layer six lines
above it, subscribed only while the list is up, and it is scoped to a key aimed
INSIDE the control so Escape anywhere else on a dialog still cancels the dialog.
The alternative to delivering row 5 at all was narrowing it, and that was refused
because the narrowing is destructive: three of five surfaces teach "Escape folds
this list", and on the two that did not, Escape discarded an unsaved space draft
with no confirmation (`space-editor.tsx:442-446`).

**`openOnMount` rather than a focusing ref (review fix).** The list is gated on
`open`, and on the two properties panels the ONLY thing that opened it was a
`useCallback` whose whole body was `node?.focus()`, handed over as `inputRef`.
Nothing bound the two, and every option assertion in those suites typed first —
so deleting that ref left every test green while a user pressing "Add tag" got a
bare field with no list until they typed. The prop states the request the host is
actually making, and the control's initial `open` no longer waits for a focus
event to arrive.

**A test is named for what it renders.** `tag-combobox.test.tsx`'s Escape case
was titled for "the space editors' whole close" while rendering a bare
`TagCombobox` with neither editor mounted. It is now named for the control it
renders, and the claim about those two surfaces is asserted on those two
surfaces.

## Verification

`npx tsc --noEmit`: clean. `npx biome check` on the ten touched files: clean.
`npx vitest run` over `tag-combobox.test.tsx` (35), `properties-panel.test.tsx`
(47), `file-properties.test.tsx` (22), `note-filter-bar.test.tsx` (18),
`space-editor.test.tsx` (51) and `session-space-editor.test.tsx` (46): 219 pass,
none skipped. The mid-press commit guard was re-run on its own: `holds the fold
through a press on an outside control, so that press still lands` still passes,
which is the assertion that the three new press names did not fold the list
between a pointer going down and the click landing.

Each fix was falsified by mutation:

| mutation | fails |
|---|---|
| the `window` keydown claim deleted | `claims Escape before a dismissable layer above it could read the key`, and `folds the list on Escape and keeps the draft…` on BOTH space editors |
| `auxclick` + `contextmenu` deleted from the press layer | `folds on a right press outside it…`, `folds on a middle press outside it…` |
| `openOnMount` deleted from the properties panel callsite | `shows the vault's tags on the press that asks for them, with no key pressed` on both the note and the file surface, and nothing else |
| the caret effect deleted | the two above, plus `takes the caret and starts unfolded when the host mounted it on a press` |

`useState(openOnMount)` on its own is not separately falsifiable — with the caret
effect in place the focus event opens the list anyway — and it is kept because
the point of the prop is that the fold state does not depend on that side effect.
