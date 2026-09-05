/**
 * Listening as one action (Epic 68, Story 68.4, AD-218).
 *
 * What is asserted here that nothing else asserts:
 *
 * 1. **The palette's verb is the shared verb** — `bots-toggle-listening`
 *    dispatches `toggleVoiceListening`, which calls `voice_wake_toggle`
 *    without reading the store first (the palette can open with no pane
 *    mounted), mirrors what Rust stored and re-reads availability.
 * 2. **The control's words are the state** — "Listening off" /
 *    "Listening on · hey nixie" — and it flips through the same verb.
 * 3. **Absent, never disabled** (AD-27) — under `BotVoiceWake`'s rule:
 *    no bots capability, an unanswered probe, an `unsupported` answer, or
 *    wake facts not yet read.
 */
import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import {
  BotListeningToggle,
  LISTENING_TOGGLE_LABEL,
  listeningLabel,
  toggleVoiceListening,
} from "@/components/bots/bot-listening-toggle";
import { paletteActionHandlers } from "@/components/command-palette/actions";
import type { VoiceUnavailableVm, VoiceWakeVm } from "@/lib/ipc/client";
import { capabilitiesStore, DEFAULT_CAPABILITIES } from "@/lib/stores/capabilities";
import { voiceStore } from "@/lib/stores/voice";

const voiceWakeToggle = vi.fn<() => Promise<VoiceWakeVm>>();
const voiceAvailability = vi.fn<() => Promise<VoiceUnavailableVm | null>>();
vi.mock("@/lib/ipc/client", async (importOriginal) => {
  const actual = await importOriginal<typeof import("@/lib/ipc/client")>();
  return {
    ...actual,
    voiceWakeToggle: () => voiceWakeToggle(),
    voiceAvailability: () => voiceAvailability(),
  };
});

const WAKE_OFF: VoiceWakeVm = {
  enabled: false,
  phrase: "hey nixie",
  limits: "limits",
  locale: "en-US",
  localeChosen: null,
  onDeviceLocales: ["en-US"],
  stopPhrase: "stop",
  voiceTarget: null,
};
const WAKE_ON: VoiceWakeVm = { ...WAKE_OFF, enabled: true };
const NOT_AUTHORIZED: VoiceUnavailableVm = {
  kind: "notAuthorized",
  message: "keeper is not allowed to use the microphone — allow it under Settings > keeper",
};

beforeEach(() => {
  voiceWakeToggle.mockReset();
  voiceAvailability.mockReset();
  voiceAvailability.mockResolvedValue(null);
  capabilitiesStore.getState().applySnapshot({ ...DEFAULT_CAPABILITIES, bots: true });
  voiceStore.setState({ state: null, unavailable: null, wake: WAKE_OFF });
});

describe("toggleVoiceListening", () => {
  it("is the verb the command palette dispatches, and it flips through Rust", async () => {
    voiceWakeToggle.mockResolvedValue(WAKE_ON);
    voiceAvailability.mockResolvedValue(NOT_AUTHORIZED);
    const handler = paletteActionHandlers["bots-toggle-listening"];
    expect(handler).toBeTypeOf("function");

    await handler(null);

    expect(voiceWakeToggle).toHaveBeenCalledTimes(1);
    // What Rust stored, and the port's answer on switching on, are both
    // mirrored — the refusal is shown beside the switch, whichever surface
    // flipped it.
    expect(voiceStore.getState().wake).toEqual(WAKE_ON);
    expect(voiceStore.getState().unavailable).toEqual(NOT_AUTHORIZED);

    // A toggle, not a set: Rust flips it back.
    voiceWakeToggle.mockResolvedValue(WAKE_OFF);
    voiceAvailability.mockResolvedValue(null);
    await toggleVoiceListening();
    expect(voiceStore.getState().wake).toEqual(WAKE_OFF);
    expect(voiceStore.getState().unavailable).toBeNull();
  });

  it("leaves the mirror where Rust left it when the flip is refused", async () => {
    voiceWakeToggle.mockRejectedValue(new Error("the settings table is read-only"));
    await expect(toggleVoiceListening()).rejects.toThrow("read-only");
    expect(voiceStore.getState().wake).toEqual(WAKE_OFF);
    expect(voiceAvailability).not.toHaveBeenCalled();
  });
});

describe("BotListeningToggle", () => {
  it("says the state and flips it", async () => {
    voiceWakeToggle.mockResolvedValue(WAKE_ON);
    render(<BotListeningToggle />);
    const button = screen.getByRole("button", { name: LISTENING_TOGGLE_LABEL });
    expect(button).toHaveTextContent("Listening off");
    expect(button).toHaveAttribute("aria-pressed", "false");

    fireEvent.click(button);
    await waitFor(() => expect(button).toHaveTextContent("Listening on · hey nixie"));
    expect(button).toHaveAttribute("aria-pressed", "true");
  });

  it("draws the phone's 44 pt height when asked", () => {
    render(<BotListeningToggle phone />);
    expect(screen.getByRole("button", { name: LISTENING_TOGGLE_LABEL })).toHaveClass("h-11");
  });

  it("is absent under the wake band's rule", () => {
    voiceStore.setState({ unavailable: { kind: "unsupported", message: "no port" } });
    const { unmount } = render(<BotListeningToggle />);
    expect(screen.queryByRole("button")).toBeNull();
    unmount();

    voiceStore.setState({ unavailable: undefined });
    const unanswered = render(<BotListeningToggle />);
    expect(screen.queryByRole("button")).toBeNull();
    unanswered.unmount();

    voiceStore.setState({ unavailable: null, wake: null });
    const unread = render(<BotListeningToggle />);
    expect(screen.queryByRole("button")).toBeNull();
    unread.unmount();

    voiceStore.setState({ wake: WAKE_OFF });
    capabilitiesStore.getState().applySnapshot({ ...DEFAULT_CAPABILITIES, bots: false });
    render(<BotListeningToggle />);
    expect(screen.queryByRole("button")).toBeNull();
  });

  it("stays, with its sentence beside the switch elsewhere, under every other refusal", () => {
    voiceStore.setState({ unavailable: NOT_AUTHORIZED, wake: WAKE_ON });
    render(<BotListeningToggle />);
    expect(screen.getByRole("button", { name: LISTENING_TOGGLE_LABEL })).toHaveTextContent(
      listeningLabel(WAKE_ON),
    );
  });
});
