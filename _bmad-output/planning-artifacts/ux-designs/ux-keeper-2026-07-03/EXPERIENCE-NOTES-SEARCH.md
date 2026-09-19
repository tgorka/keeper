---
name: keeper
parent: EXPERIENCE-NOTES.md
status: final
created: 2026-09-19
updated: 2026-09-19
binds: Epic 76; AD-261…AD-268; UX-DR94…UX-DR99
sources:
  - EXPERIENCE-NOTES.md
  - DESIGN.md
  - DESIGN-NOTES.md
  - coordinator-decisions (owner ask and screenshot description; 2026-09-19)
  - G2 notes UI grounding; G1 index; G3 settings; G4 storage
  - R3 search UX research (sources accessed 2026-09-19)
---

# keeper — Experience Spine, Notes search extension (Epic 76)

> Extends `EXPERIENCE-NOTES.md`; everything not restated here keeps its existing contract. Token references `{...}` resolve in `DESIGN.md` first and `DESIGN-NOTES.md` second; inherited shadcn roles are named as such. This spine changes the note-list search and its open-note marks, not search-everywhere, the sessions matcher, or a space's `text:` semantics. AD-261…AD-268 are the epic's architecture decisions; this document defines UX-DR94…UX-DR99. Spines win over mockups. The diagrams specify hierarchy, not measured pixels.

## Foundation

**The owner's screenshot, as described.** At approximately 330 px the list shows `+ Add a tag filter`, `Changed by agent`, `Pinned only`, then `Search this vault`, then `Searching the files, not an index`, `688 notes`, and title/date/snippet/tag rows. The owner's priority is the tags, particularly when the list is unfolded. The answer is not to hide tags in a filter menu: remove repeated button wording, keep the chip states visible, and put the evidence for a search into the row and the opened note.

**What exists and what this changes.** The bar's current DOM order is scope → tag chips → chooser → origin → pinned → conditional save → inline chooser → search → posture caption (`src/components/notes/note-filter-bar.tsx:240-356`). Its own rationale calls fixed order a muscle movement rather than a search (`src/components/notes/note-filter-bar.tsx:5-9`). The new order moves the chooser to the beginning of the tag group, inserts service visibility after pinned, and keeps the outer grammar **scope → tags → toggles → save**. The search remains underneath. The count remains outside the bar in the existing count slot (`src/components/notes/notes-pane.tsx:637-641`).

The old posture caption is retired, not shortened: it is currently rendered at `src/components/notes/note-filter-bar.tsx:354-356`, while the list predicate searches index fields, not the full body (`src-tauri/crates/keeper-core/src/notes/index.rs:216-232`; `src-tauri/crates/keeper/src/notes_ipc.rs:449-452`). Epic 76 deliberately adds a disposable search index beside the model, not a second owner of the notes (AD-261). The new caption says the actual state, never an architectural slogan.

**Overrides to the old spine.** This extension replaces the bar overflow, search caption, search-mode line-2 precedence and search-mode ordering in `EXPERIENCE-NOTES.md:106-121`, and the corresponding bar/row rules in `DESIGN-NOTES.md:88-114`. It does not revive the older lens/date controls that are absent from the current bar (`src/components/notes/note-filter-bar.tsx:240-324`). Non-search rows retain their existing behavior. The old desktop-only statement in `EXPERIENCE-NOTES.md:18,26` is not a reason to omit the current phone surface: it already uses `NoteList` and `NoteEditor` (`src/components/notes/notes-phone-pane.tsx:51-52`). Phone-specific wrapping and targets are defined in UX-DR99.

**Token discipline.** `{colors.search-highlight}` is a semantic role, not permission to copy the old hex literal from `DESIGN.md:27-28`. Bind it to the shipped theme's `--search-highlight` and its paired `--search-highlight-foreground`; light and dark values already exist (`src/index.css:179-180,287-288`). Author-written `==highlight==` retains the distinct `--mark` / `--mark-foreground` pair (`src/index.css:181-187,289-293`). No new palette or type family is introduced. `{typography.caption}`, `{rounded.sm}`, `{rounded.md}`, `{rounded.full}`, and the inherited 4 px spacing scale are defined in `DESIGN.md:51-65`; `{components.note-row}`, `{components.tag-chip}`, and `{components.filter-chip-bar}` come from `DESIGN-NOTES.md:88-114` with the behavioral overrides below.

## Surfaces

| Surface | Reached from | Purpose / governing decision |
|---|---|---|
| Expanded filter band | Notes list; unfold list | Keep scope and tags legible, with stable icon controls — UX-DR94 |
| Search result row | Type into the list query | Show the passage that answered, without inventing word matches for meaning — UX-DR95 |
| Open-note search marks | Open a note from the queried list | Keep the same lexical evidence visible while reading — UX-DR96 |
| Search field and conditional status | Expanded list; folded Search action | Identify words/meaning/indexing and explain a degraded mode — UX-DR97 |
| Service visibility and count line | Bar's eye control; Notes settings for filenames | Hide routine files without implying deletion or incomplete indexing — UX-DR98 |
| Narrow list / folded rail / phone stack | Resize, fold/unfold, phone navigation | Preserve every action and accessible target at 240/320/420 px — UX-DR99 |

**Diagram legend.** `[+]` = lucide `Plus` chooser; `[B]` = `Bot`; `[P]` = `Pin`; `[H]` = `EyeOff`, hiding pressed; `[E]` = `Eye`, hiding not pressed; `[S]` = `Bookmark`, Save as space; `[Q]` = `Search`; `[*]` = `Sparkles`; `[L]` = static `Loader`; `[x]` = `X`. These are ASCII stand-ins, not proposed visible text labels. `==word==` denotes a painted match, never inserted Markdown. `...` denotes visual ellipsis. Dates, titles and counts in mocks are fixture examples, not measurements of the owner's vault. Desktop mock widths are outer list-column widths, not character counts.

## UX-DR94 — The icon bar's row grammar and fixed order

