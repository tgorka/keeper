import { open as openFile } from "@tauri-apps/plugin-dialog";
import { act, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { TranscribeAFileHost, transcribeAFile } from "@/components/transcription/transcribe-a-file";
import { TRANSCRIPTION_PROGRESS_LABEL } from "@/components/transcription/transcription-progress";
import { Toaster } from "@/components/ui/sonner";
import * as ipc from "@/lib/ipc/client";
import { transcriptionStore } from "@/lib/stores/transcription";
import { TRANSCRIPT_FIXTURE } from "../../../dev/transcription-fixture";

vi.mock("@tauri-apps/plugin-dialog", () => ({ open: vi.fn() }));
vi.mock("@/lib/ipc/client", () => ({
  transcriptionStart: vi.fn(),
  transcriptionCancel: vi.fn(),
  transcriptionStatus: vi.fn(),
  transcriptRead: vi.fn(),
  transcriptMedia: vi.fn(),
  listenTranscriptWritten: vi.fn(() => Promise.resolve(() => {})),
}));

const file = "/Users/alice/call.m4a";
let progress: ((p: ipc.TranscriptionProgressVm) => void) | undefined;
const send = (phase: ipc.TranscriptionProgressVm["phase"], extra: object = {}) =>
  act(async () =>
    progress?.({
      jobId: "job",
      phase,
      part: 1,
      parts: 1,
      message: null,
      transcriptPath: null,
      fraction: null,
      elapsedMs: 0,
      replaceable: false,
      ...extra,
    }),
  );
beforeEach(() => {
  // Clear, not reset: setup.ts's matchMedia stub, which the Toaster reads, is a mock too.
  vi.clearAllMocks();
  progress = undefined;
  transcriptionStore.setState({ status: null, error: null, jobs: {}, people: {}, dictionary: {} });
  vi.mocked(ipc.transcriptionStart).mockImplementation((_path, onProgress) => {
    progress = onProgress;
    return Promise.resolve("job");
  });
  vi.mocked(ipc.transcriptionCancel).mockResolvedValue(undefined);
  vi.mocked(ipc.transcriptionStatus).mockRejectedValue(new Error("not needed here"));
  vi.mocked(ipc.transcriptRead).mockResolvedValue(structuredClone(TRANSCRIPT_FIXTURE));
  vi.mocked(ipc.transcriptMedia).mockResolvedValue({
    parts: [],
    hasCamera: false,
    hasScreen: false,
  });
});

describe("Transcribe a File…", () => {
  it("opens the Settings picker, follows the job in one toast and opens the transcript", async () => {
    vi.mocked(openFile).mockResolvedValue(file);
    render(
      <>
        <Toaster />
        <TranscribeAFileHost />
      </>,
    );
    await act(() => transcribeAFile());
    expect(openFile).toHaveBeenCalledWith({
      directory: false,
      multiple: false,
      title: "Transcribe a file",
    });
    expect(ipc.transcriptionStart).toHaveBeenCalledWith(file, expect.any(Function), false);
    await send("transcribing");
    expect(await screen.findByText("transcribing · Part 1 of 1")).toBeInTheDocument();
    // No estimate yet: the bar is indeterminate, never a guessed zero.
    expect(
      screen.getByRole("progressbar", { name: TRANSCRIPTION_PROGRESS_LABEL }),
    ).not.toHaveAttribute("aria-valuenow");
    await send("transcribing", { fraction: 0.375, elapsedMs: 65_000 });
    // Sonner re-renders an updated toast on its own tick.
    expect(await screen.findByText("1:05")).toBeInTheDocument();
    expect(screen.getByRole("progressbar", { name: TRANSCRIPTION_PROGRESS_LABEL })).toHaveAttribute(
      "aria-valuenow",
      "38",
    );
    expect(screen.queryByRole("button", { name: "Open transcript" })).toBeNull();
    await send("done", { transcriptPath: `${file}.transcript.json` });
    expect(await screen.findByText("The transcript is ready.")).toBeInTheDocument();
    // One toast for the file, updated in place rather than stacked.
    expect(screen.getAllByText("call.m4a")).toHaveLength(1);
    expect(screen.queryByRole("dialog")).toBeNull();
    fireEvent.click(screen.getByRole("button", { name: "Open transcript" }));
    await screen.findByRole("dialog");
    await waitFor(() => expect(ipc.transcriptRead).toHaveBeenCalledWith(`${file}.transcript.json`));
  });

  it("does nothing when the picker is cancelled", async () => {
    vi.mocked(openFile).mockResolvedValue(null);
    render(<Toaster />);
    await act(() => transcribeAFile());
    expect(ipc.transcriptionStart).not.toHaveBeenCalled();
    expect(screen.queryByRole("listitem")).toBeNull();
  });

  it("cancels a running job from its toast", async () => {
    vi.mocked(openFile).mockResolvedValue(file);
    render(<Toaster />);
    await act(() => transcribeAFile());
    await send("diarizing");
    fireEvent.click(await screen.findByRole("button", { name: "Cancel transcription" }));
    expect(ipc.transcriptionCancel).toHaveBeenCalledWith("job");
    await send("cancelled");
    expect(await screen.findByText("Transcription cancelled.")).toBeInTheDocument();
  });

  it("shows the shell's sentence on a failure and tries the same file again", async () => {
    vi.mocked(openFile).mockResolvedValue(file);
    render(<Toaster />);
    await act(() => transcribeAFile());
    await send("failed", { message: "The transcription models are missing." });
    expect(await screen.findByText(/The transcription models are missing\./)).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "Try again" }));
    await waitFor(() => expect(ipc.transcriptionStart).toHaveBeenCalledTimes(2));
    expect(vi.mocked(ipc.transcriptionStart).mock.calls[1][0]).toBe(file);
    expect(openFile).toHaveBeenCalledTimes(1);
  });
});
