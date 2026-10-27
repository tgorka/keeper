import { act, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { beforeEach, expect, it, vi } from "vitest";
import { AlwaysAllowedSection } from "@/components/agents/always-allowed-section";
import type * as Ipc from "@/lib/ipc/client";
import type { AgentGrantsVm, AgentGrantVm } from "@/lib/ipc/client";

const mock = vi.hoisted(() => ({
  push: null as ((grants: AgentGrantsVm) => void) | null,
  subscribe: vi.fn(),
  unsubscribe: vi.fn().mockResolvedValue(undefined),
  revoke: vi.fn(),
}));
vi.mock("@/lib/ipc/client", async (original) => ({
  ...(await original<typeof Ipc>()),
  agentGrantsSubscribe: mock.subscribe,
  agentGrantsUnsubscribe: mock.unsubscribe,
  agentGrantRevoke: mock.revoke,
}));

const NOTE = "Revoking a grant stops its new uses. Approvals it has already issued still run.";

function grant(patch: Partial<AgentGrantVm> & Pick<AgentGrantVm, "id">): AgentGrantVm {
  return {
    database: "alpha",
    agentActor: "nixi",
    agentName: "Nixi",
    tool: "run",
    scope: "Run “git status …”, whatever follows “status”",
    createdAtMs: 1780000000000,
    state: "active",
    sentence: "Always allowed in every session with Nixi in alpha.",
    canRevoke: true,
    ...patch,
  };
}

beforeEach(() => {
  mock.push = null;
  mock.subscribe.mockReset().mockImplementation(async (onGrants) => {
    mock.push = onGrants;
    return "sub-1";
  });
  mock.revoke.mockReset().mockResolvedValue(undefined);
});

function show(grants: AgentGrantVm[]) {
  render(<AlwaysAllowedSection open />);
  act(() => mock.push?.({ grants, note: NOTE }));
}

it("is absent while the person holds no grant", async () => {
  render(<AlwaysAllowedSection open />);
  await waitFor(() => expect(mock.subscribe).toHaveBeenCalled());
  act(() => mock.push?.({ grants: [], note: NOTE }));
  expect(screen.queryByRole("heading", { name: "Always allowed" })).not.toBeInTheDocument();
});

it("says what revoking does and revokes a grant by its database and id", async () => {
  show([
    grant({ id: "g1" }),
    grant({ id: "g2", state: "revoked", sentence: "Revoked.", canRevoke: false }),
  ]);
  await screen.findByRole("heading", { name: "Always allowed" });
  expect(screen.getByText(NOTE)).toBeInTheDocument();
  const rows = within(screen.getByRole("list", { name: "Always allowed" })).getAllByRole(
    "listitem",
  );
  expect(within(rows[0]).getByText("Run “git status …”, whatever follows “status”")).toBeVisible();
  expect(within(rows[1]).queryByRole("button", { name: /Revoke/ })).not.toBeInTheDocument();
  fireEvent.click(within(rows[0]).getByRole("button", { name: /Revoke/ }));
  await waitFor(() => expect(mock.revoke).toHaveBeenCalledWith("alpha", "g1"));
});

it("shows a refused grant and a refused revocation in Rust's words", async () => {
  show([
    grant({
      id: "g1",
      state: "refused",
      canRevoke: false,
      sentence: "The server refused this grant: that agent does not serve you in this database.",
    }),
    grant({
      id: "g2",
      session: "hej",
      refused: "The server refused revoking it: it was already revoked.",
    }),
  ]);
  const alerts = await screen.findAllByRole("alert");
  expect(alerts.map((alert) => alert.textContent)).toEqual([
    "The server refused this grant: that agent does not serve you in this database.",
    "The server refused revoking it: it was already revoked.",
  ]);
  expect(screen.getByText("This session only: hej")).toBeInTheDocument();
});

it("keeps a failed revoke's sentence beside the list", async () => {
  mock.revoke.mockRejectedValueOnce({ message: "This grant is already revoked." });
  show([grant({ id: "g1" })]);
  fireEvent.click(await screen.findByRole("button", { name: /Revoke/ }));
  await waitFor(() =>
    expect(screen.getByRole("alert")).toHaveTextContent("This grant is already revoked."),
  );
});

it("shows a grant made on a signed-out device as Inactive, in Rust's words, and still revocable", async () => {
  const sentence =
    "Inactive: made on “laptop”, a device that was signed out, so it decides nothing. Revoke it to clear it.";
  show([grant({ id: "g1", state: "inactive", sentence })]);
  const row = within(await screen.findByRole("list", { name: "Always allowed" })).getByRole(
    "listitem",
  );
  expect(row).toHaveAttribute("data-state", "inactive");
  expect(within(row).getByText("Inactive")).toBeVisible();
  expect(within(row).queryByText("Active")).toBeNull();
  expect(within(row).getByText(sentence)).toBeVisible();
  fireEvent.click(within(row).getByRole("button", { name: /Revoke/ }));
  await waitFor(() => expect(mock.revoke).toHaveBeenCalledWith("alpha", "g1"));
});
