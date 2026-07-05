/**
 * Shared Bridges-surface constants and presentation helpers (Story 6.2, FR-25,
 * UX-DR13).
 *
 * The single source of truth for the companion-stack docs link and the discovery
 * status → label map, so the pane and card render identical copy without either
 * inventing status text. Risk-tier badges and ack copy still come only from the
 * backend catalog data — nothing here duplicates that.
 */
import type { BridgeStatus } from "@/lib/ipc/client";

/**
 * The companion-stack docs link surfaced in the "No bridges found" empty state
 * (UX-DR13). No hosted companion stack exists yet — this points at keeper's real
 * repository docs and is the single constant to repoint when a dedicated
 * companion-stack page lands. Never a fabricated hosted service.
 */
export const COMPANION_STACK_DOCS_URL = "https://github.com/tgorka/keeper/tree/main/docs";

/**
 * The short status word shown on a bridge card for each discovered
 * {@link BridgeStatus}. Setup/login state only — live connection health
 * (Connected / Disconnected in the 6.5 sense) is a separate placeholder dot.
 */
export const BRIDGE_STATUS_LABEL: Record<BridgeStatus, string> = {
  loggedIn: "Connected",
  notLoggedIn: "Action needed",
  configured: "Not set up",
};
