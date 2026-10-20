import type { WebSearchLevel } from "@/lib/ipc/client";

/**
 * An agent's three web search levels, in the order Settings › Agents and the
 * agent room's menu offer them (R457; story 102.3).
 */
export const WEB_SEARCH_LEVELS: ReadonlyArray<readonly [WebSearchLevel, string]> = [
  ["off", "Off"],
  ["ask", "Ask"],
  ["withoutAsking", "Without asking"],
];
