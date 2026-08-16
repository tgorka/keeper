import { fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { SessionPatternVm, SessionTemplateEntryVm } from "@/lib/ipc/client";

// The room reads through one command and writes through one; the create verb is
// the pane's, injected. Stubbing at the IPC boundary leaves the real component —
// the real refusals, the real copy, the real path handling — under test.
vi.mock("@/lib/ipc/client", () => ({
  sessionsTemplateEntries: vi.fn(),
  sessionsTemplateRename: vi.fn(),
}));

import { SESSION_PATTERN_INSTALL_LABEL } from "@/components/sessions/session-pattern-picker";
import {
  SESSION_TEMPLATE_FILE_TESTID,
  SESSION_TEMPLATE_RENAME,
  SESSION_TEMPLATE_RENAME_CONFIRM,
  SESSION_TEMPLATE_RENAME_NAME_LABEL,
  SESSION_TEMPLATE_SECTION_TESTID,
  SESSION_TEMPLATES_EMPTY,
  SESSION_TEMPLATES_LOADING,
  SESSION_TEMPLATES_NEW,
  SESSION_TEMPLATES_NEW_NAME_LABEL,
  SESSION_TEMPLATES_NO_FILES,
  SessionTemplates,
  sessionTemplateTaken,
} from "@/components/sessions/session-templates";
import { sessionsTemplateEntries, sessionsTemplateRename } from "@/lib/ipc/client";
import { activePanel, panelsStore, resetPanelsStoreForTest } from "@/lib/stores/panels";

const mockEntries = vi.mocked(sessionsTemplateEntries);
const mockRename = vi.mocked(sessionsTemplateRename);

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
  return { subpath, name, mtimeMs: NOW - 60_000 };
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
    expect(await zone.findByRole("button", { name: /AGENTS\.md/ })).toBeInTheDocument();
    expect(zone.queryByRole("button", { name: /questions\.md/ })).not.toBeInTheDocument();
    const named = within(screen.getByTestId(namedId));
    expect(named.getByRole("button", { name: /questions\.md/ })).toBeInTheDocument();
    expect(named.queryByRole("button", { name: /AGENTS\.md/ })).not.toBeInTheDocument();
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
    expect(await screen.findByRole("button", { name: /questions\.md/ })).toBeInTheDocument();
    expect(screen.queryByText(SESSION_TEMPLATES_NO_FILES)).not.toBeInTheDocument();
  });

  it("opens a file row at the EXACT subpath Rust returned", async () => {
    // The literal string the shell composed. Nothing here joins the zone's
    // subfolder onto it — the assertion is the AD-65 guard.
    const said = "60-sessions/_template/interview/questions.md";
    mockEntries.mockResolvedValue([entry(said, "questions.md")]);
    open([namedTemplate()]);

    fireEvent.click(await screen.findByRole("button", { name: /questions\.md/ }));

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
    await screen.findByRole("button", { name: /AGENTS\.md/ });

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
    expect(await screen.findByRole("button", { name: /CLAUDE\.md/ })).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: /AGENTS\.md/ })).not.toBeInTheDocument();
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
    expect(await zone.findByRole("button", { name: /AGENTS\.md/ })).toBeInTheDocument();
    expect(zone.queryByText(SESSION_TEMPLATES_LOADING)).not.toBeInTheDocument();

    // And the refused one says so in Rust's words, under its own heading, rather
    // than sitting on "Reading…" over a read that already stopped.
    const refused = within(screen.getByTestId(refusedId));
    expect(refused.getByText(said)).toBeInTheDocument();
    expect(refused.queryByText(SESSION_TEMPLATES_LOADING)).not.toBeInTheDocument();
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
