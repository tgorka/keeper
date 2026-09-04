/**
 * Talk mode you can see (Story 62.6, FR-407, FR-408, NFR-51, AD-170/AD-171).
 *
 * What is asserted here that nothing else asserts:
 *
 * 1. **Three states, told apart without colour** — the button's accessible
 *    name, its `data-state` and `aria-pressed` differ across idle, listening
 *    and speaking; the status line says which in words.
 * 2. **Heard text reaches the composer, not the wire** — a button-started
 *    turn's `heard` snapshot is handed to `onHeard` as `"button"` once, and
 *    `BotComposer` puts it in the field where it can be edited; nothing
 *    calls send. A phrase-started turn is handed as `"phrase"`.
 * 3. **Stop abandons** — pressing while listening calls `voice_stop`, and the
 *    idle snapshot that follows is rendered idle: the surface honours the
 *    release rather than remembering a press.
 * 4. **The first press asks, by name, once** — `voice_authorize` precedes
 *    `voice_start`; a granted answer starts; a refusal starts nothing and
 *    renders Rust's sentence with Open Settings.
 * 5. **A voice turn's answer is spoken; a typed one is not** —
 *    `speakIfHeard` calls `voice_speak` from `heard` and never from idle.
 * 6. **Absent** where `capabilities.bots` is off, while availability is
 *    unanswered, and on `unsupported`.
 */

import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { BOT_COMPOSER_LABEL, BotComposer } from "@/components/bots/bot-composer";
import {
  BotVoiceMic,
  BotVoiceStatus,
  micState,
  speakIfHeard,
  VOICE_HEARD_STATUS,
  VOICE_LISTENING_STATUS,
  VOICE_OPEN_SETTINGS_LABEL,
  VOICE_SPEAKING_STATUS,
  VOICE_STOP_LISTENING_LABEL,
  VOICE_STOP_SPEAKING_LABEL,
  VOICE_TALK_LABEL,
} from "@/components/bots/bot-voice-mic";
import type { VoiceStateVm, VoiceUnavailableVm } from "@/lib/ipc/client";
import { capabilitiesStore, DEFAULT_CAPABILITIES } from "@/lib/stores/capabilities";
import { voiceStore } from "@/lib/stores/voice";

const voiceAuthorize = vi.fn<() => Promise<VoiceUnavailableVm | null>>();
const voiceStart = vi.fn<() => Promise<void>>();
const voiceStop = vi.fn<() => Promise<void>>();
const voiceSpeak = vi.fn<(text: string) => Promise<void>>();
const voiceStopSpeaking = vi.fn<() => Promise<void>>();
const iosOpenAppSettings = vi.fn<() => Promise<void>>();
vi.mock("@/lib/ipc/client", async (importOriginal) => {
  const actual = await importOriginal<typeof import("@/lib/ipc/client")>();
  return {
    ...actual,
    voiceAuthorize: () => voiceAuthorize(),
    voiceStart: () => voiceStart(),
    voiceStop: () => voiceStop(),
    voiceSpeak: (text: string) => voiceSpeak(text),
    voiceStopSpeaking: () => voiceStopSpeaking(),
    iosOpenAppSettings: () => iosOpenAppSettings(),
  };
});

const IDLE: VoiceStateVm = { kind: "idle", wake: null, listeningForWake: false };
const LISTENING: VoiceStateVm = { kind: "listening", heard: "", level: null };
const LISTENING_HALF: VoiceStateVm = {
  kind: "listening",
  heard: "what did I save",
  level: 0.3,
};
const HEARD: VoiceStateVm = { kind: "heard", text: "what did I save yesterday", level: null };
const SPEAKING: VoiceStateVm = { kind: "speaking" };
const NOT_AUTHORIZED: VoiceUnavailableVm = {
  kind: "notAuthorized",
  message:
    "keeper is not allowed to use the microphone or speech recognition on this phone — allow both under Settings > keeper",
};
const NO_MODEL: VoiceUnavailableVm = {
  kind: "noOnDeviceModel",
  locale: "pl_PL",
  message: "on-device speech recognition for pl_PL is not on this phone",
};
const UNSUPPORTED: VoiceUnavailableVm = {
  kind: "unsupported",
  message: "voice is not available in this build",
};

