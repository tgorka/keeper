import { useCallback, useEffect, useId, useRef, useState } from "react";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Checkbox } from "@/components/ui/checkbox";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { Textarea } from "@/components/ui/textarea";
import type {
  ArtifactOfferVm,
  ChoiceVm,
  DestinationVm,
  FilesListingVm,
  KnowledgeNoteVm,
  NoteIntentVm,
  NoteTextVm,
  PanelIntentVm,
  PromoteRowVm,
  PromoteState,
  SessionPromoteVm,
  UnlistedVm,
} from "@/lib/ipc/client";
import {
  listenSessionsChanged,
  sessionsKnowledgeRead,
  sessionsKnowledgeReview,
  sessionsPromote,
  sessionsPromotePanel,
  sessionsPromoteTo,
  syncBrowse,
} from "@/lib/ipc/client";
import { syncErrorMessage } from "@/lib/stores/sync";
import { cn } from "@/lib/utils";

export interface ArchiveReview {
  /** Rust says every row has a choice, and no reread or panel write is out. */
  ready: boolean;
  /** The choices Rust kept (`SessionPromoteVm.intent.choices`), one per row. */
  choices: ChoiceVm[];
  /** The checklist's `SessionPromoteVm.revision`: Rust refuses a changed one. */
  revision: string;
}

interface Props {
  rootId: string;
  sessionId: string;
  /** Archive choices are explicit for every table row and unlisted file. */
  onArchiveReview?: (review: ArchiveReview) => void;
}

/** Counts a write in flight for the whole panel: archive waits for it. */
type Track = <T>(work: Promise<T>) => Promise<T>;

/** A change of what the person did with one note, forwarded to Rust with the next read. */
type NoteChange = Partial<Omit<NoteIntentVm, "path">>;

function StateBadge({ state }: { state: PromoteState }) {
  return (
    <Badge
      variant={state === "missingTarget" ? "destructive" : "outline"}
      className={cn(
        "whitespace-normal",
        state === "missingSource" && "text-muted-foreground",
        state === "stale" && "border-held text-held",
      )}
    >
      {
        {
          ok: "Up to date",
          stale: "Newer here",
          missingSource: "Source gone",
          missingTarget: "Target missing",
          unreadable: "Unreadable row",
          unknown: "State unknown",
        }[state]
      }
    </Badge>
  );
}

function Reason({ children }: { children: string }) {
  return (
    <p className="break-words text-muted-foreground text-xs [overflow-wrap:anywhere]">{children}</p>
  );
}

/** Browses folders beneath the Rust-offered destination; Rust composes the target. */
function VaultDestination({
  rootId,
  destination,
  busy,
  onPromote,
  onCancel,
}: {
  rootId: string;
  destination: DestinationVm;
  busy: boolean;
  onPromote: (folder: string, name: string) => void;
  onCancel: () => void;
}) {
  const id = useId();
  const box = useRef<HTMLFieldSetElement>(null);
  const [trail, setTrail] = useState([destination.folder]);
  const folder = trail[trail.length - 1];
  const [listing, setListing] = useState<FilesListingVm | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [name, setName] = useState(destination.name);
  useEffect(() => {
    box.current?.focus();
  }, []);
  useEffect(() => {
    let live = true;
    setListing(null);
    setError(null);
    void syncBrowse(rootId, folder).then(
      (next) => {
        if (live) setListing(next);
      },
      (raw: unknown) => {
        if (live) setError(syncErrorMessage(raw, "Could not read the vault folders."));
      },
    );
    return () => {
      live = false;
    };
  }, [rootId, folder]);
  return (
    <fieldset
      ref={box}
      tabIndex={-1}
      disabled={busy}
      className="min-w-0 space-y-2 rounded-md border border-border p-3 outline-none focus-visible:ring-2 focus-visible:ring-ring"
    >
      <legend className="px-1 font-medium text-sm">Choose a notes folder</legend>
      <p className="break-all font-mono text-xs">{folder}</p>
      {trail.length > 1 && (
        <Button
          type="button"
          variant="outline"
          size="sm"
          onClick={() => setTrail(trail.slice(0, -1))}
        >
          Parent folder
        </Button>
      )}
      {listing === null && error === null && <p role="status">Reading folders…</p>}
      {listing?.entries
        ?.filter((entry) => entry.kind === "folder")
        .map((entry) => (
          <Button
            key={entry.relativePath}
            type="button"
            variant="outline"
            size="sm"
            className="mr-2 max-w-full whitespace-normal break-all"
            onClick={() => setTrail([...trail, entry.relativePath])}
          >
            {entry.name}
          </Button>
        ))}
      {listing?.detail && <p role="status">{listing.detail}</p>}
      {error && (
        <p role="alert" className="text-destructive">
          {error}
        </p>
      )}
      {listing?.write.writable ? (
        <form
          onSubmit={(event) => {
            event.preventDefault();
            onPromote(folder, name);
          }}
          className="space-y-2"
        >
          <Label htmlFor={id}>Note filename</Label>
          <Input id={id} value={name} onChange={(event) => setName(event.target.value)} required />
          <Button type="submit" size="sm" disabled={busy || name.trim() === ""}>
            Promote to this folder
          </Button>
        </form>
      ) : (
        listing?.write.reason && <p role="status">{listing.write.reason}</p>
      )}
      <Button type="button" variant="ghost" size="sm" onClick={onCancel}>
        Cancel folder choice
      </Button>
    </fieldset>
  );
}

