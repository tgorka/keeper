/**
 * Shared Bridges-surface constants and presentation helpers (Story 6.2, FR-25,
 * UX-DR13).
 *
 * The single source of truth for the companion-stack docs link and the discovery
 * status → label map, so the pane and card render identical copy without either
 * inventing status text. Risk-tier badges and ack copy still come only from the
 * backend catalog data — nothing here duplicates that.
 */
import type { BridgeLoginPhase, BridgeStatus } from "@/lib/ipc/client";

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

/**
 * The live state word shown in the bridge login Sheet for each
 * {@link BridgeLoginPhase} (Story 6.3, FR-26). One transport-agnostic word per
 * phase so the stepper reads identically whichever transport powered the login.
 */
export const BRIDGE_LOGIN_PHASE_LABEL: Record<BridgeLoginPhase, string> = {
  choosingMethod: "Connecting",
  waiting: "Waiting",
  qr: "Scan QR",
  codeEntry: "Enter code",
  success: "Linked ✓",
  failure: "Couldn't connect",
};
