/**
 * An agents session composer's leading `@` (109.24, Q109-21 a).
 *
 * Which agents a token offers is `keeper_core::hq::mention`, proven in its own
 * tests; these inject the resolver with canned answers and assert the
 * composer's obligations: it asks only about a leading `@` word, the keyboard
 * picks and inserts an agent without sending, and the line then goes out whole
 * through `onSend` — Rust decides what an `@` line does.
 */
import { fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { MENTION_MENU_LABEL } from "@/components/agents/mention";
import { Composer } from "@/components/chat/composer";
import type { ChatInlinePreviewVm } from "@/lib/ipc/client";
import { attachmentsStore } from "@/lib/stores/attachments";
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

beforeEach(() => {
  attachmentsStore.getState().clear();
  draftsStore.getState().clear();
});

const AGENTS = [
  { id: "mason", name: "Mason" },
  { id: "tola-grey", name: "Tola Grey" },
];

/** Rust's answer: every agent whose handle starts with the leading `@` word. */
function answer(before: string, after: string): ChatInlinePreviewVm {
  const token = /^\s*(@\S*)$/.exec(before)?.[1] ?? null;
  return {
    before,
    after,
    token,
    rows:
      token === null
        ? []
        : AGENTS.filter((agent) => agent.id.startsWith(token.slice(1))).map((agent) => ({
            kind: "mention",
            key: agent.id,
            label: `@${agent.id}`,
            detail: agent.name,
            insert: `@${agent.id}`,
          })),
    note: null,
  };
}

function mount() {
  const onSend = vi.fn(async (_body: string) => {});
  const resolveMention = vi.fn(async (before: string, after: string) => answer(before, after));
  render(
    <Composer
      transport="session"
      accountId="alpha"
      roomId="dev1.1"
      onSend={onSend}
      resolveMention={resolveMention}
    />,
  );
  const field = screen.getByLabelText<HTMLTextAreaElement>("Message");
  return { field, onSend, resolveMention };
}

function type(field: HTMLTextAreaElement, value: string, caret = value.length) {
  fireEvent.change(field, {
    target: { value, selectionStart: caret, selectionEnd: caret },
  });
}

describe("Composer — @ brings another agent in", () => {
  it("offers Rust's agents for a leading @ and picks one with the keyboard, sending nothing", async () => {
    const { field, onSend, resolveMention } = mount();
    type(field, "@");
    const menu = await screen.findByRole("listbox", { name: MENTION_MENU_LABEL });
    expect(resolveMention).toHaveBeenLastCalledWith("@", "");
    expect(
      within(menu)
        .getAllByRole("option")
        .map((option) => option.textContent),
    ).toEqual(["@masonMason", "@tola-greyTola Grey"]);
    expect(field).toHaveAttribute("aria-expanded", "true");
    fireEvent.keyDown(field, { key: "ArrowDown" });
    fireEvent.keyDown(field, { key: "Enter" });
    expect(field.value).toBe("@tola-grey ");
    expect(onSend).not.toHaveBeenCalled();
    await waitFor(() =>
      expect(screen.queryByRole("listbox", { name: MENTION_MENU_LABEL })).toBeNull(),
    );
  });

  it("sends the @ line whole once the agent is chosen", async () => {
    const { field, onSend } = mount();
    type(field, "@to");
    await screen.findByRole("listbox", { name: MENTION_MENU_LABEL });
    fireEvent.keyDown(field, { key: "Tab" });
    type(field, `${field.value}what do the notes say?`);
    fireEvent.keyDown(field, { key: "Enter" });
    await waitFor(() => expect(onSend).toHaveBeenCalledTimes(1));
    expect(onSend.mock.calls[0]?.[0]).toBe("@tola-grey what do the notes say?");
  });

  it("asks nothing about an @ later in the line", async () => {
    const { field, resolveMention } = mount();
    type(field, "ask @to");
    type(field, "@to", 1);
    await waitFor(() => expect(screen.queryByRole("listbox")).toBeNull());
    expect(resolveMention).not.toHaveBeenCalled();
  });
});
