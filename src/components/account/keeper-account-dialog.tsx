/**
 * The keeper-account entry, reached from the footer's Add account menu
 * (Epic 83, AD-318).
 *
 * Mounted once at the app root beside {@link AccountSetupSheet} and opened by
 * `accountStore.entryOpen`. It holds no field of its own: the body is the one
 * {@link SetupLinkField}, so a link pasted or scanned here takes exactly the
 * road it takes from Settings or the wizard — `openSetup` closes this dialog
 * and the confirmation sheet takes over. With an account already set up it
 * says, before anything else, what a new link does to it.
 */
import { SetupLinkField } from "@/components/account/account-setup-sheet";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import { accountStore, useAccountStore } from "@/lib/stores/account";

export const KEEPER_ACCOUNT_TITLE = "Set up a keeper account";
export const CHANGE_KEEPER_ACCOUNT_TITLE = "Change keeper account";
export const KEEPER_ACCOUNT_DESCRIPTION =
  "Paste the setup link you were given, or scan its QR code. keeper shows you where it signs in before anything is written.";

/**
 * What the dialog says when an account is already set up (amendment A1). It
 * cannot know yet whether the link is for another account — a link for the
 * same one replaces nothing — so it says what another account's link does
 * and leaves the verdict to the sheet, which hears it from Rust.
 */
export function changeKeeperAccountDescription(name: string): string {
  return `A link for another account replaces ${name} on this device; the next step says so before anything is written. Its files in the settings repository are kept.`;
}

export function KeeperAccountDialog() {
  const open = useAccountStore((s) => s.entryOpen);
  const configured = useAccountStore((s) => s.vm.configured);
  const name = useAccountStore((s) => s.vm.name);
  return (
    <Dialog
      open={open}
      onOpenChange={(next) => {
        if (!next) {
          accountStore.getState().closeEntry();
        }
      }}
    >
      {/* One size up from the default, so the field, Continue and the scan
          control sit on one row rather than the scan wrapping alone. */}
      <DialogContent className="sm:max-w-lg">
        <DialogHeader>
          <DialogTitle>
            {configured ? CHANGE_KEEPER_ACCOUNT_TITLE : KEEPER_ACCOUNT_TITLE}
          </DialogTitle>
          <DialogDescription>
            {configured
              ? changeKeeperAccountDescription(name ?? "the current account")
              : KEEPER_ACCOUNT_DESCRIPTION}
          </DialogDescription>
        </DialogHeader>
        <SetupLinkField id="keeper-account-link" />
      </DialogContent>
    </Dialog>
  );
}
