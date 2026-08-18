# Spec 46.14 — Edit and delete outside the vault

story: 46.14
status: implemented; the macOS trash call and every `keeper` shell line are gate-pending
branch: `feat/epic-46-config-and-the-gaps`
binds: AD-102 (scopes AD-89, does not overturn it); NFR-30; FR-175, FR-176, FR-145, AD-65
carries forward: spec-45-3's directory refusal, unchanged and unwidened

## What the owner asked for, and what the sentence was actually saying

> *"I want to edit text files not necessarily in the notes folder."*
> *"I want to delete files that are not notes."*

keeper's answer was:

> AGENTS.md is outside tgdrive's notes vault (10-notes), and keeper only writes
> inside the vault it manages. You can open and reveal this file here; changing
> it is your file manager's job.

That sentence is AD-89's promise 1 read out loud, and it is not a bug. It has
teeth on both halves:

- every write went through `notes_vault::write_vault_file` + `mark_dirty`, and
  **`mark_dirty` is what makes a change reach the reconciler and the commit
  cadence.** A file outside a vault has no vault to mark.
- every delete went through `notes_vault::trash_note`, which moves the bytes
  into `<vault>/.keeper/trash/`. **NFR-30 forbids an `unlink`.** Outside a vault
  there is no vault trash to move into.

So "let me edit anywhere" is a **second writer** with different sync and
recovery semantics. AD-102's instruction is to build it and say what it costs.
That is what this story is: not a relaxed guard, a named second path.

## The correction this spec has to record before anything else

AD-102's own wording is *"a plain atomic write that no sync engine learns
about"*, and the brief repeats it as *"no sync, no commit, no conflict copy, no
history"*. **For the case the owner actually hit, half of that is false, and
shipping it as copy would have been a new lie replacing an old refusal.**

The file in the report is `AGENTS.md` inside `tgdrive` — a *sync profile*. The
folder engine watches the whole profile root (`engine.rs`'s `FolderWatcher` /
`watch_wake`) and `browse::classify` reports any non-excluded file in a
repository as `Synced`. An edit to `AGENTS.md` **is** committed and **does**
reach the other machine, exactly as an edit made in Finder would be. What is
genuinely absent is everything the *vault* provides.

So the surface says what is absent and refuses to overstate it:

> AGENTS.md is not one of keeper's notes — it is outside tgdrive's notes vault
> (10-notes). keeper saves it straight to the file and sends a delete to this
> computer's trash: no note history, no search index and no conflict copy.
> Nothing about how tgdrive syncs this folder changes.

`the_caveat_names_what_is_missing_without_overstating_it` asserts each absent
thing by name *and* asserts the sentence does not contain "will not sync" or
"does not sync". A caveat that overstates is a caveat people learn to ignore.

### Narrowed by Story 53.3, and this is the record of it

The rule above — *the standing fact is on screen before the first keystroke* —
**stands**. What Story 53.3 changed is how much of it stands there by default:
the band now shows ONE sentence and the four above are a press away on the
control beside them.

> AGENTS.md is not one of keeper's notes: no note history, no search index and
> no conflict copy.

Three things make that a narrowing rather than a deletion, and each is enforced:

- **The short form is composed in Rust**, beside the long one
  (`WriteScope::unmanaged_caveat_short`), and both are built from one
  `UNMANAGED_ABSENT` list so they cannot come to name different absences.
  `the_short_caveat_still_names_what_is_missing` asserts the head, the three
  absences, that it is one sentence, and that it drops only the qualifiers.
  The webview never clips the long one: `viewers/types.ts` forbids paraphrase
  and a character count would land inside the clause that names what is absent.
- **It is never folded to nothing.** `FilesWriteVm.caveat_short` is `Some`
  exactly when `caveat` is, and a frame handed a caveat with no short form keeps
  showing it whole (`keeps a caveat whole when its host carries no short form`).
