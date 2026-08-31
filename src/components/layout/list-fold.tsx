/**
 * The fold: how much of a long list is on screen, and the control that changes
 * it (Epic 32, Story 32.5; extracted by Epic 58, Story 58.3).
 *
 * Every list in this app that can outgrow its card shows a fixed number of rows
 * and offers one link-weight control to show the rest — Activity, Pending and
 * Parked on the Sync pane, a copy report's per-outcome groups, and a task's run
 * history. One implementation rather than one per pane, because the folded and
 * unfolded numbers are a **single global preference** read out of Rust
 * ({@link syncListSizes}), and a second copy of this hook would be a second
 * place for that preference to be honoured differently.
 *
 * `tasks-pane.tsx` states the rule this file exists to keep honest: the Tasks
 * pane reuses the Sync pane's list idioms *"rather than inventing a third"*.
 */
import { useState } from "react";
import { syncListSizes } from "@/lib/stores/sync-detail";

/**
 * The fold control's labels.
 *
 * "Show all N" and not "Show more": the unfolded size is a fixed setting, so the
 * button reveals a known quantity and can say so. `{n}` is the count that will be
 * visible after the press, which for a list longer than the unfolded size is that
 * size rather than the list's length — the button must not promise rows the query
 * never asked Rust for.
 */
export const LIST_FOLD_MORE_LABEL = (n: number) => `Show all ${n}`;
export const LIST_FOLD_LESS_LABEL = "Show fewer";

/**
 * How many rows a list shows, and the state of the control that changes it.
 *
 * Shared by every folding list so one press-and-read habit works everywhere,
 * and so the folded/unfolded numbers cannot drift apart between them.
 * Collapsing is per-list state rather than per-card: a long Activity list and a
 * two-row Problems list have nothing to say to each other about how much of
 * themselves the user wants to see.
 *
 * Takes `null` for an unread list so a caller may call it before its own early
 * return, which a hook must be.
 */
export function useFold<T>(rows: readonly T[] | null): {
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
export function FoldToggle({
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
    ? LIST_FOLD_LESS_LABEL
    : LIST_FOLD_MORE_LABEL(Math.min(rows.length, syncListSizes().unfolded));
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
