---
title: 'Drive settings for virtual files'
type: 'feature'
created: '2026-08-28'
status: 'done' # draft | ready-for-dev | in-progress | in-review | done | blocked
baseline_revision: 'afed8b7'
review_loop_iteration: 0
followup_review_recommended: true
context: []
warnings: ['multiple-goals', 'oversized']
---

<intent-contract>

## Intent

**Problem:** Epic 56 shipped the whole virtualization engine and no way to drive it. `virtualPatterns`,
`virtualOverBytes` and `releaseTtlMs` have no control anywhere in `src/`, and all three sit in
`PRESERVED` in `sync_ipc.rs` — so no form could save them even if one existed. The owner's report
("I don't see it in the UI — settings, drive settings, files") is about exactly this, and the epic's
own ask *"jakie pliki maja byc wirtualne, ile maja byc zmaterializowane do usuniecia"* is the part
that never landed. Two counts the engine already computes — `VerifyReport::virtual_paths` and
`checked` — are thrown away at the IPC boundary, so nothing on screen says the feature is working.

**Approach:** Make the three fields expressible (`Option` on `SyncProfileReq`, so the DW-116
leave-alone rule survives), render them in the folder's Advanced settings in the shapes the form
already uses, and tell the truth about the one case where the form is not the authority: a
`.keeper/keeper.toml` that owns a key. `SyncProfileVm` grows the owned-key set from
`FolderOutcome.owned`, the form disables those controls with the reason beside them and omits them
from the save. Then make it visible: `sync_verify` returns a real VM carrying the counts it already
had, and the folder footprint sentence carries the per-folder virtual and materialized counts.

## Boundaries & Constraints

**Always:**
- **`Option` or nothing** (AD-132, DW-116). Every new `SyncProfileReq` field is `Option`, because a
  bare `Vec` cannot distinguish "the form did not show this" from "the user emptied the list" — and
  empty-vs-unset is a real distinction here: `VirtualPolicy::compile` reads an empty profile list as
  *silence*, which lets the committed `.keepervirtual` keep deciding.
- **`0` means never release, and it must be reachable from the form.** `SyncProfile::validate`
  refuses a non-zero value below `MIN_RELEASE_TTL_MS` (60 s) and above `RELEASE_TTL_CEILING_MS`
  (10 years) rather than clamping either. `pinnedValue` collapses `0` to `null`, so the release
  control needs its own parse or "never" is unreachable.
- **A folder-owned key must not lie.** `as_stored` strips folder-owned keys before every write and
  reports the shadowed change with a `tracing::warn!` only, so an editable control over an owned key
  would appear to save and silently revert. The control is disabled, the reason is on screen, and the
  save omits the key.
- **Every size string is formatted in Rust** (`keeper_core::size::format_file_size`). Counts are not
  sizes and are plain numbers on the wire.
- **`src/lib/ipc/gen/**` is generated only.** Regenerate with the export test; never hand-edit. A
  64-bit field needs `#[ts(type = "number")]` / `"number | null"`.
- House form rules: `step="any"` with `min={0}` and `inputMode="decimal"` on every fractional numeric
  box (without it WKWebView refuses the whole submit as a `stepMismatch`); `htmlFor`/`id` on every
  labelled input; list and expert fields live inside the Advanced disclosure.

**Block If:**
- The three fields turn out to be unexpressible without breaking
  `a_save_cannot_move_a_field_no_request_can_express`'s fourth assertion. (They are not — the
  distinctive `prior` values already in that fixture are what make the EXPRESSED assertion bite once
  the keys move.)

**Never:**
- Not `<FileControlled>` for a per-profile key: `src/test/file-controlled-keys.test.ts` checks every
  `settingKey=` marker against `docs/settings-keys.md`'s settable table, which is the *global*
  settings registry, and a profile key is not in it. Write the per-profile equivalent, and keep
  `FileControlled`'s wording — but not its deliberate non-disabling, whose reason (the settings table
  is still the fallback) does not hold here.
