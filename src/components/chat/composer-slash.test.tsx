/**
 * The messenger composer's slash path (Story 100.5, R385).
 *
 * What a `/` line means is `keeper_core::chat::commands`, proven in
 * `keeper-core/tests/chat_commands.rs`. These tests inject the resolver with
 * canned verdicts and assert the composer's obligations: the keyboard model of
 * the shared menu, that `text` and `emote` leave through the right dispatcher,
 * that a `refusal` sends nothing and keeps the draft, that a command leaves
 * through `onCommand` and its outcome is shown — and that the messenger's own
 * keys (Enter, ⇧Enter, ↑ in an empty field, Esc on a reply) keep their meaning
 * whenever the menu is closed.
 */
import { act, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { ChatCommandOutcome } from "@/components/chat/chat-commands";
import {
  COMPOSER_COMMAND_UNRESOLVED,
  Composer,
  type SendContext,
} from "@/components/chat/composer";
import { SLASH_PREVIEW_FAILED } from "@/components/slash-menu/slash-menu";
import { SLASH_HELP_LABEL, SLASH_MENU_LABEL } from "@/components/slash-menu/slash-menu-list";
import type { ChatCommandPreviewVm, ChatCommandRowVm } from "@/lib/ipc/client";
import { attachmentsStore } from "@/lib/stores/attachments";
import type { PendingContext } from "@/lib/stores/composer";
import { draftsStore } from "@/lib/stores/drafts";

vi.mock("@tauri-apps/plugin-dialog", () => ({ open: vi.fn() }));
vi.mock("@/lib/crash-report", () => ({ reportFrontendError: vi.fn() }));
vi.mock("@/lib/ipc/client", async (importOriginal) => {
  const actual = await importOriginal<typeof import("@/lib/ipc/client")>();
  return {
    ...actual,
    loadDraft: vi.fn(async () => null),
    saveDraft: vi.fn(async () => {}),
    clearDraft: vi.fn(async () => {}),
    loadRemoteDraft: vi.fn(async () => null),
    mirrorDraft: vi.fn(async () => {}),
    clearDraftMirror: vi.fn(async () => {}),
    telemetryCapture: vi.fn(async () => {}),
  };
});

import { reportFrontendError } from "@/lib/crash-report";
import {
  clearDraft,
  clearDraftMirror,
  loadDraft,
  mirrorDraft,
  saveDraft,
  telemetryCapture,
} from "@/lib/ipc/client";

beforeEach(() => {
  attachmentsStore.getState().clear();
  draftsStore.getState().clear();
  vi.mocked(reportFrontendError).mockClear();
  // No stored draft and no mirror, unless a test backs them with its own store.
  vi.mocked(loadDraft).mockImplementation(async () => null);
  vi.mocked(saveDraft).mockImplementation(async () => {});
  vi.mocked(clearDraft).mockImplementation(async () => {});
  vi.mocked(mirrorDraft).mockImplementation(async () => {});
  vi.mocked(clearDraftMirror).mockImplementation(async () => {});
});
afterEach(() => {
  attachmentsStore.getState().clear();
});

/** Where a new message in the mounted room goes. */
const NEW_MESSAGE: SendContext = { accountId: "acct", roomId: "!room:example.org", target: null };

const ESCAPE_HINT = "To send a message that starts with a slash, double it: //etc sends /etc.";

function row(name: string, overrides: Partial<ChatCommandRowVm> = {}): ChatCommandRowVm {
  return {
    name,
    aliases: [],
    description: `The ${name} command.`,
    args: "none",
    argHint: null,
    available: true,
    reason: null,
    ...overrides,
  };
}

const TOPIC = row("topic", { args: "required", argHint: "the new topic" });
const INVITE = row("invite", { args: "required", argHint: "@name:server" });
const HELP = row("help", { aliases: ["h", "commands"] });

function preview(draft: string, overrides: Partial<ChatCommandPreviewVm> = {}) {
  return {
    draft,
    verdict: { kind: "text", text: draft },
    rows: [],
    escapeHint: ESCAPE_HINT,
    ...overrides,
  } satisfies ChatCommandPreviewVm;
}

/** The composer over a table of canned answers, keyed by the exact draft. */
function mount(
  answers: Record<string, ChatCommandPreviewVm>,
  options: {
    pending?: PendingContext | null;
    outcome?: ChatCommandOutcome | null;
    reject?: boolean;
    /** Answers instead of the table — a test that needs to hold an answer back. */
    resolver?: (draft: string) => Promise<ChatCommandPreviewVm>;
    onCommand?: (command: {
      name: string;
      args: string | null;
    }) => Promise<ChatCommandOutcome | null>;
  } = {},
) {
  const onSend = vi.fn(async (_body: string, _context: SendContext, _line?: string) => {});
  const onSendEmote = vi.fn(async (_body: string, _context: SendContext, _line: string) => {});
  const onCommand = vi.fn(
    options.onCommand ??
      (async (_command: { name: string; args: string | null }) =>
        options.outcome === undefined ? null : options.outcome),
  );
  const onEmptyArrowUp = vi.fn();
  const onCancelPending = vi.fn(() => null);
  const resolveCommand = vi.fn(async (draft: string, _replying: boolean) => {
    if (options.reject === true) {
      throw new Error("shell gone");
    }
    if (options.resolver !== undefined) {
      return await options.resolver(draft);
    }
    return answers[draft] ?? preview(draft);
  });
  const view = render(
    <Composer
      accountId="acct"
      roomId="!room:example.org"
      onSend={onSend}
      onSendEmote={onSendEmote}
      onCommand={onCommand}
      resolveCommand={resolveCommand}
      pending={options.pending ?? null}
      onEmptyArrowUp={onEmptyArrowUp}
      onCancelPending={onCancelPending}
    />,
  );
  const field = screen.getByLabelText<HTMLTextAreaElement>("Message");
  return {
    unmount: view.unmount,
    onSend,
    onSendEmote,
    onCommand,
    onEmptyArrowUp,
    onCancelPending,
    resolveCommand,
    field,
  };
}

function type(field: HTMLElement, value: string) {
  fireEvent.change(field, { target: { value } });
}

/** One keydown; `true` when the composer claimed it. */
function key(field: HTMLElement, init: { key?: string; shiftKey?: boolean } = {}): boolean {
  return !fireEvent.keyDown(field, { key: init.key ?? "Enter", ...init });
}

const MENU = {
  "/": preview("/", { rows: [TOPIC, INVITE, HELP] }),
  "/t": preview("/t", {
    verdict: { kind: "refusal", message: "/topic needs the new topic." },
    rows: [TOPIC],
  }),
};

it("runs a session slash command without an emote transport and never sends it as prose", async () => {
  const onCommand = vi.fn(async () => null);
  const onSend = vi.fn(async () => {});
  render(
    <Composer
      transport="session"
      accountId="alpha"
      roomId="session-command"
      onSend={onSend}
      onCommand={onCommand}
      resolveCommand={async (draft) =>
        preview(draft, {
          rows: [row("clean")],
          verdict: { kind: "command", name: "clean", args: null },
        })
      }
    />,
  );
  const field = screen.getByLabelText<HTMLTextAreaElement>("Message");
  type(field, "/clean");
  await screen.findByLabelText(SLASH_MENU_LABEL);
  key(field);
  await waitFor(() => expect(onCommand).toHaveBeenCalledWith({ name: "clean", args: null }));
  expect(onSend).not.toHaveBeenCalled();
  await waitFor(() => expect(field).toHaveValue(""));
});

describe("session slash commands with queued attachments", () => {
  const file = {
    id: "queued",
    kind: "path" as const,
    path: "/tmp/report.txt",
    filename: "report.txt",
  };

  function mountCaption(answer: ChatCommandPreviewVm, session = true) {
    const onSend = vi.fn(async () => {});
    const onSendAttachments = vi.fn(async () => {});
    const onCommand = vi.fn(async () => ({ tone: "done" as const, message: "Stop requested." }));
    const resolveCommand = vi.fn(async () => answer);
    render(
      <Composer
        transport={session ? "session" : "matrix"}
        accountId="alpha"
        roomId="caption-command"
        onSend={onSend}
        onSendEmote={session ? undefined : vi.fn(async () => {})}
        onSendAttachments={onSendAttachments}
        onCommand={onCommand}
        resolveCommand={resolveCommand}
      />,
    );
    act(() => attachmentsStore.getState().add(file));
    return {
      field: screen.getByLabelText<HTMLTextAreaElement>("Message"),
      onSend,
      onSendAttachments,
      onCommand,
      resolveCommand,
    };
  }

  it("runs stop without uploading or consuming the queued file", async () => {
    const { field, onSend, onSendAttachments, onCommand } = mountCaption(
      preview("/stop", { verdict: { kind: "command", name: "stop", args: null } }),
    );
    type(field, "/stop");
    key(field);
    await waitFor(() => expect(field).toHaveValue(""));
    expect(onCommand).toHaveBeenCalledWith({ name: "stop", args: null });
    expect(onSendAttachments).not.toHaveBeenCalled();
    expect(onSend).not.toHaveBeenCalled();
    expect(attachmentsStore.getState().pending).toEqual([file]);
    expect(screen.getByText("report.txt")).toBeInTheDocument();
  });

  it.each([
    "/unknown",
    "/model unavailable",
  ])("refuses %s without sending its queued file", async (line) => {
    const { field, onSend, onSendAttachments, onCommand } = mountCaption(
      preview(line, { verdict: { kind: "refusal", message: "Command unavailable." } }),
    );
    type(field, line);
    key(field);
    expect(await screen.findByText("Command unavailable.")).toBeInTheDocument();
    expect(onSendAttachments).not.toHaveBeenCalled();
    expect(onSend).not.toHaveBeenCalled();
    expect(onCommand).not.toHaveBeenCalled();
    expect(field).toHaveValue(line);
    expect(attachmentsStore.getState().pending).toEqual([file]);
  });

  it.each([
    "/stop",
    "/unknown",
    "/model unavailable",
  ])("sends escaped %s as the literal attachment caption", async (caption) => {
    const line = `/${caption}`;
    const { field, onSend, onSendAttachments, onCommand } = mountCaption(
      preview(line, { verdict: { kind: "text", text: caption } }),
    );
    type(field, line);
    key(field);
    await waitFor(() => expect(onSendAttachments).toHaveBeenCalledWith([file], caption));
    expect(onSend).not.toHaveBeenCalled();
    expect(onCommand).not.toHaveBeenCalled();
    await waitFor(() => expect(field).toHaveValue(""));
    expect(attachmentsStore.getState().pending).toEqual([]);
  });

  it("preserves messenger slash captions without command resolution", async () => {
    const { field, onSend, onSendAttachments, onCommand, resolveCommand } = mountCaption(
      preview("/stop", { verdict: { kind: "command", name: "stop", args: null } }),
      false,
    );
    type(field, "/stop");
    key(field);
    await waitFor(() => expect(onSendAttachments).toHaveBeenCalledWith([file], "/stop"));
    expect(onSend).not.toHaveBeenCalled();
    expect(onCommand).not.toHaveBeenCalled();
    expect(resolveCommand).not.toHaveBeenCalled();
  });
});

describe("Composer slash menu", () => {
  it("opens over a leading slash with Rust's rows, and asks nothing about prose", async () => {
    const { field, resolveCommand } = mount(MENU);
    type(field, "hello /there");
    expect(screen.queryByLabelText(SLASH_MENU_LABEL)).not.toBeInTheDocument();
    expect(resolveCommand).not.toHaveBeenCalled();
    type(field, "/");
    const menu = await screen.findByLabelText(SLASH_MENU_LABEL);
    const names = within(menu)
      .getAllByRole("option")
      .map((option) => option.textContent ?? "");
    expect(names).toHaveLength(3);
    expect(names[0]).toContain("/topic");
    expect(names[0]).toContain("<the new topic>");
    expect(names[2]).toContain("also /h, /commands");
    expect(screen.getByText(ESCAPE_HINT)).toBeInTheDocument();
  });

  it("moves the highlight with the arrows, wrapping", async () => {
    const { field } = mount(MENU);
    type(field, "/");
    const menu = await screen.findByLabelText(SLASH_MENU_LABEL);
    const current = () =>
      within(menu)
        .getAllByRole("option")
        .findIndex((option) => option.getAttribute("aria-selected") === "true");
    expect(current()).toBe(0);
    expect(key(field, { key: "ArrowDown" })).toBe(true);
    expect(current()).toBe(1);
    expect(key(field, { key: "ArrowUp" })).toBe(true);
    expect(key(field, { key: "ArrowUp" })).toBe(true);
    expect(current()).toBe(2);
  });

  it("completes on Enter, and a second Enter runs the command", async () => {
    const { field, onCommand, onSend } = mount(
      {
        ...MENU,
        "/help": preview("/help", {
          verdict: { kind: "command", name: "help", args: null },
          rows: [HELP],
        }),
        "/invite": preview("/invite", { rows: [INVITE] }),
        "/invite @bob:example.org": preview("/invite @bob:example.org", {
          verdict: { kind: "command", name: "invite", args: "@bob:example.org" },
          rows: [INVITE],
        }),
      },
      { outcome: { tone: "done", message: "Invited @bob:example.org." } },
    );
    type(field, "/");
    await screen.findByLabelText(SLASH_MENU_LABEL);
    key(field, { key: "ArrowDown" });
    expect(key(field)).toBe(true);
    // An argument follows, so completion leaves the caret where it goes.
    await waitFor(() => expect(field.value).toBe("/invite "));
    expect(onCommand).not.toHaveBeenCalled();
    type(field, "/invite @bob:example.org");
    await screen.findByLabelText(SLASH_MENU_LABEL);
    key(field);
    await waitFor(() =>
      expect(onCommand).toHaveBeenCalledWith({ name: "invite", args: "@bob:example.org" }),
    );
    expect(await screen.findByRole("status")).toHaveTextContent("Invited @bob:example.org.");
    await waitFor(() => expect(field.value).toBe(""));
    expect(onSend).not.toHaveBeenCalled();
  });

  it("accepts with Tab as well", async () => {
    const { field } = mount(MENU);
    type(field, "/");
    await screen.findByLabelText(SLASH_MENU_LABEL);
    expect(key(field, { key: "Tab" })).toBe(true);
    await waitFor(() => expect(field.value).toBe("/topic "));
  });

  // DW-1430 (R391): the menu's use is counted as 100.1's closed surfaces, once
  // per draft session each; nothing about the command, the draft or the room.
  it("counts the menu opened and a row chosen once per draft session", async () => {
    vi.mocked(telemetryCapture).mockClear();
    const { field, onCommand } = mount(
      {
        ...MENU,
        "/topic Friday": preview("/topic Friday", {
          verdict: { kind: "command", name: "topic", args: "Friday" },
          rows: [TOPIC],
        }),
      },
      { outcome: { tone: "done", message: "The topic is now Friday." } },
    );
    const surfaces = () => vi.mocked(telemetryCapture).mock.calls.map(([event]) => event.surface);
    type(field, "/");
    await screen.findByLabelText(SLASH_MENU_LABEL);
    await waitFor(() => expect(surfaces()).toEqual(["slashMenuOpened"]));
    type(field, "/t");
    await screen.findByLabelText(SLASH_MENU_LABEL);
    expect(key(field, { key: "Tab" })).toBe(true);
    await waitFor(() => expect(field.value).toBe("/topic "));
    // The menu closes and opens again within the session: still one opening.
    type(field, "/topic Friday");
    await screen.findByLabelText(SLASH_MENU_LABEL);
    key(field);
    await waitFor(() => expect(onCommand).toHaveBeenCalledWith({ name: "topic", args: "Friday" }));
    expect(surfaces()).toEqual(["slashMenuOpened", "slashMenuChosen"]);
    // The line ran and the field emptied: the next `/` is a new session.
    await waitFor(() => expect(field.value).toBe(""));
    type(field, "/");
    await screen.findByLabelText(SLASH_MENU_LABEL);
    await waitFor(() =>
      expect(surfaces()).toEqual(["slashMenuOpened", "slashMenuChosen", "slashMenuOpened"]),
    );
    for (const [event] of vi.mocked(telemetryCapture).mock.calls) {
      expect(event).toEqual({ kind: "interaction", durationMs: null, surface: event.surface });
    }
  });

  it("stays dismissed for the token Escape dismissed, and returns for the next", async () => {
    const { field } = mount(MENU);
    type(field, "/");
    await screen.findByLabelText(SLASH_MENU_LABEL);
    expect(key(field, { key: "Escape" })).toBe(true);
    await waitFor(() => expect(screen.queryByLabelText(SLASH_MENU_LABEL)).not.toBeInTheDocument());
    type(field, "/t");
    expect(await screen.findByLabelText(SLASH_MENU_LABEL)).toBeInTheDocument();
  });

  it("leaves ⇧Enter and every unclaimed key to the field while open", async () => {
    const { field } = mount(MENU);
    type(field, "/");
    await screen.findByLabelText(SLASH_MENU_LABEL);
    expect(key(field, { key: "Enter", shiftKey: true })).toBe(false);
    expect(key(field, { key: "a" })).toBe(false);
    expect(screen.getByLabelText(SLASH_MENU_LABEL)).toBeInTheDocument();
  });
});

describe("Composer slash verdicts", () => {
  it("sends text verbatim through onSend — the escape arrives unwrapped", async () => {
    const { field, onSend, onSendEmote } = mount({
      "//etc": preview("//etc", { verdict: { kind: "text", text: "/etc" } }),
    });
    type(field, "//etc");
    expect(screen.queryByLabelText(SLASH_MENU_LABEL)).not.toBeInTheDocument();
    key(field);
    // The line as typed rides along, so an Undo puts back `//etc`, not `/etc` (R393).
    await waitFor(() => expect(onSend).toHaveBeenCalledWith("/etc", NEW_MESSAGE, "//etc"));
    expect(onSendEmote).not.toHaveBeenCalled();
  });

  it("sends an action through onSendEmote and never as text", async () => {
    const { field, onSend, onSendEmote } = mount({
      "/me waves": preview("/me waves", { verdict: { kind: "emote", text: "waves" } }),
    });
    type(field, "/me waves");
    key(field);
    await waitFor(() =>
      expect(onSendEmote).toHaveBeenCalledWith("waves", NEW_MESSAGE, "/me waves"),
    );
    expect(onSend).not.toHaveBeenCalled();
    await waitFor(() => expect(field.value).toBe(""));
  });

  it("shows a refusal, sends nothing, and keeps the draft", async () => {
    const message = "keeper has no /tpoic command. The closest is /topic.";
    const { field, onSend, onSendEmote, onCommand } = mount({
      "/tpoic x": preview("/tpoic x", { verdict: { kind: "refusal", message } }),
    });
    type(field, "/tpoic x");
    key(field);
    expect(await screen.findByRole("alert")).toHaveTextContent(message);
    expect(onSend).not.toHaveBeenCalled();
    expect(onSendEmote).not.toHaveBeenCalled();
    expect(onCommand).not.toHaveBeenCalled();
    expect(field.value).toBe("/tpoic x");
    // A refusal is the answer, not a failure: nothing for `keeper.log` (R393).
    expect(reportFrontendError).not.toHaveBeenCalled();
    // Editing the draft retires the sentence.
    type(field, "/tpoic");
    await waitFor(() => expect(screen.queryByRole("alert")).not.toBeInTheDocument());
  });

  it("keeps the draft and says so when the command failed", async () => {
    const { field } = mount(
      {
        "/topic Friday": preview("/topic Friday", {
          verdict: { kind: "command", name: "topic", args: "Friday" },
          rows: [TOPIC],
        }),
      },
      { outcome: { tone: "failed", message: "keeper couldn't change the topic." } },
    );
    type(field, "/topic Friday");
    key(field);
    expect(await screen.findByRole("alert")).toHaveTextContent("keeper couldn't change the topic.");
    expect(field.value).toBe("/topic Friday");
  });

  it("sends nothing when the line could not be resolved, and says why", async () => {
    const { field, onSend } = mount({}, { reject: true });
    type(field, "/me waves");
    key(field);
    // One line under the field — the submit's sentence, not a second one about
    // the menu — and one report for the draft session, not one per ask.
    await waitFor(() =>
      expect(screen.getByRole("alert")).toHaveTextContent(COMPOSER_COMMAND_UNRESOLVED),
    );
    expect(onSend).not.toHaveBeenCalled();
    expect(reportFrontendError).toHaveBeenCalledTimes(1);
  });

  it("answers /help itself with every row a bare slash lists, and the escape", async () => {
    const { field, onCommand } = mount({
      ...MENU,
      "/help": preview("/help", {
        verdict: { kind: "command", name: "help", args: null },
        rows: [HELP],
      }),
    });
    type(field, "/help");
    await screen.findByLabelText(SLASH_MENU_LABEL);
    key(field);
    const panel = await screen.findByLabelText(SLASH_HELP_LABEL);
    expect(panel).toHaveTextContent("/topic <the new topic>");
    expect(panel).toHaveTextContent("/invite");
    expect(panel).toHaveTextContent(ESCAPE_HINT);
    expect(onCommand).not.toHaveBeenCalled();
    await waitFor(() => expect(field.value).toBe(""));
  });

  it("tells Rust a reply is pending", async () => {
    const { field, resolveCommand } = mount(MENU, {
      pending: { mode: "reply", targetKey: "k1", sender: "Bob", bodyPreview: "hi" },
    });
    type(field, "/");
    await screen.findByLabelText(SLASH_MENU_LABEL);
    expect(resolveCommand).toHaveBeenCalledWith("/", true);
  });
});

describe("Composer keys with the menu closed", () => {
  it("edits the last message on ↑ in an empty field", () => {
    const { field, onEmptyArrowUp } = mount(MENU);
    expect(key(field, { key: "ArrowUp" })).toBe(true);
    expect(onEmptyArrowUp).toHaveBeenCalledTimes(1);
  });

  it("cancels a pending reply on Escape once the menu is dismissed", async () => {
    const { field, onCancelPending } = mount(MENU, {
      pending: { mode: "reply", targetKey: "k1", sender: "Bob", bodyPreview: "hi" },
    });
    type(field, "/");
    await screen.findByLabelText(SLASH_MENU_LABEL);
    key(field, { key: "Escape" });
    expect(onCancelPending).not.toHaveBeenCalled();
    key(field, { key: "Escape" });
    expect(onCancelPending).toHaveBeenCalledTimes(1);
  });

  /** An edit's text is that message's new body: no menu, no command. */
  it("opens no menu while editing and sends the line as the edit", async () => {
    const { field, onSend, resolveCommand } = mount(MENU, {
      pending: { mode: "edit", targetKey: "k2" },
    });
    type(field, "/me waves");
    key(field);
    await waitFor(() =>
      expect(onSend).toHaveBeenCalledWith("/me waves", {
        ...NEW_MESSAGE,
        target: { mode: "edit", targetKey: "k2" },
      }),
    );
    expect(resolveCommand).not.toHaveBeenCalled();
    expect(screen.queryByLabelText(SLASH_MENU_LABEL)).not.toBeInTheDocument();
  });
});

/** Answers held back until the test lets them go, in the order they were asked. */
function heldAnswers() {
  const held: Array<{ draft: string; answer: (vm: ChatCommandPreviewVm) => void }> = [];
  return {
    resolver: (draft: string) =>
      new Promise<ChatCommandPreviewVm>((answer) => {
        held.push({ draft, answer });
      }),
    /** Answer every ask so far: `verdicts[draft]`, or prose. */
    release: async (verdicts: Record<string, ChatCommandPreviewVm>) => {
      await act(async () => {
        for (const ask of held.splice(0)) {
          ask.answer(verdicts[ask.draft] ?? preview(ask.draft));
        }
      });
    },
  };
}

describe("Composer slash lifetimes (R393)", () => {
  const NEW_TRIP = {
    "/new Trip": preview("/new Trip", {
      verdict: { kind: "command", name: "new", args: "Trip" },
    }),
  };

  it("runs a line once however often Enter or Send is pressed while Rust is asked", async () => {
    const asks = heldAnswers();
    const { field, onCommand } = mount(
      {},
      { resolver: asks.resolver, outcome: { tone: "done", message: "Asked your agent." } },
    );
    type(field, "/new Trip");
    key(field);
    key(field);
    fireEvent.click(screen.getByRole("button", { name: "Send message" }));
    await asks.release(NEW_TRIP);
    await waitFor(() => expect(onCommand).toHaveBeenCalledTimes(1));
    await act(async () => {});
    expect(onCommand).toHaveBeenCalledTimes(1);
    // Released: the next line runs.
    await waitFor(() => expect(field.value).toBe(""));
    type(field, "/new Trip");
    key(field);
    await asks.release(NEW_TRIP);
    await waitFor(() => expect(onCommand).toHaveBeenCalledTimes(2));
  });

  it("keeps what was typed while a slow action was being resolved", async () => {
    const asks = heldAnswers();
    const { field, onSendEmote } = mount({}, { resolver: asks.resolver });
    type(field, "/me waves");
    key(field);
    type(field, "and then this");
    await asks.release({
      "/me waves": preview("/me waves", { verdict: { kind: "emote", text: "waves" } }),
    });
    await waitFor(() =>
      expect(onSendEmote).toHaveBeenCalledWith("waves", NEW_MESSAGE, "/me waves"),
    );
    await act(async () => {});
    expect(field.value).toBe("and then this");
  });

  it("keeps what was typed while a slow command ran", async () => {
    const asks = heldAnswers();
    const { field, onCommand } = mount(
      {},
      { resolver: asks.resolver, outcome: { tone: "done", message: "Asked your agent." } },
    );
    type(field, "/new Trip");
    key(field);
    type(field, "/new Other");
    await asks.release(NEW_TRIP);
    await waitFor(() => expect(onCommand).toHaveBeenCalledWith({ name: "new", args: "Trip" }));
    await act(async () => {});
    expect(field.value).toBe("/new Other");
  });

  it("keeps the next message saved and mirrored when the slow line before it is sent (R411)", async () => {
    // The room's stored draft and its cross-device mirror, as the shell keeps them.
    let stored: string | null = null;
    let mirrored: string | null = null;
    vi.mocked(saveDraft).mockImplementation(async (_a, _r, body) => {
      stored = body;
    });
    vi.mocked(clearDraft).mockImplementation(async () => {
      stored = null;
    });
    vi.mocked(loadDraft).mockImplementation(async () => stored);
    vi.mocked(mirrorDraft).mockImplementation(async (_a, _r, body) => {
      mirrored = body;
    });
    vi.mocked(clearDraftMirror).mockImplementation(async () => {
      mirrored = null;
    });
    const asks = heldAnswers();
    const first = mount({}, { resolver: asks.resolver });
    type(first.field, "/me waves");
    key(first.field);
    // The next message, typed before its own save came due.
    type(first.field, "the next message");
    await asks.release({
      "/me waves": preview("/me waves", { verdict: { kind: "emote", text: "waves" } }),
    });
    await waitFor(() => expect(first.onSendEmote).toHaveBeenCalledTimes(1));
    await act(async () => {});
    // Away to another room, and back.
    first.unmount();
    const second = mount({});
    await waitFor(() => expect(second.field.value).toBe("the next message"));
    expect(mirrored).toBe("the next message");
  });

  it("sends nothing for a text answer that arrives after its room was left", async () => {
    const asks = heldAnswers();
    const { field, onSend, unmount } = mount({}, { resolver: asks.resolver });
    type(field, "/plain hi");
    key(field);
    unmount();
    await asks.release({
      "/plain hi": preview("/plain hi", { verdict: { kind: "text", text: "hi" } }),
    });
    expect(onSend).not.toHaveBeenCalled();
  });

  it("drops an answer that arrives after its room was left", async () => {
    const asks = heldAnswers();
    const { field, onCommand, unmount } = mount({}, { resolver: asks.resolver });
    type(field, "/leave");
    key(field);
    unmount();
    await asks.release({
      "/leave": preview("/leave", { verdict: { kind: "command", name: "leave", args: null } }),
    });
    expect(onCommand).not.toHaveBeenCalled();
  });

  it("forgets an Escape once the token changes — back to the old one, or a new draft", async () => {
    const { field } = mount(MENU);
    const gone = () =>
      waitFor(() => expect(screen.queryByLabelText(SLASH_MENU_LABEL)).not.toBeInTheDocument());
    type(field, "/");
    await screen.findByLabelText(SLASH_MENU_LABEL);
    key(field, { key: "Escape" });
    await gone();
    type(field, "/t");
    await screen.findByLabelText(SLASH_MENU_LABEL);
    // Back to the token Escape dismissed: it was retired by the change.
    type(field, "/");
    expect(await screen.findByLabelText(SLASH_MENU_LABEL)).toBeInTheDocument();
    key(field, { key: "Escape" });
    await gone();
    // Leaving command mode retires it too: the next draft starts fresh.
    type(field, "");
    type(field, "/");
    expect(await screen.findByLabelText(SLASH_MENU_LABEL)).toBeInTheDocument();
  });

  it("points the field at the list and at the option the arrows highlight", async () => {
    const { field } = mount(MENU);
    expect(field).toHaveAttribute("aria-autocomplete", "list");
    expect(field).toHaveAttribute("aria-expanded", "false");
    expect(field).not.toHaveAttribute("aria-activedescendant");
    type(field, "/");
    const list = await screen.findByLabelText(SLASH_MENU_LABEL);
    const options = within(list).getAllByRole("option");
    await waitFor(() => expect(field).toHaveAttribute("aria-activedescendant", options[0]?.id));
    expect(list).toHaveAttribute("role", "listbox");
    expect(field).toHaveAttribute("aria-controls", list.id);
    expect(field).toHaveAttribute("aria-expanded", "true");
    key(field, { key: "ArrowDown" });
    await waitFor(() => expect(field).toHaveAttribute("aria-activedescendant", options[1]?.id));
    expect(options[1]).toHaveAttribute("aria-selected", "true");
    key(field, { key: "ArrowUp" });
    key(field, { key: "ArrowUp" });
    await waitFor(() => expect(field).toHaveAttribute("aria-activedescendant", options[2]?.id));
    key(field, { key: "Escape" });
    await waitFor(() => expect(field).not.toHaveAttribute("aria-activedescendant"));
    expect(field).not.toHaveAttribute("aria-controls");
    expect(field).toHaveAttribute("aria-expanded", "false");
  });

  it("says under the field that the menu could not be asked, and reports once per draft", async () => {
    const { field, resolveCommand } = mount({}, { reject: true });
    type(field, "/");
    expect(await screen.findByRole("alert")).toHaveTextContent(SLASH_PREVIEW_FAILED);
    type(field, "/t");
    type(field, "/to");
    await waitFor(() => expect(resolveCommand).toHaveBeenCalledTimes(3));
    await act(async () => {});
    expect(reportFrontendError).toHaveBeenCalledTimes(1);
    // Prose ends the session, and the line with it; the next draft reports anew.
    type(field, "hello");
    await waitFor(() => expect(screen.queryByRole("alert")).not.toBeInTheDocument());
    type(field, "/");
    await waitFor(() => expect(reportFrontendError).toHaveBeenCalledTimes(2));
  });
});
