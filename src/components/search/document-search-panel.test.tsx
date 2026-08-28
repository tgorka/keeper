/**
 * FR-267 — the notes/sessions half of the search surface.
 *
 * Both hooks are mocked: what is under test is the panel's own contract — that
 * only the selected source scans, that a row opens the document through the one
 * file/note target and closes the surface, and that every state says which one
 * it is rather than showing an empty box (UX-DR44).
 */
import { fireEvent, render, screen } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { NoteSearchHitVm, SessionSearchHitVm } from "@/lib/ipc/client";

interface HookResult<T> {
  hits: T[];
  running: boolean;
  error: string | null;
}

const notesResult: HookResult<NoteSearchHitVm> = { hits: [], running: false, error: null };
const sessionsResult: HookResult<SessionSearchHitVm> = { hits: [], running: false, error: null };
const notesArgs = vi.fn<(vaultId: string | null, query: string) => void>();
const sessionsArgs = vi.fn<(rootId: string | null, query: string) => void>();

vi.mock("@/hooks/use-notes-search", () => ({
  useNotesSearch: (vaultId: string | null, query: string) => {
    notesArgs(vaultId, query);
    return notesResult;
  },
}));
vi.mock("@/hooks/use-sessions-search", () => ({
  useSessionsSearch: (rootId: string | null, query: string) => {
    sessionsArgs(rootId, query);
    return sessionsResult;
  },
}));

import {
  DOCUMENT_SEARCH_ROW_TESTID,
  DocumentSearchPanel,
} from "@/components/search/document-search-panel";
import { notesVaultsStore } from "@/lib/stores/notes-vaults";
import { activePanel, panelsStore, resetPanelsStoreForTest } from "@/lib/stores/panels";
import { primaryViewStore } from "@/lib/stores/primary-view";
import { sessionsRootsStore } from "@/lib/stores/sessions-roots";

/** What the strip is showing — the panel a click on a row would have filled. */
function opened() {
  return activePanel(panelsStore.getState()).target;
}

function noteHit(p: Partial<NoteSearchHitVm> = {}): NoteSearchHitVm {
  return {
    id: p.id ?? "n1",
    path: p.path ?? "ideas/plan.md",
    title: p.title ?? "The plan",
    line: p.line ?? 4,
    snippet: p.snippet ?? "the plan is simple",
  };
}

function sessionHit(p: Partial<SessionSearchHitVm> = {}): SessionSearchHitVm {
  return {
    sessionId: p.sessionId ?? "s1",
    sessionTitle: p.sessionTitle ?? "Round two",
    file: p.file ?? "2026-08-14-1030-plan.md",
    subpath: p.subpath ?? "60-sessions/active/2026-08-14-round-two/2026-08-14-1030-plan.md",
    line: p.line ?? 7,
    snippet: p.snippet ?? "the plan is flat",
  };
}

beforeEach(() => {
  notesResult.hits = [];
  notesResult.running = false;
  notesResult.error = null;
  sessionsResult.hits = [];
  sessionsResult.running = false;
  sessionsResult.error = null;
  notesArgs.mockReset();
  sessionsArgs.mockReset();
  notesVaultsStore.setState({ activeVaultId: "v1" });
  sessionsRootsStore.setState({ activeRootId: "r1" });
  primaryViewStore.setState({ view: "inbox" });
  resetPanelsStoreForTest();
});

afterEach(() => {
  notesVaultsStore.setState({ activeVaultId: null });
  sessionsRootsStore.setState({ activeRootId: null });
  primaryViewStore.setState({ view: "inbox" });
});

