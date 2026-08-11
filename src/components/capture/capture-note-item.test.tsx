/**
 * "Open in a capture window", the story's one entry point on a note (Story
 * 45.15, FR-191).
 *
 * **This is a second door.** The other is the global hotkey, which opens the
 * prewarmed window and is Story 45.14's. Counting doors is what this file is
 * for: the capture window's own chrome has twelve tests and every one of them
 * enters through a window that is already open, so without this file the act
 * that *makes* a window from a note — the sentence in the story's title — has
 * none.
 *
 * It asserts the CALL and its arguments, not that a menu item rendered. A menu
 * item that renders and hands on the wrong note is a person watching keeper
 * open somebody else's note in a window they asked for on this one.
 */
import { fireEvent, render, screen } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";

const notesCaptureOpen = vi.fn<(target: unknown) => Promise<void>>();
const notesCaptureWindows = vi.fn<() => Promise<unknown[]>>();
vi.mock("@/lib/ipc/client", () => ({
  notesCaptureOpen: (target: unknown) => notesCaptureOpen(target),
  notesCaptureWindows: () => notesCaptureWindows(),
}));

import { CAPTURE_NOTE_LABEL, CaptureNoteItem } from "@/components/capture/capture-note-item";
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuTrigger,
} from "@/components/ui/dropdown-menu";
import { capabilitiesStore, DEFAULT_CAPABILITIES } from "@/lib/stores/capabilities";
import { resetCaptureWindowsStoreForTest } from "@/lib/stores/capture-windows";

/**
 * Mounted inside a real Radix menu rather than bare, because the thing this
 * component is is "one `DropdownMenuItem` and no wrapper" — a wrapper breaks
 * typeahead and arrow-key roving, and only a real menu can tell.
 */
function menu(props: { vaultId: string; noteId: string }) {
  return render(
    <DropdownMenu defaultOpen>
      <DropdownMenuTrigger>Actions</DropdownMenuTrigger>
      <DropdownMenuContent>
        <CaptureNoteItem {...props} />
      </DropdownMenuContent>
    </DropdownMenu>,
  );
}

beforeEach(() => {
  vi.clearAllMocks();
  resetCaptureWindowsStoreForTest();
  notesCaptureOpen.mockResolvedValue(undefined);
  notesCaptureWindows.mockResolvedValue([]);
  capabilitiesStore.getState().applySnapshot({ ...DEFAULT_CAPABILITIES, notes: true });
});

describe("CaptureNoteItem", () => {
  it("opens the note it was given as a capture window", async () => {
    menu({ vaultId: "vault-a", noteId: "note-1" });
    fireEvent.click(await screen.findByRole("menuitem", { name: CAPTURE_NOTE_LABEL }));
    // The target, whole. A mutation that keeps the vault and drops the note —
    // or hands on a note id from somewhere else — resolves the same `undefined`
    // and renders identically.
    expect(notesCaptureOpen).toHaveBeenCalledWith({
      kind: "note",
      vaultId: "vault-a",
      noteId: "note-1",
    });
  });

  it("carries the note it is mounted on, not the first one in the document", async () => {
    // Two items in one menu, which is what a workspace with several note panels
    // produces. A component reading anything but its own props passes the test
    // above and fails this one.
    render(
      <DropdownMenu defaultOpen>
        <DropdownMenuTrigger>Actions</DropdownMenuTrigger>
        <DropdownMenuContent>
          <CaptureNoteItem vaultId="vault-a" noteId="note-1" />
          <CaptureNoteItem vaultId="vault-b" noteId="note-2" />
        </DropdownMenuContent>
      </DropdownMenu>,
    );
    const items = await screen.findAllByRole("menuitem", { name: CAPTURE_NOTE_LABEL });
    expect(items).toHaveLength(2);
    const second = items[1];
    expect(second).toBeDefined();
    if (second !== undefined) {
      fireEvent.click(second);
    }
    expect(notesCaptureOpen).toHaveBeenCalledWith({
      kind: "note",
      vaultId: "vault-b",
      noteId: "note-2",
    });
    expect(notesCaptureOpen).toHaveBeenCalledTimes(1);
  });

  it("is absent where a capture window cannot exist", () => {
    // The same flag the in-app capture chord reads. Absent rather than present
    // and refusing: an item that renders and rejects is an affordance that lies
    // about what this build can do.
    capabilitiesStore.getState().applySnapshot({ ...DEFAULT_CAPABILITIES, notes: false });
    menu({ vaultId: "vault-a", noteId: "note-1" });
    expect(screen.queryByRole("menuitem", { name: CAPTURE_NOTE_LABEL })).toBeNull();
  });
});
