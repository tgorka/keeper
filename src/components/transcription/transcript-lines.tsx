/**
 * A transcript's lines, read: one row per line with the speaker's square — a
 * play button — the name, the start and the length, the line's ⋯ and the words.
 *
 * The transcript viewer draws them — in its dialog and in a note's media block
 * alike — with its corrections behind the ⋯.
 */
import { Ellipsis, Play } from "lucide-react";
import { type ReactNode, type Ref, useCallback } from "react";
import { Button } from "@/components/ui/button";
import { DropdownMenuTrigger } from "@/components/ui/dropdown-menu";
import { useWindowedRows, type WindowedRows } from "@/components/ui/window-list";
import type { Speaker } from "@/lib/ipc/gen/Speaker";
import type { Utterance } from "@/lib/ipc/gen/Utterance";
import { cn } from "@/lib/utils";

/** What a row draws of a line: an `Utterance` is one, and so is a block's line. */
export type LineVm = Pick<Utterance, "id" | "speaker" | "start" | "end" | "text">;
/** What a row draws of a speaker. */
export type LineSpeakerVm = Pick<Speaker, "id" | "name">;

/**
 * A speaker's ink, by their place in the transcript, from the bounded palette
 * the gate measures in both themes. Always drawn beside the name, never alone.
 * Literal class names, because Tailwind only generates what it can read.
 */
const SPEAKER_INKS = [
  "bg-bot-ink-lapis",
  "bg-bot-ink-clay",
  "bg-bot-ink-verdigris",
  "bg-bot-ink-ochre",
  "bg-bot-ink-steel",
  "bg-bot-ink-madder",
  "bg-bot-ink-olive",
] as const;

/** Each speaker's ink, by their place in `speakers`. */
export function speakerInks(speakers: readonly LineSpeakerVm[]): Map<string, string> {
  return new Map(speakers.map((s, index) => [s.id, SPEAKER_INKS[index % SPEAKER_INKS.length]]));
}

export function speakerName(speaker: LineSpeakerVm): string {
  return speaker.name ?? (speaker.id === "ME" ? "You" : `Speaker ${speaker.id.replace(/^S/, "")}`);
}

/** `hh:mm:ss`, the clock a line's start is read on. */
export function timestamp(seconds: number): string {
  const n = Math.floor(seconds);
  return `${Math.floor(n / 3600)
    .toString()
    .padStart(2, "0")}:${Math.floor((n / 60) % 60)
    .toString()
    .padStart(2, "0")}:${(n % 60).toString().padStart(2, "0")}`;
}

/** How long a line lasts, in whole seconds: "21 s", and from a minute "1 min 05 s". */
export function lineLength(seconds: number): string {
  const n = Math.max(0, Math.round(seconds));
  if (n < 60) return `${n} s`;
  return `${Math.floor(n / 60)} min ${(n % 60).toString().padStart(2, "0")} s`;
}

/** A menu opened from an icon is as wide as its words, not as its trigger. */
export const MENU_CONTENT = "w-auto min-w-44";
export const PLAY_FROM_HERE = "Play from here";
export const COPY_CLIP_FROM_HERE = "Copy clip from here…";
/** A line's rows are assumed this tall until measured. */
const ROW_HEIGHT = 84;
const ROW_GAP = 4;

/**
 * The line's ⋯: quiet until the line is pointed at or holds focus, never gone —
 * the reader sees there is more to a line without hunting for it.
 */
export function LineMenuTrigger({ id }: { id: string }) {
  return (
    <DropdownMenuTrigger asChild>
      <Button
        size="icon-xs"
        variant="ghost"
        aria-label={`Line actions ${id}`}
        className="text-faint group-focus-within:text-foreground group-hover:text-foreground data-[state=open]:text-foreground"
      >
        <Ellipsis aria-hidden="true" />
      </Button>
    </DropdownMenuTrigger>
  );
}

/**
 * The speaker's square as the line's play button: a small ▶ always drawn in it,
 * brought forward when the square is pointed at or focused. Only a square where
 * nothing can be played.
 */
function PlaySquare({
  line,
  ink,
  playable,
  onSeek,
}: {
  line: LineVm;
  ink: string | undefined;
  playable: boolean;
  onSeek: (seconds: number, play?: boolean) => void;
}) {
  if (!playable)
    return <span aria-hidden="true" className={cn("size-2 shrink-0 rounded-[2px]", ink)} />;
  return (
    <button
      type="button"
      aria-label={`Play from ${timestamp(line.start)}`}
      className={cn(
        "group/play inline-flex size-4 shrink-0 items-center justify-center rounded-[3px] text-background outline-none focus-visible:ring-2 focus-visible:ring-ring",
        ink ?? "bg-muted-foreground",
      )}
      onClick={() => onSeek(line.start, true)}
    >
      <Play
        aria-hidden="true"
        className="size-2 fill-current opacity-50 transition-[opacity,width,height] group-hover/play:size-2.5 group-hover/play:opacity-100 group-focus-visible/play:size-2.5 group-focus-visible/play:opacity-100"
      />
    </button>
  );
}

