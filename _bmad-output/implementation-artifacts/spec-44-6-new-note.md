# Story 44.6 — New Note

status: implemented
epic: 44 — The vocabulary is the space, and the note is a document
binds: FR-160, UX-DR58
depends on: 44.3 (default spaces), 44.4 (a space's stored settings)
feeds: 44.7 (templates), 44.8 (update notes from their template)

---

## 0. What was already there

The epic warns that three stories in waves 1 and 2 discovered the thing they were
asked to add already existed as a value nobody applied. This story is the fourth,
and the largest of them.

**A creation path existed, complete, and was already wired to six surfaces.**

| Surface | Entry point | State before this story |
|---|---|---|
| Command palette `notes-new` | `src/components/command-palette/actions.ts:99` | worked |
| `⌘⌥N` | `src/hooks/use-notes-shortcut.ts:77` | worked |
| Native menu bar | `keeper_core::palette::registry_sections` | worked |
| Tray → New Note | `notes_ipc::tray_new_note` | **created the note and never opened it** — see below |
| Quick capture | `notes_ipc::commit_buffer` | worked |
| Empty-vault CTA | `notes-pane.tsx` `onEmptyAction` | worked |
| **The notes pane itself** | — | **absent** |
| **A space row** | — | **absent** |

**One row of that table was wrong when this spec was first written, and finding
out why is the most useful thing in this story.** The tray's New Note (and
Today's Journal) create the note, raise the main window, and emit
`keeper://notes-open-note` carrying the new ref. Nothing in the webview
subscribed: `listenNotesOpenNote` was declared in `client.ts` and **called from
nowhere**. So the tray created a note, brought the window to the front, and
showed the user whatever had already been on screen. Two of its three siblings,
`listenNotesShowUnread` and `listenNotesCaptureShown`, are still in that state
(DW-172).

Nothing failed, and that is the point. This is the variant W3TemplateUpdate
named while 44.8 was landing, and it is worse than the armed kind: **a dead
value can be a promise the app has been making and cannot keep, and there is no
error to find.** FR-102 says the tray exists so the app window is optional for a
whole day; it has not been able to deliver that since Epic 36.

Fixed here, because "New Note" is this story's subject and shipping a spec that
claimed the tray worked would have been worse than the bug: `useNotesOpenNote`
(`src/hooks/use-notes-open-note.ts`), mounted at the app root beside
`useNotifyNavigate` — not inside the notes view, since the whole point is that
the event arrives while another view is on screen.

`notes_create(vault_id, NoteCreateReq { title, body, template, dest, tags })`
existed at `notes_ipc.rs:1415`. `NoteCreateReq.template` was honoured, the vault's
`default_template` was the fallback, and a template that could not be read already
degraded to a plain note and already said so at `INFO`. `createNote()` already
selected the created note, and the editor already focused itself and put the caret
in the body.

So **44.6 is wiring, not building** — with two genuinely new pieces: space
inheritance, and a channel for telling the user something about a note that was
created anyway. Everything else in this story is a control that did not exist on
the one surface where a person would look for it.

Reported on `hub` before implementation started, as the epic asks.

**And a fifth dead value, found late.** The epic's tally was a space's `sort`,
`keeper.limit`, the recording note's tags, and this story's whole creation path.
There is one more, and it was hiding inside the path itself:
**`NoteCreateReq.dest` was a field every caller passed `None` for** — the tray,
quick capture, the wikilink stub, the palette, `blank_note()`, the tests, all of
them. It has existed since Epic 37 and has never once named a folder. The space
seed is the first thing in this app to set it, which is what turned a dormant
overwrite path into a reachable one and made §11's backstop necessary. A dead
field is not harmless; it is an untested code path waiting for its first caller.

---

## 1. What this story adds

1. A **New note** control at the head of the scope column: a note in the vault.
2. A **`+`** on every space row: a note *that space will list*.
3. **Space inheritance.** A note created in a space is given the tags, folder and
   flags that space's own query needs, derived in `keeper-core` from the DSL.
4. **A verdict.** After the note is written, the space's query is run over the
   bytes that were written. If it does not select them, the note still exists and
   one finished sentence says it will not appear there.
5. **`notices`** on the create's result — the shared channel 44.7's
   missing-template sentence also travels on.

The palette, `⌘⌥N`, the tray and the menu bar are unchanged and inherit the new
return shape for free: they create with no space, which is the rail's behaviour.

---

## 2. The design position

### The seed is a best effort. The evaluator is the authority.

Two functions, in the order the shell calls them:

```
keeper_core::notes::seed::inherit(query) -> Seed { tags, dest, pinned, archived, capture }
keeper_core::notes::seed::verdict(space_name, query, &IndexEntry, body, now_ms) -> Option<String>
```

`inherit` walks the query's terms and says what to give the note. It is a guess:
the DSL can ask for facts no creation can produce, and can ask for two folders at
once.

`verdict` is not a guess. The shell writes the note, re-indexes **the bytes it just
wrote** through the reconciler's own parser
(`keeper::notes_vault::index_written`, new, a thin wrapper over the existing
private `parse_note`), and runs `query::eval` over the resulting `IndexEntry`.

This is the whole design. Consequences, each of which is a defect that cannot
happen:

- **No false success.** A seed that was not enough cannot report that the note
  will appear, because the query is what decides and the query was run.
- **No false failure.** A term the seed never touched but creation happened to
  satisfy — `date:created>=-7d`, `origin:local` — is satisfied, and nobody is told
  otherwise. A per-flag "can we satisfy this?" table would have refused both.
- **One evaluator.** There is no second reading of the DSL anywhere. Not in
  TypeScript (AD-58), and not in a second Rust function either.
- **One indexer.** The answer given at creation time is produced by the same
  `parse_note` that will produce the answer a second later when the reconciler
  catches up. "keeper said it would appear here and it did not" is
  unrepresentable.

### `NoteCreateReq.space` is an id, never a query

The frontend sends the space's note id and nothing else. Rust reads that space
note **once** and takes three things off it — name, query text, default template
(`SpaceForCreate`) — so a note cannot be seeded from one version of the space and
templated from another. No surface outside Rust ever learns what `is:pinned`
means.

### A note is created even when the space will not list it

Losing a thought over an unsatisfiable saved view is the wrong trade, and it is the
same trade `template_source` already refuses for a missing template. The note goes
to the vault root (or to whatever folder the seed did manage to name) and the user
is told, in one sentence, naming the terms that defeated it.

### The decline is visible at INFO, not only in the return value

`tracing::info!`, never `debug!`. Nothing sets `RUST_LOG` in the packaged app, so a
`debug!` is a decision nobody can see on the machine that made it (DW-162). Every
path in this story that declines to act logs at INFO: the unsatisfiable space, a
space id that names nothing in the index, and a note that could not be re-read to
check.

### `conjunction` beside `decompose`, not inside it

`query::decompose` is **all-or-nothing** on purpose: a chip bar that saved three
terms of a four-term query would silently delete the fourth. Creation cannot use
that rule — a space filtered `tag:work date:created>=-7d` has one term creation
acts on and one it does not need to, and refusing the pair wholesale would leave
the new note untagged for no reason.

So `query::conjunction(input) -> Option<Vec<Term>>` was added beside it. The two
share the tokenizer and nothing else, which is the property that matters: there is
still one grammar and one definition of a token. `Term.source` is the term's
verbatim text, which is what a refusal is worded from — a term reported as anything
but what its author typed sends them looking for a query they never wrote.

---

## 3. What the seed does with each term

Derived from a **flat conjunction only**. A query carrying `|`, a group or a
dangling `-` seeds nothing: a term lifted out of a disjunction is not a term the
whole query requires, and filing a note for a condition the query never insisted on
is worse than filing it plainly.

| Term | Seeded | Why |
|---|---|---|
| `tag:x` | tag `x`, normalised by `tags::normalise` | The one definition of a tag. `tag:#Client/Acme` gives `client/acme`. |
| `tag:x/*` | nothing | Descendants *without* the node itself. No single tag satisfies it. |
| `-tag:x` | nothing | Already true of a note with no tags. |
| `is:pinned` | `pinned: true` | Ordinary frontmatter Obsidian shows as a property. |
| `is:archived` | `archived: true` | Same. |
| `is:capture` | `keeper.capture: true` | The reserved namespace's one documented sub-key. |
| `is:journal` | `dest = journal` | `notes_vault::parse_note` computes the flag from that prefix. |
| `is:untagged` | nothing | Already true. Seeding it would have to *remove* a tag another term asked for. |
| `is:template` | tag `template` | A template is a note **tagged** `template` (AD-82), so making one is one tag, never a folder keeper owns. Added once 44.7 landed; see §7. |
| `is:space` | nothing | `spaces/` is the one folder that decides a file's **kind**. A note with no query there is a broken space row nobody asked for. |
| `is:recording` / `conflict` / `orphan` / `unparsed` / `oversize` / `unstable_identity` | nothing | Facts another subsystem produces. Fabricating one is lying about the vault. |
| `is:unread` | nothing | A per-device mark about somebody else's write. A note you just typed is read. |
| `path:<glob>` | the literal directory prefix | See below. |
| `field:` / `date:` / `origin:` / `text:` / `link:` / `backlink:` | nothing | Not tags, a folder or a flag — the three things the story names. `date:` and `origin:` usually turn out satisfied anyway, which `verdict` discovers. |
| anything, negated | nothing | There is no such thing as writing "not in this folder" into a file. |

**`path:` and the dropped last segment.** The last segment is always dropped: it is
the filename pattern, and a note whose filename comes from its first line cannot
promise to match it. `journal/**` → `journal`; `journal/2026/*.md` →
`journal/2026`; `notes/inbox` → `notes`; `*.md` → nothing. Everything up to the
first segment carrying a glob character is taken. `.`, `..` and a leading `/` end
the prefix rather than being walked, so a destination assembled out of a saved query
can never name anything above the vault (AD-65, FR-145).

**Two folders.** The first named wins. Picking the second would make the outcome
depend on term order in a grammar whose terms are otherwise commutative. The note
lands in the first and the sentence names the second.

---

## 4. I/O matrix

`create` below means: press New note, from the rail or from a space row.

| Input | Seed | On disk | Told |
|---|---|---|---|
| Rail, any vault | — | `2026-08-09-untitled.md` at the vault root | nothing |
| Palette `notes-new`, `⌘⌥N`, tray, menu bar | — | same | nothing |
| Space `Inbox` (`is:untagged`) | nothing | root, no tags | nothing |
| Space `Journal` (`is:journal`) | `dest=journal` | `journal/2026-08-09-untitled.md` | nothing |
| Space `Pinned` (`is:pinned`) | `pinned` | root, `pinned: true` | nothing |
| Space `Recordings` (`is:recording`) | nothing | root | "A new note can't satisfy is:recording, so this note is in the vault but won't appear in Recordings." |
| `tag:client/acme tag:billable` | both tags | root, `tags: [client/acme, billable]` | nothing |
| `tag:#Client/Acme` | `client/acme` | one tag, folded | nothing |
| `tag:work -tag:draft` | `work` | one tag | nothing |
| `tag:client/*` | nothing | root | names `tag:client/*` |
| `tag:work date:created>=-7d` | `work` | one tag | nothing — the note was created now |
| `date:created<=2020-01-01` | nothing | root | names `date:created<=2020-01-01` |
| `origin:local` | nothing | root | nothing — an uncommitted note reads as local |
| `origin:agent` | nothing | root | names `origin:agent` |
| `text:agenda`, empty note | nothing | root | names `text:agenda` |
| `text:agenda`, template body contains it | nothing | root | nothing — the body is read |
| `is:capture` | `capture` | `keeper.capture: true` | nothing |
| `is:archived` | `archived` | `archived: true` | nothing |
| `is:space` | nothing | root, **not** in `spaces/` | names `is:space` |
| `path:journal/** path:archive/**` | `dest=journal` | `journal/…` | names `path:archive/**` |
| `tag:work is:untagged` (contradiction) | `work` | one tag | names `is:untagged` |
| `tag:a \| tag:b` (structure) | nothing | root | "A new note can't satisfy Either's query, so this note is in the vault but won't appear there." |
| `tag:work \|` (does not parse) | nothing | root | "Broken's query can't be read, so it selects nothing. This note is in the vault, but it won't appear there." |
| Space id naming nothing in the index | nothing | root | nothing shown; `INFO` says the space is gone |
| Space's `keeper.template` names a missing file | as the query says | root, no scaffold | 44.7's missing-template sentence, on the same `notices` channel |
| No vault flagged | — | nothing written | the rail's button is disabled; `createNote` resolves `null` |

---

## 5. Edge cases

| Case | Behaviour | Why |
|---|---|---|
| A space deleted between the click and the write | Ordinary note, `INFO` line, no notice | The thought is worth more than the filing. |
| The note cannot be re-read after writing | Note returned, no notice, `INFO` line | keeper failed to check its own work; the note is fine. Saying nothing would be the honest half of it. |
| `is:Pinned` (capitalised) | Pinned | The parser folds case before matching its closed flag set, so this does too. A seed that only knew the lowercase spelling would create an unpinned note in the Pinned space. |
| `tag:---` (normalises to nothing) | No tag, and the note honestly does not appear | The DSL lets `tag:---` match nothing; a note tagged `---` would be worse. |
| Two spellings of one tag from caller + space | One tag | Unioned through `tags::normalise_all`, which folds and de-duplicates in first-appearance order. |
| Caller names a `dest` **and** the space seeds one | The caller's | A caller naming one is answering a question the space only implied. |
| The template also carries tags | Unioned, `template` already stripped by `keeper-core` (AD-82) | The copy is not a template. |
| A collision counter renames the file (`…-2.md`) | The verdict's preview used the un-numbered name | The only divergence is the counter, and it matters only to a `path:` glob that discriminates on it. Named here rather than discovered. |
| `backlink:` in a space's query | Unbound, matches nothing, declines | The same degradation a broken query already takes. Binding an index for a note that has no inbound links yet would answer the same. |
| Two notices at once (missing template **and** won't appear) | Both render | Different sentences from different code paths; they cannot be identical, which is why the rendered key is the sentence. |
| Quick capture | Unchanged | Its `keeper.capture` mark is now spelled as the seed it always was, rather than as a `bool` parameter. |
| Today's journal (`⌘⌥J`) | Unchanged | `create_journal`, not `create_note` — the journal's filename comes from the configured path template and the collision counter must not move it. |
| A wikilink stubbing out a new note | `space: null` | A wikilink names a note, not a space. |

---

## 6. Where the caret goes

The AC is "the caret lands in the body", and it was already true — but only as an
accident of three separate facts, none of which had a test:

1. `createNote` selects the new note, so the editor mounts on it.
2. The editor's boot effect calls `editorView.focus()`.
3. The buffer **is the body**: the frontmatter block travels beside it on the body
   channel (AD-58), so offset zero is the body's first byte and there is no `---`
   for a caret to land above.

`src/components/notes/new-note-caret.test.tsx` mounts the **real** `NoteEditor` —
its own boot effect, its own dynamic imports, its own extension list — opened on a
note shaped the way `notes_create` writes one, and reads the live `EditorView`
through `EditorView.findFromDOM`. It asserts focus, the absence of `---` from the
buffer, and the caret at the end of the body; and, for a templated note, at the
`{{cursor}}` offset, which is an offset into the **body** and therefore cannot land
inside the block.

Nothing about the caret needed changing. It needed proving, and now removing
`editorView.focus()` fails two tests instead of none.

---

## 7. Deliberately NOT done

- ~~**`is:template` is not seeded (DW-167).**~~ **Closed before this spec shipped.**
  It was deferred because 44.7 was moving the predicate off the `templates/`
  folder and onto the note's own tag *while this story was being written*, and
  seeding either would have been seeding against a rule that was about to stop
  being true. 44.7 landed during the verification pass —
  `notes_vault::parse_note` now reads `templates::is_template` — so the reason
  expired and the arm is in: a space filtered `is:template` seeds the tag
  `template` and makes a new template. `verdict` needed no change at all,
  which is the design working: it re-runs the real query over the real bytes,
  so it simply stopped declining. Two tests
  (`a_template_space_creates_a_template`,
  `a_template_space_that_also_names_the_tag_adds_it_once`) and two mutants
  (M9, M10). Leaving a known-fillable hole in the deliverable because the
  ledger entry was already written would have been the wrong call.
- **`is:space` is not seeded, on purpose and permanently.** Creating a plain note
  in `spaces/` manufactures a space with no query — a broken row in the rail the
  user did not ask for. This is not a limitation to lift later.
- **No arbitrary frontmatter from `field:`.** A space filtered
  `field:priority=high` does not get a note with `priority: high`. Writing
  arbitrary frontmatter out of a filter turns New Note into a data-entry form, and
  the story's vocabulary is the three things a space selects on: its tags, its
  folder, its flag.
- **The seed does not rename the note to satisfy a `path:` filename pattern.** The
  title comes from the first line the user types; a create that named the file to
  satisfy a glob would be choosing the title.
- **No client-side list refresh after a create.** The row arrives when the
  reconciler streams it, exactly as it does for pin, archive and delete. Adding a
  refetch here and nowhere else would be one surface with a different rule.
- **`notes_vault::parse_note`'s flag block was not refactored (DW-166).** The
  seed's inverse mapping (`is:journal` → `journal/`) spells `journal` a second
  time, in `seed::JOURNAL_DIR`, with a doc comment naming the function it
  inverts. Hoisting the shared constant would have collided head-on with 44.7's
  in-flight change to the `is:template` predicate three lines away. Worth doing
  once wave 3 has landed; it is one constant and one `starts_with`.
- **No test of the shell's own wiring.** `create_for_space`, `space_source` and
  `index_written` live in the `keeper` crate, which does not build on Linux
  (AD-56). Everything decidable was pushed into `keeper-core`, where it is proved;
  what is left in the shell is one read, one call and one `push`.
- **No new dependency.** None was needed.

---

## 8. Files

**`keeper-core`**

- `src/notes/seed.rs` — **new.** `Seed`, `inherit`, `verdict`, `JOURNAL_DIR`,
  `literal_dir`. 25 tests.
- `src/notes/query.rs` — `Term` and `conjunction`, additive. `parse`, `eval` and
  `decompose` untouched. 6 new tests.
- `src/notes/vm.rs` — `NoteCreateReq.space`; `NoteCreateVm` (new).
- `src/notes/mod.rs` — one line.

**`keeper` shell**

- `src/notes_ipc.rs` — `notes_create` returns `NoteCreateVm`; `create_for_space`
  and `SpaceForCreate`/`space_source` (new); `create_note` takes the seed and the
  `notices` out-parameter, applies the seed's tags, folder, `pinned`, `archived`
  and `capture`. The capture path's `capture: bool` became a `Seed`.
- `src/notes_vault.rs` — `index_written` (new).

**Frontend**

- `src/lib/ipc/client.ts` — `notesCreate` returns `NoteCreateVm`.
- `src/lib/ipc/gen/NoteCreateVm.ts`, `NoteCreateReq.ts` — regenerated by `ts-rs`.
- `src/hooks/use-notes-actions.ts` — `createNote(spaceId = null)`.
- `src/components/notes/notes-pane.tsx` — the rail's control, the notices, and the
  space callback. `NEW_NOTE_LABEL`, `NOTES_NOTICE_SLOT`.
- `src/components/notes/space-list.tsx` — `onNewNote`, and the `+` on each row.
- `src/components/notes/editor/wikilink.ts` — `space: null`, `created.note.id`.

**Tests**

- `src/components/notes/new-note-caret.test.tsx` — **new**, 3 tests over the real
  editor.
- `src/components/notes/notes-pane.test.tsx` — 5 new tests; the fake `notes_create`
  now models Rust's seed for the four seeded defaults and throws on a lens it does
  not know.

---

## 9. Proof by reverting

Every mechanism was removed, the tests were run, the failures recorded, and the
tree restored.

**Everything below was re-run from scratch under a private harness**
(`~/.w3newnote/mutate.py`) after `/tmp/mutate*.py` turned out to be shared by
several agents in this wave, with at least one background job executing another
agent's script. My original mutations were inline heredocs, so there was no
script file to swap — but the restore copies lived in `/tmp` and that is the
same class of exposure. The harness now prints, for every mutation: the unified
diff against pristine **at the moment the tests ran**, a guard that exactly one
owned file differed, the run's own `test result:` line, and the failing test
names. It runs the unmutated suite as a baseline before *and* after the sweep,
and aborts on a dirty, red or unfinished one.

| # | Mutation | Tests that caught it |
|---|---|---|
| M1 | `seed_flag` stops setting `pinned` | `the_pinned_space_creates_a_pinned_note`, `an_is_flag_is_matched_the_way_the_parser_folds_it`, `creating_into_a_seeded_default_produces_a_note_that_default_selects` (3) |
| M2 | `verdict` never declines | 12, including `a_space_no_creation_can_satisfy_names_the_term_that_defeated_it`, `a_space_whose_query_cannot_be_read_still_creates_and_says_why`, `a_date_window_in_the_past_is_reported_rather_than_ignored` |
| M4 | `conjunction` skips `\|`/`(`/`)` instead of returning `None` | `query::a_query_with_structure_is_not_a_conjunction`, `seed::a_query_with_structure_seeds_nothing_and_blames_no_term`, `seed::a_space_whose_query_cannot_be_read_still_creates_and_says_why` (3) |
| M5 | `literal_dir` keeps the filename segment | `a_path_glob_commits_to_its_literal_directory_and_never_to_its_pattern` (1) |
| M9 | `is:template` is not seeded | `a_template_space_creates_a_template` (1). Not its sibling: with the flag unseeded, `is:template tag:template` still gets the tag from the `tag:` term, so that test stays green and is M10's job. |
| M10 | `add_tag` stops de-duplicating | `a_template_space_that_also_names_the_tag_adds_it_once` (1) |
| M6 | `note-editor.tsx` stops calling `editorView.focus()` | `opens focused, with the caret in an empty body and no block in the buffer`, `puts the caret after a template's scaffold rather than in front of it` (2) |
| M7 | the pane passes `null` instead of `space.id` | `creates from a space into that space, carrying the space's id and not its query`, `still creates from a space no new note can satisfy…`, `clears a previous create's notice…` (3) |
| M8 | the pane drops the create's `notices` | `still creates from a space no new note can satisfy…`, `clears a previous create's notice…` (2) |
| M11 | `App.tsx` stops mounting `useNotesOpenNote()` | `App > subscribes to the tray's open-note bridge as soon as the app mounts` (1) |

Four of these are worth more than their row.

**M11 survived its first run, and the reason is the bug it is about.** The first
version of the fix had `use-notes-open-note.test.ts` — six tests over the hook,
all passing — and deleting `useNotesOpenNote()` from `App.tsx` broke none of
them, because `renderHook(() => useNotesOpenNote())` mounts the hook itself. A
suite can be exhaustive about a listener and never once ask whether anything
subscribes it, which is *exactly* how `listenNotesOpenNote` came to be declared
and called from nowhere for two epics. `use-notify-navigate.test.ts` has the
same shape, which is presumably why nobody noticed. The assertion therefore
lives in `App.test.tsx`, where `<App />` is really rendered, and M11 now fails.

**M5 initially caught nothing.** The first `literal_dir` test used only globs
whose last segment carried a `*`, so the glob-character break already produced
the right answer and dropping the last segment was unobservable. That is the
same shape as W3Counts' survivor and W3Csv's: *a fixture that cannot reach the
boundary the code branches on*. `path:notes/inbox` reaches it, and the mutation
now fails.

**M4's first verdict was contaminated, and the contamination was in the
dangerous direction — a "caught" claim with something else under it.** It ran
with the filter `notes::` (the whole notes tree) while its baseline covered only
`notes::seed` and `notes::query`. It reported nine failures; six were in
`notes::csv`, `notes::template_update` and `notes::templates`, modules other
agents were mutating at that moment. `pub fn conjunction` has exactly two
callers in the workspace — `seed.rs` and its own tests, grepped rather than
assumed — so those six could not have been mine. The published *number* was
right and the *evidence* was junk, which is the same defect. Re-run scoped to
the two modules this story owns, with the baseline covering exactly that scope:
green before, green after, three failures, all mine. **The rule: a baseline must
cover exactly the scope the mutation run covers.** In a wave where five agents
mutate five modules of one crate, a broad `cargo test` filter is a contamination
funnel.

**The harness ate my own work once, and the tests caught it.** A sweep was
running when I edited `seed.rs` to add the `is:template` arm; its `restore_all`
wrote the pristine copy back over the production change while the tests I had
added afterwards survived. The file was then one whose tests asserted a
behaviour its code no longer had — and it failed on the next run, immediately
and unambiguously. `restore_all` now refuses to overwrite a file whose mtime is
newer than its snapshot when no mutation is applied, and says so.

Final state, all green, no mutant applied, marker file absent:

```
cargo test --manifest-path src-tauri/Cargo.toml -p keeper-core --lib notes::seed    # 27 passed
cargo test --manifest-path src-tauri/Cargo.toml -p keeper-core --lib notes::query   # 45 passed
bun run test src/components/notes/notes-pane.test.tsx                               # 21 passed
bun run test src/components/notes/new-note-caret.test.tsx                           # 3 passed
```

---

## 10. Acceptance

| AC | Where |
|---|---|
| A note created inside a space is selected by that space when it appears | `seed::creating_into_a_seeded_default_produces_a_note_that_default_selects` over `DEFAULT_SPACES` itself; `notes-pane`'s "creates from a space into that space" asserts the rendered row in Pinned |
| Created from the rail it appears in the default list | `notes-pane`'s "creates from the rail into the default list and opens the note", which also asserts `space: null` on the wire |
| The space's default template is applied | 44.7's, through the middle rung of `template_source`; this story supplies `SpaceForCreate.template` from the same single read of the space note |
| The caret lands in the body | `new-note-caret.test.tsx`, over the real editor (§6) |
| A space whose query cannot be satisfied by creation still creates a note and says it will not appear here | `seed::a_space_no_creation_can_satisfy_names_the_term_that_defeated_it`; `notes-pane`'s "still creates from a space no new note can satisfy" asserts the note is open, the notice renders, and Recordings does not list it |
| Creating from a space whose template was deleted still creates a note and says the template is missing | 44.7's `TemplateChoice::Missing` sentence, carried on this story's `notices` channel and rendered by this story's notice slot |
| From the rail, from a space, and from the command palette | The rail's button, the `+` on each space row, and `notes-new` — which routes through the same `createNote` and therefore through the same assertion |

---

## 11. The part of this story that has never been compiled

Written down here rather than left in a chat thread, because a lesson that only
exists in IRC is one the next person re-learns. W3Csv reached the same
conclusion from the other end of the wave — a `tsc` error that sat broken for
hours behind a green vitest suite and a green 15/15 mutation sweep — and the
general form is worth stating plainly:

> **A green test suite plus a green mutation sweep is not a green build.**
> vitest transpiles without typechecking; `cargo test` compiles only the crates
> it runs. Mutation testing cannot help either, because a mutant only probes
> behaviour the tests already assert. Both systems answer *does the code do the
> right thing*. Neither answers *does the code compile*.

On this wave the compile question was the one nobody owned, and **this story is
the sharpest instance of it in the epic.** Not a hypothetical: the `keeper`
shell crate cannot build on Linux at all — `cargo check -p keeper --lib` dies in
`glib-sys`'s build script before it reaches a single line of keeper's own
source, because there is no `pkg-config` on this host. So four things 44.6 added
have **never been through any compiler, on any machine**:

| Symbol | File | What a compiler would settle |
|---|---|---|
| `create_for_space` | `notes_ipc.rs:1730` | that `NoteCreateVm` is constructed with both fields; that `Frontmatter::parse`'s `(fm, offset)` tuple is destructured the right way round; that `&space.name` coerces to `&str` |
| `SpaceForCreate` / `space_source` | `notes_ipc.rs:1795`, `:1815` | that `snapshot.by_id` borrows survive being turned into owned `String`s; that `Option` short-circuits (`?`) line up with the return type |
| `index_written` | `notes_vault.rs:1351` | that the synthesised `FileStat` matches the struct; that `i128::from(now) * 1_000_000` is the right type for `mtime_ns` |
| the seed application in `create_note` | `notes_ipc.rs` | that `tags::normalise_all(...chain(...).map(String::as_str))` satisfies `impl IntoIterator<Item = &'a str>`; that `FieldValue::Bool` exists |

Every one of those is a *type* question, which is exactly the class no test in
this repo can reach for this crate. That is why the decidable half of the story
was pushed into `keeper-core` — `seed.rs` is 27 tests and `query.rs` 45, all
runnable on any host (AD-55/AD-56) — and why what is left in the shell is
deliberately one read, one call and one `push` per branch. But "small" is not
"compiled", and this epic has already shipped three things that were green and
did nothing.

### Writing this section found a real one

This is not theoretical, and the proof is that hand-auditing the list above
turned up an actual compile error in code that had passed every test in this
story: a refusal built with `IpcErrorCode::InvalidInput`, **a variant that does
not exist**. Notes refusals use `NotesInvalid`. A compiler says that in
milliseconds; 444 green Rust tests and 443 green frontend tests said nothing,
because none of them can reach this crate. Fixed, and the comment at the site
names DW-170 so the next reader knows why a typo like that survived to be found
by eye.

The rest of the surface was then audited the same way, symbol by symbol, and is
clean: `SpaceDef.template` exists (44.7 added it), `FieldValue::{Str, Bool,
List, Map}` all exist, and `Frontmatter::parse` really does return
`(Frontmatter, usize)` in that order. Confirmed by reading the definitions, not
by remembering them.

### And it found a data-loss path this story made reachable

The audit's second result, and the more serious one. `write_note` →
`write_vault_file` → `atomic_write` **overwrites unconditionally**. The only
thing standing between a create and somebody's existing note is
`naming::note_filename`'s collision counter, which is derived from
`notes_vault::siblings` — and `siblings` is `read_dir(...).unwrap_or_default()`,
so a directory it **cannot read** is reported as an **empty** one. An unreadable
folder therefore yields a filename keeper believes is free, and the write
replaces a note that is already there, in a vault whose next commit carries the
replacement to every machine.

That path was unreachable before this story, and the reason is this epic's
recurring lesson for the fifth time: **`NoteCreateReq.dest` was a field nobody
applied.** Every caller in the app passed `None` — the tray, capture, the
wikilink stub, the palette, `blank_note()`, the tests. The only directory ever
listed was the vault root, and a root keeper cannot read is a vault that is
already gone. The space seed is the first thing in this app to choose a
subdirectory, so 44.6 is what makes the hole reachable and 44.6 is what closes
it: `create_note` now refuses if the path it is about to write already exists,
naming the file and saying nothing was changed.

Refusing rather than picking another name is deliberate. If keeper cannot list
the folder it is writing into, it does not know what else is in there either,
and a blank note is the cheapest thing in the app to ask for again.

**What the macOS gate owes this story**, concretely, beyond `cargo check`:

1. Create a note from the rail. It appears in the default list and the caret is
   in the body.
2. Create one from **Pinned**. The file on disk carries `pinned: true` and the
   note appears in Pinned.
3. Create one from **Journal**. The file lands under `journal/`.
4. Create one from **Recordings**. The note exists, the notice renders above the
   list, and `Console.app` carries the matching `INFO` line — the log check
   matters as much as the UI one, because DW-162 is the story of a decision that
   only existed in a `debug!` the packaged app cannot print.
5. Create one from **Inbox** twice. The second create clears the first's notice.
6. **The overwrite backstop.** `chmod 000` a subfolder the seed targets, create
   into that space, and confirm keeper refuses with the sentence naming the path
   — and that the file already there is byte-identical afterwards. This is the
   only gate item about data rather than about display, and byte equality is the
   check, because a note that was replaced with a blank one looks like a note
   that was created.

Filed as DW-170 so it is owed rather than remembered. (DW-169 went to 44.8 while this was being written.)
