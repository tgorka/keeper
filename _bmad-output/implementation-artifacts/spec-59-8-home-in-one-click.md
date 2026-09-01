---
title: 'Story 59.8: home, in one click'
type: 'feature'
created: '2026-08-31'
status: 'done'
baseline_revision: '925bdf4'
final_revision: 'd2493d7'
review_loop_iteration: 0
followup_review_recommended: false
context:
  - '{project-root}/docs/project-context.md'
---

<intent-contract>

## Intent

**Problem:** The owner asked, in one line, *"for the folder add option of home dir"*, and the
epic-59 triage chose the picker reading of it (`epic-59:34`, `:220`): a task addresses a profile or
the host, never a path, so this is Add-folder's ask and not the Tasks pane's. Today Add-folder has
exactly one way to name a folder — the native directory dialog at `add-folder-form.tsx:1208` — and
the path it returns is rendered into a read-only `<p>`. There is **no** typed entrance, so no way to
name a folder by writing it; **nothing anywhere in the tree expands `~`** (confirmed by grep over
`src-tauri/crates` and `src`: no tilde-expanding call anywhere, and the only `~` handling in the
frontend is Markdown fence and subscript parsing); and reaching home through the dialog is several
clicks into a directory macOS hides by default. A profile's `local_path` must be absolute
(`keeper-sync/src/profile/mod.rs:1158`) but nothing forbids it being `$HOME`.

**Approach:** Three changes in one screen, and no new command, no new wire field, no schema.
1. The add form's folder becomes a **typed box beside the picker**, which is what makes `~`
   reachable at all: a directory dialog has no way to say *"the folder I mean is the one I can
   name"*. Editing is untouched — still a read-only line, still `SYNC_PATH_FIXED_NOTE`, because the
   engine binds a profile to its folder.
2. A **Home** control beside `Choose folder` fills that box with the resolved home directory (not a
   `~`), and a typed `~` / `~/sub` resolves through one exported pure function,
   `syncExpandHome(raw, home)`. Home itself comes from the shell — `homeDir()` from
   `@tauri-apps/api/path`, i.e. Rust's own `dirs::home_dir()` — read once per open into one piece of
   state that the control, the resolution and the warning all share.
3. One sentence, `SYNC_HOME_FOLDER_WARNING`, rendered when the resolved path **is** home, naming
   what that pulls in: caches and databases, `~/Library` or `~/.config`, `~/.ssh` and `~/.gnupg`,
   every `node_modules`, every VM image — and that the secrets among them would be pushed to the
   remote. It points at `Skip these files` under `Advanced options`, which is the box that solves it.

The Rust half of this story is a **guard test and no production change** — the story's one Rust place
spent on pinning the check the browser-side decision leans on. The argument is in the Design Notes
under *Why `~` is expanded in the browser and not in Rust*, because it is the decision a later reader
is most likely to re-litigate.

## Boundaries & Constraints

**Always:**
- A literal `~` must not reach the store. Held by `validate`'s absolute check, now pinned by
  `an_unexpanded_tilde_can_never_be_stored_as_a_folder_name` for `~`, `~/notes` and `~alice/notes`.
- Home is a **legal** folder: nothing refuses it, nothing disables the submit, no confirmation
  dialog. The epic's acceptance is a sentence, not a gate.
- `~alice/...` is left **exactly as typed**. This layer cannot know another user's home, and
  resolving it to *this* user's would sync a different folder from the one named.
- No fallback home. If the shell cannot answer, the Home control is disabled, `~` does not expand
  and the warning does not render — `/home/$USER` is wrong on macOS, and a plausible-but-wrong home
  is the one failure nobody would notice.
- The form sends the **resolved** path, so the line under the box and the row in the store are the
  same string.
- Edit mode keeps its shape byte for byte: no textbox, no picker, no Home, and the fixed-path note.
- Nothing else about a path is normalized here — no `..` collapsing, no trailing-slash trimming, no
  symlink resolution. Those are the engine's, and a second opinion would be a second answer.

