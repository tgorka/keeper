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
import {
  Bookmark,
  Bot,
  Eye,
  EyeOff,
  Loader,
  Minus,
  Pin,
  Plus,
  Search,
  Sparkles,
  X,
} from "lucide-react";
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
import { IconHint } from "@/components/ui/tooltip";
import { tagsVocabulary } from "@/lib/ipc/client";
import {
  notesFiltersStore,
  scopeLabel,
  type TagChip,
  useNotesFiltersStore,
} from "@/lib/stores/notes-filters";
import { useNotesListStore } from "@/lib/stores/notes-list";
import { useNotesSearchState } from "@/lib/stores/notes-search-state";
import { useNotesVaultsStore } from "@/lib/stores/notes-vaults";
import { settingsUiStore } from "@/lib/stores/settings-ui";
import { cn } from "@/lib/utils";

/** The chip bar's own tag chooser (Story 44.13). */
export const ADD_TAG_FILTER = "Add a tag filter";

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
      <IconHint label={clearLabel}>
        <button
          type="button"
          aria-label={clearLabel}
          onClick={onClear}
          className="rounded-full outline-none hover:bg-background/40 focus-visible:ring-2 focus-visible:ring-ring"
        >
          <X aria-hidden="true" className="size-3" />
        </button>
      </IconHint>
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
      <IconHint label={`Clear tag ${chip.tag} filter`}>
        <button
          type="button"
          aria-label={`Clear tag ${chip.tag} filter`}
          onClick={() => onRemove(chip.tag)}
          className="rounded-full outline-none hover:bg-background/40 focus-visible:ring-2 focus-visible:ring-ring"
        >
          <X aria-hidden="true" className="size-3" />
        </button>
      </IconHint>
    </span>
  );
}

