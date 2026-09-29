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
import { type Ref, useCallback, useEffect, useImperativeHandle, useRef, useState } from "react";
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
  releaseMediaElement,
  SCRUB_LABEL,
  SKIP_SECONDS,
} from "@/components/notes/editor/recording-transport";
import { Button } from "@/components/ui/button";
import type { TranscriptMediaVm } from "@/lib/ipc/client";
import { cn } from "@/lib/utils";
import { fileAssetUrl } from "@/lib/viewers/file-asset-url";
import { locate, sessionLength } from "./session-timeline";
import { TRANSCRIPTION_SELECT } from "./transcript-viewer";

export const PLAYER_LABEL = "Transcript player";
export const PICTURE_LABEL = "Picture";
export const SOUND_LABEL = "Sound";
export const PIN_LABEL = "Keep the player on top";
export const FOLLOW_LABEL = "Follow the transcript";
export const PART_UNAVAILABLE_SENTENCE =
  "This part is not in a synced folder, so keeper cannot play it here.";

export interface TranscriptPlayerHandle {
  /** Move to `seconds` on the transcript's timeline; `play` starts it, absent keeps it as it was. */
  seek: (seconds: number, play?: boolean) => void;
}

type Picture = "screen" | "camera" | "both";
type Sound = "system" | "microphone" | "both";

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
  ref,
}: {
  media: TranscriptMediaVm;
  pinned: boolean;
  onPinnedChange: (pinned: boolean) => void;
  follow: boolean;
  onFollowChange: (follow: boolean) => void;
  /** Where the timeline is, on every tick and every seek. */
  onTime: (seconds: number) => void;
  /** A play or a seek: the reader moved the player on purpose. */
  onJump: () => void;
  ref?: Ref<TranscriptPlayerHandle>;
}) {
  const parts = media.parts;
  const total = sessionLength(parts);
  const [index, setIndex] = useState(0);
  const [time, setTime] = useState(0);
  const [playing, setPlaying] = useState(false);
  const [refused, setRefused] = useState<string | null>(null);
  const [picture, setPicture] = useState<Picture>("both");
  const [sound, setSound] = useState<Sound>("both");
  const part = parts[index];
  const mainSrc = part?.screen
    ? fileAssetUrl(part.screen.profileId, part.screen.relativePath)
    : null;
  const video = part?.screen?.kind === "video";
  const cameraSrc =
    part?.camera && picture !== "screen"
      ? fileAssetUrl(part.camera.profileId, part.camera.relativePath)
      : null;
  const [main, attachMain] = useMediaElement<HTMLMediaElement>(mainSrc);
  const [camera, attachCamera] = useMediaElement<HTMLVideoElement>(cameraSrc);
  const pending = useRef<{ local: number; play: boolean } | null>(null);
  const at = useRef({ index, playing, onTime, onJump });
  at.current = { index, playing, onTime, onJump };
  const showMain = video && (picture !== "camera" || cameraSrc === null);
  const choosePicture = media.hasCamera && media.hasScreen;
  const chooseSound = parts.some((p) => p.audioTracks.length > 1);

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
      const target = locate(parts, seconds);
      if (!target) return;
      const keep = play ?? at.current.playing;
      at.current.onJump();
      report(parts[target.index].offset + target.local);
      const element = main.current;
      if (target.index === at.current.index && element && element.readyState >= 1) {
        element.currentTime = target.local;
        if (camera.current) camera.current.currentTime = target.local;
        if (keep) start(element);
        return;
      }
      pending.current = { local: target.local, play: keep };
      setIndex(target.index);
    },
    [parts, report, start, main, camera],
  );
  useImperativeHandle(ref, () => ({ seek }), [seek]);

  if (!part) return null;
  const partName = part.file.split("/").pop() ?? part.file;
  const mainElementProps = {
    ref: attachMain,
    preload: "metadata",
    playsInline: true,
    "aria-label": partName,
    className: showMain
      ? "max-h-[30dvh] min-w-0 flex-1 rounded-md bg-muted object-contain"
      : "hidden",
    onLoadedMetadata: (event: React.SyntheticEvent<HTMLMediaElement>) => {
      const element = event.currentTarget;
      applySound(element);
      const landing = pending.current;
      if (!landing) return;
      pending.current = null;
      element.currentTime = landing.local;
      if (camera.current) camera.current.currentTime = landing.local;
      if (landing.play) start(element);
    },
    onTimeUpdate: (event: React.SyntheticEvent<HTMLMediaElement>) => {
      const element = event.currentTarget;
      report(part.offset + element.currentTime);
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
        <p className="text-muted-foreground">{PART_UNAVAILABLE_SENTENCE}</p>
      ) : (
        <div className="flex min-w-0 gap-2">
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
              className="max-h-[30dvh] min-w-0 flex-1 rounded-md bg-muted object-contain"
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
      <div className="flex min-w-0 flex-wrap items-center gap-1">
        <Button
          size="sm"
          variant="outline"
          aria-label={playing ? PAUSE_LABEL : PLAY_LABEL}
          disabled={mainSrc === null}
          onClick={() => {
            const element = main.current;
            if (!element) return;
            if (playing) element.pause();
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
          onClick={() => seek(Math.max(0, time - SKIP_SECONDS))}
        >
          {BACK_GLYPH}
        </Button>
        <Button
          size="sm"
          variant="ghost"
          aria-label={FORWARD_LABEL}
          disabled={mainSrc === null}
          onClick={() => seek(Math.min(total, time + SKIP_SECONDS))}
        >
          {FORWARD_GLYPH}
        </Button>
        <input
          type="range"
          aria-label={SCRUB_LABEL}
          className="min-w-24 flex-1 accent-primary"
          min={0}
          max={total}
          step={0.1}
          value={Math.min(time, total)}
          onChange={(event) => seek(Number(event.target.value))}
        />
        <span className="font-mono text-xs tabular-nums">
          {clock(time)} / {clock(total)}
        </span>
      </div>
      <div className="flex min-w-0 flex-wrap items-center gap-2">
        {choosePicture && (
          <select
            aria-label={PICTURE_LABEL}
            className={cn(TRANSCRIPTION_SELECT, "w-auto")}
            value={picture}
            onChange={(event) => setPicture(event.target.value as Picture)}
          >
            <option value="both">Screen and camera</option>
            <option value="screen">Screen</option>
            <option value="camera">Camera</option>
          </select>
        )}
        {chooseSound && (
          <select
            aria-label={SOUND_LABEL}
            className={cn(TRANSCRIPTION_SELECT, "w-auto")}
            value={sound}
            onChange={(event) => setSound(event.target.value as Sound)}
          >
            <option value="both">Call and microphone</option>
            <option value="system">Call</option>
            <option value="microphone">Microphone</option>
          </select>
        )}
        <Button
          size="sm"
          variant={pinned ? "secondary" : "ghost"}
          aria-pressed={pinned}
          onClick={() => onPinnedChange(!pinned)}
        >
          {PIN_LABEL}
        </Button>
        <Button
          size="sm"
          variant={follow ? "secondary" : "ghost"}
          aria-pressed={follow}
          onClick={() => onFollowChange(!follow)}
        >
          {FOLLOW_LABEL}
        </Button>
      </div>
      <p className="min-w-0 break-words text-muted-foreground text-xs">
        {parts.length > 1 && `Part ${index + 1} of ${parts.length} · `}
        <span className="font-mono">{partName}</span>
      </p>
      {refused && (
        <p role="alert" className="break-words text-destructive" title={refused}>
          {PLAY_REFUSED_LABEL}
        </p>
      )}
    </section>
  );
}
