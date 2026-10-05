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
 * 6. **The section exists on `voice_availability`'s answer alone (AD-179)**:
 *    absent while that question has not been answered, whatever
 *    `capabilities.bots` says.
 * 7. **The language control (Epic 63)** — offers exactly what the device can
 *    run on-device plus "Choose for me", sends the choice (or `null`) through
 *    Rust and re-asks availability; is absent on an empty list with Rust's
 *    sentence explaining; shows the language in force whether the setting
 *    is unset or explicit, and withholds it while Rust refuses that language.
 * 8. **Folded to one line (Epic 64, Story 64.1, AD-184)** — with a `fold`,
 *    the block is a disclosure whose label is the truthful line: the switch,
 *    the phrase and the language live from `VoiceWakeVm`, and the refusal's
 *    first clause when the port refuses. Folded, no control is in the tree;
 *    unfolded, the whole block is. Without a `fold` nothing above changes.
 * 9. **The switch is intent (Epic 65, Story 65.2, AD-190)** — a refusal at
 *    arming time is written as ON and shown as a refusal with its remedy;
 *    a grant through the switch lifts a stale permission refusal and only
 *    that; a refusal the probe did not predict is the turn's reason, beside
 *    the switch, once.
 * 10. **The stop word (Epic 67, Story 67.3, AD-208)** — one line under the
 *    phrase shows `VoiceWakeVm.stopPhrase` and saves it through the same
 *    `voiceWakeSet` as the phrase; a refused word renders Rust's sentence.
 * 11. **The turn models' line (Epic 97, UX-DR142)** — under the switch, each
 *    state's sentence letter for letter from `VoiceWakeVm.turnModels`, named
 *    for what it is; absent where Rust sends none or voice is unsupported.
 *    It follows the shell's `keeper://voice-wake` event through one listener
 *    however many hosts are open, is read again when a host opens or the
 *    document comes back, sets no timer, and a read answered after a newer
 *    VM landed never puts the older one back.
 */