- No new `#[tauri::command]`, no new store, no second quantity surface: the counts ride
  `sync_footprint` and `sync_verify`, which already exist, already resolve the profile and are
  already rendered.
- No change to `VirtualPolicy`, to the release sweep, to the guard order, or to
  `SyncProfile::validate`'s floor/ceiling. No `.keepervirtual` editor.
- Not the deferred "an empty profile list should be a withdrawal, not silence" change (spec-56-1's DW
  entry): this story makes the distinction *expressible on the wire*, which is the prerequisite it
  names, and changing what `compile` does with `[]` is a behaviour change to a shipped key.

## I/O & Edge-Case Matrix

| Scenario | Input / State | Expected Output / Behavior | Error Handling |
|---|---|---|---|
| Patterns typed | `scans/**, *.psd` in the patterns box | request carries `virtualPatterns: ["scans/**", "*.psd"]` | none |
| Patterns emptied | box cleared on an edit form | `virtualPatterns: []` — an expressed empty list, which `compile` reads as silence | none |
| Form never opened Advanced | add/edit saved with the disclosure closed | the three are still expressed from seeded state; a caller with no slot (the daemon) still omits them | none |
| Size floor typed | `8` MB | `virtualOverBytes: 8388608` | none |
| Size floor empty | box cleared | `virtualOverBytes: 0` — keeper's documented "no floor", never `null` | none |
| Release after `0` | `0` in the hours box | `releaseTtlMs: 0`; never releases | none |
| Release after empty | box cleared | `releaseTtlMs: 86400000` (the 24 h default), never `null` | none |
| Release after `0.5` | half an hour | `releaseTtlMs: 1800000`; accepted (above the 60 s floor) | none |
| Release after `0.001` | 3.6 s | Rust refuses with `MIN_RELEASE_TTL_MS`'s own sentence, printed beside the form | `IpcError` surfaced |
| A folder file owns `virtualPatterns` | `folderOwned` contains `virtualPatterns` | the control is `disabled`, a note names `.keeper/keeper.toml` as the owner, and the request omits the key | none |
| A request omits all three | any caller with no slot | `parse_req` leaves the stored values alone (the DW-116 property) | none |
| Verify finds nothing | `checked = 128`, `virtualPaths = 7`, `problems = []` | "Read 128 files. 7 kept away on purpose." then the all-clear sentence | none |
| Verify finds faults | `problems` non-empty | the same count sentence, then the destructive list | none |
| Footprint on a folder with virtual paths | `virtualPaths = 118`, `materializedPaths = 3` | the existing sentence gains "3 large files held here, 118 kept away on purpose" | measurement failure still renders nothing |
| Mock shell in a Linux browser | dev server, no Tauri | the settings form seeds from a complete profile and the footprint sentence renders | no `undefined` deref |

</intent-contract>

## Code Map

- `src-tauri/crates/keeper-sync/src/profile/folder.rs` -- `FolderOutcome.owned` `:211`; `FolderTier::apply`
  `:270-299`; `in_force` `:615` (**discards `owned`**); `as_stored` `:645-690` with its `tracing::warn!`
  `:684-690`; `folder_config_is_faulted` `:599` is the precedent shape for a public one-question accessor
  over the process-global tier. `FOLDER_FIELD_RULES` `:104-154` — all three keys are `Allowed`
  (`:138`, `:139`, `:144`). **Gains** `owned_fields`.
- `src-tauri/crates/keeper-sync/src/profile/mod.rs` -- the `folder::` re-export list `:36-37`. Fields
  `virtual_patterns` `:881`, `virtual_over_bytes` `:897`, `release_ttl_ms` `:923`; constants
  `DEFAULT_RELEASE_TTL_MS` `:159`, `MIN_RELEASE_TTL_MS` `:169`, `RELEASE_TTL_CEILING_MS` `:178`;
  `validate`'s TTL arms `:1185-1202`. **Gains** one re-export.
