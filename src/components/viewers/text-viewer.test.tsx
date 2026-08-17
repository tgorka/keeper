/**
 * Story 45.6, the half a module test cannot reach.
 *
 * `text-editor-host.test.ts` proves the pieces. This suite mounts the real
 * `TextEditorSurface` — its own boot effect, its own dynamic imports, a real
 * `EditorView` in a real host element — and types at the real content DOM.
 *
 * DW-171 is the reason it is written this way. The `mermaid` fence defect got
 * through because no test ever assembled the markdown language *and* the plugin
 * into one view: every suite tested a piece over a stack it built itself, and
 * the product's own stack was never constructed anywhere. The same trap is open
 * here in two places — the read-only guard is an `EditorView.editable` facet
 * that only means anything inside a mounted view, and the `content` prop is
 * reconciled by a dispatch that only exists once there is a document to
 * dispatch against. Both are asserted below by typing, not by calling.
 */
import type { EditorView } from "@codemirror/view";
import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { useState } from "react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { withRangeRects } from "@/test/layout";
import { TEXT_EDIT_MAX_BYTES } from "./text-editor-host";
import { TextEditorSurface } from "./text-viewer";

// jsdom has no `Range.getClientRects`, and CodeMirror's measure pass calls it on
// any frame that elapses mid-test. The throw escapes every `try` and takes the
// run's exit code with it, and whether it fires at all depends on how slow the
// machine was — so it presents as a flaky suite rather than a missing stub.
// `src/test/layout.ts` owns the answer; re-deriving it here would be a second
// set of fake metrics for the same jsdom gap.
let undoRects: (() => void) | null = null;

beforeEach(() => {
  undoRects = withRangeRects();
});

afterEach(() => {
  undoRects?.();
  undoRects = null;
});

/** The live editor's content DOM, once the lazy chunk has landed. */
async function content(expected: string): Promise<HTMLElement> {
  return await waitFor(() => {
    const node = document.querySelector<HTMLElement>(".cm-content");
    expect(node).not.toBeNull();
    expect(node?.closest(".cm-editor")).not.toBeNull();
    expect(document.querySelector(".cm-content")?.textContent).toContain(
      expected.split("\n")[0] ?? "",
    );
    return node as HTMLElement;
  });
}

/**
 * The live `EditorView`, found through the DOM rather than handed over.
 *
 * Through `findFromDOM` on purpose: it proves the view a test is asserting
 * against is the one the component actually mounted into the page, not one the
 * test built. The `EditorView` name is imported as a TYPE at the top and as a
 * VALUE here — a value import at module scope would pull the whole editor into
 * the test's own graph and hide whether the component lazily loaded it.
 */
async function liveView(): Promise<EditorView> {
  const editor = document.querySelector(".cm-editor");
  expect(editor).not.toBeNull();
  const view = (await import("@codemirror/view")).EditorView.findFromDOM(editor as HTMLElement);
  expect(view).not.toBeNull();
  return view as EditorView;
}

/**
 * Put text into the document the way a *user* does, through CodeMirror's own
 * input handling.
 *
 * A paste, not a `view.dispatch`. That distinction is the whole reason this
 * suite exists: `dispatch` applies a change to the state regardless of every
 * read-only facet, so a test built on it would pass over an editor that had no
 * read-only guard at all. `handlers.paste` is a real user path, it is gated on
 * `EditorState.readOnly`, and jsdom can deliver it — whereas jsdom does not
 * implement `contenteditable`, so a `beforeinput` never mutates anything for
 * CodeMirror's DOM observer to read back.
 */
function type(node: HTMLElement, text: string): void {
  fireEvent.paste(node, {
    clipboardData: {
      getData: (mime: string) => (mime === "text/plain" ? text : ""),
      types: ["text/plain"],
    },
  });
}

