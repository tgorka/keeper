---
title: 'The machine name, the commit subject, and the knobs that were hiding'
type: 'feature'
created: '2026-07-28'
status: 'review'
baseline_revision: '5c40a22'
---

<intent-contract>

## Intent

**Problem:** Four gaps that share `SyncProfileReq` and one form.

(a) `db::set_device_label` (`keeper-sync/src/db.rs:193` pre-edit) has existed since Story 23.4 with
no caller outside its own test. The label is minted once from `hostname` at first `Engine::open`
(`engine.rs:233`, `keeper/src/sync.rs:137-150`) and rides every commit as `Keeper-Device`
(`provenance.rs:67`), so the name a machine happened to have on the day sync was first opened is the
name every commit it will ever make carries.

(b) `change_subject` (`provenance.rs:243-258`) is fixed: `sync(<profile>): 3 added, 1 modified,
1 deleted`, with no way to say anything else.

(c) `pollIntervalMs` governs sync latency — the engine paces its tree walk by it (`engine.rs`
`scan_is_due`, DW-116) — and is reachable only from `keeper-syncd`'s CLI
(`keeper-syncd/src/commands.rs:322-323`). The form's own header comment claimed it "was deleted in
Rust rather than exposed here", which stopped being true on 2026-07-28.

(d) `parse_req` (`sync_ipc.rs:378-451` pre-edit) built from `SyncProfile::new` and then re-added
three survivors by name (`enabled`, `author_override`, `volume_id`). `db::upsert_profile` replaces
the whole JSON row, so every field added to `SyncProfile` after that list was written was silently
reset by every save from the app. `poll_interval_ms` was exactly that field, for two months.

(e) `settleSeconds` rendered `String(profile.settleMs / 1000)` — the STORED number, not the one in
force. `effective_settle_ms` (`profile.rs:232-238`) substitutes `REMOVABLE_SETTLE_MS` for a
removable profile that pins nothing, and the measured app showed `5` for a removable folder whose
real wait was 10 s. In add mode the box was blank while 5 000 was applied.

**Approach:**

* **Device label.** `Engine.device` becomes `Mutex<DeviceIdentity>` with a snapshot accessor
  `Engine::device()` and `Engine::set_device_label`, which writes the row first and only then the
  in-memory copy every commit reads. `db::set_device_label` gains validation and returns the label as
  stored. Two commands, `sync_device` / `sync_device_set_label`, and a "This device" block in
  Settings → Sync. The **id never moves** and is not a parameter anywhere.
* **Commit subject.** `SyncProfile.commit_subject_template: String`, empty meaning the mechanical
  default. One tiny scanner (`provenance::pieces`) drives both the renderer and the validator, so a
  template cannot pass `validate` and then render as something else. `stage_and_commit` takes
  `profile: &SyncProfile` in the slot that held `profile_name: &str`.
* **Poll interval.** A form field, plus `SyncProfile::effective_poll_interval_ms()` — the floor moves
  out of `scan_is_due`'s local `const MIN_SCAN_INTERVAL_MS` so the form and 34.9's degraded-watcher
  warning can name the cadence in force without re-deriving it.
* **`parse_req` inverted.** `prior.clone()` is the base; `SyncProfile::new` is reached only when
  there is no prior. Preservation becomes structural rather than a list someone has to remember.
* **Real defaults.** `SyncProfileVm.settleMs` / `pollIntervalMs` become `Option<u64>` — `None` means
  "this profile pins nothing", which is the encoding `effective_settle_ms` reads — beside
  `effectiveSettleMs` / `effectivePollIntervalMs`, the numbers actually in force. An empty box means
  "keeper picks" and its placeholder names what keeper will pick; a typed number Rust will not honour
  verbatim gets a note under the field saying what it will use.

## Boundaries & Constraints

**Always:** The device id is immutable — it is what `git::commit::author_for` derives the
non-routable `sync@<id>.keeper.invalid` address from and what distinguishes two machines a user has
called the same thing in one history. A rename affects later commits only; nothing rewrites history.
An empty label is refused at the store, the way `upsert_profile` refuses an invalid profile. A commit
subject is exactly one line and never blank. `parse_req`'s `Option` slots all mean "not expressed →
keep what is stored"; none of them is a reset instruction. Rust stays the authority on effective
values; the four constants mirrored into `sync.ts` produce placeholders and notes only. Rust
workspace lints (no `unwrap` in new code); TS 2-space / 100-col / double quotes / no `any`.

