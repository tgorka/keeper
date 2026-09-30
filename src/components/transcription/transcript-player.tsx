/**
 * The transcript's player: a session's parts as one timeline.
 *
 * `RecordingTransport` pairs the videos a note embeds, grouped by where they sit
 * in the document; it knows one file per track and no parts. Here the pair is
 * always this part's screen and camera, and the timeline runs across parts, so
 * the player keeps its own clock and borrows the transport's vocabulary: the
 * glyphs, the labels, the drift threshold and the release.
 *
 * The part's own file leads. It carries the sound, so it keeps playing — hidden
 * — when only the camera is shown; the camera is muted and pulled back to it
 * when the two drift apart. At the end of a part the next one is loaded and
 * started where it begins, and a seek to another part loads that part and lands
 * once the file knows its length.
 */
import {
  Columns2,
  Crosshair,
  Mic,
  Monitor,
  Phone,
  Pin,
  PinOff,
  Video,
  Volume2,
} from "lucide-react";
import {
  type ReactNode,
  type Ref,
  useCallback,
  useEffect,
  useImperativeHandle,
  useMemo,
  useRef,
  useState,
} from "react";
import {
  BACK_GLYPH,
  BACK_LABEL,
  clock,
  FORWARD_GLYPH,
  FORWARD_LABEL,
  MAX_DRIFT_SECONDS,
  PAUSE_GLYPH,
  PAUSE_LABEL,
  PLAY_GLYPH,
  PLAY_LABEL,
  PLAY_REFUSED_LABEL,
  primeFirstFrame,
  recordingAssetUrl,
  releaseMediaElement,
  SCRUB_LABEL,
  SKIP_SECONDS,
} from "@/components/notes/editor/media-playback";
import { Button } from "@/components/ui/button";
import { Toggle } from "@/components/ui/toggle";
import { ToggleGroup, ToggleGroupItem } from "@/components/ui/toggle-group";
import { IconHint } from "@/components/ui/tooltip";
import type { MediaRef, TranscriptMediaVm } from "@/lib/ipc/client";
import { fileAssetUrl } from "@/lib/viewers/file-asset-url";
import { locate, sessionLength } from "./session-timeline";

export const PLAYER_LABEL = "Transcript player";
export const PICTURE_LABEL = "Picture";
export const SOUND_LABEL = "Sound";
export const PIN_LABEL = "Keep the player on top";
export const FOLLOW_LABEL = "Follow the transcript";
export const PART_UNAVAILABLE_SENTENCE =
  "This part is in no synced folder and among no recordings, so keeper cannot play it here.";
/** A part whose bytes the sync has not brought to this device yet (an LFS pointer). */
export const PART_NOT_HERE_SENTENCE =
  "This part is not on this Mac yet. Once the sync brings it, it plays here.";

export interface TranscriptPlayerHandle {
  /** Move to `seconds` on the transcript's timeline; `play` starts it, absent keeps it as it was. */
  seek: (seconds: number, play?: boolean) => void;
  pause: () => void;
}

/** A window of the timeline, in seconds: the player plays only inside it. */
export interface PlayerWindow {
  from: number;
  to: number;
}

/** The URL a part's file is served on: from its synced folder, or from its recording by session. */
function mediaUrl(ref: MediaRef): string {
  return ref.via === "recording"
    ? recordingAssetUrl(ref.sessionId, ref.relativePath)
    : fileAssetUrl(ref.profileId, ref.relativePath);
}
/**
 * An icon toggle's state must read at a glance: on wears the accent — its ink
 * and its edge on the lifted fill — and off is a quiet outline. Keyed on ARIA,
 * not `data-state`: the tooltip's trigger shares the element and writes its own
 * `data-state` ("closed") over the toggle's, which left on and off alike.
 */
const TOGGLE_STATE =
  "border-border aria-pressed:border-primary aria-pressed:bg-secondary aria-pressed:text-primary";
