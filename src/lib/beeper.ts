import type { AccountVm } from "@/lib/ipc/client";

/**
 * The homeserver host that identifies a Beeper account. Beeper logins persist
 * as a plain `StoredSession::Password`, so after login a Beeper account is
 * indistinguishable from any other password/OIDC account except by its
 * homeserver — `matrix.beeper.com` is the only durable signal (Story 2.4).
 *
 * `homeserverUrl` is the SDK-resolved homeserver (after `.well-known`
 * discovery), so this check assumes Beeper keeps resolving to
 * `matrix.beeper.com`; were Beeper to redirect its well-known to a different
 * host, Beeper accounts would silently stop being recognized. A durable
 * account-kind tag would remove that coupling (see deferred-work.md).
 */
export const BEEPER_HOMESERVER_HOST = "matrix.beeper.com";

/**
 * True when {@link account} is a Beeper account, identified by its resolved
 * homeserver host being exactly {@link BEEPER_HOMESERVER_HOST}.
 *
 * The host is matched exactly (not by substring) so a lookalike homeserver such
 * as `matrix.beeper.com.evil.example` does NOT match. A malformed or empty
 * `homeserverUrl` returns `false` and never throws.
 */
export function isBeeperAccount(account: AccountVm): boolean {
  try {
    // `hostname` (not `host`) so an explicit port never defeats the match.
    const host = new URL(account.homeserverUrl).hostname;
    return host.toLowerCase() === BEEPER_HOMESERVER_HOST;
  } catch {
    return false;
  }
}
