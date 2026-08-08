---
title: 'Story 42.4: The Note Stub, at the Only Moment It Will Be Written'
type: 'feature'
created: '2026-08-08'
status: 'done'
blocking_condition: ''
baseline_revision: '824d76f'
final_revision: '553ff2795fa058ec46416ad557a205f9d73955e3'
review_loop_iteration: 0
followup_review_recommended: false
context:
  - '{project-root}/docs/project-context.md'
  - '{project-root}/_bmad-output/planning-artifacts/epic-42-the-recordings-archive.md'
  - '{project-root}/_bmad-output/implementation-artifacts/spec-42-1-a-session-is-a-row.md'
---

<intent-contract>

## Intent

**Problem:** nobody documents a meeting an hour later. The minute the recording stops is the entire
window in which anything will ever be written about it, and today keeper spends that minute showing a
summary card and then closing it. What the session was *for* is lost in the one moment it was still
in someone's head.

**Approach:** finalize composes a markdown stub — prefilled with everything keeper already knows —
and the stop surface presents it with the cursor in the body. One key dismisses. A stub the user
never touched is **deleted** rather than left behind, because an archive full of empty notes is worse
than one with none.

## Boundaries & Constraints

**Always:**
- The frontmatter is the notes subsystem's frontmatter. It round-trips through
  `notes::frontmatter::Frontmatter::parse` unchanged — not "parses without erroring", but the
  keys read back byte-identically and the body offset is exact, the way
  `a_keeper_authored_block_round_trips_its_own_keys` asserts for a keeper-authored block.
- The `session:` link carries the **immutable** session id (Story 40.3), never a path. A retitle
  moves the folder; the link must survive it.
- Every path in the stub is relative. FR-145's rule, and the same reason 42.1's columns are.
- Two sessions stopped in the same minute produce two stubs with **distinct names**. A minute-
  resolution stamp is not a unique name and must not be treated as one.
