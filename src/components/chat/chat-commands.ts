/**
 * What a chat or agent room does with a slash command Rust resolved (Story
 * 100.5, R385).
 *
 * `keeper_core::chat::commands` decides what a line means; this file only runs
 * the acts it names, each through an existing IPC — `roomSetTopic`,
 * `roomInvite`, `agentConversationNew`, `agentModelCommand` (whose answer is
 * Rust's own sentence) — and says what happened where the
 * person is looking. `/leave` asks first: the host opens the confirmation and
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
export const CHAT_NEW_ASKED =
  "Asked your agent for a new conversation. It appears in the list once the agent has made it.";
export const CHAT_NEW_FAILED =
  "keeper couldn't ask your agent for a new conversation, so nothing was sent.";
export const CHAT_MODEL_FAILED =
  "keeper couldn't read or change your agent's model, so nothing changed.";
export const chatCommandUnknown = (name: string) =>
  `keeper has no way to run /${name} here, so nothing happened.`;

/** Rust's sentence from a rejected command, when it carries one. */
function refusalOf(error: unknown): string | null {
  // keeper's IPC refusal is a plain object; a thrown `Error` is the bridge's.
  if (error instanceof Error) {
    return null;
  }
  if (typeof error === "object" && error !== null && "message" in error) {
    const { message } = error as { message: unknown };
    if (typeof message === "string" && message.trim() !== "") {
      return message;
    }
  }
  return null;
}

/**
 * The room's command runner. Resolves to the outcome to show, or `null` when
 * the act has its own surface — `/leave`'s confirmation.
 */
export function chatCommandHost(deps: {
  setTopic: (topic: string) => Promise<void>;
  invite: (userId: string) => Promise<void>;
  newConversation: (title: string | null) => Promise<unknown>;
  /** `/model`: Rust's sentence for what the agent runs on, or now runs on. */
  model: (args: string | null) => Promise<string>;
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
      case "new":
        return attempt(
          command,
          () => deps.newConversation(command.args),
          CHAT_NEW_ASKED,
          CHAT_NEW_FAILED,
        );
      case "model":
        try {
          return { tone: "done", message: await deps.model(command.args) };
        } catch (error) {
          // Rust's refusal is its own sentence (the home not here, a bot the
          // agent is not offered, the file changed meanwhile), and a WARN line.
          reportFrontendError("chat-command:model", error, null);
          return { tone: "failed", message: refusalOf(error) ?? CHAT_MODEL_FAILED };
        }
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
