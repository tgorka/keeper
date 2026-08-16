/**
 * The writing tools moved, and nothing was left behind (Story 50.3, row 7).
 *
 * **Why a guard test and not only a behaviour test.** What Story 50.3 promises
 * is not "a session log has a slash menu" — that is asserted where it can be
 * seen, in `text-file-viewer.test.tsx` over a real editor, and for a note in the
 * note editor's own suite, which this story did not touch. What it promises
 * *additionally* is that there is exactly ONE of each tool in the repository.
 * That claim has no runtime symptom: two `FormatAction` lists, or a second
 * `autocompletion()` over markdown, would pass every behaviour test in this
 * project on the day they were written and then drift apart quietly — which is
 * the failure `text-editor-host.ts`'s module comment was written to prevent, and
 * the reason the tools moved rather than being copied.
 *
 * So the scan below reads the source of `src/` and asserts the shape of the
 * graph: one definition, one wiring, two importers reaching it rather than
 * around it.
 *
 * Comments are stripped before scanning, deliberately. A paragraph that names
 * the slash menu is documentation and not a definition, and a guard that fails
 * on prose is a guard people learn to delete.
 */
import { readdirSync, readFileSync, statSync } from "node:fs";
import { join, relative, resolve } from "node:path";
import { completionStatus, currentCompletions } from "@codemirror/autocomplete";
import { EditorState } from "@codemirror/state";
import { EditorView } from "@codemirror/view";
import { afterAll, beforeAll, describe, expect, it, vi } from "vitest";
import { withRangeRects } from "@/test/layout";
import { markdownWritingTools } from "./writing-tools";

const SRC = resolve(import.meta.dirname, "../../..");

/** Every `.ts`/`.tsx` file under `src/`, so a second copy cannot hide in a
 *  corner of the tree nobody scans. */
function sources(dir: string): string[] {
  return readdirSync(dir).flatMap((entry) => {
    const path = join(dir, entry);
    if (statSync(path).isDirectory()) {
      return sources(path);
    }
    return path.endsWith(".ts") || path.endsWith(".tsx") ? [path] : [];
  });
}

/**
 * Every shipped source file, as code with its comments removed.
 *
 * Test files are excluded because a test that imports a definition to assert
 * about it is not a second definition — `slash-menu.test.ts` drives the real
 * source through a real view, which is the point of it. This file is excluded
 * for the reason `file-controlled-keys.test.ts` excludes itself: the symbol
 * names below are string literals in it, so the scanner would otherwise report
 * its own source as the second copy.
 */
