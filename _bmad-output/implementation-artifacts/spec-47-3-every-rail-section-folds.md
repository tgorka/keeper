# Spec 47.3 — Every rail section folds

<intent-contract>

**The ask.** "Fold panels other than the first."

**The reading that shipped, and the reading that was meant.** Story 46.13 read this as the Files
panel strip and shipped a per-panel fold, recording the alternate reading in its own spec
(`spec-46-13-…md`, Report B). The owner has since confirmed the alternate reading is the live one,
and it is about a different surface entirely: **the notes rail**, the 240px column in
`NotesPane` holding SPACES, TAGS and FILES.

**What was actually there.** Exactly one thing in that whole column folded, and it was the wrong
one for two reasons.

| surface | section | before this story |
| --- | --- | --- |
| chat sidebar (`app-shell.tsx`) | Spaces (Matrix spaces) | folds, persisted — `SIDEBAR_GROUPS`, Story 45.20 |
| chat sidebar | Networks | folds, persisted — Story 45.20 |
| Files panel strip (`panel-strip.tsx`) | each panel | folds, persisted — Story 46.13 |
| **notes rail** (`space-list.tsx:167`) | **Spaces (saved queries)** | **plain `<section aria-label="Spaces">`, no control** |
| **notes rail** (`tag-tree.tsx:208`) | **Tags** | **plain `<section aria-label="Tags">`, no control** |
| **notes rail** (`physical-tree.tsx:177`) | **Files** | folded via a local `useState(false)` — **forgotten on every surface switch** |

`SIDEBAR_GROUPS = ["spaces", "networks"]` governs `spaces-group.tsx` and `networks-group.tsx`. Its
`spaces` is the Matrix space list. The notes rail's Spaces is a list of saved queries. They share a
word and nothing else, and that collision is the first design constraint below.

**Delivered.** All three notes-rail sections fold, through one shared control, persisted in the
rail's own cookie, restored at the pane. The Files tree's `useState` is gone.

</intent-contract>

## The three things that had to be got right

### 1. `SIDEBAR_GROUPS` is a closed union used as a storage key — so the notes rail got its own

Two honest shapes existed. The cheap one — widen `SIDEBAR_GROUPS`, one cookie, one namespace — was
**rejected**, and not on taste:

- `SIDEBAR_GROUPS` **already contains `spaces`**. Widening the union to serve the notes rail's
  Spaces section would have made one persisted bit stand for two unrelated sections on two
  unrelated surfaces. Folding a saved-query list would have folded a Matrix space list.
- That defect is invisible to every existing test, because no test renders both surfaces.
- The surfaces are genuinely independent: a person may reasonably want Spaces shut in the chat
  sidebar and open in the notes rail.

So: `NOTES_RAIL_GROUPS = ["spaces", "tags", "files"]` in `keeper_notes_rail_fold`, a second cookie.
The cost is one extra `hydrate…` call site, paid in `NotesPane` (see §3 of the edge cases).

**The old-cookie question was verified, not assumed.** `readSidebarFold` drops unknown keys — that
is now pinned by an explicit literal rather than inherited from the composer:

- `sidebar-fold.test.ts` → *"still reads, and still writes, the exact cookie the pre-47.3 build left
  behind"* asserts both directions against the hand-written byte string
  `keeper_sidebar_fold=menu%3A1%7Cspaces%3A0%7Cnetworks%3A1`.
- `notes-rail-fold.test.ts` → *"the two surfaces do not read each other"* asserts a jar holding only
  the chat cookie leaves the rail at its defaults, and a jar holding only the rail cookie leaves the
  chat sidebar at its defaults. **These are Main's requested test for the branch not taken:** merge
  the two key sets later and both go red.

### 2. `FoldableGroup` already existed — so it was split, not copied

The notes sections could not use it as-is: their bodies are trees rather than `<li>` lists, one owns
the column's flexible height, one carries a second control in its header, and all three read a
different store. Per the brief, it was **changed once**:

