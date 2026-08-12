import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";

// The bar reads the tag vocabulary when its chooser opens (Story 44.13); the
// rest of this suite must not pay a Tauri round trip for that.
vi.mock("@/lib/ipc/client", () => ({
  tagsVocabulary: vi.fn(),
}));

import { ADD_TAG_FILTER, NoteFilterBar } from "@/components/notes/note-filter-bar";
import { NotesEmptyState } from "@/components/notes/notes-empty-state";
import { tagComboboxCreate, tagComboboxNoMatch } from "@/components/notes/tag-combobox";
import { tagsVocabulary } from "@/lib/ipc/client";
import {
  emptyFilterReason,
  notesFiltersStore,
  resetNotesFiltersStoreForTest,
} from "@/lib/stores/notes-filters";

const mockVocabulary = vi.mocked(tagsVocabulary);

/**
 * The three-state tag chip (Story 43.3, FR-148, UX-DR54).
 *
 * These assertions are about the rendered control rather than about the store,
 * because the claim the story makes is a claim about what a user can SEE: a chip
 * whose state you have to hover to discover has, in practice, one state. So the
 * sign glyph and the accessible name are read off the DOM, and the empty
 * state's sentence is asserted through the component that renders it rather than
 * through the function that composes it.
 */
beforeEach(() => {
  resetNotesFiltersStoreForTest();
  mockVocabulary.mockReset();
  mockVocabulary.mockResolvedValue({
    entries: [
      { path: "client", count: 3 },
      { path: "client/acme", count: 2 },
      { path: "draft", count: 1 },
    ],
  });
});

const bar = () => <NoteFilterBar onSaveAsSpace={vi.fn()} />;

/** The chip's own button for one tag — the one that cycles it. */
function chipFor(tag: string): HTMLElement {
  const chip = screen
    .getAllByRole("button")
    .find((button) => (button.getAttribute("aria-label") ?? "").startsWith(`Tag ${tag}:`));
  if (chip === undefined) {
    throw new Error(`no chip for ${tag}`);
  }
  return chip;
}

describe("the tag chip's three states", () => {
  it("cycles include, exclude, off as the chip is pressed", () => {
    notesFiltersStore.getState().setTagTerm("draft", "include");
    const { rerender } = render(bar());

    fireEvent.click(chipFor("draft"));
    expect(notesFiltersStore.getState().tagTerms).toEqual([{ tag: "draft", term: "exclude" }]);
    rerender(bar());

    fireEvent.click(chipFor("draft"));
    // Off removes the chip from the bar: a control showing a state that does
    // nothing is a control people press by accident.
    expect(notesFiltersStore.getState().tagTerms).toEqual([]);
    rerender(bar());
    expect(screen.queryByText("draft")).toBeNull();
  });

  it("shows which state it is in without being hovered", () => {
    notesFiltersStore.getState().setTagTerm("draft", "include");
    const { rerender } = render(bar());

    const included = chipFor("draft");
    const includedChip = included.closest("[data-tag-term]");
    expect(included).toHaveAccessibleName("Tag draft: included. Exclude it instead.");
    // The sign is a rendered element, not a `::before` or a `title` attribute —
    // neither of which survives a screenshot, a print, or a user in a hurry.
    expect(included.querySelector("svg")).not.toBeNull();
    expect(includedChip?.getAttribute("data-tag-term")).toBe("include");
    const includedClass = includedChip?.className;

    notesFiltersStore.getState().setTagTerm("draft", "exclude");
    rerender(bar());

    const excluded = chipFor("draft");
    const excludedChip = excluded.closest("[data-tag-term]");
    expect(excluded).toHaveAccessibleName("Tag draft: excluded. Stop filtering by it.");
    expect(excludedChip?.getAttribute("data-tag-term")).toBe("exclude");
    // Two chips in different states must not render identically: the state is
    // carried by the styling and the glyph as well as by the name.
    expect(excludedChip?.className).not.toBe(includedClass);
  });

  it("clears straight to off from either state, without walking the cycle", () => {
    notesFiltersStore.getState().setTagTerm("draft", "exclude");
    render(bar());

    fireEvent.click(screen.getByLabelText("Clear tag draft filter"));
    expect(notesFiltersStore.getState().tagTerms).toEqual([]);
  });

  it("offers Save as space once a chip is set, so an exclusion can be kept", () => {
    const { rerender } = render(bar());
    expect(screen.queryByText("Save as space")).toBeNull();

    notesFiltersStore.getState().setTagTerm("draft", "exclude");
    rerender(bar());
    expect(screen.getByText("Save as space")).toBeInTheDocument();
  });
});

describe("the empty result", () => {
  it("names the term that emptied it, including the exclusion", () => {
    const state = notesFiltersStore.getState();
    state.setTagTerm("client/acme", "include");
    state.setTagTerm("draft", "exclude");

    render(
      <NotesEmptyState
        kind="no-matches"
        detail={emptyFilterReason(notesFiltersStore.getState())}
        onAction={vi.fn()}
      />,
    );

    expect(screen.getByText("No notes match these filters.")).toBeInTheDocument();
    expect(screen.getByText("Narrowed by client/acme and not draft.")).toBeInTheDocument();
  });

  it("says only the fixed sentence when nothing is narrowing", () => {
    render(<NotesEmptyState kind="empty-vault" detail={null} onAction={vi.fn()} />);

    expect(screen.getByText("This vault is empty. Write the first note.")).toBeInTheDocument();
    expect(screen.queryByText(/^Narrowed by/)).toBeNull();
  });
});

