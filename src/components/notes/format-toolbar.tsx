/**
 * The formatting menu (Story 44.9, FR-164, UX-DR60).
 *
 * **Why this file holds no editor.** Every `@codemirror/*` value in the note
 * editor lives inside its boot closure so the main bundle never loads the
 * editor chunk to paint a pane. The toolbar sits in that main bundle, so it
 * speaks in `FormatAction` — plain data — and `note-editor.tsx` turns the
 * action into a command inside the closure that owns the view. The import
 * below is `import type`, which is erased.
 *
 * **Why every button cancels its own mousedown.** A toolbar that steals the
 * caret is not a toolbar. Pressing a button blurs the editor, and a blurred
 * CodeMirror keeps its selection but the click has already fired the editor's
 * `blur` handler and moved DOM focus — so the command lands, focus is somewhere
 * else, and the next thing the user types goes nowhere. `preventDefault` on
 * `mousedown` stops focus from moving in the first place, and the command
 * handlers call `view.focus()` anyway, so the caret is where it was whichever
 * way the button was reached.
 *
 * **Why the two popovers are hand-rolled.** They need `mousedown` cancelled on
 * exactly the elements that must not take focus and honoured on the two number
 * fields that must, which is the one thing a menu primitive does not let you
 * say. There is no new dependency here and there is not going to be one.
 */
import {
  Bold,
  Code,
  Heading,
  Highlighter,
  Italic,
  Link,
  List,
  ListOrdered,
  ListTodo,
  Quote,
  Smile,
  SquareCode,
  Strikethrough,
  Subscript,
  Superscript,
  Table,
  Underline,
} from "lucide-react";
import { type MouseEvent, useCallback, useId, useMemo, useState } from "react";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { type EmojiMatch, emojiFor, matchEmoji } from "@/lib/emoji/match";
import type { FormatAction } from "./editor/format-commands";

/** Which extra panel, if any, is open. Only ever one. */
type Panel = "heading" | "table" | "emoji" | null;

/** How many emoji the picker shows at once.
 *
 * Six rows of eight. `EMOJI_MATCH_LIMIT` (50) is tuned for a completion menu
 * you arrow through; a grid you look at can hold more without becoming harder
 * to read, and the panel scrolls past the third row anyway. */
const EMOJI_PICKER_LIMIT = 48;

/**
 * What the picker shows before anything is typed.
 *
 * Not the table's head, which is what an unfiltered `matchEmoji("")` gives:
 * `EMOJI_TABLE` is ordered by shortcode, so opening the panel used to show
 * `+1`, `100`, `1234`, `8ball`, `a`, `ab`, `abacus` and a run of flags —
 * accurate, and a wall of letter-symbols nobody came here for.
 *
 * Shortcodes rather than characters, resolved through `emojiFor`, so this list
 * names things the emoji table already defines instead of becoming a second
 * place a character is written down. Anything the table stops recognising drops
 * out silently rather than rendering as a blank button; the test pins that the
 * list resolves in full today.
 *
 * A starting point, not a taxonomy — search reaches the other ~1800.
 */
const EMOJI_OPENING: readonly string[] = [
  "smile",
  "smiley",
  "grin",
  "joy",
  "wink",
  "blush",
  "thinking",
  "neutral_face",
  "slightly_frowning_face",
  "cry",
  "sob",
  "scream",
  "sunglasses",
  "heart_eyes",
  "hugs",
  "raised_eyebrow",
  "+1",
  "-1",
  "ok_hand",
  "clap",
  "raised_hands",
  "pray",
  "muscle",
  "wave",
  "heart",
  "fire",
  "star",
  "sparkles",
  "tada",
  "rocket",
  "zap",
  "boom",
  "white_check_mark",
  "x",
  "warning",
  "question",
  "exclamation",
  "bulb",
  "pushpin",
  "paperclip",
  "memo",
  "book",
  "calendar",
  "hourglass",
  "coffee",
  "eyes",
  "brain",
  "chart_with_upwards_trend",
];

const HEADING_LEVELS = [1, 2, 3, 4, 5, 6] as const;

/** The straight-through buttons, in the order the toolbar shows them.
 *
 *  Grouped the way markdown groups them — the inline marks, then the block
 *  ones, then link — so the five Story 45.10 added sit beside their relatives
 *  rather than at the end where a toolbar goes to accumulate. */
