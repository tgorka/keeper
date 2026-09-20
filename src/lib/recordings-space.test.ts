import { act, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";

vi.mock("@/lib/ipc/client", () => ({
  notesSpaces: vi.fn(),
  notesSpaceTouch: vi.fn(),
  notesSpacePark: vi.fn(),
  notesVaults: vi.fn(async () => []),
  notesVaultActive: vi.fn(async () => null),
  notesVaultSetActive: vi.fn(async () => {}),
}));

import type { NoteSpaceVm } from "@/lib/ipc/client";
import { notesSpacePark, notesSpaceTouch } from "@/lib/ipc/client";
import { openRecordingsSpace, RECORDINGS_SPACE_KEY } from "@/lib/recordings-space";
import {
  noteQueryFor,
  notesFiltersStore,
  resetNotesFiltersStoreForTest,
} from "@/lib/stores/notes-filters";
import { notesVaultsStore } from "@/lib/stores/notes-vaults";
import { primaryViewStore } from "@/lib/stores/primary-view";

const SPACE: NoteSpaceVm = {
  id: "recordings",
  name: "Recordings",
  defaultKey: RECORDINGS_SPACE_KEY,
  query: "is:recording",
  sort: "modified desc",
  sortEffective: "modified desc",
  limit: 0,
  icon: null,
  order: 0,
  folder: null,
  template: null,
  warnings: [],
  error: null,
  updatedMs: null,
  pinned: false,
  ttlHours: null,
  expiresMs: null,
  expiryPhrase: "",
  text: null,
  parent: null,
  leafName: "Recordings",
  depth: 0,
  descendants: 0,
  restore: {
    tagTerms: {},
    flags: ["recording"],
    origin: null,
    text: null,
    sort: null,
    opaque: false,
  },
};

beforeEach(() => {
  resetNotesFiltersStoreForTest();
  notesVaultsStore.setState({ activeVaultId: "vault" });
  primaryViewStore.getState().setView("inbox");
  vi.mocked(notesSpaceTouch).mockReset().mockResolvedValue(SPACE);
  vi.mocked(notesSpacePark).mockReset().mockResolvedValue(SPACE);
});

describe("openRecordingsSpace", () => {
  it("enters the acknowledged query and clears residue from the last search", async () => {
    notesFiltersStore.getState().setText("old budget");
    openRecordingsSpace(SPACE);
    expect(primaryViewStore.getState().view).toBe("notes");
    await waitFor(() => expect(notesFiltersStore.getState().scope).toMatchObject({ id: SPACE.id }));
    expect(notesFiltersStore.getState().text).toBe("");
    expect(notesFiltersStore.getState().flags).toEqual(["recording"]);
    expect(notesSpacePark).toHaveBeenCalledWith(
      "vault",
      "All notes",
      expect.objectContaining({ text: "old budget" }),
    );
  });

  it("keeps Recordings scoped and preserves in-space edits on repeated entry", async () => {
    const filters = notesFiltersStore.getState();
    filters.enterSpace(SPACE);
    filters.setText("unsaved interview");
    filters.setTagTerm("transcribed", "exclude");
    filters.setSort({ key: "name", dir: "asc" });
    await act(async () => {
      openRecordingsSpace(SPACE);
      openRecordingsSpace(SPACE);
    });
    expect(primaryViewStore.getState().view).toBe("notes");
    expect(noteQueryFor(notesFiltersStore.getState(), 0, 20)).toMatchObject({
      spaceId: SPACE.id,
      text: "unsaved interview",
      tags: { transcribed: "exclude" },
      sort: "name asc",
    });
  });

  it("does not enter a stale space if the touch was refused", async () => {
    vi.mocked(notesSpaceTouch).mockRejectedValueOnce(new Error("Space no longer exists"));
    openRecordingsSpace(SPACE);
    await waitFor(() => expect(notesSpaceTouch).toHaveBeenCalled());
    expect(notesFiltersStore.getState().scope).toEqual({ kind: "all" });
  });
});
