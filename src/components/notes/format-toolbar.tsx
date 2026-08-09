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
  Italic,
  Link,
  List,
  ListOrdered,
  Quote,
  Strikethrough,
  Table,
} from "lucide-react";
import { type MouseEvent, useCallback, useId, useState } from "react";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import type { FormatAction } from "./editor/format-commands";

/** Which extra panel, if any, is open. Only ever one. */
type Panel = "heading" | "table" | null;

const HEADING_LEVELS = [1, 2, 3, 4, 5, 6] as const;

/** The straight-through buttons, in the order the toolbar shows them. */
const DIRECT: readonly { action: FormatAction; label: string; Icon: typeof Bold }[] = [
  { action: { kind: "bold" }, label: "Bold", Icon: Bold },
  { action: { kind: "italic" }, label: "Italic", Icon: Italic },
  { action: { kind: "strikethrough" }, label: "Strikethrough", Icon: Strikethrough },
  { action: { kind: "code" }, label: "Inline code", Icon: Code },
  { action: { kind: "bullet" }, label: "Bullet list", Icon: List },
  { action: { kind: "ordered" }, label: "Numbered list", Icon: ListOrdered },
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
        onClick={() => setPanel((open) => (open === "heading" ? null : "heading"))}
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
        onClick={() => setPanel((open) => (open === "table" ? null : "table"))}
      >
        <Table aria-hidden="true" />
      </Button>

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
            <label className="text-[11px] text-muted-foreground" htmlFor={`${ids}-rows`}>
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
            <label className="text-[11px] text-muted-foreground" htmlFor={`${ids}-columns`}>
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
            <label className="text-[11px] text-muted-foreground" htmlFor={`${ids}-header`}>
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
