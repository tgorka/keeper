import { fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { beforeEach, expect, it, vi } from "vitest";
import { DatabaseApprovalCard } from "@/components/agents/approval-card";
import type * as Ipc from "@/lib/ipc/client";
import type { AgentApprovalVm } from "@/lib/ipc/client";

const mock = vi.hoisted(() => ({
  decide: vi.fn(),
  always: vi.fn(),
}));
vi.mock("@/lib/ipc/client", async (original) => ({
  ...(await original<typeof Ipc>()),
  agentSessionApprovalDecide: mock.decide,
  agentApprovalAlwaysAllow: mock.always,
}));

const STANDING_NARROW =
  "Nixi may run “git status …” in every session you share with it in alpha, without asking — whatever follows “status”.";
const STANDING_WIDE =
  "Nixi may run “git …” in every session you share with it in alpha, without asking — whatever arguments follow.";
const REVOKE_NOTE =
  "Revoking a grant stops its new uses. Approvals it has already issued still run.";

function card(patch: Partial<AgentApprovalVm> = {}): AgentApprovalVm {
  return {
    id: "ap1",
    database: "alpha",
    sessionId: "dev1.1",
    toolName: "run",
    tier: "T2",
    summary: "List the changed files",
    action: '{"argv":["git","status","--porcelain"]}',
    bindingDigest: "digest-ap1",
    expiresAtMs: 1800000000000,
    createdAtMs: 1780000000000,
    state: "pending",
    canDecide: true,
    sentence: "Waiting for you.",
    actions: ["allowOnce", "alwaysAllow", "sessionOnly", "deny"],
    declinedBySend: true,
    grant: {
      agentName: "Nixi",
      tool: "run",
      tier: "T2",
      scopes: [
        {
          id: "programArg",
          label: "“git status …”",
          standing: STANDING_NARROW,
          session:
            "Nixi may run “git status …” in this session, without asking — whatever follows “status”.",
        },
        {
          id: "program",
          label: "“git …”",
          standing: STANDING_WIDE,
          session:
            "Nixi may run “git …” in this session, without asking — whatever arguments follow.",
        },
      ],
      expiries: [
        { label: "Until you revoke it" },
        { days: 1, label: "For a day" },
        { days: 7, label: "For a week" },
      ],
      never: "It never applies in a session that has read outside content, at T4.",
      revokeNote: REVOKE_NOTE,
    },
    ...patch,
  };
}

beforeEach(() => {
  mock.decide.mockReset().mockResolvedValue({ state: "sent", sentence: "Approved." });
  mock.always
    .mockReset()
    .mockResolvedValue({ state: "sent", sentence: "Allowed, and always allowed from now on." });
});

/** Open the card's `…` menu and pick `label`. */
function pick(label: string) {
  const trigger = screen.getByRole("button", { name: "More actions" });
  fireEvent.pointerDown(trigger, { button: 0, ctrlKey: false });
  fireEvent.click(within(screen.getByRole("menu")).getByRole("menuitem", { name: label }));
}

it("lists Rust's actions in the menu, in its order", () => {
  render(<DatabaseApprovalCard approval={card()} />);
  fireEvent.pointerDown(screen.getByRole("button", { name: "More actions" }), { button: 0 });
  expect(
    within(screen.getByRole("menu"))
      .getAllByRole("menuitem")
      .map((item) => item.textContent),
  ).toEqual(["Allow once", "Always allow …", "This session only", "Deny"]);
});

it("leaves Always allow out of a card whose actions do not offer it", () => {
  render(
    <DatabaseApprovalCard
      approval={card({ tier: "T4", actions: ["allowOnce", "deny"], grant: undefined })}
    />,
  );
  fireEvent.pointerDown(screen.getByRole("button", { name: "More actions" }), { button: 0 });
  const items = within(screen.getByRole("menu")).getAllByRole("menuitem");
  expect(items.map((item) => item.textContent)).toEqual(["Allow once", "Deny"]);
});

it("confirms Always allow with its breadth, then writes the chosen scope and expiry", async () => {
  render(<DatabaseApprovalCard approval={card()} />);
  pick("Always allow …");
  const dialog = await screen.findByRole("alertdialog");
  expect(dialog).toHaveTextContent(STANDING_NARROW);
  expect(dialog).toHaveTextContent(REVOKE_NOTE);
  expect(mock.always).not.toHaveBeenCalled();
  // The wider scope says it is wider before anything is written.
  fireEvent.click(within(dialog).getByRole("radio", { name: "“git …”" }));
  expect(dialog).toHaveTextContent(STANDING_WIDE);
  fireEvent.click(within(dialog).getByRole("radio", { name: "For a week" }));
  fireEvent.click(within(dialog).getByRole("button", { name: "Always allow" }));
  await waitFor(() =>
    expect(mock.always).toHaveBeenCalledWith(
      { database: "alpha", sessionId: "dev1.1" },
      "ap1",
      "digest-ap1",
      "program",
      false,
      7,
    ),
  );
});

it("This session only binds the grant to the session, narrowest scope by default", async () => {
  render(<DatabaseApprovalCard approval={card()} />);
  pick("This session only");
  const dialog = await screen.findByRole("alertdialog");
  expect(dialog).toHaveTextContent("in this session, without asking — whatever follows");
  fireEvent.click(within(dialog).getByRole("button", { name: "Allow in this session" }));
  await waitFor(() =>
    expect(mock.always).toHaveBeenCalledWith(
      { database: "alpha", sessionId: "dev1.1" },
      "ap1",
      "digest-ap1",
      "programArg",
      true,
      null,
    ),
  );
});

it("cancelling the confirm writes nothing; Deny from the menu denies", async () => {
  render(<DatabaseApprovalCard approval={card()} />);
  pick("Always allow …");
  fireEvent.click(
    within(await screen.findByRole("alertdialog")).getByRole("button", { name: "Cancel" }),
  );
  await waitFor(() => expect(screen.queryByRole("alertdialog")).not.toBeInTheDocument());
  pick("Deny");
  await waitFor(() =>
    expect(mock.decide).toHaveBeenCalledWith(
      { database: "alpha", sessionId: "dev1.1" },
      "ap1",
      "digest-ap1",
      false,
    ),
  );
  expect(mock.always).not.toHaveBeenCalled();
});

it("shows a refused grant's sentence and keeps the card decidable", async () => {
  mock.always.mockRejectedValueOnce({
    message: "The server refused your decision: that agent does not serve you in this database.",
  });
  render(<DatabaseApprovalCard approval={card()} />);
  pick("Always allow …");
  fireEvent.click(
    within(await screen.findByRole("alertdialog")).getByRole("button", { name: "Always allow" }),
  );
  await waitFor(() =>
    expect(screen.getByRole("alert")).toHaveTextContent("that agent does not serve you"),
  );
  expect(screen.getByRole("button", { name: "Allow once" })).toBeEnabled();
});

it("returns focus to the card's menu once the confirm closes", async () => {
  render(<DatabaseApprovalCard approval={card()} />);
  pick("Always allow …");
  fireEvent.click(
    within(await screen.findByRole("alertdialog")).getByRole("button", { name: "Cancel" }),
  );
  await waitFor(() => expect(screen.queryByRole("alertdialog")).not.toBeInTheDocument());
  await waitFor(() =>
    expect(document.activeElement).toBe(screen.getByRole("button", { name: "More actions" })),
  );
});

it("offers no decision on a T4 request this device cannot decide, and says why (R109T-02)", () => {
  for (const sentence of [
    "Needs a decision from another device.",
    "Can't be decided until the agent's host is known.",
  ]) {
    const { unmount } = render(
      <DatabaseApprovalCard
        approval={card({
          tier: "T4",
          toolName: "delegate",
          canDecide: false,
          actions: [],
          grant: undefined,
          declinedBySend: false,
          sentence,
        })}
      />,
    );
    const article = screen.getByRole("article");
    expect(within(article).getByText(sentence)).toBeVisible();
    expect(within(article).queryByRole("button", { name: "Allow once" })).toBeNull();
    expect(within(article).queryByRole("button", { name: "Deny" })).toBeNull();
    expect(within(article).queryByRole("button", { name: "More actions" })).toBeNull();
    unmount();
  }
});
