/**
 * Story 44.6: the tray's New Note has to OPEN the note it creates.
 *
 * The defect this suite exists to keep dead is not a wrong behaviour, it is an
 * absent one. `listenNotesOpenNote` was declared in the IPC client and called
 * from nowhere, so `keeper://notes-open-note` — emitted by `tray_new_note` and
 * by Today's Journal — reached no listener at all. The note was created, the
 * window was raised, and the user was shown whatever had been on screen. It
 * failed silently for two epics because nothing about it can fail loudly.
 *
 * So the first assertion is the load-bearing one: **something registered for
 * that event**. The rest describe what it then does.
 */
import { renderHook, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { NoteRefVm } from "@/lib/ipc/client";

type OpenHandler = (event: { payload: NoteRefVm }) => void;
let registered: OpenHandler | undefined;
let registeredFor: string | undefined;
const unlisten = vi.fn();
let listenImpl: (event: string, handler: OpenHandler) => Promise<() => void>;

vi.mock("@tauri-apps/api/event", () => ({
  listen: (event: string, handler: OpenHandler) => listenImpl(event, handler),
}));

const setActiveVault = vi.fn(async (_vaultId: string) => {});
vi.mock("@/lib/stores/notes-vaults", async (importOriginal) => {
  const actual = await importOriginal<typeof import("@/lib/stores/notes-vaults")>();
  return { ...actual, setActiveVault: (id: string) => setActiveVault(id) };
});

import { useNotesOpenNote } from "@/hooks/use-notes-open-note";
import { NOTES_OPEN_NOTE_EVENT } from "@/lib/ipc/client";
import { notesListStore, resetNotesListStoreForTest } from "@/lib/stores/notes-list";
import { notesVaultsStore, resetNotesVaultsStoreForTest } from "@/lib/stores/notes-vaults";
import { primaryViewStore } from "@/lib/stores/primary-view";

const REF: NoteRefVm = {
  vaultId: "vault-a",
  id: "new-1",
  path: "2026-08-09-untitled.md",
  title: "Untitled",
};

function fire(ref: NoteRefVm = REF): void {
  registered?.({ payload: ref });
}

beforeEach(() => {
  registered = undefined;
  registeredFor = undefined;
  unlisten.mockClear();
  setActiveVault.mockClear();
  listenImpl = (event, handler) => {
    registeredFor = event;
    registered = handler;
    return Promise.resolve(unlisten);
  };
  resetNotesListStoreForTest();
  resetNotesVaultsStoreForTest();
  primaryViewStore.setState({ view: "inbox" });
});

afterEach(() => {
  resetNotesListStoreForTest();
  resetNotesVaultsStoreForTest();
  primaryViewStore.setState({ view: "inbox" });
});

describe("useNotesOpenNote", () => {
  it("subscribes to the shell's open-note event at all", async () => {
    renderHook(() => useNotesOpenNote());

    // The whole defect, in one assertion. Before this hook the event had no
    // listener anywhere in the webview.
    await waitFor(() => expect(registered).toBeTypeOf("function"));
    expect(registeredFor).toBe(NOTES_OPEN_NOTE_EVENT);
  });

  it("switches to the notes view and selects the note the shell created", async () => {
    notesVaultsStore.getState().setActiveVaultId("vault-a");
    renderHook(() => useNotesOpenNote());
    await waitFor(() => expect(registered).toBeTypeOf("function"));

    fire();

    expect(primaryViewStore.getState().view).toBe("notes");
    expect(notesListStore.getState().selected).toEqual({
      vaultId: "vault-a",
      noteId: "new-1",
    });
    // The vault was already active, so the switch is not re-asked for: telling
    // Rust to activate the vault it is already on would be a needless round
    // trip on the tray's hot path.
    expect(setActiveVault).not.toHaveBeenCalled();
  });

  it("makes the note's own vault active when the webview is showing another", async () => {
    notesVaultsStore.getState().setActiveVaultId("vault-b");
    renderHook(() => useNotesOpenNote());
    await waitFor(() => expect(registered).toBeTypeOf("function"));

    // The tray acts on the vault Rust considers active, which can differ from
    // the one the webview last showed. The ref carries its own vault for
    // exactly this case.
    fire();

    expect(setActiveVault).toHaveBeenCalledWith("vault-a");
    // Selected with its vault regardless, because selection is stored per
    // vault: pane 3 shows the note the moment its vault becomes active.
    expect(notesListStore.getState().selected).toEqual({
      vaultId: "vault-a",
      noteId: "new-1",
    });
  });

  it("unlistens on unmount", async () => {
    const { unmount } = renderHook(() => useNotesOpenNote());
    await waitFor(() => expect(registered).toBeTypeOf("function"));

    unmount();

    expect(unlisten).toHaveBeenCalledTimes(1);
  });

  it("is inert rather than fatal outside a Tauri host", async () => {
    listenImpl = () => Promise.reject(new Error("no tauri host"));

    expect(() => renderHook(() => useNotesOpenNote())).not.toThrow();
    await Promise.resolve();
    expect(primaryViewStore.getState().view).toBe("inbox");
  });

  it("survives `listen` throwing synchronously", async () => {
    listenImpl = () => {
      throw new Error("tauri internals absent");
    };

    expect(() => renderHook(() => useNotesOpenNote())).not.toThrow();
  });
});
