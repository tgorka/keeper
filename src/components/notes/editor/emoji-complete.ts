/**
 * `:shortcode:` emoji, in the editor (Story 45.11, FR-185, AD-92, UX-DR74).
 *
 * Two halves of one affordance:
 *
 *  - **A menu**, for the shortcode you half remember. It is one more
 *    `CompletionSource` in the one `autocompletion()` call this product makes
 *    over markdown — `editor/writing-tools.ts` since Story 50.3, the note
 *    editor's own call before that — and not a second popup. Story 43.9 is what
 *    a from-scratch popup costs: the `/` menu had never opened for anybody, ever,
 *    because it anchored `from` at the trigger character and the matcher then
 *    filtered every option out. So this source is shaped exactly like
 *    `tag-complete.ts` — `from` sits AFTER the colon so the match span is the
 *    word being typed, `apply` reaches back to `from - 1` to take the colon with
 *    it, and `filter: false` hands narrowing to keeper's own matcher rather than
 *    to CodeMirror's fuzzy one.
 *  - **A closing colon**, for the shortcode you know. Typing `:tada:` straight
 *    through produces 🎉 and never has to be noticed as a menu interaction.
 *
 * **The menu does NOT open on a bare colon, and that is the design.** Prose is
 * full of colons — `Note:`, `12:30`, `https://`, `title: value` — and a menu
 * that appears at every one of them is a menu people switch off. Two rules keep
 * it quiet:
 *
 *  1. At least one shortcode character must follow the colon. A bare `:` opens
 *     nothing unless it was asked for explicitly (Ctrl-Space) — the identical
 *     rule `tag-complete.ts` applies to a bare `#`, for the identical reason:
 *     the ambiguity is settled by what was typed, never by a setting.
 *  2. The colon must START a word. A colon preceded by a word character, a
 *     slash or another colon is punctuation inside something else, so `12:30`,
 *     `key:value`, `https://x` and `::` stay shut while `:tada`, `(:tada` and
 *     `**:tada` open.
 *
 * **Why the closing colon is a transaction filter and not `commitCharacters`.**
 * CodeMirror has a `commitCharacters` field for exactly this shape of thing,
 * and it is the wrong tool here: it commits on `keydown` and then lets the
 * keystroke through, which is correct for a language server (`.` commits the
 * member and is then typed) and would leave `🎉:` in a note. A filter over the
 * insertion replaces the whole `:tada:` with the character and nothing is left
 * behind. It also answers the unknown case in the same breath: `:zzzz:` is not
 * in the table, the filter declines, and the text stays exactly as typed.
 *
 * **The table is not in the app's startup bundle.** It is ~45 KB of generated
 * data, and it is reached by a plain static import on purpose: this module sits
 * behind `editor/writing-tools.ts`, which both surfaces that mount the writing
 * tools reach through a dynamic `import()`, so the cost is paid when a note or a
 * markdown file is opened and never by someone who only listed folders. A second
 * layer of laziness here would buy nothing and would make the closing colon race
 * the chunk it needs to answer.
 */
import type {
  Completion,
  CompletionContext,
  CompletionResult,
  CompletionSource,
} from "@codemirror/autocomplete";
import { EditorState, type Extension, type TransactionSpec } from "@codemirror/state";
import type { EditorView } from "@codemirror/view";
import { type EmojiMatch, emojiFor, matchEmoji } from "@/lib/emoji/match";

/**
 * A colon followed by shortcode characters, with the caret at the end.
 *
 * `+` and `-` are in the class so `:+1` and `:-1` can be typed at all. Upper
 * case is in it because `matchEmoji` folds case: rejecting `:Sm` at the trigger
 * while the matcher would happily have answered it is the kind of split rule
 * that becomes a bug report about emoji "not working sometimes".
 */
const OPEN_SHORTCODE = /:[a-zA-Z0-9_+-]*$/;

/** The same shape, but complete — at least one character between the colons. */
const TYPED_SHORTCODE = /:([a-zA-Z0-9_+-]+)$/;

/**
 * What may not sit immediately before the colon.
 *
 * A word character means the colon is inside a token (`12:30`, `key:value`); a
 * slash means it is inside a URL (`https://`); a second colon means a shortcode
 * is being closed or `::` typed, and reopening the menu there would offer to
 * finish a word that is already finished.
 */
const NOT_BEFORE_COLON = /[\w:/]/;

/** Typed text that has narrowed nothing: absent, or only word separators. */
const NOTHING_TYPED = /^_*$/;

