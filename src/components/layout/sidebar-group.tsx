/**
 * A section that folds (Story 45.20, FR-198, UX-DR81, UX-DR82; Story 47.3).
 *
 * SPACES and NETWORKS in the chat sidebar were the first two, and they shared
 * this rather than each growing a header of their own, because what is shared is
 * the part that would rot: a disclosure button in the tab order, an accessible
 * name that survives the rail, `aria-expanded` pointing at the region it opens,
 * and the rule that a folded section hides its rows and never its own control.
 * Two copies of that agree until the day one of them is edited, and the symptom
 * — a section you can fold and cannot unfold — is invisible to a test that only
 * ever renders the other one.
 *
 * Story 47.3 made the notes rail's three sections fold too. They did not fit the
 * chat-sidebar-shaped component: their bodies are trees rather than lists, one
 * of them owns the column's flexible height, one carries a second control in its
 * header, and they read a different store. So the component was split rather
 * than copied — {@link FoldSection} is the whole mechanism and knows no store,
 * and {@link FoldableGroup} is the two-line binding to the chat sidebar's.
 *
 * **Folding hides the rows, not the section.** The header stays, because the
 * only way back is through it, and because a header can carry a control the
 * folded rows are the way to reach — Spaces' "Restore default spaces" is exactly
 * that, so `actions` renders in the header row and stays reachable while folded.
 *
 * `hidden` rather than unmounting: the content is already loaded, the state is
 * remembered across restarts, and a `hidden` element is out of the tab order and
 * out of the accessibility tree, which is the whole requirement. It also gives
 * the flex behaviour a folded section needs for free — `display: none` takes the
 * body out of the column entirely, so a folded section stops claiming height and
 * its siblings grow into the space instead of a gap opening under it.
 */
import type { LucideIcon } from "lucide-react";
import type { ReactNode } from "react";
import { FOLD_STRIP } from "@/components/layout/fold-strip";
import { Button } from "@/components/ui/button";
import { Tooltip, TooltipContent, TooltipProvider, TooltipTrigger } from "@/components/ui/tooltip";
import { type SidebarGroup, sidebarFoldStore, useSidebarFold } from "@/lib/stores/sidebar-fold";
import { cn } from "@/lib/utils";