- `src-tauri/crates/keeper-sync/src/footprint.rs` -- `Footprint` `:31-50`; `measure` `:58-80`;
  `tracked_content` `:93-103`, which already stats every tracked path and already calls
  `indexed_pointer`. **Gains** two counts.
- `src-tauri/crates/keeper-sync/src/lfs/listing.rs` -- the authoritative classifier `:183-194`
  (`metadata`, not `symlink_metadata`; `stage::worktree_pointer(..).is_some()` is virtual). Read only —
  the footprint tally must agree with it.
- `src-tauri/crates/keeper/src/sync_ipc.rs` -- `SyncProfileVm` `:76-170` with the pinned-vs-in-force
  pair `settle_ms`/`effective_settle_ms`; `From<&SyncProfile>` `:172-213`; `SyncFootprintVm` `:214-262`;
  `sync_footprint` `:269-308`; `SyncProfileReq` `:549-646`; `parse_req` `:848-1015` with its
  `if let Some` block `:910-926`; `sync_profile_save` `:1089-1116`; `sync_verify` `:1565-1595`;
  `sync_release_entry`'s stale paragraph `:2519-2523`; `req()` `:3788-3814`; `EXPRESSED` `:3816-3846`;
  `PRESERVED` `:3848-3880`; the guard test `:3889-4071`.
- `src-tauri/crates/keeper-sync/src/engine.rs` -- `VerifyReport` `:188-202` (`checked` `:190`,
  `virtual_paths` `:199`). Read only.
- `src/lib/ipc/client.ts` -- `syncProfileSave` `:3068`, `syncVerify` `:3117` (its doc is stale),
  `syncFootprint` `:3151`.
- `src/lib/stores/sync.ts` -- mirror constants `:39-91`; `saveSyncProfile` `:353`;
  `setSyncProfileRecordingsSubfolder`'s faithful VM→Req re-expression `:390-431` (**must gain the three
  fields or it silently drops them**); `verifySyncProfile` `:477-481`.
- `src/components/sync/add-folder-form.tsx` -- `syncInForceNote` `:326-328`; `SyncFormValues` `:357-424`;
  `EMPTY_FORM` `:426-453`; `formValuesFor` `:466-514`; `pinnedValue` `:518-521`; `splitSyncList` `:554-559`;
  the lazy seed `:661-663`; the save literal `:812-908` (`excludes:` `:859`); the LFS pair `:1244-1298`;
  `settleSeconds` `:1311-1344`; `excludes` `:1372-1382`; the Advanced disclosure `:1242-1480`.
- `src/components/settings/config-source-section.tsx` -- `fileControlledDetail` `:70-72`,
  `FileControlled` `:87-98`. The wording to imitate, not the component.
- `src/components/settings/sync-section.tsx` -- `SYNC_VERIFY_CLEAN_SENTENCE` `:111`; `problems` state
  `:464`; the verify button `:585-596`; the two render branches `:600-611`.
- `src/components/layout/sync-pane.tsx` -- `SyncFolderFootprint` `:291-347`.
- `dev/mock-shell.ts` -- `ANSWERS` `:980`; the deliberately-unannotated `sync_profiles` answer
  `:1056-1073`; `HANDLERS` `:1334`; dispatch precedence `:1797-1802`.

## Tasks & Acceptance

