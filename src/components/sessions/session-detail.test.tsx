import { cleanup, render, screen, waitFor, within } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type {
  SessionDetailVm,
  SessionEntryVm,
  SessionReferencesVm,
  SessionSpaceVm,
  SessionTreeVm,
} from "@/lib/ipc/client";

const sessionsDetail = vi.fn();
const sessionsTree = vi.fn();
const sessionsRefs = vi.fn();
const sessionsSpaces = vi.fn();
const sessionsSpaceFiles = vi.fn();
const listenSessionsChanged = vi.fn();
const syncOpenEntry = vi.fn();
const revealPath = vi.fn();
vi.mock("@/lib/ipc/client", () => ({
  sessionsDetail: (rootId: unknown, sessionId: unknown) => sessionsDetail(rootId, sessionId),
  sessionsTree: (rootId: unknown, sessionId: unknown) => sessionsTree(rootId, sessionId),
  sessionsRefs: (rootId: unknown, sessionId: unknown) => sessionsRefs(rootId, sessionId),
  sessionsSpaces: (rootId: unknown) => sessionsSpaces(rootId),
  sessionsSpaceFiles: (rootId: unknown, sessionId: unknown) =>
    sessionsSpaceFiles(rootId, sessionId),
  // The spaces section's write path, unreachable from these cases but imported
  // by the module under test — a mock factory that omits an export makes the
  // import itself throw, not the call.
  sessionsSpaceDelete: vi.fn(),
  sessionsSpacesRestore: vi.fn(),
  sessionsSpaceSave: vi.fn(),
  notesSpaceTerms: vi.fn(),
  // The file verbs, for the same reason: the Files heading imports all three
  // and the tree imports the fourth (FR-262).
  sessionsFileNew: vi.fn(),
  sessionsFileNewKind: vi.fn(),
  sessionsFileDelete: vi.fn(),
  sessionsLogToday: vi.fn(),
  listenSessionsChanged: (cb: unknown) => listenSessionsChanged(cb),
  syncOpenEntry: (id: unknown, subpath: unknown) => syncOpenEntry(id, subpath),
  revealPath: (path: unknown) => revealPath(path),
}));

// The refs widget's one external-open path, reached from this surface only if
// somebody presses a link row — mocked because the real plugin talks to Tauri.
vi.mock("@tauri-apps/plugin-opener", () => ({ openUrl: vi.fn(async () => {}) }));

import {
  SESSION_DETAIL_FILES_HEADING,
  SESSION_DETAIL_LOG_HEADING,
  SESSION_DETAIL_OPEN_ABOUT_LABEL,
  SESSION_DETAIL_OPEN_README_LABEL,
  SESSION_DETAIL_PROPERTIES_HEADING,
  SESSION_DETAIL_UNFILED_HEADING,
  SESSION_DETAIL_UNFILED_HINT,
  SESSION_DETAIL_WORKSPACE_CAVEAT,
  SessionDetail,
} from "@/components/sessions/session-detail";
import {
  SESSION_REFS_ALL_RESOLVED,
  SESSION_REFS_EMPTY,
  SESSION_REFS_HEADING,
} from "@/components/sessions/session-refs";
import { SESSION_SPACES_EMPTY, SESSION_SPACES_HEADING } from "@/components/sessions/session-spaces";
import { SESSION_TREE_EMPTY } from "@/components/sessions/session-tree";
import { panelsStore } from "@/lib/stores/panels";

const NOW = Date.now();

/** The fence's own sentence, as Rust composes it (AD-113) — abbreviated here. */
const LOCK_SENTENCE =
  "60-sessions/active/2026-08-10-keeper/workspace is inside a session's workspace — scratch that is not versioned, not synced, and dies with the session.";

function detail(over: Partial<SessionDetailVm> = {}): SessionDetailVm {
  return {
    id: "01J5AAAAAAAAAAAAAAAAAAAAAA",
    path: "active/2026-08-10-keeper",
    title: "keeper — rolling work session",
    status: "active",
    archivedYear: null,
    pinned: true,
    tags: ["project/keeper"],
    properties: [
      { key: "tool", value: "Claude Code (Opus 5)" },
      { key: "goal", value: "keeper the app and tgdrive the data" },
    ],
    continues: [],
    continuedBy: ["01J6BBBBBBBBBBBBBBBBBBBBBB"],
    summary: "State as of opening. Two tracks.",
    log: [
      { date: "2026-08-11", title: "shipped 0.6.5", body: "Release drafted; DMG attached." },
      { date: "2026-08-10", title: "opened", body: "" },
    ],
    // The folder contract, which is what every case in this file exercises: a
    // README-backed session with no task files and nothing unfiled. The flat
    // contract's own rendering is tested where it is built, not by widening
    // every fixture here into a shape it never has to draw.
    shape: "folder",
    unfiled: [],
    tasks: [],
    ...over,
  };
}

