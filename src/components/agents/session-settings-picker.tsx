import { Brain, ChevronDown, Info, ShieldAlert, Star } from "lucide-react";
import { type ReactNode, useRef, useState } from "react";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import {
  Command,
  CommandEmpty,
  CommandGroup,
  CommandInput,
  CommandItem,
  CommandList,
} from "@/components/ui/command";
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuLabel,
  DropdownMenuRadioGroup,
  DropdownMenuRadioItem,
  DropdownMenuTrigger,
} from "@/components/ui/dropdown-menu";
import { Popover, PopoverContent, PopoverTrigger } from "@/components/ui/popover";
import { HoverHint } from "@/components/ui/tooltip";
import {
  type AgentModelOptionVm,
  type AgentSessionFooterVm,
  type AgentSessionPickerVm,
  type AgentSessionRef,
  type AgentSettingsChange,
  agentSessionSettingsChange,
} from "@/lib/ipc/client";
import { rememberAgentPick } from "@/lib/stores/agent-picks";
import { toggleModelFavorite, useModelFavorites } from "@/lib/stores/model-favorites";
import { syncErrorMessage } from "@/lib/stores/sync";
import { cn } from "@/lib/utils";

/** A chip in the composer's bar that opens nothing: the agent offers no alternative. */
function ReadOnlyChip({
  name,
  value,
  hint,
  icon,
}: {
  name: string;
  value: string;
  hint: string;
  icon?: ReactNode;
}) {
  return (
    <HoverHint label={name} detail={hint}>
      <span
        data-slot="agent-readonly-chip"
        className="inline-flex min-h-11 max-w-full items-center gap-1 rounded-md px-2 text-muted-foreground text-xs [overflow-wrap:anywhere]"
      >
        {icon}
        <span className="sr-only">{name}: </span>
        {value}
      </span>
    </HoverHint>
  );
}

/**
 * The composer bar's model and effort controls, t3.chat-style (109.18's pickers; owner ruling
 * 2026-10-27): always drawn. Each is the interactive picker where Rust says there is a choice
 * (`footer.choosesModel` / `choosesEffort`), else a read-only chip naming what the next answer
 * uses. Every decision — what is offered, which is current, which is a default, the labels, the
 * provider groups, what a running turn, a refusal or a failed model means — is Rust's. Only the
 * search text, the stars (a lens, in a cookie) and the order they give are this component's; what
 * a change wrote is remembered as this device's last pick for `agent` (Q109-16 b). The model
 * popover may be held by the view (`modelsOpen`), so the list's right-click "Model…" opens this
 * same picker.
 *
 * It renders as `display: contents` inside the composer's wrapping bar: the controls sit in the
 * bar's row, the sentences on full-width lines after it.
 */
