# Lane: fetch / pull / converge / push / commit (the git-protocol half)

## Scope read

Code (all under `src-tauri/crates/keeper-sync/`):

- `src/git/mod.rs` (whole, 62 lines)
- `src/git/fetch.rs` 1–563 (structural) — `fetch`, `static_credential`, `classify`, `summarize`, `FlatProgress`
- `src/git/cli.rs` 1–1303 (structural + full arg-builder region 700–905), plus targeted reads of 300–700, 1103–1303
- `src/git/conflict.rs` 1–300
- `src/git/commit.rs` 1–663 (structural; full reads of 140–330, 300–560, 563–660)
- `src/git/push_http.rs` 1–263, 263–470, 473–623, plus grep of 637–760
- `src/git/repo.rs` 1–123, 123–303, 539–803, 2974–3033, plus greps for `open`/`trust`/`is_ancestor`
- `src/git/resolve.rs` 1–93
- `src/names.rs` 1–300, `src/provenance.rs` 1–300
- `src/engine.rs` 5769–6099, 6096–6433, 6422–6623, 6622–6903, 6899–7023, 7351–7583, 8644–8763, 12805–12820
- `src/http.rs` 39–118, `src/error.rs` (retriability region 260–300)

Vendored dependency source (`~/.cargo/git/checkouts/gitoxide-…/0ae3023`):

- `gix-transport/src/client/blocking_io/http/reqwest/remote.rs` 39–203
- `gix-odb/src/store_impls/loose/write.rs` 180–230; `gix-odb/src/store_impls/dynamic/write.rs` 1–60
- `gix/src/filter.rs` 290–297; `gix/src/open/permissions.rs` 214–218; `gix/src/config/cache/access.rs` 246–260, 417–421

Docs: `docs/sync.md` §5, §6, §7, §11, §16, §18, §19, §20; `docs/upstream/gitoxide-filter-process-leak.md` 1–73; `docs/constraints-and-limitations.md` (section index). Evidence: `../evidence-hesperia.md` in full, including the object-store line 43.

Measured (throwaway git fixtures under `/tmp`, no repository file touched): the conflict matrix below, and the auto-gc test in F-9.

## How it works

**Pull.** `do_pull` (engine.rs:6098) commits settled local work first so the merge meets a clean tree (6114), then `spawn_blocking`s `git::fetch::fetch` with one refspec — `+refs/heads/<branch>:refs/remotes/origin/<branch>` (6136-6139) — never shallow (6137). Credentials go through gix's programmatic callback (fetch.rs:157) instead of a helper process (AD-53). `summarize` (fetch.rs:299) derives `fast_forward` from `merge_base(local, remote) == local`. Apply is decided by ancestry, not by that bit: ahead → nothing (6222-6229), behind → `git merge --ff-only` (6235), diverged → `converge_with_conflict_copies` (6423). A phone takes `apply_in_process` (6359): gitoxide fast-forward or a refusal sentence, never a merge (AD-199).

**Converge.** Three `git diff --name-only` calls give ours/theirs/differing (6432-6445); the intersection minus `regenerable` gets a `.sync-conflict-…` copy via `std::fs::copy` (6480), then one `git merge --no-edit --allow-unrelated-histories -s ort -X theirs -X no-renames -m <msg>` (6486; args at cli.rs:832).

**Push.** `do_push` (6892) commits, refuses to publish while any LFS upload is outstanding (6906-6916), then `push_once` (6739): desktop `git push --porcelain -- origin <ref>:<ref>` (6770), phone `push_http::push` (6793) — keeper's own `git-receive-pack` smart-HTTP client with a `gix-pack` base-only pack (push_http.rs:337). One rejected push triggers exactly one reconcile+retry (`reconcile_and_retry_push`, 6840).

**Commit.** `Engine::commit` (7352) builds the author from `author_override`/device (commit.rs:394), stamps `Keeper-*` trailers (provenance.rs:104), enqueues LFS upload rows *before* the commit (7451-7487), then `stage_and_commit` (commit.rs:135) writes the index and folds the **whole** index into trees (`write_tree_from_index`, commit.rs:520).

## Findings

### F-PULLPUSH-1 A modify/delete divergence livelocks the profile permanently — P0, effort M

Evidence — measured against the exact vector `merge_theirs_args` builds (cli.rs:832-857):

```
rename vs modify     rc=1   CONFLICT (modify/delete): d/x.txt deleted in HEAD and modified in remote.
                            unmerged=2  MERGE_HEAD=yes
second merge         rc=128 error: Merging is not possible because you have unmerged files.
git merge --ff-only  rc=128 (same)
```

`-X theirs` resolves *content* conflicts only; it cannot resolve modify/delete. `merge_theirs` (cli.rs:331-339) has no failure branch, and a grep for `MERGE_HEAD|merge --abort|merge_abort|unmerged` across `keeper-sync/src` finds **no** production hit. The error text matches none of the AUTH/DIVERGED/NETWORK needles (cli.rs:1104-1136), so it falls through to `SyncError::GitCommand`, which is `Retriability::Transient` (error.rs:263-265).

