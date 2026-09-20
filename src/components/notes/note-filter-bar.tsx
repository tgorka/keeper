import {
  ArrowUpDown,
  Bookmark,
  Bot,
  Eye,
  EyeOff,
  FilePlus,
  HardDrive,
  Loader,
  LockKeyhole,
  Minus,
  Pin,
  Plus,
  Search,
  Sparkles,
  X,
} from "lucide-react";
import { type Ref, useEffect, useRef, useState } from "react";
import { SearchField } from "@/components/notes/search-field";
import { SORT_DIR_LABELS, SPACE_SORT_KEYS } from "@/components/notes/space-editor";
import { Button } from "@/components/ui/button";
import { Popover, PopoverContent, PopoverTrigger } from "@/components/ui/popover";
import { IconHint } from "@/components/ui/tooltip";
import { notesCreate } from "@/lib/ipc/client";
import {
  ALL_NOTES_SCOPE,
  type NoteSortChoice,
  notesFiltersStore,
  persistIncludePrivate,
  scopeLabel,
  type TagChip,
  useNotesFiltersStore,
} from "@/lib/stores/notes-filters";
import { useNotesListStore } from "@/lib/stores/notes-list";
import { useNotesSearchState } from "@/lib/stores/notes-search-state";
import { useNotesVaultsStore } from "@/lib/stores/notes-vaults";
import { panelsStore } from "@/lib/stores/panels";
import { settingsUiStore } from "@/lib/stores/settings-ui";
import { syncErrorMessage } from "@/lib/stores/sync";
import { cn } from "@/lib/utils";

export const NOTES_SEARCH_PLACEHOLDER = "Search notes";

