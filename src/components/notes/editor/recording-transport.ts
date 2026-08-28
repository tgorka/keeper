/**
 * One transport for the videos of one session (Story 43.6, FR-151, UX-DR53).
 *
 * A screen track and a camera track from the same `session:` are not two files
 * that happen to be adjacent; they are two views of one moment, and one moment
 * has one clock. Two native `<video>` transports side by side offer the reader
 * two clocks and no way to keep them agreeing — the first thing they do is
 * disagree. So the pair gets one scrub bar, one play/pause, one `±10s` and one
 * current-time readout, and each track keeps its own volume and mute, because
 * how loud the camera is next to the screen is a mixing decision and not a time
 * decision (UX-DR53).
 *
 * **The controls are the easy half; telling the truth about a pair is the
 * hard half.** Everything below exists because a pair can be in a state no
 * single element can report:
 *
 * - Two elements decoding two files drift. Neither is wrong; they are simply
 *   two independent clocks, and nothing in the platform ties them together.
 * - One will stall on a seek while the other lands it. A transport that asked
 *   the last element it touched would say "playing" while half the pair is
 *   frozen — the reader sees the lie before they see the frames.
 * - `play()` returns a promise that can reject: an autoplay policy, a detached
 *   element, a resource that went away. A pair where one `play()` was refused
 *   is NOT playing, and a play button stuck showing "Pause" is the worst
 *   possible answer because it blames the reader's next click.
 *
 * Every state this module reports is therefore a fold over ALL tracks, never a
 * reading of one: `some` for the bad news, `every` for the good.
 *
 * **What the pair is, is decided by the session, not by the number two.** The
 * fold is written over a list. Three angles of one moment is the same claim as
 * two, and the trio buffers when any one of them stalls. What is special is
 * ONE: a lone video keeps its native controls (see {@link RecordingTransport}),
 * because there is no second clock to keep it honest and a hand-built bar for
 * one track would be a worse `<video controls>` — no fullscreen, no
 * picture-in-picture, no captions menu, no platform keyboard conventions. This
 * module engages only where it is the only thing that can do the job.
 */

/**
 * How far two tracks may disagree before the transport pulls the follower back
 * to the scrub position.
 *
 * Half a second, and both bounds are real. Below it the measurement is noise:
 * `timeupdate` is only required to fire about four times a second, so two
 * tracks sampled at different moments legitimately read up to a quarter second
 * apart while being perfectly in step, and a tighter threshold would re-seek
 * forever against its own sampling jitter. Above it two views of one moment
 * visibly disagree — a cursor that moves before the click, a mouth that moves
 * after the word. And a correction is not free: assigning `currentTime` forces
 * a real seek, and on a screen recording with sparse keyframes that seek can
 * cost longer than the drift it repaired, so the threshold has to be a number
 * we are willing to pay a seek for.
 */
export const MAX_DRIFT_SECONDS = 0.5;

/** What the `±10s` buttons move by, and what `skip` is called with. */
export const SKIP_SECONDS = 10;

/** The transport's accessible name; a note may hold more than one session. */
export const TRANSPORT_LABEL = "Recording transport";

/** Labels, spelled once, because the tests assert against the same constants
 *  the reader sees and a renamed button must not silently pass. */
export const PLAY_LABEL = "Play";
export const PAUSE_LABEL = "Pause";
export const BACK_LABEL = "Back 10 seconds";
export const FORWARD_LABEL = "Forward 10 seconds";
export const SCRUB_LABEL = "Scrub";
export const MUTE_LABEL = "Mute";
export const UNMUTE_LABEL = "Unmute";
export const VOLUME_LABEL = "Volume";
export const BUFFERING_LABEL = "Buffering";
export const SEEKING_LABEL = "Seeking";

/** What the status line says when a `play()` was refused. The reason follows
 *  in the title, because the pair's state is the news and the platform's
 *  wording for it is the detail. */
export const PLAY_REFUSED_LABEL = "Playback was refused";

/**
 * What each control SHOWS. The labels above are what it is CALLED.
 *
 * Measured, not guessed: in a real WKWebView at a 320 px note pane the bar as
 * Story 43.6 shipped it laid out over five distinct rows, with "Back 10
 * seconds" broken across two line boxes and "Forward 10 seconds" across three
 * — the owner's "Back 10 / Pla y / Forward 10 seconds". A sentence on a button
 * is a sentence with break opportunities in it, and a flex row cannot make one
 * fit a pane that is narrower than the sentence.
 *
 * So every control in the row shows a glyph and carries its phrase in
 * `aria-label`, which is the one arrangement that costs a screen reader
 * nothing: the accessible name is unchanged from what shipped, and the visible
 * text no longer contains a single break opportunity. That last part is the
 * invariant the tests assert, because it is the property that actually
 * prevents the wrap — a class name would not.
 *
 * Deliberately outside the emoji block: U+25BA, U+275A, U+21BA, U+21BB and
 * U+266A have no emoji presentation, so they render as text in the editor's
 * own font rather than as colour images at a size nobody chose.
 */
