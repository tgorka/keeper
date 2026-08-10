# Story 45.16 — Quick Capture Knows Its Template and Its Tag

status: implemented
epic: 45 — Open it, change it, put it back
binds: FR-193
depends on: 44.3 (the four default spaces), 44.6 (the create path and `seed::verdict`), 44.7 (templates and `expand`), 45.14 (quick capture mounts the note editor)
feeds: nothing yet — a Captures space is a `tag:` space anyone can write

---

## What was already there, and what was not

The epic family's recurring lesson again, and this time the answer is "most of the
selection half, none of the configuration half".

| Already existed | Where | State |
|---|---|---|
| `keeper.capture: true` on every captured note | `notes_ipc::create_note` | Working since 36.4 |
| `is:capture` in the closed `is:` set | `notes::query::IS_FLAGS` | Working |
| `Seed.capture`, and `is:capture` seeding it | `notes::seed` | Working (44.6) |
| `templates::expand`, the marker strip, provenance | `notes::templates` | Working (44.7) |
| A capture applying the **vault's** `default_template` | `template_source`'s third rung | Working — and it is not capture-specific |
| `seed::verdict` — the space's real query over the real bytes | `notes::seed` | Working (44.6) |
| `NotesConfig.default_template` on the wire | `NoteVaultSettingsReq` | **Settable only over IPC — no surface in the app edits it** |

So captures were **already** selectable by something that is not a path. What did not
exist was a **tag** — `keeper.capture` is a reserved frontmatter key the user cannot see,
type into `tag:`, or find in the tag tree — and there was no capture-specific template.
And the vault-level template setting had no UI at all, which is why Settings gained a
section rather than a field.

Nothing else asked for by this story was already present. That is the honest answer to
"check whether the thing already exists": one half of it was, and it is reused rather
than replaced — `is:capture` still works, with or without a tag, and there is a test
that says so.

---

## Positions taken

### The capture tag ships OFF, and that default is the story's main decision

44.7 wrote down why its three shipped templates add no tags of their own:

| Space | Selects by |
|---|---|
| Inbox | `is:untagged` |
| Journal | the `journal/` path |
| Recordings | the `session:` frontmatter key |

An Inbox template that tagged its notes `inbox` would file every one of them **straight
out of the Inbox**. A capture tag is the identical hazard with a wider blast radius: not
one template's copies but *every thought the user captures*, leaving the one space 44.3
seeds to receive them.

So `NotesConfig.capture_tag` defaults to `None`, and the reason is written at the field.
The alternative — shipping `capture` — would change what an existing vault does on
upgrade, in a vault keeper did not otherwise touch, with nobody having asked. That is the
same refusal 44.7 made and the same one `templates/` grandfathering makes.

**And the cost is computed rather than asserted.** `seed::capture_tag_cost` runs each
space's own stored query, through `query::eval`, over the note a capture would write, and
returns one finished sentence per space that lists captures today and would stop. The
Settings surface renders that list. A hardcoded line about Inbox in the webview would be
wrong for a vault whose Inbox has been edited and silent for the space the user wrote
themselves (AD-55, AD-58).

### What a capture tag does to the five spaces 44.3 seeds, measured

`what_a_capture_tag_does_to_every_space_a_fresh_vault_is_seeded_with` asserts this table
by running each default's real query. It is a table, not a paragraph, and the test fails
loudly if a sixth default space is added without an answer.

| Space | Query | Lists an untagged capture | Lists a capture tagged `capture` |
|---|---|---|---|
| Inbox | `is:untagged` | **yes** | **no** |
| Journal | `is:journal` | no | no |
| Pinned | `is:pinned` | no | no |
| Recordings | `is:recording` | no | no |
| Templates (45.20) | `is:template` | no | no |

One row moves. That row is the whole cost, and the sentence the user is shown names
`is:untagged` and the space by name because `verdict` composed it.

### `template` is refused as a capture tag

A capture tagged `template` would make **every captured thought a scaffold** — AD-82's
marker arriving through the front door, the mirror of the defect 44.7 fixed by stripping
it on copy. `seed::capture_tag` refuses it, in the one place both the settings save and
the seed read, so the form and the note cannot disagree. A nested `template/inbox` is
somebody's own filing under a word keeper reserves at the root and is left alone — 44.7's
ruling for the copy path, spelled the same way here.

