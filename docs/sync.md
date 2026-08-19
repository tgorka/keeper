# Folder Sync

Keeper can keep a local folder in step with a git repository — a Forgejo
instance, GitHub, or anything else speaking the git protocol. It is built on
[gitoxide](https://github.com/GitoxideLabs/gitoxide), stores large files through
git-LFS, and ships both inside the app and as a standalone Linux daemon
(`keeper-syncd`).

The design and its evidence live in
`_bmad-output/planning-artifacts/research-sync-2026-07-25.md` (research) and the
`AD-40 … AD-53` block of the Architecture Spine (decisions). This document is
the operator's view: what it does, what it deliberately does not do, and how to
diagnose it.

---

## 1. Prerequisites

**A `git` binary is required.** This is not a soft dependency.

gitoxide covers clone, fetch, checkout, status, the index, object and commit
creation, `.gitattributes` filtering, progress and cancellation. It does **not**
implement five things keeper needs, and upstream has declined to build the
largest of them (push: issue #306, closed `NOT_PLANNED` on 2026-07-22). Those
five shell out:

| Operation | Why gitoxide cannot |
| --- | --- |
| `git push` | Not implemented; explicitly not planned |
| `git worktree add\|remove\|prune` | gitoxide's worktree API is read-only |
| `git sparse-checkout init\|set\|reapply` | gitoxide never reads `.git/info/sparse-checkout` |
| `git gc` / `repack` | No maintenance module exists |
| `git merge` | No merge, reset, restore or switch workflow exists — and a fetch alone only moves the remote-tracking ref |

Additionally, `file://` and `ssh://` remotes make gitoxide spawn
`git-upload-pack` and `ssh` respectively — so a pendrive-to-server sync over a
local path also needs the binary present.

Requirements:

- **git ≥ 2.42** (for `sparse-checkout --cone` and `ls-files --format`).
- No `git-lfs` installation is needed. Keeper implements the LFS client itself.

If git is missing or too old, `CapabilitiesVm.sync` is false, every sync surface
is **absent rather than broken**, and the app says exactly what to install.
`keeper-syncd doctor` reports the same and exits with code 3.

---

## 2. Concepts

### Adopting a folder you already have

Pointing a profile at an **existing, non-empty** folder is the ordinary case and
works: keeper initializes a repository in place, attaches the remote, commits
what is there, and reconciles it with whatever the remote already holds through
the normal conflict path (§5). Nothing is overwritten and nothing is deleted to
make that happen. An empty folder is cloned into instead.

### Profiles

A **profile** binds one local folder to one repository. It is the unit of
configuration, concurrency, progress, logging and failure — everything keeper
reports names a profile. Profiles run concurrently and fail independently.

| Field | Meaning |
| --- | --- |
| `localPath` | Absolute path to the synced folder |
| `remoteUrl` | Repository URL (https, ssh, or a local path) |
| `branch` | Branch this profile tracks |
| `direction` | `bidirectional`, `pushOnly`, or `pullOnly` |
| `lane` | `main`, or `worktree` for the review lane (see §7) |
| `subpaths` | Repository subpaths to materialize; empty means everything |
| `excludes` | Extra exclusion globs on top of the built-in set |
| `removable` | The folder lives on removable media (see §6) |
| `volumeId` | Which volume it is bound to. Written by keeper, not by you (§6) |
| `lfsMode` | `materialize`, `pointerOnly`, or `disabled` |
| `lfsThresholdBytes` | Files at or above this are tracked through LFS (default 4 MiB) |
| `lfsNever` | Globs that never go through LFS, whatever their size (default none) |
| `lfsPruneLocal` | Release local LFS objects once the remote holds them (default **true**) |
| `settleMs` | Quiescence window (see §4) |
| `tags` | Extra `Keeper-Tag:` provenance trailers |

### State

Persisted in `<data dir>/sync.db` (WAL SQLite, beside `keeper.db` and
`archive.db`): profiles, a durable **work journal**, a file-state cache, and this
installation's device identity.

The journal is what makes the reliability promise true. Every unit of network
work is recorded **before** it is attempted and cleared only once its effect is
durable. A crash between the two costs a repeat, never a loss — and any unit
left mid-flight is returned to the queue at the next start.

---

## 3. What a sync actually does

1. If the profile is on removable media, confirm the volume is attached. If it
   is not, **stop here** — see §6.
2. Fetch from the remote (skipped when offline; the work is queued).
3. Apply what fast-forwards. Where local and remote both changed, resolve
   without asking (§5).
4. Scan the working tree, discard anything excluded, and hold anything that is
   not demonstrably complete (§4).
5. Stage what settled, route oversized files through LFS (§8), and commit with
   provenance trailers (§9).
6. Transfer any LFS objects the commit queued.
7. Push — but **only** once step 6 has nothing outstanding. A commit *this* pass
   made, whose pointers name objects the remote does not have yet, is held back
   rather than published; a commit you made yourself with plain `git` is not
   tracked that way, which §8 spells out.

Local git — staging, committing, reading status — **never** requires the
network. Only fetch, push and LFS transfers do.

---

## 4. Only complete files are synchronized

The hazard: you start a 3 GB download in Chrome into a synced folder, and a
naive syncer commits and uploads the half-written file.

Keeper applies four tiers. **Only the last one is a proof**; the others narrow
the window.

| Tier | What it does | Why it is not sufficient alone |
| --- | --- | --- |
| 0 — name and shape | Excludes known in-flight and lock-file conventions | `curl` and `wget` write their **final** filename from byte 0 |
| 1 — event trigger | Filesystem events; on Linux a close-write shortens the wait | A program can close and reopen a file; macOS has no close event at all |
| 2 — quiescence | Size, mtime, ctime and inode unchanged across a window | A writer that stalls longer than the window looks finished |
| 3 — open-writer veto | Linux only: `/proc/locks`, optionally open file descriptors | Almost nothing takes advisory locks; other users' processes are invisible |
| 4 — verify-on-read | Re-stats the open descriptor before and after, hashes while reading | — this is the proof |

Tier 0 covers, among others: `*.crdownload` (Chrome), `*.part` (Firefox),
`*.partial` (rclone), `~$*` (Office), `.~lock.*#` (LibreOffice), editor swap
files, `.DS_Store` and `._*`, `.fuse_hidden*`, `.nfs*`, and — because Safari's
partial download is a **package directory**, not a file — the whole
`**/*.download/**` subtree.

**Quiescence windows:**

| Situation | Window |
| --- | --- |
| Linux, after a close-write | 1 s |
| Default | 5 s |
| Removable or network media | 10 s |
| Hard ceiling — forced through regardless | 60 s |

A file whose modification time is more than 10 s in the **future** is never
held: a machine with a broken clock would otherwise wedge it forever.

### Two honest gaps

- **macOS has no tier 3.** `lsof` is far too slow to run per candidate,
  `libproc` needs root for anything outside your own user, and the only true
  close-write signal (EndpointSecurity) requires an entitlement granted by
  Apple. macOS therefore relies on tiers 0, 1, 2 and 4 with the 5 s window.
- **Tier 4 is the only guarantee.** If a file changes while being read, the
  transfer is abandoned and re-queued silently. That is a normal event, not an
  error, and it is never surfaced as a failure.

### iCloud placeholders

On macOS, keeper checks `SF_DATALESS` before opening any file and skips
placeholders. Opening a dataless file silently **materializes** it — without
this check, syncing a folder under iCloud Drive would drag your entire cloud
library onto the disk.

---

## 5. Conflicts are resolved without asking

Convergence never waits on a prompt.

When both sides changed a file, the remote revision keeps the canonical path and
your local revision is preserved as:

```
<name>.sync-conflict-<UTC yyyymmdd-hhmmss>-<device>.<ext>
```

Both are committed as ordinary tracked files, so every peer sees the same pair.
This is the Syncthing resolution shape, chosen because it is honest and
reversible where a content merge is neither.

**Modification always beats deletion.** A remote delete never removes a file you
just edited, and a local delete never removes a file someone else just edited.

Every conflict raises a non-blocking warning naming both paths. Nothing stops.

### When keeper does ask

Only for conditions no policy can decide:

- a path the remote or a peer filesystem cannot represent (**you must rename**);
- storage quota exhausted;
- credentials rejected;
- a one-way review lane whose remote branch has diverged (§7) — where a human
  decision is the point, not a failure.

---

## 6. Removable media

The failure this prevents is the expensive one: unplugging a drive and having
that read as *"the user deleted 40 GB"*.

A profile marked `removable` is bound to the **volume**, not the path. Keeper
writes `.keeper-sync/volume.json` at the mount root and matches on its contents,
so re-mounting at a different mountpoint is still the same volume.

The binding is made **once**, the first time keeper sees the media: the marker
is minted if the volume carries none, its id is recorded in the profile as
`volumeId`, and the profile id is recorded in the marker. From then on that id
is what every scan compares against — which is what lets a *different* stick
mounted at the same path be refused (§6, `Foreign`) instead of silently synced
into. An existing marker is joined, never re-minted, so a volume already used by
another profile or another machine keeps its identity.

Two things are refused rather than adopted, both for the same reason — a marker
in the wrong place would make the pause that protects you unreachable:

- a folder that **is not there**. With the drive unplugged its mountpoint is
  gone, and adopting the nearest directory that does exist would mark the
  internal disk. Create the folder on the drive first.
- a folder on the **same volume keeper itself lives on**. A removable volume is
  by definition not the one holding keeper's own state; marking that one would
  bind the profile to the whole machine, after which nothing could ever read as
  detached. The profile says so and stays paused.

When the marker is absent the profile enters **`Paused (media absent)`** — a
normal state, not an error:

- the watcher is torn down;
- the journal is retained intact;
- **no deletion is staged, committed or pushed**;
- the tray and the UI say why.

Re-attach the drive and the profile resumes from the journal. An unmount during
a transfer aborts that operation as retryable rather than truncating it.

Repositories on removable media are opened with full trust **after** the marker
verifies the media is yours. This matters more than it sounds: on a directory
not owned by the current user, gitoxide's default reduced-trust mode **silently
discards repository-local filter configuration**, which would mean the LFS clean
filter never runs and a multi-gigabyte file gets committed raw into git history
with no error at all.

---

## 7. Review lanes — the bot-to-human airlock

A profile with `direction = pushOnly` and `lane = worktree` is designed for
autonomous agents.

Keeper creates a linked worktree on a generated branch `keeper/<profile>/<id>`.
The agent writes only there. Keeper commits with `Keeper-Source: bot` provenance
and pushes **that branch only** — never the base branch, and **never a
force-push**. The handoff to a human is a pull request, whose number and URL are
recorded and surfaced.

If the lane's remote branch has diverged, keeper stops and warns rather than
resolving. That is deliberate: the whole point of a lane is that a human decides.

---

## 8. Large files

**LFS is mandatory above the threshold, and this is not tuning.** gitoxide has
no streaming object read — reading a 3 GB blob means a 3 GB allocation — so
large content must never become a git blob.

Files at or above `lfsThresholdBytes` (default 4 MiB) are tracked automatically:
keeper maintains `.gitattributes` and commits it with provenance. The bytes move
through keeper's own LFS client, streamed and hashed in a single pass, never
buffered.

### Keeper owns `filter.lfs.process` in its own repositories

Keeper's own staging path invokes no filter — it writes the pointer blob and the
worktree file's stat directly. The registration exists for the *other* git in
the folder: the one a human runs by hand.

That registration has to claim `filter.lfs.process`, not only
`filter.lfs.clean`/`smudge`. **git prefers a `process` driver over a
clean/smudge pair regardless of which scope each was defined in**, and
`git lfs install` writes `filter.lfs.process` into `~/.gitconfig`. So on every
machine that has ever had the real git-lfs — every developer's — a repository
-local clean/smudge pair is silently outranked and never runs.

What answers instead is git-lfs, which fetches objects itself and, when it
cannot resolve one, dies mid-protocol. Under `filter.lfs.required=false` git
absorbs that and writes **zero bytes** — for that path and every remaining path
in the same checkout. One object missing from the server is enough to empty an
entire fast-forward's worth of media.

`required` stays `false`, but for a narrower reason than before: keeper's filter
answers a per-path failure with `status=error` and stays up, so git falls back
for one path instead of emptying the rest. What `false` still buys is that a
worktree whose keeper binary has moved remains checkout-able, as pointers,
rather than hard-failing every git command in the folder.

Two guards sit behind it:

- **A file that is empty while its pointer names non-zero bytes is never
  staged.** There is no editing sequence that produces that pair, and committing
  it replaces every peer's only reference to the real object with a reference to
  nothing.
- **`keeper-syncd verify --remote`** asks the server whether it actually holds
  every object the pointers name. See below.

### Verifying the half that loses data

`keeper-syncd verify` checks that every pointer in the worktree names an object
*this machine* still has. That is the half answerable without a network.

`keeper-syncd verify --remote` asks the other half, and it is the one that finds
permanent loss: a pointer whose object never reached the server is a valid git
blob, a clean `git status` and a green folder, while the content exists on
exactly one machine in the world. Nothing in the sync path notices — the push
gate that should have prevented it is precisely what is being checked.

It transfers nothing: one batch round trip per few hundred objects, using the
`download` operation because its per-object 404 is the server saying "I cannot
serve this". A non-empty result exits non-zero so a cron wrapper sees it.

### The second local copy, and `lfsPruneLocal`

On the machine where content originates every LFS file exists **twice**: once in
the worktree, and once in `<git-dir>/lfs/objects` as the byte-identical object
the clean path streamed there to compute the pointer. That copy is unavoidable
at stage time — the bytes have to be read and hashed — but it is not needed
forever. Measured on a 211 GB archive: 215 GB of worktree content plus 215 GB of
store objects on one 920 GB drive.

keeper releases it at the end of a successful sync — `lfsPruneLocal` is **on by
default**. An object is released only when **all** of these hold:

1. **The journal references no transfer for it.** Not an inference from ref
   positions — keeper's own durable record of what it still owes. This also
   keeps prune from fighting the engine, which re-cleans an object it still owes
   an upload for (observed: delete one mid-upload and it is back within a
   minute).
2. **The worktree still holds the real content**, at the recorded length and not
   pointer text. This is what makes the release cheap to undo — the object is not
   the only local copy, the *file* is — and it is the condition `git lfs prune`
   cannot express, which is why its known failure mode (deleting objects for
   staged files, git-lfs#5636) cannot occur here.
3. **Nothing else is running.** It happens after the upload queue has drained to
   quiescence and after the push, never between them.

The honest trade: the drive stops being self-sufficient. Every file is still
there, but restoring one the worktree later loses now needs the network. Set
`lfsPruneLocal = false` on a machine that has to recover without one — the
object store then stays a complete local copy, at roughly twice the disk.

It was off by default until 0.8.12, and the reason for the change is what the
three conditions above already prove: the copy is redundant, and rebuilding it
costs one local read of a file that never left. As a default it was the wrong
way round — the cost was paid silently and continuously by every originating
machine, while the benefit only ever appears on a machine that is offline *and*
has lost a worktree file. A store written before the change is carried over once
by a marked migration in `db.rs`; a `false` set afterwards is an opt-out and is
never rewritten.

A failure to release is logged and never fails the sync — reclaiming space is
housekeeping.

### The rule is recorded per extension — which is why `lfsNever` exists

The decision is per file and by size, but the rule keeper writes into
`.gitattributes` covers the whole **extension** (`*.mp4`), so siblings are
covered and the block stays small on a media folder. That is the right trade in
a media folder and a trap in a mixed repository: one 300 KB note crossing the
threshold writes `*.md filter=lfs`, and from then on every note in the
repository is an opaque pointer — no diff, no merge, no blame.

Lowering the threshold to catch bulk media makes that outcome *more* likely, not
less. `lfsNever` is the escape hatch:

```toml
lfsThresholdBytes = 262144        # 256 KiB — catch bulk media
lfsNever = ["*.md", "*.txt"]      # ...but never the formats you read as text
```

Same dialect as gitignore: a pattern with no `/` matches its basename at any
depth, a pattern containing `/` is anchored at the repository root. A malformed
glob is refused at startup rather than ignored — an opt-out that silently does
nothing is how a note ends up an opaque pointer months later.

The trade-off it accepts is real, and it is the reason this is the user's call
rather than a built-in list: a matched file stays an ordinary git blob however
large it grows, and gitoxide has no streaming object read. Exclude formats you
need to diff, not formats you happen to have a lot of.

- **Downloads resume.** An interrupted transfer restarts from where it stopped,
  and the digest of the existing prefix is carried forward.
- **Uploads do not resume.** The LFS `basic` adapter has no resumable upload in
  the specification, `tus` is experimental and upload-only, and `multipart` is a
  proposal with no implementation anywhere. An interrupted upload retries from
  zero.
- **Every object is verified** against both its SHA-256 and its expected byte
  count, in both directions. A mismatch discards the staged bytes rather than
  resuming from a poisoned prefix.

### A pointer is never published ahead of its object

A pointer is ~130 bytes naming content by digest and carrying none of it, so a
commit pushed before its objects have been transferred produces the one failure
nobody observes: git accepts the push, the remote reports itself up to date, and
the next peer to clone checks out a tree of text stubs with no error anywhere.

So the push waits. While a folder owes the remote any object, its push is
**held** — the Sync view shows the folder as syncing and each affected file says
what it is waiting for — and it is released as soon as a sync pass finds nothing
outstanding. That check runs at the start of every pass rather than only when an
upload reports in, so a crash between the last upload and the release cannot
strand a folder holding a commit it is allowed to publish. Nothing is lost while
it waits: the commit is durable locally and the objects are in `.git/lfs`.

**The promise covers the commits keeper makes, which is narrower than it sounds.**
What the push consults is keeper's own queue of outstanding uploads, and that
queue only ever learns of an object from a commit keeper staged. A commit *you*
make with plain `git` (*Working in the folder with plain `git`*, below) runs
through the same clean filter and files the object into the same store — but it
queues no upload, so the gate has nothing to count, and the next push publishes
that pointer while its bytes are still only on your machine. Nothing afterwards
compares the pointers in history against the objects the remote holds, so keeper
does not notice later either. Hence the practical rule: **let keeper commit your
large files.** Save them and leave them; the sync that stages a file is the sync
that owes its upload. Committing a large file by hand is the one remaining way to
put an unbacked pointer on the remote.

### Where the objects actually go

The LFS API is always HTTP, even when git is not, so how keeper reaches it
depends on the remote:

| remote | endpoint | credential |
|---|---|---|
| `https://…` | derived, or `lfs.url` / `remote.<n>.lfsurl` from `.lfsconfig` | the profile's stored token, as HTTP Basic |
| `ssh://…`, `git@host:…` | the `href` `git-lfs-authenticate` names over ssh, else derived — `.lfsconfig` outranks both | the `Bearer` that command mints; the stored token only when the named host *is* the ssh host |
| a filesystem path, `file://…` | none — the objects are copied store to store — unless `.lfsconfig` names a server | none; the stored token if `.lfsconfig` named a server |

**An ssh remote needs no stored token.** `git push` over ssh authenticates with
your ssh key, and keeper asks the same server for an LFS credential the same way
the `git-lfs` client does: `ssh <host> git-lfs-authenticate <path> upload`.
Forgejo and Gitea answer with a short-lived `Bearer` JWT they sign themselves and
the endpoint to spend it at. keeper caches it briefly per repository and
operation — they send no expiry although the token really does expire, so keeper
imposes its own — and re-derives it whenever the server rejects one. The ssh call
runs with `BatchMode=yes`, no password prompts, no askpass and a connect timeout,
so it can never block a background sync on a passphrase or host-key prompt.

**The endpoint is the server's answer; your token is not.** The `href` keeper
contacts is whatever the server named, checked only for being `http(s)` — the
server is the authority on where its own API lives. On Forgejo and Gitea that URL
is built from `AppURL`/`ROOT_URL`, which is a *different* setting from
`SSH_DOMAIN`, so an install whose web name differs from its git-ssh name
legitimately names a different host and keeper follows it (both are disclosed in
[egress.md](egress.md)). What does not follow it is your credential: the profile's
stored token is attached only when the named host equals the ssh remote's host. A
server that mints its own `Bearer` is honoured wherever it points; one that names
a foreign host and mints nothing gets an unauthenticated request rather than a
token you stored for somewhere else.

If the remote has no `git-lfs-authenticate` at all — a plain bare repository
behind a login shell — keeper falls back to the derived HTTPS endpoint with the
stored token. If the remote *does* have it and refuses (LFS switched off, or your
key lacks access), the folder says so and quotes the server's own words, because
that message is the only diagnostic these servers give. A handshake that cannot
reach the server at all is a connectivity condition instead, and is retried like
any other.

**A filesystem remote has no LFS server** — unless the repository names one. A
pendrive is the case that matters (§6): `git push` has always copied its own
objects into such a remote, and keeper now does the same for the content the
pointers name, straight between the two `lfs/objects` stores, verified on arrival.
Without that a pendrive carries a tree of stubs — which is what it used to do.
The exception is `.lfsconfig`: a repository whose `lfs.url` or
`remote.origin.lfsurl` names an LFS server has settled the question, so keeper
talks to that server over HTTP with the profile's stored token even though the git
remote is a path on disk. That is git-lfs's own precedence and it is deliberate —
someone who names a server beside a pendrive meant it — but it is also why "a path
remote reaches no network" holds only in the absence of that file.

Against Forgejo specifically, keeper works around several server behaviours: the
LFS media type must be the first value in `Accept` (or the server returns 415);
the `Content-Range` total is computed incorrectly, so only the start byte is
trusted; and range offsets are parsed as 32-bit, so resume above 2 GiB falls
back to a restart.

### Working in the folder with plain `git`

keeper registers itself as the repository's `lfs` clean/smudge filter
(`filter.lfs.clean` / `filter.lfs.smudge` in `.git/config`) — whichever keeper
executable is running, the app or the daemon, since both answer
`lfs clean|smudge` on the same code. That is what lets you use ordinary `git`
inside a synced folder: `git status` stays clean, `git checkout` restores real
content rather than pointer text, and a commit you make by hand stores a pointer
and files the object into keeper's store, exactly as keeper's own commits do.

Two bounds on that, both worth knowing before you rely on it.

**Until 0.6.4 the app-registered filter did not work at all.** The desktop binary
registered itself and had no such subcommand, so every invocation failed — and
because the filter is registered as *not required* (below), git could not tell a
failed filter from an absent one and stored the bytes it was handed. A folder set
up by the app before 0.6.4 could therefore have committed a large file raw, and
read every LFS-tracked file as modified for no visible reason. Nothing repairs
such a commit retroactively; the filter simply behaves from here.

**A commit you make by hand is not covered by the publication gate.** The clean
filter stores the object, but only a commit keeper stages records that the object
is owed to the remote — see *A pointer is never published ahead of its object*
above. Committing a large file yourself and letting keeper push is the one way to
publish a pointer whose bytes never leave your disk.

The filter is registered as **not required**, deliberately. If the keeper binary
moves, git still works — checkouts simply yield pointer files, which is
recoverable. A required filter would instead hard-fail every git command in the
repository.

**A `git lfs install` elsewhere on the machine is ignored inside managed
folders.** That command writes an `lfs` filter driver into your *global*
`~/.gitconfig` — including `filter.lfs.process`, the long-running protocol — and
keeper drops every `filter "lfs"` section that does not come from the
repository's own `.git/config` before it reads a single file. It does this in
memory: your `~/.gitconfig` is never edited and keeps working for every
repository keeper does not manage. `filter.lfs.process` is also removed from the
repository's own config, where `git lfs install --local` would put it.

This is not tidiness. Until 0.8.9 such a driver took over completely: gitoxide
answers with the first `filter "lfs"` section it finds and does not merge keys
across scopes, so the global one hid keeper's `clean`/`smudge` *and* its
`required = false`. A `process` driver that cannot be launched — and on a desktop
launch `PATH` is Finder's, which does not include Homebrew's `git-lfs` — then
failed every `status` with `Process handshake with command … "git-lfs
filter-process" … failed to fill whole buffer`. Nothing committed, so the index
was never rewritten, so the next pass re-read the same file: the folder was stuck
for good. It is the same `git lfs install` whose hooks keeper already declines to
run (it drives `git` with `core.hooksPath` pointed at a path that cannot exist).

`lfsMode = pointerOnly` leaves excluded paths as pointer files. This is the only
lever that reduces LFS traffic — sparse checkout does **not**, because git-lfs is
entirely sparse-checkout-unaware. Such a file reads clean regardless: handed
bytes that are already a pointer, the clean filter re-emits them unchanged rather
than storing them and naming *that*, so the worktree text and the blob in the
index stay the same object. Byte-for-byte, not re-rendered — a pointer written
with a different but legal spelling would otherwise change the blob's hash and
the file would read as modified forever.

---

## 9. Provenance

Every commit keeper makes carries trailers, so *"where did this come from"* is
answerable from a clone alone, offline, with no keeper installed:

```
sync(tgdrive): 3 added, 1 modified

Keeper-Profile: tgdrive
Keeper-Device: Dev Laptop (01JQ8Z3K7N4YB2XR6W5V9TMCFD)
Keeper-Origin: delectra
Keeper-Source: watch
Keeper-Agent: keeper-sync/0.3.0
Keeper-Tag: drive
```

`Keeper-Source` is `watch`, `manual`, `cli` or `bot`. The device id is minted
once per installation; the label is editable and the id is not. By default the
git author email is a non-routable `sync@<device-id>.keeper.invalid`, so history
is attributable without publishing a real address — override it per profile if
you want a real one.

Provenance rides git's own metadata rather than a sidecar file precisely so it
cannot drift the first time someone uses plain `git`.

---

## 10. Offline behaviour

Offline is a normal state, not an error.

- Local git keeps working. Changes are detected, staged and committed.
- Fetch, push and LFS transfers are queued in the journal.
- Retries use exponential backoff with full jitter (2 s base, 10 min ceiling).
  The jitter is not cosmetic: it stops every profile — and every machine —
  retrying in lockstep the instant a server comes back.
- Connectivity is **observed from outcomes**, never polled. There is no
  reachability probe and no captive-portal heuristic.
- Work that became due while the app was closed runs on the first tick after it
  reopens.

The status line reports what is waiting, e.g. `tgdrive — offline, 12 waiting`.

### The half-offline case: a connection that neither works nor fails

Being offline is easy to handle because it announces itself. The expensive case
is a socket whose peer has gone — a laptop that changed networks mid-object, a
NAT mapping that expired, a link that stopped passing packets — because nothing
is delivered and no error arrives either. Without a timeout that transfer waits
forever, and since the engine runs one operation per profile and returns
in-flight units to the queue only at startup, everything queued behind it waits
with it. Observed on a folder pulling 53 GB: nine downloads sat in `running` for
sixteen hours, backoff long expired, 95 units behind them, not one byte written.

Transfers therefore bound **silence**, not duration: an established connection
that delivers nothing for 60 s fails as a network error and is retried with the
usual backoff, while a transfer that keeps delivering bytes may run for hours —
which multi-gigabyte objects on a slow link genuinely do. A total request
timeout would have to be longer than the longest legitimate transfer, and would
therefore be useless for catching a stall. Connecting is bounded separately, at
15 s.

Both live in `keeper_sync::http`, which is the only place a client is built.

---

## 11. Progress and warnings

- **Tray glyph.** Monochrome template icons distinguished by shape, never
  colour — the system recolours them for light and dark menu bars, so colour
  cannot carry meaning. Four rotating frames signal activity, advanced by the
  existing ~1 Hz tick; separate glyphs cover armed, paused/media-absent and
  warning.
- **Tray status line**, e.g.
  `Transferring tgdrive — 42/310 files · 1.2 GB of 4.7 GB · 104 files left, 53.1 GB`.
  The tail is the queue *behind* the file in flight, which the other numbers say
  nothing about: `1.2 GB of 4.7 GB` describes one object, and a folder pulling
  53 GB showed a line like it for two days without ever suggesting how much was
  left. Counted from the journal, never estimated — a remaining *time* on a link
  whose throughput varies by an order of magnitude is a guess dressed as a fact.
- **In-app**: a progress meter, the same line, and — while a transfer is running
  — the repository-relative path of the file being moved, under the bar beside
  the rate. The path rides the streamed progress rather than the line: that
  string is the tray's too, and a path four folders deep does not belong in a
  menu item. A queue that predates the name is filled in from the index on the
  next drain, once per folder per run, because naming otherwise happens only at
  enqueue and an upgrade mid-backlog would never reach that moment again.
- **In-app**: a sticky amber warning banner. Warnings that need a decision get
  an inline action button.
- **Notifications** fire exactly once per warning onset.
- **Never a toast** for connectivity or any other persistent condition.

Progress is reported in bytes where a total is known, because a file-counted bar
sits at 50% for ten minutes when one of the two files is a 4 GB video.

### The Pending list runs in both directions

`git status` and the completeness gate see only what this machine changed, so
until 0.8.14 the Pending list meant "not synced yet, **outbound**" while saying
"not synced yet". A folder pulling 53 GB listed nothing at all: its 106 queued
objects lived in the journal, which that list never read.

Queued LFS downloads now appear with the reason `incoming`, carrying the size —
the one fact worth knowing about an object that has not arrived, since a queue
of 106 is two minutes or four days depending on it. Names come from the same
`label` the transfer line uses, so a queue that predates them is filled in from
the index on the next drain; an object queued for a path since deleted cannot be
named and says so rather than being dropped, because a list that disagrees with
the count in the status line is worse than an ugly row.

Uploads are deliberately not listed: the path an upload carries is already
reported by `git status` as a local change, and one fact wearing two hats reads
as two.

### The Sync view's Activity list

Each folder's card lists the files it recently carried, newest first. Every row
says two separate things, one at each end: a glyph for what happened on this disk
(added, changed, deleted, conflict copy) and a glyph for how far it got toward
the remote.

| delivery | meaning |
|---|---|
| reached the remote | the work that had to deliver it finished |
| on its way | queued, running, or waiting on a condition — a held push (§8) is this |
| failed, still retrying | it failed and keeper is still trying; no button, because nothing has stopped |
| stopped retrying | keeper gave up; the row offers **Retry** |

A row with anything recorded against it opens a popover naming the file and
quoting the engine's message verbatim, so *why* a file has not arrived is
answerable where the file is. The Problems section further down remains the
inventory of stopped work — it is the only surface for failures that belong to no
single file, such as a fetch or a pull request — but it reports the unit of work
and never the path, which is why the reason now also lives on the row.

A file with no delivery glyph at all is one no unit of work is accountable for: a
row recorded before this column existed, or a conflict copy a merge just wrote,
whose publication belongs to a commit that does not exist yet. Absence there is a
deliberate answer, not a gap.

---

## 12. `keeper-syncd` — the standalone daemon

The same engine with no application installed. Linux-first, depends on neither
Tauri nor matrix-sdk.

```
keeper-syncd init                 # write a documented example config
keeper-syncd add ...              # register a profile
keeper-syncd list                 # profiles and their state
keeper-syncd status [--json]      # current status
keeper-syncd sync [--once]        # sync now
keeper-syncd watch                # the daemon entry point
keeper-syncd pause <id> | resume <id>
keeper-syncd verify [id]          # re-verify stored content
keeper-syncd doctor               # diagnose the environment
keeper-syncd logs
```

Paths follow XDG: `$XDG_CONFIG_HOME/keeper-sync/config.toml`,
`$XDG_DATA_HOME/keeper-sync/sync.db`, `$XDG_STATE_HOME/keeper-sync/`.

Configuration is TOML mapping one-to-one onto a profile, so a profile moves
between the app and the daemon by copying a table. **Unknown keys are an
error** — silently ignoring a typo is how someone ends up believing sync is
configured when it is not.

Secrets come from an environment variable or a per-key file under the config
directory. A secret file that is group- or world-readable is **refused**, not
warned about. Secrets are never written to the config, the database, a log, or
a commit.

Install the user service from `packaging/keeper-syncd.service`.
`SIGTERM` performs a bounded graceful finalize: an in-flight push aborts
resumably rather than being killed mid-write.

Exit codes: `0` success, `1` operational failure, `2` configuration error,
`3` missing prerequisite (no usable git).

### Installing, and keeping it current

Releases carry a prebuilt binary per target, so a server does not need a Rust
toolchain:

```
keeper-syncd-x86_64-unknown-linux-gnu          # + .sha256
keeper-syncd-aarch64-apple-darwin              # + .sha256
```

```
curl -fLO https://github.com/tgorka/keeper/releases/latest/download/keeper-syncd-x86_64-unknown-linux-gnu
curl -fLO https://github.com/tgorka/keeper/releases/latest/download/keeper-syncd-x86_64-unknown-linux-gnu.sha256
sha256sum -c keeper-syncd-x86_64-unknown-linux-gnu.sha256
install -m 0755 keeper-syncd-x86_64-unknown-linux-gnu ~/.local/bin/keeper-syncd
```

After that the daemon updates itself on request:

```
keeper-syncd update --check    # report what is available, change nothing
keeper-syncd update            # download, verify the checksum, replace the binary
```

`doctor` also reports an available version, as a **warning that never fails the
run** — a machine that could not reach GitHub is not a machine that is out of
date, and saying otherwise would make `doctor` lie in exactly the situation
where you need it to be honest.

**It never installs by itself.** The daemon holds a durable journal and can be
mid-push at any moment; swapping its binary on a timer is how a routine release
becomes a corrupted transfer. The install is also not a restart: the file is
replaced through a rename, and the running process keeps its old inode until
you restart it. `update` says so rather than leaving you to assume.

Integrity is a **checksum, not a signature**. The download is hashed while it
streams and refused on mismatch, so a truncated or substituted asset never
reaches disk — but that authenticates the transfer, not the publisher. The
desktop app's updater verifies a minisign signature and is the stronger of the
two. If you need publisher authentication on a server, build from source or
verify the release yourself.

If the release for your platform is missing, `update` says which asset it looked
for instead of failing vaguely; only `linux-x86_64` and `macos-aarch64` are
published today.

---

## 13. Making a folder visible to agents

The reason to run `keeper-syncd` on a dev box or a container rather than the
desktop app: an autonomous agent working in that environment sees a plain
directory, kept in step with a repository, with no application running.

```bash
cargo build --release -p keeper-syncd
install -m 0755 src-tauri/target/release/keeper-syncd ~/.local/bin/

keeper-syncd init
keeper-syncd add --name agent-data \
  --path ~/agent-data \
  --remote https://forgejo.example.com/dev/agent-data.git \
  --branch main
keeper-syncd doctor          # confirms git, paths, watcher limits, disk
keeper-syncd watch           # or run it under the systemd unit
```

Credentials never go in the config. Supply one of:

```bash
export KEEPER_SYNC_SECRET_SYNC_<PROFILE-ID>_CREDENTIAL='<token>'
# or, for a long-running daemon:
install -m 0600 /dev/stdin ~/.config/keeper-sync/secrets/sync-<profile-id>-credential <<< '<token>'
```

A secret file that is group- or world-readable is **refused**, not warned about.

Agents then read and write `~/agent-data` as an ordinary directory. Everything
in this document still applies to it: only complete files are committed (§4),
divergence produces conflict copies rather than prompts (§5), large files go
through LFS (§8), every change carries provenance naming the host (§9), and an
offline period queues work rather than losing it (§10).

Two things worth setting deliberately for an agent workspace:

- **`direction`**. A bot that proposes rather than commits wants
  `--direction pushOnly --lane worktree`, which publishes to a generated branch
  and opens a pull request instead of touching the base branch (§7).
- **`excludes`**. Agent tooling leaves scratch files around. The built-in tier-0
  set covers editor and download conventions but not your build outputs.

## 14. Security posture

- Credentials live in the OS keychain (or, headless, a `0600` file). Everything
  gitoxide drives — fetch and the first clone — takes them through a
  programmatic callback, so no helper subprocess is involved.
- **Push is the exception**, because it shells out to `git` (§1). It runs with
  the inherited `credential.helper` chain **cleared** and a helper of keeper's
  own that answers from the environment, never from the argument vector. The
  clearing is the security-relevant half: without it git falls through to
  whatever the OS store holds for that host, and keeper would push as an
  account the profile never named. A profile with no credential fails as
  unauthenticated rather than borrowing one — and for the same reason, a
  credential you have working in `git` alone will *not* be picked up here.
  Store it against the profile.
- SSH remotes delegate entirely to your own `ssh` binary, agent and config —
  including the LFS credential, which keeper obtains by running
  `git-lfs-authenticate` over that same ssh connection (§8). Keeper never reads a
  private key, and the token that command returns is held in memory for minutes,
  never written to `sync.db` or any log. The ssh call is made with
  `BatchMode=yes` and prompting disabled, so it fails rather than waiting for a
  passphrase nobody is there to type.
- **The profile's own token never leaves the host the profile named.** An ssh
  remote's server answers the handshake with the address of its LFS API, and it is
  free to name a different host (§8). A `Bearer` the server minted is spent
  wherever it points — it is the server's own credential, scoped by the server.
  Your stored token is not: it is attached only when the named host matches the
  ssh remote's host, so a compromised or merely misconfigured forge cannot use the
  handshake to redirect your keychain secret somewhere else.
- Every configured remote is a **disclosed egress destination** — disclosed in
  [egress.md](egress.md), which the release workflow diffs against the previous
  tag, per profile and including the LFS endpoint an ssh remote's server names.
  Note the boundary honestly: Settings → About renders the *app's* live egress
  list, and that list is computed from your signed-in Matrix accounts plus the
  update endpoint — folder-sync remotes are not in it. What keeps them from
  drifting is the release diff, not that view.
- Logs carry ids, hosts, paths and byte counts — never credentials, never file
  content.

---

## 15. Troubleshooting

**`doctor` first.** It reports git's presence and version, each profile's path
and writability, removable-volume presence, inotify limits, journal depth and
free space, and exits non-zero when something is genuinely wrong.

| Symptom | Cause and fix |
| --- | --- |
| Every sync surface is missing | No usable `git`. Install git ≥ 2.42. |
| A file never syncs | Check tier 0 — is it `*.part`, `~$…`, inside a `.download` package, or matched by a profile exclude? |
| A file syncs late | It is settling. Large files written slowly wait for the quiescence window, up to the 60 s ceiling. |
| `…sync-conflict-…` files appeared | Both sides changed the same file. Both revisions are kept; delete the one you do not want. |
| Profile says *drive not connected* | The volume marker is absent. Re-attach the drive; nothing was deleted. |
| A new removable profile stays *drive not connected* | Its folder is missing, or it is on this computer's own disk. Keeper will not mark either (§6): create the folder on the drive, or clear `removable`. |
| *A different volume is mounted at this path* | Another drive is where this profile's own drive should be. Nothing is synced either way until the right one is attached. |
| Authentication rejected, but `git` works in that folder by hand | Keeper uses the credential stored against the profile and nothing else — plain `git` is reading your OS credential store, which keeper deliberately ignores (§14). Add the token to the profile. |
| Watch mode misses changes on a network mount | inotify/FSEvents do not work on many network and FUSE mounts. Enable polling for that profile. |
| `Too many open files` / watches exhausted | Raise `fs.inotify.max_user_watches`. One watcher is used per profile, not per folder. |
| LFS upload restarts from zero | Expected. The `basic` adapter has no resumable upload. |
| Resume fails on an object above 2 GiB | Forgejo parses range offsets as 32-bit. Keeper restarts the transfer instead. |
| A large file's pointer is on the remote but its content is not | For commits keeper makes, fixed: the push waits for its objects (§8). For a commit that already exists, nothing re-drives it — keeper compares nothing between history and the remote's object store. If the file's row offers **Retry**, the upload is parked and that is the button; a sync pass alone never re-drives parked work. If there is no unit at all (the commit was made by hand, or predates 0.6.4), the only route is to change the file and let keeper commit the new revision, which queues a fresh upload. The old revision's bytes stay on the machine that made it. |
| *Publishing is on hold until this folder's large files reach the remote* | Not a failure. An upload has not landed yet; the next sync pass that finds nothing outstanding releases the push. If it persists, look at the affected file's own row for the reason the upload is failing. |
| Every large file fails on an `ssh://` remote although `git push` works | The LFS API is HTTPS even when git is ssh. Keeper asks the server for a credential with `git-lfs-authenticate` — if that is refused, the file's row quotes the server's own words. `Unknown git command` or `LFS Server is not enabled` means LFS is switched off on the forge. If the server answers but the transfer is rejected as unauthenticated, check whether its `href` names a different host from your ssh remote: keeper will not send that host the token you stored for this profile (§8), so such a server has to mint its own. |
| Large files never arrive on a pendrive or other path remote | Fixed: objects are copied between the two `lfs/objects` stores (§8). A transfer that ran while the drive was absent is deferred, and reattaching the drive releases it on the next pass; one that already parked needs **Retry** on the file's row. |
| History is growing fast | git keeps every revision. Run `git gc` on the repository, raise the LFS threshold, or exclude churning files. |
| `status failed: … Process handshake with command … "git-lfs filter-process" … failed to fill whole buffer`, over and over | A `git lfs install` on this machine wrote an `lfs` filter driver into your global `~/.gitconfig`, and until 0.8.9 that driver hid keeper's own and failed every `status` (§8). Fixed from 0.8.9 — update. On an older build, `git config --global --remove-section filter.lfs` (keeper needs no `git-lfs`), then **Sync now**. |

Enable **Settings → Advanced → Debug logging** for on-disk logs
(`~/Library/Logs/keeper/keeper.log` on macOS, `$XDG_STATE_HOME/keeper/` on
Linux).

---

## 16. Current implementation status

This document describes the designed behaviour. As of 2026-07-25 the engine and
the `keeper-syncd` daemon implement and verify §§1–10 and §12 against real git
remotes, including a full LFS round trip (upload, peer clone, download,
materialize) against a local LFS server and the review-lane airlock. One part is
not yet reachable at runtime:

- **§11 progress and warnings.** These are engine-side and correct — the tray
  decision, the status line and the warning onset logic are implemented and
  tested — but the desktop app surfaces that render them are not wired up.

## 17. Measured envelopes

Measured on a release build, Linux, AMD Ryzen AI 9 HX PRO 370, local disk, with
a `file://` remote so the numbers reflect the engine rather than a network.

| Scenario | Result |
| --- | --- |
| 100 000 files / 393 MB — first pass (scan, gate, commit, push) | 19.9 s, peak RSS 69.6 MiB |
| 100 000 files — steady state, nothing changed | 0.76 s, peak RSS 60.7 MiB |
| 2 GiB single file — adopt, clean to a pointer, store | 1.8 s, **peak RSS 17.5 MiB** |

The last row is the one that matters for the design: a 2 GiB object is handled
in about 17 MiB of memory, roughly a 120:1 ratio. That is the difference between
LFS streaming the content and gitoxide buffering it, and it is why §8 makes LFS
mandatory rather than optional.

Steady-state cost is dominated by one `lstat` per file, so a folder that is not
changing is cheap to keep watched.

## 18. Deliberate limitations

- **The daemon's update is checksum-verified, not signature-verified.** It
  proves the bytes arrived intact from the URL it asked; it does not prove who
  published them. The desktop app verifies a minisign signature and is the
  stronger of the two. Closing the gap means shipping a signing key to a
  binary that is frequently the only thing installed on a machine, which is a
  decision worth making deliberately rather than by default.
- **Only `linux-x86_64` and `macos-aarch64` binaries are published.** Other
  targets build from source. `update` names the asset it looked for rather than
  failing vaguely.
- **`update` never runs on a timer, and never restarts the daemon.** Both are
  deliberate; see §12.

1. **No content merge.** Divergent text files produce conflict copies, not a
   three-way merge.
2. **No resumable LFS upload.** It does not exist in the protocol.
3. **Sparse checkout does not reduce LFS traffic.** Use `pointerOnly` and
   `subpaths`.
4. **macOS has no open-writer veto** (see §4).
5. **A `git` binary is required** (see §1).
6. **No automatic history pruning.** Sync churn grows a repository; `git gc` is
   available but shrinking history is a destructive operation keeper will not
   perform on its own. `lfsPruneLocal` is not this: it releases *local object
   copies* the remote already holds and never touches history.
