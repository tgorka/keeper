import {
  ChevronRight,
  Clipboard,
  Crosshair,
  Ellipsis,
  ListPlus,
  Pencil,
  Play,
  RotateCcw,
  SkipForward,
  Split,
  UserPlus,
  UserRoundPen,
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
import { Button } from "@/components/ui/button";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuLabel,
  DropdownMenuRadioGroup,
  DropdownMenuRadioItem,
  DropdownMenuSeparator,
  DropdownMenuSub,
  DropdownMenuSubContent,
  DropdownMenuSubTrigger,
  DropdownMenuTrigger,
} from "@/components/ui/dropdown-menu";
import { Input } from "@/components/ui/input";
import { Lamp, type LampState } from "@/components/ui/lamp";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import { Tabs, TabsList, TabsTrigger } from "@/components/ui/tabs";
import { ToggleGroup, ToggleGroupItem } from "@/components/ui/toggle-group";
import { TextFileViewer } from "@/components/viewers/text-file-viewer";
import { TextEditorSurface } from "@/components/viewers/text-viewer";
import {
  dictionaryAcceptSuggestion,
  listenTranscriptWritten,
  type MediaClipVm,
  type MediaWindowVm,
  type TrackOrigin,
  type TranscriptMediaVm,
  type TranscriptVm,
  transcriptAddSpeaker,
  transcriptAssignSpeaker,
  transcriptClip,
  transcriptEditUtterance,
  transcriptInsertUtterance,
  transcriptMedia,
  transcriptMergeSpeakers,
  transcriptRead,
  transcriptReassignUtterance,
  transcriptRenameSpeaker,
  transcriptSplitUtterance,
} from "@/lib/ipc/client";
import type { DictionarySuggestion } from "@/lib/ipc/gen/DictionarySuggestion";
import type { Speaker } from "@/lib/ipc/gen/Speaker";
import type { Transcript } from "@/lib/ipc/gen/Transcript";
import type { Utterance } from "@/lib/ipc/gen/Utterance";
import type { VoicesDriveVm } from "@/lib/ipc/gen/VoicesDriveVm";
import { syncErrorMessage } from "@/lib/stores/sync";
import {
  refreshTranscription,
  startTranscription,
  transcriptionRunning,
  transcriptionStore,
  useTranscriptionStore,
} from "@/lib/stores/transcription";
import { cn } from "@/lib/utils";
import { FILE_FORMATS } from "@/lib/viewers/registry";
import type { ViewerProps } from "@/lib/viewers/types";
import { type ClipRequest, CopyClipDialog } from "./copy-clip";
import { currentUtterance, nearestLine, nextLine } from "./session-timeline";
import { TRANSCRIBE_AGAIN_LABEL, TranscribeAgainDialog } from "./transcribe-again";
import {
  COPY_CLIP_FROM_HERE,
  LineMenuTrigger,
  MENU_CONTENT,
  PLAY_FROM_HERE,
  speakerInks,
  speakerName,
  TranscriptLines,
  timestamp,
  useTranscriptRows,
} from "./transcript-lines";
import {
  type Picture,
  type PlayerWindow,
  type Sound,
  TranscriptPlayer,
  type TranscriptPlayerHandle,
} from "./transcript-player";
import { TranscriptionJob } from "./transcription-job";

export const TRANSCRIPTION_SELECT =
  "h-9 w-full min-w-0 rounded-md border border-input bg-background px-2 text-sm focus-visible:ring-2 focus-visible:ring-ring";
const STATUS = {
  auto: "Matched",
  suggested: "Suggested",
  confirmed: "Confirmed",
  unknown: "Unknown",
  self: "You",
} as const;
/**
 * A speaker's status as a lamp: named by a person (or the one recording) is
 * lit, a guess waiting for a yes is working, an unnamed voice is idle.
 */
const STATUS_LAMP: Record<Speaker["status"], LampState> = {
  confirmed: "live",
  self: "live",
  auto: "working",
  suggested: "working",
  unknown: "idle",
};
/**
 * The speakers a line can be given, each once: every speaker with lines, and a
 * lineless one only when it names somebody no other speaker does — an unnamed
 * voice, or one just added. A lineless speaker whose person another speaker
 * already carries would list that person twice.
 */
export function offeredSpeakers(transcript: Transcript): Speaker[] {
  const speaking = new Set(transcript.utterances.map((utterance) => utterance.speaker));
  const carried = new Set(
    transcript.speakers.flatMap((s) => (speaking.has(s.id) && s.personId ? [s.personId] : [])),
  );
  return transcript.speakers.filter((speaker) => {
    if (speaking.has(speaker.id) || speaker.personId === null) return true;
    if (carried.has(speaker.personId)) return false;
    carried.add(speaker.personId);
    return true;
  });
}
/** The tracks a transcript heard, in the order a speaker can be added to them. */
function heardOrigins(transcript: Transcript): TrackOrigin[] {
  const heard = new Set<TrackOrigin>([
    ...transcript.source.parts.flatMap((part) => part.tracks.map((track) => track.origin)),
    ...transcript.speakers.map((speaker) => speaker.origin),
  ]);
  return (["system", "microphone", "mixed"] as const).filter((origin) => heard.has(origin));
}
const ORIGIN_NAME: Record<TrackOrigin, string> = {
  system: "Call",
  microphone: "Microphone",
  mixed: "Mixed",
};
/** `text` with every case-insensitive occurrence of `needle` in a `<mark>`, keyed by offset. */
function marked(text: string, needle: string): ReactNode {
  if (!needle) return text;
  const lower = text.toLocaleLowerCase();
  const runs: ReactNode[] = [];
  let from = 0;
  for (let at = lower.indexOf(needle); at >= 0; at = lower.indexOf(needle, at + needle.length)) {
    if (at > from) runs.push(text.slice(from, at));
    runs.push(
      <mark
        key={at}
        className="bg-[var(--search-highlight)] text-[var(--search-highlight-foreground)]"
      >
        {text.slice(at, at + needle.length)}
      </mark>,
    );
    from = at + needle.length;
  }
  if (from < text.length) runs.push(text.slice(from));
  return runs;
}
export const SEARCH_LABEL = "Search the transcript";
export const SOURCE_TAB = "Source";
export const TRANSCRIPT_TAB = "Transcript";
export const ADD_SPEAKER_LABEL = "Add speaker";
export const TRANSCRIPT_ACTIONS_LABEL = "Transcript actions";
export const COPY_AS_NOTE_EMBED = "Copy as note embed";
export const NEAREST_LINE_LABEL = "Go to their nearest line";
export const NEXT_LINE_LABEL = "Go to their next line";
/** Keys that scroll a focused box; pressing one is the reader taking the scroll back. */
const SCROLL_KEYS: Record<string, true> = {
  ArrowUp: true,
  ArrowDown: true,
  PageUp: true,
  PageDown: true,
  Home: true,
  End: true,
  " ": true,
};
/**
 * The voices drive a transcript's corrections belong to: the one named, else
 * the drive whose folder holds the transcript, else the first (Rust's
 * `drive_for_path` falls back the same way). The folder test is a whole-segment
 * prefix so `/Volumes/work2/…` is never taken for the drive at `/Volumes/work`.
 */
