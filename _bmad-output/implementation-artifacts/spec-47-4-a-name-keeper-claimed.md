# Spec 47.4 — A name keeper claimed, and labels that follow the registry

story: 47.4
closes: DW-191, DW-195, and the same defect one file over in `templates.rs` (found here, assigned
by Main mid-story, **fixed rather than ledgered** — no new DW number)
opens: nothing
sentinel: `MUT47-4`

files owned and touched:

- `src-tauri/crates/keeper-core/src/notes/default_spaces.rs` — DW-191. The `seed` function and the
  seeded-key ledger live here, not in `notes/seed.rs`; ownership confirmed by Main.
- `src-tauri/crates/keeper-core/src/notes/templates.rs` — the same defect against the shipped
  templates. Assigned by Main after I reported it.
- `src-tauri/crates/keeper-core/src/palette.rs` — DW-195, the projection.
- `src-tauri/crates/keeper/src/tray.rs` — DW-195, the glue. **Never compiled.**
- `src/test/tray-notes-labels.test.ts` — CREATED. The only gate for `tray.rs` that runs on Linux.

`notes/seed.rs` was read and **not modified**. DW-191 names it as the location, but `plan`, `seed`,
`record_deleted` and the ledger are all in `notes/default_spaces.rs`; `seed.rs` is Story 44.6's
"what a new note must carry", an unrelated module with a colliding name.

---

## The lesson this story is worth reading for

**Any "these two things must not drift" test that reads VALUES cannot see the drift — only its
absence today.**

DW-195 is one word spelled in two places. The obvious test asserts the tray's label equals the
registry's title. It passes over `new_note: "New Note".to_owned()` hand-typed **inside the
projection function** — the exact duplication it was written to catch, because both sides of the
equality are then the same literal typed twice. That mutation survived my first sweep.

The repair is to test the property that actually holds, which is about source text and not about
values: **each title is spelled exactly once across the two files.** That has a hole too —
`format!("Quick {}", "Capture")` never spells the word and produces it anyway, and that mutation
survived the repair. So a second probe: **the functions that MOVE labels contain no string literal
at all.** Two probes, because the first had a hole I went looking for instead of assuming away.

This generalises past labels to every duplication-is-forbidden invariant in the repo: one source of
truth, two readers, a test comparing what the readers produce. The test cannot distinguish "read
from the source" from "retyped to match the source". Only reading the source can.

The same habit found the templates defect: the fix for DW-191 was finished and green, and the
question that followed was not "is it done" but **"where else does this exact shape live"**. One
grep away, in `templates.rs`, doing the same thing to a different file in the same vault.

---

## Part one — DW-191: the default space that comes back

### The product call

`seed` recorded a ledger entry for each default space it **created**. A default it stood down for —
because the vault already held a space of that name — left no entry. So the user's own space was
not protected by the mechanism that protects keeper's: delete it, and the next seed saw a name
absent from both the vault and the ledger, and wrote keeper's version in its place.

**The call: the ledger records the names keeper has CLAIMED.** A default it stood down for is
recorded exactly like one it created.

One function, `claimed(&[ExistingSpace]) -> BTreeSet<String>`, read by both `plan` (what to skip)
and `seed` (what to record). One function rather than a filter written twice, because "stood down
for" and "claimed" must be the same answer; two spellings drift, and the symptom is a name recorded
as claimed that the planner still writes, or the reverse.

**What it costs, stated rather than discovered.** A name freed deliberately is not re-offered:
rename your own "Journal" to "Diary" and keeper will not write its Journal, because it stood down
for that name once. That is the asymmetry this module had already chosen and written down — *"the
cost of not offering a space is a menu item away and the cost of resurrecting four the user deleted
is keeper editing their vault behind their back."* Restore ignores the ledger, which is its entire
job, so the escape hatch is one menu item; writing into somebody's vault uninvited has none.

### The upgrade path

An installed vault holds a ledger written under the old meaning: the keys keeper WROTE, with the
names it stood down for missing. **Decision: the first run after this change reconciles.** It writes
no space note — nothing is missing — and records the claims the old ledger could not have held. The
alternative, waiting until a run happens to write something, leaves the defect live on exactly the
vaults that already have it, which is every installed one.

Two guards on that write, each its own test:

