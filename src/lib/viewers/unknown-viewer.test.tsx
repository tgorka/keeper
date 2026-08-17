/**
 * The unknown viewer's tests (Story 45.2, AD-91).
 *
 * Rendered against the real component in a real host with the real capability
 * store, because the defects epic 44 shipped green were all things a suite
 * never assembled. What this asserts is the AD-91 contract in full: the file is
 * named, the extension is named, the size is stated, the two actions are
 * offered when they can work and ABSENT when they cannot, and no absolute path
 * ever reaches the DOM.
 */

import { fireEvent, render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";

const revealPath = vi.fn(async (_path: unknown) => undefined);
vi.mock("@/lib/ipc/client", () => ({
  revealPath: (path: unknown) => revealPath(path),
  syncOpenEntry: vi.fn(async () => undefined),
}));

import { capabilitiesStore, DEFAULT_CAPABILITIES } from "@/lib/stores/capabilities";
import {
  resolveViewer,
  UNKNOWN_ENTRY,
  UNKNOWN_VIEWER_EXTENSION_SLOT,
  UNKNOWN_VIEWER_FORMAT_SLOT,
  UNKNOWN_VIEWER_NO_EXTENSION,
  UNKNOWN_VIEWER_OPEN_LABEL,
  UNKNOWN_VIEWER_REVEAL_LABEL,
  UNKNOWN_VIEWER_SIZE_SLOT,
  UNKNOWN_VIEWER_SIZE_UNKNOWN,
  UNKNOWN_VIEWER_TESTID,
  UnknownViewer,
  type ViewerFile,
} from "@/lib/viewers";

const ABSOLUTE = "/Users/ada/Volumes/merope/inbox/board.sketchpad";

function unknownFile(overrides: Partial<ViewerFile> = {}): ViewerFile {
  return {
    name: "board.sketchpad",
    kind: "file",
    relativePath: "inbox/board.sketchpad",
    profileId: "profile-1",
    absolutePath: ABSOLUTE,
    sizeLabel: "1.2 MB",
    openWith: null,
    writeCaveat: null,
    writeCaveatShort: null,
    writeRefusal: null,
    ...overrides,
  };
}

/** Turn the reveal capability on, as Rust would on a desktop build. */
function withFileManager() {
  capabilitiesStore.getState().applySnapshot({
    ...DEFAULT_CAPABILITIES,
    revealInFileManager: true,
  });
}

afterEach(() => {
  capabilitiesStore.setState({ capabilities: DEFAULT_CAPABILITIES, hydrated: false });
  revealPath.mockClear();
});

describe("the unknown viewer says what the file is", () => {
  it("names the file, its extension and its size", () => {
    render(<UnknownViewer file={unknownFile()} entry={UNKNOWN_ENTRY} />);

    expect(screen.getByTestId(UNKNOWN_VIEWER_TESTID)).toBeInTheDocument();
    expect(screen.getByText("board.sketchpad")).toBeInTheDocument();
    expect(screen.getByTestId(UNKNOWN_VIEWER_EXTENSION_SLOT)).toHaveTextContent(".sketchpad");
    expect(screen.getByTestId(UNKNOWN_VIEWER_SIZE_SLOT)).toHaveTextContent("1.2 MB");
    expect(screen.getByTestId(UNKNOWN_VIEWER_FORMAT_SLOT)).toHaveTextContent(UNKNOWN_ENTRY.label);
  });

  it("says the extension is None rather than leaving the cell empty", () => {
    render(
      <UnknownViewer
        file={unknownFile({ name: "Makefile", relativePath: "Makefile" })}
        entry={UNKNOWN_ENTRY}
      />,
    );
    expect(screen.getByTestId(UNKNOWN_VIEWER_EXTENSION_SLOT)).toHaveTextContent(
      UNKNOWN_VIEWER_NO_EXTENSION,
    );
  });

  it("says the size is unknown rather than inventing one", () => {
    render(<UnknownViewer file={unknownFile({ sizeLabel: null })} entry={UNKNOWN_ENTRY} />);
    expect(screen.getByTestId(UNKNOWN_VIEWER_SIZE_SLOT)).toHaveTextContent(
      UNKNOWN_VIEWER_SIZE_UNKNOWN,
    );
  });

  it("never renders the absolute path (FR-145)", () => {
    withFileManager();
    const { container } = render(<UnknownViewer file={unknownFile()} entry={UNKNOWN_ENTRY} />);
    expect(container.textContent).not.toContain(ABSOLUTE);
    expect(container.textContent).not.toContain("/Users/ada");
    expect(container.textContent).toContain("inbox/board.sketchpad");
  });

  it("distinguishes a format keeper never claimed from one it cannot draw yet", () => {
    const { container: unknown } = render(
      <UnknownViewer file={unknownFile()} entry={UNKNOWN_ENTRY} />,
    );
    expect(unknown.textContent).toContain("no viewer for this format");

    const pdf = resolveViewer(unknownFile({ name: "report.pdf" }));
    const { container: recognised } = render(
      <UnknownViewer file={unknownFile({ name: "report.pdf" })} entry={pdf} />,
    );
    expect(recognised.textContent).toContain("PDF");
    expect(recognised.textContent).toContain("cannot show it here yet");
  });

  it("draws the same card for a file keeper refuses to write", () => {
    // `writeRefusal` rides on every `ViewerFile` now, and this viewer offers no
    // write at all — no editor, no Save, nothing a refusal could take away. So
    // it must ignore the field rather than grow a banner about a control it
    // does not have, and it must not spill the sentence's path into a surface
    // FR-145 keeps paths out of.
    const sentence =
      "60-sessions/active/2026-08-10-keeper/workspace/board.sketchpad is inside a session's " +
      "workspace — keeper reads it but never writes there.";
    const { container: plain } = render(
      <UnknownViewer file={unknownFile()} entry={UNKNOWN_ENTRY} />,
    );
    const { container: refused } = render(
      <UnknownViewer file={unknownFile({ writeRefusal: sentence })} entry={UNKNOWN_ENTRY} />,
    );

    expect(refused.textContent).not.toContain("never writes there");
    expect(refused.innerHTML).toBe(plain.innerHTML);
  });
});

describe("the unknown viewer's two actions", () => {
  it("reveals through the absolute path Rust composed", () => {
    withFileManager();
    render(<UnknownViewer file={unknownFile()} entry={UNKNOWN_ENTRY} />);

    fireEvent.click(screen.getByRole("button", { name: UNKNOWN_VIEWER_REVEAL_LABEL }));
    expect(revealPath).toHaveBeenCalledWith(ABSOLUTE);
  });

  it("omits Reveal where the platform has no file manager", () => {
    // Absent, not disabled: a disabled control is a promise the platform
    // cannot keep.
    render(<UnknownViewer file={unknownFile()} entry={UNKNOWN_ENTRY} />);
    expect(screen.queryByRole("button", { name: UNKNOWN_VIEWER_REVEAL_LABEL })).toBeNull();
  });

  it("omits Reveal when the surface holds no absolute path", () => {
    withFileManager();
    render(<UnknownViewer file={unknownFile({ absolutePath: null })} entry={UNKNOWN_ENTRY} />);
    expect(screen.queryByRole("button", { name: UNKNOWN_VIEWER_REVEAL_LABEL })).toBeNull();
  });

  it("offers Open only when the surface supplied an opener, and calls that one", () => {
    expect(screen.queryByRole("button", { name: UNKNOWN_VIEWER_OPEN_LABEL })).toBeNull();

    const openWith = vi.fn(async () => undefined);
    render(<UnknownViewer file={unknownFile({ openWith })} entry={UNKNOWN_ENTRY} />);
    fireEvent.click(screen.getByRole("button", { name: UNKNOWN_VIEWER_OPEN_LABEL }));
    expect(openWith).toHaveBeenCalledTimes(1);
  });

  it("swallows an opener that rejects rather than throwing out of the click", async () => {
    // Leaving keeper is best effort — a handler that will not launch is not
    // something this pane can repair, and an unhandled rejection in a click is
    // how a pane stops responding.
    const openWith = vi.fn(async () => {
      throw new Error("no handler");
    });
    render(<UnknownViewer file={unknownFile({ openWith })} entry={UNKNOWN_ENTRY} />);
    fireEvent.click(screen.getByRole("button", { name: UNKNOWN_VIEWER_OPEN_LABEL }));
    await expect(openWith.mock.results[0]?.value).rejects.toThrow("no handler");
    expect(screen.getByTestId(UNKNOWN_VIEWER_TESTID)).toBeInTheDocument();
  });
});
