# Epic 56 — The file is there, even when it is not

created: '2026-08-22'
source: the owner's virtual-files request, researched before this spine was written — four web research passes (git-native selective fetch, OS placeholder technologies, eviction safety, metadata and pattern-file design) and three read-only repository scouts. Research: `research-virtual-files-2026-08-22.md`; run folder `research/virtual-files-2026-08-22/`.
binds: FR-328…FR-339, NFR-40, NFR-41; AD-122…AD-130 (new, `architecture/architecture-keeper-2026-07-03/ARCHITECTURE-VIRTUAL-FILES.md`); AD-46 (untouched), AD-52 (untouched), `lfs_prune_local` (untouched)

## What he said

> *tak zeby niektore pliki (zdefiniowane w pliku albo konfiguracji jak gitignore) z lfs nie
> byly pobierane na lokalnym klonie*
>
> *wciaz chce miec informacje ze te pliki tam sa (moze byc pod postacia pointera jesli trzeba
> — przydalyby sie metainformacje)*
>
> *jak potrzebuje pliku to chce miec opcje jego zmaterializowania — a po uzyciu zwolnienia
> materializacji — moze byc 24h po uzyciu — moze byc w conocnym skrypcie*
>
> *chce zeby to bylo szybkie i efektywne, ale rowniez proste i przedewszystkim bezpieczne*
>
> *Glownie bede uzywal na serwerze (keeper-sync, ale desktopowa apka tez) — jakby byla jakas
> systemowa wersja ktora pokazuje wirtualnie pliki w Finderze czy w ls to by bylo super.*

Four asks and a wish. The first three are 80% built already and nobody knew — the fourth is
the one the research closes, and it closes it as *no*, on macOS, structurally.

## Verdicts

| # | ask | verdict | mechanism |
|---|---|---|---|
| 1 | some LFS files are not downloaded, selected gitignore-style | **half-present** | `LfsMode::PointerOnly` already leaves every LFS path as pointer text (`profile/mod.rs:81-89`, applied `engine.rs:5025,5037`) and `subpaths[]` already gates transfers per subtree (`engine.rs:4984-4998`). What is missing is the **selector**: a per-path policy. `lfs_never` (`profile/mod.rs:813`) is the right *shape* — a gitignore-dialect GlobSet compiled with refuse-on-typo (`stage.rs:124-152`) — and exactly the wrong *meaning* |
| 2 | still know the file is there, with metadata | **present but unwired** | the pointer carries `oid` and exact `size`; `stage::indexed_size` (`stage.rs:800-820`) and `indexed_pointer` (`stage.rs:792-798`) already answer both without loading content. `browse.rs:613-626` ignores them and reports `fs::metadata().len()` — so a virtual file renders as 130 bytes today |
| 3 | materialize on demand | **present, with no door** | `lfs::stage::materialize` (`stage.rs:1118-1143`) is atomic (`.keeper.*.tmp` + rename) and ships. `Engine::materialize_pending` (`engine.rs:4998-5069`) is the one decision point. There is **no CLI verb and no IPC command** that reaches either, and `claim_ready` has no urgency dimension so a click queues behind `CLAIM_LIMIT = 16` background units per tick (`db.rs:772-776`, `engine.rs:331-336`) |
| 4 | release the materialization, ~24h after use | **absent, and the nearest thing is its inverse** | `lfs::prune` releases the *store* copy while requiring that *"the worktree still holds the real content"*, and states that a path whose worktree content is pointer text *"is never a candidate"* (`prune.rs:28-33`). Dehydration is the opposite operation: afterwards the store object is the **only** local copy. Building it by relaxing that predicate converts a safe operation into a deleting one |
| 4b | "moze byc w conocnym skrypcie" | **no cron, and better without one** | no `.timer` unit exists anywhere in the repository; the daemon's `ExecStart` is `keeper-syncd watch` (`keeper-syncd.service:33`), and the tree already refuses timer-driven work on this daemon one layer over, because it *"can be mid-push at any moment"* (`docs/sync.md:889-891`). But the ledger for a lazy sweep already exists: `materialized (profile_id, path, at_ms)` (`db.rs:142-146`) is a per-path timestamped materialization record, written by `remember_materialized` (`db.rs:324-338`). The sweep rides the success edge `prune_lfs_store` already rides (`engine.rs:3052-3066`) |
| 5 | a system-level version showing virtual files in Finder / `ls` | **rejected on macOS, deferred on Linux** | `NSFileProviderReplicatedExtension` gives exactly these semantics — dataless items, `fetchContents` on read, `evictItem` — but its storage is exposed under `~/Library/CloudStorage/<Provider>` and its container path is relative to an app-group container. There is no API to virtualize a path the user chose. `SF_DATALESS` *"may not be set or unset from user space"* (`chflags(2)`). Kexts are policy-dead. macFUSE's licence forbids redistribution bundled with commercial software; fuse-t is paid for commercial use — both fail `cargo deny`. See AD-130 |

## The one finding that shapes everything else