| guard | why |
| --- | --- |
| only when the claim set actually GREW | `.keeper-spaces.json` is synced content. A rewrite on every launch is a commit per launch in somebody's vault history; the sync engine cannot tell keeper's bookkeeping from a real edit. |
| only when the ledger was READABLE | An automatic run has already returned `Blocked`. The remaining path is Restore over a ledger keeper could not parse — it may be a newer build's, and replacing it re-offers that build's defaults. |

### `SeedOutcome` gained no variant, deliberately

The upgrade run still answers `AlreadySatisfied`. `keeper/src/notes_ipc.rs` matches this enum
exhaustively in two places and is **owned by L5Tail this wave**; a fifth variant, or data on
`AlreadySatisfied`, would force an edit in a file I do not own, in a crate that does not compile on
this host, and would break layer 4's PR until layer 5's landed. What changed instead:

- the variant's doc says the run may still have recorded a claim, and why that is bookkeeping
  rather than an outcome;
- its log sentence went from `wrote nothing` to `wrote no spaces`, because the first is now capable
  of being false.

The claim write is silent for the reason every other ledger write here is silent: `keys_recorded` is
documented best-effort (`let _ = vault.write(...)`) and has never reported. This extends an existing
silent-by-design write; it does not add a silent arm to the outcome enum, which is the property
`no_seed_outcome_reports_below_the_level_the_app_can_print` protects.

### The doc comments that stated the old meaning

| where | was | now |
| --- | --- | --- |
| module header | "the spaces keeper seeds into a vault, and the rule for when it may" | adds the names it claims, plus a paragraph naming DW-191 and pointing at `claimed` |
| `plan`'s `offered` | "the keys this vault has already been given" | "the names keeper has claimed in this vault" |
| `DeleteRecord::AlreadyRecorded` | "keeper seeded the space, so it recorded the key when it wrote the note" | keeper claims a key the first time it seeds **or stands down for** it |
| `SeedOutcome::AlreadySatisfied` | silent about the ledger | may have recorded a claim; why not a fifth variant |
| `record_deleted` | "`seed` records only the keys it WROTE …" | rewritten, below |

**`record_deleted`'s comment was not stale — it was false when written.** It claimed the case that
made the function load-bearing was a default stood down for a user's own space of the same name. It
never was: that space is the *user's*, it carries no `keeper.default`, so `default_key_of` answers
`None`, and the deletion is correctly `NotADefault` and records nothing. The comment asserted a
guarantee and *was* the guarantee — the hazard was described as handled and was not. That is why
DW-191 existed at all. It now says so, and says what `record_deleted` IS still load-bearing for: a
ledger write that failed, leaving keeper's spaces on disk with nothing recording them, so the
deletion must.

`LEDGER_REL`'s and `LEDGER_NOTE`'s wording already said "offered", which is the claimed meaning, and
was left alone. `notes/vm.rs`'s empty-state sentence ("keeper seeded this space…") is keyed on the
`keeper.default` marker and only ever shown for a space keeper really wrote; still true, not mine,
untouched.

### I/O matrix — `seed`

| ledger | `spaces/` | mode | outcome | ledger after |
| --- | --- | --- | --- | --- |
| absent | absent | FirstRun | `Wrote` × 5 | all five |
| absent | user's "Inbox" | FirstRun | `Wrote` × 4 | **all five** — `inbox` claimed, not written |
| four keys (old meaning) | keeper's four + user's "Inbox" | FirstRun | `AlreadySatisfied` | **all five** — the upgrade write |
| all five | all five present | FirstRun | `AlreadySatisfied` | unchanged, **not rewritten** |
| all five | one deleted | FirstRun | `AlreadySatisfied` | unchanged |
| unparseable | any | FirstRun | `Blocked` | untouched |
| unopenable (EACCES) | any | FirstRun | `Blocked`, naming the errno | untouched |
| unparseable | everything present | Restore | `AlreadySatisfied` | **untouched** — no ledger invented over one keeper could not read |
| unparseable | some missing | Restore | `Wrote` | replaced (pre-existing 44.3 behaviour, unchanged) |
| any | `spaces/` unlistable | either | `Blocked` | untouched |
| any | a write fails part way | FirstRun | `Stopped` | what landed, plus claims |

### `claimed` — I/O

| existing space | claims |
| --- | --- |
| none | ∅ |
| `Unfiled` carrying `keeper.default: inbox` | `inbox` — survives a rename |
| `Inbox` / `inbox` / `  INBOX  `, no marker | `inbox` — the `naming::slug` fold |
| `Clients`, no marker | ∅ |
| `Clients` carrying `keeper.default: archive` | ∅ — a marker this build does not know is not a key |
| `Inbox` + `Sessions`(recordings) + `Clients` | `{inbox, recordings}` |

