import { useState } from "react";

/** AD-280: menu ownership is transient and never changes row selection. */
export function useMenuTarget(): {
  targetId: string | null;
  onOpenChange(id: string): (open: boolean) => void;
  rowProps(id: string): { "data-menu-target"?: ""; "aria-expanded"?: boolean };
} {
  const [targetId, setTargetId] = useState<string | null>(null);
  return {
    targetId,
    onOpenChange: (id) => (open) => {
      setTargetId((current) => (open ? id : current === id ? null : current));
    },
    rowProps: (id) => (targetId === id ? { "data-menu-target": "", "aria-expanded": true } : {}),
  };
}

export const MENU_TARGET_RING =
  "data-[menu-target]:ring-2 data-[menu-target]:ring-inset data-[menu-target]:ring-ring";
