/**
 * The Sync primary view (Epic 32, Story 32.5, AD-S1..AD-S6; Epic 33,
 * Stories 33.3 and 33.4; Epic 34, Story 34.7).
 *
 * Folder sync worked long before it could be watched: Settings could name a
 * profile's state but never a *file*. This pane answers the three questions a
 * sync tool has to answer — is it working right now and on what, what has it
 * done to my files lately and what is it about to do, and what went wrong —
 * with one card per configured folder: a header (state, the Rust-composed line,
 * progress, the row actions), then Activity, Pending, and a Problems section
 * that exists only while something is wrong.
 *
 * Every Activity row also says how far that file got toward the remote, and a
 * row that did not get there can be asked why. Until then the failure text
 * existed only in the Problems section below, which names the unit of work and
 * never the file — so nothing on screen connected the two.
 *
 * Everything this view can do about the folder in front of you, it does here
 * rather than sending you to Settings for it: adding a folder, editing one,
 * and removing one. The two configuration modes go through the shared
 * {@link AddFolderForm} (AD-C7), which is one component in two modes so the
 * two surfaces cannot word or validate the same profile differently; removal
 * reuses the Settings row's confirmation wording and store action for the same
 * reason — two surfaces must not describe what removal keeps differently, and
 * what it keeps is the only thing anyone confirming it cares about. Until
 * Story 33.4 the empty state named Settings and stopped there, which meant a
 * new user's Sync view could only tell them to leave it; until edit mode, a
 * mistyped remote URL could only be fixed by removing the folder and setting
 * it up again.
 *
 * Beneath the folders sits the one thing here that is not a folder: a one-time
 * verified copy (Story 33.3, AD-C1). It is deliberately adjacent to sync and
 * deliberately unlike it — a job with a beginning and an end, no profile, no
 * schedule, nothing watched afterwards — so it is drawn as an outline under the
 * solid cards rather than as one more of them. Its whole value is the part `cp`
 * does not do, so the card says that in one sentence and claims nothing beyond
 * it: bytes, modification time and the executable bit are what a copy carries.
 *
 * Two rules carried over from the Settings section, because they are the whole
 * reason this surface can be trusted:
 *   - {@link SyncStatusVm.line} is rendered verbatim. It is composed in Rust so
 *     the tray and this window can never word the same state differently. The
 *     state badge beside it is a separate, coarser projection of `state`, not a
 *     second wording of the line.
 *   - Nothing here promises a finish time for a settling file. The quiet window
 *     restarts on every write, so the pane reports how long keeper has been
 *     waiting and stops there.
 *
 * It reuses the {@link RecordingPane} outer chrome (`<section>`/`<header>`/
 * `<ScrollArea>`) and its centered content column (UX-DR29) so the non-chat
 * primary views read as one family. The whole surface is capability-gated at
 * the app-shell / sidebar level: a machine with no usable `git` gets no sync UI
 * at all, never a disabled one.
 */
import { open as openFolder } from "@tauri-apps/plugin-dialog";
import {
  ArrowDownToLine,
  ArrowUpFromLine,
  CircleAlert,
  CircleArrowDown,
  CircleArrowUp,
  CircleCheck,
  CircleDashed,
  CircleMinus,
  CirclePlus,
  CircleSlash,
  Clock,
  FileIcon,
  SquarePen,
  TriangleAlert,
} from "lucide-react";
import { type ReactNode, useEffect, useId, useState } from "react";
import {
  SYNC_NOW_LABEL,
  SYNC_PAUSE_LABEL,
  SYNC_PROGRESS_LABEL,
  SYNC_REMOVE_CANCEL_LABEL,
  SYNC_REMOVE_CONFIRM_LABEL,
  SYNC_REMOVE_CONFIRM_SENTENCE,
  SYNC_REMOVE_CONFIRM_TITLE,
  SYNC_REMOVE_LABEL,
  SYNC_RESUME_LABEL,
  SyncFolderPath,
  syncRemoteHost,
} from "@/components/settings/sync-section";
import { AddFolderForm, SYNC_ADD_TITLE, SYNC_EDIT_TITLE } from "@/components/sync/add-folder-form";
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
import { Card, CardContent, CardHeader, CardTitle } from "@/components/ui/card";
import { Checkbox } from "@/components/ui/checkbox";
import { Label } from "@/components/ui/label";
import {
  Popover,
  PopoverContent,
  PopoverDescription,
  PopoverHeader,
  PopoverTitle,
  PopoverTrigger,
} from "@/components/ui/popover";
import { Progress } from "@/components/ui/progress";
import { ScrollArea } from "@/components/ui/scroll-area";
import { Separator } from "@/components/ui/separator";
import { formatDraftAge } from "@/lib/format-time";
import type {
  CopyJobState,
  CopyJobVm,
  SyncActivityVm,
  SyncOutcomeVm,
  SyncParkedVm,
  SyncPendingVm,
  SyncProblemsVm,
  SyncProfileVm,
  SyncStatusVm,
} from "@/lib/ipc/client";
import {
  type CopyGroup,
  cancelCopyJob,
  copyEntryGroups,
  copyJobFraction,
  isCopyJobTerminal,
  isCopyRunning,
  startCopyJob,
  startCopyJobPolling,
  useCopyJobStore,
} from "@/lib/stores/copy-job";
import {
  ensureSyncHydrated,
  isSyncStatusActive,
  removeSyncProfile,
  rescanSyncProfile,
  setSyncProfileEnabled,
  startSyncStatusPolling,
  syncErrorMessage,
  syncProfileNow,
  useSyncStore,
} from "@/lib/stores/sync";
import {
  hydrateSyncListSizes,
  refreshSyncDetail,
  refreshSyncDetailAll,
  retrySyncParked,
  retrySyncParkedAll,
  startSyncDetailPolling,
  startSyncProgressStream,
  syncListSizes,
  syncLiveFraction,
  syncLiveRate,
  useSyncDetailStore,
} from "@/lib/stores/sync-detail";

/** What this view is for, in the sync voice: sentence case, no promises. */
export const SYNC_PANE_SUBTITLE =
  "What keeper has synced, what it is waiting on, and anything that needs you.";

/** Shown while the profile list has never been read. Unknown, not empty. */
export const SYNC_PANE_LOADING_SENTENCE = "Loading folders…";

/**
 * Shown once the mirror has loaded and there is genuinely nothing configured.
 * Says what the form beneath it is for; it never points at another surface,
 * because this one can do the thing.
 */
export const SYNC_PANE_EMPTY_SENTENCE =
  "No folders are set up to sync yet. Point keeper at a folder and the git remote to sync it with.";

/** Section titles. */
export const SYNC_ACTIVITY_TITLE = "Activity";
export const SYNC_PENDING_TITLE = "Pending";
export const SYNC_PROBLEMS_TITLE = "Problems";

/** Names the in-flight path for a screen reader; the bar carries it on screen. */
export const SYNC_CURRENT_LABEL = "Currently syncing:";

/** Shown under a section whose read has not landed yet. */
export const SYNC_LIST_LOADING_SENTENCE = "Loading…";

/**
 * The Activity empty state. Says what would put a row here rather than
 * reporting an absence of data.
 */
export const SYNC_ACTIVITY_EMPTY_SENTENCE =
  "Nothing has synced yet. Files show up here as keeper carries them.";

/** The Pending empty state. */
export const SYNC_PENDING_EMPTY_SENTENCE = "Nothing is waiting to sync.";

/** The reason line for a file inside its quiet window. Never a countdown. */
export const SYNC_SETTLING_SENTENCE = "Waiting for writes to stop";

/**
 * Why a settling file has no finish time. Shown once under the Pending list
 * whenever at least one file is settling — the window restarts on every write,
 * so any estimate keeper offered would be a guess it could not keep.
 */
export const SYNC_SETTLING_NOTE =
  "keeper waits for a file to stop changing before it copies it. Every new write starts that wait over, so there is no finish time to show.";

/** Conflict-copy copy. Explains what happened to both versions, plainly. */
export const SYNC_CONFLICT_TITLE = "Conflict copies";
export const SYNC_CONFLICT_SENTENCE =
  "This folder and the remote changed the same file. The remote version kept the original name, and your version was saved beside it under the name below. Nothing was overwritten.";
export const SYNC_CONFLICT_NOTE =
  "Delete a copy once you have taken what you need from it — it leaves this list on its own.";

/** Parked-work copy. */
export const SYNC_PARKED_TITLE = "Stopped retrying";
export const SYNC_PARKED_SENTENCE =
  "keeper stopped retrying these after repeated failures. Retry puts one back in the queue; Retry all puts back every one listed here.";
export const SYNC_PARKED_NO_ERROR_SENTENCE = "No error was recorded.";

/**
 * Unspellable-name copy (Story 47.2, DW-200).
 *
 * The sentence has one job that is easy to get wrong: say what keeper WILL NOT
 * do, so the reader stops waiting for it. keeper carries the file's bytes
 * perfectly — git stores the raw name — but it cannot address the file from a
 * pane, because the only rendering a pane can show is lossy and two different
 * files can share it. That is why every row prints the escaped form beside the
 * readable one: the escaped form is the one a person can act on.
 */
export const SYNC_UNSPELLABLE_TITLE = "Names that are not text";
export const SYNC_UNSPELLABLE_SENTENCE =
  "These files have names that are not valid text. keeper syncs them normally — nothing is lost — but it will not open, edit or delete them from this app, because the name it can show you is not the name on disk and two different files can look identical here.";
