---
name: 'keeper'
type: architecture-spine-companion
purpose: build-substrate
altitude: initiative
paradigm: 'hexagonal Rust core + unidirectional view-model projection — unchanged; a virtual file is a state of an existing LFS path, not a new domain'
scope: 'keeper virtual files — LFS-tracked content a clone knows about but does not hold: a per-path virtualization policy, metadata without bytes, explicit hydrate/dehydrate verbs, and a lazy release sweep on the success edge'
status: final
created: '2026-08-22'
binds: [FR-328..FR-339, NFR-40, NFR-41]
sources:
  - _bmad-output/planning-artifacts/research-virtual-files-2026-08-22.md
  - _bmad-output/planning-artifacts/research/virtual-files-2026-08-22/
  - docs/sync.md §4, §8, §12
parent: ARCHITECTURE-SPINE.md
---

# Architecture Companion — Virtual files

Extends the frozen spine with **AD-122..AD-130**. Nothing here renegotiates it: large content
still must never become a git blob (AD-46), `keeper-sync` still reaches the OS only through
`SyncPlatform` (AD-40/AD-52), the engine is still the only thing that decides *when* to use a
capability, and keeper still owns `filter.lfs.process` in its own repositories.

**The one-sentence shape:** a *virtual file* is an LFS path whose worktree bytes are the
committed pointer — nothing more — and keeper is the only thing that knows how to turn that
into content and back, on request, with proof.

**What is genuinely new is one verb.** keeper already ships the pointer-only checkout state, the
atomic hydrate, the per-path timestamped ledger, the journal that drives a download, and the
remote-presence proof (research §9). It has no **dehydrate**, and dehydrate is where data is
lost if it is wrong (research §6).

---

## Architecture decisions AD-122 … AD-130

### AD-122 — Virtualization is a per-path policy, and it is a new type

- **Binds:** FR-328, FR-329; Epic 56
- **Decision.** Virtualization is selected by a **`VirtualPolicy`** compiled once per run from
  (a) a committed root-level pattern file in gitignore dialect and (b) the profile's own
  configuration, which **overrides** the file. It is *not* a fourth `LfsMode` variant and *not*
  an extension of `lfs_never`.
- **Why a new type.** `LfsMode` is profile-wide, compared in nine places and projected in four
  (`stage.rs:91,149,1022`; `engine.rs:1986,4768,5003,5025,5037,5439`; `commands.rs:414,439`;
  `sync_ipc.rs:624,764`); the ask is per path. `MediaPolicy` (`profile/mod.rs:364-389`) is this
  tree's own precedent that a differently-scoped answer gets its own enum rather than a new
  variant of the old one. And `lfs_never` means *"never route this through LFS"* — very nearly
  the opposite of *"route it through LFS and keep it virtual"*; sharing its name or its plumbing
  would be a trap (research §11.1, §11.2).
- **Shape.** `VirtualPolicy { patterns: GlobSet, never: GlobSet, over_bytes: u64 }`, built by the
  same discipline `LfsPolicy::from_profile` already uses (`stage.rs:124-152`): gitignore dialect —
  a pattern with no `/` is rewritten `**/pattern`, otherwise root-anchored — and **a malformed
  glob is a hard `SyncError::Config` at startup, never silently dropped**. An opt-out that
  silently does nothing is how a note ends up an opaque pointer months later
  (`docs/sync.md:427-429`); an opt-*in* that silently does nothing is how 200 GB arrives on a
  metered link.
- **Precedence, and why the file is committed.** The committed file states the *repository's*
  intent — the same role `.lfsconfig` plays for git-lfs, whose `fetchinclude`/`fetchexclude` are
  on its security allow-list and are overridden by local git config (research §3.1, §7). The
  profile's configuration is the *machine's* answer, and wins. A server that must keep
  everything says so without editing a file its peers share.
- **Every policy term must be answerable from the pointer, never from the bytes.** Size is
  (the pointer carries it); MIME type is not, once the file is a stub. git-annex documents this
  exact trap for `mimetype=` (research §7). So the expression set is: paths, plus a size floor.
  No boolean language — git-annex's own docs call the `.gitattributes` encoding of one
  *"not recommended"*.