**Block If:** (none — every port, helper and mapper this needed already existed)

**Never:** Do not make the trailer block templatable (`provenance::commit_message`) — the epic is
explicit and a clone must be able to trust its shape. Do not let an unknown placeholder reach a
commit silently (refuse it on save; leave it verbatim if it somehow arrives). Do not send `null` from
the form for a knob the form displays: `null` is the omission Rust reads as "leave whatever is
stored", the opposite instruction. Do not add a second encoding of "let keeper choose" — it is
`stored == default`, in one place, and the wire says so with `Option`.

## I/O & Edge-Case Matrix

| Scenario | Input / State | Expected Output / Behavior | Error Handling |
|----------|--------------|---------------------------|----------------|
| Rename the device | `"  Studio Mac  "` | Row and live copy hold `Studio Mac`; the next commit's `Keeper-Device` says it; `device_id` unchanged | n/a |
| Rename to blank | `""` / `"   "` / `"\n"` | Refused; row and live copy unchanged | `SyncError::Config` → `internal` |
| Commits made before a rename | any | Keep the old label — history is not rewritten | n/a |
| Empty subject template | `""` or whitespace | `sync(p): 3 added, 1 modified, 1 deleted`, byte for byte | n/a |
| Custom template | `"backup {profile}: {changed} file(s)"` | `backup docs: 1 file(s)`; trailer block still parses whole | n/a |
| Unknown placeholder, on save | `"{Profile} moved"` | Refused, message names `{Profile}` and lists the known set | `SyncError::Config` |
| Unknown placeholder, at render | a row that bypassed `validate` | Left verbatim; the commit still happens | Not an error |
| Multi-line template | `"one\ntwo"` | One line: `one two` (`sanitize`) | n/a |
| Template renders empty | `"{deleted}"` with 0 deletions | Falls back to the mechanical subject | n/a |
| Stray brace | `"{"`, `"a{b"`, `"{}"`, `"{1}"`, `"{a b}"` | Literal text; the validator does not flag it | n/a |
| Edit saves with no poll slot | `pollIntervalMs: null`, prior 45 000 | Stays 45 000 | n/a |
| Edit saves any field | request has no slot for `enabled`/`volumeId`/`id` | Unchanged, by construction | n/a |
| Unpinned settle, fixed disk | stored 5 000 | VM `settleMs: null`, `effectiveSettleMs: 5000`; box empty, placeholder `5` | n/a |
| Unpinned settle, removable | stored 5 000, `removable` | VM `settleMs: null`, `effectiveSettleMs: 10000`; box empty, placeholder `10` | n/a |
| Removable box ticked mid-edit | nothing saved yet | Placeholder flips to `10` live | n/a |
| Typed 5 s on removable | box holds `5` | Note: "keeper is using 10 s here." | n/a |
| Typed 1 s cadence | box holds `1` | Note: "keeper is using 2 s here."; stored 1 000, floored at use | `validate` accepts 0 and 1 000 |
| Empty numeric box saved | box blank | Sends keeper's documented default, not `null` | n/a |

</intent-contract>

## Code Map

- `keeper-sync/src/provenance.rs` — `SUBJECT_PLACEHOLDERS`, the `Piece`/`Pieces` scanner,
  `placeholder_name`, `unknown_subject_placeholder`, `change_subject(template, …)` and the extracted
  `mechanical_subject`. `sanitize` and `commit_message` untouched.
- `keeper-sync/src/profile.rs` — `commit_subject_template`, `MIN_POLL_INTERVAL_MS`,
  `effective_poll_interval_ms()`, the `validate` arm for an unknown placeholder.
- `keeper-sync/src/db.rs` — `DeviceIdentity` doc, `device_identity` doc, `set_device_label` now
  validating and returning `Result<String>`.
