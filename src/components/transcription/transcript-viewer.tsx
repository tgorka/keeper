import { useCallback, useEffect, useRef, useState } from "react";
import { Button } from "@/components/ui/button";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import { Input } from "@/components/ui/input";
import { useWindowedRows } from "@/components/ui/window-list";
import { TextFileViewer } from "@/components/viewers/text-file-viewer";
import {
  dictionaryAcceptSuggestion,
  type TranscriptVm,
  transcriptAssignSpeaker,
  transcriptEditUtterance,
  transcriptInsertUtterance,
  transcriptMergeSpeakers,
  transcriptRead,
  transcriptReassignUtterance,
  transcriptRenameSpeaker,
  transcriptSplitUtterance,
} from "@/lib/ipc/client";
import type { DictionarySuggestion } from "@/lib/ipc/gen/DictionarySuggestion";
import type { Speaker } from "@/lib/ipc/gen/Speaker";
import type { Utterance } from "@/lib/ipc/gen/Utterance";
import type { VoicesDriveVm } from "@/lib/ipc/gen/VoicesDriveVm";
import { syncErrorMessage } from "@/lib/stores/sync";
import {
  refreshTranscription,
  transcriptionStore,
  useTranscriptionStore,
} from "@/lib/stores/transcription";
import { FILE_FORMATS } from "@/lib/viewers/registry";
import type { ViewerProps } from "@/lib/viewers/types";

export const TRANSCRIPTION_SELECT =
  "h-9 w-full min-w-0 rounded-md border border-input bg-background px-2 text-sm focus-visible:ring-2 focus-visible:ring-ring";
