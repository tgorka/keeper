# Spec 46.10 — The recording destination in one place

story: 46.10
epic: 46 (the file is the setting)
status: implemented, frontend gates green on Linux; shell-crate claims are gate-checked
binds: AD-104 (left alone, see below), epic 46 "Not merging the recordings subfolder into
the path template", epic 46 "Not allowing recordings at the profile root"

## What the report said, and what was actually true

> "writing to a synced folder automatically drops into a `recordings/` subfolder; change it
> to the main folder and let me pick the whole subfolder path in the session folder setting."

Re-verified from source, and the scout was right on every point:

- **`recordings` is not hardcoded in any write path.** It is
  `default_recordings_subfolder()` → `DEFAULT_RECORDINGS_SUBFOLDER`
  (`keeper-sync/src/profile/mod.rs:185`, `:445`, `:750`), joined exactly once in
  `SyncProfile::recordings_root` (`:839-842`).
- **It was already user-editable**, in Settings → Sync → a folder → *Recordings subfolder*
  (`add-folder-form.tsx`, writing `SyncProfileReq.recordings_subfolder`).
- **The owner never found it** because the surface where recording is configured — the
  Recording pane's Destination card — receives only the fully-resolved absolute
  `destinationDir` and by design never joins a profile path to a subfolder.
