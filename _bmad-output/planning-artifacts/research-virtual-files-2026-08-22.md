---
name: 'keeper'
type: research
topic: 'virtual files — LFS content that is present as metadata and materialized on demand'
decision: 'how keeper should let a clone hold knowledge of large LFS-tracked content without holding its bytes, materialize it on request, and release the materialization again — safely'
status: final
created: '2026-08-22'
run_folder: _bmad-output/planning-artifacts/research/virtual-files-2026-08-22
digests:
  - digests/r1-git-native.md — git-native selective fetch, git-annex, DVC, partial clone
  - digests/r2-virtual-filesystems.md — macOS File Provider/FSKit, Linux FUSE/fanotify HSM, Windows cfapi, real implementations
  - digests/r3-eviction-safety.md — proof-before-drop, atomic replace, in-use detection, TTL policy, mass-hydration threats
  - digests/r4-metadata-and-patterns.md — metadata carriers, pointer formats, gitignore/gitattributes semantics, Rust crates + licences
context:
  - context/g1-lfs-machinery.md — keeper's existing LFS + materialization machinery (path:line)
  - context/g2-surfaces.md — the daemon, IPC, frontend, progress and platform surfaces (path:line)
  - context/g3-bmad-state.md — BMAD conventions, free numbers
---

# Research — Virtual files: metadata without bytes, materialization on demand

**Evidence grades.** `[SOURCE]` = external primary source, cited in the named digest with URL
and access date 2026-08-22. `[REPO]` = read out of this worktree, cited `path:line`.
`[INFERENCE]` = reasoning over cited facts, no source of its own. `[UNVERIFIED]` = looked for,
not found — never to be repeated as fact.

**How to cite this document.** Sections are numbered `§N.M` and are stable. Rust doc-comments in
this tree already cite research this way (`keeper-sync/src/lfs/basic.rs:50` cites *research
§5.10*); new code that implements a decision here should cite the section that justifies it.

---

## 1. The question, and the answer in one paragraph

The owner's ask, restated: a clone must be able to **not download** selected LFS-tracked
content, must still **know that content exists** (with metadata), must be able to
**materialize** a file on demand and **release** that materialization afterwards — lazily is
fine, 24 h after last use, in a nightly pass. Primary host is the `keeper-syncd` server;
the desktop app matters; an OS-level presentation that makes the absent file show up in `ls`
and Finder would be a bonus.

**The answer this research supports:** build it as a **pointer-in-the-worktree** design with an
explicit hydrate/dehydrate verb pair and a keeper-owned last-use ledger — *not* as a
filesystem virtualization. Every shipped git-native system in this space made the same choice
(§2), keeper already holds four of the five primitives (§9), and the one primitive it lacks —
a safe *dehydrate* — is a small, well-bounded addition whose failure modes are documented in
detail by other people's incidents (§6). OS-level placeholders are **not** a deferred version
of the same feature: on macOS they are structurally unable to decorate an arbitrary worktree
path (§4.1), and on Linux they cost either a mount or a privileged kernel-version-gated daemon
(§4.2). They are a separate, optional, later deliverable with a different shape (§11.3).

---

## 2. Prior art splits into exactly two families

`[SOURCE]` r1 §Verdict, r2 §Verdict.

| Family | Members | What `ls -l` shows | Offline behaviour | Cost |
| --- | --- | --- | --- | --- |
| **Placeholder in the worktree** | git-lfs pointer, git-annex (locked symlink / unlocked pointer file), DVC `.dvc`, macOS `.icloud` plists | the **placeholder's** size (~130 B for LFS) | fully usable: metadata is on disk, an explicit command fails loudly | none — it is ordinary files |
| **Filesystem virtualization** | VFS for Git (ProjFS), EdenFS/Sapling, rclone/JuiceFS mounts, Dropbox/OneDrive/iCloud, hf-mount | the **real** size | blocks the syscall, or `EIO` | a mount, a privileged daemon, or a platform entitlement |

Two findings decide the family for keeper:

1. **The only git-specific member of the virtualization family was abandoned.** VFS for Git was
   retired and Microsoft moved to Scalar (partial clone + sparse-checkout, no virtualization);
   the macOS port died with the kernel APIs it needed. `[SOURCE]` r2.
2. **The placeholder family degrades to "you can still read the metadata"; the virtualization
   family degrades to "your syscall hangs".** For a stated requirement of *"above all simple
   and safe"*, that asymmetry is the whole argument. `[SOURCE]` r2 §Verdict.

### 2.1 The `git status` constraint eliminates the clever middle options

