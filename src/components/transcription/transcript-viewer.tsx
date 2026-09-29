import {
  ChevronRight,
  Ellipsis,
  ListPlus,
  Pencil,
  Play,
  RotateCcw,
  Split,
  UserPlus,
  UserRoundPen,
} from "lucide-react";
import { type ReactNode, type Ref, useCallback, useEffect, useMemo, useRef, useState } from "react";
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
import { useWindowedRows } from "@/components/ui/window-list";
import { TextFileViewer } from "@/components/viewers/text-file-viewer";
import { TextEditorSurface } from "@/components/viewers/text-viewer";
import {
  dictionaryAcceptSuggestion,
  type TrackOrigin,
  type TranscriptMediaVm,
  type TranscriptVm,
  transcriptAddSpeaker,
  transcriptAssignSpeaker,
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
import { currentUtterance } from "./session-timeline";
import { TRANSCRIBE_AGAIN_LABEL, TranscribeAgainDialog } from "./transcribe-again";
import { TranscriptPlayer, type TranscriptPlayerHandle } from "./transcript-player";
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
function speakerName(speaker: Speaker): string {
  return speaker.name ?? (speaker.id === "ME" ? "You" : `Speaker ${speaker.id.replace(/^S/, "")}`);
}
function timestamp(seconds: number): string {
  const n = Math.floor(seconds);
  return `${Math.floor(n / 3600)
    .toString()
    .padStart(2, "0")}:${Math.floor((n / 60) % 60)
    .toString()
    .padStart(2, "0")}:${(n % 60).toString().padStart(2, "0")}`;
}
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
 * Hidden until the line is pointed at or holds focus, and always in the tab
 * order: the discreet way into a correction. Shown outright where nothing can
 * hover.
 */
const REVEALED_ON_HOVER =
  "opacity-0 transition-opacity group-focus-within:opacity-100 group-hover:opacity-100 focus-visible:opacity-100 data-[state=open]:opacity-100 [@media(hover:none)]:opacity-100";
/** A menu opened from an icon is as wide as its words, not as its trigger. */
const MENU_CONTENT = "w-auto min-w-44";
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
}: {
  path: string | null;
  onClose: () => void;
  profileId?: string | null;
}) {
  return (
    <Dialog
      open={path !== null}
      onOpenChange={(open) => {
        if (!open) onClose();
      }}
    >
      {/* Most of the window: the player can use the width, and the lines keep
          a reading measure of their own inside it. */}
      <DialogContent className="flex h-[92dvh] w-[min(96vw,1600px)] min-w-0 max-w-none flex-col sm:max-w-none">
        <DialogHeader>
          <DialogTitle>Transcript</DialogTitle>
          <DialogDescription>
            Read and correct what was said. Recognition stays on this Mac.
          </DialogDescription>
        </DialogHeader>
        {path && <TranscriptViewer key={path} path={path} profileId={profileId} />}
      </DialogContent>
    </Dialog>
  );
}
export function TranscriptViewer({
  path,
  profileId = null,
  onUnreadable,
}: {
  path: string;
  profileId?: string | null;
  /** Called instead of showing the error when the file cannot be read as a transcript. */
  onUnreadable?: () => void;
}) {
  const [vm, setVm] = useState<TranscriptVm | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const [suggestions, setSuggestions] = useState<DictionarySuggestion[]>([]);
  const [editing, setEditing] = useState<string | null>(null);
  const [draft, setDraft] = useState("");
  const [panel, setPanel] = useState<{ id: string; kind: "split" | "insert" } | null>(null);
  const [naming, setNaming] = useState<{ speakerId: string; mode: "rename" | "new" } | null>(null);
  const [tab, setTab] = useState<"transcript" | "source">("transcript");
  const [media, setMedia] = useState<TranscriptMediaVm | null>(null);
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
  const player = useRef<TranscriptPlayerHandle | null>(null);
  const search = useRef<HTMLInputElement | null>(null);
  const prelude = useRef<HTMLDivElement | null>(null);
  const playerBox = useRef<HTMLDivElement | null>(null);
  const listBox = useRef<HTMLOListElement | null>(null);
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
    setMedia(null);
    setMediaError(null);
    setTime(null);
    setAdded(null);
    void transcriptMedia(path)
      .then((next) => {
        if (generation.current === mine) setMedia(next);
      })
      .catch((cause: unknown) => {
        if (generation.current === mine) setMediaError(syncErrorMessage(cause));
      });
    return () => {
      generation.current += 1;
    };
  }, [path, reads]);
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
  const utterances = vm?.transcript.utterances;
  const getKey = useCallback(
    (index: number) => utterances?.[index]?.id ?? String(index),
    [utterances],
  );
  const list = useWindowedRows({
    count: utterances?.length ?? 0,
    getKey,
    rowHeight: 84,
    gap: 4,
    scrollMargin: geometry.margin,
    stickyInset: geometry.inset,
  });
  const reveal = list.reveal;
  const showsBody = vm !== null && tab === "transcript";
  const playable = media !== null && media.parts.length > 0;
  // The list starts below the heading, the legend and the player, all in the
  // one scroll box, and a pinned player covers the top of it.
  useEffect(() => {
    if (!showsBody) return;
    const measure = () => {
      const margin = listBox.current?.offsetTop ?? 0;
      const inset = pinned && playable ? (playerBox.current?.offsetHeight ?? 0) : 0;
      setGeometry((prev) =>
        prev.margin === margin && prev.inset === inset ? prev : { margin, inset },
      );
    };
    measure();
    const observer = new ResizeObserver(measure);
    for (const box of [prelude.current, playerBox.current]) if (box) observer.observe(box);
    return () => observer.disconnect();
  }, [showsBody, pinned, playable]);
  const current = follow && time !== null && utterances ? currentUtterance(utterances, time) : -1;
  useEffect(() => {
    if (current >= 0 && followScroll && editing === null) reveal(current);
  }, [current, followScroll, editing, reveal]);
  const needle = query.trim().toLocaleLowerCase();
  const matches = useMemo(() => {
    if (!vm || !needle) return [];
    const names = new Map(
      vm.transcript.speakers.map((s) => [s.id, speakerName(s).toLocaleLowerCase()]),
    );
    return vm.transcript.utterances.flatMap((u, index) =>
      u.text.toLocaleLowerCase().includes(needle) || names.get(u.speaker)?.includes(needle)
        ? [index]
        : [],
    );
  }, [vm, needle]);
  const matching = useMemo(() => new Set(matches), [matches]);
  const active = activeMatch < matches.length ? activeMatch : -1;
  /** Play from a line, or put the player there when Follow ties the two together. */
  const seekTo = (utterance: Utterance, play?: boolean) =>
    player.current?.seek(utterance.start, play);
  const jump = (step: 1 | -1) => {
    if (!vm || matches.length === 0) return;
    const next =
      active < 0
        ? step > 0
          ? 0
          : matches.length - 1
        : (active + step + matches.length) % matches.length;
    setActiveMatch(next);
    reveal(matches[next]);
    if (follow) seekTo(vm.transcript.utterances[matches[next]]);
  };
  const offered = vm ? offeredSpeakers(vm.transcript) : [];
  const speakers = useMemo(() => {
    const byId = new Map<string, { speaker: Speaker; ink: string; lines: number }>();
    vm?.transcript.speakers.forEach((speaker, index) => {
      byId.set(speaker.id, {
        speaker,
        ink: SPEAKER_INKS[index % SPEAKER_INKS.length],
        lines: 0,
      });
    });
    for (const utterance of vm?.transcript.utterances ?? []) {
      const entry = byId.get(utterance.speaker);
      if (entry) entry.lines += 1;
    }
    return byId;
  }, [vm]);
  const named = naming ? speakers.get(naming.speakerId)?.speaker : undefined;
  return (
    <section
      aria-label="Transcript viewer"
      className="flex min-h-0 min-w-0 flex-1 flex-col gap-3 text-sm"
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
            <Tabs value={tab} onValueChange={(next) => setTab(next as "transcript" | "source")}>
              <TabsList aria-label="Transcript view">
                <TabsTrigger value="transcript">{TRANSCRIPT_TAB}</TabsTrigger>
                <TabsTrigger value="source">{SOURCE_TAB}</TabsTrigger>
              </TabsList>
            </Tabs>
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
            {canTranscribe && sourcePath && (
              <Button
                size="sm"
                variant="ghost"
                className="ml-auto"
                disabled={transcriptionRunning(redoJob)}
                onClick={() => setReplacing(true)}
              >
                <RotateCcw aria-hidden="true" />
                {TRANSCRIBE_AGAIN_LABEL}…
              </Button>
            )}
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
              playing and the reader comes back to where they were. */}
          {/* biome-ignore lint/a11y/noStaticElementInteractions: listens for the reader's own scrolling, which is not an action on this box */}
          <div
            {...list.viewportProps}
            hidden={tab === "source"}
            className="relative min-h-0 flex-1 overflow-auto"
            onWheel={() => setFollowScroll(false)}
            onTouchMove={() => setFollowScroll(false)}
            onPointerDown={(event) => {
              // The scrollbar is the box itself; a press on a row is not a scroll.
              if (event.target === event.currentTarget) setFollowScroll(false);
            }}
            onKeyDown={(event) => {
              if (event.target === event.currentTarget && SCROLL_KEYS[event.key])
                setFollowScroll(false);
            }}
          >
            <div ref={prelude} className="mx-auto min-w-0 max-w-4xl space-y-3 px-3 pb-3">
              <header className="min-w-0 space-y-1">
                <h2 className="break-words font-heading text-title">
                  {vm.transcript.source.files.join(", ")}
                </h2>
                <p className="text-muted-foreground text-xs">
                  {new Date(vm.transcript.createdAt).toLocaleString()} ·{" "}
                  <span className="font-mono">{timestamp(vm.transcript.duration)}</span> ·{" "}
                  {vm.transcript.language === "auto"
                    ? "Automatic language"
                    : vm.transcript.language === "en"
                      ? "English"
                      : "Polish"}{" "}
                  ·{" "}
                  <span className="break-words">
                    {vm.transcript.engine.asr} · {vm.transcript.engine.diarizer} ·{" "}
                    {vm.transcript.engine.embedding}
                  </span>
                </p>
              </header>
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
            {media && playable && (
              <div
                ref={playerBox}
                className={cn(
                  "min-w-0 px-3 pb-3",
                  pinned && "sticky top-0 z-10 border-b bg-background pt-1",
                )}
              >
                <TranscriptPlayer
                  ref={player}
                  media={media}
                  pinned={pinned}
                  onPinnedChange={setPinned}
                  follow={follow}
                  onFollowChange={setFollow}
                  onTime={setTime}
                  onJump={() => setFollowScroll(true)}
                />
              </div>
            )}
            {vm.transcript.utterances.length === 0 && (
              <p className="p-3 text-muted-foreground">No speech was found in this file.</p>
            )}
            <ol
              ref={listBox}
              aria-label="Utterances"
              className="relative"
              style={{ height: list.totalSize }}
            >
              {list.rows.map((row) => {
                const utterance = vm.transcript.utterances[row.index];
                if (!utterance) return null;
                const speaking = row.index === current;
                const who = speakers.get(utterance.speaker);
                return (
                  <li
                    key={row.key}
                    {...list.rowProps(row)}
                    aria-current={speaking ? "true" : undefined}
                    data-match={matching.has(row.index) ? "" : undefined}
                    className="group min-w-0 px-3"
                  >
                    <div
                      className={cn(
                        "mx-auto min-w-0 max-w-4xl rounded-md border border-transparent px-3 py-2",
                        speaking && "border-primary bg-secondary",
                        matching.has(row.index) && "ring-1 ring-ring",
                        active >= 0 && matches[active] === row.index && "ring-2 ring-primary",
                      )}
                    >
                      <div className="flex min-w-0 items-center gap-2 text-muted-foreground text-xs">
                        <span
                          aria-hidden="true"
                          className={cn("size-2 shrink-0 rounded-[2px]", who?.ink)}
                        />
                        <span className="min-w-0 truncate font-medium">
                          {who ? speakerName(who.speaker) : utterance.speaker}
                        </span>
                        {playable ? (
                          <button
                            type="button"
                            className="rounded-sm font-mono tabular-nums hover:text-foreground focus-visible:outline focus-visible:outline-ring"
                            aria-label={`Go to ${timestamp(utterance.start)}, ${utterance.id}`}
                            onClick={() => seekTo(utterance)}
                          >
                            {timestamp(utterance.start)}
                          </button>
                        ) : (
                          <span className="font-mono tabular-nums">
                            {timestamp(utterance.start)}
                          </span>
                        )}
                        {utterance.edited && utterance.asrText ? (
                          <details className="min-w-0">
                            <summary className="cursor-pointer">Edited · Recognised text</summary>
                            <p className="whitespace-pre-wrap break-words">{utterance.asrText}</p>
                          </details>
                        ) : (
                          utterance.edited && <span>Added by hand</span>
                        )}
                        <div className="ml-auto">
                          <LineMenu
                            utterance={utterance}
                            offered={offered}
                            playable={playable}
                            busy={busy}
                            onPlay={() => seekTo(utterance, true)}
                            onEdit={() => {
                              if (follow) seekTo(utterance);
                              setEditing(utterance.id);
                              setDraft(utterance.text);
                            }}
                            onReassign={(speakerId) =>
                              void act(() =>
                                transcriptReassignUtterance(path, utterance.id, speakerId),
                              )
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
                        </div>
                      </div>
                      {editing === utterance.id ? (
                        <EditLine
                          fieldRef={editField}
                          utterance={utterance}
                          draft={draft}
                          busy={busy}
                          onDraft={setDraft}
                          onSave={() => void save(utterance.id)}
                          onCancel={() => setEditing(null)}
                        />
                      ) : (
                        <p className="mt-1 max-w-[80ch] whitespace-pre-wrap break-words font-normal text-foreground text-title leading-relaxed">
                          {marked(utterance.text, needle)}
                        </p>
                      )}
                      {panel?.id === utterance.id && panel.kind === "split" && (
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
                      )}
                      {panel?.id === utterance.id && panel.kind === "insert" && (
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
                      )}
                    </div>
                  </li>
                );
              })}
            </ol>
          </div>
        </>
      )}
    </section>
  );
}
/**
 * A line's corrections, behind one ⋯ button. The button is always in the tab
 * order; the pointer finds it on hover.
 */
function LineMenu({
  utterance,
  offered,
  playable,
  busy,
  onPlay,
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
      <DropdownMenuTrigger asChild>
        <Button
          size="icon-xs"
          variant="ghost"
          aria-label={`Line actions ${utterance.id}`}
          className={REVEALED_ON_HOVER}
        >
          <Ellipsis aria-hidden="true" />
        </Button>
      </DropdownMenuTrigger>
      <DropdownMenuContent
        align="end"
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
            Play from here
          </DropdownMenuItem>
        )}
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
        className="min-h-20 w-full max-w-[80ch] rounded-md border bg-background p-2 text-title font-normal leading-relaxed"
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
