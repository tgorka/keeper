---
title: 'LFS over a filesystem remote'
type: 'feature'
created: '2026-07-29'
status: 'review'
baseline_revision: '1be95be'
---

<intent-contract>

## Intent

**Problem:** `lfs::endpoint::derive` refuses a filesystem remote outright — "remote is a local path
and has no LFS server" — and that refusal is correct. There is no server behind
`/Volumes/pendrive/photos.git`, and inventing one produces a confusing 404 later.

On its own, that correct refusal was silently destructive. `git push` copies its own objects into a
local bare repository perfectly well, so a profile synced to a pendrive published the **pointer**
while the object it names stayed in the source machine's `.git/lfs`. Every part of the system then
agreed nothing was wrong: `git push` succeeded, the remote's tree contained the file,
`git ls-tree` listed it. What it contained was ~130 bytes of text naming content the pendrive did
not have. Plug that drive into another machine and the file is a stub.

The LFS upload unit did surface a failure — `endpoint::derive` returns `SyncError::Config`, which is
`Permanent`, so the unit parked — but with story 34.15's push gate absent there was nothing to stop
the pointer being published first, and a parked unit in the Problems section named no file. So the
observable state was: the copy looks complete, and one row of a section elsewhere says
"Large file upload · stopped after 1 attempt".

This is not a marginal topology. AD-48 makes removable media a first-class profile kind, with a
volume marker, an absence-is-never-deletion rule and a whole `volume` module; "the video is too big
for git" is why LFS is in this engine at all. A pendrive that carries a tree of stubs is the
intersection of the two headline features.

It was found because story 34.15's gate made it fail honestly. With the push held until objects
land, `durability_matrix`'s `a_kill_during_a_large_object_transfer_leaves_the_object_recoverable` —
which uses a local bare repository as its remote — stopped passing, and revealed that its assertion
had been satisfied by a pointer for as long as it had existed.

**Approach:** Copy the object. A filesystem remote has no LFS *server* but it is perfectly capable
of holding LFS *objects*, and both sides use the client store layout because both sides are
clients. `lfs::local::remote_store` answers whether a remote is a path and where its store is;
`transfer` moves one object between two stores through `LfsStore::insert_verified`, which hashes as
it writes and publishes by rename only once both the digest and the length match. `do_lfs` branches
to it **before** resolving an endpoint, because there is no endpoint to resolve.

This is what upstream git-lfs calls a *standalone transfer agent*
(`lfs.standalonetransferagent`), which exists for exactly this topology. Nothing here is novel; it
was simply missing.

## Boundaries & Constraints

**Always:** the copy is streaming and bounded — `insert_verified` reads in 128 KiB chunks and
nothing sizes a buffer from the object (NFR-23), which is the same rule the rest of `lfs::store`
carries. It runs on `spawn_blocking`, because it hashes gigabytes. Verification is on arrival, in
both directions. An unmounted volume answers `SyncError::MediaAbsent`, which AD-48 makes absence
rather than failure. An explicit `.lfsconfig` outranks this branch entirely: someone who has named
an LFS server beside a pendrive remote meant it.

**Block If:** the remote path is not reachable — `MediaAbsent`, deferred until the volume returns.

**Never:** do not treat this as a fallback for a failed HTTP transfer. A remote is either a
filesystem path or a URL and the two never overlap, so this and the batch client are alternatives,
not a retry chain. Do not assume the remote's git directory: a bare repository is its own, a
non-bare one keeps it under `.git`, and guessing wrong creates a second store that never shares a
byte with the one git itself uses. Do not use the server-side sharding layout; both stores here are
client stores.

## I/O & Edge-Case Matrix

Remote shape → whether there is a local store:

| remote | `remote_store` |
| --- | --- |
| `https://…`, `http://…`, `ssh://…`, `git://…` | `None` — it has a real server; go and talk to it |
| `git@host:owner/repo.git` (scp-style) | `None` — a network remote, not a path with a colon in it |
| `/srv/git/r.git` | `Some(/srv/git/r.git/lfs)` |
| `file:///srv/git/r.git` | `Some(/srv/git/r.git/lfs)` |
| `file://localhost/srv/git/r.git` | `Some(/srv/git/r.git/lfs)` — an empty authority and `localhost` are the two RFC 8089 spellings of "this machine" |
| `file://nas.example/srv/git/r.git` | **`None`** — a named authority is somebody else's host to dial, and treating it as a local path was a live defect (below) |
| `file:///srv/my%20repo.git` | `Some(/srv/my repo.git/lfs)` — the URL form is percent-decoded, because that is the directory `git push` uses |
| `/srv/my%20repo.git` (a bare path) | `Some(/srv/my%20repo.git/lfs)` — **not** decoded: a directory genuinely named that way is legal, and the bytes the user typed are the answer |
| `C:/repos/r.git` | `Some(C:/repos/r.git/lfs)` — a drive letter is a path, which is what the one-character-authority guard protects |
| a path whose `.git` is a directory | the store sits under `.git/lfs` |

Transfer behaviour:

| condition | behaviour |
| --- | --- |
| the object is absent from the target | streamed, hashed and published; the byte count is returned |
| the object is already present at the right length | `Ok(0)`, nothing moved — a journal unit re-driven after a crash lands here every time |
| the source store lacks the object | `SyncError::Integrity` naming the oid, the expected length and "absent" — not an io error |
| the source is shorter or longer than the pointer claims | `Integrity`; nothing is published, so a retry starts from zero rather than from poisoned bytes |
| the volume is not mounted | `SyncError::MediaAbsent` → `Deferred`, waiting for the drive |
| direction is download | the same copy, reversed; `materialize_pending` then replaces the checked-out pointer |
| `lfsMode = disabled` | `do_lfs` returns before any of this |

</intent-contract>

## Code Map

