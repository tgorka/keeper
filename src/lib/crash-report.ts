import { describeFailure, FAILURE_TRANSPORT_MAX } from "@/lib/failure-envelope";
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
/**
 * The same, for `console.error` / `console.warn` lines (102.1 #3): their own
 * budget, so a chatty library cannot spend the one real errors need.
 */
const MAX_CONSOLE_REPORTS_PER_LOAD = 50;
const reported = new Set<string>();
const consoleReported = new Set<string>();

/**
 * The devtools console as it was before {@link hookConsole}: where a refused
 * report is written, so the refusal can never loop back into the funnel.
 */
const unhookedConsoleError: (...args: unknown[]) => void = console.error.bind(console);

/**
 * Write an error the webview could not handle to the app log (`keeper.log`) —
 * and, under diagnostics consent, to PostHog with its message redacted by
 * Rust (R461).
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
  const { classification } = describeFailure(error);
  const key = `${source}\u0000${classification}`;
  if (reported.has(key) || reported.size >= MAX_REPORTS_PER_LOAD) {
    return;
  }
  reported.add(key);
  const stack = error instanceof Error ? (error.stack ?? null) : null;
  // Uncut, so Rust redacts a secret whole before anything caps it (R495 04).
  send(source, describeFailure(error, FAILURE_TRANSPORT_MAX).message, stack, componentStack);
}

function send(
  source: string,
  message: string,
  stack: string | null,
  componentStack: string | null,
): void {
  // The log itself refused the line, so the devtools console is the only
  // witness left; never a second report, which would loop through here.
  const refused = (failure: unknown) => {
    unhookedConsoleError("crash-report: keeper.log refused a report", source, message, failure);
  };
  try {
    void frontendErrorReport(source, message, stack, componentStack).catch(refused);
  } catch (failure) {
    // A wrapper that throws before it returns a promise is refused the same
    // way: this runs inside other catch handlers, so it must not throw.
    refused(failure);
  }
}

/** One console line as text: strings as written, anything else as the funnel describes it. */
function consoleText(args: unknown[]): string {
  return args
    .map((arg) =>
      typeof arg === "string" ? arg : describeFailure(arg, FAILURE_TRANSPORT_MAX).message,
    )
    .join(" ");
}

/**
 * Forward every `console.error` and `console.warn` to the funnel (102.1 #3),
 * once per distinct line and source per load, after the console has printed
 * it. Returns the function that puts the console back.
 */
export function hookConsole(target: Pick<Console, "error" | "warn"> = console): () => void {
  const original = { error: target.error, warn: target.warn };
  let forwarding = false;
  const hook =
    (level: "error" | "warn") =>
    (...args: unknown[]): void => {
      original[level].apply(target, args);
      if (forwarding) return;
      forwarding = true;
      try {
        const source = `console.${level}`;
        // Uncut, so Rust redacts a secret whole before anything caps it
        // (R495 04); a line past the transport bound loses its cut token.
        const message = describeFailure(consoleText(args), FAILURE_TRANSPORT_MAX).message;
        const key = `${source}\u0000${message}`;
        if (consoleReported.has(key) || consoleReported.size >= MAX_CONSOLE_REPORTS_PER_LOAD) {
          return;
        }
        consoleReported.add(key);
        send(source, message, null, null);
      } finally {
        forwarding = false;
      }
    };
  target.error = hook("error");
  target.warn = hook("warn");
  return () => {
    target.error = original.error;
    target.warn = original.warn;
  };
}

/**
 * Report what escapes every handler: uncaught exceptions and rejected
 * promises nobody awaited, and every console error or warning. Installed
 * once, by the app entry point.
 */
export function installGlobalErrorReporting(): void {
  window.addEventListener("error", (event) => {
    reportFrontendError("window.error", event.error ?? event.message, null);
  });
  window.addEventListener("unhandledrejection", (event) => {
    reportFrontendError("unhandledrejection", event.reason, null);
  });
  hookConsole();
}
