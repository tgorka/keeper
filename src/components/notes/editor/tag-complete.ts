/**
 * `#` tag completion (Story 37.3, FR-104).
 *
 * Hierarchical by construction: the vault's tag tree is flattened to full paths
 * (`work`, `work/clients`, `work/clients/acme`), so completing segment by
 * segment is just prefix matching over that list and needs no special casing.
 *
 * Story 42.5: that tree now counts BOTH producers, so this source offers tags
 * that exist only on recordings without knowing recordings exist — one
 * vocabulary, reached from both surfaces. The plain `<Input>` in the recording
 * metadata card cannot consume a `CompletionSource`, so it has its own
 * affordance (`components/tags/tag-vocabulary-input.tsx`) over the same
 * vocabulary; neither one decides what a tag is.
 *
 * The ambiguity with markdown headings is resolved by position, not by a
 * setting: `# ` at the start of a line is a heading and never opens the popup,
 * because the trigger requires a tag character immediately after the `#`.
 * Escape closes the popup and leaves the literal `#` behind — a completion that
 * eats what you typed when you dismiss it is worse than no completion.
 *
 * Story 44.13: **which of those tags match what has been typed is no longer
 * CodeMirror's opinion.** It used to be — this source handed over the whole
 * vocabulary and let the library's fuzzy matcher narrow it, which meant `#ent`
 * offered `client` on a substring hit in the middle of a segment, and it meant
 * the editor and the tag chooser answered the same question two different ways.
 * Both now ask `components/tags/tag-match.ts`, which matches the way the tag
 * tree is shaped: at segment boundaries. `filter: false` is what hands the
 * decision over, and it is why `validFor` is gone with it — a result that
 * survives the next keystroke without re-querying is a result that never
 * narrows once keeper is the one narrowing it.
 */
import type {
  CompletionContext,
  CompletionResult,
  CompletionSource,
} from "@codemirror/autocomplete";
import { matchTags } from "@/components/tags/tag-match";
import type { NoteTagNodeVm } from "@/lib/ipc/client";

/** A `#` followed by tag characters, with the caret at the end. */
const OPEN_TAG = /#[\w/-]*$/;

/** Supplies every tag in the vault as a full path. Async because the tag tree
 *  lives in Rust; the caller decides whether to cache it. */
export type TagSource = () => Promise<string[]>;

/** Flatten the tag tree to full paths, which is what completion matches on.
 *  Lives beside the source rather than in the editor so the vocabulary the
 *  notes surface actually offers can be asserted on its own. */
export function tagPaths(nodes: readonly NoteTagNodeVm[], into: string[] = []): string[] {
  for (const node of nodes) {
    into.push(node.path);
    tagPaths(node.children, into);
  }
  return into;
}

export function tagCompleteSource(tags: TagSource): CompletionSource {
  return async (context: CompletionContext): Promise<CompletionResult | null> => {
    const opened = context.matchBefore(OPEN_TAG);
    if (opened === null) {
      return null;
    }
    // A `#` in the first column with nothing after it is far more likely to
    // become a heading than a tag, so it waits to be asked.
    if (opened.text === "#" && !context.explicit) {
      return null;
    }
    const all = await tags();
    return {
      from: opened.from + 1,
      options: matchTags(opened.text.slice(1), all).map((tag) => ({ label: tag, type: "keyword" })),
      filter: false,
    };
  };
}