export const PLAY_GLYPH = "\u25BA";
export const PAUSE_GLYPH = "\u275A\u275A";
export const BACK_GLYPH = `\u21BA${SKIP_SECONDS}`;
export const FORWARD_GLYPH = `\u21BB${SKIP_SECONDS}`;
export const MUTE_GLYPH = "\u266A";

/**
 * The slice of `HTMLMediaElement` a transport drives.
 *
 * Structural, not nominal, and deliberately so: jsdom implements no media
 * playback at all — `play()` returns `undefined` after logging "not
 * implemented", `readyState` never leaves 0, `duration` is `NaN` and no
 * `timeupdate`, `waiting` or `seeked` ever fires. The only way to prove a fold
 * over a stalling, rejecting, drifting pair is to drive tracks that can stall,
 * reject and drift, so the type has to admit one. A real `HTMLVideoElement`
 * satisfies it as it stands.
 */
export interface TransportTrack extends EventTarget {
  currentTime: number;
  readonly duration: number;
  controls: boolean;
  volume: number;
  muted: boolean;
  play(): Promise<void> | void;
  pause(): void;
}

/** What the pair is doing — never what one element is doing. */
export type TransportPlayback = "paused" | "playing" | "buffering";

/** Everything the bar draws, folded over every track. */
export interface TransportState {
  /** `buffering` whenever the pair cannot honour the reader's intent. */
  playback: TransportPlayback;
  /** A seek at least one track has not confirmed. Reported separately from
   *  {@link playback} because a seek that half-lands while paused is still a
   *  desynchronised pair, and silence about it is the failure this story
   *  exists to prevent. */
  seeking: boolean;
  /** The scrub position: what the reader asked for, and what drift is
   *  corrected toward. */
  position: number;
  /** The longest track's duration, or `NaN` before any metadata arrives. */
  duration: number;
  trackCount: number;
  /** Why the last `play()` did not take, or `null`. */
  failure: string | null;
}

interface Member {
  readonly track: TransportTrack;
  /**
   * The widget host the track was mounted into.
   *
   * It stays the member's address even while the element is somewhere else:
   * the stage below re-parents every track into ONE container so the pair
   * reads as one player, and the host is then an empty span holding the
   * embed's place in the note. Document order — and therefore which track
   * leads — is still read from the hosts, because they are the things that sit
   * where the author put them.
   */
  readonly host: HTMLElement;
  /**
   * The track's own DOM node, when the track is one.
   *
   * A fake track is an `EventTarget` and nothing more (that is the whole point
   * of {@link TransportTrack} being structural), so everything that MOVES an
   * element has to be able to find there is no element to move.
   */
  readonly element: HTMLElement | null;
  /** The file name, so four identical "Mute" buttons are four named ones. */
  readonly name: string;
  /** Set by `waiting`/`stalled`/`error`, cleared by `playing`/`canplay`. */
  stalled: boolean;
  /** A track that ran out is not drifting, it is finished: correcting it back
   *  toward a position the longer track is still advancing past would seek it
   *  in a loop it can never win. */
  ended: boolean;
  /** The box holding this track and its own mixer while the pair is grouped:
   *  the answer to a mute slider that floated away from the video it governs. */
  box: HTMLElement | null;
  mixer: HTMLElement | null;
  unwire: () => void;
}

/** The bar is one node the group re-parents, never one node per track. */
interface TransportBar {
  readonly dom: HTMLElement;
  dispose(): void;
}

/**
 * The one container the grouped pair lives in.
 *
 * A row of track boxes with the single transport beneath it, mounted in the
 * leading embed's host. It exists because "the pair reads as one player" is
 * not something CSS can say about two `<video>` elements in two different
 * `.cm-line` blocks: CodeMirror owns those lines, they are block-level, and no
 * rule this module is allowed to write puts them side by side. Re-parenting is
 * the only honest way, and it has the pleasant side effect that handing the
 * group down to a new leader moves ONE node — the tracks travel inside it, so
 * position, play state and focus survive exactly as the bar alone used to.
 */
interface TransportStage {
  readonly dom: HTMLElement;
  /** Where the track boxes go, above the bar. */
  readonly tracks: HTMLElement;
  dispose(): void;
}

