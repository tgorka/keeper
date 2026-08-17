/**
 * The registry's `document` viewer, mounted the way a panel mounts it
 * (Story 45.8, FR-181, FR-182, AD-87, AD-91).
 *
 * **Every test here goes through `viewerComponentFor`.** Importing
 * `DocumentViewer` directly would prove the component works and prove nothing
 * about the binding — and "declared and never mounted" is DW-172, which shipped
 * green in epic 44 precisely because the tests mounted the unit rather than the
 * wiring. A viewer bound in a table nobody exercises is that defect wearing a
 * different hat, so the table is what is exercised.
 *
 * The IPC surface is mocked because these are states a real synced folder
 * produces on demand and cannot produce on request — a decompression bomb, a
 * 400-page PDF, a workbook with 50 000 rows. What Rust makes of those bytes is
 * proved on the other side of the boundary, in `keeper-core`'s own tests over
 * real containers this test cannot build. Everything above the IPC line —
 * the registry, the binding, 44.10's real window, the real degradation path —
 * is the real thing.
 */
import { render, screen, waitFor } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import type { DocumentVm, SheetVm, SlideVm, WordBlockVm } from "@/lib/ipc/client";
import { type ListGeometry, withListGeometry } from "@/test/layout";

const syncReadDocument = vi.fn<(profileId: string, subpath: string) => Promise<DocumentVm>>();

vi.mock("@/lib/ipc/client", () => ({
  syncReadDocument: (profileId: string, subpath: string) => syncReadDocument(profileId, subpath),
  revealPath: vi.fn(async () => undefined),
  syncOpenEntry: vi.fn(async () => undefined),
}));

import { UNKNOWN_VIEWER_TESTID, type ViewerFile, viewerComponentFor } from "@/lib/viewers";
import {
  DOCUMENT_DETAIL_TESTID,
  DOCUMENT_PAGES_TESTID,
  DOCUMENT_PDF_TESTID,
  DOCUMENT_SHEET_TAB_TESTID,
  DOCUMENT_SHEET_TESTID,
  DOCUMENT_SLIDE_TESTID,
  DOCUMENT_VIEWER_TESTID,
  DocumentView,
} from "./document-viewer";

const VIEWPORT_PX = 400;
const ROW_PX = 40;

let geometry: ListGeometry | null = null;

afterEach(() => {
  geometry?.undo();
  geometry = null;
  syncReadDocument.mockReset();
});

function target(overrides: Partial<ViewerFile> = {}): ViewerFile {
  return {
    name: "report.pdf",
    kind: "file",
    relativePath: "papers/report.pdf",
    profileId: "profile-1",
    absolutePath: "/Volumes/merope/papers/report.pdf",
    sizeLabel: "2.4 MB",
    openWith: null,
    writeCaveat: null,
    writeRefusal: null,
    ...overrides,
  };
}

function vm(overrides: Partial<DocumentVm> = {}): DocumentVm {
  return {
    format: "pdf",
    sizeBytes: 2_400_000,
    sizeLabel: "2.4 MB",
    detail: null,
    truncated: false,
    pdf: null,
    words: null,
    slides: null,
    sheets: null,
    ...overrides,
  };
}

function block(text: string, style: WordBlockVm["style"] = "paragraph"): WordBlockVm {
  return { style, runs: [{ text, bold: false, italic: false }] };
}

function sheet(name: string, rows: string[][]): SheetVm {
  return {
    name,
    rows,
    rowCount: rows.length,
    columnCount: Math.max(...rows.map((row) => row.length), 0),
    truncated: false,
  };
}

function slide(number: number, title: string): SlideVm {
  return { number, title, lines: [] };
}

/** Mount through the registry, exactly as a panel does. */
function mount(file: ViewerFile) {
  const { entry, Component } = viewerComponentFor(file);
  return { entry, ...render(<Component file={file} entry={entry} />) };
}