/** A host that owns the buffer, exactly as 45.4's raw view will. */
function Controlled({
  initial,
  language = null,
  fileName = "config.toml",
  readOnly,
  sizeLabel,
  onChange,
  onSave,
  writingTools,
}: {
  initial: string;
  language?: string | null;
  fileName?: string;
  readOnly?: boolean;
  sizeLabel?: string;
  onChange?: (next: string) => void;
  onSave?: (next: string) => void;
  writingTools?: boolean;
}) {
  const [text, setText] = useState(initial);
  return (
    <>
      <TextEditorSurface
        content={text}
        language={language}
        fileName={fileName}
        sizeLabel={sizeLabel}
        writingTools={writingTools}
        readOnly={readOnly}
        onChange={(next) => {
          setText(next);
          onChange?.(next);
        }}
        onSave={onSave}
      />
      <output data-testid="buffer">{text}</output>
    </>
  );
}

describe("TextEditorSurface, in a real EditorView", () => {
  it("opens with the file's content byte for byte, including a tab and a CRLF", async () => {
    // The three shapes a naive editor silently rewrites: a hard tab, CRLF line
    // endings, and no trailing newline. All three have to survive an open.
    const body = "a\tb\r\nsecond line\r\nno trailing newline";
    render(<Controlled initial={body} />);
    await content(body);

    const view = await liveView();

    expect(view.state.doc.toString()).toBe(body);
  });

  it("carries a trailing newline, and its absence, out through onChange", async () => {
    const withNewline: string[] = [];
    // `{"one\n"}` and not `"one\n"`: a JSX string attribute does not process
    // escapes, so the plain form would have made this a two-character
    // backslash-n and the test would have proved nothing about newlines at all.
    const first = render(
      <Controlled initial={"one\n"} onChange={(next) => withNewline.push(next)} />,
    );
    const node = await content("one");
    const view = await liveView();
    // At the very end of the document, which for a file ending in a newline is
    // the start of an empty last line — the position a trimming editor loses.
    view.dispatch({ selection: { anchor: view.state.doc.length } });

    type(node, "!");

    // `[length - 1]` and not `.at(-1)`: this project's `lib` is ES2020, so
    // `Array.prototype.at` runs under Bun and fails `tsc --noEmit`.
    await waitFor(() => expect(withNewline[withNewline.length - 1]).toBe("one\n!"));
    first.unmount();

    // And the file that has no trailing newline keeps not having one.
    const withoutNewline: string[] = [];
    render(<Controlled initial="two" onChange={(next) => withoutNewline.push(next)} />);
    const second = await content("two");
    const view2 = await liveView();
    view2.dispatch({ selection: { anchor: view2.state.doc.length } });

    type(second, "!");

    await waitFor(() => expect(withoutNewline[withoutNewline.length - 1]).toBe("two!"));
    expect(withoutNewline[withoutNewline.length - 1]?.endsWith("\n")).toBe(false);
  });

  it("reports the buffer byte for byte on every keystroke", async () => {
    const seen: string[] = [];
    render(<Controlled initial="" onChange={(next) => seen.push(next)} />);
    const node = await content("");

    type(node, "x");
    await waitFor(() => expect(seen).toEqual(["x"]));
    type(node, "y");

    await waitFor(() => expect(seen).toEqual(["x", "xy"]));
  });

  it("adopts a content prop change without remounting the editor", async () => {
    function Outside() {
      const [text, setText] = useState("first");
      return (
        <>
          <TextEditorSurface content={text} language={null} fileName="a.txt" />
          <button type="button" onClick={() => setText("second")}>
            replace
          </button>
        </>
      );
    }
    render(<Outside />);
    await content("first");
    const before = await liveView();

    fireEvent.click(document.querySelector("button") as HTMLElement);

    await waitFor(() => {
      expect(before.state.doc.toString()).toBe("second");
    });
    // The same view object: a remount would lose the caret, the selection and
    // the undo stack, which is what makes 45.4's write-through survivable.
    expect(await liveView()).toBe(before);
  });

  it("rebuilds for another file of the same name, so no undo crosses between them", async () => {
    // The same defect story 51.5's markdown pane had, on this side of the toggle:
    // nothing above this component remounts it when a panel replaces its target,
    // so keying only on the grammar left one undo history spanning two files —
    // one undo restores the other file's text, `onChange` reports it, and the
    // next save writes it here. `fileName` is identical in both renders on
    // purpose: a display name is not an identity.
    function Outside() {
      const [inLog, setInLog] = useState(true);
      return (
        <>
          <TextEditorSurface
            content={inLog ? "the log's plan" : "the root plan"}
            language={null}
            fileName="plan.md"
            loadedFrom={{
              profileOrVaultId: "p1",
              relativePath: inLog ? "log/plan.md" : "plan.md",
            }}
          />
          <button type="button" onClick={() => setInLog(false)}>
            open the other one
          </button>
        </>
      );
    }
    render(<Outside />);
    await content("the log's plan");
    // Imported here rather than at module scope, for the reason `liveView` gives
    // about `EditorView`: a static value import would pull the editing commands
    // into this test's own graph and hide whether the component loaded them.
    const { undoDepth } = await import("@codemirror/commands");
    const before = await liveView();
    before.dispatch({
      changes: { from: before.state.doc.length, insert: "!" },
      userEvent: "input.type",
    });
    expect(undoDepth(before.state)).toBeGreaterThan(0);

    fireEvent.click(document.querySelector("button") as HTMLElement);
    await content("the root plan");

    const after = await liveView();
    expect(after).not.toBe(before);
    expect(undoDepth(after.state)).toBe(0);
  });

  it("does not re-dispatch when the prop comes back unchanged", async () => {
    // The controlled-input loop, which runs on EVERY keystroke: type -> onChange
    // -> parent state -> the same string back as `content` -> the reconcile
    // effect. If the surface dispatched that identical string as a whole-document
    // replacement, CodeMirror would map the caret through the change and land it
    // at the END of the file — so typing in the middle of a config file would
    // teleport you to the bottom of it after every character.
    //
    // The caret is put in the MIDDLE deliberately: a caret already at the end
    // maps to the end either way, and a test written that way passes over an
    // editor with no guard at all.
    render(<Controlled initial="abcdef" />);
    const node = await content("abcdef");
    const view = await liveView();
    view.dispatch({ selection: { anchor: 2 } });

    type(node, "X");

    await waitFor(() => expect(view.state.doc.toString()).toBe("abXcdef"));
    expect(view.state.selection.main.head).toBe(3);
  });

  it("ignores a programmatic change while read-only, rather than reporting it", async () => {
    // Belt as well as braces, and the braces have a gap. `EditorView.editable`
    // and `EditorState.readOnly` stop a PERSON; neither stops `view.dispatch`,
    // and 45.4's rendered CSV view holds this very buffer and dispatches into
    // it. An edit escaping as `onChange` on an oversize file would hand a
    // truncated prefix to a save, which is the one failure that loses a file.
    const seen: string[] = [];
    render(<Controlled initial="fixed" readOnly onChange={(next) => seen.push(next)} />);
    await content("fixed");
    const view = await liveView();

    view.dispatch({ changes: { from: 0, to: 0, insert: "smuggled " } });

    // The document did change — `dispatch` is allowed to. What must not happen
    // is the surface telling its parent that the user edited the file.
    expect(view.state.doc.toString()).toBe("smuggled fixed");
    expect(seen).toEqual([]);
  });

  it("saves exactly what was typed, on Mod-s", async () => {
    const saved: string[] = [];
    render(<Controlled initial="line" onSave={(next) => saved.push(next)} />);
    const node = await content("line");
    const view = await liveView();
    view.dispatch({ selection: { anchor: view.state.doc.length } });

    type(node, "!");
    await waitFor(() => expect(view.state.doc.toString()).toBe("line!"));
    fireEvent.keyDown(node, { key: "s", code: "KeyS", ctrlKey: true, cancelable: true });

    await waitFor(() => expect(saved).toEqual(["line!"]));
  });

  it("claims Tab, so 43.1's fix is not undone by a second editor", async () => {
    // The whole reason this story reuses the note editor's host. An unclaimed
    // Tab escapes to the web view, which edits the DOM under CodeMirror.
    render(<Controlled initial="alpha" />);
    const node = await content("alpha");
    const view = await liveView();
    view.dispatch({ selection: { anchor: 0 } });

    const notCancelled = fireEvent.keyDown(node, {
      key: "Tab",
      code: "Tab",
      keyCode: 9,
      cancelable: true,
    });

    expect(notCancelled).toBe(false);
    await waitFor(() => expect(view.state.doc.toString()).toBe("  alpha"));
    expect(view.state.doc.toString()).not.toContain("\t");
  });

  it("keeps 43.1's escape hatch: Escape then Tab leaves and writes nothing", async () => {
    render(<Controlled initial="alpha" />);
    const node = await content("alpha");
    const view = await liveView();

    fireEvent.keyDown(node, { key: "Escape", code: "Escape", keyCode: 27, cancelable: true });
    const notCancelled = fireEvent.keyDown(node, {
      key: "Tab",
      code: "Tab",
      keyCode: 9,
      cancelable: true,
    });

    expect(notCancelled).toBe(true);
    expect(view.state.doc.toString()).toBe("alpha");
  });
});