- Composition is pure and lives in `keeper-core`; every byte of file IO lives in the shell, which is
  the rule `notes/mod.rs` states for the whole notes subsystem ("It takes bytes and returns values.
  It never opens a file").
- A stub that cannot be written is logged, never a recording failure. Finalize already succeeded.

**Block If:**
- Writing the stub would land it inside the recordings folder of a profile that is also a vault.
  `RecordingsConfig::validate` refuses overlapping subtrees, so "through the notes writer" means a
  sibling subtree in the vault — not the session folder. If no such destination resolves, the stub
  is written beside the session folder instead, and that degrade is logged rather than guessed at.

**Never:**
- No tag normalisation or resolution against the notes tag tree (42.5). Tags are carried as stored.
- No transcription, summarisation or any inference. Named because it is the first thing a "note about
  a recording" invites.
- No edit of a stub the user did touch, ever, by any later automatic pass.
- No absolute path in the stub or its frontmatter.

## I/O & Edge-Case Matrix

| Scenario | Input / State | Expected | Error |
|---|---|---|---|
| Stop a recording | a session finalizes | exactly ONE stub, prefilled, presented with the cursor in the body | logged, not surfaced |
| Dismiss untouched | user presses the dismiss key, body unedited | no file remains on disk | none |
| Edit then save | user types and saves | the file stays; in a vault it appears in the notes index | honest error |
| Two in one minute | two sessions stop within the same minute | two stubs, distinct names, neither overwritten | none |
| Frontmatter round-trip | the composed stub | `Frontmatter::parse` reads every key back unchanged and the body offset is exact | none |
| Session link | any stub | carries the immutable session id, never a path | none |
| Destination is a vault | profile flagged as both | written through the notes writer into a vault subtree, not into the recordings folder | degrade logged |
| Destination is a plain folder | no vault | written beside the session folder | none |
| Untitled session | no `meta.title` | a stub whose title is the session's date, never an empty heading | none |
| No participants, no tags | bare session | those lines are omitted, not left as empty labels | none |
| Write fails | read-only volume | logged with ids only; the recording is untouched and still finalized | never surfaced as a recording failure |
| Stub already exists | a re-finalize of the same session | the existing stub is left alone; a second is not written | none |

</intent-contract>

## Code Map

- `src-tauri/crates/keeper-core/src/notes/` — a new submodule for stub composition. Pure: it takes the
  session's facts and returns `(filename, contents)`. It never opens a file, per `notes/mod.rs`.
- `src-tauri/crates/keeper-core/src/notes/frontmatter.rs` — read-only. `Frontmatter::serialise_new`
  composes the block; `Frontmatter::parse` is what the round-trip test asserts against.
- `src-tauri/crates/keeper-core/src/notes/naming.rs` — read-only, and the uniqueness precedent:
  `note_filename(title, date, taken)` plus a caller-supplied set of taken names.
- `src-tauri/crates/keeper/src/notes_vault.rs` / `notes_ipc.rs` — where the write happens, and where
  `siblings()` supplies the taken-name set inside a vault. **Beside a session folder there is no such
  helper today** — this story supplies the equivalent, and it must read the directory rather than
  trust a stamp.
- `src-tauri/crates/keeper/src/ipc.rs` — finalize composes and writes; the stop surface's VM gains
  the stub, and a dismiss command deletes an untouched one.
- `src/components/layout/recording-summary-card.tsx` — presents the stub with the cursor in the body.
- Read-only: `spec-42-1` (the identity and the relative-path rule), `notes/mod.rs` (the pure/impure
  boundary and `NotesError`'s variants).

## Tasks & Acceptance

**Execution:**
- [x] Pure stub composition in `keeper-core`, frontmatter through the notes serialiser.
- [x] Collision-safe naming, by reading the destination directory — never by trusting the stamp.
- [x] Vault-aware destination resolution, with the beside-the-folder degrade logged.
- [x] Write at finalize, best-effort, never a recording failure.
- [x] The stop surface presents it with the cursor in the body; one key dismisses.
- [x] Dismiss-untouched deletes the file.
- [x] Tests: every matrix row, plus the frontmatter round-trip.

**Acceptance Criteria:**
- Stopping a recording writes exactly one stub whose frontmatter round-trips through the notes
  frontmatter parser unchanged.
- Editing and saving keeps it, and it appears in the notes index when the destination is a vault.
- Dismissing without editing leaves no file.
- The stub contains no absolute path.
- Two sessions stopped in the same minute produce two stubs with distinct names.

## Design Notes

**The composer takes both the RFC 3339 stamps and epoch milliseconds, and that is not redundancy.**
`keeper-core` has no calendar library (AD-55 declines one), so epoch milliseconds alone cannot yield a
local date: a session at 00:30 local in UTC+2 is 22:30Z the previous day, and the note's date — and
its filename — would be wrong by a day. The offset-carrying string supplies the local calendar; the
epoch pair supplies the absolute span, which is the only honest way to measure a duration without
calendar arithmetic. The shell derives both from the same two stamps, so they cannot disagree, and
the crate stays clock-free because both arrive as parameters.

**`notes::templates::Stamp` was widened to `pub(crate)` rather than duplicated.** A second RFC 3339
reader for a format keeper itself writes is two parsers that will eventually disagree about what date
`2026-08-08T00:30:00+02:00` is.

**"Never edited" is byte-identity against a recomposition, computed at dismiss time.** The alternative
was a stored hash, and it is worse precisely where it matters: a hash is a second record of the truth
and goes stale when the file is edited outside keeper — the one case where deleting is unrecoverable.
Recomposition carries no such state. The cost, accepted deliberately: a stub composed by an older
build stops matching a newer one and becomes undismissable. That is the safe direction, and dismissal
happens seconds after finalize in the same build.

**Every uncertainty keeps the file.** Unreadable, no frontmatter, a manifest that no longer composes,
a failed delete — all return "kept". And the dismiss command takes only a folder, so no argument the
caller could get wrong can widen what gets deleted.

**Dismiss unlinks rather than trashing.** `notes_vault::trash_note` exists to protect bytes a person
wrote; byte-identity has just proved nobody wrote any, and a trash copy is exactly the empty-note
litter dismissing exists to prevent. AC3 says no file remains.

**The surface never holds keeper's frontmatter.** The VM carries `bodyOffset` in UTF-16 code units,
and the textarea is given only `contents.slice(bodyOffset)`; a save sends head + draft. So the user
cannot type inside the block AC1's byte-identical round-trip depends on, and the dismiss check keeps
meaning what it says.

**Escape saves a dirty draft before dismissing, always.** The dismiss command judges byte-identity on
disk, so typing and pressing Escape without saving would have deleted the words. A save that rejects
skips the dismissal by control flow — deleting the pristine file those words were meant to replace is
the one unrecoverable move available here.

**A stub is located by reading the `session:` frontmatter field of the `.md` files in the directory**
(head-capped), not by guessing at a filename. The filename is derived from a title the user can
change; the session id cannot be.

**The stub lives in the session folder's parent, or a vault subtree — never inside the session
folder.** `RecordingsConfig::validate` refuses overlapping subtrees, so "through the notes writer"
could never have meant the recordings folder. A retitle therefore moves the session without moving
its note.