describe("the document viewer's binding", () => {
  it("is what the registry hands a panel for all four document formats", () => {
    // The DW-172 assertion. Not "DocumentViewer renders" — "asking the registry
    // for a .docx yields something other than the placeholder".
    for (const [name, format] of [
      ["a.pdf", "pdf"],
      ["a.docx", "docx"],
      ["a.pptx", "pptx"],
      ["a.xlsx", "xlsx"],
    ] as const) {
      const { entry, Component } = viewerComponentFor(target({ name, relativePath: name }));
      expect(entry.viewer, name).toBe("document");
      expect(entry.format, name).toBe(format);
      expect(entry.writable, `${name} must be read-only`).toBe(false);
      expect(Component.name, name).toBe("DocumentViewer");
    }
  });

  it("says nothing about a write refusal it does not own", async () => {
    // Two verdicts now travel on a `ViewerFile`, and a document has both: the
    // FORMAT refuses (`entry.writable` is false above, for all four), and a
    // document inside a session's `workspace/` carries the LOCATION's refusal
    // as well. This viewer offers no editor, so neither is its business — it
    // must draw the document and leave the sentences to the surface that
    // actually offers a write.
    geometry = withListGeometry({ viewport: VIEWPORT_PX, row: ROW_PX });
    syncReadDocument.mockResolvedValue(
      vm({
        format: "docx",
        words: { blocks: [block("hello")], blockCount: 1, truncated: false },
      }),
    );
    const { container } = mount(
      target({
        name: "report.docx",
        relativePath: "60-sessions/active/2026-08-10-keeper/workspace/report.docx",
        writeRefusal:
          "60-sessions/active/2026-08-10-keeper/workspace/report.docx is inside a session's " +
          "workspace — keeper reads it but never writes there.",
      }),
    );

    expect(await screen.findByText("hello")).toBeInTheDocument();
    expect(container.textContent).not.toContain("never writes there");
  });

  it("leaves a document format keeper does not implement on the unknown viewer", async () => {
    // OpenDocument is deliberately not implemented: it is a different container
    // with a different XML vocabulary, and half a renderer for it would be
    // worse than the placeholder. This asserts the honest outcome rather than
    // asserting a comment.
    mount(target({ name: "thesis.odt", relativePath: "thesis.odt" }));

    expect(await screen.findByTestId(UNKNOWN_VIEWER_TESTID)).toBeInTheDocument();
    expect(syncReadDocument).not.toHaveBeenCalled();
  });
});

