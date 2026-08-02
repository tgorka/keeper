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
 * **Tags intersect.** Two chips mean "both", and removing one WIDENS the result
 * rather than replacing it. Rust evaluates that; this component only composes
 * the chip set and never inspects a row.
 *
 * **Nothing here is a navigation** (UX-DR41). Adding a chip that excludes the
 * open note leaves that note open and simply stops listing its row. The chips do
 * not animate in or out either: a filter change is a cut, because an animated
 * bar moves the target the user is reaching for.
 *
 * The Save-as-space button appears only once something beyond the scope is
 * active, because a filter worth keeping is one you built rather than one you
 * clicked once — and a filter you can build but not keep trains people not to
 * build filters.
 */
import { Search, X } from "lucide-react";
import { type KeyboardEvent, type Ref, useId } from "react";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { notesFiltersStore, scopeLabel, useNotesFiltersStore } from "@/lib/stores/notes-filters";

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
  const tags = useNotesFiltersStore((s) => s.tags);
  const text = useNotesFiltersStore((s) => s.text);
  const agentOnly = useNotesFiltersStore((s) => s.agentOnly);
  const pinnedOnly = useNotesFiltersStore((s) => s.pinnedOnly);
  const fieldId = useId();

  // "Beyond scope" is the trigger, not "any chip": scoping to Pinned is
  // navigation-shaped and saving it as a space would just duplicate the row that
  // is already in the sidebar.
  const savable = tags.length > 0 || agentOnly || pinnedOnly || text.trim() !== "";

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
        {tags.map((tag) => (
          <FilterChip
            key={tag}
            label={tag}
            clearLabel={`Clear tag ${tag} filter`}
            onClear={() => notesFiltersStore.getState().removeTag(tag)}
          />
        ))}
        {agentOnly && (
          <FilterChip
            label="Changed by agent"
            clearLabel="Clear changed-by-agent filter"
            onClear={() => notesFiltersStore.getState().setAgentOnly(false)}
          />
        )}
        {pinnedOnly && (
          <FilterChip
            label="Pinned only"
            clearLabel="Clear pinned-only filter"
            onClear={() => notesFiltersStore.getState().setPinnedOnly(false)}
          />
        )}
        {!agentOnly && (
          <Button
            type="button"
            variant="ghost"
            size="xs"
            className="shrink-0 text-muted-foreground"
            onClick={() => notesFiltersStore.getState().setAgentOnly(true)}
          >
            Changed by agent
          </Button>
        )}
        {!pinnedOnly && (
          <Button
            type="button"
            variant="ghost"
            size="xs"
            className="shrink-0 text-muted-foreground"
            onClick={() => notesFiltersStore.getState().setPinnedOnly(true)}
          >
            Pinned only
          </Button>
        )}
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
