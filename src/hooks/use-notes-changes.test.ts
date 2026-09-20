import { act, renderHook, waitFor } from "@testing-library/react";
import { beforeEach, expect, it, vi } from "vitest";
import { useNotesChanges } from "@/hooks/use-notes-changes";
import type { NoteChangeBatch, NoteListVm, NoteRowVm } from "@/lib/ipc/client";
import { notesList, notesSubscribeChanges } from "@/lib/ipc/client";
import { notesFiltersStore, resetNotesFiltersStoreForTest } from "@/lib/stores/notes-filters";
import { notesListStore, resetNotesListStoreForTest } from "@/lib/stores/notes-list";

vi.mock("@/lib/ipc/client", () => ({
  notesList: vi.fn(),
  notesSubscribeChanges: vi.fn(async () => "changes"),
  notesUnsubscribeChanges: vi.fn(async () => {}),
  notesTree: vi.fn(),
}));
const service: NoteRowVm = {
  id: "service",
  vaultId: "v1",
  path: "agents.md",
  title: "Agent instructions",
  snippet: "",
  hit: null,
  tags: [],
  updatedMs: 1,
  pinned: false,
  archived: false,
  unread: false,
  conflict: false,
  origin: "",
  predicates: [],
  unresolvedTarget: "",
  headRev: "",
  order: { value: 0, source: "default" },
};
const hidden: NoteListVm = {
  rows: [],
  total: 0,
  matched: 0,
  hidden: 1,
  private: 0,
  notice: null,
  offset: 0,
};
const visible: NoteListVm = {
  rows: [service],
  total: 1,
  matched: 1,
  hidden: 0,
  private: 0,
  notice: null,
  offset: 0,
};
const batch: NoteChangeBatch = {
  vaultId: "v1",
  ops: [{ op: "reset", rows: [service] }],
  total: 1,
  matched: 1,
  hidden: 0,
  private: 0,
};
beforeEach(() => {
  vi.clearAllMocks();
  resetNotesFiltersStoreForTest();
  resetNotesListStoreForTest();
  vi.mocked(notesList)
    .mockReset()
    .mockImplementation(async (_vault, query) => (query.hideServiceFiles ? hidden : visible));
});

it("never inserts a hidden service row from the whole-vault stream, and reveals it on toggle", async () => {
  renderHook(() => useNotesChanges("v1"));
  await waitFor(() => expect(notesListStore.getState().hidden).toBe(1));
  const onBatch = vi.mocked(notesSubscribeChanges).mock.calls[0][1];
  await act(async () => onBatch(batch));
  expect(notesListStore.getState().rows).toEqual([]);
  expect(notesListStore.getState().hidden).toBe(1);
  act(() => notesFiltersStore.getState().setHideServiceFiles(false));
  await waitFor(() => expect(notesListStore.getState().rows).toEqual([service]));
  expect(notesListStore.getState().hidden).toBe(0);
});

it("keeps the acknowledged hidden count when a refresh fails", async () => {
  renderHook(() => useNotesChanges("v1"));
  await waitFor(() => expect(notesListStore.getState().hidden).toBe(1));
  vi.mocked(notesList).mockRejectedValueOnce(new Error("index unavailable"));
  await act(async () => vi.mocked(notesSubscribeChanges).mock.calls[0][1](batch));
  expect(notesListStore.getState().hidden).toBe(1);
  expect(notesListStore.getState().rows).toEqual([]);
});

it("drops a streamed refresh that finishes after the eye changed", async () => {
  renderHook(() => useNotesChanges("v1"));
  await waitFor(() => expect(notesListStore.getState().hidden).toBe(1));
  // This repo's ES2022 lib predates Promise.withResolvers.
  let release: (vm: NoteListVm) => void = () => {};
  vi.mocked(notesList).mockImplementationOnce(
    () =>
      new Promise((resolve) => {
        release = resolve;
      }),
  );
  act(() => vi.mocked(notesSubscribeChanges).mock.calls[0][1](batch));
  act(() => notesFiltersStore.getState().setHideServiceFiles(false));
  await waitFor(() => expect(notesListStore.getState().rows).toEqual([service]));
  await act(async () => release(hidden));
  expect(notesListStore.getState().rows).toEqual([service]);
  expect(notesListStore.getState().hidden).toBe(0);
});

it("clears stale rows on a rejected search and recovers on refresh", async () => {
  notesFiltersStore.getState().setHideServiceFiles(false);
  renderHook(() => useNotesChanges("v1"));
  await waitFor(() => expect(notesListStore.getState().rows).toEqual([service]));
  vi.mocked(notesList).mockRejectedValueOnce(new Error("Search failed."));
  act(() => notesFiltersStore.getState().setText("budget"));
  await waitFor(() => expect(notesListStore.getState().searchError).toBe("Search failed."));
  expect(notesListStore.getState().rows).toEqual([]);
  await act(async () => vi.mocked(notesSubscribeChanges).mock.calls[0][1](batch));
  expect(notesListStore.getState().searchError).toBeNull();
  expect(notesListStore.getState().rows).toEqual([service]);
});

it("preserves the window while preference hydration gates the first read", () => {
  notesListStore.getState().reset(visible);
  notesListStore.getState().growWindow();
  const limit = notesListStore.getState().limit;
  renderHook(() => useNotesChanges("v1", false));
  expect(notesList).not.toHaveBeenCalled();
  expect(notesListStore.getState().rows).toEqual([service]);
  expect(notesListStore.getState().limit).toBe(limit);
});

it("re-queries the combined result when any selected drive changes", async () => {
  notesFiltersStore.getState().setVaultIds(["v1", "v2"]);
  renderHook(() => useNotesChanges("v1"));
  await waitFor(() => expect(notesListStore.getState().loaded).toBe(true));
  const second = vi.mocked(notesSubscribeChanges).mock.calls.find(([id]) => id === "v2");
  expect(second).toBeDefined();
  vi.mocked(notesList).mockResolvedValueOnce({
    ...visible,
    rows: [{ ...service, vaultId: "v2" }],
    private: 2,
  });
  await act(async () => second?.[1]({ ...batch, vaultId: "v2" }));
  expect(notesListStore.getState().rows[0]?.vaultId).toBe("v2");
  expect(notesListStore.getState().private).toBe(2);
});

it("rejects a late answer after a drive or private toggle changed the query", async () => {
  let release: (vm: NoteListVm) => void = () => {};
  vi.mocked(notesList).mockImplementationOnce(
    () =>
      new Promise((resolve) => {
        release = resolve;
      }),
  );
  renderHook(() => useNotesChanges("v1"));
  act(() => {
    notesFiltersStore.getState().setVaultIds(["v2"]);
    notesFiltersStore.getState().setIncludePrivate(true);
  });
  await waitFor(() => expect(notesListStore.getState().loaded).toBe(true));
  await act(async () => release({ ...visible, notice: "Old query notice" }));
  expect(notesListStore.getState().rows).toEqual([]);
  expect(notesListStore.getState().notice).toBeNull();
});
