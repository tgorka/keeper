/**
 * The bindings' tests (Story 45.2, AD-87).
 *
 * The one that matters most is the last describe: two DIFFERENT hosts, each
 * asking the registry for the same file and mounting what it gets, must render
 * the same thing. "It opens in Files but not in a note" is the bug this epic
 * exists to fix, and asserting it against two real hosts is the difference
 * between a table that should agree and one that is shown to.
 */

import { render } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";

vi.mock("@/lib/ipc/client", () => ({
  revealPath: vi.fn(async () => undefined),
  syncOpenEntry: vi.fn(async () => undefined),
  // Story 45.4's `text` viewer loads its own bytes. Held pending on purpose:
  // what this file asserts is that two hosts render the SAME thing, and a read
  // that never resolves keeps both of them in one deterministic state without
  // turning a registry test into a loading test.
  // The executor form rather than `Promise.withResolvers`, which this project's
  // `lib: ES2020` does not declare. Nothing ever settles it, so there are no
  // resolvers to name anyway.
  syncReadText: vi.fn(() => new Promise(() => undefined)),
  // Story 45.8's `document` viewer loads its own too, and is held pending for
  // exactly the same reason.
  syncReadDocument: vi.fn(() => new Promise(() => undefined)),
  syncWriteEntry: vi.fn(async () => undefined),
}));

import type { RecordingNoteTargetKind } from "@/lib/ipc/client";
import {
  registeredViewerIds,
  resolveViewer,
  resolveViewerComponent,
  UnknownViewer,
  VIEWER_COMPONENTS,
  type ViewerFile,
  viewerComponentFor,
} from "@/lib/viewers";

function file(overrides: Partial<ViewerFile> = {}): ViewerFile {
  return {
    name: "board.sketchpad",
    kind: "file",
    relativePath: "inbox/board.sketchpad",
    profileId: "profile-1",
    absolutePath: "/Volumes/merope/inbox/board.sketchpad",
    sizeLabel: "1.2 MB",
    openWith: null,
    writeCaveat: null,
    writeRefusal: null,
    ...overrides,
  };
}

describe("an unbound viewer is visible, not silent", () => {
  it("falls back to the unknown viewer and says which id was unbound", () => {
    // DW-172: three tray listeners shipped declared-and-never-mounted because
    // nothing said so. A wave-2 story that forgets its binding gets a line.
    const info = vi.spyOn(console, "info").mockImplementation(() => undefined);
    const pdf = resolveViewer(file({ name: "report.pdf" }));

    expect(resolveViewerComponent(pdf, {})).toBe(UnknownViewer);
    expect(info).toHaveBeenCalledTimes(1);
    expect(info.mock.calls[0]?.[0]).toContain(pdf.viewer);

    // Once, not once a frame: this is called from a render path.
    expect(resolveViewerComponent(pdf, {})).toBe(UnknownViewer);
    expect(info).toHaveBeenCalledTimes(1);
    info.mockRestore();
  });

  it("returns the bound component when there is one", () => {
    const unknown = resolveViewer(file());
    expect(resolveViewerComponent(unknown)).toBe(UnknownViewer);
  });

  it("binds no id the table cannot produce", () => {
    // A binding for an id no row names is a component that can never mount —
    // dead code that reads as coverage.
    const produced = registeredViewerIds();
    for (const id of Object.keys(VIEWER_COMPONENTS)) {
      expect(produced.has(id as never)).toBe(true);
    }
  });
});

describe("viewerComponentFor is total", () => {
  const kinds: RecordingNoteTargetKind[] = ["video", "image", "audio", "file", "folder"];

  it("yields a row and a component for every kind and every shape of name", () => {
    vi.spyOn(console, "info").mockImplementation(() => undefined);
    for (const kind of kinds) {
      for (const name of ["", ".", "notes.md", "clip.mov", "x.constructor", "Makefile"]) {
        const resolved = viewerComponentFor(file({ name, kind }));
        expect(resolved.entry).toBeDefined();
        expect(resolved.Component).toBeTypeOf("function");
      }
    }
    vi.restoreAllMocks();
  });
});

/** The Files pane's shape: a row, and the viewer beneath it. */
function FilesPaneLikeHost({ target }: { target: ViewerFile }) {
  const { entry, Component } = viewerComponentFor(target);
  return (
    <div data-host="files">
      <Component file={target} entry={entry} />
    </div>
  );
}

/** A note embed's shape: the same viewer inside an article. */
function NoteEmbedLikeHost({ target }: { target: ViewerFile }) {
  const { entry, Component } = viewerComponentFor(target);
  return (
    <article data-host="note">
      <Component file={target} entry={entry} />
    </article>
  );
}

describe("two surfaces asking about one file get the same answer", () => {
  it.each([
    ["board.sketchpad", "file"],
    ["report.pdf", "file"],
    ["notes.md", "file"],
    ["clip.mov", "video"],
  ] as const)("%s renders identically in both hosts", (name, kind) => {
    vi.spyOn(console, "info").mockImplementation(() => undefined);
    const target = file({ name, kind, relativePath: `inbox/${name}` });

    const files = render(<FilesPaneLikeHost target={target} />);
    const note = render(<NoteEmbedLikeHost target={target} />);

    // Compared as markup rather than by looking for one viewer's test id: the
    // claim is that the two hosts agree, and it has to keep meaning that as
    // wave 2 binds the ids that are still unbound today.
    const fromFiles = files.container.firstElementChild?.innerHTML;
    const fromNote = note.container.firstElementChild?.innerHTML;
    expect(fromFiles).toBeTruthy();
    expect(fromNote).toBe(fromFiles);
    vi.restoreAllMocks();
  });

  it("hands both hosts the identical row object", () => {
    const target = file({ name: "budget.csv" });
    vi.spyOn(console, "info").mockImplementation(() => undefined);
    expect(viewerComponentFor(target).entry).toBe(viewerComponentFor({ ...target }).entry);
    expect(viewerComponentFor(target).Component).toBe(viewerComponentFor({ ...target }).Component);
    vi.restoreAllMocks();
  });

  it("hands both hosts the same row for a file keeper refuses to write", () => {
    // `writeRefusal` is the LOCATION's verdict, and this layer must not read
    // it. Provenance decides what a surface may OFFER over a file — the Save
    // button, the format toolbar — and the registry decides only what a file
    // IS. A build that resolved a fenced file to some read-only row would make
    // the same `.md` two different formats depending on where it sat.
    const fenced = file({
      name: "notes.md",
      relativePath: "60-sessions/active/2026-08-10-keeper/workspace/notes.md",
      writeRefusal:
        "60-sessions/active/2026-08-10-keeper/workspace/notes.md is inside a session's " +
        "workspace — keeper reads it but never writes there.",
    });
    const ordinary = file({ name: "notes.md", relativePath: "inbox/notes.md" });
    vi.spyOn(console, "info").mockImplementation(() => undefined);

    expect(viewerComponentFor(fenced).entry).toBe(viewerComponentFor(ordinary).entry);
    expect(viewerComponentFor(fenced).Component).toBe(viewerComponentFor(ordinary).Component);
    vi.restoreAllMocks();
  });
});