/**
 * Which track leads: the one that appears FIRST in the note.
 *
 * Join order would do it most of the time and would be wrong exactly when it
 * matters — two embeds resolve through two independent IPC round trips, and
 * whichever answers first would otherwise own the clock and the bar. Reading
 * the document decides it the way the reader would: the bar sits under the top
 * video, and the top video is the reference the other is corrected toward.
 * Disconnected hosts compare as neither before nor after, and `sort` is stable,
 * so they keep their join order rather than shuffling.
 */
function byDocumentOrder(one: Member, other: Member): number {
  const relation = one.host.compareDocumentPosition(other.host);
  if ((relation & Node.DOCUMENT_POSITION_FOLLOWING) !== 0) {
    return -1;
  }
  if ((relation & Node.DOCUMENT_POSITION_PRECEDING) !== 0) {
    return 1;
  }
  return 0;
}

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
 * A control on the bar or on a mixer: a real `<button>`, keyboard reachable
 * and announced as a control rather than as decorated text.
 *
 * `glyph` and `label` are separate arguments and never the same string. What
 * the control shows has to survive a 320 px pane; what it is CALLED has to be
 * a phrase a screen reader can read out. Collapsing the two is how the shipped
 * bar came to wrap "Forward 10 seconds" across three line boxes.
 */
function control(
  className: string,
  glyph: string,
  label: string,
  run: () => void,
): HTMLButtonElement {
  const button = document.createElement("button");
  button.type = "button";
  button.className = className;
  button.textContent = glyph;
  button.setAttribute("aria-label", label);
  button.addEventListener("click", run);
  return button;
}

/**
 * One track's volume and mute, beside that track.
 *
 * These are the two controls native `controls` took away, and the two the
 * transport deliberately does not centralise: a shared volume would mean the
 * reader can never turn the camera down under the screen, which is the one
 * mixing decision a two-view recording actually needs (UX-DR53).
 */
function mixer(member: Member): HTMLElement {
  const strip = document.createElement("span");
  strip.className = "cm-lp-recording-mix";

  const mute = control("cm-lp-recording-mix-mute", MUTE_GLYPH, MUTE_LABEL, () => {
    member.track.muted = !member.track.muted;
    paint();
  });
  function paint(): void {
    const label = member.track.muted ? UNMUTE_LABEL : MUTE_LABEL;
    // One glyph for both states, struck through when muted by the theme's
    // `[aria-pressed="true"]` rule: two different symbols would be two things
    // to learn, and the pressed state is already the accessible truth.
    mute.textContent = MUTE_GLYPH;
    // The file name is in the accessible name because a session's two tracks
    // are two "Mute" buttons said twice to anyone not looking at the screen.
    mute.setAttribute("aria-label", `${label} ${member.name}`);
    mute.setAttribute("aria-pressed", String(member.track.muted));
  }
  paint();

  const volume = document.createElement("input");
  volume.type = "range";
  volume.className = "cm-lp-recording-mix-volume";
  volume.min = "0";
  volume.max = "1";
  volume.step = "0.05";
  volume.value = String(member.track.volume);
  volume.setAttribute("aria-label", `${VOLUME_LABEL} ${member.name}`);
  volume.addEventListener("input", () => {
    member.track.volume = Number(volume.value);
  });

  strip.append(mute, volume);
  return strip;
}

/**
 * The one bar: the pair's clock, drawn from the pair's state.
 *
 * It reads nothing off any element. Every value it shows came from the fold, so
 * there is no path by which it can report the track the transport happened to
 * touch last.
 *
 * **Two rows, and the split is the fix for the wrap.** The controls live in a
 * row that must never break; the status is not a control and gets a line of
 * its own. That matters because the status is the one thing here that is a
 * real sentence — "Playback was refused" — and a sentence cannot be made to
 * fit a 320 px pane beside five controls. Putting it on the control row meant
 * either wrapping the row or truncating the one message the reader most needs
 * to read. It collapses to nothing while empty, which is almost always.
 */