/** Shared three-state grammar; the search field opts out of sequential chip focus. */
export function TagFilterChip({
  chip,
  onCycle,
  onRemove,
  tabStop = true,
  phone = false,
}: {
  chip: TagChip;
  onCycle: (tag: string) => void;
  onRemove: (tag: string) => void;
  tabStop?: boolean;
  phone?: boolean;
}) {
  const excluded = chip.term === "exclude";
  const Sign = excluded ? Minus : Plus;
  const label = excluded
    ? `Tag ${chip.tag}: excluded. Stop filtering by it.`
    : `Tag ${chip.tag}: included. Exclude it instead.`;
  return (
    <span
      data-slot="filter-chip"
      data-tag-term={chip.term}
      className={cn(
        "inline-flex max-w-full shrink-0 items-center rounded-full text-meta",
        excluded
          ? "bg-destructive/15 text-destructive line-through decoration-destructive/60"
          : "bg-accent text-accent-foreground",
      )}
    >
      <IconHint label={label}>
        <button
          type="button"
          aria-label={label}
          tabIndex={tabStop ? 0 : -1}
          onPointerDown={tabStop ? undefined : (event) => event.preventDefault()}
          onClick={() => onCycle(chip.tag)}
          className={cn(
            "inline-flex min-w-0 items-center gap-1 rounded-full pl-2 outline-none focus-visible:ring-2 focus-visible:ring-ring",
            phone ? "h-11 min-w-11" : "h-6 min-w-6",
          )}
        >
          <Sign aria-hidden="true" className="size-3 shrink-0" />
          <span className="truncate">{chip.tag}</span>
        </button>
      </IconHint>
      <IconHint label={`Clear tag ${chip.tag} filter`}>
        <button
          type="button"
          aria-label={`Clear tag ${chip.tag} filter`}
          tabIndex={tabStop ? 0 : -1}
          onPointerDown={tabStop ? undefined : (event) => event.preventDefault()}
          onClick={() => onRemove(chip.tag)}
          className={cn(
            "flex shrink-0 items-center justify-center rounded-full outline-none hover:bg-background/40 focus-visible:ring-2 focus-visible:ring-ring",
            phone ? "size-11" : "size-6",
          )}
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
  onSaveAsSpace: (anchor: HTMLButtonElement) => void;
  searchRef?: Ref<HTMLTextAreaElement>;
  phone?: boolean;
  onHideServiceFilesChange?: (hidden: boolean) => void;
}) {
  const filters = useNotesFiltersStore((state) => state);
  const {
    scope,
    tagTerms,
    text,
    agentOnly,
    pinnedOnly,
    hideServiceFiles,
    sort,
    vaultIds,
    includePrivate,
  } = filters;
  const vaultId = useNotesVaultsStore((state) => state.activeVaultId);
  const vaults = useNotesVaultsStore((state) => state.vaults);
  const searchState = useNotesSearchState((state) =>
    vaultId ? state.byVault[vaultId] : undefined,
  );
  const searchError = useNotesListStore((state) => state.searchError);
  const notice = useNotesListStore((state) => state.notice);
  const searching = useNotesListStore((state) => state.searching);
  const [stalled, setStalled] = useState(false);
  const [menu, setMenu] = useState<"sort" | "drives" | "create" | null>(null);
  const [creating, setCreating] = useState(false);
  const [actionError, setActionError] = useState<string | null>(null);
  const settingsObserver = useRef<MutationObserver | null>(null);
  useEffect(() => () => settingsObserver.current?.disconnect(), []);
  useEffect(() => {
    setStalled(false);
    if (!searching) return;
    const timer = setTimeout(() => setStalled(true), 500);
    return () => clearTimeout(timer);
  }, [searching]);
  const indexing = searchState?.phase === "indexing";
  const meaning = searchState?.phase === "meaning" && !notice;
  const Glyph = indexing ? Loader : meaning ? Sparkles : Search;
  const mode = indexing ? "Indexing" : meaning ? "Words + meaning" : "Words";
  const settingsLabel = "Meaning is off — choose an embedding model in Settings";
  const meaningOff =
    searchState?.phase === "words" &&
    searchState.model === "" &&
    (!searchState.sentence || searchState.sentence === settingsLabel);
  const progress = indexing
    ? `Indexing ${searchState.indexed.toLocaleString()}${searchState.total > 0 ? `/${searchState.total.toLocaleString()}` : ""} notes`
    : searchState?.phase === "words" && searchState.embeddable > searchState.embedded
      ? `Indexing meaning ${searchState.embedded.toLocaleString()}/${searchState.embeddable.toLocaleString()} chunks`
      : "";
  const failure = searchError;
  const status =
    failure ||
    progress ||
    (stalled ? "Searching…" : "") ||
    notice ||
    searchState?.sentence ||
    (meaningOff ? settingsLabel : "");
  const selectedIds = vaultIds.length ? vaultIds : vaultId ? [vaultId] : [];
  const driveDescription = vaultIds.length
    ? `Search drives: ${vaultIds.map((id) => vaults?.find((vault) => vault.id === id)?.name ?? id).join(", ")}`
    : "Search the active drive only";
  const target = phone ? "size-11" : "size-6";
  const iconClass = cn(
    target,
    "shrink-0 rounded-[7px] p-0 text-muted-foreground aria-pressed:bg-accent aria-pressed:text-accent-foreground",
  );
  const optionClass = cn(
    "flex w-full items-center gap-2 rounded-[7px] px-2 text-left text-sm hover:bg-accent focus-visible:ring-2 focus-visible:ring-ring",
    phone ? "min-h-11" : "min-h-8",
  );
  const popupClass = "w-[280px] max-w-[calc(100vw-16px)] gap-1 rounded-[10px] p-2";
  const savable =
    tagTerms.length > 0 ||
    filters.flags.length > 0 ||
    filters.origin !== null ||
    agentOnly ||
    pinnedOnly ||
    text.trim() !== "";

  function openSearchSettings() {
    settingsObserver.current?.disconnect();
    settingsUiStore.getState().setSettingsOpen(true);
    const focus = () => {
      const target = document.getElementById("notes-embedding-model");
      if (!target) return false;
      target.scrollIntoView({ block: "center" });
      target.focus();
      return true;
    };
    if (!focus()) {
      settingsObserver.current = new MutationObserver(() => {
        if (focus()) settingsObserver.current?.disconnect();
      });
      settingsObserver.current.observe(document.body, { childList: true, subtree: true });
    }
  }

  async function createFromSearch(id: string) {
    if (creating) return;
    setCreating(true);
    setActionError(null);
    const snapshot = notesFiltersStore.getState();
    try {
      const created = await notesCreate(id, {
        title: null,
        body: snapshot.text,
        tags: snapshot.tagTerms.filter((chip) => chip.term === "include").map((chip) => chip.tag),
        template: null,
        dest: null,
        space:
          id === vaultId &&
          snapshot.scope.kind === "space" &&
          !snapshot.scope.id.startsWith("keeper:")
            ? snapshot.scope.id
            : null,
      });
      panelsStore
        .getState()
        .setActiveTarget({ kind: "note", vaultId: id, noteId: created.note.id });
      setMenu(null);
    } catch (error) {
      setActionError(syncErrorMessage(error, "Could not create a note from this search."));
    } finally {
      setCreating(false);
    }
  }

  return (
    <div
      data-slot="note-filter-bar"
      data-tier={phone ? "phone" : "desktop"}
      className={cn(
        "@container flex min-w-0 shrink-0 flex-col gap-2 border-b border-border py-2",
        phone ? "px-2" : "px-3",
      )}
    >
      <div
        data-slot="filter-actions"
        className={cn(
          "flex min-w-0 flex-wrap items-start",
          phone ? "gap-1" : "gap-0 @[220px]:gap-1",
        )}
      >
        <IconHint label="Changed by agent">
          <Button
            type="button"
            variant="ghost"
            className={iconClass}
            aria-label="Changed by agent"
            aria-pressed={agentOnly}
            onClick={() => filters.setAgentOnly(!agentOnly)}
          >
            <Bot aria-hidden="true" className="size-4" />
          </Button>
        </IconHint>
        <IconHint label="Pinned only">
          <Button
            type="button"
            variant="ghost"
            className={iconClass}
            aria-label="Pinned only"
            aria-pressed={pinnedOnly}
            onClick={() => filters.setPinnedOnly(!pinnedOnly)}
          >
            <Pin aria-hidden="true" className="size-4" />
          </Button>
        </IconHint>
        <IconHint label="Hide service files">
          <Button
            type="button"
            variant="ghost"
            className={iconClass}
            aria-label="Hide service files"
            aria-pressed={hideServiceFiles}
            onClick={() =>
              (onHideServiceFilesChange ?? filters.setHideServiceFiles)(!hideServiceFiles)
            }
          >
            {hideServiceFiles ? (
              <EyeOff aria-hidden="true" className="size-4" />
            ) : (
              <Eye aria-hidden="true" className="size-4" />
            )}
          </Button>
        </IconHint>
        <Popover open={menu === "sort"} onOpenChange={(open) => setMenu(open ? "sort" : null)}>
          <IconHint label="Sort notes">
            <PopoverTrigger asChild>
              <Button
                type="button"
                variant="ghost"
                className={iconClass}
                aria-label="Sort notes"
                aria-description={sort ? `${sort.key} ${sort.dir}` : "Context order"}
              >
                <ArrowUpDown aria-hidden="true" className="size-4" />
              </Button>
            </PopoverTrigger>
          </IconHint>
          <PopoverContent align="start" collisionPadding={8} className={popupClass}>
            <fieldset
              aria-label="Sort notes"
              className={cn("overflow-y-auto", phone ? "max-h-[264px]" : "max-h-48")}
            >
              <button
                type="button"
                className={optionClass}
                aria-pressed={sort === null}
                onClick={() => {
                  filters.setSort(null);
                  setMenu(null);
                }}
              >
                Context order
              </button>
              {text.trim() !== "" && (
                <button
                  type="button"
                  className={optionClass}
                  aria-pressed={sort?.key === "relevance"}
                  onClick={() => {
                    filters.setSort({ key: "relevance", dir: "desc" });
                    setMenu(null);
                  }}
                >
                  Relevance
                </button>
              )}
              {SPACE_SORT_KEYS.flatMap(({ key, label }) =>
                (["asc", "desc"] as const).map((dir, index) => (
                  <button
                    key={`${key}-${dir}`}
                    type="button"
                    className={optionClass}
                    aria-pressed={sort?.key === key && sort.dir === dir}
                    onClick={() => {
                      filters.setSort({ key: key as NoteSortChoice["key"], dir });
                      setMenu(null);
                    }}
                  >
                    {label} · {SORT_DIR_LABELS[key][index]}
                  </button>
                )),
              )}
            </fieldset>
          </PopoverContent>
        </Popover>
        <Popover open={menu === "drives"} onOpenChange={(open) => setMenu(open ? "drives" : null)}>
          <IconHint label="Search drives">
            <PopoverTrigger asChild>
              <Button
                type="button"
                variant="ghost"
                className={iconClass}
                aria-label="Search drives"
                aria-description={driveDescription}
              >
                <HardDrive aria-hidden="true" className="size-4" />
              </Button>
            </PopoverTrigger>
          </IconHint>
          <PopoverContent align="start" collisionPadding={8} className={popupClass}>
            <p className="text-xs text-muted-foreground">
              No selection searches the active drive only.
            </p>
            <div className={cn("overflow-y-auto", phone ? "max-h-[264px]" : "max-h-48")}>
              {vaults === null ? (
                <p>Loading drives…</p>
              ) : (
                vaults.map((vault) => (
                  <label key={vault.id} className={optionClass}>
                    <input
                      type="checkbox"
                      checked={vaultIds.includes(vault.id)}
                      onChange={(event) =>
                        filters.setVaultIds(
                          event.target.checked
                            ? [...vaultIds, vault.id]
                            : vaultIds.filter((id) => id !== vault.id),
                        )
                      }
                    />
                    <span className="min-w-0 truncate">{vault.name}</span>
                  </label>
                ))
              )}
            </div>
          </PopoverContent>
        </Popover>
        <IconHint label="Include private notes">
          <Button
            type="button"
            variant="ghost"
            className={iconClass}
            aria-label="Include private notes"
            aria-pressed={includePrivate}
            onClick={() => {
              void persistIncludePrivate(!includePrivate).catch((error) =>
                setActionError(syncErrorMessage(error, "Could not save private-note visibility.")),
              );
            }}
          >
            <LockKeyhole aria-hidden="true" className="size-4" />
          </Button>
        </IconHint>
        <Popover open={menu === "create"} onOpenChange={(open) => setMenu(open ? "create" : null)}>
          <IconHint label="New note from search">
            <PopoverTrigger asChild>
              <Button
                type="button"
                variant="ghost"
                className={iconClass}
                aria-label="New note from search"
                disabled={creating || selectedIds.length === 0}
                onClick={(event) => {
                  if (selectedIds.length === 1) {
                    event.preventDefault();
                    void createFromSearch(selectedIds[0]);
                  }
                }}
              >
                <FilePlus aria-hidden="true" className="size-4" />
              </Button>
            </PopoverTrigger>
          </IconHint>
          <PopoverContent align="start" collisionPadding={8} className={popupClass}>
            <p className="text-xs">Choose a drive for the new note</p>
            <div className={cn("overflow-y-auto", phone ? "max-h-[264px]" : "max-h-48")}>
              {selectedIds.map((id) => (
                <button
                  key={id}
                  type="button"
                  disabled={creating}
                  className={optionClass}
                  onClick={() => void createFromSearch(id)}
                >
                  {vaults?.find((vault) => vault.id === id)?.name ?? id}
                </button>
              ))}
            </div>
          </PopoverContent>
        </Popover>
        <IconHint label="Save as space">
          <Button
            type="button"
            variant="ghost"
            className={iconClass}
            aria-label="Save as space"
            aria-description={
              !vaultId
                ? "Choose a drive before saving a space."
                : !savable
                  ? "Add a search term or filter before saving a space."
                  : undefined
            }
            disabled={!savable || !vaultId}
            onClick={(event) => onSaveAsSpace(event.currentTarget)}
          >
            <Bookmark aria-hidden="true" className="size-4" />
          </Button>
        </IconHint>
      </div>
      <SearchField
        text={text}
        chips={tagTerms}
        phone={phone}
        searchRef={searchRef}
        description={[
          mode,
          ...new Set(
            [
              failure,
              progress,
              stalled ? "Searching…" : "",
              notice,
              searchState?.sentence,
              meaningOff ? settingsLabel : "",
            ].filter(Boolean),
          ),
          driveDescription,
        ].join(". ")}
        glyph={
          <Glyph
            aria-hidden="true"
            data-slot="search-state-glyph"
            data-phase={mode}
            className="size-4"
          />
        }
      >
        {scope.kind !== "all" && (
          <span
            data-slot="filter-chip"
            className={cn(
              "inline-flex max-w-full shrink-0 items-center rounded-full bg-accent pl-2 text-meta",
              phone ? "h-11" : "h-6",
            )}
          >
            <span className="min-w-0 truncate">{scopeLabel(scope)}</span>
            <IconHint label={`Clear ${scopeLabel(scope)} scope`}>
              <button
                type="button"
                tabIndex={-1}
                aria-label={`Clear ${scopeLabel(scope)} scope`}
                onPointerDown={(event) => event.preventDefault()}
                onClick={() => filters.setScope(ALL_NOTES_SCOPE)}
                className={cn("flex shrink-0 items-center justify-center", target)}
              >
                <X aria-hidden="true" className="size-3" />
              </button>
            </IconHint>
          </span>
        )}
        {tagTerms.map((chip) => (
          <TagFilterChip
            key={chip.tag}
            chip={chip}
            phone={phone}
            tabStop={false}
            onCycle={filters.cycleTag}
            onRemove={filters.removeTag}
          />
        ))}
        {[
          ...filters.flags.map((flag) => ({
            label: `is:${flag}`,
            remove: () => filters.setFlags(filters.flags.filter((value) => value !== flag)),
          })),
          ...(filters.origin
            ? [{ label: `origin:${filters.origin}`, remove: () => filters.setOrigin(null) }]
            : []),
        ].map(({ label, remove }) => (
          <span
            key={label}
            data-slot="filter-chip"
            className={cn(
              "inline-flex max-w-full shrink-0 items-center rounded-full bg-accent pl-2 text-meta",
              phone ? "h-11" : "h-6",
            )}
          >
            <span className="min-w-0 truncate">{label}</span>
            <IconHint label={`Clear ${label} filter`}>
              <button
                type="button"
                tabIndex={-1}
                aria-label={`Clear ${label} filter`}
                onPointerDown={(event) => event.preventDefault()}
                onClick={remove}
                className={cn("flex shrink-0 items-center justify-center", target)}
              >
                <X aria-hidden="true" className="size-3" />
              </button>
            </IconHint>
          </span>
        ))}
      </SearchField>
      {status && (
        <div
          role="status"
          data-slot="notes-search-status"
          className="min-w-0 text-xs leading-4 text-muted-foreground"
        >
          <IconHint label={status}>
            {!failure && (meaningOff || searchState?.phase === "refused") ? (
              <button
                type="button"
                aria-label={meaningOff ? settingsLabel : `${status} Settings`}
                className="flex w-full min-w-0 items-center gap-1 text-left"
                onClick={openSearchSettings}
              >
                <span className="truncate">{status}</span>
                <span className="shrink-0">Settings</span>
              </button>
            ) : (
              <span className="block truncate">{status}</span>
            )}
          </IconHint>
        </div>
      )}
      {actionError && (
        <p role="alert" className="text-xs text-destructive">
          {actionError}
        </p>
      )}
    </div>
  );
}
