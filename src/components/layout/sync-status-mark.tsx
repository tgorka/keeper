/**
 * The mark that says whether one file in the Files tab is synced (Story 44.17,
 * FR-173).
 *
 * **Why this is a glyph plus a sentence and not a coloured dot.** The state
 * this exists to make visible is *excluded* — a file a pattern will never let
 * sync — and the failure it prevents is that file reading as one that is about
 * to arrive. Two states that differ only in hue are two states somebody
 * eventually reads as the same thing, on a bad monitor or with the common form
 * of colour blindness. So each state carries its own shape, and each carries
 * the sentence Rust composed for it as its accessible name.
 *
 * **Not focusable, on purpose.** Story 43.8's tree runs a roving tabindex:
 * exactly one row is in the tab order and its actions join the tab order only
 * while it is the focused row. A status mark is not an action — there is
 * nothing to activate — so it takes no tabindex at all rather than becoming a
 * dead stop somebody has to Tab past on every row.
 *
 * **The words are not written here.** `detail` arrives composed in Rust, from
 * the same `Engine::pending` reason the Sync pane's Pending card renders. A
 * second copy of those sentences in TypeScript is a second copy that gets
 * edited once, and then the two surfaces disagree about the same file.
 */

import type { LucideIcon } from "lucide-react";
import { Ban, Check, CircleAlert, CircleDashed, Clock } from "lucide-react";
import type { FilesEntrySyncVm, FilesSyncStatusVm } from "@/lib/ipc/client";
import { cn } from "@/lib/utils";

/** Test id for one row's sync mark. */
export const FILES_SYNC_MARK_TESTID = "files-sync-mark";

/**
 * The short name of each state, used when Rust sent no sentence.
 *
 * Only `synced` ever reaches this in practice — a file that is where it should
 * be has no story — but the map is total so a state added in Rust cannot reach
 * the screen nameless.
 */
export const FILES_SYNC_MARK_LABEL: Record<FilesSyncStatusVm, string> = {
  synced: "Synced",
  waiting: "Waiting to sync",
  excluded: "Excluded from sync",
  notInRepository: "Not in a repository",
  unknown: "Sync state unknown",
};

/** One shape per state. The shape is what carries the distinction. */
const MARK_ICON: Record<FilesSyncStatusVm, LucideIcon> = {
  synced: Check,
  waiting: Clock,
  excluded: Ban,
  notInRepository: CircleDashed,
  unknown: CircleAlert,
};

/**
 * Tone is emphasis, never information.
 *
 * A synced file is the common case and recedes; a file the engine could not
 * answer about is the one that wants a person's attention. Nothing here is the
 * only carrier of a distinction — remove every class and the marks are still
 * five different shapes with five different names.
 *
 * The recessive tone is `faint`, which is the token held to 3:1 for exactly this
 * job. It used to be `text-muted-foreground/60`, and that measured 2.45:1 in the
 * light theme: an opacity modifier discards the contrast its token was verified
 * at, and a graphic nobody can see is not quiet emphasis, it is an absent mark.
 * Non-text graphics have a floor too (SC 1.4.11), even when they carry a label.
 */
const MARK_TONE: Record<FilesSyncStatusVm, string> = {
  synced: "text-faint",
  waiting: "text-primary",
  excluded: "text-faint",
  notInRepository: "text-muted-foreground",
  unknown: "text-destructive",
};

/** The mark for one browsed entry. */
export function SyncStatusMark({ sync }: { sync: FilesEntrySyncVm }) {
  const Icon = MARK_ICON[sync.status];
  const label = sync.detail ?? FILES_SYNC_MARK_LABEL[sync.status];
  return (
    <span
      role="img"
      aria-label={label}
      title={label}
      data-testid={FILES_SYNC_MARK_TESTID}
      data-sync-status={sync.status}
      className={cn("flex shrink-0 items-center", MARK_TONE[sync.status])}
    >
      <Icon className="size-3.5" aria-hidden="true" />
    </span>
  );
}
