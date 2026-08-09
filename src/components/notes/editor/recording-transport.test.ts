import { beforeEach, describe, expect, it } from "vitest";
import {
  BACK_LABEL,
  BUFFERING_LABEL,
  clock,
  FORWARD_LABEL,
  MAX_DRIFT_SECONDS,
  MUTE_LABEL,
  PAUSE_LABEL,
  PLAY_LABEL,
  PLAY_REFUSED_LABEL,
  RecordingTransport,
  SCRUB_LABEL,
  SEEKING_LABEL,
  SKIP_SECONDS,
  TRANSPORT_LABEL,
  type TransportTrack,
  transportFor,
  UNMUTE_LABEL,
  VOLUME_LABEL,
} from "./recording-transport";

/**
 * A media element that can do the things jsdom's cannot.
 *
 * jsdom implements no playback whatsoever: `play()` logs "not implemented" and
 * returns `undefined`, `readyState` never leaves 0, `duration` is `NaN`, and
 * `timeupdate`, `waiting`, `stalled` and `seeked` never fire. Every behaviour
 * this transport exists for — a stall, a refusal, a seek that half-lands, two
 * clocks drifting apart — is therefore unreachable through a real `<video>` on
 * any machine in CI, and the only honest option is to drive a track that can
 * produce them and to say so. What this fake does NOT model is decoding: it
 * cannot tell us that a real pair of `.mov` files stays in step, only that the
 * transport reacts correctly to the events a pair produces.
 */
class FakeTrack extends EventTarget implements TransportTrack {
  duration = Number.NaN;
  controls = true;
  volume = 1;
  muted = false;
  paused = true;
  /** What the next `play()` does. An autoplay policy is an `Error`. */
  refusal: Error | null = null;
  /** Whether a seek confirms. A track that never answers is the half-landed
   *  seek the transport must keep visible. */
  confirms = true;
  /** Whether the element takes the assignment at all. A detached or errored
   *  element swallows it and raises no `seeking`, so the transport learns
   *  nothing unless it recorded the request before making it. */
  accepts = true;
  playCalls = 0;
  pauseCalls = 0;
  #time = 0;

  get currentTime(): number {
    return this.#time;
  }

  set currentTime(value: number) {
    if (!this.accepts) {
      return;
    }
    this.#time = value;
    this.dispatchEvent(new Event("seeking"));
    if (this.confirms) {
      this.dispatchEvent(new Event("seeked"));
    }
  }

  async play(): Promise<void> {
    this.playCalls += 1;
    if (this.refusal !== null) {
      throw this.refusal;
    }
    this.paused = false;
    this.dispatchEvent(new Event("playing"));
  }

  pause(): void {
    this.pauseCalls += 1;
    this.paused = true;
    this.dispatchEvent(new Event("pause"));
  }

  /** Playback advancing: the clock moved and the element said so. */
  advanceTo(value: number): void {
    this.#time = value;
    this.dispatchEvent(new Event("timeupdate"));
  }

  /** The clock moved and the element said NOTHING — which is exactly how two
   *  tracks drift apart without either one noticing. */
  driftTo(value: number): void {
    this.#time = value;
  }
}

/** Hosts in document order, as two `![[…mov]]` embeds sit in a note. */
function stage(count: number): { root: HTMLElement; hosts: HTMLElement[] } {
  const root = document.createElement("div");
  document.body.append(root);
  const hosts = Array.from({ length: count }, () => {
    const host = document.createElement("span");
    host.className = "cm-lp-recording";
    root.append(host);
    return host;
  });
  return { root, hosts };
}

const NAMES = ["screen-0000.mov", "camera-0000.mov", "slides-0000.mov"];

interface Pair {
  transport: RecordingTransport;
  tracks: FakeTrack[];
  hosts: HTMLElement[];
  root: HTMLElement;
}

/** `count` tracks of one session, each mounted in its own host, joined in
 *  document order. Durations differ, as two views of one moment really do. */
function joined(count: number): Pair {
  const { root, hosts } = stage(count);
  const transport = new RecordingTransport();
  const tracks = hosts.map((host, index) => {
    const track = new FakeTrack();
    track.duration = 120 - index;
    transport.join(track, host, NAMES[index]);
    return track;
  });
  return { transport, tracks, hosts, root };
}

