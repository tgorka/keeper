import { act, fireEvent, render, screen, within } from "@testing-library/react";
import { useState } from "react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { SessionDrives } from "@/components/agents/session-drives";
import type * as Ipc from "@/lib/ipc/client";
import type { AgentSessionDrivesVm } from "@/lib/ipc/client";

const mock = vi.hoisted(() => ({ set: vi.fn() }));
vi.mock("@/lib/ipc/client", async (original) => ({
  ...(await original<typeof Ipc>()),
  agentSessionDriveSet: mock.set,
}));

const target = { database: "alpha", sessionId: "s.1" };
const WARNING = "Once you tick a drive, the agent may read it and send what it reads to the web.";

function drives(patch: Partial<AgentSessionDrivesVm> = {}): AgentSessionDrivesVm {
  return {
    drives: [
      { name: "alpha", ticked: false },
      { name: "gamma", ticked: false },
      { name: "beta", ticked: false, reason: "Ben reads alpha but cannot read beta." },
    ],
    warning: WARNING,
    pending: false,
    ...patch,
  };
}

function Harness({ vm }: { vm: AgentSessionDrivesVm }) {
  const [open, setOpen] = useState(true);
  return (
    <SessionDrives
      target={target}
      agentName="Nixie"
      drives={vm}
      open={open}
      onOpenChange={setOpen}
    />
  );
}

// The open menu is modal: what sits outside it is hidden from the accessibility tree until it
// closes, so the chips, the note and the alert are found with `hidden: true` while it is open.
function item(name: string) {
  return screen.getByRole("menuitemcheckbox", { name: new RegExp(`^${name}`) });
}

