# Spec 44.5 — A Note Has an Order You Can See

status: implemented
created: 2026-08-09
epic: 44 (the vocabulary is the space, and the note is a document)
binds: FR-159, AD-81
depends on: 44.3 (default spaces), 44.12 (row truncation conventions)
consumed by: 44.4 (`sort: order` calls this module's comparator)

## What this story settles

`order` is a property of the **note**. It lives in the note's own frontmatter, it
travels with the file through a clone, a sync or an Obsidian edit, and Obsidian
shows it in the property list. Which key a space sorts by is a lens the viewer
chose and is Story 44.4's; that story's `order` case calls this module's
comparator rather than deciding again what a note's position means.

## The two decisions this story was asked to make and justify

### The default is the constant `0`, and it is never written to a file

Every note needs an order or a list is half-ordered — some rows placed and the
rest wherever they landed, which is the randomness this story exists to remove.
Two candidates:

| Candidate | What it buys | What it costs |
| --- | --- | --- |
| A timestamp (`created` or `modified`) | Every note distinct, so nothing ever ties | `order` becomes a second copy of a date and drifts from it the moment `created` is edited. It makes an unordered list *look* ordered: a column of thirteen-digit numbers is exactly "an ordering the reader cannot account for", with extra digits. And it means stamping an `order` into ten thousand notes keeper did not author, which FR-121 forbids. |
| A constant | Nothing invented; an absent key is honestly an absent position | Every un-ordered note ties, so the tiebreak carries the whole list |

**Chosen: the constant `0`, ascending, negatives allowed.** The cost is paid
once, openly, by a stated tiebreak — rather than paid silently by whatever
sequence the entries happened to arrive in. CSS `order` is the same design for
the same reason, so `order: -1` meaning "before the un-ordered majority" is a
convention a reader already has.

The default is **implicit**. Absent frontmatter *means* 0; keeper does not write
`order: 0` to make it so. `notes_set_order(…, null)` therefore **removes** the
key rather than zeroing it: `order: 0` present in a file claims the user
deliberately placed this note where every silent note already is, and that is a
different statement from silence.

### The value is `f64`, not an integer

`order: 1.5` is what a person types to slot a note between 1 and 2, and it is
what a future drag-to-reorder writes so that moving one note does not rewrite the
frontmatter of every note after it. An integer field would read `1.5` and `1.2` as
the same `1` — **a tie invented by the type rather than by the vault**, which is
the precise failure this story is about. Comparison is `f64::total_cmp`, so the
order is total even if a NaN ever reaches it.

### What a tie does

`cmp_order` is three terms, ascending:

1. **`order`**, by `total_cmp` — the fact the user set.
2. **Folded title**, by `search::fold_cmp` — the same folding rule search and a
   space's `name` sort use. This is the tiebreak a reader can account for: with a
   constant default most notes tie, and "then alphabetically" is a sentence the
   list's own contents demonstrate.
3. **Vault-relative path**, unique by construction, which makes the order total.

Term 3 is not belt-and-braces. `notes_vault`'s reconciler holds its entries in a
`HashMap`, so the sequence handed to a sort is in hash order and differs between
launches. A comparator returning `Equal` for two distinct notes would let that
reshuffle through into the list and the same vault would present a different
order every time the app opened. **Nothing in the comparator may depend on input
position — not even as a stable-sort fallback.**

`cmp_order_desc` reverses the `order` value only; the alphabet and the path stay
ascending, matching the other four sort keys and `notes_ipc::list_order`'s
existing shape.

## I/O matrix

### `order::read_order(&Frontmatter) -> NoteOrder`

| Frontmatter | `value` | `source` | Why |
| --- | --- | --- | --- |
| no `order` key | `0.0` | `default` | The story's first requirement: no note is unplaced |
| no frontmatter block at all | `0.0` | `default` | Same |
| `order: 3` | `3.0` | `own` | The normal case |
| `order: 1.5` / `order: 1.2` | `1.5` / `1.2` | `own` | Distinct, and must stay distinct |
| `order: -1` | `-1.0` | `own` | Before the un-ordered majority |
| `order: "7"` | `7.0` | `own` | A quoted number is a hand-edit or a template, not a mistake, and the value is unambiguous |
| `order:` (empty) | `0.0` | `default` | What Obsidian writes when a property is cleared: an absent value, not a broken one. Matches `Frontmatter::as_list`'s treatment of an empty scalar |
| `order: soon` | `0.0` | `unreadable` | Still totally ordered, and the surface says the file and the list disagree |
| `order: [1, 2]` | `0.0` | `unreadable` | Same |
| `order: 3rd` | `0.0` | `unreadable` | Same |
| key present, value outside the parser's subset | `0.0` | `unreadable` | Checked via `Frontmatter::keys`, because `get` returns `None` for both "no key" and "no value I understand" and those are different answers |
| `order: null` | `0.0` | `default` | `Frontmatter` spells an explicit null as the empty string, so it reads exactly like a cleared property |
| `order: NaN` / `order: inf` | `0.0` | `unreadable` | `f64::from_str` accepts both; a position is neither. The frontmatter scanner keeps them out of `Num`, and the string branch must not let them back in — a NaN order is the one value `total_cmp` can place but nobody can explain |

### `order::set_order_in(source, f64) -> String` / `clear_order_in(source) -> String`

| Source | Call | Result |
| --- | --- | --- |
| `---\ntitle: A note\r\n# a comment\r\ntags:\r\n  - one\r\norder: 2\r\n---\r\nBody.\r\n` | `set(5.0)` | Only `2` → `5`. Comment, CRLF endings, key order, block list and body byte-identical |
| same | `set(2.5)` | `order: 2.5`, still a YAML number |
| `# Title\n\nBody.\n` (no block) | `set(1.0)` | `---\norder: 1\n---\n# Title\n\nBody.\n` — body's first line unshifted |
| `---\ntitle: A\norder: 4\n---\nBody.\n` | `clear()` | `---\ntitle: A\n---\nBody.\n`, and `set(4.0)` on the result restores the original bytes exactly |
| non-finite `f64` | `set(NaN)` | Cannot arrive over IPC (JSON has no `NaN`/`Infinity`); if it ever did, `FieldValue::Num`'s renderer quotes it so the document stays parseable. No failure mode of its own |

### The row (`note-row.tsx`)

| `order.source` | Drawn | `data-order-source` | Accessible name suffix |
| --- | --- | --- | --- |
| `own` | `3`, `1.5`, `-1` — unrounded | `own` | `order 3` |
| `default` | the value, dimmed | `default` | `order 0, the default` |
| `unreadable` | the value + `?` | `unreadable` | `order 0, the default; this note's own order is not a number` |

The `?` exists because the destructive tint is not a carrier on a monochrome
panel (UX-DR43). The order is in the row's `aria-label` because `aria-label`
overrides the element's contents for name computation — a number only painted is
a number a screen-reader user never receives, and then the ordering is exactly as
unaccountable for them as it was for everyone before this story.

## Edge cases and how each is answered

| Case | Answer |
| --- | --- |
| A note has never had an order | The implicit default, `0`, `source: default`. Nothing is written to the file |
| `order: 0` written deliberately | `source: own`. Same number, different fact; the row distinguishes them |
| Two notes tie on order and on folded title (`Ábc` vs `abc`) | Path decides. Asserted both directions plus reflexivity |
| Entries arrive in a different sequence between launches | Irrelevant by construction: the comparator is total. Asserted over **all 720 permutations** of a fixture whose titles, paths and placements all disagree |
| A space sorted by `order` where nobody has placed anything | One tie class, so the list reads as alphabetical-by-title. Intended, and named in `cmp_order`'s doc so 44.4's UI does not describe it as something else |
| An old `index.json` on disk, written before this field existed | `INDEX_SCHEMA` 1 → 2. A declared discard-and-cold-scan rather than a silent deserialisation failure that happens to take the same branch |
| `field:order=3` in a space query | Unaffected. The raw text still lands in `IndexEntry.fields["order"]`; the new field is the *typed* copy |
| Pinned notes inside a space | Not this module's business. `cmp_order` never looks at `pinned` |
| Sorting cost | `order` is parsed once per note in `parse_note`, not per comparison. Sorting 10 000 notes is ~130 000 comparisons; a comparator that re-read `fields["order"]` would be 130 000 string parses. Likewise `search::fold_cmp` was added so the title term allocates nothing — `fold_str(a).cmp(&fold_str(b))` would be ~260 000 throwaway `String`s for the same answer |

## What changed

| File | Change |
| --- | --- |
| `keeper-core/src/notes/order.rs` | **New.** `NOTE_ORDER_KEY`, `DEFAULT_NOTE_ORDER`, `NoteOrder`, `NoteOrderSource`, `read_order`, `cmp_order`, `cmp_order_desc`, `set_order_in`, `clear_order_in` |
| `keeper-core/src/notes/mod.rs` | Registers the module |
| `keeper-core/src/notes/search.rs` | `fold_cmp` + the `FoldChars` iterator; `fold_str` is now that same walk, so there is still exactly one folding rule |
| `keeper-core/src/notes/index.rs` | `IndexEntry.order`; `INDEX_SCHEMA` 1 → 2 |
| `keeper-core/src/notes/vm.rs` | `NoteRowVm.order` |
| `keeper/src/notes_vault.rs` | `parse_note` reads the order once |
| `keeper/src/notes_ipc.rs` | `row_of` passes it straight through; new `notes_set_order` command |
| `keeper/src/lib.rs` | Registers the command |
| `src/lib/ipc/client.ts` | `notesSetOrder`; re-exports the two generated types |
| `src/components/notes/note-row.tsx` | The order cell, `formatNoteOrder`, `noteOrderLabel`, `NOTE_ORDER_UNREADABLE_MARK` |
| `src/lib/ipc/gen/NoteOrder.ts`, `NoteOrderSource.ts` | Generated by the ts-rs export step; not hand-written |
| fixtures in `note-list.test.tsx`, `notes-pane.test.tsx`, `stores/notes-list.test.ts` | Gained `order`, because a row without one is a state the shell cannot produce |

`note-list.tsx` was **not** modified: the order is a cell inside the row, and the
list container belongs to 44.10.

## Proof: every guarantee was reverted and the failure watched

Each mutation was applied alone and reverted afterwards.

### `keeper-core` — `cargo test -p keeper-core --lib notes::order`

| Mutation | Test that caught it |
| --- | --- |
| `cmp_order` loses the `path` term | `a_folded_title_tie_still_resolves_and_only_by_path` **and** `the_order_is_total_and_independent_of_the_sequence_it_arrives_in` — the second one is the important half: with the term gone the comparator returns `Equal` for the two same-titled notes and the stable sort leaks input position, so different permutations produce different lists |
| `cmp_order` loses the folded-title term | `the_order_is_total_and_independent_of_the_sequence_it_arrives_in` |
| an unreadable `order` returns a plain `default` instead of `unreadable` | `an_order_that_is_not_a_number_falls_back_visibly_rather_than_silently` |
| `DEFAULT_NOTE_ORDER` becomes `1.0` | `a_note_that_never_had_an_order_takes_the_default` |
| `set_order_in` re-serialises the whole block instead of splicing one key | `writing_an_order_changes_that_key_and_no_other_byte` (the comment, the CRLFs and the key order all move) |

The permutation test was **strengthened during this exercise**: its first version
had six distinct titles, so dropping the `path` term left the comparator total
*for that fixture* and the mutation survived it. Two notes in the tie class now
share a title up to case, which is what makes every term load-bearing.

### Frontend — `bun run test note-list.test.tsx note-row.test.tsx`

| Mutation | Tests that caught it |
| --- | --- |
| `formatNoteOrder` drops the `?`, leaving colour as the only carrier | `carries the mark in text rather than in colour alone`; `falls back visibly when the note's own order is not a number`; `renders the orders in the sequence Rust sent` |
| the order is drawn but left out of the row's `aria-label` | `draws the default …, and says it is the default`; `distinguishes an order the note stated from the default it was given`; `falls back visibly …` |
| only a note that stated an order gets one drawn | `shows an order on every row, never on only the placed ones` + 3 others |
| the row rounds the value to an integer | `draws the note's own order, including a fraction and a negative, unrounded` |

Baselines after restore: `notes::order` 11/11, `notes::search` 11/11,
`notes::index` 31/31; frontend 20/20 across the two files. The whole-module run
`notes::` reports 296 passed / 3 failed, and all three failures are
`notes::sort::tests::*` — Story 44.4's module, in flight in the same worktree,
reported to its owner.

## What could NOT be proved on this host — say it plainly

* **The `keeper` shell crate does not build on this Linux box.** `cargo check -p
  keeper --lib` fails in `glib-sys`' build script (`pkg-config` is absent), so it
  never reaches type-checking. The edits to `notes_vault.rs` (`parse_note` reading
  the order), `notes_ipc.rs` (`row_of`, `notes_set_order`) and `lib.rs` (the
  command registration) are therefore **unverified by a compiler here**. They were
  written against the immediately adjacent code — `notes_set_order` is
  `notes_set_flag`'s body with the value swapped — but that is an argument, not a
  build. They must be compiled and exercised on the macOS host.
* **Nothing here proves a real reorder round-trips through a real vault.** The
  frontmatter splice is proved over `&str` in `keeper-core`; the path from a click
  to `write_note` to the watcher to a re-parsed `IndexEntry` is shell code and is
  in the bullet above. Specifically unproven: that `write_note` commits the change
  and that the reconciler republishes a row whose `order` reflects it.
* **Nothing here proves Obsidian shows the value.** `order` is a plain
  unnamespaced YAML scalar in the note's own block, which is the whole reason for
  choosing it, but that is a claim about Obsidian's property editor and this
  worktree contains no Obsidian.
* **The list's "does not re-sort" assertion is a fixture, not a code mutation.**
  Proving it by inserting a sort into `note-list.tsx` was declined because 44.10
  is editing that file concurrently and a write-then-restore could clobber it.
  Instead the test feeds the same rows in reverse and asserts the reversed output:
  a component that sorted internally would emit ascending for both inputs and fail
  the second assertion. That is the same proof, expressed in data.
* **No visual check.** The dim/normal/destructive tints and the `tabular-nums`
  alignment of the order column were not seen in a browser. jsdom has no fonts, so
  whether a column of orders actually aligns beside the timestamp at the pane's
  minimum width is unmeasured here.

## Deliberately NOT done

* **No drag-to-reorder.** The story asks for an order that is visible and that
  persists; a drag surface is a separate interaction with its own keyboard story.
  `f64` was chosen partly so that adding one later does not require renumbering,
  and `notes_set_order` is the write path it will use.
* **No `order` in the space editor, and no rail ordering.** A space's own order and
  its sort are 44.4's, and this story deliberately owns only the note's side.
* **No sorting by order anywhere.** `cmp_order` exists and is exported; nothing in
  this story calls it. `sort: order` is 44.4's case and it calls this comparator —
  one definition of "a note's position", by agreement over `hub`.
* **No `order` stamped into existing notes, and no migration writing one.** The
  default is implicit precisely so keeper never touches a file to establish it.
* **No numeric bounds and no uniqueness rule.** Two notes may both say `order: 3`;
  that is a tie, and a tie has a stated answer. Refusing duplicates would mean
  keeper rewriting somebody's frontmatter to enforce an invariant nobody asked for.
* **No sort indicator or "sorted by" header on the list.** That is a property of
  the space and belongs with 44.4/44.11.
* **No count of placed vs unplaced notes.** Counts are 44.11.
