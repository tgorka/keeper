/**
 * The Files primary view — a browser over every synced folder (Story 43.8,
 * FR-153, AD-74, AD-75, AD-65).
 *
 * Epic 42 gave keeper an archive of recordings and a note that knows where its
 * files are. Neither could answer the plainest question a person has about a
 * folder keeper syncs: *what is actually in it?* The recordings live there, the
 * notes vault lives there, and so does everything else the folder holds — and
 * nothing in the app would show it to you.
 *
 * **Read-only by construction, which is a design decision and not an
 * omission.** keeper's whole promise about a synced folder is that it never
 * moves a file you did not ask it to move, and a browser with a delete key in
 * it is the shortest path to breaking that promise by accident (AD-75). There
 * is no rename, no delete, no move, no drag target and no new-folder control in
 * this file, and {@link FILES_WRITE_CONTROL_LABELS} exists so a test can assert
 * that in one line rather than by inspection. The three things a row can do —
 * reveal it, copy its path, hand it to the system's default application — all
 * leave keeper and touch nothing.
 *
 * **The frontend never composes a path** (AD-65). Every call carries a profile
 * id and a `subpath` that Rust itself produced; the join onto the folder root
 * happens once, in `keeper_sync::browse`, which also refuses `..` lexically and
 * refuses a symlink out of the tree after canonicalisation. What arrives here
 * is a relative path to render and an absolute path that is only ever an
 * action's argument.
 *
 * **Lazy, because one of these folders is a pendrive with a hundred thousand
 * files on it.** Mounting lists the profiles and nothing else; a folder's
 * children are asked for the first time it is expanded, and never again unless
 * Refresh asks. Collapsing keeps what was loaded, so re-opening a branch is
 * free.
 *
 * **An absent drive and an empty folder are different facts.** They look
 * identical to the filesystem — an unplugged volume simply has no directory
 * there — and telling someone their recordings folder is empty when their stick
 * is in a drawer is the fastest way to lose their trust in the surface. Rust
 * answers with a `state` and, for everything but `listed`, `entries: null`
 * rather than `[]`, so this pane has to unwrap before it can render "empty" and
 * meets the state on the way.
 *
 * The chrome is {@link SyncPane}'s and {@link RecordingsPane}'s — `<section>` /
 * `<header>` / `<ScrollArea>` — so the non-chat primary views read as one
 * family, and the whole surface is capability-gated on `sync` at the nav entry
 * and at the render chain: where no folder can be synced there is nothing to
 * browse, so the entry is absent rather than empty.
 */

import type { LucideIcon } from "lucide-react";
import {
  ChevronDown,
  ChevronRight,
  FileAudio,
  File as FileIcon,
  FileImage,
  FileVideo,
  Folder,
  FolderOpen,
} from "lucide-react";
import type { KeyboardEvent as ReactKeyboardEvent } from "react";
import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { Alert, AlertDescription } from "@/components/ui/alert";
import { Button } from "@/components/ui/button";
import { ScrollArea } from "@/components/ui/scroll-area";
import type { FilesEntryVm, FilesListingVm, IpcError, SyncProfileVm } from "@/lib/ipc/client";
import { revealPath, syncBrowse, syncOpenEntry, syncProfiles } from "@/lib/ipc/client";
import { useCapabilitiesStore } from "@/lib/stores/capabilities";
import { cn } from "@/lib/utils";

/** The pane's heading, and the accessible name of the surface itself. */
export const FILES_PANE_TITLE = "Files";

/** The one honest sentence under the heading: what this shows, and what it does not do. */
export const FILES_PANE_SUBTITLE =
  "Everything in the folders keeper syncs. Read-only — nothing here moves, renames or deletes a file.";

/** The accessible name of the tree (distinct from the pane's own name). */
export const FILES_TREE_LABEL = "Synced folders";

/** The header control that re-reads every folder already on screen. */
export const FILES_REFRESH_LABEL = "Refresh";

/** What a folder with nothing in it says. The one place "empty" is the truth. */
export const FILES_EMPTY_FOLDER_SENTENCE = "This folder is empty.";

/** What a directory says while its listing is in flight. */
export const FILES_READING_SENTENCE = "Reading…";

