import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

vi.mock("@/lib/ipc/client", () => ({
  revealPath: vi.fn(() => Promise.resolve()),
  recordingRetitle: vi.fn(),
}));

import {
  RECOVERY_DISMISS_LABEL,
  REVEAL_IN_FINDER_LABEL,
  RecordingSummaryCard,
  SUMMARY_FOLDER_TESTID,
  SUMMARY_RETITLE_CANCEL_TESTID,
  SUMMARY_RETITLE_EDIT_TESTID,
  SUMMARY_RETITLE_FAULT_TESTID,
  SUMMARY_RETITLE_FIELD_TESTID,
  SUMMARY_RETITLE_LABEL,
  SUMMARY_RETITLE_SAVE_TESTID,
  SUMMARY_RETITLE_UNTITLED_LABEL,
} from "@/components/layout/recording-summary-card";
import type { RecordingSummaryVm } from "@/lib/ipc/client";
import { recordingRetitle, revealPath } from "@/lib/ipc/client";
import { capabilitiesStore, DEFAULT_CAPABILITIES } from "@/lib/stores/capabilities";

const mockReveal = vi.mocked(revealPath);
const mockRetitle = vi.mocked(recordingRetitle);

const FOLDER = "/Users/alice/Movies/keeper/keeper-rec 2026-07-19 14.23.45";

/** Where the rename lands: the template re-rendered with the new title. */
const MOVED_FOLDER = "/Users/alice/Movies/keeper/2026/2026-07-19 1423 standup";

/** The summary the command resolves — the session AT ITS NEW LOCATION. */
const MOVED: RecordingSummaryVm = {
  sessionFolder: MOVED_FOLDER,
  screenSegmentCount: 3,
  title: "Standup",
  totalBytes: 412_000_000,
};

beforeEach(() => {
  mockReveal.mockReset();
  mockReveal.mockResolvedValue(undefined);
  mockRetitle.mockReset();
  mockRetitle.mockResolvedValue(MOVED);
  // Reveal is capability-gated: default it ON for the base cases.
  capabilitiesStore
    .getState()
    .applySnapshot({ ...DEFAULT_CAPABILITIES, revealInFileManager: true });
});

afterEach(() => {
  capabilitiesStore.setState({ capabilities: DEFAULT_CAPABILITIES, hydrated: false });
  vi.clearAllMocks();
});

