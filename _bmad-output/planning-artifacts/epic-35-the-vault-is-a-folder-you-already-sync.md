# Epic 35 — The vault is a folder you already sync

status: draft
created: 2026-08-02
altitude: epic
parent: Phase 5 (Notes), Epic 23 (sync foundation), Epic 34 (sync you can see)
source: `product-inputs-notes-2026-08-02.md` (the numbering spine), the divergent session in
`brainstorm-keeper-notes-2026-08-02/`, and a full read of `keeper-sync`'s profile, exclusion
and watcher paths plus `keeper-core`'s module set
binds: FR-94–FR-97, FR-121, FR-122, NFR-28, AD-54–AD-57

## Why this epic exists

Every note app in the world begins by asking where to put things. This one must not, because
keeper already knows: the user has told it, once, by adding a folder to sync. A vault is that
folder plus a flag.

That is not a simplification for its own sake — it is the deletion of an entire settings
surface. No vault picker, no path validator, no "import your existing notes", no migration
story, no second configuration store to keep consistent with the first. The brainstorm named
this as the phase's structural win and warned, in the same breath, not to reintroduce any of
it. This epic is where that discipline is either held or lost.

Three facts about the codebase make the flag nearly free, and they are why this epic is six
stories and not sixteen:

1. **A sync element is already a `SyncProfile`**, and profiles persist as one JSON blob per
   profile in `sync.db`. A new `#[serde(default)]` field *is* the migration. There is no SQL
   to write, no schema version to bump, and an older build that reads a newer blob simply
   ignores a key it does not know.
2. **A watcher already exists per profile.** Epic 34 story 34.9 wired `watch::FolderWatcher`
   into the supervisor: a 1 Hz tick driving paced scans plus a `notify` watcher with a 500 ms
   debounce and a 15-minute backstop rescan. An external process writing a file inside a
   synced folder is *already* noticed. Notes needs to subscribe to that stream, not build a
   second one.
3. **`keeper-core` is provably pure**, and two CI gates say so: `check:core-tauri-free` and
   `check:core-sync-free`. The notes domain — frontmatter, naming, tags, links, templates, the
   space query language, the index model — belongs there precisely because it is the part with
   no I/O in it, and the gates will keep it honest without anyone remembering to.

### Where we take a position

**The index is not a database, and we will not let it become one.** The temptation is a
SQLite table beside `sync.db` with a `notes` schema, FTS5, and a migration path. We are not
doing that, for a reason that is measurable rather than aesthetic: a personal vault is
10 000 files at the top of the envelope, the whole frontmatter set is a few megabytes of
strings, and a bounded parallel scan answers a content query in tens of milliseconds. A
database buys nothing at that size and costs a consistency problem forever — every external
write by Obsidian, an agent or a `git checkout` becomes a cache-invalidation bug. So the index
is in-memory, rebuilt from disk, and persisted only as an advisory `<vault>/.keeper/index.json`
for cold start. A corrupt or absent cache is a rescan, never an error (AD-57). Deleting
`.keeper/` is a supported recovery procedure, and story 35.5's acceptance says so.

**"Added to the vault's ignore rules" means keeper's rules, not git's.** FR-121 requires that
`.keeper/` never syncs. The obvious implementation — write a `.gitignore` — is wrong here and
contradicts a decision already made: keeper authors no `.gitignore` (epic 34 story 34.9(d);
the engine writes only `.gitattributes`, and git stays the authority on ignore rules the user
owns). Keeper's own ignore mechanism is tier-0 exclusion, so `**/.keeper/**` joins
`BUILTIN_EXCLUDES` in `keeper-sync/src/exclude.rs` and the index is invisible to the commit
path, the pending list and the activity feed at once — which is exactly what AD-34-15 says an
excluded path must be.

**`.obsidian/` is not merely "not written". It is not read.** A coexistence promise that
depends on us being careful is not a promise. The scan skips the directory by name before it
opens anything inside it, and story 35.5 asserts that with an instrumented IO counter rather
than a code review.

## Stories

### Story 35.1: A Profile Can Say It Is a Vault
**Rust-only (`keeper-sync`).** Bindings: no.

Add `notes: Option<NotesConfig>` to `SyncProfile` (`keeper-sync/src/profile.rs`) behind
`#[serde(default)]`, with `NotesConfig { subfolder: String }` defaulting to `notes`, plus a
`SyncProfile::vault_root()` returning `local_path.join(subfolder)` and refusing a subfolder
that escapes the profile root. Extend `parse_req` (`keeper-sync`'s profile request path and
`keeper/src/sync_ipc.rs`) under the AD-34-9 rule — clone the prior profile as the base — so
flagging a vault cannot silently reset a knob the form did not show. `**/.keeper/**` joins
`BUILTIN_EXCLUDES` (`keeper-sync/src/exclude.rs`) with both a name rule and a subtree rule,
per that module's own doc contract.
AC: a `sync.db` blob written by 0.6.5 loads on the new build with `notes: None` and no error
line; setting the flag, restarting and reading it back yields the same subfolder; a subfolder
of `../evil` is rejected at construction with a typed `SyncError`, not at use; a file under
`.keeper/` never appears in `Engine::pending` and never enters a commit.

### Story 35.2: The Notes Capability, and the Flag in the Form
**Crosses the IPC boundary.** Bindings: **yes**.

