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
 * 7. **The language control (Epic 63)** — offers exactly what the device can
 *    run on-device plus "Choose for me", sends the choice (or `null`) through
 *    Rust and re-asks availability; is absent on an empty list with Rust's
 *    sentence explaining; shows the language in force whether the setting
 *    is unset or explicit, and withholds it while Rust refuses that language.
 */

import { readFileSync } from "node:fs";
import path from "node:path";
import { fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import {
  BotVoiceWake,
  VOICE_LOCALE_AUTO_LABEL,
  VOICE_LOCALE_LABEL,
  VOICE_LOCALE_NOTE,
  voiceListeningIn,
  voiceLocaleName,
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
const voiceAvailability = vi.fn<() => Promise<VoiceUnavailableVm | null>>();
const voiceLocaleSet = vi.fn<(locale: string | null) => Promise<VoiceWakeVm>>();
vi.mock("@/lib/ipc/client", async (importOriginal) => {
  const actual = await importOriginal<typeof import("@/lib/ipc/client")>();
  return {
    ...actual,
    voiceWakeSet: (enabled: boolean, phrase: string) => voiceWakeSet(enabled, phrase),
    voiceAuthorize: () => voiceAuthorize(),
    voiceAvailability: () => voiceAvailability(),
    voiceLocaleSet: (locale: string | null) => voiceLocaleSet(locale),
  };
});

/** iOS's `VoicePlatform::limits` as `keeper-core` holds it, so the fixture is the real sentence. */
function rustLimits(): string {
  const source = readFileSync(
    path.resolve(__dirname, "../../../src-tauri/crates/keeper-core/src/voice/platform.rs"),
    "utf8",
  );
  // The iOS constant, which is the one this fixture stands for; the Mac has
  // its own sentence in the same file and must not be picked up here.
  const match = /noun: "phone",[\s\S]*?limits: "([^"]+)"/.exec(source);
  if (match?.[1] === undefined) {
    throw new Error("VoicePlatform::IOS.limits not found in voice/platform.rs");
  }
  return match[1];
}

