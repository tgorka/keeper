import { fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import type {
  CardAgentVm,
  SessionDetailVm,
  SessionRootVm,
  SessionRowVm,
  SessionTaskVm,
} from "@/lib/ipc/client";

const sessionsDetail = vi.fn();
const sessionsTaskMove = vi.fn();
const sessionsRoots = vi.fn();
const sessionsList = vi.fn();
vi.mock("@/lib/ipc/client", async (importOriginal) => {
  const actual = await importOriginal<typeof import("@/lib/ipc/client")>();
  return {
    ...actual,
    sessionsDetail: (...args: unknown[]) => sessionsDetail(...args),
    sessionsTaskMove: (...args: unknown[]) => sessionsTaskMove(...args),
    sessionsRoots: (...args: unknown[]) => sessionsRoots(...args),
    sessionsList: (...args: unknown[]) => sessionsList(...args),
    listenSessionsChanged: vi.fn(async () => () => {}),
  };
});

import { BOARD_ALLOW_ELSEWHERE } from "@/components/notes/board-card-agent";
import { SESSION_ACTIONS_LABEL } from "@/components/sessions/session-actions";
import {
  SESSIONS_PHONE_MOVE_TO,
  SessionsPhoneBoard,
  SessionsPhoneList,
  sessionsPhoneMoveName,
} from "@/components/sessions/sessions-phone-pane";
import { resetSessionsListStoreForTest } from "@/lib/stores/sessions-list";
import { resetSessionsRootsStoreForTest } from "@/lib/stores/sessions-roots";

function task(relPath: string, status: string | null, order: number): SessionTaskVm {
  return {
    id: `path:${relPath}`,
    relPath,
    title: relPath.replace(".md", ""),
    status,
    order,
    orderIsOwn: true,
    tags: ["task"],
    unstableIdentity: true,
    agent: null,
  };
}

function detail(tasks: SessionTaskVm[]): SessionDetailVm {
  return {
    id: "01J8SESSION",
    path: "active/2026-10-12-board",
    title: "The board",
    status: "active",
    archivedYear: null,
    pinned: false,
    tags: [],
    properties: [],
    continues: [],
    continuedBy: [],
    summary: "",
    log: [],
    shape: "flat",
    tasks,
    room: null,
  };
}

const TASKS = [
  task("draft-a.md", "todo", 2),
  task("draft-b.md", "todo", 1),
  task("shipped.md", "done", 1),
  task("someday.md", "maybe", 1),
];

function board() {
  return render(<SessionsPhoneBoard rootId="p1" sessionId="01J8SESSION" onBack={() => {}} />);
}

beforeEach(() => {
  sessionsDetail.mockReset();
  sessionsTaskMove.mockReset();
  sessionsDetail.mockResolvedValue(detail(TASKS));
  sessionsTaskMove.mockResolvedValue({ unpushed: null });
});

describe("SessionsPhoneBoard (98.2, UX-DR145)", () => {
  it("pages the four columns with their counts, and gives strays a page of their own", async () => {
    board();
    const pager = await screen.findByRole("navigation", { name: "Columns" });
    expect(
      within(pager)
        .getAllByRole("button")
        .map((button) => button.textContent),
    ).toEqual(["In preparation 0", "To do 2", "Done 1", "Deferred 0", "Not in a column 1"]);
    // Each page lists its column's cards, in the order Rust resolves a move against.
    const todo = screen.getByRole("list", { name: "To do" });
    expect(
      within(todo)
        .getAllByRole("listitem")
        .map((item) => item.getAttribute("data-card-key")),
    ).toEqual(["draft-b.md", "draft-a.md"]);
    // The card whose status is none of the four is not dropped.
    expect(
      within(screen.getByRole("list", { name: "Not in a column" })).getByText("someday"),
    ).toBeInTheDocument();
  });

  it("has no stray page when every card is in a column", async () => {
    sessionsDetail.mockResolvedValue(detail(TASKS.slice(0, 3)));
    board();
    const pager = await screen.findByRole("navigation", { name: "Columns" });
    expect(within(pager).getAllByRole("button")).toHaveLength(4);
  });

  it("Move to… writes the card to the end of the column it joins, then re-reads", async () => {
    board();
    const picker = await screen.findByRole("combobox", {
      name: sessionsPhoneMoveName("draft-a"),
    });

    fireEvent.change(picker, { target: { value: "done" } });

    await waitFor(() =>
      expect(sessionsTaskMove).toHaveBeenCalledWith("p1", "01J8SESSION", "draft-a.md", "done", 1),
    );
    await waitFor(() => expect(sessionsDetail).toHaveBeenCalledTimes(2));
  });

  it("a refused move shows keeper's own sentence and re-reads nothing", async () => {
    sessionsTaskMove.mockRejectedValue(
      new Error("The card was not moved — this phone has 1 commit the remote does not."),
    );
    board();
    const picker = await screen.findByRole("combobox", {
      name: sessionsPhoneMoveName("shipped"),
    });

    fireEvent.change(picker, { target: { value: "todo" } });

    expect(await screen.findByRole("status")).toHaveTextContent(
      "The card was not moved — this phone has 1 commit the remote does not.",
    );
    expect(sessionsDetail).toHaveBeenCalledTimes(1);
  });
  it("a move whose push did not land says so, and the board re-reads the moved card", async () => {
    sessionsTaskMove.mockResolvedValue({
      unpushed: "Moved on this phone; the folder is paused, so it is not on your Mac yet.",
    });
    board();
    const picker = await screen.findByRole("combobox", {
      name: sessionsPhoneMoveName("draft-a"),
    });

    fireEvent.change(picker, { target: { value: "done" } });

    expect(await screen.findByRole("status")).toHaveTextContent(
      "Moved on this phone; the folder is paused, so it is not on your Mac yet.",
    );
    await waitFor(() => expect(sessionsDetail).toHaveBeenCalledTimes(2));
  });

  it("Move to… is an entry the picker can rest on, and choosing it moves nothing", async () => {
    board();
    const picker = await screen.findByRole("combobox", {
      name: sessionsPhoneMoveName("draft-a"),
    });
    // iOS's wheel cannot rest on a disabled row: it would sit on the first
    // column instead, and *Done* tapped only to look would move the card.
    expect(within(picker).getByRole("option", { name: SESSIONS_PHONE_MOVE_TO })).toBeEnabled();

    fireEvent.change(picker, { target: { value: "" } });

    expect(sessionsTaskMove).not.toHaveBeenCalled();
    expect(screen.queryByRole("status")).toBeNull();
  });

  it("marks the page the pager comes to rest on, not each page a slide passes", async () => {
    board();
    const nav = await screen.findByRole("navigation", { name: "Columns" });
    const pager = screen.getByRole("region", { name: "To do" }).parentElement as HTMLElement;
    let left = 0;
    Object.defineProperty(pager, "clientWidth", { configurable: true, value: 390 });
    Object.defineProperty(pager, "scrollLeft", {
      configurable: true,
      get: () => left,
      set: (value: number) => {
        left = value;
      },
    });
    const current = () =>
      within(nav)
        .getAllByRole("button")
        .filter((tab) => tab.getAttribute("aria-current") === "true")
        .map((tab) => tab.textContent);
    const scrollTo = (x: number) => {
      left = x;
      fireEvent.scroll(pager);
    };

    // A tap on a far tab marks it at once, and the slide through To do and
    // Done on the way does not walk the mark through them.
    fireEvent.click(within(nav).getByRole("button", { name: /^Deferred/ }));
    expect(current()).toEqual(["Deferred 0"]);
    scrollTo(390);
    expect(current()).toEqual(["Deferred 0"]);
    scrollTo(780);
    expect(current()).toEqual(["Deferred 0"]);
    scrollTo(1170);
    fireEvent(pager, new Event("scrollend"));
    expect(current()).toEqual(["Deferred 0"]);

    // A swipe back a page: the mark follows once the pager rests.
    scrollTo(780);
    expect(current()).toEqual(["Deferred 0"]);
    fireEvent(pager, new Event("scrollend"));
    expect(current()).toEqual(["Done 1"]);

    // Where the WebView sends no `scrollend`, a short quiet after the last
    // scroll decides it.
    scrollTo(390);
    await waitFor(() => expect(current()).toEqual(["To do 2"]));
  });

  it("an agent card scheduled elsewhere says where to allow it, and offers no Allow", async () => {
    const agent: CardAgentVm = {
      run: { value: "blocked", readable: true },
      assignee: { value: "tola-grey", readable: true },
      host: null,
      requestedBy: null,
      schedule: { value: "0 6 * * *", readable: true },
      lastRun: null,
      workflow: null,
      scheduledBy: { value: "tola-grey", readable: true },
      integrity: null,
      runningOn: null,
      waiting: null,
    };
    sessionsDetail.mockResolvedValue(detail([{ ...task("nightly.md", "todo", 1), agent }]));
    board();

    expect(await screen.findByText(BOARD_ALLOW_ELSEWHERE)).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: /^Allow/ })).toBeNull();
  });
});