export const SYNC_UNSPELLABLE_NOTE =
  "The second line of each row is the name byte for byte, safe to paste into a terminal. Rename the file there and it will behave like any other.";

/**
 * The fold control's labels.
 *
 * "Show all N" and not "Show more": the unfolded size is a fixed setting, so the
 * button reveals a known quantity and can say so. `{n}` is the count that will be
 * visible after the press, which for a list longer than the unfolded size is that
 * size rather than the list's length — the button must not promise rows the query
 * never asked Rust for.
 */
export const SYNC_FOLD_MORE_LABEL = (n: number) => `Show all ${n}`;
export const SYNC_FOLD_LESS_LABEL = "Show fewer";

/**
 * How many rows a list shows, and the control to change it.
 *
 * Shared by all three lists on the card so one press-and-read habit works
 * everywhere, and so the folded/unfolded numbers cannot drift apart between them.
 * Collapsing is per-list state rather than per-card: a long Activity list and a
 * two-row Problems list have nothing to say to each other about how much of
 * themselves the user wants to see.
 */
function useFold<T>(rows: readonly T[] | null): {
  visible: readonly T[];
  hidden: number;
  expanded: boolean;
  toggle: () => void;
  limit: number;
} {
  const [expanded, setExpanded] = useState(false);
  const { folded, unfolded } = syncListSizes();
  const limit = expanded ? unfolded : folded;
  // An unread list folds to nothing rather than to `null`: every caller already
  // branches on its own `rows === null` before reaching the rows, and a nullable
  // `visible` only moves that check somewhere it has to be repeated.
  const visible = (rows ?? []).slice(0, limit);
  return {
    visible,
    hidden: (rows?.length ?? 0) - visible.length,
    expanded,
    limit,
    toggle: () => setExpanded((v) => !v),
  };
}

/**
 * The fold control, or nothing when the whole list already fits.
 *
 * Rendered below its list rather than beside the heading: it is about the rows,
 * and the row it sits under is the last one shown — which is where the eye
 * already is when it runs out of list.
 */
function FoldToggle({
  rows,
  fold,
  label,
}: {
  rows: readonly unknown[];
  fold: { hidden: number; expanded: boolean; toggle: () => void; limit: number };
  label: string;
}) {
  // Nothing folded and nothing to fold back: the control would do nothing in
  // either direction.
  if (fold.hidden === 0 && !fold.expanded) {
    return null;
  }
  // Capped at the list's own length: unfolding cannot reveal rows Rust was never
  // asked for, so "Show all 100" over a 12-row list would be a promise the query
  // cannot keep.
  const text = fold.expanded
    ? SYNC_FOLD_LESS_LABEL
    : SYNC_FOLD_MORE_LABEL(Math.min(rows.length, syncListSizes().unfolded));
  return (
    <button
      type="button"
      onClick={fold.toggle}
      // A link-weight control, not a Button: it changes how much of a list is on
      // screen, which is not an action on the folder and must not carry the same
      // visual weight as Retry or Sync now.
      className="self-start text-muted-foreground text-xs underline decoration-dotted hover:text-foreground"
      // Named for its list: three folds on one card would otherwise be three
      // buttons a screen reader calls the same thing.
      aria-label={`${text}: ${label}`}
    >
      {text}
    </button>
  );
}

/**
 * The forget-and-look-again action.
 *
 * Named for what a person wants ("check every file again"), not for what it does
 * to the database. The note says the rest, because the button is only ever the
 * right answer to a specific symptom: files that are on disk and not in Pending.
 *
 * That happens because change detection compares each path against the size,
 * mtime, ctime and inode it recorded last time — and a copy that preserves the
 * modification time (which keeper's own Copy files once does) can land a file
 * that matches a remembered row exactly. Re-scanning cannot find it; only
 * forgetting can.
 */
export const SYNC_RESCAN_LABEL = "Recheck all files";
export const SYNC_RESCAN_NOTE =
  "Forget what keeper remembers about this folder and look at every file again. Use it when a file is on disk but never appears as waiting to sync — a copy that kept its original modification time can look identical to a file keeper has already accounted for.";

/** The parked-unit action label. */
export const SYNC_RETRY_LABEL = "Retry";

/**
 * The bulk action, offered only when there is more than one parked unit — beside
 * a single row it would be a second button that does exactly what the first one
 * does.
 *
 * "Retry all" and not "Retry everything": what it requeues is this folder's
 * parked units, and a folder card is the only place it appears. The count is in
 * the accessible name rather than the visible label, because the list it sits
 * above already shows how many there are to anyone who can see it.
 */
export const SYNC_RETRY_ALL_LABEL = "Retry all";

/**
 * Delivery copy. The label names what is behind an Activity row's delivery
 * glyph, because the glyph says what state the file is in but not that there
 * is anything to read about it; the sentence answers the question a `failed`
 * row otherwise raises — why it has no Retry when the parked list's rows do.
 */
export const SYNC_DELIVERY_DETAIL_LABEL = "What happened to this file";
export const SYNC_DELIVERY_RETRYING_SENTENCE =
  "keeper is still retrying this one on its own, so there is nothing to press yet. If it gives up, this is where the Retry appears.";

/**
 * The coarse state word shown beside the Rust-composed line. Keys mirror the
 * Rust `SyncStatusVm.state` values; an unrecognized state renders as itself
 * rather than as nothing, so a state added in Rust is visible before it is
 * translated here.
 */
export const SYNC_STATE_WORDS: Record<string, string> = {
  idle: "Idle",
  watching: "Watching",
  syncing: "Syncing",
  offline: "Offline",
  mediaAbsent: "Large files missing",
  paused: "Paused",
  needsAttention: "Needs attention",
};

/**
 * The icon, its tone and the screen-reader word for each recorded activity
 * kind.
 *
 * Four silhouettes rather than four variations on a page outline: at this size
 * a plus, a pen and a minus tucked inside the same file shape all read as the
 * same grey smudge, which is what made a column of them unscannable. Only a
 * conflict copy carries colour — it is the one row that asks the reader to do
 * something.
 */
const ACTIVITY_KINDS: Record<string, { icon: typeof FileIcon; word: string; tone: string }> = {
  added: { icon: CirclePlus, word: "Added", tone: "text-muted-foreground" },
  modified: { icon: SquarePen, word: "Changed", tone: "text-muted-foreground" },
  deleted: { icon: CircleMinus, word: "Deleted", tone: "text-muted-foreground" },
  conflict: { icon: TriangleAlert, word: "Conflict copy", tone: "text-destructive" },
};

/**
 * The icon, its tone and the screen-reader word for how far a file got toward
 * the remote.
 *
 * The kind glyph at the other end of the row says what happened on this disk;
 * this one says whether the remote ever heard about it. Success is muted on
 * purpose — it is what nearly every row is, and a column of ticks in green
 * would bury the one row that needs a reader. Only the two failures carry
 * colour, and `abandoned` is worded with the Problems section's own title
 * because it is the same fact about the same unit of work, not a second one.
 *
 * `unknown` is deliberately absent, and so is any value Rust adds before this
 * record knows it: a row with no accountable unit of work has no delivery fact
 * to report, and a glyph would invent one. Such a row renders nothing at all —
 * not a placeholder, not a gap held open.
 */
export const SYNC_DELIVERY_STATES: Record<
  string,
  { icon: typeof FileIcon; word: string; tone: string }
> = {
  success: { icon: CircleCheck, word: "Reached the remote", tone: "text-muted-foreground" },
  inProgress: { icon: CircleDashed, word: "On its way", tone: "text-muted-foreground" },
  failed: { icon: CircleAlert, word: "Failed, still retrying", tone: "text-destructive" },
  abandoned: { icon: CircleSlash, word: SYNC_PARKED_TITLE, tone: "text-destructive" },
};

/**
 * Which way a pending row is travelling, as a glyph and a word.
 *
 * The list carries both directions now, and a column of paths that does not say
 * which way each one is going asks the reader to infer it from the sentence at
 * the far end of the row. The glyph is the same size and tone as the Activity
 * list's, because the two lists are read in one glance.
 *
 * Screen readers get the word, not the arrow: "up" is a shape, not a fact.
 */
export const SYNC_PENDING_INBOUND_WORD = "Coming in";
export const SYNC_PENDING_OUTBOUND_WORD = "Going out";

/**
 * The glyph a pending row leads with, and the words behind it.
 *
 * Two questions in one mark, because a row has space for one: an arrow's
 * **direction** says which way the file is travelling, and whether it is
 * **circled** says whether the thing at the other end already exists. A bare
 * arrow is content nobody has yet; a circled one is a second version of
 * something that does.
 *
 * `PENDING_INBOUND_UPDATE_MARK` below is deliberately unused. An inbound row is
 * always new content — a download is queued only for a path whose worktree
 * still holds pointer text (`lfs::stage::pending_smudges`), so "an update
 * arriving" is not a state this queue can be in. When it becomes one, that is
 * the mark it takes and the scheme above stays true.
 *
 * The word is what a screen reader gets. An arrow is a shape, and "up" is not a
 * fact about a file.
 */
