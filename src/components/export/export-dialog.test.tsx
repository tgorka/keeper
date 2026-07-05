import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { ExportProgressVm, ExportRequestVm } from "@/lib/ipc/client";

// Mock the typed IPC client so the dialog never touches Tauri.
const startExport = vi.fn();
const cancelExport = vi.fn();
const revealPath = vi.fn();
vi.mock("@/lib/ipc/client", () => ({
  startExport: (request: unknown, onProgress: unknown) => startExport(request, onProgress),
  cancelExport: (id: unknown) => cancelExport(id),
  revealPath: (path: unknown) => revealPath(path),
}));

// Mock the folder picker.
const openFolder = vi.fn();
vi.mock("@tauri-apps/plugin-dialog", () => ({
  open: (...args: unknown[]) => openFolder(...args),
}));

// Mock the toast so we can assert the success toast + Reveal action.
const toastSuccess = vi.fn();
vi.mock("sonner", () => ({
  toast: { success: (...args: unknown[]) => toastSuccess(...args) },
}));

import { ExportDialog } from "@/components/export/export-dialog";
import { exportStore } from "@/lib/stores/export";

/** The onProgress callback captured from the latest startExport call. */
function capturedOnProgress(): (b: ExportProgressVm) => void {
  const calls = startExport.mock.calls;
  const call = calls[calls.length - 1];
  if (call === undefined) {
    throw new Error("startExport was not called");
  }
  return call[1] as (b: ExportProgressVm) => void;
}

function progress(
  p: Partial<ExportProgressVm> & Pick<ExportProgressVm, "exportId" | "phase">,
): ExportProgressVm {
  return {
    exportId: p.exportId,
    phase: p.phase,
    messagesWritten: p.messagesWritten ?? 0,
    totalMessages: p.totalMessages ?? null,
    mediaCopied: p.mediaCopied ?? 0,
    mediaSkipped: p.mediaSkipped ?? 0,
    outputPaths: p.outputPaths ?? [],
    error: p.error ?? null,
  };
}

beforeEach(() => {
  startExport.mockReset();
  startExport.mockResolvedValue(1);
  cancelExport.mockReset();
  cancelExport.mockResolvedValue(undefined);
  revealPath.mockReset();
  revealPath.mockResolvedValue(undefined);
  openFolder.mockReset();
  toastSuccess.mockReset();
  exportStore.setState({
    isOpen: true,
    preset: { scope: "chat", accountId: "acctA", roomId: "!r1" },
    job: null,
  });
});

afterEach(() => {
  exportStore.setState({ isOpen: false, job: null });
  vi.clearAllMocks();
});

