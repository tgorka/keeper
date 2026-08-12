/**
 * The filter chip bar and the search field (Epic 37, Story 37.3, FR-103/FR-104,
 * FR-118, UX-DR37, UX-DR41).
 *
 * Chips beat a filter panel because they are simultaneously the control and the
 * state: what is filtering you is what is on screen, dismissible in place, in a
 * fixed order so the bar's shape is learnable. That fixed order — scope, tag
 * chips, origin, pinned — is the reason removing a chip is a muscle movement
 * rather than a search.
 *
 * **Tag terms intersect, and a tag chip has three states** (Story 43.3). Off,
 * include, exclude — cycled by pressing the chip, and told apart on sight by a
 * `+`/`−` sign and a colour rather than by a tooltip. A control whose state you
 * have to point at to discover is a control with one state. Rust evaluates the
 * terms; this component only composes them and never inspects a row.
 *
 * **Nothing here is a navigation** (UX-DR41). Adding a chip that excludes the
 * open note leaves that note open and simply stops listing its row. The chips do
 * not animate in or out either: a filter change is a cut, because an animated
 * bar moves the target the user is reaching for.
 *
 * **Origin and pinned are toggles, not chips that appear** (Story 49). They
 * used to be a button that could only turn the filter ON, unmounted the moment
 * it did, and a chip elsewhere in the bar that was the only way back. The chip
 * is gone: one persistent control each, `aria-pressed`, and the chip's own
 * `bg-accent` as the pressed paint — so the bar's fixed order holds still
 * whatever is on, which is the whole reason the order was fixed.
 *
 * **The bar can now make a tag chip, and only ever an existing tag** (Story
 * 44.13). Until this the only way to raise a tag chip was to find the tag in
 * the sidebar tree, which is a fine way to browse and a poor way to reach a tag
 * you can already name. The chooser refuses to create: a chip for a tag no note
 * carries produces an empty list with no explanation, so it says there is no
 * such tag instead. The space editor, which authors a filter rather than
 * running one, takes the opposite setting for the reason stated there.
 *
 * The Save-as-space button appears only once something beyond the scope is
 * active, because a filter worth keeping is one you built rather than one you
 * clicked once — and a filter you can build but not keep trains people not to
 * build filters.
 */
import { Minus, Plus, Search, X } from "lucide-react";
import {
  type KeyboardEvent,
  type Ref,
  useCallback,
  useEffect,
  useId,
  useRef,
  useState,
} from "react";
import { TagCombobox } from "@/components/notes/tag-combobox";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { tagsVocabulary } from "@/lib/ipc/client";
import {
  notesFiltersStore,
  scopeLabel,
  type TagChip,
  useNotesFiltersStore,
} from "@/lib/stores/notes-filters";
import { cn } from "@/lib/utils";

/** The chip bar's own tag chooser (Story 44.13). */
export const ADD_TAG_FILTER = "Add a tag filter";

/** The header caption under the search field, kept verbatim. */
export const NOTES_SEARCH_POSTURE = "Searching the files, not an index";

/** The search field's placeholder. */
export const NOTES_SEARCH_PLACEHOLDER = "Search this vault";

/** One dismissible chip. Every chip in this bar can be removed in place. */
function FilterChip({
  label,
  clearLabel,
  onClear,
}: {
  label: string;
  clearLabel: string;
  onClear: () => void;
}) {
  return (
    <span
      data-slot="filter-chip"
      className="inline-flex shrink-0 items-center gap-1 rounded-full bg-accent px-2 py-0.5 text-accent-foreground text-xs"
    >
      {label}
      <button
        type="button"
        aria-label={clearLabel}
        onClick={onClear}
        className="rounded-full outline-none hover:bg-background/40 focus-visible:ring-2 focus-visible:ring-ring"
      >
        <X aria-hidden="true" className="size-3" />
      </button>
    </span>
  );
}

/**
 * A tag chip: the sign says which of the three states it is in, the body cycles
 * to the next one, and the `×` takes it straight to off (FR-148, UX-DR54).
 *
 * Three things carry the state, deliberately redundantly, because each of them
 * fails for someone: the `+`/`−` glyph (invisible to a screen reader, so it is
 * `aria-hidden`), the background colour (invisible to a colour-blind user, and
 * to anyone in a hurry), and the accessible name, which spells the state and
 * what pressing will do. None of them is a tooltip: a chip whose state you have
 * to hover to learn has, in practice, one state.
 *
 * `aria-pressed` is not used. It has two values and this control has three, and
 * a toggle button that reports `pressed=false` while actively excluding notes is
 * worse than no ARIA state at all — so the state lives in the name, where it can
 * be said exactly.
 *
 * Exported, and told what to do rather than reaching for the store, because the
 * space editor (Story 43.4) renders the same control over a draft term list that
 * is deliberately NOT the live filter — editing a space must not re-filter the
 * list behind the dialog. A second copy of the chip would be a second copy of
 * the three rules above, and the first one to rot would be the accessible name,
 * which nobody looks at.
 */
