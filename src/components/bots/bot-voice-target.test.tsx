/**
 * Who a spoken turn talks to (Epic 67, Story 67.1, AD-206).
 *
 * What is asserted here that nothing else asserts:
 *
 * 1. **Unset is "most recently talked to"** — the select's resting value, with
 *    every pinned bot as an option in Rust's order.
 * 2. **A choice is written and read back** — `voice_target_set` is called
 *    with the bot id (`null` for unset) and the control shows what Rust
 *    stored, never what was clicked.
 * 3. **A stale choice reads as unset** — a `voiceTarget` naming a bot that is
 *    no longer pinned shows as the unset option, the way Rust treats it.
 * 4. **Absent with nothing to choose** — no pinned bot, no control (AD-27);
 *    a failed write shows the sentence.
 * 5. **The numbers beside the names** (Epic 68, AD-216) — an option reads
 *    "Butler · first word ~25 s" from `voice_target_speeds`'s median, a bot
 *    with no median reads its name alone, and a failed read leaves the
 *    names.
 * 6. **The person's agents** (AD-384, 108.2) — listed after the bots in Rust's
 *    order, one group and no group with none; choosing one stores its
 *    `agent:<agent>`, a stored one reads back as chosen, and one not listed
 *    yet stays chosen until the agents arrive, never rewritten.
 * 7. **The old choice's notice** (UX-DR-E108-1) — Rust's one-time sentence
 *    about a cleared choice of an old agent room is shown until dismissed.
 */