describe("TextEditorSurface, at the size limit", () => {
  /** One byte under the limit, so the editor must still be live. */
  const justUnder = "a".repeat(TEXT_EDIT_MAX_BYTES - 1);
  /** One byte over. The boundary is `>`, and a test on one side cannot say so. */
  const justOver = "a".repeat(TEXT_EDIT_MAX_BYTES + 1);

  it("a file just under the limit is editable and shows no banner", async () => {
    const seen: string[] = [];
    render(<Controlled initial={justUnder} onChange={(next) => seen.push(next)} />);
    const node = await content("aaa");
    const view = await liveView();
    view.dispatch({ selection: { anchor: 0 } });

    type(node, "z");

    await waitFor(() => expect(seen).toHaveLength(1));
    expect(seen[0]?.startsWith("za")).toBe(true);
    expect(document.querySelector('[data-testid="text-viewer-oversize"]')).toBeNull();
  });

  it("a file over the limit opens read-only, names its size, and refuses input", async () => {
    const seen: string[] = [];
    render(
      <Controlled initial={justOver} sizeLabel="4.2 MB" onChange={(next) => seen.push(next)} />,
    );
    const node = await content("aaa");

    const banner = document.querySelector('[data-testid="text-viewer-oversize"]');
    expect(banner?.textContent).toContain("4.2 MB");
    expect(banner?.textContent).toContain("read-only");

    const view = await liveView();
    view.dispatch({ selection: { anchor: 0 } });
    type(node, "z");

    // Not "eventually not called" — the editable facet refuses the input
    // synchronously, and the document is unchanged.
    expect(view.state.doc.length).toBe(justOver.length);
    expect(seen).toEqual([]);
  });

  it("names no size rather than a wrong one when the caller has no label", async () => {
    render(<Controlled initial={justOver} />);
    await content("aaa");

    const banner = document.querySelector('[data-testid="text-viewer-oversize"]');
    expect(banner?.textContent).toContain("too large to edit");
    // A TypeScript byte formatter is forbidden (Rust owns the words), so the
    // honest degraded message names no size at all rather than inventing one.
    expect(banner?.textContent).not.toMatch(/\d+(\.\d+)?\s?(bytes|kB|MB|GB)/);
  });

  it("measures UTF-8 bytes, not UTF-16 units, so Rust and the browser agree", async () => {
    // 400 000 three-byte characters is 1 200 000 bytes but only 400 000
    // `String.length` units. Measured with `.length` this file would open
    // editable while Rust had already refused it as oversize — and the two
    // would disagree about the same file.
    const wide = "☃".repeat(400_000);
    render(<Controlled initial={wide} sizeLabel="1.2 MB" />);
    await content("☃");

    expect(document.querySelector('[data-testid="text-viewer-oversize"]')?.textContent).toContain(
      "1.2 MB",
    );
  });

  it("an explicit readOnly file refuses input even below the limit", async () => {
    const seen: string[] = [];
    render(<Controlled initial="fixed" readOnly onChange={(next) => seen.push(next)} />);
    const node = await content("fixed");
    const view = await liveView();
    view.dispatch({ selection: { anchor: 0 } });

    type(node, "z");

    expect(view.state.doc.toString()).toBe("fixed");
    expect(seen).toEqual([]);
    // No banner: this file is not too big, it is simply not writable here, and
    // saying "too large" about it would be a lie.
    expect(document.querySelector('[data-testid="text-viewer-oversize"]')).toBeNull();
  });
});