- **M5's guard keeps its teeth.** `shows AD-102's caveat before the first
  keystroke, and only when there is one` now asserts the SHORT sentence in the
  band, still asserts each absence by name, and asserts that the short form is
  not a prefix of the long one — so a build that dropped the band, stopped
  naming what is missing, or truncated in TypeScript still fails.

## Where the line is drawn, since the brief asked

Three concentric cases, and only the middle one changed:

| where the file is | before | now |
|---|---|---|
| inside the notes vault | vault writer, vault trash | **unchanged** |
| inside the profile, outside every vault | refused | second writer, OS trash, caveat first |
| outside every sync profile | unreachable | **still unreachable** |

The third row is not a decision made by a guard — it is a decision made by the
address space. keeper names a file as `(profile id, profile-relative subpath)`;
there is no id to pass for a file in no profile, `sync_browse` never lists one,
and `resolve_existing` refuses anything whose *canonical* form leaves the
profile root, symlinks included. `a_path_outside_the_profile_is_refused_by_both_writers`
pins all three shapes (`..`, absolute, and a symlink out) against both a scope
with a vault and a scope without one.

**Directories stay refused at every location.** spec-45-3's argument — "one
confirmation over a folder holding 100 000 files is not a confirmation" — is not
weakened by the destination changing, so the `is_dir` check sits *outside* the
vault/unmanaged fork rather than in one arm of it. Mutation **M3** is exactly
the widening, and it is caught.

## The type-level separation (the brief's requirement 3)

Not a boolean, not a comment. The fork is one function and the two writers do
not share a signature.

```rust
pub enum WriteRoute<V> {
    Vault { vault: V, path: VaultPath },   // carries the caller's vault
    Unmanaged(UnmanagedPath),              // nowhere to put one
}

WriteScope::route<V>(&self, vault: Option<V>, root: &Path, subpath: &str)
    -> Result<WriteRoute<V>, WriteRefusal>