export function NoteFilterBar({
  onSaveAsSpace,
  searchRef,
  phone = false,
  onHideServiceFilesChange,
}: {
  /** Promote the current chip set to a space note (`⌘⇧S`, FR-105). */
  onSaveAsSpace: () => void;
  /** So `⌘F` and the palette's Search Notes can put the caret in the field. */
  searchRef?: Ref<HTMLInputElement>;
  phone?: boolean;
  onHideServiceFilesChange?: (hidden: boolean) => void;
}) {
  const scope = useNotesFiltersStore((s) => s.scope);
  const tagTerms = useNotesFiltersStore((s) => s.tagTerms);
  const text = useNotesFiltersStore((s) => s.text);
  const agentOnly = useNotesFiltersStore((s) => s.agentOnly);
  const pinnedOnly = useNotesFiltersStore((s) => s.pinnedOnly);
  const hideServiceFiles = useNotesFiltersStore((s) => s.hideServiceFiles);
  const vaultId = useNotesVaultsStore((s) => s.activeVaultId);
  const searchState = useNotesSearchState((s) => (vaultId ? s.byVault[vaultId] : undefined));
  const searchError = useNotesListStore((s) => s.searchError);
  const fieldId = useId();
  const [adding, setAdding] = useState(false);
  const [vocabulary, setVocabulary] = useState<readonly string[]>([]);
  const addRef = useRef<HTMLButtonElement>(null);
  const inputRef = useRef<HTMLInputElement | null>(null);
  const settingsObserver = useRef<MutationObserver | null>(null);
  useEffect(() => {
    const unsubscribe = settingsUiStore.subscribe((state) => {
      if (!state.settingsOpen) settingsObserver.current?.disconnect();
    });
    return () => {
      unsubscribe();
      settingsObserver.current?.disconnect();
    };
  }, []);
  const attachSearchRef = useCallback(
    (node: HTMLInputElement | null) => {
      inputRef.current = node;
      if (typeof searchRef === "function") searchRef(node);
      else if (searchRef) searchRef.current = node;
    },
    [searchRef],
  );

  const indexing = searchState?.phase === "indexing";
  const meaning = searchState?.phase === "meaning";
  const Glyph = indexing ? Loader : meaning ? Sparkles : Search;
  const mode = indexing ? "Indexing" : meaning ? "Words + meaning" : "Words";
  const settingsLabel = "Meaning is off — choose an embedding model in Settings";
  const meaningOff =
    searchState?.phase === "words" &&
    searchState.model === "" &&
    (searchState.sentence === "" || searchState.sentence === settingsLabel);
  const progress = indexing
    ? `Indexing ${searchState.indexed.toLocaleString()}${searchState.total > 0 ? `/${searchState.total.toLocaleString()}` : ""} notes`
    : searchState?.phase === "words" && searchState.embeddable > searchState.embedded
      ? `Indexing meaning ${searchState.embedded.toLocaleString()}/${searchState.embeddable.toLocaleString()} chunks`
      : "";
  const failure = text.trim() ? searchError : null;
  const status = failure || searchState?.sentence || progress || (meaningOff ? settingsLabel : "");
  const target = phone ? "size-11" : "size-6";
  const iconClass = cn(
    target,
    "shrink-0 p-0 text-muted-foreground aria-pressed:bg-accent aria-pressed:text-accent-foreground",
  );

  function openSearchSettings(): void {
    settingsObserver.current?.disconnect();
    const focus = () => {
      const target = document.getElementById("notes-embedding-model");
      if (!target) return false;
      target.scrollIntoView({ block: "center" });
      target.focus();
      return true;
    };
    settingsUiStore.getState().setSettingsOpen(true);
    if (!focus()) {
      settingsObserver.current = new MutationObserver(() => {
        if (focus()) settingsObserver.current?.disconnect();
      });
      settingsObserver.current.observe(document.body, { childList: true, subtree: true });
    }
  }

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
    <div
      data-slot="note-filter-bar"
      data-tier={phone ? "phone" : "desktop"}
      className={cn(
        "flex min-w-0 shrink-0 flex-col gap-2 border-border border-b py-2",
        phone ? "px-2" : "px-3",
      )}
    >
      <div
        data-slot="filter-chip-bar"
        className={
          phone
            ? "grid min-w-0 grid-cols-[minmax(44px,1fr)_176px]"
            : "flex min-w-0 items-start gap-1"
        }
        style={
          phone ? { gridTemplateAreas: "'scope scope' 'chooser actions' 'tags tags'" } : undefined
        }
      >
        <div
          data-slot="filter-tag-lane"
          className={
            phone
              ? "contents"
              : "flex max-h-40 min-w-0 flex-1 flex-wrap items-start gap-1 overflow-y-auto"
          }
        >
          {scope.kind !== "all" && (
            <div
              style={phone ? { gridArea: "scope" } : undefined}
              className={cn(
                "min-w-0 max-w-full [&>[data-slot=filter-chip]]:relative [&>[data-slot=filter-chip]]:block [&>[data-slot=filter-chip]]:truncate [&>[data-slot=filter-chip]>button]:absolute [&>[data-slot=filter-chip]>button]:top-0 [&>[data-slot=filter-chip]>button]:right-0 [&_svg]:mx-auto",
                phone
                  ? "mb-1 [&>[data-slot=filter-chip]]:h-11 [&>[data-slot=filter-chip]]:pr-12 [&>[data-slot=filter-chip]]:leading-10 [&_button]:size-11"
                  : "[&>[data-slot=filter-chip]]:h-6 [&>[data-slot=filter-chip]]:pr-7 [&_button]:size-6",
              )}
            >
              <FilterChip
                label={scopeLabel(scope)}
                clearLabel={`Clear ${scopeLabel(scope)} scope`}
                onClear={() => notesFiltersStore.getState().setScope(scope)}
              />
            </div>
          )}
          <IconHint label={ADD_TAG_FILTER}>
            <Button
              ref={addRef}
              type="button"
              variant="ghost"
              size={phone ? "icon" : "icon-xs"}
              style={phone ? { gridArea: "chooser" } : undefined}
              className={iconClass}
              aria-label={ADD_TAG_FILTER}
              aria-expanded={adding}
              onClick={() => (adding ? closeChooser() : setAdding(true))}
            >
              <Plus aria-hidden="true" className="size-4" />
            </Button>
          </IconHint>
          <div
            data-slot="filter-tags"
            style={phone ? { gridArea: "tags" } : undefined}
            className={cn(
              phone ? "flex max-h-40 min-w-0 flex-wrap gap-1 overflow-y-auto" : "contents",
              "[&>[data-slot=filter-chip]]:min-w-0 [&>[data-slot=filter-chip]]:max-w-full [&>[data-slot=filter-chip]]:gap-0 [&>[data-slot=filter-chip]]:px-0 [&>[data-slot=filter-chip]]:py-0 [&_button:first-child]:block [&_button:first-child]:min-w-0 [&_button:first-child]:truncate [&_button:first-child]:px-1 [&_button:last-child]:shrink-0 [&_svg]:inline-block",
              phone
                ? "[&_button]:h-11 [&_button:first-child]:min-w-11 [&_button:last-child]:w-11"
                : "[&_button]:h-6 [&_button:first-child]:min-w-6 [&_button:last-child]:w-6",
            )}
          >
            {tagTerms.map((chip) => (
              <TagFilterChip
                key={chip.tag}
                chip={chip}
                onCycle={(tag) => notesFiltersStore.getState().cycleTag(tag)}
                onRemove={(tag) => notesFiltersStore.getState().removeTag(tag)}
              />
            ))}
          </div>
        </div>
        <div
          data-slot="filter-actions"
          style={phone ? { gridArea: "actions" } : undefined}
          className={cn("flex shrink-0 flex-nowrap items-start", phone ? "gap-0" : "gap-1")}
        >
          <IconHint label="Changed by agent">
            <Button
              type="button"
              variant="ghost"
              size={phone ? "icon" : "icon-xs"}
              className={iconClass}
              aria-label="Changed by agent"
              aria-pressed={agentOnly}
              onClick={() => notesFiltersStore.getState().setAgentOnly(!agentOnly)}
            >
              <Bot aria-hidden="true" className="size-4" />
            </Button>
          </IconHint>
          <IconHint label="Pinned only">
            <Button
              type="button"
              variant="ghost"
              size={phone ? "icon" : "icon-xs"}
              className={iconClass}
              aria-label="Pinned only"
              aria-pressed={pinnedOnly}
              onClick={() => notesFiltersStore.getState().setPinnedOnly(!pinnedOnly)}
            >
              <Pin aria-hidden="true" className="size-4" />
            </Button>
          </IconHint>
          <IconHint label="Hide service files">
            <Button
              type="button"
              variant="ghost"
              size={phone ? "icon" : "icon-xs"}
              className={iconClass}
              aria-label="Hide service files"
              aria-pressed={hideServiceFiles}
              onClick={() =>
                (onHideServiceFilesChange ?? notesFiltersStore.getState().setHideServiceFiles)(
                  !hideServiceFiles,
                )
              }
            >
              {hideServiceFiles ? (
                <EyeOff aria-hidden="true" className="size-4" />
              ) : (
                <Eye aria-hidden="true" className="size-4" />
              )}
            </Button>
          </IconHint>
          <span className={cn(target, "shrink-0")}>
            {savable && (
              <IconHint label="Save as space">
                <Button
                  type="button"
                  variant="ghost"
                  size={phone ? "icon" : "icon-xs"}
                  className={iconClass}
                  aria-label="Save as space"
                  onClick={onSaveAsSpace}
                >
                  <Bookmark aria-hidden="true" className="size-4" />
                </Button>
              </IconHint>
            )}
          </span>
        </div>
      </div>
      {adding && (
        <TagCombobox
          label={ADD_TAG_FILTER}
          placeholder="Type or browse"
          vocabulary={vocabulary}
          chosen={tagTerms.map((chip) => chip.tag)}
          // The bar's own press is what mounted this, so the caret comes here
          // and the list is unfolded to browse (Story 53.2). This was a ref
          // callback calling `node?.focus()`, which left the browse half of
          // UX-DR61 riding on a focus side effect nothing declared.
          openOnMount
          onChoose={(tag) => notesFiltersStore.getState().setTagTerm(tag, "include")}
          onDismiss={closeChooser}
        />
      )}
      <div className="relative min-w-0">
        <Glyph
          aria-hidden="true"
          data-slot="search-state-glyph"
          data-phase={mode}
          className="pointer-events-none absolute top-1/2 left-2 size-4 -translate-y-1/2 text-muted-foreground"
        />
        <Input
          ref={attachSearchRef}
          id={fieldId}
          type="search"
          aria-label={NOTES_SEARCH_PLACEHOLDER}
          aria-describedby={`${fieldId}-description`}
          placeholder={NOTES_SEARCH_PLACEHOLDER}
          className={cn(
            "min-w-0 pl-8 [&::-webkit-search-cancel-button]:appearance-none",
            phone ? "h-11 pr-12" : "h-8 pr-8",
          )}
          value={text}
          onChange={(event) => notesFiltersStore.getState().setText(event.target.value)}
          onKeyDown={onSearchKeyDown}
        />
        {text !== "" && (
          <IconHint label="Clear search">
            <Button
              type="button"
              variant="ghost"
              size={phone ? "icon" : "icon-xs"}
              aria-label="Clear search"
              className={cn(iconClass, "absolute top-1/2 right-0 -translate-y-1/2")}
              onClick={() => {
                notesFiltersStore.getState().setText("");
                inputRef.current?.focus();
              }}
            >
              <X aria-hidden="true" className="size-4" />
            </Button>
          </IconHint>
        )}
      </div>
      <span id={`${fieldId}-description`} className="sr-only">
        {mode}
        {status ? `. ${status}` : ""}
      </span>
      {status !== "" && (
        <div
          role="status"
          data-slot="notes-search-status"
          className="min-w-0 text-muted-foreground text-xs"
        >
          {!failure && (meaningOff || searchState?.phase === "refused") ? (
            <button
              type="button"
              aria-label={meaningOff ? settingsLabel : `${status} Settings`}
              className="flex w-full min-w-0 items-center gap-1 text-left underline-offset-2 hover:underline focus-visible:ring-2 focus-visible:ring-ring"
              onClick={openSearchSettings}
            >
              <span className="truncate">
                {meaningOff ? "Meaning is off — choose an embedding model in" : status}
              </span>
              <span className="shrink-0">Settings</span>
            </button>
          ) : (
            <span className="block truncate">{status}</span>
          )}
        </div>
      )}
    </div>
  );
}
