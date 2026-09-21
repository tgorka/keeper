---
name: keeper
parent: EXPERIENCE-NOTES-SEARCH.md
status: final
created: 2026-09-20
updated: 2026-09-20
binds: Epics 77 and 78 (historical), amended by Epics 79 and 80; AD-269…AD-280, AD-283, AD-284, AD-287…AD-296; UX-DR100…UX-DR110
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
  - local://coordinator-decisions-2.md
  - local://triage2-ScoutBar2.md
  - local://triage2-ScoutRail2.md
  - local://triage2-ScoutNav2.md
---

# keeper — Experience Spine, Search bar, spaces rail and run detail

> Extends `EXPERIENCE-NOTES-SEARCH.md`, continuing UX-DR94–99 at UX-DR100. Everything not explicitly replaced keeps its existing contract. This is the visual/interaction contract for the three surfaces, not implementation or measured browser evidence. Spines win over mockups. Pixel values below are CSS px, including the house mapping of the phone's 44 pt target to 44 px. Geometry is measured against the allocated pane, not the application viewport.

## Foundation

**The owner's priorities.** Put icons above one search box, with tags inside it; stop making tag entry unfold a second form. Let a named search return exactly as saved. Put temporary searches under All notes, make slash names a hierarchy and pins a within-group ordering, and replace the rail's hover icons with a right-click menu. The right task pane must show the selected run and its log, not duplicate task configuration; the report `61194 bytes; 83 files: 1 copied, 82 identical, 0 left alone, 0 failed` needs the width of a line, not the width left after several metadata cells.

**Grounding.** The current bar is chips/chooser/toggles above a separate `type=search` input (`src/components/notes/note-filter-bar.tsx:329-560`). Its explicit chooser is at 478–492 and its conditionally mounted clear at 515–531. The rail is a flat `spaces.map`, followed by FilePlus/Pencil/Trash2 hover targets (`src/components/notes/space-list.tsx:213-359`). Run reports occupy a `flex-1` leftover-width span (`src/components/layout/tasks-pane.tsx:1282-1325`); `src/components/layout/panel-strip.tsx` hosts task facts again. These are the replaced surfaces, not hypothetical components. The screenshot wording above is the owner's reported example; no screenshot pixels were available to measure in this planning lane.

**Explicit supersessions.** UX-DR100–102 replace UX-DR94's chooser/row geometry, UX-DR97's single-line input and conditional clear, and UX-DR99's no-second-icon-row rule where 44 px targets require wrapping. AD-268's relative order, names, target floors and no-disappearing-controls rule remain. **Coordinator resolution of AD-273:** “remove-only” binds the field's keyboard contract, not chip semantics. Chip-body click still cycles include → exclude → off, shared with the space editor; chips are not focus stops, Backspace at caret-start removes the last chip, and X removes explicitly. Suggestions add a chip with the chosen sign. UX-DR103–104 narrow UX-DR35's no-dialog premise to allow a non-modal naming popover (AD-271). UX-DR105 replaces the duplicated right-pane task rendering when a run is selected. Navigation history versus note revisions remains outside this document; Chrome's Back/Forward research does not add browser-history controls here.

> **Amendment, 2026-09-20 — owner's 0.8.31 corrections.** The historical AD-273 resolution immediately above is reversed by AD-287: only the sign flips include/exclude, only × (or the existing removal keys) removes, and the chip body is inert. AD-288 replaces the two-zone geometry with the in-field nine-control flow in UX-DR106. The dated amendment at the end of this file is authoritative wherever the historical sections differ; they remain intact to record what shipped and why the owner changed it.

**Tokens.** Use the system stacks in `DESIGN.md`: body 13 px / 20 px line box, caption 11 px / 16 px line box, section label 11 px / 16 px at weight 600, mono 12 px / 20 px, title 15 px / 20 px at weight 600. Line boxes snap to the inherited 4 px rhythm. Input radius 5 px, row/button radius 7 px, transient surface radius 10 px, tag radius 9999 px. Panes use `--background` / `--foreground`, 1 px `--border` separators and no shadow. Every suggestion, naming popup, menu and tooltip uses `--popover` / `--popover-foreground` and the existing primitive's shadow and 1 px foreground/10 ring; never literal black, an invented fill, or a new palette. AD-279 authorizes the tooltip exception to DESIGN.md's unchanged-primitives rule. Focus/menu-target rings use 2 px `--ring`. Hover/selection uses the shipped neutral role; held amber is not a new hover color, violet is not private-note paint, and recording red is not task-status paint. Existing include/exclude token colors and the destructive text treatment are reused, not recolored.

**Ownership.** Rust owns hierarchy/order/expiry, query results, availability facts and run/log facts. React owns caret spans, focus, disclosure, layout and rendering view models over IPC; it does not parse report sentences into numbers, reconstruct a rail tree from slash strings, compare expiry timestamps to delete a space, or sort combined drive results. The core/shell work belongs to `epic77-core` and `epic78-core`; surfaces belong to `epic77-surface` and `epic78-surface`; this file belongs only to `plan`. Shell changes in `src-tauri/crates/keeper/**` are by inspection, awaiting CI's macOS job.

## A. Search bar — icons above, chips and caret inside one border

> **Amended 2026-09-20, UX-DR106–107.** Icons now begin inside the field, with Add tag first and Save last; the old eight-slot band, 4+4 / 6+2 / 8 phone budgets, permanent glyph/clear lanes and inline chip-before-text flow below are historical. The new section supplies all replacement measurements. The trailing slot is stateful Reset/Clear, not unconditional text-only clear. AD-287 also supersedes every body-cycle statement in this section; token-span preservation and IME protection remain.

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

> **Amended 2026-09-20 by UX-DR106:** “two zones” and the old width table are superseded after the owner rejected the shipped separation. AD-268's order among existing controls, no-disappearance rule and target floors survive; Add tag is inserted first.

**Rule.** The fixed eight-control band precedes a single border containing scope, tag tokens and text; width changes wrapping, not availability or order.

**Mechanism.** Apply the exact target/width/height budgets above in `src/components/notes/note-filter-bar.tsx`; `src/components/notes/notes-phone-pane.tsx` uses the same bar and 44 px tier. The chooser fold is removed. Keep independent three-row chip and three-line text caps, an unconditional clear slot and the 48 px folded-rail handoff.

**Tokens.** DESIGN.md's 4 px rhythm, 5 px input / 7 px button radii, inherited input/ring roles, existing signed-token paint; no new colors.

**What is deliberately not on it:** hidden actions, a second tag-input form, an icon overflow menu, a smaller target to fake the 240 px fit, or a taller column floor.

### UX-DR101 — A token suggestion changes only its token

> **Amended 2026-09-20 by UX-DR107:** body cycling is revoked because a second click deleted the chip the owner intended to flip. Sign toggles include↔exclude; body is inert; × removes. Caret suggestion semantics remain, and the same signed choices now serve explicit Add tag and note-row tag entry.

