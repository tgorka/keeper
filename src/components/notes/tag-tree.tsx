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
 * The sign alone changes polarity, the label is inert, and × removes. Tree
 * choices share the bar's chip grammar without clearing unrelated filters.
 *
 * The tree is unbounded, so it owns its own scroll container and pairs
 * `min-h-0 flex-1` with it (the AD-34-4 rule): however many tags a vault grows,
 * everything below this in the column stays reachable.
 */
import { ChevronDown, ChevronRight, Plus } from "lucide-react";
import { useEffect, useState } from "react";
import { FoldSection } from "@/components/layout/sidebar-group";
import { TagFilterChip } from "@/components/notes/note-filter-bar";
import { DriveHeading } from "@/components/notes/space-list";
import type { NoteTagNodeVm } from "@/lib/ipc/client";
import { notesTagTree } from "@/lib/ipc/client";
import {
  notesFiltersStore,
  type TagChip,
  tagChipState,
  useNotesFiltersStore,
} from "@/lib/stores/notes-filters";
import { notesRailFoldStore, useNotesRailFold } from "@/lib/stores/notes-rail-fold";

function TagNode({
  node,
  level,
  posInSet,
  setSize,
  expanded,
  tagTerms,
  onToggleExpanded,
}: {
  node: NoteTagNodeVm;
  level: number;
  posInSet: number;
  setSize: number;
  expanded: ReadonlySet<string>;
  tagTerms: readonly TagChip[];
  onToggleExpanded: (path: string) => void;
}) {
  const hasChildren = node.children.length > 0;
  const isOpen = expanded.has(node.path);
  const term = tagChipState(tagTerms, node.path);

  return (
    <div
      tabIndex={-1}
      role="treeitem"
      aria-level={level}
      aria-posinset={posInSet}
      aria-setsize={setSize}
      aria-expanded={hasChildren ? isOpen : undefined}
      aria-selected={term === "include"}
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
        <div className="flex min-w-0 flex-1 items-center gap-1">
          {term === "off" ? (
            <span className="inline-flex min-w-0 items-center gap-1 text-sm">
              <button
                type="button"
                aria-label={`Include tag ${node.path}`}
                className="flex size-6 shrink-0 items-center justify-center rounded-full outline-none hover:bg-accent focus-visible:ring-2 focus-visible:ring-ring"
                onClick={() => notesFiltersStore.getState().setTagTerm(node.path, "include")}
              >
                <Plus aria-hidden="true" className="size-3" />
              </button>
              <span className="truncate">{node.name}</span>
            </span>
          ) : (
            <TagFilterChip
              chip={{ tag: node.path, term }}
              tabStop
              onToggleSign={() => notesFiltersStore.getState().toggleTagSign(node.path)}
              onRemove={() => notesFiltersStore.getState().removeTag(node.path)}
            />
          )}
          <span className="ml-auto shrink-0 text-muted-foreground text-xs">
            {node.count}
            <span className="sr-only"> items</span>
          </span>
        </div>
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
              tagTerms={tagTerms}
              onToggleExpanded={onToggleExpanded}
            />
          ))}
        </div>
      )}
    </div>
  );
}

export function TagTree({ vaultId, driveName }: { vaultId: string | null; driveName?: string }) {
  const [nodes, setNodes] = useState<NoteTagNodeVm[]>([]);
  const [expanded, setExpanded] = useState<ReadonlySet<string>>(() => new Set());
  const tagTerms = useNotesFiltersStore((s) => s.tagTerms);
  const folded = useNotesRailFold((state) => state.groups.tags);

  useEffect(() => {
    if (vaultId === null) {
      setNodes([]);
      return;
    }
    let cancelled = false;
    setNodes([]);
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
    // The section owns the column's spare height while it is open and gives all
    // of it back when it is shut. Both halves matter: `flex-1` on a folded
    // section would leave an empty stripe where the tree used to be, and the
    // body's own `hidden` only takes the BODY out of the layout, not the
    // section around it (Story 47.3, AD-34-4).
    <FoldSection
      label="Tags"
      icon={folded ? ChevronRight : ChevronDown}
      folded={folded}
      onToggle={() => notesRailFoldStore.getState().toggleGroup("tags")}
      id={`notes-rail-tags-${vaultId}`}
      // `flex-1` without `min-h-0`, and the missing half is the point. A flex
      // item with `min-height: 0` may be laid out shorter than its own contents,
      // and this section's header is `shrink-0` — so when the rail ran out of
      // room the section's box shrank, the header inside it did not, and the
      // header painted over the section below. That is the TAGS and FILES
      // captions sitting on top of each other at the foot of the rail.
      //
      // Left at `auto`, the section's floor is its own min-content height: the
      // header, since the body below carries `min-h-0` and may still collapse to
      // nothing. Sections shrink to their captions and no further, and the
      // scroll container around them takes over from there.
      className={folded ? "shrink-0" : "flex-1"}
      bodyClassName="flex min-h-0 flex-1 flex-col"
    >
      {driveName && <DriveHeading name={driveName} />}
      <div aria-label="Tag tree" className="min-h-0 flex-1 overflow-y-auto" role="tree">
        {nodes.map((node, index) => (
          <TagNode
            key={node.path}
            node={node}
            level={1}
            posInSet={index + 1}
            setSize={nodes.length}
            expanded={expanded}
            tagTerms={tagTerms}
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
    </FoldSection>
  );
}
