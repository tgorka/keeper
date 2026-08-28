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
| `lfsMode` | `materialize` (**default**), `pointerOnly`, or `disabled`; releasing content needs `pointerOnly` (§9) |
| `lfsThresholdBytes` | Files at or above this are tracked through LFS (default 4 MiB) |
| `lfsNever` | Globs that never go through LFS, whatever their size (default none) |
| `lfsPruneLocal` | Release local LFS objects once the remote holds them (default **true**) |
| `virtualPatterns` | Paths whose absent content `verify` will not call a fault (default none; see §9) |
| `virtualOverBytes` | Size floor for that policy; `0` means no floor (default `0`) |
| `releaseTtlMs` | How long content may stay after its release clock last moved; `0` disables (default 24 h) |
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
   provenance trailers (§10).
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
*this machine* still has — except where the folder's virtual-file policy
authorizes that content to stay away. Those paths are counted and reported as
virtual (`N virtual`, `verified[].virtual` in `--json`) instead of being called
a fault, because that is the normal state of a folder that keeps pointers.

The policy in force is read from the `.keepervirtual` standing in the
**worktree**, never from `HEAD`, and a `virtualPatterns` list on the profile or
in a folder TOML layer above it replaces the file's list wholesale — so what is
in force may be neither committed nor in that file at all. (Protections, the
`!` lines, are the union of every source and are never dropped.) A folder with
no policy at all, and a folder whose `lfsMode` is `disabled`, excuse nothing.

An excuse needs every fact that is free, and misses none of them:

- the **index** carries a pointer for that path whose oid and size are the ones
  on disk, which is what tells a checkout's committed pointer from a
  pointer-shaped file somebody saved by hand;
- the **policy** answers that the path may stay away;
- the object is genuinely **absent** from this machine's store rather than
  sitting there truncated;
- and where the folder's remote is a **directory this machine can see**, that
  store holds the object. Without this one the division of labour below is
  false for an external drive, whose remote half asks nothing at all. A drive
  that is out is absence rather than failure, and leaves the excuse to the
  facts above it.

Everything unproven is still reported. That is the half answerable without a
network.

`keeper-syncd verify --remote` asks the other half, and it is the one that finds
permanent loss: a pointer whose object never reached the server is a valid git
blob, a clean `git status` and a green folder, while the content exists on
exactly one machine in the world. Nothing in the sync path notices — the push
gate that should have prevented it is precisely what is being checked.

It transfers nothing: one batch round trip per few hundred objects, using the
`download` operation because its per-object 404 is the server saying "I cannot
serve this". A non-empty result exits non-zero so a cron wrapper sees it.

### The second local copy, and `lfsPruneLocal`

On the machine where content originates, every LFS file whose worktree holds the
real content exists **twice**: once in the worktree, and once in
`<git-dir>/lfs/objects` as the byte-identical object the clean path streamed
there to compute the pointer. A path whose worktree bytes are the pointer is the
inverse case — there the store object is the *only* local copy of the content
(§9). That copy is unavoidable at stage time — the bytes have to be read and
hashed — but it is not needed forever. Measured on a 211 GB archive: 215 GB of
worktree content plus 215 GB of store objects on one 920 GB drive.

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
   staged files, git-lfs#5636) cannot occur here. It is also the clause that
   keeps this and virtual files (§9) off each other's ground: a path holding
   pointer text fails it, so a virtual path is **never** a prune candidate and
   the two features can never contend for the same byte.
3. **Nothing else is running.** It happens after the upload queue has drained to
   quiescence and after the push, never between them.

The honest trade: the drive stops being self-sufficient. Every file the worktree
still holds is intact, but restoring one it later loses now needs the network —
and so does a path whose content has been released, which holds pointer text and
nothing else (§9). Set `lfsPruneLocal = false` on a machine that has to recover
without one — the object store then stays a complete local copy, at roughly
twice the disk.

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

### A resume does not re-read what this process already hashed

A resumed download must still produce a digest over every byte, including the
ones it did not fetch this time, so `resume_offset` reads the whole `.part` back
and re-hashes it. Correct, and not cheap: a 970 MB partial on an external drive
is about fourteen seconds of reading before a single new byte is asked for.

On a link that interrupts often that cost is paid on every retry and it **grows
with progress**, which is what made a folder get slower the longer it ran.
Measured over 44 GB pulled through a 240 kB/s tunnel: three partials above
600 MB, all resuming repeatedly, and an effective rate that fell from the link's
own 300 kB/s to a fifth of it.

The hasher is now kept in memory when a transfer is interrupted, and handed back
**only** while the partial is exactly as long as it was when the state was taken.
Nothing here rewrites a partial in place — a download appends and nothing else —
so equal length means equal bytes; and the finished object's digest is still
checked against its oid before the file is committed, so a wrong prefix cannot
pass, only fail late. A restart falls back to reading, which is the honest answer
there: `Sha256`'s intermediate state has no serialized form.

### The filter's output is buffered, and why that shows up as energy

Content reaches git through the filter's stdout, and `std::io::Stdout` is a
`LineWriter`: it flushes to the last newline in every write it is handed.
Binary content has newlines all through it, so nothing coalesces — a large
object crossed the pipe in `io::copy`-sized mouthfuls, each one a syscall and a
wake for git on the other end.

That is invisible in a CPU column and plain in an energy one. A folder syncing
all day was measured at **8 200 context switches a second against 3% CPU**, with
about two idle wakeups a second — not a timer problem, an I/O-shape problem. The
filter now gathers its output in 256 KiB buffers, which is safe only because
every response already ends in an explicit flush; the protocol has to flush,
because git waits on it.

This is a reduction, not a cure. A folder moving tens of gigabytes over a slow
link is expensive because of the bytes, and no amount of buffering changes that.

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
inside a synced folder: `git status` stays clean — the property the whole design
rests on — a `git checkout` restores real content rather than pointer text
**wherever this machine holds the object**, and a commit you make by hand stores
a pointer and files the object into keeper's store, exactly as keeper's own
commits do. Where the object is not here the checkout leaves the pointer text in
the worktree instead: keeper hydrates nothing on read, and materializing is an
explicit verb (§9).

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

