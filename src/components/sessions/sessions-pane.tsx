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
import { useCallback, useEffect, useState } from "react";
import { SessionActions } from "@/components/sessions/session-actions";
import { SessionDetail } from "@/components/sessions/session-detail";
import {
  SESSION_PATTERN_INSTALL_FAILED,
  SESSION_PATTERN_INSTALL_TITLE,
  SessionPatternPicker,
} from "@/components/sessions/session-pattern-picker";
import { SessionRow } from "@/components/sessions/session-row";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { InputGroup, InputGroupInput } from "@/components/ui/input-group";
import { useSessionsChanges } from "@/hooks/use-sessions-changes";
import { countLabel, SESSIONS } from "@/lib/count-label";
import type { SessionPatternVm, SessionRowVm } from "@/lib/ipc/client";
import {
  sessionsCreate,
  sessionsPatterns,
  sessionsRescan,
  sessionsTemplateInstall,
} from "@/lib/ipc/client";
import { panelsStore } from "@/lib/stores/panels";
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
import { syncErrorMessage } from "@/lib/stores/sync";

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

/**
 * The create verb (FR-238, FR-253): the title, and what to shape the session
 * from — one row, one Create. The pattern defaults to the zone's own template
 * and is a change away from being the session you were just in.
 */
export const SESSIONS_NEW_LABEL = "New session";
export const SESSIONS_NEW_TITLE_LABEL = "Session title";
export const SESSIONS_NEW_CONFIRM_LABEL = "Create";

/** The status chips, in board order. */
const STATUS_CHOICES: { value: SessionsStatusFilter; label: string }[] = [
  { value: "all", label: "All" },
  { value: "active", label: "Active" },
  { value: "archived", label: "Archived" },
];