describe("a PDF", () => {
  it("renders the platform's own renderer, with the page count keeper probed", async () => {
    syncReadDocument.mockResolvedValue(
      vm({
        format: "pdf",
        pdf: { version: "1.7", pageCount: 12, encrypted: false, servable: true },
      }),
    );

    mount(target());

    expect(await screen.findByTestId(DOCUMENT_PAGES_TESTID)).toHaveTextContent("12");
    const embed = screen.getByTestId(DOCUMENT_PDF_TESTID);
    expect(embed).toHaveAttribute("type", "application/pdf");
    // Served by 45.7's protocol from the profile root, not by a path.
    expect(embed).toHaveAttribute("src", "keeper-file://profile-1/papers/report.pdf");
  });

  it("mounts ONE element for a 400-page document", async () => {
    // The story's bound, asserted by counting. The bound here is structural
    // rather than a cap: the pages live inside the platform's renderer, so
    // there is nothing for keeper to mount 400 of. A regression that started
    // drawing a page per page would fail this by count.
    syncReadDocument.mockResolvedValue(
      vm({
        format: "pdf",
        pdf: { version: "1.4", pageCount: 400, encrypted: false, servable: true },
      }),
    );

    mount(target());

    expect(await screen.findByTestId(DOCUMENT_PAGES_TESTID)).toHaveTextContent("400");
    expect(screen.getAllByTestId(DOCUMENT_PDF_TESTID)).toHaveLength(1);
  });

  it("omits the page count rather than guessing when the probe could not read one", async () => {
    syncReadDocument.mockResolvedValue(
      vm({
        format: "pdf",
        pdf: { version: "1.7", pageCount: null, encrypted: false, servable: true },
      }),
    );

    mount(target());

    // The pages still render — only the number is missing.
    expect(await screen.findByTestId(DOCUMENT_PDF_TESTID)).toBeInTheDocument();
    expect(screen.queryByTestId(DOCUMENT_PAGES_TESTID)).not.toBeInTheDocument();
  });

  it("shows the placeholder instead of an embed pointed at nothing when there is no profile", async () => {
    // A file outside every sync profile can be viewed but not read: there is no
    // profile-scoped URL for it. An <embed src=""> would paint an empty box,
    // which is the lie this whole story is written against.
    mount(target({ profileId: null }));

    const placeholder = await screen.findByTestId(UNKNOWN_VIEWER_TESTID);
    expect(placeholder).toHaveTextContent(/not inside a synced folder/i);
    expect(screen.queryByTestId(DOCUMENT_PDF_TESTID)).not.toBeInTheDocument();
  });

  it("refuses to draw an embed with no URL when a HOST mounts the view directly", async () => {
    // `DocumentView` is exported so a surface holding different coordinates — a
    // note embed with a vault id rather than a profile id (45.12) — renders the
    // same document without forking this file. Such a host can legitimately
    // have a loaded VM and no servable URL, and that combination is reachable
    // ONLY through this entry point: `DocumentViewer` never gets there, because
    // a file with no profile fails in the loader first.
    //
    // Mounted directly on purpose, and it is the one place in this file that
    // is. Going through the registry cannot reach this branch, so a test that
    // insisted on it would leave the guard unexercised — which it was, until a
    // mutation deleting the guard survived the sweep.
    const file = target({ name: "attached.pdf", relativePath: "notes/attached.pdf" });
    const { entry } = viewerComponentFor(file);

    render(
      <DocumentView
        file={file}
        entry={entry}
        vm={vm({
          format: "pdf",
          pdf: { version: "1.7", pageCount: 9, encrypted: false, servable: true },
        })}
        pdfSrc={null}
      />,
    );

    expect(await screen.findByTestId(UNKNOWN_VIEWER_TESTID)).toHaveTextContent(
      /cannot reach this PDF's bytes/i,
    );
    expect(screen.queryByTestId(DOCUMENT_PDF_TESTID)).not.toBeInTheDocument();
  });
});

describe("a Word document", () => {
  it("renders its paragraphs and their outline level", async () => {
    geometry = withListGeometry({ viewport: VIEWPORT_PX, row: ROW_PX });
    syncReadDocument.mockResolvedValue(
      vm({
        format: "docx",
        sizeLabel: "48 kB",
        words: {
          blocks: [block("Quarterly Report", "title"), block("Revenue rose.")],
          blockCount: 2,
          truncated: false,
        },
      }),
    );

    mount(target({ name: "report.docx", relativePath: "report.docx" }));

    expect(await screen.findByText("Quarterly Report")).toBeInTheDocument();
    expect(screen.getByText("Quarterly Report").closest("p")).toHaveAttribute(
      "data-block-style",
      "title",
    );
    expect(screen.getByText("Revenue rose.")).toBeInTheDocument();
  });

  it("mounts a window over a long document, not the document", async () => {
    geometry = withListGeometry({ viewport: VIEWPORT_PX, row: ROW_PX });
    const blocks = Array.from({ length: 3_000 }, (_, at) => block(`paragraph ${at}`));
    syncReadDocument.mockResolvedValue(
      vm({
        format: "docx",
        words: { blocks, blockCount: 12_400, truncated: true },
      }),
    );

    mount(target({ name: "long.docx", relativePath: "long.docx" }));

    await screen.findByTestId(DOCUMENT_VIEWER_TESTID);
    const mounted = document.querySelectorAll("[data-window-row]");
    // A viewport of 400 px over 40 px rows is ten rows plus the overscan.
    expect(mounted.length).toBeGreaterThan(0);
    expect(mounted.length).toBeLessThan(40);
  });

  it("says the document is longer than what is shown, with the document's own count", async () => {
    // 44.11's rule. `blockCount` is the document's 12 400, never the 3 000 that
    // were sent — a reader must be able to tell a window from a whole file.
    geometry = withListGeometry({ viewport: VIEWPORT_PX, row: ROW_PX });
    syncReadDocument.mockResolvedValue(
      vm({
        format: "docx",
        truncated: true,
        detail: "this document has 12400 paragraphs; keeper is showing the first 3000",
        words: { blocks: [block("one")], blockCount: 12_400, truncated: true },
      }),
    );

    mount(target({ name: "long.docx", relativePath: "long.docx" }));

    expect(await screen.findByTestId(DOCUMENT_DETAIL_TESTID)).toHaveTextContent("12400");
    expect(screen.getByText("12400")).toBeInTheDocument();
  });
});

