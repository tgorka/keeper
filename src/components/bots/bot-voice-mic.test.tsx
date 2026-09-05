/**
 * Talk mode you can see (Story 62.6, FR-407, FR-408, NFR-51, AD-170/AD-171;
 * Epic 67, AD-205).
 *
 * What is asserted here that nothing else asserts:
 *
 * 1. **Three states, told apart without colour** — the button's accessible
 *    name, its `data-state` and `aria-pressed` differ across idle, listening
 *    and speaking; the status line says which in words.
 * 2. **The button sends nothing and speaks nothing (AD-205)** — a turn's
 *    `heard` snapshot is shown as a status and nothing else happens in the
 *    webview: no `botsChatSend`, no `voice_speak` (which no longer exists as
 *    a binding). The send and the speak are Rust's, whether the turn was the
 *    button's or the phrase's — one path.
 * 3. **Stop abandons** — pressing while listening calls `voice_stop`, and the
 *    idle snapshot that follows is rendered idle: the surface honours the
 *    release rather than remembering a press.
 * 4. **The first press asks, by name, once** — `voice_authorize` precedes
 *    `voice_start`; a granted answer starts; a refusal starts nothing and
 *    renders Rust's sentence with Open Settings.
 * 5. **Absent** where `capabilities.bots` is off, while availability is
 *    unanswered, and on `unsupported`.
 */

import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import {
  BotVoiceMic,
  BotVoiceStatus,
  micState,
  VOICE_HEARD_STATUS,
  VOICE_LISTENING_STATUS,
  VOICE_OPEN_SETTINGS_LABEL,
  VOICE_SENDING_STATUS,
  VOICE_SPEAKING_STATUS,
  VOICE_STOP_LISTENING_LABEL,
  VOICE_STOP_SPEAKING_LABEL,
  VOICE_TALK_LABEL,
} from "@/components/bots/bot-voice-mic";
import type { VoiceStateVm, VoiceUnavailableVm } from "@/lib/ipc/client";
import * as client from "@/lib/ipc/client";
import { capabilitiesStore, DEFAULT_CAPABILITIES } from "@/lib/stores/capabilities";
import { voiceStore } from "@/lib/stores/voice";

