/**
 * What a room does with a resolved slash command (Story 100.5): each act runs
 * through its IPC, a success is a quiet sentence, a failure is a sentence and a
 * `keeper.log` line (R381), and `/leave` only asks.
 */
import { beforeEach, describe, expect, it, vi } from "vitest";
import {
  CHAT_TOPIC_FAILED,
  chatCommandHost,
  chatInviteFailed,
  chatTopicChanged,
} from "@/components/chat/chat-commands";

vi.mock("@/lib/crash-report", () => ({ reportFrontendError: vi.fn() }));

import { reportFrontendError } from "@/lib/crash-report";

function host(fail = false) {
  const act = () => (fail ? Promise.reject(new Error("M_FORBIDDEN")) : Promise.resolve());
  const deps = {
    setTopic: vi.fn(act),
    invite: vi.fn(act),
    askLeave: vi.fn(),
  };
  return { deps, run: chatCommandHost(deps) };
}

beforeEach(() => vi.mocked(reportFrontendError).mockClear());

describe("chatCommandHost", () => {
  it("runs each act with its argument and says what happened", async () => {
    const { deps, run } = host();
    expect(await run({ name: "topic", args: "Friday" })).toEqual({
      tone: "done",
      message: chatTopicChanged("Friday"),
    });
    expect(deps.setTopic).toHaveBeenCalledWith("Friday");
    await run({ name: "invite", args: "@bob:example.org" });
    expect(deps.invite).toHaveBeenCalledWith("@bob:example.org");
    expect(reportFrontendError).not.toHaveBeenCalled();
  });

  it("turns a refused act into a failure sentence and a log line", async () => {
    const { run } = host(true);
    expect(await run({ name: "topic", args: "Friday" })).toEqual({
      tone: "failed",
      message: CHAT_TOPIC_FAILED,
    });
    expect(await run({ name: "invite", args: "@bob:example.org" })).toEqual({
      tone: "failed",
      message: chatInviteFailed("@bob:example.org"),
    });
    expect(reportFrontendError).toHaveBeenCalledTimes(2);
  });

  it("only asks before leaving", async () => {
    const { deps, run } = host();
    expect(await run({ name: "leave", args: null })).toBeNull();
    expect(deps.askLeave).toHaveBeenCalledTimes(1);
  });

  it("says so, rather than doing nothing, for a name it has no act for", async () => {
    const { run } = host();
    const outcome = await run({ name: "frobnicate", args: "opus" });
    expect(outcome?.tone).toBe("failed");
    expect(reportFrontendError).toHaveBeenCalledTimes(1);
  });
});
