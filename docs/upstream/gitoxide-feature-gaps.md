# gitoxide: what keeper still needs the `git` binary for

A feature-gap report for [GitoxideLabs/gitoxide](https://github.com/GitoxideLabs/gitoxide),
written from one consumer that is trying to have no `git` dependency at all.

Every gap below is something keeper shells out for **today**, in
`keeper_sync::git::cli`. Each entry names the exact argument vector keeper
builds, what gitoxide already provides, what is missing, and an implementation
estimate with the reason for it. The four items in group A are the ones that
force the dependency to exist; group B is what keeper can delete on its own side
because gitoxide already covers it (filed here because "the CLI shim has twelve
subcommands in it" reads worse than it is).

**Versions read for this report:** `gix 0.86.0` / `gix-filter 0.33.0` (the
release keeper pins) and `origin/main` at `a1d5a5520`. Claims about main are
from the source in that revision, not from documentation.

**Why this matters to a consumer, in one line:** invoking `git` is not linking
it, so a GPLv2 binary on `PATH` does not contaminate an MIT/Apache app — but it
does mean shipping a desktop application that silently depends on a program the
user may not have, at a version keeper has to floor-check (`>= 2.42`), whose
hooks have to be neutralised on every call (`-c core.hooksPath=/dev/null`), and
whose failure text has to be parsed to produce a diagnosis. Each gap closed
deletes a class of field failure, not just a function.

---

## Group A — the gaps that keep the `git` dependency alive

### A1. `push` — send-pack over the wire

**What keeper runs**

```
git -c credential.helper= [-c credential.helper=<helper>] \
    push --porcelain [--force] -- <remote> <refspec>
```

**What gitoxide has.** Everything except the protocol's writing half.
`gix-refspec` parses push refspecs (`gix-refspec/tests/refspec/parse/push.rs`),
`gix/src/push.rs` on main models the `push.default` config values, `gix-protocol`
has the full v0/v1/v2 transport and `fetch` negotiation, `gix-pack` can *write*
packs, and `gix-transport` already speaks the `git-receive-pack` service name.

**What is missing.** `send-pack` itself: advertise-refs on the receive-pack
service, the command list (`old new ref`), report-status / report-status-v2
parsing, and the pack generation for exactly the objects the remote lacks.
`git grep -l send.?pack` on main returns test fixtures only.

**Estimate: large — the biggest item on this list, and the only one that is
protocol work.** Weeks, not days, for a first version. The pieces that make it
tractable are already there (`gix-pack` writing, `gix-protocol` framing,
`gix-negotiate` for the have/want side), so the work is the receive-pack
conversation plus its error surface, not new plumbing. A first version could
reasonably refuse everything except a fast-forward update of a single ref with
an explicit refspec, and still remove the shim from most consumers: that is the
only shape keeper ever asks for.

**What a consumer needs from the API, concretely:** per-ref outcome, not an
aggregate. `--porcelain` is what keeper asks git for because a rejected push
must be distinguishable *per ref* and by *reason* (non-fast-forward vs hook vs
permission vs remote-unpack-failure). keeper's field bug DW-207 was exactly this:
git's stderr summary names only the remote, so every rejected push read
identically in the log.

### A2. `worktree add` / `remove` / `prune` — linked worktree administration

**What keeper runs**

```
git worktree add --detach <absolute-path> <branch>
git worktree remove --force <absolute-path>
git worktree prune
```

keeper uses a linked worktree as the staging area for a checkout it must be able
to abandon (a pendrive pulled mid-sync, AD-48), so this is not a developer
convenience — it is the transactional boundary.

**What gitoxide has.** The reading half, completely: `Repository::worktrees()`,
`worktree_proxy_by_id()`, `main_repo()`, `is_bare()` (`gix/src/repository/worktree.rs`
on main), and `gix-discover` understands `.git` files, `commondir`, and
`gitdir` indirection well enough to *open* any worktree git creates.

**What is missing.** Creating one, and its administrative files:
`.git/worktrees/<id>/{gitdir,commondir,HEAD,index}`, the `.git` file in the new
directory, `ORIG_HEAD`, the locking convention, and the pruning rule (an
administrative directory whose `gitdir` target no longer exists). Removal is the
same set in reverse plus the worktree contents.

**Estimate: small-to-medium, and the best value on this list.** Days. It is
file writing against a documented layout, `gix-discover` already contains the
parser for every file that has to be produced, and `gix-worktree-state` already
does the checkout the new worktree needs. No protocol, no object database work,
no new dependency. A correct `add`/`remove`/`prune` trio plus a `worktrees()`
that reports lock state would close the whole gap.

### A3. `sparse-checkout set --cone` / `disable` — narrowing a checkout

**What keeper runs**

```
git sparse-checkout set --cone --skip-checks -- <subpath>...
git sparse-checkout disable
```

**What gitoxide has.** The index side of the feature and none of the driving.
`gix-index/src/access/sparse.rs` reads and writes the sparse bits,
`SKIP_WORKTREE` is understood throughout, `gix-pathspec` and `gix-ignore` can
compile the pattern language, and `gix::Repository` already exposes
`index.sparse` handling. keeper reads `.git/info/sparse-checkout` itself
(`git::repo::sparse_patterns`) because reading is easy.

**What is missing.** The operation: write `.git/info/sparse-checkout`, set
`core.sparseCheckout`, recompute which entries are inside the cone, set or clear
`SKIP_WORKTREE` on each, then add or remove the corresponding worktree files —
and do the last step without destroying a modified file that is about to leave
the cone (git refuses; a library must at least be able to report it).

**Estimate: medium.** A week-ish. The pattern matching and the index flags
exist, so this is the reconciliation loop plus its refusal cases. The subtlety
is not the cone algebra, it is deciding what to do with dirty paths on the way
out, which is a policy the API has to expose rather than pick.

### A4. `gc` — repacking and pruning

**What keeper runs**

```
git gc --quiet
```

**What gitoxide has.** `gix-pack` writes packs, `gix-odb` reads loose and packed
objects and can iterate both, `gix-ref` has a packed-refs writer.

**What is missing.** Any aggregate maintenance entry point at all:
`git grep -lE "fn (gc|repack|maintenance)"` on main returns nothing. The
constituent decisions — which loose objects are reachable, when to repack, what
to keep for reflog safety, pruning unreferenced LFS-style large blobs — have no
home yet.

**Estimate: large, and easy to split.** The useful subset for an application is
much smaller than `git gc`: "pack every loose object that is reachable, write
`packed-refs`, delete what is now redundant" is days of work on top of
`gix-pack`, and would let a long-running app stop growing a six-figure loose
object directory. Full parity (reflog expiry, `gc.pruneExpire`, cruft packs,
multi-pack-index maintenance, bitmap generation) is a project.

---

## Group B — already possible, so keeper's shim should shrink

These are in keeper's shim for historical reasons, not because gitoxide lacks
them. They are listed so the report is honest about the size of the real gap,
and they are keeper's work to remove, not gitoxide's.

| keeper's current call | the gitoxide equivalent, verified in the pinned release |
| --- | --- |
| `git merge-base <a> <b>` | `Repository::merge_base` (`gix/src/repository/revision.rs`) |
| `git merge-base --is-ancestor <a> <b>` | same, plus an id comparison |
| `git rev-parse --verify <ref>` | `Repository::rev_parse_single` |
| `git diff --name-only <from> <to>` | `Repository::diff_tree_to_tree` |
| `git status --porcelain` (the notes vault only) | `gix::status`, which the sync engine already uses |
| `git merge --ff-only <ref>` | `merge_commits` decides it; the remaining work is a ref edit plus a worktree update |
| `git merge --no-edit -X theirs <ref>` | `Repository::merge_trees` with `tree_merge_options`, which has the ours/theirs resolution knobs |
| `git switch [-c] <branch>` | branch creation is a ref edit; the worktree update is the same missing piece as in the merge rows |

The one thing every merge/switch row shares is **applying a computed tree to an
existing worktree**: gitoxide can produce the merged tree, and
`gix-worktree-state` can materialise a tree into an empty directory, but there is
no "make this worktree look like that tree, and tell me what you refuse to
overwrite". That single operation would move four of these eight rows from
"possible with care" to "trivial", and it is the natural companion to A3, which
needs the same loop.

**Estimate for that one operation: medium**, and it is the highest-leverage
thing on the whole page after A2.

---

## Two smaller asks that are not features

1. **`gix-filter`: reap `process` filter children.** Already written and
   measured; see `docs/upstream/gitoxide-filter-process-leak.md` and
   tgorka/gitoxide#1 / #2. keeper pins a fork for it today. It is the one item
   here that is a defect rather than a gap.
2. **A way to reach the filter pipeline a `status()` used.** `Repository::status`
   takes no pipeline and returns none, `index_as_worktree`'s `Outcome` does not
   hand its `resource_cache` back, and `worktree_stream`'s pipeline is
   unreachable — which is why the leak above could not be worked around by any
   consumer, and why a consumer cannot pool one long-running filter across
   passes either. Exposing it would make the 5-10 ms handshake per pass optional
   rather than mandatory. **Estimate: small**, but it is an API-shape decision
   more than an implementation.

---

## Priority, from a consumer's point of view

1. **A2 (worktree add/remove/prune)** — smallest gap, unblocks the
   transactional checkout, no protocol work.
2. **The "apply a tree to a worktree" operation** — unblocks merge, switch and
   half of group B.
3. **A3 (sparse-checkout)** — needs the same loop as (2), so it follows cheaply.
4. **A1 (push)** — the largest, and the only one that removes the last reason to
   have `git` installed at all. Worth scoping down to fast-forward-single-ref
   first.
5. **A4 (gc)** — worth doing as the narrow "pack the loose objects" subset long
   before parity.