- `keeper-sync/src/engine.rs` — `device: Mutex<DeviceIdentity>`, `Engine::device()`,
  `Engine::set_device_label()`, seven use sites re-pointed at a snapshot, `scan_is_due`'s floor moved
  to the profile, `stage_and_commit` call, `head_provenance` test helper.
- `keeper-sync/src/git/commit.rs` — `stage_and_commit`'s fourth parameter and the `change_subject`
  call; six test call sites.
- `keeper-sync/src/git/repo.rs`, `keeper-sync/tests/lfs_roundtrip.rs` — profile fixtures for the same
  parameter.
- `keeper/src/sync_ipc.rs` — `SyncProfileVm` (+4 fields, `settleMs` widened), `SyncProfileReq` (+2),
  `parse_req` inverted, `SyncDeviceVm`, `sync_device`, `sync_device_set_label`.
- `keeper/src/lib.rs` — two commands registered.
- `src/lib/ipc/gen/{SyncProfileVm,SyncProfileReq,SyncDeviceVm}.ts` — hand-matched ts-rs output.
- `src/lib/ipc/client.ts` — `SyncDeviceVm` re-export/import, `syncDevice`, `syncDeviceSetLabel`.
- `src/lib/stores/sync.ts` — four mirrored knob constants.
- `src/components/sync/add-folder-form.tsx` — `pollSeconds` / `commitSubjectTemplate` state,
  `pinnedValue`, `effectiveSettleSeconds`, `effectivePollSeconds`, `syncInForceNote`, three
  placeholders, two in-force notes, two new fields, submit payload, corrected header doc.
- `src/components/settings/sync-section.tsx` — the "This device" block.
- Tests: `provenance.rs`, `profile.rs`, `db.rs`, `engine.rs`, `git/commit.rs`, `sync_ipc.rs`,
  `add-folder-form.test.tsx`, `sync-section.test.tsx`, plus fixture updates in `sync-pane.test.tsx`
  and `sync.test.ts`.

## Tasks & Acceptance

**Execution:**
- [x] `provenance.rs` — one scanner, two consumers; `change_subject` takes a template first and falls
  back to `mechanical_subject` for an empty template and for one that renders to nothing. — Makes the
  subject shapeable without a second grammar the validator could disagree with.
- [x] `profile.rs` — the template field, the poll floor, `effective_poll_interval_ms`, the `validate`
  arm. — Puts both "number in force" rules in the one place that owns the profile's defaults.
- [x] `db.rs` — `set_device_label` validates, trims and returns what it stored; the id's immutability
  and the no-rewrite rule are written down where the write happens.
- [x] `engine.rs` — interior mutability for the label, a snapshot accessor, the setter; the floor
  moved out of `scan_is_due`.
- [x] `git/commit.rs` (+ `repo.rs`, `lfs_roundtrip.rs`) — `profile: &SyncProfile` in place of
  `profile_name: &str`, so the subject and the name come from one argument.
- [x] `sync_ipc.rs` — `parse_req` clones `prior`; the VM separates pinned from effective; the two
  device commands; `SyncDeviceVm`.
- [x] `lib.rs`, `client.ts`, `gen/*.ts` — the commands reachable and typed.
- [x] `sync.ts` — the four mirrored constants, each documented as mirroring its Rust const.
- [x] `add-folder-form.tsx` — the two new fields, the placeholders, the in-force notes, the submit
  payload that sends keeper's default rather than `null`.
- [x] `sync-section.tsx` — the device name, its id, and what a rename does and does not do.
- [x] Tests as listed in Verification.

**Acceptance Criteria:**
- Given a profile with `poll_interval_ms = 45_000`, when an edit is saved through `parse_req` with no
  `pollIntervalMs`, then it is still 45 000.
- Given `SyncProfile` gains a field, when the sync_ipc suite runs, then
  `a_save_cannot_move_a_field_no_request_can_express` fails until the field is classified.
- Given a device renamed to `Studio Mac`, when the next commit is made, then `Keeper-Device` says
  `Studio Mac (<id>)` with the same id, and the earlier commit still says the old name.
- Given an empty template, when a commit is made, then the subject is byte-for-byte today's.
- Given `"backup {profile}: {changed} file(s)"`, when a commit carrying one added file is made, then
  the first line is `backup docs: 1 file(s)` and the trailer block still parses.
