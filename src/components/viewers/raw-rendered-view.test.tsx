/**
 * The raw/rendered toggle, against a real render (Story 45.4, 45.10).
 *
 * The raw half is a REAL controlled `<textarea>` passed in as the editor,
 * not a mock of 45.6's module. What is being asserted here is that this
 * component holds one buffer and hands the exact characters back — and a mocked
 * module would let that hold for a component that had quietly grown a second
 * copy of the text. The markdown pane mounts a REAL `EditorView` with the real
 * decoration layer, for the reason DW-165 existed.
 *
 * **The one seam.** Two tests here are about the *refusal lifecycle* — a
 * refusal names itself, and a refusal does not outlive the bytes it was about
 * — and they need a document the renderer cannot draw. Until Story 45.10 that
 * document was a ```mermaid fence, because DW-165 made one throw. It draws now,
 * so those two tests force the failure through `mountMarkdownPreview` instead.
 * The alternative was to keep asserting the lifecycle over a document that no
 * longer refuses, which is a test that proves nothing, or to delete the
 * lifecycle coverage along with the defect that happened to trigger it.
 */
import "@codemirror/lang-markdown";
import "@codemirror/language";
import "@codemirror/state";
import "@/components/notes/editor/live-preview";
// Story 55.3's `==` delimiter table, awaited unconditionally by the mount —
// warmed for the same reason as the four above.
import "@/components/notes/editor/markdown-marks";
// Note mode's mount awaits three more chunks — the editing keymap, Story 43.1's
// Tab bindings and (story 52.3) the writing tools — so they are warmed here for
// the same reason as the four above: `settle()` drains microtasks and never a
// frame, and a cold `import()` would not have resolved by the time it returns.
// The writing tools pull the completion package and the emoji table behind them,
// which is precisely why they are lazy in the product and why a cold registry
// leaves the pane un-mounted for longer than eight ticks here.
import "@/components/notes/editor/indent-keymap";
import "@/components/notes/editor/writing-tools";
import { completionStatus, currentCompletions, startCompletion } from "@codemirror/autocomplete";
import { undo, undoDepth } from "@codemirror/commands";
import { EditorView } from "@codemirror/view";
import { act, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { useState } from "react";
import { afterAll, beforeAll, beforeEach, describe, expect, it, vi } from "vitest";
import { SLASH_COMMANDS } from "@/components/notes/editor/slash-menu";
import { matchEmoji } from "@/lib/emoji/match";
import type { NoteCsvVm } from "@/lib/ipc/client";
import { withRangeRects } from "@/test/layout";
import type { MarkdownPreview } from "./markdown-preview";
import type { RawEditorProps } from "./raw-rendered-view";
import { RawRenderedView } from "./raw-rendered-view";
import type { FileOrigin } from "./use-text-file";
import { VIEW_MODE_COOKIE } from "./view-mode";

/** The sentence the forced failure returns, when one is forced. Null means the
 *  real renderer runs, which is what every other test in this file gets. */
let forcedPreviewFailure: string | null = null;

/** The sentence a forced ADOPTION failure returns — the other half of the
 *  lifecycle, reachable only since `setContent` existed. Null means the real
 *  view adopts. */
let forcedAdoptFailure: string | null = null;

/** Every `vaultId` a pane was constructed with, in order. The pane reads its
 *  options once, so this is what says whether a hydrated vault was picked up. */
const mountedVaults: (string | null | undefined)[] = [];

/**
 * How many panes were constructed, and how many were destroyed.
 *
 * The counter exists because the leak it guards is invisible to the DOM: a mount
 * that resolves after its effect was cleaned up parents a real `EditorView` into
 * a host React has already detached, so every `querySelector` says the pane is
 * gone while a keymap and a measure loop are still live on it.
 */
const panes = { built: 0, destroyed: 0 };

vi.mock("./markdown-preview", async (importOriginal) => {
  const actual = await importOriginal<typeof import("./markdown-preview")>();
  return {
    ...actual,
    mountMarkdownPreview: async (
      host: HTMLElement,
      text: string,
      options: Parameters<typeof actual.mountMarkdownPreview>[2],
    ): Promise<MarkdownPreview> => {
      mountedVaults.push(options.vaultId);
      panes.built += 1;
      if (forcedPreviewFailure === null) {
        const real = await actual.mountMarkdownPreview(host, text, options);
        // Wrapped rather than replaced: the view is the real one, and only the
        // outcomes no document reliably produces are substituted.
        return {
          ...real,
          setContent: (next: string) => forcedAdoptFailure ?? real.setContent(next),
          destroy: () => {
            panes.destroyed += 1;
            real.destroy();
          },
        };
      }
      // The real module empties the host on failure so the reader never sees a
      // fragment of a render; the double has to do the same or the assertions
      // about what is left behind would be about the double.
      host.replaceChildren();
      return {
        failure: forcedPreviewFailure,
        setContent: () => null,
        // Nothing was mounted, so a toolbar press has nowhere to land — which is
        // what the real module answers for a document it could not draw.
        runFormat: null,
        destroy: () => {
          panes.destroyed += 1;
        },
      };
    },
  };
});

/**
 * Drain the microtasks the markdown mount rides on, and nothing else.
 *
 * Deliberately not `waitFor`, which advances timers: letting a frame run starts
 * CodeMirror's measure pass, jsdom has no `Range.getClientRects` for it to
 * measure with, and the throw leaves the view without the decorations this file
 * is asserting about. That is a fact about jsdom, not about the feature — the
 * same trade `recording-embed.test.ts` makes, and the reason the CodeMirror
 * packages are imported statically above: a warm module registry makes
 * `mountMarkdownPreview`'s `import()` resolve in microtasks.
 */
async function settle(): Promise<void> {
  await act(async () => {
    for (let tick = 0; tick < 8; tick += 1) {
      await Promise.resolve();
    }
  });
}

/** A real controlled editor. Story 45.6's surface is one of these with a
 *  CodeMirror in it; what this component needs from it is `content` in and the
 *  exact characters out, and that is what this asserts against. */
function TestEditor({
  content,
  onChange,
  onSave,
  readOnly,
  language,
  fileName,
  sizeLabel,
}: RawEditorProps): React.ReactElement {
  return (
    <textarea
      aria-label={`Source of ${fileName}`}
      value={content}
      readOnly={readOnly === true}
      data-language={language ?? "none"}
      data-size-label={sizeLabel ?? ""}
      onChange={(event) => onChange?.(event.target.value)}
      // This double reads `metaKey` itself, so the test's keydown and this
      // handler agree by construction. NOT the jsdom trap: CodeMirror's `Mod`
      // resolves to `Ctrl` on a non-Mac platform and jsdom reports one, so a
      // `metaKey` keydown aimed at a REAL editor matches nothing and the
      // assertion passes because nothing happened. Where a real editor is
      // driven — `text-file-viewer.test.tsx` — the modifier is chosen with
      // CodeMirror's own predicate.
      onKeyDown={(event) => {
        if (event.key === "s" && event.metaKey) {
          event.preventDefault();
          void onSave?.(event.currentTarget.value);
        }
      }}
    />
  );
}

/** A cookie jar that is not the document's, so one test cannot see another's
 *  preference. The real jar is exercised once, deliberately, below. */
function jar(initial = ""): { read: () => string; write: (value: string) => void } {
  let stored = initial;
  return {
    read: () => stored,
    write: (assignment) => {
      stored = assignment.slice(0, assignment.indexOf(";"));
    },
  };
}

type HostProps = Omit<
  React.ComponentProps<typeof RawRenderedView>,
  "editor" | "content" | "loadedFrom"
> & {
  initial: string;
  onSaved?: (text: string) => void;
  /** Where the buffer was read from. Defaulted to the display name under one
   *  profile, which is what a flat panel produces — the tests that are ABOUT
   *  two files with one name pass their own. */
  loadedFrom?: FileOrigin;
};

/** The host owns the buffer, exactly as 45.6's loading hook does. */
function Host({ initial, onSaved, loadedFrom, ...rest }: HostProps): React.ReactElement {
  const [content, setContent] = useState(initial);
  return (
    <RawRenderedView
      {...rest}
      loadedFrom={loadedFrom ?? { profileOrVaultId: "profile-1", relativePath: rest.fileName }}
      content={content}
      editor={TestEditor}
      onChange={setContent}
      onSave={(text) => onSaved?.(text)}
    />
  );
}

const CSV_TABLE: NoteCsvVm = {
  relPath: "data/rows.csv",
  rev: "rev-1",
  columns: 2,
  totalRows: 2,
  rows: [
    { index: 0, line: 1, cells: ["name", "qty"], ragged: false },
    { index: 1, line: 2, cells: ["widget", "3"], ragged: false },
  ],
  notices: [],
};

/**
 * jsdom has no `Range.getClientRects`, and CodeMirror's measure pass — which
 * runs on any animation frame that elapses during a test — calls it. Without
 * this the run throws an unhandled error at a time that depends on how slow the
 * machine was, which is a suite that is green until it is not.
 */
let removeRangeRects: (() => void) | null = null;
beforeAll(() => {
  removeRangeRects = withRangeRects();
});
afterAll(() => {
  removeRangeRects?.();
});

beforeEach(() => {
  vi.restoreAllMocks();
  forcedPreviewFailure = null;
  forcedAdoptFailure = null;
  mountedVaults.length = 0;
  panes.built = 0;
  panes.destroyed = 0;
});

describe("the toggle remembers per format, not per file", () => {
  it("keeps a format's chosen view across files, and keeps the formats apart", () => {
    const cookie = jar();
    const json = { format: "json", rendered: "structure", language: "json", cookie } as const;

    const first = render(<Host {...json} fileName="a.json" initial='{"a":1}' />);
    expect(screen.getByRole("tab", { name: "Structure" })).toHaveAttribute("aria-selected", "true");
    fireEvent.click(screen.getByRole("tab", { name: "Source" }));
    expect(screen.getByLabelText("Source of a.json")).toBeInTheDocument();
    first.unmount();

    // A DIFFERENT file, same format: the preference travels.
    const second = render(<Host {...json} fileName="b.json" initial='{"b":2}' />);
    expect(screen.getByRole("tab", { name: "Source" })).toHaveAttribute("aria-selected", "true");
    expect(screen.getByLabelText("Source of b.json")).toHaveValue('{"b":2}');
    second.unmount();

    // A different FORMAT is untouched by that choice.
    render(
      <Host
        format="csv"
        rendered="table"
        language="csv"
        cookie={cookie}
        fileName="c.csv"
        initial={"a,b\n"}
        csv={{ vaultId: "vault-1", target: "c.csv" }}
        csvOptions={{ read: async () => CSV_TABLE }}
      />,
    );
    expect(screen.getByRole("tab", { name: "Table" })).toHaveAttribute("aria-selected", "true");
  });

  it("adopts the new format's remembered view when the same mount changes file", () => {
    const cookie = jar(`${VIEW_MODE_COOKIE}=json%3Araw`);
    const view = render(
      <Host
        format="markdown"
        rendered="markdown"
        language="markdown"
        cookie={cookie}
        fileName="a.md"
        initial={"# hi\n"}
      />,
    );
    expect(screen.getByRole("tab", { name: "Preview" })).toHaveAttribute("aria-selected", "true");

    view.rerender(
      <Host
        format="json"
        rendered="structure"
        language="json"
        cookie={cookie}
        fileName="a.json"
        initial='{"a":1}'
      />,
    );
    // Markdown's default was never written, and JSON's stored `raw` is honoured
    // during the render rather than a frame later.
    expect(screen.getByRole("tab", { name: "Source" })).toHaveAttribute("aria-selected", "true");
  });

  it("writes the choice to the real document cookie when no jar is supplied", () => {
    // biome-ignore lint/suspicious/noDocumentCookie: arranging or clearing cookie state is this test's subject
    document.cookie = `${VIEW_MODE_COOKIE}=; path=/; max-age=0`;
    render(
      <Host format="json" rendered="structure" language="json" fileName="a.json" initial="{}" />,
    );
    fireEvent.click(screen.getByRole("tab", { name: "Source" }));
    expect(document.cookie).toContain(`${VIEW_MODE_COOKIE}=json%3Araw`);
    // biome-ignore lint/suspicious/noDocumentCookie: arranging or clearing cookie state is this test's subject
    document.cookie = `${VIEW_MODE_COOKIE}=; path=/; max-age=0`;
  });

  it("offers no toggle at all for a format whose only view is raw", () => {
    render(
      <Host format="plain" rendered={null} language="plain" fileName="notes.txt" initial="hello" />,
    );
    expect(screen.queryByRole("tablist")).toBeNull();
    expect(screen.getByLabelText("Source of notes.txt")).toHaveValue("hello");
  });
});

describe("raw is always editable and saves the exact bytes typed", () => {
  it("hands back the characters typed, byte for byte, including the odd ones", () => {
    const saved: string[] = [];
    // No CRLF here: a `<textarea>` normalises `\r\n` to `\n` before React ever
    // sees it, which is a fact about the DOM element standing in for 45.6's
    // editor and not about this component. CRLF fidelity is asserted where the
    // real CodeMirror is, in 45.6's suite.
    const typed = '"quoted, field",2\n\tcafé \\ "\n';
    render(
      <Host
        format="csv"
        rendered="table"
        language="csv"
        cookie={jar(`${VIEW_MODE_COOKIE}=csv%3Araw`)}
        fileName="rows.csv"
        initial=""
        onSaved={(text) => saved.push(text)}
      />,
    );

    const editor = screen.getByLabelText("Source of rows.csv");
    fireEvent.change(editor, { target: { value: typed } });
    fireEvent.keyDown(editor, { key: "s", metaKey: true });

    expect(saved).toEqual([typed]);
  });

  it("passes the registry's language and Rust's size label down untouched", () => {
    render(
      <Host
        format="json"
        rendered="structure"
        language="json"
        cookie={jar(`${VIEW_MODE_COOKIE}=json%3Araw`)}
        fileName="big.json"
        initial="{}"
        sizeLabel="4.2 MB"
      />,
    );
    const editor = screen.getByLabelText("Source of big.json");
    // Never derived from the name here: a second extension table in a viewer is
    // the defect 45.2 exists to prevent.
    expect(editor).toHaveAttribute("data-language", "json");
    expect(editor).toHaveAttribute("data-size-label", "4.2 MB");
  });

  it("says why writing is refused instead of quietly disabling the editor", () => {
    render(
      <Host
        format="plain"
        rendered={null}
        language="plain"
        fileName="readme.txt"
        initial="x"
        readOnly
        readOnlyReason="this file is not inside a sync profile, so keeper cannot write it"
      />,
    );
    expect(screen.getByRole("status")).toHaveTextContent("not inside a sync profile");
    expect(screen.getByLabelText("Source of readme.txt")).toHaveAttribute("readonly");
  });
});

describe("a malformed file names the error and the line, and stays editable", () => {
  it("shows the sentence and the line, falls back to source, and lets you fix it", () => {
    const cookie = jar();
    render(
      <Host
        format="json"
        rendered="structure"
        language="json"
        cookie={cookie}
        fileName="broken.json"
        initial={'{\n  "a": 1,\n  "b": oops\n}\n'}
      />,
    );

    const alert = screen.getByRole("alert");
    expect(alert).toHaveTextContent("line 3, column 8");
    expect(alert).toHaveTextContent("a value was expected here");
    // Editable, not a dead pane: AD-88's whole point is that raw can always save.
    const editor = screen.getByLabelText("Source of broken.json");
    expect(editor).not.toHaveAttribute("readonly");

    // And the reader's stated preference is NOT overwritten by a broken file:
    // the tab still shows Structure selected, and the jar was never written.
    expect(screen.getByRole("tab", { name: "Structure" })).toHaveAttribute("aria-selected", "true");
    expect(cookie.read()).toBe("");
  });

  it("returns to the structure the moment the file becomes JSON again", () => {
    render(
      <Host
        format="json"
        rendered="structure"
        language="json"
        cookie={jar()}
        fileName="x.json"
        initial="{"
      />,
    );
    expect(screen.getByRole("alert")).toBeInTheDocument();

    fireEvent.change(screen.getByLabelText("Source of x.json"), {
      target: { value: '{"a":1}' },
    });

    expect(screen.queryByRole("alert")).toBeNull();
    expect(screen.getByText("a")).toBeInTheDocument();
  });

  it("keeps a JSONL file's good records and names only the line that failed", () => {
    render(
      <Host
        format="jsonl"
        rendered="structure"
        language="json"
        cookie={jar()}
        fileName="log.jsonl"
        initial={'{"a":1}\n{"a":\n{"a":3}\n'}
      />,
    );

    // No fallback to source: the records that parsed are whole and true, which
    // is most of why JSON Lines exists.
    expect(screen.queryByLabelText("Source of log.jsonl")).toBeNull();
    expect(screen.getByText(/line 2, column 6/)).toBeInTheDocument();
    expect(screen.getAllByText("a")).toHaveLength(2);
  });

  it("falls back to source for a JSONL file where nothing at all parsed", () => {
    render(
      <Host
        format="jsonl"
        rendered="structure"
        language="json"
        cookie={jar()}
        fileName="all-bad.jsonl"
        initial={"{\n{\n"}
      />,
    );
    expect(screen.getByRole("alert")).toHaveTextContent("line 1");
    expect(screen.getByLabelText("Source of all-bad.jsonl")).toBeInTheDocument();
  });
});

describe("an empty file renders in both views without throwing", () => {
  it.each([
    ["json", "empty.json"],
    ["jsonl", "empty.jsonl"],
  ] as const)("says a %s file is empty rather than calling it broken", (format, name) => {
    render(
      <Host
        format={format}
        rendered="structure"
        language="json"
        cookie={jar()}
        fileName={name}
        initial=""
      />,
    );

    // Not an alert: an empty file is a file, and a parse error would be a lie.
    expect(screen.queryByRole("alert")).toBeNull();
    expect(screen.getByRole("status")).toHaveTextContent("this file is empty");

    fireEvent.click(screen.getByRole("tab", { name: "Source" }));
    expect(screen.getByLabelText(`Source of ${name}`)).toHaveValue("");
  });

  it("renders an empty markdown file in both views", async () => {
    const { container } = render(
      <Host
        format="markdown"
        rendered="markdown"
        language="markdown"
        cookie={jar()}
        fileName="empty.md"
        initial=""
      />,
    );

    await settle();
    expect(container.querySelector(".cm-content")).not.toBeNull();
    expect(screen.queryByRole("alert")).toBeNull();

    fireEvent.click(screen.getByRole("tab", { name: "Source" }));
    expect(screen.getByLabelText("Source of empty.md")).toHaveValue("");
  });

  it("renders an empty CSV through 44.16's own no-rows sentence", async () => {
    render(
      <Host
        format="csv"
        rendered="table"
        language="csv"
        cookie={jar()}
        fileName="empty.csv"
        initial=""
        csv={{ vaultId: "vault-1", target: "empty.csv" }}
        csvOptions={{
          read: async () => ({
            ...CSV_TABLE,
            relPath: "empty.csv",
            columns: 0,
            totalRows: 0,
            rows: [],
          }),
        }}
      />,
    );
    expect(await screen.findByText("empty.csv has no rows")).toBeInTheDocument();
  });
});

describe("markdown renders through the note editor's own preview", () => {
  it("draws the decoration layer's own marks, not a second renderer's HTML", async () => {
    const { container } = render(
      <Host
        format="markdown"
        rendered="markdown"
        language="markdown"
        cookie={jar()}
        fileName="note.md"
        initial={"# Title\n\n*em*\n"}
      />,
    );

    await settle();
    expect(container.querySelector(".cm-lp-h1")).not.toBeNull();
    expect(container.querySelector(".cm-lp-em")?.textContent).toBe("em");
  });

  it("draws a mermaid diagram rather than refusing the document (DW-165 is fixed)", async () => {
    const cookie = jar();
    const { container } = render(
      <Host
        format="markdown"
        rendered="markdown"
        language="markdown"
        cookie={cookie}
        fileName="diagram.md"
        initial={"intro\n\n```mermaid\ngraph TD;\nA-->B;\n```\n"}
      />,
    );

    await settle();
    // Until Story 45.10 this pane showed a refusal naming DW-165, because the
    // renderer threw on constructing a view over this exact document.
    //
    // Asserted as "the pane did not fall back to source", not as "there is no
    // alert on screen": mermaid itself degrades to a `role="alert"` inside its
    // own block under jsdom, which has no `getComputedTextLength` for it to
    // measure with. That degrade is the widget's documented behaviour and is
    // not what this test is about — the pane refusing to draw at all is.
    expect(screen.queryByLabelText("Source of diagram.md")).toBeNull();
    expect(container.querySelector(".cm-mermaid-block")).not.toBeNull();
    expect(screen.getByRole("tab", { name: "Preview" })).toHaveAttribute("aria-selected", "true");
    expect(cookie.read()).toBe("");
  });

  it("names a document it cannot draw and hands over an editable source view", async () => {
    forcedPreviewFailure = "keeper could not draw this document: the renderer threw";
    const cookie = jar();
    render(
      <Host
        format="markdown"
        rendered="markdown"
        language="markdown"
        cookie={cookie}
        fileName="broken.md"
        initial={"# Title\n"}
      />,
    );

    await settle();
    expect(screen.getByRole("alert")).toHaveTextContent("the renderer threw");
    expect(screen.getByLabelText("Source of broken.md")).not.toHaveAttribute("readonly");
    // Still the reader's stated preference; the next markdown file previews.
    expect(screen.getByRole("tab", { name: "Preview" })).toHaveAttribute("aria-selected", "true");
    expect(cookie.read()).toBe("");
  });

  it("previews again the moment the source is edited into something it can draw", async () => {
    forcedPreviewFailure = "keeper could not draw this document: the renderer threw";
    const { container } = render(
      <Host
        format="markdown"
        rendered="markdown"
        language="markdown"
        cookie={jar()}
        fileName="broken.md"
        initial={"# Title\n"}
      />,
    );
    await settle();
    expect(screen.getByRole("alert")).toBeInTheDocument();

    // The reader edits the file. A refusal that outlived the bytes it was
    // about would keep the pane on source forever, and the reader would have no
    // way to tell a file keeper will not draw from a file keeper has given up
    // on — which is the "makes the user think the file changed" failure this
    // story forbids.
    forcedPreviewFailure = null;
    fireEvent.change(screen.getByLabelText("Source of broken.md"), {
      target: { value: "# just a heading\n" },
    });
    await settle();

    expect(screen.queryByRole("alert")).toBeNull();
    expect(container.querySelector(".cm-lp-h1")).not.toBeNull();
  });
});

describe("the CSV table is 44.16's, and an edit does not go round it", () => {
  it("sends a cell as coordinates plus the revision, never as a re-serialised file", async () => {
    const setCell = vi.fn(async () => CSV_TABLE);
    const onExternalWrite = vi.fn();
    render(
      <Host
        format="csv"
        rendered="table"
        language="csv"
        cookie={jar()}
        fileName="rows.csv"
        initial={"name,qty\nwidget,3\n"}
        csv={{ vaultId: "vault-1", target: "rows.csv" }}
        csvOptions={{ read: async () => CSV_TABLE, setCell }}
        onExternalWrite={onExternalWrite}
      />,
    );

    const cell = await screen.findByText("widget");
    fireEvent.click(cell);
    const input = screen.getByLabelText("Edit cell");
    fireEvent.change(input, { target: { value: "gadget" } });
    fireEvent.blur(input);

    await waitFor(() => expect(setCell).toHaveBeenCalledTimes(1));
    // Row, column, value and the revision the table was read at. The untouched
    // rows are not in this call at all, which is what makes the byte-identical
    // round trip `keeper-core::notes::csv` promises possible.
    expect(setCell).toHaveBeenCalledWith("vault-1", "rows.csv", "rev-1", 1, 0, "gadget");
    await waitFor(() => expect(onExternalWrite).toHaveBeenCalledTimes(1));
  });

  it("does not tell the host to re-read when Rust refused the edit", async () => {
    const onExternalWrite = vi.fn();
    render(
      <Host
        format="csv"
        rendered="table"
        language="csv"
        cookie={jar()}
        fileName="rows.csv"
        initial={"name,qty\nwidget,3\n"}
        csv={{ vaultId: "vault-1", target: "rows.csv" }}
        csvOptions={{
          read: async () => CSV_TABLE,
          setCell: async () => {
            throw new Error("rows.csv changed on disk since this table was opened");
          },
        }}
        onExternalWrite={onExternalWrite}
      />,
    );

    fireEvent.click(await screen.findByText("widget"));
    fireEvent.change(screen.getByLabelText("Edit cell"), { target: { value: "gadget" } });
    fireEvent.blur(screen.getByLabelText("Edit cell"));

    // 44.16's own degrade renders Rust's sentence and repaints the last state
    // keeper confirmed. What must NOT happen is the host discarding the
    // reader's buffer for a write that did not land.
    expect(await screen.findByText(/changed on disk/)).toBeInTheDocument();
    expect(onExternalWrite).not.toHaveBeenCalled();
  });

  it("says a CSV outside a notes vault opens as source, rather than drawing nothing", () => {
    render(
      <Host
        format="csv"
        rendered="table"
        language="csv"
        cookie={jar()}
        fileName="rows.csv"
        initial={"a,b\n"}
        csv={null}
      />,
    );
    expect(screen.getByRole("alert")).toHaveTextContent("inside a notes vault");
    expect(screen.getByLabelText("Source of rows.csv")).toHaveValue("a,b\n");
  });
});

describe("the structure view shows what the file says", () => {
  it("shows a number's own characters, not a double's version of them", () => {
    render(
      <Host
        format="json"
        rendered="structure"
        language="json"
        cookie={jar()}
        fileName="ids.json"
        initial='{"id": 12345678901234567890}'
      />,
    );
    expect(screen.getByText("12345678901234567890")).toBeInTheDocument();
  });

  it("marks a repeated key rather than showing only the one that wins", () => {
    render(
      <Host
        format="json"
        rendered="structure"
        language="json"
        cookie={jar()}
        fileName="dup.json"
        initial='{"a":1,"a":2}'
      />,
    );
    expect(screen.getAllByText("a")).toHaveLength(2);
    expect(screen.getByText(/repeated key/)).toBeInTheDocument();
  });

  it("says how much of a very large document it is not drawing", () => {
    const many = `[${Array.from({ length: 5_100 }, (_, at) => at).join(",")}]`;
    render(
      <Host
        format="json"
        rendered="structure"
        language="json"
        cookie={jar()}
        fileName="many.json"
        initial={many}
      />,
    );
    // A truncated view that says nothing reads as a short file.
    expect(screen.getByRole("status")).toHaveTextContent("showing the first 5000 of 5101 values");
  });
});

/**
 * Story 51.5's third view, against the real thing (FR-294).
 *
 * The pane is a REAL `EditorView` carrying the real decoration layer and the
 * real editing keymap, and the host above it is the same one-buffer `Host` every
 * other test in this file uses. That combination is the whole point: what has to
 * hold is that the buffer, the dirty text and the Save are the SAME ones the
 * Source tab has, and a mocked mount could not tell a shared buffer from a
 * second copy of the text.
 */

/** The live view inside the pane, asserted rather than assumed. */
function paneView(container: HTMLElement): EditorView {
  const content = container.querySelector<HTMLElement>(".cm-content");
  expect(content, "the pane mounted no CodeMirror").not.toBeNull();
  const view = EditorView.findFromDOM(content as HTMLElement);
  expect(view, "no EditorView is mounted in that content DOM").not.toBeNull();
  return view as EditorView;
}

/**
 * Type at the caret, one character per transaction.
 *
 * Per character and with the `input.type` user event, because that is how an
 * edit actually arrives and because the shape row 5 is about — a view rebuilt
 * between keystrokes — is invisible to a single bulk dispatch.
 */
async function typeAtCaret(view: EditorView, text: string): Promise<void> {
  for (const character of text) {
    await act(async () => {
      const at = view.state.selection.main.head;
      view.dispatch({
        changes: { from: at, insert: character },
        selection: { anchor: at + character.length },
        userEvent: "input.type",
      });
    });
    await settle();
  }
}

/** The modifier CodeMirror's `Mod-s` resolves to in this environment. A
 *  constant for the reason `text-file-viewer.test.tsx` states: jsdom presents
 *  itself as something other than a Mac, so `Mod` binds to Ctrl and a
 *  Cmd-flagged event would match nothing, assert nothing, and still pass. */
const MOD = { ctrlKey: true };

/** A writable markdown file, in whichever mode the jar asks for. */
function markdownHost(over: Partial<React.ComponentProps<typeof Host>> = {}): React.ReactElement {
  return (
    <Host
      format="markdown"
      rendered="markdown"
      language="markdown"
      noteMode
      cookie={jar()}
      fileName="log.md"
      initial={"# Session\n\nalpha\n"}
      preview={{ vaultId: null }}
      {...over}
    />
  );
}

/**
 * The same file, with the buffer in the TEST's hands rather than `Host`'s.
 *
 * The tests that replace the file a mounted panel is showing need one commit
 * carrying new bytes AND a new identity, and `Host` owns its text in `useState`
 * — a re-render cannot hand it another file's. Which is the shape a panel really
 * has: `panelsStore.setActiveTarget` swaps the target under a `PanelFrame` keyed
 * on the panel, not on the file.
 */
function noteFile(over: {
  content: string;
  loadedFrom: FileOrigin;
  cookie: { read: () => string; write: (value: string) => void };
  fileName?: string;
  preview?: { vaultId: string | null };
  onChange?: (next: string) => void;
}): React.ReactElement {
  return (
    <RawRenderedView
      format="markdown"
      rendered="markdown"
      language="markdown"
      noteMode
      editor={TestEditor}
      fileName={over.fileName ?? "plan.md"}
      preview={over.preview ?? { vaultId: null }}
      content={over.content}
      loadedFrom={over.loadedFrom}
      cookie={over.cookie}
      onChange={over.onChange}
      onSave={() => {}}
    />
  );
}

/** Opened straight into Note mode, which is what a reader who chose it once
 *  gets on every markdown file after. */
async function openNote(
  over: Partial<React.ComponentProps<typeof Host>> = {},
): Promise<{ container: HTMLElement; view: EditorView }> {
  const { container } = render(
    markdownHost({ cookie: jar(`${VIEW_MODE_COOKIE}=markdown%3Anote`), ...over }),
  );
  await settle();
  return { container, view: paneView(container) };
}

describe("Note mode is a third view over one buffer (Story 51.5)", () => {
  it("row 1: offers Preview, Source and Note, and a reader lands on Note", async () => {
    const cookie = jar();
    render(markdownHost({ cookie }));
    await settle();

    // The order is the reading order and is unchanged. Where a reader LANDS is
    // not: story 51.5 wrote "a person opening a file to read it must not land in
    // an editor" and the owner has since asked for the opposite twice (story
    // 52.3, `spec-51-5:62`). Note mode is the same live-preview drawing Preview
    // shows, with a caret in it, so what he lands in still reads as his document.
    expect(screen.getAllByRole("tab").map((tab) => tab.textContent)).toEqual([
      "Preview",
      "Source",
      "Note",
    ]);
    expect(screen.getByRole("tab", { name: "Note" })).toHaveAttribute("aria-selected", "true");
    // The editable pane, not merely the lit tab.
    expect(screen.getByRole("textbox", { name: "Note of log.md" })).toBeInTheDocument();
    // And the jar is untouched: a default is not an answer the reader gave, so
    // nothing was recorded on his behalf.
    expect(cookie.read()).toBe("");
  });

  it("offers no Note tab when the frame did not say the file may be written", async () => {
    // The view half of rows 7–9. WHICH files may be written is the frame's
    // verdict and is asserted against Rust's own refusal in its suite; what this
    // component owes is that it never invents the tab for itself.
    render(markdownHost({ noteMode: undefined }));
    await settle();

    expect(screen.queryByRole("tab", { name: "Note" })).toBeNull();
    expect(screen.getAllByRole("tab")).toHaveLength(2);
    // Story 52.3: and the DEFAULT falls back with the tab. A file that offers no
    // Note mode — read-only, oversize, `workspace/` — still opens in Preview, so
    // the new default cannot leave a reader looking at a view that is not there.
    expect(screen.getByRole("tab", { name: "Preview" })).toHaveAttribute("aria-selected", "true");
  });

  it("row 11: restores Note from the jar, as an editable region with the file's name", async () => {
    const { container } = await openNote();

    expect(screen.getByRole("tab", { name: "Note" })).toHaveAttribute("aria-selected", "true");
    // Editable, and named: CodeMirror gives its content `role="textbox"` and no
    // accessible name, so a pane without the label announces itself as an
    // unlabelled text box.
    expect(screen.getByRole("textbox", { name: "Note of log.md" })).toBeInTheDocument();
    // The note editor's own decorations, so this is that renderer and not a
    // second one that happens to produce similar HTML.
    expect(container.querySelector(".cm-lp-h1")).not.toBeNull();
  });

  it("row 10: a jar written before Note mode existed still resolves", async () => {
    render(markdownHost({ cookie: jar(`${VIEW_MODE_COOKIE}=markdown%3Araw`) }));
    await settle();

    expect(screen.getByRole("tab", { name: "Source" })).toHaveAttribute("aria-selected", "true");
    expect(screen.getByLabelText("Source of log.md")).toHaveValue("# Session\n\nalpha\n");
  });

  it("lights Preview for a `note` jar on a file that offers no Note tab", async () => {
    // Not "lights nothing at all", which is what reading the stored preference
    // verbatim would do — and the jar is left holding `note`, so the next
    // writable markdown file still honours it.
    const cookie = jar(`${VIEW_MODE_COOKIE}=markdown%3Anote`);
    render(markdownHost({ noteMode: undefined, cookie }));
    await settle();

    expect(screen.getByRole("tab", { name: "Preview" })).toHaveAttribute("aria-selected", "true");
    expect(cookie.read()).toBe(`${VIEW_MODE_COOKIE}=markdown%3Anote`);
  });

  it("row 2: renders what is typed and reports it to the buffer the Save writes", async () => {
    const saved: string[] = [];
    const { container, view } = await openNote({ onSaved: (text) => saved.push(text) });

    await act(async () => {
      view.dispatch({ selection: { anchor: view.state.doc.length } });
    });
    await typeAtCaret(view, "## Later\n");
    // Live, through the same decoration layer the Preview tab mounts.
    expect(container.querySelector(".cm-lp-h2")).not.toBeNull();

    // Row 13. The same chord and the same `onSave` the Source tab calls, and it
    // carries the characters the view actually holds.
    fireEvent.keyDown(view.contentDOM, { key: "s", ...MOD });
    await settle();
    expect(saved).toEqual(["# Session\n\nalpha\n## Later\n"]);
  });

  it("row 3: an edit made in Note mode is on the Source tab, unsaved", async () => {
    const saved: string[] = [];
    const { view } = await openNote({ onSaved: (text) => saved.push(text) });

    await act(async () => {
      view.dispatch({ selection: { anchor: view.state.doc.length } });
    });
    await typeAtCaret(view, "beta\n");
    fireEvent.click(screen.getByRole("tab", { name: "Source" }));

    // One buffer: the Source tab is looking at the characters Note mode
    // produced, and nothing has been written.
    expect(screen.getByLabelText("Source of log.md")).toHaveValue("# Session\n\nalpha\nbeta\n");
    expect(saved).toEqual([]);
  });

  it("row 4: an edit made in Source is in Note mode, and the caret does not jump", async () => {
    render(markdownHost({ cookie: jar(`${VIEW_MODE_COOKIE}=markdown%3Araw`) }));
    await settle();
    fireEvent.change(screen.getByLabelText("Source of log.md"), {
      target: { value: "# Session\n\nalpha and more\n" },
    });
    fireEvent.click(screen.getByRole("tab", { name: "Note" }));
    await settle();

    // The unsaved edit crossed the switch: Note mode mounts over the host's
    // buffer, not over the bytes the file was opened with.
    const view = paneView(document.body);
    expect(view.state.doc.toString()).toBe("# Session\n\nalpha and more\n");

    // And the caret stays where a person put it, through the round trip the old
    // `[text]`-keyed effect destroyed: the keystroke is reported upward, the
    // host stores it, the identical string comes back as a prop, and the
    // adoption is a no-op. Re-queried rather than read off the handle above,
    // because a rebuilt view leaves that handle holding a destroyed one whose
    // state still reads correctly — which is a test that cannot fail.
    const at = view.state.doc.toString().indexOf("alpha") + "alpha".length;
    await act(async () => {
      view.dispatch({ selection: { anchor: at } });
    });
    await typeAtCaret(view, "!");

    const live = paneView(document.body);
    expect(live).toBe(view);
    expect(live.state.doc.toString()).toBe("# Session\n\nalpha! and more\n");
    expect(live.state.selection.main.head).toBe(at + 1);
  });

  it("row 5: ten keystrokes leave one view standing, with its undo history", async () => {
    const { container, view } = await openNote();

    await act(async () => {
      view.dispatch({ selection: { anchor: view.state.doc.length } });
    });
    await typeAtCaret(view, "0123456789");
    expect(view.state.doc.toString()).toBe("# Session\n\nalpha\n0123456789");

    // The same view object, not a tenth rebuild of it. This is the assertion
    // that fails against a `[text]`-keyed mount effect: an editable pane reports
    // every keystroke upward and gets the identical string straight back, so a
    // text key would tear the view down on every character.
    expect(paneView(container)).toBe(view);
    // And the history is intact, which is what a reader actually notices. A
    // rebuilt view has an empty one, so `undo` would change nothing.
    expect(undoDepth(view.state)).toBeGreaterThan(0);
    await act(async () => {
      expect(undo(view)).toBe(true);
    });
    expect(view.state.doc.toString()).not.toContain("0123456789");
  });

  it("row 6: Preview is still read-only, so typing in it changes nothing", async () => {
    const { container, view } = await openNote({
      cookie: jar(`${VIEW_MODE_COOKIE}=markdown%3Arendered`),
    });

    // Both halves of the clamp, because one is not enough: `editable` stops
    // typing and `readOnly` is what stops Enter, Backspace, cut and paste,
    // which arrive as commands rather than as input.
    expect(container.querySelector('[contenteditable="true"]')).toBeNull();
    expect(view.state.readOnly).toBe(true);

    fireEvent.keyDown(view.contentDOM, { key: "x" });
    expect(view.state.doc.toString()).toBe("# Session\n\nalpha\n");
  });

  it("row 12: outside a vault an embed degrades to its link rather than crashing", async () => {
    const { container, view } = await openNote({ initial: "see ![[notes/other]]\n" });

    // 50.3's measured degrade, unchanged by the mode: the decoration layer
    // renders the wikilink with its target, because there is no vault to
    // resolve it against.
    expect(view.state.doc.toString()).toBe("see ![[notes/other]]\n");
    expect(container.textContent).toContain("notes/other");
    expect(screen.queryByRole("alert")).toBeNull();
  });

  it("keeps a CRLF file's own line endings, so an untouched save is not a whole diff", async () => {
    const { view } = await openNote({ initial: "# Session\r\nalpha\r\n" });

    // The `lineSeparator` facet, asserted where it matters: this buffer is one
    // a save writes, and a document that hands back "\n" for every line would
    // rewrite every line of the file on the first Save.
    expect(view.state.doc.toString()).toBe("# Session\r\nalpha\r\n");
    expect(view.state.doc.lines).toBe(3);
  });

  it("adopts the vault the file turns out to be in, when the list arrives late", async () => {
    // The panel mounts before the vault mirror is hydrated — `text-file-viewer`
    // says so in as many words ("`null` while the mirror is unread … the table
    // appears when the list arrives, which is one frame"). What must not happen
    // is what the fix to this story found: nothing rebuilt the pane, so a file
    // that IS in a vault kept the out-of-vault degrade and resolved every
    // wikilink against `""` for the life of the panel.
    const cookie = jar(`${VIEW_MODE_COOKIE}=markdown%3Anote`);
    const loadedFrom: FileOrigin = { profileOrVaultId: "p1", relativePath: "log/plan.md" };
    const { rerender } = render(
      noteFile({ cookie, loadedFrom, content: "# Session\n", preview: { vaultId: null } }),
    );
    await settle();
    const first = paneView(document.body);
    expect(mountedVaults).toEqual([null]);

    rerender(
      noteFile({ cookie, loadedFrom, content: "# Session\n", preview: { vaultId: "vault-7" } }),
    );
    await settle();

    // Rebuilt, and rebuilt WITH the vault that arrived: `livePreview` reads it
    // once, at construction, so nothing else can carry it into a live view.
    expect(mountedVaults).toEqual([null, "vault-7"]);
    expect(paneView(document.body)).not.toBe(first);

    // And the buffer still does not rebuild it, which is the property the options
    // key must not cost: one rebuild per keystroke is the defect the `[text]` key
    // was removed for.
    const live = paneView(document.body);
    await act(async () => {
      live.dispatch({ selection: { anchor: live.state.doc.length } });
    });
    await typeAtCaret(live, "x");
    expect(mountedVaults).toEqual([null, "vault-7"]);
    expect(paneView(document.body)).toBe(live);
  });

  it("gives a second file of the same name its own view and its own undo history", async () => {
    // Story 51.1 made two markdown files with one basename in two directories an
    // ordinary session layout, and a panel replaces its target in place. Keyed on
    // the display name — which is what this pane used to be — both files are one
    // view: one undo restores the OTHER file's text, `onChange` reports it, and
    // the next Save writes it here.
    const cookie = jar(`${VIEW_MODE_COOKIE}=markdown%3Anote`);
    const onChange = vi.fn();
    const { rerender } = render(
      noteFile({
        cookie,
        onChange,
        content: "the log's plan\n",
        loadedFrom: { profileOrVaultId: "p1", relativePath: "log/plan.md" },
      }),
    );
    await settle();
    const first = paneView(document.body);
    await act(async () => {
      first.dispatch({
        changes: { from: first.state.doc.length, insert: "typed here\n" },
        userEvent: "input.type",
      });
    });
    expect(undoDepth(first.state)).toBeGreaterThan(0);

    rerender(
      noteFile({
        cookie,
        onChange,
        content: "the root plan\n",
        loadedFrom: { profileOrVaultId: "p1", relativePath: "plan.md" },
      }),
    );
    await settle();

    const second = paneView(document.body);
    expect(second).not.toBe(first);
    expect(second.state.doc.toString()).toBe("the root plan\n");
    // The history does not reach back into the other file, which is the half a
    // reader would lose a file to.
    expect(undoDepth(second.state)).toBe(0);
    await act(async () => {
      expect(undo(second)).toBe(false);
    });
    expect(second.state.doc.toString()).toBe("the root plan\n");
  });

  it("adopts an outside change without reporting it, so the file does not go dirty", async () => {
    const cookie = jar(`${VIEW_MODE_COOKIE}=markdown%3Anote`);
    const loadedFrom: FileOrigin = { profileOrVaultId: "p1", relativePath: "log/plan.md" };
    const onChange = vi.fn();
    const { rerender } = render(noteFile({ cookie, loadedFrom, onChange, content: "alpha\n" }));
    await settle();
    const view = paneView(document.body);

    // Somebody else wrote the file and the host re-read it: the same file, new
    // bytes. Note mode handles it exactly as Source does — one minimal dispatch
    // into the live view (`TextEditorMount.setContent`), not a rebuild — so the
    // caret moves with a real outside change in both, and the no-op that protects
    // it when the text has NOT moved is asserted in `markdown-preview.test.ts`.
    rerender(noteFile({ cookie, loadedFrom, onChange, content: "alpha and theirs\n" }));
    await settle();

    expect(paneView(document.body)).toBe(view);
    expect(view.state.doc.toString()).toBe("alpha and theirs\n");
    // Never reported back: the loader's `dirty` is `content !== persisted`, so a
    // report here would mark a file dirty for bytes that came off its own disk
    // and then offer to save them back over a newer write.
    expect(onChange).not.toHaveBeenCalled();
  });

  it("shows the source, out loud, when the pane refuses a change it is handed", async () => {
    const cookie = jar(`${VIEW_MODE_COOKIE}=markdown%3Anote`);
    const loadedFrom: FileOrigin = { profileOrVaultId: "p1", relativePath: "log/plan.md" };
    const { rerender } = render(noteFile({ cookie, loadedFrom, content: "alpha\n" }));
    await settle();

    // The other half of the refusal lifecycle, and the half that had no test: a
    // throw inside the adoption's dispatch. It used to leave the module — through
    // an effect with no `try` around it — and take the panel down.
    forcedAdoptFailure = "keeper could not draw this change: a field refused this update";
    rerender(noteFile({ cookie, loadedFrom, content: "beta\n" }));
    await settle();

    expect(screen.getByRole("alert")).toHaveTextContent("a field refused this update");
    // The source, holding the new bytes: the one view that is always editable
    // (AD-88), which is where a refusal has always sent the reader.
    expect(screen.getByLabelText("Source of plan.md")).toHaveValue("beta\n");
    expect(document.querySelector(".cm-content")).toBeNull();
  });

  it("leaves nothing behind when it is unmounted mid-mount", async () => {
    const { unmount, container } = render(
      markdownHost({ cookie: jar(`${VIEW_MODE_COOKIE}=markdown%3Anote`) }),
    );

    // No `settle()` first, deliberately: the mount is several awaits deep, and
    // this is the window where the effect's `disposed` flag is the only thing
    // between a resolved mount and an EditorView parented to a detached node,
    // holding a keymap and a measure loop nothing will ever tear down.
    unmount();
    await settle();

    // The pane that landed after the unmount was destroyed rather than merely
    // detached. `querySelector` cannot say this: the host it was parented into
    // left the document with the component, so a leaked view is invisible to
    // every DOM assertion and still live.
    expect(panes.built).toBe(1);
    expect(panes.destroyed).toBe(1);
    expect(container.querySelector(".cm-content")).toBeNull();
    expect(document.querySelector(".cm-content")).toBeNull();
  });

  it("leaves exactly one live view after two mode switches in a row", async () => {
    const cookie = jar(`${VIEW_MODE_COOKIE}=markdown%3Anote`);
    const { container } = render(markdownHost({ cookie }));
    await settle();

    // Faster than the mount wave: three commits, three async mounts in flight,
    // and two of them resolving into a host their effect has already cleaned up.
    fireEvent.click(screen.getByRole("tab", { name: "Preview" }));
    fireEvent.click(screen.getByRole("tab", { name: "Note" }));
    fireEvent.click(screen.getByRole("tab", { name: "Preview" }));
    await settle();

    expect(container.querySelectorAll(".cm-content")).toHaveLength(1);
    // And it is the view the last press asked for, not whichever wave landed
    // last: the read-only one.
    expect(paneView(container).state.readOnly).toBe(true);
    // Every earlier wave was torn down rather than left holding a view on a
    // detached host: one pane per commit, and all but the live one destroyed by
    // their own effect's cleanup.
    expect(panes.built).toBeGreaterThan(1);
    expect(panes.destroyed).toBe(panes.built - 1);
  });
});

/**
 * Note mode is a place you can write (Story 52.3, FR-303/304/305).
 *
 * Everything here is the real thing: the real writing tools in a real
 * `EditorView`, the real `FormatToolbar`, and the same one-buffer `Host` the
 * tests above use. A mock of the tools would prove the pane asked for them and
 * nothing about whether a press, a `/` or a `:tada:` reaches the buffer a save
 * writes — which is the whole of what was missing.
 */
describe("Note mode has the writing tools (Story 52.3)", () => {
  it("row 1: renders the toolbar, and a press changes the document", async () => {
    const { view } = await openNote();

    // The control the Notes surface and the Source tab both have, in the pane
    // that had none until this story.
    const bold = await screen.findByRole("button", { name: "Bold" });

    const at = view.state.doc.toString().indexOf("alpha");
    await act(async () => {
      view.dispatch({ selection: { anchor: at, head: at + "alpha".length } });
    });
    fireEvent.click(bold);
    await settle();

    // The characters, in the buffer a save writes — not a class on a span. A
    // toolbar that decorated the view without editing the document would pass
    // every assertion about its own presence.
    expect(view.state.doc.toString()).toBe("# Session\n\n**alpha**\n");
  });

  it("draws no toolbar over Preview, which nothing can type into (AD-88)", async () => {
    const { view } = await openNote({ cookie: jar(`${VIEW_MODE_COOKIE}=markdown%3Arendered`) });

    // The pair, and both halves matter: a toolbar over a read-only pane is a
    // control that announces its own refusal, and the extensions behind it are
    // absent rather than present-and-inert.
    expect(screen.queryByRole("button", { name: "Bold" })).toBeNull();
    expect(view.state.readOnly).toBe(true);
    expect(startCompletion(view)).toBe(false);
  });

  it("row 2: opens the slash menu on `/`, from the shared source", async () => {
    const { view } = await openNote();

    await act(async () => {
      view.dispatch({ selection: { anchor: view.state.doc.length } });
    });
    await typeAtCaret(view, "/");
    await vi.waitFor(() => expect(completionStatus(view.state)).toBe("active"));

    // The product's whole slash vocabulary — not a menu this pane grew for
    // itself. Sorted on both sides: with no query typed yet CodeMirror orders the
    // options itself, and the ORDER is `slash-menu.ts`'s own business rather than
    // something this surface promises.
    expect(
      currentCompletions(view.state)
        .map((option) => option.label)
        .sort(),
    ).toEqual(SLASH_COMMANDS.map((command) => command.label).sort());
  });

  it("row 2: completes an emoji shortcode, and commits one typed in full", async () => {
    const { view } = await openNote({ initial: "# Session\n\n" });

    await act(async () => {
      view.dispatch({ selection: { anchor: view.state.doc.length } });
    });
    await typeAtCaret(view, ":sm");
    await vi.waitFor(() => expect(completionStatus(view.state)).toBe("active"));

    // Keeper's own matcher's answer, in its order: the source hands narrowing to
    // `matchEmoji` (`filter: false`), so what the menu offers IS that answer.
    const matches = matchEmoji("sm");
    expect(matches.length).toBeGreaterThan(0);
    expect(currentCompletions(view.state).map((option) => option.label)).toEqual(
      matches.map((hit) => hit.shortcode),
    );

    // The other half of Story 45.11, which travels with the menu: a shortcode
    // typed straight through becomes its character.
    await typeAtCaret(view, "\n:tada:");
    expect(view.state.doc.toString()).toBe("# Session\n\n:sm\n🎉");
  });
});

/** A file whose first bytes are a properties block, and the body under it. */
const BLOCK = "---\ntitle: Weekly\ntags:\n  - about\n---\n";
const BODY = "# Weekly\n\nalpha\n";
/** The byte-order mark Excel and Notepad leave in front of a file. Rust leaves
 *  it out of the block it answers with — `file_properties::block_of` — so the
 *  buffer carries a byte the form's block does not. */
const MARK = "\u{feff}";

describe("the frontmatter block is drawn once (Story 52.3)", () => {
  it("row 4: keeps the block out of the Note pane when the form is holding it", async () => {
    const { container, view } = await openNote({
      initial: BLOCK + BODY,
      frontmatterInForm: BLOCK,
    });

    // The document the pane holds is the body, exactly — not the body plus a
    // stray blank line where the block's closing fence used to be.
    expect(view.state.doc.toString()).toBe(BODY);
    // And nothing of the block is on screen as text, which is what the owner saw
    // twice: once as the form above, once as `---` lines in his document.
    expect(container.textContent).not.toContain("title: Weekly");
    expect(container.textContent).not.toContain("---");
  });

  it("row 4: draws it as text for a host that has no form, which is unchanged", async () => {
    // The note embed and any other host that passes no properties address. The
    // block IS the document there, and hiding it would be hiding bytes nothing
    // else on screen accounts for.
    const { view } = await openNote({ initial: BLOCK + BODY });

    expect(view.state.doc.toString()).toBe(BLOCK + BODY);
  });

  it("draws it as text while the form's read is still out, and if it refused", async () => {
    // `null` is both of those states — `FileProperties` reports it at the start of
    // every read and again when the read rejects — and it is the state a file on a
    // pendrive is in for the first hundreds of milliseconds. Hiding on "a form was
    // mounted" hid the block from the FIRST frame, so those bytes were in neither
    // the form nor the text, and permanently so for a read that refused.
    const { view } = await openNote({ initial: BLOCK + BODY, frontmatterInForm: null });

    expect(view.state.doc.toString()).toBe(BLOCK + BODY);
  });

  it("draws it as text when the form's block is not what the buffer begins with", async () => {
    // The disagreeing case: the form is holding the block that was on disk and the
    // buffer's first bytes are something else. Hiding a LENGTH here — or hiding
    // whatever a second parser thought looked like a block — is how bytes nothing
    // on screen accounts for disappear.
    const { view } = await openNote({
      initial: `---\ntitle: Renamed\n---\n${BODY}`,
      frontmatterInForm: BLOCK,
    });

    expect(view.state.doc.toString()).toBe(`---\ntitle: Renamed\n---\n${BODY}`);
  });

  it("hides the byte-order mark with the block, because Rust left it out", async () => {
    // Excel's marker. Rust skips it and answers with the block alone, and
    // `readFrontmatter` wants `---` at byte zero — so while this seam re-parsed
    // the buffer, the two disagreed and the block was drawn in the form AND in the
    // pane, which is FR-304 unmet on exactly these files.
    const saved: string[] = [];
    const { view } = await openNote({
      initial: MARK + BLOCK + BODY,
      frontmatterInForm: BLOCK,
      onSaved: (text) => saved.push(text),
    });

    expect(view.state.doc.toString()).toBe(BODY);

    // And the mark comes back with the block, in front of the edit: it belongs to
    // the file, and a save that dropped it would be this pane rewriting a byte
    // nobody asked it to touch.
    await act(async () => {
      view.dispatch({ selection: { anchor: view.state.doc.length } });
    });
    await typeAtCaret(view, "beta\n");
    fireEvent.keyDown(view.contentDOM, { key: "s", ...MOD });
    await settle();

    expect(saved).toEqual([`${MARK}${BLOCK}# Weekly\n\nalpha\nbeta\n`]);
  });

  it("keeps a leading thematic break on screen, because the form draws none of it", async () => {
    // `---` ⏎ `# Heading` ⏎ `---` is a document whose first line is a thematic
    // break, and Rust calls it frontmatter. The FORM shows a tag row and not one
    // character of the heading, so hiding the span left the reader's own prose on
    // no screen but the Source tab.
    const brokenUp = `---\n# Heading\n---\n${BODY}`;
    const { container, view } = await openNote({
      initial: brokenUp,
      frontmatterInForm: `---\n# Heading\n---\n`,
    });

    expect(view.state.doc.toString()).toBe(brokenUp);
    expect(container.textContent).toContain("Heading");
  });

  it("hides a block the form cannot parse, which it draws verbatim instead", async () => {
    // `PropertiesPanel`'s unparsed arm renders the block exactly as it is on disk,
    // so those characters ARE on screen above the pane — the one thing hiding
    // depends on. A rule that only counted `key: value` rows would draw this one
    // twice.
    const odd = "---\n!anchored\n---\n";
    const { view } = await openNote({ initial: odd + BODY, frontmatterInForm: odd });

    expect(view.state.doc.toString()).toBe(BODY);
  });

  it("does not shorten the shown text under the caret when a block is typed", async () => {
    // Story 52.3's own defect, and the reason this seam is the form's block rather
    // than a parse of the buffer: with the buffer as the source, the moment a
    // reader typed the closing `---` his first three lines vanished from the pane
    // he was typing in. The form is holding no block for an unblocked file, so
    // there is nothing to hide however the document grows.
    const { view } = await openNote({ initial: "", frontmatterInForm: "" });

    await typeAtCaret(view, "---\n---\nhi");

    expect(view.state.doc.toString()).toBe("---\n---\nhi");
  });

  it("row 4: the Source tab still shows every byte of the file", async () => {
    render(
      markdownHost({
        cookie: jar(`${VIEW_MODE_COOKIE}=markdown%3Araw`),
        initial: BLOCK + BODY,
        frontmatterInForm: BLOCK,
      }),
    );
    await settle();

    // The one view that is always the file's characters (AD-88). Hiding anything
    // here would be a lie about what a save writes.
    expect(screen.getByLabelText("Source of log.md")).toHaveValue(BLOCK + BODY);
  });

  it("row 5: a save from Note mode writes the whole file, block included", async () => {
    const saved: string[] = [];
    const { view } = await openNote({
      initial: BLOCK + BODY,
      frontmatterInForm: BLOCK,
      onSaved: (text) => saved.push(text),
    });

    await act(async () => {
      view.dispatch({ selection: { anchor: view.state.doc.length } });
    });
    await typeAtCaret(view, "beta\n");
    fireEvent.keyDown(view.contentDOM, { key: "s", ...MOD });
    await settle();

    // Byte for byte, and this is the assertion the hiding stands or falls on: the
    // reader was shown the body and the file keeps its properties. A save that
    // wrote what the pane was holding would silently delete the block — and the
    // form above it would then be writing into a file that no longer has one.
    expect(saved).toEqual([`${BLOCK}# Weekly\n\nalpha\nbeta\n`]);
  });
});

describe("the default view is Note when Note is possible (Story 52.3)", () => {
  it("row 7: a remembered `rendered` choice still opens Preview", async () => {
    // The promise that makes reversing the default safe. Nothing the reader has
    // already clicked changes under him, and the jar is left exactly as he left
    // it — a default is not an answer, so it never writes one.
    const cookie = jar(`${VIEW_MODE_COOKIE}=markdown%3Arendered`);
    render(markdownHost({ cookie }));
    await settle();

    expect(screen.getByRole("tab", { name: "Preview" })).toHaveAttribute("aria-selected", "true");
    expect(cookie.read()).toBe(`${VIEW_MODE_COOKIE}=markdown%3Arendered`);
  });

  it("row 8: a read-only markdown file opens Preview and offers no Note tab", async () => {
    // The frame's verdict for an oversize file, a `workspace/` one, or a format
    // keeper will not rewrite, arriving here as `readOnly` — the belt beside
    // `noteMode`'s braces. A default that ignored it would light a tab that is
    // not there.
    render(markdownHost({ readOnly: true, readOnlyReason: "keeper will not write this file" }));
    await settle();

    expect(screen.queryByRole("tab", { name: "Note" })).toBeNull();
    expect(screen.getByRole("tab", { name: "Preview" })).toHaveAttribute("aria-selected", "true");
    // And no toolbar anywhere on the surface: there is no editable pane to have
    // one, in either of this file's two views.
    expect(screen.queryByRole("button", { name: "Bold" })).toBeNull();
  });
});