### Edge cases handled

- **Restore + name collision.** Restore ignores the ledger but still stands down by name, so the
  claim is recorded there too. keeper claimed the name either way.
- **A renamed default.** Claimed by key, so a vault whose ledger was deleted re-acquires the key
  rather than growing a second Inbox.
- **A ledger the user deleted on purpose** (`LEDGER_NOTE` invites it). The next run claims
  everything present and rewrites — which is what "be offered all of them again" means, minus the
  names already taken.
- **An unreadable space note inside `spaces/`.** Its name reads empty, so it claims nothing; its
  filename is still in `taken`, so keeper's lands beside it under a counter. Unchanged.
- **Two defaults written in one pass.** The filename counter and `taken` list are untouched.

---

## Part two — the same defect in `templates.rs`

Found by asking where else the shape lives; assigned by Main; fixed here rather than ledgered.

`plan_templates` stands a shipped template down when the vault already holds a file of that name
(`Journal entry.md` and `journal-entry.md` are one name, folded by `naming::slug`), and
`seed_templates` recorded `template.key` only on the write path. **Delete your own
`journal-entry.md` and keeper's shipped scaffold arrives in its place** — the surprise 44.7 refused
for templates in prose, which `plan_templates` had been producing since.

Same fix, same two guards, same upgrade decision: `claimed_templates(&[String]) -> BTreeSet<String>`
read by both the planner and the seeder, a reconciliation write on the settled path gated on
grew-and-readable.

**Are the two ledgers the same thing? The FILE FORMAT is; the PRESENCE RULE is not** — and the doc
comment on `claimed_templates` says so in one sentence, so the next reader does not try to merge
them. Parsing is already shared (`default_spaces::parse_ledger`), because parsing genuinely is one
rule. Presence is not: a space is claimed by its `keeper.default` marker **or** its display name,
because a space note carries an identity that survives a rename; a template carries no marker at all
— AD-82 makes a template an ordinary note with an ordinary tag, so nothing on it says "this is
keeper's Journal entry" — and its only identity is its filename. One rule reads two fields, the
other reads one. Sharing a function would mean inventing a marker templates do not have.

Templates have no `record_deleted` equivalent, so this is the whole of the gap there: nothing else
tombstones a shipped template.

44.7's own doc comments were re-read for false statements and none were found: `journal_template`'s
"an absent template is `None`, deliberately… deleting the shipped journal scaffold has to stick"
was true then and is more true now, and the ledger note and both ledger-IO comments already said
"offered" rather than "wrote".

### I/O matrix — `seed_templates`

| ledger | `templates/` | mode | outcome | ledger after |
| --- | --- | --- | --- | --- |
| absent | absent | FirstRun | `Wrote` × 3 | all three |
| absent | user's `journal-entry.md` | FirstRun | `Wrote` × 2 | **all three** — `journal` claimed, not written |
| two keys (old meaning) | keeper's two + user's journal file | FirstRun | `AlreadySatisfied` | **all three** — the upgrade write |
| all three | all three present | FirstRun | `AlreadySatisfied` | unchanged, **not rewritten** |
| unparseable | everything present | Restore | `AlreadySatisfied` | **untouched** |
| any | `templates/` unlistable | either | `Blocked` | untouched |

---

## Part three — DW-195: the tray's notes labels

### What projects, and what does not

`tray_notes_labels(notes: bool) -> Option<TrayNotesLabels>` in `keeper-core::palette`, plus
`TrayNotesLabels::painted(state)` for the two composed empty states. Three named ids
(`NOTES_NEW_ID`, `NOTES_CAPTURE_ID`, `NOTES_JOURNAL_ID`), not "the Notes category": `notes-open`,
`notes-search` and `notes-switch-vault` are registered and deliberately absent from the menu bar,
and a test asserts they are registered so that assertion is not vacuous.

**Named fields, not a `Vec`.** This is the whole defence against the hazard DW-195 warns about. The
recording verbs take the registry's ORDER because they are rebuilt per menu; the notes items are
built once and only mutated (AD-61 — a Linux tray menu cannot be swapped after it is set), so a
positional projection would move a label onto the wrong handle the day the registry is reshuffled.
A field cannot slip.

