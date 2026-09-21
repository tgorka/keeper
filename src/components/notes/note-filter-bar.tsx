import {
  ArrowUpDown,
  Bot,
  Check,
  Eye,
  EyeOff,
  FilePlus,
  Folder,
  HardDrive,
  Loader,
  LockKeyhole,
  LockKeyholeOpen,
  Minus,
  Pin,
  Plus,
  Save,
  Search,
  Sparkles,
  Tags,
  X,
} from "lucide-react";
import { type Ref, useEffect, useRef, useState } from "react";
import { SearchField } from "@/components/notes/search-field";
import { SignedPopover, TAG_TERM_PAINT } from "@/components/notes/signed-popover";
import { SORT_DIR_LABELS, SPACE_SORT_KEYS } from "@/components/notes/space-editor";
import { matchTags } from "@/components/tags/tag-match";
import { Button } from "@/components/ui/button";
import { Popover, PopoverContent, PopoverTrigger } from "@/components/ui/popover";
import { HoverHint, IconHint } from "@/components/ui/tooltip";
import { notesCreate, tagsVocabulary } from "@/lib/ipc/client";
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

/** Shared split-action grammar; search chips opt out of sequential focus. */
export function TagFilterChip({
  chip,
  onToggleSign,
  onRemove,
  tabStop = true,
  phone = false,
}: {
  chip: TagChip;
  onToggleSign: (tag: string) => void;
  onRemove: (tag: string) => void;
  tabStop?: boolean;
  phone?: boolean;
}) {
  const excluded = chip.term === "exclude";
  const Sign = excluded ? Minus : Plus;
  const label = `${excluded ? "Include" : "Exclude"} tag ${chip.tag}`;
  const actionClass = cn(
    "flex shrink-0 items-center justify-center rounded-full outline-none hover:bg-background/40 focus-visible:ring-2 focus-visible:ring-inset focus-visible:ring-ring",
    phone ? "size-11" : "size-6",
  );
  return (
    <span
      data-slot="filter-chip"
      data-tag-term={chip.term}
      className={cn(
        "inline-flex max-w-full shrink-0 items-center rounded-full text-meta font-medium",
        TAG_TERM_PAINT[chip.term],
      )}
    >
      <IconHint label={label}>
        <button
          type="button"
          aria-label={label}
          aria-description={`Tag ${chip.tag}: ${excluded ? "excluded" : "included"}`}
          tabIndex={tabStop ? 0 : -1}
          onPointerDown={tabStop ? undefined : (event) => event.preventDefault()}
          onClick={() => onToggleSign(chip.tag)}
          className={cn(actionClass, "hover:ring-1 hover:ring-inset hover:ring-current")}
        >
          <Sign aria-hidden="true" className="size-3.5" />
        </button>
      </IconHint>
      <span
        className={cn(
          "min-w-0 truncate px-1",
          excluded && "line-through decoration-destructive/60",
        )}
      >
        {chip.tag}
      </span>
      <IconHint label={`Clear tag ${chip.tag} filter`}>
        <button
          type="button"
          aria-label={`Clear tag ${chip.tag} filter`}
          tabIndex={tabStop ? 0 : -1}
          onPointerDown={tabStop ? undefined : (event) => event.preventDefault()}
          onClick={() => onRemove(chip.tag)}
          className={cn(actionClass, "hover:text-foreground")}
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
  onCreateNotices,
}: {
  onSaveAsSpace: (anchor: HTMLButtonElement) => void;
  searchRef?: Ref<HTMLTextAreaElement>;
  phone?: boolean;
  onHideServiceFilesChange?: (hidden: boolean) => void;
  onCreateNotices?: (notices: string[]) => void;
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
  const [menu, setMenu] = useState<"tags" | "sort" | "drives" | "create" | null>(null);
  const [creating, setCreating] = useState(false);
  const [actionError, setActionError] = useState<string | null>(null);
  const [tagQuery, setTagQuery] = useState("");
  const [vocabulary, setVocabulary] = useState<readonly string[]>([]);
  const [tagsLoading, setTagsLoading] = useState(false);
  const [tagsError, setTagsError] = useState<string | null>(null);
  const localSearchRef = useRef<HTMLTextAreaElement | null>(null);
  const settingsObserver = useRef<MutationObserver | null>(null);
  useEffect(() => () => settingsObserver.current?.disconnect(), []);
  useEffect(() => {
    setStalled(false);
    if (!searching) return;
    const timer = setTimeout(() => setStalled(true), 500);
    return () => clearTimeout(timer);
  }, [searching]);
  useEffect(() => {
    if (menu !== "tags") return;
    let cancelled = false;
    setTagsLoading(true);
    setTagsError(null);
    void tagsVocabulary()
      .then((vm) => {
        if (!cancelled) setVocabulary(vm.entries.map((entry) => entry.path));
      })
      .catch(() => {
        if (!cancelled)
          setTagsError("Could not load tags. Search and existing filters are still available.");
      })
      .finally(() => {
        if (!cancelled) setTagsLoading(false);
      });
    return () => {
      cancelled = true;
    };
  }, [menu]);
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
      const element = document.getElementById("notes-embedding-model");
      if (!element) return false;
      element.scrollIntoView({ block: "center" });
      element.focus();
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
          snapshot.scope.kind === "space" && !snapshot.scope.id.startsWith("keeper:")
            ? snapshot.scope.id
            : null,
        spaceVaultId: snapshot.scope.kind === "space" ? snapshot.scope.vaultId : null,
      });
      onCreateNotices?.(created.notices);
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
      <SearchField
        text={text}
        chips={tagTerms}
        phone={phone}
        searchRef={(node) => {
          localSearchRef.current = node;
          if (typeof searchRef === "function") searchRef(node);
          else if (searchRef) searchRef.current = node;
        }}
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
            className="size-5"
          />
        }
        controls={[
          <Popover
            key="tags"
            open={menu === "tags"}
            onOpenChange={(open) => {
              setMenu(open ? "tags" : null);
              if (open) setTagQuery("");
            }}
          >
            <IconHint label="Add tag">
              <PopoverTrigger asChild>
                <Button type="button" variant="ghost" className={iconClass} aria-label="Add tag">
                  <span className="relative size-4">
                    <Tags aria-hidden="true" className="size-4" />
                    <Plus
                      aria-hidden="true"
                      className="absolute -right-0.5 -top-0.5 size-2 bg-background"
                    />
                  </span>
                </Button>
              </PopoverTrigger>
            </IconHint>
            <SignedPopover
              matches={matchTags(tagQuery, vocabulary)}
              chips={tagTerms}
              phone={phone}
              loading={tagsLoading}
              error={tagsError}
              query={{ value: tagQuery, onChange: setTagQuery }}
              onChoose={(tag, term) => {
                filters.setTagTerm(tag, term);
                setMenu(null);
              }}
              onCloseAutoFocus={(event) => {
                event.preventDefault();
                localSearchRef.current?.focus();
              }}
            />
          </Popover>,
          <IconHint key="agent" label="Changed by agent">
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
          </IconHint>,
          <IconHint key="pin" label="Pinned only">
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
          </IconHint>,
          <IconHint key="service" label="Hide service files">
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
          </IconHint>,
          <Popover
            key="sort"
            open={menu === "sort"}
            onOpenChange={(open) => setMenu(open ? "sort" : null)}
          >
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
          </Popover>,
          <Popover
            key="drives"
            open={menu === "drives"}
            onOpenChange={(open) => setMenu(open ? "drives" : null)}
          >
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
              <div className="max-h-72 overflow-y-auto">
                {vaults === null ? (
                  <p>Loading drives…</p>
                ) : vaults.length === 0 ? (
                  <p>No drives available.</p>
                ) : (
                  vaults.map((vault) => (
                    <HoverHint key={vault.id} label={vault.name} detail={vault.root}>
                      <label
                        className={cn(
                          "relative flex min-h-12 w-full cursor-pointer items-center gap-2 rounded-[7px] px-2 py-2 text-left outline-none focus-within:ring-2 focus-within:ring-inset focus-within:ring-ring",
                          vaultIds.includes(vault.id)
                            ? "bg-accent text-accent-foreground"
                            : "hover:bg-accent/50",
                        )}
                      >
                        <input
                          type="checkbox"
                          checked={vaultIds.includes(vault.id)}
                          aria-label={vault.name}
                          aria-description={vault.root}
                          className="sr-only"
                          onChange={() =>
                            filters.setVaultIds(
                              vaultIds.includes(vault.id)
                                ? vaultIds.filter((id) => id !== vault.id)
                                : [...vaultIds, vault.id],
                            )
                          }
                        />
                        <HardDrive aria-hidden="true" className="size-4 shrink-0" />
                        <span className="min-w-0 flex-1">
                          <span className="block truncate text-sm font-medium leading-4">
                            {vault.name}
                          </span>
                          <span className="block truncate text-meta leading-4 text-muted-foreground">
                            {vault.root}
                          </span>
                        </span>
                        <span className="size-4 shrink-0">
                          {vaultIds.includes(vault.id) && (
                            <Check aria-hidden="true" className="size-4" />
                          )}
                        </span>
                      </label>
                    </HoverHint>
                  ))
                )}
              </div>
            </PopoverContent>
          </Popover>,
          <IconHint key="private" label="Include private notes">
            <Button
              type="button"
              variant="ghost"
              className={iconClass}
              aria-label="Include private notes"
              aria-pressed={includePrivate}
              onClick={() => {
                void persistIncludePrivate(!includePrivate).catch((error) =>
                  setActionError(
                    syncErrorMessage(error, "Could not save private-note visibility."),
                  ),
                );
              }}
            >
              {includePrivate ? (
                <LockKeyholeOpen aria-hidden="true" className="size-4" />
              ) : (
                <LockKeyhole aria-hidden="true" className="size-4" />
              )}
            </Button>
          </IconHint>,
          <Popover
            key="create"
            open={menu === "create"}
            onOpenChange={(open) => setMenu(open ? "create" : null)}
          >
            <IconHint label="New note from search">
              <PopoverTrigger asChild>
                <Button
                  type="button"
                  variant="ghost"
                  className={iconClass}
                  aria-label="New note from search"
                  disabled={creating || selectedIds.length === 0}
                  onClick={(event) => {
                    if (
                      selectedIds.length === 1 &&
                      (scope.kind !== "space" || selectedIds[0] === scope.vaultId)
                    ) {
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
                {(scope.kind === "space"
                  ? [scope.vaultId, ...selectedIds.filter((id) => id !== scope.vaultId)]
                  : selectedIds
                ).map((id) => (
                  <button
                    key={id}
                    type="button"
                    disabled={creating}
                    className={optionClass}
                    onClick={() => void createFromSearch(id)}
                  >
                    {vaults?.find((vault) => vault.id === id)?.name ?? id}
                    {scope.kind === "space" &&
                      ` — ${id === scope.vaultId ? "in" : "outside"} ${scope.name}`}
                  </button>
                ))}
              </div>
            </PopoverContent>
          </Popover>,
          <IconHint key="save" label="Save as space">
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
              <Save aria-hidden="true" className="size-4" />
            </Button>
          </IconHint>,
        ]}
      >
        {scope.kind !== "all" && (
          <span
            data-slot="filter-chip"
            className={cn(
              "inline-flex max-w-full shrink-0 items-center gap-1 rounded-[7px] border border-border bg-muted pl-2 text-meta font-medium text-foreground",
              phone ? "h-11" : "h-6",
            )}
          >
            <Folder aria-hidden="true" className="size-4 shrink-0" />
            <span className="min-w-0 truncate" aria-description={scopeLabel(scope)}>
              {scope.kind === "space"
                ? scope.name.replace(/^["“«]|["”»]$/g, "") === filters.enteredSpace?.restore.text
                  ? "Space"
                  : `Space: ${scope.name}`
                : scopeLabel(scope)}
            </span>
            <IconHint label={`Clear scope ${scopeLabel(scope)}`}>
              <button
                type="button"
                tabIndex={-1}
                aria-label={`Clear scope ${scopeLabel(scope)}`}
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
            onToggleSign={filters.toggleTagSign}
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
