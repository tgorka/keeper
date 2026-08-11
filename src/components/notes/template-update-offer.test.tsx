import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type {
  TemplateChangeVm,
  TemplateUpdateNoteVm,
  TemplateUpdateOfferVm,
} from "@/lib/ipc/client";

vi.mock("@/lib/ipc/client", () => ({
  notesTemplateUpdatePreview: vi.fn(),
  notesTemplateUpdateApply: vi.fn(),
  notesRestoreRevision: vi.fn(),
}));

import {
  TEMPLATE_OFFER_IDLE_MS,
  TemplateUpdateOffer,
} from "@/components/notes/template-update-offer";
import {
  notesRestoreRevision,
  notesTemplateUpdateApply,
  notesTemplateUpdatePreview,
} from "@/lib/ipc/client";

const mockPreview = vi.mocked(notesTemplateUpdatePreview);
const mockApply = vi.mocked(notesTemplateUpdateApply);
const mockRestore = vi.mocked(notesRestoreRevision);

function change(p: Partial<TemplateChangeVm> = {}): TemplateChangeVm {
  return {
    index: p.index ?? 0,
    removed: p.removed ?? [],
    added: p.added ?? ["## Actions"],
    atLine: p.atLine ?? 5,
    skipped: p.skipped ?? null,
  };
}

function note(
  p: Partial<TemplateUpdateNoteVm> & Pick<TemplateUpdateNoteVm, "noteId" | "title">,
): TemplateUpdateNoteVm {
  return {
    noteId: p.noteId,
    title: p.title,
    path: p.path ?? `notes/${p.noteId}.md`,
    changes: p.changes ?? [change()],
    blocked: p.blocked ?? null,
    stalePath: p.stalePath ?? null,
  };
}

function offer(p: Partial<TemplateUpdateOfferVm> = {}): TemplateUpdateOfferVm {
  return {
    templatePath: p.templatePath ?? "templates/journal.md",
    templateTitle: p.templateTitle ?? "Journal",
    notes: p.notes ?? [note({ noteId: "n1", title: "Monday" })],
    declined: p.declined ?? null,
  };
}

beforeEach(() => {
  vi.useFakeTimers({ shouldAdvanceTime: true });
  mockPreview.mockReset();
  mockApply.mockReset();
  mockRestore.mockReset();
  mockRestore.mockResolvedValue(undefined);
});

afterEach(() => {
  vi.useRealTimers();
});

/** Get past the settle window the offer waits for before it asks anything. */
function settle(): void {
  vi.advanceTimersByTime(TEMPLATE_OFFER_IDLE_MS);
}

describe("TemplateUpdateOffer — nothing happens without a decision", () => {
  it("says nothing at all for a note that is not a template", async () => {
    mockPreview.mockResolvedValue(null);
    render(<TemplateUpdateOffer vaultId="v1" noteId="t1" rev="r1" />);
    settle();

    await waitFor(() => expect(mockPreview).toHaveBeenCalledWith("v1", "t1"));
    expect(screen.queryByRole("button", { name: "Review changes" })).toBeNull();
    // "Not a template" must not read as a refusal.
    expect(screen.queryByRole("status")).toBeNull();
  });

  it("does not ask while the editor is still settling", () => {
    mockPreview.mockResolvedValue(offer());
    render(<TemplateUpdateOffer vaultId="v1" noteId="t1" rev="r1" />);

    vi.advanceTimersByTime(TEMPLATE_OFFER_IDLE_MS - 1);
    expect(mockPreview).not.toHaveBeenCalled();
  });

  it("prints keeper's own sentence when it declines, rather than an empty dialog", async () => {
    mockPreview.mockResolvedValue(
      offer({
        notes: [],
        declined: "No note in this vault records “Journal” as the template it came from.",
      }),
    );
    render(<TemplateUpdateOffer vaultId="v1" noteId="t1" rev="r1" />);
    settle();

    expect(await screen.findByRole("status")).toHaveTextContent(
      "No note in this vault records “Journal” as the template it came from.",
    );
    expect(screen.queryByRole("button", { name: "Review changes" })).toBeNull();
  });

  it("declining the dialog applies nothing", async () => {
    mockPreview.mockResolvedValue(offer());
    render(<TemplateUpdateOffer vaultId="v1" noteId="t1" rev="r1" />);
    settle();

    fireEvent.click(await screen.findByRole("button", { name: "Review changes" }));
    await screen.findByRole("dialog");
    // The button that writes is dead until a note is chosen.
    expect(screen.getByRole("button", { name: "Update 0 notes" })).toBeDisabled();

    fireEvent.click(screen.getAllByRole("button", { name: "Not now" })[0]);
    expect(mockApply).not.toHaveBeenCalled();
  });

  it("has no control that selects every note at once", async () => {
    mockPreview.mockResolvedValue(
      offer({
        notes: [
          note({ noteId: "n1", title: "Monday" }),
          note({ noteId: "n2", title: "Tuesday" }),
          note({ noteId: "n3", title: "Wednesday" }),
        ],
      }),
    );
    render(<TemplateUpdateOffer vaultId="v1" noteId="t1" rev="r1" />);
    settle();
    fireEvent.click(await screen.findByRole("button", { name: "Review changes" }));

    // One checkbox per note and not one more: a "select all" is the destructive
    // reading with a confirmation step in front of it (UX-DR59).
    expect(await screen.findAllByRole("checkbox")).toHaveLength(3);
    expect(screen.queryByRole("checkbox", { name: /all/i })).toBeNull();
  });
});

