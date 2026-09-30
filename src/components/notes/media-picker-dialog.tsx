/**
 * What a new media block plays: a recording, or a media or transcript file in
 * the note's drive — or a recording not made yet, a block that records here
 * (the *Media player…* row of *Insert widget* and of `/`).
 *
 * The dialog chooses and Rust composes: the pick goes to `media_block_compose`
 * as a session id or a drive-relative path the listing itself answered with,
 * and the block text that comes back is what lands in the note (AD-65). No
 * path is joined here and no block is spelled here.
 *
 * Recordings come from the recordings index, newest first, searched as the
 * Recordings pane searches. Files come from the drive's own listing
 * (`sync_browse`), folder by folder; only what a player can play or a
 * transcript is offered.
 */
import { ChevronLeft, Folder } from "lucide-react";
import { useEffect, useId, useState } from "react";
import { Button } from "@/components/ui/button";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import { Input } from "@/components/ui/input";
import { Tabs, TabsContent, TabsList, TabsTrigger } from "@/components/ui/tabs";
import {
  type FilesEntryVm,
  type FilesListingVm,
  type MediaPickReq,
  mediaBlockCompose,
  type RecordingHitVm,
  searchRecordings,
  syncBrowse,
} from "@/lib/ipc/client";
import { useCapabilitiesStore } from "@/lib/stores/capabilities";
import { syncErrorMessage } from "@/lib/stores/sync";
import { clock } from "./editor/media-playback";

export const MEDIA_PICKER_TITLE = "Insert a media player";
/** The tab, and its button, that insert a block which records here. */
export const NEW_RECORDING_LABEL = "New recording";
const SEARCH_DELAY_MS = 200;
const RECORDINGS_SHOWN = 50;

/** A file a media block can name: something it plays, or a transcript. */
function playable(entry: FilesEntryVm): boolean {
  return (
    entry.kind === "video" ||
    entry.kind === "audio" ||
    entry.name === "transcript.json" ||
    entry.name.endsWith(".transcript.json")
  );
}

export function MediaPickerDialog({
  open,
  profileId,
  onClose,
  onPicked,
}: {
  open: boolean;
  /** The note's drive. */
  profileId: string;
  onClose: () => void;
  /** The block Rust composed, ready to insert. */
  onPicked: (block: string) => void;
}) {
  const [problem, setProblem] = useState<string | null>(null);
  // Recording is a Mac's (desktop macOS ≥ 13): elsewhere there is nothing to
  // start, so no block that records is offered.
  const canRecord = useCapabilitiesStore((s) => s.capabilities.recording);
  const [busy, setBusy] = useState(false);
  async function compose(pick: MediaPickReq) {
    setBusy(true);
    try {
      const block = await mediaBlockCompose(profileId, pick);
      setProblem(null);
      onPicked(block);
    } catch (cause) {
      setProblem(syncErrorMessage(cause));
    } finally {
      setBusy(false);
    }
  }
  return (
    <Dialog
      open={open}
      onOpenChange={(next) => {
        if (!next) {
          setProblem(null);
          onClose();
        }
      }}
    >
      <DialogContent className="flex max-h-[80dvh] flex-col sm:max-w-lg">
        <DialogHeader>
          <DialogTitle>{MEDIA_PICKER_TITLE}</DialogTitle>
          <DialogDescription>
            Play a recording or a file from this drive inside the note, with its transcript under
            it.
          </DialogDescription>
        </DialogHeader>
        <Tabs defaultValue="recordings" className="flex min-h-0 flex-1 flex-col">
          <TabsList>
            <TabsTrigger value="recordings">Recordings</TabsTrigger>
            <TabsTrigger value="files">Files in this drive</TabsTrigger>
            {canRecord && <TabsTrigger value="new">{NEW_RECORDING_LABEL}</TabsTrigger>}
          </TabsList>
          <TabsContent value="recordings" className="flex min-h-0 flex-1 flex-col">
            {open && (
              <RecordingChoices
                busy={busy}
                onPick={(sessionId) => void compose({ kind: "session", sessionId })}
              />
            )}
          </TabsContent>
          <TabsContent value="files" className="flex min-h-0 flex-1 flex-col">
            {open && (
              <FileChoices
                profileId={profileId}
                busy={busy}
                onPick={(relativePath) => void compose({ kind: "file", relativePath })}
              />
            )}
          </TabsContent>
          {canRecord && (
            <TabsContent value="new" className="flex flex-col items-start gap-3">
              <p className="text-muted-foreground text-sm">
                A block that records here: press Start in it, and when you stop, it plays the
                recording.
              </p>
              <Button
                type="button"
                disabled={busy}
                onClick={() => void compose({ kind: "newRecording" })}
              >
                {NEW_RECORDING_LABEL}
              </Button>
            </TabsContent>
          )}
        </Tabs>
        {problem !== null && (
          <p role="alert" className="text-destructive text-sm">
            {problem}
          </p>
        )}
      </DialogContent>
    </Dialog>
  );
}