**Only the label.** The tray's "Open Recordings Folder" reveals the LIVE session's output path while
the registry's verb reveals the CONFIGURED destination — same words, two different folders. The
notes verbs keep their `tray-note-*` ids, their handlers and their enabled state; a test asserts no
registry id is spelled anywhere in `tray.rs`.

**The Recording-section count assertion is untouched.** Nothing was added to, removed from or
re-homed out of the Recording category; `registry_sections(true, false)` still finds exactly three.

### The composition moved too

The brief's constraint — *every decision that can be a pure function in keeper-core* — outranks
DW-195's suggested shape (*keep `paint_notes` composing the suffix*). Both suffixes,
`… (no vault yet)` and ` — hotkey unavailable` (UX-DR43), are now in `painted`, tested on Linux, and
`tray.rs` composes nothing. Both are **appended, never substituted**, so a retitle reaches the empty
states as well; that is its own test, because a substitution would leave the menu bar saying the old
word the moment a vault is missing.

### A drift found on the way in

The registry ships `Today's Journal` with an ASCII apostrophe; `tray.rs` spelled `Today’s Journal`
with U+2019. **UX-DR42 says the four surfaces carry one title, and they did not — they had already
drifted, and no gate anywhere noticed.** The registry wins verbatim, apostrophe included, so the
menu bar now shows the plain form. Not the other direction: the palette matches keystrokes against
`title_lower` by subsequence, and a curly apostrophe nobody's keyboard types would trade a glyph for
a search regression on `today's`.

### Initial labels

The three items are still built at the registry's **bare** word, not at an empty state. Before the
first index publish keeper does not know whether there is a vault or whether the hotkey registered,
and `Quick Capture — hotkey unavailable` on a tray built one tick early is a lie in the other
direction. Behaviour is byte-identical to what shipped, apostrophe aside.

### I/O matrix — `tray_notes_labels` / `painted`

| `notes` | vault | hotkey | new_note | capture | journal |
| --- | --- | --- | --- | --- | --- |
| false | — | — | `None` — no notes section | | |
| true | — | — (build time, uncomposed) | `New Note` | `Quick Capture` | `Today's Journal` |
| true | yes | yes | `New Note` | `Quick Capture` | `Today's Journal` |
| true | no | yes | `New Note… (no vault yet)` | `Quick Capture` | `Today's Journal… (no vault yet)` |
| true | yes | no | `New Note` | `Quick Capture — hotkey unavailable` | `Today's Journal` |
| true | no | no | `New Note… (no vault yet)` | `Quick Capture — hotkey unavailable` | `Today's Journal… (no vault yet)` |

`capture` carries no vault suffix on purpose: quick capture works without a vault — it is the verb
that makes one usable — and that asymmetry is asserted rather than left to the reader.

`None` has two causes (capability off; the registry could not answer for one of its own ids) and one
correct response, so it is one answer. The second is a bug rather than a state, and
`the_tray_builds_no_notes_section_when_the_capability_is_off` plus
`every_label_the_trays_notes_section_shows_is_the_registrys_own_word` are what make it one.

---

## Mutation table

Baseline established GREEN in every scope each sweep uses, immediately before it, in the same
command the sweep runs: `cargo test -p keeper-core --lib --` for `palette::`,
`notes::default_spaces::`, `notes::templates::` and `notes::seed::`, and
`npx vitest run src/test/tray-notes-labels.test.ts`. A sweep over a red test measures nothing and
reports success. Drivers: `/tmp/mut47-4.py`, `/tmp/mut47-4b.py`, `/tmp/mut47-4c.py`.

