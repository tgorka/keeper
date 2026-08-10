/**
 * The registry's `text` viewer, mounted the way a panel mounts it
 * (Story 45.4, AD-87, AD-88).
 *
 * **Every test here goes through `viewerComponentFor`.** Importing
 * `TextFileViewer` directly would prove the component works and prove nothing
 * about the binding — and "declared and never mounted" is DW-172, which shipped
 * green in epic 44 because `renderHook` mounts the hook itself and can never
 * see that `App` does not. A viewer bound in a table nobody exercises is the
 * same defect wearing a different hat.
 *
 * The IPC surface is mocked because these are the states a real vault produces
 * on demand and cannot produce on request; everything below the IPC line —
 * 45.6's loading hook, 45.6's real CodeMirror, the toggle, the structure view —
 * is the real thing.
 */
import { EditorView } from "@codemirror/view";
import { act, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterAll, beforeAll, beforeEach, describe, expect, it, vi } from "vitest";
import type { TextFileVm } from "@/lib/ipc/client";
import { withRangeRects } from "@/test/layout";

const syncReadText = vi.fn<(profileId: string, subpath: string) => Promise<TextFileVm>>();
const syncWriteEntry = vi.fn<(profileId: string, subpath: string, text: string) => Promise<void>>();

vi.mock("@/lib/ipc/client", () => ({
  syncReadText: (profileId: string, subpath: string) => syncReadText(profileId, subpath),
  syncWriteEntry: (profileId: string, subpath: string, text: string) =>
    syncWriteEntry(profileId, subpath, text),
  notesCsvRead: vi.fn(),
  notesCsvSetCell: vi.fn(),
  revealPath: vi.fn(async () => undefined),
  syncOpenEntry: vi.fn(async () => undefined),
}));

import { type ViewerFile, viewerComponentFor } from "@/lib/viewers";
import { TextFileViewer } from "./text-file-viewer";

function target(overrides: Partial<ViewerFile> = {}): ViewerFile {
  return {
    name: "config.json",
    kind: "file",
    relativePath: "inbox/config.json",
    profileId: "profile-1",
    absolutePath: "/Volumes/merope/inbox/config.json",
    sizeLabel: "412 bytes",
    openWith: null,
    ...overrides,
  };
}

function vm(overrides: Partial<TextFileVm> = {}): TextFileVm {
  return {
    text: '{"port": 8080}',
    sizeBytes: 14,
    sizeLabel: "14 bytes",
    oversize: false,
    binary: false,
    detail: null,
    ...overrides,
  };
}

/** Mount exactly as a panel host does: ask the registry, render what it says. */
function openThroughTheRegistry(file: ViewerFile) {
  const { entry, Component } = viewerComponentFor(file);
  return { entry, ...render(<Component file={file} entry={entry} />) };
}

/** Drain microtasks without letting a frame run — see `raw-rendered-view.test.tsx`. */
async function settle(): Promise<void> {
  await act(async () => {
    for (let tick = 0; tick < 10; tick += 1) {
      await Promise.resolve();
    }
  });
}

/**
 * The real CodeMirror the raw view mounts, once its chunks have landed.
 *
 * 45.6's editor loads its grammar through `import()`, so the editor is not in
 * the DOM on the first tick. Waiting on a timer is safe here because
 * `withRangeRects` above gives the measure pass something to measure.
 */
async function editorHost(): Promise<HTMLElement> {
  await waitFor(() => expect(document.querySelector(".cm-content")).not.toBeNull());
  return document.querySelector(".cm-content") as HTMLElement;
}

let removeRangeRects: (() => void) | null = null;
beforeAll(() => {
  removeRangeRects = withRangeRects();
});
afterAll(() => {
  removeRangeRects?.();
});

beforeEach(() => {
  syncReadText.mockReset();
  syncWriteEntry.mockReset();
  // biome-ignore lint/suspicious/noDocumentCookie: arranging or clearing cookie state is this test's subject
  document.cookie = "keeper_viewer_modes=; path=/; max-age=0";
});

