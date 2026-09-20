import {
  act,
  createEvent,
  fireEvent,
  render,
  renderHook,
  screen,
  waitFor,
} from "@testing-library/react";
import { createRef } from "react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { NoteFilterBar } from "@/components/notes/note-filter-bar";
import { useNotesChanges } from "@/hooks/use-notes-changes";
import type { NoteVaultVm } from "@/lib/ipc/client";
import { notesCreate, notesList, notesSubscribeChanges, tagsVocabulary } from "@/lib/ipc/client";
import {
  noteQueryFor,
  notesFiltersStore,
  resetNotesFiltersStoreForTest,
} from "@/lib/stores/notes-filters";
import { notesListStore, resetNotesListStoreForTest } from "@/lib/stores/notes-list";
import { notesSearchStateStore } from "@/lib/stores/notes-search-state";
import { notesVaultsStore } from "@/lib/stores/notes-vaults";

vi.mock("@/lib/ipc/client", () => ({
  tagsVocabulary: vi.fn(),
  notesCreate: vi.fn(),
  notesIncludePrivateGet: vi.fn(async () => false),
  notesIncludePrivateSet: vi.fn(async () => {}),
  notesList: vi.fn(),
  notesSubscribeChanges: vi.fn(async () => "changes"),
  notesUnsubscribeChanges: vi.fn(async () => {}),
}));

beforeEach(() => {
  resetNotesFiltersStoreForTest();
  resetNotesListStoreForTest();
  notesSearchStateStore.setState({ byVault: {} });
  notesVaultsStore.setState({
    activeVaultId: "v1",
    vaults: [
      { id: "v1", name: "Work" },
      { id: "v2", name: "Personal" },
    ] as NoteVaultVm[],
  });
  vi.mocked(tagsVocabulary)
    .mockReset()
    .mockResolvedValue({
      entries: [
        { path: "client", count: 3 },
        { path: "client/acme", count: 2 },
        { path: "draft", count: 1 },
      ],
    });
  vi.mocked(notesCreate)
    .mockReset()
    .mockResolvedValue({
      note: { id: "new", vaultId: "v1", path: "new.md", title: "New note" },
      notices: [],
    });
  vi.mocked(notesSubscribeChanges).mockClear();
  vi.mocked(notesList).mockResolvedValue({
    rows: [],
    total: 0,
    matched: 0,
    hidden: 0,
    private: 0,
    notice: null,
    offset: 0,
  });
});

function mount() {
  render(<NoteFilterBar onSaveAsSpace={vi.fn()} />);
  return screen.getByRole("combobox", { name: "Search notes" }) as HTMLTextAreaElement;
}

async function typePrompt(prompt: string, caret = prompt.length) {
  const field = mount();
  act(() => field.focus());
  fireEvent.change(field, {
    target: { value: prompt, selectionStart: caret, selectionEnd: caret },
  });
  await screen.findByRole("option", { name: "Include tag client" });
  return field;
}