function transportBar(transport: RecordingTransport): TransportBar {
  const dom = document.createElement("span");
  dom.className = "cm-lp-recording-transport";
  dom.setAttribute("role", "group");
  dom.setAttribute("aria-label", TRANSPORT_LABEL);

  const row = document.createElement("span");
  row.className = "cm-lp-recording-transport-row";

  const back = control("cm-lp-recording-transport-skip", BACK_GLYPH, BACK_LABEL, () => {
    transport.skip(-SKIP_SECONDS);
  });
  const toggle = control("cm-lp-recording-transport-toggle", PLAY_GLYPH, PLAY_LABEL, () => {
    void transport.toggle();
  });
  const forward = control("cm-lp-recording-transport-skip", FORWARD_GLYPH, FORWARD_LABEL, () => {
    transport.skip(SKIP_SECONDS);
  });

  const scrub = document.createElement("input");
  scrub.type = "range";
  scrub.className = "cm-lp-recording-scrub";
  scrub.min = "0";
  scrub.max = "0";
  scrub.step = "0.05";
  scrub.value = "0";
  scrub.setAttribute("aria-label", SCRUB_LABEL);
  scrub.addEventListener("input", () => {
    transport.seekTo(Number(scrub.value));
  });

  const readout = document.createElement("span");
  readout.className = "cm-lp-recording-time";

  // `role="status"`: a stall, a refused play and a half-landed seek are all
  // things the reader must be TOLD about, not left to infer from a thumb that
  // stopped moving.
  const status = document.createElement("span");
  status.className = "cm-lp-recording-transport-status";
  status.setAttribute("role", "status");

  function render(state: TransportState): void {
    const playing = state.playback !== "paused";
    toggle.textContent = playing ? PAUSE_GLYPH : PLAY_GLYPH;
    toggle.setAttribute("aria-label", playing ? PAUSE_LABEL : PLAY_LABEL);
    // Until metadata arrives the pair has no span, and a range input whose
    // `max` is 0 clamps every value the reader sets back to 0 — a control that
    // looks live and does nothing. Disabled says the true thing: not yet. The
    // `±10s` buttons stay live, because a relative move needs no span.
    const measured = Number.isFinite(state.duration);
    scrub.disabled = !measured;
    scrub.max = String(measured ? state.duration : 0);
    scrub.value = String(state.position);
    // No spaces around the separator, and that is not typographic fussiness:
    // a space is a break opportunity, and the readout is on the row that must
    // not break. At 320 px the spaced form laid out over three line boxes.
    readout.textContent = `${clock(state.position)}/${clock(state.duration)}`;
    // A refusal outranks a stall: the pair is not merely late, it declined.
    status.textContent =
      state.failure !== null
        ? PLAY_REFUSED_LABEL
        : state.seeking
          ? SEEKING_LABEL
          : state.playback === "buffering"
            ? BUFFERING_LABEL
            : "";
    if (state.failure !== null) {
      status.title = state.failure;
    } else {
      status.removeAttribute("title");
    }
  }

  row.append(back, toggle, forward, scrub, readout);
  dom.append(row, status);
  const unsubscribe = transport.subscribe(render);
  render(transport.state);

  return {
    dom,
    dispose: () => {
      unsubscribe();
      dom.remove();
    },
  };
}

/** The stage: a row of track boxes with the one bar beneath them. */
function transportStage(transport: RecordingTransport): TransportStage {
  const dom = document.createElement("span");
  dom.className = "cm-lp-recording-stage";

  const tracks = document.createElement("span");
  tracks.className = "cm-lp-recording-tracks";

  const bar = transportBar(transport);
  dom.append(tracks, bar.dom);

  return {
    dom,
    tracks,
    dispose: () => {
      bar.dispose();
      dom.remove();
    },
  };
}

/**
 * The clock of one session's videos.
 *
 * Engages at two tracks and disengages back to one, which is the whole of the
 * "what about a single video" decision: a lone `<video>` keeps `controls` and
 * behaves exactly as Story 42.6 shipped it, and the moment a second track of
 * the same session mounts, both give their native transports up — a native
 * transport carries its own scrub bar, and a second scrub bar is a second
 * clock, which is the thing this class exists to remove.
 */
export class RecordingTransport {
  private readonly members: Member[] = [];
  private readonly listeners = new Set<(state: TransportState) => void>();
  /** Tracks that were told to seek and have not said they arrived. */
  private readonly unconfirmed = new Set<TransportTrack>();
  /** What the READER asked for. `playback` is what the pair managed. */
  private intent: "paused" | "playing" = "paused";
  private position = 0;
  private failure: string | null = null;
  private stage: TransportStage | null = null;
  /** Set while the transport is the one calling `pause()`, so a track's own
   *  `pause` event is not mistaken for the track dropping out of the pair. */
  private applying = false;
  /** Bumped by every intent change, so a `play()` whose promises settle after
   *  the reader already pressed pause cannot overwrite the newer answer. */
  private generation = 0;

  /** The one element every follower is corrected toward. */
  private reference(): Member | undefined {
    return this.members[0];
  }

