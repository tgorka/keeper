# Epic 56 Context: The file is there, even when it is not

<!-- Generated from planning artifacts. Regenerate with compile-epic-context if planning docs change. -->

## Goal

Let a clone hold LFS-tracked content as metadata only — the committed pointer, byte for byte — while
still knowing what the content is, being able to ask for the bytes on demand, and letting them go
again safely once they are provably elsewhere. Roughly 80% of the mechanism already ships and is
unreachable: pointer-only checkout, atomic hydrate, the per-path ledger, the journal that drives a
download and the remote-presence proof all exist; what is missing is a per-path *selector*, a
*dehydrate* verb, honest metadata on every surface, and the row states and verbs that make it usable
from the app. The one load-bearing separation, taken from the way git-lfs lost data doing the
opposite: **the pattern file authorizes hydration; only per-object proof authorizes deletion.**
Editing the policy can never delete a byte — it changes what future arrivals materialize and what
becomes *eligible*, after which each object must still pass five refusals.

## Stories

- Story 56.1: A policy that says which files may stay away
- Story 56.2: A listing that knows what it does not hold
- Story 56.3: A file you can ask for
- Story 56.4: A release that refuses five times before it deletes
- Story 56.5: It lets go a day after it landed, on its own
- Story 56.6: The checks stop calling the normal state a fault
- Story 56.7: The row says what it is, and what a delete will do
- Story 56.9: The button, and the time you have left
- Story 56.8: docs/sync.md grows a virtual-files chapter (last, after 56.9)

## Requirements & Constraints

Each id names one observable behaviour a spec cites and a reviewer checks.

| id | statement | story |
|---|---|---|
| FR-328 | A folder may declare, in a committed root-level file in gitignore dialect, which paths are allowed to stay unmaterialized after a pull | 56.1 |
| FR-329 | The profile's own configuration overrides that file, and a malformed pattern refuses at startup with the pattern quoted | 56.1 |
| FR-330 | Editing the policy never deletes content; it changes what future arrivals materialize and what becomes eligible for release | 56.1, 56.5 |
| FR-331 | A virtual file's worktree bytes are exactly the committed pointer, and the path reads clean in `git status` | 56.1 |
| FR-332 | keeper can turn a materialized path back into its pointer, atomically, without disturbing a reader holding it open | 56.4 |
| FR-333 | A release refuses, by a distinguishable typed error, when the path is modified, open, unproven on the remote, pinned, or already a pointer | 56.4 |
| FR-334 | A materialization carries keeper's own last-use timestamp, and a path may be pinned against release | 56.2, 56.5 |
| FR-335 | Materializations older than a per-profile TTL (default 24 h, `0` disables) are released on the next successful sync, under a per-pass budget, and a failure never fails the sync | 56.5 |
| FR-336 | Every keeper surface reports a virtual file's true size and oid from the index and pointer, never the worktree stat | 56.2, 56.7 |
| FR-337 | A listing states what is virtual, what is materialized and when it was last used, in a human form and a stable JSON form; remote presence only on request | 56.2 |
| FR-338 | A human or an agent can materialize one path on demand, from the daemon or the app, with progress, idempotently, and a modified file is never overwritten | 56.3 |
| FR-339 | `verify` distinguishes intentionally-virtual from unredeemable, and a verified copy of a virtual file hydrates or refuses by name — never copies pointer text silently | 56.6 |
| FR-340 | A listing reports a modification time for every entry, and for a virtual path the honest size from the pointer beside it | 56.2 |
| FR-341 | For content created or modified locally, the release clock starts when keeper confirmed that path reached the remote; until that confirmation exists the path is not eligible at any age | 56.5 |
| FR-342 | A Files row distinguishes virtual, materializing and materialized by shape and by accessible name, not by colour alone | 56.7 |
| FR-343 | A materialized row shows the time remaining before release, counting down live, and a virtual row offers materialize; the deadline crosses the boundary as one absolute instant | 56.9 |
| FR-344 | The policy and the TTL are read from the folder's committed and per-host TOML layers as well as the profile, so the daemon and the app honour the same answer; a save from either surface never erases the other's value | 56.1 |
| FR-345 | A deletion plan classifies a virtual or materialized path as travelling, and says so before the deletion happens | 56.7 |
| NFR-40 | No operation may make an object's only remaining copy unreachable. Every deletion is authorized by per-object proof at the moment of deletion, never by a pattern, an age or a ref comparison | 56.4, 56.5 |
| NFR-41 | The virtual state is never reported as a fault. A folder whose policy leaves 10 000 paths virtual shows no errors and no warnings for that fact alone | 56.6 |

