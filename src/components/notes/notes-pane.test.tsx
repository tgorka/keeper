import { cleanup, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type {
  NoteCreateReq,
  NoteCreateVm,
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
  // Story 45.16 added both to `NoteVaultVm`. `null` is the shipped default and
  // the one this suite means: no capture template, no capture tag.
  captureTemplate: null,
  captureTag: null,
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

/** How many notes this test has created, so each gets its own id and title. */
let createdCount = 0;
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
  // Zero is "no cap" on the wire (Story 44.11), and it is what a seeded space
  // sends: none of the four carries a `keeper.limit`.
  limit = 0,
): NoteSpaceVm {
  return {
    id,
    name,
    query,
    sort: "modified desc",
    sortEffective: "modified desc",
    limit,
    icon,
    defaultKey,
    // None of the four seeded spaces hands out a template: each of them selects
    // on something that is not a tag, so a template that added one could file a
    // new note straight out of the space that offered it.
    template: null,
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
 * text is parsed and applied, and then its `keeper.limit` caps what the space
 * SELECTS (Story 44.11). Only the `is:` forms the seeded defaults use are
 * understood here, and an unknown one throws rather than quietly matching
 * everything — a fake that shrugged at a query it did not know would turn a
 * broken lens into a green test.
 *
 * `total` is post-cap and `matched` is pre-cap, exactly as `project_list`
 * composes them, and the page is carved out of the selection afterwards — so a
 * test can tell a count of the set from a count of the page.
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
  const cap = stored === undefined || stored.limit === 0 ? rows.length : stored.limit;
  const selected = rows.slice(0, cap);
  const page = query.limit === 0 ? selected.length : query.limit;
  return {
    rows: selected.slice(query.offset, query.offset + page),
    total: selected.length,
    matched: rows.length,
    offset: query.offset,
  };
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

/**
 * `notes_create`, as Rust does it (Story 44.6, FR-160).
 *
 * The fake writes a row into the vault and, when the ask named a space, applies
 * that space's **seed** first — the tags, folder and flags its query needs —
 * exactly as `keeper_core::notes::seed` derives them. So the assertions below
 * are about the row the list then holds, not about a request object: a pane
 * that stopped sending the space id, or that sent the query text instead, gets
 * an unseeded note and fails.
 *
 * The derivation itself is proved in `keeper-core` over the real DSL; this is
 * the four seeded defaults and nothing else. A query it does not know **throws**
 * rather than shrugging, for `matchesSpaceQuery`'s reason: a fake that quietly
 * accepted an unknown lens would turn a broken create into a green test.
 *
 * `notices` is Rust's sentence for a create that could not be what the space
 * asked for. `is:recording` is the story's own example — keeper does not write
 * recordings — so the note exists and the space will not list it.
 */
function create(vaultId: string, req: NoteCreateReq): NoteCreateVm {
  createdCount += 1;
  const id = `new-${createdCount}`;
  const made = row(id, `Untitled ${createdCount}`, []);
  const notices: string[] = [];
  const space = spaceList.find((candidate) => candidate.id === req.space);
  if (req.space !== null && space === undefined) {
    throw new Error(`no such space: ${req.space}`);
  }
  switch (space?.query) {
    case undefined:
    // `is:untagged` needs nothing: a new note has no tags.
    case "is:untagged":
      break;
    case "is:pinned":
      made.pinned = true;
      break;
    case "is:journal":
      made.path = `journal/${id}.md`;
      break;
    case "is:recording":
      notices.push(
        `A new note can't satisfy is:recording, so this note is in the vault but won't appear in ${space.name}.`,
      );
      break;
    default:
      throw new Error(`the fake does not seed: ${space?.query}`);
  }
  contents[vaultId] ??= [];
  contents[vaultId].push(made);
  return { note: { vaultId, id, path: made.path, title: made.title }, notices };
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
    notesCreate: vi.fn(async (vaultId: string, req: NoteCreateReq) => create(vaultId, req)),
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
    notesDeletePlan: vi.fn(async (_vaultId: string, noteId: string) => ({
      path: `${noteId}.md`,
      question: `Delete "${noteId}"?`,
      consequence: `keeper removes ${noteId}.md from this vault.`,
      recovery: "keeper moves it into the vault's trash.",
    })),
    notesSpaceSave: vi.fn(async () => ({
      vaultId: "vault-a",
      id: "space-1",
      path: "spaces/s.md",
      title: "Saved filter",
    })),
  };
});

import {
  COLUMN_COLLAPSE_PREFIX,
  COLUMN_EXPAND_PREFIX,
  COLUMN_RAIL_CONTROL_SLOT,
} from "@/components/layout/surface-column";
import { NOTE_DELETE_CANCEL, NOTE_DELETE_CONFIRM } from "@/components/notes/note-delete-dialog";
import { NOTES_SEARCH_PLACEHOLDER } from "@/components/notes/note-filter-bar";
import {
  NEW_NOTE_LABEL,
  NOTES_COUNT_SLOT,
  NOTES_NOTICE_SLOT,
  NOTES_RAIL_LIST_LABEL,
  NotesPane,
} from "@/components/notes/notes-pane";
import { RESTORE_DEFAULTS } from "@/components/notes/space-list";
import { COLUMN_RESIZER_LABEL } from "@/components/ui/resizable-columns";
import { TooltipProvider } from "@/components/ui/tooltip";
import { WINDOW_ROW_ATTR } from "@/components/ui/window-list";
import { COLUMN_WIDTH_COOKIE, SURFACE_COLUMNS } from "@/lib/column-widths";
import { notesCreate, notesDelete, notesDeletePlan } from "@/lib/ipc/client";
import { COLUMN_FOLD_COOKIE, resetColumnFoldForTest } from "@/lib/stores/column-fold";
import { notesFiltersStore, resetNotesFiltersStoreForTest } from "@/lib/stores/notes-filters";
import { resetNotesListStoreForTest } from "@/lib/stores/notes-list";
import {
  NOTES_RAIL_FOLD_COOKIE,
  notesRailFoldCookie,
  readNotesRailFold,
  resetNotesRailFoldForTest,
} from "@/lib/stores/notes-rail-fold";
import { resetNotesVaultsStoreForTest } from "@/lib/stores/notes-vaults";
import { resetPanelsStoreForTest } from "@/lib/stores/panels";
import { primaryViewStore } from "@/lib/stores/primary-view";
import { type ListGeometry, withListGeometry } from "@/test/layout";

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
  // Copies: a create pushes a row into the active vault's list, and a shared
  // reference would carry that note into every later test in this file.
  contents["vault-a"] = [...ROWS_A];
  contents["vault-b"] = [...ROWS_B];
  spaceList = SEEDED_SPACES;
  createdCount = 0;
  // `a4` is the one note keeper wrote about a recording.
  recordingIds = { a4: true };
  resetNotesVaultsStoreForTest();
  resetNotesListStoreForTest();
  resetNotesFiltersStoreForTest();
  // Story 46.12: the pane hosts the panel strip, so a test can leave a SECOND
  // panel behind and the next one starts with two documents on screen. It was
  // safe to omit while the pane could only ever retarget the one note panel.
  resetPanelsStoreForTest();
  // Story 47.3: the rail's fold is a cookie AND a module-level "already
  // restored" latch, so one test's fold would otherwise be the next test's
  // restore. Both halves have to go.
  resetNotesRailFoldForTest();
  primaryViewStore.getState().setView("notes");
});

