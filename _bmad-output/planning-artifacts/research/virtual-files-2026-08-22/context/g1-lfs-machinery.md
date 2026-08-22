# keeper's existing LFS + materialization machinery
_agent: G1LfsMap · accessed: 2026-08-22_

Read-only grounding for a virtual-files (on-demand materialization) capability. Every claim carries `path:line` against this worktree. Anything I could not establish from the repository is marked **UNVERIFIED**.

All paths are relative to `src-tauri/crates/` unless stated otherwise.

---

## 0. Shape of the subsystem

keeper ships a first-party git-LFS client; there is no `git-lfs` binary dependency (`keeper-sync/src/lfs/mod.rs:1-30`). Modules: `audit, basic, batch, endpoint, filter, local, pktline, pointer, prune, ssh, stage, store` (`keeper-sync/src/lfs/mod.rs:74-85`).

---

## 1. Pointer lifecycle — how a large file becomes a pointer, and what the worktree holds

### The routing decision

* Size alone decides, deliberately: `applies(profile, size)` is `profile.lfs_mode != LfsMode::Disabled && size >= profile.lfs_threshold_bytes` (`keeper-sync/src/lfs/stage.rs:90-92`). The doc comment above it explicitly rejects extension allow-lists — "the 6 GB `.csv` export is exactly what must not become a git blob" (`stage.rs:85-89`).
* The richer form is `LfsPolicy`, compiled once per run from the profile: `{ threshold, enabled, never }` (`stage.rs:116-121`), built in `LfsPolicy::from_profile` (`stage.rs:124-152`), applied per repository-relative path in `LfsPolicy::applies` (`stage.rs:157-166`). `lfsNever` globs use gitignore semantics — a pattern with no `/` is rewritten to `**/pattern`, otherwise it is root-anchored (`stage.rs:130-133`) — and a malformed glob is a hard `SyncError::Config`, never silently dropped, because a typo in an opt-out that quietly does nothing is how a note ends up an opaque pointer months later (`stage.rs:138-140`, motivated at `stage.rs:110-114`).

### Staging (`prepare`)

`lfs::stage::prepare(profile, store, candidates)` (`stage.rs:1016-1063`):
1. Returns empty immediately when `lfs_mode == Disabled` (`stage.rs:1022-1024`).
2. Builds the `LfsPolicy` **before** the walk so a bad glob fails the whole run rather than after an arbitrary number of files have already been routed (`stage.rs:1025-1027`).
3. `store.ensure_layout()` (`stage.rs:1028`).
4. Per candidate: `symlink_metadata`; a vanished path is skipped as an ordinary outcome (`stage.rs:1033-1036`); symlinks and non-matching sizes are skipped (`stage.rs:1039-1041`).
5. `clean(store, &absolute)` streams the file into `<git-dir>/lfs/objects`, hashing as it goes, and returns the `Pointer` (`stage.rs:1043`, implementation `stage.rs:717-739`). Nothing is ever fully buffered (`stage.rs:713-716`). A length mismatch mid-read is `SyncError::Integrity` and the caller requeues rather than committing a pointer to content that moved (`stage.rs:725-733`).
6. The rendered pointer bytes go into `staging.substitutions[rela]`, and a `StagedObject { path, oid, size }` into `staging.uploads` (`stage.rs:1044-1051`).
7. A per-extension `.gitattributes` pattern is accumulated (`pattern_for`, `stage.rs:186-199`; `pattern_for_extension`, `stage.rs:216-218`) and written by `ensure_attributes` (`stage.rs:656-...`), setting `staging.attributes_changed` (`stage.rs:1058-1060`). The attribute line is `filter=lfs diff=lfs merge=lfs -text` — matching what `git lfs track` writes, so a real client agrees with it (`stage.rs:46-49`).

`LfsStaging` is `{ substitutions: BTreeMap<PathBuf, Vec<u8>>, uploads: Vec<StagedObject>, attributes_changed: bool }` (`stage.rs:70-79`).

### Commit (`git::commit::stage_and_commit`)

Signature takes `substitutions: &BTreeMap<PathBuf, Vec<u8>>` (`keeper-sync/src/git/commit.rs:138-146`). The module doc states the precondition: content above the threshold **must already** have been replaced by its pointer, because gitoxide's `write_blob_stream` is not streaming despite the name — a 3 GB file committed here would be a 3 GB allocation (`git/commit.rs:23-27`).

The three load-bearing lines:
* the blob written is the pointer when a substitution exists (`git/commit.rs:196-202`);
* the index entry's **stat is taken from the WORKTREE file**, not from the pointer (`git/commit.rs:179-180`, `git/commit.rs:231`, and the comment at `git/commit.rs:197-201` says this is exactly why `gix::status`/`git status` read the path as unchanged without reading gigabytes back — "exactly how git+LFS itself works");
* the index is written **before** the commit so a crash between the two is re-driven on the next pass, NFR-24 (`git/commit.rs:276-280`).

### What the worktree file contains, by state

| State | Worktree bytes | Index blob | Index stat | `<git-dir>/lfs/objects` |
|---|---|---|---|---|
| Just staged/committed on the originating machine | **real content** (nothing in `stage.rs` rewrites the file) | ~130-byte pointer (`git/commit.rs:202`) | worktree file's (`git/commit.rs:231`) | holds the object (`stage.rs:1043` → `clean`) |
| After `lfs_prune_local` released it | real content | pointer | worktree's | **object deleted** (`lfs/prune.rs:143-156`) |
| Freshly checked out from a fetch, object absent | **pointer text** (`lfs/filter.rs:190-199` pass-through; also `stage.rs:1074-1077`) | pointer | pointer-sized | absent |
| Freshly checked out, object present | real content after `materialize` (`stage.rs:1118-1143`) | pointer | stale until `refresh_index_stat` (`engine.rs:5064-5067`) | holds the object |
| `LfsMode::PointerOnly`, permanently | **pointer text forever** (`engine.rs:5025-5027`, `5037-5039`; stated at `lfs/filter.rs:208-210`) | pointer | pointer-sized | may or may not hold it |