const DIRECT: readonly { action: FormatAction; label: string; Icon: typeof Bold }[] = [
  { action: { kind: "bold" }, label: "Bold", Icon: Bold },
  { action: { kind: "italic" }, label: "Italic", Icon: Italic },
  { action: { kind: "underline" }, label: "Underline", Icon: Underline },
  { action: { kind: "strikethrough" }, label: "Strikethrough", Icon: Strikethrough },
  { action: { kind: "mark" }, label: "Highlight", Icon: Highlighter },
  { action: { kind: "subscript" }, label: "Subscript", Icon: Subscript },
  { action: { kind: "superscript" }, label: "Superscript", Icon: Superscript },
  { action: { kind: "code" }, label: "Inline code", Icon: Code },
  { action: { kind: "codeblock" }, label: "Code block", Icon: SquareCode },
  { action: { kind: "bullet" }, label: "Bullet list", Icon: List },
  { action: { kind: "ordered" }, label: "Numbered list", Icon: ListOrdered },
  { action: { kind: "task" }, label: "Task list", Icon: ListTodo },
  { action: { kind: "quote" }, label: "Quote", Icon: Quote },
  { action: { kind: "link" }, label: "Link", Icon: Link },
];

export interface FormatToolbarProps {
  /** Run the action against whatever the editor's selection is right now. */
  onAction: (action: FormatAction) => void;
}