- `FoldSection` (`sidebar-group.tsx`) is the whole mechanism and knows **no store** — it takes
  `folded` + `onToggle`. It keeps every property the original had: the disclosure in the tab order,
  `aria-label` carrying the verb (`Expand X` / `Collapse X`, WCAG 2.5.3), `aria-expanded`,
  `aria-controls` pointing at a region that exists, `hidden` rather than unmount, and the rule that
  a fold hides the rows and never the header.
- `FoldableGroup` is now a binding of `FoldSection` to `sidebarFoldStore`. Its rendered DOM is
  unchanged in every respect the chat sidebar's tests name; `spaces-group.test.tsx` and
  `networks-group.test.tsx` pass untouched.
- New props, each earning its place: `as` (`ul` vs `div`), `className`/`bodyClassName` (§3),
  `actions` (a header control OUTSIDE the folded region) and `notice` (what `actions` had to say,
  also outside).

The cookie **encoding** was extracted the same way and for the same reason, into
`src/lib/stores/fold-cookie.ts`: `readFoldFlags` / `foldFlagsCookie` / `persistFold`. Two copies of a
wire format agree until the day one is edited, and a build that writes a cookie an older build
silently drops is invisible to a test that exercises one surface.

### 3. The tag tree is `flex-1` — a folded Tags gives the column its height back

`hidden` sets `display: none`, which takes the **body** out of the flex column for free. It does
**not** take the `<section>` out. A section left at `min-h-0 flex-1` around a hidden body is an empty
stripe where the tree used to be — the fold would read as a rendering bug.

So `TagTree` passes `className={folded ? "shrink-0" : "min-h-0 flex-1"}`, and
`rail-fold.test.tsx` → *"gives the column its height back while folded and takes it again when
opened"* asserts both transitions. Spaces (`shrink-0`) and Files (`min-h-0 shrink-0`) never claimed
the spare height, so they need no switch.

## The `space-list.tsx:162-165` objection, checked against the markup

The recorded reason not to fold Spaces:

> The section renders even when the vault has none. Spaces are the rail now (Story 44.3): folding it
> away when it is empty would hide the one control that fills it, and a vault whose owner deleted
> every default would have no way back.

