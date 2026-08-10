# Story 44.7 — Templates

status: implemented
epic: 44 — The vocabulary is the space, and the note is a document
binds: FR-161, FR-162, AD-82
depends on: 44.6 (New Note — owns the creation path)
feeds: 44.8 (Update Notes From Their Template — consumes the provenance keys)

---

## What was already there, and dead

The epic's recurring lesson landed again, harder than in waves 1–2. **Story 44.7 was
roughly seventy percent already built and unreachable.** Before writing anything:

| Already existed | Where | State |
|---|---|---|
| The placeholder expander (`{{date:FMT}}`, `{{time:FMT}}`, `{{title}}`, `{{cursor}}`, `{{id}}`) with a moment-token renderer | `keeper-core/src/notes/templates.rs` | Working, tested, FR-100 |
| `notes_templates` — lists every template in the vault | `keeper/src/notes_ipc.rs` | Working |
| `NoteCreateReq.template`, `NoteTemplateVm` | `keeper-core/src/notes/vm.rs` | Working |
| `NotesConfig.default_template` — a **vault-level** default template | `keeper-sync/src/profile.rs` | Read, written back, and applied |
| `is:template` query flag | `keeper-core/src/notes/query.rs` | Working |
| A missing template degrading to a plain note, **at `INFO`** | `notes_ipc.rs::template_source` | Already correct — not re-added |

So the story was not "build templates". It was four things, two of them defects.

### Defect 1 — the copy got the template's frontmatter pasted into its body

`template_source` returned the template file's **raw text** and handed the whole thing to
`templates::expand`. The result was written as the new note's *body*, underneath a fresh
frontmatter block. Every note ever created from a template therefore looked like:

```markdown
---
id: 01NEW…
created: 2026-08-09T…
---

---
id: 01TEMPLATE…
tags: [template]
---
# Standup
```

A literal `---` block, in the body, carrying the template's identity and its `template`
tag as text. Nobody could have used this feature and not noticed — which is the evidence
that nobody had. This is why the AC "the copy carries every tag except `template`" could
not previously be satisfied by any spelling: the tags were not being *carried*, they were
being smuggled through as prose.

### Defect 2 — `is:template` was computed from a directory keeper owns

`notes_vault.rs::parse_note` set the flag with `rel.starts_with("templates/")`.

That is precisely what **AD-82 rejects**: *"Not a new file type, not a directory keeper
owns."* The `template` tag existed in the vocabulary and decided nothing. The flag now
comes from `templates::is_template(&fm)` — the frontmatter tag — with the directory
grandfathered (see *Compatibility*, below).

### The two genuinely missing things

3. A **space-level** default template (`keeper.template`). Only the vault-level one existed.
4. **Provenance** — nothing recorded which template made a note. 44.8 cannot exist without it.

---

## Positions taken

### One renderer, two spellings — and why "one grammar" could not be obeyed literally

The brief said the placeholders are `{yyyy}`/`{mm}`/`{dd}`, *"one substitution grammar in
this app, not two"*. Grepping found **two grammars already shipped**, in different domains:

| Grammar | Owner | Domain |
|---|---|---|
| `{{date:FMT}}`, `{{title}}`, `{{cursor}}`, `{{id}}` | `notes::templates` (FR-100) | **body** text; Obsidian's own Templates-plugin syntax |
| `{yyyy}`, `{yy}`, `{mm}`, `{dd}`, `{HH}`, `{MM}`, `{SS}`, `{slug}`, `{seq}` | `recording::path_template`, `notes::naming::journal_path` | **paths** — folder and file names |

Neither could simply be deleted. The double-brace set is what Obsidian writes, and the
constraint *"a note keeper writes stays a note Obsidian renders"* extends to templates
authored in Obsidian. The single-brace set is what the owner has already learned from the
recording destination field.

**The path renderers are structurally unreusable for a body**, which the brief asked to be
said rather than worked around. `journal_path` normalises separators, drops `..` and empty
segments and appends `.md`; `PathTemplate::render` collapses folder components that render
empty and refuses reserved device names. Both are correct for a filename and both destroy
a document.

