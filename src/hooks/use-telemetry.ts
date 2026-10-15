import { useEffect, useRef } from "react";
import { logFailure } from "@/lib/failure";
import { type TelemetryEventReq, type TelemetrySurface, telemetryCapture } from "@/lib/ipc/client";

/**
 * No payload, DOM, account context, or exception can cross this boundary. Rust enforces consent.
 * `surface` names a declared `keeper_interaction` surface (a closed set, Epic 100) and is only
 * accepted with `kind: "interaction"`.
 */
export async function captureTelemetry(
  kind: TelemetryEventReq["kind"],
  durationMs: number | null = null,
  surface: TelemetrySurface | null = null,
): Promise<void> {
  try {
    await telemetryCapture({
      kind,
      durationMs:
        durationMs !== null && Number.isFinite(durationMs)
          ? Math.min(60_000, Math.max(0, Math.round(durationMs)))
          : null,
      ...(surface === null ? {} : { surface }),
    });
  } catch (error) {
    // Observability is never on the critical path, including when the IPC service is absent:
    // a refused capture is a log line, never a sentence.
    logFailure("telemetry.capture")(error);
  }
}

/**
 * Count one use of a declared epic-100 surface — the slash menu opened or a command chosen
 * from it, keeper opening on the default agent — as a `keeper_interaction` carrying only that
 * closed name (R381). Never pass anything typed, a room or an agent.
 */
export function captureSurface(
  surface: TelemetrySurface,
  durationMs: number | null = null,
): Promise<void> {
  return captureTelemetry("interaction", durationMs, surface);
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
