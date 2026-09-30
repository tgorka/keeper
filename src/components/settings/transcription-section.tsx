import { useEffect, useId, useState } from "react";
import {
  TRANSCRIPTION_SELECT,
  TranscriptDialog,
} from "@/components/transcription/transcript-viewer";
import { TranscriptionJob } from "@/components/transcription/transcription-job";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { Switch } from "@/components/ui/switch";
import {
  type DictionaryTermVm,
  dictionaryTermDelete,
  dictionaryTermSave,
  type ModelChoiceVm,
  type PersonVm,
  type TranscriptionLanguage,
  type TranscriptionModelsVm,
  transcriptionModelsAvailable,
  transcriptionModelsFetch,
  voicesPeopleMerge,
  voicesPersonDelete,
  voicesPersonRename,
  voicesPersonSetSelf,
} from "@/lib/ipc/client";
import { syncErrorMessage } from "@/lib/stores/sync";
import {
  pickFileToTranscribe,
  refreshTranscription,
  refreshVoicesDrive,
  saveTranscriptionSettings,
  startTranscription,
  transcriptionStore,
  useTranscriptionStore,
} from "@/lib/stores/transcription";

export function AfterRecordingSwitch() {
  const id = useId();
  const status = useTranscriptionStore((s) => s.status);
  const saving = useTranscriptionStore((s) => s.saving);
  const error = useTranscriptionStore((s) => s.error);
  useEffect(() => {
    if (!transcriptionStore.getState().status) void refreshTranscription();
  }, []);
  return (
    <div className="space-y-2">
      <div className="flex items-center justify-between gap-2">
        <Label htmlFor={id}>Transcribe after recording</Label>
        <Switch
          id={id}
          checked={status?.afterRecording ?? false}
          disabled={!status || saving}
          onCheckedChange={(afterRecording) => void saveTranscriptionSettings({ afterRecording })}
        />
      </div>
      <p className="text-muted-foreground text-xs">
        Applies to sessions saved to a drive that keeps voices, when the models are ready.
      </p>
      {error && (
        <p role="alert" className="text-destructive">
          {error}
        </p>
      )}
    </div>
  );
}
export function TranscriptionSection({ open }: { open: boolean }) {
  return open ? <TranscriptionSettings /> : null;
}
function TranscriptionSettings() {
  const id = useId();
  const status = useTranscriptionStore((s) => s.status);
  const saving = useTranscriptionStore((s) => s.saving);
  const readError = useTranscriptionStore((s) => s.error);
  const [error, setError] = useState<string | null>(null);
  const [fetching, setFetching] = useState(false);
  const [path, setPath] = useState<string | null>(null);
  const [view, setView] = useState<string | null>(null);
  const [models, setModels] = useState<TranscriptionModelsVm | null>(null);
  useEffect(() => {
    void refreshTranscription();
  }, []);
  useEffect(() => {
    if (status?.models.state !== "fetching") return;
    const timer = window.setInterval(() => void refreshTranscription(), 1500);
    return () => window.clearInterval(timer);
  }, [status?.models.state]);
  // A fetch that lands changes which model folders are here.
  const modelsState = status?.models.state;
  useEffect(() => {
    if (modelsState === undefined || modelsState === "fetching") return;
    let live = true;
    void transcriptionModelsAvailable()
      .then((next) => live && setModels(next))
      .catch((cause: unknown) => live && setError(syncErrorMessage(cause)));
    return () => {
      live = false;
    };
  }, [modelsState]);
  const pick = async () => {
    try {
      const picked = await pickFileToTranscribe();
      if (picked) {
        setPath(picked);
        await startTranscription(picked);
      }
    } catch (cause) {
      setError(syncErrorMessage(cause));
    }
  };
  return (
    <section
      id="settings-transcription"
      aria-labelledby={`${id}-title`}
      className="flex min-w-0 flex-col gap-3 border-t pt-4 text-sm"
    >
      <h3 id={`${id}-title`} className="font-medium">
        Transcription
      </h3>
      <p className="text-muted-foreground">Transcribe and recognise speakers on this Mac.</p>
      {!status && !readError && <p role="status">Reading transcription settings…</p>}
      {!status && readError && (
        <div className="space-y-2">
          <p role="alert" className="text-destructive">
            {readError}
          </p>
          <Button size="sm" variant="outline" onClick={() => void refreshTranscription()}>
            Try again
          </Button>
        </div>
      )}
      {error && (
        <p role="alert" className="text-destructive">
          {error}
        </p>
      )}
      {status && (
        <>
          <p role="status">{status.models.sentence}</p>
          {status.reason && <p>{status.reason}</p>}
          {status.models.state === "noAccount" && (
            <p className="text-muted-foreground">
              Connect your account in Settings → Account to fetch the models.
            </p>
          )}
          {(status.models.state === "missing" || status.models.state === "failed") && (
            <Button
              className="self-start"
              variant="outline"
              size="sm"
              disabled={fetching}
              onClick={() => {
                setFetching(true);
                setError(null);
                void transcriptionModelsFetch()
                  .then((next) => transcriptionStore.setState({ status: next }))
                  .catch((cause: unknown) => setError(syncErrorMessage(cause)))
                  .finally(() => setFetching(false));
              }}
            >
              {fetching ? "Fetching models…" : "Fetch models"}
            </Button>
          )}
          {status.models.missing.length > 0 && (
            <details>
              <summary className="cursor-pointer text-muted-foreground">
                Missing model files
              </summary>
              <ul className="space-y-1 py-2 font-mono text-xs">
                {status.models.missing.map((file) => (
                  <li key={file} className="break-all">
                    {file}
                  </li>
                ))}
              </ul>
            </details>
          )}
          <Label htmlFor={`${id}-language`}>Language</Label>
          <select
            id={`${id}-language`}
            className={TRANSCRIPTION_SELECT}
            disabled={saving}
            value={status.language}
            onChange={(event) =>
              void saveTranscriptionSettings({
                language: event.target.value as TranscriptionLanguage,
              })
            }
          >
            <option value="auto">Automatic</option>
            <option value="en">English</option>
            <option value="pl">Polish</option>
          </select>
          <AfterRecordingSwitch />
          {models && (
            <>
              <ModelSelect
                id={`${id}-asr-model`}
                label="Speech model"
                value={status.asrModel}
                fallback={models.defaults.asr}
                choices={models.asr}
                disabled={saving}
                onChange={(asrModel) => void saveTranscriptionSettings({ asrModel })}
              />
              <ModelSelect
                id={`${id}-diarization-model`}
                label="Speaker model"
                value={status.diarizationModel}
                fallback={models.defaults.diarization}
                choices={models.diarization}
                disabled={saving}
                onChange={(diarizationModel) =>
                  void saveTranscriptionSettings({ diarizationModel })
                }
              />
            </>
          )}
          {status.voicesDrives.length === 0 && (
            <p className="text-muted-foreground">
              Choose a folder that keeps voices in Settings → Sync to keep people and a dictionary.
            </p>
          )}
          {status.voicesDrives.map((drive) => (
            <VoicesDrive
              key={drive.profileId}
              profileId={drive.profileId}
              name={drive.name}
              subfolder={drive.subfolder}
            />
          ))}
          <Button className="self-start" variant="outline" size="sm" onClick={() => void pick()}>
            Transcribe a file…
          </Button>
          {path && <TranscriptionJob path={path} onOpen={setView} />}
        </>
      )}
      <TranscriptDialog
        path={view}
        onClose={() => {
          setView(null);
          // A speaker assigned in there may have added or merged a person.
          for (const drive of status?.voicesDrives ?? [])
            void refreshVoicesDrive(drive.profileId).catch((cause: unknown) =>
              setError(syncErrorMessage(cause)),
            );
        }}
      />
    </section>
  );
}
/**
 * One role's model: the config repository's choice first, then every model
 * folder on this Mac for that role. An incomplete one is shown but cannot be
 * picked; a pick that is no longer here stays visible, so the refusal the
 * status line gives has something to point at.
 */
