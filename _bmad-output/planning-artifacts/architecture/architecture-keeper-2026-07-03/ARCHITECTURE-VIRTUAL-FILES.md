---
name: 'keeper'
type: architecture-spine-companion
purpose: build-substrate
altitude: initiative
paradigm: 'hexagonal Rust core + unidirectional view-model projection — unchanged; a virtual file is a state of an existing LFS path, not a new domain'
scope: 'keeper virtual files — LFS-tracked content a clone knows about but does not hold: a per-path virtualization policy, metadata without bytes, explicit hydrate/dehydrate verbs, a release clock keyed on the sync-confirmed edge, and the states a Files row must be able to show'
status: final
created: '2026-08-22'
binds: [FR-328..FR-345, NFR-40, NFR-41]
sources:
  - _bmad-output/planning-artifacts/research-virtual-files-2026-08-22.md
  - _bmad-output/planning-artifacts/research/virtual-files-2026-08-22/
  - docs/sync.md §4, §8, §12
parent: ARCHITECTURE-SPINE.md
---

# Architecture Companion — Virtual files

Extends the frozen spine with **AD-122..AD-134** (AD-131..AD-134 added 2026-08-22, on the owner's
second pass). Nothing here renegotiates it: large content
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

## Architecture decisions AD-122 … AD-134

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
- **Amended 2026-08-22, and narrowed by AD-131.** For content **this clone authored** — created or
  modified locally — last use is the wrong clock, and AD-131 replaces it with the sync-confirmed
  edge. Everything else in this decision stands unchanged, and AD-131 states the reason.
- **Decision.** The existing `materialized (profile_id, path, at_ms)` ledger (`db.rs:142-147`)
  gains `last_used_ms`, `pinned` and the object's `oid`/`size_bytes`. The release sweep runs
  **on the same success edge `prune_lfs_store` already rides** — `mark_synced`
  (`engine.rs:3185-3197`, the prune call at `:3188-3196`) — after the queue has drained and the
  push has landed.
- **Why not atime.** Linux has defaulted to `relatime` since 2.6.30, and `noatime`/`lazytime` are
  common; a TTL keyed on atime systematically mis-retains (research §6.6). keeper writes its own
  timestamp at materialize and at every use it can observe (an IPC open, a `sync_open_entry`, a
  media-protocol read).
