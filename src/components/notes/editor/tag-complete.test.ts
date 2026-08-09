import {
  autocompletion,
  type CompletionContext,
  type CompletionResult,
  currentCompletions,
} from "@codemirror/autocomplete";
import { EditorState } from "@codemirror/state";
import { EditorView } from "@codemirror/view";
import { describe, expect, it, vi } from "vitest";
import { tagCompleteSource, tagPaths } from "@/components/notes/editor/tag-complete";
import type { NoteTagNodeVm } from "@/lib/ipc/client";

function node(p: Partial<NoteTagNodeVm> & Pick<NoteTagNodeVm, "path">): NoteTagNodeVm {
  return {
    name: p.name ?? p.path.split("/").pop() ?? p.path,
    path: p.path,
    count: p.count ?? 1,
    children: p.children ?? [],
  };
}

/**
 * The slice of `CompletionContext` this source touches: the text before the
 * caret and whether the popup was asked for explicitly. Enough for the rules
 * below; NOT enough for the one claim that broke when Story 44.13 took the
 * filtering away from CodeMirror, which is asserted against a real view at the
 * bottom of this file.
 */
function context(before: string, explicit = false): CompletionContext {
  const match = /#[\w/-]*$/.exec(before);
  return {
    explicit,
    matchBefore: () =>
      match === null ? null : { from: match.index, to: before.length, text: match[0] },
  } as unknown as CompletionContext;
}

/** The labels the popup would show for the text typed so far. */
async function offered(vocabulary: string[], before: string): Promise<string[]> {
  const result = (await tagCompleteSource(async () => vocabulary)(
    context(before),
  )) as CompletionResult | null;
  return result === null ? [] : result.options.map((option) => String(option.label));
}

describe("tagPaths", () => {
  it("flattens the tree to full paths, ancestors first", () => {
    const renewal = node({ path: "client/acme/renewal" });
    const acme = node({ path: "client/acme", children: [renewal] });
    const tree = [node({ path: "client", children: [acme] }), node({ path: "standup" })];

    expect(tagPaths(tree)).toEqual(["client", "client/acme", "client/acme/renewal", "standup"]);
  });

  it("flattens an empty tree to nothing", () => {
    expect(tagPaths([])).toEqual([]);
  });
});

describe("tagCompleteSource", () => {
  it("offers a tag that exists only on recordings, because the tree it flattens counts both producers (Story 42.5)", async () => {
    // `q3/kickoff` has never been on a note: it arrived through the recording
    // producer added by Story 42.5. The notes surface offers it anyway — the
    // mirror of AC2, and the reason there is only one vocabulary.
    const tree = [
      node({ path: "meeting" }),
      node({ path: "q3", children: [node({ path: "q3/kickoff" })] }),
    ];

    // Story 44.13 narrowed this list. It used to be every tag in the vault,
    // because CodeMirror did the matching after the fact; keeper does it now,
    // so `meeting` — which no segment of `q` reaches — is no longer offered.
    expect(await offered(tagPaths(tree), "notes on #q")).toEqual(["q3", "q3/kickoff"]);
  });

  it("matches at a segment boundary, not by substring (Story 44.13)", async () => {
    // The failure this replaces: CodeMirror's fuzzy filter hit `client` on the
    // `ent` buried in the middle of it, and offered a tag nobody was reaching
    // for. A segment either starts with what was typed or it does not.
    const vocabulary = ["client", "client/acme", "renewal/entry"];

    expect(await offered(vocabulary, "#ent")).toEqual(["renewal/entry"]);
    expect(await offered(vocabulary, "#acme")).toEqual(["client/acme"]);
  });

  it("re-queries as the tag is typed rather than pinning the first list", async () => {
    // `validFor` let CodeMirror reuse a result across further keystrokes. That
    // was safe while CodeMirror was also re-filtering it; with `filter: false`
    // it would freeze the popup on whatever the first character matched.
    const result = (await tagCompleteSource(async () => ["standup"])(
      context("a #sta"),
    )) as CompletionResult;

    expect(result.filter).toBe(false);
    expect(result.validFor).toBeUndefined();
  });

  it("stays shut when there is no open tag before the caret", async () => {
    expect(await offered(["standup"], "just prose")).toEqual([]);
  });

  it("leaves a bare `#` alone unless the popup was asked for, so a heading stays a heading", async () => {
    const source = tagCompleteSource(async () => ["standup"]);

    expect(await source(context("#"))).toBeNull();
    expect(await source(context("#", true))).not.toBeNull();
  });

  it("replaces only the tag text, never the `#` that opened it", async () => {
    const result = (await tagCompleteSource(async () => ["standup"])(
      context("a #sta"),
    )) as CompletionResult;

    expect(result.from).toBe("a ".length + 1);
  });
});

/**
 * The impure half, driven through a real `EditorView`.
 *
 * Everything above talks to a hand-made `CompletionContext`, and a hand-made
 * context cannot see the failure this change could actually cause: `filter:
 * false` tells CodeMirror to stop narrowing the list, and a `validFor` left
 * beside it would tell CodeMirror to stop re-asking as well — so the popup
 * would freeze on whatever the first keystroke matched and every assertion
 * above would still pass. Epic 43 shipped exactly this shape of defect twice.
 * The only way to know is to type into the thing.
 */
describe("the popup in a real editor", () => {
  // jsdom lays nothing out, so CodeMirror's measure pass throws on a `Range`
  // with no client rects. Same shim, same reason, as `indent-keymap.test.ts`.
  if (!Range.prototype.getClientRects) {
    Range.prototype.getClientRects = () =>
      Object.assign([] as DOMRect[], { item: () => null }) as unknown as DOMRectList;
    Range.prototype.getBoundingClientRect = () => new DOMRect();
  }

  const VOCABULARY = ["work", "work/clients", "work/clients/acme", "worry"];

  it("narrows as each character is typed, and reaches the child past the slash", async () => {
    const parent = document.createElement("div");
    document.body.append(parent);
    const view = new EditorView({
      parent,
      state: EditorState.create({
        extensions: [autocompletion({ override: [tagCompleteSource(async () => VOCABULARY)] })],
      }),
    });
    const type = (text: string) => {
      view.dispatch({
        changes: { from: view.state.doc.length, insert: text },
        selection: { anchor: view.state.doc.length + text.length },
        userEvent: "input.type",
      });
    };
    const shown = async (expected: string[]) => {
      await vi.waitFor(() => {
        expect(currentCompletions(view.state).map((option) => String(option.label))).toEqual(
          expected,
        );
      });
    };

    type("#w");
    await shown(["work", "worry", "work/clients", "work/clients/acme"]);

    type("ork");
    await shown(["work", "work/clients", "work/clients/acme"]);

    // Past the separator: the segment boundary is where the hierarchy opens,
    // and the parent drops out because a second segment was asked for.
    type("/c");
    await shown(["work/clients", "work/clients/acme"]);

    view.destroy();
    parent.remove();
  });
});
