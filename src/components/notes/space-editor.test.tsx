import { fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { NoteSpaceVm } from "@/lib/ipc/client";

vi.mock("@/lib/ipc/client", () => ({
  notesSpaceTerms: vi.fn(),
  notesSpaceSave: vi.fn(),
  notesTagTree: vi.fn(),
}));

import {
  SPACE_NO_NAME,
  SPACE_NO_TERMS,
  SPACE_SORT_NOTES,
  SPACE_SORT_RECORDED_NOTE,
  SPACE_TERMS_BROKEN,
  SPACE_TERMS_READONLY,
  SpaceEditor,
} from "@/components/notes/space-editor";
import { notesSpaceSave, notesSpaceTerms, notesTagTree } from "@/lib/ipc/client";

const mockTerms = vi.mocked(notesSpaceTerms);
const mockSave = vi.mocked(notesSpaceSave);
const mockTagTree = vi.mocked(notesTagTree);

/**
 * A hand-written space using every construct the chip vocabulary cannot hold.
 * The byte-identity guarantee is only worth anything for a query like this one.
 *
 * Both lossy fixtures are the exact strings
 * `keeper-core::notes::query::tests::the_editors_worked_*_lossy_example_*` feed
 * the real decomposer, and the mocked replies below are what that decomposer
 * actually returns for them: this file mocks the IPC boundary, so a mock that
 * invented a shape Rust never produces would be a green test over a broken
 * seam. A grouped query is refused **whole** — a group is not a term, so there
 * is no honest way to name the offending part of it — and a flat one names each
 * offending term.
 */
const LOSSY_QUERY =
  "tag:client/acme (tag:urgent | tag:blocked) path:journal/** field:priority=high date:modified>=-14d -(tag:done tag:archive) tag:client/*";

/** The flat companion: no grouping, so each refused term is named on its own. */
const FLAT_LOSSY_QUERY = "tag:client/acme path:journal/** date:modified>=-14d";

function space(p: Partial<NoteSpaceVm> = {}): NoteSpaceVm {
  return {
    id: p.id ?? "s1",
    name: p.name ?? "Active work",
    query: p.query ?? "tag:client/acme -tag:draft",
    sort: p.sort ?? "modified desc",
    sortEffective: p.sortEffective ?? "modified desc",
    limit: p.limit ?? 500,
    icon: p.icon ?? null,
    defaultKey: p.defaultKey ?? null,
    warnings: p.warnings ?? [],
    order: p.order ?? 0,
    error: p.error ?? null,
  };
}

function open(vm: NoteSpaceVm = space()) {
  const onClose = vi.fn();
  const onSaved = vi.fn();
  render(<SpaceEditor vaultId="vault-1" space={vm} onClose={onClose} onSaved={onSaved} />);
  return { onClose, onSaved };
}

/** The one argument every assertion below reads: the request that was written. */
function savedRequest() {
  const call = mockSave.mock.calls[0];
  if (call === undefined) {
    throw new Error("nothing was saved");
  }
  return call[1];
}

beforeEach(() => {
  mockTerms.mockReset();
  mockSave.mockReset();
  mockTagTree.mockReset();
  mockSave.mockResolvedValue({
    vaultId: "vault-1",
    id: "s1",
    path: "spaces/active.md",
    title: "Active work",
  });
  mockTagTree.mockResolvedValue({
    nodes: [
      { name: "client", path: "client", count: 3, children: [] },
      { name: "draft", path: "draft", count: 2, children: [] },
      { name: "urgent", path: "urgent", count: 1, children: [] },
    ],
  });
  mockTerms.mockResolvedValue({
    kind: "chips",
    tags: [
      { tag: "client/acme", term: "include" },
      { tag: "draft", term: "exclude" },
    ],
    flags: [],
    origin: null,
    text: null,
  });
});

afterEach(() => {
  vi.clearAllMocks();
});

describe("editing a space changes what it selects", () => {
  it("writes the DSL the chips now say, not the DSL it was opened with", async () => {
    open();

    // Cycle the include chip to exclude, and drop the exclusion entirely.
    fireEvent.click(
      await screen.findByRole("button", { name: "Tag client/acme: included. Exclude it instead." }),
    );
    fireEvent.click(screen.getByRole("button", { name: "Clear tag draft filter" }));
    fireEvent.click(screen.getByRole("button", { name: "Save" }));

    await waitFor(() => expect(mockSave).toHaveBeenCalledTimes(1));
    expect(savedRequest().query).toBe("-tag:client/acme");
  });

  it("adds a tag by typing it, from a list that is still there to be browsed (Story 44.13)", async () => {
    open();

    await screen.findByRole("button", { name: /Tag client\/acme/ });
    const field = screen.getByLabelText("Add a tag");
    // The list is populated before a key is pressed — that is the browse half
    // the `<select>` this replaced was the only half of. `draft` is absent
    // because the space already carries it. Scoped to the combobox's own
    // listbox, because Story 44.4 put two real `<select>`s on this form and
    // their `<option>`s answer to the same role.
    expect(
      within(screen.getByRole("listbox"))
        .getAllByRole("option")
        .map((row) => row.textContent),
    ).toEqual(["client", "urgent"]);

    fireEvent.change(field, { target: { value: "urg" } });
    fireEvent.keyDown(field, { key: "Enter" });
    fireEvent.click(screen.getByRole("button", { name: "Save" }));

    await waitFor(() => expect(mockSave).toHaveBeenCalledTimes(1));
    expect(savedRequest().query).toBe("tag:client/acme -tag:draft tag:urgent");
  });

  it("takes a tag the vault does not have yet, because a space is authored (Story 44.13)", async () => {
    // The opposite setting from the filter bar, and for the stated reason: a
    // space is a document, and naming the tag the work is about to carry
    // before the first note carries it is ordinary. The typed text goes into
    // the query verbatim; `notes::query` normalises every `tag:` on the way
    // back in, so nothing here decides what a tag is.
    open();

    await screen.findByRole("button", { name: /Tag client\/acme/ });
    const field = screen.getByLabelText("Add a tag");
    fireEvent.change(field, { target: { value: "client/newco" } });

    expect(screen.getByText('Create tag "client/newco"')).toBeInTheDocument();
    fireEvent.keyDown(field, { key: "Enter" });
    fireEvent.click(screen.getByRole("button", { name: "Save" }));

    await waitFor(() => expect(mockSave).toHaveBeenCalledTimes(1));
    expect(savedRequest().query).toBe("tag:client/acme -tag:draft tag:client/newco");
  });

  it("does not let Enter in the chooser save the space that is still being described", async () => {
    // The dialog has a default button. A chooser that let Enter through would
    // write the space on the keystroke meant to add a term to it.
    open();

    await screen.findByRole("button", { name: /Tag client\/acme/ });
    fireEvent.keyDown(screen.getByLabelText("Add a tag"), { key: "Enter" });

    expect(mockSave).not.toHaveBeenCalled();
  });

  /**
   * The chip's position is the target the cursor is already on, so cycling a
   * chip must not move it to the end of the row (Story 43.3's `withTagTerm`).
   */
  it("keeps a cycled chip where it was, so the query keeps its order", async () => {
    open();

    fireEvent.click(
      await screen.findByRole("button", { name: "Tag client/acme: included. Exclude it instead." }),
    );
    fireEvent.click(screen.getByRole("button", { name: "Save" }));

    await waitFor(() => expect(mockSave).toHaveBeenCalledTimes(1));
    expect(savedRequest().query).toBe("-tag:client/acme -tag:draft");
  });

  it("carries the lens, origin and search terms it did not edit back out again", async () => {
    mockTerms.mockResolvedValue({
      kind: "chips",
      tags: [{ tag: "client/acme", term: "include" }],
      flags: ["pinned"],
      origin: "agent",
      text: "quarterly review",
    });
    open(space({ query: 'tag:client/acme is:pinned origin:agent text:"quarterly review"' }));

    await screen.findByRole("button", { name: /Tag client\/acme/ });
    fireEvent.click(screen.getByRole("button", { name: "Save" }));

    await waitFor(() => expect(mockSave).toHaveBeenCalledTimes(1));
    expect(savedRequest().query).toBe(
      'tag:client/acme is:pinned origin:agent text:"quarterly review"',
    );
  });

  it("removes a lens term when it is taken off", async () => {
    mockTerms.mockResolvedValue({
      kind: "chips",
      tags: [{ tag: "client/acme", term: "include" }],
      flags: ["pinned"],
      origin: null,
      text: null,
    });
    open(space({ query: "tag:client/acme is:pinned" }));

    fireEvent.click(await screen.findByRole("button", { name: "Remove is:pinned" }));
    fireEvent.click(screen.getByRole("button", { name: "Save" }));

    await waitFor(() => expect(mockSave).toHaveBeenCalledTimes(1));
    expect(savedRequest().query).toBe("tag:client/acme");
  });
});

describe("the icon", () => {
  it("persists the icon that was chosen", async () => {
    open();

    await screen.findByRole("button", { name: /Tag client\/acme/ });
    fireEvent.click(screen.getByRole("button", { name: "star" }));
    fireEvent.click(screen.getByRole("button", { name: "Save" }));

    await waitFor(() => expect(mockSave).toHaveBeenCalledTimes(1));
    expect(savedRequest().icon).toBe("star");
  });

  it("shows the stored icon as the chosen one when the editor opens", async () => {
    open(space({ icon: "flag" }));

    expect(await screen.findByRole("button", { name: "flag" })).toHaveAttribute(
      "aria-pressed",
      "true",
    );
    expect(screen.getByRole("button", { name: "No icon" })).toHaveAttribute(
      "aria-pressed",
      "false",
    );
  });

  /**
   * An icon name the set no longer holds is not a selection and is not rewritten
   * either: nothing in the picker is pressed, and a save that did not touch the
   * picker sends the stored name straight back.
   */
  it("shows no selection for an icon outside the set and leaves the stored name alone", async () => {
    open(space({ icon: "sparkles" }));

    await screen.findByRole("button", { name: /Tag client\/acme/ });
    for (const name of ["No icon", "star", "flag", "inbox"]) {
      expect(screen.getByRole("button", { name })).toHaveAttribute("aria-pressed", "false");
    }
    fireEvent.click(screen.getByRole("button", { name: "Save" }));

    await waitFor(() => expect(mockSave).toHaveBeenCalledTimes(1));
    expect(savedRequest().icon).toBe("sparkles");
  });

  it("clears the icon when No icon is chosen", async () => {
    open(space({ icon: "flag" }));

    fireEvent.click(await screen.findByRole("button", { name: "No icon" }));
    fireEvent.click(screen.getByRole("button", { name: "Save" }));

    await waitFor(() => expect(mockSave).toHaveBeenCalledTimes(1));
    expect(savedRequest().icon).toBeNull();
  });

  /**
   * The set has to cover the four seeded defaults (Story 44.3) or the rail that
   * replaced the fixed rows cannot draw the glyphs those rows drew. Named one by
   * one rather than counted, because a count passes while the wrong twenty-four
   * icons are in the map.
   */
  it("offers every glyph a seeded default asks for", async () => {
    open();

    for (const name of ["inbox", "calendar-days", "pin", "video"]) {
      expect(await screen.findByRole("button", { name })).toBeInTheDocument();
    }
  });
});

describe("the sort", () => {
  it("shows the ordering the list is actually running, and saves it back", async () => {
    open(space({ sort: "created asc", sortEffective: "created asc" }));

    expect(await screen.findByLabelText("Sort by")).toHaveValue("created");
    expect(screen.getByLabelText("Direction")).toHaveValue("asc");

    fireEvent.change(screen.getByLabelText("Sort by"), { target: { value: "recorded" } });
    fireEvent.change(screen.getByLabelText("Direction"), { target: { value: "desc" } });
    fireEvent.click(screen.getByRole("button", { name: "Save" }));

    await waitFor(() => expect(mockSave).toHaveBeenCalledTimes(1));
    expect(savedRequest().sort).toBe("recorded desc");
  });

  it("offers all five facts a space can order by", async () => {
    open();

    expect(
      within(await screen.findByLabelText("Sort by"))
        .getAllByRole("option")
        .map((option) => option.getAttribute("value")),
    ).toEqual(["order", "name", "created", "modified", "recorded"]);
  });

  /**
   * "Ascending" is a word about the machine. What a reader wants to know is
   * whether the newest is at the top — and for `name` that question has
   * different words, so the labels follow the key.
   */
  it("words the direction in the terms of whatever is being sorted", async () => {
    open();

    const direction = await screen.findByLabelText("Direction");
    expect(
      within(direction)
        .getAllByRole("option")
        .map((o) => o.textContent),
    ).toEqual(["Oldest first", "Newest first"]);

    fireEvent.change(screen.getByLabelText("Sort by"), { target: { value: "name" } });
    expect(
      within(direction)
        .getAllByRole("option")
        .map((o) => o.textContent),
    ).toEqual(["A to Z", "Z to A"]);
  });

  /**
   * `recorded` and `order` are the two keys whose behaviour their own name does
   * not give away, and both rules are decisions rather than accidents. The other
   * three carry no line, because a sentence under every control is a sentence
   * nobody reads.
   */
  it("explains the two sorts whose names do not give their behaviour away", async () => {
    open();

    await screen.findByLabelText("Sort by");
    expect(screen.queryByText(SPACE_SORT_RECORDED_NOTE)).not.toBeInTheDocument();
    expect(document.querySelector("[data-slot='sort-note']")).toBeNull();

    fireEvent.change(screen.getByLabelText("Sort by"), { target: { value: "recorded" } });
    expect(screen.getByText(SPACE_SORT_RECORDED_NOTE)).toBeInTheDocument();

    // `order` promises a manual ordering while most notes have never been given
    // one. A reader not told that concludes the sort is broken rather than that
    // the vault is unordered, which is the misreading AD-81 exists to prevent.
    //
    // Changing the key deliberately does NOT reset the direction. Rust owns the
    // table of which key means which way, and reproducing it here to reset a
    // dropdown would be a second copy of it in the language that cannot run its
    // tests. Nothing is hidden by keeping it: the direction's own labels follow
    // the key, so this still-`desc` control now reads "Highest first".
    fireEvent.change(screen.getByLabelText("Sort by"), { target: { value: "order" } });
    expect(screen.getByLabelText("Direction")).toHaveValue("desc");
    expect(screen.getByText(SPACE_SORT_NOTES["order desc"] ?? "")).toBeInTheDocument();

    // The alphabet does not reverse with the direction; only the position does,
    // which is why neither line says "reverse alphabetically".
    fireEvent.change(screen.getByLabelText("Direction"), { target: { value: "asc" } });
    expect(screen.getByText(SPACE_SORT_NOTES["order asc"] ?? "")).toBeInTheDocument();

    fireEvent.change(screen.getByLabelText("Sort by"), { target: { value: "modified" } });
    expect(document.querySelector("[data-slot='sort-note']")).toBeNull();
  });

  /**
   * The repair path for Story 44.4's visible fallback.
   *
   * The form is seeded from `sortEffective` — what Rust decided — never from the
   * stored text, so a value keeper could not read shows as the ordering the list
   * is running. Working that out in TypeScript would be a second copy of the
   * fallback rule in the language that cannot run its tests.
   */
  it("shows the fallback for a sort keeper could not read, and says why", async () => {
    const said =
      'keeper doesn\'t know the sort "bananas", so this space is sorted by modified, newest first.';
    open(space({ sort: "bananas", sortEffective: "modified desc", warnings: [said] }));

    expect(await screen.findByLabelText("Sort by")).toHaveValue("modified");
    expect(screen.getByLabelText("Direction")).toHaveValue("desc");
    expect(screen.getByText(said)).toBeInTheDocument();
  });

  /**
   * Saving writes the canonical spelling over `bananas`. That IS a rewrite of a
   * value keeper did not understand — the one in this surface — and it is
   * allowed here for a reason the icon does not have: the form showed the
   * fallback and the sentence explaining it, so pressing Save is the user
   * agreeing to the repair rather than keeper editing a file behind their back.
   */
  it("repairs an unreadable sort on save, having shown what it was going to write", async () => {
    open(space({ sort: "bananas", sortEffective: "modified desc", warnings: ["…"] }));

    await screen.findByLabelText("Sort by");
    fireEvent.click(screen.getByRole("button", { name: "Save" }));

    await waitFor(() => expect(mockSave).toHaveBeenCalledTimes(1));
    expect(savedRequest().sort).toBe("modified desc");
  });
});

describe("the rail position", () => {
  it("shows an empty box for a space nobody positioned, and saves it as unpositioned", async () => {
    open();

    expect(await screen.findByLabelText("Rail position")).toHaveValue(null);
    fireEvent.click(screen.getByRole("button", { name: "Save" }));

    await waitFor(() => expect(mockSave).toHaveBeenCalledTimes(1));
    expect(savedRequest().order).toBe(0);
  });

  it("shows and saves a position, including a negative and a fraction", async () => {
    open(space({ order: -1 }));

    expect(await screen.findByLabelText("Rail position")).toHaveValue(-1);

    fireEvent.change(screen.getByLabelText("Rail position"), { target: { value: "1.5" } });
    fireEvent.click(screen.getByRole("button", { name: "Save" }));

    await waitFor(() => expect(mockSave).toHaveBeenCalledTimes(1));
    expect(savedRequest().order).toBe(1.5);
  });

  /**
   * Clearing the box to retype is a state that exists for as long as it takes to
   * type a digit. It must mean "unpositioned", which is the same 0 an absent
   * `keeper.order` means — never a refusal to save the name the user also
   * changed.
   */
  it("treats an emptied box as unpositioned rather than as a reason to refuse", async () => {
    open(space({ order: 4 }));

    fireEvent.change(await screen.findByLabelText("Rail position"), { target: { value: "" } });
    fireEvent.click(screen.getByRole("button", { name: "Save" }));

    await waitFor(() => expect(mockSave).toHaveBeenCalledTimes(1));
    expect(savedRequest().order).toBe(0);
  });
});

describe("renaming", () => {
  it("sends the new name, trimmed", async () => {
    open();

    fireEvent.change(await screen.findByLabelText("Name"), {
      target: { value: "  Archive triage  " },
    });
    fireEvent.click(screen.getByRole("button", { name: "Save" }));

    await waitFor(() => expect(mockSave).toHaveBeenCalledTimes(1));
    expect(savedRequest().name).toBe("Archive triage");
    expect(savedRequest().id).toBe("s1");
  });

  it("refuses to save a space with no name left", async () => {
    open();

    fireEvent.change(await screen.findByLabelText("Name"), { target: { value: "   " } });

    expect(screen.getByText(SPACE_NO_NAME)).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Save" })).toBeDisabled();
    fireEvent.click(screen.getByRole("button", { name: "Save" }));
    expect(mockSave).not.toHaveBeenCalled();
  });
});

describe("cancelling", () => {
  it("writes nothing at all, whatever was typed", async () => {
    const { onClose, onSaved } = open();

    fireEvent.change(await screen.findByLabelText("Name"), { target: { value: "Renamed" } });
    fireEvent.click(screen.getByRole("button", { name: "star" }));
    fireEvent.click(screen.getByRole("button", { name: "Clear tag draft filter" }));
    fireEvent.click(screen.getByRole("button", { name: "Cancel" }));

    expect(mockSave).not.toHaveBeenCalled();
    expect(onClose).toHaveBeenCalledTimes(1);
    expect(onSaved).not.toHaveBeenCalled();
  });
});

describe("a space with no terms left", () => {
  /**
   * The AC's sharpest edge: an empty query is not "everything". A saved view
   * that silently widens to the whole vault is how a bulk action becomes a
   * data-loss story, so the form refuses before the round trip.
   */
  it("refuses to save rather than becoming everything", async () => {
    open();

    fireEvent.click(await screen.findByRole("button", { name: "Clear tag client/acme filter" }));
    fireEvent.click(screen.getByRole("button", { name: "Clear tag draft filter" }));

    expect(screen.getByText(SPACE_NO_TERMS)).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Save" })).toBeDisabled();
    fireEvent.click(screen.getByRole("button", { name: "Save" }));
    expect(mockSave).not.toHaveBeenCalled();
  });

  it("counts a lens, an origin and a search as terms too", async () => {
    mockTerms.mockResolvedValue({
      kind: "chips",
      tags: [],
      flags: ["pinned"],
      origin: null,
      text: null,
    });
    open(space({ query: "is:pinned" }));

    await screen.findByRole("button", { name: "Remove is:pinned" });
    expect(screen.queryByText(SPACE_NO_TERMS)).not.toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Save" })).toBeEnabled();

    fireEvent.click(screen.getByRole("button", { name: "Remove is:pinned" }));
    expect(screen.getByText(SPACE_NO_TERMS)).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Save" })).toBeDisabled();
  });
});

describe("a space whose query the chips cannot hold", () => {
  beforeEach(() => {
    // What the real decomposer returns for LOSSY_QUERY: refused whole.
    mockTerms.mockResolvedValue({ kind: "unrepresentable", terms: [LOSSY_QUERY] });
  });

  it("shows the query it will not touch, and offers no chips at all", async () => {
    open(space({ query: LOSSY_QUERY }));

    expect(await screen.findByText(SPACE_TERMS_READONLY)).toBeInTheDocument();
    expect(screen.getByText(LOSSY_QUERY)).toBeInTheDocument();
    expect(screen.queryByLabelText("Add a tag")).not.toBeInTheDocument();
    expect(screen.queryByRole("button", { name: /^Tag / })).not.toBeInTheDocument();
  });

  it("names each refused term when the query is flat enough to name them", async () => {
    mockTerms.mockResolvedValue({
      kind: "unrepresentable",
      terms: ["path:journal/**", "date:modified>=-14d"],
    });
    open(space({ query: FLAT_LOSSY_QUERY }));

    expect(await screen.findByText(SPACE_TERMS_READONLY)).toBeInTheDocument();
    expect(screen.getByText("path:journal/**")).toBeInTheDocument();
    expect(screen.getByText("date:modified>=-14d")).toBeInTheDocument();
    // The one term the chips COULD have held is still not offered as a chip:
    // a partial chip set is the silent term-dropping this arm exists to refuse.
    expect(screen.queryByRole("button", { name: /^Tag / })).not.toBeInTheDocument();
  });

  it("saves a flat lossy query back byte for byte too", async () => {
    mockTerms.mockResolvedValue({
      kind: "unrepresentable",
      terms: ["path:journal/**", "date:modified>=-14d"],
    });
    open(space({ query: FLAT_LOSSY_QUERY }));

    await screen.findByText(SPACE_TERMS_READONLY);
    fireEvent.change(screen.getByLabelText("Name"), { target: { value: "Renamed" } });
    fireEvent.click(screen.getByRole("button", { name: "Save" }));

    await waitFor(() => expect(mockSave).toHaveBeenCalledTimes(1));
    expect(savedRequest().query).toBe(FLAT_LOSSY_QUERY);
  });

  /**
   * The safety property the whole read-only arm exists for. Rename it, save it,
   * and the query that reaches Rust is the query that came off disk — byte for
   * byte, every construct intact. Re-emitting from chips here would silently
   * delete `date:modified>=-14d`, which the story calls the worst available
   * outcome.
   */
  it("saves the stored query back byte for byte when only the name changed", async () => {
    open(space({ query: LOSSY_QUERY }));

    await screen.findByText(SPACE_TERMS_READONLY);
    fireEvent.change(screen.getByLabelText("Name"), { target: { value: "Archive triage" } });
    fireEvent.click(screen.getByRole("button", { name: "Save" }));

    await waitFor(() => expect(mockSave).toHaveBeenCalledTimes(1));
    expect(savedRequest().query).toBe(LOSSY_QUERY);
    expect(savedRequest().name).toBe("Archive triage");
  });

  it("still lets the icon change, because that is not a term", async () => {
    open(space({ query: LOSSY_QUERY }));

    await screen.findByText(SPACE_TERMS_READONLY);
    fireEvent.click(screen.getByRole("button", { name: "inbox" }));
    fireEvent.click(screen.getByRole("button", { name: "Save" }));

    await waitFor(() => expect(mockSave).toHaveBeenCalledTimes(1));
    expect(savedRequest().icon).toBe("inbox");
    expect(savedRequest().query).toBe(LOSSY_QUERY);
  });

  it("carries the limit it never showed, and the sort it now does (Story 44.4)", async () => {
    // `limit` is still a value the form does not render, so it round-trips
    // untouched. `sort` used to be one of those and is not any more: the form
    // shows it, so what it saves is what the controls said — spelled the one
    // canonical way, which is what makes the fallback below a repair.
    open(
      space({ query: LOSSY_QUERY, sort: "created asc", sortEffective: "created asc", limit: 42 }),
    );

    await screen.findByText(SPACE_TERMS_READONLY);
    fireEvent.click(screen.getByRole("button", { name: "Save" }));

    await waitFor(() => expect(mockSave).toHaveBeenCalledTimes(1));
    expect(savedRequest().sort).toBe("created asc");
    expect(savedRequest().limit).toBe(42);
  });
});

describe("a space whose query does not parse", () => {
  /**
   * A broken space's row already says it is broken. Its editor must say the same
   * thing rather than showing an empty chip set, which would be one Save away
   * from replacing a typo with a space that selects the whole vault.
   */
  it("says so and keeps the unreadable query rather than offering an empty chip set", async () => {
    mockTerms.mockRejectedValue({ code: "invalidInput", message: "unknown search key `nope`" });
    open(space({ query: "nope:x", error: "unknown search key `nope`" }));

    expect(await screen.findByText(SPACE_TERMS_BROKEN)).toBeInTheDocument();
    expect(screen.queryByText(SPACE_NO_TERMS)).not.toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Save" })).toBeEnabled();

    fireEvent.change(screen.getByLabelText("Name"), { target: { value: "Broken but named" } });
    fireEvent.click(screen.getByRole("button", { name: "Save" }));

    await waitFor(() => expect(mockSave).toHaveBeenCalledTimes(1));
    expect(savedRequest().query).toBe("nope:x");
  });
});