- **Why an edge and not a clock of this decision's own.** One clock per host process is a standing
  rule in this tree: the notes cadence rides the ~1 Hz supervisor tick rather than owning a timer,
  because *"two schedulers over one git repository is how you get concurrent index locks"* (AD-62,
  `keeper/src/notes_vault.rs:2578-2582`; the desktop tick itself, `keeper/src/lib.rs:509-541`;
  the engine's, `TICK_MS`, `engine.rs:338`). Every periodic thing the engine already does is a
  due-gate on that tick — `scan_is_due` (`engine.rs:1304`) paced by the profile's own
  `poll_interval_ms`, `sweep_is_due` (`engine.rs:1390`) paced by `SWEEP_EVERY_MS`
  (`engine.rs:352`).
- **The anti-timer stance, read correctly.** An earlier draft of this decision cited
  `docs/sync.md:889-893` as a general refusal of timer-driven work. It is not: it refuses exactly
  one thing — replacing the running daemon's **binary** unattended, because the daemon *"can be
  mid-push at any moment"* — and it says so about `update`, restated at `docs/sync.md:1066-1067`.
  Scheduled *work* is already an endorsed invocation mode in this tree: `keeper-syncd sync --once`
  is documented as *"the cron entry point"* (`keeper-syncd/src/commands.rs:232`) and
  `verify --remote` *"exits non-zero so a cron wrapper sees it"* (`docs/sync.md:325-326`). An
  external scheduler is therefore a **supported second driver** of release, not a violation of
  anything — AD-135…AD-137 give it a shape, and AD-133 makes automatic release one of three modes
  rather than the only one.
  "Nightly" remains expressible honestly without any scheduler at all: **the first successful sync
  after the TTL expires**. A folder that never syncs never releases, which is the correct
  direction — nothing was proven about the remote either.
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

### AD-131 — For content this clone authored, the release clock starts at the sync-confirmed edge

- **Binds:** FR-341; Epic 56. **Narrows AD-126.**
- **Decision.** A materialized path has **two** possible clocks, and which one applies is decided
  by provenance, not by configuration:
  - **Arrived from the remote and never modified here** — the clock is `last_used_ms` (AD-126).
    The bytes exist upstream by construction; releasing them loses nothing.
  - **Created or modified locally** — the clock starts at the moment keeper *confirmed* the
    content reached the remote for that exact path, and the TTL (default 24 h, per profile,
    `0` disables) runs from there. Until that confirmation exists the path is **not eligible at
    any age**.
- **Why the existing ledger cannot answer this.** `materialized (profile_id, path, at_ms)`
  (`db.rs:142-147`) records *"paths this clone has ever held real content for"* — its own doc says
  the row is *"written when content lands"*. That is the materialization clock. Nothing in the
  schema records when a path's content was last confirmed present upstream: `journal` is a work
  queue whose rows are **deleted** on success (`db.rs:826-832`, *"the only place work leaves the
  journal"*), and `activity` is documented as *"a human-facing log, not a source of truth"*
  (`db.rs:99-101`). So the sync-confirmed clock is a **new column** on `materialized`
  (`synced_at_ms`, `NULL` = never confirmed), written where the proof is already obtained.
- **Where it is written.** Not at `mark_synced` — that edge is per *profile*, and a profile-wide
  "it worked" says nothing about one path. It is written where the per-path fact is already known:
  the upload unit's completion for a locally-authored object, and `lfs::audit`'s per-object
  `download`-operation answer (`audit.rs:29-31`) for the general case. This is the same proof
  AD-125's fifth refusal already requires, so the column is a **memo of a proof keeper already
  pays for**, not a new round trip.
- **Why this is safer, not merely what was asked.** The owner asked for "24 h after a positive
  sync". The dangerous case is the one the request implies: a file created locally, materialized
  by definition, whose only copy is here. AD-125's remote-proof refusal already blocks that
  release — but a TTL keyed on *use* makes such a path eligible after a day of not being touched
  and then relies entirely on that one refusal holding. Keying the clock on the confirmation
  itself means the sweep never even **considers** a path whose bytes are not known to be
  elsewhere. Defence in depth, in the one place data loss happens (NFR-40).
- **Rejected — one clock for both cases.** A single `last_used_ms` cannot express "never
  confirmed", and a single `synced_at_ms` would refuse to release remote-origin content that was
  never re-pushed, which is most of it. Two clocks, one selector, and the selector is a fact the
  ledger already has to know.

### AD-132 — Policy and TTL are configured where both hosts read: the folder TOML tier

- **Binds:** FR-344; Epic 56. **Extends AD-122's precedence order.**
- **Decision.** The virtualization policy and its TTL are read from, in ascending precedence: the
  committed root-level pattern file (AD-122), the folder's `.keeper/keeper.toml` `[folder]` table,
  the folder's `keeper.<host>.toml`, and last the host's own profile record. The **folder TOML
  tier is the canonical home** for anything the owner wants both surfaces to honour.
- **Why, and this is the fact that makes it necessary.** The desktop app and `keeper-syncd` do
  **not** share a profile store. The app keeps profiles as a JSON blob per row in the `profiles`
  table of `sync.db` (`db.rs:61-67`); the daemon keeps them as `[[profile]]` tables in
  `~/.config/keeper-sync/config.toml` (`keeper-syncd/src/config.rs:44-47`,
  `keeper-syncd/src/platform.rs:32`). They do not even share a data dir (`ipc.rs:651-656` vs
  `keeper-syncd/src/platform.rs:77-81`). A pattern list typed into the app's folder form is
  therefore **invisible to the daemon**, and the reverse. The one config surface both sides read
  on every profile load is the six-tier TOML stack (`keeper-core/src/config/mod.rs:13-20`,
  `keeper-sync/src/profile/folder.rs:4-7`), whose `[folder]` table is folded by the *same* key
  functions the daemon uses for `[[profile]]` (`keeper-syncd/src/config.rs:205-209`) — *"two
  readers of one shape is how a key comes to mean two things"* (`profile/mod.rs:16-20`).
- **Consequence for the app's save path.** Every new field on `SyncProfileReq`
  (`keeper/src/sync_ipc.rs:559`) is `Option`, because `parse_req` (`:838`) starts from
  `prior.clone()` and *"anything this function does not carry is erased on save"*. A
  `Vec<String>` of patterns sent unconditionally by a form that does not render the control is
  DW-116 verbatim — the bug where *"every save from the app silently pulled the cadence back to
  15 s"* (`sync_ipc.rs:833-837`), whose regression test is
  `saving_an_edit_does_not_reset_a_daemon_configured_scan_cadence` (`sync_ipc.rs:3889-3893`).
- **Consequence for the UI.** A surface that shows the policy must also show **which tier is in
  force**, because a TOML layer outranks the form and keeps winning on every read (AD-98). The
  vocabulary exists: `ConfigTierVm` (`keeper-core/src/vm.rs:4691-4705`) and `config_layers`
  (`keeper/src/ipc.rs:1748-1780`).

### AD-133 — The wire carries one absolute deadline; the countdown is rendered, never shipped

- **Binds:** FR-340, FR-342, FR-343; Epic 56
- **Decision.** `FilesEntrySyncVm` gains, beside `status`: the honest size (AD-127), a
  **modification time**, and — for a materialized path with a live TTL — **one absolute epoch-ms
  deadline** (`releases_after_ms`). The remaining-time text is computed in the frontend from
  `deadline − Date.now()`. Rust ships a moment; TypeScript renders a duration.
- **Why the split, given this tree composes its sentences in Rust.** The rule that status wording
  lives in Rust (`sync-status-mark.tsx:20-23`) exists so two surfaces cannot disagree. A
  countdown is the one string Rust cannot own: it is stale the instant it is serialized, and the
  Files tree **does not poll at all** — its listings are on-demand (`files-pane.tsx:725-729`;
  no interval in `src/lib/stores/files-tree.ts`), unlike the Sync pane's 2 s/5 s pollers
  (`sync.ts:81,88`, `sync-detail.ts:102`). The precedent for "a clock is a rendering concern" is
  already stated in this tree (`note-row.tsx:7-8`, `src/lib/format-time.ts:73-79`), and the
  Sync pane already renders elapsed time from a timestamp this way (`syncPendingReason` /
  `formatSyncWaited`, `sync-pane.tsx:613-623`).
- **The tick is owned once by the pane, not by the row.** Files rows are windowed
  (`useWindowedRows`, `files-pane.tsx:127`), so a per-row `setInterval` would arm and disarm on
  every scroll. The existing shape to copy is `UndoSendPill`: one shared 1 s interval plus a pure
  `secondsLeft(deadlineMs, now)` helper (`undo-send-pill.tsx:26-28,41-48`), with the
  `motion-reduce` and announce-once rules it already argues.
- **The states are four, and each needs a shape.** `FilesSyncStatusVm`
  (`keeper-core/src/vm.rs:3812`) and its engine-side source `EntrySyncStatus`
  (`keeper-sync/src/browse.rs:158`) both grow: **virtual**, **materializing**, **materialized**.
  Three exhaustive `Record<FilesSyncStatusVm, …>` maps make that a compile error until each state
  has a label, a glyph and a tone (`sync-status-mark.tsx:40,49,71`) — and the house rule is that
  **shape carries the distinction, colour is emphasis only** (`sync-status-mark.tsx:4-11`). The TS
  mirror is ts-rs-generated and gated (`bindings:check`, `package.json:24`); a 64-bit deadline
  field must carry `#[ts(type = "number")]` or it arrives as `bigint` and every comparison in the
  countdown breaks (`keeper-core/src/notes/vm.rs:13-16`).
- **`materializing` is the first in-flight state a Files row has ever had.** The pane deliberately
  has no loading flag — *"a node with no listing yet IS the in-flight state"* (`files-pane.tsx:726-728`)
  — so this is new surface, and it must not invent a percentage: an unknown byte total renders
  indeterminate, the way the Sync section already does (`sync-section.tsx:552-556`).

### AD-134 — Every consumer that branches on sync status must classify the virtual states explicitly

- **Binds:** FR-345, NFR-40; Epic 56
- **Decision.** Adding a state to `FilesSyncStatusVm` is not complete when the row renders. Every
  existing `match`/`matches!` over that enum must be revisited in the same story, and the
  **delete confirmation is the one that can lie**: `FilesDeletePlanVm::compose`
  (`keeper-core/src/vm.rs:4402-4422`) counts a file as travelling only if its status is
  `Synced | Waiting | Unknown`. A new variant falls into the *"stays on this machine"* bucket by
  default — so keeper would tell the user a deletion is local while it removes the pointer that
  is the tracked content, and the deletion travels.
- **Why this is stated as its own decision.** The same function's doc already argues the
  principle for `Unknown`: of the two available guesses, *"only one of them is safe to be wrong
  about"*, and picking the quiet one *"would be the same lie `Unknown` was introduced to refuse"*
  (`vm.rs:4396-4401`). A virtual path is the strongest case of it: deleting a pointer is a
  content deletion that looks like deleting 130 bytes.
- **Consequence.** A virtual or materialized path **travels**. The plan says so, and the story
  that adds the states carries the test that pins it.

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
- A scheduler of any kind — moved OUT of this companion rather than deferred: the owner's
  cron-task ask is its own subsystem with its own hosts, its own records and its own UI, and it
  lives in `ARCHITECTURE-SCHEDULED-TASKS.md` (AD-135..AD-137) and Epic 57. This companion's only
  obligation to it is AD-133's third release mode.

## Feasibility

FR-328–FR-345 are implementable within AD-122..AD-134 plus the frozen spine, **with no new
crates**: the glob machinery is already resolved in `keeper-sync` (`gix-attributes`, `gix-quote`,
and the existing `GlobSet` used by `LfsPolicy`), the transport, journal, ledger, remote proof and
atomic-publish primitives all ship today (research §9), the ledger's new columns are the additive
`ensure_*_column` idiom already in `db.rs:156-158`, the countdown reuses a tick shape that already
exists (`undo-send-pill.tsx:41-48`), and the only new OS-facing question — a race-free "is this
file open" answer — is a `SyncPlatform` method whose honest default is `Unknown`, which AD-125
turns into a refusal rather than a guess.

The riskiest seams, in order: **dehydrate's five refusals** (AD-125 — this is where data is lost
if it is wrong), **the index-stat repair after a release** (AD-125's last clause — get it wrong
and every dehydrated path reads MODIFIED forever, which is DW-140's shape), and **the sweep's
budget and success-edge placement** (AD-126/AD-131 — get it wrong and a policy edit turns one sync
into an hour), and **the delete plan's new bucket** (AD-134 — get it wrong and a confirmation
dialog tells the user a deletion stays local while it travels). Each is testable against a real
repository fixture with real bytes, which is the
standard this tree already holds itself to; the recurring lesson recorded in
`sprint-status.yaml` — that a story asserting its central claim through a pure function while the
risk lives in the impure shell comes back `incorrect` — applies hardest to AD-125.
