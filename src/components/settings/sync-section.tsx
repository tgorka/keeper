/**
 * Settings → Sync section (Epic 29, Stories 29.4 + 29.5, FR-77..FR-93).
 *
 * Lists every configured folder↔repository binding with the status line Rust
 * composed for it, and lets a folder be synced now, paused, resumed, edited or
 * removed. Both adding and editing are the shared {@link AddFolderForm} (Story
 * 33.4) in its two modes, which the Sync view renders too — the form carries
 * rules Rust enforces, and two copies of it would drift. This is the surface
 * that removes a folder, so it is also the surface where "I typed the remote
 * wrong" used to mean removing it and starting over. The whole section is
 * capability-gated at its call site (`CapabilitiesVm.sync`): a machine with no
 * usable `git` gets no sync UI at all, never a disabled or failing one.
 *
 * Two rules this surface exists to honor:
 *   - `SyncStatusVm.line` is rendered verbatim. It is composed in Rust so the
 *     tray and this window can never word the same state differently.
 *   - Removing a profile is a configuration change. The folder and everything
 *     in it stay on disk, and the confirmation says so in those words.
 */
import { useEffect, useState } from "react";
import { AddFolderForm, SYNC_ADD_TITLE, SYNC_EDIT_TITLE } from "@/components/sync/add-folder-form";
import { Alert, AlertAction, AlertDescription } from "@/components/ui/alert";
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
import { Button } from "@/components/ui/button";
import { Progress } from "@/components/ui/progress";
import type { SyncProfileVm, SyncStatusVm } from "@/lib/ipc/client";
import {
  ensureSyncHydrated,
  isSyncStatusActive,
  removeSyncProfile,
  setSyncProfileEnabled,
  startSyncStatusPolling,
  syncErrorMessage,
  syncProfileNow,
  syncProgressFraction,
  useSyncStore,
  verifySyncProfile,
} from "@/lib/stores/sync";

/** Section heading. */
export const SYNC_SECTION_TITLE = "Sync";

/** The honest scope disclosure for the whole section (project voice). */
export const SYNC_SECTION_SENTENCE =
  "keeper syncs each folder below with the git remote you point it at, and nothing else. Pausing or removing a folder here changes only what keeper does — it never deletes what is in the folder.";

/** Shown once the mirror has loaded and there is genuinely nothing configured. */
export const SYNC_NO_PROFILES_SENTENCE = "No folders are set up to sync yet.";

/** Row action labels. */
export const SYNC_NOW_LABEL = "Sync now";
export const SYNC_PAUSE_LABEL = "Pause";
export const SYNC_RESUME_LABEL = "Resume";
export const SYNC_REMOVE_LABEL = "Remove";

/** The needs-attention alert's inline action: re-check the folder's contents. */
export const SYNC_VERIFY_LABEL = "Check files";

/**
 * The all-clear line after a check that found nothing wrong.
 *
 * Says what the check actually did. keeper records no per-file digest — the
 * pass reads every file (failing only if it changes under the read) and
 * confirms each large-file object is present at its recorded size. Claiming
 * "matched its recorded digest" described a comparison that never happens.
 */
export const SYNC_VERIFY_CLEAN_SENTENCE =
  "Every file read cleanly, and every large file's stored copy is present.";

/** Used only if Rust flagged attention without naming a reason. */
export const SYNC_ATTENTION_FALLBACK_SENTENCE =
  "This folder needs your attention before it can sync again.";

/** Accessible name for a row's progress meter. */
export const SYNC_PROGRESS_LABEL = "Sync progress";

/** The remove confirmation. Says plainly that nothing on disk is deleted. */
export const SYNC_REMOVE_CONFIRM_TITLE = "Stop syncing this folder?";
export const SYNC_REMOVE_CONFIRM_SENTENCE =
  "keeper stops syncing this folder and forgets its settings. The folder and its contents are left on disk exactly as they are — removing a folder never deletes anything.";
export const SYNC_REMOVE_CONFIRM_LABEL = "Stop syncing";
export const SYNC_REMOVE_CANCEL_LABEL = "Keep syncing";

/**
 * The host a git remote points at — the part worth showing in a settings row.
 * Handles the scp-like `git@host:org/repo` form as well as a real URL, and
 * falls back to the raw string when it is neither, so a hand-written remote is
 * still shown honestly rather than blanked.
 */
export function syncRemoteHost(remoteUrl: string): string {
  const trimmed = remoteUrl.trim();
  const scpLike = /^[^/@]+@([^/:]+):/.exec(trimmed);
  if (scpLike !== null) {
    return scpLike[1];
  }
  try {
    const parsed = new URL(trimmed);
    return parsed.host === "" ? trimmed : parsed.host;
  } catch {
    return trimmed;
  }
}

