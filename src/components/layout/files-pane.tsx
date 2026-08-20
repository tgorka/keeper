/**
 * The Files primary view — a browser over every synced folder (Story 43.8,
 * FR-153, AD-74, AD-65; the write path is Story 45.3, FR-175, FR-176, AD-89).
 *
 * Epic 42 gave keeper an archive of recordings and a note that knows where its
 * files are. Neither could answer the plainest question a person has about a
 * folder keeper syncs: *what is actually in it?* The recordings live there, the
 * notes vault lives there, and so does everything else the folder holds — and
 * nothing in the app would show it to you.
 *
 * **AD-75 said this pane never writes. AD-89 retired it, deliberately and by
 * the owner, and the reversal is recorded here because this is where the next
 * reader will come looking.** The old rule was right while Files was a window
 * onto the sync engine's world: keeper's promise about a synced folder is that
 * it never moves a file you did not ask it to move, and a browser with a delete
 * key in it is the shortest path to breaking that promise by accident. The
 * owner has now asked it to delete and create, and the honest answer was to
 * give the surface one write path rather than to leave it read-only and watch a
 * second one grow beside it. Three narrower promises replaced the one broad
 * one:
 *
 * 1. **Only inside a notes vault.** A file outside one is listed, viewed and
 *    opened, and the surface *says why* it cannot be changed rather than
 *    offering an action that will fail. The verdict is
 *    `entry.write.writable`/`entry.write.reason`, composed in Rust and carried
 *    on the listing so the control is absent-with-a-reason rather than
 *    present-and-failing.
 * 2. **One writer.** Everything goes through `notes_vault::write_vault_file` +
 *    `mark_dirty`, the path notes and Story 44.16's CSV editor already use, and
 *    every removal goes through `trash_note`, which the reconciler already
 *    understands.
 * 3. **Every destructive act is confirmed, and the confirmation names the
 *    file.** Its sentences are `sync_delete_plan`'s, composed in Rust from the
 *    same code the delete runs, so the dialog cannot promise something the
 *    command then refuses.
 *
 * What still does not exist is {@link FILES_UNBUILT_CONTROL_LABELS}, asserted
 * by name so a half-built Rename cannot arrive quietly. Reveal, copy path and
 * open-with are unchanged and still leave keeper entirely.
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
 * free — within one mount.
 *
 * **Across mounts, only the expansion survives, and it survives on purpose
 * (Story 46.3).** This pane is unmounted whenever another primary view is up,
 * so everything below was thrown away every time somebody looked at Notes.
 * Which folders are open now lives in {@link "@/lib/stores/files-tree"} and
 * comes back; the listings and the failures do not, because a cached listing
 * restored from the last run is a claim about a disk keeper has not looked at.
 * The cost is one `sync_browse` per remembered folder when this pane mounts,
 * which is the honest price of the tree being where you left it.
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
  Check,
  ChevronDown,
  ChevronRight,
  Clapperboard,
  Copy,
  ExternalLink,
  Folder,
  FolderOpen,
  FolderSearch,
  ListChecks,
  NotebookPen,
  Paperclip,
  RefreshCw,
  Trash2,
} from "lucide-react";
import type {
  KeyboardEvent as ReactKeyboardEvent,
  MouseEvent as ReactMouseEvent,
  ReactNode,
  RefCallback,
} from "react";
import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { planPriorityActions } from "@/components/layout/priority-actions";
import { useSurfaceColumn } from "@/components/layout/surface-column";
import { SyncStatusMark } from "@/components/layout/sync-status-mark";
import { ATTACH_TO_NOTE_LABEL, AttachToNoteDialog } from "@/components/notes/attach-to-note-dialog";
import { Alert, AlertDescription } from "@/components/ui/alert";
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
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import {
  ContextMenu,
  ContextMenuContent,
  ContextMenuItem,
  ContextMenuSeparator,
  ContextMenuTrigger,
} from "@/components/ui/context-menu";
import { FullValueButton, useOverflowing } from "@/components/ui/overflow-value";
import { useWindowedRows } from "@/components/ui/window-list";
import { useLongPress } from "@/hooks/use-long-press";
import { SURFACE_COLUMNS } from "@/lib/column-widths";
import { countLabel, ITEMS } from "@/lib/count-label";
import type {
  FilesDeletePlanVm,
  FilesEntryVm,
  FilesFolderRoleVm,
  FilesListingVm,
  IpcError,
  PanelTargetVm,
  SyncProfileVm,
} from "@/lib/ipc/client";
import {
  revealPath,
  syncBrowse,
  syncCreateEntry,
  syncDeleteEntries,
  syncDeletePlan,
  syncOpenEntry,
  syncProfiles,
} from "@/lib/ipc/client";
import { useCapabilitiesStore } from "@/lib/stores/capabilities";
import { columnFoldStore } from "@/lib/stores/column-fold";
import {
  filesTreeStore,
  nodeKey,
  nodeKeyProfile,
  nodeKeySubpath,
  reachableNodeKeys,
  useFilesTree,
} from "@/lib/stores/files-tree";
import { useNotesVaultsStore } from "@/lib/stores/notes-vaults";
import { panelsStore } from "@/lib/stores/panels";
import { cn } from "@/lib/utils";
import { resolveViewer, VIEWER_ICON } from "@/lib/viewers";

/**
 * The height a tree row is assumed to be until it has been mounted once: an
 * `h-6` control inside a `py-1` row.
 *
 * An assumption, not a fact. The tree interleaves prose rows — "this folder is
 * empty", and the sentence Rust composes for a drive that is not plugged in —
 * and those wrap to two and three lines in a narrow pane. The window measures
 * what a row really is on first mount; this is only where it starts.
 */
const FILES_ROW_ESTIMATE = 32;

/** The pane's display name.
 *
 * It reaches the screen through {@link SURFACE_COLUMNS}`["files-tree"].title`
 * and the shared fold chrome, which draws it once at the top of the column and
 * points the section's `aria-labelledby` at it — so the region's name and the
 * heading a reader hears are the same element rather than the same word twice.
 * Still exported: the surface's copy and its tests are written around it. */
export const FILES_PANE_TITLE = "Files";

/** The one honest sentence under the heading: what this shows, and what it can change. */
export const FILES_PANE_SUBTITLE =
  "Everything in the folders keeper syncs. Files in a notes vault can be created and deleted here; everything else is read-only.";

/**
 * The column the tree occupies (Story 48.1).
 *
 * `shrink-0` and a width from {@link useSurfaceColumn}, where it used to be
 * `flex-1`. `flex-1` was never a decision — it was two panes carrying the same
 * class — and it split the surface evenly between a folder list and the
 * document that list opens. The strip beside it is the flexible one now, which
 * is the arrangement Notes has had since Story 46.12.
 */
// `shrink-0` on purpose, for now: Story 55.1 made the Notes columns yield so
// the note panel stops being clipped, and deliberately did not touch this
// surface or Chat. They share the same `PanelStrip` and therefore the same
// symptom — a tree dragged wide starves the document beside it — but changing
// three surfaces on one bug's evidence is how a fix becomes a regression
// somewhere nobody was looking. Tracked in deferred work.
const FILES_COLUMN_CLASS =
  "flex min-w-0 shrink-0 flex-col border-border border-r bg-background last:border-r-0";

/** The accessible name of the tree (distinct from the pane's own name). */
export const FILES_TREE_LABEL = "Synced folders";

/** The header control that re-reads every folder already on screen. */
export const FILES_REFRESH_LABEL = "Refresh";

/**
 * The folded rail's way back to a selection (Story 48.1, second cut).
 *
 * "Selection" and not "Delete": what the strip hides is a selection, and the
 * two things you can do to one — delete it, attach it to a note — are the
 * header's, where the count that makes them safe to press is.
 */
export const FILES_SELECTION_LABEL = "Selection";

/**
 * How many rows are held, said the same way wherever it is said.
 *
 * Two surfaces spell this: the header's count badge, and the folded rail's way
 * back to the selection. They answer about different subsets on purpose — the
 * rail names every selected row, the badge names the ones keeper may delete —
 * so the number differs and the sentence must not. It was written out twice,
 * which is one place too many for a string a test asserts by name.
 *
 * The sentence and not a numeral, because this is the accessible name in both
 * places. The badge draws the figure; a reader who cannot see it is told what
 * the figure counts.
 */
export function filesSelectionSentence(count: number): string {
  return `${countLabel(count, ITEMS)} selected`;
}

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

/**
 * The three things "open" can mean here, worded so a reader can tell which is
 * which (Story 46.13, FR-215, UX-DR77).
 *
 * Three verbs already existed and only one of them had a name. A single click
 * replaced what the active panel was showing, a double click opened a second
 * panel beside it, and the row's own button handed the file to the operating
 * system — and that last one, the only one with a label, was called `Open`,
 * which is the word all three of them deserve. A reader who has never seen this
 * pane cannot discover two of the three, and cannot tell what the third does
 * without pressing it.
 *
 * So the menu names all three, and the button that leaves keeper says the same
 * thing the menu item says. Naming them is most of this fix; the menu is how a
 * name becomes reachable.
 *
 * "Panel" rather than "tab" — the owner's report said tab, and the product has
 * never had one. What is beside the tree is a panel, the store is `panels`, and
 * a menu that invented a second word for it would be teaching the reader a
 * vocabulary the rest of keeper does not use.
 */
export const FILES_OPEN_HERE_LABEL = "Open in this panel";
export const FILES_OPEN_BESIDE_LABEL = "Open in a new panel";
export const FILES_OPEN_LABEL = "Open in the default app";

