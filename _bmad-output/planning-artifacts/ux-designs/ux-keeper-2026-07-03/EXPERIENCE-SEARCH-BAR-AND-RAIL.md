---
name: keeper
parent: EXPERIENCE-NOTES-SEARCH.md
status: final
created: 2026-09-20
updated: 2026-09-20
binds: Epics 77 and 78; AD-269…AD-280, AD-283, AD-284; UX-DR100…UX-DR105
sources:
  - DESIGN.md
  - DESIGN-NOTES.md
  - EXPERIENCE-NOTES-SEARCH.md
  - local://coordinator-decisions.md
  - local://research-ux-2026-09-20.md
  - local://triage-ScoutSpaces.md
  - local://triage-ScoutSearchBar.md
  - local://triage-ScoutNoteList.md
  - local://triage-ScoutTheme.md
  - local://triage-ScoutEditorNav.md
  - local://triage-ScoutCopyEngine.md
  - local://triage-ScoutTasksUi.md
---

# keeper — Experience Spine, Search bar, spaces rail and run detail

> Extends `EXPERIENCE-NOTES-SEARCH.md`, continuing UX-DR94–99 at UX-DR100. Everything not explicitly replaced keeps its existing contract. This is the visual/interaction contract for the three surfaces, not implementation or measured browser evidence. Spines win over mockups. Pixel values below are CSS px, including the house mapping of the phone's 44 pt target to 44 px. Geometry is measured against the allocated pane, not the application viewport.

## Foundation

**The owner's priorities.** Put icons above one search box, with tags inside it; stop making tag entry unfold a second form. Let a named search return exactly as saved. Put temporary searches under All notes, make slash names a hierarchy and pins a within-group ordering, and replace the rail's hover icons with a right-click menu. The right task pane must show the selected run and its log, not duplicate task configuration; the report `61194 bytes; 83 files: 1 copied, 82 identical, 0 left alone, 0 failed` needs the width of a line, not the width left after several metadata cells.

**Grounding.** The current bar is chips/chooser/toggles above a separate `type=search` input (`src/components/notes/note-filter-bar.tsx:329-560`). Its explicit chooser is at 478–492 and its conditionally mounted clear at 515–531. The rail is a flat `spaces.map`, followed by FilePlus/Pencil/Trash2 hover targets (`src/components/notes/space-list.tsx:213-359`). Run reports occupy a `flex-1` leftover-width span (`src/components/layout/tasks-pane.tsx:1282-1325`); `src/components/layout/panel-strip.tsx` hosts task facts again. These are the replaced surfaces, not hypothetical components. The screenshot wording above is the owner's reported example; no screenshot pixels were available to measure in this planning lane.

**Explicit supersessions.** UX-DR100–102 replace UX-DR94's chooser/row geometry, UX-DR97's single-line input and conditional clear, and UX-DR99's no-second-icon-row rule where 44 px targets require wrapping. AD-268's relative order, names, target floors and no-disappearing-controls rule remain. **Coordinator resolution of AD-273:** “remove-only” binds the field's keyboard contract, not chip semantics. Chip-body click still cycles include → exclude → off, shared with the space editor; chips are not focus stops, Backspace at caret-start removes the last chip, and X removes explicitly. Suggestions add a chip with the chosen sign. UX-DR103–104 narrow UX-DR35's no-dialog premise to allow a non-modal naming popover (AD-271). UX-DR105 replaces the duplicated right-pane task rendering when a run is selected. Navigation history versus note revisions remains outside this document; Chrome's Back/Forward research does not add browser-history controls here.

**Tokens.** Use the system stacks in `DESIGN.md`: body 13 px / 20 px line box, caption 11 px / 16 px line box, section label 11 px / 16 px at weight 600, mono 12 px / 20 px, title 15 px / 20 px at weight 600. Line boxes snap to the inherited 4 px rhythm. Input radius 5 px, row/button radius 7 px, transient surface radius 10 px, tag radius 9999 px. Panes use `--background` / `--foreground`, 1 px `--border` separators and no shadow. Every suggestion, naming popup, menu and tooltip uses `--popover` / `--popover-foreground` and the existing primitive's shadow and 1 px foreground/10 ring; never literal black, an invented fill, or a new palette. AD-279 authorizes the tooltip exception to DESIGN.md's unchanged-primitives rule. Focus/menu-target rings use 2 px `--ring`. Hover/selection uses the shipped neutral role; held amber is not a new hover color, violet is not private-note paint, and recording red is not task-status paint. Existing include/exclude token colors and the destructive text treatment are reused, not recolored.

**Ownership.** Rust owns hierarchy/order/expiry, query results, availability facts and run/log facts. React owns caret spans, focus, disclosure, layout and rendering view models over IPC; it does not parse report sentences into numbers, reconstruct a rail tree from slash strings, compare expiry timestamps to delete a space, or sort combined drive results. The core/shell work belongs to `epic77-core` and `epic78-core`; surfaces belong to `epic77-surface` and `epic78-surface`; this file belongs only to `plan`. Shell changes in `src-tauri/crates/keeper/**` are by inspection, awaiting CI's macOS job.

## A. Search bar — icons above, chips and caret inside one border

### Geometry and control order

There are **2 zones**: an action band above, then **1 bordered field**. The status sentence, when needed, is below the field; the existing notes count stays outside the bar in the pane's count slot. Desktop bar padding is 12 px horizontal / 8 px vertical; phone padding is 8 px on both axes. Gap between action band and field is 8 px. No chips, scope label or tag chooser occupy the action band.

The fixed icon sequence is **Bot → Pin → EyeOff/Eye → ArrowUpDown → HardDrive → LockKeyhole → FilePlus → Bookmark**. The old three toggles retain their order; the new query controls precede creation actions; Save remains last. There is no Plus chooser and no overflow/more menu. Icon ink is 16 × 16 px; each desktop target is 24 × 24 px and each phone target 44 × 44 px. Disabled actions retain their target rectangle and visible icon; disability is by query/capability state, never width.