export const PENDING_MARKS: Record<string, { icon: typeof FileIcon; word: string }> = {
  untracked: { icon: ArrowUpFromLine, word: `New file · ${SYNC_PENDING_OUTBOUND_WORD}` },
  added: { icon: ArrowUpFromLine, word: `New file · ${SYNC_PENDING_OUTBOUND_WORD}` },
  modified: { icon: CircleArrowUp, word: `Changed · ${SYNC_PENDING_OUTBOUND_WORD}` },
  deleted: { icon: CircleMinus, word: `Deleted · ${SYNC_PENDING_OUTBOUND_WORD}` },
  settling: { icon: Clock, word: SYNC_SETTLING_SENTENCE },
  incoming: { icon: ArrowDownToLine, word: `New file · ${SYNC_PENDING_INBOUND_WORD}` },
};

/** Reserved for an inbound row that replaces content this clone already has. */
export const PENDING_INBOUND_UPDATE_MARK = CircleArrowDown;

/** Names the row a transfer is moving right now. */
export const SYNC_PENDING_CURRENT_WORD = "Syncing now";

/** Why a file is waiting, for every reason except `settling` (which is timed). */
const PENDING_REASONS: Record<string, string> = {
  untracked: "New file, not synced yet",
  added: "Added, not synced yet",
  modified: "Changed, not synced yet",
  deleted: "Deleted, not synced yet",
  // The only inbound reason, and worded to say so: the four above are things
  // this machine did, this one is content the remote still holds.
  incoming: "Waiting to download",
};

/**
 * What each parked unit of work was trying to do. The kind is the journal's own
 * discriminant and may grow in Rust, so an unknown one renders as itself.
 */
const PARKED_KINDS: Record<string, string> = {
  push: "Push",
  pull: "Pull",
  lfsUpload: "Large file upload",
  lfsDownload: "Large file download",
  openPullRequest: "Open pull request",
  verify: "Verify",
};

/**
 * How long keeper has been waiting, as a duration — never a remaining time.
 *
 * Coarse on purpose: the figure is re-rendered on the detail poll, and a
 * second-by-second counter beside "waiting for writes to stop" would read as a
 * countdown to something.
 */
export function formatSyncWaited(sinceMs: number, now: number = Date.now()): string {
  const elapsedMs = Math.max(0, now - sinceMs);
  const minutes = Math.floor(elapsedMs / 60_000);
  if (minutes < 1) {
    return "under a minute";
  }
  if (minutes < 60) {
    return `${minutes} min`;
  }
  const hours = Math.floor(elapsedMs / 3_600_000);
  if (hours < 24) {
    return `${hours} hr`;
  }
  const days = Math.floor(elapsedMs / 86_400_000);
  return days === 1 ? "1 day" : `${days} days`;
}

/**
 * Why one file is waiting. A settling file reports how long it has been
 * waiting and nothing more; every other reason is a fixed phrase, with an
 * unrecognized one rendered as itself.
 */
export function syncPendingReason(pending: SyncPendingVm, now: number = Date.now()): string {
  if (pending.reason === "settling") {
    return pending.sinceMs === null
      ? SYNC_SETTLING_SENTENCE
      : `${SYNC_SETTLING_SENTENCE} · ${formatSyncWaited(pending.sinceMs, now)} so far`;
  }
  // No size here any more: every row carries one, and it is rendered in a column
  // of its own rather than glued onto a sentence which is itself no longer
  // shown. What this composes now is the row's ACCESSIBLE description; the
  // glyph is what a sighted reader gets.
  return PENDING_REASONS[pending.reason] ?? pending.reason;
}

/** What a parked unit was doing and how hard keeper tried. */
export function syncParkedSummary(unit: SyncParkedVm): string {
  const attempts = unit.attempts === 1 ? "1 attempt" : `${unit.attempts} attempts`;
  return `${PARKED_KINDS[unit.kind] ?? unit.kind} · stopped after ${attempts}`;
}

// ---------------------------------------------------------------------------
// Copy files once (Epic 33, Story 33.3, AD-C1..AD-C6)
// ---------------------------------------------------------------------------

/**
 * What the card is, and what separates it from every card above it. "Once" is
 * the whole distinction: no profile is created, nothing is watched, and
 * finishing changes nothing about either side.
 */
export const COPY_TITLE = "Copy files once";
export const COPY_SUBTITLE =
  "A one-time job, not a folder to keep in sync. keeper copies what you point it at and then forgets it.";

/**
 * The claim the feature exists to make (AD-C2), and the one it must never be
 * read as making. Every copy tool stops at a successful write; this one reads
 * the bytes back off the destination, so the card says exactly that — and then
 * says what a copy carries, because bytes, modification time and the executable
 * bit are all it carries.
 */
export const COPY_VERIFY_SENTENCE =
  "Every file is read back from the destination and compared with the source, and counts as copied only when the two match.";
export const COPY_CARRIES_SENTENCE =
  "Contents, modification time and the executable bit are carried; nothing else about a file is.";

/**
 * The two paths. Each note says the thing a user would otherwise have to find
 * out by running the job: a folder source contributes its contents rather than
 * itself, and the destination is a container, not the copy's new name.
 */
export const COPY_FROM_LABEL = "From";
export const COPY_FROM_NOTE = "A file, or a folder whose contents are copied.";
export const COPY_INTO_LABEL = "Into";
export const COPY_INTO_NOTE = "A folder. Everything lands inside it.";
export const COPY_NOTHING_CHOSEN_LABEL = "Nothing chosen";

/** Test ids for the two chosen-path displays (read-only truncated paths). */
export const COPY_SOURCE_TESTID = "copy-source-path";
export const COPY_DESTINATION_TESTID = "copy-destination-path";

/**
 * Test id for the settled job's report. The section exists only for a terminal
 * job, so its absence is the assertion that a partial report is never drawn.
 */
export const COPY_REPORT_TESTID = "copy-report";

/**
 * The pickers: short on screen, named in full to a screen reader. Two of these
 * read "Choose folder", and a third sits in the add-folder form above them, so
 * the visible word alone would leave three buttons indistinguishable by name.
 */
export const COPY_CHOOSE_FILE_TEXT = "Choose file";
export const COPY_CHOOSE_FOLDER_TEXT = "Choose folder";
export const COPY_PICK_SOURCE_FILE_LABEL = "Choose a source file";
export const COPY_PICK_SOURCE_FOLDER_LABEL = "Choose a source folder";
export const COPY_PICK_DESTINATION_LABEL = "Choose a destination folder";

/**
 * The replace choice, off by default (AD-C4). The note says what *off* means,
 * because "replace" alone leaves the user to guess whether the alternative is
 * skipping, failing, or renaming.
 */
export const COPY_REPLACE_LABEL = "Replace files that already exist";
export const COPY_REPLACE_NOTE =
  "Left off, a file that is already identical is skipped, and one that differs is left alone and reported.";

/** The two actions. Only one of them is ever on screen. */
export const COPY_SUBMIT_LABEL = "Copy";
export const COPY_STOP_LABEL = "Stop";

/** Between the start landing and the first snapshot: under way, not idle. */
export const COPY_STARTING_SENTENCE = "Starting…";

/** Names the meter and the in-flight path for a screen reader. */
export const COPY_PROGRESS_LABEL = "Copy progress";
export const COPY_CURRENT_LABEL = "Currently copying:";

/** The settled job's section, and the two endings that have no file list. */
export const COPY_RESULT_TITLE = "Result";
export const COPY_STOPPED_SENTENCE =
  "You stopped the copy. The file in flight left nothing behind, so the destination holds only whole, verified files.";
export const COPY_NOTHING_SENTENCE = "The source held no files, so nothing was copied.";

/**
 * The word for each job state. `done` is "Finished" rather than "Done": a job
 * can finish with files that failed, and "Done" over a report led by failures
 * would be a verdict the report contradicts.
 */
export const COPY_STATE_WORDS: Record<CopyJobState, string> = {
  copying: "Copying",
  verifying: "Verifying",
  done: "Finished",
  failed: "Failed",
  cancelled: "Stopped",
};

/**
 * The heading over each outcome's files, and the word the summary counts it
 * with. Both are keyed by the wire outcome, so one Rust grows later renders as
 * itself rather than vanishing.
 */
const COPY_OUTCOME_TITLES: Record<string, string> = {
  failed: "Could not be copied",
  collision: "Already there, and different",
  copied: "Copied and verified",
  identical: "Already identical",
};

const COPY_OUTCOME_COUNTS: Record<string, string> = {
  failed: "failed",
  collision: "left untouched",
  copied: "copied and verified",
  identical: "already identical",
};

/**
 * What a group means, where its heading does not already say it. `failed` needs
 * none — every row under it carries its own reason — and `copied` is the claim
 * the card already makes at the top.
 */
const COPY_OUTCOME_NOTES: Record<string, string> = {
  collision: `A file of the same name is already at the destination and its contents differ. keeper left it exactly as it was; turn on ${COPY_REPLACE_LABEL} to overwrite it on the next copy.`,
  identical: "The destination already held these bytes, so nothing was written.",
};

/** Decimal size units, counted the way a file manager counts them. */
const COPY_BYTE_UNITS = ["bytes", "kB", "MB", "GB", "TB"];

/**
 * A byte figure for the progress line and the report.
 *
 * Deliberately not `formatSize` from the recording surface: that one is a
 * digit-for-digit mirror of the Rust tray's whole-MB convention, and a copy of
 * three text files would read "0 MB of 0 MB" through it. Truncates for the same
 * reason it does — a figure here must never overstate what has reached disk.
 */