function seed(
  overrides: {
    unavailable?: VoiceUnavailableVm | null | undefined;
    state?: VoiceStateVm | null;
    bots?: boolean;
  } = {},
) {
  capabilitiesStore
    .getState()
    .applySnapshot({ ...DEFAULT_CAPABILITIES, bots: overrides.bots ?? true });
  voiceStore.setState({
    state: overrides.state ?? IDLE,
    unavailable: "unavailable" in overrides ? overrides.unavailable : null,
    wake: null,
  });
}

/** The controls as a person meets them: the line above, the button in the row. */
function Surface({ onHeard }: { onHeard: (text: string, origin: "button" | "phrase") => void }) {
  return (
    <>
      <BotVoiceStatus />
      <BotVoiceMic onHeard={onHeard} />
    </>
  );
}

beforeEach(() => {
  voiceAuthorize.mockReset();
  voiceStart.mockReset();
  voiceStop.mockReset();
  voiceSpeak.mockReset();
  voiceStopSpeaking.mockReset();
  iosOpenAppSettings.mockReset();
  voiceAuthorize.mockResolvedValue(null);
  voiceStart.mockResolvedValue();
  voiceStop.mockResolvedValue();
  voiceSpeak.mockResolvedValue();
  voiceStopSpeaking.mockResolvedValue();
  iosOpenAppSettings.mockResolvedValue();
  seed();
});

describe("micState", () => {
  it("wears one of three faces for every snapshot", () => {
    expect(micState(null)).toBe("idle");
    expect(micState(IDLE)).toBe("idle");
    expect(micState(HEARD)).toBe("idle");
    expect(micState({ kind: "sending", answering: false })).toBe("idle");
    expect(micState({ kind: "sending", answering: true })).toBe("idle");
    expect(micState({ kind: "failed", reason: "x" })).toBe("idle");
    expect(micState(LISTENING)).toBe("listening");
    expect(micState(SPEAKING)).toBe("speaking");
  });
});

describe("BotVoiceMic — three states you can read", () => {
  it("is Talk while idle, unpressed, with no status band", () => {
    render(<Surface onHeard={() => {}} />);
    const button = screen.getByRole("button", { name: VOICE_TALK_LABEL });
    expect(button).toHaveAttribute("data-state", "idle");
    expect(button).toHaveAttribute("aria-pressed", "false");
    expect(screen.queryByRole("status")).toBeNull();
  });

  it("is Cancel this question while listening, pressed, and the band shows the transcript as it forms", () => {
    seed({ state: LISTENING });
    const { rerender } = render(<Surface onHeard={() => {}} />);
    const button = screen.getByRole("button", { name: VOICE_STOP_LISTENING_LABEL });
    expect(button).toHaveAttribute("data-state", "listening");
    expect(button).toHaveAttribute("aria-pressed", "true");
    const band = screen.getByRole("status");
    expect(band).toHaveAttribute("data-voice", "listening");
    expect(band).toHaveTextContent(VOICE_LISTENING_STATUS);

    voiceStore.getState().applyState(LISTENING_HALF);
    rerender(<Surface onHeard={() => {}} />);
    expect(screen.getByRole("status")).toHaveTextContent("what did I save");
  });

  it("is Stop this answer while the answer is read aloud, and says so", () => {
    seed({ state: SPEAKING });
    render(<Surface onHeard={() => {}} />);
    const button = screen.getByRole("button", { name: VOICE_STOP_SPEAKING_LABEL });
    expect(button).toHaveAttribute("data-state", "speaking");
    expect(button).toHaveAttribute("aria-pressed", "false");
    expect(screen.getByRole("status")).toHaveTextContent(VOICE_SPEAKING_STATUS);
  });

  it("names each state differently, so no two are told apart by colour alone", () => {
    expect(VOICE_TALK_LABEL).not.toBe(VOICE_STOP_LISTENING_LABEL);
    expect(VOICE_STOP_LISTENING_LABEL).not.toBe(VOICE_STOP_SPEAKING_LABEL);
    expect(VOICE_TALK_LABEL).not.toBe(VOICE_STOP_SPEAKING_LABEL);
  });
});