const LIMITS = rustLimits();
/** hesperia's real answer: four English variants and nothing else. */
const ON_DEVICE = ["en-ID", "en-PH", "en-SA", "en-US"];
const OFF: VoiceWakeVm = {
  enabled: false,
  phrase: "nixie",
  limits: LIMITS,
  locale: "en-US",
  localeChosen: null,
  onDeviceLocales: ON_DEVICE,
};
const ON: VoiceWakeVm = { ...OFF, enabled: true };
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
/** The owner's case: a Polish phone whose on-device assets are English. */
const POLISH_REFUSED: VoiceUnavailableVm = {
  kind: "noOnDeviceRecognition",
  locale: "pl-PL",
  message:
    "speech recognition for pl-PL has no on-device asset on this phone — downloading it under Settings > General > Keyboard > Dictation Languages may add one, or choose en-ID, en-PH, en-SA or en-US",
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
  voiceAvailability.mockReset();
  voiceLocaleSet.mockReset();
  voiceAuthorize.mockResolvedValue(null);
  voiceAvailability.mockResolvedValue(null);
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

describe("BotVoiceWake — the language", () => {
  const control = () => screen.getByRole("combobox", { name: VOICE_LOCALE_LABEL });

  it("offers exactly the on-device languages plus Choose for me, and sends a choice through Rust", async () => {
    seed();
    const chosen: VoiceWakeVm = { ...OFF, locale: "en-PH", localeChosen: "en-PH" };
    voiceLocaleSet.mockResolvedValue(chosen);
    render(<BotVoiceWake />);
    const options = within(control()).getAllByRole("option");
    expect(options.map((option) => option.textContent)).toEqual([
      VOICE_LOCALE_AUTO_LABEL,
      ...ON_DEVICE.map(voiceLocaleName),
    ]);
    expect(options.map((option) => (option as HTMLOptionElement).value)).toEqual([
      "",
      ...ON_DEVICE,
    ]);
    // Polish is not on the list, and no option claims it.
    expect(screen.queryByRole("option", { name: /Polish|pl-PL/ })).toBeNull();
    fireEvent.change(control(), { target: { value: "en-PH" } });
    await waitFor(() => expect(voiceLocaleSet).toHaveBeenCalledWith("en-PH"));
    await waitFor(() => expect(voiceStore.getState().wake).toEqual(chosen));
    expect(control()).toHaveValue("en-PH");
    // Availability is asked again: whether the language in force runs here
    // is Rust's answer, refreshed after every write.
    await waitFor(() => expect(voiceAvailability).toHaveBeenCalledTimes(1));
  });

  it("sends null for Choose for me, and shows the refusal Rust then gives beside the control", async () => {
    seed({ wake: { ...OFF, locale: "en-US", localeChosen: "en-US" } });
    voiceLocaleSet.mockResolvedValue({ ...OFF, locale: "pl-PL", localeChosen: null });
    voiceAvailability.mockResolvedValue(POLISH_REFUSED);
    render(<BotVoiceWake />);
    expect(control()).toHaveValue("en-US");
    fireEvent.change(control(), { target: { value: "" } });
    await waitFor(() => expect(voiceLocaleSet).toHaveBeenCalledWith(null));
    expect(await screen.findByRole("status")).toHaveTextContent(POLISH_REFUSED.message);
    expect(control()).toHaveValue("");
    // No "listens in Polish" beside a sentence saying Polish cannot run here.
    expect(screen.queryByText(voiceListeningIn("pl-PL"))).toBeNull();
  });

  it("is absent — no control, no note — on an empty list, and Rust's sentence explains", () => {
    const none: VoiceUnavailableVm = {
      kind: "noOnDeviceRecognition",
      locale: "pl-PL",
      message:
        "speech recognition for pl-PL has no on-device asset on this phone — no language on this phone can run locally right now",
    };
    seed({ wake: { ...OFF, locale: "pl-PL", onDeviceLocales: [] }, unavailable: none });
    render(<BotVoiceWake />);
    expect(screen.queryByRole("combobox", { name: VOICE_LOCALE_LABEL })).toBeNull();
    expect(screen.queryByText(VOICE_LOCALE_NOTE)).toBeNull();
    expect(screen.getByRole("status")).toHaveTextContent(none.message);
    // The wake switch is still there: the refusal is a state, not absence.
    expect(screen.getByRole("switch", { name: WAKE_SWITCH_LABEL })).toBeInTheDocument();
  });

  it("offers a list of one as a control, not as absence", () => {
    seed({ wake: { ...OFF, onDeviceLocales: ["en-US"] } });
    render(<BotVoiceWake />);
    expect(within(control()).getAllByRole("option")).toHaveLength(2);
  });

  it("shows the language in force as Choose for me while the setting is unset", () => {
    seed();
    render(<BotVoiceWake />);
    expect(control()).toHaveValue("");
    expect(control()).toHaveDisplayValue(VOICE_LOCALE_AUTO_LABEL);
    expect(screen.getByText(voiceListeningIn("en-US"))).toBeInTheDocument();
    expect(voiceListeningIn("en-US")).toBe("Listens in American English (en-US).");
  });

  it("shows the explicit language when the setting is set", () => {
    seed({ wake: { ...OFF, locale: "en-SA", localeChosen: "en-SA" } });
    render(<BotVoiceWake />);
    expect(control()).toHaveValue("en-SA");
    expect(control()).toHaveDisplayValue(voiceLocaleName("en-SA"));
    expect(screen.getByText(voiceListeningIn("en-SA"))).toBeInTheDocument();
  });

  it("keeps the refusal and its remedy beside the control that fixes it", () => {
    seed({ wake: { ...OFF, locale: "pl-PL" }, unavailable: POLISH_REFUSED });
    render(<BotVoiceWake />);
    const section = screen.getByRole("region", { name: WAKE_PHRASE_LABEL });
    expect(section).toContainElement(control());
    expect(within(section).getByRole("status")).toHaveTextContent(POLISH_REFUSED.message);
    expect(screen.queryByText(voiceListeningIn("pl-PL"))).toBeNull();
  });

  it("says what the list is: this device's own languages, not the model's", () => {
    seed();
    render(<BotVoiceWake />);
    expect(screen.getByText(VOICE_LOCALE_NOTE)).toBeInTheDocument();
    expect(VOICE_LOCALE_NOTE).toMatch(/on this device only/);
    expect(VOICE_LOCALE_NOTE).toMatch(/not every language the model understands/);
  });

  it("a refused write renders Rust's sentence and leaves the choice where Rust left it", async () => {
    seed();
    voiceLocaleSet.mockRejectedValue({
      code: "internal",
      message: "pl-PL cannot run on this phone — choose one of the languages listed",
      accountId: null,
      retriable: false,
    });
    render(<BotVoiceWake />);
    fireEvent.change(control(), { target: { value: "en-US" } });
    expect(await screen.findByRole("alert")).toHaveTextContent(/cannot run on this phone/);
    expect(control()).toHaveValue("");
    expect(voiceAvailability).not.toHaveBeenCalled();
  });

  it("names an unfamiliar identifier as it is, and an OS-spelled one by its language", () => {
    expect(voiceLocaleName("zz-ZZ")).toBe("zz-ZZ");
    expect(voiceLocaleName("en_US")).toBe("American English (en_US)");
    // The region stays: en-ID, en-PH and en-SA are four English entries.
    expect(voiceLocaleName("pl-PL")).toBe("Polish (Poland) (pl-PL)");
    expect(voiceLocaleName("en-PH")).toBe("English (Philippines) (en-PH)");
  });
});
