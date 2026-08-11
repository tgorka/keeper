import { act, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { RecordingHitVm, RecordingSearchVm } from "@/lib/ipc/client";

// Mock the typed IPC client so the pane never touches Tauri.
const searchRecordings = vi.fn();
const recordingOpenPath = vi.fn();
const revealPath = vi.fn();
vi.mock("@/lib/ipc/client", () => ({
  searchRecordings: (filter: unknown) => searchRecordings(filter),
  recordingOpenPath: (path: unknown) => recordingOpenPath(path),
  revealPath: (path: unknown) => revealPath(path),
}));

import {
  RECORDINGS_COUNT_SLOT,
  RECORDINGS_LIST_LABEL,
  RECORDINGS_PANE_TITLE,
  RECORDINGS_REFRESH_LABEL,
  RecordingsPane,
} from "@/components/recordings/recordings-pane";
import { WINDOW_ROW_ATTR, WINDOW_VIEWPORT_ATTR } from "@/components/ui/window-list";
import { capabilitiesStore, DEFAULT_CAPABILITIES } from "@/lib/stores/capabilities";
import { primaryViewStore } from "@/lib/stores/primary-view";
import { type ListGeometry, withListGeometry } from "@/test/layout";

/** The two empty-state sentences, verbatim — they are the assertion. */
const NOTHING_RECORDED = "Nothing recorded yet. Record a session and it lands here.";
const NOTHING_MATCHES = "No recordings match this filter.";

const ROOT = "/Users/alice/Movies/keeper";

function hit(p: Partial<RecordingHitVm> & Pick<RecordingHitVm, "sessionId">): RecordingHitVm {
  const relativePath = p.relativePath ?? `keeper-rec ${p.sessionId}`;
  return {
    sessionId: p.sessionId,
    relativePath,
    absolutePath: p.absolutePath ?? `${ROOT}/${relativePath}`,
    title: p.title === undefined ? null : p.title,
    startedTs: p.startedTs === undefined ? 1_700_000_000_000 : p.startedTs,
    endedTs: p.endedTs === undefined ? 1_700_000_060_000 : p.endedTs,
    durationMs: p.durationMs === undefined ? 60_000 : p.durationMs,
    totalBytes: p.totalBytes ?? 412_000_000,
    durability: p.durability ?? "local",
    tags: p.tags ?? [],
    playablePath:
      p.playablePath === undefined ? `${ROOT}/${relativePath}/screen-0001.mp4` : p.playablePath,
  };
}

/**
 * What the command resolves with: the page, and the archive-wide count behind
 * it (Story 44.11). `total` defaults to the page's length — every fixture
 * below is under the engine's 200-row page except where a test says otherwise,
 * and those are the fixtures where the two numbers must differ.
 */
function found(rows: RecordingHitVm[], total = rows.length): RecordingSearchVm {
  return { rows, total };
}

beforeEach(() => {
  searchRecordings.mockReset();
  searchRecordings.mockResolvedValue(found([]));
  recordingOpenPath.mockReset();
  recordingOpenPath.mockResolvedValue(undefined);
  revealPath.mockReset();
  revealPath.mockResolvedValue(undefined);
  // The pane only ever renders where recording is on; Reveal is its own flag.
  capabilitiesStore.getState().applySnapshot({
    ...DEFAULT_CAPABILITIES,
    recording: true,
    revealInFileManager: true,
  });
  primaryViewStore.getState().setView("recordings");
});

afterEach(() => {
  vi.clearAllMocks();
  capabilitiesStore.setState({ capabilities: DEFAULT_CAPABILITIES, hydrated: false });
  primaryViewStore.getState().setView("inbox");
});

describe("RecordingsPane", () => {
  it("names itself as a region so the shell's absence assertion has something to miss", async () => {
    render(<RecordingsPane />);
    expect(screen.getByRole("region", { name: RECORDINGS_PANE_TITLE })).toBeInTheDocument();
    // The filter row is above the fold, not behind a disclosure.
    expect(screen.getByLabelText("Search recordings")).toBeInTheDocument();
    expect(screen.getByLabelText("Start date")).toBeInTheDocument();
    expect(screen.getByLabelText("End date")).toBeInTheDocument();
    expect(screen.getByLabelText("Participant")).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Durability" })).toBeInTheDocument();
    await waitFor(() => expect(searchRecordings).toHaveBeenCalled());
  });

  it("fires one search for a burst of keystrokes, not one per keystroke", async () => {
    vi.useFakeTimers();
    try {
      render(<RecordingsPane />);
      const input = screen.getByLabelText("Search recordings");
      fireEvent.change(input, { target: { value: "s" } });
      fireEvent.change(input, { target: { value: "st" } });
      fireEvent.change(input, { target: { value: "sta" } });

      // Nothing has reached Rust yet — the debounce is the whole point.
      expect(searchRecordings).not.toHaveBeenCalled();

      await act(async () => {
        vi.advanceTimersByTime(200);
      });

      // One round trip for the word, carrying the LAST keystroke.
      expect(searchRecordings).toHaveBeenCalledTimes(1);
      expect(searchRecordings).toHaveBeenCalledWith(expect.objectContaining({ query: "sta" }));
    } finally {
      vi.runOnlyPendingTimers();
      vi.useRealTimers();
    }
  });

  it("discards a slow response that a newer query has already superseded", async () => {
    let resolveStale: (result: RecordingSearchVm) => void = () => {};
    searchRecordings.mockReturnValueOnce(
      new Promise<RecordingSearchVm>((resolve) => {
        resolveStale = resolve;
      }),
    );
    searchRecordings.mockResolvedValueOnce(
      found([hit({ sessionId: "new", title: "The newer answer" })]),
    );

    render(<RecordingsPane />);
    await waitFor(() => expect(searchRecordings).toHaveBeenCalledTimes(1));

    fireEvent.change(screen.getByLabelText("Search recordings"), { target: { value: "newer" } });
    await waitFor(() => expect(searchRecordings).toHaveBeenCalledTimes(2));
    expect(await screen.findByText("The newer answer")).toBeInTheDocument();

    // The first query finally answers — too late, and about a question nobody
    // is asking any more.
    await act(async () => {
      resolveStale(found([hit({ sessionId: "stale", title: "The stale answer" })]));
    });

    expect(screen.queryByText("The stale answer")).not.toBeInTheDocument();
    expect(screen.getByText("The newer answer")).toBeInTheDocument();
  });

  it("tells an empty archive apart from an over-narrow filter, in two sentences", async () => {
    render(<RecordingsPane />);

    // Nothing recorded: an invitation to record.
    expect(await screen.findByText(NOTHING_RECORDED)).toBeInTheDocument();
    expect(screen.queryByText(NOTHING_MATCHES)).not.toBeInTheDocument();

    // The same empty list, a different reason — and therefore a different
    // sentence, with the chips still on screen above it.
    fireEvent.change(screen.getByLabelText("Participant"), { target: { value: "nobody" } });
    expect(await screen.findByText(NOTHING_MATCHES)).toBeInTheDocument();
    expect(screen.queryByText(NOTHING_RECORDED)).not.toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Remove Participant: nobody" })).toBeInTheDocument();
  });

  it("sends you to the capture surface from the empty archive, and widens from the empty filter", async () => {
    render(<RecordingsPane />);

    fireEvent.click(await screen.findByRole("button", { name: "Go to Recording" }));
    expect(primaryViewStore.getState().view).toBe("recording");

    primaryViewStore.getState().setView("recordings");
    fireEvent.change(screen.getByLabelText("Participant"), { target: { value: "nobody" } });
    fireEvent.click(await screen.findByRole("button", { name: "Clear filters" }));

    await waitFor(() =>
      expect(searchRecordings).toHaveBeenLastCalledWith(
        expect.objectContaining({ participant: null, tags: [], query: "" }),
      ),
    );
    expect(await screen.findByText(NOTHING_RECORDED)).toBeInTheDocument();
  });

  it("narrows the list through the engine when a tag chip is added", async () => {
    searchRecordings.mockResolvedValue(
      found([
        hit({ sessionId: "s1", title: "Standup", tags: ["standup"] }),
        hit({ sessionId: "s2", title: "Retro", tags: ["retro"] }),
      ]),
    );
    render(<RecordingsPane />);
    expect(await screen.findByText("Standup")).toBeInTheDocument();

    const trigger = screen.getByRole("button", { name: "Tag" });
    fireEvent.pointerDown(trigger, { button: 0, ctrlKey: false });
    fireEvent.pointerUp(trigger, { button: 0 });
    const menu = await screen.findByRole("menu");
    fireEvent.click(await within(menu).findByRole("menuitem", { name: "standup" }));

    // The narrowing happens in Rust, through 42.2's engine — not by filtering
    // the rows already on screen.
    await waitFor(() =>
      expect(searchRecordings).toHaveBeenLastCalledWith(
        expect.objectContaining({ tags: ["standup"] }),
      ),
    );
    expect(screen.getByRole("button", { name: "Remove Tag: standup" })).toBeInTheDocument();
  });

  it("hands a row's play request the absolute media path, and reveal the session folder", async () => {
    searchRecordings.mockResolvedValue(
      found([hit({ sessionId: "s1", title: "Standup", relativePath: "2026/standup" })]),
    );
    render(<RecordingsPane />);
    expect(await screen.findByText("Standup")).toBeInTheDocument();
    expect(screen.getByRole("list", { name: RECORDINGS_LIST_LABEL })).toBeInTheDocument();

    fireEvent.click(screen.getByRole("button", { name: "Play: Standup" }));
    expect(recordingOpenPath).toHaveBeenCalledWith(`${ROOT}/2026/standup/screen-0001.mp4`);

    // Story 40.4 moves folders on a retitle; the row carries the CURRENT path,
    // so Reveal opens where the session is now and never where it used to be.
    fireEvent.click(screen.getByRole("button", { name: "Reveal in Finder: Standup" }));
    expect(revealPath).toHaveBeenCalledWith(`${ROOT}/2026/standup`);
  });

  it("drops Reveal from every row where the platform has no file manager", async () => {
    capabilitiesStore.getState().applySnapshot({
      ...DEFAULT_CAPABILITIES,
      recording: true,
      revealInFileManager: false,
    });
    searchRecordings.mockResolvedValue(
      found([hit({ sessionId: "s1", title: "Standup", relativePath: "2026/standup" })]),
    );
    render(<RecordingsPane />);
    expect(await screen.findByText("Standup")).toBeInTheDocument();

    expect(screen.queryByRole("button", { name: /Reveal in Finder/ })).not.toBeInTheDocument();
    // The path is still on screen, as text rather than as a control that refuses.
    expect(screen.getByText("2026/standup")).toBeInTheDocument();
  });

  it("re-asks the archive on Refresh, so a session recorded just now appears without a restart", async () => {
    searchRecordings.mockResolvedValueOnce(found([]));
    render(<RecordingsPane />);
    expect(await screen.findByText(NOTHING_RECORDED)).toBeInTheDocument();

    searchRecordings.mockResolvedValue(
      found([hit({ sessionId: "fresh", title: "Recorded just now" })]),
    );
    fireEvent.click(screen.getByRole("button", { name: RECORDINGS_REFRESH_LABEL }));

    expect(await screen.findByText("Recorded just now")).toBeInTheDocument();
  });

  it("surfaces a rejected query as an alert rather than an empty archive", async () => {
    searchRecordings.mockRejectedValue({
      code: "internal",
      message: "archive.db is locked",
      accountId: null,
      retriable: true,
    });
    render(<RecordingsPane />);

    const alert = await screen.findByRole("alert");
    expect(alert).toHaveTextContent("archive.db is locked");
    // A failure is not the same fact as an empty archive.
    expect(screen.queryByText(NOTHING_RECORDED)).not.toBeInTheDocument();
  });
});

/**
 * Story 44.10 — the archive, not a screenful.
 *
 * A recordings row is the one row of the three whose height genuinely varies:
 * enough tags wrap the badges onto a third line, and a machine with no Finder
 * grows a path line. So this asserts the window over a MEASURED list, where the
 * total the scrollbar reports is a running correction rather than a constant
 * times a count. `withListGeometry` is what makes any of it observable — jsdom
 * lays nothing out and would report a list that mounted all two thousand rows
 * as perfectly bounded.
 */
describe("RecordingsPane — the archive, not a screenful", () => {
  const ROW_PX = 68;
  const VISIBLE_ROWS = 10;
  const OVERSCAN = 6;

  const MANY = Array.from({ length: 2000 }, (_, index) =>
    hit({ sessionId: `s${index}`, title: `Session ${index}` }),
  );

  let geometry: ListGeometry | null = null;

  afterEach(() => {
    geometry?.undo();
    geometry = null;
  });

  function mountedRows(): number[] {
    return Array.from(document.querySelectorAll(`[${WINDOW_ROW_ATTR}]`)).map((element) =>
      Number(element.getAttribute(WINDOW_ROW_ATTR)),
    );
  }

  function viewport(): HTMLElement {
    const element = document.querySelector(`[${WINDOW_VIEWPORT_ATTR}]`);
    if (!(element instanceof HTMLElement)) {
      throw new Error("the recordings list has no scroll viewport");
    }
    return element;
  }

  async function renderArchive(): Promise<void> {
    geometry = withListGeometry({ viewport: VISIBLE_ROWS * ROW_PX, row: ROW_PX });
    searchRecordings.mockResolvedValue(found(MANY));
    render(<RecordingsPane />);
    await screen.findByRole("list", { name: RECORDINGS_LIST_LABEL }, { timeout: 2000 });
    await screen.findByText("Session 0");
  }

  it("mounts a window over two thousand sessions, not two thousand rows", async () => {
    await renderArchive();

    expect(mountedRows().length).toBeLessThanOrEqual(VISIBLE_ROWS + OVERSCAN * 2);
    expect(screen.queryByText("Session 1999")).toBeNull();
  });

  it("reaches the last session by scrolling", async () => {
    await renderArchive();

    // The total is a measured running estimate, so the bottom moves as rows are
    // seen; scrolling to whatever the list currently claims is the bottom is
    // what a person with a scrollbar actually does.
    for (let attempt = 0; attempt < 12 && screen.queryByText("Session 1999") === null; attempt++) {
      const height = Number.parseFloat(
        screen.getByRole("list", { name: RECORDINGS_LIST_LABEL }).style.height,
      );
      act(() => geometry?.scrollTo(viewport(), height - VISIBLE_ROWS * ROW_PX));
    }

    expect(screen.getByText("Session 1999")).toBeInTheDocument();
    expect(mountedRows().length).toBeLessThanOrEqual(VISIBLE_ROWS + OVERSCAN * 2);
  });

  /**
   * Story 44.11, and this is the AC's own shape: the count is asserted with
   * virtualisation ON, over a fixture two orders of magnitude larger than one
   * window. Two thousand sessions, about twenty rows in the DOM, and the header
   * says two thousand.
   */
  it("says how many sessions exist, not how many rows the window mounted", async () => {
    await renderArchive();

    expect(mountedRows().length).toBeLessThanOrEqual(VISIBLE_ROWS + OVERSCAN * 2);
    expect(count()).toBe(`${(2000).toLocaleString()} sessions`);
  });

  /**
   * The trap the backend change exists for. The engine's page stops at 200, so
   * an archive of two thousand hands back two hundred rows — and a count taken
   * from the array would read `200 sessions` on a machine with ten times that.
   */
  it("says the archive's count even when the page it was sent is smaller", async () => {
    geometry = withListGeometry({ viewport: VISIBLE_ROWS * ROW_PX, row: ROW_PX });
    searchRecordings.mockResolvedValue(found(MANY.slice(0, 200), 2000));
    render(<RecordingsPane />);
    await screen.findByText("Session 0");

    expect(count()).toBe(`${(2000).toLocaleString()} sessions`);
  });
});

/** The count the pane's header shows, or `null` while it shows none. */
function count(): string | null {
  return document.querySelector(`[data-slot="${RECORDINGS_COUNT_SLOT}"]`)?.textContent ?? null;
}

describe("RecordingsPane — how many sessions", () => {
  it("says zero rather than hiding the count when nothing matches", async () => {
    render(<RecordingsPane />);
    await screen.findByText(NOTHING_RECORDED);

    // The empty state replaces the LIST; the count sits in the header, which is
    // rendered in every state. A count that vanished exactly when the answer is
    // "none" would be a count that never answers the question it was asked.
    expect(count()).toBe("0 sessions");
  });

  it("counts the filtered set, and moves when the filter does", async () => {
    searchRecordings.mockResolvedValue(
      found([
        hit({ sessionId: "s1", title: "Standup", tags: ["standup"] }),
        hit({ sessionId: "s2", title: "Retro", tags: ["retro"] }),
      ]),
    );
    render(<RecordingsPane />);
    await screen.findByText("Standup");
    expect(count()).toBe("2 sessions");

    searchRecordings.mockResolvedValue(
      found([hit({ sessionId: "s1", title: "Standup", tags: ["standup"] })]),
    );
    fireEvent.change(screen.getByLabelText("Participant"), { target: { value: "ada" } });

    await waitFor(() => expect(count()).toBe("1 session"));
  });

  it("says nothing at all before the first answer lands", () => {
    // `0 sessions` before a query has run is a claim nobody has checked. The
    // count appears with the answer, not with the pane.
    render(<RecordingsPane />);

    expect(count()).toBeNull();
  });

  it("groups a five-digit archive rather than setting it solid", async () => {
    searchRecordings.mockResolvedValue(found([hit({ sessionId: "s1" })], 12_345));
    render(<RecordingsPane />);

    await waitFor(() => expect(count()).toBe(`${(12_345).toLocaleString()} sessions`));
  });
});