The pointer file is bounded: `pointer::MAX_POINTER_BYTES` (~1 KiB, referenced at `stage.rs:1090`, `lfs/filter.rs:88`), and `is_pointer_candidate` is a bounded 100-byte memcmp probe on the head (`lfs/pointer.rs:249-253`).

---

## 2. Pull / checkout path — where 'download this object now' is decided

### The chain

`Engine::sync_once` runs commit → pull → drain → push (`engine.rs:5209-5216`). After `do_pull` (`engine.rs:3392`) it constructs the store and calls `materialize_pending` **before** the push leg, because otherwise the scan would see a pointer-sized file where the index records the full length and call it an edit (`engine.rs:5222-5229`). A second call site is the post-apply path at `engine.rs:5994-5997` (warn-only), and a third is after a filesystem-remote copy at `engine.rs:4972-4974`.

### `Engine::materialize_pending` — the decision point

`engine.rs:4998-5069`. This is the single place where 'download this object now' is decided:

1. `lfs_mode == Disabled` → return (`engine.rs:5003-5005`).
2. Tracked paths from the index, filtered through the profile's `SparseCone::new(&profile.subpaths)` (`engine.rs:5007-5011`). The doc block at `engine.rs:4985-4996` explains this is the `fetchinclude`/`fetchexclude` idiom: **the cone filters the transfer, not just the checkout**, because a cone sparse-checkout on its own reduces no LFS traffic whatsoever — git-lfs is entirely sparse-checkout-unaware. Filtering here rather than relying on paths simply being absent is deliberate: `sparse-checkout set` refuses to remove a path with local modifications, and one stale pointer left behind would pull down the gigabytes the profile exists to avoid (`engine.rs:4991-4996`). This is today's only per-path 'do not fetch' lever.
3. `lfs::stage::pending_smudges(&profile.local_path, &tracked)` (`engine.rs:5012`) returns every worktree file that is ≤ `MAX_POINTER_BYTES`, parses as a pointer, and therefore still needs content (`stage.rs:1081-1112`). Cost is one `stat` per ordinary file (`stage.rs:1078-1080`).
4. For each pending smudge:
   * **object already in store** → `PointerOnly` skips (`engine.rs:5025-5027`); otherwise `lfs::stage::materialize` (`engine.rs:5028`) and `db::remember_materialized` — "the one moment this is knowable" (`engine.rs:5029-5033`).
   * **object not in store** → `PointerOnly` skips (`engine.rs:5037-5039`); otherwise enqueue `WorkKind::LfsDownload { oid, size }` via `db::enqueue_unique`, with the path carried **beside** the unit as a `label` so two paths sharing one object stay one download (`engine.rs:5040-5053`).
5. If anything was materialized, `git::repo::refresh_index_stat` re-stats the touched paths so status does not call them modified (`engine.rs:5061-5067`; function at `git/repo.rs:1947`).

### The transfer itself

Journal dispatch maps `WorkKind::LfsDownload { oid, size }` → `do_lfs(..., upload=false)` (`engine.rs:3010-3012`). `do_lfs` (`engine.rs:4760-…`): early-return on `Disabled` (`engine.rs:4771-4773`); publishes `SyncPhase::DownloadingLfs` with the label as `current`, because a transfer moves exactly one object and the path is the most useful thing a progress line can say (`engine.rs:4776-4788`); reads `.lfsconfig` for endpoint override (`engine.rs:4792-4803`); a filesystem remote short-circuits to `copy_lfs_object` (`engine.rs:4812-4818`), which on the download side ends by calling `materialize_pending` (`engine.rs:4966-4974`); otherwise `lfs_access` + `BatchClient` with a one-object `want` list (`engine.rs:4826-4835`). An unreachable filesystem remote is `SyncError::MediaAbsent` (`engine.rs:~4944`).

### What it would take to say 'not this one'

Three existing levers, none of them per-file-on-demand:
* **Whole-profile:** `LfsMode::PointerOnly` (`engine.rs:5025`, `5037`).
* **Per-subtree:** `profile.subpaths` via `SparseCone` (`engine.rs:5008-5011`).
* **Never-track (upload side only):** `lfs_never` in `LfsPolicy` (`stage.rs:125-146`) — governs *staging*, not fetching.

A per-path 'virtual' predicate would be inserted at `engine.rs:5019` (the `for smudge in &pending` head) and would have to answer both the materialize branch (5020) and the enqueue branch (5037). The `LfsPolicy` type is the natural home for a compiled `virtual` GlobSet, mirroring `never` (`stage.rs:116-121`). **This is my reading of the code, not a documented plan** — no `virtual`/`hydrate`/`dehydrate` symbol exists anywhere in the crate (searched; zero matches).

---

## 3. `lfs/prune.rs` + `lfs_prune_local` — the release primitive that already exists

Closest existing analogue to 'release a materialization', and **the inverse** of what virtual files need.

### The three safety conditions