Why: after one such divergence every later pull *and* every fast-forward exits 128, forever, with backoff and no user-visible cause. `cli.rs:823-827` records exactly this ending in the field ("`fatal: Exiting because of an unresolved conflict`… the profile stopped syncing entirely") and the fix applied then — `-X no-renames` — removed one *trigger* and left the trap. Worse, `write_tree_from_index` skips unmerged entries (`if entry.stage() != Stage::Unconflicted … continue`, commit.rs:530-532), so a commit taken while the index is unmerged omits the contested path from the tree and leaves `MERGE_HEAD` dangling — silent content loss on whichever side lost.

Fix: on a non-zero `merge_theirs`/`merge_ff_only`, run `git merge --abort` before returning, and add a `MERGE_HEAD`-exists precondition to `commit_local` that refuses to commit (or aborts first) rather than tree-building over an unmerged index. Then resolve modify/delete deterministically per AD-43 — `git checkout --theirs/--ours <path>` plus `git add` — driven by `conflict::resolve` (F-2).

### F-PULLPUSH-2 The AD-43 resolution matrix has no production caller, and the real behaviour contradicts it — P1, effort M

Evidence: `conflict::resolve` (conflict.rs:96-126) implements and exhaustively tests "a modification always beats a deletion" (conflict.rs:117-118). Grepping `conflict::resolve|Resolution::|ChangeKind::` across `keeper-sync/src`, `keeper-syncd/src` and `keeper/src` returns **only** the definition and its own tests. The one export the engine uses is `conflict_name` (engine.rs:6468; `keeper/src/notes_vault.rs:2056`).

Why: `docs/sync.md` §5 states the rule as product behaviour, and §18 claims §§1–8 are "implemented and verified". The measured behaviour is the opposite: modify/delete is not resolved at all (F-1), and if a later commit is ever taken over the unmerged index the surviving side is decided by which stage happens to be absent, not by AD-43. The most thoroughly tested module in this lane defends a policy nothing executes.

Fix: have `converge_with_conflict_copies` classify each contested path into `ChangeKind` from the three `diff_names` sets it already computes, call `resolve`, and apply `TakeRemote`/`KeepLocal`/`ConflictCopy` explicitly — so `-X theirs` is only ever asked the one question it can answer.

### F-PULLPUSH-3 A fetch has no read timeout: a half-open socket parks the profile forever — P1, effort S

Evidence: gix's reqwest transport builds its own client and sets exactly one timeout:

```rust
let client = reqwest::blocking::ClientBuilder::new()
    .connect_timeout(std::time::Duration::from_secs(20))
    .http1_title_case_headers()
```

(`gix-transport/src/client/blocking_io/http/reqwest/remote.rs:63-65` — no `read_timeout`, no total `timeout`.) `git::fetch::fetch` (fetch.rs:112) does nothing to bound it, and `do_pull` wraps it in a bare `spawn_blocking` with no `tokio::time::timeout` (engine.rs:6132-6153); a grep for `tokio::time::timeout` in engine.rs finds only the shutdown drain (3265).

Why: this is precisely the failure `docs/sync.md` §11 claims to have closed — "a socket whose peer has gone… nothing is delivered and no error arrives either. Without a timeout that transfer waits forever, and since the engine runs one operation per profile… everything queued behind it waits with it." §11 then says "All three live in `keeper_sync::http`, which is the only place a client is built" — untrue for the fetch path, which never touches that client. The hesperia log's `tcp connect error: deadline has elapsed` is gix's hard-coded 20 s **connect** timeout firing (not `http::CONNECT_TIMEOUT`, which is 15 s, http.rs:53); the connect phase is the only bounded phase. A tailnet peer that completes the TCP handshake and then goes silent hangs the fetch, and with it the profile's single in-flight operation, until the process restarts.

Fix: configure the transport's connect timeout through `gitoxide.http.connectTimeout` (`gix/src/config/tree/sections/gitoxide.rs:191`) so it is keeper's number rather than gix's, and — since the upstream reqwest transport exposes no read timeout — bound the whole `spawn_blocking` fetch with a generous `tokio::time::timeout` that sets `self.interrupt` and returns `SyncError::Network` on expiry. Correct §11's "only place a client is built" sentence either way.

### F-PULLPUSH-4 No `git` invocation has a wall-clock bound — P1, effort S

Evidence: `capture` ends with `command.output()` (cli.rs:617) with no timeout anywhere in the module; every verb funnels through it (`run` cli.rs:437 → `run_as` cli.rs:441-478).

Why: `GIT_TERMINAL_PROMPT=0`, `GIT_ASKPASS=""` and `stdin(Stdio::null())` (cli.rs:592-602) close the *prompt* hang, and they are good. They do not close the *network* hang: `git push` over HTTPS to a peer that accepted the connection and then stopped answering has no `http.lowSpeedLimit` set anywhere (grep for `lowSpeed` across `src-tauri/crates`: no matches), so it waits indefinitely on a `spawn_blocking` thread while holding the profile's reservation. Same shape as F-3, different door. `Command::output()` also accumulates stdout and stderr unbounded in memory; `STDERR_CAP` (cli.rs:77) is applied only after the whole buffer exists (cli.rs:469).

