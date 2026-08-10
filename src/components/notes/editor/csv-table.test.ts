import { EditorState } from "@codemirror/state";
import { EditorView } from "@codemirror/view";
import { beforeEach, describe, expect, it, vi } from "vitest";
import type * as IpcClient from "@/lib/ipc/client";
import type { NoteCsvVm } from "@/lib/ipc/client";
import {
  CSV_CELL_LABEL,
  CSV_MISSING_CLASS,
  CSV_RAGGED_CLASS,
  CsvTableWidget,
  isCsvTarget,
  renderCsvTableInto,
} from "./csv-table";
import { livePreview } from "./live-preview";

/** What the renderer's own widget — the one `live-preview` constructs, with no
 *  seam for a test to inject through — reaches for. */
const readCsv = vi.fn<typeof IpcClient.notesCsvRead>();
const writeCell = vi.fn<typeof IpcClient.notesCsvSetCell>();

vi.mock("@/lib/ipc/client", async (importOriginal) => ({
  ...(await importOriginal<typeof IpcClient>()),
  notesCsvRead: (vaultId: string, target: string) => readCsv(vaultId, target),
  notesCsvSetCell: (
    vaultId: string,
    target: string,
    rev: string,
    row: number,
    column: number,
    value: string,
  ) => writeCell(vaultId, target, rev, row, column, value),
}));

// jsdom does no layout, so CodeMirror's measure pass would throw out of the
// test on the first frame. Same shim, same reason, as `recording-embed.test.ts`.
if (!Range.prototype.getClientRects) {
  Range.prototype.getClientRects = () =>
    Object.assign([] as DOMRect[], { item: () => null }) as unknown as DOMRectList;
  Range.prototype.getBoundingClientRect = () => new DOMRect();
}

const VAULT = "vault-1";
const TARGET = "attachments/people.csv";

/** The table Rust answers with for a clean two-by-three file. */
function table(overrides: Partial<NoteCsvVm> = {}): NoteCsvVm {
  return {
    relPath: TARGET,
    rev: "rev-1",
    columns: 2,
    totalRows: 3,
    rows: [
      { index: 0, line: 1, cells: ["name", "note"], ragged: false },
      { index: 1, line: 2, cells: ["Doe, Jane", "ok"], ragged: false },
      { index: 2, line: 3, cells: ["Roe", "also ok"], ragged: false },
    ],
    notices: [],
    ...overrides,
  };
}

/** Every cell of the rendered table, row by row, so an assertion can be about
 *  the whole grid rather than about one lucky element. */
function grid(host: HTMLElement): string[][] {
  return [...host.querySelectorAll("tr")].map((row) =>
    [...row.querySelectorAll("th, td")].map((cell) => cell.textContent ?? ""),
  );
}

function cellAt(host: HTMLElement, row: number, column: number): HTMLElement {
  const cell = host.querySelector<HTMLElement>(`[data-row="${row}"][data-column="${column}"]`);
  if (cell === null) {
    throw new Error(`no cell at ${row},${column}`);
  }
  return cell;
}

/// The one editing input, or a named failure. A `!` here would turn "the click
/// opened no editor" — the defect these tests exist to catch — into a
/// TypeError several lines further on, blaming the assignment instead.
function editing(host: HTMLElement, label?: string): HTMLInputElement {
  const selector = label === undefined ? "input" : `input[aria-label="${label}"]`;
  const input = host.querySelector<HTMLInputElement>(selector);
  if (input === null) {
    throw new Error(`no cell is being edited (${selector})`);
  }
  return input;
}

/** Let the fired-and-forgotten render settle. */
async function settled(): Promise<void> {
  await Promise.resolve();
  await Promise.resolve();
  await Promise.resolve();
}

beforeEach(() => {
  readCsv.mockReset();
  writeCell.mockReset();
});

describe("isCsvTarget", () => {
  it("claims a csv embed whatever case the export used, and nothing else", () => {
    expect(isCsvTarget("attachments/people.csv")).toBe(true);
    expect(isCsvTarget("EXPORT.CSV")).toBe(true);
    expect(isCsvTarget("attachments/clip.mov")).toBe(false);
    expect(isCsvTarget("notes/csv")).toBe(false);
    expect(isCsvTarget("csv.md")).toBe(false);
  });
});

