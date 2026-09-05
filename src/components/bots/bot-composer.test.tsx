/**
 * The Bots composer's keyboard contract and its slash path (Epic 61, Stories
 * 61.4 and 61.9).
 *
 * # What is asserted here, and what is asserted in Rust
 *
 * The composer decides nothing about what a draft means: which command a
 * spelling is, whether it may run, what to say when it may not, and whether a
 * leading slash was escaped are all `keeper_core::bots::commands`, proven in
 * `keeper-core/tests/bots_commands.rs`. Re-deciding any of it here would be a
 * second matcher, and the one that disagrees is always the one a person is
 * looking at.
 *
 * So these tests inject the resolver and assert the composer's *obligations*
 * against each of the three verdicts:
 *
 * - `prose` is sent verbatim — which is what makes the escape work, because
 *   `//etc` arrives as the text `/etc` and the composer must not re-touch it;
 * - `command` leaves through `onCommand` and never through `onSend`;
 * - `refusal` is shown, **nothing is sent**, and the draft is kept.
 *
 * Plus the keyboard model, where two of the three rules are refusals and are
 * asserted through `defaultPrevented` rather than through the field's value:
 * jsdom does not implement a textarea's own insertion behaviour, so what the
 * component actually promises is that it does not intercept those chords. A
 * test that checked for a `"\n"` would be testing jsdom.
 */
import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import {
  BOT_COMPOSER_HELP_LABEL,
  BOT_COMPOSER_LABEL,
  BOT_COMPOSER_MENU_LABEL,
  BOT_COMPOSER_PICKER_ABOVE,
  BOT_COMPOSER_SEND_LABEL,
  BOT_COMPOSER_STOP_LABEL,
  BotComposer,
  botComposerNoBot,
} from "@/components/bots/bot-composer";
import type { BotCommandContext } from "@/components/bots/bot-slash-menu";
import type { BotCommandPreviewVm, BotCommandRowVm } from "@/lib/ipc/client";

/** A context in which everything is configured. */
const READY: BotCommandContext = {
  providerKind: "ollama",
  hasProvider: true,
  hasBot: true,
  hasSession: true,
  modelTools: true,
};

const HELP_ROW: BotCommandRowVm = {
  name: "help",
  aliases: ["h", "commands"],
  description: "List every command keeper knows.",
  args: "none",
  argHint: null,
  available: true,
  reason: null,
  warning: null,
};

const HISTORY_ROW: BotCommandRowVm = {
  name: "history",
  aliases: ["sessions"],
  description: "List your conversations, or search them by a word.",
  args: "optionalRest",
  argHint: "a word to search for",
  available: true,
  reason: null,
  warning: null,
};

const ESCAPE_HINT = "To send a message that starts with a slash, double it: //etc sends /etc.";

function preview(overrides: Partial<BotCommandPreviewVm>): BotCommandPreviewVm {
  return {
    draft: "",
    verdict: { kind: "prose", text: "" },
    rows: [],
    note: null,
    escapeHint: ESCAPE_HINT,
    ...overrides,
  };
}

/**
 * A resolver over a table of canned answers, keyed by the exact draft.
 *
 * A table and not a matcher: the point is that the composer obeys whatever
 * arrives, so the answers are stated rather than computed.
 */
function mount(
  answers: Record<string, BotCommandPreviewVm>,
  overrides: {
    streaming?: boolean;
    disabled?: boolean;
    context?: BotCommandContext;
    hostSays?: string;
    pickerPlace?: string;
  } = {},
) {
  const onSend = vi.fn();
  const onStop = vi.fn();
  // `null` is "the host acted"; a sentence is what the host says it did not.
  const onCommand = vi.fn<(command: { name: string; args: string | null }) => string | null>(
    () => overrides.hostSays ?? null,
  );
  const resolveDraft = vi.fn(
    async (draft: string) =>
      answers[draft] ?? preview({ draft, verdict: { kind: "prose", text: draft } }),
  );
  render(
    <BotComposer
      onSend={onSend}
      onStop={onStop}
      onCommand={onCommand}
      commandContext={overrides.context ?? READY}
      streaming={overrides.streaming ?? false}
      disabled={overrides.disabled ?? false}
      resolveDraft={resolveDraft}
      pickerPlace={overrides.pickerPlace}
    />,
  );
  const field = screen.getByLabelText(BOT_COMPOSER_LABEL);
  return { onSend, onStop, onCommand, resolveDraft, field };
}

/** Type a draft, replacing whatever is there. */
function type(field: HTMLElement, value: string) {
  fireEvent.change(field, { target: { value } });
}

