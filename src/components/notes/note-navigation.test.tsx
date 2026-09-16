import { fireEvent, render, screen } from "@testing-library/react";
import { beforeEach, expect, it, vi } from "vitest";
import { NoteNavigation } from "@/components/notes/note-editor";
import { TooltipProvider } from "@/components/ui/tooltip";
import type * as IpcClient from "@/lib/ipc/client";
import { activePanel, panelsStore, resetPanelsStoreForTest } from "@/lib/stores/panels";

vi.mock("@/lib/ipc/client", async (original) => ({
  ...(await original<typeof IpcClient>()),
  notesBodyRead: vi.fn(async (_vault: string, note: string) => ({ text: `# Note ${note}` })),
}));

beforeEach(resetPanelsStoreForTest);

it("disables stack ends and opens a named entry directly", async () => {
  const id = panelsStore.getState().activeId;
  for (const noteId of ["A", "B", "C", "D"]) {
    panelsStore.getState().setActiveTarget({ kind: "note", vaultId: "v", noteId });
  }
  render(
    <TooltipProvider>
      <NoteNavigation panelId={id} />
    </TooltipProvider>,
  );
  expect(screen.getByRole("button", { name: "Forward" })).toBeDisabled();
  fireEvent.pointerDown(screen.getByRole("button", { name: "Navigation history" }), {
    button: 0,
    ctrlKey: false,
  });
  fireEvent.click(await screen.findByRole("menuitem", { name: "Note A" }));
  expect(activePanel(panelsStore.getState()).target).toEqual({
    kind: "note",
    vaultId: "v",
    noteId: "A",
  });
  expect(screen.getByRole("button", { name: "Back" })).toBeDisabled();
  fireEvent.click(screen.getByRole("button", { name: "Forward" }));
  expect(activePanel(panelsStore.getState()).target).toEqual({
    kind: "note",
    vaultId: "v",
    noteId: "B",
  });
});