import { readFileSync } from "node:fs";
import path from "node:path";
import { act, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import {
  BotVoiceWake,
  STOP_PHRASE_LABEL,
  STOP_SAVE_LABEL,
  TURN_MODELS_LABEL,
  VOICE_FOLDED_OFF,
  VOICE_LOCALE_AUTO_LABEL,
  VOICE_LOCALE_LABEL,
  VOICE_LOCALE_NOTE,
  voiceFoldedLine,
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

const voiceWakeSet =
  vi.fn<(enabled: boolean, phrase: string, stopPhrase: string) => Promise<VoiceWakeVm>>();
const voiceAuthorize = vi.fn<() => Promise<VoiceUnavailableVm | null>>();
const voiceAvailability = vi.fn<() => Promise<VoiceUnavailableVm | null>>();
const voiceLocaleSet = vi.fn<(locale: string | null) => Promise<VoiceWakeVm>>();
const voiceWakeGet = vi.fn<() => Promise<VoiceWakeVm>>();
/** What the shell's `keeper://voice-wake` reaches: every live listener. */
let wakeListeners: ((wake: VoiceWakeVm) => void)[] = [];
const unlistenVoiceWake = vi.fn();
const listenVoiceWake = vi.fn((onWake: (wake: VoiceWakeVm) => void) => {
  wakeListeners.push(onWake);
  return Promise.resolve(() => {
    unlistenVoiceWake();
    wakeListeners = wakeListeners.filter((each) => each !== onWake);
  });
});
vi.mock("@/lib/ipc/client", async (importOriginal) => {
  const actual = await importOriginal<typeof import("@/lib/ipc/client")>();
  return {
    ...actual,
    voiceWakeSet: (enabled: boolean, phrase: string, stopPhrase: string) =>
      voiceWakeSet(enabled, phrase, stopPhrase),
    voiceAuthorize: () => voiceAuthorize(),
    voiceAvailability: () => voiceAvailability(),
    voiceLocaleSet: (locale: string | null) => voiceLocaleSet(locale),
    voiceWakeGet: () => voiceWakeGet(),
    listenVoiceWake: (onWake: (wake: VoiceWakeVm) => void) => listenVoiceWake(onWake),
  };
});

/** The shell sends `wake` on `keeper://voice-wake`. */
function shellSends(wake: VoiceWakeVm) {
  act(() => {
    for (const listener of wakeListeners) {
      listener(wake);
    }
  });
}

/** A promise the test settles when it chooses. */
function deferred<T>() {
  let resolve: (value: T) => void = () => {};
  const promise = new Promise<T>((settle) => {
    resolve = settle;
  });
  return { promise, resolve };
}

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
  stopPhrase: "stop",
  voiceTarget: null,
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
const NO_MICROPHONE: VoiceUnavailableVm = {
  kind: "noMicrophone",
  message: "no microphone is available on this device",
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
  voiceWakeGet.mockReset();
  listenVoiceWake.mockClear();
  unlistenVoiceWake.mockClear();
  // Unanswered unless a test answers it: the seeded VM is what is drawn.
  voiceWakeGet.mockReturnValue(new Promise<never>(() => {}));
  voiceAuthorize.mockResolvedValue(null);
  voiceAvailability.mockResolvedValue(null);
  voiceStore.setState({ state: null, unavailable: undefined, wake: null });
  capabilitiesStore.getState().applySnapshot(DEFAULT_CAPABILITIES);
});

describe("BotVoiceWake — where it exists", () => {
  it("exists on the availability answer alone, whatever capabilities.bots says (AD-179)", () => {
    seed({ bots: false });
    render(<BotVoiceWake />);
    expect(screen.getByRole("switch", { name: WAKE_SWITCH_LABEL })).toBeInTheDocument();
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
    await waitFor(() => expect(voiceWakeSet).toHaveBeenCalledWith(true, "nixie", "stop"));
    expect(voiceAuthorize).toHaveBeenCalledTimes(1);
    await waitFor(() => expect(screen.getByRole("switch")).toBeChecked());
    expect(voiceStore.getState().wake).toEqual(ON);
  });

  it("a refused permission persists the switch ON, keeps the phrase, and shows the sentence saying what to allow (AD-190)", async () => {
    seed();
    voiceAuthorize.mockResolvedValue(NOT_AUTHORIZED);
    voiceWakeSet.mockResolvedValue(ON);
    render(<BotVoiceWake />);
    fireEvent.click(screen.getByRole("switch", { name: WAKE_SWITCH_LABEL }));
    // The person's choice is what is written — never the port's "no".
    await waitFor(() => expect(voiceWakeSet).toHaveBeenCalledWith(true, "nixie", "stop"));
    expect(await screen.findByRole("status")).toHaveTextContent(NOT_AUTHORIZED.message);
    await waitFor(() => expect(screen.getByRole("switch")).toBeChecked());
    expect(voiceStore.getState().unavailable).toEqual(NOT_AUTHORIZED);
    expect(voiceStore.getState().wake).toEqual(ON);
  });

  it("a grant given through the switch clears the stale refusal, as the mic button does", async () => {
    seed({ unavailable: NOT_AUTHORIZED });
    voiceWakeSet.mockResolvedValue(ON);
    render(<BotVoiceWake />);
    expect(screen.getByRole("status")).toHaveTextContent(NOT_AUTHORIZED.message);
    fireEvent.click(screen.getByRole("switch", { name: WAKE_SWITCH_LABEL }));
    await waitFor(() => expect(voiceWakeSet).toHaveBeenCalledWith(true, "nixie", "stop"));
    await waitFor(() => expect(voiceStore.getState().unavailable).toBeNull());
    expect(screen.queryByRole("status")).toBeNull();
  });

  it("a grant lifts only a permission refusal: a missing on-device asset is still missing", async () => {
    seed({ wake: { ...OFF, locale: "pl-PL" }, unavailable: POLISH_REFUSED });
    voiceWakeSet.mockResolvedValue({ ...ON, locale: "pl-PL" });
    render(<BotVoiceWake />);
    fireEvent.click(screen.getByRole("switch", { name: WAKE_SWITCH_LABEL }));
    await waitFor(() => expect(voiceWakeSet).toHaveBeenCalledWith(true, "nixie", "stop"));
    expect(voiceStore.getState().unavailable).toEqual(POLISH_REFUSED);
    expect(screen.getByRole("status")).toHaveTextContent(POLISH_REFUSED.message);
    await waitFor(() => expect(screen.getByRole("switch")).toBeChecked());
  });

  it("a refusal the probe did not predict — the turn failed at arming — is Rust's reason beside the switch, switch on", () => {
    // The port refused `start_listening` for the phrase: `voice_ipc::arm`
    // fails the turn with the same sentence a probe would have given.
    seed({ wake: ON, state: { kind: "failed", reason: NO_MICROPHONE.message } });
    render(<BotVoiceWake />);
    expect(screen.getByRole("switch")).toBeChecked();
    expect(screen.getByRole("status")).toHaveTextContent(NO_MICROPHONE.message);
  });

  it("does not say the same refusal twice when the probe and the turn agree", () => {
    seed({
      wake: ON,
      unavailable: NOT_AUTHORIZED,
      state: { kind: "failed", reason: NOT_AUTHORIZED.message },
    });
    render(<BotVoiceWake />);
    expect(screen.getAllByRole("status")).toHaveLength(1);
  });

  it("turning the switch off asks nothing: only arming is a deliberate voice act", async () => {
    seed({ wake: ON });
    voiceWakeSet.mockResolvedValue(OFF);
    render(<BotVoiceWake />);
    fireEvent.click(screen.getByRole("switch", { name: WAKE_SWITCH_LABEL }));
    await waitFor(() => expect(voiceWakeSet).toHaveBeenCalledWith(false, "nixie", "stop"));
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
    await waitFor(() => expect(voiceWakeSet).toHaveBeenCalledWith(false, "ok", "stop"));
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

  it("shows the stop word under the phrase and saves it through the same write (Epic 67, AD-208)", async () => {
    seed();
    voiceWakeSet.mockResolvedValue({ ...OFF, stopPhrase: "przestań" });
    render(<BotVoiceWake />);
    const box = screen.getByLabelText(STOP_PHRASE_LABEL, { selector: "input" });
    expect(box).toHaveValue("stop");
    expect(screen.getByRole("button", { name: STOP_SAVE_LABEL })).toBeDisabled();
    fireEvent.change(box, { target: { value: "przestań" } });
    expect(screen.getByRole("button", { name: STOP_SAVE_LABEL })).toBeEnabled();
    fireEvent.click(screen.getByRole("button", { name: STOP_SAVE_LABEL }));
    await waitFor(() => expect(voiceWakeSet).toHaveBeenCalledWith(false, "nixie", "przestań"));
    // Saving the word is not arming: nothing is asked for by name.
    expect(voiceAuthorize).not.toHaveBeenCalled();
    await waitFor(() => expect(voiceStore.getState().wake?.stopPhrase).toBe("przestań"));
    expect(screen.getByRole("button", { name: STOP_SAVE_LABEL })).toBeDisabled();
  });

  it("a refused stop word renders Rust's sentence and the held word stays", async () => {
    seed();
    const refusal = {
      code: "internal",
      message:
        'use at least 3 letters in total — "no" is too short for the recogniser to tell from noise',
      accountId: null,
      retriable: false,
    };
    voiceWakeSet.mockRejectedValue(refusal);
    render(<BotVoiceWake />);
    fireEvent.change(screen.getByLabelText(STOP_PHRASE_LABEL, { selector: "input" }), {
      target: { value: "no" },
    });
    fireEvent.click(screen.getByRole("button", { name: STOP_SAVE_LABEL }));
    await waitFor(() => expect(voiceWakeSet).toHaveBeenCalledWith(false, "nixie", "no"));
    expect(await screen.findByRole("alert")).toHaveTextContent(refusal.message);
    expect(voiceStore.getState().wake).toEqual(OFF);
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
    seed({ wake: ON, state: { kind: "listening", heard: "what time", level: null } });
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

describe("BotVoiceWake — folded to one line (Story 64.1)", () => {
  it("says whether listening is armed, the phrase and the language, from the setting", () => {
    expect(voiceFoldedLine(ON, null)).toBe('Listening for "nixie" · en-US');
    expect(voiceFoldedLine(OFF, null)).toBe(`${VOICE_FOLDED_OFF} · en-US`);
    expect(voiceFoldedLine({ ...OFF, locale: "pl-PL" }, null)).toBe(`${VOICE_FOLDED_OFF} · pl-PL`);
  });

  it("appends the refusal's first clause, so 'not allowed' is read without unfolding", () => {
    // Each of Rust's sentences opens with the fact and follows it with the
    // remedy after a dash, a comma or a semicolon; the remedy is what
    // unfolding shows. A sentence with no such break is carried whole.
    expect(voiceFoldedLine(OFF, NOT_AUTHORIZED)).toBe(
      `${VOICE_FOLDED_OFF} · en-US · keeper is not allowed to use the microphone`,
    );
    expect(voiceFoldedLine({ ...OFF, locale: "pl-PL" }, POLISH_REFUSED)).toBe(
      `${VOICE_FOLDED_OFF} · pl-PL · speech recognition for pl-PL has no on-device asset on this phone`,
    );
    expect(voiceFoldedLine(OFF, NO_MICROPHONE)).toBe(
      `${VOICE_FOLDED_OFF} · en-US · no microphone is available on this device`,
    );
  });

  it("folded, draws the line as a collapsed disclosure and no control", () => {
    seed({ unavailable: NOT_AUTHORIZED });
    const onToggle = vi.fn();
    render(<BotVoiceWake fold={{ folded: true, onToggle }} />);
    const line = voiceFoldedLine(OFF, NOT_AUTHORIZED);
    const disclosure = screen.getByRole("button", { name: `Expand ${line}` });
    expect(disclosure).toHaveAttribute("aria-expanded", "false");
    expect(disclosure).toHaveTextContent(line);
    // Hidden, not merely small: nothing of the block is reachable folded.
    expect(screen.queryByRole("switch")).toBeNull();
    expect(screen.queryByRole("combobox")).toBeNull();
    expect(screen.queryByRole("status")).toBeNull();
    fireEvent.click(disclosure);
    expect(onToggle).toHaveBeenCalledTimes(1);
  });

  it("unfolded, is the whole block under an expanded disclosure", () => {
    seed({ unavailable: NOT_AUTHORIZED });
    render(<BotVoiceWake fold={{ folded: false, onToggle: vi.fn() }} />);
    expect(screen.getByRole("button", { name: /^Collapse / })).toHaveAttribute(
      "aria-expanded",
      "true",
    );
    expect(screen.getByRole("switch", { name: WAKE_SWITCH_LABEL })).toBeInTheDocument();
    expect(screen.getByRole("combobox", { name: VOICE_LOCALE_LABEL })).toBeInTheDocument();
    expect(screen.getByRole("status")).toHaveTextContent(NOT_AUTHORIZED.message);
    expect(screen.getByText(LIMITS)).toBeInTheDocument();
  });

  it("follows the switch, the phrase and the language live", () => {
    seed();
    const { rerender } = render(<BotVoiceWake fold={{ folded: true, onToggle: vi.fn() }} />);
    expect(screen.getByRole("button")).toHaveTextContent(`${VOICE_FOLDED_OFF} · en-US`);
    voiceStore.getState().applyWake({ ...ON, phrase: "hej keeper", locale: "en-PH" });
    rerender(<BotVoiceWake fold={{ folded: true, onToggle: vi.fn() }} />);
    expect(screen.getByRole("button")).toHaveTextContent('Listening for "hej keeper" · en-PH');
    voiceStore.getState().applyAvailability(POLISH_REFUSED);
    rerender(<BotVoiceWake fold={{ folded: true, onToggle: vi.fn() }} />);
    expect(screen.getByRole("button")).toHaveTextContent(
      "· speech recognition for pl-PL has no on-device asset on this phone",
    );
  });

  it("is still absent where the block is absent", () => {
    seed({ unavailable: UNSUPPORTED });
    const { container } = render(<BotVoiceWake fold={{ folded: true, onToggle: vi.fn() }} />);
    expect(container).toBeEmptyDOMElement();
  });
});

/** `TurnModelsState::vm` for each state, the sentences as `keeper_core::voice::turn_models` words them. */
const FALLBACK = "keeper waits 1.8 s after you stop";
const TURN_READY: NonNullable<VoiceWakeVm["turnModels"]> = {
  state: "ready",
  sentence: "Turn models ready",
  missing: [],
};
const TURN_FETCHING: NonNullable<VoiceWakeVm["turnModels"]> = {
  state: "fetching",
  sentence: `Fetching the turn models from your account… ${FALLBACK} until they are here`,
  missing: [],
};
const TURN_STATES: NonNullable<VoiceWakeVm["turnModels"]>[] = [
  TURN_READY,
  {
    state: "missing",
    sentence: `Turn models missing: silero-vad/model.onnx — ${FALLBACK}`,
    missing: ["silero-vad/model.onnx"],
  },
  {
    state: "missing",
    sentence: `Turn models out of date — keeper brings them up to date after the next sync, and ${FALLBACK} until then`,
    missing: [],
  },
  {
    state: "failed",
    sentence: `The speech detection model “silero-v5” set by \`transcription.vad_model\` is not on this device. Set \`transcription.vad_model\` in your account's settings.toml to another folder of _models/, or remove it to use the one [vad] in models.toml names. Until then, ${FALLBACK}.`,
    missing: [],
  },
  {
    state: "failed",
    sentence: `The turn models could not be fetched: the config repository did not answer — ${FALLBACK}`,
    missing: [],
  },
  {
    state: "noAccount",
    sentence: `No turn models without an account — ${FALLBACK}. They come from your account's settings repository (Settings → Account).`,
    missing: [],
  },
  TURN_FETCHING,
];

describe("BotVoiceWake — the turn models' line (UX-DR142)", () => {
  it.each(
    TURN_STATES.map((turnModels) => [turnModels.sentence, turnModels]),
  )("says %s, letter for letter, as the line named for the turn models", (_, turnModels) => {
    seed({ wake: { ...OFF, turnModels } });
    render(<BotVoiceWake />);
    const line = screen.getByRole("status", { name: TURN_MODELS_LABEL });
    expect(line.textContent).toBe(turnModels.sentence);
  });

  it("sits under the switch, before the phrase", () => {
    seed({ wake: { ...OFF, turnModels: TURN_READY } });
    render(<BotVoiceWake />);
    const line = screen.getByRole("status", { name: TURN_MODELS_LABEL });
    const toggle = screen.getByRole("switch", { name: WAKE_SWITCH_LABEL });
    const phrase = screen.getByLabelText(WAKE_PHRASE_LABEL, { selector: "input" });
    expect(toggle.compareDocumentPosition(line) & Node.DOCUMENT_POSITION_FOLLOWING).toBeTruthy();
    expect(line.compareDocumentPosition(phrase) & Node.DOCUMENT_POSITION_FOLLOWING).toBeTruthy();
  });

  it("is absent where Rust sends no turn models: no line, nothing in its place", () => {
    seed({ wake: OFF });
    render(<BotVoiceWake />);
    expect(screen.queryByRole("status", { name: TURN_MODELS_LABEL })).toBeNull();
    expect(screen.queryByText(/turn models/i)).toBeNull();
  });

  it("is absent with the block where voice is unsupported", () => {
    seed({ wake: { ...OFF, turnModels: TURN_READY }, unavailable: UNSUPPORTED });
    const { container } = render(<BotVoiceWake />);
    expect(container).toBeEmptyDOMElement();
  });
});

describe("BotVoiceWake — the turn models' line follows the shell", () => {
  const MISSING = TURN_STATES[1];
  const FAILED = TURN_STATES[4];
  const lineText = () => screen.getByRole("status", { name: TURN_MODELS_LABEL }).textContent;

  it.each([
    ["missing", MISSING],
    ["failed", FAILED],
  ])("goes %s → fetching → ready on the shell's events alone, with no timer", (_, first) => {
    const interval = vi.spyOn(window, "setInterval");
    seed({ wake: { ...OFF, turnModels: first } });
    render(<BotVoiceWake />);
    expect(lineText()).toBe(first.sentence);
    shellSends({ ...OFF, turnModels: TURN_FETCHING });
    expect(lineText()).toBe(TURN_FETCHING.sentence);
    shellSends({ ...OFF, turnModels: TURN_READY });
    expect(lineText()).toBe(TURN_READY.sentence);
    expect(interval).not.toHaveBeenCalled();
    expect(voiceWakeGet).toHaveBeenCalledTimes(1);
    interval.mockRestore();
  });

  it("holds one listener for every open host and lets it go with the last", async () => {
    seed({ wake: { ...OFF, turnModels: MISSING } });
    const settings = render(<BotVoiceWake />);
    const pane = render(<BotVoiceWake fold={{ folded: false, onToggle: vi.fn() }} />);
    await act(() => Promise.resolve());
    expect(listenVoiceWake).toHaveBeenCalledTimes(1);
    shellSends({ ...OFF, turnModels: TURN_READY });
    for (const line of screen.getAllByRole("status", { name: TURN_MODELS_LABEL })) {
      expect(line.textContent).toBe(TURN_READY.sentence);
    }
    settings.unmount();
    expect(unlistenVoiceWake).not.toHaveBeenCalled();
    pane.unmount();
    expect(unlistenVoiceWake).toHaveBeenCalledTimes(1);
  });

  it("reads when a host opens and when it unfolds; folded, it reads and listens to nothing", () => {
    seed({ wake: { ...OFF, turnModels: MISSING } });
    const { rerender } = render(<BotVoiceWake fold={{ folded: true, onToggle: vi.fn() }} />);
    expect(voiceWakeGet).not.toHaveBeenCalled();
    expect(listenVoiceWake).not.toHaveBeenCalled();
    rerender(<BotVoiceWake fold={{ folded: false, onToggle: vi.fn() }} />);
    expect(voiceWakeGet).toHaveBeenCalledTimes(1);
    expect(listenVoiceWake).toHaveBeenCalledTimes(1);
  });

  it("reads again when the document comes back into view, once however many hosts are open", () => {
    seed({ wake: { ...OFF, turnModels: MISSING } });
    render(<BotVoiceWake />);
    render(<BotVoiceWake fold={{ folded: false, onToggle: vi.fn() }} />);
    expect(voiceWakeGet).toHaveBeenCalledTimes(2);
    act(() => {
      document.dispatchEvent(new Event("visibilitychange"));
    });
    expect(voiceWakeGet).toHaveBeenCalledTimes(3);
  });

  it("drops a read answered after the shell's newer VM", async () => {
    const read = deferred<VoiceWakeVm>();
    voiceWakeGet.mockReturnValueOnce(read.promise);
    seed({ wake: { ...OFF, turnModels: MISSING } });
    render(<BotVoiceWake />);
    shellSends({ ...OFF, turnModels: TURN_READY });
    await act(async () => read.resolve({ ...OFF, turnModels: TURN_FETCHING }));
    expect(lineText()).toBe(TURN_READY.sentence);
  });

  it("keeps the later read when two answer out of order", async () => {
    const older = deferred<VoiceWakeVm>();
    const newer = deferred<VoiceWakeVm>();
    voiceWakeGet.mockReturnValueOnce(older.promise).mockReturnValueOnce(newer.promise);
    seed({ wake: { ...OFF, turnModels: MISSING } });
    render(<BotVoiceWake />);
    act(() => {
      document.dispatchEvent(new Event("visibilitychange"));
    });
    await act(async () => newer.resolve({ ...OFF, turnModels: TURN_READY }));
    await act(async () => older.resolve({ ...OFF, turnModels: TURN_FETCHING }));
    expect(lineText()).toBe(TURN_READY.sentence);
  });

  it("keeps a saved phrase over a read asked before the save", async () => {
    const read = deferred<VoiceWakeVm>();
    voiceWakeGet.mockReturnValueOnce(read.promise);
    voiceWakeSet.mockResolvedValue({ ...OFF, phrase: "hey keeper", turnModels: TURN_FETCHING });
    seed({ wake: { ...OFF, turnModels: TURN_FETCHING } });
    render(<BotVoiceWake />);
    const box = screen.getByLabelText(WAKE_PHRASE_LABEL, { selector: "input" });
    fireEvent.change(box, { target: { value: "hey keeper" } });
    fireEvent.click(screen.getByRole("button", { name: WAKE_SAVE_LABEL }));
    await waitFor(() => expect(voiceStore.getState().wake?.phrase).toBe("hey keeper"));
    await act(async () => read.resolve({ ...OFF, turnModels: TURN_FETCHING }));
    expect(voiceStore.getState().wake?.phrase).toBe("hey keeper");
  });

  it("drops a read left pending by a closed host once the reopened host's read landed", async () => {
    const closed = deferred<VoiceWakeVm>();
    const reopened = deferred<VoiceWakeVm>();
    voiceWakeGet.mockReturnValueOnce(closed.promise).mockReturnValueOnce(reopened.promise);
    seed({ wake: { ...OFF, turnModels: MISSING } });
    render(<BotVoiceWake />).unmount();
    render(<BotVoiceWake />);
    await act(async () => reopened.resolve({ ...OFF, turnModels: TURN_READY }));
    await act(async () => closed.resolve({ ...OFF, turnModels: TURN_FETCHING }));
    expect(lineText()).toBe(TURN_READY.sentence);
  });

  it("lets the listener go when the host closed before it was registered", async () => {
    seed({ wake: { ...OFF, turnModels: MISSING } });
    render(<BotVoiceWake />).unmount();
    await act(() => Promise.resolve());
    expect(wakeListeners).toHaveLength(0);
    expect(unlistenVoiceWake).toHaveBeenCalledTimes(1);
  });
});
