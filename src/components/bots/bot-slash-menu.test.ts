/**
 * The bot pane's side of its slash commands (Epic 61, Story 61.9): the context
 * it sends and what it does with a resolved command. The menu's mechanics are
 * shared and tested in `slash-menu/slash-menu.test.ts`. The rules — exact beats
 * prefix, the nearest match, the escape, availability — are
 * `keeper_core::bots::commands` and are proven in
 * `keeper-core/tests/bots_commands.rs`, because a second matcher in this file
 * would be a second opinion about what `/mod` means.
 */
import { describe, expect, it, vi } from "vitest";
import {
  BOT_COMMAND_GRANT_IS_A_SURFACE,
  BOT_COMMAND_HISTORY_IS_A_LIST,
  BOT_COMMAND_METADATA_IS_A_TOGGLE,
  botCommandContext,
  botCommandHost,
} from "@/components/bots/bot-slash-menu";

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
