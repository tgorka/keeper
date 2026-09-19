---
status: review
baseline_revision: a925c4b
final_revision: ''
---

# Story 76.4 — Service files hidden by default

<intent-contract>

## Problem

Routine service files crowd the notes list; their names and the person's last visibility choice have no persistent home.

## Approach

Declare the keys in the existing registry, normalize the configurable name list, and expose a pure basename predicate. The shell alone applies it to list results; the UI mirrors the dedicated preference.

## Always

Compare the final path segment case-insensitively in every folder. Missing or corrupt name settings use the four documented defaults. An intentionally empty list hides nothing. Persist the hide choice independently of vault settings.

## Block If

A configured name contains a path separator: ignore it rather than reinterpret a path as a basename.

## Never

Never hide a file from links, the Files tree, sync or search-everywhere. Never silently turn a corrupt name setting into an empty list.

## I/O and edge-case matrix

| Input | Output / test |
| --- | --- |
| `agents.md`, `docs/Log.MD` with defaults | Both match; `matches_basename_in_any_folder_without_case` |
| Configured `docs/log.md` or `docs\\log.md` | Ignored; `ignores_paths_and_blank_names` |
| Empty and whitespace names | Dropped on write/read; registry round trip |
| Absent names row | Four defaults; registry degradation test |
| Corrupt JSON / non-string array | Four defaults; registry degradation test |
| Set mixed-case duplicated names, then get | Trimmed, lowercase, stable deduplication |
| Explicit empty names list | Empty list retained |
| Absent hide flag / stored `0` | true / false; persistence round trip |

</intent-contract>

## Code Map

- `src-tauri/crates/keeper-core/src/config/keys.rs:509` — notes key declarations.
- `src-tauri/crates/keeper-core/src/registry.rs:1308` — active-vault settings seam; typed settings added beside it.
- `src-tauri/crates/keeper-core/src/registry.rs:680` — corrupt JSON degradation precedent.
- `src-tauri/crates/keeper-core/src/notes/vm.rs:110,203,1106` — row, list and query wire types.
- `src-tauri/crates/keeper-core/src/notes/service_files.rs:1` — pure basename predicate and defaults.

## Tasks & Acceptance

Acceptance, verbatim from the epic: a vault with `agents.md` at the root and `docs/Log.MD` in a folder lists neither with the toggle on and both with it off, and the count line reads `N notes · 2 hidden` only while it is on (mutation-proved: the filter test fails when the comparison becomes case-sensitive or path-anchored); a wikilink to `agents.md` opens it while hidden; ⌘⇧F finds a word in it while hidden; the Files tree still lists it; the toggle survives a relaunch in the state it was left (a registry test round-trips the key; the hydrate is exercised where it mounts, as `notes-rail-fold` is); Esc in the field does not flip it and *Save as space* does not offer for it alone; the names list edited in Settings takes effect on the next `notes_list` without a restart; `docs/settings-keys.md` shows both keys with the generated text; the command pairs and `matches_filter` change are **by inspection, awaiting CI macOS**.

## Design Notes

### Rust settings and predicate

`notes.service_file_names` is UserGlobal / AnyLayer / Json. Its layer spelling follows the existing JSON-in-TOML-string convention. `notes.hide_service_files` is SessionState / Never / Flag01, default `1`. Registry normalization is also applied to layer reads. Neither getter caches values. New search VMs carry UTF-16 ranges without attempting conversion in the renderer.

- Hydration is a memoised app-wide promise, validates the flag as boolean (default true otherwise), and gates reads without clearing an existing list/window on pane remount.
- Settings adopts Rust's normalized service-name readback; dev mock fixtures round-trip visibility, service names and embedding selection and supply hidden counts.
- Core normalizes service names once in the registry; basename classification avoids per-name allocation.
- Shell wiring (by inspection): service filenames load only when hiding is enabled, and subscriptions read the persisted visibility flag by default.

### Folder-scope exception (DW-266, explicitly approved by Main)

The physical folder scope still calls `notesTree`, not `notes_list`. It shows
service files and reports `hidden: 0`, preserving the Files tree's unfiltered
contract. Main explicitly waived this scope for Epic 76 in the Bar lane's hub
thread after the gap was raised. The clean fix is optional `folderPath` on
`NoteQueryReq` plus a folder scope filter in `notes_list`, not a frontend copy
of basename matching. All other scopes use the persisted eye preference.

## Verification

Isolated `rustup run stable-x86_64-unknown-linux-gnu rustc --test --edition=2021 src-tauri/crates/keeper-core/src/notes/service_files.rs` followed by its test binary: **2 passed**. `matches_basename_in_any_folder_without_case` was mutation-proved twice: case-sensitive equality and full-path comparison each failed at `assertion failed: is_service_file("docs/Log.MD", &names)`. Both mutations were restored; the same two tests passed again. Coordinator gate still owes `config::keys`, `registry`, `notes::vm`, generated bindings and shell/UI integration.

