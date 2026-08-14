import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { SessionSpaceVm } from "@/lib/ipc/client";

// The editor reaches for exactly two commands: the pure decomposer it shares
// with notes, and the sessions save. Stubbing the boundary rather than the
// component keeps the real form — its refusals, its chips, its byte-identity
// rule — under test.
vi.mock("@/lib/ipc/client", () => ({
  notesSpaceTerms: vi.fn(),
  sessionsSpaceSave: vi.fn(),
}));

import {
  SPACE_NO_NAME,
  SPACE_NO_TERMS,
  SPACE_TERMS_READONLY,
} from "@/components/notes/space-editor";
import {
  SESSION_SPACE_EDIT_TITLE,
  SESSION_SPACE_NEW_TITLE,
  SESSION_SPACE_SAVE_FAILED,
  SESSION_SPACE_SORT_NOTES,
  SESSION_SPACE_TERMS_BROKEN,
  SessionSpaceEditor,
} from "@/components/sessions/session-space-editor";
import { notesSpaceTerms, sessionsSpaceSave } from "@/lib/ipc/client";

const mockTerms = vi.mocked(notesSpaceTerms);
const mockSave = vi.mocked(sessionsSpaceSave);

function space(p: Partial<SessionSpaceVm> = {}): SessionSpaceVm {
  return {
    id: p.id ?? "_spaces/tasks.md",
    name: p.name ?? "Tasks",
    query: p.query ?? "tag:task",
    sort: p.sort ?? "order asc",
    sortEffective: p.sortEffective ?? "order asc",
    icon: p.icon ?? null,
    defaultKey: p.defaultKey ?? null,
    order: p.order ?? 2,
    warnings: p.warnings ?? [],
    error: p.error ?? null,
  };
}

function open(vm: SessionSpaceVm | null, vocabulary: readonly string[] = ["task", "log"]) {
  const onClose = vi.fn();
  const onSaved = vi.fn();
  render(
    <SessionSpaceEditor
      rootId="p1"
      space={vm}
      vocabulary={vocabulary}
      onClose={onClose}
      onSaved={onSaved}
    />,
  );
  return { onClose, onSaved };
}

/** What reached Rust — the second argument, which is the whole request. */
function savedRequest() {
  return mockSave.mock.calls[0][1];
}

beforeEach(() => {
  mockTerms.mockReset();
  mockTerms.mockResolvedValue({
    kind: "chips",
    tags: [{ tag: "task", term: "include" }],
    flags: [],
    origin: null,
    text: null,
    fields: [],
  });
  mockSave.mockReset();
  mockSave.mockResolvedValue("_spaces/tasks.md");
});

afterEach(() => {
  vi.clearAllMocks();
});

describe("SessionSpaceEditor identity", () => {
  it("saves an existing space under its own path, so a rename rewrites one file", async () => {
    const { onSaved } = open(space());

    await screen.findByRole("button", { name: /Tag task/ });
    fireEvent.change(screen.getByLabelText("Name"), { target: { value: "Work items" } });
    fireEvent.click(screen.getByRole("button", { name: "Save" }));

    await waitFor(() => expect(mockSave).toHaveBeenCalledTimes(1));
    expect(savedRequest()).toMatchObject({ id: "_spaces/tasks.md", name: "Work items" });
    expect(onSaved).toHaveBeenCalled();
  });

  /**
   * `null`, never a name keeper invented. The filename a new space lands in is
   * Rust's to derive (AD-65) — it has to slugify, avoid `_`-prefixed collisions
   * and stay stable across a rename, and a second derivation over here would be
   * a second answer to the same question.
   */
  it("asks for a new space with a null id rather than composing a filename", async () => {
    open(null);

    fireEvent.change(screen.getByLabelText("Name"), { target: { value: "Blocked" } });
    fireEvent.keyDown(screen.getByLabelText("Add a tag"), { key: "Enter" });
    fireEvent.change(screen.getByLabelText("Add a tag"), { target: { value: "blocked" } });
    fireEvent.keyDown(screen.getByLabelText("Add a tag"), { key: "Enter" });
    fireEvent.click(screen.getByRole("button", { name: "Save" }));

    await waitFor(() => expect(mockSave).toHaveBeenCalledTimes(1));
    expect(savedRequest().id).toBeNull();
  });

  it("titles itself for what it is doing", async () => {
    open(null);
    expect(screen.getByText(SESSION_SPACE_NEW_TITLE)).toBeInTheDocument();
  });

  it("titles an edit as an edit", async () => {
    open(space());
    expect(await screen.findByText(SESSION_SPACE_EDIT_TITLE)).toBeInTheDocument();
  });
});