| Slot | Accessible name = tooltip, verbatim | Meaning / state |
|---|---|---|
| 1 Bot | `Changed by agent` | Stable label; `aria-pressed` reports the current origin restriction |
| 2 Pin | `Pinned only` | Stable label; `aria-pressed` reports the note-pin restriction, not a space pin |
| 3 EyeOff / Eye | `Hide service files` | EyeOff pressed = hidden; Eye unpressed = included; existing persisted preference |
| 4 ArrowUpDown | `Sort notes` | Menu, `aria-expanded`; current effective sort is an accessible description, not a changing label |
| 5 HardDrive | `Search drives` | Multi-select popover, `aria-expanded`; description lists chosen drives; no selected-count badge stealing target width |
| 6 LockKeyhole | `Include private notes` | Default off, stable label and `aria-pressed`; no violet/incognito implication |
| 7 FilePlus | `New note from search` | Creates from the current prompt and included tags; if multiple drives are selected, first asks which one |
| 8 Bookmark | `Save as space` | Opens the naming popover in B; unavailable state retains its slot and explains what is missing |
| Field | `Search notes` | Stable combobox name/placeholder for one or multiple drives; associated description names the effective drive scope |
| Trailing X | `Clear search` | Always rendered; disabled exactly when the text is empty; clears text only, not chips/scope/drives/toggles |

Names and hints are identical through `IconHint`; icons are `aria-hidden`. The field's new scope-neutral name explicitly replaces `Search this vault`, which would lie for multiple drives. Hint delay remains 500 ms. Toggle labels do not change to “Show…” on press. Sort/drive menus close with Escape and return focus to their trigger; they are not the tag listbox.

Sort uses the existing `SpaceSort` labels: Relevance, Order, Name, Created, Modified, Recorded; the selected field and direction are explicit, reusing the existing direction wording in `src/components/notes/space-editor.tsx:108-129`. Drive choices have checked state, stay open for multiple choices, and follow the core's empty-set/current-drive contract without calling an empty set “all drives”. Both popovers are 280 px wide, clamped to the available viewport minus 16 px, offset 4 px below their trigger, with 8 px inner padding; option height is at least 32 px desktop / 44 px phone, with at most 6 visible options before vertical scrolling. New-note drive choice uses the same geometry and single-choice rows; Escape creates nothing and preserves the prompt. Do not promise that an excluded tag, an agent flag, or every arbitrary saved DSL predicate can be synthesized into a new note.

### Field geometry and caps

The frame is full bar-inner width, border 1 px, radius 5 px, padding 8 px. It contains a 16 px decorative Search / Sparkles / static Loader glyph in a **20 px leading lane**, a flexible chip/text region, and an always-present trailing clear target separated by 4 px. The clear lane is 28 px desktop (24 + 4), 48 px phone (44 + 4). Minimum editable-region width is 0; long content must never force the outer pane wider. The glyph and X align to the first 24 px desktop / 44 px phone content row and remain visible when inner content scrolls.

- **Chip ink and targets:** desktop token height 24 px; phone token/target height 44 px. Gap between chips and between wrapped chip rows is 4 px. Label uses caption, one line, 8 px leading inset; the 12 px sign precedes it with a 4 px gap. The chip body cycles its sign and has a minimum 24 × 24 px desktop / 44 × 44 px phone target. The separate trailing remove glyph is 12 px in a 24 × 24 px desktop / 44 × 44 px phone target; body and remove regions never overlap. Full token width is capped by the available chip/text region; label ellipsizes before sign or remove shrinks. Full tag text and polarity remain in the accessible description and popover-palette hover/assistive-technology detail.
- **Inline rest:** scope, if any, comes first, then ordered tag chips, then the text caret in the same inner flow. A one-line editor takes the remaining line when at least 96 px remains; otherwise it wraps to the next line. No separator/border creates a second input. Scope removal remains a distinct named action and is not a tag polarity change.
- **Text:** 13 px text / 20 px line box, resting at 1 line, grows to **3 lines = 60 px**, then scrolls vertically. A multi-line editor takes the full flexible-region width after the chips, so line 2 does not begin in a 96 px sliver beside a token. Soft wrapping changes presentation, never inserts query characters. Hard line breaks are preserved; Shift+Enter inserts one when the person wants it. No horizontal text scroll.
- **Chips:** wrap whole tokens, never words within a token. Up to **3 visible rows = 80 px desktop (3 × 24 + 2 × 4), 140 px phone (3 × 44 + 2 × 4)**. Scope consumes one of these rows when it wraps. More rows scroll within that chip region; no `+N filters` truncation. In capped mode the text starts on its own full-width row after a 4 px gap and stays visible, outside the chip scroller. This is still one bordered field, not a revived chooser fold.
- **Bounds:** empty/one-line field minimum height is **42 px desktop** (2 border + 16 padding + 24 content) / **62 px phone** (2 + 16 + 44). With both independent caps reached, maximum is **162 px desktop** (2 + 16 + 80 + 4 + 60) / **222 px phone** (2 + 16 + 140 + 4 + 60). A newly added token scrolls into the chip viewport; the text viewport follows its caret independently. No animation between heights.

### Exact width budgets

For desktop, field-inner flexible width = column − 24 bar inset − 2 border − 16 padding − 20 glyph lane − 28 clear lane = **column − 90**. For phone it is column − 16 − 2 − 16 − 20 − 48 = **column − 102**. The clear target never consumes editable space unexpectedly because its lane never disappears.

| Allocated column | Desktop field / editable-region widths | Desktop action band | Phone field / editable-region widths | Phone action band | What yields |
|---|---|---|---|---|---|
| **240 px floor** | 216 / 150 px | 8 × 24 = 192 px, 0 px gaps, 1 row | 224 / 138 px | 4 + 4 targets; each line 4 × 44 + 3 × 4 = 188 px | Whole chips wrap; labels ellipsize; long text grows. No control drops. |
| **320 px** | 296 / 230 px | 8 × 24 + 7 × 4 = 220 px, 1 row | 304 / 218 px | 6 + 2 targets; first line 284 px | Same chip/text caps; phone band wraps in DOM order. No control drops. |
| **420 px** | 396 / 330 px | 220 px, 1 row | 404 / 318 px | 8 targets, 380 px, 1 row | More chips share a line; no control or label is added. |
| **720 px** | 696 / 630 px | 220 px, 1 row | 704 / 618 px | 380 px, 1 row | Field uses the available width; maximum heights stay identical. |

