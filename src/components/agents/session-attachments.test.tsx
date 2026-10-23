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

  it("reads a blocked file as not sent, never styled as the refused one, Rust's sentences verbatim", () => {
    const refusedSentence =
      "huge.zip is larger than the 25.0 MB the agents server takes from one message, so the server refused it.";
    const blockedSentence =
      "tiny.csv was not sent: the agents server refused huge.zip, attached to the same message. Attach tiny.csv again.";
    render(
      <SessionAttachments
        attachments={[
          { ...file, id: "huge", name: "huge.zip", state: "refused", sentence: refusedSentence },
          { ...file, id: "tiny", name: "tiny.csv", state: "blocked", sentence: blockedSentence },
        ]}
      />,
    );
    const tiny = screen.getByRole("button", { name: "Open tiny.csv" });
    expect(tiny).toHaveAccessibleDescription("2 kB · not sent");
    const [hugeStatus, tinyStatus] = screen.getAllByRole("status");
    expect(tinyStatus).toHaveTextContent(`tiny.csv: not sent${blockedSentence}`);
    expect(tinyStatus).not.toHaveTextContent("blocked");
    expect(screen.getByText(blockedSentence)).not.toHaveClass("text-destructive");
    expect(screen.getByText(blockedSentence)).toHaveClass("text-muted-foreground");
    // Only the file the server actually refused carries the error styling.
    expect(screen.getByRole("button", { name: "Open huge.zip" })).toHaveAccessibleDescription(
      "2 kB · refused",
    );
    expect(hugeStatus).toHaveTextContent(`huge.zip: refused${refusedSentence}`);
    expect(screen.getByText(refusedSentence)).toHaveClass("text-destructive");
    expect(document.querySelectorAll(".text-destructive")).toHaveLength(1);
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

  it("shows only an authorized thumbnail before Open, and retains Open after decode refusal", () => {
    const own = {
      ...file,
      name: "photo.png",
      mime: "image/png",
      thumbnailUrl: "keeper-hq-blob://blob/alpha/sha256:abc?thumbnail=1",
    };
    render(
      <SessionAttachments
        attachments={[own, { ...own, id: "other", name: "other.png", thumbnailUrl: undefined }]}
      />,
    );
    const thumb = screen.getByRole("img", { name: "Thumbnail of photo.png" });
    expect(thumb).toHaveAttribute("src", own.thumbnailUrl);
    expect(screen.queryByRole("img", { name: "Thumbnail of other.png" })).not.toBeInTheDocument();
    expect(screen.queryByRole("img", { name: "photo.png" })).not.toBeInTheDocument();
    fireEvent.error(thumb);
    expect(screen.queryByRole("img", { name: "Thumbnail of photo.png" })).not.toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "Open photo.png" }));
    expect(screen.getByRole("img", { name: "photo.png" })).toHaveAttribute("src", own.url);
  });
});