/**
 * Story 48.1: this pane's two fixed columns fold and resize. Both halves have
 * to go between tests, like every other cookie-backed fold in this file.
 */
beforeEach(() => {
  resetColumnFoldForTest();
});

afterEach(() => {
  resetNotesVaultsStoreForTest();
  resetNotesListStoreForTest();
  resetNotesFiltersStoreForTest();
  resetPanelsStoreForTest();
  resetNotesRailFoldForTest();
  // biome-ignore lint/suspicious/noDocumentCookie: clearing cookie state this suite arranged
  document.cookie = `${NOTES_RAIL_FOLD_COOKIE}=; path=/; max-age=0`;
  primaryViewStore.getState().setView("inbox");
  resetColumnFoldForTest();
  // biome-ignore lint/suspicious/noDocumentCookie: clearing cookie state this suite arranged
  document.cookie = `${COLUMN_FOLD_COOKIE}=; path=/; max-age=0`;
  // biome-ignore lint/suspicious/noDocumentCookie: clearing cookie state this suite arranged
  document.cookie = `${COLUMN_WIDTH_COOKIE}=; path=/; max-age=0`;
});

/**
 * Story 47.3, and the DW-172 shape again.
 *
 * The restore is `hydrateNotesRailFold`, and the only thing that can fail is
 * the pane not calling it. A store-level test cannot see that: it calls the
 * hydrate itself and passes over a pane that never does. So this one arranges
 * a real cookie, mounts the real pane, and asserts the rail came up the way
 * the cookie says — which is false the moment the call is dropped.
 *
 * Tags is absent here on purpose: this suite's vault has no tags, so the Tags
 * section does not render at all. Its fold is covered in `rail-fold.test.tsx`.
 */