### Four rungs, in `keeper-core`, because the shell trimmed one and not the next

`template_source`'s `or_else` chain filtered `req.template` for blankness and not
`space_template`. The ordering and the blank rule now live in `templates::rung` over a
named-field `TemplateRungs` struct — named fields rather than four positional
`Option<&str>`, because four same-typed parameters is a call site where two can be
swapped silently and the failure is a note templated from the wrong rung.

`named` → `space` → **`capture`** → `vault_default`. The capture rung sits between the
space and the vault because it is chosen for a *surface*: narrower than the vault, wider
than one note. A capture has no space, so in practice it is capture-template-then-vault-
default — and with nothing configured a capture gets exactly what it got before this
story, which is what makes the change safe for existing vaults.

The shell derives the capture rung from `seed.capture` rather than taking a parameter, so
a future caller cannot acquire a capture's scaffold by forgetting to say it is not one.

### One producer for what a capture carries

`seed::capture(tag)` returns the whole `Seed`: the reserved mark **and** the configured
tag. They were about to live in two places — the mark in the shell's commit path, the tag
beside it — and two producers of "what a capture carries" drift the moment one gains a
rule. The symptom would be a note that is a capture to one surface and not to another.

### The test double became the production function

44.6's `seed.rs` carried `as_created`, a `#[cfg(test)]` mirror of the shell's parser whose
doc said *"Production never calls it."* This story needs the same projection in production
— to answer "would a space list a capture" before the capture exists — and writing a
second one would have been two models of one thing inside the module whose entire purpose
is refusing that.

So `as_created` is **gone** and `seed::projected(seed, title, body, stamp, now_ms)` is
production. All twenty-two of 44.6's existing tests go through it unchanged, which is the
regression net that says the promotion changed no behaviour. It also now uses
`naming::note_filename` and the stamp's own date instead of a hardcoded filename, so the
path a `path:` space is asked about is the path a create would actually pick.

**This is a model of a parser this crate cannot compile, and that is a real risk named at
the function.** What bounds it: the rules mirrored are each one line in
`notes_vault::parse_note` and each already lives in this crate —
`templates::is_template`, `JOURNAL_DIR`, the `spaces/` and `templates/` prefixes — so the
model can only drift by somebody adding a rule to the parser, which is the moment to add
it here.

### No new default space, and no new module

A "Captures" default space would be seeded into fresh vaults and **not** into existing
ones — `plan`'s ledger says "already offered" — so the same build would ship two different
rails. 45.17 owns default-space deletion and 45.20 owns adding one; a `tag:<capture tag>`
space is three clicks in the space editor and `seed::inherit` already seeds it, which
`creating_by_hand_into_a_capture_space_seeds_the_same_tag_capture_writes` asserts.

No `capture.rs` either. Every decision had a home: the tag rule and the seed in `seed.rs`
beside `verdict`, the rung ordering in `templates.rs` beside the other rungs. 44.7's
"a `template.rs` beside `templates.rs` is a rename waiting to half-land" generalises.

---

## Contract

```rust
// keeper_core::notes::seed
pub fn capture_tag(configured: &str) -> Option<String>;   // canonical, or None; refuses `template`
pub fn capture(tag: Option<&str>) -> Seed;                // the mark AND the tag, one producer
pub fn projected(seed: &Seed, title: &str, body: &str, stamp: &str, now_ms: i64) -> IndexEntry;
pub fn capture_verdict(space_name: &str, query: &str, tag: Option<&str>, stamp: &str, now_ms: i64)
    -> Option<String>;
pub fn capture_tag_cost(space_name: &str, query: &str, tag: Option<&str>, stamp: &str, now_ms: i64)
    -> Option<String>;                                    // only what the tag COSTS

// keeper_core::notes::templates
pub struct TemplateRungs<'a> { named, space, capture, vault_default }
pub fn rung<'a>(rungs: TemplateRungs<'a>) -> Option<&'a str>;

// keeper_sync::profile::NotesConfig
pub capture_template: Option<String>;
pub capture_tag: Option<String>;                          // canonical form, None by default
```

