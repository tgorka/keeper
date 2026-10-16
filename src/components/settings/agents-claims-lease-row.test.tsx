import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import {
  AgentsClaimsLeaseRow,
  CLAIMS_LEASE_LABEL,
} from "@/components/settings/agents-claims-lease-row";
import { agentsClaimsLeaseGet, agentsClaimsLeaseSet } from "@/lib/ipc/client";

vi.mock("@/lib/ipc/client", () => ({
  agentsClaimsLeaseGet: vi.fn(),
  agentsClaimsLeaseSet: vi.fn(),
}));
vi.mock("@tauri-apps/api/app", () => ({
  getVersion: vi.fn(() => Promise.resolve("0.11.0")),
}));

beforeEach(() => {
  vi.clearAllMocks();
});

describe("AgentsClaimsLeaseRow (R455)", () => {
  it("shows the stored lease and turns the quarter-hour one on and off again", async () => {
    vi.mocked(agentsClaimsLeaseGet).mockResolvedValue("short");
    vi.mocked(agentsClaimsLeaseSet).mockImplementation((lease) => Promise.resolve(lease));
    render(<AgentsClaimsLeaseRow open />);
    const box = await screen.findByRole("checkbox", { name: CLAIMS_LEASE_LABEL });
    await waitFor(() => expect(box).toBeEnabled());
    expect(box).not.toBeChecked();

    fireEvent.click(box);
    expect(agentsClaimsLeaseSet).toHaveBeenLastCalledWith("quarter-hour");
    await waitFor(() => expect(box).toBeChecked());
    await waitFor(() => expect(box).toBeEnabled());

    fireEvent.click(box);
    expect(agentsClaimsLeaseSet).toHaveBeenLastCalledWith("short");
    await waitFor(() => expect(box).not.toBeChecked());
  });

  it("shows what Rust read back, not what was clicked", async () => {
    // A settings file decides it: the stored choice does not take effect.
    vi.mocked(agentsClaimsLeaseGet).mockResolvedValue("short");
    vi.mocked(agentsClaimsLeaseSet).mockResolvedValue("short");
    render(<AgentsClaimsLeaseRow open />);
    const box = await screen.findByRole("checkbox", { name: CLAIMS_LEASE_LABEL });
    await waitFor(() => expect(box).toBeEnabled());
    fireEvent.click(box);
    await waitFor(() => expect(box).toBeEnabled());
    expect(box).not.toBeChecked();
  });

  it("says why a save failed and keeps the lease it had", async () => {
    vi.mocked(agentsClaimsLeaseGet).mockResolvedValue("quarter-hour");
    vi.mocked(agentsClaimsLeaseSet).mockRejectedValue({
      code: "internal",
      message: "could not write setting: database is locked",
    });
    render(<AgentsClaimsLeaseRow open />);
    const box = await screen.findByRole("checkbox", { name: CLAIMS_LEASE_LABEL });
    await waitFor(() => expect(box).toBeChecked());
    fireEvent.click(box);
    expect(await screen.findByRole("alert")).toHaveTextContent("database is locked");
    expect(box).toBeChecked();
  });
});