function bar(pair: Pair): HTMLElement {
  const found = pair.root.querySelector<HTMLElement>(".cm-lp-recording-transport");
  if (found === null) {
    throw new Error("no transport bar is mounted");
  }
  return found;
}

function press(within: HTMLElement, label: string): void {
  const button = within.querySelector<HTMLButtonElement>(`button[aria-label="${label}"]`);
  if (button === null) {
    throw new Error(`no control labelled ${label}`);
  }
  button.click();
}

function slider(within: HTMLElement, label: string): HTMLInputElement {
  const input = within.querySelector<HTMLInputElement>(`input[aria-label="${label}"]`);
  if (input === null) {
    throw new Error(`no slider labelled ${label}`);
  }
  return input;
}

/** Drag a range input the way a reader does: a value and an `input` event. */
function drag(input: HTMLInputElement, value: number): void {
  input.value = String(value);
  input.dispatchEvent(new Event("input"));
}

function statusOf(pair: Pair): string {
  return bar(pair).querySelector(".cm-lp-recording-transport-status")?.textContent ?? "";
}

beforeEach(() => {
  document.body.replaceChildren();
});

describe("engagement", () => {
  it("leaves a lone video its own native controls and builds no transport", () => {
    const pair = joined(1);

    // The whole of the single-video decision: nothing to keep in step, so
    // nothing is taken away. A hand-built bar for one track would be a worse
    // `<video controls>` — no fullscreen, no picture-in-picture, no captions.
    expect(pair.tracks[0].controls).toBe(true);
    expect(pair.root.querySelector(".cm-lp-recording-transport")).toBeNull();
    expect(pair.root.querySelector(".cm-lp-recording-mix")).toBeNull();
    expect(pair.transport.state.trackCount).toBe(1);
  });

  it("takes both native transports away the moment a second track of the session mounts", () => {
    const pair = joined(2);

    // A native transport carries its own scrub bar, and a second scrub bar is
    // a second clock — the thing this whole module exists to remove.
    expect(pair.tracks.map((track) => track.controls)).toEqual([false, false]);
    expect(pair.root.querySelectorAll(".cm-lp-recording-transport")).toHaveLength(1);
    // Under the FIRST embed in the note, because that is where a reader looks
    // for the clock of the thing they are watching.
    expect(bar(pair).parentElement).toBe(pair.hosts[0]);
    expect(bar(pair).getAttribute("aria-label")).toBe(TRANSPORT_LABEL);
    // One mixer per track, beside its own video — never on the shared bar.
    expect(pair.root.querySelectorAll(".cm-lp-recording-mix")).toHaveLength(2);
  });

  it("leads with the first embed in the note however the two resolve", () => {
    const { root, hosts } = stage(2);
    const transport = new RecordingTransport();
    const second = new FakeTrack();
    const first = new FakeTrack();

    // The lower embed's IPC round trip answered first. Join order would hand it
    // the clock and the bar; document order does not.
    transport.join(second, hosts[1], NAMES[1]);
    transport.join(first, hosts[0], NAMES[0]);

    expect(root.querySelector(".cm-lp-recording-transport")?.parentElement).toBe(hosts[0]);

    first.duration = 60;
    second.duration = 60;
    first.advanceTo(12);
    // The reference is the leading track: its clock is the readout.
    expect(transport.state.position).toBe(12);
  });

  it("hands the bar down when the leading track scrolls away, and keeps the position", () => {
    const pair = joined(3);
    pair.tracks[0].advanceTo(42);

    pair.transport.leave(pair.tracks[0]);

    expect(bar(pair).parentElement).toBe(pair.hosts[1]);
    // The bar is MOVED, not rebuilt: a reader who scrolled the top video out of
    // view has not asked to lose their place.
    expect(pair.transport.state.position).toBe(42);
    expect(pair.transport.state.trackCount).toBe(2);
    expect(pair.root.querySelectorAll(".cm-lp-recording-transport")).toHaveLength(1);
  });

  it("gives the native controls back when the pair falls to one track", () => {
    const pair = joined(2);

    pair.transport.leave(pair.tracks[0]);

    expect(pair.tracks[1].controls).toBe(true);
    expect(pair.root.querySelector(".cm-lp-recording-transport")).toBeNull();
    expect(pair.root.querySelector(".cm-lp-recording-mix")).toBeNull();
  });

  it("spans the LONGEST track, so the last second of the recording is reachable", () => {
    const pair = joined(2);

    // Two views of one moment stop when their own writer stopped; the camera
    // track is routinely a second short. Taking the minimum would put the end
    // of the screen recording out of the scrub bar's reach.
    expect(pair.transport.state.duration).toBe(120);
    expect(slider(bar(pair), SCRUB_LABEL).max).toBe("120");
  });

  it("disables the scrub bar until the pair has a span, rather than leaving it inert", () => {
    const { root, hosts } = stage(2);
    const transport = new RecordingTransport();
    const tracks = hosts.map((host, index) => {
      const track = new FakeTrack();
      transport.join(track, host, NAMES[index]);
      return track;
    });
    const scrubbed = root.querySelector<HTMLInputElement>(
      `input[aria-label="${SCRUB_LABEL}"]`,
    ) as HTMLInputElement;

    // `preload="metadata"` on a removable volume may take a while, or never
    // arrive. A range input with `max="0"` silently clamps every value the
    // reader drags to back to zero: it looks live and moves nothing.
    expect(scrubbed.disabled).toBe(true);

    for (const track of tracks) {
      track.duration = 90;
      track.dispatchEvent(new Event("durationchange"));
    }

    expect(scrubbed.disabled).toBe(false);
    expect(scrubbed.max).toBe("90");

    drag(scrubbed, 45);
    expect(tracks.map((track) => track.currentTime)).toEqual([45, 45]);
  });
});

