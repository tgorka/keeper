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
| `lfsMode` | `materialize` (**default**), `pointerOnly`, or `disabled`; `disabled` never lets content go, `pointerOnly` always may, and `materialize` may for whatever its virtualization policy authorizes (§9) |
| `lfsThresholdBytes` | Files at or above this are tracked through LFS (default 4 MiB) |
| `lfsNever` | Globs that never go through LFS, whatever their size (default none) |
| `lfsPruneLocal` | Release local LFS objects once the remote holds them (default **true**) |
| `virtualPatterns` | Paths whose content may stay on the server: not fetched on arrival, releasable, and not a fault when absent (default none; see §9) |
| `virtualOverBytes` | Size floor for that policy, inclusive — and, with no permissive pattern in force from any source, the selector itself; `0` is no floor, so nothing stays away *for being large* — it decides nothing about a folder that named patterns (default `0`; see §9) |
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
in a folder TOML layer above it replaces the file's list wholesale where it
carries at least one permissive line of its own (§9) — so what is in force may be
neither committed nor in that file at all. (Protections, the `!` lines, are the
union of every source and are never dropped.)

Two folders excuse nothing whatever else is true: one whose `lfsMode` is
`disabled`, and one no source has said anything about — no `.keepervirtual`, no
`virtualPatterns` and no `virtualOverBytes` floor — **unless** its `lfsMode` is
`pointerOnly`, which is itself the folder saying every tracked path may keep its
pointer. A `pointerOnly` folder therefore needs no `.keepervirtual` and no
`virtualPatterns` to have its absent objects excused. That is the only fact the
mode replaces; a `pointerOnly` folder earns the other three per path exactly as
every other folder does, and its excused paths are counted in the same
`virtualPaths` number.

An excuse needs every fact that is free, and misses none of them:

- the **index** carries a pointer for that path whose oid and size are the ones
  on disk, which is what tells a checkout's committed pointer from a
  pointer-shaped file somebody saved by hand;
- the path is **authorized** to stay away — either the policy says so, or the
  folder's `lfsMode` is `pointerOnly`, which says so for the whole folder;
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

`lfsMode = pointerOnly` leaves excluded paths as pointer files. It is one of
keeper's three levers over LFS traffic — the others are `subpaths` and §9's
virtualization policy, which since story 56.10 is asked before anything is
fetched — while git's own sparse checkout reduces **none** of it, because git-lfs
is entirely sparse-checkout-unaware. Such a file reads clean regardless: handed
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
that file at all. The boundary is worth stating exactly, and it changed in story
56.14: the override is decided on the **permissive half alone**. A
`virtualPatterns` list that parses to at least one *positive* line replaces the
file's positive list entirely; a list carrying nothing but `!` protections
replaces nothing, and its protections simply union in with everyone else's. That
is AD-123's rule in one sentence — a policy edit may widen what may leave and may
never narrow what is kept — and the older reading broke it in the worst
direction: one machine restating one exception installed an empty positive list
and silently un-authorized the whole committed zone, which `verify` then reported
as missing objects. `.keepervirtual` itself, `.lfsconfig`, `.git/`, `.keeper/`
and git's own control files — `.gitattributes`, `.gitignore` and `.gitmodules` —
can never be virtual whatever any pattern says; a virtualized `.gitattributes`
would break LFS routing for its whole subtree.

`virtualOverBytes` is a size floor under the whole policy: below it a path is
materialized whatever matched it. When **no permissive pattern is in force from
any source** it is also the **selector** (story 56.16) — a size is a statement
about which files may stay away, so a folder whose only virtualization setting is
a floor authorizes every LFS path at or above it, and `tier` reports `SizeFloor`
for that state. A committed `.keepervirtual`, or a profile list with a positive
line, takes that job back: the floor never widens a zone some source already
named, and inside such a zone it keeps its older job of holding the small ones
back. It defaults to `0`, which is no floor: nothing stays away **for being
large**, and that is the whole of what a zero says. It decides nothing about a
folder that named patterns — with `virtualOverBytes: 0` and a committed
`.keepervirtual` covering `40-media/**`, `tier` is `PatternFile`, the folder is
consulted on every verify and every release sweep, and everything under that
pattern stays away. Only where no source named anything either does a zero leave
`tier` at `Unset` and the folder unconsulted. So putting the floor back to `0` is
not how a folder is made to stop virtualizing: the pattern that named those paths
is what decides them, and it is the line to remove.
The boundary is **inclusive**, so a file exactly at the floor is eligible. It
comes only from the profile tiers, because gitignore dialect has no spelling for
a size.

Three bands result, and the middle one is the one operators ask about. With
`lfsMode: materialize`, `lfsThresholdBytes: 262144` and `virtualOverBytes:
1048576`:

```
< 256 KiB          not LFS at all — stored in git, always present locally
256 KiB … 1 MiB    LFS-tracked (uploaded to the server) AND kept on this computer
>= 1 MiB           LFS-tracked and a placeholder — fetched when it is opened
```

That is the intended shape rather than a gap: the floor is a *local space*
decision and the LFS threshold is a *transport* decision, and they are allowed to
disagree. Under `pointerOnly` or `disabled` there is no middle band, because
neither mode both tracks a file and keeps its content here.

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
different problems and deserve different words. The refusal happens wherever the
policy is compiled, which is every consumer above — a folder's verify line, its
sync pass, a release request or sweep, and the Files pane's listing — and it is
contained to that one folder either way: the daemon reports it on that folder's
line and carries on with the rest. There is no process-wide startup abort, and
one folder's bad pattern stops nothing else.

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

**The policy authorizes; it does not instruct** — but it has four consumers, and
one of them runs before a byte is fetched. `verify` asks whether an absent object
is a fault or the ordinary state of a folder that keeps pointers (§8); its answer
there is one of the four facts that excuse an absent object, every one of which
has to be free (§8). The **arrival** path asks before it publishes or downloads
anything and skips both for a path the policy authorizes, which is what makes
"do not fetch the big ones" mean it rather than fetching them and calling them
virtual afterwards — so the policy *is* a download filter, and has been since
story 56.10. The **release** door asks it twice, once about the folder before any
path is named and once per path (both below). And the **Files pane** asks it per
materialized row, to decide whether that row counts a deadline down or carries a
word instead. Two folders excuse and authorize nothing whatever else is true, and
they are §8's two: one whose `lfsMode` is `disabled`, and one no source has said
anything about — no permissive pattern in force from any source and no positive
`virtualOverBytes` floor, so `tier` is `Unset`.

What the policy still never does is delete. A `Virtual` answer makes content
*eligible* to be given back; only a release takes it — `dehydrate`, or the sweep
— per object, and only after every refusal below is proved. The whole-profile
lever is still `lfsMode = pointerOnly` and the coarse per-path one is still the
profile's `subpaths`; the policy is the fine one, and the only one a repository
can commit for every clone.

