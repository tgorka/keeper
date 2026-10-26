/**
 * This device's last model and effort pick per agent (109.18; Q109-16 b: "the last pick is offered
 * as that agent's default for new sessions"), remembered in a cookie.
 *
 * Per device and keyed by the agent's actor id, in the same pattern as the model favourites
 * (`model-favorites.ts`) and every other remembered lens (`fold-cookie.ts`): no settings row, no
 * IPC write, not `localStorage`. What is remembered is exactly what Rust answered the change wrote
 * (`AgentSettingsChangedVm.pick`); a reset forgets the agent. Whether a remembered pick still
 * applies is Rust's decision: a new session hands it over and Rust writes only what the agent still
 * offers (`keeper_core::hq::settings::initial`).
 */
import type { AgentSettingsPick, Effort } from "@/lib/ipc/client";
import { FOLD_MAX_AGE, persistFold } from "@/lib/stores/fold-cookie";

export const AGENT_PICKS_COOKIE = "keeper_agent_picks";

type Picks = Record<string, AgentSettingsPick>;

const EFFORTS: readonly Effort[] = ["low", "medium", "high", "xhigh"];

/** The remembered picks in `cookie`; anything unreadable is dropped. */
export function readAgentPicks(cookie: string): Picks {
  for (const part of cookie.split(";")) {
    const [name, ...value] = part.trim().split("=");
    if (name !== AGENT_PICKS_COOKIE) continue;
    try {
      const parsed: unknown = JSON.parse(decodeURIComponent(value.join("=")));
      if (typeof parsed !== "object" || parsed === null || Array.isArray(parsed)) return {};
      const picks: Picks = {};
      for (const [agent, raw] of Object.entries(parsed)) {
        if (typeof raw !== "object" || raw === null) continue;
        const { model, effort } = raw as Record<string, unknown>;
        const pick: AgentSettingsPick = {
          model: typeof model === "string" && model.length > 0 ? model : null,
          effort: EFFORTS.find((known) => known === effort) ?? null,
        };
        if (pick.model !== null || pick.effort !== null) picks[agent] = pick;
      }
      return picks;
    } catch {
      return {};
    }
  }
  return {};
}

/** The `document.cookie` assignment that records `picks`. */
export function agentPicksCookie(picks: Picks): string {
  const value = encodeURIComponent(JSON.stringify(picks));
  return `${AGENT_PICKS_COOKIE}=${value}; path=/; max-age=${FOLD_MAX_AGE}`;
}

function held(): Picks {
  return typeof document === "undefined" ? {} : readAgentPicks(document.cookie);
}

/** This device's last pick for `agent`, or `null` when there is none. */
export function lastAgentPick(agent: string): AgentSettingsPick | null {
  return held()[agent] ?? null;
}

/** Remember what a change wrote for `agent`; a pick of two defaults (a reset) forgets it. */
export function rememberAgentPick(agent: string, pick: AgentSettingsPick): void {
  const picks = held();
  if (pick.model === null && pick.effort === null) delete picks[agent];
  else picks[agent] = { model: pick.model, effort: pick.effort };
  persistFold(agentPicksCookie(picks));
}