/** Narrows the vocabulary. A seam, so a test can assert what the source asks for. */
export type EmojiSource = (query: string) => readonly EmojiMatch[];

/** Resolves a complete shortcode. The other half of the same seam. */
export type EmojiLookup = (shortcode: string) => string | undefined;

/**
 * Whether the colon at `at` begins a word rather than sitting inside one.
 *
 * Both halves of this module ask it, of the same document, and they must never
 * answer differently: a menu that opened where the closing colon will not
 * commit is a menu that offers something it cannot deliver.
 */
function startsAWord(text: string, at: number): boolean {
  return at === 0 || !NOT_BEFORE_COLON.test(text[at - 1] as string);
}

export function emojiCompleteSource(source: EmojiSource = matchEmoji): CompletionSource {
  return (context: CompletionContext): CompletionResult | null => {
    const opened = context.matchBefore(OPEN_SHORTCODE);
    if (opened === null) {
      return null;
    }
    const typed = opened.text.slice(1);
    // A lone `:` waits to be asked, so a colon in prose costs nothing. `:___`
    // waits too: it cuts into no words, so it has narrowed nothing, and the
    // matcher would hand back the head of the whole table for it.
    if (NOTHING_TYPED.test(typed) && !context.explicit) {
      return null;
    }
    const line = context.state.doc.lineAt(opened.from);
    if (!startsAWord(line.text, opened.from - line.from)) {
      return null;
    }
    const matches = source(typed);
    if (matches.length === 0) {
      // An unknown shortcode is text. A result with no options would look the
      // same to CodeMirror; saying it here is what tells the next reader it was
      // a decision rather than an oversight.
      return null;
    }
    const options: Completion[] = matches.map((hit) => ({
      // The label is the shortcode, because that is what everything downstream
      // compares against the typed text — the tests included. What the row
      // SHOWS leads with the character, because a column of names is not how
      // anybody picks an emoji.
      label: hit.shortcode,
      displayLabel: `${hit.emoji} ${hit.shortcode}`,
      type: "text",
      // `from` is the character after the `:`, so the colon is one to the left
      // and has to leave with the word — otherwise picking `tada` writes `:🎉`.
      apply: (view: EditorView, _completion: Completion, from: number, to: number) => {
        view.dispatch({ changes: { from: from - 1, to, insert: hit.emoji } });
      },
    }));
    // `filter: false` hands narrowing to `matchEmoji`, and that is also why
    // there is no `validFor`: a result that survives the next keystroke without
    // re-querying is a result that never narrows once keeper is the one
    // narrowing it — the reasoning `tag-complete.ts` records for Story 44.13.
    return { from: opened.from + 1, options, filter: false };
  };
}

/**
 * Turn `:tada:` into 🎉 as the closing colon is typed.
 *
 * A transaction filter rather than an `inputHandler` because it must be
 * provable: a filter sees exactly the transaction real typing produces, so a
 * test that types the way every other editor test types is testing the shipped
 * path and not a hook nobody can reach from jsdom.
 *
 * It declines for everything that is not one person typing one colon at one
 * caret — a paste, a multi-cursor insert, a remote reconcile — because those
 * are not somebody finishing a shortcode, and rewriting them would be this
 * module deciding what a document says.
 */
export function emojiShortcodeCommit(lookup: EmojiLookup = emojiFor): Extension {
  return EditorState.transactionFilter.of((tr): TransactionSpec | readonly TransactionSpec[] => {
    if (!tr.docChanged || !tr.isUserEvent("input.type")) {
      return tr;
    }
    // One person, one colon, one caret. A paste, a multi-cursor insert or a
    // reconcile from Rust is not somebody finishing a shortcode.
    let at = -1;
    let edits = 0;
    tr.changes.iterChanges((fromA, toA, _fromB, _toB, inserted) => {
      edits += 1;
      at = fromA === toA && inserted.toString() === ":" ? fromA : -1;
    });
    if (edits !== 1 || at < 0) {
      return tr;
    }
    const line = tr.startState.doc.lineAt(at);
    const typed = TYPED_SHORTCODE.exec(line.text.slice(0, at - line.from));
    if (typed === null) {
      return tr;
    }
    const start = at - (typed[0] as string).length;
    if (!startsAWord(line.text, start - line.from)) {
      return tr;
    }
    const emoji = lookup(typed[1] as string);
    if (emoji === undefined) {
      // Unknown, so it is text: the colon is typed and `:zzzz:` stays put.
      return tr;
    }
    return {
      changes: { from: start, to: at, insert: emoji },
      selection: { anchor: start + emoji.length },
      userEvent: "input.type",
    };
  });
}
