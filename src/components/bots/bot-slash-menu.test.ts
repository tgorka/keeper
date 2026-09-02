/**
 * The slash menu's mechanics (Epic 61, Story 61.9).
 *
 * Everything asserted here is a fact about a `<textarea>`, not about what a
 * command means: which drafts are candidates at all, which token a dismissal
 * belongs to, and which keystrokes the menu claims. The rules — exact beats
 * prefix, the nearest match, the escape, availability — are
 * `keeper_core::bots::commands` and are proven in
 * `keeper-core/tests/bots_commands.rs`, because a second matcher in this file
 * would be a second opinion about what `/mod` means.
 */
import { describe, expect, it, vi } from "vitest";
import {
  acceptedDraft,
  argumentHint,
  BOT_COMMAND_GRANT_IS_A_SURFACE,
  BOT_COMMAND_HISTORY_IS_A_LIST,
  BOT_COMMAND_METADATA_IS_A_TOGGLE,
  botCommandContext,
  botCommandHost,
  isCommandLine,
  MAX_COMMAND_CHARS,
  menuKeyAction,
  nextIndex,
  slashToken,
} from "@/components/bots/bot-slash-menu";
import type { BotCommandRowVm } from "@/lib/ipc/client";

/** No modifier held — the ordinary keystroke. */
const PLAIN = { shiftKey: false, metaKey: false, ctrlKey: false, altKey: false };

function row(overrides: Partial<BotCommandRowVm> = {}): BotCommandRowVm {
  return {
    name: "bot",
    aliases: [],
    description: "Switch to another bot by name.",
    args: "required",
    argHint: "a bot name",
    available: true,
    reason: null,
    warning: null,
    ...overrides,
  };
}

describe("isCommandLine", () => {
  it("accepts a leading slash on its own line", () => {
    expect(isCommandLine("/")).toBe(true);
    expect(isCommandLine("/help")).toBe(true);
    expect(isCommandLine("/bot work hermes")).toBe(true);
  });

  /**
   * The escape is not a command line, which is what stops a menu opening over
   * a message that is deliberately about a path.
   */
  it("refuses a doubled slash, so the escape opens no menu", () => {
    expect(isCommandLine("//etc")).toBe(false);
  });

  /** A slash inside a path or a fraction stays a slash. */
  it("refuses a slash that is not first", () => {
    expect(isCommandLine("what is in /etc/hosts?")).toBe(false);
    expect(isCommandLine("2/3 of it")).toBe(false);
    expect(isCommandLine("")).toBe(false);
  });

  /** A command is one line, so a pasted diff cannot open a menu. */
  it("refuses a multi-line draft and an over-long one", () => {
    expect(isCommandLine("/new\nand then prose")).toBe(false);
    expect(isCommandLine(`/${"x".repeat(MAX_COMMAND_CHARS)}`)).toBe(false);
  });
});

describe("slashToken", () => {
  it("is the word after the slash, lowercased", () => {
    expect(slashToken("/Help")).toBe("help");
    expect(slashToken("/bot work hermes")).toBe("bot");
    expect(slashToken("/")).toBe("");
  });

  it("is null where there is no command line", () => {
    expect(slashToken("//etc")).toBeNull();
    expect(slashToken("hello")).toBeNull();
  });
});

describe("menuKeyAction", () => {
  it("claims the five keys the menu owns", () => {
    expect(menuKeyAction("ArrowUp", true, PLAIN)).toBe("up");
    expect(menuKeyAction("ArrowDown", true, PLAIN)).toBe("down");
    expect(menuKeyAction("Enter", true, PLAIN)).toBe("accept");
    expect(menuKeyAction("Tab", true, PLAIN)).toBe("accept");
    expect(menuKeyAction("Escape", true, PLAIN)).toBe("dismiss");
  });

  /**
   * The rule that keeps the menu from eating a keystroke it did not act on:
   * anything outside the closed set is somebody else's, so it is left alone.
   */
  it("claims nothing else, and nothing at all while closed", () => {
    expect(menuKeyAction("a", true, PLAIN)).toBeNull();
    expect(menuKeyAction("Backspace", true, PLAIN)).toBeNull();
    expect(menuKeyAction("Home", true, PLAIN)).toBeNull();
    for (const key of ["ArrowUp", "ArrowDown", "Enter", "Tab", "Escape"]) {
      expect(menuKeyAction(key, false, PLAIN)).toBeNull();
    }
  });

  /**
   * Shift+Enter is the newline whatever is on screen, and a modified arrow is
   * a selection or a word jump. None of them are the menu's.
   */
  it("leaves modified chords alone", () => {
    expect(menuKeyAction("Enter", true, { ...PLAIN, shiftKey: true })).toBeNull();
    expect(menuKeyAction("Enter", true, { ...PLAIN, metaKey: true })).toBeNull();
    expect(menuKeyAction("Enter", true, { ...PLAIN, ctrlKey: true })).toBeNull();
    expect(menuKeyAction("Tab", true, { ...PLAIN, shiftKey: true })).toBeNull();
    expect(menuKeyAction("ArrowDown", true, { ...PLAIN, altKey: true })).toBeNull();
  });
});

