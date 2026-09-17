---
status: review
baseline_revision: 6e5695e
final_revision: ''
---

# Story 74.3 — A folder in the drive holds the task ledger

<intent-contract>

## Problem

The owner asked for the task configuration and its run history to live **in a drive**, in a folder he picks "the same way as with notes". Nothing like it exists: a task's configuration lives in `sync.db` on one machine, and its run history is 50 rows in the same database (`db.rs:223-231`). Neither travels, and neither can be read by a person or by keeper on the other machine.

## Approach

Do exactly what the notes vault does, because it already solved this: a flag on the synced folder, a validated subfolder, and a machine-local selector.

## Always

`[folder.tasks] subfolder` on the profile (`TasksConfig`), defaulting to `tasks`, validated by `NotesConfig`'s rules verbatim and **refused, never corrected**: never empty (a ledger is a folder inside the profile, never its root), never absolute (`Path::join` with an absolute right-hand side discards the left one), never escaping, never overlapping another feature's subfolder. The folder file's overlay stays deep, and one unknown key still discards the whole layer — a typo that travels between clones must not mean something new.

## Block If

No ledger folder is configured: nothing is invented, runs stay in `task_runs`, and the surface says where a ledger would live.

## Never

Never `.keeper/`. `BUILTIN_EXCLUDES` hides everything under it except a direct-child `*.toml` (AD-100), so a per-task tree there would never sync — which is the whole point of the request — and epic 73 measured what a tracked file inside an excluded directory costs (DW-255, DW-256). Never compose a path by hand: `browse::plain_segments` and `WriteScope::route`, per AD-65.

</intent-contract>

## Code Map

- `keeper-sync/src/profile/mod.rs:256-372` — `NotesConfig` and its validator, the shape to copy; `SyncProfile`'s `notes`/`recordings`/`sessions` fields at `:967-991`.
- `keeper-sync/src/profile/folder.rs:605-608,699-703,1089-1096` — the deep overlay, the all-or-nothing discard, and the real table grammar.
- `keeper-core/src/config/keys.rs:509-518` — `notes.active_vault`, `MachineLocal`, default `""`; `tasks.ledger_vault` is its twin, machine-local because a profile id is minted per machine.
- `keeper-sync/src/browse.rs:746,807` + `files_write.rs:518` — `resolve`, `plain_segments`, `WriteScope::route`.
- `crates/keeper/src/sync_ipc.rs:1115-1171` — the folder-edit request's per-feature fields (shell crate: macOS CI).

## Tasks & Acceptance

Acceptance, verbatim from the epic: *a folder-file fixture carrying `[folder.tasks] subfolder = "70-tasks"` round-trips and keeps sibling tables intact; an absolute, empty, escaping or notes-overlapping subfolder is refused and **not stored**; the key appears in `docs/settings-keys.md` with its scope and default; with no ledger configured every existing test still passes and nothing writes outside the database.*

## Verification

- `TasksConfig { subfolder }`, `DEFAULT_TASKS_SUBFOLDER = "tasks"`, `SyncProfile::tasks`, `SyncProfile::tasks_root()` and the `[folder.tasks]` table with the deep overlay and the all-or-nothing layer discard. `None` means "holds no ledger" and `tasks_root()` returns `None` for it rather than inventing `local_path/tasks`, so no existing folder starts receiving files on upgrade.
- `TasksConfig::validate` takes the sibling blocks as arguments (the overlap rules are rules about pairs) and refuses empty, absolute, escaping, and overlap with a notes vault, a recordings root or a sessions zone — refused, never corrected.
- `cargo test -p keeper-sync` green, including the folder-file round trip with `[folder.tasks] subfolder = "70-tasks"` beside its sibling tables and the four refusals, each asserting the value was **not stored**.
- `keeper-sync` is `keeper-core`-free, and that bit: the grammar was first written in `keeper-core` and had to move, because `keeper-sync/Cargo.toml:10` forbids the edge and `keeper-syncd` must not link matrix-sdk. It now lives in `keeper-sync/src/ledger.rs`, and the three trigger words reach the frontend as strings the way `TaskVm.mode` already does.

**Owed:** the settings UI and the `tasks.ledger_vault` machine-local key are not in this change. The ledger is reachable today through the profile flag (the `[folder.tasks]` table in the folder file, which travels), so the mechanism is complete and testable; what is missing is the picker that makes it discoverable, and the folder-edit IPC field beside `notes`/`recordings`/`sessions` in the shell crate. DW-258.

## What the macOS job caught that no local gate could

The shell crate does not build on the Linux dev host, so `crates/keeper` is
verified only by CI — and it found four real defects in this epic's shell-side
edits, each on its own round trip:

1. `entry_vm` still matched `CopyOutcome::Skipped`, whose only producer was the
   date window. (The local sweep was a grep, and mine filtered comment lines in
   a way that hid this call site.)
2. Four lines of `///` on a function parameter, which `rustc` rejects outright.
   Nothing local parses that file, so nothing local could say so.
3. `SyncProfile` gained `tasks` and the shell's own guard demanded it be named
   in `EXPRESSED` or `PRESERVED` — the guard doing exactly its job. `PRESERVED`,
   truthfully: no form shows the flag, so no request may express it.
4. The same guard's second half: for every preserved field the fixture's `prior`
   must differ from a fresh profile, or the preservation assertion is vacuous.
   `tasks` was `None` in both, and the guard said so by name.

Recorded because it is the argument for those guards, and because the pattern
generalises: on this host a shell-crate change is verified by inspection, and
inspection misses a match arm, a doc comment and two halves of a guard.
