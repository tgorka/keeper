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
import { SESSION_RECORD_NAME, SessionDetail } from "@/components/sessions/session-detail";
import {
  SESSION_PATTERN_INSTALL_FAILED,
  SessionPatternPicker,
} from "@/components/sessions/session-pattern-picker";
import { SessionRow } from "@/components/sessions/session-row";
import { SessionTemplates } from "@/components/sessions/session-templates";
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

/**
 * The templates chip's label — the room, named as the operator named it.
 *
 * A peer of the status chips in the row and nothing like one underneath: see
 * `SessionsBoardMode`. It is not a fourth `STATUS_CHOICES` entry because those
 * values are matched against `row.status`, and a template has no status.
 */
export const SESSIONS_TEMPLATES_LABEL = "Templates";

/**
 * What a refused pattern read says.
 *
 * The read backs two surfaces — the create row's picker and the Templates room —
 * so the sentence names what could not be read rather than which surface asked
 * for it, and it lands in the pane's one alert region beside a failed rows read.
 * Rust's own words come first when it gave any; this is the fallback.
 */
export const SESSIONS_PATTERNS_FAILED =
  "keeper couldn't read what this zone can make a session from.";

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
  const mode = useSessionsListStore((s) => s.mode);
  const setText = useSessionsListStore((s) => s.setText);
  const setStatus = useSessionsListStore((s) => s.setStatus);
  const setPinnedOnly = useSessionsListStore((s) => s.setPinnedOnly);
  const setUnreadOnly = useSessionsListStore((s) => s.setUnreadOnly);
  const setMode = useSessionsListStore((s) => s.setMode);

  // Which room the board is in, read once and named: the templates list stands
  // in for the rows, and the controls that only filter rows stand down.
  const showingTemplates = mode === "templates";

  const filtered = rows === null ? [] : filterRows(rows, { text, status, pinnedOnly, unreadOnly });
  const anyFilter = text.trim() !== "" || status !== "all" || pinnedOnly || unreadOnly;

  // Opening a session opens its RECORD in the panel strip — the SAME file
  // target the Files pane sets and the SAME editor behind it (AD-109,
  // UX-DR91): the target is `(profileId, relativePath)`, the profile id IS
  // the root id (AD-107), and the path is the zone subfolder joined with the
  // session's folder. Everything downstream — the markdown editor, live
  // external changes, the raw/rendered toggle — is Epic 45/46 machinery,
  // reused rather than rebuilt.
  //
  // **The name is imported, not typed.** This composed a literal `README.md`
  // with no shape branch, which was wrong for every flat session on the drive:
  // a flat session's record was `about.md`, so opening a row from here landed
  // the operator on the missing-file sentence. That was broken before Story
  // 52.1 and is fixed here on its own merits — by reading the one constant the
  // detail's own header reads, so the two surfaces cannot come to name
  // different files again.
  const openReadme = useCallback((rootId: string, subfolder: string, sessionPath: string) => {
    panelsStore.getState().setActiveTarget({
      kind: "file",
      profileId: rootId,
      relativePath: `${subfolder}/${sessionPath}/${SESSION_RECORD_NAME}`,
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
  /**
   * Every pattern the zone offers, tagged with the root it was read for, or
   * `null` before the first answer for that root.
   *
   * The tag is the `rowsRootId` stale-guard the rows mirror already uses, and it
   * is here for the same reason: the read is one directory walk PER pattern, so
   * a root switch leaves the previous zone's list in hand for the whole round
   * trip. Untagged, the Templates room drew root A's template headings under
   * root B — with LIVE Rename buttons, each one addressing root B's id with
   * root A's folder name — for the duration of B's read.
   */
  const [patterns, setPatterns] = useState<{
    rootId: string;
    list: SessionPatternVm[];
  } | null>(null);
  /**
   * Rust's refusal of that read, or `null`. Local rather than the rows mirror's
   * `error`: a rows re-read clears that slot, and it would take a pattern-read
   * failure nobody retried with it. Both sentences land in the one alert region
   * below.
   */
  const [patternsError, setPatternsError] = useState<string | null>(null);
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

  // Read the patterns when the create row opens or the Templates room does, and
  // again whenever the zone changes under either — a session created a minute
  // ago is a pattern a minute later, and a stale list would offer an id the
  // shell no longer resolves. The read is one directory walk per pattern; it
  // belongs to those two surfaces, not to the pane, so a board nobody is
  // creating on and nobody has opened Templates on does no walking at all.
  const rowsRootId = useSessionsListStore((s) => s.rowsRootId);
  const rowCount = rows?.length ?? 0;
  const rootId = activeRoot?.id ?? null;
  // Writing keeper's template into the zone creates a pattern without creating
  // a session, so the rows never move and the re-read above never fires. The
  // nonce is that missing signal — one number, bumped by the only verb that
  // needs it, rather than a second copy of the read with its own defaulting.
  const [installNonce, setInstallNonce] = useState(0);
  // biome-ignore lint/correctness/useExhaustiveDependencies: `rowsRootId`/`rowCount`/`installNonce` are re-run triggers, not reads — the zone's changed event re-reads the rows, and this re-reads the patterns behind them so neither an open picker nor the Templates room can show a template the shell no longer resolves.
  useEffect(() => {
    if ((!creating && !showingTemplates) || rootId === null) {
      return;
    }
    let live = true;
    // Cleared as the read starts, so re-entering the room is a retry that says
    // nothing about the attempt before it.
    setPatternsError(null);
    sessionsPatterns(rootId)
      .then((list) => {
        if (!live) {
          return;
        }
        setPatterns({ rootId, list });
        // Default to the zone's own answer, and fall back to the newest pattern
        // in a zone with no `_template/`. Resolving in the setter keeps a
        // user's choice through a re-read: only an id that stopped existing is
        // replaced.
        setPatternId((current) =>
          current !== null && list.some((pattern) => pattern.id === current)
            ? current
            : (list[0]?.id ?? null),
        );
      })
      // A rejection used to be an unhandled one, back when this read only filled
      // a `<Select>` inside a create row the operator had just opened. It now
      // gates a whole board mode: the Templates room waits on `patterns`, so a
      // refusal with no catch left that room on "Reading templates…" forever,
      // with nothing said and nothing to press.
      .catch((raw: unknown) => {
        if (!live) {
          return;
        }
        setPatternsError(syncErrorMessage(raw, SESSIONS_PATTERNS_FAILED));
      });
    return () => {
      live = false;
    };
    // `rowsRootId`/`rowCount` are the zone's own change signal, mirrored: the
    // changed event re-reads the rows, which re-reads the patterns.
  }, [creating, showingTemplates, rootId, rowsRootId, rowCount, installNonce]);

  // The list, resolved against the root asking for it — the `openSessionId`
  // idiom two blocks up, and the same reason: state derived at render cannot
  // lag behind the root the way an effect chasing it can. A switch answers
  // `null` until the new read lands, which is what both consumers already draw
  // as "not here yet".
  const patternList = patterns !== null && patterns.rootId === rootId ? patterns.list : null;

  // Write a template into the zone (FR-268, FR-270): keeper's own skeleton as
  // the zone's `_template/` when no name is given, a named `_template/<name>/`
  // when one is. The name argument has existed on the command since it was
  // written and was dropped here; the Templates room is the surface that has a
  // name to pass. Both callers land on this one function because both need the
  // same thing afterwards — the pattern re-read the nonce triggers.
  //
  // The picker decides whether to OFFER the nameless one — it holds the list
  // that answers "does this zone have a template" — and the pane owns the call.
  // The write goes through the same plan/journal/exec path every lifecycle verb
  // uses, so the zone's history records it exactly as it records a create.
  //
  // Resolves with Rust's refusal sentence, or `null` when the write landed:
  // `installError` is where the create row shows it, and the returned sentence
  // is how the Templates room says it in its own live region without keeping a
  // second copy of this catch.
  const [installing, setInstalling] = useState(false);
  const [installError, setInstallError] = useState<string | null>(null);
  const installTemplate = useCallback(
    (name?: string): Promise<string | null> => {
      if (rootId === null) {
        return Promise.resolve(null);
      }
      setInstalling(true);
      setInstallError(null);
      return sessionsTemplateInstall(rootId, name)
        .then(() => {
          setInstallNonce((n) => n + 1);
          return null;
        })
        .catch((raw: unknown) => {
          const refusal = syncErrorMessage(raw, SESSION_PATTERN_INSTALL_FAILED);
          setInstallError(refusal);
          return refusal;
        })
        .finally(() => setInstalling(false));
    },
    [rootId],
  );

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
            patterns={patternList}
            value={patternId}
            onChange={setPatternId}
            // Wrapped rather than passed: the picker hands its `onClick` straight
            // to this prop, so a bare reference would arrive with a MouseEvent
            // where the template's name goes. The create row's offer is always
            // the zone's own `_template/`, which is the nameless call.
            onInstallTemplate={() => void installTemplate()}
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
          {/* Search filters session rows; in the Templates room it would be
              inert chrome, so it stands down rather than lying about what it
              does. Same for Pinned and Unread below. */}
          {!showingTemplates && (
            <InputGroup>
              <InputGroupInput
                placeholder="Search sessions"
                aria-label="Search sessions"
                value={text}
                onChange={(e) => setText(e.target.value)}
              />
            </InputGroup>
          )}
          {/* Wraps rather than clips: six controls in one row, in a pane whose
              width is the user's business. */}
          <div className="flex flex-wrap items-center gap-1">
            {STATUS_CHOICES.map((choice) => (
              <Button
                key={choice.value}
                type="button"
                size="sm"
                // Nothing reads as chosen while Templates is open: a status chip
                // is a slice of the rows, and the rows are not what is showing.
                variant={!showingTemplates && status === choice.value ? "secondary" : "ghost"}
                onClick={() => {
                  setStatus(choice.value);
                  // A status is an answer about sessions, so asking for one is
                  // also the way back out of the Templates room.
                  setMode("sessions");
                }}
              >
                {choice.label}
              </Button>
            ))}
            {/* A peer of the three above, where the operator asked for it — and
                a mode underneath, not a fourth filter value.

                A toggle, like the Pinned and Unread chips it sits in a row with,
                and for their reason: a chip that draws itself as chosen and does
                nothing when pressed again leaves the way out of the room stated
                in a comment and nowhere in the UI. `aria-pressed` says the same
                thing to a screen reader that `variant` says to an eye. */}
            <Button
              type="button"
              size="sm"
              variant={showingTemplates ? "secondary" : "ghost"}
              aria-pressed={showingTemplates}
              onClick={() => setMode(showingTemplates ? "sessions" : "templates")}
            >
              {SESSIONS_TEMPLATES_LABEL}
            </Button>
            <span className="min-w-0 flex-1" />
            {!showingTemplates && (
              <>
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
              </>
            )}
          </div>
        </div>
      )}

      <div className="min-h-0 flex-1 overflow-y-auto px-6 py-3">
        {(error !== null || patternsError !== null) && (
          <div
            role="alert"
            className="mb-2 rounded-md bg-destructive/10 p-3 text-destructive text-sm"
          >
            {/* Two reads can fail at once — the rows and the pattern list — and
                they are two sentences, not one. Both land in the region the pane
                already has for a read that failed, rather than growing a second
                alert beside it. */}
            {error !== null && <p>{error}</p>}
            {patternsError !== null && <p>{patternsError}</p>}
          </div>
        )}
        {roots !== null && roots.length === 0 ? (
          <div className="flex h-full flex-col items-center justify-center gap-2 text-center">
            <p className="font-medium text-sm">{SESSIONS_NO_ROOT_TITLE}</p>
            <p className="max-w-md text-muted-foreground text-sm">{SESSIONS_NO_ROOT_BODY}</p>
          </div>
        ) : showingTemplates && activeRoot !== null ? (
          // The templates room takes the list's place rather than sitting above
          // it: it is a different thing to look at, not a section of the board.
          // The nonce this hands down is the same one the create row's install
          // bumps — a rename lands in the same re-read a create does.
          //
          // Not drawn at all once the read is refused: the room's only waiting
          // line means "the answer has not arrived yet", and after a refusal it
          // is not coming. The sentence above says why, and leaving the room and
          // coming back re-runs the read — which is the retry, and is why the chip
          // had to become a toggle before this branch could exist.
          patternsError === null && (
            <SessionTemplates
              rootId={activeRoot.id}
              patterns={patternList}
              onInstallTemplate={installTemplate}
              installing={installing}
              onChanged={() => setInstallNonce((n) => n + 1)}
            />
          )
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