describe("inline signed suggestions", () => {
  it("accepts only an active option, replacing just its token and preserving the caret", async () => {
    const field = await typePrompt("budget clie for Bali", 11);
    fireEvent.keyDown(field, { key: "Enter" });
    expect(notesFiltersStore.getState().tagTerms).toEqual([]);
    fireEvent.keyDown(field, { key: "ArrowDown" });
    expect(field).toHaveAttribute("aria-activedescendant");
    fireEvent.keyDown(field, { key: "Enter" });
    expect(field).toHaveValue("budget  for Bali");
    expect(field.selectionStart).toBe(7);
    expect(field).toHaveFocus();
    expect(notesFiltersStore.getState().tagTerms).toEqual([{ tag: "client", term: "include" }]);
  });

  it("dismisses on typing past the token and reopens on a caret click", async () => {
    const field = await typePrompt("client");
    fireEvent.change(field, { target: { value: "client trip", selectionStart: 11 } });
    expect(screen.queryByRole("listbox")).toBeNull();
    field.setSelectionRange(3, 3);
    fireEvent.click(field);
    expect(screen.getByRole("option", { name: "Exclude tag client" })).toBeVisible();
    fireEvent.keyDown(field, { key: "Escape" });
    expect(field).toHaveValue("client trip");
    expect(screen.queryByRole("listbox")).toBeNull();
    fireEvent.click(field);
    expect(screen.getByRole("listbox")).toBeVisible();
  });

  it("uses the typed minus alias without eating surrounding whitespace", async () => {
    const field = await typePrompt("plan -clie now", 10);
    fireEvent.keyDown(field, { key: "ArrowDown" });
    fireEvent.keyDown(field, { key: "Enter" });
    expect(field).toHaveValue("plan  now");
    expect(notesFiltersStore.getState().tagTerms).toEqual([{ tag: "client", term: "exclude" }]);
  });

  it("never accepts or removes chips during composition", async () => {
    notesFiltersStore.getState().setTagTerm("draft", "include");
    const field = await typePrompt("client");
    fireEvent.compositionStart(field);
    fireEvent.keyDown(field, { key: "ArrowDown", isComposing: true });
    fireEvent.keyDown(field, { key: "Enter", isComposing: true });
    field.setSelectionRange(0, 0);
    fireEvent.keyDown(field, { key: "Backspace", isComposing: true });
    expect(notesFiltersStore.getState().tagTerms).toEqual([{ tag: "draft", term: "include" }]);
    expect(field).toHaveValue("client");
  });

  it("loads vocabulary on focus, not mount, and exposes a failed load without blocking editing", async () => {
    vi.mocked(tagsVocabulary).mockRejectedValue(new Error("offline"));
    const field = mount();
    expect(tagsVocabulary).not.toHaveBeenCalled();
    act(() => field.focus());
    fireEvent.change(field, { target: { value: "client" } });
    await screen.findByText(/Could not load tags/);
    expect(field).toHaveValue("client");
    expect(screen.queryByRole("option")).toBeNull();
    vi.mocked(tagsVocabulary).mockResolvedValue({ entries: [{ path: "client", count: 1 }] });
    act(() => notesFiltersStore.getState().requestSpacesReload());
    expect(await screen.findByRole("option", { name: "Include tag client" })).toBeVisible();
    expect(screen.queryByText(/Could not load tags/)).toBeNull();
  });

  it("reopens dismissed suggestions with Down while preserving text and focus", async () => {
    const field = await typePrompt("client");
    fireEvent.keyDown(field, { key: "Escape" });
    expect(screen.queryByRole("listbox")).toBeNull();
    fireEvent.keyDown(field, { key: "ArrowDown" });
    expect(screen.getByRole("option", { name: "Include tag client" })).toBeVisible();
    expect(field).toHaveValue("client");
    expect(field).toHaveFocus();
    fireEvent.keyDown(field, { key: "ArrowDown" });
    fireEvent.keyDown(field, { key: "Enter" });
    expect(notesFiltersStore.getState().tagTerms).toEqual([{ tag: "client", term: "include" }]);
  });

  it("submits with Enter and reserves hard line breaks for Shift+Enter", async () => {
    const field = await typePrompt("client");
    for (const dismiss of [false, true]) {
      if (dismiss) fireEvent.keyDown(field, { key: "Escape" });
      const enter = createEvent.keyDown(field, { key: "Enter", cancelable: true });
      fireEvent(field, enter);
      expect(enter.defaultPrevented).toBe(true);
      const newline = createEvent.keyDown(field, {
        key: "Enter",
        shiftKey: true,
        cancelable: true,
      });
      fireEvent(field, newline);
      expect(newline.defaultPrevented).toBe(false);
    }
    expect(field).toHaveValue("client");
    expect(notesFiltersStore.getState().tagTerms).toEqual([]);
  });

  it("refreshes suggestions on the same vault batch that refreshes the list", async () => {
    renderHook(() => useNotesChanges("v1"));
    const field = await typePrompt("client");
    vi.mocked(tagsVocabulary).mockResolvedValue({
      entries: [{ path: "client/new-arrival", count: 1 }],
    });
    await act(async () =>
      vi.mocked(notesSubscribeChanges).mock.calls[0][1]({
        vaultId: "v1",
        ops: [],
        total: 0,
        matched: 0,
        hidden: 0,
        private: 0,
      }),
    );
    expect(
      await screen.findByRole("option", { name: "Include tag client/new-arrival" }),
    ).toBeVisible();
    expect(screen.queryByRole("option", { name: "Include tag client" })).toBeNull();
    expect(field).toHaveValue("client");
  });
});