describe("SessionDrives", () => {
  beforeEach(() => {
    mock.set.mockReset().mockResolvedValue(undefined);
  });

  it("says what ticking allows beside every drive, and again once the first one is ticked", async () => {
    const view = render(<Harness vm={drives()} />);
    expect(screen.getByText("No drive ticked")).toBeInTheDocument();
    const menu = screen.getByRole("menu");
    expect(within(menu).getByText(WARNING)).toBeInTheDocument();
    const gamma = item("gamma");
    expect(gamma).toHaveAttribute("aria-checked", "false");
    expect(gamma).toHaveAccessibleDescription(expect.stringContaining(WARNING));

    await act(async () => {
      fireEvent.click(gamma);
    });
    expect(mock.set).toHaveBeenCalledWith(target, "gamma", true);
    // Rust's next view carries the tick.
    view.rerender(
      <Harness
        vm={drives({
          drives: [
            { name: "alpha", ticked: false },
            { name: "gamma", ticked: true },
            { name: "beta", ticked: false, reason: "Ben reads alpha but cannot read beta." },
          ],
          pending: true,
        })}
      />,
    );
    const note = screen.getByRole("note", { name: "What ticking a drive allows", hidden: true });
    expect(note).toHaveTextContent(WARNING);
    expect(
      within(screen.getByRole("list", { name: "Drives ticked", hidden: true })).getByText("gamma"),
    );
    expect(screen.getByText("Waiting for the server…")).toBeInTheDocument();
    expect(item("gamma")).toHaveAttribute("aria-checked", "true");
  });

  it("unticks through the same menu", async () => {
    render(
      <Harness
        vm={drives({
          drives: [
            { name: "alpha", ticked: true },
            { name: "gamma", ticked: false },
          ],
        })}
      />,
    );
    await act(async () => {
      fireEvent.click(item("alpha"));
    });
    expect(mock.set).toHaveBeenCalledWith(target, "alpha", false);
    expect(screen.queryByRole("note", { hidden: true })).toBeNull();
  });

  it("shows a drive it may not read with Rust's reason and never ticks it", async () => {
    render(<Harness vm={drives()} />);
    const beta = item("beta");
    expect(beta).toHaveAttribute("aria-disabled", "true");
    expect(beta).toHaveAccessibleDescription(
      expect.stringContaining("Ben reads alpha but cannot read beta."),
    );
    await act(async () => {
      fireEvent.click(beta);
    });
    expect(mock.set).not.toHaveBeenCalled();
  });

  it("brings the server's refusal to the drive's row and under the chips", () => {
    const refused = "The server refused gamma: not everyone who reads alpha may read it.";
    render(
      <Harness
        vm={drives({
          drives: [
            { name: "alpha", ticked: false },
            { name: "gamma", ticked: false, refused },
          ],
          refused,
        })}
      />,
    );
    expect(screen.getByRole("alert", { hidden: true })).toHaveTextContent(refused);
    expect(item("gamma")).toHaveAccessibleDescription(expect.stringContaining(refused));
  });

  it("says a write keeper could not make", async () => {
    mock.set.mockRejectedValueOnce({ message: "Only a person in this chat may choose." });
    render(<Harness vm={drives()} />);
    await act(async () => {
      fireEvent.click(item("alpha"));
    });
    expect(screen.getByRole("alert", { hidden: true })).toHaveTextContent(
      "Only a person in this chat may choose.",
    );
    expect(screen.queryByRole("note", { hidden: true })).toBeNull();
  });

  it("announces a refusal inside the open menu, where a screen reader still is", () => {
    const refused = "The server refused gamma: not everyone who reads alpha may read it.";
    const vm = drives({ refused });
    const view = render(<Harness vm={vm} />);
    // Not `hidden: true`: the alert is in the accessibility tree while the menu is open.
    expect(within(screen.getByRole("menu")).getByRole("alert")).toHaveTextContent(refused);
    fireEvent.keyDown(screen.getByRole("menu"), { key: "Escape" });
    view.rerender(<Harness vm={vm} />);
    expect(screen.queryByRole("menu")).toBeNull();
    expect(screen.getByRole("alert")).toHaveTextContent(refused);
  });

  it("marks a ticked drive that stopped being allowed and unticks it from its chip", async () => {
    const reason = "Nixie no longer offers gamma. Untick it to change the other drives.";
    render(
      <Harness
        vm={drives({
          drives: [
            { name: "gamma", ticked: true, reason },
            { name: "alpha", ticked: true },
          ],
        })}
      />,
    );
    expect(item("gamma")).not.toHaveAttribute("aria-disabled", "true");
    expect(screen.getByText(reason, { selector: "p" })).toBeInTheDocument();
    fireEvent.keyDown(screen.getByRole("menu"), { key: "Escape" });
    await act(async () => {
      fireEvent.click(screen.getByRole("button", { name: "Untick gamma" }));
    });
    expect(mock.set).toHaveBeenCalledWith(target, "gamma", false);
    expect(screen.queryByRole("button", { name: "Untick alpha" })).toBeNull();
  });

  it("says once, on every failing chip, that unticking either unticks both", async () => {
    const reason =
      "Untick delta and gamma to change the other drives: Nixie no longer offers them. " +
      "Unticking either unticks both.";
    render(
      <Harness
        vm={drives({
          drives: [
            { name: "delta", ticked: true, reason },
            { name: "gamma", ticked: true, reason },
            { name: "alpha", ticked: true },
          ],
        })}
      />,
    );
    fireEvent.keyDown(screen.getByRole("menu"), { key: "Escape" });
    expect(screen.getAllByText(reason, { selector: "p" })).toHaveLength(1);
    for (const name of ["delta", "gamma"])
      expect(screen.getByRole("button", { name: `Untick ${name}` })).toHaveAccessibleDescription(
        reason,
      );
    await act(async () => {
      fireEvent.click(screen.getByRole("button", { name: "Untick delta" }));
    });
    // Rust's `set` unticks every failing drive in that one write.
    expect(mock.set).toHaveBeenCalledWith(target, "delta", false);
  });
});