describe("nextIndex", () => {
  it("wraps at both ends", () => {
    expect(nextIndex(0, 3, "down")).toBe(1);
    expect(nextIndex(2, 3, "down")).toBe(0);
    expect(nextIndex(0, 3, "up")).toBe(2);
  });

  it("stays put over an empty list", () => {
    expect(nextIndex(0, 0, "down")).toBe(0);
  });
});

describe("acceptedDraft", () => {
  /**
   * A command that takes an argument is completed with the space, so the caret
   * lands where the argument goes; one that takes none is completed without,
   * because a trailing space is text after a name that refuses text.
   */
  it("adds a space only where an argument follows", () => {
    expect(acceptedDraft(row({ name: "bot", args: "required" }))).toBe("/bot ");
    expect(acceptedDraft(row({ name: "history", args: "optionalRest" }))).toBe("/history ");
    expect(acceptedDraft(row({ name: "help", args: "none", argHint: null }))).toBe("/help");
  });
});

describe("argumentHint", () => {
  it("brackets an optional argument and angles a required one", () => {
    expect(argumentHint(row({ args: "required", argHint: "a bot name" }))).toBe("<a bot name>");
    expect(argumentHint(row({ args: "optionalRest", argHint: "a word" }))).toBe("[a word]");
    expect(argumentHint(row({ args: "none", argHint: null }))).toBeNull();
  });
});

describe("botCommandContext", () => {
  /**
   * `modelTools` passes through as a tri-state. Flattening `null` to `false`
   * here would hide the grant row for a model that may well take tools.
   */
  it("reads four facts off the pane and keeps the tri-state", () => {
    expect(
      botCommandContext({
        providerKind: "ollama",
        providerCount: 2,
        botId: "01J8",
        hasSession: false,
        modelTools: null,
      }),
    ).toEqual({
      providerKind: "ollama",
      hasProvider: true,
      hasBot: true,
      hasSession: false,
      modelTools: null,
    });
    expect(
      botCommandContext({
        providerKind: null,
        providerCount: 0,
        botId: null,
        hasSession: false,
        modelTools: false,
      }),
    ).toMatchObject({ hasProvider: false, hasBot: false, modelTools: false });
  });
});

describe("botCommandHost", () => {
  const bots = [
    { id: "01J8A", name: "Work Hermes" },
    { id: "01J8B", name: "Local llama" },
  ];

  function host() {
    const newConversation = vi.fn();
    const selectBot = vi.fn();
    const selectModel = vi.fn();
    return {
      run: botCommandHost({ bots, newConversation, selectBot, selectModel }),
      newConversation,
      selectBot,
      selectModel,
    };
  }

  it("acts on the three commands this pane owns, and says so by returning null", () => {
    const { run, newConversation, selectBot, selectModel } = host();
    expect(run({ name: "new", args: null })).toBeNull();
    expect(newConversation).toHaveBeenCalledOnce();
    expect(run({ name: "bot", args: "local LLAMA" })).toBeNull();
    expect(selectBot).toHaveBeenCalledWith("01J8B");
    expect(run({ name: "model", args: "llama4:8b" })).toBeNull();
    expect(selectModel).toHaveBeenCalledWith("llama4:8b");
  });

  /**
   * A name nothing answers to is the pane's own refusal, and it quotes what was
   * typed rather than a tidied copy — so somebody who typed a stale name sees
   * the stale name.
   */
  it("refuses a bot name nothing answers to, and selects nothing", () => {
    const { run, selectBot } = host();
    const message = run({ name: "bot", args: "Hermes at home" });
    expect(message).toContain("Hermes at home");
    expect(selectBot).not.toHaveBeenCalled();
  });

  /**
   * The three commands whose effect is a control somewhere else say where it
   * is. A command that silently did nothing is the affordance AD-27 forbids,
   * and this pane is the only thing that knows which surfaces it holds.
   */
  it("names the surface for the commands that are controls elsewhere", () => {
    const { run } = host();
    expect(run({ name: "metadata", args: null })).toBe(BOT_COMMAND_METADATA_IS_A_TOGGLE);
    expect(run({ name: "grant", args: null })).toBe(BOT_COMMAND_GRANT_IS_A_SURFACE);
    expect(run({ name: "history", args: null })).toBe(BOT_COMMAND_HISTORY_IS_A_LIST);
  });
});