function PromotionRow({
  item,
  rootId,
  sessionId,
  archive,
  onChoose,
  onChanged,
  track,
  focusAfter,
}: {
  /** A table row, or a workspace file no row names. */
  item: PromoteRowVm | UnlistedVm;
  rootId: string;
  sessionId: string;
  archive: boolean;
  onChoose: (promote: boolean, target: string) => void;
  onChanged: () => Promise<void>;
  track: Track;
  /** Asks the panel to focus this source's row action after its next read. */
  focusAfter: (source: string) => void;
}) {
  const row = "state" in item ? item : null;
  const id = useId();
  const trigger = useRef<HTMLButtonElement>(null);
  const field = useRef<HTMLInputElement>(null);
  const [target, setTarget] = useState(row === null ? (item as UnlistedVm).suggested : row.target);
  const [editing, setEditing] = useState(false);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  useEffect(() => {
    if (editing) field.current?.focus();
  }, [editing]);
  function close() {
    setEditing(false);
    trigger.current?.focus();
  }
  async function promote() {
    if (archive) {
      onChoose(true, target);
      close();
      return;
    }
    setBusy(true);
    setError(null);
    try {
      await track(sessionsPromote(rootId, sessionId, item.source, target, row?.note ?? ""));
      focusAfter(item.source);
      setEditing(false);
      await onChanged();
    } catch (raw) {
      setError(syncErrorMessage(raw, "Could not promote this file."));
    } finally {
      setBusy(false);
    }
  }
  const unreadable = row?.state === "unreadable";
  return (
    <li
      className="min-w-0 space-y-2 border-border border-b py-3 last:border-0"
      aria-label={item.source || `Unreadable line ${row?.line}`}
    >
      {unreadable ? (
        <pre className="whitespace-pre-wrap break-all text-sm">{row?.raw}</pre>
      ) : (
        <div className="grid min-w-0 grid-cols-1 gap-1 sm:grid-cols-2 sm:gap-4">
          <p className="break-all font-mono text-xs">{item.source}</p>
          <p className="break-all font-mono text-xs">
            <span aria-hidden>→ </span>
            {row?.target || "Not promoted"}
          </p>
        </div>
      )}
      <div className="flex flex-wrap items-center gap-2">
        {row !== null ? (
          <StateBadge state={row.state} />
        ) : (
          <Badge variant="outline">Not yet promoted</Badge>
        )}
        {row?.note && (
          <span className="min-w-0 break-words text-muted-foreground text-sm [overflow-wrap:anywhere]">
            {row.note}
          </span>
        )}
        {item.refused === null && (
          <Button
            ref={trigger}
            type="button"
            size="sm"
            variant="outline"
            disabled={busy}
            aria-expanded={editing}
            data-promote-action={item.source}
            onClick={() => (editing ? close() : setEditing(true))}
          >
            {row !== null ? "Re-promote…" : "Promote…"}
          </Button>
        )}
        {archive && (
          <Button
            type="button"
            size="sm"
            variant="ghost"
            onClick={() => {
              onChoose(false, target);
              setEditing(false);
            }}
          >
            Skip this row
          </Button>
        )}
      </div>
      {row?.problem && <Reason>{row.problem}</Reason>}
      {item.refused && !unreadable && row?.state !== "missingSource" && (
        <Reason>{item.refused}</Reason>
      )}
      {editing && (
        <form
          className="space-y-2"
          onSubmit={(event) => {
            event.preventDefault();
            void promote();
          }}
        >
          <Label htmlFor={id}>Artifact target</Label>
          <Input
            ref={field}
            id={id}
            value={target}
            disabled={busy}
            onChange={(event) => {
              setTarget(event.target.value);
            }}
            required
          />
          <div className="flex flex-wrap gap-2">
            <Button type="submit" size="sm" disabled={busy}>
              {archive ? "Promote before archiving" : busy ? "Promoting…" : "Promote file"}
            </Button>
            <Button type="button" variant="ghost" size="sm" disabled={busy} onClick={close}>
              Cancel target
            </Button>
          </div>
        </form>
      )}
      {item.choice && (
        <p role="status" className="break-all text-sm">
          {item.choice.promote
            ? `Will promote to ${item.choice.target}`
            : "Skipped for this archive"}
        </p>
      )}
      {error && (
        <p role="alert" className="text-destructive text-sm">
          {error}
        </p>
      )}
    </li>
  );
}