describe("TextEditorSurface, when a grammar will not load", () => {
  it("still opens, still edits, and says why at INFO", async () => {
    // Main's condition on the dependency: a chunk that will not fetch must
    // leave a monochrome but fully working editor, never a broken pane.
    vi.resetModules();
    vi.doMock("@codemirror/legacy-modes/mode/toml", () => {
      throw new Error("chunk 4f2a failed to load");
    });
    const info = vi.spyOn(console, "info").mockImplementation(() => {});
    try {
      const { TextEditorSurface: Fresh } = await import("./text-viewer");
      const seen: string[] = [];
      function Host() {
        const [text, setText] = useState("key = 1");
        return (
          <Fresh
            content={text}
            language="toml"
            fileName="config.toml"
            onChange={(next) => {
              setText(next);
              seen.push(next);
            }}
          />
        );
      }
      render(<Host />);
      const node = await content("key = 1");
      const view = await liveView();
      view.dispatch({ selection: { anchor: view.state.doc.length } });

      type(node, "0");

      await waitFor(() => expect(seen).toEqual(["key = 10"]));
      await waitFor(() => {
        expect(info.mock.calls.some((call) => String(call[0]).includes("toml"))).toBe(true);
      });
    } finally {
      info.mockRestore();
      vi.doUnmock("@codemirror/legacy-modes/mode/toml");
      vi.resetModules();
    }
  });

  it("says nothing for the ids that are text with no syntax", async () => {
    // `plain` and `csv` have no grammar ON PURPOSE — a CSV's structure is 45.4's
    // table, not a comma tokeniser. Logging "no grammar is wired" for them would
    // put a line in the console for the two most common files in a vault, and a
    // log that cries wolf is a log nobody reads when a row really is missing.
    const info = vi.spyOn(console, "info").mockImplementation(() => {});
    try {
      for (const language of ["plain", "csv", null]) {
        const view = render(
          <TextEditorSurface content="a,b" language={language} fileName="t.csv" />,
        );
        await content("a,b");
        view.unmount();
      }

      expect(info.mock.calls.map((call) => String(call[0]))).toEqual([]);
    } finally {
      info.mockRestore();
    }
  });

  it("names an id nobody wired, so a missing binding is visible (DW-172)", async () => {
    // `php` is in the registry's vocabulary and has no tokeniser in this build.
    // The file still opens and still edits; what must not happen is that it goes
    // monochrome with nothing anywhere saying why.
    const info = vi.spyOn(console, "info").mockImplementation(() => {});
    try {
      render(<TextEditorSurface content="<?php" language="php" fileName="index.php" />);
      await content("<?php");

      await waitFor(() => {
        expect(info.mock.calls.some((call) => String(call[0]).includes('"php"'))).toBe(true);
      });
    } finally {
      info.mockRestore();
    }
  });
});