  /**
   * The pair's duration is its LONGEST track.
   *
   * Two views of one moment start together and stop when their own writer
   * stopped, so the camera track is routinely a second shorter than the screen
   * track. Taking the shortest would make the last second of the recording
   * unreachable on the scrub bar; taking the longest makes it reachable, and a
   * track that ran out simply sits on its final frame.
   */
  private span(): number {
    let longest = Number.NaN;
    for (const member of this.members) {
      const { duration } = member.track;
      if (Number.isFinite(duration) && !(duration <= longest)) {
        longest = duration;
      }
    }
    return longest;
  }

  get state(): TransportState {
    const seeking = this.unconfirmed.size > 0;
    // The fold, and the reason this class exists. `some` for the bad news:
    // one stalled track makes the PAIR buffering, however happily the other
    // one is decoding.
    const playback: TransportPlayback =
      this.intent === "paused"
        ? "paused"
        : seeking || this.members.some((member) => member.stalled)
          ? "buffering"
          : "playing";
    return {
      playback,
      seeking,
      position: this.position,
      duration: this.span(),
      trackCount: this.members.length,
      failure: this.failure,
    };
  }

  subscribe(listener: (state: TransportState) => void): () => void {
    this.listeners.add(listener);
    return () => {
      this.listeners.delete(listener);
    };
  }

  private emit(): void {
    const { state } = this;
    for (const listener of this.listeners) {
      listener(state);
    }
  }

  /**
   * Add a mounted track. `host` must already contain `track`: the leader is
   * decided by document position, and a detached element has none.
   */
  join(track: TransportTrack, host: HTMLElement, name: string): void {
    if (this.members.some((member) => member.track === track)) {
      return;
    }
    const member: Member = {
      track,
      host,
      // `instanceof Element` rather than a cast: {@link TransportTrack} is
      // structural precisely so a fake can satisfy it, and a fake has no node
      // to re-parent. Asking the question here means nothing below has to.
      element: track instanceof HTMLElement ? track : null,
      name,
      stalled: false,
      ended: false,
      box: null,
      mixer: null,
      unwire: () => {},
    };
    member.unwire = this.wire(member);
    this.members.push(member);
    hostOwners.set(host, this);
    this.refresh();
  }

  /**
   * Drop a track that is going away — scrolled out of the viewport, edited out
   * of the note, or whose file stopped loading.
   *
   * Called before the widget empties its host, so the group can move to the
   * next track rather than being thrown away with the element it happened to
   * hang under. The departing element is handed back to its own host FIRST,
   * because while the pair is grouped it lives in the shared stage, and the
   * teardown that follows looks for it in the host — a track left behind in
   * the stage would keep its decoder and its open range requests against a
   * file the user is trying to eject.
   */
  leave(track: TransportTrack): void {
    const index = this.members.findIndex((member) => member.track === track);
    if (index < 0) {
      return;
    }
    const [member] = this.members.splice(index, 1);
    member.unwire();
    this.unbox(member);
    // A departed track will never confirm its seek, and leaving it in the set
    // would wedge the bar on "Seeking" for the tracks that are still here.
    this.unconfirmed.delete(track);
    hostOwners.delete(member.host);
    this.refresh();
  }

  /** Hand `host`'s track back and drop it. The host-shaped door into
   *  {@link leave}, for a widget tearing down a host whose element the stage
   *  is holding. */
  leaveHost(host: HTMLElement): void {
    const member = this.members.find((candidate) => candidate.host === host);
    if (member !== undefined) {
      this.leave(member.track);
    }
  }

  /** Return a member's element to its own host and drop its box. The inverse
   *  of the boxing in {@link engage}, and the thing that makes both leaving
   *  and falling back to one track put the note back the way it was. */
  private unbox(member: Member): void {
    if (member.box === null) {
      return;
    }
    if (member.element !== null) {
      member.host.append(member.element);
    }
    member.mixer?.remove();
    member.mixer = null;
    member.box.remove();
    member.box = null;
  }

  private wire(member: Member): () => void {
    const undo: (() => void)[] = [];
    const on = (type: string, run: () => void): void => {
      member.track.addEventListener(type, run);
      undo.push(() => member.track.removeEventListener(type, run));
    };

    on("timeupdate", () => {
      this.advance(member);
    });
    on("waiting", () => {
      this.stall(member, true);
    });
    on("stalled", () => {
      this.stall(member, true);
    });
    // An element that failed is not coming back on its own, and it must not
    // hold the pair on "Seeking" forever waiting for a confirmation it cannot
    // send. It is still stalled: the pair is not playing.
    on("error", () => {
      this.unconfirmed.delete(member.track);
      this.stall(member, true);
    });
    on("playing", () => {
      this.stall(member, false);
    });
    on("canplay", () => {
      this.stall(member, false);
    });
    // A seek this transport did not order — the platform's own, or a drift
    // correction — is still a seek the pair is waiting on.
    on("seeking", () => {
      member.ended = false;
      this.unconfirmed.add(member.track);
      this.emit();
    });
    on("seeked", () => {
      this.unconfirmed.delete(member.track);
      this.emit();
    });
    on("durationchange", () => {
      this.emit();
    });
    on("loadedmetadata", () => {
      this.emit();
    });
    on("pause", () => {
      this.stopped(member);
    });
    on("ended", () => {
      member.ended = true;
      this.finished();
    });

    return () => {
      for (const off of undo) {
        off();
      }
    };
  }

