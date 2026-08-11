/**
 * The tag chooser (Story 44.13, FR-169, UX-DR61).
 *
 * **A field AND a list, not a field OR a list.** Every tag chooser keeper had
 * was one or the other: the space editor was a `<select>` you could browse but
 * not type into, and the recording card was a text field with a `<datalist>`
 * you could type into but could not read until you had typed. Both halves are
 * load-bearing — you type when you know the tag's name and you browse when you
 * are asking the vault what it has — so this control renders the list
 * permanently under the field and narrows it as you type. There is no popup and
 * no expanded/collapsed state, because a list you have to open is a list nobody
 * browses.
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
import { type KeyboardEvent, type ReactNode, type Ref, useEffect, useId, useState } from "react";
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

  function choose(row: Row | undefined): void {
    if (row === undefined) {
      return;
    }
    onChoose(row.tag);
    setQuery("");
    setActive(0);
  }

  function onKeyDown(event: KeyboardEvent<HTMLInputElement>): void {
    if (event.key === "ArrowDown" || event.key === "ArrowUp") {
      event.preventDefault();
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
    <div className="flex flex-col gap-1.5">
      <Label htmlFor={fieldId}>{label}</Label>
      <Input
        ref={inputRef}
        id={fieldId}
        role="combobox"
        // Always true, and honestly so: the list below is always rendered. A
        // combobox that reports itself collapsed while its options are on
        // screen tells a screen-reader user the opposite of what is there.
        aria-expanded
        aria-controls={listId}
        aria-autocomplete="list"
        aria-activedescendant={rows.length === 0 ? undefined : `${fieldId}-o${at}`}
        autoComplete="off"
        className="h-8"
        placeholder={placeholder}
        value={query}
        onChange={(event) => {
          setQuery(event.target.value);
          setActive(0);
        }}
        onKeyDown={onKeyDown}
      />
      {/* A `div` rather than a `ul`: `role="listbox"` on a list element is a
          role that overrides the element's own semantics, which is a lint the
          repo takes seriously and an inconsistency a screen reader has to
          resolve. The listbox is deliberately unnamed — the field owns the
          accessible name and points here with `aria-controls`, and repeating
          the name would give it two targets. */}
      <div
        id={listId}
        role="listbox"
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