describe("one clock", () => {
  it("plays and pauses both tracks from one button", async () => {
    const pair = joined(2);

    press(bar(pair), PLAY_LABEL);
    await Promise.resolve();
    await Promise.resolve();

    expect(pair.tracks.map((track) => track.playCalls)).toEqual([1, 1]);
    expect(pair.transport.state.playback).toBe("playing");
    expect(bar(pair).querySelector(".cm-lp-recording-transport-toggle")?.textContent).toBe(
      PAUSE_LABEL,
    );

    press(bar(pair), PAUSE_LABEL);

    expect(pair.tracks.map((track) => track.pauseCalls)).toEqual([1, 1]);
    expect(pair.transport.state.playback).toBe("paused");
    expect(bar(pair).querySelector(".cm-lp-recording-transport-toggle")?.textContent).toBe(
      PLAY_LABEL,
    );
  });

  it("scrubs both tracks to the same second", () => {
    const pair = joined(2);

    drag(slider(bar(pair), SCRUB_LABEL), 37.5);

    expect(pair.tracks.map((track) => track.currentTime)).toEqual([37.5, 37.5]);
    expect(pair.transport.state.position).toBe(37.5);
  });

  it("moves both tracks by ten seconds, from the shared position and not from either clock", () => {
    const pair = joined(2);
    pair.transport.seekTo(30);
    // One track has quietly fallen behind. `+10s` is ten seconds from the
    // TRANSPORT's position, so the skip is also a resynchronisation.
    pair.tracks[1].driftTo(28);

    press(bar(pair), FORWARD_LABEL);

    expect(pair.tracks.map((track) => track.currentTime)).toEqual([
      30 + SKIP_SECONDS,
      30 + SKIP_SECONDS,
    ]);

    press(bar(pair), BACK_LABEL);

    expect(pair.tracks.map((track) => track.currentTime)).toEqual([30, 30]);
  });

  it("clamps a skip to the recording rather than seeking off either end", () => {
    const pair = joined(2);
    pair.transport.seekTo(5);

    press(bar(pair), BACK_LABEL);

    expect(pair.transport.state.position).toBe(0);
    expect(pair.tracks.map((track) => track.currentTime)).toEqual([0, 0]);

    pair.transport.seekTo(119);
    press(bar(pair), FORWARD_LABEL);

    expect(pair.transport.state.position).toBe(120);
  });

  it("is one clock for three angles as much as for two", async () => {
    const pair = joined(3);

    expect(pair.root.querySelectorAll(".cm-lp-recording-transport")).toHaveLength(1);
    expect(pair.tracks.map((track) => track.controls)).toEqual([false, false, false]);

    press(bar(pair), PLAY_LABEL);
    await Promise.resolve();
    await Promise.resolve();
    drag(slider(bar(pair), SCRUB_LABEL), 12);

    expect(pair.tracks.map((track) => track.currentTime)).toEqual([12, 12, 12]);

    // The fold is `some` for the bad news, whatever the arity: one of three
    // stalling makes the trio buffering.
    pair.tracks[2].dispatchEvent(new Event("waiting"));

    expect(pair.transport.state.playback).toBe("buffering");
  });
});