/** Dispatch one keydown and report whether the component claimed it. */
function key(
  field: HTMLElement,
  init: { key?: string; shiftKey?: boolean; metaKey?: boolean; ctrlKey?: boolean } = {},
): boolean {
  return !fireEvent.keyDown(field, { key: init.key ?? "Enter", ...init });
}

describe("BotComposer keyboard", () => {
  it("sends on Enter", async () => {
    const { onSend, field } = mount({});
    type(field, "hello");
    expect(key(field)).toBe(true);
    await waitFor(() => expect(onSend).toHaveBeenCalledWith("hello"));
    expect(field).toHaveValue("");
  });

  /**
   * Prose is sent without asking, because no command and no escape can begin
   * with anything but a slash — so the ordinary message, which is nearly every
   * message, spends no round trip. This is also what keeps the pane's own
   * tests able to mock the IPC surface without mocking a resolver.
   */
  it("sends prose without resolving it", async () => {
    const { onSend, resolveDraft, field } = mount({});
    type(field, "hello");
    key(field);
    await waitFor(() => expect(onSend).toHaveBeenCalledWith("hello"));
    expect(resolveDraft).not.toHaveBeenCalled();
  });

  it("leaves Shift+Enter to the textarea", async () => {
    const { onSend, field } = mount({});
    type(field, "hello");
    expect(key(field, { shiftKey: true })).toBe(false);
    await waitFor(() => expect(onSend).not.toHaveBeenCalled());
  });

  /** ⌘Enter is deliberately unbound, and must not be swallowed either. */
  it("leaves the meta and control chords unbound", async () => {
    const { onSend, field } = mount({});
    type(field, "hello");
    expect(key(field, { metaKey: true })).toBe(false);
    expect(key(field, { ctrlKey: true })).toBe(false);
    await waitFor(() => expect(onSend).not.toHaveBeenCalled());
  });

  it("shows Stop instead of Send while an answer arrives", () => {
    mount({}, { streaming: true });
    expect(screen.getByText(BOT_COMPOSER_STOP_LABEL)).toBeInTheDocument();
    expect(screen.queryByText(BOT_COMPOSER_SEND_LABEL)).not.toBeInTheDocument();
  });

  /**
   * The field is never disabled, because `/help` is how somebody with nothing
   * configured finds out what to do. Send is what refuses, and the line says
   * why.
   */
  it("says why it cannot send, and still takes typing", () => {
    const { field } = mount({}, { disabled: true });
    expect(screen.getByText(botComposerNoBot(BOT_COMPOSER_PICKER_ABOVE))).toBeInTheDocument();
    expect(field).not.toBeDisabled();
  });

  /**
   * Story 63.1, FR-411: one sentence, and the place in it is the tier's. The
   * desktop's wording is the literal it always was; a phone, whose picker is
   * a sheet, names the sheet — "above" there points at a back bar.
   */
  it("names where the bot is chosen on this tier, and the Mac wording is unchanged", () => {
    mount({}, { disabled: true });
    expect(screen.getByText("Choose a bot above and this will send to it.")).toBeInTheDocument();
    cleanup();
    mount({}, { disabled: true, pickerPlace: "in the Bot and model sheet" });
    expect(
      screen.getByText("Choose a bot in the Bot and model sheet and this will send to it."),
    ).toBeInTheDocument();
    expect(screen.queryByText(/Choose a bot above/)).not.toBeInTheDocument();
  });
});

