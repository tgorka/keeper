/**
 * The `![[…csv]]` embed rendered as a table you can edit (Story 44.16, FR-172).
 *
 * **This file holds no CSV grammar.** Not one `split(",")`, not one quote rule,
 * not one line-ending decision. Quoting, embedded newlines and separators, the
 * byte-order mark and the trailing newline all live in `keeper-core`'s
 * `notes::csv`, which records byte spans and splices one field at a time — so a
 * file the user only looked at comes back byte-identical, and an edited cell
 * moves its own bytes and no others. A TypeScript parser here would be the
 * second opinion that makes that promise unkeepable: what the table showed and
 * what got written would be two answers to the same question.
 *
 * So the contract across IPC is deliberately narrow. Down: "row 4, column 2 is
 * now this", plus the revision the table was read at. Up: decoded cells, the
 * coordinates they came from, and whatever finished sentences Rust has about
 * the file. The webview cannot spell the file's quoting, which is exactly why
 * it cannot reformat it.
 *
 * **No second embed syntax.** `![[target]]` is the one embed this app has, the
 * one the attachments panel writes and the one Obsidian reads; a `.csv` target
 * gets a table and everything else is untouched. The target is passed to Rust
 * verbatim — the webview never joins a vault root to a subpath (AD-65).
 *
 * **Degrading, never an empty box.** Every failure — a file the vault does not
 * have, one too large to table, one that is not UTF-8, a revision that moved
 * underneath — renders the ordinary wikilink with Rust's sentence above it.
 * That is `MermaidWidget`'s rule (UX-DR44) and it is the same rule here for the
 * same reason: an empty box tells the user their data is gone when the file on
 * disk is intact.
 *
 * **One cell is editable at a time.** A five-hundred-row table is five hundred
 * rows of DOM, not five hundred rows of `<input>`; the input is created when a
 * cell is entered and removed when it commits. A ragged row's missing columns
 * are drawn as absent rather than as empty cells, because an empty cell invites
 * an edit keeper will refuse: it does not add a field to a row it did not write.
 */
import { WidgetType } from "@codemirror/view";
import { type NoteCsvVm, notesCsvRead, notesCsvSetCell } from "@/lib/ipc/client";

/** The extension that makes an embed a table. Lower-cased before comparison —
 *  `DATA.CSV` came off somebody's Windows export and is the same file. */
const CSV_EXTENSION = ".csv";

/** Whether this embed target names a CSV. The only classification this file
 *  does, and it is about the *embed*, not about the file's kind — 43.5's
 *  `kind_for_file_name` stays the one answer to what a file IS. */
export function isCsvTarget(target: string): boolean {
  return target.toLowerCase().endsWith(CSV_EXTENSION);
}

/** What a cell's editor is announced as, so the control has a name. */
export const CSV_CELL_LABEL = "Edit cell";

/** The class a row carries when its field count is not the header's. */
export const CSV_RAGGED_CLASS = "cm-csv-ragged";

/** The class a column a ragged row does not have carries. */
export const CSV_MISSING_CLASS = "cm-csv-missing";

/** How the widget reaches the backend. Injected so the degrade paths — which
 *  are the interesting ones — are reachable without a Tauri host. */
export interface CsvTableOptions {
  /** Read the table. Defaults to the `notes_csv_read` command. */
  read?: (vaultId: string, target: string) => Promise<NoteCsvVm>;
  /** Write one cell. Defaults to the `notes_csv_set_cell` command. */
  setCell?: (
    vaultId: string,
    target: string,
    rev: string,
    row: number,
    column: number,
    value: string,
  ) => Promise<NoteCsvVm>;
  /** Whether this render has been abandoned (the widget was destroyed). */
  cancelled?: () => boolean;
}

/** The ordinary wikilink: what the embed renders as before it resolves, and
 *  what it stays as when it resolves to nothing the vault has. */
function link(target: string): HTMLElement {
  const anchor = document.createElement("a");
  anchor.className = "cm-lp-wikilink";
  anchor.textContent = target;
  return anchor;
}

/** Rust's sentence, in the place the reader is already looking. */
function notice(text: string, alert: boolean): HTMLElement {
  const paragraph = document.createElement("p");
  paragraph.className = alert ? "cm-csv-error" : "cm-csv-notice";
  paragraph.setAttribute("role", alert ? "alert" : "status");
  // `textContent`, never `innerHTML`: this text came off a file the user did
  // not necessarily write.
  paragraph.textContent = text;
  return paragraph;
}