Stated in the module doc (`lfs/prune.rs:28-45`), implemented in `plan` (`lfs/prune.rs:78-108`). The framing: `git lfs prune` prunes on a heuristic and its known failure is deleting objects for staged files, producing commits that can no longer be pushed (git-lfs#5636); it cannot do better because it has no record of which uploads actually landed — keeper does (`prune.rs:22-29`).

1. **Nothing in the journal references the oid.** `owed` is every oid the journal still names — outstanding uploads and queued downloads alike (`prune.rs:70-72`, checked `prune.rs:88`). An owed upload is an exact answer, not an inference from ref positions; and the engine *rebuilds* an object it still owes, so pruning under an outstanding upload is merely futile (`prune.rs:29-35`). Supplied by `db::referenced_oids` (`db.rs:966-…`), called at `engine.rs:5302`.
2. **The worktree still holds the real content.** `worktree_holds_content` (`prune.rs:113-139`): `symlink_metadata`; must be a regular file whose length equals `recorded.size` (`prune.rs:127-129`); above `MAX_POINTER_BYTES` length alone settles it with one `lstat` (`prune.rs:130-132`); below it, at most a pointer-sized read plus `!is_pointer_candidate` (`prune.rs:133-138`). A mismatch is not an error — modified, mid-write, or pointer-text all mean 'not a candidate', the safe direction (`prune.rs:119-123`). The doc is explicit: *'A path whose worktree content is pointer text is the inverse case — there the store object IS the only local copy, and it is never a candidate'* (`prune.rs:41-44`).
3. **The remote confirms it holds the object.** Deliberately left to the caller because it is the only condition needing the network, and it must be answered by the same upload-batch the upload path uses — an object with neither `actions` nor `error` means the server already has the content — not by a cheaper existence probe (`prune.rs:36-41`).

Condition 3 is **not** enforced in the code path I traced: `Engine::prune_lfs_store` (`engine.rs:5299-5320`) calls `plan` with `tracked` and `owed`, then `release` directly — I found no batch round trip between them. **UNVERIFIED** whether condition 3 is satisfied indirectly (e.g. because `referenced_oids` only stops naming an oid once its upload unit is `complete`d, `db.rs:826-830`); the module doc says 'left to the caller' and this caller does not appear to ask.

### What is deleted, what is kept

`release(store, objects)` deletes `store.object_path(&oid)` one object at a time, tolerating `NotFound`, returning bytes reclaimed (`prune.rs:143-156`). Only the **store object** is deleted; the worktree file and the index entry are untouched. Deduplicated by oid so two paths with identical content release once (`prune.rs:76`, `prune.rs:88`). Nothing in `plan` deletes; `release` does, one at a time, so a failure midway leaves a consistent store rather than a half-applied plan (`prune.rs:45-47`). Idempotence is tested (`prune.rs:180-184`).

### `git status` afterwards

Unchanged. The index entry still holds the pointer blob with the worktree's stat (`git/commit.rs:197-202`), and the worktree file is untouched, so nothing about the status comparison moved. Rebuilding the object costs one local read (`prune.rs:32-35`).

### Config plumbing

`profile.lfs_prune_local: bool`, `#[serde(default = "default_true")]` (`profile/mod.rs:811-812`), default `true` (`profile/mod.rs:938`). Invoked after a drain when true, warn-only on error (`engine.rs:3056-3062`). CLI: `--lfs-prune-local` / `--no-lfs-prune-local`, mutually exclusive (`keeper-syncd/src/commands.rs:370-378`); only the opt-out is applied on `add`, so a changed default is not quietly undone by the tool meant to honour it (`keeper-syncd/src/commands.rs:713-718`).

---

## 4. The full config surface a 'virtual' mode would join

### `LfsMode`

`#[serde(rename_all = "camelCase")] pub enum LfsMode` (`profile/mod.rs:80-95`): `Materialize` (default), `PointerOnly`, `Disabled`. `PointerOnly` is documented as the `fetchinclude`/`fetchexclude` idiom for a partial or metered profile (`profile/mod.rs:88-89`). `Disabled` is only honest for a text-only repository, and the engine warns when an oversized file appears rather than silently committing it raw (`profile/mod.rs:90-92`, warning at `engine.rs:1986-1990`).

### `MediaPolicy` — the per-subtree sibling

`profile/mod.rs:364-389`: a **distinct** enum, `Materialize` (default, `profile/mod.rs:380`) / `PointerOnly` (`profile/mod.rs:388`), scoped to one subtree because a folder can reasonably want its notes and documents on disk while its video stays a pointer (`profile/mod.rs:365-368`). Serialized as `"materialize"` / `"pointerOnly"` (`profile/mod.rs:1810-1811`). Its doc explicitly contrasts itself with `lfs_prune_local`: what prune releases is a second copy of content the worktree still holds, where this is the content itself (`profile/mod.rs:376-379`). This is the precedent that a per-path answer is a *new* enum, not a reuse.

### The profile fields

```
pub lfs_mode: LfsMode,                                    profile/mod.rs:789
#[serde(default = "default_lfs_threshold")]
pub lfs_threshold_bytes: u64,                             profile/mod.rs:790-791
#[serde(default = "default_true")]
pub lfs_prune_local: bool,                                profile/mod.rs:811-812
#[serde(default)]
pub lfs_never: Vec<String>,                               profile/mod.rs:828-829
```
Defaults in `SyncProfile::new`: `lfs_mode: Materialize`, `lfs_threshold_bytes: DEFAULT_LFS_THRESHOLD_BYTES`, `lfs_never: Vec::new()`, `lfs_prune_local: true` (`profile/mod.rs:935-938`). Serde-default coverage is tested: a minimal JSON parses to the defaults (`profile/mod.rs:1339-1343`), and a table omitting `lfsPruneLocal` still parses to the same value as a fresh profile (`profile/mod.rs:1375-1381`).

Note `lfs_mode` carries **no** `#[serde(default)]` (`profile/mod.rs:789`), unlike its neighbours — a stored row missing the key would fail to parse. **UNVERIFIED** whether any row can lack it; `upsert_profile` always writes the full serialization (`db.rs:517-521`).

### Persistence

* Table: `CREATE TABLE IF NOT EXISTS profiles (id TEXT PRIMARY KEY, json TEXT NOT NULL, state TEXT NOT NULL DEFAULT 'idle', …)` (`db.rs:60-65`). The whole profile is one JSON blob.
* `db::upsert_profile` validates, then funnels through `profile::as_stored(profile, prior)` before serializing (`db.rs:517-521`).
* **Migration idiom to copy:** `ensure_prune_default` (`db.rs:183-205`) is a one-shot keyed on a `meta` marker `"lfs_prune_local_default_on"` (`db.rs:161-162`), using `UPDATE profiles SET json = json_set(json, '$.lfsPruneLocal', json('true')) WHERE json_extract(json, '$.lfsPruneLocal') = 0` (`db.rs:197-202`). The comment states why `json_set` rather than a serde read-modify-write: it preserves every other key byte for byte, **including keys written by a newer keeper**, which a round trip through `SyncProfile` would drop (`db.rs:194-196`). Rationale for needing a migration at all: a changed serde default cannot reach an install that already exists, because a profile is stored as its serialization and serialization writes every key (`db.rs:164-170`). Tests assert byte-for-byte preservation (`db.rs:2252-2260`) and that the one-shot does not re-run (`db.rs:2281-2293`).
* Config-file layer: `[folder]` accepts both `lfs_never` and `lfsThresholdBytes`, snake and camel folded onto one key (`profile/folder.rs:825-827`, `profile/folder.rs:883-893`; `canonical_key` at `profile/mod.rs:1194-1195`). A file-supplied value is in force on read but **never** written back into the stored row (`profile/folder.rs:1054-1081`).

### CLI surface (`keeper-syncd`)

`LfsModeArg { Materialize, PointerOnly, Disabled }` (`commands.rs:413-418`) with `From<LfsModeArg> for LfsMode` (`commands.rs:439-447`); flag `--lfs-mode` default `materialize` (`commands.rs:343-345`); `--lfs-threshold-bytes` (`commands.rs:358-359`); `--lfs-never` repeatable, motivated by the per-extension `.gitattributes` rule (`commands.rs:379-387`); applied at `commands.rs:708-724`. Daemon TOML parses `lfs_mode = "materialize"` (`config.rs:537-546`) and `"pointerOnly"` (`config.rs:523-524`).

### IPC / TS surface (`keeper` shell)

`SyncProfileView { lfs_mode: String, lfs_threshold_bytes: u64 }` (`sync_ipc.rs:85-88`), projected via `lfs_str` (`sync_ipc.rs:624-630`), populated at `sync_ipc.rs:183-185`. Request side: `lfs_mode: String` + `lfs_threshold_bytes: Option<u64>` (`sync_ipc.rs:479-482`), parsed by a three-arm match that errors on an unknown mode (`sync_ipc.rs:764-773`), applied at `sync_ipc.rs:805-816`. Every `Option` on the request means 'the caller did not express this', never 'reset it' — a form that does not show a knob must not be able to move it (`sync_ipc.rs:806-812`).

**A new mode therefore touches at minimum:** `profile/mod.rs:81`, `commands.rs:414`+`439`, `sync_ipc.rs:624`+`764`, the TS type generated from `sync_ipc.rs:86`, plus every `== LfsMode::` / `!= LfsMode::Disabled` comparison (`stage.rs:91`, `stage.rs:149`, `stage.rs:1022`, `engine.rs:1986`, `4768`, `5003`, `5025`, `5037`, `5439`).

---

## 5. The journal — could a user-initiated on-demand download reuse it?

**Yes, with one gap.**

### Units and states

`WorkKind::LfsUpload { oid, size }` and `WorkKind::LfsDownload { oid, size }` (`db.rs:614-617`), discriminants `"lfsUpload"` / `"lfsDownload"` (`db.rs:659-660`). `covered_while_running()` returns true for both, because a transfer is content-addressed — `LfsDownload { oid, size }` names immutable bytes, so a second unit for an object already in flight can only fetch what the first is fetching; a push is the opposite, since it publishes whatever the worktree holds *when it runs* (`db.rs:635-651`). States include `pending` / `running` / `parked` / `deferred` (`db.rs:1005`, `db.rs:1062-1066`, `db.rs:1292-1296`).

### Enqueue → drive

* `db::enqueue_unique(conn, profile_id, kind, …)` deduplicates on the payload (`db.rs:739-744`), which is why the human-readable path is attached separately: `db::label_unit(conn, id, label)` is `UPDATE journal SET label = ?2 WHERE id = ?1 AND label IS NULL`, first-writer-wins so a last-one-wins race cannot make the line flicker (`db.rs:280-286`). The engine does exactly this pair at `engine.rs:5046-5052`.
* `db::claim_ready(conn, profile_id, now_ms, limit)` claims in one statement so two supervisors can never take the same row (`db.rs:772-776`). `CLAIM_LIMIT = 16` units per profile per tick, small on purpose so a tick draining a thousand units cannot hold the reservation for minutes and starve the watcher (`engine.rs:331-336`). Supervisor tick is `TICK_MS = 1_000` (`engine.rs:338-339`).
* `ClaimedUnit.attempts` is the pre-increment value **plus one** — the attempt the caller is about to perform — because the backoff schedule treats attempt 0 as immediate, so reporting the pre-increment value would give the first retry no delay at all (`db.rs:695-702`; tests `db.rs:1606-1608`, `db.rs:1629-1633`, `db.rs:2034-2038`).
* `db::complete(conn, id)` is a `DELETE` and is the **only** place work leaves the journal (`db.rs:826-830`).
* `db::unpark(conn, profile_id, unit_id)` only moves a row out of `parked`, never a pending (which would defeat its backoff) or running one (which would pull it from under the supervisor) (`db.rs:1062-1068`, test `db.rs:2144-2148`).
* `Engine::drain_journal(profile, scan_when_idle, source)` (`engine.rs:2563-2567`) is the driver; dispatch at `engine.rs:3010-3015`. `sync_once` loops the drain to quiescence with a **strictly decreasing** outstanding count, so the loop is bounded by the queue rather than by the queue's health (`engine.rs:5255-5276`).

### Visibility

`db::queued_downloads(conn, profile_id) -> Vec<(Option<String> label, String oid, u64 size)>` (`db.rs:294-297`) — exists purely to name inbound objects for a human. `db::pending_count` deliberately excludes parked rows (`db.rs:1003-1006`), which is why `ProblemReport.parked` is their only surface (`engine.rs:~296-300`, `ParkedUnit` at `engine.rs:280-291`).

### The gap for an on-demand fetch

Everything a user-initiated download needs is present — `enqueue_unique` + `label_unit` + the supervisor — and `covered_while_running` already makes a repeat click idempotent (`db.rs:649-651`). What is **missing**: the journal exposes no priority/urgency dimension in `claim_ready` (`db.rs:772-776`), so a user's click queues behind up to `CLAIM_LIMIT` background units per tick; and `do_lfs` early-returns unconditionally under `LfsMode::Disabled` (`engine.rs:4771-4773`) while `materialize_pending` skips under `PointerOnly` (`engine.rs:5025`, `5037`) — a virtual mode wanting on-demand materialization must bypass those guards on the user-initiated path rather than route through them.

---

## 6. `SyncBlocker` and the pending-visibility types

**There is no type named `SyncBlocker` anywhere in this repository.** I searched `src-tauri/crates` and `src` for `SyncBlocker` and for `blocker` generally; the only matches are recording-permission prose in `keeper-core/src/recording.rs:4009-4019` and `src/components/layout/recording-pane.tsx:162`, plus an FTS comment at `keeper-core/src/archive/fts.rs:95-96`. None are sync types.

The types that actually express 'an LFS object this clone does not hold yet':

### `PendingReason::Incoming`

`pub enum PendingReason` (`engine.rs:225-257`). Variants: `Settling { since_ms }` (229), `Untracked` (231), `Modified` (233), staged-as-new (234-235), `Deleted` (237), and:

> `/// Queued to come IN: an LFS object this clone does not hold yet.` (`engine.rs:238`)
> `Incoming { size_bytes: u64, replacing: bool }` (`engine.rs:256`)

`replacing` distinguishes a second version arriving from a first arrival, and it is answerable **only** from keeper's own materialization record (`engine.rs:240-244`; the shell renders it as `"incomingUpdate"` at `sync_ipc.rs:1198-1202`). `size_bytes` is carried because *'a queue of 106 is two minutes or four days depending on it'* (`engine.rs:254-256`).

### `PendingFile`

`{ path: String, reason: PendingReason, size_bytes: Option<u64> }` (`engine.rs:259-278`). Inbound rows have always carried the announced size; outbound rows are measured off the worktree, so a list holding both reads as one list rather than two half-populated columns. `None` where there is nothing to measure (`engine.rs:266-276`).

### `Engine::pending`

`engine.rs:5697-5866`. Outbound rows come from status buckets, sized by `fs::metadata` — or, for a deletion, off the index via `deleted_sizes`, because the file is gone and what was there is the whole question (`engine.rs:5817-5830`). The inbound half is added **after**, so a local change takes precedence on a name collision: what the operator did is the more actionable fact (`engine.rs:5832-5835`). It reads `db::materialized_paths` into `held` (`engine.rs:5835-5837`) and joins against `db::queued_downloads` (`engine.rs:5838-5840`); an object whose label is absent is named `"LFS object <oid[..12]>…"` rather than dropped, so this list cannot disagree with the status-line count (`engine.rs:5841-5847`); `replacing = held.contains(&path)` (`engine.rs:5849`).

### The `materialized` table

`CREATE TABLE IF NOT EXISTS materialized (profile_id TEXT NOT NULL, path TEXT NOT NULL, at_ms INTEGER NOT NULL, …)` (`db.rs:142-146`), written by `db::remember_materialized` with `INSERT OR REPLACE` because re-materializing is the ordinary case and the newest timestamp is the useful one (`db.rs:324-338`), read in bulk by `db::materialized_paths` because one statement beats a query per line (`db.rs:341-352`). **This table is already, precisely, a per-path materialization ledger** — it records *that* a path was materialized and *when*. It is the obvious carrier for a last-used timestamp driving lazy release.

### `browse::EntrySyncStatus`

`BrowseEntry { name, relative_path, absolute_path, is_dir, size_bytes: Option<u64>, sync: EntrySyncStatus, unspellable }` (`browse.rs:88-134`). `size_bytes` comes off the **same `stat`** `is_dir` already paid for — free — and is `None` for directories on purpose, because `metadata().len()` answers for a directory and the number means nothing (`browse.rs:107-120`, `browse.rs:613-626`; test `browse.rs:1157-1200` pins `Some(0)` vs `None` as different facts). `EntrySyncStatus` is `Synced | Excluded | Waiting { reason: Option<PendingReason> } | NotInRepository | Unknown` (`browse.rs:157-198`, classification at `browse.rs:727-743`). `PendingView::Unavailable` maps every row to `Unknown` rather than lying with an empty known list, which would mark everything `Synced` (`browse.rs:536-538`, `browse.rs:732-734`). Listing is capped at `LISTING_CAP = 1000` with an explicit `truncated` flag (`browse.rs:76-77`, `browse.rs:634-637`, `browse.rs:248-250`).

### `lfs/audit.rs`

The remote half of verification — the check that runs after the push gate, because the gate can be wrong. `Engine::verify` checks the local half: every pointer names an object this machine still has, at the right length (`audit.rs:6-10`). The other half is the one that loses data: if the object never reached the server, every peer that clones gets ~130 bytes of text, and **nothing anywhere says so** — git is satisfied, the remote is up to date, the UI is green (`audit.rs:12-19`). Module doc records the incident: on 2026-08-12 a commit of 127 objects published with four never uploaded; an audit found **16 objects, 8.0 GB missing on the server** while both folders reported a clean sync; four recordings unrecoverable because the only copies were on a machine that had since replaced them with pointer text (`audit.rs:19-27`).

Types: `MissingObject { path, oid, size, code, reason }` (`audit.rs:49-66`) — `size` is "the size of the hole" (`audit.rs:57-58`); the server's own message is never carried, only a code and a locally-written sentence, because `error.rs` requires messages to carry nothing but ids, hosts, paths, counts and status codes and an LFS error message is attacker-influenced text (`audit.rs:60-64`, `explain` at `audit.rs:68-75`). `RemoteAudit { checked, objects, bytes, missing }` with `missing_bytes()` and `is_intact()` (`audit.rs:77-100`). `tracked_objects` dedupes by oid but keeps every path, sorted, because a missing object must be reported as the file a human recognises rather than as a digest (`audit.rs:102-136`); the empty object is skipped since a server is not obliged to store it and asking would report a hole that is not one (`audit.rs:117-121`). `report` sorts largest hole first and treats a bare object with neither actions nor error as **present**, because a false positive sends someone hunting for bytes that are fine — the expensive kind (`audit.rs:138-172`, test `audit.rs:~230-241`). Deliberately does not re-upload: detection and repair are different operations with different risks, and the auditing machine usually does not have the bytes (`audit.rs:34-40`). Gated off under `LfsMode::Disabled` (`engine.rs:5439-5441`).

### `SyncError::LfsUploadPending`

`SyncError::LfsUploadPending { objects: u32 }` (`error.rs:121-122`), classified `Retriability::Deferred` (`error.rs:228-229`), code `"lfsUploadPending"` (`error.rs:280`). Raised at `engine.rs:3938-3941` to hold a push whose objects have not landed. A `Deferred` sitting under `InProgress` is defended explicitly: it has a `last_error` spelling out what it waits for, and rendering that as a failure would accuse keeper of breaking while it is doing the careful thing (`db.rs:1292-1296`). Released by a narrow undefer sibling when the last upload clears (`db.rs:896-902`). State projection at `engine.rs:2736-2740`. The desktop maps it to `IpcErrorCode::SyncUnavailable` — a wait, not a bug (`sync_ipc.rs:686-687`, test `sync_ipc.rs:3925-3929`); the daemon maps it to `EXIT_FAILURE` on purpose, since a run that published nothing is not a success (`keeper-syncd/src/commands.rs:1997-2007`).

Relevant hazard for any new permanent-ish LFS error: an ssh failure misclassified `Permanent` parks the `LfsUpload` unit on its FIRST failure, a parked unit still counts toward `outstanding_count`, and the push then stays held with no auto-recovery until someone runs `db::unpark` — "that is publishing stopped by a lid" (`lfs/ssh.rs:479-484`, `lfs/ssh.rs:1025-1031`).

---

## 7. What breaks when a worktree file is pointer text instead of content

The risk register for a virtual-files design — pointer text in the worktree is *already* a supported state, and the tree carries scar tissue from getting it wrong.

### 7.1 `gix::status` and the racily-clean rule

The design rests on: **pointer blob + worktree stat = clean status** (`git/repo.rs:1946-1947` names it exactly that; established empirically per `stage.rs:26-32`). Two failure modes:

* **Racily clean.** An entry whose mtime is not older than the index is re-read regardless of stat, and re-reading an LFS entry finds the worktree's gigabytes where the blob holds a pointer (`stage.rs:34-40`, `stage.rs:855-859`). `stage::is_false_modification(repo, rela, absolute)` (`stage.rs:924-975`) dismisses that without reading content, requiring **four** conditions: (a) the worktree stat still matches the entry's, using the repo's own `core.trustCtime`/`core.checkStat` rather than assumed settings, so the comparison is the same one status made — and this rejects an in-place edit that preserved the byte count, which a size-only comparison would have dropped for good (`stage.rs:862-873`, `stage.rs:962-968`); (b) `.gitattributes` routes the path through the `lfs` filter — `routed_through_lfs`, `stage.rs:990-1010`, `Source::WorktreeThenIdMapping` because this is the check-in direction (`stage.rs:985-988`); (c) the staged blob really is a pointer (`stage.rs:970-972`); (d) `HEAD` already records that same blob, so a pointer staged but not yet committed is not stranded until the file changes again (`stage.rs:882-886`, `head_records` at `stage.rs:1013-…`). The stat check leads because it costs one `lstat` and keeps attribute/ODB reads off the path every non-LFS modification takes (`stage.rs:956-961`). Every failure to read answers 'this is a real modification' (`stage.rs:918-922`). The design note at `stage.rs:876-905` records that an earlier version asked about the blob's *shape* instead of the path's *routing*, so a real edit to a text file whose content is literally a pointer (documentation, a fixture, a peer's un-smudged pointer) landing in the race window at the same length was dismissed — and dismissed again on every later scan: 'a data-loss-class outcome'. Called from the scan at `engine.rs:4185`.
* **Stale index stat after materialization.** `refresh_index_stat` is called in exactly one place, right after materializing (`engine.rs:5061-5067`), and `git/repo.rs:1879-1885` says that is correct and **not enough**: a run that dies between writing the real files and refreshing their stat (e.g. `Too many open files`) leaves the invariant broken, hence a repair pass (`git/repo.rs:1930-1938`, driven from `engine.rs:5367-5369`, `engine.rs:5382`).
* An empty-blob carve-out matters here: `pointer_blob` answers `None` for a zero-length blob, because every empty tracked file shares the one empty blob and answering `Some` made `indexed_pointer` call all of them LFS entries and hand `is_false_modification` a dismissible entry for each (`stage.rs:764-789`). Note the asymmetry: `Pointer::parse` *does* read zero bytes as the empty pointer, and `lfs::filter` depends on that (`stage.rs:770-778`, `lfs/filter.rs:258-262`).

