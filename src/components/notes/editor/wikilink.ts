/**
 * The `[[` wikilink grammar, plus completion and create-on-Enter (Story 37.7,
 * FR-108).
 *
 * The candidate list comes from the core link graph over `notes_link_targets`,
 * never from anything the webview keeps — the index lives in Rust and a second
 * copy here would be a second answer to "does this note exist".
 *
 * The create-on-Enter branch is the one that matters: a name with no match is
 * not an error and not a dead end. Accepting it writes the note through the
 * ordinary writer and links it, so following a link you have just invented is
 * the same act as following one that was already there.
 *
 * {@link WIKILINK} lives here rather than beside the decorations that use it
 * because it is now read by two of them — the live-preview renderer and the
 * recording-embed widget — and a wikilink that the renderer and the embed
 * disagreed about would be a link that renders one way and resolves another.
 */
import type {
  Completion,
  CompletionContext,
  CompletionResult,
  CompletionSource,
} from "@codemirror/autocomplete";
import type { EditorView } from "@codemirror/view";
import { notesCreate, notesLinkTargets } from "@/lib/ipc/client";

/** The text between `[[` and the caret. */
const OPEN_WIKILINK = /\[\[([^[\]|]*)$/;

/**
 * `[[target]]`, `[[target|alias]]` and the `![[…]]` embed form, anywhere in a
 * line. Lezer's markdown grammar knows nothing about wikilinks, so this is the
 * whole of keeper's knowledge of the syntax.
 *
 * Group 1 is the target, group 2 the alias when there is one, and a leading `!`
 * survives in group 0 — which is how a caller tells an embed from a link
 * without a second pattern.
 *
 * Stateful (`g`): reset `lastIndex` or use `matchAll`, never `test`.
 */
export const WIKILINK = /!?\[\[([^[\]|]+)(?:\|([^[\]]+))?]]/g;

/** The attribute a click handler reads the link target back out of. */
export const WIKILINK_ATTR = "data-keeper-wikilink";

/** Replace the typed name with `name]]`, swallowing brackets already closed
 *  for us, and park the caret after the link. */
function insertLink(view: EditorView, from: number, to: number, name: string): void {
  const closing = view.state.sliceDoc(to, to + 2) === "]]" ? to + 2 : to;
  view.dispatch({
    changes: { from, to: closing, insert: `${name}]]` },
    selection: { anchor: from + name.length + 2 },
  });
}

/**
 * Wikilink completion over one vault.
 *
 * `onCreated` is called with the note the create branch wrote, so the surface
 * around the editor (the list, the backlinks panel) can pick it up without
 * waiting for the watcher to come back around.
 */
export function wikilinkSource(
  vaultId: string,
  onCreated?: (noteId: string) => void,
): CompletionSource {
  return async (context: CompletionContext): Promise<CompletionResult | null> => {
    const opened = context.matchBefore(OPEN_WIKILINK);
    if (opened === null) {
      return null;
    }
    const typed = opened.text.slice(2);
    if (typed === "" && !context.explicit) {
      // An empty `[[` still completes, but only once the user asks — otherwise
      // every opening bracket costs an IPC round trip.
      return { from: opened.from + 2, options: [], validFor: /^[^[\]|]*$/ };
    }
    const targets = await notesLinkTargets(vaultId, typed);
    const options: Completion[] = targets.map((target) => ({
      label: target.title,
      detail: target.path,
      apply: (view: EditorView, _completion: Completion, from: number, to: number) => {
        insertLink(view, from, to, target.title);
      },
    }));

    const exact = targets.some((target) => target.title.toLowerCase() === typed.toLowerCase());
    if (typed.trim() !== "" && !exact) {
      options.push({
        label: typed,
        detail: "create and link",
        // Ranked last: an existing note is almost always what was meant.
        boost: -50,
        apply: (view: EditorView, _completion: Completion, from: number, to: number) => {
          insertLink(view, from, to, typed);
          void notesCreate(vaultId, {
            title: typed,
            body: null,
            template: null,
            dest: null,
            tags: [],
            // A wikilink names a note, not a space. The note it stubs out has
            // to be reachable by the link that was just written and by nothing
            // else, so it inherits nothing.
            space: null,
          }).then((created) => {
            onCreated?.(created.note.id);
          });
        },
      });
    }

    return { from: opened.from + 2, options, validFor: /^[^[\]|]*$/ };
  };
}