## 9. Virtual files

A clone can hold an LFS-tracked path as metadata only: the worktree carries the
committed pointer, the content stays on the server, and keeper fetches it back
when somebody asks for it. That state is called **virtual**. This chapter is the
operator's view of it — what the states mean, what decides them, what a release
refuses to do, and what this deliberately does not attempt.

None of it is a filesystem trick. A virtual file *is* the pointer git already
committed, which is both the reason the design works and the cost it accepts;
the last two sections say so plainly.

### The four states a row can be in

`EntrySyncStatus` (`browse.rs`) is the engine's answer for one path and
`FilesSyncStatusVm` is its wire spelling. Three of its eight variants are this
feature's, beside the ordinary `synced`; the remaining four — `waiting`,
`excluded`, `notInRepository` and `unknown` — are other chapters' business:

| state | glyph | what keeper says about it |
| --- | --- | --- |
| `synced` | a check mark (`Check`) | nothing — a synced file has no story |
| `virtual` | a cloud (`Cloud`) | "This file's content is not stored on this computer — only a placeholder is, so it takes up almost no space. The size shown is the content's." |
| `materializing` | an arrow into a line (`ArrowDownToLine`) | "keeper has this file's content queued to download to this computer." |
| `materialized` | a drive (`HardDrive`) | "This file's content is on this computer. keeper may release it again later to free the space, and can fetch it back." |

Those sentences are composed in Rust, in one function, and the app renders them
rather than writing its own. Each is also its state's accessible name.

**Shape carries the distinction and colour never does.** Two states that differ
only in hue are two states somebody eventually reads as the same thing, on a bad
monitor or with the common form of colour blindness. So each of the eight states
carries its own glyph and its own label — both maps are injective across all
eight — while the tone map deliberately **collides**: four states share
`text-faint`. Tone is emphasis, never information, and a reader who cannot tell
two tones apart loses nothing, because the difference was never carrying any.

`materializing` is **indeterminate, and says so**: the mark is a
`role="progressbar"` carrying no `aria-valuenow` at all, and that absence is the
state. keeper does not know how far a queued download has got and will not
invent a percentage.

A `virtual`, `materializing` or `materialized` path **travels**. Deleting one is
not a local space decision, and the confirmation says so in the same words it
uses for any other file that syncs: *"This file syncs, so deleting it here
removes it from every machine that syncs {profile_name}."* — the profile's own
name, which is not necessarily the folder's. Only an excluded path, and one in no
repository at all, are local-only.

Two edges follow from where those states are decided. **Only files reach
`virtual`, `materializing` and `materialized`** — the probe answers nothing for a
path that is not a file, and the classifier requires a non-directory before it
will say `virtual` — so a **folder** holding nothing but virtual content reads
`synced`. And **exclusion is decided first**, a profile's own pattern beating
everything else, so an excluded path reads `excluded` whatever its bytes are: a
virtual path inside an exclusion draws the local-only delete confirmation, not
the one above.

### How a path becomes virtual

The policy lives in **`.keepervirtual`** at the repository root, committed like
any other file and read from the **worktree**, never from `HEAD` — what applies
is the policy standing in the folder, not the one the last commit happened to
carry. An absent file is silence, not a fault — but only a genuinely absent one.
Any other failure to read it — a permission bit, a directory standing at that
name, bytes that are not UTF-8 — faults that folder's verify with `could not read
<path>: …`, which is not the quoted-pattern message below and so is easy not to
recognise.

It is not called `.keeperignore` because "ignore" is the wrong verb: these paths
are tracked, committed and wanted, and only their bytes may stay away. It plays
`.lfsconfig`'s role — the repository's intent, which a machine's own
configuration may override.

Same dialect as gitignore:

- a pattern with no `/` matches its basename at any depth;
- a leading `/` anchors at the repository root;
- a trailing `/` covers the subtree below it;
- a `!` line is a **protection** — this content stays here — and gets an
  implicit subtree expansion that positive lines deliberately do not;
- `\!` and `\#` escape a real filename beginning with either, and `#` is a
  comment in the file only, never inside a TOML array.

One departure, and it is the one worth remembering: **a protection wins
unconditionally**, rather than by being the last match. Protections are also the
**union** of every source, while a positive list from a higher tier replaces the
file's list **wholesale** — so what is in force may be neither committed nor in
that file at all. The boundary is worth stating exactly: a `virtualPatterns` list
that parses to **at least one line**, positive or protective, counts as that tier
having stated the policy and replaces the file's positive list entirely. A tier
carrying nothing but `!` protections therefore installs an empty positive list
and silently un-authorizes the whole committed zone, which `verify` then reports
as missing objects — so a protection written up there has to sit beside the
positive patterns you still want. `.keepervirtual` itself, `.lfsconfig`, `.git/`,
`.keeper/` and git's own control files — `.gitattributes`, `.gitignore` and
`.gitmodules` — can never be virtual whatever any pattern says; a virtualized
`.gitattributes` would break LFS routing for its whole subtree.

`virtualOverBytes` is a size floor under the whole policy: below it a path is
materialized whatever matched it. It defaults to `0` — no floor — and the
boundary is **inclusive**, so a file exactly at the floor is eligible. It comes
only from the profile tiers, because gitignore dialect has no spelling for a
size.

Precedence, ascending:

```
.keepervirtual  <  stored profile row  <  <folder>/.keeper/keeper.toml  <  <folder>/.keeper/keeper.<host>.toml
```

The folder files outrank the row keeper stored because they are resolved on
**every** read of that profile and are never written back: the row is what the
app last saved, the files are what the folder says now, and a value the folder
keeps stating keeps winning. Both files sync, the per-host one deliberately.
Note where it lives — `<folder>/.keeper/keeper.<host>.toml`, inside `.keeper`
rather than beside it.

**A malformed pattern is refused with the pattern quoted**, as typed, before any
anchoring rule has touched it:

```
invalid .keepervirtual pattern "media/[": …
```