Desktop switches to 4 px icon gaps when its inner width reaches 220 px (column 244 px); below that it uses 0 px. Phone uses 4 px column/row gaps and natural wrapping with no CSS reordering; action-band height is 92 px for 2 rows and 44 px for 1. On desktop it is always 24 px. All bands are start-aligned. Two *zones* do not mean a promise of exactly two painted lines on a 240 px phone. The existing 48 px folded list rail remains a different surface: Search unfolds and focuses this field without clearing state.

### Suggestions and keyboard

A suggestion is for the whitespace token under the caret, not the whole prompt. Reuse the fetched tag vocabulary and `matchTags`; the caret-span exception is AD-274, not a second vault/query parser. A click that places the caret on a recognized tag token opens the same suggestion; a second click must also work without requiring double-click word selection. Typing past that token closes its old suggestions; a new matching token may produce a new list. IME composition is ordinary editing: neither Enter nor Backspace commits/removes tokens during composition.

Anchor the tag listbox to the **bottom start of the entire bordered field**, offset 4 px, not to a moving caret or icon. Width equals the field's border-box width; viewport collision padding is 8 px and it flips above if needed. Use 4 px inner padding. Each tag has **2 successive options**, `+ tag` then `− tag`, rather than buttons nested inside an option: accessible names `Include tag {tag}` and `Exclude tag {tag}`. At most **6 option rows / 3 tag pairs** are visible: 32 px per desktop row / 44 px phone row, for 200 / 272 px including padding; further matches scroll. Long labels truncate visually with their full accessible names. Current polarity has a check glyph as well as wording; no duplicate chip is created when that same polarity is accepted.

The editable node uses combobox/list-autocomplete semantics with a controlled listbox. DOM focus stays in the textbox while `aria-activedescendant` identifies the active option. Down Arrow enters option 1; Up/Down traverse options and scroll the active option into view. Enter accepts only an active option: remove exactly the captured token span (including its typed `-` alias), leave every other character/whitespace intact, insert/update the chip once, and retain the caret at that removed span in the surviving text. Enter without an active option does not silently accept suggestion 1. Tab leaves the field without accepting; Left/Right/Home/End and ordinary editing continue to edit text rather than navigate a chip grid. Escape first closes the suggestion, then on a separate press clears nonempty text, then follows the existing one-chip-at-a-time empty-field escape walk. One event has one effect.

**Chips are keyboard-remove-only, not focus stops; click still cycles.** Clicking/tapping the body cycles include → exclude → off exactly as the shared `TagFilterChip` does today. Preserve body names `Tag {tag}: included. Exclude it instead.` / `Tag {tag}: excluded. Stop filtering by it.`; no binary `aria-pressed`. The separate X remains `Clear tag {tag} filter`. Both actions are omitted from sequential Tab traversal; assistive technology can reach their actions and pointer/touch can activate them, with textbox focus/caret retained or restored. No chip grid is nested in the combobox. Backspace with a collapsed caret at text offset 0 **immediately removes** the last tag chip once, without a select-first intermediate state; elsewhere retain native deletion/selection behavior. Announce `Removed {tag} filter` politely. Suggestions add signed chips; accepting an already-present tag updates its sign without duplication. Keyboard-only users can remove/re-add with the desired sign through the same suggestion flow. Include/exclude counts and signs remain in the field description; color is never the sole carrier.

This borrows Primer's remove/wrap/max-height keyboard pattern and APG's manual-selection combobox, not Linear's separate formula-chip editor: research §1.2–1.3 and §2 ([Primer](https://primer.style/product/components/text-input-with-tokens/), [APG](https://www.w3.org/WAI/ARIA/apg/patterns/combobox/), accessed 2026-09-20). **Keeper deliberately differs from Primer's fully remove-only token semantics by retaining its existing click-to-cycle body**, per the coordinator's AD-273 resolution above. Primer's cited component is deprecated; it is an interaction precedent, not a dependency proposal. GitHub's `-` qualifier convention informs the input alias, not GitHub query syntax. Linear documents creating filters from typed `@` tokens but does not establish in-box chip replacement; keeper's span-only replacement is its own decision.

### State coverage

| State | Visible and observable behavior |
|---|---|
| Empty | Mode glyph, placeholder, all 8 action slots; clear X visible but disabled. No empty-chip lane height. Setup/index/error sentence may still apply. |
| Text only | Caret in first content row; text grows from 1 to 3 lines; clear is enabled even for whitespace. One activation empties the controlled text and refocuses the field. |
| Chips only | Chips followed by resting caret; clear remains disabled because it does not clear filters. Backspace at start removes the final tag. |
| Chips + text beyond 3 lines each | 80/140 px chip viewport, 4 px separation, 60 px text viewport inside the same border. Both scroll without page overflow or hiding X. |
| Suggestion open | Full-field-width listbox; active option visible; Escape dismisses without clearing; accepting changes only the token span. Empty vocabulary/no match does not offer to create a tag. |
| Vocabulary loading/failure | If the suggestion layer was requested, show `Loading tags…` or an inline failure sentence in the same popover; no selectable fake option. Existing query/chips remain usable. |
| Searching | Keep effective Search/Sparkles glyph; do not claim indexing. After 500 ms show `Searching…` in the existing status slot unless a higher-priority error/indexing state owns it. No pulsing spinner. |
| No results | Result area says `No matches in the selected drives.`; clear-text recovery leaves chips/drives intact. Unknown/failed/pending count is not 0. All-service/private-hidden cases retain the core's withheld-count explanations and named reveal action. |
| Semantic leg unavailable | Words results remain visible; show `Meaning is unavailable — {safe reason}. Searching words only.` with Settings when actionable. No-model state retains `Meaning is off — choose an embedding model in Settings`. A skipped query embedding is not silently advertised as hybrid. |
| Space changed | Incoming space replaces text, chips, scope, flags and sort, including an empty prompt; the persisted service-view preference is not silently rewritten. An old response cannot restore the previous text/results. Clear works on the first activation after the transition. |
| Create/save pending or failed | Query remains intact. Disable only the in-flight action to prevent duplicate activation; name the failure next to its owning popover/action, not as a no-results state. |