import { act, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import {
  BotVoiceTarget,
  VOICE_TARGET_AGENT_GROUP,
  VOICE_TARGET_LABEL,
  VOICE_TARGET_NOTICE_DISMISS,
  VOICE_TARGET_RECENT_LABEL,
  VOICE_TARGET_UNLISTED_LABEL,
  voiceTargetOptionLabel,
} from "@/components/bots/bot-voice-target";
import type { BotVm, VoiceAgentTargetVm, VoiceTargetSpeedVm, VoiceWakeVm } from "@/lib/ipc/client";
import { voiceStore } from "@/lib/stores/voice";

const botsBotsList = vi.fn<() => Promise<BotVm[]>>();
const voiceTargetSet = vi.fn<(target: string | null) => Promise<VoiceWakeVm>>();
const voiceTargetSpeeds = vi.fn<() => Promise<VoiceTargetSpeedVm[]>>();
const voiceAgentTargets = vi.fn<() => Promise<VoiceAgentTargetVm[]>>();
const voiceOldTargetNoticeGet = vi.fn<() => Promise<string | null>>();
const voiceOldTargetNoticeDismiss = vi.fn<() => Promise<void>>();
vi.mock("@/lib/ipc/client", async (importOriginal) => {
  const actual = await importOriginal<typeof import("@/lib/ipc/client")>();
  return {
    ...actual,
    botsBotsList: () => botsBotsList(),
    voiceTargetSet: (target: string | null) => voiceTargetSet(target),
    voiceTargetSpeeds: () => voiceTargetSpeeds(),
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

function bot(id: string, name: string): BotVm {
  return {
    id,
    providerId: "p1",
    target: id,
    name,
    pinOrder: 0,
    shape: null,
    colour: null,
    mark: null,
    createdMs: 0,
  };
}

const BOTS = [bot("a", "Archivist"), bot("b", "Butler")];

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
  botsBotsList.mockReset();
  voiceTargetSet.mockReset();
  voiceTargetSpeeds.mockReset();
  voiceAgentTargets.mockReset();
  botsBotsList.mockResolvedValue(BOTS);
  voiceTargetSpeeds.mockResolvedValue([]);
  voiceAgentTargets.mockResolvedValue([]);
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

describe("BotVoiceTarget", () => {
  it("rests on most recently talked to, and lists every pinned bot", async () => {
    render(<BotVoiceTarget />);
    const control = await screen.findByRole("combobox", { name: VOICE_TARGET_LABEL });
    expect(control).toHaveValue("");
    const options = screen.getAllByRole("option");
    expect(options.map((option) => option.textContent)).toEqual([
      VOICE_TARGET_RECENT_LABEL,
      "Archivist",
      "Butler",
    ]);
  });

  it("writes a choice and shows what Rust stored", async () => {
    render(<BotVoiceTarget />);
    const control = await screen.findByRole("combobox", { name: VOICE_TARGET_LABEL });
    fireEvent.change(control, { target: { value: "b" } });
    await waitFor(() => expect(voiceTargetSet).toHaveBeenCalledWith("b"));
    await waitFor(() => expect(control).toHaveValue("b"));
    expect(voiceStore.getState().wake?.voiceTarget).toBe("b");

    // Back to unset is `null`, not the empty string.
    fireEvent.change(control, { target: { value: "" } });
    await waitFor(() => expect(voiceTargetSet).toHaveBeenLastCalledWith(null));
    await waitFor(() => expect(control).toHaveValue(""));
  });

  it("shows a choice naming an unpinned bot as unset, the way Rust treats it", async () => {
    voiceStore.setState({ wake: { ...WAKE, voiceTarget: "gone" } });
    render(<BotVoiceTarget />);
    const control = await screen.findByRole("combobox", { name: VOICE_TARGET_LABEL });
    expect(control).toHaveValue("");
  });

  it("is absent with no pinned bot and no conversation, and before the wake facts are read", async () => {
    botsBotsList.mockResolvedValue([]);
    const { unmount } = render(<BotVoiceTarget />);
    await waitFor(() => expect(botsBotsList).toHaveBeenCalledTimes(1));
    expect(screen.queryByRole("combobox")).toBeNull();
    unmount();

    botsBotsList.mockResolvedValue(BOTS);
    voiceStore.setState({ wake: null });
    render(<BotVoiceTarget />);
    await waitFor(() => expect(botsBotsList).toHaveBeenCalledTimes(2));
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
    render(<BotVoiceTarget />);
    const control = await screen.findByRole("combobox", { name: VOICE_TARGET_LABEL });
    fireEvent.change(control, { target: { value: "a" } });
    await screen.findByRole("alert");
    expect(screen.getByRole("alert")).toHaveTextContent("the settings table is read-only");
    expect(control).toHaveValue("");
  });

  it("shows each bot's first-token median beside its name (AD-216)", async () => {
    voiceTargetSpeeds.mockResolvedValue([
      { botId: "a", firstTokenMedianMs: null },
      { botId: "b", firstTokenMedianMs: 25_400 },
    ]);
    render(<BotVoiceTarget />);
    await screen.findByRole("combobox", { name: VOICE_TARGET_LABEL });
    await waitFor(() =>
      expect(screen.getAllByRole("option").map((option) => option.textContent)).toEqual([
        VOICE_TARGET_RECENT_LABEL,
        "Archivist",
        "Butler · first word ~25 s",
      ]),
    );
  });

  it("keeps the names when the speeds could not be read", async () => {
    voiceTargetSpeeds.mockRejectedValue(new Error("no store"));
    render(<BotVoiceTarget />);
    await screen.findByRole("combobox", { name: VOICE_TARGET_LABEL });
    await waitFor(() => expect(voiceTargetSpeeds).toHaveBeenCalledTimes(1));
    expect(screen.getAllByRole("option").map((option) => option.textContent)).toEqual([
      VOICE_TARGET_RECENT_LABEL,
      "Archivist",
      "Butler",
    ]);
  });

  it("rounds the median to whole seconds and never says zero", () => {
    expect(voiceTargetOptionLabel("nixie", 28_550)).toBe("nixie · first word ~29 s");
    expect(voiceTargetOptionLabel("ollama", 1_900)).toBe("ollama · first word ~2 s");
    expect(voiceTargetOptionLabel("ollama", 240)).toBe("ollama · first word ~1 s");
    expect(voiceTargetOptionLabel("new", null)).toBe("new");
  });
  it("lists the person's agents after the bots, in Rust's order, in one group", async () => {
    voiceAgentTargets.mockResolvedValue([NIXIE, TOLA]);
    render(<BotVoiceTarget />);
    await screen.findByRole("option", { name: "Dr Tola Grey" });
    expect(optionTexts()).toEqual([
      VOICE_TARGET_RECENT_LABEL,
      "Archivist",
      "Butler",
      "Nixie",
      "Dr Tola Grey",
    ]);
    const group = screen.getByRole("group", { name: VOICE_TARGET_AGENT_GROUP });
    expect(
      within(group)
        .getAllByRole("option")
        .map((option) => option.textContent),
    ).toEqual(["Nixie", "Dr Tola Grey"]);
  });

  it("stores a chosen agent as agent:<agent> and shows it chosen", async () => {
    voiceAgentTargets.mockResolvedValue([NIXIE, TOLA]);
    render(<BotVoiceTarget />);
    const control = await screen.findByRole("combobox", { name: VOICE_TARGET_LABEL });
    await screen.findByRole("option", { name: "Dr Tola Grey" });
    fireEvent.change(control, { target: { value: TOLA.target } });
    await waitFor(() => expect(voiceTargetSet).toHaveBeenCalledWith("agent:tola-grey"));
    await waitFor(() => expect(control).toHaveValue(TOLA.target));
    expect(voiceStore.getState().wake?.voiceTarget).toBe(TOLA.target);
  });

  it("still stores a bot by its id beside the agents", async () => {
    voiceAgentTargets.mockResolvedValue([NIXIE]);
    render(<BotVoiceTarget />);
    const control = await screen.findByRole("combobox", { name: VOICE_TARGET_LABEL });
    await screen.findByRole("option", { name: "Nixie" });
    fireEvent.change(control, { target: { value: "a" } });
    await waitFor(() => expect(voiceTargetSet).toHaveBeenCalledWith("a"));
    await waitFor(() => expect(control).toHaveValue("a"));
  });

  it("reads a stored agent back as that row chosen", async () => {
    voiceStore.setState({ wake: { ...WAKE, voiceTarget: NIXIE.target } });
    voiceAgentTargets.mockResolvedValue([NIXIE, TOLA]);
    render(<BotVoiceTarget />);
    await screen.findByRole("option", { name: "Dr Tola Grey" });
    expect(screen.getByRole("combobox", { name: VOICE_TARGET_LABEL })).toHaveValue(NIXIE.target);
    expect(screen.getByRole("option", { name: "Nixie" })).toHaveProperty("selected", true);
    expect(screen.queryByRole("option", { name: VOICE_TARGET_UNLISTED_LABEL })).toBeNull();
  });

  it("keeps a stored agent chosen until the agents arrive late, never rewriting it", async () => {
    vi.useFakeTimers({ shouldAdvanceTime: true });
    voiceStore.setState({ wake: { ...WAKE, voiceTarget: TOLA.target } });
    // No pinned bot either: the stored agent alone keeps the control.
    botsBotsList.mockResolvedValue([]);
    render(<BotVoiceTarget />);
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

  it("offers the bots alone, with no empty group, when this device holds no agent", async () => {
    render(<BotVoiceTarget />);
    await screen.findByRole("combobox", { name: VOICE_TARGET_LABEL });
    await waitFor(() => expect(voiceAgentTargets).toHaveBeenCalled());
    expect(optionTexts()).toEqual([VOICE_TARGET_RECENT_LABEL, "Archivist", "Butler"]);
    expect(screen.queryByRole("group")).toBeNull();
  });

  it("says once that an old room choice was cleared, until it is dismissed", async () => {
    const said = "Speak to named a conversation of your assistant on Matrix.";
    voiceOldTargetNoticeGet.mockResolvedValue(said);
    render(<BotVoiceTarget />);
    expect(await screen.findByRole("status")).toHaveTextContent(said);
    fireEvent.click(screen.getByRole("button", { name: VOICE_TARGET_NOTICE_DISMISS }));
    await waitFor(() => expect(voiceOldTargetNoticeDismiss).toHaveBeenCalledTimes(1));
    expect(screen.queryByText(said)).toBeNull();
  });
});