function entry(over: Partial<SessionEntryVm> & Pick<SessionEntryVm, "name">): SessionEntryVm {
  const relPath = over.relPath ?? over.name;
  return {
    relPath,
    parent: "",
    depth: 1,
    isDir: false,
    subpath: `60-sessions/active/2026-08-10-keeper/${relPath}`,
    absolutePath: `/Users/tgorka/tgdrive/60-sessions/active/2026-08-10-keeper/${relPath}`,
    size: { bytes: 2048, label: "2.0 kB" },
    mtimeMs: NOW - 60_000,
    sync: { status: "synced", detail: null },
    locked: null,
    // A directory is never deletable from this tree (FR-262); a file here is,
    // unless a case says otherwise.
    undeletable: over.isDir === true ? "Removing a folder is a Finder job." : null,
    ...over,
  };
}

function tree(over: Partial<SessionTreeVm> = {}): SessionTreeVm {
  return {
    truncated: false,
    entries: [
      entry({ name: "artifacts", isDir: true, size: null }),
      entry({
        name: "release-notes.md",
        relPath: "artifacts/release-notes.md",
        parent: "artifacts",
        depth: 2,
      }),
      entry({ name: "workspace", isDir: true, size: null, locked: LOCK_SENTENCE }),
      entry({
        name: "iter-3.md",
        relPath: "workspace/iter-3.md",
        parent: "workspace",
        depth: 2,
        locked: LOCK_SENTENCE,
      }),
    ],
    ...over,
  };
}

function refs(over: Partial<SessionReferencesVm> = {}): SessionReferencesVm {
  return {
    missing: 0,
    truncated: false,
    refs: [
      {
        kind: "note",
        target: "Vault as a lens",
        label: "Vault as a lens",
        source: "README.md",
        panelTarget: { kind: "note", vaultId: "tgdrive", noteId: "01JLENS" },
        url: null,
        notice: null,
      },
    ],
    ...over,
  };
}

/**
 * One space, for the cases about where the section sits.
 *
 * The default below is a zone with none, which is the ordinary state of a
 * session that predates spaces — and the state every other case in this file
 * wants, because a section listing files would put a second copy of each
 * filename on the surface and make "the tree shows X" ambiguous.
 */
function space(): SessionSpaceVm {
  return {
    id: "_spaces/log.md",
    name: "Log",
    query: "tag:log",
    sort: "modified desc",
    sortEffective: "modified desc",
    icon: null,
    defaultKey: "log",
    order: 3,
    warnings: [],
    error: null,
  };
}

beforeEach(() => {
  sessionsDetail.mockResolvedValue(detail());
  sessionsTree.mockResolvedValue(tree());
  sessionsRefs.mockResolvedValue(refs());
  sessionsSpaces.mockResolvedValue([]);
  sessionsSpaceFiles.mockResolvedValue([]);
  listenSessionsChanged.mockResolvedValue(() => {});
  panelsStore.setState(panelsStore.getInitialState(), true);
});

afterEach(() => {
  vi.clearAllMocks();
});

function mount() {
  return render(
    <SessionDetail
      rootId="tgdrive"
      subfolder="60-sessions"
      sessionId="01J5AAAAAAAAAAAAAAAAAAAAAA"
      onBack={() => {}}
    />,
  );
}