Story 56.16 has two consequences for a folder whose **only** virtualization
setting is a size floor, and both are behaviour its owner will see. **`verify`
stops calling it faulty**: its tier is `SizeFloor` rather than `Unset`, so absent
objects at or above the floor are counted as virtual instead of reported as
missing. For somebody who set the floor deliberately that is the report finally
agreeing with the configuration; for somebody who set it while the setting was
inert, it is a signal that goes quiet. **And the release sweep arms for it**: the
folder now passes the mode gate below, so local content past its `releaseTtlMs`
becomes eligible to be given back. That is the point rather than a side effect —
a folder that may never let go can never become light — and *eligible* is the
whole of it: every individual deletion still proves its own case per object,
through every refusal in *What a release refuses to do* below.

### What a release refuses to do

Removing a path's content is the one operation in this chapter that can destroy
data, so it proves its case before it writes anything, and the proof is the same
whether you asked for it or the sweep did.

Before anything about the path is asked, a release is gated on the **folder**,
and two of the three modes answer there alone. `lfsMode = pointerOnly` lets
content go by configuration — every path in such a folder is virtual on purpose,
so no policy is compiled and no file is read. A folder with large-file support
off refuses `LfsDisabled`. The default `materialize` mode is neither answer: its
policy is compiled here, and the folder is refused `AlwaysMaterializes` only
where that policy **authorizes nothing at all** — no permissive pattern in force
from any source and, since story 56.16, no positive `virtualOverBytes` floor
either. That is the state the refusal was always about, and its old reasoning is
intact for it: a folder that may keep every path would re-materialize this one on
the next pass, so the release would be undone and the fetch spent twice. A
`materialize` folder that authorizes *something* has no such answer to give and
releases, as it has since story 56.10 — and because story 56.16 brought the bare
floor inside that question, a folder whose only virtualization setting is a floor
now passes this gate where it could not before.

The same compiled policy is then asked again per path, at the **committed
pointer's** size rather than the worktree's, so the answer is the same before and
after a release: a path it does not authorize refuses `NotVirtual`, which names
the pattern list to add a line to rather than the folder's mode to change. Every
one of these answers on the request door and in the sweep alike, before the hash,
before the question to the server, and before the open-file question.

Behind those two gates, six more per-path refusals, all values of the one enum
`ContentRefusal` that `AlwaysMaterializes`, `LfsDisabled` and `NotVirtual` belong
to, carried by `SyncError::Refused`. Each is a distinguishable value a caller
branches on rather than a line in a log:

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
shortcut is a **filesystem remote's own** object store, proved per object.

`.lfsconfig` still outranks it: a repository that names an LFS server has settled
where the question goes, and that server's answer decides. The store is asked
when there is no server keeper can actually address — either no `.lfsconfig` at
all, or one whose named endpoint cannot be reached from a path remote. Before
this, such a folder refused `UnprovenOnRemote` for ever and could never release
anything. It is still fail-closed in the direction that matters: a store that
does not hold the object at exactly that size keeps the local copy.

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
holding that path's object), `local_origin`, and `release_at_ms` (an instant
somebody named for this one path, and `NULL` on every row nobody has named one
for, which is nearly all of them).

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

A single path can also carry an instant somebody named, instead of inheriting
the folder's window. `keeper-syncd materialize <profile> <subpath> --for 2h`,
and the same choice on a Files row, write an absolute `release_at_ms` against
that one path, and from then on that is the instant the sweep reads. It
**replaces** the folder's window in both directions rather than bounding it:
two hours against a twenty-four-hour folder means the path goes twenty-two
hours sooner than everything around it, and a week against the same folder
means it stays six days longer. It is deliberately not a floor, not a ceiling
and not a `min`/`max` blend of the two clocks — somebody who asks for a file
*for two hours* has said what they want, and a window they never set is not a
better answer than the one they gave. The pin still outranks it absolutely,
because a pin is checked before any clock is looked at: a pinned path carrying
a deadline an hour in the past is not a candidate, and is never even asked
about.

What a named instant does **not** do is move the locally-authored rule. The
provenance question above is asked first and unconditionally, so a path this
clone wrote that nothing has confirmed the server holds is still **not eligible
at any age** — chosen duration or not. That ordering is the whole point rather
than an accident of where the code fell: if a deadline could be honoured over a
`NULL` `synced_at_ms`, then `--for 1m` would be a way around the one barrier
that exists to stop keeper deleting bytes that live on exactly one machine, and
the barrier would be worth nothing to the person it protects. The instruction is
also spent the moment it is served — a release clears `release_at_ms` along with
the content — so a path materialized again later starts on the folder's window
again, rather than being instantly eligible off a deadline already in the past.

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

**And `0` stays `0` for a path somebody named a deadline for.** A per-file
instruction does not resurrect the sweep in a folder whose automatic release is
switched off: the window is never armed and the pass returns before it reads a
clock, so nothing is released, and the row goes on reading `Manual`, the word
for an indefinite window, which is still the truth about it. That direction is
deliberate rather than a case nobody thought about. The documented meaning of
`0` is *keeper deletes nothing here on its own*, and a per-file deadline that
punched through it would make that knob unsafe to lean on — for exactly the
operator who set it because deletions in that folder were unacceptable, and who
would discover the exception as missing bytes. A folder with no clock has no
window for a deadline to override. The instruction is still recorded against the
path, and it starts being honoured the moment the folder's window is switched
on.

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

**The automatic sweep rides the first successful sync after the window expires,
and is never itself on a timer.** Nothing inside the engine schedules it: no
thread, no interval and no in-process timer — two schedulers over one git
repository produce concurrent index locks — and that edge is also the moment
keeper has just proved it can reach the remote it would have to fetch the
content back from. (A `release` **task** is a second, explicit driver for the
same sweep, and §14 covers it — including that with no release task stored,
nothing in this section changes. keeper does ship a `.timer` unit, and what it
triggers is that task's one-shot verb, not this pass.) So **a folder that never
syncs never releases anything** on the success edge,
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
indefinite, the folder's mode keeps *this path* (`materialize`, with nothing in
its policy authorizing that path to stay away), or the folder's large-file
support is off while the ledger rows it had survive — and exactly one of the two
is ever present. The last two both draw the same word, `Kept`, and it is the
sentence beside it that tells them apart: keeper keeps the reasons separate
because telling the owner of a `disabled` folder that it "is set to keep
large-file content" is false of the folder and points at a setting that reads the
other way. The pane ticks once a second, and only while some row is actually
counting.

Automatic release on that success edge runs wherever the folder authorizes
something: in a `pointerOnly` folder always, and in a `materialize` one for the
paths its policy authorizes — which, since story 56.16, includes a folder whose
only virtualization setting is a `virtualOverBytes` floor. A folder that
authorizes nothing at all declines the whole sweep, and the decline is a debug
line, so no surface will tell you why nothing happened.

There are now three ways content is released, and they share one body. This
success edge is the automatic one. Releasing **by hand** is `dehydrate` and the
**Release** action on a materialized row. And releasing on a **schedule you
choose** is a `release` task (§14), whose three modes — `off`, `manual`,
`scheduled` — decide what happens to this success edge as well: `manual` stops
it riding the sync, `scheduled` leaves it running and adds a schedule, and with
no release task stored at all nothing here changes.

### The verbs

| verb | what it does |
| --- | --- |
| `ls-files [profile] [--remote]` | what this clone actually holds, per LFS path; `--remote` adds the per-object question to the server |
| `materialize <profile> <subpath> [--for <duration>]` | fetch one path's content, waiting for the transfer if the object is not here yet, and optionally keep it for a stated time rather than for the folder's window |
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

