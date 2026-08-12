/**
 * The Sessions primary view — the status board over LLM work sessions in the
 * flagged zones keeper already syncs (Phase 7, FR-224, FR-228, UX-DR85).
 *
 * A board first, a list second: rows arrive freshness-sorted from Rust with
 * status glyphs and the last log line as subtitle, so "what is being worked
 * on" answers itself without a click. The filter row sits above the fold, the
 * recordings-browser posture (UX-DR50 precedent).
 *
 * **Files are the only truth** shapes what this renders: every fact on a row
 * is a projection of the zone's own files, streamed back through one
 * payload-free changed event and a re-read (`use-sessions-changes`). There is
 * no session state to mutate here — this phase's board reads; the lifecycle
 * verbs land with their own stories.
 *
 * **Capability gating is absence** (FR-223): this pane renders only where
 * `CapabilitiesVm.sessions` is on, gated at the nav entry and the render
 * chain like every gated surface. A flagged-root-free build shows the one
 * honest empty state and a way to Settings → Sync.
 */
import { useCallback, useEffect } from "react";
import { SessionRow } from "@/components/sessions/session-row";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { InputGroup, InputGroupInput } from "@/components/ui/input-group";
import { useSessionsChanges } from "@/hooks/use-sessions-changes";
import { countLabel, SESSIONS } from "@/lib/count-label";
import type { SessionRowVm } from "@/lib/ipc/client";
import { revealPath, sessionsRescan } from "@/lib/ipc/client";
import { useCapabilitiesStore } from "@/lib/stores/capabilities";
import {
  filterRows,
  type SessionsStatusFilter,
  useSessionsListStore,
} from "@/lib/stores/sessions-list";
import {
  refreshSessionsRoots,
  useActiveSessionsRoot,
  useSessionsRootsStore,
} from "@/lib/stores/sessions-roots";

/** The pane's heading, and the accessible name of the surface itself. */
export const SESSIONS_PANE_TITLE = "Sessions";

/** The one honest sentence under the heading. */
export const SESSIONS_PANE_SUBTITLE =
  "LLM work sessions in the folders you sync — what moved, where, and how long ago.";

/** The accessible name of the row list. */
export const SESSIONS_LIST_LABEL = "Work sessions";

/** The no-root empty state: capability on, nothing flagged (FR-222). */
export const SESSIONS_NO_ROOT_TITLE = "No sessions folder yet";
export const SESSIONS_NO_ROOT_BODY =
  "Flag a synced folder — Settings → Sync → your folder → “This folder has sessions”. keeper adopts the 60-sessions layout that is already there.";

/** The filtered-to-nothing empty state, distinct from the above on purpose. */
export const SESSIONS_NO_MATCH_LABEL = "Nothing matches this filter.";

/** The rescan verb — the sessions "Rebuild index" (FR-225). */
export const SESSIONS_RESCAN_LABEL = "Rescan";

/** The status chips, in board order. */
const STATUS_CHOICES: { value: SessionsStatusFilter; label: string }[] = [
  { value: "all", label: "All" },
  { value: "active", label: "Active" },
  { value: "archived", label: "Archived" },
];