describe("volume and mute stay per track", () => {
  it("changes one track's volume and leaves the other's alone", () => {
    const pair = joined(2);
    const mixers = pair.root.querySelectorAll<HTMLElement>(".cm-lp-recording-mix");

    drag(slider(mixers[0], `${VOLUME_LABEL} ${NAMES[0]}`), 0.25);

    // Mixing, not timing: turning the screen recording down under the camera is
    // the one thing a reader of a two-view recording actually needs (UX-DR53).
    expect(pair.tracks[0].volume).toBe(0.25);
    expect(pair.tracks[1].volume).toBe(1);
    // And it moved no clock.
    expect(pair.tracks.map((track) => track.currentTime)).toEqual([0, 0]);
  });

  it("mutes one track and leaves the other audible", () => {
    const pair = joined(2);
    const mixers = pair.root.querySelectorAll<HTMLElement>(".cm-lp-recording-mix");

    press(mixers[1], `${MUTE_LABEL} ${NAMES[1]}`);

    expect(pair.tracks[1].muted).toBe(true);
    expect(pair.tracks[0].muted).toBe(false);
    // The control now offers the way back, and says which track it is: two
    // identical "Mute" buttons are one control said twice to a screen reader.
    press(mixers[1], `${UNMUTE_LABEL} ${NAMES[1]}`);
    expect(pair.tracks[1].muted).toBe(false);
  });

  it("gives every track its own named pair of controls", () => {
    const pair = joined(2);

    for (const name of [NAMES[0], NAMES[1]]) {
      expect(pair.root.querySelector(`[aria-label="${MUTE_LABEL} ${name}"]`)).not.toBeNull();
      expect(pair.root.querySelector(`[aria-label="${VOLUME_LABEL} ${name}"]`)).not.toBeNull();
    }
  });
});

