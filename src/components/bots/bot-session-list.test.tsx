/**
 * The conversation list, the archive, and continue (Epic 61, Story 61.6,
 * FR-381, FR-382).
 *
 * What is asserted here that nothing else in the tree asserts:
 *
 * 1. **The order is Rust's.** The rows render in the order they arrived, even
 *    when a later row carries a newer activity than an earlier one — the list
 *    never sorts, so a second order cannot exist.
 * 2. **The search is Rust's too.** A needle that appears in no title on screen
 *    still returns its row, because the match happened over message bodies in
 *    SQL; a component that filtered locally would drop that row and pass every
 *    other test in this file.
 * 3. **The count line reads the backend total**, not the number of rows drawn.
 * 4. **Archive is reversible from the UI**: an archived row offers Unarchive and
 *    sends `false`, which is what makes it a column flip rather than a delete.
 * 5. **Delete is confirmed, and the confirmation names the object** and what
 *    goes with it — and deleting the conversation on screen closes it.
 * 6. **The remote-session sentence appears only when there is one**, and only on
 *    the conversation being read.
 * 7. **Two empty states, two sentences**: nothing asked yet, versus nothing
 *    matching a filter.
 */
import { cleanup, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import {
  BOT_SESSION_ACTIONS_LABEL,
  BOT_SESSION_ARCHIVE_LABEL,
  BOT_SESSION_ARCHIVED_MARK,
  BOT_SESSION_DELETE_CANCEL,
  BOT_SESSION_DELETE_CONFIRM,
  BOT_SESSION_DELETE_LABEL,
  BOT_SESSION_LIST_EMPTY,
  BOT_SESSION_LIST_LABEL,
  BOT_SESSION_NO_MATCH,
  BOT_SESSION_READ_FAILED,
  BOT_SESSION_REMOTE_MARK,
  BOT_SESSION_REMOTE_NOTE,
  BOT_SESSION_RENAME_CONFIRM,
  BOT_SESSION_RENAME_FIELD_LABEL,
  BOT_SESSION_RENAME_LABEL,
  BOT_SESSION_SEARCH_LABEL,
  BOT_SESSION_UNARCHIVE_LABEL,
  BotSessionList,
} from "@/components/bots/bot-session-list";
import type {
  BotSessionListVm,
  BotSessionRowVm,
  BotSessionVm,
  BotTranscriptSource,
} from "@/lib/ipc/client";
import { capabilitiesStore, DEFAULT_CAPABILITIES } from "@/lib/stores/capabilities";

const botsSessionsSearch = vi.fn<(req: unknown) => Promise<BotSessionListVm>>();
const botsSessionRename = vi.fn<(id: string, title: string) => Promise<BotSessionVm>>();
const botsSessionArchive = vi.fn<(id: string, archived: boolean) => Promise<BotSessionVm>>();
const botsSessionDelete = vi.fn<(id: string) => Promise<void>>();

vi.mock("@/lib/ipc/client", () => ({
  botsSessionsSearch: (req: unknown) => botsSessionsSearch(req),
  botsSessionRename: (id: string, title: string) => botsSessionRename(id, title),
  botsSessionArchive: (id: string, archived: boolean) => botsSessionArchive(id, archived),
  botsSessionDelete: (id: string) => botsSessionDelete(id),
}));

const NOW = Date.now();

function session(fields: Partial<BotSessionVm> & { id: string; title: string }): BotSessionVm {
  return {
    botId: "bot-1",
    providerId: "prov-1",
    createdMs: NOW - 86_400_000,
    updatedMs: NOW - 3_600_000,
    archived: false,
    remoteSessionId: null,
    // Epic 63's two gateway facts: absent on a row no gateway described.
    remoteLastActiveMs: null,
    remoteSource: null,
    ...fields,
  };
}

function row(
  fields: Partial<BotSessionVm> & { id: string; title: string },
  latestActivityMs: number = NOW - 3_600_000,
  messageCount = 2,
  transcript: BotTranscriptSource = "local",
): BotSessionRowVm {
  return { session: session(fields), latestActivityMs, messageCount, transcript };
}

const ROW_A = row({ id: "s-a", title: "What changed in the drive" }, NOW - 60_000, 4);
const ROW_B = row({ id: "s-b", title: "Draft the release note" }, NOW - 7_200_000, 2);

function answer(rows: BotSessionRowVm[], total: number = rows.length): BotSessionListVm {
  return { rows, total };
}

/** Mount the list, returning the callbacks the pane would have passed. */
function mount({ sessions = [] as BotSessionVm[], openId = null as string | null } = {}) {
  const onOpen = vi.fn();
  const onNew = vi.fn();
  const onChanged = vi.fn();
  const onClosed = vi.fn();
  render(
    <BotSessionList
      sessions={sessions}
      openId={openId}
      onOpen={onOpen}
      onNew={onNew}
      onChanged={onChanged}
      onClosed={onClosed}
    />,
  );
  return { onOpen, onNew, onChanged, onClosed };
}

/** Open one row's overflow menu and hand back the menu. */
async function openMenu(title: string) {
  const trigger = await screen.findByRole("button", {
    name: `${BOT_SESSION_ACTIONS_LABEL} ${title}`,
  });
  // Radix opens on pointer events, not click (the note-actions precedent).
  fireEvent.pointerDown(trigger, { button: 0, ctrlKey: false });
  fireEvent.pointerUp(trigger, { button: 0 });
  return await screen.findByRole("menu");
}

beforeEach(() => {
  botsSessionsSearch.mockReset();
  botsSessionsSearch.mockResolvedValue(answer([ROW_A, ROW_B]));
  botsSessionRename.mockReset();
  botsSessionRename.mockResolvedValue(session({ id: "s-a", title: "Renamed" }));
  botsSessionArchive.mockReset();
  botsSessionArchive.mockResolvedValue(session({ id: "s-a", title: "A", archived: true }));
  botsSessionDelete.mockReset();
  botsSessionDelete.mockResolvedValue(undefined);
});

afterEach(() => {
  vi.clearAllMocks();
});

describe("the list", () => {
  /**
   * The order on screen is the order that arrived. `ROW_B` carries the OLDER
   * activity and is second, and a component that sorted by anything of its own
   * would still pass — so the second half of this test hands back a page whose
   * activities are ascending and asserts the DOM keeps that order anyway.
   */
  it("renders the rows in the order Rust returned them, and never re-sorts", async () => {
    botsSessionsSearch.mockResolvedValue(
      answer([
        row({ id: "s-old", title: "Oldest activity first on purpose" }, NOW - 90_000_000, 1),
        row({ id: "s-new", title: "Newest activity second" }, NOW - 1_000, 1),
      ]),
    );
    mount();

    const list = await screen.findByRole("list", { name: BOT_SESSION_LIST_LABEL });
    await waitFor(() => expect(within(list).getAllByRole("listitem")).toHaveLength(2));
    const titles = within(list)
      .getAllByRole("listitem")
      .map((item) => item.textContent ?? "");
    expect(titles[0]).toContain("Oldest activity first on purpose");
    expect(titles[1]).toContain("Newest activity second");
  });

  /** Clicking a row is continue: it replays from keeper's store, nothing else. */
  it("opens the conversation the row names", async () => {
    const { onOpen } = mount();

    // Anchored: the row's menu trigger is labelled with the same title, and an
    // unanchored pattern matches both.
    fireEvent.click(await screen.findByRole("button", { name: /^What changed in the drive/ }));

    expect(onOpen).toHaveBeenCalledWith("s-a");
  });

  /**
   * The count line is the backend's `total`, which is bigger than the page.
   * A count taken from `rows.length` would read "2 conversations" over a set of
   * forty — the defect `count-label.ts` exists to make unreachable.
   */
  it("counts the matched set and not the rows it drew", async () => {
    botsSessionsSearch.mockResolvedValue(answer([ROW_A, ROW_B], 40));
    mount();

    expect(await screen.findByText("2 of 40 conversations")).toBeInTheDocument();
  });

  /** The activity is shown through the app's own relative-date vocabulary. */
  it("dates a row by its latest activity", async () => {
    botsSessionsSearch.mockResolvedValue(
      answer([row({ id: "s-a", title: "Recent" }, NOW - 5 * 60_000, 1)]),
    );
    mount();

    // The vocabulary is `formatDraftAge`'s, through `Intl.RelativeTimeFormat`,
    // so the exact punctuation is the runtime locale's business — what this
    // asserts is that the row dates itself from the ACTIVITY it was handed.
    expect(await screen.findByText(/5 min/)).toBeInTheDocument();
  });

  /** A read that failed says so where the reader is looking, and stays. */
  it("prints a refusal when the read fails", async () => {
    // An envelope with no sentence of its own, so what lands on screen is the
    // fallback this surface owns rather than a raw error string.
    botsSessionsSearch.mockRejectedValue({});
    mount();

    expect(await screen.findByRole("alert")).toHaveTextContent(BOT_SESSION_READ_FAILED);
  });
});

describe("search", () => {
  /**
   * The needle reaches Rust, and the row that comes back renders even though
   * its title does not contain the needle — because the hit was in a message
   * body. This is the assertion a local filter cannot pass.
   */
  it("sends the query to Rust and renders a hit whose title does not match", async () => {
    mount();
    await screen.findByText(/What changed in the drive/);

    botsSessionsSearch.mockResolvedValue(
      answer([row({ id: "s-body", title: "Tuesday" }, NOW - 60_000, 3)]),
    );
    fireEvent.change(screen.getByRole("textbox", { name: BOT_SESSION_SEARCH_LABEL }), {
      target: { value: "certificate" },
    });

    await waitFor(() =>
      expect(botsSessionsSearch).toHaveBeenCalledWith({
        text: "certificate",
        scope: "live",
        limit: 0,
      }),
    );
    expect(await screen.findByText("Tuesday")).toBeInTheDocument();
    expect(screen.queryByText(/What changed in the drive/)).toBeNull();
  });

  /** The archived filter asks for the archive, and nothing else. */
  it("asks Rust for the archive when the Archived chip is pressed", async () => {
    mount();
    await screen.findByText(/What changed in the drive/);

    fireEvent.click(screen.getByRole("button", { name: "Archived" }));

    await waitFor(() =>
      expect(botsSessionsSearch).toHaveBeenCalledWith({ text: "", scope: "archived", limit: 0 }),
    );
    expect(screen.getByRole("button", { name: "Archived" })).toHaveAttribute(
      "aria-pressed",
      "true",
    );
  });
});

describe("the two empty states", () => {
  it("says nothing has been asked yet when no filter is on", async () => {
    botsSessionsSearch.mockResolvedValue(answer([]));
    mount();

    expect(await screen.findByText(BOT_SESSION_LIST_EMPTY)).toBeInTheDocument();
    expect(screen.queryByText(BOT_SESSION_NO_MATCH)).toBeNull();
  });

  it("says nothing matches when a filter is on — the opposite fact", async () => {
    botsSessionsSearch.mockResolvedValue(answer([]));
    mount();
    await screen.findByText(BOT_SESSION_LIST_EMPTY);

    fireEvent.change(screen.getByRole("textbox", { name: BOT_SESSION_SEARCH_LABEL }), {
      target: { value: "kubernetes" },
    });

    expect(await screen.findByText(BOT_SESSION_NO_MATCH)).toBeInTheDocument();
    expect(screen.queryByText(BOT_SESSION_LIST_EMPTY)).toBeNull();
  });
});

describe("archive", () => {
  it("files a live conversation, and does not delete it", async () => {
    const { onChanged } = mount();
    const menu = await openMenu("What changed in the drive");

    fireEvent.click(within(menu).getByRole("menuitem", { name: BOT_SESSION_ARCHIVE_LABEL }));

    await waitFor(() => expect(botsSessionArchive).toHaveBeenCalledWith("s-a", true));
    expect(botsSessionDelete).not.toHaveBeenCalled();
    await waitFor(() => expect(onChanged).toHaveBeenCalled());
  });

  /**
   * The reversal, from the UI: an archived row's menu offers Unarchive and it
   * sends `false` through the same command. An archive with no way back would
   * pass the test above and fail this one.
   */
  it("takes an archived conversation back out, through the same command", async () => {
    botsSessionsSearch.mockResolvedValue(
      answer([row({ id: "s-filed", title: "Filed away", archived: true }, NOW - 90_000_000, 6)]),
    );
    mount();
    const menu = await openMenu("Filed away");

    expect(within(menu).queryByRole("menuitem", { name: BOT_SESSION_ARCHIVE_LABEL })).toBeNull();
    fireEvent.click(within(menu).getByRole("menuitem", { name: BOT_SESSION_UNARCHIVE_LABEL }));

    await waitFor(() => expect(botsSessionArchive).toHaveBeenCalledWith("s-filed", false));
    expect(botsSessionDelete).not.toHaveBeenCalled();
  });

  /** An archived row is marked, because a widening filter shows both kinds. */
  it("marks an archived row", async () => {
    botsSessionsSearch.mockResolvedValue(
      answer([row({ id: "s-filed", title: "Filed away", archived: true }, NOW - 1_000, 1), ROW_A]),
    );
    mount();

    expect(await screen.findByText(BOT_SESSION_ARCHIVED_MARK)).toBeInTheDocument();
  });
});

describe("delete", () => {
  /**
   * The confirmation names the conversation and what goes with it, and nothing
   * is deleted until it is confirmed — the chain-of-custody rule.
   */
  it("names the conversation and its messages before deleting anything", async () => {
    mount();
    const menu = await openMenu("What changed in the drive");

    fireEvent.click(within(menu).getByRole("menuitem", { name: BOT_SESSION_DELETE_LABEL }));

    const dialog = await screen.findByRole("alertdialog");
    expect(dialog).toHaveTextContent('Delete "What changed in the drive"?');
    expect(dialog).toHaveTextContent("4 messages");
    expect(botsSessionDelete).not.toHaveBeenCalled();

    fireEvent.click(within(dialog).getByRole("button", { name: BOT_SESSION_DELETE_CONFIRM }));
    await waitFor(() => expect(botsSessionDelete).toHaveBeenCalledWith("s-a"));
  });

  /**
   * Story 63.1, FR-412: the body names the machine the store is on, in the
   * tier's own word. The Mac's sentence is the literal it always was; a phone
   * — the reduced tier, `bots` on and every desktop flag off — says "this
   * phone" and never "this Mac".
   */
  it("says which device holds the store, per tier", async () => {
    // The Mac: the desktop tier, which is any hydrated mirror with a desktop
    // flag on.
    capabilitiesStore.getState().applySnapshot({ ...DEFAULT_CAPABILITIES, bots: true, sync: true });
    mount();
    let menu = await openMenu("What changed in the drive");
    fireEvent.click(within(menu).getByRole("menuitem", { name: BOT_SESSION_DELETE_LABEL }));
    let dialog = await screen.findByRole("alertdialog");
    expect(dialog).toHaveTextContent(
      "The conversation and its 4 messages are removed from keeper's own store on this Mac, in one step, and cannot be brought back. Nothing on your drive changes, and the model is not told.",
    );
    cleanup();

    // The phone.
    capabilitiesStore.getState().applySnapshot({ ...DEFAULT_CAPABILITIES, bots: true });
    mount();
    menu = await openMenu("What changed in the drive");
    fireEvent.click(within(menu).getByRole("menuitem", { name: BOT_SESSION_DELETE_LABEL }));
    dialog = await screen.findByRole("alertdialog");
    expect(dialog).toHaveTextContent("removed from keeper's own store on this phone,");
    expect(dialog).not.toHaveTextContent("this Mac");
    capabilitiesStore.setState({ capabilities: DEFAULT_CAPABILITIES, hydrated: false });
  });

  it("deletes nothing when the confirmation is declined", async () => {
    mount();
    const menu = await openMenu("What changed in the drive");
    fireEvent.click(within(menu).getByRole("menuitem", { name: BOT_SESSION_DELETE_LABEL }));

    const dialog = await screen.findByRole("alertdialog");
    fireEvent.click(within(dialog).getByRole("button", { name: BOT_SESSION_DELETE_CANCEL }));

    expect(botsSessionDelete).not.toHaveBeenCalled();
  });

  /** Deleting the conversation on screen closes it, rather than leaving the
   *  pane rendering rows whose conversation no longer exists. */
  it("closes the conversation on screen when that is the one deleted", async () => {
    const { onClosed, onChanged } = mount({ openId: "s-a" });
    const menu = await openMenu("What changed in the drive");
    fireEvent.click(within(menu).getByRole("menuitem", { name: BOT_SESSION_DELETE_LABEL }));
    const dialog = await screen.findByRole("alertdialog");

    fireEvent.click(within(dialog).getByRole("button", { name: BOT_SESSION_DELETE_CONFIRM }));

    await waitFor(() => expect(onClosed).toHaveBeenCalled());
    await waitFor(() => expect(onChanged).toHaveBeenCalled());
  });

  it("leaves the open conversation alone when a different one is deleted", async () => {
    const { onClosed } = mount({ openId: "s-b" });
    const menu = await openMenu("What changed in the drive");
    fireEvent.click(within(menu).getByRole("menuitem", { name: BOT_SESSION_DELETE_LABEL }));
    const dialog = await screen.findByRole("alertdialog");

    fireEvent.click(within(dialog).getByRole("button", { name: BOT_SESSION_DELETE_CONFIRM }));

    await waitFor(() => expect(botsSessionDelete).toHaveBeenCalledWith("s-a"));
    expect(onClosed).not.toHaveBeenCalled();
  });
});

describe("rename", () => {
  it("sends the typed name on Enter, and re-reads afterwards", async () => {
    const { onChanged } = mount();
    const menu = await openMenu("What changed in the drive");

    fireEvent.click(within(menu).getByRole("menuitem", { name: BOT_SESSION_RENAME_LABEL }));
    const field = await screen.findByRole("textbox", { name: BOT_SESSION_RENAME_FIELD_LABEL });
    // Prefilled with the name it is about, so a rename is an edit and not a
    // retype.
    expect(field).toHaveValue("What changed in the drive");
    fireEvent.change(field, { target: { value: "Drive changes" } });
    fireEvent.keyDown(field, { key: "Enter" });

    await waitFor(() => expect(botsSessionRename).toHaveBeenCalledWith("s-a", "Drive changes"));
    await waitFor(() => expect(onChanged).toHaveBeenCalled());
  });

  it("also sends it from the confirm control", async () => {
    mount();
    const menu = await openMenu("What changed in the drive");

    fireEvent.click(within(menu).getByRole("menuitem", { name: BOT_SESSION_RENAME_LABEL }));
    const field = await screen.findByRole("textbox", { name: BOT_SESSION_RENAME_FIELD_LABEL });
    fireEvent.change(field, { target: { value: "Drive changes" } });
    fireEvent.click(screen.getByRole("button", { name: BOT_SESSION_RENAME_CONFIRM }));

    await waitFor(() => expect(botsSessionRename).toHaveBeenCalledWith("s-a", "Drive changes"));
  });

  it("writes nothing when the rename is escaped", async () => {
    mount();
    const menu = await openMenu("What changed in the drive");

    fireEvent.click(within(menu).getByRole("menuitem", { name: BOT_SESSION_RENAME_LABEL }));
    const field = await screen.findByRole("textbox", { name: BOT_SESSION_RENAME_FIELD_LABEL });
    fireEvent.change(field, { target: { value: "Drive changes" } });
    fireEvent.keyDown(field, { key: "Escape" });

    expect(botsSessionRename).not.toHaveBeenCalled();
    await waitFor(() =>
      expect(screen.queryByRole("textbox", { name: BOT_SESSION_RENAME_FIELD_LABEL })).toBeNull(),
    );
  });
});

describe("the remote session id", () => {
  /** Shown on the conversation being read, when one is held. A row with an id
   *  but no session API behind it reads locally, so the sentence is the older
   *  one about the reference. */
  it("says which session the other side calls it, and that it may be gone", async () => {
    botsSessionsSearch.mockResolvedValue(
      answer([row({ id: "s-a", title: "Draft the release note", remoteSessionId: "hermes-9f21" })]),
    );
    mount({ openId: "s-a" });

    expect(await screen.findByText(BOT_SESSION_REMOTE_NOTE("hermes-9f21"))).toBeInTheDocument();
  });

  /** Absent when there is no id — the sentence is a fact about a reference, so
   *  a conversation with no reference has nothing to say. */
  it("says nothing when no remote session is held", async () => {
    botsSessionsSearch.mockResolvedValue(answer([row({ id: "s-a", title: "Local only" })]));
    mount({ openId: "s-a" });
    await screen.findByText("Local only");

    expect(screen.queryByText(/calls this session/)).toBeNull();
  });

  /** And absent on a row that is not the one being read. */
  it("says nothing on a conversation that is not open", async () => {
    botsSessionsSearch.mockResolvedValue(
      answer([row({ id: "s-a", title: "Draft the release note", remoteSessionId: "hermes-9f21" })]),
    );
    mount({ openId: null });
    await screen.findByText("Draft the release note");

    expect(screen.queryByText(/calls this session/)).toBeNull();
  });
});

describe("which is which (AD-181)", () => {
  const REMOTE = row(
    {
      id: "s-r",
      title: "From the phone",
      remoteSessionId: "hermes-9f21",
      remoteSource: "api",
      remoteLastActiveMs: NOW - 5 * 60_000,
    },
    NOW - 3 * 3_600_000,
    0,
    "remote",
  );

  /** A remote row is marked, dated by the gateway's clock, and does not count a
   *  local copy that may hold none of the other device's turns. */
  it("marks a row whose transcript lives on the gateway and dates it by the gateway", async () => {
    botsSessionsSearch.mockResolvedValue(answer([REMOTE, ROW_A]));
    mount();

    const remoteRow = await screen.findByRole("button", { name: /^From the phone/ });
    expect(remoteRow).toHaveTextContent(BOT_SESSION_REMOTE_MARK);
    expect(remoteRow).toHaveTextContent(/5 min/);
    expect(remoteRow).not.toHaveTextContent("0 messages");

    const localRow = screen.getByRole("button", { name: /^What changed in the drive/ });
    expect(localRow).not.toHaveTextContent(BOT_SESSION_REMOTE_MARK);
    expect(localRow).toHaveTextContent("4 messages");
  });

  /** The open remote row says which door wrote it and when it last moved; the
   *  replay sentence is not shown, because nothing was replayed. */
  it("names the gateway session, its writer and its last activity on the open row", async () => {
    botsSessionsSearch.mockResolvedValue(answer([REMOTE]));
    mount({ openId: "s-r" });

    // The age's punctuation is the runtime locale's (`formatDraftAge`); the
    // facts asserted are the id, the door and that an age is named.
    const note = await screen.findByText(/Read from the gateway's session hermes-9f21/);
    expect(note).toHaveTextContent("written via api");
    expect(note).toHaveTextContent(/last active .*5 min/);
    expect(note).toHaveTextContent("Every device you use with this bot writes here");
    expect(screen.queryByText(/replays from its own store/)).toBeNull();
  });

  /** The same id on an endpoint that keeps no session API is a local row, and
   *  says so with the older sentence — the label is `transcript`'s, never the
   *  id's. */
  it("keeps the local sentence for an id the gateway can no longer serve", async () => {
    botsSessionsSearch.mockResolvedValue(
      answer([row({ id: "s-a", title: "Older gateway", remoteSessionId: "hermes-9f21" })]),
    );
    mount({ openId: "s-a" });

    expect(await screen.findByText(BOT_SESSION_REMOTE_NOTE("hermes-9f21"))).toBeInTheDocument();
    expect(screen.queryByText(/Read from the gateway/)).toBeNull();
    expect(screen.getByRole("button", { name: /^Older gateway/ })).toHaveTextContent("2 messages");
  });
});