**The reading in the brief holds, and the markup confirms it.** The comment is about the section
being ABSENT (`return null`), not folded. The control it protects is `RESTORE_DEFAULTS` ("Restore
default spaces"), which in the original markup lived in the header row at `space-list.tsx:172-185` —
a **sibling** of the label, not a child of the `<ul>` at `:192`. A fold that hides only the `<ul>`
therefore cannot hide it.

The comment was **corrected in place, not deleted**: it now states the original reason and then why
folding is compatible with it. And the objection was extended one step the original did not take:

> a control that is reachable while folded and whose report is not is a control that does nothing
> when you press it.

`restoreResult` (`<p role="status">`) was also in the folded region's path, so `FoldSection` grew
`notice` and both live in the header. `rail-fold.test.tsx` → *"still restores the defaults while
folded, and still says what happened"* presses the button with Spaces shut and asserts both the IPC
call and the sentence. Mutation M4′/M10′ below prove the placement, not merely the presence.

## I/O matrix

### `readFoldFlags(cookie, name, keys, defaults)` — `fold-cookie.ts`

| in | out |
| --- | --- |
| jar without `name` | `defaults` unchanged |
| `name=a%3A1%7Cb%3A0` | `{a: true, b: false, …defaults}` |
| entry with no `:` | dropped, that key stays at its default |
| value not `0`/`1` (`tags:yes`) | dropped |
| key not in `keys` (`recordings:1`) | dropped |
| `name` present twice in the jar | last wins (loop is not early-exit) |

### `readNotesRailFold` / `notesRailFoldCookie`

| in | out |
| --- | --- |
| `""` / no rail cookie | `{spaces: false, tags: false, files: true}` |
| `keeper_notes_rail_fold=spaces%3A1%7Ctags%3A0%7Cfiles%3A0` | `{spaces: true, tags: false, files: false}` |
| jar containing only `keeper_sidebar_fold=…spaces:1…` | defaults — **no cross-read** |
| any of the 8 fold states | round-trips through its own writer |
| `{spaces: true, tags: false, files: true}` | `keeper_notes_rail_fold=spaces%3A1%7Ctags%3A0%7Cfiles%3A1; path=/; max-age=31536000` |

### `readSidebarFold` — unchanged contract, re-verified

| in | out |
| --- | --- |
| `keeper_sidebar_fold=menu%3A1%7Cspaces%3A0%7Cnetworks%3A1` (pre-47.3 bytes) | `{menu: true, groups: {spaces: false, networks: true}}` |
| that same value, written by `sidebarFoldCookie` | byte-identical to the pre-47.3 writer's output |
| jar containing only `keeper_notes_rail_fold=…` | all-open defaults — **no cross-read** |

### `FoldSection`

| in | rendered |
| --- | --- |
| `folded: false` | header + `<Body id hidden={false}>`; disclosure named `Collapse {label}`, `aria-expanded="true"` |
| `folded: true` | header + `<Body hidden>`; disclosure named `Expand {label}`, `aria-expanded="false"`; body out of the a11y tree and the tab order |
| `actions` | rendered in the header row, sibling of the disclosure (never nested — a button in a button is invalid HTML) |
| `notice` | rendered under the header, above the body, outside the folded region |
| `collapsed: true` | icon-only disclosure, `px-1`, no visible label (chat sidebar only) |
| `as="ul"` | body is `<ul>` — used by `FoldableGroup` for `<li>` rows |

### The three sections

| section | `id` | body | section class open → folded | default |
| --- | --- | --- | --- | --- |
| Spaces | `notes-rail-spaces` | `<ul>` of space rows | `shrink-0` (no change) | open |
| Tags | `notes-rail-tags` | `role="tree"` inside a flex body | `min-h-0 flex-1` → `shrink-0` | open |
| Files | `notes-rail-files` | `role="tree"`, `max-h-48 overflow-y-auto` | `min-h-0 shrink-0` (no change) | **folded** |

**Files defaults folded, and that is load-bearing.** The tree issues one `notes_tree` per expanded
directory and has arrived collapsed since Story 37.9. An all-open default would turn every mount of
the notes surface into a cold read of the vault root. That is why `readFoldFlags` takes `defaults`
rather than assuming "everything starts open", and why the lazy-load effect gates on `folded` rather
than on the old local `open`.

## Edge cases

1. **A key collision between the two surfaces** — structurally impossible, two cookie names, two
   closed key sets, asserted in both directions.
2. **A cookie written by the pre-47.3 build** — still read, and still written byte-identically.
3. **Hydration site.** The chat sidebar hydrates in `AppShell` because its drawer is unmounted on the
   phone tier. The rail hydrates in `NotesPane` instead: these three sections render nowhere else,
   and the pane is unmounted whenever another primary view is showing. Idempotent, so React's
   double-invoked development effect and every later remount restore exactly once and cannot
   overwrite a fold the user has changed since.
4. **DW-172's shape.** A store-level test calls the hydrate itself and passes over a pane that never
   does. `notes-pane.test.tsx` → *"comes up folded the way the cookie left it"* arranges a real
   cookie, mounts the real pane, and asserts Files came up **unfolded against its own default** —
   which cannot pass on a pane that ignored the cookie and fell back to defaults.
5. **A folded Spaces with an empty vault** — header, disclosure and Restore all reachable; §"the
   objection" above.
6. **A jar that refuses writes** — `persistFold` swallows; the fold still applies for the session.
   Unchanged from Story 45.20.
7. **`TagTree` with no tags / `PhysicalTree` with no vault** still `return null`, so those sections
   are ABSENT rather than folded. Pre-existing and unchanged — see "Deliberately NOT done".
8. **A dialog open while Spaces is folded.** `SpaceEditor` and `NoteDeleteDialog` were moved outside
   `FoldSection` (the component now returns a fragment), so a fold cannot hide an open confirmation.
9. **Two toggles in flight** — `toggleGroup` reads `get()` and writes the whole key set, so the
   cookie always carries all three sections at their real values, never a partial write.

## Mutation table

Every fix proved by removing it and watching a **named** test fail. Sentinel `MUT47-3`; twelve
mutations, twelve kills, zero survivors. Restore verified by comparing every touched file on disk to
its pre-sweep snapshot (all equal), by `git diff -- src/` (no sentinel line), and by a repo-wide
`grep MUT47-3` (no matches — the three files I CREATED are invisible to `git diff`, so the grep is
the check that covers them).

| # | mutation | named test that went red |
| --- | --- | --- |
| M1 | drop `hydrateNotesRailFold(document.cookie)` in `NotesPane` | `NotesPane rail fold › comes up folded the way the cookie left it` |
| M2 | `toggleGroup` stops calling `persistFold` | `the notes rail remembers its fold across a remount › keeps {Spaces,Tags,Files} folded` |
| M3 | drop `hidden={folded}` from `FoldSection`'s body | `… › hides the {spaces,tree} and keeps the header, so there is a way back` |
| M4 | delete the header `actions` slot | `… › still restores the defaults while folded, and still says what happened` |
| M4′ | **move `actions` INSIDE the folded region** (placement, not deletion) | same |
| M5 | Tags keeps `min-h-0 flex-1` while folded | `… › gives the column its height back while folded and takes it again when opened` |
| M6 | `notesRailUnfolded()` returns `files: false` | `… › arrives shut and reads no directory until it is opened` |
| M7 | `NOTES_RAIL_FOLD_COOKIE` set to `keeper_sidebar_fold` | `the two surfaces do not read each other` (all three) |
| M8 | `readFoldFlags` stops dropping unknown keys | `readNotesRailFold › drops a key this build does not know and keeps the ones it does` |
| M9 | the Files lazy-load effect stops gating on `folded` | `… › arrives shut and reads no directory until it is opened` |
| M10 | delete the `notice` slot | `… › still restores the defaults while folded, and still says what happened` |
| M10′ | **move `notice` INSIDE the folded region** (placement, not deletion) | same |

M4/M10 delete; M4′/M10′ relocate into the body. Both pairs were run because a deletion mutation only
proves the slot is *needed*, not that it is *outside the fold* — which is the actual contract.

**The remount test is the cookie's, not the store's.** It folds, unmounts, **wipes the store**
(asserting the wipe took), hydrates from the string the fold wrote, remounts, and asserts the
section came back the way it was left. An in-process remount would have passed on a memory-only
store — which is precisely what the Files fold was before this story.

## Acceptance

`bun run test src/components/notes/ src/components/layout/ src/lib/stores/` — **EXIT=0, three
consecutive runs**, 104 files / 1788 tests each. `bun run tsc --noEmit` clean.

| requirement | where |
| --- | --- |
| a test per section: it folds | `rail-fold.test.tsx` × 3 (`hides … keeps the header`) |
| … stays folded across a remount | `rail-fold.test.tsx` `it.each` over all three, through the real cookie |
| … header controls reachable while folded | the disclosure in all three; Restore + its report for Spaces |
| an old cookie still loads | `sidebar-fold.test.ts` (literal pre-47.3 bytes) + `notes-rail-fold.test.ts` (no cross-read) |
| mutation-prove each fold and the persistence | table above |

## Deliberately NOT done

- **`TagTree` and `PhysicalTree` still `return null` when they have nothing to show.** So an empty
  Tags section is absent rather than folded, which is a different rule from Spaces' (Story 44.3
  requires Spaces to render empty because it holds the control that fills it). Pre-existing, not
  introduced, and changing it is a UX decision about whether an empty section should occupy a row.
  **Proposed as a new DW entry** — see the report.