describe("DocumentSearchPanel", () => {
  it("scans only the selected source", () => {
    render(<DocumentSearchPanel source="notes" active onClose={vi.fn()} />);
    // The notes hook gets the live vault; the sessions hook gets null, which is
    // how the inactive source costs a render rather than a folder walk.
    expect(notesArgs).toHaveBeenLastCalledWith("v1", "");
    expect(sessionsArgs).toHaveBeenLastCalledWith(null, "");
  });

  it("passes the typed query to the active source", () => {
    render(<DocumentSearchPanel source="sessions" active onClose={vi.fn()} />);
    fireEvent.change(screen.getByLabelText("Search query"), { target: { value: "plan" } });
    expect(sessionsArgs).toHaveBeenLastCalledWith("r1", "plan");
    expect(notesArgs).toHaveBeenLastCalledWith(null, "plan");
  });

  it("opens a note hit in the notes view and closes the surface", () => {
    notesResult.hits = [noteHit()];
    const onClose = vi.fn();
    render(<DocumentSearchPanel source="notes" active onClose={onClose} />);
    fireEvent.click(screen.getByTestId(`${DOCUMENT_SEARCH_ROW_TESTID}-0`));
    expect(primaryViewStore.getState().view).toBe("notes");
    expect(opened()).toEqual({ kind: "note", vaultId: "v1", noteId: "n1" });
    expect(onClose).toHaveBeenCalledTimes(1);
  });

  it("opens a session hit through the Rust-composed subpath", () => {
    sessionsResult.hits = [sessionHit()];
    const onClose = vi.fn();
    render(<DocumentSearchPanel source="sessions" active onClose={onClose} />);
    // The row names the session first: a flat pool holds one `about.md` per
    // session, so the path alone would not say which one this is.
    expect(screen.getByText("Round two · 2026-08-14-1030-plan.md")).toBeInTheDocument();
    fireEvent.click(screen.getByTestId(`${DOCUMENT_SEARCH_ROW_TESTID}-0`));
    expect(primaryViewStore.getState().view).toBe("sessions");
    expect(opened()).toEqual({
      kind: "file",
      profileId: "r1",
      relativePath: "60-sessions/active/2026-08-14-round-two/2026-08-14-1030-plan.md",
    });
    expect(onClose).toHaveBeenCalledTimes(1);
  });

  it("tints the query inside the snippet", () => {
    notesResult.hits = [noteHit({ snippet: "the plan is simple" })];
    render(<DocumentSearchPanel source="notes" active onClose={vi.fn()} />);
    fireEvent.change(screen.getByLabelText("Search query"), { target: { value: "plan" } });
    const marks = document.querySelectorAll("mark");
    expect(marks).toHaveLength(1);
    expect(marks[0]?.textContent).toBe("plan");
  });

  it("names the missing scope rather than showing an empty list", () => {
    notesVaultsStore.setState({ activeVaultId: null });
    render(<DocumentSearchPanel source="notes" active onClose={vi.fn()} />);
    expect(screen.getByText(/No vault is open/)).toBeInTheDocument();
  });

  it("names the missing zone for sessions", () => {
    sessionsRootsStore.setState({ activeRootId: null });
    render(<DocumentSearchPanel source="sessions" active onClose={vi.fn()} />);
    expect(screen.getByText(/No sessions zone is open/)).toBeInTheDocument();
  });

  it("reports a failed scan", () => {
    sessionsResult.error = "zone vanished";
    render(<DocumentSearchPanel source="sessions" active onClose={vi.fn()} />);
    expect(screen.getByRole("alert")).toHaveTextContent("zone vanished");
  });

  it("says it is searching, then says nothing matched", () => {
    sessionsResult.running = true;
    const { rerender } = render(<DocumentSearchPanel source="sessions" active onClose={vi.fn()} />);
    fireEvent.change(screen.getByLabelText("Search query"), { target: { value: "plan" } });
    expect(screen.getByText("Searching…")).toBeInTheDocument();

    sessionsResult.running = false;
    rerender(<DocumentSearchPanel source="sessions" active onClose={vi.fn()} />);
    expect(screen.getByText("No matches in this zone.")).toBeInTheDocument();
  });

  it("does not scan while the surface is closed", () => {
    render(<DocumentSearchPanel source="notes" active={false} onClose={vi.fn()} />);
    expect(notesArgs).toHaveBeenLastCalledWith(null, "");
  });
});
