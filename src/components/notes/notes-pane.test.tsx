import { cleanup, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type {
  NoteListVm,
  NoteQueryReq,
  NoteRowVm,
  NoteSpaceVm,
  NoteVaultVm,
} from "@/lib/ipc/client";

/**
 * The editor is a sibling's surface and pulls CodeMirror in with it; the pane's
 * contract with it is two props, so the stub renders exactly those and nothing
 * else. `data-note-id` is what the vault-switch assertions read.
 */
vi.mock("@/components/notes/note-editor", () => ({
  NoteEditor: ({ vaultId, noteId }: { vaultId: string; noteId: string | null }) => (
    <div data-testid="note-editor" data-vault-id={vaultId} data-note-id={noteId ?? ""} />
  ),
}));

/**
 * A fake vault backend that answers `notes_list` the way Rust does — tags
 * INTERSECT — so the chip assertions below are about rendered rows rather than
 * about a request object. A test that only inspected the composed query would
 * pass just as happily if the rows never changed.
 */
const VAULT_A: NoteVaultVm = {
  id: "vault-a",
  profileId: "profile-a",
  name: "Mind",
  subfolder: "notes",
  root: "/home/dev/mind/notes",
  indexed: true,
  noteCount: 3,
  unreadCount: 1,
  cadence: { commitIdleMs: 2000, pushIntervalMs: 30000, pushOnBlur: true },
};

const VAULT_B: NoteVaultVm = {
  ...VAULT_A,
  id: "vault-b",
  profileId: "profile-b",
  name: "Work",
  root: "/home/dev/work/notes",
  noteCount: 1,
  unreadCount: 0,
};

function row(id: string, title: string, tags: string[]): NoteRowVm {
  return {
    id,
    path: `${id}.md`,
    title,
    snippet: `${title} body`,
    tags,
    updatedMs: 1_754_000_000_000,
    pinned: false,
    archived: false,
    unread: false,
    conflict: false,
    origin: "",
    headRev: "",
    order: { value: 0, source: "default" },
  };
}

const ROWS_A: NoteRowVm[] = [
  row("a1", "Pricing", ["work", "urgent"]),
  row("a2", "Standup", ["work"]),
  row("a3", "Garden", ["home"]),
  row("a4", "Quarterly review", []),
];

const ROWS_B: NoteRowVm[] = [row("b1", "Roadmap", ["work"])];

/** Vault contents, keyed by vault id. Empty arrays are a legal answer. */
const contents: Record<string, NoteRowVm[]> = {
  "vault-a": ROWS_A,
  "vault-b": ROWS_B,
};

/**
 * The notes whose frontmatter carries `session:`, which is what makes the index
 * flag them `recording` (Story 42.4).
 *
 * Held beside the rows rather than on them because that is where it lives in the
 * real system: the flag is the index's, `NoteRowVm` carries no slot for it, and
 * the only way the surface can ask about it is the request. So a Recordings
 * assertion below fails unless the pane actually sent `is:recording`.
 */
let recordingIds: Record<string, true> = {};

let activeVault = "vault-a";
let vaultList: NoteVaultVm[] = [VAULT_A, VAULT_B];

/**
 * The four spaces keeper seeds into a fresh vault (Story 44.3), in the shape
 * `notes_spaces` returns them. The rail renders these and nothing else, so a
 * test that wants to press Recordings has to have keeper's seed on disk — which
 * is the point: before this story there was a hard-coded row that worked with
 * `notesSpaces` returning `[]`.
 *
 * The `query` strings are the ones `keeper_core::notes::default_spaces` writes,
 * so this fixture and the seeder cannot drift into agreeing about a lens the
 * vault does not actually hold.
 */
const SEEDED_SPACES: NoteSpaceVm[] = [
  space("s-inbox", "Inbox", "is:untagged", "inbox", "inbox"),
  space("s-journal", "Journal", "is:journal", "calendar-days", "journal"),
  space("s-pinned", "Pinned", "is:pinned", "pin", "pinned"),
  space("s-recordings", "Recordings", "is:recording", "video", "recordings"),
];

function space(
  id: string,
  name: string,
  query: string,
  icon: string,
  defaultKey: string | null,
): NoteSpaceVm {
  return {
    id,
    name,
    query,
    sort: "modified desc",
    sortEffective: "modified desc",
    limit: 500,
    icon,
    defaultKey,
    warnings: [],
    order: 0,
    error: null,
  };
}

/** What the vault's `spaces/` currently holds, as the rail will read it. */
let spaceList: NoteSpaceVm[] = SEEDED_SPACES;

/**
 * The predicate `notes_list` applies: tag terms intersect — every `include`
 * present, every `exclude` absent — text is a substring, and every requested
 * flag must be one the entry carries. No row here carries any flag but
 * `recording`, exactly as `has_flag` would report.
 *
 * A `spaceId` is resolved the way Rust resolves it: the space's stored query
 * text is parsed and applied. Only the `is:` forms the seeded defaults use are
 * understood here, and an unknown one throws rather than quietly matching
 * everything — a fake that shrugged at a query it did not know would turn a
 * broken lens into a green test.
 */
function evaluate(vaultId: string, query: NoteQueryReq): NoteListVm {
  const stored = spaceList.find((candidate) => candidate.id === query.spaceId);
  if (query.spaceId !== null && stored === undefined) {
    throw new Error(`no such space: ${query.spaceId}`);
  }
  const rows = (contents[vaultId] ?? []).filter((candidate) => {
    for (const [tag, term] of Object.entries(query.tags)) {
      if (candidate.tags.includes(tag) !== (term === "include")) {
        return false;
      }
    }
    if (query.text !== null && !candidate.title.toLowerCase().includes(query.text.toLowerCase())) {
      return false;
    }
    if (!query.flags.every((flag) => flag === "recording" && candidate.id in recordingIds)) {
      return false;
    }
    return stored === undefined || matchesSpaceQuery(stored.query, candidate);
  });
  return { rows, total: rows.length, offset: 0 };
}

/** The `is:` predicates the seeded defaults store, evaluated over a row. */
function matchesSpaceQuery(dsl: string, candidate: NoteRowVm): boolean {
  switch (dsl) {
    case "is:untagged":
      return candidate.tags.length === 0;
    case "is:journal":
      return candidate.path.startsWith("journal/");
    case "is:pinned":
      return candidate.pinned;
    case "is:recording":
      return candidate.id in recordingIds;
    default:
      throw new Error(`the fake does not evaluate: ${dsl}`);
  }
}

vi.mock("@/lib/ipc/client", async (importOriginal) => {
  const actual = await importOriginal<typeof import("@/lib/ipc/client")>();
  return {
    ...actual,
    notesVaults: vi.fn(async () => vaultList),
    notesVaultActive: vi.fn(async () => activeVault),
    notesVaultSetActive: vi.fn(async (vaultId: string) => {
      activeVault = vaultId;
    }),
    notesList: vi.fn(async (vaultId: string, query: NoteQueryReq) => evaluate(vaultId, query)),
    notesTree: vi.fn(async () => ({ relDir: "", dirs: [], notes: [] })),
    notesTagTree: vi.fn(async () => ({ nodes: [] })),
    notesSpaces: vi.fn(async () => spaceList),
    notesSpacesRestoreDefaults: vi.fn(async () => 0),
    notesSubscribeChanges: vi.fn(async () => "sub-1"),
    notesUnsubscribeChanges: vi.fn(async () => undefined),
    notesCreate: vi.fn(async () => ({
      vaultId: "vault-a",
      id: "a4",
      path: "a4.md",
      title: "Untitled",
    })),
    notesJournalToday: vi.fn(async () => ({
      vaultId: "vault-a",
      id: "journal",
      path: "journal/today.md",
      title: "Today",
    })),
    notesSetFlag: vi.fn(async () => undefined),
    notesMarkRead: vi.fn(async () => undefined),
    notesReveal: vi.fn(async () => undefined),
    notesDelete: vi.fn(async () => undefined),
    notesSpaceSave: vi.fn(async () => ({
      vaultId: "vault-a",
      id: "space-1",
      path: "spaces/s.md",
      title: "Saved filter",
    })),
  };
});

import { NotesPane } from "@/components/notes/notes-pane";
import { TooltipProvider } from "@/components/ui/tooltip";
import { resetNotesFiltersStoreForTest } from "@/lib/stores/notes-filters";
import { resetNotesListStoreForTest } from "@/lib/stores/notes-list";
import { resetNotesVaultsStoreForTest } from "@/lib/stores/notes-vaults";
import { primaryViewStore } from "@/lib/stores/primary-view";

function renderPane() {
  return render(
    <TooltipProvider>
      <NotesPane />
    </TooltipProvider>,
  );
}

/** Wait until the first list read has painted its rows. */
async function waitForRows(...titles: string[]) {
  await waitFor(() => {
    for (const title of titles) {
      expect(
        screen.getByRole("button", { name: new RegExp(`Note, ${title}`) }),
      ).toBeInTheDocument();
    }
  });
}

/** Open the vault switcher's menu. Radix opens on pointer-down, not on click. */
async function openVaultMenu() {
  const trigger = await screen.findByRole("button", { name: /^Vault / });
  fireEvent.pointerDown(trigger, { button: 0, ctrlKey: false });
  fireEvent.pointerUp(trigger, { button: 0 });
  return await screen.findByRole("menu");
}

beforeEach(() => {
  activeVault = "vault-a";
  vaultList = [VAULT_A, VAULT_B];
  contents["vault-a"] = ROWS_A;
  contents["vault-b"] = ROWS_B;
  spaceList = SEEDED_SPACES;
  // `a4` is the one note keeper wrote about a recording.
  recordingIds = { a4: true };
  resetNotesVaultsStoreForTest();
  resetNotesListStoreForTest();
  resetNotesFiltersStoreForTest();
  primaryViewStore.getState().setView("notes");
});

afterEach(() => {
  resetNotesVaultsStoreForTest();
  resetNotesListStoreForTest();
  resetNotesFiltersStoreForTest();
  primaryViewStore.getState().setView("inbox");
});

describe("NotesPane filters", () => {
  it("intersects tag chips, and clearing one widens the result", async () => {
    renderPane();
    await waitForRows("Pricing", "Standup", "Garden");

    // One chip: everything tagged `work`.
    fireEvent.click(screen.getAllByRole("button", { name: "Tag work, on this note" })[0]);
    await waitFor(() => {
      expect(screen.queryByRole("button", { name: /Note, Garden/ })).not.toBeInTheDocument();
    });
    expect(screen.getByRole("button", { name: /Note, Pricing/ })).toBeInTheDocument();
    expect(screen.getByRole("button", { name: /Note, Standup/ })).toBeInTheDocument();

    // Two chips intersect — AND, never OR. `Standup` carries `work` but not
    // `urgent`, so a union would leave it on screen and this would fail.
    fireEvent.click(screen.getByRole("button", { name: "Tag urgent, on this note" }));
    await waitFor(() => {
      expect(screen.queryByRole("button", { name: /Note, Standup/ })).not.toBeInTheDocument();
    });
    expect(screen.getByRole("button", { name: /Note, Pricing/ })).toBeInTheDocument();

    // Clearing one chip WIDENS: `Standup` comes back, `Garden` stays excluded.
    fireEvent.click(screen.getByRole("button", { name: "Clear tag urgent filter" }));
    await waitFor(() => {
      expect(screen.getByRole("button", { name: /Note, Standup/ })).toBeInTheDocument();
    });
    expect(screen.queryByRole("button", { name: /Note, Garden/ })).not.toBeInTheDocument();
  });

  it("keeps the open note open when a filter excludes its row", async () => {
    renderPane();
    await waitForRows("Garden");

    fireEvent.click(screen.getByRole("button", { name: /Note, Garden/ }));
    await waitFor(() => {
      expect(screen.getByTestId("note-editor")).toHaveAttribute("data-note-id", "a3");
    });

    // `Garden` is not tagged `work`, so this filter excludes the open note.
    fireEvent.click(screen.getAllByRole("button", { name: "Tag work, on this note" })[0]);
    await waitFor(() => {
      expect(screen.queryByRole("button", { name: /Note, Garden/ })).not.toBeInTheDocument();
    });
    // The row is gone from the list; the note is still open (UX-DR41).
    expect(screen.getByTestId("note-editor")).toHaveAttribute("data-note-id", "a3");
  });
});

describe("NotesPane vault switching", () => {
  it("does not clear the open note when the vault changes", async () => {
    renderPane();
    await waitForRows("Pricing");

    fireEvent.click(screen.getByRole("button", { name: /Note, Pricing/ }));
    await waitFor(() => {
      expect(screen.getByTestId("note-editor")).toHaveAttribute("data-note-id", "a1");
    });

    const menu = await openVaultMenu();
    fireEvent.click(within(menu).getByRole("menuitem", { name: /Work/ }));
    await waitFor(() => {
      expect(screen.getByRole("button", { name: /Note, Roadmap/ })).toBeInTheDocument();
    });
    // The other vault is listed and the note is not on screen — but it was not
    // closed, which is what the switch back proves.
    expect(screen.getByTestId("note-editor")).toHaveAttribute("data-note-id", "");

    const backMenu = await openVaultMenu();
    fireEvent.click(within(backMenu).getByRole("menuitem", { name: /Mind/ }));
    await waitFor(() => {
      expect(screen.getByTestId("note-editor")).toHaveAttribute("data-note-id", "a1");
    });
  });
});

describe("NotesPane empty states", () => {
  it("invites a first note when the vault is empty, and widening when a filter excludes everything", async () => {
    contents["vault-a"] = [];
    renderPane();

    await screen.findByText("This vault is empty. Write the first note.");
    expect(screen.queryByText("No notes match these filters.")).not.toBeInTheDocument();

    // Same empty list, different reason — and therefore a different sentence.
    contents["vault-a"] = ROWS_A;
    fireEvent.change(screen.getByRole("searchbox", { name: "Search this vault" }), {
      target: { value: "nothing matches this" },
    });
    await screen.findByText("No matches in this vault.");
    expect(
      screen.queryByText("This vault is empty. Write the first note."),
    ).not.toBeInTheDocument();
  });

  it("offers Settings → Sync, not an empty list, when no folder is a vault", async () => {
    vaultList = [];
    renderPane();

    await screen.findByText(
      "No notes vault yet. Flag a folder you already sync and it becomes one.",
    );
    fireEvent.click(screen.getByRole("button", { name: "Open Settings → Sync" }));
    expect(primaryViewStore.getState().view).toBe("sync");
  });
});

describe("NotesPane rail", () => {
  /**
   * The whole of AD-79 in one assertion. Before this story the four rows were
   * `<Button>`s built from a `SCOPE_ROWS` array in this file's component and
   * `notesSpaces` returned `[]`; now every one of them is a row the space list
   * drew from what the vault holds. Emptying the vault's `spaces/` therefore
   * empties the rail, which a hard-coded row could not do.
   */
  it("renders the four defaults as spaces, and nothing when the vault has none", async () => {
    renderPane();

    const rail = await screen.findByRole("navigation", { name: "Notes" });
    for (const name of ["Inbox", "Journal", "Pinned", "Recordings"]) {
      expect(await within(rail).findByRole("button", { name })).toBeInTheDocument();
    }

    cleanup();
    spaceList = [];
    renderPane();
    const bare = await screen.findByRole("navigation", { name: "Notes" });
    await waitFor(() => {
      expect(within(bare).queryByRole("button", { name: "Inbox" })).not.toBeInTheDocument();
    });
    for (const name of ["Journal", "Pinned", "Recordings"]) {
      expect(within(bare).queryByRole("button", { name })).not.toBeInTheDocument();
    }
  });

  /**
   * AD-80. Today never filtered anything and is deleted rather than ported. The
   * *action* it performed is not: `⌘⌥J`, the tray and the palette still open
   * today's journal entry, and none of them is in this pane.
   */
  it("has no Today row, and no row that opens a note instead of filtering", async () => {
    renderPane();
    const rail = await screen.findByRole("navigation", { name: "Notes" });
    await within(rail).findByRole("button", { name: "Inbox" });

    expect(within(rail).queryByRole("button", { name: "Today" })).not.toBeInTheDocument();
    expect(within(rail).queryByText("Today")).not.toBeInTheDocument();
  });

  it("lists exactly the notes keeper wrote about a recording", async () => {
    renderPane();
    await waitForRows("Pricing", "Quarterly review");

    fireEvent.click(await screen.findByRole("button", { name: "Recordings" }));

    // `a4` carries `session:`; `a1` does not, and no tag, folder or filename
    // convention distinguishes them — only the seeded space's own `is:recording`
    // does, sent as a `spaceId` and evaluated against the space the vault holds.
    await waitFor(() => {
      expect(screen.queryByRole("button", { name: /Note, Pricing/ })).not.toBeInTheDocument();
    });
    expect(screen.getByRole("button", { name: /Note, Quarterly review/ })).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: /Note, Garden/ })).not.toBeInTheDocument();

    // It is a filter like every other row, so it is dismissible in place and
    // widening brings the rest back rather than needing a second visit.
    fireEvent.click(screen.getByRole("button", { name: "Clear Recordings scope" }));
    await waitFor(() => {
      expect(screen.getByRole("button", { name: /Note, Pricing/ })).toBeInTheDocument();
    });
  });

  it("selects the unfiled through the seeded Inbox, which is a note and not a table here", async () => {
    renderPane();
    await waitForRows("Pricing", "Quarterly review");

    fireEvent.click(await screen.findByRole("button", { name: "Inbox" }));

    // `a4` is the only row with no tags. The store sends no flag at all now —
    // `is:untagged` is a string in the vault — so this passes only if the space
    // reached the query.
    await waitFor(() => {
      expect(screen.queryByRole("button", { name: /Note, Pricing/ })).not.toBeInTheDocument();
    });
    expect(screen.getByRole("button", { name: /Note, Quarterly review/ })).toBeInTheDocument();
  });

  it("says the vault has no recordings rather than blaming a filter the user did not set", async () => {
    recordingIds = {};
    renderPane();
    await waitForRows("Pricing");

    fireEvent.click(await screen.findByRole("button", { name: "Recordings" }));

    await screen.findByText(
      "No recording notes yet. keeper writes one each time a recording stops.",
    );
    // The generic sentence would send someone hunting for a chip to remove.
    expect(screen.queryByText("No notes match these filters.")).not.toBeInTheDocument();

    // And it is not a dead end: the one action returns to the whole vault.
    fireEvent.click(screen.getByRole("button", { name: "Show all notes" }));
    await waitFor(() => {
      expect(screen.getByRole("button", { name: /Note, Pricing/ })).toBeInTheDocument();
    });
  });

  /**
   * The sentence follows the marker keeper wrote, not the name. A default is
   * renameable like any other space (AD-79), and a Recordings space someone
   * called "Sessions" is still the one that can say who writes recording notes.
   */
  it("keeps the recordings sentence after the space is renamed, and does not lend it out", async () => {
    recordingIds = {};
    spaceList = [
      space("s-recordings", "Sessions", "is:recording", "video", "recordings"),
      // A space of the user's own, called Recordings, carrying no marker.
      space("s-mine", "Recordings", "is:pinned", "star", null),
    ];
    renderPane();
    await waitForRows("Pricing");

    fireEvent.click(await screen.findByRole("button", { name: "Sessions" }));
    await screen.findByText(
      "No recording notes yet. keeper writes one each time a recording stops.",
    );

    fireEvent.click(screen.getByRole("button", { name: "Show all notes" }));
    await waitForRows("Pricing");
    fireEvent.click(screen.getByRole("button", { name: "Recordings" }));
    // No row is pinned, so this selects nothing — and it gets the ordinary
    // sentence, because it is not keeper's Recordings space.
    await screen.findByText("No notes match these filters.");
  });
});
