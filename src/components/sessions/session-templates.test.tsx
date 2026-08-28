import { fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { SessionPatternVm, SessionTemplateEntryVm } from "@/lib/ipc/client";

// The room reads through one command and writes through five; the template create
// verb is the pane's, injected. Stubbing at the IPC boundary leaves the real
// component — the real refusals, the real copy, the real path handling — under
// test.
vi.mock("@/lib/ipc/client", () => ({
  sessionsTemplateEntries: vi.fn(),
  sessionsTemplateRename: vi.fn(),
  sessionsTemplateFileNew: vi.fn(),
  sessionsTemplateDirNew: vi.fn(),
  sessionsTemplateRenameEntry: vi.fn(),
  sessionsTemplateDeleteEntry: vi.fn(),
}));

import { SESSION_PATTERN_INSTALL_LABEL } from "@/components/sessions/session-pattern-picker";
import {
  SESSION_TEMPLATE_DELETE_BODY,
  SESSION_TEMPLATE_DELETE_TITLE,
  SESSION_TEMPLATE_ENTRY_DELETE,
  SESSION_TEMPLATE_ENTRY_RENAME,
  SESSION_TEMPLATE_ENTRY_RENAME_LABEL,
  SESSION_TEMPLATE_FILE_TESTID,
  SESSION_TEMPLATE_NEW_CONFIRM,
  SESSION_TEMPLATE_NEW_FILE,
  SESSION_TEMPLATE_NEW_FILE_LABEL,
  SESSION_TEMPLATE_NEW_FOLDER,
  SESSION_TEMPLATE_NEW_FOLDER_LABEL,
  SESSION_TEMPLATE_PLACEHOLDERS,
  SESSION_TEMPLATE_PLACEHOLDERS_UNKNOWN,
  SESSION_TEMPLATE_RENAME,
  SESSION_TEMPLATE_RENAME_CONFIRM,
  SESSION_TEMPLATE_RENAME_NAME_LABEL,
  SESSION_TEMPLATE_SECTION_TESTID,
  SESSION_TEMPLATES_EMPTY,
  SESSION_TEMPLATES_HINT,
  SESSION_TEMPLATES_LOADING,
  SESSION_TEMPLATES_NEW,
  SESSION_TEMPLATES_NEW_NAME_LABEL,
  SESSION_TEMPLATES_NO_FILES,
  SESSION_TEMPLATES_PLACEHOLDERS_LABEL,
  SessionTemplates,
  sessionTemplateTaken,
  templateTree,
} from "@/components/sessions/session-templates";
import {
  sessionsTemplateDeleteEntry,
  sessionsTemplateDirNew,
  sessionsTemplateEntries,
  sessionsTemplateFileNew,
  sessionsTemplateRename,
  sessionsTemplateRenameEntry,
} from "@/lib/ipc/client";
import { activePanel, panelsStore, resetPanelsStoreForTest } from "@/lib/stores/panels";

const mockEntries = vi.mocked(sessionsTemplateEntries);
const mockRename = vi.mocked(sessionsTemplateRename);
const mockFileNew = vi.mocked(sessionsTemplateFileNew);
const mockDirNew = vi.mocked(sessionsTemplateDirNew);
const mockRenameEntry = vi.mocked(sessionsTemplateRenameEntry);
const mockDeleteEntry = vi.mocked(sessionsTemplateDeleteEntry);

const NOW = Date.now();

/** The zone's own `_template/` as `sessions_patterns` returns it. */
function zoneTemplate(): SessionPatternVm {
  return {
    id: "_template",
    kind: "template",
    label: "Zone template",
    detail: "the zone's own skeleton — copied whole",
    mtimeMs: null,
    copies: [{ relPath: "AGENTS.md", isDir: false }],
    skips: [],
  };
}

/** A `_template/<name>/` (FR-266). */
function namedTemplate(name = "interview"): SessionPatternVm {
  return {
    id: `_template/${name}`,
    kind: "template",
    label: name,
    detail: "a named template — copied whole",
    mtimeMs: NOW - 3 * 24 * 60 * 60_000,
    copies: [{ relPath: "questions.md", isDir: false }],
    skips: [],
  };
}

/** A session pattern, which this room must not show. */
function sessionPattern(): SessionPatternVm {
  return {
    id: "01J5AAAAAAAAAAAAAAAAAAAAAA",
    kind: "session",
    label: "keeper — rolling work session",
    detail: "continues this session",
    mtimeMs: NOW - 60 * 60_000,
    copies: [],
    skips: [],
  };
}

/**
 * One entry, exactly as the shell composes it: a profile-relative subpath whose
 * zone subfolder Rust knows and this file does not (AD-65).
 */
function entry(subpath: string, name: string): SessionTemplateEntryVm {
  return { subpath, name, mtimeMs: NOW - 60_000, isDir: false };
}

/**
 * One directory, as the shell lists it — a row of its own, whether or not
 * anything is inside it. An empty one had no row at all before story 50.2's
 * review, so `New folder` made something no verb in the room could reach.
 */
function folder(subpath: string, name: string): SessionTemplateEntryVm {
  return { subpath, name, mtimeMs: NOW - 60_000, isDir: true };
}

/** What the strip is showing — the panel a press on a file row filled. */
function opened() {
  return activePanel(panelsStore.getState()).target;
}

function open(patterns: SessionPatternVm[] | null, rootId = "tgdrive") {
  const onInstallTemplate = vi.fn(async (_name?: string): Promise<string | null> => null);
  const onChanged = vi.fn();
  const view = render(
    <SessionTemplates
      rootId={rootId}
      patterns={patterns}
      onInstallTemplate={onInstallTemplate}
      installing={false}
      onChanged={onChanged}
      nowMs={NOW}
    />,
  );
  return { onInstallTemplate, onChanged, view };
}

beforeEach(() => {
  mockEntries.mockReset();
  mockEntries.mockResolvedValue([]);
  mockRename.mockReset();
  mockRename.mockResolvedValue("_template/kick-off");
  mockFileNew.mockReset();
  mockFileNew.mockResolvedValue("60-sessions/_template/interview/notes.md");
  mockDirNew.mockReset();
  mockDirNew.mockResolvedValue(undefined);
  mockRenameEntry.mockReset();
  mockRenameEntry.mockResolvedValue("60-sessions/_template/interview/record.md");
  mockDeleteEntry.mockReset();
  mockDeleteEntry.mockResolvedValue(undefined);
  resetPanelsStoreForTest();
});

afterEach(() => {
  vi.clearAllMocks();
});

describe("SessionTemplates listing", () => {
  it("shows one section per template, in the order Rust returned, and no sessions", async () => {
    mockEntries.mockResolvedValue([entry("60-sessions/_template/AGENTS.md", "AGENTS.md")]);
    open([zoneTemplate(), namedTemplate(), sessionPattern()]);

    expect(await screen.findByRole("heading", { name: "Zone template" })).toBeInTheDocument();
    expect(screen.getByRole("heading", { name: "interview" })).toBeInTheDocument();
    // A session is a pattern, not a template: the create row offers it, this
    // room does not.
    expect(
      screen.queryByRole("heading", { name: "keeper — rolling work session" }),
    ).not.toBeInTheDocument();
  });

  it("asks for each template by the name Rust gave it, and each section shows its OWN files", async () => {
    // A distinct answer per name, so the assertion has somewhere to land: a
    // `toHaveBeenCalledWith` on its own survives every heading, row and verb in
    // this component being deleted, and cannot tell which section got which read.
    mockEntries.mockImplementation(async (_rootId: string, name?: string) =>
      name === undefined
        ? [entry("60-sessions/_template/AGENTS.md", "AGENTS.md")]
        : [entry(`60-sessions/_template/${name}/questions.md`, "questions.md")],
    );
    open([zoneTemplate(), namedTemplate()]);

    // `_template` IS the zone's template; it has no name argument, and a
    // TypeScript-composed `"_template"` name would address `_template/_template`.
    await waitFor(() => expect(mockEntries).toHaveBeenCalledWith("tgdrive", undefined));
    expect(mockEntries).toHaveBeenCalledWith("tgdrive", "interview");

    // And the consequence: each section drew the answer to its own question. A
    // component that fired both reads and rendered either one under both
    // headings fails here.
    const zoneId = `${SESSION_TEMPLATE_SECTION_TESTID}-_template`;
    const namedId = `${SESSION_TEMPLATE_SECTION_TESTID}-_template/interview`;
    const zone = within(await screen.findByTestId(zoneId));
    // Exact names, not `/AGENTS\.md/`: a row carries its own Rename and Delete
    // now, and those are labelled with the entry they act on — a substring match
    // would find three buttons and fail on the ambiguity rather than on the room.
    expect(await zone.findByRole("button", { name: "AGENTS.md" })).toBeInTheDocument();
    expect(zone.queryByRole("button", { name: "questions.md" })).not.toBeInTheDocument();
    const named = within(screen.getByTestId(namedId));
    expect(named.getByRole("button", { name: "questions.md" })).toBeInTheDocument();
    expect(named.queryByRole("button", { name: "AGENTS.md" })).not.toBeInTheDocument();
  });

  it("passes a hand-made template's folder name through untouched, and lists what came back", async () => {
    // A named template's `label` IS its directory name (`sessions_ipc.rs:731-740`)
    // and the shell addresses it verbatim — only a NEW name gets slugged. A slug
    // or a re-derivation here would look for `_template/interview-kit` and find
    // an empty room where the operator's files are. This is the ONLY guard on
    // that rule, so it asserts the room as well as the argument: only the
    // verbatim name answers with files, and a slug would leave the section
    // saying it is empty over a template with a file in it.
    mockEntries.mockImplementation(async (_rootId: string, name?: string) =>
      name === "Interview Kit"
        ? [entry("60-sessions/_template/Interview Kit/questions.md", "questions.md")]
        : [],
    );
    open([namedTemplate("Interview Kit")]);

    await waitFor(() => expect(mockEntries).toHaveBeenCalledWith("tgdrive", "Interview Kit"));
    expect(await screen.findByRole("button", { name: "questions.md" })).toBeInTheDocument();
    expect(screen.queryByText(SESSION_TEMPLATES_NO_FILES)).not.toBeInTheDocument();
  });

  it("opens a file row at the EXACT subpath Rust returned", async () => {
    // The literal string the shell composed. Nothing here joins the zone's
    // subfolder onto it — the assertion is the AD-65 guard.
    const said = "60-sessions/_template/interview/questions.md";
    mockEntries.mockResolvedValue([entry(said, "questions.md")]);
    open([namedTemplate()]);

    fireEvent.click(await screen.findByRole("button", { name: "questions.md" }));

    expect(opened()).toEqual({ kind: "file", profileId: "tgdrive", relativePath: said });
    expect(screen.getByTestId(`${SESSION_TEMPLATE_FILE_TESTID}-${said}`)).toBeInTheDocument();
  });

  it("says an empty template is empty rather than looking broken", async () => {
    open([zoneTemplate()]);
    expect(await screen.findByText(SESSION_TEMPLATES_NO_FILES)).toBeInTheDocument();
  });

  it("re-reads for the new root and keeps no cross-root rows", async () => {
    mockEntries.mockResolvedValue([entry("60-sessions/_template/AGENTS.md", "AGENTS.md")]);
    const { view } = open([zoneTemplate()]);
    await screen.findByRole("button", { name: "AGENTS.md" });

    mockEntries.mockResolvedValue([entry("work/_template/CLAUDE.md", "CLAUDE.md")]);
    view.rerender(
      <SessionTemplates
        rootId="neuradrive"
        patterns={[zoneTemplate()]}
        onInstallTemplate={vi.fn(async () => null)}
        installing={false}
        onChanged={vi.fn()}
        nowMs={NOW}
      />,
    );

    await waitFor(() => expect(mockEntries).toHaveBeenLastCalledWith("neuradrive", undefined));
    expect(await screen.findByRole("button", { name: "CLAUDE.md" })).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "AGENTS.md" })).not.toBeInTheDocument();
  });

  it("costs one section its files when one template's read is refused, not the room", async () => {
    // What `template_at` answers for a name it will not join. It is one
    // template's refusal: the sibling read succeeded, and `Promise.all` used to
    // throw that answer away and leave EVERY section on "Reading…" for good.
    const said =
      '"interview" is not a name keeper will look for under _template/ — a template\'s ' +
      'directory name carries no separators, is not "." or "..", and does not begin with ' +
      "a dot or an underscore.";
    mockEntries.mockImplementation((_rootId: string, name?: string) =>
      name === "interview"
        ? Promise.reject({ message: said })
        : Promise.resolve([entry("60-sessions/_template/AGENTS.md", "AGENTS.md")]),
    );
    open([zoneTemplate(), namedTemplate()]);

    const zoneId = `${SESSION_TEMPLATE_SECTION_TESTID}-_template`;
    const refusedId = `${SESSION_TEMPLATE_SECTION_TESTID}-_template/interview`;

    // The sibling still shows what it answered.
    const zone = within(await screen.findByTestId(zoneId));
    expect(await zone.findByRole("button", { name: "AGENTS.md" })).toBeInTheDocument();
    expect(zone.queryByText(SESSION_TEMPLATES_LOADING)).not.toBeInTheDocument();

    // And the refused one says so in Rust's words, under its own heading, rather
    // than sitting on "Reading…" over a read that already stopped.
    const refused = within(screen.getByTestId(refusedId));
    expect(refused.getByText(said)).toBeInTheDocument();
    expect(refused.queryByText(SESSION_TEMPLATES_LOADING)).not.toBeInTheDocument();
  });

  // Row 11 of the 51.4 matrix. The room is where somebody stands when they open
  // a template file to edit it, so it is the room that has to state what a
  // `{{token}}` does — a table in `docs/sessions.md` is not read by a person
  // already typing.
  it("states the placeholder vocabulary, including the rule for an unknown token", async () => {
    open([zoneTemplate()]);

    // The list is one press away and its summary is always on screen: a person
    // who does not know the feature exists still meets the sentence.
    const disclosure = await screen.findByText(SESSION_TEMPLATES_PLACEHOLDERS_LABEL);
    expect(disclosure).toBeInTheDocument();

    // Every token, with what it means beside it. Asserting the pairs rather than
    // the tokens alone: a list of six code spans with no explanations would pass
    // a `getByText("{{title}}")` and teach nobody anything.
    for (const { token, means } of SESSION_TEMPLATE_PLACEHOLDERS) {
      expect(screen.getByText(token)).toBeInTheDocument();
      expect(screen.getByText(means)).toBeInTheDocument();
    }

    // The rule that makes the feature safe in a document full of braces, and the
    // one thing trying it cannot teach you: an unexpanded `{{foo}}` looks exactly
    // like a bug until somebody says it is the contract.
    expect(screen.getByText(SESSION_TEMPLATE_PLACEHOLDERS_UNKNOWN)).toBeInTheDocument();

    // And the restated copy promise: expansion happens INTO the new session, so
    // the template's own bytes are still untouched.
    expect(screen.getByText(SESSION_TEMPLATES_HINT)).toBeInTheDocument();
  });
});

