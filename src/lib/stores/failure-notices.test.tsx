import { act, render, screen } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { FailureStateVm } from "@/lib/ipc/client";

let send: ((state: FailureStateVm) => void) | null = null;

vi.mock("@/lib/ipc/client", () => ({
  // Rust answers a subscription with the standing set at once, then changes.
  failureSubscribe: vi.fn(async (onState: (state: FailureStateVm) => void) => {
    send = onState;
    onState({
      notices: [
        {
          surface: "push",
          operation: "pushRegistration",
          account: "acct-a",
          sentence: "keeper could not turn on push notifications for one of your accounts.",
        },
      ],
    });
    return "sub-1";
  }),
  failureUnsubscribe: vi.fn(async () => undefined),
  frontendErrorReport: vi.fn(async () => undefined),
}));

vi.mock("sonner", () => ({ toast: { error: vi.fn() } }));

import { toast } from "sonner";
import {
  failureNoticesStore,
  useFailureNotice,
  useFailureNotices,
} from "@/lib/stores/failure-notices";

function Harness() {
  useFailureNotices();
  const voice = useFailureNotice("voice");
  const push = useFailureNotice("push");
  return (
    <>
      <p data-testid="voice">{voice ?? ""}</p>
      <p data-testid="push">{push ?? ""}</p>
    </>
  );
}

beforeEach(() => {
  failureNoticesStore.setState({ notices: [] }, true);
});

afterEach(() => {
  send = null;
  vi.clearAllMocks();
});

describe("the mirror of Rust's background failures (R387)", () => {
  it("shows the snapshot's failure from before it listened, then each change, until Rust clears it", async () => {
    const { unmount } = render(<Harness />);
    await vi.waitFor(() => expect(send).not.toBeNull());

    // A push failure that happened before the webview subscribed.
    expect(screen.getByTestId("push")).toHaveTextContent(
      "keeper could not turn on push notifications for one of your accounts.",
    );
    expect(toast.error).toHaveBeenCalledTimes(1);

    act(() =>
      send?.({
        notices: [
          {
            surface: "push",
            operation: "pushRegistration",
            account: "acct-a",
            sentence: "keeper could not turn on push notifications for one of your accounts.",
          },
          {
            surface: "voice",
            operation: "voiceRearm",
            account: null,
            sentence: "keeper could not start listening for the wake phrase again.",
          },
        ],
      }),
    );
    expect(screen.getByTestId("voice")).toHaveTextContent(
      "keeper could not start listening for the wake phrase again.",
    );
    // The standing push failure is not said a second time.
    expect(toast.error).toHaveBeenCalledTimes(2);

    // The push registration succeeded: Rust's next state no longer holds it.
    act(() =>
      send?.({
        notices: [
          {
            surface: "voice",
            operation: "voiceRearm",
            account: null,
            sentence: "keeper could not start listening for the wake phrase again.",
          },
        ],
      }),
    );
    expect(screen.getByTestId("push")).toHaveTextContent(/^$/);
    expect(screen.getByTestId("voice")).not.toHaveTextContent(/^$/);

    unmount();
  });
});