Status is caption / 16 px, with an 8 px gap below the field only while present. Keep 1 visible line, ellipsizing explanation before the nonshrinking Settings action; the complete sentence is the accessible description and appears on focus/hover in a popover-palette hint. Error > indexing > stalled search > semantic availability; all concurrent facts remain in the description. No healthy “Ready” sentence. Search provenance/highlighting remains UX-DR95–96 except the epic's explicit sort override.

### UX-DR100 — Two zones, one field, no missing controls

**Rule.** The fixed eight-control band precedes a single border containing scope, tag tokens and text; width changes wrapping, not availability or order.

**Mechanism.** Apply the exact target/width/height budgets above in `src/components/notes/note-filter-bar.tsx`; `src/components/notes/notes-phone-pane.tsx` uses the same bar and 44 px tier. The chooser fold is removed. Keep independent three-row chip and three-line text caps, an unconditional clear slot and the 48 px folded-rail handoff.

**Tokens.** DESIGN.md's 4 px rhythm, 5 px input / 7 px button radii, inherited input/ring roles, existing signed-token paint; no new colors.

**What is deliberately not on it:** hidden actions, a second tag-input form, an icon overflow menu, a smaller target to fake the 240 px fit, or a taller column floor.

### UX-DR101 — A token suggestion changes only its token

**Rule.** A suggestion adds a signed chip without consuming the surrounding prompt; chip-body click keeps the existing three-state cycle, while field keyboard operations remove without moving focus into chips.

**Mechanism.** `src/components/notes/note-filter-bar.tsx` owns caret/listbox interaction and preserves the shared chip-body cycle; `src/components/tags/tag-match.ts` remains the matcher; `src/lib/stores/notes-filters.ts` receives the term/text operation. Six visible options, two per tag, input-owned active descendant, keyboard-remove-only tokens with explicit X, native editing/IME protection, and popup-first Escape follow the contracts above.

**Tokens.** Field-width `--popover` layer at 4 px offset, 10 px radius; 32/44 px option rows; existing neutral active-row and 2 px ring treatments.

**What is deliberately not on it:** a focusable tag grid, chip Tab stops, whole-prompt replacement, automatic tag creation, a second chip-state grammar, or imported Primer code.

### UX-DR102 — Search state speaks without moving the controls

**Rule.** Clear works once and retains filters; pending, empty and meaning-unavailable are distinct facts, not interchangeable blank lists.

**Mechanism.** `src/components/notes/note-filter-bar.tsx` renders a permanently mounted X and non-search-type multiline editing surface; `src/lib/stores/notes-search-state.ts` carries effective availability and `src/hooks/use-notes-changes.ts` rejects obsolete answers. `src/components/notes/notes-pane.tsx` and the phone counterpart own count/empty results. Core/shell availability and privacy facts must be supplied before the renderer can make the sentences above.

**Tokens.** Static 16 px state glyph, caption/16 px status, existing paired popover hint tokens. No extra AI/status hue.

**What is deliberately not on it:** native search cancel chrome, second-click clearing, a fake hybrid glyph, lexical search blocked on embeddings, or hidden-count arithmetic from mounted rows.

| Owner ask | Specified difference | Reason |
|---|---|---|
| “Two rows, icons above” | 2 zones; the phone icon band wraps to 2 lines at 240/320 px | Eight 44 px targets cannot fit one 224/304 px inner line; nothing may disappear. |
| Tags as widgets in the box | Click cycles include → exclude → off; keyboard removes without focusing chips; suggestions add | Coordinator resolves AD-273 in favor of the existing shared click grammar plus Primer-inspired keyboard behavior, not Primer's fully remove-only semantics. |
| Click the word again for +/− | Any caret click on a recognized token can reopen; no double-click requirement | Works with keyboard/touch caret placement and does not hijack native word selection. |
| Clear the prompt | Always-visible X clears text, not tags/drive scope | The person asked to clear the prompt, not discard the rest of the query. |

## B. Spaces rail — All notes, temporary group, slash groups

### Rows, grouping and lifetime

The rail keeps its existing 240 px default / 180 px floor; it is not the 240 px-minimum notes-list column. Use 8 px row insets, 4 px between glyph and label, 4 px vertical gap between sections, and 7 px row radius. Desktop row minimum height is 32 px; phone is 44 px. A second caption line adds 16 px, with 4 px gap below the 20 px name line and 8 px total vertical padding: a two-line row is **48 px**. Names use 13 px / 20 px; counts/lifetime captions use 11 px / 16 px, `--muted-foreground`, never warning amber.

`All notes` stays first. Immediately below it, indented **12 px**, show disclosure **`Temporary spaces`** with its saved-space count **when at least 1 temporary space exists**; at 0 the group is absent, matching Rust's synthetic-row contract. It is not a second selectable All notes query. Default it expanded when first encountered. Both manually saved temporary spaces (default 48 h) and auto-parked `actual-<parent space name>` searches (2 h) live here, showing their full names rather than joining the slash tree. Order is All notes → Temporary spaces (pinned first, then soonest-expiring within each pin partition) → persistent slash tree depth-first (siblings pinned, then existing rail order) → Uncategorized. React consumes this order, never re-sorts it or fabricates editable files for synthetic rows.

For `Journal/Bali`, show `Journal` with a 12 px chevron in a **24 × 32 px desktop / 44 × 44 px phone** disclosure target, then `Bali` indented another 12 px when expanded. A row with children owns `aria-expanded` and a named child group; accessible labels retain the full path. A virtual `Journal` has no select-query action: clicking its name or caret expands/collapses. If `Journal` is itself a saved space, its label enters that space while the distinct caret only discloses children; these targets never overlap. A leaf uses the same leading disclosure lane as a spacer so sibling labels align. Retain its 16 px space icon and existing broken-space lamp/subtitle treatment. Counts at the trailing edge reserve 32 px and mean **descendant saved spaces, not notes and not virtual groups**; full large counts remain accessible if visually ellipsized. Rust supplies the grouping/count/order facts.

Indent is 12 px per level, visually capped at **36 px** so arbitrarily deep slash paths do not consume the 180 px rail. Semantic depth is not capped: full path and hierarchy remain accessible, and deeper nodes reuse the 36 px indent with their parent path in focus/hover detail. Names ellipsize to 1 line; expiry and warning captions wrap rather than force horizontal page scrolling. Disclosure state survives a rail refresh in the same mounted vault; changing a name or pin must not collapse an unrelated group.

