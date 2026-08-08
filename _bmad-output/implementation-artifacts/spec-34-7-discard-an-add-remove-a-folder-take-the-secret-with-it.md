---
title: 'Discard an add, remove a folder, take the secret with it'
type: 'feature'
created: '2026-07-28'
status: 'review'
baseline_revision: '5c40a22'
---

<intent-contract>

## Intent

**Problem:** Three gaps around ending things, all in the same flow.
(a) The Sync view mounts the shared `AddFolderForm` with no `onCancel` (`sync-pane.tsx:551-557`), so add mode has no way out while edit mode has one (`add-folder-form.tsx:765-769`) — the only exit was the header toggle, which is not what a user reads as "discard this".
(b) The Sync view has no remove affordance at all. The only one is in Settings (`sync-section.tsx:238-246`), and the Sync view's own header comment (`sync-pane.tsx:13-16`) declared that absence deliberate.
(c) `Engine::remove_profile` (`engine.rs:352-358`) deleted the journal, file_state, activity and profile rows but nothing deleted `sync/<id>/credential` (`profile.rs:241-243`). Because the key is derived from the profile id, and a removed id is never re-derived, that keychain item outlived the profile *permanently* and was unreachable by every code path keeper has (AD-34-14).

**Approach:**
(a) Pass `onCancel` to the pane's add form. Discard closes the disclosure, which unmounts the form — the draft lives entirely in that component's state, so unmounting *is* the discard. It is offered only when the list is non-empty, matching the form's own documented rule and keeping a user out of a state with no way back.
(b) Add a Remove button to the folder card and render the Settings confirmation constants verbatim, calling the same `removeSyncProfile` store action. No second wording, no second flow.
(c) `Engine::remove_profile` reads the profile, deletes `profile.secret_key()` through the platform port, and only then deletes the rows. The confirmation copy is rewritten to say both halves of what removal does.

## Boundaries & Constraints

**Always:** One definition of the removal wording (`SYNC_REMOVE_CONFIRM_*` in `sync-section.tsx`), imported by both surfaces. One definition of the credential key (`SyncProfile::secret_key`). The credential delete happens **before** any row delete, and its failure aborts the removal with the profile intact. The on-disk contract at `sync_ipc.rs:499-500` is unchanged: the folder, its contents and its `.git` are never touched. Repo conventions: 2-space/100-char/double-quoted TS with no `any`; Rust with no `unwrap()` in new code; comments explain the decision, not the syntax.

**Block If:** (none — every open question was decidable from the code; the decisions are recorded in Design Notes)

**Never:** Do not touch `SyncActivityList` or `ACTIVITY_KINDS` (story 34.6), `parse_req` (34.5), or `add-folder-form.tsx` (34.4 owns it) — which is why the discard control keeps the shared form's existing `SYNC_FORM_CANCEL_LABEL` ("Cancel") instead of gaining a second, add-only label. Do not delete anything from the working tree on removal. Do not make removal best-effort about the secret: a swallowed keychain error is exactly the orphan AD-34-14 exists to prevent.

## I/O & Edge-Case Matrix

| Scenario | Input / State | Expected Output / Behavior | Error Handling |
|----------|--------------|---------------------------|----------------|
| Discard a filled add | folders exist, add form open with name/URL/path typed | disclosure closes; `sync_profile_save` never called; reopening shows empty fields, "No folder chosen", disabled submit | None — no IPC involved |
| Add form in the empty state | `profiles === []` | no discard control at all (nothing to return to, and the header action that reopens it is hidden) | n/a |
| Save that settles | add succeeds, no token to store | `onSaved(_, true)` closes the disclosure — same end state as discard | n/a |
| Save that does not settle | token write failed, or a new folder's token acknowledgement | disclosure stays open; discard now available (list is no longer empty) and closes it without undoing the stored token | Message stays on screen |
| Remove from the Sync view | card's Remove clicked | `alertdialog` with the Settings title/sentence/labels; nothing removed yet | n/a |
| Remove confirmed | "Stop syncing" | `removeSyncProfile(id)`; card disappears; no detail re-read for the dead id | Failure renders in the card's action-error line |
| Remove declined | "Keep syncing" | dialog closes; `sync_profile_remove` never called; the folder goes on reporting | n/a |
| Remove, profile has a token | keychain holds `sync/<id>/credential` | secret deleted first, then rows, then in-memory state | — |
| Remove, profile has no token | no such keychain item | `secret_delete` is a documented no-op on both desktop backends and the daemon's file store, so removal proceeds | — |
| Remove, keychain refuses | locked keychain / access denied | `Err` before anything is deleted: profile still configured, secret still there, removal retryable | Surfaced as the card's action error |
| Remove, row delete fails after the secret is gone | db error | profile keeps configuration but no token — reports as an auth failure next sync, fixed by re-entering the token or removing again | Propagated |
| Remove an id with no profile row | already removed | no secret to delete; row deletes stay idempotent; call succeeds | — |

