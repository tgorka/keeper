/**
 * The wake phrase band (Story 62.5, FR-404–FR-406, AD-27, AD-168, AD-169).
 *
 * What is asserted here that nothing else asserts:
 *
 * 1. **Off until chosen** — a fresh read renders the switch unchecked and the
 *    chip absent. Seed `enabled: true` in the fixture and the default-off test
 *    fails alone.
 * 2. **A refused phrase renders Rust's sentence** — the rejection's `message`
 *    lands in an alert letter for letter, and nothing is written locally.
 * 3. **The listening state is announced** — the chip is a `status` live region
 *    present exactly while the snapshot says the microphone is open.
 * 4. **The limits sentence sits beside the switch** — inside the same
 *    `section`, from `VoiceWakeVm.limits`, and it says every fact.
 * 5. **Absent on `unsupported`, present-with-a-prompt on `notAuthorized`** —
 *    the one is no control at all; the other keeps the switch and shows the
 *    sentence saying what to allow.
 * 6. **The section is absent where `capabilities.bots` is off**, and while the
 *    availability question has not been answered.
 */

import { readFileSync } from "node:fs";
import path from "node:path";
import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import {
  BotVoiceWake,
  WAKE_PHRASE_LABEL,
  WAKE_SAVE_LABEL,
  WAKE_SWITCH_LABEL,
  wakeListeningLabel,
} from "@/components/bots/bot-voice-wake";
import type { VoiceStateVm, VoiceUnavailableVm, VoiceWakeVm } from "@/lib/ipc/client";
import { capabilitiesStore, DEFAULT_CAPABILITIES } from "@/lib/stores/capabilities";
import { voiceStore } from "@/lib/stores/voice";

const voiceWakeSet = vi.fn<(enabled: boolean, phrase: string) => Promise<VoiceWakeVm>>();
const voiceAuthorize = vi.fn<() => Promise<VoiceUnavailableVm | null>>();
vi.mock("@/lib/ipc/client", async (importOriginal) => {
  const actual = await importOriginal<typeof import("@/lib/ipc/client")>();
  return {
    ...actual,
    voiceWakeSet: (enabled: boolean, phrase: string) => voiceWakeSet(enabled, phrase),
    voiceAuthorize: () => voiceAuthorize(),
  };
});

/** `LISTENING_LIMITS` as `keeper-core` holds it, so the fixture is the real sentence. */
function rustLimits(): string {
  const source = readFileSync(
    path.resolve(__dirname, "../../../src-tauri/crates/keeper-core/src/voice/mod.rs"),
    "utf8",
  );
  const match = /pub const LISTENING_LIMITS: &str = "([^"]+)";/.exec(source);
  if (match?.[1] === undefined) {
    throw new Error("LISTENING_LIMITS not found in voice/mod.rs");
  }
  return match[1];
}

const LIMITS = rustLimits();
const OFF: VoiceWakeVm = { enabled: false, phrase: "nixie", limits: LIMITS };
const ON: VoiceWakeVm = { enabled: true, phrase: "nixie", limits: LIMITS };
const IDLE_ARMED: VoiceStateVm = { kind: "idle", wake: "nixie", listeningForWake: true };
const IDLE_RELEASED: VoiceStateVm = { kind: "idle", wake: null, listeningForWake: false };
const NOT_AUTHORIZED: VoiceUnavailableVm = {
  kind: "notAuthorized",
  message: "keeper is not allowed to use the microphone — allow both under Settings > keeper",
};
const UNSUPPORTED: VoiceUnavailableVm = {
  kind: "unsupported",
  message: "voice is not available in this build",
};

