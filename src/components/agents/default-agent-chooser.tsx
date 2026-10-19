/**
 * The launch's "Choose the agent keeper opens when it starts" (story 102.7,
 * R463), at the top of the Agents window's list on the desktop and the phone.
 *
 * Shown once, by the launch, when Rust says nothing is chosen and there is an
 * agent to choose (`AgentsLaunchVm.chooser`). Non-blocking: a region in the
 * list, never a dialog — the person may ignore it and open any room. Every
 * agent is a button (the Tab order reaches them); the choices are Rust's, the
 * same Settings › Agents › Default agent lists. A choice stores the default
 * and opens that agent's conversation at once with its composer focused;
 * "Not now" puts it away until keeper is updated. Either takes it off screen.
 *
 * Mounted with the Agents window, it also reads the default agent once when
 * nothing has yet (a launch the person cut short, a phone that restored its
 * room): the rows' "Default" marker reads the same answer. A read that fails
 * is said here, whether or not the chooser is shown (R381), as well as in
 * `keeper.log`.
 *
 * The person always wins (R397): a choice opens its conversation only if the
 * person has not opened another room or window — nor left the Agents window
 * — since pressing it, however long the save or the conversation request
 * took. The choice is saved either way.
 */
import { useEffect, useId, useRef } from "react";
import { toast } from "sonner";
import { Button } from "@/components/ui/button";
import { useShellLayout } from "@/hooks/use-shell-layout";
import {
  guardNavigation,
  type NavigationGuard,
  openDefaultAgent,
  saveDefaultAgent,
} from "@/lib/default-agent-actions";
import { defaultAgentStore, useDefaultAgentStore } from "@/lib/stores/default-agent";
import { cn } from "@/lib/utils";

export const DEFAULT_AGENT_CHOOSER_TITLE = "Choose the agent keeper opens when it starts";
export const DEFAULT_AGENT_CHOOSER_NOTE =
  "Its conversation opens whenever keeper starts. You can change it in Settings › Agents, or with “Make default” on any agent here.";
export const DEFAULT_AGENT_CHOOSER_NOT_NOW = "Not now";

export function DefaultAgentChooser() {
  const open = useDefaultAgentStore((s) => s.chooserOpen);
  const vm = useDefaultAgentStore((s) => s.vm);
  const saving = useDefaultAgentStore((s) => s.saving);
  const readError = useDefaultAgentStore((s) => s.readError);
  const { phone } = useShellLayout();
  const id = useId();
  // The choices still opening: leaving the Agents window cancels them.
  const opening = useRef(new Set<NavigationGuard>());
  useEffect(() => {
    if (defaultAgentStore.getState().vm === null) {
      void defaultAgentStore.getState().load();
    }
    const choices = opening.current;
    return () => {
      for (const guard of choices) {
        guard.cancel();
      }
    };
  }, []);
  // Rust's sentence for a read that failed, where the person is looking.
  const failure =
    readError === null ? null : (
      <p
        role="alert"
        data-testid="default-agent-read-failure"
        className="mx-3 my-2 text-destructive text-xs [overflow-wrap:anywhere]"
      >
        {readError}
      </p>
    );
  // Chosen meanwhile (Settings, a row's menu) or nothing left to choose.
  if (!open || vm === null || vm.defaultAgent !== null || vm.choices.length === 0) {
    return failure;
  }
  const target = phone ? "min-h-11" : undefined;

  const choose = async (agent: string) => {
    // Taken at the press, before the save is awaited (R397).
    const guard = guardNavigation();
    opening.current.add(guard);
    const saved = await saveDefaultAgent(agent);
    if (saved === null) {
      // Said by `saveDefaultAgent`; the chooser stays for another try.
      guard.release();
      opening.current.delete(guard);
      return;
    }
    defaultAgentStore.getState().closeChooser();
    openDefaultAgent(saved, guard);
  };
  const putAway = async () => {
    const failed = await defaultAgentStore.getState().dismissChooser();
    if (failed !== null) {
      toast.error(failed);
    }
  };

  return (
    <>
      {failure}
      <section
        aria-labelledby={`${id}-title`}
        data-testid="default-agent-chooser"
        className="mx-3 my-2 flex min-w-0 flex-col gap-2 rounded-md border bg-card p-3 text-sm"
      >
        <h2 id={`${id}-title`} className="font-medium [overflow-wrap:anywhere]">
          {DEFAULT_AGENT_CHOOSER_TITLE}
        </h2>
        <p className="text-muted-foreground text-xs [overflow-wrap:anywhere]">
          {DEFAULT_AGENT_CHOOSER_NOTE}
        </p>
        <ul aria-label="Agents" className="flex min-w-0 flex-wrap gap-2">
          {vm.choices.map((choice) => (
            <li key={choice.agent} className="min-w-0 max-w-full">
              <Button
                type="button"
                variant="secondary"
                size="sm"
                disabled={saving}
                title={choice.agent}
                onClick={() => void choose(choice.agent)}
                className={cn("max-w-full", target)}
              >
                <span className="truncate">{choice.name}</span>
              </Button>
            </li>
          ))}
        </ul>
        <Button
          type="button"
          variant="ghost"
          size="sm"
          disabled={saving}
          onClick={() => void putAway()}
          className={cn("self-start", target)}
        >
          {DEFAULT_AGENT_CHOOSER_NOT_NOW}
        </Button>
      </section>
    </>
  );
}