describe("the registry's `text` id really mounts this viewer", () => {
  it("resolves a .json file to a component that draws its structure", async () => {
    syncReadText.mockResolvedValue(vm());
    const { entry } = openThroughTheRegistry(target());

    // The row the table chose, so a failure here says which half broke.
    expect(entry.viewer).toBe("text");
    expect(entry.rendered).toBe("structure");

    await settle();
    expect(screen.getByRole("tab", { name: "Structure" })).toHaveAttribute("aria-selected", "true");
    expect(screen.getByText("port")).toBeInTheDocument();
    expect(screen.getByText("8080")).toBeInTheDocument();
    // The path arrived as the listing produced it, and nothing was joined.
    expect(syncReadText).toHaveBeenCalledWith("profile-1", "inbox/config.json");
  });

  it("says it is opening rather than flashing an empty editor", () => {
    // Never settles, so the viewer is held in the state under test. The
    // executor form because this project's `lib: ES2020` has no
    // `Promise.withResolvers`, and there is nothing to resolve regardless.
    syncReadText.mockReturnValue(new Promise<TextFileVm>(() => undefined));
    openThroughTheRegistry(target());
    expect(screen.getByRole("status")).toHaveTextContent("opening config.json");
  });
});

describe("the states a real vault produces", () => {
  it("refuses bytes that are not text, in Rust's own words, with no editor", async () => {
    syncReadText.mockResolvedValue(
      vm({ text: null, binary: true, detail: "config.json is not text keeper can edit" }),
    );
    openThroughTheRegistry(target());
    await settle();

    expect(screen.getByRole("alert")).toHaveTextContent("is not text keeper can edit");
    // Rendering `text ?? ""` would put an empty editable pane over a binary
    // file and offer to save it, which is how an editor overwrites a `.png`.
    expect(screen.queryByRole("tablist")).toBeNull();
    expect(document.querySelector(".cm-content")).toBeNull();
  });

  it("shows Rust's sentence when the file cannot be read at all", async () => {
    syncReadText.mockRejectedValue({ message: "inbox/config.json: no such file or directory" });
    openThroughTheRegistry(target());
    await settle();

    expect(screen.getByRole("alert")).toHaveTextContent("no such file or directory");
  });

  it("says a file outside every profile can be shown but not written", async () => {
    openThroughTheRegistry(target({ profileId: null }));
    await settle();

    // The hook never calls a command it cannot scope — reading through
    // `absolutePath` would go around browse.rs's containment (AD-65).
    expect(syncReadText).not.toHaveBeenCalled();
    expect(screen.getByRole("alert")).toHaveTextContent("not inside a synced folder");
  });
});

/** The modifier CodeMirror's `Mod-s` resolves to **in this test environment**.
 *
 *  A constant, not a browser check. `src/test/no-user-agent-gating.test.ts`
 *  forbids asking the browser which platform it is anywhere under `src/`,
 *  because in this app that answer comes from the Rust capabilities handshake
 *  and a client-side guess is how the rule rots. Here the constant is also the
 *  honest value: jsdom presents itself as something other than a Mac, so
 *  CodeMirror binds `Mod` to Ctrl, and a Cmd-flagged event would match nothing,
 *  assert nothing, and still pass. */
const MOD = { ctrlKey: true };

/** Type into the real editor, the way an edit actually arrives. */
async function retype(editor: HTMLElement, text: string): Promise<void> {
  await act(async () => {
    const view = EditorView.findFromDOM(editor);
    view?.dispatch({ changes: { from: 0, to: view.state.doc.length, insert: text } });
  });
  await settle();
}

