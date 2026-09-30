import { fireEvent, render, screen, within } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import {
  type LineVm,
  lineLength,
  TranscriptLines,
  useTranscriptRows,
} from "@/components/transcription/transcript-lines";

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

function Lines({
  current,
  playable,
  onSeek,
}: {
  current: number;
  playable: boolean;
  onSeek: (seconds: number, play?: boolean) => void;
}) {
  const list = useTranscriptRows(lines);
  return (
    <TranscriptLines
      lines={lines}
      speakers={speakers}
      list={list}
      current={current}
      playable={playable}
      onSeek={onSeek}
      menu={(line: LineVm) => <span>menu {line.id}</span>}
    />
  );
}

describe("The lines", () => {
  it("mark the line being said, say start and length, and play from the speaker's square", () => {
    const onSeek = vi.fn();
    render(<Lines current={1} playable onSeek={onSeek} />);
    const second = screen.getByText("menu u2").closest("li");
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
  });

  it("draws plain squares and plain times where nothing can be played", () => {
    render(<Lines current={-1} playable={false} onSeek={vi.fn()} />);
    expect(screen.queryByRole("button", { name: /^Play from/ })).toBeNull();
    expect(screen.queryByRole("button", { name: /^Go to/ })).toBeNull();
  });
});
