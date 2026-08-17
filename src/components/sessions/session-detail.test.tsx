import { act, cleanup, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type {
  SessionDetailVm,
  SessionEntryVm,
  SessionReferencesVm,
  SessionSpaceVm,
  SessionTaskVm,
  SessionTreeVm,
} from "@/lib/ipc/client";

const sessionsDetail = vi.fn();
const sessionsTree = vi.fn();
const sessionsRefs = vi.fn();
const sessionsSpaces = vi.fn();
const sessionsSpaceFiles = vi.fn();
const sessionsFileNewKind = vi.fn();
const listenSessionsChanged = vi.fn();
// The fold default the detail reads on mount (Story 49.3): named here rather
// than stubbed inline, because the restore cases below turn it on and off.
const sessionsSpacesFoldedGet = vi.fn();
// The board's one write (FR-263), named because Story 51.7 asserts it is
// reachable on a folder-shaped session — the surface where the board itself was
// absent until this story.
const sessionsTaskMove = vi.fn();
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
  // Five arguments, the fifth being the id of the space that pressed the button
  // (Story 52.5). A mock that forwarded four would drop the one argument that
  // decides where the file lands, and this suite could not see it go missing.
  sessionsFileNewKind: (
    rootId: unknown,
    sessionId: unknown,
    kind: unknown,
    title: unknown,
    spaceId: unknown,
  ) => sessionsFileNewKind(rootId, sessionId, kind, title, spaceId),
  sessionsFileDelete: vi.fn(),
  sessionsLogToday: vi.fn(),
  // And the board's one write (FR-263), imported transitively through
  // SessionBoard.
  sessionsTaskMove: (
    rootId: unknown,
    sessionId: unknown,
    rel: unknown,
    status: unknown,
    index: unknown,
  ) => sessionsTaskMove(rootId, sessionId, rel, status, index),
  // The spaces section resolves a row's note through the vault mirror and the
  // 45.18 bridge (Story 49.2), so the store and the bridge both reach this
  // module. `notesVaults` answering an empty list is a machine with no vault
  // configured, which is what every case in this file is about — the spaces
  // section is exercised in `session-spaces.test.tsx`.
  notesVaults: vi.fn(async () => []),
  notesVaultActive: vi.fn(async () => null),
  notesVaultSetActive: vi.fn(async () => undefined),
  notesTree: vi.fn(),
  listenSessionsChanged: (cb: unknown) => listenSessionsChanged(cb),
  syncOpenEntry: (id: unknown, subpath: unknown) => syncOpenEntry(id, subpath),
  sessionsSpacesFoldedGet: () => sessionsSpacesFoldedGet(),
  revealPath: (path: unknown) => revealPath(path),
}));

// The refs widget's one external-open path, reached from this surface only if
// somebody presses a link row — mocked because the real plugin talks to Tauri.
vi.mock("@tauri-apps/plugin-opener", () => ({ openUrl: vi.fn(async () => {}) }));

import {
  SESSION_BOARD_EMPTY,
  SESSION_BOARD_HEADING,
  SESSION_BOARD_MOVE_LABEL,
} from "@/components/sessions/session-board";
import {
  SESSION_DETAIL_FILES_HEADING,
  SESSION_DETAIL_LOG_HEADING,
  SESSION_DETAIL_OPEN_RECORD_LABEL,
  SESSION_DETAIL_PROPERTIES_HEADING,
  SESSION_DETAIL_WORKSPACE_CAVEAT,
  SessionDetail,
} from "@/components/sessions/session-detail";
import { SESSION_FILE_NEW_PROMPT_LABEL } from "@/components/sessions/session-file-actions";
import {
  SESSION_REFS_ALL_RESOLVED,
  SESSION_REFS_EMPTY,
  SESSION_REFS_HEADING,
} from "@/components/sessions/session-refs";
import {
  SESSION_SPACE_NEW_NOTE,
  SESSION_SPACE_UNTAGGED_KEY,
  SESSION_SPACES_EMPTY,
  SESSION_SPACES_HEADING,
  SESSION_SPACES_NO_FILES,
} from "@/components/sessions/session-spaces";
import { SESSION_TREE_EMPTY } from "@/components/sessions/session-tree";
import { writeCookie } from "@/components/ui/cookie-writer";
import { resetNotesVaultsStoreForTest } from "@/lib/stores/notes-vaults";
import { panelsStore } from "@/lib/stores/panels";
import {
  readSessionSpacesFold,
  resetSessionSpacesFoldForTest,
  SESSION_SPACES_FOLD_COOKIE,
  sessionSpacesFoldCookie,
  sessionSpacesFoldStore,
  setSpacesFoldedDefault,
  spaceFoldKey,
} from "@/lib/stores/session-spaces-fold";

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
    // README-backed session with no task files. The flat contract's own
    // rendering is tested where it is built, not by widening every fixture here
    // into a shape it never has to draw.
    //
    // There is no `unfiled` key any more (Story 52.4): what declares no kind is
    // the `Untagged` space's selection, which arrives on `sessions_space_files`
    // like every other space's.
    shape: "folder",
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
 * filename on the surface and make "the tree shows X" ambiguous. The overrides
 * are for the fold cases, which need a second space to tell an untouched one
 * apart from a recorded one.
 */