```

- `VaultPath` — private `String`, no `From`, **one constructor**: `route`, which
  can only reach it through a scope that already holds a vault subfolder. Spent
  by `notes_vault::write_vault_file(&Vault, …)` / `trash_note(&Vault, …)`, which
  already required a `Vault` and still do. The vault path is unreachable without
  a `Vault` — that was true before this story and is untouched by it.
- `UnmanagedPath` — private absolute `PathBuf` that `resolve_existing` already
  proved is a real descendant of the profile root after symlinks, plus the
  profile-relative string for the sentence. One constructor, same one. No public
  accessor for the absolute path (FR-145); the only two functions that need it
  live in the module.
- `V` is generic because `keeper-sync` has never heard of `notes_vault::Vault`
  (AD-40) and does not need to. Carrying the vault *inside* the variant is the
  load-bearing part: the vault arm cannot be reached without a vault because it
  holds one, and `write_unmanaged` / `trash_unmanaged` have no parameter a
  `Vault` fits.

`WriteScope::owner(subpath, is_dir)` is the same classifier without the disk,
for the listing. One `classify`, two callers — Story 45.3's rule is that the
flag a row renders and the answer the command gives are the same question asked
twice, and `the_listings_verdict_and_the_commands_route_never_disagree` runs
eight paths × two scopes through both and compares. It is lexical because
`route` canonicalises, and paying a `stat` per row to re-learn the `is_dir` that
`read_dir` just supplied would make opening a folder slower for nothing.

## The OS trash

`trash_unmanaged(&UnmanagedPath, &TrashTarget, local_now_ms) -> Result<PathBuf, WriteRefusal>`.

`TrashTarget` is an enum and not a path, because the two platforms answer in
different shapes:

- **macOS → `TrashTarget::Finder`.** `NSFileManager
  trashItemAtURL:resultingItemURL:error:`. It picks the right `.Trashes` for the
  volume the file is on and records the Put Back location; hand-rolling that is
  how a file on a pendrive ends up in the boot volume's trash.
- **everywhere else → `TrashTarget::Freedesktop(PathBuf)`.** `$XDG_DATA_HOME/Trash`,
  defaulting to `$HOME/.local/share/Trash`. Naming the directory is what makes
  the whole removal assertable over a temp directory *on this box* — a promise
  only macOS could check would be a promise nobody checks.
- **no home directory → `WriteRefusal::NoSystemTrash`.** A refusal, never a
  fallback to `unlink`. The only reason the second writer is allowed to delete
  at all is that the bytes stay reachable.

Freedesktop implementation notes worth keeping: the `.trashinfo` file is created
**first**, with `create_new`, and *that* is the lock — two deletions of two files
called `notes.md`, or one racing the desktop's own, and the loser takes the next
name instead of overwriting the winner's bytes. A move that fails takes the info
file back out with it. `Path=` is percent-encoded through `url` (already a
dependency) rather than a hand-rolled encoder, because the character set is
exactly what a hand-rolled encoder gets subtly wrong and getting it wrong means
a Restore that puts the file back somewhere else. Cross-device rename falls back
to copy-then-remove, the same shape `trash_note` already uses.

### The dependency, said loudly

**`objc2-foundation`, macOS-only, MIT.**

```toml
# keeper-sync/Cargo.toml — the FIRST platform-conditional dep in this crate
[target.'cfg(target_os = "macos")'.dependencies]
objc2-foundation = { workspace = true }
```

- **Adds no new crate and no new crate version.** objc2-foundation 0.3.2 /
  objc2 0.6.4 are already in `Cargo.lock` (tao/wry/muda), and the `keeper` crate
  already names both for the iOS backup-exclusion FFI. Only the edge is new,
  plus one feature: `NSFileManager` added to the workspace catalog's existing
  `["NSURL", "NSError", "NSString", "NSValue"]`.
- **No `unsafe`.** `trashItemAtURL:resultingItemURL:error:` and
  `NSFileManager::defaultManager()` are *safe* objc2 bindings, like the
  `objc2-user-notifications` precedent and unlike `exclude_from_backup`.
  Nothing is added to the audited-FFI inventory in
  `docs/constraints-and-limitations.md`.
- **AD-40 is intact.** `keeper-syncd` must keep linking this crate on a headless
  Linux server; a macOS-gated edge does not disturb that, an unconditional one
  would have. It does change what `cargo tree` / `cargo deny` show per host, and
  that is the first time that has been true of this crate.

## Two sentences that had to be rewritten, not just added

**1. The delete confirmation's `recovery`.** It said *"keeper moves it into the
vault's trash rather than erasing it, and the removal is recorded in this
folder's history."* For a file no vault holds, **both halves are lies.**
`FilesDeletePlanVm::compose`'s `files` argument therefore gained a per-file
`FilesDeleteDestinationVm`, because one drag over a vault and the folder beside
it selects both and wording the commoner one is how a confirmation becomes a
lie. Three arms: all-vault (unchanged wording), all-system, and mixed, which
counts both.

**2. `NoVault` / `OutsideVault`'s own sentences.** After this story those two are
reached **only from the create path** (`directory`, `create`) — an existing
out-of-vault *file* is now routed, not refused. So they say what is still
refused, and the phrase the owner quoted back at us ("changing it is your file
manager's job") is gone from the tree. Creating remains vault-only; see
"deliberately NOT done".

A third refusal, `VaultUnreachable`, is new and deliberately distinct from
`NoVault`: it is "the scope says in-vault and the caller handed no vault", i.e.
the two answers `vault_and_scope` exists to keep identical having come apart
(a profile configured with a vault the registry has no live slot for, mid-start).
Reported, never downgraded — downgrading it writes a note through the unmanaged
path.

## I/O matrix

`scope = WriteScope::new("Vault", Some("10-notes"))` over a profile root holding
`10-notes/`, `10-notes/daily/`, `10-notes-archive/`, `photos/`, `AGENTS.md`.

| call | input | output |
|---|---|---|
| `route` | `10-notes/Report.md`, vault live | `Vault { vault, path: "Report.md" }` |
| `route` | `10-notes/daily/Mon.md` | `Vault { …, path: "daily/Mon.md" }` |
| `route` | `AGENTS.md` | `Unmanaged("AGENTS.md")` |
| `route` | `10-notes-archive/old.md` | `Unmanaged` — component match, not `starts_with` |
| `route` | `photos/a.png` | `Unmanaged` |
| `route` | any of the above, scope with **no** vault | `Unmanaged` |
| `route` | `10-notes` | `Err(VaultRoot)` |
| `route` | `photos`, `10-notes/daily`, `10-notes-archive` | `Err(IsDirectory)` |
| `route` | `../etc/passwd`, `/etc/passwd`, `10-notes/../../etc` | `Err(Escapes)` (with and without a vault) |
| `route` | `escape` → symlink out of the root | `Err(Escapes)` |
| `route` | `gone.md` | `Err(Missing)` |
| `route` | `10-notes/Report.md`, vault `None` | `Err(VaultUnreachable)` |
| `owner` | every row above | identical verdict, no disk |
| `write_unmanaged` | `"after\n"` | file is `after\n`; **zero** `.keeper.*.tmp` left |
| `write_unmanaged` | `""`, `"no newline"`, `"\r\nwindows\r\n"` | byte-exact, no normalisation |
| `trash_unmanaged` | `AGENTS.md`, freedesktop temp trash | `…/Trash/files/AGENTS.md` holding the original bytes; original gone; `…/Trash/info/AGENTS.md.trashinfo` = `[Trash Info]\nPath=<pct-encoded abs>\nDeletionDate=2026-03-31T23:33:20\n` |
| `trash_unmanaged` | second `AGENTS.md` | `…/files/AGENTS.2.md`; the first is byte-identical and untouched |
| `trash_unmanaged` | `my notes/a b.md` | `Path=…/my%20notes/a%20b.md` |
| `unmanaged_caveat` | `AGENTS.md`, vault at `10-notes` | names the vault; lists history/index/conflict-copy; never says "sync" |
| `unmanaged_caveat` | `clip.txt`, no vault | "Field holds no notes vault"; same three absences |
| `os_trash` | this box | `Freedesktop(<abs>/Trash)` off macOS, `Finder` on it |
| `FilesDeletePlanVm::compose` | 1 loose file | "…this computer's trash…", no "vault's trash", no "folder's history" |
| `FilesDeletePlanVm::compose` | 2 notes + 1 loose | "Nothing is erased: 2 of these 3 go to the vault's trash …, and the other 1 go to this computer's trash, because they are not notes." |
| `FilesWriteVm` | vault file | `{ writable: true, reason: null, caveat: null }` |
| `FilesWriteVm` | loose file | `{ writable: true, reason: null, caveat: <sentence> }` |
| `FilesWriteVm` | folder outside vault | `{ writable: false, reason: <IsDirectory>, caveat: null }` |
| `TextFileFrame` | `writeCaveat` non-null | `<p role="status" data-testid="text-file-caveat">` **above** the error banner, editor still mounted |
| `TextFileFrame` | `writeCaveat` null | no such element |

## Edge cases

- **`10-notes-archive` beside `10-notes`.** Component-by-component, never
  `starts_with` on the string. Inherited from `vault_relative` and re-asserted
  through `route`, because the consequence changed: it used to be a refusal and
  is now a *routing* decision, and getting it wrong now means a vault file
  taking the plain writer.
- **The vault root itself** is `VaultRoot` and not `IsDirectory`: it is a
  directory, but the next step differs ("use Finder" vs "no, not this one"), so
  the more specific refusal is checked first.
- **An escape in a profile with no vault.** `vault_relative` tests for a vault
  *before* it tests the path, so it answers `NoVault` to `../etc` — and `route`
  maps `NoVault` to "the second writer's business". Containment therefore has to
  be established before the fork, which `resolve_existing` does. I wrote an
  explicit `plain_segments` pre-check as belt-and-braces, mutation-tested it
  (**M4**), found it killed nothing, and **removed it**; the ordering carries the
  guarantee and the doc comment now says so instead of claiming a false reason.
- **A trash name collision** picks `AGENTS.2.md`, not `AGENTS.md.2` — a trashed
  Markdown file should still be Markdown to everything that reads it.
  `.gitignore` has no extension to go before and becomes `.gitignore.2`.
  Bounded at 1 000 attempts, then an `AlreadyExists` naming the file.
- **A failed `write_unmanaged` rename** removes the temp. It is tier-0 excluded,
  so it would never be committed — but it is still litter in the owner's folder.
- **`mark_dirty` after a mixed delete** fires only if a *vault* removal actually
  succeeded. A batch of loose files marks nothing; a batch that trashed nothing
  logs "delete removed nothing" at `info!` (DW-162).
- **`sync_delete_entries` resolves the trash once per batch**, not per file:
  `os_trash` reads the environment and a machine with no home gives the same
  refusal every time.
- **The `is_dir` the plan used to carry.** `DeleteTarget.is_dir` was passed to
  `browse::status_of`, and it could only ever be `false` there — `scope.file`
  had already refused every directory. It is now the literal `false` with a
  comment, and `DeleteTarget`/`deletable` are deleted rather than left as a
  constant dressed as a fact.

## Mutation table

Sentinel `MUT46-14`; one at a time; each restored and the restoration verified by
reading the region back and re-running. `grep -rn MUT46-14 src src-tauri` exits 1.

| # | mutation | expected to die | result |
|---|---|---|---|
| M1 | `route` falls through to `WriteRoute::Unmanaged` for an in-vault path (the two writers merged) | `a_vault_file_is_never_routed_to_the_plain_writer` | **caught** — that test **+** `an_in_vault_path_with_no_vault_in_hand_is_refused_rather_than_downgraded` (29 passed / 2 failed) |
| M2 | `freedesktop_trash` erases the file instead of moving it (NFR-30 broken) | `an_out_of_vault_delete_lands_in_the_os_trash_and_never_unlinks` | **caught** — that test **+** `a_second_file_of_the_same_name_gets_its_own_place_in_the_trash` (29/2) |
| M3 | `is_dir` refused only inside the vault (spec-45-3 widened to the OS trash) | `a_directory_is_refused_inside_and_outside_the_vault` | **caught** (30/1) |
| M4 | the redundant `plain_segments` pre-check removed from `route` | *nothing* | **survived — deliberately.** The check was dead: `resolve_existing` already refuses every escape before the vault question. Removed for real and the doc comment corrected; the surviving mutant is the finding. |
| M5 | `TextFileFrame` renders no caveat banner | `shows AD-102's caveat before the first keystroke` | **caught** (1 failed / 22 passed) |

