import { fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { describe, expect, it } from "vitest";
import { SessionAttachments } from "@/components/agents/session-attachments";
import type { AgentAttachmentVm } from "@/lib/ipc/client";

const file: AgentAttachmentVm = {
  id: "file-1",
  name: "prices.csv",
  mime: "text/csv",
  size: 1204,
  sizeLabel: "2 kB",
  state: "uploading",
  url: "keeper-hq-blob://blob/alpha/sha256:abc",
};

describe("session attachment chips", () => {
  it("announces upload, offline wait, and confirmed delivery without fetching a preview", () => {
    const { rerender } = render(<SessionAttachments attachments={[file]} />);
    for (const state of ["uploading", "waiting", "sent"] as const) {
      rerender(<SessionAttachments attachments={[{ ...file, state }]} />);
      expect(screen.getByRole("button", { name: "Open prices.csv" })).toHaveAccessibleDescription(
        `2 kB · ${state}`,
      );
      expect(screen.getByRole("status")).toHaveTextContent(`prices.csv: ${state}`);
      expect(screen.queryByTitle("Preview prices.csv")).not.toBeInTheDocument();
      expect(screen.queryByRole("img")).not.toBeInTheDocument();
    }
  });

  it("keeps the server's refusal beside its own file and in its announcement", () => {
    const sentence =
      "The agents server does not hold prices.csv, so it refused the message that names it. Attach it again.";
    render(
      <SessionAttachments
        attachments={[
          { ...file, state: "refused", sentence },
          { ...file, id: "other", name: "other.csv", state: "sent" },
        ]}
      />,
    );
    const refused = screen
      .getAllByRole("listitem")
      .find((item) => within(item).queryByRole("button", { name: "Open prices.csv" }));
    expect(refused).toBeDefined();
    expect(within(refused as HTMLElement).getByRole("status")).toHaveTextContent(sentence);
    expect(screen.getByRole("button", { name: "Open prices.csv" })).toHaveAccessibleDescription(
      "2 kB · refused",
    );
    expect(screen.getAllByText(sentence)).toHaveLength(1);
  });

  it("opens the opaque scheme URL only on request and removes the preview on close", async () => {
    render(<SessionAttachments attachments={[file]} />);
    expect(screen.queryByTitle("Preview prices.csv")).not.toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "Open prices.csv" }));
    expect(screen.getByRole("button", { name: "Retry opening" })).toHaveFocus();
    expect(screen.getByTitle("Preview prices.csv")).toHaveAttribute("src", file.url);
    expect(screen.getByRole("link", { name: "Download file" })).toHaveAttribute("href", file.url);
    fireEvent.click(screen.getByRole("button", { name: "Close" }));
    expect(screen.queryByTitle("Preview prices.csv")).not.toBeInTheDocument();
    await waitFor(() =>
      expect(screen.getByRole("button", { name: "Open prices.csv" })).toHaveFocus(),
    );
  });

  it("retries a failed open through the same scheme with a fresh request", () => {
    render(<SessionAttachments attachments={[{ ...file, name: "plan.png", mime: "image/png" }]} />);
    fireEvent.click(screen.getByRole("button", { name: "Open plan.png" }));
    fireEvent.error(screen.getByRole("img", { name: "plan.png" }));
    expect(screen.getByRole("alert")).toHaveTextContent("keeper could not open plan.png");
    fireEvent.click(screen.getByRole("button", { name: "Retry opening" }));
    expect(screen.queryByRole("alert")).not.toBeInTheDocument();
    expect(screen.getByRole("img", { name: "plan.png" })).toHaveAttribute(
      "src",
      `${file.url}?retry=1`,
    );
  });
});
