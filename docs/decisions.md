# Project decisions

Durable, project-level decisions — the ones that shape what keeper will and won't build,
recorded so they stay discoverable instead of living only in planning artifacts. Each entry
cross-references its source (PRD / architecture spine) so a reader can always trace a decision
back to where it was made. Entries are numbered D-1, D-2, … as they are made.

## D-1 — Paid Apple Developer Program: deferred by decision

keeper defers the $99/yr Apple Developer Program. This is a deliberate deferral, not an
oversight and not a purchase — recorded here because the deferral is load-bearing.

- **Unlocks (only the paid program grants these):** APNs push; the Notification Service
  Extension (NSE) for background notification decryption, with its 24 MB memory ceiling and
  App-Group store-layout implications; TestFlight; App Groups; and AltStore PAL notarization for
  EU distribution. (PRD §13.5; architecture spine "Deferred")
- **Opening trigger:** the gate opens only when push becomes a product goal. (PRD §13.5; §13.8,
  the paid-program-timing open question)
- **Constraint it forces:** once push is on the table, keeper's client-only posture makes it a
  PRD-level question — push must ride a homeserver operator's gateway, Beeper's, or a user-run
  Sygnal. It must **never** ride project infrastructure. (NFR-11; PRD §13.5) This is the
  load-bearing reason the gate exists: keeper contacts only user-configured homeservers/bridges,
  Beeper's API when a Beeper account exists, and the signed-update endpoint — running push
  infrastructure would break that invariant.
- **Cheap-now mitigations already paid for:** the single `Platform::data_dir()` root keeps all
  account state under one path, so a future App Group container move (NSE era) is a path change,
  not a data migration. (AD-29; FR-65) Plan B (UniFFI + native SwiftUI shell) stays shelved with
  its revisit triggers recorded — the blank-webview bug class proving unfixable across Tauri
  releases, or NSE work beginning. (PRD §13.8)
- **Status / owner:** deferred. Revisit is owned by the PM/owner, when push demand is real.
  (PRD §13.8, the paid-program-timing open question)

## D-2 — Virtual files are pointer files, not filesystem virtualization

keeper will let a clone hold LFS-tracked content as metadata only, materialize it on request and
release it again — but it will **not** make that content appear at its true size in `ls` or in
Finder. This is a closed question on macOS and a deferred one on Linux, and it is recorded here
because the "why not" will otherwise be re-asked every time somebody sees Dropbox do it.

- **What is built:** the virtual state is the committed git-LFS pointer in the worktree,
  byte-for-byte. Metadata (true size, oid, modification time, provenance, where the bytes really
  are) is answered from the index, the pointer, keeper's own ledger and `git log` — never from the
  worktree stat for the size. Hydrate and dehydrate are explicit verbs; release is lazy, budgeted,
  and rides a successful sync. (AD-122…AD-134; Epic 56)
- **When a locally-authored file may be released:** never until keeper has confirmed that exact
  path reached the remote, and then a TTL (24 h by default) after that confirmation — not after
  last use. Content that merely arrived from the remote and was never modified here keys off last
  use instead. Two clocks, because only one of the two cases can lose data, and that case gets the
  stricter one. (AD-131; Epic 56, story 56.5)
- **Automatic release is the default, not the only path:** release is a keeper *task* with three
  modes — off, manual, scheduled — so "a nightly script" is a supported way to run it rather than
  a workaround. (AD-136; Epic 57, story 57.4)
- **Why not macOS File Provider:** `NSFileProviderReplicatedExtension` has exactly the right
  semantics — dataless items, `fetchContents` on read, `evictItem` — but its storage is exposed
  under `~/Library/CloudStorage/<Provider>` and its container path is relative to an app-group
  container. There is no documented API to virtualize a path the *user* chose, which is the only
  path a synced folder ever has. `SF_DATALESS` *"may not be set or unset from user space"*
  (`chflags(2)`), so keeper cannot mark its own files dataless either. Kexts are policy-dead.
  (research §4.1, §11.3; AD-130)
