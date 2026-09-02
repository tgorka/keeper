/**
 * The pinned bots: a hand order that persists, reachable from the keyboard, and
 * a strip that degrades honestly when it overflows (Story 61.7, FR-383).
 *
 * What is asserted here that nothing else asserts:
 *
 * 1. **The reorder is not a gesture-only affordance.** `Alt` with an arrow on
 *    the focused bot submits the WHOLE new order and the answer becomes the
 *    mirror — UX-DR28 forbids a gesture being the only path, and the chat pins
 *    strip's own spec records that it left exactly this gap.
 * 2. **The order submitted is complete.** Rust refuses a partial order, so a
 *    strip that submitted only what it was drawing would fail every reorder
 *    once there were more bots than slots. The truncated case is asserted
 *    directly.
 * 3. **Overflow is counted, not scrolled.** Nine bots in a scrolling strip are
 *    bots you cannot see and were not told about.
 * 4. **Identity is in the accessible name**, so two bots differing only in
 *    their ink are two different controls to a screen reader.
 */
import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import {
  BOT_PINS_LABEL,
  BOT_PINS_UNPIN,
  BOT_PINS_UNPIN_BODY,
  BOT_PINS_VISIBLE,
  BotPinsStrip,
  botPinsOverflowNote,
  botPinsUnpinTitle,
  botPinsWindow,
} from "@/components/bots/bot-pins-strip";
import type { BotVm } from "@/lib/ipc/client";
import { botsStore } from "@/lib/stores/bots";

const reorder = vi.fn();
const remove = vi.fn();

vi.mock("@/lib/ipc/client", async (importOriginal) => {
  const actual = await importOriginal<typeof import("@/lib/ipc/client")>();
  return {
    ...actual,
    botsBotsReorder: (order: string[]) => {
      reorder(order);
      // What Rust answers: the rows in their new order, renumbered.
      return Promise.resolve(
        order.map((id, index) => ({ ...bot(id, id.toUpperCase()), pinOrder: index })),
      );
    },
    botsBotRemove: (botId: string) => {
      remove(botId);
      return Promise.resolve();
    },
    botsBotsList: () => Promise.resolve([] as BotVm[]),
  };
});

function bot(id: string, name: string, extra: Partial<BotVm> = {}): BotVm {
  return {
    id,
    providerId: "prov-1",
    target: "llama4:8b",
    name,
    pinOrder: 0,
    shape: null,
    colour: null,
    mark: null,
    createdMs: 1,
    ...extra,
  };
}

const HAND = [bot("a", "Alpha"), bot("b", "Beta"), bot("c", "Gamma")];

beforeEach(() => {
  reorder.mockClear();
  remove.mockClear();
  botsStore.getState().reset();
});

describe("botPinsWindow", () => {
  it("draws everything while everything fits", () => {
    expect(botPinsWindow(HAND, "a")).toEqual({ shown: HAND, hidden: 0 });
  });

  it("bounds the row and counts the rest", () => {
    const many = Array.from({ length: BOT_PINS_VISIBLE + 3 }, (_, i) => bot(`b${i}`, `Bot ${i}`));
    const windowed = botPinsWindow(many, "b0");
    expect(windowed.shown).toHaveLength(BOT_PINS_VISIBLE);
    expect(windowed.hidden).toBe(3);
  });

  it("never hides the bot being talked to", () => {
    // The interesting case, and the one nobody looks at: the open
    // conversation's own bot sits past the bound.
    const many = Array.from({ length: BOT_PINS_VISIBLE + 3 }, (_, i) => bot(`b${i}`, `Bot ${i}`));
    const windowed = botPinsWindow(many, "b10");
    expect(windowed.shown.map((row) => row.id)).toContain("b10");
    expect(windowed.shown).toHaveLength(BOT_PINS_VISIBLE);
    // And the hand order still leads: only the last slot is given up.
    expect(windowed.shown[0]?.id).toBe("b0");
    expect(windowed.hidden).toBe(3);
  });
});