and the message names the source carrying it: `.keepervirtual` for a line in the
file, `virtualPatterns` for one in the profile or a folder TOML layer. A typo in
a file every clone shares and a typo in one machine's own configuration are
different problems and deserve different words. The refusal happens where the
policy is compiled, which is inside `verify` — the daemon contains it to that
one folder's verify line and carries on with the rest. There is no process-wide
startup abort, and one folder's bad pattern stops nothing else.

A line that is only punctuation — a bare `!`, `/` or `!/` — is not refused but
dropped like a blank line, reported nowhere and not counted as that source having
stated a policy at all, which costs most on a protection the operator believes
they wrote.

**Editing the policy never deletes a byte.** Nothing in it deletes, prunes,
dehydrates, truncates or rewrites any worktree content, and nothing in it ever
will: a policy edit is allowed to change an *answer*, never a file. That is not
a style preference. It is git-lfs#3092, where a pattern change dropped content
that existed nowhere else. Only per-object proof ever authorizes deleting a
byte, and it is taken at the moment of the deletion.

**The policy authorizes; it does not instruct.** The only thing that consults it
today is `verify`, where its job is to stop a folder full of pointers being
reported as a fault (§8) — those paths are counted as virtual instead. Its answer
is one of the four facts that excuse an absent object, every one of which has to
be free (§8), and a folder whose tier is unset or whose `lfsMode` is `disabled`
excuses nothing whatever the policy says. It is not a download filter. What keeps
content off this machine on arrival is `lfsMode = pointerOnly`, the whole-profile
lever, or a path outside the profile's `subpaths`, the per-path one — or an
explicit release of content that was already here. A path the policy calls
virtual, whose object is in this machine's store, is materialized by the next
pull like any other.

### What a release refuses to do

Removing a path's content is the one operation in this chapter that can destroy
data, so it proves its case before it writes anything, and the proof is the same
whether you asked for it or the sweep did.

Before anything about the path is asked, a release is gated on the folder's
**mode**: only `lfsMode = pointerOnly` may let content go. A folder in the
default `materialize` mode refuses with `AlwaysMaterializes` — it would
re-materialize the path on the next pass, so the release would be undone — and a
folder with large-file support off refuses `LfsDisabled`. Both answer on the
request door and in the sweep alike, before the hash, before the question to the
server, and before the open-file question.

Behind that gate, six per-path refusals — seven release refusals in all, one
enum: `ContentRefusal`, carried by `SyncError::Refused`. Each is a
distinguishable value a caller branches on rather than a line in a log:

| refusal | the condition |
| --- | --- |
| `Modified` | the file does not hold the content this folder committed, or what stands there is not a regular file |
| `Pinned` | somebody asked keeper to keep this one |
| `UnprovenOnRemote` | nothing has confirmed the server can serve the object |
| `Open` | another program has the file open |
| `OpenUnknown` | this machine cannot tell whether anything has |
| `AlreadyPointer` | there was no content here to remove — the one refusal that repairs rather than declines |

`Modified` is decided by **hashing** the bytes — length first, then SHA-256 —
never by comparing timestamps, so a same-length edit cannot slip through; it is
also the answer when what stands at the path is not a regular file at all — a
directory, a fifo, a socket, a device node or a symlink — settled by one `lstat`
before any hashing starts. `Pinned` is checked **twice**: once before the two
expensive proofs, the hash and the round trip, and again with nothing at all
between that check and the deletion. `AlreadyPointer` is not a failure and exits
`0`; every other refusal exits `1`. It is also the one refusal that is not inert,
and its two callers differ: through `dehydrate` it refreshes that path's index
stat, which is the documented repair for a release interrupted between the rename
and the re-stat, while the sweep reaching it retracts the stale ledger row
instead — best-effort, and never for a pinned row. Every other refusal changes
nothing on disk.

The remote proof is asked per object at the moment of the deletion: one LFS
batch call using the `download` operation, because a per-object 404 there is the
server saying it cannot serve this. The answer has to carry a `download` action
with an href and an exactly matching size. Everything else — a transport
failure, a refused batch, a repository the credential cannot see — collapses to
*not proven*, which is a refusal and never a retriable fault.

**There is no trust-the-local-store escape, and its absence is deliberate.** The
store copy cannot be a precondition: `lfsPruneLocal` deletes the store object
precisely when the worktree holds the content (§8), so a rule that leaned on the
store would be leaning on the thing the other feature removes. The one local
shortcut is a **filesystem remote's own** object store, proved per object, and
only where no `.lfsconfig` names a server to ask instead.

**Where the operating system cannot answer "is this file open" without racing,
keeper refuses rather than guesses.** On **Linux** it can be asked, and keeper
asks it: `/proc/<pid>/fd` is read in-process — no `lsof`, no process spawn, no
new dependency, no `libc` — and a descriptor is matched to the target by
**device and inode identity**, taken by `stat`ing the magic link so procfs
resolves it straight to the open file's inode. That is the kernel's own answer
rather than a parsed text snapshot, and it is why a deleted-but-open file's
` (deleted)` suffix, another mount namespace's path prefix, a hardlink and a
`..` or symlink spelling of the same path are all non-issues. Both hosts answer
the same way from the same code: the `keeper-syncd` daemon, and the desktop app
when it runs on Linux.

On Linux, `closed` therefore means something exact: **no process whose
descriptor table keeper is permitted to read holds this file open.** `/proc/<pid>/fd`
is readable only by the process's own user, so a program running as another
user — root included — is a blind spot, and that narrowing is stated here rather
than discovered. It is an accepted cost with a reason. Demanding *total*
completeness would make the answer *cannot tell* on every Linux machine that has
ever booted, because pid 1 alone guarantees an unreadable table — which is the
refuse-everything outcome this whole section used to describe. And this
particular refusal is not what the no-data-loss guarantee rests on: the content
proof and the per-object remote proof are. A release is a `rename(2)` and never
a truncation, so a program that already has the file open finishes reading it
undisturbed; the harm from a missed opener is that its *next* open reads ~130
bytes of pointer text, and the content it wanted comes back by asking for that
path again — materialized from the server whose ability to serve that exact
object was proved a moment earlier, in the same release. Not from the local
store: `lfsPruneLocal` is on by default and releases the store copy precisely
when the worktree holds the content, so a materialized path is exactly the state
in which there may be no local copy left to fall back on.