/**
 * Story 50.3, the surface's own half of the rule.
 *
 * `text-file-viewer.test.tsx` proves the shipped path: a session log opened
 * through the registry has the toolbar, the menu and the emoji. What only this
 * suite can prove is the guard BELOW that decision — this component refuses the
 * tools over a buffer nobody can write even when its caller asks for them,
 * because it is mounted over buffers no `sync_read_text` produced (a note embed,
 * a paste) and "the toolbar edits nothing" must not depend on which caller you
 * are. The positive case is asserted beside it deliberately: an absence test
 * with no present case beside it stays green against a build that has no toolbar
 * anywhere.
 */
describe("TextEditorSurface, and the writing tools", () => {
  it("mounts the toolbar over a writable markdown buffer, and its press lands", async () => {
    render(<Controlled initial="alpha" language="markdown" fileName="README.md" writingTools />);
    await content("alpha");
    const view = await liveView();
    view.dispatch({ selection: { anchor: 0, head: "alpha".length } });

    fireEvent.click(screen.getByRole("button", { name: "Bold" }));

    // Read back through the host's buffer, not the view: what a toolbar owes is
    // an edit the surface REPORTS, because that is the only text a save can see.
    await waitFor(() => expect(screen.getByTestId("buffer")).toHaveTextContent("**alpha**"));
  });

  it("withholds them from a buffer nobody can write", async () => {
    render(
      <Controlled initial="alpha" language="markdown" fileName="README.md" writingTools readOnly />,
    );
    await content("alpha");

    // Absent rather than present-and-failing. A read-only markdown buffer is the
    // `workspace/` case (AD-113) and the oversize case, and a toolbar over
    // either would be a control that announces its own refusal.
    expect(screen.queryByRole("button", { name: "Bold" })).toBeNull();
  });

  it("withholds them from a buffer whose caller never asked", async () => {
    render(<Controlled initial="alpha" language="markdown" fileName="README.md" />);
    await content("alpha");

    // The registry's `format` verdict lives above this component, so the default
    // is off: a surface that has not thought about it gets the plain editor it
    // had before Story 50.3.
    expect(screen.queryByRole("button", { name: "Bold" })).toBeNull();
  });

  it("draws no toolbar over the host until the editor is inside it", async () => {
    // The window the commit's own claim denied. `writingTools` is known at the
    // first render and the editor is six dynamic imports away, so a toolbar
    // drawn from the flag alone is live and clickable over an empty host — and
    // every press in that window reaches a null mount and is swallowed, which
    // is exactly the shape `TextEditorMount.runFormat`'s null was chosen to
    // prevent. Asserted synchronously, before a single microtask has run.
    render(<Controlled initial="alpha" language="markdown" fileName="README.md" writingTools />);

    expect(document.querySelector(".cm-editor")).toBeNull();
    expect(screen.queryByRole("button", { name: "Bold" })).toBeNull();

    // …and it does arrive, so this is a claim about WHEN rather than a test
    // that would pass over a build with no toolbar at all.
    await content("alpha");
    await screen.findByRole("button", { name: "Bold" });
  });

  it("takes the toolbar away for the whole of a rebuild, not just its start", async () => {
    // The second, guaranteed instance: the extension list is fixed at
    // construction, so a change to `language` tears the view down and builds
    // another asynchronously — with `writingTools` true throughout. The cleanup
    // nulls the mount, so between it and the next resolve there is nothing for
    // a press to land in.
    const { rerender } = render(
      <Controlled initial="alpha" language="markdown" fileName="README.md" writingTools />,
    );
    await content("alpha");
    await screen.findByRole("button", { name: "Bold" });

    rerender(<Controlled initial="alpha" language="toml" fileName="README.md" writingTools />);

    // Synchronous again: React has run the cleanup and started the new mount,
    // and the new mount cannot have resolved.
    expect(document.querySelector(".cm-editor")).toBeNull();
    expect(screen.queryByRole("button", { name: "Bold" })).toBeNull();

    await content("alpha");
    await screen.findByRole("button", { name: "Bold" });
  });
});