Within **each** group, pinned children precede unpinned children in Rust order. If there is at least 1 pin, render a `PINNED` section label (11 px / 16 px, 4 px top/bottom padding), followed by the pinned rows; if unpinned siblings follow, place a 1 px `--border` separator with 4 px vertical margins, then those rows. There is no empty pinned heading and no second global favorites copy. A 12 px decorative Pin may accompany a pinned row's label; it is not a hover action. Pinning does **not** make a temporary space permanent or exempt it from expiry.

A temporary leaf has an always-visible lifetime caption beneath its name while it exists: `expires in N d` for at least 24 h remaining, rounding up whole days (48 h shows `expires in 2 d`); under 24 h show **`expires today`**, not hours/minutes/seconds. At/after the acknowledged expiry while removal is pending use `Expiry pending`, not a fabricated negative count. The exact expiry timestamp and `Temporary · resets after opening · 48 h lifetime` (or its actual duration) are available in focus/hover detail and the row's accessible description. Hover/menu/disclosure alone never resets the clock. Opening the actual space does, subject to AD-270's write threshold: only after more than 10% of its TTL has elapsed; captions always show the acknowledged file fact. Failed refresh/save reports failure, not a fictitious reset. No per-row interval animates a countdown; update from the rail's normal snapshot/time presentation.

Expired spaces leave the rail only after the reconciler's acknowledged move to vault trash; this never deletes notes matched by that space. Trash/recovery is the existing notes mechanism, not a new temporary-space recycle bin. Arc supplies reset-on-open and an archive stage, and Chrome supplies staged inactive-item handling (research §5, [Arc](https://resources.arc.net/hc/en-us/articles/19228855311127-Auto-Archive-Clean-as-you-go), [Chrome](https://support.google.com/chrome/answer/2391819?hl=en&co=GENIE.Platform%3DAndroid), accessed 2026-09-20). Neither establishes a shipping reset-on-open saved-search pattern. Whole-day wording, the 48 h/2 h defaults and the write threshold are keeper decisions, not claims about Arc.

### Context menu and its target

The three hover buttons are **removed from the row, not made more transparent**:

| Removed icon | Replacement |
|---|---|
| FilePlus, `New note in {space}` | Context-menu item `New note in this space` |
| Pencil, `Edit space {space}` | Context-menu item `Edit space…` |
| Trash2, `Delete space {space}` | Last context-menu item `Delete space…`, preserving the existing confirm/trash path |

Invoke with right-click/control-click, Shift+F10/Menu on the focused row, or the existing touch long-press route. No new ellipsis appears on hover. The menu's accessible description names the full target path, so a short `Bali` row cannot be confused with another group's Bali. The menu is 240 px wide, clamped to viewport minus 16 px; 4 px padding; 32 px desktop / 44 px phone item minimum height; 1 px separators with 4 px vertical margins. Pointer invocation anchors at the invocation point; keyboard invocation at the row's bottom-start; collision padding is 8 px. Maximum height is viewport height minus 16 px with vertical scrolling.

Order for a real editable space, with separators exactly as shown:

1. `Open space`
2. `Add to current search`
3. **separator**
4. `Pin space` / `Unpin space`
5. `Edit space…` (name, lifetime and existing space properties remain in this editor)
6. **separator**
7. `New note in this space`
8. `New space…`
9. **separator**
10. `Delete space…`

Open replaces the whole query (AD-272); Add is the only merge verb. Pin/unpin acts on the right-clicked space, not the selected one. New space uses the naming flow below; creating under a slash group prefills that group's prefix. Virtual groups offer only `Expand group` / `Collapse group`, separator, `New space…`; synthetic All notes/Uncategorized omit pin/edit/delete because they have no file, and retain only their meaningful open/add/create actions. Do not offer disabled destructive commands for imaginary files. A broken saved query retains edit/delete and its existing explanatory subtitle; open/add must refuse honestly rather than silently widen the search. Existing restore-defaults header action remains reachable even with Spaces folded.

**Menu-target marking is not selection.** While the menu is open, draw a **2 px inset `--ring` ring** around exactly its row, radius 7 px, and expose `aria-expanded=true`/menu popup state on its invoking control. The selected-space fill and `aria-current` do not move. Escape, click-away and choosing an item remove the menu ring through the menu's `onOpenChange`; dismissal alone does not navigate, pin, save or reset TTL. Return keyboard focus to the invoking row unless an invoked editor owns focus. An already selected row has fill + ring; an unselected row has ring only. This is the AD-280 shared idiom for note, space, task and run rows, not four independently timed highlights.

