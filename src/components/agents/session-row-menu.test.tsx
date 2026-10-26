import { fireEvent, render, screen, waitFor, within } from "@testing-library/react";
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
  titleWrite: { state: "confirmed" },
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

// Typing as a browser does: the keys replace whatever is selected, or go in at
// the caret when nothing is.
function type(field: HTMLInputElement, text: string) {
  const start = field.selectionStart ?? field.value.length;
  const end = field.selectionEnd ?? field.value.length;
  fireEvent.change(field, {
    target: { value: field.value.slice(0, start) + text + field.value.slice(end) },
  });
}

it("saves exactly the title typed into a freshly opened rename editor", async () => {
  rename.mockResolvedValue(undefined);
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
  const editor = await screen.findByRole<HTMLInputElement>("textbox", { name: "Session title" });
  await waitFor(() => expect(editor).toHaveFocus());
  expect(editor).toHaveValue("plan");
  type(editor, "hej");
  fireEvent.click(screen.getByRole("button", { name: "Save" }));
  await waitFor(() => expect(rename).toHaveBeenCalledTimes(1));
  expect(rename).toHaveBeenCalledWith(row, "hej");
});

it("shows a later server refusal after local success and recovers the proposal on a fresh mount", async () => {
  rename.mockResolvedValue(undefined);
  const view = (session: AgentSessionRowVm) => (
    <ul>
      <li>
        <SessionRowMenu row={session}>
          <button type="button">{session.displayName}</button>
        </SessionRowMenu>
      </li>
    </ul>
  );
  const mounted = render(view(row));
  fireEvent.contextMenu(screen.getByRole("button", { name: row.displayName }));
  fireEvent.click(await screen.findByRole("menuitem", { name: "Rename…" }));
  fireEvent.change(await screen.findByRole("textbox", { name: "Session title" }), {
    target: { value: "hej" },
  });
  fireEvent.click(screen.getByRole("button", { name: "Save" }));
  await waitFor(() => expect(screen.queryByRole("textbox")).not.toBeInTheDocument());
  const pending: AgentSessionRowVm = {
    ...row,
    title: "hej",
    displayName: "nixi · hej",
    titleWrite: { state: "pending" },
  };
  mounted.rerender(view(pending));
  expect(screen.getByRole("status")).toHaveTextContent("waiting for the server");
  const refused: AgentSessionRowVm = {
    ...row,
    titleWrite: {
      state: "refused",
      title: "hej",
      sentence: "The server refused this rename: you are no longer a member of this chat.",
    },
  };
  mounted.rerender(view(refused));
  expect(screen.getByRole("alert")).toHaveTextContent(
    "The server refused this rename: you are no longer a member of this chat.",
  );
  expect(screen.getByRole("button", { name: "Retry rename" })).toBeVisible();
  mounted.unmount();
  const reopened = render(view(refused));
  fireEvent.contextMenu(screen.getByRole("button", { name: row.displayName }));
  const menu = await screen.findByRole("menu");
  expect(
    within(menu).getByText(
      refused.titleWrite.state === "refused" ? refused.titleWrite.sentence : "",
    ),
  ).toBeVisible();
  fireEvent.click(within(menu).getByRole("menuitem", { name: "Retry rename…" }));
  expect(await screen.findByRole("textbox", { name: "Session title" })).toHaveValue("hej");
  fireEvent.click(screen.getByRole("button", { name: "Cancel" }));
  fireEvent.click(screen.getByRole("button", { name: "Retry rename" }));
  expect(await screen.findByRole("textbox", { name: "Session title" })).toHaveValue("hej");
  fireEvent.click(screen.getByRole("button", { name: "Save" }));
  await waitFor(() => expect(screen.queryByRole("textbox")).not.toBeInTheDocument());
  reopened.rerender(view(pending));
  expect(screen.queryByRole("alert")).not.toBeInTheDocument();
  expect(screen.getByRole("status")).toHaveTextContent("waiting for the server");
  reopened.rerender(view({ ...pending, titleWrite: { state: "confirmed" } }));
  expect(screen.queryByRole("status")).not.toBeInTheDocument();
  expect(screen.queryByRole("alert")).not.toBeInTheDocument();
  expect(screen.queryByRole("button", { name: "Retry rename" })).not.toBeInTheDocument();
});
