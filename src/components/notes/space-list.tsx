import { ChevronDown, ChevronRight, Pin, RotateCcw } from "lucide-react";
import { Fragment, useCallback, useEffect, useRef, useState } from "react";
import { FoldSection } from "@/components/layout/sidebar-group";
import { NoteDeleteDialog } from "@/components/notes/note-delete-dialog";
import { SpaceEditor } from "@/components/notes/space-editor";
import { spaceIcon } from "@/components/notes/space-icons";
import {
  ContextMenu,
  ContextMenuContent,
  ContextMenuItem,
  ContextMenuSeparator,
  ContextMenuTrigger,
} from "@/components/ui/context-menu";
import { Lamp } from "@/components/ui/lamp";
import { MENU_TARGET_RING, useMenuTarget } from "@/components/ui/menu-target";
import { HoverHint, IconHint } from "@/components/ui/tooltip";
import { openNotesSpace } from "@/hooks/use-notes-actions";
import { useShellLayout } from "@/hooks/use-shell-layout";
import type { NoteSpaceVm } from "@/lib/ipc/client";
import { notesSpaceSave, notesSpaces, notesSpacesRestoreDefaults } from "@/lib/ipc/client";
import { ALL_SPACE_ID, GROUP_SPACE_PREFIX, TEMPORARY_SPACE_ID } from "@/lib/notes/all-spaces";
import {
  ALL_NOTES_SCOPE,
  notesFiltersStore,
  useNotesFiltersStore,
} from "@/lib/stores/notes-filters";
import { notesRailFoldStore, useNotesRailFold } from "@/lib/stores/notes-rail-fold";
import { syncErrorMessage } from "@/lib/stores/sync";
import { cn } from "@/lib/utils";

export const SPACE_BROKEN_SUBTITLE = "This space's query can't be read";
export const SPACE_SETTINGS_SUBTITLE = "Some of this space's settings can't be read";
export const RESTORE_DEFAULTS = "Restore default spaces";
export const RESTORE_NOTHING_MISSING = "Nothing was missing.";
export const RESTORE_FAILED = "keeper couldn't restore the default spaces.";
export const DELETE_SPACE = "Delete space";

