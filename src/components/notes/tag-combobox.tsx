/**
 * The tag chooser (Story 44.13, FR-169, UX-DR61).
 *
 * **A field AND a list, not a field OR a list.** Every tag chooser keeper had
 * was one or the other: the space editor was a `<select>` you could browse but
 * not type into, and the recording card was a text field with a `<datalist>`
 * you could type into but could not read until you had typed. Both halves are
 * load-bearing — you type when you know the tag's name and you browse when you
 * are asking the vault what it has — so this control renders the list under the
 * field and narrows it as you type. There is no popup, and no press that opens
 * one, because a list you have to open is a list nobody browses.
 *
 * **Story 53.2 narrows that sentence; it does not reverse it.** Browsing still
 * costs no deliberate press — focus on the field opens the list, so does typing,
 * so does an arrow key. What the control gained is a notion of *done choosing*:
 * focus leaving the whole control, a press outside it, or Escape where a
 * dismissable layer above has not already claimed that key. After that the
 * list is `hidden` rather than unmounted — the `FoldSection` idiom
 * (`sidebar-group.tsx:215`, and the reason at `:32-37`) — so it stops claiming
 * height in the column while remaining the same list, in the same order, one
 * focus away. That is what makes this one component the fix on all five
 * surfaces: the two space editors (`space-editor.tsx`, `session-space-editor.tsx`)
 * mount it unconditionally with no `onDismiss` and so had no close path at all,
 * and they need none of their own now, because the state lives here.
 *
 * **What matches is not this file's decision.** `components/tags/tag-match.ts`
 * owns it, and the editor's `#` popup asks the same function, so the two
 * surfaces cannot drift into two answers for "does `cl/ac` find
 * `client/acme`". This file owns only what happens to a match once it is one.
 *
 * **Creating is the caller's permission, and refusing says why.** A filter chip
 * can only narrow to tags that exist — filtering by a tag no note carries is an
 * empty list with no explanation — so it passes `allowCreate={false}` and the
 * control says there is no such tag. A surface that WRITES a tag passes
 * `allowCreate`, and the typed text goes out verbatim: what a tag means is
 * settled in `keeper-core/src/notes/tags.rs`, at the boundary, and a control
 * that folded case here would be the second place that decides.
 *
 * **Focus never leaves the field.** Arrowing moves `aria-activedescendant`
 * rather than the caret, and choosing an option — by key or by click — puts the
 * caret back with the query cleared, because tagging is a thing people do
 * several times in a row and a control that drops focus after each one makes
 * the second tag cost a reach for the mouse.
 */
import {
  type KeyboardEvent,
  type ReactNode,
  type Ref,
  useEffect,
  useId,
  useRef,
  useState,
} from "react";
import { matchTags, namesTag } from "@/components/tags/tag-match";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { cn } from "@/lib/utils";

/** What the list says when the query matches nothing and creating is refused. */
export function tagComboboxNoMatch(query: string): string {
  return `No tag matches "${query}". This chooses from the tags your vault already has.`;
}

/** What the list says when the query names a tag that is already chosen. */
export function tagComboboxAlreadyChosen(query: string): string {
  return `"${query}" is already on this list.`;
}

/** What the list says with an empty vault and an empty query. */
export const TAG_COMBOBOX_NO_VOCABULARY = "This vault has no tags yet.";

/** The offer to make a tag the vocabulary does not have, where that is allowed. */
export function tagComboboxCreate(query: string): string {
  return `Create tag "${query}"`;
}

/** One row of the list: a tag the vocabulary has, or the offer to create one. */
type Row = { readonly kind: "tag" | "create"; readonly tag: string };