Fix: give `capture` a deadline (spawn plus a `try_wait` poll), kill the child on expiry and map it to `SyncError::Network`; and add `-c http.lowSpeedLimit=1000 -c http.lowSpeedTime=60` to `repository_config_args` so `git push` fails on a stalled transfer the way the LFS client already does.

### F-PULLPUSH-5 A profile with no stored credential fetches through the machine's credential-helper chain — P1, effort S

Evidence: `fetch` installs the static callback only when a credential exists —

```rust
if let Some(credential) = credential {
    connection.set_credentials(move |action| static_credential(&username, &secret, action));
}
```

(fetch.rs:157-171) — and `repo::open`/`open_read_only` pass `gix::open::Options::default()` with no config override (repo.rs:108-114). The other two doors both clear the chain unconditionally: `clone` uses `with_in_memory_config_overrides(["index.sparse=false", "credential.helper="])` (repo.rs:569) and the shim always prepends `-c credential.helper=` (cli.rs:693-700).

Why: `repo::clone`'s own doc spells out the hazard this leaves open — "it is not merely 'unauthenticated', it is *authenticated as somebody else*: whatever account the system git store happens to hold for that host, regardless of which profile is syncing" (repo.rs:548-556). On any Mac that has run `git config --global credential.helper osxkeychain`, a keeper profile whose token was never stored fetches as whichever account the keychain holds. Second order: removable media is opened `Trust::Full` (engine.rs:5837, repo.rs:110) = `Permissions::all()` (`gix/src/open/permissions.rs:215`), so a `credential.helper = !<command>` in the pendrive's own `.git/config` becomes an executed command. `docs/sync.md` §16 claims "Everything gitoxide drives — fetch and the first clone — takes them through a programmatic callback"; that is unconditional for the first clone and conditional for fetch.

Fix: clear the helper chain on the fetch path too — open the repository for fetching with an in-memory `credential.helper=` override, as `clone` does — and install `static_credential` unconditionally, answering `Get` with an explicit "no credential" when the profile has none, so the failure is `Auth` rather than a silent substitution.

### F-PULLPUSH-6 The phone's push cannot complete any pack that takes more than 60 s to send — P1, effort M

Evidence: `push_after_commit` is handed `self.http` (engine.rs:6750), i.e. `http::client` with `READ_TIMEOUT = 60 s` (http.rs:64). The pack is a `Vec<u8>` (push_http.rs:255, 338), copied again into `request_body` (`Vec::with_capacity(128 + pack.len())`, push_http.rs:645) and sent with `.body(body)` (push_http.rs:176).

Why: this crate already diagnosed the identical failure for LFS and wrote it down — "While a body is being sent the server says nothing, because that is correct — so a read timeout is a duration cap wearing a silence costume, and an object that needs longer than the cap can never finish however good the link is. Eight objects on a real folder retried every 61 seconds for eighteen hours" (`lfs/basic.rs:700-705`; the fix was `TRANSFER_READ_TIMEOUT = 30 min`, http.rs:83). The push path was never given that client. It is not hypothetical for a first push: `prepare` with `remote_old = None` walks the entire history with nothing hidden (push_http.rs:346-351) and writes every object as a plain base entry — the module docs are explicit, "no deltas, no thin pack" (push_http.rs:29-30). On a repository the size of tgdrive that is on the order of a gigabyte (evidence: 1.02 GiB in packs, 181 210 objects), held in RAM roughly three times over (entries + pack + body) on a phone.

Fix: use `transfer_http` (the 30-minute client) for the `git-receive-pack` POST, and stream the pack out of a temp file through a sized body instead of buffering it — `lfs::basic::SizedFileBody` exists for exactly this. Longer term, cap or refuse an unbounded first push, or enable delta compression in `build_pack`.

### F-PULLPUSH-7 `Trust::Full` on removable media honours every repo-scope filter driver; only `filter "lfs"` is sanitised — P2, effort M

Evidence: gix collects **all** filter drivers and rejects only by trust — `repo.config.resolved.sections_by_name("filter")…filter(|s| repo.filter_config_section()(s.meta()))` (`gix/src/filter.rs:291-297`) — and `Trust::Full` maps to `Permissions::all()` (`gix/src/open/permissions.rs:215`). keeper's two guards are both `lfs`-only: `drop_foreign_lfs_driver` matches `subsection_name() == "lfs"` (repo.rs:203-210) and the strip inside `enforce_local_config_with_filter` filters on the same subsection (repo.rs:745-753).

Why: `.gitattributes` is *synced content* — any peer can commit `*.md filter=x`. The driver definition must come from config, and on removable media `.git/config` is exactly the file another machine wrote. Nothing strips a `[filter "x"] clean = …` from it, so keeper's own status walk and checkout spawn it. The removable path is opened `Trust::Full` deliberately (AD-48, §6), and the volume marker is a JSON file on the same volume, so "the media is yours" is a weaker claim than "nobody has written to this `.git/config`". Related surface at the same trust level: `core.attributesFile`, `core.excludesFile` and `core.askpass` are read as trusted paths (`gix/src/config/cache/access.rs:247, 418, 199`). [INFERENCE] an `include.path` in the pendrive's `.git/config` would also survive keeper's rewrite, because `read_config` uses `from_path_no_includes` (repo.rs:797) while `gix::open_opts` follows includes — so an included `filter.lfs.process` would be neither seen by the strip nor classified as foreign scope.