  private stall(member: Member, stalled: boolean): void {
    if (member.stalled === stalled) {
      return;
    }
    member.stalled = stalled;
    this.emit();
  }

  /**
   * A track paused itself while the reader wanted the pair playing.
   *
   * Letting the others run on is the silent desynchronisation this story is
   * about, so the pair stops with it. The reader sees a paused transport and a
   * pair that is still aligned, which is recoverable; they would not see a pair
   * that quietly grew a ten-second gap.
   */
  private stopped(member: Member): void {
    if (this.applying || this.intent === "paused" || member.ended) {
      return;
    }
    this.pause();
  }

  private finished(): void {
    if (!this.members.every((member) => member.ended)) {
      // The shorter track ran out. The pair keeps running to the longer one's
      // end, which is why `span` is the maximum.
      this.emit();
      return;
    }
    this.pause();
  }

  private advance(member: Member): void {
    // While a seek is unconfirmed the elements are reporting where they WERE,
    // and believing them would drag the scrub thumb backwards out from under
    // the reader who just moved it.
    if (this.unconfirmed.size > 0) {
      return;
    }
    if (member === this.reference()) {
      this.position = member.track.currentTime;
    }
    this.correctDrift();
    this.emit();
  }

  /**
   * Pull every follower back to the scrub position once it has drifted past
   * {@link MAX_DRIFT_SECONDS}.
   *
   * Toward the scrub position, not toward each other: the position is what the
   * bar shows and what the reader asked for, and a pair that agrees with each
   * other while disagreeing with the readout is a third kind of lie.
   */
  private correctDrift(): void {
    if (this.unconfirmed.size > 0) {
      return;
    }
    const reference = this.reference();
    for (const member of this.members) {
      if (member === reference || member.ended) {
        continue;
      }
      if (Math.abs(member.track.currentTime - this.position) <= MAX_DRIFT_SECONDS) {
        continue;
      }
      // Assigning `currentTime` is a seek; the `seeking` event it raises puts
      // the pair back into the unconfirmed set, so the correction is visible
      // for as long as it takes.
      member.track.currentTime = this.position;
    }
  }

  /**
   * Start the pair, or report honestly that it did not start.
   *
   * `allSettled`, never `all`: `all` rejects on the first failure and leaves
   * the other track playing on its own, which is precisely the half-state the
   * transport must not enter. One refusal pauses everything and the bar says
   * so, because a pair where one `play()` rejected is not playing.
   */
  async play(): Promise<void> {
    this.generation += 1;
    const generation = this.generation;
    this.intent = "playing";
    this.failure = null;
    this.emit();
    const results = await Promise.allSettled(
      // `async` rather than calling `play()` bare: it turns a host that throws
      // synchronously, and one that returns no promise at all, into the same
      // settled result as one that rejects.
      this.members.map(async (member) => {
        await member.track.play();
      }),
    );
    if (generation !== this.generation) {
      // The reader pressed pause, or scrubbed, while the promises were in
      // flight. Their answer is newer than this one.
      return;
    }
    const refused = results.find((result) => result.status === "rejected");
    if (refused === undefined) {
      this.emit();
      return;
    }
    this.intent = "paused";
    this.applying = true;
    for (const member of this.members) {
      member.track.pause();
    }
    this.applying = false;
    // The platform's own wording for the refusal, kept as the detail behind
    // PLAY_REFUSED_LABEL rather than paraphrased into something less true.
    const cause: unknown = refused.reason;
    this.failure = cause instanceof Error ? cause.message : String(cause);
    this.emit();
  }

  pause(): void {
    this.generation += 1;
    this.intent = "paused";
    this.applying = true;
    for (const member of this.members) {
      member.track.pause();
    }
    this.applying = false;
    this.emit();
  }

  /** Reads the reader's intent, not the fold: a buffering pair was asked to
   *  play, so the button under their finger says Pause and must pause. */
  toggle(): Promise<void> {
    if (this.intent === "playing") {
      this.pause();
      return Promise.resolve();
    }
    return this.play();
  }