/**
 * Reads one copy of a note through Rust, keeping only the newest answer:
 * a read that a later one superseded is discarded. Each kept read's
 * revision goes to `onRead`, for Rust to say whether it is current.
 */
function useNoteRead(
  rootId: string,
  sessionId: string,
  path: string,
  copy: boolean,
  onRead: (revision: string) => void,
) {
  const reads = useRef(0);
  const [read, setRead] = useState<NoteTextVm | null>(null);
  const [reading, setReading] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const open = useCallback(async () => {
    const seq = ++reads.current;
    setReading(true);
    setError(null);
    try {
      const next = await sessionsKnowledgeRead(rootId, sessionId, path, copy);
      if (seq === reads.current) {
        setRead(next);
        onRead(next.revision);
      }
    } catch (raw) {
      if (seq === reads.current) setError(syncErrorMessage(raw, "Could not open the note."));
    } finally {
      if (seq === reads.current) setReading(false);
    }
  }, [rootId, sessionId, path, copy, onRead]);
  const close = useCallback(() => {
    reads.current += 1;
    setRead(null);
    setReading(false);
  }, []);
  return { read, reading, error, open, close };
}

function KnowledgeCard({
  note,
  rootId,
  sessionId,
  onChange,
  onChanged,
  track,
}: {
  note: KnowledgeNoteVm;
  rootId: string;
  sessionId: string;
  /** Forwards what the person did with this note; resolves once Rust answered. */
  onChange: (path: string, change: NoteChange) => Promise<void>;
  onChanged: () => Promise<void>;
  track: Track;
}) {
  const consentId = useId();
  const reviewId = useId();
  const trigger = useRef<HTMLButtonElement>(null);
  const copyButton = useRef<HTMLButtonElement>(null);
  const heading = useRef<HTMLHeadingElement>(null);
  const focusAfterPromotion = useRef(false);
  const { path } = note;
  const readCandidate = useCallback(
    (revision: string) => void onChange(path, { read: revision, consent: null }),
    [onChange, path],
  );
  const readCopy = useCallback(
    (revision: string) => void onChange(path, { copy: revision }),
    [onChange, path],
  );
  const candidate = useNoteRead(rootId, sessionId, path, false, readCandidate);
  const copy = useNoteRead(rootId, sessionId, path, true, readCopy);
  const [picking, setPicking] = useState(false);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const body = candidate.read;
  const current = note.candidateRead === "current";
  const destination = note.destination;
  useEffect(() => {
    if (!note.consented) setPicking(false);
  }, [note.consented]);
  useEffect(() => {
    // After a promotion the trigger is gone or disabled: focus stays in this
    // card, on its notes-copy reader, or on its heading when there is none.
    if (busy || !focusAfterPromotion.current) return;
    focusAfterPromotion.current = false;
    (copyButton.current ?? heading.current)?.focus();
  }, [busy]);
  async function promote(folder: string, name: string) {
    if (!note.consented || body === null) return;
    setBusy(true);
    setError(null);
    try {
      await track(sessionsPromoteTo(rootId, sessionId, path, folder, name, body.revision));
      setPicking(false);
      copy.close();
      focusAfterPromotion.current = true;
      await onChange(path, { consent: null, copy: null });
    } catch (raw) {
      setError(syncErrorMessage(raw, "Could not promote the note."));
      await onChanged();
    } finally {
      setBusy(false);
    }
  }
  async function review(checked: boolean) {
    if (note.copyRead !== "current" || copy.read === null) return;
    setBusy(true);
    setError(null);
    try {
      await track(sessionsKnowledgeReview(rootId, sessionId, path, checked, copy.read.revision));
      await onChanged();
    } catch (raw) {
      setError(syncErrorMessage(raw, "Could not change the review."));
    } finally {
      setBusy(false);
    }
  }
  const failure = error ?? candidate.error ?? copy.error;
  return (
    <li
      aria-label={note.title ?? note.path}
      className="min-w-0 space-y-2 rounded-md border border-border p-3"
    >
      <h4 ref={heading} tabIndex={-1} className="break-words font-medium text-sm outline-none">
        {note.title ?? note.path}
      </h4>
      <p className="break-all text-muted-foreground text-xs">
        {note.agent
          ? `Written by ${note.agent}${note.host ? ` on ${note.host}` : ""}`
          : "Author not recorded"}
      </p>
      {note.state && <StateBadge state={note.state} />}
      {note.problem && <Reason>{note.problem}</Reason>}
      <div className="flex flex-wrap items-center gap-2">
        <Button
          type="button"
          size="sm"
          variant="outline"
          disabled={candidate.reading || note.problem !== null}
          onClick={() => {
            if (body === null || note.candidateRead === "stale") void candidate.open();
            else {
              candidate.close();
              void onChange(path, { read: null, consent: null });
            }
          }}
        >
          {candidate.reading
            ? "Reading note…"
            : body === null
              ? "Read whole note"
              : note.candidateRead === "stale"
                ? "Read the new version"
                : "Close note"}
        </Button>
      </div>
      {body !== null && note.candidateRead === "stale" && (
        <p role="status" className="text-held text-xs">
          This note changed since you opened it. What you read is not what would be promoted.
        </p>
      )}
      {body !== null && (
        <Textarea
          aria-label="Whole knowledge note"
          readOnly
          value={body.text}
          rows={12}
          className={cn(
            "max-h-80 min-w-0 resize-y font-mono text-xs",
            !current && "text-muted-foreground",
          )}
        />
      )}
      {note.unavailable && <Reason>{note.unavailable}</Reason>}
      {destination && (
        <div className="space-y-2">
          <div className="flex items-center gap-2">
            <Checkbox
              id={consentId}
              checked={note.consented}
              disabled={busy || !current}
              onCheckedChange={(value) =>
                void onChange(path, {
                  consent: value === true && body !== null ? body.revision : null,
                })
              }
            />
            <Label htmlFor={consentId}>I reviewed this version</Label>
          </div>
          <p className="text-muted-foreground text-xs">
            {current
              ? "Promoting saves your review in the notes copy. The session’s candidate stays unchanged."
              : "Read the whole note first: your review is of the version you read."}
          </p>
          {destination.fixed ? (
            <Button
              ref={trigger}
              type="button"
              size="sm"
              variant="outline"
              disabled={busy || !note.consented}
              onClick={() => void promote(destination.folder, destination.name)}
            >
              {note.copy?.there === false ? "Restore the notes copy" : "Promote this version again"}
            </Button>
          ) : (
            <Button
              ref={trigger}
              type="button"
              size="sm"
              variant="outline"
              disabled={busy || !note.consented}
              aria-expanded={picking}
              onClick={() => setPicking(!picking)}
            >
              Promote to notes…
            </Button>
          )}
          {picking && note.consented && !destination.fixed && (
            <VaultDestination
              rootId={rootId}
              destination={destination}
              busy={busy}
              onPromote={(folder, name) => void promote(folder, name)}
              onCancel={() => {
                setPicking(false);
                trigger.current?.focus();
              }}
            />
          )}
        </div>
      )}
      {note.copy !== null && (
        <div className="space-y-2 border-border border-t pt-2">
          <p className="break-all font-mono text-xs">
            <span className="font-sans">Notes copy: </span>
            {note.copy.path}
          </p>
          {note.copy.problem && <Reason>{note.copy.problem}</Reason>}
          {!note.copy.there ? (
            <p className="text-destructive text-xs">
              The notes copy is missing, so there is nothing to review there. Read the note and
              restore the copy.
            </p>
          ) : (
            <>
              <div className="flex flex-wrap items-center gap-2">
                <Button
                  ref={copyButton}
                  type="button"
                  size="sm"
                  variant="outline"
                  disabled={copy.reading}
                  onClick={() => {
                    if (copy.read === null || note.copyRead === "stale") void copy.open();
                    else {
                      copy.close();
                      void onChange(path, { copy: null });
                    }
                  }}
                >
                  {copy.read === null
                    ? "Read notes copy"
                    : note.copyRead === "stale"
                      ? "Read the notes copy again"
                      : "Close notes copy"}
                </Button>
                <Checkbox
                  id={reviewId}
                  checked={note.reviewedByMe}
                  disabled={busy || note.copyRead !== "current" || note.foreignCopy !== null}
                  onCheckedChange={(value) => void review(value === true)}
                />
                <Label htmlFor={reviewId}>Reviewed by me in notes</Label>
              </div>
              <p className="text-muted-foreground text-xs">
                {note.foreignCopy !== null
                  ? "This file is not the copy the note published, so no review is written into it."
                  : `${
                      note.reviewedBy
                        ? `Last review in notes: ${note.reviewedBy}. `
                        : "Nobody has reviewed the notes copy. "
                    }${
                      note.copyRead === "current"
                        ? "Your review is of the notes copy you have open."
                        : "Read the notes copy to change your review of it."
                    }`}
              </p>
              {copy.read !== null && (
                <Textarea
                  aria-label="Notes copy"
                  readOnly
                  value={copy.read.text}
                  rows={12}
                  className={cn(
                    "max-h-80 min-w-0 resize-y font-mono text-xs",
                    note.copyRead !== "current" && "text-muted-foreground",
                  )}
                />
              )}
            </>
          )}
        </div>
      )}
      {failure && (
        <p role="alert" className="text-destructive text-sm">
          {failure}
        </p>
      )}
    </li>
  );
}