export function SessionSettingsPicker({
  target,
  agent,
  footer,
  picker,
  untrusted,
  modelsOpen: heldOpen,
  onModelsOpenChange,
}: {
  target: AgentSessionRef;
  agent: string;
  footer: AgentSessionFooterVm;
  /** Absent where this person may not change them, or while they are still arriving. */
  picker?: AgentSessionPickerVm;
  /** Rust's sentence while the session is untrusted: a marker in the bar says it. */
  untrusted?: string;
  modelsOpen?: boolean;
  onModelsOpenChange?: (open: boolean) => void;
}) {
  const favorites = useModelFavorites();
  const [ownOpen, setOwnOpen] = useState(false);
  const modelsOpen = heldOpen ?? ownOpen;
  const setModelsOpen = onModelsOpenChange ?? setOwnOpen;
  const [notice, setNotice] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  const writing = useRef(false);
  const modelButton = useRef<HTMLButtonElement>(null);
  const effortButton = useRef<HTMLButtonElement>(null);
  const group = useRef<HTMLFieldSetElement>(null);

  async function apply(change: AgentSettingsChange) {
    // A second click while one write is in flight is ignored, never queued.
    if (writing.current) return;
    writing.current = true;
    setError(null);
    setNotice(null);
    try {
      const changed = await agentSessionSettingsChange(target, change);
      rememberAgentPick(agent, changed.pick);
      setNotice(changed.note);
    } catch (raw) {
      setError(syncErrorMessage(raw, "keeper could not change the model or effort."));
    } finally {
      writing.current = false;
    }
  }

  const models = picker?.models ?? [];
  const starred = models.filter((model) => favorites.includes(model.id));
  const rest = models.filter((model) => !favorites.includes(model.id));
  // Rust's provider groups, in the order the list first names them (t3.chat).
  const providers = [...new Set(rest.map((model) => model.provider))];
  const star = (id: string, keyboard: boolean) => {
    toggleModelFavorite(id);
    // Starring moves the row between groups, which mounts a new button: a keyboard user's
    // focus follows it rather than falling to the page.
    if (!keyboard) return;
    requestAnimationFrame(() => {
      for (const button of document.querySelectorAll<HTMLButtonElement>("[data-star]")) {
        if (button.dataset.star === id) button.focus();
      }
    });
  };
  const row = (model: AgentModelOptionVm) => (
    <CommandItem
      key={model.id}
      value={model.id}
      data-checked={model.current}
      // The option's own name: without it the star button's label is read into every row.
      aria-label={[model.id, model.current && "in use", model.default && "default"]
        .filter(Boolean)
        .join(", ")}
      onSelect={() => {
        setModelsOpen(false);
        void apply({ kind: "model", model: model.id });
      }}
      className="min-h-11"
    >
      <button
        type="button"
        aria-label={
          favorites.includes(model.id) ? `Unstar ${model.id}` : `Star ${model.id} as a favourite`
        }
        aria-pressed={favorites.includes(model.id)}
        data-star={model.id}
        className="pointer-events-auto rounded-sm p-1 hover:bg-secondary"
        onPointerDown={(event) => event.preventDefault()}
        onClick={(event) => {
          event.stopPropagation();
          star(model.id, false);
        }}
        // Enter and Space belong to the star: left to bubble, cmdk takes Enter as choosing the
        // highlighted row, which writes a model the person never pointed at (R10918-01).
        onKeyDown={(event) => {
          if (event.key !== "Enter" && event.key !== " ") return;
          event.preventDefault();
          event.stopPropagation();
          star(model.id, true);
        }}
      >
        <Star
          aria-hidden="true"
          className={cn("size-4", favorites.includes(model.id) && "fill-current")}
        />
      </button>
      <span className="min-w-0 flex-1 break-all">{model.id}</span>
      {model.default && <Badge variant="outline">Default</Badge>}
    </CommandItem>
  );
  const write = picker?.write;
  // A refusal written here before this mount still stands until a new change sets it aside.
  const alert = error ?? (write?.state === "refused" ? write.sentence : null);
  const notes: ReactNode[] = [];
  if (picker?.failed) {
    notes.push(
      <p
        key="failed"
        role="alert"
        className="flex items-start gap-1 text-held text-xs [overflow-wrap:anywhere]"
      >
        <Info aria-hidden="true" className="mt-0.5 size-3.5 shrink-0" />
        <span>{picker.failed}</span>
      </p>,
    );
  }
  if (picker?.fallback) {
    notes.push(
      <p
        key="fallback"
        role="status"
        className="flex items-start gap-1 text-held text-xs [overflow-wrap:anywhere]"
      >
        <Info aria-hidden="true" className="mt-0.5 size-3.5 shrink-0" />
        <span>{picker.fallback}</span>
      </p>,
    );
  }
  if (notice && picker?.running) {
    notes.push(
      <p
        key="notice"
        role="status"
        className="text-muted-foreground text-xs [overflow-wrap:anywhere]"
      >
        {notice}
      </p>,
    );
  }
  if (alert) {
    notes.push(
      <p key="alert" role="alert" className="text-destructive text-xs [overflow-wrap:anywhere]">
        {alert}
      </p>,
    );
  }
  return (
    <div data-slot="agent-session-settings" className="contents">
      <fieldset
        ref={group}
        tabIndex={-1}
        aria-label="Model and effort"
        className="flex min-w-0 max-w-full flex-wrap items-center gap-0.5 text-xs"
      >
        {picker && footer.choosesModel ? (
          <Popover open={modelsOpen} onOpenChange={setModelsOpen}>
            <PopoverTrigger asChild>
              <Button
                ref={modelButton}
                variant="ghost"
                size="sm"
                className="h-auto min-h-11 max-w-full justify-start whitespace-normal break-all"
                aria-label={`Model: ${footer.model}`}
              >
                <span className="min-w-0">{footer.model}</span>
                <ChevronDown aria-hidden="true" />
              </Button>
            </PopoverTrigger>
            <PopoverContent
              align="start"
              aria-label="Choose a model"
              className="w-[min(22rem,calc(100vw-1.5rem))] p-0"
            >
              <Command label="Search models">
                <CommandInput placeholder="Search models…" />
                <CommandList label="Offered models">
                  <CommandEmpty>No offered model matches.</CommandEmpty>
                  {starred.length > 0 && (
                    <CommandGroup heading="Favourites">{starred.map(row)}</CommandGroup>
                  )}
                  {providers.map((provider) => (
                    <CommandGroup key={provider} heading={provider}>
                      {rest.filter((model) => model.provider === provider).map(row)}
                    </CommandGroup>
                  ))}
                </CommandList>
                {picker.running && (
                  <p className="border-t px-3 py-2 text-muted-foreground text-xs">
                    {picker.running}
                  </p>
                )}
              </Command>
            </PopoverContent>
          </Popover>
        ) : (
          <ReadOnlyChip
            name="Model"
            value={footer.model}
            hint="The next answer uses this model; there is no other to choose here."
          />
        )}
        {picker && footer.choosesEffort ? (
          <DropdownMenu>
            <DropdownMenuTrigger asChild>
              <Button
                ref={effortButton}
                variant="ghost"
                size="sm"
                className="min-h-11"
                aria-label={`Effort: ${footer.effort}`}
              >
                <Brain aria-hidden="true" />
                {footer.effort}
                <ChevronDown aria-hidden="true" />
              </Button>
            </DropdownMenuTrigger>
            <DropdownMenuContent align="start" className="max-w-[calc(100vw-1.5rem)]">
              <DropdownMenuLabel>Reasoning effort</DropdownMenuLabel>
              <DropdownMenuRadioGroup
                value={picker.efforts.find((effort) => effort.current)?.effort ?? ""}
                onValueChange={(value) => {
                  const effort = picker.efforts.find((option) => option.effort === value);
                  if (effort) void apply({ kind: "effort", effort: effort.effort });
                }}
              >
                {picker.efforts.map((effort) => (
                  <DropdownMenuRadioItem
                    key={effort.effort}
                    value={effort.effort}
                    className="min-h-11"
                  >
                    <span className="flex-1">{effort.label}</span>
                    {effort.default && <Badge variant="outline">Default</Badge>}
                  </DropdownMenuRadioItem>
                ))}
              </DropdownMenuRadioGroup>
              {picker.running && (
                <p className="max-w-64 px-2 py-1.5 text-muted-foreground text-xs">
                  {picker.running}
                </p>
              )}
            </DropdownMenuContent>
          </DropdownMenu>
        ) : (
          <ReadOnlyChip
            name="Effort"
            icon={<Brain aria-hidden="true" className="size-3.5 shrink-0" />}
            value={footer.effort}
            hint="The next answer uses this effort; there is no other to choose here."
          />
        )}
        {picker?.resettable && (
          <Button
            variant="ghost"
            size="sm"
            className="min-h-11"
            aria-label="Reset model and effort to the agent's defaults"
            onClick={() => {
              // Reset removes itself; keep keyboard focus in the bar: on the model's picker, else
              // the effort's, else the group itself when both are read-only chips (R598-06).
              (modelButton.current ?? effortButton.current ?? group.current)?.focus();
              void apply({ kind: "reset" });
            }}
          >
            Reset
          </Button>
        )}
        {write?.state === "pending" && (
          <span role="status" className="px-1 text-muted-foreground">
            Waiting to sync.
          </span>
        )}
        {untrusted && (
          <Popover>
            <PopoverTrigger asChild>
              <Button
                variant="ghost"
                size="sm"
                data-slot="agent-untrusted-marker"
                className="min-h-11 gap-1 px-2 text-held"
              >
                <ShieldAlert aria-hidden="true" className="size-3.5" />
                Untrusted
                <span className="sr-only">: {untrusted}</span>
              </Button>
            </PopoverTrigger>
            <PopoverContent
              align="start"
              aria-label="Why commands ask"
              className="w-[min(20rem,calc(100vw-1.5rem))] text-sm"
            >
              {untrusted}
            </PopoverContent>
          </Popover>
        )}
      </fieldset>
      {notes.length > 0 && <div className="order-last flex basis-full flex-col gap-1">{notes}</div>}
    </div>
  );
}
