import { ChevronDown, ChevronRight } from "lucide-react";
import { useId, useRef, useState } from "react";
import { Button } from "@/components/ui/button";
import { type AgentSessionViewVm, agentSessionToolSet } from "@/lib/ipc/client";
import { setAgentViewPrefs, useAgentViewPrefsWrite } from "@/lib/stores/agent-view-prefs";
import { syncErrorMessage } from "@/lib/stores/sync";

export function SessionControls({
  vm,
  toolsOpen: controlled,
  onToolsOpenChange,
}: {
  vm: AgentSessionViewVm;
  toolsOpen?: boolean;
  onToolsOpenChange?: (open: boolean) => void;
}) {
  // The switches show the preferences as Rust resolves them now, carried with
  // the view: switched on another device or in a settings file, they follow.
  const { prefs } = vm;
  const write = useAgentViewPrefsWrite();
  const [ownToolsOpen, setOwnToolsOpen] = useState(false);
  const toolsOpen = controlled ?? ownToolsOpen;
  const setToolsOpen = onToolsOpenChange ?? setOwnToolsOpen;
  const [pending, setPending] = useState<string | null>(null);
  const writing = useRef(false);
  const [error, setError] = useState<string | null>(null);
  const toolsId = useId();

  // A session that cannot be changed here draws no switches at all: a tool
  // not changeable is absent, not disabled.
  const tools = vm.readOnly ? [] : vm.tools;
  // Never the `disabled` attribute while a write is in flight: it would drop
  // keyboard focus to the page. A click then is ignored instead (the write
  // store ignores a preference click too).
  return (
    <div className="shrink-0 border-border border-b px-3 py-1">
      <fieldset className="flex flex-wrap items-center gap-1" aria-label="Answer preferences">
        {vm.readsTrace && (
          <Button
            variant={prefs.showReasoning ? "secondary" : "ghost"}
            className="min-h-11"
            aria-pressed={prefs.showReasoning}
            aria-busy={write.busy}
            onClick={() => void setAgentViewPrefs({ showReasoning: !prefs.showReasoning })}
          >
            Show thinking
          </Button>
        )}
        <Button
          variant={prefs.showDetails ? "secondary" : "ghost"}
          className="min-h-11"
          aria-pressed={prefs.showDetails}
          aria-busy={write.busy}
          onClick={() => void setAgentViewPrefs({ showDetails: !prefs.showDetails })}
        >
          Show details
        </Button>
        {tools.length > 0 && (
          <Button
            variant="ghost"
            className="min-h-11"
            aria-expanded={toolsOpen}
            aria-controls={toolsId}
            onClick={() => setToolsOpen(!toolsOpen)}
          >
            {toolsOpen ? <ChevronDown aria-hidden="true" /> : <ChevronRight aria-hidden="true" />}
            Tools
          </Button>
        )}
      </fieldset>
      {toolsOpen && tools.length > 0 && (
        <fieldset id={toolsId} className="max-h-48 overflow-y-auto pb-2">
          <legend className="text-muted-foreground text-xs">Tools allowed in this session</legend>
          <p className="py-1 text-muted-foreground text-xs">
            Turning a tool on only restores what this agent offers.
          </p>
          <div className="flex flex-wrap gap-1">
            {tools.map((tool) => (
              <Button
                key={tool.name}
                role="switch"
                aria-checked={tool.on}
                variant={tool.on ? "secondary" : "outline"}
                className="h-auto min-h-11 max-w-full whitespace-normal break-all"
                aria-busy={pending === tool.name}
                onClick={async () => {
                  if (writing.current) return;
                  writing.current = true;
                  setPending(tool.name);
                  setError(null);
                  try {
                    await agentSessionToolSet(
                      { database: vm.database, sessionId: vm.sessionId },
                      tool.name,
                      !tool.on,
                    );
                  } catch (raw) {
                    setError(syncErrorMessage(raw, "keeper could not change this tool."));
                  } finally {
                    writing.current = false;
                    setPending(null);
                  }
                }}
              >
                <span>{tool.name}</span>
                <span aria-hidden="true" className="text-muted-foreground">
                  {tool.on ? "On" : "Off"}
                </span>
              </Button>
            ))}
          </div>
        </fieldset>
      )}
      {(write.error || error) && (
        <p role="alert" className="py-1 text-destructive text-sm [overflow-wrap:anywhere]">
          {error ?? write.error}
        </p>
      )}
      {write.shadowed && (
        <p role="alert" className="py-1 text-held text-xs [overflow-wrap:anywhere]">
          Saved, but a settings file keeps this preference as it is.
        </p>
      )}
    </div>
  );
}
