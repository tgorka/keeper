import { render, screen } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import { PaletteActionRow, PaletteChatRow } from "@/components/command-palette/palette-rows";
import { Command, CommandList } from "@/components/ui/command";
import { accountHueVar } from "@/lib/account-hue";
import type { PaletteActionVm, PaletteChatVm } from "@/lib/ipc/client";

function chat(p: Partial<PaletteChatVm> & Pick<PaletteChatVm, "roomId">): PaletteChatVm {
  return {
    id: `${p.accountId ?? "acc-a"}|${p.roomId}`,
    accountId: p.accountId ?? "acc-a",
    roomId: p.roomId,
    displayName: p.displayName ?? p.roomId,
    hueIndex: p.hueIndex ?? 0,
    network: p.network ?? null,
    isDirect: p.isDirect ?? false,
  };
}

function action(
  p: Partial<PaletteActionVm> & Pick<PaletteActionVm, "id" | "title">,
): PaletteActionVm {
  return {
    id: p.id,
    title: p.title,
    category: p.category ?? "Navigation",
    keywords: p.keywords ?? [],
    shortcut: p.shortcut ?? null,
    requiresOpenChat: p.requiresOpenChat ?? false,
    requiresRecording: p.requiresRecording ?? false,
    toggleGroup: p.toggleGroup ?? null,
  };
}

/** cmdk items must render inside a Command/CommandList context. */
function renderInList(node: React.ReactNode) {
  return render(
    <Command shouldFilter={false}>
      <CommandList>{node}</CommandList>
    </Command>,
  );
}

describe("palette rows", () => {
  it("renders a chat row with the account hue dot and network badge", () => {
    renderInList(
      <PaletteChatRow
        chat={chat({
          roomId: "!alpha",
          displayName: "Alpha Team",
          hueIndex: 3,
          network: "Telegram",
        })}
        onSelect={vi.fn()}
      />,
    );
    expect(screen.getByText("Alpha Team")).toBeInTheDocument();
    const dot = screen.getByTestId("account-hue-dot");
    expect(dot.style.backgroundColor).toBe(accountHueVar(3));
    expect(screen.getByText("Telegram")).toBeInTheDocument();
  });

  it("omits the network badge for a native room (null network)", () => {
    renderInList(
      <PaletteChatRow
        chat={chat({ roomId: "!native", displayName: "Native", network: null })}
        onSelect={vi.fn()}
      />,
    );
    expect(screen.getByText("Native")).toBeInTheDocument();
    expect(screen.queryByText("Telegram")).not.toBeInTheDocument();
  });

  it("renders an action row with its shortcut chip only when set", () => {
    const { rerender } = renderInList(
      <PaletteActionRow
        action={action({ id: "open-inbox", title: "Open Inbox", shortcut: "⌘1" })}
        onSelect={vi.fn()}
      />,
    );
    expect(screen.getByText("Open Inbox")).toBeInTheDocument();
    expect(screen.getByText("⌘1")).toBeInTheDocument();

    rerender(
      <Command shouldFilter={false}>
        <CommandList>
          <PaletteActionRow
            action={action({ id: "open-archive", title: "Open Archive", shortcut: null })}
            onSelect={vi.fn()}
          />
        </CommandList>
      </Command>,
    );
    expect(screen.getByText("Open Archive")).toBeInTheDocument();
    expect(screen.queryByText("⌘1")).not.toBeInTheDocument();
  });
});