export function TagFilterChip({
  chip,
  onCycle,
  onRemove,
}: {
  chip: TagChip;
  onCycle: (tag: string) => void;
  onRemove: (tag: string) => void;
}) {
  const excluded = chip.term === "exclude";
  const Sign = excluded ? Minus : Plus;
  return (
    <span
      data-slot="filter-chip"
      data-tag-term={chip.term}
      className={cn(
        "inline-flex shrink-0 items-center gap-1 rounded-full px-2 py-0.5 text-xs",
        excluded
          ? "bg-destructive/15 text-destructive line-through decoration-destructive/60"
          : "bg-accent text-accent-foreground",
      )}
    >
      <button
        type="button"
        aria-label={
          excluded
            ? `Tag ${chip.tag}: excluded. Stop filtering by it.`
            : `Tag ${chip.tag}: included. Exclude it instead.`
        }
        onClick={() => onCycle(chip.tag)}
        className="inline-flex items-center gap-0.5 rounded-full outline-none focus-visible:ring-2 focus-visible:ring-ring"
      >
        <Sign aria-hidden="true" className="size-3" />
        {chip.tag}
      </button>
      <button
        type="button"
        aria-label={`Clear tag ${chip.tag} filter`}
        onClick={() => onRemove(chip.tag)}
        className="rounded-full outline-none hover:bg-background/40 focus-visible:ring-2 focus-visible:ring-ring"
      >
        <X aria-hidden="true" className="size-3" />
      </button>
    </span>
  );
}