| # | mutation | killed by |
| --- | --- | --- |
| M1 | `seed` records only what it wrote (the claim removed) | `a_default_stood_down_for_a_name_the_user_took_is_claimed_and_their_space_stays_gone`, `a_ledger_written_under_the_old_meaning_is_reconciled_by_the_first_run_after_it` |
| M2 | the upgrade write removed | `a_ledger_written_under_the_old_meaning_is_reconciled_by_the_first_run_after_it` |
| M3 | upgrade write ignores the readable guard | `a_restore_with_nothing_to_restore_does_not_write_a_ledger_over_one_it_could_not_read` |
| M4 | upgrade write on every run (changed-set guard removed) | `a_settled_vault_touches_nothing_at_all_on_the_runs_after_the_upgrade` |
| M5 | `claimed` drops the by-name arm | six, incl. two pre-existing 44.3 tests |
| M6 | `claimed` drops the by-key arm | `a_default_is_claimed_by…`, `a_default_that_was_renamed_is_still_that_default` |
| M7 | `claimed` stops folding the name through `naming::slug` | five, incl. two pre-existing |
| T1 | `seed_templates` records only what it wrote | `a_template_stood_down_for_a_name_the_user_took_is_claimed_and_their_file_stays_gone`, `a_template_ledger_written_under_the_old_meaning_is_reconciled_by_the_next_run` |
| T2 | the template upgrade write removed | `a_template_ledger_written_under_the_old_meaning_is_reconciled_by_the_next_run` |
| T3 | template upgrade write ignores the readable guard | `a_template_restore_with_nothing_to_restore_leaves_an_unreadable_ledger_alone` |
| T4 | template upgrade write on every run | `a_settled_template_vault_touches_nothing_on_the_runs_after_the_upgrade` |
| T5 | `claimed_templates` stops folding the filename | six, incl. two pre-existing 45.20 tests |
| T6 | `claimed_templates` answers the empty set | six, same |
| M8 | `tray_notes_labels` hand-types `"New Note"` | `spells each of the three words exactly once`, `carries no text at all through the three functions` |
| M8b | the base word assembled in pieces: `format!("Quick {}", "Capture")` | `carries no text at all through the three functions` |
| M9 | `capture`/`journal` swapped onto the wrong fields | `every_label_the_trays_notes_section_shows_is_the_registrys_own_word`, `the_notes_labels_land_on_the_handle_they_belong_to_whatever_the_registry_order` |
| M10 | the UX-DR43 hotkey suffix dropped | `the_two_empty_states_are_said_in_words_and_only_on_the_verbs_they_are_about` |
| M11 | the no-vault suffix substitutes instead of appending | `a_retitled_notes_verb_reaches_the_tray_in_every_state_it_can_be_in`, `the_two_empty_states…` |
| M12 | FR-122 capability gate bypassed | `the_tray_builds_no_notes_section_when_the_capability_is_off` |
| M13 | the tray widened to a non-tray verb (`notes-open`) | `every_label_the_trays_notes_section_shows_is_the_registrys_own_word` |
| M14 | `tray.rs` spells a label again | `lets the tray spell none of the three verbs`, `spells each … exactly once`, `carries no text…` |
| M15 | `tray.rs` composes the hotkey suffix again | `lets the tray spell neither empty-state suffix`, `carries no text…` |
| M15b | `tray.rs` re-adds the no-vault suffix at the paint site | same |
| M16 | `tray.rs` stops projecting at build time | `takes its labels from the projection at build time and at paint time` |
| M16b | `tray.rs` stops composing at paint time | same |
| M17 | a registry id reaches the tray's dispatch | `keeps the registry to the words and never to the click`, `carries no text…` |

**26 mutations, 26 kills, 0 survivors — after M8 and M15 survived the first pass.**

- **M8** is the lesson at the top of this spec.
- **M15** survived for a duller reason worth recording: the source-reading test parsed string
  literals with a paired-quote regex over the whole file, and a single unbalanced quote earlier in
  `tray.rs` (in a trailing comment) shifts every pair after it, so the extractor was reading the
  GAPS BETWEEN literals. It reported five green tests over a file it had mis-parsed. Replaced with
  line-oriented matching over comment-stripped source: no pairing, nothing to desynchronise.

Two mutations are worth naming as deliberately weak evidence: **M16 used
`TrayNotesLabels::default()`, which would not compile** (no `Default` derive). It was measuring
whether the source scan notices the projection is gone, which it does, and that is all it was asked
to measure — there is no compiler on this host to satisfy anyway.

**Restore verified by reading the diff**, not by memory: `git diff` shows six hunks in `tray.rs`,
seven in `templates.rs`, seven in `palette.rs` and fourteen in `default_spaces.rs`, all inside
ranges I edited, and no hunk in any file another layer owns. Repo-wide grep for `MUT47-4`: zero
hits. The one file I CREATED, `src/test/tray-notes-labels.test.ts`, is invisible to `git diff` and
was checked by name.

---

## Deliberately NOT done

- **`SeedOutcome` gained no fifth variant** and `AlreadySatisfied` gained no payload —
  `notes_ipc.rs` is L5Tail's and matches exhaustively, and a shared-type change would ship a layer
  that breaks the shell build until the next one lands.