**Block If:** the home directory could only be obtained by adding a command to the `keeper` shell
crate (which cannot link on this host, so it could not be proved), or if resolving `~` in the
browser would let a relative path bypass a Rust check rather than merely arrive as a refusal.

**Never:** touch `src/components/layout/**` (`tasks-pane.tsx`/`.test.tsx` are `Main`'s for 59.1, and
this story needs nothing there); add a `~`-expanding call inside `keeper-sync`, `keeper-syncd` or the
shell crate; make a home folder refusable; re-implement `validate`'s absolute rule in TypeScript;
add an IPC command or a capability grant.

## I/O & Edge-Case Matrix

| Scenario | Input / State | Expected Output / Behavior | Error Handling |
|----------|--------------|---------------------------|----------------|
| Home in one press | add form, shell says `/Users/alice` | the box holds `/Users/alice`; the line under it says the same | No error expected |
| Typed `~/notes` | add form | the line reads `/Users/alice/notes` **before** the press; the save sends that, never `~/notes` | No error expected |
| Typed `~` | add form | resolves to `/Users/alice`; the warning appears | No error expected |
| Typed `~alice/notes` | add form | left literal on screen and on the wire; Rust answers `local path must be absolute, got ~alice/notes`, rendered in the form | Rust's refusal, verbatim |
| Folder inside home | `~/notes` or `/Users/alice/notes` | **no** warning; a sentence that fires on the common answer is one nobody reads on the rare one | No error expected |
| Warning is not sticky | home chosen, then changed to a subfolder | the sentence disappears | No error expected |
| Shell cannot say where home is | `homeDir()` rejects | Home disabled; `~/notes` stays literal and is refused by Rust; no console noise and no visible error | Swallowed by design |
| Existing home folder | edit form whose stored path is home | the sentence renders; still no textbox, no picker, no Home | No error expected |
| Picker still primary | `Choose folder` | unchanged: absolute path, cancellation writes nothing | unchanged |
| Any tilde reaching the write door | `~`, `~/notes`, `~alice/notes` on a `SyncProfile` | refused by `validate`, message quotes the path | a refusal, never a folder named `~` |
| Dev viewing aid | `bun run dev` | `plugin:path\|resolve_directory` answers `/Users/tgorka` for `BaseDirectory.Home` (21), `null` for every other directory | `null` reads as "the shell could not say" |

</intent-contract>

## Code Map

- `src/components/sync/add-folder-form.tsx` -- `SYNC_HOME_FOLDER_LABEL`,
  `SYNC_FOLDER_PLACEHOLDER`, `SYNC_HOME_FOLDER_WARNING`, `syncExpandHome` (exported, carrying the
  layer argument); `home` state and the `homeDir()` effect; `resolvedPath` / `homeChosen` derived
  once beside `virtualOverBandNote`; the folder row split into an edit branch (unchanged) and an add
  branch (typed `Input`, resolved line, Home, `Choose folder`); `localPath: resolvedPath` in the
  request; the header comment's local-path bullet.
- `src/components/sync/add-folder-form.test.tsx` -- `@tauri-apps/api/path` mocked; six tests in
  `AddFolderForm choosing a home directory (Story 59.8)`.
- `src-tauri/crates/keeper-sync/src/profile/mod.rs` -- **test only**:
  `an_unexpanded_tilde_can_never_be_stored_as_a_folder_name`, beside
  `relative_local_paths_are_rejected`.
- `dev/mock-shell.ts` -- one `HANDLERS` entry for `plugin:path|resolve_directory`, so the story is
  visible in `bun run dev` instead of showing a permanently disabled control that reads as a bug.
  **Not in this story's commit:** that file turned out to be shared by three stories at once, so
  `Main` carries it, and this hunk with it.