So: **one renderer, `notes::templates::expand_body`, resolving both spellings.** Not a
second module, not a second walk. A user who learned `{yyyy}/{mm}/{dd}` in the destination
field gets it in a body; a template authored in Obsidian keeps working.

`{mm}` is the **month** and `{MM}` is the **minute**; inside `{{date:…}}` moment reverses
them. That collision is pre-existing and documented at `path_template`, but this is the one
file where both are in scope, so it is stated at the top of the module and asserted by
`the_two_vocabularies_disagree_about_mm_and_both_are_honoured`.

Single-brace `{title}`, `{slug}` and `{seq}` are deliberately **absent**: the first two
already have a body spelling in `{{title}}` (two spellings of one value in one document is
the divergence being avoided) and `{seq}` is a path concept that disambiguates a colliding
folder and means nothing in a paragraph.

### The marker is the frontmatter tag, never an inline `#template`

`is_template` reads `tags:` only, not `tags::note_tags` (which unions in the body's inline
tags). This is load-bearing: **the body is copied verbatim**, so an inline `#template`
would ride into every copy and make each one a template of itself — the exact failure
AD-82 names. Keeping the marker in frontmatter lets the copy drop it by *not copying a
property*, instead of editing somebody's prose on the way past. A note *about* templates
that mentions `#template` in a sentence stays a note.

### Provenance: two keys, inside the reserved namespace

```yaml
keeper:
  from_template: templates/journal-entry.md
  from_template_id: 01J8ZQ…
```

Three decisions, each contested with a peer and settled:

- **Inside `keeper:`, not a bare top-level `template:`.** W3TemplateUpdate proposed the
  bare key on Obsidian-readability grounds. Overruled on one architectural ground: keeper's
  own bookkeeping lives under the reserved map and nowhere else (`keeper.capture`,
  `keeper.default`, `keeper.space`, `keeper.sort`, `keeper.limit`, `keeper.icon`,
  `keeper.order`). A top-level `template:` puts a keeper-written key in the *user's*
  property namespace where it can collide with a property they already keep, and nothing
  marks it as machine-written.
- **`from_template`, not `template`.** On a **space** note `keeper.template` means "notes
  created here start from this"; on an ordinary note it would mean "was made from this".
  One spelling, opposite meanings, one vault, is a trap for whoever reads the vault next.
- **Both a path and an id.** Main's steer was explicit that a path breaks on rename, which
  in a synced vault is not hypothetical. Matching on path alone means the day someone
  renames a template, 44.8's finder returns zero notes and reports success — a silent
  nothing, the failure this epic has shipped twice. The id is one frontmatter line, is
  already in the template's own frontmatter, and turns that silent zero into a hit. 44.8
  matches id first, path second. **When only the path is present** — a hand-written note,
  or one created before this landed — the path is all there is, and a rename orphans it;
  44.8 owns the sentence for that case.
- **No version and no hash.** A hash would let 44.8 claim "unchanged since creation", but a
  note's body is edited constantly so the hash goes stale within seconds while still
  reading as authoritative. 44.8's "edited since creation" rule is derived from real
  content. Agreed with W3TemplateUpdate, who does not want one.

### Properties cross over, six keys do not

The owner asked for notes that render beautifully "using the structure the app can already
show — headings, a properties block, a table". A properties block on the *created* note
means the template's own frontmatter properties must cross over, which is also what a user
expects. So `Expanded.properties` carries them in **source order** (Obsidian renders
properties in file order; an author who put `project` above `status` arranged that).

Six keys never cross:

| Key | What copying it would break |
|---|---|
| `id` | Two notes sharing an id — a pin, an unread mark or a sync conflict lands on the wrong file |
| `created`, `updated` | The copy would carry the template's history |
| `title` | Every note named `Daily Template` |
| `keeper` | Would carry `keeper.template` onto an ordinary note, and would fight the provenance the caller writes into the same map |
| `tags` | **The sharpest one.** Tags cross via `Expanded.tags`, where the marker is stripped. Leaving `tags` in the properties hands the caller the raw list a *second* time — marker included — and the copy is a template after all. A test caught exactly this during implementation (M5 below). |

### No callouts in the shipped templates

