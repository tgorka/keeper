import type { CompletionContext, CompletionResult } from "@codemirror/autocomplete";
import { describe, expect, it } from "vitest";
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
 * caret and whether the popup was asked for explicitly. Standing up a real
 * CodeMirror view to assert a list of tags would test CodeMirror.
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

    expect(await offered(tagPaths(tree), "notes on #q")).toEqual(["meeting", "q3", "q3/kickoff"]);
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
