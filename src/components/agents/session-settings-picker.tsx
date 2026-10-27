import { Brain, ChevronDown, Info, RotateCcw, Star } from "lucide-react";
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
  DropdownMenuItem,
  DropdownMenuLabel,
  DropdownMenuRadioGroup,
  DropdownMenuRadioItem,
  DropdownMenuSeparator,
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

/**
 * The bar's widths, read from the composer bar's own `@container/bar` (FooterOneRow), never the
 * viewport. Under 24rem (where the context is the ring alone) the chips tighten: less padding and
 * a smaller chevron on the model, none on the effort menu. Under 19rem — the Notes Assistant's 280px minimum is a 255px bar —
 * the effort folds into the model chip's words: the documented exception to R-HQ-85's separate
 * effort chip (docs/agents.md).
 */
const NARROW_TIGHT = "@max-[24rem]/bar:px-2";
const NARROW_HIDDEN = "@max-[24rem]/bar:hidden";
const FOLDED_HIDDEN = "@max-[19rem]/bar:hidden";
const FOLDED_SHOWN = "hidden @max-[19rem]/bar:block";
const FOLDED_SR_ONLY = "@max-[19rem]/bar:sr-only";

/** A chip in the composer's bar that opens nothing: the agent offers no alternative. */
function ReadOnlyChip({
  name,
  value,
  hint,
  icon,
  className,
}: {
  name: string;
  value: ReactNode;
  hint: string;
  icon?: ReactNode;
  className?: string;
}) {
  return (
    <HoverHint label={name} detail={hint}>
      <span
        data-slot="agent-readonly-chip"
        className={cn(
          "inline-flex min-h-11 min-w-0 items-center gap-1 whitespace-nowrap rounded-md px-2 text-muted-foreground text-xs",
          className,
        )}
      >
        {icon}
        <span className="sr-only">{name}: </span>
        {value}
      </span>
    </HoverHint>
  );
}

/** The model chip's words: the model's own name, with the effort folded in at the minimum. */
function ModelWords({ footer }: { footer: AgentSessionFooterVm }) {
  return (
    <>
      <span className={cn("min-w-0 truncate", FOLDED_HIDDEN)}>{footer.shortModel}</span>
      <span className={cn("min-w-0 truncate", FOLDED_SHOWN)}>{footer.compact}</span>
    </>
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
 * It renders as `display: contents` inside the composer's bar, which holds one row
 * (FooterOneRow): the group never forces a wrap (its basis is 0, it grows up to its content and
 * the model's name truncates), Reset lives in the model popover and the effort menu, and the
 * sentences sit on full-width lines after the row.
 */
export function SessionSettingsPicker({
  target,
  agent,
  footer,
  picker,
  modelsOpen: heldOpen,
  onModelsOpenChange,
}: {
  target: AgentSessionRef;
  agent: string;
  footer: AgentSessionFooterVm;
  /** Absent where this person may not change them, or while they are still arriving. */
  picker?: AgentSessionPickerVm;
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
  const reset = (
    <>
      <RotateCcw aria-hidden="true" />
      Reset to the agent's defaults
    </>
  );
  // Neither chip opens anything, yet a choice is saved: Reset stays in the bar (AD-260).
  const resetInBar =
    picker?.resettable && !(picker && (footer.choosesModel || footer.choosesEffort));
  if (write?.state === "pending") {
    notes.push(
      <p key="pending" role="status" className="text-muted-foreground text-xs">
        Waiting to sync.
      </p>,
    );
  }
  return (
    <div data-slot="agent-session-settings" className="contents">
      <fieldset
        ref={group}
        tabIndex={-1}
        aria-label="Model and effort"
        className="flex min-w-0 max-w-max flex-1 basis-0 items-center gap-0.5 text-xs"
      >
        {picker && footer.choosesModel ? (
          <Popover open={modelsOpen} onOpenChange={setModelsOpen}>
            <PopoverTrigger asChild>
              <Button
                variant="ghost"
                size="sm"
                className={cn("min-h-11 min-w-0 shrink justify-start", NARROW_TIGHT)}
                aria-label={`Model: ${footer.model}`}
              >
                <ModelWords footer={footer} />
                <ChevronDown aria-hidden="true" className="size-4 @max-[24rem]/bar:size-3" />
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
                {picker.resettable && (
                  <div className="border-t p-1">
                    <Button
                      variant="ghost"
                      size="sm"
                      className="min-h-11 w-full justify-start"
                      aria-label="Reset model and effort to the agent's defaults"
                      onClick={() => {
                        setModelsOpen(false);
                        void apply({ kind: "reset" });
                      }}
                    >
                      {reset}
                    </Button>
                  </div>
                )}
              </Command>
            </PopoverContent>
          </Popover>
        ) : (
          <ReadOnlyChip
            name="Model"
            value={<ModelWords footer={footer} />}
            hint="The next answer uses this model; there is no other to choose here."
            className="shrink"
          />
        )}
        {picker && footer.choosesEffort ? (
          <DropdownMenu>
            <DropdownMenuTrigger asChild>
              <Button
                variant="ghost"
                size="sm"
                className={cn("min-h-11 shrink-0", NARROW_TIGHT)}
                aria-label={`Effort: ${footer.effort}`}
              >
                <Brain aria-hidden="true" />
                <span className={FOLDED_SR_ONLY}>{footer.effort}</span>
                <ChevronDown aria-hidden="true" className={NARROW_HIDDEN} />
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
              {picker.resettable && (
                <>
                  <DropdownMenuSeparator />
                  <DropdownMenuItem
                    className="min-h-11"
                    aria-label="Reset model and effort to the agent's defaults"
                    onSelect={() => void apply({ kind: "reset" })}
                  >
                    {reset}
                  </DropdownMenuItem>
                </>
              )}
            </DropdownMenuContent>
          </DropdownMenu>
        ) : (
          <ReadOnlyChip
            name="Effort"
            icon={<Brain aria-hidden="true" className="size-3.5 shrink-0" />}
            value={footer.effort}
            hint="The next answer uses this effort; there is no other to choose here."
            // Folded into the model chip's words at the minimum; still read aloud here.
            className={cn("shrink-0", FOLDED_SR_ONLY)}
          />
        )}
        {resetInBar && (
          <Button
            variant="ghost"
            size="sm"
            className="min-h-11 shrink-0"
            aria-label="Reset model and effort to the agent's defaults"
            onClick={() => {
              // Reset removes itself; keep keyboard focus in the bar, on the group (R598-06).
              group.current?.focus();
              void apply({ kind: "reset" });
            }}
          >
            Reset
          </Button>
        )}
      </fieldset>
      {notes.length > 0 && <div className="order-last flex basis-full flex-col gap-1">{notes}</div>}
    </div>
  );
}
