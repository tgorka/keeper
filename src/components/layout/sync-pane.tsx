/**
 * The Sync primary view (Epic 32, Story 32.5, AD-S1..AD-S6; Epic 33,
 * Story 33.4).
 *
 * Folder sync worked long before it could be watched: Settings could name a
 * profile's state but never a *file*. This pane answers the three questions a
 * sync tool has to answer — is it working right now and on what, what has it
 * done to my files lately and what is it about to do, and what went wrong —
 * with one card per configured folder: a header (state, the Rust-composed line,
 * progress, the row actions), then Activity, Pending, and a Problems section
 * that exists only while something is wrong.
 *
 * Settings keeps profile *configuration* (AD-S1) — editing, pausing, removing.
 * This view owns activity, state and problems, and never offers Remove. The
 * one configuration act it does offer is the first one: adding a folder, via
 * the shared {@link AddFolderForm} (AD-C7). Until Story 33.4 the empty state
 * named Settings and stopped there, which meant a new user's Sync view could
 * only tell them to leave it.
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
import { FileIcon, FileMinus, FilePen, FilePlus, GitMerge } from "lucide-react";
import { useEffect, useState } from "react";
import {
  SYNC_NOW_LABEL,
  SYNC_PAUSE_LABEL,
  SYNC_PROGRESS_LABEL,
  SYNC_RESUME_LABEL,
  syncRemoteHost,
} from "@/components/settings/sync-section";
import { AddFolderForm, SYNC_ADD_TITLE } from "@/components/sync/add-folder-form";
import { Alert, AlertDescription } from "@/components/ui/alert";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Card, CardContent, CardHeader, CardTitle } from "@/components/ui/card";
import { Progress } from "@/components/ui/progress";
import { ScrollArea } from "@/components/ui/scroll-area";
import { formatDraftAge } from "@/lib/format-time";
import type {
  SyncActivityVm,
  SyncParkedVm,
  SyncPendingVm,
  SyncProblemsVm,
  SyncProfileVm,
  SyncStatusVm,
} from "@/lib/ipc/client";
import {
  ensureSyncHydrated,
  isSyncStatusActive,
  setSyncProfileEnabled,
  startSyncStatusPolling,
  syncErrorMessage,
  syncProfileNow,
  useSyncStore,
} from "@/lib/stores/sync";
import {
  refreshSyncDetail,
  refreshSyncDetailAll,
  retrySyncParked,
  startSyncDetailPolling,
  startSyncProgressStream,
  syncLiveFraction,
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
  "keeper stopped retrying these after repeated failures. Retry puts one back in the queue.";
export const SYNC_PARKED_NO_ERROR_SENTENCE = "No error was recorded.";

/** The parked-unit action label. */
export const SYNC_RETRY_LABEL = "Retry";

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

/** The icon and screen-reader word for each recorded activity kind. */
const ACTIVITY_KINDS: Record<string, { icon: typeof FileIcon; word: string }> = {
  added: { icon: FilePlus, word: "Added" },
  modified: { icon: FilePen, word: "Changed" },
  deleted: { icon: FileMinus, word: "Deleted" },
  conflict: { icon: GitMerge, word: "Conflict copy" },
};

/** Why a file is waiting, for every reason except `settling` (which is timed). */
const PENDING_REASONS: Record<string, string> = {
  untracked: "New file, not synced yet",
  added: "Added, not synced yet",
  modified: "Changed, not synced yet",
  deleted: "Deleted, not synced yet",
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
  return PENDING_REASONS[pending.reason] ?? pending.reason;
}

