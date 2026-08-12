/**
 * SPACES sidebar group (FR-22, UX-DR, Story 4.5).
 *
 * A labeled `section-label` group of single-select rows, one per Matrix Space the
 * user belongs to (across all accounts), in the Rust-authoritative order the
 * {@link spacesStore} mirror holds. Selecting a Space filters the Unified Inbox to
 * that Space's joined rooms; the filtering itself is computed in Rust (poked via
 * {@link setSpaceFilter}) — this component only records the selection and reflects
 * it as `aria-current` / accent styling.
 *
 * Single-select toggle: clicking the active row clears the filter; clicking any
 * other row selects it. The group is hidden entirely (`return null`) when the
 * aggregated Space list is empty (UX-DR — no label, no rows).
 *
 * **It folds, and it survives the menu folding** (Story 45.20, UX-DR81). The
 * group used to be dropped from the collapsed rail outright, on the grounds
 * that it "needs labels + names" — so folding the drawer silently removed a
 * navigation surface instead of shrinking one. On the rail each row is its
 * Space's avatar carrying the Space's name as its accessible name, which is the
 * rail every chat app draws and is a name rather than an unlabelled glyph. No
 * tooltip: the avatar already shows the Space's own initials, and a tooltip
 * would restate the accessible name a second time over a control whose other
 * gesture is a press.
 *
 * The group's own fold is remembered separately from the menu's, because "give
 * me the width back" and "I do not care about Spaces today" are two asks.
 */

import { Layers } from "lucide-react";
import { roomInitials } from "@/components/chat/RoomAvatar";
import { FOLD_STRIP } from "@/components/layout/fold-strip";
import { FoldableGroup } from "@/components/layout/sidebar-group";
import { Avatar, AvatarFallback, AvatarImage } from "@/components/ui/avatar";
import type { SpaceVm } from "@/lib/ipc/client";
import { setSpaceFilter } from "@/lib/ipc/client";
import { spacesStore, useSpacesStore } from "@/lib/stores/spaces";
import { cn } from "@/lib/utils";

export function SpacesGroup({ collapsed = false }: { collapsed?: boolean }) {
  const spaces = useSpacesStore((s) => s.spaces);
  const activeSpace = useSpacesStore((s) => s.activeSpace);

  // Hidden entirely when there are no Spaces (UX-DR): no label, no rows.
  if (spaces.length === 0) {
    return null;
  }

  const onRowClick = (space: SpaceVm) => {
    const isActive =
      activeSpace?.accountId === space.accountId && activeSpace?.spaceId === space.spaceId;
    if (isActive) {
      // Toggle off: clear the selection and the Rust filter.
      spacesStore.getState().setActiveSpace(null);
      void setSpaceFilter(null, null).catch(() => {});
    } else {
      const selection = { accountId: space.accountId, spaceId: space.spaceId };
      spacesStore.getState().setActiveSpace(selection);
      void setSpaceFilter(space.accountId, space.spaceId).catch(() => {});
    }
  };

  return (
    <FoldableGroup label="Spaces" icon={Layers} group="spaces" collapsed={collapsed}>
      {spaces.map((space) => {
        const isActive =
          activeSpace?.accountId === space.accountId && activeSpace?.spaceId === space.spaceId;
        const httpAvatar =
          space.avatarUrl && /^https?:\/\//.test(space.avatarUrl) ? space.avatarUrl : null;
        return (
          <li key={`${space.accountId}:${space.spaceId}`}>
            <button
              type="button"
              onClick={() => onRowClick(space)}
              aria-current={isActive ? "true" : undefined}
              aria-pressed={isActive}
              // The name is the Space's own in BOTH renderings. On the rail it
              // is the only carrier of the name, which is the whole difference
              // between a folded menu and a strip of glyphs; unfolded it is
              // identical to the visible text, so the two cannot come apart.
              aria-label={space.name}
              className={cn(
                "flex items-center rounded-md text-left outline-none focus-visible:ring-2 focus-visible:ring-ring focus-visible:ring-inset",
                // On the rail this row is one item on the strip, at the strip's
                // item size. It used to be `p-1.5` around a 24px avatar with a
                // paragraph explaining that 24+6+6 is 36 — a sum that stops
                // being 36 the day the avatar changes, and a hover pill 4px
                // narrower than its neighbour's when it did.
                collapsed
                  ? cn("justify-center", FOLD_STRIP.controlClass)
                  : "w-full gap-2 px-2 py-1.5",
                isActive ? "bg-accent text-accent-foreground" : "hover:bg-accent",
              )}
            >
              <Avatar size="sm">
                {httpAvatar !== null && <AvatarImage src={httpAvatar} alt="" />}
                <AvatarFallback>{roomInitials(space.name)}</AvatarFallback>
              </Avatar>
              {!collapsed && <span className="truncate text-sm">{space.name}</span>}
            </button>
          </li>
        );
      })}
    </FoldableGroup>
  );
}
