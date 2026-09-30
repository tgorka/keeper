/**
 * What a media block draws for recording (story 88.9): a `record = "new"`
 * block's recorder — the Recording pane's setup and live controls, inside
 * the note — and the live view of the block that names the session
 * recording right now.
 *
 * Start records a session linked to this note. The moment the start
 * answers, the block that pressed it splices Rust's `session = "<id>"` body
 * over its own fence and the note is written at once, so from then on the
 * block IS the session's: it draws the live banner with Stop while that
 * session records in this note, and the player once it stops. Only one
 * session records at a time, so every record block while it does — in this
 * note or any other — says where the recording is and offers no Start.
 *
 * What a body is — a record block, or which session it names — is Rust's
 * answer (`media_block_recording`): TypeScript never reads a key (AD-351).
 */
import { type ReactNode, useCallback, useEffect, useRef, useState } from "react";
import {
  START_RECORDING_LABEL,
  startBlockedNote,
  startGate,
} from "@/components/layout/recording-pane";
import { ActiveRecordingBanner } from "@/components/recording/active-recording-banner";
import { RecordingAudioControls } from "@/components/recording/recording-audio-controls";
import { RecordingSourcePicker } from "@/components/recording/recording-source-picker";
import { RecordingWebcamControls } from "@/components/recording/recording-webcam-controls";
import { Button } from "@/components/ui/button";
import { useRecordingPermission } from "@/hooks/use-recording-permission";
import { isLiveRecording, useRecordingSession } from "@/hooks/use-recording-session";
import {
  type MediaBlockRecordingVm,
  mediaBlockRecording,
  mediaBlockRecordStarted,
  type RecordingLinkedNoteVm,
  recordingLinkedNote,
} from "@/lib/ipc/client";
import { resolveWikilink } from "@/lib/notes/follow-link";
import { notesVaultsStore, setActiveVault } from "@/lib/stores/notes-vaults";
import { panelsStore } from "@/lib/stores/panels";
import { primaryViewStore } from "@/lib/stores/primary-view";
import { systemAudioEnabled, useSystemAudioEnabled } from "@/lib/stores/recording-audio";
import { micDeviceId, micEnabled, useMicEnabled } from "@/lib/stores/recording-mic";
import {
  ensureRecordingSettingsHydrated,
  useRecordingSettings,
} from "@/lib/stores/recording-settings";
import { selectedRecordingTarget, useSelectedRecordingTarget } from "@/lib/stores/recording-source";
import { cameraDeviceId, webcamEnabled } from "@/lib/stores/recording-webcam";

/** What a record block says where it cannot record: Preview, a read-only note. */
export const NOT_RECORDED_SENTENCE = "Not recorded yet.";

/** What a record block says while a recording started elsewhere runs. */
export const RECORDING_ELSEWHERE_SENTENCE = "A recording is running.";

/** What a block naming the live session says where it has no controls. */
export const RECORDING_NOW_SENTENCE = "Recording…";

/** What the block that pressed Start says when it could not take the session's name. */
export const RECORDING_UNNAMED_SENTENCE =
  "Recording — this block could not take its name; stop it in the Recording pane.";

/** The words before the link to the note that is recording. */
export const RECORDING_IN_LABEL = "Recording in";

/** What a record block outside a vault says: the session could not be linked. */
export const NO_NOTE_SENTENCE = "Save this note in a vault to record into it.";

/** Where the next recording is saved, as the setup names it. */
export function destinationSentence(place: string): string {
  return `Saves to ${place}.`;
}

/**
 * What Rust says the body `source` is, for the block in the note `notePath`
 * of vault `profileId` — one round trip: a record block, the session it
 * names, and whether that session is recording now, into this note. `null`
 * while Rust is asked. `ended` says the live session has stopped, so the
 * block goes back to its player without asking again.
 */
export function useBlockRecording(
  source: string,
  profileId: string,
  notePath: () => string | null,
): { kind: MediaBlockRecordingVm | null; ended: () => void } {
  const [answer, setAnswer] = useState<{ source: string; kind: MediaBlockRecordingVm } | null>(
    null,
  );
  const path = useRef(notePath);
  path.current = notePath;
  useEffect(() => {
    let live = true;
    void mediaBlockRecording(source, profileId, path.current()).then(
      (kind) => {
        if (live) setAnswer({ source, kind });
      },
      () => {
        if (live) {
          setAnswer({ source, kind: { records: false, session: null, live: false, here: false } });
        }
      },
    );
    return () => {
      live = false;
    };
  }, [source, profileId]);
  const ended = useCallback(() => {
    setAnswer((held) =>
      held === null ? held : { ...held, kind: { ...held.kind, live: false, here: false } },
    );
  }, []);
  // An answer about another body is no answer about this one.
  return { kind: answer?.source === source ? answer.kind : null, ended };
}

