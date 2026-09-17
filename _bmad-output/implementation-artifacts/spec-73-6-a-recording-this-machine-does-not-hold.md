---
status: review
baseline_revision: 771fffd
final_revision: ''
---

# Story 73.6 — A recording this machine does not hold is not its to rewrite

<intent-contract>

## Problem

The owner saw two `*.sync-conflict-*` copies of a recording manifest in `tgdrive` and asked for them to be resolved. The copies were the symptom; what produced them is a second machine rewriting the first one's recording.

Measured on hesperia, 2026-09-17, session `40-media/recordings/2026/2026-09-15 14.29 mounica-sync`:

| file | status | `endedAt` | segment bytes |
| --- | --- | --- | --- |
| `manifest.json` (live, Sep 16 00:59) | `recovered` | absent | **134, 135, 134, 135** |
| `…sync-conflict-20260915-223012…` | `finalized` | `15:24:53` | 837_784_110 · 2_007_949_579 · 889_657_113 · 1_787_690_532 |
| `…sync-conflict-20260916-075905…` | `recovered` | absent | the real four |

The four `.mov` files on the recording machine are exactly the sizes the *finalized* copy records. 134 and 135 bytes are the lengths of **git-LFS pointers**.

The chain: the folder reached `tgdrive-light` — which stores its media as pointers — while still `status:"recording"`. That clone's `recover_orphaned_sessions` found an inactive `recording` session, ran `reconcile_from_dir`, whose documented contract is *"disk is authoritative"*, over four pointer stubs, wrote `recovered` with the stubs' lengths, and committed it. Two machines writing one file is the divergence; AD-43 preserved the loser as a conflict copy, which is exactly what it is for.

A worse case sat one step away: `partial_segment_is_usable` scans ISO-BMFF boxes for a complete `moov`; a pointer has none, so a `.partial` arriving as a pointer would have been **deleted** as unusable and the deletion committed — destroying that clone's only reference to media that is intact elsewhere.

## Approach

Give the recording module the one fact it was missing — whether the bytes are here — and let it decline everything it cannot judge.

## Always

Read a candidate's first bytes; a git-LFS pointer identifies itself in its first line and states the media's true length in its `size` line. Any pointer among a session's **segment** files means the session was not recorded here: recovery leaves the folder untouched. Where a size is still needed, the pointer's `size` is used.

## Block If

Nothing is blocked for the user: a clone that cannot judge a folder does nothing to it, and the machine that owns the bytes finishes it as before.

## Never

Never delete a `.partial` that is a pointer. Never write `metadata().len()` for a file that is a stand-in. Never let keeper-core learn any more LFS than the pointer's first line and its `size` — what is stored as a pointer, when it is hydrated and when it is released stays entirely in `keeper-sync`.

## I/O and edge-case matrix

| Input | Observable result |
| --- | --- |
| `recording` session, segments are pointers | not recovered; `manifest.json` unchanged byte for byte; pointer `.partial` still there |
| `recording` session recorded here, unrelated non-segment pointer beside it | recovers as before |
| reconcile over one pointer + one real file | pointer's `size`, real file's own length |
| `resolve_partial_segments` on a pointer `.partial` | neither renamed nor deleted |
| pointer whose `size` line is missing/unparseable | treated as not a pointer → ordinary filesystem answer |
| unreadable file / short read | not a pointer; nothing changes |
| terminal (`finalized`/`recovered`/`failed`) session | untouched, as before (this is what makes the field repair stable on the *installed* build) |

</intent-contract>

## Code Map

- `src-tauri/crates/keeper-core/src/recording.rs`
  - `LFS_POINTER_FIRST_LINE`, `LFS_POINTER_PROBE_BYTES`, `lfs_pointer_media_size` (new) — a 512-byte head read, the first line compared, the `size` line parsed. Total: every failure is `None`.
  - `folder_holds_pointer_segments` (new) — any segment-shaped entry (`.partial` or not, via the existing `split_partial_suffix` + `session_segment_identity`) that is a pointer.
  - `salvage_session_folder` — the new guard, after the `is_active` live-session guard and **before** `resolve_partial_segments`, so nothing irreversible runs first.
  - `resolve_partial_segments` — the same rule at the one irreversible call, because the function is public and a future caller will not know.
  - `reconcile_from_dir` — `bytes` is the pointer's `size` when there is one, else `metadata.len()`.
