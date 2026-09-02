/**
 * A bot's identity: shape and mark first, colour second (Story 61.7, FR-383).
 *
 * Three things are asserted here that nothing else in the tree asserts, and
 * each one is a defect that would otherwise ship silently:
 *
 * 1. **Colour is never the only carrier.** A colour with no shape paints no
 *    ink at all, and the identity is spoken in words — so two bots differing
 *    only in their ink have different accessible names. Remove the pairing
 *    guard in `BotIdentityCell` and `an ink is not painted without a shape`
 *    fails; drop the colour out of `botIdentityPhrase` and
 *    `two bots differing only in colour are still two identities` fails.
 * 2. **The palette is closed.** A colour token from outside
 *    `BOT_IDENTITY_COLOURS` is treated as absent rather than drawn as
 *    `currentColor`, and the picker refuses to send a colour with no shape
 *    before Rust has to.
 * 3. **A mark is either an icon keeper knows or the character somebody
 *    typed**, and the two are spoken differently — "marked mic" is an icon,
 *    "marked the character K" is not.
 */
import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import {
  BOT_IDENTITY_COLOURS,
  BOT_IDENTITY_NEEDS_SHAPE,
  BOT_IDENTITY_SHAPES,
  BotIdentityCell,
  BotIdentityPicker,
  botIdentityPhrase,
  botPinLabel,
  isBotIdentityColour,
  isBotIdentityShape,
} from "@/components/bots/bot-identity";
import type { BotVm } from "@/lib/ipc/client";

const identitySave = vi.fn();

vi.mock("@/lib/ipc/client", async (importOriginal) => {
  const actual = await importOriginal<typeof import("@/lib/ipc/client")>();
  return {
    ...actual,
    botsBotIdentitySave: (
      botId: string,
      shape: string | null,
      colour: string | null,
      mark: string | null,
    ) => {
      identitySave(botId, shape, colour, mark);
      return Promise.resolve({ ...BOT, shape, colour, mark });
    },
  };
});

const BOT: BotVm = {
  id: "bot-1",
  providerId: "prov-1",
  target: "llama4:8b",
  name: "Llama",
  pinOrder: 0,
  shape: null,
  colour: null,
  mark: null,
  createdMs: 1,
};

const cell = () => {
  const node = document.querySelector('[data-slot="bot-identity"]');
  if (node === null) {
    throw new Error("no identity cell rendered");
  }
  return node;
};

describe("the closed sets", () => {
  it("is bounded, and refuses a token from outside itself", () => {
    // The bound is the story: an unbounded list is the free colour picker under
    // another name, and `DESIGN.md` rejects that rather than deferring it.
    expect(BOT_IDENTITY_COLOURS.length).toBeGreaterThanOrEqual(6);
    expect(BOT_IDENTITY_COLOURS.length).toBeLessThanOrEqual(10);
    expect(BOT_IDENTITY_SHAPES.length).toBeGreaterThanOrEqual(4);
    expect(BOT_IDENTITY_SHAPES.length).toBeLessThanOrEqual(6);

    for (const colour of BOT_IDENTITY_COLOURS) {
      expect(isBotIdentityColour(colour), colour).toBe(true);
    }
    expect(isBotIdentityColour("hotpink")).toBe(false);
    expect(isBotIdentityColour("emerald")).toBe(false);
    expect(isBotIdentityColour(null)).toBe(false);
    expect(isBotIdentityShape("blob")).toBe(false);
  });

  it("paints a distinct drawing for every shape, so two shapes never collide", () => {
    // Two shapes sharing a drawing is colour becoming the only carrier again,
    // by accident — the lamp's own contract, restated for the cell.
    const drawings = new Set<string>();
    for (const shape of BOT_IDENTITY_SHAPES) {
      const { unmount } = render(
        <BotIdentityCell identity={{ shape, colour: "clay", mark: null }} />,
      );
      drawings.add(cell().innerHTML);
      unmount();
    }
    expect(drawings.size).toBe(BOT_IDENTITY_SHAPES.length);
  });
});

describe("botIdentityPhrase", () => {
  it("two bots differing only in colour are still two identities", () => {
    const clay = botPinLabel({ ...BOT, shape: "hollow", colour: "clay", mark: "mic" });
    const steel = botPinLabel({ ...BOT, shape: "hollow", colour: "steel", mark: "mic" });
    expect(clay).not.toBe(steel);
    // And each one still names the shape and the mark FIRST, which is what a
    // reader who cannot tell the two inks apart is left with.
    expect(clay).toBe("Llama — hollow cell in clay, marked mic");
    expect(steel).toContain("hollow cell");
    expect(steel).toContain("marked mic");
  });

  it("says an icon by name and a typed mark as the character it is", () => {
    expect(botIdentityPhrase({ shape: "filled", colour: null, mark: "flask-conical" })).toBe(
      "filled cell, marked flask conical",
    );
    expect(botIdentityPhrase({ shape: "filled", colour: null, mark: "K" })).toBe(
      "filled cell, marked the character K",
    );
  });

  it("names an unchosen identity, and one this build cannot draw", () => {
    expect(botIdentityPhrase({ shape: null, colour: null, mark: null })).toBe("no identity chosen");
    // AD-27: an unknown is never rendered as a `false`. A shape a newer build
    // stored is present-and-unrenderable, which is a different fact from none.
    expect(botIdentityPhrase({ shape: "trapezoid", colour: "clay", mark: null })).toBe(
      "a shape this version of keeper cannot draw",
    );
  });

  it("leaves a colour out of the words when there is no shape to pair it with", () => {
    expect(botIdentityPhrase({ shape: null, colour: "clay", mark: "mic" })).toBe("marked mic");
  });
});