function withoutComments(path: string): string {
  const source = readFileSync(path, "utf8");
  return source.replace(/\/\/.*$/gm, "").replace(/\/\*[\s\S]*?\*\//g, "");
}

const CODE: ReadonlyArray<{ path: string; code: string }> = sources(SRC)
  .filter(
    (path) =>
      !path.endsWith(".test.ts") && !path.endsWith(".test.tsx") && path !== import.meta.filename,
  )
  .map((path) => ({ path: relative(SRC, path), code: withoutComments(path) }));

function filesMatching(pattern: RegExp): string[] {
  const hits = CODE.filter(({ code }) => pattern.test(code));
  return hits.map(({ path }) => path).sort();
}

const SHARED = "components/notes/editor/writing-tools.ts";
const NOTE_EDITOR = "components/notes/note-editor.tsx";
const FILE_HOST = "components/viewers/text-editor-host.ts";
const SLASH_MENU = "components/notes/editor/slash-menu.ts";
const EMOJI = "components/notes/editor/emoji-complete.ts";
const FORMAT_COMMANDS = "components/notes/editor/format-commands.ts";

/**
 * The tools this story moved, with the module that defines each and the
 * declaration that would have to be duplicated for a second copy to exist.
 *
 * The definitions did not change file — each was already its own module — so
 * what moved is the WIRING, and that is what the second matrix below is about:
 * after 50.3 the only code naming these symbols is the module that defines one
 * and the module that mounts them all.
 */
const TOOLS: ReadonlyArray<[string, string, RegExp]> = [
  ["slashMenuSource", SLASH_MENU, /export function slashMenuSource\b/],
  ["emojiCompleteSource", EMOJI, /export function emojiCompleteSource\b/],
  ["emojiShortcodeCommit", EMOJI, /export function emojiShortcodeCommit\b/],
  ["formatCommand", FORMAT_COMMANDS, /export function formatCommand\b/],
];

/**
 * What a failure here should tell the next contributor.
 *
 * A scan like this is an instrument for drift, and the day it fires is usually
 * the day somebody added a THIRD editor surface for an honest reason — not the
 * day they did something wrong. So every assertion below carries the
 * instruction, and the instruction is always the same shape: extend
 * `writing-tools.ts` and mount it, rather than restate one of these tools where
 * you are standing. Deleting the assertion is the one answer that reintroduces
 * the defect the story was written to remove.
 */
const EXTEND_INSTEAD = `Do not restate this in a new surface. Add what you need to ${SHARED} — it already takes the caller's own completion sources as an argument — and mount that module instead. If a third surface genuinely needs a different set, widen the shared module's contract and add the new path to this test's expectations, deliberately.`;

describe("one definition of each writing tool", () => {
  it.each(TOOLS)("%s is declared once, in its own module", (symbol, definedIn, declaration) => {
    expect(
      filesMatching(declaration),
      `${symbol} is declared in more than one place, or has moved. ${EXTEND_INSTEAD}`,
    ).toEqual([definedIn]);
  });

  it.each(TOOLS)("%s is named only by that module and the shared one", (symbol, definedIn) => {
    // The whole of row 7. A surface that grew its own call to one of these — the
    // copy this story exists to prevent — appears here as a third path.
    expect(
      filesMatching(new RegExp(`\\b${symbol}\\b`)),
      `a third module now calls ${symbol} directly. ${EXTEND_INSTEAD}`,
    ).toEqual([definedIn, SHARED].sort());
  });

  it("declares the toolbar's action vocabulary once", () => {
    // `FormatAction` is DATA and travels: the toolbar speaks it, both surfaces
    // pass it, and the file host names it in its mount contract. That is one
    // vocabulary being used, which is the opposite of the failure — so what is
    // asserted is the declaration, not the mentions.
    expect(
      filesMatching(/export type FormatAction\b/),
      `there is a second FormatAction union. The toolbar speaks one vocabulary; a second one is two toolbars that agree until they do not. Import the type from ${FORMAT_COMMANDS} and add a variant there if one is missing.`,
    ).toEqual([FORMAT_COMMANDS]);
  });

  it("configures completion over markdown in exactly one place", () => {
    // The wiring, not the sources. Two `autocompletion()` calls is how one
    // surface comes to offer four sources and the other three, with nothing on
    // screen to say which one you are looking at.
    expect(
      filesMatching(/\bautocompletion\(/),
      `a second surface configures completion of its own. ${EXTEND_INSTEAD} Two calls is how one editor comes to offer four sources and another three, with nothing on screen to say which one you are looking at.`,
    ).toEqual([SHARED]);
  });

  it("renders one toolbar component, on both surfaces", () => {
    expect(
      filesMatching(/export function FormatToolbar\b/),
      "there is a second format toolbar component. Reuse @/components/notes/format-toolbar — it speaks plain FormatAction data and holds no editor, which is what lets both surfaces mount it.",
    ).toEqual(["components/notes/format-toolbar.tsx"]);
    expect(
      filesMatching(/<FormatToolbar\b/),
      "the set of surfaces mounting the toolbar has changed. That is allowed — a new one is welcome — but it has to be added here on purpose, so the next reader knows how many editors this control is expected to serve.",
    ).toEqual([NOTE_EDITOR, "components/viewers/text-viewer.tsx"].sort());
  });
});

describe("both surfaces reach the tools through the shared module", () => {
  it("is imported by the note editor and the file editor host, and by nobody else", () => {
    expect(
      filesMatching(/writing-tools"/),
      "the set of surfaces mounting the writing tools has changed. Add the new one here deliberately; the number of editors keeper has is a fact worth writing down.",
    ).toEqual([FILE_HOST, NOTE_EDITOR].sort());
  });

  it("is imported lazily by both, so a pane that draws a file row pays nothing", () => {
    // A static edge from either surface would pull the emoji table and the
    // completion package into the main bundle — the boundary NFR-27 stands on,
    // and the reason both callers name this module inside an `import()`.
    const note = CODE.find(({ path }) => path === NOTE_EDITOR)?.code ?? "";
    const host = CODE.find(({ path }) => path === FILE_HOST)?.code ?? "";
    const lazily =
      "reach this module through a dynamic import() inside the closure that owns the view. A static import pulls ~45 KB of emoji table and the completion package into the main bundle, which is the boundary quick capture's 300 ms budget stands on.";
    expect(note, `the note editor must ${lazily}`).toContain('import("./editor/writing-tools")');
    expect(host, `the file editor host must ${lazily}`).toContain(
      'import("../notes/editor/writing-tools")',
    );
  });
});

// jsdom has no `Range.getClientRects`, so CodeMirror's measure pass throws on
// any animation frame that elapses during a test. `withRangeRects` hands back a
// real rect; the undo is mandatory because `Range.prototype` is shared.
let restoreRects: (() => void) | null = null;

beforeAll(() => {
  restoreRects = withRangeRects();
});

afterAll(() => {
  restoreRects?.();
  restoreRects = null;
});

describe("a caller's own completion sources", () => {
  it("are offered alongside the shared ones rather than replaced by them", async () => {
    // The one thing about this composition no other suite can see. The note
    // editor's wikilink and tag sources arrive as `vaultSources`; a version of
    // this function that dropped them would leave the slash menu and emoji
    // working perfectly and silently delete completion of `[[` and `#` from the
    // product. A stand-in source rather than the real wikilink one, because what
    // is being proved is the plumbing and not the vault.
    const parent = document.createElement("div");
    document.body.append(parent);
    const view = new EditorView({
      parent,
      state: EditorState.create({
        doc: "",
        extensions: [
          markdownWritingTools([
            (context) => {
              const opened = context.matchBefore(/@\w*/);
              if (opened === null) {
                return null;
              }
              return { from: opened.from, options: [{ label: "@alpha" }] };
            },
          ]),
        ],
      }),
    });

    view.dispatch({
      changes: { from: 0, insert: "@al" },
      selection: { anchor: 3 },
      userEvent: "input.type",
    });
    await vi.waitFor(() => expect(completionStatus(view.state)).toBe("active"));

    expect(currentCompletions(view.state).map((option) => option.label)).toEqual(["@alpha"]);

    view.destroy();
    parent.remove();
  });
});