Wire: `NoteVaultSettingsReq.{captureTemplate, captureTag}` (absent = unexpressed, `""` =
clear), `NoteVaultVm.{captureTemplate, captureTag}` (what is in force, AD-34-8), and
`notes_capture_impact(vaultId, tag) -> string[]`.

Frontend: `src/components/notes/capture-settings.tsx` (Settings → Quick capture) and
`src/components/notes/template-select.tsx`, the shared chooser 44.7's rule now lives in
once.

---

## I/O matrix

### `capture_tag(configured)`

| Input | Result | Why |
|---|---|---|
| `capture` | `Some("capture")` | |
| `#Quick Capture` | `Some("quick-capture")` | the one tag rule, applied on the way in |
| `Template/Inbox` | `Some("template/inbox")` | filed under a reserved word, not the marker |
| `""`, `"   "`, `"#"`, `"###"`, `"---"`, `"/"`, `"//"` | `None` | cleared and unusable are one state |
| `template`, `#Template`, `"  TEMPLATE  "` | `None` | AD-82: every capture would be a scaffold |

### `capture(tag)`

| `tag` | `Seed.capture` | `Seed.tags` | `Seed.dest` |
|---|---|---|---|
| `Some("#Quick Capture")` | `true` | `["quick-capture"]` | `None` |
| `None` | `true` | `[]` | `None` |
| `Some("---")` | `true` | `[]` | `None` |
| `Some("template")` | `true` | `[]` | `None` |

Never a folder: a capture is filed by tag, which is the story's sentence.

### `rung(TemplateRungs)`

| named | space | capture | vault_default | Result |
|---|---|---|---|---|
| `n.md` | `s.md` | `c.md` | `v.md` | `n.md` |
| — | `s.md` | `c.md` | `v.md` | `s.md` |
| — | — | `c.md` | `v.md` | `c.md` |
| — | — | — | `v.md` | `v.md` |
| — | — | — | — | `None` |
| `"   "` | `s.md` | — | — | `s.md` — a blank rung never shadows the one beneath |
| — | `""` | `"\t\n"` | `v.md` | `v.md` |
| `""` | `"  "` | `"\n"` | `" "` | `None` |
| — | — | `" templates/capture.md\n"` | — | `templates/capture.md` (trimmed) |

### `capture_verdict(space, query, tag, …)` — `None` means "the space lists it"

| Query | tag `None` | tag `Some("capture")` |
|---|---|---|
| `is:untagged` (Inbox) | `None` | sentence naming `is:untagged` and the space |
| `tag:capture` | sentence | `None` |
| `tag:inbox` with tag `inbox/capture` | — | `None` (a subtree term is satisfied by the tag under it) |
| `is:capture` | `None` | `None` |
| `text:agenda` | sentence | sentence — a capture's body is not knowable in advance |

### `capture_tag_cost` — the same question, filtered to what the tag COSTS

| Space lists an untagged capture | …and a tagged one | Result |
|---|---|---|
| yes | no | `Some(sentence)` — the only case worth showing |
| yes | yes | `None` |
| no | no | `None` — a space that never listed captures is not a cost of turning the tag on |
| no | yes | `None` — a gain, and the surface does not claim gains it cannot promise |

### The Settings surface

| State | Rendered |
|---|---|
| no vault flagged | **nothing** — never a section of disabled controls |
| several vaults, none active | nothing — no honest answer to "which vault's captures" |
| one vault, none active | that vault (flagging a folder and going straight to Settings is ordinary) |
| several vaults, one active | the active one |
| stored template in the list | selected |
| stored template **not** in a list that loaded | its own option, `<path> — not in this vault`, plus the red sentence |
| stored template, list **failed to load** | its own option, bare path, **no** red sentence |
| tag typed | consequence recomputed for the typed tag, before Save |
| tag saved | field shows the **folded** spelling Rust stored |
| tag `template` saved | field shows empty — the refusal is visible rather than silent |
| save rejected | the typed value is kept and one sentence says nothing changed |

---

## Edge cases and the reasoning behind each

**A capture with no template configured still creates.** With `capture_template` unset the
rung falls to `vault_default`, and with that unset too `template_source` returns
`TemplateChoice::None` and the note is written plain. This is the path every existing
vault is on, and it is byte-for-byte what it was before this story.

