# Lane: LFS — pointer format, staging/routing, transfer, filter process, store & prune

## Scope read

| file | ranges |
|---|---|
| `src-tauri/crates/keeper-sync/src/lfs/pointer.rs` | 1–510 (whole) |
| `src-tauri/crates/keeper-sync/src/lfs/stage.rs` | 1–703, 704–1003, 1299–1713, 1754–1833, 1959–2013, tests 2400–4061 (grep) |
| `src-tauri/crates/keeper-sync/src/lfs/store.rs` | 1–563 (+ grep to 767) |
| `src-tauri/crates/keeper-sync/src/lfs/filter.rs` | 1–783 (+ grep 560–1186) |
| `src-tauri/crates/keeper-sync/src/lfs/pktline.rs` | 1–235 (whole) |
| `src-tauri/crates/keeper-sync/src/lfs/prune.rs` | 1–188 (whole) |
| `src-tauri/crates/keeper-sync/src/lfs/basic.rs` | 227–403, 397–563, 629–843, 1029–1138 (+ item grep) |
| `src-tauri/crates/keeper-sync/src/lfs/batch.rs` | full item/const/test grep |
| `src-tauri/crates/keeper-sync/src/lfs/audit.rs` | 1–123 |
| `src-tauri/crates/keeper-sync/src/lfs/virtual_policy.rs` | 599–663 |
| `src-tauri/crates/keeper-sync/src/lfs/listing.rs` | item grep |
| `src-tauri/crates/keeper-sync/src/http.rs` | 1–300 |
| `src-tauri/crates/keeper-sync/src/engine.rs` | 4439–4563, 5689–5743, 6379–6663, 6659–6803, 6859–6938, 7853–8283, 9099–9203 (+ greps) |
| `src-tauri/crates/keeper-sync/src/db.rs` | 154–203, index/query greps |
| `src-tauri/Cargo.toml` | 189–323; `src-tauri/Cargo.lock` gix-filter entry |
| `docs/sync.md` | 271–651, 3000–3030 (§20) |
| `docs/upstream/gitoxide-filter-process-leak.md` | 1–300 |
| `docs/constraints-and-limitations.md` | grep (no LFS entries) |
| `../evidence-hesperia.md` | whole, incl. the object-store section |
| tests | `tests/lfs_filter_process.rs` (item grep); in-crate tests in `stage.rs`, `store.rs`, `batch.rs`, `basic.rs`, `engine.rs` 18196–18492 |

Not read (other lanes): `lfs/ssh.rs`, `lfs/endpoint.rs`, `lfs/hydrate.rs`, `lfs/local.rs`, `credential.rs` beyond call sites, virtual-file policy.

## How it works

**Pointer** (`pointer.rs`). `Pointer::render` emits `version <SPEC_V1>\n` then every other key — `oid`, `size`, extensions — in one ASCII-ascending sort (`pointer.rs:206-232`). Canonicality is *defined* by re-rendering the input and comparing bytes (`:171-174`), so a non-canonical spelling is never silently re-encoded. The ceiling is exclusive at 1024 (`:126`, `:117`).

**Routing** (`stage.rs`). Two independent gates decide LFS-vs-blob: the size rule (`LfsPolicy::applies`, `:193-201` — threshold, `lfsNever`, git-control-file refusal) and the attribute rule (`already_routed`, `:1443-1459` → `LfsRouting::routes`, `:1385-1396`). `prepare` routes a path if **either** answers yes (`:1665`), so an existing `filter=lfs` attribute overrides the threshold — which is git's contract and the right answer to the owner's question. `clean` (`:785-830`) streams the file into the store and returns `Pointer::new`; the commit substitutes the pointer blob while keeping the worktree file's stat.

**Repair** (`stage.rs:1493-1580`, `engine.rs:6576-6640`). Entries whose blob is not a pointer but whose attributes say `filter=lfs` are re-staged in batches of 500 over a 5 000-entry window, with `repairable_entry` (`:1412-1418`) skipping empty blobs and non-files, and `unconverted_after_repair` (`:1582-1608`) quarantining paths a commit did not move.

