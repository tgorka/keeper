/**
 * The archive Export dialog (Story 5.5, FR-35, AD-11, UX-DR11).
 *
 * Opened from the conversation header or search results with a scope preset. It
 * offers a scope picker (this Chat / this Account / everything), format checkboxes
 * (JSON, Markdown — at least one required), an include-media toggle, and a
 * destination folder chosen via the dialog plugin. Start spawns a background export
 * that streams progress into {@link exportStore}; while running it shows a
 * {@link Progress} bar with counts and a Cancel button. On success it fires a Sonner
 * toast with a Reveal-in-Finder action (and an in-dialog Reveal), and on failure it
 * shows a **persistent** {@link AlertDialog} (never toast-only) noting that partial
 * output was cleaned up. The dialog reads `archive.db` only — it never touches a
 * live session, so a signed-out Account is still exportable.
 */
import { open as openFolder } from "@tauri-apps/plugin-dialog";
import { useCallback, useEffect, useMemo, useState } from "react";
import { toast } from "sonner";
import {
  AlertDialog,
  AlertDialogAction,
  AlertDialogContent,
  AlertDialogDescription,
  AlertDialogFooter,
  AlertDialogHeader,
  AlertDialogTitle,
} from "@/components/ui/alert-dialog";
import { Button } from "@/components/ui/button";
import { Checkbox } from "@/components/ui/checkbox";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import { Label } from "@/components/ui/label";
import { Progress } from "@/components/ui/progress";
import { RadioGroup, RadioGroupItem } from "@/components/ui/radio-group";
import { Switch } from "@/components/ui/switch";
import type { ExportRequestVm, ExportScopeKind, IpcError } from "@/lib/ipc/client";
import { cancelExport, revealPath, startExport } from "@/lib/ipc/client";
import { exportStore, useExportStore } from "@/lib/stores/export";

/** Structural guard for the IpcError envelope surfaced on a start rejection. */
function isIpcError(value: unknown): value is IpcError {
  if (typeof value !== "object" || value === null) {
    return false;
  }
  const v = value as Record<string, unknown>;
  return typeof v.code === "string" && typeof v.message === "string";
}

export function ExportDialog() {
  const isOpen = useExportStore((s) => s.isOpen);
  const preset = useExportStore((s) => s.preset);

  // The terminal-state surfaces (success toast + persistent failure alert) live in
  // an always-mounted watcher, NOT inside the dialog body — a background export can
  // finish or fail after the user has closed the dialog (the footer offers "Close"
  // while running), and UX-DR11's persistent failure alert must still appear. The
  // dialog body itself remounts per open (keyed on the preset) so the form seeds
  // cleanly each time.
  return (
    <>
      <ExportJobWatcher />
      {isOpen ? (
        <ExportDialogInner key={`${preset.scope}|${preset.accountId}|${preset.roomId}`} />
      ) : null}
    </>
  );
}

/**
 * Always-mounted watcher for an export job's terminal state (Story 5.5, UX-DR11).
 * Independent of whether the dialog is open, so closing the dialog mid-run never
 * swallows the success toast or the persistent failure alert. Fires the completion
 * toast once per job and renders the failure {@link AlertDialog} until dismissed.
 */
function ExportJobWatcher() {
  const job = useExportStore((s) => s.job);
  const [toastedId, setToastedId] = useState<number | null>(null);

  const failed = job !== null && job.phase === "failed";

  // Fire the success toast exactly once per completed job, from an effect so it is
  // never a render-time side effect (safe under StrictMode double-invoke).
  useEffect(() => {
    if (job === null || job.phase !== "completed" || toastedId === job.exportId) {
      return;
    }
    setToastedId(job.exportId);
    const firstPath = job.outputPaths[0];
    toast.success("Export complete", {
      description: `${job.messagesWritten} message(s) written.`,
      action:
        firstPath !== undefined
          ? {
              label: "Reveal in Finder",
              onClick: () => {
                revealPath(firstPath).catch(() => {});
              },
            }
          : undefined,
    });
  }, [job, toastedId]);

  const acknowledgeFailure = useCallback(() => {
    exportStore.getState().clearJob();
  }, []);

  return (
    <AlertDialog open={failed}>
      <AlertDialogContent>
        <AlertDialogHeader>
          <AlertDialogTitle>Export failed</AlertDialogTitle>
          <AlertDialogDescription>
            {job?.error ?? "The export could not be completed."} Any partial files were deleted.
          </AlertDialogDescription>
        </AlertDialogHeader>
        <AlertDialogFooter>
          <AlertDialogAction onClick={acknowledgeFailure}>Dismiss</AlertDialogAction>
        </AlertDialogFooter>
      </AlertDialogContent>
    </AlertDialog>
  );
}

