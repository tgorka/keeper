/**
 * What a chat does with a slash command Rust resolved (Story 100.5, R385).
 *
 * `keeper_core::chat::commands` decides what a line means; this file only runs
 * the acts it names, each through an existing IPC — `roomSetTopic`,
 * `roomInvite` — and says what happened where the person is looking. `/leave` asks first: the host opens the confirmation and
 * leaves nothing on its own. A failure is a sentence beside the composer and a
 * line in `keeper.log` (`reportFrontendError`, on top of the core's own WARN),
 * never only one of them (R381).
 */
import { reportFrontendError } from "@/lib/crash-report";

/** A command as the composer hands it over: the registry name and its argument. */
export interface ChatCommandInvocation {
  /** The registry name, never the alias that was typed. */
  name: string;
  /** What followed it, trimmed, or `null`. */
  args: string | null;
}

/** What the composer shows after a command ran — or did not. */
export interface ChatCommandOutcome {
  /** `done` is a quiet status line; `failed` is an alert and keeps the draft. */
  tone: "done" | "failed";
  message: string;
}

export const chatTopicChanged = (topic: string) => `The topic is now "${topic}".`;
export const CHAT_TOPIC_FAILED =
  "keeper couldn't change the topic: the server refused it or could not be reached. Nothing changed.";
export const chatInvited = (user: string) => `Invited ${user}.`;
export const chatInviteFailed = (user: string) =>
  `keeper couldn't invite ${user}: the server refused it or could not be reached.`;
export const chatCommandUnknown = (name: string) =>
  `keeper has no way to run /${name} here, so nothing happened.`;

/**
 * The room's command runner. Resolves to the outcome to show, or `null` when
 * the act has its own surface — `/leave`'s confirmation.
 */
export function chatCommandHost(deps: {
  setTopic: (topic: string) => Promise<void>;
  invite: (userId: string) => Promise<void>;
  askLeave: () => void;
}): (command: ChatCommandInvocation) => Promise<ChatCommandOutcome | null> {
  const attempt = async (
    command: ChatCommandInvocation,
    act: () => Promise<unknown>,
    done: string,
    failed: string,
  ): Promise<ChatCommandOutcome> => {
    try {
      await act();
      return { tone: "done", message: done };
    } catch (error) {
      reportFrontendError(`chat-command:${command.name}`, error, null);
      return { tone: "failed", message: failed };
    }
  };
  return async (command) => {
    const args = command.args ?? "";
    switch (command.name) {
      case "topic":
        return attempt(
          command,
          () => deps.setTopic(args),
          chatTopicChanged(args),
          CHAT_TOPIC_FAILED,
        );
      case "invite":
        return attempt(command, () => deps.invite(args), chatInvited(args), chatInviteFailed(args));
      case "leave":
        deps.askLeave();
        return null;
      default:
        // Rust only names commands this table runs; a new registry entry with
        // no row here says so instead of doing nothing.
        reportFrontendError("chat-command", new Error(`no host for /${command.name}`), null);
        return { tone: "failed", message: chatCommandUnknown(command.name) };
    }
  };
}