</intent-contract>

## Code Map

- `src/components/layout/sync-pane.tsx` -- header comment (the claim that Settings "is the only surface that removes a folder"); the add-form disclosure; `SyncProfileCard`'s `run` helper, header actions and the new confirm dialog.
- `src/components/settings/sync-section.tsx` -- header comment; `SYNC_REMOVE_CONFIRM_SENTENCE` (the single definition of the wording, now shared with the pane); the permanently mounted `AddFolderForm` that deliberately gets no `onCancel`.
- `src/components/sync/add-folder-form.tsx` -- **read only.** `onCancel` is already a prop rendering `SYNC_FORM_CANCEL_LABEL` beside the submit (:765-769); its JSDoc (:292-294) already states the rule this story applies.
- `src/lib/stores/sync.ts` -- `removeSyncProfile` (unchanged behavior; its doc comment now names the token).
- `src-tauri/crates/keeper-sync/src/engine.rs` -- `remove_profile`: the credential delete and the ordering rationale.
- `src-tauri/crates/keeper-sync/src/profile.rs` -- **read only.** `secret_key()` is the one definition of the keychain key.
- `src-tauri/crates/keeper/src/sync_ipc.rs` -- `sync_profile_remove`: doc only; the body already delegates to the engine.
- Tests: `src/components/layout/sync-pane.test.tsx`, `src-tauri/crates/keeper-sync/src/engine.rs` (`mod tests`).

## Tasks & Acceptance

**Execution:**
- [x] `sync-pane.tsx` -- Pass `onCancel={empty ? undefined : () => setAdding(false)}` to the pane's `AddFolderForm`, with a comment on why unmounting is the discard and why the empty state gets none. -- Add mode can be abandoned, and abandoning it leaves nothing behind.
- [x] `sync-pane.tsx` -- Rewrite the header comment's second paragraph: this view now adds, edits *and* removes, and shares removal's wording and store action with Settings. -- The file no longer documents an absence it does not have.
- [x] `sync-pane.tsx` -- Add `confirmOpen` state, a ghost `Remove` button after `Edit`, and the `AlertDialog` built from the Settings constants and `removeSyncProfile`. -- Removal is reachable where the folder is.
- [x] `sync-pane.tsx` -- Give `run` a `refresh = true` parameter and pass `false` for removal. -- No detail re-read against an id the store has just forgotten.
- [x] `sync-section.tsx` -- Rewrite `SYNC_REMOVE_CONFIRM_SENTENCE` to name both halves (settings + token go; folder, contents and git repository stay), and update the header comment plus the add-form call site. -- One sentence, true after AD-34-14, rendered identically on both surfaces.
- [x] `engine.rs` -- `remove_profile` reads the profile and deletes `profile.secret_key()` before `db::delete_profile`, with the ordering rationale in the doc comment. -- The secret cannot outlive the profile.
- [x] `sync_ipc.rs` -- Extend the command doc: the credential is the one thing deleted outside `sync.db`, and that is not an exception to the on-disk contract. -- The contract at :499-500 reads correctly rather than incompletely.
- [x] `sync.ts` -- `removeSyncProfile`'s doc names the token. -- The store layer stops implying configuration-only means nothing at all is deleted.
- [x] `sync-pane.test.tsx` -- Discard test (no save, empty reopened form, no chosen folder); no-discard assertion in the empty-state test; a `remove a folder` describe: confirm-then-remove with the shared sentence, decline, and no detail re-read after removal.
- [x] `engine.rs` -- `removing_a_profile_deletes_its_credential_and_only_its_credential`: two profiles with two secrets, remove one, assert its secret is gone and the other's survives.