### AD-123 — The policy authorizes hydration; only proof authorizes deletion

- **Binds:** FR-330, NFR-40; Epic 56
- **Decision.** `VirtualPolicy` decides **what may be left unmaterialized on arrival**. It is
  **never** consulted to decide that bytes may be deleted. Deletion is authorized per object, at
  the moment of deletion, by the conditions in AD-125.
- **Why this is the load-bearing decision.** git-lfs shipped precisely the inverse and lost data:
  `lfs.fetchexclude` began outranking *"referenced by the current checkout"*, so an excluded
  path's object was pruned (git-lfs#3092). Every documented data-loss incident in this space is a
  reachability-enumeration bug, not a transfer bug (research §6.2). The requested feature is
  literally *"a gitignore-like file naming files that are not downloaded"* — the same shape as the
  mechanism that caused #3092. The separation is the mitigation.
- **Consequence.** Editing the pattern file can never delete anything. It changes what future
  arrivals materialize, and it makes existing materializations *eligible* for the sweep — which
  then applies AD-125 per object.

### AD-124 — The virtual state IS the committed pointer, byte for byte

- **Binds:** FR-331; Epic 56
- **Decision.** A virtual file's worktree bytes are exactly the pointer blob the index holds.
  Not a sparse file, not a zero-filled file of the true length, not a file whose identity lives
  in an xattr, and not a pointer carrying extra keeper keys.
- **Why.** Three independent reasons, each sufficient:
  - **`git status` must stay clean.** Pointer blob + worktree stat = clean status is the
    invariant this tree already rests on (`git/repo.rs:1946-1947`). Any other content is a
    modification, forever (research §2.1).
  - **The pointer encoding is unique.** The LFS spec permits unknown keys but there is *exactly
    one valid encoding*, so adding a key changes the blob OID — a content change masquerading as
    an annotation (research §5.1). `filter::clean` already re-emits pointer text byte-for-byte
    rather than re-rendering it, for exactly this reason (`lfs/filter.rs:232-254`,
    `docs/sync.md:586-591`).
  - **xattrs do not survive copying.** `rsync` needs `-X` and copies only `user.*` as a non-root
    user; `cp` needs `--preserve=xattr`; `tar` needs opt-in. A stub identified only by an xattr
    becomes an anonymous file after one `rsync` (research §5.1). xattrs may decorate; they may
    never identify.
- **Consequence, stated plainly because it is the user-visible cost:** `ls -l` and `du` report
  ~130 bytes, and an application that opens a virtual file reads pointer text. keeper's own
  surfaces must not repeat that lie (AD-127); other people's tools will.

### AD-125 — Dehydrate is a new primitive with five refusals, not a relaxed prune

- **Binds:** FR-332, FR-333, NFR-40; Epic 56
- **Decision.** `lfs::stage::dehydrate` sits beside `materialize` (`stage.rs:1118-1143`), uses
  the identical publish discipline — sibling `.keeper.<name>.tmp` + `rename(2)` — and refuses,
  loudly and by name, on **all five** of:
  1. **the path is modified** relative to the index, or its stat says racily-clean;
  2. **the path is open** by any process;
  3. **the remote does not provably hold the object** — a `download`-operation batch probe, whose
     per-object 404 is the server saying *"I cannot serve this"* (`lfs/audit.rs:29-31`), or the
     local store holds it *and* the profile is configured to trust the store;
  4. **the path is pinned**;
  5. **the worktree already holds pointer text** — nothing to do, and the store object may then
     be the only local copy.
- **Why not a relaxation of `prune`.** `prune`'s condition 2 is *"the worktree still holds the
  real content"*, and a path holding pointer text is explicitly never a candidate
  (`prune.rs:28-33`). Dehydration inverts that: afterwards **the store object is the only local
  copy**. Relaxing the predicate would silently convert a safe operation into a deleting one
  (research §9.1).