- **`claimed` and `claimed_templates` were not merged.** The presence rules genuinely differ; the
  one sentence saying why is on `claimed_templates` so the next reader does not try.
- **The registry title was not changed to a typographic apostrophe.** It would reach all four
  surfaces, which is the point of a registry, and would break subsequence search on `today's`.
- **`notes/vm.rs`'s empty-state sentence untouched** — keyed on the marker, so only shown for a
  space keeper really wrote, and still true.
- **No `Default` derive on `TrayNotesLabels`.** A default set of labels is three empty strings — a
  tray with three blank rows — and nothing needs it.
- **`notes/seed.rs` unmodified.** Named by DW-191, but the ledger is not there.
- **No new dependency.**

## What I could not verify here, and why

**No line of `src-tauri/crates/keeper/src/tray.rs` was compiled.** The `keeper` shell crate does not
build on Linux (no GTK/webkit), and per the wave's constraint I ran no
`cargo build/check/clippy/test -p keeper`. Everything below the shell — the projection, the
composition, both claim rules, both upgrades — is `keeper-core` and runs on this host. What is
unproven is sixteen lines of Tauri glue.

What stands in for a compiler, and what does not:

- `src/test/tray-notes-labels.test.ts` reads `tray.rs` as text on every host. It proves the
  projection is called at both sites, that no label or suffix is spelled in the file, and that no
  registry id reached the dispatch. **It cannot prove the file type-checks.**
- Hand-checked against the signatures; each is a one-line risk. `&labels.new_note` is `&String` →
  `&str` by deref coercion into `menu_item(_, _, &str, _)`. `labels` is borrowed by three
  `menu_item` calls and then moved into the same struct literal — the borrows end first under NLL.
  `TrayNotesLabels` derives `Clone`, which `NotesItems`' own `#[derive(Clone)]` requires.
  `painted(&self, TrayNotesState) -> Self` is called on an owned field and its result bound before
  use.

### Ordered gate checks, on the macOS host (hesperia)

1. `cargo test -p keeper-core --lib -- notes::seed:: notes::default_spaces:: notes::templates::
   palette::` — 184 passed, exit 0 here; it should be identical there. If it is not, the difference
   is the host, and that matters more than anything below.
2. `cargo check -p keeper` — the only thing that can fail is my sixteen lines. Most likely failure:
   an import name (`tray_notes_labels`, `TrayNotesLabels`, `TrayNotesState` are all `pub` in
   `keeper_core::palette`) or the `&String` → `&str` coercion.
3. `cargo clippy -p keeper -- -D warnings` — watch for `clippy::redundant_clone` on `painted`'s
   three `String`s, and for an unused import if a name went in that is not used.
4. `cargo test -p keeper` — nothing in `tray.rs`'s own test modules touches the notes labels
   (`sync_tray_tests` is about glyphs), so a failure here is a compile failure, not a behaviour one.
5. Build, install, **open the menu-bar icon.** Three things, in order:
   - the three notes rows read `New Note`, `Quick Capture`, `Today's Journal` — **with a plain
     apostrophe**, which is the visible change;
   - with no vault registered, `New Note… (no vault yet)` and `Today's Journal… (no vault yet)`,
     and `Quick Capture` unsuffixed;
   - with ⌘⌥K held by another app so registration fails, `Quick Capture — hotkey unavailable`.
6. **DW-191 on a real vault**, which no test on either host can do. Register a vault that already
   contains a space named `Inbox` the user wrote. Confirm `.keeper-spaces.json` gains `"inbox"` on
   the first launch **without** keeper writing an Inbox note. Delete their Inbox from the rail,
   relaunch, confirm nothing arrives in its place. Then `Restore default spaces` and confirm
   keeper's Inbox appears — the claim must be escapable.
7. **The same for templates**: a vault holding the user's own `templates/journal-entry.md`. Confirm
   `.keeper-templates.json` gains `"journal"`, that the file is byte-unchanged, that deleting it
   sticks, and that `Restore default templates` brings keeper's back.
8. **The upgrade, on the owner's actual vault** — the one that has the old ledgers. Confirm the
   first launch after this build rewrites `.keeper-spaces.json` and `.keeper-templates.json` once
   (a single sync commit naming those files, no notes) and that the second launch rewrites nothing.
   That second launch is the churn guard, and it is the one thing here that a user would notice as
   a bug rather than as a fix.