describe("the truth about a pair", () => {
  it("reports the pair as buffering while one track is stalled, never as playing", async () => {
    const pair = joined(2);
    press(bar(pair), PLAY_LABEL);
    await Promise.resolve();
    await Promise.resolve();
    expect(pair.transport.state.playback).toBe("playing");

    pair.tracks[1].dispatchEvent(new Event("waiting"));

    // The other track is decoding happily. Asking it — or asking whichever
    // element the transport touched last — would answer "playing", and the
    // reader would be told the recording is running while half of it is frozen.
    expect(pair.tracks[0].paused).toBe(false);
    expect(pair.transport.state.playback).toBe("buffering");
    expect(statusOf(pair)).toBe(BUFFERING_LABEL);
    // The intent is still to play, so the button under the reader's finger is
    // still the one that stops it.
    expect(bar(pair).querySelector(".cm-lp-recording-transport-toggle")?.textContent).toBe(
      PAUSE_LABEL,
    );

    pair.tracks[1].dispatchEvent(new Event("playing"));

    expect(pair.transport.state.playback).toBe("playing");
    expect(statusOf(pair)).toBe("");
  });

  it("does not claim to be playing when one track's play() was refused", async () => {
    const pair = joined(2);
    pair.tracks[1].refusal = new Error("NotAllowedError: play() failed");

    press(bar(pair), PLAY_LABEL);
    await Promise.resolve();
    await Promise.resolve();
    await Promise.resolve();

    // A pair where one play() rejected is not playing, and the other track must
    // not be left running alone: it would be a solo view of a two-view moment,
    // drifting further from its partner every second.
    expect(pair.transport.state.playback).toBe("paused");
    expect(pair.tracks[0].paused).toBe(true);
    expect(pair.tracks[0].pauseCalls).toBe(1);
    expect(pair.transport.state.failure).toContain("NotAllowedError");
    expect(statusOf(pair)).toBe(PLAY_REFUSED_LABEL);
    // And the button says Play, so the reader's next click starts it rather
    // than "stopping" something that never began.
    expect(bar(pair).querySelector(".cm-lp-recording-transport-toggle")?.textContent).toBe(
      PLAY_LABEL,
    );
  });

  it("does not resurrect a refusal the reader already overruled by pausing", async () => {
    const pair = joined(2);
    pair.tracks[1].refusal = new Error("NotAllowedError: play() failed");

    const playing = pair.transport.play();
    // The reader changed their mind while the promises were in flight. Telling
    // them a play they abandoned was refused is noise about a dead question.
    pair.transport.pause();
    await playing;

    expect(pair.transport.state.playback).toBe("paused");
    expect(pair.transport.state.failure).toBeNull();
  });

  it("still reports a refusal that lands after a scrub, because it is still true", async () => {
    const pair = joined(2);
    pair.tracks[1].refusal = new Error("NotAllowedError: play() failed");

    const playing = pair.transport.play();
    // Scrubbing does not withdraw "play" — the reader still wants the pair
    // running, and one track refusing is news whichever second they land on.
    pair.transport.seekTo(64);
    await playing;

    expect(pair.transport.state.position).toBe(64);
    expect(pair.transport.state.playback).toBe("paused");
    expect(pair.transport.state.failure).toContain("NotAllowedError");
  });

  it("keeps a seek only one track confirmed visible instead of settling on it", () => {
    const pair = joined(2);
    pair.tracks[1].confirms = false;

    pair.transport.seekTo(45);

    // Half the pair is at 0:45 and half is wherever it was. The transport says
    // so rather than drawing a settled bar over a desynchronised pair.
    expect(pair.transport.state.seeking).toBe(true);
    expect(statusOf(pair)).toBe(SEEKING_LABEL);

    pair.tracks[1].dispatchEvent(new Event("seeked"));

    expect(pair.transport.state.seeking).toBe(false);
    expect(statusOf(pair)).toBe("");
  });

  it("keeps a seek a track swallowed without a word visible too", () => {
    const pair = joined(2);
    // Not "asked and did not answer" — asked and did not so much as raise
    // `seeking`. An element that fell off its volume does this, and the only
    // way to know is to have written the request down before making it.
    pair.tracks[1].accepts = false;

    pair.transport.seekTo(45);

    expect(pair.tracks[0].currentTime).toBe(45);
    expect(pair.tracks[1].currentTime).toBe(0);
    expect(pair.transport.state.seeking).toBe(true);
    expect(statusOf(pair)).toBe(SEEKING_LABEL);
  });

  it("does not drag the scrub thumb backwards while a seek is unconfirmed", () => {
    const pair = joined(2);
    pair.tracks[0].confirms = false;

    pair.transport.seekTo(90);
    // The leading element is still reporting where it WAS. Believing it would
    // yank the thumb out from under the reader who just moved it.
    pair.tracks[0].advanceTo(3);

    expect(pair.transport.state.position).toBe(90);
  });

  it("stops the pair when one track pauses itself out from under it", async () => {
    const pair = joined(2);
    press(bar(pair), PLAY_LABEL);
    await Promise.resolve();
    await Promise.resolve();

    pair.tracks[1].pause();

    // Letting the other run on is the silent desynchronisation this transport
    // exists to prevent: a paused pair is recoverable, a ten-second gap is not.
    expect(pair.transport.state.playback).toBe("paused");
    expect(pair.tracks[0].paused).toBe(true);
  });

  it("lets the shorter track end without stopping the longer one", () => {
    const pair = joined(2);
    void pair.transport.play();

    pair.tracks[1].dispatchEvent(new Event("ended"));

    // The camera stopped a second early; the screen recording has a second to
    // go, and `span` made that second reachable.
    expect(pair.transport.state.playback).not.toBe("paused");

    pair.tracks[0].dispatchEvent(new Event("ended"));

    expect(pair.transport.state.playback).toBe("paused");
  });

  it("stops waiting on a track that errored rather than saying Seeking forever", async () => {
    const pair = joined(2);
    press(bar(pair), PLAY_LABEL);
    await Promise.resolve();
    await Promise.resolve();
    pair.tracks[1].confirms = false;
    pair.transport.seekTo(20);
    expect(pair.transport.state.seeking).toBe(true);

    pair.tracks[1].dispatchEvent(new Event("error"));

    // It will never confirm, and holding the bar on "Seeking" for a track that
    // is gone would be a promise the transport cannot keep. The pair is not
    // seeking, it is broken — and those are different things to say.
    expect(pair.transport.state.seeking).toBe(false);
    expect(pair.transport.state.playback).toBe("buffering");
  });

  it("stops waiting on a track that left mid-seek", () => {
    const pair = joined(3);
    pair.tracks[2].confirms = false;
    pair.transport.seekTo(20);

    pair.transport.leave(pair.tracks[2]);

    expect(pair.transport.state.seeking).toBe(false);
  });
});

