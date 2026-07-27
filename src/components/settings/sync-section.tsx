/**
 * Settings → Sync section (Epic 29, Stories 29.4 + 29.5, FR-77..FR-93).
 *
 * Lists every configured folder↔repository binding with the status line Rust
 * composed for it, and lets a folder be synced now, paused, resumed, removed,
 * or added. The whole section is capability-gated at its call site
 * (`CapabilitiesVm.sync`): a machine with no usable `git` gets no sync UI at
 * all, never a disabled or failing one.
 *
 * Two rules this surface exists to honor:
 *   - `SyncStatusVm.line` is rendered verbatim. It is composed in Rust so the
 *     tray and this window can never word the same state differently.
 *   - Removing a profile is a configuration change. The folder and everything
 *     in it stay on disk, and the confirmation says so in those words.
 */
import { open as openFolder } from "@tauri-apps/plugin-dialog";
import { ChevronDown, ChevronRight } from "lucide-react";
import { type FormEvent, useEffect, useId, useState } from "react";
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
import { Checkbox } from "@/components/ui/checkbox";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { Progress } from "@/components/ui/progress";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import type { SyncProfileVm, SyncStatusVm } from "@/lib/ipc/client";
import {
  ensureSyncHydrated,
  isSyncStatusActive,
  removeSyncProfile,
  SYNC_DEFAULT_BRANCH,
  SYNC_DEFAULT_LANE,
  SYNC_DEFAULT_LFS_THRESHOLD_BYTES,
  type SyncDirection,
  type SyncLfsMode,
  saveSyncProfile,
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

/** Add-profile form copy. */
export const SYNC_ADD_TITLE = "Add a folder";
export const SYNC_NAME_LABEL = "Name";
export const SYNC_FOLDER_LABEL = "Folder";
export const SYNC_CHOOSE_FOLDER_LABEL = "Choose folder";
export const SYNC_NO_FOLDER_CHOSEN_LABEL = "No folder chosen";
export const SYNC_REMOTE_URL_LABEL = "Remote URL";
export const SYNC_BRANCH_LABEL = "Branch";
export const SYNC_DIRECTION_LABEL = "Direction";
export const SYNC_ADVANCED_LABEL = "Advanced options";
export const SYNC_LFS_MODE_LABEL = "Large files";
export const SYNC_LFS_THRESHOLD_LABEL = "Track files at or above (MB)";
export const SYNC_REMOVABLE_LABEL = "This folder is on removable or network storage";
export const SYNC_REMOVABLE_NOTE =
  "keeper waits longer for writes to settle there, because those volumes report changes late.";
export const SYNC_ADD_SUBMIT_LABEL = "Add folder";

/** Test id for the form's chosen-folder display (a read-only truncated path). */
export const SYNC_FORM_PATH_TESTID = "sync-form-path";

/** Test id for the Advanced disclosure toggle. */
export const SYNC_ADVANCED_TOGGLE_TESTID = "sync-advanced-toggle";

/** Test id for the direction Select trigger. */
export const SYNC_DIRECTION_SELECT_TESTID = "sync-direction-select";

/** Test id for the LFS-mode Select trigger. */
export const SYNC_LFS_SELECT_TESTID = "sync-lfs-select";

/** Honest labels for the three directions (the values mirror Rust). */
const DIRECTION_LABELS: Record<SyncDirection, string> = {
  bidirectional: "Both ways",
  pushOnly: "Push only",
  pullOnly: "Pull only",
};

/** Honest labels for the three LFS modes (the values mirror Rust). */
const LFS_MODE_LABELS: Record<SyncLfsMode, string> = {
  materialize: "Download large files",
  pointerOnly: "Keep pointers only",
  disabled: "Do not use LFS",
};

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
      <AddSyncProfileForm disabled={profiles === null} />
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

/** The add-profile form's fields, all as typed. */
interface SyncFormValues {
  name: string;
  localPath: string;
  remoteUrl: string;
  branch: string;
  direction: SyncDirection;
  lfsMode: SyncLfsMode;
  /** The LFS threshold in MB; empty defers to the Rust default. */
  lfsThresholdMb: string;
  removable: boolean;
}

const EMPTY_FORM: SyncFormValues = {
  name: "",
  localPath: "",
  remoteUrl: "",
  branch: SYNC_DEFAULT_BRANCH,
  direction: "bidirectional",
  lfsMode: "materialize",
  lfsThresholdMb: String(SYNC_DEFAULT_LFS_THRESHOLD_BYTES / 1024 / 1024),
  removable: false,
};

/**
 * Create a profile. A rejected save keeps every typed value and shows the
 * Rust validation message inline — losing a half-typed remote URL to a
 * validation error would be infuriating.
 */
function AddSyncProfileForm({ disabled }: { disabled: boolean }) {
  const [form, setForm] = useState<SyncFormValues>(EMPTY_FORM);
  const [expanded, setExpanded] = useState(false);
  const [saving, setSaving] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const fieldId = useId();

  /** Open the OS-native directory picker; a cancellation writes nothing. */
  const pickFolder = async () => {
    try {
      const selection = await openFolder({ directory: true });
      if (typeof selection === "string") {
        // Merge into the *live* values, not the render snapshot closed over
        // before the picker opened, so a field typed meanwhile survives.
        setForm((live) => ({ ...live, localPath: selection }));
      }
    } catch {
      // Picker cancellation / failure → keep the current folder (no write).
    }
  };

  const submit = async (event: FormEvent<HTMLFormElement>) => {
    event.preventDefault();
    setSaving(true);
    setError(null);
    // An empty or unusable threshold defers to the Rust default rather than
    // sending a number nobody asked for.
    const thresholdMb = Number.parseFloat(form.lfsThresholdMb);
    try {
      await saveSyncProfile({
        id: null,
        name: form.name.trim(),
        localPath: form.localPath,
        remoteUrl: form.remoteUrl.trim(),
        branch: form.branch.trim(),
        direction: form.direction,
        lane: SYNC_DEFAULT_LANE,
        subpaths: [],
        excludes: [],
        removable: form.removable,
        lfsMode: form.lfsMode,
        lfsThresholdBytes:
          Number.isFinite(thresholdMb) && thresholdMb > 0
            ? Math.round(thresholdMb * 1024 * 1024)
            : null,
        settleMs: null,
        tags: [],
      });
      setForm(EMPTY_FORM);
    } catch (raw) {
      setError(syncErrorMessage(raw));
    } finally {
      setSaving(false);
    }
  };

  const incomplete =
    form.name.trim() === "" || form.localPath === "" || form.remoteUrl.trim() === "";

  return (
    <form
      className="mt-1 flex flex-col gap-2 border-border border-t pt-3"
      onSubmit={(event) => {
        void submit(event);
      }}
    >
      <p className="font-medium">{SYNC_ADD_TITLE}</p>
      <div className="flex items-center justify-between gap-2">
        <Label htmlFor={`${fieldId}-name`}>{SYNC_NAME_LABEL}</Label>
        <Input
          id={`${fieldId}-name`}
          className="w-56"
          value={form.name}
          disabled={disabled || saving}
          onChange={(event) => setForm((live) => ({ ...live, name: event.target.value }))}
        />
      </div>
      <div className="flex items-center justify-between gap-2">
        <div className="flex min-w-0 flex-col gap-0.5">
          <Label>{SYNC_FOLDER_LABEL}</Label>
          <p
            className="truncate font-mono text-muted-foreground text-xs"
            data-testid={SYNC_FORM_PATH_TESTID}
            title={form.localPath === "" ? undefined : form.localPath}
          >
            {form.localPath === "" ? SYNC_NO_FOLDER_CHOSEN_LABEL : form.localPath}
          </p>
        </div>
        <Button
          type="button"
          variant="outline"
          size="sm"
          className="shrink-0"
          disabled={disabled || saving}
          onClick={() => {
            void pickFolder();
          }}
        >
          {SYNC_CHOOSE_FOLDER_LABEL}
        </Button>
      </div>
      <div className="flex items-center justify-between gap-2">
        <Label htmlFor={`${fieldId}-remote`}>{SYNC_REMOTE_URL_LABEL}</Label>
        <Input
          id={`${fieldId}-remote`}
          className="w-56"
          value={form.remoteUrl}
          disabled={disabled || saving}
          onChange={(event) => setForm((live) => ({ ...live, remoteUrl: event.target.value }))}
        />
      </div>
      <div className="flex items-center justify-between gap-2">
        <Label htmlFor={`${fieldId}-branch`}>{SYNC_BRANCH_LABEL}</Label>
        <Input
          id={`${fieldId}-branch`}
          className="w-32"
          value={form.branch}
          disabled={disabled || saving}
          onChange={(event) => setForm((live) => ({ ...live, branch: event.target.value }))}
        />
      </div>
      <div className="flex items-center justify-between gap-2">
        <Label id={`${fieldId}-direction-label`}>{SYNC_DIRECTION_LABEL}</Label>
        <Select
          value={form.direction}
          disabled={disabled || saving}
          onValueChange={(value) =>
            setForm((live) => ({ ...live, direction: value as SyncDirection }))
          }
        >
          <SelectTrigger
            className="w-40"
            data-testid={SYNC_DIRECTION_SELECT_TESTID}
            aria-labelledby={`${fieldId}-direction-label`}
          >
            <SelectValue />
          </SelectTrigger>
          <SelectContent>
            {Object.entries(DIRECTION_LABELS).map(([value, label]) => (
              <SelectItem key={value} value={value}>
                {label}
              </SelectItem>
            ))}
          </SelectContent>
        </Select>
      </div>
      <Button
        type="button"
        variant="ghost"
        size="sm"
        className="w-fit justify-start gap-1 px-1"
        data-testid={SYNC_ADVANCED_TOGGLE_TESTID}
        aria-expanded={expanded}
        onClick={() => setExpanded((shown) => !shown)}
      >
        {expanded ? <ChevronDown aria-hidden /> : <ChevronRight aria-hidden />}
        {SYNC_ADVANCED_LABEL}
      </Button>
      {expanded && (
        <div className="flex flex-col gap-2 pl-1">
          <div className="flex items-center justify-between gap-2">
            <Label id={`${fieldId}-lfs-label`}>{SYNC_LFS_MODE_LABEL}</Label>
            <Select
              value={form.lfsMode}
              disabled={disabled || saving}
              onValueChange={(value) =>
                setForm((live) => ({ ...live, lfsMode: value as SyncLfsMode }))
              }
            >
              <SelectTrigger
                className="w-52"
                data-testid={SYNC_LFS_SELECT_TESTID}
                aria-labelledby={`${fieldId}-lfs-label`}
              >
                <SelectValue />
              </SelectTrigger>
              <SelectContent>
                {Object.entries(LFS_MODE_LABELS).map(([value, label]) => (
                  <SelectItem key={value} value={value}>
                    {label}
                  </SelectItem>
                ))}
              </SelectContent>
            </Select>
          </div>
          <div className="flex items-center justify-between gap-2">
            <Label htmlFor={`${fieldId}-threshold`}>{SYNC_LFS_THRESHOLD_LABEL}</Label>
            <Input
              id={`${fieldId}-threshold`}
              type="number"
              min={0}
              className="w-24"
              value={form.lfsThresholdMb}
              disabled={disabled || saving}
              onChange={(event) =>
                setForm((live) => ({ ...live, lfsThresholdMb: event.target.value }))
              }
            />
          </div>
          <div className="flex items-center justify-between gap-2">
            <Label htmlFor={`${fieldId}-removable`}>{SYNC_REMOVABLE_LABEL}</Label>
            <Checkbox
              id={`${fieldId}-removable`}
              checked={form.removable}
              disabled={disabled || saving}
              onCheckedChange={(checked) =>
                setForm((live) => ({ ...live, removable: checked === true }))
              }
            />
          </div>
          <p className="text-muted-foreground text-xs">{SYNC_REMOVABLE_NOTE}</p>
        </div>
      )}
      {error !== null && <p className="text-destructive text-xs">{error}</p>}
      <Button
        type="submit"
        variant="outline"
        size="sm"
        className="w-fit"
        disabled={disabled || saving || incomplete}
      >
        {SYNC_ADD_SUBMIT_LABEL}
      </Button>
    </form>
  );
}