- Given a removable folder that pins no wait, when its edit form is opened, then the settle box is
  empty with placeholder `10`, and the Advanced disclosure offers the scan cadence.
- Given `1` typed into the cadence box, when it is rendered, then "keeper is using 2 s here." appears.

## Design Notes

**Why `Option` on the VM and not a mirrored default in TS.** "This profile pins nothing" is a real
state — it is what `effective_settle_ms` branches on — and the wire should be able to say it. With
`settleMs: number` the frontend would have to compare against a hard-coded 5 000 to know whether the
box should show a value or a placeholder, which is a second encoding of a load-bearing rule. `None`
states it once, in Rust.

**Why the form sends the default instead of `null` for an empty box.** AD-34-9's rule is that a
request's silence never moves a stored value, and `parse_req` now honours that for every `Option`
slot uniformly. That makes `null` unavailable as an "unpin me" instruction — which is right: silence
and instruction should not share a spelling. So the form spells "keeper picks" with keeper's own
number, which is exactly how the backend encodes it (`stored == DEFAULT_SETTLE_MS`). A removable
folder therefore still gets its 10 s after a save from a blank box; there is a test.

**Why the effective values are recomputed in the form as well as sent by Rust.** The placeholder has
to follow the removable checkbox *before* anything is saved, and a typed 5 on removable diverges from
what will be in force while the user is still typing. Both need a live answer, and an IPC round-trip
per keystroke is absurd. `effectiveSettleSeconds` therefore mirrors `effective_settle_ms`, documented
as such, and `profileVm`'s `effectiveSettleMs` is what the seeded-edit-form test compares it against
— that test is the drift guard.

**Why an unknown placeholder is refused rather than rendered or dropped.** Rendering `{oops}` into
every commit forever is worse than a rejected save, and dropping it silently is worse still. Refusal
lands where the field is still on screen. `validate` also runs on load, so a hand-edited
`config.json` cannot smuggle one in; the renderer's verbatim fallback exists so a row that somehow
did get in still commits, visibly wrong rather than invisibly.

**Why the grammar has no escape sequence.** `{` plus ASCII letters/underscores plus `}` is a
reference; every other `{` is text. That makes `{Profile}` a reported typo rather than accidental
decoration, and it costs only the ability to write a literal `{profile}` in a commit subject.

**Why the device id may not move, and what a rename actually reaches.** `author_for` derives
`sync@<id>.keeper.invalid` from it and every trailer records it beside the label, so moving it would
re-author history's future and make one machine read as two. Note that **conflict copies are named
after the LABEL, not the id** (`engine.rs` passes `device.label` into
`git::conflict::conflict_name`), so a rename does change future conflict filenames — `conflict_name`
already sanitizes and caps a hostile label, which is why no second guard was added at the store.

**Why `Mutex<DeviceIdentity>` and a cloning accessor.** `Engine` is only ever held as an `Arc`
(`keeper/src/sync.rs`), so `&mut self` is unavailable. A guard-returning accessor would mean holding
the mutex across a commit; a clone costs two `String`s once per commit. Callers that need both halves
take one snapshot, so a rename cannot land between reading the label and reading the id.

**Why `stage_and_commit` takes the profile.** It needs two profile-derived strings now, and
`author_for` in the same module already takes `&SyncProfile`. Replacing the parameter rather than
adding one keeps the signature from growing again the next time.

## Verification

**Not run by me, deliberately:** no build, no linter, no formatter, no test suite. `cargo build`,
`cargo clippy`, `cargo fmt`, `bun run check`, `bun run bindings:check` and `vitest` were all left to
the parent agent, which runs them once after the whole Epic 34 batch lands. Four other agents were
editing this worktree concurrently — including the same `engine.rs` and `git/commit.rs` regions — so a
mid-flight build would have reported their in-progress state, not mine.

**Coordinated over `hub` before editing shared regions:** `stage_and_commit`'s signature and its
engine call site with S348Progress (they append their staging sink after `substitutions`; I replaced
`profile_name` in place, distinct lines); `scan_is_due`'s floor with S349Watcher (who is consuming
`effective_poll_interval_ms` in their degraded-watcher warning); both `Provenance::new` sites with
S3410SyncNow (whose `source:` argument is the line below the two device arguments I changed); and the
shared TS fixture files with everyone.