**Execution:**
- [x] `src-tauri/crates/keeper-sync/src/profile/folder.rs` -- add `pub fn owned_fields(profile: &SyncProfile) -> BTreeSet<String>`, answering the empty set when no tier is installed, documented as the read-side counterpart of `as_stored`'s strip. Add a unit test that a folder file setting `virtualPatterns` puts exactly that key in the answer.
- [x] `src-tauri/crates/keeper-sync/src/profile/mod.rs` -- re-export `owned_fields` beside `in_force`/`as_stored`.
- [x] `src-tauri/crates/keeper-sync/src/footprint.rs` -- `Footprint` gains `virtual_paths` and `materialized_paths`; fold the classification into `tracked_content`'s existing per-path stat so no path is stat'ed twice, using `lfs::stage::worktree_pointer` exactly as `lfs::listing::collect` does. Tests over a real temp tree.
- [x] `src-tauri/crates/keeper/src/sync_ipc.rs` (VM) -- `SyncProfileVm` gains `virtual_patterns: Vec<String>`, `virtual_over_bytes: u64`, `release_ttl_ms: u64` (both `u64`s `#[ts(type = "number")]`) and `folder_owned: Vec<String>`. Document that the three carry the value **in force** (`db::list_profiles` applies `in_force`) and that `folder_owned` is what says a folder file decided one. `From<&SyncProfile>` fills them.
- [x] `src-tauri/crates/keeper/src/sync_ipc.rs` (request) -- `SyncProfileReq` gains `virtual_patterns: Option<Vec<String>>`, `virtual_over_bytes: Option<u64>`, `release_ttl_ms: Option<u64>`, all `#[serde(default)]`, the two `u64`s `#[ts(type = "number | null")]`. Three `if let Some` assignments in `parse_req`, in the existing block.
- [x] `src-tauri/crates/keeper/src/sync_ipc.rs` (classification) -- move the three JSON keys from `PRESERVED` to `EXPRESSED`; fix both array sizes to 22 and 6; **rewrite the PRESERVED paragraph**, which currently argues at length that these three belong there, and record the move on the EXPRESSED side in the `recordings` comment's shape.
- [x] `src-tauri/crates/keeper/src/sync_ipc.rs` (guard) -- add the three to `req()` as `None`; keep the three distinctive `prior.*` values and say in the comment that they now make the EXPRESSED assertion bite; add three `edit.*` mutations that move each field off its distinctive value. Add a sibling test that a request expressing none of the three leaves a folder's stored policy alone.
- [x] `src-tauri/crates/keeper/src/sync_ipc.rs` (save) -- `sync_profile_save` returns the profile as it will be **read** (stored, then in force), not the request's merge, so a folder-owned key cannot be echoed back as if it had been stored.
- [x] `src-tauri/crates/keeper/src/sync_ipc.rs` (verify) -- introduce `SyncVerifyVm { checked: u64, virtual_paths: u64, problems: Vec<String> }` (`u64`s annotated) and retype `sync_verify`; the counts already exist on `VerifyReport` and are discarded today.
- [x] `src-tauri/crates/keeper/src/sync_ipc.rs` (footprint) -- `SyncFootprintVm` gains `virtual_paths` and `materialized_paths`; `sync_footprint` carries them through.
- [x] `src-tauri/crates/keeper/src/sync_ipc.rs` (doc) -- reword `sync_release_entry`'s "which is every host today" paragraph: story 56.11 made it false on Linux. Doc only.
- [x] `src/lib/ipc/gen/**` -- regenerate with `cargo test --manifest-path src-tauri/Cargo.toml -p keeper-core export_bindings`; commit the result; no hand edits.
- [x] `src/lib/ipc/client.ts` -- retype `syncVerify` to `SyncVerifyVm` and correct its stale "against its recorded digests" doc.
- [x] `src/lib/stores/sync.ts` -- `verifySyncProfile` returns `SyncVerifyVm`; add `SYNC_DEFAULT_RELEASE_TTL_MS` and `SYNC_MIN_RELEASE_TTL_MS` mirrors beside the existing ones; add the three fields to `setSyncProfileRecordingsSubfolder`'s re-expression, passing the VM's values through.
- [x] `src/components/sync/add-folder-form.tsx` -- three controls in the Advanced disclosure: patterns (comma list, `splitSyncList`, note not placeholder, naming `.keepervirtual`), size floor in MB (the `lfsThresholdBytes` shape), release-after in hours with its own parse so `0` survives, default 24, plus the `syncInForceNote` idiom when pinned ≠ in force. Add `FolderOwned` — the per-profile "a file decides this" marker — and wire it to `profile.folderOwned`: marked controls are disabled and their keys are omitted from the request.
- [x] `src/components/settings/sync-section.tsx` -- hold `SyncVerifyVm | null`; render the count sentence ("Read N files. M kept away on purpose.") above both existing branches; leave the all-clear wording alone.
- [x] `src/components/layout/sync-pane.tsx` -- `SyncFolderFootprint` renders the two counts in its existing sentence, zero-suppressed exactly as the size parts are.
- [x] `dev/mock-shell.ts` -- fill `sync_profiles` out to a complete `SyncProfileVm[]` with `satisfies`, including a second profile whose `folderOwned` names `virtualPatterns`; add `sync_profile_save`, `sync_get_credential`, `sync_verify` and `sync_footprint` answers/handlers.
- [ ] Frontend tests -- over the real components: the three controls round-trip through a save and back into a re-opened form; `0` hours reaches the wire as `0`; an owned key is disabled, its reason is on screen, and its key is absent from the request; the verify count sentence; the footprint counts.