describe("SessionTemplates creating", () => {
  it("sends the name to the pane's install verb and clears the field", async () => {
    const { onInstallTemplate } = open([zoneTemplate()]);

    fireEvent.change(screen.getByLabelText(SESSION_TEMPLATES_NEW_NAME_LABEL), {
      target: { value: "  interview  " },
    });
    fireEvent.click(screen.getByRole("button", { name: SESSION_TEMPLATES_NEW }));

    await waitFor(() => expect(onInstallTemplate).toHaveBeenCalledWith("interview"));
    await waitFor(() =>
      expect(screen.getByLabelText(SESSION_TEMPLATES_NEW_NAME_LABEL)).toHaveValue(""),
    );
  });

  it("is inert with an empty name — Rust is never asked what an empty name means", async () => {
    const { onInstallTemplate } = open([zoneTemplate()]);
    // Let the section's read land first, so the only thing under test after this
    // is the press.
    await screen.findByText(SESSION_TEMPLATES_NO_FILES);

    fireEvent.click(screen.getByRole("button", { name: SESSION_TEMPLATES_NEW }));
    fireEvent.change(screen.getByLabelText(SESSION_TEMPLATES_NEW_NAME_LABEL), {
      target: { value: "   " },
    });
    fireEvent.click(screen.getByRole("button", { name: SESSION_TEMPLATES_NEW }));

    expect(onInstallTemplate).not.toHaveBeenCalled();
  });

  it("says a name Rust won't make a folder out of in Rust's own words, keeping it typed", async () => {
    // What `template_mint` actually answers for a name with nothing sluggable in
    // it (`sessions_ipc.rs`). The previous fixture here was a "File exists (os
    // error 17)" refusal, which install cannot produce: `compile_install`
    // trash-then-writes through an occupied name and answers `Ok`, and the
    // `MkDir` under it is idempotent by contract. The occupied-name case is now
    // refused by the room before the wire — see the test below.
    const said =
      '"###" has nothing in it a folder can be named after — a named template needs ' +
      "letters or digits.";
    const { onInstallTemplate } = open([zoneTemplate()]);
    onInstallTemplate.mockResolvedValue(said);

    fireEvent.change(screen.getByLabelText(SESSION_TEMPLATES_NEW_NAME_LABEL), {
      target: { value: "###" },
    });
    fireEvent.click(screen.getByRole("button", { name: SESSION_TEMPLATES_NEW }));

    expect(await screen.findByRole("status")).toHaveTextContent(said);
    expect(screen.getByLabelText(SESSION_TEMPLATES_NEW_NAME_LABEL)).toHaveValue("###");
  });

  it("refuses a name the zone already has before the wire, case and all", async () => {
    const { onInstallTemplate } = open([zoneTemplate(), namedTemplate()]);
    await screen.findByRole("heading", { name: "interview" });

    // Case-insensitively: the drives this syncs to treat `Interview` and
    // `interview` as one directory, so the second create lands on the first.
    fireEvent.change(screen.getByLabelText(SESSION_TEMPLATES_NEW_NAME_LABEL), {
      target: { value: "  Interview  " },
    });
    fireEvent.click(screen.getByRole("button", { name: SESSION_TEMPLATES_NEW }));

    expect(await screen.findByRole("status")).toHaveTextContent(sessionTemplateTaken("interview"));
    // `sessions_template_install` would trash this template's AGENTS.md and
    // about.md into `.keeper/trash/` and write keeper's over them, and answer
    // `Ok`. New template is not the verb that gets to do that, so Rust is never
    // asked and the name stays put for a correction.
    expect(onInstallTemplate).not.toHaveBeenCalled();
    expect(screen.getByLabelText(SESSION_TEMPLATES_NEW_NAME_LABEL)).toHaveValue("  Interview  ");
  });

  it("asks nothing while the pattern list is still out — there is no list to collide with", () => {
    const { onInstallTemplate } = open(null);

    fireEvent.change(screen.getByLabelText(SESSION_TEMPLATES_NEW_NAME_LABEL), {
      target: { value: "interview" },
    });
    // Visibly inert, not silently: the collision check has nothing to answer
    // against yet, and install would write over whatever the read is about to
    // show.
    expect(screen.getByRole("button", { name: SESSION_TEMPLATES_NEW })).toBeDisabled();
    fireEvent.keyDown(screen.getByLabelText(SESSION_TEMPLATES_NEW_NAME_LABEL), { key: "Enter" });

    expect(onInstallTemplate).not.toHaveBeenCalled();
  });

  it("offers both paths to a zone with no template at all", () => {
    const { onInstallTemplate } = open([]);

    expect(screen.getByText(SESSION_TEMPLATES_EMPTY)).toBeInTheDocument();
    // The create verb, and the zone-template install the create row also offers —
    // reachable from the room the operator is standing in.
    expect(screen.getByRole("button", { name: SESSION_TEMPLATES_NEW })).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: SESSION_PATTERN_INSTALL_LABEL }));

    expect(onInstallTemplate).toHaveBeenCalledWith();
    // A zone with no template has nothing to walk: no read is fired for it.
    expect(mockEntries).not.toHaveBeenCalled();
  });
});

