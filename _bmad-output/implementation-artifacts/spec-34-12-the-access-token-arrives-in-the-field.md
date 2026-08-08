---
title: 'The access token arrives in the field'
type: 'feature'
created: '2026-07-29'
status: 'done'
baseline_revision: '88452c1'
final_revision: '1ef0854eb2eda88a9036f7c5aa74e7216623decf'

---

<intent-contract>

## Intent

**Problem:** Story 34.4 shipped a token field with three controls — an eye toggle over the input, a
"Show stored token" button that fetched from the keychain on press, and a "Clear token" button that
deleted the keychain item directly. The owner ran it and asked for something different: fill the
field with the stored token by default, drop both buttons, and keep the eye as the only way to see
it. The two extra buttons existed because the field opened blank, which forced the convention "an
empty field means keep whatever is stored" — and that convention is precisely what made a separate
Clear button necessary.

**Approach:** On mount, an edit form reads `syncGetCredential(profile.id)` and puts the answer in the
field, masked. "Show stored token" and "Clear token" are deleted. Emptying the pre-filled field now
means "remove it", which is only safe because the save no longer reads the field alone: a
`StoredToken` baseline records what the keychain answered when the form opened (`reading`, `stored`,
`absent`, `unreadable`), and `credentialWrite(baseline, field)` decides between one `set`, one
`clear`, and no call at all. Only a `stored` baseline licenses a removal, so a keychain that could
not be read cannot destroy a working credential by looking like a cleared field. A field
byte-identical to what was read produces no keychain call.

## This overrides a recorded decision

**AD-34-7 (Epic 34) says:** "A secret is revealable, never silently readable. … revealing a *stored*
token is a separate, explicit act that fetches from the keychain on demand and is never part of
loading the form. The token is not sent to the frontend when the edit form opens."

**The owner overrode it on 2026-07-29**, for their own machine, in these words: fill the field with
the stored token by default instead of having "Show stored token" and "Clear token" buttons, and
keep it as dots by default so it is revealed only by clicking the eye.

**The consequence, plainly:** the stored token now crosses the IPC boundary into the webview every
time an edit form opens. Before, it crossed only on an explicit press. Anything that can read the
webview's memory or its DOM — a devtools session, a renderer compromise, a screen reader, an
accessibility tool walking the tree — now sees the secret for any folder whose edit form has been
opened, whether or not the user asked to see it. Two properties of AD-34-7 survive: `SyncProfileVm`
still carries no token, so the profile *list* (read for every folder, on a poll) never carries one;
and the value is masked in the DOM's rendered form until the eye is pressed, which is a defence
against shoulder-surfing and screen sharing but not against anything running inside the webview.
AD-34-7 itself is left standing in the epic — this spec is the exception to it, not its repeal.

## Boundaries & Constraints

**Always:** The field is `type="password"` on every open; `tokenVisible` is mount-scoped state that
starts `false`, and every surface unmounts the form when it closes, so a reveal cannot survive into
the next open. The read is keyed on `profile?.id`, never on the `profile` object — the mirror store
hands this component a fresh object on each refresh, and a re-run read would overwrite what the user
had typed. The read seeds the field only while it is still empty. All three read outcomes get their
own note; a failed read additionally prints what the keychain said. The add form is unchanged: no
read, empty field, eye available. TS/Biome rules (no `any`, 2-space, 100-char, double quotes).

**Block If:** (none — `syncGetCredential`, `syncSetCredential` and `syncClearCredential` all exist
from Story 34.4 and Story 32.7, and no Rust change is needed.)