git-lfs shipped ask 1 and ask 4 as **one mechanism** and lost data for it. `lfs.fetchexclude` —
a gitignore-style list of paths not to download — began outranking *"referenced by the current
checkout"*, so an excluded path's object was pruned (git-lfs#3092). It is not an isolated bug:
every documented data-loss incident in this space is a reachability-enumeration bug — staged
objects (git-lfs#5636, where *"`--verify-remote` does not prevent this"*), stash-only objects
(#4206), git-annex's `--all`/`--key`/`--unused` bypassing `numcopies` without `--force`, `dvc gc`
breaking a shared cache.

So AD-123 splits them, and the split is the epic's spine: **the pattern file authorizes
hydration decisions. Only per-object proof authorizes deletion.** Editing the pattern file can
never delete a byte. It changes what future arrivals materialize, and it makes existing
materializations *eligible* — after which each object must still pass five refusals (AD-125).

The two designs that demand proof are the two that have not lost data: git-annex refuses to drop
unless it can verify copies elsewhere (*"Could only verify the existence of 0 out of 1 necessary
copies"*), and `git lfs prune` fails closed when `origin` is absent because *"everything is
treated as 'unpushed'"*. keeper can do better than both, because it already has the proof
primitive: `lfs::audit` asks the remote's batch API with the `download` operation, whose
per-object 404 is the server saying *"I cannot serve this"* (`audit.rs:29-31`).

## What is deliberately not built

- **On-read hydration.** A `grep -r`, Spotlight, a backup agent, an antivirus scanner or a `du`
  walks the tree and hydrates everything; Microsoft documents *"large-scale hydration and
  unexpected data consumption"*, Nextcloud shipped an infinite implicit-hydration loop (#7747),
  and Lustre HSM ships `NBR` — return `ENODATA` rather than restore. keeper already chose this
  side for iCloud placeholders (`docs/sync.md:163-168`). Materialization is a verb somebody calls.
- **A fourth `LfsMode`.** `MediaPolicy` (`profile/mod.rs:364-389`) is this tree's own precedent
  that a differently-scoped answer gets its own type.
- **Anything in the pointer.** Its encoding is unique, so an added key changes the blob OID —
  a content change wearing an annotation's clothes.
- **xattr identity.** `rsync` needs `-X` and copies only `user.*` as a non-root user; a stub
  identified only by an xattr becomes an anonymous file after one copy.
- **`fanotify` pre-content HSM.** The only mechanism that could decorate paths in place, and not
  ready: kernel ≥ 6.14, `CAP_SYS_ADMIN`, `mmap` materializes whole files because the page-fault
  hook was merged and then backed out, directory events deadlock, and every read re-fires because
  the BPF suppression is unimplemented. Revisit trigger recorded in AD-130.

## What the binds mean

FR-328…FR-339 and NFR-40/NFR-41 are allocated here; FR-327 and NFR-39 were the previous
ceilings. Each names one observable behaviour so a spec can cite it and a reviewer can check it.

| id | statement | story | AD |
|---|---|---|---|
| FR-328 | A folder may declare, in a committed root-level file in gitignore dialect, which paths are allowed to stay unmaterialized after a pull | 56.1 | AD-122 |
| FR-329 | The profile's own configuration overrides that file, and a malformed pattern refuses at startup with the pattern quoted | 56.1 | AD-122 |
| FR-330 | Editing the policy never deletes content; it changes what future arrivals materialize and what becomes eligible for release | 56.1, 56.5 | AD-123 |
| FR-331 | A virtual file's worktree bytes are exactly the committed pointer, and the path reads clean in `git status` | 56.1 | AD-124 |
| FR-332 | keeper can turn a materialized path back into its pointer, atomically, without disturbing a reader holding it open | 56.4 | AD-125 |
| FR-333 | A release refuses, by a distinguishable typed error, when the path is modified, open, unproven on the remote, pinned, or already a pointer | 56.4 | AD-125 |
| FR-334 | A materialization carries keeper's own last-use timestamp, and a path may be pinned against release | 56.2, 56.5 | AD-126 |
| FR-335 | Materializations older than a per-profile TTL (default 24 h, `0` disables) are released on the next successful sync, under a per-pass budget, and a failure never fails the sync | 56.5 | AD-126 |
| FR-336 | Every keeper surface reports a virtual file's true size and oid from the index and pointer, never the worktree stat | 56.2, 56.7 | AD-127 |
| FR-337 | A listing states what is virtual, what is materialized and when it was last used, in a human form and a stable JSON form; remote presence only on request | 56.2 | AD-127 |
| FR-338 | A human or an agent can materialize one path on demand, from the daemon or the app, with progress, idempotently, and a modified file is never overwritten | 56.3 | AD-128 |
| FR-339 | `verify` distinguishes intentionally-virtual from unredeemable, and a verified copy of a virtual file hydrates or refuses by name — never copies pointer text silently | 56.6 | AD-129 |
| NFR-40 | No operation may make an object's only remaining copy unreachable. Every deletion is authorized by per-object proof at the moment of deletion, never by a pattern, an age or a ref comparison | 56.4, 56.5 | AD-123, AD-125 |
| NFR-41 | The virtual state is never reported as a fault. A folder whose policy leaves 10 000 paths virtual shows no errors and no warnings for that fact alone | 56.6 | AD-129 |

## Why the suite cannot see the risk in this epic

Stated up front because `sprint-status.yaml` already records the lesson twice: *every story that
came back `incorrect` asserts its central claim through a pure function or a hand-placed input
while the risk lives in the impure shell.*

For this epic the impure shell is: a real repository with real bytes, a real `rename(2)`, a real
index whose stat tuple must still read clean afterwards, a real open file descriptor, and a real
batch round trip that answers 404. A `dehydrate()` unit test over a `TestPlatform` and a
hand-written pointer proves nothing about the two failures that matter — a released file that
reads MODIFIED forever (DW-140's shape) and a released file whose bytes existed nowhere else.
**56.4 and 56.5 must assert against a real git fixture and a real process holding the file open**,
in the manner of story 34.11, which is the model this tree names.

## Stack order

    56.1  a policy that says which files may stay away    (VirtualPolicy, the committed pattern file, profile override, refuse-on-typo, size floor)
    56.2  a listing that knows what it does not hold      (indexed_size/oid everywhere, EntrySyncStatus::Virtual, the ledger's new columns, human + stable JSON)
    56.3  a file you can ask for                          (materialize verb: CLI + IPC, journal reuse, urgency over CLAIM_LIMIT, progress, never overwrite a modified file)
    56.4  a release that refuses five times before it deletes  (dehydrate beside materialize, the five refusals, remote proof, rename discipline, index-stat repair)
    56.5  it lets go a day later, on its own              (last_used_ms, pin, TTL, per-pass budget, on the success edge, never fails a sync)
    56.6  the checks stop calling the normal state a fault (verify, copy.rs refusal-or-hydrate, the re-clean check)
    56.7  the row says what it is, and what you can do    (Files pane badge + materialize/release/pin actions, Sync pane pending, honest sizes)
    56.8  docs/sync.md grows a virtual-files chapter      (§8 sibling: the states, the five refusals, the TTL, what ls and du will lie about, and what macOS will never do)

56.1 → 56.2 → 56.3 → 56.4 → 56.5 is a strict chain: each needs its predecessor's type. 56.6 and
56.7 are disjoint from each other and depend on 56.2 (state vocabulary) and 56.4 (the verb they
offer). 56.8 last, and only after 56.5 — a documented TTL that does not yet exist is the one
sentence a reader will act on.

## Acceptance, per story

**56.1** — a repository-relative path resolves to `Virtual` or `Materialize` by policy alone, with
no network and no bytes read. A malformed glob fails `Engine::open` with the pattern quoted. The
profile's configuration overrides the committed file, and the committed file is read from the
worktree at run start, never from `HEAD`. `lfs_never` is untouched and still means what it meant.

**56.2** — for a path holding pointer text, every keeper surface reports the pointer's size and
oid, never 130 bytes: `browse`, the Files pane VM, `pending`, and a new listing with a human form
and a `--json` form whose schema is the contract. Remote presence is absent from the listing
unless explicitly asked for. The ledger carries `last_used_ms`, `pinned`, `oid`, `size_bytes`, and
its migration is the `meta`-marker + `json_set` idiom already in `db.rs:161-205`.

**56.3** — `keeper-syncd materialize <profile> <subpath>` and `sync_materialize_entry` both land
real bytes for a virtual path, report progress through the existing `SyncPhase::DownloadingLfs`
sink, are idempotent under a repeat request, and **refuse a path whose worktree content is
modified**. A user-requested unit is claimed ahead of background units in the same tick. The path
reads clean in `git status` afterwards.

**56.4** — `dehydrate` turns a materialized path back into its exact committed pointer, and
refuses by name on each of: modified, open, remote-unproven, pinned, already-pointer. The refusal
is a typed error a caller can distinguish, not a log line. After a successful release the path
reads **clean**, not modified, on a real fixture — and on a platform with no race-free open-file
answer, the operation refuses rather than guesses.

**56.5** — a materialized path whose `last_used_ms` is older than the TTL is released on the next
successful sync, subject to the per-pass budget and every 56.4 refusal; a pinned path never is; a
sweep failure is logged and never fails the sync. Default TTL 24 h, configurable per profile, and
`0` disables the sweep entirely (the `lfs.pruneoffsetdays` convention: a zero disables that
condition rather than meaning "immediately").

**56.6** — `verify` distinguishes intentionally-virtual from unredeemable and stops reporting the
first; `keeper-syncd verify --remote` still finds real loss. A verified copy of a virtual file
either hydrates first or refuses by name — it never copies 130 bytes of pointer text silently.

**56.7** — a virtual row is visibly virtual, shows its true size, and offers materialize /
release / pin; a materialized row offers release. Nothing promises a finish time.

**56.8** — the chapter states the four states, the five refusals, the TTL and its budget, that
`ls`, `du` and third-party apps will see ~130 bytes and why that is the only representation
`git status` tolerates, and that Finder integration is closed rather than pending — with the
reason, so it is not re-asked.