function space(over: Partial<SessionSpaceVm> = {}): SessionSpaceVm {
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
    newFileKind: "log",
    // Story 51.3's two keys: a space that says nothing about how it opens or
    // how much it shows, which is what every case here except the fold ones
    // wants. `null` and not omitted — the field is required on the wire, and a
    // fixture that could leave it out would let a call site read `undefined`
    // as "follow the setting" by accident rather than by decision.
    folded: null,
    rows: null,
    // Story 52.5's key: this space names no destination, which is byte-for-byte
    // today's behaviour.
    createDir: "",
    ...over,
  };
}

/**
 * The residue space, as `sessions_spaces` sends it (Story 52.4).
 *
 * Its query is every kind negated, so Rust answers `newFileKind: null` and puts
 * {@link UNTAGGED_REFUSAL} on the selection — this fixture is that pair and not
 * a hand-made variant of it.
 */
const UNTAGGED = space({
  id: "_spaces/untagged.md",
  name: "Untagged",
  query: "-tag:about -tag:log -tag:prompt -tag:ref -tag:task",
  sort: "name asc",
  sortEffective: "name asc",
  icon: "inbox",
  defaultKey: SESSION_SPACE_UNTAGGED_KEY,
  order: 6,
  newFileKind: null,
});

/** `spaces::Refusal::Negated`'s sentence, as Rust words it. */
const UNTAGGED_REFUSAL =
  "this space asks for what is left over — every one of its terms is a negation — so it names " +
  "no kind, and a create writes one kind with one tag. There is nothing a file made here could " +
  "be: make the file from Files below, and it appears here until you give it a kind tag.";

/** One row of a space's selection, named by the file it is. */
function row(relPath: string) {
  return {
    id: `path:${relPath}`,
    relPath,
    subpath: `60-sessions/active/2026-08-10-keeper/${relPath}`,
    title: relPath.replace(/\.md$/, ""),
    tags: [],
    mtimeMs: NOW - 1_000,
    unstableIdentity: true,
  };
}

beforeEach(() => {
  sessionsDetail.mockResolvedValue(detail());
  sessionsTree.mockResolvedValue(tree());
  sessionsRefs.mockResolvedValue(refs());
  sessionsSpaces.mockResolvedValue([]);
  sessionsSpaceFiles.mockResolvedValue([]);
  listenSessionsChanged.mockResolvedValue(() => {});
  // Unfolded, which is the registry's own default. A case that wants the other
  // answer says so.
  sessionsSpacesFoldedGet.mockResolvedValue(false);
  panelsStore.setState(panelsStore.getInitialState(), true);
  resetNotesVaultsStoreForTest();
});

