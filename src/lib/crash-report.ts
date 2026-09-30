import { frontendErrorReport } from "@/lib/ipc/client";

/**
 * How many distinct errors one page load may write to the app log.
 *
 * A render loop or a failing timer can raise the same error every frame; the
 * log is for reading the first few back off a user's machine, not for a
 * gigabyte of one line. Identical messages are written once.
 */
const MAX_REPORTS_PER_LOAD = 20;
const reported = new Set<string>();

/**
 * Write an error the webview could not handle to the app log (`keeper.log`).
 *
 * Best effort by construction: a failed report is dropped, never thrown,
 * because the caller is already an error path and a second error there would
 * hide the first.
 */
export function reportFrontendError(
  source: string,
  error: unknown,
  componentStack: string | null,
): void {
  const message = error instanceof Error ? `${error.name}: ${error.message}` : String(error);
  if (reported.has(message) || reported.size >= MAX_REPORTS_PER_LOAD) {
    return;
  }
  reported.add(message);
  const stack = error instanceof Error ? (error.stack ?? null) : null;
  void frontendErrorReport(source, message, stack, componentStack).catch(() => {});
}

/**
 * Report what escapes every handler: uncaught exceptions and rejected
 * promises nobody awaited. Installed once, by the app entry point.
 */
export function installGlobalErrorReporting(): void {
  window.addEventListener("error", (event) => {
    reportFrontendError("window.error", event.error ?? event.message, null);
  });
  window.addEventListener("unhandledrejection", (event) => {
    reportFrontendError("unhandledrejection", event.reason, null);
  });
}