**Rule.** The bar reads scope → tags → toggles → save in one stable grammar, with the tag chooser among the chips and all three toggles staying in the same place in both states.

**Mechanism.** The top band has a flexible leading scope/tag lane and a fixed trailing action run. Within the leading lane: optional scope, then `Plus`, then the existing ordered `TagFilterChip` instances. A chip is the control and its state; it is never reduced to a colored dot or an icon. The chooser uses only `Plus`, not a second `Tag` glyph squeezed into the same target. It opens the existing inline `TagCombobox` below the band, before the search field, preserving its focus-on-open and no-create filter behavior (`src/components/notes/note-filter-bar.tsx:325-338`; `src/components/notes/tag-combobox.tsx:16-30,150-160`).

The action run is **Bot → Pin → EyeOff/Eye → Bookmark**. Reserve the save slot even when its action is absent, so the eye and pin never shift on a keystroke. Save remains conditional on the existing savable expression: tag terms, agent, pinned or nonblank text (`src/components/notes/note-filter-bar.tsx:228-230,313-323`). Service visibility alone does not create a savable filter; it is the person's viewing preference, not a new space predicate (AD-267; space `text:` stays unchanged). The icon is `Bookmark`, not `Save`: this stores a space, not the note being edited.

| Control | Lucide icon | Accessible name and tooltip, verbatim | State contract |
|---|---|---|---|
| Add tag | `Plus` | `Add a tag filter` | `aria-expanded` tracks the chooser; not `aria-pressed` |
| Origin filter | `Bot` | `Changed by agent` | `aria-pressed={agentOnly}` |
| Pin filter | `Pin` | `Pinned only` | `aria-pressed={pinnedOnly}` |
| Service visibility | `EyeOff` pressed; `Eye` unpressed | `Hide service files` in both states | `aria-pressed={hideServiceFiles}`; true means hidden |
| Save filter | `Bookmark` | `Save as space` | Ordinary action; no pressed state |
| Search input | leading glyph per UX-DR97 | `Search this vault` | Input name and placeholder stay identical |
| Clear query | `X` | `Clear search` | Action appears only for a non-empty field |