describe("RecordingSummaryCard", () => {
  it("renders the completion variant: count, size, folder, and Reveal", () => {
    render(
      <RecordingSummaryCard
        variant="completion"
        sessionFolder={FOLDER}
        screenSegmentCount={3}
        totalBytes={412_000_000}
      />,
    );

    expect(screen.getByText(/Saved 3 segments · 412 MB/)).toBeInTheDocument();
    expect(screen.getByText(FOLDER)).toBeInTheDocument();
    const reveal = screen.getByRole("button", { name: REVEAL_IN_FINDER_LABEL });
    fireEvent.click(reveal);
    expect(mockReveal).toHaveBeenCalledWith(FOLDER);
    // No dismiss on the completion variant (a finalized session is never dismissed).
    expect(screen.queryByRole("button", { name: RECOVERY_DISMISS_LABEL })).not.toBeInTheDocument();
  });

  it("says '1 segment' (singular) for a single-segment session", () => {
    render(
      <RecordingSummaryCard
        variant="completion"
        sessionFolder={FOLDER}
        screenSegmentCount={1}
        totalBytes={1_000_000}
      />,
    );
    expect(screen.getByText(/Saved 1 segment · 1 MB/)).toBeInTheDocument();
  });

  it("renders the recovered variant: interruption copy, warning edge, and Dismiss", () => {
    const onDismiss = vi.fn();
    render(
      <RecordingSummaryCard
        variant="recovered"
        sessionFolder={FOLDER}
        screenSegmentCount={2}
        totalBytes={200_000_000}
        onDismiss={onDismiss}
      />,
    );

    expect(
      screen.getByText(/A recording was interrupted; 2 segments were saved/),
    ).toBeInTheDocument();
    // The bridge-degraded warning edge (the bridge-card recipe). The card root,
    // not the live region — the announced outcome is a non-interactive block
    // inside it now that the card carries a rename editor.
    const card = screen.getByText(/A recording was interrupted/).closest("[data-slot='card']");
    expect(card?.className).toContain("border-bridge-degraded/50");
    expect(card?.className).toContain("text-bridge-degraded");

    fireEvent.click(screen.getByRole("button", { name: RECOVERY_DISMISS_LABEL }));
    expect(onDismiss).toHaveBeenCalledTimes(1);
  });

  it("degrades to a figureless headline (never '0 segments · 0 MB') when the summary is unavailable", () => {
    render(
      <RecordingSummaryCard
        variant="completion"
        sessionFolder={FOLDER}
        screenSegmentCount={null}
        totalBytes={null}
      />,
    );
    // The honest degraded shape: outcome + folder + Reveal, no fabricated zero.
    expect(screen.getByText("Recording saved")).toBeInTheDocument();
    expect(screen.queryByText(/0 segments/)).not.toBeInTheDocument();
    expect(screen.queryByText(/0 MB/)).not.toBeInTheDocument();
    expect(screen.getByText(FOLDER)).toBeInTheDocument();
    expect(screen.getByRole("button", { name: REVEAL_IN_FINDER_LABEL })).toBeInTheDocument();
  });

  it("degrades the recovered variant to the interruption headline without fabricated figures", () => {
    render(
      <RecordingSummaryCard
        variant="recovered"
        sessionFolder={FOLDER}
        screenSegmentCount={null}
        totalBytes={null}
        onDismiss={vi.fn()}
      />,
    );
    expect(screen.getByText("A recording was interrupted")).toBeInTheDocument();
    expect(screen.queryByText(/0 segments/)).not.toBeInTheDocument();
  });

  it("hides the Reveal button when revealInFileManager is false", () => {
    capabilitiesStore
      .getState()
      .applySnapshot({ ...DEFAULT_CAPABILITIES, revealInFileManager: false });
    render(
      <RecordingSummaryCard
        variant="completion"
        sessionFolder={FOLDER}
        screenSegmentCount={1}
        totalBytes={1_000_000}
      />,
    );
    expect(screen.queryByRole("button", { name: REVEAL_IN_FINDER_LABEL })).not.toBeInTheDocument();
  });

  it("saves a typed title through recordingRetitle with the session's folder", async () => {
    const onRetitled = vi.fn();
    render(
      <RecordingSummaryCard
        variant="completion"
        sessionFolder={FOLDER}
        screenSegmentCount={3}
        totalBytes={412_000_000}
        onRetitled={onRetitled}
      />,
    );

    // An untitled session invites a name rather than offering a verb.
    fireEvent.click(screen.getByText(SUMMARY_RETITLE_UNTITLED_LABEL));
    fireEvent.change(screen.getByTestId(SUMMARY_RETITLE_FIELD_TESTID), {
      target: { value: "Standup" },
    });
    fireEvent.click(screen.getByTestId(SUMMARY_RETITLE_SAVE_TESTID));

    await waitFor(() => expect(mockRetitle).toHaveBeenCalledWith(FOLDER, "Standup"));
    await waitFor(() => expect(onRetitled).toHaveBeenCalledWith(MOVED));
  });

  it("re-renders the folder and title the rename RESOLVED, and reveals the new path", async () => {
    render(
      <RecordingSummaryCard
        variant="completion"
        sessionFolder={FOLDER}
        screenSegmentCount={3}
        totalBytes={412_000_000}
      />,
    );

    fireEvent.click(screen.getByTestId(SUMMARY_RETITLE_EDIT_TESTID));
    fireEvent.change(screen.getByTestId(SUMMARY_RETITLE_FIELD_TESTID), {
      target: { value: "Standup" },
    });
    fireEvent.click(screen.getByTestId(SUMMARY_RETITLE_SAVE_TESTID));

    // The old path is gone from disk, so it must be gone from the card: the
    // mono line — and Reveal in Finder, which points at it — is the NEW one.
    await waitFor(() =>
      expect(screen.getByTestId(SUMMARY_FOLDER_TESTID).textContent).toBe(MOVED_FOLDER),
    );
    expect(screen.queryByText(FOLDER)).not.toBeInTheDocument();
    expect(screen.getByText("Standup")).toBeInTheDocument();
    // The editor closed on success, so the affordance reads as a rename now.
    expect(screen.getByText(SUMMARY_RETITLE_LABEL)).toBeInTheDocument();

    fireEvent.click(screen.getByRole("button", { name: REVEAL_IN_FINDER_LABEL }));
    expect(mockReveal).toHaveBeenCalledWith(MOVED_FOLDER);
  });

  it("prints a live-session refusal verbatim and keeps the typed text", async () => {
    mockRetitle.mockRejectedValue({
      code: "recordingSessionLive",
      message: "This recording is still running. Stop it before renaming the session.",
      accountId: null,
      retriable: false,
    });
    render(
      <RecordingSummaryCard
        variant="completion"
        sessionFolder={FOLDER}
        screenSegmentCount={3}
        totalBytes={412_000_000}
      />,
    );

    fireEvent.click(screen.getByTestId(SUMMARY_RETITLE_EDIT_TESTID));
    fireEvent.change(screen.getByTestId(SUMMARY_RETITLE_FIELD_TESTID), {
      target: { value: "Standup" },
    });
    fireEvent.click(screen.getByTestId(SUMMARY_RETITLE_SAVE_TESTID));

    await waitFor(() =>
      expect(screen.getByTestId(SUMMARY_RETITLE_FAULT_TESTID).textContent).toBe(
        "This recording is still running. Stop it before renaming the session.",
      ),
    );
    // The words the refusal is about are still there to be corrected, and the
    // session has not moved.
    expect(screen.getByTestId(SUMMARY_RETITLE_FIELD_TESTID)).toHaveValue("Standup");
    expect(screen.getByTestId(SUMMARY_FOLDER_TESTID).textContent).toBe(FOLDER);

    // Cancel retracts the refusal along with the edit.
    fireEvent.click(screen.getByTestId(SUMMARY_RETITLE_CANCEL_TESTID));
    expect(screen.queryByTestId(SUMMARY_RETITLE_FAULT_TESTID)).not.toBeInTheDocument();
  });

  it("disables Save while the rename is in flight", async () => {
    // The rename is held open so the button can be observed mid-flight. The
    // executor form, not `Promise.withResolvers`: the project compiles against
    // `lib: ES2020`, where that constructor method does not exist.
    let land!: (summary: RecordingSummaryVm) => void;
    mockRetitle.mockReturnValue(
      new Promise<RecordingSummaryVm>((resolve) => {
        land = resolve;
      }),
    );
    render(
      <RecordingSummaryCard
        variant="completion"
        sessionFolder={FOLDER}
        screenSegmentCount={3}
        totalBytes={412_000_000}
      />,
    );

    fireEvent.click(screen.getByTestId(SUMMARY_RETITLE_EDIT_TESTID));
    fireEvent.change(screen.getByTestId(SUMMARY_RETITLE_FIELD_TESTID), {
      target: { value: "Standup" },
    });
    fireEvent.click(screen.getByTestId(SUMMARY_RETITLE_SAVE_TESTID));

    await waitFor(() => expect(screen.getByTestId(SUMMARY_RETITLE_SAVE_TESTID)).toBeDisabled());
    expect(mockRetitle).toHaveBeenCalledTimes(1);
    // A second click while the first is in flight sends nothing.
    fireEvent.click(screen.getByTestId(SUMMARY_RETITLE_SAVE_TESTID));
    expect(mockRetitle).toHaveBeenCalledTimes(1);

    land(MOVED);
    await waitFor(() =>
      expect(screen.getByTestId(SUMMARY_FOLDER_TESTID).textContent).toBe(MOVED_FOLDER),
    );
  });

  it("disables Save while the text is unchanged, and enables it on a real edit", () => {
    render(
      <RecordingSummaryCard
        variant="completion"
        sessionFolder={FOLDER}
        title="Standup"
        screenSegmentCount={3}
        totalBytes={412_000_000}
      />,
    );

    fireEvent.click(screen.getByTestId(SUMMARY_RETITLE_EDIT_TESTID));
    // The editor opens seeded with the current title — nothing to save yet.
    expect(screen.getByTestId(SUMMARY_RETITLE_FIELD_TESTID)).toHaveValue("Standup");
    expect(screen.getByTestId(SUMMARY_RETITLE_SAVE_TESTID)).toBeDisabled();

    // Whitespace around the same title is the same title.
    fireEvent.change(screen.getByTestId(SUMMARY_RETITLE_FIELD_TESTID), {
      target: { value: "  Standup  " },
    });
    expect(screen.getByTestId(SUMMARY_RETITLE_SAVE_TESTID)).toBeDisabled();

    fireEvent.change(screen.getByTestId(SUMMARY_RETITLE_FIELD_TESTID), {
      target: { value: "Retro" },
    });
    expect(screen.getByTestId(SUMMARY_RETITLE_SAVE_TESTID)).toBeEnabled();

    // Clearing a titled session IS an edit (it moves back to the untitled path).
    fireEvent.change(screen.getByTestId(SUMMARY_RETITLE_FIELD_TESTID), {
      target: { value: "" },
    });
    expect(screen.getByTestId(SUMMARY_RETITLE_SAVE_TESTID)).toBeEnabled();
  });

  it("dismisses the folder the session moved to, not the path the rename invalidated", async () => {
    // A dismissal is keyed off the manifest at the folder it is handed, and the
    // pre-rename folder no longer has one — latching there is a silent no-op
    // and the card returns on the next scan.
    const onDismiss = vi.fn();
    render(
      <RecordingSummaryCard
        variant="recovered"
        sessionFolder={FOLDER}
        screenSegmentCount={2}
        totalBytes={200_000_000}
        onDismiss={onDismiss}
      />,
    );

    fireEvent.click(screen.getByTestId(SUMMARY_RETITLE_EDIT_TESTID));
    fireEvent.change(screen.getByTestId(SUMMARY_RETITLE_FIELD_TESTID), {
      target: { value: "Standup" },
    });
    fireEvent.click(screen.getByTestId(SUMMARY_RETITLE_SAVE_TESTID));
    await waitFor(() =>
      expect(screen.getByTestId(SUMMARY_FOLDER_TESTID).textContent).toBe(MOVED_FOLDER),
    );

    fireEvent.click(screen.getByRole("button", { name: RECOVERY_DISMISS_LABEL }));
    expect(onDismiss).toHaveBeenCalledWith(MOVED_FOLDER);
  });

  it("holds the renamed session's folder and title while the owner adopts the move", async () => {
    const { rerender } = render(
      <RecordingSummaryCard
        variant="completion"
        sessionFolder={FOLDER}
        screenSegmentCount={3}
        totalBytes={412_000_000}
      />,
    );

    fireEvent.click(screen.getByTestId(SUMMARY_RETITLE_EDIT_TESTID));
    fireEvent.change(screen.getByTestId(SUMMARY_RETITLE_FIELD_TESTID), {
      target: { value: "Standup" },
    });
    fireEvent.click(screen.getByTestId(SUMMARY_RETITLE_SAVE_TESTID));
    await waitFor(() => expect(screen.getByText("Standup")).toBeInTheDocument());

    // The owner adopts the new folder and re-fetches its summary, so the title
    // and figures are momentarily absent again. The card must not flash back to
    // "untitled with no figures" for a session it just renamed.
    rerender(
      <RecordingSummaryCard
        variant="completion"
        sessionFolder={MOVED_FOLDER}
        title={null}
        screenSegmentCount={null}
        totalBytes={null}
      />,
    );
    expect(screen.getByText("Standup")).toBeInTheDocument();
    expect(screen.getByText(/Saved 3 segments · 412 MB/)).toBeInTheDocument();

    // A DIFFERENT session in the same slot retires the override entirely.
    rerender(
      <RecordingSummaryCard
        variant="completion"
        sessionFolder="/Users/alice/Movies/keeper/keeper-rec 2026-07-20 09.00.00"
        title={null}
        screenSegmentCount={1}
        totalBytes={1_000_000}
      />,
    );
    await waitFor(() => expect(screen.queryByText("Standup")).not.toBeInTheDocument());
    expect(screen.getByTestId(SUMMARY_FOLDER_TESTID).textContent).toBe(
      "/Users/alice/Movies/keeper/keeper-rec 2026-07-20 09.00.00",
    );
  });

  it("re-seeds the editor when the title lands, so an empty draft cannot clear it", async () => {
    // The completion card's title arrives one IPC round trip after the terminal
    // settles. An editor opened inside that window shows an empty field for a
    // session that HAS a title, and saving it would clear the title and move the
    // folder back to the untitled path.
    const { rerender } = render(
      <RecordingSummaryCard
        variant="completion"
        sessionFolder={FOLDER}
        title={null}
        screenSegmentCount={null}
        totalBytes={null}
      />,
    );

    fireEvent.click(screen.getByTestId(SUMMARY_RETITLE_EDIT_TESTID));
    expect(screen.getByTestId(SUMMARY_RETITLE_FIELD_TESTID)).toHaveValue("");
    expect(screen.getByTestId(SUMMARY_RETITLE_SAVE_TESTID)).toBeDisabled();

    rerender(
      <RecordingSummaryCard
        variant="completion"
        sessionFolder={FOLDER}
        title="Standup"
        screenSegmentCount={3}
        totalBytes={412_000_000}
      />,
    );

    await waitFor(() =>
      expect(screen.getByTestId(SUMMARY_RETITLE_FIELD_TESTID)).toHaveValue("Standup"),
    );
    expect(screen.getByTestId(SUMMARY_RETITLE_SAVE_TESTID)).toBeDisabled();
    fireEvent.click(screen.getByTestId(SUMMARY_RETITLE_SAVE_TESTID));
    expect(mockRetitle).not.toHaveBeenCalled();
  });

  it("keeps a draft the user typed when the title lands mid-edit", async () => {
    const { rerender } = render(
      <RecordingSummaryCard
        variant="completion"
        sessionFolder={FOLDER}
        title={null}
        screenSegmentCount={null}
        totalBytes={null}
      />,
    );

    fireEvent.click(screen.getByTestId(SUMMARY_RETITLE_EDIT_TESTID));
    fireEvent.change(screen.getByTestId(SUMMARY_RETITLE_FIELD_TESTID), {
      target: { value: "Retro" },
    });

    rerender(
      <RecordingSummaryCard
        variant="completion"
        sessionFolder={FOLDER}
        title="Standup"
        screenSegmentCount={3}
        totalBytes={412_000_000}
      />,
    );

    // The re-seed is for an UNTOUCHED draft only — typed words are the user's.
    await waitFor(() => expect(screen.getByTestId(SUMMARY_RETITLE_SAVE_TESTID)).toBeEnabled());
    expect(screen.getByTestId(SUMMARY_RETITLE_FIELD_TESTID)).toHaveValue("Retro");
  });

  it("keeps the rename editor out of the live region and carries focus with it", () => {
    render(
      <RecordingSummaryCard
        variant="completion"
        sessionFolder={FOLDER}
        title="Standup"
        screenSegmentCount={3}
        totalBytes={412_000_000}
      />,
    );

    fireEvent.click(screen.getByTestId(SUMMARY_RETITLE_EDIT_TESTID));
    const field = screen.getByTestId(SUMMARY_RETITLE_FIELD_TESTID);
    // Focus lands on the field the click opened, not on document.body.
    expect(field).toHaveFocus();
    // A text input inside an aria-atomic `role="status"` re-announces the whole
    // card — headline and full folder path — on every keystroke.
    const live = screen.getByRole("status");
    expect(live).not.toContainElement(field);
    expect(live).not.toContainElement(screen.getByTestId(SUMMARY_RETITLE_SAVE_TESTID));
    // The announced outcome still IS a live region.
    expect(live).toContainElement(screen.getByTestId(SUMMARY_FOLDER_TESTID));

    // Leaving the editor returns focus to the control that opened it.
    fireEvent.click(screen.getByTestId(SUMMARY_RETITLE_CANCEL_TESTID));
    expect(screen.getByTestId(SUMMARY_RETITLE_EDIT_TESTID)).toHaveFocus();
  });
});