**Transfer** (`basic.rs`, `batch.rs`, `http.rs`). One journal unit = one object (`engine.rs:5576-5583`) = one `objects/batch` POST (`engine.rs:7960`) = one PUT/GET. Uploads use a `SizedFileBody` with an exact `size_hint` (`basic.rs:1129-1132`) so reqwest sets a real `Content-Length`; a 90 s **progress** watchdog (`basic.rs:707-720`) guards it, backed by a 30-minute read ceiling (`http.rs:83`). Downloads resume via `.part` plus a cached hasher keyed on exact length (`store.rs:76-99`, `:420-450`), verify digest and size, then rename.

**Filter** (`filter.rs`, `pktline.rs`). `run_process` serves git's v2 long-running protocol; per-path failures answer `status=error` and the process stays up (`:585-592`).

## Findings

### F-LFS-1 `already_routed` bypasses the git-control-file guard, so the `.gitattributes`-as-pointer defect is still reachable — P1, effort M

**Evidence.** The guard exists only on the size path:
```rust
// stage.rs:193-196
pub fn applies(&self, path: &Path, size: u64) -> bool {
    if !self.enabled || size < self.threshold || is_git_control_file(path) {
```
The attribute path has no equivalent — `already_routed` (`stage.rs:1443-1459`) ends at `routed_through_lfs(repo, &index, &key, entry.mode)` with no control-file, no `lfsNever` and no mode check, and `prepare` accepts either answer:
```rust
// stage.rs:1665
if !policy.applies(rela, metadata.len()) && !already_routed(repo, rela) {
```
`mismatched_filtered_paths` (`stage.rs:1493-1573`) is likewise unfiltered, so the repair sweep will *enqueue* such a path for conversion. The tests cover only the size path — `stage.rs:3032-3047` asserts `policy.applies` refuses the three names; nothing asserts anything about `already_routed` or `prepare` for a control file.

**Why.** This is the exact defect the module's own doc calls "not a hypothetical" (`stage.rs:100-109`) and the one hesperia paid 1 314 669 `gix_attributes … non-ascii` WARNs for on 2026-08-27/28. `GIT_CONTROL_FILES` closed the *size* door; the *attribute* door is still open, and once a stale anchored rule (`/eclipse/.gitattributes filter=lfs …`) is in the file nothing ever removes it — `ensure_attributes` only appends and repairs quoting (`stage.rs:692-745`), and `repair_managed_block` explicitly refuses to touch anything it did not write (`:529-546`). hesperia's `.gitattributes` is 180 lines / 179 `filter=lfs` rules, none of which any code path can retire. A hand-run `git lfs track ".gitattributes"` reaches it too. Consequence when it fires: every attribute lookup in that subtree resolves against pointer text, so `filter=lfs` silently stops applying below it — files that should be LFS become plain blobs, which is the class the anomaly line `files git carries as plain blobs … files=600 bytes=1958664573` reports.

