/**
 * The ⌘9 bot composer's slash commands — what the pane does with them (Epic 61,
 * Story 61.9, FR-385).
 *
 * The menu's mechanics are shared with the messenger composer and live in
 * `@/components/slash-menu/slash-menu`; what a command *means* is
 * `keeper_core::bots::commands`. This file holds only the bot pane's side: the
 * context it sends with each preview and what it does with a resolved command.
 */
import type { ProviderKind } from "@/lib/ipc/client";

/**
 * The context the composer sends with every preview, assembled from what the
 * pane holds.
 *
 * Its own type rather than the generated request so the pane passes four plain
 * facts and the composer does the naming — and so `modelTools` stays a
 * tri-state on the way through: `null` is *the endpoint did not say*, never
 * *no* (AD-27).
 */
export interface BotCommandContext {
  /** The chosen bot's provider kind, or `null` when no bot is chosen. */
  providerKind: ProviderKind | null;
  /** Whether any provider is configured. */
  hasProvider: boolean;
  /** Whether a bot is chosen. */
  hasBot: boolean;
  /** Whether the open conversation has started. */
  hasSession: boolean;
  /** Whether the chosen model takes tools, as the endpoint stated it. */
  modelTools: boolean | null;
}

// ---------------------------------------------------------------------------
// What the pane does with a resolved command.
//
// Here rather than inline in `bots-pane.tsx` for two reasons: the sentences are
// copy and belong beside the rest of this story's copy, and the pane is a file
// several stories are wiring at once — a switch statement grown there is a
// merge conflict with a `return null` in it.
// ---------------------------------------------------------------------------

/** What `/metadata` says: the toggle is a control, not a command's effect. */
export const BOT_COMMAND_METADATA_IS_A_TOGGLE =
  "The per-answer details toggle is in the pane header.";

/** What `/grant` says once resolution has already allowed it. */
export const BOT_COMMAND_GRANT_IS_A_SURFACE =
  "What this bot may read and write is in the bar above, and in Settings → Bots.";

/** What `/history` says: the list is already on screen. */
export const BOT_COMMAND_HISTORY_IS_A_LIST = "Your conversations are listed above, newest first.";

/** What `/bot` says when no bot answers to the name that was typed. */
export const botCommandNoSuchBot = (typed: string | null) =>
  `No bot here is called "${typed ?? ""}". The bots you have are listed above.`;

/**
 * The four facts the registry reasons about, read off what the pane holds.
 *
 * `modelTools` passes through untouched: `null` is *the endpoint did not say*,
 * and flattening it to `false` here would hide the grant affordance for a
 * model that may well take tools (AD-27).
 */
export function botCommandContext(input: {
  providerKind: ProviderKind | null;
  providerCount: number;
  botId: string | null;
  hasSession: boolean;
  modelTools: boolean | null;
}): BotCommandContext {
  return {
    providerKind: input.providerKind,
    hasProvider: input.providerCount > 0,
    hasBot: input.botId !== null,
    hasSession: input.hasSession,
    modelTools: input.modelTools,
  };
}

/**
 * What the Bots pane does with a command keeper resolved.
 *
 * Returns `null` when it acted and the sentence to show when it did not — the
 * contract `BotComposer.onCommand` states, and the reason a command that lands
 * on a surface this pane does not own still says something true instead of
 * doing nothing. `/help` never arrives: the composer answers it.
 */
export function botCommandHost(deps: {
  bots: readonly { id: string; name: string }[];
  newConversation: () => void;
  selectBot: (botId: string) => void;
  selectModel: (model: string) => void;
}): (command: BotCommandInvocation) => string | null {
  return (command) => {
    switch (command.name) {
      case "new": {
        deps.newConversation();
        return null;
      }
      case "bot": {
        const wanted = (command.args ?? "").toLowerCase();
        // By name, because the name is what the picker shows and what somebody
        // would type; the id is a ULID nobody reads.
        const match = deps.bots.find((bot) => bot.name.toLowerCase() === wanted);
        if (match === undefined) {
          return botCommandNoSuchBot(command.args);
        }
        deps.selectBot(match.id);
        return null;
      }
      case "model": {
        if (command.args === null) {
          return null;
        }
        // Not verified against the model list on purpose: the endpoint is the
        // authority on its own model names, a send is what asks it, and a
        // picker that refused a tag the server would have accepted is a
        // client inventing a rule.
        deps.selectModel(command.args);
        return null;
      }
      case "metadata":
        return BOT_COMMAND_METADATA_IS_A_TOGGLE;
      case "grant":
        return BOT_COMMAND_GRANT_IS_A_SURFACE;
      default:
        return BOT_COMMAND_HISTORY_IS_A_LIST;
    }
  };
}

/** What the composer hands its parent when a command is accepted. */
export interface BotCommandInvocation {
  /** The registry name, never the alias that was typed. */
  name: string;
  /** What followed it, trimmed, or `null` when nothing did. */
  args: string | null;
}
