/**
 * The session's own file tree (Phase 7, FR-254, AD-117) — the session folder
 * as the small workspace it is.
 *
 * **Why a tree and not five flat lists.** A session folder is a workspace:
 * `artifacts/` grows subfolders, `workspace/` grows whatever the agent felt
 * like, and a flat per-section list could only ever show the top of each. So
 * the detail browses a session the way the Files pane browses a synced folder
 * — real nesting, one sync mark per entry, the same words for the same state —
 * and differs from it exactly where a session differs from a folder:
 *
 * - **The whole tree arrives at once.** A synced folder is unbounded, so the
 *   Files pane lists lazily; a session is bounded by its own contract, and its
 *   sections open together, so lazily browsing would trade one git query for
 *   five (AD-114 at one level down).
 * - **Everything starts open, except below `workspace/`.** What a person opened
 *   a session to see is what is in it; what they did not is the twentieth file
 *   of a `node_modules` the agent installed — and scratch is where that lands.
 * - **No selection and no multi-select.** The row's verbs are open, open-with,
 *   reveal and delete, one row at a time. This used to say "and no delete", and
 *   FR-262 is what changed the answer: a flat session is a pool a person adds
 *   to constantly, and a surface that can only grow is one whose mistakes are
 *   permanent. Deleting is still a single, confirmed, recoverable gesture — a
 *   trash move — rather than the beginning of a file manager.
 *
 * Every fact on a row was decided in Rust: the `subpath` a file target is set
 * from (AD-65 — nothing here joins a path), the sync mark and its sentence
 * (the Files tab's own, from one `Engine::pending` answer), `locked` — the
 * workspace fence's refusal sentence, on exactly the paths a write refuses
 * (AD-113) — and `undeletable`, which is `sessions::files::check_deletable`'s
 * refusal on exactly the paths a delete refuses. The row draws its Delete when
 * that is null and says the reason when it is not; **it never re-derives the
 * rule**, so a fifth refusal added in Rust reaches this tree without anyone
 * remembering to come here. This file renders them and runs the keyboard.
 */
import {
  ChevronDown,
  ChevronRight,
  ExternalLink,
  Folder,
  FolderOpen,
  Lock,
  SquareArrowOutUpRight,
  Trash2,
} from "lucide-react";
import type { KeyboardEvent as ReactKeyboardEvent } from "react";
import { useCallback, useMemo, useRef, useState } from "react";
import { SyncStatusMark } from "@/components/layout/sync-status-mark";
import {
  AlertDialog,
  AlertDialogAction,
  AlertDialogCancel,
  AlertDialogContent,
  AlertDialogDescription,
  AlertDialogFooter,
  AlertDialogHeader,
  AlertDialogTitle,
} from "@/components/ui/alert-dialog";
import { Button } from "@/components/ui/button";
import { formatDraftAge } from "@/lib/format-time";
import type { SessionEntryVm } from "@/lib/ipc/client";
import { revealPath, sessionsFileDelete, syncOpenEntry } from "@/lib/ipc/client";
import { useCapabilitiesStore } from "@/lib/stores/capabilities";
import { syncErrorMessage } from "@/lib/stores/sync";
import { resolveViewer, VIEWER_ICON } from "@/lib/viewers";

/** The tree's accessible name. */
export const SESSION_TREE_LABEL = "Session files";

/** What a session with nothing in it says — honestly, rather than vanishing. */
export const SESSION_TREE_EMPTY = "This session has no files yet.";

/**
 * What a walk that ran out of budget says. It names the cause, because the one
 * way to hit this is a `workspace/` somebody let a package manager into, and
 * knowing that is what turns a truncated tree into a shrug rather than a bug.
 */
export const SESSION_TREE_TRUNCATED =
  "Too many files to show them all — the rest are on disk. A session's workspace is scratch; it is not meant to hold a dependency tree.";

/** Row verbs, in the order they matter. */
export const SESSION_TREE_OPEN_LABEL = "Open in the panel";
export const SESSION_TREE_OPEN_EXTERNAL_LABEL = "Open in the default app";
export const SESSION_TREE_REVEAL_LABEL = "Reveal in Finder";
export const SESSION_TREE_DELETE_LABEL = "Delete";

