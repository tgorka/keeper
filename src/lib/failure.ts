/**
 * The two ways a failed call ends in the webview (Epic 100, R381): an error
 * reaches a person and the log, never only one of them.
 *
 * - {@link sayFailure}: a failure the person can feel — the room was not
 *   archived, the draft was not saved — is a toast in the words Rust gave
 *   (or the fallback) **and** a `keeper.log` line through the one front
 *   funnel, {@link reportFrontendError}.
 * - {@link logFailure}: a failure nobody can feel — a best-effort cleanup, a
 *   refresh whose last answer still stands, a cancel whose surface is gone —
 *   is a `keeper.log` line only.
 *
 * An empty `catch` around a call is neither, and `src/test/no-silent-catch.test.ts`
 * fails on one.
 */
import { toast } from "sonner";
import { reportFrontendError } from "@/lib/crash-report";
import { syncErrorMessage } from "@/lib/stores/sync";

/**
 * A catch handler for a failure the person can feel: a toast saying it and a
 * `keeper.log` line under `source` (a short, fixed name such as
 * `"chat.archive"`, never content).
 */
export function sayFailure(source: string, fallback: string): (raw: unknown) => void {
  return (raw: unknown) => {
    reportFrontendError(source, raw, null);
    toast.error(syncErrorMessage(raw, fallback));
  };
}

/**
 * A catch handler for a failure nobody can feel: a `keeper.log` line under
 * `source`, nothing on screen.
 */
export function logFailure(source: string): (raw: unknown) => void {
  return (raw: unknown) => {
    reportFrontendError(source, raw, null);
  };
}