export function voicesDriveFor(
  drives: readonly VoicesDriveVm[] | undefined,
  path: string,
  profileId: string | null,
): VoicesDriveVm | undefined {
  const holds = (drive: VoicesDriveVm) => {
    const root = drive.localPath.replace(/[\\/]+$/, "");
    return (
      root !== "" &&
      path.length > root.length &&
      path.startsWith(root) &&
      (path[root.length] === "/" || path[root.length] === "\\")
    );
  };
  return (
    drives?.find((drive) => drive.profileId === profileId) ?? drives?.find(holds) ?? drives?.[0]
  );
}
/**
 * A file named like a transcript that keeper cannot read as one (an unrelated
 * `transcript.json`, a newer version, no local path) still opens: as the JSON
 * text it is.
 */
export function TranscriptFileViewer(props: ViewerProps) {
  return <TranscriptOrText key={props.file.absolutePath ?? props.file.relativePath} {...props} />;
}
function TranscriptOrText({ file, frame }: ViewerProps) {
  const [unreadable, setUnreadable] = useState(false);
  const text = FILE_FORMATS.get("json");
  if (text && (unreadable || !file.absolutePath))
    return <TextFileViewer file={file} entry={text} frame={frame} />;
  if (!file.absolutePath)
    return <p className="p-3 text-sm">This transcript has no local file to open.</p>;
  return (
    <TranscriptViewer
      path={file.absolutePath}
      profileId={file.profileId}
      onUnreadable={() => setUnreadable(true)}
    />
  );
}
export function TranscriptDialog({
  path,
  onClose,
  profileId = null,
  at,
}: {
  path: string | null;
  onClose: () => void;
  profileId?: string | null;
  /** Seconds to open the player at, paused: where a note's block was. */
  at?: number;
}) {
  return (
    <Dialog
      open={path !== null}
      onOpenChange={(open) => {
        if (!open) onClose();
      }}
    >
      {/* Most of the window: the player can use the width, and the lines run
          as wide as it. */}
      <DialogContent className="flex h-[92dvh] w-[min(96vw,1600px)] min-w-0 max-w-none flex-col sm:max-w-none">
        <DialogHeader>
          <DialogTitle>Transcript</DialogTitle>
          <DialogDescription>
            Read and correct what was said. Recognition stays on this Mac.
          </DialogDescription>
        </DialogHeader>
        {path && <TranscriptViewer key={path} path={path} profileId={profileId} at={at} />}
      </DialogContent>
    </Dialog>
  );
}
/** What a host holding the viewer can ask of its player. */
export interface TranscriptViewerHandle {
  /** Move to `seconds`; `play` starts it, absent keeps it as it was. Held until the player is there. */
  seek: (seconds: number, play?: boolean) => void;
  pause: () => void;
}
export interface TranscriptViewerProps {
  path: string;
  profileId?: string | null;
  /** Called instead of showing the error when the file cannot be read as a transcript. */
  onUnreadable?: () => void;
  /** Seconds the player is put at, paused, once it can play. */
  at?: number;
  /**
   * The element that scrolls the viewer when it is not its own — a note
   * editor's scroller. Given (even `null`, not known yet), the viewer grows
   * with its lines inside the host's flow and has no scroll box, no Source tab:
   * the lines window against this scroller and the pinned block sticks to it.
   */
  scroller?: HTMLElement | null;
  /** Only the lines overlapping `[from, to)`, and a player that plays only there. */
  window?: MediaWindowVm;
  /** The heading: absent is what the transcript is of, `null` none. */
  title?: string | null;
  /** What to play, instead of asking for the transcript's own media. */
  media?: TranscriptMediaVm;
  initialPicture?: Picture;
  initialSound?: Sound;
  /** `false` keeps every `<video>` unmounted; `asleepText` holds the player's place. */
  playerAwake?: boolean;
  asleepText?: string;
  /** Under the player's own rows, above the speakers: a note block's markers. */
  markers?: ReactNode;
  /** More `DropdownMenuItem`s at the end of the viewer's ⋯. */
  menuItems?: ReactNode;
  /** What a Copy clip composes; the transcript's own clip unless given. */
  composeClip?: (from: string | null, to: string | null, words: boolean) => Promise<MediaClipVm>;
  onTime?: (seconds: number) => void;
  onPlayingChange?: (playing: boolean) => void;
  ref?: Ref<TranscriptViewerHandle>;
}
export function TranscriptViewer({
  path,
  profileId = null,
  onUnreadable,
  at,
  scroller,
  window,
  title,
  media: givenMedia,
  initialPicture,
  initialSound,
  playerAwake = true,
  asleepText,
  markers,
  menuItems,
  composeClip,
  onTime,
  onPlayingChange,
  ref,
}: TranscriptViewerProps) {
  const embedded = scroller !== undefined;
  const [vm, setVm] = useState<TranscriptVm | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const [suggestions, setSuggestions] = useState<DictionarySuggestion[]>([]);
  const [editing, setEditing] = useState<string | null>(null);
  const [draft, setDraft] = useState("");
  const [panel, setPanel] = useState<{ id: string; kind: "split" | "insert" } | null>(null);
  const [naming, setNaming] = useState<{ speakerId: string; mode: "rename" | "new" } | null>(null);
  const [tab, setTab] = useState<"transcript" | "source">("transcript");
  const [fetchedMedia, setFetchedMedia] = useState<TranscriptMediaVm | null>(null);
  const [mediaError, setMediaError] = useState<string | null>(null);
  const [pinned, setPinned] = useState(true);
  const [follow, setFollow] = useState(true);
  /** Where the player is, or null until it has been played or moved. */
  const [time, setTime] = useState<number | null>(null);
  /** Off from the reader's own scroll until the next play or seek. */
  const [followScroll, setFollowScroll] = useState(true);
  const [query, setQuery] = useState("");
  const [activeMatch, setActiveMatch] = useState(-1);
  const [added, setAdded] = useState<string | null>(null);
  const [replacing, setReplacing] = useState(false);
  /** Bumped when a Transcribe again has written the file anew, to read it again. */
  const [reads, setReads] = useState(0);
  const [clip, setClip] = useState<ClipRequest | null>(null);
  const player = useRef<TranscriptPlayerHandle | null>(null);
  /** The seek still to be applied, once the transcript and its player are here. */
  const landing = useRef<{ at: number; play: boolean } | undefined>(
    at === undefined ? undefined : { at, play: false },
  );
  const search = useRef<HTMLInputElement | null>(null);
  const prelude = useRef<HTMLDivElement | null>(null);
  const pinnedBox = useRef<HTMLDivElement | null>(null);
  const listBox = useRef<HTMLOListElement | null>(null);
  const ownBox = useRef<HTMLDivElement | null>(null);
  const editField = useRef<HTMLTextAreaElement | null>(null);
  const nameField = useRef<HTMLInputElement | null>(null);
  const [geometry, setGeometry] = useState({ margin: 0, inset: 0 });
  const generation = useRef(0);
  const drives = useTranscriptionStore((s) => s.status?.voicesDrives);
  const canTranscribe = useTranscriptionStore((s) => s.status?.available === true);
  const sourcePath = vm?.sourcePath ?? null;
  const redoJob = useTranscriptionStore((s) => (sourcePath ? s.jobs[sourcePath] : undefined));
  const bank = voicesDriveFor(drives, path, profileId);
  const unreadable = useRef(onUnreadable);
  unreadable.current = onUnreadable;
  const hooks = useRef({ onTime, onPlayingChange });
  hooks.current = { onTime, onPlayingChange };
  // biome-ignore lint/correctness/useExhaustiveDependencies: `reads` is the request to read the same path again
  useEffect(() => {
    const mine = ++generation.current;
    setVm(null);
    setError(null);
    setEditing(null);
    setPanel(null);
    setNaming(null);
    setBusy(false);
    setSuggestions([]);
    void refreshTranscription();
    void transcriptRead(path)
      .then((next) => {
        if (generation.current === mine && next.path === path) setVm(next);
      })
      .catch((cause: unknown) => {
        if (generation.current !== mine) return;
        if (unreadable.current) unreadable.current();
        else setError(syncErrorMessage(cause));
      });
    setTime(null);
    setAdded(null);
    return () => {
      generation.current += 1;
    };
  }, [path, reads]);
  // A host that knows what to play (a note's block) says so; otherwise the
  // transcript's own media is asked for.
  const ownMedia = givenMedia === undefined;
  // biome-ignore lint/correctness/useExhaustiveDependencies: `reads` is the request to read the same path again
  useEffect(() => {
    setFetchedMedia(null);
    setMediaError(null);
    if (!ownMedia) return;
    let live = true;
    void transcriptMedia(path)
      .then((next) => {
        if (live) setFetchedMedia(next);
      })
      .catch((cause: unknown) => {
        if (live) setMediaError(syncErrorMessage(cause));
      });
    return () => {
      live = false;
    };
  }, [path, reads, ownMedia]);
  const media = givenMedia ?? fetchedMedia;
  // Rust wrote this transcript — a job, a correction here or in another window,
  // a redo — so the lines are read again, keeping everything the reader holds.
  useEffect(() => {
    let unlisten: (() => void) | null = null;
    let live = true;
    void listenTranscriptWritten((written) => {
      if (written !== path) return;
      const mine = generation.current;
      void transcriptRead(path)
        .then((next) => {
          if (generation.current === mine && next.path === path) setVm(next);
        })
        .catch(() => {});
    })
      .then((stop) => {
        if (live) unlisten = stop;
        else stop();
      })
      .catch(() => {});
    return () => {
      live = false;
      unlisten?.();
    };
  }, [path]);
  const act = async (operation: () => Promise<TranscriptVm>) => {
    if (busy) return;
    const mine = generation.current;
    setBusy(true);
    setError(null);
    try {
      const next = await operation();
      if (mine === generation.current && next.path === path) setVm(next);
      // Assigning and merging change the bank; Settings' People list reads it here.
      if (bank)
        transcriptionStore.setState((s) => ({
          people: { ...s.people, [bank.profileId]: next.people },
        }));
    } catch (cause) {
      if (mine === generation.current) setError(syncErrorMessage(cause));
    } finally {
      if (mine === generation.current) setBusy(false);
    }
  };
  const save = (id: string) => {
    const mine = generation.current;
    return act(async () => {
      const result = await transcriptEditUtterance(path, id, draft);
      if (generation.current === mine && result.transcript.path === path) {
        setSuggestions(result.suggestions);
        setEditing(null);
      }
      return result.transcript;
    });
  };
  /** Split and insert: the panel closes on success and stays open with the sentence on a refusal. */
  const changeLines = (operation: () => Promise<TranscriptVm>) => {
    const mine = generation.current;
    return act(async () => {
      const next = await operation();
      if (generation.current === mine && next.path === path) setPanel(null);
      return next;
    });
  };
  const windowFrom = window?.from ?? 0;
  const windowTo = window?.to ?? null;
  const whole = windowFrom === 0 && windowTo === null;
  /**
   * The lines shown: all of them, or those overlapping a block's window
   * `[from, to)` as Rust's `overlaps` reads it. Every index below is into this.
   */
  const utterances = useMemo(() => {
    const all = vm?.transcript.utterances;
    if (!all || whole) return all;
    return all.filter((u) => u.end > windowFrom && (windowTo === null || u.start < windowTo));
  }, [vm, whole, windowFrom, windowTo]);
  const duration = vm?.transcript.duration ?? 0;
  const playerWindow = useMemo<PlayerWindow | undefined>(
    () => (whole ? undefined : { from: windowFrom, to: windowTo ?? duration }),
    [whole, windowFrom, windowTo, duration],
  );
  const list = useTranscriptRows(utterances, {
    scrollMargin: geometry.margin,
    stickyInset: geometry.inset,
    scroller: embedded ? scroller : undefined,
  });
  const reveal = list.reveal;
  const viewportRef = list.viewportProps.ref;
  const attachOwnBox = useCallback(
    (element: HTMLDivElement | null) => {
      ownBox.current = element;
      return viewportRef(element);
    },
    [viewportRef],
  );
  const showsBody = vm !== null && tab === "transcript";
  const playable = media !== null && media.parts.length > 0;
  const awake = playable && playerAwake;
  const sticks = pinned && playable;
  useImperativeHandle(
    ref,
    () => ({
      seek: (seconds, play) => {
        if (player.current) player.current.seek(seconds, play);
        else landing.current = { at: seconds, play: play ?? false };
      },
      pause: () => player.current?.pause(),
    }),
    [],
  );
  useEffect(() => {
    if (landing.current === undefined || !vm || !awake || !player.current) return;
    player.current.seek(landing.current.at, landing.current.play);
    landing.current = undefined;
  }, [vm, awake]);
  // The list starts below the heading and the pinned block, all in the one
  // scroll box — the viewer's own, or the note's — and a pinned block covers
  // the top of it. Only where the list sits is read, never set: nothing these
  // numbers change resizes what is observed, so a resize cannot feed itself.
  useEffect(() => {
    const box = embedded ? scroller : ownBox.current;
    if (!showsBody || !box) return;
    const measure = () => {
      const listTop = listBox.current?.getBoundingClientRect().top;
      const margin =
        listTop === undefined
          ? 0
          : listTop - box.getBoundingClientRect().top - box.clientTop + box.scrollTop;
      const inset = sticks ? (pinnedBox.current?.offsetHeight ?? 0) : 0;
      setGeometry((prev) =>
        prev.margin === margin && prev.inset === inset ? prev : { margin, inset },
      );
    };
    measure();
    const observer = new ResizeObserver(measure);
    for (const watched of [prelude.current, pinnedBox.current, box, ...box.children])
      if (watched) observer.observe(watched);
    // What a note holds above the viewer moves it without resizing anything
    // observed here; the next scroll finds it where it is.
    if (embedded) box.addEventListener("scroll", measure, { passive: true });
    return () => {
      observer.disconnect();
      box.removeEventListener("scroll", measure);
    };
  }, [showsBody, sticks, embedded, scroller]);
  // In a note the reader scrolls the note, not the viewer: that is the reader
  // taking the scroll back, as a scroll of the viewer's own box is.
  useEffect(() => {
    if (!embedded || !scroller) return;
    const release = () => setFollowScroll(false);
    const pressed = (event: Event) => {
      if (event.target === scroller) release();
    };
    const keyed = (event: KeyboardEvent) => {
      if (event.target === scroller && SCROLL_KEYS[event.key]) release();
    };
    scroller.addEventListener("wheel", release, { passive: true });
    scroller.addEventListener("touchmove", release, { passive: true });
    scroller.addEventListener("pointerdown", pressed);
    scroller.addEventListener("keydown", keyed);
    return () => {
      scroller.removeEventListener("wheel", release);
      scroller.removeEventListener("touchmove", release);
      scroller.removeEventListener("pointerdown", pressed);
      scroller.removeEventListener("keydown", keyed);
    };
  }, [embedded, scroller]);
  const current = follow && time !== null && utterances ? currentUtterance(utterances, time) : -1;
  useEffect(() => {
    if (current >= 0 && followScroll && editing === null) reveal(current);
  }, [current, followScroll, editing, reveal]);
  const needle = query.trim().toLocaleLowerCase();
  const matches = useMemo(() => {
    if (!vm || !utterances || !needle) return [];
    const names = new Map(
      vm.transcript.speakers.map((s) => [s.id, speakerName(s).toLocaleLowerCase()]),
    );
    return utterances.flatMap((u, index) =>
      u.text.toLocaleLowerCase().includes(needle) || names.get(u.speaker)?.includes(needle)
        ? [index]
        : [],
    );
  }, [vm, utterances, needle]);
  const matching = useMemo(() => new Set(matches), [matches]);
  const active = activeMatch < matches.length ? activeMatch : -1;
  /** Play from a line, or put the player there when Follow ties the two together. */
  const seekTo = (utterance: Utterance, play?: boolean) =>
    player.current?.seek(utterance.start, play);
  /** A speaker's line found from the player's time, brought into view and the player put there, playing or not. */
  const goToLine = (speakerId: string, find: typeof nearestLine) => {
    if (!utterances) return;
    const index = find(utterances, speakerId, time ?? windowFrom);
    if (index < 0) return;
    reveal(index);
    seekTo(utterances[index]);
  };
  const jump = (step: 1 | -1) => {
    if (!utterances || matches.length === 0) return;
    const next =
      active < 0
        ? step > 0
          ? 0
          : matches.length - 1
        : (active + step + matches.length) % matches.length;
    setActiveMatch(next);
    reveal(matches[next]);
    if (follow) seekTo(utterances[matches[next]]);
  };
  const offered = vm ? offeredSpeakers(vm.transcript) : [];
  const speakers = useMemo(() => {
    const inks = speakerInks(vm?.transcript.speakers ?? []);
    const byId = new Map<string, { speaker: Speaker; ink: string; lines: number }>();
    for (const speaker of vm?.transcript.speakers ?? [])
      byId.set(speaker.id, { speaker, ink: inks.get(speaker.id) ?? "", lines: 0 });
    for (const utterance of utterances ?? []) {
      const entry = byId.get(utterance.speaker);
      if (entry) entry.lines += 1;
    }
    return byId;
  }, [vm, utterances]);
  const named = naming ? speakers.get(naming.speakerId)?.speaker : undefined;
  const heading = title === undefined ? (vm?.transcript.source.title ?? null) : title;
  const lines = utterances ?? [];
  const meta = vm && (
    <p className="min-w-0 break-words">
      {new Date(vm.transcript.createdAt).toLocaleString()} ·{" "}
      <span className="font-mono">{timestamp(vm.transcript.duration)}</span> ·{" "}
      {vm.transcript.language === "auto"
        ? "Automatic language"
        : vm.transcript.language === "en"
          ? "English"
          : "Polish"}{" "}
      · {vm.transcript.engine.asr} · {vm.transcript.engine.diarizer} ·{" "}
      {vm.transcript.engine.embedding}
    </p>
  );
  return (
    <section
      aria-label="Transcript viewer"
      className={cn("flex min-w-0 flex-col gap-3 text-sm", !embedded && "min-h-0 flex-1")}
      onKeyDown={(event) => {
        if (
          tab === "transcript" &&
          (event.metaKey || event.ctrlKey) &&
          event.key.toLowerCase() === "f"
        ) {
          event.preventDefault();
          search.current?.focus();
          search.current?.select();
        }
      }}
    >
      {error && (
        <p role="alert" className="break-words text-destructive">
          {error}
        </p>
      )}
      {!vm && !error && <p role="status">Opening transcript…</p>}
      {vm && (
        <>
          <div className="flex min-w-0 flex-wrap items-center gap-2">
            {!embedded && (
              <Tabs value={tab} onValueChange={(next) => setTab(next as "transcript" | "source")}>
                <TabsList aria-label="Transcript view">
                  <TabsTrigger value="transcript">{TRANSCRIPT_TAB}</TabsTrigger>
                  <TabsTrigger value="source">{SOURCE_TAB}</TabsTrigger>
                </TabsList>
              </Tabs>
            )}
            {tab === "transcript" && (
              <div className="flex min-w-0 flex-1 flex-wrap items-center gap-1">
                <Input
                  ref={search}
                  type="search"
                  aria-label={SEARCH_LABEL}
                  placeholder="Search"
                  className="min-w-0 max-w-96 flex-1 basis-40"
                  value={query}
                  onChange={(event) => {
                    setQuery(event.target.value);
                    setActiveMatch(-1);
                  }}
                  onKeyDown={(event) => {
                    if (event.key === "Enter" && !event.nativeEvent.isComposing) {
                      event.preventDefault();
                      jump(event.shiftKey ? -1 : 1);
                    } else if (event.key === "Escape" && query) {
                      event.preventDefault();
                      setQuery("");
                      setActiveMatch(-1);
                    }
                  }}
                />
                {needle && (
                  <span aria-live="polite" className="font-mono text-muted-foreground text-xs">
                    {matches.length === 0
                      ? "No matches"
                      : active < 0
                        ? `${matches.length} ${matches.length === 1 ? "match" : "matches"}`
                        : `${active + 1} of ${matches.length}`}
                  </span>
                )}
                <Button
                  size="sm"
                  variant="ghost"
                  aria-label="Previous match"
                  disabled={matches.length === 0}
                  onClick={() => jump(-1)}
                >
                  ↑
                </Button>
                <Button
                  size="sm"
                  variant="ghost"
                  aria-label="Next match"
                  disabled={matches.length === 0}
                  onClick={() => jump(1)}
                >
                  ↓
                </Button>
              </div>
            )}
            <div className="ml-auto flex items-center gap-1">
              {canTranscribe && sourcePath && (
                <Button
                  size="sm"
                  variant="ghost"
                  disabled={transcriptionRunning(redoJob)}
                  onClick={() => setReplacing(true)}
                >
                  <RotateCcw aria-hidden="true" />
                  {TRANSCRIBE_AGAIN_LABEL}…
                </Button>
              )}
              <DropdownMenu>
                <DropdownMenuTrigger asChild>
                  <Button size="icon-sm" variant="ghost" aria-label={TRANSCRIPT_ACTIONS_LABEL}>
                    <Ellipsis aria-hidden="true" />
                  </Button>
                </DropdownMenuTrigger>
                <DropdownMenuContent align="end" className={MENU_CONTENT}>
                  <DropdownMenuItem
                    onSelect={() => setClip(playerWindow ?? { from: null, to: null })}
                  >
                    <Clipboard aria-hidden="true" />
                    {COPY_AS_NOTE_EMBED}
                  </DropdownMenuItem>
                  {menuItems && (
                    <>
                      <DropdownMenuSeparator />
                      {menuItems}
                    </>
                  )}
                </DropdownMenuContent>
              </DropdownMenu>
            </div>
          </div>
          {sourcePath && redoJob && redoJob.phase !== "done" && (
            <TranscriptionJob path={sourcePath} onOpen={() => setReads((n) => n + 1)} />
          )}
          <TranscribeAgainDialog
            open={replacing}
            onClose={() => setReplacing(false)}
            onConfirm={() => {
              if (sourcePath)
                void startTranscription(sourcePath, () => setReads((n) => n + 1), true);
            }}
          />
          {tab === "source" && <TranscriptSource vm={vm} />}
          {/* Hidden rather than unmounted under Source, so a playing player keeps
              playing and the reader comes back to where they were. In a note
              the note scrolls: this is plain flow that grows with its lines. */}
          {/* biome-ignore lint/a11y/noStaticElementInteractions: listens for the reader's own scrolling, which is not an action on this box */}
          <div
            {...(embedded ? {} : list.viewportProps)}
            ref={embedded ? undefined : attachOwnBox}
            hidden={tab === "source"}
            className={cn("relative min-w-0", !embedded && "min-h-0 flex-1 overflow-auto")}
            onWheel={embedded ? undefined : () => setFollowScroll(false)}
            onTouchMove={embedded ? undefined : () => setFollowScroll(false)}
            onPointerDown={(event) => {
              // The scrollbar is the box itself; a press on a row is not a scroll.
              if (!embedded && event.target === event.currentTarget) setFollowScroll(false);
            }}
            onKeyDown={(event) => {
              if (!embedded && event.target === event.currentTarget && SCROLL_KEYS[event.key])
                setFollowScroll(false);
            }}
          >
            <div ref={prelude} className="min-w-0 space-y-3 px-3 empty:hidden">
              {heading && <h2 className="break-words font-heading text-title">{heading}</h2>}
              {mediaError && (
                <p className="break-words text-muted-foreground">
                  The recording cannot be played here: {mediaError}
                </p>
              )}
              {suggestions.map((suggestion, index) => (
                <div
                  key={`${suggestion.from}-${suggestion.to}`}
                  className="flex flex-wrap items-center gap-2 rounded-md border p-2"
                >
                  <span className="min-w-0 break-words">
                    Remember ‘{suggestion.from}’ → ‘{suggestion.to}’?
                  </span>
                  <Button
                    size="sm"
                    variant="outline"
                    disabled={busy || !bank}
                    onClick={() => {
                      if (!bank) return;
                      void act(async () => {
                        const terms = await dictionaryAcceptSuggestion(
                          bank.profileId,
                          suggestion.from,
                          suggestion.to,
                        );
                        transcriptionStore.setState((s) => ({
                          dictionary: { ...s.dictionary, [bank.profileId]: terms },
                        }));
                        setSuggestions((held) => held.filter((_, i) => i !== index));
                        return vm;
                      });
                    }}
                  >
                    Accept
                  </Button>
                  <Button
                    size="sm"
                    variant="ghost"
                    disabled={busy}
                    onClick={() => setSuggestions((held) => held.filter((_, i) => i !== index))}
                  >
                    Dismiss
                  </Button>
                  {!bank && (
                    <p className="text-muted-foreground">
                      Choose a folder that keeps voices in Settings → Sync to remember this term.
                    </p>
                  )}
                </div>
              ))}
            </div>
            {/* The player, what it plays and who speaks: pinned, the whole block
                stays at the top of the scroll area while the lines go by. */}
            <div
              ref={pinnedBox}
              className={cn(
                "min-w-0 space-y-3 px-3 py-3",
                sticks && "sticky top-0 z-10 border-b bg-background",
              )}
            >
              {media && awake ? (
                <TranscriptPlayer
                  ref={player}
                  media={media}
                  window={playerWindow}
                  initialPicture={initialPicture}
                  initialSound={initialSound}
                  pinned={pinned}
                  onPinnedChange={setPinned}
                  follow={follow}
                  onFollowChange={setFollow}
                  onTime={(seconds) => {
                    setTime(seconds);
                    hooks.current.onTime?.(seconds);
                  }}
                  onJump={() => setFollowScroll(true)}
                  onPlayingChange={(playing) => hooks.current.onPlayingChange?.(playing)}
                  meta={meta}
                />
              ) : (
                <>
                  {playable && (
                    <div className="flex aspect-video max-h-[30dvh] w-full items-center justify-center rounded-md bg-muted text-muted-foreground text-xs">
                      {asleepText}
                    </div>
                  )}
                  <div className="text-muted-foreground text-xs">{meta}</div>
                </>
              )}
              {markers}
              <ul aria-label="Speakers" className="flex min-w-0 flex-wrap items-center gap-1.5">
                {[...speakers.values()]
                  .filter((entry) => entry.lines > 0)
                  .map((entry) => (
                    <li key={entry.speaker.id}>
                      <SpeakerChip
                        speaker={entry.speaker}
                        ink={entry.ink}
                        lines={entry.lines}
                        vm={vm}
                        busy={busy}
                        act={act}
                        onName={(mode) => setNaming({ speakerId: entry.speaker.id, mode })}
                        focusName={() => nameField.current?.focus()}
                        onNearest={() => goToLine(entry.speaker.id, nearestLine)}
                        onNext={() => goToLine(entry.speaker.id, nextLine)}
                      />
                    </li>
                  ))}
                <li>
                  <AddSpeaker
                    transcript={vm.transcript}
                    busy={busy}
                    onAdd={async (origin, label) => {
                      let ok = false;
                      await act(async () => {
                        const next = await transcriptAddSpeaker(path, origin, label);
                        const known = new Set(vm.transcript.speakers.map((s) => s.id));
                        const fresh = next.transcript.speakers.find((s) => !known.has(s.id));
                        setAdded(
                          fresh
                            ? `${speakerName(fresh)} added. Give it lines from each line’s menu, under Change speaker.`
                            : null,
                        );
                        ok = true;
                        return next;
                      });
                      return ok;
                    }}
                  />
                </li>
              </ul>
              {naming && named && (
                <SpeakerNameForm
                  fieldRef={nameField}
                  key={`${naming.speakerId}:${naming.mode}`}
                  speaker={named}
                  mode={naming.mode}
                  busy={busy}
                  onCancel={() => setNaming(null)}
                  onSave={(name) =>
                    void act(async () => {
                      const result =
                        naming.mode === "new"
                          ? await transcriptAssignSpeaker(vm.path, naming.speakerId, null, name)
                          : await transcriptRenameSpeaker(vm.path, naming.speakerId, name);
                      setNaming(null);
                      return result;
                    })
                  }
                />
              )}
              {added && (
                <p role="status" className="break-words text-muted-foreground">
                  {added}
                </p>
              )}
            </div>
            {lines.length === 0 && (
              <p className="p-3 text-muted-foreground">
                {vm.transcript.utterances.length === 0
                  ? "No speech was found in this file."
                  : "Nothing was said in this stretch."}
              </p>
            )}
            <TranscriptLines
              lines={lines}
              speakers={vm.transcript.speakers}
              list={list}
              listRef={listBox}
              current={current}
              playable={playable}
              onSeek={(seconds, play) => player.current?.seek(seconds, play)}
              matching={matching}
              cardClassName={(index) =>
                cn(
                  matching.has(index) && "ring-1 ring-ring",
                  active >= 0 && matches[active] === index && "ring-2 ring-primary",
                )
              }
              menu={(_, index) => {
                const utterance = lines[index];
                return (
                  <LineMenu
                    utterance={utterance}
                    offered={offered}
                    playable={playable}
                    busy={busy}
                    onPlay={() => seekTo(utterance, true)}
                    onCopyClip={() => setClip({ from: utterance.start, to: utterance.end })}
                    onEdit={() => {
                      if (follow) seekTo(utterance);
                      setEditing(utterance.id);
                      setDraft(utterance.text);
                    }}
                    onReassign={(speakerId) =>
                      void act(() => transcriptReassignUtterance(path, utterance.id, speakerId))
                    }
                    onPanel={(kind) =>
                      setPanel((open) =>
                        open?.id === utterance.id && open.kind === kind
                          ? null
                          : { id: utterance.id, kind },
                      )
                    }
                    focusEdit={() => editField.current?.focus()}
                  />
                );
              }}
              note={(_, index) => {
                const utterance = lines[index];
                if (!utterance.edited) return null;
                return utterance.asrText ? (
                  <details className="min-w-0">
                    <summary className="cursor-pointer">Edited · Recognised text</summary>
                    <p className="whitespace-pre-wrap break-words">{utterance.asrText}</p>
                  </details>
                ) : (
                  <span>Added by hand</span>
                );
              }}
              body={(line, index) =>
                editing === line.id ? (
                  <EditLine
                    fieldRef={editField}
                    utterance={lines[index]}
                    draft={draft}
                    busy={busy}
                    onDraft={setDraft}
                    onSave={() => void save(line.id)}
                    onCancel={() => setEditing(null)}
                  />
                ) : needle ? (
                  <p className="mt-1 whitespace-pre-wrap break-words font-normal text-foreground text-title leading-relaxed">
                    {marked(line.text, needle)}
                  </p>
                ) : undefined
              }
              after={(line, index) => {
                if (panel?.id !== line.id) return null;
                const utterance = lines[index];
                return panel.kind === "split" ? (
                  <SplitPanel
                    utterance={utterance}
                    busy={busy}
                    onSplit={(wordIndex) =>
                      void changeLines(() =>
                        transcriptSplitUtterance(path, utterance.id, wordIndex),
                      )
                    }
                    onCancel={() => setPanel(null)}
                  />
                ) : (
                  <InsertPanel
                    utterance={utterance}
                    speakers={offered}
                    busy={busy}
                    onInsert={(speakerId, text) =>
                      void changeLines(() =>
                        transcriptInsertUtterance(path, utterance.id, speakerId, text),
                      )
                    }
                    onCancel={() => setPanel(null)}
                  />
                );
              }}
            />
          </div>
          <CopyClipDialog
            request={clip}
            onClose={() => setClip(null)}
            compose={composeClip ?? ((from, to, words) => transcriptClip(path, from, to, words))}
          />
        </>
      )}
    </section>
  );
}
/**
 * A line's actions behind one ⋯ button, right after its time: playing and
 * copying first, the corrections under them.
 */