Research §4 grounds the state distinction in Radix's open state and APG focus return, supported by Linear's active-trigger convention ([Radix](https://www.radix-ui.com/primitives/docs/components/context-menu), [APG](https://www.w3.org/WAI/ARIA/apg/patterns/menubar/), [Linear](https://linear.app/changelog/2021-11-08-linear-preview-new-filters), accessed 2026-09-20). The AppKit outline evidence was secondary retrieval; do not promote it to a verified current-HIG requirement or copy its blue color.

### Save as space naming popover

Anchor **4 px below Bookmark, end-aligned**; when opened by the existing save shortcut, use that same button anchor. From `New space…`, anchor below the invoking row after its menu closes. Width **320 px**, capped to viewport width minus **16 px**, with 8 px collision margins and a flip above if needed. It may extend beyond a 240 px list pane: it is a portal, not a reason to widen that pane. On a 240 px viewport its width is 224 px. Surface `--popover`, radius 10 px, padding 12 px, gaps 8 px; title `Save as space`, 15 px / 20 px. No modal scrim and no focus trap.

Visible content order: label `Name`, single-line input (32 px desktop / 44 px phone), a 16 px caption explaining `Use / to group spaces`, checkbox `Temporary space` (unchecked for ordinary save), its conditional `Expires after inactivity (hours)` numeric input prefilled **48**, then Cancel / Save footer. Temporary helper: `Opening this space refreshes its lifetime. Expired spaces go to Trash.` The duration is validated against the core's positive-u32 contract, not a second frontend limit; invalid input shows the core-backed inline error and prevents submission. The full popover scrolls at viewport height minus 16 px if the on-screen keyboard/short window leaves less room, keeping fields/footer reachable. Default footer targets are at least 64 × 32 px desktop / 64 × 44 px phone, with an 8 px gap; they wrap only if the clamped inner width cannot contain both.

For Save as space, prefill the current composed name (existing scope → signed tag terms, with exclude spelled `not …` → agent phrase → quoted nonblank prompt, joined by ` · `, fallback `Untitled space`, from `src/components/notes/notes-pane.tsx:290-298`). Select the prefilled name on entry; do not truncate its stored value. Slash remains a naming/hierarchy convention interpreted by Rust. New space from a group seeds `<group path>/Untitled space`; it is still a draft, not an automatically saved empty query. Unsupported/empty-query validation remains visible rather than inventing a match-all query. Capture the current search once when opening; a later background response cannot change the named draft.

Enter saves when valid and not composing text; Escape cancels with no write and returns to the trigger. Tab may leave; clicking away cancels the unsaved draft. While saving, retain the draft, change the action caption to `Saving…`, prevent duplicate submit, and show any failure inline below the affected field/footer. On acknowledged success, close and refresh/reveal the new row from the returned `NoteSpaceVm`, expanding its parents without changing the current query as a side effect. If there is no rail in the phone level, show `Space saved: {name}` in the existing status channel and expose it on the next rail visit. Never display a success before the file save; never require vault-switch/relaunch to discover the row.

### UX-DR103 — The rail exposes hierarchy, pinning and lifetime

**Rule.** Temporary searches are children under All notes, slash groups disclose their contents, and pins reorder only their own group; expiry is visible without a sub-day countdown.

**Mechanism.** `src/components/notes/space-list.tsx` renders the Rust-composed hierarchy/order/counts with the exact rows and disclosure targets above; `src/components/notes/space-editor.tsx` exposes lifetime/pin facts without deriving them. `src/lib/stores/notes-filters.ts` enters a space with complete replacement, not residue from the previous search. Core space parsing/order and shell reconciler supply file-backed expiry and trash behavior (AD-269/270/272).

**Tokens.** 12 px level indent capped at 36 px, 32/44 px minimum rows, 48 px two-line rows, 11 px captions and 7 px radius; existing sidebar/neutral roles only.

**What is deliberately not on it:** global duplicated pins, pin-implies-permanent, ticking second counters, client-side expiry deletion, or an invented query for a virtual group.

### UX-DR104 — A menu marks its target and a save asks for a name

**Rule.** Context-menu open state marks the targeted row without selecting it; save asks for a name beside the action and makes the acknowledged space immediately discoverable.

**Mechanism.** `src/components/notes/space-list.tsx` removes all three hover targets and consumes the shared open-state idiom through `src/components/ui/context-menu.tsx`; `src/components/notes/note-filter-bar.tsx`, `src/components/notes/notes-pane.tsx`, `src/components/notes/notes-phone-pane.tsx` and `src/hooks/use-notes-actions.ts` carry the draft/save/returned-row flow. Menu dismissal leaves selection/query/TTL alone. Naming is a non-modal popover, not a revived create dialog.

**Tokens.** 2 px inset ring for menu ownership, 240 px menu, 320 px naming surface, both clamped to viewport minus 16 px and painted with `--popover` / `--popover-foreground`.

**What is deliberately not on it:** right-click-select, ghost hover buttons, a black tooltip exception, a focus trap, or optimistic success with a stale rail.

| Owner ask | Specified difference | Reason |
|---|---|---|
| Temporary subspaces under All | Named `Temporary spaces` disclosure directly below All notes, present only with at least 1 temporary search | Gives all temporary searches one destination without an empty synthetic section; Rust owns group presence. |
| Opening resets expiry | File-backed refresh after more than 10% of the TTL, not a write on every click | AD-270 avoids synced commit churn while preventing device-local deletion clocks. |
| Pin above unpinned | Within each group; pin does not suppress expiry | Ordering and lifetime are separate facts; Arc's tab-pin behavior is not silently imported. |
| Mark the right-clicked row | Ring without changing selection; clear ring on dismissal | AD-280 and research §4 distinguish menu target from navigation/selection. |
| Popup to create a space | Anchored, non-modal naming popover | Narrows UX-DR35 without blocking the whole window or hiding the saved row. |

## C. Task run detail — facts first, log below

### Navigation, selected row and full-width facts

Keep task configuration in its task surface. Clicking/pressing Enter on a run in the middle column opens that **specific run** in the right pane through the run panel target (AD-284), not another task configuration instance. The heading identifies task and run; switching runs immediately changes heading/selected state and clears the old log while the new one loads. Restoring a run target restores its identity, never “whatever was latest”.

A run row has 8 px horizontal/vertical padding, 7 px radius, 4 px between metadata and report blocks, minimum 44 px height and no fixed maximum height. First block is outcome/time/host/trigger/lateness with 8 px gaps and ordinary wrapping. The report is a **second block occupying 100% of the row's inner width**, never `flex-1` alongside host/time. Use body 13 px / 20 px; unbounded engine sentences wrap at spaces with `overflow-wrap:anywhere` for long paths/identifiers. No ellipsis or 2-line clamp on run facts. At 240/320/420/720 px row widths the inner report widths are 224/304/404/704 px; it may use more than 2 lines at the floor.

Selected run uses the existing neutral selected fill and foreground, plus `aria-current=true` and a full accessible name identifying task/run/outcome. Keyboard focus uses a 2 px `--ring` focus indicator; a different right-clicked run gets UX-DR104's inset menu ring without stealing the selected fill. No green status dot doubles as selection. Run rows remain buttons/links in a list, not a new listbox that overloads arrow-key behavior. Selecting a task or dismissing a context menu cannot silently claim a different run is selected.

The right pane uses 12 px horizontal/vertical body padding and 12 px section gaps, with full-width facts above the log. On a 240/320/420/720 px allocated pane, the facts have **216/296/396/696 px**. Use one 15 px / 20 px heading, then outcome and timestamp/host/trigger on their own full-width fact lines, then the report. A label/value fact is a block, not a fixed 2-column key/value grid. Source/destination/ledger paths use 12 px mono / 20 px and wrap anywhere, without occupying a side column beside the report.

The owner's sample remains a truthful uninterrupted report value, wrapped across the full measure:

```text
Run 42 · Copy archive
Completed
Started … · Host …
61194 bytes; 83 files: 1 copied, 82 identical,
0 left alone, 0 failed

Log
Load older
…latest stored log lines…
```

Diagram line breaks are illustrative, not a requirement to parse punctuation. Rust owns all counts and any new overwritten/deleted facts; React must not split this string at `;` or infer counts by reading file-log lines. If the wire supplies structured fact groups, they may occupy separate full-width lines; if it supplies one report sentence, render it whole. Zero is a real reported zero; absent reports say `No run report was recorded`, not fabricated zero counters. A running outcome says `Running`, without claiming final counts.

### Tail-first log geometry and behavior

After facts, place a 1 px `--border` divider with 12 px above/below spacing, heading `Log` (13 px / 20 px), then an 8 px gap and the log region. The log is ordinary pane content, not a black terminal-card theme: `--background` / `--foreground`, 1 px `--border`, 5 px radius, 8 px inner padding, 12 px mono / 20 px lines. Render escaped text and preserve line breaks; soft-wrap long paths anywhere instead of horizontal page scrolling. A real scrollable log viewport has preferred height **320 px**, min-height **120 px when that much pane height is available**, max-height **480 px**; on shorter panes it shrinks with `min-height:0` and the outer pane remains scrollable. No 120 px minimum may push the controls off a short/phone viewport. No fixed row-count estimate is substituted for bytes.

First request is the bounded **tail**, initial/page budget **65,536 bytes**. It renders in original file order, older above newer; “tail-first” does **not** reverse lines. The initial viewport is positioned at the end after the tail arrives, and reads `Showing latest log content` if earlier bytes exist. Keep a **32 px desktop / 44 px phone** top toolbar outside the scrolling text, with `Load older` at its start while the reader supplies an older cursor. Each activation requests one older chunk using the returned cursor, shows `Loading older…` in that same slot, and disables duplicate activation. On prepend, preserve the previously top-visible text's viewport position; do not jump to the new beginning or bottom. At the oldest boundary replace the action with caption `Beginning of log`. The renderer never guesses byte offsets, assumes a line fits in a chunk, or independently decodes split UTF-8; the bounded reader contract owns those boundaries.

Keep a maximum **1,048,576 decoded-text source bytes** mounted per run. Once a prepend would exceed that window, evict the newest already-read chunks and show `Older log content · Return to latest` in the toolbar; Return to latest re-requests the bounded tail. Do not grow the DOM without bound as Load older is repeated. This byte-window policy is keeper's implementation budget, not an externally sourced product claim, and must be kept consistent with the epic-78 reader contract. A request/response is scoped to task + run + log generation; a late older-page response from run A must not paint over run B.

The log is read-only and independently text-selectable. Its scroll region is keyboard focusable with accessible name `Run log`; standard Page Up/Down/Home/End operate within it when focused. Selecting text or loading older never follows the tail automatically. This design does not promise a live-tail subscription: a running run may truthfully say `Log will be available when this run writes it`. If the backend reports a changed/truncated log while paging, retain readable content, show `Log changed. Reload latest` and restart from the reader's current generation on that action rather than splicing unrelated bytes.

### Run-detail states

| State | What the pane shows |
|---|---|
| No run selected | `Select a run to read its log.`; no duplicate task form pretending to be a run |
| Run/log loading | Known run heading/facts remain; `Loading log…` in the log region; no old run's text, no shimmer |
| Completed with changed files | Summary facts, then copied / overwritten / deleted / FAILED / collision (`left-alone`) lines in stored order; identical files are counts-only |
| Completed with **no changed files, no failures and no collisions** | The stored summary remains; caption `No files changed.`; 0 per-file rows; not `Log missing` and not an empty error surface |
| Failed run / failures without changes | Failure outcome and actual FAILED lines/reasons; never the success-like no-change caption alone |
| Collisions, including a run with no changes | Summary plus actual collision (`left-alone`) lines name each file the run refused to replace. If no files changed, retain `No files changed.` alongside those refusal lines, never instead of them. Identical files still have no per-file rows. |
| Ledger not configured | Rust-composed unavailable sentence naming the missing ledger and Settings remedy; expose the planned Settings → Tasks route (78.5), while retained summary facts remain readable. No empty box or unresolved spinner. |
| Historical file absent | Rust-composed sentence explaining that this run's log is no longer available; keep run facts and distinguish absence from an empty changed-file list. No frontend diagnosis from a missing body. |
| Read failure / older-page failure | Inline actual safe failure plus Retry in the owning log/toolbar area; already-read text is kept; no synthetic “Beginning of log” |
| Cursor reaches start | Beginning caption replaces Load older; no more reader requests for an absent cursor |
| Large/long-path log | Soft-wrap in the region, 65,536-byte requests, 1,048,576-byte mounted window; older/newest navigation stays available |
| Run switched during a request | New run owns selection/heading/loading; old response is discarded |
| Menu open on another run | Selected run pane stays; only the invoked row gains the transient 2 px ring |

### UX-DR105 — A run owns the right pane

**Rule.** A selected run gets full-width facts and a bounded tail-first log; identical files are counts, while changes, failures and collisions retain named file evidence.

**Mechanism.** `src/components/layout/tasks-pane.tsx` makes run rows actionable and gives report text a whole block; `src/components/layout/panel-strip.tsx` renders the run target; `src/lib/stores/panels.ts` preserves task/run identity; `src/lib/ipc/client.ts` consumes `sync_task_run_log`. `src-tauri/crates/keeper-sync/src/copy.rs` supplies AD-283's changed/failure/collision log, with identical entries omitted; `src-tauri/crates/keeper/src/sync_ipc.rs` and the ledger reader supply bounded text, older cursor and truthful availability. No client parser manufactures log facts.

**Tokens.** Body/mono metrics, 12 px pane insets, 320 px preferred log viewport, existing background/border roles, neutral selection fill, and the shared 2 px menu-target ring. No new terminal color scheme.

**What is deliberately not on it:** a second task form, a report squeezed beside host/time, reversed log lines, unbounded log mounting, identical-file chatter, a fake live-tail promise, or a “no changes” success sentence hiding failures or collisions.

| Owner ask | Specified difference | Reason |
|---|---|---|
| Facts take “a line or two” | A full-width block with no line-count cap | At 240 px the true report may need more than 2 lines; clipping would hide failed/left-alone counts. |
| Tail if long, batch loading | Tail first in file order, explicit Load older, bounded byte window and Return to latest | Protects both the shell read and renderer memory while preserving the reading position. |
| Log only copied/deleted/overwritten files | Also show FAILED and collision (`left-alone`) lines; identical files remain counts-only | Coordinator's AD-283 amendment: a collision names a file the run refused to repair. Hiding that evidence would repeat the owner's silent-nonrepair problem; the 82 identical rows are the removable noise. |
| Right-click marks a run | Ring on the menu target; selected run fill and pane remain unchanged | Dismissal must not navigate or mutate selection. |

## Implementation handoff and verification contract

This planning file runs no product code and makes **no visual verification claim**. The budgets are arithmetic and interaction contracts, not screenshots of implemented controls. No build, formatter, source edit or project-wide validation is part of this lane. The implementation must exercise the real mounted desktop and phone surfaces, not only isolated jsdom components.

**Planning proof performed:** a `jq` arithmetic check over 240/320/420/720 px confirmed field widths 216/296/396/696 px desktop and 224/304/404/704 px phone, editable widths 150/230/330/630 px desktop and 138/218/318/618 px phone, and phone wrap capacities 4/6/8/8 targets. All eight 24 px desktop targets fit even at the floor. This checks the numeric budget only; it is not a rendered-surface or accessibility result.

| Surface / stack rung | Observable proof required, with landing files | Existing tests to retarget, not re-pin |
|---|---|---|
| Search / `epic77-surface` after `epic77-core` | In `src/components/notes/note-filter-bar.tsx` and `src/components/notes/notes-phone-pane.tsx`, measure 240/320/420/720 px allocated widths, eight targets/order, 24/44 px floors, both caps and no page overflow. Drive token acceptance in the middle of a prompt, polarity replacement, Backspace at start, IME, popup-first Escape, clear on first pointer activation after a space switch, and new-note/save flows. | `src/components/notes/note-filter-bar.test.tsx:299-324` pins the removed chooser fold; `:382-393` must cover actual clear transitions rather than be accepted as browser evidence; `:116-123` button presence alone does not prove saving. Keep matching/state behavior tests where they defend real contracts. |
| Rail / `epic77-surface` after `epic77-core` | In `src/components/notes/space-list.tsx`, drive All/Temporary, 0 and many children, Journal/Bali, same-name leaves, deep hierarchy, pins inside Temp, expiry captions above/below 1 day, failed reset, broken query, context-menu cancel and save→returned row without remount. Measure at the rail's 180/240/320 px widths; test keyboard and long-press. | `src/components/notes/space-list.test.tsx:141-176` pins residue/toggle selection; rewrite for replace versus explicit merge. Retarget hover-button queries to menu behavior; `src/components/notes/notes-pane.test.tsx:315`'s save mock needs a reachable save→rail observation. |
| Shared target marking / `epic77-surface`, reused by `epic78-surface` | `src/components/ui/context-menu.tsx` idiom used by note/space/task/run rows: menu on B while A selected gives ring B/fill A; Escape/click-away clear only ring B and restore focus appropriately. Check light/dark paired popover tokens. | `src/components/notes/note-row.test.tsx:290-355` must cover dismissal/selection distinction, not merely the available menu verbs. |
| Run detail / `epic78-surface` after `epic78-core` | `src/components/layout/tasks-pane.tsx` and `src/components/layout/panel-strip.tsx`: open a run from the actual middle column, read the owner's report at 240/320/420/720 px, load tail then older, observe stable reading position, no-change/failure/missing-ledger states, byte-window boundary, text selection and late A→B responses. | `src/components/layout/panel-strip.test.tsx:1015-1024` pins duplicated task facts and must become run identity/detail behavior. `src/components/layout/tasks-pane.test.tsx` run-report/history assertions and `src/lib/stores/panels.test.ts:219-250` target semantics must follow the new reachable run target. |

For all surfaces: light/dark, 200% zoom, long untranslated paths/names, keyboard-only operation, screen-reader names/state, short window and phone keyboard inset. Use the real `dev/probe`/app harness; read DOM target rectangles, overflow and scroll positions while exercising controls. A screenshot establishes paint, not one-click clearing, token-span preservation, saved-row refresh or bounded paging. Source-text/old-markup tests are deleted or rewritten to observable behavior, never updated to enshrine the replacement markup.

## UX-DR → story mapping

| UX decision | Owning story / rung | Binding |
|---|---|---|
| UX-DR100 — Two zones, one field, no missing controls | 77.6 / `epic77-surface` | AD-268, AD-273; NFR-69 target floors retained |
| UX-DR101 — A token suggestion changes only its token | 77.6 / `epic77-surface` | AD-273, AD-274 |
| UX-DR102 — Search state speaks without moving the controls | 77.4 producer / `epic77-core`; 77.6 renderer / `epic77-surface` | AD-275, AD-276, AD-277; UX-DR97 state truth |
| UX-DR103 — The rail exposes hierarchy, pinning and lifetime | 77.2 producer / `epic77-core`; 77.3 renderer / `epic77-surface` | AD-269, AD-270, AD-272 |
| UX-DR104 — A menu marks its target and a save asks for a name | 77.1, 77.3, 77.7 / `epic77-surface`; shared idiom consumed by 78.4 / `epic78-surface` | AD-271, AD-279, AD-280 |
| UX-DR105 — A run owns the right pane | 78.3/78.4 reader and log producer / `epic78-core`; 78.4 renderer / `epic78-surface` | AD-283, AD-284 |

**What stays out.** No source implementation, new dependencies, alternate theme, editor navigation redesign, saved-query DSL parser in TypeScript, recording changes, live log subscription, or copy-policy changes. The epic/spec lanes own FR/NFR allocation and the copy engine's deletion/restoration/lookback policy; this spine neither re-triages those verdicts nor silently changes pinned architecture decisions.