Checked before writing them, as instructed. `src/components/notes/editor/live-preview.ts`
decorates `Blockquote` (`cm-lp-quote`) and has no callout handling; `HIDDEN_MARKS` hides
`QuoteMark` and nothing else. A `> [!note]` therefore renders as a blockquote with a
literal `[!note]` line in the app that wrote it, and as a callout in Obsidian. **A shipped
template that looks broken in keeper is worse than a plain one**, so the bodies use only
ATX headings, aligned GFM tables, task lists and paragraphs.
`no_shipped_template_uses_syntax_the_apps_own_renderer_cannot_draw` is the gate, not this
paragraph.

GFM tables *are* safe: the parser is `markdown({ base: markdownLanguage })` (GFM on), live
preview leaves the table as aligned source — readable, which is exactly the trade 44.9 made
for its table builder — and Obsidian renders it.

### The shipped templates add no tags, and that is the interesting part

Each of the three spaces that most wants a template selects on something that is **not a
tag**:

| Space | Selects by |
|---|---|
| Inbox | `is:untagged` |
| Journal | the `journal/` path |
| Recordings | the `session:` frontmatter key |

An Inbox template that helpfully tagged its notes `inbox` would file every one of them
**straight out of the Inbox** — the space that offered the template would be the one space
the note could not appear in. So all three ship with `tags: [template]` and nothing else,
and `every_shipped_template_is_a_template_and_makes_notes_that_are_not` asserts it with
that reason in the failure message.

---

## Contract

All in the **existing** `keeper_core::notes::templates` — no new module. A `template.rs`
beside `templates.rs` is a rename waiting to half-land.

```rust
pub const TEMPLATE_TAG:          &str = "template";           // the marker (AD-82)
pub const SPACE_TEMPLATE_KEY:    &str = "template";           // in a SPACE note's keeper: map
pub const FROM_TEMPLATE_KEY:     &str = "from_template";      // in a NEW note's keeper: map
pub const FROM_TEMPLATE_ID_KEY:  &str = "from_template_id";
pub const TEMPLATES_DIR:         &str = "templates";          // a default, not a rule
pub const TEMPLATE_LEDGER_REL:   &str = ".keeper-templates.json";

pub struct Expanded { body, caret, tags, properties, source_id }
pub struct Provenance { path: Option<String>, id: Option<String> }

pub fn expand(source, ctx) -> Expanded                   // NOTE-level: splits frontmatter, drops the marker
pub fn expand_body(text, ctx) -> (String, Option<usize>) // string-level; 44.8 re-renders through this
pub fn is_template(fm) -> bool
pub fn space_default_template(fm) -> Option<String>
pub fn provenance(source) -> Provenance                  // total, never errors
pub fn provenance_pairs(rel, source_id) -> Vec<(String, FieldValue)>
pub fn missing_template_notice(named) -> String          // finished sentence, composed in Rust
pub fn seed_templates(vault, mode) -> SeedOutcome
pub fn report_template_seed(&SeedOutcome) -> (Level, String)
```

The old string-level `expand` was **renamed** to `expand_body`, and `expand` is now the
note-level entry point, per Main's naming. Two call sites, both migrated; no shim left.

### Resolution order for a create (three rungs, most specific first)

`req.template` → the **space's** `keeper.template` → the **vault's** `default_template` → none.

A caller naming one is answering a question the space only implied; a space naming one is
answering a question the vault only implied.

### Seeding

`seed_templates` **reuses** `default_spaces::{SeedVault, SeedMode, SeedOutcome, parse_ledger,
REPORT_FLOOR}` rather than declaring a second port: both seeds do the same dangerous thing —
write notes into somebody's real vault, on removable media, through the sync engine — and
must agree about what "absent" and "could not tell" mean. Only the ledger, the contents and
the wording differ. `default_spaces.rs` itself was not edited.

Its **own** ledger (`.keeper-templates.json`), because a vault seeded by the previous build
has the spaces ledger and no templates ledger, and that state must read as "offer the
templates". One ledger could not say both.

---

## I/O matrix

### `expand(source, ctx)` — ctx at `2026-08-02T14:35:09+02:00`

