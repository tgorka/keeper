import { useEffect, useRef } from "react";
import { type TelemetryEventReq, telemetryCapture } from "@/lib/ipc/client";

/** No payload, DOM, account context, or exception can cross this boundary. Rust enforces consent. */
export async function captureTelemetry(
  kind: TelemetryEventReq["kind"],
  durationMs: number | null = null,
): Promise<void> {
  try {
    await telemetryCapture({
      kind,
      durationMs:
        durationMs !== null && Number.isFinite(durationMs)
          ? Math.min(60_000, Math.max(0, Math.round(durationMs)))
          : null,
    });
  } catch {
    // Observability is never on the critical path, including when the IPC service is absent.
  }
}

/** Time from navigation to the first painted non-splash UI, not Matrix sync or web vitals. */
export function useTelemetryReadiness(ready: boolean) {
  const reported = useRef(false);
  useEffect(() => {
    if (!ready || reported.current) return;
    let second = 0;
    const first = requestAnimationFrame(() => {
      second = requestAnimationFrame(() => {
        reported.current = true;
        void captureTelemetry("appReady", performance.now());
      });
    });
    return () => {
      cancelAnimationFrame(first);
      cancelAnimationFrame(second);
    };
  }, [ready]);
}

/** Only two declared surfaces; never observes global clicks or recording controls. */
export function useTelemetryOpening(
  kind: "settingsOpened" | "commandPaletteOpened",
  open: boolean,
) {
  const reported = useRef(false);
  useEffect(() => {
    if (!open) {
      reported.current = false;
      return;
    }
    if (reported.current) return;
    const started = performance.now();
    let second = 0;
    const first = requestAnimationFrame(() => {
      second = requestAnimationFrame(() => {
        reported.current = true;
        void captureTelemetry(kind);
        void captureTelemetry("interaction", performance.now() - started);
      });
    });
    return () => {
      cancelAnimationFrame(first);
      cancelAnimationFrame(second);
    };
  }, [kind, open]);
}
