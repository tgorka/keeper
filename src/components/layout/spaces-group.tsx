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
 */

import { roomInitials } from "@/components/chat/RoomAvatar";
import { Avatar, AvatarFallback, AvatarImage } from "@/components/ui/avatar";
import type { SpaceVm } from "@/lib/ipc/client";
import { setSpaceFilter } from "@/lib/ipc/client";
import { spacesStore, useSpacesStore } from "@/lib/stores/spaces";
import { cn } from "@/lib/utils";

export function SpacesGroup() {
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
    <section aria-label="Spaces" className="flex flex-col px-2 pb-1">
      <span className="px-2 py-1 font-medium text-muted-foreground text-xs uppercase tracking-wide">
        Spaces
      </span>
      <ul className="flex flex-col gap-0.5">
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
                className={cn(
                  "flex w-full items-center gap-2 rounded-md px-2 py-1.5 text-left outline-none focus-visible:ring-2 focus-visible:ring-ring focus-visible:ring-inset",
                  isActive ? "bg-accent text-accent-foreground" : "hover:bg-accent",
                )}
              >
                <Avatar size="sm">
                  {httpAvatar !== null && <AvatarImage src={httpAvatar} alt="" />}
                  <AvatarFallback>{roomInitials(space.name)}</AvatarFallback>
                </Avatar>
                <span className="truncate text-sm">{space.name}</span>
              </button>
            </li>
          );
        })}
      </ul>
    </section>
  );
}