function seed(
  overrides: {
    wake?: VoiceWakeVm | null;
    unavailable?: VoiceUnavailableVm | null | undefined;
    state?: VoiceStateVm | null;
    bots?: boolean;
  } = {},
) {
  // `"unavailable" in overrides` rather than a destructuring default: an
  // explicit `undefined` is the "not yet asked" case under test.
  capabilitiesStore
    .getState()
    .applySnapshot({ ...DEFAULT_CAPABILITIES, bots: overrides.bots ?? true });
  voiceStore.setState({
    wake: "wake" in overrides ? (overrides.wake ?? null) : OFF,
    unavailable: "unavailable" in overrides ? overrides.unavailable : null,
    state: "state" in overrides ? (overrides.state ?? null) : IDLE_RELEASED,
  });
}

beforeEach(() => {
  voiceWakeSet.mockReset();
  voiceAuthorize.mockReset();
  voiceAuthorize.mockResolvedValue(null);
  voiceStore.setState({ state: null, unavailable: undefined, wake: null });
  capabilitiesStore.getState().applySnapshot(DEFAULT_CAPABILITIES);
});

describe("BotVoiceWake — where it exists", () => {
  it("is absent where capabilities.bots is off", () => {
    seed({ bots: false });
    const { container } = render(<BotVoiceWake />);
    expect(container).toBeEmptyDOMElement();
  });

  it("is absent while the availability question has not been answered", () => {
    seed({ unavailable: undefined });
    const { container } = render(<BotVoiceWake />);
    expect(container).toBeEmptyDOMElement();
  });

  it("is absent — no control, no sentence — where voice is unsupported", () => {
    seed({ unavailable: UNSUPPORTED });
    const { container } = render(<BotVoiceWake />);
    expect(container).toBeEmptyDOMElement();
    expect(screen.queryByRole("switch")).toBeNull();
  });

  it("is present with a prompt where voice is not authorized: the switch stays and the sentence says what to allow", () => {
    seed({ unavailable: NOT_AUTHORIZED });
    render(<BotVoiceWake />);
    expect(screen.getByRole("switch", { name: WAKE_SWITCH_LABEL })).toBeInTheDocument();
    expect(screen.getByRole("status")).toHaveTextContent(NOT_AUTHORIZED.message);
  });
});