/** What the pane says when no profile is configured at all — a different fact
 * from every profile being paused, which is why it is a different sentence. */
export const FILES_NO_PROFILES_SENTENCE =
  "No folders are set up yet. Add one in Sync and it appears here.";

/** What the pane says when every configured profile is paused. A paused folder
 * is not listed: keeper is not watching it, and browsing it would imply it is. */
export const FILES_ALL_PAUSED_SENTENCE = "Every folder is paused. Resume one in Sync to browse it.";

/** The row action labels. Each one reads a file or leaves keeper entirely. */
export const FILES_REVEAL_LABEL = "Reveal in Finder";
export const FILES_COPY_PATH_LABEL = "Copy path";
export const FILES_COPIED_LABEL = "Copied";
export const FILES_OPEN_LABEL = "Open";

/**
 * Every label this surface must never grow, named so a test can assert their
 * absence rather than a reviewer having to notice their arrival.
 *
 * AD-75 is a promise, and a promise that is only kept by everyone remembering
 * it is a promise with a date on it. The next person to add "just a rename" to
 * this pane will fail a test that says why.
 */
export const FILES_WRITE_CONTROL_LABELS = [
  "Rename",
  "Delete",
  "Move",
  "New folder",
  "Trash",
  "Duplicate",
  "Paste",
  "Cut",
  "Upload",
  "Save",
] as const;

/** Test id for one tree row, suffixed with the row's node key. */
export const FILES_ROW_TESTID = "files-row";

/** Test id for the sentence a non-`listed` state produces — the absent drive,
 * the folder that moved, the volume that is not the expected one. Distinct from
 * the empty-folder sentence on purpose: that distinction is the story. */
export const FILES_STATE_DETAIL_TESTID = "files-state-detail";

/** The glyph for each kind of the one attachment vocabulary (AD-73).
 *
 * Keyed on the wire type rather than on `string`, so a kind added to
 * `RecordingNoteTargetKind` fails this file to compile instead of silently
 * rendering nothing. The vocabulary is 43.5's to widen; this is the browser
 * noticing when it does. */
const KIND_ICON: Record<FilesEntryVm["kind"], LucideIcon> = {
  video: FileVideo,
  image: FileImage,
  audio: FileAudio,
  file: FileIcon,
  folder: Folder,
};

/**
 * The key one directory is remembered under: a profile and a subpath.
 *
 * `\u0000` rather than `/` or `:` because both are legal in the strings being
 * joined — a folder called `a/b` cannot exist, but a profile id and a subpath
 * concatenated with `:` can collide with a different pair, and a cache that
 * confuses two directories would show one folder's contents under another
 * folder's name.
 */
function nodeKey(profileId: string, subpath: string): string {
  return `${profileId}\u0000${subpath}`;
}

/** One `treeitem`: a profile root, or one entry inside one. */
interface TreeNodeRow {
  kind: "node";
  /** {@link nodeKey} of this node's own directory-or-file. */
  key: string;
  profileId: string;
  /** Profile-relative; `""` for a profile root. Rust produced it, and it is
   * what goes straight back as the next call's subpath (AD-65). */
  subpath: string;
  name: string;
  /** 1-based, and the only thing carrying depth: the DOM is flat. */
  level: number;
  isFolder: boolean;
  open: boolean;
  /** The node whose expansion revealed this one; `null` for a profile root.
   * Left-arrow climbs it, and Right-arrow uses it to check that the row below
   * really is this folder's child. */
  parentKey: string | null;
  /** `null` for a profile root, which is a folder with no dirent behind it and
   * therefore no reveal / copy / open actions of its own. */
  entry: FilesEntryVm | null;
}

/** One line of prose between rows: "Reading…", the empty-folder sentence, the
 * absent-drive sentence, the truncation notice. Deliberately not a `treeitem` —
 * there is nothing to select or expand, and making it focusable would put dead
 * stops in the arrow path. */
interface TreeNoteRow {
  kind: "note";
  key: string;
  level: number;
  text: string;
  /** The listing state that produced it, or `null` when no listing did (a
   * failure, or a read still in flight). */
  state: string | null;
  /** An ordinary fact rather than an explanation — the empty folder. It carries
   * no state-detail test id, because "empty" is the one answer that is not a
   * reason the folder could not be read. */
  plain?: boolean;
}