### 7.2 The clean filter re-emitting a pointer

`lfs::filter::clean` explicitly re-emits pointer text rather than hashing it (`lfs/filter.rs:232-254`). The doc spells out the failure: hashing pointer text emits `Pointer::new(hash(P), len(P))` — a pointer naming a pointer. First the path reads MODIFIED, enough for `git merge -X theirs` or `--ff-only` to refuse with "local changes would be overwritten"; then, if the index takes the clean, the commit replaces the only reference every peer has to the real object with a reference to 130 bytes of text, and the object's oid is no longer named anywhere in the tree (`lfs/filter.rs:206-224`). Pointer text in the worktree is not a corner case — it is a state this very module produces, and `LfsMode::PointerOnly` makes it permanent (`lfs/filter.rs:206-212`). Re-emitting rather than re-encoding is deliberate: `Pointer::parse` accepts non-canonical spellings and re-rendering would change the blob hash, making the path read modified for a second, subtler reason (`lfs/filter.rs:230-233`). Canonicality is a listed quirk of the format (`lfs/mod.rs:69-72`); an empty file is its own pointer and passes through unchanged (`lfs/mod.rs:73`).

### 7.3 The smudge cascade / empty-file catastrophe

`lfs::filter::smudge` passes the bytes through unchanged when the input is not a pointer or the object is not in the store — what git-lfs does, and it keeps a partial fetch usable instead of failing the checkout (`lfs/filter.rs:186-199`). The reason keeper owns `filter.lfs.process` at all: when `filter.lfs.process` is defined git uses it and ignores `clean`/`smudge` entirely, and `git lfs install` writes a **global** process driver that outranks a repository-local clean/smudge pair — so on any machine where real git-lfs was ever installed, keeper's filter was never once invoked (`lfs/filter.rs:42-50`). Under `filter.lfs.required=false` git swallows a filter failure and writes **zero bytes** for that path *and every remaining path in the same checkout*: on 2026-08-16 one object missing from the server turned 122 recordings, 74 GB of pointers, into 122 empty files in a single fast-forward (`lfs/filter.rs:56-66`). The guard against *committing* that damage is `stage::truncated_media` (`stage.rs:843-848`), deliberately narrow — it asks only about **emptiness**, because a re-encoded video legitimately has a different length and zero is the one length that is never an edit (`stage.rs:836-841`); a missing file is a deletion and not its business (`stage.rs:840-842`). Called at `engine.rs:4168-4170`.