**How this epic must be tested.** Every story that came back `incorrect` in this repository asserted
its central claim through a pure function or a hand-placed input while the risk lived in the impure
shell. Here the impure shell is: a real repository with real bytes, a real `rename(2)`, a real index
whose stat tuple must still read clean afterwards, a real open file descriptor, and a real batch
round trip that answers 404. **56.4 and 56.5 must assert against a real git fixture and a real
process holding the file open**, in the manner of story 34.11. A `dehydrate()` unit test over a
`TestPlatform` and a hand-written pointer proves nothing about the two failures that matter: a
released file that reads MODIFIED forever, and a released file whose bytes existed nowhere else. A
story that proves the TTL by sleeping is asserting the wrong thing — the clock is injected.

## Technical Decisions

**The virtual state is the committed pointer, byte for byte.** Not a sparse file, not a zero-filled
file of the true length, not an xattr-identified stub, and not a pointer carrying extra keeper keys.
Three independent reasons: pointer blob + worktree stat = clean `git status` is the invariant the
tree already rests on, and any other content is a modification forever; the LFS pointer encoding is
unique, so an added key changes the blob OID — a content change wearing an annotation's clothes;
xattrs do not survive `rsync`/`cp`/`tar` without opt-in flags, so a stub identified only by an xattr
becomes an anonymous file after one copy. Accepted cost, stated so it is chosen and not discovered:
`ls -l`, `du` and third-party apps see ~130 bytes. keeper's own surfaces must not repeat that lie.

**Virtualization is a new type, not a fourth `LfsMode`.** `VirtualPolicy { patterns, never,
over_bytes }`, compiled once per run, built by the same discipline `LfsPolicy::from_profile` already
uses: gitignore dialect (no `/` → rewritten `**/pattern`, otherwise root-anchored) and **a malformed
glob is a hard config error at startup, never silently dropped**. `LfsMode` is profile-wide; the ask
is per path. `lfs_never` means "never route this through LFS" — very nearly the opposite of "route
it through LFS and keep it virtual"; sharing its name or its plumbing would be a trap, and
`lfs_never` stays untouched and keeps meaning what it meant. Every policy term must be answerable
from the pointer, never from the bytes: so paths plus a size floor, and no boolean language and no
MIME matching.

**Precedence, ascending:** the committed root-level pattern file (read from the worktree at run
start, never from `HEAD`) → `<folder>/.keeper/keeper.toml` `[folder]` → `<folder>/keeper.<host>.toml`
→ the host's own profile record. The **folder TOML tier is the canonical home** for anything both
hosts must honour, because the desktop app and `keeper-syncd` do not share a profile store or even a
data dir — patterns typed into the app's form are invisible to the daemon and vice versa. A surface
that shows the policy must also show **which tier is in force**, because a TOML layer outranks the
form and keeps winning on every read.

**Dehydrate is a new primitive with five refusals, not a relaxed prune.** It sits beside
`materialize`, uses the identical publish discipline (sibling `.keeper.<name>.tmp` + `rename(2)`),
and refuses loudly and by name — a typed error a caller can distinguish, not a log line — on all
five of: (1) the path is modified relative to the index or its stat says racily-clean; (2) the path
is open by any process; (3) the remote does not provably hold the object (a `download`-operation
batch probe whose per-object 404 is the server saying "I cannot serve this"), or the local store
holds it *and* the profile is configured to trust the store; (4) the path is pinned; (5) the worktree
already holds pointer text — nothing to do, and the store object may then be the only local copy.
`prune`'s condition 2 is "the worktree still holds the real content", and a path holding pointer text
is explicitly never a candidate: dehydration *inverts* that, so relaxing the predicate would
silently convert a safe operation into a deleting one. **"Open" must be a kernel fact, not an `lsof`
snapshot** — where no race-free primitive exists for a platform, refuse to dehydrate on that platform
rather than guess; the honest default is `Unknown`, which becomes a refusal. **Truncation is
forbidden**: `rename()` leaves open descriptors on the old inode intact, and truncating a file
another process has `mmap`ed delivers SIGBUS. **After the rename the index stat is repaired** via the
existing refresh/repair path, or the false-modification check fails and every dehydrated path reads
MODIFIED forever.