export interface TranscriptLinesProps {
  lines: readonly LineVm[];
  /** In the transcript's order, which is what gives each its ink. */
  speakers: readonly LineSpeakerVm[];
  list: WindowedRows<string>;
  listRef?: Ref<HTMLOListElement>;
  /** The index of the line being said, or -1. */
  current: number;
  playable: boolean;
  onSeek: (seconds: number, play?: boolean) => void;
  /** The line's ⋯: a `DropdownMenu` holding a {@link LineMenuTrigger}. */
  menu: (line: LineVm, index: number) => ReactNode;
  /** In place of the words (an edit field); `undefined` draws the words. */
  body?: (line: LineVm, index: number) => ReactNode;
  /** Beside the time (the viewer's "Edited" note). */
  note?: (line: LineVm, index: number) => ReactNode;
  /** Under the words (the viewer's split and insert panels). */
  after?: (line: LineVm, index: number) => ReactNode;
  /** Extra classes for a row's card: a search match's ring. */
  cardClassName?: (index: number) => string | false | undefined;
  /** `data-match` on the rows that match the reader's search. */
  matching?: ReadonlySet<number>;
}

/** The windowed `<ol>` of lines; its scroll box and window belong to the caller. */
export function TranscriptLines({
  lines,
  speakers,
  list,
  listRef,
  current,
  playable,
  onSeek,
  menu,
  body,
  note,
  after,
  cardClassName,
  matching,
}: TranscriptLinesProps) {
  const inks = speakerInks(speakers);
  const names = new Map(speakers.map((s) => [s.id, speakerName(s)]));
  return (
    <ol
      ref={listRef}
      aria-label="Utterances"
      className="relative"
      style={{ height: list.totalSize }}
    >
      {list.rows.map((row) => {
        const line = lines[row.index];
        if (!line) return null;
        const speaking = row.index === current;
        return (
          // No inset of its own: the card's edge is the column's edge, so the
          // words run as wide as the player above them.
          <li
            key={row.key}
            {...list.rowProps(row)}
            aria-current={speaking ? "true" : undefined}
            data-match={matching?.has(row.index) ? "" : undefined}
            className="group min-w-0"
          >
            <div
              className={cn(
                "min-w-0 rounded-md border border-transparent px-3 py-2",
                speaking && "border-primary bg-secondary",
                cardClassName?.(row.index),
              )}
            >
              <div className="flex min-w-0 items-center gap-2 text-muted-foreground text-xs">
                <PlaySquare
                  line={line}
                  ink={inks.get(line.speaker)}
                  playable={playable}
                  onSeek={onSeek}
                />
                <span className="min-w-0 truncate font-medium">
                  {names.get(line.speaker) ?? line.speaker}
                </span>
                <span className="shrink-0 font-mono tabular-nums">
                  {playable ? (
                    <button
                      type="button"
                      className="rounded-sm hover:text-foreground focus-visible:outline focus-visible:outline-ring"
                      aria-label={`Go to ${timestamp(line.start)}, ${line.id}`}
                      onClick={() => onSeek(line.start)}
                    >
                      {timestamp(line.start)}
                    </button>
                  ) : (
                    timestamp(line.start)
                  )}
                  {` · ${lineLength(line.end - line.start)}`}
                </span>
                {menu(line, row.index)}
                {note?.(line, row.index)}
              </div>
              {body?.(line, row.index) ?? (
                <p className="mt-1 whitespace-pre-wrap break-words font-normal text-foreground text-title leading-relaxed">
                  {line.text}
                </p>
              )}
              {after?.(line, row.index)}
            </div>
          </li>
        );
      })}
    </ol>
  );
}

/** The windowed rows over `lines`, keyed by line id. */
export function useTranscriptRows(
  lines: readonly LineVm[] | undefined,
  options: { scrollMargin?: number; stickyInset?: number; scroller?: HTMLElement | null } = {},
): WindowedRows<string> {
  const getKey = useCallback((index: number) => lines?.[index]?.id ?? String(index), [lines]);
  return useWindowedRows({
    count: lines?.length ?? 0,
    getKey,
    rowHeight: ROW_HEIGHT,
    gap: ROW_GAP,
    scrollMargin: options.scrollMargin ?? 0,
    stickyInset: options.stickyInset ?? 0,
    scroller: options.scroller,
  });
}