**`keeper-sync/src/lfs/local.rs`** (new) — `remote_store` (path → `LfsStore`, `None` for a URL),
`remote_path` (the scheme and scp-style discrimination, borrowing `endpoint`'s two guards),
`transfer` (the verified copy, returning bytes moved) and `is_reachable` (asked of the store's
parent, because `lfs/` may legitimately not exist on a remote that has never received an object).
A fifth item, `describe`, shipped in the first pass and has been **deleted**: it was a `pub`
forwarder over `LfsStore::object_path` with no call site anywhere, and `transfer` already calls
`object_path` directly. Two spellings of one operation, one of them dead, is worse than the log line
it was meant to serve.

Two `remote_path` defects were fixed on the way, and the first is the more serious of the two:

- **A named `file://` authority was ignored.** The rule was written in the comment from the start —
  "the host is empty or `localhost`; anything else is a URL for somebody else to dial" — and never
  applied, so `file://nas.example/srv/r.git` yielded the *local* path `/srv/r.git`. That is this
  module's own failure mode arriving through the door this module installed: `do_lfs` consults
  `remote_store` before `endpoint::derive`, so the local branch won, the objects were copied into a
  same-named directory on **this** machine, the upload unit completed, story 34.15's gate was
  satisfied by that completion, and the pointer was published to a remote that will never hold the
  object. Before this transport existed the same URL produced a loud permanent refusal; returning
  `None` restores it.
- **A `file://` path was not percent-decoded.** `git remote add r file:///srv/my%20repo.git` pushes
  to `/srv/my repo.git`, so keeper has to resolve to the same directory or the objects land in a
  literally-`%20` sibling no `git` command will ever look in — a second store sharing no bytes with
  git's own, which is precisely what `remote_store`'s bare/non-bare check exists to prevent. Only the
  URL form is decoded; a bare path is bytes the user typed. A `%` that does not introduce two hex
  digits, and a sequence that decodes to invalid UTF-8, are both kept verbatim rather than refused —
  git's own `url_decode` rejects them, but the question here is "which directory", and the honest
  answer for a byte that cannot be decoded is the byte that was written.

**`keeper-sync/src/engine.rs`** — `do_lfs` branches to `copy_lfs_object` when `lfsconfig.is_none()`
and the remote is a path, ahead of `lfs_access`. `copy_lfs_object` checks reachability, picks the
direction, runs the copy on `spawn_blocking`, reports the bytes as transferred, and materializes
after a download.

**`keeper-sync/src/lfs/store.rs`** — unchanged, and load-bearing: `insert_verified` is what makes
the copy safe, and `object_path`'s doc comment is why the server-side layout is not used here.

**`keeper-syncd/tests/durability_matrix.rs`** — the strengthened assertion.

## Tasks & Acceptance

**Execution:**

1. Add `lfs::local` with the remote-store discrimination and the verified copy.
2. Branch `do_lfs` to it before endpoint resolution, and only when no `.lfsconfig` override exists.
3. Report the moved bytes as transferred traffic, and materialize after a download.
4. Strengthen the durability matrix to check the remote's object store rather than its tree.

**Acceptance Criteria:**

1. A profile whose remote is a filesystem path transfers its LFS objects into that remote's
   `lfs/objects`, verified.
2. The remote holds the content, not merely a blob for the path — checkable without keeper, by
   finding a file of the pointer's exact length under the remote's store.
3. An object already present is a no-op, so a re-driven unit succeeds rather than failing.
4. A truncated or absent source publishes nothing.
5. An unmounted volume defers rather than failing.
6. A `.lfsconfig` naming an LFS server is still honoured for a path remote.
7. A URL remote is unaffected and still uses the batch client.

## Design Notes

**Why a bare versus non-bare remote is checked rather than assumed.** A git remote is almost always
a bare repository, so `<path>/lfs` is the common answer — but a remote pointing at a working copy
keeps its git directory under `.git`, and both occur in the wild. Guessing would create a second
store that never shares a byte with the one `git` itself would use for that repository: objects
would exist on disk, in the wrong place, and every later `contains` would miss them. One `is_dir`
answers it, and the doc comment says which layout was found and why it matters.

**Why the bytes are reported as transferred traffic.** `add_transferred` is described as "bytes this
run moved over the network", and a copy to a pendrive is not a network transfer. It is still traffic
the user is waiting on: an SMB mount is a network by any honest reading, and even a local drive is a
bus with a rate the progress meter exists to show. A transfer that reported zero would make a
multi-gigabyte copy look like an idle run, which is the opposite of what the progress work in this
epic was for.

**Why this is an alternative and not a fallback.** It would have been possible to leave
`endpoint::derive` refusing, catch the refusal in `do_lfs` and copy instead. That reads as error
handling and would put a correct-and-expected outcome on a failure path — worse, it would mean a
genuinely misconfigured URL remote could fall into a local copy against a directory that happened to
exist. A remote is either a path or a URL; deciding that once, up front, on the same discrimination
`endpoint` already makes, keeps the two transports as peers.

**Why an explicit `.lfsconfig` still outranks it.** A repository that names its own LFS server has
settled the question, and the combination is real: a bare repository on a pendrive, with
`lfs.url` pointing at a server the objects should actually live on. Checking `lfsconfig.is_none()`
before the branch preserves that, and it is the same precedence `lfs_access` applies for an ssh
remote (story 34.17).

**Why a missing source object is `Integrity` rather than an io error.** `std::fs::File::open`
returning `NotFound` is technically an io failure, but "the store does not hold the object its
pointer names" is a fact about content, and on the download side it means the *remote* never received
it — which is the very failure this story exists to prevent, seen from the other end. An `Io` error
would name a path and an errno; `Integrity` names the oid, the length expected and "absent", which is
the sentence a human needs.

**Why `is_reachable` asks about the parent.** The obvious check is whether the store's own directory
exists, and it is wrong: a remote that has never received an LFS object legitimately has no `lfs/`
directory, so that check would report a perfectly attached drive as absent on the very first upload.
The git directory above it, which any successful `git push` to that remote has created, is the honest
question.

**Why not shell out to `git lfs`.** The same reason the rest of Epic 25 does not: it would add a
runtime prerequisite beyond the `git` binary AD-41 already requires, and `git-lfs`'s own standalone
transfer agent is a subprocess protocol we would have to implement anyway to drive. The copy is
twenty lines over machinery that already exists.

## Verification

**The load-bearing test is the durability matrix, and it is load-bearing because it used to lie.**
`a_kill_during_a_large_object_transfer_leaves_the_object_recoverable` seeds a 24 MiB file, kills a
sync mid-transfer, then syncs to completion and asserts the object reached the remote. Its remote is
a local bare repository, and it checked with `git ls-tree`, which reports the *blob* for a path — and
for an LFS path that blob is the pointer. So the assertion passed for as long as it existed while the
object never left the source machine. It now additionally walks the remote's own `lfs/objects` and
requires a file of exactly `BIG` bytes.

**Mutation-checked.** Making `transfer` return `Ok(0)` without copying makes the new assertion fail
— `… lfs/objects held: []` — while `git ls-tree` still reports `big.bin` present, which is precisely
the old false pass reproduced. The mutation was reverted and the suite is green.

**Unit tests, `keeper-sync/src/lfs/local.rs`:**

- `a_url_remote_has_no_local_store_and_a_path_does` — the discrimination table above, including the
  scp-style and drive-letter guards.
- `a_file_url_naming_a_host_is_somebody_elses_to_dial` — `file://nas.example/srv/r.git` is `None`,
  and `file://localhost/…` and `file:///…` are both `Some`. Without it the named-authority case
  copied objects into a local directory and reported success, which satisfied story 34.15's gate with
  a lie.
- `a_percent_escape_in_a_file_url_names_the_directory_git_names` — `file:///srv/my%20repo.git`
  resolves to `/srv/my repo.git`, while the bare path `/srv/my%20repo.git` does not decode. Both
  halves matter: decoding neither loses git's directory, decoding both invents a different one.
- `a_non_bare_remote_keeps_its_objects_under_dot_git` — both layouts, on real directories.
- `an_object_copied_between_stores_arrives_whole_and_verified` — 400,000 bytes so the streaming loop
  runs several times; asserts the reported byte count, that the target `contains` the object, that
  the bytes read back equal the source, and that a second copy reports `0` moved.
- `a_missing_source_object_is_named_rather_than_reported_as_an_io_error` — `Integrity`, not `Io`.
- `a_truncated_source_never_becomes_a_published_object` — a length mismatch publishes nothing, so
  `contains` still answers false afterwards.
- `an_unmounted_volume_is_absent_and_a_mounted_one_is_present_before_its_first_object` —
  `is_reachable` in both directions, and the second half is the one that matters: a real bare-repo
  directory with no `lfs/` yet answers **present**, because that is every pendrive on its first
  upload, and asking about the store's own directory instead of its parent would report a perfectly
  attached drive as absent. A path that does not exist answers absent, which the engine maps to
  `MediaAbsent`.

**Not covered, explicitly:**

- No test drives `copy_lfs_object` itself. The engine-level branch — the reachability check, the
  direction choice, the `spawn_blocking` hop, `add_transferred` and the post-download materialize —
  is covered only transitively by the durability matrix, which exercises it through a real
  `keeper-syncd` process. The seam between `do_lfs` and `lfs::local` has no direct test.
- Nothing has been run against a real pendrive, an SMB mount, or any volume that can be physically
  removed mid-copy. The `MediaAbsent` *decision* is now covered as a unit (above); what is not
  covered is a drive yanked during `insert_verified` — the atomic-publish reasoning says that leaves
  nothing behind, and that reasoning is still untested. The earlier claim here, that `MediaAbsent`
  "is exercised by pointing at a path that does not exist", was false when written: `is_reachable`
  had no test at all.
- The objects this transport publishes into a remote store are created from a `NamedTempFile`, so
  they land `0o600`, owned by the user running keeper. On the shared-bare-repository topology this
  story exists to serve, a second user's keeper or `git-lfs` cannot read them, and the failure reads
  as a filesystem mishap rather than a permission decision. Filed as DW-127; not addressed here.
- A relative or `~`-prefixed remote path is taken verbatim, so it resolves against the process's
  working directory rather than the repository (DW-128). Under systemd or an app bundle that is `/`.
  Unlike the `file://` authority defect above, this one was left deferred: it changes what an
  existing profile resolves to, which is a migration question rather than a patch.
- The `file://` forms are covered by `remote_path`'s unit tests — including the named authority and
  the percent escape — but not end to end.
- No figure here is a measurement. The 24 MiB and 400,000-byte sizes are fixture sizes, not
  benchmarks; nothing profiles the copy.
- The whole workspace, including this change, passed `cargo fmt`, `cargo clippy --workspace
  --all-targets -- -D warnings` and `cargo test --workspace` on macOS/arm64 — **as this story
  originally shipped.** The epic-34 review (2026-07-30) then fixed `remote_path`'s two defects,
  deleted `describe` and added three tests; the workspace has not been re-run green as one command
  since.