/** A segment wears the same accent when chosen; the unchosen stay in the trough. */
const SEGMENT_STATE =
  "aria-checked:border-primary aria-checked:bg-background aria-checked:text-primary";
/** The tooltip says the state in words, beside what the icon shows. */
function stateHint(label: string, on: boolean): string {
  return `${label}: ${on ? "on" : "off"}`;
}

export type Picture = "screen" | "camera" | "both";
export type Sound = "system" | "microphone" | "both";

/** One segment of a picture or sound choice: its value, its name, its glyph. */
interface Choice<V extends string> {
  value: V;
  label: string;
  icon: ReactNode;
}
const PICTURE_CHOICES: readonly Choice<Picture>[] = [
  { value: "screen", label: "Screen", icon: <Monitor aria-hidden="true" /> },
  { value: "camera", label: "Camera", icon: <Video aria-hidden="true" /> },
  { value: "both", label: "Screen and camera", icon: <Columns2 aria-hidden="true" /> },
];
const SOUND_CHOICES: readonly Choice<Sound>[] = [
  { value: "system", label: "Call", icon: <Phone aria-hidden="true" /> },
  { value: "microphone", label: "Microphone", icon: <Mic aria-hidden="true" /> },
  { value: "both", label: "Call and microphone", icon: <Volume2 aria-hidden="true" /> },
];

/**
 * A segmented icon choice. Radix makes it a radio group, so each segment's name
 * is its `aria-label` and its tooltip says the same words; a click on the
 * segment already chosen keeps it (Radix would otherwise answer "").
 */
function ChoiceGroup<V extends string>({
  label,
  value,
  choices,
  onChange,
}: {
  label: string;
  value: V;
  choices: readonly Choice<V>[];
  onChange: (value: V) => void;
}) {
  return (
    <ToggleGroup
      type="single"
      aria-label={label}
      value={value}
      onValueChange={(next) => {
        if (next) onChange(next as V);
      }}
    >
      {choices.map((choice) => (
        <IconHint key={choice.value} label={stateHint(choice.label, choice.value === value)}>
          <ToggleGroupItem value={choice.value} aria-label={choice.label} className={SEGMENT_STATE}>
            {choice.icon}
          </ToggleGroupItem>
        </IconHint>
      ))}
    </ToggleGroup>
  );
}

/**
 * A video's box: 16:9 from its width, so an element that has not loaded its
 * metadata yet (and has no size of its own) still holds the player's height
 * instead of collapsing; `object-contain` letterboxes any other shape into it.
 */
const VIDEO_BOX = "aspect-video max-h-[30dvh] min-w-0 flex-1 rounded-md bg-muted object-contain";

/** WebKit's `audioTracks`; absent from TypeScript's DOM library and from Chromium. */
interface AudioTracks {
  readonly length: number;
  readonly [index: number]: { enabled: boolean } | undefined;
}

/**
 * An element that loads `src` when React hands it over and lets it go when
 * React takes it back — the part ends, the camera is hidden, the viewer closes.
 * The source is set here rather than as a prop so the two cannot come apart: a
 * prop is set once, and an element that was released and handed back (Strict
 * Mode does exactly that) would come back with nothing to play.
 */
function useMediaElement<E extends HTMLMediaElement>(src: string | null) {
  const current = useRef<E | null>(null);
  const attach = useCallback(
    (node: E | null) => {
      current.current = node;
      if (!node || src === null) return;
      // `preload="metadata"` fetches no frame, so an unplayed video is a grey
      // box; the prime buys the first one. Registered before `src`, which is
      // what starts the load.
      if (node instanceof HTMLVideoElement) primeFirstFrame(node);
      node.src = src;
      return () => {
        if (current.current === node) current.current = null;
        releaseMediaElement(node);
      };
    },
    [src],
  );
  return [current, attach] as const;
}