describe("BotComposer slash path", () => {
  /**
   * The escape, end to end from the composer's side: Rust answers `prose` with
   * one slash removed, and the composer sends exactly that. `//etc` reaches the
   * model as `/etc`, and never as `//etc`.
   */
  it("sends the escaped text verbatim and opens no menu", async () => {
    const { onSend, onCommand, field } = mount({
      "//etc": preview({ draft: "//etc", verdict: { kind: "prose", text: "/etc" } }),
    });
    type(field, "//etc");
    expect(screen.queryByLabelText(BOT_COMPOSER_MENU_LABEL)).not.toBeInTheDocument();
    key(field);
    await waitFor(() => expect(onSend).toHaveBeenCalledWith("/etc"));
    expect(onCommand).not.toHaveBeenCalled();
  });

  /**
   * The whole reason the story exists: an unknown command is a refusal shown in
   * the pane, and **nothing reaches the model**. The draft is kept, because it
   * is one keystroke from being right.
   */
  it("shows a refusal, sends nothing, and keeps the draft", async () => {
    const message = "keeper has no /histroy command. The closest is /history.";
    const { onSend, onCommand, field } = mount({
      "/histroy": preview({ draft: "/histroy", verdict: { kind: "refusal", message } }),
    });
    type(field, "/histroy");
    key(field);
    expect(await screen.findByRole("alert")).toHaveTextContent(message);
    expect(onSend).not.toHaveBeenCalled();
    expect(onCommand).not.toHaveBeenCalled();
    expect(field).toHaveValue("/histroy");
  });

  /** Editing the draft retires the refusal: it was about the old text. */
  it("retires a refusal when the draft changes", async () => {
    const { field } = mount({
      "/x": preview({ draft: "/x", verdict: { kind: "refusal", message: "No /x." } }),
    });
    type(field, "/x");
    key(field);
    expect(await screen.findByRole("alert")).toBeInTheDocument();
    type(field, "/xy");
    await waitFor(() => expect(screen.queryByRole("alert")).not.toBeInTheDocument());
  });

  it("forwards a resolved command and sends no prose", async () => {
    const { onSend, onCommand, field } = mount({
      "/history invoice": preview({
        draft: "/history invoice",
        verdict: { kind: "command", name: "history", args: "invoice" },
        rows: [HISTORY_ROW],
      }),
    });
    type(field, "/history invoice");
    key(field);
    await waitFor(() =>
      expect(onCommand).toHaveBeenCalledWith({ name: "history", args: "invoice" }),
    );
    expect(onSend).not.toHaveBeenCalled();
    expect(field).toHaveValue("");
  });

  /**
   * A command the host could not act on says so, in the host's own words and
   * in the same place a refusal appears. A command that silently did nothing
   * is the affordance AD-27 forbids, and only the host knows which of its
   * surfaces it can reach.
   */
  it("shows what the host says when the host did not act", async () => {
    const says = "Grants are in Settings → Bots.";
    const { field } = mount(
      {
        "/grant": preview({
          draft: "/grant",
          verdict: { kind: "command", name: "grant", args: null },
        }),
      },
      { hostSays: says },
    );
    type(field, "/grant");
    key(field);
    expect(await screen.findByRole("alert")).toHaveTextContent(says);
  });

  /**
   * A command whose precondition is unmet is *listed with its reason* rather
   * than hidden — hiding it would make the menu shrink as somebody configures
   * the app — and running it refuses instead of acting.
   */
  it("lists an unavailable command with its reason and refuses to run it", async () => {
    const reason = "No bot is chosen, so this has nothing to act on. Choose one above.";
    const unavailable: BotCommandRowVm = {
      ...HISTORY_ROW,
      name: "new",
      args: "none",
      argHint: null,
      available: false,
      reason,
    };
    const { onCommand, onSend, field } = mount(
      {
        "/new": preview({
          draft: "/new",
          verdict: { kind: "refusal", message: `/new — ${reason}` },
          rows: [unavailable],
        }),
      },
      { context: { ...READY, hasBot: false, providerKind: null } },
    );
    type(field, "/new");
    const menu = await screen.findByLabelText(BOT_COMPOSER_MENU_LABEL);
    expect(menu).toHaveTextContent(reason);
    // `/new` is already complete, so Enter runs it rather than completing it.
    key(field);
    expect(await screen.findByRole("alert")).toHaveTextContent(reason);
    expect(onCommand).not.toHaveBeenCalled();
    expect(onSend).not.toHaveBeenCalled();
  });

  /** The Hermes disclosure is shown where it arrives, and nowhere else. */
  it("shows the Hermes sentence only when the preview carries one", async () => {
    const note = "Hermes' own commands stay on Hermes.";
    const answers = {
      "/h": preview({
        draft: "/h",
        verdict: { kind: "command", name: "help", args: null },
        rows: [HELP_ROW],
        note,
      }),
    };
    const { field } = mount(answers);
    type(field, "/h");
    expect(await screen.findByText(note)).toBeInTheDocument();
  });

  it("shows no note for a provider that has nothing to disclose", async () => {
    const { field } = mount({
      "/h": preview({
        draft: "/h",
        verdict: { kind: "command", name: "help", args: null },
        rows: [HELP_ROW],
      }),
    });
    type(field, "/h");
    await screen.findByLabelText(BOT_COMPOSER_MENU_LABEL);
    expect(screen.queryByText(/Hermes/)).not.toBeInTheDocument();
  });
});