/** The message a rejected IPC call carries, or a last-resort description. */
function reasonOf(error: unknown): string {
  if (typeof error === "object" && error !== null && "message" in error) {
    const { message } = error as { message: unknown };
    if (typeof message === "string" && message !== "") {
      return message;
    }
  }
  return String(error);
}

/**
 * Render `target`'s table into `host`, replacing whatever it held.
 *
 * Never rejects. A failure is a rendering outcome, not an exception for
 * somebody else: the degraded node — the link, plus why — is the point.
 */
export async function renderCsvTableInto(
  host: HTMLElement,
  vaultId: string,
  target: string,
  options: CsvTableOptions = {},
): Promise<void> {
  const read = options.read ?? notesCsvRead;
  let table: NoteCsvVm;
  try {
    table = await read(vaultId, target);
  } catch (error) {
    if (options.cancelled?.() === true) {
      return;
    }
    host.replaceChildren(notice(reasonOf(error), true), link(target));
    return;
  }
  if (options.cancelled?.() === true) {
    return;
  }
  paint(host, vaultId, target, table, options);
}

/** Draw one state of the table. Called again with the answer to every edit, so
 *  what is on screen is always a table Rust just described. */
function paint(
  host: HTMLElement,
  vaultId: string,
  target: string,
  table: NoteCsvVm,
  options: CsvTableOptions,
  failure?: string,
): void {
  const children: HTMLElement[] = [];
  if (failure !== undefined) {
    children.push(notice(failure, true));
  }
  for (const sentence of table.notices) {
    children.push(notice(sentence, false));
  }

  const element = document.createElement("table");
  element.className = "cm-csv-table";
  const body = document.createElement("tbody");
  for (const row of table.rows) {
    body.append(rowElement(host, vaultId, target, table, row, options));
  }
  element.append(body);
  children.push(element);

  if (table.rows.length === 0) {
    // An empty file is a real file and a real answer. Saying "this file has no
    // rows" is not the same as rendering nothing, which reads as a failure.
    children.push(notice(`${table.relPath} has no rows`, false));
  }

  host.replaceChildren(...children);
}

/** One record. The first is drawn as the header because that is what the first
 *  record of a CSV almost always is — and it stays editable, because keeper has
 *  no way to know it is one and a header with a typo is a cell like any other. */
function rowElement(
  host: HTMLElement,
  vaultId: string,
  target: string,
  table: NoteCsvVm,
  row: NoteCsvVm["rows"][number],
  options: CsvTableOptions,
): HTMLElement {
  const element = document.createElement("tr");
  if (row.ragged) {
    element.classList.add(CSV_RAGGED_CLASS);
    // The row says what is odd about it, on itself, so the reader does not have
    // to count columns to find the one the notice is about.
    element.title = `line ${row.line}: ${row.cells.length} of ${table.columns} fields`;
  }

  row.cells.forEach((value, column) => {
    const cell = document.createElement(row.index === 0 ? "th" : "td");
    cell.className = "cm-csv-cell";
    cell.dataset.row = String(row.index);
    cell.dataset.column = String(column);
    cell.tabIndex = 0;
    cell.textContent = value;
    cell.addEventListener("click", () => {
      beginEdit(host, vaultId, target, table, cell, options);
    });
    cell.addEventListener("keydown", (event) => {
      if (event.key === "Enter") {
        event.preventDefault();
        beginEdit(host, vaultId, target, table, cell, options);
      }
    });
    element.append(cell);
  });

  // A row shorter than the header is drawn short, with the absence marked.
  // Padding it with empty cells would make it look like a row with blanks in
  // it, which is a different file from the one on disk.
  for (let column = row.cells.length; column < table.columns; column += 1) {
    const missing = document.createElement(row.index === 0 ? "th" : "td");
    missing.className = CSV_MISSING_CLASS;
    missing.setAttribute("aria-label", "no field");
    element.append(missing);
  }

  return element;
}

/** Put an input in the cell. Exactly one exists at a time: the previous one has
 *  already committed and been removed by its own blur. */
