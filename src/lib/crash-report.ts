import { describeFailure } from "@/lib/failure-envelope";
import { frontendErrorReport } from "@/lib/ipc/client";

/**
 * How many distinct errors one page load may write to the app log.
 *
 * A render loop or a failing timer can raise the same error every frame; the
 * log is for reading the first few back off a user's machine, not for a
 * gigabyte of one line. The same failure from the same source — the same
 * `source` and the same classification ({@link describeFailure}: an IPC
 * envelope's code, an `Error`'s name and message) — is written once. A
 * hundred, not twenty (R387): polled reads report their failures too, and
 * twenty distinct lines could be spent on those before a later, different one.
 */
const MAX_REPORTS_PER_LOAD = 100;
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
  const { classification, message } = describeFailure(error);
  const key = `${source}\u0000${classification}`;
  if (reported.has(key) || reported.size >= MAX_REPORTS_PER_LOAD) {
    return;
  }
  reported.add(key);
  const stack = error instanceof Error ? (error.stack ?? null) : null;
  // The log itself refused the line, so the devtools console is the only
  // witness left; never a second report, which would loop through here.
  const refused = (failure: unknown) => {
    console.error("crash-report: keeper.log refused a report", source, message, failure);
  };
  try {
    void frontendErrorReport(source, message, stack, componentStack).catch(refused);
  } catch (failure) {
    // A wrapper that throws before it returns a promise is refused the same
    // way: this runs inside other catch handlers, so it must not throw.
    refused(failure);
  }
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
