---
title: 'Story 41.2: Choosing Where Recordings Live, Once'
type: 'feature'
created: '2026-08-07'
status: 'review'
blocking_condition: ''
baseline_revision: '2265b83'
final_revision: ''
review_loop_iteration: 0
followup_review_recommended: false
context:
  - '{project-root}/docs/project-context.md'
  - '{project-root}/_bmad-output/planning-artifacts/epic-41-a-finished-segment-is-already-on-the-drive.md'
---

<intent-contract>

## Intent

**Problem:** `recording.destination_dir` is a bare absolute path, and story 41.1 just gave profiles a
way to say they hold recordings. Between those two facts sits an ambiguity the user cannot see: a
destination that happens to sit inside a synced profile's tree already syncs — by accident, on the
profile's terms, with no statement anywhere that it does. Epic 41's position is that sync is a
consequence of where recordings live, not a checkbox beside it (UX-DR47), which means the destination
has to become a resolved DECISION rather than a path.

**Approach:** `recording.destination_profile_id` joins `recording.destination_dir` in the settings k/v,
and exactly one of them is in force. `RecordingSettingsVm` carries which choice is active, the RESOLVED
absolute root, and the profile's name, so every surface renders one line of truth instead of deriving
it. The setter refuses a profile that is not recordings-flagged, and refuses a plain directory inside a
synced profile's tree that is not that profile's recordings root — the ambiguous case. The destination
card offers "a folder" or "a synced folder", and when a synced folder is chosen it states the
consequence in the recorder's words.

## Boundaries & Constraints

**Always:**
- Exactly one of the two keys is in force, and the VM says which. Both set is a state the setter
  refuses to create and the getter resolves deterministically (profile wins, and it says so in a log
  line) rather than guessing silently.
- The resolved root is computed in ONE place — Rust — and handed to the frontend. No surface joins
  `local_path` and a subfolder itself.
- A profile id that is not recordings-flagged is refused by the command, not merely hidden by the UI.
- A plain folder inside a synced profile's tree is refused UNLESS it is exactly that profile's
  recordings root, and the refusal names the profile it would have collided with.
- With no recordings-flagged profile present, the surface behaves exactly as it does today: one folder
  chooser, no new copy, no empty picker.
- Choosing a profile persists across a restart, and the resolved root survives a profile rename (it is
  resolved from the id, never cached as a string).

**Block If:**
- The resolved root could not be produced without `keeper-sync` on the shell's dependency edge. It can:
  the shell already depends on `keeper-sync` (desktop only), and the getter degrades to the plain-path
  answer when the engine is unavailable.

**Never:**
- No second "also sync my recordings" toggle. The destination decides, and the card states the
  consequence.
- No writing into a profile's `recordings` block from this surface — flagging a profile is
  `keeper-syncd`'s job (41.1 made it a preserved field for exactly that reason).
- No change to how `recording_start` resolves its root beyond reading the new effective answer.

## I/O & Edge-Case Matrix

| Scenario | Input / State | Expected | Error |
|---|---|---|---|
| Fresh install | neither key set | `kind: "folder"`, resolved root = today's default `~/Movies/keeper` | none |
| Plain folder | `destination_dir` set, no profile id | `kind: "folder"`, resolved root = that path, `profileName: null` | none |
| Synced folder | `destination_profile_id` = a flagged profile | `kind: "profile"`, resolved root = `<local_path>/<recordings.subfolder>`, `profileName` = the profile's name | none |
| Profile renamed | same, after a rename | the VM shows the new name and the same resolved root | none |
| Profile un-flagged behind our back | id set, profile's `recordings` now `None` | the getter degrades to the plain-path answer and logs why; the read never fails | none |
| Profile deleted | id set, no such profile | same degrade + log | none |
| Engine unavailable (no git) | id set | same degrade + log — a machine with no git can still record | none |
| Setting a non-flagged profile | `recording_settings_set` with that id | refused, naming the profile and what it lacks; nothing written | typed `IpcError` |
| Setting an unknown profile | a bogus id | refused; nothing written | typed |
| Ambiguous plain folder | a path inside profile `tgdrive`'s tree, not its recordings root | refused, naming `tgdrive` | typed |
| The unambiguous exception | a path that IS `tgdrive`'s recordings root, chosen as a plain folder | accepted and normalised to the PROFILE choice, because they are the same place and one of them carries the consequence | none |
| Plain folder outside every profile | any other absolute path | accepted | none |
| Both keys somehow set | a hand-edited `config.json` | the profile wins, a `warn` says so, and the next write clears the loser | none |
| Default when exactly one flagged profile exists | fresh install, one flagged profile | that profile is the DEFAULT choice (subfolder `recordings`, media `PointerOnly`, push `SessionEnd`) | none |
| No flagged profile | fresh install, none | the surface is today's surface | none |

</intent-contract>

## Code Map

- `src-tauri/crates/keeper-core/src/registry.rs` — `RECORDING_DESTINATION_PROFILE_KEY` +
  getter/setter beside the `destination_dir` pair.
- `src-tauri/crates/keeper-core/src/vm.rs` — `RecordingSettingsVm` gains
  `destination_kind: RecordingDestinationKind` (`Folder` | `Profile`), `destination_profile_id:
  Option<String>`, `destination_profile_name: Option<String>`, and keeps `destination_dir` as the
  RESOLVED absolute root. Bindings regenerate — the epic says yes.
- `src-tauri/crates/keeper/src/ipc.rs` — `effective_destination_dir` learns the profile answer;
  `read_recording_settings` fills the new fields; `write_recording_settings` gains the two refusals;
  a `recordings_profiles()` read for the picker (or an extension of the existing sync-profile list
  command).
- `src/lib/ipc/client.ts` + `src/components/recording/recording-destination-controls.tsx` — the
  "a folder" / "a synced folder" choice, the resolved line, and the consequence sentence.

## Tasks & Acceptance

**Execution:**
- [ ] Registry key + accessors.
- [ ] VM fields + the `RecordingDestinationKind` enum; regenerate bindings.
- [ ] Resolution in one place, with the degrade-and-log rules for every unusable profile answer.
- [ ] The two setter refusals, each naming what is wrong and what it would have collided with.
- [ ] The normalisation of "plain folder that IS a profile's recordings root" to the profile choice.
- [ ] The picker's data source: which profiles are recordings-flagged.
- [ ] The frontend choice, the resolved line, the consequence sentence, and today's behaviour when no
      flagged profile exists.
- [ ] Rust tests: every matrix row. Frontend tests: the choice, the refusal rendered verbatim, and the
      no-flagged-profile case.

**Acceptance Criteria:**
- Given a flagged profile chosen as the destination, when the settings are read, then the resolved root
  is `<local_path>/recordings` and the profile's name is carried; it survives a restart and a rename.
- Given a non-flagged profile id, when `recording_settings_set` runs, then it is refused and nothing is
  written.
- Given a plain folder inside a synced profile's tree that is not its recordings root, when it is
  submitted, then it is refused with a message naming that profile.
- Given no recordings-flagged profile, when the destination card renders, then it is today's card.
- Given `bun run bindings:check`, when it runs against the committed tree, then it exits 0.

## Design Notes

(Written at the end of implementation.)