`--for` takes a whole number of minutes, hours or days — `30m`, `2h`, `1d` — and
`0`, which means indefinite: the path is on the folder's own window and nothing
else, which is what the verb has always done with no flag at all. The two are
not quite the same instruction, and the difference shows on exactly one row.
**No flag** says nothing about retention, so a deadline this path is already
carrying is left standing — that is what keeps `materialize` unchanged for every
script and every internal caller written before this existed, the copy planner
among them, which hydrates a path only so a `copy` can read real bytes and has
no opinion about how long it stays. **`--for 0`** is somebody saying
*indefinitely* out loud, and it withdraws such a deadline. On the ordinary path,
which is carrying none, the two do the same nothing.

Anything else is refused before the command runs at all, in one
sentence that names those forms and quotes back what was typed: `1w`, `2`,
`-1h`, `1.5h`, `2 h`, `0m`, the empty string, and a number of days so large the
product overflows are all the same refusal, taken by the argument parser before
a profile is opened or a pointer is looked at. That is the same habit as
`releaseTtlMs` being refused rather than clamped — a duration is the kind of
thing a script gets wrong once, and hearing about it before any bytes move
costs nothing, while a `materialize` that fetches four gigabytes and *then*
argues about its flag costs the whole transfer.

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

**Materialize** now offers the same choice at the click. Right-clicking a
`virtual` row opens the verb into a submenu — 1 hour, 8 hours, 24 hours,
indefinitely — and every one of them is the same command with a different
duration, so there is no second verb to learn and no dialog to dismiss. The
promoted icon control in the hover cluster keeps the single meaning it has had
since it was first drawn — fetch it and say nothing about how long — because an
icon has no room for four words and a control that silently picked one of them
would be worse than one that picks none. That is the CLI's no-flag case, and
the submenu's *Indefinitely* is its `--for 0`; the four choices and the button
are one command with one argument, not two verbs.

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
keeper-syncd tasks list | status <task> | run <task>   # scheduled housekeeping (§14)
keeper-syncd tasks set <task> [flags] | enable <task> | disable <task> | forget <task>
keeper-syncd doctor               # diagnose the environment
keeper-syncd logs
```

`ls-files` answers "what does this clone actually hold" for LFS-tracked paths.

**The printed row and the `--json` document do not carry the same fields**, and
the difference matters if you are reading one and scripting the other. The
printed form is a count line followed by one row per path, and a row is four
columns — the state, the size, the path, and the object id — with `[pinned]`
after it when the path is pinned:

```
tgdrive: 2 LFS path(s) — 1 virtual, 1 materialized, 0 absent
  materialized     4.0 MB  40-media/clip.mp4  3f79bb7b435b05321651daefd374cdc681dc06faa65e374e38337b88ca046dea
  virtual        128.0 MB  scans/2019/box-7.tiff  9b2c1f04a7e5d3018f6b24cc90ae71d5386f0b4e2c7a915d68f3410b7d33ae10  [pinned]