describe("SessionTemplates renaming", () => {
  it("never offers to rename the zone's own template — the directory name is the contract", async () => {
    open([zoneTemplate(), namedTemplate()]);
    await screen.findByRole("heading", { name: "Zone template" });

    expect(
      screen.queryByRole("button", { name: `${SESSION_TEMPLATE_RENAME} Zone template` }),
    ).not.toBeInTheDocument();
    expect(
      screen.getByRole("button", { name: `${SESSION_TEMPLATE_RENAME} interview` }),
    ).toBeInTheDocument();
  });

  it("renames a named template by name and re-reads", async () => {
    const { onChanged } = open([namedTemplate()]);

    fireEvent.click(
      await screen.findByRole("button", { name: `${SESSION_TEMPLATE_RENAME} interview` }),
    );
    fireEvent.change(screen.getByLabelText(SESSION_TEMPLATE_RENAME_NAME_LABEL), {
      target: { value: "Kick Off" },
    });
    fireEvent.click(screen.getByRole("button", { name: SESSION_TEMPLATE_RENAME_CONFIRM }));

    // The name, not the id: Rust slugs it and answers with the new id.
    await waitFor(() =>
      expect(mockRename).toHaveBeenCalledWith("tgdrive", "interview", "Kick Off"),
    );
    await waitFor(() => expect(onChanged).toHaveBeenCalled());
  });

  it("renames a hand-made template under the name on disk, not a slug of it", async () => {
    open([namedTemplate("Interview Kit")]);

    fireEvent.click(
      await screen.findByRole("button", { name: `${SESSION_TEMPLATE_RENAME} Interview Kit` }),
    );
    fireEvent.change(screen.getByLabelText(SESSION_TEMPLATE_RENAME_NAME_LABEL), {
      target: { value: "Kick Off" },
    });
    fireEvent.click(screen.getByRole("button", { name: SESSION_TEMPLATE_RENAME_CONFIRM }));

    // The source is addressed verbatim — it exists — and only the destination is
    // Rust's to slug.
    await waitFor(() =>
      expect(mockRename).toHaveBeenCalledWith("tgdrive", "Interview Kit", "Kick Off"),
    );
  });

  it("renders a refused rename verbatim and changes nothing", async () => {
    const said = "a template named kick-off is already here";
    mockRename.mockRejectedValue({ message: said });
    const { onChanged } = open([namedTemplate()]);

    fireEvent.click(
      await screen.findByRole("button", { name: `${SESSION_TEMPLATE_RENAME} interview` }),
    );
    fireEvent.change(screen.getByLabelText(SESSION_TEMPLATE_RENAME_NAME_LABEL), {
      target: { value: "Kick Off" },
    });
    fireEvent.click(screen.getByRole("button", { name: SESSION_TEMPLATE_RENAME_CONFIRM }));

    expect(await screen.findByRole("status")).toHaveTextContent(said);
    // Nothing moved: the section keeps its name, the form stays open on the name
    // that was typed, and no re-read was asked for.
    expect(screen.getByRole("heading", { name: "interview" })).toBeInTheDocument();
    expect(screen.getByLabelText(SESSION_TEMPLATE_RENAME_NAME_LABEL)).toHaveValue("Kick Off");
    expect(onChanged).not.toHaveBeenCalled();
  });

  it("is inert with an empty new name", async () => {
    open([namedTemplate()]);

    fireEvent.click(
      await screen.findByRole("button", { name: `${SESSION_TEMPLATE_RENAME} interview` }),
    );
    fireEvent.change(screen.getByLabelText(SESSION_TEMPLATE_RENAME_NAME_LABEL), {
      target: { value: "  " },
    });
    fireEvent.click(screen.getByRole("button", { name: SESSION_TEMPLATE_RENAME_CONFIRM }));

    expect(mockRename).not.toHaveBeenCalled();
  });

  it("sends ONE rename when Enter is double-tapped", async () => {
    // The confirm button beside the field is disabled while the write is out;
    // Enter went straight past that. The second call would carry the SAME source
    // name, and this is what Rust answers it with once the first move has
    // happened (`sessions_ipc.rs`) — a refusal painted over a rename that worked.
    const refused =
      "there is no template at _template/interview in this zone, so there is " +
      "nothing to rename.";
    let landed: (id: string) => void = () => {};
    let sent = 0;
    mockRename.mockImplementation(() => {
      sent += 1;
      return sent === 1
        ? new Promise<string>((resolve) => {
            landed = resolve;
          })
        : Promise.reject({ message: refused });
    });
    const { onChanged } = open([namedTemplate()]);

    fireEvent.click(
      await screen.findByRole("button", { name: `${SESSION_TEMPLATE_RENAME} interview` }),
    );
    const field = screen.getByLabelText(SESSION_TEMPLATE_RENAME_NAME_LABEL);
    fireEvent.change(field, { target: { value: "Kick Off" } });
    fireEvent.keyDown(field, { key: "Enter" });
    fireEvent.keyDown(field, { key: "Enter" });

    expect(mockRename).toHaveBeenCalledTimes(1);
    // Let the one that was sent land, and check the consequence: the rename that
    // worked is not covered by a refusal about a source that had already moved.
    landed("_template/kick-off");
    await waitFor(() => expect(onChanged).toHaveBeenCalledTimes(1));
    expect(screen.queryByRole("status")).not.toBeInTheDocument();
  });
});

