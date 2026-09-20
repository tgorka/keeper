import { act, cleanup, fireEvent, render, screen, within } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import { Kbd } from "@/components/ui/kbd";
import {
  HOVER_HINT_DELAY_MS,
  HoverHint,
  IconHint,
  Tooltip,
  TooltipContent,
  TooltipProvider,
  TooltipTrigger,
} from "@/components/ui/tooltip";

afterEach(() => {
  cleanup();
  vi.useRealTimers();
});

describe("readable hints", () => {
  it("waits the chosen delay on every hover even under an instant root provider", () => {
    vi.useFakeTimers();
    render(
      <TooltipProvider delayDuration={0}>
        <IconHint label="Refresh">
          <button type="button" aria-label="Refresh">
            ↻
          </button>
        </IconHint>
        <IconHint label="Delete">
          <button type="button" aria-label="Delete">
            ×
          </button>
        </IconHint>
      </TooltipProvider>,
    );
    const refresh = screen.getByRole("button", { name: "Refresh" });
    fireEvent.pointerMove(refresh, { pointerType: "mouse" });
    act(() => vi.advanceTimersByTime(HOVER_HINT_DELAY_MS - 1));
    expect(screen.queryByRole("tooltip")).toBeNull();
    act(() => vi.advanceTimersByTime(1));
    expect(screen.getByRole("tooltip")).toHaveTextContent("Refresh");
    fireEvent.pointerLeave(refresh);
    fireEvent.keyDown(refresh, { key: "Escape" });
    fireEvent.pointerMove(screen.getByRole("button", { name: "Delete" }), { pointerType: "mouse" });
    act(() => vi.advanceTimersByTime(HOVER_HINT_DELAY_MS - 1));
    expect(screen.queryByRole("tooltip")).toBeNull();
    act(() => vi.advanceTimersByTime(1));
    expect(screen.getByRole("tooltip")).toHaveTextContent("Delete");
  });

  it("opens the complete row name and bounded plain detail on keyboard focus", () => {
    const label = "A complete title that the narrow row cannot possibly fit on one line";
    const detail = "First line. Second line. Third line. Fourth line. **Rust** <b>plain text</b>";
    render(
      <HoverHint label={label} detail={detail}>
        <button type="button">
          <span className="truncate">{label}</span>
        </button>
      </HoverHint>,
    );
    expect(screen.queryByRole("tooltip")).toBeNull();
    act(() => screen.getByRole("button").focus());
    const hint = screen.getByRole("tooltip");
    expect(hint).toHaveTextContent(label);
    expect(document.querySelector('[data-slot="tooltip-content"]')).toHaveClass("max-w-xs");
    expect(document.querySelector('[data-slot="tooltip-content"]')).toHaveClass(
      "bg-popover",
      "text-popover-foreground",
      "ring-1",
      "ring-foreground/10",
    );
    expect(document.querySelector('[data-slot="tooltip-content"]')).not.toHaveClass(
      "bg-foreground",
    );
    expect(within(hint).getByText(detail)).toHaveClass("line-clamp-3");
    expect(within(hint).getByText(label)).not.toHaveClass("truncate");
    expect(hint.querySelector("b")).toBeNull();
    fireEvent.keyDown(screen.getByRole("button"), { key: "Escape" });
    expect(screen.queryByRole("tooltip")).toBeNull();
  });

  it("keeps a label-only icon hint operable without adding a tab stop", () => {
    const select = vi.fn();
    render(
      <IconHint label="Open">
        <button type="button" aria-label="Open" onClick={select}>
          ↗
        </button>
      </IconHint>,
    );
    act(() => screen.getByRole("button", { name: "Open" }).focus());
    expect(screen.getByRole("button", { name: "Open" })).toHaveFocus();
    expect(screen.getByRole("tooltip")).toHaveTextContent("Open");
    fireEvent.click(screen.getByRole("button", { name: "Open" }));
    expect(select).toHaveBeenCalledOnce();
  });
});

it("keeps keyboard hints on the muted surface inside the popover tooltip", () => {
  render(
    <TooltipProvider>
      <Tooltip open>
        <TooltipTrigger>Shortcut</TooltipTrigger>
        <TooltipContent>
          <Kbd>Esc</Kbd>
        </TooltipContent>
      </Tooltip>
    </TooltipProvider>,
  );
  const key = document.querySelector('[data-slot="tooltip-content"] [data-slot="kbd"]');
  expect(key).toHaveClass("bg-muted", "text-muted-foreground");
  expect(key?.className).not.toContain("bg-background/");
  expect(key?.className).not.toContain(":text-background");
});
