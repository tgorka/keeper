import { open as openFile } from "@tauri-apps/plugin-dialog";
import { act, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { TranscriptionSection } from "@/components/settings/transcription-section";
import * as ipc from "@/lib/ipc/client";
import { transcriptionStore } from "@/lib/stores/transcription";
import { TRANSCRIPT_FIXTURE } from "../../../dev/transcription-fixture";

vi.mock("@tauri-apps/plugin-dialog", () => ({ open: vi.fn() }));
vi.mock("@/lib/ipc/client", () => ({
  transcriptionStatus: vi.fn(),
  transcriptionStart: vi.fn(),
  transcriptRead: vi.fn(),
  transcriptMedia: vi.fn(),
  voicesPeople: vi.fn(),
  dictionaryTerms: vi.fn(),
  dictionaryTermSave: vi.fn(),
  voicesPersonRename: vi.fn(),
}));

beforeEach(() => {
  vi.resetAllMocks();
  transcriptionStore.setState({ status: null, error: null, jobs: {}, people: {}, dictionary: {} });
  vi.mocked(ipc.transcriptionStatus).mockResolvedValue({
    available: true,
    reason: null,
    models: { state: "ready", sentence: "Ready", missing: [] },
    afterRecording: false,
    language: "auto",
    voicesDrives: [
      {
        profileId: "drive",
        name: "Drive",
        voicesRoot: "/Volumes/merope/tgdrive/voices",
        subfolder: "voices",
        localPath: "/Volumes/merope/tgdrive",
      },
    ],
  });
  vi.mocked(ipc.voicesPeople).mockResolvedValue(structuredClone(TRANSCRIPT_FIXTURE.people));
  vi.mocked(ipc.dictionaryTerms).mockResolvedValue([]);
  vi.mocked(ipc.transcriptRead).mockResolvedValue(structuredClone(TRANSCRIPT_FIXTURE));
  vi.mocked(ipc.transcriptMedia).mockResolvedValue({
    parts: [],
    hasCamera: false,
    hasScreen: false,
  });
});

describe("Transcription settings", () => {
  it("cancels a dictionary term without saving it", async () => {
    render(<TranscriptionSection open />);
    fireEvent.click(await screen.findByRole("button", { name: "Add term" }));
    fireEvent.change(screen.getByRole("textbox", { name: "Dictionary term" }), {
      target: { value: "Kowalski" },
    });
    fireEvent.click(screen.getByRole("button", { name: "Cancel" }));
    expect(screen.queryByRole("textbox", { name: "Dictionary term" })).toBeNull();
    expect(ipc.dictionaryTermSave).not.toHaveBeenCalled();
  });

  it("cancels a person's new name without saving it", async () => {
    render(<TranscriptionSection open />);
    fireEvent.click((await screen.findAllByRole("button", { name: "Rename person" }))[0]);
    fireEvent.change(screen.getByRole("textbox", { name: "Person name" }), {
      target: { value: "Not this" },
    });
    fireEvent.click(screen.getByRole("button", { name: "Cancel" }));
    expect(screen.queryByRole("textbox", { name: "Person name" })).toBeNull();
    expect(ipc.voicesPersonRename).not.toHaveBeenCalled();
  });

  it("opens a finished transcript only when asked, and re-reads People when it closes", async () => {
    let progress: ((p: ipc.TranscriptionProgressVm) => void) | undefined;
    vi.mocked(openFile).mockResolvedValue("/Users/alice/call.m4a");
    vi.mocked(ipc.transcriptionStart).mockImplementation((_path, onProgress) => {
      progress = onProgress;
      return Promise.resolve("job");
    });
    render(<TranscriptionSection open />);
    await screen.findAllByRole("button", { name: "Rename person" });
    fireEvent.click(screen.getByRole("button", { name: "Transcribe a file…" }));
    await waitFor(() => expect(progress).toBeDefined());
    await act(async () =>
      progress?.({
        jobId: "job",
        phase: "done",
        part: 1,
        parts: 1,
        message: null,
        transcriptPath: "/Users/alice/call.m4a.transcript.json",
        fraction: 1,
        elapsedMs: 1_000,
        replaceable: false,
      }),
    );
    expect(screen.queryByRole("dialog")).toBeNull();
    fireEvent.click(screen.getByRole("button", { name: "Open transcript" }));
    await screen.findByRole("dialog");
    vi.mocked(ipc.voicesPeople).mockResolvedValue([
      ...TRANSCRIPT_FIXTURE.people,
      { ...TRANSCRIPT_FIXTURE.people[1], id: "p-jo", name: "Jo" },
    ]);
    fireEvent.keyDown(screen.getByRole("dialog"), { key: "Escape" });
    await waitFor(() =>
      expect(screen.getAllByRole("button", { name: "Rename person" })).toHaveLength(3),
    );
    expect(screen.queryByRole("dialog")).toBeNull();
  });
});