export function TagCombobox({
  label,
  vocabulary,
  chosen = [],
  allowCreate = false,
  placeholder,
  onChoose,
  onDismiss,
  inputRef,
}: {
  /** The visible label; also the accessible name of the field and of the list. */
  label: string;
  /** Every tag the vault knows, as full paths, in the order it wants browsing. */
  vocabulary: readonly string[];
  /** Tags this chooser has already taken. They leave the list and cannot be
   *  re-created, so the control never offers a second copy of one. */
  chosen?: readonly string[];
  /** Whether a tag outside the vocabulary may be created by typing it. */
  allowCreate?: boolean;
  placeholder?: string;
  /** Receives the tag exactly as the vocabulary spells it, or — for a created
   *  tag — exactly as the user typed it. Never a form this control invented. */
  onChoose: (tag: string) => void;
  /** Escape on an empty query. Absent, Escape only clears the query, which is
   *  what an inline chooser with nowhere to go should do. */
  onDismiss?: () => void;
  inputRef?: Ref<HTMLInputElement>;
}) {
  const fieldId = useId();
  const listId = `${fieldId}-list`;
  const [query, setQuery] = useState("");
  const [active, setActive] = useState(0);
  // Story 53.2. Closed until the user is at the field: the surfaces that mount
  // this control unconditionally would otherwise claim a list's height before
  // anybody has come to read it. Every way INTO the control opens it — focus,
  // typing, an arrow key — so there is still no press whose job is "open".
  const [open, setOpen] = useState(false);
  const root = useRef<HTMLDivElement>(null);
  // True from a pointer going down anywhere until it comes back up. The
  // focus-out that a press causes must NOT close the list mid-press: this list
  // sits above a dialog's Save button, and hiding it between mousedown and
  // mouseup moves that button out from under the cursor, so the click the user
  // meant never lands. While a press is in flight the close is left to the
  // `click` below, by which time the browser has already settled what was hit.
  const pressing = useRef(false);

  const typed = query.trim();

  // Recomputed every render rather than memoised: every caller builds `chosen`
  // by mapping its own chip list, so the dependency changes identity on each
  // render and a `useMemo` here would miss every time while looking like it
  // did not. One pass over the vocabulary is what this costs, honestly.
  const taken = new Set(chosen);
  const rows: Row[] = matchTags(query, vocabulary)
    .filter((tag) => !taken.has(tag))
    .map((tag) => ({ kind: "tag", tag }) as const);
  // The offer to create comes last, never first: it is the answer to "the vault
  // does not have this", and putting it above the matches would make Enter
  // create a near-duplicate of the tag the user was one keystroke from.
  if (allowCreate && typed !== "" && !namesTag(typed, vocabulary) && !namesTag(typed, chosen)) {
    rows.push({ kind: "create", tag: typed });
  }

  // A narrowing list must not leave the highlight pointing past its end, or
  // Enter would choose nothing while a row looks selected.
  const at = rows.length === 0 ? 0 : Math.min(active, rows.length - 1);

  // The highlighted row has to stay on screen while the arrow keys walk a list
  // taller than its box; without this the selection is real but invisible,
  // which reads as the arrow keys doing nothing.
  useEffect(() => {
    document.getElementById(`${fieldId}-o${at}`)?.scrollIntoView?.({ block: "nearest" });
  }, [at, fieldId]);

  // The outside-press layer, subscribed only while there is something to close.
  // Capture, so a handler that stops propagation on the way up cannot leave the
  // list open over a surface the user has moved on from.
  useEffect(() => {
    if (!open) {
      return;
    }
    const down = (): void => {
      pressing.current = true;
    };
    const up = (): void => {
      pressing.current = false;
    };
    const clicked = (event: MouseEvent): void => {
      if (root.current?.contains(event.target as Node) !== true) {
        setOpen(false);
      }
    };
    document.addEventListener("pointerdown", down, true);
    document.addEventListener("pointerup", up, true);
    document.addEventListener("pointercancel", up, true);
    document.addEventListener("click", clicked, true);
    return () => {
      document.removeEventListener("pointerdown", down, true);
      document.removeEventListener("pointerup", up, true);
      document.removeEventListener("pointercancel", up, true);
      document.removeEventListener("click", clicked, true);
      // A press that ended with this layer gone must not leave the next
      // focus-out believing a button is still held down.
      pressing.current = false;
    };
  }, [open]);

  function choose(row: Row | undefined): void {
    if (row === undefined) {
      return;
    }
    // Choosing is not "done choosing" (Story 53.2). Tagging happens in runs and
    // the caret stays put, so the list stays up for the next one.
    onChoose(row.tag);
    setQuery("");
    setActive(0);
  }

  function onKeyDown(event: KeyboardEvent<HTMLInputElement>): void {
    if (event.key === "ArrowDown" || event.key === "ArrowUp") {
      event.preventDefault();
      // An arrow on a folded list is a request to browse it, so it opens and
      // then moves the highlight — one keystroke, not two. The highlight is not
      // reset on opening: row 0 is where it already was.
      setOpen(true);
      if (rows.length === 0) {
        return;
      }
      const step = event.key === "ArrowDown" ? 1 : rows.length - 1;
      setActive((current) => (Math.min(current, rows.length - 1) + step) % rows.length);
      return;
    }
    if (event.key === "Enter") {
      // Swallowed even with nothing to choose: this control is mounted inside a
      // dialog whose default button would otherwise save the space the user was
      // still describing.
      event.preventDefault();
      if (!open) {
        // Nothing is highlighted while the list is folded away, and committing a
        // row nobody can see is not what Enter means. A keystroke or an arrow
        // brings the list back first.
        return;
      }
      choose(rows[at]);
      return;
    }
    if (event.key === "Escape") {
      // Stopped here rather than allowed to bubble: the filter bar's Esc walks
      // the chip stack down one chip per press, and dismissing this chooser
      // must not also throw away the chip behind it.
      event.preventDefault();
      event.stopPropagation();
      if (query !== "") {
        setQuery("");
        setActive(0);
        return;
      }
      // Story 53.2: on an empty query Escape folds the list away, and then the
      // host's dismiss runs if it has one. It is not the only fold, and on a
      // surface inside a Radix dialog it is not the reachable one — that
      // library's dismissable layer claims Escape at the document in the
      // CAPTURE phase and closes the dialog before this handler runs, which is
      // those dialogs' own older decision. The folds that always work are focus
      // leaving and a press outside, which is why they exist.
      setOpen(false);
      onDismiss?.();
    }
  }

  let empty: ReactNode = null;
  if (rows.length === 0) {
    if (typed === "") {
      empty = TAG_COMBOBOX_NO_VOCABULARY;
    } else if (namesTag(typed, chosen)) {
      empty = tagComboboxAlreadyChosen(typed);
    } else {
      empty = tagComboboxNoMatch(typed);
    }
  }

  return (
    <div ref={root} className="flex flex-col gap-1.5">
      <Label htmlFor={fieldId}>{label}</Label>
      <Input
        ref={inputRef}
        id={fieldId}
        role="combobox"
        // The real state, not a literal (Story 53.2): the list below is hidden
        // when the chooser is folded, and a combobox that reported itself
        // expanded over a hidden listbox would tell a screen-reader user the
        // opposite of what is there — which is exactly why this was `true`
        // while the list was permanent.
        aria-expanded={open}
        aria-controls={listId}
        aria-autocomplete="list"
        aria-activedescendant={!open || rows.length === 0 ? undefined : `${fieldId}-o${at}`}
        autoComplete="off"
        className="h-8"
        placeholder={placeholder}
        value={query}
        onChange={(event) => {
          setQuery(event.target.value);
          setActive(0);
          setOpen(true);
        }}
        onKeyDown={onKeyDown}
        // `focus-within` semantics, on the only focusable thing this control
        // has: the rows are `tabIndex={-1}` and prevent the mousedown default,
        // so the caret never leaves the field for one. The guard is written
        // against the whole control anyway — focus going anywhere INSIDE it is
        // not the user having stopped choosing, and a row that ever becomes a
        // tab stop must not fold the list on the way in.
        onFocus={() => setOpen(true)}
        onBlur={(event) => {
          if (root.current?.contains(event.relatedTarget) === true) {
            return;
          }
          // A press is in flight, so this focus-out is the browser's mousedown
          // default and not a decision. Folding now would move whatever is
          // being pressed out from under the cursor before the click lands; the
          // document `click` layer above folds it once that press has arrived.
          if (pressing.current) {
            return;
          }
          setOpen(false);
        }}
      />
      {/* A `div` rather than a `ul`: `role="listbox"` on a list element is a
          role that overrides the element's own semantics, which is a lint the
          repo takes seriously and an inconsistency a screen reader has to
          resolve. The listbox is deliberately unnamed — the field owns the
          accessible name and points here with `aria-controls`, and repeating
          the name would give it two targets.

          `hidden` rather than unmounted while folded, the way a folded
          `FoldSection` body is (`sidebar-group.tsx:215`): the rows stay built,
          out of the tab order and out of the accessibility tree, and
          `display: none` is what takes their height out of the column instead
          of leaving a gap where the list was. */}
      <div
        id={listId}
        role="listbox"
        hidden={!open}
        className="max-h-48 overflow-y-auto rounded-md border border-border"
      >
        {rows.map((row, index) => (
          // biome-ignore lint/a11y/useKeyWithClickEvents: an option in the aria-activedescendant combobox pattern is not itself a keyboard target — the field keeps focus, and ArrowUp/ArrowDown/Enter on the field are this row's keyboard equivalent, asserted in this component's suite.
          <div
            key={`${row.kind}:${row.tag}`}
            id={`${fieldId}-o${index}`}
            role="option"
            // Not in the tab order, and deliberately: the ARIA pattern moves a
            // highlight, not the caret. `-1` is what makes the row programmatic
            // rather than a second stop between the field and the next control.
            tabIndex={-1}
            aria-selected={index === at}
            data-active={index === at}
            data-slot={row.kind === "create" ? "tag-option-create" : "tag-option"}
            className={cn(
              "cursor-pointer px-2 py-1 text-sm",
              index === at && "bg-accent text-accent-foreground",
              row.kind === "create" && "text-muted-foreground italic",
            )}
            // The caret stays put through the press: mousing at a list must not
            // cost the keyboard user their place in the field.
            onMouseDown={(event) => event.preventDefault()}
            onClick={() => choose(row)}
          >
            {row.kind === "create" ? tagComboboxCreate(row.tag) : row.tag}
          </div>
        ))}
        {empty !== null && <p className="px-2 py-1 text-muted-foreground text-sm">{empty}</p>}
      </div>
    </div>
  );
}
