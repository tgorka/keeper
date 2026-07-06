import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { ApprovalDraftVm } from "@/lib/ipc/client";

// Mock the typed IPC wrappers so the pane never touches Tauri. Each is a spy so
// the matrix rows (approve ok/fail, discard, inline edit) are observable.
const listPendingDrafts = vi.fn<() => Promise<ApprovalDraftVm[]>>();
const approveDraft = vi.fn<(a: string, r: string, b: string) => Promise<void>>();
const clearDraft = vi.fn<(a: string, r: string) => Promise<void>>();
const clearDraftMirror = vi.fn<(a: string, r: string) => Promise<void>>();
const saveDraft = vi.fn<(a: string, r: string, b: string) => Promise<void>>();
const mirrorDraft = vi.fn<(a: string, r: string, b: string) => Promise<void>>();

vi.mock("@/lib/ipc/client", () => ({
  listPendingDrafts: () => listPendingDrafts(),
  approveDraft: (a: string, r: string, b: string) => approveDraft(a, r, b),
  clearDraft: (a: string, r: string) => clearDraft(a, r),
  clearDraftMirror: (a: string, r: string) => clearDraftMirror(a, r),
  saveDraft: (a: string, r: string, b: string) => saveDraft(a, r, b),
  mirrorDraft: (a: string, r: string, b: string) => mirrorDraft(a, r, b),
}));

// Mock the toast surface: the discard undo action and approve-fail error are
// observable, and the undo callback is captured so the test can invoke it.
const toastFn = vi.fn();
const toastError = vi.fn();
vi.mock("sonner", () => ({
  toast: Object.assign((message: string, opts?: unknown) => toastFn(message, opts), {
    error: (message: string) => toastError(message),
  }),
}));

import { ApprovalPane } from "@/components/approval/approval-pane";
import { draftsStore } from "@/lib/stores/drafts";

function draft(overrides: Partial<ApprovalDraftVm> = {}): ApprovalDraftVm {
  return {
    accountId: "a1",
    accountUserId: "@alice:example.org",
    hueIndex: 0,
    roomId: "!r1:example.org",
    displayName: "Room One",
    network: null,
    body: "half a message",
    updatedTs: Date.now() - 5 * 60_000,
    ...overrides,
  };
}

beforeEach(() => {
  vi.clearAllMocks();
  listPendingDrafts.mockResolvedValue([]);
  approveDraft.mockResolvedValue(undefined);
  clearDraft.mockResolvedValue(undefined);
  clearDraftMirror.mockResolvedValue(undefined);
  saveDraft.mockResolvedValue(undefined);
  mirrorDraft.mockResolvedValue(undefined);
  draftsStore.getState().clear();
});

afterEach(() => {
  draftsStore.getState().clear();
});

describe("ApprovalPane grouping", () => {
  it("groups pending drafts by account then chat with a silent You proposer", async () => {
    listPendingDrafts.mockResolvedValue([
      draft({ accountId: "a1", roomId: "!r1:x", displayName: "Room One" }),
      draft({ accountId: "a1", roomId: "!r2:x", displayName: "Room Two", body: "b2" }),
      draft({
        accountId: "a2",
        accountUserId: "@bob:example.org",
        roomId: "!r3:x",
        displayName: "Room Three",
        network: "Telegram",
        body: "b3",
      }),
    ]);

    render(<ApprovalPane />);

    // Both account section headers render.
    expect(await screen.findByText("@alice:example.org")).toBeInTheDocument();
    expect(screen.getByText("@bob:example.org")).toBeInTheDocument();
    // Each chat name renders.
    expect(screen.getByText("Room One")).toBeInTheDocument();
    expect(screen.getByText("Room Two")).toBeInTheDocument();
    expect(screen.getByText("Room Three")).toBeInTheDocument();
    // Every row carries the silent "You" proposer.
    expect(screen.getAllByText("You")).toHaveLength(3);
    // The bridged row shows a network badge.
    expect(screen.getByLabelText("Telegram network")).toBeInTheDocument();
    // The row's accessible name carries the account identity so a screen-reader
    // user can tell same-named rooms across accounts apart on this dispatch surface.
    expect(
      screen.getByRole("button", { name: /Draft in Room One on @alice:example\.org/ }),
    ).toBeInTheDocument();
    expect(
      screen.getByRole("button", { name: /Draft in Room Three on @bob:example\.org/ }),
    ).toBeInTheDocument();
  });

  it("lists an unresolved-room draft with the room id as its name and no network", async () => {
    listPendingDrafts.mockResolvedValue([
      draft({ roomId: "!offline:x", displayName: "!offline:x", network: null }),
    ]);
    render(<ApprovalPane />);
    expect(await screen.findByText("!offline:x")).toBeInTheDocument();
    expect(screen.queryByLabelText(/network$/)).not.toBeInTheDocument();
  });
});

