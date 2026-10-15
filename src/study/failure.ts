import { invoke } from "@tauri-apps/api/core";
import { describeFailure } from "@/lib/failure-envelope";

/**
 * The study entry's `logFailure` (R381): a catch handler that writes a failure
 * to `keeper.log` through `frontend_error_report`, the command
 * `@/lib/crash-report` uses — invoked raw, because this entry imports no app
 * client or store — in the words `describeFailure` gives it, the funnel's
 * own: an IPC envelope by its code, never its provider text.
 */
export function logStudyFailure(source: string): (raw: unknown) => void {
  return (raw: unknown) => {
    const { message } = describeFailure(raw);
    // The log itself refused the line, so the devtools console is the only
    // witness left.
    void invoke("frontend_error_report", {
      source,
      message,
      stack: null,
      componentStack: null,
    }).catch((failure: unknown) => {
      console.error("study: keeper.log refused a report", source, message, failure);
    });
  };
}