All icon buttons carry the name on the button and a decorative `aria-hidden` icon. Wrap with `IconHint` using that same name, not a competing `title` attribute. This reuses the current hint component and its 500 ms delay (`src/components/ui/tooltip.tsx:51-83`), and the existing toggle semantics at `src/components/notes/note-filter-bar.tsx:293-312`. Labels do not change on press: W3C APG explicitly requires a stable toggle label with `aria-pressed` (R3 §3; W3C, <https://www.w3.org/WAI/ARIA/apg/patterns/button/>, accessed 2026-09-19). The tooltip is descriptive, never the only way to know a state; focus/hover opens it and Escape dismisses it without activating the button (R3 §3; W3C APG Tooltip pattern, <https://www.w3.org/WAI/ARIA/apg/patterns/tooltip/>, accessed 2026-09-19).

**Tokens and geometry.** Use `{components.filter-chip-bar}`, 12 px horizontal / 8 px vertical insets, 8 px between bands and 4 px between controls, from the inherited 4 px scale. Desktop action targets are at least 24 × 24 CSS px with 16 px icons; chip cycle and dismiss targets must also reach 24 px without overlapping each other. These minimum targets implement W3C WCAG 2.2 SC 2.5.8 rather than relying on its spacing exception (R3 §3; W3C, <https://www.w3.org/WAI/WCAG22/Understanding/target-size-minimum.html>, accessed 2026-09-19). Phone uses `{spacing.touch-target-min}`; see UX-DR99.

Off = shadcn `muted-foreground` on transparent; pressed = shadcn `accent` / `accent-foreground`; focus = existing visible shadcn `ring`, not a new color. Tag include retains its `+` and accent wash; exclude retains `−`, destructive tint and strikethrough. Its accessible names remain `Tag {tag}: included. Exclude it instead.` and `Tag {tag}: excluded. Stop filtering by it.`; the separate dismiss remains `Clear tag {tag} filter`. Do **not** apply binary `aria-pressed` to this three-state control (`src/components/notes/note-filter-bar.tsx:96-160`).

**240 px — desktop, chips wrap in the leading lane; action run never wraps**

```text
+------------------------------+
| [+]        [B][P][H][S]       |
| [+work x]                    |
| [-draft x]                   |
| [Q Search this vault      ]  |
+------------------------------+
```

**420 px — desktop, the same grammar has room to read across**

```text
+------------------------------------------------------+
| [+] [+work x] [-draft x]            [B][P][H][S]       |
| [Q Search this vault                              ]  |
+------------------------------------------------------+
```

| State / branch | What is shown and what happens |
|---|---|
| Scope = all | No scope chip; chooser is first in the leading lane |
| Space/folder scope | Scope chip before chooser; long label ellipsizes without losing its dismiss target or full accessible name |
| No tag terms | Chooser remains; no empty chip placeholder |
| Include → exclude → off | Existing three-state cycle, AND semantics and in-place ordering; off removes only that chip (`src/lib/stores/notes-filters.ts:127-176`) |
| Chooser open, vocabulary loading | Inline chooser keeps focus and an explicit loading state; no false empty vocabulary |
| Vocabulary empty or query has no tag | Existing no-match/refusal sentence; no create-tag action (`src/components/notes/tag-combobox.tsx:36-38`) |
| Chooser load fails | Inline error in chooser; existing chips/query remain; dismiss returns to `Plus` |
| Agent/pin off or on | Same control, same slot, same name; pressed paint and ARIA change |
| Save eligible / not eligible | Bookmark action / empty reserved trailing slot; never a disabled mystery icon |
| Many or long tags | Chips wrap as units; long text ellipsizes, sign and dismiss stay visible; no hidden active terms behind a “more filters” button |
| Toggle/filter excludes open note | Its row disappears, the open note stays open; filtering is not navigation (`src/components/notes/note-filter-bar.tsx:17-18`) |
| No vault / no notes capability | No dead filter bar; existing no-vault or capability surface owns the space |

**What is deliberately not on it, and must not be added:** visible text labels on the three toggles; a second row of toggles; a filter dialog; an AND/OR switch; a new tag-state grammar; a model chooser inside the bar; a note-save button; animated chip entry or toggle motion. Tags win by retaining words, signs and editable state while secondary controls become compact. Carbon's guidance supports truncating a long tag label rather than wrapping its text (R3 §4; IBM Carbon, <https://carbondesignsystem.com/components/tag/usage/>, accessed 2026-09-19); this spine deliberately wraps **whole chips**, not text within a chip.

## UX-DR95 — Marked matches in rows

**Rule.** In search mode a row shows the matched passage with real `<mark>` runs, or an unmarked passage explicitly labelled `matched by meaning`, and never presents a score as evidence.

**Mechanism.** A nonblank list query activates search mode. Render AD-266's `row.hit.snippet` in the existing line-2 excerpt slot, replacing the plain snippet/provenance branch only in that mode (`src/components/notes/note-row.tsx:315-318`). `hit.marks` contains half-open UTF-16 ranges into **that snippet**. React renders escaped text segments interleaved with `<mark>` elements; it does not inject HTML, re-match terms in JavaScript, or slice body offsets into the excerpt. Rust owns matching, folding, excerpt selection and offsets (AD-263/AD-266).

Line 1 retains title, unread weight, pin/conflict glyph and date; its manually assigned order number is absent in search mode because it no longer explains the sort (`src/components/notes/note-row.tsx:273-313`). The number is not replaced by a relevance score or ordinal. Search order is Rust score order, one best chunk per note; pinned/conflicted state does not reorder the answer (AD-265/AD-266). Clearing the query restores the existing shelf order and order number. Row height remains `{components.note-row}` = 64 px (`src/components/notes/note-row.tsx:234-247`; `DESIGN-NOTES.md:88-94`).

For `why=meaning`, reserve the full literal label `matched by meaning` on line 2, in `{typography.caption}`, before the unmarked excerpt. No term is marked merely because it is similar. For `why=words` or `both`, render the lexical marks; do not add a redundant hybrid badge. An unread hit shows its search evidence rather than replacing it with origin; origin stays in the accessible description and hover detail, while the unread dot/title and conflict treatment remain visible. This is an explicit search-only override, not deletion of provenance.

Retain tag pills and their existing click-to-filter contract (`src/components/notes/note-row.tsx:319-333`). The inline pill budget is **up to three**, not a demand to squeeze three long strings next to the evidence. At narrow widths use fewer inline pills, including zero, with all omitted tags available through the existing `+N` overflow interaction (`src/components/notes/note-row.tsx:336-377`). A semantic label never truncates to make room for a pill. Hover detail uses the search excerpt in search mode rather than the unrelated ordinary excerpt; the present whole-row hint is at `src/components/notes/note-row.tsx:389-391`. Full text must also be available by focus and opening the note, not only hover.

**Match count decision.** Do not add a per-hit count in this epic. It may be omitted and is omitted deliberately: AD-266 supplies snippet ranges, not a whole-note occurrence total, so `marks.length` would be a misleading document count. Craft documents a per-document count with preview (R3 §1; Craft Help, <https://support.craft.do/en/organize-and-find/search>, accessed 2026-09-19), but that precedent does not justify manufacturing the missing fact. If a later contract supplies a complete count, its optional small `1 match` / `N matches` caption must never displace the evidence; it is not part of this implementation.

**Tokens.** `<mark>` uses `{colors.search-highlight}` and the shipped paired search foreground, with ordinary row font weight: bold continues to mean unread, not matched. The same pair applies over selected/hovered rows. Other snippet text uses shadcn `muted-foreground`; tag pills use `{components.tag-chip}` and `{rounded.full}`. No row-height growth for badges.

**240 px — two 64 px row examples; tag overflow yields to evidence**

```text
+------------------------------+
| o Project plan          2h   |
| ...==budget==...       [+4]  |
|                              |
| o Annual review         1d   |
| matched by meaning     [+2]  |
|                              |
+------------------------------+
```

**420 px — evidence and tags can coexist**

```text
+------------------------------------------------------+
| o Project plan                                  2h   |
| ...the ==budget== was approved...  [work][plan][+2]   |
|                                                      |
| o Annual review                                 1d   |
| matched by meaning  Financial outlook...      [work]  |
|                                                      |
+------------------------------------------------------+
```

| State / branch | What is shown and what happens |
|---|---|
| Query empty or whitespace-only | Existing snippet/provenance branch, ordinary order number and ordering; no search marks |
| Lexical hit | Rust-selected excerpt with marked runs; clipping may shorten context but must leave matched evidence visible |
| Both lexical and vector hit | Same marked lexical evidence; no score and no “both” decoration |
| Vector-only hit | Full `matched by meaning` label, zero `<mark>` runs, excerpt in remaining width or full detail on focus/open |
| Lexical evidence only in title/metadata | No fabricated body marks; Rust's snippet identifies the matched source text; opening may correctly produce zero body ranges |
| Unread search hit | Evidence in line 2; provenance remains accessible and in detail, unread dot/title remain |
| Pinned/conflicted search hit | Existing state glyph/edge; no promotion over the ranking |
| Several matching chunks in one note | One row for the best chunk, not repeated notes |
| Unsupported/invalid/out-of-revision ranges | No guessed marks; plain escaped excerpt while fresh matching evidence is obtained |
| Long title or excerpt / long tags | Title/context ellipsis; signs/glyphs and semantic label remain; pills move into existing overflow |
| Query changed, old result response arrives | Old generation is discarded; neither its rows nor its marks replace the current answer |
| No matching notes | `No matches in this vault.` with Clear search; active chips remain; no low-confidence padding |
| Search unavailable | Explicit search failure/status, not an empty-result claim; ordinary note opening remains possible |

**Why this evidence.** Match-anchored excerpts are visible in Omnisearch's implementation (R3 §1; scambier/Omnisearch, <https://raw.githubusercontent.com/scambier/obsidian-omnisearch/master/src/tools/text-processing.ts>, accessed 2026-09-19). Smart Connections explicitly distinguishes semantic similarity from exact query text (R3 §2; Brian Petro/Smart Connections, <https://raw.githubusercontent.com/brianpetro/obsidian-smart-connections/master/README.md>, accessed 2026-09-19). `matched by meaning` is keeper's wording, not a claim that another product uses that phrase.

**What is deliberately not on it:** a relevance bar; per-hit score number; probability; ranking ordinal; fake lexical marks for a semantic hit; a second result row for another chunk of the same note; mandatory occurrence counts; automatic scrolling or animated resorting; a third line added to every row.

## UX-DR96 — Marks in the open note

**Rule.** A note opened from the queried list keeps Rust's lexical match ranges painted for that query until the query is cleared, the note changes, or Escape dismisses those marks, without taking over the editor's own find state.

**Mechanism.** The open action carries the vault/note/query context, not just a search term injected into the find bar. AD-266's `notes_note_marks(vault_id, note_id, query)` returns `{rev, ranges}` with half-open UTF-16 positions into the body. A persistent `searchMarks` StateField paints them; it is a sibling of, not a reuse of, the fading `flashExternal` mechanism (`src/components/notes/editor/live-preview.ts:1182-1225`). The current body/frontmatter split is exact and separate (`src-tauri/crates/keeper/src/notes_ipc.rs:172-180`): these positions never count a YAML block removed before the editor received the text.

Accept ranges only for the current vault, note, query generation and body revision. Clear old decorations immediately when any of those identities becomes invalid; late responses cannot restore them. While a matching request is in flight the body stays readable and editable. Local typing or an external revision invalidates the old evidence; do not paint old offsets over different words. Reapply only when Rust can return ranges for the current acknowledged body revision. An unsaved buffer can therefore temporarily have no list-search marks; never use a second frontend matcher to hide that distinction.

**Lifetime.** Keeping the list query non-empty is necessary but not sufficient: this must be the note opened **from that list query**. Opening by a link, palette, history or other navigation does not silently inherit the list's marks. Query edits while that list-opened note remains active clear old marks and request new ones. A row disappearing because a tag or the service toggle changed does not navigate the editor. Folding the list does not clear its query or the opened note's marks: ownership must survive the bar's unmount. Dismissing with Escape suppresses reapplication for the current opening/query generation; an unrelated index refresh must not repaint what the person dismissed. Changing the query or explicitly opening a result again creates a new generation.

**Find-bar precedence.** The existing ⌘F find is independent (`src/components/notes/editor/find-panel.tsx:422-455`; `src/components/notes/note-editor.tsx:597-606`), and the notes pane already yields the shortcut when the editor handled it (`src/components/notes/notes-pane.tsx:335-360`). Do not call its external-query hook to implement this feature. Escape is handled by the innermost active surface: autocomplete/tooltip first; an open ⌘F find panel next, using its existing dismissal; then the list-search marks; then the editor's existing source-reveal/focus escape chain. One press has one effect. Closing ⌘F does not also dismiss list-search marks; clearing the list query does not erase ⌘F's search string or decorations. When their ranges overlap, find's active-match treatment is visually uppermost so the next/previous target remains identifiable; list marks do not double-darken that range. Native selection/caret remain legible above both.

**Tokens.** Persistent list marks use `{colors.search-highlight}` and the paired search foreground, identical to rows. `--mark` stays reserved for authored highlights; an authored mark is restored unchanged when search marks clear (`src/index.css:179-187,287-293`). `flashExternal` keeps its own transient external-write meaning. Use `{typography.prose}` and the existing `{spacing.note-measure}`; no additional panel chrome. VS Code's two find-decoration roles and warning against obscuring underlying decorations inform the overlap rule, not a promise about keeper's current renderer (R3 §1; Microsoft VS Code, <https://code.visualstudio.com/api/references/theme-color>, accessed 2026-09-19).

**240 px — the body can wrap; marked positions do not change meaning**

```text
+------------------------------+
| Project plan                 |
|                              |
| We approved the ==budget==   |
| for the autumn workshop.     |
| The ==budget== includes      |
| venue and travel costs.      |
+------------------------------+
```

**420 px — same ranges, wider reading measure**

```text
+------------------------------------------------------+
| Project plan                                         |
|                                                      |
| We approved the ==budget== for the autumn workshop.   |
| The ==budget== includes venue and travel costs.       |
|                                                      |
+------------------------------------------------------+
```

| State / branch | What is shown and what happens |
|---|---|
| Open lexical/both result; matching revision | Persistent marks on Rust ranges; normal selection and editing |
| Open meaning-only result | No invented body highlights; the row already explains the semantic match |
| Matched source is title/properties, not body | Empty body ranges are valid; no fallback highlight in unrelated prose |
| Open not originating from list query | No list-search marks, even if the list still contains text |
| Query changes while same result note remains | Clear old marks, request current-query ranges; new response must match generation |
| Query cleared, including whitespace-only | Remove all list-search marks immediately; keep ⌘F find untouched |
| Note/vault changes, including same note id in another vault | Clear old marks and pending ownership before showing the new body |
| Escape with popup or tooltip open | Dismiss that surface only |
| Escape with ⌘F find panel open | Find panel consumes the press; list-search marks remain |
| Escape with only list-search marks active | Remove them, keep note/query/caret; suppress late reapplication for that generation |
| Find and list-search ranges overlap | Find's current-match treatment wins at overlap; other list marks remain |
| Local edit, external write, stale revision, out-of-order response | Never apply stale ranges; body remains editable; refresh evidence only for the matching revision |
| Note is gone or failed to open | Existing missing/error surface; no decorations against an absent body |
| List folds/unfolds; row filtered out | Query/opening context survives; no incidental mark dismissal |
| Authored `==mark==`, Unicode/emoji or combining characters | Authored marks unchanged; Rust UTF-16 ranges preserve complete intended characters, no frontend folding |

**What is deliberately not on it:** a second find bar; a persistent “search mode” ribbon; a new close button; automatic scrolling on each query or indexing update; a flash-then-fade entrance; changes to note contents; vector-colored sentences; using `flashExternal`'s timeout as search lifetime. R3 did not establish exact persistent-highlight lifetimes for Bear, Apple Notes or Craft; this is an explicit keeper interaction decision, not a copied undocumented behavior.

## UX-DR97 — The search field's state glyph and the conditional status line

**Rule.** The search field always identifies whether it can search words, words plus meaning, or is indexing, and a single status line appears only for an actionable or unfinished state.

**Mechanism.** Keep the `Search this vault` input, place one leading state glyph **inside** its frame and a trailing `X` clear button when the field has text. Reserve the trailing slot so text never jumps when the clear appears. Use exactly three primary glyphs: static `Loader` for indexing; `Search` for words only; `Sparkles` for words + meaning. `Brain` is not used. The glyph is not a mode switch or a focus stop. Its meaning and progress are included in an associated description; do not change the input's accessible name. Hover/focus on the field may expose the same full state description without creating a nested button.

The status slot below the field replaces `NOTES_SEARCH_POSTURE`; it is absent, including its gap, when its sentence is empty. It can say `Indexing n/N notes` or `Indexing meaning n/N chunks`, with actual units and values from Rust, or the link **`Meaning is off — choose an embedding model in Settings`**. The link opens the existing Settings surface at the embedding-model setting (AD-264); it does not choose or download a model. Existing Settings has a shared body and a Notes section insertion point (`src/components/settings/settings-dialog.tsx:131,233-237`). No model configured means words search remains fully usable. A missing/unreachable/unsupported configured provider is a different sentence from “choose a model”, not a silent downgrade.

At one-line widths, keep `Settings` visible at the trailing end of the link and ellipsize the explanatory span; the link's accessible name and focus/hover detail retain the full exact sentence. At adequate width it reads unabridged. Progress is compacted first to `Indexing n/N` while its accessible description retains the unit. A very large number pair remains available in full detail; do not wrap the bar into multiple status captions.

**Status precedence.** A current blocking search failure outranks indexing, indexing outranks the meaning-off setup link, and a ready healthy words+meaning mode has no status caption. Error text names its actual cause and recovery action; no raw credential, URL token or note content is echoed. Other simultaneous states remain in the input's full description. Glyph precedence stays indexing → effective words+meaning → words only; errors are expressed as sentences, not a fourth inscrutable glyph. An embedding backfill may therefore show `Loader` while lexical results remain usable. The glyph must describe effective availability, not merely the existence of a selected model.

An in-flight ordinary query does not change the glyph to indexing: searching and building an index are different facts. Publish current-query lexical results without waiting for vectors; ignore old-query responses. Keep input focus and the selected note stable on a later hybrid result update, with no animated resort. Avoid a status flash on each keystroke; a stalled request may use the same line with `Searching…` only after 500 ms. This threshold is a design choice informed by Algolia's debounce-plus-300-ms stall guidance, not a measured keeper latency (R3 §6; Algolia, <https://www.algolia.com/doc/ui-libraries/autocomplete/guides/debouncing-sources/>, accessed 2026-09-19). Index progress is not subject to that query-stall delay. A polite live description announces state transitions, not every tick.

**Tokens.** Input `{rounded.sm}`, shadcn `input` / `border` / `ring`; normal state glyph shadcn `muted-foreground`; status `{typography.caption}`. Settings link uses the existing link/focus treatment, not a new AI accent. Search glyph and clear each have a reserved lane, 24 px minimum desktop clear target and 44 px phone target. No spinner animation, pulsing loader, shimmer or progress skeleton; `Loader` is a static state sign plus numeric progress.

**240 px — indexing, and the alternate no-model line**

```text
+------------------------------+
| [L budget               x]   |
| Indexing 42/688              |
+------------------------------+
| [Q budget               x]   |
| Meaning is off... Settings   |
+------------------------------+
```

**420 px — ready hybrid has no caption; no-model does**

```text
+------------------------------------------------------+
| [* budget                                         x] |
+------------------------------------------------------+
| [Q budget                                         x] |
| Meaning is off — choose an embedding model in Settings|
+------------------------------------------------------+
```

| State / branch | Leading glyph | Status and behavior |
|---|---|---|
| Lexical indexing with known count | `Loader` | `Indexing n/N notes`; partial results labelled by this progress, never a complete-empty claim |
| Indexing total not yet known | `Loader` | `Indexing n notes`; no fabricated denominator or percentage |
| Embedding backfill | `Loader` | `Indexing meaning n/N chunks`; words search remains usable |
| No embedding model | `Search` | Exact meaning-off Settings link, including while query empty |
| Model ready, vectors usable | `Sparkles` | No caption; associated description says `Words + meaning` |
| Model chosen but no vectors usable yet | `Loader` while backfilling; otherwise `Search` | Honest progress or refusal; never imply hybrid readiness |
| Provider offline / unsupported / missing model / failed embedding | `Search` unless indexing still genuinely active | Reasoned meaning-unavailable sentence with Settings action; lexical results still render |
| Index/search failure | Effective mode glyph | Explicit failure and recovery action; not `No matches` and not a perpetual loader |
| Ordinary query pending <500 ms | Effective mode glyph | No new transient caption; no stale-response overwrite |
| Ordinary query stalled ≥500 ms | Effective mode glyph | `Searching…` in same slot unless higher-priority failure/progress owns it |
| Query text empty | Mode glyph remains; no trailing clear | Setup/progress/error caption remains if applicable; normal browse list |
| Query text non-empty | Mode glyph + `X` | Clear activates without blur; keeps scope/tags/toggles |
| Escape in non-empty field | Same glyph | Clear query first; next Escape follows existing chip/focus walk (`src/components/notes/note-filter-bar.tsx:232-244`) |
| Results empty and indexing complete | Effective mode glyph | Result-area empty state, not a second caption under the field |

**What is deliberately not on it:** `Searching the files, not an index`; BM25/cosine jargon; a search-submit button; a third engine-picker row; an always-visible “Ready” caption; a downloads/progress dialog; animated indexing; a semantic score slider. Words search is not disabled until the person enables meaning.

## UX-DR98 — The service-files toggle and the N notes · M hidden count line

**Rule.** Service files are hidden by default through one persistent eye toggle, while the count states exactly how many matching notes are visible and how many this toggle withheld.

**Mechanism.** `Hide service files` is `EyeOff` with `aria-pressed=true` by default; press changes it to `Eye`, `aria-pressed=false`, and the same accessible name. It is the third toggle after Bot and Pin, not a menu setting or a chip that appears somewhere else to undo it. The default filename list is `index.md`, `agents.md`, `claude.md`, `log.md`; matching is case-insensitive on the basename in any folder. No glob, folder-exclusion or content classification is implied (AD-267).

The name list is the global preference `notes.service_file_names`; the toggle's last choice is global session-state `notes.hide_service_files`. Persist through the settings/registry seam, not a component default or a whole-vault write. The key classifications and existing string persistence mechanism are at `src-tauri/crates/keeper-core/src/config/keys.rs:60-79` and `src-tauri/crates/keeper-core/src/registry.rs:208-233`. Existing agent/pinned filters are frontend state, not evidence that these toggles already persist (`src/lib/stores/notes-filters.ts:197-198,257`). Do not claim otherwise.

Names are editable in the Notes area of the existing Settings body, not a new dialog in this bar; global controls must not disappear behind the active-vault-only form (`src/components/notes/capture-settings.tsx:70-105`; `src/components/settings/settings-dialog.tsx:233-237`). Configuration edits update the current list and its count. An empty configured name list means hide nothing. Hidden is **not** unindexed, deleted, excluded from sync, or forbidden to open by a link. Search-everywhere and space-text grammar remain outside this epic's changes.

**Count arithmetic and exact format.** Rust supplies `total` and `matched` for the visible selection as today, plus the hidden count. Let M be the number of notes matching the **same current scope, text query and other filters** that the service toggle alone excludes, counted before paging or a space cap. With hiding off, M is zero. Never count all service files in the vault if only some match this query; never count vectors/chunks as notes. Never compute any of these values from mounted rows.

Render `countLabel(total, NOTES, { of: matched })`, then ` · ${M.toLocaleString()} hidden` **only if M > 0**. Thus the ordinary format is exactly **`684 notes · 4 hidden`**. The count stays in `NOTES_COUNT_SLOT`, outside `NoteFilterBar` (`src/components/notes/notes-pane.tsx:637-641`); phone already has the same slot and count helper (`src/components/notes/notes-phone-pane.tsx:316-323`). Existing count grammar is not replaced:

- `1 note`; `0 notes`; `2 notes`.
- A cap only adds `of` when `matched > total`: `20 of 684 notes · 4 hidden`.
- In the capped form the noun agrees with the number immediately before it: `1 of 4 notes`, not `1 of 4 note`.
- A genuine floor uses `atLeast` and always plural: `1+ notes`, not `1+ note`. Do not pass that flag merely because UI rows are paged.
- Number grouping is locale-aware through `toLocaleString`. `hidden` is invariant: `1 note · 1 hidden`, `0 notes · 4 hidden`, never `1 hiddens`.
- An unknown count is not zero. Keep its loading/unavailable state until Rust supplies it.

These grammar rules come from `src/lib/count-label.ts:51,85-123`. The appended hidden clause is this epic's decision, **not** a documented industry convention: R3 §5 found no product source for this exact count line. Raycast's default hidden-file exclusion and explicit toggle are a useful reversibility precedent, not evidence for our count format (Raycast Manual, <https://manual.raycast.com/file-search>, accessed 2026-09-19).

**Tokens.** Eye toggle uses UX-DR94's pressed paint, `{rounded.md}` and focus ring. Count uses `{typography.caption}`, shadcn `muted-foreground` and existing count-slot border/insets. The count's hidden clause is not a warning color: this is an intentional filter. Avoid a new badge adjacent to the icon that competes with the tag chips.

**240 px — hidden by default, same slot when revealed**

```text
+------------------------------+
| [+]        [B][P][H][S]       |
| [Q Search this vault      ]  |
| 684 notes · 4 hidden         |
+------------------------------+
| [+]        [B][P][E][S]       |
| [Q Search this vault      ]  |
| 688 notes                    |
+------------------------------+
```

**420 px — a capped space distinguishes the cap from hiding**

```text
+------------------------------------------------------+
| [Weekly x] [+] [+work x]            [B][P][H][S]       |
| [Q Search this vault                              ]  |
| 20 of 684 notes · 4 hidden                           |
+------------------------------------------------------+
```

| State / branch | What is shown and what happens |
|---|---|
| First use; default names | EyeOff pressed, matching service files withheld, hidden clause only if any were withheld |
| Hiding off | Eye unpressed; service files are eligible for the same query; no `0 hidden` clause |
| Hiding on, zero service matches | EyeOff pressed; plain visible count; not a broken toggle |
| Only service files match | `0 notes · M hidden`; result-area action `Show service files` sets the same toggle false; no dead-end “no notes” claim |
| Ordinary and service files match | Visible count + hidden clause, both for this query/filter scope |
| Space cap applies | `N of K notes · M hidden`; cap and service omission are not conflated |
| Service names edited / empty list | Re-evaluate using effective list; empty list hides zero; no hardcoded fallback over a deliberately empty preference |
| Open note becomes hidden | Keep editor open; row removed only; existing note link remains valid |
| Clear search / Clear filters | Clear query/filter state, not the persisted service-visibility preference |
| Relaunch / switch vault | Restore the global last eye state before claiming count completeness; names remain global |
| Toggle persistence fails | Revert to acknowledged state and show a save-failure sentence; never display a persisted-looking success |
| Names are controlled by a file layer | Settings uses existing file-controlled indication; the bar uses the effective list (`src/components/settings/config-source-section.tsx:87-90`) |
| Count pending / query response obsolete | No invented zero, no old-query hidden total paired with new-query results |

**What is deliberately not on it:** a second hide switch in the expanded bar; text labels on the eye toggle; a hidden-files submenu; downranking as a third state; deleting or unindexing service files; auto-hiding every dotfile; a sync-exclusion control; `0 hidden` boilerplate; saving the eye preference into a space's query.

## UX-DR99 — Narrow-width behavior at 240/320/420 px

**Rule.** Width changes only wrapping and available context, never the reading-order contract, action availability, target floor or query state, and folding replaces the whole column with its existing 48 px rail rather than squeezing the bar into it.

**Mechanism: desktop.** Use the actual allocated column width, not a viewport breakpoint. The list is 320 px by default with a 240 px floor; the notes rail is 240 px by default with a 180 px floor (`src/lib/column-widths.ts:123-128`). `columnStyle` uses flex basis plus minimum width rather than a hard width (`src/lib/column-widths.ts:195-213`). This spine neither raises those floors nor requires the editor to absorb overflow.

At all three desktop widths reserve a 108 px trailing run: four 24 px slots plus three 4 px gaps. The bar has 24 px total horizontal inset and a 4 px gap between lanes. Remaining leading scope/tag width = `column width − 24 − 108 − 4`: **104 / 184 / 284 px** at 240 / 320 / 420. The `Plus` target sits in that lane with the tag chips. The action run stays top-aligned and fixed even when scope and tags need several lines. Long chip text ellipsizes within its own width budget; its sign, cycle target, dismiss target and full accessible name survive. The scope is first and may occupy a line by itself. The tag lane grows vertically; it is never truncated to “+N filters”. With enough chips to exhaust the available pane height, that lane scrolls vertically within the available filter-band budget while the fixed action run, search, status, count and at least one list row remain reachable. Focus scrolls a clipped chip into view. No second row of toggles is created.

The search frame is `min-width: 0` between its reserved glyph/clear slots; a long query scrolls inside the input. Only status explanation text may ellipsize, with the action word and full accessible description retained (UX-DR97). The count line normally stays one line; for unusually large locale-formatted cap/hidden totals it may wrap **between clauses**, never through a number or by dropping `hidden`. A count is information worth a second line; a second row of toggles is not.

**Mechanism: phone.** Preserve `{spacing.touch-target-min}` = 44 px (`DESIGN.md:74`) rather than shrinking phone controls to desktop's 24 px. R3 cites Apple HIG's 44 pt button hit region (Apple, <https://developer.apple.com/design/human-interface-guidelines/buttons>, accessed 2026-09-19); this repo's concrete CSS mapping is the inherited token, not a claim that pt and CSS px are universally interchangeable.

At 240 px use 8 px left/right insets and zero gaps between the five 44 px icon targets: **`8 + 5 × 44 + 8 = 236 px ≤ 240 px`**. The visual top action line contains Plus and the four trailing slots; scope, when present, is a full-width leading line and the tag chips wrap full-width below the action line. This is an explicit **visual-wrap exception**, approved for the phone: DOM/accessibility reading order still remains scope → chooser/tag chips → Bot → Pin → eye → save. No CSS `order` changes the speech/keyboard sequence; the visible placement uses named layout areas. Focus indicators make traversal to/from the wrapped chips apparent. At 320 and 420 px retain the same phone structure with breathing space between the chooser and trailing group; do not switch to a different control vocabulary.

Phone chip cycle and dismiss hit regions are also 44 px and non-overlapping; the chip pill may keep its compact visual ink but consumes its actual target area. Phone search height/clear target stays 44 px. Input text, glyph meanings, marks, settings behavior and counts are otherwise the same. The current phone **does not mount `NoteFilterBar`**: it hand-rolls a `VaultSwitcher` plus search Input (`src/components/notes/notes-phone-pane.tsx:279-299`). Implement this contract there by using the shared bar, retaining the phone's vault/navigation controls, rather than assuming a desktop-only edit reaches it. Its existing pulsing scan strip (`src/components/notes/notes-phone-pane.tsx:301-309`) is replaced by the same static UX-DR97 status; do not show two indexing indicators.

**Mechanism: folded.** The column's fold strip is exactly 48 px (`src/components/layout/fold-strip.tsx:141`). The list rail retains Search → Note list/count → conditional Clear filters (`src/components/notes/notes-pane.tsx:466-504`). Search unfolds and focuses the field, preserving its query; Note list unfolds without inventing navigation. The count detail adds the same hidden clause; the compact badge is only the visible count, never a guessed sum. Clear filters clears ordinary filters/query, not the persisted eye preference. The full bar, chips and input are absent in the folded body, not microscopically rendered. Unfold restores remembered width and state. The separate notes-navigation rail retains Vaults (`NotebookPen`), New note (`FilePlus`), Spaces (`Layers`), Tags (`Tags`), Files (`Folder`) and its existing section-opening behavior (`src/components/notes/notes-pane.tsx:428-454`); it is not replaced by the new list-filter icons.

**Tokens.** `{components.filter-chip-bar}`, `{components.tag-chip}`, `{components.note-row}`, `{typography.caption}`, `{rounded.md}`, `{spacing.touch-target-min}`; inherited 4 px scale for desktop, 8 px phone inset, and the existing 48 px fold-strip constant. The result row keeps its 64 px height; tags move into overflow before search evidence loses its entire line. At widths below the desktop floor the shell's existing stack/fold policy owns the transition, not an undocumented smaller bar.

**240 px — desktop, phone, and the distinct folded rail**

```text
DESKTOP                        PHONE (44 px targets)
+----------------------------+ +----------------------------+
| [+]      [B][P][H][S]       | | [+] [B] [P] [H] [S]        |
| [+work x]                  | | [+work     x]              |
| [-draft x]                 | | [-draft    x]              |
| [Q budget              x]  | | [Q budget              x]  |
| 8 notes · 1 hidden         | | 8 notes · 1 hidden         |
+----------------------------+ +----------------------------+

FOLDED LIST (48 px; not a 240 px mini-bar)
+------+
| open |
| [Q]  |
| list |
|  8   |
| [FX] |  Clear filters, only when applicable
+------+
```

**420 px — desktop retains inline chips; phone retains larger targets**

```text
DESKTOP
+------------------------------------------------------+
| [+] [+work x] [-draft x]            [B][P][H][S]       |
| [* budget                                         x] |
| 8 notes · 1 hidden                                   |
+------------------------------------------------------+
PHONE
+------------------------------------------------------+
| [+]                        [B] [P] [H] [S]            |
| [+work       x] [-draft       x]                      |
| [* budget                                         x] |
| 8 notes · 1 hidden                                   |
+------------------------------------------------------+
```

| State / width | What is shown and what wraps |
|---|---|
| Desktop 240 px | 104 px scope/tag lane; fixed 108 px action run; whole chips wrap, icon controls do not |
| Desktop 320 px | 184 px scope/tag lane; same fixed action run; fewer chip lines, no feature changes |
| Desktop 420 px | 284 px scope/tag lane; same run; more visible context/tag pills, not extra controls |
| Phone 240 px | 236 px total icon-row budget; 44 px targets; chips full-width below; scope above when present |
| Phone 320 / 420 px | Same phone layout and reading order, more space between groups; no lost toggle or abbreviated accessible name |
| Many chips / very short window | Leading chip lane gets bounded vertical scrolling; search/actions/count and a usable list region remain reachable |
| One extremely long scope/tag | Text ellipsis and full focus/hover description; sign/removal target cannot shrink |
| Long query or status | Query scrolls inside its input; status explanation ellipsizes, Settings action survives |
| Large capped/hidden counts | Count wraps at clause boundaries if necessary; no truncation of numeric meaning |
| Fold list with query/tags active | Existing 48 px list rail with Search, count/detail and Clear filters; state retained |
| Unfold through Search | Remembered width restored; caret lands in the existing query, no filter reset |
| Fold/unfold navigation rail | Its five existing actions stay; no service-toggle duplication there |
| Empty list or no results | Full bar remains where a vault exists; recovery actions fit without displacing toggles |
| Keyboard, touch, assistive technology | Same names and semantic order; touch never requires tooltip discovery; focus never clips outside a scroll region |
| Dark/light, hover/selected/focus, reduced motion | Existing paired tokens remain legible; states change as cuts in every motion preference |

**What is deliberately not on it:** dropped icons; a second row of toggles; a “more” menu that hides active filters; horizontal page overflow; shrinking targets to claim a fit; a raised column minimum; a redesigned phone navigation system; a compressed filter bar on the 48 px rail; animation, relevance bars or per-hit scores.

## Verification contract for implementation

This is a planning artifact, not a rendered implementation or a claim that the proposed layout has already passed a browser measurement. No code, builds, tests or formatters were run for this document. The arithmetic above is a design budget; implementation must prove the actual surface with the existing real-App probe entry (`dev/probe/main.tsx:1-56`) at **240, 320 and 420 px allocated list widths**, plus the 48 px folded rail and the phone branch.

The proof must exercise: zero/many/long include/exclude chips; every toggle both ways; eligible/ineligible save without moving adjacent icons; query clear and Escape; indexing/words/meaning/error statuses; all-hidden and capped counts; lexical/both/meaning-only rows including unread/conflict and long tags; opening a result, editing/query changing, note changing, late ranges, Escape dismissal and overlapping ⌘F; light/dark and keyboard/touch focus. Measure target rectangles, overflow, fixed 64 px rows, chip scrolling and restoration on unfold. A screenshot alone does not prove the chooser, Settings link or persisted eye state can be used. No product-behavior verification is claimed until that implementation exists.

*Citation spot-check note:* G2/G5 line numbers had drifted: current width definitions are `src/lib/column-widths.ts:123-128` and `columnStyle` is `:195-213`; row height is `src/components/notes/note-row.tsx:234-247`; count grammar is `src/lib/count-label.ts:104-123`. Those current ranges were read while authoring this spine.

## UX-DR → story mapping

| UX decision | Story key(s) | Binding |
|---|---|---|
| UX-DR94 — The icon bar's row grammar and fixed order | `76-5-a-bar-that-speaks-in-icons` | AD-268; service control consumes AD-267 |
| UX-DR95 — Marked matches in rows | `76-2-a-list-that-ranks-and-marks-what-matched`; `76-6-meaning-from-the-model-you-already-have` | AD-263, AD-265, AD-266 |
| UX-DR96 — Marks in the open note | `76-3-the-open-note-marks-the-same-words` | AD-263, AD-266 |
| UX-DR97 — The search field's state glyph and the conditional status line | `76-1-a-search-index-beside-the-model` (progress producer); `76-5-a-bar-that-speaks-in-icons` (surface); `76-6-meaning-from-the-model-you-already-have` (meaning states) | AD-261, AD-264, AD-268 |
| UX-DR98 — The service-files toggle and the N notes · M hidden count line | `76-4-service-files-hidden-by-default` | AD-267 |
| UX-DR99 — Narrow-width behavior at 240/320/420 px | `76-5-a-bar-that-speaks-in-icons`; `76-2-a-list-that-ranks-and-marks-what-matched` (row overflow); `76-4-service-files-hidden-by-default` (count) | AD-266, AD-267, AD-268 |
