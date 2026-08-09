import { fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { NoteListVm, NoteQueryReq, NoteRowVm, NoteVaultVm } from "@/lib/ipc/client";

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
 * The predicate `notes_list` applies: tags intersect, text is a substring, and
 * every requested flag must be one the entry carries. No row here carries any
 * flag but `recording`, exactly as `has_flag` would report.
 */
function evaluate(vaultId: string, query: NoteQueryReq): NoteListVm {
  const rows = (contents[vaultId] ?? []).filter((candidate) => {
    if (!query.tags.every((tag) => candidate.tags.includes(tag))) {
      return false;
    }
    if (query.text !== null && !candidate.title.toLowerCase().includes(query.text.toLowerCase())) {
      return false;
    }
    if (!query.flags.every((flag) => flag === "recording" && candidate.id in recordingIds)) {
      return false;
    }
    return true;
  });
  return { rows, total: rows.length, offset: 0 };
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
    notesSpaces: vi.fn(async () => []),
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

describe("NotesPane Recordings lens", () => {
  it("lists exactly the notes keeper wrote about a recording", async () => {
    renderPane();
    await waitForRows("Pricing", "Quarterly review");

    fireEvent.click(screen.getByRole("button", { name: "Recordings" }));

    // `a4` carries `session:`; `a1` does not, and no tag, folder or filename
    // convention distinguishes them — only the flag the request asked for.
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

  it("says the vault has no recordings rather than blaming a filter the user did not set", async () => {
    recordingIds = {};
    renderPane();
    await waitForRows("Pricing");

    fireEvent.click(screen.getByRole("button", { name: "Recordings" }));

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
});
