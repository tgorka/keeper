---
title: 'Story 41.1: A Profile Can Say It Holds Recordings'
type: 'feature'
created: '2026-08-07'
status: 'done'
blocking_condition: ''
baseline_revision: '8b3e0e2'
final_revision: '06e76e3a2622bd57bff5bcb8efb466c8e3741572'
review_loop_iteration: 0
followup_review_recommended: false
context:
  - '{project-root}/docs/project-context.md'
  - '{project-root}/_bmad-output/planning-artifacts/epic-41-a-finished-segment-is-already-on-the-drive.md'
---

<intent-contract>

## Intent

**Problem:** A sync profile can already say it holds a notes vault (`notes: Option<NotesConfig>`), and
nothing lets it say it holds recordings. Epic 41 needs that fact to exist before anything can act on
it: story 41.2 resolves a recording destination to a profile, 41.4 guards its `note_finished`
assertion by "inside this profile's recordings root", and 41.5 decides when to push by this profile's
policy. Today none of those have a place to read from.

**Approach:** `RecordingsConfig` is `NotesConfig` applied a second time (AD-66): a `#[serde(default)]`
field on the profile's JSON blob, so the migration IS the serde attribute — a `sync.db` written by
0.6.5 loads with `recordings: None` and says nothing. It carries the subfolder, the media policy and
the push policy, validates like `NotesConfig` does, and additionally refuses to overlap that profile's
notes vault in either direction, because one folder cannot be both a vault and a recordings root
without two subsystems writing the same tree.

## Boundaries & Constraints

**Always:**
- Validation happens at CONSTRUCTION, through `SyncProfile::validate`, so an invalid config cannot be
  built rather than being caught at use.
- The overlap check is symmetric: recordings inside notes, notes inside recordings, and the two being
  equal are all refused, and the refusal names both subfolders.
- `PushPolicy::SessionEnd` is the default, because pushing a 2 GB LFS object during the meeting eats
  the uplink the meeting runs on.
- Every field round-trips through the JSON blob, including which `PushPolicy` variant is in force.
- Absent means absent: a profile with no `recordings` block is not "recordings with defaults".

**Block If:**
- `NotesConfig` turned out not to be a usable template. It is: same shape, same validation rules, same
  `#[serde(default)]` migration story.

**Never:**
- No new storage, no new table, no IPC surface, no VM, no bindings — this story is a type and its
  validation. Nothing reads it yet.
- No default subfolder written into an existing profile as a side effect of loading it.

## I/O & Edge-Case Matrix

| Scenario | Input / State | Expected | Error |
|---|---|---|---|
| 0.6.5 blob | JSON with no `recordings` key | loads, `recordings: None`, no log line | none |
| Round trip | a fully configured block | every field survives, push variant included | none |
| Default subfolder | `RecordingsConfig::default()`-shaped construction | `recordings` | none |
| Empty subfolder | `""` or `"   "` | refused at construction | typed `SyncError` |
| Absolute subfolder | `/Users/x/rec` | refused | typed |
| Escaping subfolder | `../evil`, `a/../..` | refused | typed |
| Overlap: equal | notes `vault`, recordings `vault` | refused, naming both | typed |
| Overlap: nested | notes `vault`, recordings `vault/rec` | refused, naming both | typed |
| Overlap: reversed | notes `vault/notes`, recordings `vault` | refused, naming both | typed |
| Sibling folders | notes `vault`, recordings `recordings` | accepted | none |
| Prefix but not path | notes `rec`, recordings `recordings` | accepted — `rec` is not an ancestor of `recordings` | none |
| Root join | a valid config | `recordings_root()` = `local_path/subfolder` | none |
| Media policy | `Materialize` / `PointerOnly` | both round-trip; `PointerOnly` is what a phone-sized clone wants | none |
| Push window | `Window { quiet_from, quiet_to }` | round-trips; validation of the window's own shape follows `NotesConfig`'s strictness | typed when malformed |

</intent-contract>

## Code Map

- `src-tauri/crates/keeper-sync/src/profile.rs` — `RecordingsConfig`, `MediaPolicy`, `PushPolicy`,
  `DEFAULT_RECORDINGS_SUBFOLDER`, `SyncProfile.recordings`, `recordings_root()`, and the
  `SyncProfile::validate` call plus the notes-overlap rule.
- Read-only reference: `NotesConfig` and `vault_root()` in the same file — the pattern being applied a
  second time — and `keeper-sync/src/db.rs`'s profile blob read/write.

## Tasks & Acceptance

**Execution:**
- [ ] `RecordingsConfig` + `MediaPolicy` + `PushPolicy` with serde shapes matching the file's
      conventions, and doc comments saying WHY each exists (AD-66, AD-70).
- [ ] `DEFAULT_RECORDINGS_SUBFOLDER`.
- [ ] `SyncProfile.recordings: Option<RecordingsConfig>` behind `#[serde(default)]`.
- [ ] `RecordingsConfig::validate` mirroring `NotesConfig::validate`, plus the symmetric notes-overlap
      refusal naming both subfolders.
- [ ] `SyncProfile::recordings_root()` and the `validate` wiring.
- [ ] Tests: every matrix row.

**Acceptance Criteria:**
- Given a `sync.db` profile blob written by 0.6.5, when it loads, then `recordings` is `None` and no
  error line is emitted.
- Given `recordings.subfolder = "../evil"`, when the profile is constructed, then it is refused with a
  typed `SyncError`.
- Given a recordings subfolder equal to, inside, or containing the notes subfolder, when validated,
  then it is refused with a message naming both.
- Given a configured profile, when it round-trips through the JSON blob, then every field including the
  push-policy variant is preserved.

## Design Notes

(Written at the end of implementation.)