/** The confirmation, and what it promises. */
export const SESSION_TREE_DELETE_TITLE = "Delete this file?";

/**
 * The body, which says where the file goes rather than that it is gone.
 *
 * A trash move is recoverable and a person deciding deserves to know that — but
 * the sentence stops short of "you can undo this", because recovery is Finder's
 * job here and keeper has no button for it.
 */
export const SESSION_TREE_DELETE_BODY =
  "keeper moves it to the trash, so it is recoverable from there. Any reference pointing at it will report it missing.";

/** What a failed delete says when Rust has nothing more specific. */
export const SESSION_TREE_DELETE_FAILED = "keeper couldn't delete that file. Nothing was changed.";

/** One row, for tests that need to find one by path. */
export const SESSION_TREE_ROW_TESTID = "session-tree-row";

/** The indent per level, and the base pad — the Files pane's own numbers. */
const INDENT_PX = 16;
const PAD_PX = 8;

/**
 * The rows that render, in order, with each folder's open state applied.
 *
 * The flat `SessionEntryVm[]` is already in render order (the shell walked it
 * that way), so this is a filter, not a sort: an entry renders when every
 * ancestor of it is open. Deriving it per render rather than storing a nested
 * shape is what keeps the tree's order the shell's decision.
 */
function visibleRows(entries: SessionEntryVm[], open: Set<string>): SessionEntryVm[] {
  return entries.filter((entry) => {
    if (entry.parent === "") {
      return true;
    }
    // Every ancestor, not just the parent: a closed section hides its whole
    // subtree, however deep the entry sits.
    const parts = entry.parent.split("/");
    for (let index = 0; index < parts.length; index += 1) {
      if (!open.has(parts.slice(0, index + 1).join("/"))) {
        return false;
      }
    }
    return true;
  });
}

/**
 * The one folder whose subtree does NOT open on arrival — see below.
 */
const SCRATCH_DIR = "workspace";

/**
 * The folders open on arrival: **all of them, except inside `workspace/`**.
 *
 * This used to open the top level only. The flat contract is what changed the
 * answer: a flat session's structure is no longer "which sections exist" — the
 * sections are gone — it is the files themselves, and `artifacts/` is now the
 * only place real nesting lives. Opening one level deep would show a person a
 * folder icon where the thing they came to see is.
 *
 * The tree already arrives whole (the shell walks it in one pass), so this
 * costs no read; it is purely which rows render.
 *
 * `workspace/` keeps its subtree closed, and this is the same judgement the
 * original one-level rule was making rather than a retreat from it: scratch is
 * the one directory in a session with no contract about its size, the one the
 * truncation notice names by name, and the one an agent points a package
 * manager at. Its own row still opens, so its contents are one click away and
 * never hidden — what stays closed is the depth below them.
 */
export function initialOpenFolders(entries: SessionEntryVm[]): Set<string> {
  return new Set(
    entries
      .filter(
        (entry) =>
          entry.isDir &&
          entry.parent !== SCRATCH_DIR &&
          !entry.parent.startsWith(`${SCRATCH_DIR}/`),
      )
      .map((entry) => entry.relPath),
  );
}

export interface SessionTreeProps {
  /** The profile id — a sessions root IS a profile (AD-107). */
  rootId: string;
  /** Which session these files are in — the delete verb's other half. */
  sessionId: string;
  entries: SessionEntryVm[];
  truncated: boolean;
  /** Open a file in the panel strip through the one file target (AD-109). */
  onOpen: (entry: SessionEntryVm) => void;
  /** Re-read the surface after a delete, without waiting on the watcher. */
  onChanged: () => void;
}