export function FoldSection({
  label,
  icon: Icon,
  folded,
  onToggle,
  id,
  collapsed = false,
  actions,
  notice,
  className,
  bodyClassName,
  as: Body = "div",
  children,
}: {
  /** The section's name: its accessible name, and its visible label. */
  label: string;
  /** The glyph beside the label. A chevron where the direction is the point. */
  icon: LucideIcon;
  /** Whether this section's rows are folded away right now. */
  folded: boolean;
  /** Fold or unfold. The caller owns where that is remembered. */
  onToggle: () => void;
  /** The id of the region the disclosure controls. Unique in the document. */
  id: string;
  /** Whether the whole surrounding menu is folded to an icon rail. */
  collapsed?: boolean;
  /**
   * Extra header controls, beside the disclosure and OUTSIDE the folded region.
   *
   * A sibling of the disclosure rather than a child: a button inside a button is
   * invalid HTML, and browsers recover from it by dropping one of the two.
   */
  actions?: ReactNode;
  /**
   * What `actions` had to say, under the header and OUTSIDE the folded region.
   *
   * A header control that stays reachable while folded needs somewhere to
   * report that stays readable while folded, or pressing it is a silent nothing
   * — which is exactly the shape of bug a fold introduces and a fold test that
   * only looks at the rows never sees.
   */
  notice?: ReactNode;
  /** Layout for the `<section>` itself. A section that owns the column's spare
   *  height must stop owning it when folded, and only the caller knows that. */
  className?: string;
  /** Layout for the region the fold hides. */
  bodyClassName?: string;
  /** `ul` when the children are `<li>` rows; `div` for anything else. */
  as?: "ul" | "div";
  children: ReactNode;
}) {
  const name = folded ? `Expand ${label}` : `Collapse ${label}`;
  const disclosure = (
    <Button
      type="button"
      variant="ghost"
      // The name carries the direction because on the rail there is no
      // visible text at all, and it CONTAINS the visible label so the two do
      // not disagree where there is (WCAG 2.5.3). `aria-expanded` is what
      // actually states the current state; the verb is what makes an
      // icon-only control say what pressing it does.
      aria-label={name}
      aria-expanded={!folded}
      aria-controls={id}
      data-slot="sidebar-group-fold"
      // On the rail this is one more item on a strip, so it is the strip's
      // item and not a squashed row: it used to be `h-auto py-1` with a 14px
      // glyph, ~22px tall among 36px nav buttons directly above it, which is
      // the rhythm changing halfway down the strip that the owner saw.
      size={collapsed ? FOLD_STRIP.controlSize : undefined}
      className={cn(
        // `text-muted-foreground`, not `text-faint`: this label is the
        // visible name of a control, and a control's own name is held to
        // 4.5:1 however label-like it looks.
        "label-caps text-muted-foreground",
        collapsed ? "shrink-0" : "h-auto min-w-0 flex-1 justify-start gap-2 px-2 py-1",
      )}
      onClick={onToggle}
    >
      <Icon aria-hidden="true" className={cn("shrink-0", !collapsed && "size-3.5")} />
      {!collapsed && label}
    </Button>
  );
  return (
    <section
      aria-label={label}
      data-fold-strip-items={collapsed ? "nested" : undefined}
      className={cn(
        "flex flex-col",
        // On the rail the strip's own container owns the inset and the rhythm
        // for every section on it — see `sidebar-pane.tsx`. A section that
        // brought its own would be the second opinion this module exists to
        // stop having.
        collapsed ? FOLD_STRIP.gapClass : "px-2 pb-1",
        className,
      )}
    >
      <div className={cn("flex shrink-0 items-center gap-1", collapsed && "justify-center")}>
        {collapsed ? (
          // Folded to a glyph, the tooltip is the only thing that says which
          // section this is — the same answer, in the same words, that the
          // strips around it give (`fold-strip.tsx`).
          //
          // Its own `TooltipProvider`, for the reason `surface-column.tsx`
          // gives for the same decision: a section that only names itself
          // inside the app shell is a section that goes silent in every other
          // host, and this one is rendered directly by its own suite.
          <TooltipProvider>
            <Tooltip>
              <TooltipTrigger asChild>{disclosure}</TooltipTrigger>
              <TooltipContent side="right">{name}</TooltipContent>
            </Tooltip>
          </TooltipProvider>
        ) : (
          disclosure
        )}
        {actions}
      </div>
      {notice}
      <Body
        id={id}
        hidden={folded}
        data-fold-strip-items={collapsed ? "nested" : undefined}
        className={bodyClassName}
      >
        {children}
      </Body>
    </section>
  );
}

/** {@link FoldSection} bound to the chat sidebar's remembered fold. */
export function FoldableGroup({
  label,
  icon,
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
  return (
    <FoldSection
      label={label}
      icon={icon}
      folded={folded}
      onToggle={() => sidebarFoldStore.getState().toggleGroup(group)}
      id={`sidebar-group-${group}`}
      collapsed={collapsed}
      as="ul"
      // On the rail these rows are items on the strip and take the strip's
      // rhythm. Off it they are text rows in a 260px drawer, where 2px is the
      // denser spacing a list of names wants — this `gap-0.5` is the one that
      // used to leak onto the strip and change its rhythm halfway down.
      bodyClassName={cn(
        "flex flex-col",
        collapsed ? cn(FOLD_STRIP.gapClass, "items-center") : "gap-0.5",
      )}
    >
      {children}
    </FoldSection>
  );
}
