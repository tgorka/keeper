# Phase 7 — Sessions (Epics 47–49)

status: draft
created: 2026-08-12
altitude: phase
source: `product-inputs-sessions-2026-08-12.md` (the numbering spine — FR-222…FR-252,
NFR-36…NFR-39, AD-107…AD-115, UX-DR85…UX-DR92, Epics 47–49, allocated there and nowhere
else), derived from `_bmad-output/brainstorming/brainstorm-keeper-sessions-2026-08-12/`
(ground-truth.md is the verified on-disk contract of the two live zones)

Owner-requested phase: LLM work sessions — manual or hybrid — as a lens over the
`60-sessions/` zones keeper already syncs on tgdrive and neuradrive. See what is being
worked on, edit the record with the full notes experience including capture, watch agent
changes live, create from template or from a previous session with two-way lineage,
promote workspace files to artifacts, archive through the zone's own checklist, search
with tags/properties/pins.

The route is locked by the spine and must not be re-argued in a story:

- **A sessions root is a sessions-flagged `SyncProfile` plus a subfolder** (default
  `60-sessions`); adopt-only, skeleton creation separate and explicit (AD-107).
- **The sessions domain is pure and lands in `keeper-core`** — index, naming,
  promote-table parser, lineage, freshness, plans — reachable from nextest with no
  filesystem (AD-108). The shell executes.
- **One grammar, one frontmatter writer, one editor.** Sessions extend `notes::query`
  and `notes::frontmatter` and open files through the notes body machinery via file
  targets; a fork of any of these is a defect (AD-109).
- **Files are the only truth.** Status is location, freshness is derived and split by
  subtree, lineage is frontmatter on both ends, promotion is the README table; the
  index is a disposable cache (AD-110, AD-112).
- **Lifecycle verbs are journaled plans**; promote copies pass the stability gate; the
  archive move is the last step (AD-111).
- **Workspace is a read-only projection** — listed, never written, never committed,
  never searched by default (AD-113).
- **IPC and shell placement mirror notes exactly** (AD-114, AD-115).

## Epics

| epic | title | stories | binds |
|---|---|---|---|
| 47 | The zone you already have | 6 | FR-222–FR-232, NFR-36, AD-107, AD-108, AD-110, AD-113 (index side), UX-DR85, UX-DR86, UX-DR92 |
| 48 | Open it, log it, watch it | 7 | FR-233–FR-242, FR-250–FR-252, NFR-37, NFR-39, AD-109, AD-113 (viewer side), AD-114, AD-115, UX-DR89, UX-DR91 |
| 49 | The promote airlock | 6 | FR-243–FR-249, NFR-38, AD-111, AD-112, UX-DR87, UX-DR88, UX-DR90 |

19 stories. Every story is scoped to one dev session and independently shippable.

## Epic 47 — The zone you already have

Root flag, pure domain, index, and the board. Exit gate: flag tgdrive's zone and the
board is correct with zero on-disk writes beyond `.keeper/` and minted ids.

- **47.1 — `SyncProfile.sessions`, capability, and the flag surface** (Rust + UI).
  `SessionsConfig` field with `#[serde(default)]`; `CapabilitiesVm.sessions`; the
  Settings → Sync folder-card toggle with subfolder name and adopt-only validation.
  Skeleton creation is NOT this story. Binds FR-222 (flag half), FR-223, AD-107.
- **47.2 — `keeper_core::sessions` model, naming, and index** (pure Rust). Session
  discovery rules, ULID identity via `notes::frontmatter`, status-from-location,
  freshness fold over supplied `(path, mtime)` facts, `SessionIndexSnapshot`, cache
  serde. Binds FR-225, FR-226, FR-227 (read side), FR-228, NFR-36, AD-108, AD-110.
- **47.3 — the shell indexer and watcher tap** (Rust). `keeper::sessions_root`: cold
  scan, `.keeper/sessions-index.json`, the watcher subscription with the
  workspace-events-are-freshness-only rule, budgeted workspace walks, rebuild verb.
  Binds FR-225, NFR-36, NFR-37 (scan half), AD-110, AD-113.
- **47.4 — list IPC and the board** (crosses IPC). `SessionRowVm`,
  `SessionChangeBatch`/`SessionListOp`, `sessions_list`/`sessions_subscribe_changes`,
  the sessions primary view, rows with the two freshness signals, status glyphs, pins
  read, unread read, `sessions-list`/`sessions-roots` stores, `use-sessions-changes`.
  Binds FR-224, FR-228, FR-232 (render), FR-234 (list half), UX-DR85, UX-DR86, AD-114.
- **47.5 — query grammar extension and the filter bar** (crosses IPC). The
  `notes::query` extension point with the sessions `is:` set, `sessions_search`
  bounded scan (README/artifacts/prompts/refs; workspace toggle), the chip bar and
  search field, `is:stale`. Binds FR-229, FR-230, AD-109 (grammar), UX-DR92.
- **47.6 — default spaces, pins write, skeleton creation** (crosses IPC). Sessions
  default spaces seeded/restorable; pin toggle writing frontmatter; the explicit
  skeleton-creation action with its named-writes confirmation. Binds FR-222 (skeleton
  half), FR-231, FR-232, NFR-39 (pin write), AD-107 detail.

## Epic 48 — Open it, log it, watch it