export function formatCopyBytes(bytes: number): string {
  const total = Math.max(0, Math.floor(bytes));
  if (total < 1000) {
    return total === 1 ? "1 byte" : `${total} bytes`;
  }
  let scaled = total;
  let unit = 0;
  while (scaled >= 1000 && unit < COPY_BYTE_UNITS.length - 1) {
    scaled /= 1000;
    unit += 1;
  }
  const tenths = Math.floor(scaled * 10);
  return `${Math.floor(tenths / 10)}.${tenths % 10} ${COPY_BYTE_UNITS[unit]}`;
}

/**
 * The state word and whatever counters are genuinely known, shaped like the
 * Rust-composed sync line so the two read as one family.
 *
 * A total the engine has not worked out yet is `0`, which means unknown: it is
 * left out rather than printed, because this card must never claim a total it
 * does not have.
 */
export function copyProgressSentence(job: CopyJobVm): string {
  const counters: string[] = [];
  if (job.filesTotal > 0) {
    counters.push(`${job.filesDone} of ${job.filesTotal} files`);
  }
  if (job.bytesTotal > 0) {
    counters.push(`${formatCopyBytes(job.bytesDone)} of ${formatCopyBytes(job.bytesTotal)}`);
  }
  const word = COPY_STATE_WORDS[job.state];
  return counters.length === 0 ? word : `${word} — ${counters.join(" · ")}`;
}

/**
 * What happened, counted off the very grouping the list below renders (AD-C6)
 * rather than tallied separately — so the headline and the rows cannot disagree
 * about how many files failed.
 */
export function copySummarySentence(groups: readonly CopyGroup[]): string {
  return groups
    .map(
      (group) => `${group.entries.length} ${COPY_OUTCOME_COUNTS[group.outcome] ?? group.outcome}`,
    )
    .join(" · ");
}

export function SyncPane() {
  const profiles = useSyncStore((state) => state.profiles);
  const statuses = useSyncStore((state) => state.statuses);
  const readError = useSyncStore((state) => state.error);
  /**
   * Whether the add form is held open independently of the empty state. The
   * header action toggles it; an add that leaves the form with something to
   * show sets it, which is what keeps that message alive across the flip from
   * empty to populated.
   */
  const [adding, setAdding] = useState(false);

  // Three lifetimes, all torn down together: the shared profile/status mirror
  // poll, the modest per-profile detail poll, and the progress stream that is
  // the only sub-second source of in-flight counters.
  useEffect(() => {
    // The row counts come first, because `unfolded` is the `LIMIT` the activity
    // read runs with: hydrating after the first read would fetch the fallback
    // 100 and then quietly disagree with the saved setting until the next poll.
    // A failure is not worth surfacing — the fallbacks match the Rust defaults,
    // so the lists render correctly either way.
    void hydrateSyncListSizes()
      .catch(() => {})
      .then(ensureSyncHydrated)
      .then(refreshSyncDetailAll);
    const stopStatuses = startSyncStatusPolling();
    const stopDetail = startSyncDetailPolling();
    const stopProgress = startSyncProgressStream();
    return () => {
      stopStatuses();
      stopDetail();
      stopProgress();
    };
  }, []);

  const empty = profiles !== null && profiles.length === 0;

  /**
   * Nothing configured → open the add form and leave it open. The one thing
   * worth doing here with no folders set up is set one up, so this surface
   * offers it rather than naming another one that would.
   *
   * Opening the *same* disclosure rather than rendering a second copy is the
   * point: the add that fills this list flips `empty` false mid-flight, and a
   * form conditioned on `empty` would unmount before it could report a
   * keychain failure or hand over the Clear button that undoes a stored token.
   */
  useEffect(() => {
    if (empty) {
      setAdding(true);
    }
  }, [empty]);

  const showAddForm = profiles !== null && adding;

  return (
    <section
      aria-label="Sync"
      // Last child of the shell row, so the trailing edge cancels; see
      // DESIGN.md → Elevation & Depth.
      className="flex min-w-0 flex-1 flex-col border-border border-r bg-background last:border-r-0"
    >
      <header className="flex shrink-0 items-start justify-between gap-4 border-border border-b px-6 py-4">
        <div className="min-w-0">
          <h1 className="font-heading text-title">Sync</h1>
          <p className="text-muted-foreground text-sm">{SYNC_PANE_SUBTITLE}</p>
        </div>
        {profiles !== null && !empty && (
          <Button
            type="button"
            variant="outline"
            size="sm"
            className="shrink-0"
            aria-expanded={adding}
            onClick={() => setAdding((shown) => !shown)}
          >
            {SYNC_ADD_TITLE}
          </Button>
        )}
      </header>

      <ScrollArea className="min-h-0 flex-1">
        {/* Centered single column at content-max-width (UX-DR29), matching the
            Recording pane rather than the full-bleed Bridges body. */}
        <div className="mx-auto flex w-full max-w-[720px] flex-col gap-6 p-6">
          {readError !== null && (
            <p role="alert" className="text-destructive text-sm">
              {readError}
            </p>
          )}
          {/* Only claim "nothing configured" once a read has actually landed —
              before that the list is unknown, not empty. */}
          {profiles === null && (
            <p className="text-muted-foreground text-sm">{SYNC_PANE_LOADING_SENTENCE}</p>
          )}
          {empty && <p className="text-muted-foreground text-sm">{SYNC_PANE_EMPTY_SENTENCE}</p>}
          {/* Whether it arrived as the empty state or from the header action,
              this is the one add form; see `showAddForm` above for why.

              Discarding closes the disclosure, which unmounts the form and
              takes the draft with it — the same end state a settled save
              reaches, so the next open always starts from an empty form rather
              than from an abandoned one. It is offered only when there is
              something to go back to: with nothing configured this form is the
              whole content of the surface and the header action that would
              reopen it is hidden, so a discard there would leave a user with
              nothing to do and no way to undo it. */}
          {showAddForm && (
            <Card size="sm">
              <CardContent>
                <AddFolderForm
                  onSaved={(_profile, settled) => setAdding(!settled)}
                  onCancel={empty ? undefined : () => setAdding(false)}
                />
              </CardContent>
            </Card>
          )}
          {profiles?.map((profile) => (
            <SyncProfileCard key={profile.id} profile={profile} status={statuses[profile.id]} />
          ))}
          {/* The break between the folders keeper keeps, and the one thing on
              this surface it keeps nothing about. */}
          <Separator />
          <CopyCard />
        </div>
      </ScrollArea>
    </section>
  );
}