export function SessionsPane() {
  const canReveal = useCapabilitiesStore((s) => s.capabilities.revealInFileManager);
  const roots = useSessionsRootsStore((s) => s.roots);
  const activeRoot = useActiveSessionsRoot();
  const setActiveRootId = useSessionsRootsStore((s) => s.setActiveRootId);

  // Hydrate the roots mirror on mount; the changed-event handler keeps it hot.
  useEffect(() => {
    void refreshSessionsRoots();
  }, []);

  // Keep the row mirror live for the active root.
  useSessionsChanges(activeRoot?.id ?? null);

  const rows = useSessionsListStore((s) => s.rows);
  const text = useSessionsListStore((s) => s.text);
  const status = useSessionsListStore((s) => s.status);
  const pinnedOnly = useSessionsListStore((s) => s.pinnedOnly);
  const unreadOnly = useSessionsListStore((s) => s.unreadOnly);
  const error = useSessionsListStore((s) => s.error);
  const setText = useSessionsListStore((s) => s.setText);
  const setStatus = useSessionsListStore((s) => s.setStatus);
  const setPinnedOnly = useSessionsListStore((s) => s.setPinnedOnly);
  const setUnreadOnly = useSessionsListStore((s) => s.setUnreadOnly);

  const filtered = rows === null ? [] : filterRows(rows, { text, status, pinnedOnly, unreadOnly });
  const anyFilter = text.trim() !== "" || status !== "all" || pinnedOnly || unreadOnly;

  // Opening a session this story means revealing its folder — the detail panel
  // is the next story's surface, and a dead click is worse than an honest one.
  const openRow = useCallback(
    (row: SessionRowVm) => {
      if (activeRoot !== null && canReveal) {
        void revealPath(`${activeRoot.root}/${row.path}`);
      }
    },
    [activeRoot, canReveal],
  );

  return (
    <section
      aria-label={SESSIONS_PANE_TITLE}
      className="flex min-w-0 flex-1 flex-col border-border border-r bg-background last:border-r-0"
    >
      <header className="flex shrink-0 items-start justify-between gap-4 border-border border-b px-6 py-4">
        <div className="min-w-0">
          <h1 className="font-heading text-title">{SESSIONS_PANE_TITLE}</h1>
          <p className="text-muted-foreground text-sm">{SESSIONS_PANE_SUBTITLE}</p>
          {rows !== null && (
            <p role="status" className="figures text-muted-foreground text-xs">
              {countLabel(filtered.length, SESSIONS)}
            </p>
          )}
        </div>
        {activeRoot !== null && (
          <Button
            type="button"
            variant="outline"
            size="sm"
            className="shrink-0"
            onClick={() => void sessionsRescan(activeRoot.id)}
          >
            {SESSIONS_RESCAN_LABEL}
          </Button>
        )}
      </header>

      {/* The root switcher renders only with two or more roots — a label that
          is always the same value is noise (the capture-footer rule). */}
      {roots !== null && roots.length > 1 && (
        <div className="flex shrink-0 gap-1 border-border border-b px-6 py-2">
          {roots.map((root) => (
            <Button
              key={root.id}
              type="button"
              size="sm"
              variant={root.id === activeRoot?.id ? "secondary" : "ghost"}
              onClick={() => setActiveRootId(root.id)}
            >
              {root.name}
              {root.unreadCount > 0 && (
                <Badge variant="secondary" className="figures ml-1">
                  {root.unreadCount}
                </Badge>
              )}
            </Button>
          ))}
        </div>
      )}

      {/* The filter row, above the fold (UX-DR85). */}
      {activeRoot !== null && (
        <div className="flex shrink-0 flex-col gap-2 border-border border-b px-6 py-3">
          <InputGroup>
            <InputGroupInput
              placeholder="Search sessions"
              aria-label="Search sessions"
              value={text}
              onChange={(e) => setText(e.target.value)}
            />
          </InputGroup>
          <div className="flex items-center gap-1">
            {STATUS_CHOICES.map((choice) => (
              <Button
                key={choice.value}
                type="button"
                size="sm"
                variant={status === choice.value ? "secondary" : "ghost"}
                onClick={() => setStatus(choice.value)}
              >
                {choice.label}
              </Button>
            ))}
            <span className="min-w-0 flex-1" />
            <Button
              type="button"
              size="sm"
              variant={pinnedOnly ? "secondary" : "ghost"}
              aria-pressed={pinnedOnly}
              onClick={() => setPinnedOnly(!pinnedOnly)}
            >
              Pinned
            </Button>
            <Button
              type="button"
              size="sm"
              variant={unreadOnly ? "secondary" : "ghost"}
              aria-pressed={unreadOnly}
              onClick={() => setUnreadOnly(!unreadOnly)}
            >
              Unread
            </Button>
          </div>
        </div>
      )}

      <div className="min-h-0 flex-1 overflow-y-auto px-6 py-3">
        {error !== null && (
          <div
            role="alert"
            className="mb-2 rounded-md bg-destructive/10 p-3 text-destructive text-sm"
          >
            {error}
          </div>
        )}
        {roots !== null && roots.length === 0 ? (
          <div className="flex h-full flex-col items-center justify-center gap-2 text-center">
            <p className="font-medium text-sm">{SESSIONS_NO_ROOT_TITLE}</p>
            <p className="max-w-md text-muted-foreground text-sm">{SESSIONS_NO_ROOT_BODY}</p>
          </div>
        ) : rows !== null && filtered.length === 0 && anyFilter ? (
          <p className="py-8 text-center text-muted-foreground text-sm">
            {SESSIONS_NO_MATCH_LABEL}
          </p>
        ) : (
          <ul aria-label={SESSIONS_LIST_LABEL} className="flex flex-col gap-2">
            {filtered.map((row) => (
              <li key={row.id}>
                <SessionRow row={row} onOpen={openRow} />
              </li>
            ))}
          </ul>
        )}
      </div>
    </section>
  );
}