| Input | `body` | `tags` | `properties` | `source_id` | `caret` |
|---|---|---|---|---|---|
| `tags: [journal, Daily, template, work/notes]`, body `# Hi` | `# Hi` | `[journal, daily, work/notes]` | — | the id | `None` |
| `tags: ["#Template", " TEMPLATE "]` | body | `[]` | — | id | `None` |
| `tags: [template, template/daily]` | body | `[template/daily]` | — | id | `None` |
| `tags: template` (bare scalar, Obsidian style) | body | `[]` | — | id | `None` |
| body with no placeholders | **byte-identical** | | | | `None` |
| `{yyyy}-{mm}-{dd} {HH}:{MM}:{SS} {yy}` | `2026-08-02 14:35:09 26` | | | | |
| `{{date:MM-mm}}` | `08-35` | | | | |
| `{mm} {MM}` | `08 35` | | | | |
| `{{yyyy}}`, `{{ dd }}` | unchanged, literal | | | | |
| `{n} {} {seq} {slug} {title} {YYYY}` | unchanged, literal | | | | |
| `{ {yyyy}` | `{ 2026` | | | | |
| `a { b` | `a { b` | | | | |
| `half open {{title` | literal, verbatim tail | | | | |
| `now_local` unparseable | every date placeholder left visible, both spellings | | | | |
| no frontmatter at all | whole file is the body | `[]` | `[]` | `None` | |
| empty source | `""` | `[]` | `[]` | `None` | `None` |
| `status: draft`, `project: Acme` + the six private keys | body | from `tags` | `[status, project]` in source order | | |
| a frontmatter value the parser cannot model | body | | key **skipped**, never guessed | | |
| `{yyyy}-{mm}{{cursor}}!` | `2026-08!` | | | | `Some(7)` — byte index into the **expanded** text |

### `provenance(source)` — total, never errors

| Input | `path` | `id` |
|---|---|---|
| `keeper: { from_template: …, from_template_id: … }` | set | set |
| `keeper: { capture: true, from_template: …, from_template_id: … }` | set | set |
| `keeper: { from_template: "   " }` | `None` | `None` |
| `keeper: not-a-map` | `None` | `None` |
| `keeper: { capture: true }` | `None` | `None` |
| no frontmatter / empty / `# just a body` | `None` | `None` |

### `provenance_pairs(rel, source_id)`

| Input | Output |
|---|---|
| path + id | both pairs, path first |
| path, `source_id = None` | one pair — never a pair with a hole in it |
| `rel = ""` or whitespace | `[]` |

### `space_default_template(fm)`

| `keeper:` holds | Result |
|---|---|
| `template: templates/journal.md` | `Some("templates/journal.md")` |
| `template: ""` / `"   "` | `None` — cleared and never set are one state |
| `template: 7` | `None` |
| no `template` key / `keeper: nonsense` / no frontmatter | `None` |

### `seed_templates(vault, mode)`