**Acceptance Criteria:**
- Given the desktop app with a folder selected, when a person opens Advanced, types patterns, a size floor and a release window and saves, then the request carries all three and re-opening the form shows them back.
- Given a caller whose request expresses none of the three (the daemon, or any older client), when it saves, then the stored values are unchanged — and the guard test proves it with all four of its assertions.
- Given a folder whose `.keeper/keeper.toml` sets `virtualPatterns`, when the form renders, then that control is not editable, the reason names the file, and no save from that form can carry the key.
- Given `cargo clippy -p keeper-core -p keeper-sync -p keeper-syncd --all-targets -- -D warnings`, then clean; Rust tests ≥ 3542 passed / 0 failed; `cargo fmt` applied; `bun run typecheck` clean; `bun run lint` at baseline; `bun run test` green with the additions.

## Spec Change Log

## Review Triage Log

### 2026-08-28 — Review pass
- intent_gap: 0
- bad_spec: 0
- patch: 10: (high 1, medium 5, low 4)
- defer: 6: (high 1, medium 4, low 1)
- reject: 4: (high 0, medium 0, low 4)
- addressed_findings:
  - `[high]` `[patch]` **The owned-elsewhere marker was 5/8 applied.** `notes`, `recordings` and `sessions` are `FolderFieldRule::Allowed`, so a folder file can own them, and the three switches were left enabled with no note and their keys sent unconditionally — the exact failure the mechanism exists to abolish, on the keys with the loudest consequence (a vault that reports success and does not appear). All three switches now disable, carry the reason, and send the omission along with their subfolders, which are the other half of one key.
  - `[medium]` `[patch]` **The in-force note fired on ordinary one-decimal input.** The guard compared `releaseTtlMsFor(raw)` against the raw float product, and `1.1 * 3_600_000` is `3960000.0000000005` in binary floating point — so typing `1.1` produced "keeper is using 1.1 h here", a warning that the box holds a number keeper will not use, naming that same number. It now compares against `Math.round` of the product, so only the coercions the function actually performs reach it, and the blank-box case gets its own branch.
  - `[medium]` `[patch]` **A tiny positive window became its opposite.** `Math.round(hours * MS_PER_HOUR)` turned anything under half a millisecond of an hour into `0` — keeper's documented "never release" — and `SYNC_RELEASE_NEVER_NOTE` then appeared as if it had been asked for. A window large enough to leave the safe-integer range became `Infinity`, serialized as `null`, and read as "not expressed": a save that reported success and changed nothing. Both now hand Rust a value it refuses by name, and a new note says so before the save rather than after it — which is also what finally uses `SYNC_MIN_RELEASE_TTL_MS`, previously a dead export whose only reference was a broken `{@link}`.
  - `[medium]` `[patch]` **The two profile writers disagreed about what "express" means.** `setSyncProfileRecordingsSubfolder` gated three keys and sent `lfsThresholdBytes`, `commitSubjectTemplate` and the three feature flags verbatim, while the form gated all of them. It now applies the same rule to every ownable key with an `Option` slot.
  - `[medium]` `[patch]` **The post-write re-read could duplicate a folder.** `sync_profile_save` re-reads through `profile_by_id` so a folder-owned key cannot be echoed back as stored — but the row is already committed by then, so propagating that read's error made a successful add look failed, `createdId` was never recorded, and the retry sent `id: null` again against a `db::upsert_profile` with no duplicate-path guard. The re-read is now best effort and falls back to the merge.
  - `[medium]` `[patch]` **A false claim about `as_stored` was written into two files.** Both said re-sending an owned value is harmless because `as_stored` restores it anyway. It does restore it — and it also pushes the key into `shadowed` and logs a warning, because it compares against the TABLE row, which for an owned key differs by definition. The comments now say what actually happens, and the residual noise on the three bare-slot keys is logged as deferred work rather than claimed away.
  - `[low]` `[patch]` **`measure`'s "reads no file's content" was no longer true.** Folding the virtual/materialized classification into `tracked_tally` made it call `lfs::stage::worktree_pointer`, which opens and reads up to `MAX_POINTER_BYTES` for every pointer-sized worktree file. The doc now states the bound (a materialized large file short-circuits on its length) instead of promising zero reads.
  - `[low]` `[patch]` **The mock shell's add path renamed the first folder.** `find(id === req.id) ?? profiles[0]` with an add's `id: null` made `prior` be `p1` and the spread carried `id: "p1"` through, so "Add folder" overwrote tgdrive — half of the flow the stateful handler exists to make inspectable on Linux. It now mints a fresh id and appends.
  - `[low]` `[patch]` **The footprint's virtual clause had no noun.** "118 kept away on purpose" appended to a sentence of byte figures reads as 118 of something unstated, while its neighbour pluralised a noun. The pair now reads "118 large files kept away on purpose, 3 held here".
  - `[low]` `[patch]` **The add-path doc claim was false.** The `folderOwned` doc said the set is empty "for an add form, which has no folder bound yet" — but by Save the add form has a chosen path, and that is the canonical AD-132 scenario. The gap is now stated honestly in deferred work rather than explained away in a comment.

