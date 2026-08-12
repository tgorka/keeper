import { render, screen, waitFor, within } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { SessionDetailVm } from "@/lib/ipc/client";

const sessionsDetail = vi.fn();
const listenSessionsChanged = vi.fn();
vi.mock("@/lib/ipc/client", () => ({
  sessionsDetail: (rootId: unknown, sessionId: unknown) => sessionsDetail(rootId, sessionId),
  listenSessionsChanged: (cb: unknown) => listenSessionsChanged(cb),
}));

import {
  SESSION_DETAIL_ARTIFACTS_HEADING,
  SESSION_DETAIL_LOG_HEADING,
  SESSION_DETAIL_PROPERTIES_HEADING,
  SESSION_DETAIL_WORKSPACE_CAVEAT,
  SESSION_DETAIL_WORKSPACE_HEADING,
  SessionDetail,
} from "@/components/sessions/session-detail";
import { panelsStore } from "@/lib/stores/panels";

const NOW = Date.now();

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
    artifacts: [
      {
        name: "release-notes.md",
        relPath: "artifacts/release-notes.md",
        size: 2048,
        mtimeMs: NOW - 60_000,
        isDir: false,
      },
    ],
    refs: [],
    prompts: [
      {
        name: "01-scope.md",
        relPath: "prompts/01-scope.md",
        size: 512,
        mtimeMs: NOW - 3_600_000,
        isDir: false,
      },
    ],
    workspace: [
      {
        name: "iter-3.md",
        relPath: "workspace/iter-3.md",
        size: 4096,
        mtimeMs: NOW - 120_000,
        isDir: false,
      },
    ],
    extras: [],
    ...over,
  };
}

beforeEach(() => {
  sessionsDetail.mockResolvedValue(detail());
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

  it("opens a file through the one file target; workspace rows carry the caveat and the lock", async () => {
    mount();
    const artifacts = await screen.findByRole("region", {
      name: SESSION_DETAIL_ARTIFACTS_HEADING,
    });
    within(artifacts).getByText("release-notes.md").click();
    await waitFor(() => {
      const target = panelsStore.getState().panels.find((p) => p.target?.kind === "file")?.target;
      expect(target).toMatchObject({
        kind: "file",
        profileId: "tgdrive",
        relativePath: "60-sessions/active/2026-08-10-keeper/artifacts/release-notes.md",
      });
    });

    const workspace = screen.getByRole("region", { name: SESSION_DETAIL_WORKSPACE_HEADING });
    expect(within(workspace).getByText(SESSION_DETAIL_WORKSPACE_CAVEAT)).toBeInTheDocument();
    expect(within(workspace).getByLabelText("read-only")).toBeInTheDocument();
  });

  it("re-reads when the changed event names this root — the agent's write moves the view", async () => {
    mount();
    await screen.findByRole("region", { name: SESSION_DETAIL_LOG_HEADING });
    expect(sessionsDetail).toHaveBeenCalledTimes(1);
    const onChanged = listenSessionsChanged.mock.calls[0][0] as (rootId: string) => void;
    sessionsDetail.mockResolvedValue(
      detail({
        log: [
          { date: "2026-08-12", title: "agent wrote", body: "" },
          { date: "2026-08-11", title: "shipped 0.6.5", body: "" },
        ],
      }),
    );
    onChanged("tgdrive");
    await screen.findByText("agent wrote");
    // A change on ANOTHER root is not this detail's business.
    onChanged("neuradrive");
    expect(sessionsDetail).toHaveBeenCalledTimes(2);
  });
});
