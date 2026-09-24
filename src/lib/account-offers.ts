/**
 * What the organisation account offers from the person's other devices (Epic
 * 84, AD-323, UX-DR118): the words and small shapes the three surfaces that
 * show offers share — Settings › Sync (drives), Settings › Bots (endpoints)
 * and the add-account screen (Matrix accounts).
 *
 * An offer is a description, never a thing already added: a drive still needs
 * a folder on this device, a Matrix account a sign-in, and an endpoint its key
 * unless it uses the account. Nothing here adds anything by itself, and every
 * block is absent while its list is empty (AD-27) — an install with no account,
 * or with nothing on its other devices, looks exactly as it did before.
 */

/** The heading of every offers block. */
export const ACCOUNT_OFFERS_TITLE = "From your account";

/**
 * The host an endpoint or homeserver URL points at, which is the part a row
 * has room for. A value that does not parse as a URL is shown as it is,
 * rather than blanked: it is still the thing the other device holds.
 */
export function offerUrlHost(url: string): string {
  const trimmed = url.trim();
  try {
    const { host } = new URL(trimmed);
    return host === "" ? trimmed : host;
  } catch {
    return trimmed;
  }
}