**Rule.** A suggestion adds a signed chip without consuming the surrounding prompt; chip-body click keeps the existing three-state cycle, while field keyboard operations remove without moving focus into chips.

**Mechanism.** `src/components/notes/note-filter-bar.tsx` owns caret/listbox interaction and preserves the shared chip-body cycle; `src/components/tags/tag-match.ts` remains the matcher; `src/lib/stores/notes-filters.ts` receives the term/text operation. Six visible options, two per tag, input-owned active descendant, keyboard-remove-only tokens with explicit X, native editing/IME protection, and popup-first Escape follow the contracts above.

**Tokens.** Field-width `--popover` layer at 4 px offset, 10 px radius; 32/44 px option rows; existing neutral active-row and 2 px ring treatments.

**What is deliberately not on it:** a focusable tag grid, chip Tab stops, whole-prompt replacement, automatic tag creation, a second chip-state grammar, or imported Primer code.

### UX-DR102 — Search state speaks without moving the controls

> **Amended 2026-09-20 by UX-DR106:** the always-mounted trailing slot remains, but its action is Reset to the entered space when the query has drifted, Clear search otherwise. It is never two adjacent controls. Search availability/error truth is unchanged.

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

> **Amended 2026-09-20, UX-DR108–109.** The owner's correction removes PINNED captions, changes `Temporary spaces` to `Temporary` with an icon, makes new-space Temporary default ON, replaces Bookmark with Save, extends the context menu, and groups the rail by the selected drives. The old row geometry and expiry semantics stand except where the new section explicitly replaces them.

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

> **Amended 2026-09-20 by UX-DR108:** the single-drive projection becomes Rust-composed multi-drive groups; one selected drive has no redundant drive header, two or more do. `Temporary` gains its own icon and PINNED captions disappear; pin order and acknowledged expiry remain.

**Rule.** Temporary searches are children under All notes, slash groups disclose their contents, and pins reorder only their own group; expiry is visible without a sub-day countdown.

**Mechanism.** `src/components/notes/space-list.tsx` renders the Rust-composed hierarchy/order/counts with the exact rows and disclosure targets above; `src/components/notes/space-editor.tsx` exposes lifetime/pin facts without deriving them. `src/lib/stores/notes-filters.ts` enters a space with complete replacement, not residue from the previous search. Core space parsing/order and shell reconciler supply file-backed expiry and trash behavior (AD-269/270/272).

**Tokens.** 12 px level indent capped at 36 px, 32/44 px minimum rows, 48 px two-line rows, 11 px captions and 7 px radius; existing sidebar/neutral roles only.

**What is deliberately not on it:** global duplicated pins, pin-implies-permanent, ticking second counters, client-side expiry deletion, or an invented query for a virtual group.

### UX-DR104 — A menu marks its target and a save asks for a name

> **Amended 2026-09-20 by UX-DR108–109:** add Duplicate space / Add sub-space to the menu and default new-space Temporary ON with the house Switch. The target ring, non-modal naming, focus return and acknowledged-save rules remain.

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

> **Amended 2026-09-20 by UX-DR110:** truncated task/run identity strings use the shared bold-label/muted-detail HoverHint; run facts and logs retain their full-width, non-clamped presentation. Epic 80 repairs the ledger eligibility defect; this UX amendment does not redefine the reader or manufacture historical log content.

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

> **Historical proof only, 2026-09-20:** the arithmetic immediately above verifies the superseded epic-77 geometry, not the 0.8.31 correction. UX-DR106's new nine-slot arithmetic and the amendment's verification handoff below replace it for epic 79.

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

## Owner's 0.8.31 corrections — amendment dated 2026-09-20

This section records a second field report, against `c1b1d97` / v0.8.31, not a retroactive description of epic 77. Its authority is `local://coordinator-decisions-2.md` (AD-287…AD-296), plus the coordinator's explicit clarifications: Add tag is a ninth, first slot; a single trailing slot switches Reset/Clear by drift state; a single selected drive has no redundant group header. The first amendment was designed before the owner used it; this one responds to what the owner actually found confusing.

**Scope and ownership.** This document lands on `plan`. The current five-rung order is `plan` → `epic80-tasks-core` → `epic80-tasks-surface` → `epic79-notes-core` → `epic79-notes-surface`. React lays out controls, owns caret/focus and renders facts; Rust owns query/space/drive identity, hierarchy/order, persistence and availability. No TypeScript rail union, saved-query parser or expiry policy is introduced here. Anything in `src-tauri/crates/keeper/**` is **by inspection, awaits CI's macOS job**. This is a planning artifact, not an implemented or visually verified surface.

### A. The in-field control line — AD-288

**Replaced evidence.** `local://triage2-ScoutBar2.md`, claims 7–8 and 10, records the separate eight-slot band, 20 px glyph lane, 28/48 px trailing lane, 80/140 px chip cap and 60 px text cap in `note-filter-bar.tsx` / `search-field.tsx`. The actual source at `src/components/notes/search-field.tsx:301` renders the clear X at **16 px**, like the leading glyph; the digest's tentative 12 px inference is not the shipped measurement. This amendment makes the contrast explicit rather than relying on that inference.

#### Anatomy and full slot table

One border encloses everything. Reading order is **leading glyph → icon run → caret/text**, with the stateful trailing action at the first line's end. Scope/tag chips occupy the rows **after** the text, not the space before its caret. There is no outer icon band, no fold-out tag form and no overflow/more button.

```text
Wide, one text line:
┌───────────────────────────────────────────────────────────────┐
│ [mode] [Tag+] [Bot] [Pin] [Eye] [Sort] [Drives] [Lock] [New] [Save] text… [×/↶] │
│ [scope ×]  [+][tag][×]  [−][tag][×]                            │
└───────────────────────────────────────────────────────────────┘
Narrow, schematic only (exact row allocation is in the table):
│ [mode] [first icons…]                               [×/↶] │
│ [remaining icons…]                                      │
│ text starts at the inner padding edge, not a glyph lane  │
│ continued text starts at that same padding edge         │
│ [scope ×]  [+][tag][×]                                   │
```

The diagram is not a width measurement. Every icon slot is **24 × 24 px desktop / 44 × 44 px phone**, with **16 × 16 px** ink and **4 px** inter-slot gaps at every width. All nine slots remain mounted when disabled; no width-dependent omission, reordering or zero-gap compression.

