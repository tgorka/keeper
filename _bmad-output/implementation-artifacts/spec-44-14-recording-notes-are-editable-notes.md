---
title: 'Story 44.14: Recording Notes Are Editable Notes'
type: 'feature'
created: '2026-08-09'
status: 'review'
blocking_condition: ''
baseline_revision: '15a3b41'
final_revision: ''
review_loop_iteration: 0
followup_review_recommended: false
context:
  - '{project-root}/docs/project-context.md'
  - '{project-root}/_bmad-output/planning-artifacts/epic-44-the-vocabulary-is-the-space.md'
  - '{project-root}/_bmad-output/implementation-artifacts/spec-42-4-the-note-stub.md'
  - '{project-root}/_bmad-output/implementation-artifacts/spec-42-5-one-tag-vocabulary.md'
  - '{project-root}/_bmad-output/implementation-artifacts/spec-43-2-a-recording-note-says-it-is-one.md'
  - '{project-root}/_bmad-output/implementation-artifacts/spec-44-13-tag-entry-that-completes.md'
---

<intent-contract>

## Intent

**Problem, as measured rather than as reported.** The epic says the recording note's tags "render
inert". They did not: `tags:` is a block list and the properties panel's generic list control gave
it removable chips and a text box, exactly as it does for any other list key. What was actually
wrong is three things the word "inert" covers and misnames, and two of them are worse than
inertness:

1. **The tag row had no vocabulary.** The add affordance was a bare `<Input>` that wrote whatever
   was typed. Story 42.5 exists because `Client/Acme` and `client/acme` are the same tag written
   twice and a person spends an afternoon reconciling them; this was the one tag-writing surface in
   the notes pane still creating that problem, and 44.13 had already built the control that ends it.
2. **`session:` was fully editable.** It rendered as an ordinary text `<Input>` labelled `session`.
   Typing in it rewrites the identity that 42.6's `recording_note_targets`, 43.2's `is:recording`
   predicate and the Recordings default space all resolve through — from a panel that shows none of
   those consequences. `id:` was already read-only for precisely this reason (FR-97) and `session:`
   is the same kind of fact; it had simply never been added to the rule.
3. **`recordings` came off silently.** One press removed keeper's classification tag, the note
   dropped out of the tag tree's `recordings` node and out of the Recordings space, and nothing on
   disk was deleted — so the note looks lost, with no message anywhere saying what happened or how
   to undo it.

**Approach.** `tags:` stops using the generic list control and gets `TagsProperty`: the same chips,
plus 44.13's `TagCombobox` behind an "Add a tag" toggle, reading 42.5's `tags_vocabulary`. `session:`
joins `id:` in the panel's read-only identity rule. Removing `recordings` from a recording note is
**refused with a sentence rendered in the panel**, not disabled and not ignored. The write still
goes through the panel's one existing path — `spliceProperty` into `notesSave` — so `Frontmatter`'s
byte-preservation promise is unchanged and is asserted as byte identity rather than as `toContain`.

### What already existed, and what was reused rather than restated

| Existing behaviour | Where | Reused as |
|---|---|---|
| Chips + remove, and the `Remove <tag> from <key>` name | `properties-panel.tsx::PropertyControl`, list branch | copied into `TagsProperty` unchanged, so no muscle memory and no test name moved |
| The one write path: splice one value span, send block + body at `baseRev` | `properties-panel.tsx::spliceProperty` / `write` | untouched; `TagsProperty` emits a serialised list and nothing else |
| List style preservation (block stays block, flow stays flow) | `properties-panel.tsx::serialiseList` | untouched; it is what makes the byte-identity assertions hold |
| Field + permanent list, `allowCreate`, keyboarding, refusal sentences | `notes/tag-combobox.tsx` (44.13) | mounted as-is; no props added, no second chooser |
| "Which tags match / does this text already name a tag" | `tags/tag-match.ts::namesTag` (44.13) | the recordings-tag comparison, so every casing Rust folds is protected |
| The toggle-then-chooser shape, and reading the vocabulary on open | `note-filter-bar.tsx` (44.13) | the same shape in the panel, same focus return on dismiss |
| `session:` as the recording-note predicate | `properties-panel.tsx::recordingSessionId` | the only reading; `TagsProperty` takes the resolved id as a prop |
| keeper's kind tag, and that it is compared through `normalise` | `keeper-core/notes/recording_note.rs::RECORDINGS_TAG` | still the authority; the panel mirrors the spelling and compares case-folded |