type TreeRow = TreeNodeRow | TreeNoteRow;

/** Structural guard for the IpcError envelope surfaced on a rejection. */
function isIpcError(value: unknown): value is IpcError {
  if (typeof value !== "object" || value === null) {
    return false;
  }
  const v = value as Record<string, unknown>;
  return typeof v.code === "string" && typeof v.message === "string";
}

export function FilesPane() {
  // A platform with no user-visible file manager gets no Reveal affordance —
  // the row shows its path as inert text instead, the idiom `SyncFolderPath` and
  // the recordings row already use. A control that fails on activation is worse
  // than no control.
  const canReveal = useCapabilitiesStore((s) => s.capabilities.revealInFileManager);

  const [profiles, setProfiles] = useState<SyncProfileVm[] | null>(null);
  const [expanded, setExpanded] = useState<ReadonlySet<string>>(() => new Set());
  const [listings, setListings] = useState<ReadonlyMap<string, FilesListingVm>>(() => new Map());
  // No `loading` set: a node with no listing yet IS the in-flight state, and
  // the tree already says "Reading…" for it. A second flag tracking the same
  // fact is a second thing to get out of step with the first.
  const [failures, setFailures] = useState<ReadonlyMap<string, string>>(() => new Map());
  const [copied, setCopied] = useState<string | null>(null);
  // The roving tabindex's memory. Only a hint: the row it names may have been
  // collapsed away or dropped by a refresh, in which case `activeKey` below
  // falls back to the first row rather than leaving the tree with no tab stop.
  const [rememberedKey, setRememberedKey] = useState<string | null>(null);
  // Keyboard navigation has to MOVE focus, not merely mark a row active, so the
  // rendered elements are held by key. A ref rather than state: writing one
  // during render must not schedule another.
  const rowRefs = useRef(new Map<string, HTMLDivElement>());

  useEffect(() => {
    let live = true;
    syncProfiles()
      .then((list) => {
        if (live) {
          setProfiles(list);
        }
      })
      .catch(() => {
        // The profile list failing is the whole surface failing; an empty list
        // is the honest rendering, and the Sync pane is where that failure is
        // diagnosed. This pane does not grow a second sync-health readout.
        if (live) {
          setProfiles([]);
        }
      });
    return () => {
      live = false;
    };
  }, []);

  /**
   * Ask Rust for one directory.
   *
   * Always re-asks: the two callers are "expand for the first time" (which
   * checks the cache before calling) and Refresh (which means "ask again"), so
   * a cache check in here would make Refresh a no-op.
   */
  const load = useCallback((profileId: string, subpath: string) => {
    const key = nodeKey(profileId, subpath);

    syncBrowse(profileId, subpath)
      .then((listing) => {
        setListings((prev) => new Map(prev).set(key, listing));
        setFailures((prev) => {
          if (!prev.has(key)) {
            return prev;
          }
          const next = new Map(prev);
          next.delete(key);
          return next;
        });
      })
      .catch((error: unknown) => {
        // The message is composed in Rust to be shown verbatim — it names the
        // folder and the next step — so it is rendered rather than replaced.
        setFailures((prev) =>
          new Map(prev).set(key, isIpcError(error) ? error.message : String(error)),
        );
        setListings((prev) => {
          if (!prev.has(key)) {
            return prev;
          }
          const next = new Map(prev);
          next.delete(key);
          return next;
        });
      });
  }, []);

  const toggle = useCallback(
    (profileId: string, subpath: string) => {
      const key = nodeKey(profileId, subpath);
      const opening = !expanded.has(key);
      setExpanded((prev) => {
        const next = new Set(prev);
        if (opening) {
          next.add(key);
        } else {
          next.delete(key);
        }
        return next;
      });
      // Lazy, and once: the fetch fires on the opening edge only, and only for
      // a directory that has never answered. Collapsing keeps what was loaded,
      // so re-opening a branch costs nothing — and a branch whose last attempt
      // failed is retried, because the retry is the only way back from a folder
      // that was briefly unreadable.
      if (opening && !listings.has(key)) {
        load(profileId, subpath);
      }
    },
    [expanded, listings, load],
  );

  /** Re-read every directory currently open. Nothing else changes: a refresh
   * that collapsed the tree would lose the place the person was looking at. */
  const refresh = useCallback(() => {
    syncProfiles()
      .then(setProfiles)
      .catch(() => undefined);
    for (const key of expanded) {
      const separator = key.indexOf("\u0000");
      load(key.slice(0, separator), key.slice(separator + 1));
    }
  }, [expanded, load]);

  const copyPath = useCallback((absolutePath: string) => {
    // Best effort, and deliberately so: a clipboard the browser refuses is not
    // a reason to show an error, and the path is on screen either way.
    void navigator.clipboard
      ?.writeText(absolutePath)
      .then(() => setCopied(absolutePath))
      .catch(() => undefined);
  }, []);

  // Only enabled profiles are browsable. A paused folder is one keeper is not
  // watching, and listing it would imply otherwise.
  const enabled = useMemo(() => (profiles ?? []).filter((p) => p.enabled), [profiles]);

  let emptySentence: string | null = null;
  if (profiles !== null && enabled.length === 0) {
    emptySentence = profiles.length === 0 ? FILES_NO_PROFILES_SENTENCE : FILES_ALL_PAUSED_SENTENCE;
  }

  /**
   * Every row the tree renders right now, flattened in DOM order.
   *
   * **Flat, not nested, and that is the keyboard model's foundation.** WAI-ARIA
   * allows a tree whose depth is carried by `aria-level` alone, and it is the
   * shape that makes "the next visible item" a single array index rather than a
   * walk. Every arrow key in {@link onRowKeyDown} is one step through this list,
   * so what a person sees and what the keyboard moves through cannot drift.
   *
   * Notes — "Reading…", "this folder is empty", the absent-drive sentence — are
   * rows too, but not `treeitem`s: they are not things you can select or
   * expand, and making them focusable would put dead stops in the arrow path.
   */
  const rows = useMemo<TreeRow[]>(() => {
    const out: TreeRow[] = [];
    const walk = (profileId: string, subpath: string, level: number) => {
      const key = nodeKey(profileId, subpath);
      const failure = failures.get(key);
      if (failure !== undefined) {
        out.push({ kind: "note", key: `${key}\u0001error`, level, text: failure, state: null });
        return;
      }
      const listing = listings.get(key);
      if (listing === undefined) {
        out.push({
          kind: "note",
          key: `${key}\u0001reading`,
          level,
          text: FILES_READING_SENTENCE,
          state: null,
        });
        return;
      }
      // `entries === null` is the load-bearing branch: it is what Rust sends
      // for an absent drive, a foreign volume and a folder that moved, and it
      // is why none of the three can render as "this folder is empty".
      if (listing.entries === null) {
        out.push({
          kind: "note",
          key: `${key}\u0001state`,
          level,
          text: listing.detail ?? FILES_EMPTY_FOLDER_SENTENCE,
          state: listing.state,
        });
        return;
      }
      if (listing.entries.length === 0) {
        out.push({
          kind: "note",
          key: `${key}\u0001empty`,
          level,
          text: FILES_EMPTY_FOLDER_SENTENCE,
          state: listing.state,
          plain: true,
        });
        return;
      }
      for (const entry of listing.entries) {
        const childKey = nodeKey(profileId, entry.relativePath);
        const isFolder = entry.kind === "folder";
        const open = isFolder && expanded.has(childKey);
        out.push({
          kind: "node",
          key: childKey,
          profileId,
          subpath: entry.relativePath,
          name: entry.name,
          level,
          isFolder,
          open,
          parentKey: key,
          entry,
        });
        if (open) {
          walk(profileId, entry.relativePath, level + 1);
        }
      }
      if (listing.detail !== null) {
        out.push({
          kind: "note",
          key: `${key}\u0001capped`,
          level,
          text: listing.detail,
          state: listing.state,
        });
      }
    };
    for (const profile of enabled) {
      const key = nodeKey(profile.id, "");
      const open = expanded.has(key);
      out.push({
        kind: "node",
        key,
        profileId: profile.id,
        subpath: "",
        name: profile.name,
        level: 1,
        isFolder: true,
        open,
        parentKey: null,
        entry: null,
      });
      if (open) {
        walk(profile.id, "", 2);
      }
    }
    return out;
  }, [enabled, expanded, listings, failures]);

  const nodes = useMemo(
    () => rows.filter((row): row is TreeNodeRow => row.kind === "node"),
    [rows],
  );

  /**
   * The one row in the tree that is in the page's tab order.
   *
   * A roving tabindex, because the alternative — every row focusable — makes a
   * folder of two hundred files two hundred Tab presses wide, and a synced
   * folder on a pendrive is exactly the surface someone crosses without a
   * mouse. Tab reaches the tree once; the arrows move inside it.
   *
   * Falls back to the first row whenever the remembered one is no longer
   * visible (its branch was collapsed, or a refresh dropped it), so the tree
   * can never end up with no tab stop at all.
   */
  const activeKey =
    rememberedKey !== null && nodes.some((node) => node.key === rememberedKey)
      ? rememberedKey
      : (nodes[0]?.key ?? null);

  const focusRow = useCallback((key: string) => {
    setRememberedKey(key);
    rowRefs.current.get(key)?.focus();
  }, []);

  /**
   * The tree's keyboard model (WAI-ARIA APG, Tree View).
   *
   * Up/Down step one visible row, Home/End jump to the ends, Right expands a
   * closed folder and then descends into an open one, Left collapses an open
   * folder and otherwise climbs to its parent, and Enter/Space toggles. Right
   * descends only into a row that is genuinely this folder's child, so an open
   * folder whose only content is "this folder is empty" does not quietly move
   * focus to the next sibling and look like it worked.
   */
  const onRowKeyDown = useCallback(
    (event: ReactKeyboardEvent<HTMLDivElement>, node: TreeNodeRow) => {
      const index = nodes.findIndex((candidate) => candidate.key === node.key);
      const step = (target: number) => {
        const next = nodes[Math.min(Math.max(target, 0), nodes.length - 1)];
        if (next !== undefined) {
          event.preventDefault();
          focusRow(next.key);
        }
      };
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
          step(nodes.length - 1);
          break;
        case "ArrowRight":
          if (!node.isFolder) {
            break;
          }
          if (node.open) {
            if (nodes[index + 1]?.parentKey === node.key) {
              step(index + 1);
            }
          } else {
            event.preventDefault();
            setRememberedKey(node.key);
            toggle(node.profileId, node.subpath);
          }
          break;
        case "ArrowLeft":
          if (node.isFolder && node.open) {
            event.preventDefault();
            setRememberedKey(node.key);
            toggle(node.profileId, node.subpath);
          } else if (node.parentKey !== null) {
            event.preventDefault();
            focusRow(node.parentKey);
          }
          break;
        case "Enter":
        case " ":
          if (node.isFolder) {
            event.preventDefault();
            setRememberedKey(node.key);
            toggle(node.profileId, node.subpath);
          }
          break;
        default:
          break;
      }
    },
    [focusRow, nodes, toggle],
  );

  const renderNode = (node: TreeNodeRow) => {
    const active = node.key === activeKey;
    // Only the active row's actions join the tab order, so Tab moves *into* the
    // focused row rather than through every action of every row in the tree.
    const actionTabIndex = active ? 0 : -1;
    const entry = node.entry;
    // A folder shows open or closed; anything else takes the vocabulary's glyph.
    let Icon: LucideIcon = node.open ? FolderOpen : Folder;
    if (!node.isFolder && entry !== null) {
      Icon = KIND_ICON[entry.kind];
    }
    return (
      <div
        key={node.key}
        ref={(element) => {
          if (element === null) {
            rowRefs.current.delete(node.key);
          } else {
            rowRefs.current.set(node.key, element);
          }
        }}
        role="treeitem"
        tabIndex={active ? 0 : -1}
        aria-level={node.level}
        aria-expanded={node.isFolder ? node.open : undefined}
        aria-label={node.name}
        data-testid={`${FILES_ROW_TESTID}-${entry === null ? node.profileId : node.subpath}`}
        onKeyDown={(event) => onRowKeyDown(event, node)}
        onFocus={() => setRememberedKey(node.key)}
        className="flex items-center gap-1 rounded-sm px-2 py-1 hover:bg-accent/50 focus-visible:outline-2 focus-visible:outline-ring"
        style={{ paddingInlineStart: `${(node.level - 1) * 16 + 8}px` }}
      >
        {node.isFolder ? (
          <Button
            type="button"
            variant="ghost"
            size="sm"
            tabIndex={-1}
            className={cn(
              "h-6 min-w-0 flex-1 justify-start gap-1 px-1",
              entry === null ? "font-medium" : "font-normal",
            )}
            onClick={() => toggle(node.profileId, node.subpath)}
          >
            {node.open ? (
              <ChevronDown className="size-3.5 shrink-0" aria-hidden="true" />
            ) : (
              <ChevronRight className="size-3.5 shrink-0" aria-hidden="true" />
            )}
            <Icon className="size-4 shrink-0 text-muted-foreground" aria-hidden="true" />
            <span className="truncate text-sm">{node.name}</span>
          </Button>
        ) : (
          <span className="flex min-w-0 flex-1 items-center gap-1 px-1">
            {/* The chevron column is held open for a file so names line up with
                the folders beside them rather than stepping left. */}
            <span className="size-3.5 shrink-0" aria-hidden="true" />
            <Icon className="size-4 shrink-0 text-muted-foreground" aria-hidden="true" />
            <span className="truncate text-sm">{node.name}</span>
          </span>
        )}
        {entry !== null && (
          <span className="flex shrink-0 items-center gap-1">
            {!node.isFolder && (
              <Button
                type="button"
                variant="ghost"
                size="sm"
                tabIndex={actionTabIndex}
                className="h-6 px-2 text-xs"
                onClick={() => {
                  void syncOpenEntry(node.profileId, entry.relativePath).catch(() => undefined);
                }}
              >
                {FILES_OPEN_LABEL}
              </Button>
            )}
            {canReveal && (
              <Button
                type="button"
                variant="ghost"
                size="sm"
                tabIndex={actionTabIndex}
                className="h-6 px-2 text-xs"
                onClick={() => {
                  void revealPath(entry.absolutePath).catch(() => undefined);
                }}
              >
                {FILES_REVEAL_LABEL}
              </Button>
            )}
            <Button
              type="button"
              variant="ghost"
              size="sm"
              tabIndex={actionTabIndex}
              className="h-6 px-2 text-xs"
              onClick={() => copyPath(entry.absolutePath)}
            >
              {copied === entry.absolutePath ? FILES_COPIED_LABEL : FILES_COPY_PATH_LABEL}
            </Button>
          </span>
        )}
      </div>
    );
  };

  return (
    <section
      aria-label={FILES_PANE_TITLE}
      className="flex min-w-0 flex-1 flex-col border-border border-r bg-background"
    >
      <header className="flex shrink-0 items-start justify-between gap-4 border-border border-b px-6 py-4">
        <div className="min-w-0">
          <h1 className="font-heading font-medium text-lg">{FILES_PANE_TITLE}</h1>
          <p className="text-muted-foreground text-sm">{FILES_PANE_SUBTITLE}</p>
        </div>
        <Button type="button" variant="outline" size="sm" className="shrink-0" onClick={refresh}>
          {FILES_REFRESH_LABEL}
        </Button>
      </header>

      <ScrollArea className="min-h-0 flex-1">
        <div className="px-4 py-3">
          {emptySentence !== null ? (
            <Alert>
              <AlertDescription>{emptySentence}</AlertDescription>
            </Alert>
          ) : (
            <div aria-label={FILES_TREE_LABEL} role="tree" className="flex flex-col">
              {rows.map((row) =>
                row.kind === "node" ? (
                  renderNode(row)
                ) : (
                  <p
                    key={row.key}
                    data-testid={row.plain === true ? undefined : FILES_STATE_DETAIL_TESTID}
                    data-state={row.state ?? undefined}
                    className={cn(
                      "px-2 py-1 text-sm",
                      row.state === null && row.text !== FILES_READING_SENTENCE
                        ? "text-destructive"
                        : "text-muted-foreground",
                    )}
                    style={{ paddingInlineStart: `${(row.level - 1) * 16 + 8}px` }}
                  >
                    {row.text}
                  </p>
                ),
              )}
            </div>
          )}
        </div>
      </ScrollArea>
    </section>
  );
}