Fix: strip **every** `filter.*` section that is not keeper's own `lfs` from the merged snapshot in `drop_foreign_lfs_driver`; and refuse (or ignore) a repo-scope `include.path` / `core.attributesFile` / `core.excludesFile` on a removable profile.

### F-PULLPUSH-8 Every commit rewrites the whole tree and the whole index, however few paths changed — P2, effort M

Evidence: `write_tree_from_index` iterates `index.entries()` in full and calls `write_tree` for every directory frame (commit.rs:520-590; `fold_top` 592-608); `write_tree` calls `repo.write_object` (commit.rs:618). gitoxide's write path has **no existence check** — `store::Handle::write_*` delegates straight to `loose_dbs[0]` (`gix-odb/src/store_impls/dynamic/write.rs:31-40`), which compresses into a tempfile and `persist`s it (`gix-odb/src/store_impls/loose/write.rs:190-231`; the crate's own note: "This will cost at least 4 IO operations"). The index is written in full first (commit.rs:316-317).

Why: on hesperia that is a 26.4 MB index write plus one zlib-compress-and-rename per directory of a 155 626-entry tree, on a USB APFS volume, to record `sync(tgdrive): 1 added` (evidence lines 40, 43). It is invisible in the walk numbers because it is not the walk. It is also real work when the object already sits in one of the 6 packs. [INFERENCE] the per-commit magnitude in directories: `loose 403` is consistent with a tree of a few hundred directories, so today this is seconds rather than minutes — but it scales with directory count, not with change size, and nothing caches it.

Fix: keep the index's tree extension instead of dropping it (`index.remove_tree()`, commit.rs:313) and rebuild only invalidated subtrees; or, as a three-line stopgap, short-circuit `write_tree` with `repo.has_object(id)` before writing.

### F-PULLPUSH-9 Three shim verbs are unreachable, `gc` never runs, and §7's "linked worktree" is not what the code does — P2, effort M

Evidence: a repo-wide grep for `worktree_add|worktree_remove|worktree_prune|\.gc\(` finds only the definitions (cli.rs:268, 275, 282, 305) and their tests (cli.rs:1443-1467, 1763-1783) — corroborated independently by the evidence file. `ensure_lane` (engine.rs:5784-5795) does `current_branch` plus `ensure_branch`, i.e. `git switch -c` in the profile's *own* worktree. Measured: `git merge` does not trigger auto-gc either (`gc.auto=1`, `gc.autoDetach=false`; loose 38 → 40 across a merge), and keeper spawns none of the commands that do (`commit`, `fetch`, `receive-pack`, `rebase`, `am`).

Why: `cli.rs:302-304` claims `gc` "is the only thing keeping a long-lived profile's object store bounded" — it bounds nothing, because nothing calls it. `docs/sync.md` §20.6 does declare "No automatic history pruning" deliberate, but justifies it by history *rewriting* being destructive; `gc --quiet` repacks and is not, so the dead verb is not covered by that limitation and the code comment is simply wrong. Separately, §7 states "Keeper creates a linked worktree on a generated branch" and §18 lists the review lane as verified; the engine only switches branches, so an agent writing into a lane writes into the user's own checkout. The base-branch guarantee does hold — `working_branch` (engine.rs:5772-5779) and the refspec (6961-6962) mean the base branch is never in a push — so the airlock's *safety* property is intact and only its *isolation* property is missing.

Fix: either call `gc` from the existing task scheduler (§14) on a cadence and delete the three worktree verbs, or wire `worktree_add`/`remove`/`prune` into lane setup. Correct `cli.rs:302-304` and `docs/sync.md` §7 to match whichever is chosen.

### F-PULLPUSH-10 Staging applies exactly one filter (LFS); any other `.gitattributes` clean rule makes the path permanently dirty — P2 (P1 once any peer commits `text=auto`), effort M

Evidence: `stage_and_commit` reads the worktree file raw and substitutes only an LFS pointer —

```rust
match substitutions.get(rela) {
    Some(pointer) => (mode, pointer.clone()),
    None => { let bytes = std::fs::read(&absolute)?; (mode, bytes) }
}
```

(commit.rs:238-249) — while `gix::status`, which produces the change set, compares through the full filter pipeline (repo.rs:196-206 documents `index_as_worktree` streaming content "through the filter").

Why: the blob keeper writes is the raw worktree bytes; the blob gix's status expects is the *cleaned* bytes. For `filter=lfs` they agree, because `lfs::stage::prepare` supplies the pointer. For any other attribute — `text`/`eol`/`text=auto` (built-in CRLF conversion), `ident`, or a user-defined `filter=<name>` reachable under F-7 — they do not, so the path reads as modified on every walk and every commit rewrites it identically. That is the self-perpetuating shape this crate has already recorded twice (repo.rs:160-168; the `repair_recorded_pointers` sweep, engine.rs:6572-6604). keeper's own `.gitattributes` writes only `filter=lfs` rules, so the trigger is user- or peer-authored — and `.gitattributes` is synced content, so one peer adding `* text=auto` propagates the state to every client. [INFERENCE] on the loop itself; the two code facts are direct.