```

The object id is printed **in full** rather than abbreviated: it is the handle
`verify`, the store and every later verb take, and a prefix somebody has to
expand by hand is not a handle. Every row's size and object id are the ones the
**pointer** names — never the ~130 bytes of pointer text a virtual path occupies
on disk.

A **modification time**, and what the ledger recorded about a path once it has
been materialized, exist only under `--json`: they are `mtimeMs`,
`materializedAtMs`, `lastUsedMs` and `syncedAtMs`, and no `keeper-syncd ls-files`
invocation without `--json` prints any of them. (This paragraph used to claim the
printed row carried a modification time. It never has.)

The global `--json` flag makes the output a stable document whose field names are
the contract. The byte count on a listing row is `sizeBytes`, and it is a
**number for every row**, `virtual`, `materialized` and `absent` alike: the
pointer is the source in all three cases, so there is no state in which it can be
`null`.

**One document, two spellings of "byte count", deliberately named here so you do
not find out by reading a `null`.** A listing row spells it `sizeBytes`; a
`remote.missing[]` entry — and a `repair.unrecoverable[]` entry beside it —
spells it `size`. Both kinds of record carry a `path`, so a consumer that walks
every object with a `path` key and reads `.size` gets a number for half of them
and `null` for the other half. This is not confined to `ls-files --remote --json`:
`verify --json` nests the same audit under each folder's `remote` key, so the
asymmetry is in that document too. Renaming either field would break a published
contract, so it stays and is documented rather than quietly corrected — read
`sizeBytes` on a listing row and `size` on an audit entry.

Remote presence is **absent unless you ask**: `--remote` adds the same batch
round trip `verify --remote` makes, because whether the server holds an object
cannot be known without asking it, and a listing that implied it did would be
guessing about the one thing worth being sure of.

**A present `remote` key does not prove a server was asked, and this matters if
you are monitoring.** `remote.missing == []` means *nothing keeper could ask
reported this object missing* — not *the server confirmed every object*. Three
folders produce a fully-formed, intact-looking audit with no round trip at all:
one whose `lfsMode` is `disabled`, one with no LFS-tracked pointers, and one whose
remote is a filesystem path with no `.lfsconfig` naming a server — for which there
is no server in the picture at all, and the peer's own `verify` is the answer. All
three are by design (see §8's division of labour and §9's note on the
filesystem-remote shortcut), and all three mean a wrapper reading an empty
`missing` list as "every object is safely on a server" is wrong about exactly the
folders where it would most like to be right.

**No field distinguishes them, and `checked` is not that field.** The first two
cases report `checked: 0`, which looks like the discriminator until you meet the
third: a filesystem remote counts every tracked path into `checked` and then
returns without asking anything. So a monitor that needs "a server said yes" has
to know the folder's `lfsMode` and the shape of its `remoteUrl`, both of which
`keeper-syncd list --json` reports on the profile row (the human `list` line
carries the remote but not the mode). Recorded rather than papered over: adding a
flag would change a published `--json` contract.

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
says `released` with the bytes this machine **no longer holds** — which is not
always the size of the file. A release is a `rename(2)`, and `rename(2)` replaces
one directory entry: a file with more than one hard link still has its content on
disk under the other name, so nothing was reclaimed and the figure is `0 B`
(`sizeBytes: 0` under `--json`). That is a real, successful release — the pointer
IS at that path afterwards — and the log line names the link count. On a platform
with no hard-link count the figure is the pointer's size.

It is the one verb here that can
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

**The profile selector is not a refusal, and does not exit `1`.** A first
argument that names no folder, or that names two folders sharing one name, is a
configuration error and exits **`2`** — before any of the gates above is
consulted, and on `materialize`, `dehydrate`, `pin` and `unpin` alike. So a
script that reads `1` as "keeper declined" and anything non-zero as "keeper
declined" will read a typo'd folder name as a refusal unless it separates the
two. An id always resolves to exactly one folder (it is the profile row's
primary key), so `2` from an existing folder means two of your folders share a
name and you should name the one you mean by its id.

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

**The automatic sweep never runs on a timer.** Nothing in the engine schedules
it — no schedule, no thread, no in-process timer: it rides the first successful
sync after the window expires, which
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

`materialize <id> <path> --for <duration>` is the third way a path's release
time is decided, between the folder's window and the pin: it records an instant
on that one path, which then stands in for `releaseTtlMs` in both directions
while the pin and the unconfirmed-authorship refusal both still outrank it. §9's
"Two release clocks, and which one applies" is the authority for how the three
compose, including what a `releaseTtlMs` of `0` does to a named deadline; this
section only notes that the sweep reading it is the same sweep, running the same
refusals.

Releasing on a schedule you choose, or from a button, is a `release` **task**
(§14): its three modes are `off`, `manual` and `scheduled`, `manual` is what
takes the sweep off the sync edge entirely, and with no release task stored
nothing above changes. No stored timestamp authorizes a deletion here either —
it only decides which paths are worth asking about.

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
resumably rather than being killed mid-write. `packaging/` also holds
`keeper-syncd-tasks@.service` and `keeper-syncd-tasks@.timer`, the pair that puts
one task on a schedule with no daemon and no app running at all — see §14.

Exit codes: `0` success, `1` operational failure, `2` configuration error,
`3` missing prerequisite (no usable git), and `4` the work did not run and that
is not a failure. `4` comes only from `tasks run` (§14); no other verb can
produce it, so nothing that already reads `$?` changes.

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

## 14. Tasks — named work with a schedule and a memory

A **task** is a piece of keeper's own housekeeping with a name, a schedule, and
a record of what happened. That last part is the reason it exists: periodicity
was never the missing piece — both hosts have run a 1 Hz tick with due-gates on
it for a long time, and `sync --once` has always been the documented `cron`
entry point — but nothing *remembered*. There was no name to ask about, no next
due time, and no last outcome, so no surface could honestly tell you whether
housekeeping had run. Non-execution of housekeeping is invisible by nature, and
that is what a task closes.

Every run is written down: when it started, when it finished, on which host, how
it ended, and one line of detail. The history is capped at **50 runs per task**
by the store itself, the same discipline the activity log follows, so a schedule
doing its job cannot grow `sync.db`. Unlike the activity log, which is
explicitly not a source of truth, this record **is** the answer to "when did
this last run, and what happened".

### The two kinds, and the one that will never exist

A task's `kind` is one of keeper's own verbs, never a shell string:

| kind | what one run does |
| --- | --- |
| `sync` | one full sync pass over the named folder, or over every enabled folder when the task is host-wide — the same `sync --once` body, taking the same per-folder reservation |
| `release` | one release sweep over the named folder, or over every enabled folder — the same body §9 describes, with every one of its refusals |

Both reuse the existing implementation rather than gaining a second one, which
is what makes "a task is not a privileged caller" true rather than promised: a
`release` task refuses exactly where `dehydrate` refuses, hashes the actual
bytes the same way, asks the server the same per-object question at the moment
of the deletion, and honours the pin, the per-file deadline and both budgets.

**`update` is not a task kind and never will be.** There is no such value to
write, and a hand-written row naming one is skipped as an unreadable kind rather
than honoured. The reason is §13's: the daemon holds a durable journal and can
be mid-push at any moment, so swapping its binary unattended is how a routine
release becomes a corrupted transfer. A schedule that installs software is the
one thing this whole mechanism is built to refuse.

A kind cannot be changed on a stored task, either. The armed window and the
whole run history belong to the kind that made them, so `tasks set nightly --kind
release` on a `sync` task is refused by name — forget it and create it again.

### The schedule, and what happens to one keeper cannot read

Three forms, and nothing else:

| form | meaning |
| --- | --- |
| `0 3 * * *` | a **5-field cron** expression: minute, hour, day-of-month, month, day-of-week. Each field is a comma-separated list of `*`, a single number, a `low-high` range, or either of those with a `/step` — `*/15`, `1-5/2`. A step on a single number (`3/2`) is refused as a contradiction, and a range never wraps (`5-1` is a mistake, not "17:00 through 01:00"). `7` is a second spelling of Sunday. Local wall-clock time. |
| `@hourly` `@daily` `@weekly` | exactly `0 * * * *`, `0 0 * * *` and `0 0 * * 0`. They desugar to cron rather than to intervals on purpose: `@daily` as "24 hours after whenever it was armed" would drift a nightly sweep to whatever time the host last restarted, and nightly would stop meaning night. |
| `every 30m` `every 2h` `every 1d` | a plain interval. Units are `s`, `m`, `h`, `d`, and the long spellings (`minutes`, `hours`, `days`) work too. |

Two things about the cron half are worth knowing before you rely on them. The
day rule is **vixie's**, reproduced deliberately: when both day-of-month and
day-of-week are restricted, a date matching *either* one fires — and whether a
field counts as restricted is decided by its **first character**, so `*/2` is
unrestricted and `1-31` is not, even though the range selects every day. A
dialect that agreed with cron on the easy fields and diverged on the day rule
would be worse than one that never claimed the name. And month and weekday
**names** (`JAN`, `MON`) are deliberately absent: the grammar is small, and a
name nobody parses is worse than a refusal that says so.

`every <n>` measures from the **end of the previous run**, not from a fixed
origin. A task whose pass takes ninety seconds on `every 1m` therefore fires
about every two and a half minutes. That drift is the deliberate choice: a fixed
origin would make a task that overran come due the instant it finished, which
over a git repository is a worse answer.

**A schedule keeper cannot parse is refused when it is saved, with the
expression quoted.** Not coerced, not clamped, not accepted and quietly ignored
— refused at the write door, where the person who typed it is standing. Four
refusals:

- it is not one of the three forms above;
- it fires **more often than once a minute** — `every 30s` parses and is then
  refused by the floor, so you are told about the floor rather than about a unit
  keeper supposedly does not understand. Below a minute a scheduler spends more
  time waking than working;
- it fires **less often than once a year** — reachable only from `every`, where
  `every 100000000d` multiplies cleanly and arms a window in the year 275 000.
  Write a calendar pattern instead (`0 0 1 1 *`), which the cron half expresses
  exactly and is not subject to this ceiling;
- it **matches no instant at all** — `0 0 30 2 *` parses as five valid fields
  and names a date that does not exist.

Those last two matter more than they look. A schedule that parses to "never"
while its row reports itself enabled is precisely the invisible failure this
chapter exists to prevent, and it is the only failure mode a scheduler has that
nobody notices.

### The verbs

```
keeper-syncd tasks list                       # every task, its target, schedule and last run
keeper-syncd tasks status <task>              # one task and its recorded runs, newest first
keeper-syncd tasks run <task>                 # run one now, and exit with what happened
keeper-syncd tasks set <task> [flags]         # create one, or change one that exists
keeper-syncd tasks enable <task>              # put it back in service
keeper-syncd tasks disable <task>             # take it out of service, forgetting nothing
keeper-syncd tasks forget <task>              # delete it and its whole run history
```

`list` is the only one that addresses no task; every other verb acts on exactly
one row, and a missing selector is told rather than read as "all of them" — for
`run` and `forget` that would be a fan-out nobody asked for. A selector gets one
of three answers, and they are three different pieces of advice: a spelling
keeper could never have stored (leading or trailing whitespace, or empty) is
quoted back; a well-formed id with no such row lists what *is* stored; and a row
this build cannot read says so with its reason rather than claiming no match.

`tasks set` takes six flags:

| flag | effect |
| --- | --- |
| `--kind sync\|release` | required to **create**; kept as stored on update, and refused if it names a different kind |
| `--profile <SEL>` | bind the task to one folder, by id or name |
| `--host-wide` | unbind it: the task belongs to the machine, not to one folder |
| `--schedule <EXPR>` | the expression above, validated here |
| `--no-schedule` | forget the schedule — `--schedule`'s inverse |
| `--mode off\|manual\|scheduled` | who may trigger it |

On update **every flag you omit keeps its stored value**, so changing a schedule
cannot unbind a folder by not mentioning it. The two clearing flags exist
because a knob you can set but never unset is a dead knob. On create, `--mode`
defaults to `scheduled` when you gave a `--schedule` and `manual` when you did
not — defaulting to `scheduled` unconditionally would make `tasks set nightly
--kind sync` fail at the write door for a reason the caller never mentioned.

`mode` and `enabled` answer different questions and both are read. **`mode` says
who may trigger the task** — `off` refuses everyone including an explicit
request, `manual` runs only when asked, `scheduled` is the only one that is ever
*due*. **`enabled` says whether the row is live at all**, and `tasks enable` /
`tasks disable` are the only things that write it: `tasks set` never touches it,
so a settings-shaped read-modify-write cannot silently un-pause a task somebody
paused. A disabled `scheduled` task keeps its schedule and resumes on it, and
bringing a task back into service arms its schedule **afresh** rather than firing
a window that fell into the past while it was out of service.

### Exit codes, which is what `tasks run` is for

`tasks run` exists to be called from a wrapper that branches on `$?`, so the
numbers are the contract:

| code | meaning | what a wrapper should do |
| --- | --- | --- |
| `0` | the work ran and did what it was asked to | nothing |
| `4` | the work did **not** run, and nothing is wrong: the drive is unplugged, the folder is paused, the folder was already syncing, or another host holds this task's lease. Recorded as `deferred` or `busy` | **nothing — do not alert** |
| `1` | the work ran and failed, or a stored outcome this build cannot read was recorded. The reason is in the run's `detail` | alert; a retry may work |
| `2` | the selector is wrong, or the task is off or disabled | fix something; retrying changes nothing |
| `3` | a prerequisite is missing — in practice, `git` | install it |

`4` is the one that did not exist before tasks did, and it is worth the number.
Folding a deferral into `0` would make an external drive unplugged for a month
indistinguishable from a nightly sweep that is working. Folding it into `1` would
page somebody every night for §6's *an unplugged volume is absence, never
failure* — and an alert that fires nightly for a normal condition is an alert
nobody reads. Nothing that worked before returns `4`: it is reachable only from
`tasks run`.

A deferred or busy run does not eat its slot. Whatever window was armed is
replaced by the sooner of *a minute from now* and the next scheduled instant, so
the sweep happens in the minute after the drive comes back rather than tomorrow
night.

**That one-minute retry and the fifty-run cap interact, and the interaction can
cost you the record.** A `scheduled` task whose condition stays unmet — a
removable folder whose drive is out — defers, re-arms a minute later, defers
again, and writes a row every minute. Fifty minutes of that evicts everything
older, so `tasks status` a month later shows fifty identical `deferred` rows from
the last hour and no trace of the last successful sweep. If you need the history
to survive a long absence, `tasks disable` the task while the drive is away, or
drive it from the timer with `--mode manual` so the cadence is the timer's and
nothing re-arms a minute later. This is a real limit of a bounded log, not a
fault: the cap is what stops a schedule doing its job from growing `sync.db`.

**`tasks run` does not check whether the task is due.** It performs the work,
every time, which is what makes a `cron` entry, a systemd timer and a person at
a prompt behave identically — and what makes the timer's own cadence the one that
matters for that driver (see below). A schedule window still in the future is
left alone, so a run now is not a request to skip tonight's; a window that was
**already open** when the run finished has just been served, and is re-armed to
the following instant rather than firing again on the next tick.

The five recorded outcomes are `ok`, `busy`, `deferred`, `failed` and
`abandoned`. `abandoned` is written *by the next host*, when it reclaims a lease
whose holder never closed its run — so a killed process leaves a closed record
rather than a wedged row.

### `--json`

The global `--json` flag works on either side of the subcommand and makes the
output a document whose field names are the contract. camelCase throughout,
matching `sizeBytes` and `profileId` elsewhere in this document.

`tasks list` emits `{ "tasks": [...], "unknown": [...] }`. Both keys are always
present, the empty array included — an absent `unknown` would make "this build
can read everything stored" indistinguishable from an older consumer's document.
Each `unknown` entry is `{ "id", "reason" }`.

A task carries eleven keys, always all eleven:

```json
{ "id": "nightly", "kind": "release", "mode": "scheduled", "enabled": true,
  "profileId": null, "profile": null, "schedule": "0 3 * * *",
  "nextDueMs": 1764000000000, "runningHost": null, "leaseUntilMs": null,
  "lastRun": { "id": 7, "taskId": "nightly", "startedMs": 1763913600000,
               "finishedMs": 1763913604000, "outcome": "ok",
               "detail": "released 3 paths (5242880 bytes) from 1 folders, 0 declined, 0 already syncing, 0 unavailable",
               "host": "01J8ZQ…#48213" } }