**Two release clocks, one selector, and the selector is provenance — not configuration.** A path that
**arrived from the remote and was never modified here** keys off `last_used_ms`: the bytes exist
upstream by construction. A path **created or modified locally** keys off the moment keeper
*confirmed* the content reached the remote for that exact path, and is **not eligible at any age**
until that confirmation exists. This is a safety improvement, not a tuning: the dangerous path is a
locally-authored file whose bytes exist nowhere else, and a TTL keyed on use would make it *eligible*
after a day of not being touched, leaving everything on one refusal holding. Two independent
barriers, in the one place where a bug deletes data.

**The ledger, not the pointer, carries mutable state.** `materialized` gains `last_used_ms`,
`synced_at_ms` (`NULL` = never confirmed), `pinned`, `oid`, `size_bytes`, added by the additive
`ensure_*_columns` idiom. `at_ms` stays the *materialization* clock. `synced_at_ms` is written where
the per-path fact is already known — the upload unit's completion for a locally-authored object, and
`lfs::audit`'s per-object `download` answer for the general case — **not** at `mark_synced`, which is
per *profile* and says nothing about one path. It is a memo of a proof keeper already pays for.
Last-used is keeper's own timestamp, never `atime` (Linux has defaulted to `relatime` since 2.6.30).

**Periodicity is a due-gate on the tick each host already runs.** One clock per host process is a
standing rule — two schedulers over one git repository is how you get concurrent index locks. The
release *sweep* rides the same success edge `prune_lfs_store` already rides, after the queue drained
and the push landed; it is budgeted per pass (a ceiling on objects and bytes, so a policy edit that
makes 40 000 paths eligible cannot turn one sync into an hour); and a sweep failure is logged and
**never** fails the sync — reclaiming space is housekeeping. "Nightly" is expressible honestly with
no scheduler at all: the first successful sync after the TTL expires. A folder that never syncs never
releases, which is the correct direction — nothing was proven about the remote either. A pin is a
hard floor: the pattern file is advisory about hydration, the pin is enforced against release.

**Hydration is an explicit verb and it reuses the journal.** There is **no on-read hydration**: a
`git checkout` of a virtual path yields pointer text. Materialize-on-read is the feature's largest
liability, not a convenience — a `grep -r`, Spotlight, a backup agent, an antivirus scanner or a `du`
walks the tree and hydrates everything. The verb enqueues the existing LFS-download work kind through
the enqueue-unique path, so a repeat request is idempotent for free (a content-addressed download is
already "covered while running"), then publishes through the existing atomic materialize. A
**user-requested unit must outrank background work**: the claim has no urgency dimension today and 16
units per profile per tick would otherwise put a human's click behind a thousand queued objects. A
modified file is never overwritten by a hydrate.

**Adding a state is not done when the row renders.** Every `match`/`matches!` over the status enum
must be revisited in the same story, and the delete confirmation is the one that can lie: it counts a
file as travelling only for `Synced | Waiting | Unknown`, so a new variant falls into the "stays on
this machine" bucket by default — keeper would tell the user a deletion is local while it removes the
pointer that *is* the tracked content. A virtual or materialized path **travels**, explicitly, with a
test pinning it.

**Three existing behaviours must stop reporting the normal state as a fault**, in the same epic that
creates the state they misread, because a false-positive wall is indistinguishable from a broken
feature: `verify` flags a worktree pointer whose object the store lacks (under a virtual policy that
is the *normal* state — it must distinguish intentionally-virtual from unredeemable, while
`verify --remote` remains the check that finds real loss); the copy planner has no pointer awareness
and copies 130 bytes silently, while it already refuses a dataless iCloud placeholder for the same
class of reason; and the re-clean-and-compare check hashes the worktree file back in, which is
meaningless for a path holding pointer text.