A related liveness hazard: registering `filter.lfs.process` is a promise gitoxide collects on hard — `maybe_launch_process` fails *before* driver leniency is consulted, so a driver that cannot be launched fails `status` outright whatever `filter.lfs.required` says; nothing commits, the index is never rewritten, and the same entry is re-read forever. DW-206 measured that in the field: 90 430 identical log lines, unclearable by restart or reinstall (`lfs/filter.rs:729-736`). Mitigations: a 900-second per-request watchdog that exits so the parent sees a closed pipe rather than waiting forever — after a stalled filter froze a vault for four days (`lfs/filter.rs:397-424`, constant `lfs/filter.rs:397`, per-minute progress line `lfs/filter.rs:410-418`); and the request is always drained to the flush packet before the response starts, or both pipes fill at 64 KiB and deadlock (`lfs/filter.rs:298-310`). `delay` is deliberately **not** advertised, because it only buys concurrency for a filter that fetches over the network and this one never does (`lfs/filter.rs:294-298`) — directly relevant to any design wanting checkout-triggered hydration.

### 7.4 `stability.rs` / `is_dataless`

Orthogonal to LFS but the closest existing precedent for placeholder semantics, and mandatory rather than optional (`stability.rs:32-38`). `is_dataless(path)` reads macOS `SF_DATALESS` via `lstat`, which does **not** materialize the file whereas `open` does (`stability.rs:164-181`); always `Ok(false)` off macOS, because on Linux/Windows a FUSE-backed cloud mount either has the bytes or fails the read, and neither is triggered by opening (`stability.rs:183-189`). `StabilityVerdict::Dataless` short-circuits before anything can open the file, and a failed probe is treated as 'settling' rather than as permission to proceed (`stability.rs:573-584`), with a second defence-in-depth guard inside the function that actually opens the file (`stability.rs:820-829`). Module doc: hashing a Documents tree under iCloud Drive without this check drags the user's entire cloud library down over their network (`stability.rs:32-38`).