describe("BotVoiceWake — the switch and the phrase", () => {
  it("is off until chosen: a fresh read renders the switch unchecked, the shipped phrase, and no chip", () => {
    seed();
    render(<BotVoiceWake />);
    expect(screen.getByRole("switch", { name: WAKE_SWITCH_LABEL })).not.toBeChecked();
    expect(screen.getByLabelText(WAKE_PHRASE_LABEL, { selector: "input" })).toHaveValue("nixie");
    expect(screen.queryByRole("status")).toBeNull();
  });

  it("turning the switch on asks by name first (FR-408), then writes the switch and the phrase through Rust", async () => {
    seed();
    voiceWakeSet.mockResolvedValue(ON);
    render(<BotVoiceWake />);
    fireEvent.click(screen.getByRole("switch", { name: WAKE_SWITCH_LABEL }));
    await waitFor(() => expect(voiceWakeSet).toHaveBeenCalledWith(true, "nixie"));
    expect(voiceAuthorize).toHaveBeenCalledTimes(1);
    await waitFor(() => expect(screen.getByRole("switch")).toBeChecked());
    expect(voiceStore.getState().wake).toEqual(ON);
  });

  it("a refused permission leaves the switch off, keeps the phrase, and shows the sentence saying what to allow", async () => {
    seed();
    voiceAuthorize.mockResolvedValue(NOT_AUTHORIZED);
    voiceWakeSet.mockResolvedValue(OFF);
    render(<BotVoiceWake />);
    fireEvent.click(screen.getByRole("switch", { name: WAKE_SWITCH_LABEL }));
    await waitFor(() => expect(voiceWakeSet).toHaveBeenCalledWith(false, "nixie"));
    expect(await screen.findByRole("status")).toHaveTextContent(NOT_AUTHORIZED.message);
    expect(screen.getByRole("switch")).not.toBeChecked();
    expect(voiceStore.getState().unavailable).toEqual(NOT_AUTHORIZED);
  });

  it("turning the switch off asks nothing: only arming is a deliberate voice act", async () => {
    seed({ wake: ON });
    voiceWakeSet.mockResolvedValue(OFF);
    render(<BotVoiceWake />);
    fireEvent.click(screen.getByRole("switch", { name: WAKE_SWITCH_LABEL }));
    await waitFor(() => expect(voiceWakeSet).toHaveBeenCalledWith(false, "nixie"));
    expect(voiceAuthorize).not.toHaveBeenCalled();
  });

  it("a refused phrase renders Rust's sentence, and the switch and store are untouched", async () => {
    seed();
    const refusal = {
      code: "internal",
      message:
        'use at least 5 letters in total — "ok" is too short for the recogniser to tell from noise',
      accountId: null,
      retriable: false,
    };
    voiceWakeSet.mockRejectedValue(refusal);
    render(<BotVoiceWake />);
    const box = screen.getByLabelText(WAKE_PHRASE_LABEL, { selector: "input" });
    fireEvent.change(box, { target: { value: "ok" } });
    fireEvent.click(screen.getByRole("button", { name: WAKE_SAVE_LABEL }));
    await waitFor(() => expect(voiceWakeSet).toHaveBeenCalledWith(false, "ok"));
    expect(await screen.findByRole("alert")).toHaveTextContent(refusal.message);
    expect(screen.getByRole("switch")).not.toBeChecked();
    expect(voiceStore.getState().wake).toEqual(OFF);
  });

  it("does not offer Save for a phrase that is what Rust already holds", () => {
    seed();
    render(<BotVoiceWake />);
    expect(screen.getByRole("button", { name: WAKE_SAVE_LABEL })).toBeDisabled();
    fireEvent.change(screen.getByLabelText(WAKE_PHRASE_LABEL, { selector: "input" }), {
      target: { value: "hej keeper" },
    });
    expect(screen.getByRole("button", { name: WAKE_SAVE_LABEL })).toBeEnabled();
  });
});

describe("BotVoiceWake — the chip and the sentence", () => {
  it("announces the listening state as a status chip while the microphone is open for the phrase", () => {
    seed({ wake: ON, state: IDLE_ARMED });
    render(<BotVoiceWake />);
    const chip = screen.getByRole("status");
    expect(chip).toHaveTextContent(wakeListeningLabel("nixie"));
    expect(chip).toHaveAttribute("aria-live", "polite");
  });

  it("keeps the chip during a turn's listening and drops it once the microphone is released", () => {
    seed({ wake: ON, state: { kind: "listening", heard: "what time" } });
    const { rerender } = render(<BotVoiceWake />);
    expect(screen.getByRole("status")).toHaveTextContent(wakeListeningLabel(null));
    voiceStore.getState().applyState({ kind: "speaking" });
    rerender(<BotVoiceWake />);
    expect(screen.queryByRole("status")).toBeNull();
  });

  it("shows no chip from the switch alone: the snapshot, not the setting, is what lights it", () => {
    seed({ wake: ON, state: null });
    render(<BotVoiceWake />);
    expect(screen.getByRole("switch")).toBeChecked();
    expect(screen.queryByRole("status")).toBeNull();
  });

  it("renders the limits sentence beside the switch, verbatim from keeper-core, stating every fact", () => {
    seed();
    render(<BotVoiceWake />);
    const section = screen.getByRole("region", { name: WAKE_PHRASE_LABEL });
    expect(section).toContainElement(screen.getByRole("switch"));
    expect(section).toHaveTextContent(LIMITS);
    for (const fact of [
      "another app is in front",
      "screen is locked",
      "turn it off",
      "force-quit",
      "microphone indicator",
      "cannot be hidden",
      "battery",
    ]) {
      expect(LIMITS).toContain(fact);
    }
    expect(LIMITS).not.toMatch(/not yet|for now|coming|later/);
  });
});
