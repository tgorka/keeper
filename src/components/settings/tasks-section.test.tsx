import { act, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { TasksSection } from "@/components/settings/tasks-section";
import type { IpcError, SyncProfileVm, TasksLedgerVm, TaskVm } from "@/lib/ipc/client";
import { syncProfiles, syncTasks, syncTasksLedger, syncTasksLedgerSet } from "@/lib/ipc/client";

vi.mock("@/lib/ipc/client", () => ({
  syncProfiles: vi.fn(),
  syncTasks: vi.fn(),
  syncTasksLedger: vi.fn(),
  syncTasksLedgerSet: vi.fn(),
}));

const LEDGER_NOTICE = "Permission denied writing .keeper/keeper.toml";
const ledger: TasksLedgerVm = {
  chosenProfileId: "missing",
  resolvedProfileId: "archive",
  resolvedProfileName: "Archive",
  root: "/archive/tasks",
  subfolder: "tasks",
  subfolderSource: "default",
  writable: false,
  exists: false,
  notice: LEDGER_NOTICE,
};
beforeEach(() => {
  vi.clearAllMocks();
  vi.mocked(syncTasksLedger).mockResolvedValue(ledger);
  vi.mocked(syncProfiles).mockResolvedValue([
    { id: "archive", name: "Archive" },
  ] as SyncProfileVm[]);
  vi.mocked(syncTasks).mockResolvedValue({
    unknown: [],
    tasks: [
      {
        id: "copy",
        kind: "copy",
        ledgerPath: "/archive/tasks/copy",
        copyDestination: "/backup",
      } as TaskVm,
    ],
  });
  vi.mocked(syncTasksLedgerSet).mockResolvedValue(ledger);
});

describe("Tasks settings", () => {
  it("displays engine resolution instead of claiming the unavailable choice is active", async () => {
    render(<TasksSection open />);
    expect(await screen.findByText("/archive/tasks")).toBeInTheDocument();
    expect(screen.getByText(/Subfolder source: default/)).toHaveTextContent(
      "Folder file writable: no",
    );
    expect(screen.getByText(/engine default keeps task logs/)).toBeInTheDocument();
    expect(screen.getByText(LEDGER_NOTICE)).toBeInTheDocument();
    expect(screen.getByRole("combobox", { name: "Ledger folder" })).toHaveValue("missing");
    expect(screen.getByText(/chosen folder could not be honoured/)).toBeInTheDocument();
    expect(screen.getByText("/archive/tasks/copy")).toBeInTheDocument();
    expect(screen.getByText("/backup")).toBeInTheDocument();
    expect(screen.getByText(/keeper-copy-\*\.log/)).toBeInTheDocument();
  });

  it("acknowledges a saved choice only after the engine resolves it and refreshes task paths", async () => {
    render(<TasksSection open />);
    await screen.findByText("/archive/tasks");
    vi.mocked(syncTasksLedgerSet).mockResolvedValue({ ...ledger, chosenProfileId: "archive" });
    vi.mocked(syncTasks).mockResolvedValue({
      unknown: [],
      tasks: [
        {
          id: "copy",
          kind: "copy",
          ledgerPath: "/new-ledger/copy",
          copyDestination: "/backup",
        } as TaskVm,
      ],
    });
    await act(async () =>
      fireEvent.change(screen.getByRole("combobox"), { target: { value: "archive" } }),
    );
    expect(syncTasksLedgerSet).toHaveBeenCalledWith("archive", "tasks");
    expect(screen.getByRole("combobox")).toHaveValue("archive");
    expect(screen.getByText("/new-ledger/copy")).toBeInTheDocument();
    expect(screen.queryByText(/chosen folder could not be honoured/)).not.toBeInTheDocument();
  });

  it("does not display an unsaved choice when the setter refuses", async () => {
    render(<TasksSection open />);
    await screen.findByText("/archive/tasks");
    vi.mocked(syncTasksLedgerSet).mockRejectedValue({
      code: "internal",
      message: "Settings file is read-only",
      accountId: null,
      retriable: false,
    } satisfies IpcError);
    fireEvent.change(screen.getByRole("combobox"), { target: { value: "archive" } });
    await waitFor(() =>
      expect(screen.getByRole("alert")).toHaveTextContent("Settings file is read-only"),
    );
    expect(screen.getByRole("combobox")).toHaveValue("missing");
    expect(screen.getByText("/archive/tasks")).toBeInTheDocument();
  });

  it("shows the typed ledger read refusal without inventing a resolved path", async () => {
    vi.mocked(syncTasksLedger).mockRejectedValueOnce({
      code: "internal",
      message: "Task ledger is unavailable",
      accountId: null,
      retriable: false,
    } satisfies IpcError);
    render(<TasksSection open />);
    expect(await screen.findByRole("alert")).toHaveTextContent("Task ledger is unavailable");
    expect(screen.queryByText("/archive/tasks")).toBeNull();
  });

  it("saves a chosen subfolder and displays the resolved path returned by the write", async () => {
    vi.mocked(syncTasksLedger).mockResolvedValue({ ...ledger, chosenProfileId: "archive" });
    render(<TasksSection open />);
    await screen.findByText("/archive/tasks");
    fireEvent.change(screen.getByLabelText("Task subfolder"), { target: { value: "run-records" } });
    vi.mocked(syncTasksLedgerSet).mockResolvedValue({
      ...ledger,
      chosenProfileId: "archive",
      subfolder: "run-records",
      root: "/archive/run-records",
      subfolderSource: "folder-file",
      writable: true,
      notice: null,
    });
    fireEvent.click(screen.getByRole("button", { name: "Save subfolder" }));
    expect(await screen.findByText("/archive/run-records")).toBeInTheDocument();
    expect(syncTasksLedgerSet).toHaveBeenCalledWith("archive", "run-records");
    expect(screen.getByText(/Subfolder source: folder-file/)).toHaveTextContent(
      "Folder file writable: yes",
    );
  });
});