/** One folder: what it is doing, what it has done, and what is wrong with it. */
function SyncProfileCard({
  profile,
  status,
}: {
  profile: SyncProfileVm;
  status: SyncStatusVm | undefined;
}) {
  const detail = useSyncDetailStore((state) => state.detail[profile.id]);
  const progress = useSyncDetailStore((state) => state.progress[profile.id]);
  const [busy, setBusy] = useState(false);
  const [actionError, setActionError] = useState<string | null>(null);
  /**
   * What the last `Sync now` on this card did, or `null` before one has run.
   *
   * Held here rather than read off the polled status: a pass that stages
   * nothing finishes in milliseconds while the status poll runs at 2 s (10 s
   * when idle), so a report that waited for the poll would arrive long after
   * the click — if the status moved at all, which for "nothing to do" it does
   * not.
   */
  const [outcome, setOutcome] = useState<SyncOutcomeVm | null>(null);
  /**
   * Whether this folder's configuration is open for editing. Per card, because
   * the form it reveals is about this folder and nothing else.
   */
  const [editing, setEditing] = useState(false);
  /** Whether this folder's remove confirmation is open. Also per card. */
  const [confirmOpen, setConfirmOpen] = useState(false);

  /**
   * Run a card action with the shared busy/error lifecycle, then re-read this
   * folder's lists: an action is exactly the moment the three lists are most
   * likely to have moved, and the poll is deliberately too slow to notice.
   *
   * `refresh` is false for removal alone. There is no folder left to re-read,
   * and asking anyway would record three rejections against an id the store
   * has just forgotten.
   */
  const run = async (action: () => Promise<void>, refresh = true) => {
    setBusy(true);
    setActionError(null);
    // Any action supersedes the last sync report: Pause and Remove make it
    // stale, and a fresh Sync now is about to replace it.
    setOutcome(null);
    try {
      await action();
    } catch (raw) {
      setActionError(syncErrorMessage(raw));
    } finally {
      if (refresh) {
        await refreshSyncDetail(profile.id);
      }
      setBusy(false);
    }
  };

  /**
   * Put one parked unit of work back in the queue.
   *
   * Shared by the two surfaces that offer it — the Problems section's own list
   * and the Activity row whose file that unit was carrying — so one click
   * cannot take the busy lock and re-read the lists while the other does
   * neither.
   */
  const retryUnit = (unitId: number) => {
    void run(async () => {
      await retrySyncParked(profile.id, unitId);
    });
  };

  /**
   * Put every parked unit back in the queue, under the same busy lock one Retry
   * takes. The ids are passed in from the rendered list rather than re-read
   * here, so what the click requeues is what the user was looking at — a unit
   * that parked between the render and the click is left for the next press.
   */
  const retryAllUnits = (unitIds: readonly number[]) => {
    void run(async () => {
      await retrySyncParkedAll(profile.id, unitIds);
    });
  };

  const active = status !== undefined && isSyncStatusActive(status);
  const fraction = syncLiveFraction(status, progress);
  const percent = fraction === null ? null : Math.round(fraction * 100);
  // The live detail comes off the stream alone, so a window that just mounted
  // honestly has none of it until the next event — and a folder the poll calls
  // settled has none either, however recently an event arrived.
  const streamed = active ? progress : undefined;
  const current = streamed?.current ?? null;
  // Both figures worded the way the Rust status line words them, so this fast
  // copy and the 2 s-polled sentence above read as the same quantity rather
  // than two competing claims. Built as a list for the reason
  // `copyProgressSentence` builds one: a figure that is not known yet drops out
  // and leaves no orphaned separator behind.
  //
  // Kept as two values rather than one joined string because they jitter at
  // different speeds and only one of them can be given a fixed box: the rate
  // changes every tick and has a bounded width, the file count changes once
  // per file and has none.
  let files: string | null = null;
  if (streamed !== undefined) {
    if (streamed.filesTotal !== null) {
      files = `${streamed.filesDone}/${streamed.filesTotal} files`;
    } else if (streamed.filesDone > 0) {
      files = `${streamed.filesDone} files`;
    }
  }
  // `null` covers both "too little measured to say" and "nothing is moving";
  // neither is a rate, and printing "0 B/s" for the second would be a claim
  // about an idle wire (AD-34-13).
  const rate = syncLiveRate(status, progress);
  const rateText = rate === null ? null : `${formatCopyBytes(rate)}/s`;
  const flying = [files, rateText].filter((figure) => figure !== null);

  return (
    <Card size="sm">
      <CardHeader>
        {/* Wraps rather than overflowing. The actions used to be `shrink-0` beside
            a `min-w-0` title, which works until there are five of them: the row
            then pushes past the card's own edge and the last button is simply
            unreachable. Wrapping costs a second line on a narrow window and keeps
            every action clickable, which is the trade worth making — a control you
            cannot see is not a control. */}
        <div className="flex flex-wrap items-start justify-between gap-x-3 gap-y-2">
          <div className="flex min-w-0 flex-1 basis-64 flex-col gap-1">
            <div className="flex items-center gap-2">
              <CardTitle>{profile.name}</CardTitle>
              {status !== undefined && (
                <Badge
                  variant={
                    status.state === "needsAttention"
                      ? "destructive"
                      : status.state === "syncing"
                        ? "default"
                        : "secondary"
                  }
                >
                  {SYNC_STATE_WORDS[status.state] ?? status.state}
                </Badge>
              )}
            </div>
            {/* Verbatim, never recomposed: the tray renders this same sentence.
                A sentence, though — so it is set in the room's voice. Mono is
                for the things below it that line up. */}
            {status !== undefined && (
              <span className="figures text-muted-foreground text-xs">{status.line}</span>
            )}
            {/* Where it lives and where it points, on one line: two stacked
                muted lines under an already-muted status line read as one grey
                block with no shape to it. */}
            <p className="flex min-w-0 items-baseline gap-1.5 text-muted-foreground text-xs">
              <SyncFolderPath
                profile={profile}
                className="min-w-0 truncate font-mono"
                onError={setActionError}
              />
              <span aria-hidden="true">·</span>
              <span className="shrink-0">{syncRemoteHost(profile.remoteUrl)}</span>
            </p>
          </div>
          <div className="flex flex-wrap items-center justify-end gap-1">
            <Button
              type="button"
              variant="outline"
              size="xs"
              disabled={busy}
              onClick={() => {
                void run(async () => {
                  setOutcome(await syncProfileNow(profile.id));
                });
              }}
            >
              {SYNC_NOW_LABEL}
            </Button>
            <Button
              type="button"
              variant="outline"
              size="xs"
              disabled={busy}
              onClick={() => {
                void run(async () => {
                  await setSyncProfileEnabled(profile.id, !profile.enabled);
                });
              }}
            >
              {profile.enabled ? SYNC_PAUSE_LABEL : SYNC_RESUME_LABEL}
            </Button>
            {/* Between the act-now pair above and the configuration pair below,
                because it is a bit of both: it acts now, but only a folder whose
                Pending list is visibly wrong needs it. Ghost weight for the same
                reason — reaching for this should feel like an exception. */}
            <Button
              type="button"
              variant="ghost"
              size="xs"
              disabled={busy}
              title={SYNC_RESCAN_NOTE}
              onClick={() => {
                void run(async () => {
                  await rescanSyncProfile(profile.id);
                });
              }}
            >
              {SYNC_RESCAN_LABEL}
            </Button>
            <Button
              type="button"
              variant="ghost"
              size="xs"
              aria-expanded={editing}
              disabled={busy}
              onClick={() => setEditing((shown) => !shown)}
            >
              {SYNC_EDIT_TITLE}
            </Button>
            {/* No louder than Edit beside it, and deliberately so: the
                destructive step is the confirmation, not the button that opens
                it. Nothing here is irreversible until the dialog is accepted. */}
            <Button
              type="button"
              variant="ghost"
              size="xs"
              disabled={busy}
              onClick={() => setConfirmOpen(true)}
            >
              {SYNC_REMOVE_LABEL}
            </Button>
          </div>
        </div>
        {/* A bar only where there is a real denominator. Without one the
            Rust-composed line above still says what is happening, which beats a
            meter that invents a position. `aria-valuetext` reuses that line
            rather than wording progress a second way. */}
        {percent !== null && (
          <Progress
            className="mt-1"
            value={percent}
            aria-label={`${SYNC_PROGRESS_LABEL}: ${profile.name}`}
            aria-valuenow={percent}
            aria-valuetext={status?.line}
          />
        )}
        {(current !== null || flying.length > 0) && (
          // The live detail under the bar: how fast and how far through the
          // set, then the path being carried.
          //
          // The figures lead because they are the fixed-width half and the one
          // that changes every tick: a reader watching a rate should not have
          // to find it at the end of a path whose length depends on how deep
          // the file happens to sit. The path truncates into whatever room is
          // left, which is the same bargain as before — a long path gives way
          // to the figures rather than pushing them off the card — with the
          // shrink-0 half now at the front where the eye lands.
          <p className="flex min-w-0 items-baseline gap-1.5 text-muted-foreground text-xs">
            {files !== null && <span className="figures shrink-0 font-mono">{files}</span>}
            {files !== null && <span aria-hidden="true">·</span>}
            {/* A RESERVED box, not merely a monospaced one. Mono fixes the
                width of a digit and says nothing about how many there are:
                `2 kB/s` against `294.8 kB/s` is four characters, and at one
                update a second that would drag everything after it left and
                right continuously.

                `11ch` is the widest this formatter can produce — `999 bytes/s`,
                verified across the whole range rather than estimated.
                Right-aligned so the digits grow leftward into the reserved room
                and the separator never moves, and held open even when there is
                no rate: the row must not jump because a folder went quiet for a
                tick. It stands empty then rather than reading `0 B/s`, which
                would claim a measurement of an idle wire (AD-34-13). */}
            <span className="figures min-w-[11ch] shrink-0 text-right font-mono">
              {rateText ?? ""}
            </span>
            {current !== null && <span aria-hidden="true">·</span>}
            {current !== null && (
              <span className="truncate font-mono" title={current}>
                {/* Under a moving bar the bare path is unambiguous on screen; to
                    a screen reader it would be a path from nowhere. */}
                <span className="sr-only">{SYNC_CURRENT_LABEL} </span>
                {current}
              </span>
            )}
          </p>
        )}
        {outcome !== null && (
          // Inline, in the card that was acted on, beside everything else this
          // header reports about the folder — the sync surfaces have never used
          // a toast, and a disappearing one for the result of a button in view
          // would be the worst place to start. Rust composed the sentence
          // (`SyncOutcomeVm.line`) for the same reason it composes the status
          // line above: the Settings row says it in these words too.
          // `role="status"` announces it without stealing focus; conflicts take
          // the destructive colour because "both versions survive, go and look"
          // is not a checkmark, but they are non-blocking by contract, so this
          // is never an interrupting alert.
          <p
            role="status"
            className={
              outcome.conflicts.length > 0
                ? "text-destructive text-xs"
                : "text-muted-foreground text-xs"
            }
          >
            {outcome.line}
          </p>
        )}
        {actionError !== null && <p className="text-destructive text-xs">{actionError}</p>}
        {detail?.error != null && (
          <p role="alert" className="text-destructive text-xs">
            {detail.error}
          </p>
        )}
      </CardHeader>
      <CardContent className="flex flex-col gap-5">
        {/* Mounted fresh on each open, so it seeds from the profile as it is
            now; `settled` false leaves it standing over a keychain failure that
            nothing else on screen would report. */}
        {editing && (
          <AddFolderForm
            className="border-border border-b pb-5"
            profile={profile}
            onSaved={(_saved, settled) => setEditing(!settled)}
            onCancel={() => setEditing(false)}
          />
        )}
        <SyncActivityList
          profile={profile}
          rows={detail?.activity ?? null}
          busy={busy}
          onRetry={retryUnit}
        />
        <SyncPendingList profile={profile} rows={detail?.pending ?? null} current={current} />
        <SyncProblemsSection
          profile={profile}
          problems={detail?.problems ?? null}
          busy={busy}
          onRetry={retryUnit}
          onRetryAll={retryAllUnits}
        />
      </CardContent>
      {/* The Settings row's confirmation, verbatim: same title, same sentence,
          same two labels, same store action. Removal is the one thing on this
          card that cannot be undone, so what it keeps has to be worded in
          exactly one place. */}
      <AlertDialog open={confirmOpen} onOpenChange={setConfirmOpen}>
        <AlertDialogContent>
          <AlertDialogHeader>
            <AlertDialogTitle>{SYNC_REMOVE_CONFIRM_TITLE}</AlertDialogTitle>
            <AlertDialogDescription>{SYNC_REMOVE_CONFIRM_SENTENCE}</AlertDialogDescription>
          </AlertDialogHeader>
          <AlertDialogFooter>
            <AlertDialogCancel>{SYNC_REMOVE_CANCEL_LABEL}</AlertDialogCancel>
            <AlertDialogAction
              variant="destructive"
              onClick={() => {
                void run(async () => {
                  await removeSyncProfile(profile.id);
                }, false);
              }}
            >
              {SYNC_REMOVE_CONFIRM_LABEL}
            </AlertDialogAction>
          </AlertDialogFooter>
        </AlertDialogContent>
      </AlertDialog>
    </Card>
  );
}