export function SessionTree({
  rootId,
  sessionId,
  entries,
  truncated,
  onOpen,
  onChanged,
}: SessionTreeProps) {
  const canReveal = useCapabilitiesStore((s) => s.capabilities.revealInFileManager);
  const [open, setOpen] = useState<Set<string>>(() => initialOpenFolders(entries));
  // The roving tabindex's memory: exactly one row is in the tab order, and it
  // is the last one focused rather than always the first — so Tab back into a
  // tree returns where the person left it (Story 43.8's rule).
  const [activeKey, setActiveKey] = useState<string | null>(null);
  // Which file the confirmation is about, `null` when it is closed. The entry
  // rather than its path, so the dialog can name the file even after a re-read
  // has removed the row that opened it.
  const [deleting, setDeleting] = useState<SessionEntryVm | null>(null);
  const [notice, setNotice] = useState<string | null>(null);
  const rowRefs = useRef(new Map<string, HTMLDivElement>());
  const nowMs = Date.now();

  const confirmDelete = useCallback(() => {
    if (deleting === null) {
      return;
    }
    const target = deleting;
    setDeleting(null);
    setNotice(null);
    sessionsFileDelete(rootId, sessionId, target.relPath)
      .then(onChanged)
      .catch((raw: unknown) => setNotice(syncErrorMessage(raw, SESSION_TREE_DELETE_FAILED)));
  }, [deleting, rootId, sessionId, onChanged]);

  const rows = useMemo(() => visibleRows(entries, open), [entries, open]);
  const active = rows.some((row) => row.relPath === activeKey)
    ? activeKey
    : (rows[0]?.relPath ?? null);

  const toggle = useCallback((relPath: string) => {
    setOpen((previous) => {
      const next = new Set(previous);
      if (!next.delete(relPath)) {
        next.add(relPath);
      }
      return next;
    });
  }, []);

  const focusRow = useCallback((relPath: string) => {
    setActiveKey(relPath);
    rowRefs.current.get(relPath)?.focus();
  }, []);

  const onKeyDown = useCallback(
    (event: ReactKeyboardEvent<HTMLDivElement>, entry: SessionEntryVm) => {
      const index = rows.findIndex((row) => row.relPath === entry.relPath);
      const step = (target: number) => {
        const next = rows[Math.min(Math.max(target, 0), rows.length - 1)];
        if (next !== undefined) {
          event.preventDefault();
          focusRow(next.relPath);
        }
      };
      const isOpen = open.has(entry.relPath);
      switch (event.key) {
        case "ArrowDown":
          step(index + 1);
          break;
        case "ArrowUp":
          step(index - 1);
          break;
        case "Home":
          step(0);
          break;
        case "End":
          step(rows.length - 1);
          break;
        case "ArrowRight":
          if (!entry.isDir) {
            break;
          }
          if (isOpen) {
            // Already open: the right arrow walks INTO the folder, which is
            // the next row exactly when that row is a child of this one.
            if (rows[index + 1]?.parent === entry.relPath) {
              step(index + 1);
            }
          } else {
            event.preventDefault();
            toggle(entry.relPath);
          }
          break;
        case "ArrowLeft":
          if (entry.isDir && isOpen) {
            event.preventDefault();
            toggle(entry.relPath);
          } else if (entry.parent !== "") {
            event.preventDefault();
            focusRow(entry.parent);
          }
          break;
        case "Enter":
          event.preventDefault();
          if (entry.isDir) {
            toggle(entry.relPath);
          } else {
            onOpen(entry);
          }
          break;
        default:
          break;
      }
    },
    [rows, open, focusRow, toggle, onOpen],
  );

  if (entries.length === 0) {
    return <p className="px-2 text-muted-foreground text-xs">{SESSION_TREE_EMPTY}</p>;
  }

  return (
    <div className="flex flex-col gap-1">
      {/* A `div`, not a `ul`: an ARIA tree with a roving tabindex is not a
          list, and `ul`/`li` would fight the pattern. */}
      <div role="tree" aria-label={SESSION_TREE_LABEL} className="flex flex-col">
        {rows.map((entry) => {
          const isOpen = entry.isDir && open.has(entry.relPath);
          // These facts DESCRIBE the row; none is part of its name. A tree
          // row's name is its file — that is what a person navigating by first
          // letter matches against — and `aria-label` replaces the subtree's
          // contribution to the name, so a size rendered only as a child would
          // be visible and unspeakable (the Files pane's finding, applied).
          const key = encodeURIComponent(entry.relPath);
          const sizeId = entry.size === null ? null : `session-size-${key}`;
          const ageId = entry.mtimeMs > 0 ? `session-age-${key}` : null;
          const lockId = entry.locked === null ? null : `session-lock-${key}`;
          const describedBy =
            [sizeId, ageId, lockId].filter((id) => id !== null).join(" ") || undefined;
          return (
            // A `div`, as above: this is the `treeitem` of the tree opened
            // above, and a `li` would need a `ul` the roving tabindex fights.
            <div
              key={entry.relPath}
              ref={(element) => {
                if (element === null) {
                  rowRefs.current.delete(entry.relPath);
                } else {
                  rowRefs.current.set(entry.relPath, element);
                }
              }}
              role="treeitem"
              tabIndex={active === entry.relPath ? 0 : -1}
              aria-level={entry.depth}
              aria-expanded={entry.isDir ? isOpen : undefined}
              aria-label={entry.name}
              aria-describedby={describedBy}
              data-testid={`${SESSION_TREE_ROW_TESTID}-${entry.relPath}`}
              onKeyDown={(event) => onKeyDown(event, entry)}
              onFocus={() => setActiveKey(entry.relPath)}
              className="group flex items-center gap-1 rounded-sm px-2 py-1 hover:bg-accent/50 focus-visible:outline-2 focus-visible:outline-ring"
              style={{ paddingInlineStart: `${(entry.depth - 1) * INDENT_PX + PAD_PX}px` }}
            >
              <Button
                type="button"
                variant="ghost"
                size="sm"
                // Off the tab order: the ROW is what Tab reaches, and this
                // button is the row's own primary gesture rather than a second
                // stop beside it.
                tabIndex={-1}
                className="h-6 min-w-0 flex-1 justify-start gap-1 px-1 font-normal"
                onClick={() => (entry.isDir ? toggle(entry.relPath) : onOpen(entry))}
              >
                {entry.isDir ? (
                  <>
                    {isOpen ? (
                      <ChevronDown aria-hidden="true" className="size-3.5 shrink-0" />
                    ) : (
                      <ChevronRight aria-hidden="true" className="size-3.5 shrink-0" />
                    )}
                    {isOpen ? (
                      <FolderOpen
                        aria-hidden="true"
                        className="size-4 shrink-0 text-muted-foreground"
                      />
                    ) : (
                      <Folder
                        aria-hidden="true"
                        className="size-4 shrink-0 text-muted-foreground"
                      />
                    )}
                  </>
                ) : (
                  <>
                    {/* The chevron's width, kept, so names line up under the
                        folder they are in rather than sliding left. */}
                    <span aria-hidden="true" className="size-3.5 shrink-0" />
                    <SessionFileIcon name={entry.name} />
                  </>
                )}
                <span className="truncate text-sm">{entry.name}</span>
              </Button>

              {entry.locked !== null && (
                <>
                  <Lock aria-hidden="true" className="size-3 shrink-0 text-muted-foreground" />
                  <span id={lockId ?? undefined} className="sr-only">
                    {entry.locked}
                  </span>
                </>
              )}
              {entry.size !== null && (
                <span
                  id={sizeId ?? undefined}
                  className="figures shrink-0 text-muted-foreground text-xs"
                >
                  {entry.size.label}
                </span>
              )}
              {entry.mtimeMs > 0 && (
                <span
                  id={ageId ?? undefined}
                  className="figures w-16 shrink-0 text-right text-muted-foreground text-xs"
                >
                  {formatDraftAge(entry.mtimeMs, nowMs)}
                </span>
              )}
              <SyncStatusMark sync={entry.sync} />

              {/* The row's other verbs. Icon-only and revealed on hover or
                  focus — the tree is narrow, and a session's rows are read far
                  more often than they are acted on. They join the tab order
                  only while their row is the focused one. */}
              {!entry.isDir && (
                <span className="flex shrink-0 items-center gap-0.5 opacity-0 focus-within:opacity-100 group-hover:opacity-100">
                  <Button
                    type="button"
                    variant="ghost"
                    size="icon"
                    tabIndex={active === entry.relPath ? 0 : -1}
                    aria-label={SESSION_TREE_OPEN_EXTERNAL_LABEL}
                    title={SESSION_TREE_OPEN_EXTERNAL_LABEL}
                    className="size-6"
                    onClick={() => {
                      void syncOpenEntry(rootId, entry.subpath).catch(() => undefined);
                    }}
                  >
                    <ExternalLink aria-hidden="true" className="size-3.5" />
                  </Button>
                  {canReveal && (
                    <Button
                      type="button"
                      variant="ghost"
                      size="icon"
                      tabIndex={active === entry.relPath ? 0 : -1}
                      aria-label={SESSION_TREE_REVEAL_LABEL}
                      title={SESSION_TREE_REVEAL_LABEL}
                      className="size-6"
                      onClick={() => {
                        void revealPath(entry.absolutePath).catch(() => undefined);
                      }}
                    >
                      <SquareArrowOutUpRight aria-hidden="true" className="size-3.5" />
                    </Button>
                  )}
                  {/* Live exactly when Rust says the file is deletable, and
                      DISABLED-with-the-reason when it is not (FR-262). Neither
                      state is decided here: `undeletable` is
                      `files::check_deletable`'s own refusal, so the button and
                      the command cannot disagree, and a fifth rule added in
                      Rust arrives with its sentence already written.

                      A disabled button rather than no button, because the two
                      files a person will actually try to delete — `about.md`
                      and `AGENTS.md` — are the ones whose refusal is
                      surprising, and a control that quietly is not there
                      teaches nothing.

                      `locked` is the one case that drops the button entirely:
                      a `workspace/` file already carries the fence's sentence
                      on the same row, and a disabled Delete would say scratch
                      is scratch a second time (UX-DR43). Scratch is also one of
                      the four refusals `check_deletable` states, so this is a
                      choice about which of two identical sentences to show —
                      not a rule re-derived here. */}
                  {entry.locked === null && (
                    <Button
                      type="button"
                      variant="ghost"
                      size="icon"
                      disabled={entry.undeletable !== null}
                      tabIndex={active === entry.relPath ? 0 : -1}
                      aria-label={entry.undeletable ?? `${SESSION_TREE_DELETE_LABEL} ${entry.name}`}
                      title={entry.undeletable ?? SESSION_TREE_DELETE_LABEL}
                      className="size-6 text-muted-foreground hover:text-destructive"
                      onClick={() => {
                        setNotice(null);
                        setDeleting(entry);
                      }}
                    >
                      <Trash2 aria-hidden="true" className="size-3.5" />
                    </Button>
                  )}
                </span>
              )}
            </div>
          );
        })}
      </div>
      {truncated && (
        <p role="status" className="px-2 text-muted-foreground text-xs">
          {SESSION_TREE_TRUNCATED}
        </p>
      )}
      {notice !== null && (
        <p role="status" className="px-2 text-destructive text-xs">
          {notice}
        </p>
      )}

      {/* The confirmation names the file, because "Delete this file?" over a
          tree of forty rows is a question about which one. */}
      <AlertDialog open={deleting !== null} onOpenChange={(next) => !next && setDeleting(null)}>
        <AlertDialogContent>
          <AlertDialogHeader>
            <AlertDialogTitle>{SESSION_TREE_DELETE_TITLE}</AlertDialogTitle>
            <AlertDialogDescription>
              {deleting?.relPath} — {SESSION_TREE_DELETE_BODY}
            </AlertDialogDescription>
          </AlertDialogHeader>
          <AlertDialogFooter>
            <AlertDialogCancel>Cancel</AlertDialogCancel>
            <AlertDialogAction onClick={confirmDelete}>
              {SESSION_TREE_DELETE_LABEL}
            </AlertDialogAction>
          </AlertDialogFooter>
        </AlertDialogContent>
      </AlertDialog>
    </div>
  );
}

/**
 * A file's icon, through the one viewer registry — so a `.csv` in a session
 * looks like a `.csv` in the Files pane. The registry decides; this maps its
 * answer to the shared icon table.
 */
function SessionFileIcon({ name }: { name: string }) {
  const Icon = VIEWER_ICON[resolveViewer({ name, kind: "file" }).icon];
  return <Icon aria-hidden="true" className="size-4 shrink-0 text-muted-foreground" />;
}