describe("saving goes through Story 45.3's one write path", () => {
  it("writes the exact buffer to the profile and subpath it was given", async () => {
    syncReadText.mockResolvedValue(vm({ text: "hello\n" }));
    syncWriteEntry.mockResolvedValue(undefined);
    openThroughTheRegistry(target({ name: "notes.txt", relativePath: "inbox/notes.txt" }));
    const editor = await editorHost();

    // Through the real CodeMirror the raw view mounts, not through a stand-in:
    // the claim is that the characters the reader produced are the characters
    // the write command receives, tabs and trailing newline included.
    await retype(editor, "goodbye\n\tindented\n");
    fireEvent.keyDown(editor, { key: "s", ...MOD });
    await settle();

    expect(syncWriteEntry).toHaveBeenCalledWith(
      "profile-1",
      "inbox/notes.txt",
      "goodbye\n\tindented\n",
    );
  });

  it("declines out loud, and does not write, when nothing changed", async () => {
    const info = vi.spyOn(console, "info").mockImplementation(() => {});
    syncReadText.mockResolvedValue(vm({ text: "hello\n" }));
    openThroughTheRegistry(target({ name: "notes.txt", relativePath: "inbox/notes.txt" }));
    const editor = await editorHost();

    fireEvent.keyDown(editor, { key: "s", ...MOD });
    await settle();

    expect(syncWriteEntry).not.toHaveBeenCalled();
    // DW-162: a save that silently does nothing looks like a save that worked.
    expect(info).toHaveBeenCalledWith(expect.stringContaining("nothing changed"));
    info.mockRestore();
  });

  it("keeps a Windows file's line endings when one word is edited", async () => {
    syncReadText.mockResolvedValue(vm({ text: "alpha\r\nbeta\r\ngamma\r\n" }));
    syncWriteEntry.mockResolvedValue(undefined);
    openThroughTheRegistry(target({ name: "notes.txt", relativePath: "inbox/notes.txt" }));
    const editor = await editorHost();

    // Edited IN PLACE, by position, deliberately: replacing the whole document
    // with a CRLF string would re-introduce the terminators as ordinary
    // characters and hide the thing being asserted. What has to survive is the
    // text the editor was CONSTRUCTED with, because that is where a normalising
    // buffer does its damage — one word retyped, every line in the file
    // changed, and a whole-file diff on the next sync of a file the reader
    // believes they barely touched.
    //
    // `TextFileVm`'s own doc comment promises the opposite in its own words:
    // "no line-ending normalisation ... a file opened and saved untouched is
    // the same file, which is the only thing that makes an editor over synced
    // content safe to use at all". Rust keeps that promise; this asserts the
    // editor above it does too.
    await act(async () => {
      const view = EditorView.findFromDOM(editor);
      const at = view?.state.doc.toString().indexOf("beta") ?? -1;
      view?.dispatch({ changes: { from: at, to: at + 4, insert: "BETA" } });
    });
    await settle();
    fireEvent.keyDown(editor, { key: "s", ...MOD });
    await settle();

    expect(syncWriteEntry).toHaveBeenCalledWith(
      "profile-1",
      "inbox/notes.txt",
      "alpha\r\nBETA\r\ngamma\r\n",
    );
  });

  it("puts Rust's refusal of a save where the reader is looking", async () => {
    syncReadText.mockResolvedValue(vm({ text: "hello\n" }));
    syncWriteEntry.mockRejectedValue({
      message: "inbox/notes.txt is on a read-only volume, so keeper did not write it",
    });
    openThroughTheRegistry(target({ name: "notes.txt", relativePath: "inbox/notes.txt" }));
    const editor = await editorHost();

    await retype(editor, "goodbye\n");
    fireEvent.keyDown(editor, { key: "s", ...MOD });
    await settle();

    // Whether a LOCATION can be written is Rust's answer and it arrives here,
    // as a sentence. A viewer that swallowed it would leave the reader
    // believing a file was saved that was not.
    expect(screen.getByRole("alert")).toHaveTextContent("read-only volume");
    // And the buffer is not rolled back: losing what somebody typed is worse
    // than showing text the disk does not have yet.
    expect(EditorView.findFromDOM(editor)?.state.doc.toString()).toBe("goodbye\n");
  });

  it("refuses a format keeper must not rewrite, by name, before an edit is possible", async () => {
    syncReadText.mockResolvedValue(vm({ text: "hello\n" }));
    const { entry } = viewerComponentFor(target({ name: "notes.txt" }));

    // Built by hand on purpose. No `viewer: "text"` row is non-writable today,
    // so the registry cannot produce this input — and a guard that only runs on
    // inputs the current table cannot produce is precisely the guard that rots
    // unnoticed until the row that needs it is added.
    render(
      <TextFileViewer
        file={target({ name: "notes.txt", relativePath: "inbox/notes.txt" })}
        entry={{ ...entry, writable: false, label: "Locked" }}
      />,
    );
    await editorHost();

    expect(screen.getByRole("status")).toHaveTextContent("keeper does not write Locked files");
  });
});

describe("the CSV table cannot be reached from a panel yet, and says so", () => {
  it("opens a CSV as its source and names what is missing", async () => {
    syncReadText.mockResolvedValue(vm({ text: "name,qty\nwidget,3\n" }));
    openThroughTheRegistry(target({ name: "rows.csv", relativePath: "inbox/rows.csv" }));
    const editor = await editorHost();

    // Pinned deliberately. A panel holds a sync profile id; 44.16's commands
    // want a notes vault id; deriving one from the other in the webview is the
    // path arithmetic AD-65 forbids, and Story 45.18 owns the resolution. When
    // it lands, this assertion is the one that should change.
    expect(screen.getByRole("alert")).toHaveTextContent("inside a notes vault");
    expect(editor.textContent).toContain("widget");
  });
});