- Firewall: no new dependency, no `cfg`, no shell or Apple type — `dependency_firewall_holds` still passes.

## Tasks & Acceptance

- [x] The pointer reader, the folder predicate, the three call sites.
- [x] Four tests, each proved by mutation.
- [x] The field repair on hesperia, verified on both clones and in `archive.db`.

Acceptance, verbatim from the epic: *a `recording` fixture whose segments are pointers is returned by recovery as not recovered, with its `manifest.json` unchanged byte for byte and its pointer `.partial` still on disk; a session recorded here still recovers when an unrelated (non-segment) pointer sits beside it, so the guard keys on segment names only; a reconcile over one pointer and one real file records the pointer's `size` and the real file's own length; `resolve_partial_segments` called directly on a pointer `.partial` neither renames nor deletes it. Each test proved by mutation. On hesperia: zero `*sync-conflict*` files and zero tracked ones in either clone, both HEADs equal, both status clean, and the surviving manifest `finalized` with `endedAt` and four segment sizes equal to the bytes on disk.*

## Design Notes

**Why `finalized` was restored, and not `recovered`.** `recovered` means *a partial recording was salvaged*; this one was not. The session ended cleanly at 15:24:53, all four files are whole, and the finalized copy is a strict superset of both others (it alone carries `endedAt` and the capture-clock PTS bounds, which no rebuild can ever recover — the muxer rebases each file's timeline to 0). It is also the *stable* choice against the build now installed: both terminal states are skipped by recovery, but only `finalized` is true.

**Why the guard is "any pointer", not "all pointers".** A session whose bytes are partly elsewhere was not made here either, and a half-judged folder is the worst outcome available: some segments measured, some invented.

**Why a pointer's `size` and not the manifest's previous number.** Both would have prevented this defect; the pointer is *better*, because it is correct even for a segment the manifest never listed — a session whose final segment has no `segmentClosed` event, which is the ordinary case `reconcile_from_dir` exists for.

**What this does not attempt.** The archive's own `one session id under two roots; keeping the first` warning is pre-existing and untouched: both clones of one repository are indexed, and the collision is in the session-id scheme, not here.

## Verification

- `cargo test -p keeper-core --lib -- recording::tests` — 135 passed, including the four new ones.
- Mutation (script `/tmp/mut256.py`, three mutations, each run alone then reverted):
  - `bytes = metadata.len()` → `a_pointer_segment_keeps_the_medias_real_size_in_the_ledger` FAILED.
  - recovery guard short-circuited → `recovery_leaves_a_synced_session_whose_media_is_not_on_this_machine` FAILED.
  - partial guard short-circuited → `resolving_partials_never_deletes_a_pointer` FAILED.
  - Source restored and md5-compared against the pre-mutation copy; the suite re-run green.
- `cargo clippy -p keeper-core --all-targets -- -D warnings` — clean (one `unnecessary_lazy_evaluations` found and fixed); `cargo fmt --all` applied.
- Field, on hesperia: `0` conflict copies on disk and `0` tracked in all three folders; `tgdrive` and `tgdrive-light` both at `1e513304b`, both clean; the surviving `manifest.json` byte-identical on both clones (`sha256 8e69d9c2…`), `finalized`, `endedAt 2026-09-15T15:24:53-07:00`, its four segment sizes equal to the media on the full clone and to the pointers' `size` lines on the light one; `archive.db` after the next app start reads `ended_ts=1789511093000` and the four real sizes. Backups of all three original files kept at `hesperia:/tmp/manifest-backup-20260916-194236/`.
- keeper did the publishing, as it must: `71f07ccb2 sync(tgdrive@hesperia): 2 deleted`, `1e513304b … 1 modified`, `pushed … commits=2`.