- **Why exactly these five.** Each closes a cited failure: dirty-eviction is refused by Apple's
  File Provider (`unsyncedEdits`) and by Lustre HSM (`DIRTY` blocks `hsm_release`);
  open-eviction returns `EBUSY` on Apple, and cachefilesd skips objects the kernel says are in
  use; inference-instead-of-proof is git-lfs#5636 and the whole `--verify-remote` design;
  pinning is git-annex's `required`; and case 5 is the inverse of `prune`'s condition 2
  (research §6.1–§6.4).
- **"Open" must be a kernel fact, not an `lsof` snapshot** — `lsof` polling is TOCTOU by
  construction, and this tree already refuses it on cost grounds (`docs/sync.md:155-158`). Where
  no race-free primitive exists for a platform, the honest answer is to **refuse to dehydrate on
  that platform** rather than to guess. `SyncPlatform` (`platform.rs:24-110`) is the seam.
- **After the rename, the index stat is repaired** by the existing `refresh_index_stat` /
  `repair_index_stat` path (`engine.rs:5064`, `git/repo.rs:1880-1947`) — otherwise
  `is_false_modification` (`stage.rs:924-975`) fails condition (a) and every dehydrated path
  reads MODIFIED.
- **Truncation is forbidden.** `rename()` leaves open descriptors on the old inode intact;
  truncating a file another process has `mmap`ed delivers SIGBUS (research §6.5).

### AD-126 — Last use is keeper's own timestamp, and the sweep rides the success edge

- **Binds:** FR-334, FR-335; Epic 56
- **Decision.** The existing `materialized (profile_id, path, at_ms)` ledger (`db.rs:142-146`)
  gains `last_used_ms`, `pinned` and the object's `oid`/`size_bytes`. The release sweep runs
  **on the same success edge `prune_lfs_store` already rides** (`mark_synced`,
  `engine.rs:3052-3066`) — after the queue has drained and the push has landed — not on a timer
  and not from a cron entry.
- **Why not atime.** Linux has defaulted to `relatime` since 2.6.30, and `noatime`/`lazytime` are
  common; a TTL keyed on atime systematically mis-retains (research §6.6). keeper writes its own
  timestamp at materialize and at every use it can observe (an IPC open, a `sync_open_entry`, a
  media-protocol read).
- **Why not a timer.** There is no `.timer` unit anywhere in the repository — the daemon's
  `ExecStart` is `keeper-syncd watch` (`keeper-syncd/packaging/keeper-syncd.service:33`) — and
  the tree's stated reason for refusing timer-driven work on this daemon is one layer over:
  it "holds a durable journal and can be mid-push at any moment", so it never self-updates on a
  timer either (`docs/sync.md:889-891`).
  "Nightly" is therefore expressed honestly: **the first successful sync after the TTL expires**.
  A folder that never syncs never releases, which is the correct direction — nothing was proven
  about the remote either.
- **The sweep is budgeted.** A per-pass ceiling on objects and bytes, so a policy change that
  makes 40 000 paths eligible cannot turn one sync into an hour of stat-and-rename. Reclaiming
  space is housekeeping: a failure is logged and **never** fails the sync, exactly as
  `lfs_prune_local` already behaves (`docs/sync.md:367-368`).
- **A pin is a hard floor**, mirroring git-annex's `required`/`wanted` split: the pattern file is
  advisory about hydration, the pin is enforced against release (research §3.2).

### AD-127 — Metadata comes from the index, the pointer and the ledger; never from the stat

- **Binds:** FR-336, FR-337; Epic 56
- **Decision.** Every surface that reports a virtual file reports the **honest** size and a
  **virtual** state. The honest size is already written and unused:
  `stage::indexed_size` (`stage.rs:800-820`) answers from the pointer for an LFS entry and from
  the object header otherwise, loading no content; `indexed_pointer` (`stage.rs:792-798`) gives
  the oid. `browse.rs:613-626` currently reports `fs::metadata().len()` and must not.
- **One derivation, more callers.** `engine.rs:4498-4510` is already a second inlined copy of the
  index-size idea; this decision collapses both to `indexed_size` rather than adding a third.
- **The state vocabulary extends what exists** rather than inventing a parallel one:
  `browse::EntrySyncStatus` (`browse.rs:157-198`) gains a `Virtual` member, and
  `PendingReason::Incoming { size_bytes, replacing }` (`engine.rs:238-256`) stays the inbound
  vocabulary. There is no `SyncBlocker` type in this repository and none is introduced.
