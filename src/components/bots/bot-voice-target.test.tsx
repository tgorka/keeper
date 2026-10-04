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
 * 6. **The assistant's conversations** (AD-384) — listed after the bots, the
 *    DM first, one group per account when more than one has any and no group
 *    with none; choosing one stores its `agent:<room id>`, a stored one reads
 *    back as chosen, and one not listed yet stays chosen until the rooms
 *    arrive, never rewritten.
 */
import { act, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import {
  BotVoiceTarget,
  VOICE_TARGET_AGENT_GROUP,
  VOICE_TARGET_LABEL,
  VOICE_TARGET_RECENT_LABEL,
  VOICE_TARGET_UNLISTED_LABEL,
  voiceTargetOptionLabel,
} from "@/components/bots/bot-voice-target";
import type {
  AccountVm,
  BotVm,
  VoiceAgentTargetVm,
  VoiceTargetSpeedVm,
  VoiceWakeVm,
} from "@/lib/ipc/client";
import { accountsStore } from "@/lib/stores/accounts";
import { voiceStore } from "@/lib/stores/voice";

const botsBotsList = vi.fn<() => Promise<BotVm[]>>();
const voiceTargetSet = vi.fn<(target: string | null) => Promise<VoiceWakeVm>>();
const voiceTargetSpeeds = vi.fn<() => Promise<VoiceTargetSpeedVm[]>>();
const voiceAgentTargets = vi.fn<() => Promise<VoiceAgentTargetVm[]>>();
vi.mock("@/lib/ipc/client", async (importOriginal) => {
  const actual = await importOriginal<typeof import("@/lib/ipc/client")>();
  return {
    ...actual,
    botsBotsList: () => botsBotsList(),
    voiceTargetSet: (target: string | null) => voiceTargetSet(target),
    voiceTargetSpeeds: () => voiceTargetSpeeds(),
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
  botsBotsList.mockReset();
  voiceTargetSet.mockReset();
  voiceTargetSpeeds.mockReset();
  voiceAgentTargets.mockReset();
  botsBotsList.mockResolvedValue(BOTS);
  voiceTargetSpeeds.mockResolvedValue([]);
  voiceAgentTargets.mockResolvedValue([]);
  voiceTargetSet.mockImplementation((target) => Promise.resolve({ ...WAKE, voiceTarget: target }));
  voiceStore.setState({ state: null, unavailable: null, wake: WAKE });
  accountsStore.setState({ accounts: [account("acc-1", "@tg:example.org")] });
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
  it("lists the assistant's conversations after the bots, the DM first, in one group", async () => {
    voiceAgentTargets.mockResolvedValue([DM, READING]);
    render(<BotVoiceTarget />);
    await screen.findByRole("option", { name: "Nixi — reading list" });
    expect(optionTexts()).toEqual([
      VOICE_TARGET_RECENT_LABEL,
      "Archivist",
      "Butler",
      "Nixi",
      "Nixi — reading list",
    ]);
    const group = screen.getByRole("group", { name: VOICE_TARGET_AGENT_GROUP });
    expect(
      within(group)
        .getAllByRole("option")
        .map((option) => option.textContent),
    ).toEqual(["Nixi", "Nixi — reading list"]);
  });

  it("groups the conversations by account when two accounts have an assistant", async () => {
    accountsStore.setState({
      accounts: [account("acc-1", "@tg:example.org"), account("acc-2", "@tg:work.org")],
    });
    const workDm = room("acc-2", "!work-dm:work.org", "Nixi", "main");
    voiceAgentTargets.mockResolvedValue([DM, READING, workDm]);
    render(<BotVoiceTarget />);
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

  it("stores a chosen conversation as agent:<room id> and shows it chosen", async () => {
    voiceAgentTargets.mockResolvedValue([DM, READING]);
    render(<BotVoiceTarget />);
    const control = await screen.findByRole("combobox", { name: VOICE_TARGET_LABEL });
    await screen.findByRole("option", { name: "Nixi — reading list" });
    fireEvent.change(control, { target: { value: READING.target } });
    await waitFor(() => expect(voiceTargetSet).toHaveBeenCalledWith("agent:!reading:example.org"));
    await waitFor(() => expect(control).toHaveValue(READING.target));
    expect(voiceStore.getState().wake?.voiceTarget).toBe(READING.target);
  });

  it("still stores a bot by its id beside the conversations", async () => {
    voiceAgentTargets.mockResolvedValue([DM]);
    render(<BotVoiceTarget />);
    const control = await screen.findByRole("combobox", { name: VOICE_TARGET_LABEL });
    await screen.findByRole("option", { name: "Nixi" });
    fireEvent.change(control, { target: { value: "a" } });
    await waitFor(() => expect(voiceTargetSet).toHaveBeenCalledWith("a"));
    await waitFor(() => expect(control).toHaveValue("a"));
  });

  it("reads a stored conversation back as that row chosen", async () => {
    voiceStore.setState({ wake: { ...WAKE, voiceTarget: DM.target } });
    voiceAgentTargets.mockResolvedValue([DM, READING]);
    render(<BotVoiceTarget />);
    await screen.findByRole("option", { name: "Nixi — reading list" });
    expect(screen.getByRole("combobox", { name: VOICE_TARGET_LABEL })).toHaveValue(DM.target);
    expect(screen.getByRole("option", { name: "Nixi" })).toHaveProperty("selected", true);
    expect(screen.queryByRole("option", { name: VOICE_TARGET_UNLISTED_LABEL })).toBeNull();
  });

  it("keeps a stored conversation chosen until the rooms arrive late, never rewriting it", async () => {
    vi.useFakeTimers({ shouldAdvanceTime: true });
    voiceStore.setState({ wake: { ...WAKE, voiceTarget: READING.target } });
    // No pinned bot either: the stored conversation alone keeps the control.
    botsBotsList.mockResolvedValue([]);
    render(<BotVoiceTarget />);
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

  it("offers the bots alone, with no empty group, when the assistant has no conversation", async () => {
    render(<BotVoiceTarget />);
    await screen.findByRole("combobox", { name: VOICE_TARGET_LABEL });
    await waitFor(() => expect(voiceAgentTargets).toHaveBeenCalled());
    expect(optionTexts()).toEqual([VOICE_TARGET_RECENT_LABEL, "Archivist", "Butler"]);
    expect(screen.queryByRole("group")).toBeNull();
  });
});
