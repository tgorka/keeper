/**
 * What the voice pill draws for each snapshot (Story 64.4, FR-436–FR-439,
 * AD-185).
 *
 * Per state, because the pill is the one surface a person sees while keeper
 * is behind another app, and a state it renders wrongly is a turn they read
 * as stalled. Reduced motion is asserted as an attribute the fill carries
 * rather than a computed style: jsdom applies no CSS, so the `transition`
 * class's presence is the whole of what can be checked here, and the
 * `data-motion` mark is what a screenshot's DOM probe reads on the Mac.
 */
import { render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import type { VoiceStateVm } from "@/lib/ipc/client";
import {
  PILL_ANSWERING,
  PILL_HEARD,
  PILL_LISTENING,
  PILL_SPEAKING,
  PILL_THINKING,
  pillArmedLine,
  VoicePill,
} from "./voice-pill";

const originalMatchMedia = window.matchMedia;

function preferReducedMotion(reduced: boolean) {
  window.matchMedia = vi.fn().mockImplementation((query: string) => ({
    matches: query.includes("prefers-reduced-motion") ? reduced : false,
    media: query,
    onchange: null,
    addListener: vi.fn(),
    removeListener: vi.fn(),
    addEventListener: vi.fn(),
    removeEventListener: vi.fn(),
    dispatchEvent: vi.fn(),
  }));
}

afterEach(() => {
  window.matchMedia = originalMatchMedia;
});

function lamp(): string | null {
  return document.querySelector('[data-slot="lamp"]')?.getAttribute("data-state") ?? null;
}

function fill(): HTMLElement | null {
  return document.querySelector('[data-slot="voice-pill-fill"]');
}

describe("VoicePill per state", () => {
  it("says Listening with a live lamp and the words so far", () => {
    const state: VoiceStateVm = { kind: "listening", heard: "what did I", level: 0.4 };
    render(<VoicePill state={state} />);
    expect(screen.getByRole("status")).toHaveTextContent(PILL_LISTENING);
    expect(screen.getByRole("status")).toHaveAttribute("aria-live", "polite");
    expect(lamp()).toBe("live");
    expect(screen.getByText("what did I")).toBeInTheDocument();
    expect(fill()).toHaveStyle({ width: "40%" });
  });

  it("says Heard with the final text and a working lamp", () => {
    const state: VoiceStateVm = { kind: "heard", text: "what did I save", level: 0.1 };
    render(<VoicePill state={state} />);
    expect(screen.getByRole("status")).toHaveTextContent(PILL_HEARD);
    expect(lamp()).toBe("working");
    expect(screen.getByText("what did I save")).toBeInTheDocument();
    expect(fill()).toHaveStyle({ width: "10%" });
  });

  it("says Thinking before the first token and Answering after it, with no meter", () => {
    const thinking = render(<VoicePill state={{ kind: "sending", answering: false }} />);
    expect(screen.getByRole("status")).toHaveTextContent(PILL_THINKING);
    expect(lamp()).toBe("working");
    // The microphone is released: no level, so no bar at all (AD-27), never
    // an empty track that reads as silence.
    expect(fill()).toBeNull();
    thinking.unmount();
    render(<VoicePill state={{ kind: "sending", answering: true }} />);
    expect(screen.getByRole("status")).toHaveTextContent(PILL_ANSWERING);
  });

  it("says Speaking with a live lamp", () => {
    render(<VoicePill state={{ kind: "speaking" }} />);
    expect(screen.getByRole("status")).toHaveTextContent(PILL_SPEAKING);
    expect(lamp()).toBe("live");
    expect(fill()).toBeNull();
  });

  it("shows the failure sentence itself with a fault lamp", () => {
    const reason = "The microphone is in use by another app.";
    render(<VoicePill state={{ kind: "failed", reason }} />);
    expect(screen.getByRole("status")).toHaveTextContent(reason);
    expect(screen.getByRole("status")).toHaveClass("text-destructive");
    expect(lamp()).toBe("fault");
  });

  it("names the phrase during the armed glance", () => {
    render(<VoicePill state={{ kind: "idle", wake: "nixie", listeningForWake: true }} />);
    expect(screen.getByRole("status")).toHaveTextContent(pillArmedLine("nixie"));
    expect(screen.getByRole("status")).toHaveTextContent("\u201Cnixie\u201D");
    expect(lamp()).toBe("idle");
  });

  it("draws nothing to say when idle and unarmed, and before any snapshot", () => {
    const idle = render(
      <VoicePill state={{ kind: "idle", wake: null, listeningForWake: false }} />,
    );
    expect(screen.getByRole("status")).toBeEmptyDOMElement();
    expect(fill()).toBeNull();
    idle.unmount();
    render(<VoicePill state={null} />);
    expect(screen.getByRole("status")).toBeEmptyDOMElement();
    expect(document.querySelector('[data-voice-pill="none"]')).not.toBeNull();
  });

  it("draws no meter while the port has not measured a level", () => {
    // A `listening` snapshot before the first buffer, or on a port with no
    // meter: `null` is absence, not zero.
    render(<VoicePill state={{ kind: "listening", heard: "", level: null }} />);
    expect(fill()).toBeNull();
    expect(document.querySelector('[data-slot="voice-pill-level"]')).toBeNull();
  });
});

describe("the words line", () => {
  it("ellipsises at the start so the newest words stay visible", () => {
    render(<VoicePill state={{ kind: "listening", heard: "the words so far", level: 0.5 }} />);
    const line = document.querySelector('[data-slot="voice-pill-words"]');
    expect(line).not.toBeNull();
    // `dir="rtl"` puts the overflow — and the ellipsis — at the line's start;
    // `<bdi>` keeps the words themselves left-to-right, punctuation included.
    expect(line).toHaveAttribute("dir", "rtl");
    expect(line).toHaveClass("truncate");
    expect(line?.querySelector("bdi")).toHaveTextContent("the words so far");
  });

  it("is absent while there are no words", () => {
    render(<VoicePill state={{ kind: "listening", heard: "", level: 0.5 }} />);
    expect(document.querySelector('[data-slot="voice-pill-words"]')).toBeNull();
  });
});

describe("the level under reduced motion", () => {
  it("is a static fill: the width is set and nothing transitions", () => {
    preferReducedMotion(true);
    render(<VoicePill state={{ kind: "listening", heard: "", level: 0.75 }} />);
    const bar = fill();
    expect(bar).toHaveAttribute("data-motion", "static");
    expect(bar).toHaveStyle({ width: "75%" });
    expect(bar?.className).not.toMatch(/transition/);
  });

  it("eases the fill otherwise", () => {
    preferReducedMotion(false);
    render(<VoicePill state={{ kind: "listening", heard: "", level: 0.75 }} />);
    const bar = fill();
    expect(bar).toHaveAttribute("data-motion", "eased");
    expect(bar).toHaveClass("transition-[width]");
  });

  it("clamps a reading outside 0..1 rather than drawing past the track", () => {
    render(<VoicePill state={{ kind: "listening", heard: "", level: 1.4 }} />);
    expect(fill()).toHaveStyle({ width: "100%" });
  });
});