- **No fold for the notes rail as a whole.** The chat sidebar folds to a 48px icon rail
  (`collapsed`); the notes rail has no such mode, and `FoldSection` supports `collapsed` only for
  its existing caller. Adding one is a layout story, not this one.
- **No keyboard shortcut, no "fold all".** Not asked for.
- **`spec-46-13-…md` was not amended.** Its Report B records the alternate reading; it now needs a
  supersession note saying the alternate reading was the live one and is closed by this story. That
  file is not in my ownership — flagged to Main.
- **The Files panel strip's per-panel fold (46.13) is untouched.** It is a real feature and a
  correct one; it simply was not the report.
- **The formatter and linter were not run** (Main runs them once at the end, per the wave rule). Two
  lines in `rail-fold.test.tsx` are likely to be re-wrapped by biome.

## What I could not verify here, and why

1. **Nothing Rust, nothing packaged.** This story is entirely frontend; the `keeper` shell crate does
   not build on Linux and was never touched. No `cargo` command was run and none is needed.
2. **Layout is asserted by class, not by geometry.** jsdom has no layout engine, so
   "a folded Tags gives its space to the list rather than leaving a gap" is proved as
   *the section stops carrying `flex-1` and starts carrying `shrink-0`* — the necessary and
   sufficient condition in this flex column — and not as measured pixels. The visual result needs a
   human on the real app.