**Acceptance Criteria:**
- Given the add form is open with every field filled, when the discard control is clicked, then no profile is created and reopening the form shows empty fields, no chosen folder and a disabled submit.
- Given nothing is configured, when the add form is shown as the empty state, then no discard control is rendered.
- Given a configured folder, when Remove is clicked, then a confirmation appears carrying the exact Settings sentence and nothing is removed until it is accepted; declining removes nothing.
- Given the confirmation is accepted, when removal succeeds, then the card disappears and the removed folder's lists are not re-read.
- Given a profile with a stored credential, when it is removed, then `sync/<id>/credential` is gone, another profile's credential is untouched, and nothing under the folder or its `.git` was written.
- Given `secret_delete` fails, when removal is attempted, then it returns an error with the profile and its rows still present.

## Design Notes

**Why the discard is just an unmount.** The draft (`form`, `storedToken`, `error`) lives only in `AddFolderForm`'s own state, seeded once on mount. `showAddForm` gates the element, so `setAdding(false)` unmounts it and the draft is unrecoverable by construction — there is no store to clear and no reset function that could be forgotten. A settled save reaches the identical end state via `onSaved(_, true) -> setAdding(!settled)`, so discard and save leave the disclosure consistent, which is what the story asked to check.

**Why the empty state gets no discard.** `AddFolderForm`'s JSDoc already draws the line: a surface that reveals the form behind an action passes `onCancel`; one that keeps it permanently on screen passes nothing. With no folders the pane's form *is* the content, and the header action that would reopen it is hidden (`profiles !== null && !empty`) — a discard there would strand the user in a surface offering nothing to do. Conditioning on `empty` applies the existing rule rather than inventing an exception to it.

**Why Settings' copy still has no discard, either.** Same rule, same conclusion, for a different reason: that form is a permanent part of the section, so "cancel" could only mean "clear the fields" — a second meaning for a shared control, on the one surface where abandoning a draft already costs a single click on the dialog's own close.

**Why the label stays "Cancel".** The story calls it a discard; the control is the shared form's existing cancel, and its label lives in `add-folder-form.tsx`, which story 34.4 owns this cycle. Adding an add-mode-only label would mean a new prop on a file another story is editing, to change one word that already reads correctly beside "Add folder". The behavior the acceptance criteria describe is what was implemented; the wording is the shared form's.

**Why the credential delete lives in the engine, not the IPC command.** Three reasons, in order of weight. First, the ordering below is only a *guarantee* if both deletes are one operation: put the secret delete in the Tauri command and every other caller of `remove_profile` silently skips it. Second, `keeper-syncd` is exactly that other caller — it removes profiles through the same `Engine` and never touches the IPC layer, so a shell-side delete would leave the daemon orphaning secrets on every removal. Third, the read side of the same secret already lives here (`Engine::credential`, `engine.rs:1038-1041`), so this is deletion next to its read rather than a new concern in a new crate. The Tauri command needed no body change at all — only a doc that stops implying `sync.db` is the whole of what removal touches.

**Correction (2026-08-08, epic-34 review finding 3).** The second reason above is false and always was: `keeper-syncd` has no profile-removal path at all — no `remove_profile` call, no `Remove` command — and its `reconcile` deliberately *reports* a profile the config stopped naming rather than deleting it (AD-48). This spec's own Verification section states the correct fact ("`remove_profile`'s only callers are `sync_profile_remove` and tests"), so the document contradicted itself and the wrong half is the half that got copied into the shipped rustdoc. The conclusion still holds on reasons one and three, and `Engine::remove_profile`'s doc comment has been rewritten to say so.

**Ordering, and what a partial failure looks like.** Secret first, then rows, then in-memory state; every step's error propagates. The two failure modes are not symmetric, which is what decides the order: a secret orphaned by a deleted row is keyed on an id nothing re-derives, so it is unreachable *forever* — while a row that survives a failed secret delete is just a folder that still syncs and a Remove the user can press again. So the credential goes first and its failure aborts before anything is deleted. If the row delete then fails, what remains is a profile whose token is gone: it reports itself as an authentication failure on the next sync (`engine.rs:1862-1866` already handles a missing credential as a named, user-actionable state) and is fixed by entering the token again or removing again. Neither residue is silent, and neither is permanent.

