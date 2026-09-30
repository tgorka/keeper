/**
 * The panel a ` ```keeper-media ` block becomes in a note (UX-DR124, UX-DR125).
 *
 * **The transcript viewer itself, embedded.** A transcribed block is
 * `TranscriptViewer` with everything the dialog offers — edit, split, add a
 * line, change a speaker, rename and merge speakers, search, transcribe again,
 * copy a clip. What the block adds is its window (only the lines inside
 * `from`/`to`), its markers, and its own verbs appended to the viewer's ⋯. The
 * viewer has no scroll box of its own here: it grows with its lines inside the
 * note, windows them against the editor's scroller and pins its player to the
 * top of it, so the note has one scrollbar.
 *
 * **Everything the block knows came from Rust.** `media_block_resolve` reads the
 * body and answers with the window, the media references and the markers; a
 * refusal is Rust's sentence, shown above the block's own text (UX-DR44). A
 * marker edit is `media_block_edit`'s new body, spliced by the editor over the
 * block's range as it is at that moment.
 *
 * **One player at a time, and only near the screen (AD-359).** The player —
 * and so every `<video>` — mounts when an observer with one screen of margin
 * sees the block, and unmounts when it leaves unless it is playing. Starting
 * this block pauses any other in the same note pane.
 *
 * **It learns of its transcript without rewriting the note (AD-357).** When
 * `keeper://transcript-written` names the block's transcript path — the
 * expected one, before the first transcript exists — the block resolves again.
 */
import { Ellipsis, MoveHorizontal } from "lucide-react";
import {
  type FormEvent,
  type ReactNode,
  useCallback,
  useEffect,
  useId,
  useRef,
  useState,
} from "react";
import {
  REMOVE_RECORDING_LABEL,
  RemoveRecordingDialog,
} from "@/components/recordings/remove-recording-dialog";
import { type ClipRequest, CopyClipDialog } from "@/components/transcription/copy-clip";
import { currentUtterance } from "@/components/transcription/session-timeline";
import { timestamp } from "@/components/transcription/transcript-lines";
import {
  TranscriptPlayer,
  type TranscriptPlayerHandle,
} from "@/components/transcription/transcript-player";
import { TranscriptDialog, TranscriptViewer } from "@/components/transcription/transcript-viewer";
import { TranscriptionJob } from "@/components/transcription/transcription-job";
import { Button } from "@/components/ui/button";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuTrigger,
} from "@/components/ui/dropdown-menu";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { IconHint } from "@/components/ui/tooltip";
import {
  listenTranscriptWritten,
  type MarkerEditReq,
  type MediaBlockVm,
  type MediaMarkerVm,
  mediaBlockClip,
  mediaBlockEdit,
  mediaBlockResolve,
} from "@/lib/ipc/client";
import { useCapabilitiesStore } from "@/lib/stores/capabilities";
import { syncErrorMessage } from "@/lib/stores/sync";
import {
  startTranscription,
  transcriptionRunning,
  useTranscriptionStore,
} from "@/lib/stores/transcription";
import { cn } from "@/lib/utils";
import type { MediaBlockMountArgs } from "./editor/media-block";
import { clock } from "./editor/media-playback";
import { LiveRecordingBlock, MediaRecorderPanel, useBlockRecording } from "./media-recorder-panel";

export const MEDIA_BLOCK_MENU_LABEL = "Media block actions";
export const OPEN_TRANSCRIPT_LABEL = "Open transcript";
export const COPY_CLIP_LABEL = "Copy clip…";
export const MARK_MOMENT_LABEL = "Mark this moment";
export const MARK_WINDOW_LABEL = "Mark a window…";
export const EDIT_BLOCK_SOURCE_LABEL = "Edit block source";
export const REMOVE_WIDGET_LABEL = "Remove widget";
export const NOT_TRANSCRIBED_SENTENCE = "Not transcribed yet.";
export const TRANSCRIBE_LABEL = "Transcribe";
export const MARKER_COPY_LINK = "Copy link";
export const MARKER_COPY_CLIP = "Copy clip";
export const MARKER_RENAME = "Rename…";
export const MARKER_REMOVE = "Remove";
/** What the player's box says while it waits to be near the screen. */
const PLAYER_ASLEEP = "The player starts when this block is on screen.";
/** A splice that found the block changed under it. */
const BLOCK_MOVED = "The block changed while keeper was answering. Try again.";