- **A MULTI-SEGMENT head is accepted today.** `RecordingsConfig::validate`
  (`profile/mod.rs:489-547`) refuses exactly four things: empty, absolute (`is_absolute` **and**
  a leading `/` or `\`, because absoluteness is platform-shaped), any `..` component, and
  component-wise overlap with the notes vault. It has no segment-count rule. Existing tests
  already store `media/sessions` (`profile/mod.rs:1418`), `sessions/final` and
  `media/screen-recordings` (`sync_ipc.rs:2729`, `:2689`).
  **So this story makes no validation change**, and the card says the nesting is allowed
  rather than leaving it to be discovered by a refusal that never comes.
- **Writing at the profile root stays refused.** Untouched, for the reason the epic spine
  gives.

## The shape

One card shows and sets the whole path, as its two real parts.

| | head — *Recordings subfolder* | tail — *Session folder* |
|---|---|---|
| stored in | the sync profile row in `sync.db` | `recording.path_template`, the settings k/v table |
| lifetime | per FOLDER — travels to every machine syncing it | per MACHINE |
| written by | `sync_profile_save` (the folder form's command) | `recording_settings_set` |
| nested allowed | yes, and the card says so | yes |
| refusal source | `RecordingsConfig::validate`, verbatim | `TemplateError`, verbatim |

Not merged, per the spine: merging puts a fact that must be identical on both machines into
a key that cannot be, and the second machine records somewhere else.

## I/O matrix

| # | destination in force | what the card renders | on save |
|---|---|---|---|
| 1 | plain folder (`kind: folder`) | no head field, no travels note, no "this half is local" note — one part, and nothing about it travels | n/a |
| 2 | synced folder, row present | head field seeded with `profiles[i].subfolder`; travels note naming the profile; tail field + preview; "this half is this Mac's alone" | — |
| 3 | head typed ≠ stored | + the consequence sentence, naming the OLD head | `Save subfolder` live |
| 4 | head typed = stored (typed back) | consequence gone again | `Save subfolder` disabled |
| 5 | `Save subfolder` pressed, accepted | field disabled during the write; then re-seeded from Rust's stored value (which may be trimmed); resolved-root line and picker rows both re-read; consequence gone | `sync_profile_save` with the whole stored profile + one field |
| 6 | `Save subfolder` pressed, refused | `RecordingsConfig::validate`'s sentence in the head's OWN fault slot; typed text kept for correction | nothing stored |
| 7 | profile removed between the read and the write | `SYNC_PROFILE_GONE` in the head fault slot; **no** `sync_profile_save` call | nothing sent |
| 8 | picker switched to another synced folder | head re-seeded from THAT folder's subfolder; any unsaved text and any pending consequence dropped | — |
| 9 | synced kind, row absent (list unreadable) | no head field (there is no folder behind an edit box); the existing 41.2 degrade is unchanged | n/a |
| 10 | store not hydrated | head field disabled with the rest of the card | — |
| 11 | head box emptied | savable — Rust refuses it in its own words ("must not be empty"), which is more use than a greyed button that explains nothing | refusal printed |

## Edge cases

- **Empty is a value, not an omission.** The head is sent verbatim (whitespace-trimmed by
  Rust's `recordings_subfolder(req)` and otherwise untouched). Correcting `/tmp` to `tmp`
  here would make a save succeed against a folder nobody named — the same reasoning
  `sync_ipc.rs:800-818` already records for why the recordings subfolder is not
  slash-stripped where the notes one is.
- **The head is never sliced back out of `destinationDir`.** `Path::join` normalises nothing,
  so `20-media//sessions` and `20-media/sessions` resolve to one root and are two different
  stored values; only the stored one may be echoed back to a profile write. It is therefore
  carried on the row (`RecordingProfileVm.subfolder`), read in the same breath as the root it
  was joined from, so the pair cannot describe two different profiles.
- **A write that lost a race.** `saveHead` clears the pending-edit flag only when the field
  still holds the text it sent, so typing on during a write leaves the newer text standing —
  the template field's existing rule, applied a second time.
- **A refused write must not be retyped over.** The pending-edit flag stays set on refusal,
  so the two re-reads that follow a *successful* write cannot re-seed the box over the text
  the refusal is about.
- **A folder switch is not an edit.** The seeding effect resets the pending-edit flag when
  the profile id moves, so text typed for folder A is never offered as an edit of folder B.
- **The resolved root moves on a head write**, and the recording-settings mirror caches it.
  `refreshRecordingSettings()` exists for exactly that: the head is a *sync-profile* write,
  not a settings write, so nothing else would have invalidated the mirror. Both re-reads are
  mutation-proven separately (M5a, M5b).
- **A stale mirror cannot clobber.** `setSyncProfileRecordingsSubfolder` re-reads the profile
  from `sync_profiles` immediately before re-sending it, because `parse_req` assigns
  `name`/`localPath`/`remoteUrl`/`branch`/`direction`/`lane`/`subpaths`/`excludes`/
  `removable`/`lfsMode` unconditionally from the request. The window is one round trip, which
  is the window the folder form already has; no narrower one exists, since
  `sync_profile_save` takes no revision.
- **Unpinned knobs stay unpinned.** `settleMs`/`pollIntervalMs`/`authorOverride`/
  `notesSubfolder` are `null` on the VM exactly when the profile pins nothing, and `null` is
  the request's AD-34-9 "not expressed" — so passing the VM's own value through is the
  faithful thing. Mutation-proven (M9), because freezing DW-116's cadence at 15 s is the
  precise bug `parse_req`'s doc comment says bit twice.

## The consequence sentence

The most important thing in the story. Changing the head is a pure configuration write:
nothing under the old head is copied, moved or rewritten. The bytes are safe and everything
that *points* at them is not — the recordings archive is rebuilt by walking the recordings
root, so sessions under the old head leave the browser at the next rebuild, and a session's
note stub embeds `![[<head>/…]]`, which stops resolving.

So the card says it **before** the write: on screen from the first keystroke that makes the
box differ, and gone again the moment the write lands or the box is typed back. It
interpolates the OLD head, because "sessions already there" is only actionable if you are
told where there is. Rendered un-muted (`text-foreground`) rather than
`text-destructive`: nothing here is refused or wrong. No confirm dialog — this card states
consequences rather than gating them (41.2's synced note, 41.7's drive note), and the repo
has no warning colour token to invent one with.

## Mutation table

`bun run test src/components/recording/recording-destination-controls.test.tsx`, one
mutation at a time, each carrying the greppable sentinel `MUT46-10`, each restored and the
restore verified by sha256 against a pristine manifest. **12/12 caught.**

| # | mutation | test that failed |
|---|---|---|
| M1 | head travels note deleted | shows the head and the tail as two settings… |
| M2 | tail "this half is local" note deleted | shows the head and the tail as two settings… |
| M3 | consequence gated on `headSaving` (i.e. shown *after* the write) | says what a head change costs BEFORE it is written |
| M4 | head write made a no-op | writes the head through the profile writer… (+2) |
| M5 | both post-write re-reads removed | writes the head through the profile writer… |
| M5a | only `refreshRecordingSettings()` removed (root line stale) | writes the head through the profile writer… |
| M5b | only `loadProfiles()` removed (head box stale) | writes the head through the profile writer… |
| M6 | refusal swallowed instead of printed | prints the validator's refusal beside the head |
| M7 | folder-switch branch of the seeding effect disabled | seeds the head from the folder in force… |
| M8 | head offered for a plain folder (`profiles[0]` fallback) | offers no head at all for a plain folder |
| M9 | `pollIntervalMs` frozen at `SYNC_DEFAULT_POLL_INTERVAL_MS` | writes the head through the profile writer… |
| M10 | vanished-profile guard removed | says the folder is gone rather than writing… |

Post-sweep: `grep -rn MUT46-10 src src-tauri/crates` → no matches; all four touched files
sha256-identical to the pristine manifest.

## Files

| file | change |
|---|---|
| `keeper-core/src/vm.rs` | `RecordingProfileVm` gains `subfolder: String` |
| `src/lib/ipc/gen/RecordingProfileVm.ts` | regenerated by ts-rs (`cargo test -p keeper-core --lib export_bindings_recordingprofilevm`) |
| `keeper/src/ipc.rs` | `DestinationProfileRow` gains `recordings_subfolder: Option<String>`; `destination_profile_row` fills it from the same `recordings` block as the root; `destination_profile_vms` carries it with the same `?`; `flagged_row` fixture + `destination_profiles_lists_only_the_folders_that_hold_recordings` extended with a nested-head row |
| `src/lib/stores/sync.ts` | new `setSyncProfileRecordingsSubfolder`, `SYNC_PROFILE_GONE`, and `SYNC_RECORDINGS_SUBFOLDER_LABEL` (moved here from `add-folder-form.tsx`) |
| `src/lib/stores/recording-settings.ts` | new `refreshRecordingSettings` |
| `recording-destination-controls.tsx` | head field, travels note, consequence, own fault slot, tail's local note; profile read made re-runnable |
| `add-folder-form.tsx` / `.test.tsx` | import the label from `sync.ts` instead of declaring it (clean move, no re-export) |
| `docs/recording.md` | new "The whole path, in one card" subsection: the head/tail table, "changing the subfolder moves no files", and why the profile root stays refused |

## Deliberately NOT done

- **No validation change.** A multi-segment head is already legal, so the escaping /
  absolute / vault-overlap tests the story authorised are not needed and would have been
  duplicates of `profile/mod.rs:1426` and `sync_ipc.rs:2750`.
- **No merged single path field.** Forbidden by the spine, with the reason.
- **No recordings at the profile root.** Untouched.
- **No live preview under the head field.** Composing `<localPath>/<typed head>` would need
  `local_path` on `RecordingProfileVm`, and that VM's whole contract is that no surface joins
  a local path to a subfolder. The consequence sentence occupies that slot with something
  worth more.
- **No shared control with the folder form.** The two do different jobs — the form composes a
  whole profile from scratch, the card edits one field on a stored one and carries a
  consequence the add flow has nothing to warn about. Only the LABEL is shared, moved to
  `sync.ts` so neither component imports the other for a string.
- **The card's LAYOUT is left alone (AD-104).** The head row reproduces the template row's
  existing shape — fixed short label, fixed-width field, `shrink-0` button — and adds no
  variable-width or asynchronously-resolving element, which is the actual AD-104 failure
  mode. AD-104's extraction (`PaneHeader`, W3Files/46.13) is about pane headers
  (identity/status/actions), not settings-form rows, and does not apply here. The
  over-constrained-row observation in the scout note is noted and **not** acted on in this
  story: nothing in it changes the row's behaviour, and restyling a working card is not a
  change this story can prove.
- **No new dependency.** No `localStorage`.

## What I could not verify here, and why

`keeper` (the Tauri shell) does not build on Linux — no GTK/webkit — so `cargo build/check/
clippy/test -p keeper` was never run. That covers **every claim about `keeper/src/ipc.rs`**:
the new `DestinationProfileRow.recordings_subfolder` field, its two fill sites, the
`flagged_row` fixture line, and the extended
`destination_profiles_lists_only_the_folders_that_hold_recordings` test.

Mitigations actually applied:
- `RecordingProfileVm` itself lives in **keeper-core**, which does build here, and its
  ts-rs export test was run: `cargo test -p keeper-core --lib
  export_bindings_recordingprofilevm` → `1 passed`, and the regenerated
  `RecordingProfileVm.ts` matched byte-for-byte.
- **No new tauri command**, so `src/test/command-registration.test.ts` needs nothing: the
  head's write reuses `sync_profile_save` and its read reuses `sync_profiles`, both already
  registered and already invoked from TypeScript.
- Only `recordings_root` is ever the gate in the six existing `unflagged.recordings_root =
  None` test sites, and `destination_profile_vms` checks it first, so those fixtures'
  now-stale `recordings_subfolder` cannot change any verdict. Recorded as **DW-196** — the
  two-`Option` pair should be one field; collapsing it is thirteen edit sites in a file no
  local gate can compile, so it belongs in the next story already running the macOS gate.

## Gate checks, in order

1. `cargo fmt --manifest-path src-tauri/Cargo.toml --all --check`
2. `cargo clippy --manifest-path src-tauri/Cargo.toml --workspace --all-targets -- -D warnings`
   — **on macOS.** Expect zero warnings from `ipc.rs`; the new field is read in
   `destination_profile_vms`, so no `dead_code`.
3. `cargo nextest run --manifest-path src-tauri/Cargo.toml -p keeper -E
   'test(destination_profiles_lists_only_the_folders_that_hold_recordings)'` — **on macOS.**
   Asserts the head reaches the picker row and that `40-media/recordings` is carried whole.
4. `cargo nextest run --manifest-path src-tauri/Cargo.toml -p keeper` — **on macOS**, for the
   other twelve `flagged_row` consumers.
5. `bun run bindings:check` — **on macOS** (it runs the whole Rust suite);
   `RecordingProfileVm.ts` must come back unchanged.
6. `bun run test src/components/recording/ src/components/sync/add-folder-form.test.tsx`
   — **run here, EXIT=0, 170/170, three consecutive runs.**
7. `bun run test src/components/layout/recording-pane.test.tsx src/components/settings/`
   — **run here, EXIT=0, 202/202** (the two other surfaces that mount this card).
8. `bunx tsc --noEmit` — **run here**, no diagnostics in any touched file.
9. Manual, on macOS: flag a synced folder, open Recording → Destination, change the head to
   `40-media/recordings`, confirm the consequence sentence appears before the button, save,
   confirm the resolved root line and the session-folder preview both move, and confirm the
   same value now shows in Settings → Sync → that folder.