## Boundaries & Constraints

**Always:**
- One write path. `TagsProperty` calls the panel's existing `onChange` with a serialised list; the
  panel splices it. Every byte outside `[valueFrom, valueTo)` is the caller's original.
- One predicate for "is this a recording note": the `sessionId` the panel already resolved.
- A refusal is a sentence on screen. Never a disabled control, never a no-op.
- The vocabulary is read when the chooser opens, never on mount.
- Text goes to Rust exactly as the vocabulary spells it, or exactly as the user typed it.

**Block If:**
- Nothing. Frontend-only, additive to one surface, no wire change and no binding change.

**Never:**
- No normalisation in TypeScript. Case folding here decides what to OFFER and what to PROTECT, never
  what a tag IS (AD-20, 42.5).
- No second tag chooser, no second matcher, no second frontmatter writer.
- No new dependency.
- No absolute path and no root-plus-subpath join enters the panel (AD-65, FR-145) — untouched by
  this story and still asserted by the suite it inherited.

## I/O & Edge-Case Matrix

| Scenario | Input / State | Expected | Error |
|---|---|---|---|
| Add from the vocabulary | recording note, `client/an` typed, Enter | `  - client/anvil` appended last; every other byte identical | none |
| Add, style preserved | note's `tags:` is a block list | new item is a block item; a flow list stays flow | none |
| Add, write shape | any add | one `notesSave(sub, body, baseRev, block)`; body untouched | write failure → the panel's existing alert |
| Remove a user tag | `work` chipped off | block is the original minus that one line | as above |
| Remove the last tag | one item, block style | `tags: []` — `serialiseList`'s existing answer, unchanged | none |
| Remove `recordings`, recording note | `session:` present | refused; `role="alert"` carries the reason; nothing written; the chip stays | none |
| Remove `Recordings` / `RECORDINGS` / `recordings ` | any spelling Rust folds | refused identically — `namesTag`, not `===` | none |
| Remove `recordings`, ordinary note | no `session:` | allowed, and no alert: keeper owns no row in a vault it did not write | none |
| `session:` | recording note | read-only, monospace, copyable through `OverflowValue`; no textbox, no spinbutton | none |
| Different casing of a tag already on the note | `Work` with `work` chipped | no create row; `"Work" is already on this list.`; Enter writes nothing | none |
| Different casing of a vocabulary tag | `CLIENT/ANVIL` | offers `client/anvil`; Enter writes the vault's spelling | none |
| A tag the vault has never seen | `client/newco` | create row, last; Enter writes it verbatim | none |
| Vocabulary unreadable | `tags_vocabulary` rejects | nothing to browse, typing still works, creating still writes | none |
| Vocabulary not asked for | panel mounted, chooser closed | `tags_vocabulary` is NOT called | none |
| Chooser dismissed | Escape on an empty query | closed, focus back on the toggle | none |
| `tags:` written as an empty scalar | `tags:` with nothing after it | still the tag row; the first add writes `tags: [x]` | none |
| `tags:` nested under a map | indented `key: value` beneath it | the panel's existing "nested value — edit it in the note" | none |
| Block keeper cannot parse | unchanged | raw preview, no controls at all | none |
| A non-recording note's `tags:` | ordinary note | the same chooser — a tag is a tag wherever it is chosen (44.13) | none |

</intent-contract>

## Code Map

- `src/components/notes/properties-panel.tsx`
  - `TAGS_KEY`, `RECORDINGS_TAG`, `ADD_NOTE_TAG`, `recordingsTagRefusal` — **new.**
  - `TagsProperty` — **new.** Chips, the toggle, `TagCombobox` with `allowCreate`, the refusal.
  - `PropertyControl` — the read-only identity branch now covers `session:` as well as `id:`.
  - The entry map routes `tags:` to `TagsProperty`; `recording:`/`files:` routing is unchanged.
- `src/components/notes/properties-panel.test.tsx` — `tagsVocabulary` added to the module mock; a
  new suite, `PropertiesPanel — a recording note's tags`.

Nothing else changed. No Rust, no IPC, no generated bindings.

## Tasks & Acceptance

**Execution:**
- [x] The tag row mounts 44.13's chooser with `allowCreate`; no second chooser was written.
- [x] `session:` is read-only, beside `id:`, under one rule.
- [x] Removing `recordings` from a recording note is refused with the reason on screen.
- [x] The write is the panel's existing splice; byte identity asserted, not `toContain`.
- [x] The vocabulary is 42.5's, read on open.
- [x] Every test proved by mutating the code it defends.