M5's guard was **re-anchored by Story 53.3** to the narrowed rule (see *Narrowed
by Story 53.3* above): the band it looks for now carries Rust's one-line form and
the four-sentence one is a press away. The mutation it dies on is unchanged —
`TextFileFrame` rendering no caveat band — and two more now kill it as well:
rendering the short form as a clip of the long one, and folding the band to
nothing when a host carries no short form.

M4 is why the mutation sweep was worth running rather than reporting: it removed
nine lines of code and one paragraph of comment that stated a rationale the code
did not have.

## What I could not verify here, and why

**The `keeper` shell crate does not link on Linux.** `cargo check -p keeper` was
attempted once and dies in `gobject-sys`'s build script (no `pkg-config`, no
GTK/webkit on this box), so **every line I changed in
`src-tauri/crates/keeper/src/sync_ipc.rs` is unproven** — the routing in
`sync_write_entry`, `sync_delete_plan`, `sync_delete_entries`, the new
`routable_profile`, `destination_of`, `WriteOutcome`, and the three-way listing
verdict in `files_listing_vm`. So is `finder_trash`, which is `#[cfg(target_os =
"macos")]` and therefore not even parsed here.

Everything with a decision in it was pushed into `keeper-sync` for exactly this
reason: `sync_ipc.rs` now *spends* a verdict and never reaches one, and
`finder_trash` is ten lines with no branch a test could take.