| Slot in reading/Tab order | Glyph and accessible name | State / contract |
|---|---|---|
| Leading, not a control | Search / Sparkles / static Loader, **20 × 20 px**, `--foreground` | Decorative effective-mode/status ink; no hit target, no X silhouette, no animated pulse. Reserves 20 px + 4 px only on physical row 1. |
| 1 | Tag with plus badge, **Add tag** | Opens the shared signed vocabulary popover in B; 16 px combined ink bounding box, not a tenth action. |
| 2 | Bot, **Changed by agent** | Stable name; `aria-pressed` carries restriction. |
| 3 | Pin, **Pinned only** | Stable name; `aria-pressed` refers to notes, not space pins. |
| 4 | EyeOff / Eye, **Hide service files** | Existing persisted preference; same label, state reflected by glyph and `aria-pressed`. |
| 5 | ArrowUpDown, **Sort notes** | Menu; selected sort/direction in its description. |
| 6 | HardDrive, **Search drives** | Row-selectable multi-select in D; no badge consuming width. |
| 7 | LockKeyhole when off / LockOpen when on, **Include private notes** | Stable name and `aria-pressed`; no incognito hue. |
| 8 | FilePlus, **New note from search** | Preserve the space when creating in its drive; name the limitation when choosing another drive, as the epic's Rust-backed creation contract requires. |
| 9 | Save, **Save as space** | Replaces Bookmark; naming popover remains anchored to this slot. |
| Text | **Search notes** | Combobox/manual-selection suggestions, 13 px / 20 px text. |
| Trailing, one slot | X **12 × 12 px**, **Clear search**; or RotateCcw **14 × 14 px**, **Reset to «{space name}»** | Same fixed 24/44 px target, same location. Normal ink `--muted-foreground`, hover/focus ink `--foreground`. The leading 20 px glyph is visibly larger than either action. Never display both actions. |

Existing control names remain unchanged. Each icon's hint repeats its accessible name; disabled reasons are descriptions, not renamed controls. Tab follows the icon sequence, textbox, trailing action, then the surrounding surface; the decorative glyph and inert chip labels are not stops.

#### Width and wrap derivation

Keep bar horizontal padding **12 px desktop / 8 px phone**, vertical padding **8 px**. Field border **1 px**, radius **5 px**, padding **8 px**. Let `C` be allocated column width, `F` field border-box width and `W` full inner width:

- Desktop: `F = C − 24 px`; `W = F − 18 px = C − 42 px`.
- Phone: `F = C − 16 px`; `W = F − 18 px = C − 34 px`.
- On physical row 1 only, the leading reservation is **24 px** (20 + 4), trailing reservation **28 px desktop / 48 px phone** (target + 4). Icon/text budget `A = W − 52 px` desktop / `W − 72 px` phone.
- `k` icons cost `k × target + (k − 1) × 4 px`. Greedily lay the nine icons in DOM order using `A` on row 1 and `W` on later rows. The caret joins the last icon row only when its **remaining width after a 4 px gap is at least 96 px**. Otherwise the text begins on the next full-width row. Do not move an earlier icon after text to fake a fit.
- **Text inset is 9 px from the field border-box start** (1 px border + 8 px padding), or **21 px from the column start desktop / 17 px phone**. Every continuation text line starts there; it does not reserve 20 px for a glyph above it, 28/48 px for a clear above it, or centre itself under anything. The first text line alone may be indented by preceding icons. No horizontal text scroll.

| Allocated column | Desktop `F / W / A` | Desktop icon rows, by slots | Desktop caret start and width | Phone `F / W / A` | Phone icon rows, by slots | Phone caret start and width |
|---|---|---|---|---|---|---|
| **240 px** | 216 / 198 / 146 px | 1–5, then 6–9 (**5+4**) | Physical row 3, **198 px**; row 2 has only 86 px after icons + gap | 224 / 206 / 134 px | 1–2, 3–6, 7–9 (**2+4+3**) | Physical row 4, **206 px**; row 3 has only 62 px left |
| **320 px** | 296 / 278 / 226 px | 1–8, then 9 (**8+1**) | Row 2 after slot 9, **250 px** | 304 / 286 / 214 px | 1–4, then 5–9 (**4+5**) | Row 3, **286 px**; row 2 has only 46 px left |
| **420 px** | 396 / 378 / 326 px | 1–9 (**9**) | Row 2, **378 px**; row 1 has only 74 px left | 404 / 386 / 314 px | 1–6, then 7–9 (**6+3**) | Row 2 after slot 9, **242 px** |
| **720 px** | 696 / 678 / 626 px | 1–9 (**9**) | Row 1 after slot 9, **374 px** | 704 / 686 / 614 px | 1–9 (**9**) | Row 1 after slot 9, **182 px** |

All nine icons alone fit physical row 1 at **342 px desktop / 534 px phone**. All nine plus the 96 px minimum text start fit it at **442 px desktop / 634 px phone**. These are container widths, not viewport/media breakpoints. The old 4+4 / 6+2 / 8 phone rule is revoked: it budgeted eight targets in an external band without the leading/trailing reservations.

#### Vertical caps and reset/clear states

Icon-bearing rows are **24 px desktop / 44 px phone** tall, with **4 px** between wrapped control rows and before a standalone text start. Text line boxes are **20 px**, at most **3 visible lines / 60 px of text**; further content scrolls vertically within the text region. When line 1 shares a target row, its line box is vertically centred in that 24/44 px row; continuation starts after that row, at the fixed 9 px inset. The target row's extra 4/24 px is control allocation, not a fourth text line. Soft wrap never inserts query characters; hard breaks and IME behavior remain intact. The first-line controls never scroll away with text.

Chips begin **4 px below the last visible text line/row**; absent chips consume **0 px** and no gap. Scope is first, then ordered tag terms. Chips wrap whole tokens, with **4 px** horizontal/vertical gaps, capped at **3 rows = 80 px desktop / 140 px phone**, then independently scroll. A long label ellipsizes before a sign or remove target shrinks. No `+N filters` substitute; newly added chips scroll into view and the caret stays visible independently.

| Column | Desktop empty / one text line, no chips | Desktop at both caps | Phone empty / one text line, no chips | Phone at both caps |
|---|---|---|---|---|
| 240 px | 94 px | 218 px | 182 px | 366 px |
| 320 px | 70 px | 194 px | 134 px | 318 px |
| 420 px | 66 px | 190 px | 110 px | 294 px |
| 720 px | 42 px | 166 px | 62 px | 246 px |

These are field border-box heights, excluding the bar's **16 px** vertical padding and optional status. Max = resting height + **40 px** for two more text lines + **4 px** chip gap + **80/140 px** chip cap. A short phone viewport scrolls the enclosing pane; do not shrink targets, conceal slots or force a taller viewport floor.

| Query state | One trailing action and result |
|---|---|
| No entered space | Clear search; disabled only when text is empty, including the chips-only state. Activation clears text only and refocuses it. |
| Entered space, editable query equals its restore | Clear search, with the same text-empty rule. Clearing a nonempty restored prompt creates drift, so the slot then becomes Reset. |
| Entered space, query differs from its restore | Reset to «name», enabled even if the current text is empty. Activation reapplies the space restore once and refocuses text; slot becomes Clear again. |
| Reset/clear just activated; late search result arrives | No late result reinstates the old query. Keep the established response-generation protection. |
| Long space name | Target width stays 24/44 px; full name is in the hint/accessibility name, not a wide button caption. |