function LineMenu({
  utterance,
  offered,
  playable,
  busy,
  onPlay,
  onCopyClip,
  onEdit,
  onReassign,
  onPanel,
  focusEdit,
}: {
  utterance: Utterance;
  offered: readonly Speaker[];
  playable: boolean;
  busy: boolean;
  onPlay: () => void;
  onCopyClip: () => void;
  onEdit: () => void;
  onReassign: (speakerId: string) => void;
  onPanel: (kind: "split" | "insert") => void;
  /** Focus the edit field Edit text opened. */
  focusEdit: () => void;
}) {
  // An item that opens a field keeps the focus there, not on the ⋯ it came from.
  const keepFocus = useRef(false);
  return (
    <DropdownMenu>
      <LineMenuTrigger id={utterance.id} />
      <DropdownMenuContent
        align="start"
        className={MENU_CONTENT}
        onCloseAutoFocus={(event) => {
          if (!keepFocus.current) return;
          keepFocus.current = false;
          // Only once the menu has let go: a field focused while its focus
          // trap still stands is pulled back into the menu and dropped.
          event.preventDefault();
          focusEdit();
        }}
      >
        {playable && (
          <DropdownMenuItem onSelect={onPlay}>
            <Play aria-hidden="true" />
            {PLAY_FROM_HERE}
          </DropdownMenuItem>
        )}
        <DropdownMenuItem onSelect={onCopyClip}>
          <Clipboard aria-hidden="true" />
          {COPY_CLIP_FROM_HERE}
        </DropdownMenuItem>
        <DropdownMenuSeparator />
        <DropdownMenuItem
          disabled={busy}
          onSelect={() => {
            keepFocus.current = true;
            onEdit();
          }}
        >
          <Pencil aria-hidden="true" />
          Edit text
        </DropdownMenuItem>
        <DropdownMenuSub>
          <DropdownMenuSubTrigger disabled={busy}>
            <UserRoundPen aria-hidden="true" />
            Change speaker
          </DropdownMenuSubTrigger>
          <DropdownMenuSubContent>
            <DropdownMenuRadioGroup
              value={utterance.speaker}
              onValueChange={(next) => {
                if (next !== utterance.speaker) onReassign(next);
              }}
            >
              {offered.map((s) => (
                <DropdownMenuRadioItem key={s.id} value={s.id}>
                  {speakerName(s)}
                </DropdownMenuRadioItem>
              ))}
            </DropdownMenuRadioGroup>
          </DropdownMenuSubContent>
        </DropdownMenuSub>
        <DropdownMenuSeparator />
        {utterance.words.length > 1 && (
          <DropdownMenuItem disabled={busy} onSelect={() => onPanel("split")}>
            <Split aria-hidden="true" />
            Split…
          </DropdownMenuItem>
        )}
        <DropdownMenuItem disabled={busy} onSelect={() => onPanel("insert")}>
          <ListPlus aria-hidden="true" />
          Add a line after
        </DropdownMenuItem>
      </DropdownMenuContent>
    </DropdownMenu>
  );
}
function EditLine({
  utterance,
  draft,
  busy,
  onDraft,
  onSave,
  onCancel,
  fieldRef,
}: {
  utterance: Utterance;
  draft: string;
  busy: boolean;
  onDraft: (draft: string) => void;
  onSave: () => void;
  onCancel: () => void;
  fieldRef: Ref<HTMLTextAreaElement>;
}) {
  return (
    <div className="mt-1 space-y-2">
      <textarea
        ref={fieldRef}
        aria-label={`Edit ${utterance.id}`}
        className="min-h-20 w-full rounded-md border bg-background p-2 text-title font-normal leading-relaxed"
        value={draft}
        disabled={busy}
        onChange={(event) => onDraft(event.target.value)}
        onKeyDown={(event) => {
          if (event.key === "Escape") {
            event.preventDefault();
            onCancel();
          } else if (event.key === "Enter" && !event.shiftKey && !event.nativeEvent.isComposing) {
            event.preventDefault();
            onSave();
          }
        }}
      />
      <div className="flex gap-2">
        <Button size="sm" disabled={busy} onClick={onSave}>
          Save text
        </Button>
        <Button size="sm" variant="ghost" disabled={busy} onClick={onCancel}>
          Cancel edit
        </Button>
      </div>
    </div>
  );
}
/**
 * One speaker in the legend: ink, name, status lamp and line count, with every
 * change to the speaker behind it.
 */