/**
 * The origin and pinned toggles (Story 49).
 *
 * These were one-way controls: the button existed only while the filter was
 * OFF and could only turn it on, and the way back was a chip that had replaced
 * it, somewhere else in the bar, named something else. So the assertions are
 * that ONE control, found by one name, is there in both states and reports
 * which one it is in — `pressed`, off a role+name query, because that is the
 * fact a screen reader is given and the class it is painted with is not.
 */
describe("the origin and pinned toggles", () => {
  for (const { name, read, set } of [
    {
      name: "Changed by agent",
      read: () => notesFiltersStore.getState().agentOnly,
      set: (on: boolean) => notesFiltersStore.getState().setAgentOnly(on),
    },
    {
      name: "Pinned only",
      read: () => notesFiltersStore.getState().pinnedOnly,
      set: (on: boolean) => notesFiltersStore.getState().setPinnedOnly(on),
    },
  ]) {
    it(`turns ${name} on and back off from the same control`, () => {
      render(bar());

      const off = screen.getByRole("button", { name, pressed: false });
      fireEvent.click(off);

      expect(read()).toBe(true);
      // The same name, still one control, now reporting the other state — the
      // press did not move the off-switch to a chip elsewhere in the bar.
      const on = screen.getByRole("button", { name, pressed: true });
      expect(screen.getAllByRole("button", { name })).toHaveLength(1);

      fireEvent.click(on);
      expect(read()).toBe(false);
      expect(screen.getByRole("button", { name, pressed: false })).toBeInTheDocument();
    });

    it(`shows ${name} already pressed when the filter is on before the bar mounts`, () => {
      set(true);
      render(bar());

      expect(screen.getByRole("button", { name, pressed: true })).toBeInTheDocument();
    });
  }

  it("leaves the three-state tag chip without a pressed state", () => {
    // Deliberate, and documented on `TagFilterChip`: three states are not a
    // toggle, and a control reporting `pressed=false` while it is actively
    // EXCLUDING notes would be worse than saying nothing. The state is in the
    // name instead. Asserted so the next pass at this bar does not "fix" it.
    notesFiltersStore.getState().setTagTerm("draft", "exclude");
    render(bar());

    const chip = chipFor("draft");
    expect(chip).not.toHaveAttribute("aria-pressed");
    expect(chip).toHaveAccessibleName("Tag draft: excluded. Stop filtering by it.");
  });
});

/**
 * The bar's own tag chooser (Story 44.13, FR-169, UX-DR61).
 *
 * Driven through the real bar and the real store rather than through the
 * control in isolation, because the claim the story makes is that a chip
 * ARRIVES — the control's own suite proves it can pick a tag, and this proves
 * the pick lands on the filter that runs the list.
 */
describe("adding a tag filter from the bar", () => {
  /** Open the chooser and wait for the vocabulary it reads on opening. */
  async function openChooser(): Promise<HTMLElement> {
    render(bar());
    fireEvent.click(screen.getByRole("button", { name: ADD_TAG_FILTER }));
    await screen.findByRole("option", { name: "client" });
    return screen.getByRole("combobox", { name: ADD_TAG_FILTER });
  }

  it("browses the vault's tags before a key is pressed", async () => {
    await openChooser();

    expect(screen.getAllByRole("option").map((row) => row.textContent)).toEqual([
      "client",
      "client/acme",
      "draft",
    ]);
  });

  it("narrows as you type and raises the chip the keyboard lands on", async () => {
    const field = await openChooser();

    fireEvent.change(field, { target: { value: "acme" } });
    expect(screen.getAllByRole("option").map((row) => row.textContent)).toEqual(["client/acme"]);

    fireEvent.keyDown(field, { key: "Enter" });

    expect(notesFiltersStore.getState().tagTerms).toEqual([
      { tag: "client/acme", term: "include" },
    ]);
    expect(chipFor("client/acme")).toBeInTheDocument();
  });

  it("refuses to invent a tag, because a filter can only narrow to what exists", async () => {
    const field = await openChooser();

    fireEvent.change(field, { target: { value: "nonesuch" } });

    expect(screen.getByText(tagComboboxNoMatch("nonesuch"))).toBeInTheDocument();
    expect(screen.queryByText(tagComboboxCreate("nonesuch"))).toBeNull();

    fireEvent.keyDown(field, { key: "Enter" });
    expect(notesFiltersStore.getState().tagTerms).toEqual([]);
  });

  it("leaves out the tags already on the bar", async () => {
    notesFiltersStore.getState().setTagTerm("draft", "exclude");
    await openChooser();

    expect(screen.getAllByRole("option").map((row) => row.textContent)).toEqual([
      "client",
      "client/acme",
    ]);
  });

  it("takes the caret on opening and gives it back on Escape", async () => {
    // Keyboard-alone operation is the AC, and the round trip is the whole of
    // it: a control you can reach but not leave strands the Tab order.
    const field = await openChooser();
    expect(document.activeElement).toBe(field);

    fireEvent.keyDown(field, { key: "Escape" });

    await waitFor(() =>
      expect(document.activeElement).toBe(screen.getByRole("button", { name: ADD_TAG_FILTER })),
    );
    expect(screen.queryByRole("combobox", { name: ADD_TAG_FILTER })).toBeNull();
  });

  it("does not read the vocabulary until the chooser is asked for", () => {
    render(bar());

    expect(mockVocabulary).not.toHaveBeenCalled();
  });
});