describe("ExportDialog", () => {
  it("renders nothing when closed", () => {
    exportStore.setState({ isOpen: false });
    const { container } = render(<ExportDialog />);
    expect(container).toBeEmptyDOMElement();
  });

  it("builds the correct ExportRequestVm from scope/format/media + destination", async () => {
    openFolder.mockResolvedValue("/Users/me/Exports");
    render(<ExportDialog />);
    // Choose a destination.
    fireEvent.click(screen.getByRole("button", { name: /choose folder/i }));
    await waitFor(() => expect(screen.getByText("/Users/me/Exports")).toBeInTheDocument());
    // Toggle include-media on.
    fireEvent.click(screen.getByLabelText("Include media files"));
    // Start.
    fireEvent.click(screen.getByRole("button", { name: /start export/i }));
    await waitFor(() => expect(startExport).toHaveBeenCalled());
    const request = startExport.mock.calls[0]?.[0] as ExportRequestVm;
    expect(request).toEqual({
      scope: "chat",
      accountId: "acctA",
      roomId: "!r1",
      json: true,
      markdown: true,
      includeMedia: true,
      destinationDir: "/Users/me/Exports",
    });
  });

  it("blocks Start until at least one format is selected", async () => {
    openFolder.mockResolvedValue("/dest");
    render(<ExportDialog />);
    fireEvent.click(screen.getByRole("button", { name: /choose folder/i }));
    await waitFor(() => expect(screen.getByText("/dest")).toBeInTheDocument());
    // Uncheck both formats.
    fireEvent.click(screen.getByLabelText(/JSON/i));
    fireEvent.click(screen.getByLabelText(/Markdown/i));
    expect(screen.getByText(/select at least one format/i)).toBeInTheDocument();
    expect(screen.getByRole("button", { name: /start export/i })).toBeDisabled();
    // No call is made.
    expect(startExport).not.toHaveBeenCalled();
  });

  it("blocks Start until a destination is chosen", () => {
    render(<ExportDialog />);
    expect(screen.getByRole("button", { name: /start export/i })).toBeDisabled();
  });

  it("shows a Progress bar + Cancel while running and cancels on click", async () => {
    openFolder.mockResolvedValue("/dest");
    render(<ExportDialog />);
    fireEvent.click(screen.getByRole("button", { name: /choose folder/i }));
    await waitFor(() => expect(screen.getByText("/dest")).toBeInTheDocument());
    fireEvent.click(screen.getByRole("button", { name: /start export/i }));
    await waitFor(() => expect(startExport).toHaveBeenCalled());
    // Stream a running batch.
    capturedOnProgress()(
      progress({ exportId: 1, phase: "running", messagesWritten: 3, totalMessages: 10 }),
    );
    await waitFor(() =>
      expect(screen.getByRole("status", { name: /export progress/i })).toBeInTheDocument(),
    );
    expect(screen.getByText(/3 \/ 10 messages/i)).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: /cancel export/i }));
    expect(cancelExport).toHaveBeenCalledWith(1);
  });

  it("fires a success toast with a Reveal action on completion", async () => {
    openFolder.mockResolvedValue("/dest");
    render(<ExportDialog />);
    fireEvent.click(screen.getByRole("button", { name: /choose folder/i }));
    await waitFor(() => expect(screen.getByText("/dest")).toBeInTheDocument());
    fireEvent.click(screen.getByRole("button", { name: /start export/i }));
    await waitFor(() => expect(startExport).toHaveBeenCalled());
    capturedOnProgress()(
      progress({
        exportId: 1,
        phase: "completed",
        messagesWritten: 9,
        outputPaths: ["/dest/chat-r1/export.json"],
      }),
    );
    await waitFor(() => expect(toastSuccess).toHaveBeenCalled());
    const call = toastSuccess.mock.calls[0];
    const opts = call?.[1] as { action?: { label: string; onClick: () => void } };
    expect(opts.action?.label).toMatch(/reveal in finder/i);
    // Invoking the action reveals the first output path.
    opts.action?.onClick();
    expect(revealPath).toHaveBeenCalledWith("/dest/chat-r1/export.json");
    // The in-dialog Reveal button is also present.
    expect(screen.getByRole("button", { name: /reveal in finder/i })).toBeInTheDocument();
  });

  it("shows a persistent alert (not toast-only) on failure noting cleanup", async () => {
    openFolder.mockResolvedValue("/dest");
    render(<ExportDialog />);
    fireEvent.click(screen.getByRole("button", { name: /choose folder/i }));
    await waitFor(() => expect(screen.getByText("/dest")).toBeInTheDocument());
    fireEvent.click(screen.getByRole("button", { name: /start export/i }));
    await waitFor(() => expect(startExport).toHaveBeenCalled());
    capturedOnProgress()(
      progress({ exportId: 1, phase: "failed", error: "destination not writable" }),
    );
    await waitFor(() => expect(screen.getByRole("alertdialog")).toBeInTheDocument());
    expect(screen.getByText(/export failed/i)).toBeInTheDocument();
    expect(screen.getByText(/partial files were deleted/i)).toBeInTheDocument();
    // A failure is NOT delivered as a toast (persistent alert only).
    expect(toastSuccess).not.toHaveBeenCalled();
  });

  it("still shows the persistent failure alert after the dialog is closed mid-run", async () => {
    // UX-DR11: a background export that fails after the user closed the dialog must
    // still surface the persistent alert (the terminal surfaces are always-mounted).
    openFolder.mockResolvedValue("/dest");
    render(<ExportDialog />);
    fireEvent.click(screen.getByRole("button", { name: /choose folder/i }));
    await waitFor(() => expect(screen.getByText("/dest")).toBeInTheDocument());
    fireEvent.click(screen.getByRole("button", { name: /start export/i }));
    await waitFor(() => expect(startExport).toHaveBeenCalled());
    // Close the dialog while the job is still running.
    exportStore.getState().close();
    await waitFor(() =>
      expect(screen.queryByRole("dialog", { name: /export your local archive/i })).toBeNull(),
    );
    // The job then fails — the alert must appear even though the dialog is closed.
    capturedOnProgress()(
      progress({ exportId: 1, phase: "failed", error: "destination not writable" }),
    );
    await waitFor(() => expect(screen.getByRole("alertdialog")).toBeInTheDocument());
    expect(screen.getByText(/partial files were deleted/i)).toBeInTheDocument();
  });

  it("disables the chat/account scopes when the preset lacks their ids", () => {
    exportStore.setState({
      isOpen: true,
      preset: { scope: "everything", accountId: null, roomId: null },
      job: null,
    });
    render(<ExportDialog />);
    expect(screen.getByLabelText("This chat")).toBeDisabled();
    expect(screen.getByLabelText("This account")).toBeDisabled();
    expect(screen.getByLabelText("Everything")).not.toBeDisabled();
  });
});
