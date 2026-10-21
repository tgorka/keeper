import { ChevronDown, ChevronRight } from "lucide-react";
import { useEffect, useId, useState } from "react";
import { Button } from "@/components/ui/button";
import { type AgentSessionViewVm, agentSessionToolSet } from "@/lib/ipc/client";
import {
  loadAgentViewPrefs,
  setAgentViewPrefs,
  useAgentViewPrefs,
} from "@/lib/stores/agent-view-prefs";
import { syncErrorMessage } from "@/lib/stores/sync";

export function SessionControls({ vm }: { vm: AgentSessionViewVm }) {
  const prefs = useAgentViewPrefs();
  const [toolsOpen, setToolsOpen] = useState(false);
  const [pending, setPending] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  const toolsId = useId();
  useEffect(() => {
    void loadAgentViewPrefs();
  }, []);

  return (
    <div className="shrink-0 border-border border-b px-3 py-1">
      <fieldset className="flex flex-wrap items-center gap-1" aria-label="Answer preferences">
        {vm.readsTrace && (
          <Button
            variant={prefs.value.showReasoning ? "secondary" : "ghost"}
            className="min-h-11"
            aria-pressed={prefs.value.showReasoning}
            disabled={!prefs.loaded || prefs.busy}
            onClick={() => void setAgentViewPrefs({ showReasoning: !prefs.value.showReasoning })}
          >
            Show thinking
          </Button>
        )}
        <Button
          variant={prefs.value.showDetails ? "secondary" : "ghost"}
          className="min-h-11"
          aria-pressed={prefs.value.showDetails}
          disabled={!prefs.loaded || prefs.busy}
          onClick={() => void setAgentViewPrefs({ showDetails: !prefs.value.showDetails })}
        >
          Show details
        </Button>
        {vm.tools.length > 0 && (
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
      {toolsOpen && vm.tools.length > 0 && (
        <fieldset id={toolsId} className="max-h-48 overflow-y-auto pb-2">
          <legend className="text-muted-foreground text-xs">Tools allowed in this session</legend>
          <p className="py-1 text-muted-foreground text-xs">
            Turning a tool on only restores what this agent offers.
          </p>
          <div className="flex flex-wrap gap-1">
            {vm.tools.map((tool) => (
              <Button
                key={tool.name}
                role="switch"
                aria-checked={tool.on}
                variant={tool.on ? "secondary" : "outline"}
                className="h-auto min-h-11 max-w-full whitespace-normal break-all"
                disabled={pending !== null || vm.readOnly}
                onClick={async () => {
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
      {(prefs.error || error) && (
        <div
          role="alert"
          className="flex flex-wrap items-center gap-2 py-1 text-destructive text-sm [overflow-wrap:anywhere]"
        >
          <span>{error ?? prefs.error}</span>
          {!prefs.loaded && (
            <Button variant="ghost" className="min-h-11" onClick={() => void loadAgentViewPrefs()}>
              Try preferences again
            </Button>
          )}
        </div>
      )}
    </div>
  );
}
