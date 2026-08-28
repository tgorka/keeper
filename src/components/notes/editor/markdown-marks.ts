/**
 * `==highlight==`, which no markdown parser in this app knew about (Story 55.3).
 *
 * The editor loads CommonMark + GFM + Subscript + Superscript. None of those
 * define `==`, so a highlight typed by hand stayed four literal equals signs in
 * the rendered note — which is why the toolbar could not simply grow a button:
 * there was no node for a command to toggle and nothing for the preview layer
 * to decorate.
 *
 * This is the delimiter definition `@lezer/markdown` is missing, written
 * against the same interface its own `Strikethrough` uses and following the
 * same flanking rules. Highlight is not in CommonMark or GFM; it is the
 * spelling Obsidian, Discord and most wikis settled on, and it is the one
 * `format-commands.ts` already names in a comment while explaining what `_` is
 * *not*.
 *
 * # The flanking rules, and why they are copied rather than simplified
 *
 * A delimiter may open only if what follows it is not whitespace, and may close
 * only if what precedes it is not whitespace — with the punctuation cases that
 * let `==a==,` close against a comma. Without them `a == b` becomes a highlight
 * that swallows the rest of the paragraph, and an arithmetic note turns yellow
 * halfway down. The predicates below are the ones CommonMark specifies for
 * emphasis and GFM reuses for `~~`.
 */

import { tags } from "@lezer/highlight";
import type { MarkdownConfig } from "@lezer/markdown";

/** The character, once. `=` is 61. */
const EQUALS = 61;

/**
 * CommonMark's punctuation class, which `@lezer/markdown` keeps to itself.
 *
 * Copied rather than imported because it is not exported; kept in one place so
 * the two flanking tests below cannot drift apart.
 */
const PUNCTUATION =
  /[!-/:-@[-`{-~¡§«¶·»¿;·՚-՟։֊־׀׃׆׳״؉؊،؍؛؞؟٪-٭۔܀-܍߷-߹࠰-࠾࡞।॥॰૰෴๏๚๛༄-༒༔༺-༽྅࿐-࿔࿙࿚၊-၏჻፠-፨᐀᙭᙮᚛᚜᛫-᛭᜵᜶។-៖៘-៚᠀-᠊᥄᥅᨞᨟᪠-᪦᪨-᪭᭚-᭠᯼-᯿᰻-᰿᱾᱿᳀-᳇᳓‐-‧‰-⁃⁅-⁑⁓-⁞⁽⁾₍₎⌈-⌋〈〉❨-❵⟅⟆⟦-⟯⦃-⦘⧘-⧛⧼⧽⳹-⳼⳾⳿⵰⸀-⸮⸰-⹂、-〃〈-】〔-〟〰〽゠・꓾꓿꘍-꘏꙳꙾꛲-꛷꡴-꡷꣎꣏꣸-꣺꣼꤮꤯꥟꧁-꧍꧞꧟꩜-꩟꫞꫟꫰꫱꯫﴾﴿︐-︙︰-﹒﹔-﹡﹣﹨﹪﹫！-＃％-＊，-／：；？＠［-］＿｛｝｟-･]/;

const HighlightDelim = { resolve: "Highlight", mark: "HighlightMark" };

/**
 * `==` as an inline delimiter pair, producing `Highlight` / `HighlightMark`.
 *
 * Placed after Emphasis so `**==both==**` nests the way the other marks do.
 */
export const Highlight: MarkdownConfig = {
  defineNodes: [
    { name: "Highlight", style: { "Highlight/...": tags.special(tags.content) } },
    { name: "HighlightMark", style: tags.processingInstruction },
  ],
  parseInline: [
    {
      name: "Highlight",
      parse(cx, next, pos) {
        // Exactly two. `===` is a setext heading rule at the start of a line and
        // has no business becoming a mark anywhere else either.
        if (next !== EQUALS || cx.char(pos + 1) !== EQUALS || cx.char(pos + 2) === EQUALS) {
          return -1;
        }
        const before = cx.slice(pos - 1, pos);
        const after = cx.slice(pos + 2, pos + 3);
        const spaceBefore = /\s|^$/.test(before);
        const spaceAfter = /\s|^$/.test(after);
        const punctBefore = PUNCTUATION.test(before);
        const punctAfter = PUNCTUATION.test(after);
        return cx.addDelimiter(
          HighlightDelim,
          pos,
          pos + 2,
          // May open: something non-blank follows.
          !spaceAfter && (!punctAfter || spaceBefore || punctBefore),
          // May close: something non-blank precedes.
          !spaceBefore && (!punctBefore || spaceAfter || punctAfter),
        );
      },
      after: "Emphasis",
    },
  ],
};

/**
 * Every mark extension this app's markdown speaks, as one list.
 *
 * Two call sites build a markdown parser — the note editor and the file
 * viewer's rendered `.md` — and `markdown-preview.ts`'s own header explains
 * that a note and the same bytes opened from Files must not render
 * differently. One exported list is how that stays true when the next mark
 * arrives.
 */
export const MARKDOWN_MARKS: readonly MarkdownConfig[] = [Highlight];
