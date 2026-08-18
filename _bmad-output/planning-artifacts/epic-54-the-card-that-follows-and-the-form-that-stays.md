# Epic 54 — The card that follows, and the form that stays

created: '2026-08-17'
source: the owner's eighth report — two regressions against the merged epic-53 build on hesperia (main 9a95acf). Three read-only scouts measured both before this spine was written.
binds: FR-323…FR-327; AD-102 (untouched), UX-DR61 (untouched), spec-52-3's request (restored), spec-53-3 (two clauses overridden here, explicitly)

## What he said

> *drop dziala teraz — lubie przerywane linie zeby widiec gdzie drop zrobic, ale drag ma
> regresje: nie ma animacji ani animacji przesuwania oraz czesto inne czesci aplikacji
> sa "zaznaczane" przypadkowo*

> *note folding — dziala folding i unfolding tekstu ale mialo byc i z czescia properties
> ktorej teraz brakuje (za to w notes jest prefix z properisami ktorego nie chcialem w notes)*

The drop works. Good — that was three reports and a Tauri source read. What
replaced it gave back none of what `draggable="true"` was quietly doing besides
starting a drag.

## Verdicts

| # | claim | verdict | mechanism |
|---|---|---|---|
| 1a | no animation, nothing moves | **absent** | the entire drag inventory is `cursor-grab` (static), `data-[dragging]:opacity-50`, and the column's dashed border. No transform, no translate, no ghost, no placeholder, no transition anywhere in the board. The card **stays where it is** and goes half-transparent while the pointer walks away from it |
| 1b | other parts get selected | **broken** | `select-none` is on the pressed `<li>` only (`task-board.tsx:325`), nothing suppresses selection on the document, and **no `preventDefault()` is issued on the press**. A native drag never selected text; `draggable` was doing double duty and both halves were removed together |
| 1c | he likes the dashed lines | **keep** | `task-board.tsx:443-445` — they mark a **column**, not an insertion slot. `dropAt` computes a slot index and nothing renders it |
| 2a | the properties section is missing | **folded, with a near-invisible affordance** | `file-frame-fold.ts:61` defaults `properties: true`; the control is an unlabelled 32px `SlidersHorizontal` ghost icon sitting next to Save, and the region is **unmounted**, not hidden — so the grid vanished with no residue |
| 2b | a properties prefix in the note tab | **broken, by a recorded decision** | same root cause. Folded ⇒ `FileProperties` never mounts ⇒ `formBlock` stays null ⇒ `frontmatterInForm={null}` ⇒ the pane draws `---\ntitle: …\n---` as the first lines of his prose |

## 2a and 2b are one defect

`spec-53-3` chose to default the fold closed *"like the notes surface"*, and to hand
the block back to the panes when the form folds away — *"with no form on screen the
document has to draw the `---` block or a file's `tags:` would be on screen
nowhere at all."* Coherent in isolation, and it **overturned the owner's own 52.3
instruction** by changing that request's antecedent without re-checking it.

The symmetry it leaned on does not exist: on a **note** the frontmatter is a
separate store field (`notes-editor.ts:16-19`), so a closed panel hides nothing;
on a **file** the buffer IS the whole file, so a closed form dumps the block into
the reader's prose. `raw-rendered-view.tsx:199-201` already states that asymmetry
two files away from the code that assumed it away.

So this epic overrides two clauses of spec 53.3, and says why in both places:
- the file surface defaults properties **open** — he asked for an *option to fold*, and an option to fold presumes the thing is there;
- a folded form **still hides the block**. Folded is not absent: the fold control names it, and the spec's objection is answered by the control, not by putting YAML in his document.

## What the drag needs, from the repo's own idioms

Nothing here is invented. `chat-row.tsx:459-461` is the pointer-follow precedent —
`transform: translateX(${dx}px)`, **no** transition while the gesture is live so the
element tracks the finger 1:1, and a transition on release so it settles, gated on
`useReducedMotion()`. `resizable-columns.tsx:202-203` is the selection precedent:
it captures the pointer **and** calls `preventDefault()` on the press, which is
exactly why a seam drag leaks no selection.

Reduced motion cuts the **landing** transition, not the live follow: direct
manipulation is not animation. `chat-row.tsx:459` encodes that distinction already.

## The suite cannot see either regression

By construction, and worth stating because it is why both shipped: jsdom has no
layout and no compositor, so it can never observe a transform reaching the screen
or a selection painted by WebKit. The board's 19 green tests include the only
visual assertion in the file — a `className.includes("border-dashed")` substring
check. And two of the tests written *for* the 52.3 request were **disarmed in the
same commit that broke it**: both now `hydrateFileFrameFold({properties: false})`
in `beforeEach`, arranging the fold open, so they assert a state no fresh install
is ever in.

One test asserts the bug as correct — `text-file-frame.test.tsx:896`, *"folds the
form away and back, and the pane takes the block back with it"*. It must be
re-anchored, not deleted: fold→block-returns stays wrong, but the fold itself is
still right.

## Stack order

    54.1  a card that follows the finger      (transform, landing settle, selection suppression; the column cue kept)
    54.2  properties you can see, and never in your prose  (default open, a real disclosure, a folded form that still holds the block)

Disjoint files; linear only for review's sake.