```

A task's `runningHost` and a run's `host` are the same string,
`<device-id>#<pid>`. The device id alone would not do — on Linux the daemon and
the app can share one data directory's `sync.db` and therefore one device row —
and the pid is also the only part a person can check against a process list to
see whether the holder is still alive.

`profileId` and `profile` are `null` rather than absent, because here `null` is a
**real value**: a host-wide task belongs to the machine rather than to a folder,
and that is an answer. A `profileId` that is *set* while `profile` is `null` is
the other case — a task bound to a folder that is gone.

A run carries seven keys, plus `unknownOutcome` **only** when the store held a
spelling this build cannot read. That conditional key is the whole contract of
the run document: `outcome: null` alone cannot separate "still in flight" from "a
newer keeper wrote something we cannot read", so the presence of the second key
is the signal, which is exactly why it is absent rather than null in the first
case. `startedMs` and `finishedMs` are absolute instants, never countdowns — a
countdown is stale the moment it is serialized.

The other envelopes: `tasks status` emits `{ "task": …, "runs": [...] }`;
`tasks run` emits `{ "task", "run", "outcome", "exit" }`, where `run` is `null`
when a lease held elsewhere meant no run of ours was ever opened, and `exit`
repeats the process status so a caller that captured stdout need not consult two
channels; `tasks set`, `tasks enable` and `tasks disable` all emit
`{ "task": … }` — the row **read back from the store**, never the row that was
submitted; and `tasks forget` emits `{ "forgot": "<id>" }` and nothing else,
because echoing the fields of something that no longer exists would invite a
consumer to believe it still does.

### The release task, and its three modes

§9's release sweep rides the first successful sync after a window expires. A
`release` task is a **second driver** for that same sweep, and the mode on the
task decides how the two relate:

| release task | the success edge (§9) | `tasks run <id>` |
| --- | --- | --- |
| **none stored** | runs, exactly as before | — |
| `off` (or `enabled = 0`) | does not run | refused: an "off" that still runs when asked is not off |
| `manual` | **does not run** | runs |
| `scheduled` | still runs | runs |
| stored but **unreadable** by this build | **runs** — see below | refused: the row cannot be selected either |

**The first row is the important one: with no release task stored, nothing about
§9 changes.** That is the arm an un-migrated `sync.db` takes, and it is why
upgrading into this feature changes nothing at all about when content is
released. `scheduled` *adds* a driver rather than replacing one — somebody who
put the sweep on a schedule did not thereby ask for less housekeeping than they
had — while `manual` is the ask this feature was built for: *deletion does not
have to be automatic, it can be a script run at the right time*.

A folder's own release row outranks a host-wide one, and where several apply the
least permissive wins. And if the task table cannot be read **at all** — the
other host holding a write lock, say — the sweep declines that pass rather than
guessing that nobody switched it off, because the honest answer to "may I delete
content" when the governing instruction is unreadable is no.

**The last row fails the other way, and it is worth knowing before you rely on an
`off` row.** A whole unreadable *table* declines; a single unreadable *row* does
not. Governance is folded over the rows this build could parse, and a row whose
`kind` or `mode` a newer keeper wrote is not among them (NFR-43 lists it as
unknown instead), so governance resolves to *nothing stored* and the §9 success
edge sweeps exactly as if the row were absent. This is reachable: it is the same
forward-compatibility case §14 describes above under *unhosted*, and it arises
when two keeper versions share one `sync.db` or when one host is downgraded. If
you are switching automatic release **off** and two versions are in play, verify
with `keeper-syncd tasks list` on the **older** binary that the row appears under
`tasks` and not under `unknown` — a row it lists as unknown is a row it will not
honour.

One more release-task answer is worth knowing before it surprises you: a run
where **nothing looked** is normally recorded `deferred` and exits `4`, not `0`.
That covers a folder whose `releaseTtlMs` is `0`, one whose large-file support is
off, one whose folder configuration is faulted, one whose own release row says
`off`, and a host whose every folder is paused. Counting those as swept would have
made *released 0 paths from 10 folders* mean either "ten folders had nothing due"
or "ten folders refused", and the operator who most needs that difference is the
one who asked for a run and got silence. The **one** exception is a host with no
folders configured at all: that is `ok` and exit `0`, deliberately, because a box
with nothing to sweep has nothing wrong with it and never will until somebody adds
a folder — so a wrapper that treats `0` as "the sweep is working" should also be
watching that the box still has the folders it is supposed to have.

### Which host actually runs a task — the platform asymmetry, stated once

This is the part to read before setting a schedule, because the two platforms
are genuinely not the same and the difference is load-bearing rather than a gap
somebody will close next month.

**On Linux there is a real background host.** `keeper-syncd watch` evaluates due
tasks on the supervisor tick it already runs, so with the systemd user service
enabled **and lingering turned on** tasks run with nobody logged in. Lingering is
not optional and not a nicety: a `--user` unit is stopped when its user's last
session ends, so without `loginctl enable-linger $USER` the daemon dies at logout
and its schedule stops with it. A systemd **timer** calling `keeper-syncd tasks
run` is the second way, needs no daemon at all, and needs lingering for exactly
the same reason — see the pair below.

**On macOS keeper ships no background host for the daemon.** A
`keeper-syncd-aarch64-apple-darwin` binary *is* published (§13), so its verbs run
there by hand or from a `cron` entry you write yourself — but keeper ships **no
launchd agent**, so nothing starts `watch` and nothing triggers `tasks run`
unless you arrange it. The host keeper does provide is the desktop app, and it is
a real one: closing the window hides it and keeps the process, the engine and its
notifications alive, and launch-at-login exists. But **quit means quit** — a task
runs only while keeper is running, and the Tasks view says so rather than
implying a schedule that cannot fire.

A task that **no present host can run** reads **unhosted**, with the reason,
rather than *enabled and quiet*. Exactly three states produce it: a task bound to
a folder keeper no longer syncs; a `scheduled` task with no schedule stored; and
a task whose `mode` is a spelling this build does not understand, which is what a
newer keeper on the other host can write. A task that is `off` or disabled is
**never** unhosted, whatever else is wrong with it — that gate is asked first,
because raising an alarm about a row somebody deliberately silenced is noise. A
`manual` task is not unhosted either, and reads *on request* — unless its folder
is gone, which defeats being asked as thoroughly as it defeats a schedule, so
that gate sits above every mode gate. A reason is attached to the unhosted
verdict and to nothing else, so a reason present at all **is** the alarm.

In the app, ⌘8 opens **Tasks**: every row states its kind, schedule, the host
that will actually run it, its next due time, its last run and its last outcome,
and offers **Run now**. A failure notifies **once per onset** — on the
`healthy → failing` edge, not once per attempt — which is the engine's existing
rule and exists because a text-keyed version of it once produced thousands of
notifications an hour.

One honest limit on that view, and one thing it does check that you might expect
it not to. The limit: its Linux daemon verdict is computed from
`keeper-syncd.service` being enabled **and** resolving the same data directory,
which by default it does not (`~/.local/share/keeper-sync` against
`~/.local/share/dev.tgorka.keeper`) — that case gets its own sentence rather than
being reported as a working daemon. But a box driven **only** by the timer pair
below has no `keeper-syncd.service` enabled at all, so the app names itself as
the host for those tasks: the timer still runs them, and the view does not know
the timer exists.

And the verdict **does** check lingering, so *logged in or not* means it. There
are two daemon sentences, not one, because `systemctl --user is-enabled` answers
*wanted at login* and not *survives logout*:

- lingering on — *"the keeper-syncd unit on this machine runs this, logged in or
  not"*
- lingering off — *"the keeper-syncd unit on this machine runs this while you are
  logged in — lingering is off, so its schedule stops when your session ends"*

The app asks by stat-ing `/var/lib/systemd/linger/$USER`, which is not a guess
at the answer `loginctl show-user "$USER" --property=Linger` would give: it is
the same answer. That property's getter in logind is one call to
`user_check_linger_file`, whose entire body is `access()` on that path, and
`loginctl enable-linger` is what creates the file. A machine with no systemd at
all has no such file and no `systemctl` either, so it reads as *no daemon* and
never reaches a daemon sentence. If neither `$USER` nor `$LOGNAME` is set the app
cannot form the name and reports the session-only sentence — under-claiming the
daemon's reach rather than promising a run that will not happen.

### Exactly one runner, whoever asks

A task row carries `runningHost` and `leaseUntilMs`, claimed in the same single
`UPDATE` that opens the run — the claim the transfer journal already uses — so
two supervisors cannot take one row. This matters on Linux, where the daemon and
the app can both be running against one `sync.db`. The lease lasts **one hour**
and an expired one is reclaimable, so a killed host does not wedge a task
forever; the reclaiming host closes the orphaned run as `abandoned`. A
`keeper-syncd tasks run` that meets a live lease exits `4` and records `busy`:
nothing failed, and nothing happened either.