describe("ApprovalPane empty state", () => {
  it("shows the verbatim empty-state copy when there are no drafts", async () => {
    listPendingDrafts.mockResolvedValue([]);
    render(<ApprovalPane />);
    expect(
      await screen.findByText(
        "Nothing waiting. Drafts you write stay here until you approve them — nothing sends without you.",
      ),
    ).toBeInTheDocument();
    // No error affordance on a genuinely-empty pane.
    expect(screen.queryByRole("button", { name: /retry/i })).not.toBeInTheDocument();
  });

  // P6: a query rejection with no last-known rows shows the error affordance, not
  // the "nothing waiting" copy — a load failure must never masquerade as empty.
  it("shows an error affordance (not the empty copy) when the query rejects", async () => {
    listPendingDrafts.mockRejectedValue(new Error("ipc down"));
    render(<ApprovalPane />);

    expect(await screen.findByText("Couldn't load pending drafts.")).toBeInTheDocument();
    expect(
      screen.queryByText(
        "Nothing waiting. Drafts you write stay here until you approve them — nothing sends without you.",
      ),
    ).not.toBeInTheDocument();

    // Retry re-runs the query; once it resolves with rows, the list renders.
    listPendingDrafts.mockResolvedValue([draft({ roomId: "!r1:x", displayName: "Room One" })]);
    fireEvent.click(screen.getByRole("button", { name: /retry/i }));
    expect(await screen.findByText("Room One")).toBeInTheDocument();
    expect(screen.queryByText("Couldn't load pending drafts.")).not.toBeInTheDocument();
  });
});

