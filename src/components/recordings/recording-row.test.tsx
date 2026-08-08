import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import type { RecordingHitVm } from "@/lib/ipc/client";

// The row never touches Tauri: it calls back into the pane, which owns the IPC.
// The client is still mocked because the module graph reaches it.
vi.mock("@/lib/ipc/client", () => ({
  searchRecordings: vi.fn(),
  recordingOpenPath: vi.fn(),
  revealPath: vi.fn(),
}));

import {
  DURABILITY_LOCAL_LABEL,
  DURABILITY_PUSHED_LABEL,
} from "@/components/recording/active-recording-banner";
import {
  RECORDINGS_COPIED_LABEL,
  RECORDINGS_COPY_ID_LABEL,
  RECORDINGS_PLAY_LABEL,
  RECORDINGS_REVEAL_LABEL,
  RECORDINGS_ROW_DURABILITY_TESTID,
  RecordingRow,
} from "@/components/recordings/recording-row";

const ROOT = "/Users/alice/Movies/keeper";

/** A session that started at a fixed instant, so the date line is stable. */
const STARTED_TS = 1_700_000_000_000;

function hit(p: Partial<RecordingHitVm> & Pick<RecordingHitVm, "sessionId">): RecordingHitVm {
  const relativePath = p.relativePath ?? "2026/keeper-rec 2026-07-19 14.23.45";
  return {
    sessionId: p.sessionId,
    relativePath,
    absolutePath: p.absolutePath ?? `${ROOT}/${relativePath}`,
    title: p.title === undefined ? null : p.title,
    startedTs: p.startedTs === undefined ? STARTED_TS : p.startedTs,
    endedTs: p.endedTs === undefined ? STARTED_TS + 60_000 : p.endedTs,
    durationMs: p.durationMs === undefined ? 60_000 : p.durationMs,
    totalBytes: p.totalBytes ?? 412_000_000,
    durability: p.durability ?? "local",
    tags: p.tags ?? [],
    playablePath:
      p.playablePath === undefined ? `${ROOT}/${relativePath}/screen-0001.mp4` : p.playablePath,
  };
}

function renderRow(
  vm: RecordingHitVm,
  overrides: { canReveal?: boolean; onReveal?: () => void; onPlay?: () => void } = {},
) {
  // A row is an `<li>`; give it the list it belongs to so the DOM is legal.
  return render(
    <ul>
      <RecordingRow
        hit={vm}
        canReveal={overrides.canReveal ?? true}
        onReveal={overrides.onReveal ?? vi.fn()}
        onPlay={overrides.onPlay ?? vi.fn()}
      />
    </ul>,
  );
}

beforeEach(() => {
  // jsdom lacks a clipboard by default.
  Object.assign(navigator, {
    clipboard: { writeText: vi.fn(() => Promise.resolve()) },
  });
});