describe("BotVoiceMic — heard text goes to the composer, not the wire", () => {
  it("hands a button-started turn's transcript on as `button`, once", async () => {
    const onHeard = vi.fn();
    const { rerender } = render(<Surface onHeard={onHeard} />);
    fireEvent.click(screen.getByRole("button", { name: VOICE_TALK_LABEL }));
    await waitFor(() => expect(voiceStart).toHaveBeenCalledTimes(1));

    voiceStore.getState().applyState(LISTENING);
    rerender(<Surface onHeard={onHeard} />);
    voiceStore.getState().applyState(HEARD);
    rerender(<Surface onHeard={onHeard} />);
    expect(onHeard).toHaveBeenCalledTimes(1);
    expect(onHeard).toHaveBeenCalledWith("what did I save yesterday", "button");
    expect(screen.getByRole("status")).toHaveTextContent(VOICE_HEARD_STATUS);

    // The same snapshot re-rendered is not a second hearing.
    rerender(<Surface onHeard={onHeard} />);
    expect(onHeard).toHaveBeenCalledTimes(1);
  });

  it("hands a turn nobody pressed for — the phrase's — on as `phrase`", () => {
    const onHeard = vi.fn();
    const { rerender } = render(<Surface onHeard={onHeard} />);
    voiceStore.getState().applyState(LISTENING);
    rerender(<Surface onHeard={onHeard} />);
    voiceStore.getState().applyState(HEARD);
    rerender(<Surface onHeard={onHeard} />);
    expect(onHeard).toHaveBeenCalledWith("what did I save yesterday", "phrase");
    expect(voiceStart).not.toHaveBeenCalled();
  });

  it("puts the heard text in the composer's field, where it can be edited, and sends nothing", () => {
    const onSend = vi.fn();
    const composer = (heard: { text: string; seq: number } | null) => (
      <BotComposer
        onSend={onSend}
        onStop={() => {}}
        onCommand={() => null}
        commandContext={{
          providerKind: "hermes",
          hasProvider: true,
          hasBot: true,
          hasSession: true,
          modelTools: null,
        }}
        streaming={false}
        disabled={false}
        heard={heard}
      />
    );
    const { rerender } = render(composer(null));
    const field = screen.getByLabelText(BOT_COMPOSER_LABEL);
    expect(field).toHaveValue("");

    rerender(composer({ text: "what did I save yesterday", seq: 1 }));
    expect(field).toHaveValue("what did I save yesterday");
    expect(onSend).not.toHaveBeenCalled();

    fireEvent.change(field, { target: { value: "what did I save on Tuesday" } });
    expect(field).toHaveValue("what did I save on Tuesday");
    expect(onSend).not.toHaveBeenCalled();

    // The same words heard again are a new hand-off, not a stale one.
    rerender(composer({ text: "what did I save yesterday", seq: 2 }));
    expect(field).toHaveValue("what did I save yesterday");

    fireEvent.keyDown(field, { key: "Enter" });
    expect(onSend).toHaveBeenCalledWith("what did I save yesterday");
  });
});

describe("BotVoiceMic — stop abandons, and the surface honours the release", () => {
  it("calls voice_stop while listening and renders the idle snapshot that follows", async () => {
    seed({ state: LISTENING_HALF });
    const { rerender } = render(<Surface onHeard={() => {}} />);
    fireEvent.click(screen.getByRole("button", { name: VOICE_STOP_LISTENING_LABEL }));
    await waitFor(() => expect(voiceStop).toHaveBeenCalledTimes(1));
    expect(voiceStopSpeaking).not.toHaveBeenCalled();

    // What Rust streams after `Abandoned`: idle, microphone released.
    voiceStore.getState().applyState(IDLE);
    rerender(<Surface onHeard={() => {}} />);
    expect(screen.getByRole("button", { name: VOICE_TALK_LABEL })).toHaveAttribute(
      "aria-pressed",
      "false",
    );
    expect(screen.queryByRole("status")).toBeNull();
  });

  it("calls voice_stop_speaking, not voice_stop, while the answer is read aloud", async () => {
    seed({ state: SPEAKING });
    render(<Surface onHeard={() => {}} />);
    fireEvent.click(screen.getByRole("button", { name: VOICE_STOP_SPEAKING_LABEL }));
    await waitFor(() => expect(voiceStopSpeaking).toHaveBeenCalledTimes(1));
    expect(voiceStop).not.toHaveBeenCalled();
  });
});