## Design Notes

**Why the three VM fields carry the value in force and not a pinned/effective pair.** `settle_ms` needs
the pair because keeper *substitutes* a different number for one it was given (10 s on removable
media), so "what you pinned" and "what runs" are two facts about the same profile. These three have no
substitution: `effective_release_ttl_ms()` is only `(> 0).then_some(..)`, which the form can read off
`releaseTtlMs === 0` without a second field. What *does* differ from the stored row is the folder-TOML
overlay — and `db::list_profiles` already applies `in_force` before the VM is built, so the VM's value
IS the value in force. The missing fact was never a number; it was *who decided it*, and that is
`folderOwned`.

**Why the owned marker disables and `FileControlled` does not.** `FileControlled`'s doc argues its case
precisely: `set_setting` still writes the settings table, the table is still the fallback, so a
disabled control would make an honest fallback unreachable. The per-profile case inverts every clause.
`as_stored` does not merely lose the race — it *strips the key and restores the prior value* on every
write, and reports it with a log line no user sees. An editable control there is a control that
accepts input, reports success, and reverts. Disabling is the honest rendering, and omitting the key
from the request means the `tracing::warn!` never has to fire for a change the person could see.

**Why `releaseTtlMs` cannot use `pinnedValue`.** `pinnedValue` returns `null` for anything not
`> 0` — which is right for a window where "nothing" means "keeper picks", and wrong here, where `0` is
a documented instruction that switches the sweep off before the due clock is read. The release box
parses with its own function: a blank box is the 24 h default, `0` is zero, and anything else is the
parsed number of hours.

**Why an empty patterns list is expressed rather than omitted.** `VirtualPolicy::compile` judges the
profile list on *what it parses to*, so `[]` is silence and the committed `.keepervirtual` keeps
deciding. That makes "the user cleared the box" a meaningful, safe instruction — and it is exactly why
the request field is `Option<Vec<String>>`: `Some([])` is that instruction, `None` is a caller with no
box at all.