Fix: run staged content through `repo.filter_pipeline(...)`'s `convert_to_git` and use its output as the blob, keeping the LFS substitution as the pre-step; or, cheaper, detect at commit time any staged path whose resolved attributes name `text`/`eol`/`ident`/a non-`lfs` filter and refuse it by name rather than committing a blob that can never match.

### F-PULLPUSH-11 Conflict copies: untracked until a later pass, overwritable, and lossy for non-UTF-8 names — P2, effort S

Evidence: `converge_with_conflict_copies` writes the copy with `std::fs::copy(&source, &destination)` (engine.rs:6480) into the worktree and then merges; nothing stages it. The stamp is per pass (`conflict_stamp(self.platform.now_ms())`, engine.rs:6260) and the name is `<stem>.sync-conflict-<ts>-<device>.<ext>` (conflict.rs:146-167), so two passes within one second on one path silently overwrite the first copy (`fs::copy` truncates). The stem comes from `name.to_string_lossy()` (conflict.rs:157-158).

Why: `docs/sync.md` §5 promises "Both are committed as ordinary tracked files, so every peer sees the same pair" — they are committed only by whichever *later* pass's `commit_local` picks them up, and a crash or an `LfsUploadPending` hold in between leaves the one artifact the user must act on untracked and unpublished. The hesperia evidence shows the shape: a 92-byte `events.sync-conflict…` object stuck at 356 upload attempts while the profile's push sat `deferred`. The lossy stem also contradicts `names.rs`'s own doctrine — "a lossy rendering is fine to *show* and must never be used to *reach*" (names.rs:33) — and here it is used to *create*, so two distinct non-UTF-8 siblings collapse onto one copy name. On the LFS question specifically: the copy preserves the extension, so it re-matches the same `*.ext filter=lfs` rule and the losing content gets its own object and its own upload unit — it is not a dangling pointer, **provided** the worktree held content rather than a pointer when the copy was taken (materialize mode). For a path the folder keeps virtual, `fs::copy` copies pointer text.

Fix: stage and commit the conflict copies inside `converge_with_conflict_copies`, in the same commit as the merge; use `create_new(true)` plus a numeric suffix on collision; and build the copy name from the raw path bytes (`OsStr`) rather than from `to_string_lossy`.

### F-PULLPUSH-12 Nothing is ever shallow or partial, and the first clone takes every branch — P2, effort S

Evidence: `do_pull` sets `shallow: None` (engine.rs:6137); `open_repo` calls `git::repo::clone(..., None, ...)` for `shallow_depth` (engine.rs:5893-5900). Both knobs are plumbed (fetch.rs:39, repo.rs:567) and never non-`None` in production. `clone` sets `with_ref_name(Some(branch))` but leaves gix's default fetch refspec, so all heads are fetched; steady-state fetches are correctly narrowed to one branch (engine.rs:6138).

Why: the product stores "files, not code". On hesperia a fresh clone of tgdrive pays 1.02 GiB of packs and 181 210 objects for 476 commits, plus 4.8 GB of LFS, to reach a state the user only wants the tip of. There is no `--filter=blob:none` path either — which is the shape a sync product over git normally wants, and the same shape the virtual-file tier already implements one layer up for LFS.

Fix: expose the existing `shallow` knob per profile and default a *new* clone to a bounded depth (pre-boundary history stays recoverable from the remote); evaluate gix's partial-clone support for `blob:none` as the strategically right answer for a files-not-code store.

### F-PULLPUSH-13 File/directory and case-only collisions hit the same unabortable trap — P2, effort S (subsumed by F-1's fix)

Evidence (measured, same vector):

```
dir-vs-file       rc=1  CONFLICT (file/directory): directory in the way of d from HEAD;
                        moving it to d~HEAD instead.       unmerged=1  MERGE_HEAD=yes
case-only rename  rc=1  CONFLICT (modify/delete): f.txt deleted in HEAD and modified in remote.
                        unmerged=2  MERGE_HEAD=yes
```

Why: hesperia's repositories run `core.ignorecase=true` (evidence line 41), so a case-only rename made on a Linux peer arrives at a macOS client as exactly this. Both leave the profile in the F-1 livelock, and the `d~HEAD` rescue file appears in the worktree with nothing telling the user it exists.

Fix: as F-1, plus surface `~HEAD`-style rescue paths as conflict activity rows so they are not silent litter.

### F-PULLPUSH-14 `docs/sync.md` §11 asserts a single HTTP client that the fetch path does not use — P3, effort S

Evidence: §11 — "All three live in `keeper_sync::http`, which is the only place a client is built." gix-transport builds its own (`…/reqwest/remote.rs:63`) and `git push` uses libcurl inside the `git` binary. Two of the three network legs in this lane are outside that claim.

Why: this sentence is what a maintainer reads before deciding whether the half-offline case is handled, and it is the reason F-3 and F-4 are easy to miss.

Fix: rewrite the paragraph to name which legs `keeper_sync::http` covers (LFS batch, LFS transfer, forge API, phone push) and state explicitly what bounds fetch and `git push`.