describe("SessionTemplates tree", () => {
  /** Matrix row 16: the payload is flat with the paths in it; the room reads them. */
  it("groups a nested file under its folder, labelled by basename, and folds it away", async () => {
    mockEntries.mockResolvedValue([
      entry("60-sessions/_template/interview/prompts/hand-off.md", "prompts/hand-off.md"),
      entry("60-sessions/_template/interview/README.md", "README.md"),
    ]);
    open([namedTemplate()]);

    const tree = await screen.findByRole("tree", { name: "interview" });
    const rows = within(tree).getAllByRole("treeitem");
    // The folder the path implied, then the file inside it, then the root file —
    // the shell's own order, with the folder standing where its newest file was.
    expect(rows.map((row) => row.getAttribute("aria-label"))).toEqual([
      "prompts",
      "hand-off.md",
      "README.md",
    ]);
    expect(rows.map((row) => row.getAttribute("aria-level"))).toEqual(["1", "2", "1"]);
    // The label is the BASENAME: the folder above it says the rest, which is what
    // a tree is for. The flat list said `prompts/hand-off.md` on one line.
    expect(within(tree).getByRole("button", { name: "hand-off.md" })).toBeInTheDocument();

    // Collapsing the folder takes its subtree with it.
    fireEvent.click(within(tree).getByRole("button", { name: "prompts" }));
    expect(within(tree).queryByRole("button", { name: "hand-off.md" })).not.toBeInTheDocument();
    expect(within(tree).getByRole("button", { name: "README.md" })).toBeInTheDocument();
  });

  /**
   * Matrix row 12, from the room's side: the template itself is not a row, so
   * there is no Rename or Delete in this tree that could be aimed at the template
   * root. That verb is the section heading's, and Rust refuses an empty `rel`
   * anyway (`template::entry_rel`).
   */
  it("has no row for the template itself", async () => {
    mockEntries.mockResolvedValue([
      entry("60-sessions/_template/interview/README.md", "README.md"),
    ]);
    open([namedTemplate()]);

    const tree = await screen.findByRole("tree", { name: "interview" });
    expect(within(tree).getAllByRole("treeitem")).toHaveLength(1);
    expect(
      within(tree).queryByLabelText(`${SESSION_TEMPLATE_ENTRY_DELETE} interview`),
    ).not.toBeInTheDocument();
  });

  /**
   * Matrix row 15, as a claim about the ROOM. The fixture now carries a
   * `.DS_Store`, because the previous one carried none: both assertions passed
   * over a room that would have rendered every entry it was handed, dotfiles
   * included, which is the one test in the story that survived its own feature.
   *
   * Rust drops them first — `pattern_files` skips every dotfile except
   * `.gitkeep` and `no_entry_verb_can_name_a_dotfile` refuses to name one — and
   * the room asks again anyway, because every verb a row carries is refused for a
   * dotfile and a row nothing can act on is a row not to draw.
   */
  it("shows no dotfile and offers no verb naming one", async () => {
    mockEntries.mockResolvedValue([
      entry("60-sessions/_template/README.md", "README.md"),
      entry("60-sessions/_template/.DS_Store", ".DS_Store"),
      entry("60-sessions/_template/.hidden/notes.md", ".hidden/notes.md"),
    ]);
    open([zoneTemplate()]);
    await screen.findByRole("button", { name: "README.md" });

    // The room drew what it should and nothing else — one row, for the one entry
    // a verb here can address.
    const tree = screen.getByRole("tree", { name: "Zone template" });
    expect(
      within(tree)
        .getAllByRole("treeitem")
        .map((row) => row.getAttribute("aria-label")),
    ).toEqual(["README.md"]);
    expect(screen.queryByText(/DS_Store/)).not.toBeInTheDocument();
    for (const button of screen.getAllByRole("button")) {
      const named = `${button.getAttribute("aria-label") ?? ""} ${button.textContent ?? ""}`;
      expect(named).not.toContain("DS_Store");
    }
    // A dotted DIRECTORY takes its subtree with it, rather than leaving a folder
    // row nothing can rename.
    expect(within(tree).queryByRole("treeitem", { name: "notes.md" })).not.toBeInTheDocument();
  });

  /**
   * The P1 of story 50.2's review, from the room's side: `New folder` made a
   * directory the room could not draw, because the listing sent files only and
   * the tree invented folders from the ancestors of file paths. An empty one
   * named no file, so it had no row — and a row that is not there carries no
   * Rename and no Delete, which is the complaint the story was written to fix.
   */
  it("draws an EMPTY folder as a row, carrying the verbs that can undo it", async () => {
    mockEntries.mockResolvedValue([
      folder("60-sessions/_template/interview/artifacts", "artifacts"),
    ]);
    open([namedTemplate()]);

    const tree = await screen.findByRole("tree", { name: "interview" });
    const rows = within(tree).getAllByRole("treeitem");
    expect(rows.map((row) => row.getAttribute("aria-label"))).toEqual(["artifacts"]);
    expect(rows[0]).toHaveAttribute("aria-expanded", "true");
    // And the section does not call itself empty over a folder that is in it.
    expect(screen.queryByText(SESSION_TEMPLATES_NO_FILES)).not.toBeInTheDocument();

    // Renameable, by the folder's own template-relative path…
    fireEvent.click(within(tree).getByLabelText(`${SESSION_TEMPLATE_ENTRY_RENAME} artifacts`));
    fireEvent.change(screen.getByLabelText(SESSION_TEMPLATE_ENTRY_RENAME_LABEL), {
      target: { value: "Outputs" },
    });
    fireEvent.click(screen.getByRole("button", { name: SESSION_TEMPLATE_RENAME_CONFIRM }));
    await waitFor(() =>
      expect(mockRenameEntry).toHaveBeenCalledWith("tgdrive", "interview", "artifacts", "Outputs"),
    );

    // …and deletable, through the confirmation that names it.
    fireEvent.click(within(tree).getByLabelText(`${SESSION_TEMPLATE_ENTRY_DELETE} artifacts`));
    const dialog = await screen.findByRole("alertdialog");
    expect(dialog).toHaveTextContent("artifacts");
    fireEvent.click(within(dialog).getByRole("button", { name: SESSION_TEMPLATE_ENTRY_DELETE }));
    await waitFor(() =>
      expect(mockDeleteEntry).toHaveBeenCalledWith("tgdrive", "interview", "artifacts"),
    );
  });

  /**
   * The room is keyboard-navigable by construction — a roving tabindex over
   * ArrowUp/Down/Left/Right, Home, End and Enter — and every test in this file
   * passed with the whole `onKeyDown` replaced by an empty function. These two
   * are what make that mutation red.
   *
   * This one is the reviewer's sequence, and it covers three things in one press
   * chain: ArrowLeft from a nested row walks to the ancestor, ArrowLeft on the
   * open folder collapses it, and `visibleRows` takes the whole subtree away.
   */
  it("walks ArrowLeft to the folder above a row, then folds it away", async () => {
    mockEntries.mockResolvedValue([
      folder("60-sessions/_template/interview/prompts", "prompts"),
      entry("60-sessions/_template/interview/prompts/hand-off.md", "prompts/hand-off.md"),
      entry("60-sessions/_template/interview/README.md", "README.md"),
    ]);
    open([namedTemplate()]);

    const tree = await screen.findByRole("tree", { name: "interview" });
    const nested = within(tree).getByRole("treeitem", { name: "hand-off.md" });
    nested.focus();

    fireEvent.keyDown(nested, { key: "ArrowLeft" });
    expect(within(tree).getByRole("treeitem", { name: "prompts" })).toHaveFocus();

    fireEvent.keyDown(within(tree).getByRole("treeitem", { name: "prompts" }), {
      key: "ArrowLeft",
    });
    expect(within(tree).queryByRole("treeitem", { name: "hand-off.md" })).not.toBeInTheDocument();
    const folded = within(tree).getByRole("treeitem", { name: "prompts" });
    expect(folded).toHaveFocus();
    expect(folded).toHaveAttribute("aria-expanded", "false");
    // The roving tabindex went with the focus: exactly one row is in the tab
    // order, and it is the row the person is standing on.
    expect(folded).toHaveAttribute("tabindex", "0");
    expect(within(tree).getByRole("treeitem", { name: "README.md" })).toHaveAttribute(
      "tabindex",
      "-1",
    );
  });

  it("steps down, jumps to the ends, unfolds with ArrowRight and opens with Enter", async () => {
    const readme = "60-sessions/_template/interview/README.md";
    mockEntries.mockResolvedValue([
      folder("60-sessions/_template/interview/prompts", "prompts"),
      entry("60-sessions/_template/interview/prompts/hand-off.md", "prompts/hand-off.md"),
      entry(readme, "README.md"),
    ]);
    open([namedTemplate()]);

    const tree = await screen.findByRole("tree", { name: "interview" });
    const row = (name: string) => within(tree).getByRole("treeitem", { name });
    const before = opened();
    row("prompts").focus();

    fireEvent.keyDown(row("prompts"), { key: "ArrowDown" });
    expect(row("hand-off.md")).toHaveFocus();
    fireEvent.keyDown(row("hand-off.md"), { key: "End" });
    expect(row("README.md")).toHaveFocus();
    fireEvent.keyDown(row("README.md"), { key: "Home" });
    expect(row("prompts")).toHaveFocus();

    // Enter toggles a folder rather than opening anything…
    fireEvent.keyDown(row("prompts"), { key: "Enter" });
    expect(within(tree).queryByRole("treeitem", { name: "hand-off.md" })).not.toBeInTheDocument();
    expect(opened()).toEqual(before);

    // …ArrowRight on a folded folder opens it rather than stepping past it…
    fireEvent.keyDown(row("prompts"), { key: "ArrowRight" });
    expect(row("hand-off.md")).toBeInTheDocument();
    expect(row("prompts")).toHaveFocus();
    // …and on an open one it walks into it.
    fireEvent.keyDown(row("prompts"), { key: "ArrowRight" });
    expect(row("hand-off.md")).toHaveFocus();

    // Enter on a file is the press that opens it, at Rust's own subpath.
    fireEvent.keyDown(row("README.md"), { key: "Enter" });
    expect(opened()).toEqual({ kind: "file", profileId: "tgdrive", relativePath: readme });
  });
});