“Differs” compares the editable projection restored by `enterSpace` (scope identity, tag terms, text, query flags and sort), not a parse of the displayed sentence. Do not reset selected search drives or persisted service-file/private preferences unless the producer's restore contract actually includes them. Reset is not Open: it does not write the space, re-arm its TTL, create an auto-parked search, navigate to another drive or discard unrelated persisted settings. The existing status priority, 500 ms delayed searching sentence, no-results and semantic-unavailable states from UX-DR102 remain.

#### UX-DR106 — One field contains the run; width changes its wrapping

**Binds:** AD-268, AD-288, AD-295; Stories 79.2/79.3, `epic79-notes-surface`. **Prevents:** a resurrected outer band, missing Add tag at the floor, first-line glyph lanes indenting all later text, or two competing clear/reset controls.

**Rule.** Use the nine-slot table and the exact container budgets above; shrink neither hit targets nor the 96 px text start. The single trailing slot is Reset only while an entered space has query drift, Clear otherwise.

**Mechanism / landing files.** `src/components/notes/note-filter-bar.tsx` supplies the ordered controls, Add tag, Save and private glyph state; `src/components/notes/search-field.tsx` owns the single border, first-line layout, continuation inset and stateful trailing control; `src/lib/stores/notes-filters.ts` supplies the restore comparison/action; `src/components/notes/notes-phone-pane.tsx` consumes the 44 px tier. The 48 px folded-list rail's unfold-and-focus handoff is unchanged.

**Tokens.** 1 px border, 5 px field radius, 4 px gaps, 20 px mode ink, 16 px action ink, 12/14 px trailing ink, inherited foreground/muted/ring roles.

| What the owner asked for | What is specified where different | Reason |
|---|---|---|
| One line, icons in the field's first line | A single in-field flow, wrapping to the exact rows above; one physical line only when it fits | Nine 44 px phone targets plus the mode/clear/text cannot fit 240 px. AD-268 wins over a literally impossible single line. |
| Reset, maybe instead of × | One stateful Reset/Clear slot, not permanent Reset and not two buttons | Retains text-only clearing in a pristine query while making recovery from space drift explicit. |
| Add the tag icon back | First slot opens a signed vocabulary popover; no fold-out chooser row | Restores browse-and-pick without reviving the layout the owner rejected in epic 77. |

### B. The sign is an action; the label is not — AD-287, AD-289

**Replaced evidence.** `local://triage2-ScoutBar2.md`, claims 3/6/9, and `local://triage2-ScoutRail2.md`, claims 10–11: `src/components/notes/note-filter-bar.tsx:64-103` combines sign/label in the cycling button; `tag-suggest.tsx:60-79` uses plain text signs and one accent-only row style; row tags call the filter immediately, with hit rows hiding all tags. Those are observable contracts now revoked, not missing styling classes alone.

#### Chip anatomy and input equivalence

```text
Included: [ + ][ label ][ × ]
Excluded: [ − ][ label ][ × ]    label struck through, sign and × never struck
          flip   inert   remove

Scope:    [ Folder  Space: name ][ × ]
                 inert           remove scope
```

| Region | Desktop / phone geometry | Treatment and action |
|---|---|---|
| Sign | **24 × 24 px / 44 × 44 px**; Plus/Minus ink **14 × 14 px** | Separate button, toggles include↔exclude forever, never off. Hit rectangle includes no label pixels. Accessible action names `Exclude tag {tag}` when included, `Include tag {tag}` when excluded; current polarity in description. |
| Label | Flexible width, **4 px** inset at each side, **11 px / 16 px**, weight **500** | Plain span, no button role, click handler, pointer cursor, Tab stop or hover highlight. One line, ellipsis; full tag and state available to assistive technology and hint. |
| × | **24 × 24 px / 44 × 44 px**; ink **12 × 12 px** | Separate button, `Clear tag {tag} filter`; removes once. Never toggles the sign. |
| Whole chip | Height **24/44 px**, radius **9999 px**, max width `W` from A | Minimum reserved action width **48/88 px**, plus label insets; label yields first. No overlapping pseudo-element targets. |

**Include paint:** `--accent` background, `--accent-foreground` label/sign, no strikethrough. **Exclude paint:** existing `--destructive` at 15% for the background, `--destructive` text, **1 px** label-only line-through at destructive/60; the Minus is a drawn lucide glyph, not a font-dependent hyphen. Labels retain weight 500 in both states; sign stroke **2 px**. In this theme `--accent` is the neutral shadcn interaction surface, **not** brand green (`src/index.css:151-159`); do not substitute the held/health/incognito colors to make “plus” louder. The explicit plus/minus shape, strike and spoken state remain when color is unavailable.

**Sub-target states:** sign hover gets an inset **1 px** current-color outline and `--background`/40 wash confined to its own target; × hover gets that same wash and `--foreground` ink only in its own target. Keyboard-visible focus on either is a **2 px inset `--ring`** outline, so the chip scroller never clips it. Pressed activation keeps the highlight until release; focus/hover never mutate state. Label hover may expose detail but does not recolor the whole chip as a button. Disabled chip actions keep their rectangles, use the house disabled treatment and explain why; a disabled sign is not a removable label.

In editors and the tag tree, Tab reaches sign then ×; Enter/Space activates the focused action. An unselected tree tag has an explicit Include action to create the term; once selected it renders the split sign/label/× grammar. Tree disclosure remains distinct and does not flip polarity.

Inside the search field, retain the no-chip-Tab-stop rule and ordinary Left/Right/Home/End editing. Pointer sign/× actions retain the textbox caret. Keyboard polarity equivalence is the Add tag button or caret suggestion: choose Include/Exclude for the existing tag to update it, not create a duplicate. Backspace at collapsed caret offset 0 removes the last tag immediately; Escape closes a popup first, then clears text, then removes the last chip under the existing fallback. Both ignore composition; native range deletion remains native. These are explicit removal keys, not a hidden third sign state.

#### One signed-choice design, shared entrances

Each tag has **two successive full-row options**, Include then Exclude, not two tiny nested buttons in one listbox option. Within each row: **24 px** decorative sign cell, **8 px** gap, flexible label, **16 px** current-state Check reservation; **8 px** horizontal padding. Row height **32 px desktop / 44 px phone**, radius **7 px**. The include row uses the include chip's accent/sign vocabulary; exclude uses destructive/15, destructive Minus and label-only strikethrough. Labels are **13 px / 20 px**, weight **500**. `Include tag {tag}` / `Exclude tag {tag}` remain full accessible option names.

Hover and active-descendant selection receive a **2 px inset `--ring`** outline; retain polarity paint instead of washing every option into the same neutral state. The current query polarity also has a Check, separately from the active option ring; accepting that same polarity is idempotent. Opposite polarity replaces the term. None of these choices means remove; removal belongs to ×.