const voiceAuthorize = vi.fn<() => Promise<VoiceUnavailableVm | null>>();
const voiceStart = vi.fn<() => Promise<void>>();
const voiceStop = vi.fn<() => Promise<void>>();
const voiceStopSpeaking = vi.fn<() => Promise<void>>();
const iosOpenAppSettings = vi.fn<() => Promise<void>>();
const botsChatSend = vi.fn<() => Promise<string>>();
vi.mock("@/lib/ipc/client", async (importOriginal) => {
  const actual = await importOriginal<typeof import("@/lib/ipc/client")>();
  return {
    ...actual,
    voiceAuthorize: () => voiceAuthorize(),
    voiceStart: () => voiceStart(),
    voiceStop: () => voiceStop(),
    voiceStopSpeaking: () => voiceStopSpeaking(),
    iosOpenAppSettings: () => iosOpenAppSettings(),
    botsChatSend: () => botsChatSend(),
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
const SENDING: VoiceStateVm = { kind: "sending", answering: false };
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
function Surface() {
  return (
    <>
      <BotVoiceStatus />
      <BotVoiceMic />
    </>
  );
}

beforeEach(() => {
  voiceAuthorize.mockReset();
  voiceStart.mockReset();
  voiceStop.mockReset();
  voiceStopSpeaking.mockReset();
  iosOpenAppSettings.mockReset();
  botsChatSend.mockReset();
  voiceAuthorize.mockResolvedValue(null);
  voiceStart.mockResolvedValue();
  voiceStop.mockResolvedValue();
  voiceStopSpeaking.mockResolvedValue();
  iosOpenAppSettings.mockResolvedValue();
  botsChatSend.mockResolvedValue("never");
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
    render(<Surface />);
    const button = screen.getByRole("button", { name: VOICE_TALK_LABEL });
    expect(button).toHaveAttribute("data-state", "idle");
    expect(button).toHaveAttribute("aria-pressed", "false");
    expect(screen.queryByRole("status")).toBeNull();
  });

  it("is Cancel this question while listening, pressed, and the band shows the transcript as it forms", () => {
    seed({ state: LISTENING });
    const { rerender } = render(<Surface />);
    const button = screen.getByRole("button", { name: VOICE_STOP_LISTENING_LABEL });
    expect(button).toHaveAttribute("data-state", "listening");
    expect(button).toHaveAttribute("aria-pressed", "true");
    const band = screen.getByRole("status");
    expect(band).toHaveAttribute("data-voice", "listening");
    expect(band).toHaveTextContent(VOICE_LISTENING_STATUS);

    voiceStore.getState().applyState(LISTENING_HALF);
    rerender(<Surface />);
    expect(screen.getByRole("status")).toHaveTextContent("what did I save");
  });

  it("is Stop this answer while the answer is read aloud, and says so", () => {
    seed({ state: SPEAKING });
    render(<Surface />);
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

describe("BotVoiceMic — the turn is Rust's from the phrase to the last word (AD-205)", () => {
  it("shows a button-started turn's hearing and sending as status, and sends nothing itself", async () => {
    const { rerender } = render(<Surface />);
    fireEvent.click(screen.getByRole("button", { name: VOICE_TALK_LABEL }));
    await waitFor(() => expect(voiceStart).toHaveBeenCalledTimes(1));

    // What Rust streams as it hears, sends and speaks — the same path as the
    // phrase's; the button only pressed `voice_start`.
    voiceStore.getState().applyState(LISTENING);
    rerender(<Surface />);
    voiceStore.getState().applyState(HEARD);
    rerender(<Surface />);
    expect(screen.getByRole("status")).toHaveTextContent(VOICE_HEARD_STATUS);
    voiceStore.getState().applyState(SENDING);
    rerender(<Surface />);
    expect(screen.getByRole("status")).toHaveTextContent(VOICE_SENDING_STATUS);
    voiceStore.getState().applyState(SPEAKING);
    rerender(<Surface />);
    expect(screen.getByRole("status")).toHaveTextContent(VOICE_SPEAKING_STATUS);

    expect(botsChatSend).not.toHaveBeenCalled();
    // The composer receives nothing: the field is not this control's to fill.
    expect(screen.queryByRole("textbox")).toBeNull();
  });

  it("shows a phrase-started turn the same way, with nothing pressed", () => {
    const { rerender } = render(<Surface />);
    voiceStore.getState().applyState(LISTENING);
    rerender(<Surface />);
    voiceStore.getState().applyState(HEARD);
    rerender(<Surface />);
    expect(screen.getByRole("status")).toHaveTextContent(VOICE_HEARD_STATUS);
    expect(voiceStart).not.toHaveBeenCalled();
    expect(botsChatSend).not.toHaveBeenCalled();
  });

  it("has no way to read an answer aloud from the webview", () => {
    // The `voice_speak` binding went with `speakIfHeard`: the answer is
    // spoken by Rust when the stream closes, screen or no screen.
    expect("voiceSpeak" in client).toBe(false);
  });
});

describe("BotVoiceMic — stop abandons, and the surface honours the release", () => {
  it("calls voice_stop while listening and renders the idle snapshot that follows", async () => {
    seed({ state: LISTENING_HALF });
    const { rerender } = render(<Surface />);
    fireEvent.click(screen.getByRole("button", { name: VOICE_STOP_LISTENING_LABEL }));
    await waitFor(() => expect(voiceStop).toHaveBeenCalledTimes(1));
    expect(voiceStopSpeaking).not.toHaveBeenCalled();

    // What Rust streams after `Abandoned`: idle, microphone released.
    voiceStore.getState().applyState(IDLE);
    rerender(<Surface />);
    expect(screen.getByRole("button", { name: VOICE_TALK_LABEL })).toHaveAttribute(
      "aria-pressed",
      "false",
    );
    expect(screen.queryByRole("status")).toBeNull();
  });

  it("calls voice_stop_speaking, not voice_stop, while the answer is read aloud", async () => {
    seed({ state: SPEAKING });
    render(<Surface />);
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
    render(<Surface />);
    fireEvent.click(screen.getByRole("button", { name: VOICE_TALK_LABEL }));
    await waitFor(() => expect(order).toEqual(["authorize", "start"]));
  });

  it("renders a refusal as Rust's sentence with Open Settings, and starts nothing", async () => {
    voiceAuthorize.mockResolvedValue(NOT_AUTHORIZED);
    render(<Surface />);
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
    const { rerender } = render(<Surface />);
    fireEvent.click(screen.getByRole("button", { name: VOICE_TALK_LABEL }));
    await waitFor(() => expect(voiceStore.getState().unavailable).toBeNull());

    seed({ unavailable: NO_MODEL });
    rerender(<Surface />);
    fireEvent.click(screen.getByRole("button", { name: VOICE_TALK_LABEL }));
    await waitFor(() => expect(voiceStart).toHaveBeenCalledTimes(2));
    expect(voiceStore.getState().unavailable).toEqual(NO_MODEL);
    expect(screen.getByRole("status")).toHaveTextContent(NO_MODEL.message);
    expect(screen.queryByRole("button", { name: VOICE_OPEN_SETTINGS_LABEL })).toBeNull();
  });
});

describe("BotVoiceMic — absence", () => {
  it("renders nothing without the bots capability", () => {
    seed({ bots: false });
    render(<Surface />);
    expect(screen.queryByRole("button")).toBeNull();
    expect(screen.queryByRole("status")).toBeNull();
  });

  it("renders nothing while availability is unanswered, and on unsupported", () => {
    seed({ unavailable: undefined });
    const { rerender } = render(<Surface />);
    expect(screen.queryByRole("button")).toBeNull();

    seed({ unavailable: UNSUPPORTED });
    rerender(<Surface />);
    expect(screen.queryByRole("button")).toBeNull();
    expect(screen.queryByRole("status")).toBeNull();
  });
});
