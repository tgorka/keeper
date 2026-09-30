/**
 * The note widgets in the `/` menu: the same list the toolbar's *Insert
 * widget* offers (`@/lib/notes/widgets`).
 *
 * A note-only source, handed to `markdownWritingTools` by the note editor as
 * one of its vault sources: a media block needs the note's drive, which a
 * session log opened from Files does not have.
 *
 * **Where it opens.** At the start of a line, like the rest of the menu — its
 * rows then sit beside the shared ones — and also after a space, because a
 * widget is something a person reaches for mid-thought. A `/` glued to text
 * (`and/or`, `1/2`, a path) is never a menu. After a space only the widgets
 * are offered, never the literal insertions, so ` /usr` cannot become a
 * subscript on Enter.
 *
 * **What it does.** The gallery writes its head on a line of its own. The
 * media player removes the typed `/…` and hands the position to the editor,
 * which opens the picker; the block Rust composes lands there.
 */
import type {
  Completion,
  CompletionContext,
  CompletionResult,
  CompletionSource,
} from "@codemirror/autocomplete";
import type { EditorView } from "@codemirror/view";
import { GALLERY_HEAD, NOTE_WIDGETS } from "@/lib/notes/widgets";

/** A `/` at the start of the line or after whitespace, then a word, at the caret. */
const WIDGET_SLASH = /(^|\s)\/(\w*)$/;

export function widgetSlashSource(onPickMedia: (at: number) => void): CompletionSource {
  return (context: CompletionContext): CompletionResult | null => {
    const line = context.state.doc.lineAt(context.pos);
    if (context.pos !== line.to) {
      return null;
    }
    const typed = line.text.slice(0, context.pos - line.from);
    const match = WIDGET_SLASH.exec(typed);
    if (match === null) {
      return null;
    }
    const slash = context.pos - match[2].length - 1;
    const atLineStart = slash === line.from;
    const options: Completion[] = NOTE_WIDGETS.filter(
      // At the start of a line the shared menu already offers Gallery.
      (widget) => widget.id === "media" || !atLineStart,
    ).map((widget) => ({
      label: widget.label,
      detail: widget.detail,
      // Before the shared rows: a widget is what this row was typed for.
      boost: 1,
      apply: (view: EditorView, _completion: Completion, _from: number, to: number) => {
        if (widget.id === "gallery") {
          // A callout only begins at the start of a line; the space the person
          // typed before the `/` is theirs and stays on the line above.
          const insert = `\n${GALLERY_HEAD}`;
          view.dispatch({
            changes: { from: slash, to, insert },
            selection: { anchor: slash + insert.length },
            userEvent: "input.complete",
          });
          return;
        }
        view.dispatch({
          changes: { from: slash, to, insert: "" },
          selection: { anchor: slash },
          userEvent: "input.complete",
        });
        onPickMedia(slash);
      },
    }));
    return { from: slash + 1, options, validFor: /^\w*$/ };
  };
}