/** What sync has done to this folder's files, newest first as Rust ordered it. */
function SyncActivityList({
  profile,
  rows,
  busy,
  onRetry,
}: {
  profile: SyncProfileVm;
  rows: SyncActivityVm[] | null;
  busy: boolean;
  onRetry: (unitId: number) => void;
}) {
  const fold = useFold(rows);
  return (
    <div className="flex flex-col gap-2">
      {/* The project's group-label treatment (the Bridges / Approvals panes and
          the sidebar groups): a quiet micro-label, so the card's own title
          stays the loudest thing in it. */}
      <h2 className="label-caps text-faint">{SYNC_ACTIVITY_TITLE}</h2>
      {rows === null ? (
        <p className="text-muted-foreground text-xs">{SYNC_LIST_LOADING_SENTENCE}</p>
      ) : rows.length === 0 ? (
        <p className="text-muted-foreground text-xs">{SYNC_ACTIVITY_EMPTY_SENTENCE}</p>
      ) : (
        <ul
          aria-label={`${SYNC_ACTIVITY_TITLE}: ${profile.name}`}
          className="flex flex-col gap-1.5"
        >
          {fold.visible.map((row) => {
            const kind = ACTIVITY_KINDS[row.kind] ?? {
              icon: FileIcon,
              word: row.kind,
              tone: "text-muted-foreground",
            };
            const Icon = kind.icon;
            return (
              <li key={`${row.tsMs}-${row.kind}-${row.path}`} className="flex items-center gap-2">
                <Icon aria-hidden="true" className={`size-3.5 shrink-0 ${kind.tone}`} />
                {/* The kind is carried by an icon, so its word is spoken but not
                    repeated on screen. */}
                <span className="sr-only">{kind.word}</span>
                <span className="min-w-0 flex-1 truncate font-mono text-xs" title={row.path}>
                  {row.path}
                </span>
                {/* A size nobody measured shows nothing at all: "0 B" would
                    claim the file was empty, and "unknown" is noise on a line
                    already busy answering when. */}
                {row.sizeBytes !== null && (
                  <span className="shrink-0 font-mono text-muted-foreground text-xs">
                    {formatCopyBytes(row.sizeBytes)}
                  </span>
                )}
                <span className="figures shrink-0 text-muted-foreground text-xs">
                  {formatDraftAge(row.tsMs)}
                </span>
                <SyncDeliveryMark row={row} busy={busy} onRetry={onRetry} />
              </li>
            );
          })}
        </ul>
      )}
      {rows !== null && rows.length > 0 && (
        <FoldToggle rows={rows} fold={fold} label={`${SYNC_ACTIVITY_TITLE}: ${profile.name}`} />
      )}
    </div>
  );
}

/**
 * How far one file got toward the remote: the glyph that ends its Activity
 * row, and — when something went wrong carrying it — the popover that says
 * what.
 *
 * A popover rather than a line under the row because the failure is an engine
 * message of no fixed length, and one of those beneath every failing row would
 * push the answer to "what has synced lately" off the card. It is worth the
 * click: before it, a reader who wanted to know why a file had not arrived had
 * only the Problems section far below, which reports the unit of work and
 * never the path, so the two halves of the answer could not be put together on
 * screen at all.
 *
 * A row with nothing recorded against it stays a plain icon rather than an
 * empty trigger. A control that opens nothing claims there is something to
 * read, and a list where most rows are fine would be mostly that claim.
 */
function SyncDeliveryMark({
  row,
  busy,
  onRetry,
}: {
  row: SyncActivityVm;
  busy: boolean;
  onRetry: (unitId: number) => void;
}) {
  const state = SYNC_DELIVERY_STATES[row.delivery];
  // `unknown`, and anything Rust grows later: no glyph at all, for the reason
  // {@link SYNC_DELIVERY_STATES} gives.
  if (state === undefined) {
    return null;
  }
  // Read out as a local because the guard on it has to hold inside a callback:
  // only an abandoned unit is a human's to restart, and only one keeper still
  // remembers has an id to name.
  const unitId = row.unitId;
  const Icon = state.icon;
  // The state rides an icon on screen and a word to a screen reader, exactly as
  // the kind glyph at the other end of the row does.
  const glyph = (
    <>
      <Icon aria-hidden="true" className={`size-3.5 shrink-0 ${state.tone}`} />
      <span className="sr-only">{state.word}</span>
    </>
  );
  if (row.failure === null) {
    return glyph;
  }

  return (
    <Popover>
      <PopoverTrigger asChild>
        {/* Named the way the folder-path button is named: what the glyph shows
            is a state, which does not say that activating it explains the
            state — so the accessible name carries that, and the path, because
            a card holds as many of these as it holds rows. */}
        <button
          type="button"
          aria-label={`${SYNC_DELIVERY_DETAIL_LABEL}: ${row.path}`}
          className="flex shrink-0 items-center rounded-sm outline-none focus-visible:ring-2 focus-visible:ring-ring"
        >
          {glyph}
        </button>
      </PopoverTrigger>
      <PopoverContent align="end" className="gap-2">
        <PopoverHeader>
          {/* The row truncates the path to fit beside the figures; this is the
              one place it has room to be read whole. */}
          <PopoverTitle className="font-mono text-xs [overflow-wrap:anywhere]">
            {row.path}
          </PopoverTitle>
          <PopoverDescription className="text-xs">{state.word}</PopoverDescription>
        </PopoverHeader>
        {/* Verbatim: this is the one string here keeper did not write, and a
            reworded git or LFS message is one nobody can search for.

            Toned by the state rather than always destructive, because this line
            is not always a failure. A push held until its own large files reach
            the remote is `inProgress` and carries the reason it is waiting —
            "publishing is on hold until…" — which in red would accuse keeper of
            breaking while it is in fact doing the careful thing. That is the
            same mistake the engine used to make by reporting every deferred
            unit as a missing drive. */}
        {/* Sans, not mono: this is a sentence git or LFS wrote, and a sentence
            set in the register's face is terminal cosplay. The face is for
            things that line up. */}
        <p className={`text-xs [overflow-wrap:anywhere] ${state.tone}`}>{row.failure}</p>
        {/* No Retry for a failed row: keeper has not stopped, and a button
            saying otherwise would invite a click that changes nothing. */}
        {row.delivery === "failed" && (
          <p className="text-muted-foreground text-xs">{SYNC_DELIVERY_RETRYING_SENTENCE}</p>
        )}
        {row.delivery === "abandoned" && unitId !== null && (
          <Button
            type="button"
            variant="outline"
            size="xs"
            className="self-start"
            disabled={busy}
            aria-label={`${SYNC_RETRY_LABEL}: ${row.path}`}
            onClick={() => onRetry(unitId)}
          >
            {SYNC_RETRY_LABEL}
          </Button>
        )}
      </PopoverContent>
    </Popover>
  );
}

/** What sync has seen but not yet carried, and why each one is waiting. */
function SyncPendingList({
  profile,
  rows,
  current,
}: {
  profile: SyncProfileVm;
  rows: SyncPendingVm[] | null;
  /** The path a transfer is moving right now, from the streamed progress. */
  current: string | null;
}) {
  const settling = rows?.some((row) => row.reason === "settling");
  const fold = useFold(rows);
  return (
    <div className="flex flex-col gap-2">
      <h2 className="label-caps text-faint">{SYNC_PENDING_TITLE}</h2>
      {rows === null ? (
        <p className="text-muted-foreground text-xs">{SYNC_LIST_LOADING_SENTENCE}</p>
      ) : rows.length === 0 ? (
        <p className="text-muted-foreground text-xs">{SYNC_PENDING_EMPTY_SENTENCE}</p>
      ) : (
        <>
          <ul
            aria-label={`${SYNC_PENDING_TITLE}: ${profile.name}`}
            className="flex flex-col gap-1.5"
          >
            {fold.visible.map((row) => {
              const mark = PENDING_MARKS[row.reason];
              const Mark = mark?.icon ?? FileIcon;
              // The row the transfer is on right now. Compared by path because
              // that is what both sides are keyed by; a row that is merely
              // *next* is not marked, because "in flight" is a fact and "about
              // to be" is a guess about a queue that reorders.
              const syncing = current !== null && row.path === current;
              return (
                <li
                  key={`${row.reason}-${row.path}`}
                  aria-current={syncing ? "true" : undefined}
                  className={`-mx-1.5 flex items-center gap-3 rounded-sm px-1.5 ${
                    syncing ? "bg-muted" : ""
                  }`}
                >
                  <Mark aria-hidden="true" className="size-3.5 shrink-0 text-muted-foreground" />
                  {/* The sentence the row no longer shows. A glyph is the whole
                      answer for a reader who can see it and none of it for one
                      who cannot. */}
                  <span className="sr-only">
                    {/* A settling row is described rather than named: its whole
                        point is HOW LONG it has been held, which a fixed word
                        cannot carry and the glyph certainly cannot. */}
                    {row.reason === "settling" || mark === undefined
                      ? syncPendingReason(row)
                      : mark.word}
                    {syncing ? ` · ${SYNC_PENDING_CURRENT_WORD}` : ""}{" "}
                  </span>
                  <span
                    className={`min-w-0 flex-1 truncate font-mono text-xs ${
                      syncing ? "text-foreground" : ""
                    }`}
                    title={`${row.path} — ${syncPendingReason(row)}`}
                  >
                    {row.path}
                  </span>
                  {/* A size nobody could measure shows nothing, the same rule
                      the Activity list follows: "0 B" would claim the file is
                      empty. */}
                  {row.sizeBytes !== null && (
                    <span className="shrink-0 font-mono text-muted-foreground text-xs">
                      {formatCopyBytes(row.sizeBytes)}
                    </span>
                  )}
                </li>
              );
            })}
          </ul>
          <FoldToggle rows={rows} fold={fold} label={`${SYNC_PENDING_TITLE}: ${profile.name}`} />
          {settling && <p className="text-muted-foreground text-xs">{SYNC_SETTLING_NOTE}</p>}
        </>
      )}
    </div>
  );
}

