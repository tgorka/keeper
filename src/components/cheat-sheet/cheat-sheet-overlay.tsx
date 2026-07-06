/**
 * Cheat-sheet overlay (Story 9.3, epic 9 spine).
 *
 * The ⌘? discovery surface: a searchable, read-only reference listing every shortcut
 * grouped by category, generated from the same action registry the palette consumes
 * (`cheat_sheet_sections` → `registry_sections()` → `palette_actions()`). There is no
 * hand-maintained list here — each toggle pair arrives already collapsed to one
 * unambiguous row (e.g. "Archive / Unarchive Chat" · `E`). The overlay fetches the
 * sections fresh on open (no TS store holds the reference), so adding/removing a
 * registry action changes this surface automatically (UX-DR15).
 *
 * This is a *reference*, not a runner: selecting a row does not dispatch. The search
 * box uses cmdk's own `shouldFilter` — a plain presentation filter over the static
 * reference list, NOT the Rust-authoritative chat/action ranking (AD-20 forbids
 * re-ranking *results*; this is not a result set). Modal depth ≤ 1: it is a single
 * overlay that owns Esc via the dialog.
 */
import { useEffect, useState } from "react";
import {
  Command,
  CommandDialog,
  CommandEmpty,
  CommandGroup,
  CommandInput,
  CommandItem,
  CommandList,
} from "@/components/ui/command";
import { Kbd } from "@/components/ui/kbd";
import type { MenuSectionVm } from "@/lib/ipc/client";
import { cheatSheetSections } from "@/lib/ipc/client";
import { useCheatSheetStore } from "@/lib/stores/cheat-sheet";

export function CheatSheetOverlay() {
  const isOpen = useCheatSheetStore((state) => state.isOpen);
  const close = useCheatSheetStore((state) => state.close);
  const [sections, setSections] = useState<MenuSectionVm[]>([]);

  // Fetch the reference fresh each time the overlay opens — no store mirrors it, so
  // the surface always reflects the current registry (UX-DR15). A fetch failure
  // leaves the last-known (or empty) sections and shows the empty state honestly.
  useEffect(() => {
    if (!isOpen) {
      return;
    }
    let cancelled = false;
    void cheatSheetSections()
      .then((next) => {
        if (!cancelled) {
          setSections(next);
        }
      })
      .catch(() => {
        // Read-only reference; a transient failure just shows no rows.
      });
    return () => {
      cancelled = true;
    };
  }, [isOpen]);

  const onOpenChange = (open: boolean) => {
    if (!open) {
      close();
    }
  };

  return (
    <CommandDialog
      open={isOpen}
      onOpenChange={onOpenChange}
      title="Keyboard Shortcuts"
      description="Every keeper shortcut, grouped by category."
      className="w-[560px] max-w-[560px]"
    >
      {/* cmdk's own substring filter over the static reference (presentation, not
          result ranking). Each toggle pair is already one row from the registry. */}
      <Command>
        <CommandInput placeholder="Search shortcuts…" />
        <CommandList>
          <CommandEmpty>No shortcuts found.</CommandEmpty>
          {sections.map((section) => (
            <CommandGroup key={section.category} heading={section.category}>
              {section.items.map((item) => (
                <CommandItem
                  key={item.id}
                  // A search value that also matches the category so a category-name
                  // query keeps the group's rows; selecting a row is a no-op (reference).
                  value={`${item.title} ${section.category} ${item.shortcut ?? ""}`}
                >
                  <span className="truncate">{item.title}</span>
                  {item.shortcut !== null && <Kbd className="ml-auto">{item.shortcut}</Kbd>}
                </CommandItem>
              ))}
            </CommandGroup>
          ))}
        </CommandList>
      </Command>
    </CommandDialog>
  );
}
