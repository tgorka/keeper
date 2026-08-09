/**
 * The `/` command menu (Story 37.6, FR-100 adjacent).
 *
 * Deliberately narrow: it triggers at the start of an empty line and nowhere
 * else, so a slash inside a path or a fraction stays a slash. Everything it
 * offers is a literal insertion the editor can perform on its own — there is no
 * command in the IPC surface that expands a template into an open buffer
 * (`notes_templates` lists them; `notes_create` applies one at creation), so
 * templates are reached by making a note, not by typing into one. Offering a
 * template here that silently did nothing would be worse than not offering it.
 */
import type {
  Completion,
  CompletionContext,
  CompletionResult,
  CompletionSource,
} from "@codemirror/autocomplete";
import type { EditorView } from "@codemirror/view";

/** A slash at the very start of an otherwise empty line, caret after it. */
const OPEN_SLASH = /^\/\w*$/;

/** A table the user can tab through rather than a table they must draw. */
const TABLE_SKELETON = "| Column | Column |\n| --- | --- |\n|  |  |\n";

export interface SlashCommand {
  /** What the menu row reads. */
  label: string;
  /** The one-line explanation beside it. */
  detail: string;
  /** The text to put in the document, computed at accept time. */
  text: (now: Date) => string;
}

/** The closed set. Dates use the host locale's ISO forms, which is what the
 *  journal and frontmatter already speak. */
export const SLASH_COMMANDS: readonly SlashCommand[] = [
  {
    label: "Today's date",
    detail: "YYYY-MM-DD",
    text: (now) => now.toISOString().slice(0, 10),
  },
  {
    label: "Current time",
    detail: "HH:MM",
    text: (now) =>
      `${String(now.getHours()).padStart(2, "0")}:${String(now.getMinutes()).padStart(2, "0")}`,
  },
  { label: "Task", detail: "- [ ] …", text: () => "- [ ] " },
  { label: "Table", detail: "two columns, one row", text: () => TABLE_SKELETON },
  { label: "Code fence", detail: "```", text: () => "```\n\n```\n" },
  { label: "Mermaid diagram", detail: "```mermaid", text: () => "```mermaid\ngraph TD\n\n```\n" },
];

export function slashMenuSource(
  commands: readonly SlashCommand[] = SLASH_COMMANDS,
): CompletionSource {
  return (context: CompletionContext): CompletionResult | null => {
    const line = context.state.doc.lineAt(context.pos);
    const typed = line.text.slice(0, context.pos - line.from);
    // Start of line only, and nothing after the caret: `/` mid-sentence is a
    // slash, and that is not negotiable per the interaction grammar.
    if (context.pos !== line.to || !OPEN_SLASH.test(typed)) {
      return null;
    }
    const options: Completion[] = commands.map((command) => ({
      label: command.label,
      detail: command.detail,
      // Computed on accept, not on open: a menu left hanging over midnight
      // must not insert yesterday.
      //
      // `from` is the character after the `/`, so the slash itself is one to
      // the left and has to be swallowed with the word — otherwise picking
      // "Task" would leave `/- [ ] ` in the note.
      apply: (view: EditorView, _completion: Completion, from: number, to: number) => {
        view.dispatch({ changes: { from: from - 1, to, insert: command.text(new Date()) } });
      },
    }));
    // **After the `/`, not at it (Story 43.9).** Completion filters options by
    // fuzzy-matching the text between `from` and the caret against each label.
    // Anchoring `from` at the slash put a `/` into that pattern, no label
    // contains one, every option was filtered out — and a completion with zero
    // options is not a menu, so the popup never appeared at all. `validFor`
    // describes the same span and therefore drops the slash with it.
    return { from: line.from + 1, options, validFor: /^\w*$/ };
  };
}
