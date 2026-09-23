import { useEffect, useState } from "react";
import { FileControlled } from "@/components/settings/config-source-section";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import {
  type BotModelVm,
  botsModelsList,
  botsProvidersList,
  type EmbeddingModelVm,
  notesEmbeddingModelGet,
  notesEmbeddingModelSet,
  notesServiceFileNamesGet,
  notesServiceFileNamesSet,
} from "@/lib/ipc/client";
import { useCapabilitiesStore, useIsReducedCapabilityPlatform } from "@/lib/stores/capabilities";
import { useNotesSearchState } from "@/lib/stores/notes-search-state";

export const NOTES_SEARCH_SETTINGS_TITLE = "Notes search";
export const EMBEDDING_UNKNOWN = "This model has not reported whether it supports embeddings.";
const NONE = "none";
type Choice = { provider: string; providerName: string; model: BotModelVm };

export function SearchSettingsSection({ open }: { open: boolean }) {
  const [names, setNames] = useState("");
  const [savedNames, setSavedNames] = useState("");
  const [model, setModel] = useState<EmbeddingModelVm | null>(null);
  const [choices, setChoices] = useState<Choice[]>([]);
  const [loaded, setLoaded] = useState(false);
  const [saving, setSaving] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [providerErrors, setProviderErrors] = useState<string[]>([]);
  const hydrated = useCapabilitiesStore((state) => state.hydrated);
  const phone = useIsReducedCapabilityPlatform();
  const showEmbedding = hydrated && !phone;
  const states = useNotesSearchState((state) => state.byVault);
  useEffect(() => {
    if (!open) return;
    let live = true;
    setLoaded(false);
    setError(null);
    setProviderErrors([]);
    void Promise.all([notesServiceFileNamesGet(), notesEmbeddingModelGet()])
      .then(([files, value]) => {
        if (!live) return;
        setNames(files.join(", "));
        setSavedNames(files.join(", "));
        setModel(value);
        setLoaded(true);
      })
      .catch(() => {
        if (live) setError("Notes search settings could not be read.");
      });
    if (showEmbedding)
      void botsProvidersList()
        .then(async (providers) => {
          const rows = await Promise.allSettled(
            providers.map(async (provider) => {
              const models = await botsModelsList(provider.id);
              return models
                .filter((each) => each.embedding !== false)
                .map((each) => ({
                  provider: provider.id,
                  providerName: provider.name,
                  model: each,
                }));
            }),
          );
          if (live) {
            setChoices(rows.flatMap((row) => (row.status === "fulfilled" ? row.value : [])));
            setProviderErrors(
              rows.flatMap((row, index) =>
                row.status === "rejected"
                  ? [
                      `Embedding models from ${providers[index].name} could not be read. Check the provider in Settings.`,
                    ]
                  : [],
              ),
            );
          }
        })
        .catch(() => {
          if (live) setError("Embedding models could not be read. Check the provider in Settings.");
        });
    return () => {
      live = false;
    };
  }, [open, showEmbedding]);

  const saveNames = async () => {
    if (!loaded || saving || names === savedNames) return;
    setSaving(true);
    setError(null);
    const files = names
      .split(",")
      .map((name) => name.trim())
      .filter(Boolean);
    try {
      await notesServiceFileNamesSet(files);
      const saved = await notesServiceFileNamesGet();
      setNames(saved.join(", "));
      setSavedNames(saved.join(", "));
    } catch {
      setError("Service files could not be saved.");
    } finally {
      setSaving(false);
    }
  };
  const selected = model ? JSON.stringify([model.provider, model.model]) : NONE;
  const current = choices.find(
    (each) => each.provider === model?.provider && each.model.id === model?.model,
  );
  const refusals = [
    ...new Set(
      Object.values(states)
        .filter((state) => state.phase === "refused")
        .map((state) => state.sentence)
        .filter(Boolean),
    ),
  ];
  return (
    <section
      id="notes-search-settings"
      className="mt-2 flex flex-col gap-2 border-border border-t pt-3 text-sm"
    >
      <p className="font-medium">{NOTES_SEARCH_SETTINGS_TITLE}</p>
      <div className="flex flex-col gap-1.5">
        <Label htmlFor="notes-service-files">Service files</Label>
        <FileControlled settingKey="notes.service_file_names" />
        <Input
          id="notes-service-files"
          value={names}
          disabled={!loaded || saving}
          onChange={(event) => setNames(event.target.value)}
          onBlur={() => void saveNames()}
          onKeyDown={(event) => {
            if (event.key === "Enter") {
              event.preventDefault();
              void saveNames();
            }
          }}
        />
        <p className="text-muted-foreground text-sm">
          Comma-separated file names, hidden from the note list in any folder, as are space
          definitions in the spaces folder. The eye toggle shows them again; links, Files and search
          everywhere still find them.
        </p>
      </div>
      {showEmbedding && (
        <div className="flex flex-col gap-1.5">
          <Label htmlFor="notes-embedding-model">Embedding model</Label>
          <Select
            value={selected}
            disabled={!loaded || saving}
            onValueChange={(value) => {
              const choice = choices.find(
                (each) => JSON.stringify([each.provider, each.model.id]) === value,
              );
              const next =
                value === NONE
                  ? null
                  : choice
                    ? { provider: choice.provider, model: choice.model.id }
                    : undefined;
              if (next === undefined) return;
              setSaving(true);
              setError(null);
              void notesEmbeddingModelSet(next)
                .then(() => setModel(next))
                .catch(() => {
                  setError("The embedding model could not be saved.");
                })
                .finally(() => setSaving(false));
            }}
          >
            <SelectTrigger id="notes-embedding-model">
              <SelectValue placeholder="Choose an embedding model" />
            </SelectTrigger>
            <SelectContent>
              <SelectItem value={NONE}>None — search by words only</SelectItem>
              {choices.map((each) => (
                <SelectItem
                  key={JSON.stringify([each.provider, each.model.id])}
                  value={JSON.stringify([each.provider, each.model.id])}
                >
                  {each.providerName} · {each.model.id}
                  {each.model.embedding == null ? " — capability unknown" : ""}
                </SelectItem>
              ))}
            </SelectContent>
          </Select>
          {model && !current && (
            <p role="status" className="text-muted-foreground text-sm">
              Configured embedding model {model.provider} · {model.model} is unavailable.
            </p>
          )}
          {providerErrors.map((sentence) => (
            <p key={sentence} role="status" className="text-destructive text-sm">
              {sentence}
            </p>
          ))}
          {current?.model.embedding == null && current && (
            <p className="text-muted-foreground text-sm">{EMBEDDING_UNKNOWN}</p>
          )}
          {refusals.map((sentence) => (
            <p key={sentence} role="status" className="text-destructive text-sm">
              {sentence}
            </p>
          ))}
        </div>
      )}
      {error && (
        <p role="alert" className="text-destructive text-sm">
          {error}
        </p>
      )}
    </section>
  );
}
