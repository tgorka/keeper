/**
 * The URL a session's file is played over, and Story 44.1's first-frame
 * policy — facts about a media element every player shares.
 */
import { describe, expect, it } from "vitest";
import {
  FRAME_PRIME_SECONDS,
  primeFirstFrame,
  RECORDING_ASSET_SCHEME,
  recordingAssetUrl,
} from "./media-playback";

const SESSION = "01KYH5DXGP1XQRHTME8CJFVEJ6-01KZHS7EJB5QKR8T9CHXQ46RNS";
const FOUND = "recordings/2026/2026-08-08 15.52 pricing call";

describe("recordingAssetUrl", () => {
  it("escapes each segment and keeps the separators, so a space cannot end the path", () => {
    expect(recordingAssetUrl(SESSION, `${FOUND}/screen-0000.mov`)).toBe(
      `${RECORDING_ASSET_SCHEME}://${SESSION}/recordings/2026/` +
        "2026-08-08%2015.52%20pricing%20call/screen-0000.mov",
    );
  });
});

/**
 * Story 44.1's defect and its price.
 *
 * **What jsdom can say and what it cannot.** jsdom implements no media
 * playback: `readyState` is 0 forever, no frame is ever decoded, and a canvas
 * readback of a `<video>` here would be meaningless. So nothing below proves a
 * frame appeared — that was measured in a real WKWebView, on the owner's own
 * two-track session, and the pixel counts are in the spec. What these assert
 * is the POLICY: who gets asked for a frame, who is left alone, and how often.
 * Getting that wrong is a control that moves the recording under the reader's
 * hand, which is the more dangerous half.
 */
describe("primeFirstFrame", () => {
  /** A `<video>` with a settable `readyState`, which jsdom's is not. */
  function video(readyState: number): HTMLVideoElement {
    const element = document.createElement("video");
    Object.defineProperty(element, "readyState", { configurable: true, value: readyState });
    return element;
  }

  it("buys a frame for an element that has metadata and nothing to show", () => {
    const player = video(1);
    primeFirstFrame(player);

    player.dispatchEvent(new Event("loadedmetadata"));

    expect(player.currentTime).toBe(FRAME_PRIME_SECONDS);
  });

  it("asks before the metadata arrives and not a moment sooner", () => {
    const player = video(0);
    primeFirstFrame(player);

    // A seek issued at HAVE_NOTHING is recorded as a default start position and
    // raises nothing — the element would never confirm it and the transport
    // would sit on Seeking for a request no one can answer.
    expect(player.currentTime).toBe(0);
  });

  it("leaves an element the reader or the transport already moved exactly where it is", () => {
    const player = video(1);
    primeFirstFrame(player);
    // By the time metadata lands, the pair has been placed at 37.5 s — either
    // by a scrub or by a transport seating a late-joining track.
    player.currentTime = 37.5;

    player.dispatchEvent(new Event("loadedmetadata"));

    expect(player.currentTime).toBe(37.5);
  });

  it("buys nothing for an element that already has a frame", () => {
    // HAVE_CURRENT_DATA: there is a frame, so the range request would be spent
    // for nothing on a file that may be on a pendrive.
    const player = video(2);
    primeFirstFrame(player);

    player.dispatchEvent(new Event("loadedmetadata"));

    expect(player.currentTime).toBe(0);
  });

  it("buys one frame and not one per event, even for a reader back at the top", () => {
    const player = video(1);
    primeFirstFrame(player);
    player.dispatchEvent(new Event("loadedmetadata"));
    expect(player.currentTime).toBe(FRAME_PRIME_SECONDS);

    // Back to exactly zero — a reader who scrubbed to the start. The "has it
    // been moved" guard cannot tell this apart from an untouched element, so
    // the only thing stopping a second `loadedmetadata` (a source change, a
    // reload) from moving them again is that the prime is genuinely once.
    player.currentTime = 0;
    player.dispatchEvent(new Event("loadedmetadata"));

    expect(player.currentTime).toBe(0);
  });
});