| Entrance / landing file | Popup and interaction |
|---|---|
| Caret suggestion — `src/components/notes/search-field.tsx`, `tag-suggest.tsx` | Field-width popup at bottom-start, **4 px** offset, **8 px** collision margin, **4 px** padding; up to **6 option rows / 3 pairs**, **200 px desktop / 272 px phone** including padding, then scroll. Input owns focus/active descendant; Down/Up browse, Enter accepts only the active option, Tab does not accept. Accept removes only the captured token span; typing on closes the old match. |
| Add tag — `src/components/notes/note-filter-bar.tsx`, `tag-suggest.tsx` | **280 px** wide, clamped to viewport minus **16 px**, bottom-start of Tag+ at **4 px** offset; same padding/options/palette. A **32 px desktop / 44 px phone** search input, separated from options by **4 px**, gets initial focus; empty input browses the fetched vocabulary rather than inventing tags. Same matcher and signed choices as the caret entrance. Choose once, close, return to the original search caret; no prompt text is consumed. |
| Note-row tag, including the existing overflow popup — `src/components/notes/note-row.tsx`, `tag-suggest.tsx` | Same **280 px** clamp/offset/padding, anchored to the clicked tag; only that tag's two options, **72 px desktop / 96 px phone** including padding. No extra input. Open on pointer or Enter/Space and focus the Include option; arrow keys move between the options, Enter/Space chooses, Escape closes and returns to the tag. Merely opening or dismissing never filters, selects or opens the note. |

The two requested call sites, caret suggestion and row-tag popup, therefore have **one paint and action vocabulary**, not two meanings of `+`/`−`; the restored Add tag action is a third entrance to it. The row popup may use ordinary option buttons while the caret uses active-descendant listbox options: share rendering/action data, not invalid nested interactive markup or an unnecessary text combobox.

`note-row.tsx` keeps **3** preferred visible tags, stable-partitioning tags absent from the query before already-included/excluded tags while preserving source order within each partition. Search-hit rows also expose these tags; otherwise the promised action is unreachable precisely during a search. Long tags truncate inside their **24 px desktop / 44 px phone** minimum targets; overflow tags use the same signed popup. If available width cannot contain all preferred tags at those targets, use the existing overflow route rather than shrinking the targets or crushing the excerpt to 0 px. A result repaint after acceptance must not leave keyboard focus in an unmounted row; if its originating row disappears, return focus to Search notes.

**Scope is a third treatment.** Use `--muted` background / `--foreground` text, **1 px `--border`** outline, **7 px** rounded rectangle rather than a pill, **16 px Folder** glyph and visible `Space:` prefix. Not `--secondary`: it is the same value as `--accent` in this theme and would not distinguish anything. Height stays **24/44 px**, label **11 px / 16 px**, weight **500**, **8 px** left padding and **4 px** glyph gap. Only its trailing **24/44 px** × is actionable, named `Clear scope {full name}`; there is no polarity sign or strike. Tag/flag chips do not adopt this identity treatment.

