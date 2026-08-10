# Spec 44.8 — Update Notes From Their Template

status: implemented
created: 2026-08-09
epic: 44 (the vocabulary is the space, and the note is a document)
binds: FR-163, UX-DR59
depends on: 44.7 (the provenance keys `keeper.from_template` / `keeper.from_template_id`)
touches: 38.4/38.5 (note history — this story gives it its first verb)

## What was already there, and what was not

The epic's recurring lesson held again, in both directions.

**Already built, and I did not rebuild it.** `keeper-core/src/notes/templates.rs`
already owned the whole placeholder grammar; `notes_ipc::notes_templates`,
`NoteCreateReq.template` and `NotesConfig.default_template` were already wired.
44.7 extended that module rather than adding a second one.

**Already there and dead: nothing.** This is the one story in the wave whose
central value genuinely did not exist. There is no provenance key anywhere before
44.7 — I grepped `vm.rs`, `frontmatter.rs`, `notes_ipc.rs` and `src/lib/ipc/gen/`
— so "which template made this note" was unrecorded and unrecoverable.

**Already there and dead in the thing this story leans on: the note history.**
`NoteHistoryPanel` lists revisions and shows diffs, and there was **no way to act
on one**. No `notes_restore_revision`, no restore button, nothing. The story's
acceptance criterion — "accepting is undoable through the existing note history"
— was unmeetable as written: the existing history could *show* you the note as it
was and could not *put it back*. That gap is closed here (see
[The undo](#the-undo-is-the-notes-own-history-and-it-had-to-be-built)), and the
history panel now does something for every note in the vault, not just for the
ones a template touched.

## The decision this story exists to make

> **A template update re-applies the template's own edit, change by change, and a
> change lands in a note only where the note still says, byte for byte, what the
> template used to say.**

### The candidates, and why the others lose

| Shape | What it buys | Why not |
| --- | --- | --- |
| Replace the note with the new template | Trivial; every note ends up "correct" | It is a deletion with a progress bar. A note somebody wrote in is the thing the template existed to produce. |
| Apply only to untouched notes | Provably safe | Applies to almost nothing. The epic says so in the story text: most notes have been written in, "because that is what a note is for". Useful as a *floor*, useless as the whole rule. |
| Append the new template under the old content | Never destroys | Produces notes with two of every heading. It does not update anything; it litters. |
| Per-note diff the user accepts | Necessary | It is a *surface*, not a rule. Something still has to decide what the diff proposes. Shipped as well — it is the preview — but it does not answer the question. |
| **Re-apply the template's edit where the note still matches** | Handles the written-in note, which is the whole problem; degrades to "untouched notes only" exactly when it must; never removes a line the user wrote | Skips changes over text the author rewrote. That is the correct answer, stated out loud, not a silent failure. |

**Chosen: the last one.** It is the only candidate that is safe *and* does
something in the common case, and it contains the "untouched notes only" shape as
its degenerate case: in a note nobody has touched, every anchor matches and every
change lands.

### Why the destructive reading is unreachable, not merely discouraged

Three structural properties, each tested:

1. **The only lines a change may delete are lines the OLD TEMPLATE wrote.** They
   come out of a diff of old-template against new-template, so a line the user
   typed is never in a `removed` set. There is no code path that removes a line
   from a note without that line being byte-identical to template text.
   (`no_change_ever_proposes_removing_a_line_the_template_did_not_write` checks
   this over a table, and separately checks that every non-template line of the
   note survives the write, in order.)
2. **A change is applied only where its anchor is unique.** Two matches is a
   refusal (`Skip::Ambiguous`), not a coin toss.
3. **`apply` returns `Option<String>`, and the shell writes only what it
   returns.** Declining, selecting only skipped changes, and a blocked note all
   produce `None`. There is no branch in `run_template_update` that writes bytes
   `apply` did not hand it.

And two surface properties:

4. **There is no "apply to all".** One checkbox per note and no control that ticks
   them together. N notes cost N deliberate acts. A select-all is the destructive
   reading with a confirmation step in front of it.
5. **`MAX_OFFER_NOTES = 200`.** Past that keeper declines and says the number. An
   offer nobody can read is not consent.

### Frontmatter is never touched

A template update writes note **body** lines and nothing else. The note's `id`,
`tags`, `order` and reserved `keeper:` map are its own. Two consequences worth
stating:

- A note whose recorded `keeper.from_template` path has gone stale (the template
  was renamed; `keeper.from_template_id` is what found it) is **reported in the
  preview and not repaired**. Repairing it would put a frontmatter write on the
  same button as a body write, and one button with two write shapes is how a
  careful feature grows a way to lose a property.
- **`updated` is not restamped.** That key means "when someone last wrote in this
  note", and keeper propagating a heading is not that. It also keeps the write
  shape body-only, which is what makes the undo diff in the history panel show
  exactly the template's change and nothing else.

### The one placement rule that is not literal

A change the template gained **at its end** goes to the **end of the note**, not
to wherever the template's last line now sits.

Without this the headline case is actively harmful. Template gains `## Actions`
after `## Notes`; the author has written under `## Notes`. Anchoring literally
would slide `## Actions` *above* everything they wrote and silently re-file all of
it under a section they have never seen. Appending re-parents nothing. For an
untouched note the two placements are the same position, so the safe case is
unaffected. (`a_note_written_in_since_keeps_what_was_written_and_still_gains_the_section`.)

## The undo is the note's own history, and it had to be built

`note-history-panel.tsx` and `notes_history` were read before anything was
promised. What they actually record:

- History is a **revwalk of the commits `keeper-sync` writes** (AD-63). There is
  no parallel store, and `notes_history` / `notes_diff` are pure reads.
- A revision exists only **after the vault commits**, which happens on an idle
  debounce (`commit_idle_ms`, 2 s by default) via the cadence — not on save.
- Before this story there was **no restore**. The panel could show and could not
  act.

So two things were required for the AC to be true rather than aspirational:

**1. `notes_restore_revision(vaultId, noteId, rev)`.** `git show <rev>:<path>` →
`write_note`. A read plus the ordinary write path — no new engine API, no second
committer. A restore is itself a revision, so undoing an undo is free. It is
exposed in the history panel behind a two-press control (arm, then write) and is
what the offer's per-note **Undo** calls.

**2. A recoverability gate, because "there is a revision" is not enough.** If a
note has uncommitted changes, the newest revision is *not* the text about to be
overwritten, and restoring it would throw away the user's most recent writing as
well. So eligibility is **git's copy is byte-identical to what is on disk** —
answered by one `git status --porcelain -z` for the whole vault, never one call
per note. A note that fails it is listed **with its changes** and cannot be
ticked, and the sentence says the vault commits on its own within seconds. keeper
never forces a commit from here; `flush()` is asynchronous and a second committer
over one repository is exactly what `notes_vault.rs` already warns against.

`Recoverability::{Committed, Modified, Untracked}` is a pure input to
`plan_note`, so all three arms are tested without a filesystem.

## Where the "before" text comes from

The baseline is captured in **`notes_open`**, not `notes_save`, and the
distinction is load-bearing:

- The autosave fires ~400 ms after typing stops. A save-time baseline would be
  "the text before the last burst of keystrokes", and the offer would show the
  tail of an edit rather than the edit.
- git cannot supply it either: at the moment a template is saved, `HEAD` is
  usually the version before the *previous* edit, so a diff against it would
  re-offer changes the user already decided about last week.

`TEMPLATE_BEFORE` therefore holds "the template as you opened it", per vault and
path, bounded at 32 entries, and does not survive a restart. **keeper offers to
propagate an edit it watched happen, never one it inferred** — an external edit in
Obsidian produces no offer, and that is stated rather than papered over.

## I/O matrix

### `made_from(provenance, template)` — the finder

| Note records | Template has | Result | Why |
| --- | --- | --- | --- |
| id `A`, path `p` | id `A`, path `p` | `ById` | |
| id `A`, path `p` | id `A`, path `q` | `ById` | The template moved; the id survives the rename. |
| id `A`, path `p` | id `B`, path `p` | **no match** | A template deleted and replaced at the same path does not adopt the old one's notes. |
| path `p` only | id `A`, path `p` | `ByPath` | A hand-written note, or one from before 44.7. All the evidence there is. |
| path `q` only | id `A`, path `p` | no match | |
| nothing | anything | no match | |

Provenance is read from the **index**, not from files:
`provenance_from_index(&entry.fields)` inverts `FieldValue::index_string`'s
one-level-map rendering (`"key: value"` joined by `FIELD_LIST_SEPARATOR`). The
round trip is asserted against the real renderer, never a hand-written string. The
cost of this: a ten-thousand-note vault is scanned in memory with **zero file
reads** to find candidates; only the candidates are opened.

Two things 44.7 changed underneath this story, both absorbed without a code
change here and both worth writing down:

- **`is:template` now means the frontmatter tag, not the `templates/` directory.**
  It used to be `rel.starts_with("templates/")`, which is the directory-ownership
  model AD-82 rejects. The baseline capture in `notes_open` already asked
  `templates::is_template(&fm)`, so it agrees with the new rule by construction —
  a template tagged anywhere in the vault now gets a baseline and can therefore
  make an offer.
- **`templates::Expanded` gained `properties`** (the template's own frontmatter,
  minus six private keys), so creating a note now copies properties as well as
  body and tags. A template *update* still copies neither properties nor tags:
  see [Deliberately NOT done](#deliberately-not-done). The field exists; this
  story reads only `.body`, deliberately.

### `changes(old, new)` — the diff

| Input | Output |
| --- | --- |
| identical texts | `[]` |
| a heading rewritten | one change, `removed: ["## Notes"]`, `added: ["## Observations"]` |
| a section appended | one change, `removed: []`, `added: ["", "## Actions"]`, `before` = the template's tail |
| either side > `MAX_DIFF_LINES` (600) | one change whose `removed` is the whole old text — degraded, still safe: it lands only where the old template survives contiguously and once |
| a template edit that touched only its own frontmatter | `[]` (`expand` splits the block off before diffing) |

### `plan_note` / `locate` — one change against one note

| Note state | Outcome | Sentence shown |
| --- | --- | --- |
| still matches, one place | `Applies { at }` | "Lands at line N" |
| author rewrote that part | `Skipped(Diverged)` | "You have written over this part of the note, so keeper left it as you wrote it." |
| the anchor occurs twice | `Skipped(Ambiguous)` | "This text appears more than once in the note, so keeper cannot tell which place the template means." |
| nothing to anchor to (old template was empty) | `Skipped(Unanchored)` | "The template gives keeper nothing in this note to position this against, so keeper will not guess where it goes." |

Anchors are tried longest first — two-sided (3, 2, 1 lines), then before-only,
then after-only, then the `removed` lines alone. A shorter anchor matches a
superset of the places a longer one does, so the search takes the **longest anchor
that matches at all** and refuses if that one is not unique. An anchor made only
of blank lines is passed over: matching whitespace is a coin toss dressed as a
decision.

### `apply(body, plan, accepted)`

| Selection | Result |
| --- | --- |
| `[]` | `None` — nothing to write |
| only skipped changes | `None` |
| any selection, note blocked | `None` |
| out-of-range index | ignored, no panic |
| one appliable change | `Some(body')` |
| several | all spliced, bottom-up; overlapping spans drop the later one |
| result equals the input | `None` |

### `offer(...)` — the refusal precedence

`no notes` → `too many notes` → `nothing changed` → `nothing applies` → an offer.
Each is a different fact and gets its own sentence, because "nothing happened" is
the message this epic has already shipped twice by accident.

## Edge cases

| Case | Behaviour |
| --- | --- |
| Note with CRLF line endings | Matched with the carriage return trimmed; inserted lines get the note's own terminator. A CRLF note stays CRLF. |
| Note with no trailing newline | Does not gain one. `split_lines` drops the phantom element the terminator produces and `rejoin` puts the real terminator back. |
| Placeholder in the template (`{{date:YYYY-MM-DD}}`) | Both sides are expanded with the **note's own** context (`created` from its frontmatter, its title, its id) before diffing, so a year-old journal entry's date line is recognised as the template's. A placeholder is never written into a note as literal text. |
| `{{cursor}}` in an added line | Consumed by `expand`; the offset is discarded — a bulk update has no caret to place. |
| The template edited is itself made from a template | The template being edited is excluded from its own candidate list. |
| Template renamed since the note was made | Found by id; the stale path is shown in the preview and the note's frontmatter is left alone. |
| Note deleted between preview and apply | Skipped with a sentence naming it. |
| Note edited between preview and apply | The plan is **rebuilt from disk** at apply time. A change that no longer matches is skipped with a sentence; the request carries indices, never text. |
| keeper restarted between the edit and the apply | The apply is refused with a sentence; the baseline is per-session by design. |
| No git, or not a repository | `uncommitted_paths` returns `None` ⇒ every note reads as `Untracked` ⇒ every note is blocked. Unprovable recoverability is treated as none. |
| A user's own top-level `keeper:` map with unrelated keys | No provenance; the note is not a candidate. |

## Every place keeper declines to act, and where it says so

Per DW-162 — a story shipped green twice while doing nothing on the owner's
machine, the second time behind a `tracing::debug!` the app cannot print because
`RUST_LOG` is unset — **every refusal here is INFO or above and also reaches the
screen**:

| Decline | Log | Screen |
| --- | --- | --- |
| Not a template / keeper has no baseline | none (not a refusal — the command returns `null`) | nothing, deliberately: "this is not a template" must not read as a refusal |
| No note came from this template | `tracing::info!` | the sentence, in the editor's status line |
| More than 200 notes | `tracing::info!` | the sentence, naming the count |
| The template's text did not change | `tracing::info!` | the sentence |
| Nothing still matches | `tracing::info!` | the sentence |
| An offer was made | `tracing::info!` with the note count | the banner |
| Applied | `tracing::info!` with updated/skipped counts | the result list |
| A note skipped during apply | — | its own sentence in the result list |

## Tests, and the mutations that prove them

```
cargo test -p keeper-core --lib notes::template_update    42 passed, 0 failed
bun run vitest run src/components/notes/template-update-offer.test.tsx    11 passed
bun run vitest run src/components/notes/note-history-panel.test.tsx        4 passed
```

The four suites that mount the real `NoteEditor` — `attachments-panel`,
`format-toolbar`, `new-note-caret`, `editor/tab-wiring` — were run because this
story mounts a new component inside it: 33 passed. `tsc --noEmit` reports nothing
against any file this story touches.

### How these verdicts were obtained, because the first set were worthless

The first sweep ran from `/tmp/mutate.py`. That path is shared: a sibling agent
overwrote it mid-run, my background job executed their script, and their results
landed in my log. Worse, a killed run left **the M3 mutant applied in the working
tree**, so every verdict after it was measured against broken code — which
announced itself as one unrelated test failing under every single mutation. Two
jobs were also writing to one log, which is how `M11` came back both CAUGHT and
SURVIVED.

All of it was discarded. The harness now lives at a private path, takes one
sha256 pristine copy, runs **one** mutation at a time, verifies the file is
byte-identical to pristine after each, and runs the unmutated suite as a baseline
**before and after** the sweep — both `GREEN []`. Its filter is
`notes::template_update`, exactly the scope this story owns, so a sibling
mid-sweep in `notes::csv` cannot be read as my failure.

### Three mutations survived the clean sweep. All three were real.

**M5 — the trailing-append rule removed — survived, and it was the worst hole
available.** It is the most consequential decision in the story and the test
asserted `updated.contains("## Actions")` plus "the author's lines are still
there". Both remain true under literal placement: nothing is deleted, the heading
is present — it has simply moved above everything the author wrote and re-filed
all of it under a section they have never seen. *Survival, not position, was what
the test checked.* Fixed by asserting the whole resulting document.

**M7 — blank-only anchors allowed — survived because the guard had become
unexercised.** It earned its keep before the phantom-line fix; afterwards no test
distinguished it. Fixed with `a_blank_line_is_not_an_anchor_even_when_it_is_the_only_unique_one`,
which builds the one shape that separates the rules: a note where the author
rewrote both sides of a change and the only surviving context is a single empty
line that happens to occur exactly once. The same test then shows the change
landing when a real anchor survives, so the rule is about blankness and not about
that shape of change.

**M8 — `apply`'s empty-selection early return removed — survived because it was
an equivalent mutant, and the honest response was to delete the code.**
`split_lines`/`rejoin` round-trip exactly, so a run with no accepted span
reconstructs the body byte for byte and the final `updated != body` comparison
returns `None` on its own. The early return was an unreachable second answer to a
question already answered. A guard no test can distinguish from its absence is a
guard the next reader will trust for a promise it does not make. Removed; M8 was
re-pointed at the comparison that actually carries the guarantee, and is caught by
nine tests.

### The table

Every row below is from the guarded sweep, green baseline at both ends.

| # | Mutation | Caught by |
| --- | --- | --- |
| M1 | `made_from` ignores the id and matches on path alone | `a_note_from_a_different_template_is_not_found_even_at_the_same_path`, `a_note_stamped_with_this_templates_id_is_found_after_a_rename` |
| M2 | `apply` ignores `plan.blocked` | `a_blocked_note_writes_nothing_for_any_selection`, `a_note_the_vault_has_not_committed_cannot_be_updated` |
| M3 | `locate` takes the first of several matches instead of refusing | `a_change_that_could_land_in_two_places_is_refused_rather_than_guessed` |
| M4 | `plan_note` diffs raw template text instead of expanding it | `a_placeholder_is_compared_and_written_as_this_note_has_it`, `an_edit_to_only_the_templates_own_frontmatter_changes_nothing` |
| M5 | the trailing-append rule removed (literal placement everywhere) | `a_note_written_in_since_keeps_what_was_written_and_still_gains_the_section` — **only after that test was strengthened; it survived first** |
| M6 | `split_lines` keeps the phantom trailing element | 8 tests, including `an_untouched_note_takes_every_change` and `a_crlf_note_keeps_its_line_endings` |
| M7 | blank-only anchors allowed | `a_blank_line_is_not_an_anchor_even_when_it_is_the_only_unique_one` — **new test; it survived first** |
| M8 | `apply` drops the `updated != body` comparison | 9 tests, including `declining_returns_no_text_to_write_at_all` — **re-pointed after the original mutant proved equivalent** |
| M9 | `offer` never takes its first declining branch | `each_reason_for_declining_gets_its_own_sentence` |
| M10 | `provenance_from_index` stops reading the id | `provenance_reads_back_out_of_the_index_exactly_as_it_went_in` |
| M11 | the CRLF terminator is dropped on inserted lines | `a_crlf_note_keeps_its_line_endings` |
| F1 | the dialog sends every note, ticked or not | `sends only the ticked notes, and only their appliable changes` |
| F2 | the dialog sends skipped changes too | `sends only the ticked notes, and only their appliable changes` |
| F3 | a blocked note becomes tickable | `cannot tick a note keeper could not put back, and says why`, `cannot tick a note where nothing would land` |
| F4 | a `declined` offer is swallowed instead of printed | `prints keeper's own sentence when it declines, rather than an empty dialog` |
| F5 | the settle window removed (asks on every autosave) | `does not ask while the editor is still settling` |
| F6 | restore writes on the first press | `writes the selected revision back, but only on the second press` + 2 |
| F7 | restore always writes the newest revision | `restores the revision the reader selected, not the newest one` |

11 Rust mutations and 7 frontend mutations; all 18 caught, zero surviving. The
frontend verdicts are `returncode`-based rather than scraped from vitest's
output, so the non-TTY `FAIL … > name` formatting that manufactured false
survivors for a sibling cannot reach them; the failing test names printed above
each verdict were read by eye.

### What these tests cannot prove, and what the macOS gate owes this story

The `keeper` shell crate does not build on Linux — `glib-sys` and `gobject-sys`
build scripts fail against the missing system libraries, before reaching a line
of keeper's own source. So this story's shell half has **never been compiled
anywhere**: `notes_template_update_preview`, `notes_template_update_apply`,
`notes_restore_revision`, `build_offer`, `run_template_update`, `note_ctx`,
`head_rev_of`, `revision_text`, `uncommitted_paths`, and the one line added to
`notes_open`. Every decision they call into is in `keeper-core` and is proved
above; what is unproved is the plumbing — and two of these functions are on a
**write** path, which is the sharper end of the same caveat.

| Uncompiled symbol | The question a compiler settles |
| --- | --- |
| `build_offer`, `run_template_update` | do `&&IndexEntry` deref-coercions in `NoteInput`'s struct-field positions type-check; does `&Arc<IndexSnapshot>` coerce to `&IndexSnapshot` at the `spawn_blocking` boundary |
| `uncommitted_paths` | does `git status --porcelain -z` really emit `XY <path>\0` with the rename origin in the following field, and does `strip_prefix(subfolder/)` land on vault-relative paths |
| `revision_text` | does `git show <rev>:<subfolder>/<path>` resolve for a note in a vault subfolder |
| `head_rev_of` | does `git log -n1 --format=%H -- <path>` return empty (not an error) for an untracked note |
| `notes_open` + `TEMPLATE_BEFORE` | is the baseline actually taken for a tagged template, now that `is:template` means the tag |

**What was done instead of compiling, because "it looks right" is not a check.**
While this story was finishing, story 44.6 found a `IpcErrorCode::InvalidInput`
in its own uncompiled shell code — a variant that does not exist — after 444
green Rust tests and 443 green frontend tests had said nothing about it, because
none of them can reach the `keeper` crate. So every symbol this story's shell
half names was read against its definition rather than against memory:

- No `IpcErrorCode` anywhere in the new code. Every refusal goes through the
  existing `notes_error(NotesError::…)` helper, and the three variants used —
  `Template(String)`, `NotFound(String)`, `VaultUnknown(String)` — were checked
  against `notes/mod.rs`.
- `IndexSnapshot::entries(&self) -> &[IndexEntry]`, and the four `IndexEntry`
  fields read (`id`, `path`, `title`, `fields: BTreeMap<String, String>`),
  checked against `notes/index.rs`.
- `templates::is_template(&Frontmatter) -> bool`, `TemplateCtx { title, id,
  now_local }` and `Frontmatter::{parse -> (Frontmatter, usize), as_string ->
  Option<&str>}` checked against their definitions.
- `vault.local_path` and `format!("{}/{rel}", vault.config.subfolder)` are copied
  verbatim from `revisions` and `diff` a few lines above in the same file, which
  is the strongest evidence available here short of a build.

That audit cannot find a borrow error or a coercion failure; it can find the
class of mistake 44.6 hit, and it did not find one.

**What the gate has to check by hand, both halves each time.** DW-162 is the
story of a decision that existed only in a `debug!` the packaged app cannot
print, so checking the screen without checking the log would repeat it:

1. Edit a template's body, wait ~4 s → the banner appears naming the note count,
   **and** Console.app carries `notes: offering a template update`.
2. Edit a template nothing was made from → the banner carries the "No note in
   this vault records…" sentence, **and** the log carries the matching
   `template update declined` line at INFO.
3. Open the review dialog, tick nothing, press `Not now` → `git status` in the
   vault is unchanged. This is the "declining changes nothing on disk, byte for
   byte" criterion; the pure half is proved, the on-disk half is this.
4. Tick one note, apply → only that note's file changes, its frontmatter block is
   byte-identical (including an unchanged `updated`), and the added lines are in
   the right place.
5. Press its `Undo` → the file is byte-identical to what it was before step 4.
6. Open that note's history → the pre-update revision is listed and
   `Restore this version` writes it back on the second press.
7. Edit a note, do not wait for the commit, then open the offer → that note is
   listed, greyed, with the "has changes this vault has not committed yet"
   sentence. Wait a few seconds and reopen → it is tickable.
8. Edit a template in Obsidian instead → no banner at all, and no log line. The
   baseline is per-session by design and this is the case that proves it is not
   silently inventing one from git.

## Cost

- **Finding candidates:** in-memory scan of the index, zero file reads.
- **Building the offer:** one `git status --porcelain -z` for the vault, plus one
  file read per candidate note, capped at 200.
- **When it runs:** once per settled edit of a template — 4 s after the last
  successful save, and never twice for the same content revision. Not on every
  autosave; that would put a vault scan behind every pause for thought.
- **Applying:** one file read and at most one write per selected note, plus one
  `git log -n1` per note to resolve its undo revision.

## Deliberately NOT done

- **No "apply to all", and no select-all checkbox.** Argued above. This is the
  single most likely review comment and the answer is no.
- **No word-level or intra-line diff.** A change whose anchor is a fragment
  matches in far more places than it should. Lines are the unit a person means by
  "the template changed" and the unit that can be located safely.
- **No three-way merge with conflict markers.** Writing `<<<<<<<` into somebody's
  note is a worse outcome than leaving the change out and saying so. The
  conflict-copy machinery (AD-43) exists for genuine two-sided divergence; a
  template edit is not that.
- **No repair of a stale `keeper.from_template` path.** Reported, not written. One
  button, one write shape.
- **No propagation of the template's tags or properties.** 44.7's `Expanded` now
  carries `properties`, so it would be one line to push a template's new
  frontmatter key into every note made from it. No: a note's properties are the
  note's — its `order`, its tags, its own fields — and a bulk frontmatter write
  is the one shape of this feature that could cost somebody a value with no diff
  to read first. A template edit propagates prose.
- **`updated` is not restamped**, for the reason given above.
- **No offer for a template edited outside keeper.** The baseline is captured on
  open. An Obsidian edit produces no offer; keeper does not reconstruct one from
  git, because the reconstruction would be wrong in the ordinary case.
- **No persistence of the baseline across a restart.** Same reason. A stale
  baseline would offer a diff spanning several sessions.
- **No undo of a whole batch behind one button.** Undo is per note, for the same
  reason there is no apply-to-all.
- **No new space-query predicate for "notes from this template".** `IndexEntry.fields`
  flattens a one-level map into a single `keeper` entry, so `field:keeper.from_template`
  is not addressable today. Making it addressable means changing how the
  reconciler flattens maps, which is a change to every `field:` query in the
  vault — a story of its own, not a rider on this one. Filed below.
- **No `keeper-sync` change.** All git use here is the existing read-only
  `git_out` escape hatch that `revisions` and `diff` already use. No second
  committer.

## Deferred work

- **DW-168 — `IndexEntry.fields` cannot address a nested key.** The reconciler
  flattens a one-level frontmatter map through `FieldValue::index_string`, so a
  note's whole `keeper:` map lands under the single key `keeper` and the space
  DSL's `field:` predicate cannot reach `keeper.from_template`. 44.8 works around
  it with `provenance_from_index`, which inverts that flattening for its own two
  keys. The general fix — index a one-level map as `parent.child` entries — would
  let a user write a space for "everything from the journal template", and would
  need `RESERVED_FIELD_PREFIX`'s "a user's `keeper:` map indexes under the bare
  key" note revisited at the same time.
- **DW-169 — the restore verb is now general and under-surfaced.**
  `notes_restore_revision` exists and works for any note and any revision, but it
  is only reachable from the history panel and from a template update's Undo.
  A conflict copy, the trash, and the "this note isn't on disk any more" banner
  are all places a restore belongs.