`src/test/command-registration.test.ts` passes (3/3) — I added no command, so
that gate is satisfied on this box.

### Ordered gate checks, on the macOS host

1. **`cargo build -p keeper`** — the objc2 call is the risk. If it fails it will
   be in `finder_trash`: the exact API is
   `NSFileManager::defaultManager().trashItemAtURL_resultingItemURL_error(&url, Some(&mut landed)) -> Result<(), Retained<NSError>>`
   (objc2-foundation 0.3.2, safe binding, features `NSFileManager`/`NSURL`/`NSError`/`NSString`).
   `NSURL::fileURLWithPath_isDirectory` is the idiom `keeper/src/ipc.rs:961`
   already uses.
2. **`cargo clippy -p keeper --all-targets`** and **`cargo test -p keeper`**.
3. **`cargo deny check`** — new macOS-only edge on `objc2-foundation` (MIT).
   `cargo tree -p keeper-sync` differs by host now.
4. **Open the Files pane on a profile with a notes vault. Select a file inside
   the vault.** No caveat banner. Save. It commits on the notes cadence exactly
   as before. This is the regression check that matters most.
5. **Select `AGENTS.md` (or any file beside the vault).** The editor opens with a
   standing muted sentence above it, before you type, naming the vault it is
   outside of and the three things it does not get. Type; press Save; the bytes
   on disk change. `ls -a <profile>` shows **no** `.keeper.*.tmp`.