function ModelSelect({
  id,
  label,
  value,
  fallback,
  choices,
  disabled,
  onChange,
}: {
  id: string;
  label: string;
  value: string;
  fallback: string;
  choices: ModelChoiceVm[];
  disabled: boolean;
  onChange: (id: string) => void;
}) {
  const gone = value !== "" && !choices.some((choice) => choice.id === value);
  return (
    <>
      <Label htmlFor={id}>{label}</Label>
      <select
        id={id}
        className={TRANSCRIPTION_SELECT}
        disabled={disabled}
        value={value}
        onChange={(event) => onChange(event.target.value)}
      >
        <option value="">From the config repository ({fallback})</option>
        {choices.map((choice) => (
          <option key={choice.id} value={choice.id} disabled={!choice.complete}>
            {choice.complete ? choice.id : `${choice.id} (incomplete)`}
          </option>
        ))}
        {gone && (
          <option value={value} disabled>
            {value} (not on this Mac)
          </option>
        )}
      </select>
    </>
  );
}
function VoicesDrive({
  profileId,
  name,
  subfolder,
}: {
  profileId: string;
  name: string;
  subfolder: string;
}) {
  const people = useTranscriptionStore((s) => s.people[profileId]);
  const terms = useTranscriptionStore((s) => s.dictionary[profileId]);
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const [term, setTerm] = useState<DictionaryTermVm | "new" | null>(null);
  const [text, setText] = useState("");
  const [aliases, setAliases] = useState("");
  useEffect(() => {
    let live = true;
    void refreshVoicesDrive(profileId).catch((cause: unknown) => {
      if (live) setError(syncErrorMessage(cause));
    });
    return () => {
      live = false;
    };
  }, [profileId]);
  const run = async (operation: () => Promise<void>) => {
    if (busy) return false;
    setBusy(true);
    setError(null);
    try {
      await operation();
      return true;
    } catch (cause) {
      setError(syncErrorMessage(cause));
      return false;
    } finally {
      setBusy(false);
    }
  };
  const putPeople = async (operation: Promise<PersonVm[]>) => {
    const next = await operation;
    transcriptionStore.setState((s) => ({ people: { ...s.people, [profileId]: next } }));
  };
  const putTerms = async (operation: Promise<DictionaryTermVm[]>) => {
    const next = await operation;
    transcriptionStore.setState((s) => ({ dictionary: { ...s.dictionary, [profileId]: next } }));
  };
  return (
    <div className="min-w-0 space-y-3 rounded-md border p-3">
      <h4 className="break-words font-medium">{name}</h4>
      <p className="break-words font-mono text-muted-foreground text-xs">{subfolder}</p>
      {error && (
        <p role="alert" className="text-destructive">
          {error}
        </p>
      )}
      <h5 className="font-medium">People</h5>
      {!people && <p role="status">Reading people…</p>}
      {people?.length === 0 && (
        <p className="text-muted-foreground">Assign a speaker in a transcript to add a person.</p>
      )}
      {people?.map((person) => (
        <PersonRow
          key={person.id}
          person={person}
          people={people}
          busy={busy}
          rename={(name) => run(() => putPeople(voicesPersonRename(profileId, person.id, name)))}
          markSelf={() => run(() => putPeople(voicesPersonSetSelf(profileId, person.id)))}
          merge={(into) => run(() => putPeople(voicesPeopleMerge(profileId, person.id, into)))}
          remove={() => run(() => putPeople(voicesPersonDelete(profileId, person.id)))}
        />
      ))}
      <h5 className="font-medium">Dictionary</h5>
      {!terms && <p role="status">Reading dictionary…</p>}
      {terms?.length === 0 && (
        <p className="text-muted-foreground">
          Add names and jargon with the spellings recognition should replace.
        </p>
      )}
      {terms?.map((item) => (
        <div key={item.id} className="flex min-w-0 flex-wrap items-center gap-2">
          <div className="min-w-0 flex-1 basis-32">
            <strong className="break-words">{item.text}</strong>
            <p className="break-words text-muted-foreground text-xs">{item.aliases.join(", ")}</p>
          </div>
          <Button
            size="sm"
            variant="outline"
            disabled={busy}
            onClick={() => {
              setTerm(item);
              setText(item.text);
              setAliases(item.aliases.join(", "));
            }}
          >
            Edit term
          </Button>
          <Button
            size="sm"
            variant="ghost"
            disabled={busy}
            onClick={() => void run(() => putTerms(dictionaryTermDelete(profileId, item.id)))}
          >
            Delete term
          </Button>
        </div>
      ))}
      <Button
        size="sm"
        variant="outline"
        disabled={busy}
        onClick={() => {
          setTerm("new");
          setText("");
          setAliases("");
        }}
      >
        Add term
      </Button>
      {term && (
        <form
          className="space-y-2"
          onSubmit={(event) => {
            event.preventDefault();
            void run(async () => {
              await putTerms(
                dictionaryTermSave(
                  profileId,
                  term === "new" ? null : term.id,
                  text,
                  aliases
                    .split(",")
                    .map((a) => a.trim())
                    .filter(Boolean),
                ),
              );
              setTerm(null);
            });
          }}
        >
          <Label>
            Term
            <Input
              aria-label="Dictionary term"
              value={text}
              onChange={(event) => setText(event.target.value)}
            />
          </Label>
          <Label>
            Aliases, separated by commas
            <Input
              aria-label="Dictionary aliases"
              value={aliases}
              onChange={(event) => setAliases(event.target.value)}
            />
          </Label>
          <div className="flex gap-2">
            <Button type="submit" size="sm" disabled={busy || !text.trim()}>
              Save term
            </Button>
            <Button type="button" size="sm" variant="ghost" onClick={() => setTerm(null)}>
              Cancel
            </Button>
          </div>
        </form>
      )}
    </div>
  );
}
function PersonRow({
  person,
  people,
  busy,
  rename,
  markSelf,
  merge,
  remove,
}: {
  person: PersonVm;
  people: PersonVm[];
  busy: boolean;
  rename: (name: string) => Promise<boolean>;
  markSelf: () => Promise<boolean>;
  merge: (id: string) => Promise<boolean>;
  remove: () => Promise<boolean>;
}) {
  const [editing, setEditing] = useState(false);
  const [name, setName] = useState(person.name);
  const [confirm, setConfirm] = useState(false);
  return (
    <div className="space-y-2 border-b pb-3">
      <p className="break-words">
        <strong>{person.name}</strong>
        {person.isSelf && <span className="ml-2 rounded border px-1 text-xs">You</span>} ·{" "}
        {person.samples} samples
      </p>
      {!person.hasEmbeddingForModel && (
        <p className="text-muted-foreground text-xs">
          No sample for the current model yet. Kept clips will be processed before matching.
        </p>
      )}
      <div className="flex flex-wrap gap-2">
        <Button
          size="sm"
          variant="outline"
          disabled={busy}
          onClick={() => {
            setName(person.name);
            setEditing(true);
          }}
        >
          Rename person
        </Button>
        {!person.isSelf && (
          <Button size="sm" variant="outline" disabled={busy} onClick={() => void markSelf()}>
            Mark as me
          </Button>
        )}
        {people.length > 1 && (
          <select
            aria-label={`Merge ${person.name} into`}
            className={`${TRANSCRIPTION_SELECT} flex-1 basis-32`}
            disabled={busy}
            value=""
            onChange={(event) => void merge(event.target.value)}
          >
            <option value="" disabled>
              Merge into…
            </option>
            {people
              .filter((p) => p.id !== person.id)
              .map((p) => (
                <option key={p.id} value={p.id}>
                  {p.name}
                </option>
              ))}
          </select>
        )}
        <Button size="sm" variant="ghost" disabled={busy} onClick={() => setConfirm(true)}>
          Delete person
        </Button>
      </div>
      {editing && (
        <form
          className="flex flex-wrap gap-2"
          onSubmit={(event) => {
            event.preventDefault();
            void rename(name).then((saved) => {
              if (saved) setEditing(false);
            });
          }}
        >
          <Input
            aria-label="Person name"
            className="min-w-0 flex-1 basis-32"
            value={name}
            onChange={(event) => setName(event.target.value)}
          />
          <Button type="submit" size="sm" disabled={busy || !name.trim()}>
            Save name
          </Button>
          <Button type="button" size="sm" variant="ghost" onClick={() => setEditing(false)}>
            Cancel
          </Button>
        </form>
      )}
      {confirm && (
        <div role="alert" className="space-y-2 rounded-md border border-destructive p-2">
          <p>Delete {person.name}? This removes their voice clips on every device.</p>
          <div className="flex gap-2">
            <Button size="sm" variant="destructive" disabled={busy} onClick={() => void remove()}>
              Confirm delete
            </Button>
            <Button size="sm" variant="outline" onClick={() => setConfirm(false)}>
              Keep person
            </Button>
          </div>
        </div>
      )}
    </div>
  );
}
