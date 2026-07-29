---
title: 'Open the synced folder'
type: 'feature'
created: '2026-07-29'
status: 'review'
baseline_revision: '88452c1'
---

<intent-contract>

## Intent

**Problem:** `sync_open_path` was specified in epic 32 as part of story 32.4 (the IPC surface) and
never shipped — epic 34 records it as a real gap rather than a 34 item
(`epic-34-...md:347`). Every other verb from that story exists; this one is absent from the invoke
handler (`keeper/src/lib.rs`), from `sync_ipc.rs` and from `src/lib/ipc/client.ts`. The consequence is
visible on both sync surfaces: the folder's path is rendered as inert text (`sync-pane.tsx:709`,
`sync-section.tsx:344`) with `title` set so it can at least be *read* when truncated, and there is no
way to get to the folder from the app at all. The nearest thing keeper has is the recording folder's
reveal, which is bound to the recording destination and takes no argument.

**Approach:** Add one desktop command that takes a **profile id** and reveals that profile's
`local_path`, resolved in Rust from the stored profile, through the same
`tauri_plugin_opener::reveal_item_in_dir` seam the recordings folder, the export reveal and the tray
already use. Check the folder is a directory before revealing, and report the two ways it can be
absent in words that name a next step. In the UI, turn the path line itself into the control — one
shared `SyncFolderPath` component rendered by both surfaces, gated on
`capabilities.revealInFileManager`.

## Boundaries & Constraints

**Always:** The command's only argument is the id. The path is read from the profile the engine
stored, so the reachable set is exactly "folders keeper already syncs" and cannot be widened by the
webview. Errors carry a sentence composed in Rust, because `syncErrorMessage` renders `message`
verbatim and both surfaces show it. The affordance is a real `<button>`, so it is keyboard-reachable
without adding a key handler, and its accessible name carries the verb plus the folder.

**Block If:** (none)

**Never:** Do not add a second path-opening mechanism — `reveal_path(path)` (Story 5.5) already
exists for a path the frontend legitimately holds, and sync must not extend that reach. Do not touch
`parse_req`, `SyncProfileReq` or the credential commands in `sync_ipc.rs`; do not touch
`SyncActivityList` or the Sync view's header actions; do not route the open through the cards'
`run()` action lifecycle (it would clear the last sync report and take the busy lock for something
that changes nothing).

## I/O & Edge-Case Matrix

| Scenario | Input / State | Expected Output / Behavior | Error Handling |
|----------|--------------|---------------------------|----------------|
| Ordinary open | id of a profile whose folder exists | that folder revealed in Finder | — |
| Unknown id | id nothing stored | `internal`, message names the id | never resolves to a default or the first profile |
| Frontend asks for a path | — | impossible: the command has no path parameter | — |
| Fixed folder deleted / moved | `local_path` is not a directory | `internal`, "… is not there. It was moved, renamed or deleted outside keeper — use Edit folder to point keeper at it." | no reveal attempted |
| Removable volume unplugged | `removable` profile, mount point gone | `internal`, "… is not there. This folder lives on removable media — reattach the volume, then open it again." | no reveal attempted |
| Path exists but is a file | hand-edited profile | treated as absent (the `is_dir` check) | same sentence |
| File manager refuses | plugin returns `Err` | `internal`, "could not open `<path>` in the file manager: …" | never a panic |
| No engine (no usable git) | `sync::engine` fails | the engine's own `SyncError` through `sync_ipc_error` | — |
| Platform with no file manager | `revealInFileManager` false | path renders as plain text, no control | nothing to activate, nothing to fail |
| Keyboard user | Tab to the path | focus ring, Enter/Space opens | native button semantics |

</intent-contract>

## Code Map

- `src-tauri/crates/keeper/src/sync_ipc.rs` -- `find_profile` (pure resolution, split out of
  `profile_by_id`, which now delegates to it), `open_failure`, `unavailable_sentence`, the
  `sync_open_path` command, and three tests.
- `src-tauri/crates/keeper/src/lib.rs` -- one line in the desktop `generate_handler!` list.
- `src/lib/ipc/client.ts` -- `syncOpenPath(id)`.
- `src/components/settings/sync-section.tsx` -- `SYNC_OPEN_PATH_LABEL`, the shared `SyncFolderPath`
  component, and the Settings row's path line.
- `src/components/layout/sync-pane.tsx` -- the folder card's path line.
- `src/components/settings/sync-section.test.tsx`, `src/components/layout/sync-pane.test.tsx` -- the
  `syncOpenPath` mock, the `revealInFileManager` snapshot, and five tests.

## Tasks & Acceptance

**Execution:**
- [x] `sync_ipc.rs` -- `find_profile(profiles, id)`: pure over the list, `SyncError::Config` naming
  the id when absent. `profile_by_id` now calls it, so both paths refuse an unknown id with one
  wording. -- Resolution is testable without an engine.
- [x] `sync_ipc.rs` -- `unavailable_sentence(profile)`: the removable and the fixed case, each naming
  the folder and its own next step. `open_failure(message)`: the `internal`, non-retriable envelope
  for a message written for a reader rather than derived from a `SyncError`. -- An absent folder is
  actionable.