AD-292 retains identity but not a duplicated prompt sentence. The scope contains a concise space identity, never a second query-summary chip. If the stored display name is exactly the restored prompt (ignoring the auto-name's quote wrapper), render `Space` plus its existing drive/path identity in the description/hint instead of echoing the sentence beside itself; preserve the real stored name and full accessible description. Do not rename files or parse a query DSL for this presentation rule. The prompt appears only in the text region.

#### UX-DR107 — Two signed actions share one vocabulary

**Binds:** AD-287, AD-289, AD-292, AD-293; Stories 79.1/79.3/79.8, `epic79-notes-surface`. **Prevents:** accidental chip deletion, inert labels that look clickable, scope mistaken for a tag, or row tags applying a query before the person chooses a sign.

**Rule.** `[sign][label][×]` is binary flip / inert / remove. All signed suggestions reuse that paint and action contract; only the caret entrance consumes a token span.

**Mechanism / landing files.** `note-filter-bar.tsx`'s shared chip, `src/components/notes/tag-tree.tsx`, `src/components/notes/space-editor.tsx`, `src/components/sessions/session-space-editor.tsx`, `src/lib/stores/notes-filters.ts`, `tag-suggest.tsx` and `note-row.tsx` move together. `src/components/notes/notes-pane.tsx` and `note-list.tsx` carry the row choice without immediate cycling. No compatibility body-cycle path remains.

**Tokens.** Existing accent/destructive/muted families, 14 px sign, 12 px ×, separate 24/44 px targets, 2 px inset focus ring; no new colors.

| What the owner asked for | What is specified where different | Reason |
|---|---|---|
| × removes; the sign alone flips | Also retains explicit Backspace/Escape removal and keyboard signed choices | Pointer intent is honored without removing the existing keyboard path or adding a chip grid to the textbox. |
| A more obvious minus | Minus glyph, destructive family and label-only strike, not color alone | Distinguishable in both themes and without color perception. |
| Do not repeat the space sentence | Keep a distinct identity chip; suppress its prompt-shaped display label, never the restored query | Dropping scope would change the search, while silently renaming a saved space would change the owner's data. |

### C. The rail follows selected drives — AD-294, AD-295

**Replaced evidence.** `local://triage2-ScoutRail2.md`, claims 2–7: `notes-pane.tsx:628-634` binds SpaceList/TagTree/PhysicalTree to one `activeVaultId`; `space-list.tsx:219` overrides `Temporary`, `:237-241` paints PINNED, `:364-439` owns the current menu; `notes-pane.tsx:613-621` adds the standalone New note button. `src-tauri/crates/keeper-core/src/notes/rail.rs:105` creates Temporary with no icon. This is a multi-drive producer change before it is a paint change.

#### Group anatomy, one drive and four

`DESIGN-NOTES.md:209` says **“a per-vault accent hue (vault identity is the switcher's name, not a colour — the ‘no per-network theming’ rule, restated for vaults)”** is to be avoided. AD-294's tint does **not** repeal that rule: **the drive's NAME is the identity; the wash is only a grouping cue**. Four drives are not four colors.

- **One selected/effective drive:** keep the current unheaded rail; no new drive-group header, no wash, no extra gutter. The existing top-level drive identity is sufficient. Within-drive order, indentation and width do not change.
- **Two or more selected drives:** render one header per Rust-supplied drive group in producer order, then that drive's All notes → Temporary (when nonempty) → persistent tree → Uncategorized. Use the same **`--muted` header wash** and `--foreground` name for every drive. Body rows retain the existing sidebar/background paint; **no whole-group colored panel**, new color variable or index-to-hue assignment.
- **Four selected drives:** four named headers and four trees in one vertically scrolling rail, not four horizontal columns, tabs or equal-height nested scrollers. Adding the second drive adds headers, not another label gutter or a color change to existing rows. Never merge identical `Journal/Bali` names across drives.
- Header minimum height **32 px desktop / 44 px phone**, horizontal padding **8 px**, glyph **HardDrive 16 × 16 px**, glyph/name gap **8 px**, name **13 px / 20 px** at weight **600**, **1 px `--border`** bottom divider, **8 px** between drive groups. Header is a label, not a hidden drive toggle. Name ellipsizes at one line; full name/root path uses E's hint and accessible group label.
- Rail default/floor stays **240/180 px**. Descendant row inset, **12 px** per hierarchy level capped at **36 px**, **32/44 px** minimum row targets and 7 px row radii remain. Drive grouping adds **0 px** indentation to space rows. The same drive grouping and identity reaches Tags and physical folders in `tag-tree.tsx` / `physical-tree.tsx`, not just spaces.
- Loading/error is scoped to the named drive, not represented as an empty tree or stale contents from the previous selection. Keep available groups readable while a selected drive is unavailable. Empty explicit selection follows the existing current-drive fallback; do not label it “all drives.” Core supplies effective membership, errors and order; React cannot infer completeness from the rows that happened to load.

**Temporary:** label exactly `Temporary`, with **16 × 16 px Clock** ink, `--muted-foreground`, plus the separate **12 px chevron** inside the existing **24 × 32 px desktop / 44 × 44 px phone** disclosure target. Glyph/name gap **4 px**. Rust supplies the icon key `clock`, already mapped by `space-icons.ts`; React must not replace every group icon with a chevron. Count, 12 px indent under All notes, omission at 0 temporary children, acknowledged whole-day captions and expiry semantics remain.

**Pins:** remove PINNED captions and their 4 px top/bottom padding everywhere, including Temporary. Keep Rust's within-group pinned-first order, the existing **12 px decorative Pin**, and the **1 px `--border`** separator with **4 px** vertical margins only where pinned siblings are followed by unpinned siblings in that same group. No empty caption, extra pin section, global favorites duplicate or stronger font weight for a pin.

**Creation:** remove the standalone New note row/button above the rail, not just its label. All notes retains its contextual new-note action in that row's own drive; FilePlus in the search field remains New note from search, a different promise. Save as space uses Save ink. New-space naming opens with Temporary **ON**, **48 h** prefilled; switching it off makes a permanent draft without silently changing other fields. Editing an existing space preserves its actual lifetime, and Duplicate preserves the source lifetime; default-ON is not a migration of every existing space.

#### Context menu, exact order

Retain UX-DR104's **240 px** menu width (viewport minus **16 px** clamp), **4 px** padding, **32/44 px** item heights, **8 px** collision padding and **2 px** target ring independent of selection. For a real editable space:

1. Open space
2. Add to current search
3. Separator — **1 px**, **4 px** vertical margins
4. Pin space / Unpin space
5. Edit space…
6. **Duplicate space**
7. Separator — **1 px**, **4 px** vertical margins
8. New note in this space
9. **Add sub-space**
10. New space…
11. Separator — **1 px**, **4 px** vertical margins
12. Delete space…

Duplicate targets the right-clicked row's drive/id, not the selected space. It saves a new id with the leaf suffix ` copy` (`Journal/Bali copy`), same query, prompt, sort, pin and TTL configuration and the remaining existing space properties. A temporary duplicate gets its own acknowledged lifetime, not a promise to inherit a possibly expired deadline. Show pending/error in the owning action context, suppress repeat submission while pending, and reveal the returned row only on acknowledged success; selection/search does not jump. Naming collision/error is the existing save contract, not a frontend suffix counter.

Add sub-space opens the existing naming popover in that row's drive with the full `<parent>/` prefix; focus the name input with the caret after the slash. No file is made before Save. It is available for a real leaf as well as a real parent; a virtual slash group offers Expand/Collapse, separator, Add sub-space, New space… and no Duplicate/Edit/Delete. Synthetic All notes/Uncategorized offer only their meaningful open/add/new-note/new-space actions; Temporary is disclosure only, not a fake file to duplicate or delete. Broken saved queries retain meaningful repair/duplicate/delete actions; Open/Add still refuse truthfully.

#### UX-DR108 — A drive name groups the rail without becoming a palette

**Binds:** AD-291, AD-294, AD-295; Stories 79.5 (`epic79-notes-core` producer, `epic79-notes-surface` renderer), 79.6 (`epic79-notes-surface`). **Prevents:** a list searching four drives while its trees show one, hue-only identity, redundant one-drive chrome, or a menu action writing to the wrong drive.

**Rule.** One effective drive has no new header; several have named neutral-wash headers, independently identified rows and unchanged within-drive geometry. Temporary gets Clock, pins lose captions, creation lives in existing row/search actions, and Duplicate/Add sub-space bind to their invoked row.

**Mechanism / landing files.** Rust composition in `src-tauri/crates/keeper-core/src/notes/rail.rs` and its view models; multi-drive `notes_spaces` in `src-tauri/crates/keeper/src/notes_ipc.rs`, generated IPC consumed through `src/lib/ipc/client.ts`. Render in `src/components/notes/space-list.tsx`, `notes-pane.tsx`, `notes-phone-pane.tsx`, `tag-tree.tsx`, `physical-tree.tsx`; naming/default in `space-name-popover.tsx`. The implementation epic owns binding generation, not a hand-written adapter.

**Tokens.** Existing muted/sidebar/foreground/border roles, 32/44 px drive headers, 16 px HardDrive/Clock, no per-drive hue or new palette.

| What the owner asked for | What is specified where different | Reason |
|---|---|---|
| Drive tint and drive name per group | Same neutral header wash for every drive; no header or wash for one drive | Names identify drives; DESIGN-NOTES forbids per-vault hues, and a one-drive header would waste a row. |
| Remove PINNED | Remove caption, retain small pin glyph and sibling separator | Ordering remains apparent without a repeated section heading; pin is still a fact. |
| Duplicate / Add sub-space | Duplicate preserves properties under a new id; Add sub-space opens a draft, not an immediate empty file | Neither action should mutate the original or silently create a query before the name is accepted. |

### D. Toggles say what persists; drive choices are rows — AD-290

**Replaced evidence.** `local://triage2-ScoutNav2.md` §3 and `local://triage2-ScoutBar2.md` claims 1/12 inventory five raw checkbox sites and the select surfaces. The drive name is already inside a clickable label at `note-filter-bar.tsx:383-400`; the defect is absent selected-row treatment, not a claim that only its checkbox worked. House primitives are `src/components/ui/switch.tsx:19-28` and `checkbox.tsx:14-25`, not a new Toggle component.

| Meaning | House treatment and metrics | Persistence/focus contract |
|---|---|---|
| One boolean, immediate session behavior | House Switch, default **32 × 18.4 px** track, **16 px** thumb; labeled row minimum **32 px desktop / 44 px phone**, **8 px** label/control gap | `role=switch`, checked state, label **13 px / 20 px**; caption **11 px / 16 px**, `For this session` only where actually session-scoped. Switch changes immediately. Do not label persisted query settings session-only. |
| One persisted boolean setting | Same Switch/row geometry, not a second checkbox style; existing file/source indicator and caption explaining scope, e.g. `Saved for this space` or the actual device/account scope | Saving/disabled/error belongs to the setting row; no false success before acknowledgement. Draft space switches remain draft until Save, with `Saved with this space` helper. |
| Multi-select membership or affirmative form choice | House Checkbox, **16 × 16 px** box, existing **14 × 14 px** Check; **24 × 24 px desktop / 44 × 44 px phone** minimum nonoverlapping action area | Checked state, not switch-on/off metaphor. In the drives picker the whole option row is the one checkbox target, as specified below; no checkbox nested inside a second button. |
| Small mutually exclusive set | Existing button/radio-style choices, **32 px desktop / 44 px phone** minimum height, **8 px** horizontal label padding, **4 px** gaps; wrap as whole choices | Selected fill + explicit checked/pressed semantics and keyboard navigation appropriate to the chosen primitive; not several independent switches allowing impossible combinations. Dynamic/long lists remain selects. |

Switch checked/unchecked and disabled paint stay the house `--primary` / `--input` roles; keyboard focus is the existing **2 px** ring. Never infer save scope from color. Phone rows reserve a real **44 px** hit area; do not assume the primitive's expanded pseudo-element alone meets the floor or allow adjacent targets to overlap.

Named migrations: `space-editor.tsx` Pinned/Temporary and `space-name-popover.tsx` Temporary use Switch; `format-toolbar.tsx` table-header choice uses Checkbox; `note-filter-bar.tsx` drive choices use the row below. The compact icon switches in A keep their explicitly named `aria-pressed` controls instead of becoming anonymous little tracks squeezed into the icon run. The epic's select inventory decides conversions; recording surfaces remain out of scope, native empty-string sentinel contracts are not broken by a blind swap to Radix Select, and this amendment does not authorize a global new toggle primitive.

#### Drive row anatomy and states

Search drives remains **280 px** wide, viewport-minus-**16 px** clamp, **8 px** inner padding, **4 px** below the trigger. Each drive row is **48 px** minimum in both tiers: **8 px** top/bottom padding + **16 px** name line + **16 px** path line. Layout is **16 px HardDrive**, **8 px** gap, flexible two-line text, **8 px** gap, **16 px Check reservation**, with **8 px** horizontal padding and **7 px** radius. Name **13 px / 16 px**, weight **500**; path **11 px / 16 px**, `--muted-foreground`, single-line ellipsis. Full name/root path uses E's hint, not an unstyled native title.

The whole row is one checkbox control with name `{drive name}` and path description; pointer/touch anywhere in it toggles once. **Selected:** `--accent` / `--accent-foreground` fill and visible Check; **unselected:** popover background and reserved blank check lane; **hover:** accent/50 for an unselected row; **focus:** 2 px inset `--ring`, without erasing the Check. Space toggles the focused row; Tab/Shift+Tab moves among available rows and out; Escape closes and returns to Search drives. It stays open after a selection. Do not create a second focus stop on a decorative checkbox/check glyph.

At most **6 rows = 288 px** are visible before scrolling; with **16 px** popover padding the list surface is **304 px** high, clamped further by the available viewport/keyboard inset. A long name yields before either glyph. Loading, no available drives and load failure render distinct non-option sentences, with Retry only for failure. Known unavailable drives keep name/path and an explanatory disabled state rather than vanishing. The description states the effective current-drive fallback for an empty explicit set; an unchecked list must not claim “searching no drives” or “all drives.”

#### UX-DR109 — Control shape conveys choice; nearby text conveys persistence

**Binds:** AD-290, AD-291; Story 79.4 / `epic79-notes-surface`; task-form choices consume the same house rule on `epic80-tasks-surface`. **Prevents:** browser-default checkboxes beside house settings, colors that pretend to encode persistence, and a drive picker that visually acknowledges only the tiny box.

**Rule.** Switch for a single boolean, Checkbox for membership, radio-style choices for a small exclusive set. Persisted versus session-only is a truthful label/source distinction. Drive membership is a whole 48 px row with a checked fill, Check and secondary path.

**Mechanism / landing files.** Reuse `src/components/ui/switch.tsx` / `checkbox.tsx`; migrate `note-filter-bar.tsx`, `space-editor.tsx`, `space-name-popover.tsx`, `format-toolbar.tsx` and the epic's approved select inventory. Reuse rather than restyle the primitives globally.

**Tokens.** Existing primary/input/foreground/accent roles, 32 × 18.4 px track, 16 px Checkbox/glyph, 48 px drive row, 2 px ring; no new toggle theme.

| What the owner asked for | What is specified where different | Reason |
|---|---|---|
| Nicer toggles / analyze all selects | Convert by choice semantics; keep long/dynamic selects and the recording exclusion | A device list is not a switch, and many switches cannot safely express one mutually exclusive choice. |
| Click the whole drive name | Entire name/path row toggles once and shows selected fill + Check | The label already forwarded clicks; the missing feedback is now explicit and keyboard-equivalent. |
| Temporary by default | ON for a new naming draft, not a forced change to an existing space | Honors the new default without rewriting saved intent. |

### E. HoverHint has a title and a detail — AD-296

**Replaced evidence.** `local://triage2-ScoutNav2.md` §2 and `local://triage2-ScoutRail2.md` claims 9–10 locate the existing plain label/detail in `src/components/ui/tooltip.tsx:69-70`, **320 px** max width, **500 ms** delay and detail's old three-line clamp. They also distinguish existing note-row/rail hints from the absent editor-title/tasks hints. A tasks popup is new reachability, not evidence that three identical popups already existed.

**Two-line means two semantic blocks, not clipping a full path to two physical lines.**

1. Label first: **12 px / 16 px**, weight **600**, `--popover-foreground`; full title/name, wraps anywhere if necessary.
2. Detail below: **12 px / 16 px**, weight **400**, `--muted-foreground`, **4 px** gap; full vault-relative path, excerpt or task identity/detail as appropriate. Path may use the inherited mono family at the same metrics. Omit this block and the gap when absent.

Popup width is content-sized up to **320 px**, clamped to viewport width minus **16 px**, **12 px** horizontal / **6 px** vertical padding, inherited **7 px** transient radius, **1 px** foreground/10 ring and existing shadow, `--popover` background. Offset **4 px**, collision margin **8 px**; default side top, rail side right, flip to avoid clipping. Keep **500 ms** pointer dwell, open on keyboard focus without requiring pointer movement, Escape dismisses. Tooltip never takes focus and contains no buttons. Remove the generic three-line detail clamp where it would hide the very path this feature promises; wrap complete strings rather than ellipsizing them again. Excerpts remain the producer's bounded prose, not a frontend Markdown renderer.

Very long content must not trap controls or become the only way to read a fact: the full string remains in the trigger's accessible description and the underlying detail surface. At short-window/zoom sizes the tooltip is collision-constrained, not a modal or a horizontally scrolling card. Phone does not acquire hover; keep full facts accessible through the existing selected-note/task detail and accessible description without stealing the row's context-menu long press.

| Required call site | Label / detail | Landing and state coverage |
|---|---|---|
| Note-list row | Full note title / prose excerpt (search-hit excerpt where present) | `src/components/notes/note-row.tsx`: existing hint now has title hierarchy; missing excerpt is label-only; no duplicate H1 or raw wiki syntax is introduced. |
| Editor title/path | Full derived note title / full vault-relative path | `src/components/notes/note-editor.tsx`: one trigger over title/path pair, not two competing popups; path stays available when visually truncated. Unsaved/no-path shows the title without a fabricated path. Keyboard reach uses the existing title/header control or a single focusable text region if none exists; no nested button. |
| Tasks pane truncated identities | Human task name / immutable id and actual path/host/profile detail; run label / full run id as applicable | `src/components/layout/tasks-pane.tsx`: task title, host/profile and run-id truncation sites use HoverHint. AD-300's task name is first, id secondary; absent values are omitted, never fabricated. `TaskDetail` and selected run log retain full-width facts, not tooltip-only reports. |

The rail's already-mounted `HoverHint` in `space-list.tsx` inherits the same primitive rule, including the multi-drive header's full name/path detail; it is not a fourth competing design. `IconHint` remains a label-only wrapper and keeps its stable names and 500 ms dwell. No global migration of unrelated native titles is included.

#### UX-DR110 — A truncated fact opens the same two-block hint everywhere

**Binds:** AD-296; the shared primitive change in `src/components/ui/tooltip.tsx` and the tasks-pane call sites land together on **`epic80-tasks-surface`**. Story 79.7 on **`epic79-notes-surface`** adds only the remaining editor-title/note-row call sites atop that primitive; it does not re-edit the primitive for the same contract. **Prevents:** a plain first line that cannot be read as a title, a full-path popup that truncates its own path, and three incompatible tooltip palettes.

**Rule.** One HoverHint: 600-weight label, muted detail, 320 px maximum, 500 ms dwell; each named surface supplies truthful identity/detail and keyboard access.

**Mechanism / landing files.** `src/components/ui/tooltip.tsx` owns typography/geometry; `note-row.tsx`, `note-editor.tsx` and `tasks-pane.tsx` supply their facts; existing `space-list.tsx` consumers inherit it. No source-derived path guessing and no new hint primitive.

**Tokens.** Popover pair, muted detail, 12/16 px typography, 4 px block gap, 320 px maximum width, 7 px radius.

| What the owner asked for | What is specified where different | Reason |
|---|---|---|
| Bold first line, description below | Two semantic blocks, each allowed to wrap | A physical two-line cap would hide the full path requested in the same report. |
| Full path on hover | Also keyboard focus and accessible description; phone keeps detail access | Hover alone excludes keyboard/touch and cannot be the sole carrier of a fact. |
| Same popup on task strings | Add actual task consumers, not only restyle the primitive | The field digest found no task popup to inherit the change. |

### Amendment handoff and planning proof

**Executed planning proof:** two `jq` arithmetic calculations over all eight desktop/phone cases at 240/320/420/720 px produced the `F / W / A`, icon distribution, caret widths, resting/capped heights and first-line thresholds in A. This replaces the historic eight-slot calculation. It is arithmetic proof only: **no browser measurements, source implementation, builds, formatter or test suite were run by this lane**.

| Contract to prove during implementation | Landing file(s) and exact scenario | Existing evidence to retarget |
|---|---|---|
| UX-DR106 geometry/actions | `search-field.tsx`, `note-filter-bar.tsx`, `notes-phone-pane.tsx`: all four widths in both tiers; nine slots + one trailing action, actual rectangles ≥24/44 px, continuation inset 9 px, both caps; pristine-space Clear → drift Reset → restored Clear; Add tag then choose without prompt consumption | `note-filter-bar.test.tsx:210-240,279-286,346-376` from `local://triage2-ScoutBar2.md`; old band/three-state expectations are not visual proof |
| UX-DR107 sign and shared choices | `note-filter-bar.tsx`, `tag-tree.tsx`, both space editors, `note-row.tsx`, `tag-suggest.tsx`: sign pressed repeatedly never removes; label never changes query; × does; row popup cancel is inert; caret acceptance changes only its span; same tag choice idempotent; hit row exposes tag route | `note-filter-bar.test.tsx:221-240`, `notes-filters.test.ts:253`, `tag-tree.test.tsx:92-115`, `space-editor.test.tsx:145-149,301-304`, `session-space-editor.test.tsx:202`, `note-row.test.tsx` immediate-toggle fixtures; behavior not SVG/source-text pins |
| UX-DR108 drive identity and menus | `rail.rs`, `space-list.tsx`, `notes-pane.tsx`, `tag-tree.tsx`, `physical-tree.tsx`: one drive with no new header; four with named groups; duplicate names/ids in different drives; partial unavailable drive; context action on unselected drive; Temporary 0/many, pins/no pins; Duplicate acknowledged row; Add sub-space cancelled draft | `space-list.test.tsx` old Temporary/PINNED/menu queries and `space-name-popover.test.tsx:74-78` default-OFF interaction from `local://triage2-ScoutRail2.md` |
| UX-DR109 whole-row choice | `note-filter-bar.tsx`: click name/path/Check each toggles exactly once, Space works, checked fill and name/path observable, last explicit drive removal describes fallback; naming/editor/form switch saved-vs-draft semantics | `note-filter-bar.test.tsx:243-256` bare checkbox selector; existing space/form tests should assert observable state, not native input structure |
| UX-DR110 real hint reachability | `tooltip.tsx`, `note-row.tsx`, `note-editor.tsx`, `tasks-pane.tsx`: hover after 500 ms, keyboard focus, Escape, no detail, long paths, task name/id, both themes; full string readable and no report clamping | Existing tooltip/row/editor/tasks tests plus real browser; `withTextLayout` measures trigger truncation only, as `local://triage2-ScoutNav2.md` notes |

All visual gates include light/dark, 200% zoom, long unbroken names, right-to-left/keyboard reading order as supported by the existing shell, a short window and phone keyboard inset. Measure border-box widths, target rectangles, overflow and scroll behavior on the actual mounted surface; a screenshot alone proves neither sign safety nor reset correctness. Contrast must meet existing NFR-75 for informational text; token names alone are not a measurement.

**Intentional limits.** This amendment allocates UX-DR106–110 only; UX-DR111–115 are unallocated. It writes no ledgers and adds no source/tests. Epic 79 owns the configurable spaces-folder migration, new-note membership refusal and Back/Forward caret within the existing 32 px buttons; those are not an editor-navigation redesign here. Epic 80 owns ledger eligibility, tasks drive roles, form/name/path/Finder behavior. UX-DR105's historical run/log geometry is not otherwise changed.