Every blind spot that is not that narrowing **refuses**, and the list is short
and deliberate: no procfs at all, or one with no resolvable `/proc/self`, so
keeper cannot even establish whose process table it is reading; a `/proc/<pid>`
keeper cannot enter, where it cannot tell "somebody else's process" from "a
process hidden from us" — the `hidepid=1` shape, on which keeper therefore
refuses **every** release, correctly, because it can see that processes exist
about which it knows nothing; a descriptor table that stopped enumerating
part-way, because a table that was not finished was not examined; and a target
that cannot be `stat`ed. Each of those answers *cannot tell*, which is
`OpenUnknown`, which declines.

`hidepid=2` is **not** on that list, and the distinction is worth stating
because the intuitive reading is the wrong one. That setting makes other users'
`/proc/<pid>` directories invisible rather than merely unreadable, so an
unprivileged keeper enumerates only its own user's processes — and reads every
one of them in full. So it answers *closed* there, and that answer sits inside
the sentence above rather than contradicting it: what disappeared from the
listing is exactly the set whose descriptor tables keeper was never allowed to
read.

**On macOS and Windows the answer is still *cannot tell*, so both `dehydrate`
and the release sweep still refuse `OpenUnknown` there.** macOS publishes no
`/proc`, and the only routes to its per-process descriptor list are a wrapper
crate that drags a libclang build dependency into every release build, or
hand-written foreign-function calls whose answer struct is not declared by the
`libc` crate and would have to be laid out by hand — in the one crate that
cannot be built on the machine this was developed on, where a wrong layout reads
the wrong bytes in silence. `lsof` is refused by name. So the honest answer
there is the refusing one, and it is recorded rather than guessed. An ordinary
folder never reaches the question at all — its mode refuses first.

When a release does go through it publishes with a `rename(2)` and never a
truncation. The pointer is written to a sibling temporary file, given the
target's own mode, and renamed over the path — `rename(2)` leaves descriptors
already open on the old inode reading the old bytes, so a program part-way
through the file finishes reading the file it opened. `set_len` or a truncating
open would not: a reader would see the content vanish under it, and truncating a
file another process has `mmap`ed delivers SIGBUS. Immediately before the rename
the file is re-`lstat`ed against the size, the mtime and, on unix, the inode
recorded earlier in the pass, so any movement becomes an integrity error with
nothing published. After the rename the index entry's stat is refreshed; without
that the index still carries the content's stat while the file is ~130 bytes, and
`git status` reports the path ` M` forever. A crash between the two is repaired
by asking to release the path again.

### Two release clocks, and which one applies