describe("one field retains its controls", () => {
  it.each([" ", "budget"])("clears %j in one activation and leaves filters intact", (text) => {
    notesFiltersStore.getState().setTagTerm("draft", "exclude");
    notesFiltersStore.getState().setText(text);
    const ref = createRef<HTMLTextAreaElement>();
    render(<NoteFilterBar onSaveAsSpace={vi.fn()} searchRef={ref} />);
    const clear = screen.getByRole("button", { name: "Clear search" });
    fireEvent.click(clear);
    expect(ref.current).toHaveValue("");
    expect(ref.current).toHaveFocus();
    expect(clear).toBeDisabled();
    expect(notesFiltersStore.getState().tagTerms).toEqual([{ tag: "draft", term: "exclude" }]);
  });

  it("removes the last chip only at a collapsed caret start; clicks retain the three-state cycle", () => {
    notesFiltersStore.getState().setTagTerm("draft", "include");
    notesFiltersStore.getState().setTagTerm("client", "include");
    const field = mount();
    fireEvent.change(field, { target: { value: "abc" } });
    field.setSelectionRange(0, 2);
    fireEvent.keyDown(field, { key: "Backspace" });
    expect(notesFiltersStore.getState().tagTerms).toHaveLength(2);
    field.setSelectionRange(0, 0);
    fireEvent.keyDown(field, { key: "Backspace" });
    expect(notesFiltersStore.getState().tagTerms).toEqual([{ tag: "draft", term: "include" }]);
    const included = screen.getByRole("button", {
      name: "Tag draft: included. Exclude it instead.",
    });
    expect(included).toHaveAttribute("tabindex", "-1");
    fireEvent.click(included);
    fireEvent.click(
      screen.getByRole("button", { name: "Tag draft: excluded. Stop filtering by it." }),
    );
    expect(notesFiltersStore.getState().tagTerms).toEqual([]);
  });

  it("serializes sort, drives and private choices made through the real controls", async () => {
    mount();
    fireEvent.click(screen.getByRole("button", { name: "Sort notes" }));
    fireEvent.click(screen.getByRole("button", { name: "Name · Z to A" }));
    fireEvent.click(screen.getByRole("button", { name: "Search drives" }));
    fireEvent.click(screen.getByRole("checkbox", { name: "Personal" }));
    fireEvent.keyDown(screen.getByRole("checkbox", { name: "Personal" }), { key: "Escape" });
    fireEvent.click(screen.getByRole("button", { name: "Include private notes" }));
    await waitFor(() =>
      expect(noteQueryFor(notesFiltersStore.getState(), 0, 20)).toMatchObject({
        sort: "name desc",
        vaultIds: ["v2"],
        includePrivate: true,
      }),
    );
  });

  it("offers relevance only while a nonblank prompt stands", () => {
    const field = mount();
    fireEvent.click(screen.getByRole("button", { name: "Sort notes" }));
    expect(screen.queryByRole("button", { name: "Relevance" })).toBeNull();
    fireEvent.change(field, { target: { value: "budget" } });
    expect(screen.getByRole("button", { name: "Relevance" })).toBeVisible();
    fireEvent.change(field, { target: { value: " " } });
    expect(screen.queryByRole("button", { name: "Relevance" })).toBeNull();
  });

  it("keeps the eight keyboard controls in their agreed order", () => {
    mount();
    expect(
      screen
        .getAllByRole("button")
        .slice(0, 8)
        .map((button) => button.getAttribute("aria-label")),
    ).toEqual([
      "Changed by agent",
      "Pinned only",
      "Hide service files",
      "Sort notes",
      "Search drives",
      "Include private notes",
      "New note from search",
      "Save as space",
    ]);
  });

  it("asks which selected drive receives a new note and keeps the prompt", async () => {
    notesFiltersStore.getState().setVaultIds(["v1", "v2"]);
    notesFiltersStore.setState({
      scope: { kind: "space", id: "journal", name: "Journal", defaultKey: null },
    });
    notesFiltersStore.getState().setText("A thought to keep");
    notesFiltersStore.getState().setTagTerm("client", "include");
    notesFiltersStore.getState().setTagTerm("draft", "exclude");
    const field = mount();
    fireEvent.click(screen.getByRole("button", { name: "New note from search" }));
    expect(notesCreate).not.toHaveBeenCalled();
    fireEvent.click(screen.getByRole("button", { name: "Personal" }));
    await waitFor(() =>
      expect(notesCreate).toHaveBeenCalledWith(
        "v2",
        expect.objectContaining({ body: "A thought to keep", tags: ["client"], space: null }),
      ),
    );
    expect(field).toHaveValue("A thought to keep");
    fireEvent.click(screen.getByRole("button", { name: "New note from search" }));
    fireEvent.click(screen.getByRole("button", { name: "Work" }));
    await waitFor(() =>
      expect(notesCreate).toHaveBeenLastCalledWith(
        "v1",
        expect.objectContaining({ body: "A thought to keep", tags: ["client"], space: "journal" }),
      ),
    );
  });

  it("exposes preserved flag and origin terms as removable filters", () => {
    notesFiltersStore.getState().setFlags(["archived", "pinned"]);
    notesFiltersStore.getState().setOrigin("agent");
    mount();
    fireEvent.click(screen.getByRole("button", { name: "Clear is:archived filter" }));
    fireEvent.click(screen.getByRole("button", { name: "Clear origin:agent filter" }));
    expect(noteQueryFor(notesFiltersStore.getState(), 0, 20)).toMatchObject({
      flags: ["pinned"],
      origin: null,
    });
    expect(screen.getByRole("button", { name: "Changed by agent" })).toHaveAttribute(
      "aria-pressed",
      "false",
    );
  });

  it("renders the producer notice and gives failure precedence", () => {
    notesListStore.setState({
      notice: "Meaning is unavailable — model timed out. Searching words only.",
    });
    notesFiltersStore.getState().setText("budget");
    mount();
    expect(screen.getByRole("status")).toHaveTextContent("model timed out");
    act(() => notesListStore.getState().failSearch("Search failed."));
    expect(screen.getByRole("status")).toHaveTextContent("Search failed.");
  });

  // AD-268's promise survived the two-zone rewrite: the icon run has ONE order and
  // nothing drops out of it. The rewrite deleted the old inventory test, and the
  // phone pane's is unordered, so this is the only place the order is a contract.
  it("keeps the eight controls in their fixed order", () => {
    notesFiltersStore.getState().setText("budget");
    mount();
    const names = screen
      .getAllByRole("button")
      .map((button) => button.getAttribute("aria-label"))
      .filter((name): name is string =>
        [
          "Changed by agent",
          "Pinned only",
          "Hide service files",
          "Sort notes",
          "Search drives",
          "Include private notes",
          "New note from search",
          "Save as space",
        ].includes(name ?? ""),
      );
    expect(names).toEqual([
      "Changed by agent",
      "Pinned only",
      "Hide service files",
      "Sort notes",
      "Search drives",
      "Include private notes",
      "New note from search",
      "Save as space",
    ]);
  });
});