- [x] `sync_ipc.rs` -- `sync_open_path(state, id)`: `engine_of` → `list_profiles` → `find_profile` →
  `is_dir` guard → `reveal_item_in_dir`. No path parameter anywhere. -- The webview cannot name a
  folder.
- [x] `lib.rs` -- registered in the `#[cfg(desktop)]` command list beside `sync_verify`. The whole
  `sync_ipc` module is already desktop-only, so no mobile stub exists to write.
- [x] `client.ts` -- `syncOpenPath(id)`, documented against `revealPath` so the distinction between
  the two is written down at the call site.
- [x] `sync-section.tsx` -- `SYNC_OPEN_PATH_LABEL = "Reveal in Finder"` and `SyncFolderPath`
  ({ profile, className, onError }): a button when `revealInFileManager`, a `<span>` when not,
  `aria-label` = verb + path, `title` = the bare path (its existing job), `onError` feeding the
  caller's action-error line.
- [x] `sync-section.tsx` + `sync-pane.tsx` -- both path lines render `SyncFolderPath`, each passing
  its own text classes (plain in Settings, mono in the card).
- [x] Tests -- Rust: the path comes from the stored profile; an unknown id (and an empty store) is
  refused with the id named; the two absent-folder sentences name the folder, differ from each other,
  and stay non-retriable. TS: clicking the control calls the binding with `"p1"` and triggers no sync;
  a rejection renders verbatim; the capability off leaves readable text and no button.

**Acceptance Criteria:**
- Given a profile whose folder exists, when the path control is activated, then that folder is
  revealed in the OS file manager (Finder on macOS).
- Given any frontend caller, when it invokes `sync_open_path`, then it can pass only an id — no path
  parameter exists, so no path outside the stored profiles is reachable.
- Given an id nothing stored, when the command runs, then it rejects with a message naming the id and
  reveals nothing.
- Given a removable profile whose volume is detached, when the control is activated, then the
  rejection says to reattach the volume; given a fixed folder that is gone, it says the folder moved
  and points at Edit folder.
- Given `revealInFileManager` is false, when either surface renders, then the path is still legible
  and there is no control to activate.

## Design Notes

**Which existing mechanism was reused, and why.** There were two candidates. `recording_reveal_folder`
is the *shape* this command copies — no path from the caller, the target resolved in Rust from stored
state — but it does not generalise: it takes no argument and resolves
`effective_destination_dir(&data_dir)`, the one recordings destination, and its
`nearest_existing_ancestor` fallback is deliberate for a folder that may not exist yet (Story 20.4).
A synced folder is the opposite case: it either exists or its absence is the news, and walking up to
`/Volumes` — or to `/` — would answer a click with somebody else's folder. `reveal_path(path)` is the
*primitive*, and it is the wrong door for sync: it accepts a path from the webview, which is exactly
the reach this command must not have. So the reuse is at the layer that matters — both go through the
one `tauri_plugin_opener::reveal_item_in_dir` seam already shared by `reveal_path`, the recordings
reveal and the tray, so there is still exactly one way keeper shows a folder. No capability grant was
needed: Tauri capabilities gate the webview's calls into a plugin, not a Rust-side call, which is why
the tray's reveal works today with the current `desktop.json`.

**Why the id and not the path.** A `sync_open_path(path)` would be `reveal_path` with a longer name,
and it would hand the webview an arbitrary-path opener under a sync-shaped label. Taking the id keeps
the reachable set equal to "profiles the engine stored": to open `/etc` a caller would first have to
persuade `sync_profile_save` to store a profile pointing there, which is a different, already-audited
decision with its own UI. The comment on the command says this, because the property is invisible
from the signature alone — it lives in the *absence* of a parameter.

**Why the `is_dir` check rather than letting the plugin fail.** `reveal_item_in_dir` fails the same
way whether the volume is out, the folder was deleted, or the path names a file, and on some
platforms it succeeds at showing an empty window. The interesting distinction — a removable volume
that will come back versus a folder that has moved — is knowable only from the profile, and it is the
whole difference between "plug the stick in" and "tell keeper where it went". Removable absence is a
first-class state in this codebase (AD-48: absence is a pause, never a deletion), so reporting it as a
silent no-op would contradict the engine.

**Why the message is composed in Rust and not funnelled through `sync_ipc_error`.** That funnel takes
its wording from the `SyncError`'s own `Display`, and no variant fits: `MediaAbsent` cannot name the
folder, `Io` reads as "open failed for X: entity not found", and `Config` prefixes *invalid sync
configuration*, which would blame the settings for a stick that is merely unplugged. Both surfaces
render `IpcError.message` verbatim through `syncErrorMessage`, and `sync_ipc.rs` already composes
user-facing sentences in Rust for exactly this reason (`SyncStatusVm.line`, `outcome_line`), so the
sentence is written once, in Rust, where the two cases are distinguishable.