**A capture whose configured template is gone still creates**, with 44.7's
`missing_template_notice` — and unlike the pre-45.14 buffer path, the capture window now
has a `notices` channel to show it on, because `resolve_capture_draft` passes a real
`&mut notices` rather than `&mut Vec::new()`.

**The tag is folded on the way IN, not on the way out.** The stored value is what the note
will carry. Folding at use time would leave the form saying `#Quick Capture` while every
note said `quick-capture` — two spellings of one tag in front of the person who chose it
(AD-34-8).

**A configured value that is not a tag clears the field, visibly.** There is no notice
channel on the settings save, so the refusal is shown by the field coming back empty after
the round trip. The standing explanation above the field states the folding rule and the
leave-it-empty escape hatch, so an empty field after typing `---` reads as "that was not a
tag" rather than as a lost save.

**The impact preview asks about the tag the form is HOLDING**, not the one on disk, so the
consequence arrives before Save rather than after it. `null` is asked for an empty field,
which is always an empty answer and is what makes the control's off state honest.

**A `text:` space always reports that captures will not appear.** Two facts about a capture
are unknowable in advance — its title (the first line of text not yet typed) and its body
— and both are passed as empty rather than invented. That is the honest answer to *will
captures appear here*, and not a claim about any particular capture. Written at the
function, because an invented body would make a `text:` space claim a capture will land in
it.

**`projected` mirrors the two folder-prefix rules as well as the tag rules.** `is:space`
declines to seed a destination, but `path:spaces/**` does not — so a seed *can* produce
`dest = "spaces"`, and `parse_note` would flag that note `space` while the projection did
not. Same for `path:templates/**` and 44.7's grandfathered `templates/` prefix. Found by
auditing the function's own doc comment against the parser it claims to mirror (A5 below).

**A stamp too short to carry a date leaves the name undated rather than panicking.**
`stamp.get(..10)` returns `None` on a short or non-boundary slice; `note_filename` already
treats an empty date as "no date prefix".

---

## Verification

### The acceptance command, run by name

`cargo test --manifest-path src-tauri/Cargo.toml -p keeper-core --lib notes::`

**EXIT=0 — 516 passed, 0 failed**, on the run taken after every sibling's sweep window
shut. It was **RED for most of this story's life** —
`notes::vm::tests::only_a_seeded_space_promises_to_stay_deleted`, 45.17's, since fixed by
its owner — and that is written down rather than smoothed over, because the honest reading
of a red acceptance command is "somebody's, and here is whose", never a narrower green
substituted for it. It also drove a real harness decision: the sweep does **not** gate on
`notes::`, because a gate that is already red scores every mutation as CAUGHT for the
wrong reason.

**Everything else, run by name:**

| Gate | Result |
|---|---|
| `cargo test … -p keeper-core --lib notes::` | **EXIT=0, 516 passed** |
| `cargo test … -p keeper-core --lib notes::seed` | **EXIT=0, 42 passed** |
| `cargo test … -p keeper-core --lib notes::templates` | **EXIT=0, 59 passed** |
| `cargo test … -p keeper-sync --lib profile::` | **EXIT=0, 25 passed** |
| `bun run vitest run capture-settings.test.tsx` | **EXIT=0, 21 passed** |
| capture-settings + settings-dialog + notes-pane | **EXIT=0, 90 passed, three consecutive repeats, zero unhandled, zero `export is defined`** |
| `bunx tsc --noEmit` | zero errors in any file this story touches |
| ts-rs binding export | `NoteVaultVm.ts` / `NoteVaultSettingsReq.ts` regenerated, never hand-written |

### What I could not verify here, and why

**The `keeper` shell crate does not build on Linux** (AD-55/AD-56, `glib-sys`). Every
decision this story makes lives in `keeper-core` or `keeper-sync`, where it is proved.
What is **wiring, and has never been compiled or executed**:

- `apply_settings`'s two new arms — including the `seed::capture_tag` call that is the
  only thing making the stored tag canonical. The rule is proved; **the call is not**.
- `template_source`'s new `capture` rung and the `seed.capture` derivation that gates it.
  The ordering is proved; the wiring that supplies it is not.