6. **Delete `AGENTS.md`.** The confirmation says "this computer's trash", does
   **not** say "vault's trash" or "this folder's history". Confirm. Open Finder →
   Trash: the file is there, and **right-click → Put Back offers the original
   folder.** That last clause is the one thing only macOS can prove — the Linux
   test asserts the freedesktop `Path=` line, which is the same fact by a
   different mechanism.
7. **Select one note and one out-of-vault file together and delete.** The
   confirmation names both destinations and counts them. Afterwards: the note is
   in `<vault>/.keeper/trash/`, the other file is in Finder's Trash.
8. **Select a folder — inside the vault and outside it.** Both still refused, by
   name, as a folder.
9. **A profile with no notes vault at all.** Files in it are now editable and
   deletable; the caveat says "<profile> holds no notes vault". The **New file**
   control is still absent, and its sentence now reads "…will not create a new
   file in it. Files already there can still be edited and deleted."
10. **Delete a file on an external volume** (the one case where the trash is on
    another filesystem). It should still land in the Trash — macOS handles this
    itself via `trashItem`; the copy-then-remove fallback is the freedesktop
    path only.

## Deliberately NOT done

- **Removing `WriteScope::file`, which this story left with no production
  caller.** `files_listing_vm` asks `owner` now and `deletable` is deleted, so
  `file` is reached only from its own six tests. That is worse than dead code —
  it answers `Err(OutsideVault)` for exactly the paths AD-102 routes to the
  second writer, so it is the pre-fork mental model still sitting in the module
  looking plausible. It is **DW-197**, with the migration written out, because
  the deletion is not the mechanical part: six existing tests assert refusals
  through it that are now routing decisions, and each needs its intent
  re-expressed rather than repointed.
- **Creating outside the vault.** AD-102 names editing and deleting. A create
  needs the collision rule, the name rules and a directory to create *in*, and
  "which folder does New file appear on" is a surface question this story did
  not ask. `sync_create_entry` still opens with `writable_profile` and still
  refuses; its two refusal sentences were reworded to say only that.
- **Repairing `AD-102`'s own wording.** The epic spine says "no sync engine
  learns about it"; that is inaccurate for a file inside a sync profile and this
  spec records why rather than editing the spine mid-wave.
- **A `topdir` (`.Trash-$uid`) freedesktop trash.** The spec permits the home
  trash with a copy for cross-filesystem cases, which is what is implemented.
  A per-volume trash would avoid the copy on external drives on Linux; keeper's
  Linux surface is the headless daemon, which does not delete files.
- **Making `notes_vault::write_vault_file` take `&VaultPath`.** It would tighten
  the vault side further, but it has four callers in `notes_ipc.rs` — a file two
  other agents are in this wave, and one that does not compile here. The vault
  path already requires a `Vault`, which is the promise the brief asked to keep.
- **Repairing the delete-plan's `is_dir` for folders.** Folders never reach the
  plan; see edge cases.

## Files changed