  /**
   * Move the whole pair to `seconds`.
   *
   * Every track is marked unconfirmed before any is asked, so a track that
   * never answers keeps the pair visibly seeking instead of letting the bar
   * settle on a position only half the pair reached.
   */
  seekTo(seconds: number): void {
    const duration = this.span();
    const bounded = Math.max(0, Number.isFinite(duration) ? Math.min(seconds, duration) : seconds);
    this.position = bounded;
    this.failure = null;
    for (const member of this.members) {
      member.ended = false;
      this.unconfirmed.add(member.track);
      member.track.currentTime = bounded;
    }
    this.emit();
  }

  skip(delta: number): void {
    this.seekTo(this.position + delta);
  }

  /** Engage the group, disengage back to a lone video, or re-seat the stage
   *  under a new leader. */
  private refresh(): void {
    this.members.sort(byDocumentOrder);
    if (this.members.length > 1) {
      this.engage();
    } else {
      this.disengage();
    }
    this.emit();
  }

  /**
   * Gather every track into one stage under the leading embed.
   *
   * Each track goes into its OWN box with its own mixer, which is the whole of
   * the fix for a mute slider that sat away from the track it governs: before
   * this, the mixer was appended to the widget's host and the two hosts are
   * two separate lines, so the sliders read as furniture belonging to the note
   * rather than to a video. A box is a boundary, and a control inside one is
   * unambiguously about what else is inside it.
   *
   * `append` on a node that is already a child MOVES it, so re-running this on
   * every join re-sorts the boxes into document order for free, and re-seating
   * the stage under a new leader carries the tracks inside it — one node moved,
   * in one task, which is what keeps a playing element playing.
   */
  private engage(): void {
    this.stage ??= transportStage(this);
    const stage = this.stage;
    for (const member of this.members) {
      // The native transport carries its own scrub bar, and a second scrub bar
      // is a second clock.
      member.track.controls = false;
      if (member.box === null) {
        const box = document.createElement("span");
        box.className = "cm-lp-recording-track";
        if (member.element !== null) {
          box.append(member.element);
        }
        member.mixer = mixer(member);
        box.append(member.mixer);
        member.box = box;
      }
      stage.tracks.append(member.box);
    }
    const leader = this.reference();
    if (leader !== undefined && stage.dom.parentElement !== leader.host) {
      leader.host.append(stage.dom);
    }
  }

  /** Give a lone track its native controls back and put its element home. */
  private disengage(): void {
    for (const member of this.members) {
      member.track.controls = true;
      this.unbox(member);
    }
    this.stage?.dispose();
    this.stage = null;
  }
}

/**
 * Which transport is holding a widget host's track.
 *
 * Keyed by HOST, not by element, and that is the only key that still works
 * once the pair is grouped: staging moves each `<video>` into the leader's
 * stage, so a follower's host is an empty span and a teardown that searched it
 * for a media element would find nothing, release nothing, and leave the
 * transport holding an element whose widget is gone — the exact leak
 * `releaseRecordingMedia` was written to prevent, reintroduced by the fix for
 * the layout. The host is also the one thing a widget still has while it is
 * being destroyed.
 *
 * Weak, because a strong map here would pin every host a long editing session
 * ever scrolled past.
 */
const hostOwners = new WeakMap<HTMLElement, RecordingTransport>();

/** Hand `host`'s track back to it and drop it from its transport. A no-op for
 *  a host that never joined one, which is what makes it safe on every teardown
 *  path — including the lone video that was never grouped. */
export function releaseHost(host: HTMLElement): void {
  hostOwners.get(host)?.leaveHost(host);
}

/**
 * The transports of one editor, keyed by session.
 *
 * Scoped rather than global because "the same session" is only the same pair
 * inside one view: two editors open on one note are two readers, and one of
 * them pressing play must not move the other one's video. The scope is held
 * weakly, so a destroyed `EditorView` takes its transports with it.
 */
const scopes = new WeakMap<object, Map<string, RecordingTransport>>();

/** The transport for `sessionId` within `scope`, created on first ask. */
export function transportFor(scope: object, sessionId: string): RecordingTransport {
  let bySession = scopes.get(scope);
  if (bySession === undefined) {
    bySession = new Map();
    scopes.set(scope, bySession);
  }
  let transport = bySession.get(sessionId);
  if (transport === undefined) {
    transport = new RecordingTransport();
    bySession.set(sessionId, transport);
  }
  return transport;
}