describe("SessionsPhoneList (98.2, UX-DR145)", () => {
  const root: SessionRootVm = {
    id: "p1",
    name: "tgdrive",
    subfolder: "60-sessions",
    root: "/var/mobile/sync/p1/60-sessions",
    indexed: true,
    activeCount: 1,
    unreadCount: 0,
  };
  const row: SessionRowVm = {
    id: "01J8SESSIONAAAAAAAAAAAAAAA",
    path: "active/2026-10-12-budget",
    title: "Budget review",
    status: "active",
    archivedYear: null,
    workspaceMs: null,
    recordMs: null,
    lastLogDate: "",
    lastLogLine: "",
    snippet: "",
    tags: [],
    pinned: false,
    unread: false,
    origin: "local",
    headRev: "rev",
    conflict: false,
    lineage: false,
  };

  beforeEach(() => {
    resetSessionsRootsStoreForTest();
    resetSessionsListStoreForTest();
    sessionsRoots.mockReset();
    sessionsList.mockReset();
    sessionsRoots.mockResolvedValue([root]);
    sessionsList.mockResolvedValue([row]);
  });

  it("a session's row opens its board and carries no lifecycle menu", async () => {
    const onOpen = vi.fn();
    render(<SessionsPhoneList onBack={() => {}} onOpen={onOpen} />);

    fireEvent.click(await screen.findByRole("button", { name: /Budget review/ }));

    expect(onOpen).toHaveBeenCalledWith({ rootId: "p1", sessionId: row.id });
    expect(screen.queryByRole("button", { name: SESSION_ACTIONS_LABEL })).toBeNull();
  });

  it("says why when the zones themselves could not be read", async () => {
    sessionsRoots.mockRejectedValue(new Error("keeper could not read the sessions folders."));
    render(<SessionsPhoneList onBack={() => {}} onOpen={() => {}} />);

    expect(await screen.findByRole("alert")).toHaveTextContent(
      "keeper could not read the sessions folders.",
    );
  });
});