const STATUS = {
  auto: "Matched",
  suggested: "Suggested",
  confirmed: "Confirmed",
  unknown: "Unknown",
  self: "You",
} as const;
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
      <DialogContent className="flex h-[85dvh] min-w-0 flex-col sm:max-w-3xl">
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
  const generation = useRef(0);
  const drives = useTranscriptionStore((s) => s.status?.voicesDrives);
  const bank = voicesDriveFor(drives, path, profileId);
  const unreadable = useRef(onUnreadable);
  unreadable.current = onUnreadable;
  useEffect(() => {
    const mine = ++generation.current;
    setVm(null);
    setError(null);
    setEditing(null);
    setPanel(null);
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
    return () => {
      generation.current += 1;
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
  const utterances = vm?.transcript.utterances;
  const getKey = useCallback(
    (index: number) => utterances?.[index]?.id ?? String(index),
    [utterances],
  );
  const list = useWindowedRows({ count: utterances?.length ?? 0, getKey, rowHeight: 116, gap: 8 });
  return (
    <section
      aria-label="Transcript viewer"
      className="flex min-h-0 min-w-0 flex-1 flex-col gap-3 text-sm"
    >
      {error && (
        <p role="alert" className="break-words text-destructive">
          {error}
        </p>
      )}
      {!vm && !error && <p role="status">Opening transcript…</p>}
      {vm && (
        <>
          <header className="min-w-0 border-b pb-2">
            <h2 className="break-words font-heading text-title">
              {vm.transcript.source.files.join(", ")}
            </h2>
            <p className="text-muted-foreground">
              {new Date(vm.transcript.createdAt).toLocaleString()} ·{" "}
              <span className="font-mono">{timestamp(vm.transcript.duration)}</span> ·{" "}
              {vm.transcript.language === "auto"
                ? "Automatic language"
                : vm.transcript.language === "en"
                  ? "English"
                  : "Polish"}
            </p>
            <p className="break-words text-muted-foreground text-xs">
              {vm.transcript.engine.asr} · {vm.transcript.engine.diarizer} ·{" "}
              {vm.transcript.engine.embedding}
            </p>
          </header>
          <details className="min-w-0" open>
            <summary className="cursor-pointer font-medium">Speakers</summary>
            <div className="max-h-64 space-y-2 overflow-auto py-2">
              {vm.transcript.speakers
                .filter((speaker) =>
                  vm.transcript.utterances.some((utterance) => utterance.speaker === speaker.id),
                )
                .map((speaker) => (
                  <SpeakerRow key={speaker.id} speaker={speaker} vm={vm} busy={busy} act={act} />
                ))}
            </div>
          </details>
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
          <div {...list.viewportProps} className="min-h-0 flex-1 overflow-auto">
            {vm.transcript.utterances.length === 0 && (
              <p className="p-3 text-muted-foreground">No speech was found in this file.</p>
            )}
            <ol aria-label="Utterances" className="relative" style={{ height: list.totalSize }}>
              {list.rows.map((row) => {
                const utterance = vm.transcript.utterances[row.index];
                if (!utterance) return null;
                return (
                  <li
                    key={row.key}
                    {...list.rowProps(row)}
                    className="min-w-0 rounded-md border p-3"
                  >
                    <div className="mb-2 flex flex-wrap items-center gap-2">
                      <span className="font-mono text-xs">{timestamp(utterance.start)}</span>
                      <select
                        aria-label={`Speaker for ${utterance.id}`}
                        className={`${TRANSCRIPTION_SELECT} max-w-48`}
                        value={utterance.speaker}
                        disabled={busy}
                        onChange={(event) =>
                          void act(() =>
                            transcriptReassignUtterance(path, utterance.id, event.target.value),
                          )
                        }
                      >
                        {vm.transcript.speakers.map((s) => (
                          <option key={s.id} value={s.id}>
                            {speakerName(s)}
                          </option>
                        ))}
                      </select>
                      {utterance.edited && utterance.asrText ? (
                        <details className="text-muted-foreground text-xs">
                          <summary>Edited · Recognised text</summary>
                          <p className="whitespace-pre-wrap break-words">{utterance.asrText}</p>
                        </details>
                      ) : (
                        utterance.edited && (
                          <span className="text-muted-foreground text-xs">Added by hand</span>
                        )
                      )}
                      <div className="ml-auto flex flex-wrap gap-1">
                        {utterance.words.length > 1 && (
                          <Button
                            size="sm"
                            variant="ghost"
                            aria-label={`Split… ${utterance.id}`}
                            aria-expanded={panel?.id === utterance.id && panel.kind === "split"}
                            disabled={busy}
                            onClick={() =>
                              setPanel((open) =>
                                open?.id === utterance.id && open.kind === "split"
                                  ? null
                                  : { id: utterance.id, kind: "split" },
                              )
                            }
                          >
                            Split…
                          </Button>
                        )}
                        <Button
                          size="sm"
                          variant="ghost"
                          aria-label={`Add a line after ${utterance.id}`}
                          aria-expanded={panel?.id === utterance.id && panel.kind === "insert"}
                          disabled={busy}
                          onClick={() =>
                            setPanel((open) =>
                              open?.id === utterance.id && open.kind === "insert"
                                ? null
                                : { id: utterance.id, kind: "insert" },
                            )
                          }
                        >
                          Add a line after
                        </Button>
                      </div>
                    </div>
                    {editing === utterance.id ? (
                      <div className="space-y-2">
                        <textarea
                          aria-label={`Edit ${utterance.id}`}
                          className="min-h-20 w-full rounded-md border bg-background p-2"
                          value={draft}
                          disabled={busy}
                          onChange={(event) => setDraft(event.target.value)}
                          onKeyDown={(event) => {
                            if (event.key === "Escape") {
                              event.preventDefault();
                              setEditing(null);
                            } else if (
                              event.key === "Enter" &&
                              !event.shiftKey &&
                              !event.nativeEvent.isComposing
                            ) {
                              event.preventDefault();
                              void save(utterance.id);
                            }
                          }}
                        />
                        <div className="flex gap-2">
                          <Button size="sm" disabled={busy} onClick={() => void save(utterance.id)}>
                            Save text
                          </Button>
                          <Button
                            size="sm"
                            variant="ghost"
                            disabled={busy}
                            onClick={() => setEditing(null)}
                          >
                            Cancel edit
                          </Button>
                        </div>
                      </div>
                    ) : (
                      <button
                        type="button"
                        className="w-full whitespace-pre-wrap break-words text-left focus-visible:outline focus-visible:outline-ring"
                        aria-label={`Edit ${utterance.id}: ${utterance.text}`}
                        disabled={busy}
                        onClick={() => {
                          setEditing(utterance.id);
                          setDraft(utterance.text);
                        }}
                      >
                        {utterance.text}
                      </button>
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
                        speakers={vm.transcript.speakers}
                        busy={busy}
                        onInsert={(speakerId, text) =>
                          void changeLines(() =>
                            transcriptInsertUtterance(path, utterance.id, speakerId, text),
                          )
                        }
                        onCancel={() => setPanel(null)}
                      />
                    )}
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
function SpeakerRow({
  speaker,
  vm,
  busy,
  act,
}: {
  speaker: Speaker;
  vm: TranscriptVm;
  busy: boolean;
  act: (operation: () => Promise<TranscriptVm>) => Promise<void>;
}) {
  const [mode, setMode] = useState<"rename" | "new" | null>(null);
  const [name, setName] = useState("");
  const candidates = new Set(speaker.candidates.map((c) => c.personId));
  const people = [
    ...vm.people.filter((p) => candidates.has(p.id)),
    ...vm.people.filter((p) => !candidates.has(p.id)),
  ];
  return (
    <div className="min-w-0 space-y-2 rounded-md border p-2">
      <div className="flex flex-wrap items-center gap-2">
        <strong className="break-words">{speakerName(speaker)}</strong>
        <span className="rounded border px-1.5 py-0.5 text-xs">{STATUS[speaker.status]}</span>
        {speaker.score !== null && (
          <span className="font-mono text-xs">{speaker.score.toFixed(2)}</span>
        )}
      </div>
      {speaker.candidates.length > 0 && (
        <p className="break-words text-muted-foreground text-xs">
          Candidates:{" "}
          {speaker.candidates.map((c) => `${c.name} (${c.score.toFixed(2)})`).join(", ")}
        </p>
      )}
      <div className="flex flex-wrap gap-2">
        <select
          aria-label={`This is… ${speaker.id}`}
          className={`${TRANSCRIPTION_SELECT} flex-1 basis-36`}
          disabled={busy}
          value=""
          onChange={(event) => {
            if (event.target.value === "new") {
              setMode("new");
              setName("");
            } else
              void act(() =>
                transcriptAssignSpeaker(vm.path, speaker.id, event.target.value, null),
              );
          }}
        >
          <option value="" disabled>
            This is…
          </option>
          {people.map((p) => (
            <option key={p.id} value={p.id}>
              {p.name}
              {candidates.has(p.id) ? " · suggested" : ""}
            </option>
          ))}
          <option value="new">New person…</option>
        </select>
        <Button
          variant="outline"
          size="sm"
          disabled={busy}
          onClick={() => {
            setMode("rename");
            setName(speakerName(speaker));
          }}
        >
          Rename label
        </Button>
        {vm.transcript.speakers.length > 1 && (
          <select
            aria-label={`Merge ${speaker.id} into`}
            className={`${TRANSCRIPTION_SELECT} flex-1 basis-36`}
            disabled={busy}
            value=""
            onChange={(event) =>
              void act(() => transcriptMergeSpeakers(vm.path, speaker.id, event.target.value))
            }
          >
            <option value="" disabled>
              Merge into…
            </option>
            {vm.transcript.speakers
              .filter((s) => s.id !== speaker.id)
              .map((s) => (
                <option key={s.id} value={s.id}>
                  {speakerName(s)}
                </option>
              ))}
          </select>
        )}
      </div>
      {mode && (
        <form
          className="flex flex-wrap gap-2"
          onSubmit={(event) => {
            event.preventDefault();
            void act(async () => {
              const result =
                mode === "new"
                  ? await transcriptAssignSpeaker(vm.path, speaker.id, null, name)
                  : await transcriptRenameSpeaker(vm.path, speaker.id, name);
              setMode(null);
              return result;
            });
          }}
        >
          <Input
            aria-label={mode === "new" ? "New person name" : "Speaker label"}
            className="min-w-0 flex-1 basis-36"
            value={name}
            onChange={(event) => setName(event.target.value)}
          />
          <Button type="submit" size="sm" disabled={busy || (mode === "new" && !name.trim())}>
            {mode === "new" ? "Create person" : "Save label"}
          </Button>
          <Button type="button" size="sm" variant="ghost" onClick={() => setMode(null)}>
            Cancel
          </Button>
        </form>
      )}
    </div>
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
      <select
        aria-label={`Speaker for the line after ${utterance.id}`}
        className={`${TRANSCRIPTION_SELECT} max-w-48`}
        value={speakerId}
        disabled={busy}
        onChange={(event) => setSpeakerId(event.target.value)}
      >
        {speakers.map((s) => (
          <option key={s.id} value={s.id}>
            {speakerName(s)}
          </option>
        ))}
      </select>
      <Input
        aria-label={`Text of the line after ${utterance.id}`}
        className="min-w-0 flex-1 basis-48"
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