describe("ApprovalPane approve", () => {
  it("dispatches approve and clears local + mirror + marker on success", async () => {
    const d = draft({ accountId: "a1", roomId: "!r1:x", body: "ship it" });
    // The re-query reflects the store: once the marker is cleared (approve success),
    // the authoritative list no longer returns the draft — mirroring the DB clear.
    listPendingDrafts.mockImplementation(() =>
      Promise.resolve(draftsStore.getState().keys.has("a1 !r1:x") ? [d] : []),
    );
    draftsStore.getState().mark("a1", "!r1:x", true);
    render(<ApprovalPane />);

    const row = await screen.findByRole("button", { name: /Draft in Room One/ });
    row.focus();
    fireEvent.keyDown(row, { key: "Enter", metaKey: true });

    await waitFor(() => expect(approveDraft).toHaveBeenCalledWith("a1", "!r1:x", "ship it"));
    await waitFor(() => expect(clearDraft).toHaveBeenCalledWith("a1", "!r1:x"));
    expect(clearDraftMirror).toHaveBeenCalledWith("a1", "!r1:x");
    // Marker cleared → the presence set no longer holds this key.
    await waitFor(() => expect(draftsStore.getState().keys.has("a1 !r1:x")).toBe(false));
    // P4: the approved row is optimistically removed from the pane.
    await waitFor(() =>
      expect(screen.queryByRole("button", { name: /Draft in Room One/ })).not.toBeInTheDocument(),
    );
  });

  // P4: a rapid double ⌘Enter while the first approve is in flight must not dispatch
  // the same draft twice (the in-flight guard drops the second).
  it("ignores a second Cmd+Enter while the first approve is in flight", async () => {
    let resolveApprove: (() => void) | undefined;
    approveDraft.mockImplementation(
      () =>
        new Promise<void>((resolve) => {
          resolveApprove = resolve;
        }),
    );
    const d = draft({ accountId: "a1", roomId: "!r1:x", body: "ship it" });
    listPendingDrafts.mockResolvedValue([d]);
    draftsStore.getState().mark("a1", "!r1:x", true);
    render(<ApprovalPane />);

    const row = await screen.findByRole("button", { name: /Draft in Room One/ });
    row.focus();
    // Two ⌘Enter in quick succession, before the first resolves.
    fireEvent.keyDown(row, { key: "Enter", metaKey: true });
    fireEvent.keyDown(row, { key: "Enter", metaKey: true });

    await waitFor(() => expect(approveDraft).toHaveBeenCalledTimes(1));
    // Let the first (only) dispatch settle.
    resolveApprove?.();
    await waitFor(() => expect(clearDraft).toHaveBeenCalledTimes(1));
    expect(approveDraft).toHaveBeenCalledTimes(1);
  });

  it("retains the draft and shows an error when the send fails", async () => {
    approveDraft.mockRejectedValue(new Error("dispatch failed"));
    const d = draft({ accountId: "a1", roomId: "!r1:x", body: "ship it" });
    listPendingDrafts.mockResolvedValue([d]);
    draftsStore.getState().mark("a1", "!r1:x", true);
    render(<ApprovalPane />);

    const row = await screen.findByRole("button", { name: /Draft in Room One/ });
    row.focus();
    fireEvent.keyDown(row, { key: "Enter", metaKey: true });

    await waitFor(() => expect(toastError).toHaveBeenCalled());
    // Draft retained: no clear, marker intact.
    expect(clearDraft).not.toHaveBeenCalled();
    expect(clearDraftMirror).not.toHaveBeenCalled();
    expect(draftsStore.getState().keys.has("a1 !r1:x")).toBe(true);
  });
});

describe("ApprovalPane discard + undo", () => {
  it("discards the draft with a 5 s undo toast that restores it", async () => {
    const d = draft({ accountId: "a1", roomId: "!r1:x", body: "maybe not" });
    listPendingDrafts.mockResolvedValue([d]);
    draftsStore.getState().mark("a1", "!r1:x", true);
    render(<ApprovalPane />);

    const row = await screen.findByRole("button", { name: /Draft in Room One/ });
    row.focus();
    fireEvent.keyDown(row, { key: "Backspace", metaKey: true });

    // Removed local + mirror + marker.
    await waitFor(() => expect(clearDraft).toHaveBeenCalledWith("a1", "!r1:x"));
    expect(clearDraftMirror).toHaveBeenCalledWith("a1", "!r1:x");
    expect(draftsStore.getState().keys.has("a1 !r1:x")).toBe(false);

    // A 5 s undo toast fired; invoking its action fully restores the draft.
    expect(toastFn).toHaveBeenCalledWith(
      "Draft discarded",
      expect.objectContaining({ duration: 5000 }),
    );
    const opts = toastFn.mock.calls[0][1] as { action: { label: string; onClick: () => void } };
    expect(opts.action.label).toBe("Undo");
    opts.action.onClick();
    // The undo awaits saveDraft before re-marking presence (restore-race guard), so
    // the marker and mirror settle asynchronously.
    await waitFor(() => expect(saveDraft).toHaveBeenCalledWith("a1", "!r1:x", "maybe not"));
    await waitFor(() => expect(draftsStore.getState().keys.has("a1 !r1:x")).toBe(true));
    expect(mirrorDraft).toHaveBeenCalledWith("a1", "!r1:x", "maybe not");
  });

  // P4: a discarded row is optimistically removed from the pane.
  it("optimistically removes the discarded row from the pane", async () => {
    const d = draft({ accountId: "a1", roomId: "!r1:x", body: "maybe not" });
    // The re-query reflects the store: with the marker cleared the list is empty.
    listPendingDrafts.mockImplementation(() =>
      Promise.resolve(draftsStore.getState().keys.has("a1 !r1:x") ? [d] : []),
    );
    draftsStore.getState().mark("a1", "!r1:x", true);
    render(<ApprovalPane />);

    const row = await screen.findByRole("button", { name: /Draft in Room One/ });
    row.focus();
    fireEvent.keyDown(row, { key: "Backspace", metaKey: true });

    await waitFor(() =>
      expect(screen.queryByRole("button", { name: /Draft in Room One/ })).not.toBeInTheDocument(),
    );
  });
});