**Relevance:** on macOS this is the OS-level virtual-file signal keeper already respects — but only as *refusal*, never as something keeper produces. **UNVERIFIED** whether `SF_DATALESS` can be set by a non-Apple provider; nothing in this repository attempts it.

### 7.5 `copy.rs`

`copy.rs` refuses to copy a dataless placeholder, returning `PlanItem::Refused` with the reason *'a dataless iCloud placeholder; copying it would materialize the file from the network'* (`copy.rs:823-830`), and refuses FIFOs, sockets and device nodes for the same class of reason — opening a FIFO would block forever (`copy.rs:47-49`). It imports `is_dataless` from `stability` (`copy.rs:67-68`). It has **no** LFS-pointer awareness: copying a pointer-text file today copies 130 bytes of text with no warning. A concrete gap a virtual-files feature must close.

### 7.6 Size-reporting surfaces

* `browse.rs` reports `size_bytes` straight off `fs::metadata().len()` (`browse.rs:613-626`) — a virtual file renders as ~130 bytes. The honest number already exists: `stage::indexed_size(repo, rela)` answers from the pointer for an LFS entry and from the object header otherwise, never loading content, and is the only place left to ask for a path gone from the worktree (`stage.rs:800-820`). Used for the tier-0 expansion at `engine.rs:5768-5770`; **not** wired into `browse`.
* `engine.rs:4498-4510` is a second, inlined copy of the same idea (index key → header size → `Pointer::parse` → pointer size), with the same forward-slash key conversion `indexed_pointer` makes (`engine.rs:4498-4500`).
* `Engine::pending` sizes outbound rows off `fs::metadata` (`engine.rs:5824-5829`), under-reporting a virtual file.