3. **`hidden` semantics rely on jsdom's default stylesheet.** Testing Library's role queries exclude
   the folded body, which is the same mechanism Story 45.20's chat-sidebar tests already depend on.
   Real-browser behaviour is stronger, not weaker, but it is inherited rather than re-measured.
4. **Cookie persistence across a real app restart** is exercised as string → parse → store, not as
   WebKit actually retaining `keeper_notes_rail_fold` for a year.

### Ordered gate checks

Run in this order; each assumes the previous passed.

1. `bun run tsc --noEmit -p tsconfig.json` → clean. *(ran, clean)*
2. `bun run test src/components/notes/ src/components/layout/ src/lib/stores/` → EXIT=0.
   *(ran, three consecutive times)*
3. `bun run test` (full frontend suite) → EXIT=0. **Not run here** — the wave rule reserves it for
   Main. Nothing outside the three directories imports `fold-cookie.ts` or `notes-rail-fold.ts`;
   `sidebar-group.tsx`'s two existing callers both live in `src/components/layout/`.
4. `bun run lint` / formatter → Main's, once.
5. `cargo check --manifest-path src-tauri/Cargo.toml -p keeper` on **hesperia** — required by the
   repo's gate, not by this change: no Rust file was touched.
6. **On the real app (hesperia), the smoke test this story is actually about:**
   1. Open Notes. All three of SPACES, TAGS and FILES carry a chevron control.
   2. Fold SPACES. The rows go; the header, the chevron and the ↺ Restore button stay. Press ↺ while
      folded and read the sentence it prints — that sentence must be visible with the section shut.
   3. Fold TAGS. **No empty stripe** where the tree was; the sections below move up. Unfold — the
      tree takes the spare height back and scrolls.
   4. Open FILES, expand a folder, switch to Chats and back. FILES is still open **and still
      expanded** — this is the half that was a `useState` and forgot.
   5. Quit and relaunch. All three folds are as they were left.
   6. Open the chat sidebar. Fold its SPACES group. Return to Notes: the notes rail's Spaces is
      **unaffected**. Fold the notes rail's Spaces, return to Chats: the chat sidebar's Spaces is
      unaffected. This is the collision the two-cookie decision exists to prevent, and it is the one
      thing on this list a unit test can only prove structurally.