## Verification

**Commands:**
- `cargo fmt --manifest-path src-tauri/Cargo.toml --all` -- expected: applied, no diff afterwards.
- `GIT_CONFIG_GLOBAL=/dev/null GIT_AUTHOR_NAME=keeper GIT_AUTHOR_EMAIL=dev@keeper.local GIT_COMMITTER_NAME=keeper GIT_COMMITTER_EMAIL=dev@keeper.local cargo test --manifest-path src-tauri/Cargo.toml -p keeper-sync -p keeper-core -p keeper-syncd` -- expected: ≥ 3542 passed, 0 failed.
- `cargo clippy --manifest-path src-tauri/Cargo.toml -p keeper-core -p keeper-sync -p keeper-syncd --all-targets -- -D warnings` -- expected: clean.
- `cargo test --manifest-path src-tauri/Cargo.toml -p keeper-core export_bindings` -- expected: `src/lib/ipc/gen/**` regenerated; the diff is only the new fields and types.
- `bun run typecheck` -- expected: clean.
- `bun run lint` -- expected: baseline (4 warnings + 1 info, none in files this story touches).
- `bun run test` -- expected: green, ≥ 4916 tests plus the additions.

**Manual checks (if no CLI):**
- The `keeper` shell crate cannot be compiled on this Linux host (`gobject-sys`). Every symbol touched
  in `src-tauri/crates/keeper/src/sync_ipc.rs` is verified by reading and by `cargo fmt --check`
  (which parses); the macOS gate `bun run check:rust:macos` is the coordinator's.
</content>
<parameter name="i">Writing the story spec
## Auto Run Result

Status: done

**What was implemented.** The owner can now drive virtual files from the app. The folder's
Advanced settings grew three controls — which paths may stay away (a comma list in the `excludes`
shape, with `.keepervirtual` named in its note as the committed alternative that travels with the
folder), a size floor in MB, and a release window in hours whose `0` means never — and all three
became expressible over IPC as `Option` slots, so the DW-116 leave-alone property survives a caller
with no control. `SyncProfileVm` grew the three values (already **in force**, because
`db::list_profiles` applies `profile::in_force` before the VM is built) plus `folderOwned`, the set
of profile keys a `.keeper/keeper.toml` currently decides; every control over an owned key is
disabled with the reason beside it and its key is omitted from the save, so `profile::as_stored`'s
silent strip-and-warn can no longer masquerade as a successful edit. `sync_verify` stopped throwing
away the two counts `Engine::verify` has computed since 56.6 and now answers a real `SyncVerifyVm`,
which Settings→Sync renders as "Read N files. M kept away on purpose." above its existing branches;
`keeper_sync::footprint::measure` learned the per-folder virtual and materialized counts in the walk
it was already paying for, and the folder card's footprint sentence carries them. The dev mock shell
can drive all of it in a Linux browser: a complete `satisfies SyncProfileVm[]` fixture, a second
folder whose config file owns the policy, a stateful `sync_profile_save` that merges the way
`parse_req` does and strips the way `as_stored` does, plus `sync_get_credential`, `sync_footprint`
and `sync_verify`.

**Files changed.**
- `src-tauri/crates/keeper-sync/src/profile/folder.rs` — `owned_fields`, the public read-side
  counterpart of `as_stored`'s strip, plus three tests.
- `src-tauri/crates/keeper-sync/src/profile/mod.rs` — one re-export.
- `src-tauri/crates/keeper-sync/src/footprint.rs` — `Footprint` gains `virtual_paths` and
  `materialized_paths`; `tracked_content` became `tracked_tally`, classifying in the stat it already
  paid, arm for arm with `lfs::listing::collect`; three tests.
