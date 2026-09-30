/**
 * What every surface that plays a recording shares: the transport's words and
 * glyphs, the clock, the drift threshold, the URL a session's file is served
 * over, and the two facts about an `HTMLMediaElement` that a player must honour
 * — show a first frame, and let go of the resource when it goes away.
 *
 * Consumers: the transcript player (the viewer's and the note's media block),
 * the gallery block, and the Files panel's media viewer. This module imports
 * NOTHING, which is what lets a React surface and a CodeMirror widget share it
 * without either pulling the other's dependencies in.
 */

/**
 * How far two tracks may disagree before the player pulls the follower back.
 *
 * Half a second, and both bounds are real. Below it the measurement is noise:
 * `timeupdate` is only required to fire about four times a second, so two
 * tracks sampled at different moments legitimately read up to a quarter second
 * apart while being perfectly in step, and a tighter threshold would re-seek
 * forever against its own sampling jitter. Above it two views of one moment
 * visibly disagree. A correction is a real seek, so the threshold has to be a
 * number worth paying a seek for.
 */
export const MAX_DRIFT_SECONDS = 0.5;

/** What the `±10s` buttons move by. */
export const SKIP_SECONDS = 10;

/** Labels, spelled once, because the tests assert against the same constants
 *  the reader sees and a renamed button must not silently pass. */
export const PLAY_LABEL = "Play";
export const PAUSE_LABEL = "Pause";
export const BACK_LABEL = "Back 10 seconds";
export const FORWARD_LABEL = "Forward 10 seconds";
export const SCRUB_LABEL = "Scrub";

/** What a player says when a `play()` was refused. */
export const PLAY_REFUSED_LABEL = "Playback was refused";

/**
 * What each control SHOWS; the labels above are what it is CALLED.
 *
 * A sentence on a button has break opportunities in it and wraps in a narrow
 * pane (measured in a real WKWebView at 320 px), so the row shows glyphs and
 * carries the phrase in `aria-label`. Deliberately outside the emoji block:
 * these code points have no emoji presentation, so they render as text in the
 * surrounding font.
 */
export const PLAY_GLYPH = "\u25BA";
export const PAUSE_GLYPH = "\u275A\u275A";
export const BACK_GLYPH = `\u21BA${SKIP_SECONDS}`;
export const FORWARD_GLYPH = `\u21BB${SKIP_SECONDS}`;

/** `1:03` / `1:02:03`, and `--:--` for a duration nothing has reported yet. */
export function clock(seconds: number): string {
  if (!Number.isFinite(seconds) || seconds < 0) {
    return "--:--";
  }
  const whole = Math.floor(seconds);
  const minutes = Math.floor(whole / 60) % 60;
  const rest = String(whole % 60).padStart(2, "0");
  const hours = Math.floor(whole / 3600);
  if (hours > 0) {
    return `${hours}:${String(minutes).padStart(2, "0")}:${rest}`;
  }
  return `${minutes}:${rest}`;
}

/**
 * The scheme `keeper-recording://` is served on (`recording_protocol.rs`).
 *
 * A scheme of its own beside `keeper-note://` and `keeper-file://` because a
 * recordings root is provably outside every vault, and it is addressed by the
 * session's identity rather than by a path, so a retitle cannot break it.
 */
export const RECORDING_ASSET_SCHEME = "keeper-recording";

/**
 * The URL the webview plays a session's file over.
 *
 * Both halves arrive from Rust — the session id and a target the index
 * composed — and neither is joined onto anything here (AD-65). Each path
 * segment is percent-encoded so a `/` stays a separator and a space does not
 * end the path.
 */
export function recordingAssetUrl(sessionId: string, relativePath: string): string {
  const path = relativePath.split("/").map(encodeURIComponent).join("/");
  return `${RECORDING_ASSET_SCHEME}://${encodeURIComponent(sessionId)}/${path}`;
}

/**
 * How far past a standing position a video is nudged to make it show a frame.
 *
 * In a real WKWebView `preload="metadata"` settles at `readyState` 1, and a
 * video at HAVE_METADATA with no video data represents transparent black — a
 * canvas readback counted zero lit pixels. So every mounted video is primed:
 * one seek, one keyframe, paid on open rather than on play, which is far
 * cheaper than `preload="auto"` downloading the file.
 *
 * Non-zero because a seek to the current position may be collapsed into
 * nothing; far below one frame (33 ms at 30 fps) so the frame shown is the one
 * asked for.
 */
export const FRAME_PRIME_SECONDS = 0.001;

/** `HAVE_CURRENT_DATA`: the first `readyState` with a frame to paint. */
const HAVE_CURRENT_DATA = 2;

/**
 * Ask a video for the frame that `preload="metadata"` does not fetch.
 *
 * Once, on `loadedmetadata`, and only for an element nobody has moved: by the
 * time metadata arrives the reader may have scrubbed, and dragging the element
 * back to the top would move the recording under someone's hand. An element
 * that already has a frame is left alone.
 */
export function primeFirstFrame(player: HTMLVideoElement): void {
  // `once`: an element scrubbed BACK to zero would otherwise be primed again by
  // a later `loadedmetadata` from a source change.
  player.addEventListener(
    "loadedmetadata",
    () => {
      if (player.currentTime !== 0 || player.readyState >= HAVE_CURRENT_DATA) {
        return;
      }
      player.currentTime = FRAME_PRIME_SECONDS;
    },
    { once: true },
  );
}

/**
 * Make a media element let go of the resource it selected.
 *
 * Removing the node is not enough: a `<video>` or `<audio>` with a `src` holds
 * an open range-request pipeline and a decoder until it is told to let go,
 * against files that may live on a removable volume the user then cannot
 * eject. `pause()` first so nothing decodes while the source is pulled;
 * clearing `src` only changes what the NEXT load fetches, and `load()` is what
 * actually aborts the selected resource.
 */
export function releaseMediaElement(player: HTMLMediaElement): void {
  player.pause();
  player.removeAttribute("src");
  player.load();
}
