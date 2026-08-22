# Epic 56 — The file is there, even when it is not

created: '2026-08-22'
source: the owner's virtual-files request, researched before this spine was written — four web research passes (git-native selective fetch, OS placeholder technologies, eviction safety, metadata and pattern-file design) and three read-only repository scouts. Research: `research-virtual-files-2026-08-22.md`; run folder `research/virtual-files-2026-08-22/`.
binds: FR-328…FR-345 (allocated here; FR-340…FR-345 added by the owner's second pass, 2026-08-22), NFR-40, NFR-41; AD-122…AD-134 (new, `architecture/architecture-keeper-2026-07-03/ARCHITECTURE-VIRTUAL-FILES.md`); AD-126 (narrowed here by AD-131, explicitly); AD-62 (untouched, and load-bearing), AD-98 (untouched), AD-46 (untouched), AD-52 (untouched), `lfs_prune_local` (untouched)
see-also: Epic 57 (`epic-57-a-task-that-runs-when-it-should.md`) — the scheduler the fourth ask asked for, split out with its own ADs

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
| 4b | "moze byc w conocnym skrypcie" | **cron is already supported; the sweep still does not need it** — corrected 2026-08-22, see below | The ledger for a lazy sweep exists: `materialized (profile_id, path, at_ms)` (`db.rs:142-147`), written by `remember_materialized` (`db.rs:328-338`). The sweep rides the success edge `prune_lfs_store` already rides — `mark_synced` (`engine.rs:3185-3197`). **The first draft of this row was wrong twice:** it read `docs/sync.md:889-893` as a general refusal of timer-driven work (it refuses exactly one thing — replacing the daemon **binary** unattended, restated at `:1066-1067` about `update`), and it cited `engine.rs:3052-3066` for the success edge, which is `adopt_volume`. Scheduled work is endorsed in this tree: `keeper-syncd sync --once` is *"the cron entry point"* (`keeper-syncd/src/commands.rs:232`) and `verify --remote` *"exits non-zero so a cron wrapper sees it"* (`docs/sync.md:325-326`) |
| 5 | a system-level version showing virtual files in Finder / `ls` | **rejected on macOS, deferred on Linux** | `NSFileProviderReplicatedExtension` gives exactly these semantics — dataless items, `fetchContents` on read, `evictItem` — but its storage is exposed under `~/Library/CloudStorage/<Provider>` and its container path is relative to an app-group container. There is no API to virtualize a path the user chose. `SF_DATALESS` *"may not be set or unset from user space"* (`chflags(2)`). Kexts are policy-dead. macFUSE's licence forbids redistribution bundled with commercial software; fuse-t is paid for commercial use — both fail `cargo deny`. See AD-130 |

## The second pass — what he said next

> *wirtualne pliki chce zeby byly managowane przez keepera - keeper-sync i przez desktop app -
> chce zeby byla konfiguracyjne opcje (jakie pliki maja byc wirtualne, ile maja byc
> zmaterializowane do usuniecia - 24h by default po pozytywnym zsynchronizowaniu jezeli zostal
> zmodyfikowany lub stworzony lokalnie)*
>
> *Chce miec metadane dostepne lokalnie (nazwa, modyfikacja, size itp - moze dane sa w git)*
>
> *W ui keepera desktop chce widziec te pliki w files i ikonka powinna miec informacje ze plik
> jest wirtualny, wirtualny-zmaterializowany, sciagany-materializujacy. chce miec tez przycisk to
> zmaterializowania i licznik czasu kiedy jeszcze bedzie lokalnie*
>
> *Usuwanie nie musi byc automatyczne, moze to byc skrypt i puszczany w odpowiednim czasie (cron
> job like) - chce miec tez opcje w keeperze do uruchamiania croon taskow na sync i desktop -
> zeby byl opcje i widok w ui do taskow*

Most of this the epic already had. **Four things it did not**, and one of them is a scheduler that
is not this epic at all.

| # | what is new | verdict | why |
|---|---|---|---|
| a | the TTL runs from the **successful sync**, for content created or modified locally | **a different clock, not a tuning** | `materialized (profile_id, path, at_ms)` is the *materialization* clock — its own doc says the row is *"written when content lands"* (`db.rs:133-147`). Nothing in the schema records when a path was last **confirmed present upstream**: `journal` rows are `DELETE`d on success (`db.rs:826-832`) and `activity` is *"a human-facing log, not a source of truth"* (`db.rs:99-101`). A new `synced_at_ms` column, and AD-131 narrows AD-126 to say which clock applies when. It is also the **safer** reading — see below |
| b | configuration honoured by **both** keeper-sync and the desktop app | **impossible as first drafted** | the two hosts do not share a profile store: the app keeps profiles as a JSON blob per row in `sync.db`'s `profiles` table (`db.rs:61-67`), the daemon keeps `[[profile]]` tables in `~/.config/keeper-sync/config.toml` (`keeper-syncd/src/config.rs:44-47`), and they do not share a data dir either (`ipc.rs:651-656` vs `keeper-syncd/src/platform.rs:77-81`). Patterns typed into the app's form would be invisible to the daemon. The one surface both read on every profile load is the six-tier TOML stack (`keeper-core/src/config/mod.rs:13-20`) — AD-132 |
| c | **modification time** in the local metadata | **absent from the wire, free to add** | `FilesEntryVm` carries `name, relativePath, absolutePath, kind, sync, size, folderRole, write` and nothing time-shaped. `browse.rs` documents `size_bytes` as **free** — *"the `stat` behind it is the same one `is_dir` already paid for"* (`browse.rs:110-116`) — and mtime comes off that same `stat`. For a virtual path the honest size still comes from the pointer (AD-127); mtime is the worktree's, and that is the correct answer |
| d | three icon states, a materialize button, and a **countdown** of time left locally | **two of three exist as patterns; the countdown is new** | `FilesSyncStatusVm` has five variants and three exhaustive `Record` maps that make a new state a compile error until it has a label, a glyph and a tone (`sync-status-mark.tsx:40,49,71`); a row action is one entry in the `actions` array and appears in both the hover cluster and the context menu (`files-pane.tsx:635,1695-1740,1988-2008`). But **no Files row has ever had an in-flight state** — the pane refuses a loading flag on purpose (`files-pane.tsx:726-728`) — and the Files tree **does not poll at all**, so a countdown must be client-derived from one absolute deadline shipped by Rust (AD-133) |
| e | cron tasks on sync **and** desktop, with options and a Tasks view | **a subsystem, not a story here** | there is no task record anywhere: no name, no schedule, no last-run time, no result. Periodicity exists as due-gates on each host's existing tick (`engine.rs:1304,1390`; the app's own 1 Hz tick at `keeper/src/lib.rs:509-541`), and AD-62 forbids a second clock. Split into **Epic 57** with AD-135…AD-137, because it has its own records, its own hosts and its own UI — and because *"jakie pliki maja byc wirtualne"* must ship whether or not a scheduler ever does |