/** Open the note `link` names, switching vaults when it is in another. */
async function openLinkedNote(link: RecordingLinkedNoteVm): Promise<void> {
  const found = await resolveWikilink(link.profileId, link.path.replace(/\.md$/i, ""));
  if (found.note === null) return;
  if (notesVaultsStore.getState().activeVaultId !== link.profileId) {
    await setActiveVault(link.profileId);
    if (notesVaultsStore.getState().activeVaultId !== link.profileId) return;
  }
  panelsStore
    .getState()
    .openPanel({ kind: "note", vaultId: link.profileId, noteId: found.note.id });
  primaryViewStore.getState().setView("notes");
}

export interface MediaRecorderPanelProps {
  /** The vault the note is in (its profile's id); empty when none. */
  profileId: string;
  /** The block's body, verbatim. */
  source: string;
  /** The note's path in its vault, or `null` for a note that has none — read
   *  when asked, as the note may be renamed while the block stays mounted. */
  notePath: () => string | null;
  /** The note's link target: the next session's title. */
  noteLink: string | null;
  /** Preview, or a read-only note: the sentence alone, no controls. */
  preview: boolean;
  /** Replace this block's body; false when the block moved or changed. */
  replaceSource: (next: string) => boolean;
  /** Write the note now, as ⌘S does. */
  saveNote: () => void;
  /** The block's own ⋯ menu (Edit block source, Remove widget). */
  menu?: ReactNode;
}

export function MediaRecorderPanel(props: MediaRecorderPanelProps) {
  if (props.preview) {
    return (
      <div className="flex items-center justify-between gap-2 rounded-md border border-border p-3">
        <p className="text-muted-foreground text-sm">{NOT_RECORDED_SENTENCE}</p>
      </div>
    );
  }
  return <RecorderControls {...props} />;
}

/**
 * The block that names the session recording now. In its own note's editor
 * it is the live banner with Stop; anywhere without controls — Preview, the
 * Files preview, another note — one "Recording…" line. When the session
 * stops — or fails — `onEnded` hands the block back to its player, which
 * says what Rust knows about the session; restarts are the Recording pane's.
 */
export function LiveRecordingBlock({
  controls,
  onEnded,
}: {
  controls: boolean;
  onEnded: () => void;
}) {
  const { status, elapsed, stop } = useRecordingSession();
  const live = isLiveRecording(status);
  const seenLive = useRef(false);
  if (live) seenLive.current = true;
  const ended =
    status.state === "finalized" ||
    status.state === "failed" ||
    status.state === "recovered" ||
    (seenLive.current && !live);
  useEffect(() => {
    if (ended) onEnded();
  }, [ended, onEnded]);
  if (!controls) {
    return <p className="text-muted-foreground text-sm">{RECORDING_NOW_SENTENCE}</p>;
  }
  return (
    <section aria-label="Recording" className="flex flex-col gap-3 rounded-md border border-border">
      <ActiveRecordingBanner
        status={status}
        elapsed={elapsed}
        onStop={() => {
          void stop();
        }}
        onRestart={onEnded}
        onDismiss={onEnded}
      />
    </section>
  );
}