describe("TemplateUpdateOffer — the preview", () => {
  it("shows each note's changes, where they land, and why the others do not", async () => {
    mockPreview.mockResolvedValue(
      offer({
        notes: [
          note({
            noteId: "n1",
            title: "Monday",
            changes: [
              change({ index: 0, added: ["## Actions"], atLine: 7 }),
              change({
                index: 1,
                removed: ["## Notes"],
                added: ["## Observations"],
                atLine: null,
                skipped: "You have written over this part of the note.",
              }),
            ],
          }),
        ],
      }),
    );
    render(<TemplateUpdateOffer vaultId="v1" noteId="t1" rev="r1" />);
    settle();
    fireEvent.click(await screen.findByRole("button", { name: "Review changes" }));

    expect(await screen.findByText("Lands at line 7")).toBeInTheDocument();
    expect(screen.getByText("+ ## Actions")).toBeInTheDocument();
    expect(screen.getByText("- ## Notes")).toBeInTheDocument();
    expect(screen.getByText("You have written over this part of the note.")).toBeInTheDocument();
  });

  it("cannot tick a note keeper could not put back, and says why", async () => {
    mockPreview.mockResolvedValue(
      offer({
        notes: [
          note({
            noteId: "n1",
            title: "Monday",
            blocked: "“Monday” is not in this vault's history yet.",
          }),
        ],
      }),
    );
    render(<TemplateUpdateOffer vaultId="v1" noteId="t1" rev="r1" />);
    settle();
    fireEvent.click(await screen.findByRole("button", { name: "Review changes" }));

    expect(await screen.findByRole("checkbox", { name: "Monday" })).toBeDisabled();
    expect(screen.getByText("“Monday” is not in this vault's history yet.")).toBeInTheDocument();
    // Its changes are still shown: waiting has to be visibly worth something.
    expect(screen.getByText("+ ## Actions")).toBeInTheDocument();
  });

  it("cannot tick a note where nothing would land", async () => {
    mockPreview.mockResolvedValue(
      offer({
        notes: [
          note({
            noteId: "n1",
            title: "Monday",
            changes: [change({ atLine: null, skipped: "You have written over this part." })],
          }),
        ],
      }),
    );
    render(<TemplateUpdateOffer vaultId="v1" noteId="t1" rev="r1" />);
    settle();
    fireEvent.click(await screen.findByRole("button", { name: "Review changes" }));

    expect(await screen.findByRole("checkbox", { name: "Monday" })).toBeDisabled();
  });
});

describe("TemplateUpdateOffer — accepting", () => {
  it("sends only the ticked notes, and only their appliable changes", async () => {
    mockPreview.mockResolvedValue(
      offer({
        notes: [
          note({
            noteId: "n1",
            title: "Monday",
            changes: [
              change({ index: 0 }),
              change({ index: 1, atLine: null, skipped: "You rewrote this." }),
              change({ index: 2, added: ["## Later"] }),
            ],
          }),
          note({ noteId: "n2", title: "Tuesday" }),
        ],
      }),
    );
    mockApply.mockResolvedValue({ updated: [], skipped: [] });
    render(<TemplateUpdateOffer vaultId="v1" noteId="t1" rev="r1" />);
    settle();
    fireEvent.click(await screen.findByRole("button", { name: "Review changes" }));

    fireEvent.click(await screen.findByRole("checkbox", { name: "Monday" }));
    expect(screen.getByRole("button", { name: "Update 1 note" })).toBeEnabled();
    fireEvent.click(screen.getByRole("button", { name: "Update 1 note" }));

    await waitFor(() => expect(mockApply).toHaveBeenCalledTimes(1));
    expect(mockApply).toHaveBeenCalledWith("v1", {
      templatePath: "templates/journal.md",
      // Tuesday was never ticked; change 1 could not land.
      selections: [{ noteId: "n1", changes: [0, 2] }],
    });
  });

  it("offers each updated note its own undo, against the revision Rust reported", async () => {
    mockPreview.mockResolvedValue(offer());
    mockApply.mockResolvedValue({
      updated: [{ noteId: "n1", title: "Monday", applied: 1, undoRev: "abc123" }],
      skipped: ["“Tuesday” could not be read, so it was left alone."],
    });
    render(<TemplateUpdateOffer vaultId="v1" noteId="t1" rev="r1" />);
    settle();
    fireEvent.click(await screen.findByRole("button", { name: "Review changes" }));
    fireEvent.click(await screen.findByRole("checkbox", { name: "Monday" }));
    fireEvent.click(screen.getByRole("button", { name: "Update 1 note" }));

    expect(await screen.findByText("Updated 1 note.")).toBeInTheDocument();
    expect(
      screen.getByText("“Tuesday” could not be read, so it was left alone."),
    ).toBeInTheDocument();

    fireEvent.click(screen.getByRole("button", { name: "Undo" }));
    await waitFor(() => expect(mockRestore).toHaveBeenCalledWith("v1", "n1", "abc123"));
    expect(await screen.findByRole("button", { name: "Undone" })).toBeDisabled();
  });

  it("says so when the apply itself failed", async () => {
    mockPreview.mockResolvedValue(offer());
    mockApply.mockRejectedValue({
      code: "notesInvalid",
      message:
        "keeper no longer knows what this template said before it was edited, so it will not change any note from it.",
    });
    render(<TemplateUpdateOffer vaultId="v1" noteId="t1" rev="r1" />);
    settle();
    fireEvent.click(await screen.findByRole("button", { name: "Review changes" }));
    fireEvent.click(await screen.findByRole("checkbox", { name: "Monday" }));
    fireEvent.click(screen.getByRole("button", { name: "Update 1 note" }));

    expect(await screen.findByText(/no longer knows what this template said/)).toBeInTheDocument();
  });
});