**Why (a) is a safety improvement and not just the requested behaviour.** The dangerous path is the
one the request describes: a file created locally, materialized by definition, whose bytes exist
nowhere else. AD-125's fifth refusal already blocks releasing it — but a TTL keyed on *use* makes
that path *eligible* after a day of not being touched, and then everything rests on that one
refusal holding. Keying the clock on the confirmation means the sweep never **considers** a path
whose content is not known to be elsewhere. Two independent barriers, in the one place where a bug
deletes data (NFR-40).

**What the second pass does not change.** The five refusals (AD-125), the pattern-file/deletion
split (AD-123), the pointer-is-the-state rule (AD-124), and the closed answer on Finder/`ls`
(AD-130, D-2) all stand exactly as written.

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

FR-328…FR-345 and NFR-40/NFR-41 are allocated here; FR-327 and NFR-39 were the previous
ceilings, and FR-340…FR-345 were added by the owner's second pass on 2026-08-22. Each names one
observable behaviour so a spec can cite it and a reviewer can check it. FR-346…FR-352 and
NFR-42/NFR-43 belong to Epic 57 and are not this epic's to satisfy.

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
| FR-340 | A listing reports a modification time for every entry, and for a virtual path the honest size from the pointer beside it | 56.2 | AD-133 |
| FR-341 | For content created or modified locally, the release clock starts when keeper confirmed that path reached the remote; until that confirmation exists the path is not eligible at any age | 56.5 | AD-131 |
| FR-342 | A Files row distinguishes virtual, materializing and materialized by shape and by accessible name, not by colour alone | 56.7 | AD-133 |
| FR-343 | A materialized row shows the time remaining before release, counting down live, and a virtual row offers materialize; the deadline crosses the boundary as one absolute instant | 56.9 | AD-133 |
| FR-344 | The policy and the TTL are read from the folder's committed and per-host TOML layers as well as the profile, so the daemon and the app honour the same answer; a save from either surface never erases the other's value | 56.1 | AD-132, AD-98 |
| FR-345 | A deletion plan classifies a virtual or materialized path as travelling, and says so before the deletion happens | 56.7 | AD-134 |
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

    56.1  a policy that says which files may stay away    (VirtualPolicy, the committed pattern file, the folder TOML tier, profile override, refuse-on-typo, size floor)
    56.2  a listing that knows what it does not hold      (indexed_size/oid everywhere, EntrySyncStatus::Virtual, mtime, the ledger's new columns, human + stable JSON)
    56.3  a file you can ask for                          (materialize verb: CLI + IPC, journal reuse, urgency over CLAIM_LIMIT, progress, never overwrite a modified file)
    56.4  a release that refuses five times before it deletes  (dehydrate beside materialize, the five refusals, remote proof, rename discipline, index-stat repair)
    56.5  it lets go a day after it landed, on its own    (synced_at_ms and last_used_ms, which clock applies, pin, TTL, per-pass budget, on the success edge, never fails a sync)
    56.6  the checks stop calling the normal state a fault (verify, copy.rs refusal-or-hydrate, the re-clean check)
    56.7  the row says what it is, and what a delete will do (three new states in both enums, honest size, mtime, the delete plan's bucket)
    56.9  the button, and the time you have left          (materialize/release/pin verbs on the row, the countdown from one absolute deadline, one tick per pane, indeterminate progress while materializing)
    56.8  docs/sync.md grows a virtual-files chapter      (§8 sibling: the states, the five refusals, the two clocks, what ls and du will lie about, and what macOS will never do)

56.1 → 56.2 → 56.3 → 56.4 → 56.5 is a strict chain: each needs its predecessor's type. 56.6 and
56.7 are disjoint from each other and depend on 56.2 (state vocabulary) and 56.4 (the verb they
offer). **56.9 depends on 56.7 (the states) and 56.5 (the deadline the countdown counts)** and is
the last code story. 56.8 last of all, and only after 56.9 — a documented TTL that does not yet
exist is the one sentence a reader will act on.

## Acceptance, per story

**56.1** — a repository-relative path resolves to `Virtual` or `Materialize` by policy alone, with
no network and no bytes read. A malformed glob fails `Engine::open` with the pattern quoted. The
precedence is: committed pattern file, then `<folder>/.keeper/keeper.toml`, then
`keeper.<host>.toml`, then the profile — and the committed file is read from the worktree at run
start, never from `HEAD`. Every new field on `SyncProfileReq` is `Option`, so a save from a form
that does not render the control leaves a daemon-set value alone (the DW-116 rule,
`sync_ipc.rs:833-837`); a test asserts that, in the manner of
`saving_an_edit_does_not_reset_a_daemon_configured_scan_cadence`. `lfs_never` is untouched and
still means what it meant.

**56.2** — for a path holding pointer text, every keeper surface reports the pointer's size and
oid, never 130 bytes: `browse`, the Files pane VM, `pending`, and a new listing with a human form
and a `--json` form whose schema is the contract. Every entry also carries a **modification time**,
off the `stat` the listing already pays for (`browse.rs:110-116`). Remote presence is absent from
the listing unless explicitly asked for. The ledger carries `last_used_ms`, `synced_at_ms`,
`pinned`, `oid`, `size_bytes`, added by the additive `ensure_*_columns` idiom already in
`db.rs:156-158`.

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

**56.5** — the clock is chosen by provenance, and the test asserts both branches: a path that
**arrived from the remote and was never modified here** releases when `last_used_ms` is older than
the TTL; a path **created or modified locally** is not eligible at any age until `synced_at_ms` is
set, and then releases a TTL after that instant. Release happens on the next successful sync
(`mark_synced`), subject to the per-pass budget and every 56.4 refusal; a pinned path never is; a
sweep failure is logged and never fails the sync. Default TTL 24 h, configurable per profile and
per folder TOML layer, and `0` disables the sweep entirely (the `lfs.pruneoffsetdays` convention:
a zero disables that condition rather than meaning "immediately"). A story that proves the TTL by
sleeping is asserting the wrong thing — the clock is injected.

**56.6** — `verify` distinguishes intentionally-virtual from unredeemable and stops reporting the
first; `keeper-syncd verify --remote` still finds real loss. A verified copy of a virtual file
either hydrates first or refuses by name — it never copies 130 bytes of pointer text silently.

**56.7** — a row is visibly **virtual**, **materializing** or **materialized**, by shape and by
accessible name, in both `EntrySyncStatus` (`browse.rs:158`) and `FilesSyncStatusVm`
(`vm.rs:3812`), with the three exhaustive `Record` maps answered (`sync-status-mark.tsx:40,49,71`)
and the bindings regenerated rather than hand-edited (`bindings:check`). It shows its true size and
its mtime. And `FilesDeletePlanVm::compose` (`vm.rs:4402-4422`) classifies the new states
**explicitly**: a virtual or materialized path **travels**, and a test pins it — the default
`matches!` bucket would tell the user a deletion stays local while it removes tracked content.

**56.9** — a virtual row offers **Materialize** and a materialized row offers **Release** and
**Pin**, added to the one `actions` array so each appears in both the hover cluster and the context
menu (`files-pane.tsx:1695-1740,1988-2008`). A materialized path with a live TTL shows the time
remaining, counting down: the wire carries one absolute epoch-ms deadline with
`#[ts(type = "number")]`, and the pane owns **one** interval for every row, in the shape
`UndoSendPill` already uses (`undo-send-pill.tsx:26-28,41-48`), because rows are windowed. While a
path is materializing the row is indeterminate — it never invents a percentage
(`sync-section.tsx:552-556`) — and nothing promises a finish time. Under `motion-reduce` the
countdown still reads as text.

**56.8** — the chapter states the four row states, the five refusals, **both release clocks and
which one applies when**, the per-pass budget, that `ls`, `du` and third-party apps will see ~130
bytes and why that is the only representation `git status` tolerates, and that Finder integration
is closed rather than pending — with the reason, so it is not re-asked. It also states, in one
sentence, that automatic release on the success edge is the default and that a scheduled or manual
release is Epic 57's `tasks` verb — so a reader looking for "the nightly script" finds where it
lives instead of assuming it does not exist.
