import { act, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

vi.mock("@/lib/ipc/client", () => ({
  recordingPathPreview: vi.fn(),
  recordingSettingsGet: vi.fn(),
  recordingSettingsSet: vi.fn(),
}));

// The OS-native directory picker (the export-dialog mock pattern).
const openFolder = vi.fn();
vi.mock("@tauri-apps/plugin-dialog", () => ({
  open: (...args: unknown[]) => openFolder(...args),
}));

import {
  CHOOSE_FOLDER_LABEL,
  DESTINATION_LOCAL_ONLY_NOTE,
  DESTINATION_NEXT_SESSION_NOTE,
  DESTINATION_PATH_TESTID,
  DESTINATION_TEMPLATE_FAULT_TESTID,
  DESTINATION_TEMPLATE_PREVIEW_TESTID,
  DESTINATION_TEMPLATE_SAVE_TESTID,
  DESTINATION_TEMPLATE_TESTID,
  RecordingDestinationControls,
} from "@/components/recording/recording-destination-controls";
import type { RecordingPathPreviewVm, RecordingSettingsVm } from "@/lib/ipc/client";
import { recordingPathPreview, recordingSettingsGet, recordingSettingsSet } from "@/lib/ipc/client";
import { recordingMetaStore } from "@/lib/stores/recording-meta";
import {
  RECORDING_PATH_TEMPLATE_DEFAULT,
  resetRecordingSettingsForTest,
} from "@/lib/stores/recording-settings";

const mockGet = vi.mocked(recordingSettingsGet);
const mockSet = vi.mocked(recordingSettingsSet);
const mockPreview = vi.mocked(recordingPathPreview);

const DEFAULTS: RecordingSettingsVm = {
  segmentMb: 500,
  durationCapMinutes: 30,
  destinationDir: "/Users/alice/Movies/keeper",
  fps: 30,
  codec: "h264",
  scalePercent: 100,
  echoCancellation: false,
  pathTemplate: RECORDING_PATH_TEMPLATE_DEFAULT,
};

/** What Rust renders for the default template with no session title (the
 * `{slug}` collapse case), at the matrix's 2026-08-05T14:32 local. */
const DEFAULT_PREVIEW: RecordingPathPreviewVm = {
  relativePath: "2026/2026-08-05 1432",
  absolutePath: "/Users/alice/Movies/keeper/2026/2026-08-05 1432",
  problem: null,
};

/** 40.1's `TemplateError::ParentComponent` sentence, verbatim: the card prints
 * the Rust wording, so the test asserts the Rust wording. */
const TRAVERSAL_REASON = 'a template cannot contain a "." or ".." folder';

beforeEach(() => {
  resetRecordingSettingsForTest();
  // The meta store is a module-level singleton shared with every other test
  // file: a title left behind here would re-root a sibling suite's preview.
  recordingMetaStore.getState().setFields({ title: "" });
  mockGet.mockReset();
  mockGet.mockResolvedValue(DEFAULTS);
  mockSet.mockReset();
  mockSet.mockImplementation((vm) => Promise.resolve(vm));
  openFolder.mockReset();
  mockPreview.mockReset();
  mockPreview.mockResolvedValue(DEFAULT_PREVIEW);
});

afterEach(() => {
  vi.clearAllMocks();
  recordingMetaStore.getState().setFields({ title: "" });
});

describe("RecordingDestinationControls", () => {
  it("shows the effective folder with the next-session note and local-only copy", async () => {
    render(<RecordingDestinationControls />);

    // The Rust-resolved EFFECTIVE default is always a concrete folder.
    await waitFor(() =>
      expect(screen.getByTestId(DESTINATION_PATH_TESTID)).toHaveTextContent(
        "/Users/alice/Movies/keeper",
      ),
    );
    expect(screen.getByText(DESTINATION_NEXT_SESSION_NOTE)).toBeInTheDocument();
    expect(screen.getByText(DESTINATION_LOCAL_ONLY_NOTE)).toBeInTheDocument();
    // Local-only: no share/cloud/network affordance anywhere in the card.
    expect(screen.queryByText(/share|cloud|network|http/i)).not.toBeInTheDocument();
  });

  it("opens the native directory picker and persists a confirmed selection", async () => {
    openFolder.mockResolvedValue("/Users/alice/Recordings");
    render(<RecordingDestinationControls />);
    await waitFor(() =>
      expect(screen.getByRole("button", { name: CHOOSE_FOLDER_LABEL })).toBeEnabled(),
    );

    fireEvent.click(screen.getByRole("button", { name: CHOOSE_FOLDER_LABEL }));

    await waitFor(() => expect(openFolder).toHaveBeenCalledWith({ directory: true }));
    await waitFor(() =>
      expect(mockSet).toHaveBeenCalledWith({
        ...DEFAULTS,
        destinationDir: "/Users/alice/Recordings",
      }),
    );
    // The card reflects the effective persisted folder.
    await waitFor(() =>
      expect(screen.getByTestId(DESTINATION_PATH_TESTID)).toHaveTextContent(
        "/Users/alice/Recordings",
      ),
    );
  });

  it("keeps the current folder when the picker is cancelled", async () => {
    // A cancelled native picker resolves `null` — no write, no change.
    openFolder.mockResolvedValue(null);
    render(<RecordingDestinationControls />);
    await waitFor(() =>
      expect(screen.getByRole("button", { name: CHOOSE_FOLDER_LABEL })).toBeEnabled(),
    );

    fireEvent.click(screen.getByRole("button", { name: CHOOSE_FOLDER_LABEL }));

    await waitFor(() => expect(openFolder).toHaveBeenCalled());
    expect(mockSet).not.toHaveBeenCalled();
    expect(screen.getByTestId(DESTINATION_PATH_TESTID)).toHaveTextContent(
      "/Users/alice/Movies/keeper",
    );
  });

  it("keeps the current folder when the picker throws", async () => {
    openFolder.mockRejectedValue(new Error("picker unavailable"));
    render(<RecordingDestinationControls />);
    await waitFor(() =>
      expect(screen.getByRole("button", { name: CHOOSE_FOLDER_LABEL })).toBeEnabled(),
    );

    fireEvent.click(screen.getByRole("button", { name: CHOOSE_FOLDER_LABEL }));

    await waitFor(() => expect(openFolder).toHaveBeenCalled());
    expect(mockSet).not.toHaveBeenCalled();
  });

  it("disables the chooser until the shared store hydrates", () => {
    // Never-resolving hydration: the affordance must not pretend to work.
    mockGet.mockImplementation(() => new Promise(() => {}));
    render(<RecordingDestinationControls />);

    expect(screen.getByRole("button", { name: CHOOSE_FOLDER_LABEL })).toBeDisabled();
  });

  it("previews a typed template through Rust and writes nothing", async () => {
    render(<RecordingDestinationControls />);
    const field = screen.getByTestId(DESTINATION_TEMPLATE_TESTID);
    // The field seeds from the EFFECTIVE template the store hydrated with.
    await waitFor(() => expect(field).toHaveValue(RECORDING_PATH_TEMPLATE_DEFAULT));

    mockPreview.mockResolvedValue({
      relativePath: "2026/08/05",
      absolutePath: "/Users/alice/Movies/keeper/2026/08/05",
      problem: null,
    });
    fireEvent.change(field, { target: { value: "{yyyy}/{mm}/{dd}" } });

    // The typed text goes to the backend verbatim; the empty meta title is
    // sent as `null` so Rust renders its untitled collapse.
    await waitFor(() => expect(mockPreview).toHaveBeenCalledWith("{yyyy}/{mm}/{dd}", null));
    await waitFor(() =>
      expect(screen.getByTestId(DESTINATION_TEMPLATE_PREVIEW_TESTID)).toHaveTextContent(
        "/Users/alice/Movies/keeper/2026/08/05",
      ),
    );
    // Typing is not a write.
    expect(mockSet).not.toHaveBeenCalled();
  });

  it("prints the Rust reason for an unparseable template and disables the save", async () => {
    render(<RecordingDestinationControls />);
    const field = screen.getByTestId(DESTINATION_TEMPLATE_TESTID);
    await waitFor(() => expect(field).toHaveValue(RECORDING_PATH_TEMPLATE_DEFAULT));

    mockPreview.mockResolvedValue({
      relativePath: null,
      absolutePath: null,
      problem: TRAVERSAL_REASON,
    });
    fireEvent.change(field, { target: { value: "../{yyyy}" } });

    await waitFor(() =>
      expect(screen.getByTestId(DESTINATION_TEMPLATE_FAULT_TESTID)).toHaveTextContent(
        TRAVERSAL_REASON,
      ),
    );
    expect(screen.getByTestId(DESTINATION_TEMPLATE_SAVE_TESTID)).toBeDisabled();
    // Never both: a template that cannot name a folder shows no path.
    expect(screen.queryByTestId(DESTINATION_TEMPLATE_PREVIEW_TESTID)).not.toBeInTheDocument();
  });

  it("saves a cleared template and repopulates from the effective default", async () => {
    render(<RecordingDestinationControls />);
    const field = screen.getByTestId(DESTINATION_TEMPLATE_TESTID);
    await waitFor(() => expect(field).toHaveValue(RECORDING_PATH_TEMPLATE_DEFAULT));

    // Clearing is a save, not a fault: Rust previews a blank as the default.
    fireEvent.change(field, { target: { value: "" } });
    await waitFor(() => expect(screen.getByTestId(DESTINATION_TEMPLATE_SAVE_TESTID)).toBeEnabled());
    // The stored key is cleared, but the EFFECTIVE echo is never blank.
    mockSet.mockResolvedValue({ ...DEFAULTS, pathTemplate: RECORDING_PATH_TEMPLATE_DEFAULT });

    fireEvent.click(screen.getByTestId(DESTINATION_TEMPLATE_SAVE_TESTID));

    await waitFor(() => expect(mockSet).toHaveBeenCalledWith({ ...DEFAULTS, pathTemplate: "" }));
    await waitFor(() => expect(field).toHaveValue(RECORDING_PATH_TEMPLATE_DEFAULT));
  });

  it("re-asks Rust for the preview when the destination folder changes", async () => {
    render(<RecordingDestinationControls />);
    // The first preview is rooted at the hydrated folder.
    await waitFor(() =>
      expect(screen.getByTestId(DESTINATION_TEMPLATE_PREVIEW_TESTID)).toHaveTextContent(
        "/Users/alice/Movies/keeper/2026/2026-08-05 1432",
      ),
    );
    const beforeFolderChange = mockPreview.mock.calls.length;

    // The command resolves the root itself, so the answer it already gave is
    // stale the moment the folder moves: Rust must be asked again.
    openFolder.mockResolvedValue("/Volumes/Pendrive");
    mockPreview.mockResolvedValue({
      relativePath: "2026/2026-08-05 1432",
      absolutePath: "/Volumes/Pendrive/2026/2026-08-05 1432",
      problem: null,
    });
    fireEvent.click(screen.getByRole("button", { name: CHOOSE_FOLDER_LABEL }));

    await waitFor(() =>
      expect(screen.getByTestId(DESTINATION_PATH_TESTID)).toHaveTextContent("/Volumes/Pendrive"),
    );
    await waitFor(() => expect(mockPreview.mock.calls.length).toBeGreaterThan(beforeFolderChange));
    await waitFor(() =>
      expect(screen.getByTestId(DESTINATION_TEMPLATE_PREVIEW_TESTID)).toHaveTextContent(
        "/Volumes/Pendrive/2026/2026-08-05 1432",
      ),
    );
  });

  it("applies only the newest preview response when replies land out of order", async () => {
    // Manually controlled replies, so the test decides the landing order.
    const pending: Array<{
      promise: Promise<RecordingPathPreviewVm>;
      resolve: (vm: RecordingPathPreviewVm) => void;
      reject: (reason: unknown) => void;
    }> = [];
    mockPreview.mockImplementation(() => {
      let resolve!: (vm: RecordingPathPreviewVm) => void;
      let reject!: (reason: unknown) => void;
      const promise = new Promise<RecordingPathPreviewVm>((res, rej) => {
        resolve = res;
        reject = rej;
      });
      pending.push({ promise, resolve, reject });
      return promise;
    });

    render(<RecordingDestinationControls />);
    const field = screen.getByTestId(DESTINATION_TEMPLATE_TESTID);
    await waitFor(() => expect(field).toHaveValue(RECORDING_PATH_TEMPLATE_DEFAULT));
    // [0] is the seed preview; it is left in flight and failed last, below.
    await waitFor(() => expect(pending).toHaveLength(1));

    fireEvent.change(field, { target: { value: "{yyyy}/a" } });
    await waitFor(() => expect(pending).toHaveLength(2));
    fireEvent.change(field, { target: { value: "{yyyy}/b" } });
    await waitFor(() => expect(pending).toHaveLength(3));

    // The newest reply lands first.
    await act(async () => {
      pending[2].resolve({
        relativePath: "2026/b",
        absolutePath: "/Users/alice/Movies/keeper/2026/b",
        problem: null,
      });
    });
    expect(screen.getByTestId(DESTINATION_TEMPLATE_PREVIEW_TESTID)).toHaveTextContent(
      "/Users/alice/Movies/keeper/2026/b",
    );
    expect(screen.getByTestId(DESTINATION_TEMPLATE_SAVE_TESTID)).toBeEnabled();

    // The loser of the race answers late with a different path: it is dropped.
    await act(async () => {
      pending[1].resolve({
        relativePath: "2026/a",
        absolutePath: "/Users/alice/Movies/keeper/2026/a",
        problem: null,
      });
    });
    expect(screen.getByTestId(DESTINATION_TEMPLATE_PREVIEW_TESTID)).toHaveTextContent(
      "/Users/alice/Movies/keeper/2026/b",
    );
    expect(screen.getByTestId(DESTINATION_TEMPLATE_PREVIEW_TESTID)).not.toHaveTextContent(
      "/Users/alice/Movies/keeper/2026/a",
    );

    // A stale round trip that FAILS late is dropped by the same token, so it
    // cannot blank the newest path or re-disable the save.
    await act(async () => {
      pending[0].reject(new Error("preview round trip lost"));
    });
    expect(screen.getByTestId(DESTINATION_TEMPLATE_PREVIEW_TESTID)).toHaveTextContent(
      "/Users/alice/Movies/keeper/2026/b",
    );
    // The save's verdict is the newest response's, not the stragglers'.
    expect(screen.getByTestId(DESTINATION_TEMPLATE_SAVE_TESTID)).toBeEnabled();
  });

  it("previews untitled on the Settings surface even with a title pending", async () => {
    // A title typed on the pre-record pane outlives that card's mount; the
    // Settings dialog must not inherit it, or `{slug}`'s collapse never shows.
    act(() => {
      recordingMetaStore.getState().setFields({ title: "Standup" });
    });
    render(<RecordingDestinationControls />);

    await waitFor(() => expect(mockPreview).toHaveBeenCalled());
    for (const call of mockPreview.mock.calls) {
      expect(call[1]).toBeNull();
    }
  });

  it("previews against the pending title on the pre-record surface", async () => {
    act(() => {
      recordingMetaStore.getState().setFields({ title: "Standup" });
    });
    render(<RecordingDestinationControls withNextSessionTitle />);

    await waitFor(() =>
      expect(mockPreview).toHaveBeenCalledWith(RECORDING_PATH_TEMPLATE_DEFAULT, "Standup"),
    );

    // Whitespace is not a title: it collapses to `null` so Rust renders the
    // untitled form rather than a folder named with blanks.
    act(() => {
      recordingMetaStore.getState().setFields({ title: "   " });
    });
    await waitFor(() =>
      expect(mockPreview).toHaveBeenLastCalledWith(RECORDING_PATH_TEMPLATE_DEFAULT, null),
    );
  });

  it("keeps the typed template and prints a refused write, then clears it on success", async () => {
    render(<RecordingDestinationControls />);
    const field = screen.getByTestId(DESTINATION_TEMPLATE_TESTID);
    await waitFor(() => expect(field).toHaveValue(RECORDING_PATH_TEMPLATE_DEFAULT));

    mockPreview.mockResolvedValue({
      relativePath: "2026/notes",
      absolutePath: "/Users/alice/Movies/keeper/2026/notes",
      problem: null,
    });
    fireEvent.change(field, { target: { value: "{yyyy}/notes" } });
    await waitFor(() => expect(screen.getByTestId(DESTINATION_TEMPLATE_SAVE_TESTID)).toBeEnabled());

    // The preview said fine and the write still refused: an IPC rejection is a
    // `{ code, message }` envelope, and its sentence is what must print.
    const WRITE_REFUSAL = "keeper could not write the recording settings.";
    mockSet.mockRejectedValueOnce({
      code: "internal",
      message: WRITE_REFUSAL,
      retriable: true,
    });
    fireEvent.click(screen.getByTestId(DESTINATION_TEMPLATE_SAVE_TESTID));

    await waitFor(() =>
      expect(screen.getByTestId(DESTINATION_TEMPLATE_FAULT_TESTID)).toHaveTextContent(
        WRITE_REFUSAL,
      ),
    );
    // The mirror reverted behind the refusal; the field did NOT — the text the
    // sentence is about stays put, ready to be corrected or retried.
    expect(field).toHaveValue("{yyyy}/notes");

    fireEvent.click(screen.getByTestId(DESTINATION_TEMPLATE_SAVE_TESTID));

    await waitFor(() =>
      expect(mockSet).toHaveBeenLastCalledWith({ ...DEFAULTS, pathTemplate: "{yyyy}/notes" }),
    );
    // Confirmed: the stale refusal is gone and the field holds the effective
    // template Rust echoed back.
    await waitFor(() =>
      expect(screen.queryByTestId(DESTINATION_TEMPLATE_FAULT_TESTID)).not.toBeInTheDocument(),
    );
    expect(field).toHaveValue("{yyyy}/notes");
  });
});
