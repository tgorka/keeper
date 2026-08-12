/**
 * One row of the sessions board (Phase 7, FR-228, UX-DR85, UX-DR86).
 *
 * A pure presentational component, the `recording-row.tsx` shape: the pane
 * owns the data and the filter, this file owns what one session looks like.
 * Nothing here reads or writes.
 *
 * The row is a status line first (UX-DR85): status glyph, title, the last log
 * line as subtitle, and the TWO freshness signals — workspace ("the agent is
 * iterating") and record ("something was written or promoted") — as separate
 * marks with their own relative times, never merged into one dot (UX-DR86).
 * A user who knows notes already knows the rest: the pin, the unread dot and
 * the tag badges are the same affordances with sessions data (UX-DR92).
 */
import { Archive, CircleDot, GitBranch, Pencil, Pin, TriangleAlert, Wrench } from "lucide-react";
import { Badge } from "@/components/ui/badge";
import { formatDraftAge } from "@/lib/format-time";
import type { SessionRowVm } from "@/lib/ipc/client";
import { isStale } from "@/lib/stores/sessions-list";
import { cn } from "@/lib/utils";

/** The two freshness signals' visible names — the zone's own split. */
export const SESSION_WORKSPACE_SIGNAL_LABEL = "workspace";
export const SESSION_RECORD_SIGNAL_LABEL = "record";

/** What an empty freshness side shows: nothing has happened, said plainly. */
export const SESSION_NO_ACTIVITY_LABEL = "—";

/** Status words, spoken by the glyph's accessible name. */
export const SESSION_STATUS_ACTIVE_LABEL = "active";
export const SESSION_STATUS_STALE_LABEL = "active, stale";
export const SESSION_STATUS_ARCHIVED_LABEL = "archived";

/** Test ids: the two signals and the status glyph, for assertion by meaning. */
export const SESSION_ROW_STATUS_TESTID = "session-row-status";
export const SESSION_ROW_WORKSPACE_TESTID = "session-row-workspace";
export const SESSION_ROW_RECORD_TESTID = "session-row-record";

export interface SessionRowProps {
  row: SessionRowVm;
  /** "now", injectable so tests pin the clock. */
  nowMs?: number;
  onOpen: (row: SessionRowVm) => void;
  /**
   * The row's trailing actions (the lifecycle overflow menu), rendered
   * OUTSIDE the open button — the row is a flex pair of the clickable body
   * and this slot, because a menu button nested in a button is not HTML.
   */
  actions?: React.ReactNode;
}

/** One freshness signal: a glyph, a label for readers, and a relative time. */
function FreshnessSignal({
  label,
  ms,
  nowMs,
  testId,
  Icon,
}: {
  label: string;
  ms: number | null;
  nowMs: number;
  testId: string;
  Icon: typeof Wrench;
}) {
  const age = ms === null ? SESSION_NO_ACTIVITY_LABEL : formatDraftAge(ms, nowMs);
  return (
    <span
      data-testid={testId}
      className="flex items-center gap-1 text-muted-foreground text-xs"
      title={`${label}: ${age}`}
    >
      <Icon aria-hidden className="size-3" />
      <span className="sr-only">{label}</span>
      <span className="figures">{age}</span>
    </span>
  );
}

export function SessionRow({ row, nowMs = Date.now(), onOpen, actions }: SessionRowProps) {
  const stale = isStale(row, nowMs);
  const statusLabel =
    row.status === "archived"
      ? SESSION_STATUS_ARCHIVED_LABEL
      : stale
        ? SESSION_STATUS_STALE_LABEL
        : SESSION_STATUS_ACTIVE_LABEL;
  return (
    <div
      className={cn(
        "flex items-start gap-1 rounded-md border border-border pr-1",
        "hover:bg-accent/50",
        row.status === "archived" && "opacity-80",
      )}
    >
      <button
        type="button"
        onClick={() => onOpen(row)}
        className="flex min-w-0 flex-1 flex-col gap-1 px-3 py-2 text-left focus-visible:outline-2"
      >
        <span className="flex min-w-0 items-center gap-2">
          {/* The status glyph: filled-fresh / hollow-stale for active, the box
            for archived — location and mtimes, never a stored state. */}
          <span data-testid={SESSION_ROW_STATUS_TESTID} title={statusLabel}>
            {row.status === "archived" ? (
              <Archive aria-hidden className="size-3.5 text-muted-foreground" />
            ) : (
              <CircleDot
                aria-hidden
                // The healthy green is the bridge-health token, not a new color:
                // one palette for "this thing is alive" across the app (UX-DR92).
                className={cn("size-3.5", stale ? "text-muted-foreground" : "text-bridge-healthy")}
              />
            )}
            <span className="sr-only">{statusLabel}</span>
          </span>
          {row.pinned && <Pin aria-label="pinned" className="size-3 text-muted-foreground" />}
          <span className="min-w-0 flex-1 truncate font-medium text-sm">{row.title}</span>
          {row.lineage && (
            <GitBranch aria-label="has lineage" className="size-3 text-muted-foreground" />
          )}
          {row.conflict && (
            <TriangleAlert aria-label="conflict" className="size-3.5 text-bridge-degraded" />
          )}
          {row.unread && (
            <span
              aria-label="unread"
              className="size-2 shrink-0 rounded-full bg-primary"
              role="status"
            />
          )}
        </span>
        {(row.lastLogLine !== "" || row.snippet !== "") && (
          <span className="truncate text-muted-foreground text-xs">
            {row.lastLogDate !== "" && <span className="figures">{row.lastLogDate} — </span>}
            {row.lastLogLine !== "" ? row.lastLogLine : row.snippet}
          </span>
        )}
        <span className="flex items-center gap-3">
          <FreshnessSignal
            label={SESSION_WORKSPACE_SIGNAL_LABEL}
            ms={row.workspaceMs}
            nowMs={nowMs}
            testId={SESSION_ROW_WORKSPACE_TESTID}
            Icon={Wrench}
          />
          <FreshnessSignal
            label={SESSION_RECORD_SIGNAL_LABEL}
            ms={row.recordMs}
            nowMs={nowMs}
            testId={SESSION_ROW_RECORD_TESTID}
            Icon={Pencil}
          />
          <span className="min-w-0 flex-1" />
          {row.tags.slice(0, 4).map((tag) => (
            <Badge key={tag} variant="secondary" className="max-w-32 truncate">
              {tag}
            </Badge>
          ))}
        </span>
      </button>
      {actions !== undefined && <span className="shrink-0 py-1.5">{actions}</span>}
    </div>
  );
}