### 7.7 Verification

`Engine::verify` reads at most `MAX_POINTER_BYTES` of each file, probes `is_pointer_candidate`, parses, and reports the path as bad when the store lacks the object (`engine.rs:5637-5645`). Under a virtual-files mode that is the *normal* state, so this would produce mass false positives unless taught which paths are intentionally virtual. The upload-side re-clean check at `engine.rs:5553-5560` (hash the worktree file back in and compare) has the same character: it silently fails for a path holding pointer text.

### 7.8 Watcher wake and the media/file URI handlers

* **Watcher:** `materialize` writes to a sibling `.keeper.<name>.tmp` and renames, so an interrupted materialization leaves the pointer intact rather than a truncated video; the staging name carries keeper's own prefix, which tier 0 already excludes, so the watcher cannot mistake it for user content (`stage.rs:1113-1116`, implementation `stage.rs:1128-1141`). Any new materialize/dehydrate path **must** reuse this convention or it will wake the watcher and generate spurious commits.
* **File/media protocol handlers:** `keeper/src/file_protocol.rs:8-33` and `keeper-sync/src/file_serve.rs:13-20` both delegate all path resolution to `browse::resolve` — the same function `sync_browse`, `sync_open_entry`, `sync_read_text` and `files_write::resolve_existing` call, "the second caller of a rule, not a second copy of one" (`file_serve.rs:16-19`) — and collapse every refusal to a single 404 so a probe cannot tell 'outside the folder' from 'not there' from 'no such profile' (`file_protocol.rs:29-32`). Range, slice-cap and 200/206/416/404 shapes are `note_protocol`'s and are called rather than copied (`file_serve.rs:43-44`). Neither has pointer awareness: serving a virtual file today streams ~130 bytes of pointer text to the webview as if it were the media, and a `Range` request is satisfied against the pointer's length. Both hop to the blocking pool before resolving, so an on-demand hydrate on this path would not stall the webview thread (`file_protocol.rs:54-60`).
* **Notes/CSV readers:** I found no `std::fs::read` / `File::open` / `metadata(` in `keeper-core/src/notes/csv.rs`, `keeper-core/src/file_asset.rs`, or `keeper/src/file_protocol.rs` (searched; zero matches), so I cannot substantiate a pointer-text hazard in those specific readers. **UNVERIFIED.**

