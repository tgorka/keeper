# Epic 33 — Add a folder where you are, and copy files once, verified

status: draft
created: 2026-07-28
altitude: epic
parent: Epic 32 (sync visibility), AD-40..AD-53 (Phase 4 folder sync spine)

## Why this epic exists

Two gaps, one shared theme: moving bytes should be doable from where you are, and provable
afterwards.

1. **Adding a folder still means leaving.** Epic 32 gave sync its own view, but the only way to
   set a folder up is Settings → Sync. The view that exists to answer "what is sync doing" cannot
   answer "start doing it", so the first thing a new user does there is bounce to another surface.

2. **There is no way to just copy something, once.** keeper knows how to move bytes with integrity
   — it streams with bounded buffers, hashes while writing, and publishes by rename only after the
   digest matches (`lfs/store.rs`, `stability.rs`) — but that machinery is reachable only by
   setting up a *sync relationship* with a git remote. A user who wants "put this folder over
   there, and tell me it arrived intact" has to use `cp` and trust it.

The second is not a convenience wrapper around `cp`. The point is the part `cp` does not do:
re-read what was written and prove it matches, per file, and say so.

## Decisions

**AD-C1 — A copy is a job, never a relationship.**
Binds: the model. Prevents: a one-time copy quietly becoming a second kind of sync. Rule: a job has
a lifecycle (`queued → copying → verifying → done | failed | cancelled`) and a per-file report. It
is never written into `profiles`, never joins the journal, and finishing it changes nothing about
the folders it touched.

**AD-C2 — Copied means re-read and matched, not written.**
Binds: what "done" claims. Prevents: the universal lie of copy tools — that a successful `write()`
means the bytes are correct on the other side. Rule: hash the source while streaming it out, then
read the destination file back and compare. A file only counts as copied when the destination's own
digest matches what the source produced. Verification is not an option to switch off; a copy that
cannot be proven is a failure.

**AD-C3 — Nothing partial is ever visible at the destination.**
Binds: crash behaviour. Prevents: a truncated file that looks finished. Rule: stream into a temp
file in the destination's own directory (so the rename is atomic on the same filesystem), publish
by rename only after the digest matches, and delete the temp on any failure — the discipline
`lfs/store.rs` already uses.

**AD-C4 — An existing destination is never silently overwritten.**
Binds: collisions. Prevents: the classic tool that eats the newer file. Rule: compare digests
first. Identical → skipped and reported as identical, which is honest and fast. Different → left
untouched and reported as a collision, unless the job was explicitly created with `replace`, in
which case the old bytes are replaced only after the new ones are verified.

**AD-C5 — Memory is bounded by a constant, not by the largest file.**
Binds: scale. Prevents: a 50 GB file being a 50 GB allocation. Rule: the same chunked streaming as
the rest of the crate (`HASH_CHUNK_BYTES`), so peak RSS is a buffer regardless of file size —
matching the measured 2 GiB-in-17.5 MiB envelope the LFS path already holds.

**AD-C6 — The report is per file, with a reason.**
Binds: the result surface. Prevents: "42 files copied" hiding three that were skipped and one that
collided. Rule: every entry is `copied | identical | collision | failed{reason}`, and the summary
is derived from the entries rather than counted separately.

**AD-C7 — Add-a-folder lives in both places, from one component.**
Binds: the form. Prevents: two add-folder forms drifting apart. Rule: the existing Settings form is
extracted into a shared component and rendered in both surfaces; the Sync view offers it inline for
its empty state and behind an "Add a folder" action once folders exist.

## Stories

**33.1 — The copy engine.** `keeper-sync/src/copy.rs`: walk a source (file or directory), stream
each file through a hasher into a destination temp, verify by re-read, publish by rename, and
return a per-file report. Cancellable. Bounded buffers.
AC: a copied tree matches byte-for-byte and every entry says `copied`; a destination that already
holds identical bytes reports `identical` and is not rewritten; a differing destination is
untouched and reports `collision`; a mid-copy cancel leaves no temp file and no partial
destination; a source that changes under the read fails that entry rather than publishing it.

**33.2 — Copy over IPC.** `copy_start`, `copy_status`, `copy_cancel`, and a progress stream. Jobs
are keyed by an opaque id and live in app memory.
AC: a job reports rising progress and a terminal state; cancelling stops it promptly; the report
survives long enough to be read after completion.

**33.3 — The copy surface.** A "Copy files once" card in the Sync view: source and destination
pickers, a replace-existing choice defaulting to off, a live progress line, and the per-file result
list grouped by outcome.
AC: a copy of a small tree shows every file's outcome; a collision is visible and explains that
nothing was overwritten; the surface never claims a total it does not have.

**33.4 — Add a folder from the Sync view.** Extract the Settings add-profile form into a shared
component and render it in the Sync view — inline in the empty state, behind an action otherwise.
AC: a folder added from the Sync view appears there immediately and in Settings; the two forms
cannot drift because there is only one.

## Deferred

- Moving (copy-then-delete): deleting the source is a different risk class and wants its own
  confirmation story.
- Preserving extended attributes, resource forks and ACLs: `ditto` territory; this copies bytes,
  timestamps and the executable bit, and says so rather than implying more.
- Resuming an interrupted copy mid-file: a re-run re-copies the unfinished file, which is correct
  and simple; resume matters for multi-gigabyte single files over a slow link, which is not this.
