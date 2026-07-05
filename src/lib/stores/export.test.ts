import { beforeEach, describe, expect, it } from "vitest";
import type { ExportProgressVm } from "@/lib/ipc/client";
import { exportStore } from "@/lib/stores/export";

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

describe("exportStore", () => {
  beforeEach(() => {
    exportStore.setState({
      isOpen: false,
      preset: { scope: "everything", accountId: null, roomId: null },
      job: null,
    });
  });

  it("opens with a preset and clears any prior job", () => {
    exportStore.getState().startJob(1);
    exportStore.getState().open({ scope: "chat", accountId: "acctA", roomId: "!r1" });
    const s = exportStore.getState();
    expect(s.isOpen).toBe(true);
    expect(s.preset).toEqual({ scope: "chat", accountId: "acctA", roomId: "!r1" });
    expect(s.job).toBeNull();
  });

  it("close leaves an in-flight job running", () => {
    exportStore.getState().open({ scope: "everything", accountId: null, roomId: null });
    exportStore.getState().startJob(7);
    exportStore.getState().close();
    expect(exportStore.getState().isOpen).toBe(false);
    expect(exportStore.getState().job?.exportId).toBe(7);
  });

  it("startJob resets the mirror to a fresh running state", () => {
    exportStore.getState().startJob(42);
    const job = exportStore.getState().job;
    expect(job).not.toBeNull();
    expect(job?.exportId).toBe(42);
    expect(job?.phase).toBe("running");
    expect(job?.messagesWritten).toBe(0);
  });

  it("applyProgress folds a matching batch into the job", () => {
    exportStore.getState().startJob(3);
    exportStore
      .getState()
      .applyProgress(
        progress({ exportId: 3, phase: "running", messagesWritten: 5, totalMessages: 10 }),
      );
    const job = exportStore.getState().job;
    expect(job?.messagesWritten).toBe(5);
    expect(job?.totalMessages).toBe(10);
    expect(job?.phase).toBe("running");
  });

  it("applyProgress records the completed terminal batch with output paths", () => {
    exportStore.getState().startJob(3);
    exportStore.getState().applyProgress(
      progress({
        exportId: 3,
        phase: "completed",
        messagesWritten: 9,
        outputPaths: ["/x/export.json", "/x/transcript.md"],
      }),
    );
    const job = exportStore.getState().job;
    expect(job?.phase).toBe("completed");
    expect(job?.outputPaths).toEqual(["/x/export.json", "/x/transcript.md"]);
  });

  it("applyProgress records the failed batch's error", () => {
    exportStore.getState().startJob(3);
    exportStore
      .getState()
      .applyProgress(progress({ exportId: 3, phase: "failed", error: "destination not writable" }));
    expect(exportStore.getState().job?.phase).toBe("failed");
    expect(exportStore.getState().job?.error).toBe("destination not writable");
  });

  it("applyProgress ignores a batch for a stale (superseded) job id", () => {
    exportStore.getState().startJob(5);
    exportStore
      .getState()
      .applyProgress(progress({ exportId: 4, phase: "running", messagesWritten: 99 }));
    // The current job (5) is untouched by the stale (4) batch.
    expect(exportStore.getState().job?.exportId).toBe(5);
    expect(exportStore.getState().job?.messagesWritten).toBe(0);
  });

  it("clearJob drops the mirror", () => {
    exportStore.getState().startJob(1);
    exportStore.getState().clearJob();
    expect(exportStore.getState().job).toBeNull();
  });
});
