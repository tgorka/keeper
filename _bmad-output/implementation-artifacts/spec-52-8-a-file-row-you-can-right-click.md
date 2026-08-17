# Spec 52.8 — A file row you can right-click

story: 52.8
status: in-progress
branch: `work/epic-52-file-row-menu` (on top of `work/epic-52-drag-drops`)
baseline_revision: c873fa6
final_revision: ''
binds: FR-312; AD-65
sentinel: `MUT52-8`

<intent-contract>

**The ask, verbatim.** *"prawy klawisz myszy wciaz nie dziala w session files"*.

**Why story 51.6 did not deliver it.** That story was titled "a row you can
right-click" and scoped itself, in its own spec (`spec-51-6:14,82,108`), to a
SPACE row. There is exactly one `ContextMenu` mount in `src/components/sessions/`
— `space-row-menu.tsx:262`, consumed at `session-spaces.tsx:828`. The FILES tree
rows are bare `<div role="treeitem">` (`session-tree.tsx:328-336`) whose only
verbs are three hover-revealed icon buttons. The tree's accessible name is
literally "Session files". That is what he is right-clicking.

**Always**
- Every row in the FILES tree — file and folder alike — answers a right-click with
  the SAME menu a space row gives, mounted from the existing component. No second
  menu, no duplicated verb list.
- The menu respects what Rust already said about each path: a locked or
  undeletable entry shows Delete disabled WITH the reason, never a Delete that
  fails afterwards.
- Rename is offered only where it means something — a markdown file, not a
  directory and not a non-markdown file — because renaming writes a frontmatter
  `title:`.
- The keyboard route (Shift+F10 / Menu key) and the long-press route come with it,
  since both already live in the component.
- Refusals land in the tree's existing live region, where its other refusals land.

**Block if**
- The row is the session record: Rename is refused by `files::renames`, and the
  menu shows that refusal rather than hiding the item.

**Never**
- Never write a second context menu for these rows.
- Never let the menu offer a verb the row cannot perform.

</intent-contract>

## Code Map

| where | change |
|---|---|
| `src/components/sessions/space-row-menu.tsx:127-155,271+` | two optional props: `deleteRefusal?: string \| null`, `renamable?: boolean`; Delete renders disabled-with-reason, Rename is omitted when not renamable |
| `src/components/sessions/session-tree.tsx:311-336` | the treeitem is wrapped in `SpaceRowMenu`, passing the entry's own refusals (`locked`, `undeletable`) and `renamable` |
| `src/components/sessions/session-tree.tsx` | `useLongPress` spread on the row, for phone parity with `files-pane.tsx:1763` |

## Tasks & Acceptance

| # | acceptance |
|---|---|
| 1 | right-clicking a file row in the FILES tree opens the menu with its seven verbs |
| 2 | right-clicking a FOLDER row opens the menu with Rename absent and Delete governed by the folder's own refusal |
| 3 | a locked entry's Delete is disabled and its accessible description is Rust's sentence |
| 4 | the record row offers no working Rename, and says why |
| 5 | Shift+F10 opens the menu on a focused row |
| 6 | Open-in-this-panel from a tree row opens the file the row names |
| 7 | the tree's 20 existing tests stay green, and the new ones assert the ROWS have the menu — not that the menu component works |

## Design Notes

_(filled at review)_

## Verification

_(filled at review)_
