/**
 * An agent instance's online state as Rust says it (`AgentInstancePresenceVm`): a lamp and its
 * word, never the lamp alone. "Not known here" is this device not seeing presence (its copy of the
 * database is not syncing now) — not an instance that is offline.
 */
import { Lamp } from "@/components/ui/lamp";
import type { AgentInstancePresenceVm } from "@/lib/ipc/client";

export const PRESENCE_WORD: Record<AgentInstancePresenceVm, string> = {
  online: "Online",
  offline: "Offline",
  unknown: "Not known here",
};

export function PresenceMark({ presence }: { presence: AgentInstancePresenceVm }) {
  return (
    <span data-presence={presence} className="inline-flex shrink-0 items-center gap-1">
      <Lamp state={presence === "online" ? "live" : "idle"} label={null} />
      <span className={presence === "online" ? "" : "text-muted-foreground"}>
        {PRESENCE_WORD[presence]}
      </span>
    </span>
  );
}