- `resolve_capture_draft` passing `seed::capture(vault.config.capture_tag.as_deref())`.
- `notes_vault::vault_vm`'s two new fields.
- The whole of `notes_capture_impact`, including its registration in `lib.rs`.

**No note has ever been captured through this code.** No capture has been written to a
real vault, no template has been expanded through the capture path, and no tag has reached
a real frontmatter block. The projection this story reasons with is a model of
`parse_note`; the real parser has never seen a capture with a tag on it.

**First checks on the macOS gate**, in order:
1. `cargo check -p keeper` — the five wiring points above are unproven syntax.
2. Settings → Quick capture: set a tag. Confirm the field comes back **folded**, and that
   the consequence list names your Inbox.
3. Summon quick capture, type a word, dismiss. Open the note: it must carry
   `tags: [<your tag>]` **and** `keeper.capture: true`, and it must NOT carry the
   `template` tag if you chose a template.
4. Confirm the note has left the Inbox and appears in a `tag:<your tag>` space. That is
   the trade, and step 2 is supposed to have warned you about it before step 3.
5. Set the capture tag to `template`. It must clear.

---

## Mutation results

Harness: `~/.W3CaptureTag/sweep.py`, never `/tmp`. Sentinel `MUTCT<NN>`, unique in both
directions and present only in the mutant. Every mutation asserts its anchor appears
**exactly once** before the edit and exactly once after the restore, with the mutant at
zero — an anchor miss stops the run rather than being skipped. Restores are targeted
single-occurrence replacements, never a whole-file write, because `templates.rs` and
`settings-dialog.tsx` were being edited by siblings throughout.

Gates were asserted green **before and after** the sweep. The Rust gates are
`notes::seed`, `notes::templates` and `profile::` rather than `notes::` — deliberately,
because `notes::` was already red from a sibling's in-flight work and **a gate that is
already red scores every mutation as CAUGHT for the wrong reason**. The harness aborts
rather than sweeping over a red gate.

**30 sweep mutations, 30 caught, 0 survivors.** Baselines: before — `notes::seed` 39/39,
`notes::templates` 59/59, `profile::` 25/25, TS 62/62, all EXIT=0; after — identical.
Sentinel residue after the run: **0** in every file.

| # | Mutation | File | Verdict |
|---|---|---|---|
| M01 | `capture_tag` accepts the `template` marker | seed.rs | caught |
| M02 | `capture_tag` stores what was typed instead of the canonical tag | seed.rs | caught |
| M03 | a capture carries no tag however the setting is configured | seed.rs | caught |
| M04 | a capture is not marked as one | seed.rs | caught |
| M05 | the verdict answers about an untagged capture whatever the tag is | seed.rs | caught |
| M06 | the verdict invents a body a capture has not been given | seed.rs | caught |
| M07 | a projected capture carries no capture flag | seed.rs | caught |
| M08 | a projected note tagged `template` is not flagged as one | seed.rs | caught |
| M09 | a projected note carries none of the seed's tags | seed.rs | caught |
| M10 | the projected filename loses the stamp's date | seed.rs | caught |
| M11 | a projected note ignores the seed's destination folder | seed.rs | caught |
| M12 | the capture rung outranks the space's template | templates.rs | caught |
| M13 | the vault default outranks the capture template | templates.rs | caught |
| M14 | a rung is not trimmed before it is handed on | templates.rs | caught |
| M15 | a blank rung shadows the rung beneath it | templates.rs | caught |
| M16 | the caller's own template is ignored | templates.rs | caught |
| M17 | the shipped default tags every capture | profile.rs | caught |
| M18 | the shipped default gives every capture a template | profile.rs | caught |
| M19 | the save sends the whole form instead of one knob | capture-settings.tsx | caught |
| M20 | the impact is asked about the SAVED tag, not the one being typed | capture-settings.tsx | caught |
| M21 | an emptied field asks about the empty string rather than no tag | capture-settings.tsx | caught |
| M22 | the saved tag is not mirrored back into the field | capture-settings.tsx | caught |
| M23 | with several vaults and none active it configures the first | capture-settings.tsx | caught |
| M24 | choosing a template changes the control and saves nothing | capture-settings.tsx | caught |
| M25 | the tag save sends an empty tag whatever was typed | capture-settings.tsx | caught |
| M26 | a stored template is never treated as unlisted | template-select.tsx | caught |
| M27 | a list that has not loaded is treated as proof the template is gone | template-select.tsx | caught |
| M28 | only the first template is offered | template-select.tsx | caught |
| M29 | the stored value only gets its own option once keeper knows it is gone | template-select.tsx | caught |
| M30 | the section is never mounted in Settings | settings-dialog.tsx | caught |