/** The write controls, and the two sentences the surface uses around them
 * (Story 45.3). Every other word in the confirmation is Rust's. */
export const FILES_DELETE_LABEL = "Delete";
export const FILES_NEW_FILE_LABEL = "New file";
export const FILES_NEW_FILE_NAME_LABEL = "New file name";
export const FILES_CREATE_LABEL = "Create";
export const FILES_CANCEL_LABEL = "Cancel";

/** How the header counts what Delete would act on. A count, because the
 * confirmation is where the files are named. */
export const FILES_SELECTED_TESTID = "files-selection-count";

/** Test id for the delete confirmation's body, so a test reads Rust's
 * sentences rather than re-deriving them. */
export const FILES_CONFIRM_TESTID = "files-delete-confirm";

/** Test id for the sentence a refused write leaves on screen. */
export const FILES_WRITE_ERROR_TESTID = "files-write-error";

/**
 * The write controls this surface does NOT have, named so a test can assert
 * their absence rather than a reviewer having to notice their arrival.
 *
 * **This list used to include Delete, and that was AD-75.** AD-89 retired the
 * rule and Story 45.3 built the two controls the owner asked for; what is left
 * here is the work nobody has done yet, and the assertion is worth keeping for
 * the reason the AD-75 one was: a control that arrives before its command does
 * is a control that fails on click. `Save` left the list when Story 45.6 built
 * it. The next person to add "just a rename" to this pane will fail a test that
 * says so.
 */
export const FILES_UNBUILT_CONTROL_LABELS = [
  "Rename",
  "Move",
  "New folder",
  "Duplicate",
  "Paste",
  "Cut",
  "Upload",
] as const;

/** Test id for one tree row, suffixed with the row's node key. */
export const FILES_ROW_TESTID = "files-row";

/**
 * Test id for the count beside an open folder's name (Story 44.11). A slot, so
 * a test asserts the number rather than re-deriving the sentence.
 */
export const FILES_COUNT_SLOT = "files-entry-count";

/** Test id for the sentence a non-`listed` state produces — the absent drive,
 * the folder that moved, the volume that is not the expected one. Distinct from
 * the empty-folder sentence on purpose: that distinction is the story. */
export const FILES_STATE_DETAIL_TESTID = "files-state-detail";

/** Test id for a row's size cell (Story 45.5). A slot, so a test reads the
 * rendered figure rather than re-deriving the sentence around it. */
export const FILES_SIZE_SLOT = "files-entry-size";

/** Test id for the words behind a configured folder's glyph (Story 45.5). The
 * glyph is the visible form of the same fact; this is the speakable one. */
export const FILES_ROLE_SLOT = "files-entry-role";

/**
 * What the size column means, stated where a person can read it (Story 45.5,
 * FR-178).
 *
 * The base is a visible choice — a 1 048 576-byte file reads "1.0 MB" here and
 * would read "1.0 MiB" under the other convention — so the pane says which one
 * it made rather than leaving the reader to work it out by comparing against
 * Finder. It is on the column, not on every row, because it is one fact about
 * the surface and not a fact about any file.
 */
export const FILES_SIZE_BASE_NOTE = "Sizes are decimal: 1 kB is 1000 bytes, the same as Finder.";

/**
 * The glyph for each of the viewer registry's icon names (Story 45.5, FR-178).
 *
 * **This replaced a `Record<FilesEntryVm["kind"], LucideIcon>` that lived here
 * from 43.8 until 45.5.** That map was keyed on the five-value attachment
 * vocabulary, so every text file, source file, CSV, JSON and PDF in a synced
 * folder drew the same blank page — the pane could already tell a video from an
 * image and could not tell a spreadsheet from an executable.
 *
 * **The table itself moved to `@/lib/viewers` in FR-254**, when the session
 * tree became the second surface drawing file rows: a glyph is a property of
 * the format, so it belongs with the registry that answers every other question
 * about the format, and importing it from a pane would have pulled this file's
 * dialogs and stores into a tree that wanted one icon. Re-exported here so the
 * 45.5-era call sites and their tests keep their import.
 */
export { VIEWER_ICON } from "@/lib/viewers";

/**
 * The glyph for a folder keeper itself put somewhere (Story 45.5, FR-178).
 *
 * An overlay on top of the folder glyph rather than a row in the registry,
 * because a role is not a format: {@link VIEWER_ICON} answers "what is this
 * kind of thing", which is the same answer on every machine, and this answers
 * "what did THIS INSTALLATION configure this folder as". Rust decides it from
 * the profile's `notes.subfolder` and `recordings.subfolder`; nothing here
 * looks at a name, so a vault called `Second Brain` is marked and an ordinary
 * folder called `10-notes` is not.
 *
 * A role beats the open/closed chevron pairing on purpose: which folder is the
 * vault is worth more at a glance than whether it happens to be expanded, and
 * the chevron beside it already says that.
 */
const FOLDER_ROLE_ICON: Record<FilesFolderRoleVm, LucideIcon> = {
  notesVault: NotebookPen,
  recordings: Clapperboard,
};

/** What a role-carrying folder's icon means, for the row's title. Two folders
 * in a list of forty look identical without it. */
const FOLDER_ROLE_TITLE: Record<FilesFolderRoleVm, string> = {
  notesVault: "Your notes vault",
  recordings: "Where recordings are saved",
};

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
  /**
   * How many entries this folder's listing holds, already worded — or `null`
   * for anything that is not an open folder with a listing in hand (Story
   * 44.11).
   *
   * Only an OPEN folder carries one. A listing survives a collapse in
   * `listings`, so a closed folder could show the number it had when it was
   * last read — a count of rows nobody can see, taken at a time nobody can
   * name. A count belongs beside the rows it counts.
   */
  count: string | null;
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

/**
 * The inline "name your new file" row (Story 45.3, FR-176).
 *
 * A row rather than a modal, and rather than a `window.prompt`: it sits inside
 * the folder it will write into, so what the name means is visible while it is
 * being typed. Deliberately not a `treeitem` for the same reason a note row is
 * not — there is nothing to select or expand — and it holds the only text input
 * this pane has.
 */
interface TreeCreateRow {
  kind: "create";
  key: string;
  level: number;
  profileId: string;
  /** The directory the file lands in, profile-relative. Rust joins the name
   * onto it (AD-65). */
  subpath: string;
}

type TreeRow = TreeNodeRow | TreeNoteRow | TreeCreateRow;

/** Structural guard for the IpcError envelope surfaced on a rejection. */
function isIpcError(value: unknown): value is IpcError {
  if (typeof value !== "object" || value === null) {
    return false;
  }
  const v = value as Record<string, unknown>;
  return typeof v.code === "string" && typeof v.message === "string";
}

/**
 * How many entries one folder holds, worded — or `null` where there is nothing
 * honest to say (Story 44.11, FR-166).
 *
 * **The count is of the listing, and it says when the listing is a floor.**
 * `keeper_sync::browse` stops at `LISTING_CAP` entries and reports that it did
 * (`truncated`), a bit that has been on the wire since Story 43.8 and that
 * nothing has ever read. It is exactly the fact this count needs: below the cap
 * the number is the folder, at the cap it is a floor, and `1,000+ items` says
 * so in the number rather than leaving the reader to find the sentence
 * underneath.
 *
 * Counting past the cap was considered and rejected. The reader would have to
 * keep `stat`ing dirents to apply the same exclusion rule to each one, which on
 * the fifty-thousand-entry folder that motivated the cap is fifty thousand
 * syscalls to turn `1,000+` into `48,213` — a worse answer to the same
 * question, since "more than a thousand, open it in Finder" is already what the
 * reader can act on.
 *
 * `null` for every state that is not a listing: an absent drive, a foreign
 * volume, a folder that moved, a read in flight. None of those knows a number,
 * and `0 items` would be a claim about a folder keeper could not open.
 */
function entryCount(listing: FilesListingVm | undefined): string | null {
  if (listing?.entries == null) {
    return null;
  }
  return countLabel(listing.entries.length, ITEMS, { atLeast: listing.truncated });
}

/** What the affordance opens, for its accessible name. Every row's is the
 * same verb; the file's own name is what distinguishes them. */
export const FILES_NAME_LABEL = "file name";

/**
 * How wide one row action's control is, in px.
 *
 * A `size="icon-sm"` ghost `Button`: a 32px square, which is DESIGN.md's
 * load-bearing control height and is what `buttonVariants` draws. Declared
 * rather than measured, and the difference from `PriorityActions` — which
 * refuses a declared table of widths — is the difference between a WORD and a
 * SQUARE. A word's width is a guess about a font, a locale and a text-size
 * setting, and it is wrong on the first machine that disagrees. A square's side
 * is a class.
 */
const FILES_ROW_ACTION_PX = 32;

/** The row's own `gap-1`, in px. The arithmetic below and the layout above read
 *  the same number, so they cannot come to disagree. */
const FILES_ROW_GAP_PX = 4;

/** What one level of nesting costs, in px — the row's `paddingInlineStart`. */
const FILES_ROW_INDENT_PX = 16;

/** The row's `px-2`, in px. Spent twice: once as the base of the indent, once
 *  at the trailing edge. */
const FILES_ROW_PAD_PX = 8;

/**
 * Pixels the row spends before the name: the chevron column (14) the files hold
 * open so their names line up with the folders', the file-type glyph (16), the
 * two 4px gaps between them and the name group's own `px-1`.
 */
const FILES_ROW_GLYPHS_PX = 46;