describe("RecordingRow", () => {
  it("names an untitled session by its date and folder rather than leaving a blank line", () => {
    renderRow(hit({ sessionId: "s1", title: null, relativePath: "2026/standup-2026-07-19" }));

    const dateLabel = new Date(STARTED_TS).toLocaleString();
    expect(screen.getByText(`${dateLabel} · standup-2026-07-19`)).toBeInTheDocument();
    // And the date is not then repeated on the meta line beneath it.
    expect(screen.getByText(`1:00 · 412 MB`)).toBeInTheDocument();
  });

  it("keeps a titled session's date on the meta line beside its duration and size", () => {
    renderRow(hit({ sessionId: "s1", title: "Standup", totalBytes: 1_290_000_000 }));

    expect(screen.getByText("Standup")).toBeInTheDocument();
    expect(
      screen.getByText(`${new Date(STARTED_TS).toLocaleString()} · 1:00 · 1.2 GB`),
    ).toBeInTheDocument();
  });

  it("says the date is unknown rather than inventing one for a stampless manifest", () => {
    renderRow(hit({ sessionId: "s1", title: null, startedTs: null, durationMs: null }));

    expect(screen.getByText("Date unknown · keeper-rec 2026-07-19 14.23.45")).toBeInTheDocument();
    // No duration to print either — the meta line is the size alone.
    expect(screen.getByText("412 MB")).toBeInTheDocument();
  });

  it("renders each stored tag as its own chip, exactly as stored", () => {
    renderRow(hit({ sessionId: "s1", title: "Standup", tags: ["work/standup", "q3"] }));

    expect(screen.getByText("work/standup")).toBeInTheDocument();
    expect(screen.getByText("q3")).toBeInTheDocument();
  });

  it("carries epic 41's own durability word beside the glyph, never a second vocabulary", () => {
    const { unmount } = renderRow(hit({ sessionId: "s1", title: "Standup", durability: "local" }));
    expect(screen.getByTestId(RECORDINGS_ROW_DURABILITY_TESTID)).toHaveTextContent(
      DURABILITY_LOCAL_LABEL,
    );
    unmount();

    // `verified` reads the same as `pushed`: the recording is on the drive either way.
    renderRow(hit({ sessionId: "s2", title: "Retro", durability: "verified" }));
    expect(screen.getByTestId(RECORDINGS_ROW_DURABILITY_TESTID)).toHaveTextContent(
      DURABILITY_PUSHED_LABEL,
    );
  });

  it("prints no durability word at all for a state it does not know", () => {
    renderRow(hit({ sessionId: "s1", title: "Standup", durability: "teleported" }));

    expect(screen.queryByText("teleported")).not.toBeInTheDocument();
    expect(screen.queryByTestId(RECORDINGS_ROW_DURABILITY_TESTID)).not.toBeInTheDocument();
  });

  it("drops Reveal where there is no file manager and shows the path as inert text", () => {
    renderRow(hit({ sessionId: "s1", title: "Standup", relativePath: "2026/standup" }), {
      canReveal: false,
    });

    expect(
      screen.queryByRole("button", { name: new RegExp(RECORDINGS_REVEAL_LABEL) }),
    ).not.toBeInTheDocument();
    // The path is still worth knowing — as text, not as a control that refuses.
    const path = screen.getByText("2026/standup");
    expect(path).toBeInTheDocument();
    expect(path.tagName).toBe("SPAN");
  });

  it("drops Play for a session with no media file rather than opening a folder", () => {
    renderRow(hit({ sessionId: "s1", title: "Standup", playablePath: null }));

    expect(
      screen.queryByRole("button", { name: new RegExp(RECORDINGS_PLAY_LABEL) }),
    ).not.toBeInTheDocument();
    // The other two actions are unaffected.
    expect(
      screen.getByRole("button", { name: `${RECORDINGS_REVEAL_LABEL}: Standup` }),
    ).toBeInTheDocument();
  });

  it("hands the whole hit back to the pane for Play and Reveal", () => {
    const onPlay = vi.fn();
    const onReveal = vi.fn();
    const vm = hit({ sessionId: "s1", title: "Standup" });
    renderRow(vm, { onPlay, onReveal });

    fireEvent.click(screen.getByRole("button", { name: `${RECORDINGS_PLAY_LABEL}: Standup` }));
    expect(onPlay).toHaveBeenCalledWith(vm);

    fireEvent.click(screen.getByRole("button", { name: `${RECORDINGS_REVEAL_LABEL}: Standup` }));
    expect(onReveal).toHaveBeenCalledWith(vm);
  });

  it("puts the immutable session id on the clipboard and confirms it transiently", async () => {
    renderRow(hit({ sessionId: "01JABCDEF", title: "Standup" }));

    fireEvent.click(screen.getByRole("button", { name: `${RECORDINGS_COPY_ID_LABEL}: Standup` }));

    expect(navigator.clipboard.writeText).toHaveBeenCalledWith("01JABCDEF");
    expect(await screen.findByText(RECORDINGS_COPIED_LABEL)).toBeInTheDocument();
  });

  it("swallows a clipboard the browser refuses rather than surfacing an error", async () => {
    Object.assign(navigator, {
      clipboard: { writeText: vi.fn(() => Promise.reject(new Error("denied"))) },
    });
    renderRow(hit({ sessionId: "01JABCDEF", title: "Standup" }));

    fireEvent.click(screen.getByRole("button", { name: `${RECORDINGS_COPY_ID_LABEL}: Standup` }));

    await waitFor(() => expect(navigator.clipboard.writeText).toHaveBeenCalled());
    // No confirmation, no alert — the id simply did not make it.
    expect(screen.queryByText(RECORDINGS_COPIED_LABEL)).not.toBeInTheDocument();
    expect(screen.queryByRole("alert")).not.toBeInTheDocument();
  });
});
