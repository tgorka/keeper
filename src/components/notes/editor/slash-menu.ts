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
import { gfmTable } from "./format-commands";

/** A slash at the very start of an otherwise empty line, caret after it. */
const OPEN_SLASH = /^\/\w*$/;

/**
 * A table the user can tab through rather than a table they must draw.
 *
 * **Story 44.9 changed the bytes this inserts, deliberately.** 43.9 pinned a
 * hand-written skeleton whose pipes did not line up; the toolbar's table
 * builder writes an aligned GFM table for the reason stated in
 * `format-commands.ts`, and two table commands with two different outputs in
 * one editor is the kind of divergence nobody notices until a diff is
 * unreadable. So there is one builder, and `/` calls it: two columns and one
 * body row under a header, which is what this row has always promised.
 */
const TABLE_SKELETON = gfmTable({ rows: 2, columns: 2, header: true });

export interface SlashCommand {
  /** What the menu row reads. */
  label: string;
  /** The one-line explanation beside it. */
  detail: string;
  /** The text to put in the document, computed at accept time. */
  text: (now: Date) => string;
  /**
   * Where the caret lands inside the inserted text, as an offset into it.
   *
   * Absent means the end, which is right for everything that inserts a
   * *prefix* — a date, a task marker — and wrong for everything that inserts a
   * *pair*. Picking "Superscript" and getting `^^` with the caret past both
   * carets means typing the exponent outside its own delimiters, which is the
   * kind of thing a person does once and then stops using the menu.
   */
  caret?: number;
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
  // The three inline marks Story 45.10 added. Each is written in the same
  // spelling `format-commands.ts` writes, because a note must not be able to
  // tell which of the two doors a mark came through.
  { label: "Subscript", detail: "H~2~O", text: () => "~~", caret: 1 },
  { label: "Superscript", detail: "x^2^", text: () => "^^", caret: 1 },
  { label: "Underline", detail: "<u>…</u>", text: () => "<u></u>", caret: 3 },
  // The caret goes on the empty line between the fences, not after the closing
  // one: a code fence you have to arrow back into is a code fence you retype.
  { label: "Code fence", detail: "```", text: () => "```\n\n```\n", caret: 4 },
  {
    label: "Mermaid diagram",
    detail: "```mermaid",
    text: () => "```mermaid\ngraph TD\n\n```\n",
    caret: 20,
  },
  // The one way into Story 44.15's block that does not require knowing its
  // syntax. The caret lands after the space, where the folder goes; the block
  // stays source until the caret leaves it, so a half-typed folder is never
  // listed.
  {
    label: "Gallery",
    detail: "> [!gallery] a folder of media",
    text: () => "> [!gallery] ",
  },
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
        const insert = command.text(new Date());
        const start = from - 1;
        view.dispatch({
          changes: { from: start, to, insert },
          selection: { anchor: start + Math.min(command.caret ?? insert.length, insert.length) },
        });
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
