/**
 * The messenger composer's mid-text `/` (Story 101.3, R418).
 *
 * What a token offers is `keeper_core::chat::inline`, proven in its own tests;
 * these inject the resolver with canned answers and assert the composer's
 * obligations: it asks about the split at the caret, Enter inserts the
 * highlighted row in place of the token and nothing else, Escape dismisses the
 * menu without sending, and a whole-line command is left to its own menu.
 */
import { act, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { Composer, type SendContext } from "@/components/chat/composer";
import { INLINE_MENU_LABEL, INLINE_PREVIEW_FAILED } from "@/components/slash-menu/inline-slash";
import type { ChatCommandPreviewVm, ChatInlinePreviewVm } from "@/lib/ipc/client";
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

import { reportFrontendError } from "@/lib/crash-report";

beforeEach(() => {
  attachmentsStore.getState().clear();
  draftsStore.getState().clear();
  vi.mocked(reportFrontendError).mockClear();
});

const TOLA = {
  kind: "mention",
  key: "mention:@tola-grey:example.org",
  label: "@tola-grey",
  detail: "Tola Grey",
  insert: "@tola-grey",
};
const PLAN = {
  kind: "file",
  key: "file:01TG:notes/plans/trip.md",
  label: "Trip to Tatry",
  detail: "tgdrive · notes/plans/trip.md",
  insert: "tgdrive:notes/plans/trip.md",
} as const;

/** Rust's answer for a token at the end of `before`: TOLA and PLAN for any `/t…`. */
function answer(before: string, after: string): ChatInlinePreviewVm {
  const token = /\s(\/[^\s/]*)$/.exec(before)?.[1] ?? null;
  return {
    before,
    after,
    token,
    rows: token?.startsWith("/t") ? [{ ...TOLA, kind: "mention" }, { ...PLAN }] : [],
    note: null,
  };
}

function mount(options: { reject?: boolean } = {}) {
  const onSend = vi.fn(async (_body: string, _context: SendContext, _line?: string) => {});
  const resolveInline = vi.fn(async (before: string, after: string) => {
    if (options.reject === true) {
      throw new Error("shell gone");
    }
    return answer(before, after);
  });
  const resolveCommand = vi.fn(
    async (draft: string): Promise<ChatCommandPreviewVm> => ({
      draft,
      verdict: { kind: "text", text: draft },
      rows: [],
      escapeHint: "",
    }),
  );
  render(
    <Composer
      accountId="acct"
      roomId="!agent:example.org"
      onSend={onSend}
      onSendEmote={vi.fn(async () => {})}
      onCommand={vi.fn(async () => null)}
      resolveCommand={resolveCommand}
      resolveInline={resolveInline}
    />,
  );
  const field = screen.getByLabelText<HTMLTextAreaElement>("Message");
  return { field, onSend, resolveInline };
}

/** Type `value` with the caret at `caret` (its end by default). */
function type(field: HTMLTextAreaElement, value: string, caret = value.length) {
  fireEvent.change(field, {
    target: { value, selectionStart: caret, selectionEnd: caret },
  });
}

describe("Composer — a / in the middle of a message", () => {
  it("asks about the draft split at the caret and draws Rust's rows", async () => {
    const { field, resolveInline } = mount();
    type(field, "ask /to about it", "ask /to".length);
    const menu = await screen.findByRole("listbox", { name: INLINE_MENU_LABEL });
    expect(resolveInline).toHaveBeenLastCalledWith("ask /to", " about it");
    expect(
      within(menu)
        .getAllByRole("option")
        .map((o) => o.textContent),
    ).toEqual(["@tola-greyTola Grey", "Trip to Tatrytgdrive · notes/plans/trip.md"]);
    expect(field).toHaveAttribute("aria-expanded", "true");
  });

  it("inserts the highlighted row in place of the token on Enter, sending nothing", async () => {
    const { field, onSend } = mount();
    type(field, "ask /to about it", "ask /to".length);
    await screen.findByRole("listbox", { name: INLINE_MENU_LABEL });
    fireEvent.keyDown(field, { key: "ArrowDown" });
    fireEvent.keyDown(field, { key: "Enter" });
    expect(field.value).toBe("ask tgdrive:notes/plans/trip.md about it");
    expect(onSend).not.toHaveBeenCalled();
    await waitFor(() =>
      expect(screen.queryByRole("listbox", { name: INLINE_MENU_LABEL })).toBeNull(),
    );
  });

  it("closes on Escape for that token, and Enter then sends the line as typed", async () => {
    const { field, onSend } = mount();
    type(field, "ask /to");
    await screen.findByRole("listbox", { name: INLINE_MENU_LABEL });
    fireEvent.keyDown(field, { key: "Escape" });
    expect(screen.queryByRole("listbox", { name: INLINE_MENU_LABEL })).toBeNull();
    fireEvent.keyDown(field, { key: "Enter" });
    await waitFor(() => expect(onSend).toHaveBeenCalledTimes(1));
    expect(onSend.mock.calls[0]?.[0]).toBe("ask /to");
  });

  it("leaves a whole-line command to its own menu", async () => {
    const { field, resolveInline } = mount();
    type(field, "/topic /to");
    await act(async () => {});
    expect(resolveInline).not.toHaveBeenCalled();
  });

  it("says a failed lookup once under the field and reports it once", async () => {
    const { field } = mount({ reject: true });
    type(field, "ask /t");
    expect(await screen.findByText(INLINE_PREVIEW_FAILED)).toBeInTheDocument();
    type(field, "ask /to");
    await waitFor(() => expect(reportFrontendError).toHaveBeenCalledTimes(1));
    expect(screen.getAllByText(INLINE_PREVIEW_FAILED)).toHaveLength(1);
  });
});
