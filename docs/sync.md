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
| `lfsMode` | `materialize`, `pointerOnly`, or `disabled` |
| `lfsThresholdBytes` | Files at or above this are tracked through LFS (default 4 MiB) |
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
6. Push.

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

- **Downloads resume.** An interrupted transfer restarts from where it stopped,
  and the digest of the existing prefix is carried forward.
- **Uploads do not resume.** The LFS `basic` adapter has no resumable upload in
  the specification, `tus` is experimental and upload-only, and `multipart` is a
  proposal with no implementation anywhere. An interrupted upload retries from
  zero.
- **Every object is verified** against both its SHA-256 and its expected byte
  count, in both directions. A mismatch discards the staged bytes rather than
  resuming from a poisoned prefix.

Against Forgejo specifically, keeper works around several server behaviours: the
LFS media type must be the first value in `Accept` (or the server returns 415);
the `Content-Range` total is computed incorrectly, so only the start byte is
trusted; and range offsets are parsed as 32-bit, so resume above 2 GiB falls
back to a restart.

`lfsMode = pointerOnly` leaves excluded paths as pointer files. This is the only
lever that reduces LFS traffic — sparse checkout does **not**, because git-lfs is
entirely sparse-checkout-unaware.

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

---

## 11. Progress and warnings

- **Tray glyph.** Monochrome template icons distinguished by shape, never
  colour — the system recolours them for light and dark menu bars, so colour
  cannot carry meaning. Four rotating frames signal activity, advanced by the
  existing ~1 Hz tick; separate glyphs cover armed, paused/media-absent and
  warning.
- **Tray status line**, e.g.
  `Transferring tgdrive — 42/310 files · 1.2 GB of 4.7 GB`.
- **In-app**: a progress meter and a sticky amber warning banner. Warnings that
  need a decision get an inline action button.
- **Notifications** fire exactly once per warning onset.
- **Never a toast** for connectivity or any other persistent condition.

Progress is reported in bytes where a total is known, because a file-counted bar
sits at 50% for ten minutes when one of the two files is a 4 GB video.

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

---

## 13. Security posture

- Credentials live in the OS keychain (or, headless, a `0600` file) and are
  injected through gitoxide's programmatic credential callback — no helper
  subprocess is ever spawned.
- SSH remotes delegate entirely to your own `ssh` binary, agent and config.
  Keeper never reads a private key.
- Every configured remote is a **disclosed egress destination**, computed from
  the live profile set and shown in Settings → About. It cannot drift from
  reality because it is not maintained by hand.
- Logs carry ids, hosts, paths and byte counts — never credentials, never file
  content.

---

## 14. Troubleshooting

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
| Watch mode misses changes on a network mount | inotify/FSEvents do not work on many network and FUSE mounts. Enable polling for that profile. |
| `Too many open files` / watches exhausted | Raise `fs.inotify.max_user_watches`. One watcher is used per profile, not per folder. |
| LFS upload restarts from zero | Expected. The `basic` adapter has no resumable upload. |
| Resume fails on an object above 2 GiB | Forgejo parses range offsets as 32-bit. Keeper restarts the transfer instead. |
| History is growing fast | git keeps every revision. Run `git gc` on the repository, raise the LFS threshold, or exclude churning files. |

Enable **Settings → Advanced → Debug logging** for on-disk logs
(`~/Library/Logs/keeper/keeper.log` on macOS, `$XDG_STATE_HOME/keeper/` on
Linux).

---

## 15. Current implementation status

This document describes the designed behaviour. As of 2026-07-25 the engine and
the `keeper-syncd` daemon implement and verify §§1–7, §9, §10 and §12 against
real git remotes. Three parts are not yet reachable at runtime:

- **§8 large files.** The LFS client is written and unit-tested, but nothing in
  the commit path routes an oversized file through it yet, so LFS does not
  engage. Files above the threshold are currently committed as ordinary git
  blobs — which is exactly what §8 says must not happen for multi-gigabyte
  content. Do not point a profile at large binaries until this lands.
- **§7 review lanes.** The worktree commands exist; the engine does not yet
  create a lane or open a pull request. A `pushOnly` profile still pushes
  correctly, it just pushes its own branch rather than a generated lane.
- **§11 progress and warnings.** These are engine-side and correct — the tray
  decision, the status line and the warning onset logic are implemented and
  tested — but the desktop app surfaces that render them are not wired up.

## 16. Deliberate limitations

1. **No content merge.** Divergent text files produce conflict copies, not a
   three-way merge.
2. **No resumable LFS upload.** It does not exist in the protocol.
3. **Sparse checkout does not reduce LFS traffic.** Use `pointerOnly` and
   `subpaths`.
4. **macOS has no open-writer veto** (see §4).
5. **A `git` binary is required** (see §1).
6. **No automatic history pruning.** Sync churn grows a repository; `git gc` is
   available but shrinking history is a destructive operation keeper will not
   perform on its own.
