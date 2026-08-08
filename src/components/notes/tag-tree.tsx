// biome-ignore-all lint/a11y/useSemanticElements: the rule maps role="group" to
// <fieldset>, a FORM grouping element. The WAI-ARIA tree pattern requires the
// nested container of a treeitem to carry role="group"; a fieldset there would
// break the tree for every screen reader that implements the pattern. The rule
// is suppressed for the file because the element sits inside a JSX `&&` child,
// where no per-node suppression comment can be placed.
/**
 * The TAGS lens (Epic 37, Story 37.3, FR-104; Story 42.5, FR-143).
 *
 * A hierarchical tree with per-node counts, built in Rust. Tags arrive
 * normalised — lower-case, slash-separated, no leading `#` — so nothing here
 * re-cases or re-splits them: the tree is rendered, not derived.
 *
 * **A count is the sum of every producer behind it (Story 42.5).** It was once
 * the union of each note's frontmatter `tags` and its inline `#a/b` tags;
 * recording tags now feed the same posting map through the same entry point,
 * so a node reading 5 means 5 things — say 2 notes and 3 recordings — not 5
 * notes and some recordings it declined to mention. Nothing on this side
 * filters or re-adds: whatever Rust counted is what renders, which is why the
 * sum arrives here for free and why the accessible name says "items".
 *
 * **The counts are of the unfiltered vault.** They do not shrink as chips are
 * added, and that is deliberate: a count that changes meaning mid-interaction is
 * a lie, and the number people want from this tree is "how much is under here",
 * not "how much survives what I have already narrowed to".
 *
 * Selecting a node sets that tag as the filter; ⇧-selecting adds it to the
 * intersection. Both are filters, never navigations (UX-DR41) — the open note
 * stays open even when the new intersection excludes its row.
 *
 * The tree is unbounded, so it owns its own scroll container and pairs
 * `min-h-0 flex-1` with it (the AD-34-4 rule): however many tags a vault grows,
 * everything below this in the column stays reachable.
 */
import { ChevronDown, ChevronRight } from "lucide-react";
import { useEffect, useState } from "react";
import type { NoteTagNodeVm } from "@/lib/ipc/client";
import { notesTagTree } from "@/lib/ipc/client";
import { notesFiltersStore, useNotesFiltersStore } from "@/lib/stores/notes-filters";
import { cn } from "@/lib/utils";

function TagNode({
  node,
  level,
  posInSet,
  setSize,
  expanded,
  activeTags,
  onToggleExpanded,
}: {
  node: NoteTagNodeVm;
  level: number;
  posInSet: number;
  setSize: number;
  expanded: ReadonlySet<string>;
  activeTags: readonly string[];
  onToggleExpanded: (path: string) => void;
}) {
  const hasChildren = node.children.length > 0;
  const isOpen = expanded.has(node.path);
  const active = activeTags.includes(node.path);

  return (
    <div
      tabIndex={-1}
      role="treeitem"
      aria-level={level}
      aria-posinset={posInSet}
      aria-setsize={setSize}
      aria-expanded={hasChildren ? isOpen : undefined}
      aria-selected={active}
    >
      <div className="flex items-center">
        {hasChildren ? (
          <button
            type="button"
            tabIndex={-1}
            aria-hidden="true"
            className="shrink-0 rounded-sm p-0.5 text-muted-foreground outline-none hover:bg-accent"
            onClick={() => onToggleExpanded(node.path)}
          >
            {isOpen ? <ChevronDown className="size-3" /> : <ChevronRight className="size-3" />}
          </button>
        ) : (
          <span aria-hidden="true" className="size-4 shrink-0" />
        )}
        <button
          type="button"
          // The count belongs in the accessible name, not in a visually adjacent
          // orphan a screen reader would read as a separate number.
          // "items", not "notes": since Story 42.5 the number behind a node is
          // notes plus recordings, and a name that says otherwise is the exact
          // half-truth this story deleted.
          aria-label={`Tag ${node.path}, ${node.count} items, filter`}
          aria-pressed={active}
          className={cn(
            "flex min-w-0 flex-1 items-center gap-1 rounded-md px-1.5 py-1 text-left outline-none",
            "focus-visible:ring-2 focus-visible:ring-ring focus-visible:ring-inset",
            active ? "bg-accent text-accent-foreground" : "hover:bg-accent/50",
          )}
          onClick={(event) => {
            const filters = notesFiltersStore.getState();
            if (event.shiftKey) {
              // Add to the intersection rather than replacing it: two chips mean
              // both, which is the whole contract of this bar.
              filters.toggleTag(node.path);
              return;
            }
            filters.clearAll();
            filters.toggleTag(node.path);
          }}
        >
          <span className="min-w-0 truncate text-sm">{node.name}</span>
          <span aria-hidden="true" className="ml-auto shrink-0 text-muted-foreground text-xs">
            {node.count}
          </span>
        </button>
      </div>
      {hasChildren && isOpen && (
        <div role="group" className="pl-3">
          {node.children.map((child, index) => (
            <TagNode
              key={child.path}
              node={child}
              level={level + 1}
              posInSet={index + 1}
              setSize={node.children.length}
              expanded={expanded}
              activeTags={activeTags}
              onToggleExpanded={onToggleExpanded}
            />
          ))}
        </div>
      )}
    </div>
  );
}

export function TagTree({ vaultId }: { vaultId: string | null }) {
  const [nodes, setNodes] = useState<NoteTagNodeVm[]>([]);
  const [expanded, setExpanded] = useState<ReadonlySet<string>>(() => new Set());
  const activeTags = useNotesFiltersStore((s) => s.tags);

  useEffect(() => {
    if (vaultId === null) {
      setNodes([]);
      return;
    }
    let cancelled = false;
    void notesTagTree(vaultId)
      .then((tree) => {
        if (!cancelled) {
          setNodes(tree.nodes);
        }
      })
      .catch(() => {
        // A tree that will not load leaves the section empty rather than
        // breaking the column: the list beside it still works, and tags are one
        // way in among several.
      });
    return () => {
      cancelled = true;
    };
  }, [vaultId]);

  if (nodes.length === 0) {
    return null;
  }

  return (
    <section aria-label="Tags" className="flex min-h-0 flex-1 flex-col px-2 pb-1">
      <span className="px-2 py-1 font-medium text-muted-foreground text-xs uppercase tracking-wide">
        Tags
      </span>
      <div aria-label="Tag tree" className="min-h-0 flex-1 overflow-y-auto" role="tree">
        {nodes.map((node, index) => (
          <TagNode
            key={node.path}
            node={node}
            level={1}
            posInSet={index + 1}
            setSize={nodes.length}
            expanded={expanded}
            activeTags={activeTags}
            onToggleExpanded={(path) =>
              setExpanded((live) => {
                const next = new Set(live);
                if (!next.delete(path)) {
                  next.add(path);
                }
                return next;
              })
            }
          />
        ))}
      </div>
    </section>
  );
}