function ArtifactOffer({
  offer,
  rootId,
  sessionId,
  open,
  onToggle,
  onChanged,
  track,
  focusAction,
}: {
  offer: ArtifactOfferVm;
  rootId: string;
  sessionId: string;
  open: boolean;
  onToggle: (open: boolean) => void;
  onChanged: () => Promise<void>;
  track: Track;
  focusAction: (source: string) => void;
}) {
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const destination = offer.destination;
  async function promote(folder: string, name: string) {
    setBusy(true);
    setError(null);
    try {
      await track(sessionsPromoteTo(rootId, sessionId, offer.path, folder, name, null));
      onToggle(false);
      focusAction(offer.path);
      await onChanged();
    } catch (raw) {
      setError(syncErrorMessage(raw, "Could not promote the artifact."));
    } finally {
      setBusy(false);
    }
  }
  return (
    <li aria-label={offer.path} className="space-y-2">
      <p className="break-all font-mono text-xs">{offer.path}</p>
      {offer.unavailable && <Reason>{offer.unavailable}</Reason>}
      {destination &&
        (destination.fixed ? (
          <Button
            type="button"
            variant="outline"
            size="sm"
            disabled={busy}
            data-promote-action={offer.path}
            onClick={() => void promote(destination.folder, destination.name)}
          >
            Promote again to {destination.folder}/{destination.name}
          </Button>
        ) : (
          <Button
            type="button"
            variant="outline"
            size="sm"
            aria-expanded={open}
            data-promote-action={offer.path}
            onClick={() => onToggle(!open)}
          >
            Promote artifact to notes…
          </Button>
        ))}
      {open && destination && !destination.fixed && (
        <VaultDestination
          rootId={rootId}
          destination={destination}
          busy={busy}
          onCancel={() => {
            onToggle(false);
            focusAction(offer.path);
          }}
          onPromote={(folder, name) => void promote(folder, name)}
        />
      )}
      {error && (
        <p role="alert" className="text-destructive text-sm">
          {error}
        </p>
      )}
    </li>
  );
}