| Vault state | Mode | Outcome |
|---|---|---|
| fresh (no ledger, no `templates/`) | FirstRun | `Wrote([inbox-note, journal-entry, recording-notes])`, ledger recorded |
| already seeded | FirstRun | `AlreadySatisfied` |
| already seeded, one template **deleted** | FirstRun | `AlreadySatisfied` — a deleted default stays deleted |
| already seeded, one deleted | Restore | `Wrote([the one])` — the ledger gets no vote when the user asks |
| `templates/Journal Entry.md` exists (user's own) | FirstRun | writes the other two; the user's file untouched, name folded through `naming::slug` |
| ledger present and unparseable | FirstRun | `Blocked(sentence naming the file)`, **writes nothing** |
| ledger present and unparseable | Restore | writes all three |
| `templates/` unlistable (sleeping USB) | FirstRun | `Blocked(sentence naming the directory)`, writes nothing |
| one write fails partway | FirstRun | `Stopped { written }`, and **what landed is recorded** so the next run does not double it |

### The space editor

| State | Rendered |
|---|---|
| space has no template | select on "No template" |
| space names a template in the list | that option selected |
| space names a template **not** in a list that loaded | option rendered as `<path> — not in this vault`, selected, plus `SPACE_TEMPLATE_MISSING` in red |
| space names a template and the list **failed to load** | option rendered as the bare path, selected, **no** red sentence |
| save with a template chosen | `template: "<vault-relative path>"` |
| save with "No template" | `template: null` — never `""` |
| save without touching a missing template | the stored path is **kept**, not dropped |

---

## Edge cases and the reasoning behind each

**A note created from a template that has since been deleted is still created.** The note
is worth more than the scaffold. `TemplateChoice::Missing` is a third state distinct from
`None`, because "nobody named one" and "one was named and is gone" produce the same note
and must not produce the same silence. The user gets `missing_template_notice` — a finished
sentence composed in Rust, naming the path and saying *both halves* ("…so this note was
created without it"), because a sentence that names only the failure reads as a failure
while the note sits right there. It travels in `NoteCreateVm.notices` (44.6's channel) and
is **also** logged at `INFO`.

**`INFO`, never `debug!` (DW-162).** Nothing sets `RUST_LOG` in the packaged app, so
`tracing::debug!` is dead code there. Every path in this story that can decline to act says
so at `INFO` or above: the missing-template log line, and all five seed outcomes.
`every_seed_outcome_is_reported_at_a_level_the_app_can_actually_print` asserts the level
against `default_spaces::REPORT_FLOOR` — a gate, not a promise in a comment.

**An absent ledger is `Ok(Some(empty))`, never `Ok(None)`.** The first draft returned
`Ok(None)` for a missing file, which `plan_templates` reads as "keeper could not tell" — so
a **fresh vault reported `AlreadySatisfied` and wrote nothing**. Green on every test, silent
on the owner's machine: precisely this epic's recurring failure, caught by
`a_first_run_on_a_fresh_vault_writes_all_three_and_records_them`. The invariant is now stated
in the function's doc comment, mirroring `default_spaces::read_ledger`.

**No placeholder inside a table cell.** Learned here. The journal's Log table first read
`| {HH}:{MM} |` — nine source characters padded to a nine-wide column, and five characters
once expanded. The template file looked aligned and **every note made from it did not**. A
placeholder's rendered width is not its source width, so a cell holding one can be aligned
in the scaffold or in the note but never both — and the note is what a person reads. Gated
by `a_shipped_templates_tables_are_aligned_after_expansion`, which asserts pipe columns on
the **expanded** text.

**An unknown `{{…}}` token is not rewritten by the single-brace pass.** `{{yyyy}}` would
otherwise come back as `{2026}` — an unknown silently rewritten, the opposite of the closed
set's promise. `push_literal` runs on literal runs only, never on re-emitted unknown tokens.

**An unclosed `{` resumes after the brace, not after the `}`.** So one stray brace cannot
swallow the placeholder behind it: `{ {yyyy}` is `{ 2026`.

**One `keeper:` map, assembled once.** `create_note` previously pushed `("keeper", …)` for
capture. Provenance goes into the same map. Two pushes would write the key twice and
`Frontmatter`'s reader takes the first — so a captured note made from a template would have
silently lost whichever came second.

**The tag union is normalised across all three producers.** Caller's tags, the space's, and
the template's go through `tags::normalise_all` together, so a space naming `#Work`, a
caller naming `work` and a template naming `Work` do not put three spellings in the file.

**The editor never lies about what the file says.** A `<select>` whose value matches no
option renders the *first* one — here "No template" — and the next Save would make that lie
true. So the stored value gets its own option whenever it is unlisted, and only the *red
sentence* waits for keeper to actually know the template is gone. The two conditions are
separate (`templateUnlisted` vs `templateMissing`) because an empty list from a failed read
is not evidence a template is missing.

---

## Compatibility

`is:template` now means `templates::is_template(&fm) || rel.starts_with("templates/")`.

The tag is the marker (AD-82). The directory is **grandfathered, not a second rule**: a
vault seeded by an earlier build has untagged notes under `templates/` that the template
list has always shown, and silently un-templating them on upgrade would be keeper taking a
feature away from a file it did not change. The half AD-82 was actually asking for — a
tagged template *anywhere* — now works.

---

## Verification

| Gate | Result |
|---|---|
| `cargo test … -p keeper-core --lib notes::templates` | **46 passed, 0 failed** |
| `bun run vitest run space-editor.test.tsx space-list.test.tsx` | **58 passed, 0 failed** |
| `npx tsc --noEmit` | clean for every file this story touched |
| ts-rs binding export tests | 171 passed; `NoteSpaceVm.ts` / `NoteSpaceReq.ts` regenerated, never hand-written |

**The `keeper` shell crate cannot be compiled on this host** — `glib-sys` and `gobject-sys`
fail their build scripts on Linux (AD-55/AD-56). Every shell edit in this story is therefore
wiring only, with each decision it needs living in `keeper-core` where it is proved:
`is_template`, `space_default_template`, `expand`, `provenance_pairs`,
`missing_template_notice`, `seed_templates` and `report_template_seed`. **What remains
unproven here is the plumbing that calls them** — `template_source`'s three rungs,
`create_note`'s frontmatter assembly, `create_journal`, `space_def`, `notes_space_save`,
`VaultSeedFiles::write` and the two seed entry points. Those need the macOS gate.

### Mutation results

**Harness provenance, given the `/tmp/mutate*.py` clobber incident.** My harness never
existed on disk: it lived as functions in the in-process Python kernel, writing directly to
the source file with a `finally` restore. `inspect.getsource` on it raises
`OSError: could not get source code`, which is the proof — there is no backing file for
anyone to swap. `/tmp` holds `mutate-fe.py`, `mutate_fe.py` and `mutate_rust.py`; none are
mine and none were read by this run.

Adopting W3Csv's three rules:

1. **Unmutated baseline before *and* after the sweep** — before: `test result: ok, 46 passed`;
   after: `test result: ok, 46 passed`. A restore that silently did not happen is invisible
   otherwise.
2. **Anchor misses are an alarm, not a skip** — `mutate()` asserts the anchor appears
   *exactly once* and raises otherwise, so a mutation already present in the file stops the
   run instead of being measured against broken code.
3. **A cancelled run leaves the mutant applied.** One cell of mine *was* interrupted (M8, on
   the 1800 s cap). Its `finally` ran, and rather than trust that, all twelve anchors were
   re-checked against the pristine text — all present, no residue (`filter(|_| false)`,
   `let _ = body_offset;`, `{false && templateUnlisted` all absent). M8 was then re-run
   cleanly on its own.

Also corrected: my first frontend pass reported M9–M11 as **NOT CAUGHT**. That was a bug in
my *parser*, not a hole in the suite — vitest emits ANSI-wrapped `FAIL` lines under a
non-TTY and my regex expected the TTY `× name Nms` form. Re-run with the escapes stripped,
all three are caught, and the raw assertion text was read by eye for M9 before believing it.
No claim in this spec rests on the first pass.

The four load-bearing mutations were re-verified a second time under the same private
harness with the runner's own panic text captured.

| # | Mutation | Verdict | Caught by |
|---|---|---|---|
| M1 | the `template` tag is not dropped from the copy | caught, **re-verified** | `the_copy_carries_every_tag_except_the_marker`, `the_marker_is_matched_after_normalisation_not_as_typed`, `a_tag_merely_filed_under_template_is_kept`, `every_shipped_template_is_a_template_and_makes_notes_that_are_not`, `a_template_hands_its_own_properties_to_the_copy_but_never_its_identity` |
| M2 | `expand` copies the whole file, frontmatter included (**the shipped defect**) | caught, **re-verified** | `the_templates_own_frontmatter_never_reaches_the_body` (*"inbox smuggles a frontmatter fence into its body"*), `a_template_without_placeholders_copies_byte_identically`, `every_shipped_template_is_a_template_and_makes_notes_that_are_not` |
| M3 | the single-brace `{yyyy}` vocabulary is not resolved | caught | `the_recording_path_vocabulary_resolves_in_a_body`, `the_two_vocabularies_disagree_about_mm_and_both_are_honoured`, `a_cursor_offset_survives_a_single_brace_expansion_before_it`, `an_unclosed_single_brace_is_literal_and_a_later_token_still_resolves`, `a_template_with_no_frontmatter_at_all_is_still_a_body` |
| M4 | provenance records only the path, never the id | caught | `expansion_reports_the_templates_own_id_for_provenance`, `provenance_round_trips_through_a_written_note` |
| M5 | `tags` left in the copied properties (marker sneaks back) | caught, **re-verified** | `a_template_hands_its_own_properties_to_the_copy_but_never_its_identity` (*"source order, and only the author's own keys"*) |
| M6 | an absent ledger reports as unreadable (silent no-op on a fresh vault) | caught, **re-verified** | `a_first_run_on_a_fresh_vault_writes_all_three_and_records_them`, `restore_ignores_the_ledger_and_replaces_only_what_is_missing`, `a_template_already_there_under_the_same_name_is_never_doubled`, `a_write_that_fails_partway_records_what_landed` (*"expected Stopped, got AlreadySatisfied"*) |
| M7 | a shipped template tags its notes `inbox` (files them out of the Inbox) | caught | `every_shipped_template_is_a_template_and_makes_notes_that_are_not` (*"inbox hands its copy [\"template\"]…"*) |
| M8 | the journal table puts a placeholder back in a cell | caught | `a_shipped_templates_tables_are_aligned_after_expansion` |
| M9 | the space's template is not sent on save | caught | `saves the chosen template as its vault-relative path`, `clears the setting to null rather than to an empty path`, `keeps showing a template the vault no longer has, and says so` |
| M10 | an unlisted template gets no option, so the select silently reads "No template" | caught | `keeps showing a template the vault no longer has, and says so`, `does not call a template missing when the list simply failed to load` |
| M11 | a failed list read is treated as proof the template is gone | caught | `does not call a template missing when the list simply failed to load` |

**No survivors.** Two of these — **M5 and M6** — were not synthetic: both were live bugs the
tests found during implementation, and both have this epic's signature shape. M5 would have
made every templated note a template of itself; M6 would have made the seeder green
everywhere and silent on a fresh vault.

---

## Deliberately NOT done

- **Retroactive application of an edited template.** UX-DR59 and Story 44.8. This story
  writes the provenance 44.8 reads and stops there.
- **No version or content hash in the provenance.** Reasoned above.
- **`template/daily` is not stripped from a copy.** Only the exact `template` tag leaves. A
  nested tag is somebody's own filing under a word keeper happens to reserve at the root,
  `is_template` does not read it as a marker either, and removing it would be deleting a tag
  that was never doing anything.
- **No `{slug}`, `{seq}` or single-brace `{title}` in bodies.** `{{title}}` already exists;
  a second spelling of one value in one document is the divergence this module exists to
  prevent. `{seq}` disambiguates a colliding *folder* and has no meaning in a paragraph.
- **The `field:` query predicate still cannot address `keeper.from_template`.** The index
  flattens a one-level map through `FieldValue::index_string`, so it lands under the bare
  key `keeper` as `"from_template: …\nfrom_template_id: …"`. Nobody can therefore write a
  space for "notes from this template". Out of scope here: fixing it means changing how the
  index flattens reserved maps, which touches every `keeper.*` key and every space that
  reads one. 44.8 inverts the flattening on its own side to run its finder over the
  in-memory snapshot with no file reads.
- **No callouts, and no attempt to teach the renderer them.** Adding callout support to
  `live-preview.ts` is a renderer story, not a template story.
- **No "apply a template to an existing note" command.** Not asked for, and it is the
  destructive reading of 44.8's problem.
- **No template *picker* at create time.** 44.6 owns the creation surface; a space names its
  template and the palette creates from the rail. A per-create chooser is a dialog, and
  UX-DR35 says the create path has none.
- **The vault-level `default_template` was left exactly as it was.** It already worked, it is
  now the third rung, and re-homing it into the space vocabulary would delete a setting
  people may be using.
- **`default_spaces.rs` was not edited**, as instructed — its port and outcome types are
  imported, not modified.

---

## Deferred work filed

Nothing new. DW-163 (`keeper.limit`) and DW-164 (`recorded`) are W3SpaceSort's and belong to
44.11. The `field:`-cannot-address-`keeper.*` limitation above is recorded here rather than
as a DW entry because 44.8 works around it in-tree and no surface currently promises
otherwise.
