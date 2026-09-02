/**
 * The metadata caption, its expander and its persisted toggle (Epic 61, Story
 * 61.8, FR-384).
 *
 * Five things are asserted here that nothing else in the tree asserts:
 *
 * 1. **Off renders nothing.** The default is off and the default is what a
 *    fresh store holds, so a row with a full set of numbers still shows none of
 *    them until somebody asks.
 * 2. **Absent is absent, field by field.** Each nullable column is nulled on
 *    its own and the caption and the expander are checked for the label AND for
 *    a `0` — because the failure this story exists to prevent is not a missing
 *    row, it is a zero that looks like a measurement.
 * 3. **The expander is a real disclosure**, with `aria-expanded` moving and a
 *    panel that is absent rather than hidden while shut.
 * 4. **A partial row says so** in the caption, not only in the sibling sentence
 *    `bot-message.tsx` owns.
 * 5. **The toggle is persisted in Rust and defaults off** — it hydrates from
 *    `bots_message_details_get` and writes through `bots_message_details_set`,
 *    and the palette's verb is the same function the chip calls.
 *
 * The caption is deliberately checked for the ABSENCE of a live region too: a
 * row streams token by token, and an `aria-live` metadata line would re-read
 * itself on every delta.
 */
import { fireEvent, render, screen, waitFor } from "@testing-library/react";

import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import {
  BOT_META_LESS_LABEL,
  BOT_META_MORE_LABEL,
  BOT_META_NOTHING_REPORTED,
  BOT_META_PANEL_LABEL,
  BOT_META_PARTIAL,
  BOT_META_TOGGLE_LABEL,
  BotMessageMeta,
  BotMetaToggle,
  metaCaption,
  toggleBotMessageDetails,
} from "@/components/bots/bot-message-meta";
import { paletteActionHandlers } from "@/components/command-palette/actions";
import type { BotMessageVm } from "@/lib/ipc/client";
import { botsStore } from "@/lib/stores/bots";

/** What the fake Rust setting currently holds. */
let stored = false;
const detailsGet = vi.fn(() => Promise.resolve(stored));
const detailsSet = vi.fn((shown: boolean) => {
  stored = shown;
  return Promise.resolve();
});

vi.mock("@/lib/ipc/client", async (importOriginal) => {
  const actual = await importOriginal<typeof import("@/lib/ipc/client")>();
  return {
    ...actual,
    botsMessageDetailsGet: () => detailsGet(),
    botsMessageDetailsSet: (shown: boolean) => detailsSet(shown),
  };
});

/** A fully-reported answer: every optional column carries a number. */
function answer(overrides: Partial<BotMessageVm> = {}): BotMessageVm {
  return {
    id: "m1",
    sessionId: "s1",
    seq: 1,
    role: "assistant",
    content: "Two files mention it.",
    model: "llama3.1:8b",
    providerId: "01J8BOTPROVOLLAMAAAAAAAAAA",
    promptTokens: 1204,
    completionTokens: 87,
    totalTokens: 1291,
    ttftMs: 340,
    durationMs: 12_400,
    finishReason: "stop",
    requestId: "chatcmpl-abc123",
    toolCallCount: 0,
    partial: false,
    createdMs: 1_700_000_000_000,
    ...overrides,
  };
}

beforeEach(() => {
  stored = false;
  detailsGet.mockClear();
  detailsSet.mockClear();
  botsStore.getState().setMetaShown(false);
});

afterEach(() => {
  botsStore.getState().setMetaShown(false);
});

describe("the toggle governs whether anything is shown at all", () => {
  it("renders nothing while the toggle is off, however much was recorded", () => {
    const { container } = render(<BotMessageMeta message={answer()} />);

    expect(container).toBeEmptyDOMElement();
    // Not merely invisible: the numbers are not in the document.
    expect(screen.queryByText(/llama3\.1/)).not.toBeInTheDocument();
    expect(screen.queryByText(BOT_META_MORE_LABEL)).not.toBeInTheDocument();
  });

  it("renders the compact caption once the toggle is on", () => {
    botsStore.getState().setMetaShown(true);
    render(<BotMessageMeta message={answer()} />);

    expect(
      screen.getByText("llama3.1:8b · 1,291 tokens · first token after 340 ms · answered in 0:12"),
    ).toBeInTheDocument();
  });

  it("says nothing about a question, which has no model and no tokens", () => {
    botsStore.getState().setMetaShown(true);
    const { container } = render(
      <BotMessageMeta message={answer({ role: "user", content: "where is it?" })} />,
    );

    expect(container).toBeEmptyDOMElement();
  });

  it("keeps the caption out of every live region, so a stream cannot spam it", () => {
    botsStore.getState().setMetaShown(true);
    const { container } = render(<BotMessageMeta message={answer()} />);

    expect(container.querySelector("[aria-live]")).toBeNull();
    expect(container.querySelector('[role="status"]')).toBeNull();
    expect(container.querySelector('[role="alert"]')).toBeNull();
    expect(container.querySelector('[role="log"]')).toBeNull();
  });
});

