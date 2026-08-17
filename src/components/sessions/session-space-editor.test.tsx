import { act, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
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
  SESSION_SPACE_CREATE_DIR_LABEL,
  SESSION_SPACE_CREATE_DIR_NOTE,
  SESSION_SPACE_EDIT_TITLE,
  SESSION_SPACE_FOLDED_LABEL,
  SESSION_SPACE_FOLDED_NOTE,
  SESSION_SPACE_NEW_TITLE,
  SESSION_SPACE_ROWS_LABEL,
  SESSION_SPACE_ROWS_NOTE,
  SESSION_SPACE_SAVE_FAILED,
  SESSION_SPACE_SORT_NOTES,
  SESSION_SPACE_TERMS_BROKEN,
  SessionSpaceEditor,
  sessionSpaceCreateDirEmptyNote,
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
    // The editor neither reads nor writes the kind a space can create — that
    // is the section's control (Story 49.2). Present because the VM requires
    // it, and `null` because nothing here is about a creatable space.
    newFileKind: p.newFileKind ?? null,
    // A space that says nothing about how it opens or how much it shows, which
    // is every space that existed before Story 51.3.
    folded: p.folded ?? null,
    rows: p.rows ?? null,
    // A space that says nothing about where its creates land, which is every
    // space that existed before Story 52.5 — and since Story 53.5 `null` is how
    // that is spelled: an ABSENT key, distinct from the `""` an operator writes
    // to mean the session's own root. `?? null` would swallow a deliberate `""`,
    // so the presence of the property is what decides.
    createDir: "createDir" in p ? (p.createDir ?? null) : null,
    // What an absent key inherits, which is Rust's answer and `""` unless a test
    // is about the inheritance.
    createDirDefault: p.createDirDefault ?? "",
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
    // Re-anchored by Story 53.2. This Enter used to land on row 0 of an
    // unfiltered open list and quietly take whichever tag the fixture happened
    // to put first; with the list folded until somebody comes to the field it
    // takes nothing, and the tag this space gets is the one that was typed.
    // Still swallowed, so the dialog's default button does not save either.
    fireEvent.keyDown(screen.getByLabelText("Add a tag"), { key: "Enter" });
    expect(mockSave).not.toHaveBeenCalled();

    fireEvent.change(screen.getByLabelText("Add a tag"), { target: { value: "blocked" } });
    fireEvent.keyDown(screen.getByLabelText("Add a tag"), { key: "Enter" });
    fireEvent.click(screen.getByRole("button", { name: "Save" }));

    await waitFor(() => expect(mockSave).toHaveBeenCalledTimes(1));
    expect(savedRequest().id).toBeNull();
    expect(savedRequest().query).toBe("tag:blocked");
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

/**
 * How a space opens and how much it shows (Story 51.3, rows 9–11).
 *
 * **Row 9 is the one that matters and it is asserted here as well as in Rust.**
 * `render_edit` replaces the whole `keeper:` map, so the destroying bug lives in
 * whichever hop drops the field — and a form that seeded the controls correctly
 * and left them out of the request would pass every Rust test in the crate.
 */
describe("SessionSpaceEditor fold and row cap", () => {
  it("row 9: sends both keys back untouched when something else was edited", async () => {
    open(space({ folded: true, rows: 5 }));

    await screen.findByRole("button", { name: /Tag task/ });
    fireEvent.change(screen.getByLabelText("Name"), { target: { value: "Work items" } });
    fireEvent.click(screen.getByRole("button", { name: "Save" }));

    await waitFor(() => expect(mockSave).toHaveBeenCalledTimes(1));
    expect(savedRequest()).toMatchObject({ name: "Work items", folded: true, rows: 5 });
  });

  /** The controls show what the file says, so Save is the person agreeing with
   *  what is on screen rather than a form guessing. */
  it("seeds both controls from the space's own file", async () => {
    open(space({ folded: false, rows: 5 }));

    await screen.findByRole("button", { name: /Tag task/ });
    expect(screen.getByLabelText(SESSION_SPACE_FOLDED_LABEL)).toHaveValue("unfolded");
    expect(screen.getByLabelText(SESSION_SPACE_ROWS_LABEL)).toHaveValue(5);
  });

  /**
   * Row 10, and the reason this is a three-option control rather than a
   * checkbox: a space that says nothing must be able to keep saying nothing, or
   * the first Save of any space would take it out from under
   * `sessions.spaces_folded` forever.
   */
  it("row 10: writes neither key for a space that was given neither", async () => {
    open(space());

    await screen.findByRole("button", { name: /Tag task/ });
    expect(screen.getByLabelText(SESSION_SPACE_FOLDED_LABEL)).toHaveValue("unset");
    expect(screen.getByLabelText(SESSION_SPACE_ROWS_LABEL)).toHaveValue(null);
    fireEvent.click(screen.getByRole("button", { name: "Save" }));

    await waitFor(() => expect(mockSave).toHaveBeenCalledTimes(1));
    expect(savedRequest()).toMatchObject({ folded: null, rows: null });
  });

  it("row 11: writes exactly what the two controls were set to", async () => {
    open(space());

    await screen.findByRole("button", { name: /Tag task/ });
    fireEvent.change(screen.getByLabelText(SESSION_SPACE_FOLDED_LABEL), {
      target: { value: "folded" },
    });
    fireEvent.change(screen.getByLabelText(SESSION_SPACE_ROWS_LABEL), { target: { value: "3" } });
    fireEvent.click(screen.getByRole("button", { name: "Save" }));

    await waitFor(() => expect(mockSave).toHaveBeenCalledTimes(1));
    expect(savedRequest()).toMatchObject({ folded: true, rows: 3 });
  });

  /** Going back to "however the setting says" has to be sayable, not just
   *  reachable on the way in — otherwise the third state is a one-way door. */
  it("clears a stored fold back to nothing said", async () => {
    open(space({ folded: true }));

    await screen.findByRole("button", { name: /Tag task/ });
    fireEvent.change(screen.getByLabelText(SESSION_SPACE_FOLDED_LABEL), {
      target: { value: "unset" },
    });
    fireEvent.click(screen.getByRole("button", { name: "Save" }));

    await waitFor(() => expect(mockSave).toHaveBeenCalledTimes(1));
    expect(savedRequest().folded).toBeNull();
  });

  /**
   * A cap has to be a whole number above zero. Everything else writes NO key,
   * because a section capped at zero rows under a header that still counts them
   * is not a thing anybody asked for — and refusing the whole save over a
   * presentation field would be worse.
   */
  it.each([
    ["", null],
    ["0", null],
    ["-2", null],
    ["2.5", null],
    ["many", null],
    ["3", 3],
  ])("reads a cap of %o as %o", async (typed, expected) => {
    open(space({ rows: 5 }));

    await screen.findByRole("button", { name: /Tag task/ });
    fireEvent.change(screen.getByLabelText(SESSION_SPACE_ROWS_LABEL), { target: { value: typed } });
    fireEvent.click(screen.getByRole("button", { name: "Save" }));

    await waitFor(() => expect(mockSave).toHaveBeenCalledTimes(1));
    expect(savedRequest().rows).toBe(expected);
  });

  /** Both lines are always on screen, because the two keys are the two things
   *  about a space nobody can guess from a label. */
  it("says what each of the two controls does", async () => {
    open(space());

    expect(await screen.findByText(SESSION_SPACE_FOLDED_NOTE)).toBeInTheDocument();
    expect(screen.getByText(SESSION_SPACE_ROWS_NOTE)).toBeInTheDocument();
  });
});

describe("SessionSpaceEditor failure", () => {
  /**
   * The refusal is Rust's to word (Story 52.5). `sessions_space_save` refuses a
   * destination through `files::check_dir` before anything is written, and each
   * rule has its own sentence — a path that leaves the session, `workspace/`
   * being scratch that dies with the session, a dotted folder the markdown scan
   * never reads back. Rendering {@link SESSION_SPACE_SAVE_FAILED} for all three
   * would tell the operator only that something went wrong, which is the silence
   * this surface exists to end.
   */
  it("renders the refusal keeper actually gave, not a generic one", async () => {
    mockSave.mockRejectedValue(
      new Error(
        "workspace/ is scratch that is not versioned, not synced, and dies with the session",
      ),
    );
    const { onSaved } = open(space());

    await screen.findByRole("button", { name: /Tag task/ });
    fireEvent.change(screen.getByLabelText(SESSION_SPACE_CREATE_DIR_LABEL), {
      target: { value: "workspace/logs" },
    });
    fireEvent.click(screen.getByRole("button", { name: "Save" }));

    expect(await screen.findByText(/dies with the session/)).toBeInTheDocument();
    expect(screen.queryByText(SESSION_SPACE_SAVE_FAILED)).toBeNull();
    expect(onSaved).not.toHaveBeenCalled();
  });

  /** And the fallback is still there for a rejection that says nothing readable —
   *  an empty message must not render as an empty refusal. */
  it("says the write failed and changes nothing, rather than closing as if it worked", async () => {
    mockSave.mockRejectedValue(new Error(""));
    const { onSaved } = open(space());

    await screen.findByRole("button", { name: /Tag task/ });
    fireEvent.click(screen.getByRole("button", { name: "Save" }));

    expect(await screen.findByText(SESSION_SPACE_SAVE_FAILED)).toBeInTheDocument();
    expect(onSaved).not.toHaveBeenCalled();
  });
});

describe("SessionSpaceEditor destination", () => {
  /**
   * Story 52.5, FR-309. A space may name a folder its creates land in. The form's
   * whole job here is to carry the answer both ways without editing it: Rust owns
   * what a path may be, and `render_edit` rewrites the whole `keeper:` map, so a
   * save that omitted the key would delete the operator's answer — `folded`'s
   * reason, one field over.
   */
  it("round-trips the destination a space already names", async () => {
    open(space({ createDir: "logs" }));

    await screen.findByRole("button", { name: /Tag task/ });
    const field = screen.getByLabelText<HTMLInputElement>(SESSION_SPACE_CREATE_DIR_LABEL);
    expect(field.value).toBe("logs");

    fireEvent.click(screen.getByRole("button", { name: "Save" }));
    await waitFor(() => expect(mockSave).toHaveBeenCalledTimes(1));
    expect(savedRequest().createDir).toBe("logs");
  });

  it("sends what was typed, trimmed", async () => {
    open(space());

    await screen.findByRole("button", { name: /Tag task/ });
    fireEvent.change(screen.getByLabelText(SESSION_SPACE_CREATE_DIR_LABEL), {
      target: { value: "  notes/2026  " },
    });
    fireEvent.click(screen.getByRole("button", { name: "Save" }));

    await waitFor(() => expect(mockSave).toHaveBeenCalledTimes(1));
    expect(savedRequest().createDir).toBe("notes/2026");
  });

  /**
   * Story 53.5 changes what a cleared box means. `""` used to be "no answer" and
   * wrote no key; now it is a DELIBERATE "the session's own root", written as the
   * empty key so an unrelated Save keeps saying it. The absent state — which is
   * what inherits — is `null`, and only an untouched field sends that.
   */
  it("sends an explicit empty destination when the box is cleared", async () => {
    open(space({ createDir: "logs" }));

    await screen.findByRole("button", { name: /Tag task/ });
    fireEvent.change(screen.getByLabelText(SESSION_SPACE_CREATE_DIR_LABEL), {
      target: { value: "   " },
    });
    fireEvent.click(screen.getByRole("button", { name: "Save" }));

    await waitFor(() => expect(mockSave).toHaveBeenCalledTimes(1));
    expect(savedRequest().createDir).toBe("");
  });

  /**
   * Story 53.5, acceptance 9 from the form's side. A space whose file names no
   * destination must still name none after a save that was about something else:
   * sending `""` here would persist a key nobody typed and stop the space
   * inheriting its default's folder, and sending the INHERITED folder would be
   * keeper writing a default into the operator's file (AD-121).
   */
  it("sends null for a destination nobody touched, so no key is written", async () => {
    open(space({ defaultKey: "tasks", createDirDefault: "tasks" }));

    await screen.findByRole("button", { name: /Tag task/ });
    fireEvent.change(screen.getByLabelText("Name"), { target: { value: "Backlog" } });
    fireEvent.click(screen.getByRole("button", { name: "Save" }));

    await waitFor(() => expect(mockSave).toHaveBeenCalledTimes(1));
    expect(savedRequest().createDir).toBeNull();
  });

  /**
   * Story 53.5, acceptance 3 from the form's side. The inherited folder is the
   * PLACEHOLDER and never the value: in the box it would be saved on the next
   * press, and the operator would end up with a key keeper wrote for them.
   */
  it("shows the inherited folder as the placeholder and not as the value", async () => {
    open(space({ defaultKey: "tasks", createDirDefault: "tasks" }));

    await screen.findByRole("button", { name: /Tag task/ });
    const field = screen.getByLabelText<HTMLInputElement>(SESSION_SPACE_CREATE_DIR_LABEL);
    expect(field.value).toBe("");
    expect(field.placeholder).toBe("tasks");
  });

  /**
   * Story 53.5, acceptance 4. Two files can both draw an empty box, and the
   * operator cannot act on either without being told which one they are looking
   * at — so the sentence under the field is the distinction, and it tracks the
   * box as it is typed in.
   */
  it("says which of the two empties an inheriting space is, as the box is typed in", async () => {
    open(space({ defaultKey: "tasks", createDirDefault: "tasks" }));
    await screen.findByRole("button", { name: /Tag task/ });
    expect(
      screen.getByText(sessionSpaceCreateDirEmptyNote(null, "tasks") ?? ""),
    ).toBeInTheDocument();

    // Clearing the box is a different answer, and the sentence says so before
    // anything is saved.
    fireEvent.change(screen.getByLabelText(SESSION_SPACE_CREATE_DIR_LABEL), {
      target: { value: "x" },
    });
    fireEvent.change(screen.getByLabelText(SESSION_SPACE_CREATE_DIR_LABEL), {
      target: { value: "" },
    });
    expect(screen.getByText(sessionSpaceCreateDirEmptyNote("", "tasks") ?? "")).toBeInTheDocument();
  });

  /**
   * The other half of acceptance 4: a space whose file already carries the empty
   * key reads as the session's own root the moment it opens, and does not borrow
   * the `refs/` it would otherwise have inherited.
   */
  it("reads an explicit empty destination as the session's own folder on open", async () => {
    open(space({ createDir: "", defaultKey: "refs", createDirDefault: "refs" }));
    await screen.findByRole("button", { name: /Tag task/ });
    const field = screen.getByLabelText<HTMLInputElement>(SESSION_SPACE_CREATE_DIR_LABEL);
    expect(field.value).toBe("");
    expect(field.placeholder).toBe("The session's own folder");
    expect(screen.getByText(sessionSpaceCreateDirEmptyNote("", "refs") ?? "")).toBeInTheDocument();
  });

  /**
   * The three sentences, on their own — the branch table, so a reworded note
   * cannot quietly collapse two states into one.
   */
  it("names the inherited folder in one sentence and the root in the other", () => {
    expect(sessionSpaceCreateDirEmptyNote(null, "tasks")).toContain("tasks");
    expect(sessionSpaceCreateDirEmptyNote("", "tasks")).toContain("tasks");
    expect(sessionSpaceCreateDirEmptyNote(null, "tasks")).not.toBe(
      sessionSpaceCreateDirEmptyNote("", "tasks"),
    );
    // No default to inherit: one sentence for both empties, because there is
    // only one answer.
    expect(sessionSpaceCreateDirEmptyNote(null, "")).toBe(sessionSpaceCreateDirEmptyNote("", ""));
    // A box with a folder in it explains itself.
    expect(sessionSpaceCreateDirEmptyNote("logs", "tasks")).toBeNull();
  });

  /** The note is always on screen: "New files go in" alone reads like a filter,
   *  and the three places keeper refuses to write are not guessable from a label. */
  it("says what the field does and where keeper will not write", async () => {
    open(space());

    expect(await screen.findByText(SESSION_SPACE_CREATE_DIR_NOTE)).toBeInTheDocument();
  });
});

describe("SessionSpaceEditor reach", () => {
  /**
   * Story 52.6, FR-310. The form is taller than a 900px window, and the shadcn
   * panel constrains width only, centred by a transform — a transform creates no
   * scroll container, so the top of the form was unreachable rather than merely
   * clipped.
   *
   * jsdom cannot measure that: `src/test/setup.ts:84-103` shims every rect to
   * 1024×768, so `getBoundingClientRect()` and `scrollHeight` here are fiction.
   * The assertion is therefore by className — the pattern this repo already uses
   * for caps CSS-less jsdom cannot compute (`chat/composer.test.tsx:992`) — and
   * the real measurement is owed in a browser at 900×900: the panel's
   * `getBoundingClientRect().top >= 0` and the body's
   * `scrollHeight > clientHeight`.
   */
  it("caps the panel height and scrolls the form inside it, so both ends are reachable", async () => {
    open(space());
    await screen.findByRole("button", { name: /Tag task/ });

    const panel = screen.getByRole("dialog");
    // A height-capped flex column that clips — the Settings idiom
    // (`settings-dialog.tsx:110`), not a second invention.
    expect(panel.className).toContain("max-h-[85vh]");
    expect(panel.className).toContain("overflow-hidden");
    expect(panel.className).toContain("flex");
    expect(panel.className).toContain("flex-col");
    // The width the surface already had is untouched; only the height gained a cap.
    expect(panel.className).toContain("sm:max-w-lg");
    // The panel clips. It is never the thing that scrolls.
    expect(panel.className).not.toContain("overflow-y-auto");

    const body = panel.querySelector<HTMLElement>(":scope > .overflow-y-auto");
    expect(body).not.toBeNull();
    // `min-h-0` is load-bearing, not decoration. A flex child defaults to
    // `min-height:auto`, which is its *content* size, so without `min-h-0` this
    // body grows straight past the panel's cap and bleeds out of the dialog
    // instead of scrolling — the exact bug this test exists for. Removing it "to
    // simplify" reopens it. `min-w-0` lets the help copy wrap instead of clipping
    // on the right; `flex-1` is what makes the body take the bounded remainder.
    expect(body?.className).toContain("min-h-0");
    expect(body?.className).toContain("min-w-0");
    expect(body?.className).toContain("flex-1");

    // And it is the *form* that scrolls, between a pinned header and footer — so
    // the classes cannot quietly drift onto some empty wrapper.
    expect(body?.contains(screen.getByLabelText("Name"))).toBe(true);
    expect(body?.contains(screen.getByRole("button", { name: "Save" }))).toBe(false);
    expect(body?.contains(screen.getByText(SESSION_SPACE_EDIT_TITLE))).toBe(false);
  });

  /**
   * The half of the fix that is easy to delete because it looks decorative. The
   * body holds two children that own a scroll region — the icon grid and the tag
   * combobox's listbox, which Story 53.2 folds with `hidden` but still renders —
   * and a flex item whose own overflow is
   * not `visible` has an automatic minimum size of ZERO. They are therefore the
   * only children that can give ground, so without `shrink-0` the flex algorithm
   * hands them the body's entire negative free space: the icon chooser and the
   * tag list collapse to a sliver and the body never scrolls at all. It is the
   * same algorithm that squeezed the files pane's prose to one word per line
   * (`files-pane.test.tsx:2897`), in the other axis.
   */
  it("keeps the icon chooser and the terms section from absorbing the body's overflow", async () => {
    open(space());
    await screen.findByRole("button", { name: /Tag task/ });

    const panel = screen.getByRole("dialog");
    const body = panel.querySelector<HTMLElement>(":scope > .overflow-y-auto");
    const iconGroup = screen.getByRole("group", { name: "Icon" });
    const terms = screen.getByRole("region", { name: "Terms" });

    // Both are direct children of the scrolling body, and neither may shrink.
    expect(iconGroup.parentElement).toBe(body);
    expect(terms.parentElement).toBe(body);
    expect(iconGroup.className).toContain("shrink-0");
    expect(terms.className).toContain("shrink-0");

    // And each still contains the scroll region that is the reason for it.
    expect(iconGroup.querySelector(".overflow-y-auto")).not.toBeNull();
    expect(terms.querySelector<HTMLElement>("[role='listbox']")?.className).toContain(
      "overflow-y-auto",
    );
  });
});

/**
 * Folding the tag list away (Story 53.2, FR-315).
 *
 * This dialog is one of the two surfaces that mounted the chooser
 * unconditionally and passed it no `onDismiss`, so it had no close path in the
 * product at all: the list was on screen above the Save button from the moment
 * the editor opened. The close is the control's own now — it opens folded and
 * the caret leaving folds it again — so these assertions are about this surface
 * rather than about the mechanism, which has its own suite in
 * `tag-combobox.test.tsx`. Escape is deliberately not asserted here: Radix's
 * dismissable layer claims Escape at the document in the CAPTURE phase, so on
 * this dialog it closes the editor before the chooser sees the key — this
 * form's own older decision, and not something to fight from inside a combobox.
 */
describe("SessionSpaceEditor tag chooser", () => {
  it("opens with the list folded, and the caret is what unfolds it", async () => {
    open(space());

    await screen.findByRole("button", { name: /Tag task/ });
    expect(screen.queryByRole("listbox")).toBeNull();

    const field = screen.getByLabelText("Add a tag");
    act(() => {
      field.focus();
    });

    // `task` is absent because the space already carries it, which is the same
    // browsable list 44.13 shipped — one focus away instead of always there.
    expect(
      within(screen.getByRole("listbox"))
        .getAllByRole("option")
        .map((row) => row.textContent),
    ).toEqual(["log"]);
  });

  it("folds the list on a press elsewhere on the form, and that press still lands", async () => {
    const { onClose } = open(space());

    await screen.findByRole("button", { name: /Tag task/ });
    const field = screen.getByLabelText("Add a tag");
    act(() => {
      field.focus();
    });
    expect(screen.getByRole("listbox")).toBeVisible();

    // The press, in the order a browser fires it. The fold must not happen
    // between the pointer going down and the click landing, or every control
    // below this list moves out from under the cursor mid-press.
    const name = screen.getByLabelText("Name");
    fireEvent.pointerDown(name);
    act(() => {
      name.focus();
    });
    fireEvent.pointerUp(name);
    fireEvent.click(name);

    expect(screen.queryByRole("listbox")).toBeNull();
    expect(document.activeElement).toBe(name);
    expect(onClose).not.toHaveBeenCalled();
    expect(screen.getByRole("button", { name: "Save" })).toBeInTheDocument();
  });
});