describe("a presentation", () => {
  it("renders a slide per slide, numbered and titled", async () => {
    geometry = withListGeometry({ viewport: VIEWPORT_PX, row: ROW_PX });
    syncReadDocument.mockResolvedValue(
      vm({
        format: "pptx",
        slides: {
          slides: [slide(1, "Welcome"), slide(2, "Numbers")],
          slideCount: 2,
          truncated: false,
        },
      }),
    );

    mount(target({ name: "deck.pptx", relativePath: "deck.pptx" }));

    await screen.findByTestId(DOCUMENT_VIEWER_TESTID);
    expect(screen.getAllByTestId(DOCUMENT_SLIDE_TESTID)).toHaveLength(2);
    expect(screen.getByText("Welcome")).toBeInTheDocument();
    expect(screen.getByText("Numbers")).toBeInTheDocument();
  });

  it("mounts a window over a long deck", async () => {
    geometry = withListGeometry({ viewport: VIEWPORT_PX, row: ROW_PX });
    const slides = Array.from({ length: 150 }, (_, at) => slide(at + 1, `Slide ${at + 1}`));
    syncReadDocument.mockResolvedValue(
      vm({ format: "pptx", slides: { slides, slideCount: 150, truncated: false } }),
    );

    mount(target({ name: "big.pptx", relativePath: "big.pptx" }));

    await screen.findByTestId(DOCUMENT_VIEWER_TESTID);
    expect(screen.getAllByTestId(DOCUMENT_SLIDE_TESTID).length).toBeLessThan(40);
  });
});

