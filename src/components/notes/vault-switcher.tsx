/**
 * The vault switcher (Epic 37, Story 37.1, FR-95, UX-DR36, UX-DR41).
 *
 * The account switcher's component and affordance, verbatim: a mark, the name, a
 * state glyph, and a `DropdownMenu` carrying every vault, `Vault settings…`, and
 * — always last, never gated by count — `Add a notes vault…`. A vault is the
 * notes scope in exactly the way an account is the message scope, so it takes
 * that shape rather than inventing a picker.
 *
 * It sits at the head of the notes scope column rather than replacing the
 * account switcher in the sidebar footer, because the account switcher is
 * app-global and swapping it per view would make a global control's identity
 * depend on which view happens to be open.
 *
 * **Switching is a filter** (UX-DR41, FR-95): the list re-lists and the editor
 * keeps whatever it had if that note belongs to the new vault. Never a reload,
 * never a spinner over the frame — which is why nothing here unmounts anything
 * and the switch is one `notes_vault_set_active` round trip.
 *
 * The state glyph is a projection of `indexed`, and the honest reading of that
 * flag matters: before a cold scan finishes, `noteCount` is the best so far and
 * not a total, so the row says the vault is still being read rather than showing
 * a number that will change under the user.
 */
import { Check, Loader2, NotebookPen, Plus, Settings } from "lucide-react";
import { useEffect, useState } from "react";
import { Button } from "@/components/ui/button";
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuSeparator,
  DropdownMenuTrigger,
} from "@/components/ui/dropdown-menu";
import type { NoteVaultVm } from "@/lib/ipc/client";
import { setActiveVault, useNotesVaultsStore } from "@/lib/stores/notes-vaults";
import { primaryViewStore } from "@/lib/stores/primary-view";
import { cn } from "@/lib/utils";

/** The two entries below the vault rows, worded exactly as the menu shows them. */
export const VAULT_SETTINGS_LABEL = "Vault settings…";
export const ADD_VAULT_LABEL = "Add a notes vault…";

/**
 * The vault's index state as a glyph. A spinner while the cold scan is running,
 * a check once it has finished — shape, never colour (UX-DR43), so it reads on a
 * monochrome panel and in both themes.
 */
function IndexGlyph({ vault }: { vault: NoteVaultVm }) {
  if (!vault.indexed) {
    return (
      <Loader2
        aria-label="Reading this vault"
        className="size-3.5 shrink-0 animate-spin text-muted-foreground"
      />
    );
  }
  return <Check aria-label="Ready" className="size-3.5 shrink-0 text-muted-foreground" />;
}

export function VaultSwitcher() {
  const vaults = useNotesVaultsStore((s) => s.vaults);
  const activeVaultId = useNotesVaultsStore((s) => s.activeVaultId);
  const switcherNonce = useNotesVaultsStore((s) => s.switcherNonce);
  const [open, setOpen] = useState(false);

  // `⌘⌥V` and the palette's Switch Vault open this menu. The menu's open state
  // belongs to the `DropdownMenu`, so the keyboard path bumps a nonce and the
  // component opens itself — rather than two owners disagreeing about a menu.
  useEffect(() => {
    if (switcherNonce > 0) {
      setOpen(true);
    }
  }, [switcherNonce]);

  // Nothing to switch between and nothing read yet: the pane's own no-vault
  // state is the surface that speaks here, and a switcher naming nothing would
  // just be furniture above it.
  if (vaults === null || vaults.length === 0) {
    return null;
  }
  const active = vaults.find((vault) => vault.id === activeVaultId) ?? null;

  return (
    <DropdownMenu open={open} onOpenChange={setOpen}>
      <DropdownMenuTrigger asChild>
        <Button
          type="button"
          variant="ghost"
          aria-label={active === null ? "Choose a vault" : `Vault ${active.name}`}
          className="w-full justify-start gap-2"
        >
          <NotebookPen aria-hidden="true" className="shrink-0" />
          <span className="min-w-0 truncate text-sm">{active?.name ?? "Choose a vault"}</span>
          {active !== null && <IndexGlyph vault={active} />}
        </Button>
      </DropdownMenuTrigger>
      <DropdownMenuContent align="start" className="w-64">
        {vaults.map((vault) => (
          <DropdownMenuItem
            key={vault.id}
            onSelect={() => {
              void setActiveVault(vault.id);
            }}
          >
            <span className="min-w-0 truncate">{vault.name}</span>
            {/* The unread count rides the row it belongs to, so switching to the
                vault an agent has been writing in is one glance and one press. */}
            {vault.unreadCount > 0 && (
              <span className="ml-auto shrink-0 text-muted-foreground text-xs">
                {vault.unreadCount}
              </span>
            )}
            {vault.id === activeVaultId && (
              <Check
                aria-hidden="true"
                className={cn(vault.unreadCount > 0 ? "ml-1" : "ml-auto")}
              />
            )}
          </DropdownMenuItem>
        ))}
        <DropdownMenuSeparator />
        {/* One editor, two doors: the knobs live in the folder's own card in
            Settings → Sync, and a second copy here would be a drift surface. */}
        <DropdownMenuItem onSelect={() => primaryViewStore.getState().setView("sync")}>
          <Settings aria-hidden="true" />
          {VAULT_SETTINGS_LABEL}
        </DropdownMenuItem>
        {/* Always last and never gated by count — the account switcher's rule. */}
        <DropdownMenuItem onSelect={() => primaryViewStore.getState().setView("sync")}>
          <Plus aria-hidden="true" />
          {ADD_VAULT_LABEL}
        </DropdownMenuItem>
      </DropdownMenuContent>
    </DropdownMenu>
  );
}
