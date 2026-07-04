import type { AccountVm } from "@/lib/ipc/client";

/**
 * True when {@link account} is a Beeper account, identified by its durable
 * `provider` tag (Story 2.5).
 *
 * The tag is stamped once at add time by the authenticating provider and
 * persisted in the non-secret `keeper.db` registry (a legacy row is migrated by
 * a one-time inference on restore). Identity is therefore durable and no longer
 * coupled to the resolved homeserver host — were Beeper to redirect its
 * `.well-known` to a different host, Beeper accounts would still be recognized.
 */
export function isBeeperAccount(account: AccountVm): boolean {
  return account.provider === "beeper";
}
