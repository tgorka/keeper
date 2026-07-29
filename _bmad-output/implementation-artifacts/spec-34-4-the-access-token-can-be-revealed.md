---
title: 'The access token can be revealed'
type: 'feature'
created: '2026-07-28'
status: 'review'
baseline_revision: '5c40a22'
---

<intent-contract>

## Intent

**Problem:** The sync access-token field (`src/components/sync/add-folder-form.tsx:685-697`) is a
`type="password"` box with no way to read it back — not what the user is typing, and not what is
already in the keychain. A mistyped token is unverifiable before saving, and a stored one is
unrecoverable afterwards, even though the secret is sitting in the OS keychain under
`sync/<id>/credential` and `SyncPlatform::secret_get` (`keeper-sync/src/platform.rs:33`) has been
able to read it since Story 24.2. Both help strings promise the opposite of a fix — the add form
says keeper "never shows it again — there is no way to read a stored token back out", the edit form
says "keeper cannot show a stored token".

**Approach:** Add `sync_get_credential(id) -> Option<String>` beside `sync_set_credential`, bind it
as `syncGetCredential`, and give the field two distinct affordances: an eye toggle over the input
that flips `password`/`text` for whatever the field holds, and a separate "Show stored token" action
that calls the new command on press. Per AD-34-7 the read is never part of loading: `SyncProfileVm`
carries no token, the form still seeds `token: ""` in both modes, and nothing calls
`syncGetCredential` outside the button's `onClick`. Rewrite both help strings to say where the token
is kept and that showing it is something the user asks for.

## Boundaries & Constraints

**Always:** The eye is a real `<button type="button">` — keyboard reachable, `aria-pressed` tracking
state, `aria-label` naming the next press (`Show token` / `Hide token`), and a `lucide-react`
`Eye`/`EyeOff` glyph, matching the icon set already imported in this file. The reveal reports all
three outcomes as themselves: a token fills the field and unmasks it, `null` prints "No token is
stored for this folder.", a rejection prints the read-failure prefix plus `syncErrorMessage(raw)`.
Clearing a revealed token blanks the field, because a revealed value left behind would be written
straight back by the next save. TS/Biome rules (no `any`, 2-space, 100-char, double quotes); Rust
follows the neighbouring command's shape exactly.

**Block If:** (none — the port method, the profile lookup helper and the error mapper all exist)

**Never:** Do not add a token field to `SyncProfileVm`, and do not call `syncGetCredential` on
mount, on expand, or from any store refresh. Do not render the toggle as a span with a click
handler. Do not touch `parse_req` (Story 34.5), `SyncActivityVm` (34.6) or `sync_profile_remove`
(34.7). Do not seed the field from a profile in `formValuesFor`.

## I/O & Edge-Case Matrix

| Scenario | Input / State | Expected Output / Behavior | Error Handling |
|----------|--------------|---------------------------|----------------|
| Type then reveal | Add mode, `ghp_secret` typed, eye pressed | Input `type="text"` showing `ghp_secret`; button `aria-pressed="true"`, label `Hide token` | n/a |
| Reveal then hide | Same, eye pressed twice | Back to `type="password"`, `aria-pressed="false"`, label `Show token` | n/a |
| Edit form opens | Existing profile, disclosure expanded | Field empty, no `placeholder`, `sync_get_credential` not called | n/a |
| Show stored token, one exists | `Ok(Some("ghp_stored"))` | Field holds `ghp_stored` and is unmasked | n/a |
| Show stored token, none stored | `Ok(None)` | "No token is stored for this folder."; field stays empty and masked | Not an error path |
| Show stored token, read fails | keychain rejects | "The stored token could not be read: <message>"; no "none stored" line | `syncErrorMessage(raw)` after the prefix |
| Clear after a reveal | Revealed token, Clear pressed | "The stored token was removed."; field emptied and re-masked; a following save writes no credential | Clear failure keeps the existing error line |
| Type after a reveal or a clear | Any keystroke | "removed" and "none stored" lines both retract | n/a |

</intent-contract>