A sparse file, a truncated file, or a zero-filled file of the right length is *different
content* from the checked-out blob, so git reports the path modified forever. git-lfs escapes
this only because **the pointer IS the committed blob**: a worktree full of pointers is clean by
construction. `[SOURCE]` r2 §Verdict. keeper's own tree states the same invariant — *pointer
blob + worktree stat = clean status* `[REPO]` `git/repo.rs:1946-1947`, and the failure when it
is violated is recorded in anger: hashing pointer text emits a pointer naming a pointer, the
path reads MODIFIED, and a commit that accepts it replaces every peer's only reference to the
real object with a reference to 130 bytes of text `[REPO]` `lfs/filter.rs:232-254`.

**Consequence:** any virtual-file representation that is *not* the exact committed pointer blob
is disqualified before any other consideration.

---

## 3. Selective fetch, and the pattern-file question

### 3.1 git-lfs already ships the mechanism, including its dialect

`[SOURCE]` r1 §1, §3.

- `lfs.fetchinclude` / `lfs.fetchexclude` are comma-separated path lists matched *"using
  wildcard matching as per gitignore(5)"*, and they are honoured by **fetch, smudge and prune**.
- `git lfs smudge` on an excluded path copies the pointer to stdout unchanged — i.e. a
  gitignore-style list already yields per-path pointer-vs-content behaviour at checkout, with no
  filter replacement.
- `.lfsconfig` is a **committed, repo-scoped** config file, and `lfs.fetchinclude`/`fetchexclude`
  are on its security allow-list. Local git config overrides it.
- `git lfs checkout` is the offline materializer: it writes real content where the worktree holds
  *"placeholder pointer content with the same SHA"*, downloads nothing, and **never overwrites a
  modified file**.
- `git lfs ls-files` marks state in-band: `*` = full object, `-` = pointer; `--json` is the
  stable contract and `--debug` is explicitly unstable.

**What to copy:** the dialect (gitignore), the precedence (committed file, overridden by local
config), the state marker (a single character in a listing, plus JSON), and the *"never overwrite
a modified file"* rule.

**What to refuse to copy:** `lfs.fetchexclude` doubling as an eviction rule (§6.2).

### 3.2 git-annex is the closest prior art to the whole product intent

`[SOURCE]` r1 §3, r3 §1.

- `wanted` (preferred content) is advisory and drives `--auto` get/drop; `required` is enforced
  and forbids dropping. Two separate expressions, one advisory and one a hard floor.
- `numcopies`/`mincopies` make `drop` refuse unless it can **verify** N copies elsewhere;
  `mincopies` exists precisely because `numcopies` is not concurrency-safe.
- The key name itself carries backend, size (`-sNNNN`) and optionally mtime, so
  `git annex examinekey` answers size with zero network.
- Locked mode is a dangling symlink; unlocked mode is a pointer file. `ls -l` shows the
  placeholder in both cases.

**What to copy:** the advisory/enforced split (a pattern file says what *may* be virtual; a
separate pin says what must *never* be released), and verification-before-drop.

### 3.3 Partial clone and sparse-checkout are the wrong layer