A host that quits mid-run hands back the leases of the tasks it is **not**
running and leaves the one it is. That lease then expires the ordinary way,
which is why the expiry rule above is the floor and not the exception —
releasing it early would let the other host open a second concurrent run over
the same git tree.

A task never holds a git index concurrently with its host's own sync pass. Both
kinds take the very reservation the sync tick takes, so that is a structural fact
about the code rather than a convention — which is also what makes exit `4`'s
"the folder was already syncing" reachable rather than aspirational.

### The systemd timer pair — a schedule with no app and no GUI

Two files ship beside the daemon's own unit, in `packaging/`:

```
keeper-syncd-tasks@.service    # a oneshot: `keeper-syncd tasks run %i`
keeper-syncd-tasks@.timer      # the trigger
```

Both are **templates**, and the instance name is the task id — the same string
`tasks list` prints — so one pair of files drives every task on the box:

Both need **systemd 244 or newer**. The service sets `Restart=` on a
`Type=oneshot` unit, which systemd refused outright until v244 relaxed the rule;
before that, `daemon-reload` logs *"Service has Restart= setting other than no,
which isn't allowed for Type=oneshot services. Refusing."* and the unit never
loads, so the timer fires every night onto nothing. Debian 11, Ubuntu 20.04 and
RHEL 9 are all newer; Debian 10, Ubuntu 18.04 and RHEL 8 are not. On one of
those, delete the unit's `Restart=`/`RestartSec=` pair and let the timer's next
trigger be the retry — everything else in both files is older than v244.

Run this from the repository root, where `src-tauri/` is the cargo workspace:

```
# 1. the binary, and the two units
install -Dm755 src-tauri/target/release/keeper-syncd ~/.local/bin/keeper-syncd
cd src-tauri/crates/keeper-syncd/packaging
install -Dm644 keeper-syncd-tasks@.service ~/.config/systemd/user/keeper-syncd-tasks@.service
install -Dm644 keeper-syncd-tasks@.timer   ~/.config/systemd/user/keeper-syncd-tasks@.timer
systemctl --user daemon-reload

# 2. lingering — REQUIRED, not optional. Without it every --user unit on this
#    box, this timer included, stops when your last session ends.
loginctl enable-linger "$USER"
loginctl show-user "$USER" --property=Linger     # must print Linger=yes

# 3. the task
keeper-syncd tasks set nightly --kind release --mode manual

# 4. the cadence. NOTE THE EMPTY ASSIGNMENT: OnCalendar= is a LIST, so a
#    drop-in that only adds a value leaves the shipped `daily` in place and the
#    timer then elapses TWICE. The empty line clears the list first.
systemctl --user edit keeper-syncd-tasks@nightly.timer
#   [Timer]
#   OnCalendar=
#   OnCalendar=*-*-* 03:00:00

# 5. enable the TIMER, then read back what systemd actually resolved
systemctl --user enable --now keeper-syncd-tasks@nightly.timer
systemctl --user list-timers 'keeper-syncd-tasks@*'
systemctl --user show keeper-syncd-tasks@nightly.timer -p TimersCalendar

# 6. prove it end to end, without waiting for 03:00
systemctl --user start keeper-syncd-tasks@nightly.service
keeper-syncd tasks status nightly
```

Step 2 is the one people skip, and skipping it produces a schedule that works
until you log out and silently never again. **Step 4's empty `OnCalendar=` is the
other one**, and it is the more expensive: forget it and a `release` task deletes
content at midnight as well as at the hour you chose, which nothing at setup time
reveals — `list-timers` shows only the *next* elapse, which is why step 5 also
reads `TimersCalendar` back, and that is the line that shows both entries if you
left both. Step 6 is worth doing once: it exercises the real unit, and
`tasks status` then shows the run it produced, with its outcome.

Enable the **timer**, never the service. The shipped service has no `[Install]`
section at all, so `systemctl --user enable` on it answers *"The unit files have
no installation config"* rather than doing something subtly wrong — that absence
is deliberate, because a timer-driven oneshot that *could* be enabled would run
once at login and never again, which looks like it worked.