- **Two output shapes, deliberately** — the split git-lfs made and documented: a human listing,
  and a **stable JSON** listing that is the contract (`git lfs ls-files --json` is stable,
  `--debug` is explicitly not). Remote presence is never implied by a listing; it costs a round
  trip and is requested explicitly, the way `git annex whereis` reports only what it last heard
  (research §5.3).
- **Mutable state never enters the pointer.** Last-used and pinned are ledger columns. The
  pointer is a git blob; mutating it dirties the tree and rewrites content. git-annex's own
  refusal to record last-verified timestamps in versioned storage is the cited precedent
  (research §5.1).

### AD-128 — Hydration is an explicit verb, and it reuses the journal

- **Binds:** FR-338; Epic 56
- **Decision.** There is **no on-read hydration**. A `git checkout` of a virtual path yields
  pointer text, exactly as today. Materialization happens because a human or an agent asked for
  it, through `keeper-syncd materialize <profile> <subpath>` or
  `sync_materialize_entry(profile, subpath)`.
- **Why.** Materialize-on-read is the feature's largest liability, not a convenience: a `grep -r`,
  Spotlight, a backup agent, an antivirus scanner or a `du` walks the tree and hydrates
  everything. Microsoft documents *"large-scale hydration and unexpected data consumption"*;
  Nextcloud shipped an infinite implicit-hydration loop; Lustre ships a non-blocking-restore mode
  that returns `ENODATA` rather than hydrating (research §8). This tree already chose the
  conservative side of exactly this question for iCloud placeholders
  (`docs/sync.md:163-168`, `copy.rs:823-830`).
- **Also why:** on-read hydration through the smudge filter would require advertising the `delay`
  capability keeper deliberately leaves unadvertised (`lfs/filter.rs:294-298`) plus a second state
  machine, with the DW-206 launch-failure hazard attached (`lfs/filter.rs:729-736`).
- **Mechanism.** The verb enqueues the existing `WorkKind::LfsDownload` through
  `enqueue_unique` + `label_unit` (`db.rs:739-744`, `db.rs:280-286`) — so a repeat request is
  idempotent for free, because `covered_while_running` already answers true for a
  content-addressed download (`db.rs:635-651`) — then publishes through `stage::materialize`.
  A **user-requested unit outranks background work**: `claim_ready` (`db.rs:772-776`) has no
  urgency dimension today, and `CLAIM_LIMIT = 16` per profile per tick (`engine.rs:331-336`)
  would otherwise put a human's click behind a thousand queued objects.
- **A modified file is never overwritten by a hydrate** — git-lfs's `checkout` rule
  (research §3.1), and the same direction as `materialize`'s existing store-presence
  precondition (`stage.rs:1120-1125`).

### AD-129 — The engine must stop reporting the normal state as a fault

- **Binds:** FR-339, NFR-41; Epic 56
- **Decision.** Three existing behaviours are corrected in the same epic that creates the state
  they misread, because each of them turns a working feature into a wall of false alarms:
  - `Engine::verify` reports a path as bad when the worktree holds a pointer whose object the
    store lacks (`engine.rs:5637-5645`). Under a virtual policy that is the *normal* state. It
    must distinguish **intentionally virtual** from **unredeemable**, and
    `keeper-syncd verify --remote` remains the check that finds real loss
    (`docs/sync.md:313-326`).
  - `copy.rs` has no LFS-pointer awareness: it copies 130 bytes of pointer text with no warning
    (`context/g1-lfs-machinery.md` §7.5), while it already refuses a dataless iCloud placeholder
    for the same class of reason (`copy.rs:823-830`). A verified copy of a virtual file must
    either hydrate first or refuse by name — silently copying a stub onto a pendrive is the
    "present but empty" failure this tree already fixed once for filesystem remotes
    (`lfs/local.rs:11-19`).
  - The upload-side re-clean check (`engine.rs:5553-5560`) hashes the worktree file back in and
    compares; for a path holding pointer text that comparison is meaningless.