describe("a workbook", () => {
  it("renders a tab per sheet and the active sheet's cells", async () => {
    geometry = withListGeometry({ viewport: VIEWPORT_PX, row: ROW_PX });
    syncReadDocument.mockResolvedValue(
      vm({
        format: "xlsx",
        sheets: {
          sheets: [
            sheet("Revenue", [
              ["Region", "Total"],
              ["Europe", "1200"],
            ]),
            sheet("Notes", [["Checked by", "Ada"]]),
          ],
          sheetCount: 2,
          truncated: false,
        },
      }),
    );

    mount(target({ name: "budget.xlsx", relativePath: "budget.xlsx" }));

    await screen.findByTestId(DOCUMENT_VIEWER_TESTID);
    const tabs = screen.getAllByTestId(DOCUMENT_SHEET_TAB_TESTID);
    expect(tabs.map((tab) => tab.textContent)).toEqual(["Revenue", "Notes"]);
    // The sheet NAME, which is the story's asserted content for this format.
    expect(screen.getByTestId(DOCUMENT_SHEET_TESTID)).toHaveAttribute("data-sheet-name", "Revenue");
    expect(screen.getByText("Europe")).toBeInTheDocument();
  });

  it("shows another sheet when its tab is chosen", async () => {
    geometry = withListGeometry({ viewport: VIEWPORT_PX, row: ROW_PX });
    syncReadDocument.mockResolvedValue(
      vm({
        format: "xlsx",
        sheets: {
          sheets: [sheet("Revenue", [["Europe"]]), sheet("Notes", [["Ada"]])],
          sheetCount: 2,
          truncated: false,
        },
      }),
    );

    mount(target({ name: "budget.xlsx", relativePath: "budget.xlsx" }));

    await screen.findByTestId(DOCUMENT_VIEWER_TESTID);
    screen.getAllByTestId(DOCUMENT_SHEET_TAB_TESTID)[1].click();

    await waitFor(() => {
      expect(screen.getByTestId(DOCUMENT_SHEET_TESTID)).toHaveAttribute("data-sheet-name", "Notes");
    });
    expect(screen.getByText("Ada")).toBeInTheDocument();
  });

  it("marks the chosen sheet without drawing a second edge under the tab strip", async () => {
    geometry = withListGeometry({ viewport: VIEWPORT_PX, row: ROW_PX });
    syncReadDocument.mockResolvedValue(
      vm({
        format: "xlsx",
        sheets: {
          sheets: [sheet("Revenue", [["Europe"]]), sheet("Notes", [["Ada"]])],
          sheetCount: 2,
          truncated: false,
        },
      }),
    );

    mount(target({ name: "budget.xlsx", relativePath: "budget.xlsx" }));

    await screen.findByTestId(DOCUMENT_VIEWER_TESTID);
    const [active, idle] = screen.getAllByTestId(DOCUMENT_SHEET_TAB_TESTID);

    // DESIGN.md → Elevation & Depth: a seam has exactly one owner. The strip
    // owns the hairline; the active tab used to add `border-b-2` on top of it,
    // so one tab sat over 3px of line and the rest over 1px. The mark is an
    // overlay now — `TabsList`'s line-variant construction — and it lands ON
    // the strip's pixel rather than under it.
    expect(active.className).not.toContain("border-b");
    expect(idle.className).not.toContain("border-b");
    expect(active).toHaveClass("after:bg-primary");
    expect(active).toHaveClass("after:-bottom-px");
    // Still told apart, and not by the overlay alone.
    expect(idle).toHaveClass("after:bg-transparent");
    expect(active).toHaveAttribute("aria-selected", "true");
    expect(idle).toHaveAttribute("aria-selected", "false");
  });

  it("mounts a window over a 50 000-row sheet and still reports 50 000", async () => {
    geometry = withListGeometry({ viewport: VIEWPORT_PX, row: ROW_PX });
    const rows = Array.from({ length: 500 }, (_, at) => [`row ${at}`]);
    syncReadDocument.mockResolvedValue(
      vm({
        format: "xlsx",
        truncated: true,
        sheets: {
          sheets: [{ name: "Data", rows, rowCount: 50_000, columnCount: 1, truncated: true }],
          sheetCount: 1,
          truncated: false,
        },
      }),
    );

    mount(target({ name: "big.xlsx", relativePath: "big.xlsx" }));

    const grid = await screen.findByTestId(DOCUMENT_SHEET_TESTID);
    // The count is the sheet's real height, not the projection's length and not
    // the window's — the thing 44.11 forbids getting wrong.
    expect(grid).toHaveAttribute("data-row-count", "50000");
    expect(grid.querySelectorAll("[data-window-row]").length).toBeLessThan(40);
  });
});