Detail, editor reuse, live body changes, capture parity, cross-surface registration.
**48.1 is the risk story and runs first** (AD-109's file-target seam). Exit gate: agent
edits a README on disk → row, unread, and open-buffer diff bar all correct; capture
appends to today's log.

- **48.1 — file targets for the notes body machinery** (Rust, the seam). Teach
  `notes_vault` reader/writer/body-channels to serve `(rootKind, rootId, relPath)`
  targets with the `ReadOnly` refusal for `workspace/`; no behaviour change for
  vaults. Binds FR-233 (plumbing), FR-237 (refusal), AD-109, AD-113.
- **48.2 — session detail and tree** (crosses IPC). `SessionVm`/`SessionTreeVm`,
  `sessions_get`/`sessions_tree`, the detail panel target: header (lineage chips
  rendered read-only this story), tree sections, README open by default in the notes
  editor, workspace section muted with the zone's sentence, registry viewers,
  reveal/copy-path. Binds FR-233, FR-237, FR-252, UX-DR89 (render), UX-DR91, AD-115.
- **48.3 — unread, changes-since, history** (crosses IPC). Acknowledged-rev store in
  `.keeper/`, `sessions_mark_read`/`sessions_changes_since`, the since-you-looked row
  with per-file diffs over the existing history surfaces. Binds FR-235, FR-236.
- **48.4 — log-today** (crosses IPC). The splice-append of today's `### YYYY-MM-DD — `
  entry (creating `## Log` if missing), `sessions_log_today`, caret placement, menu +
  palette + `⌘⌥L`. Binds FR-240, NFR-39, AD-115.
- **48.5 — current session and the tray** (crosses IPC). Per-root sticky choice in the
  profile blob, `sessions_set_current`, fallback-with-"(latest)" rule, tray section
  (current session, freshness, Log Today). Binds FR-242, FR-251 (tray), AD-115.
- **48.6 — capture target: session log** (crosses IPC). `CaptureTargetVm` gains
  `session-log`; the capture footer target switch; Escape appends via the 48.4 writer;
  README-as-capture-window. Binds FR-241, UX-DR91, AD-115.
- **48.7 — linking and registration sweep** (crosses IPC). Wikilink resolution of
  sessions from notes, backlinks across indexes, quick-switcher entries, the palette
  verb sweep, `⌘7`, cheat sheet rows. Binds FR-250, FR-251, UX-DR92.

## Epic 49 — The promote airlock

Lifecycle plans. **49.1 is the risk story and runs first** (AD-111's journal). Exit
gate: phase acceptance items 3, 4, 5 (create/lineage, promote, crash-safe archive).

- **49.1 — plans, the journal, and the executor** (Rust). `keeper_core::sessions::plan`
  (CopyStep/MoveStep/SpliceStep/TrashStep + compile functions for create/promote/
  archive/delete/unarchive), the shell executor with
  `.keeper/sessions-journal.json`, resume/rollback on relaunch, move-last invariant.
  Binds NFR-38, AD-111.
- **49.2 — promote-table parser and panel** (crosses IPC). The section-scoped table
  parser with `Unreadable` rows, span-splice row writes, `SessionPromoteVm`,
  `sessions_promote`/`sessions_promote_panel`, the two-column panel with staleness/
  missing badges, stability-gate parking surfaced as "still changing". Binds FR-243,
  FR-244, NFR-39, AD-108 detail, UX-DR90.
- **49.3 — new session and new-like-this** (crosses IPC). Template copy plan,
  structure-only pattern copy, lineage writes on both ends (including into archive/),
  the new-session sheet with real-tree preview, collision counters. Binds FR-238,
  FR-239, AD-112, UX-DR88.
- **49.4 — the archive checklist** (crosses IPC). `sessions_archive_plan`/`_run`/
  `_resume`, the checklist panel (promotes → warnings → empty-workspace → move),
  hot-workspace and follow-ups warnings, delete-instead offer, `e` on the board.
  Binds FR-245, FR-246, UX-DR87.
- **49.5 — delete, trash, unarchive** (crosses IPC). Trash-based session delete with
  the confirmed dialog, restore path documented, unarchive with continue-instead
  offer. Binds FR-246 (dialog), FR-247, FR-248.
- **49.6 — sync honesty, docs, and field validation** (crosses IPC + docs). Parked-
  upload affordance on rows, `docs/sessions.md` (the operator document, mirroring
  `docs/notes.md`), the phase-acceptance sweep against the real tgdrive zone
  read-only. Binds FR-249, phase acceptance.

## Dependency order

```
47.1 → 47.2 → 47.3 → 47.4 → 47.5 → 47.6
                 47.4 → 48.1 → 48.2 → 48.3
                                48.2 → 48.4 → 48.5 → 48.6
                                48.2 → 48.7
                 47.2 → 49.1 → 49.2 → 49.3 → 49.4 → 49.5 → 49.6
```

Epic 48 and Epic 49 parallelize after 47.4/47.2 respectively; 49.2+ also reads 48.1's
file targets for opening the README from the panel, so full 49 UI work follows 48.1.

## Bindings regeneration

Stories introducing/changing exported ts-rs types (must regenerate `src/lib/ipc/gen/`):
47.1, 47.4, 47.5, 47.6, 48.2, 48.3, 48.4, 48.5, 48.6, 48.7, 49.2, 49.3, 49.4, 49.5,
49.6. Pure-Rust: 47.2, 47.3, 48.1, 49.1.

## Coverage mapping

Every FR-222…FR-252 and NFR-36…NFR-39 appears in exactly one story's binds above
except FR-224 (47.4 render + 48.2 scoping — split noted), FR-233 (48.1 plumbing +
48.2 surface), FR-237 (48.1 refusal + 48.2 render), FR-222 (47.1 flag + 47.6
skeleton), FR-246 (49.4 offer + 49.5 dialog) — each split named at both stories.