### F-PULLPUSH-15 Two clones of one remote on one machine share nothing, and there is no local peer path — P3, effort M

Evidence: the credential key is per profile (`format!("sync/{}/credential", self.id)`, profile/mod.rs:1167-1169), the LFS store is per `.git` (`LfsStore::in_git_dir`, lfs/store.rs:134), and journal rows are keyed by profile id. Exactly one remote exists per profile: `adopt`/`ensure_remote` write `remote.origin.url` (repo.rs:2737, 2921) and every fetch and push names `"origin"` literally (engine.rs:6141, 6770).

Why: on hesperia this is measured rather than theoretical — `tgdrive` and `tgdrive-light` are two clones of the same remote on one machine, and `tgdrive-light` sat **7 days behind** `tgdrive` because the shared remote was down and there is no route between two checkouts on the same disk (evidence lines 40-41). The user must also store the same token twice and stores the same LFS objects twice (4.8 GB per clone). This is coherent with AD-42 (one profile, one folder, one remote), but I found no document stating it as a deliberate limitation; §20 does not mention it.

Fix: record it in §20 with its cost, or add an optional second remote a profile may fetch from when `origin` is unreachable — the transport work is already done, since `fetch` handles a filesystem remote with no host (fetch.rs:143-145).

## Table: every `git` command keeper runs