- **Why in this epic.** A false-positive wall is indistinguishable from a broken feature, and
  this repository has already paid twice for shipping a state its own checks did not understand
  (DW-140/DW-206, `docs/sync.md:545-551`).

### AD-130 — OS-level presentation is a separate, Linux-first, read-only deliverable

- **Binds:** deferred; no FR in Epic 56
- **Decision.** keeper does **not** attempt to make virtual files appear at their true size in
  `ls` or Finder in this epic. If it ever does, the shape is a **read-only FUSE mirror mount on
  Linux** — a second view of the worktree, never a virtualization of the worktree itself — built
  on `fuser` (MIT), optionally using FUSE passthrough (kernel 6.9+) once materialized.
- **macOS is rejected outright, not deferred.** `NSFileProviderReplicatedExtension` gives exactly
  the desired semantics but its storage is exposed under `~/Library/CloudStorage/<Provider>` and
  its container path is relative to an app-group container — there is no documented API to
  virtualize a path the user chose. `SF_DATALESS` *"may not be set or unset from user space"*.
  Kexts are policy-dead. macFUSE's licence forbids redistribution bundled with commercial
  software and its kext is closed-source; fuse-t requires a paid commercial licence — both fail
  the `cargo deny` posture. FSKit is a mount, not a placeholder. And FileProvider-backed paths
  have a live deadlock report for tools that do not use `NSFileCoordinator` — which `git` does
  not (research §4.1, §11.3, §11.4).
- **`fanotify` pre-content HSM is the one mechanism that could decorate paths in place**, and it
  is not ready: kernel ≥ 6.14, `CAP_SYS_ADMIN`, `mmap` materializes whole files because the
  page-fault hook was merged and backed out, directory events and filesystem freeze deadlock, and
  every read re-fires the event because the BPF suppression is unimplemented. **Revisit trigger:**
  the page-fault hook and BPF suppression both landing (research §4.2, §11.5).
- **What the mount would cost, stated up front so it is chosen and not discovered:** any tool
  that walks it — `du`, `rsync`, a backup agent, an indexer — reads what it walks, and a
  hydrating mount would fetch everything. A read-only mirror that reports true sizes and returns
  `ENODATA` for unmaterialized content (Lustre's `NBR`) is the safe first version.

---

## Deferred

- OS-level placeholders on macOS — closed by AD-130, not deferred.
- Linux FUSE mirror mount — deferred by AD-130 with a stated shape.
- `fanotify` HSM — deferred with a stated revisit trigger.
- Copy-on-write materialization (reflink clone of the store object, with fsync-before-clone per
  git-lfs#6312) — a pure win where the filesystem supports it, and orthogonal to every decision
  above. Its own story when hydrate throughput becomes the complaint.
- Leases instead of timestamps (an explicit "I am using this" / "I am done" pair) — strictly
  better than a TTL, strictly more machinery. Revisit if the 24 h TTL proves either too eager or
  too lazy in practice.
- Windows Cloud Files API — out of scope while keeper is macOS-first + Linux server.

## Feasibility

FR-328–FR-339 are implementable within AD-122..AD-130 plus the frozen spine, **with no new
crates**: the glob machinery is already resolved in `keeper-sync` (`gix-attributes`, `gix-quote`,
and the existing `GlobSet` used by `LfsPolicy`), the transport, journal, ledger, remote proof and
atomic-publish primitives all ship today (research §9), and the only new OS-facing question — a
race-free "is this file open" answer — is a `SyncPlatform` method whose honest default is
`Unknown`, which AD-125 turns into a refusal rather than a guess.

The riskiest seams, in order: **dehydrate's five refusals** (AD-125 — this is where data is lost
if it is wrong), **the index-stat repair after a release** (AD-125's last clause — get it wrong
and every dehydrated path reads MODIFIED forever, which is DW-140's shape), and **the sweep's
budget and success-edge placement** (AD-126 — get it wrong and a policy edit turns one sync into
an hour). Each is testable against a real repository fixture with real bytes, which is the
standard this tree already holds itself to; the recurring lesson recorded in
`sprint-status.yaml` — that a story asserting its central claim through a pure function while the
risk lives in the impure shell comes back `incorrect` — applies hardest to AD-125.
