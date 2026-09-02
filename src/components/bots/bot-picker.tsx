/**
 * Which bot, and which model (Epic 61, Story 61.4).
 *
 * Two choices in one row, because they are one decision: a bot names the
 * endpoint and the profile-or-tag, and a model names what that endpoint should
 * run. Splitting them into two surfaces would let somebody pick a model no
 * chosen bot accepts.
 *
 * **The bot list is `bots_bots_list` and never an enumeration of the
 * endpoint.** keeper cannot list the profiles behind a Hermes gateway — the
 * bearer API it is allowed through has no such route — so a bot is a row
 * somebody added and verified. That is why this is a picker over keeper's own
 * rows rather than a discovery control.
 *
 * **The model list IS read from the endpoint**, per bot, and its three
 * capability flags are a tri-state. This picker prints what the endpoint said
 * and marks a capability it did not state as unknown; it never renders `null`
 * as "no".
 *
 * A models read that fails does not blank the bot choice: the bot stays
 * selected and the failure is one sentence beside the control, because a person
 * whose Ollama is restarting still has the right bot chosen.
 */
import { useEffect, useState } from "react";
import { Button } from "@/components/ui/button";
import type { BotModelVm, BotProviderVm, BotVm } from "@/lib/ipc/client";
import { botsModelsList } from "@/lib/ipc/client";
import { syncErrorMessage } from "@/lib/stores/sync";

/** The row's accessible names. */
export const BOT_PICKER_BOT_LABEL = "Bot";
export const BOT_PICKER_MODEL_LABEL = "Model";

/** What a failed model read says. Rust's own sentence comes first when it gave
 *  one; this is the fallback. */
export const BOT_PICKER_MODELS_FAILED = "keeper couldn't read what this endpoint will run.";

/** What the model control says while nothing has been read yet. */
export const BOT_PICKER_MODELS_LOADING = "Reading models…";

/** What it says when the endpoint answered with an empty roster. */
export const BOT_PICKER_NO_MODELS = "This endpoint lists no models.";

/**
 * How one model's capabilities read, given the tri-state.
 *
 * Exported because the pane's test asserts against it and because the sentence
 * is the whole point: an `unknown` capability must not read like a `false` one.
 * `null` becomes "may support"; `false` is simply not mentioned, because
 * listing everything a model cannot do is noise.
 */
export function botModelCaption(model: BotModelVm): string {
  const parts: string[] = [];
  if (model.tools === true) {
    parts.push("tools");
  } else if (model.tools === null) {
    parts.push("tools unknown");
  }
  if (model.vision === true) {
    parts.push("vision");
  } else if (model.vision === null) {
    parts.push("vision unknown");
  }
  if (model.parameterSize !== null) {
    parts.push(model.parameterSize);
  }
  return parts.join(" · ");
}

export function BotPicker({
  bots,
  providers,
  selectedBotId,
  selectedModel,
  onSelectBot,
  onSelectModel,
}: {
  bots: BotVm[];
  providers: BotProviderVm[];
  selectedBotId: string | null;
  selectedModel: string | null;
  onSelectBot: (botId: string) => void;
  onSelectModel: (model: string) => void;
}) {
  const [models, setModels] = useState<BotModelVm[] | null>(null);
  const [error, setError] = useState<string | null>(null);
  const selected = bots.find((bot) => bot.id === selectedBotId) ?? null;

  useEffect(() => {
    if (selected === null) {
      setModels(null);
      return;
    }
    // A stale-read guard rather than an abort: the read is a one-shot and the
    // only wrong outcome is an older answer landing after a newer one.
    let cancelled = false;
    setModels(null);
    setError(null);
    void botsModelsList(selected.providerId, selected.target)
      .then((read) => {
        if (!cancelled) {
          setModels(read);
        }
      })
      .catch((raw: unknown) => {
        if (!cancelled) {
          // The tree's one `IpcError`-envelope reader, rather than a fourth
          // hand-rolled structural guard: Rust composes the sentence and the
          // frontend renders it verbatim where the fact is Rust's.
          setModels([]);
          setError(syncErrorMessage(raw, BOT_PICKER_MODELS_FAILED));
        }
      });
    return () => {
      cancelled = true;
    };
  }, [selected]);

  // Choosing the first model is a **consequence** of the read, not part of it,
  // and it lives in its own effect for exactly that reason: folded into the one
  // above it would either re-trigger the request every time somebody picked a
  // model, or need a suppressed dependency list — and a suppression is a note
  // that the next reader cannot check. Here every dependency is real.
  //
  // The guard is `selectedModel !== null`, so a re-read never undoes a model
  // somebody chose.
  useEffect(() => {
    if (models === null || selectedModel !== null) {
      return;
    }
    const first = models[0];
    if (first !== undefined) {
      onSelectModel(first.id);
    }
  }, [models, selectedModel, onSelectModel]);

  return (
    <div className="flex shrink-0 flex-col gap-2 border-border border-b px-6 py-3">
      <div className="flex flex-wrap items-center gap-1">
        <span className="text-muted-foreground text-xs">{BOT_PICKER_BOT_LABEL}</span>
        {bots.map((bot) => {
          const provider = providers.find((row) => row.id === bot.providerId) ?? null;
          return (
            <Button
              key={bot.id}
              type="button"
              size="sm"
              variant={bot.id === selectedBotId ? "secondary" : "ghost"}
              aria-pressed={bot.id === selectedBotId}
              onClick={() => onSelectBot(bot.id)}
            >
              {/* The provider's name qualifies the bot, because two tenants
                  legitimately hold a bot of the same name — a work Hermes and a
                  home one — and a picker that showed only the bot name would
                  make them indistinguishable. */}
              {provider === null ? bot.name : `${bot.name} · ${provider.name}`}
            </Button>
          );
        })}
      </div>
      {selected !== null && (
        <div className="flex flex-wrap items-center gap-1">
          <span className="text-muted-foreground text-xs">{BOT_PICKER_MODEL_LABEL}</span>
          {models === null && (
            <span role="status" className="text-muted-foreground text-xs">
              {BOT_PICKER_MODELS_LOADING}
            </span>
          )}
          {models !== null && models.length === 0 && error === null && (
            <span className="text-muted-foreground text-xs">{BOT_PICKER_NO_MODELS}</span>
          )}
          {(models ?? []).map((model) => (
            <Button
              key={model.id}
              type="button"
              size="sm"
              variant={model.id === selectedModel ? "secondary" : "ghost"}
              aria-pressed={model.id === selectedModel}
              title={botModelCaption(model)}
              onClick={() => onSelectModel(model.id)}
            >
              {model.id}
            </Button>
          ))}
        </div>
      )}
      {error !== null && (
        <p role="alert" className="text-destructive text-xs">
          {error}
        </p>
      )}
    </div>
  );
}