export function TranscriptPlayer({
  media,
  pinned,
  onPinnedChange,
  follow,
  onFollowChange,
  onTime,
  onJump,
  onPlayingChange,
  window,
  initialPicture = "both",
  initialSound = "both",
  meta,
  ref,
}: {
  media: TranscriptMediaVm;
  /** With `onPinnedChange`, offers Keep the player on top; a host that scrolls as a whole has neither. */
  pinned?: boolean;
  onPinnedChange?: (pinned: boolean) => void;
  follow: boolean;
  onFollowChange: (follow: boolean) => void;
  /** Where the timeline is, on every tick and every seek. */
  onTime: (seconds: number) => void;
  /** A play or a seek: the reader moved the player on purpose. */
  onJump: () => void;
  onPlayingChange?: (playing: boolean) => void;
  /** Plays only this stretch: the scrub spans it, the player starts at `from` and stops at `to`. */
  window?: PlayerWindow;
  /** What is shown and heard first; the person may change either. */
  initialPicture?: Picture;
  initialSound?: Sound;
  /** Said on the row under the controls, beside which part is playing. */
  meta?: ReactNode;
  ref?: Ref<TranscriptPlayerHandle>;
}) {
  // A part whose length Rust could not tell (a session nobody transcribed yet)
  // takes it from its file once loaded; the parts then lie end to end.
  const [measured, setMeasured] = useState<Record<number, number>>({});
  const parts = useMemo(() => {
    if (media.parts.every((p) => p.duration > 0)) return media.parts;
    let offset = 0;
    return media.parts.map((p, i) => {
      const duration = p.duration > 0 ? p.duration : (measured[i] ?? 0);
      const placed = { ...p, offset, duration };
      offset += duration;
      return placed;
    });
  }, [media.parts, measured]);
  const total = sessionLength(parts);
  const lo = window?.from ?? 0;
  const hi = window ? (total > 0 ? Math.min(window.to, total) : window.to) : total;
  // Without a window it starts at the first part, whatever the lengths say — unknown
  // lengths (all 0) would otherwise place time 0 past every part, in the last.
  const [index, setIndex] = useState(() => (lo > 0 ? (locate(media.parts, lo)?.index ?? 0) : 0));
  const [time, setTime] = useState(lo);
  const [playing, setPlaying] = useState(false);
  const [refused, setRefused] = useState<string | null>(null);
  const [picture, setPicture] = useState<Picture>(initialPicture);
  const [sound, setSound] = useState<Sound>(initialSound);
  const part = parts[index];
  // A part that is not here is never handed to a `<video>`: it would fetch a pointer file.
  const here = part?.here !== false;
  const mainSrc = here && part?.screen ? mediaUrl(part.screen) : null;
  const video = part?.screen?.kind === "video";
  const cameraSrc = here && part?.camera && picture !== "screen" ? mediaUrl(part.camera) : null;
  const [main, attachMain] = useMediaElement<HTMLMediaElement>(mainSrc);
  const [camera, attachCamera] = useMediaElement<HTMLVideoElement>(cameraSrc);
  // A window's start is where the player first lands.
  const pending = useRef<{ local: number; play: boolean } | null>(
    lo > 0 ? { local: locate(media.parts, lo)?.local ?? 0, play: false } : null,
  );
  const at = useRef({ index, playing, onTime, onJump, onPlayingChange });
  at.current = { index, playing, onTime, onJump, onPlayingChange };
  const showMain = video && (picture !== "camera" || cameraSrc === null);
  const choosePicture = media.hasCamera && media.hasScreen;
  const chooseSound = parts.some((p) => p.audioTracks.length > 1);
  useEffect(() => at.current.onPlayingChange?.(playing), [playing]);

  const start = useCallback((element: HTMLMediaElement) => {
    setRefused(null);
    void Promise.resolve(element.play()).catch((cause: unknown) =>
      setRefused(cause instanceof Error ? cause.message : String(cause)),
    );
  }, []);

  const applySound = useCallback(
    (element: HTMLMediaElement | null) => {
      const tracks = (element as { audioTracks?: AudioTracks } | null)?.audioTracks;
      if (!tracks || !part) return;
      for (const track of part.audioTracks) {
        const target = tracks[track.index];
        if (target) target.enabled = sound === "both" || sound === track.origin;
      }
    },
    [part, sound],
  );
  useEffect(() => applySound(main.current), [applySound, main]);

  const report = useCallback((seconds: number) => {
    setTime(seconds);
    at.current.onTime(seconds);
  }, []);

  const seek = useCallback(
    (seconds: number, play?: boolean) => {
      // An end not known yet (no part's length measured) bounds nothing.
      const target = locate(parts, Math.min(Math.max(seconds, lo), hi > 0 ? hi : seconds));
      if (!target) return;
      const keep = play ?? at.current.playing;
      at.current.onJump();
      report(parts[target.index].offset + target.local);
      const element = main.current;
      if (target.index === at.current.index && element && element.readyState >= 1) {
        element.currentTime = target.local;
        if (camera.current) camera.current.currentTime = target.local;
        if (keep) start(element);
        else element.pause();
        return;
      }
      pending.current = { local: target.local, play: keep };
      setIndex(target.index);
    },
    [parts, lo, hi, report, start, main, camera],
  );
  const pause = useCallback(() => main.current?.pause(), [main]);
  useImperativeHandle(ref, () => ({ seek, pause }), [seek, pause]);

  if (!part) return null;
  const partName = part.file.split("/").pop() ?? part.file;
  const mainElementProps = {
    ref: attachMain,
    preload: "metadata",
    playsInline: true,
    "aria-label": partName,
    className: showMain ? VIDEO_BOX : "hidden",
    onLoadedMetadata: (event: React.SyntheticEvent<HTMLMediaElement>) => {
      const element = event.currentTarget;
      applySound(element);
      if (media.parts[index]?.duration === 0 && Number.isFinite(element.duration)) {
        const length = element.duration;
        setMeasured((known) => (known[index] === length ? known : { ...known, [index]: length }));
      }
      const landing = pending.current;
      if (!landing) return;
      pending.current = null;
      element.currentTime = landing.local;
      if (camera.current) camera.current.currentTime = landing.local;
      if (landing.play) start(element);
    },
    onTimeUpdate: (event: React.SyntheticEvent<HTMLMediaElement>) => {
      const element = event.currentTarget;
      const now = part.offset + element.currentTime;
      // A window ends where it says, not where the file does.
      if (window && now >= hi && !element.paused) element.pause();
      report(window ? Math.min(now, hi) : now);
      const follower = camera.current;
      if (follower && Math.abs(follower.currentTime - element.currentTime) > MAX_DRIFT_SECONDS)
        follower.currentTime = element.currentTime;
    },
    onPlay: (event: React.SyntheticEvent<HTMLMediaElement>) => {
      if (event.currentTarget !== main.current) return;
      setPlaying(true);
      if (camera.current) void Promise.resolve(camera.current.play()).catch(() => {});
    },
    onPause: (event: React.SyntheticEvent<HTMLMediaElement>) => {
      if (event.currentTarget !== main.current) return;
      setPlaying(false);
      camera.current?.pause();
    },
    onEnded: () => {
      if (index + 1 < parts.length) {
        pending.current = { local: 0, play: true };
        setIndex(index + 1);
      } else setPlaying(false);
    },
  };
  return (
    <section aria-label={PLAYER_LABEL} className="min-w-0 space-y-2">
      {mainSrc === null ? (
        <p className="text-muted-foreground">
          {here ? PART_UNAVAILABLE_SENTENCE : PART_NOT_HERE_SENTENCE}
        </p>
      ) : (
        <div className="flex min-w-0 justify-center gap-2">
          {video ? (
            <video key={`${index}:${mainSrc}`} {...mainElementProps} />
          ) : (
            <audio key={`${index}:${mainSrc}`} {...mainElementProps} />
          )}
          {cameraSrc && (
            <video
              key={`${index}:${cameraSrc}`}
              ref={attachCamera}
              muted
              preload="metadata"
              playsInline
              aria-label={part.camera?.relativePath.split("/").pop()}
              className={VIDEO_BOX}
              onLoadedMetadata={(event) => {
                const leader = main.current;
                if (!leader) return;
                event.currentTarget.currentTime = leader.currentTime;
                if (!leader.paused)
                  void Promise.resolve(event.currentTarget.play()).catch(() => {});
              }}
            />
          )}
        </div>
      )}
      <div className="flex min-w-0 flex-wrap items-center gap-x-2 gap-y-1">
        <Button
          size="sm"
          variant="outline"
          aria-label={playing ? PAUSE_LABEL : PLAY_LABEL}
          disabled={mainSrc === null}
          onClick={() => {
            const element = main.current;
            if (!element) return;
            if (playing) element.pause();
            // Play at a window's end starts the window again.
            else if (window && time >= hi) seek(lo, true);
            else {
              at.current.onJump();
              start(element);
            }
          }}
        >
          {playing ? PAUSE_GLYPH : PLAY_GLYPH}
        </Button>
        <Button
          size="sm"
          variant="ghost"
          aria-label={BACK_LABEL}
          disabled={mainSrc === null}
          onClick={() => seek(Math.max(lo, time - SKIP_SECONDS))}
        >
          {BACK_GLYPH}
        </Button>
        <Button
          size="sm"
          variant="ghost"
          aria-label={FORWARD_LABEL}
          disabled={mainSrc === null}
          onClick={() => seek(Math.min(hi, time + SKIP_SECONDS))}
        >
          {FORWARD_GLYPH}
        </Button>
        {/* React's `onChange` on a range is the `input` event, so the player —
            and with Follow on, the transcript — moves while the thumb is
            dragged, not only where it is let go. */}
        <input
          type="range"
          aria-label={SCRUB_LABEL}
          className="min-w-24 flex-1 basis-40 accent-primary"
          min={lo}
          max={hi}
          step={0.1}
          value={Math.min(Math.max(time, lo), hi)}
          onChange={(event) => seek(Number(event.target.value))}
        />
        <span className="font-mono text-muted-foreground text-xs tabular-nums">
          {clock(time)} / {clock(hi)}
        </span>
        <div className="ml-auto flex items-center gap-1">
          {choosePicture && (
            <ChoiceGroup
              label={PICTURE_LABEL}
              value={picture}
              choices={PICTURE_CHOICES}
              onChange={setPicture}
            />
          )}
          {chooseSound && (
            <ChoiceGroup
              label={SOUND_LABEL}
              value={sound}
              choices={SOUND_CHOICES}
              onChange={setSound}
            />
          )}
          {pinned !== undefined && onPinnedChange && (
            <IconHint label={stateHint(PIN_LABEL, pinned)}>
              <Toggle
                aria-label={PIN_LABEL}
                className={TOGGLE_STATE}
                pressed={pinned}
                onPressedChange={onPinnedChange}
              >
                {pinned ? <Pin aria-hidden="true" /> : <PinOff aria-hidden="true" />}
              </Toggle>
            </IconHint>
          )}
          <IconHint label={stateHint(FOLLOW_LABEL, follow)}>
            <Toggle
              aria-label={FOLLOW_LABEL}
              className={TOGGLE_STATE}
              pressed={follow}
              onPressedChange={onFollowChange}
            >
              <Crosshair aria-hidden="true" />
            </Toggle>
          </IconHint>
        </div>
      </div>
      <div className="flex min-w-0 flex-wrap items-baseline gap-x-3 gap-y-1 text-muted-foreground text-xs">
        {meta}
        <p className="ml-auto min-w-0 break-words">
          {parts.length > 1 && `Part ${index + 1} of ${parts.length} · `}
          <span className="font-mono">{partName}</span>
        </p>
      </div>
      {refused && (
        <p role="alert" className="break-words text-destructive" title={refused}>
          {PLAY_REFUSED_LABEL}
        </p>
      )}
    </section>
  );
}