describe("BotIdentityCell", () => {
  it("paints the chosen ink as a token class, per colour", () => {
    for (const colour of BOT_IDENTITY_COLOURS) {
      const { unmount } = render(
        <BotIdentityCell identity={{ shape: "filled", colour, mark: null }} />,
      );
      const node = cell();
      expect(node.getAttribute("data-colour")).toBe(colour);
      // Spelled classes, not `text-bot-ink-${colour}`: Tailwind reads source
      // text, so an interpolated class produces no CSS while every test that
      // read the attribute would still pass.
      expect(node.className).toContain(`text-bot-ink-${colour}`);
      unmount();
    }
  });

  it("an ink is not painted without a shape", () => {
    // `DESIGN.md:172`. Removing the pairing guard makes this the bare coloured
    // dot the design bans.
    render(<BotIdentityCell identity={{ shape: null, colour: "clay", mark: null }} />);
    const node = cell();
    expect(node.getAttribute("data-colour")).toBe("none");
    expect(node.className).not.toContain("text-bot-ink-clay");
    expect(node.querySelector("svg")).toBeNull();
  });

  it("treats a colour token it does not know as absent, without redrawing it", () => {
    render(<BotIdentityCell identity={{ shape: "filled", colour: "hotpink", mark: null }} />);
    expect(cell().getAttribute("data-colour")).toBe("none");
    // The shape still draws: an unknown ink is not a reason to lose the
    // identity's primary carrier.
    expect(cell().getAttribute("data-shape")).toBe("filled");
  });

  it("is hidden from the accessible tree, because its owner speaks it", () => {
    render(<BotIdentityCell identity={{ shape: "filled", colour: "clay", mark: "K" }} />);
    expect(cell().getAttribute("aria-hidden")).toBe("true");
    // The typed mark is drawn as itself rather than resolved to an icon.
    expect(cell().textContent).toBe("K");
  });
});

describe("BotIdentityPicker", () => {
  it("refuses a colour with no shape before it sends anything", async () => {
    identitySave.mockClear();
    render(<BotIdentityPicker bot={BOT} open={true} onOpenChange={() => {}} onSaved={() => {}} />);
    fireEvent.click(screen.getByRole("button", { name: "clay" }));
    // The refusal is on screen the moment the pairing breaks, not only on save.
    expect(screen.getAllByText(BOT_IDENTITY_NEEDS_SHAPE).length).toBeGreaterThan(0);
    fireEvent.click(screen.getByRole("button", { name: "Save" }));
    await waitFor(() => {
      expect(screen.getAllByText(BOT_IDENTITY_NEEDS_SHAPE).length).toBeGreaterThan(0);
    });
    expect(identitySave).not.toHaveBeenCalled();
  });

  it("saves the shape, the colour and the mark, and hands back the stored row", async () => {
    identitySave.mockClear();
    const saved = vi.fn();
    render(<BotIdentityPicker bot={BOT} open={true} onOpenChange={() => {}} onSaved={saved} />);
    fireEvent.click(screen.getByRole("button", { name: "hollow cell" }));
    fireEvent.click(screen.getByRole("button", { name: "lapis" }));
    fireEvent.change(screen.getByLabelText("Mark"), { target: { value: "K" } });
    fireEvent.click(screen.getByRole("button", { name: "Save" }));

    await waitFor(() => {
      expect(identitySave).toHaveBeenCalledWith("bot-1", "hollow", "lapis", "K");
    });
    expect(saved).toHaveBeenCalledWith(
      expect.objectContaining({ shape: "hollow", colour: "lapis", mark: "K" }),
    );
  });

  it("picks a mark out of the existing icon set", async () => {
    identitySave.mockClear();
    render(<BotIdentityPicker bot={BOT} open={true} onOpenChange={() => {}} onSaved={() => {}} />);
    fireEvent.change(screen.getByLabelText("Search icons"), { target: { value: "voice" } });
    // `space-icons.ts`' alias table is what makes this findable — the glyph is
    // called `mic`, and "voice" is one of the words somebody actually types.
    fireEvent.click(screen.getByRole("button", { name: "mic" }));
    fireEvent.click(screen.getByRole("button", { name: "Save" }));
    await waitFor(() => {
      expect(identitySave).toHaveBeenCalledWith("bot-1", null, null, "mic");
    });
  });
});
