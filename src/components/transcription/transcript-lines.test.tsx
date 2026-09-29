import { fireEvent, render, screen, within } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import { lineLength, TranscriptLinesBox } from "@/components/transcription/transcript-lines";

const lines = [
  { id: "u1", speaker: "ME", start: 0, end: 5, text: "Kowalsky will join us." },
  { id: "u2", speaker: "S1", start: 72, end: 137.4, text: "Let’s review the release." },
];
const speakers = [
  { id: "ME", name: null },
  { id: "S1", name: "Anna" },
];

describe("A line's length", () => {
  it("reads in seconds under a minute and in minutes and padded seconds from one", () => {
    expect(lineLength(21.4)).toBe("21 s");
    expect(lineLength(59.6)).toBe("1 min 00 s");
    expect(lineLength(65)).toBe("1 min 05 s");
    expect(lineLength(-1)).toBe("0 s");
  });
});

describe("The lines, read-only", () => {
  it("offers only playing, a clip and the viewer, and plays from the speaker's square", async () => {
    const onSeek = vi.fn();
    const onCopyClip = vi.fn();
    const onOpenInViewer = vi.fn();
    render(
      <TranscriptLinesBox
        lines={lines}
        speakers={speakers}
        current={1}
        playable
        onSeek={onSeek}
        onCopyClip={onCopyClip}
        onOpenInViewer={onOpenInViewer}
      />,
    );
    const second = screen.getByRole("button", { name: "Line actions u2" }).closest("li");
    expect(second).toHaveAttribute("aria-current", "true");
    expect(
      within(second as HTMLElement).getByText(
        (_, element) => element?.textContent === "00:01:12 · 1 min 05 s",
      ),
    ).toBeInTheDocument();
    expect(within(second as HTMLElement).getByText("Anna")).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "Play from 00:01:12" }));
    expect(onSeek).toHaveBeenLastCalledWith(72, true);
    fireEvent.click(screen.getByRole("button", { name: "Go to 00:00:00, u1" }));
    expect(onSeek).toHaveBeenLastCalledWith(0);

    fireEvent.keyDown(screen.getByRole("button", { name: "Line actions u1" }), { key: "Enter" });
    const menu = await screen.findByRole("menu");
    expect(
      within(menu)
        .getAllByRole("menuitem")
        .map((item) => item.textContent),
    ).toEqual(["Play from here", "Copy clip from here…", "Open in viewer"]);
    fireEvent.click(within(menu).getByRole("menuitem", { name: "Copy clip from here…" }));
    expect(onCopyClip).toHaveBeenCalledWith(lines[0]);
  });

  it("draws plain squares and no Play where nothing can be played", () => {
    render(
      <TranscriptLinesBox
        lines={lines}
        speakers={speakers}
        current={-1}
        playable={false}
        onSeek={vi.fn()}
      />,
    );
    expect(screen.queryByRole("button", { name: /^Play from/ })).toBeNull();
    // No handler and nothing to play: the ⋯ would open an empty menu, so it is not drawn.
    expect(screen.queryByRole("button", { name: /^Line actions/ })).toBeNull();
  });
});