`CapabilitiesVm` (`keeper-core/src/vm.rs`, beside `sync` at `:121`) gains `notes: bool`,
computed in the shell — notes requires folder sync, so it is `sync && desktop` and is `false`
on iOS, which is the whole of FR-122: the surface is absent, not dead. `DEFAULT_CAPABILITIES`
in `src/lib/stores/capabilities.ts` gains `notes: false` so a failed hydration cannot
advertise it. `SyncProfileReq`/`SyncProfileVm` (`keeper/src/sync_ipc.rs`) carry the flag and
the subfolder, and `src/components/sync/add-folder-form.tsx` gains a "This folder is a notes
vault" control with the subfolder field revealed only when it is on — showing the real default
`notes/` rather than a blank box (AD-34-8).
AC: `bun run bindings:check` passes with the regenerated `src/lib/ipc/gen/` tree; on an iOS
build every notes affordance is absent from the DOM, not disabled; flagging a folder in the
form and reopening it shows the flag and the subfolder that are actually in force.

### Story 35.3: `keeper_core::notes` — Frontmatter, Identity, Filenames
**Rust-only (`keeper-core`), pure.** Bindings: no.

New module `keeper-core/src/notes/` with `frontmatter.rs`, `id.rs` and `name.rs`. Frontmatter
parses and re-serialises YAML **preserving unknown keys and their order**, because the file
belongs to the user and Obsidian, not to us; keeper claims exactly the keys it documents
(`id`, `tags`, `pinned`, `archived`, and the space query key) and touches nothing else.
`id.rs` mints the ULID (`ulid` is already a `keeper-core` dependency) that makes links, pins,
unread marks and history survive a rename (FR-97). `name.rs` owns title→slug,
`YYYY-MM-DD-<slug>.md`, and the collision counter — the *rule*; the write is story 36.5. No
filesystem, no `gix`, no `tauri`.
AC: a note authored in Obsidian with six unknown frontmatter keys round-trips byte-identically
through parse→serialise; a note with no frontmatter gains a block without disturbing the first
body line; two notes titled "Meeting" on one day produce distinct filenames and distinct ULIDs;
`check:core-tauri-free` and `check:core-sync-free` stay green.

### Story 35.4: The Index Is a Model, and the Model Is Pure
**Rust-only (`keeper-core`), pure.** Bindings: no. Depends on 35.3.

`keeper-core/src/notes/index.rs`: `NoteRecord` (id, relative path, title, mtime, size,
frontmatter, tag set, outbound links), `VaultIndex` holding them by id with a path lookup, the
hierarchical tag tree with counts, and the link graph. Mutation is an `IndexDelta`
(upsert/remove by path) applied in place — because story 38.1 will feed it one changed path at
a time, and rebuilding 10 000 records to absorb one keystroke's worth of change is the failure
mode NFR-28 exists to forbid. Inputs are plain values: a relative path, bytes, an mtime. No
I/O in the crate that owns the model.
AC: building an index over 10 000 synthetic records completes within a stated budget recorded
in the bench; applying a single upsert updates the tag counts and the backlink set without
touching any other record, asserted by a mutation counter; removing the last note carrying a
tag removes the tag from the tree, and removing a leaf does not remove its parent when a
sibling survives.

### Story 35.5: Vault IO, the Scan, and a Cache Allowed to Be Wrong
**Rust-only (`keeper` shell).** Bindings: no. Depends on 35.1, 35.4.

New `keeper/src/notes_vault/` (`mod.rs`, `scan.rs`, `cache.rs`) — the AD-56 seam. The scan
enumerates the vault subfolder through `keeper_sync::exclude`'s `ExcludeSet` so keeper's
exclusion rules mean the same thing to notes as to sync, reads each file's frontmatter and
feeds `keeper_core::notes` plain inputs. `.obsidian/` is skipped **by name, before descent** —
never opened, never stat'd inside. The result persists as advisory `<vault>/.keeper/index.json`
carrying a schema version; a version mismatch, a parse failure or an absent file is a rescan
at `info` level, never an error to the user (AD-57).
AC: `rm -rf <vault>/.keeper` followed by a restart produces an identical index and one `info`
line; a vault containing an `.obsidian/` directory with 400 files records zero read or metadata
syscalls under that path, asserted by an instrumented IO counter in the test harness, not by
inspection; truncating `index.json` to half its bytes rebuilds silently; `.keeper/` appears in
no commit produced by the profile.

### Story 35.6: Many Vaults, One Watcher Budget, Ten Thousand Notes
**Rust-only (`keeper` shell).** Bindings: no. Depends on 35.5.

`notes_vault::registry`: every notes-flagged profile is a vault, all resident indices live
side by side, and the active vault is a selection held in Rust — so switching is a filter
change, not a reload (FR-95; the UI half is story 37.1). The registry subscribes to the
existing per-profile `notify` stream from `keeper-sync`'s watcher rather than constructing a
second watcher, because the host's inotify instance budget is a real ceiling and story 26.2
already spends one per profile. Add a criterion bench over a generated 10 000-note vault
covering cold index, warm reopen from the cache, and the steady-state cost of one changed path.
AC: cold index of a 10 000-note vault completes under 5 s and warm reopen under 500 ms, both
recorded in the bench output; switching the active vault performs zero filesystem syscalls;
enabling notes on three profiles leaves the process's watcher instance count unchanged from the
sync-only baseline; absorbing one changed path costs one `lstat`, asserted by the same counter
as 35.5.

## Out of scope

- Any note *writing*. The writer is story 36.5; this epic reads and models only.
- A note list, a note view, or any frontend surface beyond the profile form checkbox in 35.2.
- Vault encryption, and a real full-text engine — both declared out of phase.
- Reading or honouring `.obsidian/` configuration (themes, hotkeys, plugin settings). Keeper
  does not interoperate with Obsidian's *settings*; it interoperates with its *files*.