describe("degrading", () => {
  it("shows a corrupt document's named reason on the placeholder rather than throwing", async () => {
    syncReadDocument.mockResolvedValue(
      vm({
        format: "docx",
        detail: "keeper could not read this word document: word/document.xml is missing",
      }),
    );

    mount(target({ name: "broken.docx", relativePath: "broken.docx" }));

    const placeholder = await screen.findByTestId(UNKNOWN_VIEWER_TESTID);
    expect(placeholder).toHaveTextContent("word/document.xml is missing");
    expect(screen.queryByTestId(DOCUMENT_VIEWER_TESTID)).not.toBeInTheDocument();
  });

  it("shows a decompression bomb's refusal rather than a blank pane", async () => {
    syncReadDocument.mockResolvedValue(
      vm({
        format: "xlsx",
        detail:
          "keeper stopped reading this spreadsheet: xl/workbook.xml inflates past the 16.0 MB keeper will hold for one part, so it is a decompression bomb rather than a document",
      }),
    );

    mount(target({ name: "bomb.xlsx", relativePath: "bomb.xlsx" }));

    expect(await screen.findByTestId(UNKNOWN_VIEWER_TESTID)).toHaveTextContent(
      "decompression bomb",
    );
  });

  it("shows the command's own sentence when the read itself failed", async () => {
    syncReadDocument.mockRejectedValue({ message: "the drive is not connected" });

    mount(target());

    expect(await screen.findByTestId(UNKNOWN_VIEWER_TESTID)).toHaveTextContent(
      "the drive is not connected",
    );
  });

  it("says so when the bytes are not the format the name promised", async () => {
    // A renamed download that IS still servable: a Word document named .pdf.
    // Rendering it correctly while saying nothing would leave a reader
    // wondering why their PDF has no pages.
    syncReadDocument.mockResolvedValue(
      vm({
        format: "docx",
        words: { blocks: [block("actually a memo")], blockCount: 1, truncated: false },
      }),
    );

    mount(target({ name: "report.pdf", relativePath: "report.pdf" }));

    expect(await screen.findByTestId(DOCUMENT_DETAIL_TESTID)).toHaveTextContent(
      /named as pdf but its contents are a Word document/i,
    );
    // And it renders as what it IS.
    expect(screen.getByText("actually a memo")).toBeInTheDocument();
  });

  it("refuses to mount an embed for a real PDF the protocol will not serve", async () => {
    // The disagreement that would otherwise be SILENT. `sniff` reads the
    // content, so this is correctly a PDF; `is_servable_path` reads the name,
    // so `keeper-file://` will 404 it. Mounting the embed anyway gives an empty
    // element and no sentence, because a failed plugin render is invisible to
    // JavaScript. Rust answers the question and the viewer words it.
    syncReadDocument.mockResolvedValue(
      vm({
        format: "pdf",
        pdf: { version: "1.6", pageCount: 3, encrypted: false, servable: false },
      }),
    );

    mount(target({ name: "quarterly.xlsx", relativePath: "quarterly.xlsx" }));

    const placeholder = await screen.findByTestId(UNKNOWN_VIEWER_TESTID);
    expect(placeholder).toHaveTextContent(/this is a PDF, but it is named quarterly\.xlsx/i);
    expect(screen.queryByTestId(DOCUMENT_PDF_TESTID)).not.toBeInTheDocument();
  });

  it("warns that an encrypted PDF may not draw, while still trying", async () => {
    syncReadDocument.mockResolvedValue(
      vm({
        format: "pdf",
        detail: "this PDF is encrypted, so it may not render without its password",
        pdf: { version: "1.7", pageCount: 4, encrypted: true, servable: true },
      }),
    );

    mount(target());

    expect(await screen.findByTestId(DOCUMENT_DETAIL_TESTID)).toHaveTextContent("encrypted");
    expect(screen.getByTestId(DOCUMENT_PDF_TESTID)).toBeInTheDocument();
  });
});

describe("loading", () => {
  it("reads through the profile coordinates it was handed, never a path", async () => {
    // AD-65: the frontend joins nothing. What goes to Rust is exactly the
    // profile id and the relative path the listing produced; `absolutePath` is
    // an action argument and must never be used to read.
    syncReadDocument.mockResolvedValue(
      vm({ pdf: { version: "1.7", pageCount: 1, encrypted: false, servable: true } }),
    );

    mount(target());

    await waitFor(() => {
      expect(syncReadDocument).toHaveBeenCalledWith("profile-1", "papers/report.pdf");
    });
  });
});
