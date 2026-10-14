/**
 * Settings › Agents › Default agent (story 100.3, R383): the one agent keeper
 * opens at launch, chosen from the agents with a conversation here, or none.
 *
 * Every word but the copy is Rust's (`AgentsLaunchVm`): which agents may be
 * chosen, which one is stored, and — when the stored one does not open at
 * launch — why. A stored agent that is no longer among the choices stays
 * shown and selected beside that sentence, so the person sees what is set
 * and can change it. When a settings file decides the default, Rust's sentence
 * says so and names the file. A read or save that fails says so beside the
 * control — before the section would hide for having nothing to choose — and
 * reaches `keeper.log`. The choices are read again when the Agents window's
 * rooms change while Settings is open (R397).
 *
 * On every tier: `agents.default` is a preference about the person, the phone
 * applies the same launch rule, and the key travels with the account's
 * settings. Absent, not empty (AD-27): with no agent to choose and none
 * stored, the section renders nothing.
 */
import { useEffect, useId } from "react";
import { FileControlled } from "@/components/settings/config-source-section";
import { Label } from "@/components/ui/label";
import { RadioGroup, RadioGroupItem } from "@/components/ui/radio-group";
import { useAgentRoomsStore } from "@/lib/stores/agent-rooms";
import { defaultAgentStore, useDefaultAgentStore } from "@/lib/stores/default-agent";

export const DEFAULT_AGENT_TITLE = "Default agent";
export const DEFAULT_AGENT_NOTE =
  "When keeper starts and you have agent rooms, it opens Agents on this agent's conversation.";
export const DEFAULT_AGENT_NONE = "None";

/** The radio value for "no default agent" (a user id never starts with a dash). */
const NONE = "-";

export function DefaultAgentSection({ open }: { open: boolean }) {
  const vm = useDefaultAgentStore((s) => s.vm);
  const readError = useDefaultAgentStore((s) => s.readError);
  const saveError = useDefaultAgentStore((s) => s.saveError);
  const saving = useDefaultAgentStore((s) => s.saving);
  const agentRooms = useAgentRoomsStore((s) => s.rooms.map((room) => room.roomId).join("\n"));
  const id = useId();
  const error = readError ?? saveError;

  // biome-ignore lint/correctness/useExhaustiveDependencies: the read is redone when the Agents window's rooms change while Settings is open, which is what `agentRooms` is for.
  useEffect(() => {
    if (open) {
      void defaultAgentStore.getState().load();
    }
  }, [open, agentRooms]);

  // Absent with nothing to choose, nothing chosen and no file deciding —
  // unless something failed, which is said here.
  if (vm === null || (vm.choices.length === 0 && vm.defaultAgent === null && vm.fileSet === null)) {
    return error === null ? null : (
      <section
        aria-labelledby={`${id}-title`}
        className="flex min-w-0 flex-col gap-2 border-t pt-4 text-sm"
      >
        <h3 id={`${id}-title`} className="font-medium">
          {DEFAULT_AGENT_TITLE}
        </h3>
        <p role="alert" className="text-destructive text-xs [overflow-wrap:anywhere]">
          {error}
        </p>
      </section>
    );
  }
  const stored = vm.defaultAgent;
  const storedElsewhere = stored !== null && !vm.choices.some((c) => c.agent === stored);

  return (
    <section
      aria-labelledby={`${id}-title`}
      className="flex min-w-0 flex-col gap-2 border-t pt-4 text-sm"
    >
      <div className="flex min-w-0 flex-wrap items-center gap-2">
        <h3 id={`${id}-title`} className="font-medium">
          {DEFAULT_AGENT_TITLE}
        </h3>
        <FileControlled settingKey="agents.default" />
      </div>
      <p className="text-muted-foreground text-xs">{DEFAULT_AGENT_NOTE}</p>
      <RadioGroup
        value={stored ?? NONE}
        onValueChange={(value) => {
          void defaultAgentStore.getState().choose(value === NONE ? null : value);
        }}
        aria-label={DEFAULT_AGENT_TITLE}
        disabled={saving}
        className="gap-2"
      >
        <div className="flex items-center gap-2">
          <RadioGroupItem id={`${id}-none`} value={NONE} />
          <Label htmlFor={`${id}-none`} className="font-normal">
            {DEFAULT_AGENT_NONE}
          </Label>
        </div>
        {vm.choices.map((choice) => (
          <div key={choice.agent} className="flex min-w-0 items-center gap-2">
            <RadioGroupItem id={`${id}-${choice.agent}`} value={choice.agent} />
            <Label
              htmlFor={`${id}-${choice.agent}`}
              className="min-w-0 font-normal [overflow-wrap:anywhere]"
            >
              {choice.name}
              <span className="text-muted-foreground text-xs">{choice.agent}</span>
            </Label>
          </div>
        ))}
        {storedElsewhere && (
          <div className="flex min-w-0 items-center gap-2">
            <RadioGroupItem id={`${id}-stored`} value={stored} />
            <Label
              htmlFor={`${id}-stored`}
              className="min-w-0 font-normal [overflow-wrap:anywhere]"
            >
              {stored}
            </Label>
          </div>
        )}
      </RadioGroup>
      {vm.fileSet !== null && (
        <p role="note" className="text-muted-foreground text-xs [overflow-wrap:anywhere]">
          {vm.fileSet}
        </p>
      )}
      {vm.reason !== null && (
        <p className="text-muted-foreground text-xs [overflow-wrap:anywhere]">{vm.reason}</p>
      )}
      {error !== null && (
        <p role="alert" className="text-destructive text-xs [overflow-wrap:anywhere]">
          {error}
        </p>
      )}
    </section>
  );
}
