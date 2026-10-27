import { act, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { SessionPeople } from "@/components/agents/session-people";
import type * as Ipc from "@/lib/ipc/client";
import type { AgentMemberVm, AgentSessionMembersVm } from "@/lib/ipc/client";
import { agentSessionsStore } from "@/lib/stores/agent-sessions";

const mock = vi.hoisted(() => ({
  push: null as ((sheet: AgentSessionMembersVm) => void) | null,
  subscribe: vi.fn(),
  unsubscribe: vi.fn().mockResolvedValue(undefined),
  add: vi.fn().mockResolvedValue(undefined),
  reader: vi.fn().mockResolvedValue(undefined),
  remove: vi.fn().mockResolvedValue(undefined),
  leave: vi.fn().mockResolvedValue(undefined),
  phone: false,
}));
vi.mock("@/hooks/use-shell-layout", () => ({ useShellLayout: () => ({ phone: mock.phone }) }));
vi.mock("@/lib/ipc/client", async (original) => ({
  ...(await original<typeof Ipc>()),
  agentSessionMembersSubscribe: mock.subscribe,
  agentSessionMembersUnsubscribe: mock.unsubscribe,
  agentSessionMemberAdd: mock.add,
  agentSessionMemberReaderSet: mock.reader,
  agentSessionMemberRemove: mock.remove,
  agentSessionLeave: mock.leave,
}));

const target = { database: "alpha", sessionId: "s.1" };

function member(patch: Partial<AgentMemberVm> & Pick<AgentMemberVm, "actorId" | "name">) {
  return {
    mark: patch.name[0],
    role: "person" as const,
    readsTrace: false,
    me: false,
    joining: false,
    canSetReader: false,
    canRemove: false,
    ...patch,
  };
}

function ownerSheet(patch: Partial<AgentSessionMembersVm> = {}): AgentSessionMembersVm {
  return {
    database: "alpha",
    sessionId: "s.1",
    owner: true,
    canLeave: false,
    members: [
      member({ actorId: "ana", name: "Ana", role: "owner", readsTrace: true, me: true }),
      member({ actorId: "ben", name: "Ben", canSetReader: true, canRemove: true }),
      member({ actorId: "nixie", name: "Nixie", role: "agent", readsTrace: true }),
    ],
    addable: [{ actorId: "cal", name: "Cal", mark: "C" }],
    ...patch,
  };
}

async function openWith(sheet: AgentSessionMembersVm) {
  render(<SessionPeople target={target} />);
  fireEvent.click(screen.getByRole("button", { name: "People" }));
  await waitFor(() => expect(mock.push).not.toBeNull());
  act(() => mock.push?.(sheet));
  return screen.getByRole("dialog", { name: "People" });
}

beforeEach(() => {
  vi.clearAllMocks();
  mock.push = null;
  mock.phone = false;
  mock.subscribe.mockImplementation(async (_target, callback) => {
    mock.push = callback;
    return "people-1";
  });
});

describe("the People sheet", () => {
  it("draws Rust's members and flips a reader only through Rust's next sheet", async () => {
    const dialog = await openWith(ownerSheet());
    expect(mock.subscribe).toHaveBeenCalledWith(target, expect.any(Function));
    const members = within(within(dialog).getByRole("list", { name: "Members" })).getAllByRole(
      "listitem",
    );
    expect(members.map((li) => li.textContent)).toEqual([
      expect.stringContaining("Owner · you"),
      expect.stringContaining("Ben"),
      expect.stringContaining("Agent"),
    ]);
    // Only Ben's flag is the owner's to change; nobody else has a switch.
    expect(within(dialog).getAllByRole("switch")).toHaveLength(1);
    const flag = within(dialog).getByRole("switch", {
      name: "Ben can read thinking and tool results",
    });
    expect(flag).toHaveAttribute("aria-checked", "false");
    fireEvent.click(flag);
    await waitFor(() => expect(mock.reader).toHaveBeenCalledWith(target, "ben", true));
    expect(flag).toHaveAttribute("aria-checked", "false");
    act(() =>
      mock.push?.(
        ownerSheet({
          members: [
            member({ actorId: "ana", name: "Ana", role: "owner", readsTrace: true, me: true }),
            member({
              actorId: "ben",
              name: "Ben",
              readsTrace: true,
              canSetReader: true,
              canRemove: true,
            }),
          ],
        }),
      ),
    );
    expect(
      within(dialog).getByRole("switch", { name: "Ben can read thinking and tool results" }),
    ).toHaveAttribute("aria-checked", "true");
  });

  it("adds only the people Rust offers and shows the refusal it answers", async () => {
    const dialog = await openWith(ownerSheet());
    const offered = within(dialog).getByRole("list", { name: "People you can add" });
    expect(
      within(offered)
        .getAllByRole("button")
        .map((b) => b.getAttribute("aria-label")),
    ).toEqual(["Add Cal"]);
    mock.add.mockRejectedValueOnce({ message: "cal is not a person of alpha's directory." });
    fireEvent.click(within(offered).getByRole("button", { name: "Add Cal" }));
    await waitFor(() => expect(mock.add).toHaveBeenCalledWith(target, "cal"));
    expect(await within(dialog).findByRole("alert")).toHaveTextContent(
      "cal is not a person of alpha's directory.",
    );
    act(() =>
      mock.push?.(
        ownerSheet({
          addable: [],
          notice: "The server refused adding cal: they are not in alpha's directory.",
        }),
      ),
    );
    expect(within(dialog).getByText(/Everyone in alpha is already here/)).toBeInTheDocument();
    expect(within(dialog).getByRole("status")).toHaveTextContent("The server refused adding cal");
  });

  it("asks before removing, keeps focus on the safe choice, and removes on confirmation", async () => {
    const dialog = await openWith(ownerSheet());
    fireEvent.click(within(dialog).getByRole("button", { name: "Remove Ben" }));
    const confirm = within(dialog).getByRole("group", { name: "Remove Ben" });
    expect(within(confirm).getByRole("button", { name: "Cancel" })).toHaveFocus();
    fireEvent.click(within(confirm).getByRole("button", { name: "Cancel" }));
    // Cancelled, focus goes back to the control that asked.
    expect(within(dialog).getByRole("button", { name: "Remove Ben" })).toHaveFocus();
    expect(mock.remove).not.toHaveBeenCalled();
    fireEvent.click(within(dialog).getByRole("button", { name: "Remove Ben" }));
    fireEvent.click(
      within(within(dialog).getByRole("group", { name: "Remove Ben" })).getByRole("button", {
        name: "Remove Ben",
      }),
    );
    await waitFor(() => expect(mock.remove).toHaveBeenCalledWith(target, "ben"));
    // Removed, Ben's row is going away with its button: focus waits on Close.
    await waitFor(() =>
      expect(within(dialog).getByRole("button", { name: "Close" })).toHaveFocus(),
    );
  });

  it("gives a member who is not the owner no changes to others, only Leave", async () => {
    const dialog = await openWith(
      ownerSheet({
        owner: false,
        canLeave: true,
        addable: [],
        members: [
          member({ actorId: "ana", name: "Ana", role: "owner", readsTrace: true }),
          member({ actorId: "ben", name: "Ben", me: true }),
        ],
      }),
    );
    expect(within(dialog).queryByRole("switch")).toBeNull();
    expect(within(dialog).queryByRole("button", { name: /^Remove/ })).toBeNull();
    expect(within(dialog).queryByRole("heading", { name: "Add a person" })).toBeNull();
    expect(within(dialog).getByText("Sees answers, not thinking or tool results")).toBeVisible();
    fireEvent.click(within(dialog).getByRole("button", { name: "Leave session" }));
    fireEvent.click(
      within(within(dialog).getByRole("group", { name: "Leave session" })).getByRole("button", {
        name: "Leave session",
      }),
    );
    await waitFor(() => expect(mock.leave).toHaveBeenCalledWith(target));
    await waitFor(() => expect(screen.queryByRole("dialog")).toBeNull());
  });

  it("offers the owner no Leave at all, and shows the server's refusal of one", async () => {
    // R-HQ-40: an owner cannot leave. A leave this device wrote as a member and the
    // server refused anyway comes back in Rust's notice.
    const dialog = await openWith(ownerSheet());
    expect(within(dialog).queryByRole("button", { name: "Leave session" })).toBeNull();
    act(() =>
      mock.push?.(
        ownerSheet({
          notice: "The server refused your leaving: the session's owner cannot leave it.",
        }),
      ),
    );
    expect(within(dialog).getByRole("status")).toHaveTextContent(
      "The server refused your leaving: the session's owner cannot leave it.",
    );
    expect(within(dialog).queryByRole("button", { name: "Leave session" })).toBeNull();
  });

  it("returns focus to the People button when closed, and ends its stream", async () => {
    const dialog = await openWith(ownerSheet());
    fireEvent.click(within(dialog).getByRole("button", { name: "Close" }));
    await waitFor(() => expect(screen.queryByRole("dialog")).toBeNull());
    expect(screen.getByRole("button", { name: "People" })).toHaveFocus();
    await waitFor(() => expect(mock.unsubscribe).toHaveBeenCalledWith("people-1"));
  });
});

describe("a session that leaves the list", () => {
  const row = (sessionId: string) => ({
    database: "alpha",
    sessionId,
    title: `about ${sessionId}`,
    titleWrite: { state: "confirmed" as const },
    displayName: `nixie · about ${sessionId}`,
    handle: "jolly-pigeon-squeaks",
    createdAtMs: 1780000000000,
    trust: "native",
    instances: [],
    agentActor: "nixie",
    agentName: "Nixie",
    agentMark: "N",
    archived: false,
    lastActivityMs: 1,
    choosesDrives: false,
    choosesModel: false,
  });

  it("is closed with a note when Rust's list drops the open one, and only then", () => {
    const store = agentSessionsStore.getState();
    store.apply({ ops: [{ op: "reset", rows: [row("s.1"), row("s.2")] }], total: 2 });
    store.select({ database: "alpha", sessionId: "s.1" });
    store.apply({ ops: [{ op: "remove", index: 1 }], total: 1 });
    expect(agentSessionsStore.getState().selected).toEqual({ database: "alpha", sessionId: "s.1" });
    store.apply({ ops: [{ op: "remove", index: 0 }], total: 0 });
    expect(agentSessionsStore.getState()).toMatchObject({
      selected: null,
      gone: { database: "alpha", sessionId: "s.1", displayName: "nixie · about s.1" },
    });
    // A session just created is selected before the list holds it: not dropped.
    store.select({ database: "alpha", sessionId: "new.1" });
    store.apply({ ops: [{ op: "reset", rows: [] }], total: 0 });
    expect(agentSessionsStore.getState()).toMatchObject({
      selected: { database: "alpha", sessionId: "new.1" },
      gone: null,
    });
  });

  it("drops the note when a refused leave brings the session back", () => {
    const store = agentSessionsStore.getState();
    store.apply({ ops: [{ op: "reset", rows: [row("s.1"), row("s.2")] }], total: 2 });
    store.select({ database: "alpha", sessionId: "s.1" });
    store.apply({ ops: [{ op: "remove", index: 0 }], total: 1 });
    expect(agentSessionsStore.getState().gone).toMatchObject({ sessionId: "s.1" });
    // Another session changing keeps the note.
    store.apply({ ops: [{ op: "set", index: 0, row: row("s.2") }], total: 1 });
    expect(agentSessionsStore.getState().gone).toMatchObject({ sessionId: "s.1" });
    store.apply({ ops: [{ op: "insert", index: 0, row: row("s.1") }], total: 2 });
    expect(agentSessionsStore.getState().gone).toBeNull();
  });
});