/**
 * Pixels the row spends after the name and before the actions: the size or count
 * cell at the width a five-character figure takes in the mono face, plus the
 * sync mark and their gaps.
 */
const FILES_ROW_META_PX = 64;

/**
 * Pixels the name is never asked to give up, whatever else wants the row.
 *
 * About fourteen characters at `text-sm`, which is the point below which a file
 * name stops identifying a file — `2026-08-12-not…` still says something,
 * `2026-…` does not. This is what the owner's report was actually about: at 360px
 * the always-mounted `Open` / `Reveal in Finder` / `Copy path` text buttons, the
 * size cell and the sync mark took about 250 of them, so the name was squeezed to
 * roughly 30px on EVERY file row — which is also why every file row grew an
 * Expand trigger and folder rows, having neither a size cell nor an Open button,
 * did not. Folders looked fine because folders were never squeezed.
 */
export const FILES_NAME_FLOOR_PX = 112;

export interface FilesRowBudgetInput {
  /** The tree's own width in px, measured — see `FilesPane`'s viewport effect. */
  readonly column: number;
  /** The row's `aria-level`, 1 for a profile root. */
  readonly level: number;
}

/**
 * Pixels a row may spend on its action controls, which is everything the row has
 * left once the name has been given its floor.
 *
 * The shape `paneHeaderActionsBudget` has, for the same reason: the surface knows
 * what the pixels before the group have been spent on, and
 * {@link planPriorityActions} — the same policy the note header spends, not a
 * second one — decides how many controls that buys.
 *
 * **The reserves are declared and that is safe here, where a declared item width
 * would not be.** Being a few pixels wrong about the size cell costs the name a
 * few pixels; being wrong about a control's width pushes a control off the edge,
 * which is the defect `PriorityActions` was written to end. Nothing is lost
 * either way: every verb is in the row's context menu at every width, so this
 * arithmetic decides which of them are ALSO one click away and never whether
 * they are reachable.
 *
 * Zero, never negative: a row too narrow for any control still has a name, a
 * size and a menu, which is the 220px shape.
 */
export function filesRowActionsBudget({ column, level }: FilesRowBudgetInput): number {
  if (!Number.isFinite(column) || !Number.isFinite(level)) {
    return 0;
  }
  const indent = (level - 1) * FILES_ROW_INDENT_PX + FILES_ROW_PAD_PX;
  return Math.max(
    0,
    column -
      indent -
      FILES_ROW_PAD_PX -
      FILES_ROW_GLYPHS_PX -
      FILES_ROW_META_PX -
      FILES_NAME_FLOOR_PX,
  );
}

/** One of a row's verbs, as the row's own cluster and its menu both need it. */
interface FilesRowAction {
  /** Stable identity, so a promoted control keeps its place as labels change. */
  readonly id: string;
  /** The accessible name, the tooltip, and the words in the menu item. */
  readonly label: string;
  /** The glyph the promoted control draws. */
  readonly icon: LucideIcon;
  /** What the control and the menu item both do. One handler, so they cannot
   *  drift — the rule `PriorityAction` states and this borrows. */
  readonly onSelect: () => void;
}

/**
 * A row's name, plus the way to read it when the tree is narrower than the name
 * is (Story 44.12, FR-168, AD-83).
 *
 * The name fits the tree, truncates when it cannot, and then — and only then —
 * grows a trigger that opens the whole thing. A deeply nested path in a shallow
 * pane is the ordinary case here, and until now the tail of such a name was
 * simply unreadable: the tree has no tooltip and the row does not scroll.
 *
 * A render prop, because the two halves cannot live in one element. The
 * truncating span belongs INSIDE a folder's toggle button — clicking a folder's
 * name is how you open it — while the trigger must be outside it: a button
 * inside a button is not HTML, and its click would toggle the folder on the way
 * past.
 */
function RowName({
  name,
  tabIndex,
  children,
}: {
  name: string;
  /** The row's roving tab index, so only the focused row's affordance is a stop. */
  tabIndex: number;
  children: (ref: (element: HTMLElement | null) => void) => ReactNode;
}) {
  const { ref, overflowing } = useOverflowing();
  return (
    <>
      {children(ref)}
      {overflowing && <FullValueButton name={FILES_NAME_LABEL} value={name} tabIndex={tabIndex} />}
    </>
  );
}

/**
 * The panel target a row names, or `null` for every row that is not a file this
 * pane can open (Story 45.1, AD-90).
 *
 * A profile root has no entry, and a folder's own gesture is expand/collapse — a
 * folder is not a panel target, so opening one must not replace what the panel
 * beside the tree is showing. This is also the predicate for whether a row gets
 * a context menu at all: the menu's three items are the three ways to open a
 * file, and a menu holding none of them would be an empty menu offered on a
 * right-click, which is worse than the native one it suppressed.
 *
 * Narrowed to the `file` variant rather than the whole union: the menu's third
 * item hands the row's own subpath to the operating system, and a caller that
 * had to re-narrow a union it just built would be one `default:` away from
 * handing `syncOpenEntry` a note id.
 */
function rowTarget(node: TreeNodeRow): Extract<PanelTargetVm, { kind: "file" }> | null {
  if (node.isFolder || node.entry === null) {
    return null;
  }
  return {
    kind: "file",
    profileId: node.profileId,
    relativePath: node.entry.relativePath,
  };
}

