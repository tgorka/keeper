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
  listenTranscriptWritten: vi.fn(() => Promise.resolve(() => {})),
  voicesPeople: vi.fn(),
  dictionaryTerms: vi.fn(),
  dictionaryTermSave: vi.fn(),
  voicesPersonRename: vi.fn(),
  transcriptionModelsAvailable: vi.fn(),
  transcriptionSettingsSet: vi.fn(),
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
    asrModel: "",
    diarizationModel: "",
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
  vi.mocked(ipc.transcriptionModelsAvailable).mockResolvedValue({
    asr: [
      { id: "parakeet-tdt-0.6b-v3", complete: true },
      { id: "parakeet-tdt-0.6b-v4", complete: true },
      { id: "parakeet-half", complete: false },
    ],
    diarization: [{ id: "speaker-diarization", complete: true }],
    defaults: { asr: "parakeet-tdt-0.6b-v3", diarization: "speaker-diarization" },
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

describe("Transcription models", () => {
  it("offers the repository's choice first, disables incomplete ones and writes the key", async () => {
    const status = await ipc.transcriptionStatus();
    vi.mocked(ipc.transcriptionSettingsSet).mockResolvedValue({
      ...status,
      asrModel: "parakeet-tdt-0.6b-v4",
    });
    render(<TranscriptionSection open />);
    const speech = (await screen.findByLabelText("Speech model")) as HTMLSelectElement;
    const options = [...speech.options].map((option) => [option.text, option.disabled]);
    expect(options).toEqual([
      ["From the config repository (parakeet-tdt-0.6b-v3)", false],
      ["parakeet-tdt-0.6b-v3", false],
      ["parakeet-tdt-0.6b-v4", false],
      ["parakeet-half (incomplete)", true],
    ]);
    expect(speech.value).toBe("");

    fireEvent.change(speech, { target: { value: "parakeet-tdt-0.6b-v4" } });
    await waitFor(() =>
      expect(ipc.transcriptionSettingsSet).toHaveBeenCalledWith(
        null,
        null,
        "parakeet-tdt-0.6b-v4",
        null,
      ),
    );
    await waitFor(() => expect(speech.value).toBe("parakeet-tdt-0.6b-v4"));

    const speaker = screen.getByLabelText("Speaker model") as HTMLSelectElement;
    fireEvent.change(speaker, { target: { value: "" } });
    await waitFor(() =>
      expect(ipc.transcriptionSettingsSet).toHaveBeenLastCalledWith(null, null, null, ""),
    );
  });

  it("keeps a pick that is no longer on this Mac visible", async () => {
    const status = await ipc.transcriptionStatus();
    vi.mocked(ipc.transcriptionStatus).mockResolvedValue({
      ...status,
      diarizationModel: "diarizer-gone",
    });
    render(<TranscriptionSection open />);
    const speaker = (await screen.findByLabelText("Speaker model")) as HTMLSelectElement;
    await waitFor(() => expect(speaker.value).toBe("diarizer-gone"));
    expect(speaker.selectedOptions[0]?.text).toBe("diarizer-gone (not on this Mac)");
    expect(speaker.selectedOptions[0]?.disabled).toBe(true);
  });
});