export function SyncSection({ open }: { open: boolean }) {
  const profiles = useSyncStore((state) => state.profiles);
  const statuses = useSyncStore((state) => state.statuses);
  const readError = useSyncStore((state) => state.error);

  // Hydrate on open and poll for as long as the dialog stays open; the stop
  // function tears the poll down on close (and on unmount).
  useEffect(() => {
    if (!open) {
      return;
    }
    void ensureSyncHydrated();
    return startSyncStatusPolling();
  }, [open]);

  return (
    <div className="mt-2 flex flex-col gap-3 border-border border-t pt-3 text-sm">
      <p className="font-medium">{SYNC_SECTION_TITLE}</p>
      <p className="text-muted-foreground">{SYNC_SECTION_SENTENCE}</p>
      {readError !== null && <p className="text-destructive text-xs">{readError}</p>}
      {/* Only claim "nothing configured" once a read has actually landed —
          before that the list is unknown, not empty. */}
      {profiles !== null && profiles.length === 0 && (
        <p className="text-muted-foreground">{SYNC_NO_PROFILES_SENTENCE}</p>
      )}
      {profiles?.map((profile) => {
        // A profile can exist before its first status snapshot arrives; the row
        // then shows no line rather than inventing one.
        const status: SyncStatusVm | undefined = statuses[profile.id];
        return <SyncProfileRow key={profile.id} profile={profile} status={status} />;
      })}
      <div className="mt-1 flex flex-col gap-2 border-border border-t pt-3">
        <p className="font-medium">{SYNC_ADD_TITLE}</p>
        <AddFolderForm disabled={profiles === null} />
      </div>
    </div>
  );
}

/** One configured folder: its Rust-composed line, where it lives, and its actions. */
function SyncProfileRow({
  profile,
  status,
}: {
  profile: SyncProfileVm;
  status: SyncStatusVm | undefined;
}) {
  const [busy, setBusy] = useState(false);
  const [actionError, setActionError] = useState<string | null>(null);
  const [problems, setProblems] = useState<string[] | null>(null);
  const [confirmOpen, setConfirmOpen] = useState(false);
  /**
   * Whether this row's configuration is open for editing. Per row, because the
   * form it reveals is about this folder and nothing else.
   */
  const [editing, setEditing] = useState(false);

  /** Run a row action with the shared busy/error lifecycle. */
  const run = async (action: () => Promise<void>) => {
    setBusy(true);
    setActionError(null);
    try {
      await action();
    } catch (raw) {
      setActionError(syncErrorMessage(raw));
    } finally {
      setBusy(false);
    }
  };

  // `null` = no total known, which the meter renders as indeterminate.
  const fraction = status === undefined ? null : syncProgressFraction(status);
  const percent = fraction === null ? null : Math.round(fraction * 100);

  return (
    <div className="flex flex-col gap-1 rounded-md border border-border p-2">
      <div className="flex items-start justify-between gap-2">
        <div className="flex min-w-0 flex-col gap-0.5">
          <p className="font-medium">{profile.name}</p>
          {/* Verbatim, never recomposed: the tray renders this same sentence. */}
          {status !== undefined && (
            <span className="font-mono text-muted-foreground text-xs">{status.line}</span>
          )}
          <p className="truncate text-muted-foreground text-xs" title={profile.localPath}>
            {profile.localPath}
          </p>
          <p className="text-muted-foreground text-xs">{syncRemoteHost(profile.remoteUrl)}</p>
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
      {status !== undefined && isSyncStatusActive(status) && (
        // Indeterminate (no `aria-valuenow`) whenever no total is known — a
        // meter that invents a percentage is worse than one that admits it
        // cannot say. The shadcn wrapper spends `value` on the bar width and
        // never forwards it to Radix (so its `data-state` reads indeterminate
        // either way), which is why the value semantics are stated here:
        // `aria-valuenow` against Radix's 0..100 range, and an `aria-valuetext`
        // that reuses the Rust-composed line rather than wording progress a
        // second way.
        <Progress
          className="mt-1"
          value={percent}
          aria-label={`${SYNC_PROGRESS_LABEL}: ${profile.name}`}
          aria-valuenow={percent ?? undefined}
          aria-valuetext={status.line}
        />
      )}
      {status?.needsAttention === true && (
        // Persistent and non-modal, the ConversationHealthBanner shape: this
        // condition needs a human, so there is no dismiss control — it clears
        // when the profile recovers, not when it is waved away.
        <Alert role="alert" variant="destructive" className="mt-1">
          <AlertDescription>
            {status.error ?? status.warning ?? SYNC_ATTENTION_FALLBACK_SENTENCE}
          </AlertDescription>
          <AlertAction>
            <Button
              type="button"
              variant="outline"
              size="xs"
              disabled={busy}
              onClick={() => {
                void run(async () => {
                  setProblems(await verifySyncProfile(profile.id));
                });
              }}
            >
              {SYNC_VERIFY_LABEL}
            </Button>
          </AlertAction>
        </Alert>
      )}
      {problems !== null &&
        (problems.length === 0 ? (
          <p className="text-muted-foreground text-xs">{SYNC_VERIFY_CLEAN_SENTENCE}</p>
        ) : (
          <ul className="flex flex-col gap-0.5">
            {problems.map((problem) => (
              <li key={problem} className="font-mono text-destructive text-xs">
                {problem}
              </li>
            ))}
          </ul>
        ))}
      {actionError !== null && <p className="text-destructive text-xs">{actionError}</p>}
      {/* Under everything the row reports about the folder, so opening it never
          pushes a live warning off the top. Mounted fresh on each open, so it
          seeds from the profile as it is now; `settled` false leaves it
          standing over a keychain failure nothing else here would report. */}
      {editing && (
        <AddFolderForm
          className="mt-1 border-border border-t pt-2"
          profile={profile}
          onSaved={(_saved, settled) => setEditing(!settled)}
          onCancel={() => setEditing(false)}
        />
      )}
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
                });
              }}
            >
              {SYNC_REMOVE_CONFIRM_LABEL}
            </AlertDialogAction>
          </AlertDialogFooter>
        </AlertDialogContent>
      </AlertDialog>
    </div>
  );
}