**Acceptance Criteria:**
- `bun run test src/components/notes/properties-panel.test.tsx` — 33 passed (was 22).
- The suites that mount the real `NoteEditor` (which imports this module) or the same chooser —
  `attachments-panel.test.tsx`, `format-toolbar.test.tsx`, `editor/tab-wiring.test.tsx`,
  `note-filter-bar.test.tsx` — 42 passed.
- `biome check` over the two touched files — clean.

## Design Notes

**Two protections, two different shapes, and that is deliberate.** `session:` gets no control;
`recordings` gets a control that refuses out loud. They are not inconsistent — they are the same
rule applied to two different situations. `session:` sits in a column of typed controls where
`id:` already renders as read-only text, so a reader meets it as a fact about the note rather than
as a field that rejected them; there is no invitation to refuse. The `recordings` chip sits in a row
of chips that all come off with one press, so the invitation is already made by every sibling next
to it, and taking only this one's `×` away would be the unexplained disabled control the epic keeps
finding. Refusing is the only shape that answers "why did that not work" at the moment it is asked.

**The refusal is case-folded because Rust's is.** `recording_note.rs` puts its kind tag through
`tags::normalise` before emitting it, and its own suite pins `Recordings`, `RECORDINGS` and
`recordings ` as the same tag. A `item === "recordings"` guard here would have protected exactly one
of the four spellings and left the other three looking identical on screen and removable — a
protection that is right in the test fixture and wrong in a vault where someone once typed a capital
letter. `namesTag` is the comparison because 44.13 already wrote it and it folds case and segments
the same way; it deliberately stops short of re-implementing `normalise`, which is correct here:
anything only Rust's folding would reconcile is not a spelling of the kind tag that this note can
have on screen, because Rust wrote the tag.

**The constant is mirrored, and the mirror is the safe direction.** `RECORDINGS_TAG` is now spelled
in TypeScript as well as in Rust. Reading it off the wire would need a new IPC surface for a
five-letter string, and 44.14 is a frontend-only story; hand-writing a generated binding is
forbidden. The exposure is bounded by which way a drift fails: if Rust ever renames the kind tag,
this panel stops protecting it and starts allowing its removal — a lost protection, visible the
first time somebody looks, not a corrupted file. The reverse mirror (Rust reading a TS constant) is
not available and would be worse.

**`tags:` is claimed for every note, not only recording notes.** The story is about the recording
note, but the defect is about the key. 44.13's mandate is that every place a tag is chosen uses the
one control, and the frontmatter tag row is the last such place in the notes pane. Scoping the
chooser to recording notes would have left the ordinary note — the common case — still writing
unvalidated text into the same vocabulary. Only the two protections are scoped, and they are scoped
by the note's own `session:`, which is the same predicate the file actions above already use.

**An empty `tags:` is still the tag row.** Clearing the last tag in Obsidian leaves `tags:` with
nothing after it, which the panel's reader classifies as an empty scalar rather than a list. Falling
through to the generic control there would put the bare text box back in front of exactly the note
that has no tags yet — the one that most needs the vocabulary. The first add then writes
`tags: [x]`, which is `serialiseList`'s existing answer for a non-block style and is valid for
Obsidian to read back.

**`chosen={entry.items}` is what stops the duplicate, and it is stale for one round trip.** The
chooser hides tags the note already has and refuses to create a second casing of one. Its input is
the note's items as parsed from the block, so between an add and the parent adopting the write the
list is one tag behind. That is the same shape the filter bar has and it self-corrects on `onSaved`;
the alternative — the control keeping its own copy of the note's tags — is a second place the note's
tags would live.

**No `useMemo`, no debounce, no optimistic chip.** A tag write is a file write and there are at most
a handful of tags; the panel already writes on blur for the same reason.

## Verification

Every test below was proved by mutating the production code it defends, running the suite, watching
it fail, and restoring. Baseline was 33 passed before and after each mutation, and the file was
diffed against its pre-mutation copy at the end.