describe("drift", () => {
  it("names half a second, and neither more nor less", () => {
    // Asserted as a literal, and every test below uses literal offsets, because
    // a threshold expressed as `MAX_DRIFT_SECONDS ± something` moves whenever
    // the constant moves and defends nothing. The number has two real bounds:
    // `timeupdate` fires as seldom as four times a second, so anything under a
    // quarter second is indistinguishable from sampling jitter and would
    // re-seek forever against its own noise; and two views of one moment
    // visibly disagree well before a whole second.
    expect(MAX_DRIFT_SECONDS).toBe(0.5);
  });

  it("pulls a follower that ran 0.8s ahead back to the scrub position", () => {
    const pair = joined(2);
    pair.transport.seekTo(30);
    pair.tracks[1].driftTo(31.8);

    pair.tracks[0].advanceTo(31);

    // Toward the scrub position — what the bar shows and what the reader asked
    // for — not toward the follower's own idea of where it is.
    expect(pair.transport.state.position).toBe(31);
    expect(pair.tracks[1].currentTime).toBe(31);
  });

  it("pulls a follower that fell 0.9s behind up to the scrub position", () => {
    const pair = joined(2);
    pair.transport.seekTo(30);
    pair.tracks[1].driftTo(29.1);

    pair.tracks[0].advanceTo(30);

    expect(pair.tracks[1].currentTime).toBe(30);
  });

  it("leaves a follower 0.45s out alone, so sampling jitter costs no seek", () => {
    const pair = joined(2);
    pair.transport.seekTo(30);
    pair.tracks[1].driftTo(30.45);

    pair.tracks[0].advanceTo(30);

    // Two tracks sampled at different moments read up to a quarter second apart
    // while being perfectly in step. Correcting that would be a real seek paid
    // for a measurement error — and on a screen recording with sparse
    // keyframes, a seek can cost longer than the drift it repaired.
    expect(pair.tracks[1].currentTime).toBe(30.45);
  });

  it("shows the correction as a seek, because that is what it is", () => {
    const pair = joined(2);
    pair.tracks[1].confirms = false;
    pair.transport.seekTo(30);
    pair.tracks[1].dispatchEvent(new Event("seeked"));
    pair.tracks[1].driftTo(40);

    pair.tracks[0].advanceTo(30.5);

    // The follower was asked to jump ten seconds and has not answered. That is
    // a pair mid-repair, and the reader is told so.
    expect(pair.transport.state.seeking).toBe(true);
  });

  it("does not seek a finished track back toward a position it can never hold", () => {
    const pair = joined(2);
    pair.tracks[1].duration = 100;
    pair.transport.seekTo(99);
    pair.tracks[1].dispatchEvent(new Event("ended"));
    pair.tracks[1].driftTo(100);

    pair.tracks[0].advanceTo(110);

    // The shorter track ran out at 1:40 and the longer one has twenty seconds
    // to go. Correcting the ended track every `timeupdate` would seek it in a
    // loop it cannot win.
    expect(pair.tracks[1].currentTime).toBe(100);
  });
});

describe("the readout", () => {
  it("says minutes and seconds, and says so about a duration nothing has reported", () => {
    expect(clock(0)).toBe("0:00");
    expect(clock(9)).toBe("0:09");
    expect(clock(63.7)).toBe("1:03");
    expect(clock(3723)).toBe("1:02:03");
    expect(clock(Number.NaN)).toBe("--:--");
  });

  it("shows the shared position over the pair's span", () => {
    const pair = joined(2);

    pair.transport.seekTo(63);

    expect(bar(pair).querySelector(".cm-lp-recording-time")?.textContent).toBe("1:03 / 2:00");
  });
});

describe("transportFor", () => {
  it("gives one session one transport, and two editors two", () => {
    const one = {};
    const other = {};

    expect(transportFor(one, "session-a")).toBe(transportFor(one, "session-a"));
    expect(transportFor(one, "session-a")).not.toBe(transportFor(one, "session-b"));
    // Two editors open on one note are two readers, and one of them pressing
    // play must not move the other one's video.
    expect(transportFor(one, "session-a")).not.toBe(transportFor(other, "session-a"));
  });
});