Which clock measures a path is decided by where its content came from, and it is
recorded rather than guessed. keeper's ledger — the `materialized` table, keyed
by profile and path — carries `at_ms` (when the content landed), `last_used_ms`
(when keeper last served it), `synced_at_ms` (when the remote was observed
holding that path's object) and `local_origin`.

* Content that **arrived from the remote** and was never modified here is
  measured from `last_used_ms`, falling back to `at_ms` where keeper has never
  served it. keeper's own timestamp, never the filesystem's `atime`.
* Content **this clone authored** is measured from `synced_at_ms`, the instant
  the remote was observed holding it — and a `synced_at_ms` that is `NULL` means
  the path is **not eligible at any age**. Not zero, not the epoch, not
  absent-from-the-remote: never observed.

The second is stricter because those bytes may exist nowhere else. A file you
wrote that has not reached the server is on one machine in the world, and no
window makes deleting it acceptable. Editing a confirmed file puts it straight
back into that state: the write sets `local_origin` and clears `synced_at_ms`,
because the new bytes are not the ones the server agreed to.

`synced_at_ms` is written from exactly two places and both are per-path facts —
an upload unit's completion, which re-checks that the path still commits that
object id before writing anything down, and the per-object answer of the audit
that asks the server, written only once the server has actually been asked. It
is deliberately **never** written at a profile's success edge: "this folder
synced" says nothing about any one path inside it.

### `releaseTtlMs`, the per-pass budget, and when the sweep runs

`releaseTtlMs` is the window — how long content may stay after its clock last
moved. It defaults to **24 hours** and is settable on the profile row and in
either folder-TOML layer. **`0` disables releasing entirely**, and it does so
before any window is armed, so switching it back on later starts a fresh window
rather than firing on the next sync. A non-zero value under a minute, or one
over ten years, is **refused rather than clamped**, with the value named and `0`
offered as the way to turn releasing off: this knob never existed before, so
every out-of-range number is one somebody typed. A window between a minute and an
hour is accepted and then honoured no more often than hourly, because the look
gate below re-arms an hour ahead on every pass that runs — so a short window is
not a way to see the feature work sooner.

A pass is bounded by two ceilings rather than one — `RELEASE_BUDGET_OBJECTS`
(32) and `RELEASE_BUDGET_BYTES` (1 GiB). The count bounds **attempts** and is
checked before one, because the sweep runs inside the reservation the sync tick
already holds, and a pass walking forty thousand eligible paths would hold it
for minutes and starve the folder's watcher. The byte ceiling is checked
**after** an attempt, deliberately: the proof reads every byte of every
candidate before anything is deleted, so a count alone permits thirty-two
four-gigabyte files — 128 GB of hashing in a pass that is meant to be
housekeeping. Reaching it **stops** the pass rather than skipping the object,
because skipping would make anything larger than the whole budget permanently
unreleasable. A rotating per-profile cursor makes "the next pass takes the
remainder" true even when the first thirty-two always refuse. One residual, and
the code states it rather than leaving it silent: the accumulator adds
`row.size_bytes.unwrap_or(0)`, so a ledger row written before this feature
existed contributes nothing to the gigabyte, and a pass made of them is bounded
by the thirty-two-object count alone.

**The sweep rides the first successful sync after the window expires, and never
a timer.** There is no thread, no interval and no `.timer` unit — two schedulers
over one git repository produce concurrent index locks — and that edge is also
the moment keeper has just proved it can reach the remote it would have to fetch
the content back from. So **a folder that never syncs never releases anything**,
and neither does a paused one. Two further gates: the sweep is asked at most
once an hour, and on its first sight of a profile **in this run** it arms that
window and returns nothing. First sight is per run, not once in the folder's
life — the map that remembers it is in memory on the engine — so a freshly added
folder, a resumed one, and one whose daemon or app has only just started each arm
a fresh hour and release nothing on that pass. And it declines entirely for a
folder whose own configuration is **faulted**: either layer, `keeper.toml` or
`keeper.<host>.toml`, unreadable, failing to parse, carrying one unknown or
refused key, or failing validation. It does not fall back to the deleting
24-hour default — one bad key elsewhere in that file must not be discovered as
deletions.

A sweep failure never fails a sync. The whole pass's result collapses into one
warning; inside it every candidate's error is logged and skipped, and a refusal,
which is the expected answer for most candidates, is not even that.

A materialized row carries the **deadline**, not a countdown: the wire field is
an absolute epoch-ms instant, because a countdown is the one string Rust cannot
own — it is stale the instant it is serialized, while an instant stays true
without anyone re-asking. Where nothing is counting, the row carries a word for
why instead — it is pinned, its authorship is unconfirmed, the window is
indefinite, the folder's mode keeps everything, or the folder's large-file
support is off while the ledger rows it had survive — and exactly one of the two
is ever present. The last three all draw the same word, `Kept`, and it is the
sentence beside it that tells them apart: keeper keeps the reasons separate
because telling the owner of a `disabled` folder that it "is set to keep
large-file content" is false of the folder and points at a setting that reads the
other way. The pane ticks once a second, and only while some row is actually
counting.

Automatic release on that success edge is the only release mechanism built here,
and it runs only in a `pointerOnly` folder: nothing in a folder left in the
default `materialize` mode is ever released automatically, and the decline is a
debug line, so no surface will tell you why nothing happened. Releasing on a
**schedule** you choose — off, manual or scheduled — is **Epic 57's `tasks`
verb**, so the nightly script has a home rather than being something keeper
declines to do; releasing by hand is here today, as `dehydrate` and as the
**Release** action on a materialized row.

### The verbs

| verb | what it does |
| --- | --- |
| `ls-files [profile] [--remote]` | what this clone actually holds, per LFS path; `--remote` adds the per-object question to the server |
| `materialize <profile> <subpath>` | fetch one path's content, waiting for the transfer if the object is not here yet |
| `dehydrate <profile> <subpath>` | release one path's content, leaving the committed pointer |
| `pin <profile> <subpath>` | keep one path's content whatever the sweep says |
| `unpin <profile> <subpath>` | withdraw that instruction |

Two behaviours worth knowing before you script either of the first two.
`ls-files --remote` propagates the audit error, so a server that cannot be asked
fails the whole read-only command — and because the JSON document is printed once
after the loop, `--json --remote` then emits no document at all; missing objects,
by contrast, are reported and leave the exit code alone. And `materialize` on the
**command line** does not hand its transfer to anybody else: it drains what it
queued before it returns, and exits non-zero if the bytes did not arrive. The
**app's** door still queues and returns, because the app's own engine is the
supervisor that will deliver it and a UI must not block on a four-gigabyte fetch.

`--json` is a **global** flag and works on either side of the subcommand. §13
carries the wording contract — the lines each verb prints and the JSON field
names — and is the authority for that. Its refusal list is wider than this
section's, which covers only what a release refuses.

In the app the same operations are row actions, and a row's state decides which
it offers: **Materialize** on a `virtual` row, **Release** and **Pin** on a
`materialized` one, and nothing at all while a row is `materializing` — the
answer to "it is already coming" is to wait. Two vocabulary notes, worth having
before you go looking for something that is not there: the app says **Release**
where the CLI verb is `dehydrate`, and the app has no Unpin, because nothing on
the wire carries a **boolean** a toggle could read: the row learns that it is
pinned only as Rust's own word in `release.hold`, which cannot tell a control
which way it is about to go. `keeper-syncd unpin` is the door back out.

### What `ls`, `du` and other programs see

~130 bytes of pointer text. That is a real cost, it is accepted, and it was
chosen rather than discovered: **it is the only representation `git status`
tolerates**, because the pointer *is* the committed blob. Any other worktree
content is a modification forever, and every alternative fails on exactly that —
a sparse file, or a zero-filled file of the true length, is different bytes; an
extra key inside the pointer changes the blob's object id, a content change
wearing an annotation's clothes; and an xattr-identified stub does not survive
`cp`, `rsync` or `tar` without opt-in flags, so it becomes an anonymous file
after one copy.

keeper's own surfaces do not repeat that lie about a path whose content is away:
both report the size and object id the **pointer** names rather than the length
on disk. `ls-files` reads the pointer out of the **index**. The Files view parses
it out of the **worktree** file, and only for a row it has already marked
`virtual` or `materializing` — an excluded, untracked or waiting row that happens
to hold pointer text keeps its own ~130 bytes and carries no object id, and a
materialized row never reaches the substitution at all, because its bytes are not
a pointer. A virtual row shows the content's size because that is the fact worth
having; it is `ls` that is telling you about a placeholder.

### Finder and `ls` integration is closed, not pending

keeper will not make virtual content appear at its true size in `ls` or in
Finder. That is a **closed** question on macOS and a deferred one on Linux, and
the reasons are recorded as **D-2** in `docs/decisions.md` so they are not
re-asked every time somebody sees Dropbox do it:

- **macOS File Provider** has the right semantics and the wrong storage. An
  `NSFileProviderReplicatedExtension` keeps its domain under
  `~/Library/CloudStorage/<Provider>`, and there is no API to virtualize a path
  the *user* chose — which is the only kind of path a synced folder has. Nor can
  keeper mark its own files: `SF_DATALESS` *"may not be set or unset from user
  space"*. Kexts are policy-dead.
- **FUSE on macOS fails the licence firewall.** macFUSE forbids redistributing
  binaries bundled with commercial software and its kext is closed; fuse-t is
  free for personal use only. keeper's dependency policy is permissive-only and
  both fail it.
- **`fanotify` HSM on Linux is immature.** It needs kernel ≥ 6.14 and
  `CAP_SYS_ADMIN`, `mmap` materializes whole files because the page-fault hook
  was merged and backed out, directory events risk a filesystem-freeze deadlock,
  and every read re-fires the event because the planned BPF suppression is
  unimplemented. Revisiting it needs the page-fault hook and BPF event
  suppression both to land.
- **No on-read hydration, on any platform.** A `grep -r`, Spotlight, a backup
  agent or a `du` walking the tree would hydrate everything in it. keeper
  already took this side once, for iCloud placeholders (§4).

Linux is deferred with its shape recorded: a read-only FUSE **mirror** mount,
never a virtualization of the worktree itself.

The same rule holds inside git. A `git checkout` of a path whose object this
machine does not have yields the pointer text and leaves it there, which is
recoverable; nothing hydrates behind your back. Materializing is an explicit
verb, and that is the whole of it.

---

## 10. Provenance

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

## 11. Offline behaviour

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

A third knob lives beside them: an idle **pooled** connection is kept for at
most 20 s rather than `reqwest`'s default 90 s. Keep-alive is worth having — an
LFS batch and the download it authorises are two requests seconds apart — but a
pooled connection is only an asset while the peer still has it, and reusing one
the far side has forgotten costs a whole read timeout before anything is even
attempted.

That last one is a hypothesis rather than a diagnosis, and it is recorded as
one: throughput on a folder was seen decaying over hours (300 kB/s after a
restart, 25 kB/s an hour later) and restored by restarting the app every time. A
fresh process has an empty pool, which fits. Accumulated backoff does not
(nothing was waiting on a clock when measured), and neither does a resource leak
(67 MB, 62 threads, four sockets after an hour). If the decay outlives this, the
cause is elsewhere — and a 20 s window is still the better default.

All three live in `keeper_sync::http`, which is the only place a client is
built.

---

## 12. Progress and warnings

- **Tray glyph.** Monochrome template icons distinguished by shape, never
  colour — the system recolours them for light and dark menu bars, so colour
  cannot carry meaning. Four rotating frames signal activity, advanced by the
  existing ~1 Hz tick; separate glyphs cover armed, paused/media-absent and
  warning.
- **Tray status line**, e.g.
  `Transferring tgdrive — 42/310 files · 1.2 GB of 4.7 GB · 104 files left, 53.1 GB`.
  The tail is read from the journal every time a status is asked for, never
  remembered — a stored copy went stale between claimed batches and left the
  count unmoved for hours while completed files were dropping out of the
  Pending list beside it. It is the queue *behind* the file in flight, which
  the other numbers say nothing about: `1.2 GB of 4.7 GB` describes one object, and a folder pulling
  53 GB showed a line like it for two days without ever suggesting how much was
  left. Counted from the journal, never estimated — a remaining *time* on a link
  whose throughput varies by an order of magnitude is a guess dressed as a fact.
- **In-app**: a progress meter, the same line, and — while a transfer is running
  — the rate and then the repository-relative path of the file being moved, on
  one row under the bar. The figures lead because they are the fixed-width half
  and the half that changes every tick; the path truncates into what is left.
  The rate sits in a box reserved for the widest figure the formatter can
  produce (`999 bytes/s`) and is right-aligned inside it, so `2 kB/s` and
  `294.8 kB/s` leave everything after them in the same place; the box is held
  open, and stands empty, when there is no rate to show. The path rides the streamed progress rather than the line: that
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

Each row leads with one glyph answering two questions, because a row has space
for one: the arrow's **direction** says which way the file is travelling, and
whether it is **circled** says whether the far end already holds something. A
bare arrow is content nobody has yet; a circled one is a second version of
something that does. All four combinations exist. An inbound update is invisible in the repository —
a download is queued only for a path whose worktree holds pointer text, and that
is true of a new file and a new version alike — so it is read from keeper's own
record of what it has materialized here, written when content lands. A file
added upstream a week ago and never fetched is **new** to this machine however
old it is there, which is the question the mark answers.

The reason is no longer written out beside each path. It is the row's accessible
description, which is where it was useful; the right-hand column carries the
size instead, on **every** row — outbound sizes are measured off the worktree,
inbound ones are what the object announced. A list where only half the rows had
a figure read as two lists.

The row a transfer is on right now is marked, by name as well as by shade: a
background colour is nothing to a screen reader. Only that row — a row that is
merely next is not marked, because "in flight" is a fact and "about to be" is a
guess about a queue that reorders. It is also moved to the top, because a mark
below the fold of an eighty-row list is not a mark; nothing else changes place.

A deleted path carries a size like every other row. It cannot be stat'd — that
is what deleted means — so the figure comes from the index: what the pointer
names for an LFS entry, and the blob's own header length for an ordinary one.
Neither reads content.

### One object, one unit — including while it runs

`enqueue_unique` deduplicates against work that is queued. It used to ignore
work that is **running**, which is right for a push — that publishes whatever
the worktree holds when it runs, so a change made after it started needs its own
unit — and wrong for a transfer, which is content-addressed: the same oid names
the same immutable bytes.

The cost was invisible until the queue tail printed a total: a folder pulling
53 GB held **106 queued units for 95 distinct objects**, every duplicate a
running object re-queued by the next scan. The visible symptom is a queue that
never shrinks; the expensive one is the same bytes fetched twice.

Transfers now treat a running unit as cover, and recovery collapses the pairs
that already exist — while one half is `running` the pair is invisible, and the
moment startup returns it to `pending` there are two identical rows.

### Sync marks never hold up a listing

The Files view decorates each row with how far that path got toward the remote,
which comes from the same `pending` answer §12 describes — a whole-worktree
`git status` plus an untracked expansion that `lstat`s every candidate. The
listing used to **wait** for it before naming a single entry, and the pane's
refresh asked for it once per open directory: ten expanded folders, ten walks of
the same tree at the same moment. On a folder of tens of thousands of files on a
drive already saturated by its own transfers, that is minutes of an empty pane.

The walk is now bounded (3 s) and shared: one at a time per folder, its answer
reused for a few seconds, and — because the walk is spawned rather than awaited
in place — an answer that outruns the budget still lands, ready for the next
listing. A folder with no answer yet says so; it never reports its rows as
clean, which is the one wrong thing a fast listing could have done.

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

## 13. `keeper-syncd` — the standalone daemon

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
keeper-syncd ls-files [id]        # LFS paths: virtual, materialized or absent
keeper-syncd materialize <id> <path>  # fetch one virtual path's content
keeper-syncd dehydrate <id> <path>    # release one path's content again
keeper-syncd pin <id> <path>          # keep one path's content, whatever the sweep says
keeper-syncd unpin <id> <path>        # withdraw that instruction again
keeper-syncd doctor               # diagnose the environment
keeper-syncd logs
```

`ls-files` answers "what does this clone actually hold" for LFS-tracked paths.
Each row carries the size and object id the **pointer** names — never the ~130
bytes of pointer text a virtual path occupies on disk — plus a modification time
and, once a path has been materialized, what the ledger recorded about it. The
global `--json` flag makes the output a stable document whose field names are the
contract. The byte count is `sizeBytes`, and it is a **number for every row**,
`virtual`, `materialized` and `absent` alike: the pointer is the source in all
three cases, so there is no state in which it can be `null`. (Note that
`remote.missing[]`, which only `--remote` produces, is a different kind of
record and spells its own byte count `size`.) Remote presence is **absent unless
you ask**: `--remote` adds the same batch round trip `verify --remote` makes,
because whether the server holds an object cannot be known without asking it,
and a listing that implied it did would be guessing about the one thing worth
being sure of.

`materialize` is the verb that asks for one of those paths by name. If this
machine already holds the object it is published straight into the worktree and
the line says `materialized`; if the worktree already had the content the line
says `already materialized` and nothing is written; if the object is not here the
transfer is queued **and this command performs it before returning**, then
reports `materialized`. So a plain `keeper-syncd materialize` on a host with no
daemon anywhere still leaves the content on disk — which is what makes it usable
from a cron entry or a script, the same caller `sync --once` exists for. A run
that reports `materialized` is a run whose bytes are on disk, and its `--json`
document carries **no `unitId`**: the field names the row that *will* deliver
the content, and after a successful fetch there is no such row.

It waits for **your** transfer and nothing else. The wait ends the moment the
journal row for the object you asked for is settled, so a request for one small
file does not ride along behind the rest of a folder's download backlog; a queue
making no progress at all also ends it, so a transfer that genuinely cannot land
stops instead of spinning. And only transfers are performed — this verb never
commits, merges, pushes or opens a pull request on the way to fetching a file,
however much of that is sitting ready in the folder's queue.

A run that did not deliver **exits non-zero** and prints no materialization
document, so under `--json` stdout carries exactly one document per invocation:
the materialization, or the failure envelope. The sentence says which of four
things happened to the transfer, because they are not the same problem: another
keeper process is performing it right now (ask again shortly — nothing is
wrong); it will be retried, with the last recorded failure quoted; keeper has
**given up** on it, with the reason, and asking again queues a fresh attempt —
no `watch` daemon will ever pick a parked row up; or it finished and the content
is somehow not on disk, which asking again fixes.

Asking twice returns the same unit rather than queueing a second download of the
same bytes; a requested unit is claimed ahead of background work in the same
tick, though never as the *whole* tick, so the push that backs up your local
edits still runs. Two paths committed with identical content share one download,
and a request publishes every one of them.

The **app's** door is deliberately the other shape: it queues, returns
immediately and lets the app's own engine — which is the supervisor — deliver on
its next tick, because a UI must not block on a four-gigabyte transfer. One
engine function per shape, and no policy in either caller.

A path whose bytes on disk are neither the committed pointer nor the content it
names is **refused by name** and left exactly as it is: keeper does not
overwrite a local modification, and it will not write content back over a file
you deleted. It also refuses a **paused** folder (keeper writes nothing into
one), a folder with LFS turned off, a path outside the folder's `subpaths`, and
a subpath that leaves the folder — including through a symlink. Exit code `1`
with the reason on stderr; exit `1` too when the folder is busy syncing, so a
script can tell "you have the file" from "ask again".

`dehydrate` is `materialize` pointing the other way: it removes one path's
content and leaves the pointer the folder committed, byte for byte. The line
says `released` with the size you got back. It is the one verb here that can
destroy data, so it refuses before it writes anything — the folder **keeps
every object it holds**, which is what a folder does unless you set it to keep
pointers only, and a release there would be undone on the next pass; the file
is **not the content this folder committed** (decided by hashing it, not by
comparing timestamps, so a same-length edit cannot slip through); something has
it **open**, or this machine **cannot tell** whether anything has; nothing has
confirmed the **server can serve the object**, which is asked per object at the
moment of the deletion and where every failure to ask — unreachable, refused,
invisible repository — is also a refusal; the path is **pinned**; or it was
**already a pointer**, which is not a failure and exits `0` (`--json` says
`alreadyPointer`, with no `oid` and no `sizeBytes`). Every other refusal exits
`1` and changes nothing on disk. The release is a rename, so a program already
reading the file finishes reading it.

Once the content is gone the release **succeeded**, even if keeper could not
finish writing down that it happened: a locked database or a busy `.git` is
logged and the release still reports `released` and exits `0`. Calling a
completed deletion a failure would be the one thing this contract cannot
afford. Nothing is lost for good either — the note is rewritten the next time
that path's content lands, and an index entry left stale is repaired by asking
to release the path again.

**On Linux it goes through; on macOS and Windows it still refuses**, and that
split is deliberate rather than broken. Linux publishes every process's open
descriptors in a directory keeper can read, so keeper asks the kernel by inode
identity and gets a real answer — see "What a release refuses to do" above for
what *closed* claims there and the one narrowing it carries. macOS and Windows
give keeper no way to ask the question without racing, and guessing is the one
guess that deletes somebody's only copy, so the answer there is "cannot tell"
and the release declines.

### Letting go on its own: `releaseTtlMs`, `pin` and `unpin`

`dehydrate` releases one path because you asked. The **release sweep** releases
content whose retention window has run out, without anybody asking, and it runs
every one of the refusals above unchanged — the same hash of the actual bytes,
the same per-object question to the server taken at the moment of the deletion,
the same fail-closed answer when this machine cannot tell whether a file is
open. It is not a privileged caller, so **it releases where `dehydrate` releases
and refuses where `dehydrate` refuses**, for the same reasons.

Which clock applies to a path is decided by where its content came from, and it
is recorded rather than guessed:

* Content that **arrived from the remote** is measured from the last time
  keeper served it — an open, a text or document read, an export, the start of
  a media stream — falling back to when it landed if it has never been read
  through keeper. keeper's own timestamp, never the filesystem's `atime`, which
  on a `relatime` mount is a day stale and on a `noatime` mount never moves.
* Content **this machine created or modified** is measured from the instant the
  remote was observed holding it, and is **never eligible at any age until
  then**. A file you wrote that has not reached the server exists on one
  machine, and no window makes deleting it acceptable. Editing a confirmed file
  puts it back in that state, because the new bytes are not the ones the server
  agreed to.

`releaseTtlMs` is the window, per profile and settable in a folder's own
`.keeper/keeper.toml` (how long a repository's content may stay is a fact about
the repository, and the app and the daemon share no profile store). It defaults
to **24 hours**. **`0` disables releasing entirely** — the `lfs.pruneoffsetdays`
convention — and it disables it before any window is armed, so turning the sweep
back on later starts a fresh window rather than firing on the next sync. A
non-zero value under a minute, or one over ten years, is **refused at startup**
with the value and the reason named, rather than clamped: this knob never
existed before, so every out-of-range number is one somebody typed. And if a
folder's own `.keeper/keeper.toml` cannot be read at all, the sweep declines for
that folder rather than falling back to the 24-hour default — an operator whose
committed `releaseTtlMs = 0` stops applying because of a typo somewhere else in
that file must not discover it as deletions.

**It never runs on a timer.** There is no schedule, no thread and no `.timer`
unit: the sweep rides the first successful sync after the window expires, which
is also the moment keeper has just proved it can reach the remote it would have
to fetch the content back from. A folder that is paused, or that simply never
syncs, never releases anything. Neither does the **first** sync of a folder
keeper has not swept before — one freshly added, one just resumed, or one whose
daemon has only just restarted: that pass arms the window and releases nothing,
so a full interval always separates keeper first considering a folder from its
first deletion. A scan is a read and may run at once; this pass deletes. Each
pass is bounded — a few dozen paths and a gigabyte of hashing — and the
remainder waits for the next one: a pass resumes where the last one stopped
rather than restarting at the top of the folder, so a handful of paths that
refuse every time cannot hide everything behind them. Like `lfsPruneLocal`, it
can never fail a sync: a refusal is the expected answer for most candidates and
an error is logged and dropped.

`pin <id> <path>` is the absolute floor. A pinned path is never released, at any
age, by the sweep or by `dehydrate`, and the instruction is checked again
immediately before the deletion. It writes one fact and touches no file, so you
can pin a path before its content is here; it does refuse a path this folder
does not track as an LFS path, so a pin is never recorded where nothing will
consult it. `unpin` withdraws it and disturbs neither clock, so the path is due
whenever it would have been due had it never been pinned. `--json` for both is
exactly `profileId`, `profile`, `path`, `pinned`.

Releasing on a schedule you choose, or from a button, is a later story's
`tasks`; and no stored timestamp authorizes a deletion here — it only decides
which paths are worth asking about.

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

## 14. Making a folder visible to agents

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
through LFS (§8), every change carries provenance naming the host (§10), and an
offline period queues work rather than losing it (§11).

Two things worth setting deliberately for an agent workspace:

- **`direction`**. A bot that proposes rather than commits wants
  `--direction pushOnly --lane worktree`, which publishes to a generated branch
  and opens a pull request instead of touching the base branch (§7).
- **`excludes`**. Agent tooling leaves scratch files around. The built-in tier-0
  set covers editor and download conventions but not your build outputs.

## 15. Security posture

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

## 16. Troubleshooting

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
| Authentication rejected, but `git` works in that folder by hand | Keeper uses the credential stored against the profile and nothing else — plain `git` is reading your OS credential store, which keeper deliberately ignores (§15). Add the token to the profile. |
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

## 17. Current implementation status

This document describes the designed behaviour. As of 2026-08-25 the engine and
the `keeper-syncd` daemon implement and verify §§1–8, §10, §11 and §13 against
real git remotes, including a full LFS round trip (upload, peer clone, download,
materialize) against a local LFS server and the review-lane airlock. Virtual
files (§9) are real and exercised end to end: the excuse the policy gives
`verify`, the `ls-files` inventory, `materialize` on demand, the four row states
with the sentences and glyphs that carry them, `pin`/`unpin`, and — in a
`pointerOnly` folder — the release deadline a materialized row counts down. A
folder in the default `materialize` mode answers with a word and no instant, so
no row counts there: a counting row needs `lfsMode = pointerOnly`, a non-zero
`releaseTtlMs`, an unpinned row and, for content this clone authored, a
`synced_at_ms` that is not NULL. One part is not reachable at runtime, and one
is reachable on Linux only:

- **§12 progress and warnings.** These are engine-side and correct — the tray
  decision, the status line and the warning onset logic are implemented and
  tested — but the desktop app surfaces that render them are not wired up.
- **Releasing content (§9), on macOS and Windows.** `dehydrate` and the release
  sweep are implemented, covered by tests, and **live on Linux**: the daemon and
  the desktop app there both answer the open-file question from `/proc` by inode
  identity, so a `pointerOnly` folder really does release. Two conditions still
  gate it — a folder reaches the open-file question only when its `lfsMode` is
  `pointerOnly`, because the default `materialize` mode refuses before it, and
  macOS and Windows cannot answer that question without racing, so both refuse
  `OpenUnknown` there. Nothing releases content on a macOS or Windows machine
  until that platform can answer the question.

## 18. Measured envelopes

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

## 19. Deliberate limitations

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
  deliberate; see §13.

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
7. **A virtual file looks like ~130 bytes to everything else.** `ls -l`, `du`
   and third-party applications see the pointer text rather than the content's
   size, and there is no filesystem virtualization on any platform — a closed
   question on macOS and a deferred one on Linux (§9; `docs/decisions.md` D-2).