**Never:** Do not let an empty field mean "remove it" under any baseline except `stored` — that is
the whole safety property. Do not write a byte-identical token back. Do not add a token to
`SyncProfileVm`. Do not pass a token value to any logging call. Do not touch
`keeper-sync/src/git/repo.rs`, `keeper/src/ipc.rs`, `keeper/capabilities/desktop.json` or
`app-shell.tsx` (other agents' wave).

## I/O & Edge-Case Matrix

| Scenario | Baseline after the read | Field at save | Keychain call | Note shown |
|----------|------------------------|---------------|---------------|------------|
| Edit form opens, token exists | `stored("ghp_x")` | `"ghp_x"` (untouched) | **none** | `SYNC_TOKEN_EDIT_NOTE` |
| Edit form opens, token exists, eye pressed | `stored("ghp_x")` | shown as `type="text"`, value unchanged | none | `SYNC_TOKEN_EDIT_NOTE` |
| Reopened after a reveal | `stored("ghp_x")` | `"ghp_x"`, `type="password"` again | none | `SYNC_TOKEN_EDIT_NOTE` |
| Token replaced | `stored("ghp_x")` | `"ghp_y"` | `set(id, "ghp_y")` | `SYNC_TOKEN_EDIT_NOTE` |
| Field emptied | `stored("ghp_x")` | `""` | `clear(id)` | `SYNC_TOKEN_EDIT_NOTE` |
| Save again after a `set` | baseline moved to `stored("ghp_y")` | `"ghp_y"` | **none** | `SYNC_TOKEN_EDIT_NOTE` |
| Nothing stored | `absent` | `""` | none | `SYNC_TOKEN_NONE_STORED_NOTE` |
| Nothing stored, one typed | `absent` | `"ghp_new"` | `set` | `SYNC_TOKEN_NONE_STORED_NOTE` |
| Read failed | `unreadable` | `""` | **none — never `clear`** | `SYNC_TOKEN_UNREADABLE_NOTE` + `SYNC_TOKEN_READ_FAILED_PREFIX` + the keychain's message |
| Read failed, one typed | `unreadable` | `"ghp_typed"` | `set` | same |
| Save races the read | `reading` | anything | `set` if non-empty, else none | `SYNC_TOKEN_READING_NOTE` |
| Add form, token typed | `absent` (no read) | `"ghp_new"` | `set` on the minted id | `SYNC_TOKEN_NOTE` |
| Add form, field left empty | `absent` | `""` | none | `SYNC_TOKEN_NOTE` |
| Keychain write rejects | any | any | attempted | `SYNC_TOKEN_EDIT_FAILED_PREFIX` / `SYNC_TOKEN_FAILED_PREFIX`; form kept open (`onSaved(saved, false)`) |
| Keychain removal rejects | `stored` | `""` | attempted | `SYNC_TOKEN_REMOVE_FAILED_PREFIX` + message |

</intent-contract>

## Code Map

- `src/components/sync/add-folder-form.tsx` -- the token constants block, the new `StoredToken` /
  `CredentialWrite` types and `credentialWrite`, the `TOKEN_NOTES` map, the mount read effect, the
  save's credential leg, and the token field's JSX inside the Advanced disclosure.
- `src/components/sync/add-folder-form.test.tsx` -- the Vitest suite; the reveal and clear tests are
  replaced by the read-outcome and save-decision matrix.
- `src/components/settings/sync-section.test.tsx` -- `SYNC_TOKEN_CLEAR_LABEL` /
  `SYNC_TOKEN_STORED_LABEL` no longer exist; a surface-level test for the pre-filled edit form
  replaces the add-form Clear test.
- `src/components/layout/sync-pane.test.tsx` -- a default `syncGetCredential` answer, because this
  suite opens edit forms.
- `src/lib/ipc/client.ts` -- the doc comments on `syncSetCredential` and `syncGetCredential`, which
  both asserted AD-34-7 in prose.

No Rust changed. No generated IPC type changed — the read uses the `sync_get_credential` command
Story 34.4 added, and `Option<String>` needs no ts-rs type.

## Tasks & Acceptance

**Execution:**
- [x] `add-folder-form.tsx` -- Add `StoredToken`, `CredentialWrite` and `credentialWrite`, the pure
  decision the whole story turns on. -- One place states the rule, with the reason in its doc.
- [x] `add-folder-form.tsx` -- Replace `storedToken`/`tokenCleared`/`noStoredToken`/`revealing` state
  with `stored` (the baseline) and `readError`; keep `tokenVisible`. -- Four flags describing the
  same fact collapse into one discriminated union.
- [x] `add-folder-form.tsx` -- Add the `useEffect` that reads the credential on open, keyed on
  `profile?.id`, seeding the field only while it is empty and cancelling on unmount. -- The override.
- [x] `add-folder-form.tsx` -- Route the save's keychain leg through `credentialWrite`, add the
  `clear` branch and `SYNC_TOKEN_REMOVE_FAILED_PREFIX`, and move the baseline after a successful
  write. -- Without moving the baseline, every Save of an open form re-enters the keychain.
- [x] `add-folder-form.tsx` -- Delete `revealStoredToken`, `clearToken`, both "Clear token" buttons,
  the "Show stored token" button and the post-add acknowledgement block; a successful add now
  reports `settled = true`. -- The acknowledgement existed only to carry a Clear button; holding a
  form open for a line of text with no action would be scaffolding.
- [x] `add-folder-form.tsx` -- Rewrite the help text: `SYNC_TOKEN_NOTE` (add), and one note per read
  outcome for the edit form via `TOKEN_NOTES`. -- Story 34.4's "shows it again only when you ask" is
  no longer true for the edit form, and "no token is stored" must not be indistinguishable from a
  read that failed.
- [x] `client.ts` -- Correct both credential doc comments, which asserted the rule this story
  overrides. -- A doc that contradicts the code is worse than no doc.
- [x] `add-folder-form.test.tsx` -- Nine token tests covering the matrix above, with the negative
  assertions anchored on `onSaved` rather than on `syncProfileSave`. -- See Verification: anchoring
  on the profile save would have let a later keychain call slip past a passing test.
- [x] `sync-section.test.tsx`, `sync-pane.test.tsx` -- Default `syncGetCredential` to `null`, and
  swap the settings Clear test for a pre-filled-edit-form test. -- Every edit form now reads the
  keychain as it opens.

**Acceptance Criteria:**
- Opening Edit folder on a profile with a stored token shows a filled, masked field, and the eye
  reveals exactly the stored value.
- Reopening the form shows dots again; a previous reveal does not persist.
- Emptying the field and saving calls `sync_clear_credential`; a failed read followed by a save calls
  neither `sync_clear_credential` nor `sync_set_credential`.
- Saving without touching the field makes no keychain call at all — asserted by
  *"writes nothing to the keychain when the field still holds what was read"*.
- A folder with no stored token says so, and its empty field is not read as a removal.
- The add form is unchanged: empty field, eye available, no keychain read.
- No token value is passed to a logging call.

## Design Notes

**The three states, and how the code tells them apart.** The instruction was to work out how you
know which of "the user cleared it", "there was never one" and "the read failed" you are in, and
defend it. The field cannot tell them apart — all three render an empty box — so the discriminator
is not the field but the *answer the form opened with*, held in `stored: StoredToken`. `credentialWrite`
is the whole rule:

- `stored(v)` and `field === v` → **no call.** The untouched field. Re-storing an identical secret is
  a keychain write, and on macOS potentially a prompt, for no change.
- `stored(v)` and `field === ""` → **clear.** The field arrived carrying the token, so emptying it is
  a deliberate deletion. This is the only branch that deletes.
- `stored(v)`, anything else → **set.**
- `absent` / `reading` / `unreadable`, `field === ""` → **no call.** Under `absent` because there is
  nothing to remove; under the other two because the form never learned whether there is.
- `absent` / `reading` / `unreadable`, non-empty → **set.** A typed value is unambiguous under every
  baseline, and refusing it would make an unreadable keychain unwritable as well.

The asymmetry is deliberate. A keychain that refuses to answer produces exactly the same empty field
as a user who cleared it, and the two failure modes are not comparable: a refused deletion is
visible, retryable and costs one reopened form, while a silent deletion of a working credential is
invisible until the next sync fails to authenticate and is unrecoverable without the original token.
So the tie goes to not deleting, and the form says so out loud rather than leaving the user to infer
it — `SYNC_TOKEN_UNREADABLE_NOTE` states that saving will leave whatever is stored alone and that an
empty field will not remove it. The price, stated honestly: while the keychain cannot be read, a
token cannot be removed from this form.

**`reading` behaves like `unreadable`, not like `absent`.** A save fired before the read lands is a
real race — the Advanced disclosure can be expanded and submitted inside one round trip on a slow
keychain. Treating an in-flight read as "nothing stored" would make a fast click delete a token. The
decision is also taken *before* `saveSyncProfile` is awaited, so a read landing mid-save cannot
change what that save means.

**Why the baseline moves after a successful write.** Otherwise the second press of Save on a form
that is still open compares the field against the *original* answer and writes the same secret again
— and after a removal, tries to remove it twice. Only the edit form moves it; an add form has already
been reset to a blank draft for the next folder, whose baseline is correctly still `absent`.

**Why the post-add acknowledgement went with the Clear button.** `SYNC_TOKEN_STORED_LABEL` and its
`onSaved(saved, false)` existed for one reason, stated in the code it replaced: "Clear is the only
undo left once this form is gone." With Clear removed, the undo lives in the folder's edit form,
where the token is now visible and can be emptied. A line of text with no action, holding a form open
after a fully successful save, would have been scaffolding. An add that stores a token now reports
`settled = true` and the surface closes the form, which is what the add path already did when no
token was typed.

**Why the notes are per-outcome rather than one string.** The field looks identical under `absent`
and `unreadable`, and nearly identical under `stored` once masked. The note is the only thing that
distinguishes them, so it is not decoration — it is the report. `TOKEN_NOTES` is a
`Record<StoredToken["kind"], string>` so the compiler requires a note for every state.

**Why the read is keyed on `profile?.id`.** `profile` is an object prop supplied by the sync mirror
store, which produces a fresh object on every refresh. Depending on it would re-run the read on each
poll and clobber a half-typed token. The effect also guards with `abandoned` and seeds the field only
while `live.token === ""`, so an answer that arrives after the user has started typing is recorded in
the baseline but not painted over their input.

**Masking is mount-scoped, not reset.** `tokenVisible` is `useState(false)` and every surface renders
the form behind a `{editing && …}` / `{adding && …}` conditional, so closing the form unmounts it.
There is nothing to reset; the next open is a new component instance. `sync-pane.test.tsx` already
depended on this ("Reopening starts from the stored profile, not from the abandoned edit"), and the
new test asserts it for the reveal specifically.

## Verification

**Run, and green:**
- `bun run typecheck` — clean.
- `bun run lint` (`biome check .`) — 377 files, no diagnostics (one pre-existing schema-version info
  about `biome.json` pinning 2.5.2 against CLI 2.5.5; not mine, not touched).
- `bun run test` — 145 files, 1662 tests, all passing. The three suites I changed run 112 of them.

Rust was not built, linted or tested: this story changes no Rust, and `cargo build` for the `keeper`
crate does not work on this Linux box.

**The tests were mutation-checked, because two of them assert absence.** A test that asserts
"`syncSetCredential` was not called" passes trivially if it checks too early, so both mutations below
were applied to `credentialWrite` and the suite re-run:

- Removing the `field === stored.value` short-circuit → *writes nothing to the keychain when the
  field still holds what was read* and *replaces the stored token when a different one is typed over
  it* both fail. (Requirement 4 is genuinely defended.)
- Making the non-`stored` baselines treat an empty field as `clear` → *says no token is stored rather
  than letting an empty field imply one* and *will not read an empty field as a removal when the read
  failed* both fail. (Requirement 3/5 are genuinely defended.)

Both mutations were reverted and the suite re-run green. This is also why the negative assertions
wait on `onSaved` rather than on `syncProfileSave`: `syncProfileSave` resolves before
`refreshSyncDetail` and before the credential leg, so a `not.toHaveBeenCalled()` behind it would have
meant "not yet" rather than "never". `onSaved` is the last thing every save path does.

**Logging audit, as asked.** `src/components/sync/add-folder-form.tsx` was grepped for
`console.`, `logger`, `logEvent`, `trace(`, `debug(`, `info(`, `warn(` and `captureException`:
**no matches — the file contains no logging call of any kind.** The token therefore cannot reach one
from here. The three places the token value flows are `syncSetCredential(saved.id, credential.value)`
(the intended keychain write), the `value` prop of the `Input`, and the `stored` baseline in
component state. `syncErrorMessage(raw)` is only ever handed a rejection object, never the token;
`refreshSyncDetail(saved.id)` and `saveSyncProfile({…})` take an id and a `SyncProfileReq`, and
`SyncProfileReq` has no token field. The error strings composed in this file (`SYNC_TOKEN_*_PREFIX` +
`syncErrorMessage(raw)`) interpolate only the keychain's own message.

**Also verified by reading:** `SyncProfileVm` still declares no token field, so the profile list —
read for every folder and on a poll — carries no secret; only the per-folder `sync_get_credential`
command does, and only from this component's mount effect. `syncGetCredential` appears in
`add-folder-form.tsx` in exactly two places: the import and the effect body.
