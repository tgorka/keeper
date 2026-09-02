/**
 * The composer's slash-command menu — the mechanics, and only the mechanics
 * (Epic 61, Story 61.9, FR-385).
 *
 * **Nothing here decides what a command means.** The registry, the resolution
 * order (exact name, then alias, then the first prefix), the refusal sentences,
 * the availability reasons and the escape are all
 * `keeper_core::bots::commands`, reached through {@link botsCommandPreview}. A
 * matcher in this file beside that one would be two opinions about what `/mod`
 * means, and the one that disagrees is always the one the person is looking at.
 * So this module answers three questions a textarea has to answer locally,
 * before any round trip:
 *
 * 1. **Is this draft even a candidate?** {@link isCommandLine} — one line, one
 *    leading slash, nothing before it, and short. The note editor's own
 *    narrowness (`slash-menu.ts:4-5`: "a slash inside a path or a fraction
 *    stays a slash") restated for a `<textarea>`, and it is what keeps a
 *    keystroke of prose from becoming a round trip.
 * 2. **Which token is the menu about?** {@link slashToken} — so a dismissal can
 *    be remembered against the token it dismissed and forgotten when the token
 *    changes. Escape must mean "not this one", not "never again".
 * 3. **Did the menu handle that keystroke?** {@link menuKeyAction} — a closed
 *    set of five, and `null` for everything else. A menu that swallowed a key
 *    it did not act on would eat the webview's own handling, which is AD-27's
 *    no-dead-chord rule applied to a key rather than to a button.
 *
 * The keyboard model is the note editor's, which is `@codemirror/autocomplete`'s
 * and therefore the one this app has already taught: `↑`/`↓` move, `Enter`
 * accepts the highlighted row, `Tab` accepts it too, `Escape` dismisses. `Tab`
 * accepting rather than cycling is that library's behaviour and the reason it
 * is spelled out here: two pickers in one app where Tab means different things
 * is a smaller surface than it looks, but it is the kind of difference nobody
 * reports and everybody feels.
 */
import type { BotCommandRowVm, ProviderKind } from "@/lib/ipc/client";

/**
 * The longest draft worth asking Rust about.
 *
 * Mirrors `commands::MAX_COMMAND_CHARS`. Duplicated as a number rather than
 * fetched because it is a *bound*, not a rule: the worst a drift can do is one
 * wasted round trip that comes back as prose, which is the same answer this
 * side would have given.
 */
export const MAX_COMMAND_CHARS = 512;

/** What a keystroke did to the menu, or `null` when the menu did not act. */
export type MenuKeyAction = "up" | "down" | "accept" | "dismiss";

/**
 * Whether a draft could be a command at all.
 *
 * A doubled slash is the escape and is therefore *not* a command line: `//etc`
 * is a message about `/etc`, so no menu opens over it and no round trip is
 * spent asking.
 */
export function isCommandLine(draft: string): boolean {
  return (
    draft.length <= MAX_COMMAND_CHARS &&
    draft.startsWith("/") &&
    !draft.startsWith("//") &&
    !draft.includes("\n")
  );
}

/**
 * The token the menu is about — the word right after the slash, lowercased —
 * or `null` when the draft is not a command line.
 *
 * Lowercased because resolution is case-insensitive in Rust, so `/NEW` and
 * `/new` are one token and a dismissal of one is a dismissal of the other.
 */
export function slashToken(draft: string): string | null {
  if (!isCommandLine(draft)) {
    return null;
  }
  const rest = draft.slice(1);
  const boundary = rest.search(/\s/);
  return (boundary === -1 ? rest : rest.slice(0, boundary)).toLowerCase();
}

/**
 * What the menu should do with a keydown, or `null` to leave it alone.
 *
 * `open` is passed rather than inferred so that a dismissed menu is inert:
 * `Escape` with no menu open belongs to the app's universal Esc chain
 * (UX-DR12), and `Enter` with no menu open is the composer's send.
 */
export function menuKeyAction(
  key: string,
  open: boolean,
  modifiers: { shiftKey: boolean; metaKey: boolean; ctrlKey: boolean; altKey: boolean },
): MenuKeyAction | null {
  if (!open) {
    return null;
  }
  // A modified arrow is a selection or a word jump, and a modified Enter is the
  // chord the composer deliberately leaves unbound. None of them are the menu's.
  if (modifiers.metaKey || modifiers.ctrlKey || modifiers.altKey) {
    return null;
  }
  switch (key) {
    case "ArrowUp":
      return "up";
    case "ArrowDown":
      return "down";
    case "Enter":
      // Shift+Enter is the newline, always, menu or no menu: the one chord that
      // must never depend on what is on screen.
      return modifiers.shiftKey ? null : "accept";
    case "Tab":
      return modifiers.shiftKey ? null : "accept";
    case "Escape":
      return "dismiss";
    default:
      return null;
  }
}

/**
 * Where the highlight lands after a move, wrapping at both ends.
 *
 * Wrapping because the list is short and closed: reaching the bottom and
 * pressing `↓` again to get back to the top is what every picker in this app
 * already does, and a highlight that stops dead reads as a stuck key.
 */
export function nextIndex(index: number, count: number, action: "up" | "down"): number {
  if (count === 0) {
    return 0;
  }
  const step = action === "down" ? 1 : -1;
  return (index + step + count) % count;
}

/**
 * The draft that replaces the current one when a row is accepted.
 *
 * The name is completed and a space is added for the commands that take an
 * argument, so accepting `/bot` leaves the caret where the bot name goes rather
 * than one keystroke short of it — the note editor's caret rule
 * (`slash-menu.ts:44-52`) applied to a token instead of to a pair. A command
 * that takes nothing is completed without the space, because a trailing space
 * would be text after a name that refuses text.
 */
export function acceptedDraft(row: BotCommandRowVm): string {
  return row.args === "none" ? `/${row.name}` : `/${row.name} `;
}

/**
 * The argument hint a row shows beside its name, or `null` when it takes none.
 *
 * Claude Code's `argument-hint` (R5 §1.2), rendered as the row's own trailing
 * text rather than as a tooltip: a hint you have to hover for is a hint you
 * read once.
 */
export function argumentHint(row: BotCommandRowVm): string | null {
  if (row.argHint === null) {
    return null;
  }
  return row.args === "optionalRest" ? `[${row.argHint}]` : `<${row.argHint}>`;
}

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