export function FormatToolbar({ onAction }: FormatToolbarProps) {
  const [panel, setPanel] = useState<Panel>(null);
  // The labels have to name their fields by id, and an editor pane is not
  // guaranteed to be the only one on the page.
  const ids = useId();
  const [emojiQuery, setEmojiQuery] = useState("");
  const [rows, setRows] = useState(3);
  const [columns, setColumns] = useState(2);
  const [header, setHeader] = useState(true);

  // One handler, on every control that must not take focus. Buttons inside the
  // table form are covered too: the form's own inputs are the only elements
  // here that are allowed to move the caret out of the note.
  const keepCaret = useCallback((event: MouseEvent) => event.preventDefault(), []);

  const run = useCallback(
    (action: FormatAction) => {
      setPanel(null);
      onAction(action);
    },
    [onAction],
  );

  // `matchEmoji` over the same vocabulary `:shortcode:` completion searches —
  // the picker is a second door into it, not a second copy. An empty query is
  // the whole table's head rather than nothing, so the panel opens showing
  // emoji instead of an instruction to type.
  const emoji: EmojiMatch[] = useMemo(() => {
    if (emojiQuery.trim() === "") {
      return EMOJI_OPENING.flatMap((shortcode) => {
        const character = emojiFor(shortcode);
        return character === undefined ? [] : [{ shortcode, emoji: character }];
      });
    }
    return matchEmoji(emojiQuery, EMOJI_PICKER_LIMIT);
  }, [emojiQuery]);

  const openPanel = useCallback((which: Exclude<Panel, null>) => {
    setPanel((open) => {
      const next = open === which ? null : which;
      // Reopening starts clean: a stale query is a picker that lies about what
      // it is showing.
      if (next === "emoji") {
        setEmojiQuery("");
      }
      return next;
    });
  }, []);

  return (
    <div className="relative flex flex-wrap items-center gap-0.5 border-b px-2 py-1">
      {DIRECT.map(({ action, label, Icon }) => (
        <Button
          key={label}
          type="button"
          size="icon-sm"
          variant="ghost"
          aria-label={label}
          title={label}
          onMouseDown={keepCaret}
          onClick={() => run(action)}
        >
          <Icon aria-hidden="true" />
        </Button>
      ))}

      <Button
        type="button"
        size="icon-sm"
        variant="ghost"
        aria-label="Heading"
        title="Heading"
        aria-expanded={panel === "heading"}
        onMouseDown={keepCaret}
        onClick={() => openPanel("heading")}
      >
        <Heading aria-hidden="true" />
      </Button>

      <Button
        type="button"
        size="icon-sm"
        variant="ghost"
        aria-label="Table"
        title="Table"
        aria-expanded={panel === "table"}
        onMouseDown={keepCaret}
        onClick={() => openPanel("table")}
      >
        <Table aria-hidden="true" />
      </Button>

      <Button
        type="button"
        size="icon-sm"
        variant="ghost"
        aria-label="Emoji"
        title="Emoji"
        aria-expanded={panel === "emoji"}
        onMouseDown={keepCaret}
        onClick={() => openPanel("emoji")}
      >
        <Smile aria-hidden="true" />
      </Button>

      {panel === "emoji" ? (
        <fieldset
          // Named apart from the button that opens it, as "Heading level" and
          // "Insert table" are: two things answering to "Emoji" is ambiguous to
          // anything navigating by name, a test included.
          aria-label="Emoji picker"
          className="absolute top-full left-2 z-20 mt-1 flex w-72 flex-col gap-2 rounded-md border bg-popover p-2 shadow-md"
        >
          <label className="sr-only" htmlFor={`${ids}-emoji`}>
            Search emoji
          </label>
          <Input
            id={`${ids}-emoji`}
            className="h-8"
            placeholder="Search emoji"
            value={emojiQuery}
            onChange={(event) => setEmojiQuery(event.target.value)}
          />
          {emoji.length === 0 ? (
            // A sentence, not an empty grid: the vocabulary is large enough
            // that a blank panel reads as broken rather than as no match.
            <p className="px-1 py-2 text-muted-foreground text-xs">
              No emoji matches “{emojiQuery}”.
            </p>
          ) : (
            <div className="grid max-h-48 grid-cols-8 gap-0.5 overflow-y-auto">
              {emoji.map(({ shortcode, emoji: character }) => (
                <Button
                  key={shortcode}
                  type="button"
                  size="icon-sm"
                  variant="ghost"
                  // The shortcode is the name, because the character alone is
                  // what a screen reader would otherwise have to describe.
                  aria-label={shortcode}
                  title={`:${shortcode}:`}
                  onMouseDown={keepCaret}
                  onClick={() => run({ kind: "emoji", text: character })}
                >
                  <span aria-hidden="true" className="text-title leading-none">
                    {character}
                  </span>
                </Button>
              ))}
            </div>
          )}
        </fieldset>
      ) : null}

      {panel === "heading" ? (
        // A fieldset rather than a dialog: it takes no focus and traps none, so
        // announcing it as a dialog would promise a keyboard contract it does
        // not keep.
        <fieldset
          aria-label="Heading level"
          className="absolute top-full left-2 z-20 mt-1 flex gap-0.5 rounded-md border bg-popover p-1 shadow-md"
        >
          {HEADING_LEVELS.map((level) => (
            <Button
              key={level}
              type="button"
              size="icon-sm"
              variant="ghost"
              aria-label={`Heading ${level}`}
              onMouseDown={keepCaret}
              onClick={() => run({ kind: "heading", level })}
            >
              H{level}
            </Button>
          ))}
        </fieldset>
      ) : null}

      {panel === "table" ? (
        <fieldset
          aria-label="Insert table"
          className="absolute top-full left-2 z-20 mt-1 flex items-end gap-2 rounded-md border bg-popover p-2 shadow-md"
        >
          <div className="flex flex-col gap-1">
            <label className="text-meta text-muted-foreground" htmlFor={`${ids}-rows`}>
              Rows
            </label>
            <Input
              id={`${ids}-rows`}
              type="number"
              min={1}
              className="h-8 w-16"
              value={rows}
              onChange={(event) => setRows(Number(event.target.value))}
            />
          </div>
          <div className="flex flex-col gap-1">
            <label className="text-meta text-muted-foreground" htmlFor={`${ids}-columns`}>
              Columns
            </label>
            <Input
              id={`${ids}-columns`}
              type="number"
              min={1}
              className="h-8 w-16"
              value={columns}
              onChange={(event) => setColumns(Number(event.target.value))}
            />
          </div>
          <div className="flex items-center gap-1.5 pb-2">
            <input
              id={`${ids}-header`}
              type="checkbox"
              checked={header}
              onChange={(event) => setHeader(event.target.checked)}
            />
            <label className="text-meta text-muted-foreground" htmlFor={`${ids}-header`}>
              First row is a header
            </label>
          </div>
          <Button
            type="button"
            size="sm"
            variant="outline"
            onMouseDown={keepCaret}
            onClick={() => run({ kind: "table", rows, columns, header })}
          >
            Insert
          </Button>
        </fieldset>
      ) : null}
    </div>
  );
}
