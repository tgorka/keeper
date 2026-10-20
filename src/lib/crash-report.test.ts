import { afterEach, describe, expect, it, vi } from "vitest";

vi.mock("@/lib/ipc/client", () => ({
  frontendErrorReport: vi.fn(async () => undefined),
}));

import { hookConsole, reportFrontendError } from "@/lib/crash-report";
import { frontendErrorReport } from "@/lib/ipc/client";

afterEach(() => {
  vi.clearAllMocks();
});

/** What keeper's commands reject with: the `IpcError` envelope, not an `Error`. */
function envelope(code: string, message: string) {
  return { code, message, accountId: "acct-1", retriable: true };
}

describe("the front funnel reads keeper's IPC envelope (R387, FIX 01)", () => {
  it("logs two different envelopes from two sources by code, and a true duplicate once", () => {
    reportFrontendError(
      "chat.archive",
      envelope("internal", "M_FORBIDDEN from https://matrix.example/_matrix?access_token=syt_x"),
      null,
    );
    reportFrontendError(
      "account.setupResolve",
      envelope("oauthTimedOut", "sign-in\ntimed out"),
      null,
    );
    // The same code from another operation is another failure.
    reportFrontendError("rooms.pin", envelope("internal", "M_UNKNOWN"), null);
    // The same failure from the same operation again is not.
    reportFrontendError(
      "chat.archive",
      envelope("internal", "a different provider sentence for the same failure"),
      null,
    );

    expect(frontendErrorReport).toHaveBeenCalledTimes(3);
    expect(frontendErrorReport).toHaveBeenNthCalledWith(
      1,
      "chat.archive",
      "IpcError internal (retriable)",
      null,
      null,
    );
    expect(frontendErrorReport).toHaveBeenNthCalledWith(
      2,
      "account.setupResolve",
      "IpcError oauthTimedOut (retriable): sign-in timed out",
      null,
      null,
    );
    expect(frontendErrorReport).toHaveBeenNthCalledWith(
      3,
      "rooms.pin",
      "IpcError internal (retriable)",
      null,
      null,
    );
  });
});

describe("console.error and console.warn reach the funnel (102.1 #3)", () => {
  it("forwards each distinct line once per load, after the console printed it", () => {
    const printed: string[] = [];
    const target = {
      error: (...args: unknown[]) => printed.push(`error ${args.join(" ")}`),
      warn: (...args: unknown[]) => printed.push(`warn ${args.join(" ")}`),
    };
    const restore = hookConsole(target);
    target.error("render failed for", new Error("boom"));
    target.error("render failed for", new Error("boom"));
    target.warn("deprecated prop");
    restore();
    target.warn("after restore");

    expect(printed).toHaveLength(4);
    expect(frontendErrorReport).toHaveBeenCalledTimes(2);
    expect(frontendErrorReport).toHaveBeenNthCalledWith(
      1,
      "console.error",
      "render failed for Error: boom",
      null,
      null,
    );
    expect(frontendErrorReport).toHaveBeenNthCalledWith(
      2,
      "console.warn",
      "deprecated prop",
      null,
      null,
    );
  });

  it("never forwards a line it writes itself while forwarding", () => {
    const target = {
      error: vi.fn(),
      warn: vi.fn(),
    };
    const restore = hookConsole(target);
    vi.mocked(frontendErrorReport).mockImplementationOnce(() => {
      target.error("logged from inside a report");
      return Promise.resolve();
    });
    target.warn("a first warning");
    restore();

    expect(frontendErrorReport).toHaveBeenCalledTimes(1);
    // Printed, never forwarded.
    expect(target.error).toHaveBeenCalledTimes(1);
  });
});

/**
 * R495 04: what crosses to Rust is never cut inside a secret. The same
 * fixtures are run through Rust's bound, redaction and export in
 * `a_webview_report_is_redacted_before_any_cap` (telemetry/tests.rs).
 */
describe("the export path hands Rust a secret whole (R495 04)", () => {
  const hex = "0123456789abcdef0123456789abcdef";

  it("sends a console line past the log's 200 characters uncut", () => {
    const target = { error: vi.fn(), warn: vi.fn() };
    const restore = hookConsole(target);
    target.error(`${"x ".repeat(95)}${hex}`);
    restore();
    const sent = vi.mocked(frontendErrorReport).mock.calls[0]?.[1] ?? "";
    expect(sent).toContain(hex);
  });

  it("sends an Error past 200 characters uncut", () => {
    reportFrontendError("window.error", new Error(`${"y ".repeat(95)}token ${hex}`), null);
    const sent = vi.mocked(frontendErrorReport).mock.calls[0]?.[1] ?? "";
    expect(sent).toContain(hex);
  });

  it("drops the token a transport cut went through, never a fragment of it", () => {
    const target = { error: vi.fn(), warn: vi.fn() };
    const restore = hookConsole(target);
    // The cut at 8,000 characters lands inside the Matrix id.
    target.warn(`${"z ".repeat(3_995)}@alice:example.org`);
    restore();
    const sent = vi.mocked(frontendErrorReport).mock.calls[0]?.[1] ?? "";
    expect(sent.length).toBeLessThanOrEqual(8_001);
    expect(sent).not.toContain("@ali");
    expect(sent.endsWith("…")).toBe(true);
  });
});