export function NoteFilterBar({
  onSaveAsSpace,
  searchRef,
}: {
  /** Promote the current chip set to a space note (`⌘⇧S`, FR-105). */
  onSaveAsSpace: () => void;
  /** So `⌘F` and the palette's Search Notes can put the caret in the field. */
  searchRef?: Ref<HTMLInputElement>;
}) {
  const scope = useNotesFiltersStore((s) => s.scope);
  const tagTerms = useNotesFiltersStore((s) => s.tagTerms);
  const text = useNotesFiltersStore((s) => s.text);
  const agentOnly = useNotesFiltersStore((s) => s.agentOnly);
  const pinnedOnly = useNotesFiltersStore((s) => s.pinnedOnly);
  const fieldId = useId();
  const [adding, setAdding] = useState(false);
  const [vocabulary, setVocabulary] = useState<readonly string[]>([]);
  const addRef = useRef<HTMLButtonElement>(null);

  // Read when the chooser opens rather than when the bar mounts: the bar is on
  // screen for the whole session and the vocabulary is only wanted for the few
  // seconds someone is picking from it.
  useEffect(() => {
    if (!adding) {
      return;
    }
    let cancelled = false;
    void tagsVocabulary()
      .then((vm) => {
        if (!cancelled) {
          setVocabulary(vm.entries.map((entry) => entry.path));
        }
      })
      .catch(() => {
        // Nothing to browse and nothing to type: this chooser cannot create a
        // tag, so an unreadable vocabulary leaves it saying so rather than
        // pretending a filter could be built out of it.
        if (!cancelled) {
          setVocabulary([]);
        }
      });
    return () => {
      cancelled = true;
    };
  }, [adding]);

  // A stable ref callback, so the field takes focus once when the chooser opens
  // and not again on every render of the bar behind it.
  const focusChooser = useCallback((node: HTMLInputElement | null) => {
    node?.focus();
  }, []);

  function closeChooser(): void {
    setAdding(false);
    addRef.current?.focus();
  }

  // "Beyond scope" is the trigger, not "any chip": scoping to Pinned is
  // navigation-shaped and saving it as a space would just duplicate the row that
  // is already in the sidebar.
  const savable = tagTerms.length > 0 || agentOnly || pinnedOnly || text.trim() !== "";

  const onSearchKeyDown = (event: KeyboardEvent<HTMLInputElement>) => {
    if (event.key !== "Escape") {
      return;
    }
    // Esc walks up: the query first, then the bar, one chip per press. Clearing
    // everything at once would take away the one chip the user meant to keep.
    event.preventDefault();
    event.stopPropagation();
    if (text !== "") {
      notesFiltersStore.getState().setText("");
      return;
    }
    notesFiltersStore.getState().dropLastChip();
  };

  return (
    <div className="flex shrink-0 flex-col gap-2 border-border border-b px-3 py-2">
      <div data-slot="filter-chip-bar" className="flex flex-wrap items-center gap-1">
        {scope.kind !== "all" && (
          <FilterChip
            label={scopeLabel(scope)}
            clearLabel={`Clear ${scopeLabel(scope)} scope`}
            onClear={() => notesFiltersStore.getState().setScope(scope)}
          />
        )}
        {tagTerms.map((chip) => (
          <TagFilterChip
            key={chip.tag}
            chip={chip}
            onCycle={(tag) => notesFiltersStore.getState().cycleTag(tag)}
            onRemove={(tag) => notesFiltersStore.getState().removeTag(tag)}
          />
        ))}
        {/* Sits with the tag chips, because it makes one. The bar's order —
            scope, tags, origin, pinned — is what makes removing a chip a
            muscle movement, and a chooser filed anywhere else would be a
            second place to look for the tags. */}
        <Button
          ref={addRef}
          type="button"
          variant="ghost"
          size="xs"
          className="shrink-0 text-muted-foreground"
          aria-expanded={adding}
          onClick={() => (adding ? closeChooser() : setAdding(true))}
        >
          <Plus aria-hidden="true" className="size-3" />
          {ADD_TAG_FILTER}
        </Button>
        {/* Story 49: two real toggles, drawn as two.

            These were one-way controls. The button rendered only while the
            filter was OFF and could only turn it on; turning it off happened on
            a different control — a chip that took its place, with its own `✕`,
            named something else. So the bar had two ways to say one fact, the
            control moved on every press, and the only way back was to find the
            thing that had replaced the thing you pressed. A toggle whose
            off-switch is somewhere else is not a toggle.

            One persistent control each, in one place, with one name in both
            states — `aria-pressed` says which state it is in, and the pressed
            paint is the chip's own `bg-accent`, so on it looks exactly like the
            chip it replaces and off it looks like the control that makes one.
            The name does not change with the state: a control renamed by its own
            press is one that speech input cannot ask for twice (WCAG 2.5.3), and
            `aria-pressed` already carries the difference exactly.

            The Esc walk (`dropLastChip`) still clears them in the same order —
            it reads the store, not the chips, and the store did not change. */}
        <Button
          type="button"
          variant="ghost"
          size="xs"
          aria-pressed={agentOnly}
          className="shrink-0 text-muted-foreground aria-pressed:bg-accent aria-pressed:text-accent-foreground"
          onClick={() => notesFiltersStore.getState().setAgentOnly(!agentOnly)}
        >
          Changed by agent
        </Button>
        <Button
          type="button"
          variant="ghost"
          size="xs"
          aria-pressed={pinnedOnly}
          className="shrink-0 text-muted-foreground aria-pressed:bg-accent aria-pressed:text-accent-foreground"
          onClick={() => notesFiltersStore.getState().setPinnedOnly(!pinnedOnly)}
        >
          Pinned only
        </Button>
        {savable && (
          <Button
            type="button"
            variant="ghost"
            size="xs"
            className="ml-auto shrink-0"
            onClick={onSaveAsSpace}
          >
            Save as space
          </Button>
        )}
      </div>
      {adding && (
        <TagCombobox
          label={ADD_TAG_FILTER}
          placeholder="Type or browse"
          vocabulary={vocabulary}
          chosen={tagTerms.map((chip) => chip.tag)}
          inputRef={focusChooser}
          onChoose={(tag) => notesFiltersStore.getState().setTagTerm(tag, "include")}
          onDismiss={closeChooser}
        />
      )}
      <div className="flex items-center gap-1.5">
        <Search aria-hidden="true" className="size-4 shrink-0 text-muted-foreground" />
        <Input
          ref={searchRef}
          id={fieldId}
          type="search"
          aria-label={NOTES_SEARCH_PLACEHOLDER}
          placeholder={NOTES_SEARCH_PLACEHOLDER}
          className="h-8"
          value={text}
          onChange={(event) => notesFiltersStore.getState().setText(event.target.value)}
          onKeyDown={onSearchKeyDown}
        />
      </div>
      {/* The same posture the archive search takes, and true in the same way:
          there is no index to be stale, because the scan reads the files. */}
      <p className="text-muted-foreground text-xs">{NOTES_SEARCH_POSTURE}</p>
    </div>
  );
}
