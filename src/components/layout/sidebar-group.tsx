/**
 * A sidebar submenu that folds (Story 45.20, FR-198, UX-DR81, UX-DR82).
 *
 * SPACES and NETWORKS are the two, and they share this rather than each growing
 * a header of their own, because what is shared is the part that would rot: a
 * disclosure button in the tab order, an accessible name that survives the rail,
 * `aria-expanded` pointing at the list it opens, and the rule that a folded
 * group hides its rows and never its own control. Two copies of that agree until
 * the day one of them is edited, and the symptom — a submenu you can fold and
 * cannot unfold — is invisible to a test that only ever renders the other one.
 *
 * **Folding hides the rows, not the group.** The header stays, because the only
 * way back is through it. `hidden` rather than unmounting: the list is small,
 * the state is remembered across restarts, and a `hidden` element is out of the
 * tab order and out of the accessibility tree, which is the whole requirement.
 */
import type { LucideIcon } from "lucide-react";
import type { ReactNode } from "react";
import { Button } from "@/components/ui/button";
import { type SidebarGroup, sidebarFoldStore, useSidebarFold } from "@/lib/stores/sidebar-fold";
import { cn } from "@/lib/utils";

export function FoldableGroup({
  label,
  icon: Icon,
  group,
  collapsed,
  children,
}: {
  /** The group's name, and the visible label when the menu is unfolded. */
  label: string;
  /** The glyph that stands for the group on the folded rail. */
  icon: LucideIcon;
  /** Which remembered fold this group reads and writes. */
  group: SidebarGroup;
  /** Whether the whole menu is folded to the icon rail. */
  collapsed: boolean;
  children: ReactNode;
}) {
  const folded = useSidebarFold((state) => state.groups[group]);
  const listId = `sidebar-group-${group}`;
  return (
    <section aria-label={label} className={cn("flex flex-col pb-1", collapsed ? "px-1" : "px-2")}>
      <Button
        type="button"
        variant="ghost"
        // The name carries the direction because on the rail there is no visible
        // text at all, and it CONTAINS the visible label so the two do not
        // disagree where there is (WCAG 2.5.3). `aria-expanded` is what actually
        // states the current state; the verb is what makes an icon-only control
        // say what pressing it does.
        aria-label={folded ? `Expand ${label}` : `Collapse ${label}`}
        aria-expanded={!folded}
        aria-controls={listId}
        data-slot="sidebar-group-fold"
        className={cn(
          "h-auto py-1 font-medium text-muted-foreground text-xs uppercase tracking-wide focus-visible:ring-2 focus-visible:ring-ring focus-visible:outline-none",
          collapsed ? "justify-center px-0" : "justify-start gap-2 px-2",
        )}
        onClick={() => sidebarFoldStore.getState().toggleGroup(group)}
      >
        <Icon aria-hidden="true" className="size-3.5 shrink-0" />
        {!collapsed && label}
      </Button>
      <ul
        id={listId}
        hidden={folded}
        className={cn("flex flex-col gap-0.5", collapsed && "items-center")}
      >
        {children}
      </ul>
    </section>
  );
}