## Code Map

- `src-tauri/crates/keeper/src/sync_ipc.rs` -- `sync_set_credential` (`:661`) and
  `sync_clear_credential` are the existing pair; `profile_by_id` (`:689` pre-edit) resolves the
  profile and `sync_ipc_error` maps `SyncError`. New `sync_get_credential` sits between them.
- `src-tauri/crates/keeper-sync/src/platform.rs` -- `SyncPlatform::secret_get` (`:33`) already
  exists and already documents `Ok(None)` as an ordinary state. Nothing added to the port.
- `src-tauri/crates/keeper/src/lib.rs` -- the desktop `keeper_with_commands!` list (`:498-516`);
  one appended line.
- `src/lib/ipc/client.ts` -- the sync credential block (`:2546-2581`); `syncGetCredential` appended
  after `syncSetCredential`.
- `src/components/sync/add-folder-form.tsx` -- token constants, form state, the new
  `revealStoredToken` handler, and the token field's JSX inside the Advanced disclosure.
- `src/components/sync/add-folder-form.test.tsx` -- the existing Vitest suite; the client mock gains
  `syncGetCredential`.

## Tasks & Acceptance

**Execution:**
- [x] `sync_ipc.rs` -- Add `#[tauri::command] pub async fn sync_get_credential(state, id) ->
  Result<Option<String>, IpcError>` calling `platform.secret_get(&profile.secret_key())`, and
  correct `sync_set_credential`'s "write-only by design" doc, which the new command falsifies. --
  Gives the frontend a read that only it can trigger.
- [x] `lib.rs` -- Register `sync_ipc::sync_get_credential` in the desktop invoke handler. --
  Without it the command is unreachable.
- [x] `client.ts` -- Add `syncGetCredential(id): Promise<string | null>`, documented as
  explicit-action-only, and correct `syncSetCredential`'s "never back out" doc. -- The typed
  binding.
- [x] `add-folder-form.tsx` -- Add `tokenVisible`, `noStoredToken` and `revealing` state; the eye
  toggle button; the "Show stored token" button beside "Clear token"; the `revealStoredToken`
  handler; blanking on clear and on save; and the constants
  `SYNC_TOKEN_SHOW_LABEL`/`SYNC_TOKEN_HIDE_LABEL`/`SYNC_TOKEN_REVEAL_LABEL`/
  `SYNC_TOKEN_NONE_STORED_LABEL`/`SYNC_TOKEN_READ_FAILED_PREFIX`. -- The whole user-facing change.
- [x] `add-folder-form.tsx` -- Rewrite `SYNC_TOKEN_NOTE` and `SYNC_TOKEN_EDIT_NOTE`, and every
  comment in the file that asserts a stored token cannot be read (the header doc, the client-import
  note, `formValuesFor`, the `storedToken`/`tokenCleared` state docs, the post-save comment). --
  The help text contradicted the feature; so did six comments.
- [x] `add-folder-form.test.tsx` -- Mock `syncGetCredential`; add tests for the eye round-trip, the
  three reveal outcomes, and clear-after-reveal; extend the "starts empty" test to assert
  `syncGetCredential` was never called. -- Locks the I/O matrix, including the AD-34-7 rule.

**Acceptance Criteria:**
- Given a token typed into the field, when the eye is pressed, then the input is `type="text"` and
  shows exactly what was typed, and the button reports `aria-pressed="true"`.
- Given an existing profile, when its edit form is rendered and the Advanced disclosure expanded,
  then the token field is empty and `sync_get_credential` has not been called.
- Given that same form, when "Show stored token" is pressed and the keychain holds one, then the
  field shows it unmasked.
- Given `Ok(None)`, when the reveal returns, then "No token is stored for this folder." is shown
  rather than an empty unmasked field.
- Given a rejected read, when the reveal returns, then the failure prefix and the error message are
  shown and no "none stored" line appears.
- Neither help string claims a stored token cannot be read.

## Design Notes