export function SessionsPane() {
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

  // Opening a session opens its README in the panel strip — the SAME file
  // target the Files pane sets and the SAME editor behind it (AD-109,
  // UX-DR91): the target is `(profileId, relativePath)`, the profile id IS
  // the root id (AD-107), and the path is the zone subfolder joined with the
  // session's folder. Everything downstream — the markdown editor, live
  // external changes, the raw/rendered toggle — is Epic 45/46 machinery,
  // reused rather than rebuilt.
  const openReadme = useCallback((rootId: string, subfolder: string, sessionPath: string) => {
    panelsStore.getState().setActiveTarget({
      kind: "file",
      profileId: rootId,
      relativePath: `${subfolder}/${sessionPath}/README.md`,
    });
  }, []);
  // A row click drills into the detail (FR-233): the rendered log, the file
  // sections, the properties widget. The README stays one click away from
  // there — a review first, an edit second.
  //
  // The open id carries the ROOT it belongs to and is resolved in render:
  // a root switch changes the resolution to null (a session id is scoped to
  // its root), and a session that vanished from the rows (deleted; an
  // archived one keeps its id) resolves to null too — the board comes back,
  // never a dead pane. State derived at render beats a pair of effects that
  // chase it (and is exactly what the exhaustive-deps lint pushes toward).
  const [openSession, setOpenSession] = useState<{ rootId: string; id: string } | null>(null);
  const openRow = useCallback(
    (row: SessionRowVm) => {
      if (activeRoot !== null) {
        setOpenSession({ rootId: activeRoot.id, id: row.id });
      }
    },
    [activeRoot],
  );
  const openSessionId =
    openSession !== null &&
    openSession.rootId === activeRoot?.id &&
    (rows === null || rows.some((row) => row.id === openSession.id))
      ? openSession.id
      : null;

  // The create row, revealed in place, no dialog (FR-238): the title, and what
  // the session is shaped from (FR-253). Create lands the folder, the changed
  // event brings the row, and the README opens with the caret ready.
  const [creating, setCreating] = useState(false);
  const [newTitle, setNewTitle] = useState("");
  const [patterns, setPatterns] = useState<SessionPatternVm[] | null>(null);
  const [patternId, setPatternId] = useState<string | null>(null);
  // The palette's New Session bumps the nonce; the board answers by opening
  // its create row — the vault-switcher idiom (FR-251). A row's "New like
  // this" bumps the same nonce with its own id, so both verbs land on ONE
  // surface with one already chosen (FR-253).
  const createNonce = useSessionsListStore((s) => s.createNonce);
  const createPatternId = useSessionsListStore((s) => s.createPatternId);
  useEffect(() => {
    if (createNonce > 0) {
      setCreating(true);
      if (createPatternId !== null) {
        setPatternId(createPatternId);
      }
    }
  }, [createNonce, createPatternId]);

  // Read the patterns when the row opens, and again whenever the zone changes
  // under it — a session created a minute ago is a pattern a minute later, and
  // a stale list would offer an id the shell no longer resolves. The read is
  // one directory walk per pattern; it belongs to the open row, not the pane,
  // so a board nobody is creating on does no walking at all.
  const rowsRootId = useSessionsListStore((s) => s.rowsRootId);
  const rowCount = rows?.length ?? 0;
  const rootId = activeRoot?.id ?? null;
  // Writing keeper's template into the zone creates a pattern without creating
  // a session, so the rows never move and the re-read above never fires. The
  // nonce is that missing signal — one number, bumped by the only verb that
  // needs it, rather than a second copy of the read with its own defaulting.
  const [installNonce, setInstallNonce] = useState(0);
  // biome-ignore lint/correctness/useExhaustiveDependencies: `rowsRootId`/`rowCount`/`installNonce` are re-run triggers, not reads — the zone's changed event re-reads the rows, and this re-reads the patterns behind them so an open picker cannot offer an id the shell no longer resolves.
  useEffect(() => {
    if (!creating || rootId === null) {
      return;
    }
    let live = true;
    void sessionsPatterns(rootId).then((list) => {
      if (!live) {
        return;
      }
      setPatterns(list);
      // Default to the zone's own answer, and fall back to the newest pattern
      // in a zone with no `_template/`. Resolving in the setter keeps a
      // user's choice through a re-read: only an id that stopped existing is
      // replaced.
      setPatternId((current) =>
        current !== null && list.some((pattern) => pattern.id === current)
          ? current
          : (list[0]?.id ?? null),
      );
    });
    return () => {
      live = false;
    };
    // `rowsRootId`/`rowCount` are the zone's own change signal, mirrored: the
    // changed event re-reads the rows, which re-reads the patterns.
  }, [creating, rootId, rowsRootId, rowCount, installNonce]);

  // Adopt keeper's default as the zone's own `_template/` (FR-268). The picker
  // decides whether to offer this — it holds the list that answers "does this
  // zone have a template" — and the pane owns the call, because the pane owns
  // the read that has to happen afterwards. The write goes through the same
  // plan/journal/exec path every lifecycle verb uses, so the zone's history
  // records it exactly as it records a create.
  const [installing, setInstalling] = useState(false);
  const [installError, setInstallError] = useState<string | null>(null);
  const installTemplate = useCallback(() => {
    if (rootId === null) {
      return;
    }
    setInstalling(true);
    setInstallError(null);
    sessionsTemplateInstall(rootId, undefined, SESSION_PATTERN_INSTALL_TITLE)
      .then(() => setInstallNonce((n) => n + 1))
      .catch((raw: unknown) =>
        setInstallError(syncErrorMessage(raw, SESSION_PATTERN_INSTALL_FAILED)),
      )
      .finally(() => setInstalling(false));
  }, [rootId]);

  const closeCreate = useCallback(() => {
    setCreating(false);
    setNewTitle("");
  }, []);
  const submitCreate = useCallback(() => {
    const title = newTitle.trim();
    if (activeRoot === null || title === "") {
      return;
    }
    void sessionsCreate(activeRoot.id, title, patternId ?? undefined).then((ref) => {
      closeCreate();
      openReadme(ref.rootId, activeRoot.subfolder, ref.path);
    });
  }, [activeRoot, newTitle, patternId, closeCreate, openReadme]);

  // Drilled in: the detail replaces the board's body wholesale — the header
  // stays (New/Rescan keep working), the filter row and list yield. One pane,
  // two depths; the panel strip beside carries whatever files get opened.
  if (openSessionId !== null && activeRoot !== null) {
    return (
      <section
        aria-label={SESSIONS_PANE_TITLE}
        className="flex min-w-0 flex-1 flex-col border-border border-r bg-background last:border-r-0"
      >
        <SessionDetail
          rootId={activeRoot.id}
          subfolder={activeRoot.subfolder}
          sessionId={openSessionId}
          onBack={() => setOpenSession(null)}
        />
      </section>
    );
  }

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
          <div className="flex shrink-0 gap-2">
            <Button type="button" size="sm" onClick={() => setCreating((open) => !open)}>
              {SESSIONS_NEW_LABEL}
            </Button>
            <Button
              type="button"
              variant="outline"
              size="sm"
              onClick={() => void sessionsRescan(activeRoot.id)}
            >
              {SESSIONS_RESCAN_LABEL}
            </Button>
          </div>
        )}
      </header>

      {/* The create row, revealed in place — no dialog (FR-238, the capture
          no-filing philosophy). The title stays the one field you must fill;
          the pattern below it is already answered and shows its consequence
          (FR-253). Escape closes; Enter creates. */}
      {creating && activeRoot !== null && (
        <div className="flex shrink-0 flex-col gap-2 border-border border-b px-6 py-3">
          <div className="flex gap-2">
            <InputGroup>
              <InputGroupInput
                // biome-ignore lint/a11y/noAutofocus: the row exists because the
                // user just pressed New session; the title is the one question.
                autoFocus
                placeholder={SESSIONS_NEW_TITLE_LABEL}
                aria-label={SESSIONS_NEW_TITLE_LABEL}
                value={newTitle}
                onChange={(e) => setNewTitle(e.target.value)}
                onKeyDown={(e) => {
                  if (e.key === "Enter") {
                    submitCreate();
                  }
                  if (e.key === "Escape") {
                    closeCreate();
                  }
                }}
              />
            </InputGroup>
            <Button type="button" size="sm" onClick={submitCreate}>
              {SESSIONS_NEW_CONFIRM_LABEL}
            </Button>
          </div>
          <SessionPatternPicker
            patterns={patterns}
            value={patternId}
            onChange={setPatternId}
            onInstallTemplate={installTemplate}
            installing={installing}
            installError={installError}
          />
        </div>
      )}

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
                <SessionRow
                  row={row}
                  onOpen={openRow}
                  actions={
                    activeRoot !== null ? (
                      <SessionActions rootId={activeRoot.id} rootPath={activeRoot.root} row={row} />
                    ) : undefined
                  }
                />
              </li>
            ))}
          </ul>
        )}
      </div>
    </section>
  );
}