export function FilesPane() {
  // A platform with no user-visible file manager gets no Reveal affordance —
  // the row shows its path as inert text instead, the idiom `SyncFolderPath` and
  // the recordings row already use. A control that fails on activation is worse
  // than no control.
  const canReveal = useCapabilitiesStore((s) => s.capabilities.revealInFileManager);
  // The tree's own surface column is set up further down, once the selection and
  // the refresh it puts on its folded rail exist (Story 48.1).

  const [profiles, setProfiles] = useState<SyncProfileVm[] | null>(null);
  // Which folders are open outlives this component (Story 46.3): the shell
  // unmounts the pane on every surface switch, so a `useState` here was a set
  // the user rebuilt by hand each time they came back.
  const expanded = useFilesTree((s) => s.expanded);
  // The listings and the failures below stay component state, deliberately.
  // They cache what a directory held, and a cache restored from the last run
  // would be describing a disk keeper has not read since.
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
  // One instance for every row in the tree (Story 46.13). The hook tracks a
  // single press at a time and captures the pressed element per press, which is
  // what its own doc says it is for — a hook per row would be one timer per row
  // in a virtualised list.
  const longPress = useLongPress();

  /**
   * The selection model the rest of the pane reads (Story 45.3, FR-175).
   *
   * Node keys, not paths: a path is only unique within a profile, and the tree
   * shows several profiles at once. Delete acts on this set and the
   * confirmation counts it, which is the whole reason it exists rather than
   * being a per-row Delete button — a per-row button cannot answer "and the
   * other four".
   *
   * **One profile at a time.** Every write command is scoped to one profile
   * id, so a selection spanning two folders is a selection half of which no
   * command can act on. Selecting into a different profile REPLACES rather
   * than extends, which is a rule a person can see happen; silently dropping
   * the other half at delete time is not.
   */
  const [selected, setSelected] = useState<ReadonlySet<string>>(() => new Set());
  // Where a Shift-range starts. Null once the anchor row is gone.
  const [anchorKey, setAnchorKey] = useState<string | null>(null);
  // Rust's own delete plan, plus what the delete will need afterwards. `null`
  // is the dialog closed; there is no separate `open` boolean, because two
  // facts about one dialog is how a dialog ends up open with nothing in it.
  const [pending, setPending] = useState<{
    profileId: string;
    /** Node keys of the folders to re-read once the delete lands. */
    parents: string[];
    plan: FilesDeletePlanVm;
  } | null>(null);
  // The sentence a refused or failed write left behind. Rust's words.
  const [writeError, setWriteError] = useState<string | null>(null);
  // Which folder is being created in, by node key, and what has been typed.
  const [creatingIn, setCreatingIn] = useState<string | null>(null);
  const [createName, setCreateName] = useState("");
  // Whether Story 45.13's chooser is open. A boolean rather than a captured
  // selection, so `sources` is derived live from `selection` on every render
  // rather than frozen at open time.
  //
  // **That matters for exactly one case, and it is not the obvious one.** The
  // chooser is a modal: Radix marks everything outside it `aria-hidden`, so
  // while it is open the tree is inert and the selection cannot be changed by
  // clicking — a test that tries finds no `treeitem` at all. What CAN change it
  // underneath is a background listing refresh, which drops a vanished row out
  // of `selection` (it filters on `entry !== null`) and therefore out of
  // `sources`. A snapshot taken at open time would still be offering that row.
  //
  // Verified by reading rather than by a test, because the modal makes the
  // click path unreachable and the refresh path is not drivable with the dialog
  // mounted. The write-time plan is authoritative regardless, so the worst this
  // can cost is an offer that Rust then refuses with a sentence.
  const [attaching, setAttaching] = useState(false);
  // Which vault a note would be attached to. The Files pane browses every
  // synced folder, but a note lives in the open vault and nowhere else.
  const activeVaultId = useNotesVaultsStore((s) => s.activeVaultId);

  /**
   * Whether `profiles` is a list Rust answered with, rather than the empty list
   * this pane renders when the call failed.
   *
   * The two are indistinguishable in `profiles` itself, and Story 46.3 needs
   * them apart: a profile that is absent from an answer has been deleted, and
   * its remembered folders should go with it — but a profile absent because the
   * sync engine could not be reached has not been deleted at all, and dropping
   * someone's expansion over a call that failed would be a worse defect than
   * the one the restore exists to fix. Empty-because-broken forgets nothing.
   */
  const answered = useRef(false);
  useEffect(() => {
    let live = true;
    syncProfiles()
      .then((list) => {
        if (live) {
          answered.current = true;
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
   * Every directory `load` has already asked Rust about since this pane
   * mounted (Story 48.6).
   *
   * Consulted by ONE caller — the restore below — and by nothing else. `load`
   * itself still always re-asks, because Refresh means "ask again" and a cache
   * check in there would make it a no-op; this only lets the restore skip a
   * folder that has already been asked for on this mount, which is the whole
   * of the duplicate it was producing.
   */
  const requested = useRef<Set<string>>(new Set());

  /**
   * Ask Rust for one directory.
   *
   * Always re-asks: the two callers are "expand for the first time" (which
   * checks the cache before calling) and Refresh (which means "ask again"), so
   * a cache check in here would make Refresh a no-op.
   */
  const load = useCallback((profileId: string, subpath: string) => {
    const key = nodeKey(profileId, subpath);
    requested.current.add(key);

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

  /**
   * Re-ask for the folders that were already open before this pane mounted
   * (Story 46.3).
   *
   * This is the cost of remembering the expansion, paid deliberately and in one
   * place: `sync_browse` once per remembered folder. Only the *reachable* ones —
   * a node whose parent is shut renders nowhere, so browsing for it would buy
   * nothing — and only under a profile that is enabled, because a paused folder
   * is one keeper is not watching and this pane does not list it.
   *
   * Keyed on the profile list rather than on mount, and that ordering is
   * load-bearing: `profiles` is non-null only once `syncProfiles()` has settled,
   * which is a microtask after every mount effect in the tree — including
   * `AppShell`'s `hydrateFilesTree`. So the store is already restored by the
   * time this reads it, without this pane knowing anything about the shell.
   *
   * Once per mount, and not until the list is one Rust answered — see
   * {@link answered}. A profile enabled while this pane is on screen is not a
   * case: Sync is a different primary view, so reaching it unmounts this. A
   * list that arrives late, from a Refresh after a failed first call, is: the
   * effect stays armed until it has a real one.
   *
   * And a folder {@link load} has already been asked about on this mount is
   * skipped (Story 48.6). "Stays armed until it has a real list" and "Refresh
   * re-reads every open folder" were each correct and together they browsed
   * every remembered folder TWICE on the one path that runs both: a first
   * profile call that failed, then the Refresh that is the way back from it.
   * The store is read live here, so by then it holds exactly the folders
   * Refresh has just re-read.
   */
  const restored = useRef(false);
  useEffect(() => {
    if (restored.current || profiles === null || !answered.current) {
      return;
    }
    restored.current = true;
    // A remembered folder under a profile that no longer exists is dropped
    // here, silently. Nothing on screen names it, so there is nothing for the
    // reader to act on — and left alone it would be a cookie key nothing can
    // ever clear and a browse call against a folder keeper has forgotten.
    filesTreeStore.getState().retainProfiles(profiles.map((p) => p.id));
    const browsable = new Set(profiles.filter((p) => p.enabled).map((p) => p.id));
    for (const key of reachableNodeKeys(filesTreeStore.getState().expanded)) {
      const profileId = nodeKeyProfile(key);
      if (browsable.has(profileId) && !requested.current.has(key)) {
        load(profileId, nodeKeySubpath(key));
      }
    }
  }, [profiles, load]);

  const toggle = useCallback(
    (profileId: string, subpath: string) => {
      const key = nodeKey(profileId, subpath);
      const opening = !expanded.has(key);
      filesTreeStore.getState().setNodeOpen(key, opening);
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
      .then((list) => {
        answered.current = true;
        setProfiles(list);
      })
      .catch(() => undefined);
    for (const key of expanded) {
      load(nodeKeyProfile(key), nodeKeySubpath(key));
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
      // The name field goes at the TOP of the folder it will write into, and
      // before the empty-folder branch below: creating the first file in an
      // empty folder is exactly when this is wanted, and returning early on
      // "this folder is empty" would have made it the one place it is missing.
      if (creatingIn === key) {
        out.push({
          kind: "create",
          key: `${key}\u0001create`,
          level,
          profileId,
          subpath,
        });
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
          count: open ? entryCount(listings.get(childKey)) : null,
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
        count: open ? entryCount(listings.get(key)) : null,
      });
      if (open) {
        walk(profile.id, "", 2);
      }
    }
    return out;
  }, [creatingIn, enabled, expanded, listings, failures]);

  const nodes = useMemo(
    () => rows.filter((row): row is TreeNodeRow => row.kind === "node"),
    [rows],
  );

  /**
   * Replace, extend or toggle the selection from one row (Story 45.3).
   *
   * The three gestures a file browser has, and the modifier decides which:
   * plain replaces, Cmd/Ctrl toggles one, Shift takes the run from the anchor.
   * The run is over {@link nodes} — the flat visible order — so what a Shift
   * takes is exactly what a person sees between the two rows, including rows
   * inside an expanded folder and excluding rows inside a collapsed one.
   *
   * Crossing into another profile REPLACES whatever the modifier said, because
   * every write command is scoped to one profile and half a selection no
   * command can act on is worse than a selection that visibly reset.
   */
  const select = useCallback(
    (node: TreeNodeRow, mode: "replace" | "toggle" | "extend") => {
      setSelected((previous) => {
        const crossesProfile = [...previous].some(
          (key) => nodes.find((candidate) => candidate.key === key)?.profileId !== node.profileId,
        );
        if (mode === "replace" || crossesProfile || previous.size === 0) {
          return new Set([node.key]);
        }
        if (mode === "toggle") {
          const next = new Set(previous);
          if (!next.delete(node.key)) {
            next.add(node.key);
          }
          return next;
        }
        const from = nodes.findIndex((candidate) => candidate.key === anchorKey);
        const to = nodes.findIndex((candidate) => candidate.key === node.key);
        if (from < 0 || to < 0) {
          return new Set([node.key]);
        }
        const [low, high] = from <= to ? [from, to] : [to, from];
        return new Set(
          nodes
            .slice(low, high + 1)
            .filter((candidate) => candidate.profileId === node.profileId)
            .map((candidate) => candidate.key),
        );
      });
      // Shift keeps the anchor it is measuring from; the other two move it.
      if (mode !== "extend") {
        setAnchorKey(node.key);
      }
      // A stale refusal from a previous attempt is not about this selection.
      setWriteError(null);
    },
    [anchorKey, nodes],
  );

  /** The selected rows, in the tree's own order, and only those a listing still
   * has an entry for — a row that vanished on a refresh is not silently still
   * selected. */
  const selection = useMemo(
    () => nodes.filter((node) => selected.has(node.key) && node.entry !== null),
    [nodes, selected],
  );

  /**
   * The selected rows keeper will actually delete: the location said yes.
   *
   * A read-only file in the selection contributes nothing to the Delete
   * control and is not silently deleted either — its own row already carries
   * `write.reason`, which is where the explanation belongs. Both questions have
   * to say yes before a write control exists, and this is the location half;
   * the format half is the viewer registry's `entry.writable` (Story 45.2), and
   * a delete does not consult it because removing a PDF is not editing one.
   */
  const deletable = useMemo(
    () => selection.filter((node) => node.entry?.write.writable === true),
    [selection],
  );

  /**
   * The absolute paths of the selected rows a note could hold (Story 45.13,
   * FR-188).
   *
   * A folder is out because there is no element for a directory — the rule
   * Story 43.5 set and `notes_attach_sources` enforces again on the paths it is
   * handed. Deciding it here as well is not belt-and-braces for its own sake:
   * it decides whether the control appears at all, and a control that is always
   * there and always refuses is worse than one that is only there when it works.
   *
   * `write.writable` is deliberately not consulted. That is the LOCATION
   * question — may keeper change this file — and attaching changes the note,
   * not the file. A read-only PDF is a perfectly good thing to put in a note.
   *
   * **`flatMap` rather than `filter` and then `map`, and that is the whole
   * reason this holds paths instead of rows.** `Array.prototype.filter` does
   * not narrow its element type without a type predicate, so a filtered list of
   * rows still reads as `entry: FilesEntryVm | null` downstream and the
   * compiler demands `node.entry?.absolutePath ?? ""` at the call site. That
   * fallback was here, and it was the bad kind: it cannot happen, so no test
   * can reach it, and if it ever did it would hand Rust an empty path to attach
   * rather than nothing at all. Narrowing inside the ternary means the
   * impossible case has no value to fabricate — there is no `??` to get wrong.
   */
  const attachablePaths = useMemo(
    () =>
      selection.flatMap((node) =>
        node.entry !== null && node.entry.kind !== "folder" ? [node.entry.absolutePath] : [],
      ),
    [selection],
  );

  /**
   * The tree is a surface column: it folds away and it can be dragged wider
   * (Story 48.1). Folded it keeps a rail, because the fold takes the header
   * with the body and the header is where everything this pane can DO lives.
   *
   * Refresh is the one that works at 48px unchanged — it re-reads the folders
   * into a store the folded pane is still subscribed to, and unfolding shows
   * what it found. The selection entry is the other kind: Delete and Attach are
   * asked about a selection this strip cannot show, so it says how many rows are
   * still selected and gives them back their header. Without it a fold is a
   * selection you can neither see nor act on and cannot even clear.
   */
  const tree = useSurfaceColumn("files-tree", {
    rail: [
      { id: "refresh", icon: RefreshCw, label: FILES_REFRESH_LABEL, onSelect: refresh },
      ...(selection.length > 0
        ? [
            {
              id: "selection",
              icon: ListChecks,
              label: FILES_SELECTION_LABEL,
              detail: filesSelectionSentence(selection.length),
              count: selection.length,
              onSelect: () => columnFoldStore.getState().toggleColumn("files-tree"),
            },
          ]
        : []),
    ],
  });

  /**
   * How wide a tree row actually is, in px — the number
   * {@link filesRowActionsBudget} spends.
   *
   * The `tree` element and not the column: it is the box the rows are laid out
   * in, so `clientWidth` has already had the pane's gutters and the vertical
   * scrollbar taken out of it. One observer for the whole tree rather than one
   * per row, for `window-list`'s reason: every row is the same width by
   * construction, and the tree mounts and unmounts a screenful of them on every
   * scroll.
   *
   * A callback ref with a cleanup rather than an effect, which is the idiom
   * `useWindowedRows` attaches its own viewport with: the element comes and goes
   * with the empty state and with the fold, and a ref runs when it does. Zero is
   * never published — a platform that reports no layout at all (jsdom) gets the
   * width this column has when nobody has dragged it, so a test sees the shape a
   * fresh install has rather than the 48px one, exactly as that hook assumes a
   * viewport height rather than rendering no rows.
   */
  const [treeWidth, setTreeWidth] = useState(SURFACE_COLUMNS["files-tree"].defaultWidth);
  const attachTree = useCallback<RefCallback<HTMLElement>>((element) => {
    if (element === null) {
      return;
    }
    const read = () => {
      const width = element.clientWidth;
      setTreeWidth(width > 0 ? width : SURFACE_COLUMNS["files-tree"].defaultWidth);
    };
    read();
    if (typeof ResizeObserver === "undefined") {
      return;
    }
    // A seam drag moves no state this pane reads, so nothing here would
    // re-render and the read above would never run again.
    const observer = new ResizeObserver(read);
    observer.observe(element);
    return () => observer.disconnect();
  }, []);

  /**
   * Ask Rust what deleting these rows would do, then show its answer.
   *
   * Nothing is deleted here. The plan is a separate call from the delete on
   * purpose: it is built by the same code the delete runs, so the dialog cannot
   * name a file the command will then refuse — and a file that vanished since
   * the listing is named as a refusal rather than dropped in silence.
   *
   * **Takes its targets rather than reading the selection**, because the Delete
   * key can select a row and delete it in one keystroke, and a callback reading
   * `selection` would read the selection from before that keystroke. The
   * profile and the folders to re-read afterwards are captured here for the
   * same reason: by the time the person presses Confirm the tree may have
   * re-rendered under them.
   */
  const requestDelete = useCallback((targets: readonly TreeNodeRow[]) => {
    const first = targets[0];
    if (first === undefined) {
      return;
    }
    const profileId = first.profileId;
    // The parents of everything that is going, so the tree re-reads exactly the
    // folders whose contents changed rather than the whole open tree.
    const parents = [...new Set(targets.map((node) => node.parentKey ?? nodeKey(profileId, "")))];
    setWriteError(null);
    syncDeletePlan(
      profileId,
      targets.map((node) => node.subpath),
    )
      .then((plan) => setPending({ profileId, parents, plan }))
      .catch((error: unknown) => {
        setWriteError(isIpcError(error) ? error.message : String(error));
      });
  }, []);

  /** Carry out the delete the plan described, then re-read what changed. */
  const confirmDelete = useCallback(() => {
    if (pending === null) {
      return;
    }
    const { profileId, parents, plan } = pending;
    setPending(null);
    syncDeleteEntries(profileId, plan.files)
      .then((receipt) => {
        setSelected(new Set());
        setAnchorKey(null);
        // A panel holding a file the user just threw away must not survive to
        // explain that keeper cannot find it (Story 45.1). This is the one
        // unresolvable target that is NOT "render the reason and keep the
        // place": the reason is that they deleted it, and they know.
        for (const relativePath of receipt.deleted) {
          panelsStore.getState().closeTarget({ kind: "file", profileId, relativePath });
        }
        // Named, not swallowed: a selection that only partly went is the case a
        // person most needs told about, and the receipt already carries Rust's
        // sentence per path.
        setWriteError(
          receipt.refusals.length === 0
            ? null
            : receipt.refusals.map((refusal) => refusal.reason).join(" "),
        );
        for (const key of parents) {
          const separator = key.indexOf("\u0000");
          load(key.slice(0, separator), key.slice(separator + 1));
        }
      })
      .catch((error: unknown) => {
        setWriteError(isIpcError(error) ? error.message : String(error));
      });
  }, [load, pending]);

  /** Create the named file in the folder the inline row belongs to, then
   * re-read that folder so the new file appears where it was made. */
  const createFile = useCallback(
    (profileId: string, subpath: string) => {
      setWriteError(null);
      syncCreateEntry(profileId, subpath, createName)
        .then(() => {
          setCreatingIn(null);
          setCreateName("");
          load(profileId, subpath);
        })
        .catch((error: unknown) => {
          // The row stays open with what was typed still in it: a refused name
          // is a name to edit, and clearing the field would make the user
          // retype the part that was fine.
          setWriteError(isIpcError(error) ? error.message : String(error));
        });
    },
    [createName, load],
  );

  /** Where a row key sits in the flat render order — the window works in
   * indices, and the keyboard model works in keys. */
  const positions = useMemo(() => new Map(rows.map((row, index) => [row.key, index])), [rows]);

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

  const getKey = useCallback((index: number) => rows[index]?.key ?? String(index), [rows]);
  const list = useWindowedRows({
    count: rows.length,
    getKey,
    rowHeight: FILES_ROW_ESTIMATE,
    // The tab stop stays mounted at any scroll position. A tree scrolled away
    // from its one `tabIndex=0` row would otherwise have none, and Tab would
    // walk straight past the whole tree — the exact way windowing destroys a
    // roving tabindex without breaking a single visible thing.
    pinnedIndex: activeKey === null ? undefined : positions.get(activeKey),
    onReveal: (index) => {
      const key = rows[index]?.key;
      if (key !== undefined) {
        rowRefs.current.get(key)?.focus();
      }
    },
  });

  /**
   * Move focus to one row, wherever it is in the tree.
   *
   * The row may not be in the DOM: Home from the bottom of a thousand-file
   * folder targets a row a thousand positions away. `reveal` scrolls it into
   * the window, mounts it, and only then runs the focus above — which is why
   * this does not simply reach into `rowRefs` and hope.
   */
  const focusRow = useCallback(
    (key: string) => {
      setRememberedKey(key);
      const index = positions.get(key);
      if (index === undefined) {
        return;
      }
      list.reveal(index);
    },
    [list.reveal, positions],
  );

  /**
   * The tree's keyboard model (WAI-ARIA APG, Tree View, multi-select).
   *
   * Up/Down step one visible row, Home/End jump to the ends, Right expands a
   * closed folder and then descends into an open one, Left collapses an open
   * folder and otherwise climbs to its parent, and Enter/Space toggles. Right
   * descends only into a row that is genuinely this folder's child, so an open
   * folder whose only content is "this folder is empty" does not quietly move
   * focus to the next sibling and look like it worked.
   *
   * Story 45.3 added the selection half, APG's own spelling for a multi-select
   * tree: Shift with Up/Down extends the run while it moves, Space selects the
   * focused row, Ctrl/Cmd-Space adds it to what is already selected, Delete or
   * Backspace asks to delete the selection, and Escape clears it. **A pane that
   * can only be deleted from with a mouse is a pane whose keyboard model was
   * finished and then not extended** — this tree went to some trouble for the
   * arrows and it would be strange to stop here.
   *
   * Delete opens the confirmation and never deletes: there is no keystroke in
   * this pane that removes a file without Rust naming it first.
   */
  const onRowKeyDown = useCallback(
    (event: ReactKeyboardEvent<HTMLDivElement>, node: TreeNodeRow) => {
      const index = nodes.findIndex((candidate) => candidate.key === node.key);
      const step = (target: number) => {
        const next = nodes[Math.min(Math.max(target, 0), nodes.length - 1)];
        if (next !== undefined) {
          event.preventDefault();
          focusRow(next.key);
          if (event.shiftKey) {
            select(next, "extend");
          }
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
          if (node.isFolder) {
            event.preventDefault();
            setRememberedKey(node.key);
            toggle(node.profileId, node.subpath);
          }
          break;
        case " ":
          // Space is the selection key on a multi-select tree. It keeps the
          // folder toggle it has had since 43.8 only when it carries no
          // modifier, so the two meanings never collide.
          event.preventDefault();
          if (event.metaKey || event.ctrlKey) {
            select(node, "toggle");
          } else if (node.isFolder) {
            setRememberedKey(node.key);
            toggle(node.profileId, node.subpath);
          } else {
            select(node, "replace");
          }
          break;
        case "Delete":
        case "Backspace": {
          event.preventDefault();
          // A row that is not in the selection is what the person means by
          // pressing Delete on it, so it becomes the selection and the
          // confirmation is asked about it — one keystroke, one meaning.
          const targets = selected.has(node.key) ? selection : [node];
          if (!selected.has(node.key)) {
            select(node, "replace");
          }
          requestDelete(targets);
          break;
        }
        case "Escape":
          if (selected.size > 0) {
            event.preventDefault();
            setSelected(new Set());
            setAnchorKey(null);
          }
          break;
        default:
          break;
      }
    },
    [focusRow, nodes, requestDelete, select, selected, selection, toggle],
  );

  /**
   * What a row's own click means (Story 45.1, AD-90).
   *
   * `null` for every row that is not a file this pane can open: a profile root
   * has no entry, and a folder's click is expand/collapse — a folder is not a
   * panel target, so clicking one must not replace what the panel beside the
   * tree is showing.
   *
   * Two guards, both of them the difference between a gesture and a surprise:
   *
   * - A modifier click belongs to the selection model (Story 45.3), never to
   *   the panel. Somebody assembling a five-file selection to delete does not
   *   want five panels, and the last Shift-click of a range is not the file
   *   they were looking at.
   * - A click that landed on one of the row's own controls — the folder toggle,
   *   Open, Reveal, Copy path — is that control's click. It bubbles to the row,
   *   and without this every Copy path would also open a panel.
   */
  const clickTarget = useCallback(
    (event: ReactMouseEvent<HTMLDivElement>, node: TreeNodeRow): PanelTargetVm | null => {
      if (event.metaKey || event.ctrlKey || event.shiftKey) {
        return null;
      }
      if (event.target instanceof Element && event.target.closest("button") !== null) {
        return null;
      }
      return rowTarget(node);
    },
    [],
  );

  /**
   * Single click: select, and show this file in the panel beside the tree.
   *
   * Two things at once because they are one gesture. The selection branch runs
   * for every click, including the modified ones; the panel branch runs only
   * for an unmodified click on a file, which is {@link clickTarget}'s rule.
   * That split is the point: a person building a five-file selection to delete
   * does not want five panels, and the last Shift-click is not the file they
   * meant to open.
   *
   * A click that landed on one of the row's own buttons changes neither —
   * Copy path is not a selection gesture.
   */
  const handleRowClick = useCallback(
    (event: ReactMouseEvent<HTMLDivElement>, node: TreeNodeRow) => {
      if (!(event.target instanceof Element && event.target.closest("button") !== null)) {
        let mode: "replace" | "toggle" | "extend" = "replace";
        if (event.metaKey || event.ctrlKey) {
          mode = "toggle";
        } else if (event.shiftKey) {
          mode = "extend";
        }
        select(node, mode);
      }
      const target = clickTarget(event, node);
      if (target !== null) {
        panelsStore.getState().setActiveTarget(target);
      }
    },
    [clickTarget, select],
  );

  /** Double click: open this file BESIDE what is already open. The single click
   *  that necessarily preceded it is undone by the store, so the file that was
   *  showing comes back rather than being replaced by a second copy of this one. */
  const handleRowDoubleClick = useCallback(
    (event: ReactMouseEvent<HTMLDivElement>, node: TreeNodeRow) => {
      const target = clickTarget(event, node);
      if (target !== null) {
        panelsStore.getState().openPanel(target);
      }
    },
    [clickTarget],
  );

  const renderNode = (node: TreeNodeRow) => {
    const active = node.key === activeKey;
    // Only the active row's actions join the tab order, so Tab moves *into* the
    // focused row rather than through every action of every row in the tree.
    const actionTabIndex = active ? 0 : -1;
    const entry = node.entry;
    // The glyph, from one table and never from an extension (Story 45.5,
    // FR-178, AD-87).
    //
    // A configured role wins over everything: "which of these forty folders is
    // my vault" is worth more at a glance than whether that folder happens to
    // be expanded, and the chevron immediately beside it already says the
    // second thing. Otherwise a folder shows open or closed, and every file
    // asks the viewer registry — the only thing that knows what renders a
    // `.csv`, and therefore the only thing that should say what a `.csv` looks
    // like. Before this story a local table keyed on the five-value attachment
    // vocabulary drew a blank page for every text file, source file, CSV, JSON
    // and PDF alike.
    const role = entry?.folderRole ?? null;
    let Icon: LucideIcon;
    if (role !== null) {
      Icon = FOLDER_ROLE_ICON[role];
    } else if (node.isFolder || entry === null) {
      Icon = node.open ? FolderOpen : Folder;
    } else {
      Icon = VIEWER_ICON[resolveViewer({ name: entry.name, kind: entry.kind }).icon];
    }
    // Derived from the row's own key, which is already unique per row and
    // stable across a re-render. Percent-encoded because the key joins two
    // user-supplied strings with a NUL, and an `aria-describedby` value is a
    // space-separated list of ids — a raw path with a space in it would name
    // two ids, neither of which exists.
    const countId = `files-count-${encodeURIComponent(node.key)}`;
    const sizeId = `files-size-${encodeURIComponent(node.key)}`;
    const roleId = `files-role-${encodeURIComponent(node.key)}`;
    // Everything that DESCRIBES the row, in the order a person would say it.
    // Each of these is visible-but-unspeakable on its own: `aria-label` on the
    // row replaces its subtree's contribution to the name, so a size or a
    // vault marker rendered only as a child would be on screen and absent from
    // the accessibility tree entirely (Story 44.11's finding, applied to two
    // more facts).
    const describedBy =
      [
        node.count === null ? null : countId,
        entry?.size == null ? null : sizeId,
        role === null ? null : roleId,
      ]
        .filter((id) => id !== null)
        .join(" ") || undefined;
    // Story 46.13, FR-215: what this row can be opened as. Derived once — the
    // menu's items and the row's own click gestures must be about the same
    // target or the two gestures mean different things on the same row.
    const target = rowTarget(node);
    /**
     * Every verb this row has, in the order it matters.
     *
     * A profile root has none: it is a configured folder, and what can be done
     * to one is the Sync pane's business. Everything else has at least Copy
     * path — the row's own text, which is worth having on a clipboard whatever
     * else the platform will or will not do.
     */
    const actions: readonly FilesRowAction[] =
      entry === null
        ? []
        : [
            // A file's primary verb, and the one that leaves keeper. Absent on a
            // folder, whose own gesture is expand/collapse.
            ...(node.isFolder
              ? []
              : [
                  {
                    id: "open",
                    label: FILES_OPEN_LABEL,
                    icon: ExternalLink,
                    onSelect: () => {
                      void syncOpenEntry(node.profileId, entry.relativePath).catch(() => undefined);
                    },
                  },
                ]),
            // `FolderSearch` and not `FolderOpen`, which is the glyph
            // `properties-panel` gives this same verb. There the glyph decorates
            // a menu item that also spells the words; here it IS the control, on
            // a row whose own leading glyph is `FolderOpen` for every expanded
            // folder in the tree — two identical marks in one row read as one
            // mark drawn twice, which is half of what the owner photographed.
            ...(canReveal
              ? [
                  {
                    id: "reveal",
                    label: FILES_REVEAL_LABEL,
                    icon: FolderSearch,
                    onSelect: () => {
                      void revealPath(entry.absolutePath).catch(() => undefined);
                    },
                  },
                ]
              : []),
            {
              id: "copy",
              // The confirmation is the label and the glyph together, because the
              // control has no words on it to change: a tick says the press
              // landed, and the name says so to everyone who cannot see it.
              label: copied === entry.absolutePath ? FILES_COPIED_LABEL : FILES_COPY_PATH_LABEL,
              icon: copied === entry.absolutePath ? Check : Copy,
              onSelect: () => copyPath(entry.absolutePath),
            },
          ];
    // How many of them are on the row, as against only in its menu. The note
    // header's own policy, applied to a narrower row: a PREFIX of the list, so a
    // verb is out here only if everything above it is, and the cluster never
    // reorders itself as the seam is dragged.
    //
    // Every promoted control is charged its own gap, which over-reserves the last
    // one by 4px — the group is the row's final child, so there is nothing to its
    // right. Four pixels in the name's favour is the safe direction.
    const promoted = planPriorityActions({
      available: filesRowActionsBudget({ column: treeWidth, level: node.level }),
      reserved: 0,
      widths: actions.map(() => FILES_ROW_ACTION_PX),
      gap: FILES_ROW_GAP_PX,
    });
    const row = (
      <div
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
        // Story 45.3: the selection is a fact about the row, so it is on the
        // row rather than a class the assistive layer cannot see. Every node
        // carries it — a tree that marked only the selected rows would leave a
        // screen reader unable to say "not selected" about the others.
        aria-selected={selected.has(node.key)}
        // These facts DESCRIBE the row; none is part of its name (Story 44.11,
        // Story 45.5). A tree row's name is the folder — that is what a person
        // navigating by first letter is matching against, and folding a count,
        // a size and a vault marker into it would make "Vault" stop being the
        // row called Vault. The description is where supplementary facts
        // belong, and `aria-label` would otherwise swallow all three: it
        // replaces the subtree's contribution to the name, so anything rendered
        // only as a child would be visible and unspeakable.
        aria-describedby={describedBy}
        data-testid={`${FILES_ROW_TESTID}-${entry === null ? node.profileId : node.subpath}`}
        onKeyDown={(event) => onRowKeyDown(event, node)}
        onFocus={() => setRememberedKey(node.key)}
        onClick={(event) => handleRowClick(event, node)}
        onDoubleClick={(event) => handleRowDoubleClick(event, node)}
        // Story 46.13: the phone tier's way into the very same menu the
        // right-click opens — a ≥500ms stationary press dispatches the synthetic
        // `contextmenu` the Radix trigger below is already listening for. Spread
        // only on a row that HAS a menu — which is now every row with an entry,
        // folders included, because the menu is where the verbs the row is too
        // narrow to show have gone. A profile root still has none, so a long
        // press on one is not swallowed by the click suppressor for a menu that
        // never opens. Off the phone tier every one of these is a no-op.
        {...(entry === null ? {} : longPress)}
        className={cn(
          "flex items-center gap-1 rounded-sm px-2 py-1 hover:bg-accent/50 focus-visible:outline-2 focus-visible:outline-ring",
          selected.has(node.key) && "bg-accent",
        )}
        style={{
          paddingInlineStart: `${(node.level - 1) * FILES_ROW_INDENT_PX + FILES_ROW_PAD_PX}px`,
        }}
      >
        <RowName name={node.name} tabIndex={actionTabIndex}>
          {(nameRef) =>
            node.isFolder ? (
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
                <span ref={nameRef} className="truncate text-sm">
                  {node.name}
                </span>
              </Button>
            ) : (
              <span className="flex min-w-0 flex-1 items-center gap-1 px-1">
                {/* The chevron column is held open for a file so names line up with
                the folders beside them rather than stepping left. */}
                <span className="size-3.5 shrink-0" aria-hidden="true" />
                <Icon className="size-4 shrink-0 text-muted-foreground" aria-hidden="true" />
                <span ref={nameRef} className="truncate text-sm">
                  {node.name}
                </span>
              </span>
            )
          }
        </RowName>
        {/* What a role-carrying folder IS, for anyone who cannot see the glyph
            (Story 45.5). The icon alone answers "which of these forty folders
            is my vault" only for a sighted reader; this is the same answer for
            everyone else, and it is `sr-only` rather than visible because the
            glyph is already the visible form of it. */}
        {role !== null && (
          <span id={roleId} data-slot={FILES_ROLE_SLOT} className="sr-only">
            {FOLDER_ROLE_TITLE[role]}
          </span>
        )}
        {/* How many entries this open folder holds (Story 44.11, FR-166).
            The listing's own count, not the number of rows the window mounted
            under it — the tree is virtualised (Story 44.10), so what is in the
            DOM below this row is a screenful whatever the folder holds. */}
        {node.count !== null && (
          <span
            id={countId}
            data-slot={FILES_COUNT_SLOT}
            // Mono, because this is a column: the counts are read down the tree
            // against each other, and they only line up if a 1 is as wide as a 7.
            className="shrink-0 font-mono text-muted-foreground text-xs"
          >
            {node.count}
          </span>
        )}
        {/* How big this file is (Story 45.5, FR-178).
            The label is Rust's — `keeper_core::size::format_file_size`, decimal
            — so this pane, a note embed and the unknown viewer cannot come to
            print different numbers for the same bytes. Nothing here divides
            anything.

            A DIRECTORY HAS NO `size` AND SO RENDERS NOTHING. Not "0 B", not a
            dash: a folder showing a zero is a false claim about every folder
            that has anything in it, and the absence is carried as a `null` on
            the wire precisely so this cell cannot accidentally say it. */}
        {entry?.size != null && (
          <span
            id={sizeId}
            data-slot={FILES_SIZE_SLOT}
            title={`${entry.size.bytes} bytes. ${FILES_SIZE_BASE_NOTE}`}
            // A column of sizes, set in the register's face for the same reason
            // the count above it is.
            className="shrink-0 font-mono text-muted-foreground text-xs"
          >
            {entry.size.label}
          </span>
        )}
        {/* Between the name and the actions: what this file's sync state is.
        Never focusable — see {@link SyncStatusMark}. A profile root has no
        entry of its own and takes no mark; its children answer for themselves. */}
        {entry !== null && <SyncStatusMark sync={entry.sync} />}
        {/* Create a file here (Story 45.3, FR-176, AD-89).

            On an OPEN folder only, because the answer to "may keeper write in
            here" is the folder's own listing and a closed folder has none — and
            because a person creating a file in a folder they are not looking
            into is a person about to be surprised by where it went.

            Absent rather than disabled when the location is not writable: the
            listing's `write.reason` says why, and a disabled button with no
            sentence beside it is the failure this whole field exists to
            prevent. */}
        {node.open && (listings.get(node.key)?.write.writable ?? false) && (
          <Button
            type="button"
            variant="ghost"
            size="xs"
            tabIndex={actionTabIndex}
            className="shrink-0"
            onClick={() => {
              setWriteError(null);
              setCreateName("");
              setCreatingIn(node.key);
            }}
          >
            {FILES_NEW_FILE_LABEL}
          </Button>
        )}
        {/* The verbs this row is wide enough to show (see `promoted`).

            Icons, not words. Three text buttons — `Open`, `Reveal in Finder`,
            `Copy path` — were about 250px of a 360px column, so the name they
            sat beside was squeezed to roughly 30px on every file row in the
            tree. The word is gone from the surface and from nowhere else: it is
            still the accessible name and still the pointer's tooltip, and it is
            still spelled in full in the row's menu, which is where a reader goes
            to find out what a row can do. Story 48.9's treatment, applied to the
            one cluster it did not reach.

            An empty cluster is not rendered at all rather than rendered empty:
            at the column's 220px floor nothing promotes, and a `flex` box with
            no children still spends the row's gap. */}
        {actions.length > 0 && promoted > 0 && (
          <span className="flex shrink-0 items-center gap-1">
            {actions.slice(0, promoted).map(({ id, label, icon: Icon, onSelect }) => (
              <Button
                key={id}
                type="button"
                variant="ghost"
                size="icon-sm"
                tabIndex={actionTabIndex}
                // The whole visible word as the name rather than a description of
                // it, so speech input can ask for what the menu spells even
                // though the eye reads a picture (WCAG 2.5.3).
                aria-label={label}
                title={label}
                onClick={onSelect}
              >
                <Icon aria-hidden="true" />
              </Button>
            ))}
          </span>
        )}
      </div>
    );
    // A profile root has no verbs, so it gets no menu: an empty menu offered on
    // a right-click is worse than the native one it suppressed.
    if (actions.length === 0) {
      return row;
    }
    // Story 46.13, FR-215, UX-DR77. The house pattern, verbatim: one Radix
    // `ContextMenu` whose trigger is the row itself (`asChild`, so the DOM the
    // tree and the virtualiser see is unchanged), paired with `useLongPress` for
    // the phone tier — the same construction as `chat-row`, `favorites-section`,
    // `networks-group` and `pins-strip`. Not a fifth idiom.
    //
    // The first three items are the three verbs the pane already had and only
    // ever named one of: a single click replaced the active panel, a double click
    // opened a second one, and the row's button left keeper. Two of those were
    // undiscoverable and the third was called `Open`. The menu is where a reader
    // finds out that this row does three different things, so the wording is the
    // deliverable and the menu is the surface that carries it.
    //
    // **Reveal in Finder and Copy path are down here too, and this menu is now on
    // folder rows as well.** The row shows as many of its verbs as it has pixels
    // for and no more, so the menu has to hold ALL of them or a narrow column
    // would make one unreachable — and a folder, which never had a menu because
    // it is not a panel target, is exactly the row whose two verbs would go
    // missing first. The list does not change with the column's width: a menu
    // whose contents moved as the seam was dragged would be unlearnable, and a
    // verb that is also one click away is not a verb worth hiding.
    //
    // The rules separate what happens in this window from what leaves it and then
    // from what only names the file: each one is the sentence "and now something
    // different".
    return (
      <ContextMenu>
        <ContextMenuTrigger asChild>{row}</ContextMenuTrigger>
        <ContextMenuContent>
          {target !== null && (
            <>
              <ContextMenuItem onSelect={() => panelsStore.getState().setActiveTarget(target)}>
                {FILES_OPEN_HERE_LABEL}
              </ContextMenuItem>
              <ContextMenuItem onSelect={() => panelsStore.getState().openPanel(target)}>
                {FILES_OPEN_BESIDE_LABEL}
              </ContextMenuItem>
              <ContextMenuSeparator />
            </>
          )}
          {actions.map((action) => (
            <ContextMenuItem key={action.id} onSelect={action.onSelect}>
              {action.label}
            </ContextMenuItem>
          ))}
        </ContextMenuContent>
      </ContextMenu>
    );
  };

  /** The inline name field, rendered where the file will land. */
  const renderCreate = (row: TreeCreateRow) => (
    <div
      className="flex items-center gap-2 px-2 py-1"
      style={{ paddingInlineStart: `${(row.level - 1) * 16 + 8}px` }}
    >
      <input
        // Autofocused because the row exists only in response to pressing New
        // file: the next thing the person does is type, and a field they have
        // to click first is a field that reads as decoration.
        // biome-ignore lint/a11y/noAutofocus: the row is created by the gesture that asks for it
        autoFocus
        aria-label={FILES_NEW_FILE_NAME_LABEL}
        value={createName}
        onChange={(event) => setCreateName(event.target.value)}
        onKeyDown={(event) => {
          if (event.key === "Enter") {
            event.preventDefault();
            createFile(row.profileId, row.subpath);
          } else if (event.key === "Escape") {
            event.preventDefault();
            setCreatingIn(null);
            setWriteError(null);
          }
        }}
        className="h-6 min-w-0 flex-1 rounded-sm border border-border bg-background px-2 text-sm"
      />
      <Button
        type="button"
        variant="outline"
        size="xs"
        onClick={() => createFile(row.profileId, row.subpath)}
      >
        {FILES_CREATE_LABEL}
      </Button>
      <Button
        type="button"
        variant="ghost"
        size="xs"
        onClick={() => {
          setCreatingIn(null);
          setWriteError(null);
        }}
      >
        {FILES_CANCEL_LABEL}
      </Button>
    </div>
  );

  // Folded, the tree is its strip and the control that brings it back, and
  // nothing else is mounted (Story 48.1). An early return rather than a
  // conditional around the body, for the reason `PanelFrame` unmounts a folded
  // panel's body: the tree's rows, its virtualiser and its overflow probes are
  // exactly the cost the person reclaimed by folding it. Every hook above this
  // line still runs, so the tree store stays current and unfolding shows what
  // happened while it was away rather than a fresh scan.
  if (tree.folded) {
    return (
      <>
        <section {...tree.rootProps} className={FILES_COLUMN_CLASS}>
          {tree.chrome}
        </section>
        {tree.seam}
      </>
    );
  }

  return (
    <>
      <section {...tree.rootProps} className={FILES_COLUMN_CLASS}>
        {tree.chrome}
        {/* The heading used to sit here, over the sentence. It is one row up
            now: every foldable surface names itself in its fold row (Story
            48.3), and this pane was the only one that already had a name to
            move. What is left is the sentence and the selection's actions.

            **STACKED, and that is the whole repair.** The sentence and the
            action cluster used to be siblings in one `justify-between` row in
            which every control was `shrink-0`. The prose was therefore the only
            child that could give ground, so the flex algorithm gave it exactly
            its min-content width — the longest single word — and the owner
            photographed a paragraph running ONE WORD PER LINE down the pane
            while the buttons beside it kept their full width. It is the same
            defect the file rows had one level down, from the same cause.

            A paragraph that can only be read a word at a time is not a smaller
            version of itself, so the fix is not a better squeeze: it is to stop
            asking the prose to share a line. The actions take a row of their
            own and the sentence takes the column's full width, at EVERY width.
            One shape, no measured breakpoint, no `ResizeObserver` — nothing
            here can reflow differently on a machine whose font disagrees, and
            nothing moves as the seam is dragged.

            The action row's floor is arithmetic a reader can check: three 32px
            squares and two 8px gaps is 112px, the count badge is a figure in a
            chip, and the narrowest this column may be is 220px less the 48px
            `px-6` spends — 172. The prose below wraps into whatever is left,
            which is what a paragraph is for. */}
        <header className="flex shrink-0 flex-col gap-2 border-border border-b px-6 py-4">
          <div className="flex items-center justify-end gap-2">
            {/* Delete acts on the SELECTION, which is why the count lives here
              rather than a Delete button living on every row (Story 45.3).
              A per-row button cannot answer "and the other four", and the
              confirmation's whole job is answering exactly that.

              Present only when something is selected that keeper may delete —
              a selection of read-only files offers no Delete, and each of those
              rows already carries its own reason. */}
            {deletable.length > 0 && (
              <>
                {/* A COUNT, not a sentence beside buttons — the app's own chip,
                  the way the nav rail counts pending approvals and a chat row
                  counts mentions: the figure is what is drawn, the words are
                  what is announced. It was `1 item selected` set as running
                  text, about 90px of a 172px row, and it is the second thing
                  that made this header impossible to lay out.

                  `role="status"`, which is what `RecordingsPane` gives its own
                  count, and it earns the live region here: the number changes
                  under the reader's own clicks. It is also what makes the name
                  reachable at all — an `aria-label` on a role-less `span` is
                  the trap `phone-inbox-header` already fell into once. */}
                <Badge
                  variant="secondary"
                  role="status"
                  data-testid={FILES_SELECTED_TESTID}
                  aria-label={filesSelectionSentence(deletable.length)}
                  title={filesSelectionSentence(deletable.length)}
                  className="figures mr-auto"
                >
                  {deletable.length}
                </Badge>
                {/* Still `destructive`, which since the palette pass is a red
                  label and a red hairline rather than a tint — so the one verb
                  in this row that cannot be undone still reads as itself with
                  its word taken off. */}
                <Button
                  type="button"
                  variant="destructive"
                  size="icon-sm"
                  aria-label={FILES_DELETE_LABEL}
                  title={FILES_DELETE_LABEL}
                  className="shrink-0"
                  onClick={() => requestDelete(deletable)}
                >
                  <Trash2 aria-hidden="true" />
                </Button>
              </>
            )}
            {/* Story 45.13's entry point, on the SAME selection Delete acts on
              (Story 45.3) — there is one selection model in this pane and this
              does not add a second. Offered whenever files are selected and a
              vault is open, and never for a selection of only folders: a note
              embeds a file, and there is no element for a directory.

              Deliberately not gated on `write.writable`. That flag answers "may
              keeper change this file", and attaching changes the NOTE, not the
              file — a read-only PDF on a paused drive is a perfectly good thing
              to put in a note.

              `Paperclip` is the app's attach glyph — the composer's and
              `AttachFileButton`'s — rather than `NotebookPen`, which this very
              pane already draws on every notes-vault folder in the tree below.
              A mark that means "vault" in the body cannot also mean "attach" in
              the header. */}
            {attachablePaths.length > 0 && activeVaultId !== null && (
              <Button
                type="button"
                variant="ghost"
                size="icon-sm"
                aria-label={ATTACH_TO_NOTE_LABEL}
                title={ATTACH_TO_NOTE_LABEL}
                className="shrink-0"
                onClick={() => setAttaching(true)}
              >
                <Paperclip aria-hidden="true" />
              </Button>
            )}
            {/* `RefreshCw` is the glyph the folded rail already spends on this
              same verb (see `tree`'s rail above). One control, one mark,
              whichever side of the fold it is drawn on. */}
            <Button
              type="button"
              variant="ghost"
              size="icon-sm"
              aria-label={FILES_REFRESH_LABEL}
              title={FILES_REFRESH_LABEL}
              className="shrink-0"
              onClick={refresh}
            >
              <RefreshCw aria-hidden="true" />
            </Button>
          </div>
          <p className="text-muted-foreground text-sm">{FILES_PANE_SUBTITLE}</p>
        </header>

        {/* Rust's sentence for a write that was refused or only partly done.
          `role="alert"` because it is the answer to something the person just
          did, and it appears where they did it rather than in a toast that has
          gone by the time they look up. */}
        {writeError !== null && (
          <Alert variant="destructive" className="mx-4 mt-3 w-auto">
            <AlertDescription data-testid={FILES_WRITE_ERROR_TESTID} role="alert">
              {writeError}
            </AlertDescription>
          </Alert>
        )}

        <div {...list.viewportProps} className="min-h-0 flex-1 overflow-y-auto">
          <div className="px-4 py-3">
            {emptySentence !== null ? (
              <Alert>
                <AlertDescription>{emptySentence}</AlertDescription>
              </Alert>
            ) : (
              <div
                ref={attachTree}
                aria-label={FILES_TREE_LABEL}
                aria-multiselectable="true"
                role="tree"
                className="relative w-full"
                style={{ height: `${list.totalSize}px` }}
              >
                {list.rows.map((item) => {
                  const row = rows[item.index];
                  if (row === undefined) {
                    return null;
                  }
                  return (
                    // Presentational, so the tree still owns its `treeitem`s
                    // directly: the window needs a box to position, and a box
                    // between a tree and its items is a box with no role.
                    <div key={row.key} role="presentation" {...list.rowProps(item)}>
                      {row.kind === "node" && renderNode(row)}
                      {row.kind === "create" && renderCreate(row)}
                      {row.kind === "note" && (
                        <p
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
                      )}
                    </div>
                  );
                })}
              </div>
            )}
          </div>
        </div>

        {/* The confirmation, and every word in it is Rust's (Story 45.3,
          UX-DR66). `question` names the one file or counts the many,
          `consequence` says whether they sync, `recovery` says a copy is kept,
          and `refusals` names anything keeper will not touch. Nothing here
          paraphrases: a confirmation composed in TypeScript from a count and a
          glyph would be a second reading of the engine's answer, in the one
          place a wrong reading costs a file. */}
        <AlertDialog open={pending !== null} onOpenChange={(open) => !open && setPending(null)}>
          <AlertDialogContent>
            <AlertDialogHeader>
              <AlertDialogTitle>{pending?.plan.question}</AlertDialogTitle>
              <AlertDialogDescription data-testid={FILES_CONFIRM_TESTID}>
                {pending?.plan.consequence} {pending?.plan.recovery}
              </AlertDialogDescription>
            </AlertDialogHeader>
            {pending !== null && pending.plan.files.length > 1 && (
              <ul className="max-h-40 overflow-y-auto text-muted-foreground text-sm">
                {pending.plan.files.map((file) => (
                  <li key={file}>{file}</li>
                ))}
              </ul>
            )}
            {pending !== null && pending.plan.refusals.length > 0 && (
              <ul className="text-destructive text-sm">
                {pending.plan.refusals.map((refusal) => (
                  <li key={refusal.relativePath}>{refusal.reason}</li>
                ))}
              </ul>
            )}
            <AlertDialogFooter>
              <AlertDialogCancel>{FILES_CANCEL_LABEL}</AlertDialogCancel>
              {/* Absent, not disabled, when there is nothing left to delete: the
                refusals above already say why, and a greyed-out Delete invites
                a second click at the one thing that will not happen. */}
              {pending !== null && pending.plan.files.length > 0 && (
                <AlertDialogAction variant="destructive" onClick={confirmDelete}>
                  {FILES_DELETE_LABEL}
                </AlertDialogAction>
              )}
            </AlertDialogFooter>
          </AlertDialogContent>
        </AlertDialog>

        {/* Story 45.13. Mounted only while open, like the create row: it holds
          a search and an outcome that belong to one gesture, and a person who
          closed it and selected different files must not find the old sentence
          waiting for them. */}
        {attaching && activeVaultId !== null && (
          <AttachToNoteDialog
            vaultId={activeVaultId}
            sources={attachablePaths}
            onClose={() => setAttaching(false)}
          />
        )}
      </section>
      {tree.seam}
    </>
  );
}
