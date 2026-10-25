import { act, render, screen, waitFor } from "@testing-library/react";
import { beforeEach, expect, it, vi } from "vitest";
import { useSessionAttachmentDraft } from "@/components/agents/session-attachment-draft";
import type { PendingAttachment } from "@/lib/stores/attachments";

const ipc = vi.hoisted(() => ({ stage: vi.fn(), release: vi.fn() }));
vi.mock("@/lib/ipc/client", () => ({
  agentAttachPath: ipc.stage,
  agentAttachBytes: vi.fn(),
  agentAttachRelease: ipc.release,
}));
vi.mock("@tauri-apps/api/core", () => ({
  convertFileSrc: (handle: string) => `keeper-hq-blob://localhost/${handle}`,
}));
const target = { database: "alpha", sessionId: "device.1" };
const photo: PendingAttachment = {
  id: "photo",
  kind: "path",
  filename: "photo.png",
  path: "/chosen/photo.png",
};
function Tray({ present = true }: { present?: boolean }) {
  const draft = useSessionAttachmentDraft(target);
  return (
    <>
      {present && draft.preview(photo)}
      <button
        type="button"
        onClick={() =>
          void draft.stage(photo).then((handle) => draft.consume([photo.id], [handle]))
        }
      >
        Send
      </button>
    </>
  );
}
beforeEach(() => {
  ipc.stage.mockReset();
  ipc.release.mockReset().mockResolvedValue(undefined);
});
it("releases a removed image even when Rust finishes staging after removal", async () => {
  let resolve: (handles: string[]) => void = () => {
    throw new Error("stage not started");
  };
  ipc.stage.mockImplementation(
    () =>
      new Promise<string[]>((done) => {
        resolve = done;
      }),
  );
  const tray = render(<Tray />);
  tray.rerender(<Tray present={false} />);
  await act(async () => resolve(["latehandle"]));
  await waitFor(() => expect(ipc.release).toHaveBeenCalledWith(target, ["latehandle"]));
  expect(screen.queryByRole("img")).not.toBeInTheDocument();
});
it("sends the preview's immutable snapshot without staging it twice or releasing its transferred handle", async () => {
  ipc.stage.mockResolvedValue(["snapshot"]);
  const tray = render(<Tray />);
  await screen.findByRole("img", { name: "Thumbnail of photo.png" });
  await act(async () => screen.getByRole("button", { name: "Send" }).click());
  // Send consumed the snapshot. Remove the tray as Composer does after commit.
  tray.rerender(<Tray present={false} />);
  tray.unmount();
  expect(ipc.stage).toHaveBeenCalledTimes(1);
  expect(ipc.release).not.toHaveBeenCalledWith(target, ["snapshot"]);
});