describe("NotesPane rail fold", () => {
  it("comes up folded the way the cookie left it", async () => {
    // biome-ignore lint/suspicious/noDocumentCookie: arranging the cookie is this test's subject
    document.cookie = notesRailFoldCookie({ spaces: true, tags: true, files: false });

    renderPane();
    await waitForRows("Pricing");

    // Spaces folded: the rows are gone, the way back is not.
    expect(await screen.findByRole("button", { name: "Expand Spaces" })).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "Inbox" })).not.toBeInTheDocument();
    expect(screen.getByRole("button", { name: RESTORE_DEFAULTS })).toBeInTheDocument();
    // Files unfolded, against its own default of shut — so this cannot pass on
    // a pane that ignored the cookie and fell back to the defaults.
    expect(screen.getByRole("button", { name: "Collapse Files" })).toBeInTheDocument();
  });
});

/**
 * Story 48.1, and the reported defect: after Story 47.3 the owner wrote back
 * "wciaz tylko pierwsza kolumna jest mozliwa do foldowania" — still only the
 * first column folds. 47.3 folded the SECTIONS inside the rail, which is a
 * different thing from the rail.
 *
 * Asserted against the real pane and not against `useSurfaceColumn`, because
 * the whole defect was a mechanism that existed and was not wired to a column.
 * `surface-column.test.tsx` proves what a folded column does; this proves this
 * pane has two of them.
 */