**No new crates.** The glob machinery, transport, journal, ledger, remote proof and atomic publish
all ship today; the ledger's new columns are the additive idiom; the only new OS-facing question is a
race-free "is this file open" answer, which is a platform-trait method whose honest default is a
refusal.

## UX & Interaction Patterns

- **Four row states, each needing a shape.** The engine-side status enum and the view-model status
  enum both grow **virtual**, **materializing**, **materialized**. Three exhaustive `Record` maps
  make that a compile error until each state has a label, a glyph and a tone. House rule: **shape
  carries the distinction, colour is emphasis only** — and the words are composed in Rust, never in
  TypeScript, so two surfaces cannot disagree.
- **Rust ships a moment; TypeScript renders a duration.** The wire carries **one absolute epoch-ms
  deadline**; the remaining-time text is `deadline − Date.now()` in the frontend. A countdown is the
  one string Rust cannot own — it is stale the instant it is serialized — and the Files tree does not
  poll at all (its listings are on demand, unlike the Sync pane's pollers).
- **The tick is owned once by the pane, never by the row.** Files rows are windowed, so a per-row
  interval would arm and disarm on every scroll. Copy the existing shape: one shared 1 s interval
  plus a pure `secondsLeft(deadlineMs, now)` helper, with its `motion-reduce` and announce-once
  rules. Under `motion-reduce` the countdown still reads as text.
- **`materializing` is the first in-flight state a Files row has ever had.** The pane deliberately
  has no loading flag — "a node with no listing yet IS the in-flight state" — so this is new surface,
  and it must not invent a percentage: an unknown byte total renders indeterminate.
- **A row action is one entry in the single `actions` array**, which feeds both the hover cluster and
  the context menu. A virtual row offers **Materialize**; a materialized row offers **Release** and
  **Pin**.
- **Every entry gains a modification time**, off the `stat` the listing already pays for. For a
  virtual path the honest size still comes from the pointer; the mtime is the worktree's, and that is
  the correct answer.

## Cross-Story Dependencies

- **56.1 → 56.2 → 56.3 → 56.4 → 56.5 is a strict chain**: each needs its predecessor's type.
- **56.6 and 56.7** are disjoint from each other; both depend on 56.2 (state vocabulary) and 56.4
  (the verb they offer).
- **56.9 depends on 56.7** (the states) **and 56.5** (the deadline the countdown counts), and is the
  last code story.
- **56.8 last of all, and only after 56.9** — a documented TTL that does not yet exist is the one
  sentence a reader will act on.
- **Epic 57 boundary — what Epic 56 must NOT build.** No task record, no schedule, no last-run time,
  no last result, no Tasks view, no CLI `tasks` verb, no lease, no `tasks`/`task_runs` tables, no
  schedule parser, no systemd timer unit, and no host-ownership UI. Epic 56's *only* obligation to
  Epic 57 is that automatic release on the success edge is one of three release modes rather than the
  only one — 56.8 states in one sentence that a scheduled or manual release is Epic 57's `tasks`
  verb, so a reader looking for "the nightly script" finds where it lives instead of assuming it does
  not exist. FR-346…FR-352 and NFR-42/NFR-43 are not this epic's to satisfy.
- **Ask 5 (Finder / `ls` integration) is CLOSED, not deferred.** macOS File Provider cannot
  virtualize a path the user chose, `SF_DATALESS` is unsettable from user space, kexts are
  policy-dead, and macFUSE/fuse-t both fail the permissive-only licence firewall. Linux is deferred
  with a stated shape (a read-only FUSE **mirror** mount, never a virtualization of the worktree
  itself); `fanotify` pre-content HSM is deferred with a stated revisit trigger (the page-fault hook
  and BPF event suppression both landing). 56.8 must state this with the reason so it is not
  re-asked.

## Repo Anchors

Verified against the tree at `main-vf` (`6af6dce`). **Several citations circulating in the planning
docs are stale — these numbers, not those, are authoritative.** Corrections worth knowing:
`indexed_size` is at `stage.rs:840` (not 800-820), `indexed_pointer` at `:823` (not 792-798),
`materialize` at `:1309` (not 1118-1143); `mark_synced` is at `engine.rs:3279` (not 3185-3197);
`CLAIM_LIMIT`/`TICK_MS` are at `engine.rs:335`/`:362`; `materialized_paths` is at `db.rs:345`; the
re-clean-and-compare is at `engine.rs:5956` inside `republish_missing_objects` (there is exactly one
`stage::clean` call site in `engine.rs`, and it is not at 5553-5560); `EntrySyncStatus` is at
`browse.rs:158` and `FilesSyncStatusVm` at `vm.rs:3812` (both correct as circulated).

### Profile and policy — `src-tauri/crates/keeper-sync/src/profile/mod.rs`

| what | where |
|---|---|
| `LfsMode` enum; variants `Materialize`, `PointerOnly`, `Disabled` | `:81`, `:86`, `:89`, `:93` |
| `lfs_never: Vec<String>` (doc: gitignore dialect, refused at startup) | `:829` (doc `:813-828`) |
| `lfs_prune_local: bool`, `#[serde(default = "default_true")]` | `:812`; default `:938` |
| `subpaths: Vec<String>` — the per-path transfer gate that already exists | `:752` |
| `MediaPolicy` — the precedent that a differently-scoped answer gets its own enum | `:371` (`:369-389`) |
| `#[serde(default)]` **is** the migration (a profile is one JSON blob per row) | `:855-861`; test doc `:1360-1362` |
| `accepted_profile_keys()` — the set the folder-tier rules are checked against | `:1105` |

### LFS staging, prune, audit — `src-tauri/crates/keeper-sync/src/lfs/`

| what | where |
|---|---|
| `LfsPolicy` struct | `stage.rs:135` |
| `LfsPolicy::from_profile` — glob compilation, gitignore anchoring, refuse-on-typo | `stage.rs:152`; doc `:142-151`; anchoring `:158-162`; `SyncError::Config` `:167`, `:172` |
| `indexed_size` — honest size from the pointer/object header, loads no content | `stage.rs:840` |
| `indexed_pointer` — the oid, from the index | `stage.rs:823` |
| `materialize` — atomic publish; store-presence precondition | `stage.rs:1309`; precondition `:1311-1312` |
| `is_false_modification` — the check the index-stat repair must satisfy | `stage.rs:955` |
| `prune`'s three conditions; condition 2 and "a path holding pointer text … is never a candidate" | `prune.rs:20-39`; `:28-33` |
| `lfs::audit` — per-object 404 is the server saying "I cannot serve this"; reason mapping | `audit.rs:30-31`; `:70` |

### Engine — `src-tauri/crates/keeper-sync/src/engine.rs`

| what | where |
|---|---|
| `CLAIM_LIMIT: u32 = 16` (per profile per tick) | `:335` |
| `TICK_MS: u64 = 1_000` | `:362` |
| `Engine::run` — the supervisor loop | `:1284` |
| `tick_profile` | `:1353` |
| `scan_is_due` / `sweep_is_due` — the due-gate shape to copy | `:1398` / `:1484` |
| `drain_journal` | `:2790` |
| `mark_synced` — **the success edge**; the `prune_lfs_store` call; "never fatal" doc | `:3279`; call `:3283-3291`; doc `:3264-3278` |
| `reconcile_sparse_cone` | `:3490` |
| `materialize_pending` — the one decision point; `subpaths` cone; `PointerOnly` lever | `:5385`; `:5394`; `:5417` |
| `SyncPhase::DownloadingLfs` — the progress sink 56.3 reuses | `:5082` |
| `prune_lfs_store` | `:5691` |
| `wake_now` | `:5813` |
| `PendingReason` (inbound vocabulary; stays as is) | `:225` |
| `republish_missing_objects` + the re-clean-and-compare 56.6 must fix | `:5929`; `:5956` |
| `Engine::verify`; the pointer-without-object branch that calls the normal state bad | `:6006`; `:6040-6048` |

### Ledger and journal — `src-tauri/crates/keeper-sync/src/db.rs`

| what | where |
|---|---|
| `migrate` (private fn) | `:58` |
| table list: `profiles`, `journal`, `file_state`, `activity`, `device`, `materialized`, `meta` | `:61`, `:69`, `:85`, `:107`, `:121`, `:142`, `:149` |
| the `materialized (profile_id, path, at_ms)` table | `:142-147` |
| additive-migration idiom: `ensure_activity_columns` / `ensure_journal_columns`, called from `migrate` | `:231` / `:261`; calls `:155-157` |
| `remember_materialized` (`INSERT OR REPLACE`) | `:328`; statement `:335` |
| `materialized_paths` | `:345` |
| `label_unit` | `:281` |
| `WorkKind` (closed six-variant vocabulary) | `:609` |
| `covered_while_running` — why a repeat download request is idempotent for free | `:649` |
| `enqueue_unique` | `:740` |
| `claim_ready` — the single-statement claim, **no urgency dimension today** | `:773` |
| `complete` — "the only place work leaves the journal" (`DELETE`) | `:827` |
| forward compatibility: an unreadable/unknown row is skipped, never fatal | `:540-542`; `:1446-1447` |

### Browse — `src-tauri/crates/keeper-sync/src/browse.rs`

| what | where |
|---|---|
| `BrowseEntry` | `:88` |
| `size_bytes` field, and the "**Free** — the `stat` behind it is the same one `is_dir` already paid for" doc | `:120`; doc `:110-111` |
| **the listing site that reports `fs::metadata().len()`** and must not, for a pointer | `:619-626`; assigned `:644` |
| `EntrySyncStatus` (5 variants: `Synced`, `Waiting{reason}`, `Excluded`, `NotInRepository`, `Unknown`) | `:158-180` |
| `classify` | `:722` |

### View models — `src-tauri/crates/keeper-core/src/vm.rs`

| what | where |
|---|---|
| module doc: every IPC type derives `ts_rs::TS`, is `#[ts(export)]`, camelCase; bindings emitted to `src/lib/ipc/gen/` by the export test | `:3-7` |
| `FilesSyncStatusVm` (5 variants) | `:3812` |
| `FilesEntrySyncVm` (`status` + Rust-composed `detail`) | `:3839` |
| `FileSizeVm` | `:3883` |
| `FilesEntryVm` | `:4025` |
| `impl FilesDeletePlanVm`; `compose`; **the `travels` `matches!` that must learn the new states** | `:4384`; `:4402`; `:4412-4422` |
| `ConfigTierVm` — the vocabulary for "which tier is in force" | `:4691` |

### App IPC — `src-tauri/crates/keeper/src/`

| what | where |
|---|---|
| `SyncProfileReq` | `sync_ipc.rs:559` |
| `parse_req`; it starts from `prior.clone()`; "anything this function does not carry is erased on save" | `sync_ipc.rs:838`; `:878`; doc `:825-829` |
| the DW-116 regression test `saving_an_edit_does_not_reset_a_daemon_configured_scan_cadence` | `sync_ipc.rs:3893` |
| the daemon-only-fields doc (why `lfsPruneLocal` needs no form slot) | `sync_ipc.rs:3440-3444` |
| the command-registration macro `keeper_with_commands!`; the single `invoke_handler` + `generate_handler!` | `lib.rs:700`; `lib.rs:702` |
| the comment recording the two-registration outage | `lib.rs:686-699` |
| the Rust test that pins one registration | `ipc.rs:13943` |

### Daemon — `src-tauri/crates/keeper-syncd/src/commands.rs`

| what | where |
|---|---|
| `Command` enum (clap `Subcommand`) — where a `materialize` / `release` verb is added | `:212` |
| the dispatch `match cli.command` and its arms | `:507-550` |
| `sync --once` doc: "the cron entry point" | `:232` |
| `verify` exits non-zero so a cron wrapper sees it | `:1139-1142` |

### Config tiers

| what | where |
|---|---|
| the six-tier TOML table and per-key precedence ("later wins, per key") | `keeper-core/src/config/mod.rs:13-23` |
| the `[folder]` table = this folder's `SyncProfile` fields | `keeper-core/src/config/mod.rs:31-32` |
| folder tier: the two folder files, both of which sync | `keeper-sync/src/profile/folder.rs:4-11` |
| a folder file configures the FOLDER, never the app; `[settings]` refused by name | `folder.rs:13-29` |
| resolved at read time, **never written back** (this is what lets the file keep winning) | `folder.rs:31-38` |
| `folder_field_rule` — every field must be classified | `folder.rs:149` |
| `install_folder_tier` | `folder.rs:530` |
| the test that forces a new field to be classified | `folder.rs:681` |

### Frontend

| what | where |
|---|---|
| Files row leading-icon derivation (folder role → folder → viewer icon) | `src/components/layout/files-pane.tsx:1653-1660` |
| the single `actions` array (feeds hover cluster **and** context menu) | `files-pane.tsx:1695`; widths `:1752` |
| the sync-mark slot in the row | `files-pane.tsx:1892` |
| the hover cluster | `files-pane.tsx:1934-1936` |
| the context menu | `files-pane.tsx:1988-2008` |
| "no `loading` set: a node with no listing yet IS the in-flight state" | `files-pane.tsx:726-728` |
| `useWindowedRows` (rows are windowed — hence one tick per pane) | `files-pane.tsx:127`, usage `:1402` |
| the three exhaustive `Record<FilesSyncStatusVm, …>` maps: label, icon, tone | `src/components/layout/sync-status-mark.tsx:40`, `:49`, `:71` |
| shape carries the distinction, colour is emphasis only | `sync-status-mark.tsx:9-11` |
| the words arrive composed in Rust | `sync-status-mark.tsx:19-22` |
| `invoke` wrapper; "the only hand-written TypeScript in `src/lib/ipc/`"; `./gen/` is generated, **never hand-edited** | `src/lib/ipc/client.ts:4-7` |
| generated bindings directory (never hand-edit) | `src/lib/ipc/gen/` |
| command-registration guard, and why it lives in TS (the `keeper` shell crate does not build on Linux) | `src/test/command-registration.test.ts:1-33`; `:23-25` |
| **the one countdown precedent**: `secondsLeft` pure helper; one shared 1 s interval; `motion-reduce` + announce-once | `src/components/chat/undo-send-pill.tsx:25-27`; `:42-48`; `:6-9`, `:130` |
| indeterminate progress precedent (`null` total → never invent a denominator) | `src/components/settings/sync-section.tsx:482-484`; `:555-559` |

### Supporting seams

| what | where |
|---|---|
| `SyncPlatform` trait (the seam for a race-free "is this file open"); `now_ms` (the injected clock) | `keeper-sync/src/platform.rs:24`; `:52` |
| `repair_index_stat` / `refresh_index_stat`; "pointer blob, worktree stat, clean status" | `keeper-sync/src/git/repo.rs:2006`; `:2048`; doc `:2046-2047` |
| the copy planner's dataless-placeholder refusal — the shape a pointer refusal copies | `keeper-sync/src/copy.rs:824-830`; doc `:47-49` |
| an entry VM already carrying `mtime_ms` with the `#[ts(type = "number")]` reasoning | `keeper-core/src/sessions/vm.rs:194-196`; `:718-722` |

## Gates and Traps

**Gate commands that work on this Linux host.**

```
cargo clippy --manifest-path src-tauri/Cargo.toml -p keeper-core -p keeper-sync -p keeper-syncd --all-targets -- -D warnings
cargo test --manifest-path src-tauri/Cargo.toml -p <crate>
bun run lint          # biome check .            (package.json:16)
bun run typecheck     # tsc --noEmit             (package.json:19)
bun run test          # vitest run               (package.json:21)
```

- **`cargo nextest` is NOT installed on this host.** `bun run test:rust`
  (`package.json:23`) and `bun run bindings:check` (`:24`, which is `test:rust` plus a
  `git status --porcelain -- src/lib/ipc/gen` emptiness check) therefore **cannot run as written**.
  Use `cargo test -p <crate>` for Rust tests and inspect `src/lib/ipc/gen` by hand for binding drift.
- **The `keeper` shell crate cannot link on Linux.** Workspace members are
  `keeper-core`, `keeper`, `keeper-sync`, `keeper-syncd` (`src-tauri/Cargo.toml:3`); the three
  buildable and testable here are **`keeper-core`, `keeper-sync`, `keeper-syncd`**. Never scope a
  clippy or test invocation to `keeper` or to `--workspace` on this box. Consequences: anything the
  shell owns must be asserted from TypeScript instead (the pattern
  `src/test/command-registration.test.ts:23-25` states), and `bun run check:rust`
  (`package.json:32`, `--workspace`) is not usable here.
- **ts-rs binding rules.** Bindings are emitted **by the Rust export test**, into
  `src/lib/ipc/gen/` (`keeper-core/src/vm.rs:3-7`); that directory is generated and
  `src/lib/ipc/client.ts` is the only hand-written file under `src/lib/ipc/`
  (`client.ts:4-7`) — **never hand-edit `gen/`**. A 64-bit field **must** carry
  `#[ts(type = "number")]` or ts-rs emits `bigint`, which is not what Tauri's `JSON.parse` delivers
  and which makes every arithmetic comparison in the countdown a type error
  (`keeper-core/src/notes/vm.rs:13-16`). This applies directly to `releases_after_ms` and to the new
  mtime field.
- **Dependency-firewall scripts** (`package.json:25-27`) — a story that adds a dependency to the
  wrong crate fails these:
  - `check:core-tauri-free` — `keeper-core`'s normal+build tree must contain **no `tauri*` crate**.
  - `check:core-sync-free` — `keeper-core`'s tree must contain **no `gix`/`gix-*` and no
    `keeper-sync`**. So no view-model may reach into engine types; the engine projects into the VM,
    never the reverse.
  - `check:syncd-lean` — `keeper-syncd`'s tree must contain **no `tauri*`, no `matrix-sdk*` and no
    `keeper-core`**. So the daemon cannot borrow a view model or a formatter from core; anything both
    hosts need lives in `keeper-sync` or is duplicated deliberately.
  - `bun run check` (`:31`) chains lint + typecheck + test + all three firewalls, and does run here.
- **Durable-doc constraints already recorded** (56.8 extends, never contradicts): keeper checks for
  dataless files before opening and skips placeholders, because opening one silently materializes it
  — the conservative side of the on-read-hydration question, already chosen; `verify --remote`
  transfers nothing, uses the `download` operation for its per-object 404, and exits non-zero so a
  cron wrapper sees it; `lfsPruneLocal` releases the redundant *store* copy only when the journal
  owes no transfer for it, **the worktree still holds the real content**, and nothing else is
  running, and a failure to release is logged and never fails the sync; `lfsNever` uses the gitignore
  dialect and a malformed glob is refused at startup rather than ignored. The daemon's refusal of
  timer-driven work covers **exactly one thing** — replacing the running binary unattended, because
  the daemon can be mid-push at any moment. It is **not** a refusal of scheduled work.
- **The three second-pass deltas a spec author must not miss.**
  1. The sync-confirmed clock **narrows** the last-use clock. For content created or modified locally
     the release clock starts at the sync-confirmed edge, not last use, and such a path is not
     eligible **at any age** until that confirmation exists. The `materialized` ledger's `at_ms` is
     the *materialization* clock; `synced_at_ms` is a **new column**. Two clocks, one selector.
  2. The desktop app and `keeper-syncd` do **not** share a profile store (a JSON blob per row in
     `sync.db`'s `profiles` table vs `[[profile]]` tables in the daemon's `config.toml`) or a data
     dir. Config both surfaces must honour lives in the **folder TOML tier**. Every new
     `SyncProfileReq` field is `Option`, or it erases a daemon-set value — DW-116 verbatim.
  3. Adding a `FilesSyncStatusVm` variant is not done when the row renders: the delete plan's
     `compose` would classify a virtual path as "stays on this machine" by default, so the delete
     confirmation would lie about a deletion that travels. **56.7 carries the test.**
- **Research is cited by section number only** (`research-virtual-files-2026-08-22.md`): §2 the two
  prior-art families; §3 selective fetch and the pattern-file question; §4 OS-level placeholders
  (§4.1 macOS, §4.2 `fanotify`); §5 metadata for non-local content; §6 releasing content safely —
  the part that loses data; §7 pattern-file design; §8 accidental mass hydration; §9 what keeper
  already has; §10 the recommended design; §11 options rejected; §12 open questions (pin syntax is
  one of them).