/** What a parked unit was doing and how hard keeper tried. */
export function syncParkedSummary(unit: SyncParkedVm): string {
  const attempts = unit.attempts === 1 ? "1 attempt" : `${unit.attempts} attempts`;
  return `${PARKED_KINDS[unit.kind] ?? unit.kind} · stopped after ${attempts}`;
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
    void ensureSyncHydrated().then(refreshSyncDetailAll);
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
      className="flex min-w-0 flex-1 flex-col border-border border-r bg-background"
    >
      <header className="flex shrink-0 items-start justify-between gap-4 border-border border-b px-6 py-4">
        <div className="min-w-0">
          <h1 className="font-heading font-medium text-lg">Sync</h1>
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
              this is the one add form; see `showAddForm` above for why. */}
          {showAddForm && (
            <Card size="sm">
              <CardContent>
                <AddFolderForm onAdded={(_profile, settled) => setAdding(!settled)} />
              </CardContent>
            </Card>
          )}
          {profiles?.map((profile) => (
            <SyncProfileCard key={profile.id} profile={profile} status={statuses[profile.id]} />
          ))}
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
   * Run a card action with the shared busy/error lifecycle, then re-read this
   * folder's lists: an action is exactly the moment the three lists are most
   * likely to have moved, and the poll is deliberately too slow to notice.
   */
  const run = async (action: () => Promise<void>) => {
    setBusy(true);
    setActionError(null);
    try {
      await action();
    } catch (raw) {
      setActionError(syncErrorMessage(raw));
    } finally {
      await refreshSyncDetail(profile.id);
      setBusy(false);
    }
  };

  const active = status !== undefined && isSyncStatusActive(status);
  const fraction = syncLiveFraction(status, progress);
  const percent = fraction === null ? null : Math.round(fraction * 100);
  // The path in flight exists only on the stream, so a window that just mounted
  // honestly has none until the next event.
  const current = active ? (progress?.current ?? null) : null;

  return (
    <Card size="sm">
      <CardHeader>
        <div className="flex items-start justify-between gap-3">
          <div className="flex min-w-0 flex-col gap-1">
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
            {/* Verbatim, never recomposed: the tray renders this same sentence. */}
            {status !== undefined && (
              <span className="font-mono text-muted-foreground text-xs">{status.line}</span>
            )}
            {/* Where it lives and where it points, on one line: two stacked
                muted lines under an already-muted status line read as one grey
                block with no shape to it. */}
            <p className="flex min-w-0 items-baseline gap-1.5 text-muted-foreground text-xs">
              <span className="truncate font-mono" title={profile.localPath}>
                {profile.localPath}
              </span>
              <span aria-hidden="true">·</span>
              <span className="shrink-0">{syncRemoteHost(profile.remoteUrl)}</span>
            </p>
          </div>
          <div className="flex shrink-0 items-center gap-1">
            <Button
              type="button"
              variant="outline"
              size="xs"
              disabled={busy}
              onClick={() => {
                void run(async () => {
                  await syncProfileNow(profile.id);
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
        {current !== null && (
          <p className="truncate font-mono text-muted-foreground text-xs" title={current}>
            {/* Under a moving bar the bare path is unambiguous on screen; to a
                screen reader it would be a path from nowhere. */}
            <span className="sr-only">{SYNC_CURRENT_LABEL} </span>
            {current}
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
        <SyncActivityList profile={profile} rows={detail?.activity ?? null} />
        <SyncPendingList profile={profile} rows={detail?.pending ?? null} />
        <SyncProblemsSection
          profile={profile}
          problems={detail?.problems ?? null}
          busy={busy}
          onRetry={(unitId) => {
            void run(async () => {
              await retrySyncParked(profile.id, unitId);
            });
          }}
        />
      </CardContent>
    </Card>
  );
}

/** What sync has done to this folder's files, newest first as Rust ordered it. */
function SyncActivityList({
  profile,
  rows,
}: {
  profile: SyncProfileVm;
  rows: SyncActivityVm[] | null;
}) {
  return (
    <div className="flex flex-col gap-2">
      {/* The project's group-label treatment (the Bridges / Approvals panes and
          the sidebar groups): a quiet micro-label, so the card's own title
          stays the loudest thing in it. */}
      <h2 className="font-medium text-muted-foreground text-xs uppercase tracking-wide">
        {SYNC_ACTIVITY_TITLE}
      </h2>
      {rows === null ? (
        <p className="text-muted-foreground text-xs">{SYNC_LIST_LOADING_SENTENCE}</p>
      ) : rows.length === 0 ? (
        <p className="text-muted-foreground text-xs">{SYNC_ACTIVITY_EMPTY_SENTENCE}</p>
      ) : (
        <ul
          aria-label={`${SYNC_ACTIVITY_TITLE}: ${profile.name}`}
          className="flex flex-col gap-1.5"
        >
          {rows.map((row) => {
            const kind = ACTIVITY_KINDS[row.kind] ?? { icon: FileIcon, word: row.kind };
            const Icon = kind.icon;
            return (
              <li key={`${row.tsMs}-${row.kind}-${row.path}`} className="flex items-center gap-2">
                <Icon aria-hidden="true" className="size-3.5 shrink-0 text-muted-foreground" />
                {/* The kind is carried by an icon, so its word is spoken but not
                    repeated on screen. */}
                <span className="sr-only">{kind.word}</span>
                <span className="min-w-0 flex-1 truncate font-mono text-xs" title={row.path}>
                  {row.path}
                </span>
                <span className="shrink-0 text-muted-foreground text-xs">
                  {formatDraftAge(row.tsMs)}
                </span>
              </li>
            );
          })}
        </ul>
      )}
    </div>
  );
}

/** What sync has seen but not yet carried, and why each one is waiting. */
function SyncPendingList({
  profile,
  rows,
}: {
  profile: SyncProfileVm;
  rows: SyncPendingVm[] | null;
}) {
  const settling = rows?.some((row) => row.reason === "settling");
  return (
    <div className="flex flex-col gap-2">
      <h2 className="font-medium text-muted-foreground text-xs uppercase tracking-wide">
        {SYNC_PENDING_TITLE}
      </h2>
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
            {rows.map((row) => (
              <li key={`${row.reason}-${row.path}`} className="flex items-center gap-3">
                <span className="min-w-0 flex-1 truncate font-mono text-xs" title={row.path}>
                  {row.path}
                </span>
                <span className="shrink-0 text-muted-foreground text-xs">
                  {syncPendingReason(row)}
                </span>
              </li>
            ))}
          </ul>
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
}: {
  profile: SyncProfileVm;
  problems: SyncProblemsVm | null;
  busy: boolean;
  onRetry: (unitId: number) => void;
}) {
  if (
    problems === null ||
    (problems.error === null &&
      problems.warning === null &&
      problems.parked.length === 0 &&
      problems.conflicts.length === 0)
  ) {
    return null;
  }

  return (
    <div className="flex flex-col gap-3">
      <h2 className="font-medium text-muted-foreground text-xs uppercase tracking-wide">
        {SYNC_PROBLEMS_TITLE}
      </h2>
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
          <span aria-hidden="true">⚠</span>
          {problems.warning}
        </p>
      )}
      {problems.conflicts.length > 0 && (
        <div className="flex flex-col gap-1.5">
          <h3 className="font-medium text-xs">{SYNC_CONFLICT_TITLE}</h3>
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
      {problems.parked.length > 0 && (
        <div className="flex flex-col gap-1.5">
          <h3 className="font-medium text-xs">{SYNC_PARKED_TITLE}</h3>
          <p className="text-muted-foreground text-xs">{SYNC_PARKED_SENTENCE}</p>
          <ul aria-label={`${SYNC_PARKED_TITLE}: ${profile.name}`} className="flex flex-col gap-2">
            {problems.parked.map((unit) => {
              const summary = syncParkedSummary(unit);
              return (
                <li key={unit.id} className="flex items-start justify-between gap-3">
                  <div className="flex min-w-0 flex-col gap-0.5">
                    <span className="text-xs">{summary}</span>
                    <span className="font-mono text-destructive text-xs">
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
        </div>
      )}
    </div>
  );
}