/**
 * The dialog body, remounted per open so the form seeds cleanly from the preset.
 * Split out so a fresh `useState` seed runs on every open without an effect.
 */
function ExportDialogInner() {
  const preset = useExportStore((s) => s.preset);
  const job = useExportStore((s) => s.job);

  const [scope, setScope] = useState<ExportScopeKind>(preset.scope);
  const [json, setJson] = useState(true);
  const [markdown, setMarkdown] = useState(true);
  const [includeMedia, setIncludeMedia] = useState(false);
  const [destination, setDestination] = useState<string | null>(null);
  const [startError, setStartError] = useState<IpcError | null>(null);

  // A `chat` scope needs the preset's room; `account` needs its account. When the
  // preset lacks them (e.g. opened from global search), those scopes are disabled.
  const chatAvailable = preset.accountId !== null && preset.roomId !== null;
  const accountAvailable = preset.accountId !== null;

  const atLeastOneFormat = json || markdown;
  const running = job !== null && job.phase === "running";
  const completed = job !== null && job.phase === "completed";
  const cancelled = job !== null && job.phase === "cancelled";

  const canStart = atLeastOneFormat && destination !== null && !running;

  const close = useCallback(() => exportStore.getState().close(), []);

  const pickDestination = useCallback(async () => {
    try {
      const selection = await openFolder({ directory: true });
      if (typeof selection === "string") {
        setDestination(selection);
      }
    } catch {
      // Folder-picker cancellation / failure → keep the current destination.
    }
  }, []);

  const onStart = useCallback(async () => {
    if (!atLeastOneFormat || destination === null) {
      return;
    }
    setStartError(null);
    const request: ExportRequestVm = {
      scope,
      accountId: scope === "chat" || scope === "account" ? preset.accountId : null,
      roomId: scope === "chat" ? preset.roomId : null,
      json,
      markdown,
      includeMedia,
      destinationDir: destination,
    };
    try {
      const exportId = await startExport(request, (batch) =>
        exportStore.getState().applyProgress(batch),
      );
      exportStore.getState().startJob(exportId);
    } catch (e: unknown) {
      setStartError(
        isIpcError(e)
          ? e
          : { code: "internal", message: String(e), accountId: null, retriable: false },
      );
    }
  }, [atLeastOneFormat, destination, scope, preset, json, markdown, includeMedia]);

  const onCancel = useCallback(() => {
    if (job !== null) {
      cancelExport(job.exportId).catch(() => {});
    }
  }, [job]);

  const progressValue = useMemo(() => {
    if (job === null || job.totalMessages === null || job.totalMessages === 0) {
      return null;
    }
    return Math.min(100, Math.round((job.messagesWritten / job.totalMessages) * 100));
  }, [job]);

  const onOpenChange = useCallback(
    (next: boolean) => {
      if (!next) {
        close();
      }
    },
    [close],
  );

  return (
    <Dialog open onOpenChange={onOpenChange}>
      <DialogContent className="max-w-md" aria-label="Export your local archive">
        <DialogHeader>
          <DialogTitle>Export archive</DialogTitle>
          <DialogDescription>
            Export from your local archive on this Mac. Works offline, even for a signed-out
            account.
          </DialogDescription>
        </DialogHeader>

        {/* Scope */}
        <fieldset className="flex flex-col gap-2" disabled={running}>
          <legend className="text-sm font-medium">Scope</legend>
          <RadioGroup
            value={scope}
            onValueChange={(v) => setScope(v as ExportScopeKind)}
            aria-label="Export scope"
          >
            <div className="flex items-center gap-2">
              <RadioGroupItem value="chat" id="export-scope-chat" disabled={!chatAvailable} />
              <Label htmlFor="export-scope-chat" className={chatAvailable ? "" : "opacity-50"}>
                This chat
              </Label>
            </div>
            <div className="flex items-center gap-2">
              <RadioGroupItem
                value="account"
                id="export-scope-account"
                disabled={!accountAvailable}
              />
              <Label
                htmlFor="export-scope-account"
                className={accountAvailable ? "" : "opacity-50"}
              >
                This account
              </Label>
            </div>
            <div className="flex items-center gap-2">
              <RadioGroupItem value="everything" id="export-scope-everything" />
              <Label htmlFor="export-scope-everything">Everything</Label>
            </div>
          </RadioGroup>
        </fieldset>

        {/* Formats */}
        <fieldset className="flex flex-col gap-2" disabled={running}>
          <legend className="text-sm font-medium">Formats</legend>
          <div className="flex items-center gap-2">
            <Checkbox
              id="export-json"
              checked={json}
              onCheckedChange={(c) => setJson(c === true)}
            />
            <Label htmlFor="export-json">JSON (lossless)</Label>
          </div>
          <div className="flex items-center gap-2">
            <Checkbox
              id="export-markdown"
              checked={markdown}
              onCheckedChange={(c) => setMarkdown(c === true)}
            />
            <Label htmlFor="export-markdown">Markdown transcript</Label>
          </div>
          {!atLeastOneFormat && (
            <p className="text-xs text-destructive" role="alert">
              Select at least one format.
            </p>
          )}
        </fieldset>

        {/* Include media */}
        <div className="flex items-center justify-between gap-2">
          <Label htmlFor="export-media">Include media files</Label>
          <Switch
            id="export-media"
            checked={includeMedia}
            onCheckedChange={setIncludeMedia}
            disabled={running}
          />
        </div>

        {/* Destination */}
        <div className="flex flex-col gap-1">
          <Label>Destination</Label>
          <div className="flex items-center gap-2">
            <Button
              type="button"
              variant="outline"
              size="sm"
              onClick={pickDestination}
              disabled={running}
            >
              Choose folder…
            </Button>
            <span className="truncate text-xs text-muted-foreground" title={destination ?? ""}>
              {destination ?? "No folder chosen"}
            </span>
          </div>
        </div>

        {/* Running progress + cancel */}
        {running && job !== null && (
          <div className="flex flex-col gap-2" role="status" aria-label="Export progress">
            <Progress value={progressValue ?? undefined} />
            <p className="text-xs text-muted-foreground">
              {job.messagesWritten}
              {job.totalMessages !== null ? ` / ${job.totalMessages}` : ""} messages
              {includeMedia
                ? ` · ${job.mediaCopied} media copied, ${job.mediaSkipped} skipped`
                : ""}
            </p>
          </div>
        )}

        {/* Completed / cancelled inline note + Reveal */}
        {completed && job !== null && (
          <div className="flex items-center justify-between gap-2 rounded-md bg-accent/50 p-2 text-sm">
            <span>Export complete ({job.messagesWritten} messages).</span>
            {job.outputPaths[0] !== undefined && (
              <Button
                type="button"
                variant="ghost"
                size="sm"
                onClick={() => {
                  const p = job.outputPaths[0];
                  if (p !== undefined) {
                    revealPath(p).catch(() => {});
                  }
                }}
              >
                Reveal in Finder
              </Button>
            )}
          </div>
        )}
        {cancelled && (
          <p className="rounded-md bg-muted p-2 text-sm text-muted-foreground" role="status">
            Export cancelled. Any partial files were deleted.
          </p>
        )}

        {startError !== null && (
          <p className="text-xs text-destructive" role="alert">
            Could not start export: {startError.message}
          </p>
        )}

        <DialogFooter>
          <Button type="button" variant="ghost" onClick={close}>
            {running ? "Close" : "Done"}
          </Button>
          {running ? (
            <Button type="button" variant="destructive" onClick={onCancel}>
              Cancel export
            </Button>
          ) : (
            <Button type="button" onClick={onStart} disabled={!canStart}>
              Start export
            </Button>
          )}
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}