/**
 * Everything wrong with this folder: the live warning/error, the conflict
 * copies still on disk, and the work keeper gave up retrying.
 *
 * Renders nothing at all when there is nothing wrong (AD-S5) — an empty
 * "Problems" heading is a worry with no cause.
 */
function SyncProblemsSection({
  profile,
  problems,
  busy,
  onRetry,
  onRetryAll,
}: {
  profile: SyncProfileVm;
  problems: SyncProblemsVm | null;
  busy: boolean;
  onRetry: (unitId: number) => void;
  onRetryAll: (unitIds: readonly number[]) => void;
}) {
  // Called before the early return below, because a hook must be. `useFold`
  // takes `null` for exactly this reason.
  const parkedFold = useFold(problems?.parked ?? null);
  if (
    problems === null ||
    (problems.error === null &&
      problems.warning === null &&
      problems.parked.length === 0 &&
      problems.unspellable.length === 0 &&
      problems.conflicts.length === 0)
  ) {
    return null;
  }

  return (
    <div className="flex flex-col gap-3">
      <h2 className="label-caps text-faint">{SYNC_PROBLEMS_TITLE}</h2>
      {/* The split AD-S5 draws, in the two treatments the app already has: an
          error needs a human before the folder can progress, so it gets the
          actionable destructive notice; a warning is passive, so it gets the
          amber line the recording banner uses for exactly the same purpose. */}
      {problems.error !== null && (
        <Alert variant="destructive">
          <AlertDescription>{problems.error}</AlertDescription>
        </Alert>
      )}
      {problems.warning !== null && (
        <p className="flex items-start gap-1.5 text-held text-xs">
          {/* The tone was a bare ⚠ — an emoji, hidden from assistive tech, so
              the only thing marking this line as a warning rather than a
              statement was its amber. The icon carries the shape and the
              `sr-only` word carries the state, as the delivery marks above do. */}
          <TriangleAlert aria-hidden="true" className="mt-px size-3.5 shrink-0" />
          <span className="sr-only">Warning</span>
          {problems.warning}
        </p>
      )}
      {problems.conflicts.length > 0 && (
        <div className="flex flex-col gap-1.5">
          <h3 className="font-heading text-sm font-semibold">{SYNC_CONFLICT_TITLE}</h3>
          <p className="text-muted-foreground text-xs">{SYNC_CONFLICT_SENTENCE}</p>
          <ul
            aria-label={`${SYNC_CONFLICT_TITLE}: ${profile.name}`}
            className="flex flex-col gap-0.5"
          >
            {problems.conflicts.map((path) => (
              <li key={path} className="truncate font-mono text-xs" title={path}>
                {path}
              </li>
            ))}
          </ul>
          <p className="text-muted-foreground text-xs">{SYNC_CONFLICT_NOTE}</p>
        </div>
      )}
      {problems.unspellable.length > 0 && (
        <div className="flex flex-col gap-1.5">
          <h3 className="font-heading text-sm font-semibold">{SYNC_UNSPELLABLE_TITLE}</h3>
          <p className="text-muted-foreground text-xs">{SYNC_UNSPELLABLE_SENTENCE}</p>
          <ul
            aria-label={`${SYNC_UNSPELLABLE_TITLE}: ${profile.name}`}
            className="flex flex-col gap-1.5"
          >
            {/* Keyed on `escaped`, never on `display`: `display` is lossy and
                non-injective, so two genuinely different files in this list
                collide on it — which is the defect itself, showing up as a
                duplicate React key and one row silently not rendering. */}
            {problems.unspellable.map((name) => (
              <li key={name.escaped} className="flex min-w-0 flex-col">
                <span className="truncate font-mono text-xs" title={name.display}>
                  {name.display}
                </span>
                {/* Not truncated and selectable: this is the line a person
                    copies, and half of a byte-exact name is worse than none. */}
                <span className="select-all break-all font-mono text-meta text-muted-foreground">
                  {name.escaped}
                </span>
              </li>
            ))}
          </ul>
          <p className="text-muted-foreground text-xs">{SYNC_UNSPELLABLE_NOTE}</p>
        </div>
      )}
      {problems.parked.length > 0 && (
        <div className="flex flex-col gap-1.5">
          {/* The bulk action sits on the heading, not in the list: it acts on the
              whole group, and a row carrying it would read as another per-unit
              Retry. Above the sentence that explains what Retry does, so the
              explanation covers both buttons rather than trailing the one it
              does not mention. */}
          <div className="flex items-start justify-between gap-3">
            <h3 className="font-medium text-xs">{SYNC_PARKED_TITLE}</h3>
            {problems.parked.length > 1 && (
              <Button
                type="button"
                variant="outline"
                size="xs"
                className="shrink-0"
                disabled={busy}
                aria-label={`${SYNC_RETRY_ALL_LABEL}: ${problems.parked.length} ${SYNC_PARKED_TITLE.toLowerCase()}, ${profile.name}`}
                onClick={() => onRetryAll(problems.parked.map((unit) => unit.id))}
              >
                {SYNC_RETRY_ALL_LABEL}
              </Button>
            )}
          </div>
          <p className="text-muted-foreground text-xs">{SYNC_PARKED_SENTENCE}</p>
          <ul aria-label={`${SYNC_PARKED_TITLE}: ${profile.name}`} className="flex flex-col gap-2">
            {parkedFold.visible.map((unit) => {
              const summary = syncParkedSummary(unit);
              return (
                <li key={unit.id} className="flex items-start justify-between gap-3">
                  <div className="flex min-w-0 flex-col gap-0.5">
                    <span className="text-xs">{summary}</span>
                    <span className="text-destructive text-xs">
                      {unit.lastError ?? SYNC_PARKED_NO_ERROR_SENTENCE}
                    </span>
                  </div>
                  {/* Named for the unit it retries: several parked units mean
                      several Retry buttons, and "Retry" alone would not say
                      which one a screen reader is on. */}
                  <Button
                    type="button"
                    variant="outline"
                    size="xs"
                    className="shrink-0"
                    disabled={busy}
                    aria-label={`${SYNC_RETRY_LABEL}: ${summary}`}
                    onClick={() => onRetry(unit.id)}
                  >
                    {SYNC_RETRY_LABEL}
                  </Button>
                </li>
              );
            })}
          </ul>
          <FoldToggle
            rows={problems.parked}
            fold={parkedFold}
            label={`${SYNC_PARKED_TITLE}: ${profile.name}`}
          />
        </div>
      )}
    </div>
  );
}

/**
 * The one-time verified copy (Story 33.3, AD-C1..AD-C6).
 *
 * Drawn as a dashed outline under the solid folder cards, because a fifth solid
 * card would read as a fifth configured folder — and this configures nothing.
 *
 * Everything it claims comes from the polled job; it counts nothing itself. It
 * renders no report until that job is terminal, because `entries` is empty
 * before then and a partial report would read as a finished one.
 */
