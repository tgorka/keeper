/**
 * The shared tag-completion field (Story 42.5, FR-143, UX-DR52).
 *
 * **Why this exists.** keeper had exactly one tag-completion affordance and it
 * was a CodeMirror `CompletionSource` (`components/notes/editor/tag-complete.ts`).
 * A `CompletionSource` needs a CodeMirror view, so the recording metadata
 * card's plain `<Input>` could never reach it, and the recording surface went
 * without completion at all — which is precisely how a second tag vocabulary
 * grew up beside the first. This field closes that gap without forking
 * anything: it offers the SAME vocabulary the notes tag tree is built from,
 * read from the one Rust query that sums both producers, so a tag that exists
 * only on notes is offered while tagging a recording, and the other way round.
 *
 * **Why a `<datalist>`.** It is what this repo already reaches for when a
 * free-text field wants live suggestions — `components/search/search-panel.tsx`
 * seeds its sender field exactly this way. It costs no popover state machine,
 * no focus trap and no roving tabindex, and the browser supplies the filtering,
 * keyboard handling and screen-reader semantics a hand-rolled listbox would
 * have to re-earn. A real combobox would be the right call if picking a tag had
 * to do more than insert text. It does not.
 *
 * **Why it normalises nothing.** What a tag MEANS is decided once, in Rust
 * (`keeper-core/src/notes/tags.rs`): case, whitespace, separators, emptiness.
 * This field shows what the user typed and hands it onward verbatim. The one
 * text rule it does own is where the current tag STARTS inside a
 * comma-separated field — a caret rule, the direct analogue of the `OPEN_TAG`
 * match in `tag-complete.ts`, and not a statement about what a tag is.
 */
import { useEffect, useState } from "react";
import { Input } from "@/components/ui/input";
import { tagsVocabulary } from "@/lib/ipc/client";

/** The `<datalist>` id paired with an input id. Exported so callers (and their
 *  tests) can find the suggestions without guessing at the composition. */
export function tagVocabularyListId(inputId: string): string {
  return `${inputId}-vocabulary`;
}

/**
 * The suggestion values offered for a comma-separated tag field.
 *
 * Picking a `<datalist>` suggestion replaces the input's WHOLE value, so
 * completing the second tag in `standup, cl` means offering
 * `standup, client/acme`: the untouched head of the field, then the candidate.
 * Whatever whitespace the user typed after the comma is carried through rather
 * than regularised — this is a suggestion, not a correction, and correcting
 * here would be the frontend deciding what a tag looks like.
 *
 * No filtering happens here: the browser matches the typed text against the
 * offered values, and re-implementing that match would be a second matching
 * rule beside the one `tag-complete.ts` already gets from CodeMirror.
 */
export function tagSuggestions(typed: string, vocabulary: readonly string[]): string[] {
  const lastComma = typed.lastIndexOf(",");
  const head = typed.slice(0, lastComma + 1);
  const gap = /^\s*/.exec(typed.slice(lastComma + 1))?.[0] ?? "";
  return vocabulary.map((tag) => `${head}${gap}${tag}`);
}

export function TagVocabularyInput({
  id,
  value,
  onChange,
  placeholder,
  vaultId,
}: {
  /** The input's DOM id; the label points at it and the datalist derives from it. */
  id: string;
  /** The raw text the user has typed, comma-separated tags and all. */
  value: string;
  /** Receives the raw text, unmodified. */
  onChange: (value: string) => void;
  placeholder?: string;
  /** Scope the vocabulary to a vault; omitted, Rust resolves the active one —
   *  the recording metadata card has no vault of its own. */
  vaultId?: string;
}) {
  const [vocabulary, setVocabulary] = useState<readonly string[]>([]);
  const listId = tagVocabularyListId(id);

  useEffect(() => {
    let cancelled = false;
    void tagsVocabulary(vaultId)
      .then((vm) => {
        if (!cancelled) {
          setVocabulary(vm.entries.map((entry) => entry.path));
        }
      })
      .catch(() => {
        // A vocabulary that will not load leaves an ordinary text field behind.
        // Completion is an aid: typing a tag by hand still works, and what the
        // typed text means is settled in Rust either way.
      });
    return () => {
      cancelled = true;
    };
  }, [vaultId]);

  return (
    <>
      <Input
        id={id}
        list={listId}
        value={value}
        placeholder={placeholder}
        onChange={(event) => onChange(event.target.value)}
      />
      <datalist id={listId}>
        {tagSuggestions(value, vocabulary).map((suggestion) => (
          <option key={suggestion} value={suggestion} />
        ))}
      </datalist>
    </>
  );
}