**Why the path itself is the affordance.** The path was already the thing on screen that means "the
folder", it already carried a `title` so it could be read when truncated, and both cards' header rows
are full of buttons that act on sync rather than on the folder. A seventh button would crowd them; the
path becoming clickable adds nothing to the layout. It is a real `<button>`, so it is in the tab order
with the standard focus ring and needs no key handling of its own, and because the visible text is a
path the accessible name adds the verb: `Reveal in Finder: /Users/alice/Documents/tgdrive`. The
wording is the app's existing reveal verb, used verbatim by the export dialog and the recording
completion card.

**Why the control is capability-gated.** `revealInFileManager` (Story 5.5) exists to hide exactly this
kind of affordance where there is no user-visible file manager, and the recording completion card is
already gated on it. With the capability off the path renders as the plain text it was before, so
nothing is lost — the gate removes a control that would only reject.

**Why it is shared and why it bypasses `run()`.** Both surfaces render the same profile fields, and
the removal confirmation is already shared between them for the stated reason that two copies of one
affordance drift; one `SyncFolderPath` keeps the accessible name, the gate and the error path
identical on both. It is deliberately not routed through either card's `run()` helper: that helper
clears the last `Sync now` report and takes the busy lock, both correct for an action that changes the
folder and both wrong for one that only looks at it. A refusal still lands in the same
`actionError` line, which is what `onError` is for.

## Verification

**Deliberately not run by this agent:** `cargo build` / `cargo test` / `cargo clippy` for the `keeper`
crate. The batch constraint is explicit that they do not work on this Linux box (tauri needs GTK), so
nothing below claims a green Rust test run.

**What was actually run:**
- `bun run typecheck` -- clean.
- `bunx biome check` on the five touched TS/TSX files -- clean (formatting, lint and import order),
  and repo-wide `bun run lint` -- clean over all 377 files. (An earlier run of this session caught a
  transient `// MUTANT` / `if (true)` in `src/components/sync/add-folder-form.tsx` from another
  agent's in-flight mutation check; that file was restored byte-for-byte and re-checked here.)
- `bun run test` -- the whole frontend suite: **145 files, 1662 tests, all passing**, including the
  five new ones. Each new test was also confirmed to actually execute (`-t path --reporter=verbose`),
  so none is a silently-skipped block.
- `rustfmt --edition 2021 --check` against a **copy** of `sync_ipc.rs` in `/tmp` (a read-only opinion,
  nothing in the tree reformatted): it wanted two layout changes, both applied, and the file is now
  byte-identical to rustfmt's output. This is why the `assert!`s in the new tests are split the way
  they are — `fn_call_width` is 60, and two of them cross it.
- The type-level constructs I could not compile in place were compiled standalone with `rustc
  --edition 2021` in `/tmp`: the `find_profile<'a>` elision, `Result<&T, E>::cloned()` (stable since
  1.59, used by the refactored `profile_by_id`), and `format!("{path} …")` capturing a
  `std::path::Display`. It ran and printed both sentences.

**What was checked by reading:**
- `ipc.rs:2164-2196` (`reveal_path`), `:2649-2697` (`nearest_existing_ancestor`,
  `recording_reveal_folder`) and `tray.rs:213` — the three existing `reveal_item_in_dir` call sites,
  which is what establishes that a Rust-side reveal needs no capability entry and that
  `recording_reveal_folder` cannot be parameterised.
- `keeper-sync/src/profile.rs:142-175` — `local_path: PathBuf`, `removable`, and the `volume_id`
  comment naming absence as a pause (AD-48).
- `keeper-core/src/vm.rs:133-178` and `sync_ipc.rs:427-449` — `IpcErrorCode` has no better variant
  than `Internal` here, and `sync_ipc_error` maps `Retriability::Deferred` to `retriable: false`, so
  the hand-built envelope matches what the funnel would have produced for media absence.
- `lib.rs:22-23` and `:496-522` — `mod sync_ipc` and the whole sync command list are already
  `#[cfg(desktop)]`, so this command needs no `#[cfg(not(desktop))]` stub (unlike the `ipc.rs` reveal
  commands, which live in a module compiled for iOS).
- No generated binding changed: the command adds no `#[derive(TS)]` type, so `src/lib/ipc/gen/` is
  untouched and `bindings:check` has nothing to compare.

**Commands for the parent to run:**
- `bun run test:rust` -- expected: the three new `sync_ipc` tests pass
  (`opening_a_folder_takes_the_path_from_the_stored_profile`,
  `opening_an_unknown_profile_is_refused_and_names_the_id`,
  `an_absent_folder_reports_something_a_person_can_act_on`).
- `bun run check:rust` -- expected: clean; the file is already rustfmt-clean and the new code has no
  `unwrap()`.
- `bun run check` -- expected: green (biome, tsc and vitest all verified green here).
- Manual, on macOS: click a folder's path in the Sync view and in Settings → Sync; the folder opens in
  Finder. Unplug a removable profile's volume and click again; the card shows the reattach sentence.

## Notes

Not addressed here, and pre-existing: the export dialog's own "Reveal in Finder" (toast action and
inline button) is still un-gated on `revealInFileManager` — the open ledger entry at
`deferred-work.md:842`. This story gates its own control and adds nothing to that debt.
