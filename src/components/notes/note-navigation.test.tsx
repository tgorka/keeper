import { act, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, beforeEach, expect, it, vi } from "vitest";
import { NoteNavigation } from "@/components/notes/note-editor";
import { TooltipProvider } from "@/components/ui/tooltip";
import type * as IpcClient from "@/lib/ipc/client";
import { activePanel, panelsStore, resetPanelsStoreForTest } from "@/lib/stores/panels";

vi.mock("@/lib/ipc/client", async (original) => ({
  ...(await original<typeof IpcClient>()),
  notesBodyRead: vi.fn(async (_vault: string, note: string) => ({ text: `# Note ${note}` })),
}));

beforeEach(resetPanelsStoreForTest);
afterEach(() => vi.useRealTimers());

function navigation(notes = ["A", "B", "C", "D"]) {
  const id = panelsStore.getState().activeId;
  for (const noteId of notes) {
    panelsStore.getState().setActiveTarget({ kind: "note", vaultId: "v", noteId });
  }
  render(
    <TooltipProvider>
      <NoteNavigation panelId={id} />
    </TooltipProvider>,
  );
  return screen.getByRole("button", { name: "Back" });
}

it("keeps two buttons, disables stack ends and opens directional history by right-click", async () => {
  const back = navigation();
  expect(screen.getAllByRole("button")).toHaveLength(2);
  expect(screen.getByRole("button", { name: "Forward" })).toBeDisabled();
  fireEvent.contextMenu(back);
  await screen.findByRole("menuitem", { name: "Note A" });
  expect(screen.getAllByRole("menuitem").map((item) => item.textContent)).toEqual([
    "Note C",
    "Note B",
    "Note A",
  ]);
  fireEvent.click(screen.getByRole("menuitem", { name: "Note A" }));
  expect(activePanel(panelsStore.getState()).target).toEqual({
    kind: "note",
    vaultId: "v",
    noteId: "A",
  });
  expect(back).toBeDisabled();
  const forward = screen.getByRole("button", { name: "Forward" });
  fireEvent.contextMenu(forward);
  await screen.findByRole("menuitem", { name: "Note D" });
  expect(screen.getAllByRole("menuitem").map((item) => item.textContent)).toEqual([
    "Note B",
    "Note C",
    "Note D",
  ]);
});

it("navigates on a short click without opening a menu", () => {
  vi.useFakeTimers();
  const back = navigation();
  fireEvent.pointerDown(back, { button: 0, pointerType: "mouse" });
  act(() => vi.advanceTimersByTime(499));
  fireEvent.pointerUp(back, { button: 0 });
  fireEvent.click(back);
  act(() => vi.advanceTimersByTime(1));
  expect(screen.queryByRole("menu")).toBeNull();
  expect(activePanel(panelsStore.getState()).target).toEqual({
    kind: "note",
    vaultId: "v",
    noteId: "C",
  });
});

it("opens after a 500 ms hold without navigating on release", async () => {
  vi.useFakeTimers();
  const back = navigation();
  fireEvent.pointerDown(back, { button: 0, pointerType: "mouse" });
  act(() => vi.advanceTimersByTime(499));
  expect(screen.queryByRole("menu")).toBeNull();
  await act(async () => vi.advanceTimersByTime(1));
  expect(screen.getByRole("menu", { name: "Back navigation history" })).toBeInTheDocument();
  fireEvent.pointerUp(back, { button: 0 });
  fireEvent.click(back);
  expect(activePanel(panelsStore.getState()).target).toEqual({
    kind: "note",
    vaultId: "v",
    noteId: "D",
  });
  expect(screen.getByRole("menuitem", { name: "Note C" })).toBeInTheDocument();
});

it("opens with Down, limits the short menu to twelve entries, and reveals the whole stack", async () => {
  const back = navigation(Array.from({ length: 21 }, (_, index) => String(index)));
  fireEvent.keyDown(back, { key: "ArrowDown" });
  await screen.findByRole("menuitem", { name: "Note 19" });
  expect(screen.getAllByRole("menuitem")).toHaveLength(13);
  expect(screen.queryByRole("menuitem", { name: "Note 0" })).toBeNull();
  fireEvent.click(screen.getByRole("menuitem", { name: "Show all…" }));
  expect(await screen.findByRole("menuitem", { name: "Note 0" })).toBeInTheDocument();
  expect(screen.getAllByRole("menuitem")).toHaveLength(20);
  fireEvent.keyDown(screen.getByRole("menu"), { key: "Escape" });
  expect(screen.queryByRole("menu")).toBeNull();
});

it("supports local Back/Forward chords without opening history and refuses an empty menu", () => {
  const back = navigation();
  fireEvent.keyDown(back, { key: "ArrowLeft", altKey: true });
  expect(activePanel(panelsStore.getState()).target).toEqual({
    kind: "note",
    vaultId: "v",
    noteId: "C",
  });
  fireEvent.keyDown(back, { key: "ArrowRight", altKey: true });
  expect(activePanel(panelsStore.getState()).target).toEqual({
    kind: "note",
    vaultId: "v",
    noteId: "D",
  });
  fireEvent.contextMenu(screen.getByRole("button", { name: "Forward" }));
  expect(screen.queryByRole("menu")).toBeNull();
});
