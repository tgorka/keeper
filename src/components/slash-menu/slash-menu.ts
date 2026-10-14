/**
 * A composer's slash-command menu — the mechanics, and only the mechanics
 * (Epic 61, Story 61.9, FR-385; shared by the messenger composer since Story
 * 100.5, R385).
 *
 * **Nothing here decides what a command means.** The registry, the resolution
 * order (exact name, then alias, then the first prefix), the refusal sentences,
 * the availability reasons and the escape are Rust's — `keeper_core::bots::
 * commands` for the ⌘9 bot composer, `keeper_core::chat::commands` for chats
 * and agent rooms — reached through each composer's preview IPC. A matcher in
 * this file beside those would be a second opinion about what `/mod` means, and
 * the one that disagrees is always the one the person is looking at.
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
import type { ArgMode } from "@/lib/ipc/gen/ArgMode";

/**
 * One menu row, as both registries project it (`BotCommandRowVm`,
 * `ChatCommandRowVm`). `warning` is the bot registry's caveat a runnable row may
 * still owe; the chat registry has none.
 */
export interface SlashRow {
  /** The registry name, without the slash. */
  name: string;
  /** Every other spelling that reaches it. */
  aliases: readonly string[];
  /** The registry's one sentence. */
  description: string;
  /** What may follow the name. */
  args: ArgMode;
  /** What the argument is; `null` exactly when nothing may follow. */
  argHint: string | null;
  /** Whether Enter may run it right now. */
  available: boolean;
  /** Why not; `null` exactly when `available`. */
  reason: string | null;
  /** A caveat a runnable row still owes, where the registry has one. */
  warning?: string | null;
}

/**
 * What a preview carries that the menu reads — the shape both previews share.
 * `draft` is echoed so an answer for text the field no longer holds is dropped.
 */
export interface SlashPreview {
  draft: string;
  rows: readonly SlashRow[];
  escapeHint: string;
  /** A sentence the context owes about commands it does not serve, if any. */
  note?: string | null;
}

/**
 * The longest draft worth opening the menu for.
 *
 * Mirrors `bots::commands::MAX_COMMAND_CHARS`, which `chat::commands` shares.
 * Duplicated as a number rather than fetched because here it is a *bound*, not
 * a rule: it only decides whether the menu asks while you type. What a `/` line
 * means when it is sent is still Rust's — past the bound the messenger's answer
 * is a refusal, never text (R393) — so the worst a drift can do is a menu that
 * opens or stays shut near the edge.
 */
export const MAX_COMMAND_CHARS = 512;

/**
 * What a composer says under its field while it cannot ask Rust what a `/`
 * line is (R381): the menu is closed because of that, not because nothing
 * matched, and Enter will not send the line unresolved.
 */
export const SLASH_PREVIEW_FAILED =
  "keeper couldn't look up the commands here, so the menu is closed and the line will not be sent unresolved.";

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
export function acceptedDraft(row: SlashRow): string {
  return row.args === "none" ? `/${row.name}` : `/${row.name} `;
}

/**
 * The argument hint a row shows beside its name, or `null` when it takes none.
 *
 * Claude Code's `argument-hint` (R5 §1.2), rendered as the row's own trailing
 * text rather than as a tooltip: a hint you have to hover for is a hint you
 * read once.
 */
export function argumentHint(row: SlashRow): string | null {
  if (row.argHint === null) {
    return null;
  }
  return row.args === "optionalRest" ? `[${row.argHint}]` : `<${row.argHint}>`;
}