---

## 8. Extension points — what a virtual-files feature must touch

Ranked by how load-bearing each is.

1. **`LfsMode`** — `profile/mod.rs:80-95`. A fourth variant (or a separate per-path selector) plus its 9 comparison sites (`stage.rs:91`, `149`, `1022`; `engine.rs:1986`, `4768`, `5003`, `5025`, `5037`, `5439`) and its 4 projections (`commands.rs:414`+`439`, `sync_ipc.rs:624`+`764`). `MediaPolicy` (`profile/mod.rs:364-389`) is the precedent that a per-subtree answer is a *different* enum, not a reuse.
2. **`LfsPolicy`** — `stage.rs:116-166`. Already compiles one gitignore-dialect GlobSet (`never`) from the profile; a `virtual` GlobSet is the same shape, the same refuse-on-typo discipline (`stage.rs:138-145`), and the same repository-relative matching contract (`stage.rs:157-160`). This is where the user's 'gitignore-like pattern file' lands.
3. **`Engine::materialize_pending`** — `engine.rs:4998-5069`. The single decision point for 'download this object now'; both the materialize branch (5020-5036) and the enqueue branch (5037-5053) need the new predicate. Also the site of `remember_materialized` (5033) and `refresh_index_stat` (5064).
4. **`lfs::prune::worktree_holds_content`** — `prune.rs:113-139`. Its condition-2 contract (*'A path whose worktree content is pointer text … is never a candidate'*, `prune.rs:41-44`) is exactly inverted by a release-a-materialization feature. Dehydration is the **opposite** operation to `prune::release` (`prune.rs:143-156`) and must not be built by relaxing this predicate: after dehydration the store object *is* the only local copy, so it needs its own condition set — at minimum the remote-confirms-it-holds-it condition (`prune.rs:36-41`) that `prune_lfs_store` currently appears not to ask (see §3).
5. **`stage::is_false_modification`** — `stage.rs:924-975`. The dismissal gate every LFS path passes on every scan. Dehydration changes the worktree stat, so condition (a) (`stage.rs:962-968`) fails and the path reads MODIFIED; something must re-stat after a release, mirroring `refresh_index_stat` after a materialize (`git/repo.rs:1947`). The crash-recovery repair at `git/repo.rs:1930-1938` is the model for making that durable.
6. **`stage::materialize`** — `stage.rs:1118-1143`. The hydrate primitive: store-presence precondition returning `SyncError::Integrity` (`stage.rs:1120-1125`), temp-file + rename (`stage.rs:1113-1116`), `.keeper.*.tmp` staging name (`stage.rs:1128`). A `dehydrate` sibling belongs beside it, must use the same rename discipline, and must refuse when the store does not hold the object — otherwise dehydration is deletion.
7. **The `materialized` table** — `db.rs:142-146`, `db.rs:324-352`. Already a per-path, per-profile, timestamped materialization ledger. A `last_used_ms` column plus a nightly sweep is the smallest change that implements the requested 24h-lazy release; `ensure_prune_default`'s `json_set` one-shot (`db.rs:183-205`) is the migration idiom for the profile-JSON half and the `meta`-marker pattern (`db.rs:161-162`, `db.rs:183-193`) for the run-once half.
8. **`WorkKind` + `enqueue_unique` / `label_unit`** — `db.rs:614-617`, `db.rs:739-744`, `db.rs:280-286`. A user-initiated hydrate reuses `LfsDownload` as-is; `covered_while_running` (`db.rs:649-651`) already makes a repeat click idempotent. Needs a priority story against `CLAIM_LIMIT = 16` (`engine.rs:331-336`).
9. **`PendingReason::Incoming` / `PendingFile` / `browse::EntrySyncStatus`** — `engine.rs:238-256`, `engine.rs:259-278`, `browse.rs:157-198`. The existing vocabulary for 'not here yet'. A `Virtual`/`Dehydrated` status is a sibling variant; `stage::indexed_size` (`stage.rs:800-820`) is the already-written function supplying the honest size, `indexed_pointer` (`stage.rs:792-798`) the oid, and `lfs::audit::MissingObject` (`audit.rs:49-66`) the precedent for how much to say about an object this clone cannot redeem.
10. **`lfs::filter::smudge`** — `lfs/filter.rs:160-201`. Serves only what the store already holds and never fetches; `delay` is deliberately unadvertised for exactly that reason (`lfs/filter.rs:294-298`). Any design where a plain `git checkout` (or an OS-level placeholder open) should trigger an on-demand fetch would have to advertise `delay` and add a second state machine — a large, explicitly-declined change with the DW-206 launch-failure hazard attached (`lfs/filter.rs:729-736`).

### Not present (searched, zero matches)

* No `SyncBlocker` type. No `virtual`, `hydrate`, `dehydrate` or `placeholder` concept anywhere in `keeper-sync`.
* No OS-level placeholder *producer*. `stability.rs` only *detects* `SF_DATALESS` (`stability.rs:164-189`) and refuses to touch such files. Windows Cloud Files API, FUSE and macOS File Provider are **absent from this repository entirely**.