describe("NotesPane columns", () => {
  const railFold = `${COLUMN_COLLAPSE_PREFIX} ${SURFACE_COLUMNS["notes-rail"].label}`;
  const listFold = `${COLUMN_COLLAPSE_PREFIX} ${SURFACE_COLUMNS["notes-list"].label}`;

  it("folds the rail without taking the list with it", async () => {
    renderPane();
    await waitForRows("Pricing");

    fireEvent.click(screen.getByRole("button", { name: railFold }));

    // The rail's rows are gone — the spaces, the trees, the switcher — and what
    // the rail could DO is on the strip instead. New note is the SAME control at
    // 48px, which is why it is still findable by the words a user reads; before
    // the second cut of this story it was simply absent, which is the defect.
    expect(screen.getByRole("button", { name: NEW_NOTE_LABEL })).toHaveAttribute(
      "data-slot",
      COLUMN_RAIL_CONTROL_SLOT,
    );
    expect(screen.queryByRole("button", { name: "Inbox" })).not.toBeInTheDocument();
    expect(screen.getByRole("button", { name: /Note, Pricing/ })).toBeInTheDocument();
    expect(
      screen.getByRole("button", {
        name: `${COLUMN_EXPAND_PREFIX} ${SURFACE_COLUMNS["notes-rail"].label}`,
      }),
    ).toBeInTheDocument();
  });

  it("folds the list without taking the rail with it", async () => {
    renderPane();
    await waitForRows("Pricing");

    fireEvent.click(screen.getByRole("button", { name: listFold }));

    expect(screen.queryByRole("button", { name: /Note, Pricing/ })).not.toBeInTheDocument();
    expect(screen.getByRole("button", { name: NEW_NOTE_LABEL })).toBeInTheDocument();
  });

  it("puts a seam on each column, and takes the folded one's away", async () => {
    renderPane();
    await waitForRows("Pricing");

    expect(
      screen.getByRole("separator", {
        name: `${COLUMN_RESIZER_LABEL} ${SURFACE_COLUMNS["notes-rail"].label}`,
      }),
    ).toBeInTheDocument();
    const listSeam = `${COLUMN_RESIZER_LABEL} ${SURFACE_COLUMNS["notes-list"].label}`;
    expect(screen.getByRole("separator", { name: listSeam })).toBeInTheDocument();

    fireEvent.click(screen.getByRole("button", { name: listFold }));
    expect(screen.queryByRole("separator", { name: listSeam })).not.toBeInTheDocument();
  });

  /**
   * The two namespaces this story refused to share, proved rather than
   * asserted: folding the COLUMN leaves the SECTIONS as the user left them, so
   * unfolding gives back the rail they had rather than a default one.
   */
  it("does not disturb which rail sections are folded", async () => {
    // biome-ignore lint/suspicious/noDocumentCookie: arranging the cookie is this test's subject
    document.cookie = notesRailFoldCookie({ spaces: true, tags: false, files: false });
    renderPane();
    await waitForRows("Pricing");
    expect(await screen.findByRole("button", { name: "Expand Spaces" })).toBeInTheDocument();

    fireEvent.click(screen.getByRole("button", { name: railFold }));
    fireEvent.click(
      screen.getByRole("button", {
        name: `${COLUMN_EXPAND_PREFIX} ${SURFACE_COLUMNS["notes-rail"].label}`,
      }),
    );

    expect(await screen.findByRole("button", { name: "Expand Spaces" })).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "Inbox" })).not.toBeInTheDocument();
    expect(readNotesRailFold(document.cookie).spaces).toBe(true);
  });

  /**
   * The owner's own words, after 0.8.4 shipped: "foldowane kolumny - te puste
   * pomysl co zrobic zeby jednak elementy z wewnatrz byly osiagalne" — the
   * folded columns are empty, work out how to make what is inside reachable.
   *
   * So: nothing that was reachable unfolded is unreachable folded. The rail is
   * not a second body — the spaces, the trees and the list are unmounted, and
   * `surface-column.test.tsx` holds that line — it is the way to each of them.
   */
  it("leaves the scope column offering the vault, the create and every section", async () => {
    renderPane();
    await waitForRows("Pricing");

    fireEvent.click(screen.getByRole("button", { name: railFold }));

    // Which vault is active is a fact the strip would otherwise destroy, so it
    // rides the control that leads back to the switcher.
    expect(screen.getByRole("button", { name: `Vaults, ${VAULT_A.name}` })).toBeInTheDocument();
    expect(screen.getByRole("button", { name: NEW_NOTE_LABEL })).toBeEnabled();
    for (const section of ["Spaces", "Tags", "Files"]) {
      expect(screen.getByRole("button", { name: section })).toBeInTheDocument();
    }
  });

  it("makes a note from the folded rail without spending the fold", async () => {
    renderPane();
    await waitForRows("Pricing");
    fireEvent.click(screen.getByRole("button", { name: railFold }));

    fireEvent.click(screen.getByRole("button", { name: NEW_NOTE_LABEL }));

    // The create is the one rail control that does its whole job at 48px: the
    // note is written and opened in the strip beside, and the column the user
    // put away stays away.
    await waitFor(() => expect(notesCreate).toHaveBeenCalled());
    expect(
      screen.getByRole("button", {
        name: `${COLUMN_EXPAND_PREFIX} ${SURFACE_COLUMNS["notes-rail"].label}`,
      }),
    ).toBeInTheDocument();
  });

  it("opens the section the rail names, including one the user had folded", async () => {
    // Spaces folded AND the column folded: a control that only unfolded the
    // column would land the user on a section still put away, which is a way in
    // that goes nowhere.
    // biome-ignore lint/suspicious/noDocumentCookie: arranging the cookie is this test's subject
    document.cookie = notesRailFoldCookie({ spaces: true, tags: false, files: false });
    renderPane();
    await waitForRows("Pricing");
    fireEvent.click(screen.getByRole("button", { name: railFold }));

    fireEvent.click(screen.getByRole("button", { name: "Spaces" }));

    expect(await screen.findByRole("button", { name: "Inbox" })).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Collapse Spaces" })).toBeInTheDocument();
  });

  it("leaves the list column offering the search, the count and the way out of a filter", async () => {
    renderPane();
    await waitForRows("Pricing");

    fireEvent.click(screen.getByRole("button", { name: listFold }));

    // The count is what a 48px strip destroys most completely: folded, nothing
    // on screen said whether the lens held four notes or none.
    expect(
      screen.getByRole("button", { name: new RegExp(`^${NOTES_RAIL_LIST_LABEL}, .*notes?$`) }),
    ).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Search notes" })).toBeInTheDocument();
    // Nothing is filtered, so there is nothing to clear and no control for it.
    expect(screen.queryByRole("button", { name: /^Clear filters/ })).not.toBeInTheDocument();
  });

  it("unfolds the list and lands the caret in the search field", async () => {
    renderPane();
    await waitForRows("Pricing");
    fireEvent.click(screen.getByRole("button", { name: listFold }));

    fireEvent.click(screen.getByRole("button", { name: "Search notes" }));

    // "Unfold and put me where I asked to be", in one press. The pane already
    // answered this nonce for the palette; the rail reuses it rather than
    // growing a second focus path.
    const field = await screen.findByRole("searchbox", { name: NOTES_SEARCH_PLACEHOLDER });
    expect(field).toHaveFocus();
  });

  it("clears a filter from the folded list rail", async () => {
    renderPane();
    await waitForRows("Pricing");
    fireEvent.change(screen.getByRole("searchbox", { name: NOTES_SEARCH_PLACEHOLDER }), {
      target: { value: "pricing" },
    });
    fireEvent.click(screen.getByRole("button", { name: listFold }));

    // A filter you can neither see nor clear is worse than one you can: the
    // chips are unmounted with the body, so the strip carries the way out.
    fireEvent.click(screen.getByRole("button", { name: /^Clear filters/ }));

    expect(notesFiltersStore.getState().text).toBe("");
  });
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

