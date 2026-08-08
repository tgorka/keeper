---
title: 'Story 41.7: Recording Into a Synced Folder You Can Choose'
type: 'feature'
created: '2026-08-08'
status: 'in-progress'
blocking_condition: ''
baseline_revision: '13db998'
final_revision: ''
review_loop_iteration: 0
followup_review_recommended: false
context:
  - '{project-root}/docs/project-context.md'
  - '{project-root}/_bmad-output/planning-artifacts/epic-41-a-finished-segment-is-already-on-the-drive.md'
  - '{project-root}/_bmad-output/implementation-artifacts/spec-41-1-a-profile-can-say-it-holds-recordings.md'
  - '{project-root}/_bmad-output/implementation-artifacts/spec-41-2-choosing-where-recordings-live-once.md'
---

<intent-contract>

## Intent

**Problem, reported from the field:** the Recording pane offers only a plain folder. To record into a
synced folder the owner would have to know that a `recordings` block exists in a profile's JSON, and
set it through `keeper-syncd` — because **nothing in the app can flag a profile as holding
recordings**. Story 41.2 built the destination picker and 41.1 built the config it reads, but the one
switch that makes either reachable was never built. Observed on the owner's machine: two profiles,
both with `recordings: null`, one of them already carrying a `notes` block set through the app's own
notes-vault switch. The asymmetry is the whole bug — notes got its switch, recordings did not.

**Second problem, from the same report:** both of those profiles live on `/Volumes/merope`, a
pendrive. A destination that is sometimes not attached is a first-class case here (AD-48), and the
destination resolver cannot currently see it: `DestinationProfileRow` carries no removability and no
volume status, so a chosen profile on a detached drive resolves to a path under a mountpoint that is
not there. What happens then is a filesystem error naming a path, not a sentence naming the drive.

**Approach:** the switch, mirroring the notes-vault switch exactly — same place, same shape, same
kind of sentence — plus removable-awareness at the two moments it matters: when the owner is choosing
a destination, and when a recording is about to start.

## Boundaries & Constraints

**Always:**
- Mirror the notes-vault control (`SYNC_NOTES_LABEL` and its subfolder field). A second idiom for
  "this folder also holds X" is a worse outcome than the missing switch, because it makes the next
  one ambiguous.
- The subfolder defaults to `RecordingsConfig`'s own default. The default lives in Rust already and
  must not be restated in TypeScript.
- `RecordingsConfig::validate` already refuses an empty, absolute, escaping, or notes-overlapping
  subfolder. The form surfaces those refusals; it does not re-implement them, and it does not correct
  the owner's input silently.
- A removable destination says so **before** a recording starts, on the card where the choice is
  made. A person choosing a pendrive should learn it is a pendrive from the app, not from a failure.
- When the volume is absent, the refusal **names the volume**. `merope is not attached` is a sentence
  someone can act on; an `EPERM` on `/Volumes/merope/tgdrive/recordings` is not.

**Block If:**
- Flagging would create an overlap `RecordingsConfig::validate` refuses. That refusal is the answer,
  surfaced verbatim — the form must not pick a different subfolder to make the save succeed.

**Never:**
- No silent fallback to the plain folder when the chosen synced destination is unavailable. Story
  41.2's resolver already degrades for a profile that is gone, paused or unflagged, and each degrade
  is logged; a recording that quietly lands somewhere other than where the card said is the one
  outcome this story must not add. An absent volume is refused, not redirected.
- No writing outside the volume's own tree to "hold" a recording until the drive returns. That is a
  spooling feature with its own durability story, and inventing it here would be the third place
  recordings can live.
- No change to `RecordingsConfig`, its validation, or the resolver's existing degrade rules.

## I/O & Edge-Case Matrix

| Scenario | Input / State | Expected | Error |
|---|---|---|---|
| Flag a profile | switch on, subfolder left alone | `recordings` block written with the Rust default subfolder | none |
| Flag with a custom subfolder | `media/screen-recordings` | written as given | none |
| Subfolder overlaps the notes vault | notes at `10-notes`, recordings at `10-notes/rec` | the save is refused, naming the conflict | validation message, verbatim |
| Subfolder empty / absolute / escaping | `""`, `/tmp`, `../x` | refused, naming which rule | validation message, verbatim |
| Unflag a profile | switch off | the `recordings` block is removed; a destination pointing at it degrades per 41.2 | none |
| Unflag the CHOSEN destination | it is the current destination | the choice degrades to the plain folder, and the card says so | none |
| Picker appears | one flagged profile | the destination picker offers it — 41.2's existing machinery, now reachable | none |
| Removable destination, attached | pendrive mounted | records normally; the card says the folder is on removable media | none |
| Removable destination, detached | pendrive out, Record pressed | refused before any file is created, naming the volume | honest, non-retriable |
| Removable destination, detached, card open | pendrive out | the card says so without being asked, before Record is pressed | none |
| Volume returns | pendrive replugged | the next start works with no further action | none |
| Non-removable profile | an ordinary synced folder | no removable wording anywhere | none |

</intent-contract>

## Code Map

- `src/components/sync/add-folder-form.tsx` — the recordings switch, beside the notes-vault switch it
  mirrors.
- `src-tauri/crates/keeper/src/sync_ipc.rs` — the save path carries the `recordings` block, the way it
  already carries `notes`.
- `src-tauri/crates/keeper/src/ipc.rs` — `DestinationProfileRow` gains removability and volume status;
  `resolve_recording_destination` learns the absent case; `recording_start` refuses with the volume's
  name.
- `src/components/recording/recording-destination-controls.tsx` — the removable sentence on the card.
- Read-only and unchanged: `keeper-sync/src/profile.rs` (`RecordingsConfig` + `validate`),
  `keeper-sync/src/volume.rs` (`VolumeStatus`), 41.2's degrade rules.

## Tasks & Acceptance

**Execution:**
- [ ] The recordings switch + subfolder field, mirroring the notes-vault control.
- [ ] The save path writes and clears the `recordings` block; validation refusals surface verbatim.
- [ ] `DestinationProfileRow` carries removability and volume status.
- [ ] An absent volume refuses the start, naming the volume, before any file is created.
- [ ] The destination card says a folder is on removable media, and says when it is not attached.
- [ ] Tests: every matrix row.

**Acceptance Criteria:**
- Flagging a synced folder in the app makes it appear in the Recording destination picker, with no
  `keeper-syncd` step.
- A subfolder the shared validator refuses is refused in the form with that validator's own words.
- With the destination's volume detached, pressing Record refuses by naming the volume and creates no
  file anywhere.
- Replugging the volume makes the next start succeed with no other action.
- A non-removable synced destination shows no removable wording.

## Design Notes

(Written at the end of implementation.)
