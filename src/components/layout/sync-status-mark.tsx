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
import {
  ArrowDownToLine,
  Ban,
  Check,
  CircleAlert,
  CircleDashed,
  Clock,
  Cloud,
  HardDrive,
} from "lucide-react";
import type { ComponentProps } from "react";
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
  virtual: "Content not on this computer",
  materializing: "Content arriving",
  materialized: "Content on this computer",
  notInRepository: "Not in a repository",
  unknown: "Sync state unknown",
};

/**
 * One shape per state. The shape is what carries the distinction.
 *
 * **`materializing` is deliberately not a circular spinner-like glyph.**
 * `Loader`, `LoaderCircle` and `CircleDotDashed` were each rejected: at
 * `size-3.5` their silhouettes read as the dashed circle `notInRepository`
 * already owns, so the two states would be told apart only by tone — which is
 * exactly the collision this file's header doc exists to prevent. An arrow into
 * a line says "arriving" without borrowing another state's outline.
 */
const MARK_ICON: Record<FilesSyncStatusVm, LucideIcon> = {
  synced: Check,
  waiting: Clock,
  excluded: Ban,
  virtual: Cloud,
  materializing: ArrowDownToLine,
  materialized: HardDrive,
  notInRepository: CircleDashed,
  unknown: CircleAlert,
};

/**
 * Tone is emphasis, never information.
 *
 * A synced file is the common case and recedes; a file the engine could not
 * answer about is the one that wants a person's attention. Nothing here is the
 * only carrier of a distinction — remove every class and every state is still
 * its own shape under its own name. The number of them is deliberately not
 * written down here: it was "five" for three stories after it stopped being
 * five, and a doc that has to be counted is a doc that goes stale silently.
 *
 * The recessive tone is not one state's: it belongs to every SETTLED state, and
 * the glyph is what says which of them a row is in. Whether a settled state is
 * "nothing to do here" is the whole of the question the tone answers, so a
 * state added in Rust joins that group or leaves it by what it MEANS, and the
 * reader can check that against the map below without counting anything.
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
  virtual: "text-faint",
  materializing: "text-primary",
  materialized: "text-faint",
  notInRepository: "text-muted-foreground",
  unknown: "text-destructive",
};

/**
 * The mark for one browsed entry.
 *
 * **Why the mark and not a progress bar carries `materializing`** (Story 56.7).
 * The pane owns no tick and gains none here — Story 56.9 owns its one interval
 * — so there is nothing to drive a bar's width from, and a client-side flag
 * counting the same fact would be a second thing to get out of step. Rows are
 * windowed too, so a spinner would arm and disarm on every scroll. An
 * indeterminate progress role on the glyph that is already there says "in
 * flight, total unknown" in vocabulary a screen reader already has, and invents
 * nothing: no `aria-valuenow`, because its ABSENCE is the indeterminate state.
 * That absence is the one thing `settings/sync-section.tsx:558-573` establishes
 * and the one thing borrowed from it. Its `aria-valuetext` is NOT borrowed: that
 * pairs a short distinct name ("Sync progress: <profile>") with the Rust line,
 * whereas this mark's accessible NAME already is the Rust line, so repeating it
 * as value text makes assistive tech that reads both say one sentence twice.
 * `aria-label` is in there unchanged for every state, so the accessible name is
 * the sentence whatever the role is.
 *
 * **The role arrives by spread rather than as `role={cond ? … : …}`.** Biome's
 * `useAriaPropsSupportedByRole` can only judge a role it can read as a literal:
 * a computed one reads to it as a `span` with no role, so the name this mark has
 * always carried is reported unsupported. The other way out was two return
 * branches, which is two copies of the same mark to keep in step — the thing
 * this component exists to have exactly one of.
 *
 * **`id` is how the sentence gets SPOKEN** (story 56.14). The mark carries its
 * sentence as its own accessible name, which makes it look present in an
 * accessibility tree dump and is silent in the one place it matters: a row that
 * sets `aria-label` replaces its subtree's contribution to its own name, so a
 * reader moving down the Files tree hears the file name and nothing about where
 * its bytes are — including all three of story 56.7's virtual states, which is
 * the whole distinction that story exists to draw. Both callers had the defect
 * and both now pass an `id` their row names in `aria-describedby`
 * (`files-pane.tsx`, `sessions/session-tree.tsx`), which is the mechanism each
 * already used for its size, its date and its lock reason.
 *
 * The prop stays OPTIONAL rather than required, because the fact it fixes is a
 * property of the ROW and not of the mark: a future caller whose row does not
 * replace its own name has nothing to repair, and making the id mandatory would
 * make such a caller invent an id nothing references. A required prop would also
 * be the wrong guard — it forces an id to exist, never that a row names it — so
 * what actually holds this is one test per caller asserting the row's
 * `aria-describedby` contains the mark's id.
 */
export function SyncStatusMark({ id, sync }: { id?: string; sync: FilesEntrySyncVm }) {
  const Icon = MARK_ICON[sync.status];
  const label = sync.detail ?? FILES_SYNC_MARK_LABEL[sync.status];
  const indeterminate = sync.status === "materializing";
  const semantics: ComponentProps<"span"> = {
    "aria-label": label,
    ...(indeterminate ? { role: "progressbar" } : { role: "img" }),
  };
  return (
    <span
      id={id}
      {...semantics}
      title={label}
      data-testid={FILES_SYNC_MARK_TESTID}
      data-sync-status={sync.status}
      className={cn("flex shrink-0 items-center", MARK_TONE[sync.status])}
    >
      <Icon className="size-3.5" aria-hidden="true" />
    </span>
  );
}