describe("renderCsvTableInto", () => {
  it("draws every row and every cell the backend described", async () => {
    const host = document.createElement("div");
    await renderCsvTableInto(host, VAULT, TARGET, { read: async () => table() });

    expect(grid(host)).toEqual([
      ["name", "note"],
      ["Doe, Jane", "ok"],
      ["Roe", "also ok"],
    ]);
    // The first record is the header, and it is still a cell: keeper cannot
    // know it is one, and a typo in it must be fixable.
    expect(host.querySelectorAll("th")).toHaveLength(2);
    expect(cellAt(host, 0, 0).tagName).toBe("TH");
  });

  it("shows a ragged row with the fields it has, marked, and never padded", async () => {
    const host = document.createElement("div");
    await renderCsvTableInto(host, VAULT, TARGET, {
      read: async () =>
        table({
          columns: 3,
          rows: [
            { index: 0, line: 1, cells: ["a", "b", "c"], ragged: false },
            { index: 1, line: 2, cells: ["1", "2"], ragged: true },
            { index: 2, line: 3, cells: ["3", "4", "5", "6"], ragged: true },
          ],
          notices: ["2 of 3 rows do not have 3 fields"],
        }),
    });

    const rows = [...host.querySelectorAll("tr")];
    expect(rows[1].classList.contains(CSV_RAGGED_CLASS)).toBe(true);
    // Two real cells plus one drawn as absent — not three editable blanks.
    expect(rows[1].querySelectorAll(`.${CSV_MISSING_CLASS}`)).toHaveLength(1);
    expect(rows[1].querySelectorAll("[data-column]")).toHaveLength(2);
    // The wide row keeps its fourth field rather than losing it.
    expect(rows[2].querySelectorAll("[data-column]")).toHaveLength(4);
    expect(host.querySelector('[role="status"]')?.textContent).toContain("2 of 3 rows");
  });

  it("keeps the link and shows the reason when the file cannot be read", async () => {
    const host = document.createElement("div");
    await renderCsvTableInto(host, VAULT, TARGET, {
      read: async () => {
        throw { message: "people.csv is 9 MB, and keeper opens a CSV as a table up to 4 MB" };
      },
    });

    // Never an empty box: the reason, and the wikilink it was before.
    expect(host.querySelector('[role="alert"]')?.textContent).toContain("9 MB");
    expect(host.querySelector("a")?.textContent).toBe(TARGET);
    expect(host.querySelector("table")).toBeNull();
  });

  it("says an empty file has no rows rather than rendering nothing", async () => {
    const host = document.createElement("div");
    await renderCsvTableInto(host, VAULT, TARGET, {
      read: async () => table({ columns: 0, totalRows: 0, rows: [] }),
    });

    expect(host.textContent).toContain("has no rows");
  });

  it("sends an edited cell with the revision it read, and repaints the answer", async () => {
    const host = document.createElement("div");
    const setCell = vi.fn(async () =>
      table({
        rev: "rev-2",
        rows: [
          { index: 0, line: 1, cells: ["name", "note"], ragged: false },
          { index: 1, line: 2, cells: ["Roe, Richard", "ok"], ragged: false },
          { index: 2, line: 3, cells: ["Roe", "also ok"], ragged: false },
        ],
      }),
    );
    await renderCsvTableInto(host, VAULT, TARGET, { read: async () => table(), setCell });

    cellAt(host, 1, 0).dispatchEvent(new MouseEvent("click"));
    const input = editing(host, CSV_CELL_LABEL);
    expect(input.value).toBe("Doe, Jane");
    input.value = "Roe, Richard";
    input.dispatchEvent(new FocusEvent("blur"));
    await settled();

    expect(setCell).toHaveBeenCalledWith(VAULT, TARGET, "rev-1", 1, 0, "Roe, Richard");
    // One cell changed and the others are the bytes they were.
    expect(grid(host)).toEqual([
      ["name", "note"],
      ["Roe, Richard", "ok"],
      ["Roe", "also ok"],
    ]);
  });

  it("sends an unchanged cell too, because whether to write is Rust's decision", async () => {
    const host = document.createElement("div");
    const setCell = vi.fn(async () => table());
    await renderCsvTableInto(host, VAULT, TARGET, { read: async () => table(), setCell });

    cellAt(host, 1, 1).dispatchEvent(new MouseEvent("click"));
    const input = editing(host);
    input.dispatchEvent(new FocusEvent("blur"));
    await settled();

    // A short-circuit here would be a second copy of `set_cell`'s
    // value-comparison rule, and the copy that never runs is the one that rots.
    expect(setCell).toHaveBeenCalledWith(VAULT, TARGET, "rev-1", 1, 1, "ok");
  });

  it("abandons an edit on Escape without asking Rust to write anything", async () => {
    const host = document.createElement("div");
    const setCell = vi.fn(async () => table());
    await renderCsvTableInto(host, VAULT, TARGET, { read: async () => table(), setCell });

    cellAt(host, 1, 0).dispatchEvent(new MouseEvent("click"));
    const input = editing(host);
    input.value = "typed then thought better of it";
    input.dispatchEvent(new KeyboardEvent("keydown", { key: "Escape", bubbles: true }));
    await settled();

    expect(setCell).not.toHaveBeenCalled();
    expect(cellAt(host, 1, 0).textContent).toBe("Doe, Jane");
  });

  it("shows a refused write and puts the cell back to what is on disk", async () => {
    const host = document.createElement("div");
    const setCell = vi.fn(async () => {
      throw { message: "people.csv changed on disk since this table was opened" };
    });
    await renderCsvTableInto(host, VAULT, TARGET, { read: async () => table(), setCell });

    cellAt(host, 1, 0).dispatchEvent(new MouseEvent("click"));
    const input = editing(host);
    input.value = "never lands";
    input.dispatchEvent(new FocusEvent("blur"));
    await settled();

    expect(host.querySelector('[role="alert"]')?.textContent).toContain("changed on disk");
    // The table shows the file, not the edit that did not land.
    expect(cellAt(host, 1, 0).textContent).toBe("Doe, Jane");
  });
});

