// biome-ignore-all lint/a11y/useSemanticElements: the rule maps role="group" to
// <fieldset>, a FORM grouping element. The WAI-ARIA tree pattern requires the
// nested container of a treeitem to carry role="group"; a fieldset there would
// break the tree for every screen reader that implements the pattern. The rule
// is suppressed for the file because the element sits inside a JSX `&&` child,
// where no per-node suppression comment can be placed.
/**
 * The FILES lens (Epic 37, Story 37.9, FR-106, UX-DR38).
 *
 * The physical folder tree, collapsed by default and always exactly one click
 * away. It is not a nicety: the failure mode of every virtual-organisation
 * system is that people stop believing they know where their files are, and then
 * stop trusting the tool with their files. This tree and the per-row reveal are
 * the two antidotes, and both are mandatory.
 *
 * Levels load lazily, one `notes_tree` call per expanded directory, because a
 * vault's folder tree is unbounded and walking all of it to render a collapsed
 * group would be a cold scan the user did not ask for.
 *
 * **The fold is remembered** (Story 47.3). It still arrives collapsed on a
 * keeper that has never touched it — an open default would make every mount of
 * the notes surface a cold directory read — but once it is opened it stays
 * open across surface switches and restarts, in the notes rail's own cookie
 * alongside Spaces and Tags.
 *
 * Selecting a folder sets a folder scope. That scope is the one the list does
 * NOT serve from `notes_list`: a vault-relative directory is not one of the
 * query's axes, so FR-106's own command returns the folder's rows and the pane
 * reads them from there. Still a filter, still never a navigation (UX-DR41).
 */
import { ChevronDown, ChevronRight, Folder } from "lucide-react";
import { useEffect, useState } from "react";
import { FoldSection } from "@/components/layout/sidebar-group";
import { notesTree } from "@/lib/ipc/client";
import { notesFiltersStore, useNotesFiltersStore } from "@/lib/stores/notes-filters";
import { notesRailFoldStore, useNotesRailFold } from "@/lib/stores/notes-rail-fold";
import { cn } from "@/lib/utils";

/** Join a parent directory and a child name into a vault-relative path. */
function childPath(parent: string, name: string): string {
  return parent === "" ? name : `${parent}/${name}`;
}

function FolderNode({
  vaultId,
  path,
  name,
  level,
  posInSet,
  setSize,
  activePath,
}: {
  vaultId: string;
  path: string;
  name: string;
  level: number;
  posInSet: number;
  setSize: number;
  activePath: string | null;
}) {
  const [open, setOpen] = useState(false);
  const [dirs, setDirs] = useState<string[] | null>(null);

  useEffect(() => {
    if (!open || dirs !== null) {
      return;
    }
    let cancelled = false;
    void notesTree(vaultId, path)
      .then((folder) => {
        if (!cancelled) {
          setDirs(folder.dirs);
        }
      })
      .catch(() => {
        // An unreadable directory renders as a leaf rather than as an error: the
        // rest of the tree is still true, and the row can still be selected.
        if (!cancelled) {
          setDirs([]);
        }
      });
    return () => {
      cancelled = true;
    };
  }, [open, dirs, vaultId, path]);

  const active = activePath === path;

  return (
    <div
      tabIndex={-1}
      role="treeitem"
      aria-level={level}
      aria-posinset={posInSet}
      aria-setsize={setSize}
      aria-expanded={open}
      aria-selected={active}
    >
      <div className="flex items-center">
        <button
          type="button"
          tabIndex={-1}
          aria-hidden="true"
          className="shrink-0 rounded-sm p-0.5 text-muted-foreground outline-none hover:bg-accent"
          onClick={() => setOpen((shown) => !shown)}
        >
          {open ? <ChevronDown className="size-3" /> : <ChevronRight className="size-3" />}
        </button>
        <button
          type="button"
          aria-label={`Folder ${path}, filter`}
          aria-pressed={active}
          className={cn(
            "flex min-w-0 flex-1 items-center gap-1.5 rounded-md px-1.5 py-1 text-left outline-none",
            "focus-visible:ring-2 focus-visible:ring-ring focus-visible:ring-inset",
            active ? "bg-accent text-accent-foreground" : "hover:bg-accent/50",
          )}
          onClick={() => notesFiltersStore.getState().setScope({ kind: "folder", path })}
        >
          <Folder aria-hidden="true" className="size-3 shrink-0 text-muted-foreground" />
          <span className="min-w-0 truncate text-sm">{name}</span>
        </button>
      </div>
      {open && dirs !== null && dirs.length > 0 && (
        <div role="group" className="pl-3">
          {dirs.map((dir, index) => (
            <FolderNode
              key={dir}
              vaultId={vaultId}
              path={childPath(path, dir)}
              name={dir}
              level={level + 1}
              posInSet={index + 1}
              setSize={dirs.length}
              activePath={activePath}
            />
          ))}
        </div>
      )}
    </div>
  );
}

export function PhysicalTree({ vaultId }: { vaultId: string | null }) {
  // Story 47.3: the fold lives in the rail's cookie, not in this component. It
  // was a `useState` here, which meant the tree re-collapsed on every surface
  // switch — and it was the ONLY foldable thing in the whole notes rail, which
  // is how a rail with three sections shipped with one control.
  const folded = useNotesRailFold((state) => state.groups.files);
  const [dirs, setDirs] = useState<string[] | null>(null);
  const activePath = useNotesFiltersStore((s) => (s.scope.kind === "folder" ? s.scope.path : null));

  // `vaultId` is the reason this effect exists, not a value it reads. Dropping it
  // leaves the previous vault's directory names under the new vault's heading, which
  // is the most confusing stale state this tree can show.
  // biome-ignore lint/correctness/useExhaustiveDependencies: reset trigger, not a read
  useEffect(() => {
    // Reset on a vault switch: another vault's directory names under this
    // vault's heading would be the most confusing possible stale state.
    setDirs(null);
  }, [vaultId]);

  useEffect(() => {
    // Still lazy, and now lazy against the remembered fold: a keeper that comes
    // up with Files shut does not walk the vault's root, and one that comes up
    // with it open reads exactly one level, as a click always has.
    if (folded || dirs !== null || vaultId === null) {
      return;
    }
    let cancelled = false;
    void notesTree(vaultId, "")
      .then((folder) => {
        if (!cancelled) {
          setDirs(folder.dirs);
        }
      })
      .catch(() => {
        if (!cancelled) {
          setDirs([]);
        }
      });
    return () => {
      cancelled = true;
    };
  }, [folded, dirs, vaultId]);

  if (vaultId === null) {
    return null;
  }

  return (
    <FoldSection
      label="Files"
      icon={folded ? ChevronRight : ChevronDown}
      folded={folded}
      onToggle={() => notesRailFoldStore.getState().toggleGroup("files")}
      id="notes-rail-files"
      className="shrink-0"
      bodyClassName="max-h-48 overflow-y-auto"
    >
      {dirs !== null && (
        <div aria-label="Folder tree" role="tree">
          {dirs.map((dir, index) => (
            <FolderNode
              key={dir}
              vaultId={vaultId}
              path={dir}
              name={dir}
              level={1}
              posInSet={index + 1}
              setSize={dirs.length}
              activePath={activePath}
            />
          ))}
        </div>
      )}
    </FoldSection>
  );
}