describe("BotVoiceMic — asks by name, once, on the first press", () => {
  it("asks before it starts, and starts on a grant", async () => {
    const order: string[] = [];
    voiceAuthorize.mockImplementation(async () => {
      order.push("authorize");
      return null;
    });
    voiceStart.mockImplementation(async () => {
      order.push("start");
    });
    render(<Surface onHeard={() => {}} />);
    fireEvent.click(screen.getByRole("button", { name: VOICE_TALK_LABEL }));
    await waitFor(() => expect(order).toEqual(["authorize", "start"]));
  });

  it("renders a refusal as Rust's sentence with Open Settings, and starts nothing", async () => {
    voiceAuthorize.mockResolvedValue(NOT_AUTHORIZED);
    render(<Surface onHeard={() => {}} />);
    fireEvent.click(screen.getByRole("button", { name: VOICE_TALK_LABEL }));
    await waitFor(() =>
      expect(screen.getByRole("status")).toHaveTextContent(NOT_AUTHORIZED.message),
    );
    expect(voiceStart).not.toHaveBeenCalled();
    expect(voiceStore.getState().unavailable).toEqual(NOT_AUTHORIZED);

    fireEvent.click(screen.getByRole("button", { name: VOICE_OPEN_SETTINGS_LABEL }));
    expect(iosOpenAppSettings).toHaveBeenCalledTimes(1);
  });

  it("a grant lifts an earlier refusal but not a missing model", async () => {
    seed({ unavailable: NOT_AUTHORIZED });
    const { rerender } = render(<Surface onHeard={() => {}} />);
    fireEvent.click(screen.getByRole("button", { name: VOICE_TALK_LABEL }));
    await waitFor(() => expect(voiceStore.getState().unavailable).toBeNull());

    seed({ unavailable: NO_MODEL });
    rerender(<Surface onHeard={() => {}} />);
    fireEvent.click(screen.getByRole("button", { name: VOICE_TALK_LABEL }));
    await waitFor(() => expect(voiceStart).toHaveBeenCalledTimes(2));
    expect(voiceStore.getState().unavailable).toEqual(NO_MODEL);
    expect(screen.getByRole("status")).toHaveTextContent(NO_MODEL.message);
    expect(screen.queryByRole("button", { name: VOICE_OPEN_SETTINGS_LABEL })).toBeNull();
  });
});

describe("speakIfHeard", () => {
  it("reads the answer aloud from a turn that heard, and never from idle", () => {
    voiceStore.getState().applyState(HEARD);
    speakIfHeard("Three notes and a receipt.");
    expect(voiceSpeak).toHaveBeenCalledWith("Three notes and a receipt.");

    voiceStore.getState().applyState(IDLE);
    speakIfHeard("A typed question's answer.");
    expect(voiceSpeak).toHaveBeenCalledTimes(1);
  });
});

describe("BotVoiceMic — absence", () => {
  it("renders nothing without the bots capability", () => {
    seed({ bots: false });
    render(<Surface onHeard={() => {}} />);
    expect(screen.queryByRole("button")).toBeNull();
    expect(screen.queryByRole("status")).toBeNull();
  });

  it("renders nothing while availability is unanswered, and on unsupported", () => {
    seed({ unavailable: undefined });
    const { rerender } = render(<Surface onHeard={() => {}} />);
    expect(screen.queryByRole("button")).toBeNull();

    seed({ unavailable: UNSUPPORTED });
    rerender(<Surface onHeard={() => {}} />);
    expect(screen.queryByRole("button")).toBeNull();
    expect(screen.queryByRole("status")).toBeNull();
  });
});