**No survivors, and that is the point of the section below rather than a result.** Every
one of these is a line I had already decided was load-bearing.

**Two harness notes.** The sweep was interrupted once by an out-of-process kernel reset
between mutations; the tree was re-checked by literal sentinel count over every file
before resuming, and was clean — an interrupted sweep is a crashed sweep and a `finally`
that did not run leaves a live mutant. And the residue check is a **literal substring
count over a file list**, never `git diff`: three of this story's files are new and
therefore untracked, and a diff is blind to them.

---

## Shape audit

The sweep is a list of lines I had already decided were load-bearing. These are the probes
that came from shapes peers were bitten by, run after it was green.

**11 probes after the sweep was green. 10 caught on the first run, 1 survived and was
closed. Two real defects, one of them pre-existing and promoted into production by this
story. Every probe came from a shape somebody else was bitten by first.**

| # | Shape, and whose | What I asked | Verdict |
|---|---|---|---|
| A1 | *W3Chrome — a doc comment naming another module's behaviour is an assertion nobody runs* | Does `projected` really mirror `parse_note` "for exactly the facts a Seed can set"? | **DEFECT.** It mirrored neither folder-prefix rule. `parse_note` flags `space` from `spaces/` and `template` from the grandfathered `templates/`; a seed reaches both through `path:` (`is:space` declines to seed a destination, `path:spaces/**` does not). So 44.6's `verdict` told a `path:templates/**` space its new note would not appear when it would. Closed; probe caught. |
| A2 | same | Is the 44.7 grandfathering half of that rule pinned separately? | caught |
| A3 | *shape 8 — count your doors, and shape 3 — a decision stated where nothing can test it* | Which spaces the impact preview names was a filter living in an **uncompilable shell command**. Is that a decision? | **Yes.** Moved to `seed::capture_tag_cost` with all four quadrants tested. Probe caught. |
| A4 | same | …and does it ever say anything at all? | caught |
| A5 | *W2Media — assert what you handed on, not only what came back; a prop is a boundary* | `TemplateSelect` is handed `note=`. Does anything assert it arrives? | **No.** Nothing asserted either explanation rendered — the tag one is the only place the folding rule and the leave-it-empty escape hatch are stated at all. Closed; probe caught. |
| A6 | same | The tag field's own explanation | caught |
| A7 | *shape 1 — what composes the input?* | The save's answer goes into the shared vault mirror and **nothing renders it**, so no screen assertion can see it. Is it tested? | **No.** Closed with a two-vault test; probe caught. |
| A8 | *W2Attach — put at least two items in any collection fixture* | Would a mirror that replaced the list with only the saved vault be noticed? | caught, by the second vault's setting surviving |
| A9 | *W3Capture — two producers that run one after the other cannot share one state slot* | The tag field has two: the user's typing and the save's response. | **DEFECT.** Blur, keep typing while the write is in flight, and the response lands on the keystrokes made since — silently, with a value that looks authoritative because it came from Rust. Guarded with a functional update; probe caught. |
| A10 | same, on the chooser | The identical guard on the template select | **SURVIVED.** Correct code, unpinned invariant (W2Media's third category). Reachable with two choices while the first write is in flight. Pinned with a two-writes-in-flight test; **re-probed, caught.** |
| A11 | *W3Recording / W3Chrome — an absence with no witness in the same representation is one assertion* | Three tests assert `!has_flag(TEMPLATE_TAG)`. Does anything assert it is ever set? | **No.** Added the positive witness; without it a broken `has_flag` makes all three pass for the wrong reason. |

Checks that came back **clean**, recorded as *checked* rather than assumed, because
afterwards the two look identical:

- *W3NoteFile — `await` is not a success check when the callee catches its own failure.*
  Every `await` in `capture-settings.tsx` is on a bare IPC wrapper that rejects; none
  swallows into a store.
- *W2Attach — a mock factory's completeness is a function of how long your tests run.*
  This surface schedules no timer, debounce or interval, and `export is defined` and
  `Unhandled` both count **zero** across three repeats.
- *W3NoteFile — `mockRejectedValue` builds its rejected promise when CONFIGURED.* Three of
  mine did; all three switched to `mockImplementation(async () => { throw … })`.
- *W3NoteFile — `as T` on a fixture literal asserts rather than checks it.* No cast in this
  story's fixtures; the one place TypeScript pushed me toward one (a `let` assigned only
  inside a callback, narrowed to `never`) was rewritten as an array instead.
