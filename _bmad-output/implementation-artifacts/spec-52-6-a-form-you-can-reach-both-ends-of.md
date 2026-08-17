# Spec 52.6 — A form you can reach both ends of

story: 52.6
status: in-progress
branch: `work/epic-52-dialog-scrolls` (on top of `work/epic-52-space-create-dir`)
baseline_revision: c873fa6
final_revision: ''
binds: FR-310; WCAG 1.4.10 (reflow)
sentinel: `MUT52-6`

<intent-contract>

**The ask, verbatim.** *"jak edytuje spaces nie moge scrollowac zeby widziec
gore/dol formularza"* — with a screenshot showing `Name` and `Icon` clipped above
the window edge and the tag list running off the bottom, no scrollbar anywhere.

**The mechanism, measured.** `ui/dialog.tsx:53` constrains WIDTH only — `w-full`,
`max-w-*`, `sm:max-w-md` — and centres with `fixed top-1/2 -translate-y-1/2`.
There is no `max-h` and no `overflow`. The space editor's content is ~1220px:
Name 62 + icon fieldset 288 (its grid sitting at its own `max-h-56` cap) +
Sort/Direction/Position 62 + Opens/Rows 62 + two help paragraphs 104 + Terms and
the tag combobox 318, plus gaps 96, header 96, footer 36 and padding 48. On a
1000px window that puts the panel's top at −110px, and a transform creates no
scroll container — so the top is not merely cut off, it is unreachable.

**Always**
- The whole form is reachable at a 900px-tall window. The panel is height-capped
  and clips; the body scrolls between a pinned header and footer.
- The idiom is the one this repo already established twice —
  `settings-dialog.tsx:110` `flex max-h-[85vh] flex-col overflow-hidden` plus
  `:118` `min-h-0 flex-1 overflow-y-auto` — copied, not reinvented.
- `min-h-0` is present on the scrolling child. Without it a flex child's
  `min-height:auto` grows past the cap and bleeds out instead of scrolling; the
  settings dialog's own comment records this.
- The twin at `notes/space-editor.tsx` gets the identical change. Its header says
  the two are deliberate twins, so fixing one is the drift that comment warns of.

**Block if**
- Nothing. There is no state in which the form should be unreachable.

**Never**
- Never use `grid-rows-[…minmax(0,1fr)]`: Tailwind's arbitrary-value parser drops
  the comma inside `minmax()` and emits no CSS at all — a fix that silently is not
  one.
- Never change `ui/dialog.tsx`. Both existing tall dialogs solved it at the
  caller, and the primitive's short-dialog callers must not start scrolling.

</intent-contract>

## Code Map

| where | change |
|---|---|
| `src/components/sessions/session-space-editor.tsx:430,439` | `flex max-h-[85vh] flex-col gap-4 overflow-hidden sm:max-w-lg`; body `-mr-2 flex min-h-0 min-w-0 flex-1 flex-col gap-4 overflow-y-auto pr-2` |
| `src/components/notes/space-editor.tsx:448` and its body wrapper | the identical pair |

## Tasks & Acceptance

| # | acceptance |
|---|---|
| 1 | both dialogs' content carries the height cap and `overflow-hidden`, asserted by className — the repo's own pattern for CSS jsdom cannot measure (`composer.test.tsx:992`) |
| 2 | both bodies carry `min-h-0` and `overflow-y-auto`, asserted the same way, with the test naming why `min-h-0` is load-bearing |
| 3 | in a real browser at 900×900, the panel's `getBoundingClientRect().top >= 0` and the body's `scrollHeight > clientHeight` |
| 4 | every existing field still round-trips: the suites for both editors stay green untouched |

## Design Notes

_(filled at review)_

## Verification

_(filled at review)_