describe("CsvTableWidget", () => {
  it("renders the ordinary link first, then puts the table in its place", async () => {
    const widget = new CsvTableWidget(VAULT, TARGET, { read: async () => table() });
    const dom = widget.toDOM();

    expect(dom.querySelector("a")?.textContent).toBe(TARGET);
    await settled();
    expect(grid(dom)[1]).toEqual(["Doe, Jane", "ok"]);
  });

  it("drops a table that resolved after the widget was destroyed", async () => {
    let answer: (value: NoteCsvVm) => void = () => {};
    const widget = new CsvTableWidget(VAULT, TARGET, {
      read: async () => await new Promise<NoteCsvVm>((resolve) => (answer = resolve)),
    });
    const dom = widget.toDOM();
    widget.destroy();
    answer(table());
    await settled();

    // The link stands; nothing is written into DOM CodeMirror has thrown away.
    expect(dom.querySelector("table")).toBeNull();
  });

  it("reuses the DOM for the same embed, so the caret moving cannot lose a cell", () => {
    const one = new CsvTableWidget(VAULT, TARGET);
    expect(one.eq(new CsvTableWidget(VAULT, TARGET))).toBe(true);
    expect(one.eq(new CsvTableWidget(VAULT, "attachments/other.csv"))).toBe(false);
    expect(one.eq(new CsvTableWidget("vault-2", TARGET))).toBe(false);
  });

  it("claims a cell's events from CodeMirror and gives up the link's", () => {
    const widget = new CsvTableWidget(VAULT, TARGET);
    const cell = document.createElement("td");
    cell.className = "cm-csv-cell";
    const anchor = document.createElement("a");

    // A claimed event is one CodeMirror runs no handler for. Letting a click on
    // a cell through would reveal the line, and a revealed line drops its
    // decorations — so the click would destroy the table instead of editing it.
    // An untargeted event has `target === null`, which is the "not a cell" case.
    expect(widget.ignoreEvent(new MouseEvent("click"))).toBe(false);
    expect(widget.ignoreEvent({ target: cell } as unknown as Event)).toBe(true);
    expect(widget.ignoreEvent({ target: anchor } as unknown as Event)).toBe(false);
  });
});

describe("the renderer", () => {
  it("turns a csv embed into a table and leaves an ordinary wikilink alone", async () => {
    readCsv.mockResolvedValue(table());
    const parent = document.createElement("div");
    document.body.append(parent);
    const view = new EditorView({
      parent,
      state: EditorState.create({
        doc: `intro\n\n![[${TARGET}]]\n\n[[people.md]]\n`,
        extensions: [
          livePreview({
            vaultId: VAULT,
            assetUrl: (rel) => rel,
            onOpenLink: () => {},
            recordingSession: () => null,
          }),
        ],
      }),
    });
    await settled();

    expect(readCsv).toHaveBeenCalledWith(VAULT, TARGET);
    expect(view.contentDOM.querySelector(".cm-csv-block")).not.toBeNull();
    // The plain wikilink on the third line is still a link, not a second table.
    expect(view.contentDOM.querySelectorAll(".cm-csv-block")).toHaveLength(1);
    view.destroy();
  });
});