function RecordingChoices({
  busy,
  onPick,
}: {
  busy: boolean;
  onPick: (sessionId: string) => void;
}) {
  const id = useId();
  const [query, setQuery] = useState("");
  const [rows, setRows] = useState<RecordingHitVm[] | null>(null);
  const [problem, setProblem] = useState<string | null>(null);
  useEffect(() => {
    let live = true;
    const timer = setTimeout(() => {
      void searchRecordings({
        query,
        tags: [],
        participant: null,
        startTs: null,
        endTs: null,
        durability: null,
        profileId: null,
        limit: RECORDINGS_SHOWN,
      }).then(
        (found) => {
          if (!live) return;
          setRows(found.rows);
          setProblem(null);
        },
        (cause: unknown) => {
          if (live) setProblem(syncErrorMessage(cause));
        },
      );
    }, SEARCH_DELAY_MS);
    return () => {
      live = false;
      clearTimeout(timer);
    };
  }, [query]);
  return (
    <div className="flex min-h-0 flex-1 flex-col gap-2 pt-2">
      <label className="sr-only" htmlFor={`${id}-search`}>
        Search recordings
      </label>
      <Input
        id={`${id}-search`}
        placeholder="Search recordings"
        value={query}
        autoFocus
        onChange={(event) => setQuery(event.target.value)}
      />
      {problem !== null && <p className="text-destructive text-sm">{problem}</p>}
      {rows !== null && rows.length === 0 && (
        <p className="text-muted-foreground text-sm">
          {query.trim() === ""
            ? "keeper knows no recordings on this Mac."
            : `No recording matches “${query}”.`}
        </p>
      )}
      <ul aria-label="Recordings" className="min-h-0 flex-1 overflow-auto">
        {rows?.map((row) => (
          <li key={row.sessionId}>
            <button
              type="button"
              disabled={busy}
              onClick={() => onPick(row.sessionId)}
              className="flex w-full min-w-0 items-baseline gap-2 rounded-md px-2 py-1.5 text-left hover:bg-accent focus-visible:outline-2 focus-visible:outline-ring disabled:opacity-50"
            >
              <span className="min-w-0 flex-1 truncate">{row.title ?? row.relativePath}</span>
              <span className="figures shrink-0 text-meta text-muted-foreground">
                {row.startedTs === null ? "" : new Date(row.startedTs).toLocaleDateString()}
                {row.durationMs === null ? "" : ` · ${clock(row.durationMs / 1000)}`}
              </span>
            </button>
          </li>
        ))}
      </ul>
    </div>
  );
}

function FileChoices({
  profileId,
  busy,
  onPick,
}: {
  profileId: string;
  busy: boolean;
  onPick: (relativePath: string) => void;
}) {
  const [subpath, setSubpath] = useState("");
  const [listing, setListing] = useState<FilesListingVm | null>(null);
  const [problem, setProblem] = useState<string | null>(null);
  useEffect(() => {
    let live = true;
    setListing(null);
    void syncBrowse(profileId, subpath).then(
      (found) => {
        if (!live) return;
        setListing(found);
        setProblem(null);
      },
      (cause: unknown) => {
        if (live) setProblem(syncErrorMessage(cause));
      },
    );
    return () => {
      live = false;
    };
  }, [profileId, subpath]);
  const entries = (listing?.entries ?? []).filter(
    (entry) => entry.kind === "folder" || playable(entry),
  );
  const parent = subpath.includes("/") ? subpath.slice(0, subpath.lastIndexOf("/")) : "";
  return (
    <div className="flex min-h-0 flex-1 flex-col gap-2 pt-2">
      <div className="flex min-w-0 items-center gap-2">
        {subpath !== "" && (
          <Button
            size="icon-sm"
            variant="ghost"
            aria-label="Up one folder"
            onClick={() => setSubpath(parent)}
          >
            <ChevronLeft aria-hidden="true" />
          </Button>
        )}
        <span className="min-w-0 truncate font-mono text-meta text-muted-foreground">
          {subpath === "" ? "This drive" : subpath}
        </span>
      </div>
      {problem !== null && <p className="text-destructive text-sm">{problem}</p>}
      {listing !== null && listing.state !== "listed" && (
        <p className="text-muted-foreground text-sm">
          {listing.detail ?? "keeper cannot list this folder."}
        </p>
      )}
      {listing !== null && listing.state === "listed" && entries.length === 0 && (
        <p className="text-muted-foreground text-sm">
          Nothing here plays: no video, audio or transcript.
        </p>
      )}
      <ul aria-label="Files" className="min-h-0 flex-1 overflow-auto">
        {entries.map((entry) => (
          <li key={entry.relativePath}>
            <button
              type="button"
              disabled={busy}
              onClick={() =>
                entry.kind === "folder"
                  ? setSubpath(entry.relativePath)
                  : onPick(entry.relativePath)
              }
              className="flex w-full min-w-0 items-center gap-2 rounded-md px-2 py-1.5 text-left hover:bg-accent focus-visible:outline-2 focus-visible:outline-ring disabled:opacity-50"
            >
              {entry.kind === "folder" && (
                <Folder aria-hidden="true" className="size-4 shrink-0 text-muted-foreground" />
              )}
              <span className="min-w-0 flex-1 truncate">{entry.name}</span>
              {entry.kind !== "folder" && (
                <span className="shrink-0 text-meta text-muted-foreground">
                  {entry.kind === "file" ? "transcript" : entry.kind}
                </span>
              )}
            </button>
          </li>
        ))}
      </ul>
    </div>
  );
}