`[SOURCE]` r1 §4. LFS objects are not git blobs, so no `--filter` describes them; `git lfs prune`
is documented-broken under partial clone (git-lfs#4335, open); and partial clone *"requires that
the user be online"* — there is no offline-degraded mode. Sparse-checkout removes paths from the
worktree while leaving objects; it is orthogonal, and keeper's own docs already record that it
does **not** reduce LFS traffic `[REPO]` `docs/sync.md:1072-1073`.

One artefact of partial clone is worth imitating: `git rev-list --missing=print-info` emits
NUL-delimited `<oid> missing=yes path=… type=…` records — the closest git-native listing format
for "absent but known". `[SOURCE]` r1 §4.

### 3.4 Materialization should be copy-on-write where the filesystem allows it

`git lfs dedup` re-creates worktree files as CoW clones of the store object and *"exits
unsuccessfully"* where the filesystem cannot; `-t/--test` probes support. `[SOURCE]` r1 §2. With
CoW, materialize is O(1) and release is a plain unlink leaving the store copy intact. The
counter-example is also cited: git-lfs#6312 (open) — block-cloning objects **before they are
flushed** produced zero-filled worktree files on Windows ReFS. `[SOURCE]` r1 §3. So: fsync the
store object before cloning it, or do not clone.

---

## 4. OS-level placeholders: what is actually possible

### 4.1 macOS — the constraint is decisive and it is topological

`[SOURCE]` r2 §macOS.

- `NSFileProviderReplicatedExtension` (macOS 11+) is exactly the desired model: dataless items,
  `fetchContents` on a read that *pauses the syscall*, `evictItem` to release.
- **But the domain's storage is exposed under `~/Library/CloudStorage/<Provider>-<Domain>`.**
  `NSFileProviderDomain` offers a `volumeURL`/`volumeUUID` and a
  `pathRelativeToDocumentStorage` *"relative to the file provider's shared container"* — an
  app-group container, not an arbitrary user path. `[INFERENCE]` from those two facts plus the
  absence of any documented arbitrary-path API: **a File Provider domain cannot present virtual
  files inside a user's own git worktree.**
- `SF_DATALESS` cannot be set by hand: `chflags(2)` states the flag is internal and *"may not be
  set or unset from user space"*, with undefined behaviour if attempted. So there is no way to
  mark keeper's own files dataless outside the File Provider machinery.
- Kexts are policy-dead (*"Kexts are no longer recommended for macOS"*, plus Reduced Security
  and a reboot on Apple silicon).
- FSKit (macOS **15.4**+, entitlement `com.apple.developer.fskit.fsmodule`) is user-space and
  App-Store-compatible, but it yields a **mounted volume** — the same topological problem as
  FUSE, plus Swift app-extension packaging, and *"the current version of FSKit supports only
  `FSUnaryFileSystem`"*.
- macFUSE is **licence-disqualifying**: clause 4 of its LICENSE forbids binary redistribution
  *"bundled with commercial software … including the automated download or installation"*
  without written permission, and the kext is closed-source. `fuse-t` is *"free for personal
  use"* with a paid commercial licence. Neither passes a permissive-only firewall.
- The licence-free macOS path is an **NFS loopback mount**, proven twice: rclone's
  `--nfs-cache-handle-limit` mount mode and EdenFS (*"On macOS, EdenFS uses NFSv3"*).
- `[UNVERIFIED]` whether shipping a File Provider extension strictly requires paid Apple
  Developer Program membership. Treat "paid team + Developer ID + notarization" as the working
  assumption; do not cite it as sourced.

**Also a live hazard for developer tooling:** FileProvider-backed paths are reported to deadlock
or fail for tools that do not use `NSFileCoordinator` (`anthropics/claude-code#40783`,
2026-03-30). A design in which `git` walks a File-Provider-virtualized worktree inherits that bug
class. `[SOURCE]` r2.

### 4.2 Linux — one in-place option, and it is young

`[SOURCE]` r2 §Linux.

- **FUSE** is the boring option: `fuser` is MIT, libfuse is optional on Linux, and **FUSE
  passthrough** (kernel 6.9+, `FUSE_DEV_IOC_BACKING_OPEN` → `FOPEN_PASSTHROUGH`) is exactly the
  right primitive for "once materialized, get out of the way" — hand the kernel the fd of the
  store object and stop paying per-read round trips. Passthrough currently requires
  `CAP_SYS_ADMIN` on the daemon. Governance caveat worth recording: `fuser` announced
  *"pull requests are no longer being accepted"*.
- **`fanotify` pre-content events** (`FAN_PRE_ACCESS`/`FAN_PRE_MODIFY`, kernel **6.14** floor,
  `FAN_CLASS_PRE_CONTENT`) are the *only* mechanism that can decorate arbitrary in-place paths,
  and Meta runs it in production for HSM. Current, sourced limitations: the page-fault hook was
  merged and **backed out**, so an `mmap()` materializes the whole file at map time; directory
  pre-content events *"also suffer from deadlock problems"*; filesystem freeze can deadlock; and
  events fire on **every** read even after the range is populated because the planned BPF
  suppression *"has not yet been implemented"*. Privilege is `CAP_SYS_ADMIN`-class.
- Mechanical consequences of a mount, regardless of implementation: `ls -l` shows whatever
  `getattr` reports (so the true size is presentable), and `du`, `rsync`, an indexer or a backup
  agent will read every file it touches and thus hydrate everything (§8).

### 4.3 Windows

Cloud Files API (`cldflt.sys`, reparse points, NTFS-only, ~1 KB per placeholder, three states
placeholder/full/pinned-full, platform-drawn hydration icons that *"replace legacy icon overlay
Shell extensions"*) and ProjFS. Out of scope for keeper today; recorded so the eventual shape is
known. `[SOURCE]` r4 §5, r2 §Windows.

### 4.4 Summary table

| Technology | Platform | in `ls`/Finder at true size | on-access hydrate | eviction API | privilege / signing | licence | verdict |
| --- | --- | --- | --- | --- | --- | --- | --- |
| File Provider | macOS 11+ | yes, but only under `~/Library/CloudStorage` | yes (syscall paused) | `evictItem`, `contentPolicy` | app extension, entitlements, `[UNVERIFIED]` paid team | Apple SDK | **cannot** virtualize a worktree path → rejected |
| FSKit | macOS 15.4+ | yes, as a mount | yes | your own | `com.apple.developer.fskit.fsmodule` | Apple SDK | mount-shaped; later, if ever |
| macFUSE | macOS | yes, as a mount | yes | your own | kext approval + reboot | **fails firewall** | rejected |
| fuse-t | macOS | yes, as a mount | yes | your own | none (NFS) | **paid commercial** | rejected |
| NFS loopback | macOS/Linux | yes, as a mount | yes | your own | none | n/a | the licence-free macOS escape hatch |
| FUSE (`fuser`) | Linux | yes, as a mount | yes | your own | none (passthrough: `CAP_SYS_ADMIN`) | MIT | **the one viable bonus** |
| fanotify HSM | Linux ≥ 6.14 | yes, **in place** | yes | your own | `CAP_SYS_ADMIN` | GPL kernel API | powerful, immature, privileged |
| Cloud Files API | Windows | yes, in place | yes | platform | sync-root registration | MS SDK | out of scope |
| **LFS pointer file** | all | **no** (shows ~130 B) | no (explicit verb) | unlink + rename | **none** | n/a | **the design** |

---

## 5. Metadata for content that is not local

### 5.1 The carrier must be the file itself, plus a local ledger

`[SOURCE]` r4 §Verdict, §1, §2.

- **xattrs are not durable.** `rsync` needs `-X` and as a non-root user copies only `user.*`;
  `cp` needs `--preserve=xattr`; GNU `tar` needs explicit opt-in. A stub whose identity lives
  only in an xattr becomes an anonymous file after one `rsync` or one unzip. → xattrs may
  *decorate*, never *identify*.
- **The in-file placeholder is the dominant prior art and the only carrier that survives
  arbitrary copying**: LFS pointers, annex pointer files, `.dvc` sidecars, macOS `.icloud`
  plists.
- **The LFS pointer already carries two of the four wanted fields, mandatorily:** `oid sha256:…`
  and exact `size`. The spec requires parsers to *"preserve keys that they don't know or care
  about"*, so a keeper-namespaced key is spec-legal — but the pointer is *"unique: there is
  exactly one valid encoding"*, so **any added key changes the blob OID**. Budget is hard: under
  1024 bytes including extensions. The `ext-{order}-{name}` mechanism carries *hashes*, is
  labelled experimental, and is the wrong vehicle for descriptive metadata.
- **Mutable state must not go in the pointer.** It is a git blob; mutating it dirties the tree
  and rewrites history's content. git-annex makes exactly this split (immutable facts in the
  key, mutable location facts in the `git-annex` branch) and Joey Hess's own reasoning against
  recording last-verified timestamps is the strongest available argument: it would *"bloat the
  git-annex branch"* and *"displaying a very old timestamp for data at rest"* misleads.
- **Content-addressed metadata cannot key per-path eviction state.** annex metadata attaches to
  the key, so *"all files with the same key share the same metadata"*. Two identical 6 GB videos
  at two paths are one oid — per-path state must therefore be keyed `(profile, path)`, not by
  oid.

### 5.2 So where does each wanted field come from?

| Field | Source | Cost | Grade |
| --- | --- | --- | --- |
| exists / is virtual | index blob is a pointer + worktree bytes are that pointer | index read | `[REPO]` `stage.rs:792-798` |
| true size | pointer `size`, via `stage::indexed_size` | index read, no content | `[REPO]` `stage.rs:800-820` |
| oid | pointer `oid`, via `stage::indexed_pointer` | index read | `[REPO]` `stage.rs:792-798` |
| "where it really is" | remote batch `download` probe (per-object 404 = "I cannot serve this") | one round trip per few hundred objects | `[REPO]` `lfs/audit.rs:29-31` |
| last materialized / last used | keeper's own ledger row | one row | `[REPO]` `db.rs:142-146` |
| date, author, provenance | `git log` for the path + keeper's commit trailers | one revwalk | `[REPO]` `docs/sync.md:595-609` |

**Nothing new needs inventing.** Every field is already answerable from the index, the pointer,
the existing ledger, or the existing remote audit.

### 5.3 The listing surface to imitate

Two conventions, both cited: a human view and a **stable JSON** view, split deliberately
(`git lfs ls-files --json` is the contract, `--debug` is explicitly unstable) `[SOURCE]` r4 §1;
and `git annex whereis`, which *"does not contact remotes … it only reports on the last
information that was received"* and has a `--format` with named variables `[SOURCE]` r4 §1.
Offline-by-default, with a network probe as an explicit flag.

---

## 6. Releasing content safely — the part that loses data

### 6.1 Positive proof, never inference

`[SOURCE]` r3 §1.

- git-annex refuses to drop unless it can verify the content is elsewhere; the CLI literally
  prints `(checking origin...)` and fails with *"Could only verify the existence of 0 out of 1
  necessary copies"*. `--force` bypasses it — and so do `--all`, `--branch`, `--unused` and
  `--key`, each carrying the identical warning. **A nightly job is exactly the caller that
  reaches for `--all`.**
- `git lfs prune --verify-remote` calls the remote to confirm presence; `--when-unverified=halt`
  is the default; and *"if origin doesn't exist then by default nothing will be pruned because
  everything is treated as 'unpushed'"* — **fail-closed is the shipped default**.
- `dvc gc` refuses to run at all without an explicit scope flag, *"to avoid accidentally
  deleting data"*, and offers `--not-in-remote` — the direct analogue of "evict only what the
  remote provably has".

### 6.2 Every cited data-loss bug is a reachability-enumeration bug

`[SOURCE]` r3 §1, r1 §3.

| Incident | What was wrong | Lesson |
| --- | --- | --- |
| git-lfs#5636 | prune deleted objects for **staged** files; *"`--verify-remote` does not prevent this"* | remote proof is necessary, not sufficient — local reachability must be enumerated too |
| git-lfs#4206/#4209 | objects referenced only by **stashes** | enumerate every local reference class |
| **git-lfs#3092** | `lfs.fetchexclude` began outranking "referenced by the current checkout", so an excluded path's object was pruned | **the pattern file must never be an eviction authorization** |
| git-lfs prune, documented | *"The reflog is not considered … objects only referenced by orphaned commits are always deleted"* | a documented sharp edge, not a bug — decide it consciously |
| git-annex `--all`/`--key`/`--unused` | bypass numcopies without `--force` | batch paths need the *same* proof as interactive ones |
| DVC shared cache | `dvc gc` in one project breaks links in another | a shared object store forbids unilateral GC (git-lfs warns identically about `lfs.storage`) |
| git-lfs#6312 | CoW clone before flush ⇒ zero-filled worktree files (ReFS) | fsync before cloning |

**#3092 is the single most important finding in this whole research pass**, because the ask is
literally *"a gitignore-like file that says which files are not downloaded"* — the same shape as
`lfs.fetchexclude`. The pattern file must express **hydration policy only**. Deletion is
authorized per object, at the moment of deletion, by proof.

### 6.3 Dirty ⇒ non-evictable, and git's stat cache cannot answer it

`[SOURCE]` r3 §2, §4. Apple's File Provider fails eviction with `unsyncedEdits` /
`NSFileWriteNoPermissionError`; Lustre HSM cannot `hsm_release` a `DIRTY` file. And git's own
stat cache is unsound for this question: `racy-git` documents that *"the cached stat information
… still exactly match what you would see in the filesystem, even though the file `foo` is now
different"*. keeper's tree carries the same scar `[REPO]` `stage.rs:924-975`,
`git/repo.rs:1930-1947`.

### 6.4 In-use ⇒ non-evictable, as a kernel fact

`[SOURCE]` r3 §2. Apple returns `EBUSY` when open fds exist; rclone documents *"open files
cannot be evicted from the cache"*; cachefilesd skips objects *"if the kernel module says it is
still using them"*. On Linux the race-free primitives are `F_SETLEASE` and `fanotify`; **`lsof`
polling is TOCTOU by construction**. keeper already knows this in a different accent: it has
*no* tier-3 open-writer veto on macOS because `lsof` is too slow and `libproc` needs root
`[REPO]` `docs/sync.md:155-158`.

### 6.5 The only safe publish and the only safe unpublish is `rename(2)`

`[SOURCE]` r3 §2, §5. `rename()` atomically replaces the destination and *"open file descriptors
for oldpath are also unaffected"* — readers keep the full bytes and nobody observes a half file.
Truncating a file some process has `mmap`ed delivers **SIGBUS**. On Windows `DeleteFile` fails
outright with open handles absent `FILE_SHARE_DELETE`.

keeper's materializer already does exactly this: temp sibling `.keeper.<name>.tmp` + rename,
with the staging name carrying a prefix tier 0 already excludes so the watcher cannot mistake it
for user content `[REPO]` `stage.rs:1113-1141`. **Dehydration must reuse that discipline
verbatim.**

### 6.6 `atime` is not "last used"

`[SOURCE]` r3 §3. Linux has defaulted to `relatime` since 2.6.30 (atime updated only if older
than mtime/ctime or >24 h stale); `noatime`/`lazytime` are common. **Any TTL keyed on atime
systematically mis-retains.** Maintain an owned timestamp, written at materialize and at every
observed use.

Precedents for the policy knobs: `rclone --vfs-cache-max-age` / `--vfs-cache-max-size`;
cachefilesd's `0 <= bstop < bcull < brun < 100` watermarks; and — closest to the ask —
`lfs.pruneoffsetdays` (default 3), added to the fetch-recent windows *"to ensure that we always
keep files you download for a few days"*, with the note that a zero value disables that
retention condition entirely. `[SOURCE]` r3 §3, r1 §3. Note what that precedent is **not**: it
is calendar/ref-based. **No git-native tool tracks last *use*** — that ledger is keeper's to
own, and it is the reason the 24 h ask cannot be satisfied by any existing tool.

---

## 7. Pattern-file design

`[SOURCE]` r4 §3, §4.

- **Adopt `.gitignore` semantics verbatim.** They are specified (`gitignore(5)`: precedence,
  negation, `**`, directory rules, order-dependence), universally understood, and already the
  dialect git-lfs chose for `fetchinclude`/`fetchexclude`.
- **Rust crates, all permissive — the licence firewall does not decide between them:** `ignore`
  (`Unlicense OR MIT`) if a whole-tree walker with per-directory stacking is wanted;
  `gix-glob` / `gix-pathspec` / `gix-attributes` (`MIT OR Apache-2.0`) for git-identical
  semantics inside git's own model. keeper already resolves `gix-attributes` and `gix-quote` in
  this crate's tree `[REPO]` `keeper-sync/Cargo.toml:43-47`, and already compiles a
  gitignore-dialect `GlobSet` for `lfs_never` `[REPO]` `stage.rs:124-152`.
- **Do not invent a boolean expression language.** git-annex's
  `largerthan=100kb and not (include=*.c or include=*.h)` is the fully general design and its
  own docs warn that squeezing it into `.gitattributes` requires whitespace-free parenthesised
  mangling and is *"not recommended"*. Ship two layers instead: gitignore patterns for *which
  paths*, plus a few scalar thresholds — the shape LFS (`migrate --above`) and rclone
  (`--min-size`) actually use.
- **Every policy term must be answerable from the pointer, never from the bytes.** annex
  documents that `mimetype=` *"only matches when the content of the file is present in the local
  repository"* — which, in a virtual-files system, is the exception rather than the rule. "Over
  100 MB" is decidable from the pointer's `size`; "MIME type is video/\*" is not, once the file
  is a stub. This is a hard design constraint, not a nicety.
- **Precedence to copy:** `.lfsconfig` (committed, repo-scoped) is overridden by local git
  config. So: the committed pattern file states the repository's intent; the profile's own
  configuration overrides it for this machine. A machine that must keep everything can say so
  without editing a file everyone shares.

---

## 8. The threat that decides the default: accidental mass hydration

`[SOURCE]` r3 §6.

- Microsoft documents that disabling Files On-Demand *"may trigger large-scale hydration and
  unexpected data consumption"*.
- Nextcloud shipped an **infinite implicit-hydration loop** (#6111) and a self-triggered
  hydration deadlock (#7747).
- Lustre HSM ships a mode for exactly this: `NBR` — *"Non Blocking Restore. No automatic restore
  is triggered. Access to a released file returns ENODATA."*
- A `grep -r`, Spotlight/`mds`, a backup agent, an antivirus scanner, `rsync`, a Finder Quick
  Look or a `du` will walk a tree and hydrate everything a virtualized filesystem exposes.

keeper already lives with the macOS version of this hazard and already chose the conservative
side: it checks `SF_DATALESS` *before opening any file* and skips placeholders, because
*"without this check, syncing a folder under iCloud Drive would drag your entire cloud library
onto the disk"* `[REPO]` `docs/sync.md:163-168`, and `copy.rs` refuses to copy such a file with
that exact reason `[REPO]` `copy.rs:823-830`.

**Therefore: materialization is explicit-command-only.** Transparent-on-read is not a v1
simplification, it is the feature's biggest liability, and every system that shipped it also
shipped an allowlist, a budget, or a non-blocking mode to contain it.

---

## 9. What keeper already has

`[REPO]`, from `context/g1-lfs-machinery.md` and `context/g2-surfaces.md`.

| Primitive | Where | State |
| --- | --- | --- |
| pointer format, store, batch API, `basic` transport, ssh auth, filesystem-remote transport | `keeper-sync/src/lfs/{pointer,store,batch,basic,ssh,local}.rs` | ships |
| "do not download" switch | `LfsMode::PointerOnly` `profile/mod.rs:81-89`, applied `engine.rs:5025`, `5037` | ships, but **per profile**, not per path |
| per-subtree variant | `MediaPolicy::{Materialize,PointerOnly}` `profile/mod.rs:364-389` | ships — and is the precedent that a per-subtree answer is a *separate enum*, not a reuse |
| gitignore-dialect glob list on the profile | `lfs_never` + `LfsPolicy` `stage.rs:116-166` (refuse-on-typo `stage.rs:138-145`) | ships — the exact shape the new selector needs |
| atomic hydrate | `lfs::stage::materialize` `stage.rs:1118-1143` — store-presence precondition, `.keeper.*.tmp` + rename | ships |
| the single "download this now" decision point | `Engine::materialize_pending` `engine.rs:4998-5069` | ships |
| **per-path materialization ledger with timestamps** | `materialized (profile_id, path, at_ms)` `db.rs:142-146`, `remember_materialized` `db.rs:324-338`, `materialized_paths` `db.rs:341-352` | ships — this is the 24 h TTL's substrate |
| journal units + idempotent re-request | `WorkKind::LfsDownload` `db.rs:614-617`, `enqueue_unique` `db.rs:739-744`, `covered_while_running` `db.rs:635-651` | ships |
| remote-holds-it proof | `lfs::audit` `audit.rs:29-31` (per-object 404 on a `download` batch), `keeper-syncd verify --remote` | ships |
| safe release of the **redundant** copy | `lfs::prune` — three conditions `prune.rs:28-45` | ships |
| index-stat repair after content changes under a pointer entry | `refresh_index_stat` / `repair_index_stat` `engine.rs:5064`, `git/repo.rs:1880-1947` | ships |
| pending vocabulary for "not here yet" | `PendingReason::Incoming { size_bytes, replacing }` `engine.rs:238-256`; `browse::EntrySyncStatus` `browse.rs:157-198` | ships |
| the one recurring maintenance hook | the success edge `mark_synced → prune_lfs_store` `engine.rs:3052-3066`; supervisor `TICK_MS = 1000` `engine.rs:338` | ships — and there is **no `.timer` unit in the repository** (`keeper-syncd.service:33` is `ExecStart=…keeper-syncd watch`), while `docs/sync.md:889-891` refuses timer-driven *self-update* for a daemon that "can be mid-push at any moment" — so a "nightly" pass must ride this edge |
| OS placeholder *reading* | `stability::is_dataless` `stability.rs:164-189`, refusal `copy.rs:823-830` | ships |
| OS placeholder *producing* | — | **absent; searched, zero matches** |
| the word "virtual"/"hydrate"/"dehydrate" anywhere in `keeper-sync` | — | **absent** |

### 9.1 The one primitive that is missing, and why it is not a relaxation of `prune`

`prune`'s condition 2 is *"the worktree still holds the real content"* — a path whose worktree
content is pointer text is **never** a candidate `[REPO]` `prune.rs:28-33`. That is precisely
inverted by dehydration: after a release the **store object is the only local copy**. So
dehydration must not be built by relaxing that predicate; it needs its own condition set, whose
first member is the remote-holds-it proof (§6.1). `[REPO]` `context/g1-lfs-machinery.md` §8.4.

### 9.2 Two existing behaviours will produce mass false reports

- `Engine::verify` reports a path as bad when the worktree holds a pointer whose object the store
  lacks `[REPO]` `engine.rs:5637-5645`. Under a virtual-files policy that is the *normal* state
  → mass false positives unless `verify` is taught which paths are intentionally virtual.
- `browse` reports `size_bytes` straight off `fs::metadata().len()` `[REPO]`
  `browse.rs:613-626`, so a virtual file renders as ~130 bytes, while the honest answer is
  already written and unused (`stage::indexed_size`, `stage.rs:800-820`).

---

## 10. The recommended design

1. **Representation.** The virtual state *is* the committed LFS pointer in the worktree —
   byte-for-byte, never re-rendered (§2.1, §5.1). No sparse files, no xattr identity, no added
   pointer keys.
2. **Policy.** A committed, root-level pattern file in gitignore dialect, overridden by the
   profile's own configuration, plus a scalar size floor. Compiled once per run into the
   existing `LfsPolicy` shape with the same refuse-on-typo discipline (§7, §9).
3. **The policy authorizes hydration decisions only.** It is never an eviction authorization
   (§6.2, git-lfs#3092).
4. **Metadata.** Answered from the index + pointer + ledger + `git log`, never from the worktree
   stat; a stable JSON listing plus a human view; remote presence only on an explicit flag
   (§5.2, §5.3).
5. **Hydrate** is an explicit verb — CLI and IPC — that reuses the journal (`LfsDownload`,
   `enqueue_unique`, `covered_while_running` for idempotence) and the existing
   `stage::materialize` publish (§3.1's *"never overwrite a modified file"* rule included).
6. **Dehydrate** is a new verb beside `materialize`, using the identical temp+rename discipline,
   and refusing on **five** conditions: modified/dirty, open by any process, the object is not
   provably on the remote, the path is pinned, or the worktree already holds pointer text
   (§6.1–§6.5). Followed by the existing index-stat refresh so the path still reads clean.
7. **Release is lazy and rides the success edge**, not a timer: keeper's own `last_used_ms`
   (never atime), a TTL (default 24 h), a per-pass byte/count budget, and a pin that is a hard
   floor (§6.6, §7's advisory/enforced split, §9's success-edge row).
8. **No on-read hydration** (§8). `git checkout` of a virtual path yields pointer text, exactly
   as today.
9. **OS-level presentation** is a separate, opt-in, Linux-first FUSE **read-only mirror** — not
   a virtualization of the worktree itself — with the `du`/`rsync`/indexer hazard documented up
   front (§4.2, §8). macOS gets nothing here, for the structural reason in §4.1.

---

## 11. Options considered and rejected

### 11.1 A fourth `LfsMode` variant
Rejected. `LfsMode` is profile-wide and is compared in nine places and projected in four
`[REPO]` `context/g1-lfs-machinery.md` §8.1; the ask is per-path. `MediaPolicy` is the tree's own
precedent that a differently-scoped answer is a **new** type, not a new variant of the old one
`[REPO]` `profile/mod.rs:364-368`.

### 11.2 Reusing `lfs_never` for the pattern list
Rejected. `lfs_never` means *"never route this through LFS at all"* — nearly the opposite of
*"route it through LFS and keep it virtual"*. Sharing the name or the plumbing is a trap
`[REPO]` `context/g2-surfaces.md` §8.1.

### 11.3 macOS File Provider
Rejected outright, not deferred: it cannot decorate an arbitrary worktree path (§4.1), the
dataless flag is unsettable from user space, and FileProvider-backed paths have a live
deadlock report for tools that skip `NSFileCoordinator`.

### 11.4 macFUSE / fuse-t
Rejected on licence (§4.1). `cargo deny` would reject macFUSE; fuse-t requires a paid commercial
licence.

### 11.5 `fanotify` pre-content HSM on Linux
Not now. It is the only in-place mechanism and it is genuinely attractive for a server, but:
kernel ≥ 6.14 floor, `CAP_SYS_ADMIN`, `mmap` materializes whole files because the page-fault
hook was backed out, directory events deadlock, freeze deadlocks, and every read re-fires the
event because the BPF suppression is unimplemented (§4.2). Revisit when the BPF suppression and
the page-fault hook land.

### 11.6 On-read hydration through the LFS smudge filter
Rejected. It would require advertising the `delay` capability keeper deliberately leaves
unadvertised, plus a second state machine, with the DW-206 launch-failure hazard attached
`[REPO]` `lfs/filter.rs:294-298`, `729-736` — and it is the mass-hydration DoS surface of §8.

### 11.7 Partial clone / sparse-checkout
Wrong layer (§3.3), and keeper's docs already say sparse checkout does not reduce LFS traffic.

---

## 12. Open questions

1. **Pin syntax.** `required`-style (a second pattern list) or per-path (a ledger flag set by a
   command)? git-annex has both; the ledger flag is cheaper and does not need a committed file,
   but it does not travel between machines.
2. **Should hydration be reference-counted?** An "open in an app, then release" flow wants a
   lease, not a timestamp. A lease is strictly better and strictly more machinery; the timestamp
   plus `F_SETLEASE`-style in-use refusal may be enough.
3. **What does a dehydrate do when the remote is unreachable?** Fail closed, per §6.1's
   git-lfs precedent — but a metered/offline server that *wants* space back has a real need. A
   `--offline-trust-store` escape hatch has the shape of git-annex's `trusted`, which is
   documented to *"risk numcopies later being violated"*.
4. **CoW materialization** (§3.4) is a pure win on APFS/btrfs/XFS-reflink and would make
   hydrate O(1) and release a plain unlink. It needs a probe and the fsync-before-clone rule
   (git-lfs#6312). Worth its own story, not worth blocking on.
5. **`[UNVERIFIED]`** whether shipping a macOS File Provider extension requires the paid Apple
   Developer Program. Matters only if §11.3 is ever reopened; interacts with D-1.

---

## 13. Sources

Full citation lists — publisher, URL, publication and access date — live in the four digests
under `_bmad-output/planning-artifacts/research/virtual-files-2026-08-22/digests/`. Counts as
delivered: r1 ≈ 54 sources (git-lfs, git, git-annex, DVC, XetHub primary docs and source at
named refs), r2 ≈ 48 (Apple developer docs, TN3150, `chflags(2)`, kernel docs, LWN, macFUSE and
fuse-t licences, rclone, Sapling), r3 ≈ 32 including 7 documented incidents, r4 ≈ 40 (git-lfs
spec, gitignore(5)/gitattributes(5), crate licences, Microsoft cfapi).

Repository grounding, with `path:line` for every claim, lives beside them in `context/`.