describe("templateTree", () => {
  it("keeps the shell's order and gives a folder the place of its newest file", () => {
    const rows = templateTree([
      entry("z/_template/prompts/hand-off.md", "prompts/hand-off.md"),
      entry("z/_template/README.md", "README.md"),
      entry("z/_template/prompts/kick-off.md", "prompts/kick-off.md"),
    ]);

    // `prompts/` comes first because the file touched last is in it — the shell
    // answers newest first, and this re-sorts nothing.
    expect(rows.map((row) => row.relPath)).toEqual([
      "prompts",
      "prompts/hand-off.md",
      "prompts/kick-off.md",
      "README.md",
    ]);
    expect(rows[0]).toMatchObject({ isDir: true, entry: null, name: "prompts", parent: "" });
    expect(rows[1]).toMatchObject({
      isDir: false,
      name: "hand-off.md",
      parent: "prompts",
      depth: 2,
    });
  });

  it("gives an empty directory a row of its own", () => {
    // The P1's unit: an empty folder names no file, so an ancestor walk over file
    // paths produced nothing at all for it — no row, and therefore no verb.
    const rows = templateTree([
      folder("z/_template/artifacts", "artifacts"),
      folder("z/_template/workspace", "workspace"),
    ]);

    expect(rows.map((row) => row.relPath)).toEqual(["artifacts", "workspace"]);
    expect(rows[0]).toMatchObject({ isDir: true, depth: 1, parent: "", name: "artifacts" });
    // With the payload underneath it, so the row is Rust's answer rather than a
    // shape this file inferred.
    expect(rows[0].entry).not.toBeNull();
  });

  it("makes one row of a directory the payload named and a file implied", () => {
    // The shell orders newest-first, so a folder can arrive AFTER the file inside
    // it. That must not be two rows, and the folder must keep the place the order
    // gave it while still carrying the entry Rust listed.
    const rows = templateTree([
      entry("z/_template/prompts/hand-off.md", "prompts/hand-off.md"),
      folder("z/_template/prompts", "prompts"),
      entry("z/_template/README.md", "README.md"),
    ]);

    expect(rows.map((row) => row.relPath)).toEqual(["prompts", "prompts/hand-off.md", "README.md"]);
    expect(rows[0]).toMatchObject({ isDir: true, name: "prompts", depth: 1 });
    expect(rows[0].entry?.subpath).toBe("z/_template/prompts");
  });

  it("drops a dotfile and everything under a dotted folder", () => {
    const rows = templateTree([
      entry("z/_template/README.md", "README.md"),
      entry("z/_template/.DS_Store", ".DS_Store"),
      folder("z/_template/.git", ".git"),
      entry("z/_template/.git/config", ".git/config"),
    ]);

    expect(rows.map((row) => row.relPath)).toEqual(["README.md"]);
  });
});

