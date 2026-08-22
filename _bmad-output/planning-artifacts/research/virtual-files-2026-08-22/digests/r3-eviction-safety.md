# Safety and Correctness Engineering of On-Demand Materialization + Timed Eviction
_agent: R3Eviction · accessed: 2026-08-22_

## Verdict (decision-grade)

- **Never delete the last local copy on inference.** Both battle-tested designs demand *positive* proof: git-annex refuses to drop unless it can verify N other repositories hold the key (`numcopies`/`mincopies`), and `git lfs prune --verify-remote` calls the remote batch API to confirm presence before deletion. Everything else — "it's old", "it's in a commit", "we pushed that branch" — is inference, and every cited data-loss bug is an inference that was wrong.
- **Reachability is the hard part, not the deletion.** All five documented git-lfs/DVC/annex incidents are *reachability enumeration* bugs: staged-but-uncommitted objects (git-lfs #5636), stash-only objects (#4206), `fetchexclude` overriding "referenced by current checkout" (#3092), `--force`/`--all`/`--key`/`--unused` silently bypassing numcopies (git-annex), shared-cache breakage (`dvc gc`). A pattern-file-driven design ("these paths are virtualizable") is *exactly* the shape of git-lfs `lfs.fetchexclude`, which is the mechanism that caused #3092. Treat the pattern file as a hydration *policy*, never as an eviction *authorization*.
- **Dirty ⇒ non-evictable is an absolute invariant, and it must be enforced by the layer that owns the bytes.** Apple's File Provider fails eviction with `unsyncedEdits`/`NSFileWriteNoPermissionError`; Lustre HSM cannot `hsm_release` a `DIRTY` file. Detecting dirty via git's stat cache alone is unsound because of *racily clean* entries — same-second mtime with unchanged size reads as clean.
- **In-use ⇒ non-evictable, and "in use" must be a kernel-observed fact, not an `lsof` snapshot.** Apple returns `EBUSY` when open FDs exist; rclone documents that "open files cannot be evicted from the cache"; cachefilesd skips objects "if the kernel module says it is still using them". On Linux the only race-free primitives are `F_SETLEASE` (kernel blocks the conflicting `open`/`truncate` and signals the holder) and `fanotify`; `lsof` polling is TOCTOU by construction.
- **Truncate-in-place is forbidden; `.part` + hash-verify + `rename(2)` is the only safe publish, and the same rename is the only safe eviction.** `rename()` atomically replaces the destination and leaves *open file descriptors for the old inode unaffected* — readers keep the full bytes; nobody sees a half-file. Truncating a materialized file that some process has `mmap`ed delivers **SIGBUS** to that process. On Windows, `DeleteFile` fails outright when other handles are open without `FILE_SHARE_DELETE`.
- **`atime` is not "last used".** Linux has defaulted to `relatime` since 2.6.30 (atime updated only if older than mtime/ctime, or >24h stale), and `noatime`/`lazytime` are common. Any TTL keyed on atime will systematically over-retain or under-retain. Maintain your own last-use timestamp, written at materialize/open time.
- **Materialize-on-read is a denial-of-service surface.** A `grep -r`, Spotlight/`mds`, a backup agent, an antivirus scanner or `rsync` walks the whole tree and hydrates everything. Microsoft documents that disabling Files On-Demand "may trigger large-scale hydration and unexpected data consumption"; Nextcloud shipped an infinite implicit-hydration loop (#6111) and a self-triggered hydration deadlock (#7747). **The safe default for a server-side `keeper-syncd`-shaped design is explicit-command-only materialization**, with transparent-on-read as an opt-in that carries an allowlist and a rate/volume budget.

---

## Findings

### 1. When is dropping content safe? — the proof protocols

#### git-annex: verified copies, not assumed copies

- git-annex refuses to drop unless it can verify safety, and the verification is a live remote probe — the CLI literally prints `(checking origin...)` per file and fails with `Could only verify the existence of 0 out of 1 necessary copies` — source: Canonical/Ubuntu manpages, `git-annex-drop(1)`, git-annex 8.20210223-2ubuntu2, https://manpages.ubuntu.com/manpages/jammy/man1/git-annex-drop.1.html, published unknown (package-dated 2021).
- "git-annex will refuse to drop content if it cannot verify it is safe to do so. Usually this involves verifying that the content is stored in some other repository." — source: same manpage, as above.
- `--force` "bypasses safety checks, and forces git-annex to delete the content of the specified files, **even from the last repository that is storing their content**. Data loss can result from using this option." — source: same manpage.
- **The trap:** four *non-`--force*` options also bypass the policy. `--all`, `--branch=ref`, `--unused` and `--key=keyname` each carry the identical warning: "Note that this bypasses checking the .gitattributes annex.numcopies setting and required content settings." — source: same manpage. A batch/nightly job is precisely the caller that reaches for `--all`/`--key`.
- `mincopies` exists because `numcopies` is not concurrency-safe: "This is like numcopies, but is enforced even more strictly. While numcopies can be violated in concurrent drop situations involving special remotes that do not support locking, mincopies cannot be." — source: Hackage, git-annex 8.20210127 changelog, https://hackage.haskell.org/package/git-annex-8.20210127/changelog, published 2021-01-27.
- Trust is a *hole* in the proof: "trusted repositories are assumed to continue to contain content, so checking them is skipped… dropping content from trusted repositories does risk numcopies and mincopies later being violated." — source: git-annex documentation, `copies`, https://git-annex.branchable.com/copies/, published unknown.
- The independent-probe primitive is `git annex checkpresentkey`, which "verifies if the specified key's content is present in the specified remote" — source: Canonical/Ubuntu manpages, `git-annex-checkpresentkey(1)`, https://manpages.ubuntu.com/manpages/focal/man1/git-annex-checkpresentkey.1.html.

#### git-lfs prune: reachability + pushed-ness, optionally remote-verified

- Prune deletes any local object not referenced by: the current checkout, all existing stashes, a recent branch, a recent commit, an unpushed commit, or any other worktree checkout — source: git-lfs, `docs/man/git-lfs-prune.adoc`, https://github.com/git-lfs/git-lfs/blob/main/docs/man/git-lfs-prune.adoc @ `main`, accessed 2026-08-22.
- The last-copy rule: "When the only copy of an LFS file is local, and it is still reachable from any reference, that file can never be pruned, regardless of how old it is." Pushed-ness is derived from local-vs-remote ref differences, relying on the pre-push hook invariant — source: same file, §UNPUSHED LFS FILES.
- `--verify-remote` (`-c`) "calls the remote to ensure that any reachable LFS files to be deleted have copies on the remote before actually deleting them"; can be made default via `lfs.pruneverifyremotealways`; `--when-unverified=halt` (the default) stops on the first unverifiable object — source: same file, §VERIFY REMOTE and §OPTIONS.
- **Design tell:** "if origin doesn't exist then by default nothing will be pruned because everything is treated as 'unpushed'." Fail-closed is the shipped default — source: same file, §DEFAULT REMOTE.
- **Design tell 2:** "The reflog is not considered, only commits. Therefore LFS objects that are only referenced by orphaned commits are always deleted." — source: same file, §DESCRIPTION. Deliberate, documented, and still a sharp edge.

#### DVC gc

- `dvc gc` refuses to run at all without an explicit scope flag: "To avoid accidentally deleting data, `dvc gc` doesn't do anything unless one or a combination of scope options are provided" — source: Iterative, DVC docs, `dvc gc`, https://dvc.org/doc/command-reference/gc, accessed 2026-08-22.
- Recoverability is conditional and stated: "any files collected from the cache can be restored using `dvc fetch`, **as long as they have been previously uploaded** with `dvc push`" — source: same page.
- `--not-in-remote` is the direct analogue of "evict only what the remote provably has" — source: same page.
- **Documented surprise:** shared caches. "If a cache is shared among different projects that track some of the same files, using `dvc gc` in one project will break those overlapping data links in the other projects." The mitigation (`--projects`) requires the operator to have already fetched all relevant branches elsewhere — source: same page. This is the same hazard git-lfs warns about: "you should not run `git lfs prune` if you have different repositories sharing the same custom storage directory" (`lfs.storage`) — source: git-lfs-prune.adoc, as above.
- `--cloud` deletion "is irreversible unless there is another DVC remote or a manual backup with the same data" — source: same DVC page.

#### restic forget/prune

- Two-phase by design: `forget` removes snapshots (references); `prune` removes only data that is *then* unreferenced — source: restic docs, `Removing backup snapshots`, https://restic.readthedocs.io/en/stable/060_forget.html (source `doc/060_forget.rst` @ master), accessed 2026-08-22.
- Safety features worth copying: `--dry-run` on `forget`; policy is applied *per group* (host+paths) — "This is a safety feature to prevent accidental removal of unrelated backup sets"; `forget` errors out if a tag policy would empty an entire group; and "It is advisable to run `restic check` after pruning" — source: same page.

### 2. Atomically replacing a materialized file with a placeholder

#### POSIX rename is the primitive

- "If newpath already exists, it will be atomically replaced, so that there is no point at which another process attempting to access newpath will find it missing." — source: Linux man-pages 6.18, `rename(2)`, https://man7.org/linux/man-pages/man2/rename.2.html, dated 2026-05-24 (tarball man-pages-6.18).
- "Open file descriptors for oldpath are also unaffected." A reader that already opened the materialized file keeps reading the full content from the old inode even after the placeholder is renamed over the path — source: same page.
- "If newpath exists but the operation fails for some reason, rename() guarantees to leave an instance of newpath in place." No torn intermediate state — source: same page.
- `renameat2(RENAME_EXCHANGE)` atomically swaps two paths, and `RENAME_NOREPLACE` fails if the target exists (ext4 ≥3.15, btrfs/tmpfs ≥3.17, xfs ≥4.0, most others ≥4.9) — source: same page. `RENAME_NOREPLACE` is the clean way to lose a materialization race without clobbering.
- `rename()` fails with **EXDEV** across mount points, "even if the same filesystem is mounted on both". The object store and the worktree must be on one filesystem or the rename plan collapses into copy+delete — source: same page.

#### Truncate/mmap = SIGBUS

- `mmap(2)` documents **SIGBUS**: "Attempted access to a page of the buffer that lies beyond the end of the mapped file." — source: Linux man-pages 6.18, `mmap(2)`, https://man7.org/linux/man-pages/man2/mmap.2.html, dated 2026-05-24. Shrinking a mapped file under a reader crashes the reader. Rename never does this.
- Relatedly, `fcntl(2)` returns **EAGAIN** when "the operation is prohibited because the file has been memory-mapped by another process" — source: Linux man-pages 6.18, `fcntl(2)`, https://man7.org/linux/man-pages/man2/fcntl.2.html.

#### Windows

- "The **DeleteFile** function fails if an application attempts to delete a file that has other handles open for normal I/O or as a memory-mapped file (**FILE_SHARE_DELETE** must have been specified when other handles were opened)." — source: Microsoft, `DeleteFileW function (fileapi.h)`, https://learn.microsoft.com/en-us/windows/win32/api/fileapi/nf-fileapi-deletefilew, ms.date 2023-12-15.
- "**DeleteFile** marks a file for deletion on close. Therefore, the file deletion does not occur until the last handle to the file is closed. Subsequent calls to CreateFile to open the file fail with **ERROR_ACCESS_DENIED**." — source: same page. On Windows the eviction can *appear* to succeed and leave the path unopenable; POSIX-delete semantics differ again (`ERROR_FILE_NOT_FOUND`).

#### How HSM/cloud systems decide "in use"

- **macOS File Provider** — `evictItem(identifier:completionHandler:)` "turns a materialized item into a dataless item". It fails with: `unsyncedEdits` "if the item had nonuploaded changes"; `nonEvictable` if the user pinned it; **`EBUSY` if the item has open file descriptors on it**; `EMLINK` if too many hardlinks. "If the item has local changes, it fails with an NSFileWriteNoPermissionError." — source: Apple, `NSFileProviderManager.evictItem(identifier:completionHandler:)`, https://developer.apple.com/documentation/fileprovider/nsfileprovidermanager/evictitem(identifier:completionhandler:), © 2026 Apple.
- The system-level invariant is stated explicitly: "To avoid possible data loss, the system won't convert a materialized item into a dataless item if the item has pending changes that the File Provider extension needs to sync." — source: Apple, `Synchronizing the File Provider Extension`, https://developer.apple.com/documentation/fileprovider/synchronizing-the-file-provider-extension, © 2026 Apple.
- Directory eviction is *not* all-or-nothing and can partially apply: "If the system encounters a nonevictable child, eviction stops immediately… The system may have evicted other materialized items, based on the traversal order." — source: Apple evictItem docs, as above.
- **Lustre HSM** — release is only legal against an archived, non-dirty file. `DIRTY` means "This file has been modified since a copy of it was made in the HSM solution. DIRTY files should be archived again." `NORELEASE` pins a file ("This file will never be released"), and cannot be set on an already-`RELEASED` file — source: Lustre Operations Manual, ch. *Hierarchical Storage Management (HSM)* (Lustre ≥2.5), mirror https://github.com/VatslavDS/lustre-manual/blob/master/03.15-Hierarchical%20Storage%20Management%20(HSM).md, accessed 2026-08-22.
- Lustre's restore is *transparent and blocking*: "Released files are automatically restored when a process tries to read or modify them. The corresponding I/O will block waiting for the file to be restored." — source: same. Note the escape hatch for exactly the DoS problem: `hsm.policy=+NBR` (Non Blocking Restore) — "No automatic restore is triggered. Access to a released file returns **ENODATA**." — source: same. That is the "explicit-command-only" mode, shipped as a supported policy.
- **cachefilesd** — "Objects will be skipped if their atimes have changed **or if the kernel module says it is still using them**." — source: Canonical/Ubuntu manpages, `cachefilesd.conf(5)`, cachefilesd 0.10.10, https://manpages.ubuntu.com/manpages/noble/man5/cachefilesd.conf.5.html.
- **Linux leases** are the race-free "someone wants this file" signal: `F_SETLEASE` notifies the holder "when a process (the 'lease breaker') tries to open(2) or truncate(2) the file"; the kernel **blocks** the breaker's syscall meanwhile, and forcibly breaks the lease after `/proc/sys/fs/lease-break-time`. Leases attach to the open file description, apply only to regular files, and need `CAP_LEASE` for files you don't own — source: Linux man-pages 6.18, `F_SETLEASE(2const)`, https://man7.org/linux/man-pages/man2/F_SETLEASE.2const.html, dated 2026-01-14.

### 3. LRU / TTL eviction policy design

- **rclone VFS** is the closest shipped analogue of a nightly TTL evictor. `--vfs-cache-max-age` (default 1h) "will evict files from the cache after the set time since last access has passed… When a cached file is accessed the 1 hour timer is reset to 0". `--vfs-cache-max-size` and `--vfs-cache-min-free-space` add capacity pressure, checked every `--vfs-cache-poll-interval` (default 1m). Under pressure "rclone will attempt to evict the least accessed files from the cache first… start with files that haven't been accessed for the longest" — source: rclone, `rclone mount`, https://rclone.org/commands/rclone_mount/, accessed 2026-08-22.
- rclone documents that quotas are *soft* for two named reasons: polling granularity, **and "open files cannot be evicted from the cache"** — source: same page. Budget for over-quota; do not treat the cap as a hard bound.
- rclone also documents a durability/ordering rule directly relevant here: "files are written back to the remote only when they are closed and if they haven't been accessed for `--vfs-write-back` seconds. If rclone is quit or dies with files that haven't been uploaded, these will be uploaded next time rclone is run" — source: same page. Upload state must be crash-durable, not in-memory.
- rclone forbids cache sharing: "You should not run two copies of rclone using the same VFS cache with the same or overlapping remotes… This can potentially cause data corruption" — source: same page. Same class as the `lfs.storage` and `dvc gc --projects` warnings.
- **cachefilesd** uses hysteresis to avoid thrash: three watermarks per axis, `0 <= bstop < bcull < brun < 100` (defaults 1%/5%/7%), for blocks and file counts. Culling *starts* below `bcull`, *stops* once above `brun`, and allocation is refused below `bstop`. "The userspace daemon scans the cache to build up a table of cullable objects. These are then culled in least recently used order." `nocull` disables it entirely — source: `cachefilesd.conf(5)`, as above. **A single threshold thrashes; a start/stop pair does not.**
- **Nix** models pinning as explicit, filesystem-visible GC roots: `nix-store --gc` deletes "all paths in the Nix store not reachable via file system references from a set of 'roots'"; `--add-root` creates a symlink registered under `/nix/var/nix/gcroots/auto/`, and "when `/home/eelco/bla/result` is removed, the GC root in the auto directory becomes a dangling symlink and will be ignored by the collector". `--print-roots`/`--print-live`/`--print-dead` make the decision auditable before deleting; `--max-freed` bounds the work — source: Nix 2.35.2 Reference Manual, `nix-store --gc`, https://nix.dev/manual/nix/latest/command-ref/nix-store/gc, accessed 2026-08-22. Copy this: **roots are data, self-expiring, and inspectable.**
- **atime is unreliable as "last used".** `relatime`: "Access time is only updated if the previous access time was earlier than or equal to the current modify or change time… Since Linux 2.6.30, the kernel defaults to the behavior provided by this option (unless noatime was specified), and the strictatime option is required to obtain traditional semantics. In addition, since Linux 2.6.30, the file's last access time is always updated if it is more than 1 day old." `noatime` suppresses updates entirely; `lazytime` only updates the in-memory inode — source: `mount(8)`, https://man7.org/linux/man-pages/man8/mount.8.html, §FILESYSTEM-INDEPENDENT MOUNT OPTIONS.
- Note the irony: cachefilesd's culling *is* "based on the access time of data objects" — source: `cachefilesd.conf(5)`, as above — which is exactly why it also cross-checks with the kernel and skips objects whose atimes moved mid-scan.
- **Apple's model of "what to keep"** is a declared *working set* ("recently used items, tagged items, favorites, shared items, recently deleted items"), not a pure LRU. "It's important… to keep materialized copies of any items the user is likely to access frequently or when they're offline." — source: Apple, `Synchronizing the File Provider Extension`, as above.

### 4. Modification detection: never evict dirty content

- **Lustre**: `DIRTY` blocks release; after an archive completes the coordinator re-checks the dirty bit and, if set, dispatches another archive rather than marking archived — source: Lustre HSM chapter, as above; corroborated by Lustre HSM design deck (Rutman, SC09), https://wiki.lustre.org/images/1/12/SC09-HSM-Code.pdf.
- **Apple**: eviction fails with `unsyncedEdits` / `NSFileWriteNoPermissionError` on locally-changed items; the system will not dataless-ify an item with pending sync changes — source: Apple evictItem + Synchronizing docs, as above.
- **git-lfs**: an object referenced only by an unpushed commit "can never be pruned, regardless of how old it is" — source: git-lfs-prune.adoc, as above. But see #5636/#4206 below: *staged* and *stashed* content is not covered by that rule.
- **git's stat cache is not a sound dirty oracle.** Git compares `st_mode` (type + exec bit), `st_mtime`, `st_ctime`, `st_uid`, `st_gid`, `st_ino`, `st_size`; `st_atime` is explicitly excluded as useless; `st_dev` is only compared under `USE_STDEV` because "this member is not stable on network filesystems"; nanosecond fields only under `USE_NSEC`, disabled by default on Linux — source: git, `Documentation/technical/racy-git.adoc` @ `master`, https://raw.githubusercontent.com/git/git/master/Documentation/technical/racy-git.adoc, accessed 2026-08-22.
- The **racily clean** hazard, verbatim: modify → `update-index` → "modify 'foo' again, in-place, without changing its size". "If the modification that follows it happens very fast so that the file's `st_mtime` timestamp does not change… the cached stat information the index entry records still exactly match what you would see in the filesystem, even though the file foo is now different. This way, Git can incorrectly think files in the working tree are unmodified even though they actually are." — source: same file.
- Git's two mitigations: (a) when cached stat says unmodified **and** `st_mtime >= the index file's own mtime`, git re-reads and compares content; (b) when writing an index containing racily clean entries, "cached `st_size` information is truncated to zero before writing" so the entry can never falsely match again — source: same file. **A materializer that stamps its own timestamps into a sidecar DB inherits exactly this race and must adopt the same defence: content-hash verification whenever the recorded stat is within the ambiguity window.**
- The document also records that nanosecond timestamps are *broken* on several filesystems (CEPH, CIFS, NTFS, UDF) in current kernels — source: same file, citing https://lore.kernel.org/lkml/5577240D.7020309@gmail.com/. Do not lean on sub-second mtime for dirty detection.

### 5. Concurrency, resumption, crash, verification, durability

- **Two materializers, one object.** `renameat2` with `RENAME_NOREPLACE` "can therefore be used as a way to atomically (with respect to other threads) attempt to map an address range: one thread will succeed; all others will report failure" — the same idiom applies to publishing a path; support: ext4 ≥3.15 … most ≥4.9 — source: `rename(2)`, as above. Where unavailable, both writers should write distinct `.part` files and let plain `rename()` win last-writer-wins on byte-identical, hash-verified content (content addressing makes the race harmless).
- **Concurrent drop is the dangerous direction, not concurrent fetch.** git-annex needed a *stricter* second counter (`mincopies`) precisely because "numcopies can be violated in concurrent drop situations involving special remotes that do not support locking" — source: git-annex 8.20210127 changelog, as above. Eviction must take an exclusive per-object lock; materialization can be optimistic.
- **Verify before publishing.** git-annex exposes `checkpresentkey` for remote-side proof; content-addressed stores let you re-derive the OID from the downloaded bytes. Publishing unverified bytes into the worktree converts a network fault into a silent corruption, because from that moment the file *looks* materialized.
- **Durability.** `fsync()` is not sufficient on macOS: "while fsync() will flush all data from the host to the drive… the drive itself may not physically write the data to the platters for quite some time and it may be written in an out-of-order sequence… **This is not a theoretical edge case.** This scenario is easily reproduced with real world workloads and drive power failures. For applications that require tighter guarantees… Mac OS X provides the **F_FULLFSYNC** fcntl." — source: Apple, `fsync(2)` man page (Documentation Archive), https://developer.apple.com/library/archive/documentation/System/Conceptual/ManPages_iPhoneOS/man2/fsync.2.html, © 2018 Apple.
- Consequence for the `.part` + rename pattern: `fsync` the `.part` file **before** the rename, and `fsync` the containing **directory** after it — otherwise a crash can leave the rename durable and the data not, i.e. a zero/garbage file occupying a path that the system believes is materialized. On APFS the `.part` fsync should be `F_FULLFSYNC` when the content is the last copy.
- **Partial downloads.** rclone's `--vfs-cache-mode full` keeps partially-downloaded objects as **sparse files** and tracks which byte ranges it holds; it warns "not all file systems support sparse files. In particular FAT/exFAT do not. Rclone will perform very badly… and it will log an ERROR message if one is detected." — source: rclone mount docs, as above. Resumable materialization needs either range tracking or restart-from-zero; a partially written file must never be reachable under its final name.

### 6. Threat model: read amplification and accidental full-tree materialization

- **The mechanism is documented by Apple, and it is not hypothetical:** "the system can enumerate and materialize the content of your working set in the background, **so that Spotlight can index it**." — source: Apple, `Synchronizing the File Provider Extension`, as above. An OS indexer is a first-class hydration client by design.
- **Microsoft documents the blast radius of a policy flip:** "Turning Files On-Demand off may trigger large-scale hydration and unexpected data consumption, so align settings with data caps and shared-PC constraints." — source: NinjaOne, *How to Enable or Disable OneDrive Files On-Demand*, https://www.ninjaone.com/blog/how-to-enable-or-disable-onedrive-files-on-demand-in-windows-11/, published 2026-03-14.
- **Backup tools force full hydration as the recommended workaround.** A user reports their backup program erroring on OneDrive placeholders "because the file isn't really a file, just a database marker"; the accepted remedy is to right-click the OneDrive root and "always keep files on this device" so "your backup program will then see real files instead of cloud placeholders" — source: Microsoft Q&A, *OneDrive vs my Backup program*, https://learn.microsoft.com/en-us/answers/questions/5813310/onedrive-vs-my-backup-program-how-to-fix-this-mess. This is the whole design defeated by one `rsync`-shaped consumer.
- **Antivirus/filter drivers break placeholder semantics:** "Files On-Demand might not be compatible with some third-party antivirus solutions." — source: Microsoft Support, *Save disk space with OneDrive Files On-Demand for Windows*, https://support.microsoft.com/en-us/onedrive/save-disk-space-with-onedrive-files-on-demand-for-windows.
- **Self-inflicted hydration storms are real.** Nextcloud desktop #6111: on a server data-fingerprint change the client enters restore mode, tries to re-upload a virtual file, "That will trigger implicit hydration of it during attempt to upload it. That file being missing on the server prevents hydration. Then the client is going to be infinitely be stuck in a continuous loop" — source: nextcloud/desktop issue #6111, https://github.com/nextcloud/desktop/issues/6111, opened 2023-10-02, labelled `confirmed`.
- Nextcloud desktop #7747 logs `implicit hydration triggered by the client itself. Will lead to a deadlock.` — the sync daemon hydrating through its own VFS layer — source: nextcloud/desktop issue #7747, https://github.com/nextcloud/desktop/issues/7747, opened 2025-01-10.
- **The shipped defences, in order of strength:** (1) Lustre's `hsm.policy=+NBR` — no automatic restore at all, released files return `ENODATA` on access (source: Lustre HSM chapter, as above); (2) Apple's per-item pinning (`nonEvictable`) plus a bounded working set; (3) rclone's `--vfs-read-chunk-size` (default 128M) so a header read does not pull a whole object (source: rclone mount docs); (4) cachefilesd's `bstop` — refuse further allocation entirely below the floor (source: `cachefilesd.conf(5)`).

---

## Documented bugs / incidents (the load-bearing five-plus)

1. **git-lfs #5636 — prune deletes *staged* objects.** "Staged LFS files are deleted by `git lfs prune`. If I create a commit after that, the commit is broken and cannot be pushed. Even worse, if I have multiple local commits, **I have permanently lost data**. Note, `--verify-remote` does not prevent this." Repro is four commands: `git add .` then `git lfs prune`. git-lfs 3.4.0 — source: https://github.com/git-lfs/git-lfs/issues/5636, opened 2024-01-30. **`--verify-remote` protects the remote-presence claim, not the reachability enumeration.**
2. **git-lfs #4206 — prune deletes objects referenced only by stashes.** "running 'git lfs prune' on objects which are only referenced in stashed changes will result in those objects being deleted, and therefore the stashed changes can never be re-applied." git-lfs 2.11.0 — source: https://github.com/git-lfs/git-lfs/issues/4206, opened 2020-08-06.
3. **git-lfs #3092 — an exclusion pattern overrode "referenced by the current checkout".** Adding `fetchexclude = *` to `.lfsconfig` made prune delete *all* LFS files, including checked-out ones. Maintainer confirms this was an intentional behaviour change (commit `d2221dc`): "`git lfs prune` now does delete anything that matches `lfs.fetchexclude=*`" — source: https://github.com/git-lfs/git-lfs/issues/3092, opened 2018-06-27. **The single most transferable warning for a gitignore-style virtualization pattern file.**
4. **git-annex forum — `drop --force` in a shell one-liner destroyed a movie library.** "I'm nearly 100% certain I did a `drop --force` by mistake. I'm pulling the files from backups." The maintainer's reply: "you may have lost the content… there's no way to get it back unless you have another copy somewhere." The trigger was `m=OneMovie.m4v git annex drop --force $m` — the env-var prefix meant `$m` was empty, so `--force` applied to the whole directory — source: https://git-annex.branchable.com/forum/help__44___a_bunch_of_files_disappeared/, dated 2017-11-12/14. **An unscoped destructive command with an empty argument list must be a refusal, not a wildcard.**
5. **git-annex `move` silently used force semantics.** Maintainer: "the move command bypasses the normal numcopies check, in a way that is otherwise (AFAICR) only ever done when using `--force`… move is equivalent to `git annex copy && git annex drop --force`, rather than `git annex copy && git annex drop`." — source: https://git-annex.branchable.com/forum/git-annex_move_does_not_appear_to_respect_numcopies/. **A convenience command inherited unsafe semantics from its implementation.**
6. **Nextcloud desktop #6111** (infinite implicit-hydration loop, `confirmed`) and **#7747** (self-triggered hydration deadlock) — sources as cited above.
7. **DVC shared-cache gc** — "using `dvc gc` in one project will break those overlapping data links in the other projects" — source: DVC gc docs, as above; the same hazard is called out for `lfs.storage` in git-lfs-prune.adoc.

---

## Applicability to a git-LFS-backed virtual-file design

- A repo-local, content-addressed LFS object store plus a pointer format is *already* the substrate every one of these systems reinvents. The pointer file is a first-class placeholder: it carries oid + size, so `ls`, metadata display, and "where it really is" need no new format. Materialization = write `.part` → verify OID → `rename()` over the pointer. Eviction = write pointer to `.part` → `rename()` back. Both directions are the same atomic primitive, which is a strong simplicity argument.
- The eviction proof should be the **union** of git-lfs's and git-annex's: object present in the local store *and* provably present on the remote (`checkpresentkey`-equivalent: an LFS batch `download` probe), *and* not referenced by anything unpushed/staged/stashed, *and* not dirty, *and* not open, *and* not pinned.
- Because a git worktree materialization means the worktree file differs from the pointer that git's index records, the design must decide up front whether git sees pointers or content — and whatever it chooses, the racy-git ambiguity window means dirty detection cannot be stat-only.
- Server-side `keeper-syncd` is the *easier* environment: no Finder, no Spotlight, no antivirus filter driver, and explicit-command materialization is natural. The desktop app is where the read-amplification threat model bites, and where the OS-native placeholder APIs (File Provider on macOS, CfAPI on Windows) both already enforce dirty/open refusal on your behalf — which is a reason to use them rather than a bespoke FUSE layer.
- Do **not** build eviction on `atime`, and do **not** make the pattern file authorize deletion.

## Risks / failure modes

- **Pattern file as deletion authority** → git-lfs #3092 replayed: a broadened glob evicts checked-out, unpushed or in-flight content.
- **Nightly batch job reaches for the bulk flag** → git-annex `--all`/`--key`/`--unused` bypass numcopies without `--force` appearing anywhere in the command.
- **Stat-only dirty check** → racily-clean entries (same-second, same-size edit) are read as clean; the edit is evicted.
- **`lsof`-style in-use polling** → TOCTOU; the file is opened between the check and the rename. (Rename itself is safe for readers; the danger is the *decision*, plus writers.)
- **Truncate-in-place instead of rename** → SIGBUS for mmap readers; torn reads for everyone else.
- **Cross-filesystem store** → `EXDEV`, silently degrading the atomic rename into copy+delete.
- **fsync ordering inverted** → crash leaves a durable rename over non-durable data; on APFS plain `fsync` does not even guarantee platter ordering (`F_FULLFSYNC` required).
- **Shared object store between clones/daemons** → the `lfs.storage` / `dvc gc --projects` / rclone shared-cache hazard; one instance's eviction is another's data loss.
- **Transparent-on-read + any tree walker** (`grep -r`, `du`, backup, AV, indexer, Quick Look) → full-tree hydration; possibly an infinite loop if the daemon's own uploader reads through the VFS (Nextcloud #7747).
- **Single-threshold capacity policy** → thrash; cachefilesd needs `bstop/bcull/brun`, rclone needs a poll interval and accepts soft overshoot.
- **Windows delete-with-open-handle** → apparent success, path left unopenable (`ERROR_ACCESS_DENIED`) until the last handle closes.

## Open questions

- Does the design present pointers or content to `git status`? That choice determines whether "dirty" is a git question or a sidecar-DB question, and both have the racy-git ambiguity window.
- Is the remote-presence probe per-eviction (correct, chatty) or cached with a TTL (fast, weaker)? git-lfs makes it opt-in for exactly this cost reason; git-annex makes it default.
- Should eviction be refused, or merely deferred, when a lease/`EBUSY` signal fires? Apple returns an error and moves on; deferring is friendlier but needs a starvation bound.
- On the server, is `F_SETLEASE` viable given `CAP_LEASE` and the "regular files only, filesystem must support leases (`EINVAL` otherwise)" constraints?
- What is the pin (GC-root) representation? Nix's dangling-symlink-auto-expiry is elegant and inspectable; an in-DB flag is easier but invisible to the user.
- Should the nightly evictor have a `--max-freed`-style work bound and a mandatory dry-run/`--print-dead` equivalent before its first production run?

---

## Rules a safe design must obey (each traced to a cited failure)

1. **Never delete the last verified local copy without a positive remote-presence proof for that exact object.** — prevents: git-annex `drop --force` library loss (forum, 2017); git-lfs "only copy is local" is exactly why prune hard-refuses.
2. **Enumerate reachability to include the index (staged), stashes, all worktrees, and unpushed commits — and fail closed if any of these cannot be enumerated.** — prevents: git-lfs #5636 (staged), #4206 (stashes); mirrors git-lfs's "if origin doesn't exist… nothing will be pruned".
3. **The virtualization pattern file authorizes *non-hydration*, never *deletion*.** — prevents: git-lfs #3092, where `fetchexclude` silently outranked "referenced by the current checkout".
4. **No bulk/convenience path may bypass the safety predicate.** Every eviction, including the nightly job's, runs the identical check. — prevents: git-annex `--all`/`--branch`/`--unused`/`--key` numcopies bypass; git-annex `move` implicitly meaning `drop --force`.
5. **An empty or unresolvable target set is a refusal, not a wildcard.** — prevents: the `m=... git annex drop --force $m` incident.
6. **Dirty ⇒ never evictable, where "dirty" is content-verified whenever the recorded stat falls inside the racy window (mtime ≥ metadata-write time, or sub-second-resolution filesystem).** — prevents: racy-git false-clean (git `racy-git.adoc`); mirrors Lustre `DIRTY` blocking `hsm_release` and Apple `unsyncedEdits`.
7. **Unuploaded ⇒ never evictable, and upload state must survive a crash.** — prevents: rclone's documented "quit or dies with files that haven't been uploaded" case; Apple's "won't convert a materialized item into a dataless item if the item has pending changes".
8. **Open ⇒ never evictable, decided by a kernel-mediated signal (lease/`EBUSY`/cache-module in-use), never by a polled snapshot.** — prevents: TOCTOU eviction under an active reader; mirrors Apple `EBUSY`, rclone "open files cannot be evicted", cachefilesd "the kernel module says it is still using them".
9. **Replace only via `rename(2)` (or `renameat2`), never truncate-in-place; keep store and worktree on one filesystem.** — prevents: mmap SIGBUS (`mmap(2)`); torn reads; `EXDEV` degradation (`rename(2)`).
10. **Publish only hash-verified bytes: download to `.part`, verify the OID, `fsync` the file (`F_FULLFSYNC` on APFS), `rename`, `fsync` the directory.** — prevents: silent corruption from a truncated transfer; durable-name/non-durable-data crash windows (Apple `fsync(2)`).
11. **Take an exclusive per-object lock for eviction; allow optimistic concurrency for materialization.** — prevents: the concurrent-drop numcopies violation that forced git-annex to add `mincopies`.
12. **Default to explicit-command materialization; make transparent-on-read opt-in, path-allowlisted, and volume-budgeted.** — prevents: Spotlight/indexer background materialization (Apple, documented behaviour), backup/AV full hydration (Microsoft Q&A), "large-scale hydration and unexpected data consumption" on a policy flip.
13. **The daemon must never hydrate through its own virtual layer.** — prevents: Nextcloud #7747 self-triggered hydration deadlock and #6111's infinite upload/hydrate loop.
14. **Never key retention on `atime`; maintain an explicit last-use timestamp.** — prevents: silent misbehaviour under `relatime` (kernel default since 2.6.30), `noatime`, `lazytime` (`mount(8)`).
15. **Capacity policy needs hysteresis (start-culling / stop-culling / refuse-allocation), not one threshold; expect soft overshoot.** — prevents: thrash; mirrors cachefilesd `bcull/brun/bstop` and rclone's documented soft quotas.
16. **Pins (GC roots) must be explicit, user-visible, self-expiring, and dumpable before any deletion (`--print-dead`/`--dry-run`).** — prevents: unexpected eviction of user-critical files; mirrors Nix `--add-root`/`--print-roots` and Apple `nonEvictable`.
17. **One object store must be owned by exactly one evictor.** — prevents: DVC shared-cache link breakage, git-lfs `lfs.storage` sharing hazard, rclone "potentially cause data corruption" with a shared VFS cache.
18. **Directory-level eviction must be resumable and must report partial application.** — prevents: Apple's documented "may have evicted other materialized items, based on the traversal order" surprise.
19. **On Windows, treat a delete/replace with an open handle as a hard failure, not a retry-until-it-works.** — prevents: `DeleteFile` marking for deletion-on-close and leaving the path returning `ERROR_ACCESS_DENIED`.
20. **Every eviction is journalled with enough information to identify what was dropped and where the remaining copy is.** — prevents: the "how do I even tell what I lost" phase of the git-annex incident, where `git annex log`'s `-` lines were the only forensic trail.

## Sources

1. git-annex, `git-annex-drop(1)` — https://manpages.ubuntu.com/manpages/jammy/man1/git-annex-drop.1.html (Canonical; git-annex 8.20210223-2ubuntu2)
2. git-annex, `copies` — https://git-annex.branchable.com/copies/
3. git-annex, `git-annex-checkpresentkey(1)` — https://manpages.ubuntu.com/manpages/focal/man1/git-annex-checkpresentkey.1.html
4. git-annex 8.20210127 changelog (mincopies) — https://hackage.haskell.org/package/git-annex-8.20210127/changelog (2021-01-27)
5. git-annex forum, "help, a bunch of files disappeared" — https://git-annex.branchable.com/forum/help__44___a_bunch_of_files_disappeared/ (2017-11-12)
6. git-annex forum, "git-annex move does not appear to respect numcopies" — https://git-annex.branchable.com/forum/git-annex_move_does_not_appear_to_respect_numcopies/
7. git-lfs, `docs/man/git-lfs-prune.adoc` @ main — https://github.com/git-lfs/git-lfs/blob/main/docs/man/git-lfs-prune.adoc
8. git-lfs issue #5636, "Git LFS prune deletes staged objects" — https://github.com/git-lfs/git-lfs/issues/5636 (2024-01-30)
9. git-lfs issue #4206, "prune should not delete objects referenced by stashes" — https://github.com/git-lfs/git-lfs/issues/4206 (2020-08-06)
10. git-lfs issue #3092, "Prune deletes files referenced in the current checkout" — https://github.com/git-lfs/git-lfs/issues/3092 (2018-06-27)
11. Iterative, DVC `gc` command reference — https://dvc.org/doc/command-reference/gc
12. restic, "Removing backup snapshots" — https://restic.readthedocs.io/en/stable/060_forget.html
13. git, `Documentation/technical/racy-git.adoc` @ master — https://raw.githubusercontent.com/git/git/master/Documentation/technical/racy-git.adoc
14. Linux man-pages 6.18, `rename(2)` — https://man7.org/linux/man-pages/man2/rename.2.html
15. Linux man-pages 6.18, `mmap(2)` — https://man7.org/linux/man-pages/man2/mmap.2.html
16. Linux man-pages 6.18, `fcntl(2)` — https://man7.org/linux/man-pages/man2/fcntl.2.html
17. Linux man-pages 6.18, `F_SETLEASE(2const)` / `F_GETLEASE` — https://man7.org/linux/man-pages/man2/F_SETLEASE.2const.html
18. Linux man-pages, `mount(8)` (relatime/noatime/lazytime) — https://man7.org/linux/man-pages/man8/mount.8.html
19. Canonical, `cachefilesd.conf(5)` (brun/bcull/bstop, culling) — https://manpages.ubuntu.com/manpages/noble/man5/cachefilesd.conf.5.html
20. rclone, `rclone mount` (VFS file caching, max-age/max-size, chunked reading) — https://rclone.org/commands/rclone_mount/
21. Nix 2.35.2 Reference Manual, `nix-store --gc` (roots, --add-root, --print-dead) — https://nix.dev/manual/nix/latest/command-ref/nix-store/gc
22. Apple, `NSFileProviderManager.evictItem(identifier:completionHandler:)` — https://developer.apple.com/documentation/fileprovider/nsfileprovidermanager/evictitem(identifier:completionhandler:)
23. Apple, "Synchronizing the File Provider Extension" — https://developer.apple.com/documentation/fileprovider/synchronizing-the-file-provider-extension
24. Apple, `fsync(2)` man page / F_FULLFSYNC — https://developer.apple.com/library/archive/documentation/System/Conceptual/ManPages_iPhoneOS/man2/fsync.2.html
25. Lustre Operations Manual, "Hierarchical Storage Management (HSM)" (release/DIRTY/NORELEASE/NBR) — https://github.com/VatslavDS/lustre-manual/blob/master/03.15-Hierarchical%20Storage%20Management%20(HSM).md
26. Lustre HSM design deck (Rutman, SC09) — https://wiki.lustre.org/images/1/12/SC09-HSM-Code.pdf
27. Microsoft, `DeleteFileW function (fileapi.h)` — https://learn.microsoft.com/en-us/windows/win32/api/fileapi/nf-fileapi-deletefilew (2023-12-15)
28. Microsoft Support, "Save disk space with OneDrive Files On-Demand for Windows" — https://support.microsoft.com/en-us/onedrive/save-disk-space-with-onedrive-files-on-demand-for-windows
29. Microsoft Q&A, "OneDrive vs my Backup program - how to fix this mess" — https://learn.microsoft.com/en-us/answers/questions/5813310/onedrive-vs-my-backup-program-how-to-fix-this-mess
30. NinjaOne, "How to Enable or Disable OneDrive Files On-Demand" (large-scale hydration) — https://www.ninjaone.com/blog/how-to-enable-or-disable-onedrive-files-on-demand-in-windows-11/ (2026-03-14)
31. nextcloud/desktop issue #6111, "Restore mode on data fingerprint changes try to hydrate files missing from server" — https://github.com/nextcloud/desktop/issues/6111 (2023-10-02)
32. nextcloud/desktop issue #7747, "implicit hydration triggered by the client itself" — https://github.com/nextcloud/desktop/issues/7747 (2025-01-10)