**`keeper-sync` (compiles and is tested here):**

- `src/files_write.rs` — module doc records AD-102 and what it does *not* widen;
  `WriteRoute<V>`, `VaultPath`, `UnmanagedPath`, `WriteOwner`, `TrashTarget`;
  `WriteScope::route` / `owner` / `classify` / `unmanaged_caveat`;
  `write_unmanaged`, `os_trash`, `local_now_ms`, `trash_unmanaged`,
  `freedesktop_trash`, `finder_trash`, `numbered`, `encoded_path`,
  `deletion_date`; `WriteRefusal::NoSystemTrash` and `::VaultUnreachable`;
  `NoVault`/`OutsideVault` reworded for the create path. **+14 tests (33 total).**
- `src/platform.rs` — `civil_from_unix_ms` (one copy of Hinnant's algorithm, now
  that two modules format an instant); `machine_utc_offset_minutes` is
  `pub(crate)`.
- `src/engine.rs` — `conflict_stamp` is now a formatter over
  `civil_from_unix_ms`; its own test still passes unchanged, which is what
  proves the extraction was behaviour-preserving.
- `Cargo.toml` — the macOS-only `objc2-foundation` edge, with its why/licence/
  adds-no-new-crate comment, appended after `toml`.

**`keeper-core` (compiles and is tested here):**

- `src/vm.rs` — `FilesWriteVm.caveat` + `FilesWriteVm::unmanaged`;
  `FilesDeleteDestinationVm`; `FilesDeletePlanVm::compose` takes a per-file
  destination and the `recovery` sentence has three arms. **+1 test, 12 call
  sites migrated through new `note()`/`loose()` helpers.**

**workspace:**

- `src-tauri/Cargo.toml` — `NSFileManager` added to the objc2-foundation feature
  list; the catalog comment now names both consumers.

**`keeper` shell (unverifiable here — see gate checks):**

- `src/sync_ipc.rs` — `routable_profile`, `destination_of`, `WriteOutcome`;
  `sync_write_entry` / `sync_delete_plan` / `sync_delete_entries` route;
  `files_listing_vm`'s three-way verdict; `deletable` + `DeleteTarget` deleted.

**frontend:**

- `src/lib/viewers/types.ts` — `ViewerFile.writeCaveat`.
- `src/components/layout/panel-strip.tsx` — one line, `writeCaveat: entry.write.caveat`
  (coordinated with W3Files, who owns the file).
- `src/components/viewers/text-file-viewer.tsx` — passes it through.
- `src/components/viewers/text-file-frame.tsx` — `writeCaveat` prop,
  `TEXT_FILE_CAVEAT_TESTID`, banner above the error and outside the `savable`
  gate.
- `src/lib/ipc/gen/FilesWriteVm.ts` — regenerated by ts-rs.
- Fixtures: `document-viewer`, `media-viewer`, `text-file-viewer`,
  `components`, `unknown-viewer`, `registry` (`writeCaveat: null`),
  `export-controls` (`caveat: null`). **+2 tests.**

## Gate results on this box

- `cargo test -p keeper-sync --lib files_write::` — **33 passed, 0 failed,
  exit 0.**
- `cargo test -p keeper-sync --lib conflict_stamp` — 1 passed (the extraction is
  behaviour-preserving).
- `cargo test -p keeper-core --lib vm::` — 304 passed, 0 failed.
- `cargo clippy -p keeper-sync --lib --all-targets` — clean.
- `cargo clippy -p keeper-core --lib --all-targets` — clean.
- `bun run test src/components/viewers/ src/lib/viewers/ src/components/export/export-controls.test.tsx`
  — 17 files, **324 tests, exit 0.**
- `bun run test src/components/layout/panel-strip.test.tsx src/test/command-registration.test.ts`
  — 2 files, 19 tests, exit 0.
- `bunx tsc --noEmit` — clean apart from two pre-existing `String.replaceAll`
  errors in `src/test/capture-capability.test.ts` (W3Capture-2's in-flight edit,
  reported to them).
- Formatter and repo-wide linter deliberately not run (Main runs them once at
  the end).
