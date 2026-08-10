import { act, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import type { FilesEntryVm, FilesListingVm } from "@/lib/ipc/client";

// Only the IPC edge is mocked. Everything below the strip — the panel store, the
// viewer registry, the unknown viewer — is the real module, because the defect
// this suite exists to catch is a panel that renders nothing, and a mocked body
// can never render nothing.
const syncBrowse = vi.fn();
const syncOpenEntry = vi.fn();
const revealPath = vi.fn();
vi.mock("@/lib/ipc/client", () => ({
  syncBrowse: (id: unknown, subpath: unknown) => syncBrowse(id, subpath),
  syncOpenEntry: (id: unknown, subpath: unknown) => syncOpenEntry(id, subpath),
  revealPath: (path: unknown) => revealPath(path),
}));

import {
  PANEL_CLOSE_LABEL,
  PANEL_EMPTY_SENTENCE,
  PANEL_REASON_TESTID,
  PANEL_STRIP_LABEL,
  PANEL_TESTID,
  PANEL_UNSUPPORTED_SENTENCE,
  PanelStrip,
  panelFileGoneSentence,
} from "@/components/layout/panel-strip";
import { panelsStore, resetPanelsStoreForTest } from "@/lib/stores/panels";
import { UNKNOWN_VIEWER_TESTID } from "@/lib/viewers";

/** The exact sentence Rust composes for a profile whose drive is out. Verbatim,
 * because the whole point of the state is that it reaches the screen unaltered. */
const DRIVE_IS_OUT =
  "/Volumes/merope/Field is not there. This folder lives on removable media — reattach the volume, then open it again.";

function entry(name: string, relativePath = name): FilesEntryVm {
  return {
    name,
    relativePath,
    absolutePath: `/Users/alice/Vault/${relativePath}`,
    kind: "file",
    sync: { status: "synced", detail: null },
  } as FilesEntryVm;
}

function listed(subpath: string, entries: FilesEntryVm[]): FilesListingVm {
  return {
    profileId: "p1",
    subpath,
    // 45.3's create-in-here verdict. These fixtures are panel-rendering
    // fixtures, so the location deliberately refuses: a panel must render a
    // listing identically whether or not the folder happens to be writable.
    write: { writable: false, reason: "This folder is outside a notes vault." },
    state: "listed",
    entries,
    detail: null,
    truncated: false,
  };
}

/** Render and let the resolution the mount started settle. */
async function mount(): Promise<void> {
  render(<PanelStrip />);
  await act(async () => {
    await Promise.resolve();
  });
}

function clearCookies(): void {
  for (const part of document.cookie.split(";")) {
    const name = part.split("=")[0]?.trim();
    if (name !== undefined && name !== "") {
      // biome-ignore lint/suspicious/noDocumentCookie: arranging or clearing cookie state is this test's subject
      document.cookie = `${name}=; path=/; max-age=0`;
    }
  }
}

beforeEach(() => {
  clearCookies();
  resetPanelsStoreForTest();
  syncBrowse.mockReset();
  syncOpenEntry.mockReset();
  revealPath.mockReset();
});

describe("the panel strip", () => {
  it("says what it is showing when nothing has been opened", async () => {
    await mount();

    expect(screen.getByLabelText(PANEL_STRIP_LABEL)).toBeInTheDocument();
    expect(screen.getByText(PANEL_EMPTY_SENTENCE)).toBeInTheDocument();
  });

  it("draws a resolved file through the registry rather than deciding itself", async () => {
    syncBrowse.mockResolvedValue(listed("docs", [entry("report.pdf", "docs/report.pdf")]));
    panelsStore
      .getState()
      .setActiveTarget({ kind: "file", profileId: "p1", relativePath: "docs/report.pdf" });

    await mount();

    // It listed the file's OWN FOLDER, not the profile root: `sync_browse` is
    // the one directory reader and it carries the containment rule.
    expect(syncBrowse).toHaveBeenCalledWith("p1", "docs");

    // A `.pdf` resolves to the `document` viewer, which wave 2 has not bound —
    // so the registry falls back to the unknown viewer, which is a real
    // rendering of a real file rather than an empty frame. Asserted INSIDE the
    // frame, so this cannot pass on a viewer mounted somewhere else.
    const only = panelsStore.getState().panels[0];
    if (only === undefined) {
      throw new Error("expected one panel");
    }
    await waitFor(() => expect(screen.getByTestId(UNKNOWN_VIEWER_TESTID)).toBeInTheDocument());
    const frame = screen.getByTestId(`${PANEL_TESTID}-${only.id}`);
    expect(within(frame).getByTestId(UNKNOWN_VIEWER_TESTID)).toBeInTheDocument();
    // And the frame names the panel for a reader jumping between them.
    expect(frame).toHaveAttribute("aria-label", "report.pdf");
  });

  it("renders Rust's own reason when the drive is out, and keeps the panel", async () => {
    syncBrowse.mockResolvedValue({
      profileId: "p1",
      subpath: "docs",
      write: { writable: false, reason: "This folder is outside a notes vault." },
      state: "mediaAbsent",
      entries: null,
      detail: DRIVE_IS_OUT,
      truncated: false,
    } satisfies FilesListingVm);
    panelsStore
      .getState()
      .setActiveTarget({ kind: "file", profileId: "p1", relativePath: "docs/report.pdf" });

    await mount();

    await waitFor(() =>
      expect(screen.getByTestId(PANEL_REASON_TESTID)).toHaveTextContent(DRIVE_IS_OUT),
    );
    // The panel keeps its place, so it comes back when the drive does. A strip
    // that dropped it would need the user to find the file again.
    expect(panelsStore.getState().panels).toHaveLength(1);
    expect(panelsStore.getState().panels[0]?.target).toEqual({
      kind: "file",
      profileId: "p1",
      relativePath: "docs/report.pdf",
    });
  });

  it("trusts the state over the entry list, so an unreadable folder never reads as empty", async () => {
    // `FilesListingVm` promises `entries` is non-null exactly under `listed`,
    // and the panel checks the STATE as well rather than only unwrapping. An
    // empty array and a null are different values in TypeScript, and a surface
    // that branched on the array alone would tell someone their moved folder
    // is simply missing this file — the exact confusion the VM's own doc says
    // the two fields exist to prevent.
    syncBrowse.mockResolvedValue({
      profileId: "p1",
      subpath: "docs",
      state: "missing",
      entries: [],
      detail: "That folder is not on disk any more.",
      truncated: false,
    } as unknown as FilesListingVm);
    panelsStore
      .getState()
      .setActiveTarget({ kind: "file", profileId: "p1", relativePath: "docs/report.pdf" });

    await mount();

    await waitFor(() =>
      expect(screen.getByTestId(PANEL_REASON_TESTID)).toHaveTextContent(
        "That folder is not on disk any more.",
      ),
    );
  });

  it("names the file when its folder listed and it was not in it", async () => {
    syncBrowse.mockResolvedValue(listed("docs", [entry("other.md", "docs/other.md")]));
    panelsStore
      .getState()
      .setActiveTarget({ kind: "file", profileId: "p1", relativePath: "docs/report.pdf" });

    await mount();

    await waitFor(() =>
      expect(screen.getByTestId(PANEL_REASON_TESTID)).toHaveTextContent(
        panelFileGoneSentence("report.pdf"),
      ),
    );
  });

  it("shows the message Rust composed when the listing call is refused", async () => {
    syncBrowse.mockRejectedValue({ code: "not_found", message: "That folder is not set up." });
    panelsStore
      .getState()
      .setActiveTarget({ kind: "file", profileId: "gone", relativePath: "a.md" });

    await mount();

    await waitFor(() =>
      expect(screen.getByTestId(PANEL_REASON_TESTID)).toHaveTextContent(
        "That folder is not set up.",
      ),
    );
  });

  it("says so for a target no viewer has been built for yet", async () => {
    // Nothing in wave 1 opens a recording, and the vocabulary carries one. A
    // blank pane is the defect DW-172 shipped; a sentence is not.
    panelsStore.getState().setActiveTarget({ kind: "recording", sessionId: "sess-1" });

    await mount();

    expect(screen.getByText(PANEL_UNSUPPORTED_SENTENCE)).toBeInTheDocument();
  });
});

describe("the panel strip's close control", () => {
  it("is absent on the last panel, because closing it is refused", async () => {
    syncBrowse.mockResolvedValue(listed("", [entry("a.md")]));
    panelsStore.getState().setActiveTarget({ kind: "file", profileId: "p1", relativePath: "a.md" });

    await mount();

    expect(screen.queryByRole("button", { name: PANEL_CLOSE_LABEL })).not.toBeInTheDocument();
  });

  it("closes a panel and leaves the rest in place", async () => {
    syncBrowse.mockResolvedValue(listed("", [entry("a.md"), entry("b.md")]));
    const state = panelsStore.getState();
    state.setActiveTarget({ kind: "file", profileId: "p1", relativePath: "a.md" });
    state.openPanel({ kind: "file", profileId: "p1", relativePath: "b.md" });

    await mount();
    const first = panelsStore.getState().panels[0];
    if (first === undefined) {
      throw new Error("expected two panels");
    }
    const frame = screen.getByTestId(`${PANEL_TESTID}-${first.id}`);

    await act(async () => {
      fireEvent.click(within(frame).getByRole("button", { name: PANEL_CLOSE_LABEL }));
      await Promise.resolve();
    });

    expect(panelsStore.getState().panels).toHaveLength(1);
    expect(panelsStore.getState().panels[0]?.target).toEqual({
      kind: "file",
      profileId: "p1",
      relativePath: "b.md",
    });
    expect(screen.queryByTestId(`${PANEL_TESTID}-${first.id}`)).not.toBeInTheDocument();
  });

  it("focuses a panel the pointer lands in, so the next single click replaces that one", async () => {
    syncBrowse.mockResolvedValue(listed("", [entry("a.md"), entry("b.md")]));
    const state = panelsStore.getState();
    state.setActiveTarget({ kind: "file", profileId: "p1", relativePath: "a.md" });
    state.openPanel({ kind: "file", profileId: "p1", relativePath: "b.md" });

    await mount();
    const first = panelsStore.getState().panels[0];
    if (first === undefined) {
      throw new Error("expected two panels");
    }

    await act(async () => {
      fireEvent.mouseDown(screen.getByTestId(`${PANEL_TESTID}-${first.id}`));
      await Promise.resolve();
    });

    expect(panelsStore.getState().activeId).toBe(first.id);
    panelsStore.getState().setActiveTarget({ kind: "file", profileId: "p1", relativePath: "c.md" });
    expect(panelsStore.getState().panels.map((panel) => panel.target?.kind)).toEqual([
      "file",
      "file",
    ]);
    expect(panelsStore.getState().panels[0]?.target).toEqual({
      kind: "file",
      profileId: "p1",
      relativePath: "c.md",
    });
  });
});