| Mutation | Tests that caught it |
|---|---|
| `session:` dropped from the read-only identity rule (editable text box again) | `never offers to edit the session id` |
| The recordings-tag guard removed entirely (silent removal restored) | `refuses to remove the tag that makes the note findable, and says why`; `refuses whatever spelling of it the note carries` |
| The guard compares `item === RECORDINGS_TAG` instead of `namesTag` | `refuses whatever spelling of it the note carries` |
| The guard drops `sessionId !== null` (protects the tag in everybody's note) | `lets an ordinary note drop a tag it happens to call recordings` |
| `tags:` routed back to the generic list control | 7 tests: `adds a tag…`, `writes the vault's spelling…`, `will not make a second tag…`, `still creates a tag…`, `keeps the chooser usable…`, and both refusal tests |
| `allowCreate` removed from the chooser | `still creates a tag the vault has never seen, verbatim`; `keeps the chooser usable when the vocabulary cannot be read` |
| The vocabulary read on mount instead of on open | `does not read the vocabulary until the chooser is asked for` |
| `chosen` not passed to the chooser | `will not make a second tag out of a different casing of one already here` |
| `serialiseList` rewrites a block list as flow (byte preservation broken) | `adds a tag, and the rest of the block is the same bytes`; `removes a tag…, and the rest of the block is the same bytes`; `still creates a tag…`; `keeps the chooser usable…`; `writes the vault's spelling…`; plus the pre-existing `writes a list edit in the style the note already used` |

**What could not be proved here.** Four things, stated plainly, because this epic has now twice
shipped a claim proved through the wrong layer:

1. **Nothing was driven in a real browser or in the shell.** Every claim above is a jsdom claim
   about rendered roles, `document.activeElement` and the arguments handed to a mocked `notesSave`.
   Whether the refusal sentence is legible where it lands — it renders under a two-column grid cell
   in a narrow panel and it is three lines of text — is a judgement only somebody looking at it can
   make. Whether the chooser's 12rem list has room to open inside the properties section on the
   owner's window is likewise unmeasured.
2. **No file was written.** `notesSave` is mocked. The byte-identity assertions are about the block
   this panel hands to Rust, which is the boundary this story owns — but "the file on disk is what
   the panel showed" additionally depends on `notes_save` and `Frontmatter`, neither of which this
   story touched and neither of which this suite exercises. A round trip through the real writer,
   and Obsidian reading the result back, is unverified on this host.
3. **`tagsVocabulary()` is called without a vault id**, so Rust resolves the active vault. That the
   active vault is the vault the open note lives in is true by construction in the shell and is
   asserted nowhere; it is the same call shape 44.13's filter bar and 42.5's recording card use, and
   this story did not make it truer. The panel has no vault id to pass — it takes a subscription id.
4. **The kind-tag spelling is pinned by a TypeScript constant, not by Rust.** No test in this repo
   fails if `recording_note.rs::RECORDINGS_TAG` changes and this file does not. The failure mode is
   named in the design notes above and is a lost protection, not a corrupted note.

## Deliberately Not Done

- **No quoting of tag items on write.** `serialiseList` emits `  - <item>` verbatim, which it did
  before this story for every list key. A created tag containing `: ` or a leading `[` would write a
  block a YAML reader mis-parses. It is equally reachable through the text box this replaced, it is
  a property of `serialiseList` rather than of the tag row, and inventing a quoting rule for one key
  would be a second serialiser beside the one `Frontmatter` owns. Named here so the next person
  fixing it fixes it in one place.
- **The `recordings` chip is not visually distinguished.** It looks like every other chip and
  refuses when pressed. Marking it up front — a lock glyph, a muted colour — is a design decision
  nobody made, and the failure this story is fixing is an unexplained inert control, which a
  distinguishing mark with no words next to it also is.
- **`recording:` and `files:` stay read-only.** Story 42.4's reasoning is unchanged: typing a
  different path moves no file, it only makes the note lie. This story widened the read-only rule to
  `session:`; it did not narrow it anywhere.
- **No undo for a removed tag.** The write is one `notesSave` and the note's history panel already
  has the previous revision. A tag-specific undo would be a second history.
- **The recording metadata card still uses its `<datalist>`.** 44.13 deferred it here on the grounds
  that 44.14 owns that surface. It does not: 44.14's target is the note's properties panel, and the
  card is a comma-separated multi-tag field in the capture pane, not a chooser. Converting it is a
  change to what that field IS and belongs to whoever owns the capture surface next. Said out loud
  so it stops being handed forward as already-assigned.
- **No virtualisation of the option list.** 44.10 owns that epic-wide.