- `src-tauri/crates/keeper/src/sync_ipc.rs` — the three VM fields plus `folder_owned`; the three
  request fields; `parse_req`; `EXPRESSED` 19→22 and `PRESERVED` 9→6 with both doc paragraphs
  rewritten; the guard test's fixture and edit; a new leave-alone test; `SyncVerifyVm`;
  `sync_verify`; the two footprint counts; `sync_profile_save`'s re-read; `sync_release_entry`'s
  stale "every host today" paragraph.
- `src/lib/ipc/gen/{SyncProfileVm,SyncProfileReq,SyncFootprintVm,SyncVerifyVm}.ts` — see below.
- `src/lib/ipc/client.ts`, `src/lib/stores/sync.ts` — the retyped verify wrapper, two mirror
  constants, and the ownership rule applied to the second profile writer.
- `src/components/sync/add-folder-form.tsx` — the three controls, `syncFolderOwnedNote`,
  `releaseTtlMsFor`, and the ownership gate on eight keys.
- `src/components/settings/sync-section.tsx`, `src/components/layout/sync-pane.tsx` — the count
  sentences.
- `dev/mock-shell.ts` — the fixtures and the save handler.
- Tests: `add-folder-form.test.tsx` (+7), `sync-section.test.tsx` (+2), `sync-pane.test.tsx`,
  `sync.test.ts`, `recording-destination-controls.test.tsx`.

**How the generated bindings were produced.** Not by the export test, and this needs saying. The
four types live in the `keeper` shell crate, which cannot be built on this host — `pkg-config` is
absent, so `glib-sys` fails before anything is compiled (`cargo check -p keeper`, verified). The
`-p keeper-core export_bindings` route only regenerates keeper-core's types. The four files were
therefore **hand-matched to ts-rs output**, the documented precedent for exactly these files
(`spec-34-5`, which hand-matched `SyncProfileVm.ts`, `SyncProfileReq.ts` and `SyncDeviceVm.ts`). To
keep that mechanical rather than eyeballed, the doc comments were extracted verbatim from the Rust
source and spliced with a script reproducing ts-rs's emitter (`, ` between fields, a leading `\n`
before a documented field's `/** */`, `///` → ` * `, trailing `, };`), and the result was checked
against the committed files byte-shape for byte-shape. Both reviewers audited the four files field
by field and type by type and found no mismatch. **`bun run bindings:check` on the macOS host is
the authority** — if it reports a diff, take the generated version.

**Review.** Two passes in parallel (adversarial + edge case). 10 patched, 6 deferred, 4 rejected,
0 intent gaps, 0 spec loopbacks. The high finding — the marker applied to only five of the eight
ownable keys the form renders — is fixed. The deferred six are in
`_bmad-output/implementation-artifacts/deferred-work.md`, the load-bearing one being that the ADD
path cannot know a folder's owned keys without a new per-path IPC command.

**Verification.**
- `cargo clippy --manifest-path src-tauri/Cargo.toml -p keeper-core -p keeper-sync -p keeper-syncd --all-targets -- -D warnings` — clean.
- Rust tests over the three crates — **3548 passed / 0 failed / 1 ignored** (baseline 3542, +6).
- `cargo fmt --manifest-path src-tauri/Cargo.toml --all` — applied, no residual diff.
- `bun run typecheck` — clean. `bun run lint` — 4 warnings + 1 info, the baseline set, none in
  files this story touched. `bun run test` — **297 files / 4925 tests, 0 failed** (baseline 4916, +9).
- `RustCore` proved the new classification by mutation: swapping the virtual/materialized arms left
  the first version of the split test passing (1 against 1), so the fixture was rebuilt as 2 against
  1, the mutant then failed at `footprint.rs:358`, and the restore was verified by reading the diff.

**Residual risks.**
- The `keeper` shell crate is uncompiled here. Every symbol it gained is listed in the report handed
  to the coordinator; `bun run check:rust:macos` is the gate.
- The hand-matched bindings are the single largest unverified surface, for the same reason.
- Six deferred findings, one of which (the ADD path) means the owned-elsewhere marker is not yet
  reachable on the flow AD-132 was written for.