function RecorderControls({
  profileId,
  source,
  notePath,
  noteLink,
  replaceSource,
  saveNote,
  menu,
}: MediaRecorderPanelProps) {
  const { permission, refresh } = useRecordingPermission();
  const { status, elapsed, start, stop, acknowledge } = useRecordingSession();
  const [linked, setLinked] = useState<RecordingLinkedNoteVm | null>(null);
  /** This block started the live session but could not take its name. */
  const [unnamed, setUnnamed] = useState(false);
  const settings = useRecordingSettings();
  const live = isLiveRecording(status);

  useEffect(() => {
    void ensureRecordingSettingsHydrated();
  }, []);

  // Asked again whenever the session changes state: a start or a stop
  // anywhere moves which note is recording. With nothing recording, no note is.
  const state = status.state;
  useEffect(() => {
    if (state === "idle") {
      setLinked(null);
      return;
    }
    let current = true;
    void recordingLinkedNote().then(
      (note) => {
        if (current) setLinked(note);
      },
      () => {},
    );
    return () => {
      current = false;
    };
  }, [state]);

  const { canStart, blockedBy } = startGate(
    permission,
    useSelectedRecordingTarget().kind === "audioOnly",
    useSystemAudioEnabled(),
    useMicEnabled(),
  );

  const path = notePath();
  const inVault = path !== null && profileId !== "";

  // The block names the session the moment the start answers: Rust composes
  // the body, the splice is an ordinary edit, and the note is written at once
  // so the disk names it too. A block moved or removed meanwhile keeps its
  // `record = "new"` and says so — the recording then gets its own note when
  // it stops.
  const begin = async () => {
    const at = notePath();
    if (at === null || profileId === "") return;
    setUnnamed(false);
    const started = await start(
      selectedRecordingTarget(),
      systemAudioEnabled(),
      micEnabled(),
      micDeviceId(),
      webcamEnabled(),
      cameraDeviceId(),
      noteLink === null ? undefined : { title: noteLink },
      { profileId, path: at },
    );
    if (!started) return;
    const now = await recordingLinkedNote().catch(() => null);
    const named =
      now === null || now.profileId !== profileId || now.path !== at
        ? null
        : await mediaBlockRecordStarted(source, now.sessionId).catch(() => null);
    if (named !== null && replaceSource(named)) {
      saveNote();
    } else {
      setUnnamed(true);
    }
  };

  const banner =
    status.state === "failed" ? (
      <ActiveRecordingBanner
        status={status}
        elapsed={elapsed}
        onStop={() => {
          void stop();
        }}
        onRestart={() => {
          void begin();
        }}
        onDismiss={() => {
          void acknowledge();
        }}
      />
    ) : null;

  let body: ReactNode;
  // Start leads the widget (owner, 2026-09-30): the setup below it is three cards
  // tall, and a person who opens a note to record should not scroll to find the
  // one button the widget is for. It shares the top row with the block's ⋯.
  let startRow: ReactNode = null;
  if (live && unnamed) {
    body = <p className="text-muted-foreground text-sm">{RECORDING_UNNAMED_SENTENCE}</p>;
  } else if (live) {
    body =
      linked === null ? (
        <p className="text-muted-foreground text-sm">{RECORDING_ELSEWHERE_SENTENCE}</p>
      ) : (
        <p className="text-muted-foreground text-sm">
          {RECORDING_IN_LABEL}{" "}
          <Button
            type="button"
            variant="link"
            size="xs"
            className="h-auto p-0"
            onClick={() => {
              void openLinkedNote(linked);
            }}
          >
            {linked.title}
          </Button>
        </p>
      );
  } else if (!inVault) {
    body = <p className="text-muted-foreground text-sm">{NO_NOTE_SENTENCE}</p>;
  } else {
    const place = settings?.destinationProfileName ?? settings?.destinationDir ?? null;
    startRow = (
      <div className="flex min-w-0 flex-1 flex-wrap items-center gap-x-3 gap-y-1">
        <Button
          type="button"
          disabled={!canStart}
          onClick={() => {
            void begin();
          }}
        >
          {START_RECORDING_LABEL}
        </Button>
        {place !== null && (
          <p className="min-w-0 truncate text-muted-foreground text-xs" title={place}>
            {destinationSentence(place)}
          </p>
        )}
        {blockedBy !== null && (
          <p className="basis-full text-muted-foreground text-xs">{startBlockedNote(blockedBy)}</p>
        )}
      </div>
    );
    body = (
      <div className="flex flex-col gap-4">
        <RecordingSourcePicker active screenRecording={permission.screenRecording} />
        <RecordingAudioControls active onPermissionSettled={refresh} />
        <RecordingWebcamControls active onPermissionSettled={refresh} />
      </div>
    );
  }

  return (
    <section
      aria-label="New recording"
      className="flex flex-col gap-3 rounded-md border border-border p-3"
    >
      {(startRow !== null || menu !== undefined) && (
        <div className="flex items-start gap-2">
          {startRow ?? <div className="flex-1" />}
          {menu}
        </div>
      )}
      {banner}
      {body}
    </section>
  );
}
