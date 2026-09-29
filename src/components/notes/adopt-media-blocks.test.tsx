import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import type { MediaAdoptionVm } from "@/lib/ipc/client";

const adopt = vi.fn<(profileId: string, dryRun: boolean) => Promise<MediaAdoptionVm>>();
vi.mock("@/lib/ipc/client", () => ({
  recordingNotesAdoptMediaBlock: (profileId: string, dryRun: boolean) => adopt(profileId, dryRun),
}));

import { AdoptMediaBlocksDialog } from "./adopt-media-blocks";

beforeEach(() => {
  adopt.mockReset();
});

describe("using the media player in recording notes", () => {
  it("counts first, writes only on confirm, and says what it left alone", async () => {
    adopt.mockResolvedValue({ changed: 3, skipped: ["recordings/hand edited.md"] });

    render(<AdoptMediaBlocksDialog vaultId="v1" open onClose={() => {}} />);

    expect(
      await screen.findByText(/3 recording notes still play their files one by one/),
    ).toBeInTheDocument();
    expect(screen.getByText(/recordings\/hand edited\.md/)).toBeInTheDocument();
    expect(adopt).toHaveBeenCalledTimes(1);
    expect(adopt).toHaveBeenCalledWith("v1", true);

    fireEvent.click(screen.getByRole("button", { name: "Use the media player" }));

    await waitFor(() => expect(adopt).toHaveBeenLastCalledWith("v1", false));
    expect(
      await screen.findByText(/Done: 3 recording notes now play in one media player/),
    ).toBeInTheDocument();
  });

  it("offers nothing to do when no note qualifies", async () => {
    adopt.mockResolvedValue({ changed: 0, skipped: [] });

    render(<AdoptMediaBlocksDialog vaultId="v1" open onClose={() => {}} />);

    expect(
      await screen.findByText("No recording note in this vault still plays its files one by one."),
    ).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "Use the media player" })).toBeNull();
  });

  it("says Rust's sentence when the vault cannot be read", async () => {
    adopt.mockRejectedValue({ code: "notesInvalid", message: "This vault is not set up here." });

    render(<AdoptMediaBlocksDialog vaultId="v1" open onClose={() => {}} />);

    expect(await screen.findByRole("alert")).toHaveTextContent("This vault is not set up here.");
  });
});
