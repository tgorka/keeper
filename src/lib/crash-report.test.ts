import { afterEach, describe, expect, it, vi } from "vitest";

vi.mock("@/lib/ipc/client", () => ({
  frontendErrorReport: vi.fn(async () => undefined),
}));

import { reportFrontendError } from "@/lib/crash-report";
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