afterEach(() => {
  vi.clearAllMocks();
  resetSessionSpacesFoldForTest();
  // biome-ignore lint/suspicious/noDocumentCookie: clearing the fold this suite wrote
  document.cookie = `${SESSION_SPACES_FOLD_COOKIE}=; path=/; max-age=0`;
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

  it("puts the spaces above the files, and the log last, in document order", async () => {
    mount();
    await screen.findByRole("region", { name: SESSION_DETAIL_FILES_HEADING });
    // Asserting on the DOM's own order rather than on four separate presence
    // checks: "spaces first, log last" is a claim about sequence, and only a
    // sequence can falsify it. `compareDocumentPosition` reads the rendered
    // tree, so a reorder that satisfied every individual query but shuffled the
    // page would still fail here — which is exactly what happened to this
    // assertion in Story 52.4: the first two entries swapped and nothing else on
    // the surface could tell.
    const order = [
      // ABOVE the files, on the operator's own instruction (*"umiesc spaces
      // ponad files"*). It read the other way round until Story 52.4, on the
      // argument that the tree is what the session holds and this is a reading
      // of it — which is about which is more fundamental, not about which is
      // read more often.
      SESSION_SPACES_HEADING,
      SESSION_DETAIL_FILES_HEADING,
      SESSION_REFS_HEADING,
      SESSION_DETAIL_LOG_HEADING,
    ].map((name) => screen.getByRole("region", { name }));
    for (let index = 0; index + 1 < order.length; index += 1) {
      expect(
        order[index].compareDocumentPosition(order[index + 1]) & Node.DOCUMENT_POSITION_FOLLOWING,
      ).toBeTruthy();
    }
  });

  /**
   * Story 52.1 inverted this test. It was "names the record by shape: about.md
   * when flat, README when not", and it asserted that a flat session's record
   * opened at `…/about.md` — "opening README.md would open the migration's
   * signpost instead of the session". Both contracts keep the record at
   * `README.md` now, there is no signpost to open, and the shape must NOT reach
   * the filename at all.
   */
  it("names the record once, under both shapes, and opens the one file", async () => {
    for (const shape of ["folder", "flat"] as const) {
      cleanup();
      panelsStore.setState(panelsStore.getInitialState(), true);
      sessionsDetail.mockResolvedValue(detail({ shape }));
      mount();

      const open = await screen.findByRole("button", {
        name: SESSION_DETAIL_OPEN_RECORD_LABEL,
      });
      open.click();
      await waitFor(() => {
        const target = panelsStore.getState().panels.find((p) => p.target?.kind === "file")?.target;
        expect(target).toMatchObject({
          relativePath: "60-sessions/active/2026-08-10-keeper/README.md",
        });
      });
    }
  });

  /**
   * Row 8. The `Unfiled` badge list is gone and the same files arrive as the
   * LAST space instead.
   *
   * This test used to mock `detail({ unfiled: [...] })` and assert a region
   * called "Unfiled" holding one static badge per filename — no count, no fold,
   * no verb on any row. There is no such field on the payload now (`pool.unfiled`
   * stops at the Rust boundary), so the claim is asserted at the same level in
   * its new form: the residue is an ordinary space's selection, it sits after
   * every other space, its rows are rows, and nothing on this surface is called
   * Unfiled any more.
   */
  it("shows what declares no kind as the last space, and has no Unfiled list", async () => {
    sessionsDetail.mockResolvedValue(detail({ shape: "flat" }));
    sessionsSpaces.mockResolvedValue([space(), UNTAGGED]);
    sessionsSpaceFiles.mockResolvedValue([
      { spaceId: "_spaces/log.md", files: [], error: null, noHome: null, openRecord: false },
      {
        spaceId: UNTAGGED.id,
        files: [row("stray-thought.md"), row("pasted.md")],
        error: null,
        noHome: UNTAGGED_REFUSAL,
        openRecord: false,
      },
    ]);
    mount();

    const spaces = await screen.findByRole("region", { name: SESSION_SPACES_HEADING });
    const untagged = within(spaces).getByRole("region", { name: "Untagged" });
    expect(within(untagged).getByText("stray-thought")).toBeInTheDocument();
    expect(within(untagged).getByText("pasted")).toBeInTheDocument();

    // Last, which is the whole of the operator's instruction about where it
    // goes: asserted on sequence, because a residue section rendered above Tasks
    // would satisfy every presence check on this surface.
    const sections = within(spaces).getAllByRole("region");
    expect(sections[sections.length - 1]).toBe(untagged);

    // And the surface it replaced is gone rather than sitting beside it, which is
    // the half a presence check on the new section cannot see.
    expect(screen.queryByRole("region", { name: "Unfiled" })).toBeNull();
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
    sessionsSpaceFiles.mockResolvedValue([
      { spaceId: "_spaces/log.md", files: [], error: null, noHome: null, openRecord: false },
    ]);
    mount();

    const section = await screen.findByRole("region", { name: SESSION_SPACES_HEADING });
    expect(within(section).getByText("Log")).toBeInTheDocument();
    // The zone id alone for the definitions; the session too for the selections.
    expect(sessionsSpaces).toHaveBeenCalledWith("tgdrive");
    expect(sessionsSpaceFiles).toHaveBeenCalledWith("tgdrive", "01J5AAAAAAAAAAAAAAAAAAAAAA");
  });

  /**
   * The section writes into THIS session (Story 49.2, FR-273), with the id this
   * surface already holds.
   *
   * Asserted here and not only in the section's own suite, because a dropped
   * prop is the one defect that suite cannot see: it passes the id itself, so
   * it would stay green while the real surface wrote into `undefined`.
   *
   * The flat contract, explicitly: this file's default fixture is
   * folder-shaped, where a Log space has no create by design — see the case
   * below.
   */
  it("gives the spaces section the session a new note belongs to", async () => {
    sessionsDetail.mockResolvedValue(detail({ shape: "flat" }));
    sessionsSpaces.mockResolvedValue([space()]);
    sessionsSpaceFiles.mockResolvedValue([
      { spaceId: "_spaces/log.md", files: [], error: null, noHome: null, openRecord: false },
    ]);
    sessionsFileNewKind.mockResolvedValue("60-sessions/active/2026-08-10-keeper/untitled.md");
    mount();

    const section = await screen.findByRole("region", { name: SESSION_SPACES_HEADING });
    fireEvent.click(within(section).getByRole("button", { name: `${SESSION_SPACE_NEW_NOTE} Log` }));

    await waitFor(() =>
      expect(sessionsFileNewKind).toHaveBeenCalledWith(
        "tgdrive",
        "01J5AAAAAAAAAAAAAAAAAAAAAA",
        "log",
        "",
        // The space that pressed it (Story 52.5), so Rust can read that
        // definition's own destination.
        "_spaces/log.md",
      ),
    );
  });

  /**
   * The other half: a kind this session's contract keeps no home for.
   *
   * A folder-shaped session's log is a `## Log` heading inside README.md, not a
   * file (`pool::log_view`), so `shape::kind_dir` refuses `(Folder, Log)`,
   * `sessions_space_files` puts that refusal on the selection, and the section
   * renders it as the control's own description. Story 52.4 turned that control
   * from absent into present-and-disabled: the refusal is a thing to read, and a
   * gap where every other space has a button was a question the surface left the
   * person to answer.
   *
   * **The sentence is on the payload, not composed anywhere in TypeScript.**
   * Story 50.1 shipped a `shape` prop feeding a TS copy of `kind_dir` and a TS
   * copy of the refusal's wording; both are gone. What this suite can still see
   * that the section's own cannot is that the selections REACH the section —
   * `session-spaces.test.tsx` hands itself the payload, so a dropped
   * `selections` prop is invisible from there.
   */
  it("describes the disabled create with the no-home sentence Rust put on the selection", async () => {
    const noHome =
      "a folder-shaped session's log is a `### ` entry under `## Log` in README.md, not a " +
      "file — use New log, which appends one there.";
    sessionsSpaces.mockResolvedValue([space()]);
    sessionsSpaceFiles.mockResolvedValue([
      { spaceId: "_spaces/log.md", files: [], error: null, noHome, openRecord: false },
    ]);
    mount();

    const section = await screen.findByRole("region", { name: SESSION_SPACES_HEADING });
    // The space is listed — the query is still true here — and the write verb is
    // present, refusing, and saying why.
    expect(within(section).getByText("Log")).toBeInTheDocument();
    const create = within(section).getByRole("button", { name: `${SESSION_SPACE_NEW_NOTE} Log` });
    expect(create).toBeDisabled();
    // Resolved through the accessibility tree rather than by reading the
    // attribute: a dangling `aria-describedby` renders byte-identically and every
    // presence check still passes, which is the shape of defect this repo keeps
    // shipping.
    expect(create).toHaveAccessibleDescription(noHome);
    expect(within(section).getByText(noHome)).toBeInTheDocument();
  });

  /**
   * Rows 1 and 2 of Story 51.7 and row 2 of Story 52.4, at the mount point:
   * About's refusal is a control a person can see, focus and read.
   *
   * The defect 51.7 shipped with was that the sentence was only computed where a
   * create HAD been derived (`sessions_space_files` asked `creatable_kind`
   * first), so the one space with three reasons to refuse had none of them on
   * screen. 51.7 gave it the sentence; the button was still absent, and the
   * owner's report on 52.4 was precisely that — *"about space nie ma przycisku
   * dodaj jak inne"*. So the control is present now, disabled, and its
   * accessible description IS Rust's sentence: nothing about this refusal is
   * worded in TypeScript, and `session-spaces.test.tsx` cannot see that the
   * sentence travelled because it hands itself the payload.
   *
   * The two-term sentence, deliberately, because that is the live zone's About
   * query (`tag:about tag:recordings`): the first refusal in the chain is the
   * query's own, and it is the one a person has to read to understand why.
   */
  it("gives About a create that is present, disabled, and describes itself with Rust's refusal", async () => {
    const noHome =
      "this space asks for more than one thing, so there is no single kind a file made here " +
      "could be: every term has to hold for a file to appear, and a create writes one kind " +
      "with one tag. Narrow the query to a single `tag:` term to write into this space, or " +
      // "below" since Story 52.4 moved this section above the files. The sentence
      // is Rust's; a stale direction here is a stale direction in the product.
      "make the file from Files below and tag it so this space picks it up.";
    sessionsSpaces.mockResolvedValue([
      space({
        id: "_spaces/about.md",
        name: "About",
        query: "tag:about tag:recordings",
        defaultKey: "about",
        newFileKind: null,
      }),
    ]);
    sessionsSpaceFiles.mockResolvedValue([
      { spaceId: "_spaces/about.md", files: [], error: null, noHome, openRecord: true },
    ]);
    mount();

    const section = await screen.findByRole("region", { name: SESSION_SPACES_HEADING });
    expect(within(section).getByText(noHome)).toBeInTheDocument();
    const create = within(section).getByRole("button", {
      name: `${SESSION_SPACE_NEW_NOTE} About`,
    });
    expect(create).toBeDisabled();
    expect(create).toHaveAccessibleDescription(noHome);
  });

  /**
   * Row 3. Where the create is refused because the record already exists, the
   * space offers the verb that does apply — and it opens the same file the
   * header's own button opens, through the one file target (AD-109).
   *
   * The label is one name under both shapes since Story 52.1, and the section is
   * handed it rather than composing one. `session-spaces.tsx`
   * owns "the button renders and calls back" (`session-spaces.test.tsx`); what
   * only this suite can see is that the callback and the label the section is
   * handed are the ones that name and open this session's record.
   */
  it("opens the record from the space whose create was refused because it exists", async () => {
    sessionsDetail.mockResolvedValue(detail({ shape: "folder" }));
    sessionsSpaces.mockResolvedValue([
      space({ id: "_spaces/about.md", name: "About", query: "tag:about", newFileKind: null }),
    ]);
    sessionsSpaceFiles.mockResolvedValue([
      {
        spaceId: "_spaces/about.md",
        files: [],
        error: null,
        noHome:
          "a session has one about record — README.md, under both contracts — and keeper edits " +
          "it rather than making a second.",
        openRecord: true,
      },
    ]);
    mount();

    const section = await screen.findByRole("region", { name: SESSION_SPACES_HEADING });
    const open = within(section).getByRole("button", {
      name: SESSION_DETAIL_OPEN_RECORD_LABEL,
    });
    fireEvent.click(open);

    const target = panelsStore.getState().panels.find((p) => p.target?.kind === "file")?.target;
    expect(target).toMatchObject({
      profileId: "tgdrive",
      relativePath: "60-sessions/active/2026-08-10-keeper/README.md",
    });
  });

  /**
   * And the fix itself, at the mount point: on the SAME folder-shaped fixture a
   * References space is creatable, because `refs/` is a directory that shape's
   * pool reads — so Rust answers `no_home: null` for it. This is the case that
   * would have been red before Story 50.1 and the one the owner's report is
   * about.
   */
  it("offers a create for References on a folder-shaped session", async () => {
    sessionsSpaces.mockResolvedValue([
      space({ id: "_spaces/refs.md", name: "References", query: "tag:ref", newFileKind: "ref" }),
    ]);
    sessionsSpaceFiles.mockResolvedValue([
      { spaceId: "_spaces/refs.md", files: [], error: null, noHome: null, openRecord: false },
    ]);
    sessionsFileNewKind.mockResolvedValue(
      "60-sessions/active/2026-08-10-keeper/refs/2026-08-16-0900-untitled.md",
    );
    mount();

    const section = await screen.findByRole("region", { name: SESSION_SPACES_HEADING });
    fireEvent.click(
      within(section).getByRole("button", { name: `${SESSION_SPACE_NEW_NOTE} References` }),
    );

    await waitFor(() =>
      expect(sessionsFileNewKind).toHaveBeenCalledWith(
        "tgdrive",
        "01J5AAAAAAAAAAAAAAAAAAAAAA",
        "ref",
        "",
        // The space that pressed it (Story 52.5).
        "_spaces/refs.md",
      ),
    );
  });

  /**
   * ONE create in flight across the whole session, not one per section.
   *
   * The Files heading and every writable space both post
   * `sessions_file_new_kind` with an EMPTY title, and Rust names such a file
   * `YYYY-MM-DD-HHMM-untitled.md` from the clock to the minute; `compile_new`
   * emits a plain `WriteFile`, so two presses in the same minute resolve to one
   * filename and the second silently overwrites the first — a `tag: prompt`
   * file becoming a `tag: log` one.
   *
   * **Only this suite can see it.** Story 50.1 shipped the guard as two
   * independent `useState`s on two siblings, each with a green in-flight test in
   * its own file, and each test passed while the cross-surface press stayed
   * reachable. The flag now lives on their common parent, which is why the
   * claim is asserted where both are mounted — and asserted in BOTH directions,
   * because one shared flag and two flags that happen to agree once look the
   * same from one press.
   */
  it("keeps one create in flight across the Files heading and the spaces below", async () => {
    sessionsDetail.mockResolvedValue(detail({ shape: "flat" }));
    sessionsSpaces.mockResolvedValue([space()]);
    sessionsSpaceFiles.mockResolvedValue([
      { spaceId: "_spaces/log.md", files: [], error: null, noHome: null, openRecord: false },
    ]);
    // The executor form, not `Promise.withResolvers`: the project compiles
    // against `lib: ES2020`, where that constructor method does not exist.
    let land!: (subpath: string) => void;
    const held = () =>
      new Promise<string>((resolve) => {
        land = resolve;
      });
    sessionsFileNewKind.mockImplementation(held);
    mount();

    const spaces = await screen.findByRole("region", { name: SESSION_SPACES_HEADING });
    const files = await screen.findByRole("region", { name: SESSION_DETAIL_FILES_HEADING });
    const newPrompt = within(files).getByRole("button", { name: SESSION_FILE_NEW_PROMPT_LABEL });
    const newNote = within(spaces).getByRole("button", {
      name: `${SESSION_SPACE_NEW_NOTE} Log`,
    });

    // Files heading first: the space's create goes down with it.
    fireEvent.click(newPrompt);
    expect(newNote).toBeDisabled();
    fireEvent.click(newNote);
    expect(sessionsFileNewKind).toHaveBeenCalledTimes(1);
    await act(async () => {
      land("60-sessions/active/2026-08-10-keeper/2026-08-16-0900-untitled.md");
    });
    await waitFor(() => expect(newNote).toBeEnabled());

    // And the other way round, which two agreeing-by-accident flags would fail.
    fireEvent.click(newNote);
    expect(newPrompt).toBeDisabled();
    fireEvent.click(newPrompt);
    expect(sessionsFileNewKind).toHaveBeenCalledTimes(2);
    await act(async () => {
      land("60-sessions/active/2026-08-10-keeper/2026-08-16-0901-untitled.md");
    });
    await waitFor(() => expect(newPrompt).toBeEnabled());
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

/**
 * Matrix row 5 (Story 49.3, FR-275, FR-276) — the hydration, at the mount point.
 *
 * **Why this cannot live in the store's own suite (DW-172).** A `hydrate…` that
 * nothing calls is invisible from below: `session-spaces-fold.test.ts` passes
 * in full with the restore never wired up, and the person gets a fold that
 * forgets itself every time they leave the session. Story 48.1's mutation M3
 * measured exactly this — deleting `hydrateColumnFold` from `AppShell` killed
 * one test, the one at the mount point. So the restore is asserted HERE, by
 * mounting the real surface twice with nothing carried across but the cookie.
 */
describe("SessionDetail restores the spaces' fold", () => {
  const LOG = spaceFoldKey("tgdrive", "_spaces/log.md");

  it("row 5: finds a folded space folded after a remount, from the cookie alone", async () => {
    sessionsSpaces.mockResolvedValue([space()]);
    const first = mount();

    fireEvent.click(await screen.findByRole("button", { name: "Collapse Log" }));
    const written = document.cookie;
    expect(readSessionSpacesFold(written).get(LOG)).toBe(true);
    first.unmount();

    // A cold start: nothing in memory, and no test-side hydrate either. If the
    // detail stops calling it, this is the assertion that goes red.
    resetSessionSpacesFoldForTest();
    mount();

    expect(await screen.findByRole("button", { name: "Expand Log" })).toHaveAttribute(
      "aria-expanded",
      "false",
    );
  });

  /** The other half of the same call: the SETTING reaches the fold, so a space
   *  nobody has touched arrives the way `sessions.spaces_folded` says. */
  it("hands the default it read to a space with nothing recorded", async () => {
    sessionsSpacesFoldedGet.mockResolvedValue(true);
    sessionsSpaces.mockResolvedValue([space()]);

    mount();

    expect(await screen.findByRole("button", { name: "Expand Log" })).toHaveAttribute(
      "aria-expanded",
      "false",
    );
    expect(sessionsSpacesFoldedGet).toHaveBeenCalled();
  });

  /**
   * The race the restore used to lose (finding 1). `readSessionSpacesFold`
   * needs no IPC, but the spaces payload arrives from its own `invoke` — so a
   * restore gated behind `sessions_spaces_folded_get` let the spaces win and
   * paint every hand-folded space OPEN before snapping it shut. Here the
   * setting read never resolves at all: the fold the person made is on screen
   * anyway, which it cannot be if the cookie waits for Rust.
   */
  it("restores a folded space before the setting read has resolved", async () => {
    sessionsSpacesFoldedGet.mockReturnValue(new Promise<boolean>(() => {}));
    sessionsSpaces.mockResolvedValue([space()]);
    writeCookie(sessionSpacesFoldCookie(new Map([[LOG, true]])));

    mount();

    expect(await screen.findByRole("button", { name: "Expand Log" })).toHaveAttribute(
      "aria-expanded",
      "false",
    );
  });

  /**
   * The latch is about the COOKIE, not the setting. A second detail must not
   * overwrite a fold somebody has changed since the first — and it must still
   * arrive on the setting they changed in Settings meanwhile, or a switch they
   * just flipped would look like it had done nothing until the next restart.
   */
  it("picks up a setting changed since the first detail mounted", async () => {
    sessionsSpaces.mockResolvedValue([space()]);
    const first = mount();
    expect(await screen.findByRole("button", { name: "Collapse Log" })).toBeInTheDocument();
    first.unmount();

    sessionsSpacesFoldedGet.mockResolvedValue(true);
    mount();

    expect(await screen.findByRole("button", { name: "Expand Log" })).toHaveAttribute(
      "aria-expanded",
      "false",
    );
  });

  /**
   * Settings moves the fallback the moment the switch does. A read this detail
   * issued BEFORE that flip is older news than the flip, and applying it would
   * silently undo it — the protection the `hydrate…` latch used to give for
   * free, back when the fallback was seeded inside it.
   */
  it("does not let a read in flight undo a setting flipped while it was out", async () => {
    let answer: (value: boolean) => void = () => {};
    sessionsSpacesFoldedGet.mockReturnValue(
      new Promise<boolean>((resolve) => {
        answer = resolve;
      }),
    );
    sessionsSpaces.mockResolvedValue([space()]);
    mount();
    expect(await screen.findByRole("button", { name: "Collapse Log" })).toBeInTheDocument();

    // The switch in Settings, while this detail's read is still out.
    act(() => setSpacesFoldedDefault(true));
    expect(screen.getByRole("button", { name: "Expand Log" })).toBeInTheDocument();

    // Resolved inside `act`, so the read's `.then` has certainly run by the
    // assertion: what is being measured is that it changed nothing.
    await act(async () => {
      answer(false);
    });

    expect(sessionSpacesFoldStore.getState().defaultFolded).toBe(true);
    expect(screen.getByRole("button", { name: "Expand Log" })).toHaveAttribute(
      "aria-expanded",
      "false",
    );
  });

  /**
   * A read that fails leaves every space where the person left it: the ones
   * nobody has touched reachable, and the ones somebody folded still folded.
   * Folding sections because keeper could not reach its own settings table
   * would be a refusal dressed as a preference.
   *
   * Log carries a RECORD on purpose — that is what tells "hydrated, and the
   * fallback stood" apart from "never hydrated at all", since an un-hydrated
   * store answers unfolded for both. Without it the case stays green with the
   * whole restore deleted.
   */
  it("leaves untouched spaces open when the setting cannot be read, and folded ones folded", async () => {
    sessionsSpacesFoldedGet.mockRejectedValue(new Error("no registry"));
    sessionsSpaces.mockResolvedValue([space(), space({ id: "_spaces/tasks.md", name: "Tasks" })]);
    writeCookie(sessionSpacesFoldCookie(new Map([[LOG, true]])));

    mount();

    expect(await screen.findByRole("button", { name: "Collapse Tasks" })).toHaveAttribute(
      "aria-expanded",
      "true",
    );
    expect(screen.getByRole("button", { name: "Expand Log" })).toHaveAttribute(
      "aria-expanded",
      "false",
    );
  });
});

/**
 * Story 50.4: a file that just became `tag:ref` appears in References, with no
 * manual refresh (FR-283, AD-120).
 *
 * **Where the two halves are, so neither is asserted twice or not at all.** The
 * write emits `keeper://sessions-changed` for the root it landed in
 * (`sync_ipc::sync_write_frontmatter`) — that half is in the shell crate and is
 * verified on macOS. The half here is the one that decides whether the surface
 * is honest when it arrives: the detail re-reads BOTH space payloads on that
 * event, so a selection that changed under it is on screen.
 *
 * The re-read itself is not new — it is the listener 47.x already wired, which
 * is exactly why 50.4 adds no frontend plumbing for it. What is new is that a
 * property write now announces itself into it instead of waiting for a
 * debounced watcher, and these cases are what make that arrival meaningful.
 */
describe("SessionDetail after a file's properties change", () => {
  const REFERENCES = space({
    id: "_spaces/references.md",
    name: "References",
    query: "tag:ref",
    defaultKey: "ref",
    newFileKind: "ref",
  });

  function selection(relPath: string) {
    return {
      spaceId: REFERENCES.id,
      files: [
        {
          id: `path:${relPath}`,
          relPath,
          subpath: `60-sessions/active/s/${relPath}`,
          title: relPath.replace(/^.*\//, "").replace(/\.md$/, ""),
          tags: ["ref"],
          mtimeMs: NOW - 1_000,
          unstableIdentity: true,
        },
      ],
      error: null,
      noHome: null,
      openRecord: false,
    };
  }

  it("row 6: lists a file in refs/ that just became tag:ref, on the next read", async () => {
    sessionsSpaces.mockResolvedValue([REFERENCES]);
    sessionsSpaceFiles.mockResolvedValue([
      { spaceId: REFERENCES.id, files: [], error: null, noHome: null, openRecord: false },
    ]);
    mount();

    const section = await screen.findByRole("region", { name: SESSION_SPACES_HEADING });
    expect(within(section).queryByText("inputs")).toBeNull();

    // The tag is written, and the write says so on the zone's own event.
    const onChanged = listenSessionsChanged.mock.calls[0][0] as (rootId: string) => void;
    sessionsSpaceFiles.mockResolvedValue([selection("refs/inputs.md")]);
    onChanged("tgdrive");

    // No button was pressed on this surface. AD-120: the tag is what files the
    // file, and the space it lands in is Rust's answer to the query.
    expect(await within(section).findByText("inputs")).toBeInTheDocument();
    expect(sessionsSpaceFiles).toHaveBeenCalledTimes(2);
  });

  it("lists only what the selection payload holds, never what a tag on screen implies", async () => {
    // Story 50.4 wrote this case as "a folder-shaped session's ROOT markdown is
    // in no space at all", which was true of the reader it was written against
    // and is not true now: Story 51.1 put that shape's root markdown into the
    // pool (FR-286), so a root `stray.md` tagged `ref` IS selectable. The claim
    // that survives — and the only one this surface ever owed — is the one below:
    // it lists what the payload holds and never derives membership from a tag it
    // can see. A test that only asserted the row above would stay green over a
    // surface that filed files client-side.
    sessionsDetail.mockResolvedValue(detail({ shape: "folder" }));
    sessionsSpaces.mockResolvedValue([REFERENCES]);
    sessionsSpaceFiles.mockResolvedValue([
      { spaceId: REFERENCES.id, files: [], error: null, noHome: null, openRecord: false },
    ]);
    mount();

    const section = await screen.findByRole("region", { name: SESSION_SPACES_HEADING });
    const onChanged = listenSessionsChanged.mock.calls[0][0] as (rootId: string) => void;
    // Rust re-read the pool and still selected nothing: the root file is not in
    // it to be selected.
    sessionsSpaceFiles.mockResolvedValue([
      { spaceId: REFERENCES.id, files: [], error: null, noHome: null, openRecord: false },
    ]);
    onChanged("tgdrive");

    await waitFor(() => expect(sessionsSpaceFiles).toHaveBeenCalledTimes(2));
    expect(within(section).queryByText("stray")).toBeNull();
    expect(within(section).getByText(SESSION_SPACES_NO_FILES)).toBeInTheDocument();
  });
});

/**
 * The board follows the pool, not the shape (Story 51.7, FR-299).
 *
 * `session-board.test.tsx` owns the board's own behaviour — the four columns, the
 * drag, the stray row — and `notes/task-board.tsx` owns the drop arithmetic. What
 * only this suite can see is the gate: for two epics the board was rendered for a
 * flat session ONLY, with the reason that a folder-shaped one had no pool to tag,
 * so the owner of a folder-shaped session had never seen a board at all.
 */
describe("SessionDetail's board", () => {
  function task(over: Partial<SessionTaskVm> = {}): SessionTaskVm {
    return {
      id: "path:ship-it.md",
      relPath: "ship-it.md",
      title: "Ship it",
      status: "todo",
      order: 1.5,
      orderIsOwn: true,
      tags: ["task"],
      unstableIdentity: true,
      ...over,
    };
  }

  /** Row 4. The default fixture IS folder-shaped, which is the point. */
  it("row 4: draws a folder-shaped session's tagged root markdown as cards", async () => {
    sessionsDetail.mockResolvedValue(detail({ shape: "folder", tasks: [task()] }));
    mount();

    const board = await screen.findByRole("region", { name: SESSION_BOARD_HEADING });
    expect(within(board).getByRole("button", { name: "Ship it" })).toBeInTheDocument();
    // In the column its own `status:` names, which is what makes this a board
    // rather than a list.
    expect(within(board).getByRole("list", { name: "To do" })).toHaveTextContent("Ship it");
  });

  /**
   * Rows 5 and 6. The keyboard path, on the shape that never had one: the
   * dropdown writes through the same `sessions_task_move` a drop does, so this is
   * the assertion that the board is LIVE on a folder-shaped session and not just
   * drawn. The drag's own handlers and the index arithmetic are `task-board`'s
   * and are tested there — this repo does not ship a pointer-only affordance, so
   * the reachable-without-a-pointer half is the one worth asserting at the mount
   * point.
   */
  it("rows 5-6: moves a card by keyboard on a folder-shaped session", async () => {
    sessionsTaskMove.mockResolvedValue(undefined);
    sessionsDetail.mockResolvedValue(detail({ shape: "folder", tasks: [task()] }));
    mount();

    const board = await screen.findByRole("region", { name: SESSION_BOARD_HEADING });
    const move = within(board).getByRole("combobox", {
      name: `${SESSION_BOARD_MOVE_LABEL} — Ship it`,
    });
    fireEvent.change(move, { target: { value: "done" } });

    await waitFor(() =>
      expect(sessionsTaskMove).toHaveBeenCalledWith(
        "tgdrive",
        "01J5AAAAAAAAAAAAAAAAAAAAAA",
        "ship-it.md",
        "done",
        0,
      ),
    );
  });

  /**
   * Row 7. Nothing tagged — which is also every session with no pool at all,
   * because a pool with nothing in it selects no tasks either way. The board says
   * what a task IS rather than drawing four empty columns over a session that has
   * none: the columns are the thing that would be saying something untrue.
   */
  it("row 7: says what a task is instead of drawing columns over nothing", async () => {
    sessionsDetail.mockResolvedValue(detail({ shape: "folder", tasks: [] }));
    mount();

    const board = await screen.findByRole("region", { name: SESSION_BOARD_HEADING });
    expect(within(board).getByText(SESSION_BOARD_EMPTY)).toBeInTheDocument();
    expect(within(board).queryByRole("list", { name: "To do" })).toBeNull();
  });

  /** Row 8. The shape the board always had, unchanged in every respect. */
  it("row 8: draws a flat session's board exactly as before", async () => {
    sessionsDetail.mockResolvedValue(detail({ shape: "flat", tasks: [task()] }));
    mount();

    const board = await screen.findByRole("region", { name: SESSION_BOARD_HEADING });
    expect(within(board).getByRole("list", { name: "To do" })).toHaveTextContent("Ship it");
    expect(within(board).queryByText(SESSION_BOARD_EMPTY)).toBeNull();
  });
});