function beginEdit(
  host: HTMLElement,
  vaultId: string,
  target: string,
  table: NoteCsvVm,
  cell: HTMLElement,
  options: CsvTableOptions,
): void {
  if (cell.querySelector("input") !== null) {
    return;
  }
  const before = cell.textContent ?? "";
  const input = document.createElement("input");
  input.type = "text";
  input.className = "cm-csv-input";
  input.setAttribute("aria-label", CSV_CELL_LABEL);
  input.value = before;
  cell.replaceChildren(input);
  input.focus();

  let settled = false;
  const finish = (value: string | null): void => {
    if (settled) {
      return;
    }
    settled = true;
    if (value === null) {
      cell.replaceChildren();
      cell.textContent = before;
      return;
    }
    cell.replaceChildren();
    cell.textContent = value;
    void commit(host, vaultId, target, table, cell, value, options);
  };

  input.addEventListener("blur", () => {
    finish(input.value);
  });
  input.addEventListener("keydown", (event) => {
    if (event.key === "Enter") {
      event.preventDefault();
      finish(input.value);
      cell.focus();
    }
    if (event.key === "Escape") {
      event.preventDefault();
      // Escape restores what was there. No write, no round trip: an abandoned
      // edit is not an edit.
      finish(null);
      cell.focus();
    }
  });
}

/**
 * Send one cell's new value and repaint from the answer.
 *
 * The value is sent even when it looks unchanged. Whether a write happens is
 * one decision and it is Rust's — `set_cell` compares against the parsed field
 * and returns the file untouched when they match. A short-circuit here would
 * be a second copy of that rule, and the copy that never runs is the one that
 * rots.
 */
async function commit(
  host: HTMLElement,
  vaultId: string,
  target: string,
  table: NoteCsvVm,
  cell: HTMLElement,
  value: string,
  options: CsvTableOptions,
): Promise<void> {
  const setCell = options.setCell ?? notesCsvSetCell;
  const row = Number(cell.dataset.row);
  const column = Number(cell.dataset.column);
  try {
    const next = await setCell(vaultId, target, table.rev, row, column, value);
    if (options.cancelled?.() === true) {
      return;
    }
    paint(host, vaultId, target, next, options);
  } catch (error) {
    if (options.cancelled?.() === true) {
      return;
    }
    // The refusal is Rust's sentence, and the table is repainted from the last
    // state keeper actually confirmed — so the cell shows what is on disk
    // rather than the edit that did not land.
    paint(host, vaultId, target, table, options, reasonOf(error));
  }
}

/**
 * The CodeMirror widget that replaces a `![[….csv]]` embed.
 *
 * An **inline** replace whose host is styled `display: block`, which is the
 * same shape `RecordingEmbedWidget` uses and the only one available: these
 * decorations come from a `ViewPlugin`, and CodeMirror refuses a block
 * decoration from a plugin (DW-165). An embed is one line, so the inline form
 * costs nothing here — a mermaid fence is several, which is why that widget
 * asks for `block: true` and throws for it.
 */
export class CsvTableWidget extends WidgetType {
  /** Set by {@link destroy}, read by the fetch that may still be in flight. */
  private disposed = false;

  constructor(
    private readonly vaultId: string,
    private readonly target: string,
    private readonly options: CsvTableOptions = {},
  ) {
    super();
  }

  /** Same vault and same target, same table: CodeMirror may reuse the DOM,
   *  which is what keeps a half-typed cell alive while the caret moves. */
  eq(other: CsvTableWidget): boolean {
    return other.vaultId === this.vaultId && other.target === this.target;
  }

  toDOM(): HTMLElement {
    const host = document.createElement("div");
    host.className = "cm-csv-block";
    host.append(link(this.target));
    // Fired and forgotten, exactly as the mermaid fence is: the link is in the
    // document immediately and the table takes its place when the file is read.
    // Blocking `toDOM` on an IPC round trip would stall the editor on every
    // keystroke that rebuilds the decorations.
    void renderCsvTableInto(host, this.vaultId, this.target, {
      ...this.options,
      cancelled: () => this.disposed || this.options.cancelled?.() === true,
    });
    return host;
  }

  destroy(): void {
    this.disposed = true;
  }

  /**
   * Keep the events aimed at a cell.
   *
   * `true` means CodeMirror ignores the event entirely, including the
   * renderer's own wikilink handler. A cell has to keep its clicks and keys:
   * letting them through would put the caret on the line, and a revealed line
   * drops its decorations — so clicking a cell would destroy the table instead
   * of editing it, and typing into the input would type into the note. The same
   * trade `RecordingEmbedWidget` makes for its controls. Everything else gives
   * its events up, so the degraded link behaves like the wikilink it is.
   */
  ignoreEvent(event: Event): boolean {
    return event.target instanceof Element && event.target.closest(".cm-csv-cell, input") !== null;
  }
}