describe("an absent number renders as absent, never as zero", () => {
  /** Every nullable column, with the label and the caption phrase it owns. */
  const nullable: { field: keyof BotMessageVm; label: string; shown: RegExp }[] = [
    { field: "model", label: "Model", shown: /llama3\.1/ },
    { field: "providerId", label: "Provider", shown: /01J8BOTPROV/ },
    { field: "promptTokens", label: "Prompt tokens", shown: /1,204 tokens/ },
    { field: "completionTokens", label: "Completion tokens", shown: /87 tokens/ },
    { field: "totalTokens", label: "Total tokens", shown: /1,291 tokens/ },
    { field: "ttftMs", label: "Time to first token", shown: /340 ms/ },
    { field: "durationMs", label: "Total time", shown: /0:12/ },
    { field: "finishReason", label: "Why it stopped", shown: /stop/ },
    { field: "requestId", label: "Request id", shown: /chatcmpl-abc123/ },
  ];

  for (const { field, label, shown } of nullable) {
    it(`drops ${String(field)} entirely when the endpoint did not report it`, async () => {
      botsStore.getState().setMetaShown(true);
      // A non-zero tool-call count, so the only `0` a panel could contain is a
      // fabricated one. `toolCallCount` is the one column that is not nullable,
      // and its own zero is asserted separately below.
      render(<BotMessageMeta message={answer({ toolCallCount: 2, [field]: null })} />);
      fireEvent.click(screen.getByRole("button", { name: BOT_META_MORE_LABEL }));

      const panel = screen.getByLabelText(BOT_META_PANEL_LABEL);
      const text = panel.textContent ?? "";
      // The label is gone, so there is no row to put a value in…
      expect(screen.queryByText(label)).not.toBeInTheDocument();
      // …the value it would have carried is nowhere…
      expect(shown.test(text), `${label} value must be absent`).toBe(false);
      // …and no zero has been invented in its place. These are the exact
      // strings a `?? 0` in the renderer would produce.
      expect(screen.queryByText("0 tokens")).not.toBeInTheDocument();
      expect(screen.queryByText("0 ms")).not.toBeInTheDocument();
      expect(screen.queryByText("0:00")).not.toBeInTheDocument();
      // `toolCallCount` is 2 here, so a bare `0` anywhere is a fabrication.
      expect(screen.queryByText("0")).not.toBeInTheDocument();
      // The caption above the panel is equally free of it.
      const caption = metaCaption(answer({ [field]: null }));
      expect(caption).not.toContain("0 tokens");
      expect(caption).not.toContain("after 0 ms");
      expect(caption).not.toContain("in 0:00");
    });
  }

  it("shows every field when the endpoint reported all of them", async () => {
    botsStore.getState().setMetaShown(true);
    render(<BotMessageMeta message={answer()} />);
    fireEvent.click(screen.getByRole("button", { name: BOT_META_MORE_LABEL }));

    const panel = screen.getByLabelText(BOT_META_PANEL_LABEL);
    for (const { label, shown } of nullable) {
      expect(screen.getByText(label), `${label} row`).toBeInTheDocument();
      expect(shown.test(panel.textContent ?? ""), `${label} value`).toBe(true);
    }
  });

  it("names the silence when an endpoint reported nothing measurable", () => {
    botsStore.getState().setMetaShown(true);
    render(
      <BotMessageMeta
        message={answer({
          model: null,
          providerId: null,
          promptTokens: null,
          completionTokens: null,
          totalTokens: null,
          ttftMs: null,
          durationMs: null,
          finishReason: null,
          requestId: null,
        })}
      />,
    );

    expect(screen.getByText(BOT_META_NOTHING_REPORTED)).toBeInTheDocument();
    const bare = answer({ model: null, totalTokens: null, ttftMs: null, durationMs: null });
    expect(metaCaption(bare)).toBe("");
  });

  it("shows a zero tool-call count, because that one IS a measurement", async () => {
    botsStore.getState().setMetaShown(true);
    render(<BotMessageMeta message={answer()} />);
    fireEvent.click(screen.getByRole("button", { name: BOT_META_MORE_LABEL }));

    expect(screen.getByText("Tool calls")).toBeInTheDocument();
    expect(screen.getByText("0")).toBeInTheDocument();
  });
});

