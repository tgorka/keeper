/**
 * `#` tag completion (Story 37.3, FR-104).
 *
 * Hierarchical by construction: the vault's tag tree is flattened to full paths
 * (`work`, `work/clients`, `work/clients/acme`), so completing segment by
 * segment is just prefix matching over that list and needs no special casing.
 *
 * The ambiguity with markdown headings is resolved by position, not by a
 * setting: `# ` at the start of a line is a heading and never opens the popup,
 * because the trigger requires a tag character immediately after the `#`.
 * Escape closes the popup and leaves the literal `#` behind — a completion that
 * eats what you typed when you dismiss it is worse than no completion.
 */
import type {
  CompletionContext,
  CompletionResult,
  CompletionSource,
} from "@codemirror/autocomplete";

/** A `#` followed by tag characters, with the caret at the end. */
const OPEN_TAG = /#[\w/-]*$/;

/** Supplies every tag in the vault as a full path. Async because the tag tree
 *  lives in Rust; the caller decides whether to cache it. */
export type TagSource = () => Promise<string[]>;

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
      options: all.map((tag) => ({ label: tag, type: "keyword" })),
      validFor: /^[\w/-]*$/,
    };
  };
}
