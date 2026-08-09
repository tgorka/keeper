import { fireEvent, render, screen } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { NoteFilterBar } from "@/components/notes/note-filter-bar";
import { NotesEmptyState } from "@/components/notes/notes-empty-state";
import {
  emptyFilterReason,
  notesFiltersStore,
  resetNotesFiltersStoreForTest,
} from "@/lib/stores/notes-filters";

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
