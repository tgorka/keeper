/**
 * Tab belongs to the editor (Story 43.1, FR-146).
 *
 * CodeMirror binds no `Tab` by default, and that is a decision rather than an
 * omission: a text box that swallows Tab is a keyboard trap, so the library
 * leaves the key to the platform unless an editor claims it. The cost of not
 * claiming it is that every Tab keydown escapes `preventDefault` and reaches
 * the web view, and what a web view does with Tab inside a `contenteditable`
 * is its own business — WebKit edits the DOM under CodeMirror's feet, and the
 * garbage the editor then reads back is the "stray whitespace, maybe two blank
 * lines" the owner reported. The document never asked for any of it.
 *
 * So the fix is not "insert an indent"; it is **claim the key**. Everything
 * below is CodeMirror's own commands, and the load-bearing property is that
 * `run` returns `true`, which is what stops the keystroke before the platform
 * can act on it.
 *
 * **The escape hatch is not re-implemented here, and must not be.**
 * `@codemirror/view` already answers the accessibility problem: pressing
 * `Escape` arms `tabFocusMode` for two seconds, and while it is armed the view
 * drops the next Tab keydown before any keymap handler runs, so the browser
 * moves focus. That check sits inside the view's own event dispatch, so it
 * covers the `keymap` facet and `EditorView.domEventHandlers` alike — but a
 * bare `addEventListener("keydown", …)` on the content DOM runs after the
 * view has already declined the event, and would rebuild the keyboard trap
 * the default binding exists to prevent. `tab-wiring.test.tsx` fails on
 * exactly that mutation and on no other.
 *
 * **Why an indent unit of two spaces, which is CodeMirror's default and is
 * therefore left alone.** Obsidian opens the same file, so the indent has to be
 * something CommonMark reads the way the user meant it. Two spaces is the
 * content column of `- `, `* ` and `- [ ] ` — the markers keeper's own slash
 * menu writes — so one Tab nests a bullet or a task exactly. It is also below
 * the four-space threshold at which CommonMark turns an indented line into a
 * code block, so Tab on a plain paragraph line indents prose and never
 * silently changes what the paragraph *is*. An ordered item (`1. `, content
 * column three) needs two presses to nest; that is the price of never
 * corrupting prose, and it is a worse indent rather than a wrong document.
 * A literal `\t` is not an option at all — the epic rules it out, and a tab
 * renders at a width the next reader's editor chooses.
 */
import { acceptCompletion } from "@codemirror/autocomplete";
import { indentLess, indentMore } from "@codemirror/commands";
import type { KeyBinding } from "@codemirror/view";

/**
 * `indentWithTab`, with completion acceptance in front of it.
 *
 * The completion arm is not decoration. `indentMore` inserts at the *line
 * start*, so a Tab pressed while the slash menu or tag popup is open would push
 * whitespace in front of the `/` or `#` the popup is matching on — dismissing
 * the popup and leaving behind exactly the kind of unasked-for whitespace this
 * story exists to remove. `acceptCompletion` returns `false` when no completion
 * is active, so the ordinary path is untouched.
 */
export const indentBindings: readonly KeyBinding[] = [
  {
    key: "Tab",
    run: (view) => acceptCompletion(view) || indentMore(view),
    shift: indentLess,
  },
];