describe("SessionSpaceEditor terms", () => {
  /**
   * The guarantee the whole surface is arranged around (FR-121): a query with a
   * term the chips cannot hold is shown read-only and saved back byte for byte.
   * Re-emitting it from chips would silently drop `path:` and `date:` terms
   * from a file somebody hand-wrote.
   */
  it("saves an unrepresentable query byte for byte rather than re-emitting it", async () => {
    const stored = "tag:task path:journal/** date:modified>=-14d";
    mockTerms.mockResolvedValue({
      kind: "unrepresentable",
      terms: ["path:journal/**", "date:modified>=-14d"],
    });
    open(space({ query: stored }));

    expect(await screen.findByText(SPACE_TERMS_READONLY)).toBeInTheDocument();
    fireEvent.change(screen.getByLabelText("Name"), { target: { value: "Renamed" } });
    fireEvent.click(screen.getByRole("button", { name: "Save" }));

    await waitFor(() => expect(mockSave).toHaveBeenCalledTimes(1));
    expect(savedRequest().query).toBe(stored);
    expect(savedRequest().name).toBe("Renamed");
  });

  /**
   * A query that will not parse at all is a state of the space, not a failed
   * command. The form says so and keeps the other four fields editable —
   * refusing the whole dialog would send someone to hand-edit frontmatter to
   * change an icon.
   */
  it("says a broken query is broken, and still lets the rest be edited", async () => {
    mockTerms.mockRejectedValue(new Error("unexpected end of query"));
    open(space({ query: "tag:task AND", error: "unexpected end of query" }));

    expect(await screen.findByText(SESSION_SPACE_TERMS_BROKEN)).toBeInTheDocument();
    fireEvent.change(screen.getByLabelText("Name"), { target: { value: "Still mine" } });
    fireEvent.click(screen.getByRole("button", { name: "Save" }));

    await waitFor(() => expect(mockSave).toHaveBeenCalledTimes(1));
    expect(savedRequest()).toMatchObject({ name: "Still mine", query: "tag:task AND" });
  });

  it("cycles a tag chip from include to exclude and writes the negated term", async () => {
    open(space());

    fireEvent.click(await screen.findByRole("button", { name: /Tag task: included/ }));
    fireEvent.click(screen.getByRole("button", { name: "Save" }));

    await waitFor(() => expect(mockSave).toHaveBeenCalledTimes(1));
    expect(savedRequest().query).toBe("-tag:task");
  });

  /**
   * `field:status=todo` is what the task board's columns are made of, and until
   * the chip union grew a field arm it decomposed as unrepresentable — which
   * would have frozen every column query in this zone.
   */
  it("holds a field term as a removable chip", async () => {
    mockTerms.mockResolvedValue({
      kind: "chips",
      tags: [{ tag: "task", term: "include" }],
      flags: [],
      origin: null,
      text: null,
      fields: [{ key: "status", op: "=", value: "todo" }],
    });
    open(space({ query: "tag:task field:status=todo" }));

    fireEvent.click(await screen.findByRole("button", { name: "Remove status = todo" }));
    fireEvent.click(screen.getByRole("button", { name: "Save" }));

    await waitFor(() => expect(mockSave).toHaveBeenCalledTimes(1));
    expect(savedRequest().query).toBe("tag:task");
  });

  /**
   * `field:status!=done field:status!=deferred` is a legal pair, so removal is
   * BY POSITION. Removing "the chip that reads status" would take both and
   * silently widen the space.
   */
  it("removes one of two field chips on the same key, not both", async () => {
    mockTerms.mockResolvedValue({
      kind: "chips",
      tags: [{ tag: "task", term: "include" }],
      flags: [],
      origin: null,
      text: null,
      fields: [
        { key: "status", op: "!=", value: "done" },
        { key: "status", op: "!=", value: "deferred" },
      ],
    });
    open(space({ query: "tag:task field:status!=done field:status!=deferred" }));

    fireEvent.click(await screen.findByRole("button", { name: "Remove status != done" }));
    fireEvent.click(screen.getByRole("button", { name: "Save" }));

    await waitFor(() => expect(mockSave).toHaveBeenCalledTimes(1));
    expect(savedRequest().query).toBe("tag:task field:status!=deferred");
  });

  /**
   * An empty query is an error in `sessions::spaces::select`, not a
   * match-everything: a saved view that silently widened to the whole session is
   * how a bulk action becomes a data-loss story. The form refuses first.
   */
  it("refuses to save a space with no terms at all", async () => {
    open(space());

    fireEvent.click(await screen.findByRole("button", { name: /Tag task: included/ }));
    fireEvent.click(screen.getByRole("button", { name: /Tag task: excluded/ }));

    expect(await screen.findByText(SPACE_NO_TERMS)).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Save" })).toBeDisabled();
    expect(mockSave).not.toHaveBeenCalled();
  });

  it("refuses to save a space with no name", async () => {
    open(space());

    await screen.findByRole("button", { name: /Tag task/ });
    fireEvent.change(screen.getByLabelText("Name"), { target: { value: "   " } });

    expect(await screen.findByText(SPACE_NO_NAME)).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Save" })).toBeDisabled();
  });
});