function CopyCard() {
  const [source, setSource] = useState("");
  const [destination, setDestination] = useState("");
  const [replaceExisting, setReplaceExisting] = useState(false);
  const fieldId = useId();

  const job = useCopyJobStore((state) => state.job);
  const error = useCopyJobStore((state) => state.error);
  const running = useCopyJobStore(isCopyRunning);

  /**
   * Watch only while there is something to watch. A foreground job the user is
   * looking at is polled an order of magnitude faster than the folder detail
   * behind it, and the loop retires itself on a terminal snapshot; this
   * teardown is the other ending — the view closing mid-copy, which leaves the
   * job running in Rust and simply stops watching it.
   */
  useEffect(() => {
    if (!running) {
      return;
    }
    return startCopyJobPolling();
  }, [running]);

  /**
   * Open the OS-native picker and record what came back. A cancellation writes
   * nothing, the rule the add-folder form's picker follows.
   */
  const pick = async (directory: boolean, choose: (path: string) => void) => {
    try {
      const selection = await openFolder({ directory });
      if (typeof selection === "string") {
        choose(selection);
      }
    } catch {
      // Picker cancellation / failure → keep the current choice (no write).
    }
  };

  // Only a settled job has a report; only a running one has counters worth
  // drawing. The two are never on screen together.
  const report = job !== null && isCopyJobTerminal(job.state) ? job : null;
  const fraction = job === null ? null : copyJobFraction(job);
  const percent = fraction === null ? null : Math.round(fraction * 100);
  const sentence = job === null ? COPY_STARTING_SENTENCE : copyProgressSentence(job);
  const current = job?.current ?? null;

  return (
    <Card
      size="sm"
      className="border border-border border-dashed bg-transparent shadow-none ring-0"
    >
      <CardHeader>
        {/* Wraps rather than overflowing. The actions used to be `shrink-0` beside
            a `min-w-0` title, which works until there are five of them: the row
            then pushes past the card's own edge and the last button is simply
            unreachable. Wrapping costs a second line on a narrow window and keeps
            every action clickable, which is the trade worth making — a control you
            cannot see is not a control. */}
        <div className="flex flex-wrap items-start justify-between gap-x-3 gap-y-2">
          <div className="flex min-w-0 flex-1 basis-64 flex-col gap-1">
            <CardTitle>{COPY_TITLE}</CardTitle>
            <p className="text-muted-foreground text-xs">{COPY_SUBTITLE}</p>
          </div>
          {/* The verdict, and only once there is one: a badge over a running
              job would repeat the state word the progress line already carries. */}
          {report !== null && (
            <Badge variant={report.state === "failed" ? "destructive" : "secondary"}>
              {COPY_STATE_WORDS[report.state]}
            </Badge>
          )}
        </div>
      </CardHeader>
      <CardContent className="flex flex-col gap-4">
        <CopyPathRow
          label={COPY_FROM_LABEL}
          note={COPY_FROM_NOTE}
          path={source}
          testId={COPY_SOURCE_TESTID}
        >
          <Button
            type="button"
            variant="outline"
            size="sm"
            disabled={running}
            aria-label={COPY_PICK_SOURCE_FILE_LABEL}
            onClick={() => {
              void pick(false, setSource);
            }}
          >
            {COPY_CHOOSE_FILE_TEXT}
          </Button>
          <Button
            type="button"
            variant="outline"
            size="sm"
            disabled={running}
            aria-label={COPY_PICK_SOURCE_FOLDER_LABEL}
            onClick={() => {
              void pick(true, setSource);
            }}
          >
            {COPY_CHOOSE_FOLDER_TEXT}
          </Button>
        </CopyPathRow>
        <CopyPathRow
          label={COPY_INTO_LABEL}
          note={COPY_INTO_NOTE}
          path={destination}
          testId={COPY_DESTINATION_TESTID}
        >
          <Button
            type="button"
            variant="outline"
            size="sm"
            disabled={running}
            aria-label={COPY_PICK_DESTINATION_LABEL}
            onClick={() => {
              void pick(true, setDestination);
            }}
          >
            {COPY_CHOOSE_FOLDER_TEXT}
          </Button>
        </CopyPathRow>
        <div className="flex flex-col gap-1">
          <div className="flex items-center justify-between gap-2">
            <Label htmlFor={`${fieldId}-replace`}>{COPY_REPLACE_LABEL}</Label>
            <Checkbox
              id={`${fieldId}-replace`}
              checked={replaceExisting}
              disabled={running}
              aria-describedby={`${fieldId}-replace-note`}
              onCheckedChange={(checked) => setReplaceExisting(checked === true)}
            />
          </div>
          <p id={`${fieldId}-replace-note`} className="text-muted-foreground text-xs">
            {COPY_REPLACE_NOTE}
          </p>
        </div>
        <div className="flex flex-col gap-2">
          {/* The claim first, in the card's own voice, then the limit on it —
              two stacked muted lines would read as one grey block with neither
              of them landing. */}
          <p className="text-xs">{COPY_VERIFY_SENTENCE}</p>
          <p className="text-muted-foreground text-xs">{COPY_CARRIES_SENTENCE}</p>
          {/* The IPC refusing, which is not a job that failed: Rust rejects a
              missing source and a destination inside the source before any job
              exists, and its message names which one it was. */}
          {error !== null && (
            <p role="alert" className="text-destructive text-xs">
              {error}
            </p>
          )}
          <div className="flex justify-end">
            <Button
              type="button"
              size="sm"
              disabled={running || source === "" || destination === ""}
              onClick={() => {
                void startCopyJob(source, destination, replaceExisting);
              }}
            >
              {COPY_SUBMIT_LABEL}
            </Button>
          </div>
        </div>
        {running && (
          <div className="flex flex-col gap-2">
            <div className="flex items-center justify-between gap-3">
              <span className="min-w-0 truncate text-xs">{sentence}</span>
              <Button
                type="button"
                variant="outline"
                size="xs"
                className="shrink-0"
                onClick={() => {
                  void cancelCopyJob();
                }}
              >
                {COPY_STOP_LABEL}
              </Button>
            </div>
            {/* A bar only where there is a real denominator; without one the
                line above still says what is happening. `aria-valuetext` reuses
                that line rather than wording progress a second way. */}
            {percent !== null && (
              <Progress
                value={percent}
                aria-label={COPY_PROGRESS_LABEL}
                aria-valuenow={percent}
                aria-valuetext={sentence}
              />
            )}
            {current !== null && (
              <p className="truncate font-mono text-muted-foreground text-xs" title={current}>
                {/* Under a moving bar the bare path is unambiguous on screen; to
                    a screen reader it would be a path from nowhere. */}
                <span className="sr-only">{COPY_CURRENT_LABEL} </span>
                {current}
              </p>
            )}
          </div>
        )}
        {report !== null && <CopyReport job={report} />}
      </CardContent>
    </Card>
  );
}

/** One chosen path: what it is for, what it is, and the picker that sets it. */
function CopyPathRow({
  label,
  note,
  path,
  testId,
  children,
}: {
  label: string;
  note: string;
  path: string;
  testId: string;
  children: ReactNode;
}) {
  return (
    <div className="flex items-start justify-between gap-3">
      <div className="flex min-w-0 flex-col gap-0.5">
        <Label>{label}</Label>
        <p
          className="truncate font-mono text-xs"
          data-testid={testId}
          title={path === "" ? undefined : path}
        >
          {path === "" ? COPY_NOTHING_CHOSEN_LABEL : path}
        </p>
        <p className="text-muted-foreground text-xs">{note}</p>
      </div>
      <div className="flex shrink-0 items-center gap-1">{children}</div>
    </div>
  );
}

/**
 * What happened to every file, worst first (AD-C6).
 *
 * Only ever rendered for a settled job. The summary is counted off the same
 * grouping the lists below render, so the two cannot disagree, and the two
 * endings that legitimately have no file list say so instead of showing an
 * empty one.
 */
function CopyReport({ job }: { job: CopyJobVm }) {
  const groups = copyEntryGroups(job.entries);
  return (
    <div className="flex flex-col gap-3" data-testid={COPY_REPORT_TESTID}>
      <h2 className="label-caps text-faint">{COPY_RESULT_TITLE}</h2>
      {/* The job failing to run at all — never a file that could not be copied,
          which is an entry below and leaves the job finished. */}
      {job.error !== null && (
        <Alert variant="destructive">
          <AlertDescription>{job.error}</AlertDescription>
        </Alert>
      )}
      {job.state === "cancelled" && (
        <p className="text-muted-foreground text-xs">{COPY_STOPPED_SENTENCE}</p>
      )}
      {job.state === "done" && groups.length === 0 && (
        <p className="text-muted-foreground text-xs">{COPY_NOTHING_SENTENCE}</p>
      )}
      {groups.length > 0 && (
        <>
          <p className="text-xs">{copySummarySentence(groups)}</p>
          {groups.map((group) => (
            <CopyReportGroup key={group.outcome} group={group} />
          ))}
        </>
      )}
    </div>
  );
}

/**
 * One outcome's file list, folded to the same limits as the sync lists.
 *
 * A copy of ten thousand files reports ten thousand lines, and the report is the
 * one surface where that is the *expected* size rather than a pathology — so it
 * folds for the same reason Activity does, and to the same numbers, because a
 * user who set "show me ten" meant it about lists in general.
 *
 * Its own component because the fold is a hook: a `.map` over the groups cannot
 * call one per group without breaking the rules of hooks.
 */
function CopyReportGroup({ group }: { group: ReturnType<typeof copyEntryGroups>[number] }) {
  const fold = useFold(group.entries);
  const title = COPY_OUTCOME_TITLES[group.outcome] ?? group.outcome;
  const note = COPY_OUTCOME_NOTES[group.outcome];
  return (
    <div className="flex flex-col gap-1.5">
      <h3 className="font-heading text-sm font-semibold">{title}</h3>
      {note !== undefined && <p className="text-muted-foreground text-xs">{note}</p>}
      <ul aria-label={title} className="flex flex-col gap-1">
        {fold.visible.map((entry) => (
          <li key={entry.path} className="flex flex-col gap-0.5">
            <div className="flex items-baseline justify-between gap-3">
              <span className="min-w-0 flex-1 truncate font-mono text-xs" title={entry.path}>
                {entry.path}
              </span>
              {/* No size beside a failure: none of it reached the
                            destination, and a byte count there would read as
                            how much did. */}
              {group.outcome !== "failed" && (
                <span className="shrink-0 font-mono text-muted-foreground text-xs">
                  {formatCopyBytes(entry.bytes)}
                </span>
              )}
            </div>
            {entry.reason !== null && (
              <span className="text-destructive text-xs">{entry.reason}</span>
            )}
          </li>
        ))}
      </ul>
      <FoldToggle rows={group.entries} fold={fold} label={title} />
    </div>
  );
}
