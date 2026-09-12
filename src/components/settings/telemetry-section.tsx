import { listen } from "@tauri-apps/api/event";
import { useEffect, useRef, useState } from "react";
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
import { Label } from "@/components/ui/label";
import { Switch } from "@/components/ui/switch";
import {
  type TelemetryConsentVm,
  type TelemetryStatusVm,
  telemetryConsentSet,
  telemetryRemoteConfig,
  telemetryStatus,
} from "@/lib/ipc/client";

const CATEGORIES = [
  {
    key: "diagnostics",
    label: "Optional diagnostics",
    detail:
      "Content-free readiness and interaction timings, fixed error categories and operation traces. Never app logs or exception text.",
  },
  {
    key: "productAnalytics",
    label: "Optional product statistics",
    detail:
      "Counts of opening Settings and the command palette. No search text, messages or account identifiers.",
  },
  {
    key: "remoteConfig",
    label: "Optional remote configuration",
    detail:
      "Fetch a plain support message for this section. Cannot enable collection or change your settings, security or destinations.",
  },
] as const;

export function TelemetrySection({ open }: { open: boolean }) {
  const [status, setStatus] = useState<TelemetryStatusVm | null>(null);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState(false);
  const [message, setMessage] = useState<string | null>(null);
  const [confirmStudy, setConfirmStudy] = useState(false);
  const generation = useRef(0);
  const writing = useRef(false);

  useEffect(() => {
    if (!open) return;
    let alive = true;
    let reading = false;
    let unlisten: (() => void) | undefined;
    void listen<TelemetryStatusVm>("telemetry-status-changed", ({ payload }) => {
      if (!alive) return;
      generation.current++;
      setStatus(payload);
      if (!payload.consent.remoteConfig) setMessage(null);
      window.dispatchEvent(new Event("keeper-telemetry-consent-changed"));
    })
      .then((dispose) => {
        if (alive) unlisten = dispose;
        else dispose();
      })
      .catch(() => {
        // Focus/periodic local reads remain the fail-closed fallback if subscription fails.
      });
    const refresh = async () => {
      if (reading || writing.current) return;
      reading = true;
      const request = ++generation.current;
      try {
        const next = await telemetryStatus();
        if (alive && request === generation.current) {
          setStatus(next);
          setError(false);
        }
      } catch {
        if (alive && request === generation.current) {
          setStatus(null);
          setError(true);
        }
      } finally {
        reading = false;
      }
    };
    void refresh();
    // This reads local policy only. No SDK, flags request, or export starts here.
    const timer = setInterval(() => {
      void refresh();
    }, 2_000);
    window.addEventListener("focus", refresh);
    return () => {
      alive = false;
      generation.current++;
      unlisten?.();
      clearInterval(timer);
      window.removeEventListener("focus", refresh);
    };
  }, [open]);

  const remoteEnabled = open && status?.configured === true && status.consent.remoteConfig;
  useEffect(() => {
    setMessage(null);
    if (!remoteEnabled) return;
    let cancelled = false;
    void telemetryRemoteConfig()
      .then((config) => {
        if (!cancelled) setMessage(config.supportMessage);
      })
      .catch(() => {
        // Shipped fallback is no message. Never display an arbitrary IPC error.
      });
    return () => {
      cancelled = true;
    };
  }, [remoteEnabled]);

  const change = async (key: keyof TelemetryConsentVm, enabled: boolean) => {
    if (!status || writing.current) return;
    writing.current = true;
    setBusy(true);
    setError(false);
    const request = ++generation.current;
    if (key === "remoteConfig" && !enabled) setMessage(null);
    try {
      const next = await telemetryConsentSet({ ...status.consent, [key]: enabled });
      if (request === generation.current) setStatus(next);
      window.dispatchEvent(new Event("keeper-telemetry-consent-changed"));
    } catch {
      setError(true);
      if (request === generation.current) {
        // Never pretend a rejected write saved, especially a failed revocation.
        setStatus(null);
        setError(true);
      }
    } finally {
      writing.current = false;
      setBusy(false);
    }
  };

  return (
    <section
      className="flex min-w-0 flex-col gap-3 border-t pt-4 text-sm"
      aria-labelledby="telemetry-title"
    >
      <h3 id="telemetry-title" className="font-medium">
        Diagnostics & studies
      </h3>
      <p className="text-muted-foreground text-xs">
        Off by default, on this installation only. Each choice is independent. Turning a category
        off discards unsent records and stops future collection. Data already sent cannot be
        recalled here. No Matrix IDs, messages, filenames, paths, notes, bot content or recordings
        are sent.
      </p>
      {status ? (
        <p className="break-words text-xs" role="status">
          {!status.configured
            ? "PostHog is not configured in this build. No observability requests can be sent."
            : Object.values(status.consent).some(Boolean)
              ? `Enabled destination: ${status.host}`
              : `No active observability destination. If enabled: ${status.host}`}
        </p>
      ) : (
        <p className="text-muted-foreground text-xs">
          Local consent state unavailable; controls remain off until read.
        </p>
      )}
      {CATEGORIES.map(({ key, label, detail }) => (
        <div key={key} className="flex items-start justify-between gap-3">
          <div className="min-w-0">
            <Label htmlFor={`telemetry-${key}`}>{label}</Label>
            <p className="text-muted-foreground text-xs">{detail}</p>
          </div>
          <Switch
            id={`telemetry-${key}`}
            checked={status?.consent[key] ?? false}
            disabled={!status || busy || (!status.configured && !status.consent[key])}
            onCheckedChange={(checked) => {
              void change(key, checked);
            }}
          />
        </div>
      ))}
      {error && (
        <p role="alert" className="text-held text-xs">
          Could not read or save local consent. Telemetry may be off only for this session; saved
          consent may remain. Retry saving before restarting the app.
        </p>
      )}
      {remoteEnabled && message && (
        <section className="whitespace-pre-wrap break-words text-xs" aria-label="Support message">
          {message}
        </section>
      )}
      <div className="flex flex-col items-start gap-2">
        <p className="text-muted-foreground text-xs">
          Usability studies use a separate page of synthetic tasks, never this app. Opening it
          replaces this app document; returning reloads the app rather than restoring this view.
          Save any unfinished edits first. Capture still requires an explicit Start.
        </p>
        <Button
          type="button"
          variant="outline"
          size="sm"
          disabled={!status?.configured || busy}
          onClick={() => setConfirmStudy(true)}
        >
          Open synthetic usability study
        </Button>
      </div>
      <AlertDialog open={confirmStudy} onOpenChange={setConfirmStudy}>
        <AlertDialogContent>
          <AlertDialogHeader>
            <AlertDialogTitle>Leave the app for synthetic tasks?</AlertDialogTitle>
            <AlertDialogDescription>
              This replaces the whole app document. Unsaved edits may be lost. Cancel to finish and
              save them first. Returning from the study reloads the app.
            </AlertDialogDescription>
          </AlertDialogHeader>
          <AlertDialogFooter>
            <AlertDialogCancel>Keep editing</AlertDialogCancel>
            <AlertDialogAction
              disabled={!status?.configured || busy}
              onClick={() => window.location.assign("study.html")}
            >
              Leave app and open study
            </AlertDialogAction>
          </AlertDialogFooter>
        </AlertDialogContent>
      </AlertDialog>
    </section>
  );
}
