/**
 * Where a "Verify" button goes: straight into the verification flow for the
 * device that needs it.
 *
 * Both the global banner and the undecryptable-message stub used to set the
 * primary view to Settings and leave the person to find the verify control
 * there. On the phone that did nothing at all: a selected room outranks a view
 * level in the stack (`phone-shell.tsx`), so a Verify tapped inside a room
 * changed a view nobody could see — kalypso, 2026-10-18, with a whole room of
 * "Can't decrypt yet". The verification dialog is mounted once for the whole
 * app (`app-shell.tsx`), above every pane and every phone level, so opening it
 * works the same from anywhere.
 *
 * The first signed-in account whose device is `unverified` is the one asked
 * for. With none — the status has not arrived, or it changed under the tap —
 * Settings is still the honest destination: it says which account is in what
 * state.
 */
import { accountsStore } from "@/lib/stores/accounts";
import { encryptionStatusStore } from "@/lib/stores/encryption-status";
import { primaryViewStore } from "@/lib/stores/primary-view";
import { verificationStore } from "@/lib/stores/verification";

export function openDeviceVerification(): void {
  const { statuses } = encryptionStatusStore.getState();
  const unverified = accountsStore
    .getState()
    .accounts.find((account) => statuses[account.accountId] === "unverified");
  if (unverified) {
    verificationStore.getState().openFor(unverified.accountId);
    return;
  }
  primaryViewStore.getState().setView("settings");
}