**Why the reveal fills the field rather than a read-only display.** The field is the only place the
token is edited, and a reveal is nearly always the prelude to checking or correcting it. Filling it
also makes the eye coherent: one control governs one value, whatever its provenance. The cost is
that a revealed-then-saved profile re-writes the identical secret to the same key, which is a no-op
in effect. The one case where it would not be a no-op is reveal-then-clear, which is why
`clearToken` now blanks the field: otherwise the very next save would silently restore what the
user just deleted. There is a test for exactly that.

**Why `revealing` is not `saving`.** `saving` disables the whole form. A keychain read can put an OS
authorisation prompt in front of the window, and greying out every field the user is mid-way through
filling is a worse outcome than a second reveal press. `revealing` therefore disables only its own
button.

**Why `Ok(None)` is not an error.** The port already documents it as ordinary
(`platform.rs:31-32`), and a profile on a public remote genuinely has no token. Collapsing it into
the error path would tell the user something broke when nothing did; collapsing it into "field is
empty" would make it indistinguishable from a failed read, since both leave the box blank.

**Button naming.** The eye is labelled for the press (`Show token` → `Hide token`) rather than for
the state, because a screen reader announces the label before the action happens; `aria-pressed`
carries the state. "Show stored token" is deliberately a different string from "Show token" so the
two controls are distinguishable by name alone.

## Verification

**Not run by me, deliberately:** no build, no linter, no formatter, no test suite. `cargo build`,
`cargo clippy`, `cargo fmt`, `bun run check` and `vitest` were all left to the parent agent, which
runs the full suite once after the whole Epic 34 batch lands. Five other agents were editing this
worktree concurrently, so a mid-flight build would have reported their in-progress state, not mine.

**What was verified by reading:**
- *No secret reaches the frontend on load.* `SyncProfileVm` (`sync_ipc.rs:50-72`) declares no token
  field, and `impl From<&SyncProfile> for SyncProfileVm` (`:74-95`) copies fifteen named fields,
  none of them a secret and none of them derived from `secret_key()`. `sync_profiles` (`:457-463`)
  is `profiles.iter().map(SyncProfileVm::from).collect()` and calls nothing else;
  `sync_profile_save` (`:479-497`) returns the same mapper's output. Grepping this file for
  `secret_key` finds exactly three uses: `sync_set_credential`, the new `sync_get_credential`, and
  `sync_clear_credential` — all three reached only by their own `#[tauri::command]`. On the TS side,
  `formValuesFor` (`add-folder-form.tsx`) seeds `token: ""` unconditionally, `EMPTY_FORM` does the
  same, and `syncGetCredential` appears in exactly two places in the file: the import and the body
  of `revealStoredToken`, which is called only from the reveal button's `onClick`. There is no
  effect hook in the component at all.
- *The port method exists under the name the epic assumed.* `SyncPlatform::secret_get(&self, key:
  &str) -> Result<Option<String>>` at `keeper-sync/src/platform.rs:33`, with `Ok(None)` documented
  as "no such secret ... an ordinary state". No implementor needed changing.
- *The new command matches its neighbours structurally.* Same `state`/`id` parameter pair, same
  `profile_by_id` lookup, same `crate::sync::sync_platform(Arc::clone(&state.platform))`, same
  `.map_err(|err| sync_ipc_error(&err))` tail as `sync_set_credential` two functions above it.
- *No second registry to update.* Grepping `src-tauri` and `src` for `sync_set_credential` /
  `sync_clear_credential` finds only `lib.rs`'s invoke handler, `sync_ipc.rs` and `client.ts`; there
  is no capability file or docs page enumerating sync commands. `Option<String>` needs no ts-rs
  type, so nothing under `src/lib/ipc/gen/` changes.
- *Both files still parse.* Re-read `add-folder-form.tsx` and `add-folder-form.test.tsx` through the
  tree-sitter structural view after editing; both resolve to a complete declaration list with no
  unterminated nodes.

**Commands the parent should run:** `bun run check` (biome + tsc + vitest, including the six new
tests in `add-folder-form.test.tsx`) and `cargo clippy --workspace`.
