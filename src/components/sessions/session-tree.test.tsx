import { fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { SessionEntryVm } from "@/lib/ipc/client";

const syncOpenEntry = vi.fn();
const revealPath = vi.fn();
vi.mock("@/lib/ipc/client", () => ({
  syncOpenEntry: (id: unknown, subpath: unknown) => syncOpenEntry(id, subpath),
  revealPath: (path: unknown) => revealPath(path),
}));

import { FILES_SYNC_MARK_TESTID } from "@/components/layout/sync-status-mark";
import {
  SESSION_TREE_EMPTY,
  SESSION_TREE_LABEL,
  SESSION_TREE_OPEN_EXTERNAL_LABEL,
  SESSION_TREE_REVEAL_LABEL,
  SESSION_TREE_TRUNCATED,
  SessionTree,
} from "@/components/sessions/session-tree";
import { capabilitiesStore, DEFAULT_CAPABILITIES } from "@/lib/stores/capabilities";

const NOW = Date.now();

/** The fence's own words, as Rust composes them (AD-113). */
const LOCK_SENTENCE =
  "60-sessions/active/2026-08-10-keeper/workspace is inside a session's workspace — scratch that is not versioned, not synced, and dies with the session. keeper reads it but never writes there; promote the file into the session's artifacts instead.";

function entry(over: Partial<SessionEntryVm> & Pick<SessionEntryVm, "name">): SessionEntryVm {
  const relPath = over.relPath ?? over.name;
  return {
    relPath,
    parent: "",
    depth: 1,
    isDir: false,
    subpath: `60-sessions/active/2026-08-10-keeper/${relPath}`,
    absolutePath: `/Users/tgorka/tgdrive/60-sessions/active/2026-08-10-keeper/${relPath}`,
    size: { bytes: 2048, label: "2.0 kB" },
    mtimeMs: NOW - 60_000,
    sync: { status: "synced", detail: null },
    locked: null,
    ...over,
  };
}

/**
 * The zone's own shape: four sections, a file under two of them, and one file
 * two levels down — the case a flat list could not show at all.
 */
function zone(): SessionEntryVm[] {
  return [
    entry({ name: "artifacts", isDir: true, size: null }),
    entry({
      name: "release-notes.md",
      relPath: "artifacts/release-notes.md",
      parent: "artifacts",
      depth: 2,
    }),
    entry({
      name: "shots",
      relPath: "artifacts/shots",
      parent: "artifacts",
      depth: 2,
      isDir: true,
      size: null,
    }),
    entry({
      name: "board.png",
      relPath: "artifacts/shots/board.png",
      parent: "artifacts/shots",
      depth: 3,
    }),
    entry({ name: "workspace", isDir: true, size: null, locked: LOCK_SENTENCE }),
    entry({
      name: "iter-3.md",
      relPath: "workspace/iter-3.md",
      parent: "workspace",
      depth: 2,
      locked: LOCK_SENTENCE,
      sync: { status: "excluded", detail: "workspace/ is excluded by the zone's own pattern." },
    }),
    entry({ name: "README.md" }),
  ];
}

function mount(over: Partial<React.ComponentProps<typeof SessionTree>> = {}) {
  const onOpen = vi.fn();
  const result = render(
    <SessionTree rootId="tgdrive" entries={zone()} truncated={false} onOpen={onOpen} {...over} />,
  );
  return { ...result, onOpen };
}

beforeEach(() => {
  syncOpenEntry.mockResolvedValue(undefined);
  revealPath.mockResolvedValue(undefined);
  capabilitiesStore.setState({
    capabilities: { ...DEFAULT_CAPABILITIES, revealInFileManager: true },
    hydrated: true,
  });
});

afterEach(() => {
  vi.clearAllMocks();
  capabilitiesStore.setState({ capabilities: DEFAULT_CAPABILITIES, hydrated: false });
});

describe("SessionTree", () => {
  it("opens the sections and leaves their subtrees closed", () => {
    mount();
    const tree = screen.getByRole("tree", { name: SESSION_TREE_LABEL });
    // The sections are open, so their direct children render...
    expect(within(tree).getByRole("treeitem", { name: "release-notes.md" })).toBeInTheDocument();
    expect(within(tree).getByRole("treeitem", { name: "iter-3.md" })).toBeInTheDocument();
    // ...and the folder INSIDE a section is not, so what it holds does not.
    expect(within(tree).getByRole("treeitem", { name: "shots" })).toHaveAttribute(
      "aria-expanded",
      "false",
    );
    expect(within(tree).queryByRole("treeitem", { name: "board.png" })).not.toBeInTheDocument();
  });

  it("renders nesting as aria-level, one level per folder", () => {
    mount();
    expect(screen.getByRole("treeitem", { name: "artifacts" })).toHaveAttribute("aria-level", "1");
    expect(screen.getByRole("treeitem", { name: "release-notes.md" })).toHaveAttribute(
      "aria-level",
      "2",
    );
  });

  it("opens a closed folder and reveals what is under it", () => {
    mount();
    screen.getByRole("treeitem", { name: "shots" }).focus();
    fireEvent.keyDown(document.activeElement as Element, { key: "ArrowRight" });
    expect(screen.getByRole("treeitem", { name: "shots" })).toHaveAttribute(
      "aria-expanded",
      "true",
    );
    expect(screen.getByRole("treeitem", { name: "board.png" })).toHaveAttribute("aria-level", "3");
  });

  it("closes a folder and hides its whole subtree, however deep", () => {
    mount();
    // Open `shots` first, so there is a grandchild to hide.
    screen.getByRole("treeitem", { name: "shots" }).focus();
    fireEvent.keyDown(document.activeElement as Element, { key: "ArrowRight" });
    expect(screen.getByRole("treeitem", { name: "board.png" })).toBeInTheDocument();
    // Now close the SECTION — the grandchild goes with it, not just the child.
    screen.getByRole("treeitem", { name: "artifacts" }).focus();
    fireEvent.keyDown(document.activeElement as Element, { key: "ArrowLeft" });
    expect(screen.queryByRole("treeitem", { name: "shots" })).not.toBeInTheDocument();
    expect(screen.queryByRole("treeitem", { name: "board.png" })).not.toBeInTheDocument();
  });

  it("walks with the arrows and keeps exactly one row in the tab order", () => {
    mount();
    const first = screen.getByRole("treeitem", { name: "artifacts" });
    expect(first).toHaveAttribute("tabindex", "0");
    first.focus();
    fireEvent.keyDown(document.activeElement as Element, { key: "ArrowDown" });
    expect(document.activeElement).toBe(screen.getByRole("treeitem", { name: "release-notes.md" }));
    expect(first).toHaveAttribute("tabindex", "-1");
    fireEvent.keyDown(document.activeElement as Element, { key: "End" });
    expect(document.activeElement).toBe(screen.getByRole("treeitem", { name: "README.md" }));
    fireEvent.keyDown(document.activeElement as Element, { key: "Home" });
    expect(document.activeElement).toBe(first);
  });

  it("opens a file on Enter and toggles a folder on Enter", () => {
    const { onOpen } = mount();
    const file = screen.getByRole("treeitem", { name: "release-notes.md" });
    file.focus();
    fireEvent.keyDown(file, { key: "Enter" });
    expect(onOpen).toHaveBeenCalledWith(
      expect.objectContaining({ relPath: "artifacts/release-notes.md" }),
    );
    const folder = screen.getByRole("treeitem", { name: "artifacts" });
    folder.focus();
    fireEvent.keyDown(folder, { key: "Enter" });
    expect(folder).toHaveAttribute("aria-expanded", "false");
  });

  it("says why a locked row is locked, in the fence's own words", () => {
    mount();
    const locked = screen.getByRole("treeitem", { name: "iter-3.md" });
    expect(locked).toHaveAccessibleDescription(expect.stringContaining("never writes there"));
    // The unlocked row says nothing about writing — a lock everywhere is a
    // lock nowhere.
    expect(
      screen.getByRole("treeitem", { name: "release-notes.md" }),
    ).not.toHaveAccessibleDescription(expect.stringContaining("never writes there"));
  });

  it("carries the Files tab's own sync mark and sentence", () => {
    mount();
    const locked = screen.getByRole("treeitem", { name: "iter-3.md" });
    const mark = within(locked).getByTestId(FILES_SYNC_MARK_TESTID);
    expect(mark).toHaveAttribute("data-sync-status", "excluded");
    expect(mark).toHaveAccessibleName("workspace/ is excluded by the zone's own pattern.");
  });

  it("describes a row by its size and age without putting either in its name", () => {
    mount();
    const row = screen.getByRole("treeitem", { name: "release-notes.md" });
    expect(row).toHaveAccessibleName("release-notes.md");
    expect(row).toHaveAccessibleDescription(expect.stringContaining("2.0 kB"));
  });

  it("opens externally and reveals through the profile-relative and absolute paths", async () => {
    mount();
    const row = screen.getByRole("treeitem", { name: "release-notes.md" });
    within(row).getByLabelText(SESSION_TREE_OPEN_EXTERNAL_LABEL).click();
    await waitFor(() => {
      expect(syncOpenEntry).toHaveBeenCalledWith(
        "tgdrive",
        "60-sessions/active/2026-08-10-keeper/artifacts/release-notes.md",
      );
    });
    within(row).getByLabelText(SESSION_TREE_REVEAL_LABEL).click();
    await waitFor(() => {
      expect(revealPath).toHaveBeenCalledWith(
        "/Users/tgorka/tgdrive/60-sessions/active/2026-08-10-keeper/artifacts/release-notes.md",
      );
    });
  });

  it("offers no reveal where the platform has none", () => {
    capabilitiesStore.setState({ capabilities: DEFAULT_CAPABILITIES, hydrated: true });
    mount();
    const row = screen.getByRole("treeitem", { name: "release-notes.md" });
    expect(within(row).queryByLabelText(SESSION_TREE_REVEAL_LABEL)).not.toBeInTheDocument();
    // The other verb is not platform-conditional.
    expect(within(row).getByLabelText(SESSION_TREE_OPEN_EXTERNAL_LABEL)).toBeInTheDocument();
  });

  it("says a truncated walk was truncated, and why", () => {
    mount({ truncated: true });
    expect(screen.getByRole("status")).toHaveTextContent(SESSION_TREE_TRUNCATED);
  });

  it("says an empty session is empty rather than rendering nothing", () => {
    mount({ entries: [] });
    expect(screen.getByText(SESSION_TREE_EMPTY)).toBeInTheDocument();
    expect(screen.queryByRole("tree")).not.toBeInTheDocument();
  });
});