**Not touched:** no Rust production code, no `src-tauri/crates/keeper/**`, no wire type, no
generated binding, no capability file (`core:default` already carries `core:path:default`), nothing
under `src/components/layout/**`.

## Tasks & Acceptance

**Execution:**
- [x] Confirm by grep that nothing in the tree expands `~`, and that `validate`'s absolute check is
  the funnel every writer passes.
- [x] `add-folder-form.tsx` -- the three constants, `syncExpandHome`, the home read, the derived
  pair, the two-shape folder row, and the resolved path on the wire.
- [x] `add-folder-form.test.tsx` -- Home fills the box; `~/notes` resolves rather than being stored
  literally; `~alice/notes` stays literal and is refused; the warning is present exactly at home; no
  home known means no Home and no expansion; an existing home folder still says it.
- [x] `profile/mod.rs` -- the tilde guard test.
- [x] `dev/mock-shell.ts` -- the path-plugin fixture (carried by `Main`'s shared-files commit).

**Acceptance Criteria:**
- Given the add form, when Home is pressed, then the folder box holds this machine's home directory
  and the line under it agrees.
- Given `~/notes` typed, when the folder is saved, then the request carries `<home>/notes` and never
  `~/notes`.
- Given a `SyncProfile` whose `local_path` still begins with `~`, when it reaches any write door,
  then it is refused with the path quoted — a directory called `~` can never be created.
- Given a resolved path equal to the home directory, then the form says once what that pulls in;
  given any other folder, it says nothing.

## Design Notes

**Why `~` is expanded in the browser and not in Rust.** The invariant worth protecting is *a literal
`~` can never reach the store as a directory name* — and that invariant is **already held, in Rust,
by something the frontend cannot weaken**: `SyncProfile::validate` refuses a `local_path` that is not
absolute (`profile/mod.rs:1158`), `~/notes` is not absolute, and every entrance passes through it —
this form's IPC save, `keeper-syncd profile add`, a hand-edited `config.toml` through
`config::parse`, and `db::upsert_profile`, which validates before it writes *whatever route the
profile arrived by* (`db.rs:1546`). So expansion is not the guard. The guard exists, and the worst
case of an expansion that does not happen is a refusal quoting the path rather than a folder called
`~`.

What is left is a convenience, and it belongs to the layer where home is a fact about the **person**:
this app runs as them, so `homeDir()` is their home. Expanding inside `keeper-sync` would expand
against the `HOME` of whichever **process** performed the write, and `keeper-syncd` can run as a
different user from the one who edited its config — so the same `~` in the same file would mean
different folders depending on who saved it, and nothing about that would look wrong from either
side. An expansion whose meaning depends on the writer is worse than no expansion at all.

The daemon's own home lookup (`keeper-syncd/src/platform.rs:189`) is **not** a precedent for the
other direction, although it looks like one: it resolves the home of the process for the *process's
own* data directory, which is the one question that has a right answer there and no right answer
here. And the browser side costs nothing extra — the form has to know where home is anyway, for the
Home control and for the warning — so this is one fact used three times rather than a second source
of truth. The one Rust place this story was allotted is therefore spent on a **guard test**:
`an_unexpanded_tilde_can_never_be_stored_as_a_folder_name` pins the check the whole decision rests
on, so a future tidy-up of `validate` cannot quietly remove it.

**Why the add form gains a typed path at all.** The owner's ask has two halves and only one of them
is a button. A native directory dialog cannot express *"the folder I mean is `~/tgdrive`"* — it can
only be navigated there — and the epic's acceptance names a typed `~` explicitly. The typed box is
also the smaller of the two available changes: the alternative was a second "Home" entry inside a
picker keeper does not own. Edit mode is deliberately untouched, so the read-only path and its
explanation stay exactly as Story 32.7 left them.

**Why the resolved line stays on screen even when it repeats the box.** It is the only place a
resolved tilde becomes a fact somebody can check *before* pressing Add, and it is the surface three
test files already read (`sync-pane.test.tsx`, `sync-section.test.tsx` and this story's own all
assert on `SYNC_FORM_PATH_TESTID`). Keeping it is what let this story add a control without touching
a file it does not own.

**Why the warning is not `text-destructive`, and not muted either.** In this file
`text-destructive` means *a refusal* — a save that will come back with an error — and this save will
succeed, so wearing that colour would be a lie about what the button is about to do. Every other note
is `text-muted-foreground`, which is the colour of the twenty sentences nobody reads. It renders as
`text-foreground`: the one un-muted line in the form. `DESIGN.md` names a `warn` amber in the palette
it proposes, but no `--warn` token exists in `src/index.css` yet, and inventing a raw colour here
would be exactly the drift the design gate exists to stop.

**Why the sentence names things.** "Large and sensitive files" is something everybody agrees with and
nobody acts on. `~/.ssh`, `~/Library` and `node_modules` are things a person can go and look at, and
the last clause names the box that fixes it rather than pointing at documentation — the decision is
being made in this form, so its remedy has to be in this form.

**Why it renders in edit mode too.** The sentence is a claim about what is being synced, not about
the press. Somebody opening the form of a folder that already **is** home is the person for whom it
is currently true, and the form still offers no way to repoint the profile, so the sentence is the
only thing it can honestly say there.

## Verification

**Commands:**
- `bun run vitest run src/components/sync/add-folder-form.test.tsx` -- 54 passed / 54 (48 before).
- `bun run vitest run src/components/settings/sync-section.test.tsx` -- 37 passed / 37: the other
  surface that mounts this form and does **not** stub the path plugin, so it is the proof that an
  unanswerable `homeDir()` leaves a working form rather than a broken one.
- `bun run vitest run src/components/layout/sync-pane.test.tsx` -- 85 passed / 85, for the same
  reason. Read-only run; this story edits nothing under `src/components/layout/`.
- `cargo test -p keeper-sync --lib profile::tests::an_unexpanded` -- 1 passed. Run in a throwaway
  `git worktree` at `408fc6e` carrying only this story's hunk, because `keeper-sync` did not compile
  in the shared worktree at the time (a sibling's half-landed `TaskRow.missed_delay_ms`, agreed with
  its owner and since backed out).
- `bunx biome check` on the three frontend files -- clean.
- `bunx tsc --noEmit` -- no diagnostic in any file this story touches.

**Manual checks (if no CLI):**
- jsdom performs no layout, so no test here can see a control that left the screen. The add row is
  laid out so that it cannot: the two buttons sit in a `shrink-0` strip and the path line carries
  `min-w-0 truncate`, so the path yields and the controls do not. The `dev/mock-shell.ts` fixture
  exists so this row can be looked at in `bun run dev` without a Mac.

**Mutation proof.** Four guards mutated one at a time, restored and re-verified green after each:

| mutation | owning test that failed, and how |
|---|---|
| `syncExpandHome` returns `raw` always | `resolves a typed ~/notes instead of sending a folder literally called ~` — *Expected element to have text content: /Users/alice/notes; Received: ~/notes*; and the warning test, which could no longer find the sentence after typing `~` |
| Home's `onClick` writes nothing | `fills the folder with this machine's home directory in one press` — *expect(element).toHaveValue(/Users/alice) … Received: (empty)* |
| `homeChosen` widened to `resolvedPath.startsWith(home)` | `says what a home folder pulls in when home IS the folder, and nowhere else` — *expected document not to contain element, found `<p class="text-foreground text-xs">This is your whole home folder…`* after typing `~/notes` |
| `validate`'s `!local_path.is_absolute()` refusal disabled | `an_unexpanded_tilde_can_never_be_stored_as_a_folder_name` — *panicked … an unexpanded tilde must not reach the store: ()* |

## Review Triage Log