/** One screen of margin: a block about to scroll in has its first frame ready. */
const NEAR_MARGIN = "100% 0px";

/** The characters a marker name may not carry (a wikilink cannot), and a line break. */
const NOT_IN_A_NAME = /[[\]|#^\r\n]+/g;

/** The first words of the line being said, as a name a marker may carry. */
function nameFrom(text: string | undefined): string {
  if (text === undefined) return "";
  const cleaned = text.replace(NOT_IN_A_NAME, " ").replace(/\s+/g, " ").trim();
  const words = cleaned.split(" ").slice(0, 6).join(" ");
  return words.length > 80 ? words.slice(0, 80).trimEnd() : words;
}

/**
 * Seconds from what a person typed as a time: `hh:mm:ss`, `mm:ss` or seconds.
 * `null` for anything else. Only for this form's two fields — the block's own
 * body is read by Rust alone, which also checks the window it is given.
 */
function secondsFrom(typed: string): number | null {
  const parts = typed.trim().split(":");
  if (parts.length > 3 || parts.some((part) => !/^\d+(\.\d+)?$/.test(part))) return null;
  return parts.reduce((total, part) => total * 60 + Number(part), 0);
}

type Naming =
  | { mode: "moment"; name: string; at: number }
  | { mode: "window"; name: string; from: string; to: string }
  | { mode: "rename"; name: string; marker: string };

/** Whichever player is mounted: the viewer's, or the untranscribed block's own. */
type Seekable = Pick<TranscriptPlayerHandle, "seek" | "pause">;

export function MediaBlockPanel({
  profileId,
  source,
  editable,
  interactive,
  replaceSource,
  noteLink,
  notePath,
  saveNote,
  register,
  claimPlayback,
  scroller,
  editSource,
  remove,
}: MediaBlockMountArgs) {
  /** What Rust says the body is — in one round trip: a `record = "new"` block
   *  draws its recorder, and the block naming the live session its live view. */
  const { kind: recording, ended: recordingEnded } = useBlockRecording(source, profileId, notePath);
  const [vm, setVm] = useState<MediaBlockVm | null>(null);
  const [refusal, setRefusal] = useState<string | null>(null);
  /** Bumped by a transcript write, to resolve the same body again. */
  const [reads, setReads] = useState(0);
  const [near, setNear] = useState(false);
  const [playing, setPlaying] = useState(false);
  const [time, setTime] = useState<number | null>(null);
  /** `time` for the layer, which asks outside any render. */
  const timeNow = useRef<number | null>(null);
  timeNow.current = time;
  const [follow, setFollow] = useState(true);
  const [clip, setClip] = useState<ClipRequest | null>(null);
  const [naming, setNaming] = useState<Naming | null>(null);
  const [problem, setProblem] = useState<string | null>(null);
  const [viewer, setViewer] = useState<{ path: string; at: number } | null>(null);
  const [removing, setRemoving] = useState<string | null>(null);
  // A state, not a ref: the panel's box exists only once the block has
  // resolved, and the observer has to start then, not on the first render.
  const [root, setRoot] = useState<HTMLElement | null>(null);
  const player = useRef<Seekable | null>(null);
  /** A seek asked before anything that plays was mounted, taken when it is. */
  const pendingSeek = useRef<number | null>(null);
  /** Where a window chip's playback pauses. */
  const stopAt = useRef<number | null>(null);
  const release = useRef<(() => void) | null>(null);
  const canTranscribe = useCapabilitiesStore((s) => s.capabilities.transcription);
  const transcribePath = vm?.transcribePath ?? null;
  const job = useTranscriptionStore((s) => (transcribePath ? s.jobs[transcribePath] : undefined));

  // biome-ignore lint/correctness/useExhaustiveDependencies: `reads` is the trigger — a transcript write asks Rust again about the same body
  useEffect(() => {
    let live = true;
    void mediaBlockResolve(profileId, source).then(
      (resolved) => {
        if (!live) return;
        setVm(resolved);
        setRefusal(null);
      },
      (cause: unknown) => {
        if (!live) return;
        setVm(null);
        setRefusal(syncErrorMessage(cause));
      },
    );
    return () => {
      live = false;
    };
  }, [profileId, source, reads]);

  const transcriptPath = vm?.transcriptPath ?? null;
  useEffect(() => {
    if (transcriptPath === null) return;
    let unlisten: (() => void) | null = null;
    let live = true;
    void listenTranscriptWritten((path) => {
      if (path === transcriptPath) setReads((n) => n + 1);
    }).then(
      (stop) => {
        if (live) unlisten = stop;
        else stop();
      },
      () => {},
    );
    return () => {
      live = false;
      unlisten?.();
    };
  }, [transcriptPath]);

  useEffect(() => {
    if (root === null) {
      return;
    }
    if (typeof IntersectionObserver === "undefined") {
      setNear(true);
      return;
    }
    const observer = new IntersectionObserver(
      (entries) => {
        for (const entry of entries) setNear(entry.isIntersecting);
      },
      { rootMargin: NEAR_MARGIN },
    );
    observer.observe(root);
    return () => observer.disconnect();
  }, [root]);

  const seekTo = useCallback((seconds: number, play?: boolean) => {
    setTime(seconds);
    if (player.current === null) {
      pendingSeek.current = seconds;
      return;
    }
    player.current.seek(seconds, play);
  }, []);

  useEffect(
    () =>
      register({ seekTo: (seconds) => seekTo(seconds, false), currentTime: () => timeNow.current }),
    [register, seekTo],
  );

  const attachPlayer = useCallback((handle: Seekable | null) => {
    player.current = handle;
    if (handle !== null && pendingSeek.current !== null) {
      handle.seek(pendingSeek.current, false);
      pendingSeek.current = null;
    }
  }, []);

  const onPlayingChange = useCallback(
    (now: boolean) => {
      setPlaying(now);
      release.current?.();
      release.current = now ? claimPlayback(() => player.current?.pause()) : null;
      if (!now) stopAt.current = null;
    },
    [claimPlayback],
  );
  useEffect(() => () => release.current?.(), []);

  const onTime = useCallback((seconds: number) => {
    setTime(seconds);
    if (stopAt.current !== null && seconds >= stopAt.current) {
      stopAt.current = null;
      player.current?.pause();
    }
  }, []);

  const composeClip = useCallback(
    (from: string | null, to: string | null, words: boolean) =>
      mediaBlockClip(profileId, source, from, to, words),
    [profileId, source],
  );

  const lines = vm?.lines ?? [];
  const current = time === null ? -1 : currentUtterance(lines, time);
  const at = time ?? vm?.window.from ?? 0;
  const range =
    vm === null || (vm.window.from === 0 && vm.window.to === null)
      ? undefined
      : { from: vm.window.from, to: vm.window.to ?? vm.duration };

  async function edit(change: MarkerEditReq): Promise<boolean> {
    try {
      const next = await mediaBlockEdit(source, change);
      if (!replaceSource(next)) {
        setProblem(BLOCK_MOVED);
        return false;
      }
      setProblem(null);
      return true;
    } catch (cause) {
      setProblem(syncErrorMessage(cause));
      return false;
    }
  }

  // The block's own verbs. A transcribed block appends them to the viewer's ⋯;
  // otherwise they are the block's only menu. A preview has none.
  const session = recording?.session ?? null;
  const editItems = (
    <>
      <DropdownMenuItem onSelect={editSource}>{EDIT_BLOCK_SOURCE_LABEL}</DropdownMenuItem>
      <DropdownMenuItem variant="destructive" onSelect={remove}>
        {REMOVE_WIDGET_LABEL}
      </DropdownMenuItem>
      {session !== null && (
        <DropdownMenuItem variant="destructive" onSelect={() => setRemoving(session)}>
          {REMOVE_RECORDING_LABEL}
        </DropdownMenuItem>
      )}
    </>
  );
  // The recording is gone from the drive: this widget goes too, saved at
  // once as ⌘S would — the note is open here, so Rust left its body alone.
  const removeDialog =
    removing === null ? null : (
      <RemoveRecordingDialog
        sessionId={removing}
        onClose={() => setRemoving(null)}
        onRemoved={() => {
          remove();
          saveNote();
        }}
      />
    );
  const blockItems = (
    <>
      {vm?.transcribed && vm.transcriptPath !== null && (
        <DropdownMenuItem onSelect={() => setViewer({ path: vm.transcriptPath ?? "", at })}>
          {OPEN_TRANSCRIPT_LABEL}
        </DropdownMenuItem>
      )}
      {vm !== null && !vm.transcribed && (
        <DropdownMenuItem
          onSelect={() =>
            setClip({ from: range?.from ?? null, to: range === undefined ? null : range.to })
          }
        >
          {COPY_CLIP_LABEL}
        </DropdownMenuItem>
      )}
      {editable && vm !== null && (
        <>
          <DropdownMenuItem
            onSelect={() =>
              setNaming({
                mode: "moment",
                name: nameFrom(lines[current]?.text),
                at: Math.floor(at),
              })
            }
          >
            {MARK_MOMENT_LABEL}
          </DropdownMenuItem>
          <DropdownMenuItem
            onSelect={() => {
              const line = lines[current];
              setNaming({
                mode: "window",
                name: nameFrom(line?.text),
                from: timestamp(line?.start ?? at),
                to: timestamp(Math.ceil(line?.end ?? at + 30)),
              });
            }}
          >
            {MARK_WINDOW_LABEL}
          </DropdownMenuItem>
        </>
      )}
      {editable && editItems}
    </>
  );
  const blockMenu = interactive ? <BlockMenu>{blockItems}</BlockMenu> : null;

  // Nothing of the body is drawn before Rust has said what it is: the fence's
  // text is the source view's, never a block's face.
  if (recording === null) {
    return null;
  }
  if (recording.records) {
    // Rust refuses to resolve a block that has not recorded yet, so its
    // refusal is never the block's face.
    return (
      <MediaRecorderPanel
        profileId={profileId}
        source={source}
        notePath={notePath}
        noteLink={noteLink}
        preview={!interactive || !editable}
        replaceSource={replaceSource}
        saveNote={saveNote}
        menu={editable ? <BlockMenu>{editItems}</BlockMenu> : undefined}
      />
    );
  }
  if (recording.live) {
    // The session is still recording, so there is nothing to play yet: the
    // banner where it records, a line anywhere else.
    return <LiveRecordingBlock controls={recording.here && interactive} onEnded={recordingEnded} />;
  }

  if (refusal !== null) {
    return (
      <section aria-label="Media block" className="flex flex-col gap-2 text-sm">
        <div className="flex items-start gap-2">
          <p role="alert" className="min-w-0 flex-1 text-destructive">
            {refusal}
          </p>
          {blockMenu}
        </div>
        <pre className="overflow-auto whitespace-pre-wrap rounded-md bg-muted p-2 font-mono text-xs">
          {source}
        </pre>
        {removeDialog}
      </section>
    );
  }
  if (vm === null) {
    return null;
  }

  const mountPlayer = near || playing;
  const running = transcriptionRunning(job);
  const link = (name: string) => `[[${noteLink ?? ""}#${name}]]`;
  const markers =
    vm.markers.length === 0 ? null : (
      <ul aria-label="Markers" className="flex flex-wrap gap-1.5">
        {vm.markers.map((marker) => (
          <MarkerChip
            key={marker.name}
            marker={marker}
            editable={editable}
            menu={interactive}
            onSeek={() => {
              if (marker.to === null) {
                // A moment keeps the player as it was: playing stays playing.
                seekTo(marker.from);
              } else {
                seekTo(marker.from, true);
                stopAt.current = marker.to;
              }
            }}
            onCopyLink={() => {
              void navigator.clipboard?.writeText(link(marker.name)).catch(() => {});
            }}
            onCopyClip={() => setClip({ from: marker.from, to: marker.to ?? marker.from + 1 })}
            onRename={() => setNaming({ mode: "rename", name: marker.name, marker: marker.name })}
            onRemove={() => void edit({ op: "remove", name: marker.name })}
          />
        ))}
      </ul>
    );
  const problemLine =
    problem !== null && naming === null ? (
      <p role="alert" className="text-destructive text-xs">
        {problem}
      </p>
    ) : null;

  return (
    <section
      ref={setRoot}
      aria-label={vm.title ?? "Media"}
      className="flex min-w-0 flex-col gap-2 text-sm"
    >
      {vm.transcribed && vm.transcriptPath !== null ? (
        <>
          {problemLine}
          <TranscriptViewer
            ref={attachPlayer}
            path={vm.transcriptPath}
            profileId={profileId}
            scroller={scroller}
            window={vm.window}
            title={null}
            media={vm.media}
            initialPicture={vm.picture ?? undefined}
            initialSound={vm.sound ?? undefined}
            playerAwake={mountPlayer}
            asleepText={PLAYER_ASLEEP}
            markers={markers}
            menuItems={blockItems}
            readOnly={!interactive}
            composeClip={composeClip}
            onTime={onTime}
            onPlayingChange={onPlayingChange}
          />
        </>
      ) : (
        <>
          {mountPlayer ? (
            <TranscriptPlayer
              ref={attachPlayer}
              media={vm.media}
              window={range}
              initialPicture={vm.picture ?? undefined}
              initialSound={vm.sound ?? undefined}
              follow={follow}
              onFollowChange={setFollow}
              onTime={onTime}
              onJump={() => {}}
              onPlayingChange={onPlayingChange}
            />
          ) : (
            <div className="flex aspect-video max-h-[30dvh] w-full items-center justify-center rounded-md bg-muted text-muted-foreground text-xs">
              {PLAYER_ASLEEP}
            </div>
          )}
          {markers}
          {problemLine}
          <div className="flex flex-col gap-2">
            <div className="flex flex-wrap items-center gap-2">
              <p className="text-muted-foreground">{NOT_TRANSCRIBED_SENTENCE}</p>
              {interactive && canTranscribe && transcribePath !== null && !running && (
                <Button
                  size="sm"
                  variant="outline"
                  onClick={() =>
                    void startTranscription(transcribePath, () => setReads((n) => n + 1))
                  }
                >
                  {TRANSCRIBE_LABEL}
                </Button>
              )}
              {blockMenu && <div className="ml-auto">{blockMenu}</div>}
            </div>
            {transcribePath !== null && (
              <TranscriptionJob path={transcribePath} onOpen={() => setReads((n) => n + 1)} />
            )}
          </div>
        </>
      )}

      <CopyClipDialog request={clip} compose={composeClip} onClose={() => setClip(null)} />
      <NamingDialog
        naming={naming}
        problem={problem}
        onClose={() => {
          setNaming(null);
          setProblem(null);
        }}
        onSubmit={async (done) => {
          if (done.mode === "rename") {
            return await edit({ op: "rename", name: done.marker, newName: done.name });
          }
          if (done.mode === "moment") {
            return await edit({ op: "add", name: done.name, from: done.at, to: null });
          }
          const from = secondsFrom(done.from);
          const to = secondsFrom(done.to);
          if (from === null || to === null) {
            setProblem("Write each time as hh:mm:ss.");
            return false;
          }
          return await edit({ op: "add", name: done.name, from, to });
        }}
      />
      <TranscriptDialog
        path={viewer?.path ?? null}
        at={viewer?.at}
        profileId={profileId}
        onClose={() => setViewer(null)}
      />
      {removeDialog}
    </section>
  );
}

/** The block's ⋯ where there is no viewer to append its verbs to. */
function BlockMenu({ children }: { children: ReactNode }) {
  return (
    <DropdownMenu>
      <IconHint label={MEDIA_BLOCK_MENU_LABEL}>
        <DropdownMenuTrigger asChild>
          <Button
            size="icon"
            variant="ghost"
            className="size-7 shrink-0"
            aria-label={MEDIA_BLOCK_MENU_LABEL}
          >
            <Ellipsis aria-hidden="true" />
          </Button>
        </DropdownMenuTrigger>
      </IconHint>
      <DropdownMenuContent align="end" className="w-auto min-w-44">
        {children}
      </DropdownMenuContent>
    </DropdownMenu>
  );
}

function MarkerChip({
  marker,
  editable,
  menu,
  onSeek,
  onCopyLink,
  onCopyClip,
  onRename,
  onRemove,
}: {
  marker: MediaMarkerVm;
  editable: boolean;
  /** The chip's ⋯; a preview's chip only seeks. */
  menu: boolean;
  onSeek: () => void;
  onCopyLink: () => void;
  onCopyClip: () => void;
  onRename: () => void;
  onRemove: () => void;
}) {
  const when =
    marker.to === null ? clock(marker.from) : `${clock(marker.from)}–${clock(marker.to)}`;
  return (
    <li className="flex items-center rounded-full border bg-muted/50 text-xs">
      <button
        type="button"
        onClick={onSeek}
        aria-label={`${marker.to === null ? "Go to" : "Play"} ${marker.name}, ${when}`}
        className={cn(
          "flex items-center gap-1 py-0.5 pl-2.5 hover:bg-accent focus-visible:outline-2 focus-visible:outline-ring",
          menu ? "rounded-l-full pr-1" : "rounded-full pr-2.5",
        )}
      >
        {marker.to !== null && <MoveHorizontal aria-hidden="true" className="size-3" />}
        <span className="max-w-48 truncate">{marker.name}</span>
        <span className="figures text-muted-foreground">· {when}</span>
      </button>
      {menu && (
        <DropdownMenu>
          <DropdownMenuTrigger asChild>
            <button
              type="button"
              aria-label={`Marker actions ${marker.name}`}
              className="rounded-r-full py-0.5 pr-2 pl-1 text-muted-foreground hover:bg-accent focus-visible:outline-2 focus-visible:outline-ring"
            >
              <Ellipsis aria-hidden="true" className="size-3" />
            </button>
          </DropdownMenuTrigger>
          <DropdownMenuContent align="start" className="w-auto min-w-40">
            <DropdownMenuItem onSelect={onCopyLink}>{MARKER_COPY_LINK}</DropdownMenuItem>
            <DropdownMenuItem onSelect={onCopyClip}>{MARKER_COPY_CLIP}</DropdownMenuItem>
            {editable && (
              <>
                <DropdownMenuItem onSelect={onRename}>{MARKER_RENAME}</DropdownMenuItem>
                <DropdownMenuItem onSelect={onRemove}>{MARKER_REMOVE}</DropdownMenuItem>
              </>
            )}
          </DropdownMenuContent>
        </DropdownMenu>
      )}
    </li>
  );
}

/** Name a moment, name a window, or rename a marker: one small form, three titles. */
function NamingDialog({
  naming,
  problem,
  onClose,
  onSubmit,
}: {
  naming: Naming | null;
  problem: string | null;
  onClose: () => void;
  onSubmit: (done: Naming) => Promise<boolean>;
}) {
  return (
    <Dialog
      open={naming !== null}
      onOpenChange={(open) => {
        if (!open) onClose();
      }}
    >
      <DialogContent className="sm:max-w-sm">
        {naming && (
          <NamingForm naming={naming} problem={problem} onClose={onClose} onSubmit={onSubmit} />
        )}
      </DialogContent>
    </Dialog>
  );
}

function NamingForm({
  naming,
  problem,
  onClose,
  onSubmit,
}: {
  naming: Naming;
  problem: string | null;
  onClose: () => void;
  onSubmit: (done: Naming) => Promise<boolean>;
}) {
  const [draft, setDraft] = useState<Naming>(naming);
  const [busy, setBusy] = useState(false);
  const id = useId();
  const heading =
    naming.mode === "moment"
      ? MARK_MOMENT_LABEL
      : naming.mode === "window"
        ? "Mark a window"
        : `Rename ${naming.marker}`;
  async function submit(event: FormEvent) {
    event.preventDefault();
    setBusy(true);
    const ok = await onSubmit(draft);
    setBusy(false);
    if (ok) onClose();
  }
  return (
    <form onSubmit={(event) => void submit(event)} className="flex flex-col gap-3">
      <DialogHeader>
        <DialogTitle>{heading}</DialogTitle>
        <DialogDescription>
          {draft.mode === "moment"
            ? `At ${timestamp(draft.at)}. The marker is written into this block.`
            : "The marker is written into this block, and a link to it opens this note there."}
        </DialogDescription>
      </DialogHeader>
      <div className="flex flex-col gap-1.5">
        <Label htmlFor={`${id}-name`}>Name</Label>
        <Input
          id={`${id}-name`}
          value={draft.name}
          autoFocus
          onChange={(event) => setDraft({ ...draft, name: event.target.value })}
        />
      </div>
      {draft.mode === "window" && (
        <div className="flex gap-2">
          <div className="flex flex-1 flex-col gap-1.5">
            <Label htmlFor={`${id}-from`}>From</Label>
            <Input
              id={`${id}-from`}
              value={draft.from}
              className="figures"
              onChange={(event) => setDraft({ ...draft, from: event.target.value })}
            />
          </div>
          <div className="flex flex-1 flex-col gap-1.5">
            <Label htmlFor={`${id}-to`}>To</Label>
            <Input
              id={`${id}-to`}
              value={draft.to}
              className="figures"
              onChange={(event) => setDraft({ ...draft, to: event.target.value })}
            />
          </div>
        </div>
      )}
      {problem !== null && (
        <p role="alert" className="text-destructive text-xs">
          {problem}
        </p>
      )}
      <DialogFooter>
        <Button type="button" variant="ghost" onClick={onClose}>
          Cancel
        </Button>
        <Button type="submit" disabled={busy || draft.name.trim() === ""}>
          {naming.mode === "rename" ? "Rename" : "Mark"}
        </Button>
      </DialogFooter>
    </form>
  );
}