describe("SessionSpaceEditor sort and position", () => {
  /**
   * Seeded from `sortEffective`, never from `sort`. The form shows what the list
   * is actually doing, which is what makes Save a repair: the person saw the
   * fallback, agreed with it, and wrote it down.
   */
  it("shows the sort that is running, and saves it canonically", async () => {
    open(
      space({
        sort: "bananas",
        sortEffective: "modified desc",
        warnings: ['keeper doesn\'t know the sort "bananas", so this space is sorted by modified.'],
      }),
    );

    await screen.findByRole("button", { name: /Tag task/ });
    expect(screen.getByLabelText("Sort by")).toHaveValue("modified");
    expect(screen.getByLabelText("Direction")).toHaveValue("desc");

    fireEvent.click(screen.getByRole("button", { name: "Save" }));
    await waitFor(() => expect(mockSave).toHaveBeenCalledTimes(1));
    expect(savedRequest().sort).toBe("modified desc");
  });

  it("repeats what keeper couldn't read rather than inventing a second sentence", async () => {
    const said = 'keeper doesn\'t know the sort "bananas", so this space is sorted by modified.';
    open(space({ sort: "bananas", sortEffective: "modified desc", warnings: [said] }));

    expect(await screen.findByText(said)).toBeInTheDocument();
  });

  /**
   * A `<select>` whose value matches no option renders the FIRST one — which
   * here would read "Order" for a file that says `recorded`. That is a lie the
   * next Save would make true.
   */
  it("shows a stored sort key it does not offer, rather than silently reading as another", async () => {
    open(space({ sort: "recorded desc", sortEffective: "recorded desc" }));

    await screen.findByRole("button", { name: /Tag task/ });
    expect(screen.getByLabelText("Sort by")).toHaveValue("recorded");

    fireEvent.click(screen.getByRole("button", { name: "Save" }));
    await waitFor(() => expect(mockSave).toHaveBeenCalledTimes(1));
    expect(savedRequest().sort).toBe("recorded desc");
  });

  it("explains `order`, whose name promises a position most files have never been given", async () => {
    open(space({ sort: "order asc", sortEffective: "order asc" }));

    expect(await screen.findByText(SESSION_SPACE_SORT_NOTES["order asc"])).toBeInTheDocument();
  });

  it("says nothing extra about a sort whose name gives it away", async () => {
    open(space({ sort: "modified desc", sortEffective: "modified desc" }));

    await screen.findByRole("button", { name: /Tag task/ });
    expect(screen.queryByText(SESSION_SPACE_SORT_NOTES["order asc"])).not.toBeInTheDocument();
  });

  /**
   * `Number("")` is 0, and 0 means *unset* in this VM — so a cleared box must
   * not be read as "move this space to the front". Held as text for exactly
   * this, and the emptied form saves the same unset it started from.
   */
  it("reads a cleared position as unset rather than as zero-the-position", async () => {
    open(space({ order: 4 }));

    await screen.findByRole("button", { name: /Tag task/ });
    fireEvent.change(screen.getByLabelText("Position"), { target: { value: "" } });
    fireEvent.click(screen.getByRole("button", { name: "Save" }));

    await waitFor(() => expect(mockSave).toHaveBeenCalledTimes(1));
    expect(savedRequest().order).toBe(0);
  });

  it("keeps a fractional position, which is what a drag writes", async () => {
    open(space({ order: 2 }));

    await screen.findByRole("button", { name: /Tag task/ });
    fireEvent.change(screen.getByLabelText("Position"), { target: { value: "2.5" } });
    fireEvent.click(screen.getByRole("button", { name: "Save" }));

    await waitFor(() => expect(mockSave).toHaveBeenCalledTimes(1));
    expect(savedRequest().order).toBe(2.5);
  });
});

describe("SessionSpaceEditor failure", () => {
  it("says the write failed and changes nothing, rather than closing as if it worked", async () => {
    mockSave.mockRejectedValue(new Error("read-only volume"));
    const { onSaved } = open(space());

    await screen.findByRole("button", { name: /Tag task/ });
    fireEvent.click(screen.getByRole("button", { name: "Save" }));

    expect(await screen.findByText(SESSION_SPACE_SAVE_FAILED)).toBeInTheDocument();
    expect(onSaved).not.toHaveBeenCalled();
  });
});