describe("BotComposer command menu", () => {
  const MENU = {
    "/h": preview({
      draft: "/h",
      verdict: { kind: "command", name: "help", args: null },
      rows: [HISTORY_ROW, HELP_ROW],
    }),
  };

  it("draws the rows the preview carried, in the order it carried them", async () => {
    const { field } = mount(MENU);
    type(field, "/h");
    const menu = await screen.findByLabelText(BOT_COMPOSER_MENU_LABEL);
    const names = Array.from(menu.querySelectorAll("li")).map((item) => item.textContent ?? "");
    expect(names[0]).toContain("/history");
    expect(names[1]).toContain("/help");
  });

  it("moves the highlight with the arrows, wrapping", async () => {
    const { field } = mount(MENU);
    type(field, "/h");
    const menu = await screen.findByLabelText(BOT_COMPOSER_MENU_LABEL);
    const current = () =>
      Array.from(menu.querySelectorAll("li")).findIndex(
        (item) => item.getAttribute("aria-current") === "true",
      );
    expect(current()).toBe(0);
    expect(key(field, { key: "ArrowDown" })).toBe(true);
    expect(current()).toBe(1);
    expect(key(field, { key: "ArrowDown" })).toBe(true);
    expect(current()).toBe(0);
    expect(key(field, { key: "ArrowUp" })).toBe(true);
    expect(current()).toBe(1);
  });

  /**
   * Enter completes the highlighted name first and runs it on the second press
   * — the model that works for a command with an argument, because accepting
   * `/history` must leave the caret where the search word goes.
   */
  it("completes on Enter and runs on the second press", async () => {
    const { onCommand, field } = mount({
      ...MENU,
      "/help": preview({
        draft: "/help",
        verdict: { kind: "command", name: "help", args: null },
        rows: [HELP_ROW],
      }),
    });
    type(field, "/h");
    await screen.findByLabelText(BOT_COMPOSER_MENU_LABEL);
    key(field, { key: "ArrowDown" });
    key(field);
    await waitFor(() => expect(field).toHaveValue("/help"));
    expect(onCommand).not.toHaveBeenCalled();
  });

  it("accepts with Tab as well", async () => {
    const { field } = mount(MENU);
    type(field, "/h");
    await screen.findByLabelText(BOT_COMPOSER_MENU_LABEL);
    expect(key(field, { key: "Tab" })).toBe(true);
    await waitFor(() => expect(field).toHaveValue("/history "));
  });

  /**
   * Escape dismisses the menu for that token and leaves it dismissed while the
   * token stands — and typing on brings it back, because Escape means "not
   * this one", never "never again".
   */
  it("stays dismissed for the token Escape dismissed", async () => {
    const { field } = mount({
      ...MENU,
      "/hi": preview({
        draft: "/hi",
        verdict: { kind: "command", name: "history", args: null },
        rows: [HISTORY_ROW],
      }),
    });
    type(field, "/h");
    await screen.findByLabelText(BOT_COMPOSER_MENU_LABEL);
    expect(key(field, { key: "Escape" })).toBe(true);
    await waitFor(() =>
      expect(screen.queryByLabelText(BOT_COMPOSER_MENU_LABEL)).not.toBeInTheDocument(),
    );
    // The same token again: still dismissed.
    type(field, "/h ");
    type(field, "/h");
    await waitFor(() =>
      expect(screen.queryByLabelText(BOT_COMPOSER_MENU_LABEL)).not.toBeInTheDocument(),
    );
    // A different token: the menu is owed again.
    type(field, "/hi");
    expect(await screen.findByLabelText(BOT_COMPOSER_MENU_LABEL)).toBeInTheDocument();
  });

  /**
   * The menu claims five keys and leaves everything else alone: a menu that
   * swallowed a keystroke it did not act on would eat the webview's own
   * handling.
   */
  it("never eats a keystroke it did not handle", async () => {
    const { field } = mount(MENU);
    type(field, "/h");
    await screen.findByLabelText(BOT_COMPOSER_MENU_LABEL);
    expect(key(field, { key: "a" })).toBe(false);
    expect(key(field, { key: "Backspace" })).toBe(false);
    expect(key(field, { key: "Home" })).toBe(false);
    // Still open — none of those dismissed it either.
    expect(screen.getByLabelText(BOT_COMPOSER_MENU_LABEL)).toBeInTheDocument();
  });

  /**
   * `/help` is answered here rather than forwarded, because the rows are
   * already in this component's hands. The panel teaches the escape, which is
   * the one thing about this surface nobody can guess from the menu.
   */
  it("answers /help itself, and the panel teaches the escape", async () => {
    const { onCommand, field } = mount({
      "/help": preview({
        draft: "/help",
        verdict: { kind: "command", name: "help", args: null },
        rows: [HELP_ROW],
      }),
      "/": preview({ draft: "/", rows: [HISTORY_ROW, HELP_ROW] }),
    });
    type(field, "/help");
    key(field);
    const panel = await screen.findByLabelText(BOT_COMPOSER_HELP_LABEL);
    expect(panel).toHaveTextContent("/history");
    expect(panel).toHaveTextContent("/help");
    expect(panel).toHaveTextContent(ESCAPE_HINT);
    expect(onCommand).not.toHaveBeenCalled();
  });
});
