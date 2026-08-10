/**
 * `![[…]]` over a data file, rendered and editable inside the note
 * (Story 45.12, FR-186, FR-187, UX-DR75).
 *
 * # What is real here and what is not
 *
 * The IPC line is mocked, because these are the states a real vault produces on
 * demand and cannot produce on request. Everything below it is the shipped
 * thing: 45.2's registry, 45.4's `RawRenderedView`, 45.6's `useTextBuffer` and
 * its real CodeMirror, 44.16's table, and — in the renderer tests — a real
 * `EditorView` carrying the real `livePreview`.
 *
 * # The blocks that moved here from `csv-table.test.ts`
 *
 * The widget's own behaviour (link first, then the panel; a resolve after
 * destroy is dropped; `eq`; `ignoreEvent`) and the renderer assembly around it
 * were 44.16's. They are asserted here because the widget is here, and because
 * they are now claims about every embedded data format rather than about CSV.
 */
import { EditorState } from "@codemirror/state";
import { EditorView } from "@codemirror/view";
import { act, fireEvent, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { VIEW_MODE_COOKIE } from "@/components/viewers/view-mode";
import type * as IpcClient from "@/lib/ipc/client";
import type { NoteCsvVm, NoteEmbedVm, TextFileVm } from "@/lib/ipc/client";
import { withRangeRects } from "@/test/layout";

const embedRead = vi.fn<typeof IpcClient.notesEmbedRead>();
const embedWrite = vi.fn<typeof IpcClient.notesEmbedWrite>();
const csvRead = vi.fn<typeof IpcClient.notesCsvRead>();
const csvSetCell = vi.fn<typeof IpcClient.notesCsvSetCell>();

vi.mock("@/lib/ipc/client", async (importOriginal) => ({
  ...(await importOriginal<typeof IpcClient>()),
  notesEmbedRead: (vaultId: string, target: string) => embedRead(vaultId, target),
  notesEmbedWrite: (vaultId: string, target: string, content: string) =>
    embedWrite(vaultId, target, content),
  notesCsvRead: (vaultId: string, target: string) => csvRead(vaultId, target),
  notesCsvSetCell: (
    vaultId: string,
    target: string,
    rev: string,
    row: number,
    column: number,
    value: string,
  ) => csvSetCell(vaultId, target, rev, row, column, value),
}));

import { embedEntryFor, FileEmbedWidget } from "./file-embed";
import { mountNoteFileEmbed } from "./file-embed-host";
import { livePreview } from "./live-preview";

const VAULT = "vault-1";
const CSV_TARGET = "attachments/people.csv";
const JSON_TARGET = "attachments/config.json";

/** The two-by-three CSV Rust answers with, cells only — the bytes never cross
 *  IPC as a document, which is the whole of 44.16's contract. */
function table(overrides: Partial<NoteCsvVm> = {}): NoteCsvVm {
  return {
    relPath: CSV_TARGET,
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

function file(overrides: Partial<TextFileVm> = {}): TextFileVm {
  const text = overrides.text ?? 'name,note\r\n"Doe, Jane",ok\r\nRoe,also ok\r\n';
  return {
    text,
    sizeBytes: text.length,
    sizeLabel: `${text.length} bytes`,
    oversize: false,
    binary: false,
    detail: null,
    ...overrides,
  };
}

function embed(overrides: Partial<NoteEmbedVm> = {}): NoteEmbedVm {
  return {
    relPath: CSV_TARGET,
    name: "people.csv",
    kind: "file",
    file: file(),
    ...overrides,
  };
}

/** Ten microtasks inside `act`, which is what a dynamic import, an IPC round
 *  trip and a React commit take between them. */
async function settle(): Promise<void> {
  await act(async () => {
    for (let tick = 0; tick < 10; tick += 1) {
      await Promise.resolve();
    }
  });
}

/** The modifier CodeMirror's `Mod-s` resolves to **in this test environment**.
 *
 *  A constant, not a browser check. `src/test/no-user-agent-gating.test.ts`
 *  forbids asking the browser which platform it is anywhere under `src/`. It is
 *  also the honest value: jsdom presents itself as something other than a Mac,
 *  so CodeMirror binds `Mod` to Ctrl, and a Cmd-flagged event would match
 *  nothing, assert nothing, and still pass. */
const MOD = { ctrlKey: true };

/** Every mounted panel's teardown, drained after each test. A React root left
 *  attached would keep answering timers and IPC promises into the next test. */
const mounts: (() => void)[] = [];
let undoRects: (() => void) | null = null;

/** Mount the panel the way the widget does, into a node this test owns. */
async function panel(target: string, vaultId = VAULT): Promise<HTMLElement> {
  const container = document.createElement("div");
  document.body.append(container);
  const mounted = mountNoteFileEmbed(container, { vaultId, target });
  mounts.push(() => {
    mounted.unmount();
    container.remove();
  });
  await settle();
  return container;
}

/**
 * The panel's raw editor, once it is really there.
 *
 * `waitFor` rather than a tick count: `TextEditorSurface` mounts CodeMirror
 * through a dynamic import, so how many microtasks it takes depends on whether
 * an earlier test in this file already warmed that module — which makes a
 * fixed-tick version pass in a full run and fail when run alone.
 */
async function sourceEditor(host: HTMLElement): Promise<HTMLElement> {
  const tab = [...host.querySelectorAll('[role="tab"]')].find(
    (each) => each.textContent === "Source",
  );
  if (tab !== undefined && tab.getAttribute("aria-selected") !== "true") {
    fireEvent.click(tab);
  }
  await waitFor(() => expect(host.querySelector(".cm-content")).not.toBeNull());
  await settle();
  return host.querySelector(".cm-content") as HTMLElement;
}

beforeEach(() => {
  embedRead.mockReset();
  embedWrite.mockReset();
  csvRead.mockReset();
  csvSetCell.mockReset();
  // A default answer for the tests that mount a CSV panel to reach its SOURCE
  // pane: the rendered half reads through 44.16 whether or not a test is
  // asserting about it, and an unconfigured `vi.fn()` resolves to `undefined`,
  // which surfaces as an unhandled rejection several tests later.
  csvRead.mockResolvedValue(table());
  // 45.4 remembers the chosen view per format in a cookie, and `document.cookie`
  // is one jar for the whole file — so a test that clicks Source changes which
  // pane the NEXT test's panel opens on. Cleared, so every test states its own
  // starting view instead of inheriting one.
  // biome-ignore lint/suspicious/noDocumentCookie: clearing is the intent
  document.cookie = `${VIEW_MODE_COOKIE}=; max-age=0; path=/`;
  // jsdom has no `Range.getClientRects`, and CodeMirror's measure pass calls it
  // on any animation frame that elapses during a test. The throw lands outside
  // every `try` a test can write and takes the run's exit code with it.
  undoRects = withRangeRects();
});

afterEach(() => {
  for (const unmount of mounts.splice(0)) {
    unmount();
  }
  undoRects?.();
});

describe("which embeds get a panel", () => {
  it("is the registry's answer and not a list in this module", () => {
    expect(embedEntryFor("attachments/people.csv")?.format).toBe("csv");
    expect(embedEntryFor("EXPORT.CSV")?.format).toBe("csv");
    expect(embedEntryFor("attachments/config.json")?.format).toBe("json");
    expect(embedEntryFor("rows.jsonl")?.format).toBe("jsonl");
    // Same format under its other spelling, because the registry says so and
    // this module never learned that `.ndjson` exists.
    expect(embedEntryFor("rows.ndjson")?.format).toBe("jsonl");
  });

  it("leaves alone everything a panel would be the wrong answer for", () => {
    // A note transclusion is a different feature: mounting a raw editor over a
    // note here would be a second way to write one, without `notes_save`'s base
    // revision or its conflict copy.
    expect(embedEntryFor("Weekly review.md")).toBeNull();
    // No rendered half — a toggle showing the same bytes twice.
    expect(embedEntryFor("notes/readme.txt")).toBeNull();
    expect(embedEntryFor("src/main.rs")).toBeNull();
    // Media and documents belong to their own viewers.
    expect(embedEntryFor("attachments/clip.mov")).toBeNull();
    expect(embedEntryFor("attachments/report.pdf")).toBeNull();
    // Not an extension at all.
    expect(embedEntryFor("notes/csv")).toBeNull();
    expect(embedEntryFor("csv.md")).toBeNull();
  });
});

describe("a CSV embed", () => {
  it("renders the table through 44.16, from the vault coordinates a note has", async () => {
    embedRead.mockResolvedValue(embed());
    csvRead.mockResolvedValue(table());
    const host = await panel(CSV_TARGET);

    // The claim the epic asked to be confirmed by eye: a Files panel cannot get
    // here, because `notes_csv_read` takes a NOTES VAULT id and a panel holds a
    // sync profile id. Inside a note there is a vault id, so the table renders.
    expect(csvRead).toHaveBeenCalledWith(VAULT, CSV_TARGET);
    expect(
      [...host.querySelectorAll("tr")].map((row) =>
        [...row.querySelectorAll("th, td")].map((cell) => cell.textContent),
      ),
    ).toEqual([
      ["name", "note"],
      ["Doe, Jane", "ok"],
      ["Roe", "also ok"],
    ]);
  });

  it("writes an edited cell as coordinates, never as a re-serialised file", async () => {
    embedRead.mockResolvedValue(embed());
    csvRead.mockResolvedValue(table());
    csvSetCell.mockImplementation(async (_v, _t, _r, row, column, value) => {
      const next = table({ rev: "rev-2" });
      next.rows[row] = { ...next.rows[row], cells: [...next.rows[row].cells] };
      next.rows[row].cells[column] = value;
      return next;
    });
    const host = await panel(CSV_TARGET);

    const cell = host.querySelector<HTMLElement>('[data-row="1"][data-column="0"]');
    fireEvent.click(cell as HTMLElement);
    const input = host.querySelector("input") as HTMLInputElement;
    fireEvent.change(input, { target: { value: "Doe, Janet" } });
    fireEvent.keyDown(input, { key: "Enter" });
    await settle();

    // Row, column, value and the revision the table was read at — and no
    // document. The webview cannot spell this file's quoting, which is exactly
    // why it cannot reformat it; `keeper-core::notes::csv` splices one field's
    // bytes and copies every other byte, proved by that crate's
    // `an_edited_cell_moves_its_own_bytes_and_no_others`.
    expect(csvSetCell).toHaveBeenCalledWith(VAULT, CSV_TARGET, "rev-1", 1, 0, "Doe, Janet");
    expect(csvSetCell).toHaveBeenCalledTimes(1);

    // The untouched rows are the ones Rust described, unchanged, character for
    // character — including the comma inside a quoted field, which is the cell
    // a re-serialising writer would have mangled.
    const rows = [...host.querySelectorAll("tr")].map((row) =>
      [...row.querySelectorAll("th, td")].map((cell) => cell.textContent),
    );
    expect(rows[0]).toEqual(["name", "note"]);
    expect(rows[2]).toEqual(["Roe", "also ok"]);
    expect(rows[1]).toEqual(["Doe, Janet", "ok"]);
  });

  it("re-reads the file after a cell lands, so the source pane is not stale", async () => {
    embedRead.mockResolvedValue(embed());
    csvRead.mockResolvedValue(table());
    csvSetCell.mockResolvedValue(table({ rev: "rev-2" }));
    const host = await panel(CSV_TARGET);
    expect(embedRead).toHaveBeenCalledTimes(1);

    fireEvent.click(host.querySelector('[data-row="1"][data-column="1"]') as HTMLElement);
    const input = host.querySelector("input") as HTMLInputElement;
    fireEvent.change(input, { target: { value: "still ok" } });
    fireEvent.keyDown(input, { key: "Enter" });
    await settle();

    expect(embedRead).toHaveBeenCalledTimes(2);
  });
});

describe("a JSON embed", () => {
  it("renders the structure, and the toggle offers the source beside it", async () => {
    embedRead.mockResolvedValue(
      embed({
        relPath: JSON_TARGET,
        name: "config.json",
        file: file({ text: '{"port": 8080, "host": "merope"}' }),
      }),
    );
    const host = await panel(JSON_TARGET);

    expect(host.textContent).toContain("port");
    expect(host.textContent).toContain("8080");
    expect(host.textContent).toContain("merope");
    const tabs = [...host.querySelectorAll('[role="tab"]')].map((tab) => tab.textContent);
    expect(tabs).toEqual(["Structure", "Source"]);
    // No CSV command was called for a JSON file: the rendered half is the
    // registry row's, not a guess made here.
    expect(csvRead).not.toHaveBeenCalled();
  });

  it("names the line when the file will not parse, and keeps the source editable", async () => {
    embedRead.mockResolvedValue(
      embed({
        relPath: JSON_TARGET,
        name: "config.json",
        file: file({ text: '{\n "port": oops\n}' }),
      }),
    );
    const host = await panel(JSON_TARGET);

    const alert = host.querySelector('[role="alert"]');
    expect(alert?.textContent).toContain("line 2");
    await waitFor(() => expect(host.querySelector(".cm-content")).not.toBeNull());
  });
});

describe("an edit in the raw view", () => {
  it("saves the exact bytes, terminators and all, through the vault writer", async () => {
    embedRead.mockResolvedValue(
      embed({
        relPath: "attachments/rows.jsonl",
        name: "rows.jsonl",
        file: file({ text: '{"a":1}\r\n{"a":2}\r\n' }),
      }),
    );
    embedWrite.mockResolvedValue(undefined);
    const host = await panel("attachments/rows.jsonl");

    // Source, not the structure: the raw half is the one that writes.
    const editor = await sourceEditor(host);

    // Edited IN PLACE, by position. Replacing the whole document with a CRLF
    // string would re-introduce the terminators as ordinary characters and hide
    // the thing being asserted: what has to survive is the text the editor was
    // CONSTRUCTED with, which is where a normalising buffer does its damage.
    await act(async () => {
      const view = EditorView.findFromDOM(editor);
      const at = view?.state.doc.toString().indexOf("2") ?? -1;
      view?.dispatch({ changes: { from: at, to: at + 1, insert: "3" } });
    });
    await settle();
    fireEvent.keyDown(editor, { key: "s", ...MOD });
    await settle();

    expect(embedWrite).toHaveBeenCalledWith(
      VAULT,
      "attachments/rows.jsonl",
      '{"a":1}\r\n{"a":3}\r\n',
    );
  });

  it("passes the target the note spells, never a path joined here", async () => {
    embedRead.mockResolvedValue(embed({ relPath: CSV_TARGET, name: "people.csv" }));
    await panel("people.csv");

    // The bare name goes to Rust as written; forming `attachments/people.csv`
    // in the webview would be joining a vault root to a subpath (AD-65), and
    // Rust answered with the path it actually read.
    expect(embedRead).toHaveBeenCalledWith(VAULT, "people.csv");
  });
});

describe("an embed whose file has moved", () => {
  it("says so where the embed is, in the sentence that names what keeper looked for", async () => {
    // Rust's own wording, from `keeper_core::notes::embed::not_found_notice`.
    embedRead.mockRejectedValue({
      message:
        "people.csv: this note embeds a file the vault does not have — keeper looked for people.csv and attachments/people.csv",
    });
    const host = await panel("people.csv");

    const alert = host.querySelector('[role="alert"]');
    expect(alert?.textContent).toContain("keeper looked for");
    expect(alert?.textContent).toContain("attachments/people.csv");
    // Not an empty box, and not an editor over bytes that do not exist.
    expect(host.querySelector(".cm-content")).toBeNull();
    expect(host.querySelector("table")).toBeNull();
  });
});

describe("two embeds of one file", () => {
  it("stay in step after an edit to one, even spelled differently", async () => {
    // The same file under two spellings, which is what a person and the
    // attachments panel each write. Rust resolves both to one `relPath`, and
    // that is what the two panels agree on.
    embedRead.mockImplementation(async (_vault, target) =>
      embed({
        relPath: CSV_TARGET,
        name: "people.csv",
        file: file({ text: target === CSV_TARGET ? "a,b\n1,2\n" : "a,b\n1,2\n" }),
      }),
    );
    embedWrite.mockResolvedValue(undefined);
    const first = await panel(CSV_TARGET);
    const second = await panel("people.csv");
    expect(embedRead).toHaveBeenCalledTimes(2);

    const editor = await sourceEditor(first);
    await act(async () => {
      const view = EditorView.findFromDOM(editor);
      view?.dispatch({ changes: { from: 0, to: view.state.doc.length, insert: "a,b\n9,9\n" } });
    });
    await settle();
    fireEvent.keyDown(editor, { key: "s", ...MOD });
    await settle();

    expect(embedWrite).toHaveBeenCalledTimes(1);
    // The second panel re-read; the first did not, because it already holds
    // what it just persisted and a reload would throw that away.
    expect(embedRead).toHaveBeenCalledTimes(3);
    expect(embedRead).toHaveBeenLastCalledWith(VAULT, "people.csv");
    expect(second).toBeTruthy();
  });

  it("stay in step after a CELL edit in one, which writes without the raw editor", async () => {
    embedRead.mockResolvedValue(embed());
    csvRead.mockResolvedValue(table());
    csvSetCell.mockResolvedValue(table({ rev: "rev-2" }));
    const first = await panel(CSV_TARGET);
    await panel("people.csv");
    expect(embedRead).toHaveBeenCalledTimes(2);

    fireEvent.click(first.querySelector('[data-row="2"][data-column="1"]') as HTMLElement);
    const input = first.querySelector("input") as HTMLInputElement;
    fireEvent.change(input, { target: { value: "fine" } });
    fireEvent.keyDown(input, { key: "Enter" });
    await settle();

    // Four: the writer re-reads because its own raw buffer is now stale (that
    // is 45.4's `onExternalWrite`), and the sibling re-reads because it was
    // told. A cell edit never touches the raw editor, so this is a different
    // announcement path from the save above and it has to be wired too.
    expect(embedRead).toHaveBeenCalledTimes(4);
  });

  it("does not disturb a panel over a different file", async () => {
    embedRead.mockImplementation(async (_vault, target) =>
      target === CSV_TARGET
        ? embed()
        : embed({
            relPath: JSON_TARGET,
            name: "config.json",
            file: file({ text: "{}" }),
          }),
    );
    embedWrite.mockResolvedValue(undefined);
    csvRead.mockResolvedValue(table());
    const csv = await panel(CSV_TARGET);
    await panel(JSON_TARGET);
    expect(embedRead).toHaveBeenCalledTimes(2);

    const editor = await sourceEditor(csv);
    await act(async () => {
      const view = EditorView.findFromDOM(editor);
      view?.dispatch({ changes: { from: 0, to: view.state.doc.length, insert: "x\n" } });
    });
    await settle();
    fireEvent.keyDown(editor, { key: "s", ...MOD });
    await settle();

    // Still two: the bus is keyed per file, not per vault.
    expect(embedRead).toHaveBeenCalledTimes(2);
  });
});

describe("Rust's answer about the file, not the spelling", () => {
  it("draws the row the returned kind selects, even against the syntactic guess", async () => {
    // The widget's synchronous guess said `file` in order to let the embed try.
    // If Rust ever disagrees, Rust wins: an image row has no rendered half and
    // is not writable, so the panel offers no toggle and refuses to save rather
    // than editing bytes it has misread.
    embedRead.mockResolvedValue(
      embed({
        relPath: JSON_TARGET,
        name: "config.json",
        kind: "image",
        file: file({ text: "{}" }),
      }),
    );
    const host = await panel(JSON_TARGET);

    expect(host.querySelector('[role="tab"]')).toBeNull();
    expect(host.textContent).toContain("keeper does not write Image files");
  });
});

describe("the widget", () => {
  it("shows the ordinary link first and puts the panel in its place", async () => {
    const widget = new FileEmbedWidget(VAULT, CSV_TARGET, null, {
      mount: (container) => {
        container.textContent = "panel";
        return { unmount: () => {} };
      },
    });
    const dom = widget.toDOM();

    expect(dom.querySelector("a")?.textContent).toBe(CSV_TARGET);
    await settle();
    expect(dom.textContent).toBe("panel");
  });

  it("drops a panel whose import resolved after the widget was destroyed", async () => {
    let mounted = false;
    const widget = new FileEmbedWidget(VAULT, CSV_TARGET, null, {
      mount: () => {
        mounted = true;
        return { unmount: () => {} };
      },
    });
    const dom = widget.toDOM();
    widget.destroy();
    await settle();

    expect(mounted).toBe(false);
    // The link stands; nothing is written into DOM CodeMirror has thrown away.
    expect(dom.querySelector("a")).not.toBeNull();
  });

  it("reuses the DOM for the same embed, so the caret moving cannot lose a buffer", () => {
    const one = new FileEmbedWidget(VAULT, CSV_TARGET);
    expect(one.eq(new FileEmbedWidget(VAULT, CSV_TARGET))).toBe(true);
    expect(one.eq(new FileEmbedWidget(VAULT, "attachments/other.csv"))).toBe(false);
    expect(one.eq(new FileEmbedWidget("vault-2", CSV_TARGET))).toBe(false);
    // A recording note and an ordinary one resolve the same target against
    // different roots, so they are not the same widget.
    expect(one.eq(new FileEmbedWidget(VAULT, CSV_TARGET, "session-1"))).toBe(false);
  });

  it("claims the panel's events from CodeMirror and gives up the link's", () => {
    const widget = new FileEmbedWidget(VAULT, CSV_TARGET);
    const body = document.createElement("div");
    body.className = "cm-embed-body";
    const inside = document.createElement("input");
    body.append(inside);
    const anchor = document.createElement("a");

    // A claimed event is one CodeMirror runs no handler for. Letting a click
    // through would reveal the line, and a revealed line drops its decorations
    // — so the click would destroy the panel instead of using it. An untargeted
    // event has `target === null`, which is the "not the panel" case.
    expect(widget.ignoreEvent(new MouseEvent("click"))).toBe(false);
    expect(widget.ignoreEvent({ target: inside } as unknown as Event)).toBe(true);
    expect(widget.ignoreEvent({ target: anchor } as unknown as Event)).toBe(false);
  });
});

describe("a data embed in a recording note", () => {
  it("lets the session have its own manifest", async () => {
    let claimed = false;
    const widget = new FileEmbedWidget(VAULT, "2026/08/session/manifest.json", "session-1", {
      mount: () => {
        throw new Error("the vault must not be asked for a file the session owns");
      },
      renderRecording: async (host, sessionId, target) => {
        claimed = sessionId === "session-1" && target === "2026/08/session/manifest.json";
        if (claimed) {
          host.textContent = "chip";
        }
        return claimed;
      },
    });
    const dom = widget.toDOM();
    await settle();

    expect(claimed).toBe(true);
    expect(dom.textContent).toBe("chip");
  });

  it("falls through to the vault for an attachment the session does not own", async () => {
    let mountedInto: HTMLElement | null = null;
    const widget = new FileEmbedWidget(VAULT, CSV_TARGET, "session-1", {
      mount: (container) => {
        mountedInto = container;
        container.textContent = "panel";
        return { unmount: () => {} };
      },
      // Not a session file. That is not "missing", it is licence to look in the
      // vault — which is where the attachments panel wrote it.
      renderRecording: async () => false,
    });
    const dom = widget.toDOM();
    await settle();

    expect(mountedInto).not.toBeNull();
    expect(dom.textContent).toBe("panel");
  });
});

describe("the renderer", () => {
  it("turns a data embed into a panel and leaves an ordinary wikilink alone", async () => {
    embedRead.mockResolvedValue(embed());
    csvRead.mockResolvedValue(table());
    const parent = document.createElement("div");
    document.body.append(parent);
    const view = new EditorView({
      parent,
      state: EditorState.create({
        doc: `intro\n\n![[${CSV_TARGET}]]\n\n[[people.md]]\n`,
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
    await settle();

    // `waitFor` here too, and for the reason spelled out just below: the read
    // is issued BY the decoration builder, so "has it been called" is exactly
    // as frame-dependent as "is the host in contentDOM". Wrapping only the
    // second assertion left this test green until the box was busy — it failed
    // once in three full-suite runs during the wave-2 gate.
    await waitFor(() => expect(embedRead).toHaveBeenCalledWith(VAULT, CSV_TARGET));
    // `waitFor`, not a tick count. CodeMirror recomputes its viewport on a
    // measure pass that runs in an animation frame, so whether the widget's
    // host is in `contentDOM` at any given microtask depends on whether a frame
    // elapsed — which depends on how busy the box is. That is the same
    // load-dependent shape as the `getClientRects` fault, and a fixed number of
    // ticks turns it into a suite that is green until it is slow.
    await waitFor(() =>
      expect(view.contentDOM.querySelectorAll(".cm-embed-block")).toHaveLength(1),
    );
    // The plain wikilink on the fifth line is still a link, not a second panel.
    // And this one, for the third time and the same reason: line five is only
    // in `contentDOM` once CodeMirror's viewport has been computed. Two of the
    // three assertions in this test were wrapped and the third was not, which
    // is why it kept failing about one full-suite run in six.
    await waitFor(() => expect(view.contentDOM.textContent).toContain("people.md"));
    view.destroy();
    parent.remove();
  });

  it("leaves a markdown embed to the wikilink it has always been", async () => {
    const parent = document.createElement("div");
    document.body.append(parent);
    const view = new EditorView({
      parent,
      state: EditorState.create({
        doc: "intro\n\n![[Weekly review.md]]\n",
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
    await settle();

    expect(view.contentDOM.querySelector(".cm-embed-block")).toBeNull();
    expect(embedRead).not.toHaveBeenCalled();
    view.destroy();
    parent.remove();
  });
});
