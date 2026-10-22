import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import { BOT_IDENTITY_NEEDS_SHAPE, BotIdentityPicker } from "@/components/bots/bot-identity";
import type * as Ipc from "@/lib/ipc/client";
import type { BotVm } from "@/lib/ipc/client";

const identitySave = vi.fn();

vi.mock("@/lib/ipc/client", async (importOriginal) => {
  const actual = await importOriginal<typeof Ipc>();
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