function SpeakerChip({
  speaker,
  ink,
  lines,
  vm,
  busy,
  act,
  onName,
  focusName,
  onNearest,
  onNext,
}: {
  speaker: Speaker;
  ink: string;
  lines: number;
  vm: TranscriptVm;
  busy: boolean;
  act: (operation: () => Promise<TranscriptVm>) => Promise<void>;
  onName: (mode: "rename" | "new") => void;
  /** Focus the field Rename label… or New person… opened. */
  focusName: () => void;
  /** Take the reader, and the player, to this speaker's line nearest the player. */
  onNearest: () => void;
  /** The same, to this speaker's next line after the player, from their first past the last. */
  onNext: () => void;
}) {
  // The field an item opens keeps the focus, not the chip it came from.
  const keepFocus = useRef(false);
  const openNameField = (mode: "rename" | "new") => {
    keepFocus.current = true;
    onName(mode);
  };
  const candidates = new Map(speaker.candidates.map((c) => [c.personId, c.score]));
  const people = [
    ...vm.people.filter((p) => candidates.has(p.id)),
    ...vm.people.filter((p) => !candidates.has(p.id)),
  ];
  const name = speakerName(speaker);
  const status = STATUS[speaker.status];
  const others = vm.transcript.speakers.filter((s) => s.id !== speaker.id);
  return (
    <DropdownMenu>
      <DropdownMenuTrigger asChild>
        <Button
          size="xs"
          variant="ghost"
          disabled={busy}
          aria-label={`${name}, ${status}, ${lines} ${lines === 1 ? "line" : "lines"}`}
          className="gap-1.5 rounded-sm border-border"
        >
          <span aria-hidden="true" className={cn("size-2 shrink-0 rounded-[2px]", ink)} />
          <span className="max-w-48 truncate">{name}</span>
          <Lamp state={STATUS_LAMP[speaker.status]} label={null} />
          <span className="font-mono text-muted-foreground tabular-nums">{lines}</span>
        </Button>
      </DropdownMenuTrigger>
      <DropdownMenuContent
        className={MENU_CONTENT}
        onCloseAutoFocus={(event) => {
          if (!keepFocus.current) return;
          keepFocus.current = false;
          event.preventDefault();
          focusName();
        }}
      >
        <DropdownMenuLabel>
          {status}
          {speaker.score !== null && (
            <span className="font-mono"> · {speaker.score.toFixed(2)}</span>
          )}
        </DropdownMenuLabel>
        <DropdownMenuItem onSelect={onNearest}>
          <Crosshair aria-hidden="true" />
          {NEAREST_LINE_LABEL}
        </DropdownMenuItem>
        <DropdownMenuItem onSelect={onNext}>
          <SkipForward aria-hidden="true" />
          {NEXT_LINE_LABEL}
        </DropdownMenuItem>
        <DropdownMenuSeparator />
        <DropdownMenuSub>
          <DropdownMenuSubTrigger>
            <UserPlus aria-hidden="true" />
            This is…
          </DropdownMenuSubTrigger>
          <DropdownMenuSubContent className="max-h-80 overflow-y-auto">
            {people.map((p) => {
              const score = candidates.get(p.id);
              return (
                <DropdownMenuItem
                  key={p.id}
                  onSelect={() =>
                    void act(() => transcriptAssignSpeaker(vm.path, speaker.id, p.id, null))
                  }
                >
                  {p.name}
                  {score !== undefined && (
                    <span className="ml-auto font-mono text-muted-foreground text-xs">
                      suggested {score.toFixed(2)}
                    </span>
                  )}
                </DropdownMenuItem>
              );
            })}
            {people.length > 0 && <DropdownMenuSeparator />}
            <DropdownMenuItem onSelect={() => openNameField("new")}>New person…</DropdownMenuItem>
          </DropdownMenuSubContent>
        </DropdownMenuSub>
        <DropdownMenuItem onSelect={() => openNameField("rename")}>
          <Pencil aria-hidden="true" />
          Rename label…
        </DropdownMenuItem>
        {others.length > 0 && (
          <DropdownMenuSub>
            <DropdownMenuSubTrigger>
              <ChevronRight aria-hidden="true" />
              Merge into…
            </DropdownMenuSubTrigger>
            <DropdownMenuSubContent className="max-h-80 overflow-y-auto">
              {others.map((s) => (
                <DropdownMenuItem
                  key={s.id}
                  onSelect={() =>
                    void act(() => transcriptMergeSpeakers(vm.path, speaker.id, s.id))
                  }
                >
                  {speakerName(s)}
                </DropdownMenuItem>
              ))}
            </DropdownMenuSubContent>
          </DropdownMenuSub>
        )}
      </DropdownMenuContent>
    </DropdownMenu>
  );
}
/** The field a speaker's new label, or the new person's name, is typed into. */
function SpeakerNameForm({
  speaker,
  mode,
  busy,
  onSave,
  onCancel,
  fieldRef,
}: {
  speaker: Speaker;
  mode: "rename" | "new";
  busy: boolean;
  onSave: (name: string) => void;
  onCancel: () => void;
  fieldRef: Ref<HTMLInputElement>;
}) {
  const [name, setName] = useState(mode === "rename" ? speakerName(speaker) : "");
  return (
    <form
      className="flex min-w-0 flex-wrap items-center gap-2"
      onSubmit={(event) => {
        event.preventDefault();
        onSave(name);
      }}
    >
      <Input
        ref={fieldRef}
        aria-label={mode === "new" ? "New person name" : "Speaker label"}
        className="min-w-0 max-w-80 flex-1 basis-36"
        value={name}
        onChange={(event) => setName(event.target.value)}
        onKeyDown={(event) => {
          if (event.key === "Escape") {
            event.preventDefault();
            onCancel();
          }
        }}
      />
      <Button type="submit" size="sm" disabled={busy || (mode === "new" && !name.trim())}>
        {mode === "new" ? "Create person" : "Save label"}
      </Button>
      <Button type="button" size="sm" variant="ghost" onClick={onCancel}>
        Cancel
      </Button>
    </form>
  );
}
/**
 * The transcript's JSON, read-only, in the editor the Files panel opens a
 * `.json` in. Rust writes a transcript as two-space pretty JSON with a final
 * newline (`Transcript::to_json`), so the model laid out the same way is the
 * file's text as keeper wrote it. Inset by the pane pad, like the lines are,
 * so the text never runs into the frame.
 */