export function PromotePanel({ rootId, sessionId, onArchiveReview }: Props) {
  const [vm, setVm] = useState<SessionPromoteVm | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [refreshing, setRefreshing] = useState(true);
  const [writes, setWrites] = useState(0);
  const request = useRef(0);
  const section = useRef<HTMLElement>(null);
  const focusSource = useRef<string | null>(null);
  // What the person did, as Rust last kept it plus what they did since:
  // forwarded with every read, and replaced by Rust's answer.
  const intent = useRef<PanelIntentVm>({ choices: [], notes: [] });
  const [artifact, setArtifact] = useState<string | null>(null);
  const track = useCallback<Track>(async (work) => {
    setWrites((count) => count + 1);
    try {
      return await work;
    } finally {
      setWrites((count) => count - 1);
    }
  }, []);
  const refresh = useCallback(async () => {
    const current = ++request.current;
    setRefreshing(true);
    setError(null);
    try {
      const next = await sessionsPromotePanel(rootId, sessionId, intent.current);
      if (current === request.current) {
        intent.current = next.intent;
        setVm(next);
      }
    } catch (raw) {
      if (current === request.current) {
        setVm(null);
        setError(syncErrorMessage(raw, "Could not read the promote table."));
      }
    } finally {
      if (current === request.current) setRefreshing(false);
    }
  }, [rootId, sessionId]);
  useEffect(() => {
    void refresh();
    return () => {
      request.current += 1;
    };
  }, [refresh]);
  useEffect(() => {
    let live = true;
    let unlisten: (() => void) | undefined;
    void listenSessionsChanged((root) => {
      if (root === rootId) void refresh();
    }).then((stop) => {
      if (live) unlisten = stop;
      else stop();
    });
    return () => {
      live = false;
      unlisten?.();
    };
  }, [rootId, refresh]);
  const choose = useCallback(
    (revision: string, promote: boolean, target: string) => {
      intent.current = {
        ...intent.current,
        choices: [
          ...intent.current.choices.filter((choice) => choice.revision !== revision),
          { revision, promote, target },
        ],
      };
      return refresh();
    },
    [refresh],
  );
  const changeNote = useCallback(
    (path: string, change: NoteChange) => {
      const held = intent.current.notes.find((note) => note.path === path) ?? {
        path,
        read: null,
        copy: null,
        consent: null,
      };
      intent.current = {
        ...intent.current,
        notes: [
          ...intent.current.notes.filter((note) => note.path !== path),
          { ...held, ...change },
        ],
      };
      return refresh();
    },
    [refresh],
  );
  const focusAction = useCallback((source: string) => {
    const action = [
      ...(section.current?.querySelectorAll<HTMLElement>("[data-promote-action]") ?? []),
    ].find((element) => element.dataset.promoteAction === source);
    action?.focus();
    return action !== undefined;
  }, []);
  useEffect(() => {
    const source = focusSource.current;
    if (vm !== null && source !== null && focusAction(source)) focusSource.current = null;
  }, [vm, focusAction]);
  const focusAfter = useCallback((source: string) => {
    focusSource.current = source;
  }, []);
  useEffect(() => {
    onArchiveReview?.({
      ready: vm?.complete === true && !refreshing && writes === 0 && error === null,
      choices: vm?.intent.choices ?? [],
      revision: vm?.revision ?? "",
    });
  }, [vm, error, refreshing, writes, onArchiveReview]);
  if (vm === null)
    return (
      <div className="space-y-2">
        {error ? (
          <>
            <p role="alert">{error}</p>
            <Button type="button" variant="outline" size="sm" onClick={() => void refresh()}>
              Read again
            </Button>
          </>
        ) : (
          <p role="status">Reading the promote table…</p>
        )}
      </div>
    );
  const archive = onArchiveReview !== undefined;
  function rowElement(item: PromoteRowVm | UnlistedVm) {
    return (
      <PromotionRow
        key={item.revision}
        item={item}
        rootId={rootId}
        sessionId={sessionId}
        archive={archive}
        onChoose={(promote, target) => void choose(item.revision, promote, target)}
        onChanged={refresh}
        track={track}
        focusAfter={focusAfter}
      />
    );
  }
  return (
    <section ref={section} aria-label="Promote" className="min-w-0 space-y-4 text-sm">
      {vm.label && (
        <div className="space-y-1">
          <Badge
            variant="outline"
            className="h-auto min-h-5 max-w-full whitespace-normal break-all"
          >
            {vm.label.anyone ? "Anyone" : vm.label.readers.join(", ")} · {vm.label.integrity}
            {vm.label.localOnly ? " · local only" : ""}
          </Badge>
          <p className="break-words text-muted-foreground text-xs [overflow-wrap:anywhere]">
            {vm.label.sentence}
          </p>
        </div>
      )}
      <p className="text-muted-foreground">
        The README’s Promote table is the record.{" "}
        {archive
          ? "Choose a promotion or explicitly skip each row before archiving."
          : "Keep a settled workspace file, or review a note for the drive."}
      </p>
      {archive && refreshing && (
        <p role="status" className="text-muted-foreground">
          Checking the table again…
        </p>
      )}
      {vm.problems.length > 0 && (
        <ul aria-label="What the panel could not see" className="space-y-1 text-held">
          {vm.problems.map((problem) => (
            <li key={problem} className="break-words [overflow-wrap:anywhere]">
              {problem}
            </li>
          ))}
        </ul>
      )}
      <div>
        <h3 className="font-medium">Workspace → artifacts / notes</h3>
        {!vm.hasTable && <p>No Promote table yet.</p>}
        {vm.hasTable && vm.rows.length === 0 && (
          <p className="text-muted-foreground">No recorded promotions.</p>
        )}
        <ul>{vm.rows.map(rowElement)}</ul>
      </div>
      <div>
        <h3 className="font-medium">Unlisted workspace files</h3>
        {vm.unlisted.length === 0 ? (
          <p className="text-muted-foreground">No unlisted workspace files.</p>
        ) : (
          <ul>{vm.unlisted.map(rowElement)}</ul>
        )}
      </div>
      {vm.outRefused && (
        <p role="status" className="break-words text-muted-foreground">
          {vm.outRefused}
        </p>
      )}
      {vm.vault === null && <p className="text-muted-foreground">This drive has no notes vault.</p>}
      <div className="space-y-2">
        <h3 className="font-medium">Knowledge notes</h3>
        {vm.knowledge.length === 0 ? (
          <p className="text-muted-foreground">No knowledge notes in this session.</p>
        ) : (
          <ul className="space-y-3">
            {vm.knowledge.map((note) => (
              <KnowledgeCard
                key={note.path}
                note={note}
                rootId={rootId}
                sessionId={sessionId}
                onChange={changeNote}
                onChanged={refresh}
                track={track}
              />
            ))}
          </ul>
        )}
      </div>
      {vm.artifacts.length > 0 && (
        <div className="space-y-2">
          <h3 className="font-medium">Other artifacts</h3>
          <ul className="space-y-2">
            {vm.artifacts.map((offer) => (
              <ArtifactOffer
                key={offer.path}
                offer={offer}
                rootId={rootId}
                sessionId={sessionId}
                open={artifact === offer.path}
                onToggle={(open) => setArtifact(open ? offer.path : null)}
                onChanged={refresh}
                track={track}
                focusAction={(source) => {
                  if (!focusAction(source)) focusSource.current = source;
                }}
              />
            ))}
          </ul>
        </div>
      )}
      {error && (
        <p role="alert" className="text-destructive">
          {error}
        </p>
      )}
    </section>
  );
}
