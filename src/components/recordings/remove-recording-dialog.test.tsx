import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import type * as IpcClient from "@/lib/ipc/client";

const preview = vi.fn<typeof IpcClient.recordingRemovePreview>();
const remove = vi.fn<typeof IpcClient.recordingRemove>();
vi.mock("@/lib/ipc/client", () => ({
  recordingRemovePreview: (sessionId: string) => preview(sessionId),
  recordingRemove: (sessionId: string) => remove(sessionId),
}));

import {
  REMOVE_RECORDING_CONFIRM,
  REMOVE_RECORDING_DONE_TITLE,
  REMOVE_RECORDING_TITLE,
  RemoveRecordingDialog,
} from "@/components/recordings/remove-recording-dialog";

const PLAN: IpcClient.RecordingRemovalPreviewVm = {
  folder: "recordings/2026/standup",
  drive: "tgdrive",
  bytes: 1_500_000_000,
  files: 7,
  durability: "pushed",
  notes: [
    { vaultId: "v", path: "meetings/standup.md", title: "Standup" },
    { vaultId: "v", path: "recordings/standup.md", title: "Standup recording" },
  ],
};

const REMOVED: IpcClient.RecordingRemovedVm = {
  folder: PLAN.folder,
  bytes: PLAN.bytes,
  files: PLAN.files,
  notesChanged: [PLAN.notes[1]],
  openNotes: [PLAN.notes[0]],
  notesFailed: [],
};

beforeEach(() => {
  preview.mockReset();
  remove.mockReset();
});

describe("RemoveRecordingDialog", () => {
  it("names the folder, its size and files, the drive and the notes before it removes anything", async () => {
    preview.mockResolvedValue(PLAN);
    render(<RemoveRecordingDialog sessionId="S1" onClose={vi.fn()} onRemoved={vi.fn()} />);

    expect(await screen.findByText(REMOVE_RECORDING_TITLE)).toBeInTheDocument();
    const body = screen.getByText(/recordings\/2026\/standup/);
    expect(body).toHaveTextContent("1.5 GB");
    expect(body).toHaveTextContent("7 files");
    expect(body).toHaveTextContent(
      "It is deleted from tgdrive on every device. The drive's history still has it.",
    );
    expect(screen.getByText("Standup")).toBeInTheDocument();
    expect(screen.getByText("Standup recording")).toBeInTheDocument();
    expect(preview).toHaveBeenCalledWith("S1");
    expect(remove).not.toHaveBeenCalled();
  });

  it.each([
    [
      "local",
      "This recording has not reached tgdrive's history yet — removing it deletes the only copy.",
    ],
    ["committed", "This recording has not left this Mac yet — removing it deletes the only copy."],
  ])("never promises the history keeps a %s recording", async (durability, sentence) => {
    preview.mockResolvedValue({ ...PLAN, durability });
    render(<RemoveRecordingDialog sessionId="S1" onClose={vi.fn()} onRemoved={vi.fn()} />);

    const body = await screen.findByText(/recordings\/2026\/standup/);
    expect(body).toHaveTextContent(sentence);
    expect(body).not.toHaveTextContent("history still has it");
  });

  it("says which notes still name the removed recording, and why", async () => {
    preview.mockResolvedValue(PLAN);
    remove.mockResolvedValue({
      ...REMOVED,
      notesFailed: [{ ...PLAN.notes[1], error: "recordings/standup.md: permission denied" }],
    });
    const onRemoved = vi.fn();
    const onClose = vi.fn();
    render(<RemoveRecordingDialog sessionId="S1" onClose={onClose} onRemoved={onRemoved} />);

    fireEvent.click(await screen.findByRole("button", { name: REMOVE_RECORDING_CONFIRM }));

    expect(await screen.findByText(REMOVE_RECORDING_DONE_TITLE)).toBeInTheDocument();
    const still = screen.getByRole("list", { name: "Notes still naming this recording" });
    expect(still).toHaveTextContent("Standup recording");
    expect(still).toHaveTextContent("permission denied");
    expect(onRemoved).toHaveBeenCalled();
    expect(onClose).not.toHaveBeenCalled();
  });

  it("removes on the one destructive press and hands the answer back", async () => {
    preview.mockResolvedValue(PLAN);
    remove.mockResolvedValue(REMOVED);
    const onRemoved = vi.fn();
    const onClose = vi.fn();
    render(<RemoveRecordingDialog sessionId="S1" onClose={onClose} onRemoved={onRemoved} />);

    fireEvent.click(await screen.findByRole("button", { name: REMOVE_RECORDING_CONFIRM }));

    await waitFor(() => expect(onRemoved).toHaveBeenCalledWith(REMOVED));
    expect(remove).toHaveBeenCalledWith("S1");
    expect(onClose).toHaveBeenCalled();
  });

  it("offers no removal it cannot describe", async () => {
    preview.mockRejectedValue({
      code: "internal",
      message: "keeper does not know where this recording is.",
    });
    render(<RemoveRecordingDialog sessionId="S1" onClose={vi.fn()} onRemoved={vi.fn()} />);

    expect(await screen.findByRole("alert")).toHaveTextContent("does not know where");
    expect(screen.queryByRole("button", { name: REMOVE_RECORDING_CONFIRM })).toBeNull();
  });
});
