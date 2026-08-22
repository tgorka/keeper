# Research brief — virtual files (on-demand materialization of LFS content)

type: technical
shape: explore (with a select sub-question: which OS-level placeholder technology, if any)
mode: run (native, parallel web fan-out) + repository grounding
date: 2026-08-22
decision served: how keeper should let a clone hold knowledge of large LFS-tracked content
  without holding its bytes, materialize it on request, and release the materialization again —
  safely, on a Linux server first and the macOS desktop second.

## The ask, as stated by the owner

1. Selected LFS-tracked files are **not downloaded** into a local clone, selected by a
   configuration/pattern file in the manner of `.gitignore`.
2. The clone still **knows those files exist**; a pointer is acceptable; metadata is wanted.
3. On demand the file can be **materialized**, and afterwards the materialization **released** —
   lazily is fine, ~24 h after last use, "in a nightly script".
4. Fast and efficient, but above all **simple and safe**.
5. Primary host `keeper-syncd` on a server; the desktop app also matters. A **system-level**
   presentation showing the files virtually in `ls`/Finder would be a bonus.

## Briefs dispatched

| id | brief | digest |
|---|---|---|
| R1 | git-native and git-adjacent "don't download this, but keep knowing about it": git-lfs selective fetch/smudge/prune, partial clone and promisor remotes, sparse-checkout, git-annex (preferred/required content, numcopies, locked vs unlocked), DVC, XetHub; and the pattern-file/precedence question per tool | `digests/r1-git-native.md` |
| R2 | OS-level virtual/placeholder technologies: macOS File Provider + dataless files + FSKit + macFUSE/fuse-t licences + NFS loopback; Linux FUSE (incl. passthrough), fanotify pre-content HSM, overlayfs/autofs/fscache, sparse-file `stat` semantics; Windows cfapi/ProjFS; and the real implementations (Dropbox, OneDrive, iCloud, Nextcloud, rclone, VFS for Git, Scalar, EdenFS, JuiceFS) | `digests/r2-virtual-filesystems.md` |
| R3 | Safety engineering of materialize + timed eviction: what proof each system demands before deleting the last local copy, documented data-loss incidents, atomic replacement with readers attached, in-use detection, LRU/TTL policy design, dirty detection incl. git's racily-clean rule, concurrency and crash behaviour, and the accidental-mass-hydration threat model | `digests/r3-eviction-safety.md` |
| R4 | Metadata surfaces for non-local content (pointer formats, xattr durability, sidecars, `.icloud` placeholders, listing/query surfaces) and pattern-file design (gitignore/gitattributes semantics, Rust crates + licences, size/type policy expressions, file-manager "online-only" conventions) | `digests/r4-metadata-and-patterns.md` |

Research subagents received their brief only — no project files, no PRD, no code — per the
research firewall. Project context shaped *what was asked*, never *what is true*.

## Repository grounding (separate, and separately cited)

| id | scope | file |
|---|---|---|
| G1 | keeper's existing LFS + materialization machinery, path by path | `context/g1-lfs-machinery.md` |
| G2 | the daemon, IPC, frontend, progress and platform surfaces a virtual-file feature must reach | `context/g2-surfaces.md` |
| G3 | BMAD conventions and the free epic/AD/DW/FR numbers | `context/g3-bmad-state.md` |

Every grounding claim carries `path:line`. Grounding is admissible as evidence about *keeper*,
never as evidence about the outside world.

## Outputs

- `../../research-virtual-files-2026-08-22.md` — the synthesized, §-numbered research document
  (the citable artifact; Rust doc-comments in this tree cite research by section).
- `../../architecture/architecture-keeper-2026-07-03/ARCHITECTURE-VIRTUAL-FILES.md` — AD-122…AD-130.
- `../../epic-56-the-file-is-there-even-when-it-is-not.md` — the epic, FR-328…FR-339, NFR-40/41.
- `docs/decisions.md` D-2 — the durable "pointer files, not filesystem virtualization" decision.

## The finding that changed the plan

git-lfs shipped ask 1 and ask 3's release half as **one mechanism** — `lfs.fetchexclude`, a
gitignore-style path list — and it became an eviction authorization, deleting objects still
referenced by the current checkout (git-lfs#3092). Every documented data-loss incident in this
space has that same shape. The design therefore separates them permanently: the pattern file
authorizes *hydration* decisions; only per-object proof authorizes *deletion*.

Second: ask 5 is not a later version of the same feature. macOS cannot virtualize a path the
user chose, and the two FUSE options for macOS both fail the licence firewall. That is a closed
question, recorded as D-2 so it is not re-asked.