/**
 * Story 46.12: the notes list gets the Files tree's gesture pair, and not a
 * contract of its own.
 *
 * Single click replaces what the active panel shows; double click opens beside
 * it. There was no twin here before, because the model refused a second note
 * panel — so this block is the whole of the surface half of "several notes at
 * once", and the mocked editor is enough to read it: one `note-editor` per
 * panel, each told which note it is.
 */
describe("NotesPane opening several notes", () => {
  it("replaces the active panel on a single click", async () => {
    renderPane();
    await waitForRows("Pricing", "Garden");

    fireEvent.click(screen.getByRole("button", { name: /Note, Pricing/ }));
    await waitFor(() => {
      expect(screen.getAllByTestId("note-editor")).toHaveLength(1);
    });
    fireEvent.click(screen.getByRole("button", { name: /Note, Garden/ }));

    await waitFor(() => {
      expect(screen.getByTestId("note-editor")).toHaveAttribute("data-note-id", "a3");
    });
    // Still one panel. That is the whole difference between the two gestures.
    expect(screen.getAllByTestId("note-editor")).toHaveLength(1);
  });

  it("opens a second note beside the first on a double click", async () => {
    renderPane();
    await waitForRows("Pricing", "Garden");

    // A double click is preceded by a real single click, so every gesture below
    // is clicked and then double-clicked — which is what a mouse delivers, and
    // what the store's `replaced` bookkeeping exists to undo.
    //
    // The first note is opened with the pair too, and that is not ceremony: a
    // single click into the panel a fresh keeper starts with records "this
    // displaced nothing", and a run of previews keeps the first such record. So
    // a single-clicked first note is still a preview, and opening a second note
    // beside a preview of nothing correctly fills the frame instead of leaving
    // an empty one. Double-clicking pins it, which is the gesture that says
    // "keep this".
    const pricing = screen.getByRole("button", { name: /Note, Pricing/ });
    fireEvent.click(pricing);
    fireEvent.doubleClick(pricing);
    await waitFor(() => {
      expect(screen.getByTestId("note-editor")).toHaveAttribute("data-note-id", "a1");
    });

    const garden = screen.getByRole("button", { name: /Note, Garden/ });
    fireEvent.click(garden);
    fireEvent.doubleClick(garden);

    await waitFor(() => {
      expect(screen.getAllByTestId("note-editor")).toHaveLength(2);
    });
    // The note that was showing came back rather than being replaced by a
    // second copy of the one that was just opened.
    expect(
      screen.getAllByTestId("note-editor").map((node) => node.getAttribute("data-note-id")),
    ).toEqual(["a1", "a3"]);
  });
});