- *W3Chrome — a set is a global fact and your search is local.* `NoteVaultVm` gained two
  **required** fields, so every fixture in the tree had to grow them. Found and fixed
  `notes-pane.test.tsx`'s (reported to me by W3Export out of their own `tsc` run, which is
  exactly the mechanism working); `bunx tsc --noEmit` is now clean over every file this
  story touches.
- *W3Capture — the contracts a replaced mechanism kept are not in the diff.* This story
  replaces nothing. It **widens** `template_source` from three rungs to four and promotes
  a test double to production; both old contracts are carried by construction — with no
  capture template configured the rung result is byte-identical to the third rung, and all
  22 of 44.6's existing tests run unchanged through `projected`.
- *W3TagsDelete — a new feature can break an old contract without touching the line that
  states it.* The contract at risk here is 44.3's: **Inbox shows unfiled thoughts.** A
  capture tag breaks it, the story knows it, the default is off because of it, and the
  surface computes and shows the breakage before you choose it. That is the one thing this
  story is about rather than something it nearly did by accident.

**Owed, not done**, and on the right side of W2Media's line (production API, not a test
helper): `projected` is a hand-written model of `notes_vault::parse_note`, and the durable
repair is for the parser's flag rules to be a `keeper-core` function the shell *calls*
rather than a list this crate mirrors. That is `notes_vault`'s change, not this story's,
and A1 is what a mirrored rule costs when it drifts. It is the sixth "owed" against the
same category this wave.

---

## Deliberately NOT done

- **No "Captures" default space.** 44.3's ledger means a fifth default reaches fresh
  vaults and not existing ones, so the same build would ship two different rails. A
  `tag:<capture tag>` space is three clicks and `seed::inherit` already seeds it.
- **The capture tag is not applied retroactively.** Turning it on tags the captures you
  make next. Rewriting frontmatter keeper did not author is FR-121's refusal, and a
  setting that edits a thousand notes on save is not a setting.
- **keeper does not offer to fix your Inbox.** The consequence list says which spaces stop
  listing captures; editing somebody's saved query on their behalf is 45.17's surface and
  a much larger decision than a settings toggle.
- **No `is:` predicate for the capture tag.** It is an ordinary tag that `tag:` reads. A
  second name for one thing is the one thing epic 44 says it adds none of, and
  `is:capture` already exists for the reserved mark.
- **`space-editor.tsx` still has its own template `<select>` markup.** The shared
  `<TemplateSelect>` exists and is used here; W3Chrome owns that file this wave and asked
  to adopt it in their own pass rather than have it rewritten under them. The shared
  component preserves both of their load-bearing rules — `unlisted` and `missing` stay
  separate states, and the stored value is never written back — and keeps the
  `template-missing` `data-slot` so their suite is unaffected.
- **No per-capture verdict at commit time.** `create_for_space` runs `verdict` after a
  write because the user named a space; a capture names none, and a sentence per capture
  is noise on the surface whose whole promise is no questions (UX-DR35). The question is
  answered once, at the moment the setting is chosen.
- **The vault-level `default_template` is untouched.** It already worked, it is now the
  fourth rung, and re-homing it would delete a setting people may be using. It also, for
  the first time, has a surface that can read it — the capture chooser's explanation names
  it as the fall-through.

---

## Deferred work filed

Nothing new. The `field:`-cannot-address-`keeper.*` limitation 44.7 recorded still holds
and still does not bite here: the capture tag is an ordinary tag, so `tag:` addresses it.