export function SpaceList({
  vaultId,
  onNewNote,
  savedSpace = null,
}: {
  vaultId: string | null;
  onNewNote?: (space: NoteSpaceVm) => void;
  savedSpace?: NoteSpaceVm | null;
}) {
  const [spaces, setSpaces] = useState<NoteSpaceVm[]>([]);
  const { phone } = useShellLayout();
  const [editing, setEditing] = useState<NoteSpaceVm | null>(null);
  const [creating, setCreating] = useState<string | null>(null);
  const [deleting, setDeleting] = useState<string | null>(null);
  const [restoring, setRestoring] = useState(false);
  const [notice, setNotice] = useState<string | null>(null);
  const [collapsed, setCollapsed] = useState<ReadonlySet<string>>(new Set());
  const [revealed, setRevealed] = useState<NoteSpaceVm | null>(null);
  const request = useRef(0);
  const lastSaved = useRef<NoteSpaceVm | null>(null);
  const previousVault = useRef<string | null | undefined>(undefined);
  const rowRefs = useRef(new Map<string, HTMLButtonElement>());
  const menuOpensEditor = useRef(false);
  const menu = useMenuTarget();
  const nonce = useNotesFiltersStore((state) => state.spacesNonce);
  const [pendingSpace, setPendingSpace] = useState<NoteSpaceVm | null>(null);
  const opening = useRef(0);
  const scopedSpaceId = useNotesFiltersStore((state) =>
    state.scope.kind === "space"
      ? state.scope.id
      : state.scope.kind === "all"
        ? ALL_SPACE_ID
        : null,
  );
  const activeSpaceId = pendingSpace?.id ?? scopedSpaceId;
  const folded = useNotesRailFold((state) => state.groups.spaces);
  const reload = useCallback(
    async (reveal?: NoteSpaceVm | null) => {
      const generation = ++request.current;
      if (vaultId === null) {
        setSpaces([]);
        return;
      }
      try {
        const rows = await notesSpaces(vaultId);
        if (generation !== request.current) return;
        setSpaces(rows);
        if (reveal) {
          const byId = new Map(rows.map((row) => [row.id, row]));
          setCollapsed((current) => {
            const next = new Set(current);
            let parent = byId.get(reveal.id)?.parent ?? reveal.parent;
            while (parent) {
              next.delete(parent);
              parent = byId.get(parent)?.parent ?? null;
            }
            return next;
          });
          const fold = notesRailFoldStore.getState();
          if (fold.groups.spaces) fold.toggleGroup("spaces");
          setRevealed(reveal);
        }
      } catch (error) {
        if (generation === request.current)
          setNotice(syncErrorMessage(error, "Spaces could not be read."));
      }
    },
    [vaultId],
  );
  useEffect(() => {
    if (previousVault.current === vaultId) return;
    previousVault.current = vaultId;
    setCollapsed(new Set());
    setEditing(null);
    setCreating(null);
    setNotice(null);
    setSpaces([]);
    setPendingSpace(null);
    opening.current += 1;
  }, [vaultId]);
  useEffect(() => {
    void nonce;
    const reveal = savedSpace !== lastSaved.current ? savedSpace : null;
    lastSaved.current = savedSpace;
    void reload(reveal);
    return () => {
      request.current += 1;
    };
  }, [reload, nonce, savedSpace]);
  useEffect(() => {
    if (revealed) rowRefs.current.get(revealed.id)?.scrollIntoView?.({ block: "nearest" });
  }, [revealed]);
  const report = (error: unknown) =>
    setNotice(syncErrorMessage(error, "keeper couldn't change this space."));
  const toggle = (id: string) =>
    setCollapsed((current) => {
      const next = new Set(current);
      if (next.has(id)) next.delete(id);
      else next.add(id);
      return next;
    });
  const open = (space: NoteSpaceVm) => {
    if (vaultId === null || space.error !== null) return;
    const request = ++opening.current;
    setPendingSpace(space);
    void openNotesSpace(vaultId, space)
      .then((acknowledged) => {
        setSpaces((current) =>
          current.map((row) => (row.id === acknowledged.id ? acknowledged : row)),
        );
      })
      .catch(report)
      .finally(() => {
        if (request === opening.current) setPendingSpace(null);
      });
  };
  const pin = async (space: NoteSpaceVm) => {
    if (vaultId === null) return;
    try {
      const saved = await notesSpaceSave(vaultId, {
        id: space.id,
        name: space.name,
        query: space.query,
        sort: space.sort,
        baseSpaceId: null,
        limit: space.limit,
        icon: space.icon,
        order: space.order,
        template: space.template,
        folder: space.folder,
        text: space.text,
        ttlHours: space.ttlHours,
        pinned: !space.pinned,
      });
      await reload(saved);
    } catch (error) {
      report(error);
    }
  };
  const restore = async () => {
    if (vaultId === null) return;
    setRestoring(true);
    setNotice(null);
    try {
      const count = await notesSpacesRestoreDefaults(vaultId);
      setNotice(
        count === 0
          ? RESTORE_NOTHING_MISSING
          : count === 1
            ? "Restored 1 space."
            : `Restored ${count} spaces.`,
      );
      await reload();
    } catch (error) {
      setNotice(syncErrorMessage(error, RESTORE_FAILED));
    } finally {
      setRestoring(false);
    }
  };
  // Parent ids and their depth-first order are Rust facts. No slash parsing or sorting.
  const children = new Map<string | null, NoteSpaceVm[]>();
  for (const space of spaces) {
    const siblings = children.get(space.parent) ?? [];
    siblings.push(space);
    children.set(space.parent, siblings);
  }
  const renderRows = (parent: string | null) => {
    const siblings = children.get(parent) ?? [];
    const hasPins = siblings.some((space) => space.pinned);
    return siblings.map((space, index) => {
      const group = space.id === TEMPORARY_SPACE_ID || space.id.startsWith(GROUP_SPACE_PREFIX);
      const synthetic = space.id.startsWith("keeper:");
      const expanded = !collapsed.has(space.id);
      const hasChildren = space.descendants > 0;
      const active = space.id === activeSpaceId;
      const subtitle =
        space.error !== null
          ? SPACE_BROKEN_SUBTITLE
          : space.warnings.length > 0
            ? SPACE_SETTINGS_SUBTITLE
            : null;
      const label = space.id === TEMPORARY_SPACE_ID ? "Temporary spaces" : space.name;
      const detail = [
        space.error ?? space.warnings.join(" "),
        space.query,
        space.ttlHours == null
          ? ""
          : `Temporary · resets after opening · ${space.ttlHours} h lifetime`,
        space.expiresMs == null ? "" : new Date(space.expiresMs).toISOString(),
      ]
        .filter(Boolean)
        .join(" · ");
      const Glyph = spaceIcon(space.icon);
      const Chevron = expanded ? ChevronDown : ChevronRight;
      const childId = `space-children-${space.id}`;
      return (
        <Fragment key={space.id}>
          {space.pinned && !siblings[index - 1]?.pinned && (
            <li
              aria-hidden="true"
              className="px-2 py-1 font-semibold text-meta text-muted-foreground leading-4"
            >
              PINNED
            </li>
          )}
          {hasPins && !space.pinned && siblings[index - 1]?.pinned && (
            <li aria-hidden="true" className="my-1 border-t" />
          )}
          <li>
            <ContextMenu onOpenChange={menu.onOpenChange(space.id)}>
              <ContextMenuTrigger asChild>
                <div
                  className={cn(
                    "flex items-start rounded-[7px]",
                    MENU_TARGET_RING,
                    active ? "bg-accent text-accent-foreground" : "hover:bg-accent/50",
                  )}
                  data-menu-target={menu.rowProps(space.id)["data-menu-target"]}
                  style={{ paddingLeft: Math.min(space.depth * 12, 36) }}
                >
                  {!group &&
                    (hasChildren && space.id !== ALL_SPACE_ID ? (
                      <button
                        type="button"
                        aria-label={`${expanded ? "Collapse" : "Expand"} ${space.name}`}
                        aria-expanded={expanded}
                        aria-controls={childId}
                        className={cn(
                          "flex shrink-0 items-center justify-center rounded-[7px] outline-none focus-visible:ring-2 focus-visible:ring-ring",
                          phone ? "h-11 w-11" : "h-8 w-6",
                        )}
                        onClick={() => toggle(space.id)}
                      >
                        <Chevron aria-hidden="true" className="size-3" />
                      </button>
                    ) : (
                      <span aria-hidden="true" className={cn("shrink-0", phone ? "w-11" : "w-6")} />
                    ))}
                  <HoverHint label={label} detail={detail} side="right">
                    <button
                      type="button"
                      ref={(node) => {
                        if (node) rowRefs.current.set(space.id, node);
                        else rowRefs.current.delete(space.id);
                      }}
                      {...menu.rowProps(space.id)}
                      aria-haspopup="menu"
                      aria-current={active ? "true" : undefined}
                      aria-pressed={group ? undefined : active}
                      aria-expanded={group ? expanded : menu.rowProps(space.id)["aria-expanded"]}
                      aria-controls={group ? childId : undefined}
                      aria-label={subtitle === null ? label : `${label}, ${subtitle}`}
                      aria-description={detail}
                      className={cn(
                        "flex min-w-0 flex-1 items-start gap-1 rounded-[7px] px-2 py-1 text-left outline-none focus-visible:ring-2 focus-visible:ring-inset focus-visible:ring-ring",
                        phone ? "min-h-11" : "min-h-8",
                      )}
                      onKeyDown={(event) => {
                        if (
                          event.key === "ContextMenu" ||
                          (event.shiftKey && event.key === "F10")
                        ) {
                          event.preventDefault();
                          const bounds = event.currentTarget.getBoundingClientRect();
                          event.currentTarget.dispatchEvent(
                            new MouseEvent("contextmenu", {
                              bubbles: true,
                              clientX: bounds.left,
                              clientY: bounds.bottom,
                            }),
                          );
                        }
                      }}
                      onClick={() => (group ? toggle(space.id) : open(space))}
                    >
                      {group ? (
                        <Chevron aria-hidden="true" className="mt-1 size-3 shrink-0" />
                      ) : (
                        <Glyph
                          aria-hidden="true"
                          data-slot="space-icon"
                          data-space-icon={space.icon ?? "none"}
                          className="mt-0.5 size-4 shrink-0 text-muted-foreground"
                        />
                      )}
                      {space.error !== null && (
                        <Lamp state="fault" label={null} className="mt-1.5" />
                      )}
                      <span className="flex min-w-0 flex-1 flex-col">
                        <span className="truncate text-sm leading-5">
                          {space.id === TEMPORARY_SPACE_ID ? label : space.leafName}
                        </span>
                        {subtitle && (
                          <span className="mt-1 break-words text-meta text-muted-foreground leading-4">
                            {subtitle}
                          </span>
                        )}
                        {space.expiryPhrase && (
                          <span className="mt-1 break-words text-meta text-muted-foreground leading-4">
                            {space.expiryPhrase}
                          </span>
                        )}
                      </span>
                      {space.pinned && <Pin aria-hidden="true" className="mt-1 size-3 shrink-0" />}
                      {hasChildren && (
                        <span className="w-8 shrink-0 text-right text-meta text-muted-foreground leading-5">
                          {space.descendants}
                        </span>
                      )}
                    </button>
                  </HoverHint>
                </div>
              </ContextMenuTrigger>
              <ContextMenuContent
                aria-label={`Actions for ${space.name}`}
                collisionPadding={8}
                className={cn(
                  "max-h-[calc(100dvh-16px)] w-60 max-w-[calc(100vw-16px)]",
                  phone ? "[&_[role=menuitem]]:min-h-11" : "[&_[role=menuitem]]:min-h-8",
                )}
                onCloseAutoFocus={(event) => {
                  event.preventDefault();
                  if (!menuOpensEditor.current) rowRefs.current.get(space.id)?.focus();
                  menuOpensEditor.current = false;
                }}
              >
                {group ? (
                  <ContextMenuItem onSelect={() => toggle(space.id)}>
                    {expanded ? "Collapse group" : "Expand group"}
                  </ContextMenuItem>
                ) : (
                  <>
                    <ContextMenuItem disabled={space.error !== null} onSelect={() => open(space)}>
                      Open space
                    </ContextMenuItem>
                    <ContextMenuItem
                      disabled={space.error !== null}
                      onSelect={() => {
                        try {
                          notesFiltersStore.getState().mergeSpace(space);
                        } catch (error) {
                          report(error);
                        }
                      }}
                    >
                      Add to current search
                    </ContextMenuItem>
                  </>
                )}
                <ContextMenuSeparator />
                {!synthetic && (
                  <>
                    <ContextMenuItem onSelect={() => void pin(space)}>
                      {space.pinned ? "Unpin space" : "Pin space"}
                    </ContextMenuItem>
                    <ContextMenuItem
                      onSelect={() => {
                        menuOpensEditor.current = true;
                        setEditing(space);
                      }}
                    >
                      Edit space…
                    </ContextMenuItem>
                    <ContextMenuSeparator />
                  </>
                )}
                {!group && onNewNote && (
                  <ContextMenuItem
                    disabled={space.error !== null}
                    onSelect={() => onNewNote(space)}
                  >
                    New note in this space
                  </ContextMenuItem>
                )}
                <ContextMenuItem
                  onSelect={() => {
                    menuOpensEditor.current = true;
                    setCreating(
                      (group || hasChildren) &&
                        !space.id.startsWith("keeper:all") &&
                        space.id !== TEMPORARY_SPACE_ID
                        ? `${space.name}/Untitled space`
                        : "Untitled space",
                    );
                  }}
                >
                  New space…
                </ContextMenuItem>
                {!synthetic && (
                  <>
                    <ContextMenuSeparator />
                    <ContextMenuItem
                      variant="destructive"
                      onSelect={() => {
                        menuOpensEditor.current = true;
                        setDeleting(space.id);
                      }}
                    >
                      Delete space…
                    </ContextMenuItem>
                  </>
                )}
              </ContextMenuContent>
            </ContextMenu>
            {hasChildren && expanded && (
              <ul id={childId} aria-label={`${label} children`}>
                {renderRows(space.id)}
              </ul>
            )}
          </li>
        </Fragment>
      );
    });
  };
  return (
    <>
      <ContextMenu>
        <ContextMenuTrigger asChild>
          <div>
            <FoldSection
              label="Spaces"
              icon={folded ? ChevronRight : ChevronDown}
              folded={folded}
              onToggle={() => notesRailFoldStore.getState().toggleGroup("spaces")}
              id="notes-rail-spaces"
              className="shrink-0"
              as="ul"
              bodyClassName="flex flex-col gap-0.5"
              actions={
                <IconHint label={RESTORE_DEFAULTS}>
                  <button
                    type="button"
                    aria-label={RESTORE_DEFAULTS}
                    disabled={vaultId === null || restoring}
                    onClick={() => void restore()}
                    className="shrink-0 rounded-md p-1 text-muted-foreground outline-none hover:bg-accent/50 focus-visible:ring-2 focus-visible:ring-ring disabled:pointer-events-none disabled:opacity-50"
                  >
                    <RotateCcw aria-hidden="true" className="size-3.5" />
                  </button>
                </IconHint>
              }
              notice={
                notice !== null && (
                  <p role="status" className="px-2 pb-1 text-muted-foreground text-xs">
                    {notice}
                  </p>
                )
              }
            >
              {renderRows(null)}
            </FoldSection>
          </div>
        </ContextMenuTrigger>
        <ContextMenuContent
          aria-label="Spaces actions"
          collisionPadding={8}
          className="w-60 max-w-[calc(100vw-16px)]"
        >
          <ContextMenuItem
            className={cn(phone ? "min-h-11" : "min-h-8")}
            disabled={vaultId === null}
            onSelect={() => setCreating("Untitled space")}
          >
            New space…
          </ContextMenuItem>
        </ContextMenuContent>
      </ContextMenu>
      {vaultId !== null && (editing !== null || creating !== null) && (
        <SpaceEditor
          key={editing?.id ?? "new"}
          vaultId={vaultId}
          space={editing ?? undefined}
          initialName={creating ?? undefined}
          onClose={() => {
            setEditing(null);
            setCreating(null);
          }}
          onSaved={(saved) => {
            setEditing(null);
            setCreating(null);
            void reload(saved);
          }}
        />
      )}
      {vaultId !== null && deleting !== null && (
        <NoteDeleteDialog
          key={deleting}
          vaultId={vaultId}
          noteId={deleting}
          onClose={() => setDeleting(null)}
          onDeleted={() => {
            const filters = notesFiltersStore.getState();
            if (filters.scope.kind === "space" && filters.scope.id === deleting)
              filters.setScope(ALL_NOTES_SCOPE);
            setDeleting(null);
            void reload();
          }}
        />
      )}
    </>
  );
}