describe("ApprovalPane inline edit", () => {
  it("opens the editor on Enter and persists via saveDraft + mirrorDraft", async () => {
    const d = draft({ accountId: "a1", roomId: "!r1:x", body: "old body" });
    listPendingDrafts.mockResolvedValue([d]);
    render(<ApprovalPane />);

    const row = await screen.findByRole("button", { name: /Draft in Room One/ });
    row.focus();
    fireEvent.keyDown(row, { key: "Enter" });

    const editor = await screen.findByLabelText("Edit draft for Room One");
    fireEvent.change(editor, { target: { value: "new body" } });
    fireEvent.keyDown(editor, { key: "Enter" });

    await waitFor(() => expect(saveDraft).toHaveBeenCalledWith("a1", "!r1:x", "new body"));
    expect(mirrorDraft).toHaveBeenCalledWith("a1", "!r1:x", "new body");
    // The preview reflects the new body.
    await waitFor(() => expect(screen.getByText("new body")).toBeInTheDocument());
  });

  it("discards when an inline edit is saved trimmed-empty", async () => {
    const d = draft({ accountId: "a1", roomId: "!r1:x", body: "old body" });
    listPendingDrafts.mockResolvedValue([d]);
    draftsStore.getState().mark("a1", "!r1:x", true);
    render(<ApprovalPane />);

    const row = await screen.findByRole("button", { name: /Draft in Room One/ });
    row.focus();
    fireEvent.keyDown(row, { key: "Enter" });

    const editor = await screen.findByLabelText("Edit draft for Room One");
    fireEvent.change(editor, { target: { value: "   " } });
    fireEvent.keyDown(editor, { key: "Enter" });

    // A trimmed-empty save is a discard: clears local + mirror + marker with undo.
    await waitFor(() => expect(clearDraft).toHaveBeenCalledWith("a1", "!r1:x"));
    expect(clearDraftMirror).toHaveBeenCalledWith("a1", "!r1:x");
    expect(saveDraft).not.toHaveBeenCalled();
  });

  // P3: a trimmed-empty Enter must discard exactly once — the editor's unmount blur
  // must NOT fire a second onSaveEdit (which would toast + clear twice).
  it("discards exactly once on a trimmed-empty Enter (blur does not double-fire)", async () => {
    const d = draft({ accountId: "a1", roomId: "!r1:x", body: "old body" });
    listPendingDrafts.mockResolvedValue([d]);
    draftsStore.getState().mark("a1", "!r1:x", true);
    render(<ApprovalPane />);

    const row = await screen.findByRole("button", { name: /Draft in Room One/ });
    row.focus();
    fireEvent.keyDown(row, { key: "Enter" });

    const editor = await screen.findByLabelText("Edit draft for Room One");
    fireEvent.change(editor, { target: { value: "   " } });
    // Enter commits the discard; the ensuing unmount also fires a blur.
    fireEvent.keyDown(editor, { key: "Enter" });
    fireEvent.blur(editor);

    await waitFor(() => expect(clearDraft).toHaveBeenCalledTimes(1));
    expect(clearDraftMirror).toHaveBeenCalledTimes(1);
    // Exactly one discard toast — not two.
    expect(toastFn).toHaveBeenCalledTimes(1);
  });

  // P1: ⌘Enter typed inside the editor must NOT bubble to the row and approve; only
  // the row div's own ⌘Enter approves.
  it("does not approve when Cmd+Enter is typed inside the editor", async () => {
    const d = draft({ accountId: "a1", roomId: "!r1:x", body: "old body" });
    listPendingDrafts.mockResolvedValue([d]);
    render(<ApprovalPane />);

    const row = await screen.findByRole("button", { name: /Draft in Room One/ });
    row.focus();
    fireEvent.keyDown(row, { key: "Enter" });

    const editor = await screen.findByLabelText("Edit draft for Room One");
    fireEvent.change(editor, { target: { value: "new body" } });
    fireEvent.keyDown(editor, { key: "Enter", metaKey: true });

    // The row shortcut must not fire: no approve dispatch.
    expect(approveDraft).not.toHaveBeenCalled();
  });

  // P1 + P3: a plain Enter inside the editor saves and closes it — it must not bubble
  // to the row and re-open the editor after the save.
  it("saves on Enter inside the editor without re-opening it", async () => {
    const d = draft({ accountId: "a1", roomId: "!r1:x", body: "old body" });
    listPendingDrafts.mockResolvedValue([d]);
    render(<ApprovalPane />);

    const row = await screen.findByRole("button", { name: /Draft in Room One/ });
    row.focus();
    fireEvent.keyDown(row, { key: "Enter" });

    const editor = await screen.findByLabelText("Edit draft for Room One");
    fireEvent.change(editor, { target: { value: "new body" } });
    fireEvent.keyDown(editor, { key: "Enter" });

    // Saved once; the editor closed and did NOT re-open (no editor in the DOM).
    await waitFor(() => expect(saveDraft).toHaveBeenCalledWith("a1", "!r1:x", "new body"));
    await waitFor(() =>
      expect(screen.queryByLabelText("Edit draft for Room One")).not.toBeInTheDocument(),
    );
  });

  // The editor is seeded from the row body ONLY on the not-editing→editing
  // transition. An incoming Story 7.2 cross-device mirror edit that lands mid-edit
  // (a re-query returning a different body for the same row) must NEVER re-seed the
  // textarea and clobber the user's in-progress text.
  it("does not re-seed the editor when a mirror edit lands mid-edit", async () => {
    const d = draft({
      accountId: "a1",
      roomId: "!r1:x",
      displayName: "Room One",
      body: "old body",
    });
    listPendingDrafts.mockResolvedValue([d]);
    render(<ApprovalPane />);

    const row = await screen.findByRole("button", { name: /Draft in Room One/ });
    row.focus();
    fireEvent.keyDown(row, { key: "Enter" });

    const editor = (await screen.findByLabelText("Edit draft for Room One")) as HTMLTextAreaElement;
    // The user starts typing in-progress text.
    fireEvent.change(editor, { target: { value: "my in-progress text" } });

    // A cross-device mirror edit arrives: the next authoritative re-query returns a
    // DIFFERENT body for the same row, and a presence change triggers the re-query.
    listPendingDrafts.mockResolvedValue([{ ...d, body: "remote overwrote this" }]);
    draftsStore.getState().mark("a2", "!other:x", true);

    // Give the re-query + re-render a chance to run, then assert the in-progress
    // text survived (the textarea was NOT re-seeded from the incoming body).
    await waitFor(() => expect(listPendingDrafts).toHaveBeenCalledTimes(2));
    expect(editor.value).toBe("my in-progress text");
    expect(editor.value).not.toBe("remote overwrote this");
  });
});

describe("ApprovalPane no bulk affordance", () => {
  it("renders no select-all / approve-all control", async () => {
    listPendingDrafts.mockResolvedValue([
      draft({ roomId: "!r1:x", displayName: "Room One" }),
      draft({ roomId: "!r2:x", displayName: "Room Two", body: "b2" }),
    ]);
    render(<ApprovalPane />);
    await screen.findByText("Room One");
    expect(screen.queryByRole("checkbox")).not.toBeInTheDocument();
    expect(screen.queryByRole("button", { name: /approve all/i })).not.toBeInTheDocument();
    expect(screen.queryByRole("button", { name: /select all/i })).not.toBeInTheDocument();
  });
});