function TranscriptSource({ vm }: { vm: TranscriptVm }) {
  const content = useMemo(() => `${JSON.stringify(vm.transcript, null, 2)}\n`, [vm.transcript]);
  return (
    <div className="min-h-0 min-w-0 flex-1 overflow-auto rounded-md border p-3">
      <TextEditorSurface
        content={content}
        language={FILE_FORMATS.get("json")?.language ?? null}
        fileName={vm.path.split(/[\\/]/).pop() ?? vm.path}
        readOnly
      />
    </div>
  );
}
/**
 * A speaker nobody was heard as yet, on one track, to move lines to. The track
 * is asked only when the transcript heard two: a microphone speaker takes only
 * microphone lines.
 */
function AddSpeaker({
  transcript,
  busy,
  onAdd,
}: {
  transcript: Transcript;
  busy: boolean;
  /** Whether the speaker was added; the form stays open with its entry on a refusal. */
  onAdd: (origin: TrackOrigin, label: string | null) => Promise<boolean>;
}) {
  const origins = heardOrigins(transcript);
  const [open, setOpen] = useState(false);
  const [origin, setOrigin] = useState<TrackOrigin>(origins[0] ?? "mixed");
  const [label, setLabel] = useState("");
  if (!open)
    return (
      <Button size="xs" variant="ghost" disabled={busy} onClick={() => setOpen(true)}>
        <UserPlus aria-hidden="true" />
        {ADD_SPEAKER_LABEL}
      </Button>
    );
  return (
    <form
      aria-label={ADD_SPEAKER_LABEL}
      className="flex min-w-0 flex-wrap items-center gap-2"
      onSubmit={(event) => {
        event.preventDefault();
        void onAdd(origin, label.trim() || null).then((added) => {
          if (!added) return;
          setOpen(false);
          setLabel("");
        });
      }}
    >
      {origins.length > 1 && (
        <ToggleGroup
          type="single"
          aria-label="Heard on"
          value={origin}
          disabled={busy}
          onValueChange={(next) => {
            if (next) setOrigin(next as TrackOrigin);
          }}
        >
          {origins.map((o) => (
            <ToggleGroupItem key={o} value={o} className="px-2">
              {ORIGIN_NAME[o]}
            </ToggleGroupItem>
          ))}
        </ToggleGroup>
      )}
      <Input
        aria-label="New speaker label"
        placeholder="Label (optional)"
        className="h-8 min-w-0 max-w-60 flex-1 basis-36"
        value={label}
        disabled={busy}
        onChange={(event) => setLabel(event.target.value)}
      />
      <Button type="submit" size="sm" disabled={busy}>
        Add
      </Button>
      <Button type="button" size="sm" variant="ghost" onClick={() => setOpen(false)}>
        Cancel
      </Button>
    </form>
  );
}
/** The words a line can split before: every one but the first, since a new line needs words on both sides. */
function SplitPanel({
  utterance,
  busy,
  onSplit,
  onCancel,
}: {
  utterance: Utterance;
  busy: boolean;
  onSplit: (wordIndex: number) => void;
  onCancel: () => void;
}) {
  return (
    <fieldset className="mt-2 min-w-0 space-y-2 rounded-md border p-2">
      <legend className="px-1 text-muted-foreground text-xs">
        Choose the word the new line starts with
      </legend>
      <div className="flex flex-wrap gap-1">
        {utterance.words.map((word, index) => (
          <Button
            // biome-ignore lint/suspicious/noArrayIndexKey: words repeat; their position in the line is their identity
            key={index}
            size="sm"
            variant="outline"
            aria-label={`Start the new line at word ${index + 1}, ${word.text}`}
            disabled={busy || index === 0}
            onClick={() => onSplit(index)}
          >
            {word.text}
          </Button>
        ))}
      </div>
      <Button size="sm" variant="ghost" disabled={busy} onClick={onCancel}>
        Cancel split
      </Button>
    </fieldset>
  );
}
function InsertPanel({
  utterance,
  speakers,
  busy,
  onInsert,
  onCancel,
}: {
  utterance: Utterance;
  speakers: readonly Speaker[];
  busy: boolean;
  onInsert: (speakerId: string, text: string) => void;
  onCancel: () => void;
}) {
  const [speakerId, setSpeakerId] = useState(utterance.speaker);
  const [text, setText] = useState("");
  return (
    <form
      className="mt-2 flex min-w-0 flex-wrap gap-2 rounded-md border p-2"
      onSubmit={(event) => {
        event.preventDefault();
        if (text.trim()) onInsert(speakerId, text);
      }}
    >
      <Select value={speakerId} disabled={busy} onValueChange={setSpeakerId}>
        <SelectTrigger
          size="sm"
          aria-label={`Speaker for the line after ${utterance.id}`}
          className="max-w-48"
        >
          <SelectValue />
        </SelectTrigger>
        <SelectContent>
          {speakers.map((s) => (
            <SelectItem key={s.id} value={s.id}>
              {speakerName(s)}
            </SelectItem>
          ))}
        </SelectContent>
      </Select>
      <Input
        aria-label={`Text of the line after ${utterance.id}`}
        className="h-8 min-w-0 flex-1 basis-48"
        value={text}
        disabled={busy}
        onChange={(event) => setText(event.target.value)}
        onKeyDown={(event) => {
          if (event.key === "Escape") {
            event.preventDefault();
            onCancel();
          }
        }}
      />
      <Button type="submit" size="sm" disabled={busy || !text.trim()}>
        Add line
      </Button>
      <Button type="button" size="sm" variant="ghost" onClick={onCancel}>
        Cancel
      </Button>
    </form>
  );
}
