/**
 * The account's one-line status beside the sync status (Epic 82, UX-DR116 (4)).
 *
 * It exists for the three states a person has to know about without opening
 * Settings — offline (settings are yesterday's), sign in again (they will stop
 * following), blocked (they were not loaded) — and renders NOTHING otherwise:
 * no account, signed out, syncing and up to date are all silent, because a
 * standing "Up to date" line is noise that trains people to stop reading the
 * one that matters.
 *
 * Its shape is the offline pill's, beside which it sits: the amber `held`
 * tokens, `role="status"`, no toast and nothing modal. The sidebar footer
 * draws it as a strip ({@link AccountStatusLine}); the phone shell, which is
 * also what a desktop window narrower than the sidebar breakpoint renders,
 * draws it as a pill under the header ({@link AccountStatusPill}). The words
 * are Rust's sentence and only Rust's: Rust writes one for every state shown
 * here, so a VM without one shows nothing rather than a TypeScript paraphrase.
 */
import { CloudOff, LogIn, ShieldAlert } from "lucide-react";
import type { AccountStateVm } from "@/lib/ipc/client";
import { useAccountStore } from "@/lib/stores/account";

type Shown = Extract<AccountStateVm, "offline" | "needsSignIn" | "blocked">;

const ICON: Record<Shown, typeof CloudOff> = {
  offline: CloudOff,
  needsSignIn: LogIn,
  blocked: ShieldAlert,
};

/** What the line would say now, or `null` when it stays silent. */
export function useAccountStatusShown(): { state: Shown; sentence: string } | null {
  const state = useAccountStore((s) => s.vm.state);
  const sentence = useAccountStore((s) => s.vm.sentence);
  if (
    (state !== "offline" && state !== "needsSignIn" && state !== "blocked") ||
    sentence === null
  ) {
    return null;
  }
  return { state, sentence };
}

export function AccountStatusLine({ collapsed }: { collapsed: boolean }) {
  const shown = useAccountStatusShown();
  if (shown === null) {
    return null;
  }
  const Icon = ICON[shown.state];
  return collapsed ? (
    <div
      role="status"
      aria-label={shown.sentence}
      data-account-status={shown.state}
      className="flex shrink-0 items-center justify-center border-border border-t bg-held/10 p-3 text-held"
    >
      <Icon aria-hidden="true" className="size-5" />
      <span className="sr-only">{shown.sentence}</span>
    </div>
  ) : (
    <div
      role="status"
      data-account-status={shown.state}
      className="flex shrink-0 items-start gap-2 border-border border-t bg-held/10 p-3 text-held text-xs"
    >
      <Icon aria-hidden="true" className="mt-0.5 size-4 shrink-0" />
      <span>{shown.sentence}</span>
    </div>
  );
}

/**
 * The same line in the phone header's pill slot. Rust's sentences run longer
 * than the offline pill's (the blocked one names a file), so the pill wraps
 * inside the band's width with a rounded rectangle rather than a capsule that
 * would clip the text.
 */
export function AccountStatusPill() {
  const shown = useAccountStatusShown();
  if (shown === null) {
    return null;
  }
  const Icon = ICON[shown.state];
  return (
    <div
      role="status"
      data-account-status={shown.state}
      className="mx-3 flex max-w-md items-start gap-2 rounded-2xl bg-held/10 px-3 py-1.5 text-held text-xs shadow-xs"
    >
      <Icon aria-hidden="true" className="mt-px size-4 shrink-0" />
      <span className="min-w-0">{shown.sentence}</span>
    </div>
  );
}