**Fix.** Make `is_git_control_file` a precondition of routing, not of sizing: return `false` from `already_routed` and skip in `mismatched_filtered_paths`/`unconverted_after_repair` when it matches; and have `ensure_attributes` drop a *keeper-written* managed-block rule whose pattern resolves to a control file (it safely can — `keeper_rule_pattern` already identifies keeper's own lines).

### F-LFS-2 `.lfsconfig`, `.keepervirtual` and `.keeper/*.toml` are not protected from LFS routing at all — P1, effort S

**Evidence.** The routing guard names three files:
```rust
// stage.rs:110
const GIT_CONTROL_FILES: [&str; 3] = [".gitattributes", ".gitignore", ".gitmodules"];
```
The virtualization guard, for the same hazard, names five classes:
```rust
// virtual_policy.rs:623-635
fn is_control_file(rela: &Path) -> bool {
    if crate::lfs::stage::is_git_control_file(rela) { return true; }
    … name == VIRTUAL_PATTERN_FILE || name == ".lfsconfig" …
    rela.components().any(|part| … part == ".git" || part == FOLDER_CONFIG_DIR)
```
with the doc "A virtualized `.keepervirtual` is a policy that erases itself, and `compile` would then parse `version https://git-lfs.github.com/spec/v1` as a pattern" (`virtual_policy.rs:617-625`).

**Why.** The same sentence is true of *converting* those files, and conversion is the step that produces the pointer the doc fears. `.keeper/keeper.toml` and any `.keeper/*.toml` carry the `toml` **extension**, so they are reachable without ever crossing the threshold themselves: one oversized `.toml` anywhere in the repository writes `*.toml filter=lfs` (`stage.rs:253-255`) and every folder-config file in the tree is then `already_routed`. A `.lfsconfig` that becomes a pointer breaks endpoint resolution (`engine.rs:7918-7930` reads it as text); a `.keepervirtual` that becomes a pointer erases the release policy.

**Fix.** Make `stage::is_git_control_file` delegate to (or be replaced by) the broader `virtual_policy::is_control_file` set — one predicate, both questions — and apply it at both routing gates per F-LFS-1.

### F-LFS-3 `lfsPruneLocal` deletes the second copy without the remote confirmation three documents promise — P1, effort M

**Evidence.** `prune.rs`'s contract names three conditions and delegates the third:
```
//! 3. **The remote confirms it holds the object.** Left to the caller, because it
//!    is the only condition that needs the network.          (prune.rs:38-42)
```
The one caller supplies conditions 1 and 2 and stops:
```rust
// engine.rs:9112-9115
let owed = self.with_db(|conn| db::referenced_oids(conn, &profile.id))?;
let releasable = lfs::prune::plan(&repo, &profile.local_path, &store, &tracked, &owed)?;
```
`remote_serves` exists and is used — but only by the *release/dehydrate* path (`engine.rs:11526-11530`), never by prune. `docs/sync.md` §20 item 6 states prune "releases *local object copies* the remote already holds", and §8's three conditions substitute "Nothing else is running" for the remote check — so the code, the module doc and the user doc give three different answers.

**Why.** The engine's justification is that `do_push` holds the push while an upload is outstanding, so a returned push has landed its objects (`engine.rs:5698-5702`). `audit.rs`'s own module doc records that this gate demonstrably did not hold: "on 2026-08-12 a commit of 127 objects published with four of them never uploaded, and on two real repositories an audit found **16 objects, 8.0 GB, missing on the server** while both folders reported a clean sync" (`audit.rs:20-26`). §8 documents a second escape — a hand-made `git` commit files the object but queues no upload. In both cases prune deletes the store copy on a false premise, and the worktree file becomes the only copy in the world. The doc's stated trade ("restoring one it later loses now needs the network") is false for exactly those objects. This flipped from `false` to `true` by default in 0.8.12, so it is live on all three hesperia profiles.

**Fix.** Either gate `release` on a batched `download`-operation existence probe (one round trip per ~100 objects, the shape `audit::tracked_objects` already builds) before deleting, or — cheaper — restrict prune to oids carrying a `synced_at_ms` memo (`db::note_synced`, already written by `note_unit_synced`, `engine.rs:8098-8130`) and correct §8/§20 to say what is actually enforced.

### F-LFS-4 One batch round trip and one connection per object; the 8-way transfer window is unreachable — P2, effort M

**Evidence.**
```rust
// engine.rs:7960-7963
let want = vec![lfs::batch::ObjectId::new(oid, size)];
let batched = if upload { client.upload(&want).await } else { client.download(&want).await };
```
`do_lfs` is called once per journal unit (`engine.rs:5576-5583`) and is the only caller of `download_all`/`upload_all` (grep), so the `JoinSet` window `DEFAULT_CONCURRENT_TRANSFERS = 8` (`basic.rs:55`) never holds more than one task and `DEFAULT_BATCH_SIZE = 100` (`batch.rs:45`) is never used by the engine. `http.rs:12-21` confirms one operation per profile at a time.

**Why.** This is the answer to "will it choke at 100k LFS objects". A first materialize of a 100 000-object clone costs 100 000 `objects/batch` POSTs plus 100 000 GETs, strictly serialised — on the tailnet's RTT that is the dominant term, not the bytes. The evidence's "44 GB pulled through a 240 kB/s tunnel" was bandwidth-bound so the shape was hidden; a LAN, or a profile of many small objects, would not be.

**Fix.** Claim LFS units in groups: drain up to `DEFAULT_BATCH_SIZE` `LfsDownload`/`LfsUpload` rows for one profile into a single `do_lfs` call, so one batch response feeds the existing 8-way `run_all` window. The transfer layer already supports it unchanged; only the unit-claim loop moves.

### F-LFS-5 Nothing fsyncs an LFS object before it is published, and `contains` then trusts name+size — P2, effort S

**Evidence.**
```rust
// store.rs:321-323 (and :366-368 in insert_verified)
staged.flush().map_err(|err| SyncError::io("flush lfs temp file", staged.path(), err))?;
// store.rs:479
match staged.persist(&target) {
```
`flush` empties the `NamedTempFile`'s userspace buffer; no `sync_all`/`sync_data` exists anywhere in `lfs/` (grep across the crate finds `sync_all` only in `copy.rs:610`, `files_write.rs:1038`, `volume.rs:203`). The reader is size-only:
```rust
// store.rs:253-256
std::fs::metadata(self.object_path(oid)).map(|meta| meta.is_file() && meta.len() == expected_size)
```
and `materialize` copies store→worktree on that check alone (`stage.rs:1762-1786`), re-verifying only the *target*'s pointer, never the object's digest.

**Why.** The crate already knows the pattern — `copy.rs:609-611` fsyncs before publishing a user file. Here a power loss between `persist` and writeback can leave a correctly-named, correctly-sized object whose contents are garbage, and `contains` will hand it to `materialize`, which writes it over the worktree file. That file is then the new content and the next commit re-hashes it into a *different* oid: silent corruption with no error anywhere. It is the one place where the object store is the only copy (a virtual path, §9).

**Fix.** `staged.as_file().sync_all()` before `persist`, and the same on the `.part` before `commit_partial`; optionally re-verify the digest in `materialize` for objects the store did not just write.

### F-LFS-6 `stage::clean` writes a full store copy even when the object is already there — the optimisation landed only in the filter — P2, effort S

**Evidence.** `LfsStore::digest_of` exists precisely for this and carries the measurement:
```
/// On a folder whose objects are almost all stored already, that copy is
/// the dominant cost of a status walk. … `tmp/` grew at 9.6 MB/s and stood
/// at 5.4 GB mid-walk, so the 24.2 GB of pointer content a walk re-reads
/// costs about 42 minutes of writing — every walk           (store.rs:238-249)
```
`lfs::filter::clean` uses it (hash first, `store.contains`, re-read only if missing). `lfs::stage::clean` does not:
```rust
// stage.rs:819
let (oid, written) = store.insert_streaming(file)?;
```

**Why.** `stage::clean` is what `prepare` calls for every path it routes, and the repair sweep hands it 500 paths per batch (`REPAIR_BATCH`, `engine.rs:372`). Every one pays a full temp-file copy of the content before learning the object was already stored — the exact cost the filter path was fixed for. On a 2 GB recording that is 2 GB of pointless writes to a USB APFS volume.

**Fix.** Give `stage::clean` the same two-question shape: `LfsStore::digest_of` over the opened handle, then `store.contains(&oid, size)`, and only `store_object_from_file` when it is genuinely absent. `filter.rs:335-350` is the function to reuse.

### F-LFS-7 `lfsNever` cannot undo a rule that already exists, and `ensure_lfs_rule` writes rules without consulting it — P2, effort S

**Evidence.** `lfs_never` is read in exactly one place, `LfsPolicy::from_profile` (`stage.rs:158-179`); grep finds no other production reader. `already_routed` (`stage.rs:1443-1459`) does not consult it, so an existing `*.md filter=lfs` line keeps routing every `.md` however small. Separately:
```rust
// engine.rs:4533-4534
let pattern = lfs::stage::pattern_for_extension(ext);
match lfs::stage::ensure_attributes(&profile.local_path, std::slice::from_ref(&pattern)) {
```
`ensure_lfs_rule` checks profile existence, `enabled`, `lfs_mode` and the folder's presence (`engine.rs:4494-4532`) — but never `lfs_never`, so a recording session writes `*.<ext> filter=lfs` for an extension the profile explicitly opted out of.

**Why.** `docs/sync.md` §8 sells `lfsNever` as "the escape hatch" against the per-extension rule, and adds "an opt-out that silently does nothing is how a note ends up an opaque pointer months later." The opt-out *does* silently do nothing whenever the rule predates it, which is the ordinary way a user discovers they need it. hesperia's tgdrive has 18 `lfsNever` text extensions against 179 committed `filter=lfs` rules; whether any pair overlaps I could not check from here, but nothing in the code prevents it.

**Fix.** Consult the compiled `lfsNever` set in `already_routed` and in `ensure_lfs_rule`, and have `ensure_attributes` retire a keeper-written managed-block rule whose pattern the profile now excludes. State in §8 that the opt-out is otherwise prospective.

### F-LFS-8 A failed *clean* under `required=false` makes git store the raw content as a blob, and only the smudge half of that is documented — P2, effort M

**Evidence.** Every per-path failure answers the same way regardless of direction:
```rust
// filter.rs:588-591
tracing::warn!(error = %err, "lfs filter: refusing one path");
pktline::write_line(output, "status=error")?;
```
`docs/sync.md:295-303` explains the consequence for smudge only ("git falls back for one path instead of emptying the rest"). For clean, keeper's own doc states the other half: "because the filter is registered as *not required* …, git could not tell a failed filter from an absent one and stored the bytes it was handed" (`docs/sync.md:599-601`).

**Why.** A clean can fail for ordinary reasons — `ENOSPC` in `.git/lfs/tmp`, the 900 s `REQUEST_LIMIT` watchdog exiting the process mid-request (`filter.rs:437-500`), an unplugged volume mid-commit. The result is a multi-gigabyte git blob, which `stage.rs:1-10` says is the one thing that must never happen because gitoxide has no streaming object read. It is permanent: nothing rewrites history, and the repair sweep only converts entries that are *already* routed and not yet pointers — which is exactly what such a blob is, so it will be re-cleaned and re-fail on every pass.

**Fix.** Distinguish the directions: on a clean failure emit `status=abort` (git's "do not process this path *or any later path* with this filter") rather than `status=error`, so a storage failure stops the commit instead of embedding raw content — and say in §8 which direction each status produces.

### F-LFS-9 `prune::plan` is O(tracked) index opens and object-header probes on every successful pass — P2, effort S

**Evidence.** `plan` walks every tracked path and calls `indexed_pointer` per path (`prune.rs:83-86`), and `indexed_pointer` re-opens the index and probes the ODB each time:
```rust
// stage.rs:963-967
let index = repo.index_or_empty().ok()?;
let entry = index.entry_by_path(…)?;
pointer_blob(repo, entry.id)
```
This is the ordering hazard `mismatched_filtered_paths` documents and avoids: "measured on the owner's volume, that ordering left keeper holding the index lock at 0% CPU for eight minutes, because it probed a pack for every one of 155 662 entries before asking the cheap question" (`stage.rs:1553-1558`).

**Why.** `prune_lfs_store` runs on `mark_synced`, i.e. on every successful pass (`engine.rs:5720-5721`), over hesperia's 155 626 tracked paths against 181 210 packed objects in 6 packs. `blob_could_be_a_pointer` reads a header rather than the object, which bounds the *bytes* but not the per-entry pack lookup. `audit::tracked_objects` (`audit.rs:107-123`) has the identical shape.

**Fix.** Hoist one `repo.index_or_empty()` and iterate its entries directly (as `indexed_pointers`, `stage.rs:1130`, already does), and pre-filter on blob size via one `find_header` before any `find_object`.

### F-LFS-10 Nothing ever collects an unreferenced object from the store — P2, effort M

**Evidence.** `prune::plan` is driven by *currently tracked paths* whose *current* indexed pointer names the object (`prune.rs:83-101`). There is no scan of `objects/` anywhere: grep for `read_dir` in `lfs/` finds only `sweep_scratch` over `tmp/` and `incomplete/` (`store.rs:195-199`) and two test helpers.

**Why.** Every superseded oid (edit a tracked large file: the old object stays), every deleted path's object, and every object whose branch was reset is unreachable to prune and lives forever. `docs/sync.md` §20 item 6 covers history growth and explicitly says "`lfsPruneLocal` is not this", but nothing anywhere states that orphaned *objects* are also never collected. hesperia currently holds 23 487 objects / 4.8 GB in `.git/lfs`; `GitCli::gc` has no production caller (evidence file), so a `git lfs prune`-equivalent would have to be keeper's own.

**Fix.** Add a bounded orphan sweep to the same `mark_synced` edge: walk `objects/**` shard by shard against the set of oids reachable from the index, deleting only objects with no journal and no index reference, one shard per pass. Document it in §8 beside `lfsPruneLocal`.

### F-LFS-11 A repair batch suppresses ordinary commits for that pass — P2, effort S

**Evidence.**
```rust
// engine.rs:6643-6646
let (staged, is_repair) = match self.repair_recorded_pointers(profile)? {
    Some(repair) => (repair, true),
    None => (self.collect_stable_changes(profile)?, false),
};
```

**Why.** While any repair backlog remains, the user's own edits are never staged. With `REPAIR_BATCH = 500` against the 73 536-entry backlog the module doc measures (`stage.rs:1477-1481`), that is ~147 passes — days — in which a folder commits nothing the user did. The quarantine and window fixes bound the *sweep*; they do not restore the user's throughput while it drains. It also compounds F-LFS-3's premise: each repair commit enqueues new uploads, so with the remote down (hesperia, 6 days) the outstanding count only grows and the push stays held.

**Fix.** Merge the two staged sets rather than choosing — a repair commit and the pass's settled changes are both `StagedChange`s over disjoint path sets — or interleave (repair every Nth pass).

### F-LFS-12 One watchdog thread per filter request — P3, effort S

**Evidence.** `RequestGuard::arm` spawns a thread for every request (`filter.rs:468-490`) and the thread polls `done` only after `sleep(Duration::from_secs(5))` (`:477`), so it outlives a finished request by up to five seconds.

**Why.** hesperia has 72 641 index paths resolving to `filter: lfs`; a foreign `git status` over them is 72 641 thread creations, with roughly 120 alive at once at the measured ~41 ms/file. It is small next to the round trip, but it is exactly the kind of per-file constant that made the filter path expensive in the first place — and this is a process that already ran a machine out of process-table slots once.

**Fix.** One watchdog thread for the process, holding an `AtomicU64` deadline the request loop rewrites; or a `Condvar` so the wake is immediate rather than a 5 s poll.

### F-LFS-13 `SizedFileBody` reallocates and zeroes a 64 KiB buffer for every frame — P3, effort S

**Evidence.**
```rust
// basic.rs:1085-1087, 1119
this.buf.clear();
this.buf.resize(want, 0);
…
let chunk = this.buf.split_to(filled).freeze();
```
`split_to` hands the front of the `BytesMut` away *with its capacity*, so the retained half has none; the next `resize(want, 0)` therefore allocates and memsets a fresh 64 KiB.

**Why.** One allocation plus one 64 KiB zeroing per frame — 32 768 of each for hesperia's pending 2.0 GB upload, and 450 GB of memset over a full-archive upload — for a body that never reads the zeros it writes.

**Fix.** `this.buf.reserve(CHUNK_BYTES)` and fill through `spare_capacity_mut`/`ReadBuf::uninit`, or keep a reusable `Vec<u8>` and pay a `Bytes::copy_from_slice` instead — either removes the memset.

## What is correct / well done

- **The pointer encoding is spec-conformant and the canonicality rule is the right one.** `version` first, remaining keys in one ASCII-ascending sort (`pointer.rs:206-232`), `oid sha256:` + 64 lower-case hex only (`:283-296`), mandatory trailing LF, CR rejected, `< 1024` bytes exclusive, duplicate keys refused, unknown keys preserved across a round trip. Crucially `is_canonical` is *derived* by re-rendering (`:171-174`) rather than checklisted, and every non-canonical read is passed through byte-for-byte instead of re-encoded — which is what stops "modified forever" on a pointer another client wrote. Compared against `git-lfs/docs/spec.md` as cited in the module header and against `lfs/pointer.go`'s `blobSizeCutoff = 1024` and `Pointer.Encoded()` (extensions emitted before `oid`; the empty pointer encodes to nothing) — **[INFERENCE], from memory of the git-lfs source.** keeper's general sort coincides with git-lfs's hardcoded layout for every `ext-N-*` key, and keeper never writes an extension itself (`Pointer::new` starts with none), so a divergence is unreachable in practice: a pointer keeper writes is byte-identical to git-lfs's, hence the same blob id.
- **pkt-line framing is exactly right.** The length counts its own four bytes (`pktline.rs:110-118`, test `:229`), `MAX_DATA = 65516` matches git's `LARGE_PACKET_DATA_MAX`, a closed pipe *between* packets is `Eof` while mid-packet EOF is an error (`:37-49`), and `read_text_list` stops precisely at the flush with a test asserting the next bytes survive (`:182-203`).
- **The long-running filter conversation matches git's documented exchange**, including the part most implementations get wrong: `status=success` + flush, then the content list + flush, then a *second* status list which is empty on success and `status=error` on a mid-stream failure (`filter.rs:596-608`). `delay` is deliberately not advertised (`:371-375`), the request is always drained before the response starts (`:377-388`, `Request::drain` at `:734-748`), and a smudge request that is not a pointer spills to disk rather than to memory (`:704-720`).
- **The gitoxide fork pin is real and proven, not aspirational.** `Cargo.lock:2591-2593` resolves `gix-filter 0.33.0` from `git+https://github.com/tgorka/gitoxide?branch=keeper%2Fgix-filter-0.33-reap#0ae3023`, exactly one copy of the crate is in the tree, and `tests/lfs_filter_process.rs:220` (`a_finished_status_walk_reaps_the_filter_children_it_launched`) proves it by reading the real process table. The upstream defect report is unusually good work.
- **The timeout design is the correct answer to §11's half-offline case.** `read_timeout` bounds *silence*, never duration (`http.rs:24-40`); LFS gets a separate 30-minute client (`http.rs:64-83`); and the upload — where the server is silent *because that is correct* — is guarded by a progress watchdog reading a shared `AtomicU64` from inside the body (`basic.rs:707-720`). This is precisely why hesperia's 2.0 GB upload can still succeed on a slow tailnet; the 15 attempts are consistent with connect timeouts (the log's `tcp connect error: deadline has elapsed`), not with a duration cap.
- **The streaming upload body is correct.** Exact `SizeHint` (`basic.rs:1129-1132`), never over-reads past `remaining` (`:1080-1084`), `is_end_stream` agrees with `poll_frame`'s `None`, no trailers, and a short source fails loudly instead of stalling a `Content-Length` promise (`:1094-1100`) — with a test for each (`:1500`, `:1549`). Every retry reopens the file and re-checks its length before sending (`:653-668`), which is right for a non-resumable adapter.
- **Download resume is safe by construction.** Digest over every byte including the ones not fetched this time; the cached hasher is handed back only when the `.part` is *exactly* the length it was (`store.rs:91-99`); the finished object is verified against oid *and* size before the rename. The Forgejo `Content-Range` workaround validates the start byte only, with the reason and a test (`basic.rs:449-465`, `:1173`).
- **Verify-after-upload is implemented**, unconditionally authenticated even when the content href was pre-signed, with the reason written down (`basic.rs:775-800`).
- **The Forgejo compatibility set is pinned by tests, not by comments**: LFS media type first in `Accept` using Forgejo's own `strings.Split(hdr,";")[0]` check (`batch.rs:33-42`, test `:770`), missing `transfer` means `basic` (`:897`), neither `actions` nor `error` on an upload means already-present (`:934`), a pre-signed href never gets a credential re-attached (`basic.rs:1315`).
- **`blob_could_be_a_pointer` reads the object header, not the index stat** (`stage.rs:901-908`) — the fix for the bug where every real LFS entry answered `None` because its stat is deliberately the worktree file's.
- **The 2026-08-27 repair livelock fix is real and tested.** `repairable_entry` removes the two shapes no commit can convert (`stage.rs:1412-1418`), `unconverted_after_repair` re-derives the answer from the index rather than trusting the commit's report (`:1582-1608`), `quarantine_repair` warns once per batch with a named example (`engine.rs:6556-6572`), and `upload_is_needed(size) = size > 0` closes the empty-object hold that deferred a push nineteen times (`engine.rs:6721-6726`). Tests at `engine.rs:18361-18492` pin all three, including "a path a commit did not move must not be staged a second time".
- **`LfsRouting` holds one attribute stack per pass** (`stage.rs:1349-1396`) instead of paying `O(index)` per path — the measurement (1321 of 1606 samples building stacks) is in the doc.
- **The push hold does not block pull or converge.** It is raised only in `do_push` (`engine.rs:6910-6919`) and classified `Retriability::Deferred` (`error.rs:273`), and the profile state distinguishes "waiting" from "stopped" by asking whether any upload is still being attempted (`engine.rs:5296-5299`). With hesperia's remote down for six days that is the correct behaviour, and it is why the folder still pulls.
- **`sweep_scratch` is now on the tick, not behind a button** (`engine.rs:2196`), with the 100 GB / 863 files field measurement recorded (`store.rs:170-178`).

## Open questions I could not settle

1. **Is F-LFS-1 live on hesperia today or only latent?** The evidence says "Only ONE `.gitattributes` is tracked and it is not a pointer today", but I cannot see whether the 179-rule managed block still contains an anchored `/…/.gitattributes` rule from the eclipse incident. If it does, the next commit touching that path re-converts it and the 1.3 M warnings return. `grep -n 'gitattributes' /Volumes/merope/tgdrive/.gitattributes` settles it in one command.
2. **Do any of tgdrive's 18 `lfsNever` extensions overlap its 179 committed rules?** That decides whether F-LFS-7 is a live opt-out failure or a latent one. A `comm` between the two lists answers it.
3. **Whether the 2.0 GB upload's 15 attempts ended in the stall watchdog or in `connect_timeout`.** The lines quoted in the evidence are `tcp connect error: deadline has elapsed`, which points at connect, but I did not see per-attempt lines for that specific oid.
4. **Whether `repo.index_or_empty()` re-parses or returns a cached snapshot per call** in this gix version. F-LFS-9's cost is a full re-parse in the worst case and a stat + `Arc` clone in the best; I read the call sites, not gitoxide's caching. `[INFERENCE]` either way — the fix (hoist the index) is correct under both readings.
5. **git's exact `status=abort` semantics for the clean direction** (F-LFS-8's proposed fix). I am confident git stores the unfiltered bytes on `status=error` under `required=false` — keeper's own `docs/sync.md:599-601` says so — but whether `abort` produces a *refused commit* rather than the same silent fallback should be checked against `git/Documentation/gitattributes.txt` before implementing.
