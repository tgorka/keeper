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
 */
import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import {
  BotVoiceTarget,
  VOICE_TARGET_LABEL,
  VOICE_TARGET_RECENT_LABEL,
} from "@/components/bots/bot-voice-target";
import type { BotVm, VoiceWakeVm } from "@/lib/ipc/client";
import { voiceStore } from "@/lib/stores/voice";

const botsBotsList = vi.fn<() => Promise<BotVm[]>>();
const voiceTargetSet = vi.fn<(botId: string | null) => Promise<VoiceWakeVm>>();
vi.mock("@/lib/ipc/client", async (importOriginal) => {
  const actual = await importOriginal<typeof import("@/lib/ipc/client")>();
  return {
    ...actual,
    botsBotsList: () => botsBotsList(),
    voiceTargetSet: (botId: string | null) => voiceTargetSet(botId),
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

beforeEach(() => {
  botsBotsList.mockReset();
  voiceTargetSet.mockReset();
  botsBotsList.mockResolvedValue(BOTS);
  voiceStore.setState({ state: null, unavailable: null, wake: WAKE });
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
    voiceTargetSet.mockImplementation((botId) => Promise.resolve({ ...WAKE, voiceTarget: botId }));
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

  it("is absent with no pinned bot, and before the wake facts are read", async () => {
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
});