describe("the expander is a real disclosure", () => {
  it("moves aria-expanded and mounts the panel only when open", async () => {
    botsStore.getState().setMetaShown(true);
    render(<BotMessageMeta message={answer()} />);

    const trigger = screen.getByRole("button", { name: BOT_META_MORE_LABEL });
    expect(trigger).toHaveAttribute("aria-expanded", "false");
    expect(screen.queryByLabelText(BOT_META_PANEL_LABEL)).not.toBeInTheDocument();

    fireEvent.click(trigger);

    const open = screen.getByRole("button", { name: BOT_META_LESS_LABEL });
    expect(open).toHaveAttribute("aria-expanded", "true");
    const panel = screen.getByLabelText(BOT_META_PANEL_LABEL);
    expect(panel).toBeInTheDocument();
    // The control points at the panel it controls, by id.
    expect(open.getAttribute("aria-controls")).toBe(panel.id);

    fireEvent.click(open);
    expect(screen.getByRole("button", { name: BOT_META_MORE_LABEL })).toHaveAttribute(
      "aria-expanded",
      "false",
    );
    expect(screen.queryByLabelText(BOT_META_PANEL_LABEL)).not.toBeInTheDocument();
  });
});

describe("a row that never finished says so", () => {
  it("states the answer is incomplete in the caption", () => {
    botsStore.getState().setMetaShown(true);
    render(<BotMessageMeta message={answer({ partial: true, finishReason: null })} />);

    expect(screen.getByText(new RegExp(BOT_META_PARTIAL))).toBeInTheDocument();
  });

  it("says nothing about completeness on a row that finished", () => {
    botsStore.getState().setMetaShown(true);
    render(<BotMessageMeta message={answer()} />);

    expect(screen.queryByText(new RegExp(BOT_META_PARTIAL))).not.toBeInTheDocument();
  });
});

describe("the toggle is persisted in Rust and defaults off", () => {
  it("hydrates from the stored setting rather than assuming", async () => {
    stored = true;
    render(<BotMetaToggle />);

    await waitFor(() =>
      expect(screen.getByRole("button", { name: BOT_META_TOGGLE_LABEL })).toHaveAttribute(
        "aria-pressed",
        "true",
      ),
    );
    expect(detailsGet).toHaveBeenCalled();
  });

  it("starts off when nothing has been stored", async () => {
    render(<BotMetaToggle />);

    await waitFor(() => expect(detailsGet).toHaveBeenCalled());
    expect(screen.getByRole("button", { name: BOT_META_TOGGLE_LABEL })).toHaveAttribute(
      "aria-pressed",
      "false",
    );
    expect(botsStore.getState().metaShown).toBe(false);
    // The store's own initial value, not the one `beforeEach` just set: the
    // first paint happens before any read resolves, and a mirror that starts
    // `true` would flash a caption nobody switched on.
    expect(botsStore.getInitialState().metaShown).toBe(false);
  });

  it("stays off when the setting could not be read, rather than claiming it is on", async () => {
    detailsGet.mockImplementationOnce(() => Promise.reject(new Error("no data dir")));
    render(<BotMetaToggle />);

    await waitFor(() => expect(detailsGet).toHaveBeenCalled());
    expect(screen.getByRole("button", { name: BOT_META_TOGGLE_LABEL })).toHaveAttribute(
      "aria-pressed",
      "false",
    );
  });

  it("writes through Rust and moves the mirror the caption reads", async () => {
    render(<BotMetaToggle />);
    await waitFor(() => expect(detailsGet).toHaveBeenCalled());

    fireEvent.click(screen.getByRole("button", { name: BOT_META_TOGGLE_LABEL }));

    await waitFor(() => expect(detailsSet).toHaveBeenCalledWith(true));
    expect(botsStore.getState().metaShown).toBe(true);
    expect(stored).toBe(true);
  });

  it("is the same verb the command palette dispatches", async () => {
    const handler = paletteActionHandlers["bots-toggle-metadata"];
    expect(handler).toBeTypeOf("function");

    await handler(null);

    expect(detailsSet).toHaveBeenCalledWith(true);
    expect(botsStore.getState().metaShown).toBe(true);

    // And it is a toggle, not a set: dispatching again puts it back.
    await toggleBotMessageDetails();
    expect(detailsSet).toHaveBeenLastCalledWith(false);
    expect(botsStore.getState().metaShown).toBe(false);
  });
});
