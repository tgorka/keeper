import { ChevronDown, Info, Star } from "lucide-react";
import { useRef, useState } from "react";
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
import {
  type AgentModelOptionVm,
  type AgentSessionPickerVm,
  type AgentSessionRef,
  type AgentSettingsChange,
  agentSessionSettingsChange,
} from "@/lib/ipc/client";
import { rememberAgentPick } from "@/lib/stores/agent-picks";
import { toggleModelFavorite, useModelFavorites } from "@/lib/stores/model-favorites";
import { syncErrorMessage } from "@/lib/stores/sync";
import { cn } from "@/lib/utils";

/**
 * The composer footer's model picker, effort menu and reset (109.18; t3code's pickers). Every
 * decision — what is offered, which is current, which is a default, whether there is an effort menu
 * at all, what a running turn or a refusal means — is Rust's (`AgentSessionPickerVm`). Only the
 * search text, the stars (a lens, in a cookie) and the order they give are this component's; what a
 * change wrote is remembered as this device's last pick for `agent` (Q109-16 b). The model popover
 * may be held by the view (`modelsOpen`), so the list's right-click "Model…" opens this same picker.
 */
export function SessionSettingsPicker({
  target,
  agent,
  picker,
  modelsOpen: heldOpen,
  onModelsOpenChange,
}: {
  target: AgentSessionRef;
  agent: string;
  picker: AgentSessionPickerVm;
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

  const starred = picker.models.filter((model) => favorites.includes(model.id));
  const rest = picker.models.filter((model) => !favorites.includes(model.id));
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
  const write = picker.write;
  // A refusal written here before this mount still stands until a new change sets it aside.
  const alert = error ?? (write.state === "refused" ? write.sentence : null);
  const hasEfforts = picker.effortLabel !== undefined && picker.efforts.length > 0;
  return (
    <div data-slot="agent-session-settings" className="mt-2 flex min-w-0 flex-col gap-1">
      {/* An agent that offers nothing to choose draws no empty group. */}
      {(picker.models.length > 0 || hasEfforts) && (
        <fieldset
          aria-label="Model and effort"
          className="flex min-w-0 flex-wrap items-center gap-1 text-xs"
        >
          {picker.models.length > 0 && (
            <Popover open={modelsOpen} onOpenChange={setModelsOpen}>
              <PopoverTrigger asChild>
                <Button
                  ref={modelButton}
                  variant="ghost"
                  size="sm"
                  className="h-auto min-h-11 max-w-full justify-start whitespace-normal break-all"
                  aria-label={`Model: ${picker.modelLabel}`}
                >
                  <span className="min-w-0">{picker.modelLabel}</span>
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
                    {rest.length > 0 && (
                      <CommandGroup heading={starred.length > 0 ? "Models" : undefined}>
                        {rest.map(row)}
                      </CommandGroup>
                    )}
                  </CommandList>
                  {picker.running && (
                    <p className="border-t px-3 py-2 text-muted-foreground text-xs">
                      {picker.running}
                    </p>
                  )}
                </Command>
              </PopoverContent>
            </Popover>
          )}
          {hasEfforts && (
            <DropdownMenu>
              <DropdownMenuTrigger asChild>
                <Button
                  variant="ghost"
                  size="sm"
                  className="min-h-11"
                  aria-label={`Effort: ${picker.effortLabel}`}
                >
                  {picker.effortLabel}
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
          )}
          {picker.resettable && (
            <Button
              variant="ghost"
              size="sm"
              className="min-h-11"
              aria-label="Reset model and effort to the agent's defaults"
              onClick={() => {
                // Reset removes itself; keep keyboard focus in the footer.
                modelButton.current?.focus();
                void apply({ kind: "reset" });
              }}
            >
              Reset
            </Button>
          )}
          {write.state === "pending" && (
            <span role="status" className="text-muted-foreground">
              Waiting to sync.
            </span>
          )}
        </fieldset>
      )}
      {picker.fallback && (
        <p
          role="status"
          className="flex items-start gap-1 text-held text-xs [overflow-wrap:anywhere]"
        >
          <Info aria-hidden="true" className="mt-0.5 size-3.5 shrink-0" />
          <span>{picker.fallback}</span>
        </p>
      )}
      {notice && picker.running && (
        <p role="status" className="text-muted-foreground text-xs [overflow-wrap:anywhere]">
          {notice}
        </p>
      )}
      {alert && (
        <p role="alert" className="text-destructive text-xs [overflow-wrap:anywhere]">
          {alert}
        </p>
      )}
    </div>
  );
}