**What was verified by reading:**
- *Every `stage_and_commit` call site was updated.* Seven in total: `engine.rs` (1), `git/commit.rs`
  tests (6), `git/repo.rs` test (1), `tests/lfs_roundtrip.rs` (1) — grepped after editing, and each
  now passes a `&SyncProfile` whose `name` is the string it passed before, so no rendered subject in
  any existing assertion changes.
- *No `SyncProfile` struct literal exists anywhere.* Grepped `SyncProfile {` across `src-tauri`:
  every construction goes through `SyncProfile::new`, so the new field breaks no caller, and
  `keeper-syncd`'s TOML/CLI paths need no change (`#[serde(default)]`).
- *Every `self.device.` use site is gone.* Grepped `self\.device\.` after editing: no matches remain
  outside the accessor and the setter; the eight former uses now read from a local snapshot.
- *The template grammar's cases were traced by hand* for `{`, `a{b`, `{}`, `{1}`, `{a b}`,
  `{a{profile}`, `{oops}`, `one\ntwo`, `{deleted}` with zero deletions, and `{profile}` with a
  whitespace profile name — each matching the row in the I/O matrix above. The scanner consumes at
  least one piece per iteration in every branch, so it terminates.
- *The AD-34-9 test is complete against the current struct.* `SyncProfile` serializes 19 camelCase
  keys; EXPRESSED lists 16 and PRESERVED 3, and the test asserts the union equals the actual key set,
  so the arithmetic is checked by the test rather than by me.
- *Every TS `SyncProfileVm` / `SyncProfileReq` literal was updated.* Grepped
  `settleMs|pollIntervalMs|commitSubjectTemplate` across `src/` afterwards: nine literals across five
  files, all carrying the new keys.
- *The generated bindings match ts-rs's format byte for byte.* Compared against `cat -A` of the
  pre-edit `SyncProfileVm.ts` and `SyncActivityVm.ts`: header line, blank line, type doc block,
  fields joined by `", "`, a trailing space before each doc-commented field's newline, and
  `#[ts(type = ...)]` overrides rendered as written.
- *Both large TS files still parse.* Brace balance checked with `node`, and `add-folder-form.tsx`
  re-read through the tree-sitter structural view — it resolves to a complete declaration list with
  no unterminated nodes.

**Tests added (for the parent to run):**
- `provenance.rs` — the empty-template byte-for-byte regression, placeholder substitution, a
  completeness check that every documented placeholder is wired into the renderer, unknown-placeholder
  behaviour, stray braces, single-line enforcement, empty-render fallback.
- `profile.rs` — the cadence floor (including a stored zero, which stays valid), and template
  validation with a message that names the bad placeholder.
- `db.rs` — the trimmed return value, the id holding across a rename, and an empty label being
  refused without disturbing the row.
- `engine.rs` — `renaming_the_device_reaches_the_next_commit_and_leaves_the_id_alone`, which also
  asserts the earlier commit keeps the old name and that a refused rename leaves the live copy alone.
- `git/commit.rs` — `a_profiles_subject_template_is_what_the_commit_says`, asserting the first line
  and that the trailer block still parses whole.
- `sync_ipc.rs` — `a_save_cannot_move_a_field_no_request_can_express` (the mechanized AD-34-9 test),
  the concrete DW-116 cadence case, the template round-trip plus edge rejection, and the
  pinned-versus-in-force view-model split including the measured removable case.
- `add-folder-form.test.tsx` — placeholder follows the removable box, the in-force note appears and
  retracts, an empty box sends keeper's default rather than `null`, the cadence and template are
  carried as typed, and an edit form leaves an unpinned knob blank with the right placeholder.
- `sync-section.test.tsx` — the device name and id render, a rename round-trips through the trimmed
  stored value with an acknowledgement, and a refused rename reports instead of showing a name
  nothing will use.

**Commands the parent should run:** `bun run check`, `cargo clippy --workspace`, `cargo test -p
keeper-sync -p keeper`, and `bun run bindings:check` (which regenerates `src/lib/ipc/gen` from ts-rs
and will diff the three files hand-written here).