The one permitted documentation-generation attempt ran with
`RUSTUP_TOOLCHAIN=stable-x86_64-unknown-linux-gnu cargo test --manifest-path src-tauri/Cargo.toml -p keeper-core --lib config::keys::tests::docs::regenerate -- --ignored`.
It failed after 115 seconds on four `E0597` statement-lifetime errors in the
concurrently authored `notes/search_index.rs` (101, 124, 148, 153), before tests
ran. No diagnostics named this slice. The generator command is handed to Main
for the coordinated gate; `docs/settings-keys.md` was not hand-edited.

### UI implementation (Bar lane)

The eye is a persistent viewing preference, not a filter chip. `notes-filters.ts`
sends `hideServiceFiles` in each list request while `clearAll`, `dropLastChip`,
`isFiltered`, scope-only and space predicates continue to ignore it. Each Notes
pane hydrates the dedicated setting; an intervening user change invalidates the
read. Explicit toggle writes are serialized and failures restore acknowledged
state with a visible error. The count mirror retains Rust's `hidden`, including
the previous count on a failed refresh. Streamed whole-vault ops invalidate the
list while hiding is enabled rather than injecting service rows.

UI matrix: eye-only does not offer Save; Escape never changes the eye; relaunch
hydrates false; a late hydrate cannot undo a click; rejected persistence reports
failure; `1 note · 1 hidden`, `0 notes · 2 hidden`, capped count grammar, and no
zero-hidden suffix; all-hidden results offer Show service files without clearing
the query. These are exercised in the bar, filters, list lifecycle and phone
suites. Real geometry is owned by spec-76-5's browser probe, not jsdom.

### Settings implementation
`src/components/notes/search-settings.tsx:1` adds a global Notes search section after Capture settings (`settings-dialog.tsx:234`), independent of active-vault existence. Service files round-trip as comma-separated names, trimmed with empty entries dropped on blur/Enter. Backend owns lowercase/deduplication. Empty input means hide nothing. A failed load/save is a visible refusal, not a successful-looking reset. Matrix: loaded names → joined text; padded/empty segments → cleaned list; Enter/blur → persisted value; rejection → sentence.

Settings proof: `search-settings.test.tsx` passes the comma-separated load, trim/drop-empty, blur save and Enter-to-empty round-trip scenarios in the five-file 61-test scoped run. Real-surface probe `dev/probe/notes-search-results.html` drives the same Input/Enter path; coordinator runs `PROBE_ENTRY=notes-search-results.html bash dev/probe/measure.sh results '' 240 320 420`. No visual measurement is claimed by the unit run.

### Shell wiring (by inspection)

By inspection, awaits CI macOS; none of these shell changes was compiled here.
`src-tauri/crates/keeper/src/notes_ipc.rs:1197` (`notes_service_file_names_get`), `:1203` (`notes_service_file_names_set`), `:1209` (`notes_hide_service_files_get`), `:1215` (`notes_hide_service_files_set`) use the dedicated registry functions; setter wire keys are `names` and `hidden`. `src-tauri/crates/keeper/src/lib.rs:1298-1301` registers the four commands.

`project_list` (`notes_ipc.rs:490`) reads names once, applies the basename predicate after scope/text/other chips, counts removed notes before the space cap or page, and emits `NoteListVm.hidden`. `stream_changes` (`:5087`) also emits `NoteChangeBatch.hidden`. Main explicitly approved this additional channel field and applying the service predicate after the other filters; CoreSettingsEmbed owns the VM and Bar owns its consumers. Direct opening, links, Files, and search-everywhere are unchanged.

Matrix awaiting macOS execution: root `agents.md` and nested `docs/Log.MD` → withheld/count 2; hiding off → both shown/count 0; text/scope excludes one → count only the remaining match; cap/page → hidden count unchanged; names saved → next list uses them. Core service-name/registry tests defend case-folding and persistence; no shell mutation or runtime result is claimed. Gate: **Rust (fmt, clippy, test)** (`.github/workflows/ci.yml:28-52`, `macos-latest`).

UI proof (Bar): the seven-file scoped run passed 113 tests, including mounted
hydrate false, visible save refusal with rollback, count grammar, all-hidden
recovery, stale streaming refresh rejection, and failed-read hidden-count
retention. The eye/savable mutation was caught at the assertion that Save as
space must be absent for eye-only state (expected null, received button).
Mutation restored; final scoped rerun is recorded in spec-76-5.
