import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { beforeEach, expect, it, vi } from "vitest";
import { SessionRowMenu } from "@/components/agents/session-row-menu";
import type * as Ipc from "@/lib/ipc/client";
import type { AgentSessionRowVm } from "@/lib/ipc/client";

const rename = vi.hoisted(() => vi.fn());
vi.mock("@/lib/ipc/client", async (original) => ({
  ...(await original<typeof Ipc>()),
  agentSessionRename: rename,
}));
const row: AgentSessionRowVm = {
  database: "alpha",
  sessionId: "device.1",
  title: "plan",
  displayName: "nixi · plan",
  handle: "jolly-pigeon-squeaks",
  createdAtMs: 1700000000000,
  trust: "native",
  instances: ["nixi@electra"],
  agentActor: "nixi",
  agentName: "Nixi",
  agentMark: "N",
  archived: false,
  lastActivityMs: 1700000000000,
};
beforeEach(() => rename.mockReset());

it("retains the proposed title after a refused rename and permits a successful retry", async () => {
  rename.mockRejectedValueOnce(new Error("This device cannot write."));
  rename.mockResolvedValueOnce(undefined);
  render(
    <ul>
      <li>
        <SessionRowMenu row={row}>
          <button type="button">{row.displayName}</button>
        </SessionRowMenu>
      </li>
    </ul>,
  );
  fireEvent.contextMenu(screen.getByRole("button", { name: row.displayName }));
  fireEvent.click(await screen.findByRole("menuitem", { name: "Rename…" }));
  const editor = await screen.findByRole("textbox", { name: "Session title" });
  fireEvent.change(editor, { target: { value: "hej" } });
  fireEvent.click(screen.getByRole("button", { name: "Save" }));
  expect(await screen.findByRole("alert")).toHaveTextContent("This device cannot write.");
  expect(editor).toHaveValue("hej");
  await waitFor(() => expect(screen.getByRole("button", { name: "Save" })).toBeEnabled());
  fireEvent.click(screen.getByRole("button", { name: "Save" }));
  await waitFor(() => expect(screen.queryByRole("textbox")).not.toBeInTheDocument());
  expect(screen.queryByRole("alert")).not.toBeInTheDocument();
});
