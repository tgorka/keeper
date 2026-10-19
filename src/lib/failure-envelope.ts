/**
 * What a rejected call says in `keeper.log` (Epic 100, R381 as amended by
 * R387). Pure and import-free, so the study entry, which links no app client,
 * reads failures the same way the app's funnel (`@/lib/crash-report`) does.
 *
 * Keeper's commands reject with the `IpcError` envelope — an object with
 * `code`, `message`, `accountId` and `retriable`, not a JavaScript `Error` —
 * so `String(raw)` of one is `[object Object]`: the cause lost, and every
 * later envelope a "duplicate" of the first. An envelope is classified by its
 * closed `code`. Its `message` is logged only for the codes whose message is
 * keeper's own fixed sentence; everywhere else it may carry a homeserver's or
 * identity provider's words, which stay out of the log.
 */

/** IPC codes whose message is keeper's own fixed sentence, never provider text. */
const KEEPER_WORDED: Readonly<Record<string, true>> = {
  slidingSyncUnsupported: true,
  oauthUnsupported: true,
  oauthCancelled: true,
  oauthTimedOut: true,
  backupMalformedKey: true,
  backupIncorrectKey: true,
  backupExists: true,
  recordingSessionLive: true,
};

/** The longest message a failure line carries. */
export const FAILURE_MESSAGE_MAX = 200;

/**
 * The longest message the webview hands Rust for export (R495 04): long
 * enough that a secret is whole when Rust redacts it, and the same bound
 * Rust applies (`FRONTEND_INPUT_CAP`).
 */
export const FAILURE_TRANSPORT_MAX = 8_000;

/** One failure as the log reads it. */
export type FailureLine = {
  /** What two reports of the same failure share: the funnel's de-duplication key. */
  classification: string;
  /** The line written, bounded and free of control characters. */
  message: string;
};

/**
 * `text` without control characters, bounded to `max`. A cut drops the token
 * it went through — and the one it may have ended at — so no fragment of a
 * secret or an identifier is left too short to be recognised as one.
 */
function clean(text: string, max: number): string {
  // biome-ignore lint/suspicious/noControlCharactersInRegex: stripping them is the point.
  const stripped = text.replace(/[\u0000-\u001f\u007f-\u009f]+/g, " ").trim();
  return stripped.length > max ? `${stripped.slice(0, max).replace(/\S*$/, "")}…` : stripped;
}

function isEnvelope(raw: unknown): raw is { code: string; message: string; retriable: boolean } {
  if (typeof raw !== "object" || raw === null) {
    return false;
  }
  const candidate = raw as Record<string, unknown>;
  return (
    typeof candidate.code === "string" &&
    typeof candidate.message === "string" &&
    typeof candidate.retriable === "boolean"
  );
}

/**
 * How `raw`, whatever a catch received, is written to `keeper.log` — bounded
 * to `max`, which the export path raises to {@link FAILURE_TRANSPORT_MAX}.
 */
export function describeFailure(raw: unknown, max: number = FAILURE_MESSAGE_MAX): FailureLine {
  if (raw instanceof Error) {
    const message = clean(`${raw.name}: ${raw.message}`, max);
    return { classification: message, message };
  }
  if (isEnvelope(raw)) {
    const code = clean(raw.code, max);
    const head = `IpcError ${code}${raw.retriable ? " (retriable)" : ""}`;
    return {
      classification: `ipc:${code}`,
      message: KEEPER_WORDED[raw.code] === true ? `${head}: ${clean(raw.message, max)}` : head,
    };
  }
  if (typeof raw === "string") {
    const message = clean(raw, max);
    return { classification: message, message };
  }
  const message = `a non-error ${raw === null ? "null" : typeof raw} was thrown`;
  return { classification: message, message };
}