describe("SessionDetail", () => {
  it("renders the log newest first, with dates, titles and prose", async () => {
    mount();
    const log = await screen.findByRole("region", { name: SESSION_DETAIL_LOG_HEADING });
    const items = within(log).getAllByRole("listitem");
    expect(items[0]).toHaveTextContent("2026-08-11");
    expect(items[0]).toHaveTextContent("shipped 0.6.5");
    expect(items[0]).toHaveTextContent("Release drafted; DMG attached.");
    expect(items[1]).toHaveTextContent("2026-08-10");
  });

  it("shows the user-tier properties widget and the header facts", async () => {
    mount();
    const properties = await screen.findByRole("region", {
      name: SESSION_DETAIL_PROPERTIES_HEADING,
    });
    expect(within(properties).getByText("tool")).toBeInTheDocument();
    expect(within(properties).getByText("Claude Code (Opus 5)")).toBeInTheDocument();
    expect(screen.getByText("project/keeper")).toBeInTheDocument();
    expect(screen.getByText("State as of opening. Two tracks.")).toBeInTheDocument();
    // Lineage renders as chips (UX-DR89), one per direction present.
    expect(screen.getByText("continued →")).toBeInTheDocument();
  });

  it("opens a tree file through the one file target, on the subpath Rust composed", async () => {
    mount();
    const files = await screen.findByRole("region", { name: SESSION_DETAIL_FILES_HEADING });
    within(files).getByText("release-notes.md").click();
    await waitFor(() => {
      const target = panelsStore.getState().panels.find((p) => p.target?.kind === "file")?.target;
      expect(target).toMatchObject({
        kind: "file",
        profileId: "tgdrive",
        relativePath: "60-sessions/active/2026-08-10-keeper/artifacts/release-notes.md",
      });
    });
  });

  it("says the workspace caveat once, above the tree", async () => {
    mount();
    const files = await screen.findByRole("region", { name: SESSION_DETAIL_FILES_HEADING });
    expect(within(files).getByText(SESSION_DETAIL_WORKSPACE_CAVEAT)).toBeInTheDocument();
  });

  it("puts the files first and the log last, in document order", async () => {
    mount();
    await screen.findByRole("region", { name: SESSION_DETAIL_FILES_HEADING });
    // Asserting on the DOM's own order rather than on three separate presence
    // checks: "files first, log last" is a claim about sequence, and only a
    // sequence can falsify it. `compareDocumentPosition` reads the rendered
    // tree, so a reorder that satisfied every individual query but shuffled the
    // page would still fail here.
    const order = [
      SESSION_DETAIL_FILES_HEADING,
      // After the files, on the operator's own instruction: the tree is what the
      // session holds and this is a reading of it, so the contents come before
      // keeper's grouping of them.
      SESSION_SPACES_HEADING,
      SESSION_REFS_HEADING,
      SESSION_DETAIL_LOG_HEADING,
    ].map((name) => screen.getByRole("region", { name }));
    for (let index = 0; index + 1 < order.length; index += 1) {
      expect(
        order[index].compareDocumentPosition(order[index + 1]) & Node.DOCUMENT_POSITION_FOLLOWING,
      ).toBeTruthy();
    }
  });

  it("names the record by shape: about.md when flat, README when not", async () => {
    mount();
    expect(
      await screen.findByRole("button", { name: SESSION_DETAIL_OPEN_README_LABEL }),
    ).toBeInTheDocument();

    cleanup();
    sessionsDetail.mockResolvedValue(detail({ shape: "flat" }));
    mount();
    const open = await screen.findByRole("button", { name: SESSION_DETAIL_OPEN_ABOUT_LABEL });
    open.click();
    await waitFor(() => {
      const target = panelsStore.getState().panels.find((p) => p.target?.kind === "file")?.target;
      // The flat session's record is `about.md`; opening README.md would open
      // the migration's signpost instead of the session.
      expect(target).toMatchObject({
        relativePath: "60-sessions/active/2026-08-10-keeper/about.md",
      });
    });
  });

  it("shows unfiled root markdown as a nudge, and shows nothing when there is none", async () => {
    mount();
    // The clean session is the common one, and a permanent empty section on it
    // would make the notice mean nothing when it did appear.
    await screen.findByRole("region", { name: SESSION_DETAIL_FILES_HEADING });
    expect(
      screen.queryByRole("region", { name: SESSION_DETAIL_UNFILED_HEADING }),
    ).not.toBeInTheDocument();

    cleanup();
    sessionsDetail.mockResolvedValue(
      detail({ shape: "flat", unfiled: ["stray-thought.md", "pasted.md"] }),
    );
    mount();
    const unfiled = await screen.findByRole("region", { name: SESSION_DETAIL_UNFILED_HEADING });
    expect(within(unfiled).getByText("stray-thought.md")).toBeInTheDocument();
    expect(within(unfiled).getByText("pasted.md")).toBeInTheDocument();
    expect(within(unfiled).getByText(SESSION_DETAIL_UNFILED_HINT)).toBeInTheDocument();
  });

  it("re-reads ALL THREE when the changed event names this root — the agent's write moves the view", async () => {
    mount();
    await screen.findByRole("region", { name: SESSION_DETAIL_LOG_HEADING });
    expect(sessionsDetail).toHaveBeenCalledTimes(1);
    expect(sessionsTree).toHaveBeenCalledTimes(1);
    expect(sessionsRefs).toHaveBeenCalledTimes(1);
    const onChanged = listenSessionsChanged.mock.calls[0][0] as (rootId: string) => void;
    sessionsDetail.mockResolvedValue(
      detail({
        log: [
          { date: "2026-08-12", title: "agent wrote", body: "" },
          { date: "2026-08-11", title: "shipped 0.6.5", body: "" },
        ],
      }),
    );
    sessionsTree.mockResolvedValue(
      tree({ entries: [entry({ name: "notes.md", relPath: "artifacts/notes.md" })] }),
    );
    // The same write that adds a file can break a pointer — the count is a
    // projection of the files, so it has to move on the same event or it
    // becomes a stale claim that everything resolves.
    sessionsRefs.mockResolvedValue(
      refs({
        missing: 1,
        refs: [
          {
            kind: "missing",
            target: "40-media/moved.m4a",
            label: "the recording",
            source: "refs/inputs.md",
            panelTarget: null,
            url: null,
            notice: "40-media/moved.m4a: this session points at something the drive does not have",
          },
        ],
      }),
    );
    onChanged("tgdrive");
    await screen.findByText("agent wrote");
    await screen.findByText("notes.md");
    await screen.findByText("1 reference points at something that is not there.");
    // A change on ANOTHER root is not this detail's business.
    onChanged("neuradrive");
    expect(sessionsDetail).toHaveBeenCalledTimes(2);
    expect(sessionsTree).toHaveBeenCalledTimes(2);
    expect(sessionsRefs).toHaveBeenCalledTimes(2);
  });

  it("keeps the record when the tree read fails — a session with no files still has a log", async () => {
    sessionsTree.mockRejectedValue(new Error("walk failed"));
    mount();
    const log = await screen.findByRole("region", { name: SESSION_DETAIL_LOG_HEADING });
    expect(within(log).getAllByRole("listitem")).toHaveLength(2);
    expect(screen.getByText(SESSION_TREE_EMPTY)).toBeInTheDocument();
    // The record's error slot stays for a real failure to find the session.
    expect(screen.queryByRole("alert")).not.toBeInTheDocument();
  });

  it("lists what the session points at, beside what it holds", async () => {
    mount();
    const section = await screen.findByRole("region", { name: SESSION_REFS_HEADING });
    expect(within(section).getByText("Vault as a lens")).toBeInTheDocument();
    expect(within(section).getByText(SESSION_REFS_ALL_RESOLVED)).toBeInTheDocument();
    expect(sessionsRefs).toHaveBeenCalledWith("tgdrive", "01J5AAAAAAAAAAAAAAAAAAAAAA");
  });

  it("keeps the record when the refs read fails — as local a failure as the tree's", async () => {
    sessionsRefs.mockRejectedValue(new Error("scan failed"));
    mount();
    const log = await screen.findByRole("region", { name: SESSION_DETAIL_LOG_HEADING });
    expect(within(log).getAllByRole("listitem")).toHaveLength(2);
    expect(screen.getByText(SESSION_REFS_EMPTY)).toBeInTheDocument();
    expect(screen.queryByRole("alert")).not.toBeInTheDocument();
  });

  /**
   * Two reads, and they stay two (FR-261): the definitions belong to the zone
   * and change when someone edits one; the selections belong to this session and
   * change whenever any file in it does. Folding them together would re-parse
   * five queries every time an agent touches a log file.
   */
  it("reads the zone's spaces and this session's selections as two calls", async () => {
    sessionsSpaces.mockResolvedValue([space()]);
    sessionsSpaceFiles.mockResolvedValue([{ spaceId: "_spaces/log.md", files: [], error: null }]);
    mount();

    const section = await screen.findByRole("region", { name: SESSION_SPACES_HEADING });
    expect(within(section).getByText("Log")).toBeInTheDocument();
    // The zone id alone for the definitions; the session too for the selections.
    expect(sessionsSpaces).toHaveBeenCalledWith("tgdrive");
    expect(sessionsSpaceFiles).toHaveBeenCalledWith("tgdrive", "01J5AAAAAAAAAAAAAAAAAAAAAA");
  });

  /**
   * A zone with no `_spaces/` yet is the ordinary state of every session created
   * before this shipped — so the read failing must leave the record standing and
   * offer the defaults, not blank the surface.
   */
  it("keeps the record when the spaces read fails, and offers the defaults", async () => {
    sessionsSpaces.mockRejectedValue(new Error("no such directory"));
    sessionsSpaceFiles.mockRejectedValue(new Error("no such directory"));
    mount();

    const log = await screen.findByRole("region", { name: SESSION_DETAIL_LOG_HEADING });
    expect(within(log).getAllByRole("listitem")).toHaveLength(2);
    expect(screen.getByText(SESSION_SPACES_EMPTY)).toBeInTheDocument();
    expect(screen.queryByRole("alert")).not.toBeInTheDocument();
  });
});
