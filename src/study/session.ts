import { telemetryStudyConfig, telemetryStudyStop } from "./commands";
import type { StudyRecorder } from "./recorder";
import { installStudyTransport, type StudyTransport } from "./transport";

export interface StudyCapture {
  stop(): Promise<void>;
}

/** Every awaited start edge rechecks cancellation before it can load or initialize capture. */
export async function startStudyCapture(
  signal: AbortSignal,
  onFailure: () => void,
): Promise<StudyCapture> {
  let recorder: StudyRecorder | undefined;
  let transport: StudyTransport | undefined;
  let stopped: Promise<void> | undefined;
  let failed = false;
  let cancelReadiness: (() => void) | undefined;
  const stop = () => {
    if (stopped) return stopped;
    // Close and abort synchronously, BEFORE SDK cleanup can flush or IPC clears disclosure.
    transport?.stop();
    cancelReadiness?.();
    signal.removeEventListener("abort", abort);
    // Defer SDK cleanup so reentrant SDK failure callbacks see the same stop promise.
    stopped = Promise.resolve().then(async () => {
      try {
        await recorder?.stop();
        await telemetryStudyStop();
      } catch {
        // A failed cleanup may leave safe overdisclosure. It never reopens the fence.
        throw new Error("Study cleanup unavailable");
      }
    });
    return stopped;
  };
  const abort = () => {
    void stop().catch(() => {});
  };
  const fail = () => {
    if (failed || stopped) return;
    failed = true;
    void stop().catch(() => {});
    onFailure();
  };
  try {
    const config = await telemetryStudyConfig();
    if (!config || signal.aborted) throw new Error("Study unavailable or cancelled");
    signal.addEventListener("abort", abort, { once: true });
    transport = installStudyTransport(config.host, fail);
    // The SDK caches fetch at module evaluation; install the consent fence first.
    const module = await import("./recorder");
    if (signal.aborted || stopped) throw new Error("Study unavailable or cancelled");
    recorder = module.createStudyRecorder(config, fail);
    recorder.start();
    if (signal.aborted || stopped) throw new Error("Study unavailable or cancelled");
    await new Promise<void>((resolve, reject) => {
      const timer = setInterval(() => {
        if (recorder?.recording()) {
          clearInterval(timer);
          clearTimeout(deadline);
          cancelReadiness = undefined;
          resolve();
        }
      }, 100);
      const deadline = setTimeout(() => {
        clearInterval(timer);
        cancelReadiness = undefined;
        reject(new Error("Replay did not start"));
      }, 10_000);
      cancelReadiness = () => {
        clearInterval(timer);
        clearTimeout(deadline);
        reject(new Error("Study cancelled"));
      };
    });
    if (signal.aborted || stopped) throw new Error("Study unavailable or cancelled");
    recorder.ready();
    if (signal.aborted || stopped) throw new Error("Study unavailable or cancelled");
    return { stop };
  } catch {
    try {
      await stop();
    } catch {
      // Do not let SDK/IPC cleanup errors replace the redacted failure below.
    }
    throw new Error("Study unavailable or cancelled");
  }
}