- **Why not FUSE on macOS:** macFUSE's licence forbids redistributing binaries "bundled with
  commercial software … including the automated download or installation", and its kext is
  closed-source; fuse-t is free for personal use only. Both fail the permissive-only cargo-deny
  firewall that D-1's sibling rules already impose. (research §4.1, §11.4)
- **Why not `fanotify` HSM on Linux yet:** it is the only mechanism that can decorate paths in
  place, and it is immature — kernel ≥ 6.14, `CAP_SYS_ADMIN`, `mmap` materializes whole files
  because the page-fault hook was merged and backed out, directory events and filesystem freeze
  deadlock, and every read re-fires the event because the planned BPF suppression is
  unimplemented. (research §4.2, §11.5)
- **Why no on-read hydration at all:** a `grep -r`, Spotlight, a backup agent or a `du` walks the
  tree and hydrates everything. Microsoft documents "large-scale hydration and unexpected data
  consumption"; Nextcloud shipped an infinite implicit-hydration loop; Lustre HSM ships a mode
  that returns `ENODATA` instead of restoring. keeper already chose this side once, for iCloud
  placeholders (`docs/sync.md` §4). (research §8; AD-128)
- **The cost accepted, stated so it is chosen and not discovered:** `ls -l`, `du` and third-party
  applications see ~130 bytes of pointer text for a virtual file. That representation is not a
  shortcut — it is the only one `git status` tolerates, because the pointer *is* the committed
  blob. (research §2.1; AD-124)
- **Revisit triggers:** an Apple API that virtualizes an arbitrary path; or the `fanotify`
  page-fault hook plus BPF event suppression both landing. A read-only Linux FUSE **mirror**
  mount (never a virtualization of the worktree itself) is deferred with its shape already
  recorded. (AD-130)
- **Status / owner:** decided. Owner is the architect; Epic 56 implements the pointer design.

## D-3 — Scheduled work is keeper's own; scheduled *self-update* is not

keeper will hold named tasks with a schedule, a last run and a last result, runnable on the sync
daemon and on the desktop app and drivable from `cron`. It will **not** let a schedule replace or
restart a keeper binary. Recorded here because the two look like one feature and only one of them
is safe.

- **What is built:** a task record, a schedule validated when it is saved, a due-gate on the tick
  each host already runs, a one-shot CLI verb a `cron` entry or systemd timer can call, and a view
  that states which host will actually run each task. (AD-135…AD-137; Epic 57)
- **Why not a second scheduler:** one clock per host process, because two schedulers over one git
  repository produce concurrent index locks — the rule the notes cadence already follows. (AD-62)
- **Why `update` is excluded:** the daemon holds a durable journal and can be mid-push at any
  moment; swapping its binary unattended is how a routine release becomes a corrupted transfer.
  That refusal predates this decision and survives it. (`docs/sync.md` §13; AD-136)
- **The platform asymmetry, stated so it is not discovered:** on Linux the systemd user service is
  a real background host, and a timer/oneshot pair ships beside it calling the same one-shot verb;
  both need `loginctl enable-linger` to survive logout. That condition is checked rather than
  assumed — `systemctl --user is-enabled` means *wanted at login*, not *survives logout*, so the
  Tasks view stats the file `enable-linger` creates and shows a lingering box *"logged in or not"*
  and a non-lingering one *"its schedule stops when your session ends"*. On **macOS keeper ships no
  background host at all** — the `keeper-syncd` binary *is* built and published for macOS, and its
  one-shot verbs work there, but no launchd plist exists anywhere in the tree, so nothing starts
  `watch` and nothing triggers a task. The desktop app is therefore the only host keeper provides,
  a task runs only while keeper is running, and the UI says so rather than implying a schedule that
  cannot fire. (AD-137; `docs/sync.md` §14)
- **Revisit trigger:** a launchd agent for `keeper-syncd` on macOS. The blocker is not the crate —
  it builds and ships there — it is deciding what a background daemon on macOS should own when the
  app is already a real background host. Until then the asymmetry is visible in the product, not
  hidden.
- **Status / owner:** decided. Owner is the architect; Epic 57 implements it.