describe("SessionTemplates entry verbs", () => {
  /** Matrix row 3. */
  it("makes a file at the path typed and opens what Rust answered", async () => {
    const subpath = "60-sessions/_template/interview/notes.md";
    mockFileNew.mockResolvedValue(subpath);
    const { onChanged } = open([namedTemplate()]);
    await screen.findByText(SESSION_TEMPLATES_NO_FILES);

    fireEvent.click(screen.getByRole("button", { name: SESSION_TEMPLATE_NEW_FILE }));
    fireEvent.change(screen.getByLabelText(SESSION_TEMPLATE_NEW_FILE_LABEL), {
      target: { value: "notes.md" },
    });
    fireEvent.click(screen.getByRole("button", { name: SESSION_TEMPLATE_NEW_CONFIRM }));

    // The template's own name, verbatim, and the path inside it — nothing here
    // joins a zone subfolder onto anything (AD-65).
    await waitFor(() =>
      expect(mockFileNew).toHaveBeenCalledWith("tgdrive", "interview", "notes.md"),
    );
    // Row 14: the room re-reads through the pane's nonce after the write.
    await waitFor(() => expect(onChanged).toHaveBeenCalled());
    // And the new file opens, at the subpath Rust answered with: the reason to
    // make a file in a template is to write in it.
    expect(opened()).toEqual({ kind: "file", profileId: "tgdrive", relativePath: subpath });
  });

  /**
   * Matrix row 4: the nested path travels as typed, and the folder in front of the
   * filename is one Rust addresses rather than mints — a created parent would be
   * spelled verbatim where *New folder* folds the same words.
   */
  it("sends a nested path through untouched", async () => {
    open([namedTemplate()]);
    await screen.findByText(SESSION_TEMPLATES_NO_FILES);

    fireEvent.click(screen.getByRole("button", { name: SESSION_TEMPLATE_NEW_FILE }));
    fireEvent.change(screen.getByLabelText(SESSION_TEMPLATE_NEW_FILE_LABEL), {
      target: { value: "refs/inputs.md" },
    });
    fireEvent.click(screen.getByRole("button", { name: SESSION_TEMPLATE_NEW_CONFIRM }));

    await waitFor(() =>
      expect(mockFileNew).toHaveBeenCalledWith("tgdrive", "interview", "refs/inputs.md"),
    );
  });

  /**
   * Matrix row 6, on the zone's own template — which has no name argument — and
   * the press the P1 was found under. This used to stop at the wire call and the
   * untouched panel, so it stayed green over a verb whose result the room could
   * not draw. The re-read is the pane's (`onChanged` → new patterns identity), so
   * it is performed here the way the pane performs it.
   */
  it("makes a folder in the zone's own template with no name, and shows the row it made", async () => {
    const { onChanged, view } = open([zoneTemplate()]);
    await screen.findByText(SESSION_TEMPLATES_NO_FILES);
    const before = opened();

    fireEvent.click(screen.getByRole("button", { name: SESSION_TEMPLATE_NEW_FOLDER }));
    fireEvent.change(screen.getByLabelText(SESSION_TEMPLATE_NEW_FOLDER_LABEL), {
      target: { value: "artifacts" },
    });
    fireEvent.click(screen.getByRole("button", { name: SESSION_TEMPLATE_NEW_CONFIRM }));

    await waitFor(() => expect(mockDirNew).toHaveBeenCalledWith("tgdrive", undefined, "artifacts"));
    await waitFor(() => expect(onChanged).toHaveBeenCalled());
    // A folder has nothing to open, so the panel is left where it was.
    expect(opened()).toEqual(before);
    expect(mockFileNew).not.toHaveBeenCalled();

    // And the folder is IN the room after the re-read the press asked for: an
    // empty one used to answer with no row at all, which left it unnameable,
    // undeletable and invisible on the drive.
    mockEntries.mockResolvedValue([folder("60-sessions/_template/artifacts", "artifacts")]);
    view.rerender(
      <SessionTemplates
        rootId="tgdrive"
        patterns={[zoneTemplate()]}
        onInstallTemplate={vi.fn(async () => null)}
        installing={false}
        onChanged={onChanged}
        nowMs={NOW}
      />,
    );

    const tree = await screen.findByRole("tree", { name: "Zone template" });
    expect(within(tree).getByRole("treeitem", { name: "artifacts" })).toBeInTheDocument();
    expect(
      within(tree).getByLabelText(`${SESSION_TEMPLATE_ENTRY_DELETE} artifacts`),
    ).toBeInTheDocument();
  });

  it("is inert with an empty path — Rust is never asked what an empty one means", async () => {
    open([namedTemplate()]);
    await screen.findByText(SESSION_TEMPLATES_NO_FILES);

    fireEvent.click(screen.getByRole("button", { name: SESSION_TEMPLATE_NEW_FILE }));
    fireEvent.change(screen.getByLabelText(SESSION_TEMPLATE_NEW_FILE_LABEL), {
      target: { value: "   " },
    });
    fireEvent.click(screen.getByRole("button", { name: SESSION_TEMPLATE_NEW_CONFIRM }));

    expect(mockFileNew).not.toHaveBeenCalled();
  });

  /** Matrix rows 5 and 13: Rust's sentence, in the room's one live region. */
  it("says a refused create in Rust's words and keeps what was typed", async () => {
    const said =
      "../escape.md is not a path inside this template. keeper composes a template's paths " +
      "from the template's own directory and will not follow one back out of it.";
    mockFileNew.mockRejectedValue({ message: said });
    const { onChanged } = open([namedTemplate()]);
    await screen.findByText(SESSION_TEMPLATES_NO_FILES);

    fireEvent.click(screen.getByRole("button", { name: SESSION_TEMPLATE_NEW_FILE }));
    fireEvent.change(screen.getByLabelText(SESSION_TEMPLATE_NEW_FILE_LABEL), {
      target: { value: "../escape.md" },
    });
    fireEvent.click(screen.getByRole("button", { name: SESSION_TEMPLATE_NEW_CONFIRM }));

    expect(await screen.findByRole("status")).toHaveTextContent(said);
    // Nothing was re-read, and the path stays put for a correction.
    expect(onChanged).not.toHaveBeenCalled();
    expect(screen.getByLabelText(SESSION_TEMPLATE_NEW_FILE_LABEL)).toHaveValue("../escape.md");
  });

  /** Matrix row 7. */
  it("renames one file by its template-relative path, seeded with the name it has", async () => {
    mockEntries.mockResolvedValue([entry("60-sessions/_template/interview/about.md", "about.md")]);
    const { onChanged } = open([namedTemplate()]);

    fireEvent.click(await screen.findByLabelText(`${SESSION_TEMPLATE_ENTRY_RENAME} about.md`));
    const field = screen.getByLabelText(SESSION_TEMPLATE_ENTRY_RENAME_LABEL);
    // Most renames are a small edit to the name it has — including its extension,
    // which is part of what may be edited.
    expect(field).toHaveValue("about.md");
    fireEvent.change(field, { target: { value: "Record.md" } });
    fireEvent.click(screen.getByRole("button", { name: SESSION_TEMPLATE_RENAME_CONFIRM }));

    // The name is sent as typed: the fold is Rust's, and a slug composed here
    // would be the second namer.
    await waitFor(() =>
      expect(mockRenameEntry).toHaveBeenCalledWith("tgdrive", "interview", "about.md", "Record.md"),
    );
    await waitFor(() => expect(onChanged).toHaveBeenCalled());
  });

  /** Matrix row 9: the same verb on a folder row, addressed by the folder's path. */
  it("renames a folder row by the path the tree implied", async () => {
    mockEntries.mockResolvedValue([
      entry("60-sessions/_template/interview/refs/inputs.md", "refs/inputs.md"),
    ]);
    open([namedTemplate()]);

    fireEvent.click(await screen.findByLabelText(`${SESSION_TEMPLATE_ENTRY_RENAME} refs`));
    fireEvent.change(screen.getByLabelText(SESSION_TEMPLATE_ENTRY_RENAME_LABEL), {
      target: { value: "References" },
    });
    fireEvent.click(screen.getByRole("button", { name: SESSION_TEMPLATE_RENAME_CONFIRM }));

    await waitFor(() =>
      expect(mockRenameEntry).toHaveBeenCalledWith("tgdrive", "interview", "refs", "References"),
    );
  });

  /** Matrix row 8: a collision is Rust's refusal, and it changes nothing here. */
  it("keeps the rename form open on a refusal and re-reads nothing", async () => {
    const said =
      "record.md is already in this template — pick another name. keeper will not write over " +
      "a file or a folder somebody put there.";
    mockRenameEntry.mockRejectedValue({ message: said });
    mockEntries.mockResolvedValue([entry("60-sessions/_template/interview/about.md", "about.md")]);
    const { onChanged } = open([namedTemplate()]);

    fireEvent.click(await screen.findByLabelText(`${SESSION_TEMPLATE_ENTRY_RENAME} about.md`));
    fireEvent.change(screen.getByLabelText(SESSION_TEMPLATE_ENTRY_RENAME_LABEL), {
      target: { value: "Record.md" },
    });
    fireEvent.click(screen.getByRole("button", { name: SESSION_TEMPLATE_RENAME_CONFIRM }));

    expect(await screen.findByRole("status")).toHaveTextContent(said);
    expect(onChanged).not.toHaveBeenCalled();
    expect(screen.getByLabelText(SESSION_TEMPLATE_ENTRY_RENAME_LABEL)).toHaveValue("Record.md");
  });

  /** Matrix rows 10 and 11, through the confirmation that promises the trash. */
  it("deletes an entry only after a confirmation that says where it goes", async () => {
    mockEntries.mockResolvedValue([
      entry("60-sessions/_template/interview/refs/inputs.md", "refs/inputs.md"),
    ]);
    const { onChanged } = open([namedTemplate()]);

    // The folder row, so this covers the recursive half: a `TrashDir` takes the
    // directory whole, which is what makes it recoverable whole.
    fireEvent.click(await screen.findByLabelText(`${SESSION_TEMPLATE_ENTRY_DELETE} refs`));
    expect(mockDeleteEntry).not.toHaveBeenCalled();

    const dialog = await screen.findByRole("alertdialog");
    expect(dialog).toHaveTextContent(SESSION_TEMPLATE_DELETE_TITLE);
    // Which entry, and the three things a person needs to know before pressing it.
    expect(dialog).toHaveTextContent("refs");
    expect(dialog).toHaveTextContent(SESSION_TEMPLATE_DELETE_BODY);
    fireEvent.click(within(dialog).getByRole("button", { name: SESSION_TEMPLATE_ENTRY_DELETE }));

    await waitFor(() =>
      expect(mockDeleteEntry).toHaveBeenCalledWith("tgdrive", "interview", "refs"),
    );
    await waitFor(() => expect(onChanged).toHaveBeenCalled());
  });

  it("asks Rust nothing when the delete confirmation is cancelled", async () => {
    mockEntries.mockResolvedValue([entry("60-sessions/_template/interview/about.md", "about.md")]);
    open([namedTemplate()]);

    fireEvent.click(await screen.findByLabelText(`${SESSION_TEMPLATE_ENTRY_DELETE} about.md`));
    const dialog = await screen.findByRole("alertdialog");
    fireEvent.click(within(dialog).getByRole("button", { name: "Cancel" }));

    await waitFor(() => expect(screen.queryByRole("alertdialog")).not.toBeInTheDocument());
    expect(mockDeleteEntry).not.toHaveBeenCalled();
  });

  it("says a refused delete in Rust's words", async () => {
    const said =
      "there is nothing at about.md in this template — it moved or was removed. Read the " +
      "list again rather than trying again.";
    mockDeleteEntry.mockRejectedValue({ message: said });
    mockEntries.mockResolvedValue([entry("60-sessions/_template/interview/about.md", "about.md")]);
    const { onChanged } = open([namedTemplate()]);

    fireEvent.click(await screen.findByLabelText(`${SESSION_TEMPLATE_ENTRY_DELETE} about.md`));
    const dialog = await screen.findByRole("alertdialog");
    fireEvent.click(within(dialog).getByRole("button", { name: SESSION_TEMPLATE_ENTRY_DELETE }));

    expect(await screen.findByRole("status")).toHaveTextContent(said);
    expect(onChanged).not.toHaveBeenCalled();
  });
});