**Not every task id can be a systemd instance name.** keeper stores any id that
is not empty and not padded with whitespace, so `night sweep`, `sync@home` and
`réveil` are all valid tasks; a systemd unit name admits only ASCII
alphanumerics and `: - _ . \`, with `@` as the instance separator, and caps the
whole name at 256 bytes. Anything outside that is unusable here: `systemctl`
refuses the name, or you reach for `systemd-escape` and — because the unit passes
the instance name through verbatim, which is what keeps ordinary hyphens working
— keeper receives the escaped spelling (`night\x20sweep`) and answers *no such
task*, exit `2`. That refusal writes **no run row**, so it is invisible to
`tasks list`, `tasks status` and the Tasks view. Choose a timer-driven task's id
when you create it: keeper has no rename verb, and `tasks forget` followed by
`tasks set` throws away the run history that is the whole point of a task.

**The timer's `OnCalendar` is the cadence for this driver, so set it to when you
want the work to happen.** This is the one thing about the pair that is easy to
get wrong, and it is worth being blunt: `tasks run` **performs** the task every
time it is called and never consults the task's `schedule` column. Asking for a
run now is not asking whether a run is due — that is `tasks run`'s whole
contract, and it is what makes a `cron` entry and a person at a prompt behave
identically. The `schedule` column is read by the **in-process** hosts: the
`keeper-syncd watch` daemon's tick and the desktop app's. So an hourly trigger on
a task whose schedule reads `0 3 * * *` does the work twenty-four times a day,
not once. The shipped default is `OnCalendar=daily`, deliberately conservative,
and step 4 above is how you change it without editing a shipped file.

**Two more directives move a run away from the instant you wrote, and both are
shipped on.** `RandomizedDelaySec=3600` spreads the wake over an hour, so an
`OnCalendar=*-*-* 03:00:00` drop-in actually fires at a uniformly random point
between 03:00 and 04:00 — `list-timers` showing a NEXT column up to an hour later
than you typed is the timer working, not a fault. It exists because a fleet of
boxes all hitting one forge at exactly the same second is a self-inflicted
thundering herd; lower it if a folder is only attached during a narrow window,
because a run whose jitter pushes it outside that window defers instead. And
`Persistent=true` means a trigger missed while the machine was off fires **once**
when it comes back rather than waiting for tomorrow — so a laptop shut overnight
runs its release sweep after boot, not at 03:00. That catch-up is jittered too.
Both are worth keeping and neither is worth discovering as a surprise deletion.

Which `--mode` to pair with the timer follows from that:

| you are running | give the task | why |
| --- | --- | --- |
| the timer only, no `watch` service and no app | `--mode manual` | nothing schedules the task, so the timer is the whole cadence and there is exactly one to read |
| the timer **and** `keeper-syncd watch` | `--mode scheduled` with a schedule | both drivers run and the lease keeps them from overlapping, but **two** things move the daemon's next window and neither is obvious. If that window was already open when the timer's run finished, it has just been served and is re-armed to the following instant. And if the run recorded `busy` or `deferred` — the folder was mid-sync, the drive was out — that arm is taken *before* the trigger is even consulted and rewrites the window to the sooner of the next scheduled instant and one minute from now: a timer run that did nothing at 00:00 can leave the daemon sweeping at 00:01 instead of at the 03:00 you chose. Give the timer the coarser cadence, or prefer `--mode manual` above and keep one cadence in one place |
| neither, for now | `--mode off` | every trigger is refused, this one included, and the unit exits `2` without retrying |

One cosmetic wrinkle in the `manual` arrangement, so it does not read as a
contradiction. The window arithmetic that re-arms a `busy` or `deferred` run does
not consult the mode, so the first timer-driven run that defers writes a
`nextDueMs` a minute ahead onto a `manual` task — and the Tasks view and
`--json` will both show a next-due instant for a task that has no schedule.
Nothing acts on it: the due-gate answers *nothing to do* for any mode but
`scheduled`, so the task still runs only when the timer asks. Read the timer, not
that field, as the answer to *when will this happen* on a manual task.

What stays in keeper either way is everything keeper can be asked about: the
task's name, kind, target, mode, its whole run history, and the schedule the
in-process hosts honour. Putting a cadence in a unit file *instead of* a task —
no row at all, just a timer — is what AD-136 rejected, because then `tasks list`
and the Tasks view would have nothing to show and nothing to report on.

The unit honours the exit taxonomy twice over, because systemd asks two separate
questions. `RestartPreventExitStatus=2 3 4` answers *should this be retried*: a
configuration error, a missing `git` and a deferral are the three numbers a
restart cannot help, and a `4` retried every minute is that nightly alert nobody
reads. `SuccessExitStatus=4` answers *was this a failure*, and it is the line
that matters more than it looks: `RestartPreventExitStatus` suppresses only the
restart, so without it every night an external drive is out would end with the
instance in `failed` — red under `systemctl --user status`, listed in
`systemctl --user --failed`, firing any `OnFailure=` hook on the box. That is
systemd raising the very alert the exit code exists to prevent, and it would also
make step 6 of the install above print *"Job for
keeper-syncd-tasks@nightly.service failed"* at the moment it is meant to prove
the install works. `2` and `3` stay genuine failures. `1` stays restartable,
bounded by `StartLimitBurst=3` within `StartLimitIntervalSec=600`, after
which the instance waits for the timer's next trigger. The daemon unit's stop
contract is deliberately **not** copied: `watch` installs a SIGTERM handler and
finalizes under a 10 s bound, `tasks run` installs none — it does not need one,
because a task is idempotent and safely abandonable, and a run killed mid-flight
is closed as `abandoned` by the next host to reclaim its lease.

**One overlap keeps no record at all, and it is the only one that does not.** A
held lease records `busy`; a held per-folder reservation records `busy`; but a
trigger that arrives while the previous run is *still going* is not a run at all
— systemd merges the new start job into the in-flight one, nothing is spawned,
and the timer's own stamp advances so `Persistent=` will not catch it up either.
No `task_runs` row is written, so that skipped night is invisible to `tasks
list`, `tasks status`, `--json` and the Tasks view alike. The unit sets no
`TimeoutStartSec` on purpose — a host-wide sync task's duration is not knowable
from a unit file, so any number written there would eventually kill real work —
which means the defence is yours: **keep the cadence coarser than the work.** If
a task can take more than an hour, do not trigger it hourly, and check
`systemctl --user status keeper-syncd-tasks@<id>.service` if a night looks
missing from the history.

A plain `cron` entry is the same thing with less ceremony, and reaches the same
code path:

```
0 3 * * *  /home/you/.local/bin/keeper-syncd tasks run nightly
```

A timer, a `cron` entry, the daemon's tick and a person at a prompt all take the
same lease, walk the same refusals and leave the same row in the history. There
is no privileged caller.

**No launchd agent ships, and that is a recorded gap rather than an oversight.**
A macOS `keeper-syncd` binary is published, and the same one-shot verb works
there — a `cron` entry in your own crontab reaches the identical code path. What
keeper does not ship is a plist that starts anything, so on macOS the desktop app
is the host keeper provides, with the limits stated above. `docs/decisions.md`
D-3 names a launchd agent as the revisit trigger.

---

## 15. Making a folder visible to agents

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

## 16. Security posture

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

## 17. Troubleshooting

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
| Authentication rejected, but `git` works in that folder by hand | Keeper uses the credential stored against the profile and nothing else — plain `git` is reading your OS credential store, which keeper deliberately ignores (§16). Add the token to the profile. |
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

## 18. Current implementation status

This document describes the designed behaviour. As of 2026-08-25 the engine and
the `keeper-syncd` daemon implement and verify §§1–8, §10, §11 and §13 against
real git remotes, including a full LFS round trip (upload, peer clone, download,
materialize) against a local LFS server and the review-lane airlock. Virtual
files (§9) are real and exercised end to end: the excuse the policy gives
`verify`, the `ls-files` inventory, `materialize` on demand, the four row states
with the sentences and glyphs that carry them, `pin`/`unpin`, and the release
deadline a materialized row counts down. A counting row needs an unpinned row, a
non-zero `releaseTtlMs`, content the remote is known to hold — for bytes this
clone authored, a `synced_at_ms` that is not NULL — and a folder that authorizes
that path to stay away: `lfsMode = pointerOnly`, or the default `materialize`
with the folder's own policy resolving the path to virtual, a bare
`virtualOverBytes` floor included since story 56.16. A row whose folder keeps it
answers with a word and no instant instead. **Tasks (§14) are real** as of
2026-08-30 — the record, the dialect, the due-gate on each host's existing tick,
the seven CLI verbs with their exit taxonomy, the release task's three modes, the
⌘8 view and the systemd timer pair — with the platform limits §14 states rather
than a gap here. Two parts are not reachable everywhere:

- **§12 progress and warnings.** These are engine-side and correct — the tray
  decision, the status line and the warning onset logic are implemented and
  tested — but the desktop app surfaces that render them are not wired up.
- **Releasing content (§9), on macOS and Windows.** `dehydrate` and the release
  sweep are implemented, covered by tests, and **live on Linux**: the daemon and
  the desktop app there both answer the open-file question from `/proc` by inode
  identity, so a folder that authorizes something really does release. What
  gates reaching that question is the folder's policy and not its mode alone —
  `pointerOnly` passes always, `materialize` passes for the paths its policy
  authorizes, a bare `virtualOverBytes` floor included since story 56.16, and
  only a folder that authorizes nothing at all is refused `AlwaysMaterializes`
  before the question is reached. macOS and Windows still cannot answer that
  question without racing, so both refuse `OpenUnknown` there. Nothing releases
  content on a macOS or Windows machine until that platform can answer the
  question.
- **A packaged background host, on macOS.** Tasks (§14) run there only while the
  desktop app is running: a `keeper-syncd` binary is published for macOS and its
  one-shot verbs work, but keeper ships no launchd agent, so nothing starts
  `watch` and nothing triggers `tasks run` unless you write a `cron` entry
  yourself. On Linux both the daemon and the shipped timer pair are real hosts,
  and both need `loginctl enable-linger` to survive logout.

## 19. Measured envelopes

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

## 20. Deliberate limitations

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
  deliberate; see §13. It is also **not a task kind and never will be** (§14):
  keeper does ship a timer unit now, and `update` is precisely the one verb it
  can never be pointed at.

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