describe("BotPinsStrip", () => {
  it("is absent entirely when nothing is pinned", () => {
    render(<BotPinsStrip bots={[]} selectedBotId={null} onSelect={() => {}} />);
    expect(screen.queryByRole("navigation", { name: BOT_PINS_LABEL })).toBeNull();
  });

  it("carries each bot's identity in its accessible name", () => {
    render(
      <BotPinsStrip
        bots={[
          bot("a", "Alpha", { shape: "hollow", colour: "clay", mark: "mic" }),
          bot("b", "Beta", { shape: "hollow", colour: "steel", mark: "mic" }),
        ]}
        selectedBotId="a"
        onSelect={() => {}}
      />,
    );
    // Two bots differing ONLY in their ink are two different controls here.
    const alpha = screen.getByRole("button", { name: "Alpha — hollow cell in clay, marked mic" });
    const beta = screen.getByRole("button", { name: "Beta — hollow cell in steel, marked mic" });
    expect(alpha).not.toBe(beta);
    expect(alpha.getAttribute("aria-current")).toBe("true");
    expect(beta.getAttribute("aria-current")).toBeNull();
  });

  it("moves a bot from the keyboard and persists the whole new order", async () => {
    render(<BotPinsStrip bots={HAND} selectedBotId="a" onSelect={() => {}} />);
    const alpha = screen.getByRole("button", { name: /^Alpha/ });
    fireEvent.keyDown(alpha, { key: "ArrowRight", altKey: true });

    await waitFor(() => {
      expect(reorder).toHaveBeenCalledWith(["b", "a", "c"]);
    });
    // The answer is what the mirror holds — never an optimistic local order.
    await waitFor(() => {
      expect(botsStore.getState().bots?.map((row) => row.id)).toEqual(["b", "a", "c"]);
    });
  });

  it("moves the other way, and refuses to walk off either end", () => {
    render(<BotPinsStrip bots={HAND} selectedBotId="a" onSelect={() => {}} />);
    const gamma = screen.getByRole("button", { name: /^Gamma/ });
    fireEvent.keyDown(gamma, { key: "ArrowLeft", altKey: true });
    expect(reorder).toHaveBeenCalledWith(["a", "c", "b"]);

    reorder.mockClear();
    // The first bot cannot move earlier and the last cannot move later: there
    // is no order to submit, so nothing is submitted.
    fireEvent.keyDown(screen.getByRole("button", { name: /^Alpha/ }), {
      key: "ArrowLeft",
      altKey: true,
    });
    fireEvent.keyDown(screen.getByRole("button", { name: /^Gamma/ }), {
      key: "ArrowRight",
      altKey: true,
    });
    expect(reorder).not.toHaveBeenCalled();
  });

  it("leaves a bare arrow to the strip's own focus movement", () => {
    render(<BotPinsStrip bots={HAND} selectedBotId="a" onSelect={() => {}} />);
    fireEvent.keyDown(screen.getByRole("button", { name: /^Alpha/ }), { key: "ArrowRight" });
    expect(reorder).not.toHaveBeenCalled();
  });

  it("says how many bots it is not drawing, and still submits a complete order", async () => {
    const many = Array.from({ length: BOT_PINS_VISIBLE + 2 }, (_, i) => bot(`b${i}`, `Bot ${i}`));
    render(<BotPinsStrip bots={many} selectedBotId="b0" onSelect={() => {}} />);

    expect(screen.getAllByRole("button", { name: /^Bot / })).toHaveLength(BOT_PINS_VISIBLE);
    expect(screen.getByText(botPinsOverflowNote(2))).toBeTruthy();

    // The truncated strip still reorders over the FULL list: Rust refuses a
    // partial order, so an order of only the drawn rows would fail every time.
    fireEvent.keyDown(screen.getByRole("button", { name: /^Bot 0/ }), {
      key: "ArrowRight",
      altKey: true,
    });
    await waitFor(() => {
      expect(reorder).toHaveBeenCalledWith([
        "b1",
        "b0",
        "b2",
        "b3",
        "b4",
        "b5",
        "b6",
        "b7",
        "b8",
        "b9",
      ]);
    });
  });

  it("selects a bot on a plain click", () => {
    const onSelect = vi.fn();
    render(<BotPinsStrip bots={HAND} selectedBotId="a" onSelect={onSelect} />);
    fireEvent.click(screen.getByRole("button", { name: /^Beta/ }));
    expect(onSelect).toHaveBeenCalledWith("b");
  });

  it("unpins only after a confirmation that names what happens to what", async () => {
    render(<BotPinsStrip bots={HAND} selectedBotId="a" onSelect={() => {}} />);
    fireEvent.contextMenu(screen.getByRole("button", { name: /^Beta/ }));
    fireEvent.click(await screen.findByRole("menuitem", { name: BOT_PINS_UNPIN }));

    // The chain-of-custody rule: the sentence names the bot, the token and what
    // survives.
    expect(await screen.findByText(botPinsUnpinTitle("Beta"))).toBeTruthy();
    expect(screen.getByText(BOT_PINS_UNPIN_BODY)).toBeTruthy();
    expect(remove).not.toHaveBeenCalled();

    fireEvent.click(screen.getByRole("button", { name: BOT_PINS_UNPIN }));
    await waitFor(() => {
      expect(remove).toHaveBeenCalledWith("b");
    });
  });
});