describe("NotesPane vault switching", () => {
  /**
   * Story 46.12 sharpened this. It used to assert that switching vaults BLANKED
   * the editor and switching back restored it — the note was remembered but not
   * shown, because a single editor slot had to be told which note and this pane
   * would only name one from the active vault.
   *
   * The pane hosts the panel strip now, and the panel holds the note. A vault
   * switch changes what the LIST shows, exactly as a filter does; it is not an
   * instruction to put away a document the reader deliberately opened. Nothing
   * about the note has changed — the vault is still configured, the file is
   * still there — so hiding it would be this surface answering a question the
   * user did not ask.
   */
  it("keeps the open note on screen when the vault changes, and lists the other vault", async () => {
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
    // The other vault is listed, and the panel still holds the note it held.
    expect(screen.getByTestId("note-editor")).toHaveAttribute("data-note-id", "a1");
    expect(screen.getByTestId("note-editor")).toHaveAttribute("data-vault-id", "vault-a");
    // And the row is no longer marked open, because it is not in this list.
    expect(screen.getByRole("button", { name: /Note, Roadmap/ })).not.toHaveAttribute(
      "aria-current",
    );

    const backMenu = await openVaultMenu();
    fireEvent.click(within(backMenu).getByRole("menuitem", { name: /Mind/ }));
    await waitFor(() => {
      expect(screen.getByRole("button", { name: /Note, Pricing/ })).toHaveAttribute(
        "aria-current",
        "true",
      );
    });
    expect(screen.getByTestId("note-editor")).toHaveAttribute("data-note-id", "a1");
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

    // The rail names itself from its own visible title now (Story 48.3), so the
    // region's name is the column's display name rather than a second string.
    const rail = await screen.findByRole("navigation", {
      name: SURFACE_COLUMNS["notes-rail"].title,
    });
    for (const name of ["Inbox", "Journal", "Pinned", "Recordings"]) {
      expect(await within(rail).findByRole("button", { name })).toBeInTheDocument();
    }

    cleanup();
    spaceList = [];
    renderPane();
    const bare = await screen.findByRole("navigation", {
      name: SURFACE_COLUMNS["notes-rail"].title,
    });
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
    const rail = await screen.findByRole("navigation", {
      name: SURFACE_COLUMNS["notes-rail"].title,
    });
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

/** The count line above the note list, or `null` while it shows none. */
function noteCount(): string | null {
  return document.querySelector(`[data-slot="${NOTES_COUNT_SLOT}"]`)?.textContent ?? null;
}

/**
 * Story 44.11 — how many notes this lens holds.
 *
 * The counts under test are all Rust's: the fake `evaluate` above composes
 * `total` and `matched` exactly the way `project_list` does, so a pane that
 * reached for `rows.length` instead would be visibly wrong here rather than
 * accidentally right.
 */
describe("NotesPane — how many notes", () => {
  let geometry: ListGeometry | null = null;

  afterEach(() => {
    geometry?.undo();
    geometry = null;
  });

  it("counts the whole vault, not the rows the window mounted", async () => {
    // The AC's shape: virtualisation ON, and a fixture two orders of magnitude
    // larger than one window. Ten rows fit; four thousand exist.
    const VISIBLE_ROWS = 10;
    geometry = withListGeometry({ viewport: VISIBLE_ROWS * 64, row: 64 });
    contents["vault-a"] = Array.from({ length: 4000 }, (_, index) =>
      row(`n${index}`, `Note ${index}`, []),
    );
    renderPane();
    await waitForRows("Note 0");

    const mounted = document.querySelectorAll(`[${WINDOW_ROW_ATTR}]`).length;
    expect(mounted).toBeLessThan(100);
    expect(noteCount()).toBe(`${(4000).toLocaleString()} notes`);
  });

  it("counts the filtered set, and moves with the filter", async () => {
    renderPane();
    await waitForRows("Pricing", "Standup", "Garden");
    expect(noteCount()).toBe("4 notes");

    fireEvent.change(screen.getByLabelText("Search this vault"), {
      target: { value: "Pricing" },
    });
    await waitFor(() => expect(noteCount()).toBe("1 note"));
  });

  it("says zero rather than hiding the count when nothing matches", async () => {
    renderPane();
    await waitForRows("Pricing");

    fireEvent.change(screen.getByLabelText("Search this vault"), {
      target: { value: "nothing whatsoever" },
    });
    // The empty state replaces the LIST. The count is its sibling, so it is
    // still on screen — a count that vanished exactly when the answer is "none"
    // would never answer the question anyone asks it.
    await screen.findByText("No matches in this vault.");
    expect(noteCount()).toBe("0 notes");
  });

  it("says both numbers when a space's keeper.limit declined some of them", async () => {
    // DW-163's resolution, seen from the surface: `keeper.limit` caps what the
    // space SELECTS, and a cap that bit is never silent. Four untagged notes,
    // an Inbox that holds one.
    contents["vault-a"] = [
      row("u1", "One", []),
      row("u2", "Two", []),
      row("u3", "Three", []),
      row("u4", "Four", []),
    ];
    spaceList = [space("s-inbox", "Inbox", "is:untagged", "inbox", "inbox", 1)];
    renderPane();
    await waitForRows("One");

    fireEvent.click(await screen.findByRole("button", { name: "Inbox" }));

    await waitFor(() => expect(noteCount()).toBe("1 of 4 notes"));
    // And the list holds exactly what the count says it holds: the cap is a
    // selection cap, so the other three are not one scroll away.
    expect(screen.queryByRole("button", { name: /Note, Two/ })).toBeNull();
  });

  it("says one number when the space's cap is larger than what it matched", async () => {
    // A cap nobody reached is not worth two numbers, and `4 of 4` reads as a
    // defect rather than as a fact.
    contents["vault-a"] = [
      row("u1", "One", []),
      row("u2", "Two", []),
      row("u3", "Three", []),
      row("u4", "Four", []),
    ];
    spaceList = [space("s-inbox", "Inbox", "is:untagged", "inbox", "inbox", 500)];
    renderPane();
    await waitForRows("One");

    fireEvent.click(await screen.findByRole("button", { name: "Inbox" }));

    await waitFor(() => expect(noteCount()).toBe("4 notes"));
  });
});

/**
 * Story 44.6, FR-160. Three surfaces create a note — the rail, a space row and
 * the command palette — and the interesting one is the space, because "new note
 * in this space" is a promise about where the note turns up.
 *
 * The palette's `notes-new` and `⌘⌥N` route through the same `createNote` this
 * pane calls, with no space, so the rail's assertion below is theirs too.
 */
describe("NotesPane — new note", () => {
  /**
   * The last thing the pane actually sent, for the one assertion that is about
   * the wire. The mock's call log is not cleared between tests in this file, so
   * the last call is the one this test caused.
   */
  function lastCreate(): [string, NoteCreateReq] {
    const calls = vi.mocked(notesCreate).mock.calls as [string, NoteCreateReq][];
    return calls[calls.length - 1];
  }

  it("creates from the rail into the default list and opens the note", async () => {
    renderPane();
    await waitForRows("Pricing");

    fireEvent.click(screen.getByRole("button", { name: NEW_NOTE_LABEL }));

    // Opened: the pane hands the new id to the editor, which is what puts the
    // caret in its body (`new-note-caret.test.tsx` proves the other half).
    await waitFor(() => {
      expect(screen.getByTestId("note-editor")).toHaveAttribute("data-note-id", "new-1");
    });
    expect(lastCreate()).toEqual(["vault-a", expect.objectContaining({ space: null })]);

    // And it is in the default list. The re-read is a scope change, which is
    // what the app does when the reconciler has not yet streamed the write.
    fireEvent.click(await screen.findByRole("button", { name: "Inbox" }));
    notesFiltersStore.getState().clearAll();
    await waitFor(() => {
      expect(screen.getByRole("button", { name: /Note, Untitled 1/ })).toBeInTheDocument();
    });
  });

  it("creates from a space into that space, carrying the space's id and not its query", async () => {
    renderPane();
    await waitForRows("Pricing");

    // Pinned selects `is:pinned`, and no fixture row is pinned — so the space
    // is empty before the create and holds exactly the new note after it. A
    // create that did not inherit the flag would leave it empty.
    fireEvent.click(screen.getByRole("button", { name: "New note in Pinned" }));
    await waitFor(() => {
      expect(screen.getByTestId("note-editor")).toHaveAttribute("data-note-id", "new-1");
    });
    expect(lastCreate()).toEqual(["vault-a", expect.objectContaining({ space: "s-pinned" })]);

    fireEvent.click(screen.getByRole("button", { name: "Pinned" }));
    await waitFor(() => {
      expect(screen.getByRole("button", { name: /Note, Untitled 1/ })).toBeInTheDocument();
    });
  });

  it("still creates from a space no new note can satisfy, and says it will not appear", async () => {
    renderPane();
    await waitForRows("Pricing");

    fireEvent.click(screen.getByRole("button", { name: "New note in Recordings" }));

    // The sentence is Rust's, so this asserts the slot carries one and names
    // the space — never the wording, which this surface does not compose.
    const notice = await waitFor(() => {
      const found = document.querySelector(`[data-slot="${NOTES_NOTICE_SLOT}"]`);
      expect(found).not.toBeNull();
      return found as HTMLElement;
    });
    expect(notice.textContent).toContain("Recordings");

    // The note exists and is open: declining to file it is not declining to
    // write it.
    expect(screen.getByTestId("note-editor")).toHaveAttribute("data-note-id", "new-1");

    // And Recordings does not list it, which is what the sentence said.
    fireEvent.click(screen.getByRole("button", { name: "Recordings" }));
    await waitFor(() => {
      expect(screen.getByRole("button", { name: /Note, Quarterly review/ })).toBeInTheDocument();
    });
    expect(screen.queryByRole("button", { name: /Note, Untitled 1/ })).not.toBeInTheDocument();
  });

  it("clears a previous create's notice when the next create has nothing to say", async () => {
    renderPane();
    await waitForRows("Pricing");

    fireEvent.click(screen.getByRole("button", { name: "New note in Recordings" }));
    await waitFor(() => {
      expect(document.querySelector(`[data-slot="${NOTES_NOTICE_SLOT}"]`)).not.toBeNull();
    });

    // A stale explanation standing over a note it is not about is worse than
    // no explanation: the second note DID land where it was asked to.
    fireEvent.click(screen.getByRole("button", { name: "New note in Inbox" }));
    await waitFor(() => {
      expect(screen.getByTestId("note-editor")).toHaveAttribute("data-note-id", "new-2");
    });
    expect(document.querySelector(`[data-slot="${NOTES_NOTICE_SLOT}"]`)).toBeNull();
  });

  it("offers no create while no vault is flagged", async () => {
    vaultList = [];
    renderPane();

    await waitFor(() => {
      expect(screen.getByRole("button", { name: NEW_NOTE_LABEL })).toBeDisabled();
    });
  });
});

/**
 * Story 45.17's third door.
 *
 * The editor's menu and the sidebar's space rows are the other two, and both
 * are tested where they live. This one is the list's `Delete` key, and it needs
 * its own tests for the reason the whole wave has been re-learning: a rule with
 * many tests that all enter through one door is untested at every other door.
 */
describe("NotesPane — deleting from the list", () => {
  it("asks about the row under the cursor, and deletes nothing until it is told to", async () => {
    renderPane();
    await waitForRows("Pricing", "Standup");

    // Cursor onto the SECOND row, so "it deleted something" and "it deleted
    // the note you were looking at" cannot be the same assertion.
    const list = screen.getByRole("button", { name: /Note, Pricing/ });
    fireEvent.keyDown(list, { key: "ArrowDown" });
    fireEvent.keyDown(list, { key: "ArrowDown" });
    fireEvent.keyDown(list, { key: "Delete" });

    await waitFor(() => expect(notesDeletePlan).toHaveBeenCalledWith("vault-a", "a2"));
    expect(await screen.findByText('Delete "a2"?')).toBeInTheDocument();
    expect(notesDelete).not.toHaveBeenCalled();
  });

  it("declines without calling the delete, and the row is still listed", async () => {
    renderPane();
    await waitForRows("Pricing", "Standup");

    const list = screen.getByRole("button", { name: /Note, Pricing/ });
    fireEvent.keyDown(list, { key: "ArrowDown" });
    fireEvent.keyDown(list, { key: "Delete" });
    fireEvent.click(await screen.findByRole("button", { name: NOTE_DELETE_CANCEL }));

    await waitFor(() => expect(screen.queryByText('Delete "a1"?')).not.toBeInTheDocument());
    expect(notesDelete).not.toHaveBeenCalled();
    expect(screen.getByRole("button", { name: /Note, Pricing/ })).toBeInTheDocument();
  });

  it("deletes the note it named when the confirmation is taken", async () => {
    renderPane();
    await waitForRows("Pricing", "Standup");

    const list = screen.getByRole("button", { name: /Note, Pricing/ });
    fireEvent.keyDown(list, { key: "ArrowDown" });
    fireEvent.keyDown(list, { key: "Delete" });
    fireEvent.click(await screen.findByRole("button", { name: NOTE_DELETE_CONFIRM }));

    await waitFor(() => expect(notesDelete).toHaveBeenCalledWith("vault-a", "a1"));
  });
});
