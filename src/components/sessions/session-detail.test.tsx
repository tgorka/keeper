import { render, screen, waitFor, within } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type {
  SessionDetailVm,
  SessionEntryVm,
  SessionReferencesVm,
  SessionTreeVm,
} from "@/lib/ipc/client";

const sessionsDetail = vi.fn();
const sessionsTree = vi.fn();
const sessionsRefs = vi.fn();
const listenSessionsChanged = vi.fn();
const syncOpenEntry = vi.fn();
const revealPath = vi.fn();
vi.mock("@/lib/ipc/client", () => ({
  sessionsDetail: (rootId: unknown, sessionId: unknown) => sessionsDetail(rootId, sessionId),
  sessionsTree: (rootId: unknown, sessionId: unknown) => sessionsTree(rootId, sessionId),
  sessionsRefs: (rootId: unknown, sessionId: unknown) => sessionsRefs(rootId, sessionId),
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
  SESSION_DETAIL_PROPERTIES_HEADING,
  SESSION_DETAIL_WORKSPACE_CAVEAT,
  SessionDetail,
} from "@/components/sessions/session-detail";
import {
  SESSION_REFS_ALL_RESOLVED,
  SESSION_REFS_EMPTY,
  SESSION_REFS_HEADING,
} from "@/components/sessions/session-refs";
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

beforeEach(() => {
  sessionsDetail.mockResolvedValue(detail());
  sessionsTree.mockResolvedValue(tree());
  sessionsRefs.mockResolvedValue(refs());
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
});