// ---------------------------------------------------------------------------
// The two facts about an HTMLMediaElement that every surface needs
// ---------------------------------------------------------------------------
//
// Homed in this module, and it is worth saying why here rather than only in a
// commit message. They arrived in Story 44.1 inside `recording-embed.ts`, which
// is a CodeMirror widget module: it imports `@codemirror/view` and the Tauri
// IPC client. By Story 45.7 there are three consumers — the note embed, Story
// 44.15's gallery, and the Files panel's media viewer, which is React and has
// no business importing an editor widget to learn how to release a `<video>`.
// This module imports NOTHING, which is what makes it the honest home for two
// facts that are about the platform rather than about any surface.
//
// The alternative was a fourth module for two functions, or three surfaces each
// spelling `pause(); removeAttribute("src"); load();` for themselves — and a
// release that three places implement is a release that two of them will get
// wrong, which is the leak `releaseRecordingMedia` exists to prevent.

/**
 * How far past a standing position a video is nudged to make it show a frame.
 *
 * **The defect this exists for, measured rather than reasoned about.** In a
 * real WKWebView — the engine the Tauri shell renders in — driving the owner's
 * own two-track session, `preload="metadata"` settles at `readyState` 1
 * (HAVE_METADATA). The HTML spec says a video element at HAVE_METADATA that
 * has obtained no video data represents *transparent black*, and WebKit obeys
 * it exactly: a canvas readback of the element counted ZERO lit pixels. The
 * element is genuinely frameless. What the reader sees through it is whatever
 * background the host draws, which is the grey box the field report describes.
 *
 * **This was never a two-video defect.** The single video measured the same —
 * zero lit pixels — and read as working only because WebKit paints its native
 * `controls` chrome over the emptiness. Story 43.6's transport takes `controls`
 * away at two tracks and the emptiness becomes visible. So the prime belongs to
 * every video anyone mounts, whether there is one of them or two, and whether
 * it has native controls or not: a video that shows nothing until you press
 * play is not a video.
 *
 * **The cost, stated rather than left to be discovered.** Assigning
 * `currentTime` is a seek, and a seek is a real range request for real bytes
 * against a file that may be a multi-hundred-megabyte screen recording on a
 * pendrive or a network volume. Opening a note — or a panel — therefore touches
 * the drive once per video. That is the right price: it buys one keyframe, not
 * the `preload="auto"` download of the whole file. But it is a price, and it is
 * paid on open rather than on play.
 *
 * A millisecond, and both bounds are real. Non-zero because a seek to the
 * position the element already reports is one a user agent may collapse into
 * nothing, and nothing is exactly what cannot be afforded here. Far below one
 * frame's duration — 33 ms at 30 fps — so the frame presented is the frame at
 * the position asked for and not the one after it.
 */
export const FRAME_PRIME_SECONDS = 0.001;

/** `HAVE_CURRENT_DATA`: the first `readyState` at which the element has a
 *  frame to paint. Spelled as a constant because that threshold, not the
 *  number, is the thing being tested. */
const HAVE_CURRENT_DATA = 2;

/**
 * Ask a video for the frame that `preload="metadata"` does not fetch.
 *
 * Once, on `loadedmetadata`, and only for an element nobody has moved. Both
 * halves of that matter. Once, because the point is to buy a frame and not to
 * keep buying one. And only for an untouched element, because by the time
 * metadata arrives the reader may have scrubbed, or the transport may have
 * placed the pair at a shared position — dragging either of them back to the
 * top would make a cosmetic fix into a control that moves the recording under
 * someone's hand.
 *
 * An element that already has a frame is left alone: there is nothing to buy
 * and the request would be spent for nothing.
 */
export function primeFirstFrame(player: HTMLVideoElement): void {
  // `once`, declared rather than unregistered by hand: the guard below already
  // refuses to move an element that is not at the top, but an element the
  // reader has scrubbed BACK to zero is at the top and would be primed a
  // second time by a later `loadedmetadata` — a source change or a reload —
  // moving them a millisecond they did not ask for. The platform enforcing
  // "once" is also one fewer listener left on an element this module works
  // hard to let go of.
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
 * **Removing the node is not enough, and that is the whole of this function.**
 * A `<video>` or `<audio>` with a `src` holds an open range-request pipeline
 * and a decoder until it is told to let go. A long editing session that
 * scrolled past a dozen recordings, or a panel strip a reader opened and closed
 * all afternoon, would otherwise accumulate a dozen of them against files that
 * may live on a removable volume the user then cannot eject.
 *
 * Three calls and the order matters. `pause()` first, so nothing is decoding
 * while the source is pulled. Clearing `src` alone only changes what the NEXT
 * load would fetch — `load()` is what actually aborts the selected resource,
 * and it is the call people leave out.
 */
export function releaseMediaElement(player: HTMLMediaElement): void {
  player.pause();
  player.removeAttribute("src");
  player.load();
}