**A profile with no token removes cleanly.** `SyncPlatform::secret_delete`'s contract is that deleting an absent secret succeeds, and all three implementations honor it: `keyring::Error::NoEntry` (`ipc.rs:590-594`), `ERR_SEC_ITEM_NOT_FOUND` (`ipc.rs:774-779`), and `ErrorKind::NotFound` (`keeper-syncd/src/platform.rs:340-345`). So credential-first does not make the common public-repository case fail; the step can only fail on a genuine keychain error, which is precisely when stopping is right.

**Reading the profile rather than formatting the key.** `remove_profile` could build `sync/<id>/credential` from the id alone, but that would be a second definition of the key format, in the one function whose bug would be invisible (a mistyped key silently deletes nothing). Reading the row and calling `profile.secret_key()` keeps `profile.rs` the only place that knows, and the `if let Some(...)` keeps removal idempotent for an id with no row.

**Why removal skips the detail re-read.** Every other card action ends in `refreshSyncDetail(profile.id)` because an action is when the three lists are most likely to have moved. After a removal there is no folder to read: the three IPC calls would each reject for an unknown profile and `mergeDetail` would write that failure back into the detail store under an id `retainProfiles` has just pruned, resurrecting a dead entry until the next poll. Hence the `refresh` parameter rather than a second copy of the busy/error lifecycle.

**Copy.** The old sentence ended "removing a folder never deletes anything", which AD-34-14 makes false. The new one keeps the shape and fixes the claim: what goes (settings, the stored access token) and what stays (the folder, its contents, its git repository), ending on "never deletes your files" — the promise that is still absolute. Both surfaces render the same constant, so neither can drift.

## Verification

**Deliberately not run by me:** the build and the test suite. No `cargo build`, `cargo test`, `cargo clippy`, `cargo fmt`, `bun run check`, `bun test` or `vitest` was executed for this story — six agents are editing this tree concurrently and the parent runs the suite once at the end.

**What was actually checked, by reading:**
- `AddFolderForm`'s cancel path: `onCancel` is rendered only when defined (`add-folder-form.tsx:765-769`) and does nothing but call the callback — so the discard's semantics are entirely the caller's unmount. Confirmed the draft has no store backing: `form`/`storedToken`/`error` are `useState` inside the component, and `profile` is seeded once.
- The path display renders `SYNC_NO_FOLDER_CHOSEN_LABEL` when `form.localPath === ""` (`add-folder-form.tsx:565-570`), which is what the reopened-form assertion checks, and submit is disabled while `incomplete` (:761), so an empty reopen cannot be submitted.
- `SYNC_REMOVE_CONFIRM_SENTENCE` has exactly one definition; grep for the old wording found no other copy. The existing Settings assertions (`sync-section.test.tsx:351-352`, `/left on disk/` and `/never deletes/`) still match the new sentence, so that suite needed no edit.
- The three `SyncPlatform::secret_delete` implementations all treat a missing entry as success (cited above), so credential-first cannot fail a no-token removal.
- `db::delete_profile` (`db.rs:224-227`) removes journal, file_state and activity rows before the profile row and is safe for an unknown id (`db.rs:962-965`), so the `if let Some` guard does not change idempotence.
- `remove_profile`'s only callers are `sync_profile_remove` (`sync_ipc.rs:507`) and tests, so no other path can bypass the new delete.
- The new Rust test uses the `TestPlatform`/`Engine::open` pattern already used at `engine.rs:2431-2434`, keeping the platform handle so it can assert through the public `secret_get`. It would fail on the plausible bugs: no delete at all, a wrong key, or a store-wide clear (the second profile's secret is asserted to survive).
- The detail-store reasoning was verified against `retainProfiles` (`sync-detail.ts:109-127`) and `refreshSyncDetail` (`:163-182`); `SYNC_DETAIL_POLL_MS` is 5 s, comfortably longer than the new tests' lifetime, so the call-count assertion is not racing a poll tick.