Every invocation made with a `cwd` is prefixed with `-c core.hooksPath=/dev/null/keeper-runs-no-repository-hooks` (cli.rs:686-691) and `-c credential.helper=` (plus keeper's own env-reading helper when a credential is present) (cli.rs:693-700). Environment on every spawn: `GIT_TERMINAL_PROMPT=0`, `GIT_ASKPASS=`, `SSH_ASKPASS=`, `SSH_ASKPASS_REQUIRE=never`, `LC_ALL=C`, `LANG=C`, `stdin=/dev/null`, stdout and stderr piped (cli.rs:588-604). **Not** set: `GIT_OPTIONAL_LOCKS` (correctly unnecessary — no command below refreshes the index), `GIT_CONFIG_NOSYSTEM`. No timeout on any of them (F-4).

| # | Command | Builder | Verb | Production call site | Destructive? | Guard |
|---|---|---|---|---|---|---|
| 1 | `push --porcelain -- <remote> <refspec>` | cli.rs:702 | cli.rs:248 | engine.rs:6770 | remote refs | `force` hard-coded `false` (cli.rs:252); refspec is `working_branch` only (engine.rs:6961-6962); no `--force` reachable outside the module |
| 2 | `worktree add --no-track -b <br> <abs>` | cli.rs:718 | cli.rs:268 | **none** | creates a directory | `absolute_arg` refuses relative paths (cli.rs:895-910) |
| 3 | `worktree remove <abs>` | cli.rs:732 | cli.rs:275 | **none** | **deletes a directory** | no `--force`, so git refuses a dirty worktree |
| 4 | `worktree prune` | cli.rs:741 | cli.rs:282 | **none** | metadata only | — |
| 5 | `sparse-checkout set --cone <paths…>` | cli.rs:751 | cli.rs:289 | engine.rs:5997 | **removes worktree files outside the cone** | applied only when the cone differs from what git holds (engine.rs:5990-5999); a subpath that is empty or starts with `-` is refused (cli.rs:758-768); git leaves locally modified files behind with a warning |
| 6 | `sparse-checkout disable` | cli.rs:775 | cli.rs:296 | engine.rs:5987 | restores files | only when patterns exist and the profile names none (engine.rs:5981-5988) |
| 7 | `gc --quiet` | cli.rs:780 | cli.rs:305 | **none** (test only, cli.rs:1783) | repacks | — (F-9) |
| 8 | `merge --ff-only --quiet <ref>` | cli.rs:800 | cli.rs:319 | engine.rs:6235 | **moves the worktree** | `--ff-only`; `commit_local` ran first (engine.rs:6114); `is_ancestor` ahead-check (6222) and `outcome.fast_forward` (6233) |
| 9 | `merge --no-edit --quiet --allow-unrelated-histories -s ort -X theirs -X no-renames -m <msg> <ref>` | cli.rs:832 | cli.rs:331 | engine.rs:6486 | **overwrites contested paths with the remote's revision** | conflict copies written first (engine.rs:6446-6483); tree made clean by `commit_local`; `safe_ref` refuses a `-`-leading ref (cli.rs:786-795). **No `--abort` on failure** (F-1) |
| 10 | `merge-base <a> <b>` | cli.rs:861 | cli.rs:342 | engine.rs:6432 | no | `safe_ref` on both arguments |
| 11 | `rev-parse --verify --quiet refs/heads/<b>` | cli.rs:866 | inside cli.rs:354 | engine.rs:5794 | no | `safe_ref`; a failure is an answer, not an error |
| 12 | `switch --quiet [-c] <branch>` | cli.rs:876 | cli.rs:354 | engine.rs:5794 | **changes the checked-out branch of the user's folder** | only when `lane == Worktree` (engine.rs:5786-5788); git refuses a switch that would clobber local modifications |
| 13 | `symbolic-ref --quiet --short HEAD` | inline, cli.rs:366-373 | cli.rs:364 | engine.rs:5789 | no | a detached HEAD maps to `Ok(None)` (cli.rs:381-386) |
| 14 | `merge-base --is-ancestor <a> <b>` | cli.rs:886 | cli.rs:397 | engine.rs:6222, 12815 | no | exit 1 is the answer, not a fault (cli.rs:404-408) |
| 15 | `diff --name-only <from> <to>` | cli.rs:896 | cli.rs:425 | engine.rs:6434, 6438, 6443 | no | `safe_ref` on both arguments; output unbounded in memory (F-4) |
| 16 | `--version` | cli.rs:216 | cli.rs:232 | `resolve.rs` candidate probe | no | no `cwd`, so no `-c` prefix (cli.rs:687-689) |

**Not run by keeper at all:** `git status`, `git add`, `git commit`, `git fetch`, `git clone`, `git reset`, `git checkout -f`, `git clean`, `git rebase`, `git push --force`. Grep-confirmed: no `reset`, `checkout` or `clean` string appears in any argument builder, so there is no `reset --hard`/`checkout -f` on a dirty worktree anywhere in the engine. Consequently the "155k-line `status --porcelain`" buffering question does not arise — status is gix, in process.

The 147 orphaned processes in the field report were therefore **not** `cli.rs` children: `Command::output()` reaps synchronously. They are gix-filter's leaked long-running `process` filter children — keeper spawned as its own `filter.lfs.process` by gix's status and checkout workers — documented at `docs/upstream/gitoxide-filter-process-leak.md:1-73` ("one `Repository::status()` abandons on the order of `N_threads + 2` independent `State`s"; field count 274 zombies in 10 h 29 m) and fixed in the pinned fork `keeper/gix-filter-0.33-reap` (`src-tauri/Cargo.toml:227`). The separate consequence of registering `filter.lfs.process` in `.git/config` (repo.rs:770-781) is that **any foreign `git` a human runs in that folder spawns keeper** — one helper per git process, keeper's binary path baked into the repository's config, `required = false` so a moved binary degrades to pointers rather than bricking the folder (repo.rs:783-786).

## Conflict matrix (measured against `merge_theirs_args`' exact vector)

Command under test: `git merge --no-edit --quiet --allow-unrelated-histories -s ort -X theirs -X no-renames -m m <ref>`, in a throwaway repository. `rc`, unmerged-entry count and `MERGE_HEAD` presence were read directly after each run.

| Case | AD-43 / `conflict::resolve` says | `-X theirs` actually does | rc | Leaves MERGE_HEAD | Evidence |
|---|---|---|---|---|---|
| modify / modify (text) | `ConflictCopy` — remote wins the path, local copied aside | remote content wins; the copy was written by the engine beforehand | 0 | no | conflict.rs:122; engine.rs:6480-6486 |
| modify / modify (binary) | `ConflictCopy` | remote content wins, no markers written | 0 | no | measured (`b.bin`) |
| add / add, same path, different content | `ConflictCopy` | remote content wins | 0 | no | measured (`new.txt`) |
| delete / delete | `Nothing` | clean | 0 | no | conflict.rs:105 |
| unchanged / added, modified or deleted | `TakeRemote` | remote applied | 0 | no | conflict.rs:110 |
| added, modified or deleted / unchanged | `KeepLocal` | local kept | 0 | no | conflict.rs:113 |
| **local delete / remote modify** | `TakeRemote` (modification beats deletion) | **CONFLICT (modify/delete)** — "Version remote of f.txt left in tree" | **1** | **yes, 2 unmerged** | measured; conflict.rs:118 |
| **local modify / remote delete** | `KeepLocal` (modification beats deletion) | **CONFLICT (modify/delete)** — "Version HEAD of f.txt left in tree" | **1** | **yes, 2 unmerged** | measured; conflict.rs:117 |
| **local rename / remote modify of the old path** | `KeepLocal` for the new path, `TakeRemote` for the old | **CONFLICT (modify/delete)** on the old path | **1** | **yes, 2 unmerged** | measured |
| remote rename / local delete of the old path | — | merges cleanly — this is the case `-X no-renames` was added for | 0 | no | measured; asserted at cli.rs:1263-1300 |
| rename / rename, both sides, different targets | — | both targets kept, the old path gone | 0 | no | measured |
| **directory vs file at one path** | not modelled | **CONFLICT (file/directory)** — "moving it to `d~HEAD` instead" | **1** | **yes, 1 unmerged** | measured |
| **case-only rename vs modify** (`core.ignorecase=true`) | not modelled | **CONFLICT (modify/delete)** | **1** | **yes, 2 unmerged** | measured; evidence line 41 |
| LFS pointer vs LFS pointer, both modified | `ConflictCopy` | the remote pointer wins the path; the local *content* is copied aside from the worktree, re-matches the same `*.ext filter=lfs` rule and gets its own object and upload unit — so it is **not** a dangling pointer, provided the worktree held content rather than a pointer | 0 | no | engine.rs:6476-6483; conflict.rs:155-166 |
| path matched by `regenerable` | — | no copy; recorded in `Converged.stale` and warned once | 0 | no | engine.rs:6459-6464, 6288-6301 |
| contested path deleted locally | — | `!source.is_file()` → no copy is attempted | — | — | engine.rs:6477-6479 |

The bold rows are the F-1 / F-13 trap: `merge_theirs` returns `Err`, `SyncError::GitCommand` is classified `Transient`, nothing aborts, and every subsequent merge exits 128.

## What is correct / well done

- **Argument vectoring and injection defence.** Every invocation is an argv, never a shell string. `safe_ref` (cli.rs:786-795), `absolute_arg` (cli.rs:895-910) and the `sparse_set_args` `-`-prefix refusal (cli.rs:758-762) close the option-injection holes, and the builders are pure so the vectors are asserted without spawning anything (cli.rs:1408-1461).
- **Credential hygiene on the doors that have it.** `Credential`'s hand-written `Debug` redacts *both* fields, with the right reason (fetch.rs:67-75). The shell-out helper reads the secret from the environment rather than argv because "`ps` shows every process's argv to any user on the box" (cli.rs:39-46). `scrub_userinfo` is applied even at trace level (cli.rs:608-614) and to every server-supplied line in `push_http` (push_http.rs:196-199).
- **`-X no-renames`, with the measurement behind it.** cli.rs:815-830 records 138 311 unmerged paths from a real housekeeping move and explains why rename detection is a liability for a machine-reconciled tree. The test at cli.rs:1263-1300 drives real `git` rather than asserting the flag — "the vector proves the flag is passed, and only git proves the flag is the right one."
- **Ancestry instead of one bit.** `do_pull` refuses to read `fast_forward == false` as "diverged" and asks `is_ancestor` (engine.rs:6215-6230), which is what stops a merge-loop against a remote the profile is merely ahead of. `push_http` applies the same guard client-side, before any POST (push_http.rs:288-329), so AD-50's no-force rule is enforced on the phone rather than delegated to the server.
- **Bounded reconcile.** `reconcile_and_retry_push` is exactly one fetch-and-merge and one retry, with the field measurement that motivated it (613 rejected pushes in 135 s) written down, and the second failure demoted to `RemoteMoved`/`Transient` so the scheduler's jittered backoff takes over (engine.rs:6786-6890; error.rs:146-158). Two or three clients cannot ping-pong: each pass makes at most two push attempts.
- **Ordering of the LFS debt against the commit.** Enqueue-before-commit, with the failure direction reasoned out explicitly — "an unreferenced object on the remote costs disk; a lost obligation costs the file" (engine.rs:7431-7449) — and the zero-byte-object skip carrying the field case that motivated it (engine.rs:7455-7470).
- **Provenance as trailers**, with newline sanitisation as an explicit injection guard (provenance.rs:196-205) and the blank-line-before-trailers invariant pinned by a test that would otherwise silently rot (cli.rs:1205-1220).
- **`core.hooksPath=/dev/null/…`**, chosen so lookups fail with `ENOTDIR` rather than merely being absent (cli.rs:660-673), with the `git lfs install` hook failure that motivated it recorded in full.
- **Empty-commit avoidance** by comparing the built tree against `HEAD`'s (commit.rs:349-357), and the `sorted_len == 0 && !deleted.is_empty()` guard placed *above* the index write with the reason for that placement written out (commit.rs:169-196) — a genuine data-loss stop.
- **`parse_version` requiring the literal `git version`** after `python3 --version` was once accepted as git 3.13 and driven for every push (cli.rs:915-945).
- **Lock debris recovery**: `index.lock` older than 60 s and abandoned loose-ref locks are cleared with an explicit "a fresh lock belongs to a human's own `git`" carve-out (repo.rs:245-303), and only on the writing door — `open_read_only` deliberately repairs nothing (repo.rs:86-108).

## Open questions I could not settle

1. **Whether `collect_stable_changes` survives an unmerged index at all.** F-1 establishes that `merge_theirs` leaves one and that `write_tree_from_index` would drop those paths from the tree. What gix's `index_as_worktree` does with stage 1/2/3 entries — error, ignore, or report them as changes — decides whether the aftermath is "the profile stops" or "the profile commits a tree missing the contested paths". Needs a run against a real conflicted index.
2. **The per-commit directory count on tgdrive**, i.e. how many loose-object writes F-8 actually costs per commit. `loose 403` is suggestive, but a `gc` may have packed the rest; `git ls-tree -d -r HEAD | wc -l` on hesperia would settle it.
3. **Whether reqwest's `read_timeout` really applies while the `git-receive-pack` POST body is being sent** (F-6). This crate's own LFS notes say it does (`lfs/basic.rs:700-705`), which is why I rated it P1, but I could not exercise it here.
4. **Whether a repo-scope `include.path` survives keeper's config rewrite and can re-introduce a foreign `filter.lfs.process`** — marked `[INFERENCE]` in F-7. Needs a fixture with an include chain opened at `Trust::Full`.
5. **The 1.3 M `gix_attributes` warnings** of 2026-08-27 (an LFS pointer parsed as `.gitattributes`). Nothing in this lane can write that file as a pointer — `lfs::stage::prepare` runs on `candidates` before `.gitattributes` is appended (engine.rs:7381 vs 7395-7402) — so the mechanism belongs to the LFS staging lane, not here.
