/**
 * Who a spoken turn talks to (Epic 67, Story 67.1, AD-206; AD-384; 108.2).
 *
 * What is asserted here that nothing else asserts:
 *
 * 1. **The person's agents** — every agent of every held database, in Rust's
 *    order, in one group; unset reads "Nobody yet".
 * 2. **A choice is written and read back** — `voice_target_set` is called with
 *    the agent's `agent:<agent>` (`null` for unset) and the control shows
 *    what Rust stored, never what was clicked.
 * 3. **A value naming no agent reads as unset** — a pinned bot's id left from
 *    before Story 108.3a shows as "Nobody yet", the way Rust treats it at
 *    send time.
 * 4. **Absent with nothing to choose** — no agent, none chosen and no notice,
 *    no control (AD-27); a failed write shows the sentence.
 * 5. **A stored agent not listed yet** stays chosen until the agents arrive,
 *    never rewritten.
 * 6. **The old room notice** (UX-DR-E108-1) is shown until dismissed.
 */
import { act, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import {
  VOICE_TARGET_AGENT_GROUP,
  VOICE_TARGET_LABEL,
  VOICE_TARGET_NOTICE_DISMISS,
  VOICE_TARGET_UNLISTED_LABEL,
  VOICE_TARGET_UNSET_LABEL,
  VoiceTarget,
} from "@/components/voice/voice-target";
import type * as IpcClient from "@/lib/ipc/client";
import type { VoiceAgentTargetVm, VoiceWakeVm } from "@/lib/ipc/client";
import { voiceStore } from "@/lib/stores/voice";

const voiceTargetSet = vi.fn<(target: string | null) => Promise<VoiceWakeVm>>();
const voiceAgentTargets = vi.fn<() => Promise<VoiceAgentTargetVm[]>>();
const voiceOldTargetNoticeGet = vi.fn<() => Promise<string | null>>();
const voiceOldTargetNoticeDismiss = vi.fn<() => Promise<void>>();
vi.mock("@/lib/ipc/client", async (importOriginal) => {
  const actual = await importOriginal<typeof IpcClient>();
  return {
    ...actual,
    voiceTargetSet: (target: string | null) => voiceTargetSet(target),
    voiceAgentTargets: () => voiceAgentTargets(),
    voiceOldTargetNoticeGet: () => voiceOldTargetNoticeGet(),
    voiceOldTargetNoticeDismiss: () => voiceOldTargetNoticeDismiss(),
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

function agent(actor: string, name: string): VoiceAgentTargetVm {
  return { target: `agent:${actor}`, agent: actor, name };
}

const NIXIE = agent("nixie", "Nixie");
const TOLA = agent("tola-grey", "Dr Tola Grey");

/** Every option's words, in the order the select offers them. */
function optionTexts(): (string | null)[] {
  return screen.getAllByRole("option").map((option) => option.textContent);
}

beforeEach(() => {
  voiceTargetSet.mockReset();
  voiceAgentTargets.mockReset();
  voiceAgentTargets.mockResolvedValue([NIXIE, TOLA]);
  voiceOldTargetNoticeGet.mockReset();
  voiceOldTargetNoticeGet.mockResolvedValue(null);
  voiceOldTargetNoticeDismiss.mockReset();
  voiceOldTargetNoticeDismiss.mockResolvedValue(undefined);
  voiceTargetSet.mockImplementation((target) => Promise.resolve({ ...WAKE, voiceTarget: target }));
  voiceStore.setState({ state: null, unavailable: null, wake: WAKE });
});

afterEach(() => {
  vi.useRealTimers();
});

describe("VoiceTarget", () => {
  it("lists the person's agents in Rust's order, in one group, resting on unset", async () => {
    render(<VoiceTarget />);
    const control = await screen.findByRole("combobox", { name: VOICE_TARGET_LABEL });
    await screen.findByRole("option", { name: "Dr Tola Grey" });
    expect(control).toHaveValue("");
    expect(optionTexts()).toEqual([VOICE_TARGET_UNSET_LABEL, "Nixie", "Dr Tola Grey"]);
    const group = screen.getByRole("group", { name: VOICE_TARGET_AGENT_GROUP });
    expect(
      within(group)
        .getAllByRole("option")
        .map((option) => option.textContent),
    ).toEqual(["Nixie", "Dr Tola Grey"]);
  });

  it("stores a chosen agent as agent:<agent>, shows what Rust stored, and unsets with null", async () => {
    render(<VoiceTarget />);
    const control = await screen.findByRole("combobox", { name: VOICE_TARGET_LABEL });
    await screen.findByRole("option", { name: "Dr Tola Grey" });
    fireEvent.change(control, { target: { value: TOLA.target } });
    await waitFor(() => expect(voiceTargetSet).toHaveBeenCalledWith("agent:tola-grey"));
    await waitFor(() => expect(control).toHaveValue(TOLA.target));
    expect(voiceStore.getState().wake?.voiceTarget).toBe(TOLA.target);

    // Back to unset is `null`, not the empty string.
    fireEvent.change(control, { target: { value: "" } });
    await waitFor(() => expect(voiceTargetSet).toHaveBeenLastCalledWith(null));
    await waitFor(() => expect(control).toHaveValue(""));
  });

  it("shows a value naming no agent — a bot id from before — as unset", async () => {
    voiceStore.setState({ wake: { ...WAKE, voiceTarget: "01BOTID" } });
    render(<VoiceTarget />);
    const control = await screen.findByRole("combobox", { name: VOICE_TARGET_LABEL });
    await screen.findByRole("option", { name: "Nixie" });
    expect(control).toHaveValue("");
    expect(screen.queryByRole("option", { name: VOICE_TARGET_UNLISTED_LABEL })).toBeNull();
  });

  it("is absent with no agent, none chosen and no notice, and before the wake facts are read", async () => {
    voiceAgentTargets.mockResolvedValue([]);
    const { unmount } = render(<VoiceTarget />);
    await waitFor(() => expect(voiceAgentTargets).toHaveBeenCalledTimes(1));
    await waitFor(() => expect(voiceOldTargetNoticeGet).toHaveBeenCalledTimes(1));
    expect(screen.queryByRole("combobox")).toBeNull();
    unmount();

    voiceAgentTargets.mockResolvedValue([NIXIE]);
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
    await screen.findByRole("option", { name: "Nixie" });
    fireEvent.change(control, { target: { value: NIXIE.target } });
    await screen.findByRole("alert");
    expect(screen.getByRole("alert")).toHaveTextContent("the settings table is read-only");
    expect(control).toHaveValue("");
  });

  it("reads a stored agent back as that row chosen", async () => {
    voiceStore.setState({ wake: { ...WAKE, voiceTarget: NIXIE.target } });
    render(<VoiceTarget />);
    await screen.findByRole("option", { name: "Dr Tola Grey" });
    expect(screen.getByRole("combobox", { name: VOICE_TARGET_LABEL })).toHaveValue(NIXIE.target);
    expect(screen.getByRole("option", { name: "Nixie" })).toHaveProperty("selected", true);
    expect(screen.queryByRole("option", { name: VOICE_TARGET_UNLISTED_LABEL })).toBeNull();
  });

  it("keeps a stored agent chosen until the agents arrive late, never rewriting it", async () => {
    vi.useFakeTimers({ shouldAdvanceTime: true });
    voiceStore.setState({ wake: { ...WAKE, voiceTarget: TOLA.target } });
    // No agent listed yet: the stored one alone keeps the control.
    voiceAgentTargets.mockResolvedValue([]);
    render(<VoiceTarget />);
    const control = await screen.findByRole("combobox", { name: VOICE_TARGET_LABEL });
    expect(control).toHaveValue(TOLA.target);
    expect(screen.getByRole("option", { name: VOICE_TARGET_UNLISTED_LABEL })).toHaveProperty(
      "selected",
      true,
    );

    voiceAgentTargets.mockResolvedValue([NIXIE, TOLA]);
    await act(() => vi.advanceTimersByTimeAsync(5_000));
    await screen.findByRole("option", { name: "Dr Tola Grey" });
    expect(control).toHaveValue(TOLA.target);
    expect(screen.getByRole("option", { name: "Dr Tola Grey" })).toHaveProperty("selected", true);
    expect(screen.queryByRole("option", { name: VOICE_TARGET_UNLISTED_LABEL })).toBeNull();
    expect(voiceTargetSet).not.toHaveBeenCalled();
  });

  it("says once that an old room choice was cleared, until it is dismissed", async () => {
    const said = "Speak to named a conversation of your assistant on Matrix.";
    voiceAgentTargets.mockResolvedValue([]);
    voiceOldTargetNoticeGet.mockResolvedValue(said);
    render(<VoiceTarget />);
    expect(await screen.findByRole("status")).toHaveTextContent(said);
    fireEvent.click(screen.getByRole("button", { name: VOICE_TARGET_NOTICE_DISMISS }));
    await waitFor(() => expect(voiceOldTargetNoticeDismiss).toHaveBeenCalledTimes(1));
    expect(screen.queryByText(said)).toBeNull();
  });
});
