/**
 * Who a spoken turn talks to (Epic 67, Story 67.1, AD-206; AD-384).
 *
 * What is asserted here that nothing else asserts:
 *
 * 1. **The assistant's conversations** — the DM first, one group per account
 *    when more than one has any; unset reads "Nobody yet".
 * 2. **A choice is written and read back** — `voice_target_set` is called with
 *    the conversation's `agent:<room id>` (`null` for unset) and the control
 *    shows what Rust stored, never what was clicked.
 * 3. **A value naming no conversation reads as unset** — a pinned bot's id
 *    left from before Story 108.3a shows as "Nobody yet", the way Rust treats
 *    it at send time.
 * 4. **Absent with nothing to choose** — no conversation and none chosen, no
 *    control (AD-27); a failed write shows the sentence.
 * 5. **A stored conversation not listed yet** stays chosen until the rooms
 *    arrive, never rewritten.
 */
import { act, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import {
  VOICE_TARGET_AGENT_GROUP,
  VOICE_TARGET_LABEL,
  VOICE_TARGET_UNLISTED_LABEL,
  VOICE_TARGET_UNSET_LABEL,
  VoiceTarget,
} from "@/components/voice/voice-target";
import type { AccountVm, VoiceAgentTargetVm, VoiceWakeVm } from "@/lib/ipc/client";
import { accountsStore } from "@/lib/stores/accounts";
import { voiceStore } from "@/lib/stores/voice";

const voiceTargetSet = vi.fn<(target: string | null) => Promise<VoiceWakeVm>>();
const voiceAgentTargets = vi.fn<() => Promise<VoiceAgentTargetVm[]>>();
vi.mock("@/lib/ipc/client", async (importOriginal) => {
  const actual = await importOriginal<typeof import("@/lib/ipc/client")>();
  return {
    ...actual,
    voiceTargetSet: (target: string | null) => voiceTargetSet(target),
    voiceAgentTargets: () => voiceAgentTargets(),
  };
});

const WAKE: VoiceWakeVm = {
  enabled: true,
  phrase: "nixie",
  limits: "limits",
  locale: "en-US",
  localeChosen: null,
  onDeviceLocales: ["en-US"],
  stopPhrase: "stop",
  voiceTarget: null,
};

function account(accountId: string, userId: string): AccountVm {
  return {
    accountId,
    userId,
    homeserverUrl: "https://example.org",
    hueIndex: 0,
    provider: "password",
  };
}

function room(
  accountId: string,
  roomId: string,
  name: string,
  kind: "main" | "conversation",
): VoiceAgentTargetVm {
  return { target: `agent:${roomId}`, accountId, roomId, name, kind };
}

const DM = room("acc-1", "!dm:example.org", "Nixi", "main");
const READING = room("acc-1", "!reading:example.org", "Nixi — reading list", "conversation");

/** Every option's words, in the order the select offers them. */
function optionTexts(): (string | null)[] {
  return screen.getAllByRole("option").map((option) => option.textContent);
}

beforeEach(() => {
  voiceTargetSet.mockReset();
  voiceAgentTargets.mockReset();
  voiceAgentTargets.mockResolvedValue([DM, READING]);
  voiceTargetSet.mockImplementation((target) => Promise.resolve({ ...WAKE, voiceTarget: target }));
  voiceStore.setState({ state: null, unavailable: null, wake: WAKE });
  accountsStore.setState({ accounts: [account("acc-1", "@tg:example.org")] });
});

afterEach(() => {
  vi.useRealTimers();
});

describe("VoiceTarget", () => {
  it("lists the assistant's conversations, the DM first, in one group, resting on unset", async () => {
    render(<VoiceTarget />);
    const control = await screen.findByRole("combobox", { name: VOICE_TARGET_LABEL });
    await screen.findByRole("option", { name: "Nixi — reading list" });
    expect(control).toHaveValue("");
    expect(optionTexts()).toEqual([VOICE_TARGET_UNSET_LABEL, "Nixi", "Nixi — reading list"]);
    const group = screen.getByRole("group", { name: VOICE_TARGET_AGENT_GROUP });
    expect(
      within(group)
        .getAllByRole("option")
        .map((option) => option.textContent),
    ).toEqual(["Nixi", "Nixi — reading list"]);
  });

  it("stores a chosen conversation as agent:<room id>, shows what Rust stored, and unsets with null", async () => {
    render(<VoiceTarget />);
    const control = await screen.findByRole("combobox", { name: VOICE_TARGET_LABEL });
    await screen.findByRole("option", { name: "Nixi — reading list" });
    fireEvent.change(control, { target: { value: READING.target } });
    await waitFor(() => expect(voiceTargetSet).toHaveBeenCalledWith("agent:!reading:example.org"));
    await waitFor(() => expect(control).toHaveValue(READING.target));
    expect(voiceStore.getState().wake?.voiceTarget).toBe(READING.target);

    // Back to unset is `null`, not the empty string.
    fireEvent.change(control, { target: { value: "" } });
    await waitFor(() => expect(voiceTargetSet).toHaveBeenLastCalledWith(null));
    await waitFor(() => expect(control).toHaveValue(""));
  });

  it("shows a value naming no conversation — a bot id from before — as unset", async () => {
    voiceStore.setState({ wake: { ...WAKE, voiceTarget: "01BOTID" } });
    render(<VoiceTarget />);
    const control = await screen.findByRole("combobox", { name: VOICE_TARGET_LABEL });
    await screen.findByRole("option", { name: "Nixi" });
    expect(control).toHaveValue("");
    expect(screen.queryByRole("option", { name: VOICE_TARGET_UNLISTED_LABEL })).toBeNull();
  });

  it("is absent with no conversation and none chosen, and before the wake facts are read", async () => {
    voiceAgentTargets.mockResolvedValue([]);
    const { unmount } = render(<VoiceTarget />);
    await waitFor(() => expect(voiceAgentTargets).toHaveBeenCalledTimes(1));
    expect(screen.queryByRole("combobox")).toBeNull();
    unmount();

    voiceAgentTargets.mockResolvedValue([DM]);
    voiceStore.setState({ wake: null });
    render(<VoiceTarget />);
    await waitFor(() => expect(voiceAgentTargets).toHaveBeenCalledTimes(2));
    expect(screen.queryByRole("combobox")).toBeNull();
  });

  it("shows a failed write as its sentence and keeps the stored value", async () => {
    voiceTargetSet.mockReset();
    voiceTargetSet.mockRejectedValue({
      code: "internal",
      message: "the settings table is read-only",
      accountId: null,
      retriable: false,
    });
    render(<VoiceTarget />);
    const control = await screen.findByRole("combobox", { name: VOICE_TARGET_LABEL });
    await screen.findByRole("option", { name: "Nixi" });
    fireEvent.change(control, { target: { value: DM.target } });
    await screen.findByRole("alert");
    expect(screen.getByRole("alert")).toHaveTextContent("the settings table is read-only");
    expect(control).toHaveValue("");
  });

  it("groups the conversations by account when two accounts have an assistant", async () => {
    accountsStore.setState({
      accounts: [account("acc-1", "@tg:example.org"), account("acc-2", "@tg:work.org")],
    });
    const workDm = room("acc-2", "!work-dm:work.org", "Nixi", "main");
    voiceAgentTargets.mockResolvedValue([DM, READING, workDm]);
    render(<VoiceTarget />);
    const work = await screen.findByRole("group", {
      name: `${VOICE_TARGET_AGENT_GROUP} · @tg:work.org`,
    });
    expect(
      within(work)
        .getAllByRole<HTMLOptionElement>("option")
        .map((option) => option.value),
    ).toEqual([workDm.target]);
    const home = screen.getByRole("group", {
      name: `${VOICE_TARGET_AGENT_GROUP} · @tg:example.org`,
    });
    expect(
      within(home)
        .getAllByRole<HTMLOptionElement>("option")
        .map((option) => option.value),
    ).toEqual([DM.target, READING.target]);
  });

  it("reads a stored conversation back as that row chosen", async () => {
    voiceStore.setState({ wake: { ...WAKE, voiceTarget: DM.target } });
    voiceAgentTargets.mockResolvedValue([DM, READING]);
    render(<VoiceTarget />);
    await screen.findByRole("option", { name: "Nixi — reading list" });
    expect(screen.getByRole("combobox", { name: VOICE_TARGET_LABEL })).toHaveValue(DM.target);
    expect(screen.getByRole("option", { name: "Nixi" })).toHaveProperty("selected", true);
    expect(screen.queryByRole("option", { name: VOICE_TARGET_UNLISTED_LABEL })).toBeNull();
  });

  it("keeps a stored conversation chosen until the rooms arrive late, never rewriting it", async () => {
    vi.useFakeTimers({ shouldAdvanceTime: true });
    voiceStore.setState({ wake: { ...WAKE, voiceTarget: READING.target } });
    // No conversation listed yet: the stored one alone keeps the control.
    voiceAgentTargets.mockResolvedValue([]);
    render(<VoiceTarget />);
    const control = await screen.findByRole("combobox", { name: VOICE_TARGET_LABEL });
    expect(control).toHaveValue(READING.target);
    expect(screen.getByRole("option", { name: VOICE_TARGET_UNLISTED_LABEL })).toHaveProperty(
      "selected",
      true,
    );

    voiceAgentTargets.mockResolvedValue([DM, READING]);
    await act(() => vi.advanceTimersByTimeAsync(5_000));
    await screen.findByRole("option", { name: "Nixi — reading list" });
    expect(control).toHaveValue(READING.target);
    expect(screen.getByRole("option", { name: "Nixi — reading list" })).toHaveProperty(
      "selected",
      true,
    );
    expect(screen.queryByRole("option", { name: VOICE_TARGET_UNLISTED_LABEL })).toBeNull();
    expect(voiceTargetSet).not.toHaveBeenCalled();
  });
});
